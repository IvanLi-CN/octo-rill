mod admin_ai_records;
mod admin_runtime;
mod ai;
mod api;
mod api_keys;
mod auth;
mod briefs;
mod config;
mod content_identity_upgrade;
mod content_processing;
mod crypto;
mod database_migrations;
mod error;
mod github;
mod jobs;
mod linuxdo;
mod local_id;
mod metrics;
mod observability;
mod online_migrations;
mod passkeys;
mod postgres_database_migrations;
mod postgres_runtime;
mod public_metrics;
mod release_links;
mod runtime;
mod search;
mod search_index;
mod server;
mod session_store;
mod sqlite_write;
mod state;
mod sync;
mod translations;
mod version;
mod webhook_push;
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
        let runtime = postgres_runtime::connect(
            &config.database_url,
            config.database_pool_max_connections as u32,
        )
        .await?;
        tracing::info!("PostgreSQL runtime baseline and session store are ready");
        runtime.pool.close().await;
        return Ok(());
    }
    server::serve(config).await
}
