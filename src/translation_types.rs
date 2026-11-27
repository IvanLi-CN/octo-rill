//! Wire types for translation results, including the structured release
//! reading payload.
//!
//! The SQLite translation runtime and the PostgreSQL read surfaces return the
//! same result shape, so the type lives here rather than in the retiring
//! scheduler module.

use serde::{Deserialize, Serialize};

use crate::api::AdminBusinessOutcome;
use url::Url;

/// Structured, human-facing facts extracted from a Release body and/or its
/// frozen compare evidence. The evidence field is retained in the projection
/// for internal audit only and is never exposed by the public reading views.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseReadingResult {
    pub version: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub changes: Vec<ReleaseReadingChange>,
    #[serde(default)]
    pub machine_materials: Vec<ReleaseReadingMaterial>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<ReleaseReadingEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseReadingChange {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ReleaseReadingReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseReadingReference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseReadingMaterial {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseReadingEvidence {
    pub kind: String,
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl ReleaseReadingResult {
    pub fn is_changes(&self) -> bool {
        self.outcome == "changes"
    }

    pub fn is_insufficient(&self) -> bool {
        self.outcome == "insufficient"
    }

    /// Normalize the only clickable references allowed in human-facing Release
    /// reading content. Legacy model output may still contain `[PR#123](url)`
    /// in the change text; move those links into the controlled reference
    /// field so every projection can render the same label and URL safely.
    pub fn normalize_references(&mut self) -> Result<(), &'static str> {
        for change in &mut self.changes {
            let mut text = String::with_capacity(change.text.len());
            let mut references = change.references.clone();
            let mut cursor = 0usize;
            while let Some(open_offset) = change.text[cursor..].find('[') {
                let open = cursor + open_offset;
                text.push_str(&change.text[cursor..open]);
                let Some(label_end_offset) = change.text[open + 1..].find(']') else {
                    text.push_str(&change.text[open..]);
                    cursor = change.text.len();
                    break;
                };
                let label_end = open + 1 + label_end_offset;
                let label = &change.text[open + 1..label_end];
                let Some(target_start) = change.text[label_end + 1..].strip_prefix('(') else {
                    text.push_str(&change.text[open..=label_end]);
                    cursor = label_end + 1;
                    continue;
                };
                let Some(target_end_offset) = target_start.find(')') else {
                    return Err("reading change contains an incomplete reference");
                };
                let target_end = label_end + 1 + 1 + target_end_offset;
                let url = target_start[..target_end_offset].trim();
                let Some(label_number) = label
                    .strip_prefix("PR#")
                    .and_then(|value| value.parse::<u64>().ok().filter(|number| *number > 0))
                else {
                    return Err("reading change contains an unsupported link label");
                };
                let Some(url_number) = release_reading_pr_number(url) else {
                    return Err("reading change contains an unsupported reference URL");
                };
                if label_number != url_number {
                    return Err("reading PR reference label does not match its URL");
                }
                references.push(ReleaseReadingReference {
                    label: Some(format!("PR#{url_number}")),
                    url: url.to_owned(),
                    number: Some(url_number),
                });
                cursor = target_end + 1;
            }
            if cursor < change.text.len() {
                text.push_str(&change.text[cursor..]);
            }
            change.text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            references.sort_by(|left, right| left.url.cmp(&right.url));
            references.dedup_by(|left, right| left.url == right.url);
            for reference in &mut references {
                if let Some(number) = release_reading_pr_number(&reference.url) {
                    reference.number = Some(number);
                    reference.label = Some(format!("PR#{number}"));
                } else {
                    let label = reference
                        .label
                        .as_deref()
                        .map(str::trim)
                        .filter(|label| !label.is_empty())
                        .ok_or("reading non-PR references require a label")?;
                    if !release_reading_reference_url_is_allowed(&reference.url)
                        || release_reading_text_is_low_value(label)
                    {
                        return Err("reading contains an unsupported reference");
                    }
                    reference.label = Some(label.to_owned());
                    reference.number = None;
                }
            }
            change.references = references;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version.trim().is_empty() {
            return Err("reading.version must be non-empty");
        }
        if !matches!(self.outcome.as_str(), "changes" | "insufficient") {
            return Err("reading.outcome must be changes or insufficient");
        }
        if self.title.as_deref().is_some_and(|title| {
            let text = title.trim();
            text.is_empty() || release_reading_text_is_low_value(text)
        }) {
            return Err("reading.title must contain a useful change title");
        }
        if self.changes.iter().any(|change| {
            let text = change.text.trim();
            text.is_empty() || release_reading_text_is_low_value(text)
        }) {
            return Err("reading.changes must contain readable text");
        }
        if self
            .machine_materials
            .iter()
            .any(|material| material.label.trim().is_empty() || material.value.trim().is_empty())
        {
            return Err("reading.machine_materials must contain labels and values");
        }
        if self
            .changes
            .iter()
            .flat_map(|change| change.references.iter())
            .any(|reference| {
                if let Some(number) = release_reading_pr_number(&reference.url) {
                    reference.number != Some(number)
                        || reference.label.as_deref() != Some(format!("PR#{number}").as_str())
                } else {
                    reference.number.is_some()
                        || reference.label.as_deref().is_none_or(|label| {
                            label.trim().is_empty() || release_reading_text_is_low_value(label)
                        })
                        || !release_reading_reference_url_is_allowed(&reference.url)
                }
            })
        {
            return Err("reading references must be controlled labeled links");
        }
        if self.is_changes() && self.changes.is_empty() {
            return Err("reading.changes must be non-empty for changes outcome");
        }
        if self.is_insufficient() && !self.changes.is_empty() {
            return Err("reading.changes must be empty for insufficient outcome");
        }
        if self.is_insufficient() && !self.machine_materials.is_empty() {
            return Err("reading.machine_materials must be empty for insufficient outcome");
        }
        Ok(())
    }
}

pub fn release_reading_pr_number(raw: &str) -> Option<u64> {
    let url = Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("github.com") | Some("www.github.com"))
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    if segments.len() != 4 || segments[2] != "pull" {
        return None;
    }
    segments[3].parse::<u64>().ok().filter(|number| *number > 0)
}

fn release_reading_reference_url_is_allowed(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw.trim()) else {
        return false;
    };
    url.scheme() == "https"
        && url.query().is_none()
        && url.fragment().is_none()
        && matches!(
            url.host_str(),
            Some("github.com") | Some("www.github.com") | Some("docs.github.com")
        )
        && url.path() != "/"
}

fn release_reading_text_is_low_value(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    let version_only = normalized
        .strip_prefix("release ")
        .or_else(|| normalized.strip_prefix("version "))
        .unwrap_or(&normalized)
        .trim_start_matches('v');
    normalized.starts_with("相关链接")
        || normalized.starts_with("来源")
        || normalized.starts_with("实现依据")
        || normalized.starts_with("发布原文")
        || (normalized.starts_with("由 ") && normalized.contains("提交"))
        || normalized.starts_with("http://")
        || normalized.starts_with("https://")
        || semver::Version::parse(version_only).is_ok()
        || matches!(
            normalized.as_str(),
            "版本更新" | "更新要点" | "发布说明" | "版本发布" | "稳定版更新" | "补丁更新"
        )
}

/// One translation result as returned by both runtimes.
#[derive(Debug, Clone, Serialize)]
pub struct TranslationResultItem {
    pub producer_ref: String,
    pub entity_id: String,
    pub kind: String,
    pub variant: String,
    pub status: String,
    pub title_zh: Option<String>,
    pub summary_md: Option<String>,
    pub body_md: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading: Option<ReleaseReadingResult>,
    pub error: Option<String>,
    pub work_item_id: Option<String>,
    pub batch_id: Option<String>,
}

fn translation_batch_result_summary_total(
    summary: &crate::translation_admin_types::AdminTranslationBatchResultSummary,
) -> i64 {
    summary.ready
        + summary.error
        + summary.missing
        + summary.disabled
        + summary.queued
        + summary.running
}

pub(crate) fn translation_batch_business_outcome(
    status: &str,
    summary: &crate::translation_admin_types::AdminTranslationBatchResultSummary,
    expected_total: i64,
    error_text: Option<&str>,
) -> AdminBusinessOutcome {
    let total = translation_batch_result_summary_total(summary).max(expected_total.max(0));
    let result_pending = summary.queued > 0 || summary.running > 0;

    match status {
        "failed" => AdminBusinessOutcome {
            code: "failed".to_owned(),
            label: "业务失败".to_owned(),
            message: error_text
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("批次执行失败，未产出可用结果。")
                .to_owned(),
        },
        "queued" | "running" => AdminBusinessOutcome {
            code: "unknown".to_owned(),
            label: "处理中".to_owned(),
            message: "批次仍在执行中，业务结果尚未稳定。".to_owned(),
        },
        _ if total <= 0 => AdminBusinessOutcome {
            code: "unknown".to_owned(),
            label: "结果未知".to_owned(),
            message: "批次已结束，但没有可汇总的条目结果。".to_owned(),
        },
        _ if summary.disabled == total => AdminBusinessOutcome {
            code: "disabled".to_owned(),
            label: "AI 已禁用".to_owned(),
            message: "批次条目全部命中禁用路径，未执行翻译。".to_owned(),
        },
        _ if summary.error > 0 || summary.missing > 0 => AdminBusinessOutcome {
            code: "partial".to_owned(),
            label: "部分成功".to_owned(),
            message: "批次已完成，但仍有条目失败或缺失。".to_owned(),
        },
        _ if result_pending => AdminBusinessOutcome {
            code: "partial".to_owned(),
            label: "部分成功".to_owned(),
            message: "批次已标记完成，但仍有条目未收敛。".to_owned(),
        },
        _ if summary.ready == total => AdminBusinessOutcome {
            code: "ok".to_owned(),
            label: "业务成功".to_owned(),
            message: "批次条目已全部成功完成。".to_owned(),
        },
        _ if summary.ready > 0 && summary.ready + summary.disabled == total => {
            AdminBusinessOutcome {
                code: "ok".to_owned(),
                label: "业务成功".to_owned(),
                message: "批次已完成；可翻译条目成功完成，其余条目命中禁用路径。".to_owned(),
            }
        }
        _ => AdminBusinessOutcome {
            code: "unknown".to_owned(),
            label: "结果未知".to_owned(),
            message: "批次已结束，但缺少稳定的业务结果统计。".to_owned(),
        },
    }
}
