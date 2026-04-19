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

use crate::{
    api::require_active_user_id, briefs, config::AppConfig, crypto::EncryptedSecret,
    error::ApiError, github, linuxdo, local_id, state::AppState,
};

const SESSION_KEY_OAUTH_STATE: &str = "oauth_state";
const SESSION_KEY_LINUXDO_OAUTH_STATE: &str = "linuxdo_oauth_state";
const SESSION_KEY_USER_ID: &str = "user_id";

async fn promote_first_admin(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
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

async fn upsert_github_user(
    tx: &mut Transaction<'_, Sqlite>,
    user: &github::GitHubUser,
    email: Option<&str>,
    now: &str,
    default_daily_brief_local_time: chrono::NaiveTime,
    default_daily_brief_time_zone: &str,
) -> Result<(), ApiError> {
    #[derive(Debug, sqlx::FromRow)]
    struct ExistingBriefPreferenceRow {
        daily_brief_local_time: Option<String>,
        daily_brief_time_zone: Option<String>,
        daily_brief_utc_time: String,
    }

    let existing = sqlx::query_as::<_, ExistingBriefPreferenceRow>(
        r#"
        SELECT daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time
        FROM users
        WHERE github_user_id = ?
        LIMIT 1
        "#,
    )
    .bind(user.id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    let existing_local_time = existing
        .as_ref()
        .and_then(|row| row.daily_brief_local_time.as_deref())
        .and_then(|value| briefs::parse_daily_brief_local_time(value).ok())
        .map(briefs::format_daily_brief_local_time);
    let existing_time_zone = existing
        .as_ref()
        .and_then(|row| row.daily_brief_time_zone.as_deref())
        .and_then(briefs::canonical_supported_time_zone);
    let (
        excluded_daily_brief_utc_time,
        excluded_daily_brief_local_time,
        excluded_daily_brief_time_zone,
    ) = if let (Some(local_time), Some(time_zone)) = (existing_local_time, existing_time_zone) {
        (
            existing
                .as_ref()
                .map(|row| row.daily_brief_utc_time.clone())
                .unwrap_or_else(|| "00:00".to_owned()),
            Some(local_time),
            Some(time_zone),
        )
    } else {
        let derived_preferences = existing.as_ref().map_or(
            briefs::DailyBriefPreferences {
                local_time: default_daily_brief_local_time,
                time_zone: default_daily_brief_time_zone.to_owned(),
            },
            |row| {
                briefs::derive_daily_brief_preferences_with_defaults(
                    default_daily_brief_local_time,
                    default_daily_brief_time_zone,
                    row.daily_brief_local_time.as_deref(),
                    row.daily_brief_time_zone.as_deref(),
                    Some(row.daily_brief_utc_time.as_str()),
                    chrono::Utc::now(),
                )
            },
        );
        let derived_local_time =
            briefs::format_daily_brief_local_time(derived_preferences.local_time);
        let derived_legacy_utc_time = briefs::format_legacy_daily_brief_utc_time(
            derived_preferences.local_time,
            &derived_preferences.time_zone,
        )
        .map_err(ApiError::internal)?;
        let enabled_hours = briefs::load_enabled_daily_brief_scheduler_hours(&mut **tx)
            .await
            .map_err(ApiError::internal)?;
        let missing_hours = briefs::missing_daily_brief_scheduler_hours(
            derived_preferences.local_time,
            &derived_preferences.time_zone,
            &enabled_hours,
        )
        .map_err(ApiError::internal)?;
        if !missing_hours.is_empty() {
            let missing_hours = missing_hours
                .into_iter()
                .map(|hour| format!("{hour:02}:00Z"))
                .collect::<Vec<_>>()
                .join(", ");
            tracing::warn!(
                github_user_id = user.id,
                login = %user.login,
                local_time = %derived_local_time,
                time_zone = %derived_preferences.time_zone,
                missing_enabled_utc_slots = %missing_hours,
                "skipping daily brief profile backfill during OAuth login because required scheduler slots are disabled"
            );
            (derived_legacy_utc_time, None, None)
        } else {
            (
                derived_legacy_utc_time,
                Some(derived_local_time),
                Some(derived_preferences.time_zone),
            )
        }
    };

    sqlx::query(
        r#"
        INSERT INTO users (
          id, github_user_id, login, name, avatar_url, email,
          created_at, updated_at, last_active_at,
          daily_brief_utc_time, daily_brief_local_time, daily_brief_time_zone
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(github_user_id) DO UPDATE SET
          login = excluded.login,
          name = excluded.name,
          avatar_url = excluded.avatar_url,
          email = excluded.email,
          updated_at = excluded.updated_at,
          last_active_at = excluded.last_active_at,
          daily_brief_utc_time = excluded.daily_brief_utc_time,
          daily_brief_local_time = excluded.daily_brief_local_time,
          daily_brief_time_zone = excluded.daily_brief_time_zone
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(user.id)
    .bind(&user.login)
    .bind(user.name.as_deref())
    .bind(user.avatar_url.as_deref())
    .bind(email)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(excluded_daily_brief_utc_time)
    .bind(excluded_daily_brief_local_time)
    .bind(excluded_daily_brief_time_zone)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(())
}

pub async fn github_login(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<impl IntoResponse, ApiError> {
    let (auth_url, csrf_token) = state
        .github_oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("read:user".to_owned()))
        .add_scope(Scope::new("user:email".to_owned()))
        .add_scope(Scope::new("notifications".to_owned()))
        .add_scope(Scope::new("public_repo".to_owned()))
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
        .github_oauth
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
    let default_daily_brief_local_time =
        crate::briefs::default_daily_brief_local_time(&state.config);
    let default_daily_brief_time_zone =
        crate::briefs::default_daily_brief_time_zone(&state.config).to_owned();
    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;

    upsert_github_user(
        &mut tx,
        &user,
        email.as_deref(),
        now.as_str(),
        default_daily_brief_local_time,
        default_daily_brief_time_zone.as_str(),
    )
    .await?;

    #[derive(Debug, sqlx::FromRow)]
    struct AuthUserRow {
        id: String,
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
        && promote_first_admin(&mut tx, &auth_user_row.id, now.as_str()).await?
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

    let internal_user_id = auth_user_row.id.clone();

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
    .bind(internal_user_id.as_str())
    .bind(ciphertext)
    .bind(nonce)
    .bind(&scopes)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;

    session
        .insert(SESSION_KEY_USER_ID, internal_user_id.clone())
        .await
        .map_err(ApiError::internal)?;

    info!(user_id = internal_user_id, github_user_id = user.id, login = %user.login, "login ok");

    Ok(Redirect::to(state.config.public_base_url.as_str()))
}

#[derive(Debug, Deserialize)]
pub struct LinuxDoCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, sqlx::FromRow)]
struct LinuxDoOwnerRow {
    user_id: String,
}

fn settings_redirect(config: &AppConfig, status: Option<&str>) -> String {
    let mut url = config
        .public_base_url
        .join("settings")
        .expect("settings route should join public base url");
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("section", "linuxdo");
        if let Some(status) = status {
            pairs.append_pair("linuxdo", status);
        }
    }
    url.into()
}

async fn upsert_linuxdo_connection(
    state: &AppState,
    user_id: &str,
    user: &linuxdo::LinuxDoUser,
) -> Result<(), ApiError> {
    if let Some(owner) = sqlx::query_as::<_, LinuxDoOwnerRow>(
        r#"
        SELECT user_id
        FROM linuxdo_connections
        WHERE linuxdo_user_id = ?
        LIMIT 1
        "#,
    )
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
        && owner.user_id != user_id
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "linuxdo_already_bound",
            "linuxdo account already bound to another user",
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO linuxdo_connections (
          user_id, linuxdo_user_id, username, name, avatar_url, trust_level,
          active, silenced, linked_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
          linuxdo_user_id = excluded.linuxdo_user_id,
          username = excluded.username,
          name = excluded.name,
          avatar_url = excluded.avatar_url,
          trust_level = excluded.trust_level,
          active = excluded.active,
          silenced = excluded.silenced,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(user_id)
    .bind(user.id)
    .bind(user.username.as_str())
    .bind(linuxdo::normalize_display_name(user.name.as_deref()))
    .bind(linuxdo::resolve_avatar_url(&user.avatar_template, 96))
    .bind(user.trust_level)
    .bind(user.active)
    .bind(user.silenced)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(())
}

pub async fn linuxdo_login(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<impl IntoResponse, ApiError> {
    let _user_id = require_active_user_id(state.as_ref(), &session).await?;
    let Some(client) = state.linuxdo_oauth.as_ref() else {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "linuxdo_oauth_not_configured",
            "linuxdo oauth is not configured",
        ));
    };

    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("user".to_owned()))
        .url();

    session
        .insert(SESSION_KEY_LINUXDO_OAUTH_STATE, csrf_token.secret())
        .await
        .map_err(ApiError::internal)?;

    Ok(Redirect::to(auth_url.as_str()))
}

pub async fn linuxdo_callback(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<LinuxDoCallbackQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let Some(client) = state.linuxdo_oauth.as_ref() else {
        return Ok(Redirect::to(
            settings_redirect(&state.config, Some("not_configured")).as_str(),
        ));
    };

    let expected_state = session
        .get::<String>(SESSION_KEY_LINUXDO_OAUTH_STATE)
        .await
        .map_err(ApiError::internal)?;
    let _ = session
        .remove::<String>(SESSION_KEY_LINUXDO_OAUTH_STATE)
        .await;
    if expected_state.as_deref() != Some(query.state.as_str()) {
        return Ok(Redirect::to(
            settings_redirect(&state.config, Some("state_mismatch")).as_str(),
        ));
    }

    let token = match client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(&state.http)
        .await
    {
        Ok(token) => token,
        Err(err) => {
            tracing::warn!(error = %err, "linuxdo oauth token exchange failed");
            return Ok(Redirect::to(
                settings_redirect(&state.config, Some("exchange_failed")).as_str(),
            ));
        }
    };

    let access_token = token.access_token().secret();
    let linuxdo_user = match linuxdo::fetch_user(&state.http, access_token).await {
        Ok(user) => user,
        Err(err) => {
            tracing::warn!(error = %err, "failed to fetch linuxdo user");
            return Ok(Redirect::to(
                settings_redirect(&state.config, Some("fetch_user_failed")).as_str(),
            ));
        }
    };

    match upsert_linuxdo_connection(state.as_ref(), &user_id, &linuxdo_user).await {
        Ok(()) => Ok(Redirect::to(
            settings_redirect(&state.config, Some("connected")).as_str(),
        )),
        Err(err) if err.code() == "linuxdo_already_bound" => Ok(Redirect::to(
            settings_redirect(&state.config, Some("already_bound")).as_str(),
        )),
        Err(err) => {
            tracing::warn!(error = %err, "failed to persist linuxdo binding");
            Ok(Redirect::to(
                settings_redirect(&state.config, Some("save_failed")).as_str(),
            ))
        }
    }
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
    use super::{promote_first_admin, upsert_github_user};
    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };

    async fn setup_pool() -> SqlitePool {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
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
              ('user-first-id', 101, 'first', 0, 0, '2026-02-25T10:00:00Z', '2026-02-25T10:00:00Z'),
              (2, 102, 'second', 0, 0, '2026-02-25T11:00:00Z', '2026-02-25T11:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed users");

        let mut tx1 = pool.begin().await.expect("begin tx1");
        let promoted = promote_first_admin(&mut tx1, "user-first-id", now)
            .await
            .expect("promote first user");
        tx1.commit().await.expect("commit tx1");
        assert!(promoted);

        let mut tx2 = pool.begin().await.expect("begin tx2");
        let promoted_again = promote_first_admin(&mut tx2, "user-second-id", now)
            .await
            .expect("promote second user");
        tx2.commit().await.expect("commit tx2");
        assert!(!promoted_again);

        let admins: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM users WHERE is_admin = 1"#)
            .fetch_one(&pool)
            .await
            .expect("count admins");
        assert_eq!(admins, 1);

        let first_is_admin: i64 =
            sqlx::query_scalar(r#"SELECT is_admin FROM users WHERE id = 'user-first-id'"#)
                .fetch_one(&pool)
                .await
                .expect("query first user admin status");
        assert_eq!(first_is_admin, 1);
    }

    #[tokio::test]
    async fn upsert_github_user_backfills_missing_brief_preferences_for_existing_accounts() {
        let pool = setup_pool().await;

        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login, daily_brief_utc_time, created_at, updated_at
            )
            VALUES (
              'user-existing-id', 101, 'existing', '13:00',
              '2026-02-25T10:00:00Z', '2026-02-25T10:00:00Z'
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed legacy user");

        let github_user = crate::github::GitHubUser {
            id: 101,
            login: "existing".to_owned(),
            name: Some("Existing User".to_owned()),
            avatar_url: Some("https://avatars.example/existing.png".to_owned()),
            email: None,
        };

        let mut tx = pool.begin().await.expect("begin tx");
        upsert_github_user(
            &mut tx,
            &github_user,
            Some("existing@example.com"),
            "2026-07-25T12:00:00Z",
            chrono::NaiveTime::from_hms_opt(8, 0, 0).expect("valid default time"),
            "America/New_York",
        )
        .await
        .expect("upsert github user");
        tx.commit().await.expect("commit tx");

        let row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            r#"
            SELECT daily_brief_local_time, daily_brief_time_zone
            FROM users
            WHERE github_user_id = 101
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("load persisted preferences");

        assert_eq!(row.0.as_deref(), Some("08:00"));
        assert_eq!(row.1.as_deref(), Some("America/New_York"));
    }

    #[tokio::test]
    async fn upsert_github_user_skips_unschedulable_brief_preference_backfill() {
        let pool = setup_pool().await;
        sqlx::query(
            r#"
            UPDATE daily_brief_hour_slots
            SET enabled = CASE WHEN hour_utc = 13 THEN 0 ELSE enabled END
            "#,
        )
        .execute(&pool)
        .await
        .expect("disable required slot");

        let github_user = crate::github::GitHubUser {
            id: 102,
            login: "new-user".to_owned(),
            name: Some("New User".to_owned()),
            avatar_url: Some("https://avatars.example/new-user.png".to_owned()),
            email: None,
        };

        let mut tx = pool.begin().await.expect("begin tx");
        upsert_github_user(
            &mut tx,
            &github_user,
            Some("new-user@example.com"),
            "2026-07-25T12:00:00Z",
            chrono::NaiveTime::from_hms_opt(8, 0, 0).expect("valid default time"),
            "America/New_York",
        )
        .await
        .expect("allow login when derived brief preferences are unschedulable");
        tx.commit().await.expect("commit tx");

        let row = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            r#"
            SELECT daily_brief_utc_time, daily_brief_local_time, daily_brief_time_zone
            FROM users
            WHERE github_user_id = 102
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("load new user profile");

        assert_eq!(row.0, "13:00");
        assert_eq!(row.1, None);
        assert_eq!(row.2, None);
    }

    #[tokio::test]
    async fn upsert_github_user_preserves_existing_unschedulable_brief_preferences() {
        let pool = setup_pool().await;
        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login,
              daily_brief_utc_time, daily_brief_local_time, daily_brief_time_zone,
              created_at, updated_at
            )
            VALUES (
              'user-existing-unschedulable', 103, 'existing-unschedulable',
              '13:00', '08:00', 'America/New_York',
              '2026-02-25T10:00:00Z', '2026-02-25T10:00:00Z'
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed existing user");
        sqlx::query(
            r#"
            UPDATE daily_brief_hour_slots
            SET enabled = CASE WHEN hour_utc = 13 THEN 0 ELSE enabled END
            "#,
        )
        .execute(&pool)
        .await
        .expect("disable required slot");

        let github_user = crate::github::GitHubUser {
            id: 103,
            login: "existing-unschedulable".to_owned(),
            name: Some("Existing Unschedulable".to_owned()),
            avatar_url: Some("https://avatars.example/existing-unschedulable.png".to_owned()),
            email: None,
        };

        let mut tx = pool.begin().await.expect("begin tx");
        upsert_github_user(
            &mut tx,
            &github_user,
            Some("existing-unschedulable@example.com"),
            "2026-07-25T12:00:00Z",
            chrono::NaiveTime::from_hms_opt(8, 0, 0).expect("valid default time"),
            "America/New_York",
        )
        .await
        .expect("preserve existing unschedulable profile");
        tx.commit().await.expect("commit tx");

        let row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            r#"
            SELECT daily_brief_local_time, daily_brief_time_zone
            FROM users
            WHERE github_user_id = 103
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("load persisted preferences");

        assert_eq!(row.0.as_deref(), Some("08:00"));
        assert_eq!(row.1.as_deref(), Some("America/New_York"));
    }
}
