//! Offline SQLite to PostgreSQL migration utility.
//!
//! The application does not use this binary at runtime. It deliberately owns
//! the one-way data movement boundary so that a failed import never mutates the
//! SQLite source and can be rerun against an isolated PostgreSQL database.

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use chrono::{DateTime, NaiveDateTime, Utc};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, TryStreamExt};
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
    /// Restrict verification to one table for bounded, resumable validation.
    #[arg(long, env = "MIGRATION_VERIFY_TABLE")]
    verify_table: Option<String>,
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
    validate_sqlite_source(&args.sqlite_source)?;
    validate_postgres_target(&args.postgres_url)?;
    let (mut pg, connection) = tokio_postgres::connect(&args.postgres_url, NoTls)
        .await
        .context("connect PostgreSQL target")?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::error!(%error, "postgres connection failed");
        }
    });

    match args.command {
        Command::Preflight => {
            let sqlite = open_sqlite_source(&args).await?;
            preflight(&sqlite, &pg, &args).await?
        }
        Command::Import => {
            let sqlite = open_sqlite_source(&args).await?;
            import(&sqlite, &mut pg, &args).await?
        }
        Command::Verify => {
            let sqlite = open_sqlite_source(&args).await?;
            verify(&sqlite, &mut pg, &args).await?
        }
        Command::Rebuild => rebuild(&mut pg).await?,
    }
    Ok(())
}

fn validate_sqlite_source(source: &str) -> Result<()> {
    if source
        .split_once(':')
        .map(|(scheme, _)| scheme.eq_ignore_ascii_case("sqlite"))
        != Some(true)
    {
        bail!("offline importer requires a SQLite source URL (got {source:?})");
    }
    Ok(())
}

fn validate_postgres_target(target: &str) -> Result<()> {
    let scheme = target
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(scheme.as_str(), "postgres" | "postgresql") {
        bail!("offline importer requires a PostgreSQL target URL (got {target:?})");
    }
    Ok(())
}

async fn open_sqlite_source(args: &Args) -> Result<SqlitePool> {
    SqlitePoolOptions::new()
        .max_connections(2)
        .connect(&args.sqlite_source)
        .await
        .context("open SQLite source")
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
    // Import throughput settings for this session only: the target is a fresh
    // validation database whose durability is re-established by ``verify`` and
    // by the production switch, so per-commit fsync is pure overhead here.
    pg.batch_execute(
        "SET synchronous_commit = off;\n         SET client_min_messages = warning;\n         SET maintenance_work_mem = '1GB';\n         SET jit = off;",
    )
    .await
    .context("apply PostgreSQL import session settings")?;
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
    pg.batch_execute("COMMIT")
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
        if let Some(reason) = skipped_source_table_reason(&name, &fts_names) {
            manifest.tables.insert(
                name,
                TableManifest {
                    rows: 0,
                    sha256: String::new(),
                    target_sha256: String::new(),
                    skipped: true,
                    reason: Some(reason.into()),
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
            for index_ddl in unique_index_ddls(sqlite, name).await? {
                pg.batch_execute(&index_ddl)
                    .await
                    .with_context(|| format!("create target unique index for {name}"))?;
            }
        }
    }
    pg.batch_execute(
        "DO $$ BEGIN
           IF EXISTS (
             SELECT 1 FROM information_schema.columns
             WHERE table_schema = 'public' AND table_name = 'tower_sessions'
               AND column_name = 'expiry_date' AND data_type = 'bigint'
           ) THEN
             ALTER TABLE public.tower_sessions
               ALTER COLUMN expiry_date TYPE timestamptz
               USING to_timestamp(expiry_date);
           END IF;
         END $$;",
    )
    .await
    .context("normalize tower session expiry type")?;
    for (name, columns, _, _) in tables {
        pg.batch_execute("BEGIN")
            .await
            .with_context(|| format!("begin copy transaction for {name}"))?;
        let table_started = std::time::Instant::now();
        // Runtime defaults must not replace the source scheduler configuration,
        // including an intentionally empty source table or disabled hour slots.
        if name == "daily_brief_hour_slots" {
            pg.batch_execute("DELETE FROM public.daily_brief_hour_slots")
                .await?;
        }
        let source_rows: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {}",
            quote_sqlite_ident(&name)
        ))
        .fetch_one(sqlite)
        .await
        .with_context(|| format!("count source table {name}"))?;
        let target_rows: i64 = pg
            .query_one(
                &format!("SELECT count(*) FROM public.{}", quote_ident(&name)),
                &[],
            )
            .await
            .with_context(|| format!("count target table {name}"))?
            .get(0);
        let copy_result = if target_rows > 0 && target_rows == source_rows {
            if canonical_source_checksum(sqlite, pg, &name, &columns)
                .await?
                .1
                != canonical_target_checksum(pg, &name, &columns).await?
            {
                pg.batch_execute("ROLLBACK").await?;
                bail!("existing target content differs from source for {name}");
            }
            Ok(source_rows as u64)
        } else if target_rows == 0 {
            copy_table_bulk(sqlite, pg, &name, &columns, args.batch_size).await
        } else {
            copy_table(sqlite, pg, &name, &columns, args.batch_size).await
        };
        let rows = match copy_result {
            Ok(result) => result,
            Err(error) => {
                let _ = pg.batch_execute("ROLLBACK").await;
                return Err(error).with_context(|| format!("copy table {name}"));
            }
        };
        // A resumed copy only inserts what is missing. If the target still holds
        // rows the source no longer has, this is a source-side delete and the
        // database must be rebuilt rather than silently diverging.
        if name != "job_task_events" {
            let settled: i64 = pg
                .query_one(
                    &format!("SELECT count(*) FROM public.{}", quote_ident(&name)),
                    &[],
                )
                .await?
                .get(0);
            if settled as u64 != rows {
                let _ = pg.batch_execute("ROLLBACK").await;
                bail!(
                    "table {name}: target keeps {settled} rows but source now has {rows}; \
                     the source deleted rows, so import into a fresh database instead of reusing this one"
                );
            }
        }
        eprintln!(
            "copied table {name}: {rows} rows in {:.1}s",
            table_started.elapsed().as_secs_f64()
        );
        // Cross-engine canonical verification owns integrity: ``verify``
        // rescans normalized values on both engines. Hashing every source row
        // during the copy would scan and serialize the same data twice.
        pg.batch_execute("COMMIT")
            .await
            .with_context(|| format!("commit copy transaction for {name}"))?;
        manifest.tables.insert(
            name,
            TableManifest {
                rows,
                sha256: String::new(),
                target_sha256: String::new(),
                skipped: false,
                reason: None,
            },
        );
    }
    import_application_views(sqlite, pg).await?;
    // Keep the runtime store's schema aligned with imported login sessions.
    pg.batch_execute("INSERT INTO octo_rill.tower_sessions(id,data,expiry_date) SELECT id,data,expiry_date FROM public.tower_sessions ON CONFLICT(id) DO UPDATE SET data=EXCLUDED.data,expiry_date=EXCLUDED.expiry_date;")
        .await.context("migrate persisted login sessions to runtime store")?;
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
    pg.batch_execute("SELECT pg_advisory_unlock(hashtextextended('octo-rill:pg18-import', 0));")
        .await?;
    println!("import complete: {}", args.manifest.display());
    Ok(())
}

/// Source tables that must never be copied into the PostgreSQL target.
///
/// The SQLite SQLx history describes migrations that have no PostgreSQL
/// equivalent; copying it makes the PostgreSQL migrator treat foreign versions
/// as applied and abort. The source version is preserved in the migration
/// manifest and `octo_rill.migration_manifests` instead.
fn skipped_source_table_reason(name: &str, fts_names: &HashSet<String>) -> Option<&'static str> {
    if fts_names.contains(name) {
        return Some("SQLite FTS5 is a rebuildable projection");
    }
    if name == "_sqlx_migrations" {
        return Some("SQLite migration history is recorded in the migration manifest");
    }
    None
}

async fn import_application_views(sqlite: &SqlitePool, pg: &Client) -> Result<()> {
    // Visibility is live relational state. Importing a view's current rows would
    // silently lose follow/unfollow and access-control changes after cutover.
    for name in [
        "user_following_repos",
        "user_release_visible_repos",
        "search_release_metadata",
    ] {
        let ddl: Option<String> =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'view' AND name = ?")
                .bind(name)
                .fetch_optional(sqlite)
                .await?;
        if let Some(ddl) = ddl {
            pg.batch_execute(&translate_view_ddl(&ddl))
                .await
                .with_context(|| format!("create PostgreSQL application view {name}"))?;
        }
    }
    Ok(())
}

fn translate_view_ddl(ddl: &str) -> String {
    let sql = ddl
        .replace("CREATE VIEW IF NOT EXISTS", "CREATE OR REPLACE VIEW")
        .replace("CREATE VIEW ", "CREATE OR REPLACE VIEW ")
        .replace("instr(", "strpos(")
        .replace(" AS INTEGER)", " AS BIGINT)");
    format!("SET search_path TO public, octo_rill; {sql};")
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
    if let Some(name) = &args.verify_table {
        if !expected.tables.get(name).is_some_and(|t| !t.skipped) {
            bail!("unknown or skipped verification table {name}");
        }
    }
    for (name, table) in expected.tables.iter().filter(|(name, t)| {
        !t.skipped
            && args
                .verify_table
                .as_ref()
                .is_none_or(|selected| selected == *name)
    }) {
        let columns = column_names(sqlite, name).await?;

        let verify_started = std::time::Instant::now();
        eprintln!("verifying table {name}");
        let (source_rows, source_content) =
            canonical_source_checksum(sqlite, pg, name, &columns).await?;
        let target_content = canonical_target_checksum(pg, name, &columns).await?;
        if source_content != target_content {
            bail!("source/target content mismatch for {name}");
        }
        let target_rows: i64 = pg
            .query_one(
                &format!("SELECT count(*) FROM public.{}", quote_ident(name)),
                &[],
            )
            .await?
            .get(0);
        let append_only_event_log = name == "job_task_events";
        if !verification_counts_match(append_only_event_log, source_rows, target_rows as u64) {
            bail!(
                "verification failed for {name}: source rows {source_rows}, target rows {target_rows}, manifest rows {}",
                table.rows
            );
        }
        // Manifests written by older importers also carried a raw source hash;
        // keep validating it when present so older manifests stay meaningful.
        if !table.sha256.is_empty() {
            let (_, source_sha) = checksum_sqlite(sqlite, name, &columns).await?;
            if source_sha != table.sha256 {
                bail!("source content changed since import for {name}");
            }
        }
        eprintln!(
            "verified table {name}: {source_rows} rows in {:.1}s",
            verify_started.elapsed().as_secs_f64()
        );
    }
    println!(
        "verification passed for {} tables",
        expected
            .tables
            .iter()
            .filter(|(name, t)| !t.skipped
                && args
                    .verify_table
                    .as_ref()
                    .is_none_or(|selected| selected == *name))
            .count()
    );
    Ok(())
}

// Compare the same normalized values on both databases, independently of
// physical row order. Sorting fixed-size row digests bounds memory by rows,
// rather than retaining potentially large source documents.
fn canonical_value(value: Option<String>, kind: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    Ok(Some(if is_timestamp_type(kind) {
        let normalized = if value.ends_with("+00") {
            format!("{value}:00")
        } else {
            value.clone()
        };
        let parsed = DateTime::parse_from_rfc3339(&normalized)
            .or_else(|_| DateTime::parse_from_str(&normalized, "%Y-%m-%d %H:%M:%S%.f%#z"))
            .or_else(|_| DateTime::parse_from_str(&normalized, "%Y-%m-%d %H:%M:%S%.f%:z"))
            .or_else(|_| {
                NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%d %H:%M:%S%.f")
                    .map(|v| v.and_utc().fixed_offset())
            })
            .with_context(|| format!("normalize timestamp {value:?}"))?;
        // PostgreSQL rounds nanoseconds to microseconds on timestamp input.
        // PostgreSQL parses the fractional seconds through float8 before
        // rint; decimal half-microseconds can sit on either side of a tie.
        let fractional = value.split_once('.').map(|(_, tail)| {
            tail.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        });
        let rounded = fractional
            .map(|digits| {
                format!("0.{digits}")
                    .parse::<f64>()
                    .map(|v| (v * 1_000_000.0).round_ties_even() as u32)
            })
            .transpose()?
            .unwrap_or(0);
        let micros = parsed
            .timestamp()
            .checked_mul(1_000_000)
            .and_then(|v| v.checked_add(i64::from(rounded)))
            .context("timestamp microsecond overflow")?;
        DateTime::<Utc>::from_timestamp_micros(micros)
            .context("timestamp outside PostgreSQL verification range")?
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
    } else if matches!(kind, Some("double precision" | "real")) {
        value.parse::<f64>()?.to_string()
    } else {
        value
    }))
}

fn finish_canonical_checksum(mut hashes: Vec<[u8; 32]>) -> String {
    hashes.sort_unstable();
    let mut digest = Sha256::new();
    for hash in hashes {
        digest.update(hash);
    }
    hex::encode(digest.finalize())
}

async fn canonical_source_checksum(
    sqlite: &SqlitePool,
    pg: &Client,
    table: &str,
    columns: &[String],
) -> Result<(u64, String)> {
    let types = target_column_types(pg, table, columns).await?;
    let query = format!("SELECT * FROM {}", quote_sqlite_ident(table));
    let mut rows = sqlx::query(&query).fetch(sqlite);
    let mut hashes = Vec::new();
    while let Some(row) = rows.try_next().await? {
        let values = row_values(&row)?
            .iter()
            .zip(&types)
            .map(|(cell, kind)| canonical_value(csv_cell(cell, kind.as_deref()), kind.as_deref()))
            .collect::<Result<Vec<_>>>()?;
        hashes.push(Sha256::digest(serde_json::to_vec(&values)?).into());
    }
    let rows = hashes.len() as u64;
    Ok((rows, finish_canonical_checksum(hashes)))
}

async fn canonical_target_checksum(pg: &Client, table: &str, columns: &[String]) -> Result<String> {
    let types = target_column_types(pg, table, columns).await?;
    let selection = columns
        .iter()
        .map(|name| format!("{}::text", quote_ident(name)))
        .collect::<Vec<_>>()
        .join(",");
    let query = format!("SELECT {selection} FROM public.{}", quote_ident(table));
    let rows = pg.query_raw(&query, std::iter::empty::<&str>()).await?;
    futures_util::pin_mut!(rows);
    let mut hashes = Vec::new();
    while let Some(row) = rows.try_next().await? {
        let values = types
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let value: Option<String> = row.try_get(index)?;
                let value = if kind.as_deref() == Some("boolean") {
                    value.map(|v| {
                        if v == "t" {
                            "true".to_owned()
                        } else if v == "f" {
                            "false".to_owned()
                        } else {
                            v
                        }
                    })
                } else {
                    value
                };
                canonical_value(value, kind.as_deref())
            })
            .collect::<Result<Vec<_>>>()?;
        hashes.push(Sha256::digest(serde_json::to_vec(&values)?).into());
    }
    Ok(finish_canonical_checksum(hashes))
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
) -> Result<u64> {
    let target_types = target_column_types(pg, table, columns).await?;
    let mut offset = 0i64;
    let mut total = 0u64;
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
            // Re-entry happens after a partial copy, so values must be coerced
            // exactly like the bulk path: SQLite cells are dynamically typed and
            // binding a Text cell to a bigint parameter aborts the import.
            let placeholders = (1..=columns.len())
                .zip(target_types.iter())
                .map(|(index, target_type)| bind_placeholder(index, target_type.as_deref()))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "INSERT INTO public.{} ({}) VALUES ({}) ON CONFLICT DO NOTHING",
                quote_ident(table),
                columns
                    .iter()
                    .map(|column| quote_ident(column))
                    .collect::<Vec<_>>()
                    .join(","),
                placeholders
            );
            let params = cells
                .iter()
                .zip(target_types.iter())
                .map(|(cell, target_type)| bind_value(cell, target_type.as_deref()))
                .collect::<Result<Vec<_>>>()?;
            let refs = params
                .iter()
                .map(|value| value.as_ref() as &(dyn ToSql + Sync))
                .collect::<Vec<_>>();
            execute_with_retry(pg, &sql, &refs)
                .await
                .with_context(|| format!("insert into {table}"))?;
            total += 1;
        }
        offset += batch_size as i64;
    }
    Ok(total)
}

/// Placeholder for a target column.
///
/// The parameter itself is always bound as text (`$n::text`), and the server
/// performs the conversion to the column type. Binding directly as the column
/// type would make the client serialize with that type, which fails for
/// SQLite's dynamically typed cells (a Text cell in a bigint column).
fn bind_placeholder(index: usize, target_type: Option<&str>) -> String {
    match target_type {
        Some("bytea") => format!("decode(${index}, 'hex')"),
        Some(data_type) if needs_text_cast(data_type) => {
            format!("${index}::text::{data_type}")
        }
        _ => format!("${index}"),
    }
}

fn needs_text_cast(data_type: &str) -> bool {
    !matches!(
        data_type,
        "text" | "character varying" | "character" | "citext" | "name"
    )
}

/// Bind a source cell as text so the placeholder cast decides the target type.
fn bind_value(cell: &Cell, target_type: Option<&str>) -> Result<Box<dyn ToSql + Sync>> {
    let value = csv_cell(cell, target_type);
    let value = match (target_type, value) {
        (Some("bytea"), Some(value)) => Some(value.trim_start_matches("\\x").to_owned()),
        (_, value) => value,
    };
    Ok(Box::new(value))
}

async fn copy_table_bulk(
    sqlite: &SqlitePool,
    pg: &mut Client,
    table: &str,
    columns: &[String],
    batch_size: usize,
) -> Result<u64> {
    let target_types = target_column_types(pg, table, columns).await?;
    let copy_sql = format!(
        "COPY public.{} ({}) FROM STDIN WITH (FORMAT csv, NULL '\\N')",
        quote_ident(table),
        columns
            .iter()
            .map(|column| quote_ident(column))
            .collect::<Vec<_>>()
            .join(",")
    );
    let sink = pg.copy_in::<_, Bytes>(&copy_sql).await?;
    futures_util::pin_mut!(sink);
    let mut total = 0u64;
    let query = format!("SELECT * FROM {}", quote_sqlite_ident(table));
    let mut rows = sqlx::query(&query).fetch(sqlite);
    let mut payload = Vec::new();
    let mut batch_rows = 0usize;
    loop {
        let Some(row) = rows.try_next().await? else {
            break;
        };
        let cells = row_values(&row)?;
        for (index, (cell, target_type)) in cells.iter().zip(target_types.iter()).enumerate() {
            if index > 0 {
                payload.push(b',');
            }
            let value = csv_cell(cell, target_type.as_deref());
            match value {
                None => payload.extend_from_slice(b"\\N"),
                Some(value) => {
                    payload.push(b'"');
                    for byte in value.bytes() {
                        if byte == b'"' {
                            payload.push(b'"');
                        }
                        payload.push(byte);
                    }
                    payload.push(b'"');
                }
            }
        }
        payload.push(b'\n');
        total += 1;
        batch_rows += 1;
        if batch_rows >= batch_size {
            sink.as_mut()
                .send(std::mem::take(&mut payload).into())
                .await?;
            batch_rows = 0;
        }
    }
    if !payload.is_empty() {
        sink.as_mut().send(payload.into()).await?;
    }
    sink.as_mut().finish().await?;
    Ok(total)
}

fn csv_cell(cell: &Cell, target_type: Option<&str>) -> Option<String> {
    match cell {
        Cell::Null => None,
        Cell::Text(value) => {
            if target_type == Some("boolean") {
                Some(
                    (matches!(
                        value.to_ascii_lowercase().as_str(),
                        "1" | "true" | "t" | "yes"
                    ))
                    .to_string(),
                )
            } else if target_type == Some("bytea") {
                Some(format!("\\x{}", hex::encode(value.as_bytes())))
            } else {
                Some(value.clone())
            }
        }
        Cell::Integer(value) => {
            if target_type == Some("boolean") {
                Some((*value != 0).to_string())
            } else {
                Some(value.to_string())
            }
        }
        Cell::Real(value) => Some(value.to_string()),
        Cell::Blob(value) => Some(format!("\\x{}", hex::encode(value))),
    }
}

fn is_timestamp_type(data_type: Option<&str>) -> bool {
    data_type.is_some_and(|value| value.starts_with("timestamp"))
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
    let mut hasher = Sha256::new();
    let query = format!("SELECT * FROM {}", quote_sqlite_ident(table));
    let mut rows = sqlx::query(&query).fetch(sqlite);
    let mut total = 0u64;
    while let Some(row) = rows.try_next().await? {
        let cells = row_values(&row)?;
        hasher.update(serde_json::to_vec(
            &cells.iter().map(cell_debug).collect::<Vec<_>>(),
        )?);
        total += 1;
    }
    let _ = columns;
    Ok((total, hex::encode(hasher.finalize())))
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

async fn unique_index_ddls(sqlite: &SqlitePool, table: &str) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT sql FROM sqlite_master WHERE type = 'index' AND tbl_name = ? AND sql IS NOT NULL AND sql LIKE 'CREATE UNIQUE INDEX%' ORDER BY name",
    )
    .bind(table)
    .fetch_all(sqlite)
    .await?;
    rows.into_iter()
        .map(|row| {
            let ddl: String = row.try_get("sql")?;
            Ok(translate_index_ddl(&ddl))
        })
        .filter(|ddl| match ddl {
            Ok(sql) => {
                let normalized = sql.to_ascii_lowercase();
                !normalized.contains("json_extract(")
                    && !normalized.contains("json_type(")
                    && !normalized.contains("ifnull(")
            }
            Err(_) => true,
        })
        .collect()
}

fn translate_index_ddl(ddl: &str) -> String {
    let mut sql = ddl.replace("CREATE UNIQUE INDEX", "CREATE UNIQUE INDEX IF NOT EXISTS");
    sql = sql.replace("COLLATE NOCASE", "");
    format!("SET search_path TO public, octo_rill; {sql};")
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
    sql = sql
        .replace("AUTOINCREMENT", "")
        .replace("autoincrement", "");
    sql = sql
        .replace("COLLATE NOCASE", "")
        .replace("collate nocase", "");
    sql = sql
        .replace(" BLOB", " BYTEA")
        .replace(" blob", " BYTEA")
        .replace(" INTEGER", " BIGINT")
        .replace(" integer", " BIGINT")
        .replace(" REAL", " DOUBLE PRECISION")
        .replace(" real", " DOUBLE PRECISION");
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
    fn timestamp_checksum_matches_postgres_microsecond_rounding() {
        let kind = Some("timestamp with time zone");
        assert_eq!(
            canonical_value(Some("2026-12-01T05:32:37.829795988Z".into()), kind).unwrap(),
            canonical_value(Some("2026-12-01 05:32:37.829796+00".into()), kind).unwrap()
        );
    }

    #[test]
    fn timestamp_ties_round_to_even_microseconds() {
        let kind = Some("timestamp with time zone");
        assert_eq!(
            canonical_value(Some("2026-05-19T09:14:47.195090500Z".into()), kind).unwrap(),
            canonical_value(Some("2026-05-19T09:14:47.195090Z".into()), kind).unwrap()
        );
    }

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
    fn sqlite_migration_history_is_never_copied() {
        let fts = HashSet::new();
        assert_eq!(
            skipped_source_table_reason("_sqlx_migrations", &fts),
            Some("SQLite migration history is recorded in the migration manifest")
        );
        assert_eq!(skipped_source_table_reason("users", &fts), None);
        let mut shadow = HashSet::new();
        shadow.insert("search_documents".to_owned());
        assert_eq!(
            skipped_source_table_reason("search_documents", &shadow),
            Some("SQLite FTS5 is a rebuildable projection")
        );
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
    fn importer_boundaries_are_explicit() {
        assert!(validate_sqlite_source("sqlite:./snapshot.db").is_ok());
        assert!(validate_sqlite_source("postgres://db/app").is_err());
        assert!(validate_postgres_target("postgresql://db/app").is_ok());
        assert!(validate_postgres_target("sqlite:./snapshot.db").is_err());
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
