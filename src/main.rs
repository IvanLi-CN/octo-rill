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
mod release_links;
mod runtime;
mod server;
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
    init_tracing();

    let config = config::AppConfig::from_env()?;
    server::serve(config).await
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .with_target(false)
        .init();
}
