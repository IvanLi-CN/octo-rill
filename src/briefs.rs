use std::{collections::BTreeSet, str::FromStr};

use anyhow::{Context, Result};
use chrono::{
    DateTime, Duration, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset,
    TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
use sqlx::FromRow;

use crate::{config::AppConfig, state::AppState};

pub const DEFAULT_DAILY_BRIEF_TIME_ZONE: &str = "Asia/Shanghai";
const SUPPORTED_TIME_ZONE_SAMPLE_YEAR: i32 = 2026;
const SUPPORTED_TIME_ZONE_SCAN_DAYS: i64 = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyBriefPreferences {
    pub local_time: NaiveTime,
    pub time_zone: String,
}

#[derive(Debug, Clone)]
pub struct DailyWindow {
    pub key_date: NaiveDate,
    pub display_date: String,
    pub start_local: DateTime<FixedOffset>,
    pub end_local: DateTime<FixedOffset>,
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
    pub effective_time_zone: String,
    pub effective_local_boundary: String,
}

#[derive(Debug, FromRow)]
struct DailyBriefPreferenceRow {
    daily_brief_local_time: Option<String>,
    daily_brief_time_zone: Option<String>,
    daily_brief_utc_time: String,
}

pub fn default_daily_brief_local_time(config: &AppConfig) -> NaiveTime {
    config
        .ai_daily_at_local
        .unwrap_or_else(|| NaiveTime::from_hms_opt(8, 0, 0).expect("08:00 is valid"))
}

pub fn default_daily_brief_time_zone(config: &AppConfig) -> &str {
    &config.app_default_time_zone
}

pub fn format_daily_brief_local_time(time: NaiveTime) -> String {
    time.format("%H:%M").to_string()
}

pub fn parse_daily_brief_local_time(raw: &str) -> Result<NaiveTime> {
    let parsed = NaiveTime::parse_from_str(raw.trim(), "%H:%M")
        .with_context(|| "invalid daily brief local time (expected HH:MM)")?;
    if parsed.minute() != 0 || parsed.second() != 0 {
        anyhow::bail!("invalid daily brief local time (expected HH:00)");
    }
    Ok(parsed)
}

pub fn parse_daily_brief_time_zone(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("invalid daily brief time zone (expected IANA time zone)");
    }
    let tz = Tz::from_str(trimmed)
        .with_context(|| "invalid daily brief time zone (expected IANA time zone)")?;
    Ok(tz.to_string())
}

fn supported_time_zone_scan_start() -> NaiveDate {
    NaiveDate::from_ymd_opt(SUPPORTED_TIME_ZONE_SAMPLE_YEAR, 1, 1)
        .expect("supported time zone scan start should be valid")
}

fn sampled_utc_offset_minutes(time_zone: Tz) -> Vec<i32> {
    let mut offsets = BTreeSet::new();
    let start_day = supported_time_zone_scan_start();
    for day_offset in 0..=SUPPORTED_TIME_ZONE_SCAN_DAYS {
        let candidate_day = start_day + Duration::days(day_offset);
        let candidate_utc = Utc.from_utc_datetime(
            &candidate_day
                .and_hms_opt(12, 0, 0)
                .expect("12:00 is always valid"),
        );
        offsets.insert(
            candidate_utc
                .with_timezone(&time_zone)
                .offset()
                .fix()
                .local_minus_utc()
                / 60,
        );
    }
    offsets.into_iter().collect()
}

pub(crate) fn canonical_supported_time_zone(raw: &str) -> Option<String> {
    parse_daily_brief_time_zone(raw)
        .ok()
        .filter(|value| validate_hour_aligned_time_zone(value, Utc::now()).is_ok())
}

fn stable_supported_time_zone_or_default(
    default_time_zone: &str,
    candidate: Option<&str>,
) -> String {
    candidate
        .and_then(canonical_supported_time_zone)
        .or_else(|| canonical_supported_time_zone(default_time_zone))
        .unwrap_or_else(|| DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned())
}

pub fn validate_hour_aligned_time_zone(
    time_zone: &str,
    _reference_utc: DateTime<Utc>,
) -> Result<()> {
    let tz = resolve_tz(time_zone)?;
    for offset_minutes in sampled_utc_offset_minutes(tz) {
        if offset_minutes % 60 != 0 {
            anyhow::bail!(
                "invalid daily brief time zone (only IANA time zones with whole-hour UTC offsets year-round are supported)"
            );
        }
    }
    Ok(())
}

fn resolve_tz(raw: &str) -> Result<Tz> {
    Tz::from_str(raw).with_context(|| format!("invalid IANA time zone: {raw}"))
}

fn resolve_local_datetime(time_zone: Tz, naive: NaiveDateTime) -> DateTime<Tz> {
    match time_zone.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(first, _) => first,
        LocalResult::None => {
            for minute_offset in 1..=180 {
                let candidate = naive + Duration::minutes(minute_offset);
                match time_zone.from_local_datetime(&candidate) {
                    LocalResult::Single(dt) => return dt,
                    LocalResult::Ambiguous(first, _) => return first,
                    LocalResult::None => continue,
                }
            }
            DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).with_timezone(&time_zone)
        }
    }
}

pub fn resolve_daily_brief_local_datetime(
    time_zone: &str,
    naive: NaiveDateTime,
) -> Result<DateTime<FixedOffset>> {
    let tz = resolve_tz(time_zone)?;
    Ok(resolve_local_datetime(tz, naive).fixed_offset())
}

pub fn convert_daily_brief_utc_to_local(
    time_zone: &str,
    utc: DateTime<Utc>,
) -> Result<DateTime<FixedOffset>> {
    let tz = resolve_tz(time_zone)?;
    Ok(utc.with_timezone(&tz).fixed_offset())
}

fn derive_local_time_from_legacy_utc(
    legacy_utc_time: &str,
    time_zone: &str,
    _reference_utc: DateTime<Utc>,
) -> Option<NaiveTime> {
    let utc_time = NaiveTime::parse_from_str(legacy_utc_time.trim(), "%H:%M").ok()?;
    if utc_time.minute() != 0 || utc_time.second() != 0 {
        return None;
    }
    let tz = resolve_tz(time_zone).ok()?;
    let stable_offset_minutes = sampled_utc_offset_minutes(tz).into_iter().min()?;
    let minutes_after_midnight =
        (utc_time.hour() as i32 * 60 + utc_time.minute() as i32 + stable_offset_minutes)
            .rem_euclid(24 * 60);
    if minutes_after_midnight % 60 != 0 {
        return None;
    }
    NaiveTime::from_hms_opt((minutes_after_midnight / 60) as u32, 0, 0)
}

pub fn derive_legacy_utc_time_from_local(
    local_time: NaiveTime,
    time_zone: &str,
) -> Option<NaiveTime> {
    if local_time.minute() != 0 || local_time.second() != 0 {
        return None;
    }
    let tz = resolve_tz(time_zone).ok()?;
    let stable_offset_minutes = sampled_utc_offset_minutes(tz).into_iter().min()?;
    let minutes_after_midnight = (local_time.hour() as i32 * 60 + local_time.minute() as i32
        - stable_offset_minutes)
        .rem_euclid(24 * 60);
    if minutes_after_midnight % 60 != 0 {
        return None;
    }
    NaiveTime::from_hms_opt((minutes_after_midnight / 60) as u32, 0, 0)
}

pub fn format_legacy_daily_brief_utc_time(
    local_time: NaiveTime,
    time_zone: &str,
) -> Result<String> {
    let derived = derive_legacy_utc_time_from_local(local_time, time_zone)
        .context("failed to derive legacy daily brief UTC fallback")?;
    Ok(format_daily_brief_local_time(derived))
}

pub fn derive_daily_brief_preferences(
    config: &AppConfig,
    daily_brief_local_time: Option<&str>,
    daily_brief_time_zone: Option<&str>,
    legacy_daily_brief_utc_time: Option<&str>,
    reference_utc: DateTime<Utc>,
) -> DailyBriefPreferences {
    derive_daily_brief_preferences_with_defaults(
        default_daily_brief_local_time(config),
        default_daily_brief_time_zone(config),
        daily_brief_local_time,
        daily_brief_time_zone,
        legacy_daily_brief_utc_time,
        reference_utc,
    )
}

pub fn derive_daily_brief_preferences_with_defaults(
    default_local_time: NaiveTime,
    default_time_zone: &str,
    daily_brief_local_time: Option<&str>,
    daily_brief_time_zone: Option<&str>,
    legacy_daily_brief_utc_time: Option<&str>,
    reference_utc: DateTime<Utc>,
) -> DailyBriefPreferences {
    let time_zone = stable_supported_time_zone_or_default(default_time_zone, daily_brief_time_zone);

    let local_time = daily_brief_local_time
        .and_then(|value| parse_daily_brief_local_time(value).ok())
        .or_else(|| {
            legacy_daily_brief_utc_time.and_then(|value| {
                derive_local_time_from_legacy_utc(value, &time_zone, reference_utc)
            })
        })
        .unwrap_or(default_local_time);

    DailyBriefPreferences {
        local_time,
        time_zone,
    }
}

pub async fn load_daily_brief_preferences(
    state: &AppState,
    user_id: &str,
) -> Result<DailyBriefPreferences> {
    let row = sqlx::query_as::<_, DailyBriefPreferenceRow>(
        r#"
        SELECT daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time
        FROM users
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .context("failed to load daily brief preferences")?;

    let Some(row) = row else {
        anyhow::bail!("user not found for daily brief preferences");
    };

    Ok(derive_daily_brief_preferences(
        &state.config,
        row.daily_brief_local_time.as_deref(),
        row.daily_brief_time_zone.as_deref(),
        Some(row.daily_brief_utc_time.as_str()),
        Utc::now(),
    ))
}

pub fn compute_daily_window_for_key_date(
    preferences: &DailyBriefPreferences,
    key_date: NaiveDate,
) -> Result<DailyWindow> {
    let time_zone = resolve_tz(&preferences.time_zone)?;
    let end_local = resolve_local_datetime(
        time_zone,
        NaiveDateTime::new(key_date, preferences.local_time),
    );
    let start_local = resolve_local_datetime(
        time_zone,
        NaiveDateTime::new(key_date, preferences.local_time) - Duration::hours(24),
    );

    Ok(DailyWindow {
        key_date,
        display_date: key_date.to_string(),
        start_utc: start_local.with_timezone(&Utc),
        end_utc: end_local.with_timezone(&Utc),
        start_local: start_local.fixed_offset(),
        end_local: end_local.fixed_offset(),
        effective_time_zone: preferences.time_zone.clone(),
        effective_local_boundary: format_daily_brief_local_time(end_local.time()),
    })
}

pub fn key_date_for_now(
    preferences: &DailyBriefPreferences,
    now_utc: DateTime<Utc>,
) -> Result<NaiveDate> {
    let time_zone = resolve_tz(&preferences.time_zone)?;
    let now_local = now_utc.with_timezone(&time_zone);
    let today = now_local.date_naive();
    Ok(if now_local.time() >= preferences.local_time {
        today
    } else {
        today - Duration::days(1)
    })
}

pub fn compute_current_daily_window(
    preferences: &DailyBriefPreferences,
    now_utc: DateTime<Utc>,
) -> Result<DailyWindow> {
    let key_date = key_date_for_now(preferences, now_utc)?;
    compute_daily_window_for_key_date(preferences, key_date)
}

pub fn current_utc_offset_minutes(
    preferences: &DailyBriefPreferences,
    now_utc: DateTime<Utc>,
) -> Result<i32> {
    let time_zone = resolve_tz(&preferences.time_zone)?;
    Ok(now_utc
        .with_timezone(&time_zone)
        .offset()
        .fix()
        .local_minus_utc()
        / 60)
}

pub fn required_daily_brief_scheduler_hours(
    local_time: NaiveTime,
    time_zone: &str,
) -> Result<Vec<u8>> {
    let tz = resolve_tz(time_zone)?;
    let start_day = supported_time_zone_scan_start();
    let mut hours = BTreeSet::new();

    for day_offset in 0..=SUPPORTED_TIME_ZONE_SCAN_DAYS {
        let candidate_day = start_day + Duration::days(day_offset);
        let local_dt = resolve_local_datetime(tz, NaiveDateTime::new(candidate_day, local_time));
        hours.insert(local_dt.with_timezone(&Utc).hour() as u8);
    }

    Ok(hours.into_iter().collect())
}

pub async fn load_enabled_daily_brief_scheduler_hours<'e, E>(executor: E) -> Result<BTreeSet<u8>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_scalar::<_, i64>(
        r#"
        SELECT hour_utc
        FROM daily_brief_hour_slots
        WHERE enabled = 1
        "#,
    )
    .fetch_all(executor)
    .await
    .context("failed to load enabled daily brief scheduler slots")?
    .into_iter()
    .map(|hour| hour as u8)
    .collect())
}

pub fn missing_daily_brief_scheduler_hours(
    local_time: NaiveTime,
    time_zone: &str,
    enabled_hours: &BTreeSet<u8>,
) -> Result<Vec<u8>> {
    Ok(required_daily_brief_scheduler_hours(local_time, time_zone)?
        .into_iter()
        .filter(|hour| !enabled_hours.contains(hour))
        .collect())
}

pub async fn backfill_legacy_daily_brief_preferences(state: &AppState) -> Result<usize> {
    #[derive(Debug, FromRow)]
    struct UserPreferenceSeedRow {
        id: String,
        daily_brief_local_time: Option<String>,
        daily_brief_time_zone: Option<String>,
        daily_brief_utc_time: String,
    }

    let rows = sqlx::query_as::<_, UserPreferenceSeedRow>(
        r#"
        SELECT id, daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time
        FROM users
        ORDER BY id ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .context("failed to query users for daily brief preference backfill")?;

    let default_local_time = default_daily_brief_local_time(&state.config);
    let default_time_zone = default_daily_brief_time_zone(&state.config);
    let enabled_hours = load_enabled_daily_brief_scheduler_hours(&state.pool).await?;
    let now = Utc::now().to_rfc3339();
    let mut updated = 0usize;
    let mut skipped = 0usize;

    for row in rows {
        let derived = derive_daily_brief_preferences_with_defaults(
            default_local_time,
            default_time_zone,
            row.daily_brief_local_time.as_deref(),
            row.daily_brief_time_zone.as_deref(),
            Some(row.daily_brief_utc_time.as_str()),
            Utc::now(),
        );
        let derived_local_time = format_daily_brief_local_time(derived.local_time);
        let current_local_time = row
            .daily_brief_local_time
            .as_deref()
            .and_then(|value| parse_daily_brief_local_time(value).ok())
            .map(format_daily_brief_local_time);
        let current_time_zone = row
            .daily_brief_time_zone
            .as_deref()
            .and_then(canonical_supported_time_zone);

        if current_local_time.as_deref() == Some(derived_local_time.as_str())
            && current_time_zone.as_deref() == Some(derived.time_zone.as_str())
        {
            continue;
        }
        let missing_hours = missing_daily_brief_scheduler_hours(
            derived.local_time,
            &derived.time_zone,
            &enabled_hours,
        )?;
        if !missing_hours.is_empty() {
            let missing_hours = missing_hours
                .into_iter()
                .map(|hour| format!("{hour:02}:00Z"))
                .collect::<Vec<_>>()
                .join(", ");
            tracing::warn!(
                user_id = %row.id,
                local_time = %derived_local_time,
                time_zone = %derived.time_zone,
                missing_enabled_utc_slots = %missing_hours,
                "skipping legacy daily brief preference backfill because required scheduler slots are disabled"
            );
            skipped += 1;
            continue;
        }

        sqlx::query(
            r#"
            UPDATE users
            SET daily_brief_local_time = ?, daily_brief_time_zone = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(derived_local_time)
        .bind(&derived.time_zone)
        .bind(now.as_str())
        .bind(&row.id)
        .execute(&state.pool)
        .await
        .with_context(|| {
            format!(
                "failed to backfill daily brief preferences for user {}",
                row.id
            )
        })?;
        updated += 1;
    }

    if skipped > 0 {
        tracing::warn!(
            skipped_users = skipped,
            "skipped legacy daily brief preference backfill for users requiring disabled scheduler slots"
        );
    }

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::SocketAddr, sync::Arc};

    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use url::Url;

    use crate::{
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        state::{AppState, build_oauth_client},
    };

    async fn setup_pool() -> SqlitePool {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-briefs-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create sqlite db");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login, daily_brief_utc_time, created_at, updated_at
            )
            VALUES ('user-briefs-test', 101, 'briefs-test', '13:00', '2026-02-25T10:00:00Z', '2026-02-25T10:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed legacy user");
        pool
    }

    fn setup_state(pool: SqlitePool) -> Arc<AppState> {
        let encryption_key =
            EncryptionKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                .expect("build encryption key");
        let config = AppConfig {
            bind_addr: "127.0.0.1:58090"
                .parse::<SocketAddr>()
                .expect("parse bind addr"),
            public_base_url: Url::parse("http://127.0.0.1:58090").expect("parse public base url"),
            database_url: "sqlite::memory:".to_owned(),
            static_dir: None,
            task_log_dir: std::env::temp_dir().join("octo-rill-briefs-task-logs-tests"),
            job_worker_concurrency: 4,
            encryption_key: encryption_key.clone(),
            github: GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse github redirect"),
            },
            ai: None,
            ai_max_concurrency: 1,
            ai_model_context_limit: None,
            ai_daily_at_local: None,
            app_default_time_zone: DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned(),
        };
        let oauth = build_oauth_client(&config).expect("build oauth client");
        Arc::new(AppState {
            llm_scheduler: Arc::new(crate::ai::LlmScheduler::new(config.ai_max_concurrency)),
            translation_scheduler: Arc::new(
                crate::translations::TranslationSchedulerController::new(
                    crate::translations::TranslationRuntimeConfig::default(),
                ),
            ),
            config,
            pool,
            http: reqwest::Client::new(),
            oauth,
            encryption_key,
            runtime_owner_id: "briefs-test-runtime-owner".to_owned(),
        })
    }

    #[test]
    fn parse_local_time_rejects_non_hour_input() {
        assert!(parse_daily_brief_local_time("08:30").is_err());
        assert!(parse_daily_brief_local_time("08:00").is_ok());
    }

    #[test]
    fn compute_window_uses_requested_timezone() {
        let preferences = DailyBriefPreferences {
            local_time: NaiveTime::from_hms_opt(8, 0, 0).expect("08:00"),
            time_zone: "Asia/Shanghai".to_owned(),
        };

        let window = compute_daily_window_for_key_date(
            &preferences,
            NaiveDate::from_ymd_opt(2026, 4, 12).expect("date"),
        )
        .expect("window");

        assert_eq!(window.start_utc.to_rfc3339(), "2026-04-11T00:00:00+00:00");
        assert_eq!(window.end_utc.to_rfc3339(), "2026-04-12T00:00:00+00:00");
        assert_eq!(window.effective_time_zone, "Asia/Shanghai");
        assert_eq!(window.effective_local_boundary, "08:00");
    }

    #[test]
    fn compute_window_uses_resolved_boundary_for_dst_gap() {
        let preferences = DailyBriefPreferences {
            local_time: NaiveTime::from_hms_opt(2, 0, 0).expect("02:00"),
            time_zone: "America/New_York".to_owned(),
        };

        let window = compute_daily_window_for_key_date(
            &preferences,
            NaiveDate::from_ymd_opt(2026, 3, 8).expect("spring-forward date"),
        )
        .expect("window");

        assert_eq!(
            window.end_local.time(),
            NaiveTime::from_hms_opt(3, 0, 0).unwrap()
        );
        assert_eq!(window.effective_local_boundary, "03:00");
    }

    #[test]
    fn validate_hour_aligned_time_zone_rejects_half_hour_offsets() {
        let reference_utc = Utc
            .with_ymd_and_hms(2026, 4, 13, 0, 0, 0)
            .single()
            .expect("valid datetime");

        assert!(validate_hour_aligned_time_zone("Asia/Shanghai", reference_utc).is_ok());
        assert!(validate_hour_aligned_time_zone("Asia/Kolkata", reference_utc).is_err());
    }

    #[test]
    fn validate_hour_aligned_time_zone_rejects_seasonal_half_hour_offsets() {
        let reference_utc = Utc
            .with_ymd_and_hms(2026, 1, 13, 0, 0, 0)
            .single()
            .expect("valid datetime");

        assert!(validate_hour_aligned_time_zone("Australia/Lord_Howe", reference_utc).is_err());
    }

    #[test]
    fn derive_legacy_local_time_is_stable_across_reference_seasons() {
        let winter = Utc
            .with_ymd_and_hms(2026, 1, 13, 0, 0, 0)
            .single()
            .expect("valid winter datetime");
        let summer = Utc
            .with_ymd_and_hms(2026, 7, 13, 0, 0, 0)
            .single()
            .expect("valid summer datetime");

        let winter_preferences = derive_daily_brief_preferences_with_defaults(
            NaiveTime::from_hms_opt(8, 0, 0).expect("08:00"),
            "America/New_York",
            None,
            Some("America/New_York"),
            Some("13:00"),
            winter,
        );
        let summer_preferences = derive_daily_brief_preferences_with_defaults(
            NaiveTime::from_hms_opt(8, 0, 0).expect("08:00"),
            "America/New_York",
            None,
            Some("America/New_York"),
            Some("13:00"),
            summer,
        );

        assert_eq!(
            format_daily_brief_local_time(winter_preferences.local_time),
            "08:00"
        );
        assert_eq!(
            format_daily_brief_local_time(summer_preferences.local_time),
            "08:00"
        );
    }

    #[test]
    fn required_scheduler_hours_include_both_dst_and_standard_utc_hours() {
        let hours = required_daily_brief_scheduler_hours(
            NaiveTime::from_hms_opt(8, 0, 0).expect("08:00"),
            "America/New_York",
        )
        .expect("required scheduler hours");

        assert_eq!(hours, vec![12, 13]);
    }

    #[tokio::test]
    async fn backfill_legacy_preferences_persists_stable_local_boundary() {
        let pool = setup_pool().await;
        sqlx::query(
            r#"
            UPDATE users
            SET daily_brief_time_zone = 'America/New_York'
            WHERE id = 'user-briefs-test'
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed time zone");
        let state = setup_state(pool.clone());

        let updated = backfill_legacy_daily_brief_preferences(state.as_ref())
            .await
            .expect("backfill legacy preferences");

        assert_eq!(updated, 1);

        let row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            r#"
            SELECT daily_brief_local_time, daily_brief_time_zone
            FROM users
            WHERE id = 'user-briefs-test'
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("load backfilled preferences");

        assert_eq!(row.0.as_deref(), Some("08:00"));
        assert_eq!(row.1.as_deref(), Some("America/New_York"));
    }

    #[tokio::test]
    async fn backfill_legacy_preferences_skips_unschedulable_slots() {
        let pool = setup_pool().await;
        sqlx::query(
            r#"
            UPDATE daily_brief_hour_slots
            SET enabled = CASE WHEN hour_utc = 13 THEN 0 ELSE enabled END
            "#,
        )
        .execute(&pool)
        .await
        .expect("disable required slot");
        let state = setup_state(pool.clone());

        let updated = backfill_legacy_daily_brief_preferences(state.as_ref())
            .await
            .expect("backfill should skip unschedulable defaults");

        assert_eq!(updated, 0);

        let row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            r#"
            SELECT daily_brief_local_time, daily_brief_time_zone
            FROM users
            WHERE id = 'user-briefs-test'
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("load skipped backfill preferences");

        assert_eq!(row.0, None);
        assert_eq!(row.1, None);
    }
}
