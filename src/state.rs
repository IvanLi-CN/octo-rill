use anyhow::{Context, Result};
use oauth2::{
    AuthUrl, ClientId, ClientSecret, EndpointNotSet, EndpointSet, RedirectUrl, TokenUrl,
    basic::BasicClient,
};
use sqlx::SqlitePool;
use std::sync::Arc;
use url::Url;

use crate::{
    ai::LlmScheduler, config::AppConfig, crypto::EncryptionKey, local_id,
    translations::TranslationSchedulerController,
};

pub type GitHubOAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;
pub type LinuxDoOAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub pool: SqlitePool,
    pub http: reqwest::Client,
    pub github_oauth: GitHubOAuthClient,
    pub linuxdo_oauth: Option<LinuxDoOAuthClient>,
    pub encryption_key: EncryptionKey,
    pub llm_scheduler: Arc<LlmScheduler>,
    pub translation_scheduler: Arc<TranslationSchedulerController>,
    pub runtime_owner_id: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GitHubConnectionRow {
    pub id: String,
    pub login: String,
    pub access_token_ciphertext: Vec<u8>,
    pub access_token_nonce: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct GitHubConnectionAuth {
    pub id: String,
    pub login: String,
    pub access_token: String,
}

impl AppState {
    pub async fn load_github_connections(
        &self,
        user_id: &str,
    ) -> Result<Vec<GitHubConnectionAuth>> {
        let rows = sqlx::query_as::<_, GitHubConnectionRow>(
            r#"
            SELECT
              id,
              login,
              access_token_ciphertext,
              access_token_nonce
            FROM github_connections
            WHERE user_id = ?
            ORDER BY linked_at ASC, id ASC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("failed to load github connections")?;

        rows.into_iter()
            .map(|row| self.decrypt_github_connection(row))
            .collect()
    }

    fn decrypt_github_connection(&self, row: GitHubConnectionRow) -> Result<GitHubConnectionAuth> {
        Ok(GitHubConnectionAuth {
            id: row.id,
            login: row.login,
            access_token: self
                .encryption_key
                .decrypt_str(&row.access_token_ciphertext, &row.access_token_nonce)?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LegacyGitHubConnectionBackfillRow {
    user_id: String,
    github_user_id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
    access_token_ciphertext: Vec<u8>,
    access_token_nonce: Vec<u8>,
    scopes: String,
    created_at: String,
    updated_at: String,
}

pub async fn backfill_github_connections(pool: &SqlitePool) -> Result<()> {
    let legacy_rows = sqlx::query_as::<_, LegacyGitHubConnectionBackfillRow>(
        r#"
        SELECT
          u.id AS user_id,
          u.github_user_id,
          u.login,
          u.name,
          u.avatar_url,
          u.email,
          t.access_token_ciphertext,
          t.access_token_nonce,
          t.scopes,
          u.created_at,
          u.updated_at
        FROM users u
        JOIN user_tokens t ON t.user_id = u.id
        WHERE NOT EXISTS (
          SELECT 1
          FROM github_connections gc
          WHERE gc.user_id = u.id
        )
        ORDER BY u.created_at ASC, u.id ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .context("failed to load legacy github users for backfill")?;

    if !legacy_rows.is_empty() {
        let mut tx = pool
            .begin()
            .await
            .context("begin github connection backfill")?;
        for row in legacy_rows {
            sqlx::query(
                r#"
                INSERT INTO github_connections (
                  id,
                  user_id,
                  github_user_id,
                  login,
                  name,
                  avatar_url,
                  email,
                  access_token_ciphertext,
                  access_token_nonce,
                  scopes,
                  linked_at,
                  updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(local_id::generate_local_id())
            .bind(row.user_id)
            .bind(row.github_user_id)
            .bind(row.login)
            .bind(row.name)
            .bind(row.avatar_url)
            .bind(row.email)
            .bind(row.access_token_ciphertext)
            .bind(row.access_token_nonce)
            .bind(row.scopes)
            .bind(row.created_at)
            .bind(row.updated_at)
            .execute(&mut *tx)
            .await
            .context("failed to backfill github connection")?;
        }
        tx.commit()
            .await
            .context("commit github connection backfill")?;
    }

    sqlx::query(
        r#"
        UPDATE reaction_pat_tokens
        SET
          owner_github_connection_id = COALESCE(
            owner_github_connection_id,
            (
              SELECT gc.id
              FROM github_connections gc
              WHERE gc.user_id = reaction_pat_tokens.user_id
              ORDER BY gc.linked_at ASC, gc.id ASC
              LIMIT 1
            )
          ),
          owner_github_user_id = COALESCE(
            owner_github_user_id,
            (
              SELECT gc.github_user_id
              FROM github_connections gc
              WHERE gc.user_id = reaction_pat_tokens.user_id
              ORDER BY gc.linked_at ASC, gc.id ASC
              LIMIT 1
            )
          ),
          owner_login = COALESCE(
            owner_login,
            (
              SELECT gc.login
              FROM github_connections gc
              WHERE gc.user_id = reaction_pat_tokens.user_id
              ORDER BY gc.linked_at ASC, gc.id ASC
              LIMIT 1
            )
          )
        WHERE owner_github_connection_id IS NULL
           OR owner_github_user_id IS NULL
           OR owner_login IS NULL
        "#,
    )
    .execute(pool)
    .await
    .context("failed to backfill reaction PAT owners")?;

    Ok(())
}

pub fn build_oauth_client(config: &AppConfig) -> Result<GitHubOAuthClient> {
    let auth_url = AuthUrl::new("https://github.com/login/oauth/authorize".to_owned())
        .context("invalid github auth url")?;
    let token_url = TokenUrl::new("https://github.com/login/oauth/access_token".to_owned())
        .context("invalid github token url")?;

    let redirect_url = RedirectUrl::new(config.github.redirect_url.to_string())
        .context("invalid github redirect url")?;

    let client = BasicClient::new(ClientId::new(config.github.client_id.clone()))
        .set_client_secret(ClientSecret::new(config.github.client_secret.clone()))
        .set_auth_uri(auth_url)
        .set_token_uri(token_url)
        .set_redirect_uri(redirect_url);

    Ok(client)
}

pub fn build_linuxdo_oauth_client(config: &AppConfig) -> Result<Option<LinuxDoOAuthClient>> {
    let Some(linuxdo) = config.linuxdo.as_ref() else {
        return Ok(None);
    };

    let auth_url = AuthUrl::new("https://connect.linux.do/oauth2/authorize".to_owned())
        .context("invalid linuxdo auth url")?;
    let token_url = TokenUrl::new("https://connect.linux.do/oauth2/token".to_owned())
        .context("invalid linuxdo token url")?;
    let redirect_url = RedirectUrl::new(linuxdo.redirect_url.to_string())
        .context("invalid linuxdo redirect url")?;

    let client = BasicClient::new(ClientId::new(linuxdo.client_id.clone()))
        .set_client_secret(ClientSecret::new(linuxdo.client_secret.clone()))
        .set_auth_uri(auth_url)
        .set_token_uri(token_url)
        .set_redirect_uri(redirect_url);

    Ok(Some(client))
}

pub fn normalize_origin(url: &Url) -> Result<Url> {
    let origin = url.origin().ascii_serialization();
    Url::parse(&origin).context("failed to normalize origin url")
}
