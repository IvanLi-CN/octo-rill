use std::sync::Arc;

use anyhow::Context;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect},
};
use oauth2::{AuthorizationCode, CsrfToken, Scope, TokenResponse};
use serde::Deserialize;
use tower_sessions::Session;
use tracing::info;

use crate::{crypto::EncryptedSecret, error::ApiError, github, state::AppState};

const SESSION_KEY_OAUTH_STATE: &str = "oauth_state";
const SESSION_KEY_USER_ID: &str = "user_id";

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
        .add_scope(Scope::new("repo".to_owned()))
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
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let internal_user_id =
        sqlx::query_scalar::<_, i64>(r#"SELECT id FROM users WHERE github_user_id = ?"#)
            .bind(user.id)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?;

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
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

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
