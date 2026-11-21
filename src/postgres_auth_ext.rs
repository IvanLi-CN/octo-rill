//! PostgreSQL authentication extensions.
//!
//! WebAuthn passkey ceremonies and the LinuxDo / GitHub-connect OAuth entry
//! points. The PostgreSQL entry duplicates the legacy SQLite semantics
//! (`src/auth.rs`, `src/passkeys.rs`) against `public.*` tables so that both
//! runtimes share session keys, redirect targets, and stored data shapes.

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::Redirect,
};
use oauth2::{AuthorizationCode, CsrfToken, Scope, TokenResponse};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};
use tower_sessions::Session;
use tracing::{info, warn};
use url::Url;
use webauthn_rs::prelude::{
    AuthenticationResult, CreationChallengeResponse, DiscoverableKey, PublicKeyCredential,
    RegisterPublicKeyCredential, RequestChallengeResponse,
};

use crate::{
    config::AppConfig,
    error::ApiError,
    linuxdo, local_id,
    passkeys::{
        AttachPendingPasskeyOutcome, PasskeySummary, PendingPasskeyAuthenticationSession,
        PendingPasskeyCredentialSession, PendingPasskeyRegistrationMode,
        PendingPasskeyRegistrationSession, SESSION_KEY_PENDING_PASSKEY_AUTHENTICATION,
        SESSION_KEY_PENDING_PASSKEY_CREDENTIAL, SESSION_KEY_PENDING_PASSKEY_REGISTRATION,
        StoredPasskey, build_passkey_label, encode_credential_id, generate_user_handle_uuid,
        parse_user_handle_uuid, passkey_credential_id, pending_passkey_authentication_is_expired,
        pending_passkey_bind_is_expired, pending_passkey_registration_is_expired,
    },
    postgres_server::PostgresServerState,
    state::{ConfiguredWebauthn, LinuxDoOAuthClient},
};

/// Session keys shared with the legacy SQLite runtime.
pub(crate) const SESSION_KEY_OAUTH_STATE: &str = "oauth_state";
pub(crate) const SESSION_KEY_GITHUB_OAUTH_MODE: &str = "github_oauth_mode";
pub(crate) const SESSION_KEY_LINUXDO_OAUTH_STATE: &str = "linuxdo_oauth_state";
pub(crate) const SESSION_KEY_PENDING_LINUXDO: &str = "pending_linuxdo";
pub(crate) const SESSION_KEY_USER_ID: &str = "user_id";

pub(crate) const GITHUB_OAUTH_MODE_LOGIN: &str = "login";
const GITHUB_OAUTH_MODE_CONNECT: &str = "connect";

/// Per-process authentication configuration owned by the PostgreSQL entry.
#[derive(Clone)]
pub(crate) struct AuthExtState {
    pub(crate) public_base_url: Url,
    pub(crate) webauthn: ConfiguredWebauthn,
    pub(crate) linuxdo_oauth: Option<LinuxDoOAuthClient>,
    pub(crate) default_daily_brief_local_time: chrono::NaiveTime,
    pub(crate) default_daily_brief_time_zone: String,
}

impl AuthExtState {
    pub(crate) fn new(config: &AppConfig) -> anyhow::Result<Self> {
        Ok(Self {
            public_base_url: config.public_base_url.clone(),
            webauthn: crate::state::build_webauthn(config)?,
            linuxdo_oauth: crate::state::build_linuxdo_oauth_client(config)?,
            default_daily_brief_local_time: crate::briefs::default_daily_brief_local_time(config),
            default_daily_brief_time_zone: crate::briefs::default_daily_brief_time_zone(config)
                .to_owned(),
        })
    }
}

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

fn settings_redirect(
    base: &Url,
    section: &str,
    github_status: Option<&str>,
    linuxdo_status: Option<&str>,
    passkey_status: Option<&str>,
) -> String {
    let mut url = base
        .join("settings")
        .expect("settings route joins base url");
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
    base: &Url,
    linuxdo_status: Option<&str>,
    passkey_status: Option<&str>,
) -> String {
    let mut url = base.join("bind/github").expect("bind route joins base url");
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

fn post_linuxdo_login_redirect(base: &Url, passkey_status: Option<&str>) -> String {
    if let Some(passkey_status) = passkey_status {
        return settings_redirect(
            base,
            "passkeys",
            None,
            Some("connected"),
            Some(passkey_status),
        );
    }
    base.to_string()
}

fn post_linuxdo_bind_redirect(base: &Url, passkey_status: Option<&str>) -> String {
    if let Some(passkey_status) = passkey_status {
        return settings_redirect(
            base,
            "passkeys",
            None,
            Some("connected"),
            Some(passkey_status),
        );
    }
    settings_redirect(base, "linuxdo", None, Some("connected"), None)
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

async fn clear_pending_passkey_credential(session: &Session) {
    let _ = session
        .remove_value(SESSION_KEY_PENDING_PASSKEY_CREDENTIAL)
        .await;
}

async fn clear_pending_passkey_authentication(session: &Session) {
    let _ = session
        .remove::<PendingPasskeyAuthenticationSession>(SESSION_KEY_PENDING_PASSKEY_AUTHENTICATION)
        .await;
}

async fn clear_pending_linuxdo(session: &Session) {
    let _ = session.remove_value(SESSION_KEY_PENDING_LINUXDO).await;
}

async fn require_active_user_id(
    state: &PostgresServerState,
    session: &Session,
) -> Result<String, ApiError> {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "not logged in",
        ));
    };

    let row = sqlx::query("SELECT is_disabled, paused_at FROM public.users WHERE id = $1")
        .bind(&user_id)
        .fetch_optional(&state.runtime.pool)
        .await
        .map_err(ApiError::internal)?;
    let Some(row) = row else {
        let _ = session.clear().await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "session user not found",
        ));
    };
    let is_disabled: i64 = row.get("is_disabled");
    let paused_at: Option<String> = row.get("paused_at");
    if is_disabled != 0 {
        let _ = session.clear().await;
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }
    if paused_at.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_paused",
            "account is paused",
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    if let Err(error) = sqlx::query("UPDATE public.users SET last_active_at = $1 WHERE id = $2")
        .bind(&now)
        .bind(&user_id)
        .execute(&state.runtime.pool)
        .await
    {
        warn!(%error, user_id = %user_id, "PostgreSQL last_active_at touch failed");
    }

    Ok(user_id)
}

async fn user_is_disabled(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
) -> Result<bool, ApiError> {
    let is_disabled =
        sqlx::query_scalar::<_, i64>("SELECT is_disabled FROM public.users WHERE id = $1 LIMIT 1")
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(ApiError::internal)?;
    Ok(is_disabled != 0)
}

#[derive(sqlx::FromRow)]
struct StoredPasskeyRow {
    id: String,
    credential_id: String,
    label: String,
    passkey_json: String,
    created_at: String,
    last_used_at: Option<String>,
}

async fn load_passkeys(
    state: &PostgresServerState,
    user_id: &str,
) -> Result<Vec<StoredPasskey>, ApiError> {
    let rows = sqlx::query_as::<_, StoredPasskeyRow>(
        "SELECT id, credential_id, label, passkey_json, created_at, last_used_at \
         FROM public.user_passkeys WHERE user_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;

    rows.into_iter().map(stored_passkey_from_row).collect()
}

fn stored_passkey_from_row(row: StoredPasskeyRow) -> Result<StoredPasskey, ApiError> {
    let passkey = serde_json::from_str(&row.passkey_json).map_err(ApiError::internal)?;
    Ok(StoredPasskey {
        summary: PasskeySummary {
            id: row.id,
            label: row.label,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        },
        credential_id: row.credential_id,
        passkey,
    })
}

async fn load_user_id_by_user_handle(
    state: &PostgresServerState,
    user_handle_uuid: &str,
) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.users WHERE passkey_user_handle_uuid = $1 LIMIT 1",
    )
    .bind(user_handle_uuid)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)
}

async fn user_has_github_connection(
    state: &PostgresServerState,
    user_id: &str,
) -> Result<bool, ApiError> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.github_connections WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(count > 0)
}

async fn ensure_passkey_user_handle(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    pending_user_handle_uuid: &str,
) -> Result<bool, ApiError> {
    let existing_handle = sqlx::query_scalar::<_, Option<String>>(
        "SELECT passkey_user_handle_uuid FROM public.users WHERE id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    if let Some(existing_handle) = existing_handle.as_deref() {
        return Ok(existing_handle == pending_user_handle_uuid);
    }

    let result = sqlx::query(
        "UPDATE public.users SET passkey_user_handle_uuid = $1 \
         WHERE id = $2 AND passkey_user_handle_uuid IS NULL",
    )
    .bind(pending_user_handle_uuid)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    if result.rows_affected() > 0 {
        return Ok(true);
    }

    let refreshed_handle = sqlx::query_scalar::<_, Option<String>>(
        "SELECT passkey_user_handle_uuid FROM public.users WHERE id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    match refreshed_handle.as_deref() {
        Some(existing_handle) => Ok(existing_handle == pending_user_handle_uuid),
        None => Err(ApiError::internal("failed to persist passkey user handle")),
    }
}

async fn attach_pending_passkey_to_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    pending: &PendingPasskeyCredentialSession,
) -> Result<AttachPendingPasskeyOutcome, ApiError> {
    if !ensure_passkey_user_handle(tx, user_id, pending.user_handle_uuid.as_str()).await? {
        return Ok(AttachPendingPasskeyOutcome::RetryRequired);
    }

    let credential_id = passkey_credential_id(&pending.passkey);
    if let Some(owner_user_id) = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM public.user_passkeys WHERE credential_id = $1 LIMIT 1",
    )
    .bind(credential_id.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?
    {
        if owner_user_id != user_id {
            return Ok(AttachPendingPasskeyOutcome::AlreadyBound);
        }
        return Ok(AttachPendingPasskeyOutcome::AlreadyExists);
    }

    let passkey_json = serde_json::to_string(&pending.passkey).map_err(ApiError::internal)?;
    sqlx::query(
        "INSERT INTO public.user_passkeys \
         (id, user_id, credential_id, label, passkey_json, created_at, last_used_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(credential_id.as_str())
    .bind(pending.label.as_str())
    .bind(passkey_json)
    .bind(pending.created_at.as_str())
    .bind(Option::<String>::None)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(AttachPendingPasskeyOutcome::Attached)
}

async fn mark_passkey_authentication_used(
    tx: &mut Transaction<'_, Postgres>,
    stored_passkey: &mut StoredPasskey,
    result: &AuthenticationResult,
    used_at: &str,
) -> Result<(), ApiError> {
    let Some(_) = stored_passkey.passkey.update_credential(result) else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "passkey_mismatch",
            "passkey does not match the authenticated credential",
        ));
    };

    let passkey_json =
        serde_json::to_string(&stored_passkey.passkey).map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE public.user_passkeys SET passkey_json = $1, last_used_at = $2 WHERE id = $3",
    )
    .bind(passkey_json)
    .bind(used_at)
    .bind(stored_passkey.summary.id.as_str())
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    stored_passkey.summary.last_used_at = Some(used_at.to_owned());
    Ok(())
}

async fn consume_pending_passkey_after_login(
    state: &PostgresServerState,
    session: &Session,
    user_id: &str,
) -> Result<Option<&'static str>, ApiError> {
    let pending_passkey = session
        .get::<PendingPasskeyCredentialSession>(SESSION_KEY_PENDING_PASSKEY_CREDENTIAL)
        .await
        .map_err(ApiError::internal)?;
    let Some(pending_passkey) = pending_passkey else {
        return Ok(None);
    };
    if pending_passkey_bind_is_expired(&pending_passkey) {
        clear_pending_passkey_credential(session).await;
        return Ok(Some("expired"));
    }

    let mut tx = state
        .runtime
        .pool
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let status = match attach_pending_passkey_to_user(&mut tx, user_id, &pending_passkey).await? {
        AttachPendingPasskeyOutcome::Attached | AttachPendingPasskeyOutcome::AlreadyExists => {
            tx.commit().await.map_err(ApiError::internal)?;
            None
        }
        AttachPendingPasskeyOutcome::AlreadyBound => {
            let _ = tx.rollback().await;
            Some("passkey_already_bound")
        }
        AttachPendingPasskeyOutcome::RetryRequired => {
            let _ = tx.rollback().await;
            Some("passkey_retry_required")
        }
    };
    clear_pending_passkey_credential(session).await;
    Ok(status)
}

async fn finalize_passkey_authentication_session(
    session: &Session,
    user_id: &str,
) -> Result<(), ApiError> {
    session
        .insert(SESSION_KEY_USER_ID, user_id)
        .await
        .map_err(ApiError::internal)?;
    clear_pending_linuxdo(session).await;
    clear_pending_passkey_credential(session).await;
    Ok(())
}

async fn upsert_linuxdo_connection_record(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    pending: &PendingLinuxDoSession,
    now: &str,
) -> Result<(), ApiError> {
    if let Some(owner_user_id) = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM public.linuxdo_connections WHERE linuxdo_user_id = $1 LIMIT 1",
    )
    .bind(pending.linuxdo_user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?
        && owner_user_id != user_id
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "linuxdo_already_bound",
            "linuxdo account already bound to another user",
        ));
    }

    if let Some(existing_linuxdo_user_id) = sqlx::query_scalar::<_, i64>(
        "SELECT linuxdo_user_id FROM public.linuxdo_connections WHERE user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?
        && existing_linuxdo_user_id != pending.linuxdo_user_id
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "linuxdo_account_conflict",
            "account already bound to another linuxdo user",
        ));
    }

    sqlx::query(
        "INSERT INTO public.linuxdo_connections \
         (user_id, linuxdo_user_id, username, name, avatar_url, trust_level, active, silenced, linked_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9) \
         ON CONFLICT (user_id) DO UPDATE SET \
           linuxdo_user_id = EXCLUDED.linuxdo_user_id, \
           username = EXCLUDED.username, \
           name = EXCLUDED.name, \
           avatar_url = EXCLUDED.avatar_url, \
           trust_level = EXCLUDED.trust_level, \
           active = EXCLUDED.active, \
           silenced = EXCLUDED.silenced, \
           updated_at = EXCLUDED.updated_at",
    )
    .bind(user_id)
    .bind(pending.linuxdo_user_id)
    .bind(pending.username.as_str())
    .bind(pending.name.as_deref())
    .bind(pending.avatar_url.as_str())
    .bind(pending.trust_level)
    .bind(if pending.active { 1_i64 } else { 0_i64 })
    .bind(if pending.silenced { 1_i64 } else { 0_i64 })
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(())
}

fn should_use_github_connect_mode(
    requested_mode: Option<&str>,
    has_session_user: bool,
    has_pending_linuxdo: bool,
    has_pending_passkey: bool,
) -> bool {
    requested_mode == Some(GITHUB_OAUTH_MODE_CONNECT)
        || (has_session_user && !has_pending_linuxdo && !has_pending_passkey)
}

fn post_github_login_redirect(
    base: &Url,
    has_pending_linuxdo: bool,
    passkey_status: Option<&str>,
) -> String {
    if let Some(passkey_status) = passkey_status {
        return settings_redirect(
            base,
            "passkeys",
            has_pending_linuxdo.then_some("connected"),
            has_pending_linuxdo.then_some("connected"),
            Some(passkey_status),
        );
    }

    if has_pending_linuxdo {
        return settings_redirect(
            base,
            "github-accounts",
            Some("connected"),
            Some("connected"),
            None,
        );
    }

    base.to_string()
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

async fn load_github_connection_owner(
    tx: &mut Transaction<'_, Postgres>,
    github_user_id: i64,
) -> Result<Option<GitHubConnectionOwnerRow>, ApiError> {
    sqlx::query_as::<_, GitHubConnectionOwnerRow>(
        "SELECT id, user_id FROM public.github_connections WHERE github_user_id = $1 LIMIT 1",
    )
    .bind(github_user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

#[allow(clippy::too_many_arguments)]
async fn upsert_github_connection(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    user: &crate::github::GitHubUser,
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

    let crate::crypto::EncryptedSecret { ciphertext, nonce } = encryption_key
        .encrypt_str(access_token)
        .map_err(ApiError::internal)?;
    let connection_id = existing_id.unwrap_or_else(local_id::generate_local_id);

    sqlx::query(
        "INSERT INTO public.github_connections (
           id, user_id, github_user_id, login, name, avatar_url, email,
           access_token_ciphertext, access_token_nonce, scopes, linked_at, updated_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)
         ON CONFLICT (id) DO UPDATE SET
           github_user_id = EXCLUDED.github_user_id,
           login = EXCLUDED.login,
           name = EXCLUDED.name,
           avatar_url = EXCLUDED.avatar_url,
           email = EXCLUDED.email,
           access_token_ciphertext = EXCLUDED.access_token_ciphertext,
           access_token_nonce = EXCLUDED.access_token_nonce,
           scopes = EXCLUDED.scopes,
           updated_at = EXCLUDED.updated_at",
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
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(())
}

async fn load_auth_user_row(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
) -> Result<AuthUserRow, ApiError> {
    sqlx::query_as::<_, AuthUserRow>(
        "SELECT id, is_admin, is_disabled FROM public.users WHERE id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

async fn promote_first_admin(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    now: &str,
) -> Result<bool, ApiError> {
    let updated = sqlx::query(
        "UPDATE public.users SET is_admin = 1, updated_at = $1
         WHERE id = $2
           AND NOT EXISTS (SELECT 1 FROM public.users WHERE is_admin = 1)",
    )
    .bind(now)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(updated.rows_affected() > 0)
}

async fn load_enabled_daily_brief_scheduler_hours(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<std::collections::BTreeSet<u8>, ApiError> {
    let hours = sqlx::query_scalar::<_, i64>(
        "SELECT hour_utc FROM public.daily_brief_hour_slots WHERE enabled = 1",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(hours.into_iter().map(|hour| hour as u8).collect())
}

async fn load_existing_brief_preferences(
    tx: &mut Transaction<'_, Postgres>,
    github_user_id: i64,
) -> Result<Option<ExistingBriefPreferenceRow>, ApiError> {
    sqlx::query_as::<_, ExistingBriefPreferenceRow>(
        "SELECT daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time
         FROM public.users WHERE github_user_id = $1 LIMIT 1",
    )
    .bind(github_user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

async fn upsert_github_user_for_account(
    tx: &mut Transaction<'_, Postgres>,
    user: &crate::github::GitHubUser,
    email: Option<&str>,
    now: &str,
    default_daily_brief_local_time: chrono::NaiveTime,
    default_daily_brief_time_zone: &str,
) -> Result<String, ApiError> {
    let existing = load_existing_brief_preferences(tx, user.id).await?;

    let existing_local_time = existing
        .as_ref()
        .and_then(|row| row.daily_brief_local_time.as_deref())
        .and_then(|value| crate::briefs::parse_daily_brief_local_time(value).ok())
        .map(crate::briefs::format_daily_brief_local_time);
    let existing_time_zone = existing
        .as_ref()
        .and_then(|row| row.daily_brief_time_zone.as_deref())
        .and_then(crate::briefs::canonical_supported_time_zone);
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
            crate::briefs::DailyBriefPreferences {
                local_time: default_daily_brief_local_time,
                time_zone: default_daily_brief_time_zone.to_owned(),
            },
            |row| {
                crate::briefs::derive_daily_brief_preferences_with_defaults(
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
            crate::briefs::format_daily_brief_local_time(derived_preferences.local_time);
        let derived_legacy_utc_time = crate::briefs::format_legacy_daily_brief_utc_time(
            derived_preferences.local_time,
            &derived_preferences.time_zone,
        )
        .map_err(ApiError::internal)?;
        let enabled_hours = load_enabled_daily_brief_scheduler_hours(tx).await?;
        let missing_hours = crate::briefs::missing_daily_brief_scheduler_hours(
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
            warn!(
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
        "INSERT INTO public.users (
           id, github_user_id, login, name, avatar_url, email,
           created_at, updated_at, last_active_at,
           daily_brief_utc_time, daily_brief_local_time, daily_brief_time_zone
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         ON CONFLICT (github_user_id) DO UPDATE SET
           login = EXCLUDED.login,
           name = EXCLUDED.name,
           avatar_url = EXCLUDED.avatar_url,
           email = EXCLUDED.email,
           updated_at = EXCLUDED.updated_at,
           last_active_at = CASE
             WHEN users.paused_at IS NULL THEN EXCLUDED.last_active_at
             ELSE users.last_active_at
           END,
           daily_brief_utc_time = EXCLUDED.daily_brief_utc_time,
           daily_brief_local_time = EXCLUDED.daily_brief_local_time,
           daily_brief_time_zone = EXCLUDED.daily_brief_time_zone",
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

    sqlx::query_scalar::<_, String>("SELECT id FROM public.users WHERE github_user_id = $1 LIMIT 1")
        .bind(user.id)
        .fetch_one(&mut **tx)
        .await
        .map_err(ApiError::internal)
}

/// GitHub OAuth callback finalization.
///
/// Mirrors `src/auth.rs::finalize_github_auth`: connect mode binds the provider
/// account to the authenticated user, login mode resolves the owning account
/// (or creates it), links the connection, applies any pending LinuxDo/WebAuthn
/// bind context, and decides the post-login redirect.
pub(crate) async fn finalize_github_auth(
    state: &PostgresServerState,
    session: &Session,
    user: &crate::github::GitHubUser,
    email: Option<&str>,
    access_token: &str,
    scopes: &str,
    requested_mode: Option<String>,
) -> Result<Redirect, ApiError> {
    let base = &state.auth_ext.public_base_url;
    let now = chrono::Utc::now().to_rfc3339();
    let default_daily_brief_local_time = state.auth_ext.default_daily_brief_local_time;
    let default_daily_brief_time_zone = state.auth_ext.default_daily_brief_time_zone.as_str();

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
            clear_pending_passkey_credential(session).await;
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
    let mut tx = state
        .runtime
        .pool
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let mut login_user_after_commit: Option<String> = None;
    let mut consume_pending_passkey = false;

    let effective_mode = if should_use_github_connect_mode(
        requested_mode.as_deref(),
        session_user_id.is_some(),
        pending_linuxdo.is_some(),
        pending_passkey.is_some(),
    ) {
        GITHUB_OAUTH_MODE_CONNECT
    } else {
        GITHUB_OAUTH_MODE_LOGIN
    };

    let redirect = match effective_mode {
        GITHUB_OAUTH_MODE_CONNECT => {
            let Some(current_user_id) = session_user_id.clone() else {
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
                let _ = session.clear().await;
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "account_disabled",
                    "account is disabled",
                ));
            }

            match upsert_github_connection(
                &mut tx,
                current_user_id.as_str(),
                user,
                email,
                access_token,
                scopes,
                now.as_str(),
                &state.encryption_key,
            )
            .await
            {
                Ok(()) => settings_redirect(base, "github-accounts", Some("connected"), None, None),
                Err(error) if error.code() == "github_already_bound" => {
                    let _ = tx.rollback().await;
                    return Ok(Redirect::to(&settings_redirect(
                        base,
                        "github-accounts",
                        Some("already_bound"),
                        None,
                        None,
                    )));
                }
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(error);
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
                    user,
                    email,
                    now.as_str(),
                    default_daily_brief_local_time,
                    default_daily_brief_time_zone,
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
                let _ = session.clear().await;
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "account_disabled",
                    "account is disabled",
                ));
            }

            match upsert_github_connection(
                &mut tx,
                target_user_id.as_str(),
                user,
                email,
                access_token,
                scopes,
                now.as_str(),
                &state.encryption_key,
            )
            .await
            {
                Ok(()) => {}
                Err(error) if error.code() == "github_already_bound" => {
                    let _ = tx.rollback().await;
                    let redirect = if pending_linuxdo.is_some() {
                        bind_github_redirect(
                            base,
                            Some("github_already_bound"),
                            passkey_status_after_login,
                        )
                    } else {
                        settings_redirect(
                            base,
                            "github-accounts",
                            Some("already_bound"),
                            None,
                            passkey_status_after_login,
                        )
                    };
                    return Ok(Redirect::to(&redirect));
                }
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(error);
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
                && let Err(error) = upsert_linuxdo_connection_record(
                    &mut tx,
                    target_user_id.as_str(),
                    pending_linuxdo,
                    now.as_str(),
                )
                .await
            {
                let _ = tx.rollback().await;
                if should_clear_pending_passkey_after_linuxdo_rollback(passkey_status_after_login) {
                    clear_pending_passkey_credential(session).await;
                }
                return Ok(Redirect::to(&bind_github_redirect(
                    base,
                    Some(error.code()),
                    passkey_status_after_login,
                )));
            }

            login_user_after_commit = Some(target_user_id.clone());
            post_github_login_redirect(base, pending_linuxdo.is_some(), passkey_status_after_login)
        }
    };

    tx.commit().await.map_err(ApiError::internal)?;
    if let Some(user_id) = login_user_after_commit {
        session
            .insert(SESSION_KEY_USER_ID, user_id)
            .await
            .map_err(ApiError::internal)?;
    }
    clear_pending_linuxdo(session).await;
    if consume_pending_passkey {
        clear_pending_passkey_credential(session).await;
    }

    info!(login = %user.login, github_user_id = user.id, "github auth ok");
    Ok(Redirect::to(&redirect))
}

fn should_clear_pending_passkey_after_linuxdo_rollback(
    passkey_status_after_login: Option<&str>,
) -> bool {
    matches!(
        passkey_status_after_login,
        Some("passkey_already_bound" | "passkey_retry_required")
    )
}

#[derive(Debug, Deserialize)]
pub(crate) struct PasskeyRegisterVerifyRequest {
    credential: RegisterPublicKeyCredential,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PasskeyAuthenticateVerifyRequest {
    credential: PublicKeyCredential,
}

#[derive(Debug, Serialize)]
pub(crate) struct PasskeyRegisterVerifyResponse {
    status: String,
    next_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PasskeyAuthenticateVerifyResponse {
    status: String,
    next_path: String,
}

pub(crate) async fn passkey_register_options(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Result<Json<CreationChallengeResponse>, ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    let session_user_id = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .map_err(ApiError::internal)?;

    let (mode, user_handle_uuid, user_name, user_display_name, exclude_credentials) =
        if session_user_id.is_some() {
            let user_id = require_active_user_id(&state, &session).await?;
            let user_handle_uuid = sqlx::query_scalar::<_, Option<String>>(
                "SELECT passkey_user_handle_uuid FROM public.users WHERE id = $1 LIMIT 1",
            )
            .bind(&user_id)
            .fetch_one(&state.runtime.pool)
            .await
            .map_err(ApiError::internal)?
            .unwrap_or_else(generate_user_handle_uuid);

            let user_name = sqlx::query_scalar::<_, String>(
                "SELECT login FROM public.github_connections WHERE user_id = $1 \
                 ORDER BY linked_at ASC, id ASC LIMIT 1",
            )
            .bind(&user_id)
            .fetch_optional(&state.runtime.pool)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    "passkey_github_required",
                    "at least one github connection is required before adding a passkey",
                )
            })?;

            let existing_passkeys = load_passkeys(&state, &user_id).await?;
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
        .auth_ext
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

    Ok(Json(challenge))
}

pub(crate) async fn passkey_register_verify(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(req): Json<PasskeyRegisterVerifyRequest>,
) -> Result<Json<PasskeyRegisterVerifyResponse>, ApiError> {
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
        .auth_ext
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
            let current_user_id = require_active_user_id(&state, &session).await?;
            if current_user_id != user_id {
                return Err(ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "session user changed during passkey registration",
                ));
            }

            let mut tx = state
                .runtime
                .pool
                .begin()
                .await
                .map_err(ApiError::internal)?;
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
                    Ok(Json(PasskeyRegisterVerifyResponse {
                        status: "registered".to_owned(),
                        next_path: Some(settings_redirect(
                            &state.auth_ext.public_base_url,
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

            Ok(Json(PasskeyRegisterVerifyResponse {
                status: "pending_github_bind".to_owned(),
                next_path: Some(bind_github_redirect(
                    &state.auth_ext.public_base_url,
                    None,
                    Some("created"),
                )),
            }))
        }
    }
}

pub(crate) async fn passkey_authenticate_options(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Result<Json<RequestChallengeResponse>, ApiError> {
    let (challenge, authentication) = state
        .auth_ext
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

    Ok(Json(challenge))
}

pub(crate) async fn passkey_authenticate_verify(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(req): Json<PasskeyAuthenticateVerifyRequest>,
) -> Result<Json<PasskeyAuthenticateVerifyResponse>, ApiError> {
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
        .auth_ext
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
    let user_id = load_user_id_by_user_handle(&state, user_handle_uuid.as_str())
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "passkey_user_not_found",
                "passkey does not map to an existing account",
            )
        })?;

    if !user_has_github_connection(&state, user_id.as_str()).await? {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "passkey_github_required",
            "a github connection is required before passkey login can complete",
        ));
    }

    let mut stored_passkeys = load_passkeys(&state, user_id.as_str()).await?;
    if stored_passkeys.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "passkey_not_found",
            "no passkeys are registered for this account",
        ));
    }

    let mut tx = state
        .runtime
        .pool
        .begin()
        .await
        .map_err(ApiError::internal)?;
    if user_is_disabled(&mut tx, user_id.as_str()).await? {
        let _ = session.clear().await;
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
        .auth_ext
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
    finalize_passkey_authentication_session(&session, user_id.as_str()).await?;

    Ok(Json(PasskeyAuthenticateVerifyResponse {
        status: "authenticated".to_owned(),
        next_path: state.auth_ext.public_base_url.to_string(),
    }))
}

pub(crate) async fn github_connect(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Result<Redirect, ApiError> {
    require_active_user_id(&state, &session).await?;

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

pub(crate) async fn linuxdo_login(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Result<Redirect, ApiError> {
    let Some(client) = state.auth_ext.linuxdo_oauth.as_ref() else {
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

#[derive(Debug, Deserialize)]
pub(crate) struct LinuxDoCallbackQuery {
    code: String,
    state: String,
}

pub(crate) async fn linuxdo_callback(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<LinuxDoCallbackQuery>,
) -> Result<Redirect, ApiError> {
    let base = &state.auth_ext.public_base_url;
    let session_user_id = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .map_err(ApiError::internal)?;
    let Some(client) = state.auth_ext.linuxdo_oauth.as_ref() else {
        let redirect = if session_user_id.is_some() {
            settings_redirect(base, "linuxdo", None, Some("not_configured"), None)
        } else {
            bind_github_redirect(base, Some("not_configured"), None)
        };
        return Ok(Redirect::to(&redirect));
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
            settings_redirect(base, "linuxdo", None, Some("state_mismatch"), None)
        } else {
            bind_github_redirect(base, Some("state_mismatch"), None)
        };
        return Ok(Redirect::to(&redirect));
    }

    let token = match client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(&state.http)
        .await
    {
        Ok(token) => token,
        Err(error) => {
            warn!(%error, "linuxdo oauth token exchange failed");
            let redirect = if session_user_id.is_some() {
                settings_redirect(base, "linuxdo", None, Some("exchange_failed"), None)
            } else {
                bind_github_redirect(base, Some("exchange_failed"), None)
            };
            return Ok(Redirect::to(&redirect));
        }
    };

    let access_token = token.access_token().secret();
    let linuxdo_user = match linuxdo::fetch_user(&state.http, access_token).await {
        Ok(user) => user,
        Err(error) => {
            warn!(%error, "failed to fetch linuxdo user");
            let redirect = if session_user_id.is_some() {
                settings_redirect(base, "linuxdo", None, Some("fetch_user_failed"), None)
            } else {
                bind_github_redirect(base, Some("fetch_user_failed"), None)
            };
            return Ok(Redirect::to(&redirect));
        }
    };

    let pending = PendingLinuxDoSession::from_linuxdo_user(&linuxdo_user);

    if let Some(owner_user_id) = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM public.linuxdo_connections WHERE linuxdo_user_id = $1 LIMIT 1",
    )
    .bind(pending.linuxdo_user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?
    {
        let passkey_status_after_login =
            consume_pending_passkey_after_login(&state, &session, owner_user_id.as_str()).await?;
        session
            .insert(SESSION_KEY_USER_ID, owner_user_id)
            .await
            .map_err(ApiError::internal)?;
        clear_pending_linuxdo(&session).await;
        return Ok(Redirect::to(&post_linuxdo_login_redirect(
            base,
            passkey_status_after_login,
        )));
    }

    if let Some(user_id) = session_user_id {
        let mut tx = state
            .runtime
            .pool
            .begin()
            .await
            .map_err(ApiError::internal)?;
        if user_is_disabled(&mut tx, user_id.as_str()).await? {
            let _ = tx.rollback().await;
            let _ = session.clear().await;
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "account_disabled",
                "account is disabled",
            ));
        }

        let now = chrono::Utc::now().to_rfc3339();
        match upsert_linuxdo_connection_record(&mut tx, user_id.as_str(), &pending, &now).await {
            Ok(()) => {
                tx.commit().await.map_err(ApiError::internal)?;
                let passkey_status_after_login =
                    consume_pending_passkey_after_login(&state, &session, user_id.as_str()).await?;
                return Ok(Redirect::to(&post_linuxdo_bind_redirect(
                    base,
                    passkey_status_after_login,
                )));
            }
            Err(error) => {
                let _ = tx.rollback().await;
                let redirect = settings_redirect(base, "linuxdo", None, Some(error.code()), None);
                return Ok(Redirect::to(&redirect));
            }
        }
    }

    session
        .insert(SESSION_KEY_PENDING_LINUXDO, pending)
        .await
        .map_err(ApiError::internal)?;
    Ok(Redirect::to(&bind_github_redirect(base, None, None)))
}

#[cfg(test)]
mod tests {
    use super::{
        GITHUB_OAUTH_MODE_CONNECT, GITHUB_OAUTH_MODE_LOGIN, post_github_login_redirect,
        should_use_github_connect_mode,
    };
    use url::Url;

    #[test]
    fn connect_mode_request_wins_over_pending_bind_context() {
        assert!(should_use_github_connect_mode(
            Some(GITHUB_OAUTH_MODE_CONNECT),
            false,
            true,
            true
        ));
    }

    #[test]
    fn authenticated_session_without_pending_bind_context_uses_connect_mode() {
        assert!(should_use_github_connect_mode(None, true, false, false));
        assert!(should_use_github_connect_mode(
            Some(GITHUB_OAUTH_MODE_LOGIN),
            true,
            false,
            false
        ));
    }

    #[test]
    fn pending_bind_context_forces_login_mode() {
        assert!(!should_use_github_connect_mode(None, true, true, false));
        assert!(!should_use_github_connect_mode(None, true, false, true));
        assert!(!should_use_github_connect_mode(None, false, false, false));
    }

    #[test]
    fn post_login_redirect_prefers_passkeys_for_passkey_recovery() {
        let base = Url::parse("http://127.0.0.1:58090").expect("parse base url");
        assert_eq!(
            post_github_login_redirect(&base, true, Some("passkey_retry_required")),
            "http://127.0.0.1:58090/settings?section=passkeys&github=connected&linuxdo=connected&passkey=passkey_retry_required"
        );
        assert_eq!(
            post_github_login_redirect(&base, true, None),
            "http://127.0.0.1:58090/settings?section=github-accounts&github=connected&linuxdo=connected"
        );
        assert_eq!(
            post_github_login_redirect(&base, false, None),
            "http://127.0.0.1:58090/"
        );
    }
}
