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
//! This module also owns the **only** result-projection writer. The translation
//! lane (`postgres_translate`) stays admission and attempt state; when one of
//! its attempts finishes with output that passes the shared output contract, it
//! hands the validated payload to [`publish_translation_result`] instead of
//! writing `content_result_projections` itself.
//!
//! Terminal-status ownership for one identity lives in exactly one place:
//!
//! * `content_work_items.status` is written by this worker
//!   (`claim_next`, `publish_ready`, `publish_failure`, recovery) and by
//!   admission when it must refuse to reopen a terminal identity;
//! * `translation_work_items` / `translation_requests` are the legacy
//!   caller-facing adapter. For an identity routed through global admission they
//!   only mirror the work status ([`linked_work`] +
//!   [`sync_linked_request_status`]) and never decide a terminal state
//!   themselves.
//!
//! Release detail translation uses global admission
//! ([`admit_release_detail_translation`]); Release feed/detail reads resolve its
//! projection through `postgres_content_overlay`.
//!
//! Reading polishing is admitted per audience: `smart_public` for public
//! readers and `smart_authorized` for authorized viewers, derived from the
//! published Release text alone (ADR 0022). A Release whose published text does
//! not state a change keeps only the established `smart` summary contract; a
//! public reader never falls back to that legacy variant.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::{
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
/// `model_profile` stamped on projections published for a translation attempt.
///
/// The projection's `model_profile` is display metadata (readers select the
/// newest row for the identity, not a model), so a stable profile keeps a
/// republished identity to exactly one `content_result_projections` row even
/// when a retry lands on a different routed model.
const PUBLICATION_MODEL_PROFILE: &str = "translation-publish";
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
    crate::content_source_identity::sha256_hex(&format!(
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

/// One result-projection write for an identity that already passed its
/// pipeline output contract.
struct ProjectionWrite<'a> {
    identity_id: &'a str,
    canonical_resource_type: &'a str,
    canonical_resource_id: &'a str,
    pipeline: &'a str,
    variant: &'a str,
    target_lang: &'a str,
    protocol_version: &'a str,
    model_profile: &'a str,
    source_hash: &'a str,
    work_item_id: &'a str,
    payload_json: &'a str,
    published_at: DateTime<Utc>,
}

/// Register the canonical work identity a projection hangs off.
async fn ensure_identity_registered(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &ContentWorkIdentity,
    now: DateTime<Utc>,
) -> Result<String> {
    let identity_id = content_identity_upgrade::identity_id_for(identity)?;
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
    .execute(&mut **tx)
    .await?;
    Ok(identity_id)
}

/// Link a work item into an identity.
///
/// `content_current_result_projections` references the `(identity, work item)`
/// member pair, so the membership must exist before the current projection is
/// written — including for work items admitted before the global runtime.
async fn link_identity_member(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    work_item_id: &str,
    identity_id: &str,
    identity: &ContentWorkIdentity,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO public.content_work_identity_members (work_item_id, identity_id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, linked_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) ON CONFLICT DO NOTHING",
    )
    .bind(work_item_id)
    .bind(identity_id)
    .bind(&identity.canonical_resource_type)
    .bind(&identity.canonical_resource_id)
    .bind(&identity.pipeline)
    .bind(&identity.variant)
    .bind(&identity.target_lang)
    .bind(&identity.source_hash)
    .bind(&identity.protocol_version)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Write the result projection plus the identity's current projection.
///
/// Both statements are keyed by the canonical identity, so republishing one
/// identity updates the existing rows instead of appending duplicates.
async fn write_projection(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    write: ProjectionWrite<'_>,
) -> Result<()> {
    let projection_id: String = sqlx::query_scalar(
        "INSERT INTO public.content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, active_work_item_id, payload_json, published_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10, $11, $12, $12) ON CONFLICT (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash) DO UPDATE SET work_item_id = excluded.work_item_id, active_work_item_id = excluded.active_work_item_id, payload_json = excluded.payload_json, published_at = excluded.published_at, updated_at = excluded.updated_at RETURNING id",
    )
    .bind(local_id::generate_local_id())
    .bind(write.canonical_resource_type)
    .bind(write.canonical_resource_id)
    .bind(write.pipeline)
    .bind(write.variant)
    .bind(write.target_lang)
    .bind(write.protocol_version)
    .bind(write.model_profile)
    .bind(write.source_hash)
    .bind(write.work_item_id)
    .bind(write.payload_json)
    .bind(write.published_at)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO public.content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES ($1, $2, $2, $3, $4, $5, $5) ON CONFLICT (identity_id) DO UPDATE SET work_item_id = excluded.work_item_id, active_work_item_id = excluded.active_work_item_id, source_projection_id = excluded.source_projection_id, payload_json = excluded.payload_json, published_at = excluded.published_at, updated_at = excluded.updated_at",
    )
    .bind(write.identity_id)
    .bind(write.work_item_id)
    .bind(&projection_id)
    .bind(write.payload_json)
    .bind(write.published_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// What the translation lane hands to [`publish_translation_result`].
///
/// The lane owns admission and attempt state and already ran
/// `content_processing::validate_output`; it owns neither the identity rows nor
/// the projection tables. `item` must be the same request item the read path
/// rebuilds from live content, because the projection is keyed by its
/// `source_hash_for_item` hash.
pub(crate) struct TranslationPublication<'a> {
    /// Canonical resource type (`release`).
    pub canonical_resource_type: &'a str,
    /// Pipeline that produced the payload (`translation`).
    pub pipeline: &'a str,
    /// The request item the projection identity is keyed by.
    pub item: &'a TranslationRequestItemInput,
    /// Identity hash the attempt was bound to. It must equal the canonical hash
    /// of `item`; a stale or mismatched hash is rejected instead of published.
    pub source_hash: &'a str,
    pub payload: &'a Value,
}

/// Why a publication was rejected before it touched the projection tables.
#[derive(Debug)]
pub(crate) enum PublicationError {
    /// The attempt's hash does not describe the content being published.
    SourceHashMismatch { reported: String, canonical: String },
}

impl std::fmt::Display for PublicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceHashMismatch {
                reported,
                canonical,
            } => write!(
                formatter,
                "translation publication source_hash {reported} does not match the canonical identity hash {canonical}"
            ),
        }
    }
}

impl std::error::Error for PublicationError {}

/// Publication-time fingerprint for the work item created to own a projection.
///
/// It identifies the publisher path that produced the row, not a model route:
/// the attempt happened on the translation lane and its routing is recorded
/// there.
fn publication_fingerprint(source_hash: &str) -> String {
    crate::content_source_identity::sha256_hex(&format!(
        "{GLOBAL_PROTOCOL_VERSION}\ntranslation-publication\n{source_hash}"
    ))
}

/// Return the work item that owns a projection, creating it when absent.
///
/// A projection requires a `content_work_items` row (foreign key and admin
/// joins), but the translation lane tracks its attempt in
/// `translation_work_items`. The created row records a completed publication
/// with `attempt_count = 0`, so no content attempt, provider admission or LLM
/// audit fact is fabricated for it.
async fn publication_work_item(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &ContentWorkIdentity,
    item: &TranslationRequestItemInput,
    now: DateTime<Utc>,
) -> Result<String> {
    let snapshot = serde_json::to_string(&json!({
        "source_blocks": item.source_blocks,
        "target_slots": item.target_slots,
        "evidence_snapshot_id": Value::Null,
        "evidence_status": "not_required",
    }))?;
    let token_estimate = i64::try_from(
        item.source_blocks
            .iter()
            .map(|block| block.text.chars().count())
            .sum::<usize>(),
    )
    .unwrap_or(i64::MAX);
    let inserted = sqlx::query_scalar::<_, String>(
        "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, token_estimate, attempt_count, evidence_status, finished_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'ready', 0, $12, 0, 'not_required', $13, $13, $13) ON CONFLICT (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile) DO NOTHING RETURNING id",
    )
    .bind(local_id::generate_local_id())
    .bind(&identity.canonical_resource_type)
    .bind(&identity.canonical_resource_id)
    .bind(&identity.pipeline)
    .bind(&identity.variant)
    .bind(&identity.target_lang)
    .bind(&identity.source_hash)
    .bind(&identity.protocol_version)
    .bind(PUBLICATION_MODEL_PROFILE)
    .bind(&snapshot)
    .bind(publication_fingerprint(&identity.source_hash))
    .bind(token_estimate)
    .bind(now)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(work_item_id) = inserted {
        return Ok(work_item_id);
    }
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.content_work_items WHERE canonical_resource_type = $1 AND canonical_resource_id = $2 AND pipeline = $3 AND variant = $4 AND target_lang = $5 AND source_hash = $6 AND protocol_version = $7 AND model_profile = $8 LIMIT 1",
    )
    .bind(&identity.canonical_resource_type)
    .bind(&identity.canonical_resource_id)
    .bind(&identity.pipeline)
    .bind(&identity.variant)
    .bind(&identity.target_lang)
    .bind(&identity.source_hash)
    .bind(&identity.protocol_version)
    .bind(PUBLICATION_MODEL_PROFILE)
    .fetch_one(&mut **tx)
    .await
    .context("load publication work item")
}

/// Publish the projection for one completed translation attempt.
///
/// This is the only writer of `content_result_projections` for translations:
/// the legacy enqueue path stays admission-only and never writes projections.
/// It publishes inside the caller's transaction so the attempt's terminal state
/// and its projection commit together — a stored `ready` attempt always has its
/// projection, and a failed commit leaves neither behind.
///
/// Publication is idempotent: the identity is keyed by the attempt's canonical
/// `source_hash`, so a retried publication updates the same rows instead of
/// appending duplicates.
pub(crate) async fn publish_translation_result(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    publication: TranslationPublication<'_>,
) -> Result<()> {
    let canonical_hash = crate::content_source_identity::source_hash_for_item(publication.item);
    if canonical_hash != publication.source_hash {
        return Err(anyhow::Error::new(PublicationError::SourceHashMismatch {
            reported: publication.source_hash.to_owned(),
            canonical: canonical_hash,
        }));
    }
    let identity = ContentWorkIdentity {
        canonical_resource_type: publication.canonical_resource_type.to_owned(),
        canonical_resource_id: publication.item.entity_id.clone(),
        pipeline: publication.pipeline.to_owned(),
        variant: publication.item.variant.clone(),
        target_lang: publication.item.target_lang.clone(),
        source_hash: canonical_hash,
        protocol_version: GLOBAL_PROTOCOL_VERSION.to_owned(),
    };
    let payload_json = publication.payload.to_string();
    let now = Utc::now();
    let identity_id = ensure_identity_registered(tx, &identity, now).await?;
    let work_item_id = publication_work_item(tx, &identity, publication.item, now).await?;
    link_identity_member(tx, &work_item_id, &identity_id, &identity, now).await?;
    write_projection(
        tx,
        ProjectionWrite {
            identity_id: &identity_id,
            canonical_resource_type: &identity.canonical_resource_type,
            canonical_resource_id: &identity.canonical_resource_id,
            pipeline: &identity.pipeline,
            variant: &identity.variant,
            target_lang: &identity.target_lang,
            protocol_version: &identity.protocol_version,
            model_profile: PUBLICATION_MODEL_PROFILE,
            source_hash: &identity.source_hash,
            work_item_id: &work_item_id,
            payload_json: &payload_json,
            published_at: now,
        },
    )
    .await?;
    info!(
        identity_id = %identity_id,
        variant = %identity.variant,
        source_hash = %identity.source_hash,
        work_item_id = %work_item_id,
        "translation projection published"
    );
    Ok(())
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
    ensure_identity_registered(&mut tx, &identity, now).await?;
    link_identity_member(&mut tx, &work.id, &identity_id, &identity, now).await?;
    let payload_json = output.to_string();
    write_projection(
        &mut tx,
        ProjectionWrite {
            identity_id: &identity_id,
            canonical_resource_type: &work.canonical_resource_type,
            canonical_resource_id: &work.canonical_resource_id,
            pipeline: &work.pipeline,
            variant: &work.variant,
            target_lang: &work.target_lang,
            protocol_version: &work.protocol_version,
            model_profile: &completion.model,
            source_hash: &work.source_hash,
            work_item_id: &work.id,
            payload_json: &payload_json,
            published_at: now,
        },
    )
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
                    summary: crate::translation_error::translation_error_summary(Some(
                        &failure.message,
                    )),
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
                        summary: crate::translation_error::translation_error_summary(Some(
                            &failure.message,
                        )),
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
                    summary: crate::translation_error::translation_error_summary(Some(
                        &error.to_string(),
                    )),
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

/// Source blocks shared by every polishing variant of one Release.
///
/// The blocks are the published Release text only: `metadata`, `title`,
/// `body_markdown` come from `repo_releases`, and the revision blocks carry the
/// identity tiebreak. No block can carry compare evidence.
fn release_polish_source_blocks(
    repo_identity: &str,
    tag_name: &str,
    title: &str,
    body: &str,
    observed_at: Option<&str>,
    tiebreak: &str,
) -> Vec<TranslationSourceBlock> {
    let mut source_blocks = vec![
        TranslationSourceBlock {
            slot: "metadata".to_owned(),
            text: format!("{repo_identity}\n{}", tag_name.trim()),
        },
        TranslationSourceBlock {
            slot: "title".to_owned(),
            text: title.to_owned(),
        },
    ];
    if !body.is_empty() {
        source_blocks.push(TranslationSourceBlock {
            slot: "body_markdown".to_owned(),
            text: body.to_owned(),
        });
    }
    if let Some(observed_at) = observed_at.filter(|value| !value.trim().is_empty()) {
        source_blocks.insert(
            0,
            TranslationSourceBlock {
                slot: "source_revision_tiebreak".to_owned(),
                text: tiebreak.to_owned(),
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
    source_blocks
}

/// Published-Release-text evidence named to the reading contract.
///
/// `validate_output` requires `reading_evidence_json` to declare readable
/// evidence. Only the published title/body is named here: this producer never
/// collects compare evidence, so nothing private can reach either audience
/// (ADR 0022).
fn published_release_text_evidence(
    release_id: i64,
) -> Vec<crate::translation_types::ReleaseReadingEvidence> {
    vec![crate::translation_types::ReleaseReadingEvidence {
        kind: "release_text".to_owned(),
        reference: release_id.to_string(),
        scope: Some("published title/body; compare evidence not used".to_owned()),
    }]
}

/// One polishing request item for one variant of a Release.
fn release_polish_item(
    release_id: i64,
    variant: &str,
    source_blocks: &[TranslationSourceBlock],
) -> TranslationRequestItemInput {
    let mut source_blocks = source_blocks.to_vec();
    let mut target_slots = vec!["title_zh".to_owned(), "summary_md".to_owned()];
    if variant != "smart" {
        // Reading variants answer with the structured `reading` slot; the output
        // contract derives title_zh/summary_md from it.
        target_slots.push("reading".to_owned());
        source_blocks.push(TranslationSourceBlock {
            slot: "reading_contract".to_owned(),
            text: "human_changes_direct; machine_materials_collapsible; no_source_section; no_version_repetition; no_author_attribution".to_owned(),
        });
        source_blocks.push(TranslationSourceBlock {
            slot: "reading_evidence_json".to_owned(),
            text: serde_json::to_string(&published_release_text_evidence(release_id))
                .unwrap_or_else(|_| "[]".to_owned()),
        });
        source_blocks.push(TranslationSourceBlock {
            slot: "reading_compare".to_owned(),
            text: "Published Release title/body is the only evidence; no code compare was used."
                .to_owned(),
        });
    }
    TranslationRequestItemInput {
        producer_ref: "sync.releases.polish".to_owned(),
        kind: "release_smart".to_owned(),
        variant: variant.to_owned(),
        entity_id: release_id.to_string(),
        target_lang: TARGET_LANG.to_owned(),
        max_wait_ms: 0,
        source_blocks,
        target_slots,
    }
}

/// Whether the published Release text already states a change a reader can see.
///
/// This gates the audience reading variants. They are built from the published
/// title/body alone, so a Release that publishes only its version, a changelog
/// link or machine metadata would be answered with `insufficient` although no
/// compare evidence was ever checked — the exact confusion the release-reading
/// evidence rules forbid. Such a Release keeps only the `smart` summary contract
/// until the compare-evidence producer exists.
///
/// This is intentionally stricter than `api::release_body_has_actionable_content`
/// and does not reuse it: that gate decides whether compare evidence should be
/// fetched and already counts a template shell like `See full changelog below.`
/// as actionable. Reading admission needs the stronger fact that the published
/// text alone grounds a change statement, because a variant admitted here can
/// never fall back to compare evidence.
fn published_release_states_a_change(title: &str, body: &str) -> bool {
    const LOW_VALUE_PREFIXES: [&str; 15] = [
        "http://",
        "https://",
        "完整变更日志",
        "full changelog",
        "see full changelog",
        "查看 commits",
        "view commits",
        "更新内容",
        "主要更新",
        "release notes",
        "what's changed",
        "release-identity:",
        "source-sha:",
        "commit:",
        "sha:",
    ];
    let title = title.trim();
    if title.is_empty() {
        return false;
    }
    let lower_title = title.to_ascii_lowercase();
    let normalized_title = lower_title.trim_start_matches('v').to_owned();
    let version_only = normalized_title
        .strip_prefix("release ")
        .or_else(|| normalized_title.strip_prefix("version "))
        .unwrap_or(&normalized_title)
        .trim_matches(|ch: char| matches!(ch, ':' | '：' | '-' | '_' | '(' | ')' | '[' | ']'))
        .trim_start_matches('v');
    let title_is_version_only = semver::Version::parse(version_only).is_ok();
    if !title_is_version_only
        && title.chars().count() >= 8
        && !LOW_VALUE_PREFIXES
            .iter()
            .any(|prefix| lower_title.starts_with(prefix))
    {
        // The title itself states the change; a body is not required.
        return true;
    }
    for raw_line in body.replace("\r\n", "\n").lines() {
        let line = raw_line
            .trim()
            .trim_start_matches(['-', '*', '>', '#'])
            .trim();
        if line.chars().count() < 8 {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if LOW_VALUE_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        {
            continue;
        }
        let normalized = lower
            .strip_prefix("release ")
            .or_else(|| lower.strip_prefix("version "))
            .unwrap_or(&lower)
            .trim()
            .trim_start_matches('v');
        if semver::Version::parse(normalized).is_ok() || normalized == normalized_title {
            continue;
        }
        return true;
    }
    false
}

/// Admit one polishing variant of one Release.
///
/// Returns 1 when a work item was created and 0 when an equivalent projection
/// or an active work item already covers the variant.
struct ReleaseRevision<'a> {
    observed_at: Option<&'a str>,
    tiebreak: &'a str,
}

async fn admit_polish_variant(
    pool: &PgPool,
    release_id: i64,
    variant: &str,
    item: &TranslationRequestItemInput,
    revision: ReleaseRevision<'_>,
    fingerprint: &str,
    token_estimate: i64,
) -> Result<usize> {
    let source_hash = crate::content_source_identity::source_hash_for_item(item);
    let identity = ContentWorkIdentity {
        canonical_resource_type: "release".to_owned(),
        canonical_resource_id: release_id.to_string(),
        pipeline: "polishing".to_owned(),
        variant: variant.to_owned(),
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
        return Ok(0);
    }
    let already_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM public.content_work_items WHERE canonical_resource_type = 'release' AND canonical_resource_id = $1 AND pipeline = 'polishing' AND variant = $2 AND target_lang = $3 AND protocol_version = $4 AND status IN ('queued', 'running', 'ready', 'deferred_provider', 'blocked_config'))",
    )
    .bind(release_id.to_string())
    .bind(variant)
    .bind(TARGET_LANG)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .fetch_one(pool)
    .await?;
    if already_active {
        return Ok(0);
    }
    let snapshot = serde_json::to_string(&json!({
        "source_blocks": item.source_blocks,
        "target_slots": item.target_slots,
        "source_revision": {
            "source_observed_at": revision.observed_at,
            "source_revision_tiebreak": revision.tiebreak,
        },
        "evidence_snapshot_id": Value::Null,
        "evidence_status": "not_required",
    }))?;
    let inserted = sqlx::query(
        // `cache_hit` stays out of the insert: the migrated schema declares it
        // boolean, the imported one bigint, and both default to a falsy value,
        // so only the column default is portable.
        "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, token_estimate, attempt_count, evidence_status, created_at, updated_at) VALUES ($1, 'release', $2, 'polishing', $3, $4, $5, $6, 'postgres-default', $7, $8, 'queued', 0, $9, 0, 'not_required', now(), now()) ON CONFLICT (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile) DO NOTHING",
    )
    .bind(local_id::generate_local_id())
    .bind(release_id.to_string())
    .bind(variant)
    .bind(TARGET_LANG)
    .bind(&source_hash)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .bind(&snapshot)
    .bind(fingerprint)
    .bind(token_estimate)
    .execute(pool)
    .await
    .context("admit release polishing work")?;
    if inserted.rows_affected() == 0 {
        // A terminal row for the same source hash cannot be re-admitted, and
        // the unique conflict is the only signal. Surface it instead of letting
        // the release look silently polished.
        let terminal: Option<String> = sqlx::query_scalar(
            "SELECT status FROM public.content_work_items WHERE canonical_resource_type = 'release' AND canonical_resource_id = $1 AND pipeline = 'polishing' AND variant = $2 AND target_lang = $3 AND protocol_version = $4 AND source_hash = $5 AND model_profile = 'postgres-default' LIMIT 1",
        )
        .bind(release_id.to_string())
        .bind(variant)
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
                variant,
                status = terminal.as_deref().unwrap_or_default(),
                "release polishing work stayed terminal; not re-admitted"
            );
        }
    }
    Ok(usize::try_from(inserted.rows_affected()).unwrap_or(0))
}

/// Admit Release polishing work that the stored Release source can satisfy.
///
/// Every Release keeps the established `smart` summary contract. When the
/// published Release text already states a change, the audience reading
/// variants are admitted beside it:
///
/// - `smart_public` for public readers,
/// - `smart_authorized` for authorized viewers.
///
/// Both are built from the published title/body only, so no work item here can
/// depend on private compare evidence (ADR 0022). A Release whose published
/// text does not state a change is not admitted as a reading variant: its
/// reading would need compare evidence this producer does not collect, and a
/// public projection must never fall back to the legacy `smart` row.
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
        let observed_at = crate::content_source_identity::authoritative_release_revision(
            updated_at.as_deref().unwrap_or_default(),
            detected_at.as_deref(),
        );
        let tiebreak = crate::content_source_identity::release_source_revision_content_tiebreak(&[
            html_url.as_str(),
            tag_name.as_str(),
            title.as_str(),
            body.as_str(),
        ]);
        let base_blocks = release_polish_source_blocks(
            &repo_identity,
            tag_name.as_str(),
            title.as_str(),
            body.as_str(),
            observed_at,
            tiebreak.as_str(),
        );
        let token_estimate =
            i64::try_from(body.chars().count() + title.chars().count()).unwrap_or(i64::MAX);
        let mut variants: Vec<&str> = vec!["smart"];
        if published_release_states_a_change(title.as_str(), body.as_str()) {
            variants.push(translations::RELEASE_READING_PUBLIC_VARIANT);
            variants.push(translations::RELEASE_READING_AUTHORIZED_VARIANT);
        }
        for variant in variants {
            if admitted >= POLISH_ADMISSION_CAP {
                break;
            }
            let item = release_polish_item(*release_id, variant, &base_blocks);
            admitted += admit_polish_variant(
                pool,
                *release_id,
                variant,
                &item,
                ReleaseRevision {
                    observed_at,
                    tiebreak: tiebreak.as_str(),
                },
                &fingerprint,
                token_estimate,
            )
            .await?;
        }
    }
    if admitted > 0 {
        info!(admitted, "global content polishing work admitted");
    }
    Ok(admitted)
}

// ---------------------------------------------------------------------------
// Release detail translation admission
// ---------------------------------------------------------------------------

/// What global admission did with one Release detail request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailAdmission {
    /// A current projection already covers this exact identity. The work item
    /// is `ready`, `cache_hit`, carries zero attempts, and no provider call can
    /// ever run for it (REQ-GTP-CACHE-HIT).
    CacheHit,
    /// New work for this identity was created and queued.
    Created,
    /// Existing work for this identity was reused with its own status.
    Reused,
}

/// Admitted Release detail translation work, as the HTTP adapter reports it.
#[derive(Debug, Clone)]
pub(crate) struct AdmittedDetailWork {
    pub work_item_id: String,
    /// Authoritative `content_work_items.status`. The caller-facing adapter
    /// mirrors this value and never derives a competing terminal status.
    pub status: String,
    pub outcome: DetailAdmission,
    /// Canonical source version. The adapter records it on its request row so
    /// the row describes the same identity the work item is keyed by.
    pub source_hash: String,
    /// Canonical source blocks and target slots. They come from the server-side
    /// Release row, never from client input, so a per-user request row cannot
    /// describe a different identity than the global work it links to.
    pub source_blocks_json: String,
    pub target_slots_json: String,
}

/// Canonical identity of one Release detail translation source version.
///
/// Admission and the read path derive identities from the same request item
/// (`postgres_content_overlay::release_detail_translation_item`), so this is a
/// lookup helper, not a second identity construction.
struct DetailIdentity {
    item: TranslationRequestItemInput,
    identity: ContentWorkIdentity,
    identity_id: String,
    snapshot_json: String,
}

fn detail_identity(
    origin: &crate::postgres_content_overlay::ReleaseDetailOrigin<'_>,
    producer_ref: &str,
) -> Result<DetailIdentity> {
    let item =
        crate::postgres_content_overlay::release_detail_translation_item(origin, producer_ref);
    let source_hash = content_processing::source_hash_for_item(&item);
    let identity = ContentWorkIdentity {
        canonical_resource_type: "release".to_owned(),
        canonical_resource_id: origin.release_id.to_string(),
        pipeline: "translation".to_owned(),
        variant: "detail".to_owned(),
        target_lang: TARGET_LANG.to_owned(),
        source_hash,
        protocol_version: GLOBAL_PROTOCOL_VERSION.to_owned(),
    };
    let identity_id = content_identity_upgrade::identity_id_for(&identity)?;
    let snapshot_json = serde_json::to_string(&json!({
        "source_blocks": item.source_blocks,
        "target_slots": item.target_slots,
        "source_revision": {
            "source_observed_at": origin.observed_at,
            "source_revision_tiebreak": item
                .source_blocks
                .iter()
                .find(|block| block.slot == "source_revision_tiebreak")
                .map(|block| block.text.clone()),
        },
        "evidence_snapshot_id": Value::Null,
        "evidence_status": "not_required",
    }))?;
    Ok(DetailIdentity {
        item,
        identity,
        identity_id,
        snapshot_json,
    })
}

/// Admit one Release detail translation through the global content model.
///
/// Ownership rule: **this function is the only code that admits Release detail
/// translation work, and the global content worker is the only writer of its
/// terminal status.** The HTTP adapter writes a per-user `translation_requests`
/// row, but that row is a view: `content_work_items.status` decides what the
/// caller sees, and `publish_failure`/`publish_ready` (worker) are the only
/// places a terminal `ready`/`failed` is written for a work item this function
/// did not retire at admission time.
///
/// Order of decisions, deliberately:
/// 1. a current projection for the exact identity → `cache_hit` with zero
///    attempts, no provider call (REQ-GTP-CACHE-HIT);
/// 2. otherwise insert-or-reuse one work item keyed by the canonical identity,
///    which makes concurrent duplicate admission idempotent (REQ-PGCP-008);
/// 3. `superseded` stays a strict terminal (ADR 0015): admission never reopens
///    it, and reports the real status so the adapter can redirect or surface
///    it instead of pretending new work started.
pub(crate) async fn admit_release_detail_translation(
    pool: &PgPool,
    release_id: i64,
    fallback_model: &str,
    producer_ref: &str,
) -> Result<AdmittedDetailWork> {
    let row = sqlx::query(
        "SELECT repo_id, html_url, tag_name, name, body, updated_at, detected_at FROM public.repo_releases WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_optional(pool)
    .await
    .context("load release source for detail admission")?
    .ok_or_else(|| anyhow::anyhow!("release {release_id} not found"))?;
    let repo_id: i64 = row.try_get("repo_id")?;
    let html_url: String = row.try_get("html_url")?;
    let tag_name: String = row.try_get("tag_name")?;
    let name: Option<String> = row.try_get("name")?;
    let body: Option<String> = row.try_get("body")?;
    let updated_at: Option<String> = row.try_get("updated_at").ok().flatten();
    let detected_at: Option<String> = row.try_get("detected_at").ok().flatten();
    let observed_at = content_processing::authoritative_release_revision(
        updated_at.as_deref().unwrap_or_default(),
        detected_at.as_deref(),
    );
    let detail = detail_identity(
        &crate::postgres_content_overlay::ReleaseDetailOrigin {
            release_id,
            html_url: &html_url,
            repo_id,
            tag_name: &tag_name,
            name: name.as_deref(),
            body: body.as_deref(),
            observed_at,
        },
        producer_ref,
    )?;
    let source_hash = detail.identity.source_hash.clone();
    let token_estimate = i64::try_from(
        detail
            .item
            .source_blocks
            .iter()
            .map(|block| block.text.chars().count())
            .sum::<usize>(),
    )
    .unwrap_or(i64::MAX);

    // Configuration validity only picks the initial status of *new* work; it
    // never rewrites existing work or a published projection.
    let (configuration_valid, fingerprint) =
        match postgres_translate::route_models_for_runtime(pool, fallback_model).await {
            Ok(models) if !models.is_empty() => (true, configuration_fingerprint(&models)),
            _ => (false, cache_hit_fingerprint()),
        };

    let mut tx = pool.begin().await.context("begin detail admission")?;
    let has_projection: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM public.content_current_result_projections WHERE identity_id = $1)",
    )
    .bind(&detail.identity_id)
    .fetch_one(&mut *tx)
    .await?;
    let initial_status = if has_projection {
        // The projection already satisfies this identity: the work item exists
        // to record the hit and the requester link, with zero attempts and no
        // provider call ever made for it (REQ-GTP-CACHE-HIT).
        "ready"
    } else if configuration_valid {
        "queued"
    } else {
        "blocked_config"
    };
    let new_work_id = local_id::generate_local_id();
    // One statement, so two simultaneous admissions of the same identity cannot
    // both create work; `xmax = 0` distinguishes the insert from the conflict
    // branch, which is what makes "admitted exactly once" observable.
    // `cache_hit` stays out of the insert: the migrated schema declares it
    // boolean, an imported snapshot declares it bigint, and both default to a
    // falsy value (see `migrations_pg/0016_normalize_flag_columns.sql`).
    let (work_item_id, status, attempt_count, inserted) =
        sqlx::query_as::<_, (String, String, i32, bool)>(
            r#"
        INSERT INTO public.content_work_items (
            id, canonical_resource_type, canonical_resource_id, pipeline, variant,
            target_lang, source_hash, protocol_version, model_profile,
            source_snapshot_json, configuration_fingerprint, status, priority,
            token_estimate, attempt_count, evidence_status, created_at, updated_at
        ) VALUES (
            $1, 'release', $2, 'translation', 'detail', $3, $4, $5, 'postgres-default',
            $6, $7, $8, 0, $9, 0, 'not_required', now(), now()
        )
        ON CONFLICT (
            canonical_resource_type, canonical_resource_id, pipeline, variant,
            target_lang, source_hash, protocol_version, model_profile
        ) DO UPDATE SET updated_at = public.content_work_items.updated_at
        RETURNING id, status, attempt_count::integer AS attempt_count, (xmax = 0) AS inserted
        "#,
        )
        .bind(&new_work_id)
        .bind(release_id.to_string())
        .bind(TARGET_LANG)
        .bind(&source_hash)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(&detail.snapshot_json)
        .bind(&fingerprint)
        .bind(initial_status)
        .bind(token_estimate)
        .fetch_one(&mut *tx)
        .await?;
    let outcome = match (inserted, has_projection) {
        (true, true) => DetailAdmission::CacheHit,
        (true, false) => DetailAdmission::Created,
        // A repeated cache hit sees the zero-attempt `ready` row the first hit
        // created. Any other retained row keeps its own status and attempts:
        // admission never relabels work it did not observe, and never reopens a
        // terminal identity (ADR 0015).
        (false, _) if status == "ready" && attempt_count == 0 => DetailAdmission::CacheHit,
        (false, _) => DetailAdmission::Reused,
    };
    if outcome == DetailAdmission::CacheHit {
        mark_cache_hit(&mut tx, &work_item_id).await?;
    }
    if outcome == DetailAdmission::Created {
        // Register the identity and its membership at admission, so the worker
        // publishes under a registered identity even when it races this call.
        ensure_identity_registered(&mut tx, &detail.identity, Utc::now()).await?;
        link_identity_member(
            &mut tx,
            &work_item_id,
            &detail.identity_id,
            &detail.identity,
            Utc::now(),
        )
        .await?;
    }
    sqlx::query(
        "INSERT INTO public.content_work_admission_events (id, work_item_id, event_type, source_hash, source_revision_json, producer_ref, reason_code, created_at) VALUES ($1, $2, $3, $4, $5, $6, 'source_current', now()) ON CONFLICT DO NOTHING",
    )
    .bind(local_id::generate_local_id())
    .bind(&work_item_id)
    .bind(if outcome == DetailAdmission::Created {
        "admission_accepted"
    } else {
        "admission_noop"
    })
    .bind(&source_hash)
    .bind(source_revision_json(&detail.snapshot_json))
    .bind(producer_ref)
    .execute(&mut *tx)
    .await?;
    tx.commit().await.context("commit detail admission")?;
    if outcome == DetailAdmission::Created {
        info!(
            work_item_id = %work_item_id,
            release_id,
            status = %status,
            "global release detail translation admitted"
        );
    } else {
        debug!(
            work_item_id = %work_item_id,
            release_id,
            status = %status,
            attempts = attempt_count,
            ?outcome,
            "release detail admission reused retained work"
        );
    }
    Ok(AdmittedDetailWork {
        work_item_id,
        status,
        outcome,
        source_hash,
        source_blocks_json: serde_json::to_string(&detail.item.source_blocks)
            .unwrap_or_else(|_| "[]".to_owned()),
        target_slots_json: serde_json::to_string(&detail.item.target_slots)
            .unwrap_or_else(|_| "[]".to_owned()),
    })
}

/// Fingerprint recorded on work whose identity was satisfied from a cache hit.
/// It records that fact; it is not a provider route the item ever used.
fn cache_hit_fingerprint() -> String {
    ai::sha256_hex(&format!("{GLOBAL_PROTOCOL_VERSION}\ncache_hit"))
}

/// Record that a work item was admitted without ever needing an attempt.
///
/// `cache_hit` is `boolean` on the migrated schema but `bigint` on an imported
/// snapshot (`migrations_pg/0016_normalize_flag_columns.sql` rewrites it), and
/// PostgreSQL will not coerce one to the other. Binding a Rust `bool` or an
/// integer therefore fails on one of the two shapes, so the column type decides
/// which literal is written.
async fn mark_cache_hit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    work_item_id: &str,
) -> Result<()> {
    let data_type: Option<String> = sqlx::query_scalar(
        "SELECT data_type FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'content_work_items' AND column_name = 'cache_hit'",
    )
    .fetch_optional(&mut **tx)
    .await?;
    let statement = if data_type.as_deref() == Some("boolean") {
        "UPDATE public.content_work_items SET cache_hit = TRUE WHERE id = $1"
    } else {
        "UPDATE public.content_work_items SET cache_hit = 1 WHERE id = $1"
    };
    sqlx::query(statement)
        .bind(work_item_id)
        .execute(&mut **tx)
        .await
        .with_context(|| format!("mark content cache hit for {work_item_id}"))?;
    Ok(())
}

/// Global work behind one caller-facing request row.
///
/// The link, not the request row, is authoritative: when a request is linked to
/// global work, its terminal status is `content_work_items.status`. `None` means
/// the row has no link (every kind this slice does not route), so the caller
/// keeps reading the row itself.
pub(crate) async fn linked_work(
    pool: &PgPool,
    request_id: &str,
    user_id: &str,
) -> Option<(String, String)> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT l.work_item_id, w.status FROM public.content_request_links l JOIN public.content_work_items w ON w.id = l.work_item_id WHERE l.request_id = $1 AND l.requester_id = $2 LIMIT 1",
    )
    .bind(request_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Mirror a terminal global status onto the requester's request row.
///
/// Only the global worker decides the terminal status; this propagates it to the
/// view so admin reads and legacy consumers observe the same fact. It never
/// writes a status the work item does not have, and never writes result text —
/// that stays a projection read.
pub(crate) async fn sync_linked_request_status(
    pool: &PgPool,
    request_id: &str,
    work_status: &str,
) -> Result<()> {
    let (view_status, result_status) = match work_status {
        "ready" => ("completed", Some("ready")),
        "running" => ("running", None),
        "queued" | "blocked_config" | "deferred_provider" => ("queued", None),
        _ => ("failed", Some("error")),
    };
    sqlx::query(
        "UPDATE public.translation_requests SET status = $2, result_status = $3, updated_at = now() WHERE id = $1 AND (status <> $2 OR result_status IS DISTINCT FROM $3)",
    )
    .bind(request_id)
    .bind(view_status)
    .bind(result_status)
    .execute(pool)
    .await
    .context("mirror global work status onto the request view")?;
    Ok(())
}

/// Register the requester link that makes a caller-facing request row a view
/// of global work instead of a second writer.
pub(crate) async fn register_detail_request_link(
    pool: &PgPool,
    request_id: &str,
    work_item_id: &str,
    user_id: &str,
    producer_ref: &str,
    delivery_mode: &str,
    response_status: Option<&str>,
) -> Result<()> {
    let authorization = json!({"user_id": user_id, "captured_at": Utc::now().to_rfc3339()});
    sqlx::query(
        "INSERT INTO public.content_request_links (id, request_id, work_item_id, requester_type, requester_id, authorization_snapshot_json, producer_ref, request_source, delivery_mode, response_status, created_at, updated_at) VALUES ($1, $2, $3, 'user', $4, $5, $6, 'api', $7, $8, now(), now()) ON CONFLICT (request_id) DO UPDATE SET work_item_id = EXCLUDED.work_item_id, response_status = EXCLUDED.response_status, updated_at = now()",
    )
    .bind(local_id::generate_local_id())
    .bind(request_id)
    .bind(work_item_id)
    .bind(user_id)
    .bind(authorization.to_string())
    .bind(producer_ref)
    .bind(delivery_mode)
    .bind(response_status)
    .execute(pool)
    .await
    .context("register content request link")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip the publication path against a real PostgreSQL cluster:
    /// a validated translation output becomes a readable projection, a retry
    /// updates rather than duplicates, a stale/foreign hash is refused, and the
    /// public detail read reports `ready`.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_translation_publication_publishes_once_and_is_readable() -> anyhow::Result<()>
    {
        use sqlx::PgPool;

        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("content_publish_{}", local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;

        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY, repo_id bigint NOT NULL, html_url text NOT NULL, tag_name text NOT NULL, name text, body text);
                INSERT INTO public.repo_releases(release_id, repo_id, html_url, tag_name, name, body) VALUES (42, 7, 'https://github.com/acme/demo/releases/tag/v1', 'v1', 'v1', 'Release notes body');
                CREATE TABLE public.content_work_items(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_snapshot_json text NOT NULL, configuration_fingerprint text NOT NULL, status text NOT NULL, priority integer NOT NULL DEFAULT 0, cache_hit boolean NOT NULL DEFAULT false, token_estimate integer NOT NULL DEFAULT 0, attempt_count integer NOT NULL DEFAULT 0, evidence_status text NOT NULL DEFAULT 'not_required', finished_at timestamptz, created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile));
                CREATE TABLE public.content_work_identities(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version));
                CREATE TABLE public.content_work_identity_members(work_item_id text PRIMARY KEY, identity_id text NOT NULL, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, linked_at text NOT NULL, UNIQUE (identity_id, work_item_id));
                CREATE TABLE public.content_result_projections(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_hash text NOT NULL, work_item_id text NOT NULL, active_work_item_id text, payload_json text NOT NULL, published_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash));
                CREATE TABLE public.content_current_result_projections(identity_id text PRIMARY KEY, work_item_id text NOT NULL, active_work_item_id text, source_projection_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL);
                CREATE TABLE public.content_processing_control(id integer PRIMARY KEY, mode text NOT NULL, switch_token text, updated_at timestamptz NOT NULL);
                INSERT INTO public.content_processing_control(id, mode, updated_at) VALUES (1, 'global', now());
                CREATE TABLE public.content_identity_upgrade_control(id bigint PRIMARY KEY, status text NOT NULL, phase text NOT NULL);
                INSERT INTO public.content_identity_upgrade_control(id, status, phase) VALUES (1, 'completed', 'complete');
                "#,
            )
            .execute(&pool)
            .await?;

            let item = release_item("42", "v1", "v1", Some("Release notes body"));
            let source_hash = crate::content_source_identity::source_hash_for_item(&item);
            let payload = json!({"title_zh": "项目 v1 发布", "body_md": "发布说明正文"});

            // 1. Publish on completion.
            let mut tx = pool.begin().await?;
            publish_translation_result(
                &mut tx,
                TranslationPublication {
                    canonical_resource_type: "release",
                    pipeline: "translation",
                    item: &item,
                    source_hash: &source_hash,
                    payload: &payload,
                },
            )
            .await?;
            tx.commit().await?;

            let (projections, currents, work_items): (i64, i64, i64) = sqlx::query_as(
                "SELECT (SELECT COUNT(*) FROM public.content_result_projections), (SELECT COUNT(*) FROM public.content_current_result_projections), (SELECT COUNT(*) FROM public.content_work_items)",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(projections, 1, "one result projection");
            assert_eq!(currents, 1, "one current projection");
            assert_eq!(work_items, 1, "one work item owns the projection");

            // A publication creates no provider attempt or LLM audit fact.
            let attempt_rows: i64 = sqlx::query_scalar(
                "SELECT attempt_count::bigint FROM public.content_work_items LIMIT 1",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(attempt_rows, 0, "publication never fabricates an attempt");

            // 2. Idempotent retry: same identity republishes in place.
            let mut tx = pool.begin().await?;
            publish_translation_result(
                &mut tx,
                TranslationPublication {
                    canonical_resource_type: "release",
                    pipeline: "translation",
                    item: &item,
                    source_hash: &source_hash,
                    payload: &payload,
                },
            )
            .await?;
            tx.commit().await?;
            let projections_after: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_result_projections")
                    .fetch_one(&pool)
                    .await?;
            let currents_after: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_current_result_projections")
                    .fetch_one(&pool)
                    .await?;
            let work_items_after: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_work_items")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(projections_after, 1, "a retry must not add a projection");
            assert_eq!(currents_after, 1, "a retry must not add a current projection");
            assert_eq!(work_items_after, 1, "a retry must not add a work item");

            // 3. A source hash that does not describe the item is refused and
            // writes nothing.
            let mut foreign = source_hash.clone();
            foreign.push('0');
            let mut tx = pool.begin().await?;
            let error = publish_translation_result(
                &mut tx,
                TranslationPublication {
                    canonical_resource_type: "release",
                    pipeline: "translation",
                    item: &item,
                    source_hash: &foreign,
                    payload: &payload,
                },
            )
            .await
            .expect_err("a mismatched source hash must be refused");
            tx.rollback().await?;
            assert!(
                error.to_string().contains("does not match the canonical identity"),
                "unexpected refusal reason: {error}"
            );
            let projections_after_mismatch: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_result_projections")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(projections_after_mismatch, 1, "a refused publish writes nothing");

            // 4. The public detail read resolves the published row as ready.
            let overlay = crate::postgres_content_overlay::OverlayState::load(&pool).await;
            assert!(overlay.is_global());
            let source = crate::postgres_content_overlay::ReleaseSource {
                release_id: 42,
                repo_full_name: "acme/demo",
                repo_identity: "acme/demo",
                tag_name: "v1",
                name: Some("v1"),
                body: Some("Release notes body"),
                previous_tag_name: None,
            };
            let projection =
                crate::postgres_content_overlay::public_release_translation(&pool, overlay, &source)
                    .await?
                    .expect("the published translation is readable");
            assert_eq!(projection.status, "ready");
            assert_eq!(projection.title().as_deref(), Some("项目 v1 发布"));
            assert_eq!(projection.translation_summary().as_deref(), Some("发布说明正文"));
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

    fn release_item(
        entity_id: &str,
        tag: &str,
        title: &str,
        body: Option<&str>,
    ) -> TranslationRequestItemInput {
        crate::postgres_content_overlay::identity_item(
            "release_detail",
            "detail",
            entity_id,
            format!("acme/demo\n{tag}"),
            title.to_owned(),
            body.map(str::to_owned),
        )
    }

    /// The writer and the reader must derive one identity from one release
    /// text; a divergence would publish a projection no read path can resolve.
    #[test]
    fn writer_and_reader_derive_the_same_release_identity() {
        let item = release_item("42", "v1.2.0", "v1.2.0 release", Some("## Notes\n- item"));
        let written = crate::content_source_identity::source_hash_for_item(&item);
        let reader = crate::postgres_content_overlay::ReleaseSource {
            release_id: 42,
            repo_full_name: "acme/demo",
            repo_identity: "acme/demo",
            tag_name: "v1.2.0",
            name: Some("v1.2.0 release"),
            body: Some("## Notes\n- item"),
            previous_tag_name: None,
        };
        assert_eq!(
            reader.identity_hash("release_detail", "detail", reader.body),
            written
        );
    }

    /// The stored body keeps whatever line endings it was ingested with; both
    /// sides normalize them, so the identity must not depend on them.
    #[test]
    fn release_identity_is_stable_across_line_endings() {
        let lf = release_item("7", "v1", "one", Some("a\nb"));
        let crlf = release_item("7", "v1", "one", Some("a\r\nb"));
        assert_eq!(
            crate::content_source_identity::source_hash_for_item(&lf),
            crate::content_source_identity::source_hash_for_item(&crlf)
        );
    }

    /// A title-less release falls back to its tag on both sides.
    #[test]
    fn release_identity_uses_the_tag_when_the_name_is_blank() {
        let with_blank_name = crate::postgres_content_overlay::identity_item(
            "release_detail",
            "detail",
            "9",
            "acme/demo\nv2".to_owned(),
            "v2".to_owned(),
            None,
        );
        let item = release_item("9", "v2", "v2", None);
        assert_eq!(
            crate::content_source_identity::source_hash_for_item(&with_blank_name),
            crate::content_source_identity::source_hash_for_item(&item)
        );
    }

    /// Admission only offers reading variants to Releases whose published text
    /// already states a change; the reading contract can then answer without
    /// compare evidence, which this producer never collects.
    #[test]
    fn reading_variants_require_published_change_text() {
        assert!(published_release_states_a_change(
            "v1.2.0",
            "- 每轮最多处理 10 批，此前为 5 批"
        ));
        assert!(published_release_states_a_change(
            "CLI 4.8.0 新增 GitHub 注解支持",
            ""
        ));
        assert!(!published_release_states_a_change("v1.2.0", ""));
        assert!(!published_release_states_a_change(
            "v1.2.0",
            "See full changelog below."
        ));
        assert!(!published_release_states_a_change(
            "v3.11.3",
            "Release-Identity: b14681353067f0eb014d24b92871e898ce0bd3759b7ca2a52495f04287823ecd\nSource-SHA: d83dd15ae37130c6b424e9e6106e42638198ea6"
        ));
    }

    /// Reading variants must carry published-text evidence only: nothing in the
    /// prompt may let an audience projection derive from private compare facts.
    #[test]
    fn reading_variants_carry_published_text_evidence_only() {
        let blocks = release_polish_source_blocks(
            "acme/demo",
            "v1.2.0",
            "v1.2.0",
            "- 每轮最多处理 10 批",
            Some("2026-11-24T00:00:00Z"),
            "7",
        );
        for variant in [
            translations::RELEASE_READING_PUBLIC_VARIANT,
            translations::RELEASE_READING_AUTHORIZED_VARIANT,
        ] {
            let item = release_polish_item(42, variant, &blocks);
            assert_eq!(item.variant, variant);
            assert!(item.target_slots.iter().any(|slot| slot == "reading"));
            assert!(
                !item
                    .source_blocks
                    .iter()
                    .any(|block| block.slot == "reading_compare"
                        && block.text.contains("base_sha")),
                "reading variants must not embed compare scope"
            );
            let evidence = item
                .source_blocks
                .iter()
                .find(|block| block.slot == "reading_evidence_json")
                .map(|block| block.text.as_str())
                .expect("reading variants declare their evidence");
            assert!(evidence.contains("release_text"), "{evidence}");
            assert!(!evidence.contains("github_compare"), "{evidence}");
        }
        // The legacy summary contract keeps its published slots and no reading
        // evidence block at all.
        let smart = release_polish_item(42, "smart", &blocks);
        assert_eq!(smart.variant, "smart");
        assert!(!smart.target_slots.iter().any(|slot| slot == "reading"));
        assert!(
            !smart
                .source_blocks
                .iter()
                .any(|block| block.slot == "reading_evidence_json")
        );
    }

    /// Each audience variant is its own global identity, and no reader may fall
    /// back across audiences: public resolves only `smart_public`.
    #[test]
    fn audience_variants_are_distinct_identities() {
        let blocks = release_polish_source_blocks(
            "acme/demo",
            "v1.2.0",
            "v1.2.0",
            "- 每轮最多处理 10 批",
            None,
            "7",
        );
        let public = release_polish_item(42, translations::RELEASE_READING_PUBLIC_VARIANT, &blocks);
        let authorized = release_polish_item(
            42,
            translations::RELEASE_READING_AUTHORIZED_VARIANT,
            &blocks,
        );
        let smart = release_polish_item(42, "smart", &blocks);
        let hashes = [&public, &authorized, &smart]
            .map(crate::content_source_identity::source_hash_for_item);
        assert_ne!(hashes[0], hashes[1]);
        assert_ne!(hashes[0], hashes[2]);
        assert_ne!(hashes[1], hashes[2]);
        assert_eq!(
            crate::postgres_content_overlay::ReleaseAudience::Public.polishing_variants(),
            &[translations::RELEASE_READING_PUBLIC_VARIANT]
        );
    }

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
                // Per-prompt truncation budget: the length-recovery fixture is
                // truncated exactly once, then answered normally on retry.
                let truncated = std::sync::Arc::new(std::sync::Mutex::new(
                    std::collections::HashSet::<String>::new(),
                ));
                move |Json(payload): Json<Value>| {
                    let calls = calls.clone();
                    let truncated = truncated.clone();
                    async move {
                        let model = payload["model"].as_str().unwrap_or_default().to_owned();
                        let user = payload["messages"]
                            .as_array()
                            .and_then(|messages| {
                                messages
                                    .iter()
                                    .rev()
                                    .find(|message| message["role"] == "user")
                            })
                            .and_then(|message| message["content"].as_str())
                            .unwrap_or_default()
                            .to_owned();
                        let request: Value = serde_json::from_str(&user).unwrap_or(Value::Null);
                        let is_reading = request["target_slots"]
                            .as_array()
                            .is_some_and(|slots| slots.iter().any(|slot| slot == "reading"));
                        // The length-recovery fixture names itself in its source body
                        // and is truncated only on its first provider call, so the
                        // recovery call is answered normally.
                        let truncate = user.contains("truncate once")
                            && truncated.lock().is_ok_and(|mut seen| seen.insert(user.clone()));
                        let index = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let finish_reason = if truncate { "length" } else { "stop" };
                        let content = if is_reading {
                            serde_json::to_string(&json!({
                                "title_zh": "项目 v1 发布",
                                "summary_md": "- 每轮最多处理 10 批",
                                "reading": {
                                    "version": "v1",
                                    "outcome": "changes",
                                    "title": "新增批处理重试",
                                    "changes": [{
                                        "text": "每轮最多处理 10 批",
                                        "versions": ["v1"]
                                    }],
                                    "machine_materials": [],
                                    "evidence": []
                                }
                            }))
                            .unwrap_or_default()
                        } else {
                            "{\"title_zh\":\"é¡¹ç›® v1 å‘å¸ƒ\",\"summary_md\":\"- æ–°å¢žäº†å¹¶å‘Œå¤„ç†\"}"
                                .to_owned()
                        };
                        Json(json!({
                            "id": format!("provider-call-{index}"),
                            "model": model,
                            "choices": [{
                                "message": {"content": content},
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
                INSERT INTO public.repo_releases(release_id, repo_id, html_url, tag_name, name, body, updated_at) VALUES (42, 7, 'https://github.com/acme/demo/releases/tag/v1', 'v1', 'v1', 'Batching now processes up to 10 batches per round', '2026-11-24T00:00:00Z');
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
            assert_eq!(
                admitted, 3,
                "smart plus the two audience reading variants are admitted"
            );
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
            // The audience reading variants are admitted beside the legacy
            // contract and run through the same worker: public first, then
            // authorized.
            let public_claim = claim_next(&pool, "m1")
                .await?
                .expect("public reading work is claimable");
            assert_eq!(public_claim.work.pipeline, "polishing");
            assert_eq!(
                public_claim.work.variant,
                translations::RELEASE_READING_PUBLIC_VARIANT
            );
            execute(&pool, &http, &ai, &public_claim).await?;
            let authorized_claim = claim_next(&pool, "m1")
                .await?
                .expect("authorized reading work is claimable");
            assert_eq!(
                authorized_claim.work.variant,
                translations::RELEASE_READING_AUTHORIZED_VARIANT
            );
            execute(&pool, &http, &ai, &authorized_claim).await?;
            for published in [&public_claim, &authorized_claim] {
                let published_status: String = sqlx::query_scalar(
                    "SELECT status FROM public.content_work_items WHERE id = $1",
                )
                .bind(&published.work.id)
                .fetch_one(&pool)
                .await?;
                assert_eq!(published_status, "ready");
            }

            let (status, attempt_count): (String, i32) = sqlx::query_as(
                "SELECT status, attempt_count FROM public.content_work_items WHERE id = $1",
            )
            .bind(&claim.work.id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(status, "ready");
            assert_eq!(attempt_count, 1);

            let payload: String = sqlx::query_scalar(
                "SELECT payload_json FROM public.content_current_result_projections p JOIN public.content_work_identities i ON i.id = p.identity_id WHERE i.variant = 'smart'",
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
                admitted_text, 3,
                "admission tolerates the imported bigint cache_hit column for every variant"
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
                    {"slot": "body_markdown", "text": "Release notes body truncate once"}
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

    /// Fixture shared by the admission tests: the content tables plus the
    /// release source an admission resolves an identity from.
    async fn create_admission_schema(pool: &sqlx::PgPool) -> anyhow::Result<()> {
        sqlx::raw_sql(
            r#"
            CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY, repo_id bigint NOT NULL, html_url text NOT NULL, tag_name text NOT NULL, name text, body text, updated_at text, detected_at text);
            CREATE TABLE public.content_work_items(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_snapshot_json text NOT NULL, configuration_fingerprint text NOT NULL, status text NOT NULL, priority integer NOT NULL DEFAULT 0, cache_hit boolean NOT NULL DEFAULT false, token_estimate integer NOT NULL DEFAULT 0, attempt_count integer NOT NULL DEFAULT 0, evidence_status text NOT NULL DEFAULT 'not_required', next_retry_at text, retry_expires_at text, retry_after_at text, failure_class text, lease_owner text, lease_expires_at text, started_at text, finished_at text, cancelled_at text, supersedes_work_item_id text, batch_id text, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile));
            CREATE TABLE public.content_work_identities(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version));
            CREATE TABLE public.content_work_identity_members(work_item_id text PRIMARY KEY, identity_id text NOT NULL, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, linked_at text NOT NULL, UNIQUE (identity_id, work_item_id));
            CREATE TABLE public.content_result_projections(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_hash text NOT NULL, work_item_id text NOT NULL, active_work_item_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash));
            CREATE TABLE public.content_current_result_projections(identity_id text PRIMARY KEY, work_item_id text NOT NULL, active_work_item_id text, source_projection_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL);
            CREATE TABLE public.content_work_admission_events(id text PRIMARY KEY, work_item_id text NOT NULL, event_type text NOT NULL, replaced_by_work_item_id text, source_hash text NOT NULL, source_revision_json text NOT NULL DEFAULT '{}', producer_ref text NOT NULL DEFAULT '', requester_id text, reason_code text NOT NULL DEFAULT '', created_at text NOT NULL, UNIQUE (work_item_id, event_type, source_hash));
            CREATE TABLE public.content_attempt_events(id text PRIMARY KEY, work_item_id text NOT NULL, attempt_no bigint NOT NULL, trigger text NOT NULL, event_type text NOT NULL, result_status text, error_code text, error_summary text, failure_class text, retry_eligible bigint NOT NULL DEFAULT 0, next_retry_at text, duration_ms bigint, token_count bigint, cost_microunits bigint, created_at text NOT NULL, configuration_snapshot_json text, route_snapshot_json text, configuration_fingerprint text, UNIQUE (work_item_id, attempt_no, event_type));
            CREATE TABLE public.content_request_links(id text PRIMARY KEY, request_id text NOT NULL UNIQUE, work_item_id text NOT NULL, requester_type text NOT NULL, requester_id text, authorization_snapshot_json text NOT NULL, producer_ref text NOT NULL, request_source text NOT NULL, delivery_mode text NOT NULL, response_status text, created_at text NOT NULL, updated_at text NOT NULL);
            CREATE TABLE public.content_processing_control(id integer PRIMARY KEY, mode text NOT NULL, switch_token text, updated_at text NOT NULL);
            INSERT INTO public.content_processing_control(id, mode, updated_at) VALUES (1, 'global', now()::text);
            CREATE TABLE public.content_identity_upgrade_control(id bigint PRIMARY KEY, status text NOT NULL, phase text NOT NULL);
            INSERT INTO public.content_identity_upgrade_control(id, status, phase) VALUES (1, 'completed', 'complete');
            CREATE TABLE public.admin_runtime_settings(id integer PRIMARY KEY, llm_models_json text NOT NULL);
            INSERT INTO public.admin_runtime_settings(id, llm_models_json) VALUES (1, '["m1"]');
            INSERT INTO public.repo_releases(release_id, repo_id, html_url, tag_name, name, body, updated_at)
              VALUES (42, 7, 'https://github.com/acme/demo/releases/tag/v1', 'v1', 'v1', 'Release notes body', '2026-11-24T00:00:00Z');
            CREATE TABLE public.translation_work_items(id text PRIMARY KEY, dedupe_key text UNIQUE NOT NULL, scope_user_id text NOT NULL, kind text NOT NULL, variant text NOT NULL, entity_id text NOT NULL, target_lang text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_hash text NOT NULL, source_blocks_json text NOT NULL, target_slots_json text NOT NULL, token_estimate integer NOT NULL, deadline_at text NOT NULL, status text NOT NULL, result_status text, title_zh text, summary_md text, body_md text, error_text text, cache_hit bigint NOT NULL DEFAULT 0, batch_id text, retry_count bigint NOT NULL DEFAULT 0, created_at text NOT NULL, started_at text, finished_at text, updated_at text NOT NULL);
            CREATE TABLE public.translation_requests(id text PRIMARY KEY, mode text NOT NULL, source text NOT NULL, request_origin text NOT NULL, requested_by text, scope_user_id text NOT NULL, producer_ref text NOT NULL, kind text NOT NULL, variant text NOT NULL, entity_id text NOT NULL, target_lang text NOT NULL, max_wait_ms integer NOT NULL, source_hash text NOT NULL, source_blocks_json text NOT NULL, target_slots_json text NOT NULL, work_item_id text, status text NOT NULL, result_status text, title_zh text, summary_md text, body_md text, error_text text, created_at text NOT NULL, started_at text, finished_at text, updated_at text NOT NULL, UNIQUE (mode, request_origin, scope_user_id, producer_ref, kind, variant, entity_id, target_lang, source_hash));
            "#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn create_admission_database()
    -> anyhow::Result<(sqlx::PgPool, sqlx::PgPool, String, url::Url)> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("content_admission_{}", local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        create_admission_schema(&pool).await?;
        Ok((admin, pool, database, url))
    }

    async fn drop_admission_database(
        admin: &sqlx::PgPool,
        pool: sqlx::PgPool,
        database: &str,
    ) -> anyhow::Result<()> {
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(admin)
            .await?;
        admin.close().await;
        Ok(())
    }

    /// Two different users asking for the same Release detail share exactly one
    /// work item, and each requester gets its own link.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_detail_admission_shares_one_work_item_between_requesters()
    -> anyhow::Result<()> {
        let (admin, pool, database, _) = create_admission_database().await?;
        let result = async {
            let first = admit_release_detail_translation(&pool, 42, "m1", "user-a:detail").await?;
            assert_eq!(first.outcome, DetailAdmission::Created);
            let second = admit_release_detail_translation(&pool, 42, "m1", "user-b:detail").await?;
            assert_eq!(second.outcome, DetailAdmission::Reused);
            assert_eq!(
                second.work_item_id, first.work_item_id,
                "one canonical identity has one work item"
            );
            let work_items: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_work_items")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(work_items, 1);
            let events: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_work_admission_events WHERE event_type = 'admission_noop'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(events, 1, "the repeat admission records its own fact");

            register_detail_request_link(
                &pool,
                "request-a",
                &first.work_item_id,
                "user-a",
                "user-a:detail",
                "async",
                Some("queued"),
            )
            .await?;
            register_detail_request_link(
                &pool,
                "request-b",
                &second.work_item_id,
                "user-b",
                "user-b:detail",
                "async",
                Some("queued"),
            )
            .await?;
            let links: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_request_links WHERE work_item_id = $1",
            )
            .bind(&first.work_item_id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(links, 2, "both requesters are linked to the shared work");
            Ok::<_, anyhow::Error>(())
        }
        .await;
        drop_admission_database(&admin, pool, &database).await?;
        result
    }

    /// A current projection answers the request with zero attempts: the work
    /// item is `ready`, `cache_hit`, and never claimable.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_detail_admission_cache_hit_creates_zero_attempts() -> anyhow::Result<()> {
        let (admin, pool, database, _) = create_admission_database().await?;
        let result = async {
            let first = admit_release_detail_translation(&pool, 42, "m1", "seed").await?;
            assert_eq!(first.outcome, DetailAdmission::Created);

            // Publish a current projection for the exact identity.
            let row = sqlx::query(
                "SELECT repo_id, html_url, tag_name, name, body, updated_at, detected_at FROM public.repo_releases WHERE release_id = 42",
            )
            .fetch_one(&pool)
            .await?;
            let html_url: String = row.try_get("html_url")?;
            let repo_id: i64 = row.try_get("repo_id")?;
            let tag_name: String = row.try_get("tag_name")?;
            let name: Option<String> = row.try_get("name")?;
            let body: Option<String> = row.try_get("body")?;
            let observed_at: Option<String> = row.try_get("updated_at")?;
            let detail = detail_identity(
                &crate::postgres_content_overlay::ReleaseDetailOrigin {
                    release_id: 42,
                    html_url: &html_url,
                    repo_id,
                    tag_name: &tag_name,
                    name: name.as_deref(),
                    body: body.as_deref(),
                    observed_at: observed_at.as_deref(),
                },
                "seed",
            )?;
            let mut tx = pool.begin().await?;
            ensure_identity_registered(&mut tx, &detail.identity, Utc::now()).await?;
            link_identity_member(
                &mut tx,
                &first.work_item_id,
                &detail.identity_id,
                &detail.identity,
                Utc::now(),
            )
            .await?;
            write_projection(
                &mut tx,
                ProjectionWrite {
                    identity_id: &detail.identity_id,
                    canonical_resource_type: "release",
                    canonical_resource_id: "42",
                    pipeline: "translation",
                    variant: "detail",
                    target_lang: TARGET_LANG,
                    protocol_version: GLOBAL_PROTOCOL_VERSION,
                    model_profile: "m1",
                    source_hash: &detail.identity.source_hash,
                    work_item_id: &first.work_item_id,
                    payload_json: r#"{"title_zh":"项目 v1","body_md":"发布说明"}"#,
                    published_at: Utc::now(),
                },
            )
            .await?;
            sqlx::query("UPDATE public.content_work_items SET status = 'ready' WHERE id = $1")
                .bind(&first.work_item_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;

            // A new source version with the same text is the same identity; a
            // second release row must not be needed to observe the hit.
            sqlx::query("DELETE FROM public.content_work_items WHERE id = $1")
                .bind(&first.work_item_id)
                .execute(&pool)
                .await?;
            let hit = admit_release_detail_translation(&pool, 42, "m1", "user-b:detail").await?;
            assert_eq!(hit.outcome, DetailAdmission::CacheHit);
            assert_eq!(hit.status, "ready");
            let zero_attempts: i64 = sqlx::query_scalar(
                "SELECT attempt_count::bigint FROM public.content_work_items WHERE id = $1",
            )
            .bind(&hit.work_item_id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(zero_attempts, 0, "a cache hit never fabricates an attempt");
            let cache_hit: bool = sqlx::query_scalar(
                "SELECT cache_hit FROM public.content_work_items WHERE id = $1",
            )
            .bind(&hit.work_item_id)
            .fetch_one(&pool)
            .await?;
            assert!(cache_hit, "the work item records the real cache hit");
            let attempt_events: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_attempt_events WHERE work_item_id = $1",
            )
            .bind(&hit.work_item_id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(attempt_events, 0);
            Ok::<_, anyhow::Error>(())
        }
        .await;
        drop_admission_database(&admin, pool, &database).await?;
        result
    }

    /// Concurrent duplicate admission of one identity creates exactly one work
    /// item and one accepted admission fact.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_detail_admission_is_idempotent_under_concurrency() -> anyhow::Result<()> {
        let (admin, pool, database, _) = create_admission_database().await?;
        let result = async {
            let mut handles = Vec::with_capacity(6);
            for index in 0..6 {
                let pool = pool.clone();
                handles.push(tokio::spawn(async move {
                    admit_release_detail_translation(
                        &pool,
                        42,
                        "m1",
                        format!("user-{index}:detail").as_str(),
                    )
                    .await
                }));
            }
            let mut created = 0;
            let mut work_item_ids = std::collections::HashSet::new();
            for handle in handles {
                let admitted = handle.await??;
                if admitted.outcome == DetailAdmission::Created {
                    created += 1;
                }
                work_item_ids.insert(admitted.work_item_id);
            }
            assert_eq!(created, 1, "exactly one concurrent admission creates work");
            assert_eq!(work_item_ids.len(), 1, "every admission shares one work item");
            let work_items: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_work_items")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(work_items, 1);
            let accepted: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_work_admission_events WHERE event_type = 'admission_accepted'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(accepted, 1);
            Ok::<_, anyhow::Error>(())
        }
        .await;
        drop_admission_database(&admin, pool, &database).await?;
        result
    }

    /// The admission source item and the detail reader must agree on identity;
    /// a divergence would publish a projection no read path resolves.
    #[test]
    fn admission_item_matches_the_detail_reader_identity() {
        let item = crate::postgres_content_overlay::release_detail_translation_item(
            &crate::postgres_content_overlay::ReleaseDetailOrigin {
                release_id: 42,
                html_url: "https://github.com/acme/demo/releases/tag/v1",
                repo_id: 7,
                tag_name: "v1",
                name: Some("v1"),
                body: Some("Release notes body"),
                observed_at: Some("2026-11-24T00:00:00Z"),
            },
            "release_detail:42",
        );
        let reader = crate::postgres_content_overlay::ReleaseSource {
            release_id: 42,
            repo_full_name: "acme/demo",
            repo_identity: "acme/demo",
            tag_name: "v1",
            name: Some("v1"),
            body: Some("Release notes body"),
            previous_tag_name: None,
        };
        assert_eq!(
            content_processing::source_hash_for_item(&item),
            reader.identity_hash("release_detail", "detail", reader.body)
        );
    }
}
