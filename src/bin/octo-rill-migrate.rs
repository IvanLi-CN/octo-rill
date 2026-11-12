//! Offline SQLite to PostgreSQL migration utility.
//!
//! The application does not use this binary at runtime. It deliberately owns
//! the one-way data movement boundary so that a failed import never mutates the
//! SQLite source and can be rerun against an isolated PostgreSQL database.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool, TypeInfo, ValueRef, sqlite::SqlitePoolOptions};
use std::{collections::BTreeMap, path::PathBuf};
use tokio_postgres::{Client, NoTls, types::ToSql};

#[derive(Parser, Debug)]
#[command(
    name = "octo-rill-migrate",
    version,
    about = "Offline SQLite to PostgreSQL migration"
)]
struct Args {
    #[arg(
        long,
        env = "SQLITE_SOURCE",
        default_value = "sqlite:./.data/octo-rill.db"
    )]
    sqlite_source: String,
    #[arg(long, env = "POSTGRES_URL")]
    postgres_url: String,
    #[arg(
        long,
        env = "MIGRATION_MANIFEST",
        default_value = ".data/pg-migration-manifest.json"
    )]
    manifest: PathBuf,
    #[arg(long, env = "MIGRATION_BATCH_SIZE", default_value_t = 500)]
    batch_size: usize,
    #[arg(long, env = "MIGRATION_SAFETY_BYTES", default_value_t = 1_073_741_824)]
    safety_bytes: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check that the target filesystem has room for the estimated import.
    Preflight,
    /// Create tables and copy authoritative SQLite tables.
    Import,
    /// Compare row counts and deterministic table checksums.
    Verify,
    /// Rebuild PostgreSQL search projections after authoritative data import.
    Rebuild,
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct Manifest {
    source: String,
    target: String,
    generated_at_unix: u64,
    source_bytes: u64,
    estimated_target_bytes: u64,
    safety_bytes: u64,
    tables: BTreeMap<String, TableManifest>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct TableManifest {
    rows: u64,
    sha256: String,
    #[serde(default)]
    target_sha256: String,
    skipped: bool,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
enum Cell {
    Null,
    Text(String),
    Integer(i64),
    Real(f64),
    Blob(Vec<u8>),
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let sqlite = SqlitePoolOptions::new()
        .max_connections(2)
        .connect(&args.sqlite_source)
        .await
        .context("open SQLite source")?;
    let (mut pg, connection) = tokio_postgres::connect(&args.postgres_url, NoTls)
        .await
        .context("connect PostgreSQL target")?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::error!(%error, "postgres connection failed");
        }
    });

    match args.command {
        Command::Preflight => preflight(&sqlite, &pg, &args).await?,
        Command::Import => import(&sqlite, &mut pg, &args).await?,
        Command::Verify => verify(&sqlite, &mut pg, &args).await?,
        Command::Rebuild => rebuild(&mut pg).await?,
    }
    Ok(())
}

async fn preflight(sqlite: &SqlitePool, pg: &Client, args: &Args) -> Result<()> {
    let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(sqlite)
        .await?;
    let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
        .fetch_one(sqlite)
        .await?;
    let source_bytes = page_count.max(0) as u64 * page_size.max(0) as u64;
    let rows = sqlx::query(
        "SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(sqlite)
    .await?;
    let estimated = source_bytes
        .saturating_sub(freelist_bytes(sqlite).await?)
        .saturating_mul(2)
        .saturating_add(args.safety_bytes);
    let data_dir = pg
        .query_one("SHOW data_directory", &[])
        .await
        .context("read PostgreSQL data directory")?
        .get::<_, String>(0);
    let available = filesystem_available_bytes(&data_dir)?;
    println!(
        "source_bytes={source_bytes} authoritative_tables={} estimated_target_bytes={estimated} safety_bytes={}",
        rows.len(),
        args.safety_bytes
    );
    println!("target_data_directory={data_dir} target_free_bytes={available}");
    if estimated == 0 || available < estimated {
        bail!("capacity gate failed: target_free_bytes={available}, required_bytes={estimated}")
    }
    Ok(())
}

async fn import(sqlite: &SqlitePool, pg: &mut Client, args: &Args) -> Result<()> {
    preflight(sqlite, pg, args).await?;
    pg.batch_execute(include_str!("../../migrations_pg/0001_runtime.sql"))
        .await?;
    let mut manifest = Manifest {
        source: args.sqlite_source.clone(),
        target: redact_target(&args.postgres_url),
        generated_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        source_bytes: 0,
        estimated_target_bytes: 0,
        safety_bytes: args.safety_bytes,
        tables: BTreeMap::new(),
    };
    let table_rows = sqlx::query("SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .fetch_all(sqlite).await?;
    for table in table_rows {
        let name: String = table.try_get("name")?;
        let ddl: Option<String> = table.try_get("sql")?;
        if is_fts_table(&name, ddl.as_deref()) {
            manifest.tables.insert(
                name,
                TableManifest {
                    rows: 0,
                    sha256: String::new(),
                    target_sha256: String::new(),
                    skipped: true,
                    reason: Some("SQLite FTS5 is a rebuildable projection".into()),
                },
            );
            continue;
        }
        let columns = column_names(sqlite, &name).await?;
        if let Some(ddl) = ddl {
            let translated = translate_ddl(&ddl, &name)?;
            pg.batch_execute(&translated)
                .await
                .with_context(|| format!("create target table {name}"))?;
        }
        let (rows, sha) = copy_table(sqlite, pg, &name, &columns, args.batch_size).await?;
        let target_sha = checksum_postgres(pg, &name).await?;
        manifest.tables.insert(
            name,
            TableManifest {
                rows,
                sha256: sha,
                target_sha256: target_sha,
                skipped: false,
                reason: None,
            },
        );
    }
    let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(sqlite)
        .await?;
    let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
        .fetch_one(sqlite)
        .await?;
    manifest.source_bytes = page_count.max(0) as u64 * page_size.max(0) as u64;
    manifest.estimated_target_bytes = manifest
        .source_bytes
        .saturating_mul(2)
        .saturating_add(args.safety_bytes);
    if let Some(parent) = args.manifest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&args.manifest, serde_json::to_vec_pretty(&manifest)?)
        .await
        .with_context(|| format!("write manifest {}", args.manifest.display()))?;
    println!("import complete: {}", args.manifest.display());
    Ok(())
}

async fn verify(sqlite: &SqlitePool, pg: &mut Client, args: &Args) -> Result<()> {
    let expected: Manifest = serde_json::from_slice(&tokio::fs::read(&args.manifest).await?)?;
    for (name, table) in expected.tables.iter().filter(|(_, t)| !t.skipped) {
        let columns = column_names(sqlite, name).await?;
        let (source_rows, source_sha) = checksum_sqlite(sqlite, name, &columns).await?;
        let target_rows: i64 = pg
            .query_one(
                &format!("SELECT count(*) FROM octo_rill.{}", quote_ident(name)),
                &[],
            )
            .await?
            .get(0);
        let target_sha = checksum_postgres(pg, name).await?;
        if source_rows != target_rows as u64
            || source_sha != table.sha256
            || (!table.target_sha256.is_empty() && target_sha != table.target_sha256)
        {
            bail!(
                "verification failed for {name}: source rows/hash {source_rows}/{source_sha}, target rows/hash {target_rows}/{target_sha}, manifest source/target {}/{}",
                table.sha256,
                table.target_sha256
            );
        }
    }
    println!(
        "verification passed for {} tables",
        expected.tables.values().filter(|t| !t.skipped).count()
    );
    Ok(())
}

async fn checksum_postgres(pg: &Client, table: &str) -> Result<String> {
    let query = format!(
        "SELECT row_to_json(row)::text FROM (SELECT * FROM octo_rill.{}) AS row ORDER BY ctid",
        quote_ident(table)
    );
    let rows = pg.query(&query, &[]).await?;
    let mut hasher = Sha256::new();
    for row in rows {
        let value: String = row.get(0);
        hasher.update(value.as_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

async fn rebuild(pg: &mut Client) -> Result<()> {
    pg.batch_execute(
        "SET search_path TO octo_rill, public; CREATE EXTENSION IF NOT EXISTS pg_trgm;",
    )
    .await
    .context("enable PostgreSQL search extension")?;
    println!(
        "search projection rebuild hook completed; application-specific GIN/trigram indexes are created by PostgreSQL migrations"
    );
    Ok(())
}

async fn copy_table(
    sqlite: &SqlitePool,
    pg: &mut Client,
    table: &str,
    columns: &[String],
    batch_size: usize,
) -> Result<(u64, String)> {
    let mut offset = 0i64;
    let mut total = 0u64;
    let mut hasher = Sha256::new();
    loop {
        let query = format!(
            "SELECT * FROM {} LIMIT ? OFFSET ?",
            quote_sqlite_ident(table)
        );
        let rows = sqlx::query(&query)
            .bind(batch_size as i64)
            .bind(offset)
            .fetch_all(sqlite)
            .await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let cells = row_values(&row)?;
            let encoded = serde_json::to_vec(&cells.iter().map(cell_debug).collect::<Vec<_>>())?;
            hasher.update(&encoded);
            let placeholders = (1..=columns.len())
                .map(|i| format!("${i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "INSERT INTO octo_rill.{} ({}) VALUES ({}) ON CONFLICT DO NOTHING",
                quote_ident(table),
                columns
                    .iter()
                    .map(|c| quote_ident(c))
                    .collect::<Vec<_>>()
                    .join(","),
                placeholders
            );
            let params = cells.iter().map(to_pg_param).collect::<Vec<_>>();
            let refs = params
                .iter()
                .map(|p| p.as_ref() as &(dyn ToSql + Sync))
                .collect::<Vec<_>>();
            pg.execute(&sql, &refs)
                .await
                .with_context(|| format!("insert into {table}"))?;
            total += 1;
        }
        offset += batch_size as i64;
    }
    Ok((total, hex::encode(hasher.finalize())))
}

async fn checksum_sqlite(
    sqlite: &SqlitePool,
    table: &str,
    columns: &[String],
) -> Result<(u64, String)> {
    let rows = sqlx::query(&format!("SELECT * FROM {}", quote_sqlite_ident(table)))
        .fetch_all(sqlite)
        .await?;
    let mut hasher = Sha256::new();
    for row in &rows {
        let cells = row_values(row)?;
        hasher.update(serde_json::to_vec(
            &cells.iter().map(cell_debug).collect::<Vec<_>>(),
        )?);
    }
    let _ = columns;
    Ok((rows.len() as u64, hex::encode(hasher.finalize())))
}

async fn column_names(sqlite: &SqlitePool, table: &str) -> Result<Vec<String>> {
    Ok(
        sqlx::query(&format!("PRAGMA table_info({})", quote_sqlite_ident(table)))
            .fetch_all(sqlite)
            .await?
            .into_iter()
            .map(|r| r.try_get::<String, _>("name"))
            .collect::<std::result::Result<Vec<_>, _>>()?,
    )
}

async fn freelist_bytes(sqlite: &SqlitePool) -> Result<u64> {
    let free: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(sqlite)
        .await?;
    let page: i64 = sqlx::query_scalar("PRAGMA page_size")
        .fetch_one(sqlite)
        .await?;
    Ok(free.max(0) as u64 * page.max(0) as u64)
}

fn row_values(row: &sqlx::sqlite::SqliteRow) -> Result<Vec<Cell>> {
    (0..row.len())
        .map(|i| {
            let v = row.try_get_raw(i)?;
            if v.is_null() {
                return Ok(Cell::Null);
            }
            let type_name = v.type_info().name().to_ascii_lowercase();
            if type_name.contains("int") {
                Ok(Cell::Integer(row.try_get(i)?))
            } else if type_name.contains("real")
                || type_name.contains("floa")
                || type_name.contains("doub")
            {
                Ok(Cell::Real(row.try_get(i)?))
            } else if type_name.contains("blob") {
                Ok(Cell::Blob(row.try_get(i)?))
            } else {
                Ok(Cell::Text(row.try_get(i)?))
            }
        })
        .collect()
}

fn cell_debug(cell: &Cell) -> serde_json::Value {
    match cell {
        Cell::Null => serde_json::Value::Null,
        Cell::Text(v) => serde_json::json!({"t":v}),
        Cell::Integer(v) => serde_json::json!({"i":v}),
        Cell::Real(v) => serde_json::json!({"r":v}),
        Cell::Blob(v) => serde_json::json!({"b":hex::encode(v)}),
    }
}
fn to_pg_param(cell: &Cell) -> Box<dyn ToSql + Sync> {
    match cell {
        Cell::Null => Box::new(None::<String>),
        Cell::Text(v) => Box::new(v.clone()),
        Cell::Integer(v) => Box::new(*v),
        Cell::Real(v) => Box::new(*v),
        Cell::Blob(v) => Box::new(v.clone()),
    }
}

fn is_fts_table(name: &str, ddl: Option<&str>) -> bool {
    name.ends_with("_content")
        || name.ends_with("_data")
        || name.ends_with("_idx")
        || ddl.is_some_and(|sql| {
            sql.to_ascii_uppercase().contains("VIRTUAL TABLE")
                && sql.to_ascii_uppercase().contains("FTS")
        })
}
fn quote_ident(v: &str) -> String {
    format!("\"{}\"", v.replace('"', "\"\""))
}
fn quote_sqlite_ident(v: &str) -> String {
    quote_ident(v)
}
fn redact_target(v: &str) -> String {
    match url::Url::parse(v) {
        Ok(mut parsed) => {
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        Err(_) => "<invalid-target-url>".to_owned(),
    }
}

fn filesystem_available_bytes(path: &str) -> Result<u64> {
    use std::{ffi::CString, mem::MaybeUninit, os::unix::ffi::OsStrExt};
    let path = CString::new(std::path::Path::new(path).as_os_str().as_bytes())
        .context("invalid PostgreSQL data directory")?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("stat PostgreSQL filesystem");
    }
    let stat = unsafe { stat.assume_init() };
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}

fn translate_ddl(ddl: &str, table: &str) -> Result<String> {
    let mut sql = if ddl
        .to_ascii_uppercase()
        .contains("CREATE TABLE IF NOT EXISTS")
    {
        ddl.to_owned()
    } else {
        ddl.replace("CREATE TABLE", "CREATE TABLE IF NOT EXISTS")
    };
    sql = sql.replace("AUTOINCREMENT", "");
    sql = sql.replace("COLLATE NOCASE", "");
    sql = sql
        .replace(" BLOB", " BYTEA")
        .replace(" INTEGER", " BIGINT")
        .replace(" REAL", " DOUBLE PRECISION");
    sql = sql.replace("CURRENT_TIMESTAMP", "CURRENT_TIMESTAMP");
    if sql.to_ascii_uppercase().contains("CREATE VIRTUAL TABLE") {
        bail!("virtual table {table} must be rebuilt as a PostgreSQL projection");
    }
    Ok(format!("SET search_path TO octo_rill, public; {sql};"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_ddl_is_translated_without_sqlite_only_tokens() {
        let ddl = translate_ddl(
            "CREATE TABLE IF NOT EXISTS sample (id INTEGER PRIMARY KEY AUTOINCREMENT, body BLOB, label TEXT COLLATE NOCASE)",
            "sample",
        ).unwrap();
        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS sample"));
        assert!(ddl.contains("BIGINT"));
        assert!(ddl.contains("BYTEA"));
        assert!(!ddl.contains("AUTOINCREMENT"));
        assert!(!ddl.contains("COLLATE NOCASE"));
    }

    #[test]
    fn fts_internal_tables_are_skipped() {
        assert!(is_fts_table("search_documents_content", None));
        assert!(is_fts_table(
            "search_documents",
            Some("CREATE VIRTUAL TABLE search_documents USING fts5(body)")
        ));
        assert!(!is_fts_table("users", Some("CREATE TABLE users (id TEXT)")));
    }

    #[test]
    fn target_redaction_does_not_write_password() {
        let redacted = redact_target("postgres://user:secret@example.test:5432/db");
        assert!(!redacted.contains("secret"));
    }
}
