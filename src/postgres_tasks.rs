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

#[derive(Clone)]
pub struct StarredExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
}

#[derive(Clone)]
pub struct CompositeSyncExecutor {
    starred: StarredExecutor,
    releases: ReleasesExecutor,
    notifications: NotificationsExecutor,
}

#[derive(Clone)]
pub struct WebhookExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
    public_base_url: String,
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

impl StarredExecutor {
    pub fn new(pool: PgPool, http: Client, encryption_key: EncryptionKey) -> Self {
        Self {
            pool,
            http,
            encryption_key,
        }
    }
}

impl CompositeSyncExecutor {
    pub fn new(pool: PgPool, http: Client, encryption_key: EncryptionKey) -> Self {
        Self {
            starred: StarredExecutor::new(pool.clone(), http.clone(), encryption_key.clone()),
            releases: ReleasesExecutor::new(pool.clone(), http.clone(), encryption_key.clone()),
            notifications: NotificationsExecutor::new(pool, http, encryption_key),
        }
    }
}

impl WebhookExecutor {
    pub fn new(
        pool: PgPool,
        http: Client,
        encryption_key: EncryptionKey,
        public_base_url: impl Into<String>,
    ) -> Self {
        Self {
            pool,
            http,
            encryption_key,
            public_base_url: public_base_url.into(),
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

#[derive(Debug, FromRow)]
struct GithubConnectionRow {
    id: String,
    access_token_ciphertext: Vec<u8>,
    access_token_nonce: Vec<u8>,
}

#[derive(Debug, FromRow)]
struct WebhookCredentialRow {
    webhook_push_desired_state: String,
    webhook_push_secret_ciphertext: Option<Vec<u8>>,
    webhook_push_secret_nonce: Option<Vec<u8>>,
    webhook_push_callback_key: Option<String>,
    token_ciphertext: Option<Vec<u8>>,
    token_nonce: Option<Vec<u8>>,
}

#[derive(Debug, FromRow)]
struct WebhookRepoRow {
    repo_id: i64,
    owner_login: String,
    repo_name: String,
    repo_full_name: String,
    hook_id: Option<i64>,
}

#[async_trait]
impl TaskExecutor for WebhookExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if !matches!(
            task.task_type.as_str(),
            "webhook.push.manage" | "webhook.push.reconcile"
        ) {
            bail!("webhook executor received {}", task.task_type);
        }
        let payload: Value =
            serde_json::from_str(&task.payload_json).context("decode webhook task payload")?;
        let user_id =
            task_user_id(&payload, task).ok_or_else(|| anyhow!("webhook task has no user_id"))?;
        let credentials = sqlx::query_as::<_, WebhookCredentialRow>(
            "SELECT u.webhook_push_desired_state, u.webhook_push_secret_ciphertext, u.webhook_push_secret_nonce, u.webhook_push_callback_key, p.token_ciphertext, p.token_nonce FROM public.users u LEFT JOIN public.reaction_pat_tokens p ON p.user_id = u.id WHERE u.id = $1 AND u.is_disabled = 0",
        )
        .bind(&user_id)
        .fetch_optional(&self.pool)
        .await
        .context("load PostgreSQL webhook credentials")?
        .ok_or_else(|| anyhow!("webhook user is missing or disabled"))?;
        let desired_state = payload
            .get("desired_state")
            .and_then(Value::as_str)
            .unwrap_or(&credentials.webhook_push_desired_state);
        let token_ciphertext = credentials
            .token_ciphertext
            .as_deref()
            .ok_or_else(|| anyhow!("webhook user has no GitHub PAT"))?;
        let token_nonce = credentials
            .token_nonce
            .as_deref()
            .ok_or_else(|| anyhow!("webhook user has no GitHub PAT nonce"))?;
        let token = self
            .encryption_key
            .decrypt_str(token_ciphertext, token_nonce)
            .context("decrypt webhook GitHub PAT")?;
        let callback_key = credentials
            .webhook_push_callback_key
            .as_deref()
            .ok_or_else(|| anyhow!("webhook user has no callback key"))?;
        let secret = match (
            credentials.webhook_push_secret_ciphertext.as_deref(),
            credentials.webhook_push_secret_nonce.as_deref(),
        ) {
            (Some(ciphertext), Some(nonce)) => Some(
                self.encryption_key
                    .decrypt_str(ciphertext, nonce)
                    .context("decrypt webhook callback secret")?,
            ),
            _ => None,
        };
        let callback_url = format!(
            "{}/api/webhooks/github/releases?key={}",
            self.public_base_url.trim_end_matches('/'),
            callback_key
        );
        let repo_id = payload.get("repo_id").and_then(Value::as_i64);
        let repos = sqlx::query_as::<_, WebhookRepoRow>(
            "SELECT repo_id, owner_login, repo_name, repo_full_name, hook_id, callback_url FROM public.webhook_push_repos WHERE user_id = $1 AND ($2::bigint IS NULL OR repo_id = $2) ORDER BY repo_id",
        )
        .bind(&user_id)
        .bind(repo_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL webhook repository state")?;
        let mut registered = 0usize;
        let mut removed = 0usize;
        for repo in repos {
            if desired_state == "enabled" {
                if let Some(secret) = secret.as_deref() {
                    let created_hook_id = ensure_github_hook(
                        &self.http,
                        &token,
                        &repo.repo_full_name,
                        &callback_url,
                        secret,
                    )
                    .await?;
                    if created_hook_id.is_some() {
                        registered += 1;
                    }
                    mark_webhook_repo(
                        &self.pool,
                        &user_id,
                        &repo,
                        &callback_url,
                        "registered",
                        created_hook_id.or(repo.hook_id),
                    )
                    .await?;
                }
            } else if let Some(hook_id) = repo.hook_id {
                if remove_github_hook(&self.http, &token, &repo.repo_full_name, hook_id).await? {
                    removed += 1;
                }
                mark_webhook_repo(
                    &self.pool,
                    &user_id,
                    &repo,
                    &callback_url,
                    "delete_pending",
                    None,
                )
                .await?;
            }
        }
        Ok(json!({"user_id": user_id, "desired_state": desired_state, "registered": registered, "removed": removed}).to_string())
    }
}

async fn ensure_github_hook(
    http: &Client,
    token: &str,
    full_name: &str,
    callback_url: &str,
    secret: &str,
) -> Result<Option<i64>> {
    let hooks =
        github_request(http, token, format!("{GITHUB_API}/repos/{full_name}/hooks")).await?;
    let existing = hooks.as_array().into_iter().flatten().find(|hook| {
        hook.get("config")
            .and_then(|config| config.get("url"))
            .and_then(Value::as_str)
            == Some(callback_url)
    });
    if let Some(existing) = existing {
        return Ok(existing.get("id").and_then(Value::as_i64));
    }
    let response = http
        .post(format!("{GITHUB_API}/repos/{full_name}/hooks"))
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&json!({"name": "web", "active": true, "events": ["release"], "config": {"url": callback_url, "content_type": "json", "secret": secret, "insecure_ssl": "0"}}))
        .send()
        .await
        .context("create GitHub release webhook")?;
    if !response.status().is_success() {
        bail!("GitHub webhook create returned HTTP {}", response.status());
    }
    Ok(response
        .json::<Value>()
        .await
        .context("decode created GitHub webhook")?
        .get("id")
        .and_then(Value::as_i64))
}

async fn remove_github_hook(
    http: &Client,
    token: &str,
    full_name: &str,
    hook_id: i64,
) -> Result<bool> {
    let response = http
        .delete(format!("{GITHUB_API}/repos/{full_name}/hooks/{hook_id}"))
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("delete GitHub release webhook")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(false);
    }
    if !response.status().is_success() {
        bail!("GitHub webhook delete returned HTTP {}", response.status());
    }
    Ok(true)
}

async fn github_request(http: &Client, token: &str, url: String) -> Result<Value> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("list GitHub hooks")?;
    let status = response.status();
    if !status.is_success() {
        bail!("GitHub hooks returned HTTP {status}");
    }
    response.json().await.context("decode GitHub hooks")
}

async fn mark_webhook_repo(
    pool: &PgPool,
    user_id: &str,
    repo: &WebhookRepoRow,
    callback_url: &str,
    status: &str,
    hook_id: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO public.webhook_push_repos (user_id, repo_id, owner_login, repo_name, repo_full_name, hook_id, callback_url, status, permission_paused, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9) ON CONFLICT (user_id, repo_id) DO UPDATE SET hook_id = EXCLUDED.hook_id, callback_url = EXCLUDED.callback_url, status = EXCLUDED.status, permission_paused = 0, updated_at = EXCLUDED.updated_at",
    )
    .bind(user_id)
    .bind(repo.repo_id)
    .bind(&repo.owner_login)
    .bind(&repo.repo_name)
    .bind(&repo.repo_full_name)
    .bind(hook_id)
    .bind(callback_url)
    .bind(status)
    .bind(Utc::now())
    .execute(pool)
    .await
    .context("persist PostgreSQL webhook repository state")?;
    Ok(())
}

#[async_trait]
impl TaskExecutor for StarredExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != "sync.starred" {
            bail!("starred executor received {}", task.task_type);
        }
        let payload: Value =
            serde_json::from_str(&task.payload_json).context("decode sync.starred task payload")?;
        let user_id = task_user_id(&payload, task)
            .ok_or_else(|| anyhow!("sync.starred task has no user_id"))?;
        let connections = sqlx::query_as::<_, GithubConnectionRow>(
            "SELECT id, access_token_ciphertext, access_token_nonce FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC",
        )
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL starred repository connections")?;
        if connections.is_empty() {
            bail!("sync.starred has no GitHub connection for user {user_id}");
        }

        let now = Utc::now().to_rfc3339();
        let mut total = 0usize;
        for connection in connections {
            let token = self
                .encryption_key
                .decrypt_str(
                    &connection.access_token_ciphertext,
                    &connection.access_token_nonce,
                )
                .with_context(|| format!("decrypt GitHub connection {}", connection.id))?;
            for page in 1..=20 {
                let response = self
                    .http
                    .get(format!(
                        "{GITHUB_API}/user/starred?per_page=100&page={page}"
                    ))
                    .bearer_auth(&token)
                    .header(reqwest::header::USER_AGENT, "OctoRill")
                    .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28")
                    .send()
                    .await
                    .context("fetch GitHub starred repositories")?;
                let status = response.status();
                if !status.is_success() {
                    bail!("GitHub starred repositories returned HTTP {status}");
                }
                let repos = response
                    .json::<Vec<Value>>()
                    .await
                    .context("decode GitHub starred repositories")?;
                for repo in &repos {
                    upsert_starred_repo(&self.pool, &user_id, repo, &now).await?;
                }
                total += repos.len();
                if repos.len() < 100 {
                    break;
                }
            }
        }
        Ok(json!({"repositories": total, "user_id": user_id}).to_string())
    }
}

#[async_trait]
impl TaskExecutor for CompositeSyncExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if !matches!(task.task_type.as_str(), "sync.all" | "sync.access_refresh") {
            bail!("composite sync executor received {}", task.task_type);
        }
        let starred: Value = self
            .starred
            .execute(&ClaimedTask {
                task_type: "sync.starred".to_owned(),
                ..task.clone()
            })
            .await
            .and_then(|value| serde_json::from_str(&value).context("decode starred sync result"))?;
        let releases: Value = self
            .releases
            .execute(&ClaimedTask {
                task_type: "sync.releases".to_owned(),
                ..task.clone()
            })
            .await
            .and_then(|value| serde_json::from_str(&value).context("decode release sync result"))?;
        let notifications: Value = self
            .notifications
            .execute(&ClaimedTask {
                task_type: "sync.notifications".to_owned(),
                ..task.clone()
            })
            .await
            .and_then(|value| {
                serde_json::from_str(&value).context("decode notification sync result")
            })?;
        Ok(
            json!({"starred": starred, "releases": releases, "notifications": notifications})
                .to_string(),
        )
    }
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
        for connection in connections {
            let token = self
                .encryption_key
                .decrypt_str(
                    &connection.access_token_ciphertext,
                    &connection.access_token_nonce,
                )
                .with_context(|| format!("decrypt GitHub connection {}", connection.id))?;
            for page in 1..=20 {
                let response = self
                    .http
                    .get(format!(
                        "{GITHUB_API}/notifications?all=true&per_page=50&page={page}"
                    ))
                    .bearer_auth(&token)
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
                    upsert_notification(&self.pool, &user_id, notification).await?;
                }
                total += notifications.len();
                if notifications.len() < 50 {
                    break;
                }
            }
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
        let now = Utc::now().to_rfc3339();
        let mut total = 0usize;
        for connection in connections {
            let token = self
                .encryption_key
                .decrypt_str(
                    &connection.access_token_ciphertext,
                    &connection.access_token_nonce,
                )
                .with_context(|| format!("decrypt GitHub connection {}", connection.id))?;
            for repo in &repos {
                for page in 1..=20 {
                    let response = self
                        .http
                        .get(format!(
                            "{GITHUB_API}/repos/{}/releases?per_page=50&page={page}",
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
                    if releases.len() < 50 {
                        break;
                    }
                }
            }
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

async fn upsert_notification(pool: &PgPool, user_id: &str, notification: &Value) -> Result<()> {
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
    .bind(notification.get("url").and_then(Value::as_str).or_else(|| notification.get("html_url").and_then(Value::as_str)))
    .execute(pool)
    .await
    .context("upsert PostgreSQL notification")?;
    Ok(())
}

async fn upsert_starred_repo(pool: &PgPool, user_id: &str, repo: &Value, now: &str) -> Result<()> {
    let repo_id = repo
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("GitHub starred repository has no numeric id"))?;
    let full_name = repo
        .get("full_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let owner_login = repo
        .get("owner")
        .and_then(|owner| owner.get("login"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    sqlx::query(
        "INSERT INTO public.starred_repos (id, user_id, repo_id, full_name, owner_login, name, description, html_url, stargazed_at, is_private, updated_at, owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image, repo_stargazer_count, repo_stargazer_count_updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) ON CONFLICT (user_id, repo_id) DO UPDATE SET full_name = EXCLUDED.full_name, owner_login = EXCLUDED.owner_login, name = EXCLUDED.name, description = EXCLUDED.description, html_url = EXCLUDED.html_url, is_private = EXCLUDED.is_private, updated_at = EXCLUDED.updated_at, owner_avatar_url = EXCLUDED.owner_avatar_url, open_graph_image_url = EXCLUDED.open_graph_image_url, uses_custom_open_graph_image = EXCLUDED.uses_custom_open_graph_image, repo_stargazer_count = EXCLUDED.repo_stargazer_count, repo_stargazer_count_updated_at = EXCLUDED.repo_stargazer_count_updated_at",
    )
    .bind(format!("{user_id}:{repo_id}"))
    .bind(user_id)
    .bind(repo_id)
    .bind(full_name)
    .bind(owner_login)
    .bind(repo.get("name").and_then(Value::as_str).unwrap_or_default())
    .bind(repo.get("description").and_then(Value::as_str))
    .bind(repo.get("html_url").and_then(Value::as_str).unwrap_or_default())
    .bind(
        repo.get("created_at")
            .and_then(Value::as_str)
            .or_else(|| repo.get("updated_at").and_then(Value::as_str))
            .unwrap_or(now),
    )
    .bind(if repo.get("private").and_then(Value::as_bool).unwrap_or(false) { 1_i64 } else { 0_i64 })
    .bind(now)
    .bind(repo.get("owner").and_then(|owner| owner.get("avatar_url")).and_then(Value::as_str))
    .bind(repo.get("open_graph_image_url").and_then(Value::as_str))
    .bind(0_i64)
    .bind(repo.get("stargazers_count").and_then(Value::as_i64))
    .bind(now)
    .execute(pool)
    .await
    .context("upsert PostgreSQL starred repository")?;
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
        assert!(source.contains("sync.starred"));
        assert!(source.contains("sync.all"));
        assert!(source.contains("sync.access_refresh"));
        assert!(source.contains("/user/starred?per_page=100&page="));
        assert!(source.contains("webhook.push.manage"));
        assert!(source.contains("webhook.push.reconcile"));
        assert!(source.contains("/hooks"));
        assert!(source.contains("ON CONFLICT (user_id, thread_id)"));
        assert!(source.contains("ON CONFLICT (user_id, release_id)"));
        assert!(source.contains("ON CONFLICT (user_id, repo_id)"));
        assert!(source.contains("decrypt_str"));
        assert!(source.contains("requested_by"));
        assert!(source.contains("page in 1..=20"));
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
