use anyhow::{Context, Result};
use sqlx::{PgPool, migrate::Migrator};

/// PostgreSQL keeps an independent SQLx migration history. The SQLite
/// `_sqlx_migrations` table is deliberately never read or reused here.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations_pg/");

pub async fn run(pool: &PgPool) -> Result<()> {
    let mut tx = crate::postgres_write::begin(pool)
        .await
        .context("begin PostgreSQL migrations")?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('octo-rill:runtime-baseline', 0))")
        .execute(&mut *tx)
        .await
        .context("lock PostgreSQL migration history")?;
    MIGRATOR
        .run(&mut *tx)
        .await
        .context("failed to apply PostgreSQL migrations")?;
    tx.commit().await.context("commit PostgreSQL migrations")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MIGRATOR;

    #[test]
    fn postgres_migrator_has_an_independent_baseline() {
        assert_eq!(MIGRATOR.iter().count(), 21);
        assert_eq!(MIGRATOR.iter().next().expect("baseline").version, 1);
        let content = MIGRATOR
            .iter()
            .find(|migration| migration.version == 6)
            .expect("content processing migration");
        assert!(content.sql.contains("content_work_items"));
        assert!(content.sql.contains("timestamptz"));
        assert!(content.sql.contains("content_result_projections"));
        let translation = MIGRATOR
            .iter()
            .find(|migration| migration.version == 7)
            .expect("translation migration");
        assert!(translation.sql.contains("translation_work_items"));
        assert!(translation.sql.contains("translation_batches"));
        let audit = MIGRATOR
            .iter()
            .find(|migration| migration.version == 8)
            .expect("translation audit migration");
        assert!(audit.sql.contains("translation_attempt_events"));
        assert!(audit.sql.contains("llm_recovery_flags"));
        let audit_ids = MIGRATOR
            .iter()
            .find(|migration| migration.version == 19)
            .expect("translation audit id compatibility migration");
        assert!(audit_ids.sql.contains("translation_attempt_events"));
        assert!(audit_ids.sql.contains("translation_attempt_llm_calls"));
        let attempt_unique = MIGRATOR
            .iter()
            .find(|migration| migration.version == 20)
            .expect("translation attempt unique migration");
        assert!(attempt_unique.sql.contains("translation_attempt_events"));
        assert!(
            attempt_unique
                .sql
                .contains("work_item_id, attempt_no, event_type")
        );
        assert!(
            attempt_unique
                .sql
                .contains("CREATE UNIQUE INDEX IF NOT EXISTS")
        );
        assert!(
            attempt_unique
                .sql
                .contains("translation_attempt_events_work_item_id_attempt_no_event_ty_key")
        );
        assert!(
            attempt_unique
                .sql
                .contains("DELETE FROM public.translation_attempt_events")
        );
        let health_timestamps = MIGRATOR
            .iter()
            .find(|migration| migration.version == 21)
            .expect("llm model health timestamp migration");
        assert!(health_timestamps.sql.contains("llm_model_health"));
        assert!(health_timestamps.sql.contains("TIMESTAMPTZ"));
        assert!(audit_ids.sql.contains("ALTER COLUMN id TYPE text"));
        let brief = MIGRATOR
            .iter()
            .find(|migration| migration.version == 9)
            .expect("brief migration");
        assert!(brief.sql.contains("briefs"));
        let application = MIGRATOR
            .iter()
            .find(|migration| migration.version == 15)
            .expect("application schema migration");
        assert!(application.sql.contains("public.repo_releases"));
        assert!(application.sql.contains("public.user_repo_associations"));
        assert!(
            application
                .sql
                .contains("CREATE OR REPLACE VIEW public.user_release_visible_repos")
        );
        assert!(!application.sql.contains("OWNER TO"));
        let baseline = MIGRATOR.iter().next().expect("runtime baseline");
        for table in [
            "public.users",
            "public.github_connections",
            "public.linuxdo_connections",
            "public.user_passkeys",
            "public.user_api_keys",
        ] {
            assert!(baseline.sql.contains(table), "baseline missing {table}");
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn translation_audit_id_migration_normalizes_imported_bigint_ids() -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("migration_ids_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE translation_attempt_events(id bigint PRIMARY KEY,work_item_id text NOT NULL);
                CREATE TABLE translation_attempt_llm_calls(id bigint PRIMARY KEY,work_item_id text NOT NULL);
                INSERT INTO translation_attempt_events(id,work_item_id) VALUES(1,'work-a');
                INSERT INTO translation_attempt_llm_calls(id,work_item_id) VALUES(1,'work-a');
                "#,
            )
            .execute(&pool)
            .await?;
            sqlx::raw_sql(include_str!(
                "../migrations_pg/0019_translation_audit_id_compatibility.sql"
            ))
            .execute(&pool)
            .await?;
            for table in ["translation_attempt_events", "translation_attempt_llm_calls"] {
                let data_type: String = sqlx::query_scalar(
                    "SELECT data_type FROM information_schema.columns WHERE table_schema='public' AND table_name=$1 AND column_name='id'",
                )
                .bind(table)
                .fetch_one(&pool)
                .await?;
                assert_eq!(data_type, "text", "{table}.id must be text");
            }
            let event_id: String = sqlx::query_scalar(
                "SELECT id FROM translation_attempt_events WHERE work_item_id='work-a'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(event_id, "1");
            let call_id: String = sqlx::query_scalar(
                "SELECT id FROM translation_attempt_llm_calls WHERE work_item_id='work-a'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(call_id, "1");
            sqlx::query(
                "INSERT INTO translation_attempt_events(id,work_item_id) VALUES($1,'work-b')",
            )
            .bind(crate::local_id::generate_local_id())
            .execute(&pool)
            .await?;
            sqlx::query(
                "INSERT INTO translation_attempt_llm_calls(id,work_item_id) VALUES($1,'work-b')",
            )
            .bind(crate::local_id::generate_local_id())
            .execute(&pool)
            .await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await?;
        admin.close().await;
        result
    }
}
