//! Translation failure classification shared by the live admin and read
//! surfaces.
//!
//! Retry decisions, admin error fields and the public result payload all need
//! the same code/summary pair for one raw error string. Keeping one
//! implementation here replaces the copy in the retiring scheduler module.

/// Error text marking a polished result with no valuable version information.
/// Read paths map it to a terminal "missing" state instead of a retryable error.
pub(crate) const SMART_NO_VALUABLE_VERSION_INFO: &str = "no_valuable_version_info";

#[derive(Debug, Clone)]
pub struct ClassifiedTranslationError {
    pub code: &'static str,
    pub summary: &'static str,
    pub detail: String,
}

pub fn classify_translation_error(error_text: Option<&str>) -> Option<ClassifiedTranslationError> {
    let detail = error_text
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_owned();
    let normalized = detail.to_ascii_lowercase();

    let (code, summary) = if normalized == "ai response missing content" {
        ("empty_content", "模型输出为空")
    } else if normalized == "llm upstream temporarily unavailable" {
        ("transient", "上游暂时不可用")
    } else if normalized == "llm upstream rate limited" {
        ("rate_limited", "上游请求被限流")
    } else if normalized == "llm configuration or request rejected" {
        ("configuration", "模型配置或请求被拒绝")
    } else if normalized.contains("runtime_lease_expired") {
        ("runtime_lease_expired", "运行时租约失效")
    } else if normalized.contains("preserve markdown structure")
        || normalized.contains("markdown structure")
    {
        ("markdown_structure_mismatch", "Markdown 结构校验失败")
    } else if normalized.contains("body exceeds 3000 chars")
        || (normalized.contains("3000")
            && normalized.contains("body")
            && normalized.contains("exceed"))
        || (normalized.contains("3000")
            && normalized.contains("body")
            && normalized.contains("too long"))
    {
        ("body_too_long", "正文超过 3000 字符")
    } else if normalized.contains("output_truncated") || normalized.contains("output was truncated")
    {
        ("output_truncated", "模型输出被截断")
    } else if normalized.contains("empty summary") || normalized.contains("empty translation") {
        ("empty_translation", "模型输出为空")
    } else if normalized.contains("ai output did not satisfy")
        || normalized.contains("ai output did not contain usable")
        || normalized.contains("global content output")
    {
        ("output_contract_invalid", "模型输出未通过 JSON 契约")
    } else if normalized.contains("upstream model/channel rejected request")
        || normalized.contains("invalid_model_error")
        || normalized.contains("model not found")
        || normalized.contains("insufficient_quota")
        || normalized.contains("bad key")
        || normalized.contains("ai returned 401")
        || normalized.contains("ai returned 403")
        || normalized.contains("ai returned 429")
    {
        ("upstream_rejected", "上游模型拒绝请求")
    } else {
        ("unknown_internal_error", "翻译失败")
    };

    Some(ClassifiedTranslationError {
        code,
        summary,
        detail,
    })
}

pub fn translation_error_summary(error_text: Option<&str>) -> Option<String> {
    classify_translation_error(error_text).map(|classified| classified.summary.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_translation_error_maps_markdown_mismatch() {
        let classified = classify_translation_error(Some(
            "release detail translation failed to preserve markdown structure",
        ))
        .expect("classified markdown mismatch");

        assert_eq!(classified.code, "markdown_structure_mismatch");
        assert_eq!(classified.summary, "Markdown 结构校验失败");
        assert_eq!(
            classified.detail,
            "release detail translation failed to preserve markdown structure"
        );
    }

    #[test]
    fn classify_translation_error_maps_body_too_long() {
        let classified = classify_translation_error(Some("body exceeds 3000 chars"))
            .expect("classified overlength body");

        assert_eq!(classified.code, "body_too_long");
        assert_eq!(classified.summary, "正文超过 3000 字符");
        assert_eq!(classified.detail, "body exceeds 3000 chars");
    }

    #[test]
    fn classify_translation_error_maps_output_contract_failure() {
        let classified =
            classify_translation_error(Some("AI output did not satisfy the JSON contract"))
                .expect("classified output contract failure");

        assert_eq!(classified.code, "output_contract_invalid");
        assert_eq!(classified.summary, "模型输出未通过 JSON 契约");
    }

    #[test]
    fn classify_translation_error_maps_safe_llm_failure_classes() {
        let cases = [
            (
                "AI response missing content",
                "empty_content",
                "模型输出为空",
            ),
            (
                "LLM upstream temporarily unavailable",
                "transient",
                "上游暂时不可用",
            ),
            (
                "LLM upstream rate limited",
                "rate_limited",
                "上游请求被限流",
            ),
            (
                "LLM configuration or request rejected",
                "configuration",
                "模型配置或请求被拒绝",
            ),
        ];

        for (error, code, summary) in cases {
            let classified = classify_translation_error(Some(error)).expect("classified error");
            assert_eq!(classified.code, code);
            assert_eq!(classified.summary, summary);
            assert_eq!(classified.detail, error);
        }
    }
}
