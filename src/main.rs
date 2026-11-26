// The runtime serves PostgreSQL only. These modules keep the legacy SQLite
// implementation and its operators readable until the deletion backlog
// (仓库外运维记录) removes them; nothing in them
// is reachable from `main`, so their absence of callers is expected rather
// than dead code to delete in the persistence cutover.
#[allow(dead_code)]
mod admin_ai_records;
mod admin_job_groups;
#[allow(dead_code)]
mod admin_runtime;
#[allow(dead_code)]
mod ai;
#[allow(dead_code)]
mod api;
#[allow(dead_code)]
mod api_keys;
#[allow(dead_code)]
mod auth;
#[allow(dead_code)]
mod briefs;
mod config;
#[allow(dead_code)]
mod content_identity_upgrade;
#[allow(dead_code)]
mod content_processing;
mod content_source_identity;
mod crypto;
#[allow(dead_code)]
mod database_migrations;
#[allow(dead_code)]
mod error;
#[allow(dead_code)]
mod github;
mod github_api;
#[allow(dead_code)]
mod jobs;
#[allow(dead_code)]
mod linuxdo;
mod llm_contract;
mod local_id;
mod markdown;
#[allow(dead_code)]
mod metrics;
mod observability;
#[allow(dead_code)]
mod online_migrations;
#[allow(dead_code)]
mod passkeys;
mod postgres_admin_actions;
mod postgres_admin_ai_records;
mod postgres_admin_dashboard;
mod postgres_admin_jobs;
mod postgres_admin_llm;
mod postgres_auth_ext;
mod postgres_content;
mod postgres_content_overlay;
mod postgres_dashboard;
mod postgres_database_migrations;
mod postgres_jobs;
mod postgres_reactions;
mod postgres_repo_release_fetch;
mod postgres_repo_release_worker;
mod postgres_repo_releases;
mod postgres_runtime;
mod postgres_runtime_owner;
mod postgres_scheduler;
mod postgres_server;
mod postgres_star_epochs;
mod postgres_tasks;
mod postgres_translate;
mod postgres_translation_backfill;
mod postgres_webhook;
pub mod postgres_worker;
mod postgres_write;
#[allow(dead_code)]
mod public_metrics;
mod release_content;
#[allow(dead_code)]
mod release_links;
#[allow(dead_code)]
mod runtime;
mod search;
#[allow(dead_code)]
mod search_index;
#[allow(dead_code)]
mod server;
#[allow(dead_code)]
mod session_store;
#[allow(dead_code)]
mod sqlite_write;
#[allow(dead_code)]
mod state;
#[allow(dead_code)]
mod sync;
mod translation_admin_types;
mod translation_error;
mod translation_runtime;
mod translation_types;
#[allow(dead_code)]
mod translations;
mod version;
#[allow(dead_code)]
mod webhook_push;
#[allow(dead_code)]
mod worker_backoff;

use anyhow::Result;
use dotenvy::{dotenv, from_filename};

#[tokio::main]
async fn main() -> Result<()> {
    from_filename(".env.local").ok();
    dotenv().ok();
    observability::init_tracing();

    let config = config::AppConfig::from_env()?;
    if std::env::var("OCTORILL_PG_BOOTSTRAP_ONLY").as_deref() == Ok("1") {
        if config.database_backend()? != config::DatabaseBackend::Postgres {
            anyhow::bail!("OCTORILL_PG_BOOTSTRAP_ONLY requires a postgres DATABASE_URL");
        }
        let runtime = postgres_runtime::connect_from_config(&config).await?;
        if std::env::var("OCTORILL_PG_TASK_CLAIM_SMOKE").as_deref() == Ok("1") {
            let owner_id = "bootstrap-smoke";
            postgres_runtime_owner::register(&runtime.pool, owner_id, chrono::Utc::now()).await?;
            let heartbeat =
                postgres_runtime_owner::heartbeat(&runtime.pool, owner_id, chrono::Utc::now())
                    .await?;
            tracing::info!(heartbeat, "PostgreSQL runtime owner lease is ready");
            if std::env::var("OCTORILL_PG_TASK_FINALIZE_SMOKE").as_deref() == Ok("1") {
                let registry = postgres_worker::TaskExecutorRegistry::builder()
                    .register("sync.releases", postgres_worker::BootstrapSmokeExecutor)
                    .build();
                let ran = runtime
                    .run_registered_task_once(owner_id, &registry)
                    .await?;
                tracing::info!(ran, "PostgreSQL task execution lifecycle is ready");
            } else {
                let task = runtime
                    .claim_next_queued_task(owner_id, chrono::Utc::now())
                    .await?;
                tracing::info!(
                    claimed = task.is_some(),
                    "PostgreSQL task claim contract is ready"
                );
            }
        }
        if let Ok(lane) = std::env::var("OCTORILL_PG_SCHEDULER_TICK") {
            let now = match std::env::var("OCTORILL_PG_SCHEDULER_NOW") {
                Ok(raw) => {
                    chrono::DateTime::parse_from_rfc3339(raw.trim())?.with_timezone(&chrono::Utc)
                }
                Err(_) => chrono::Utc::now(),
            };
            let defaults = postgres_scheduler::SchedulerDefaults::from_config(&config);
            let enqueued =
                postgres_scheduler::run_lane_once(&runtime.pool, lane.trim(), &defaults, now)
                    .await?;
            tracing::info!(lane = %lane, enqueued, "PostgreSQL scheduler lane tick applied");
        }
        tracing::info!("PostgreSQL runtime baseline and session store are ready");
        runtime.pool.close().await;
        return Ok(());
    }
    // One-shot repair: republish finished translation outputs as projections
    // without running any new attempt, then exit without serving traffic.
    if std::env::var(postgres_translation_backfill::MODE_ENV).as_deref() == Ok("1") {
        if config.database_backend()? != config::DatabaseBackend::Postgres {
            anyhow::bail!(
                "{} requires a postgres DATABASE_URL",
                postgres_translation_backfill::MODE_ENV
            );
        }
        let runtime = postgres_runtime::connect_from_config(&config).await?;
        let report = postgres_translation_backfill::run_from_env(&runtime.pool).await?;
        tracing::info!(
            scanned = report.scanned,
            published = report.published,
            already_current = report.already_current,
            skipped_source_hash_mismatch = report.skipped_source_hash_mismatch,
            skipped_output_contract = report.skipped_output_contract,
            skipped_missing_release = report.skipped_missing_release,
            skipped_incomplete_source = report.skipped_incomplete_source,
            failed = report.failed,
            out_of_scope = ?report.skipped_out_of_scope,
            "translation projection backfill is complete"
        );
        runtime.pool.close().await;
        return Ok(());
    }
    // PostgreSQL is the only production runtime. The SQLite modules remain
    // compiled for the offline importer and its regression suites, but a
    // SQLite DATABASE_URL must fail closed instead of serving traffic.
    config.require_postgres()?;
    postgres_server::serve_postgres(config).await
}
