use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Context, Result, anyhow};
use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, offset::LocalResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::state::AppState;

#[derive(Debug, Clone, sqlx::FromRow)]
struct ReleaseRow {
    release_id: i64,
    full_name: String,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: String,
    is_prerelease: i64,
}

#[derive(Debug, Clone)]
struct ReleaseDigest {
    release_id: i64,
    full_name: String,
    title: String,
    body: String,
    html_url: String,
    published_at: String,
    is_prerelease: bool,
}

#[derive(Debug, Clone)]
struct ReleaseRendered {
    release_id: i64,
    title: String,
    html_url: String,
    published_at: String,
    is_prerelease: bool,
    bullets: Vec<String>,
    related_links: Vec<String>,
}

#[derive(Debug)]
struct RepoRendered {
    full_name: String,
    releases: Vec<ReleaseRendered>,
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

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectSummaryPayload {
    items: Vec<ProjectSummaryItem>,
}

#[derive(Debug, Deserialize)]
struct ProjectSummaryItem {
    release_id: i64,
    summary_bullets: Vec<String>,
}

fn truncate_chars_lossy(bytes: &[u8], max_chars: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
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

fn daily_window_for_key_date(key_date: NaiveDate, at: NaiveTime) -> DailyWindow {
    let (start_local, end_local) = compute_window_for_key_date(key_date, at);
    DailyWindow {
        key_date,
        start_utc: start_local.with_timezone(&chrono::Utc),
        end_utc: end_local.with_timezone(&chrono::Utc),
        start_local,
        end_local,
    }
}

pub fn compute_daily_window(at: Option<NaiveTime>, now: chrono::DateTime<Local>) -> DailyWindow {
    let now_naive = now.naive_local();
    let at = at.or_else(|| NaiveTime::from_hms_opt(8, 0, 0));
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

pub fn recent_key_dates(
    at: NaiveTime,
    now: chrono::DateTime<Local>,
    days: usize,
) -> Vec<NaiveDate> {
    if days == 0 {
        return Vec::new();
    }
    let current = compute_daily_window(Some(at), now).key_date;
    (0..days)
        .map(|offset| current - chrono::Duration::days(offset as i64))
        .collect()
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

    let resp = state
        .http
        .post(url)
        .bearer_auth(ai.api_key)
        .json(&req)
        .send()
        .await
        .context("AI request failed")?;

    let status = resp.status();
    let body = resp.bytes().await.context("AI read response failed")?;

    if !status.is_success() {
        let msg = extract_error_message(&body).unwrap_or_else(|| truncate_chars_lossy(&body, 400));
        return Err(anyhow!("AI returned {status}: {msg}"));
    }

    let resp: ChatCompletionsResponse =
        serde_json::from_slice(&body).context("AI response json decode failed")?;

    let content = resp
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("AI response missing content"))?;

    Ok(content)
}

fn non_empty_lines(md: &str) -> impl Iterator<Item = &str> {
    md.lines().map(str::trim).filter(|line| !line.is_empty())
}

fn extract_json_object_span(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end < start {
        return None;
    }
    Some(raw[start..=end].trim())
}

fn parse_project_summary_payload(raw: &str) -> Option<ProjectSummaryPayload> {
    fn parse_direct(raw: &str) -> Option<ProjectSummaryPayload> {
        serde_json::from_str::<ProjectSummaryPayload>(raw)
            .ok()
            .or_else(|| {
                let inner = serde_json::from_str::<String>(raw).ok()?;
                serde_json::from_str::<ProjectSummaryPayload>(&inner).ok()
            })
    }

    let trimmed = raw.trim();
    parse_direct(trimmed).or_else(|| extract_json_object_span(trimmed).and_then(parse_direct))
}

fn compact_link_label(raw: &str) -> String {
    if let Ok(parsed) = Url::parse(raw) {
        let host = parsed.host_str().unwrap_or("github.com");
        let mut out = format!("{}{}", host, parsed.path());
        if let Some(q) = parsed.query() {
            out.push('?');
            out.push_str(q);
        }
        return truncate_chars(&out, 64);
    }
    truncate_chars(raw, 64)
}

fn extract_github_links(body: &str, max_links: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for token in body.split_whitespace() {
        let candidate = token.trim_matches(|c: char| {
            matches!(
                c,
                ')' | '(' | '[' | ']' | '<' | '>' | ',' | ';' | '"' | '\'' | '.'
            )
        });
        if !(candidate.starts_with("https://") || candidate.starts_with("http://")) {
            continue;
        }
        let Ok(url) = Url::parse(candidate) else {
            continue;
        };
        let Some(host) = url.host_str() else {
            continue;
        };
        if !(host == "github.com"
            || host == "www.github.com"
            || host == "raw.githubusercontent.com")
        {
            continue;
        }

        let normalized = url.to_string();
        if seen.insert(normalized.clone()) {
            out.push(normalized);
            if out.len() >= max_links {
                break;
            }
        }
    }
    out
}

fn extract_fallback_bullets(body: &str, max_bullets: usize) -> Vec<String> {
    let mut bullets = Vec::new();
    let mut in_code = false;

    for raw in body.replace("\r\n", "\n").lines() {
        let line = raw.trim();
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || line.is_empty() {
            continue;
        }

        let content = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("+ "))
            .map(str::trim)
            .or_else(|| {
                let (head, tail) = line.split_once('.')?;
                if head.chars().all(|c| c.is_ascii_digit()) && tail.starts_with(' ') {
                    Some(tail.trim())
                } else {
                    None
                }
            })
            .unwrap_or(line);

        if content.starts_with('#') {
            continue;
        }

        bullets.push(truncate_chars(content, 180));
        if bullets.len() >= max_bullets {
            break;
        }
    }

    if bullets.is_empty() {
        let mut fallback = non_empty_lines(body)
            .filter(|line| !line.starts_with('#'))
            .take(max_bullets)
            .map(|line| truncate_chars(line, 180))
            .collect::<Vec<_>>();
        if fallback.is_empty() {
            fallback.push("本次发布未提供可提取的变更说明。".to_owned());
        }
        return fallback;
    }

    bullets
}

fn to_release_digest(rows: Vec<ReleaseRow>) -> Vec<ReleaseDigest> {
    rows.into_iter()
        .map(|r| ReleaseDigest {
            release_id: r.release_id,
            full_name: r.full_name,
            title: r
                .name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&r.tag_name)
                .to_owned(),
            body: r.body.unwrap_or_default(),
            html_url: r.html_url,
            published_at: r.published_at,
            is_prerelease: r.is_prerelease != 0,
        })
        .collect()
}

fn group_by_repo(releases: &[ReleaseDigest]) -> BTreeMap<String, Vec<ReleaseDigest>> {
    let mut grouped: BTreeMap<String, Vec<ReleaseDigest>> = BTreeMap::new();
    for r in releases {
        grouped
            .entry(r.full_name.clone())
            .or_default()
            .push(r.clone());
    }
    grouped
}

fn build_project_prompt(full_name: &str, releases: &[ReleaseDigest]) -> String {
    let mut body = String::new();
    body.push_str("你会收到同一仓库在一个时间窗口内的多个 GitHub Release。请为每个 release 提取 1-4 条变更要点。\n");
    body.push_str("输出严格 JSON（不要 markdown code block）：\n");
    body.push_str("{\"items\":[{\"release_id\":123,\"summary_bullets\":[\"...\",\"...\"]}]}\n\n");
    body.push_str("硬性要求：\n");
    body.push_str("1) 必须覆盖输入中的每个 release_id；\n");
    body.push_str("2) 不得新增输入里不存在的事实；\n");
    body.push_str("3) 不输出任何 URL；\n");
    body.push_str("4) 可标注 breaking/security/perf/docs/fix 等类型，但条目要简洁。\n\n");
    body.push_str("仓库：");
    body.push_str(full_name);
    body.push_str("\n\nReleases:\n");

    for rel in releases {
        body.push_str("\n---\n");
        body.push_str(&format!("release_id: {}\n", rel.release_id));
        body.push_str(&format!("title: {}\n", rel.title));
        body.push_str(&format!("published_at: {}\n", rel.published_at));
        body.push_str(&format!("is_prerelease: {}\n", rel.is_prerelease));
        body.push_str("notes:\n");
        body.push_str(&truncate_chars(&rel.body, 4800));
        body.push('\n');
    }

    body
}

async fn summarize_project_with_ai(
    state: &AppState,
    full_name: &str,
    releases: &[ReleaseDigest],
) -> Result<HashMap<i64, Vec<String>>> {
    if releases.is_empty() {
        return Ok(HashMap::new());
    }

    let prompt = build_project_prompt(full_name, releases);
    let raw = chat_completion(
        state,
        "你是一个严谨的发布说明整理助手，擅长在不遗漏关键信息的前提下提炼 GitHub Release 变更。",
        &prompt,
        1200,
    )
    .await?;

    let Some(payload) = parse_project_summary_payload(&raw) else {
        return Err(anyhow!("project summary json decode failed"));
    };

    let mut out = HashMap::new();
    for item in payload.items {
        let bullets = item
            .summary_bullets
            .into_iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .take(4)
            .collect::<Vec<_>>();
        if !bullets.is_empty() {
            out.insert(item.release_id, bullets);
        }
    }

    Ok(out)
}

fn build_repo_rendered(
    full_name: &str,
    releases: &[ReleaseDigest],
    ai_bullets: Option<&HashMap<i64, Vec<String>>>,
) -> RepoRendered {
    let rendered = releases
        .iter()
        .map(|release| {
            let ai = ai_bullets.and_then(|m| m.get(&release.release_id)).cloned();
            let bullets = ai.unwrap_or_else(|| extract_fallback_bullets(&release.body, 3));
            let related_links = extract_github_links(&release.body, 3);
            ReleaseRendered {
                release_id: release.release_id,
                title: release.title.clone(),
                html_url: release.html_url.clone(),
                published_at: release.published_at.clone(),
                is_prerelease: release.is_prerelease,
                bullets,
                related_links,
            }
        })
        .collect::<Vec<_>>();

    RepoRendered {
        full_name: full_name.to_owned(),
        releases: rendered,
    }
}

fn build_brief_markdown(window: &DailyWindow, repos: &[RepoRendered]) -> String {
    let total_releases = repos.iter().map(|r| r.releases.len()).sum::<usize>();
    let prerelease_count = repos
        .iter()
        .flat_map(|r| r.releases.iter())
        .filter(|r| r.is_prerelease)
        .count();

    let mut out = String::new();
    out.push_str("## 概览\n\n");
    out.push_str(&format!(
        "- 时间窗口（本地）：{} → {}\n",
        window.start_local.to_rfc3339(),
        window.end_local.to_rfc3339()
    ));
    out.push_str(&format!("- 更新项目：{} 个\n", repos.len()));
    out.push_str(&format!(
        "- Release：{} 条（预发布 {} 条）\n",
        total_releases, prerelease_count
    ));

    if repos.is_empty() {
        out.push_str("\n## 项目更新\n\n- 本时间窗口内没有新的 Release。\n");
        return out;
    }

    let repo_links = repos
        .iter()
        .map(|repo| {
            format!(
                "[{}](https://github.com/{})",
                repo.full_name, repo.full_name
            )
        })
        .collect::<Vec<_>>()
        .join("、");
    out.push_str(&format!("- 涉及项目：{}\n", repo_links));

    out.push_str("\n## 项目更新\n\n");
    for repo in repos {
        out.push_str(&format!(
            "### [{}](https://github.com/{})\n\n",
            repo.full_name, repo.full_name
        ));

        for release in &repo.releases {
            let internal_link = format!("/?tab=briefs&release={}", release.release_id);
            let prerelease_mark = if release.is_prerelease {
                " · 预发布"
            } else {
                ""
            };

            out.push_str(&format!(
                "- [{}]({}) · {}{} · [GitHub Release]({})\n",
                release.title,
                internal_link,
                release.published_at,
                prerelease_mark,
                release.html_url
            ));

            for bullet in &release.bullets {
                out.push_str(&format!("  - {}\n", bullet));
            }

            if !release.related_links.is_empty() {
                let links = release
                    .related_links
                    .iter()
                    .map(|url| format!("[{}]({})", compact_link_label(url), url))
                    .collect::<Vec<_>>()
                    .join(" · ");
                out.push_str(&format!("  - 相关链接：{}\n", links));
            }

            out.push('\n');
        }
    }

    out
}

fn contains_all_release_links(markdown: &str, release_ids: &[i64]) -> bool {
    release_ids
        .iter()
        .all(|id| markdown.contains(&format!("release={id}")))
}

async fn polish_brief_markdown(
    state: &AppState,
    markdown: &str,
    release_ids: &[i64],
) -> Option<String> {
    let prompt = format!(
        "请在不删减任何 release 条目的前提下，对下面日报做一次统一润色。\n\n硬性要求：\n1) 保留所有链接原样（尤其 /?tab=briefs&release=...）；\n2) 保留“## 概览”和“## 项目更新”两个章节；\n3) 不新增编造事实；\n4) 可以调整语句与去重。\n\n日报原文：\n{markdown}",
    );

    let polished = chat_completion(
        state,
        "你是一个发布日报编辑器，负责在不丢失信息的前提下优化可读性。",
        &prompt,
        1600,
    )
    .await
    .ok()?;

    if !polished.contains("## 概览") || !polished.contains("## 项目更新") {
        return None;
    }
    if !contains_all_release_links(&polished, release_ids) {
        return None;
    }

    Some(polished)
}

async fn build_brief_content(
    state: &AppState,
    window: &DailyWindow,
    user_id: i64,
) -> Result<String> {
    let start_utc = window.start_utc.to_rfc3339();
    let end_utc = window.end_utc.to_rfc3339();

    let rows = sqlx::query_as::<_, ReleaseRow>(
        r#"
        SELECT
          r.release_id,
          sr.full_name,
          r.tag_name,
          r.name,
          r.body,
          r.html_url,
          COALESCE(r.published_at, r.created_at, r.updated_at) AS published_at,
          r.is_prerelease
        FROM releases r
        JOIN starred_repos sr
          ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
        WHERE r.user_id = ?
          AND COALESCE(r.published_at, r.created_at, r.updated_at) >= ?
          AND COALESCE(r.published_at, r.created_at, r.updated_at) < ?
        ORDER BY sr.full_name ASC, COALESCE(r.published_at, r.created_at, r.updated_at) DESC
        LIMIT 300
        "#,
    )
    .bind(user_id)
    .bind(&start_utc)
    .bind(&end_utc)
    .fetch_all(&state.pool)
    .await
    .context("failed to query releases for brief")?;

    let releases = to_release_digest(rows);
    let grouped = group_by_repo(&releases);

    let mut repos = Vec::with_capacity(grouped.len());
    for (full_name, project_releases) in grouped {
        let ai_bullets = summarize_project_with_ai(state, &full_name, &project_releases)
            .await
            .ok();
        repos.push(build_repo_rendered(
            &full_name,
            &project_releases,
            ai_bullets.as_ref(),
        ));
    }

    let deterministic = build_brief_markdown(window, &repos);

    if state.config.ai.is_none() || releases.is_empty() {
        return Ok(deterministic);
    }

    let release_ids = releases.iter().map(|r| r.release_id).collect::<Vec<_>>();
    if let Some(polished) = polish_brief_markdown(state, &deterministic, &release_ids).await {
        return Ok(polished);
    }

    Ok(deterministic)
}

fn resolve_daily_boundary(at: Option<NaiveTime>) -> NaiveTime {
    at.or_else(|| NaiveTime::from_hms_opt(8, 0, 0))
        .expect("08:00 is valid")
}

pub async fn generate_daily_brief_for_key_date(
    state: &AppState,
    user_id: i64,
    key_date: NaiveDate,
) -> Result<String> {
    if state.config.ai.is_none() {
        return Err(anyhow!("AI is not configured (AI_API_KEY is missing)"));
    }

    let at = resolve_daily_boundary(state.config.ai_daily_at_local);
    let window = daily_window_for_key_date(key_date, at);
    let content = build_brief_content(state, &window, user_id).await?;

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
    .bind(key_date.to_string())
    .bind(&content)
    .bind(&now)
    .execute(&state.pool)
    .await
    .context("failed to store brief")?;

    Ok(content)
}

pub async fn generate_daily_brief(state: &AppState, user_id: i64) -> Result<String> {
    if state.config.ai.is_none() {
        return Err(anyhow!("AI is not configured (AI_API_KEY is missing)"));
    }

    let now_local = chrono::Local::now();
    let window = compute_daily_window(state.config.ai_daily_at_local, now_local);
    generate_daily_brief_for_key_date(state, user_id, window.key_date).await
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
    fn recent_key_dates_returns_descending_days() {
        let at = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let now = local_from_naive(
            NaiveDate::from_ymd_opt(2026, 2, 21)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
        );
        let dates = recent_key_dates(at, now, 3);
        assert_eq!(dates.len(), 3);
        assert_eq!(dates[0], NaiveDate::from_ymd_opt(2026, 2, 21).unwrap());
        assert_eq!(dates[1], NaiveDate::from_ymd_opt(2026, 2, 20).unwrap());
        assert_eq!(dates[2], NaiveDate::from_ymd_opt(2026, 2, 19).unwrap());
    }

    #[test]
    fn extract_github_links_filters_non_github() {
        let links = extract_github_links(
            "see https://github.com/acme/app/releases/tag/v1 and https://example.com/not-me",
            5,
        );
        assert_eq!(links.len(), 1);
        assert!(links[0].contains("github.com/acme/app"));
    }

    #[test]
    fn fallback_bullets_never_empty() {
        let bullets = extract_fallback_bullets("", 3);
        assert_eq!(bullets.len(), 1);
    }
}
