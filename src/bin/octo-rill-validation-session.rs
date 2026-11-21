//! Isolated validation helper. Never built into the application entry point.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use sqlx::PgPool;
use std::{collections::HashMap, path::PathBuf};
use tower_sessions::{
    SessionStore,
    session::{Id, Record},
};
use tower_sessions_sqlx_store::PostgresStore;

#[derive(Parser)]
struct Args {
    #[arg(long, env = "POSTGRES_URL")]
    postgres_url: String,
    #[arg(long)]
    user_id: String,
    #[arg(long)]
    cookie_file: PathBuf,
    /// Explicit acknowledgement that this database is a disposable validation copy.
    #[arg(long)]
    isolated_validation_copy: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.isolated_validation_copy,
        "requires --isolated-validation-copy"
    );
    let pool = PgPool::connect(&args.postgres_url)
        .await
        .context("connect validation database")?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM public.users WHERE id=$1 AND is_disabled=0)",
    )
    .bind(&args.user_id)
    .fetch_one(&pool)
    .await?;
    ensure!(exists, "validation user is missing or disabled");
    let store = PostgresStore::new(pool.clone())
        .with_schema_name("octo_rill")
        .map_err(anyhow::Error::msg)?
        .with_table_name("tower_sessions")
        .map_err(anyhow::Error::msg)?;
    store.migrate().await?;
    let mut record = Record {
        id: Id::default(),
        data: HashMap::from([("user_id".to_owned(), serde_json::json!(args.user_id))]),
        expiry_date: time::OffsetDateTime::now_utc() + time::Duration::hours(2),
    };
    store
        .create(&mut record)
        .await
        .map_err(anyhow::Error::msg)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.cookie_file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    use std::io::Write;
    writeln!(&file, "octo_rill_sid={}", record.id)?;
    println!("validation session written to private cookie file");
    pool.close().await;
    Ok(())
}
