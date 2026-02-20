use anyhow::{Context, Result};
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

const API_BASE: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct EmailItem {
    email: String,
    primary: bool,
    verified: bool,
    visibility: Option<String>,
}

pub async fn fetch_user(http: &reqwest::Client, access_token: &str) -> Result<GitHubUser> {
    let user = http
        .get(format!("{API_BASE}/user"))
        .bearer_auth(access_token)
        .header(USER_AGENT, "OctoRill")
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .send()
        .await
        .context("github /user request failed")?
        .error_for_status()
        .context("github /user returned error")?
        .json::<GitHubUser>()
        .await
        .context("github /user json decode failed")?;
    Ok(user)
}

pub async fn fetch_primary_email(
    http: &reqwest::Client,
    access_token: &str,
) -> Result<Option<String>> {
    let items = http
        .get(format!("{API_BASE}/user/emails"))
        .bearer_auth(access_token)
        .header(USER_AGENT, "OctoRill")
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .send()
        .await
        .context("github /user/emails request failed")?
        .error_for_status()
        .context("github /user/emails returned error")?
        .json::<Vec<EmailItem>>()
        .await
        .context("github /user/emails json decode failed")?;

    let primary = items
        .iter()
        .find(|e| e.primary && e.verified)
        .or_else(|| items.iter().find(|e| e.primary))
        .or_else(|| items.iter().find(|e| e.verified))
        .or_else(|| {
            items
                .iter()
                .find(|e| e.visibility.as_deref() == Some("public"))
        });

    Ok(primary.map(|e| e.email.clone()))
}
