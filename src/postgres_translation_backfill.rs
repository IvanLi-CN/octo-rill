//! One-shot backfill: publish content projections for finished legacy
//! translation outputs.
//!
//! After the content-processing cutover the public detail read resolves
//! translations only from `content_current_result_projections`, but the
//! translation outputs produced before the projection writer existed live in
//! `translation_work_items` alone. This module republishes those stored outputs
//! as projections without running anything: no attempt event, provider
//! admission or LLM audit row is created, and the legacy rows are read-only.
//!
//! The projection is published through
//! [`crate::postgres_content::publish_translation_result`], the single
//! projection writer, inside one transaction per work row. The identity it is
//! keyed by is the reader identity rebuilt from the stored Release
//! ([`crate::postgres_content_overlay::release_detail_translation_item`]); the
//! attempt's own text is rebuilt from its persisted source blocks and must hash
//! to the same identity. A mismatch means the Release changed after the attempt,
//! so the stored output describes text no reader can resolve: it is skipped and
//! reported instead of forced into the projection tables.
//!
//! Scope is Release detail translation. Other kinds keep their own read paths
//! and are counted, not published.
//!
//! Entry point: `OCTORILL_BACKFILL_TRANSLATION_PROJECTIONS=1` runs
//! [`run_from_env`] after the PostgreSQL runtime connects and exits without
//! serving traffic. `OCTORILL_BACKFILL_LIMIT` bounds how many stored outputs one
//! run inspects.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

use crate::{
    content_processing::{self, GLOBAL_PROTOCOL_VERSION},
    content_source_identity::ContentWorkIdentity,
    postgres_content::{self, TranslationPublication},
    postgres_content_overlay::{self, ReleaseDetailOrigin},
    postgres_write,
    translations::{TranslationRequestItemInput, TranslationSourceBlock},
};

/// Enables the one-shot backfill mode.
pub(crate) const MODE_ENV: &str = "OCTORILL_BACKFILL_TRANSLATION_PROJECTIONS";
/// Optional bound on how many stored outputs one run inspects.
pub(crate) const LIMIT_ENV: &str = "OCTORILL_BACKFILL_LIMIT";

const RESOURCE_TYPE: &str = "release";
const PIPELINE: &str = "translation";
const VARIANT: &str = "detail";
const TARGET_LANG: &str = "zh-CN";
/// Request kind the detail read path resolves.
const SCOPE_KIND: &str = "release_detail";
/// Producer recorded on the rebuilt reader identity.
const PRODUCER_REF: &str = "postgres.backfill.translation-projection";

/// What one backfill run did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct BackfillReport {
    /// Release-detail work rows inspected by this run.
    pub scanned: usize,
    /// Projections written by this run.
    pub published: usize,
    /// Rows whose identity already has a current projection.
    pub already_current: usize,
    /// Rows whose stored output describes Release text that no longer exists.
    pub skipped_source_hash_mismatch: usize,
    /// Rows whose stored output fails the shared output contract.
    pub skipped_output_contract: usize,
    /// Rows whose target Release row is gone or not addressable.
    pub skipped_missing_release: usize,
    /// Rows whose persisted source snapshot cannot rebuild an identity.
    pub skipped_incomplete_source: usize,
    /// Rows that failed for an unexpected reason; the run exits non-zero.
    pub failed: usize,
    /// Finished translation rows outside the Release-detail scope.
    pub skipped_out_of_scope: Vec<OutOfScopeCount>,
}

/// One finished-translation group this backfill does not publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutOfScopeCount {
    pub kind: String,
    pub target_lang: String,
    pub rows: i64,
}

/// Read the one-shot limit from the environment.
///
/// An unreadable or non-positive limit aborts the run: silently publishing
/// everything when an operator asked for a bounded repair is worse than
/// refusing.
pub(crate) fn limit_from_env() -> Result<Option<i64>> {
    let Some(raw) = std::env::var(LIMIT_ENV).ok() else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let limit = raw
        .parse::<i64>()
        .with_context(|| format!("invalid {LIMIT_ENV} (expected a positive integer): {raw}"))?;
    if limit < 1 {
        bail!("invalid {LIMIT_ENV} (expected a positive integer): {raw}");
    }
    Ok(Some(limit))
}

/// Run the backfill with the limit from the environment.
pub(crate) async fn run_from_env(pool: &PgPool) -> Result<BackfillReport> {
    let report = run(pool, limit_from_env()?).await?;
    info!(
        scanned = report.scanned,
        published = report.published,
        already_current = report.already_current,
        skipped_source_hash_mismatch = report.skipped_source_hash_mismatch,
        skipped_output_contract = report.skipped_output_contract,
        skipped_missing_release = report.skipped_missing_release,
        skipped_incomplete_source = report.skipped_incomplete_source,
        failed = report.failed,
        skipped_out_of_scope = report.skipped_out_of_scope.len(),
        "translation projection backfill finished"
    );
    Ok(report)
}

/// Publish projections for finished Release-detail translations that lack one.
pub(crate) async fn run(pool: &PgPool, limit: Option<i64>) -> Result<BackfillReport> {
    let mode = postgres_content_overlay::current_mode(pool).await;
    let mut report = BackfillReport {
        skipped_out_of_scope: out_of_scope_counts(pool).await?,
        ..BackfillReport::default()
    };
    for skipped in &report.skipped_out_of_scope {
        info!(
            kind = %skipped.kind,
            target_lang = %skipped.target_lang,
            rows = skipped.rows,
            "finished translation rows outside the Release detail backfill scope"
        );
    }
    let candidates = load_candidates(pool, limit.unwrap_or(i64::MAX)).await?;
    report.scanned = candidates.len();
    info!(
        mode = ?mode,
        limit = ?limit,
        candidates = candidates.len(),
        "translation projection backfill started"
    );
    let mut failures = Vec::new();
    for candidate in candidates {
        let work_item_id = candidate.work_item_id.clone();
        match publish_candidate(pool, &candidate).await {
            Ok(RowOutcome::Published {
                identity_id,
                source_hash,
            }) => {
                report.published += 1;
                info!(
                    work_item_id = %work_item_id,
                    release_id = %candidate.entity_id,
                    variant = VARIANT,
                    identity_id = %identity_id,
                    source_hash = %source_hash,
                    "translation projection backfilled"
                );
            }
            Ok(RowOutcome::AlreadyCurrent { identity_id }) => {
                report.already_current += 1;
                debug!(
                    work_item_id = %work_item_id,
                    release_id = %candidate.entity_id,
                    identity_id = %identity_id,
                    "translation projection already current"
                );
            }
            Ok(RowOutcome::SourceHashMismatch {
                attempted_source_hash,
                canonical_source_hash,
            }) => {
                report.skipped_source_hash_mismatch += 1;
                warn!(
                    work_item_id = %work_item_id,
                    release_id = %candidate.entity_id,
                    attempted_source_hash = %attempted_source_hash,
                    canonical_source_hash = %canonical_source_hash,
                    "stored translation describes Release text that no longer exists; projection not published"
                );
            }
            Ok(RowOutcome::OutputContractRejected { error }) => {
                report.skipped_output_contract += 1;
                warn!(
                    work_item_id = %work_item_id,
                    release_id = %candidate.entity_id,
                    error = %error,
                    "stored translation fails the global output contract; projection not published"
                );
            }
            Ok(RowOutcome::MissingRelease { reason }) => {
                report.skipped_missing_release += 1;
                warn!(
                    work_item_id = %work_item_id,
                    release_id = %candidate.entity_id,
                    reason,
                    "stored translation has no addressable Release row; projection not published"
                );
            }
            Ok(RowOutcome::IncompleteSource) => {
                report.skipped_incomplete_source += 1;
                warn!(
                    work_item_id = %work_item_id,
                    release_id = %candidate.entity_id,
                    "stored translation has no usable source snapshot; projection not published"
                );
            }
            Err(error) => {
                report.failed += 1;
                warn!(
                    work_item_id = %work_item_id,
                    release_id = %candidate.entity_id,
                    error = %error,
                    "translation projection backfill row failed"
                );
                failures.push(format!("{work_item_id}: {error}"));
            }
        }
    }
    if !failures.is_empty() {
        bail!(
            "translation projection backfill failed for {} of {} stored outputs: {}",
            failures.len(),
            report.scanned,
            failures.join("; ")
        );
    }
    Ok(report)
}

/// One finished Release-detail translation row with stored output.
struct Candidate {
    work_item_id: String,
    entity_id: String,
    source_blocks_json: String,
    title_zh: Option<String>,
    summary_md: Option<String>,
    body_md: Option<String>,
}

/// What happened to one stored output.
enum RowOutcome {
    Published {
        identity_id: String,
        source_hash: String,
    },
    AlreadyCurrent {
        identity_id: String,
    },
    SourceHashMismatch {
        attempted_source_hash: String,
        canonical_source_hash: String,
    },
    OutputContractRejected {
        error: String,
    },
    MissingRelease {
        reason: String,
    },
    IncompleteSource,
}

/// Read finished Release-detail translations with stored output.
///
/// The rows are the legacy caller-facing adapter: several requesters can hold
/// one row per identity, so duplicates collapse at the projection check instead
/// of here.
async fn load_candidates(pool: &PgPool, limit: i64) -> Result<Vec<Candidate>> {
    let rows = sqlx::query(
        "SELECT id, entity_id, source_blocks_json, title_zh, summary_md, body_md \
         FROM public.translation_work_items \
         WHERE status = 'completed' \
           AND result_status = 'ready' \
           AND kind = $1 \
           AND target_lang = $2 \
           AND (title_zh IS NOT NULL OR summary_md IS NOT NULL OR body_md IS NOT NULL) \
         ORDER BY entity_id, created_at, id \
         LIMIT $3",
    )
    .bind(SCOPE_KIND)
    .bind(TARGET_LANG)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("load finished translation outputs for projection backfill")?;
    rows.into_iter()
        .map(|row| {
            Ok(Candidate {
                work_item_id: row.try_get("id")?,
                entity_id: row.try_get("entity_id")?,
                source_blocks_json: row.try_get("source_blocks_json")?,
                title_zh: row.try_get("title_zh")?,
                summary_md: row.try_get("summary_md")?,
                body_md: row.try_get("body_md")?,
            })
        })
        .collect()
}

/// Count finished translation rows this backfill deliberately leaves alone.
async fn out_of_scope_counts(pool: &PgPool) -> Result<Vec<OutOfScopeCount>> {
    let rows = sqlx::query(
        "SELECT kind, target_lang, COUNT(*) AS rows \
         FROM public.translation_work_items \
         WHERE status = 'completed' \
           AND result_status = 'ready' \
           AND (title_zh IS NOT NULL OR summary_md IS NOT NULL OR body_md IS NOT NULL) \
           AND NOT (kind = $1 AND target_lang = $2) \
         GROUP BY kind, target_lang \
         ORDER BY kind, target_lang",
    )
    .bind(SCOPE_KIND)
    .bind(TARGET_LANG)
    .fetch_all(pool)
    .await
    .context("count finished translations outside the backfill scope")?;
    rows.into_iter()
        .map(|row| {
            Ok(OutOfScopeCount {
                kind: row.try_get("kind")?,
                target_lang: row.try_get("target_lang")?,
                rows: row.try_get("rows")?,
            })
        })
        .collect()
}

/// Stored Release row the projection identity is rebuilt from.
struct ReleaseRow {
    release_id: i64,
    repo_id: i64,
    html_url: String,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
}

async fn load_release(pool: &PgPool, release_id: i64) -> Result<Option<ReleaseRow>> {
    let row = sqlx::query(
        "SELECT repo_id, html_url, tag_name, name, body FROM public.repo_releases WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_optional(pool)
    .await
    .context("load Release row for the projection backfill")?;
    row.map(|row| {
        Ok(ReleaseRow {
            release_id,
            repo_id: row.try_get("repo_id")?,
            html_url: row.try_get("html_url")?,
            tag_name: row.try_get("tag_name")?,
            name: row.try_get("name")?,
            body: row.try_get("body")?,
        })
    })
    .transpose()
}

/// Publish the projection for one stored output, or explain why not.
async fn publish_candidate(pool: &PgPool, candidate: &Candidate) -> Result<RowOutcome> {
    let Ok(release_id) = candidate.entity_id.parse::<i64>() else {
        return Ok(RowOutcome::MissingRelease {
            reason: "entity_id is not a release id".to_owned(),
        });
    };
    let Some(release) = load_release(pool, release_id).await? else {
        return Ok(RowOutcome::MissingRelease {
            reason: "release row is gone".to_owned(),
        });
    };
    let current = postgres_content_overlay::release_detail_translation_item(
        &ReleaseDetailOrigin {
            release_id: release.release_id,
            html_url: &release.html_url,
            repo_id: release.repo_id,
            tag_name: &release.tag_name,
            name: release.name.as_deref(),
            body: release.body.as_deref(),
            observed_at: None,
        },
        PRODUCER_REF,
    );
    let Some(attempted) =
        attempted_identity_item(&candidate.source_blocks_json, &candidate.entity_id)
            .context("decode the stored translation source snapshot")?
    else {
        return Ok(RowOutcome::IncompleteSource);
    };
    let attempted_source_hash = crate::content_source_identity::source_hash_for_item(&attempted);
    let canonical_source_hash = crate::content_source_identity::source_hash_for_item(&current);
    if attempted_source_hash != canonical_source_hash {
        return Ok(RowOutcome::SourceHashMismatch {
            attempted_source_hash,
            canonical_source_hash,
        });
    }
    let raw = stored_payload_json(candidate, &current.target_slots);
    let output = match content_processing::validate_output(
        &raw,
        &current.target_slots,
        &current.source_blocks,
    ) {
        Ok(output) => output,
        Err(error) => {
            return Ok(RowOutcome::OutputContractRejected {
                error: error.to_string(),
            });
        }
    };
    let identity = ContentWorkIdentity {
        canonical_resource_type: RESOURCE_TYPE.to_owned(),
        canonical_resource_id: candidate.entity_id.clone(),
        pipeline: PIPELINE.to_owned(),
        variant: current.variant.clone(),
        target_lang: current.target_lang.clone(),
        source_hash: canonical_source_hash.clone(),
        protocol_version: GLOBAL_PROTOCOL_VERSION.to_owned(),
    };
    let identity_id = crate::content_source_identity::identity_id_for(&identity)
        .context("derive the projection identity id")?;
    if current_projection_exists(pool, &identity_id).await? {
        return Ok(RowOutcome::AlreadyCurrent { identity_id });
    }
    let mut tx = postgres_write::begin(pool).await?;
    postgres_content::publish_translation_result(
        &mut tx,
        TranslationPublication {
            canonical_resource_type: RESOURCE_TYPE,
            pipeline: PIPELINE,
            item: &current,
            source_hash: &canonical_source_hash,
            payload: &output,
        },
    )
    .await?;
    tx.commit()
        .await
        .context("commit the backfilled translation projection")?;
    Ok(RowOutcome::Published {
        identity_id,
        source_hash: canonical_source_hash,
    })
}

/// Whether the identity already resolves to a readable projection.
///
/// Checked through `content_work_identities` rather than a recomputed hash so
/// the answer is the read path's own fact; a published
/// `content_result_projections` row without its current row is republished.
async fn current_projection_exists(pool: &PgPool, identity_id: &str) -> Result<bool> {
    let found = sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM public.content_current_result_projections WHERE identity_id = $1",
    )
    .bind(identity_id)
    .fetch_optional(pool)
    .await
    .context("check the current projection for a backfill identity")?;
    Ok(found.is_some())
}

/// Rebuild the projection identity from the attempt's persisted source blocks.
///
/// The identity drops source-revision metadata (`source_observed_at`,
/// `source_revision_tiebreak`), which
/// [`postgres_content_overlay::identity_item`] does by construction: only the
/// metadata, title and body blocks reach the hash.
fn attempted_identity_item(
    source_blocks_json: &str,
    entity_id: &str,
) -> Result<Option<TranslationRequestItemInput>> {
    let Ok(blocks) = serde_json::from_str::<Vec<TranslationSourceBlock>>(source_blocks_json) else {
        return Ok(None);
    };
    let Some(metadata) = source_block_text(&blocks, "metadata") else {
        return Ok(None);
    };
    let Some(title) = source_block_text(&blocks, "title") else {
        return Ok(None);
    };
    let body = source_block_text(&blocks, "body_markdown")
        .or_else(|| source_block_text(&blocks, "excerpt"))
        .map(str::to_owned);
    Ok(Some(postgres_content_overlay::identity_item(
        SCOPE_KIND,
        VARIANT,
        entity_id,
        metadata.to_owned(),
        title.to_owned(),
        body,
    )))
}

fn source_block_text<'a>(blocks: &'a [TranslationSourceBlock], slot: &str) -> Option<&'a str> {
    blocks
        .iter()
        .find(|block| block.slot == slot)
        .map(|block| block.text.as_str())
}

/// Render the stored translation columns as the JSON object the shared output
/// contract validates.
///
/// The translation lane stores the finished text in `body_md` (mirroring it into
/// `summary_md`), so the body slot reads `body_md` first; `summary_md` only
/// covers a row whose body slot was not written.
fn stored_payload_json(candidate: &Candidate, target_slots: &[String]) -> String {
    let summary = candidate
        .body_md
        .as_deref()
        .or(candidate.summary_md.as_deref());
    let mut object = serde_json::Map::new();
    for slot in target_slots {
        let value = match slot.as_str() {
            "title_zh" => candidate.title_zh.clone().map(Value::String),
            "body_md" | "summary_md" => summary.map(|text| Value::String(text.to_owned())),
            _ => None,
        };
        object.insert(slot.clone(), value.unwrap_or(Value::Null));
    }
    Value::Object(object).to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        BackfillReport, Candidate, OutOfScopeCount, RowOutcome, TARGET_LANG,
        attempted_identity_item, source_block_text, stored_payload_json,
    };
    use crate::content_processing::validate_output;
    use crate::postgres_content_overlay::{self, ReleaseDetailOrigin};
    use crate::translations::TranslationSourceBlock;

    fn candidate(body_md: Option<&str>, summary_md: Option<&str>) -> Candidate {
        Candidate {
            work_item_id: "wi-1".to_owned(),
            entity_id: "42".to_owned(),
            source_blocks_json: "[]".to_owned(),
            title_zh: Some("项目 v1 发布".to_owned()),
            summary_md: summary_md.map(str::to_owned),
            body_md: body_md.map(str::to_owned),
        }
    }

    fn detail_slots(has_body: bool) -> Vec<String> {
        if has_body {
            vec!["title_zh".to_owned(), "body_md".to_owned()]
        } else {
            vec!["title_zh".to_owned()]
        }
    }

    /// The stored translation is valid under the shared output contract, which
    /// is what makes a backfilled projection readable. A body written to the
    /// wrong slot would fail here instead of publishing an empty translation.
    #[test]
    fn stored_payload_satisfies_the_output_contract() {
        let slots = detail_slots(true);
        let raw = stored_payload_json(&candidate(Some("发布说明正文"), None), &slots);
        let source_blocks = vec![TranslationSourceBlock {
            slot: "body_markdown".to_owned(),
            text: "Release notes body".to_owned(),
        }];
        let output = validate_output(&raw, &slots, &source_blocks).expect("valid stored output");
        assert_eq!(
            output.get("body_md").and_then(|value| value.as_str()),
            Some("发布说明正文")
        );
    }

    /// `summary_md` alone still fills the body slot for a row whose `body_md`
    /// was never written.
    #[test]
    fn stored_payload_falls_back_to_the_summary_column() {
        let slots = detail_slots(true);
        let raw = stored_payload_json(&candidate(None, Some("摘要正文")), &slots);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&raw)
                .expect("payload json")
                .get("body_md")
                .and_then(|value| value.as_str()),
            Some("摘要正文")
        );
    }

    /// Source-revision metadata is not identity, so an attempt recorded with it
    /// hashes to the same identity the reader rebuilds from live Release text.
    /// A leak would make every revision-tracked attempt look stale.
    #[test]
    fn attempted_identity_ignores_source_revision_metadata() {
        let plain = serde_json::to_string(&vec![
            TranslationSourceBlock {
                slot: "metadata".to_owned(),
                text: "acme/demo\nv1".to_owned(),
            },
            TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "v1".to_owned(),
            },
            TranslationSourceBlock {
                slot: "body_markdown".to_owned(),
                text: "Notes".to_owned(),
            },
        ])
        .expect("encode blocks");
        let revised = serde_json::to_string(&vec![
            TranslationSourceBlock {
                slot: "source_observed_at".to_owned(),
                text: "2026-11-25T02:00:00Z".to_owned(),
            },
            TranslationSourceBlock {
                slot: "source_revision_tiebreak".to_owned(),
                text: "compare-evidence".to_owned(),
            },
            TranslationSourceBlock {
                slot: "metadata".to_owned(),
                text: "acme/demo\nv1".to_owned(),
            },
            TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "v1".to_owned(),
            },
            TranslationSourceBlock {
                slot: "body_markdown".to_owned(),
                text: "Notes".to_owned(),
            },
        ])
        .expect("encode blocks");
        let plain_item = attempted_identity_item(&plain, "42")
            .expect("decode")
            .expect("identity item");
        let revised_item = attempted_identity_item(&revised, "42")
            .expect("decode")
            .expect("identity item");
        let plain_hash = crate::content_source_identity::source_hash_for_item(&plain_item);
        let revised_hash = crate::content_source_identity::source_hash_for_item(&revised_item);
        assert_eq!(plain_hash, revised_hash);

        // The reader identity for the same Release text is the same hash: this
        // is the equality the backfill relies on to publish instead of skip.
        let reader_item = postgres_content_overlay::release_detail_translation_item(
            &ReleaseDetailOrigin {
                release_id: 42,
                html_url: "https://github.com/acme/demo/releases/tag/v1",
                repo_id: 7,
                tag_name: "v1",
                name: Some("v1"),
                body: Some("Notes"),
                observed_at: Some("2026-11-25T02:00:00Z"),
            },
            "test",
        );
        assert_eq!(
            crate::content_source_identity::source_hash_for_item(&reader_item),
            plain_hash
        );
    }

    /// A stored snapshot without the metadata or title block cannot describe an
    /// identity, so the row is skipped instead of published under a guess.
    #[test]
    fn attempted_identity_requires_metadata_and_title() {
        let blocks = serde_json::to_string(&vec![TranslationSourceBlock {
            slot: "body_markdown".to_owned(),
            text: "Notes".to_owned(),
        }])
        .expect("encode blocks");
        assert!(
            attempted_identity_item(&blocks, "42")
                .expect("decode")
                .is_none()
        );
        let blocks = serde_json::to_string(&vec![
            TranslationSourceBlock {
                slot: "metadata".to_owned(),
                text: "acme/demo\nv1".to_owned(),
            },
            TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "v1".to_owned(),
            },
        ])
        .expect("encode blocks");
        let item = attempted_identity_item(&blocks, "42")
            .expect("decode")
            .expect("identity item");
        assert_eq!(item.variant, "detail");
        assert_eq!(item.target_slots, vec!["title_zh".to_owned()]);
        assert_eq!(source_block_text(&item.source_blocks, "title"), Some("v1"));
        assert_eq!(item.entity_id, "42");
        assert_eq!(item.target_lang, TARGET_LANG);
    }

    /// Rows outside the scope are reported per kind so an operator sees what the
    /// run deliberately ignored.
    #[test]
    fn out_of_scope_rows_are_reported_by_kind() {
        let report = BackfillReport {
            skipped_out_of_scope: vec![OutOfScopeCount {
                kind: "notification".to_owned(),
                target_lang: "zh-CN".to_owned(),
                rows: 9,
            }],
            ..BackfillReport::default()
        };
        assert_eq!(report.skipped_out_of_scope.len(), 1);
        assert_eq!(report.skipped_out_of_scope[0].kind, "notification");
        assert_eq!(report.skipped_out_of_scope[0].rows, 9);
        assert_eq!(report.published, 0);
        let _ = RowOutcome::IncompleteSource;
    }

    use sqlx::PgPool;

    async fn isolated_pool(name: &str) -> (PgPool, PgPool, String) {
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL").expect("TEST_PG_TRANSLATE_URL");
        let admin = PgPool::connect(&admin_url)
            .await
            .expect("connect admin database");
        let database = format!("{name}_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await
            .expect("create isolated database");
        let mut url = url::Url::parse(&admin_url).expect("parse test url");
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str())
            .await
            .expect("connect isolated database");
        crate::postgres_database_migrations::run(&pool)
            .await
            .expect("apply runtime migrations");
        (admin, pool, database)
    }

    async fn drop_isolated(admin: PgPool, pool: PgPool, database: String) {
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await
            .expect("drop isolated database");
        admin.close().await;
    }

    async fn seed_release(pool: &PgPool, release_id: i64, body: &str) {
        sqlx::query(
            "INSERT INTO public.repo_releases(id, repo_id, release_id, tag_name, name, body, html_url, published_at, updated_at) \
             VALUES ($1, 7, $2, $3, $3, $4, $5, $6, $6)",
        )
        .bind(format!("release-{release_id}"))
        .bind(release_id)
        .bind(format!("v{release_id}"))
        .bind(body)
        .bind(format!(
            "https://github.com/acme/demo/releases/tag/v{release_id}"
        ))
        .bind("2026-11-25T02:00:00Z")
        .execute(pool)
        .await
        .expect("seed release");
    }

    /// Source blocks a legacy attempt persisted for one Release detail item.
    fn stored_source_blocks(release_id: i64, body: &str) -> String {
        serde_json::to_string(&vec![
            TranslationSourceBlock {
                slot: "metadata".to_owned(),
                text: format!("acme/demo\nv{release_id}"),
            },
            TranslationSourceBlock {
                slot: "title".to_owned(),
                text: format!("v{release_id}"),
            },
            TranslationSourceBlock {
                slot: "body_markdown".to_owned(),
                text: body.to_owned(),
            },
        ])
        .expect("encode stored source blocks")
    }

    struct StoredOutput<'a> {
        id: &'a str,
        release_id: i64,
        kind: &'a str,
        variant: &'a str,
        attempted_body: &'a str,
    }

    /// Seed one finished translation row exactly as the translation lane left
    /// it: stored slots plus the source snapshot the attempt ran on.
    async fn seed_stored_output(pool: &PgPool, output: StoredOutput<'_>) {
        sqlx::query(
            "INSERT INTO public.translation_work_items(id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version, model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate, deadline_at, status, result_status, title_zh, summary_md, body_md, attempt_count, created_at, finished_at, updated_at) \
             VALUES ($1, $1, 'user-a', $2, $3, $4, 'zh-CN', 'translation-request.v1', 'postgres-default', $5, $6, $7, 0, $8::timestamptz, 'completed', 'ready', $9, $10, $10, 2, $8::timestamptz, $8::timestamptz, $8::timestamptz)",
        )
        .bind(output.id)
        .bind(output.kind)
        .bind(output.variant)
        .bind(output.release_id.to_string())
        .bind(format!("legacy-hash-{}", output.id))
        .bind(stored_source_blocks(output.release_id, output.attempted_body))
        .bind(r#"["title_zh","body_md"]"#)
        .bind("2026-11-25T02:05:00Z")
        .bind(format!("项目 v{} 发布", output.release_id))
        .bind("发布说明正文")
        .execute(pool)
        .await
        .expect("seed stored translation output");
    }

    /// The reader identity for a Release row, as the detail read path builds it.
    fn reader_item(
        release_id: i64,
        tag_name: &str,
        name: &str,
        body: &str,
    ) -> crate::translations::TranslationRequestItemInput {
        postgres_content_overlay::release_detail_translation_item(
            &ReleaseDetailOrigin {
                release_id,
                html_url: &format!("https://github.com/acme/demo/releases/tag/{tag_name}"),
                repo_id: 7,
                tag_name,
                name: Some(name),
                body: Some(body),
                observed_at: None,
            },
            "test",
        )
    }

    async fn audit_counts(pool: &PgPool) -> (i64, i64, i64, i64, i64) {
        let attempt_events =
            sqlx::query_scalar("SELECT COUNT(*) FROM public.translation_attempt_events")
                .fetch_one(pool)
                .await
                .expect("count translation attempt events");
        let llm_calls =
            sqlx::query_scalar("SELECT COUNT(*) FROM public.translation_attempt_llm_calls")
                .fetch_one(pool)
                .await
                .expect("count translation llm calls");
        let content_attempts =
            sqlx::query_scalar("SELECT COUNT(*) FROM public.content_attempt_events")
                .fetch_one(pool)
                .await
                .expect("count content attempt events");
        let content_llm =
            sqlx::query_scalar("SELECT COUNT(*) FROM public.content_attempt_llm_calls")
                .fetch_one(pool)
                .await
                .expect("count content llm calls");
        let admissions =
            sqlx::query_scalar("SELECT COUNT(*) FROM public.content_attempt_provider_admissions")
                .fetch_one(pool)
                .await
                .expect("count provider admissions");
        (
            attempt_events,
            llm_calls,
            content_attempts,
            content_llm,
            admissions,
        )
    }

    async fn legacy_rows_digest(pool: &PgPool) -> String {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT to_jsonb(t)::text FROM public.translation_work_items t ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .expect("dump legacy translation rows");
        rows.join("\n")
    }

    /// Acceptance for the backfill: on an isolated PostgreSQL 18 cluster one
    /// finished output without a projection is published, an already-published
    /// identity is left alone, a stale attempt is skipped, a second run
    /// publishes nothing, and no attempt or audit fact changes.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn backfill_publishes_only_missing_projections_without_new_attempts() -> anyhow::Result<()>
    {
        let (admin, pool, database) = isolated_pool("backfill_projection").await;
        let result = async {
            sqlx::query(
                "INSERT INTO public.users(id, login, created_at, updated_at) VALUES ('user-a', 'user-a', '2026-11-25T02:00:00Z', '2026-11-25T02:00:00Z')",
            )
            .execute(&pool)
            .await
            .expect("seed user");
            sqlx::query("UPDATE public.content_processing_control SET mode = 'global' WHERE id = 1")
                .execute(&pool)
                .await
                .expect("switch to global mode");

            seed_release(&pool, 43, "Notes A").await;
            seed_release(&pool, 44, "Notes B").await;
            seed_release(&pool, 45, "New body").await;

            seed_stored_output(
                &pool,
                StoredOutput {
                    id: "legacy-missing",
                    release_id: 43,
                    kind: "release_detail",
                    variant: "detail",
                    attempted_body: "Notes A",
                },
            )
            .await;
            seed_stored_output(
                &pool,
                StoredOutput {
                    id: "legacy-published",
                    release_id: 44,
                    kind: "release_detail",
                    variant: "detail",
                    attempted_body: "Notes B",
                },
            )
            .await;
            seed_stored_output(
                &pool,
                StoredOutput {
                    id: "legacy-stale",
                    release_id: 45,
                    kind: "release_detail",
                    variant: "detail",
                    attempted_body: "Old body",
                },
            )
            .await;
            // Out of scope: a finished notification translation is counted, not
            // published under a Release identity.
            sqlx::query(
                "INSERT INTO public.translation_work_items(id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version, model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate, deadline_at, status, result_status, title_zh, summary_md, attempt_count, created_at, finished_at, updated_at) \
                 VALUES ('legacy-notification', 'legacy-notification', 'user-a', 'notification', 'shared', 'thread-1', 'zh-CN', 'translation-request.v1', 'postgres-default', 'h', '[]', '[]', 0, '2026-11-25T02:05:00Z', 'completed', 'ready', '通知标题', '通知摘要', 1, '2026-11-25T02:05:00Z', '2026-11-25T02:05:00Z', '2026-11-25T02:05:00Z')",
            )
            .execute(&pool)
            .await
            .expect("seed out-of-scope notification output");

            // One identity already carries a published projection, written by
            // the same single writer the backfill uses.
            let published_item = reader_item(44, "v44", "v44", "Notes B");
            let published_hash =
                crate::content_source_identity::source_hash_for_item(&published_item);
            let published_payload = crate::content_processing::validate_output(
                &serde_json::json!({ "title_zh": "项目 v44 发布", "body_md": "发布说明正文" })
                    .to_string(),
                &published_item.target_slots,
                &published_item.source_blocks,
            )
            .expect("valid published payload");
            let mut tx = crate::postgres_write::begin(&pool).await.expect("begin");
            crate::postgres_content::publish_translation_result(
                &mut tx,
                crate::postgres_content::TranslationPublication {
                    canonical_resource_type: "release",
                    pipeline: "translation",
                    item: &published_item,
                    source_hash: &published_hash,
                    payload: &published_payload,
                },
            )
            .await
            .expect("pre-publish the already-current identity");
            tx.commit().await.expect("commit pre-publication");

            let baseline_audit = audit_counts(&pool).await;
            let baseline_rows = legacy_rows_digest(&pool).await;
            let baseline_projections: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_result_projections")
                    .fetch_one(&pool)
                    .await
                    .expect("count baseline projections");

            let first = super::run(&pool, None).await.expect("first backfill run");
            assert_eq!(first.scanned, 3, "only Release detail rows are inspected");
            assert_eq!(first.published, 1, "only the missing projection is published");
            assert_eq!(first.already_current, 1, "the published identity is left alone");
            assert_eq!(first.skipped_source_hash_mismatch, 1, "stale output is skipped");
            assert_eq!(first.skipped_output_contract, 0);
            assert_eq!(first.failed, 0);
            assert_eq!(
                first.skipped_out_of_scope,
                vec![super::OutOfScopeCount {
                    kind: "notification".to_owned(),
                    target_lang: "zh-CN".to_owned(),
                    rows: 1,
                }]
            );

            let after_first: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_result_projections")
                    .fetch_one(&pool)
                    .await
                    .expect("count projections after the first run");
            assert_eq!(after_first, baseline_projections + 1);
            let current_rows: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_current_result_projections",
            )
            .fetch_one(&pool)
            .await
            .expect("count current projections");
            assert_eq!(current_rows, 2, "both readable identities have a current row");

            // The stored output is readable through the public detail path.
            let state = postgres_content_overlay::OverlayState::load(&pool).await;
            let source = postgres_content_overlay::ReleaseSource {
                release_id: 43,
                repo_full_name: "acme/demo",
                repo_identity: "acme/demo",
                tag_name: "v43",
                name: Some("v43"),
                body: Some("Notes A"),
                previous_tag_name: None,
            };
            let projection =
                postgres_content_overlay::public_release_translation(&pool, state, &source)
                    .await
                    .expect("read the backfilled projection")
                    .expect("the backfilled projection resolves");
            assert_eq!(projection.status, "ready");
            assert_eq!(projection.title().as_deref(), Some("项目 v43 发布"));
            assert_eq!(
                projection.translation_summary().as_deref(),
                Some("发布说明正文")
            );

            let second = super::run(&pool, None).await.expect("second backfill run");
            assert_eq!(second.published, 0, "a second run publishes nothing new");
            assert_eq!(second.already_current, 2);
            assert_eq!(second.skipped_source_hash_mismatch, 1);
            assert_eq!(second.failed, 0);
            let after_second: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_result_projections")
                    .fetch_one(&pool)
                    .await
                    .expect("count projections after the second run");
            assert_eq!(after_second, after_first, "idempotent across runs");

            assert_eq!(audit_counts(&pool).await, baseline_audit, "no new attempt or audit fact");
            assert_eq!(legacy_rows_digest(&pool).await, baseline_rows, "legacy rows are untouched");
            let attempt_counts: Vec<i32> = sqlx::query_scalar(
                "SELECT attempt_count FROM public.content_work_items ORDER BY created_at, id",
            )
            .fetch_all(&pool)
            .await
            .expect("read content attempt counts");
            assert!(!attempt_counts.is_empty());
            assert!(
                attempt_counts.iter().all(|count| *count == 0),
                "a backfilled projection records no attempt: {attempt_counts:?}"
            );
            let lease_owners: i32 = sqlx::query_scalar(
                "SELECT COUNT(*)::int4 FROM public.content_work_items WHERE lease_owner IS NOT NULL",
            )
            .fetch_one(&pool)
            .await
            .expect("count leased content work items");
            assert_eq!(lease_owners, 0, "a backfilled projection holds no lease");
            let stale_rows: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_result_projections WHERE canonical_resource_id = '45'",
            )
            .fetch_one(&pool)
            .await
            .expect("count stale-release projections");
            assert_eq!(stale_rows, 0, "a source-hash mismatch is never forced");
            Ok::<_, anyhow::Error>(())
        }
        .await;
        drop_isolated(admin, pool, database).await;
        result
    }

    /// The limit bounds how many stored outputs one run inspects, so an
    /// operator can repair in batches.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn backfill_limit_bounds_the_inspected_rows() -> anyhow::Result<()> {
        let (admin, pool, database) = isolated_pool("backfill_limit").await;
        let result = async {
            sqlx::query(
                "INSERT INTO public.users(id, login, created_at, updated_at) VALUES ('user-a', 'user-a', '2026-11-25T02:00:00Z', '2026-11-25T02:00:00Z')",
            )
            .execute(&pool)
            .await
            .expect("seed user");
            seed_release(&pool, 51, "Notes 51").await;
            seed_release(&pool, 52, "Notes 52").await;
            for release_id in [51_i64, 52] {
                seed_stored_output(
                    &pool,
                    StoredOutput {
                        id: if release_id == 51 {
                            "legacy-51"
                        } else {
                            "legacy-52"
                        },
                        release_id,
                        kind: "release_detail",
                        variant: "detail",
                        attempted_body: if release_id == 51 {
                            "Notes 51"
                        } else {
                            "Notes 52"
                        },
                    },
                )
                .await;
            }
            let bounded = super::run(&pool, Some(1))
                .await
                .expect("bounded backfill run");
            assert_eq!(bounded.scanned, 1);
            assert_eq!(bounded.published, 1);
            let remaining = super::run(&pool, None)
                .await
                .expect("unbounded backfill run");
            assert_eq!(remaining.published, 1, "the second batch publishes the rest");
            Ok::<_, anyhow::Error>(())
        }
        .await;
        drop_isolated(admin, pool, database).await;
        result
    }
}
