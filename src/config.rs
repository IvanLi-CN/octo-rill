use std::{
    env, fmt,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    str::FromStr,
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

fn parse_positive_usize_env(name: &str, blank_is_unset: bool) -> Result<Option<usize>> {
    let Some(raw) = env::var_os(name) else {
        return Ok(None);
    };

    let raw = raw
        .into_string()
        .map_err(|_| anyhow::anyhow!("invalid {name} (expected positive integer)"))?;
    let raw = raw.trim();
    if raw.is_empty() {
        if blank_is_unset {
            return Ok(None);
        }
        anyhow::bail!("invalid {name} (expected positive integer)");
    }

    let parsed = raw
        .parse::<usize>()
        .with_context(|| format!("invalid {name} (expected positive integer)"))?;
    if parsed == 0 {
        anyhow::bail!("invalid {name} (expected positive integer)");
    }

    Ok(Some(parsed))
}

fn parse_bounded_positive_usize_env(
    name: &str,
    blank_is_unset: bool,
    max: usize,
) -> Result<Option<usize>> {
    let parsed = parse_positive_usize_env(name, blank_is_unset)?;
    if let Some(value) = parsed
        && value > max
    {
        anyhow::bail!("invalid {name} (expected positive integer <= {max})");
    }
    Ok(parsed)
}

fn validate_app_default_time_zone(raw: &str) -> Result<String> {
    let canonical = raw.trim().to_owned();
    chrono_tz::Tz::from_str(&canonical)
        .context("invalid APP_DEFAULT_TIME_ZONE (expected IANA time zone)")?;
    crate::briefs::validate_hour_aligned_time_zone(&canonical, chrono::Utc::now())
        .context("invalid APP_DEFAULT_TIME_ZONE (expected whole-hour IANA time zone year-round)")?;
    Ok(canonical)
}

fn resolve_app_default_time_zone(
    env_value: Option<String>,
    legacy_runtime_time_zone: Option<&str>,
) -> Result<String> {
    if let Some(value) = env_value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return validate_app_default_time_zone(&value);
    }

    if let Some(legacy_runtime_time_zone) = legacy_runtime_time_zone
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && let Ok(validated) = validate_app_default_time_zone(legacy_runtime_time_zone)
    {
        return Ok(validated);
    }

    Ok(crate::briefs::DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned())
}

#[derive(Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub public_base_url: Url,
    pub session_cookie_name: Option<String>,
    pub database_url: String,
    pub static_dir: Option<PathBuf>,
    pub task_log_dir: PathBuf,
    pub job_worker_concurrency: usize,
    pub encryption_key: EncryptionKey,
    pub github: GitHubOAuthConfig,
    pub linuxdo: Option<LinuxDoOAuthConfig>,
    pub ai: Option<AiConfig>,
    pub ai_max_concurrency: usize,
    pub ai_daily_at_local: Option<chrono::NaiveTime>,
    pub app_default_time_zone: String,
}

#[derive(Clone)]
pub struct GitHubOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: Url,
}

#[derive(Clone)]
pub struct LinuxDoOAuthConfig {
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

impl fmt::Debug for LinuxDoOAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinuxDoOAuthConfig")
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
            .field("session_cookie_name", &self.session_cookie_name)
            .field("database_url", &self.database_url)
            .field("static_dir", &self.static_dir)
            .field("task_log_dir", &self.task_log_dir)
            .field("job_worker_concurrency", &self.job_worker_concurrency)
            .field("github", &self.github)
            .field("linuxdo", &self.linuxdo)
            .field("ai", &self.ai)
            .field("ai_max_concurrency", &self.ai_max_concurrency)
            .field("ai_daily_at_local", &self.ai_daily_at_local)
            .field("app_default_time_zone", &self.app_default_time_zone)
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
        let session_cookie_name = env::var("OCTORILL_SESSION_COOKIE_NAME")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./.data/octo-rill.db".to_owned());

        let task_log_dir = env::var("OCTORILL_TASK_LOG_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|candidate| !candidate.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from(".data/task-logs"));

        let job_worker_concurrency =
            parse_positive_usize_env("OCTORILL_TASK_WORKERS", false)?.unwrap_or(4);

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

        let linuxdo = {
            let client_id = env::var("LINUXDO_CLIENT_ID")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
            let client_secret = env::var("LINUXDO_CLIENT_SECRET")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
            let redirect_url = env::var("LINUXDO_OAUTH_REDIRECT_URL")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());

            match (client_id, client_secret, redirect_url) {
                (None, None, None) => None,
                (Some(client_id), Some(client_secret), Some(redirect_url)) => {
                    Some(LinuxDoOAuthConfig {
                        client_id,
                        client_secret,
                        redirect_url: Url::parse(&redirect_url)
                            .context("invalid LINUXDO_OAUTH_REDIRECT_URL")?,
                    })
                }
                _ => {
                    anyhow::bail!(
                        "LINUXDO_CLIENT_ID, LINUXDO_CLIENT_SECRET, and LINUXDO_OAUTH_REDIRECT_URL must be set together"
                    )
                }
            }
        };

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

        let ai_max_concurrency = parse_bounded_positive_usize_env(
            "AI_MAX_CONCURRENCY",
            true,
            tokio::sync::Semaphore::MAX_PERMITS,
        )?
        .unwrap_or(1);

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

        let legacy_runtime_time_zone = iana_time_zone::get_timezone().ok();
        let app_default_time_zone = resolve_app_default_time_zone(
            env::var("APP_DEFAULT_TIME_ZONE").ok(),
            legacy_runtime_time_zone.as_deref(),
        )?;

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
            session_cookie_name,
            database_url,
            static_dir,
            task_log_dir,
            job_worker_concurrency,
            encryption_key,
            github: GitHubOAuthConfig {
                client_id: github_client_id,
                client_secret: github_client_secret,
                redirect_url: github_redirect_url,
            },
            linuxdo,
            ai,
            ai_max_concurrency,
            ai_daily_at_local,
            app_default_time_zone,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn set_required_env() {
        unsafe {
            env::set_var(
                "OCTORILL_ENCRYPTION_KEY_BASE64",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            );
            env::set_var("GITHUB_CLIENT_ID", "test-client-id");
            env::set_var("GITHUB_CLIENT_SECRET", "test-client-secret");
            env::set_var(
                "GITHUB_OAUTH_REDIRECT_URL",
                "http://127.0.0.1:58090/auth/callback",
            );
            env::remove_var("AI_API_KEY");
            env::remove_var("AI_MAX_CONCURRENCY");
            env::remove_var("APP_DEFAULT_TIME_ZONE");
            env::remove_var("OCTORILL_TASK_WORKERS");
            env::remove_var("OCTORILL_SESSION_COOKIE_NAME");
            env::remove_var("LINUXDO_CLIENT_ID");
            env::remove_var("LINUXDO_CLIENT_SECRET");
            env::remove_var("LINUXDO_OAUTH_REDIRECT_URL");
        }
    }

    #[test]
    fn from_env_defaults_ai_max_concurrency_to_one() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();

        let config = AppConfig::from_env().expect("build config");

        assert_eq!(config.ai_max_concurrency, 1);
    }

    #[test]
    fn from_env_treats_blank_ai_max_concurrency_as_default() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        unsafe {
            env::set_var("AI_MAX_CONCURRENCY", "   ");
        }

        let config = AppConfig::from_env().expect("blank concurrency should fall back");

        assert_eq!(config.ai_max_concurrency, 1);
    }

    #[test]
    fn from_env_rejects_zero_ai_max_concurrency() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        unsafe {
            env::set_var("AI_MAX_CONCURRENCY", "0");
        }

        let err = AppConfig::from_env().expect_err("zero concurrency should fail");

        assert!(
            err.to_string()
                .contains("invalid AI_MAX_CONCURRENCY (expected positive integer)"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn from_env_rejects_non_numeric_ai_max_concurrency() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        unsafe {
            env::set_var("AI_MAX_CONCURRENCY", "abc");
        }

        let err = AppConfig::from_env().expect_err("non-numeric concurrency should fail");

        assert!(
            err.to_string()
                .contains("invalid AI_MAX_CONCURRENCY (expected positive integer)"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn from_env_rejects_ai_max_concurrency_above_semaphore_limit() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        let overflow = tokio::sync::Semaphore::MAX_PERMITS + 1;
        unsafe {
            env::set_var("AI_MAX_CONCURRENCY", overflow.to_string());
        }

        let err = AppConfig::from_env().expect_err("out-of-range concurrency should fail");

        assert!(
            err.to_string().contains(&format!(
                "invalid AI_MAX_CONCURRENCY (expected positive integer <= {})",
                tokio::sync::Semaphore::MAX_PERMITS
            )),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn from_env_rejects_blank_task_worker_concurrency() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        unsafe {
            env::set_var("OCTORILL_TASK_WORKERS", "   ");
        }

        let err = AppConfig::from_env().expect_err("blank task worker concurrency should fail");

        assert!(
            err.to_string()
                .contains("invalid OCTORILL_TASK_WORKERS (expected positive integer)"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn from_env_rejects_non_hour_aligned_default_time_zone() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        unsafe {
            env::set_var("APP_DEFAULT_TIME_ZONE", "Asia/Kolkata");
        }

        let err = AppConfig::from_env().expect_err("non-hour-aligned default timezone should fail");

        assert!(
            err.to_string().contains(
                "invalid APP_DEFAULT_TIME_ZONE (expected whole-hour IANA time zone year-round)"
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn from_env_defaults_session_cookie_name_to_none() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();

        let config = AppConfig::from_env().expect("build config");

        assert_eq!(config.session_cookie_name, None);
    }

    #[test]
    fn from_env_trims_custom_session_cookie_name() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        unsafe {
            env::set_var("OCTORILL_SESSION_COOKIE_NAME", "  octo_rill_sid_prod  ");
        }

        let config = AppConfig::from_env().expect("build config");

        assert_eq!(
            config.session_cookie_name.as_deref(),
            Some("octo_rill_sid_prod")
        );
    }

    #[test]
    fn from_env_accepts_linuxdo_oauth_when_all_vars_exist() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        unsafe {
            env::set_var("LINUXDO_CLIENT_ID", "linuxdo-client-id");
            env::set_var("LINUXDO_CLIENT_SECRET", "linuxdo-client-secret");
            env::set_var(
                "LINUXDO_OAUTH_REDIRECT_URL",
                "http://127.0.0.1:58090/auth/linuxdo/callback",
            );
        }

        let config = AppConfig::from_env().expect("build config");

        assert_eq!(
            config
                .linuxdo
                .as_ref()
                .map(|entry| entry.client_id.as_str()),
            Some("linuxdo-client-id")
        );
    }

    #[test]
    fn from_env_rejects_partial_linuxdo_oauth_config() {
        let _guard = env_lock().lock().expect("lock env");
        set_required_env();
        unsafe {
            env::set_var("LINUXDO_CLIENT_ID", "linuxdo-client-id");
        }

        let err = AppConfig::from_env().expect_err("partial linuxdo oauth config should fail");

        assert!(
            err.to_string().contains(
                "LINUXDO_CLIENT_ID, LINUXDO_CLIENT_SECRET, and LINUXDO_OAUTH_REDIRECT_URL must be set together"
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn resolve_app_default_time_zone_prefers_legacy_runtime_when_env_unset() {
        let resolved = resolve_app_default_time_zone(None, Some("America/New_York"))
            .expect("resolve legacy runtime time zone");

        assert_eq!(resolved, "America/New_York");
    }

    #[test]
    fn resolve_app_default_time_zone_falls_back_when_legacy_runtime_is_unsupported() {
        let resolved = resolve_app_default_time_zone(None, Some("Asia/Kolkata"))
            .expect("fallback for unsupported runtime time zone");

        assert_eq!(resolved, crate::briefs::DEFAULT_DAILY_BRIEF_TIME_ZONE);
    }
}
