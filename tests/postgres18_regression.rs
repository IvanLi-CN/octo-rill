#![cfg(feature = "postgres")]

//! PostgreSQL 18 regression coverage.
//!
//! These tests intentionally use the same migration sources as the application and a
//! real PostgreSQL service. They are skipped when `DATABASE_URL` is absent so local
//! unit-test runs do not start or assume a database service.

#[path = "../src/database_migrations.rs"]
mod database_migrations;

use anyhow::{Context, Result};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{env, sync::Arc, time::Duration};
use tokio::sync::Barrier;
use uuid::Uuid;

async fn postgres_pool() -> Result<Option<PgPool>> {
    let Some(database_url) = env::var_os("DATABASE_URL") else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL 18 regression tests");
        return Ok(None);
    };
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&database_url.to_string_lossy())
        .await
        .context("connect to PostgreSQL 18 test service")?;
    Ok(Some(pool))
}

async fn migrated_pool() -> Result<Option<PgPool>> {
    let Some(pool) = postgres_pool().await? else {
        return Ok(None);
    };
    database_migrations::verify_postgres_repeatable(&pool)
        .await
        .context("apply and repeat PostgreSQL 18 migrations")?;
    Ok(Some(pool))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres18_bootstrap_is_repeatable_and_reports_server_version() -> Result<()> {
    let Some(pool) = migrated_pool().await? else {
        return Ok(());
    };

    let version = sqlx::query_scalar::<_, String>("SHOW server_version")
        .fetch_one(&pool)
        .await?;
    let version_num =
        sqlx::query_scalar::<_, i32>("SELECT current_setting('server_version_num')::int")
            .fetch_one(&pool)
            .await?;
    assert!(
        version_num >= 180000,
        "expected PostgreSQL 18, got {version}"
    );

    let migration_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = TRUE")
            .fetch_one(&pool)
            .await?;
    assert!(migration_count >= 5);
    pool.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn multiple_workers_claim_each_job_once_with_skip_locked() -> Result<()> {
    let Some(pool) = migrated_pool().await? else {
        return Ok(());
    };
    let prefix = format!("pg18-claim-{}", Uuid::new_v4());
    let now = "2026-01-01T00:00:00Z";
    for index in 0..4 {
        sqlx::query(
            "INSERT INTO job_tasks (id, task_type, status, source, payload_json, cancel_requested, created_at, updated_at) VALUES ($1, 'pg18-regression', 'queued', 'test', '{}', 0, $2, $2)",
        )
        .bind(format!("{prefix}-{index}"))
        .bind(now)
        .execute(&pool)
        .await?;
    }

    let barrier = Arc::new(Barrier::new(4));
    let mut workers = Vec::new();
    for worker in 0..4 {
        let pool = pool.clone();
        let barrier = barrier.clone();
        let prefix = prefix.clone();
        workers.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut tx = pool.begin().await?;
            let claimed = sqlx::query_scalar::<_, String>(
                "WITH candidate AS (SELECT id FROM job_tasks WHERE id LIKE $1 AND status = 'queued' ORDER BY id FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE job_tasks AS task SET status = 'running', runtime_owner_id = $2, started_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP FROM candidate WHERE task.id = candidate.id RETURNING task.id",
            )
            .bind(format!("{prefix}-%"))
            .bind(format!("worker-{worker}"))
            .fetch_optional(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok::<_, anyhow::Error>(claimed)
        }));
    }

    let mut claimed = Vec::new();
    for worker in workers {
        if let Some(id) = worker.await.context("join claim worker")?? {
            claimed.push(id);
        }
    }
    claimed.sort();
    claimed.dedup();
    assert_eq!(claimed.len(), 4, "each queued job must be claimed once");
    let running = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM job_tasks WHERE id LIKE $1 AND status = 'running'",
    )
    .bind(format!("{prefix}-%"))
    .fetch_one(&pool)
    .await?;
    assert_eq!(running, 4);
    pool.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_row_lock_serializes_conflicting_writes() -> Result<()> {
    let Some(pool) = migrated_pool().await? else {
        return Ok(());
    };
    let session_id = format!("pg18-session-{}", Uuid::new_v4());
    sqlx::query(
        "INSERT INTO tower_sessions.session (id, data, expiry_date) VALUES ($1, decode('00', 'hex'), CURRENT_TIMESTAMP + interval '1 hour')",
    )
    .bind(&session_id)
    .execute(&pool)
    .await?;

    let first_has_lock = Arc::new(tokio::sync::Notify::new());
    let release_first = Arc::new(tokio::sync::Notify::new());
    let first_pool = pool.clone();
    let first_id = session_id.clone();
    let first_has_lock_clone = first_has_lock.clone();
    let release_first_clone = release_first.clone();
    let first = tokio::spawn(async move {
        let mut tx = first_pool.begin().await?;
        sqlx::query("SELECT id FROM tower_sessions.session WHERE id = $1 FOR UPDATE")
            .bind(&first_id)
            .fetch_one(&mut *tx)
            .await?;
        first_has_lock_clone.notify_one();
        release_first_clone.notified().await;
        sqlx::query("UPDATE tower_sessions.session SET data = decode('01', 'hex') WHERE id = $1")
            .bind(&first_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok::<_, anyhow::Error>(())
    });

    first_has_lock.notified().await;
    let second_pool = pool.clone();
    let second_id = session_id.clone();
    let second = tokio::spawn(async move {
        let mut tx = second_pool.begin().await?;
        sqlx::query("SELECT id FROM tower_sessions.session WHERE id = $1 FOR UPDATE")
            .bind(&second_id)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("UPDATE tower_sessions.session SET data = decode('02', 'hex') WHERE id = $1")
            .bind(&second_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok::<_, anyhow::Error>(())
    });
    release_first.notify_one();
    first.await.context("join first session writer")??;
    second.await.context("join second session writer")??;
    let data =
        sqlx::query_scalar::<_, Vec<u8>>("SELECT data FROM tower_sessions.session WHERE id = $1")
            .bind(&session_id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        data,
        vec![2],
        "the second writer must observe the committed row"
    );
    pool.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deadlock_is_reported_as_retryable_and_transactions_can_recover() -> Result<()> {
    let Some(pool) = migrated_pool().await? else {
        return Ok(());
    };
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pg18_deadlock_probe (id TEXT PRIMARY KEY, value INTEGER NOT NULL)",
    )
    .execute(&pool)
    .await?;
    let row_a = format!("a-{}", Uuid::new_v4());
    let row_b = format!("b-{}", Uuid::new_v4());
    for id in [&row_a, &row_b] {
        sqlx::query("INSERT INTO pg18_deadlock_probe (id, value) VALUES ($1, 0)")
            .bind(id)
            .execute(&pool)
            .await?;
    }

    let barrier = Arc::new(Barrier::new(2));
    let run_worker = |first: String, second: String, pool: PgPool, barrier: Arc<Barrier>| async move {
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE pg18_deadlock_probe SET value = value + 1 WHERE id = $1")
            .bind(&first)
            .execute(&mut *tx)
            .await?;
        barrier.wait().await;
        let result = sqlx::query("UPDATE pg18_deadlock_probe SET value = value + 1 WHERE id = $1")
            .bind(&second)
            .execute(&mut *tx)
            .await;
        let code = result.err().and_then(|error| match error {
            sqlx::Error::Database(database_error) => {
                database_error.code().map(|code| code.to_string())
            }
            _ => None,
        });
        tx.rollback().await?;
        Ok::<_, anyhow::Error>(code)
    };
    let first = tokio::spawn(run_worker(
        row_a.clone(),
        row_b.clone(),
        pool.clone(),
        barrier.clone(),
    ));
    let second = tokio::spawn(run_worker(row_b, row_a, pool.clone(), barrier));
    let (first, second) = tokio::time::timeout(Duration::from_secs(10), async {
        Ok::<_, anyhow::Error>((first.await??, second.await??))
    })
    .await
    .context("deadlock detection timed out")??;
    assert!(
        [first.as_deref(), second.as_deref()].contains(&Some("40P01")),
        "one transaction must report PostgreSQL deadlock_detected (40P01), got {first:?} and {second:?}"
    );

    sqlx::query("DELETE FROM pg18_deadlock_probe WHERE id LIKE 'a-%' OR id LIKE 'b-%'")
        .execute(&pool)
        .await?;
    sqlx::query("DROP TABLE pg18_deadlock_probe")
        .execute(&pool)
        .await?;
    pool.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn search_visibility_cannot_cross_users() -> Result<()> {
    let Some(pool) = migrated_pool().await? else {
        return Ok(());
    };
    let suffix = Uuid::new_v4().simple().to_string();
    let user_a = format!("pg18-search-a-{suffix}");
    let user_b = format!("pg18-search-b-{suffix}");
    let doc_a = format!("pg18-search-doc-a-{suffix}");
    let doc_b = format!("pg18-search-doc-b-{suffix}");
    for (user_id, github_id, login) in [
        (&user_a, 8_000_001_i64, "pg18-a"),
        (&user_b, 8_000_002_i64, "pg18-b"),
    ] {
        sqlx::query("INSERT INTO users (id, github_user_id, login, created_at, updated_at) VALUES ($1, $2, $3, $4, $4)")
            .bind(user_id)
            .bind(github_id + i64::from(u32::from_str_radix(&suffix[..6], 16).unwrap_or(0)))
            .bind(login)
            .bind("2026-01-01T00:00:00Z")
            .execute(&pool)
            .await?;
    }
    for (doc_id, user_id) in [(&doc_a, &user_a), (&doc_b, &user_b)] {
        sqlx::query("INSERT INTO search_documents (id, user_id, resource_type, resource_id, title, body, created_at, updated_at) VALUES ($1, $2, 'repository', $1, 'shared marker', 'private marker', $3, $3)")
            .bind(doc_id)
            .bind(user_id)
            .bind("2026-01-01T00:00:00Z")
            .execute(&pool)
            .await?;
    }
    let visible = sqlx::query_scalar::<_, String>(
        "SELECT id FROM search_documents WHERE user_id = $1 AND (lower(title) LIKE '%marker%' OR lower(body) LIKE '%marker%')",
    )
    .bind(&user_a)
    .fetch_all(&pool)
    .await?;
    assert_eq!(visible, vec![doc_a]);
    pool.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_transaction_rolls_back_and_allows_recovery() -> Result<()> {
    let Some(pool) = migrated_pool().await? else {
        return Ok(());
    };
    let table_name = format!("pg18_recovery_{}", Uuid::new_v4().simple());
    let mut tx = pool.begin().await?;
    sqlx::query(&format!(
        "CREATE TABLE {table_name} (id integer primary key)"
    ))
    .execute(&mut *tx)
    .await?;
    let failure = sqlx::query("SELECT 1 / 0").execute(&mut *tx).await;
    assert!(failure.is_err());
    tx.rollback().await?;
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM pg_class WHERE relname = $1)")
            .bind(&table_name)
            .fetch_one(&pool)
            .await?;
    assert!(
        !exists,
        "failed migration transaction must leave no partial table"
    );
    pool.close().await;
    Ok(())
}
