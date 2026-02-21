use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::AppState;

#[derive(Debug, sqlx::FromRow)]
struct ReleaseRow {
    full_name: String,
    tag_name: String,
    name: Option<String>,
    published_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct NotificationRow {
    repo_full_name: Option<String>,
    subject_title: Option<String>,
    reason: Option<String>,
    updated_at: Option<String>,
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

pub async fn generate_daily_brief(state: &AppState, user_id: i64) -> Result<String> {
    let Some(ai) = state.config.ai.clone() else {
        return Err(anyhow!("AI is not configured (AI_API_KEY is missing)"));
    };

    let today = chrono::Local::now().date_naive().to_string();
    let since = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();

    let releases = sqlx::query_as::<_, ReleaseRow>(
        r#"
        SELECT sr.full_name, r.tag_name, r.name, r.published_at
        FROM releases r
        JOIN starred_repos sr
          ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
        WHERE r.user_id = ?
          AND r.published_at IS NOT NULL
          AND r.published_at >= ?
        ORDER BY r.published_at DESC
        LIMIT 30
        "#,
    )
    .bind(user_id)
    .bind(&since)
    .fetch_all(&state.pool)
    .await
    .context("failed to query releases for brief")?;

    let notifications = sqlx::query_as::<_, NotificationRow>(
        r#"
        SELECT repo_full_name, subject_title, reason, updated_at
        FROM notifications
        WHERE user_id = ?
          AND unread = 1
        ORDER BY updated_at DESC
        LIMIT 30
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .context("failed to query notifications for brief")?;

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

    let notifications_md = if notifications.is_empty() {
        "- (none)".to_owned()
    } else {
        notifications
            .iter()
            .map(|n| {
                let repo = n.repo_full_name.as_deref().unwrap_or("(unknown repo)");
                let title = n.subject_title.as_deref().unwrap_or("(no title)");
                let reason = n.reason.as_deref().unwrap_or("");
                let updated = n.updated_at.as_deref().unwrap_or("");
                if reason.trim().is_empty() {
                    format!("- {}: {} ({})", repo, title, updated)
                } else {
                    format!("- {}: {} ({}, {})", repo, title, reason, updated)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let user_prompt = format!(
        "Date: {today}\n\nNew releases (last 24h):\n{releases_md}\n\nUnread notifications:\n{notifications_md}\n\nWrite a concise daily brief in Markdown with sections: Releases, Notifications, Next actions. Keep it actionable."
    );

    let url = ai
        .base_url
        .join("chat/completions")
        .context("invalid AI_BASE_URL")?;

    let req = ChatCompletionsRequest {
        model: &ai.model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: "You are an assistant that writes a short, actionable GitHub daily brief in Markdown. Do not include URLs.",
            },
            ChatMessage {
                role: "user",
                content: &user_prompt,
            },
        ],
        temperature: 0.2,
        max_tokens: 800,
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
    .bind(&today)
    .bind(&content)
    .bind(&now)
    .execute(&state.pool)
    .await
    .context("failed to store brief")?;

    Ok(content)
}
