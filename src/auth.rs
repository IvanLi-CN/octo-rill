use std::sync::Arc;

use anyhow::Context;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use oauth2::{AuthorizationCode, CsrfToken, Scope, TokenResponse};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};
use tower_sessions::Session;
use tracing::info;
use webauthn_rs::prelude::{
    CreationChallengeResponse, DiscoverableKey, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse,
};

use crate::{
    api::require_active_user_id,
    briefs,
    config::AppConfig,
    crypto::EncryptedSecret,
    error::ApiError,
    github, linuxdo, local_id,
    passkeys::{
        AttachPendingPasskeyOutcome, PendingPasskeyAuthenticationSession,
        PendingPasskeyCredentialSession, PendingPasskeyRegistrationMode,
        PendingPasskeyRegistrationSession, SESSION_KEY_PENDING_PASSKEY_AUTHENTICATION,
        SESSION_KEY_PENDING_PASSKEY_CREDENTIAL, SESSION_KEY_PENDING_PASSKEY_REGISTRATION,
        attach_pending_passkey_to_user, build_passkey_label, encode_credential_id,
        generate_user_handle_uuid, load_passkeys, load_user_id_by_user_handle,
        mark_passkey_authentication_used, parse_user_handle_uuid,
        pending_passkey_authentication_is_expired, pending_passkey_bind_is_expired,
        pending_passkey_registration_is_expired, user_has_github_connection,
    },
    state::AppState,
};

const SESSION_KEY_OAUTH_STATE: &str = "oauth_state";
const SESSION_KEY_GITHUB_OAUTH_MODE: &str = "github_oauth_mode";
const SESSION_KEY_LINUXDO_OAUTH_STATE: &str = "linuxdo_oauth_state";
const SESSION_KEY_PENDING_LINUXDO: &str = "pending_linuxdo";
const SESSION_KEY_USER_ID: &str = "user_id";

const GITHUB_OAUTH_MODE_LOGIN: &str = "login";
const GITHUB_OAUTH_MODE_CONNECT: &str = "connect";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingLinuxDoSession {
    linuxdo_user_id: i64,
    username: String,
    name: Option<String>,
    avatar_url: String,
    trust_level: i64,
    active: bool,
    silenced: bool,
}

impl PendingLinuxDoSession {
    fn from_linuxdo_user(user: &linuxdo::LinuxDoUser) -> Self {
        Self {
            linuxdo_user_id: user.id,
            username: user.username.clone(),
            name: linuxdo::normalize_display_name(user.name.as_deref()),
            avatar_url: linuxdo::resolve_avatar_url(&user.avatar_template, 96),
            trust_level: user.trust_level,
            active: user.active,
            silenced: user.silenced,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct GitHubConnectionOwnerRow {
    id: String,
    user_id: String,
}

#[derive(Debug, sqlx::FromRow)]
struct AuthUserRow {
    id: String,
    is_admin: i64,
    is_disabled: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ExistingBriefPreferenceRow {
    daily_brief_local_time: Option<String>,
    daily_brief_time_zone: Option<String>,
    daily_brief_utc_time: String,
}

#[derive(Debug, sqlx::FromRow)]
struct LinuxDoOwnerRow {
    user_id: String,
}

#[derive(Debug, sqlx::FromRow)]
struct UserLinuxDoBindingRow {
    linuxdo_user_id: i64,
}

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

async fn load_existing_brief_preferences(
    tx: &mut Transaction<'_, Sqlite>,
    existing_user_id: Option<&str>,
    github_user_id: i64,
) -> Result<Option<ExistingBriefPreferenceRow>, ApiError> {
    let query = if existing_user_id.is_some() {
        r#"
        SELECT daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time
        FROM users
        WHERE id = ?
        LIMIT 1
        "#
    } else {
        r#"
        SELECT daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time
        FROM users
        WHERE github_user_id = ?
        LIMIT 1
        "#
    };

    let mut db_query = sqlx::query_as::<_, ExistingBriefPreferenceRow>(query);
    if let Some(user_id) = existing_user_id {
        db_query = db_query.bind(user_id);
    } else {
        db_query = db_query.bind(github_user_id);
    }

    db_query
        .fetch_optional(&mut **tx)
        .await
        .map_err(ApiError::internal)
}

async fn upsert_github_user_for_account(
    tx: &mut Transaction<'_, Sqlite>,
    existing_user_id: Option<&str>,
    user: &github::GitHubUser,
    email: Option<&str>,
    now: &str,
    default_daily_brief_local_time: chrono::NaiveTime,
    default_daily_brief_time_zone: &str,
) -> Result<String, ApiError> {
    let existing = load_existing_brief_preferences(tx, existing_user_id, user.id).await?;

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

    if let Some(user_id) = existing_user_id {
        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login, name, avatar_url, email,
              created_at, updated_at, last_active_at,
              daily_brief_utc_time, daily_brief_local_time, daily_brief_time_zone
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
              github_user_id = excluded.github_user_id,
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
        .bind(user_id)
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
        return Ok(user_id.to_owned());
    }

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

    let user_id = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM users
        WHERE github_user_id = ?
        LIMIT 1
        "#,
    )
    .bind(user.id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(user_id)
}

#[cfg(test)]
async fn upsert_github_user(
    tx: &mut Transaction<'_, Sqlite>,
    user: &github::GitHubUser,
    email: Option<&str>,
    now: &str,
    default_daily_brief_local_time: chrono::NaiveTime,
    default_daily_brief_time_zone: &str,
) -> Result<(), ApiError> {
    let _ = upsert_github_user_for_account(
        tx,
        None,
        user,
        email,
        now,
        default_daily_brief_local_time,
        default_daily_brief_time_zone,
    )
    .await?;
    Ok(())
}

async fn load_github_connection_owner(
    tx: &mut Transaction<'_, Sqlite>,
    github_user_id: i64,
) -> Result<Option<GitHubConnectionOwnerRow>, ApiError> {
    sqlx::query_as::<_, GitHubConnectionOwnerRow>(
        r#"
        SELECT id, user_id
        FROM github_connections
        WHERE github_user_id = ?
        LIMIT 1
        "#,
    )
    .bind(github_user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

#[allow(clippy::too_many_arguments)]
async fn upsert_github_connection(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    user: &github::GitHubUser,
    email: Option<&str>,
    access_token: &str,
    scopes: &str,
    now: &str,
    encryption_key: &crate::crypto::EncryptionKey,
) -> Result<(), ApiError> {
    let owner = load_github_connection_owner(tx, user.id).await?;
    if let Some(owner) = owner.as_ref()
        && owner.user_id != user_id
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "github_already_bound",
            "github account already bound to another user",
        ));
    }

    let existing_id = owner.as_ref().map(|row| row.id.clone());

    let EncryptedSecret { ciphertext, nonce } = encryption_key
        .encrypt_str(access_token)
        .map_err(ApiError::internal)?;
    let connection_id = existing_id.unwrap_or_else(local_id::generate_local_id);

    sqlx::query(
        r#"
        INSERT INTO github_connections (
          id,
          user_id,
          github_user_id,
          login,
          name,
          avatar_url,
          email,
          access_token_ciphertext,
          access_token_nonce,
          scopes,
          linked_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
          github_user_id = excluded.github_user_id,
          login = excluded.login,
          name = excluded.name,
          avatar_url = excluded.avatar_url,
          email = excluded.email,
          access_token_ciphertext = excluded.access_token_ciphertext,
          access_token_nonce = excluded.access_token_nonce,
          scopes = excluded.scopes,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(&connection_id)
    .bind(user_id)
    .bind(user.id)
    .bind(&user.login)
    .bind(user.name.as_deref())
    .bind(user.avatar_url.as_deref())
    .bind(email)
    .bind(ciphertext)
    .bind(nonce)
    .bind(scopes)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(())
}

async fn load_auth_user_row(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
) -> Result<AuthUserRow, ApiError> {
    sqlx::query_as::<_, AuthUserRow>(
        r#"
        SELECT id, is_admin, is_disabled
        FROM users
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

fn settings_redirect(
    config: &AppConfig,
    section: &str,
    github_status: Option<&str>,
    linuxdo_status: Option<&str>,
    passkey_status: Option<&str>,
) -> String {
    let mut url = config
        .public_base_url
        .join("settings")
        .expect("settings route should join public base url");
    {
        let mut pairs = url.query_pairs_mut();
        if section != "linuxdo" {
            pairs.append_pair("section", section);
        }
        if let Some(status) = github_status {
            pairs.append_pair("github", status);
        }
        if let Some(status) = linuxdo_status {
            pairs.append_pair("linuxdo", status);
        }
        if let Some(status) = passkey_status {
            pairs.append_pair("passkey", status);
        }
    }
    url.into()
}

fn bind_github_redirect(
    config: &AppConfig,
    linuxdo_status: Option<&str>,
    passkey_status: Option<&str>,
) -> String {
    let mut url = config
        .public_base_url
        .join("bind/github")
        .expect("bind route should join public base url");
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(status) = linuxdo_status {
            pairs.append_pair("linuxdo", status);
        }
        if let Some(status) = passkey_status {
            pairs.append_pair("passkey", status);
        }
    }
    url.into()
}

fn provisional_passkey_user_name(user_handle_uuid: &str) -> String {
    let suffix = user_handle_uuid.chars().take(8).collect::<String>();
    format!("passkey-{suffix}")
}

async fn clear_pending_passkey_registration(session: &Session) {
    let _ = session
        .remove::<PendingPasskeyRegistrationSession>(SESSION_KEY_PENDING_PASSKEY_REGISTRATION)
        .await;
}

async fn clear_pending_passkey_authentication(session: &Session) {
    let _ = session
        .remove::<PendingPasskeyAuthenticationSession>(SESSION_KEY_PENDING_PASSKEY_AUTHENTICATION)
        .await;
}

async fn upsert_linuxdo_connection_record(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    pending: &PendingLinuxDoSession,
    now: &str,
) -> Result<(), ApiError> {
    if let Some(owner) = sqlx::query_as::<_, LinuxDoOwnerRow>(
        r#"
        SELECT user_id
        FROM linuxdo_connections
        WHERE linuxdo_user_id = ?
        LIMIT 1
        "#,
    )
    .bind(pending.linuxdo_user_id)
    .fetch_optional(&mut **tx)
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

    if let Some(existing) = sqlx::query_as::<_, UserLinuxDoBindingRow>(
        r#"
        SELECT linuxdo_user_id
        FROM linuxdo_connections
        WHERE user_id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?
        && existing.linuxdo_user_id != pending.linuxdo_user_id
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "linuxdo_account_conflict",
            "account already bound to another linuxdo user",
        ));
    }

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
    .bind(pending.linuxdo_user_id)
    .bind(pending.username.as_str())
    .bind(pending.name.as_deref())
    .bind(pending.avatar_url.as_str())
    .bind(pending.trust_level)
    .bind(pending.active)
    .bind(pending.silenced)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(())
}

async fn finalize_github_auth(
    state: &Arc<AppState>,
    session: &Session,
    user: &github::GitHubUser,
    email: Option<&str>,
    access_token: &str,
    scopes: &str,
    requested_mode: Option<String>,
) -> Result<Redirect, ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    let default_daily_brief_local_time =
        crate::briefs::default_daily_brief_local_time(&state.config);
    let default_daily_brief_time_zone =
        crate::briefs::default_daily_brief_time_zone(&state.config).to_owned();

    let pending_linuxdo = session
        .get::<PendingLinuxDoSession>(SESSION_KEY_PENDING_LINUXDO)
        .await
        .map_err(ApiError::internal)?;
    let mut passkey_status_after_login: Option<&'static str> = None;
    let pending_passkey = session
        .get::<PendingPasskeyCredentialSession>(SESSION_KEY_PENDING_PASSKEY_CREDENTIAL)
        .await
        .map_err(ApiError::internal)?;
    let pending_passkey = if let Some(pending_passkey) = pending_passkey {
        if pending_passkey_bind_is_expired(&pending_passkey) {
            let _ = session
                .remove::<PendingPasskeyCredentialSession>(SESSION_KEY_PENDING_PASSKEY_CREDENTIAL)
                .await;
            passkey_status_after_login = Some("expired");
            None
        } else {
            Some(pending_passkey)
        }
    } else {
        None
    };
    let session_user_id = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .map_err(ApiError::internal)?;
    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
    let mut login_user_after_commit: Option<String> = None;
    let mut consume_pending_passkey = false;

    let effective_mode = if requested_mode.as_deref() == Some(GITHUB_OAUTH_MODE_CONNECT)
        || (session_user_id.is_some() && pending_linuxdo.is_none())
    {
        GITHUB_OAUTH_MODE_CONNECT
    } else {
        GITHUB_OAUTH_MODE_LOGIN
    };

    let redirect = match effective_mode {
        GITHUB_OAUTH_MODE_CONNECT => {
            let current_user_id = if let Some(user_id) = session_user_id.as_deref() {
                user_id.to_owned()
            } else {
                let _ = tx.rollback().await;
                return Err(ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "github connect requires an authenticated user",
                ));
            };

            let auth_user_row = load_auth_user_row(&mut tx, current_user_id.as_str()).await?;
            if auth_user_row.is_disabled != 0 {
                let _ = tx.rollback().await;
                session.clear().await;
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "account_disabled",
                    "account is disabled",
                ));
            }

            let connection = upsert_github_connection(
                &mut tx,
                current_user_id.as_str(),
                user,
                email,
                access_token,
                scopes,
                now.as_str(),
                &state.encryption_key,
            )
            .await;

            match connection {
                Ok(()) => settings_redirect(
                    &state.config,
                    "github-accounts",
                    Some("connected"),
                    None,
                    None,
                ),
                Err(err) if err.code() == "github_already_bound" => {
                    let _ = tx.rollback().await;
                    return Ok(Redirect::to(
                        settings_redirect(
                            &state.config,
                            "github-accounts",
                            Some("already_bound"),
                            None,
                            None,
                        )
                        .as_str(),
                    ));
                }
                Err(err) => {
                    let _ = tx.rollback().await;
                    return Err(err);
                }
            }
        }
        _ => {
            let owner = load_github_connection_owner(&mut tx, user.id).await?;
            let target_user_id = if let Some(owner) = owner.as_ref() {
                owner.user_id.clone()
            } else if let Some(existing_user_id) = session_user_id.as_deref() {
                existing_user_id.to_owned()
            } else {
                upsert_github_user_for_account(
                    &mut tx,
                    None,
                    user,
                    email,
                    now.as_str(),
                    default_daily_brief_local_time,
                    default_daily_brief_time_zone.as_str(),
                )
                .await?
            };

            let mut auth_user_row = load_auth_user_row(&mut tx, target_user_id.as_str()).await?;
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

            let connection = upsert_github_connection(
                &mut tx,
                target_user_id.as_str(),
                user,
                email,
                access_token,
                scopes,
                now.as_str(),
                &state.encryption_key,
            )
            .await;

            match connection {
                Ok(()) => {}
                Err(err) if err.code() == "github_already_bound" => {
                    let _ = tx.rollback().await;
                    let redirect = if pending_linuxdo.is_some() {
                        bind_github_redirect(
                            &state.config,
                            Some("github_already_bound"),
                            passkey_status_after_login,
                        )
                    } else {
                        settings_redirect(
                            &state.config,
                            "github-accounts",
                            Some("already_bound"),
                            None,
                            passkey_status_after_login,
                        )
                    };
                    return Ok(Redirect::to(redirect.as_str()));
                }
                Err(err) => {
                    let _ = tx.rollback().await;
                    return Err(err);
                }
            }

            if let Some(pending_passkey) = pending_passkey.as_ref() {
                match attach_pending_passkey_to_user(
                    &mut tx,
                    target_user_id.as_str(),
                    pending_passkey,
                )
                .await?
                {
                    AttachPendingPasskeyOutcome::Attached
                    | AttachPendingPasskeyOutcome::AlreadyExists => {
                        consume_pending_passkey = true;
                    }
                    AttachPendingPasskeyOutcome::AlreadyBound => {
                        consume_pending_passkey = true;
                        passkey_status_after_login = Some("passkey_already_bound");
                    }
                    AttachPendingPasskeyOutcome::RetryRequired => {
                        consume_pending_passkey = true;
                        passkey_status_after_login = Some("passkey_retry_required");
                    }
                }
            }

            if let Some(pending_linuxdo) = pending_linuxdo.as_ref()
                && let Err(err) = upsert_linuxdo_connection_record(
                    &mut tx,
                    target_user_id.as_str(),
                    pending_linuxdo,
                    now.as_str(),
                )
                .await
            {
                let _ = tx.rollback().await;
                return Ok(Redirect::to(
                    bind_github_redirect(
                        &state.config,
                        Some(err.code()),
                        passkey_status_after_login,
                    )
                    .as_str(),
                ));
            }

            login_user_after_commit = Some(target_user_id.clone());
            if pending_linuxdo.is_some() {
                settings_redirect(
                    &state.config,
                    "github-accounts",
                    Some("connected"),
                    Some("connected"),
                    passkey_status_after_login,
                )
            } else if let Some(passkey_status) = passkey_status_after_login {
                settings_redirect(&state.config, "passkeys", None, None, Some(passkey_status))
            } else {
                state.config.public_base_url.to_string()
            }
        }
    };

    tx.commit().await.map_err(ApiError::internal)?;
    if let Some(user_id) = login_user_after_commit {
        session
            .insert(SESSION_KEY_USER_ID, user_id)
            .await
            .map_err(ApiError::internal)?;
    }
    let _ = session
        .remove::<PendingLinuxDoSession>(SESSION_KEY_PENDING_LINUXDO)
        .await;
    if consume_pending_passkey {
        let _ = session
            .remove::<PendingPasskeyCredentialSession>(SESSION_KEY_PENDING_PASSKEY_CREDENTIAL)
            .await;
    }

    info!(login = %user.login, github_user_id = user.id, "github auth ok");
    Ok(Redirect::to(redirect.as_str()))
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
    session
        .insert(SESSION_KEY_GITHUB_OAUTH_MODE, GITHUB_OAUTH_MODE_LOGIN)
        .await
        .map_err(ApiError::internal)?;

    Ok(Redirect::to(auth_url.as_str()))
}

pub async fn github_connect(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<impl IntoResponse, ApiError> {
    let _ = require_active_user_id(state.as_ref(), &session).await?;

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
    session
        .insert(SESSION_KEY_GITHUB_OAUTH_MODE, GITHUB_OAUTH_MODE_CONNECT)
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

    let requested_mode = session
        .get::<String>(SESSION_KEY_GITHUB_OAUTH_MODE)
        .await
        .map_err(ApiError::internal)?;
    session
        .remove::<String>(SESSION_KEY_OAUTH_STATE)
        .await
        .map_err(ApiError::internal)?;
    let _ = session
        .remove::<String>(SESSION_KEY_GITHUB_OAUTH_MODE)
        .await;

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

    finalize_github_auth(
        &state,
        &session,
        &user,
        email.as_deref(),
        access_token.as_str(),
        scopes.as_str(),
        requested_mode,
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct LinuxDoCallbackQuery {
    pub code: String,
    pub state: String,
}

pub async fn linuxdo_login(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<impl IntoResponse, ApiError> {
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
    let session_user_id = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .map_err(ApiError::internal)?;
    let Some(client) = state.linuxdo_oauth.as_ref() else {
        let redirect = if session_user_id.is_some() {
            settings_redirect(&state.config, "linuxdo", None, Some("not_configured"), None)
        } else {
            bind_github_redirect(&state.config, Some("not_configured"), None)
        };
        return Ok(Redirect::to(redirect.as_str()));
    };

    let expected_state = session
        .get::<String>(SESSION_KEY_LINUXDO_OAUTH_STATE)
        .await
        .map_err(ApiError::internal)?;
    let _ = session
        .remove::<String>(SESSION_KEY_LINUXDO_OAUTH_STATE)
        .await;
    if expected_state.as_deref() != Some(query.state.as_str()) {
        let redirect = if session_user_id.is_some() {
            settings_redirect(&state.config, "linuxdo", None, Some("state_mismatch"), None)
        } else {
            bind_github_redirect(&state.config, Some("state_mismatch"), None)
        };
        return Ok(Redirect::to(redirect.as_str()));
    }

    let token = match client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(&state.http)
        .await
    {
        Ok(token) => token,
        Err(err) => {
            tracing::warn!(error = %err, "linuxdo oauth token exchange failed");
            let redirect = if session_user_id.is_some() {
                settings_redirect(
                    &state.config,
                    "linuxdo",
                    None,
                    Some("exchange_failed"),
                    None,
                )
            } else {
                bind_github_redirect(&state.config, Some("exchange_failed"), None)
            };
            return Ok(Redirect::to(redirect.as_str()));
        }
    };

    let access_token = token.access_token().secret();
    let linuxdo_user = match linuxdo::fetch_user(&state.http, access_token).await {
        Ok(user) => user,
        Err(err) => {
            tracing::warn!(error = %err, "failed to fetch linuxdo user");
            let redirect = if session_user_id.is_some() {
                settings_redirect(
                    &state.config,
                    "linuxdo",
                    None,
                    Some("fetch_user_failed"),
                    None,
                )
            } else {
                bind_github_redirect(&state.config, Some("fetch_user_failed"), None)
            };
            return Ok(Redirect::to(redirect.as_str()));
        }
    };

    let pending = PendingLinuxDoSession::from_linuxdo_user(&linuxdo_user);

    if let Some(owner) = sqlx::query_as::<_, LinuxDoOwnerRow>(
        r#"
        SELECT user_id
        FROM linuxdo_connections
        WHERE linuxdo_user_id = ?
        LIMIT 1
        "#,
    )
    .bind(pending.linuxdo_user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
    {
        session
            .insert(SESSION_KEY_USER_ID, owner.user_id)
            .await
            .map_err(ApiError::internal)?;
        let _ = session
            .remove::<PendingLinuxDoSession>(SESSION_KEY_PENDING_LINUXDO)
            .await;
        return Ok(Redirect::to(state.config.public_base_url.as_str()));
    }

    if let Some(user_id) = session_user_id {
        let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
        let auth_user_row = load_auth_user_row(&mut tx, user_id.as_str()).await?;
        if auth_user_row.is_disabled != 0 {
            let _ = tx.rollback().await;
            session.clear().await;
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "account_disabled",
                "account is disabled",
            ));
        }
        match upsert_linuxdo_connection_record(
            &mut tx,
            user_id.as_str(),
            &pending,
            &chrono::Utc::now().to_rfc3339(),
        )
        .await
        {
            Ok(()) => {
                tx.commit().await.map_err(ApiError::internal)?;
                return Ok(Redirect::to(
                    settings_redirect(&state.config, "linuxdo", None, Some("connected"), None)
                        .as_str(),
                ));
            }
            Err(err) => {
                let _ = tx.rollback().await;
                return Ok(Redirect::to(
                    settings_redirect(&state.config, "linuxdo", None, Some(err.code()), None)
                        .as_str(),
                ));
            }
        }
    }

    session
        .insert(SESSION_KEY_PENDING_LINUXDO, pending)
        .await
        .map_err(ApiError::internal)?;
    Ok(Redirect::to(
        bind_github_redirect(&state.config, None, None).as_str(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct PasskeyRegisterVerifyRequest {
    pub credential: RegisterPublicKeyCredential,
}

#[derive(Debug, Deserialize)]
pub struct PasskeyAuthenticateVerifyRequest {
    pub credential: PublicKeyCredential,
}

#[derive(Debug, Serialize)]
pub struct PasskeyRegisterVerifyResponse {
    pub status: String,
    pub next_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PasskeyAuthenticateVerifyResponse {
    pub status: String,
    pub next_path: String,
}

pub async fn passkey_register_options(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<axum::Json<CreationChallengeResponse>, ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    let session_user_id = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .map_err(ApiError::internal)?;

    let (mode, user_handle_uuid, user_name, user_display_name, exclude_credentials) =
        if session_user_id.is_some() {
            let user_id = require_active_user_id(state.as_ref(), &session).await?;
            let user_handle_uuid = sqlx::query_scalar::<_, Option<String>>(
                r#"
                SELECT passkey_user_handle_uuid
                FROM users
                WHERE id = ?
                LIMIT 1
                "#,
            )
            .bind(user_id.as_str())
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?
            .unwrap_or_else(generate_user_handle_uuid);

            let user_name = sqlx::query_scalar::<_, String>(
                r#"
                SELECT login
                FROM github_connections
                WHERE user_id = ?
                ORDER BY linked_at ASC, id ASC
                LIMIT 1
                "#,
            )
            .bind(user_id.as_str())
            .fetch_optional(&state.pool)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    "passkey_github_required",
                    "at least one github connection is required before adding a passkey",
                )
            })?;

            let existing_passkeys = load_passkeys(state.as_ref(), user_id.as_str()).await?;
            let exclude_credentials = existing_passkeys
                .iter()
                .map(|stored| stored.passkey.cred_id().clone())
                .collect::<Vec<_>>();

            (
                PendingPasskeyRegistrationMode::Authenticated { user_id },
                user_handle_uuid.clone(),
                user_name.clone(),
                user_name,
                (!exclude_credentials.is_empty()).then_some(exclude_credentials),
            )
        } else {
            let user_handle_uuid = generate_user_handle_uuid();
            (
                PendingPasskeyRegistrationMode::Onboarding,
                user_handle_uuid.clone(),
                provisional_passkey_user_name(&user_handle_uuid),
                "OctoRill Passkey".to_owned(),
                None,
            )
        };

    let (challenge, registration) = state
        .webauthn
        .start_passkey_registration(
            parse_user_handle_uuid(&user_handle_uuid)?,
            user_name.as_str(),
            user_display_name.as_str(),
            exclude_credentials,
        )
        .map_err(ApiError::internal)?;

    session
        .insert(
            SESSION_KEY_PENDING_PASSKEY_REGISTRATION,
            PendingPasskeyRegistrationSession {
                mode,
                user_handle_uuid,
                label: build_passkey_label(&now),
                started_at: now,
                registration,
            },
        )
        .await
        .map_err(ApiError::internal)?;

    let _ = session
        .remove::<PendingPasskeyCredentialSession>(SESSION_KEY_PENDING_PASSKEY_CREDENTIAL)
        .await;

    Ok(axum::Json(challenge))
}

pub async fn passkey_register_verify(
    State(state): State<Arc<AppState>>,
    session: Session,
    axum::Json(req): axum::Json<PasskeyRegisterVerifyRequest>,
) -> Result<axum::Json<PasskeyRegisterVerifyResponse>, ApiError> {
    let pending = session
        .get::<PendingPasskeyRegistrationSession>(SESSION_KEY_PENDING_PASSKEY_REGISTRATION)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "passkey_registration_missing",
                "passkey registration was not started or has already been consumed",
            )
        })?;

    if pending_passkey_registration_is_expired(&pending) {
        clear_pending_passkey_registration(&session).await;
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "passkey_registration_expired",
            "passkey registration has expired, please try again",
        ));
    }

    clear_pending_passkey_registration(&session).await;
    let passkey = state
        .webauthn
        .finish_passkey_registration(&req.credential, &pending.registration)
        .map_err(|err| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "passkey_registration_failed",
                err.to_string(),
            )
        })?;

    let now = chrono::Utc::now().to_rfc3339();
    let pending_credential = PendingPasskeyCredentialSession {
        user_handle_uuid: pending.user_handle_uuid.clone(),
        label: pending.label.clone(),
        created_at: now,
        passkey,
    };

    match pending.mode {
        PendingPasskeyRegistrationMode::Authenticated { user_id } => {
            let current_user_id = require_active_user_id(state.as_ref(), &session).await?;
            if current_user_id != user_id {
                return Err(ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "session user changed during passkey registration",
                ));
            }

            let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
            match attach_pending_passkey_to_user(&mut tx, user_id.as_str(), &pending_credential)
                .await?
            {
                AttachPendingPasskeyOutcome::Attached
                | AttachPendingPasskeyOutcome::AlreadyExists => {
                    tx.commit().await.map_err(ApiError::internal)?;
                    let _ = session
                        .remove::<PendingPasskeyCredentialSession>(
                            SESSION_KEY_PENDING_PASSKEY_CREDENTIAL,
                        )
                        .await;
                    Ok(axum::Json(PasskeyRegisterVerifyResponse {
                        status: "registered".to_owned(),
                        next_path: Some(settings_redirect(
                            &state.config,
                            "passkeys",
                            None,
                            None,
                            Some("registered"),
                        )),
                    }))
                }
                AttachPendingPasskeyOutcome::AlreadyBound => {
                    let _ = tx.rollback().await;
                    Err(ApiError::new(
                        StatusCode::CONFLICT,
                        "passkey_already_bound",
                        "this passkey is already bound to another account",
                    ))
                }
                AttachPendingPasskeyOutcome::RetryRequired => {
                    let _ = tx.rollback().await;
                    Err(ApiError::new(
                        StatusCode::CONFLICT,
                        "passkey_retry_required",
                        "this account already uses a different passkey handle; please retry from settings",
                    ))
                }
            }
        }
        PendingPasskeyRegistrationMode::Onboarding => {
            session
                .insert(SESSION_KEY_PENDING_PASSKEY_CREDENTIAL, pending_credential)
                .await
                .map_err(ApiError::internal)?;

            Ok(axum::Json(PasskeyRegisterVerifyResponse {
                status: "pending_github_bind".to_owned(),
                next_path: Some(bind_github_redirect(&state.config, None, Some("created"))),
            }))
        }
    }
}

pub async fn passkey_authenticate_options(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<axum::Json<RequestChallengeResponse>, ApiError> {
    let (challenge, authentication) = state
        .webauthn
        .start_discoverable_authentication()
        .map_err(ApiError::internal)?;

    session
        .insert(
            SESSION_KEY_PENDING_PASSKEY_AUTHENTICATION,
            PendingPasskeyAuthenticationSession {
                started_at: chrono::Utc::now().to_rfc3339(),
                authentication,
            },
        )
        .await
        .map_err(ApiError::internal)?;

    Ok(axum::Json(challenge))
}

pub async fn passkey_authenticate_verify(
    State(state): State<Arc<AppState>>,
    session: Session,
    axum::Json(req): axum::Json<PasskeyAuthenticateVerifyRequest>,
) -> Result<axum::Json<PasskeyAuthenticateVerifyResponse>, ApiError> {
    let pending = session
        .get::<PendingPasskeyAuthenticationSession>(SESSION_KEY_PENDING_PASSKEY_AUTHENTICATION)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "passkey_authentication_missing",
                "passkey authentication was not started or has already been consumed",
            )
        })?;

    if pending_passkey_authentication_is_expired(&pending) {
        clear_pending_passkey_authentication(&session).await;
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "passkey_authentication_expired",
            "passkey authentication has expired, please try again",
        ));
    }

    clear_pending_passkey_authentication(&session).await;
    let (user_handle_uuid, credential_id_bytes) = state
        .webauthn
        .identify_discoverable_authentication(&req.credential)
        .map_err(|err| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "passkey_authentication_failed",
                err.to_string(),
            )
        })?;
    let user_handle_uuid = user_handle_uuid.to_string();
    let user_id = load_user_id_by_user_handle(state.as_ref(), user_handle_uuid.as_str())
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "passkey_user_not_found",
                "passkey does not map to an existing account",
            )
        })?;

    if !user_has_github_connection(state.as_ref(), user_id.as_str()).await? {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "passkey_github_required",
            "a github connection is required before passkey login can complete",
        ));
    }

    let mut stored_passkeys = load_passkeys(state.as_ref(), user_id.as_str()).await?;
    if stored_passkeys.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "passkey_not_found",
            "no passkeys are registered for this account",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
    let auth_user_row = load_auth_user_row(&mut tx, user_id.as_str()).await?;
    if auth_user_row.is_disabled != 0 {
        session.clear().await;
        let _ = tx.rollback().await;
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }

    let discoverable_keys = stored_passkeys
        .iter()
        .map(|stored| DiscoverableKey::from(&stored.passkey))
        .collect::<Vec<_>>();
    let authentication_result = state
        .webauthn
        .finish_discoverable_authentication(
            &req.credential,
            pending.authentication,
            discoverable_keys.as_slice(),
        )
        .map_err(|err| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "passkey_authentication_failed",
                err.to_string(),
            )
        })?;

    let credential_id = encode_credential_id(credential_id_bytes);
    let matched_passkey = stored_passkeys
        .iter_mut()
        .find(|stored| stored.credential_id == credential_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "passkey_not_found",
                "the selected passkey is not registered for this account",
            )
        })?;
    let used_at = chrono::Utc::now().to_rfc3339();

    mark_passkey_authentication_used(
        &mut tx,
        matched_passkey,
        &authentication_result,
        used_at.as_str(),
    )
    .await?;

    tx.commit().await.map_err(ApiError::internal)?;
    session
        .insert(SESSION_KEY_USER_ID, user_id)
        .await
        .map_err(ApiError::internal)?;

    Ok(axum::Json(PasskeyAuthenticateVerifyResponse {
        status: "authenticated".to_owned(),
        next_path: state.config.public_base_url.to_string(),
    }))
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
              ('user-second-id', 102, 'second', 0, 0, '2026-02-25T11:00:00Z', '2026-02-25T11:00:00Z')
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
