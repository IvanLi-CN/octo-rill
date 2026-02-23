use std::collections::HashSet;

use anyhow::{Context, Result, anyhow};
use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, offset::LocalResult};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::state::AppState;

#[derive(Debug, sqlx::FromRow)]
struct ReleaseRow {
    release_id: i64,
    full_name: String,
    tag_name: String,
    name: Option<String>,
    published_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

fn truncate_chars_lossy(bytes: &[u8], max_chars: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn extract_error_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    // OpenAI-style: { "error": { "message": "..." } }
    if let Some(msg) = value
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Some(msg.to_owned());
    }
    // Fallback: { "message": "..." }
    if let Some(msg) = value
        .get("message")
        .and_then(|m| m.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Some(msg.to_owned());
    }
    None
}

fn extract_chat_content(value: &Value) -> Option<String> {
    let choices = value.get("choices")?.as_array()?;
    for choice in choices {
        if let Some(text) = choice
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(text.to_owned());
        }

        if let Some(parts) = choice
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        {
            let mut joined = Vec::new();
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
                if kind != "text" && kind != "output_text" && !kind.is_empty() {
                    continue;
                }
                if let Some(seg) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    joined.push(seg);
                }
            }
            if !joined.is_empty() {
                return Some(joined.join("\n"));
            }
        }

        if let Some(text) = choice
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(text.to_owned());
        }
    }
    None
}

fn extract_refusal_message(value: &Value) -> Option<String> {
    let choices = value.get("choices")?.as_array()?;
    for choice in choices {
        if let Some(refusal) = choice
            .get("message")
            .and_then(|m| m.get("refusal"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(refusal.to_owned());
        }
    }
    None
}

pub fn sha256_hex(input: &str) -> String {
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

fn parse_release_id_from_link_href(href: &str) -> Option<i64> {
    let href = href.trim().trim_start_matches('<').trim_end_matches('>');
    if href.is_empty() {
        return None;
    }

    let parsed = if href.starts_with("http://") || href.starts_with("https://") {
        Url::parse(href).ok()?
    } else if href.starts_with('/') || href.starts_with('?') {
        let base = Url::parse("https://octo-rill.local/").ok()?;
        base.join(href).ok()?
    } else {
        return None;
    };

    for (k, v) in parsed.query_pairs() {
        if k == "release" {
            return v.parse::<i64>().ok();
        }
    }
    None
}

fn escape_markdown_link_text(text: &str) -> String {
    text.replace('[', "\\[").replace(']', "\\]")
}

fn extract_brief_release_ids(markdown: &str) -> HashSet<i64> {
    let mut ids = HashSet::new();
    let mut cursor = 0usize;
    while let Some(rel) = markdown[cursor..].find("](") {
        let href_start = cursor + rel + 2;
        let Some(end_rel) = markdown[href_start..].find(')') else {
            break;
        };
        let href_end = href_start + end_rel;
        if let Some(id) = parse_release_id_from_link_href(&markdown[href_start..href_end]) {
            ids.insert(id);
        }
        cursor = href_end + 1;
    }
    ids
}

fn reconcile_brief_release_links(markdown: &str, targets: &[ReleaseRow]) -> String {
    let existing = extract_brief_release_ids(markdown);
    let mut missing = targets
        .iter()
        .filter(|r| !existing.contains(&r.release_id))
        .collect::<Vec<_>>();

    if missing.is_empty() {
        return markdown.to_owned();
    }

    // Keep generated links stable and deterministic.
    missing.sort_by_key(|r| r.release_id);

    let mut out = markdown.trim_end_matches('\n').to_owned();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("## Release links\n");
    for r in missing {
        let title = r
            .name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&r.tag_name);
        let label = escape_markdown_link_text(&format!("{} · {}", r.full_name, title));
        out.push_str(&format!(
            "- [{label}](/?tab=briefs&release={})\n",
            r.release_id
        ));
    }
    out
}

fn compute_daily_window_naive(
    now_local: NaiveDateTime,
    at: NaiveTime,
) -> (NaiveDateTime, NaiveDateTime, NaiveDate) {
    let today = now_local.date();
    let mut end = NaiveDateTime::new(today, at);
    // Use the latest boundary <= now.
    if now_local < end {
        end -= chrono::Duration::days(1);
    }
    let start = end - chrono::Duration::hours(24);
    (start, end, end.date())
}

fn local_from_naive(naive: NaiveDateTime) -> chrono::DateTime<Local> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        // If DST makes the time ambiguous, prefer the earliest.
        LocalResult::Ambiguous(dt, _) => dt,
        // This shouldn't happen for typical daily times like 08:00. Fallback to a
        // non-panicking conversion for robustness.
        LocalResult::None => {
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
                .with_timezone(&Local)
        }
    }
}

pub struct DailyWindow {
    pub key_date: NaiveDate,
    pub start_local: chrono::DateTime<Local>,
    pub end_local: chrono::DateTime<Local>,
    pub start_utc: chrono::DateTime<chrono::Utc>,
    pub end_utc: chrono::DateTime<chrono::Utc>,
}

pub fn compute_window_for_key_date(
    key_date: NaiveDate,
    at: NaiveTime,
) -> (chrono::DateTime<Local>, chrono::DateTime<Local>) {
    let end_naive = NaiveDateTime::new(key_date, at);
    let start_naive = end_naive - chrono::Duration::hours(24);
    (local_from_naive(start_naive), local_from_naive(end_naive))
}

pub fn compute_daily_window(at: Option<NaiveTime>, now: chrono::DateTime<Local>) -> DailyWindow {
    let now_naive = now.naive_local();
    if let Some(at) = at {
        let (start_naive, end_naive, key_date) = compute_daily_window_naive(now_naive, at);
        let start_local = local_from_naive(start_naive);
        let end_local = local_from_naive(end_naive);
        return DailyWindow {
            key_date,
            start_utc: start_local.with_timezone(&chrono::Utc),
            end_utc: end_local.with_timezone(&chrono::Utc),
            start_local,
            end_local,
        };
    }

    let end_local = now;
    let start_local = end_local - chrono::Duration::hours(24);
    DailyWindow {
        key_date: end_local.date_naive(),
        start_utc: start_local.with_timezone(&chrono::Utc),
        end_utc: end_local.with_timezone(&chrono::Utc),
        start_local,
        end_local,
    }
}

pub async fn chat_completion(
    state: &AppState,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String> {
    let Some(ai) = state.config.ai.clone() else {
        return Err(anyhow!("AI is not configured (AI_API_KEY is missing)"));
    };

    let url = ai
        .base_url
        .join("chat/completions")
        .context("invalid AI_BASE_URL")?;

    let req = ChatCompletionsRequest {
        model: &ai.model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: system,
            },
            ChatMessage {
                role: "user",
                content: user,
            },
        ],
        temperature: 0.2,
        max_tokens,
    };

    let mut last_missing: Option<String> = None;
    for attempt in 0..2 {
        let resp = state
            .http
            .post(url.clone())
            .bearer_auth(&ai.api_key)
            .json(&req)
            .send()
            .await
            .context("AI request failed")?;

        let status = resp.status();
        let body = resp.bytes().await.context("AI read response failed")?;

        if !status.is_success() {
            let msg =
                extract_error_message(&body).unwrap_or_else(|| truncate_chars_lossy(&body, 400));
            return Err(anyhow!("AI returned {status}: {msg}"));
        }

        let value: Value =
            serde_json::from_slice(&body).context("AI response json decode failed")?;
        if let Some(content) = extract_chat_content(&value) {
            return Ok(content);
        }

        let msg = if let Some(refusal) = extract_refusal_message(&value) {
            format!("AI refusal: {refusal}")
        } else {
            format!(
                "AI response missing content: {}",
                truncate_chars_lossy(&body, 400)
            )
        };
        last_missing = Some(msg);
        if attempt == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            continue;
        }
    }

    Err(anyhow!(
        "{}",
        last_missing.unwrap_or_else(|| "AI response missing content".to_owned())
    ))
}

pub async fn generate_daily_brief(state: &AppState, user_id: i64) -> Result<String> {
    if state.config.ai.is_none() {
        return Err(anyhow!("AI is not configured (AI_API_KEY is missing)"));
    };

    let now_local = chrono::Local::now();
    let window = compute_daily_window(state.config.ai_daily_at_local, now_local);
    let key_date = window.key_date.to_string();
    let start_utc = window.start_utc.to_rfc3339();
    let end_utc = window.end_utc.to_rfc3339();

    let releases = sqlx::query_as::<_, ReleaseRow>(
        r#"
        SELECT r.release_id, sr.full_name, r.tag_name, r.name, r.published_at
        FROM releases r
        JOIN starred_repos sr
          ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
        WHERE r.user_id = ?
          AND r.published_at IS NOT NULL
          AND r.published_at >= ?
          AND r.published_at < ?
        ORDER BY r.published_at DESC
        LIMIT 40
        "#,
    )
    .bind(user_id)
    .bind(&start_utc)
    .bind(&end_utc)
    .fetch_all(&state.pool)
    .await
    .context("failed to query releases for brief")?;

    let releases_md = if releases.is_empty() {
        "- (none in last 24h)".to_owned()
    } else {
        releases
            .iter()
            .map(|r| {
                let title = r
                    .name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&r.tag_name);
                let published = r.published_at.as_deref().unwrap_or("");
                format!("- {}: {} ({})", r.full_name, title, published)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let user_prompt = format!(
        "时间窗口（本地）：{start_local} → {end_local}\n\nReleases：\n{releases_md}\n\n请用中文输出一份简短、可执行的 Markdown 日报，包含两个部分：\n\n1) ## 昨日更新（Releases）\n2) ## 建议跟进（Next actions）\n\n要求：不包含任何 URL；优先输出可执行的行动项；列表不超过 10 条。",
        start_local = window.start_local.to_rfc3339(),
        end_local = window.end_local.to_rfc3339(),
    );

    let content = chat_completion(
        state,
        "你是一个助理，负责根据 GitHub Releases 列表写一份简短、可执行的中文日报（Markdown）。不要包含任何 URL。",
        &user_prompt,
        900,
    )
    .await?;
    let content = reconcile_brief_release_links(&content, &releases);

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO briefs (user_id, date, content_markdown, created_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(user_id, date) DO UPDATE SET
          content_markdown = excluded.content_markdown,
          created_at = excluded.created_at
        "#,
    )
    .bind(user_id)
    .bind(&key_date)
    .bind(&content)
    .bind(&now)
    .execute(&state.pool)
    .await
    .context("failed to store brief")?;

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_window_after_boundary() {
        let at = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 2, 21)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let (start, end, key_date) = compute_daily_window_naive(now, at);
        assert_eq!(
            start,
            NaiveDate::from_ymd_opt(2026, 2, 20)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap()
        );
        assert_eq!(
            end,
            NaiveDate::from_ymd_opt(2026, 2, 21)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap()
        );
        assert_eq!(key_date, NaiveDate::from_ymd_opt(2026, 2, 21).unwrap());
    }

    #[test]
    fn daily_window_before_boundary() {
        let at = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 2, 21)
            .unwrap()
            .and_hms_opt(7, 0, 0)
            .unwrap();
        let (start, end, key_date) = compute_daily_window_naive(now, at);
        assert_eq!(
            start,
            NaiveDate::from_ymd_opt(2026, 2, 19)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap()
        );
        assert_eq!(
            end,
            NaiveDate::from_ymd_opt(2026, 2, 20)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap()
        );
        assert_eq!(key_date, NaiveDate::from_ymd_opt(2026, 2, 20).unwrap());
    }

    #[test]
    fn extract_brief_release_ids_matches_exact_release_id() {
        let markdown = r#"
- [one](/?tab=briefs&release=12)
- [two](/?tab=briefs&release=123)
- [three](https://example.com/?release=456&foo=bar)
- [bad](/?tab=briefs&release=abc)
"#;

        let ids = extract_brief_release_ids(markdown);
        assert!(ids.contains(&12));
        assert!(ids.contains(&123));
        assert!(ids.contains(&456));
        assert!(!ids.contains(&1));
    }

    #[test]
    fn reconcile_brief_release_links_adds_missing_ids() {
        let markdown = "- [repo/a · v1.2.0](/?tab=briefs&release=12)\n";
        let targets = vec![
            ReleaseRow {
                release_id: 12,
                full_name: "repo/a".to_owned(),
                tag_name: "v1.2.0".to_owned(),
                name: None,
                published_at: None,
            },
            ReleaseRow {
                release_id: 123,
                full_name: "repo/b".to_owned(),
                tag_name: "v2.0.0".to_owned(),
                name: None,
                published_at: None,
            },
        ];

        let out = reconcile_brief_release_links(markdown, &targets);
        assert!(out.contains("release=12"));
        assert!(out.contains("release=123"));
    }

    #[test]
    fn reconcile_brief_release_links_avoids_prefix_false_positive() {
        let markdown = "- [repo/b · v2.0.0](/?tab=briefs&release=123)\n";
        let targets = vec![ReleaseRow {
            release_id: 12,
            full_name: "repo/a".to_owned(),
            tag_name: "v1.2.0".to_owned(),
            name: None,
            published_at: None,
        }];

        let out = reconcile_brief_release_links(markdown, &targets);
        assert!(out.contains("release=123"));
        assert!(out.contains("release=12"));
    }

    #[test]
    fn extract_chat_content_supports_string_message_content() {
        let v = serde_json::json!({
            "choices": [
                { "message": { "content": "  hello world  " } }
            ]
        });
        assert_eq!(extract_chat_content(&v).as_deref(), Some("hello world"));
    }

    #[test]
    fn extract_chat_content_supports_array_message_content() {
        let v = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": [
                            { "type": "text", "text": "line1" },
                            { "type": "output_text", "text": "line2" }
                        ]
                    }
                }
            ]
        });
        assert_eq!(extract_chat_content(&v).as_deref(), Some("line1\nline2"));
    }

    #[test]
    fn extract_refusal_message_reads_refusal_text() {
        let v = serde_json::json!({
            "choices": [
                { "message": { "refusal": "policy blocked" } }
            ]
        });
        assert_eq!(
            extract_refusal_message(&v).as_deref(),
            Some("policy blocked")
        );
    }
}
