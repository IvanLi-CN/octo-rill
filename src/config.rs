use std::{
    env, fmt,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use anyhow::{Context, Result};
use url::Url;

use crate::crypto::EncryptionKey;

fn ensure_trailing_slash(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    url
}

#[derive(Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub public_base_url: Url,
    pub database_url: String,
    pub static_dir: Option<PathBuf>,
    pub encryption_key: EncryptionKey,
    pub github: GitHubOAuthConfig,
    pub ai: Option<AiConfig>,
    pub ai_model_context_limit: Option<u32>,
    pub ai_daily_at_local: Option<chrono::NaiveTime>,
}

#[derive(Clone)]
pub struct GitHubOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: Url,
}

#[derive(Clone)]
pub struct AiConfig {
    pub base_url: Url,
    pub model: String,
    pub api_key: String,
}

impl fmt::Debug for AiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl fmt::Debug for GitHubOAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubOAuthConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_url", &self.redirect_url)
            .finish()
    }
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppConfig")
            .field("bind_addr", &self.bind_addr)
            .field("public_base_url", &self.public_base_url)
            .field("database_url", &self.database_url)
            .field("static_dir", &self.static_dir)
            .field("github", &self.github)
            .field("ai", &self.ai)
            .field("ai_model_context_limit", &self.ai_model_context_limit)
            .field("ai_daily_at_local", &self.ai_daily_at_local)
            .field("encryption_key", &"<redacted>")
            .finish()
    }
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let bind_addr: SocketAddr = env::var("OCTORILL_BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:58090".to_owned())
            .parse()
            .context("invalid OCTORILL_BIND_ADDR (expected ip:port)")?;

        let default_host = match bind_addr.ip() {
            IpAddr::V4(v4) if v4.is_unspecified() => "127.0.0.1".to_owned(),
            IpAddr::V6(v6) if v6.is_unspecified() => "127.0.0.1".to_owned(),
            ip => ip.to_string(),
        };
        let public_base_url = env::var("OCTORILL_PUBLIC_BASE_URL")
            .unwrap_or_else(|_| format!("http://{}:{}", default_host, bind_addr.port()));
        let public_base_url =
            Url::parse(&public_base_url).context("invalid OCTORILL_PUBLIC_BASE_URL")?;

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./.data/octo-rill.db".to_owned());

        let encryption_key = env::var("OCTORILL_ENCRYPTION_KEY_BASE64")
            .context("OCTORILL_ENCRYPTION_KEY_BASE64 is required")?;
        let encryption_key = EncryptionKey::from_base64(&encryption_key)?;

        let github_client_id =
            env::var("GITHUB_CLIENT_ID").context("GITHUB_CLIENT_ID is required")?;
        let github_client_secret =
            env::var("GITHUB_CLIENT_SECRET").context("GITHUB_CLIENT_SECRET is required")?;
        let github_redirect_url = env::var("GITHUB_OAUTH_REDIRECT_URL")
            .context("GITHUB_OAUTH_REDIRECT_URL is required")?;
        let github_redirect_url =
            Url::parse(&github_redirect_url).context("invalid GITHUB_OAUTH_REDIRECT_URL")?;

        let ai = {
            let api_key = env::var("AI_API_KEY")
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty());

            api_key.map(|api_key| {
                let base_url = env::var("AI_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1/".to_owned());
                let base_url = Url::parse(&base_url).context("invalid AI_BASE_URL")?;
                let base_url = ensure_trailing_slash(base_url);
                let model = env::var("AI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_owned());
                Ok::<_, anyhow::Error>(AiConfig {
                    base_url,
                    model,
                    api_key,
                })
            })
        }
        .transpose()?;

        let ai_model_context_limit = env::var("AI_MODEL_CONTEXT_LIMIT")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .map(|raw| {
                raw.parse::<u32>()
                    .context("invalid AI_MODEL_CONTEXT_LIMIT (expected positive integer)")
            })
            .transpose()?;

        let ai_daily_at_local = env::var("AI_DAILY_AT_LOCAL")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .map(|raw| {
                chrono::NaiveTime::parse_from_str(&raw, "%H:%M")
                    .context("invalid AI_DAILY_AT_LOCAL (expected HH:MM)")
            })
            .transpose()?
            .or_else(|| chrono::NaiveTime::from_hms_opt(8, 0, 0));

        let static_dir = {
            let candidate = PathBuf::from("web/dist");
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        };

        Ok(Self {
            bind_addr,
            public_base_url,
            database_url,
            static_dir,
            encryption_key,
            github: GitHubOAuthConfig {
                client_id: github_client_id,
                client_secret: github_client_secret,
                redirect_url: github_redirect_url,
            },
            ai,
            ai_model_context_limit,
            ai_daily_at_local,
        })
    }
}
