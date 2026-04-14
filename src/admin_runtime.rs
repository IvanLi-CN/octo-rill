use std::env;

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{Row, SqlitePool};

use crate::{
    config::AppConfig,
    state::AppState,
    translations::{
        DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY,
        DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY, TranslationRuntimeConfig,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminRuntimeSettingsSnapshot {
    pub llm_max_concurrency: usize,
    pub ai_model_context_limit: Option<u32>,
    pub translation_general_worker_concurrency: usize,
    pub translation_dedicated_worker_concurrency: usize,
}

fn load_legacy_ai_model_context_limit_from_env() -> Result<Option<u32>> {
    let Some(raw) = env::var_os("AI_MODEL_CONTEXT_LIMIT") else {
        return Ok(None);
    };

    let raw = raw.into_string().map_err(|_| {
        anyhow::anyhow!("invalid AI_MODEL_CONTEXT_LIMIT (expected positive integer)")
    })?;
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let parsed = raw
        .parse::<u32>()
        .context("invalid AI_MODEL_CONTEXT_LIMIT (expected positive integer)")?;
    if parsed == 0 {
        anyhow::bail!("invalid AI_MODEL_CONTEXT_LIMIT (expected positive integer)");
    }

    Ok(Some(parsed))
}

async fn maybe_backfill_legacy_ai_model_context_limit(pool: &SqlitePool) -> Result<()> {
    let Some(limit) = load_legacy_ai_model_context_limit_from_env()? else {
        return Ok(());
    };

    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE admin_runtime_settings
        SET
          ai_model_context_limit = ?,
          ai_model_context_limit_migrated_at = ?,
          updated_at = ?
        WHERE
          id = 1
          AND ai_model_context_limit IS NULL
          AND ai_model_context_limit_migrated_at IS NULL
        "#,
    )
    .bind(i64::from(limit))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_or_seed_runtime_settings(
    pool: &SqlitePool,
    config: &AppConfig,
) -> Result<AdminRuntimeSettingsSnapshot> {
    if let Some(snapshot) = fetch_runtime_settings(pool).await? {
        if snapshot.ai_model_context_limit.is_none() {
            maybe_backfill_legacy_ai_model_context_limit(pool).await?;
            return fetch_runtime_settings(pool).await?.ok_or_else(|| {
                anyhow::anyhow!("admin runtime settings row missing after backfill")
            });
        }
        return Ok(snapshot);
    }

    let snapshot = AdminRuntimeSettingsSnapshot {
        llm_max_concurrency: config.ai_max_concurrency,
        ai_model_context_limit: None,
        translation_general_worker_concurrency: DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY,
        translation_dedicated_worker_concurrency: DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY,
    };
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO admin_runtime_settings (
          id,
          llm_max_concurrency,
          ai_model_context_limit,
          ai_model_context_limit_migrated_at,
          translation_general_worker_concurrency,
          translation_dedicated_worker_concurrency,
          created_at,
          updated_at
        )
        VALUES (1, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO NOTHING
        "#,
    )
    .bind(i64::try_from(snapshot.llm_max_concurrency).unwrap_or(i64::MAX))
    .bind(snapshot.ai_model_context_limit.map(i64::from))
    .bind(Option::<&str>::None)
    .bind(i64::try_from(snapshot.translation_general_worker_concurrency).unwrap_or(i64::MAX))
    .bind(i64::try_from(snapshot.translation_dedicated_worker_concurrency).unwrap_or(i64::MAX))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await?;
    fetch_runtime_settings(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("admin runtime settings row missing after seed"))
}

pub async fn update_llm_runtime_settings(
    pool: &SqlitePool,
    llm_max_concurrency: usize,
    ai_model_context_limit: Option<u32>,
) -> Result<AdminRuntimeSettingsSnapshot> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE admin_runtime_settings
        SET
          llm_max_concurrency = ?,
          ai_model_context_limit = ?,
          ai_model_context_limit_migrated_at = COALESCE(ai_model_context_limit_migrated_at, ?),
          updated_at = ?
        WHERE id = 1
        "#,
    )
    .bind(i64::try_from(llm_max_concurrency).unwrap_or(i64::MAX))
    .bind(ai_model_context_limit.map(i64::from))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await?;
    fetch_runtime_settings(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("admin runtime settings row missing after llm update"))
}

pub async fn update_translation_runtime_settings(
    pool: &SqlitePool,
    general_worker_concurrency: usize,
    dedicated_worker_concurrency: usize,
) -> Result<AdminRuntimeSettingsSnapshot> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE admin_runtime_settings
        SET
          translation_general_worker_concurrency = ?,
          translation_dedicated_worker_concurrency = ?,
          updated_at = ?
        WHERE id = 1
        "#,
    )
    .bind(i64::try_from(general_worker_concurrency).unwrap_or(i64::MAX))
    .bind(i64::try_from(dedicated_worker_concurrency).unwrap_or(i64::MAX))
    .bind(now.as_str())
    .execute(pool)
    .await?;
    fetch_runtime_settings(pool).await?.ok_or_else(|| {
        anyhow::anyhow!("admin runtime settings row missing after translation update")
    })
}

pub async fn load_ai_model_context_limit(pool: &SqlitePool) -> Result<Option<u32>> {
    Ok(fetch_runtime_settings(pool)
        .await?
        .and_then(|snapshot| snapshot.ai_model_context_limit))
}

pub async fn sync_persisted_runtime_settings(
    state: std::sync::Arc<AppState>,
) -> Result<AdminRuntimeSettingsSnapshot> {
    let snapshot = load_or_seed_runtime_settings(&state.pool, &state.config).await?;

    state
        .llm_scheduler
        .set_max_concurrency(snapshot.llm_max_concurrency)
        .await;
    state
        .translation_scheduler
        .apply_runtime_config(
            state.clone(),
            TranslationRuntimeConfig::new(
                snapshot.translation_general_worker_concurrency,
                snapshot.translation_dedicated_worker_concurrency,
            ),
        )
        .await?;

    Ok(snapshot)
}

async fn fetch_runtime_settings(pool: &SqlitePool) -> Result<Option<AdminRuntimeSettingsSnapshot>> {
    let row = sqlx::query(
        r#"
        SELECT
          llm_max_concurrency,
          ai_model_context_limit,
          translation_general_worker_concurrency,
          translation_dedicated_worker_concurrency
        FROM admin_runtime_settings
        WHERE id = 1
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| AdminRuntimeSettingsSnapshot {
        llm_max_concurrency: usize::try_from(row.get::<i64, _>("llm_max_concurrency")).unwrap_or(1),
        ai_model_context_limit: row
            .get::<Option<i64>, _>("ai_model_context_limit")
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0),
        translation_general_worker_concurrency: usize::try_from(
            row.get::<i64, _>("translation_general_worker_concurrency"),
        )
        .unwrap_or(DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY),
        translation_dedicated_worker_concurrency: usize::try_from(
            row.get::<i64, _>("translation_dedicated_worker_concurrency"),
        )
        .unwrap_or(DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
    }))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::{Mutex, OnceLock};

    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use std::sync::Arc;
    use url::Url;

    use super::*;
    use crate::{
        ai::LlmScheduler,
        config::AppConfig,
        crypto::EncryptionKey,
        local_id::generate_local_id,
        state::build_oauth_client,
        translations::{TranslationRuntimeConfig, TranslationSchedulerController},
    };

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_legacy_context_limit_env() {
        unsafe {
            std::env::remove_var("AI_MODEL_CONTEXT_LIMIT");
        }
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn load_or_seed_runtime_settings_is_idempotent_for_concurrent_callers() {
        let _env_guard = env_lock().lock().expect("lock env");
        clear_legacy_context_limit_env();
        let pool = setup_pool().await;
        let config = test_config(7);

        let (left, right) = tokio::join!(
            load_or_seed_runtime_settings(&pool, &config),
            load_or_seed_runtime_settings(&pool, &config),
        );

        let left = left.expect("left seed succeeds");
        let right = right.expect("right seed succeeds");
        let stored = fetch_runtime_settings(&pool)
            .await
            .expect("fetch seed row succeeds")
            .expect("seed row exists");

        assert_eq!(left, stored);
        assert_eq!(right, stored);
        assert_eq!(stored.llm_max_concurrency, 7);
        assert_eq!(stored.ai_model_context_limit, None);
        assert_eq!(
            stored.translation_general_worker_concurrency,
            DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY,
        );
        assert_eq!(
            stored.translation_dedicated_worker_concurrency,
            DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY,
        );
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn sync_persisted_runtime_settings_updates_live_schedulers() {
        let _env_guard = env_lock().lock().expect("lock env");
        clear_legacy_context_limit_env();
        let pool = setup_pool().await;
        let config = test_config(1);
        load_or_seed_runtime_settings(&pool, &config)
            .await
            .expect("seed runtime settings");
        let state = setup_state(pool.clone(), config.clone());

        update_llm_runtime_settings(&pool, 4, Some(32_768))
            .await
            .expect("update llm settings");
        update_translation_runtime_settings(&pool, 5, 2)
            .await
            .expect("update translation settings");

        sync_persisted_runtime_settings(state.clone())
            .await
            .expect("sync persisted runtime settings");

        assert_eq!(state.llm_scheduler.max_concurrency(), 4);
        let stored = fetch_runtime_settings(&pool)
            .await
            .expect("fetch synced runtime settings")
            .expect("runtime settings should exist");
        assert_eq!(stored.ai_model_context_limit, Some(32_768));
        assert_eq!(
            state.translation_scheduler.desired_config().await,
            TranslationRuntimeConfig::new(5, 2),
        );
        assert_eq!(
            state.translation_scheduler.runtime_statuses().await.len(),
            7
        );

        state.translation_scheduler.abort_all().await;
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn load_or_seed_runtime_settings_backfills_legacy_context_limit_from_env() {
        let _env_guard = env_lock().lock().expect("lock env");
        unsafe {
            std::env::set_var("AI_MODEL_CONTEXT_LIMIT", "65536");
        }

        let pool = setup_pool().await;
        let config = test_config(3);
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO admin_runtime_settings (
              id,
              llm_max_concurrency,
              ai_model_context_limit,
              translation_general_worker_concurrency,
              translation_dedicated_worker_concurrency,
              created_at,
              updated_at
            )
            VALUES (1, ?, NULL, ?, ?, ?, ?)
            "#,
        )
        .bind(3_i64)
        .bind(i64::try_from(DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY).unwrap_or(i64::MAX))
        .bind(i64::try_from(DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY).unwrap_or(i64::MAX))
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("seed legacy runtime row");

        let snapshot = load_or_seed_runtime_settings(&pool, &config)
            .await
            .expect("backfill legacy env override");
        assert_eq!(snapshot.ai_model_context_limit, Some(65_536));

        let stored = fetch_runtime_settings(&pool)
            .await
            .expect("fetch backfilled runtime settings")
            .expect("runtime settings should exist");
        assert_eq!(stored.ai_model_context_limit, Some(65_536));

        clear_legacy_context_limit_env();
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn load_or_seed_runtime_settings_keeps_fresh_rows_on_auto_limit_even_with_legacy_env() {
        let _env_guard = env_lock().lock().expect("lock env");
        unsafe {
            std::env::set_var("AI_MODEL_CONTEXT_LIMIT", "65536");
        }

        let pool = setup_pool().await;
        let config = test_config(2);

        let snapshot = load_or_seed_runtime_settings(&pool, &config)
            .await
            .expect("seed runtime settings without legacy env carryover");
        assert_eq!(snapshot.ai_model_context_limit, None);

        let stored = fetch_runtime_settings(&pool)
            .await
            .expect("fetch seeded runtime settings")
            .expect("runtime settings should exist");
        assert_eq!(stored.ai_model_context_limit, None);

        clear_legacy_context_limit_env();
    }
    async fn setup_pool() -> SqlitePool {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-admin-runtime-{}.db",
            generate_local_id(),
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .expect("create sqlite db");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    fn setup_state(pool: SqlitePool, config: AppConfig) -> Arc<AppState> {
        let oauth = build_oauth_client(&config).expect("build oauth client");
        Arc::new(AppState {
            llm_scheduler: Arc::new(LlmScheduler::new(config.ai_max_concurrency)),
            translation_scheduler: Arc::new(TranslationSchedulerController::new(
                TranslationRuntimeConfig::default(),
            )),
            http: reqwest::Client::new(),
            oauth,
            encryption_key: config.encryption_key.clone(),
            runtime_owner_id: generate_local_id(),
            pool,
            config,
        })
    }

    fn test_config(ai_max_concurrency: usize) -> AppConfig {
        AppConfig {
            bind_addr: "127.0.0.1:58090"
                .parse::<SocketAddr>()
                .expect("parse bind addr"),
            public_base_url: Url::parse("http://127.0.0.1:58090").expect("parse public base url"),
            database_url: "sqlite::memory:".to_owned(),
            static_dir: None,
            task_log_dir: std::env::temp_dir().join("octo-rill-admin-runtime-tests"),
            job_worker_concurrency: 2,
            encryption_key: EncryptionKey::from_base64(
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            )
            .expect("build encryption key"),
            github: crate::config::GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse github redirect"),
            },
            ai: None,
            ai_max_concurrency,
            ai_daily_at_local: None,
        }
    }
}
