//! Markdown shape checks shared by the live translation paths.
//!
//! Translation and polishing may rewrite prose but must keep the source
//! document's block structure and inline markers. The legacy SQLite runtime and
//! the PostgreSQL runtime both gate on the same predicate, so it lives here
//! instead of in either read path.

/// Classify the block form of one non-empty markdown line.
fn line_prefix_kind(line: &str) -> &'static str {
    let t = line.trim();
    if t.is_empty() {
        return "blank";
    }
    if t.starts_with('#') {
        return "heading";
    }
    if t.starts_with("> ") || t == ">" {
        return "blockquote";
    }
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return "ul";
    }
    if let Some((head, tail)) = t.split_once('.')
        && !head.is_empty()
        && head.chars().all(|c| c.is_ascii_digit())
        && tail.starts_with(' ')
    {
        return "ol";
    }
    "plain"
}

/// Whether `translated` keeps the block structure and inline emphasis/code
/// markers of `source`.
pub(crate) fn markdown_structure_preserved(source: &str, translated: &str) -> bool {
    let normalized_source = source.replace("\r\n", "\n");
    let src_lines: Vec<&str> = normalized_source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    let normalized_translated = translated.replace("\r\n", "\n");
    let dst_lines: Vec<&str> = normalized_translated
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if src_lines.is_empty() {
        return true;
    }
    if dst_lines.is_empty() || src_lines.len() != dst_lines.len() {
        return false;
    }

    src_lines.iter().zip(dst_lines.iter()).all(|(s, d)| {
        if line_prefix_kind(s) != line_prefix_kind(d) {
            return false;
        }

        let src_bold_pairs = s.matches("**").count() / 2;
        let dst_bold_pairs = d.matches("**").count() / 2;
        if src_bold_pairs > 0 && dst_bold_pairs == 0 {
            return false;
        }

        let src_code_pairs = s.matches('`').count() / 2;
        let dst_code_pairs = d.matches('`').count() / 2;
        if src_code_pairs > 0 && dst_code_pairs == 0 {
            return false;
        }
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_structure_requires_inline_markers() {
        let source = "- **Nightly** build from `main`\n- Keep **bold** marker";
        let translated_missing = "- 夜间构建来自 main\n- 请保留强调";
        let translated_ok = "- **夜间**构建来自 `main`\n- 请保留 **强调** 标记";
        assert!(!markdown_structure_preserved(source, translated_missing));
        assert!(markdown_structure_preserved(source, translated_ok));
    }
}
