use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha384};
use sqlx::{SqlitePool, migrate::Migrator};

const LEGACY_SEARCH_MIGRATION_VERSION: i64 = 81;
const LEGACY_SEARCH_MIGRATION_SQL: &[u8] =
    include_bytes!("../migrations/legacy/0081_command_palette_search.sql");

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone)]
struct AppliedMigration {
    version: i64,
    checksum: Vec<u8>,
    success: i64,
}

pub async fn run(pool: &SqlitePool) -> Result<()> {
    let applied = load_applied_migrations(pool).await?;
    validate_history(applied.as_deref(), &MIGRATOR)?;

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
        if migration.success == 0 {
            bail!("database migration {} is dirty", migration.version);
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
        if migration.checksum.as_slice() != *checksum {
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
    use sqlx::sqlite::SqlitePoolOptions;

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
    async fn full_0082_history_upgrades_to_reconcile_demands_without_losing_observations() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::raw_sql(
            r#"
            CREATE TABLE _sqlx_migrations (
              version BIGINT PRIMARY KEY NOT NULL,
              description TEXT NOT NULL,
              installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
              success BOOLEAN NOT NULL,
              checksum BLOB NOT NULL,
              execution_time BIGINT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("create migration history");

        for migration in MIGRATOR.iter().filter(|migration| migration.version < 83) {
            sqlx::raw_sql(&migration.sql)
                .execute(&pool)
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "apply migration {} {}: {error}",
                        migration.version, migration.description
                    )
                });
            sqlx::query(
                "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?, ?, 1, ?, 0)",
            )
            .bind(migration.version)
            .bind(migration.description.as_ref())
            .bind(migration.checksum.as_ref())
            .execute(&pool)
            .await
            .expect("record applied migration");
        }

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

        run(&pool).await.expect("apply reconcile demand migration");

        let demand_table = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'webhook_push_reconcile_demands'",
        )
        .fetch_one(&pool)
        .await
        .expect("read demand table");
        assert_eq!(demand_table, 1);
        let scope = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT webhook_push_allows_private_repos FROM reaction_pat_tokens WHERE user_id = 'user-1'",
        )
        .fetch_one(&pool)
        .await
        .expect("read nullable PAT scope");
        assert_eq!(scope, None);
        let hook_id = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT hook_id FROM webhook_push_repos WHERE user_id = 'user-1' AND repo_id = 101",
        )
        .fetch_one(&pool)
        .await
        .expect("read retained webhook observation");
        assert_eq!(hook_id, Some(9001));
    }

    #[tokio::test]
    async fn identity_compatibility_migrator_reopens_schema_and_pre_0084_build_is_rejected() {
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
            .expect("apply identity compatibility migration");
        run(&pool)
            .await
            .expect("compatibility build reopens migrated schema");

        for table in [
            "content_work_identities",
            "content_work_identity_members",
            "content_current_result_projections",
        ] {
            let row_count = sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .expect("read empty identity upgrade table");
            assert_eq!(row_count, 0, "compatibility release backfilled {table}");
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
        let attempt_snapshot: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT configuration_snapshot_json, route_snapshot_json, configuration_fingerprint FROM content_attempt_events WHERE id = 'attempt-start-a'",
        )
        .fetch_one(&pool)
        .await
        .expect("read nullable compatibility snapshot columns");
        assert_eq!(attempt_snapshot, (None, None, None));

        let pre_identity_compatibility_migrator = Migrator {
            migrations: Cow::Owned(
                current_migrations
                    .iter()
                    .filter(|migration| migration.version < 84)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: false,
            locking: true,
            no_tx: false,
        };
        let error = pre_identity_compatibility_migrator
            .run(&pool)
            .await
            .expect_err("a pre-0084 build must not open the upgraded database");
        assert!(matches!(
            error,
            sqlx::migrate::MigrateError::VersionMissing(84)
        ));
    }
}
