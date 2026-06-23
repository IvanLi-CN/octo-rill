mod admin_runtime;
mod ai;
mod api;
mod auth;
mod briefs;
mod config;
mod crypto;
mod error;
mod github;
mod jobs;
mod linuxdo;
mod local_id;
mod observability;
mod passkeys;
mod release_links;
mod runtime;
mod server;
mod session_store;
mod sqlite_write;
mod state;
mod sync;
mod translations;
mod version;

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
