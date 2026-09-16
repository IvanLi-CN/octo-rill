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
}
