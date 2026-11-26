//! Content identity for the live read and translation paths.
//!
//! Every live writer and reader must derive the same identity for a release or
//! announcement, both for the legacy `ai_translations` display hashes and for
//! the content-processing projection hashes. Keeping one implementation here
//! replaces the copies that used to live in the retiring `api`,
//! `content_processing`, and `ai` modules.

use anyhow::Result;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::translations::{TranslationRequestItemInput, TranslationSourceBlock};

/// Protocol version stamped into content-processing work identities.
pub(crate) const GLOBAL_PROTOCOL_VERSION: &str = "content-processing.v1";

const GITHUB_DISCUSSION_PATH_SEGMENT: &str = "/discussions/";

/// Lowercase hex SHA-256 of `input`.
pub(crate) fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        write!(&mut out, "{:02x}", b).expect("sha256 hex");
    }
    out
}

pub(crate) fn parse_discussion_number_from_github_url(html_url: &str) -> Option<i64> {
    let parsed = url::Url::parse(html_url).ok()?;
    if !matches!(parsed.host_str(), Some("github.com" | "www.github.com")) {
        return None;
    }
    let (_, number_raw) = parsed.path().split_once(GITHUB_DISCUSSION_PATH_SEGMENT)?;
    number_raw
        .split('/')
        .next()
        .and_then(|value| value.trim().parse::<i64>().ok())
}

pub(crate) fn announcement_discussion_key(repo_full_name: &str, discussion_number: i64) -> String {
    format!(
        "{}#{discussion_number}",
        repo_full_name.trim().to_ascii_lowercase()
    )
}

pub fn parse_announcement_discussion_key(raw: &str) -> Option<(String, i64)> {
    let (repo_full_name, discussion_number_raw) = raw.trim().rsplit_once('#')?;
    let discussion_number = discussion_number_raw.trim().parse::<i64>().ok()?;
    let repo_full_name = repo_full_name.trim().trim_matches('/').to_ascii_lowercase();
    if repo_full_name.is_empty() || !repo_full_name.contains('/') {
        return None;
    }
    Some((repo_full_name, discussion_number))
}

pub fn announcement_translation_metadata_text(
    repo_full_name: &str,
    discussion_number: Option<i64>,
) -> String {
    let mut metadata = repo_full_name.trim().to_owned();
    if let Some(discussion_number) = discussion_number {
        metadata.push('\n');
        metadata.push_str("discussion_number=");
        metadata.push_str(discussion_number.to_string().as_str());
    }
    metadata
}

pub fn parse_announcement_translation_metadata(raw: &str) -> (String, Option<i64>) {
    let mut lines = raw.lines().map(str::trim).filter(|line| !line.is_empty());
    let repo_full_name = lines.next().unwrap_or_default().to_owned();
    let discussion_number = lines.find_map(|line| {
        line.strip_prefix("discussion_number=")
            .and_then(|value| value.trim().parse::<i64>().ok())
    });
    (repo_full_name, discussion_number)
}

pub(crate) fn release_detail_source_hash(
    repo_full_name: &str,
    original_title: &str,
    original_body: &str,
) -> String {
    let normalized_body = original_body.replace("\r\n", "\n");
    let source = format!(
        "v=1\nkind=release_detail\nrepo={}\ntitle={}\nbody={}\n",
        repo_full_name.trim(),
        original_title.trim(),
        normalized_body.trim(),
    );
    sha256_hex(&source)
}

pub(crate) fn release_feed_translation_source_hash(
    repo_full_name: &str,
    title: &str,
    body: Option<&str>,
) -> String {
    sha256_hex(&format!(
        "v=5\nkind=release\nrepo={}\ntitle={}\nbody={}\n",
        repo_full_name.trim(),
        title.trim(),
        body.unwrap_or("").trim(),
    ))
}

pub(crate) fn announcement_detail_source_hash(
    repo_full_name: &str,
    discussion_number: Option<i64>,
    original_title: &str,
    original_body: &str,
) -> String {
    let normalized_body = original_body.replace("\r\n", "\n");
    let source = format!(
        "v=1\nkind=announcement_detail\nrepo={}\ndiscussion_number={}\ntitle={}\nbody={}\n",
        repo_full_name.trim().to_ascii_lowercase(),
        discussion_number
            .map(|value| value.to_string())
            .unwrap_or_default(),
        original_title.trim(),
        normalized_body.trim(),
    );
    sha256_hex(&source)
}

pub(crate) fn announcement_feed_translation_source_hash(
    repo_full_name: &str,
    discussion_number: Option<i64>,
    title: &str,
    body: Option<&str>,
) -> String {
    sha256_hex(&format!(
        "v=1\nkind=announcement\nrepo={}\ndiscussion_number={}\ntitle={}\nbody={}\n",
        repo_full_name.trim().to_ascii_lowercase(),
        discussion_number
            .map(|value| value.to_string())
            .unwrap_or_default(),
        title.trim(),
        body.unwrap_or("").trim(),
    ))
}

pub(crate) fn notification_source_revision_tiebreak(
    thread_id: &str,
    repo_full_name: Option<&str>,
    subject_title: Option<&str>,
    reason: Option<&str>,
    subject_type: Option<&str>,
) -> String {
    format!(
        "thread={thread_id}\nrepo={}\ntitle={}\nreason={}\nsubject_type={}",
        repo_full_name.unwrap_or_default(),
        subject_title.unwrap_or_default(),
        reason.unwrap_or_default(),
        subject_type.unwrap_or_default(),
    )
}

pub(crate) fn release_source_revision_content_tiebreak(parts: &[&str]) -> String {
    let evidence_rank = if parts.iter().any(|part| {
        let normalized = part.to_ascii_lowercase();
        normalized.contains("compare evidence is unavailable")
            || normalized.contains("truncated or paginated")
            || normalized.contains("compare_unavailable")
            || normalized.contains("github_compare_incomplete")
    }) {
        0
    } else {
        1
    };
    format!("evidence_rank={evidence_rank}\n{}", parts.join("\n"))
}

pub(crate) fn authoritative_release_revision<'a>(
    updated_at: &'a str,
    detected_at: Option<&str>,
) -> Option<&'a str> {
    let updated_at = updated_at.trim();
    detected_at
        .filter(|detected_at| !detected_at.trim().is_empty() && *detected_at != updated_at)
        .and((!updated_at.is_empty()).then_some(updated_at))
}

/// Canonical content-processing source hash of one translation request item.
///
/// `source_observed_at` and `source_revision_tiebreak` blocks are revision
/// bookkeeping, not source text, so they never take part in the identity.
pub(crate) fn source_hash(item: &TranslationRequestItemInput) -> Result<String> {
    let source_blocks = item
        .source_blocks
        .iter()
        .filter(|block| {
            block.slot != "source_observed_at" && block.slot != "source_revision_tiebreak"
        })
        .collect::<Vec<_>>();
    let source = serde_json::to_string(&json!({
        "kind": item.kind,
        "variant": item.variant,
        "entity_id": item.entity_id,
        "target_lang": item.target_lang,
        "source_blocks": source_blocks,
        "target_slots": item.target_slots,
    }))?;
    Ok(sha256_hex(&format!("{GLOBAL_PROTOCOL_VERSION}\n{source}")))
}

pub(crate) fn source_hash_for_item(item: &TranslationRequestItemInput) -> String {
    source_hash(item).expect("translation request source fields are serializable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_detail_source_hash_changes_with_content() {
        let hash1 = release_detail_source_hash("acme/repo", "v1.0.0", "line one");
        let hash2 = release_detail_source_hash("acme/repo", "v1.0.0", "line two");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn release_detail_source_hash_normalizes_line_endings_and_whitespace() {
        let hash1 = release_detail_source_hash("acme/repo", " v1.0.0 ", "line one\r\nline two");
        let hash2 = release_detail_source_hash("acme/repo", "v1.0.0", "line one\nline two");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn authoritative_release_revision_prefers_distinct_detected_at() {
        assert_eq!(
            authoritative_release_revision("2026-01-01T00:00:00Z", Some("2026-01-01T00:00:01Z")),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            authoritative_release_revision("2026-01-01T00:00:00Z", Some("2026-01-01T00:00:00Z")),
            None
        );
        assert_eq!(
            authoritative_release_revision("2026-01-01T00:00:00Z", None),
            None
        );
    }

    #[test]
    fn release_source_revision_tiebreak_ranks_missing_compare_evidence_low() {
        assert_eq!(
            release_source_revision_content_tiebreak(&["compare evidence is unavailable"]),
            "evidence_rank=0\ncompare evidence is unavailable"
        );
        assert_eq!(
            release_source_revision_content_tiebreak(&["full diff"]),
            "evidence_rank=1\nfull diff"
        );
    }

    #[test]
    fn discussion_key_and_url_parsing_share_one_shape() {
        assert_eq!(announcement_discussion_key("Octo/Demo", 42), "octo/demo#42");
        assert_eq!(
            parse_discussion_number_from_github_url("https://github.com/octo/demo/discussions/42"),
            Some(42)
        );
        assert_eq!(
            parse_discussion_number_from_github_url("https://example.com/octo/demo/discussions/42"),
            None
        );
    }
}
pub(crate) fn release_smart_metadata_text(
    repo_full_name: &str,
    tag_name: &str,
    previous_tag_name: Option<&str>,
) -> String {
    let mut metadata = format!("repo={repo_full_name}\nhead_tag={tag_name}");
    if let Some(previous_tag_name) = previous_tag_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        metadata.push('\n');
        metadata.push_str("compare_base_tag=");
        metadata.push_str(previous_tag_name);
    }
    metadata
}

pub(crate) fn release_smart_feed_source_hash(
    entity_id: &str,
    repo_full_name: &str,
    title: &str,
    body: Option<&str>,
    tag_name: &str,
    previous_tag_name: Option<&str>,
) -> String {
    let mut source_blocks = vec![TranslationSourceBlock {
        slot: "title".to_owned(),
        text: title.trim().to_owned(),
    }];
    if let Some(body) = body.map(str::trim).filter(|value| !value.is_empty()) {
        source_blocks.push(TranslationSourceBlock {
            slot: "body_markdown".to_owned(),
            text: body.to_owned(),
        });
    }
    source_blocks.push(TranslationSourceBlock {
        slot: "metadata".to_owned(),
        text: release_smart_metadata_text(repo_full_name, tag_name, previous_tag_name),
    });

    crate::translations::build_source_hash(&TranslationRequestItemInput {
        producer_ref: format!("feed.smart:release:{entity_id}"),
        kind: "release_smart".to_owned(),
        variant: "feed_card".to_owned(),
        entity_id: entity_id.to_owned(),
        target_lang: "zh-CN".to_owned(),
        max_wait_ms: 0,
        source_blocks,
        target_slots: vec!["title_zh".to_owned(), "body_md".to_owned()],
    })
}

pub(crate) fn announcement_smart_feed_source_hash(
    entity_id: &str,
    repo_full_name: &str,
    discussion_number: Option<i64>,
    title: &str,
    body: Option<&str>,
) -> String {
    let mut source_blocks = vec![TranslationSourceBlock {
        slot: "title".to_owned(),
        text: title.trim().to_owned(),
    }];
    if let Some(body) = body.map(str::trim).filter(|value| !value.is_empty()) {
        source_blocks.push(TranslationSourceBlock {
            slot: "body_markdown".to_owned(),
            text: body.to_owned(),
        });
    }
    source_blocks.push(TranslationSourceBlock {
        slot: "metadata".to_owned(),
        text: crate::content_source_identity::announcement_translation_metadata_text(
            repo_full_name,
            discussion_number,
        ),
    });

    crate::translations::build_source_hash(&TranslationRequestItemInput {
        producer_ref: format!("feed.smart:announcement:{entity_id}"),
        kind: "announcement_smart".to_owned(),
        variant: "feed_card".to_owned(),
        entity_id: entity_id.to_owned(),
        target_lang: "zh-CN".to_owned(),
        max_wait_ms: 0,
        source_blocks,
        target_slots: vec!["title_zh".to_owned(), "body_md".to_owned()],
    })
}

/// Canonical work identity shared by writers and readers.
#[derive(Debug, Clone)]
pub(crate) struct ContentWorkIdentity {
    pub canonical_resource_type: String,
    pub canonical_resource_id: String,
    pub pipeline: String,
    pub variant: String,
    pub target_lang: String,
    pub source_hash: String,
    pub protocol_version: String,
}

pub(crate) fn identity_id_for(key: &ContentWorkIdentity) -> Result<String> {
    let encoded = serde_json::to_string(&(
        key.canonical_resource_type.as_str(),
        key.canonical_resource_id.as_str(),
        key.pipeline.as_str(),
        key.variant.as_str(),
        key.target_lang.as_str(),
        key.source_hash.as_str(),
        key.protocol_version.as_str(),
    ))?;
    Ok(format!(
        "content-work-{}",
        crate::content_source_identity::sha256_hex(&format!("content-work-identity.v1\n{encoded}"))
    ))
}
