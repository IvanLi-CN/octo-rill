//! PostgreSQL-native Star reconciliation epoch lifecycle.
//!
//! One `public.star_sync_epochs` row per GitHub connection carries the scan
//! cursor, the processed counters, the next-slice wake-up time and a short
//! execution lease. Executors advance exactly one page-sized slice per run so a
//! full Star sweep is spread over `star_sync_full_sweep_interval_minutes`
//! instead of one unbounded snapshot.
//!
//! Only a `succeeded` epoch authorizes removing Star memberships that were not
//! observed during the sweep; `finalize_epoch_in_tx` performs that prune inside
//! the caller's transaction so the aggregate `starred_repos` rebuild stays
//! atomic with the terminal status.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::admin_runtime;

/// GitHub page size for one reconciliation slice.
///
/// Mirrors the legacy `STAR_SYNC_FULL_PAGE_SIZE` constant: the slice budget is
/// one page per execution.
pub(crate) const STAR_SYNC_FULL_PAGE_SIZE: usize = 100;

/// Slice lease length in minutes (legacy `STAR_SYNC_EPOCH_LEASE_MINUTES`).
///
/// While the lease is alive a concurrent execution cannot claim the same slice.
pub(crate) const STAR_SYNC_EPOCH_LEASE_MINUTES: i64 = 2;

/// Delay before a failed slice may be retried on the same epoch.
pub(crate) const STAR_SYNC_EPOCH_RETRY_MINUTES: i64 = 15;

/// Legacy floor for spacing two consecutive slices.
const STAR_SYNC_MIN_SLICE_SPACING_SECONDS: i64 = 20;

const EPOCH_COLUMNS: &str = "id, user_id, github_connection_id, started_at, next_cursor, \
     total_count_at_start, processed_pages, processed_items, next_slice_not_before, \
     lease_expires_at";

/// One `public.star_sync_epochs` row as seen by the slice executor.
#[derive(Debug, Clone, FromRow)]
pub(crate) struct StarEpochRow {
    pub id: String,
    pub user_id: String,
    pub github_connection_id: String,
    pub started_at: String,
    pub next_cursor: Option<String>,
    pub total_count_at_start: Option<i64>,
    pub processed_pages: i64,
    pub processed_items: i64,
    pub next_slice_not_before: Option<String>,
    pub lease_expires_at: Option<String>,
}

/// Dispatch facts for a due epoch, mirroring the legacy schedule key shape.
#[derive(Debug, Clone)]
pub(crate) struct StarEpochDispatch {
    pub epoch_id: String,
    pub schedule_key: String,
    /// Earliest slice wake-up time; the executor advances the most overdue
    /// connection first so a busy connection cannot starve its siblings.
    pub due_at: Option<DateTime<Utc>>,
}

/// One fetched Star page, reduced to the facts epoch bookkeeping needs.
#[derive(Debug, Clone)]
pub(crate) struct StarSlicePage {
    pub repo_count: i64,
    pub total_count: i64,
    pub end_cursor: Option<String>,
}

/// Result of advancing (or finishing) one epoch slice.
#[derive(Debug, Clone)]
pub(crate) struct StarEpochProgress {
    pub epoch_id: String,
    pub github_connection_id: String,
    pub processed_pages: i64,
    pub processed_items: i64,
    pub total_count_at_start: i64,
    pub has_next_page: bool,
    pub next_slice_not_before: Option<String>,
    pub status: &'static str,
    pub membership_removed: usize,
}

/// A membership dropped by epoch finalization.
#[derive(Debug, Clone, FromRow)]
pub(crate) struct StaleStarMembership {
    pub repo_id: i64,
}

/// Load the configured full-sweep budget in minutes.
pub(crate) async fn load_full_sweep_interval_minutes(pool: &PgPool) -> Result<i64> {
    let interval = sqlx::query_scalar::<_, i64>(
        "SELECT star_sync_full_sweep_interval_minutes \
         FROM public.admin_runtime_settings WHERE id = 1 LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .context("load PostgreSQL star full sweep interval")?
    .unwrap_or(admin_runtime::DEFAULT_STAR_SYNC_FULL_SWEEP_INTERVAL_MINUTES);
    Ok(admin_runtime::normalize_star_sync_full_sweep_interval_minutes(interval))
}

/// Create a new epoch when the connection has none and its previous sweep is
/// older than the configured full-sweep interval.
///
/// Returns `None` when the connection has an active epoch that is not due yet,
/// or when the last completed sweep is still inside the interval.
pub(crate) async fn ensure_epoch_due(
    pool: &PgPool,
    user_id: &str,
    github_connection_id: &str,
    full_sweep_interval_minutes: i64,
    now: DateTime<Utc>,
) -> Result<Option<StarEpochDispatch>> {
    if let Some(active) = load_active_epoch(pool, github_connection_id).await? {
        let ready = star_sync_timestamp_due(active.next_slice_not_before.as_deref(), now)
            && star_sync_timestamp_due(active.lease_expires_at.as_deref(), now);
        return Ok(ready.then(|| epoch_dispatch(&active)));
    }

    let last_completed_at = sqlx::query_scalar::<_, Option<String>>(
        "SELECT completed_at FROM public.star_sync_epochs \
         WHERE github_connection_id = $1 AND status = 'succeeded' \
         ORDER BY completed_at DESC, id DESC LIMIT 1",
    )
    .bind(github_connection_id)
    .fetch_optional(pool)
    .await
    .context("load latest completed PostgreSQL star epoch")?
    .flatten();
    let interval =
        admin_runtime::normalize_star_sync_full_sweep_interval_minutes(full_sweep_interval_minutes);
    let due_after = now - chrono::Duration::minutes(interval);
    let due = last_completed_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|value| value.with_timezone(&Utc) <= due_after);
    if !due {
        return Ok(None);
    }

    let epoch_id = crate::local_id::generate_local_id();
    let now_rfc3339 = now.to_rfc3339();
    let inserted = sqlx::query(
        "INSERT INTO public.star_sync_epochs (\
           id, user_id, github_connection_id, status, started_at, next_cursor, \
           total_count_at_start, processed_pages, processed_items, next_slice_not_before, \
           lease_owner_id, lease_expires_at, completed_at, failure_reason, created_at, updated_at\
         ) VALUES ($1, $2, $3, 'queued', $4, NULL, NULL, 0, 0, $4, NULL, NULL, NULL, NULL, $4, $4) \
         ON CONFLICT DO NOTHING",
    )
    .bind(epoch_id.as_str())
    .bind(user_id)
    .bind(github_connection_id)
    .bind(now_rfc3339.as_str())
    .execute(pool)
    .await
    .context("create PostgreSQL star reconciliation epoch")?;
    if inserted.rows_affected() == 0 {
        // A concurrent executor created the epoch first; reuse it only when due.
        let Some(active) = load_active_epoch(pool, github_connection_id).await? else {
            return Ok(None);
        };
        let ready = star_sync_timestamp_due(active.next_slice_not_before.as_deref(), now)
            && star_sync_timestamp_due(active.lease_expires_at.as_deref(), now);
        return Ok(ready.then(|| epoch_dispatch(&active)));
    }
    Ok(Some(StarEpochDispatch {
        schedule_key: format!("epoch:{epoch_id}:slice:0"),
        epoch_id,
        due_at: Some(now),
    }))
}

/// List the active users whose epoch has a slice to run right now.
///
/// A slice is actionable when the epoch is still `queued`/`running`, its
/// `next_slice_not_before` has passed (or was never set) and no live lease
/// covers it. The scheduler uses this to wake the executor as soon as a slice
/// becomes due instead of waiting for the next delta bucket; the executor still
/// re-checks the predicate in [`claim_epoch_slice`] before doing any work.
pub(crate) async fn load_due_epoch_user_ids(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT e.user_id
         FROM public.star_sync_epochs e
         JOIN public.users u ON u.id = e.user_id
         WHERE e.status IN ('queued', 'running')
           AND (e.next_slice_not_before IS NULL
                OR e.next_slice_not_before::timestamptz <= $1::timestamptz)
           AND (e.lease_expires_at IS NULL
                OR e.lease_expires_at::timestamptz <= $1::timestamptz)
           AND u.is_disabled = 0
           AND u.paused_at IS NULL
         ORDER BY e.user_id",
    )
    .bind(now.to_rfc3339())
    .fetch_all(pool)
    .await
    .context("load due PostgreSQL star reconciliation epochs")
}

/// Claim the due slice of one epoch for a single executor.
///
/// The conditional `UPDATE ... RETURNING` is the single-flight gate: only one
/// caller can move a due, lease-free `queued`/`running` epoch into `running`.
pub(crate) async fn claim_epoch_slice(
    pool: &PgPool,
    epoch_id: &str,
    lease_owner_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<StarEpochRow>> {
    let now_rfc3339 = now.to_rfc3339();
    let lease_expires_at =
        (now + chrono::Duration::minutes(STAR_SYNC_EPOCH_LEASE_MINUTES)).to_rfc3339();
    let query = format!(
        "UPDATE public.star_sync_epochs \
         SET status = 'running', lease_owner_id = $2, lease_expires_at = $3, updated_at = $4 \
         WHERE id = $1 \
           AND status IN ('queued', 'running') \
           AND (next_slice_not_before IS NULL \
                OR next_slice_not_before::timestamptz <= $4::timestamptz) \
           AND (lease_expires_at IS NULL \
                OR lease_expires_at::timestamptz <= $4::timestamptz) \
         RETURNING {EPOCH_COLUMNS}"
    );
    sqlx::query_as::<_, StarEpochRow>(&query)
        .bind(epoch_id)
        .bind(lease_owner_id)
        .bind(lease_expires_at.as_str())
        .bind(now_rfc3339.as_str())
        .fetch_optional(pool)
        .await
        .context("claim PostgreSQL star epoch slice")
}

/// Persist a completed page and schedule the next slice.
///
/// The spacing mirrors the legacy coordinator: the full-sweep budget divided by
/// the page count implied by `total_count_at_start`, with a 20 second floor.
pub(crate) async fn schedule_next_slice(
    pool: &PgPool,
    epoch: &StarEpochRow,
    page: &StarSlicePage,
    full_sweep_interval_minutes: i64,
    lease_owner_id: &str,
    now: DateTime<Utc>,
) -> Result<StarEpochProgress> {
    let Some(next_cursor) = page.end_cursor.as_deref() else {
        fail_epoch(pool, epoch.id.as_str(), "missing_end_cursor", now).await?;
        bail!("star reconciliation response hasNextPage without endCursor");
    };
    let total_count = epoch
        .total_count_at_start
        .unwrap_or(page.total_count)
        .max(0);
    let expected_pages = ((total_count + STAR_SYNC_FULL_PAGE_SIZE as i64 - 1)
        / STAR_SYNC_FULL_PAGE_SIZE as i64)
        .max(1);
    let full_sweep_seconds =
        admin_runtime::normalize_star_sync_full_sweep_interval_minutes(full_sweep_interval_minutes)
            * 60;
    let page_spacing_seconds =
        (full_sweep_seconds / expected_pages).max(STAR_SYNC_MIN_SLICE_SPACING_SECONDS);
    let next_slice_not_before =
        (now + chrono::Duration::seconds(page_spacing_seconds)).to_rfc3339();
    let processed_pages = epoch.processed_pages + 1;
    let processed_items = epoch.processed_items + page.repo_count;
    let now_rfc3339 = now.to_rfc3339();
    let update = sqlx::query(
        "UPDATE public.star_sync_epochs \
         SET status = 'queued', next_cursor = $2, \
             total_count_at_start = COALESCE(total_count_at_start, $3), \
             processed_pages = $4, processed_items = $5, next_slice_not_before = $6, \
             lease_owner_id = NULL, lease_expires_at = NULL, updated_at = $7 \
         WHERE id = $1 AND status = 'running' AND lease_owner_id = $8",
    )
    .bind(epoch.id.as_str())
    .bind(next_cursor)
    .bind(page.total_count)
    .bind(processed_pages)
    .bind(processed_items)
    .bind(next_slice_not_before.as_str())
    .bind(now_rfc3339.as_str())
    .bind(lease_owner_id)
    .execute(pool)
    .await
    .context("schedule next PostgreSQL star reconciliation slice")?;
    if update.rows_affected() != 1 {
        bail!("star reconciliation lease was lost before scheduling the next slice");
    }
    Ok(StarEpochProgress {
        epoch_id: epoch.id.clone(),
        github_connection_id: epoch.github_connection_id.clone(),
        processed_pages,
        processed_items,
        total_count_at_start: total_count,
        has_next_page: true,
        next_slice_not_before: Some(next_slice_not_before),
        status: "queued",
        membership_removed: 0,
    })
}

/// Record a retryable slice failure and hand the epoch back to the queue.
///
/// The epoch keeps its cursor and counters, stores the upstream error and a
/// retry time instead of pretending the sweep finished. A caller whose lease was
/// already taken over by another executor is ignored, so the deferral cannot
/// release a live slice out from under the new owner.
pub(crate) async fn defer_epoch_after_failure(
    pool: &PgPool,
    epoch_id: &str,
    reason: &str,
    lease_owner_id: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    let next_slice_not_before =
        (now + chrono::Duration::minutes(STAR_SYNC_EPOCH_RETRY_MINUTES)).to_rfc3339();
    let now_rfc3339 = now.to_rfc3339();
    sqlx::query(
        "UPDATE public.star_sync_epochs \
         SET status = 'queued', failure_reason = $2, next_slice_not_before = $3, \
             lease_owner_id = NULL, lease_expires_at = NULL, updated_at = $4 \
         WHERE id = $1 AND status IN ('queued', 'running') \
           AND (lease_owner_id IS NULL OR lease_owner_id = $5)",
    )
    .bind(epoch_id)
    .bind(reason)
    .bind(next_slice_not_before.as_str())
    .bind(now_rfc3339.as_str())
    .bind(lease_owner_id)
    .execute(pool)
    .await
    .context("defer PostgreSQL star epoch after a failed slice")?;
    Ok(())
}

/// Terminally fail an epoch that cannot be retried (bad credentials, partial
/// upstream response that must never prune memberships).
pub(crate) async fn fail_epoch(
    pool: &PgPool,
    epoch_id: &str,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    let now_rfc3339 = now.to_rfc3339();
    sqlx::query(
        "UPDATE public.star_sync_epochs \
         SET status = 'failed', completed_at = $2, failure_reason = $3, \
             lease_owner_id = NULL, lease_expires_at = NULL, updated_at = $2 \
         WHERE id = $1 AND status IN ('queued', 'running')",
    )
    .bind(epoch_id)
    .bind(now_rfc3339.as_str())
    .bind(reason)
    .execute(pool)
    .await
    .context("mark PostgreSQL star epoch failed")?;
    Ok(())
}

/// Finish the epoch inside the caller's transaction.
///
/// Removes memberships this epoch never observed (and that no delta refreshed
/// after the epoch started), then marks the epoch succeeded with the final
/// counters. The returned rows let the caller rebuild aggregate Star rows in
/// the same transaction.
pub(crate) async fn finalize_epoch_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    epoch: &StarEpochRow,
    page: &StarSlicePage,
    lease_owner_id: &str,
    now: DateTime<Utc>,
) -> Result<Vec<StaleStarMembership>> {
    let stale = sqlx::query_as::<_, StaleStarMembership>(
        "SELECT repo_id FROM public.starred_repo_connection_memberships \
         WHERE user_id = $1 AND github_connection_id = $2 \
           AND (last_seen_epoch_id IS NULL OR last_seen_epoch_id <> $3) \
           AND (last_delta_seen_at IS NULL \
                OR last_delta_seen_at::timestamptz < $4::timestamptz) \
         ORDER BY repo_id ASC",
    )
    .bind(epoch.user_id.as_str())
    .bind(epoch.github_connection_id.as_str())
    .bind(epoch.id.as_str())
    .bind(epoch.started_at.as_str())
    .fetch_all(&mut **tx)
    .await
    .context("load stale PostgreSQL star memberships")?;
    sqlx::query(
        "DELETE FROM public.starred_repo_connection_memberships \
         WHERE user_id = $1 AND github_connection_id = $2 \
           AND (last_seen_epoch_id IS NULL OR last_seen_epoch_id <> $3) \
           AND (last_delta_seen_at IS NULL \
                OR last_delta_seen_at::timestamptz < $4::timestamptz)",
    )
    .bind(epoch.user_id.as_str())
    .bind(epoch.github_connection_id.as_str())
    .bind(epoch.id.as_str())
    .bind(epoch.started_at.as_str())
    .execute(&mut **tx)
    .await
    .context("delete stale PostgreSQL star memberships")?;

    let processed_pages = epoch.processed_pages + 1;
    let processed_items = epoch.processed_items + page.repo_count;
    let now_rfc3339 = now.to_rfc3339();
    let update = sqlx::query(
        "UPDATE public.star_sync_epochs \
         SET status = 'succeeded', \
             total_count_at_start = COALESCE(total_count_at_start, $2), \
             processed_pages = $3, processed_items = $4, next_slice_not_before = NULL, \
             lease_owner_id = NULL, lease_expires_at = NULL, completed_at = $5, \
             failure_reason = NULL, updated_at = $5 \
         WHERE id = $1 AND status = 'running' AND lease_owner_id = $6",
    )
    .bind(epoch.id.as_str())
    .bind(page.total_count)
    .bind(processed_pages)
    .bind(processed_items)
    .bind(now_rfc3339.as_str())
    .bind(lease_owner_id)
    .execute(&mut **tx)
    .await
    .context("complete PostgreSQL star reconciliation epoch")?;
    if update.rows_affected() != 1 {
        bail!("star reconciliation lease was lost before finalizing the epoch");
    }
    Ok(stale)
}

async fn load_active_epoch(
    pool: &PgPool,
    github_connection_id: &str,
) -> Result<Option<StarEpochRow>> {
    let query = format!(
        "SELECT {EPOCH_COLUMNS} FROM public.star_sync_epochs \
         WHERE github_connection_id = $1 AND status IN ('queued', 'running') \
         ORDER BY created_at ASC, id ASC LIMIT 1"
    );
    sqlx::query_as::<_, StarEpochRow>(&query)
        .bind(github_connection_id)
        .fetch_optional(pool)
        .await
        .context("load active PostgreSQL star reconciliation epoch")
}

fn epoch_dispatch(epoch: &StarEpochRow) -> StarEpochDispatch {
    StarEpochDispatch {
        epoch_id: epoch.id.clone(),
        schedule_key: format!(
            "epoch:{}:slice:{}:cursor:{}",
            epoch.id,
            epoch.processed_pages,
            epoch.next_cursor.as_deref().unwrap_or_default()
        ),
        due_at: epoch
            .next_slice_not_before
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc)),
    }
}

fn star_sync_timestamp_due(value: Option<&str>, now: DateTime<Utc>) -> bool {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|value| value.with_timezone(&Utc) <= now)
}
