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

    type OptionalUtc = Option<chrono::DateTime<chrono::Utc>>;
    type TimestampTriple = (OptionalUtc, OptionalUtc, OptionalUtc);
    type TimestampRow = (
        OptionalUtc,
        OptionalUtc,
        OptionalUtc,
        chrono::DateTime<chrono::Utc>,
    );

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
        assert!(
            health_timestamps
                .sql
                .contains("ALTER TABLE public.llm_model_health")
        );
        assert!(
            health_timestamps
                .sql
                .contains("ALTER COLUMN cooldown_until TYPE timestamptz")
        );
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

    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn llm_model_health_timestamp_migration_normalizes_imported_text_columns()
    -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("migration_health_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE public.llm_model_health(model text PRIMARY KEY, relevant_failure_count integer NOT NULL DEFAULT 0, window_started_at text, cooldown_until text, last_failure_class text, last_failure_at text, created_at text NOT NULL, updated_at text NOT NULL);
                INSERT INTO public.llm_model_health(model, relevant_failure_count, window_started_at, cooldown_until, last_failure_class, last_failure_at, created_at, updated_at) VALUES ('m', 2, '2026-11-21T08:36:41.688Z', '2026-11-21T08:46:41.688Z', 'rate_limited', '2026-11-21T08:36:41.688Z', '2026-11-20T00:00:00Z', '2026-11-21T08:36:41.688Z');
                INSERT INTO public.llm_model_health(model, relevant_failure_count, window_started_at, cooldown_until, last_failure_class, last_failure_at, created_at, updated_at) VALUES ('m-empty', 0, '', '', NULL, '', '2026-11-20T00:00:00Z', '2026-11-20T00:00:00Z');
                "#,
            )
            .execute(&pool)
            .await?;
            let migration = include_str!("../migrations_pg/0021_llm_model_health_timestamps.sql");
            sqlx::raw_sql(migration).execute(&pool).await?;
            for column in [
                "window_started_at",
                "cooldown_until",
                "last_failure_at",
                "created_at",
                "updated_at",
            ] {
                let data_type: String = sqlx::query_scalar(
                    "SELECT data_type FROM information_schema.columns WHERE table_schema='public' AND table_name='llm_model_health' AND column_name=$1",
                )
                .bind(column)
                .fetch_one(&pool)
                .await?;
                assert_eq!(data_type, "timestamp with time zone", "{column}");
            }
            let (window, cooldown, last_failure, created): TimestampRow = sqlx::query_as(
                "SELECT window_started_at, cooldown_until, last_failure_at, created_at FROM public.llm_model_health WHERE model = 'm'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(
                window,
                Some("2026-11-21T08:36:41.688Z".parse::<chrono::DateTime<chrono::Utc>>()?)
            );
            assert_eq!(
                cooldown,
                Some("2026-11-21T08:46:41.688Z".parse::<chrono::DateTime<chrono::Utc>>()?)
            );
            assert_eq!(
                last_failure,
                Some("2026-11-21T08:36:41.688Z".parse::<chrono::DateTime<chrono::Utc>>()?)
            );
            assert_eq!(
                created,
                "2026-11-20T00:00:00Z".parse::<chrono::DateTime<chrono::Utc>>()?
            );
            let empty: TimestampTriple = sqlx::query_as(
                "SELECT window_started_at, cooldown_until, last_failure_at FROM public.llm_model_health WHERE model = 'm-empty'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(empty, (None, None, None));
            sqlx::raw_sql(migration).execute(&pool).await?;
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public.llm_model_health")
                .fetch_one(&pool)
                .await?;
            assert_eq!(count, 2);
            sqlx::query("DROP TABLE public.llm_model_health")
                .execute(&pool)
                .await?;
            sqlx::raw_sql(
                r#"
                CREATE TABLE public.llm_model_health(model text PRIMARY KEY, relevant_failure_count integer NOT NULL DEFAULT 0, window_started_at text, cooldown_until timestamptz, last_failure_class text, last_failure_at text, created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL);
                INSERT INTO public.llm_model_health(model, relevant_failure_count, window_started_at, cooldown_until, last_failure_class, last_failure_at, created_at, updated_at) VALUES ('mixed', 2, '2026-11-21T08:36:41.688Z', '2026-11-21T08:46:41.688Z', 'rate_limited', '2026-11-21T08:36:41.688Z', '2026-11-20T00:00:00Z', '2026-11-21T08:36:41.688Z');
                "#,
            )
            .execute(&pool)
            .await?;
            sqlx::raw_sql(migration).execute(&pool).await?;
            for column in [
                "window_started_at",
                "cooldown_until",
                "last_failure_at",
                "created_at",
                "updated_at",
            ] {
                let data_type: String = sqlx::query_scalar(
                    "SELECT data_type FROM information_schema.columns WHERE table_schema='public' AND table_name='llm_model_health' AND column_name=$1",
                )
                .bind(column)
                .fetch_one(&pool)
                .await?;
                assert_eq!(data_type, "timestamp with time zone", "{column}");
            }
            let (window, cooldown, last_failure, created): TimestampRow = sqlx::query_as(
                "SELECT window_started_at, cooldown_until, last_failure_at, created_at FROM public.llm_model_health WHERE model = 'mixed'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(
                window,
                Some("2026-11-21T08:36:41.688Z".parse::<chrono::DateTime<chrono::Utc>>()?)
            );
            assert_eq!(
                cooldown,
                Some("2026-11-21T08:46:41.688Z".parse::<chrono::DateTime<chrono::Utc>>()?)
            );
            assert_eq!(
                last_failure,
                Some("2026-11-21T08:36:41.688Z".parse::<chrono::DateTime<chrono::Utc>>()?)
            );
            assert_eq!(
                created,
                "2026-11-20T00:00:00Z".parse::<chrono::DateTime<chrono::Utc>>()?
            );
            sqlx::query("DROP TABLE public.llm_model_health")
                .execute(&pool)
                .await?;
            sqlx::raw_sql(migration).execute(&pool).await?;
            let absent: bool = sqlx::query_scalar(
                "SELECT to_regclass('public.llm_model_health') IS NULL",
            )
            .fetch_one(&pool)
            .await?;
            assert!(absent);
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
