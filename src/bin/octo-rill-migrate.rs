//! Offline SQLite to PostgreSQL migration utility.
//!
//! The application does not use this binary at runtime. It deliberately owns
//! the one-way data movement boundary so that a failed import never mutates the
//! SQLite source and can be rerun against an isolated PostgreSQL database.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool, TypeInfo, ValueRef, sqlite::SqlitePoolOptions};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
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
    /// Filesystem path visible to the migration process for the PostgreSQL data volume.
    /// Set this when PostgreSQL runs in a separate container and its data directory
    /// is not visible in the migration container.
    #[arg(long, env = "MIGRATION_TARGET_PATH")]
    target_path: Option<PathBuf>,
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
    Timestamp(Option<DateTime<Utc>>),
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
    let server_version_num: i64 = pg
        .query_one("SHOW server_version_num", &[])
        .await
        .context("read PostgreSQL server_version_num")?
        .get::<_, String>(0)
        .parse()
        .context("parse PostgreSQL server_version_num")?;
    if server_version_num < 180000 {
        bail!("PostgreSQL 18 is required; server_version_num={server_version_num}");
    }
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
    let reported_data_dir = pg
        .query_one("SHOW data_directory", &[])
        .await
        .context("read PostgreSQL data directory")?
        .get::<_, String>(0);
    let target_path = args
        .target_path
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new(&reported_data_dir));
    let available = filesystem_available_bytes(target_path)?;
    println!(
        "server_version_num={server_version_num} source_bytes={source_bytes} authoritative_tables={} estimated_target_bytes={estimated} safety_bytes={}",
        rows.len(),
        args.safety_bytes
    );
    println!(
        "target_data_directory={reported_data_dir} target_filesystem_path={} target_free_bytes={available}",
        target_path.display()
    );
    if estimated == 0 || available < estimated {
        bail!("capacity gate failed: target_free_bytes={available}, required_bytes={estimated}")
    }
    Ok(())
}

async fn import(sqlite: &SqlitePool, pg: &mut Client, args: &Args) -> Result<()> {
    preflight(sqlite, pg, args).await?;
    validate_authoritative_source_tables(sqlite).await?;
    pg.batch_execute(
        "SELECT pg_advisory_lock(hashtextextended('octo-rill:pg18-import', 0)); BEGIN;",
    )
    .await
    .context("acquire PostgreSQL migration lock")?;
    // The PostgreSQL runtime migrations contain foreign keys to the legacy
    // users table. Create that authoritative table from the snapshot before
    // applying the runtime baseline; the full table pass below remains
    // idempotent and will reuse this definition.
    let users_ddl: Option<String> =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'users'")
            .fetch_one(sqlite)
            .await
            .context("read users table definition")?;
    let users_ddl =
        users_ddl.ok_or_else(|| anyhow::anyhow!("users table definition is missing"))?;
    pg.batch_execute(&translate_ddl(&users_ddl, "users")?)
        .await
        .context("create PostgreSQL users prerequisite")?;
    let baseline_result = pg
        .batch_execute(concat!(
            include_str!("../../migrations_pg/0001_runtime.sql"),
            "\n",
            include_str!("../../migrations_pg/0002_task_runtime.sql"),
            "\n",
            include_str!("../../migrations_pg/0003_runtime_owners.sql"),
            "\n",
            include_str!("../../migrations_pg/0004_search_projection.sql"),
            "\n",
            include_str!("../../migrations_pg/0005_scheduler_slots.sql"),
            "\n",
            include_str!("../../migrations_pg/0006_content_processing.sql"),
            "\n",
            include_str!("../../migrations_pg/0007_translation_runtime.sql"),
            "\n",
            include_str!("../../migrations_pg/0008_llm_translation_audit.sql"),
            "\n",
            include_str!("../../migrations_pg/0009_brief_runtime.sql")
        ))
        .await;
    if let Err(error) = baseline_result {
        let _ = pg.batch_execute("ROLLBACK; SELECT pg_advisory_unlock(hashtextextended('octo-rill:pg18-import', 0));").await;
        return Err(error.into());
    }
    pg.batch_execute(
        "COMMIT; SELECT pg_advisory_unlock(hashtextextended('octo-rill:pg18-import', 0));",
    )
    .await
    .context("commit PostgreSQL runtime baseline")?;
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
    let fts_names = fts_shadow_table_names(&table_rows)?;
    let mut tables = Vec::new();
    for table in table_rows {
        let name: String = table.try_get("name")?;
        let ddl: Option<String> = table.try_get("sql")?;
        if fts_names.contains(&name) {
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
            let dependencies = referenced_tables(&name, &ddl);
            tables.push((name, columns, translated, dependencies));
        } else {
            tables.push((name, columns, String::new(), Vec::new()));
        }
    }
    let tables = order_tables_by_dependencies(tables)?;
    for (name, _, ddl, _) in &tables {
        if !ddl.is_empty() {
            pg.batch_execute(ddl)
                .await
                .with_context(|| format!("create target table {name}"))?;
        }
    }
    for (name, columns, _, _) in tables {
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
    let manifest_json = serde_json::to_value(&manifest).context("serialize migration manifest")?;
    pg.execute(
        "INSERT INTO octo_rill.migration_manifests (source_snapshot, manifest) VALUES ($1, $2)",
        &[&args.sqlite_source, &manifest_json],
    )
    .await
    .context("persist migration manifest")?;
    println!("import complete: {}", args.manifest.display());
    Ok(())
}

async fn validate_authoritative_source_tables(sqlite: &SqlitePool) -> Result<()> {
    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('users', 'github_connections', 'job_tasks', 'content_work_items', 'translation_work_items')",
    )
    .fetch_all(sqlite)
    .await
    .context("inspect authoritative SQLite tables")?;
    let found = rows
        .into_iter()
        .map(|row| row.try_get::<String, _>("name"))
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    let missing = missing_authoritative_tables(&found);
    if !missing.is_empty() {
        bail!(
            "authoritative SQLite snapshot is incomplete; missing tables: {}",
            missing.join(", ")
        );
    }
    Ok(())
}

fn missing_authoritative_tables(found: &HashSet<String>) -> Vec<&'static str> {
    [
        "users",
        "github_connections",
        "job_tasks",
        "content_work_items",
        "translation_work_items",
    ]
    .into_iter()
    .filter(|name| !found.contains(*name))
    .collect()
}

async fn verify(sqlite: &SqlitePool, pg: &mut Client, args: &Args) -> Result<()> {
    let expected: Manifest = serde_json::from_slice(&tokio::fs::read(&args.manifest).await?)?;
    for (name, table) in expected.tables.iter().filter(|(_, t)| !t.skipped) {
        let columns = column_names(sqlite, name).await?;
        let (source_rows, source_sha) = checksum_sqlite(sqlite, name, &columns).await?;
        let target_rows: i64 = pg
            .query_one(
                &format!("SELECT count(*) FROM public.{}", quote_ident(name)),
                &[],
            )
            .await?
            .get(0);
        let target_sha = checksum_postgres(pg, name).await?;
        let append_only_event_log = name == "job_task_events";
        if !verification_counts_match(append_only_event_log, source_rows, target_rows as u64)
            || source_sha != table.sha256
            || (!append_only_event_log
                && !table.target_sha256.is_empty()
                && target_sha != table.target_sha256)
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

fn verification_counts_match(
    append_only_event_log: bool,
    source_rows: u64,
    target_rows: u64,
) -> bool {
    if append_only_event_log {
        target_rows >= source_rows
    } else {
        source_rows == target_rows
    }
}

async fn checksum_postgres(pg: &Client, table: &str) -> Result<String> {
    let query = format!(
        "SELECT row_to_json(row)::text FROM (SELECT * FROM public.{} ORDER BY ctid) AS row",
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
        "SELECT pg_advisory_lock(hashtextextended('octo-rill:pg18-rebuild', 0));\
         CREATE EXTENSION IF NOT EXISTS pg_trgm;\
         ANALYZE public.search_documents;\
         ANALYZE public.search_document_user_lanes;\
         REINDEX INDEX public.idx_search_documents_pg_vector;\
         REINDEX INDEX public.idx_search_lanes_pg_vector;\
         SELECT pg_advisory_unlock(hashtextextended('octo-rill:pg18-rebuild', 0));",
    )
    .await
    .context("rebuild PostgreSQL search projection")?;
    println!("search projection rebuilt: generated tsvectors analyzed and GIN indexes reindexed");
    Ok(())
}

async fn copy_table(
    sqlite: &SqlitePool,
    pg: &mut Client,
    table: &str,
    columns: &[String],
    batch_size: usize,
) -> Result<(u64, String)> {
    let target_types = target_column_types(pg, table, columns).await?;
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
                .zip(target_types.iter())
                .map(|(i, target_type)| {
                    if target_type.as_deref() == Some("timestamp with time zone") {
                        format!("${i}::timestamptz")
                    } else {
                        format!("${i}")
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "INSERT INTO public.{} ({}) VALUES ({}) ON CONFLICT DO NOTHING",
                quote_ident(table),
                columns
                    .iter()
                    .map(|c| quote_ident(c))
                    .collect::<Vec<_>>()
                    .join(","),
                placeholders
            );
            let params = cells
                .iter()
                .zip(target_types.iter())
                .map(|(cell, target_type)| -> Result<Box<dyn ToSql + Sync>> {
                    if target_type.as_deref() == Some("timestamp with time zone") {
                        Ok(to_pg_param(&cell_as_timestamp(cell)?))
                    } else {
                        Ok(to_pg_param(cell))
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let refs = params
                .iter()
                .map(|p| p.as_ref() as &(dyn ToSql + Sync))
                .collect::<Vec<_>>();
            execute_with_retry(pg, &sql, &refs)
                .await
                .with_context(|| format!("insert into {table}"))?;
            total += 1;
        }
        offset += batch_size as i64;
    }
    Ok((total, hex::encode(hasher.finalize())))
}

async fn target_column_types(
    pg: &Client,
    table: &str,
    columns: &[String],
) -> Result<Vec<Option<String>>> {
    let rows = pg
        .query(
            "SELECT column_name, data_type FROM information_schema.columns WHERE table_schema = 'public' AND table_name = $1",
            &[&table],
        )
        .await
        .with_context(|| format!("inspect PostgreSQL target columns for {table}"))?;
    let types = rows
        .into_iter()
        .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
        .collect::<std::collections::HashMap<_, _>>();
    Ok(columns
        .iter()
        .map(|column| types.get(column).cloned())
        .collect())
}

async fn execute_with_retry(pg: &Client, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<u64> {
    for attempt in 0..=3 {
        match pg.execute(sql, params).await {
            Ok(affected) => return Ok(affected),
            Err(error)
                if attempt < 3
                    && error
                        .code()
                        .is_some_and(|code| matches!(code.code(), "40001" | "40P01")) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(25 * (attempt + 1))).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!("retry loop returns on every attempt")
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
        Cell::Timestamp(v) => serde_json::json!({"t":v.map(|value| value.to_rfc3339())}),
        Cell::Integer(v) => serde_json::json!({"i":v}),
        Cell::Real(v) => serde_json::json!({"r":v}),
        Cell::Blob(v) => serde_json::json!({"b":hex::encode(v)}),
    }
}

fn cell_as_timestamp(cell: &Cell) -> Result<Cell> {
    match cell {
        Cell::Null => Ok(Cell::Timestamp(None)),
        Cell::Text(value) => Ok(Cell::Timestamp(Some(
            DateTime::parse_from_rfc3339(value)
                .with_context(|| format!("parse UTC timestamp {value:?}"))?
                .with_timezone(&Utc),
        ))),
        other => Ok(other.clone()),
    }
}

fn to_pg_param(cell: &Cell) -> Box<dyn ToSql + Sync> {
    match cell {
        Cell::Null => Box::new(None::<String>),
        Cell::Text(v) => Box::new(v.clone()),
        Cell::Timestamp(v) => Box::new(*v),
        Cell::Integer(v) => Box::new(*v),
        Cell::Real(v) => Box::new(*v),
        Cell::Blob(v) => Box::new(v.clone()),
    }
}

fn is_fts_table(name: &str, ddl: Option<&str>) -> bool {
    let _ = name;
    ddl.is_some_and(|sql| {
        sql.to_ascii_uppercase().contains("VIRTUAL TABLE")
            && sql.to_ascii_uppercase().contains("FTS")
    })
}

fn fts_shadow_table_names(rows: &[sqlx::sqlite::SqliteRow]) -> Result<HashSet<String>> {
    let mut names = HashSet::new();
    for row in rows {
        let name: String = row.try_get("name")?;
        let ddl: Option<String> = row.try_get("sql")?;
        if is_fts_table(&name, ddl.as_deref()) {
            names.insert(name.clone());
            for suffix in ["_content", "_data", "_idx", "_docsize", "_config"] {
                names.insert(format!("{name}{suffix}"));
            }
        }
    }
    Ok(names)
}

fn referenced_tables(table_name: &str, ddl: &str) -> Vec<String> {
    let tokens = ddl.split_whitespace().collect::<Vec<_>>();
    let mut references = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !token.eq_ignore_ascii_case("REFERENCES") {
            continue;
        }
        let Some(reference) = tokens.get(index + 1) else {
            continue;
        };
        let name = reference
            .split('(')
            .next()
            .unwrap_or(reference)
            .trim_matches(|character| {
                character == '"' || character == '`' || character == '[' || character == ']'
            })
            .to_owned();
        if !name.is_empty() && !name.eq_ignore_ascii_case(table_name) {
            references.push(name);
        }
    }
    references
}

type TableSpec = (String, Vec<String>, String, Vec<String>);

fn order_tables_by_dependencies(tables: Vec<TableSpec>) -> Result<Vec<TableSpec>> {
    let mut pending = tables
        .into_iter()
        .map(|table| (table.0.clone(), table))
        .collect::<BTreeMap<_, _>>();
    let mut ordered = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let Some(name) = pending
            .iter()
            .find(|(_, (_, _, _, dependencies))| {
                dependencies
                    .iter()
                    .all(|dependency| !pending.contains_key(dependency))
            })
            .map(|(name, _)| name.clone())
        else {
            let cycle = pending.keys().cloned().collect::<Vec<_>>().join(", ");
            bail!("cyclic SQLite foreign-key dependencies cannot be created safely: {cycle}");
        };
        if let Some((_, table)) = pending.remove_entry(&name) {
            ordered.push(table);
        }
    }
    Ok(ordered)
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

fn filesystem_available_bytes(path: &std::path::Path) -> Result<u64> {
    use std::{ffi::CString, mem::MaybeUninit, os::unix::ffi::OsStrExt};
    let path =
        CString::new(path.as_os_str().as_bytes()).context("invalid PostgreSQL data directory")?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("stat PostgreSQL filesystem");
    }
    let stat = unsafe { stat.assume_init() };
    let available = u128::from(stat.f_bavail) * u128::from(stat.f_frsize);
    Ok(u64::try_from(available).unwrap_or(u64::MAX))
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
    if sql.to_ascii_uppercase().contains("CREATE VIRTUAL TABLE") {
        bail!("virtual table {table} must be rebuilt as a PostgreSQL projection");
    }
    // Application queries use PostgreSQL's public schema. The control tables
    // remain in octo_rill, so every generated DDL explicitly puts public
    // first while retaining the control schema as a fallback.
    Ok(format!("SET search_path TO public, octo_rill; {sql};"))
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
        assert!(ddl.contains("SET search_path TO public, octo_rill"));
    }

    #[test]
    fn fts_internal_tables_are_skipped() {
        assert!(!is_fts_table("search_documents_content", None));
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

    #[test]
    fn authoritative_snapshot_gate_reports_missing_core_tables() {
        let found = HashSet::from(["users".to_owned()]);
        assert_eq!(
            missing_authoritative_tables(&found),
            vec![
                "github_connections",
                "job_tasks",
                "content_work_items",
                "translation_work_items",
            ]
        );
    }

    #[test]
    fn append_only_event_log_accepts_runtime_rows_after_snapshot() {
        assert!(verification_counts_match(true, 0, 2));
        assert!(verification_counts_match(true, 4, 6));
        assert!(!verification_counts_match(true, 4, 3));
        assert!(!verification_counts_match(false, 4, 5));
        assert!(verification_counts_match(false, 4, 4));
    }

    #[test]
    fn referenced_tables_are_created_before_dependents() {
        let tables = vec![
            (
                "notes".to_owned(),
                vec!["id".to_owned()],
                "notes ddl".to_owned(),
                vec!["users".to_owned()],
            ),
            (
                "users".to_owned(),
                vec!["id".to_owned()],
                "users ddl".to_owned(),
                Vec::new(),
            ),
        ];
        let ordered = order_tables_by_dependencies(tables).unwrap();
        assert_eq!(ordered[0].0, "users");
        assert_eq!(ordered[1].0, "notes");
    }

    #[test]
    fn self_referencing_foreign_keys_do_not_form_a_dependency_cycle() {
        assert!(
            referenced_tables(
                "job_tasks",
                "FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id)"
            )
            .is_empty()
        );
    }

    #[test]
    fn cyclic_foreign_key_dependencies_fail_closed() {
        let tables = vec![
            (
                "a".to_owned(),
                Vec::new(),
                "a ddl".to_owned(),
                vec!["b".to_owned()],
            ),
            (
                "b".to_owned(),
                Vec::new(),
                "b ddl".to_owned(),
                vec!["a".to_owned()],
            ),
        ];
        let error = order_tables_by_dependencies(tables).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cyclic SQLite foreign-key dependencies")
        );
    }
}
