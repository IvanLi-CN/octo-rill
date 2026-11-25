//! Global content-processing overlay for the PostgreSQL read paths.
//!
//! The server reads translations from the legacy `public.ai_translations`
//! surface unless `public.content_processing_control.mode = 'global'`. In
//! global mode the translated/detail/smart payloads come from the
//! content-processing projections instead. This module owns the mode switch,
//! the content-work identity hashes, and the projection selection rules ported
//! from `content_processing::read_global_resource` (exact identity, active work
//! item, and source-hash mismatch retention) plus the polishing projection
//! ordering used by release reading (`api.rs`, public release page).
//!
//! Imported production state keeps `mode = 'global'`, so the read paths must
//! resolve payloads from `public.content_current_result_projections`; the
//! `ai_translations` rows only cover a small fraction of the same releases.

use std::collections::HashMap;

use serde_json::{Value, json};
use sqlx::PgPool;
use tracing::warn;

use crate::{
    api,
    content_identity_upgrade::{self, ContentWorkIdentity},
    content_processing::GLOBAL_PROTOCOL_VERSION,
    translations::{self, TranslationRequestItemInput, TranslationSourceBlock},
};

const TARGET_LANG: &str = "zh-CN";

/// Work statuses that still represent in-flight processing. The legacy read
/// paths show retained text for these instead of the raw status.
const ACTIVE_WORK_STATUSES: [&str; 4] =
    ["queued", "running", "deferred_provider", "blocked_config"];

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentMode {
    Legacy,
    Global,
}

/// Read `public.content_processing_control.mode`.
///
/// Every failure (missing table, missing row, unexpected value) falls back to
/// [`ContentMode::Legacy`] with a warning: the legacy read paths are the
/// behaviour the runtime had before the content-processing cutover, so an
/// unreadable control row must never hide translations.
pub(crate) async fn current_mode(pool: &PgPool) -> ContentMode {
    match sqlx::query_scalar::<_, String>(
        "SELECT mode FROM public.content_processing_control WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some(mode)) => match mode.as_str() {
            "global" => ContentMode::Global,
            // `rollback_freeze` already serves the legacy read paths.
            "legacy" | "rollback_freeze" => ContentMode::Legacy,
            other => {
                warn!(
                    mode = other,
                    "unknown content processing mode; reading legacy translations"
                );
                ContentMode::Legacy
            }
        },
        Ok(None) => ContentMode::Legacy,
        Err(error) => {
            warn!(
                %error,
                "failed to read the content processing mode; reading legacy translations"
            );
            ContentMode::Legacy
        }
    }
}

/// Per-request overlay state: the mode plus whether the work-identity upgrade
/// has completed. Reading both once avoids repeating the control lookups for
/// every feed row and projection slot.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OverlayState {
    mode: ContentMode,
    identity_upgrade_complete: bool,
}

impl OverlayState {
    pub(crate) async fn load(pool: &PgPool) -> Self {
        let mode = current_mode(pool).await;
        let identity_upgrade_complete = match mode {
            ContentMode::Global => identity_upgrade_complete(pool).await.unwrap_or_else(|error| {
                warn!(
                    %error,
                    "failed to read the content identity upgrade state; assuming current projections"
                );
                true
            }),
            ContentMode::Legacy => false,
        };
        Self {
            mode,
            identity_upgrade_complete,
        }
    }

    pub(crate) const fn is_global(self) -> bool {
        matches!(self.mode, ContentMode::Global)
    }
}

async fn identity_upgrade_complete(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let state: Option<(String, String)> = sqlx::query_as(
        "SELECT status, phase FROM public.content_identity_upgrade_control WHERE id = 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(state.is_some_and(|(status, phase)| status == "completed" && phase == "complete"))
}

// ---------------------------------------------------------------------------
// Projections
// ---------------------------------------------------------------------------

/// One selected content-processing result: the work status plus the projection
/// payload that the feed/detail response shapes already understand.
#[derive(Debug, Clone)]
pub(crate) struct ContentProjection {
    pub status: String,
    pub payload: Value,
}

impl ContentProjection {
    pub(crate) fn title(&self) -> Option<String> {
        payload_string(&self.payload, "title_zh")
    }

    /// Summary field order used by the feed/detail translation slots.
    pub(crate) fn translation_summary(&self) -> Option<String> {
        payload_string(&self.payload, "body_md")
            .or_else(|| payload_string(&self.payload, "summary_md"))
    }

    /// Summary field order used by the smart (polishing) slot.
    pub(crate) fn smart_summary(&self) -> Option<String> {
        payload_string(&self.payload, "summary_md")
            .or_else(|| payload_string(&self.payload, "body_md"))
    }

    pub(crate) fn error_text(&self) -> Option<String> {
        payload_string(&self.payload, "error_detail")
            .or_else(|| payload_string(&self.payload, "error_summary"))
            .or_else(|| payload_string(&self.payload, "error"))
    }

    pub(crate) fn has_text(&self) -> bool {
        self.title().is_some_and(|title| !title.trim().is_empty())
            || self
                .translation_summary()
                .is_some_and(|summary| !summary.trim().is_empty())
    }

    /// Display status for a translation slot: finished work that already
    /// carries text stays visible as `ready`, in-flight work without text is
    /// reported as `missing`.
    pub(crate) fn translation_status(&self) -> String {
        if ACTIVE_WORK_STATUSES.contains(&self.status.as_str()) {
            if self.has_text() {
                "ready".to_owned()
            } else {
                "missing".to_owned()
            }
        } else {
            self.status.clone()
        }
    }

    /// Display status for the smart slot. Release reading keeps in-flight work
    /// statuses visible (`keep_active`); announcements degrade them to
    /// `ready`/`missing` from the retained text.
    pub(crate) fn smart_status(&self, keep_active: bool) -> String {
        let (outcome, reading_incompatible) = reading_state(&self.payload);
        if self.status == "ready" {
            if outcome.as_deref() == Some("insufficient") {
                return "insufficient".to_owned();
            }
            if reading_incompatible {
                return "incompatible".to_owned();
            }
        }
        if keep_active && ACTIVE_WORK_STATUSES.contains(&self.status.as_str()) {
            return self.status.clone();
        }
        self.translation_status()
    }
}

fn payload_string(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn parse_payload(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!({}))
}

/// Decode `reading` out of a polishing payload: `(outcome, incompatible)`.
pub(crate) fn reading_state(payload: &Value) -> (Option<String>, bool) {
    let Some(raw) = payload.get("reading") else {
        return (None, false);
    };
    let Ok(mut reading) = serde_json::from_value::<translations::ReleaseReadingResult>(raw.clone())
    else {
        return (None, true);
    };
    if reading.normalize_references().is_err() || reading.validate().is_err() {
        return (None, true);
    }
    (Some(reading.outcome), false)
}

// ---------------------------------------------------------------------------
// Identity hashes
// ---------------------------------------------------------------------------

fn normalize_body(body: &str) -> String {
    body.replace("\r\n", "\n")
}

/// Repository identity the legacy content-processing writer used for a
/// release.
///
/// The writer derives it from the release `html_url`
/// (`api::resolve_release_full_name`), falling back to `unknown/{repo_id}`
/// when the URL cannot be parsed. That fallback is reachable and common: a tag
/// containing `/` (for example `@astrojs/vue@7.1.3-beta.0`) stays unescaped in
/// the release URL, so the path has six segments instead of five and
/// `parse_repo_full_name_from_release_url` returns `None`. Readers must rebuild
/// the same identity, otherwise every projection written for such a release is
/// unreachable.
pub(crate) fn release_repo_identity(html_url: &str, repo_id: i64) -> String {
    crate::release_links::parse_repo_full_name_from_release_url(html_url)
        .unwrap_or_else(|| format!("unknown/{repo_id}"))
}

/// The translation request item a content-work identity is keyed by.
///
/// Projection identities are the canonical `source_hash_for_item` hash of this
/// item. The reader rebuilds it from live release/announcement text; the single
/// projection writer must derive it the same way, so both go through here
/// instead of maintaining two hash constructions.
///
/// Line endings are normalized here rather than by each caller: the stored body
/// keeps whatever an ingest wrote, and an identity that changed with `\r\n`
/// would make a published projection unreadable.
pub(crate) fn identity_item(
    kind: &str,
    variant: &str,
    entity_id: &str,
    metadata: String,
    title: String,
    body: Option<String>,
) -> TranslationRequestItemInput {
    let body = body
        .map(|value| value.replace("\r\n", "\n"))
        .filter(|value| !value.trim().is_empty());
    let has_body = body.is_some();
    let mut source_blocks = vec![
        TranslationSourceBlock {
            slot: "metadata".to_owned(),
            text: metadata,
        },
        TranslationSourceBlock {
            slot: "title".to_owned(),
            text: title,
        },
    ];
    if let Some(body) = body {
        source_blocks.push(TranslationSourceBlock {
            slot: "body_markdown".to_owned(),
            text: body,
        });
    }
    let target_slots = if kind.ends_with("_detail") || variant == "detail" {
        if has_body {
            vec!["title_zh".to_owned(), "body_md".to_owned()]
        } else {
            vec!["title_zh".to_owned()]
        }
    } else {
        vec!["title_zh".to_owned(), "summary_md".to_owned()]
    };
    TranslationRequestItemInput {
        producer_ref: String::new(),
        kind: kind.to_owned(),
        variant: variant.to_owned(),
        entity_id: entity_id.to_owned(),
        target_lang: TARGET_LANG.to_owned(),
        max_wait_ms: 0,
        source_blocks,
        target_slots,
    }
}

fn global_source_hash_from_fields(
    kind: &str,
    variant: &str,
    entity_id: &str,
    metadata: String,
    title: String,
    body: Option<String>,
) -> String {
    crate::content_processing::source_hash_for_item(&identity_item(
        kind, variant, entity_id, metadata, title, body,
    ))
}

/// Release-side resources for one projection lookup.
pub(crate) struct ReleaseSource<'a> {
    pub release_id: i64,
    /// Repository name used for the legacy `ai_translations` display hashes.
    /// The feed assembly compares those hashes against the visible-repository
    /// name, so they must keep using that name.
    pub repo_full_name: &'a str,
    /// Repository name used for content-processing identity hashes. It must be
    /// the writer identity from [`release_repo_identity`].
    pub repo_identity: &'a str,
    pub tag_name: &'a str,
    pub name: Option<&'a str>,
    pub body: Option<&'a str>,
    pub previous_tag_name: Option<&'a str>,
}

impl ReleaseSource<'_> {
    fn entity_id(&self) -> String {
        self.release_id.to_string()
    }

    fn title(&self) -> String {
        self.name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(self.tag_name)
            .to_owned()
    }

    /// Truncated feed body, as the feed translation cache hash uses it.
    fn feed_body(&self) -> Option<String> {
        api::release_feed_body(self.body)
    }

    /// Identity hash the read path looks a projection up by. The single
    /// projection writer must reproduce it for a published row to be readable.
    pub(crate) fn identity_hash(&self, kind: &str, variant: &str, body: Option<&str>) -> String {
        global_source_hash_from_fields(
            kind,
            variant,
            &self.entity_id(),
            format!("{}\n{}", self.repo_identity, self.tag_name),
            self.title(),
            body.map(normalize_body),
        )
    }
}

/// Announcement-side resources for one projection lookup.
pub(crate) struct AnnouncementSource<'a> {
    pub discussion_key: &'a str,
    pub repo_full_name: &'a str,
    pub discussion_number: i64,
    pub title: &'a str,
    pub body: Option<&'a str>,
}

impl AnnouncementSource<'_> {
    fn identity_hash(&self, kind: &str, variant: &str, body: Option<&str>) -> String {
        global_source_hash_from_fields(
            kind,
            variant,
            self.discussion_key,
            api::announcement_translation_metadata_text(
                self.repo_full_name,
                Some(self.discussion_number),
            ),
            self.title.to_owned(),
            body.map(normalize_body),
        )
    }
}

// ---------------------------------------------------------------------------
// Projection reads
// ---------------------------------------------------------------------------

/// Read one projection with the legacy `read_global_resource` rules:
///
/// * the identity has a current projection → `ready` with that payload;
/// * otherwise the newest work item for the identity decides the status and a
///   retained projection from another source hash provides the payload
///   (`active_work_item_id` first, then any published row);
/// * `failed` work that retained a payload is reported as `ready`.
///
/// When the work-identity upgrade has not completed, the legacy
/// `content_result_projections` tables are the source of truth instead.
pub(crate) async fn read_projection(
    pool: &PgPool,
    state: OverlayState,
    resource_type: &str,
    resource_id: &str,
    pipeline: &str,
    variant: &str,
    expected_source_hash: &str,
) -> Result<Option<ContentProjection>, sqlx::Error> {
    if state.identity_upgrade_complete {
        identity_projection(
            pool,
            resource_type,
            resource_id,
            pipeline,
            variant,
            expected_source_hash,
        )
        .await
    } else {
        legacy_projection(
            pool,
            resource_type,
            resource_id,
            pipeline,
            variant,
            expected_source_hash,
        )
        .await
    }
}

/// Read the first matching projection across alternative variants.
pub(crate) async fn read_first_projection(
    pool: &PgPool,
    state: OverlayState,
    resource_type: &str,
    resource_id: &str,
    pipeline: &str,
    variants: &[(&str, String)],
) -> Result<Option<ContentProjection>, sqlx::Error> {
    for (variant, source_hash) in variants {
        if let Some(projection) = read_projection(
            pool,
            state,
            resource_type,
            resource_id,
            pipeline,
            variant,
            source_hash,
        )
        .await?
        {
            return Ok(Some(projection));
        }
    }
    Ok(None)
}

/// Read the projection for the exact source hash only, without the retained
/// payload fallback.
///
/// Public Release surfaces use this: a translation derived from another source
/// hash describes different release text, so it must not be presented as the
/// current translation of a public release.
pub(crate) async fn read_exact_projection(
    pool: &PgPool,
    state: OverlayState,
    resource_type: &str,
    resource_id: &str,
    pipeline: &str,
    variant: &str,
    expected_source_hash: &str,
) -> Result<Option<ContentProjection>, sqlx::Error> {
    if state.identity_upgrade_complete {
        let key = ContentWorkIdentity {
            canonical_resource_type: resource_type.to_owned(),
            canonical_resource_id: resource_id.to_owned(),
            pipeline: pipeline.to_owned(),
            variant: variant.to_owned(),
            target_lang: TARGET_LANG.to_owned(),
            source_hash: expected_source_hash.to_owned(),
            protocol_version: GLOBAL_PROTOCOL_VERSION.to_owned(),
        };
        let identity_id = content_identity_upgrade::identity_id_for(&key)
            .expect("content work identity keys are serializable");
        let payload = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM public.content_current_result_projections WHERE identity_id = $1",
        )
        .bind(&identity_id)
        .fetch_optional(pool)
        .await?;
        return Ok(payload.map(|raw| ContentProjection {
            status: "ready".to_owned(),
            payload: parse_payload(&raw),
        }));
    }
    let payload = sqlx::query_scalar::<_, String>(
        r#"
        SELECT payload_json
        FROM public.content_result_projections
        WHERE canonical_resource_type = $1
          AND canonical_resource_id = $2
          AND pipeline = $3
          AND variant = $4
          AND target_lang = $5
          AND protocol_version = $6
          AND source_hash = $7
        ORDER BY updated_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(pipeline)
    .bind(variant)
    .bind(TARGET_LANG)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .bind(expected_source_hash)
    .fetch_optional(pool)
    .await?;
    Ok(payload.map(|raw| ContentProjection {
        status: "ready".to_owned(),
        payload: parse_payload(&raw),
    }))
}

struct IdentityWorkRow {
    id: String,
    status: String,
}

async fn identity_projection(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    pipeline: &str,
    variant: &str,
    expected_source_hash: &str,
) -> Result<Option<ContentProjection>, sqlx::Error> {
    let key = ContentWorkIdentity {
        canonical_resource_type: resource_type.to_owned(),
        canonical_resource_id: resource_id.to_owned(),
        pipeline: pipeline.to_owned(),
        variant: variant.to_owned(),
        target_lang: TARGET_LANG.to_owned(),
        source_hash: expected_source_hash.to_owned(),
        protocol_version: GLOBAL_PROTOCOL_VERSION.to_owned(),
    };
    let identity_id = content_identity_upgrade::identity_id_for(&key)
        .expect("content work identity keys are serializable");
    let exact = sqlx::query_scalar::<_, String>(
        "SELECT payload_json FROM public.content_current_result_projections WHERE identity_id = $1",
    )
    .bind(&identity_id)
    .fetch_optional(pool)
    .await?;
    if let Some(raw) = exact {
        return Ok(Some(ContentProjection {
            status: "ready".to_owned(),
            payload: parse_payload(&raw),
        }));
    }
    let Some(work) = identity_work(pool, &identity_id).await? else {
        return Ok(None);
    };
    let retained = sqlx::query_scalar::<_, String>(
        r#"
        SELECT p.payload_json
        FROM public.content_current_result_projections p
        JOIN public.content_work_identities i ON i.id = p.identity_id
        WHERE p.active_work_item_id = $1
          AND i.canonical_resource_type = $2
          AND i.canonical_resource_id = $3
          AND i.pipeline = $4
          AND i.variant = $5
          AND i.target_lang = $6
          AND i.protocol_version = $7
          AND i.source_hash <> $8
        ORDER BY p.published_at DESC, i.source_hash DESC
        LIMIT 1
        "#,
    )
    .bind(&work.id)
    .bind(resource_type)
    .bind(resource_id)
    .bind(pipeline)
    .bind(variant)
    .bind(TARGET_LANG)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .bind(expected_source_hash)
    .fetch_optional(pool)
    .await?;
    let payload = retained.map(|raw| parse_payload(&raw));
    if work.status == "failed" && payload.as_ref().is_some_and(Value::is_object) {
        return Ok(Some(ContentProjection {
            status: "ready".to_owned(),
            payload: payload.unwrap_or_else(|| json!({})),
        }));
    }
    Ok(Some(ContentProjection {
        status: work.status,
        payload: payload.unwrap_or_else(|| json!({})),
    }))
}

/// Newest work item behind a content-work identity: the identity's projected
/// work item first, then the member with the most relevant status.
async fn identity_work(
    pool: &PgPool,
    identity_id: &str,
) -> Result<Option<IdentityWorkRow>, sqlx::Error> {
    if let Some(work_item_id) = sqlx::query_scalar::<_, String>(
        "SELECT work_item_id FROM public.content_current_result_projections WHERE identity_id = $1",
    )
    .bind(identity_id)
    .fetch_optional(pool)
    .await?
        && let Some(status) = sqlx::query_scalar::<_, String>(
            "SELECT status FROM public.content_work_items WHERE id = $1",
        )
        .bind(&work_item_id)
        .fetch_optional(pool)
        .await?
    {
        return Ok(Some(IdentityWorkRow {
            id: work_item_id,
            status,
        }));
    }
    let row = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT w.id, w.status
        FROM public.content_work_identity_members m
        JOIN public.content_work_items w ON w.id = m.work_item_id
        WHERE m.identity_id = $1
        ORDER BY CASE w.status
          WHEN 'queued' THEN 0
          WHEN 'running' THEN 1
          WHEN 'deferred_provider' THEN 2
          WHEN 'blocked_config' THEN 3
          WHEN 'failed' THEN 4
          WHEN 'ready' THEN 5
          ELSE 6
        END, w.attempt_count DESC, w.updated_at DESC, w.id DESC
        LIMIT 1
        "#,
    )
    .bind(identity_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, status)| IdentityWorkRow { id, status }))
}

/// Legacy read path used while the work-identity upgrade is incomplete.
async fn legacy_projection(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    pipeline: &str,
    variant: &str,
    expected_source_hash: &str,
) -> Result<Option<ContentProjection>, sqlx::Error> {
    let work = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT id, status
        FROM public.content_work_items
        WHERE canonical_resource_type = $1
          AND canonical_resource_id = $2
          AND pipeline = $3
          AND variant = $4
          AND target_lang = $5
          AND source_hash = $6
          AND protocol_version = $7
        ORDER BY CASE status
          WHEN 'ready' THEN 0
          WHEN 'queued' THEN 1
          WHEN 'running' THEN 2
          WHEN 'deferred_provider' THEN 3
          WHEN 'blocked_config' THEN 4
          ELSE 5
        END, updated_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(pipeline)
    .bind(variant)
    .bind(TARGET_LANG)
    .bind(expected_source_hash)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .fetch_optional(pool)
    .await?;
    let status = work
        .as_ref()
        .map(|(_, status)| status.clone())
        .unwrap_or_else(|| "ready".to_owned());
    let exact = sqlx::query_scalar::<_, String>(
        r#"
        SELECT payload_json
        FROM public.content_result_projections
        WHERE canonical_resource_type = $1
          AND canonical_resource_id = $2
          AND pipeline = $3
          AND variant = $4
          AND target_lang = $5
          AND protocol_version = $6
          AND source_hash = $7
        ORDER BY updated_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(pipeline)
    .bind(variant)
    .bind(TARGET_LANG)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .bind(expected_source_hash)
    .fetch_optional(pool)
    .await?;
    let has_work = work.is_some();
    let retained = match (exact, work) {
        (Some(payload), _) => Some(payload),
        (None, Some((current_id, _))) => {
            let active = sqlx::query_scalar::<_, String>(
                r#"
                SELECT p.payload_json
                FROM public.content_result_projections p
                JOIN public.content_work_items w ON w.id = p.work_item_id
                WHERE p.canonical_resource_type = $1
                  AND p.canonical_resource_id = $2
                  AND p.pipeline = $3
                  AND p.variant = $4
                  AND p.target_lang = $5
                  AND p.protocol_version = $6
                  AND p.active_work_item_id = $7
                ORDER BY p.updated_at DESC, p.id DESC
                LIMIT 1
                "#,
            )
            .bind(resource_type)
            .bind(resource_id)
            .bind(pipeline)
            .bind(variant)
            .bind(TARGET_LANG)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(&current_id)
            .fetch_optional(pool)
            .await?;
            match active {
                Some(payload) => Some(payload),
                None => {
                    sqlx::query_scalar::<_, String>(
                        r#"
                    SELECT p.payload_json
                    FROM public.content_result_projections p
                    JOIN public.content_work_items w ON w.id = p.work_item_id
                    WHERE p.canonical_resource_type = $1
                      AND p.canonical_resource_id = $2
                      AND p.pipeline = $3
                      AND p.variant = $4
                      AND p.target_lang = $5
                      AND p.protocol_version = $6
                      AND p.source_hash <> $7
                      AND w.status = 'ready'
                    ORDER BY p.updated_at DESC, p.id DESC
                    LIMIT 1
                    "#,
                    )
                    .bind(resource_type)
                    .bind(resource_id)
                    .bind(pipeline)
                    .bind(variant)
                    .bind(TARGET_LANG)
                    .bind(GLOBAL_PROTOCOL_VERSION)
                    .bind(expected_source_hash)
                    .fetch_optional(pool)
                    .await?
                }
            }
        }
        (None, None) => None,
    };
    let Some(payload) = retained else {
        // Legacy `read_global_resource` keeps the work status when the work
        // item exists but no projection was published yet (without the
        // payload-driven status upgrade); only the absence of both work and
        // payload reports nothing.
        if has_work {
            return Ok(Some(ContentProjection {
                status,
                payload: json!({}),
            }));
        }
        return Ok(None);
    };
    let status = if matches!(
        status.as_str(),
        "failed" | "cancelled" | "superseded" | "not_applicable" | "blocked_config"
    ) {
        "ready".to_owned()
    } else {
        status
    };
    Ok(Some(ContentProjection {
        status,
        payload: parse_payload(&payload),
    }))
}

// ---------------------------------------------------------------------------
// Polishing projections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReleaseAudience {
    Public,
    Authorized,
}

impl ReleaseAudience {
    /// Polishing variants that may serve this audience.
    ///
    /// Public output never inherits the authorized (or legacy `smart`) variant,
    /// which can be derived from private compare evidence. Authorized viewers
    /// may also read the pre-cutover `smart` projections, matching the legacy
    /// `read_global_release_smart_with_legacy_fallback` rule.
    pub(crate) const fn polishing_variants(self) -> &'static [&'static str] {
        match self {
            Self::Public => &[translations::RELEASE_READING_PUBLIC_VARIANT],
            Self::Authorized => &[translations::RELEASE_READING_AUTHORIZED_VARIANT, "smart"],
        }
    }
}

/// Resolve which polishing variant the user may read for a repository.
pub(crate) async fn release_audience(
    pool: &PgPool,
    user_id: &str,
    repo_id: i64,
) -> Result<ReleaseAudience, sqlx::Error> {
    let public_release = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
          SELECT 1
          FROM public.user_release_visible_repos
          WHERE user_id = $1 AND repo_id = $2 AND COALESCE(is_private::text, '0') IN ('0', 'false')
        ) OR EXISTS(
          SELECT 1
          FROM public.public_repo_release_usage
          WHERE repo_id = $2
            AND access_kind IN ('github_public', 'private_owner_published')
            AND last_sync_status <> 'inaccessible'
        )
        "#,
    )
    .bind(user_id)
    .bind(repo_id)
    .fetch_one(pool)
    .await?;
    Ok(if public_release {
        ReleaseAudience::Public
    } else {
        ReleaseAudience::Authorized
    })
}

/// Newest polishing projection for a resource, following the release-reading
/// ordering (`published_at DESC`, then identity source hash).
///
/// The reading variants are keyed by the evidence snapshot the work item was
/// admitted from, so the read path cannot rebuild their source hash; the
/// projection for the newest identity of the variant is the one that describes
/// the current release text.
pub(crate) async fn read_latest_polishing_projection(
    pool: &PgPool,
    state: OverlayState,
    resource_type: &str,
    resource_id: &str,
    variants: &[&str],
) -> Result<Option<ContentProjection>, sqlx::Error> {
    for variant in variants {
        let payload = if state.identity_upgrade_complete {
            sqlx::query_scalar::<_, String>(
                r#"
                SELECT p.payload_json
                FROM public.content_current_result_projections p
                JOIN public.content_work_identities i ON i.id = p.identity_id
                WHERE i.canonical_resource_type = $1
                  AND i.canonical_resource_id = $2
                  AND i.pipeline = 'polishing'
                  AND i.variant = $3
                  AND i.target_lang = $4
                  AND i.protocol_version = $5
                ORDER BY p.published_at DESC, i.source_hash DESC
                LIMIT 1
                "#,
            )
            .bind(resource_type)
            .bind(resource_id)
            .bind(*variant)
            .bind(TARGET_LANG)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                r#"
                SELECT payload_json
                FROM public.content_result_projections
                WHERE canonical_resource_type = $1
                  AND canonical_resource_id = $2
                  AND pipeline = 'polishing'
                  AND variant = $3
                  AND target_lang = $4
                  AND protocol_version = $5
                ORDER BY updated_at DESC, id DESC
                LIMIT 1
                "#,
            )
            .bind(resource_type)
            .bind(resource_id)
            .bind(*variant)
            .bind(TARGET_LANG)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .fetch_optional(pool)
            .await?
        };
        if let Some(raw) = payload {
            return Ok(Some(ContentProjection {
                status: "ready".to_owned(),
                payload: parse_payload(&raw),
            }));
        }
        if let Some(status) =
            latest_polishing_work_status(pool, resource_type, resource_id, variant).await?
        {
            return Ok(Some(ContentProjection {
                status,
                payload: json!({}),
            }));
        }
    }
    Ok(None)
}

async fn latest_polishing_work_status(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    variant: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT status
        FROM public.content_work_items
        WHERE canonical_resource_type = $1
          AND canonical_resource_id = $2
          AND pipeline = 'polishing'
          AND variant = $3
          AND target_lang = $4
          AND protocol_version = $5
        ORDER BY CASE status
          WHEN 'ready' THEN 0
          WHEN 'queued' THEN 1
          WHEN 'running' THEN 2
          WHEN 'deferred_provider' THEN 3
          WHEN 'blocked_config' THEN 4
          WHEN 'failed' THEN 5
          ELSE 6
        END, updated_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(variant)
    .bind(TARGET_LANG)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .fetch_optional(pool)
    .await
}

// ---------------------------------------------------------------------------
// Feed / detail composite reads
// ---------------------------------------------------------------------------

/// Feed payloads plus the translation-cache hashes the feed assembly compares
/// against. The hashes intentionally stay the legacy `ai_translations` hashes:
/// they only gate which retained payload is displayed.
#[derive(Debug, Clone)]
pub(crate) struct FeedProcessing {
    pub translation_source_hash: String,
    pub detail_source_hash: String,
    pub smart_source_hash: String,
    pub translation: Option<ContentProjection>,
    pub detail: Option<ContentProjection>,
    pub smart: Option<ContentProjection>,
}

#[derive(Debug, Clone)]
pub(crate) struct DetailProcessing {
    pub translation: Option<ContentProjection>,
    pub smart: Option<ContentProjection>,
}

/// Feed projection for a release row: the truncated feed translation, the
/// detail translation, and the audience polishing reading.
pub(crate) async fn release_feed_processing(
    pool: &PgPool,
    state: OverlayState,
    source: &ReleaseSource<'_>,
    audience: ReleaseAudience,
) -> Result<FeedProcessing, sqlx::Error> {
    let entity_id = source.entity_id();
    let title = source.title();
    let feed_body = source.feed_body();
    let translation = read_first_projection(
        pool,
        state,
        "release",
        &entity_id,
        "translation",
        &[
            (
                "feed_body",
                source.identity_hash("release_summary", "feed_body", source.body),
            ),
            (
                "summary",
                source.identity_hash("release_summary", "summary", source.body),
            ),
        ],
    )
    .await?;
    let detail = read_projection(
        pool,
        state,
        "release",
        &entity_id,
        "translation",
        "detail",
        &source.identity_hash("release_detail", "detail", source.body),
    )
    .await?;
    let smart = read_latest_polishing_projection(
        pool,
        state,
        "release",
        &entity_id,
        audience.polishing_variants(),
    )
    .await?;
    Ok(FeedProcessing {
        translation_source_hash: api::release_feed_translation_source_hash(
            source.repo_full_name,
            &title,
            feed_body.as_deref(),
        ),
        detail_source_hash: api::release_detail_source_hash(
            source.repo_full_name,
            &title,
            source.body.unwrap_or(""),
        ),
        smart_source_hash: translations::release_smart_feed_source_hash(
            &entity_id,
            source.repo_full_name,
            &title,
            feed_body.as_deref(),
            source.tag_name,
            source.previous_tag_name,
        ),
        translation,
        detail,
        smart,
    })
}

/// Feed projection for an announcement row.
pub(crate) async fn announcement_feed_processing(
    pool: &PgPool,
    state: OverlayState,
    source: &AnnouncementSource<'_>,
) -> Result<FeedProcessing, sqlx::Error> {
    let key = source.discussion_key;
    let feed_body = api::release_feed_body(source.body);
    let translation = read_first_projection(
        pool,
        state,
        "announcement",
        key,
        "translation",
        &[
            (
                "feed_body",
                source.identity_hash("announcement_summary", "feed_body", source.body),
            ),
            (
                "summary",
                source.identity_hash("announcement_summary", "summary", source.body),
            ),
        ],
    )
    .await?;
    let detail = read_projection(
        pool,
        state,
        "announcement",
        key,
        "translation",
        "detail",
        &source.identity_hash("announcement_detail", "detail", source.body),
    )
    .await?;
    let smart = read_first_projection(
        pool,
        state,
        "announcement",
        key,
        "polishing",
        &[
            (
                "feed_card",
                source.identity_hash("announcement_smart", "feed_card", source.body),
            ),
            (
                "smart",
                source.identity_hash("announcement_smart", "smart", source.body),
            ),
        ],
    )
    .await?;
    Ok(FeedProcessing {
        translation_source_hash: api::announcement_feed_translation_source_hash(
            source.repo_full_name,
            Some(source.discussion_number),
            source.title,
            feed_body.as_deref(),
        ),
        detail_source_hash: api::announcement_detail_source_hash(
            source.repo_full_name,
            Some(source.discussion_number),
            source.title,
            source.body.unwrap_or(""),
        ),
        smart_source_hash: translations::announcement_smart_feed_source_hash(
            key,
            source.repo_full_name,
            Some(source.discussion_number),
            source.title,
            source.body,
        ),
        translation,
        detail,
        smart,
    })
}

/// Release detail projection: the detail translation (falling back to the
/// summary slot) and the audience polishing reading.
pub(crate) async fn release_detail_processing(
    pool: &PgPool,
    state: OverlayState,
    source: &ReleaseSource<'_>,
    audience: ReleaseAudience,
) -> Result<DetailProcessing, sqlx::Error> {
    let entity_id = source.entity_id();
    let translation = read_first_projection(
        pool,
        state,
        "release",
        &entity_id,
        "translation",
        &[
            (
                "detail",
                source.identity_hash("release_detail", "detail", source.body),
            ),
            (
                "summary",
                source.identity_hash("release_summary", "summary", source.body),
            ),
        ],
    )
    .await?;
    let smart = read_latest_polishing_projection(
        pool,
        state,
        "release",
        &entity_id,
        audience.polishing_variants(),
    )
    .await?;
    Ok(DetailProcessing { translation, smart })
}

/// Announcement detail projection.
pub(crate) async fn announcement_detail_processing(
    pool: &PgPool,
    state: OverlayState,
    source: &AnnouncementSource<'_>,
) -> Result<DetailProcessing, sqlx::Error> {
    let translation = read_projection(
        pool,
        state,
        "announcement",
        source.discussion_key,
        "translation",
        "detail",
        &source.identity_hash("announcement_detail", "detail", source.body),
    )
    .await?;
    let smart = read_projection(
        pool,
        state,
        "announcement",
        source.discussion_key,
        "polishing",
        "smart",
        &source.identity_hash("announcement_smart", "smart", source.body),
    )
    .await?;
    Ok(DetailProcessing { translation, smart })
}

/// Public Release translation: the exact detail projection only.
pub(crate) async fn public_release_translation(
    pool: &PgPool,
    state: OverlayState,
    source: &ReleaseSource<'_>,
) -> Result<Option<ContentProjection>, sqlx::Error> {
    read_exact_projection(
        pool,
        state,
        "release",
        &source.entity_id(),
        "translation",
        "detail",
        &source.identity_hash("release_detail", "detail", source.body),
    )
    .await
}

/// Public Release polishing reading: the newest public variant only.
pub(crate) async fn public_release_smart(
    pool: &PgPool,
    state: OverlayState,
    source: &ReleaseSource<'_>,
) -> Result<Option<ContentProjection>, sqlx::Error> {
    read_latest_polishing_projection(
        pool,
        state,
        "release",
        &source.entity_id(),
        ReleaseAudience::Public.polishing_variants(),
    )
    .await
}

// ---------------------------------------------------------------------------
// Translation request metadata
// ---------------------------------------------------------------------------

/// Request kinds that own a release translation payload, in the order the
/// legacy `latest_request_id_for_resource` probes them (detail first, then the
/// retained summary identity).
pub(crate) const RELEASE_TRANSLATION_REQUEST_KINDS: [&str; 2] =
    ["release_detail", "release_summary"];

/// Request kind that owns a release reading (polishing) payload.
pub(crate) const RELEASE_SMART_REQUEST_KINDS: [&str; 1] = ["release_smart"];

/// Whether a content-processing work status still represents in-flight work.
///
/// Translated payloads built from these statuses carry
/// `auto_translate: true`, matching the legacy release read contract.
pub(crate) fn is_active_work_status(status: &str) -> bool {
    ACTIVE_WORK_STATUSES.contains(&status)
}

/// Newest translation request that produced a payload for `entity_id`.
///
/// Mirrors `content_processing::latest_request_id_for_resource`: the request
/// for the first known kind wins, then the newest request of that kind. The
/// scope filter reproduces the legacy per-user lookups; the global projection
/// reads pass `None` because the work is repository-scoped.
pub(crate) async fn latest_translation_request_id(
    pool: &PgPool,
    entity_id: &str,
    kinds: &[&str],
    scope_user_id: Option<&str>,
) -> Result<Option<String>, sqlx::Error> {
    for kind in kinds {
        let id = sqlx::query_scalar::<_, String>(
            "SELECT id FROM public.translation_requests \
             WHERE entity_id = $1 AND kind = $2 AND target_lang = $3 \
               AND ($4::text IS NULL OR scope_user_id = $4) \
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(entity_id)
        .bind(*kind)
        .bind(TARGET_LANG)
        .bind(scope_user_id)
        .fetch_optional(pool)
        .await?;
        if id.is_some() {
            return Ok(id);
        }
    }
    Ok(None)
}

/// Batched variant of [`latest_translation_request_id`] for feed/list reads.
pub(crate) async fn latest_translation_request_ids(
    pool: &PgPool,
    entity_ids: &[String],
    kinds: &[&str],
    scope_user_id: Option<&str>,
) -> Result<HashMap<String, String>, sqlx::Error> {
    let mut requests = HashMap::new();
    if entity_ids.is_empty() {
        return Ok(requests);
    }
    for kind in kinds {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT DISTINCT ON (entity_id) entity_id, id \
             FROM public.translation_requests \
             WHERE entity_id = ANY($1) AND kind = $2 AND target_lang = $3 \
               AND ($4::text IS NULL OR scope_user_id = $4) \
             ORDER BY entity_id, created_at DESC, id DESC",
        )
        .bind(entity_ids)
        .bind(*kind)
        .bind(TARGET_LANG)
        .bind(scope_user_id)
        .fetch_all(pool)
        .await?;
        for (entity_id, request_id) in rows {
            requests.entry(entity_id).or_insert(request_id);
        }
    }
    Ok(requests)
}

#[cfg(test)]
mod audience_tests {
    use super::*;

    /// Public readers resolve the public variant and nothing else: the legacy
    /// `smart` row may describe a reading derived from private compare evidence.
    #[test]
    fn public_audience_never_falls_back_to_legacy_smart() {
        assert_eq!(
            ReleaseAudience::Public.polishing_variants(),
            &[translations::RELEASE_READING_PUBLIC_VARIANT]
        );
        assert!(
            !ReleaseAudience::Public
                .polishing_variants()
                .contains(&"smart")
        );
    }

    /// Authorized viewers keep the audit reading variant first, with the
    /// pre-cutover `smart` projection as the documented fallback.
    #[test]
    fn authorized_audience_reads_its_variant_then_legacy_smart() {
        let variants = ReleaseAudience::Authorized.polishing_variants();
        assert_eq!(
            variants[0],
            translations::RELEASE_READING_AUTHORIZED_VARIANT
        );
        assert_eq!(variants.last().copied(), Some("smart"));
        assert!(!variants.contains(&translations::RELEASE_READING_PUBLIC_VARIANT));
    }
}
