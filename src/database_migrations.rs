use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha384};
use sqlx::{SqlitePool, migrate::Migrator};

const LEGACY_SEARCH_MIGRATION_VERSION: i64 = 81;
const LEGACY_SEARCH_MIGRATION_SQL: &[u8] =
    include_bytes!("../migrations/legacy/0081_command_palette_search.sql");
const SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION: i64 = 98;
const LEGACY_SEARCH_SCHEDULER_INDEX_MIGRATION_CHECKSUM: &[u8; 48] = &[
    0x9b, 0xa9, 0xb7, 0x22, 0xed, 0xc0, 0x32, 0x72, 0xb5, 0x43, 0x55, 0xb0, 0xf3, 0x18, 0x1d, 0x1e,
    0xab, 0x77, 0x42, 0xa2, 0xc1, 0xf5, 0x81, 0xf2, 0x6e, 0xcb, 0xcb, 0xfa, 0xc3, 0x0f, 0xbe, 0xf8,
    0x6b, 0x7b, 0x03, 0x24, 0x33, 0x49, 0x01, 0xc6, 0x32, 0xaf, 0x04, 0x4e, 0x2a, 0x10, 0x3c, 0x07,
];

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone)]
struct AppliedMigration {
    version: i64,
    checksum: Vec<u8>,
    success: i64,
}

pub async fn run(pool: &SqlitePool) -> Result<()> {
    let Some(applied) = load_applied_migrations(pool).await? else {
        return apply_migrations(pool).await;
    };
    if applied.is_empty() {
        return apply_migrations(pool).await;
    }
    validate_history(Some(&applied), &MIGRATOR)?;
    let pending = pending_migration_versions(&applied, &MIGRATOR);
    if pending.is_empty() {
        return Ok(());
    }
    apply_migrations(pool)
        .await
        .with_context(|| format!("failed to apply pending SQLx migrations: {pending:?}"))
}

async fn apply_migrations(pool: &SqlitePool) -> Result<()> {
    normalize_legacy_migration_checksums(pool).await?;
    let mut migrator = sqlx::migrate!("./migrations");
    // Version 81 is intentionally kept as historical evidence outside the
    // discovered source set. validate_history narrows this exception to that
    // one known migration before SQLx ignores missing applied versions.
    migrator.set_ignore_missing(true);
    migrator
        .run(pool)
        .await
        .context("failed to apply database migrations")?;
    Ok(())
}

async fn normalize_legacy_migration_checksums(pool: &SqlitePool) -> Result<()> {
    let table_exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await
    .context("check SQLx migration history before checksum repair")?
        != 0;
    if !table_exists {
        return Ok(());
    }

    let current_checksum = MIGRATOR
        .iter()
        .find(|migration| migration.version == SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION)
        .map(|migration| migration.checksum.to_vec())
        .context("load current scheduler index migration checksum")?;
    let updated =
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ? AND checksum = ?")
            .bind(current_checksum)
            .bind(SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION)
            .bind(LEGACY_SEARCH_SCHEDULER_INDEX_MIGRATION_CHECKSUM.as_slice())
            .execute(pool)
            .await
            .context("forward-repair legacy scheduler index migration checksum")?;
    if updated.rows_affected() != 0 {
        tracing::warn!(
            migration_version = SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION,
            "normalized a known legacy scheduler index migration checksum before forward repair"
        );
    }
    Ok(())
}

fn pending_migration_versions(applied: &[AppliedMigration], migrator: &Migrator) -> Vec<i64> {
    let applied_versions = applied
        .iter()
        .map(|migration| migration.version)
        .collect::<HashSet<_>>();
    migrator
        .iter()
        .filter(|migration| !applied_versions.contains(&migration.version))
        .map(|migration| migration.version)
        .collect()
}

async fn load_applied_migrations(pool: &SqlitePool) -> Result<Option<Vec<AppliedMigration>>> {
    let table_exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await
    .context("check SQLx migration history")?
        != 0;
    if !table_exists {
        return Ok(None);
    }

    let rows =
        sqlx::query("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version ASC")
            .fetch_all(pool)
            .await
            .context("load SQLx migration history")?;
    Ok(Some(
        rows.into_iter()
            .map(|row| AppliedMigration {
                version: sqlx::Row::get(&row, "version"),
                checksum: sqlx::Row::get(&row, "checksum"),
                success: sqlx::Row::get(&row, "success"),
            })
            .collect(),
    ))
}

fn validate_history(applied: Option<&[AppliedMigration]>, migrator: &Migrator) -> Result<()> {
    let Some(applied) = applied else {
        return Ok(());
    };

    let expected = migrator
        .iter()
        .map(|migration| (migration.version, migration.checksum.as_ref()))
        .collect::<HashMap<_, _>>();
    let legacy_checksum = Sha384::digest(LEGACY_SEARCH_MIGRATION_SQL).to_vec();
    let applied_versions = applied
        .iter()
        .map(|migration| migration.version)
        .collect::<HashSet<_>>();
    let highest_applied = applied.iter().map(|migration| migration.version).max();

    for migration in applied {
        if migration.success != 1 {
            bail!(
                "database migration {} has invalid success flag {}",
                migration.version,
                migration.success
            );
        }
        if migration.version == LEGACY_SEARCH_MIGRATION_VERSION {
            if migration.checksum != legacy_checksum {
                bail!("database migration 81 checksum mismatch");
            }
            continue;
        }
        let Some(checksum) = expected.get(&migration.version) else {
            bail!(
                "database migration {} is missing from the application",
                migration.version
            );
        };
        let known_legacy_scheduler_index_checksum = migration.version
            == SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION
            && migration.checksum.as_slice()
                == LEGACY_SEARCH_SCHEDULER_INDEX_MIGRATION_CHECKSUM.as_slice();
        if migration.checksum.as_slice() != *checksum && !known_legacy_scheduler_index_checksum {
            bail!("database migration {} checksum mismatch", migration.version);
        }
    }

    if let Some(highest_applied) = highest_applied {
        for migration in migrator.iter() {
            if migration.version > highest_applied
                || migration.version == LEGACY_SEARCH_MIGRATION_VERSION
            {
                continue;
            }
            if !applied_versions.contains(&migration.version) {
                bail!(
                    "database migration {} is missing from history",
                    migration.version
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use sqlx::{Row, sqlite::SqlitePoolOptions};

    fn current_checksum(version: i64) -> Vec<u8> {
        MIGRATOR
            .iter()
            .find(|migration| migration.version == version)
            .map(|migration| migration.checksum.to_vec())
            .expect("migration exists")
    }

    #[test]
    fn legacy_0081_compatibility() {
        let mut history = MIGRATOR
            .iter()
            .filter(|migration| migration.version < LEGACY_SEARCH_MIGRATION_VERSION)
            .map(|migration| AppliedMigration {
                version: migration.version,
                checksum: migration.checksum.to_vec(),
                success: 1,
            })
            .collect::<Vec<_>>();
        history.push(AppliedMigration {
            version: LEGACY_SEARCH_MIGRATION_VERSION,
            checksum: Sha384::digest(LEGACY_SEARCH_MIGRATION_SQL).to_vec(),
            success: 1,
        });
        validate_history(Some(&history), &MIGRATOR).expect("legacy 0081 is accepted");

        let mut dirty_legacy = history.clone();
        dirty_legacy
            .iter_mut()
            .find(|migration| migration.version == LEGACY_SEARCH_MIGRATION_VERSION)
            .expect("legacy migration history row")
            .success = 0;
        assert!(validate_history(Some(&dirty_legacy), &MIGRATOR).is_err());

        let mut mismatched_legacy = history.clone();
        mismatched_legacy
            .iter_mut()
            .find(|migration| migration.version == LEGACY_SEARCH_MIGRATION_VERSION)
            .expect("legacy migration history row")
            .checksum[0] ^= 1;
        assert!(validate_history(Some(&mismatched_legacy), &MIGRATOR).is_err());

        let mut mismatched = history;
        mismatched[0].checksum[0] ^= 1;
        assert!(validate_history(Some(&mismatched), &MIGRATOR).is_err());
    }

    #[test]
    fn legacy_0098_checksum_is_accepted_for_forward_repair() {
        let mut history = MIGRATOR
            .iter()
            .filter(|migration| migration.version <= SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION)
            .map(|migration| AppliedMigration {
                version: migration.version,
                checksum: migration.checksum.to_vec(),
                success: 1,
            })
            .collect::<Vec<_>>();
        history
            .iter_mut()
            .find(|migration| migration.version == SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION)
            .expect("scheduler index migration history")
            .checksum = LEGACY_SEARCH_SCHEDULER_INDEX_MIGRATION_CHECKSUM.to_vec();
        validate_history(Some(&history), &MIGRATOR)
            .expect("known legacy scheduler index checksum is accepted");

        let mut mismatched = history;
        mismatched
            .iter_mut()
            .find(|migration| migration.version == SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION)
            .expect("scheduler index migration history")
            .checksum[0] ^= 1;
        assert!(validate_history(Some(&mismatched), &MIGRATOR).is_err());
    }

    #[test]
    fn unknown_or_dirty_history_is_rejected() {
        let unknown = vec![AppliedMigration {
            version: 999,
            checksum: vec![0; 48],
            success: 1,
        }];
        assert!(validate_history(Some(&unknown), &MIGRATOR).is_err());

        let dirty = vec![AppliedMigration {
            version: 80,
            checksum: current_checksum(80),
            success: 0,
        }];
        assert!(validate_history(Some(&dirty), &MIGRATOR).is_err());

        let invalid_success = vec![AppliedMigration {
            version: 80,
            checksum: current_checksum(80),
            success: 2,
        }];
        assert!(validate_history(Some(&invalid_success), &MIGRATOR).is_err());
    }

    #[test]
    fn internal_history_gap_is_rejected() {
        let missing_version = 84;
        let mut history = MIGRATOR
            .iter()
            .map(|migration| AppliedMigration {
                version: migration.version,
                checksum: migration.checksum.to_vec(),
                success: 1,
            })
            .collect::<Vec<_>>();
        history.retain(|migration| migration.version != missing_version);

        let error = validate_history(Some(&history), &MIGRATOR)
            .expect_err("an internal migration history gap must be rejected");
        assert!(
            error
                .to_string()
                .contains("database migration 84 is missing from history")
        );
    }

    #[tokio::test]
    async fn fresh_database_skips_legacy_sql() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        run(&pool).await.expect("run recovery migrations");
        let applied = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 81",
        )
        .fetch_one(&pool)
        .await
        .expect("read migration history");
        assert_eq!(applied, 0);
        let state = sqlx::query_scalar::<_, String>(
            "SELECT status FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read search state");
        assert_eq!(state, "pending");

        let demand_table = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'webhook_push_reconcile_demands'",
        )
        .fetch_one(&pool)
        .await
        .expect("read webhook reconcile demand table");
        assert_eq!(demand_table, 1);
        let scope_column = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pragma_table_info('reaction_pat_tokens') WHERE name = 'webhook_push_allows_private_repos'",
        )
        .fetch_one(&pool)
        .await
        .expect("read webhook PAT scope column");
        assert_eq!(scope_column, 1);
    }

    #[tokio::test]
    async fn existing_complete_history_validates_without_mutation() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        run(&pool).await.expect("apply complete migration history");
        let before = sqlx::query_as::<_, (i64, Vec<u8>, i64)>(
            "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&pool)
        .await
        .expect("read complete migration history");

        run(&pool)
            .await
            .expect("validate complete existing migration history");
        let after = sqlx::query_as::<_, (i64, Vec<u8>, i64)>(
            "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&pool)
        .await
        .expect("read validated migration history");
        assert_eq!(after, before);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'search_projection_backfill_state'",
            )
            .fetch_one(&pool)
            .await
            .expect("read validated schema"),
            1
        );
    }

    #[tokio::test]
    async fn empty_migration_history_restarts_initialization() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::query(
            "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("create empty SQLx history");

        run(&pool)
            .await
            .expect("restart incomplete database initialization");
        let applied =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&pool)
                .await
                .expect("read recovered migration history");
        assert_eq!(applied, MIGRATOR.iter().count() as i64);
        let final_schema = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'search_projection_backfill_state'",
        )
        .fetch_one(&pool)
        .await
        .expect("read recovered final schema");
        assert_eq!(final_schema, 1);
    }

    #[tokio::test]
    async fn existing_v90_database_applies_pending_search_migration() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");

        let pre_0091 = Migrator {
            migrations: Cow::Owned(
                MIGRATOR
                    .iter()
                    .filter(|migration| migration.version < 91)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        pre_0091
            .run(&pool)
            .await
            .expect("apply schema before projection index migration");

        sqlx::query(
            "INSERT INTO job_tasks (id, task_type, status, source, payload_json, created_at, updated_at) VALUES ('legacy-malformed-payload', 'legacy.task', 'succeeded', 'legacy', 'not-json', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed legacy malformed task payload");

        for index in 0..3 {
            let document_id = format!("release:upgrade-{index}");
            sqlx::query(
                "INSERT INTO search_documents (id,resource_type,resource_id,title,body,created_at,updated_at) VALUES (?,'release',?,'upgrade title','upgrade body',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",
            )
            .bind(&document_id)
            .bind(format!("upgrade-{index}"))
            .execute(&pool)
            .await
            .expect("seed existing search projection");
            sqlx::query(
                "INSERT INTO search_documents_fts (doc_id,title,body,repo_full_name,translated_text,smart_text) VALUES (?,'upgrade title','upgrade body','','','')",
            )
            .bind(document_id)
            .execute(&pool)
            .await
            .expect("seed existing FTS corpus");
        }
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'translations', cursor = 0, status = 'ready' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("mark existing search corpus ready");
        let old_fts_rows =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents_fts")
                .fetch_one(&pool)
                .await
                .expect("count old FTS corpus");
        assert!(old_fts_rows > 0);

        run(&pool)
            .await
            .expect("apply pending migration to existing v90 database");

        for version in [93_i64, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104] {
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = ? AND success = 1",
                )
                .bind(version)
                .fetch_one(&pool)
                .await
                .expect("read applied search migration"),
                1,
                "migration {version} should be applied",
            );
            assert_eq!(
                sqlx::query_scalar::<_, Vec<u8>>(
                    "SELECT checksum FROM _sqlx_migrations WHERE version = ?",
                )
                .bind(version)
                .fetch_one(&pool)
                .await
                .expect("read applied search migration checksum"),
                current_checksum(version),
                "migration {version} checksum should match the checked-in SQL",
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'search_projection_outbox'",
            )
            .fetch_one(&pool)
            .await
            .expect("check search projection outbox table"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM pragma_table_info('search_projection_outbox') WHERE name = 'previous_resource_id'",
            )
            .fetch_one(&pool)
            .await
            .expect("check announcement key repair column"),
            1
        );
        let announcement_trigger_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'search_announcements_outbox_au'",
        )
        .fetch_one(&pool)
        .await
        .expect("read announcement projection trigger");
        assert!(announcement_trigger_sql.contains("previous_resource_id"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_search_documents_resource_key'",
            )
            .fetch_one(&pool)
            .await
            .expect("check resource-key projection index"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_search_projection_outbox_poll'",
            )
            .fetch_one(&pool)
            .await
            .expect("check outbox poll index"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'search_projection_outbox_failed'",
            )
            .fetch_one(&pool)
            .await
            .expect("check failed projection outbox table"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM pragma_table_info('search_projection_outbox') WHERE name = 'fanout_cursor'",
            )
            .fetch_one(&pool)
            .await
            .expect("check announcement fanout cursor column"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_job_tasks_star_connection_active'",
            )
            .fetch_one(&pool)
            .await
            .expect("check active star scheduler index"),
            1
        );
        let star_scheduler_index_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_job_tasks_star_connection_active'",
        )
        .fetch_one(&pool)
        .await
        .expect("read active star scheduler index");
        assert!(star_scheduler_index_sql.contains("json_valid(payload_json)"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_search_projection_outbox_failed_poll'",
            )
            .fetch_one(&pool)
            .await
            .expect("check terminal projection recovery poll index"),
            1
        );
        sqlx::query(
            "INSERT INTO job_tasks (id, task_type, status, source, payload_json, created_at, updated_at) VALUES ('valid-star-payload', 'sync.starred.delta', 'queued', 'scheduler', '{\"github_connection_id\":\"legacy-connection\"}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed valid task payload after guarded index");
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM job_tasks WHERE task_type = 'sync.starred.delta' AND status = 'queued' AND json_valid(payload_json) AND json_extract(payload_json, '$.github_connection_id') = 'legacy-connection'",
            )
            .fetch_one(&pool)
            .await
            .expect("query valid task payload through guarded index"),
            "valid-star-payload"
        );
        let scheduler_recovery_trigger_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'search_projection_outbox_reset_source_state'",
        )
        .fetch_one(&pool)
        .await
        .expect("read scheduler recovery trigger");
        assert!(scheduler_recovery_trigger_sql.contains("fanout_cursor"));
        let document_trigger_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'search_documents_ai'",
        )
        .fetch_one(&pool)
        .await
        .expect("read bounded release document trigger");
        assert!(document_trigger_sql.contains("NEW.resource_type = 'release'"));
        assert!(!document_trigger_sql.contains("NEW.resource_type = 'announcement'"));
        let content_transition_trigger_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'search_content_projections_outbox_au_old'",
        )
        .fetch_one(&pool)
        .await
        .expect("read content transition trigger");
        assert!(content_transition_trigger_sql.contains("OLD.target_lang"));

        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 91 AND success = 1",
            )
            .fetch_one(&pool)
            .await
            .expect("read applied migration 91"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, Vec<u8>>(
                "SELECT checksum FROM _sqlx_migrations WHERE version = 91",
            )
            .fetch_one(&pool)
            .await
            .expect("read migration 91 checksum"),
            current_checksum(91)
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_user_repo_associations_repo_id', 'idx_ai_translations_search_entity', 'idx_ai_translations_search_entity_lower')",
            )
            .fetch_one(&pool)
            .await
            .expect("read migration 91 indexes"),
            3
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_content_work_items_resource_scope_status', 'idx_content_work_items_supersession_cursor', 'idx_content_work_items_supersession_history')",
            )
            .fetch_one(&pool)
            .await
            .expect("read content work supersession index"),
            3
        );
        let document_trigger_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'search_documents_ai'",
        )
        .fetch_one(&pool)
        .await
        .expect("read bounded release document trigger");
        assert!(document_trigger_sql.contains("NEW.resource_type = 'release'"));

        let state = sqlx::query_as::<_, (String, i64, String)>(
            "SELECT phase, cursor, status FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read resumable recovery state");
        assert_eq!(state, ("translations".to_owned(), 0, "ready".to_owned()));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents")
                .fetch_one(&pool)
                .await
                .expect("count retained search projections"),
            3
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'search_documents_fts_v2'",
            )
                .fetch_one(&pool)
                .await
                .expect("check existing FTS migration"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents_fts")
                .fetch_one(&pool)
                .await
                .expect("count retained FTS corpus"),
            old_fts_rows
        );
    }

    #[tokio::test]
    async fn existing_v98_history_applies_forward_scheduler_index_repair() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        let pre_0099 = Migrator {
            migrations: Cow::Owned(
                MIGRATOR
                    .iter()
                    .filter(|migration| {
                        migration.version <= SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION
                    })
                    .cloned()
                    .collect(),
            ),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        pre_0099
            .run(&pool)
            .await
            .expect("apply schema through migration 98");
        sqlx::query("DROP INDEX idx_job_tasks_star_connection_active")
            .execute(&pool)
            .await
            .expect("remove current scheduler index before legacy fixture");
        sqlx::query(
            "CREATE INDEX idx_job_tasks_star_connection_active ON job_tasks(task_type, status, json_extract(payload_json, '$.github_connection_id'))",
        )
        .execute(&pool)
        .await
        .expect("create the shipped unguarded scheduler index");
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(LEGACY_SEARCH_SCHEDULER_INDEX_MIGRATION_CHECKSUM.as_slice())
            .bind(SEARCH_SCHEDULER_INDEX_MIGRATION_VERSION)
            .execute(&pool)
            .await
            .expect("seed legacy migration checksum");

        run(&pool)
            .await
            .expect("apply forward scheduler index repair");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 99 AND success = 1",
            )
            .fetch_one(&pool)
            .await
            .expect("read forward repair migration history"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, Vec<u8>>(
                "SELECT checksum FROM _sqlx_migrations WHERE version = 98",
            )
            .fetch_one(&pool)
            .await
            .expect("read normalized migration checksum"),
            current_checksum(98)
        );
        let index_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_job_tasks_star_connection_active'",
        )
        .fetch_one(&pool)
        .await
        .expect("read repaired scheduler index");
        assert!(index_sql.contains("json_valid(payload_json)"));
    }

    #[tokio::test]
    async fn existing_v100_history_applies_and_reenters_recent_sync_task_index_migration() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        let pre_0101 = Migrator {
            migrations: Cow::Owned(
                MIGRATOR
                    .iter()
                    .filter(|migration| migration.version <= 100)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        pre_0101
            .run(&pool)
            .await
            .expect("apply schema through migration 100");
        sqlx::query(
            "INSERT INTO job_tasks (id, task_type, status, source, payload_json, result_json, created_at, updated_at) VALUES ('legacy-malformed-sync-result', 'sync.subscriptions', 'failed', 'scheduler', '{}', 'not-json', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed malformed legacy subscription result");

        run(&pool)
            .await
            .expect("apply pending recent sync task index migration");
        run(&pool)
            .await
            .expect("re-enter startup migration validation after 0101");

        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 101 AND success = 1",
            )
            .fetch_one(&pool)
            .await
            .expect("read applied migration 101"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, Vec<u8>>(
                "SELECT checksum FROM _sqlx_migrations WHERE version = 101",
            )
            .fetch_one(&pool)
            .await
            .expect("read migration 101 checksum"),
            current_checksum(101)
        );
        let index_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_job_tasks_sync_recent_roots'",
        )
        .fetch_one(&pool)
        .await
        .expect("read recent sync roots index");
        assert!(index_sql.contains("task_type = 'sync.subscriptions'"));
        assert!(index_sql.contains("json_valid(result_json)"));
        assert!(index_sql.contains("COALESCE(json_extract(result_json, '$.skipped'), 0) != 1"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_job_tasks_subscription_chain'",
            )
            .fetch_one(&pool)
            .await
            .expect("check subscription child linkage index"),
            1
        );
        sqlx::query(
            "INSERT INTO job_tasks (id, task_type, status, source, payload_json, result_json, created_at, updated_at) VALUES ('new-malformed-sync-result', 'sync.subscriptions', 'failed', 'scheduler', '{}', 'still-not-json', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("malformed result remains compatible with partial index maintenance");
    }

    #[tokio::test]
    async fn release_reading_backfill_migration_creates_tables_indexes_and_foreign_keys() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        let pre_0102 = Migrator {
            migrations: Cow::Owned(
                MIGRATOR
                    .iter()
                    .filter(|migration| migration.version <= 101)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        pre_0102
            .run(&pool)
            .await
            .expect("apply schema through migration 101");
        run(&pool)
            .await
            .expect("apply release reading backfill migration");

        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 102 AND success = 1",
            )
            .fetch_one(&pool)
            .await
            .expect("read migration 102"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 103 AND success = 1",
            )
            .fetch_one(&pool)
            .await
            .expect("read migration 103"),
            1
        );
        for table in [
            "release_reading_backfill_runs",
            "release_reading_backfill_items",
        ] {
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
                )
                .bind(table)
                .fetch_one(&pool)
                .await
                .expect("check backfill table"),
                1,
                "migration 102 must create {table}"
            );
        }
        for index in [
            "idx_release_reading_backfill_runs_status",
            "idx_release_reading_backfill_items_cursor",
            "idx_release_reading_backfill_items_release",
        ] {
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?",
                )
                .bind(index)
                .fetch_one(&pool)
                .await
                .expect("check backfill index"),
                1,
                "migration 102 must create {index}"
            );
        }
        let foreign_keys = sqlx::query("PRAGMA foreign_key_list('release_reading_backfill_items')")
            .fetch_all(&pool)
            .await
            .expect("read backfill foreign keys");
        assert_eq!(foreign_keys.len(), 2);
        let foreign_key_tables = foreign_keys
            .iter()
            .map(|row| row.get::<String, _>("table"))
            .collect::<std::collections::HashSet<_>>();
        assert!(foreign_key_tables.contains("release_reading_backfill_runs"));
        assert!(foreign_key_tables.contains("repo_releases"));

        let run_columns = sqlx::query("PRAGMA table_info('release_reading_backfill_runs')")
            .fetch_all(&pool)
            .await
            .expect("read backfill run columns");
        let run_column_names = run_columns
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<std::collections::HashSet<_>>();
        for required in [
            "id",
            "started_at",
            "window_start_utc",
            "window_end_utc",
            "status",
            "total",
            "succeeded",
            "failed",
            "waiting",
            "skipped",
            "lease_owner",
            "lease_task_id",
            "lease_expires_at",
            "error_text",
            "created_at",
            "updated_at",
        ] {
            assert!(
                run_column_names.contains(required),
                "missing run column {required}"
            );
        }
        let item_columns = sqlx::query("PRAGMA table_info('release_reading_backfill_items')")
            .fetch_all(&pool)
            .await
            .expect("read backfill item columns");
        let item_column_names = item_columns
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<std::collections::HashSet<_>>();
        for required in [
            "run_id",
            "release_id",
            "authorized_user_id",
            "published_at",
            "status",
            "attempt_count",
            "error_text",
            "updated_at",
        ] {
            assert!(
                item_column_names.contains(required),
                "missing item column {required}"
            );
        }
        let run_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'release_reading_backfill_runs'",
        )
        .fetch_one(&pool)
        .await
        .expect("read run table definition");
        assert!(run_sql.contains(
            "CHECK (status IN ('queued', 'running', 'waiting', 'paused', 'succeeded', 'failed'))"
        ));
        let item_sql = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'release_reading_backfill_items'",
        )
        .fetch_one(&pool)
        .await
        .expect("read item table definition");
        assert!(item_sql.contains("PRIMARY KEY (run_id, release_id)"));
        assert!(item_sql.contains("ON DELETE CASCADE"));
    }

    #[tokio::test]
    async fn failed_pending_migration_reports_version_and_rolls_back() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        let pre_0091 = Migrator {
            migrations: Cow::Owned(
                MIGRATOR
                    .iter()
                    .filter(|migration| migration.version < 91)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        pre_0091
            .run(&pool)
            .await
            .expect("apply schema before failed migration");
        sqlx::query("DROP TABLE ai_translations")
            .execute(&pool)
            .await
            .expect("create migration failure fixture");

        let error = run(&pool)
            .await
            .expect_err("invalid pending migration must fail startup");
        let rendered = format!("{error:#}");
        assert!(rendered.contains(
            "failed to apply pending SQLx migrations: [91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104]"
        ));
        assert!(rendered.contains("failed to apply database migrations"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 91",
            )
            .fetch_one(&pool)
            .await
            .expect("read failed migration history"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_user_repo_associations_repo_id', 'idx_ai_translations_search_entity', 'idx_ai_translations_search_entity_lower')",
            )
            .fetch_one(&pool)
            .await
            .expect("read rolled back migration indexes"),
            0
        );
    }

    #[tokio::test]
    async fn admin_collection_coverage_preserves_old_records_and_tracks_new_ones() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::raw_sql(
            r#"
            CREATE TABLE admin_collection_processing_coverage (
              record_kind TEXT NOT NULL,
              record_id TEXT NOT NULL,
              pipeline TEXT NOT NULL,
              status_origin TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (record_kind, record_id, pipeline)
            );
            CREATE TABLE repo_releases (release_id INTEGER);
            CREATE TABLE social_activity_events (
              kind TEXT,
              repo_full_name TEXT,
              discussion_number INTEGER
            );
            CREATE TABLE notifications (thread_id TEXT);
            CREATE TABLE briefs (id TEXT);
            INSERT INTO notifications (thread_id) VALUES ('old-thread');
            "#,
        )
        .execute(&pool)
        .await
        .expect("create coverage migration fixture");

        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 87)
            .expect("admin collection coverage migration");
        sqlx::raw_sql(&migration.sql)
            .execute(&pool)
            .await
            .expect("apply admin collection coverage migration");

        let old_origin = sqlx::query_scalar::<_, String>(
            "SELECT status_origin FROM admin_collection_processing_coverage
             WHERE record_kind = 'notification' AND record_id = 'old-thread' AND pipeline = 'polish'",
        )
        .fetch_one(&pool)
        .await
        .expect("read old notification coverage");
        assert_eq!(old_origin, "historical_unknown");

        sqlx::raw_sql(
            r#"
            INSERT INTO repo_releases (release_id) VALUES (101);
            INSERT INTO social_activity_events (kind, repo_full_name, discussion_number)
              VALUES ('announcement', 'Octo/Demo', 42);
            INSERT INTO notifications (thread_id) VALUES ('new-thread');
            INSERT INTO briefs (id) VALUES ('new-brief');
            "#,
        )
        .execute(&pool)
        .await
        .expect("insert new source records");

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT record_kind, record_id, pipeline, status_origin
             FROM admin_collection_processing_coverage
             WHERE status_origin = 'never_started'
             ORDER BY record_kind, record_id, pipeline",
        )
        .fetch_all(&pool)
        .await
        .expect("read new record coverage");
        assert_eq!(
            rows,
            vec![
                (
                    "announcement".to_owned(),
                    "octo/demo#42".to_owned(),
                    "polish".to_owned(),
                    "never_started".to_owned()
                ),
                (
                    "announcement".to_owned(),
                    "octo/demo#42".to_owned(),
                    "translation".to_owned(),
                    "never_started".to_owned()
                ),
                (
                    "brief".to_owned(),
                    "new-brief".to_owned(),
                    "polish".to_owned(),
                    "never_started".to_owned()
                ),
                (
                    "notification".to_owned(),
                    "new-thread".to_owned(),
                    "polish".to_owned(),
                    "never_started".to_owned()
                ),
                (
                    "notification".to_owned(),
                    "new-thread".to_owned(),
                    "translation".to_owned(),
                    "never_started".to_owned()
                ),
                (
                    "release".to_owned(),
                    "101".to_owned(),
                    "polish".to_owned(),
                    "never_started".to_owned()
                ),
                (
                    "release".to_owned(),
                    "101".to_owned(),
                    "translation".to_owned(),
                    "never_started".to_owned()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn existing_history_applies_pending_reconcile_migration_without_losing_data() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        let pre_0083 = Migrator {
            migrations: Cow::Owned(
                MIGRATOR
                    .iter()
                    .filter(|migration| migration.version < 83)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        pre_0083
            .run(&pool)
            .await
            .expect("apply schema through version 82");

        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            "INSERT INTO users (id, github_user_id, login, created_at, updated_at) VALUES ('user-1', 30215105, 'IvanLi-CN', ?, ?)",
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed migrated user");
        sqlx::query(
            r#"
            INSERT INTO reaction_pat_tokens (
              user_id, token_ciphertext, token_nonce, masked_token,
              last_check_state, last_check_message, last_checked_at, updated_at
            ) VALUES ('user-1', X'01', X'02', 'ghp_...1234', 'valid', 'token is valid', ?, ?)
            "#,
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed migrated PAT");
        sqlx::query(
            r#"
            INSERT INTO webhook_push_repos (
              user_id, repo_id, owner_login, repo_name, repo_full_name,
              hook_id, callback_url, status, updated_at
            ) VALUES ('user-1', 101, 'IvanLi-CN', 'octo-rill', 'IvanLi-CN/octo-rill',
                      9001, 'https://example.test/webhook', 'registered', ?)
            "#,
        )
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed migrated webhook observation");

        run(&pool)
            .await
            .expect("apply pending reconcile migrations");

        let demand_table = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'webhook_push_reconcile_demands'",
        )
        .fetch_one(&pool)
        .await
        .expect("read demand table");
        assert_eq!(demand_table, 1);
        let scope = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT COUNT(*) FROM pragma_table_info('reaction_pat_tokens') WHERE name = 'webhook_push_allows_private_repos'",
        )
        .fetch_one(&pool)
        .await
        .expect("check pending PAT scope migration");
        assert_eq!(scope, Some(1));
        let hook_id = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT hook_id FROM webhook_push_repos WHERE user_id = 'user-1' AND repo_id = 101",
        )
        .fetch_one(&pool)
        .await
        .expect("read retained webhook observation");
        assert_eq!(hook_id, Some(9001));
    }

    #[tokio::test]
    async fn existing_history_applies_pending_identity_compatibility_without_losing_data() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");

        let current_migrations = sqlx::migrate!("./migrations");
        let pre_identity_compatibility_migrator = Migrator {
            migrations: Cow::Owned(
                current_migrations
                    .iter()
                    .filter(|migration| migration.version < 84)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: true,
            locking: true,
            no_tx: false,
        };
        pre_identity_compatibility_migrator
            .run(&pool)
            .await
            .expect("apply schema through version 83");
        sqlx::query(
            "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, attempt_count, created_at, updated_at) VALUES ('model-a-work', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'same-source', 'protocol-1', 'model-a', '{}', 'config-a', 'blocked_config', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), ('model-b-work', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'same-source', 'protocol-1', 'model-b', '{}', 'config-b', 'ready', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed model-specific work rows");
        sqlx::query(
            "INSERT INTO content_request_links (id, request_id, work_item_id, requester_type, requester_id, authorization_snapshot_json, producer_ref, request_source, delivery_mode, created_at, updated_at) VALUES ('request-link-a', 'request-a', 'model-a-work', 'user', 'user-a', '{}', 'test', 'test', 'async', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed requester association");
        sqlx::query(
            "INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, retry_eligible, created_at) VALUES ('attempt-start-a', 'model-a-work', 1, 'initial', 'attempt_started', 0, CURRENT_TIMESTAMP), ('attempt-end-a', 'model-a-work', 1, 'initial', 'attempt_completed', 0, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed attempt history");
        sqlx::query(
            "INSERT INTO content_attempt_llm_calls (id, attempt_event_id, provider_call_id, model, status, created_at) VALUES ('call-a', 'attempt-start-a', 'provider-call-a', 'model-a', 'failed', CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed model-call history");
        sqlx::query(
            "INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, payload_json, published_at, updated_at) VALUES ('projection-a', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'protocol-1', 'model-a', 'same-source', 'model-a-work', '{\"title_zh\":\"A\"}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'), ('projection-b', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'protocol-1', 'model-b', 'same-source', 'model-b-work', '{\"title_zh\":\"B\"}', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("seed model-specific projection candidates");

        run(&pool)
            .await
            .expect("apply pending identity compatibility migrations");

        for table in [
            "content_work_identities",
            "content_work_identity_members",
            "content_current_result_projections",
        ] {
            let row_count = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
            ))
            .fetch_one(&pool)
            .await
            .expect("check pending identity migration");
            assert_eq!(row_count, 1, "compatibility release created {table}");
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_items WHERE id IN ('model-a-work', 'model-b-work')",
            )
            .fetch_one(&pool)
            .await
            .expect("read retained global work rows"),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_result_projections WHERE id IN ('projection-a', 'projection-b')",
            )
            .fetch_one(&pool)
            .await
            .expect("read retained projection rows"),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_request_links WHERE id = 'request-link-a'",
            )
            .fetch_one(&pool)
            .await
            .expect("read retained requester association"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'model-a-work'",
            )
            .fetch_one(&pool)
            .await
            .expect("read retained attempt events"),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_llm_calls WHERE id = 'call-a'",
            )
            .fetch_one(&pool)
            .await
            .expect("read retained provider-call association"),
            1
        );
    }
}
