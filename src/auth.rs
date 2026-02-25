use std::sync::Arc;

use anyhow::Context;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use oauth2::{AuthorizationCode, CsrfToken, Scope, TokenResponse};
use serde::Deserialize;
use sqlx::{Sqlite, Transaction};
use tower_sessions::Session;
use tracing::info;

use crate::{crypto::EncryptedSecret, error::ApiError, github, state::AppState};

const SESSION_KEY_OAUTH_STATE: &str = "oauth_state";
const SESSION_KEY_USER_ID: &str = "user_id";

async fn promote_first_admin(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    now: &str,
) -> Result<bool, ApiError> {
    let updated = sqlx::query(
        r#"
        UPDATE users
        SET is_admin = 1, updated_at = ?
        WHERE id = ?
          AND NOT EXISTS (SELECT 1 FROM users WHERE is_admin = 1)
        "#,
    )
    .bind(now)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(updated.rows_affected() > 0)
}

pub async fn github_login(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<impl IntoResponse, ApiError> {
    let (auth_url, csrf_token) = state
        .oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("read:user".to_owned()))
        .add_scope(Scope::new("user:email".to_owned()))
        .add_scope(Scope::new("notifications".to_owned()))
        .url();

    session
        .insert(SESSION_KEY_OAUTH_STATE, csrf_token.secret())
        .await
        .map_err(ApiError::internal)?;

    Ok(Redirect::to(auth_url.as_str()))
}

#[derive(Debug, Deserialize)]
pub struct GitHubCallbackQuery {
    pub code: String,
    pub state: String,
}

pub async fn github_callback(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<GitHubCallbackQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let expected_state = session
        .get::<String>(SESSION_KEY_OAUTH_STATE)
        .await
        .map_err(ApiError::internal)?;

    if expected_state.as_deref() != Some(query.state.as_str()) {
        return Err(ApiError::bad_request("invalid oauth state"));
    }

    session
        .remove::<String>(SESSION_KEY_OAUTH_STATE)
        .await
        .map_err(ApiError::internal)?;

    let token = state
        .oauth
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(&state.http)
        .await
        .context("github oauth token exchange failed")
        .map_err(ApiError::internal)?;

    let access_token = token.access_token().secret().to_owned();
    let scopes = token
        .scopes()
        .map(|scopes| {
            scopes
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let user = github::fetch_user(&state.http, &access_token)
        .await
        .context("failed to fetch github user")
        .map_err(ApiError::internal)?;
    let email = if user.email.is_some() {
        user.email.clone()
    } else {
        github::fetch_primary_email(&state.http, &access_token)
            .await
            .ok()
            .flatten()
    };

    let now = chrono::Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;

    sqlx::query(
        r#"
        INSERT INTO users (github_user_id, login, name, avatar_url, email, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(github_user_id) DO UPDATE SET
          login = excluded.login,
          name = excluded.name,
          avatar_url = excluded.avatar_url,
          email = excluded.email,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(user.id)
    .bind(&user.login)
    .bind(user.name.as_deref())
    .bind(user.avatar_url.as_deref())
    .bind(email.as_deref())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    #[derive(Debug, sqlx::FromRow)]
    struct AuthUserRow {
        id: i64,
        is_admin: i64,
        is_disabled: i64,
    }

    let mut auth_user_row = sqlx::query_as::<_, AuthUserRow>(
        r#"
        SELECT id, is_admin, is_disabled
        FROM users
        WHERE github_user_id = ?
        "#,
    )
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    if auth_user_row.is_admin == 0
        && promote_first_admin(&mut tx, auth_user_row.id, now.as_str()).await?
    {
        auth_user_row.is_admin = 1;
    }

    if auth_user_row.is_disabled != 0 {
        let _ = tx.rollback().await;
        session.clear().await;
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }

    let internal_user_id = auth_user_row.id;

    let EncryptedSecret { ciphertext, nonce } = state
        .encryption_key
        .encrypt_str(&access_token)
        .map_err(ApiError::internal)?;

    sqlx::query(
        r#"
        INSERT INTO user_tokens (user_id, access_token_ciphertext, access_token_nonce, scopes, updated_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
          access_token_ciphertext = excluded.access_token_ciphertext,
          access_token_nonce = excluded.access_token_nonce,
          scopes = excluded.scopes,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(internal_user_id)
    .bind(ciphertext)
    .bind(nonce)
    .bind(&scopes)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;

    session
        .insert(SESSION_KEY_USER_ID, internal_user_id)
        .await
        .map_err(ApiError::internal)?;

    info!(user_id = internal_user_id, github_user_id = user.id, login = %user.login, "login ok");

    Ok(Redirect::to(state.config.public_base_url.as_str()))
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<impl IntoResponse, ApiError> {
    session.clear().await;
    Ok(Redirect::to(state.config.public_base_url.as_str()))
}

#[cfg(test)]
mod tests {
    use super::promote_first_admin;
    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite memory db");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    #[tokio::test]
    async fn promote_first_admin_assigns_only_once() {
        let pool = setup_pool().await;
        let now = "2026-02-25T12:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, is_admin, is_disabled, created_at, updated_at)
            VALUES
              (1, 101, 'first', 0, 0, '2026-02-25T10:00:00Z', '2026-02-25T10:00:00Z'),
              (2, 102, 'second', 0, 0, '2026-02-25T11:00:00Z', '2026-02-25T11:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed users");

        let mut tx1 = pool.begin().await.expect("begin tx1");
        let promoted = promote_first_admin(&mut tx1, 1, now)
            .await
            .expect("promote first user");
        tx1.commit().await.expect("commit tx1");
        assert!(promoted);

        let mut tx2 = pool.begin().await.expect("begin tx2");
        let promoted_again = promote_first_admin(&mut tx2, 2, now)
            .await
            .expect("promote second user");
        tx2.commit().await.expect("commit tx2");
        assert!(!promoted_again);

        let admins: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM users WHERE is_admin = 1"#)
            .fetch_one(&pool)
            .await
            .expect("count admins");
        assert_eq!(admins, 1);

        let first_is_admin: i64 = sqlx::query_scalar(r#"SELECT is_admin FROM users WHERE id = 1"#)
            .fetch_one(&pool)
            .await
            .expect("query first user admin status");
        assert_eq!(first_is_admin, 1);
    }
}
