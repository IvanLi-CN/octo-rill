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
mod observability;
mod passkeys;
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

use anyhow::Result;
use dotenvy::{dotenv, from_filename};

#[tokio::main]
async fn main() -> Result<()> {
    from_filename(".env.local").ok();
    dotenv().ok();
    observability::init_tracing();

    let config = config::AppConfig::from_env()?;
    server::serve(config).await
}
