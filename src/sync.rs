use anyhow::{Context, Result, anyhow};
use axum::http::StatusCode;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::state::AppState;

const REST_API_BASE: &str = "https://api.github.com";
const GRAPHQL_URL: &str = "https://api.github.com/graphql";
const API_VERSION: &str = "2022-11-28";

#[derive(Debug, Serialize)]
pub struct SyncStarredResult {
    pub repos: usize,
}

#[derive(Debug, Serialize)]
pub struct SyncReleasesResult {
    pub repos: usize,
    pub releases: usize,
}

#[derive(Debug, Serialize)]
pub struct SyncNotificationsResult {
    pub notifications: usize,
    pub since: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct StarredRepoRow {
    repo_id: i64,
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct StarredData {
    viewer: Viewer,
}

#[derive(Debug, Deserialize)]
struct Viewer {
    #[serde(rename = "starredRepositories")]
    starred_repositories: StarredRepositories,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StarredRepositories {
    page_info: PageInfo,
    edges: Vec<StarredEdge>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StarredEdge {
    starred_at: String,
    node: RepoNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoNode {
    database_id: Option<i64>,
    name_with_owner: String,
    name: String,
    description: Option<String>,
    url: String,
    is_private: bool,
    owner: RepoOwner,
}

#[derive(Debug, Deserialize)]
struct RepoOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    id: i64,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    created_at: Option<String>,
    prerelease: bool,
    draft: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubNotification {
    id: String,
    unread: bool,
    reason: Option<String>,
    updated_at: Option<String>,
    url: Option<String>,
    subject: NotificationSubject,
    repository: NotificationRepo,
}

#[derive(Debug, Deserialize)]
struct NotificationSubject {
    title: Option<String>,
    #[serde(rename = "type")]
    subject_type: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotificationRepo {
    full_name: Option<String>,
    html_url: Option<String>,
}

pub async fn sync_starred(state: &AppState, user_id: i64) -> Result<SyncStarredResult> {
    let token = state.load_access_token(user_id).await?;

    let query = r#"
      query($after: String) {
        viewer {
          starredRepositories(first: 100, after: $after, orderBy: {field: STARRED_AT, direction: DESC}) {
            pageInfo { hasNextPage endCursor }
            edges {
              starredAt
              node {
                databaseId
                nameWithOwner
                name
                description
                url
                isPrivate
                owner { login }
              }
            }
          }
        }
      }
    "#;

    let mut after: Option<String> = None;
    let mut all: Vec<StarredEdge> = Vec::new();

    loop {
        let payload = json!({
            "query": query,
            "variables": { "after": after },
        });

        let resp = state
            .http
            .post(GRAPHQL_URL)
            .bearer_auth(&token)
            .header(USER_AGENT, "OctoRill")
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .json(&payload)
            .send()
            .await
            .context("github graphql request failed")?
            .error_for_status()
            .context("github graphql returned error")?
            .json::<GraphQlResponse<StarredData>>()
            .await
            .context("github graphql json decode failed")?;

        if let Some(errors) = resp.errors
            && !errors.is_empty()
        {
            let msg = errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(anyhow!("github graphql error: {msg}"));
        }

        let data = resp.data.context("missing graphql data")?;
        let page = data.viewer.starred_repositories;
        all.extend(page.edges);

        if !page.page_info.has_next_page {
            break;
        }

        after = page.page_info.end_cursor;
        if after.is_none() {
            break;
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await?;
    sqlx::query(r#"DELETE FROM starred_repos WHERE user_id = ?"#)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("failed to clear starred_repos")?;

    let mut inserted = 0usize;
    for edge in all {
        let Some(repo_id) = edge.node.database_id else {
            continue;
        };
        inserted += 1;

        sqlx::query(
            r#"
            INSERT INTO starred_repos (
              user_id, repo_id, full_name, owner_login, name, description, html_url, stargazed_at, is_private, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(user_id)
        .bind(repo_id)
        .bind(&edge.node.name_with_owner)
        .bind(&edge.node.owner.login)
        .bind(&edge.node.name)
        .bind(edge.node.description.as_deref())
        .bind(&edge.node.url)
        .bind(&edge.starred_at)
        .bind(edge.node.is_private as i64)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("failed to insert starred repo {}", edge.node.name_with_owner))?;
    }

    tx.commit().await?;

    Ok(SyncStarredResult { repos: inserted })
}

pub async fn sync_releases(state: &AppState, user_id: i64) -> Result<SyncReleasesResult> {
    let token = state.load_access_token(user_id).await?;

    let repos = sqlx::query_as::<_, StarredRepoRow>(
        r#"
        SELECT repo_id, full_name
        FROM starred_repos
        WHERE user_id = ?
        ORDER BY stargazed_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .context("failed to query starred repos")?;

    let mut total_releases = 0usize;
    let now = chrono::Utc::now().to_rfc3339();

    for repo in &repos {
        let mut page = 1usize;
        loop {
            let url = format!(
                "{REST_API_BASE}/repos/{}/releases?per_page=100&page={}",
                repo.full_name, page
            );

            let res = state
                .http
                .get(url)
                .bearer_auth(&token)
                .header(USER_AGENT, "OctoRill")
                .header(ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", API_VERSION)
                .send()
                .await
                .with_context(|| format!("github releases request failed: {}", repo.full_name))?;

            if res.status() == StatusCode::FORBIDDEN {
                let remaining = res
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if remaining == "0" {
                    return Err(anyhow!("github rate limit exceeded while syncing releases"));
                }
            }

            let releases = res
                .error_for_status()
                .with_context(|| format!("github releases returned error: {}", repo.full_name))?
                .json::<Vec<GitHubRelease>>()
                .await
                .with_context(|| {
                    format!("github releases json decode failed: {}", repo.full_name)
                })?;

            if releases.is_empty() {
                break;
            }

            for r in releases {
                total_releases += 1;
                sqlx::query(
                    r#"
                    INSERT INTO releases (
                      user_id, repo_id, release_id, tag_name, name, body, html_url,
                      published_at, created_at, is_prerelease, is_draft, updated_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(user_id, release_id) DO UPDATE SET
                      tag_name = excluded.tag_name,
                      name = excluded.name,
                      body = excluded.body,
                      html_url = excluded.html_url,
                      published_at = excluded.published_at,
                      created_at = excluded.created_at,
                      is_prerelease = excluded.is_prerelease,
                      is_draft = excluded.is_draft,
                      updated_at = excluded.updated_at
                    "#,
                )
                .bind(user_id)
                .bind(repo.repo_id)
                .bind(r.id)
                .bind(&r.tag_name)
                .bind(r.name.as_deref())
                .bind(r.body.as_deref())
                .bind(&r.html_url)
                .bind(r.published_at.as_deref())
                .bind(r.created_at.as_deref())
                .bind(r.prerelease as i64)
                .bind(r.draft as i64)
                .bind(&now)
                .execute(&state.pool)
                .await
                .with_context(|| {
                    format!("failed to upsert release {} {}", repo.full_name, r.tag_name)
                })?;
            }

            if page >= 50 {
                break;
            }
            page += 1;
        }
    }

    Ok(SyncReleasesResult {
        repos: repos.len(),
        releases: total_releases,
    })
}

pub async fn sync_notifications(state: &AppState, user_id: i64) -> Result<SyncNotificationsResult> {
    let token = state.load_access_token(user_id).await?;

    let since_key = "notifications_since";
    let since = sqlx::query_scalar::<_, String>(
        r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
    )
    .bind(user_id)
    .bind(since_key)
    .fetch_optional(&state.pool)
    .await
    .context("failed to query notifications since")?;

    let mut url = format!("{REST_API_BASE}/notifications?all=true&per_page=50");
    if let Some(ref since) = since {
        url.push_str("&since=");
        url.push_str(&urlencoding::encode(since));
    }

    let res = state
        .http
        .get(url)
        .bearer_auth(&token)
        .header(USER_AGENT, "OctoRill")
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .send()
        .await
        .context("github notifications request failed")?
        .error_for_status()
        .context("github notifications returned error")?
        .json::<Vec<GitHubNotification>>()
        .await
        .context("github notifications json decode failed")?;

    let now = chrono::Utc::now().to_rfc3339();
    for n in &res {
        sqlx::query(
            r#"
            INSERT INTO notifications (
              user_id, thread_id, repo_full_name, subject_title, subject_type, reason,
              updated_at, unread, url, html_url, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id, thread_id) DO UPDATE SET
              repo_full_name = excluded.repo_full_name,
              subject_title = excluded.subject_title,
              subject_type = excluded.subject_type,
              reason = excluded.reason,
              updated_at = excluded.updated_at,
              unread = excluded.unread,
              url = excluded.url,
              html_url = excluded.html_url
            "#,
        )
        .bind(user_id)
        .bind(&n.id)
        .bind(n.repository.full_name.as_deref())
        .bind(n.subject.title.as_deref())
        .bind(n.subject.subject_type.as_deref())
        .bind(n.reason.as_deref())
        .bind(n.updated_at.as_deref())
        .bind(n.unread as i64)
        .bind(n.url.as_deref().or(n.subject.url.as_deref()))
        .bind(n.repository.html_url.as_deref())
        .bind(&now)
        .execute(&state.pool)
        .await
        .context("failed to upsert notification")?;
    }

    sqlx::query(
        r#"
        INSERT INTO sync_state (user_id, key, value, updated_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(user_id, key) DO UPDATE SET
          value = excluded.value,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(user_id)
    .bind(since_key)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .context("failed to update notifications since")?;

    Ok(SyncNotificationsResult {
        notifications: res.len(),
        since,
    })
}
