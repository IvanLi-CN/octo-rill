use anyhow::{Context, Result};
use oauth2::{
    AuthUrl, ClientId, ClientSecret, EndpointNotSet, EndpointSet, RedirectUrl, TokenUrl,
    basic::BasicClient,
};
use sqlx::SqlitePool;
use url::Url;

use crate::{config::AppConfig, crypto::EncryptionKey};

pub type GitHubOAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub pool: SqlitePool,
    pub http: reqwest::Client,
    pub oauth: GitHubOAuthClient,
    pub encryption_key: EncryptionKey,
}

#[derive(Debug, sqlx::FromRow)]
struct TokenRow {
    access_token_ciphertext: Vec<u8>,
    access_token_nonce: Vec<u8>,
}

impl AppState {
    pub async fn load_access_token(&self, user_id: i64) -> Result<String> {
        let row = sqlx::query_as::<_, TokenRow>(
            r#"
            SELECT access_token_ciphertext, access_token_nonce
            FROM user_tokens
            WHERE user_id = ?
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .context("access token not found for user")?;

        self.encryption_key
            .decrypt_str(&row.access_token_ciphertext, &row.access_token_nonce)
    }
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

pub fn normalize_origin(url: &Url) -> Result<Url> {
    let origin = url.origin().ascii_serialization();
    Url::parse(&origin).context("failed to normalize origin url")
}
