//! LLM provider contract shared by the live AI and admin paths.
//!
//! Failure classification and the built-in model input limits are part of the
//! contract the live runtime depends on, not of the retiring `ai` module's
//! provider transport.

use std::collections::HashMap;
use std::sync::LazyLock;

pub(crate) const MODEL_LIMIT_UNKNOWN_FALLBACK: u32 = 32_768;
pub(crate) const MODEL_LIMIT_RESOLUTION_BUILTIN_CATALOG: &str = "builtin_catalog";
pub(crate) const MODEL_LIMIT_RESOLUTION_UNKNOWN_FALLBACK: &str = "unknown_fallback";
pub(crate) const AI_RESPONSE_MISSING_CONTENT_ERROR: &str = "AI response missing content";

static BUILTIN_MODEL_LIMITS: LazyLock<HashMap<String, u32>> = LazyLock::new(|| {
    let mut out = HashMap::new();
    for (model, limit) in [
        ("openai/gpt-5", 272_000u32),
        ("openai/gpt-5-mini", 272_000),
        ("openai/gpt-5-nano", 272_000),
        ("openai/gpt-4o", 128_000),
        ("openai/gpt-4o-mini", 128_000),
        ("openai/gpt-4.1", 1_047_576),
        ("openai/gpt-4.1-mini", 1_047_576),
        ("anthropic/claude-3.5-haiku", 200_000),
        ("anthropic/claude-3.7-sonnet", 200_000),
        ("anthropic/claude-sonnet-4", 200_000),
        ("google/gemini-2.5-flash-lite", 1_048_576),
        ("google/gemini-2.5-flash", 1_048_576),
        ("google/gemini-2.5-pro", 1_048_576),
        ("deepseek/deepseek-chat-v3.1", 32_768),
        ("deepseek/deepseek-r1-0528", 128_000),
        ("qwen/qwen3-coder", 262_144),
    ] {
        insert_model_limit(&mut out, model, limit);
    }
    out
});

/// Coarse failure class attached to an LLM call so retry and admin surfaces can
/// agree on one vocabulary instead of matching upstream prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmFailureClass {
    EmptyContent,
    Transient,
    RateLimited,
    Configuration,
}

impl LlmFailureClass {
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "empty_content" => Some(Self::EmptyContent),
            "transient" => Some(Self::Transient),
            "rate_limited" => Some(Self::RateLimited),
            "configuration" => Some(Self::Configuration),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyContent => "empty_content",
            Self::Transient => "transient",
            Self::RateLimited => "rate_limited",
            Self::Configuration => "configuration",
        }
    }

    pub const fn is_recoverable(self) -> bool {
        !matches!(self, Self::Configuration)
    }

    pub const fn safe_message(self) -> &'static str {
        match self {
            Self::EmptyContent => AI_RESPONSE_MISSING_CONTENT_ERROR,
            Self::Transient => "LLM upstream temporarily unavailable",
            Self::RateLimited => "LLM upstream rate limited",
            Self::Configuration => "LLM configuration or request rejected",
        }
    }
}

impl std::fmt::Display for LlmFailureClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn normalize_model_name(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn is_date_suffix(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.len() != 11 {
        return false;
    }
    if bytes[0] != b'-' || bytes[5] != b'-' || bytes[8] != b'-' {
        return false;
    }
    bytes
        .iter()
        .enumerate()
        .all(|(idx, b)| matches!(idx, 0 | 5 | 8) || b.is_ascii_digit())
}

fn strip_model_suffix(raw: &str) -> String {
    let mut out = raw.to_owned();
    if let Some(stripped) = out.strip_suffix("-latest") {
        out = stripped.to_owned();
    }
    if let Some(stripped) = out.strip_suffix("-preview") {
        out = stripped.to_owned();
    }
    if out.len() > 11 {
        let suffix = &out[out.len() - 11..];
        if is_date_suffix(suffix) {
            out.truncate(out.len() - 11);
        }
    }
    out
}

fn model_aliases(raw: &str) -> Vec<String> {
    let normalized = normalize_model_name(raw);
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut aliases = Vec::new();
    aliases.push(normalized.clone());

    if let Some(tail) = normalized.rsplit('/').next() {
        aliases.push(tail.to_owned());
    }
    if let Some(tail) = normalized.rsplit('.').next() {
        aliases.push(tail.to_owned());
    }

    let mut expanded = Vec::new();
    for alias in aliases {
        let stripped = strip_model_suffix(&alias);
        expanded.push(alias);
        if !stripped.is_empty() {
            expanded.push(stripped);
        }
    }

    let mut uniq = std::collections::HashSet::new();
    expanded
        .into_iter()
        .filter(|alias| !alias.is_empty())
        .filter(|alias| uniq.insert(alias.clone()))
        .collect()
}

pub(crate) fn insert_model_limit(map: &mut HashMap<String, u32>, raw_model: &str, limit: u32) {
    if limit == 0 {
        return;
    }
    for alias in model_aliases(raw_model) {
        let entry = map.entry(alias).or_insert(limit);
        if limit < *entry {
            *entry = limit;
        }
    }
}

pub(crate) fn lookup_model_limit_in_map(map: &HashMap<String, u32>, model: &str) -> Option<u32> {
    for alias in model_aliases(model) {
        if let Some(limit) = map.get(&alias) {
            return Some(*limit);
        }
    }
    None
}

/// Static built-in model catalog, keyed by every alias of each model name.
pub(crate) fn builtin_model_limits() -> &'static HashMap<String, u32> {
    &BUILTIN_MODEL_LIMITS
}

/// Static context limit for `model`, with the resolution source that produced
/// it. Callers that can reach the admin override or the synced catalog layer
/// their own lookup on top of this fallback.
pub(crate) fn resolve_model_input_limit_for_model(model: &str) -> (u32, &'static str) {
    if model.is_empty() {
        return (
            MODEL_LIMIT_UNKNOWN_FALLBACK,
            MODEL_LIMIT_RESOLUTION_UNKNOWN_FALLBACK,
        );
    }

    if let Some(limit) = lookup_model_limit_in_map(builtin_model_limits(), model) {
        return (limit.max(1), MODEL_LIMIT_RESOLUTION_BUILTIN_CATALOG);
    }

    (
        MODEL_LIMIT_UNKNOWN_FALLBACK,
        MODEL_LIMIT_RESOLUTION_UNKNOWN_FALLBACK,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_class_round_trips_and_carries_safe_messages() {
        for class in [
            LlmFailureClass::EmptyContent,
            LlmFailureClass::Transient,
            LlmFailureClass::RateLimited,
            LlmFailureClass::Configuration,
        ] {
            assert_eq!(LlmFailureClass::from_str(class.as_str()), Some(class));
            assert!(!class.safe_message().is_empty());
        }
        assert_eq!(LlmFailureClass::from_str("bogus"), None);
        assert!(!LlmFailureClass::Configuration.is_recoverable());
        assert!(LlmFailureClass::Transient.is_recoverable());
    }

    #[test]
    fn model_input_limit_resolves_aliases_and_falls_back() {
        assert_eq!(
            resolve_model_input_limit_for_model("openai/gpt-4o"),
            (128_000, MODEL_LIMIT_RESOLUTION_BUILTIN_CATALOG)
        );
        assert_eq!(
            resolve_model_input_limit_for_model("gpt-4o-2024-08-06").0,
            128_000
        );
        assert_eq!(
            resolve_model_input_limit_for_model("vendor/unknown-model"),
            (
                MODEL_LIMIT_UNKNOWN_FALLBACK,
                MODEL_LIMIT_RESOLUTION_UNKNOWN_FALLBACK
            )
        );
        assert_eq!(
            resolve_model_input_limit_for_model(""),
            (
                MODEL_LIMIT_UNKNOWN_FALLBACK,
                MODEL_LIMIT_RESOLUTION_UNKNOWN_FALLBACK
            )
        );
    }
}
