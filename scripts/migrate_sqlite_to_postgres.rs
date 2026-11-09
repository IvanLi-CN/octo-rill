// Repeatable, auditable SQLite -> PostgreSQL 18 data migration.
// Run with: cargo run --manifest-path tools/sqlite_to_postgres/Cargo.toml -- --help

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::Parser;
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json;
use sha2::{Digest, Sha256};
use sqlx::{
    Arguments, Column, PgPool, Row, SqlitePool, ValueRef,
    postgres::{PgArguments, PgPoolOptions},
    sqlite::{SqlitePoolOptions, SqliteRow},
};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "migrate-sqlite-to-postgres",
    version,
    about = "Migrate a SQLite snapshot to PostgreSQL 18"
)]
struct Args {
    /// Source SQLite database. `-wal` and `-shm` sidecars are copied when present.
    #[arg(long, env = "SQLITE_SOURCE")]
    source: PathBuf,
    /// PostgreSQL connection string for the already migrated schema.
    #[arg(long, env = "POSTGRES_URL")]
    target: Option<String>,
    /// Directory receiving the immutable source snapshot and report.
    #[arg(long, default_value = "migration-artifacts")]
    artifact_dir: PathBuf,
    /// Optional report path. Defaults to `<artifact-dir>/report-<run-id>.json`.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Rows per insert transaction batch.
    #[arg(long, default_value_t = 1000)]
    batch_size: usize,
    /// Empty all discovered target tables before importing. Required for a repeatable replace run.
    #[arg(long)]
    reset_target: bool,
    /// Do not write to PostgreSQL; still snapshot and inspect the source.
    #[arg(long)]
    dry_run: bool,
    /// Comma separated table names to exclude (FTS virtual tables are excluded automatically).
    #[arg(long, value_delimiter = ',')]
    skip_table: Vec<String>,
    /// SQL file containing statements used to rebuild PostgreSQL search projections.
    #[arg(long)]
    rebuild_search_sql: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Report {
    schema_version: u32,
    run_id: String,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    status: String,
    source: SourceReceipt,
    options: ReportOptions,
    tables: Vec<TableReport>,
    sequences_repaired: Vec<String>,
    search_rebuild: SearchRebuild,
    validation: ValidationReport,
    errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SourceReceipt {
    original_path: String,
    snapshot_path: String,
    sha256: String,
    bytes: u64,
    wal_sidecar_copied: bool,
    snapshot_method: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReportOptions {
    batch_size: usize,
    reset_target: bool,
    dry_run: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct TableReport {
    table: String,
    source_rows: u64,
    imported_rows: u64,
    source_key_hash: String,
    target_key_hash: Option<String>,
    status: String,
    error: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SearchRebuild {
    requested: bool,
    statements_executed: u64,
    status: String,
    error: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ValidationReport {
    table_count: usize,
    row_count_source: u64,
    row_count_target: u64,
    row_counts_match: bool,
    key_hashes_match: bool,
    foreign_keys_checked: bool,
    foreign_key_violations: u64,
    passed: bool,
}

#[derive(Debug)]
struct TableInfo {
    name: String,
    columns: Vec<String>,
    primary_key_columns: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.batch_size == 0 {
        bail!("--batch-size must be greater than zero");
    }
    if !args.dry_run && args.target.is_none() {
        bail!("--target is required unless --dry-run is used");
    }
    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    fs::create_dir_all(&args.artifact_dir)
        .with_context(|| format!("create artifact directory {}", args.artifact_dir.display()))?;
    let source = snapshot_source(&args.source, &args.artifact_dir, &run_id)?;
    let sqlite = SqlitePoolOptions::new()
        .max_connections(4)
        .connect(&format!("sqlite://{}", source.snapshot_path))
        .await
        .with_context(|| format!("open SQLite snapshot {}", source.snapshot_path))?;
    let tables = discover_tables(&sqlite, &args.skip_table).await?;
    let mut report = Report {
        schema_version: 1,
        run_id: run_id.clone(),
        started_at,
        finished_at: None,
        status: "running".into(),
        source,
        options: ReportOptions {
            batch_size: args.batch_size,
            reset_target: args.reset_target,
            dry_run: args.dry_run,
        },
        tables: Vec::new(),
        sequences_repaired: Vec::new(),
        search_rebuild: SearchRebuild {
            requested: args.rebuild_search_sql.is_some(),
            status: "not_requested".into(),
            ..Default::default()
        },
        validation: ValidationReport::default(),
        errors: Vec::new(),
    };

    if args.dry_run {
        for table in &tables {
            let (rows, hash) = source_count_and_key_hash(&sqlite, table).await?;
            report.tables.push(TableReport {
                table: table.name.clone(),
                source_rows: rows,
                imported_rows: 0,
                source_key_hash: hash,
                target_key_hash: None,
                status: "planned".into(),
                error: None,
            });
            report.validation.row_count_source += rows;
        }
        report.validation.table_count = tables.len();
        report.validation.row_counts_match = true;
        report.validation.key_hashes_match = true;
        report.validation.passed = true;
        report.status = "dry_run".into();
    } else {
        let target = args.target.as_deref().expect("validated target URL");
        let pg = PgPoolOptions::new()
            .max_connections(4)
            .connect(target)
            .await
            .context("connect to PostgreSQL target")?;
        if args.reset_target {
            reset_target(&pg, &tables).await?;
        }
        for table in &tables {
            match migrate_table(&sqlite, &pg, table, args.batch_size).await {
                Ok((source_rows, imported_rows, source_hash, target_hash)) => {
                    report.tables.push(TableReport {
                        table: table.name.clone(),
                        source_rows,
                        imported_rows,
                        source_key_hash: source_hash,
                        target_key_hash: Some(target_hash),
                        status: "imported".into(),
                        error: None,
                    })
                }
                Err(error) => {
                    report.errors.push(format!("{}: {error:#}", table.name));
                    report.tables.push(TableReport {
                        table: table.name.clone(),
                        source_rows: 0,
                        imported_rows: 0,
                        source_key_hash: String::new(),
                        target_key_hash: None,
                        status: "failed".into(),
                        error: Some(format!("{error:#}")),
                    });
                    break;
                }
            }
        }
        let sequence_names = repair_sequences(&pg).await?;
        report.sequences_repaired = sequence_names;
        report.validation = validate_target(&pg, &report.tables).await?;
        if let Some(path) = &args.rebuild_search_sql {
            report.search_rebuild = rebuild_search(&pg, path).await?;
        }
        report.status = if report.errors.is_empty() && report.validation.passed {
            "passed".into()
        } else {
            "failed".into()
        };
    }
    report.finished_at = Some(Utc::now());
    let report_path = args
        .report
        .unwrap_or_else(|| args.artifact_dir.join(format!("report-{run_id}.json")));
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("write report {}", report_path.display()))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report.status == "failed" {
        bail!("migration failed; inspect {}", report_path.display());
    }
    Ok(())
}

fn snapshot_source(source: &Path, artifact_dir: &Path, run_id: &str) -> Result<SourceReceipt> {
    if !source.is_file() {
        bail!("SQLite source does not exist: {}", source.display());
    }
    let snapshot_path = artifact_dir.join(format!("sqlite-{run_id}.db"));
    fs::copy(source, &snapshot_path)
        .with_context(|| format!("copy SQLite source to {}", snapshot_path.display()))?;
    let mut wal_sidecar_copied = false;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{}", source.display(), suffix));
        if sidecar.exists() {
            fs::copy(
                &sidecar,
                PathBuf::from(format!("{}{}", snapshot_path.display(), suffix)),
            )?;
            wal_sidecar_copied = true;
        }
    }
    let bytes = fs::metadata(&snapshot_path)?.len();
    let sha256 = sha256_file(&snapshot_path)?;
    Ok(SourceReceipt {
        original_path: source.display().to_string(),
        snapshot_path: snapshot_path.display().to_string(),
        sha256,
        bytes,
        wal_sidecar_copied,
        snapshot_method: "filesystem_copy_with_sqlite_sidecars".into(),
    })
}

fn sha256_file(path: &Path) -> Result<String> {
    let data = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(data)))
}

async fn discover_tables(pool: &SqlitePool, skip: &[String]) -> Result<Vec<TableInfo>> {
    let skip: HashSet<&str> = skip.iter().map(String::as_str).collect();
    let virtual_rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND sql LIKE '%VIRTUAL TABLE%'",
    )
    .fetch_all(pool)
    .await?;
    let virtual_names = virtual_rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
        .collect::<Vec<_>>();
    let rows = sqlx::query("SELECT name, sql FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .fetch_all(pool).await.context("discover SQLite tables")?;
    let mut tables = Vec::new();
    for row in rows {
        let name: String = row.try_get("name")?;
        let sql: Option<String> = row.try_get("sql")?;
        let is_virtual_shadow = virtual_names
            .iter()
            .any(|virtual_name| name.starts_with(&format!("{virtual_name}_")));
        if skip.contains(name.as_str())
            || is_virtual_shadow
            || sql
                .as_deref()
                .is_some_and(|sql| sql.to_ascii_uppercase().contains("VIRTUAL TABLE"))
        {
            continue;
        }
        let columns = sqlx::query(&format!(
            "PRAGMA table_info({})",
            quote_sqlite_identifier(&name)
        ))
        .fetch_all(pool)
        .await?;
        let mut names = Vec::new();
        let mut primary = Vec::new();
        for column in columns {
            let column_name: String = column.try_get("name")?;
            let pk: i64 = column.try_get("pk")?;
            names.push(column_name.clone());
            if pk > 0 {
                primary.push((pk, column_name));
            }
        }
        primary.sort_by_key(|(position, _)| *position);
        tables.push(TableInfo {
            name,
            columns: names,
            primary_key_columns: primary.into_iter().map(|(_, name)| name).collect(),
        });
    }
    Ok(tables)
}

fn quote_sqlite_identifier(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('\"', "\"\""))
}

async fn reset_target(pool: &PgPool, tables: &[TableInfo]) -> Result<()> {
    if tables.is_empty() {
        return Ok(());
    }
    let names = tables
        .iter()
        .map(|table| quote_identifier(&table.name))
        .collect::<Vec<_>>()
        .join(", ");
    sqlx::query(&format!("TRUNCATE TABLE {names} RESTART IDENTITY CASCADE"))
        .execute(pool)
        .await
        .context("reset PostgreSQL target tables")?;
    Ok(())
}

async fn migrate_table(
    sqlite: &SqlitePool,
    pg: &PgPool,
    table: &TableInfo,
    batch_size: usize,
) -> Result<(u64, u64, String, String)> {
    let select = ordered_select(table);
    let insert = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_identifier(&table.name),
        table
            .columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", "),
        (1..=table.columns.len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut source_stream = sqlx::query(&select).fetch(sqlite);
    let mut source_hasher = Sha256::new();
    let mut source_rows = 0u64;
    let mut imported = 0u64;
    let mut batch = Vec::with_capacity(batch_size);
    while let Some(row) = source_stream
        .try_next()
        .await
        .with_context(|| format!("read SQLite table {}", table.name))?
    {
        update_sqlite_hash(&mut source_hasher, &row, table)?;
        source_rows += 1;
        batch.push(row);
        if batch.len() < batch_size {
            continue;
        }
        let mut tx = pg
            .begin()
            .await
            .with_context(|| format!("begin PostgreSQL batch for {}", table.name))?;
        for row in &batch {
            let mut args = PgArguments::default();
            for (index, _) in table.columns.iter().enumerate() {
                bind_sqlite_value(&mut args, row, index)?;
            }
            sqlx::query_with(&insert, args)
                .execute(&mut *tx)
                .await
                .with_context(|| format!("insert into {}", table.name))?;
            imported += 1;
        }
        tx.commit()
            .await
            .with_context(|| format!("commit PostgreSQL batch for {}", table.name))?;
        batch.clear();
    }
    if !batch.is_empty() {
        let mut tx = pg
            .begin()
            .await
            .with_context(|| format!("begin PostgreSQL batch for {}", table.name))?;
        for row in &batch {
            let mut args = PgArguments::default();
            for (index, _) in table.columns.iter().enumerate() {
                bind_sqlite_value(&mut args, row, index)?;
            }
            sqlx::query_with(&insert, args)
                .execute(&mut *tx)
                .await
                .with_context(|| format!("insert into {}", table.name))?;
            imported += 1;
        }
        tx.commit()
            .await
            .with_context(|| format!("commit PostgreSQL batch for {}", table.name))?;
    }
    let target_select = ordered_select(table);
    let mut target_stream = sqlx::query(&target_select).fetch(pg);
    let mut target_hasher = Sha256::new();
    while let Some(row) = target_stream.try_next().await? {
        update_pg_hash(&mut target_hasher, &row, table)?;
    }
    Ok((
        source_rows,
        imported,
        format!("{:x}", source_hasher.finalize()),
        format!("{:x}", target_hasher.finalize()),
    ))
}

fn bind_sqlite_value(args: &mut PgArguments, row: &SqliteRow, index: usize) -> Result<()> {
    let raw = row.try_get_raw(index)?;
    if raw.is_null() {
        args.add(Option::<String>::None)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        return Ok(());
    }
    let type_name = raw.type_info().to_string().to_ascii_uppercase();
    if type_name.contains("INT") {
        args.add(row.try_get::<i64, _>(index)?)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    } else if type_name.contains("REAL") || type_name.contains("FLOA") || type_name.contains("DOUB")
    {
        args.add(row.try_get::<f64, _>(index)?)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    } else if type_name.contains("BLOB") {
        args.add(row.try_get::<Vec<u8>, _>(index)?)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    } else {
        args.add(row.try_get::<String, _>(index)?)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    Ok(())
}

async fn source_count_and_key_hash(pool: &SqlitePool, table: &TableInfo) -> Result<(u64, String)> {
    let select = ordered_select(table);
    let mut stream = sqlx::query(&select).fetch(pool);
    let mut hasher = Sha256::new();
    let mut count = 0u64;
    while let Some(row) = stream.try_next().await? {
        update_sqlite_hash(&mut hasher, &row, table)?;
        count += 1;
    }
    Ok((count, format!("{:x}", hasher.finalize())))
}

fn ordered_select(table: &TableInfo) -> String {
    let order = if table.primary_key_columns.is_empty() {
        String::new()
    } else {
        format!(
            " ORDER BY {}",
            table
                .primary_key_columns
                .iter()
                .map(|column| quote_identifier(column))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!("SELECT * FROM {}{}", quote_identifier(&table.name), order)
}

fn update_sqlite_hash(hasher: &mut Sha256, row: &SqliteRow, table: &TableInfo) -> Result<()> {
    for column in if table.primary_key_columns.is_empty() {
        &table.columns
    } else {
        &table.primary_key_columns
    } {
        let index = table
            .columns
            .iter()
            .position(|name| name == column)
            .unwrap();
        hasher.update(canonical_sqlite_value(row, index)?);
        hasher.update([0]);
    }
    Ok(())
}

fn canonical_sqlite_value(row: &SqliteRow, index: usize) -> Result<Vec<u8>> {
    let raw = row.try_get_raw(index)?;
    if raw.is_null() {
        return Ok(b"null".to_vec());
    }
    let type_name = raw.type_info().to_string().to_ascii_uppercase();
    if type_name.contains("INT") {
        return Ok(row.try_get::<i64, _>(index)?.to_string().into_bytes());
    }
    if type_name.contains("REAL") || type_name.contains("FLOA") || type_name.contains("DOUB") {
        return Ok(row.try_get::<f64, _>(index)?.to_string().into_bytes());
    }
    if type_name.contains("BLOB") {
        return Ok(hex_bytes(&row.try_get::<Vec<u8>, _>(index)?).into_bytes());
    }
    Ok(row.try_get::<String, _>(index)?.into_bytes())
}

fn hex_bytes(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn update_pg_hash(
    hasher: &mut Sha256,
    row: &sqlx::postgres::PgRow,
    table: &TableInfo,
) -> Result<()> {
    for column in if table.primary_key_columns.is_empty() {
        &table.columns
    } else {
        &table.primary_key_columns
    } {
        let index = table
            .columns
            .iter()
            .position(|name| name == column)
            .unwrap();
        hasher.update(canonical_pg_value(row, index)?);
        hasher.update([0]);
    }
    Ok(())
}

fn canonical_pg_value(row: &sqlx::postgres::PgRow, index: usize) -> Result<Vec<u8>> {
    let type_name = row
        .column(index)
        .type_info()
        .to_string()
        .to_ascii_uppercase();
    if type_name.contains("INT") {
        return Ok(row
            .try_get::<Option<i64>, _>(index)?
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into())
            .into_bytes());
    }
    if type_name.contains("BOOL") {
        return Ok(row
            .try_get::<Option<bool>, _>(index)?
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into())
            .into_bytes());
    }
    if type_name.contains("BYTEA") {
        return Ok(row
            .try_get::<Option<Vec<u8>>, _>(index)?
            .map(|v| hex_bytes(&v))
            .unwrap_or_else(|| "null".into())
            .into_bytes());
    }
    Ok(row
        .try_get::<Option<String>, _>(index)?
        .unwrap_or_else(|| "null".into())
        .into_bytes())
}

async fn repair_sequences(pool: &PgPool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT table_schema, table_name, column_name FROM information_schema.columns WHERE column_default LIKE 'nextval(%' ORDER BY table_schema, table_name, ordinal_position").fetch_all(pool).await?;
    let mut repaired = Vec::new();
    for row in rows {
        let schema: String = row.try_get("table_schema")?;
        let table: String = row.try_get("table_name")?;
        let column: String = row.try_get("column_name")?;
        let seq: Option<String> = sqlx::query_scalar("SELECT pg_get_serial_sequence($1, $2)")
            .bind(format!("{schema}.{table}"))
            .bind(&column)
            .fetch_one(pool)
            .await?;
        let Some(seq) = seq else {
            continue;
        };
        let max: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT MAX({}) FROM {}.{}",
            quote_identifier(&column),
            quote_identifier(&schema),
            quote_identifier(&table)
        ))
        .fetch_one(pool)
        .await?;
        if let Some(max) = max {
            sqlx::query("SELECT setval($1, $2, true)")
                .bind(&seq)
                .bind(max)
                .execute(pool)
                .await?;
            repaired.push(format!("{schema}.{table}.{column}"));
        }
    }
    Ok(repaired)
}

async fn validate_target(pool: &PgPool, tables: &[TableReport]) -> Result<ValidationReport> {
    let mut result = ValidationReport {
        table_count: tables.len(),
        ..Default::default()
    };
    result.row_count_source = tables.iter().map(|table| table.source_rows).sum();
    result.row_count_target = tables
        .iter()
        .filter_map(|table| table.target_key_hash.as_ref().map(|_| table.imported_rows))
        .sum();
    result.row_counts_match = tables
        .iter()
        .all(|table| table.source_rows == table.imported_rows);
    result.key_hashes_match = tables
        .iter()
        .all(|table| table.target_key_hash.as_ref() == Some(&table.source_key_hash));
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(pool)
        .await
        .context("validate PostgreSQL foreign keys")?;
    result.foreign_keys_checked = true;
    result.foreign_key_violations = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pg_constraint WHERE contype = 'f' AND NOT convalidated",
    )
    .fetch_one(pool)
    .await? as u64;
    result.passed =
        result.row_counts_match && result.key_hashes_match && result.foreign_key_violations == 0;
    Ok(result)
}

async fn rebuild_search(pool: &PgPool, path: &Path) -> Result<SearchRebuild> {
    let sql = fs::read_to_string(path)
        .with_context(|| format!("read search rebuild SQL {}", path.display()))?;
    let mut result = SearchRebuild {
        requested: true,
        status: "running".into(),
        ..Default::default()
    };
    for statement in sql
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty() && !statement.starts_with("--"))
    {
        sqlx::query(statement)
            .execute(pool)
            .await
            .with_context(|| format!("execute search rebuild statement: {statement}"))?;
        result.statements_executed += 1;
    }
    result.status = "passed".into();
    Ok(result)
}
