use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct LinuxDoUser {
    pub id: i64,
    pub username: String,
    pub name: Option<String>,
    pub avatar_template: String,
    pub active: bool,
    pub trust_level: i64,
    pub silenced: bool,
}

pub async fn fetch_user(http: &reqwest::Client, access_token: &str) -> Result<LinuxDoUser> {
    http.get("https://connect.linux.do/api/user")
        .bearer_auth(access_token)
        .send()
        .await
        .context("failed to fetch linuxdo user")?
        .error_for_status()
        .context("linuxdo user request failed")?
        .json::<LinuxDoUser>()
        .await
        .context("failed to decode linuxdo user")
}

pub fn normalize_display_name(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub fn resolve_avatar_url(template: &str, size: u16) -> String {
    let size = size.to_string();
    if template.contains("{size}") {
        return template.replace("{size}", &size);
    }

    let trimmed = template.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_owned();
    }

    if trimmed.starts_with('/') {
        return format!("https://linux.do{}", trimmed);
    }

    format!("https://linux.do/{trimmed}")
}

#[cfg(test)]
mod tests {
    use super::{normalize_display_name, resolve_avatar_url};

    #[test]
    fn resolve_avatar_url_replaces_size_placeholder() {
        assert_eq!(
            resolve_avatar_url(
                "https://linux.do/user_avatar/linux.do/reno/{size}/4043_2.png",
                96
            ),
            "https://linux.do/user_avatar/linux.do/reno/96/4043_2.png"
        );
    }

    #[test]
    fn resolve_avatar_url_prefixes_relative_paths() {
        assert_eq!(
            resolve_avatar_url("/user_avatar/linux.do/reno/96/4043_2.png", 96),
            "https://linux.do/user_avatar/linux.do/reno/96/4043_2.png"
        );
    }

    #[test]
    fn normalize_display_name_strips_empty_names() {
        assert_eq!(normalize_display_name(Some("  ")), None);
        assert_eq!(
            normalize_display_name(Some(" Reno ")),
            Some("Reno".to_owned())
        );
    }
}
