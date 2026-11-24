//! PostgreSQL-native global content worker.
//!
//! The PostgreSQL runtime owns the authoritative content tables, but until now
//! nothing claimed `content_work_items`: the SQLite scheduler stayed behind the
//! retired runtime and `translation.generate` only writes the per-user
//! translation tables. This module is the missing executor.
//!
//! Scope of this module:
//! - claim one queued work item with a bounded lease (`FOR UPDATE SKIP LOCKED`),
//! - run the existing pipeline output contract (`build_prompt` +
//!   `validate_output`) through the routed provider client,
//! - persist attempt events, provider admissions and the LLM audit links,
//! - publish `content_result_projections` and `content_current_result_projections`,
//! - recover expired leases and expose a bounded producer for Release polishing.
//!
//! Reading-evidence polishing (`smart_public` / `smart_authorized`) needs the
//! PostgreSQL evidence producer and is intentionally not admitted here; this
//! module only admits the established `smart` polishing contract whose output
//! validation is satisfied by the stored source snapshot.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::{
    ai,
    config::AiConfig,
    content_identity_upgrade::{self, ContentWorkIdentity},
    content_processing::{
        self, GLOBAL_MAX_TOKENS, GLOBAL_PROTOCOL_VERSION, RETRY_DELAYS_SECS, SourceSnapshot,
    },
    local_id,
    postgres_translate::{self, ContentProviderAdmission},
    translations::{self, TranslationRequestItemInput, TranslationSourceBlock},
};

const LEASE_OWNER: &str = "content-pg-1";
const LENGTH_RECOVERY_MAX_TOKENS: u32 = 6_000;
const RECOVERY_BATCH: i64 = 50;
const RETRY_EXPIRES_HOURS: i64 = 24;
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const RECOVERY_INTERVAL: Duration = Duration::from_secs(15);
const TARGET_LANG: &str = "zh-CN";
/// Upper bound on polishing work one release sync may admit, mirroring the
/// translation enqueue cap so a cold repository cannot create an unbounded
/// provider burst.
const POLISH_ADMISSION_CAP: usize = 25;

#[derive(Debug, Clone, sqlx::FromRow)]
struct ClaimedWork {
    id: String,
    canonical_resource_type: String,
    canonical_resource_id: String,
    pipeline: String,
    variant: String,
    target_lang: String,
    source_hash: String,
    protocol_version: String,
    source_snapshot_json: String,
    attempt_count: i32,
}

struct Claim {
    work: ClaimedWork,
    attempt_event_id: String,
}

fn configuration_fingerprint(route_models: &[String]) -> String {
    ai::sha256_hex(&format!(
        "{GLOBAL_PROTOCOL_VERSION}\nroute={}",
        serde_json::to_string(route_models).unwrap_or_default()
    ))
}

fn source_revision_json(raw_snapshot: &str) -> String {
    let snapshot = serde_json::from_str::<SourceSnapshot>(raw_snapshot).ok();
    let observed_at = snapshot
        .as_ref()
        .and_then(|snapshot| {
            snapshot
                .source_blocks
                .iter()
                .find(|block| block.slot == "source_observed_at")
        })
        .map(|block| block.text.clone());
    let tiebreak = snapshot
        .as_ref()
        .and_then(|snapshot| {
            snapshot
                .source_blocks
                .iter()
                .find(|block| block.slot == "source_revision_tiebreak")
        })
        .map(|block| block.text.clone());
    serde_json::to_string(&json!({
        "source_observed_at": observed_at,
        "source_revision_tiebreak": tiebreak,
    }))
    .unwrap_or_else(|_| "{}".to_owned())
}

/// Claim at most one queued work item.
///
/// The transaction locks the candidate row, records the attempt start with the
/// route snapshot the attempt is bound to, and only then makes it visible as
/// `running`.
async fn claim_next(pool: &PgPool, fallback_model: &str) -> Result<Option<Claim>> {
    let route_models = postgres_translate::route_models_for_runtime(pool, fallback_model)
        .await
        .map_err(|failure| anyhow::anyhow!("{}: {}", failure.class.as_str(), failure.message))?;
    if route_models.is_empty() {
        return Ok(None);
    }
    let fingerprint = configuration_fingerprint(&route_models);
    let route_snapshot_json = serde_json::to_string(&route_models)?;
    let configuration_snapshot_json = json!({
        "model_route": route_models,
        "protocol_version": GLOBAL_PROTOCOL_VERSION,
    })
    .to_string();

    let mut tx = pool.begin().await.context("begin content claim")?;
    let row = sqlx::query_as::<_, ClaimedWork>(
        r#"
        SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant,
               target_lang, source_hash, protocol_version, source_snapshot_json,
               attempt_count::integer AS attempt_count
        FROM public.content_work_items
        WHERE status = 'queued'
          AND evidence_status IN ('not_required', 'ready')
          AND (next_retry_at IS NULL OR next_retry_at::timestamptz <= now())
          AND (retry_expires_at IS NULL OR retry_expires_at::timestamptz > now())
        ORDER BY priority DESC, created_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#,
    )
    .fetch_optional(&mut *tx)
    .await
    .context("select queued content work")?;
    let Some(work) = row else {
        tx.rollback().await?;
        return Ok(None);
    };
    let attempt_count: i32 = sqlx::query_scalar(
        "UPDATE public.content_work_items SET status = 'running', attempt_count = attempt_count + 1, started_at = now(), lease_owner = $2, lease_expires_at = now() + interval '300 seconds', next_retry_at = NULL, retry_after_at = NULL, updated_at = now() WHERE id = $1 RETURNING attempt_count::integer AS attempt_count",
    )
    .bind(&work.id)
    .bind(LEASE_OWNER)
    .fetch_one(&mut *tx)
    .await
    .context("mark content work running")?;
    for event_type in ["attempt_queued", "attempt_started"] {
        sqlx::query(
            "INSERT INTO public.content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, configuration_snapshot_json, route_snapshot_json, configuration_fingerprint, created_at) VALUES ($1, $2, $3, 'initial', $4, $5, $6, $7, now()) ON CONFLICT (work_item_id, attempt_no, event_type) DO NOTHING",
        )
        .bind(local_id::generate_local_id())
        .bind(&work.id)
        .bind(i64::from(attempt_count))
        .bind(event_type)
        .bind(&configuration_snapshot_json)
        .bind(&route_snapshot_json)
        .bind(&fingerprint)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("record content {event_type}"))?;
    }
    let attempt_event_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.content_attempt_events WHERE work_item_id = $1 AND attempt_no = $2 AND event_type = 'attempt_started'",
    )
    .bind(&work.id)
    .bind(i64::from(attempt_count))
    .fetch_one(&mut *tx)
    .await
    .context("load content attempt event")?;
    tx.commit().await.context("commit content claim")?;
    Ok(Some(Claim {
        work: ClaimedWork {
            attempt_count,
            ..work
        },
        attempt_event_id,
    }))
}

fn is_retryable(class: &str) -> bool {
    matches!(class, "empty_content" | "transient" | "rate_limited")
        || matches!(class, "output_contract_invalid" | "output_truncated")
}

fn next_retry_at(
    attempt_count: i32,
    retry_expires_at: Option<DateTime<Utc>>,
    class: &str,
) -> Option<DateTime<Utc>> {
    if !is_retryable(class) {
        return None;
    }
    let now = Utc::now();
    let expires_at = retry_expires_at.unwrap_or(now + chrono::Duration::hours(RETRY_EXPIRES_HOURS));
    let index = usize::try_from(attempt_count.saturating_sub(1))
        .unwrap_or(0)
        .min(RETRY_DELAYS_SECS.len() - 1);
    let candidate = now + chrono::Duration::seconds(RETRY_DELAYS_SECS[index]);
    (candidate < expires_at).then_some(candidate)
}

async fn read_retry_expires(pool: &PgPool, work_item_id: &str) -> Option<DateTime<Utc>> {
    sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        "SELECT retry_expires_at::timestamptz FROM public.content_work_items WHERE id = $1",
    )
    .bind(work_item_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
}

async fn publish_ready(
    pool: &PgPool,
    claim: &Claim,
    completion: &postgres_translate::AuditedCompletion,
    output: &Value,
) -> Result<()> {
    let work = &claim.work;
    let now = Utc::now();
    let token_count = match (completion.input_tokens, completion.output_tokens) {
        (Some(input), Some(output)) => Some(input.saturating_add(output)),
        _ => None,
    };
    let mut tx = pool.begin().await.context("begin content publish")?;
    let owned = sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.content_work_items WHERE id = $1 AND status = 'running' AND attempt_count = $2 AND lease_owner = $3 AND lease_expires_at::timestamptz > now() FOR UPDATE",
    )
    .bind(&work.id)
    .bind(work.attempt_count)
    .bind(LEASE_OWNER)
    .fetch_optional(&mut *tx)
    .await?;
    if owned.is_none() {
        tx.rollback().await?;
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO public.content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, duration_ms, token_count, created_at) VALUES ($1, $2, $3, 'initial', 'attempt_completed', 'ready', 0, $4, $5, now()) ON CONFLICT (work_item_id, attempt_no, event_type) DO NOTHING",
    )
    .bind(local_id::generate_local_id())
    .bind(&work.id)
    .bind(i64::from(work.attempt_count))
    .bind(completion.duration_ms)
    .bind(token_count)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE public.content_work_items SET status = 'ready', finished_at = $2, lease_owner = NULL, lease_expires_at = NULL, next_retry_at = NULL, retry_after_at = NULL, updated_at = $2 WHERE id = $1",
    )
    .bind(&work.id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let provider_call_id = completion.provider_request_id.clone().unwrap_or_else(|| {
        format!(
            "llm-call:{}",
            completion.call_id.clone().unwrap_or_default()
        )
    });
    sqlx::query(
        "INSERT INTO public.content_attempt_llm_calls (id, attempt_event_id, provider_call_id, model, status, duration_ms, input_tokens, output_tokens, created_at) VALUES ($1, $2, $3, $4, 'succeeded', $5, $6, $7, $8) ON CONFLICT (attempt_event_id, provider_call_id) DO NOTHING",
    )
    .bind(
        completion
            .call_id
            .clone()
            .unwrap_or_else(local_id::generate_local_id),
    )
    .bind(&claim.attempt_event_id)
    .bind(&provider_call_id)
    .bind(&completion.model)
    .bind(completion.duration_ms)
    .bind(completion.input_tokens)
    .bind(completion.output_tokens)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let identity = ContentWorkIdentity {
        canonical_resource_type: work.canonical_resource_type.clone(),
        canonical_resource_id: work.canonical_resource_id.clone(),
        pipeline: work.pipeline.clone(),
        variant: work.variant.clone(),
        target_lang: work.target_lang.clone(),
        source_hash: work.source_hash.clone(),
        protocol_version: work.protocol_version.clone(),
    };
    let identity_id = content_identity_upgrade::identity_id_for(&identity)?;
    sqlx::query(
        "INSERT INTO public.content_work_identities (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9) ON CONFLICT DO NOTHING",
    )
    .bind(&identity_id)
    .bind(&identity.canonical_resource_type)
    .bind(&identity.canonical_resource_id)
    .bind(&identity.pipeline)
    .bind(&identity.variant)
    .bind(&identity.target_lang)
    .bind(&identity.source_hash)
    .bind(&identity.protocol_version)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO public.content_work_identity_members (work_item_id, identity_id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, linked_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) ON CONFLICT DO NOTHING",
    )
    .bind(&work.id)
    .bind(&identity_id)
    .bind(&identity.canonical_resource_type)
    .bind(&identity.canonical_resource_id)
    .bind(&identity.pipeline)
    .bind(&identity.variant)
    .bind(&identity.target_lang)
    .bind(&identity.source_hash)
    .bind(&identity.protocol_version)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let projection_id: String = sqlx::query_scalar(
        "INSERT INTO public.content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, active_work_item_id, payload_json, published_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10, $11, $12, $12) ON CONFLICT (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash) DO UPDATE SET work_item_id = excluded.work_item_id, active_work_item_id = excluded.active_work_item_id, payload_json = excluded.payload_json, published_at = excluded.published_at, updated_at = excluded.updated_at RETURNING id",
    )
    .bind(local_id::generate_local_id())
    .bind(&work.canonical_resource_type)
    .bind(&work.canonical_resource_id)
    .bind(&work.pipeline)
    .bind(&work.variant)
    .bind(&work.target_lang)
    .bind(&work.protocol_version)
    .bind(&completion.model)
    .bind(&work.source_hash)
    .bind(&work.id)
    .bind(output.to_string())
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO public.content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES ($1, $2, $2, $3, $4, $5, $5) ON CONFLICT (identity_id) DO UPDATE SET work_item_id = excluded.work_item_id, active_work_item_id = excluded.active_work_item_id, source_projection_id = excluded.source_projection_id, payload_json = excluded.payload_json, published_at = excluded.published_at, updated_at = excluded.updated_at",
    )
    .bind(&identity_id)
    .bind(&work.id)
    .bind(&projection_id)
    .bind(output.to_string())
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await.context("commit content publish")?;
    info!(
        work_item_id = %work.id,
        pipeline = %work.pipeline,
        variant = %work.variant,
        model = %completion.model,
        "global content work published"
    );
    Ok(())
}

struct FailureOutcome {
    class: String,
    summary: Option<String>,
}

async fn publish_failure(pool: &PgPool, claim: &Claim, failure: FailureOutcome) -> Result<()> {
    let work = &claim.work;
    let now = Utc::now();
    let retry_expires = read_retry_expires(pool, &work.id)
        .await
        .unwrap_or(now + chrono::Duration::hours(RETRY_EXPIRES_HOURS));
    let next_retry = next_retry_at(work.attempt_count, Some(retry_expires), &failure.class);
    let blocked = failure.class == "configuration";
    let mut tx = pool.begin().await.context("begin content failure")?;
    let owned = sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.content_work_items WHERE id = $1 AND status = 'running' AND attempt_count = $2 AND lease_owner = $3 AND lease_expires_at::timestamptz > now() FOR UPDATE",
    )
    .bind(&work.id)
    .bind(work.attempt_count)
    .bind(LEASE_OWNER)
    .fetch_optional(&mut *tx)
    .await?;
    if owned.is_none() {
        tx.rollback().await?;
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO public.content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_eligible, next_retry_at, created_at) VALUES ($1, $2, $3, 'initial', 'attempt_completed', 'failed', $4, $5, $6, $7, $8, now()) ON CONFLICT (work_item_id, attempt_no, event_type) DO NOTHING",
    )
    .bind(local_id::generate_local_id())
    .bind(&work.id)
    .bind(i64::from(work.attempt_count))
    .bind(&failure.class)
    .bind(failure.summary.as_deref())
    .bind(&failure.class)
    .bind(i64::from(next_retry.is_some()))
    .bind(next_retry)
    .execute(&mut *tx)
    .await?;
    // A retryable failure returns to `queued` with a future `next_retry_at`:
    // the claim filter already honours that timestamp, so the retry chain does
    // not depend on a second sweep and a lost sweep cannot strand the item.
    let status = if blocked {
        "blocked_config"
    } else if next_retry.is_some() {
        "queued"
    } else {
        "failed"
    };
    sqlx::query(
        "UPDATE public.content_work_items SET status = $2, failure_class = $3, next_retry_at = $4, retry_expires_at = COALESCE(retry_expires_at::timestamptz, $5), retry_after_at = $4, finished_at = $6, lease_owner = NULL, lease_expires_at = NULL, updated_at = $6 WHERE id = $1",
    )
    .bind(&work.id)
    .bind(status)
    .bind(&failure.class)
    .bind(next_retry)
    .bind(retry_expires)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await.context("commit content failure")?;
    warn!(
        work_item_id = %work.id,
        pipeline = %work.pipeline,
        failure_class = %failure.class,
        retry_scheduled = next_retry.is_some(),
        "global content work failed"
    );
    Ok(())
}

/// Run one claimed work item end to end.
async fn execute(pool: &PgPool, http: &Client, ai_config: &AiConfig, claim: &Claim) -> Result<()> {
    let work = &claim.work;
    // A malformed snapshot is a content failure, not an infrastructure error:
    // leaving the claim running would requeue the same poison row forever.
    let snapshot = match serde_json::from_str::<SourceSnapshot>(&work.source_snapshot_json) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return publish_failure(
                pool,
                claim,
                FailureOutcome {
                    // A malformed snapshot is deterministic: retrying it would
                    // only re-parse the same bytes.
                    class: "invalid_source_snapshot".to_owned(),
                    summary: Some(format!("invalid global source snapshot: {error}")),
                },
            )
            .await;
        }
    };
    let (system, user) = content_processing::build_prompt(&snapshot, &work.pipeline);
    let source = format!(
        "content_processing.global.{}.stage.content_output.role.primary",
        work.pipeline
    );
    let source_revision = source_revision_json(&work.source_snapshot_json);
    let admission = ContentProviderAdmission {
        pool,
        work_item_id: &work.id,
        attempt_event_id: &claim.attempt_event_id,
        attempt_no: i64::from(work.attempt_count),
        lease_owner: LEASE_OWNER,
        source_hash: &work.source_hash,
        source_revision_json: &source_revision,
        call_ordinal_base: 0,
    };
    let mut completion = match postgres_translate::complete_with_audit(
        pool,
        http,
        ai_config,
        postgres_translate::CompletionRequest {
            source: &source,
            requested_by: None,
            parent_task: None,
            system: &system,
            user: &user,
            max_tokens: GLOBAL_MAX_TOKENS,
        },
        Some(&admission),
    )
    .await
    {
        Ok(completion) => completion,
        Err(postgres_translate::CompletionFailure::LeaseLost) => return Ok(()),
        Err(postgres_translate::CompletionFailure::Provider(failure)) => {
            return publish_failure(
                pool,
                claim,
                FailureOutcome {
                    class: failure.class.as_str().to_owned(),
                    summary: translations::translation_error_summary(Some(&failure.message)),
                },
            )
            .await;
        }
    };

    if completion.finish_reason.as_deref() == Some("length") {
        let recovery_admission = ContentProviderAdmission {
            pool,
            work_item_id: &work.id,
            attempt_event_id: &claim.attempt_event_id,
            attempt_no: i64::from(work.attempt_count),
            lease_owner: LEASE_OWNER,
            source_hash: &work.source_hash,
            source_revision_json: &source_revision,
            call_ordinal_base: completion.attempts_used,
        };
        completion = match postgres_translate::complete_with_audit(
            pool,
            http,
            ai_config,
            postgres_translate::CompletionRequest {
                source: &source,
                requested_by: None,
                parent_task: None,
                system: &system,
                user: &user,
                max_tokens: LENGTH_RECOVERY_MAX_TOKENS,
            },
            Some(&recovery_admission),
        )
        .await
        {
            Ok(completion) => completion,
            Err(postgres_translate::CompletionFailure::LeaseLost) => return Ok(()),
            Err(postgres_translate::CompletionFailure::Provider(failure)) => {
                return publish_failure(
                    pool,
                    claim,
                    FailureOutcome {
                        class: failure.class.as_str().to_owned(),
                        summary: translations::translation_error_summary(Some(&failure.message)),
                    },
                )
                .await;
            }
        };
        if completion.finish_reason.as_deref() == Some("length") {
            return publish_failure(
                pool,
                claim,
                FailureOutcome {
                    class: "output_truncated".to_owned(),
                    summary: Some("global content output was truncated".to_owned()),
                },
            )
            .await;
        }
    }

    let output = match content_processing::validate_output(
        &completion.content,
        &snapshot.target_slots,
        &snapshot.source_blocks,
    ) {
        Ok(output) => output,
        Err(error) => {
            return publish_failure(
                pool,
                claim,
                FailureOutcome {
                    class: "output_contract_invalid".to_owned(),
                    summary: translations::translation_error_summary(Some(&error.to_string())),
                },
            )
            .await;
        }
    };
    publish_ready(pool, claim, &completion, &output).await
}

/// Requeue work whose worker lease expired without a terminal attempt.
pub(crate) async fn recover_expired_leases(pool: &PgPool) -> Result<u64> {
    let rows = sqlx::query_as::<_, (String, i32)>(
        "UPDATE public.content_work_items SET status = CASE WHEN retry_expires_at IS NOT NULL AND retry_expires_at::timestamptz <= now() THEN 'failed' ELSE 'queued' END, lease_owner = NULL, lease_expires_at = NULL, next_retry_at = CASE WHEN retry_expires_at IS NOT NULL AND retry_expires_at::timestamptz <= now() THEN NULL ELSE now() + interval '60 seconds' END, retry_expires_at = COALESCE(retry_expires_at::timestamptz, now() + interval '24 hours'), retry_after_at = now() + interval '60 seconds', updated_at = now() WHERE id IN (SELECT id FROM public.content_work_items WHERE status = 'running' AND lease_expires_at IS NOT NULL AND lease_expires_at::timestamptz <= now() ORDER BY lease_expires_at::timestamptz LIMIT $1 FOR UPDATE SKIP LOCKED) RETURNING id, attempt_count::integer AS attempt_count",
    )
    .bind(RECOVERY_BATCH)
    .fetch_all(pool)
    .await
    .context("recover expired content leases")?;
    for (id, attempt_count) in &rows {
        sqlx::query(
            "INSERT INTO public.content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_eligible, next_retry_at, created_at) VALUES ($1, $2, $3, 'automatic_recovery', 'attempt_completed', 'failed', 'runtime_lease_expired', 'worker lease expired', 'runtime_lease_expired', 1, now() + interval '60 seconds', now()) ON CONFLICT (work_item_id, attempt_no, event_type) DO NOTHING",
        )
        .bind(local_id::generate_local_id())
        .bind(id)
        .bind(i64::from(*attempt_count))
        .execute(pool)
        .await
        .context("record expired content lease")?;
    }
    if !rows.is_empty() {
        info!(
            recovered = rows.len(),
            "requeued expired content work leases"
        );
    }
    Ok(u64::try_from(rows.len()).unwrap_or(u64::MAX))
}

/// Requeue configuration-blocked work after the model route changes.
///
/// `blocked_config` is terminal for the claim filter and counts as active work
/// for producers, so without this sweep a repaired configuration would never
/// resume the work it blocked.
pub(crate) async fn unblock_reconfigured_work(pool: &PgPool, fallback_model: &str) -> Result<u64> {
    let Ok(models) = postgres_translate::route_models_for_runtime(pool, fallback_model).await
    else {
        return Ok(0);
    };
    if models.is_empty() {
        return Ok(0);
    }
    let fingerprint = configuration_fingerprint(&models);
    let rows = sqlx::query_as::<_, (String, i32)>(
        "UPDATE public.content_work_items SET status = 'queued', failure_class = NULL, next_retry_at = NULL, retry_expires_at = NULL, retry_after_at = NULL, finished_at = NULL, lease_owner = NULL, lease_expires_at = NULL, updated_at = now() WHERE id IN (SELECT id FROM public.content_work_items WHERE status = 'blocked_config' AND configuration_fingerprint <> $1 ORDER BY updated_at LIMIT $2 FOR UPDATE SKIP LOCKED) RETURNING id, attempt_count::integer AS attempt_count",
    )
    .bind(&fingerprint)
    .bind(RECOVERY_BATCH)
    .fetch_all(pool)
    .await
    .context("unblock reconfigured content work")?;
    for (id, attempt_count) in &rows {
        sqlx::query(
            "INSERT INTO public.content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, created_at) VALUES ($1, $2, $3, 'automatic_recovery', 'attempt_queued', 'queued', 1, now()) ON CONFLICT (work_item_id, attempt_no, event_type) DO NOTHING",
        )
        .bind(local_id::generate_local_id())
        .bind(id)
        .bind(i64::from(*attempt_count) + 1)
        .execute(pool)
        .await
        .context("record unblocked content work")?;
    }
    if !rows.is_empty() {
        info!(unblocked = rows.len(), "resumed reconfigured content work");
    }
    Ok(u64::try_from(rows.len()).unwrap_or(u64::MAX))
}

async fn run_loop(
    pool: PgPool,
    http: Client,
    ai_config: AiConfig,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut recovery = tokio::time::interval(RECOVERY_INTERVAL);
    recovery.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
            _ = recovery.tick() => {
                if let Err(error) = recover_expired_leases(&pool).await {
                    warn!(%error, "content worker lease recovery failed");
                }
                if let Err(error) =
                    unblock_reconfigured_work(&pool, ai_config.model.as_str()).await
                {
                    warn!(%error, "content worker configuration recovery failed");
                }
            }
            _ = tokio::time::sleep(POLL_INTERVAL) => {
                match claim_next(&pool, ai_config.model.as_str()).await {
                    Ok(Some(claim)) => {
                        if let Err(error) = execute(&pool, &http, &ai_config, &claim).await {
                            warn!(%error, work_item_id = %claim.work.id, "content worker execution failed");
                        }
                    }
                    Ok(None) => {}
                    Err(error) => warn!(%error, "content worker claim failed"),
                }
            }
        }
    }
}

/// Start the PostgreSQL content worker for this process.
pub(crate) fn spawn(
    pool: PgPool,
    http: Client,
    ai: Option<AiConfig>,
    shutdown: watch::Receiver<bool>,
) -> Option<tokio::task::JoinHandle<()>> {
    let ai = ai?;
    Some(tokio::spawn(run_loop(pool, http, ai, shutdown)))
}

/// Admit Release polishing work that the established `smart` contract can
/// validate from the stored release source alone.
///
/// Reading-evidence variants are excluded until the PostgreSQL evidence
/// producer exists: admitting them here would queue work the output contract
/// must reject.
pub(crate) async fn admit_release_polish(
    pool: &PgPool,
    release_ids: &[i64],
    fallback_model: &str,
) -> Result<usize> {
    if release_ids.is_empty() {
        return Ok(0);
    }
    let route_models = postgres_translate::route_models_for_runtime(pool, fallback_model).await?;
    if route_models.is_empty() {
        return Ok(0);
    }
    let fingerprint = configuration_fingerprint(&route_models);
    let mut admitted = 0usize;
    for release_id in release_ids {
        if admitted >= POLISH_ADMISSION_CAP {
            break;
        }
        let Ok(Some(row)) = sqlx::query(
            "SELECT repo_id, html_url, tag_name, name, body, updated_at, detected_at FROM public.repo_releases WHERE release_id = $1",
        )
        .bind(release_id)
        .fetch_optional(pool)
        .await
        else {
            continue;
        };
        let repo_id: i64 = row.try_get("repo_id")?;
        let html_url: String = row.try_get("html_url")?;
        let tag_name: String = row.try_get("tag_name")?;
        let name: Option<String> = row.try_get("name")?;
        let body: Option<String> = row.try_get("body")?;
        let updated_at: Option<String> = row.try_get("updated_at").ok().flatten();
        let detected_at: Option<String> = row.try_get("detected_at").ok().flatten();
        let body = body
            .map(|value| value.replace("\r\n", "\n"))
            .map(|value| value.trim().to_owned())
            .unwrap_or_default();
        let title = name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| tag_name.trim().to_owned());
        if title.is_empty() && body.is_empty() {
            continue;
        }
        let repo_identity =
            crate::postgres_content_overlay::release_repo_identity(&html_url, repo_id);
        let observed_at = content_processing::authoritative_release_revision(
            updated_at.as_deref().unwrap_or_default(),
            detected_at.as_deref(),
        );
        let tiebreak = content_processing::release_source_revision_content_tiebreak(&[
            html_url.as_str(),
            tag_name.as_str(),
            title.as_str(),
            body.as_str(),
        ]);
        let mut source_blocks = vec![
            TranslationSourceBlock {
                slot: "metadata".to_owned(),
                text: format!("{repo_identity}\n{}", tag_name.trim()),
            },
            TranslationSourceBlock {
                slot: "title".to_owned(),
                text: title.clone(),
            },
        ];
        if !body.is_empty() {
            source_blocks.push(TranslationSourceBlock {
                slot: "body_markdown".to_owned(),
                text: body.clone(),
            });
        }
        if let Some(observed_at) = observed_at.filter(|value| !value.trim().is_empty()) {
            source_blocks.insert(
                0,
                TranslationSourceBlock {
                    slot: "source_revision_tiebreak".to_owned(),
                    text: tiebreak.clone(),
                },
            );
            source_blocks.insert(
                0,
                TranslationSourceBlock {
                    slot: "source_observed_at".to_owned(),
                    text: observed_at.to_owned(),
                },
            );
        }
        let item = TranslationRequestItemInput {
            producer_ref: "sync.releases.polish".to_owned(),
            kind: "release_smart".to_owned(),
            variant: "smart".to_owned(),
            entity_id: release_id.to_string(),
            target_lang: TARGET_LANG.to_owned(),
            max_wait_ms: 0,
            source_blocks,
            target_slots: vec!["title_zh".to_owned(), "summary_md".to_owned()],
        };
        let source_hash = content_processing::source_hash_for_item(&item);
        let identity = ContentWorkIdentity {
            canonical_resource_type: "release".to_owned(),
            canonical_resource_id: release_id.to_string(),
            pipeline: "polishing".to_owned(),
            variant: "smart".to_owned(),
            target_lang: TARGET_LANG.to_owned(),
            source_hash: source_hash.clone(),
            protocol_version: GLOBAL_PROTOCOL_VERSION.to_owned(),
        };
        let identity_id = content_identity_upgrade::identity_id_for(&identity)?;
        let has_projection: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM public.content_current_result_projections WHERE identity_id = $1)",
        )
        .bind(&identity_id)
        .fetch_one(pool)
        .await?;
        if has_projection {
            continue;
        }
        let already_active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM public.content_work_items WHERE canonical_resource_type = 'release' AND canonical_resource_id = $1 AND pipeline = 'polishing' AND variant = 'smart' AND target_lang = $2 AND protocol_version = $3 AND status IN ('queued', 'running', 'ready', 'deferred_provider', 'blocked_config'))",
        )
        .bind(release_id.to_string())
        .bind(TARGET_LANG)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .fetch_one(pool)
        .await?;
        if already_active {
            continue;
        }
        let snapshot = serde_json::to_string(&json!({
            "source_blocks": item.source_blocks,
            "target_slots": item.target_slots,
            "source_revision": {
                "source_observed_at": observed_at,
                "source_revision_tiebreak": tiebreak,
            },
            "evidence_snapshot_id": Value::Null,
            "evidence_status": "not_required",
        }))?;
        let inserted = sqlx::query(
            // `cache_hit` stays out of the insert: the migrated schema declares
            // it boolean, the imported one bigint, and both default to a falsy
            // value, so only the column default is portable.
            "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, token_estimate, attempt_count, evidence_status, created_at, updated_at) VALUES ($1, 'release', $2, 'polishing', 'smart', $3, $4, $5, 'postgres-default', $6, $7, 'queued', 0, $8, 0, 'not_required', now(), now()) ON CONFLICT (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile) DO NOTHING",
        )
        .bind(local_id::generate_local_id())
        .bind(release_id.to_string())
        .bind(TARGET_LANG)
        .bind(&source_hash)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(&snapshot)
        .bind(&fingerprint)
        .bind(i64::try_from(body.chars().count() + title.chars().count()).unwrap_or(i64::MAX))
        .execute(pool)
        .await
        .context("admit release polishing work")?;
        if inserted.rows_affected() == 0 {
            // A terminal row for the same source hash cannot be re-admitted,
            // and the unique conflict is the only signal. Surface it instead of
            // letting the release look silently polished.
            let terminal: Option<String> = sqlx::query_scalar(
                "SELECT status FROM public.content_work_items WHERE canonical_resource_type = 'release' AND canonical_resource_id = $1 AND pipeline = 'polishing' AND variant = 'smart' AND target_lang = $2 AND protocol_version = $3 AND source_hash = $4 AND model_profile = 'postgres-default' LIMIT 1",
            )
            .bind(release_id.to_string())
            .bind(TARGET_LANG)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(&source_hash)
            .fetch_optional(pool)
            .await?;
            if matches!(
                terminal.as_deref(),
                Some("failed" | "cancelled" | "superseded")
            ) {
                tracing::debug!(
                    release_id = %release_id,
                    status = terminal.as_deref().unwrap_or_default(),
                    "release polishing work stayed terminal; not re-admitted"
                );
            }
        }
        admitted += usize::try_from(inserted.rows_affected()).unwrap_or(0);
    }
    if admitted > 0 {
        info!(admitted, "global content polishing work admitted");
    }
    Ok(admitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_content_worker_publishes_a_polish_projection() -> anyhow::Result<()> {
        use axum::{Json, Router, routing::post};
        use sqlx::PgPool;

        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("content_worker_{}", local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;

        let app = Router::new().route(
            "/chat/completions",
            post({
                let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                move |Json(payload): Json<Value>| {
                    let calls = calls.clone();
                    async move {
                        let model = payload["model"].as_str().unwrap_or_default().to_owned();
                        let index = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        // Request 2 is the length-recovery fixture: truncate once.
                        let finish_reason = if index == 1 { "length" } else { "stop" };
                        Json(json!({
                            "id": format!("provider-call-{index}"),
                            "model": model,
                            "choices": [{
                                "message": {"content": "{\"title_zh\":\"é¡¹ç›® v1 å‘å¸ƒ\",\"summary_md\":\"- æ–°å¢žäº†å¹¶å‘Œå¤„ç†\"}"},
                                "finish_reason": finish_reason
                            }],
                            "usage": {"prompt_tokens": 11, "completion_tokens": 5, "total_tokens": 16}
                        }))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve stub provider");
        });

        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE public.admin_runtime_settings(id integer PRIMARY KEY, llm_models_json text NOT NULL);
                INSERT INTO public.admin_runtime_settings(id, llm_models_json) VALUES (1, '["m1"]');
                CREATE TABLE public.llm_model_health(model text PRIMARY KEY, relevant_failure_count integer NOT NULL DEFAULT 0, window_started_at timestamptz, cooldown_until timestamptz, last_failure_class text, last_failure_at timestamptz, created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL);
                CREATE TABLE public.llm_calls(id text PRIMARY KEY, status text NOT NULL, source text NOT NULL, model text NOT NULL, requested_by text, parent_task_id text, parent_task_type text, max_tokens bigint NOT NULL, attempt_count bigint NOT NULL DEFAULT 0, scheduler_wait_ms bigint NOT NULL DEFAULT 0, duration_ms bigint, prompt_text text NOT NULL, response_text text, error_text text, created_at text NOT NULL, started_at text, finished_at text, updated_at text NOT NULL, input_messages_json text, output_messages_json text, input_tokens bigint, output_tokens bigint, cached_input_tokens bigint, total_tokens bigint, first_token_wait_ms bigint, parent_translation_batch_id text, runtime_owner_id text, lease_heartbeat_at text, failure_class text, final_model text, fallback_count bigint NOT NULL DEFAULT 0, retry_scheduled_at text, recovery_attempt_count bigint NOT NULL DEFAULT 0, parent_brief_id text, finish_reason text, provider_request_id text, provider_http_status bigint);
                CREATE TABLE public.llm_call_events(id text PRIMARY KEY, call_id text NOT NULL, event_type text NOT NULL, status text NOT NULL, source text NOT NULL, requested_by text, parent_task_id text, payload_json text, created_at text NOT NULL, failure_class text, model text, attempt bigint, retry_after_ms bigint, from_model text, to_model text, fallback_count bigint);
                CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY, repo_id bigint NOT NULL, html_url text NOT NULL, tag_name text NOT NULL, name text, body text, updated_at text, detected_at text);
                INSERT INTO public.repo_releases(release_id, repo_id, html_url, tag_name, name, body, updated_at) VALUES (42, 7, 'https://github.com/acme/demo/releases/tag/v1', 'v1', 'v1', 'Release notes body', '2026-11-24T00:00:00Z');
                CREATE TABLE public.content_work_items(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_snapshot_json text NOT NULL, configuration_fingerprint text NOT NULL, status text NOT NULL, priority integer NOT NULL DEFAULT 0, cache_hit boolean NOT NULL DEFAULT false, token_estimate integer NOT NULL DEFAULT 0, batch_id text, lease_owner text, lease_expires_at timestamptz, attempt_count integer NOT NULL DEFAULT 0, next_retry_at timestamptz, retry_expires_at timestamptz, retry_after_at timestamptz, failure_class text, supersedes_work_item_id text, cancelled_at timestamptz, created_at timestamptz NOT NULL, started_at timestamptz, finished_at timestamptz, updated_at timestamptz NOT NULL, evidence_snapshot_id text, evidence_status text NOT NULL DEFAULT 'not_required', UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile));
                CREATE TABLE public.content_attempt_events(id text PRIMARY KEY, work_item_id text NOT NULL, attempt_no bigint NOT NULL, trigger text NOT NULL, event_type text NOT NULL, result_status text, error_code text, error_summary text, failure_class text, retry_disposition text, retry_eligible bigint NOT NULL DEFAULT 0, next_retry_at text, duration_ms bigint, token_count bigint, cost_microunits bigint, created_at text NOT NULL, configuration_snapshot_json text, route_snapshot_json text, configuration_fingerprint text, UNIQUE (work_item_id, attempt_no, event_type));
                CREATE TABLE public.content_attempt_provider_admissions(id text PRIMARY KEY, attempt_event_id text NOT NULL, work_item_id text NOT NULL, attempt_no bigint NOT NULL, call_ordinal bigint NOT NULL, relation_role text NOT NULL, source_hash text NOT NULL, source_revision_json text NOT NULL DEFAULT '{}', admitted_at text NOT NULL, UNIQUE (attempt_event_id, call_ordinal));
                CREATE TABLE public.content_attempt_llm_calls(id text PRIMARY KEY, attempt_event_id text NOT NULL, provider_call_id text NOT NULL, model text NOT NULL, status text NOT NULL, duration_ms bigint, input_tokens bigint, output_tokens bigint, cost_microunits bigint, error_code text, error_summary text, created_at text NOT NULL, UNIQUE (attempt_event_id, provider_call_id));
                CREATE TABLE public.content_result_projections(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_hash text NOT NULL, work_item_id text NOT NULL, active_work_item_id text, payload_json text NOT NULL, published_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash));
                CREATE TABLE public.content_work_identities(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version));
                CREATE TABLE public.content_work_identity_members(work_item_id text PRIMARY KEY, identity_id text NOT NULL, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, linked_at text NOT NULL, UNIQUE (identity_id, work_item_id));
                CREATE TABLE public.content_current_result_projections(identity_id text PRIMARY KEY, work_item_id text NOT NULL, active_work_item_id text, source_projection_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL);
                "#,
            )
            .execute(&pool)
            .await?;

            let admitted = admit_release_polish(&pool, &[42], "m1").await?;
            assert_eq!(admitted, 1, "one polishing work item is admitted");
            let admitted_again = admit_release_polish(&pool, &[42], "m1").await?;
            assert_eq!(admitted_again, 0, "an active work item is not re-admitted");

            let ai = AiConfig {
                base_url: url::Url::parse(&format!("http://{addr}/"))?,
                model: "m1".to_owned(),
                api_key: "test-key".to_owned(),
            };
            let http = Client::new();
            let claim = claim_next(&pool, "m1")
                .await?
                .expect("queued polishing work is claimable");
            assert_eq!(claim.work.pipeline, "polishing");
            assert_eq!(claim.work.variant, "smart");
            execute(&pool, &http, &ai, &claim).await?;

            let (status, attempt_count): (String, i32) = sqlx::query_as(
                "SELECT status, attempt_count FROM public.content_work_items WHERE id = $1",
            )
            .bind(&claim.work.id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(status, "ready");
            assert_eq!(attempt_count, 1);

            let payload: String = sqlx::query_scalar(
                "SELECT payload_json FROM public.content_current_result_projections",
            )
            .fetch_one(&pool)
            .await?;
            let payload: Value = serde_json::from_str(&payload)?;
            assert_eq!(payload["title_zh"], json!("é¡¹ç›® v1 å‘å¸ƒ"));
            assert_eq!(payload["summary_md"], json!("- æ–°å¢žäº†å¹¶å‘Œå¤„ç†"));

            let events: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_attempt_events WHERE work_item_id = $1",
            )
            .bind(&claim.work.id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(events, 3, "queued, started and completed");
            let admissions: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_attempt_provider_admissions WHERE work_item_id = $1",
            )
            .bind(&claim.work.id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(admissions, 1);
            let calls: (String, String) = sqlx::query_as(
                "SELECT status, model FROM public.llm_calls ORDER BY created_at DESC LIMIT 1",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(calls.0, "succeeded");
            assert_eq!(calls.1, "m1");
            let linked: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_attempt_llm_calls WHERE attempt_event_id = $1",
            )
            .bind(&claim.attempt_event_id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(linked, 1);

            // Production databases imported from SQLite carry these columns as
            // text; every comparison must cast so the same SQL works on both
            // the migrated schema and the imported one.
            sqlx::raw_sql(
                r#"
                ALTER TABLE public.content_work_items
                    ALTER COLUMN created_at TYPE text USING created_at::text,
                    ALTER COLUMN updated_at TYPE text USING updated_at::text,
                    ALTER COLUMN started_at TYPE text USING started_at::text,
                    ALTER COLUMN finished_at TYPE text USING finished_at::text,
                    ALTER COLUMN lease_expires_at TYPE text USING lease_expires_at::text,
                    ALTER COLUMN next_retry_at TYPE text USING next_retry_at::text,
                    ALTER COLUMN retry_expires_at TYPE text USING retry_expires_at::text,
                    ALTER COLUMN retry_after_at TYPE text USING retry_after_at::text,
                    ALTER COLUMN cancelled_at TYPE text USING cancelled_at::text;
                ALTER TABLE public.content_result_projections
                    ALTER COLUMN published_at TYPE text USING published_at::text,
                    ALTER COLUMN updated_at TYPE text USING updated_at::text;
                ALTER TABLE public.content_work_items
                    ALTER COLUMN cache_hit DROP DEFAULT,
                    ALTER COLUMN cache_hit TYPE bigint USING cache_hit::int::bigint,
                    ALTER COLUMN cache_hit SET DEFAULT 0,
                    ALTER COLUMN attempt_count TYPE bigint USING attempt_count::bigint;
                INSERT INTO public.repo_releases(release_id, repo_id, html_url, tag_name, name, body, updated_at)
                  VALUES (46, 7, 'https://github.com/acme/demo/releases/tag/v2', 'v2', 'v2', 'Second notes', '2026-11-24T01:00:00Z');
                "#,
            )
            .execute(&pool)
            .await?;
            let admitted_text = admit_release_polish(&pool, &[46], "m1").await?;
            assert_eq!(
                admitted_text, 1,
                "admission tolerates the imported bigint cache_hit column"
            );
            sqlx::query(
                "DELETE FROM public.content_work_items WHERE canonical_resource_id = '46'",
            )
            .execute(&pool)
            .await?;
            sqlx::query(
                "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, evidence_status, lease_owner, lease_expires_at, created_at, updated_at) VALUES ('expired-work', 'release', '45', 'polishing', 'smart', 'zh-CN', 'expired', $1, 'postgres-default', '{}', 'fingerprint', 'running', 0, DEFAULT, 0, 1, 'not_required', 'dead-worker', (now() - interval '5 minutes')::text, (now() - interval '10 minutes')::text, (now() - interval '10 minutes')::text)",
            )
            .bind(GLOBAL_PROTOCOL_VERSION)
            .execute(&pool)
            .await?;
            let recovered = recover_expired_leases(&pool).await?;
            assert_eq!(recovered, 1, "the expired text-schema lease is recovered");
            let (expired_status, expired_next): (String, Option<String>) =
                sqlx::query_as(
                    "SELECT status, next_retry_at FROM public.content_work_items WHERE id = 'expired-work'",
                )
                .fetch_one(&pool)
                .await?;
            assert_eq!(expired_status, "queued");
            assert!(expired_next.is_some());
            sqlx::query(
                "UPDATE public.content_work_items SET status = 'superseded' WHERE id = 'expired-work'",
            )
            .execute(&pool)
            .await?;

            // A malformed snapshot must end in a recorded content failure, not a
            // permanently running claim that recovery requeues forever.
            sqlx::query(
                "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, evidence_status, created_at, updated_at) VALUES ('poison-work', 'release', '43', 'polishing', 'smart', 'zh-CN', 'poison', $1, 'postgres-default', 'not-json', 'fingerprint', 'queued', 0, DEFAULT, 0, 0, 'not_required', now(), now())",
            )
            .bind(GLOBAL_PROTOCOL_VERSION)
            .execute(&pool)
            .await?;
            let poison = claim_next(&pool, "m1")
                .await?
                .expect("poison work is claimable");
            execute(&pool, &http, &ai, &poison).await?;
            let (poison_status, poison_class): (String, Option<String>) = sqlx::query_as(
                "SELECT status, failure_class FROM public.content_work_items WHERE id = 'poison-work'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(poison_status, "failed");
            assert_eq!(poison_class.as_deref(), Some("invalid_source_snapshot"));
            let poison_completed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_attempt_events WHERE work_item_id = 'poison-work' AND event_type = 'attempt_completed'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(poison_completed, 1);

            // The length-recovery call must append its own admission instead of
            // colliding with the primary call's ordinal.
            let sd = serde_json::to_string(&json!({
                "source_blocks": [
                    {"slot": "metadata", "text": "acme/demo\nv1"},
                    {"slot": "title", "text": "v1"},
                    {"slot": "body_markdown", "text": "Release notes body"}
                ],
                "target_slots": ["title_zh", "summary_md"]
            }))?;
            sqlx::query(
                "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, evidence_status, created_at, updated_at) VALUES ('length-work', 'release', '44', 'polishing', 'smart', 'zh-CN', 'length', $1, 'postgres-default', $2, 'fingerprint', 'queued', 0, DEFAULT, 0, 0, 'not_required', now(), now())",
            )
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(&sd)
            .execute(&pool)
            .await?;
            let length_work = claim_next(&pool, "m1")
                .await?
                .expect("length work is claimable");
            execute(&pool, &http, &ai, &length_work).await?;
            let length_status: String = sqlx::query_scalar(
                "SELECT status FROM public.content_work_items WHERE id = 'length-work'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(length_status, "ready");
            let length_admissions: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_attempt_provider_admissions WHERE work_item_id = 'length-work'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(
                length_admissions, 2,
                "primary and length-recovery requests both need an admission fact"
            );

            let next = claim_next(&pool, "m1").await?;
            assert!(next.is_none(), "a ready work item is not claimable again");
            Ok::<_, anyhow::Error>(())
        }
        .await;

        server.abort();
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await?;
        admin.close().await;
        result
    }
}
