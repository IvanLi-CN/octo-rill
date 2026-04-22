use anyhow::Context;
use axum::http::StatusCode;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;
use webauthn_rs::prelude::{AuthenticationResult, Passkey, PasskeyRegistration};

use crate::{error::ApiError, local_id, state::AppState};

pub const SESSION_KEY_PENDING_PASSKEY_REGISTRATION: &str = "pending_passkey_registration";
pub const SESSION_KEY_PENDING_PASSKEY_CREDENTIAL: &str = "pending_passkey_credential";
pub const SESSION_KEY_PENDING_PASSKEY_AUTHENTICATION: &str = "pending_passkey_authentication";

pub const PASSKEY_REGISTRATION_TTL_SECS: i64 = 10 * 60;
pub const PASSKEY_AUTHENTICATION_TTL_SECS: i64 = 5 * 60;
pub const PASSKEY_BIND_TTL_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PendingPasskeyRegistrationMode {
    Authenticated { user_id: String },
    Onboarding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPasskeyRegistrationSession {
    pub mode: PendingPasskeyRegistrationMode,
    pub user_handle_uuid: String,
    pub label: String,
    pub started_at: String,
    pub registration: PasskeyRegistration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPasskeyCredentialSession {
    pub user_handle_uuid: String,
    pub label: String,
    pub created_at: String,
    pub passkey: Passkey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPasskeyAuthenticationSession {
    pub started_at: String,
    pub authentication: webauthn_rs::prelude::DiscoverableAuthentication,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PasskeySummary {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredPasskey {
    pub summary: PasskeySummary,
    pub credential_id: String,
    pub passkey: Passkey,
}

#[derive(Debug, sqlx::FromRow)]
struct StoredPasskeyRow {
    id: String,
    credential_id: String,
    label: String,
    passkey_json: String,
    created_at: String,
    last_used_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachPendingPasskeyOutcome {
    Attached,
    AlreadyExists,
    AlreadyBound,
    RetryRequired,
}

pub fn build_passkey_label(now: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(now)
        .map(|value| {
            format!(
                "通行密钥 · {}",
                value.with_timezone(&Utc).format("%Y-%m-%d %H:%M UTC")
            )
        })
        .unwrap_or_else(|_| "通行密钥".to_owned())
}

pub fn encode_credential_id(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

pub fn passkey_credential_id(passkey: &Passkey) -> String {
    encode_credential_id(passkey.cred_id().as_ref())
}

pub fn generate_user_handle_uuid() -> String {
    Uuid::new_v4().to_string()
}

pub fn parse_user_handle_uuid(value: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| ApiError::bad_request("invalid passkey user handle"))
}

pub fn session_timestamp_is_expired(value: &str, ttl_secs: i64) -> bool {
    let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) else {
        return true;
    };
    Utc::now().signed_duration_since(timestamp.with_timezone(&Utc)) > Duration::seconds(ttl_secs)
}

pub fn pending_passkey_registration_is_expired(
    pending: &PendingPasskeyRegistrationSession,
) -> bool {
    session_timestamp_is_expired(&pending.started_at, PASSKEY_REGISTRATION_TTL_SECS)
}

pub fn pending_passkey_authentication_is_expired(
    pending: &PendingPasskeyAuthenticationSession,
) -> bool {
    session_timestamp_is_expired(&pending.started_at, PASSKEY_AUTHENTICATION_TTL_SECS)
}

pub fn pending_passkey_bind_is_expired(pending: &PendingPasskeyCredentialSession) -> bool {
    session_timestamp_is_expired(&pending.created_at, PASSKEY_BIND_TTL_SECS)
}

pub async fn load_passkey_summaries(
    state: &AppState,
    user_id: &str,
) -> Result<Vec<PasskeySummary>, ApiError> {
    sqlx::query_as::<_, PasskeySummary>(
        r#"
        SELECT id, label, created_at, last_used_at
        FROM user_passkeys
        WHERE user_id = ?
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)
}

pub async fn load_passkeys(
    state: &AppState,
    user_id: &str,
) -> Result<Vec<StoredPasskey>, ApiError> {
    let rows = sqlx::query_as::<_, StoredPasskeyRow>(
        r#"
        SELECT
          id,
          credential_id,
          label,
          passkey_json,
          created_at,
          last_used_at
        FROM user_passkeys
        WHERE user_id = ?
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    rows.into_iter().map(stored_passkey_from_row).collect()
}

pub async fn load_user_id_by_user_handle(
    state: &AppState,
    user_handle_uuid: &str,
) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM users
        WHERE passkey_user_handle_uuid = ?
        LIMIT 1
        "#,
    )
    .bind(user_handle_uuid)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)
}

pub async fn user_has_github_connection(state: &AppState, user_id: &str) -> Result<bool, ApiError> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM github_connections
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(count > 0)
}

pub async fn attach_pending_passkey_to_user(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    pending: &PendingPasskeyCredentialSession,
) -> Result<AttachPendingPasskeyOutcome, ApiError> {
    let existing_handle = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT passkey_user_handle_uuid
        FROM users
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    if let Some(existing_handle) = existing_handle.as_deref() {
        if existing_handle != pending.user_handle_uuid {
            return Ok(AttachPendingPasskeyOutcome::RetryRequired);
        }
    } else {
        sqlx::query(
            r#"
            UPDATE users
            SET passkey_user_handle_uuid = ?
            WHERE id = ?
              AND passkey_user_handle_uuid IS NULL
            "#,
        )
        .bind(pending.user_handle_uuid.as_str())
        .bind(user_id)
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
    }

    let credential_id = passkey_credential_id(&pending.passkey);
    if let Some(owner_user_id) = sqlx::query_scalar::<_, String>(
        r#"
        SELECT user_id
        FROM user_passkeys
        WHERE credential_id = ?
        LIMIT 1
        "#,
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
        r#"
        INSERT INTO user_passkeys (
          id,
          user_id,
          credential_id,
          label,
          passkey_json,
          created_at,
          last_used_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
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

pub async fn mark_passkey_authentication_used(
    tx: &mut Transaction<'_, Sqlite>,
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
        r#"
        UPDATE user_passkeys
        SET passkey_json = ?, last_used_at = ?
        WHERE id = ?
        "#,
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

fn stored_passkey_from_row(row: StoredPasskeyRow) -> Result<StoredPasskey, ApiError> {
    Ok(StoredPasskey {
        summary: PasskeySummary {
            id: row.id,
            label: row.label,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        },
        credential_id: row.credential_id,
        passkey: serde_json::from_str(&row.passkey_json)
            .context("failed to deserialize stored passkey")
            .map_err(ApiError::internal)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        PASSKEY_AUTHENTICATION_TTL_SECS, PASSKEY_BIND_TTL_SECS, PASSKEY_REGISTRATION_TTL_SECS,
        build_passkey_label, session_timestamp_is_expired,
    };

    #[test]
    fn build_passkey_label_keeps_stable_utc_shape() {
        assert_eq!(
            build_passkey_label("2026-04-22T10:12:00Z"),
            "通行密钥 · 2026-04-22 10:12 UTC"
        );
    }

    #[test]
    fn session_timestamp_is_expired_handles_invalid_inputs() {
        assert!(session_timestamp_is_expired(
            "invalid",
            PASSKEY_BIND_TTL_SECS
        ));
    }

    #[test]
    fn session_timestamp_is_expired_respects_time_to_live() {
        let fresh = (chrono::Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
        let stale = (chrono::Utc::now()
            - chrono::Duration::seconds(PASSKEY_REGISTRATION_TTL_SECS + 5))
        .to_rfc3339();

        assert!(!session_timestamp_is_expired(
            &fresh,
            PASSKEY_AUTHENTICATION_TTL_SECS
        ));
        assert!(session_timestamp_is_expired(
            &stale,
            PASSKEY_REGISTRATION_TTL_SECS
        ));
    }
}
