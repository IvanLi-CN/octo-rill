//! PostgreSQL-native scheduler lanes.
//!
//! The legacy SQLite runtime spawned seven enqueue-only schedulers that decided
//! what to enqueue: daily brief hour slots, subscription runs, star sync, the
//! recent-failure retry, webhook push reconcile/audit, account pause maintenance
//! and the admin dashboard rollup. This module owns their PostgreSQL
//! equivalents.
//!
//! Every lane is enqueue-only: it reads and writes PostgreSQL tables and inserts
//! `public.job_tasks` rows. GitHub, LLM and webhook traffic belongs to the
//! registered executors in [`crate::postgres_tasks`], never to a lane.
//!
//! Two legacy rules need a PostgreSQL-native shape. The SQLite runtime coalesced
//! overlapping dispatches through an in-process background lane
//! (`try_begin_background_lane`); here every bucket claim runs
//! `INSERT ... ON CONFLICT DO UPDATE ... WHERE last_dispatch_key IS DISTINCT
//! FROM EXCLUDED.last_dispatch_key` inside the dispatch transaction, so two
//! runtimes can never both own a bucket, and a failed dispatch rolls the claim
//! back. Task inserts carry their own `WHERE NOT EXISTS` single-flight guard. The
//! webhook push audit task type has no PostgreSQL executor, so the audit lane
//! performs the dispatch the audit would have performed (one
//! `webhook.push.manage` reconcile per user) and records
//! `webhook_push_audit_last_started_at` when it does.

use anyhow::{Context, Result, anyhow};
use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, FixedOffset, NaiveDateTime, NaiveTime,
    TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::str::FromStr;
use tokio::sync::watch;
use tokio::time::Duration;
use tracing::warn;

use crate::{
    admin_runtime, briefs, config::AppConfig, jobs, local_id, postgres_server::PostgresServerState,
    postgres_star_epochs,
};

/// Lane identifiers. Logged as the stable `lane` field on lane failures and
/// accepted by [`run_lane_once`] so validation suites can tick one lane.
pub(crate) const LANE_DAILY_BRIEF: &str = "daily_brief";
pub(crate) const LANE_SUBSCRIPTIONS: &str = "subscriptions";
pub(crate) const LANE_STAR_SYNC: &str = "star_sync";
pub(crate) const LANE_RECENT_FAILURES: &str = "recent_failures";
pub(crate) const LANE_WEBHOOK_PUSH: &str = "webhook_push";
pub(crate) const LANE_ACCOUNT_PAUSE: &str = "account_pause";
pub(crate) const LANE_ADMIN_ROLLUP: &str = "admin_rollup";

const DAILY_BRIEF_INTERVAL: Duration = Duration::from_secs(45);
const SUBSCRIPTION_INTERVAL: Duration = Duration::from_secs(20);
const STAR_SYNC_INTERVAL: Duration = Duration::from_secs(20);
const RECENT_FAILURES_INTERVAL: Duration = Duration::from_secs(20);
const WEBHOOK_PUSH_INTERVAL: Duration = Duration::from_secs(60);
const ACCOUNT_PAUSE_INTERVAL: Duration = Duration::from_secs(45);
const ADMIN_ROLLUP_INTERVAL: Duration = Duration::from_secs(15 * 60);

const SUBSCRIPTION_SCHEDULE_NAME: &str = "sync.subscriptions";
const STAR_SYNC_DELTA_SCHEDULE_NAME_PREFIX: &str = "sync.starred.delta";
const RETRY_RECENT_FAILURES_SCHEDULE_NAME: &str = "retry.recent_failures";
const ACCOUNT_PAUSE_SCHEDULE_NAME: &str = "account.pause_inactive";
const ACCOUNT_PAUSE_LOCAL_TIME: NaiveTime =
    NaiveTime::from_hms_opt(3, 15, 0).expect("valid fixed local time");
const ACCOUNT_PAUSE_INACTIVITY_DAYS: i64 = 30;

const RETRY_LOOKBACK_HOURS: i64 = 24;
const RETRY_MAX_BRIEFS: i64 = 100;
const WEBHOOK_AUDIT_DEFAULT_INTERVAL_DAYS: i64 = 7;
const WEBHOOK_AUDIT_MIN_INTERVAL_DAYS: i64 = 1;
const WEBHOOK_AUDIT_MAX_INTERVAL_DAYS: i64 = 30;

const SUBSCRIPTION_TASK_TYPES: [&str; 2] =
    [jobs::TASK_SYNC_RELEASES, jobs::TASK_SYNC_NOTIFICATIONS];

const ADMIN_DASHBOARD_ROLLUP_TASK_TYPES: [&str; 3] = [
    jobs::TASK_TRANSLATE_RELEASE_BATCH,
    jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH,
    jobs::TASK_BRIEF_DAILY_SLOT,
];

/// Deployment defaults the lanes need besides their PostgreSQL tables.
#[derive(Clone, Debug)]
pub(crate) struct SchedulerDefaults {
    pub(crate) time_zone: String,
    pub(crate) local_time: NaiveTime,
}

impl SchedulerDefaults {
    pub(crate) fn from_config(config: &AppConfig) -> Self {
        Self {
            time_zone: config.app_default_time_zone.clone(),
            local_time: briefs::default_daily_brief_local_time(config),
        }
    }
}

macro_rules! spawn_lane {
    ($lane:expr, $interval:expr, $pool:expr, $defaults:expr, $shutdown:expr, $tick:path) => {{
        let pool = $pool;
        let defaults = $defaults;
        let mut shutdown = $shutdown;
        tokio::spawn(async move {
            loop {
                if let Err(error) = $tick(&pool, &defaults, Utc::now()).await {
                    log_lane_failure($lane, &error);
                }
                if shutdown_requested(&mut shutdown, $interval).await {
                    return;
                }
            }
        });
    }};
}

/// Start one tokio task per lane and stop every lane when `shutdown` flips.
pub(crate) fn spawn(state: PostgresServerState, shutdown: watch::Receiver<bool>) {
    // Validation suites exercise handlers with deterministic fixtures and tick
    // individual lanes directly; the periodic goroutine must not interfere.
    if matches!(
        std::env::var("OCTORILL_PG_SCHEDULER_ENABLED").as_deref(),
        Ok("0")
    ) {
        tracing::info!("PostgreSQL schedulers disabled by OCTORILL_PG_SCHEDULER_ENABLED");
        return;
    }
    let pool = state.runtime.pool.clone();
    let defaults = SchedulerDefaults {
        time_zone: state.auth_ext.default_daily_brief_time_zone.clone(),
        local_time: state.auth_ext.default_daily_brief_local_time,
    };
    spawn_lane!(
        LANE_DAILY_BRIEF,
        DAILY_BRIEF_INTERVAL,
        pool.clone(),
        defaults.clone(),
        shutdown.clone(),
        lane_daily_brief
    );
    spawn_lane!(
        LANE_SUBSCRIPTIONS,
        SUBSCRIPTION_INTERVAL,
        pool.clone(),
        defaults.clone(),
        shutdown.clone(),
        lane_subscriptions
    );
    spawn_lane!(
        LANE_STAR_SYNC,
        STAR_SYNC_INTERVAL,
        pool.clone(),
        defaults.clone(),
        shutdown.clone(),
        lane_star_sync
    );
    spawn_lane!(
        LANE_RECENT_FAILURES,
        RECENT_FAILURES_INTERVAL,
        pool.clone(),
        defaults.clone(),
        shutdown.clone(),
        lane_recent_failures
    );
    spawn_lane!(
        LANE_WEBHOOK_PUSH,
        WEBHOOK_PUSH_INTERVAL,
        pool.clone(),
        defaults.clone(),
        shutdown.clone(),
        lane_webhook_push
    );
    spawn_lane!(
        LANE_ACCOUNT_PAUSE,
        ACCOUNT_PAUSE_INTERVAL,
        pool.clone(),
        defaults.clone(),
        shutdown.clone(),
        lane_account_pause
    );
    spawn_lane!(
        LANE_ADMIN_ROLLUP,
        ADMIN_ROLLUP_INTERVAL,
        pool,
        defaults,
        shutdown,
        lane_admin_rollup
    );
}

fn log_lane_failure(lane: &'static str, error: &anyhow::Error) {
    warn!(
        lane,
        event = "scheduler.dispatch_failed",
        ?error,
        "PostgreSQL scheduler lane failed"
    );
}

async fn shutdown_requested(shutdown: &mut watch::Receiver<bool>, interval: Duration) -> bool {
    if *shutdown.borrow() {
        return true;
    }
    tokio::select! {
        changed = shutdown.changed() => changed.is_err() || *shutdown.borrow(),
        _ = tokio::time::sleep(interval) => false,
    }
}

/// Run one lane tick outside the long-lived runtime.
///
/// Validation suites call this instead of waiting for tokio timers, so lane
/// behaviour and enqueue counts stay deterministic.
pub(crate) async fn run_lane_once(
    pool: &PgPool,
    lane: &str,
    defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<usize> {
    match lane {
        LANE_DAILY_BRIEF => lane_daily_brief(pool, defaults, now).await,
        LANE_SUBSCRIPTIONS => lane_subscriptions(pool, defaults, now).await,
        LANE_STAR_SYNC => lane_star_sync(pool, defaults, now).await,
        LANE_RECENT_FAILURES => lane_recent_failures(pool, defaults, now).await,
        LANE_WEBHOOK_PUSH => lane_webhook_push(pool, defaults, now).await,
        LANE_ACCOUNT_PAUSE => lane_account_pause(pool, defaults, now).await,
        LANE_ADMIN_ROLLUP => lane_admin_rollup(pool, defaults, now).await,
        other => Err(anyhow!("unknown PostgreSQL scheduler lane {other}")),
    }
}

async fn lane_daily_brief(
    pool: &PgPool,
    defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<usize> {
    Ok(enqueue_daily_brief_slot_if_due(pool, defaults, now)
        .await?
        .len())
}

async fn lane_subscriptions(
    pool: &PgPool,
    _defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<usize> {
    Ok(enqueue_subscription_runs_if_due(pool, now).await?.len())
}

async fn lane_star_sync(
    pool: &PgPool,
    _defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<usize> {
    let mut task_ids = enqueue_star_sync_runs_if_due(pool, now).await?;
    task_ids.extend(enqueue_due_star_epoch_slices(pool, now).await?);
    Ok(task_ids.len())
}

async fn lane_recent_failures(
    pool: &PgPool,
    _defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<usize> {
    Ok(enqueue_recent_failures_retry_if_due(pool, now).await?.len())
}

async fn lane_webhook_push(
    pool: &PgPool,
    _defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<usize> {
    Ok(dispatch_webhook_push_if_due(pool, now).await?.len())
}

async fn lane_account_pause(
    pool: &PgPool,
    defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<usize> {
    let paused = run_account_pause_maintenance_if_due(pool, defaults, now).await?;
    Ok(usize::try_from(paused.unwrap_or(0).max(0)).unwrap_or(0))
}

async fn lane_admin_rollup(
    pool: &PgPool,
    defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<usize> {
    refresh_admin_dashboard_rollups(
        pool,
        defaults,
        crate::api::ADMIN_DASHBOARD_PREAGGREGATE_DAYS,
        now,
    )
    .await
}

// ---------------------------------------------------------------------------
// daily brief hour slots
// ---------------------------------------------------------------------------

/// Enqueue one `brief.daily_slot` task per user whose local brief time falls in
/// the current UTC hour, exactly once per enabled hour slot.
pub(crate) async fn enqueue_daily_brief_slot_if_due(
    pool: &PgPool,
    defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    let hour_utc = i64::from(now.hour());
    let hour_key = now.format("%Y-%m-%dT%H").to_string();
    let mut tx = pool
        .begin()
        .await
        .context("begin PostgreSQL daily brief dispatch")?;
    let claimed = sqlx::query_scalar::<_, i64>(
        "UPDATE public.daily_brief_hour_slots
         SET last_dispatch_at = $1, updated_at = $1
         WHERE hour_utc = $2
           AND enabled <> 0
           AND (last_dispatch_at IS NULL OR last_dispatch_at NOT LIKE $3)
         RETURNING hour_utc",
    )
    .bind(now.to_rfc3339())
    .bind(hour_utc)
    .bind(format!("{hour_key}%"))
    .fetch_optional(&mut *tx)
    .await
    .context("claim PostgreSQL daily brief hour slot")?;
    if claimed.is_none() {
        tx.commit()
            .await
            .context("commit unchanged PostgreSQL daily brief dispatch")?;
        return Ok(Vec::new());
    }

    let settings = load_runtime_settings_in(&mut tx).await?;
    let schedule_local_time = settings.daily_brief_local_time(defaults.local_time)?;
    let users = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT id, daily_brief_time_zone
         FROM public.users
         WHERE is_disabled = 0 AND paused_at IS NULL
         ORDER BY id",
    )
    .fetch_all(&mut *tx)
    .await
    .context("load PostgreSQL users for the daily brief slot")?;

    let mut task_ids = Vec::new();
    let mut first_error = None;
    for (user_id, user_time_zone) in users {
        let preferences = briefs::DailyBriefPreferences {
            local_time: schedule_local_time,
            time_zone: briefs::resolve_daily_brief_time_zone(
                defaults.time_zone.as_str(),
                user_time_zone.as_deref(),
            ),
        };
        let scheduled_local = match scheduled_local_datetime(&preferences, now) {
            Ok(value) => value,
            Err(error) => {
                first_error.get_or_insert(error);
                continue;
            }
        };
        if scheduled_local
            .with_timezone(&Utc)
            .format("%Y-%m-%dT%H")
            .to_string()
            != hour_key
        {
            continue;
        }
        let key_date = scheduled_local.date_naive() - ChronoDuration::days(1);
        let window = match briefs::compute_daily_window_for_key_date(&preferences, key_date) {
            Ok(value) => value,
            Err(error) => {
                first_error.get_or_insert(error);
                continue;
            }
        };
        let local_boundary = briefs::format_daily_brief_local_time(preferences.local_time);
        let payload = json!({
            "user_id": user_id,
            "hour_utc": hour_utc,
            "hour_key": hour_key.as_str(),
            "schedule_key": hour_key.as_str(),
            "trigger": "schedule",
            "key_date": window.key_date.to_string(),
            "local_boundary": local_boundary.as_str(),
            "scheduled_local_time": local_boundary.as_str(),
            "effective_local_boundary": window.effective_local_boundary.as_str(),
            "time_zone": window.effective_time_zone.as_str(),
            "window_start_utc": window.start_utc.to_rfc3339(),
            "window_end_utc": window.end_utc.to_rfc3339(),
        });
        match insert_single_flight(
            &mut tx,
            ScheduledTask {
                task_type: jobs::TASK_BRIEF_DAILY_SLOT,
                source: "scheduler",
                requested_by: None,
                payload,
                scope: Some(("user_id", user_id.as_str())),
            },
            now,
        )
        .await
        {
            Ok(Some(task_id)) => task_ids.push(task_id),
            Ok(None) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }

    if let Some(error) = first_error {
        tx.rollback()
            .await
            .context("roll back PostgreSQL daily brief dispatch")?;
        return Err(error);
    }
    tx.commit()
        .await
        .context("commit PostgreSQL daily brief dispatch")?;
    Ok(task_ids)
}

fn scheduled_local_datetime(
    preferences: &briefs::DailyBriefPreferences,
    now: DateTime<Utc>,
) -> Result<DateTime<FixedOffset>> {
    let reference_local = briefs::convert_daily_brief_utc_to_local(&preferences.time_zone, now)
        .with_context(|| format!("convert daily brief reference in {}", preferences.time_zone))?;
    briefs::resolve_daily_brief_local_datetime(
        &preferences.time_zone,
        NaiveDateTime::new(reference_local.date_naive(), preferences.local_time),
    )
    .with_context(|| {
        format!(
            "resolve daily brief local time in {}",
            preferences.time_zone
        )
    })
}

// ---------------------------------------------------------------------------
// subscription runs
// ---------------------------------------------------------------------------

/// Enqueue the release and notification syncs the legacy subscription run
/// covered for every active user, once per configured interval bucket.
pub(crate) async fn enqueue_subscription_runs_if_due(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    let settings = load_runtime_settings(pool).await?;
    let interval_minutes = settings.sync_interval_minutes();
    if let Some(effective_at) = settings.sync_auto_fetch_effective_at.as_deref()
        && DateTime::parse_from_rfc3339(effective_at)
            .map(|value| value.with_timezone(&Utc) > now)
            .unwrap_or(false)
    {
        return Ok(Vec::new());
    }
    let schedule_key = jobs::current_subscription_schedule_key(now, interval_minutes);
    let mut tx = pool
        .begin()
        .await
        .context("begin PostgreSQL subscription dispatch")?;
    if !claim_dispatch_bucket(
        &mut tx,
        SUBSCRIPTION_SCHEDULE_NAME,
        schedule_key.as_str(),
        now,
    )
    .await?
    {
        tx.commit()
            .await
            .context("commit unchanged PostgreSQL subscription dispatch")?;
        return Ok(Vec::new());
    }

    let users = eligible_user_ids(&mut tx).await?;
    let mut task_ids = Vec::new();
    let mut first_error = None;
    for user_id in users {
        let payload = json!({
            "user_id": user_id.as_str(),
            "trigger": "schedule",
            "schedule_key": schedule_key.as_str(),
            "interval_minutes": interval_minutes,
        });
        for task_type in SUBSCRIPTION_TASK_TYPES {
            match insert_single_flight(
                &mut tx,
                ScheduledTask {
                    task_type,
                    source: "scheduler",
                    requested_by: None,
                    payload: payload.clone(),
                    scope: Some(("user_id", user_id.as_str())),
                },
                now,
            )
            .await
            {
                Ok(Some(task_id)) => task_ids.push(task_id),
                Ok(None) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
    }

    if let Some(error) = first_error {
        tx.rollback()
            .await
            .context("roll back PostgreSQL subscription dispatch")?;
        return Err(error);
    }
    record_dispatch_task_id(
        &mut tx,
        SUBSCRIPTION_SCHEDULE_NAME,
        task_ids.first().map(String::as_str),
    )
    .await?;
    tx.commit()
        .await
        .context("commit PostgreSQL subscription dispatch")?;
    Ok(task_ids)
}

// ---------------------------------------------------------------------------
// star sync
// ---------------------------------------------------------------------------

/// Enqueue one `sync.starred` snapshot per connected active user, once per star
/// delta interval bucket.
///
/// The legacy runtime split delta and full-sweep reconciliation per GitHub
/// connection. The PostgreSQL executor always captures a complete snapshot for
/// the user, so this lane keeps the delta bucket as its cadence and coalesces on
/// the user scope; `star_sync_full_sweep_interval_minutes` therefore no longer
/// changes the cadence.
pub(crate) async fn enqueue_star_sync_runs_if_due(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    let settings = load_runtime_settings(pool).await?;
    let interval_minutes = settings.star_sync_delta_interval_minutes();
    let schedule_key = jobs::current_star_sync_delta_schedule_key(now, interval_minutes);
    let users = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT gc.user_id
         FROM public.github_connections gc
         JOIN public.users u ON u.id = gc.user_id
         WHERE u.is_disabled = 0
           AND u.paused_at IS NULL
         ORDER BY gc.user_id",
    )
    .fetch_all(pool)
    .await
    .context("load PostgreSQL GitHub connections for the star sync lane")?;

    let mut task_ids = Vec::new();
    let mut first_error = None;
    for user_id in users {
        let schedule_name = format!("{STAR_SYNC_DELTA_SCHEDULE_NAME_PREFIX}:{user_id}");
        let payload = json!({
            "user_id": user_id.as_str(),
            "trigger": "schedule",
            "schedule_key": schedule_key.as_str(),
            "interval_minutes": interval_minutes,
        });
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(error) => {
                first_error.get_or_insert(
                    anyhow::Error::new(error)
                        .context(format!("begin PostgreSQL star sync dispatch for {user_id}")),
                );
                continue;
            }
        };
        match claim_dispatch_bucket(&mut tx, schedule_name.as_str(), schedule_key.as_str(), now)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                if let Err(error) = tx.commit().await {
                    first_error.get_or_insert(anyhow::Error::new(error).context(format!(
                        "commit unchanged PostgreSQL star sync dispatch for {user_id}"
                    )));
                }
                continue;
            }
            Err(error) => {
                first_error.get_or_insert(error);
                continue;
            }
        }
        let task_id = match insert_single_flight(
            &mut tx,
            ScheduledTask {
                task_type: jobs::TASK_SYNC_STARRED,
                source: "scheduler",
                requested_by: None,
                payload,
                scope: Some(("user_id", user_id.as_str())),
            },
            now,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                first_error.get_or_insert(error);
                continue;
            }
        };
        if let Err(error) =
            record_dispatch_task_id(&mut tx, schedule_name.as_str(), task_id.as_deref()).await
        {
            first_error.get_or_insert(error);
            continue;
        }
        if let Err(error) = tx.commit().await {
            first_error.get_or_insert(anyhow::Error::new(error).context(format!(
                "commit PostgreSQL star sync dispatch for {user_id}"
            )));
            continue;
        }
        if let Some(task_id) = task_id {
            task_ids.push(task_id);
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(task_ids),
    }
}

/// Enqueue `sync.starred` for users whose reconciliation epoch has a slice due
/// right now.
///
/// [`enqueue_star_sync_runs_if_due`] only opens a new delta bucket every
/// `star_sync_delta_interval_minutes`; a slice scheduled in between would idle
/// until that bucket turns over. This probe closes the gap: it asks
/// [`postgres_star_epochs::load_due_epoch_user_ids`] which epochs are claimable
/// and pushes a task through the same `user_id` single-flight scope, so an
/// already queued or running snapshot absorbs the probe and nothing duplicates.
pub(crate) async fn enqueue_due_star_epoch_slices(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    let users = postgres_star_epochs::load_due_epoch_user_ids(pool, now).await?;
    let mut task_ids = Vec::new();
    let mut first_error = None;
    for user_id in users {
        let payload = json!({
            "user_id": user_id.as_str(),
            "trigger": "epoch_due",
        });
        match insert_single_flight_pool(
            pool,
            ScheduledTask {
                task_type: jobs::TASK_SYNC_STARRED,
                source: "scheduler",
                requested_by: None,
                payload,
                scope: Some(("user_id", user_id.as_str())),
            },
            now,
        )
        .await
        {
            Ok(Some(task_id)) => task_ids.push(task_id),
            Ok(None) => {}
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(task_ids),
    }
}

// ---------------------------------------------------------------------------
// recent failure retry
// ---------------------------------------------------------------------------

/// Re-enqueue `brief.generate` for briefs whose content refresh failed inside
/// the retry window, once per configured retry interval bucket.
///
/// The legacy lane also covered `polish` and `translation` kinds, but had already
/// delegated both to the structured translation recovery worker. That worker has
/// no PostgreSQL executor yet, so this lane stays scoped to daily briefs.
pub(crate) async fn enqueue_recent_failures_retry_if_due(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    let settings = load_runtime_settings(pool).await?;
    let interval_minutes = settings.recent_failures_interval_minutes();
    let schedule_key = jobs::current_recent_failures_retry_schedule_key(now, interval_minutes);
    let mut tx = pool
        .begin()
        .await
        .context("begin PostgreSQL recent failure retry dispatch")?;
    if !claim_dispatch_bucket(
        &mut tx,
        RETRY_RECENT_FAILURES_SCHEDULE_NAME,
        schedule_key.as_str(),
        now,
    )
    .await?
    {
        tx.commit()
            .await
            .context("commit unchanged PostgreSQL recent failure retry dispatch")?;
        return Ok(Vec::new());
    }

    let candidates = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT b.id, b.user_id, b.date
        FROM public.briefs b
        JOIN public.users u ON u.id = b.user_id
        WHERE u.is_disabled = 0
          AND u.paused_at IS NULL
          AND b.generation_source = 'content_refresh_failed'
          AND b.window_start_utc IS NOT NULL
          AND b.window_end_utc IS NOT NULL
          AND b.effective_time_zone IS NOT NULL
          AND b.effective_local_boundary IS NOT NULL
          AND b.updated_at >= $1
        ORDER BY b.updated_at DESC, b.created_at DESC, b.id DESC
        LIMIT $2
        "#,
    )
    .bind(now - ChronoDuration::hours(RETRY_LOOKBACK_HOURS))
    .bind(RETRY_MAX_BRIEFS)
    .fetch_all(&mut *tx)
    .await
    .context("load PostgreSQL recent failed briefs for the retry lane")?;

    let mut task_ids = Vec::new();
    let mut first_error = None;
    for (brief_id, user_id, date) in candidates {
        let payload = json!({
            "user_id": user_id,
            "brief_id": brief_id,
            "key_date": date,
            "trigger": "schedule.retry_recent",
            "schedule_key": schedule_key.as_str(),
            "interval_minutes": interval_minutes,
            "lookback_hours": RETRY_LOOKBACK_HOURS,
        });
        match insert_single_flight(
            &mut tx,
            ScheduledTask {
                task_type: jobs::TASK_BRIEF_GENERATE,
                source: "scheduler.retry_recent_failures",
                requested_by: Some(user_id.as_str()),
                payload,
                scope: Some(("user_id", user_id.as_str())),
            },
            now,
        )
        .await
        {
            Ok(Some(task_id)) => task_ids.push(task_id),
            Ok(None) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }

    if let Some(error) = first_error {
        tx.rollback()
            .await
            .context("roll back PostgreSQL recent failure retry dispatch")?;
        return Err(error);
    }
    record_dispatch_task_id(
        &mut tx,
        RETRY_RECENT_FAILURES_SCHEDULE_NAME,
        task_ids.first().map(String::as_str),
    )
    .await?;
    tx.commit()
        .await
        .context("commit PostgreSQL recent failure retry dispatch")?;
    Ok(task_ids)
}

// ---------------------------------------------------------------------------
// webhook push reconcile demands and audit
// ---------------------------------------------------------------------------

/// Dispatch pending webhook reconcile demands and the periodic audit.
pub(crate) async fn dispatch_webhook_push_if_due(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    let mut task_ids = Vec::new();
    let mut first_error = None;

    let demanded_users = sqlx::query_scalar::<_, String>(
        "SELECT user_id
         FROM public.webhook_push_reconcile_demands
         WHERE requested_generation > completed_generation
         ORDER BY requested_at, user_id",
    )
    .fetch_all(pool)
    .await
    .context("list PostgreSQL webhook reconcile demands")?;
    for user_id in demanded_users {
        match dispatch_reconcile_demand(pool, user_id.as_str(), now).await {
            Ok(Some(task_id)) => task_ids.push(task_id),
            Ok(None) => {}
            Err(error) => {
                warn!(
                    lane = LANE_WEBHOOK_PUSH,
                    user_id = %user_id,
                    ?error,
                    "PostgreSQL webhook reconcile demand dispatch failed"
                );
                first_error.get_or_insert(error);
            }
        }
    }

    match enqueue_webhook_audit_if_due(pool, now).await {
        Ok(audit_task_ids) => task_ids.extend(audit_task_ids),
        Err(error) => {
            first_error.get_or_insert(error);
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(task_ids),
    }
}

async fn dispatch_reconcile_demand(
    pool: &PgPool,
    user_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let demand = sqlx::query_as::<_, (i64, i64)>(
        "SELECT requested_generation, completed_generation
         FROM public.webhook_push_reconcile_demands
         WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .context("load PostgreSQL webhook reconcile demand")?;
    let Some((requested_generation, completed_generation)) = demand else {
        return Ok(None);
    };
    if requested_generation <= completed_generation {
        return Ok(None);
    }
    let generation = requested_generation.to_string();
    let terminal_failures = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM public.job_tasks
        WHERE task_type = $1
          AND requested_by = $2
          AND status = 'failed'
          AND CASE
                WHEN pg_input_is_valid(payload_json, 'jsonb')
                THEN payload_json::jsonb ->> 'demand_generation'
              END = $3
        "#,
    )
    .bind(jobs::TASK_WEBHOOK_PUSH_MANAGE)
    .bind(user_id)
    .bind(generation.as_str())
    .fetch_one(pool)
    .await
    .context("check terminal PostgreSQL webhook reconcile failure")?;
    if terminal_failures != 0 {
        return Ok(None);
    }
    let eligible = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM public.users
         WHERE id = $1
           AND is_disabled = 0
           AND paused_at IS NULL
           AND include_own_releases <> 0
           AND webhook_push_enabled <> 0
           AND webhook_push_desired_state = 'enabled'",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .context("check PostgreSQL webhook reconcile demand eligibility")?;
    if eligible == 0 {
        return Ok(None);
    }

    insert_single_flight_pool(
        pool,
        ScheduledTask {
            task_type: jobs::TASK_WEBHOOK_PUSH_MANAGE,
            source: "webhook_reconcile_demand",
            requested_by: Some(user_id),
            payload: json!({
                "user_id": user_id,
                "operation": "reconcile",
                "repo_id": Value::Null,
                "demand_generation": requested_generation,
                "retry_repo_ids": [],
                "scheduled": false,
                "retry_count": 0,
                "trigger": "reconcile_demand",
            }),
            scope: Some(("user_id", user_id)),
        },
        now,
    )
    .await
}

async fn enqueue_webhook_audit_if_due(pool: &PgPool, now: DateTime<Utc>) -> Result<Vec<String>> {
    let mut tx = pool
        .begin()
        .await
        .context("begin PostgreSQL webhook audit dispatch")?;
    let settings = load_runtime_settings_in(&mut tx).await?;
    let interval_days = settings
        .webhook_push_audit_interval_days
        .unwrap_or(WEBHOOK_AUDIT_DEFAULT_INTERVAL_DAYS)
        .clamp(
            WEBHOOK_AUDIT_MIN_INTERVAL_DAYS,
            WEBHOOK_AUDIT_MAX_INTERVAL_DAYS,
        );
    let claimed = sqlx::query_scalar::<_, i64>(
        r#"
        UPDATE public.admin_runtime_settings
        SET webhook_push_audit_last_started_at = $1, updated_at = $1
        WHERE id = 1
          AND (
            webhook_push_audit_last_started_at IS NULL
            OR CASE
                 WHEN pg_input_is_valid(
                   webhook_push_audit_last_started_at,
                   'timestamptz'
                 )
                 THEN webhook_push_audit_last_started_at::timestamptz <= $2::timestamptz
                 ELSE TRUE
               END
          )
        RETURNING id
        "#,
    )
    .bind(now.to_rfc3339())
    .bind((now - ChronoDuration::days(interval_days)).to_rfc3339())
    .fetch_optional(&mut *tx)
    .await
    .context("claim PostgreSQL webhook audit interval")?;
    if claimed.is_none() {
        tx.commit()
            .await
            .context("commit unchanged PostgreSQL webhook audit dispatch")?;
        return Ok(Vec::new());
    }

    let users = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM public.users
        WHERE is_disabled = 0
          AND paused_at IS NULL
          AND (
            (include_own_releases <> 0 AND webhook_push_desired_state = 'enabled')
            OR (
              webhook_push_desired_state IN ('paused', 'deleted')
              AND EXISTS (
                SELECT 1
                FROM public.webhook_push_repos wr
                WHERE wr.user_id = public.users.id
                  AND wr.hook_id IS NOT NULL
              )
            )
          )
        ORDER BY id
        "#,
    )
    .fetch_all(&mut *tx)
    .await
    .context("list PostgreSQL webhook audit users")?;

    let mut task_ids = Vec::new();
    let mut first_error = None;
    for user_id in users {
        match insert_single_flight(
            &mut tx,
            ScheduledTask {
                task_type: jobs::TASK_WEBHOOK_PUSH_MANAGE,
                source: "webhook-audit",
                requested_by: Some(user_id.as_str()),
                payload: json!({
                    "user_id": user_id,
                    "operation": "reconcile",
                    "repo_id": Value::Null,
                    "scheduled": true,
                    "trigger": "schedule",
                    "reason": "audit",
                }),
                scope: Some(("user_id", user_id.as_str())),
            },
            now,
        )
        .await
        {
            Ok(Some(task_id)) => task_ids.push(task_id),
            Ok(None) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }

    if let Some(error) = first_error {
        tx.rollback()
            .await
            .context("roll back PostgreSQL webhook audit dispatch")?;
        return Err(error);
    }
    tx.commit()
        .await
        .context("commit PostgreSQL webhook audit dispatch")?;
    Ok(task_ids)
}

// ---------------------------------------------------------------------------
// account pause maintenance
// ---------------------------------------------------------------------------

/// Pause accounts that have been inactive for the configured window, at most
/// once per local day.
pub(crate) async fn run_account_pause_maintenance_if_due(
    pool: &PgPool,
    defaults: &SchedulerDefaults,
    now: DateTime<Utc>,
) -> Result<Option<i64>> {
    let time_zone = Tz::from_str(defaults.time_zone.as_str())
        .with_context(|| format!("invalid app default time zone {}", defaults.time_zone))?;
    let local_now = now.with_timezone(&time_zone);
    if local_now.time() < ACCOUNT_PAUSE_LOCAL_TIME {
        return Ok(None);
    }
    let schedule_key = local_now.date_naive().format("%Y-%m-%d").to_string();
    let paused_at = now.to_rfc3339();
    let pause_before = (now - ChronoDuration::days(ACCOUNT_PAUSE_INACTIVITY_DAYS)).to_rfc3339();

    let mut tx = pool
        .begin()
        .await
        .context("begin PostgreSQL account pause maintenance")?;
    let advanced = sqlx::query_scalar::<_, String>(
        r#"
        INSERT INTO public.scheduled_task_dispatch_state (
          schedule_name, last_dispatch_key, last_task_id, updated_at
        )
        VALUES ($1, $2, NULL, $3)
        ON CONFLICT (schedule_name) DO UPDATE SET
          last_dispatch_key = EXCLUDED.last_dispatch_key,
          last_task_id = NULL,
          updated_at = EXCLUDED.updated_at
        WHERE public.scheduled_task_dispatch_state.last_dispatch_key
              IS DISTINCT FROM EXCLUDED.last_dispatch_key
        RETURNING schedule_name
        "#,
    )
    .bind(ACCOUNT_PAUSE_SCHEDULE_NAME)
    .bind(schedule_key.as_str())
    .bind(paused_at.as_str())
    .fetch_optional(&mut *tx)
    .await
    .context("record PostgreSQL account pause dispatch state")?;
    if advanced.is_none() {
        tx.commit()
            .await
            .context("commit unchanged PostgreSQL account pause maintenance")?;
        return Ok(None);
    }

    let paused = sqlx::query(
        r#"
        UPDATE public.users
        SET paused_at = $1, updated_at = $1
        WHERE is_disabled = 0
          AND paused_at IS NULL
          AND CASE
                WHEN pg_input_is_valid(
                  COALESCE(last_active_at, created_at),
                  'timestamptz'
                )
                THEN COALESCE(last_active_at, created_at)::timestamptz <= $2::timestamptz
                ELSE FALSE
              END
        "#,
    )
    .bind(paused_at.as_str())
    .bind(pause_before.as_str())
    .execute(&mut *tx)
    .await
    .context("pause inactive PostgreSQL accounts")?;
    tx.commit()
        .await
        .context("commit PostgreSQL account pause maintenance")?;
    Ok(Some(paused.rows_affected() as i64))
}

// ---------------------------------------------------------------------------
// admin dashboard rollups
// ---------------------------------------------------------------------------

/// Refresh `public.admin_dashboard_daily_rollups` for the trailing window in the
/// system time zone.
pub(crate) async fn refresh_admin_dashboard_rollups(
    pool: &PgPool,
    defaults: &SchedulerDefaults,
    days: i64,
    now: DateTime<Utc>,
) -> Result<usize> {
    let time_zone = Tz::from_str(defaults.time_zone.as_str()).unwrap_or(chrono_tz::UTC);
    let end_day = now.with_timezone(&time_zone).date_naive();
    let start_day = end_day
        .checked_sub_signed(ChronoDuration::days(days.max(1) - 1))
        .ok_or_else(|| anyhow!("invalid admin dashboard rollup range"))?;

    let mut refreshed = 0usize;
    let mut day = start_day;
    loop {
        upsert_admin_dashboard_rollup_for_day(pool, time_zone, day, now).await?;
        refreshed += 1;
        if day >= end_day {
            break;
        }
        day = day
            .succ_opt()
            .ok_or_else(|| anyhow!("invalid admin dashboard rollup day iteration"))?;
    }
    Ok(refreshed)
}

async fn upsert_admin_dashboard_rollup_for_day(
    pool: &PgPool,
    time_zone: Tz,
    day: chrono::NaiveDate,
    now: DateTime<Utc>,
) -> Result<()> {
    let (start_utc, end_utc) = local_day_bounds_utc(time_zone, day)?;
    let total_users = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM public.users
         WHERE CASE
                 WHEN pg_input_is_valid(created_at, 'timestamptz')
                 THEN created_at::timestamptz < $1
                 ELSE FALSE
               END",
    )
    .bind(end_utc)
    .fetch_one(pool)
    .await
    .context("count PostgreSQL users for the admin dashboard rollup")?;
    let active_users = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM public.users
         WHERE last_active_at IS NOT NULL
           AND CASE
                 WHEN pg_input_is_valid(last_active_at, 'timestamptz')
                 THEN last_active_at::timestamptz BETWEEN $1 AND $2
                 ELSE FALSE
               END",
    )
    .bind(start_utc)
    .bind(end_utc)
    .fetch_one(pool)
    .await
    .context("count active PostgreSQL users for the admin dashboard rollup")?;

    let rollup_date = day.format("%Y-%m-%d").to_string();
    let time_zone_name = time_zone.name().to_owned();
    let updated_at = now.to_rfc3339();
    for task_type in ADMIN_DASHBOARD_ROLLUP_TASK_TYPES {
        let counts = load_admin_dashboard_task_counts(pool, task_type, start_utc, end_utc).await?;
        sqlx::query(
            r#"
            INSERT INTO public.admin_dashboard_daily_rollups (
              rollup_date, time_zone, task_type, total_users, active_users,
              queued_count, running_count, succeeded_count, failed_count,
              canceled_count, business_ok_count, business_partial_count,
              business_failed_count, business_disabled_count, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            ON CONFLICT (rollup_date, time_zone, task_type) DO UPDATE SET
              total_users = EXCLUDED.total_users,
              active_users = EXCLUDED.active_users,
              queued_count = EXCLUDED.queued_count,
              running_count = EXCLUDED.running_count,
              succeeded_count = EXCLUDED.succeeded_count,
              failed_count = EXCLUDED.failed_count,
              canceled_count = EXCLUDED.canceled_count,
              business_ok_count = EXCLUDED.business_ok_count,
              business_partial_count = EXCLUDED.business_partial_count,
              business_failed_count = EXCLUDED.business_failed_count,
              business_disabled_count = EXCLUDED.business_disabled_count,
              updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(rollup_date.as_str())
        .bind(time_zone_name.as_str())
        .bind(task_type)
        .bind(total_users)
        .bind(active_users)
        .bind(counts.queued)
        .bind(counts.running)
        .bind(counts.succeeded)
        .bind(counts.failed)
        .bind(counts.canceled)
        .bind(counts.business_ok)
        .bind(counts.business_partial)
        .bind(counts.business_failed)
        .bind(counts.business_disabled)
        .bind(updated_at.as_str())
        .execute(pool)
        .await
        .context("upsert PostgreSQL admin dashboard daily rollup")?;
    }
    Ok(())
}

fn local_day_start_utc(time_zone: Tz, day: chrono::NaiveDate) -> Result<DateTime<Utc>> {
    time_zone
        .with_ymd_and_hms(day.year(), day.month(), day.day(), 0, 0, 0)
        .single()
        .or_else(|| {
            time_zone
                .with_ymd_and_hms(day.year(), day.month(), day.day(), 0, 0, 0)
                .earliest()
        })
        .map(|value| value.with_timezone(&Utc))
        .ok_or_else(|| anyhow!("invalid local day boundary"))
}

fn local_day_bounds_utc(
    time_zone: Tz,
    day: chrono::NaiveDate,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let next_day = day
        .succ_opt()
        .ok_or_else(|| anyhow!("invalid next local day"))?;
    Ok((
        local_day_start_utc(time_zone, day)?,
        local_day_start_utc(time_zone, next_day)?,
    ))
}

#[derive(Default)]
struct AdminDashboardTaskCounts {
    queued: i64,
    running: i64,
    succeeded: i64,
    failed: i64,
    canceled: i64,
    business_ok: i64,
    business_partial: i64,
    business_failed: i64,
    business_disabled: i64,
}

async fn load_admin_dashboard_task_counts(
    pool: &PgPool,
    task_type: &str,
    start_utc: DateTime<Utc>,
    end_utc: DateTime<Utc>,
) -> Result<AdminDashboardTaskCounts> {
    let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
        "SELECT status, payload_json, result_json
         FROM public.job_tasks
         WHERE task_type = $1
           AND created_at >= $2
           AND created_at <= $3",
    )
    .bind(task_type)
    .bind(start_utc)
    .bind(end_utc)
    .fetch_all(pool)
    .await
    .with_context(|| format!("load PostgreSQL {task_type} rows for the admin dashboard rollup"))?;

    let mut counts = AdminDashboardTaskCounts::default();
    for (status, payload_json, result_json) in rows {
        match status.as_str() {
            jobs::STATUS_QUEUED => counts.queued += 1,
            jobs::STATUS_RUNNING => counts.running += 1,
            jobs::STATUS_SUCCEEDED => counts.succeeded += 1,
            jobs::STATUS_FAILED => counts.failed += 1,
            jobs::STATUS_CANCELED => counts.canceled += 1,
            _ => {}
        }
        let payload = parse_json(payload_json.as_deref());
        let result = parse_json(result_json.as_deref());
        let code = if task_type == jobs::TASK_BRIEF_DAILY_SLOT {
            brief_business_code(status.as_str(), result.as_ref())
        } else {
            release_batch_business_code(status.as_str(), payload.as_ref(), result.as_ref())
        };
        match code {
            "ok" => counts.business_ok += 1,
            "partial" => counts.business_partial += 1,
            "failed" => counts.business_failed += 1,
            "disabled" => counts.business_disabled += 1,
            _ => {}
        }
    }
    Ok(counts)
}

fn parse_json(raw: Option<&str>) -> Option<Value> {
    raw.and_then(|value| serde_json::from_str::<Value>(value).ok())
}

fn json_i64(value: Option<&Value>, key: &str) -> i64 {
    value
        .and_then(|value| value.get(key))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn release_batch_business_code(
    status: &str,
    payload: Option<&Value>,
    result: Option<&Value>,
) -> &'static str {
    let payload_total = payload
        .and_then(|value| value.get("release_ids"))
        .and_then(Value::as_array)
        .map(|items| i64::try_from(items.len()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let mut total = json_i64(result, "total");
    let mut ready = json_i64(result, "ready");
    let mut missing = json_i64(result, "missing");
    let mut disabled = json_i64(result, "disabled");
    let mut error = json_i64(result, "error");
    let mut items_total = 0_i64;
    if let Some(items) = result
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
    {
        ready = 0;
        missing = 0;
        disabled = 0;
        error = 0;
        items_total = i64::try_from(items.len()).unwrap_or(i64::MAX);
        for item in items {
            match item.get("status").and_then(Value::as_str).unwrap_or("") {
                "ready" => ready += 1,
                "missing" => missing += 1,
                "disabled" => disabled += 1,
                "error" => error += 1,
                _ => {}
            }
        }
    }
    total = total
        .max(payload_total)
        .max(items_total)
        .max(ready + missing + disabled + error);

    if matches!(
        status,
        jobs::STATUS_CANCELED | jobs::STATUS_QUEUED | jobs::STATUS_RUNNING
    ) {
        "unknown"
    } else if status == jobs::STATUS_FAILED {
        "failed"
    } else if total <= 0 {
        "ok"
    } else if disabled == total {
        "disabled"
    } else if error > 0 && ready == 0 {
        "failed"
    } else if error > 0 || missing > 0 {
        "partial"
    } else if ready > 0 && ready >= total {
        "ok"
    } else if ready > 0 {
        "partial"
    } else {
        "unknown"
    }
}

fn brief_business_code(status: &str, result: Option<&Value>) -> &'static str {
    match status {
        jobs::STATUS_CANCELED | jobs::STATUS_QUEUED | jobs::STATUS_RUNNING => "unknown",
        jobs::STATUS_FAILED => "failed",
        _ => {
            let total = json_i64(result, "total");
            let succeeded = json_i64(result, "succeeded");
            let failed = json_i64(result, "failed");
            let canceled = json_i64(result, "canceled");
            if canceled != 0 {
                "partial"
            } else if total == 0 {
                "ok"
            } else if failed > 0 && succeeded == 0 {
                "failed"
            } else if failed > 0 {
                "partial"
            } else if succeeded > 0 {
                "ok"
            } else {
                "unknown"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

struct ScheduledTask<'a> {
    task_type: &'a str,
    source: &'a str,
    requested_by: Option<&'a str>,
    payload: Value,
    /// Payload field that scopes single-flight dedupe, e.g. `("user_id", user)`.
    scope: Option<(&'a str, &'a str)>,
}

/// Claim this bucket for the current dispatch, inside the caller's transaction.
///
/// Returns `false` when another tick already recorded the bucket key, so the
/// caller can commit without enqueueing anything.
async fn claim_dispatch_bucket(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    schedule_name: &str,
    schedule_key: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let claimed = sqlx::query_scalar::<_, String>(
        r#"
        INSERT INTO public.scheduled_task_dispatch_state (
          schedule_name, last_dispatch_key, last_task_id, updated_at
        )
        VALUES ($1, $2, NULL, $3)
        ON CONFLICT (schedule_name) DO UPDATE SET
          last_dispatch_key = EXCLUDED.last_dispatch_key,
          last_task_id = NULL,
          updated_at = EXCLUDED.updated_at
        WHERE public.scheduled_task_dispatch_state.last_dispatch_key
              IS DISTINCT FROM EXCLUDED.last_dispatch_key
        RETURNING schedule_name
        "#,
    )
    .bind(schedule_name)
    .bind(schedule_key)
    .bind(now.to_rfc3339())
    .fetch_optional(&mut **tx)
    .await
    .with_context(|| format!("claim PostgreSQL scheduler bucket {schedule_name}"))?;
    Ok(claimed.is_some())
}

async fn record_dispatch_task_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    schedule_name: &str,
    task_id: Option<&str>,
) -> Result<()> {
    let Some(task_id) = task_id else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE public.scheduled_task_dispatch_state
         SET last_task_id = $1
         WHERE schedule_name = $2",
    )
    .bind(task_id)
    .bind(schedule_name)
    .execute(&mut **tx)
    .await
    .context("record PostgreSQL scheduler dispatch task id")?;
    Ok(())
}

/// Insert one queued task unless an equivalent queued/running task exists.
///
/// The dedupe guard lives inside the insert statement so two lanes (or two
/// runtime processes) can never both create the same scope.
async fn insert_single_flight(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task: ScheduledTask<'_>,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let task_id = local_id::generate_local_id();
    let payload_json = task.payload.to_string();
    // `scope` is (payload field, expected value); the guard compares
    // `payload->>field = value`, so the value binds first.
    let (scope_field, scope_value) = match task.scope {
        Some((field, value)) => (Some(field), Some(value)),
        None => (None, None),
    };
    let inserted = sqlx::query_scalar::<_, String>(
        r#"
        INSERT INTO public.job_tasks (
          id, task_type, status, source, requested_by, payload_json, created_at, updated_at
        )
        SELECT $1, $2, 'queued', $3, $4, $5, $6, $6
        WHERE NOT EXISTS (
          SELECT 1
          FROM public.job_tasks pending
          WHERE pending.task_type = $2
            AND pending.status IN ('queued', 'running')
            AND (
              $7::text IS NULL
              OR CASE
                   WHEN pg_input_is_valid(pending.payload_json, 'jsonb')
                   THEN pending.payload_json::jsonb ->> $8::text
                 END = $7::text
            )
        )
        -- Partial unique indexes (for example idx_webhook_push_manage_inflight)
        -- also enforce single-flight; the NOT EXISTS guard cannot see an index
        -- predicate, so let the index win instead of aborting the lane.
        ON CONFLICT DO NOTHING
        RETURNING id
        "#,
    )
    .bind(task_id.as_str())
    .bind(task.task_type)
    .bind(task.source)
    .bind(task.requested_by)
    .bind(payload_json.as_str())
    .bind(now)
    .bind(scope_value)
    .bind(scope_field)
    .fetch_optional(&mut **tx)
    .await
    .with_context(|| format!("enqueue {} from {}", task.task_type, task.source))?;
    let Some(inserted) = inserted else {
        return Ok(None);
    };
    sqlx::query(
        "INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at)
         VALUES ($1, $2, 'task.created', $3, $4)",
    )
    .bind(local_id::generate_local_id())
    .bind(inserted.as_str())
    .bind(
        json!({
            "task_id": inserted.as_str(),
            "task_type": task.task_type,
            "status": jobs::STATUS_QUEUED,
            "source": task.source,
        })
        .to_string(),
    )
    .bind(now)
    .execute(&mut **tx)
    .await
    .context("record PostgreSQL scheduled task creation")?;
    Ok(Some(inserted))
}

async fn insert_single_flight_pool(
    pool: &PgPool,
    task: ScheduledTask<'_>,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let mut tx = pool
        .begin()
        .await
        .context("begin PostgreSQL scheduler enqueue")?;
    let inserted = insert_single_flight(&mut tx, task, now).await?;
    tx.commit()
        .await
        .context("commit PostgreSQL scheduler enqueue")?;
    Ok(inserted)
}

async fn eligible_user_ids(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.users WHERE is_disabled = 0 AND paused_at IS NULL ORDER BY id",
    )
    .fetch_all(&mut **tx)
    .await
    .context("load eligible PostgreSQL users for the scheduler")
}

#[derive(Debug, Default, sqlx::FromRow)]
struct RuntimeSettings {
    sync_auto_fetch_interval_minutes: Option<i64>,
    sync_auto_fetch_effective_at: Option<String>,
    star_sync_delta_interval_minutes: Option<i64>,
    retry_recent_failures_interval_minutes: Option<i64>,
    daily_brief_schedule_local_time: Option<String>,
    webhook_push_audit_interval_days: Option<i64>,
}

impl RuntimeSettings {
    fn sync_interval_minutes(&self) -> i64 {
        admin_runtime::normalize_sync_auto_fetch_interval_minutes(
            self.sync_auto_fetch_interval_minutes
                .unwrap_or(admin_runtime::DEFAULT_SYNC_AUTO_FETCH_INTERVAL_MINUTES),
        )
    }

    fn star_sync_delta_interval_minutes(&self) -> i64 {
        admin_runtime::normalize_star_sync_delta_interval_minutes(
            self.star_sync_delta_interval_minutes
                .unwrap_or(admin_runtime::DEFAULT_STAR_SYNC_DELTA_INTERVAL_MINUTES),
        )
    }

    fn recent_failures_interval_minutes(&self) -> i64 {
        admin_runtime::normalize_retry_recent_failures_interval_minutes(
            self.retry_recent_failures_interval_minutes
                .unwrap_or(admin_runtime::DEFAULT_RETRY_RECENT_FAILURES_INTERVAL_MINUTES),
        )
    }

    fn daily_brief_local_time(&self, fallback: NaiveTime) -> Result<NaiveTime> {
        match self.daily_brief_schedule_local_time.as_deref() {
            Some(raw) => briefs::parse_daily_brief_local_time(raw),
            None => Ok(fallback),
        }
    }
}

const RUNTIME_SETTINGS_SELECT: &str = "SELECT sync_auto_fetch_interval_minutes,
        sync_auto_fetch_effective_at,
        star_sync_delta_interval_minutes,
        retry_recent_failures_interval_minutes,
        daily_brief_schedule_local_time,
        webhook_push_audit_interval_days
 FROM public.admin_runtime_settings
 WHERE id = 1";

async fn load_runtime_settings(pool: &PgPool) -> Result<RuntimeSettings> {
    Ok(
        sqlx::query_as::<_, RuntimeSettings>(RUNTIME_SETTINGS_SELECT)
            .fetch_optional(pool)
            .await
            .context("load PostgreSQL scheduler runtime settings")?
            .unwrap_or_default(),
    )
}

async fn load_runtime_settings_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<RuntimeSettings> {
    Ok(
        sqlx::query_as::<_, RuntimeSettings>(RUNTIME_SETTINGS_SELECT)
            .fetch_optional(&mut **tx)
            .await
            .context("load PostgreSQL scheduler runtime settings")?
            .unwrap_or_default(),
    )
}
