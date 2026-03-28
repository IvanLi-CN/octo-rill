use anyhow::Result;
use chrono::Utc;
use sqlx::{Row, SqlitePool};

use crate::{
    config::AppConfig,
    translations::{
        DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY,
        DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminRuntimeSettingsSnapshot {
    pub llm_max_concurrency: usize,
    pub translation_general_worker_concurrency: usize,
    pub translation_dedicated_worker_concurrency: usize,
}

pub async fn load_or_seed_runtime_settings(
    pool: &SqlitePool,
    config: &AppConfig,
) -> Result<AdminRuntimeSettingsSnapshot> {
    if let Some(snapshot) = fetch_runtime_settings(pool).await? {
        return Ok(snapshot);
    }

    let snapshot = AdminRuntimeSettingsSnapshot {
        llm_max_concurrency: config.ai_max_concurrency,
        translation_general_worker_concurrency: DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY,
        translation_dedicated_worker_concurrency: DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY,
    };
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO admin_runtime_settings (
          id,
          llm_max_concurrency,
          translation_general_worker_concurrency,
          translation_dedicated_worker_concurrency,
          created_at,
          updated_at
        )
        VALUES (1, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(i64::try_from(snapshot.llm_max_concurrency).unwrap_or(i64::MAX))
    .bind(i64::try_from(snapshot.translation_general_worker_concurrency).unwrap_or(i64::MAX))
    .bind(i64::try_from(snapshot.translation_dedicated_worker_concurrency).unwrap_or(i64::MAX))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await?;
    Ok(snapshot)
}

pub async fn update_llm_runtime_settings(
    pool: &SqlitePool,
    llm_max_concurrency: usize,
) -> Result<AdminRuntimeSettingsSnapshot> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE admin_runtime_settings
        SET llm_max_concurrency = ?, updated_at = ?
        WHERE id = 1
        "#,
    )
    .bind(i64::try_from(llm_max_concurrency).unwrap_or(i64::MAX))
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

async fn fetch_runtime_settings(pool: &SqlitePool) -> Result<Option<AdminRuntimeSettingsSnapshot>> {
    let row = sqlx::query(
        r#"
        SELECT
          llm_max_concurrency,
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
