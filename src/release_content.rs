//! Release body shaping shared by every live release read path.
//!
//! Feed cards, release detail and translation submission all normalize and
//! truncate the same release body. Keeping one implementation here replaces the
//! duplicated copy in the retiring `api` module and the PostgreSQL admin
//! dashboard.

use std::borrow::Cow;

/// Hard cap on the release body length carried into feed/translation payloads.
pub(crate) const RELEASE_FEED_BODY_MAX_CHARS: usize = 3_000;

fn truncate_chars(s: &str, max_chars: usize) -> Cow<'_, str> {
    if s.chars().count() > max_chars {
        Cow::Owned(s.chars().take(max_chars).collect())
    } else {
        Cow::Borrowed(s)
    }
}

/// Normalize line endings, trim, and truncate a release body for feed use.
pub(crate) fn release_feed_body(body: Option<&str>) -> Option<String> {
    let normalized = body?.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(trimmed, RELEASE_FEED_BODY_MAX_CHARS).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_feed_body_normalizes_and_falls_back_to_none() {
        assert_eq!(release_feed_body(None), None);
        assert_eq!(release_feed_body(Some("  \r\n\t ")), None);
        assert_eq!(release_feed_body(Some("a\r\nb  ")), Some("a\nb".to_owned()));
    }

    #[test]
    fn release_feed_body_truncates_to_the_char_cap() {
        let long = "a".repeat(RELEASE_FEED_BODY_MAX_CHARS + 24);
        let trimmed = release_feed_body(Some(long.as_str())).expect("body");
        assert_eq!(trimmed.chars().count(), RELEASE_FEED_BODY_MAX_CHARS);
    }
}
