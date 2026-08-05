use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use axum::http::StatusCode;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::{error::ApiError, sqlite_write::SqliteWriteCoordinator, state::AppState};

pub const API_KEY_PREFIX: &str = "orill_ak_";
const API_KEY_RANDOM_BYTES: usize = 32;
const API_KEY_PREFIX_CHARS: usize = 18;
const API_KEY_TAIL_CHARS: usize = 6;
pub const API_KEY_NAME_MAX_CHARS: usize = 80;

#[derive(Debug, sqlx::FromRow)]
struct ApiKeyAuthRow {
    id: String,
    user_id: String,
    is_disabled: i64,
}

#[derive(Clone)]
struct DeferredApiKeyTouch {
    pool: SqlitePool,
    sqlite_writer: SqliteWriteCoordinator,
    used_at: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DeferredApiKeyTouchKey {
    runtime_owner_id: String,
    api_key_id: String,
}

#[derive(Default)]
struct DeferredApiKeyTouchQueue {
    running: bool,
    active: Option<DeferredApiKeyTouchKey>,
    pending: HashMap<DeferredApiKeyTouchKey, DeferredApiKeyTouch>,
}

static DEFERRED_API_KEY_TOUCHES: OnceLock<Mutex<DeferredApiKeyTouchQueue>> = OnceLock::new();

pub fn generate_api_key_plaintext() -> String {
    let mut bytes = [0_u8; API_KEY_RANDOM_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    format!("{API_KEY_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

pub fn hash_api_key(api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

pub fn key_prefix(api_key: &str) -> String {
    api_key.chars().take(API_KEY_PREFIX_CHARS).collect()
}

pub fn mask_api_key(api_key: &str) -> String {
    let head: String = api_key.chars().take(API_KEY_PREFIX_CHARS).collect();
    let tail_reversed: String = api_key.chars().rev().take(API_KEY_TAIL_CHARS).collect();
    let tail: String = tail_reversed.chars().rev().collect();
    format!("{head}...{tail}")
}

pub fn normalize_name(raw: Option<&str>) -> Result<String, ApiError> {
    let normalized = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("API Key");
    let char_count = normalized.chars().count();
    if char_count > API_KEY_NAME_MAX_CHARS {
        return Err(ApiError::bad_request(format!(
            "name must be at most {API_KEY_NAME_MAX_CHARS} characters"
        )));
    }
    Ok(normalized.to_owned())
}

pub async fn authenticate_api_key(state: &AppState, api_key: &str) -> Result<String, ApiError> {
    if !api_key.starts_with(API_KEY_PREFIX) {
        return Err(invalid_api_key());
    }

    let key_hash = hash_api_key(api_key);
    let row = sqlx::query_as::<_, ApiKeyAuthRow>(
        r#"
        SELECT k.id, k.user_id, u.is_disabled
        FROM user_api_keys k
        JOIN users u ON u.id = k.user_id
        WHERE k.key_hash = ?
          AND k.revoked_at IS NULL
        LIMIT 1
        "#,
    )
    .bind(key_hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(invalid_api_key());
    };
    if row.is_disabled != 0 {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }

    enqueue_api_key_last_used_touch(
        state.runtime_owner_id.clone(),
        row.id.clone(),
        chrono::Utc::now().to_rfc3339(),
        state.pool.clone(),
        state.sqlite_writer.clone(),
    );

    Ok(row.user_id)
}

fn enqueue_api_key_last_used_touch(
    runtime_owner_id: String,
    api_key_id: String,
    used_at: String,
    pool: SqlitePool,
    sqlite_writer: SqliteWriteCoordinator,
) {
    let queue = DEFERRED_API_KEY_TOUCHES.get_or_init(|| Mutex::new(Default::default()));
    let should_spawn = {
        let Ok(mut queue) = queue.lock() else {
            tracing::warn!(
                event = "api_key.last_used",
                "deferred API key touch queue poisoned"
            );
            return;
        };
        let key = DeferredApiKeyTouchKey {
            runtime_owner_id,
            api_key_id,
        };
        queue
            .pending
            .entry(key)
            .and_modify(|pending| {
                if pending.used_at < used_at {
                    pending.used_at.clone_from(&used_at);
                }
            })
            .or_insert(DeferredApiKeyTouch {
                pool,
                sqlite_writer,
                used_at,
            });
        if queue.running {
            false
        } else {
            queue.running = true;
            true
        }
    };

    if should_spawn {
        tokio::spawn(drain_api_key_last_used_touches());
    }
}

async fn drain_api_key_last_used_touches() {
    loop {
        let next = {
            let queue = DEFERRED_API_KEY_TOUCHES.get_or_init(|| Mutex::new(Default::default()));
            let Ok(mut queue) = queue.lock() else {
                tracing::warn!(
                    event = "api_key.last_used",
                    "deferred API key touch queue poisoned"
                );
                return;
            };
            let Some(key) = queue.pending.keys().next().cloned() else {
                queue.running = false;
                return;
            };
            let touch = queue.pending.remove(&key).expect("pending touch exists");
            queue.active = Some(key.clone());
            (key, touch)
        };

        let (key, touch) = next;
        let touch_result = touch
            .sqlite_writer
            .write("api_key_last_used", |_| async {
                sqlx::query(
                    r#"
                    UPDATE user_api_keys
                    SET last_used_at = ?
                    WHERE id = ?
                      AND (last_used_at IS NULL OR julianday(last_used_at) < julianday(?))
                    "#,
                )
                .bind(touch.used_at.as_str())
                .bind(key.api_key_id.as_str())
                .bind(touch.used_at.as_str())
                .execute(&touch.pool)
                .await?;
                Ok::<(), anyhow::Error>(())
            })
            .await;
        if let Err(err) = touch_result {
            tracing::warn!(
                event = "api_key.last_used",
                error = %err,
                "deferred API key last-used touch failed"
            );
        }
        if let Some(queue) = DEFERRED_API_KEY_TOUCHES.get()
            && let Ok(mut queue) = queue.lock()
        {
            queue.active = None;
        }
    }
}

#[cfg(test)]
pub fn api_key_last_used_touch_is_pending(runtime_owner_id: &str, api_key_id: &str) -> bool {
    let key = DeferredApiKeyTouchKey {
        runtime_owner_id: runtime_owner_id.to_owned(),
        api_key_id: api_key_id.to_owned(),
    };
    DEFERRED_API_KEY_TOUCHES
        .get()
        .and_then(|queue| queue.lock().ok())
        .is_some_and(|queue| {
            queue.active.as_ref() == Some(&key) || queue.pending.contains_key(&key)
        })
}

fn invalid_api_key() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_api_key",
        "invalid API key",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_prefixed_and_hashed_without_plaintext() {
        let key = generate_api_key_plaintext();
        assert!(key.starts_with(API_KEY_PREFIX));
        assert_ne!(hash_api_key(&key), key);
        assert!(mask_api_key(&key).starts_with(API_KEY_PREFIX));
    }

    #[test]
    fn normalizes_api_key_names() {
        assert_eq!(normalize_name(Some(" Deploy bot ")).unwrap(), "Deploy bot");
        assert_eq!(normalize_name(Some("   ")).unwrap(), "API Key");
        assert!(normalize_name(Some(&"x".repeat(API_KEY_NAME_MAX_CHARS + 1))).is_err());
    }
}
