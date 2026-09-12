use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use tower_sessions::Session;

use crate::{
    api,
    error::ApiError,
    jobs::{self, EnqueuedTask, NewTask},
    state::AppState,
    sync,
};

const OP_REGISTER: &str = "register";
const OP_CHECK: &str = "check";
const OP_PAUSE: &str = "pause";
const OP_DELETE: &str = "delete";
const OP_RECONCILE: &str = "reconcile";
const DESIRED_ENABLED: &str = "enabled";
const DESIRED_PAUSED: &str = "paused";
const DESIRED_DELETED: &str = "deleted";
const STATUS_REGISTERED: &str = "registered";
const STATUS_MISSING: &str = "missing";
const STATUS_PERMISSION_PAUSED: &str = "permission_paused";
const STATUS_ERROR: &str = "error";
const STATUS_CONFLICT: &str = "conflict";
const DELIVERY_RETENTION_DAYS: i64 = 30;

type HmacSha256 = Hmac<Sha256>;

pub(crate) fn user_operation_lock(user_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("webhook operation lock poisoned")
        .entry(user_id.to_owned())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

#[derive(Debug, sqlx::FromRow)]
struct UserConfigRow {
    include_own_releases: i64,
    webhook_push_enabled: i64,
    webhook_push_desired_state: String,
    webhook_push_last_completed_check_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct PatRow {
    token_ciphertext: Vec<u8>,
    token_nonce: Vec<u8>,
    last_check_state: String,
    owner_github_user_id: Option<i64>,
    owner_login: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct TargetRepo {
    repo_id: i64,
    owner_github_user_id: Option<i64>,
    owner_login: String,
    repo_name: String,
    repo_full_name: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WebhookRepoStatus {
    repo_id: i64,
    owner_login: String,
    repo_name: String,
    repo_full_name: String,
    is_private: Option<bool>,
    hook_id: Option<i64>,
    status: String,
    error_kind: Option<String>,
    error_message: Option<String>,
    permission_paused: bool,
    last_checked_at: Option<String>,
    last_registered_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebhookPushSettingsResponse {
    desired_state: String,
    enabled: bool,
    include_own_releases: bool,
    callback_ready: bool,
    pat: PatStatus,
    summary: WebhookSummary,
    schedule: ScheduleStatus,
    operation: Option<WebhookOperationSnapshot>,
    last_completed_check_at: Option<String>,
    owner_groups: Vec<WebhookOwnerGroup>,
    repos: Vec<WebhookRepoStatus>,
}

#[derive(Debug, Serialize)]
struct WebhookOperationSnapshot {
    task_id: String,
    status: String,
    operation: String,
    available_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct WebhookOwnerGroup {
    owner_login: String,
    repo_count: usize,
    pending_count: usize,
    repos: Vec<WebhookRepoStatus>,
}

#[derive(Debug, Serialize)]
struct PatStatus {
    configured: bool,
    valid: bool,
    owner_login: Option<String>,
}

#[derive(Debug, Serialize, Default)]
struct WebhookSummary {
    total: usize,
    registered: usize,
    missing: usize,
    permission_paused: usize,
    errors: usize,
    removable: usize,
}

#[derive(Debug, Serialize)]
struct ScheduleStatus {
    audit_interval_days: i64,
    last_started_at: Option<String>,
    next_started_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchSettingsRequest {
    desired_state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReconcileRequest {
    repo_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TaskEnqueueResponse {
    task_id: String,
    status: String,
    reused: bool,
    operation: String,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeConfigPatch {
    audit_interval_days: i64,
}

#[derive(Debug, Serialize)]
pub struct RuntimeConfigResponse {
    audit_interval_days: i64,
    last_started_at: Option<String>,
    next_started_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReceiverQuery {
    key: String,
}

#[derive(Debug, Deserialize)]
struct ReleasePayload {
    action: Option<String>,
    release: Option<ReleasePayloadItem>,
    repository: Option<ReleasePayloadRepo>,
}

#[derive(Debug, Deserialize)]
struct ReleasePayloadItem {
    id: i64,
    draft: bool,
}

#[derive(Debug, Deserialize)]
struct ReleasePayloadRepo {
    id: i64,
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct GitHubUser {
    id: i64,
    login: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRepoIdentity {
    id: i64,
    full_name: String,
    owner: GitHubUser,
}

#[derive(Debug, Deserialize)]
struct GitHubHook {
    id: i64,
    active: bool,
    events: Vec<String>,
    config: GitHubHookConfig,
}

#[derive(Debug, Deserialize)]
struct GitHubHookConfig {
    url: Option<String>,
    content_type: Option<String>,
}

fn repo_identity_matches(
    identity: &GitHubRepoIdentity,
    expected_owner_github_user_id: i64,
    repo: &TargetRepo,
) -> bool {
    identity.id == repo.repo_id
        && identity.owner.id == expected_owner_github_user_id
        && identity
            .full_name
            .eq_ignore_ascii_case(&repo.repo_full_name)
}

#[derive(Debug, Serialize)]
struct HookRequest<'a> {
    name: &'static str,
    active: bool,
    events: [&'static str; 1],
    config: HookRequestConfig<'a>,
}

#[derive(Debug, Serialize)]
struct HookRequestConfig<'a> {
    url: &'a str,
    content_type: &'static str,
    secret: &'a str,
    insecure_ssl: &'static str,
}

#[derive(Debug)]
struct GitHubCallError {
    status: Option<StatusCode>,
    rate_limited: bool,
    retry_after: Option<Duration>,
    message: String,
}

impl std::fmt::Display for GitHubCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

fn callback_ready(state: &AppState) -> bool {
    state.config.public_base_url.scheme() == "https"
        && state
            .config
            .public_base_url
            .host_str()
            .is_some_and(|host| host != "localhost" && host != "127.0.0.1" && host != "::1")
}

async fn load_user_config(state: &AppState, user_id: &str) -> Result<UserConfigRow, ApiError> {
    sqlx::query_as::<_, UserConfigRow>(
        r#"
        SELECT include_own_releases, webhook_push_enabled,
               webhook_push_desired_state, webhook_push_last_completed_check_at
        FROM users WHERE id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)
}

async fn load_pat(state: &AppState, user_id: &str) -> Result<Option<(PatRow, String)>, ApiError> {
    let row = sqlx::query_as::<_, PatRow>(
        r#"
        SELECT token_ciphertext, token_nonce, last_check_state,
               owner_github_user_id, owner_login
        FROM reaction_pat_tokens WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    row.map(|row| {
        let token = state
            .encryption_key
            .decrypt_str(&row.token_ciphertext, &row.token_nonce)
            .map_err(|_| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "pat_invalid",
                    "GitHub PAT 无法解密，请重新保存 PAT。",
                )
            })?;
        Ok((row, token))
    })
    .transpose()
}

fn parse_scopes(headers: &HeaderMap) -> Vec<String> {
    headers
        .get("x-oauth-scopes")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
        .collect()
}

async fn validate_pat(
    state: &AppState,
    user_id: &str,
) -> Result<(String, i64, String, bool), ApiError> {
    let Some((pat, token)) = load_pat(state, user_id).await? else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_required",
            "请先在 GitHub PAT 设置中保存 classic PAT。",
        ));
    };
    if pat.last_check_state != "valid" {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_invalid",
            "当前 GitHub PAT 未通过校验，请重新校验并保存。",
        ));
    }
    if token.starts_with("github_pat_") {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "classic_pat_required",
            "Webhook 推送当前仅支持 classic PAT。",
        ));
    }
    let response = state
        .github_rest_http
        .get(
            state
                .github_rest_api_base
                .join("user")
                .map_err(ApiError::internal)?,
        )
        .bearer_auth(&token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(ApiError::internal)?;
    let status = response.status();
    let rate_limited = status == StatusCode::TOO_MANY_REQUESTS
        || response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value == "0")
        || response.headers().contains_key("retry-after");
    if status != StatusCode::OK {
        if rate_limited {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "github_rate_limited",
                "GitHub API 当前限流，请稍后重试。",
            ));
        }
        if status.is_server_error() {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "github_unavailable",
                "GitHub user API 暂时不可用，请稍后重试。",
            ));
        }
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_invalid",
            "GitHub PAT 已失效或无法访问 GitHub user API。",
        ));
    }
    let scopes = parse_scopes(response.headers());
    if !scopes
        .iter()
        .any(|scope| scope == "public_repo" || scope == "repo")
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_scope_missing",
            "classic PAT 需要 public_repo（仅公开仓库）或 repo（包含私有仓库）权限。",
        ));
    }
    let github_user = response
        .json::<GitHubUser>()
        .await
        .map_err(ApiError::internal)?;
    if pat.owner_github_user_id != Some(github_user.id) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_owner_mismatch",
            "PAT 所属 GitHub 账号未绑定到当前 OctoRill 账号。",
        ));
    }
    if pat.owner_login.as_deref() != Some(github_user.login.as_str()) {
        sqlx::query(
            "UPDATE reaction_pat_tokens SET owner_login = ?, updated_at = ? WHERE user_id = ?",
        )
        .bind(&github_user.login)
        .bind(Utc::now().to_rfc3339())
        .bind(user_id)
        .execute(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    }
    let allows_private = scopes.iter().any(|scope| scope == "repo");
    Ok((token, github_user.id, github_user.login, allows_private))
}

fn generate_secret() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

async fn ensure_secret_and_key(
    state: &AppState,
    user_id: &str,
) -> Result<(String, String), ApiError> {
    let (_sqlite_write, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "webhook_push_secret_seed")
        .await
        .map_err(ApiError::internal)?;
    let current = sqlx::query_as::<_, (Option<Vec<u8>>, Option<Vec<u8>>, Option<String>)>(
        r#"
        SELECT webhook_push_secret_ciphertext, webhook_push_secret_nonce,
               webhook_push_callback_key
        FROM users WHERE id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let (ciphertext, nonce, key) = if let (Some(ciphertext), Some(nonce), Some(key)) = current {
        (ciphertext, nonce, key)
    } else {
        let secret = generate_secret();
        let key = crate::local_id::generate_local_id();
        let encrypted = state
            .encryption_key
            .encrypt_str(&secret)
            .map_err(ApiError::internal)?;
        sqlx::query(
            r#"
            UPDATE users
            SET webhook_push_secret_ciphertext = ?, webhook_push_secret_nonce = ?,
                webhook_push_callback_key = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&encrypted.ciphertext)
        .bind(&encrypted.nonce)
        .bind(&key)
        .bind(Utc::now().to_rfc3339())
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        (encrypted.ciphertext, encrypted.nonce, key)
    };
    tx.commit().await.map_err(ApiError::internal)?;
    let secret = state
        .encryption_key
        .decrypt_str(&ciphertext, &nonce)
        .map_err(ApiError::internal)?;
    Ok((secret, key))
}

fn callback_url(state: &AppState, key: &str) -> Result<String, ApiError> {
    let mut url = state
        .config
        .public_base_url
        .join("/api/webhooks/github/releases")
        .map_err(ApiError::internal)?;
    url.query_pairs_mut().append_pair("key", key);
    Ok(url.to_string())
}

fn next_started_at(last: Option<&str>, days: i64) -> Option<String> {
    last.and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| (value.with_timezone(&Utc) + chrono::Duration::days(days)).to_rfc3339())
}

async fn runtime_config(state: &AppState) -> Result<RuntimeConfigResponse, ApiError> {
    let (days, last) = sqlx::query_as::<_, (i64, Option<String>)>(
        r#"
        SELECT webhook_push_audit_interval_days, webhook_push_audit_last_started_at
        FROM admin_runtime_settings WHERE id = 1
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(RuntimeConfigResponse {
        audit_interval_days: days,
        next_started_at: next_started_at(last.as_deref(), days),
        last_started_at: last,
    })
}

async fn list_repo_statuses(
    state: &AppState,
    user_id: &str,
    owner_login: Option<&str>,
    desired_state: &str,
) -> Result<Vec<WebhookRepoStatus>, ApiError> {
    let owner_login = owner_login.unwrap_or("");
    sqlx::query_as::<_, WebhookRepoStatus>(
        r#"
        SELECT * FROM (
        SELECT ob.repo_id,
               substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1) AS owner_login,
               substr(ob.repo_full_name, instr(ob.repo_full_name, '/') + 1) AS repo_name,
               ob.repo_full_name,
               CASE WHEN ob.is_private IS NULL THEN NULL ELSE ob.is_private != 0 END AS is_private,
               wr.hook_id,
               COALESCE(wr.status, CASE WHEN ? = 'enabled' THEN 'waiting_registration' ELSE 'not_configured' END) AS status,
               wr.error_kind, wr.error_message,
               COALESCE(wr.permission_paused, 0) != 0 AS permission_paused,
               wr.last_checked_at, wr.last_registered_at
        FROM owned_repo_star_baselines ob
        LEFT JOIN webhook_push_repos wr
          ON wr.user_id = ob.user_id AND wr.repo_id = ob.repo_id
        WHERE ob.user_id = ?
          AND lower(substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)) = lower(?)
        UNION ALL
        SELECT wr.repo_id, wr.owner_login, wr.repo_name, wr.repo_full_name,
               NULL AS is_private, wr.hook_id, wr.status,
               wr.error_kind, wr.error_message,
               wr.permission_paused != 0 AS permission_paused,
               wr.last_checked_at, wr.last_registered_at
        FROM webhook_push_repos wr
        WHERE wr.user_id = ?
          AND NOT EXISTS (
            SELECT 1 FROM owned_repo_star_baselines ob
            WHERE ob.user_id = wr.user_id AND ob.repo_id = wr.repo_id
              AND lower(substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)) = lower(?)
          )
        ) ORDER BY lower(repo_full_name)
        "#,
    )
    .bind(desired_state)
    .bind(user_id)
    .bind(owner_login)
    .bind(user_id)
    .bind(owner_login)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)
}

fn summarize(repos: &[WebhookRepoStatus]) -> WebhookSummary {
    WebhookSummary {
        total: repos.len(),
        registered: repos
            .iter()
            .filter(|repo| repo.status == STATUS_REGISTERED)
            .count(),
        missing: repos
            .iter()
            .filter(|repo| repo.status == STATUS_MISSING)
            .count(),
        permission_paused: repos.iter().filter(|repo| repo.permission_paused).count(),
        errors: repos
            .iter()
            .filter(|repo| repo.status == STATUS_ERROR || repo.status == STATUS_CONFLICT)
            .count(),
        removable: repos.iter().filter(|repo| repo.hook_id.is_some()).count(),
    }
}

fn group_by_owner(repos: &[WebhookRepoStatus]) -> Vec<WebhookOwnerGroup> {
    let mut groups = Vec::<WebhookOwnerGroup>::new();
    for repo in repos.iter().cloned() {
        let Some(group) = groups
            .iter_mut()
            .find(|group| group.owner_login.eq_ignore_ascii_case(&repo.owner_login))
        else {
            groups.push(WebhookOwnerGroup {
                owner_login: repo.owner_login.clone(),
                repo_count: 1,
                pending_count: usize::from(repo_needs_attention(&repo)),
                repos: vec![repo],
            });
            continue;
        };
        group.repo_count += 1;
        group.pending_count += usize::from(repo_needs_attention(&repo));
        group.repos.push(repo);
    }
    groups.sort_by_key(|group| group.owner_login.to_ascii_lowercase());
    groups
}

fn repo_needs_attention(repo: &WebhookRepoStatus) -> bool {
    repo.error_message.is_some()
        || matches!(
            repo.status.as_str(),
            STATUS_MISSING
                | STATUS_PERMISSION_PAUSED
                | STATUS_ERROR
                | STATUS_CONFLICT
                | "waiting_registration"
                | "registering"
                | "processing"
                | "delete_pending"
        )
}

async fn current_operation(
    state: &AppState,
    user_id: &str,
) -> Result<Option<WebhookOperationSnapshot>, ApiError> {
    let row = sqlx::query_as::<_, (String, String, String, Option<String>)>(
        r#"
        SELECT id, status, COALESCE(json_extract(payload_json, '$.operation'), 'reconcile'), available_at
        FROM job_tasks
        WHERE task_type = ? AND requested_by = ? AND status IN (?, ?)
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(jobs::TASK_WEBHOOK_PUSH_MANAGE)
    .bind(user_id)
    .bind(jobs::STATUS_QUEUED)
    .bind(jobs::STATUS_RUNNING)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if let Some((task_id, status, operation, available_at)) = row {
        return Ok(Some(WebhookOperationSnapshot {
            task_id,
            status,
            operation,
            available_at,
        }));
    }

    let audit_row = sqlx::query_as::<_, (String, String, Option<String>)>(
        r#"
        SELECT task.id, task.status, task.available_at
        FROM webhook_push_user_operation_leases lease
        JOIN job_tasks task ON task.id = lease.task_id
        WHERE lease.user_id = ?
          AND task.task_type = ?
          AND task.status IN (?, ?)
        ORDER BY task.created_at DESC, task.id DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(jobs::TASK_WEBHOOK_PUSH_AUDIT)
    .bind(jobs::STATUS_QUEUED)
    .bind(jobs::STATUS_RUNNING)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(
        audit_row.map(|(task_id, status, available_at)| WebhookOperationSnapshot {
            task_id,
            status,
            operation: OP_RECONCILE.to_owned(),
            available_at,
        }),
    )
}

pub async fn get_settings(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<WebhookPushSettingsResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let config = load_user_config(state.as_ref(), &user_id).await?;
    let pat = load_pat(state.as_ref(), &user_id).await?;
    let owner_login = pat.as_ref().and_then(|(row, _)| row.owner_login.as_deref());
    let mut repos = list_repo_statuses(
        state.as_ref(),
        &user_id,
        owner_login,
        &config.webhook_push_desired_state,
    )
    .await?;
    let schedule = runtime_config(state.as_ref()).await?;
    let operation = current_operation(state.as_ref(), &user_id).await?;
    if operation.is_some() {
        let progress_status = if config.webhook_push_desired_state == DESIRED_ENABLED {
            "registering"
        } else {
            "processing"
        };
        for repo in &mut repos {
            if !matches!(repo.status.as_str(), STATUS_ERROR | STATUS_CONFLICT) {
                repo.status = progress_status.to_owned();
            }
        }
    }
    let owner_groups = group_by_owner(&repos);
    Ok(Json(WebhookPushSettingsResponse {
        desired_state: config.webhook_push_desired_state.clone(),
        enabled: config.webhook_push_enabled != 0,
        include_own_releases: config.include_own_releases != 0,
        callback_ready: callback_ready(state.as_ref()),
        pat: PatStatus {
            configured: pat.is_some(),
            valid: pat
                .as_ref()
                .is_some_and(|(row, _)| row.last_check_state == "valid"),
            owner_login: owner_login.map(str::to_owned),
        },
        summary: summarize(&repos),
        schedule: ScheduleStatus {
            audit_interval_days: schedule.audit_interval_days,
            last_started_at: schedule.last_started_at,
            next_started_at: schedule.next_started_at,
        },
        operation,
        last_completed_check_at: config.webhook_push_last_completed_check_at,
        owner_groups,
        repos,
    }))
}

pub async fn patch_settings(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(request): Json<PatchSettingsRequest>,
) -> Result<Json<Value>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let operation_lock = user_operation_lock(&user_id);
    let _operation_guard = operation_lock.lock().await;
    let config = load_user_config(state.as_ref(), &user_id).await?;
    let desired_state = request
        .desired_state
        .ok_or_else(|| ApiError::bad_request("desired_state is required"))?;
    if !matches!(
        desired_state.as_str(),
        DESIRED_ENABLED | DESIRED_PAUSED | DESIRED_DELETED
    ) {
        return Err(ApiError::bad_request(
            "desired_state must be enabled, paused, or deleted",
        ));
    }
    if desired_state == DESIRED_ENABLED {
        if config.include_own_releases == 0 {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "my_releases_required",
                "请先开启“我的发布”，再开启“Webhook 推送”。",
            ));
        }
        if !callback_ready(state.as_ref()) {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "webhook_callback_unavailable",
                "服务尚未配置 GitHub 可访问的 HTTPS 公共地址，请联系管理员。",
            ));
        }
        validate_pat(state.as_ref(), &user_id).await?;
        ensure_secret_and_key(state.as_ref(), &user_id).await?;
    }
    ensure_no_inflight_operation(state.as_ref(), &user_id).await?;
    let now = Utc::now().to_rfc3339();
    state
        .sqlite_writer
        .write_foreground("webhook_push_settings", |_| async {
            let updated = sqlx::query(
                "UPDATE users SET webhook_push_desired_state = ?, webhook_push_enabled = ?, updated_at = ? WHERE id = ? AND webhook_push_desired_state = ? AND webhook_push_enabled = ?",
            )
                .bind(&desired_state)
                .bind(if desired_state == DESIRED_ENABLED { 1_i64 } else { 0_i64 })
                .bind(&now)
                .bind(&user_id)
                .bind(&config.webhook_push_desired_state)
                .bind(config.webhook_push_enabled)
                .execute(&state.pool)
                .await?;
            Ok::<_, anyhow::Error>(updated.rows_affected())
        })
        .await
        .map_err(ApiError::internal)
        .and_then(|rows| {
            if rows == 0 {
                Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "webhook_push_operation_in_progress",
                    "Webhook 推送目标刚刚发生变化，请刷新后重试。",
                ))
            } else {
                Ok(())
            }
        })?;
    let task = match enqueue_manage(
        state.as_ref(),
        &user_id,
        OP_RECONCILE,
        None,
        "desired_state",
    )
    .await
    {
        Ok(task) => task,
        Err(error) => {
            let rollback_now = Utc::now().to_rfc3339();
            let rollback = state
                .sqlite_writer
                .write_foreground("webhook_push_settings_rollback", |_| async {
                    sqlx::query(
                        r#"
                        UPDATE users
                        SET webhook_push_desired_state = ?,
                            webhook_push_enabled = ?,
                            updated_at = ?
                        WHERE id = ?
                          AND webhook_push_desired_state = ?
                          AND NOT EXISTS (
                            SELECT 1 FROM job_tasks
                            WHERE task_type = ?
                              AND requested_by = ?
                              AND status IN (?, ?)
                          )
                        "#,
                    )
                    .bind(&config.webhook_push_desired_state)
                    .bind(config.webhook_push_enabled)
                    .bind(&rollback_now)
                    .bind(&user_id)
                    .bind(&desired_state)
                    .bind(jobs::TASK_WEBHOOK_PUSH_MANAGE)
                    .bind(&user_id)
                    .bind(jobs::STATUS_QUEUED)
                    .bind(jobs::STATUS_RUNNING)
                    .execute(&state.pool)
                    .await?;
                    Ok::<_, anyhow::Error>(())
                })
                .await;
            if let Err(rollback_error) = rollback {
                tracing::error!(
                    user_id,
                    ?rollback_error,
                    "failed to roll back webhook push desired state after enqueue failure"
                );
            }
            return Err(error);
        }
    };
    Ok(Json(json!({
        "desired_state": desired_state,
        "enabled": desired_state == DESIRED_ENABLED,
        "task_id": task.task_id,
        "status": task.status,
        "operation": OP_RECONCILE,
        "reused": task.reused,
    })))
}

fn task_response(task: EnqueuedTask, operation: &str) -> TaskEnqueueResponse {
    TaskEnqueueResponse {
        task_id: task.task_id,
        status: task.status,
        reused: task.reused,
        operation: operation.to_owned(),
    }
}

pub(crate) async fn ensure_no_inflight_operation(
    state: &AppState,
    user_id: &str,
) -> Result<(), ApiError> {
    if let Some(task) =
        jobs::find_inflight_task_for_requester(state, jobs::TASK_WEBHOOK_PUSH_MANAGE, user_id)
            .await
            .map_err(ApiError::internal)?
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "webhook_push_operation_in_progress",
            format!(
                "Webhook 对齐操作 {} 尚未完成，请等待本轮结束。",
                task.task_id
            ),
        ));
    }
    let now = Utc::now().to_rfc3339();
    let leased_task = sqlx::query_scalar::<_, String>(
        r#"
        SELECT lease.task_id
        FROM webhook_push_user_operation_leases lease
        JOIN job_tasks task ON task.id = lease.task_id
        WHERE lease.user_id = ?
          AND lease.expires_at >= ?
          AND task.status IN (?, ?)
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(now)
    .bind(jobs::STATUS_QUEUED)
    .bind(jobs::STATUS_RUNNING)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if let Some(task_id) = leased_task {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "webhook_push_operation_in_progress",
            format!("Webhook 对齐操作 {} 尚未完成，请等待本轮结束。", task_id),
        ));
    }
    Ok(())
}

async fn enqueue_manage(
    state: &AppState,
    user_id: &str,
    operation: &str,
    repo_id: Option<i64>,
    source: &str,
) -> Result<EnqueuedTask, ApiError> {
    ensure_no_inflight_operation(state, user_id).await?;
    let result = jobs::enqueue_task(
        state,
        NewTask {
            task_type: jobs::TASK_WEBHOOK_PUSH_MANAGE.to_owned(),
            payload: json!({
                "user_id": user_id,
                "operation": operation,
                "repo_id": repo_id,
            }),
            source: source.to_owned(),
            requested_by: Some(user_id.to_owned()),
            parent_task_id: None,
        },
    )
    .await;
    match result {
        Ok(task) => Ok(task),
        Err(error) if jobs::is_unique_violation(&error) => Err(ApiError::new(
            StatusCode::CONFLICT,
            "webhook_push_operation_in_progress",
            "Webhook 对齐操作尚未完成，请等待本轮结束。",
        )),
        Err(error) => Err(ApiError::internal(error)),
    }
}

pub(crate) async fn enqueue_reconcile_for_user(
    state: &AppState,
    user_id: &str,
    source: &str,
) -> Result<EnqueuedTask, ApiError> {
    enqueue_manage(state, user_id, OP_RECONCILE, None, source).await
}

pub async fn reconcile(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(request): Json<ReconcileRequest>,
) -> Result<Json<TaskEnqueueResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let operation_lock = user_operation_lock(&user_id);
    let _operation_guard = operation_lock.lock().await;
    let config = load_user_config(state.as_ref(), &user_id).await?;
    if config.include_own_releases == 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "my_releases_disabled",
            "请先开启“我的发布”。",
        ));
    }
    if config.webhook_push_desired_state == DESIRED_ENABLED {
        validate_pat(state.as_ref(), &user_id).await?;
    }
    Ok(Json(task_response(
        enqueue_manage(
            state.as_ref(),
            &user_id,
            OP_RECONCILE,
            request.repo_id,
            "manual",
        )
        .await?,
        OP_RECONCILE,
    )))
}

async fn load_targets(
    state: &AppState,
    user_id: &str,
    owner_github_user_id: i64,
    owner_login: &str,
    repo_id: Option<i64>,
    skip_paused: bool,
    allows_private: bool,
) -> Result<Vec<TargetRepo>> {
    sqlx::query_as::<_, TargetRepo>(
        r#"
        SELECT ob.repo_id, ? AS owner_github_user_id,
               substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1) AS owner_login,
               substr(ob.repo_full_name, instr(ob.repo_full_name, '/') + 1) AS repo_name,
               ob.repo_full_name
        FROM owned_repo_star_baselines ob
        LEFT JOIN webhook_push_repos wr
          ON wr.user_id = ob.user_id AND wr.repo_id = ob.repo_id
        WHERE ob.user_id = ?
          AND lower(substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)) = lower(?)
          AND (? IS NULL OR ob.repo_id = ?)
          AND (? = 0 OR COALESCE(wr.permission_paused, 0) = 0)
          AND (? != 0 OR COALESCE(ob.is_private, 1) = 0)
        ORDER BY lower(ob.repo_full_name)
        "#,
    )
    .bind(owner_github_user_id)
    .bind(user_id)
    .bind(owner_login)
    .bind(repo_id)
    .bind(repo_id)
    .bind(if skip_paused { 1_i64 } else { 0_i64 })
    .bind(if allows_private { 1_i64 } else { 0_i64 })
    .fetch_all(&state.pool)
    .await
    .context("load webhook push targets")
}

fn github_url(state: &AppState, repo: &TargetRepo, suffix: &str) -> Result<url::Url> {
    state
        .github_rest_api_base
        .join(&format!(
            "repos/{}/{}/hooks{suffix}",
            repo.owner_login, repo.repo_name
        ))
        .context("build github webhook URL")
}

async fn resolve_delete_target(
    state: &AppState,
    token: &str,
    expected_owner_github_user_id: i64,
    repo: &TargetRepo,
) -> std::result::Result<TargetRepo, GitHubCallError> {
    let url = state
        .github_rest_api_base
        .join(&format!("repositories/{}", repo.repo_id))
        .map_err(|error| GitHubCallError {
            status: None,
            rate_limited: false,
            retry_after: None,
            message: error.to_string(),
        })?;
    let response = state
        .github_rest_http
        .get(url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|error| GitHubCallError {
            status: error.status(),
            rate_limited: false,
            retry_after: None,
            message: error.to_string(),
        })?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    let identity = response
        .json::<GitHubRepoIdentity>()
        .await
        .map_err(|error| GitHubCallError {
            status: None,
            rate_limited: false,
            retry_after: None,
            message: error.to_string(),
        })?;
    if !repo_identity_matches(&identity, expected_owner_github_user_id, repo) {
        return Err(GitHubCallError {
            status: Some(StatusCode::FORBIDDEN),
            rate_limited: false,
            retry_after: None,
            message: "GitHub repository identity does not match the managed repository".to_owned(),
        });
    }
    let (owner_login, repo_name) =
        identity
            .full_name
            .split_once('/')
            .ok_or_else(|| GitHubCallError {
                status: None,
                rate_limited: false,
                retry_after: None,
                message: "GitHub repository full_name is invalid".to_owned(),
            })?;
    Ok(TargetRepo {
        repo_id: repo.repo_id,
        owner_github_user_id: Some(expected_owner_github_user_id),
        owner_login: owner_login.to_owned(),
        repo_name: repo_name.to_owned(),
        repo_full_name: identity.full_name,
    })
}

async fn response_error(response: reqwest::Response) -> GitHubCallError {
    let status = response.status();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);
    let rate_limited = response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "0")
        || response.headers().contains_key("retry-after");
    let body = response.text().await.unwrap_or_default();
    GitHubCallError {
        status: Some(status),
        rate_limited,
        retry_after,
        message: format!(
            "GitHub webhook API returned {status}: {}",
            body.chars().take(240).collect::<String>()
        ),
    }
}

async fn list_hooks(
    state: &AppState,
    token: &str,
    repo: &TargetRepo,
) -> std::result::Result<Vec<GitHubHook>, GitHubCallError> {
    let mut url = github_url(state, repo, "").map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
        retry_after: None,
        message: err.to_string(),
    })?;
    let mut hooks = Vec::new();
    for page in 1_u32..=5 {
        url.query_pairs_mut()
            .clear()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
        let response = state
            .github_rest_http
            .get(url.clone())
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|err| GitHubCallError {
                status: err.status(),
                rate_limited: false,
                retry_after: None,
                message: err.to_string(),
            })?;
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }
        let page_hooks =
            response
                .json::<Vec<GitHubHook>>()
                .await
                .map_err(|err| GitHubCallError {
                    status: None,
                    rate_limited: false,
                    retry_after: None,
                    message: err.to_string(),
                })?;
        let is_last_page = page_hooks.len() < 100;
        hooks.extend(page_hooks);
        if is_last_page {
            return Ok(hooks);
        }
    }
    Err(GitHubCallError {
        status: None,
        rate_limited: false,
        retry_after: None,
        message: "GitHub hook list exceeds the supported 500-hook limit".to_owned(),
    })
}

async fn create_hook(
    state: &AppState,
    token: &str,
    repo: &TargetRepo,
    callback: &str,
    secret: &str,
) -> std::result::Result<GitHubHook, GitHubCallError> {
    let url = github_url(state, repo, "").map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
        retry_after: None,
        message: err.to_string(),
    })?;
    let response = state
        .github_rest_http
        .post(url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&HookRequest {
            name: "web",
            active: true,
            events: ["release"],
            config: HookRequestConfig {
                url: callback,
                content_type: "json",
                secret,
                insecure_ssl: "0",
            },
        })
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|err| GitHubCallError {
            status: err.status(),
            rate_limited: false,
            retry_after: None,
            message: err.to_string(),
        })?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response.json().await.map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
        retry_after: None,
        message: err.to_string(),
    })
}

async fn update_hook(
    state: &AppState,
    token: &str,
    repo: &TargetRepo,
    hook_id: i64,
    active: bool,
    callback: &str,
    secret: &str,
) -> std::result::Result<GitHubHook, GitHubCallError> {
    let url = github_url(state, repo, &format!("/{hook_id}")).map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
        retry_after: None,
        message: err.to_string(),
    })?;
    let response = state
        .github_rest_http
        .patch(url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&HookRequest {
            name: "web",
            active,
            events: ["release"],
            config: HookRequestConfig {
                url: callback,
                content_type: "json",
                secret,
                insecure_ssl: "0",
            },
        })
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|err| GitHubCallError {
            status: err.status(),
            rate_limited: false,
            retry_after: None,
            message: err.to_string(),
        })?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response.json().await.map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
        retry_after: None,
        message: err.to_string(),
    })
}

async fn delete_hook(
    state: &AppState,
    token: &str,
    repo: &TargetRepo,
    hook_id: i64,
) -> std::result::Result<(), GitHubCallError> {
    let url = github_url(state, repo, &format!("/{hook_id}")).map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
        retry_after: None,
        message: err.to_string(),
    })?;
    let response = state
        .github_rest_http
        .delete(url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|err| GitHubCallError {
            status: err.status(),
            rate_limited: false,
            retry_after: None,
            message: err.to_string(),
        })?;
    if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
        return Ok(());
    }
    Err(response_error(response).await)
}

fn is_permission_error(error: &GitHubCallError) -> bool {
    if error.rate_limited {
        return false;
    }
    let message = error.message.to_ascii_lowercase();
    if message.contains("rate limit") || message.contains("rate_limit") {
        return false;
    }
    matches!(
        error.status,
        Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND)
    )
}

fn is_retryable_error(error: &GitHubCallError) -> bool {
    error.rate_limited || error.status.is_none_or(|status| status.is_server_error())
}

fn retry_delay(attempt: u8, retry_after: Option<Duration>) -> chrono::Duration {
    let backoff = match attempt {
        0 => Duration::from_secs(60),
        1 => Duration::from_secs(5 * 60),
        _ => Duration::from_secs(15 * 60),
    };
    let delay = retry_after.unwrap_or(backoff).max(backoff);
    chrono::Duration::from_std(delay).unwrap_or_else(|_| chrono::Duration::minutes(15))
}

async fn pause_user_repos_for_permission_error(
    state: &AppState,
    user_id: &str,
    error_code: &str,
    error: &ApiError,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE webhook_push_repos
        SET status = 'permission_paused', permission_paused = 1,
            error_kind = ?, error_message = ?, updated_at = ?
        WHERE user_id = ?
        "#,
    )
    .bind(error_code)
    .bind(error.to_string())
    .bind(Utc::now().to_rfc3339())
    .bind(user_id)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn persist_repo_state(
    state: &AppState,
    user_id: &str,
    repo: &TargetRepo,
    callback: &str,
    update: (&str, Option<i64>, Option<&GitHubCallError>, bool),
) -> Result<()> {
    let (status, hook_id, error, clear_pause) = update;
    let now = Utc::now().to_rfc3339();
    let permission_paused = error.is_some_and(is_permission_error);
    sqlx::query(
        r#"
        INSERT INTO webhook_push_repos (
          user_id, repo_id, owner_github_user_id, owner_login, repo_name, repo_full_name,
          hook_id, callback_url, status, error_kind, error_message,
          permission_paused, last_checked_at, last_registered_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, repo_id) DO UPDATE SET
          owner_github_user_id = excluded.owner_github_user_id,
          owner_login = excluded.owner_login, repo_name = excluded.repo_name,
          repo_full_name = excluded.repo_full_name,
          hook_id = COALESCE(excluded.hook_id, webhook_push_repos.hook_id),
          callback_url = excluded.callback_url, status = excluded.status,
          error_kind = excluded.error_kind, error_message = excluded.error_message,
          permission_paused = CASE WHEN ? THEN 0 WHEN excluded.permission_paused != 0 THEN 1 ELSE webhook_push_repos.permission_paused END,
          last_checked_at = excluded.last_checked_at,
          last_registered_at = COALESCE(excluded.last_registered_at, webhook_push_repos.last_registered_at),
          updated_at = excluded.updated_at
        "#,
    )
    .bind(user_id).bind(repo.repo_id).bind(repo.owner_github_user_id).bind(&repo.owner_login).bind(&repo.repo_name).bind(&repo.repo_full_name)
    .bind(hook_id).bind(callback).bind(status)
    .bind(error.map(|err| if is_permission_error(err) { "permission" } else { "github_error" }))
    .bind(error.map(|err| err.message.as_str()))
    .bind(if permission_paused { 1_i64 } else { 0_i64 })
    .bind(&now)
    .bind((status == STATUS_REGISTERED).then_some(now.as_str()))
    .bind(&now)
    .bind(clear_pause)
    .execute(&state.pool).await.context("persist webhook repo state")?;
    Ok(())
}

struct RepoOperationResult {
    outcome: &'static str,
    retryable: bool,
    retry_after: Option<Duration>,
}

fn repo_operation_failed(error: &GitHubCallError) -> RepoOperationResult {
    RepoOperationResult {
        outcome: "failed",
        retryable: is_retryable_error(error),
        retry_after: error.retry_after,
    }
}

fn hook_matches(hook: &GitHubHook, hook_id: Option<i64>, callback: &str) -> bool {
    hook_id.is_none_or(|expected| hook.id == expected)
        && hook.config.url.as_deref() == Some(callback)
        && hook.events.iter().any(|event| event == "release")
}

async fn run_repo_operation(
    state: &AppState,
    user_id: &str,
    repo: &TargetRepo,
    operation: &str,
    token: &str,
    callback: &str,
    secret: &str,
) -> Result<RepoOperationResult> {
    let stored = sqlx::query_as::<_, (Option<i64>, String)>(
        "SELECT hook_id, callback_url FROM webhook_push_repos WHERE user_id = ? AND repo_id = ?",
    )
    .bind(user_id)
    .bind(repo.repo_id)
    .fetch_optional(&state.pool)
    .await?;
    let stored_hook_id = stored.as_ref().and_then(|row| row.0);
    let managed_callback = stored
        .as_ref()
        .map(|row| row.1.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(callback);

    let hooks = match list_hooks(state, token, repo).await {
        Ok(hooks) => hooks,
        Err(error) => {
            let status = if is_permission_error(&error) {
                STATUS_PERMISSION_PAUSED
            } else {
                STATUS_ERROR
            };
            persist_repo_state(
                state,
                user_id,
                repo,
                managed_callback,
                (status, stored_hook_id, Some(&error), false),
            )
            .await?;
            return Ok(repo_operation_failed(&error));
        }
    };

    if operation == OP_DELETE || operation == OP_PAUSE {
        let Some(hook_id) = stored_hook_id else {
            return Ok(RepoOperationResult {
                outcome: "skipped",
                retryable: false,
                retry_after: None,
            });
        };
        let Some(hook) = hooks
            .iter()
            .find(|hook| hook_matches(hook, Some(hook_id), managed_callback))
        else {
            let status = if hooks.iter().any(|hook| hook.id == hook_id) {
                STATUS_CONFLICT
            } else {
                STATUS_MISSING
            };
            sqlx::query(
                "UPDATE webhook_push_repos SET status = ?, error_kind = NULL, error_message = NULL, updated_at = ? WHERE user_id = ? AND repo_id = ?",
            )
            .bind(status)
            .bind(Utc::now().to_rfc3339())
            .bind(user_id)
            .bind(repo.repo_id)
            .execute(&state.pool)
            .await?;
            return Ok(RepoOperationResult {
                outcome: "skipped",
                retryable: false,
                retry_after: None,
            });
        };

        if operation == OP_PAUSE {
            match update_hook(state, token, repo, hook.id, false, managed_callback, secret).await {
                Ok(updated) => {
                    persist_repo_state(
                        state,
                        user_id,
                        repo,
                        managed_callback,
                        (STATUS_REGISTERED, Some(updated.id), None, false),
                    )
                    .await?;
                    return Ok(RepoOperationResult {
                        outcome: "paused",
                        retryable: false,
                        retry_after: None,
                    });
                }
                Err(error) => {
                    persist_repo_state(
                        state,
                        user_id,
                        repo,
                        managed_callback,
                        (
                            if is_permission_error(&error) {
                                STATUS_PERMISSION_PAUSED
                            } else {
                                STATUS_ERROR
                            },
                            Some(hook.id),
                            Some(&error),
                            false,
                        ),
                    )
                    .await?;
                    return Ok(repo_operation_failed(&error));
                }
            }
        }

        match delete_hook(state, token, repo, hook_id).await {
            Ok(()) => {
                sqlx::query("DELETE FROM webhook_push_repos WHERE user_id = ? AND repo_id = ?")
                    .bind(user_id)
                    .bind(repo.repo_id)
                    .execute(&state.pool)
                    .await?;
                return Ok(RepoOperationResult {
                    outcome: "deleted",
                    retryable: false,
                    retry_after: None,
                });
            }
            Err(error) => {
                persist_repo_state(
                    state,
                    user_id,
                    repo,
                    managed_callback,
                    (
                        if is_permission_error(&error) {
                            STATUS_PERMISSION_PAUSED
                        } else {
                            STATUS_ERROR
                        },
                        Some(hook_id),
                        Some(&error),
                        false,
                    ),
                )
                .await?;
                return Ok(repo_operation_failed(&error));
            }
        }
    }

    let matches = hooks
        .iter()
        .filter(|hook| hook_matches(hook, stored_hook_id, managed_callback))
        .collect::<Vec<_>>();
    if stored_hook_id.is_some()
        && matches.is_empty()
        && hooks
            .iter()
            .any(|hook| hook_matches(hook, None, managed_callback))
    {
        persist_repo_state(
            state,
            user_id,
            repo,
            managed_callback,
            (STATUS_CONFLICT, stored_hook_id, None, false),
        )
        .await?;
        return Ok(RepoOperationResult {
            outcome: "conflict",
            retryable: false,
            retry_after: None,
        });
    }
    if matches.len() > 1 {
        persist_repo_state(
            state,
            user_id,
            repo,
            managed_callback,
            (STATUS_CONFLICT, None, None, false),
        )
        .await?;
        return Ok(RepoOperationResult {
            outcome: "conflict",
            retryable: false,
            retry_after: None,
        });
    }
    if operation == OP_CHECK {
        if let Some(hook) = matches.first() {
            let healthy = hook.active
                && hook.config.content_type.as_deref() == Some("json")
                && hook.events.as_slice() == ["release"];
            persist_repo_state(
                state,
                user_id,
                repo,
                managed_callback,
                (
                    if healthy {
                        STATUS_REGISTERED
                    } else {
                        STATUS_ERROR
                    },
                    Some(hook.id),
                    None,
                    false,
                ),
            )
            .await?;
            return Ok(RepoOperationResult {
                outcome: if healthy { "registered" } else { "failed" },
                retryable: false,
                retry_after: None,
            });
        }
        persist_repo_state(
            state,
            user_id,
            repo,
            managed_callback,
            (STATUS_MISSING, None, None, false),
        )
        .await?;
        return Ok(RepoOperationResult {
            outcome: "missing",
            retryable: false,
            retry_after: None,
        });
    }
    let result = if let Some(hook) = matches.first() {
        update_hook(state, token, repo, hook.id, true, managed_callback, secret).await
    } else {
        create_hook(state, token, repo, callback, secret).await
    };
    match result {
        Ok(hook) => {
            persist_repo_state(
                state,
                user_id,
                repo,
                callback,
                (STATUS_REGISTERED, Some(hook.id), None, true),
            )
            .await?;
            Ok(RepoOperationResult {
                outcome: "registered",
                retryable: false,
                retry_after: None,
            })
        }
        Err(error) => {
            let status = if is_permission_error(&error) {
                STATUS_PERMISSION_PAUSED
            } else {
                STATUS_ERROR
            };
            persist_repo_state(
                state,
                user_id,
                repo,
                callback,
                (status, stored_hook_id, Some(&error), false),
            )
            .await?;
            Ok(repo_operation_failed(&error))
        }
    }
}

async fn try_acquire_user_operation_lease(
    state: &AppState,
    user_id: &str,
    task_id: &str,
) -> Result<bool> {
    let now = Utc::now();
    let expires_at = (now + chrono::Duration::minutes(10)).to_rfc3339();
    let result = sqlx::query(
        r#"
        INSERT INTO webhook_push_user_operation_leases (user_id, task_id, expires_at)
        VALUES (?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
          task_id = excluded.task_id,
          expires_at = excluded.expires_at
        WHERE webhook_push_user_operation_leases.expires_at < ?
        "#,
    )
    .bind(user_id)
    .bind(task_id)
    .bind(&expires_at)
    .bind(now.to_rfc3339())
    .execute(&state.pool)
    .await?;
    Ok(result.rows_affected() != 0)
}

async fn renew_user_operation_lease(state: &AppState, user_id: &str, task_id: &str) -> Result<()> {
    let result = sqlx::query(
        "UPDATE webhook_push_user_operation_leases SET expires_at = ? WHERE user_id = ? AND task_id = ?",
    )
    .bind((Utc::now() + chrono::Duration::minutes(10)).to_rfc3339())
    .bind(user_id)
    .bind(task_id)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(anyhow!("webhook operation lease was lost"));
    }
    Ok(())
}

async fn release_user_operation_lease(state: &AppState, user_id: &str, task_id: &str) {
    let _ = sqlx::query(
        "DELETE FROM webhook_push_user_operation_leases WHERE user_id = ? AND task_id = ?",
    )
    .bind(user_id)
    .bind(task_id)
    .execute(&state.pool)
    .await;
}

async fn execute_for_user(
    state: &AppState,
    task_id: &str,
    user_id: &str,
    operation: &str,
    repo_id: Option<i64>,
    scheduled: bool,
    retry_count: u8,
) -> Result<Value> {
    while !try_acquire_user_operation_lease(state, user_id, task_id).await? {
        if jobs::is_task_cancel_requested(state, task_id).await? {
            let _ = jobs::reschedule_task(
                state,
                task_id,
                Utc::now(),
                json!({"operation": operation, "reason": "cancel_requested"}),
            )
            .await?;
            return Ok(json!({"operation": operation, "rescheduled": true}));
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let operation_lock = user_operation_lock(user_id);
    let _operation_guard = operation_lock.lock().await;
    let result = execute_for_user_locked(
        state,
        task_id,
        user_id,
        operation,
        repo_id,
        scheduled,
        retry_count,
    )
    .await;
    release_user_operation_lease(state, user_id, task_id).await;
    result
}

async fn execute_for_user_locked(
    state: &AppState,
    task_id: &str,
    user_id: &str,
    operation: &str,
    repo_id: Option<i64>,
    scheduled: bool,
    retry_count: u8,
) -> Result<Value> {
    let config = load_user_config(state, user_id)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;
    let effective_operation = if operation == OP_RECONCILE {
        match config.webhook_push_desired_state.as_str() {
            DESIRED_ENABLED => OP_REGISTER,
            DESIRED_PAUSED => OP_PAUSE,
            DESIRED_DELETED => OP_DELETE,
            _ => return Err(anyhow!("invalid persisted webhook desired state")),
        }
    } else {
        operation
    };
    if config.include_own_releases == 0 || config.webhook_push_desired_state != DESIRED_ENABLED {
        if !matches!(effective_operation, OP_DELETE | OP_PAUSE) {
            return Ok(json!({"skipped": true, "reason": "disabled"}));
        }
    } else if effective_operation == OP_DELETE {
        return Ok(json!({"skipped": true, "reason": "re_enabled"}));
    }
    let (token, owner_github_user_id, owner_login, allows_private, secret, callback) = {
        let (token, owner_github_user_id, owner_login, allows_private) =
            match validate_pat(state, user_id).await {
                Ok(value) => value,
                Err(error)
                    if scheduled
                        && matches!(
                            error.code(),
                            "pat_invalid"
                                | "pat_scope_missing"
                                | "pat_owner_mismatch"
                                | "classic_pat_required"
                        ) =>
                {
                    pause_user_repos_for_permission_error(state, user_id, error.code(), &error)
                        .await?;
                    return Ok(json!({
                        "skipped": true,
                        "reason": "permission_paused",
                        "error_code": error.code()
                    }));
                }
                Err(error) => return Err(anyhow!(error.to_string())),
            };
        let (secret, key) = ensure_secret_and_key(state, user_id)
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        let callback = callback_url(state, &key).map_err(|err| anyhow!(err.to_string()))?;
        (
            token,
            owner_github_user_id,
            owner_login,
            allows_private,
            secret,
            callback,
        )
    };
    if effective_operation == OP_REGISTER
        && let Err(error) = sync::refresh_owned_repo_release_visibility(state, user_id).await
    {
        tracing::warn!(
            user_id,
            ?error,
            "webhook push: owned repo refresh failed; using cached baseline"
        );
    }
    let targets = if matches!(effective_operation, OP_DELETE | OP_PAUSE) {
        sqlx::query_as::<_, TargetRepo>(
            r#"
            SELECT repo_id, owner_github_user_id, owner_login, repo_name, repo_full_name
            FROM webhook_push_repos
            WHERE user_id = ?
              AND hook_id IS NOT NULL
              AND (? IS NULL OR repo_id = ?)
            ORDER BY lower(repo_full_name)
            "#,
        )
        .bind(user_id)
        .bind(repo_id)
        .bind(repo_id)
        .fetch_all(&state.pool)
        .await?
    } else {
        load_targets(
            state,
            user_id,
            owner_github_user_id,
            &owner_login,
            repo_id,
            scheduled,
            allows_private,
        )
        .await?
    };
    let mut counts = HashMap::<&str, usize>::new();
    let mut retry_after: Option<Duration> = None;
    let mut retryable_failure = false;
    for (index, repo) in targets.iter().enumerate() {
        if jobs::is_task_cancel_requested(state, task_id).await? {
            let _ = jobs::reschedule_task(
                state,
                task_id,
                Utc::now(),
                json!({"operation": effective_operation, "reason": "cancel_requested"}),
            )
            .await?;
            return Ok(json!({"operation": effective_operation, "rescheduled": true}));
        }
        renew_user_operation_lease(state, user_id, task_id).await?;
        let result = match resolve_delete_target(state, &token, owner_github_user_id, repo).await {
            Ok(resolved_repo) => {
                run_repo_operation(
                    state,
                    user_id,
                    &resolved_repo,
                    effective_operation,
                    &token,
                    &callback,
                    &secret,
                )
                .await?
            }
            Err(error) => {
                let permission_paused = is_permission_error(&error);
                if matches!(effective_operation, OP_DELETE | OP_PAUSE) {
                    sqlx::query(
                        "UPDATE webhook_push_repos SET status = ?, error_kind = ?, error_message = ?, permission_paused = ?, updated_at = ? WHERE user_id = ? AND repo_id = ?",
                    )
                    .bind(if permission_paused { STATUS_PERMISSION_PAUSED } else { STATUS_ERROR })
                    .bind(if permission_paused { "permission" } else { "github_error" })
                    .bind(&error.message)
                    .bind(if permission_paused { 1_i64 } else { 0_i64 })
                    .bind(Utc::now().to_rfc3339())
                    .bind(user_id)
                    .bind(repo.repo_id)
                    .execute(&state.pool)
                    .await?;
                } else {
                    persist_repo_state(
                        state,
                        user_id,
                        repo,
                        &callback,
                        (
                            if permission_paused {
                                STATUS_PERMISSION_PAUSED
                            } else {
                                STATUS_ERROR
                            },
                            None,
                            Some(&error),
                            false,
                        ),
                    )
                    .await?;
                }
                repo_operation_failed(&error)
            }
        };
        retryable_failure |= result.retryable;
        retry_after = match (retry_after, result.retry_after) {
            (Some(current), Some(candidate)) => Some(current.max(candidate)),
            (current, candidate) => current.or(candidate),
        };
        *counts.entry(result.outcome).or_default() += 1;
        jobs::append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "stage": effective_operation, "repo_id": repo.repo_id, "repo": repo.repo_full_name,
                "index": index + 1, "total": targets.len(), "result": result.outcome,
            }),
        )
        .await?;
    }
    if jobs::is_task_cancel_requested(state, task_id).await? {
        let _ = jobs::reschedule_task(
            state,
            task_id,
            Utc::now(),
            json!({"operation": effective_operation, "reason": "cancel_requested"}),
        )
        .await?;
        return Ok(json!({"operation": effective_operation, "rescheduled": true}));
    }
    if retryable_failure && retry_count < 3 {
        let delay = retry_delay(retry_count, retry_after);
        sqlx::query(
            "UPDATE job_tasks SET payload_json = json_set(payload_json, '$.retry_count', ?) WHERE id = ?",
        )
        .bind(i64::from(retry_count + 1))
        .bind(task_id)
        .execute(&state.pool)
        .await?;
        let _ = jobs::reschedule_task(
            state,
            task_id,
            Utc::now() + delay,
            json!({
                "operation": effective_operation,
                "retry_count": retry_count + 1,
                "retry_after_seconds": delay.num_seconds(),
            }),
        )
        .await?;
        return Ok(json!({
            "operation": effective_operation,
            "total": targets.len(),
            "counts": counts,
            "rescheduled": true,
            "retry_count": retry_count + 1,
        }));
    }
    if repo_id.is_none() && effective_operation == OP_REGISTER {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE users SET webhook_push_last_completed_check_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    }
    Ok(json!({"operation": effective_operation, "total": targets.len(), "counts": counts}))
}

pub async fn execute_manage_task(
    state: &AppState,
    task_id: &str,
    payload: &Value,
) -> Result<Value> {
    let user_id = payload
        .get("user_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("webhook push user_id missing"))?;
    let operation = payload
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("webhook push operation missing"))?;
    let repo_id = payload.get("repo_id").and_then(Value::as_i64);
    let retry_count = payload
        .get("retry_count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(3) as u8;
    execute_for_user(
        state,
        task_id,
        user_id,
        operation,
        repo_id,
        false,
        retry_count,
    )
    .await
}

pub async fn execute_audit_task(state: &AppState, task_id: &str) -> Result<Value> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE admin_runtime_settings SET webhook_push_audit_last_started_at = ?, updated_at = ? WHERE id = 1")
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await?;
    let users = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM users
        WHERE (
            include_own_releases != 0
            AND webhook_push_desired_state = 'enabled'
          )
          OR (
            webhook_push_desired_state IN ('paused', 'deleted')
            AND EXISTS (
              SELECT 1
              FROM webhook_push_repos
              WHERE webhook_push_repos.user_id = users.id
                AND webhook_push_repos.hook_id IS NOT NULL
            )
          )
        ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for user_id in &users {
        match execute_for_user(state, task_id, user_id, OP_RECONCILE, None, true, 0).await {
            Ok(_) => succeeded += 1,
            Err(error) => {
                failed += 1;
                tracing::warn!(user_id, ?error, "webhook push audit user failed");
            }
        }
    }
    let cutoff = (Utc::now() - chrono::Duration::days(DELIVERY_RETENTION_DAYS)).to_rfc3339();
    let _ = sqlx::query("DELETE FROM webhook_push_deliveries WHERE received_at < ?")
        .bind(cutoff)
        .execute(&state.pool)
        .await;
    Ok(json!({"users": users.len(), "succeeded": succeeded, "failed": failed}))
}

pub async fn enqueue_audit_if_due(state: &AppState, now: DateTime<Utc>) -> Result<Option<String>> {
    let config = runtime_config(state)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;
    let due = config
        .last_started_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|last| {
            now >= last.with_timezone(&Utc) + chrono::Duration::days(config.audit_interval_days)
        });
    if !due {
        return Ok(None);
    }
    let task = jobs::enqueue_singleton_task_by_type(
        state,
        NewTask {
            task_type: jobs::TASK_WEBHOOK_PUSH_AUDIT.to_owned(),
            payload: json!({"trigger": "schedule"}),
            source: "scheduler".to_owned(),
            requested_by: None,
            parent_task_id: None,
        },
    )
    .await?;
    Ok(Some(task.task_id))
}

pub async fn admin_get_runtime_config(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<RuntimeConfigResponse>, ApiError> {
    api::require_admin_user_id(state.as_ref(), &session).await?;
    Ok(Json(runtime_config(state.as_ref()).await?))
}

pub async fn admin_patch_runtime_config(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(request): Json<RuntimeConfigPatch>,
) -> Result<Json<RuntimeConfigResponse>, ApiError> {
    api::require_admin_user_id(state.as_ref(), &session).await?;
    if !(1..=30).contains(&request.audit_interval_days) {
        return Err(ApiError::bad_request(
            "audit_interval_days must be between 1 and 30",
        ));
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE admin_runtime_settings SET webhook_push_audit_interval_days = ?, updated_at = ? WHERE id = 1")
        .bind(request.audit_interval_days).bind(now).execute(&state.pool).await.map_err(ApiError::internal)?;
    Ok(Json(runtime_config(state.as_ref()).await?))
}

fn verify_signature(secret: &str, signature: &str, body: &[u8]) -> bool {
    let Some(hex_signature) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = decode_hex(hex_signature) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn has_valid_signature_format(signature: &str) -> bool {
    signature.strip_prefix("sha256=").is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn decode_hex(value: &str) -> std::result::Result<Vec<u8>, ()> {
    if !value.len().is_multiple_of(2) {
        return Err(());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(|_| ()))
        .collect()
}

fn should_enqueue_release(
    include_own_releases: bool,
    enabled: bool,
    event: &str,
    payload: &ReleasePayload,
    expected_repo_id: i64,
    expected_repo_full_name: &str,
    repo_status: &str,
) -> bool {
    include_own_releases
        && enabled
        && event == "release"
        && payload.action.as_deref() == Some("published")
        && payload.release.as_ref().is_some_and(|item| !item.draft)
        && payload.repository.as_ref().is_some_and(|item| {
            item.id == expected_repo_id
                && item.full_name.eq_ignore_ascii_case(expected_repo_full_name)
        })
        && repo_status == STATUS_REGISTERED
}

pub async fn receive(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReceiverQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let delivery = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad_request("X-GitHub-Delivery is required"))?;
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad_request("X-GitHub-Event is required"))?;
    let hook_id = headers
        .get("x-github-hook-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| ApiError::bad_request("X-GitHub-Hook-ID is required"))?;
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_signature",
                "X-Hub-Signature-256 is required",
            )
        })?;
    if !has_valid_signature_format(signature) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "X-Hub-Signature-256 is malformed",
        ));
    }
    let row = sqlx::query_as::<_, (String, i64, String, Vec<u8>, Vec<u8>, i64, String, String)>(
        r#"
        SELECT u.id, u.include_own_releases, u.webhook_push_desired_state,
               u.webhook_push_secret_ciphertext, u.webhook_push_secret_nonce,
               wr.repo_id, wr.repo_full_name, wr.status
        FROM users u
        JOIN webhook_push_repos wr ON wr.user_id = u.id AND wr.hook_id = ?
        WHERE u.webhook_push_callback_key = ?
          AND u.webhook_push_secret_ciphertext IS NOT NULL
          AND u.webhook_push_secret_nonce IS NOT NULL
          AND EXISTS (
            SELECT 1 FROM reaction_pat_tokens pat
            WHERE pat.user_id = u.id
              AND pat.owner_github_user_id = wr.owner_github_user_id
          )
        "#,
    )
    .bind(hook_id)
    .bind(&query.key)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some(row) = row else {
        return Ok(Json(
            json!({"accepted": true, "queued": false, "reason": "unknown_hook"}),
        ));
    };
    let secret = state
        .encryption_key
        .decrypt_str(&row.3, &row.4)
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_signature",
                "Webhook secret is invalid",
            )
        })?;
    if !verify_signature(&secret, signature, &body) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "Webhook signature is invalid",
        ));
    }
    if event == "ping" {
        return Ok(Json(
            json!({"accepted": true, "queued": false, "reason": "ping"}),
        ));
    }
    let payload = serde_json::from_slice::<ReleasePayload>(&body)
        .map_err(|_| ApiError::bad_request("invalid GitHub webhook payload"))?;
    let action = payload.action.as_deref().unwrap_or("");
    let release = payload.release.as_ref();
    let repo = payload.repository.as_ref();
    let should_queue = should_enqueue_release(
        row.1 != 0,
        row.2 == DESIRED_ENABLED,
        event,
        &payload,
        row.5,
        &row.6,
        &row.7,
    );
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT OR IGNORE INTO webhook_push_deliveries (delivery_id, hook_id, repo_id, event, action, received_at) VALUES (?, ?, ?, ?, ?, ?)",
    ).bind(delivery).bind(hook_id).bind(repo.map(|item| item.id)).bind(event).bind(action).bind(&now)
        .execute(&state.pool).await.map_err(ApiError::internal)?;
    let claimed = sqlx::query(
        "UPDATE webhook_push_deliveries SET processing_state = 'processing', processing_started_at = ? WHERE delivery_id = ? AND (processing_state = 'pending' OR (processing_state = 'processing' AND processing_started_at < ?))",
    )
    .bind(&now)
    .bind(delivery)
    .bind((Utc::now() - chrono::Duration::minutes(5)).to_rfc3339())
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if claimed.rows_affected() == 0 {
        return Ok(Json(
            json!({"accepted": true, "queued": false, "reason": "duplicate"}),
        ));
    }
    if !should_queue {
        sqlx::query(
            "UPDATE webhook_push_deliveries SET processing_state = 'ignored', processing_started_at = NULL WHERE delivery_id = ?",
        )
        .bind(delivery)
        .execute(&state.pool)
        .await
        .map_err(ApiError::internal)?;
        return Ok(Json(
            json!({"accepted": true, "queued": false, "reason": "ignored"}),
        ));
    }
    let repo = repo.expect("repo checked above");
    let reused_fresh = match sync::enqueue_user_repo_release_sync(
        state.as_ref(),
        &row.0,
        repo.id,
        &repo.full_name,
    )
    .await
    {
        Ok(reused_fresh) => reused_fresh,
        Err(error) => {
            let _ = sqlx::query(
                "UPDATE webhook_push_deliveries SET processing_state = 'pending', processing_started_at = NULL WHERE delivery_id = ? AND processing_state = 'processing'",
            )
            .bind(delivery)
            .execute(&state.pool)
            .await;
            return Err(ApiError::internal(error));
        }
    };
    sqlx::query("UPDATE webhook_push_deliveries SET queued_task_id = ?, processing_state = 'queued', processing_started_at = NULL WHERE delivery_id = ?")
        .bind(format!("repo-release:{}", repo.id))
        .bind(delivery)
        .execute(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({
        "accepted": true, "queued": !reused_fresh, "reason": if reused_fresh { "fresh_cache" } else { "release_sync_queued" },
        "release_id": release.map(|item| item.id),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_verification_accepts_known_digest() {
        assert!(verify_signature(
            "It's a Secret to Everybody",
            "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17",
            b"Hello, World!",
        ));
    }

    #[test]
    fn signature_verification_rejects_wrong_digest() {
        assert!(!verify_signature("secret", "sha256=00", b"payload"));
    }

    #[test]
    fn signature_format_requires_sha256_hex_digest() {
        assert!(has_valid_signature_format(
            "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17"
        ));
        for signature in ["garbage", "sha256=xyz", "sha256=00", "sha1=757107ea"] {
            assert!(!has_valid_signature_format(signature));
        }
    }

    fn release_payload(action: &str, repo_id: i64, draft: bool) -> ReleasePayload {
        ReleasePayload {
            action: Some(action.to_owned()),
            release: Some(ReleasePayloadItem { id: 7, draft }),
            repository: Some(ReleasePayloadRepo {
                id: repo_id,
                full_name: "owner/repo".to_owned(),
            }),
        }
    }

    #[test]
    fn release_delivery_only_queues_new_published_release_for_registered_hook_repo() {
        let payload = release_payload("published", 42, false);
        assert!(should_enqueue_release(
            true,
            true,
            "release",
            &payload,
            42,
            "owner/repo",
            STATUS_REGISTERED,
        ));
        assert!(!should_enqueue_release(
            true,
            true,
            "release",
            &payload,
            42,
            "other/repo",
            STATUS_REGISTERED,
        ));

        for candidate in [
            should_enqueue_release(
                false,
                true,
                "release",
                &payload,
                42,
                "owner/repo",
                STATUS_REGISTERED,
            ),
            should_enqueue_release(
                true,
                false,
                "release",
                &payload,
                42,
                "owner/repo",
                STATUS_REGISTERED,
            ),
            should_enqueue_release(
                true,
                true,
                "push",
                &payload,
                42,
                "owner/repo",
                STATUS_REGISTERED,
            ),
            should_enqueue_release(
                true,
                true,
                "release",
                &payload,
                99,
                "owner/repo",
                STATUS_REGISTERED,
            ),
            should_enqueue_release(
                true,
                true,
                "release",
                &payload,
                42,
                "owner/repo",
                STATUS_MISSING,
            ),
            should_enqueue_release(
                true,
                true,
                "release",
                &release_payload("edited", 42, false),
                42,
                "owner/repo",
                STATUS_REGISTERED,
            ),
            should_enqueue_release(
                true,
                true,
                "release",
                &release_payload("published", 42, true),
                42,
                "owner/repo",
                STATUS_REGISTERED,
            ),
        ] {
            assert!(!candidate);
        }
    }

    #[test]
    fn paused_target_is_healthy_but_receiver_does_not_enqueue() {
        let payload = release_payload("published", 42, false);
        assert!(!should_enqueue_release(
            true,
            false,
            "release",
            &payload,
            42,
            "owner/repo",
            STATUS_REGISTERED,
        ));
    }

    #[test]
    fn retry_delay_uses_three_backoff_slots_and_later_retry_after() {
        assert_eq!(retry_delay(0, None), chrono::Duration::minutes(1));
        assert_eq!(retry_delay(1, None), chrono::Duration::minutes(5));
        assert_eq!(retry_delay(2, None), chrono::Duration::minutes(15));
        assert_eq!(
            retry_delay(0, Some(Duration::from_secs(90))),
            chrono::Duration::seconds(90)
        );
    }

    #[test]
    fn managed_hook_identity_requires_id_callback_and_release_event() {
        let hook = GitHubHook {
            id: 7,
            active: true,
            events: vec!["release".to_owned()],
            config: GitHubHookConfig {
                url: Some("https://octo.example/hook".to_owned()),
                content_type: Some("json".to_owned()),
            },
        };
        assert!(hook_matches(&hook, Some(7), "https://octo.example/hook"));
        assert!(!hook_matches(&hook, Some(8), "https://octo.example/hook"));
        assert!(!hook_matches(&hook, Some(7), "https://other.example/hook"));
    }

    #[test]
    fn managed_repository_identity_requires_canonical_name() {
        let repo = TargetRepo {
            repo_id: 42,
            owner_github_user_id: Some(7),
            owner_login: "owner".to_owned(),
            repo_name: "repo".to_owned(),
            repo_full_name: "owner/repo".to_owned(),
        };
        let identity = GitHubRepoIdentity {
            id: 42,
            full_name: "Owner/Repo".to_owned(),
            owner: GitHubUser {
                id: 7,
                login: "owner".to_owned(),
            },
        };
        assert!(repo_identity_matches(&identity, 7, &repo));
        assert!(!repo_identity_matches(
            &GitHubRepoIdentity {
                full_name: "owner/renamed".to_owned(),
                ..identity
            },
            7,
            &repo
        ));
    }

    #[tokio::test]
    async fn desired_state_migration_maps_legacy_rows_without_external_side_effects() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::raw_sql(
            r#"
            CREATE TABLE users (id TEXT PRIMARY KEY, webhook_push_enabled INTEGER NOT NULL);
            CREATE TABLE webhook_push_repos (user_id TEXT NOT NULL, hook_id INTEGER);
            CREATE TABLE job_tasks (
              id TEXT PRIMARY KEY,
              task_type TEXT NOT NULL,
              requested_by TEXT,
              status TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              error_message TEXT,
              finished_at TEXT,
              runtime_owner_id TEXT,
              lease_heartbeat_at TEXT,
              cancel_requested INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE webhook_push_user_operation_leases (
              user_id TEXT PRIMARY KEY,
              task_id TEXT NOT NULL,
              expires_at TEXT NOT NULL
            );
            INSERT INTO users (id, webhook_push_enabled) VALUES ('enabled', 1), ('paused', 0), ('deleted', 0);
            INSERT INTO webhook_push_repos (user_id, hook_id) VALUES ('paused', 9001);
            INSERT INTO job_tasks (
              id, task_type, requested_by, status, payload_json, created_at, updated_at
            ) VALUES
              ('manage-1', 'webhook.push.manage', 'enabled', 'queued', '{"operation":"register"}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
              ('manage-2', 'webhook.push.manage', 'enabled', 'running', '{"operation":"delete"}', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z'),
              ('manage-3', 'webhook.push.manage', 'enabled', 'queued', '{"operation":"check"}', '2026-01-03T00:00:00Z', '2026-01-03T00:00:00Z'),
              ('audit-1', 'webhook.push.audit', 'audit', 'queued', '{"operation":"audit"}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
              ('audit-2', 'webhook.push.audit', 'audit', 'running', '{"operation":"audit"}', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z');
            INSERT INTO webhook_push_user_operation_leases (user_id, task_id, expires_at)
            VALUES
              ('enabled', 'manage-2', '2026-01-04T00:00:00Z'),
              ('audit', 'audit-2', '2026-01-04T00:00:00Z');
            "#,
        )
        .execute(&pool)
        .await
        .expect("create legacy schema");

        sqlx::raw_sql(include_str!(
            "../migrations/0080_webhook_push_desired_state.sql"
        ))
        .execute(&pool)
        .await
        .expect("apply desired state migration");

        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT id, webhook_push_desired_state FROM users ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .expect("read migrated desired states");
        assert_eq!(
            rows,
            vec![
                ("deleted".to_owned(), "deleted".to_owned()),
                ("enabled".to_owned(), "enabled".to_owned()),
                ("paused".to_owned(), "paused".to_owned()),
            ]
        );
        let task_rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, status, error_message FROM job_tasks ORDER BY CASE task_type WHEN 'webhook.push.manage' THEN 0 ELSE 1 END, id",
        )
        .fetch_all(&pool)
        .await
        .expect("read migrated task rows");
        assert_eq!(task_rows.len(), 5);
        assert_eq!(task_rows[0].1, "canceled");
        assert_eq!(task_rows[1].1, "running");
        assert_eq!(task_rows[2].1, "canceled");
        assert_eq!(task_rows[3].0, "audit-1");
        assert_eq!(task_rows[3].1, "queued");
        assert_eq!(task_rows[4].0, "audit-2");
        assert_eq!(task_rows[4].1, "running");
        assert!(sqlx::query(
            "INSERT INTO job_tasks (id, task_type, requested_by, status, payload_json, created_at, updated_at) VALUES ('manage-4', 'webhook.push.manage', 'enabled', 'queued', '{}', '2026-01-04T00:00:00Z', '2026-01-04T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .is_err());
        let lease_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM webhook_push_user_operation_leases")
                .fetch_one(&pool)
                .await
                .expect("count migrated leases");
        assert_eq!(lease_count, 2);
        let audit_lease_task = sqlx::query_scalar::<_, String>(
            "SELECT task_id FROM webhook_push_user_operation_leases WHERE user_id = 'audit'",
        )
        .fetch_one(&pool)
        .await
        .expect("audit lease survives migration");
        assert_eq!(audit_lease_task, "audit-2");
        let available_at_column = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pragma_table_info('job_tasks') WHERE name = 'available_at'",
        )
        .fetch_one(&pool)
        .await
        .expect("check available_at column");
        assert_eq!(available_at_column, 1);
    }
}
