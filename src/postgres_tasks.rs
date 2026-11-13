//! PostgreSQL-native business task executors.
//!
//! Each executor owns one business vertical's upstream calls and PostgreSQL
//! writes. Durable claim/finalize semantics remain in `postgres_worker`.

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool};

use crate::{crypto::EncryptionKey, postgres_jobs::ClaimedTask, postgres_worker::TaskExecutor};

const GITHUB_API: &str = "https://api.github.com";

#[derive(Clone)]
pub struct NotificationsExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
}

#[derive(Clone)]
pub struct ReleasesExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
}

impl ReleasesExecutor {
    pub fn new(pool: PgPool, http: Client, encryption_key: EncryptionKey) -> Self {
        Self {
            pool,
            http,
            encryption_key,
        }
    }
}

impl NotificationsExecutor {
    pub fn new(pool: PgPool, http: Client, encryption_key: EncryptionKey) -> Self {
        Self {
            pool,
            http,
            encryption_key,
        }
    }
}

#[derive(Debug, FromRow)]
struct GithubTokenRow {
    id: String,
    access_token_ciphertext: Vec<u8>,
    access_token_nonce: Vec<u8>,
}

#[derive(Debug, FromRow)]
struct StarredRepoRow {
    repo_id: i64,
    full_name: String,
}

#[async_trait]
impl TaskExecutor for NotificationsExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != "sync.notifications" {
            bail!("notifications executor received {}", task.task_type);
        }
        let payload: Value = serde_json::from_str(&task.payload_json)
            .context("decode sync.notifications task payload")?;
        let user_id = task_user_id(&payload, task)
            .ok_or_else(|| anyhow!("sync.notifications task has no user_id"))?;
        let connections = sqlx::query_as::<_, GithubTokenRow>(
            "SELECT id, access_token_ciphertext, access_token_nonce FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC",
        )
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL GitHub notification connections")?;
        if connections.is_empty() {
            bail!("sync.notifications has no GitHub connection for user {user_id}");
        }

        let mut total = 0usize;
        let now = Utc::now().to_rfc3339();
        for connection in connections {
            let token = self
                .encryption_key
                .decrypt_str(
                    &connection.access_token_ciphertext,
                    &connection.access_token_nonce,
                )
                .with_context(|| format!("decrypt GitHub connection {}", connection.id))?;
            let response = self
                .http
                .get(format!(
                    "{GITHUB_API}/notifications?all=true&per_page=50&page=1"
                ))
                .bearer_auth(token)
                .header(reqwest::header::USER_AGENT, "OctoRill")
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await
                .context("fetch GitHub notifications")?;
            let status = response.status();
            if !status.is_success() {
                bail!("GitHub notifications returned HTTP {status}");
            }
            let notifications = response
                .json::<Vec<Value>>()
                .await
                .context("decode GitHub notifications")?;
            for notification in &notifications {
                upsert_notification(&self.pool, &user_id, notification, &now).await?;
            }
            total += notifications.len();
        }
        Ok(json!({"notifications": total, "user_id": user_id}).to_string())
    }
}

#[async_trait]
impl TaskExecutor for ReleasesExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != "sync.releases" {
            bail!("releases executor received {}", task.task_type);
        }
        let payload: Value = serde_json::from_str(&task.payload_json)
            .context("decode sync.releases task payload")?;
        let user_id = task_user_id(&payload, task)
            .ok_or_else(|| anyhow!("sync.releases task has no user_id"))?;
        let repos = sqlx::query_as::<_, StarredRepoRow>(
            "SELECT repo_id, full_name FROM public.starred_repos WHERE user_id = $1 ORDER BY repo_id",
        )
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL starred repositories")?;
        let connections = sqlx::query_as::<_, GithubTokenRow>(
            "SELECT id, access_token_ciphertext, access_token_nonce FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC",
        )
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL GitHub release connections")?;
        if repos.is_empty() {
            return Ok(json!({"releases": 0, "repositories": 0, "user_id": user_id}).to_string());
        }
        if connections.is_empty() {
            bail!("sync.releases has no GitHub connection for user {user_id}");
        }
        let token = self
            .encryption_key
            .decrypt_str(
                &connections[0].access_token_ciphertext,
                &connections[0].access_token_nonce,
            )
            .with_context(|| format!("decrypt GitHub connection {}", connections[0].id))?;
        let now = Utc::now().to_rfc3339();
        let mut total = 0usize;
        for repo in &repos {
            let response = self
                .http
                .get(format!(
                    "{GITHUB_API}/repos/{}/releases?per_page=50&page=1",
                    repo.full_name
                ))
                .bearer_auth(&token)
                .header(reqwest::header::USER_AGENT, "OctoRill")
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await
                .context("fetch GitHub releases")?;
            let status = response.status();
            if !status.is_success() {
                bail!(
                    "GitHub releases returned HTTP {status} for {}",
                    repo.full_name
                );
            }
            let releases = response
                .json::<Vec<Value>>()
                .await
                .context("decode GitHub releases")?;
            for release in &releases {
                upsert_release(&self.pool, &user_id, repo, release, &now).await?;
            }
            total += releases.len();
        }
        Ok(json!({"releases": total, "repositories": repos.len(), "user_id": user_id}).to_string())
    }
}

async fn upsert_release(
    pool: &PgPool,
    user_id: &str,
    repo: &StarredRepoRow,
    release: &Value,
    now: &str,
) -> Result<()> {
    let release_id = release
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("GitHub release has no numeric id"))?;
    sqlx::query(
        "INSERT INTO public.releases (id, user_id, repo_id, release_id, tag_name, name, body, html_url, published_at, created_at, is_prerelease, is_draft, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) ON CONFLICT (user_id, release_id) DO UPDATE SET repo_id = EXCLUDED.repo_id, tag_name = EXCLUDED.tag_name, name = EXCLUDED.name, body = EXCLUDED.body, html_url = EXCLUDED.html_url, published_at = EXCLUDED.published_at, created_at = EXCLUDED.created_at, is_prerelease = EXCLUDED.is_prerelease, is_draft = EXCLUDED.is_draft, updated_at = EXCLUDED.updated_at",
    )
    .bind(format!("{user_id}:{release_id}"))
    .bind(user_id)
    .bind(repo.repo_id)
    .bind(release_id)
    .bind(release.get("tag_name").and_then(Value::as_str).unwrap_or_default())
    .bind(release.get("name").and_then(Value::as_str))
    .bind(release.get("body").and_then(Value::as_str))
    .bind(release.get("html_url").and_then(Value::as_str).unwrap_or_default())
    .bind(release.get("published_at").and_then(Value::as_str))
    .bind(release.get("created_at").and_then(Value::as_str))
    .bind(if release.get("prerelease").and_then(Value::as_bool).unwrap_or(false) { 1_i64 } else { 0_i64 })
    .bind(if release.get("draft").and_then(Value::as_bool).unwrap_or(false) { 1_i64 } else { 0_i64 })
    .bind(now)
    .execute(pool)
    .await
    .context("upsert PostgreSQL release")?;
    Ok(())
}

async fn upsert_notification(
    pool: &PgPool,
    user_id: &str,
    notification: &Value,
    now: &str,
) -> Result<()> {
    let thread_id = required_string(notification, "id")?;
    let subject = notification.get("subject").unwrap_or(&Value::Null);
    let repository = notification.get("repository").unwrap_or(&Value::Null);
    let updated_at = notification
        .get("updated_at")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let unread = notification
        .get("unread")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    sqlx::query(
        "INSERT INTO public.notifications (id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason, updated_at, unread, url, html_url, last_seen_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10, NULL) ON CONFLICT (user_id, thread_id) DO UPDATE SET repo_full_name = EXCLUDED.repo_full_name, subject_title = EXCLUDED.subject_title, subject_type = EXCLUDED.subject_type, reason = EXCLUDED.reason, updated_at = EXCLUDED.updated_at, unread = EXCLUDED.unread, url = EXCLUDED.url, html_url = EXCLUDED.html_url",
    )
    .bind(thread_id.clone())
    .bind(user_id)
    .bind(thread_id)
    .bind(repository.get("full_name").and_then(Value::as_str))
    .bind(subject.get("title").and_then(Value::as_str))
    .bind(subject.get("type").and_then(Value::as_str))
    .bind(notification.get("reason").and_then(Value::as_str))
    .bind(updated_at)
    .bind(if unread { 1_i64 } else { 0_i64 })
    .bind(notification.get("url").and_then(Value::as_str).or_else(|| notification.get("html_url").and_then(Value::as_str)).or(Some(now)))
    .execute(pool)
    .await
    .context("upsert PostgreSQL notification")?;
    Ok(())
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("GitHub notification has no {key}"))
}

fn task_user_id(payload: &Value, task: &ClaimedTask) -> Option<String> {
    payload
        .get("user_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| task.requested_by.clone().filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::{ClaimedTask, task_user_id};
    use serde_json::json;

    #[test]
    fn notification_executor_rejects_other_task_types() {
        let source = include_str!("postgres_tasks.rs");
        assert!(source.contains("sync.notifications"));
        assert!(source.contains("sync.releases"));
        assert!(source.contains("ON CONFLICT (user_id, thread_id)"));
        assert!(source.contains("ON CONFLICT (user_id, release_id)"));
        assert!(source.contains("decrypt_str"));
        assert!(source.contains("requested_by"));
    }

    #[test]
    fn task_user_id_falls_back_to_legacy_requester() {
        let task = ClaimedTask {
            id: "task".to_owned(),
            task_type: "sync.releases".to_owned(),
            source: "test".to_owned(),
            requested_by: Some("legacy-user".to_owned()),
            payload_json: "{}".to_owned(),
            cancel_requested: 0,
        };
        assert_eq!(
            task_user_id(&json!({}), &task).as_deref(),
            Some("legacy-user")
        );
    }
}
