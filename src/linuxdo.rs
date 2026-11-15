use anyhow::{Context, Result};
use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};

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

/// Persist a LinuxDo connection in the PostgreSQL account store.  The
/// caller owns the surrounding transaction so binding and any pending
/// passkey/account updates can commit atomically.
pub async fn upsert_connection_postgres(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    user: &LinuxDoUser,
    now: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO public.linuxdo_connections
         (user_id, linuxdo_user_id, username, name, avatar_url, trust_level, active, silenced, linked_at, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$9)
         ON CONFLICT (user_id) DO UPDATE SET
           linuxdo_user_id=EXCLUDED.linuxdo_user_id, username=EXCLUDED.username,
           name=EXCLUDED.name, avatar_url=EXCLUDED.avatar_url,
           trust_level=EXCLUDED.trust_level, active=EXCLUDED.active,
           silenced=EXCLUDED.silenced, updated_at=EXCLUDED.updated_at",
    )
    .bind(user_id)
    .bind(user.id)
    .bind(&user.username)
    .bind(normalize_display_name(user.name.as_deref()))
    .bind(resolve_avatar_url(&user.avatar_template, 96))
    .bind(user.trust_level)
    .bind(user.active)
    .bind(user.silenced)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub async fn delete_connection_postgres(pool: &PgPool, user_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM public.linuxdo_connections WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
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
