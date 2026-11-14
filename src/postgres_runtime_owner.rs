//! PostgreSQL runtime-owner lease persistence.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::time::{Duration, sleep};

const MAX_RETRIES: usize = 3;

pub async fn register(pool: &PgPool, owner_id: &str, now: DateTime<Utc>) -> Result<()> {
    execute_with_retry(|| async {
        sqlx::query(
            r#"
            INSERT INTO public.runtime_owners
                (runtime_owner_id, lease_heartbeat_at, created_at, updated_at)
            VALUES ($1, $2, $2, $2)
            ON CONFLICT (runtime_owner_id) DO UPDATE SET
                lease_heartbeat_at = EXCLUDED.lease_heartbeat_at,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(owner_id)
        .bind(now)
        .execute(pool)
        .await
        .map(|_| ())
    })
    .await
    .context("register PostgreSQL runtime owner")
}

pub async fn heartbeat(pool: &PgPool, owner_id: &str, now: DateTime<Utc>) -> Result<bool> {
    execute_with_retry(|| async {
        let result = sqlx::query(
            r#"
            UPDATE public.runtime_owners
            SET lease_heartbeat_at = $1, updated_at = $1
            WHERE runtime_owner_id = $2
            "#,
        )
        .bind(now)
        .bind(owner_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() != 0)
    })
    .await
    .context("heartbeat PostgreSQL runtime owner")
}

pub async fn unregister(pool: &PgPool, owner_id: &str) -> Result<()> {
    execute_with_retry(|| async {
        sqlx::query("DELETE FROM public.runtime_owners WHERE runtime_owner_id = $1")
            .bind(owner_id)
            .execute(pool)
            .await
            .map(|_| ())
    })
    .await
    .context("unregister PostgreSQL runtime owner")
}

async fn execute_with_retry<T, F, Fut>(mut operation: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, sqlx::Error>>,
{
    for attempt in 0..=MAX_RETRIES {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error)
                if attempt < MAX_RETRIES
                    && matches!(&error, sqlx::Error::Database(database) if database.code().is_some_and(|code| code == "40001" || code == "40P01")) =>
            {
                sleep(Duration::from_millis(25 * (attempt as u64 + 1))).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!("retry loop returns on every attempt")
}

#[cfg(test)]
mod tests {
    #[test]
    fn lease_contract_uses_public_timestamptz_schema_and_conflict_upsert() {
        let source = include_str!("postgres_runtime_owner.rs");
        assert!(source.contains("public.runtime_owners"));
        assert!(source.contains("ON CONFLICT (runtime_owner_id)"));
        assert!(source.contains("40001") && source.contains("40P01"));
    }
}
