use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, offset::LocalResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::state::AppState;

const MODEL_LIMIT_UNKNOWN_FALLBACK: u32 = 32_768;
const MODEL_LIMIT_SYNC_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const MODEL_LIMIT_SAFETY_MIN_TOKENS: u32 = 512;
const MODEL_LIMIT_SAFETY_RATIO: f64 = 0.05;
const MODEL_LIMIT_DEFAULT_OUTPUT_TOKENS: u32 = 1_024;
const MODEL_LIMIT_SOURCE_OPENROUTER: &str = "https://openrouter.ai/api/v1/models";
const MODEL_LIMIT_SOURCE_LITELLM: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

#[derive(Debug, Default)]
struct ModelLimitCatalog {
    synced_limits: HashMap<String, u32>,
    synced_at: Option<Instant>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModelItem>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelItem {
    id: String,
    context_length: Option<u32>,
}

static MODEL_LIMIT_CATALOG: OnceLock<tokio::sync::RwLock<ModelLimitCatalog>> = OnceLock::new();
static BUILTIN_MODEL_LIMITS: OnceLock<HashMap<String, u32>> = OnceLock::new();

fn model_limit_catalog() -> &'static tokio::sync::RwLock<ModelLimitCatalog> {
    MODEL_LIMIT_CATALOG.get_or_init(|| tokio::sync::RwLock::new(ModelLimitCatalog::default()))
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

    let mut uniq = HashSet::new();
    expanded
        .into_iter()
        .filter(|alias| !alias.is_empty())
        .filter(|alias| uniq.insert(alias.clone()))
        .collect()
}

fn insert_model_limit(map: &mut HashMap<String, u32>, raw_model: &str, limit: u32) {
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

fn builtin_model_limits() -> &'static HashMap<String, u32> {
    BUILTIN_MODEL_LIMITS.get_or_init(|| {
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
    })
}

fn json_value_u32(value: &Value, key: &str) -> Option<u32> {
    let raw = value.get(key)?;
    if let Some(v) = raw.as_u64() {
        return u32::try_from(v).ok();
    }
    raw.as_str()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|v| *v > 0)
}

async fn fetch_openrouter_model_limits(state: &AppState) -> Result<HashMap<String, u32>> {
    let body = state
        .http
        .get(MODEL_LIMIT_SOURCE_OPENROUTER)
        .send()
        .await
        .context("openrouter model catalog request failed")?
        .error_for_status()
        .context("openrouter model catalog returned error")?
        .json::<OpenRouterModelsResponse>()
        .await
        .context("openrouter model catalog json decode failed")?;

    let mut out = HashMap::new();
    for item in body.data {
        if let Some(limit) = item.context_length {
            insert_model_limit(&mut out, &item.id, limit);
        }
    }
    Ok(out)
}

async fn fetch_litellm_model_limits(state: &AppState) -> Result<HashMap<String, u32>> {
    let body = state
        .http
        .get(MODEL_LIMIT_SOURCE_LITELLM)
        .send()
        .await
        .context("litellm model catalog request failed")?
        .error_for_status()
        .context("litellm model catalog returned error")?
        .json::<Value>()
        .await
        .context("litellm model catalog json decode failed")?;

    let Some(map) = body.as_object() else {
        return Err(anyhow!("litellm model catalog root is not an object"));
    };

    let mut out = HashMap::new();
    for (raw_model, payload) in map {
        if let Some(limit) = json_value_u32(payload, "max_input_tokens") {
            insert_model_limit(&mut out, raw_model, limit);
        }
    }
    Ok(out)
}

async fn refresh_model_limits(state: &AppState, force: bool) -> Result<()> {
    let now = Instant::now();
    if !force {
        let guard = model_limit_catalog().read().await;
        if let Some(at) = guard.synced_at
            && now.duration_since(at) < MODEL_LIMIT_SYNC_INTERVAL
        {
            return Ok(());
        }
    }

    let mut merged = HashMap::new();
    let mut errors = Vec::new();

    match fetch_openrouter_model_limits(state).await {
        Ok(values) => {
            for (key, value) in values {
                insert_model_limit(&mut merged, &key, value);
            }
        }
        Err(err) => errors.push(format!("openrouter: {err}")),
    }

    match fetch_litellm_model_limits(state).await {
        Ok(values) => {
            for (key, value) in values {
                insert_model_limit(&mut merged, &key, value);
            }
        }
        Err(err) => errors.push(format!("litellm: {err}")),
    }

    if merged.is_empty() && !errors.is_empty() {
        return Err(anyhow!(
            "model catalog refresh failed: {}",
            errors.join("; ")
        ));
    }

    if !errors.is_empty() {
        tracing::warn!(
            errors = %errors.join("; "),
            "model catalog refresh completed with partial failures"
        );
    }

    let mut guard = model_limit_catalog().write().await;
    guard.synced_limits = merged;
    guard.synced_at = Some(Instant::now());
    Ok(())
}

fn lookup_model_limit_in_map(map: &HashMap<String, u32>, model: &str) -> Option<u32> {
    for alias in model_aliases(model) {
        if let Some(limit) = map.get(&alias) {
            return Some(*limit);
        }
    }
    None
}

pub async fn resolve_model_input_limit(state: &AppState) -> u32 {
    if let Some(limit) = state.config.ai_model_context_limit {
        return limit.max(1);
    }

    let model = state
        .config
        .ai
        .as_ref()
        .map(|cfg| cfg.model.as_str())
        .unwrap_or_default();
    if model.is_empty() {
        return MODEL_LIMIT_UNKNOWN_FALLBACK;
    }

    if let Err(err) = refresh_model_limits(state, false).await {
        tracing::warn!(?err, "model catalog lazy refresh failed");
    }

    {
        let guard = model_limit_catalog().read().await;
        if let Some(limit) = lookup_model_limit_in_map(&guard.synced_limits, model) {
            return limit.max(1);
        }
    }

    if let Some(limit) = lookup_model_limit_in_map(builtin_model_limits(), model) {
        return limit.max(1);
    }

    MODEL_LIMIT_UNKNOWN_FALLBACK
}

pub async fn compute_input_budget(state: &AppState, max_tokens: u32) -> u32 {
    let model_limit = resolve_model_input_limit(state).await;
    let output_reserve = max_tokens.max(MODEL_LIMIT_DEFAULT_OUTPUT_TOKENS);
    let ratio_margin = (f64::from(model_limit) * MODEL_LIMIT_SAFETY_RATIO).ceil() as u32;
    let margin = ratio_margin.max(MODEL_LIMIT_SAFETY_MIN_TOKENS);
    model_limit
        .saturating_sub(output_reserve)
        .saturating_sub(margin)
}

pub fn estimate_text_tokens(raw: &str) -> u32 {
    let chars = raw.chars().count();
    if chars == 0 {
        return 1;
    }
    let estimated = (chars as f64 / 4.0).ceil() as u32;
    estimated.max(1)
}

pub fn pack_batch_indices(
    estimated_tokens: &[u32],
    budget: u32,
    fixed_overhead: u32,
) -> Vec<Vec<usize>> {
    if estimated_tokens.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current = Vec::new();
    let mut current_tokens = fixed_overhead;
    let hard_budget = budget.max(fixed_overhead + 1);

    for (idx, token_count) in estimated_tokens.iter().enumerate() {
        let token_count = *token_count;
        if current.is_empty() {
            current.push(idx);
            current_tokens = fixed_overhead.saturating_add(token_count);
            continue;
        }

        if current_tokens.saturating_add(token_count) > hard_budget {
            out.push(current);
            current = vec![idx];
            current_tokens = fixed_overhead.saturating_add(token_count);
            continue;
        }

        current.push(idx);
        current_tokens = current_tokens.saturating_add(token_count);
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

pub fn spawn_model_catalog_sync_task(state: Arc<AppState>) -> tokio::task::AbortHandle {
    let handle = tokio::spawn(async move {
        if let Err(err) = refresh_model_limits(state.as_ref(), true).await {
            tracing::warn!(?err, "model catalog initial refresh failed");
        }

        let mut interval = tokio::time::interval(MODEL_LIMIT_SYNC_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(err) = refresh_model_limits(state.as_ref(), true).await {
                tracing::warn!(?err, "model catalog scheduled refresh failed");
            }
        }
    });
    handle.abort_handle()
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ReleaseRow {
    release_id: i64,
    repo_id: i64,
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

fn escape_markdown_link_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if matches!(ch, '\\' | '[' | ']' | '(' | ')') {
            out.push('\\');
        }
        out.push(ch);
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

fn parse_repo_full_name_from_release_url(html_url: &str) -> Option<String> {
    let parsed = Url::parse(html_url).ok()?;
    let host = parsed.host_str()?;
    if host != "github.com" && host != "www.github.com" {
        return None;
    }

    let mut segments = parsed.path_segments()?;
    let owner = segments.next()?.trim();
    let repo = segments.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn resolve_release_full_name(html_url: &str, repo_id: i64) -> String {
    parse_repo_full_name_from_release_url(html_url).unwrap_or_else(|| format!("unknown/{repo_id}"))
}

fn is_allowed_github_url(raw: &str) -> bool {
    if raw.contains('…') {
        return false;
    }
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    host == "github.com" || host == "www.github.com" || host == "raw.githubusercontent.com"
}

fn is_allowed_markdown_link_target(raw: &str) -> bool {
    let target = raw.trim();
    if target.starts_with("/?tab=briefs&release=") {
        return true;
    }
    is_allowed_github_url(target)
}

fn trim_url_suffix(raw: &str) -> (&str, &str) {
    let mut end = raw.len();
    while end > 0 {
        let Some(ch) = raw[..end].chars().next_back() else {
            break;
        };
        if matches!(
            ch,
            ')' | ']' | '}' | ',' | ';' | '.' | ':' | '!' | '?' | '"' | '\''
        ) {
            end -= ch.len_utf8();
            continue;
        }
        break;
    }
    (&raw[..end], &raw[end..])
}

fn sanitize_url_literals(markdown: &str) -> String {
    fn sanitize_candidate(raw: &str) -> String {
        let (url, suffix) = trim_url_suffix(raw);
        if url.is_empty() || is_allowed_markdown_link_target(url) {
            return raw.to_owned();
        }

        let mut out = compact_link_label(url);
        out.push_str(suffix);
        out
    }

    let mut out = String::with_capacity(markdown.len());
    let mut i = 0usize;

    while i < markdown.len() {
        let rest = &markdown[i..];

        if rest.starts_with('<')
            && let Some(end_rel) = rest.find('>')
        {
            let inner = &rest[1..end_rel];
            if inner.starts_with("https://") || inner.starts_with("http://") {
                out.push_str(&sanitize_candidate(inner));
                i += end_rel + 1;
                continue;
            }
        }

        if rest.starts_with("https://") || rest.starts_with("http://") {
            let end_rel = rest
                .char_indices()
                .find_map(|(idx, ch)| {
                    (idx > 0 && matches!(ch, ' ' | '\t' | '\n' | '\r' | '<' | '>' | '`'))
                        .then_some(idx)
                })
                .unwrap_or(rest.len());
            let raw = &rest[..end_rel];
            out.push_str(&sanitize_candidate(raw));
            i += end_rel;
            continue;
        }

        let mut chars = rest.chars();
        let ch = chars.next().expect("rest is non-empty");
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn sanitize_markdown_links(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut i = 0usize;

    while i < markdown.len() {
        let rest = &markdown[i..];

        if rest.starts_with('[')
            && let Some(text_end_rel) = rest.find("](")
            && let Some(url_end_rel) = rest[text_end_rel + 2..].find(')')
        {
            let text_start = i + 1;
            let text_end = i + text_end_rel;
            let url_start = i + text_end_rel + 2;
            let url_end = url_start + url_end_rel;

            let text = &markdown[text_start..text_end];
            let target = &markdown[url_start..url_end];

            if is_allowed_markdown_link_target(target) {
                out.push_str(&markdown[i..=url_end]);
            } else {
                out.push_str(text);
            }
            i = url_end + 1;
            continue;
        }

        let mut chars = rest.chars();
        let ch = chars.next().expect("rest is non-empty");
        out.push(ch);
        i += ch.len_utf8();
    }

    sanitize_url_literals(&out)
}

fn strip_markdown_links_to_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < input.len() {
        let rest = &input[i..];

        if rest.starts_with('[')
            && let Some(text_end_rel) = rest.find("](")
            && let Some(url_end_rel) = rest[text_end_rel + 2..].find(')')
        {
            let text_start = i + 1;
            let text_end = i + text_end_rel;
            let url_start = i + text_end_rel + 2;
            let url_end = url_start + url_end_rel;
            let text = &input[text_start..text_end];
            out.push_str(text);
            i = url_end + 1;
            continue;
        }

        let mut chars = rest.chars();
        let ch = chars.next().expect("rest is non-empty");
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn sanitize_bullet_text(raw: &str) -> String {
    let normalized = raw.replace(['\r', '\n'], " ");
    let normalized = strip_markdown_links_to_text(&normalized);
    let normalized = strip_html_tags(&normalized);

    let mut out = Vec::new();
    for token in normalized.split_whitespace() {
        let candidate = token.trim_matches(|c: char| {
            matches!(
                c,
                ')' | '(' | '[' | ']' | '<' | '>' | ',' | ';' | '"' | '\'' | '.'
            )
        });

        if candidate.starts_with("https://") || candidate.starts_with("http://") {
            continue;
        }

        out.push(token.to_owned());
    }

    out.join(" ").trim().to_owned()
}

fn extract_github_links(body: &str, max_links: usize) -> Vec<String> {
    fn push_link(
        candidate: &str,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
        max_links: usize,
    ) -> bool {
        if !is_allowed_github_url(candidate) {
            return false;
        }
        let Ok(url) = Url::parse(candidate) else {
            return false;
        };
        let normalized = url.to_string();
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
        out.len() >= max_links
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();

    // Prefer explicit Markdown links first, e.g. [PR](https://github.com/...)
    let mut i = 0usize;
    while i < body.len() && out.len() < max_links {
        let rest = &body[i..];

        if rest.starts_with('[')
            && let Some(text_end_rel) = rest.find("](")
            && let Some(url_end_rel) = rest[text_end_rel + 2..].find(')')
        {
            let url_start = i + text_end_rel + 2;
            let url_end = url_start + url_end_rel;
            let target = body[url_start..url_end].trim();
            if push_link(target, &mut seen, &mut out, max_links) {
                return out;
            }
            i = url_end + 1;
            continue;
        }

        let mut chars = rest.chars();
        let ch = chars.next().expect("rest is non-empty");
        i += ch.len_utf8();
    }

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
        if push_link(candidate, &mut seen, &mut out, max_links) {
            break;
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

        let cleaned = sanitize_bullet_text(content);
        if cleaned.is_empty() {
            continue;
        }
        bullets.push(truncate_chars(&cleaned, 180));
        if bullets.len() >= max_bullets {
            break;
        }
    }

    if bullets.is_empty() {
        let mut fallback = non_empty_lines(body)
            .filter(|line| !line.starts_with('#'))
            .take(max_bullets)
            .map(sanitize_bullet_text)
            .filter(|line| !line.is_empty())
            .map(|line| truncate_chars(&line, 180))
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
            full_name: resolve_release_full_name(&r.html_url, r.repo_id),
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

fn build_projects_batch_prompt(projects: &[(String, Vec<ReleaseDigest>)]) -> String {
    let mut body = String::new();
    body.push_str("你会收到多个仓库在一个时间窗口内的 GitHub Release。请为每个 release 提取 1-4 条变更要点。\n");
    body.push_str("输出严格 JSON（不要 markdown code block）：\n");
    body.push_str("{\"items\":[{\"release_id\":123,\"summary_bullets\":[\"...\",\"...\"]}]}\n\n");
    body.push_str("硬性要求：\n");
    body.push_str("1) 必须覆盖输入中的每个 release_id；\n");
    body.push_str("2) 不得新增输入里不存在的事实；\n");
    body.push_str("3) 不输出任何 URL；\n");
    body.push_str("4) 可标注 breaking/security/perf/docs/fix 等类型，但条目要简洁。\n\n");

    for (full_name, releases) in projects {
        body.push_str("====\n");
        body.push_str("仓库：");
        body.push_str(full_name);
        body.push_str("\n");
        for rel in releases {
            body.push_str("\n---\n");
            body.push_str(&format!("release_id: {}\n", rel.release_id));
            body.push_str(&format!("title: {}\n", rel.title));
            body.push_str(&format!("published_at: {}\n", rel.published_at));
            body.push_str(&format!("is_prerelease: {}\n", rel.is_prerelease));
            body.push_str("notes:\n");
            body.push_str(&truncate_chars(&rel.body, 4_800));
            body.push('\n');
        }
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
            .map(|s| sanitize_bullet_text(s.trim()))
            .filter(|s| !s.is_empty())
            .take(4)
            .collect::<Vec<_>>();
        if !bullets.is_empty() {
            out.insert(item.release_id, bullets);
        }
    }

    Ok(out)
}

async fn summarize_projects_with_ai(
    state: &AppState,
    projects: &[(String, Vec<ReleaseDigest>)],
) -> HashMap<i64, Vec<String>> {
    if projects.is_empty() {
        return HashMap::new();
    }

    let estimated = projects
        .iter()
        .map(|(_, releases)| {
            releases
                .iter()
                .map(|rel| estimate_text_tokens(&rel.title) + estimate_text_tokens(&rel.body) + 48)
                .sum::<u32>()
                .max(1)
        })
        .collect::<Vec<_>>();
    let budget = compute_input_budget(state, 1_400).await;
    let groups = pack_batch_indices(&estimated, budget, 320);

    let mut merged = HashMap::<i64, Vec<String>>::new();
    for group in groups {
        let batch_projects = group
            .iter()
            .map(|idx| projects[*idx].clone())
            .collect::<Vec<_>>();
        let prompt = build_projects_batch_prompt(&batch_projects);

        let raw = chat_completion(
            state,
            "你是一个严谨的发布说明整理助手，擅长在不遗漏关键信息的前提下提炼 GitHub Release 变更。",
            &prompt,
            1_400,
        )
        .await;

        let mut parsed_ok = false;
        if let Ok(raw) = raw
            && let Some(payload) = parse_project_summary_payload(&raw)
        {
            parsed_ok = true;
            for item in payload.items {
                let bullets = item
                    .summary_bullets
                    .into_iter()
                    .map(|s| sanitize_bullet_text(s.trim()))
                    .filter(|s| !s.is_empty())
                    .take(4)
                    .collect::<Vec<_>>();
                if !bullets.is_empty() {
                    merged.insert(item.release_id, bullets);
                }
            }
        }
        if !parsed_ok {
            tracing::warn!(
                "project batch summary parse failed; fallback to per-repo summarization"
            );
        }

        for (full_name, releases) in &batch_projects {
            if let Ok(map) = summarize_project_with_ai(state, full_name, releases).await {
                for (release_id, bullets) in map {
                    merged.entry(release_id).or_insert(bullets);
                }
            }
        }
    }

    merged
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
            let title = escape_markdown_link_text(&release.title);

            out.push_str(&format!(
                "- [{}]({}) · {}{} · [GitHub Release]({})\n",
                title, internal_link, release.published_at, prerelease_mark, release.html_url
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

fn parse_internal_release_id(target: &str) -> Option<i64> {
    let base = Url::parse("https://octorill.local/").expect("valid local base url");
    let joined = base.join(target.trim()).ok()?;

    if joined.host_str() != Some("octorill.local") {
        return None;
    }
    let tab = joined
        .query_pairs()
        .find_map(|(k, v)| (k == "tab").then_some(v.into_owned()))?;
    if tab != "briefs" {
        return None;
    }

    let raw_release = joined
        .query_pairs()
        .find_map(|(k, v)| (k == "release").then_some(v.into_owned()))?;
    if !raw_release.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    raw_release.parse::<i64>().ok()
}

fn extract_internal_release_ids(markdown: &str) -> HashSet<i64> {
    let mut ids = HashSet::new();
    let mut i = 0usize;

    while i < markdown.len() {
        let rest = &markdown[i..];

        if rest.starts_with('[')
            && let Some(text_end_rel) = rest.find("](")
            && let Some(url_end_rel) = rest[text_end_rel + 2..].find(')')
        {
            let url_start = i + text_end_rel + 2;
            let url_end = url_start + url_end_rel;
            let target = &markdown[url_start..url_end];
            if let Some(release_id) = parse_internal_release_id(target) {
                ids.insert(release_id);
            }
            i = url_end + 1;
            continue;
        }

        let mut chars = rest.chars();
        let ch = chars.next().expect("rest is non-empty");
        i += ch.len_utf8();
    }

    ids
}

fn contains_all_release_links(markdown: &str, release_ids: &[i64]) -> bool {
    let present = extract_internal_release_ids(markdown);
    release_ids.iter().all(|id| present.contains(id))
}

fn reconcile_brief_release_links(markdown: &str, releases: &[ReleaseDigest]) -> String {
    let present = extract_internal_release_ids(markdown);
    let mut missing = releases
        .iter()
        .filter(|release| !present.contains(&release.release_id))
        .collect::<Vec<_>>();

    if missing.is_empty() {
        return markdown.to_owned();
    }

    missing.sort_by_key(|release| release.release_id);

    let mut out = markdown.trim_end_matches('\n').to_owned();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("## Release links\n");
    for release in missing {
        let label = escape_markdown_link_text(&format!("{}/{}", release.full_name, release.title));
        out.push_str(&format!(
            "- [{}](/?tab=briefs&release={})\n",
            label, release.release_id
        ));
    }
    out
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

    let sanitized = sanitize_markdown_links(&polished);

    if !sanitized.contains("## 概览") || !sanitized.contains("## 项目更新") {
        return None;
    }
    if !contains_all_release_links(&sanitized, release_ids) {
        return None;
    }

    Some(sanitized)
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
          r.repo_id,
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
          AND r.is_draft = 0
          AND COALESCE(r.published_at, r.created_at, r.updated_at) >= ?
          AND COALESCE(r.published_at, r.created_at, r.updated_at) < ?
        ORDER BY
          COALESCE(r.published_at, r.created_at, r.updated_at) DESC,
          r.release_id DESC
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
    let grouped = group_by_repo(&releases)
        .into_iter()
        .collect::<Vec<(String, Vec<ReleaseDigest>)>>();

    let ai_bullets = summarize_projects_with_ai(state, &grouped).await;

    let mut repos = Vec::with_capacity(grouped.len());
    for (full_name, project_releases) in grouped {
        repos.push(build_repo_rendered(
            &full_name,
            &project_releases,
            Some(&ai_bullets),
        ));
    }

    let deterministic = sanitize_markdown_links(&build_brief_markdown(window, &repos));

    if state.config.ai.is_none() || releases.is_empty() {
        return Ok(reconcile_brief_release_links(&deterministic, &releases));
    }

    let release_ids = releases.iter().map(|r| r.release_id).collect::<Vec<_>>();
    if let Some(polished) = polish_brief_markdown(state, &deterministic, &release_ids).await {
        return Ok(reconcile_brief_release_links(&polished, &releases));
    }

    Ok(reconcile_brief_release_links(&deterministic, &releases))
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
    fn extract_github_links_parses_markdown_links() {
        let links = extract_github_links(
            "- [PR #12](https://github.com/acme/app/pull/12)\n- [Doc](https://example.com/ignore)",
            5,
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "https://github.com/acme/app/pull/12");
    }

    #[test]
    fn sanitize_markdown_links_strips_disallowed_bare_urls() {
        let markdown = "- details: https://example.com/acme/releases?foo=bar";
        let sanitized = sanitize_markdown_links(markdown);
        assert!(!sanitized.contains("https://example.com"));
        assert!(sanitized.contains("example.com/acme/releases?foo=bar"));
    }

    #[test]
    fn sanitize_markdown_links_strips_disallowed_angle_autolinks() {
        let markdown = "- mirror: <https://example.com/acme/releases>";
        let sanitized = sanitize_markdown_links(markdown);
        assert!(!sanitized.contains("https://example.com"));
        assert!(sanitized.contains("example.com/acme/releases"));
    }

    #[test]
    fn sanitize_markdown_links_keeps_allowed_bare_github_urls() {
        let markdown = "- ref: https://github.com/acme/app/releases/tag/v1.2.3";
        let sanitized = sanitize_markdown_links(markdown);
        assert_eq!(sanitized, markdown);
    }

    #[test]
    fn fallback_bullets_never_empty() {
        let bullets = extract_fallback_bullets("", 3);
        assert_eq!(bullets.len(), 1);
    }

    #[test]
    fn contains_all_release_links_matches_exact_release_id() {
        let markdown = "- [v1.2.3](/?tab=briefs&release=123)";
        assert!(contains_all_release_links(markdown, &[123]));
        assert!(!contains_all_release_links(markdown, &[12, 123]));
    }

    #[test]
    fn contains_all_release_links_accepts_query_order_variants() {
        let markdown = "- [v1.2.3](/?release=123&tab=briefs)";
        assert!(contains_all_release_links(markdown, &[123]));
    }

    #[test]
    fn contains_all_release_links_requires_tab_briefs() {
        let markdown = "- [v1.2.3](/?release=123)";
        assert!(!contains_all_release_links(markdown, &[123]));
    }

    #[test]
    fn reconcile_brief_release_links_adds_missing_release_ids() {
        let markdown = "- [v1.2.3](/?tab=briefs&release=12)\n";
        let releases = vec![
            ReleaseDigest {
                release_id: 12,
                full_name: "acme/rocket".to_owned(),
                title: "v1.2.3".to_owned(),
                body: String::new(),
                html_url: "https://github.com/acme/rocket/releases/tag/v1.2.3".to_owned(),
                published_at: "2026-02-20T09:00:00Z".to_owned(),
                is_prerelease: false,
            },
            ReleaseDigest {
                release_id: 123,
                full_name: "acme/rocket".to_owned(),
                title: "v1.2.4".to_owned(),
                body: String::new(),
                html_url: "https://github.com/acme/rocket/releases/tag/v1.2.4".to_owned(),
                published_at: "2026-02-20T10:00:00Z".to_owned(),
                is_prerelease: false,
            },
        ];
        let out = reconcile_brief_release_links(markdown, &releases);
        assert!(contains_all_release_links(&out, &[12, 123]));
    }

    #[test]
    fn reconcile_brief_release_links_avoids_prefix_false_positive() {
        let markdown = "- [v1.2.4](/?tab=briefs&release=123)\n";
        let releases = vec![ReleaseDigest {
            release_id: 12,
            full_name: "acme/rocket".to_owned(),
            title: "v1.2.3".to_owned(),
            body: String::new(),
            html_url: "https://github.com/acme/rocket/releases/tag/v1.2.3".to_owned(),
            published_at: "2026-02-20T09:00:00Z".to_owned(),
            is_prerelease: false,
        }];
        let out = reconcile_brief_release_links(markdown, &releases);
        assert!(contains_all_release_links(&out, &[12, 123]));
    }

    #[test]
    fn reconcile_brief_release_links_replaces_links_missing_tab() {
        let markdown = "- [v1.2.3](/?release=123)\n";
        let releases = vec![ReleaseDigest {
            release_id: 123,
            full_name: "acme/rocket".to_owned(),
            title: "v1.2.3".to_owned(),
            body: String::new(),
            html_url: "https://github.com/acme/rocket/releases/tag/v1.2.3".to_owned(),
            published_at: "2026-02-20T09:00:00Z".to_owned(),
            is_prerelease: false,
        }];
        let out = reconcile_brief_release_links(markdown, &releases);
        assert!(out.contains("/?tab=briefs&release=123"));
    }

    #[test]
    fn build_brief_markdown_escapes_release_title_link_text() {
        let start_local = local_from_naive(
            NaiveDate::from_ymd_opt(2026, 2, 20)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
        );
        let end_local = local_from_naive(
            NaiveDate::from_ymd_opt(2026, 2, 21)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
        );
        let window = DailyWindow {
            key_date: NaiveDate::from_ymd_opt(2026, 2, 21).unwrap(),
            start_utc: start_local.with_timezone(&chrono::Utc),
            end_utc: end_local.with_timezone(&chrono::Utc),
            start_local,
            end_local,
        };
        let repo = RepoRendered {
            full_name: "acme/rocket".to_owned(),
            releases: vec![ReleaseRendered {
                release_id: 42,
                title: "v1.0 [beta](rc)".to_owned(),
                html_url: "https://github.com/acme/rocket/releases/tag/v1.0".to_owned(),
                published_at: "2026-02-20T09:00:00Z".to_owned(),
                is_prerelease: false,
                bullets: vec!["修复若干问题".to_owned()],
                related_links: Vec::new(),
            }],
        };

        let markdown = build_brief_markdown(&window, &[repo]);
        assert!(markdown.contains("- [v1.0 \\[beta\\]\\(rc\\)](/?tab=briefs&release=42)"));
    }
}
