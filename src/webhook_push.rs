use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::{Context, Result, anyhow};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
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
const OP_DELETE: &str = "delete";
const STATUS_REGISTERED: &str = "registered";
const STATUS_MISSING: &str = "missing";
const STATUS_PERMISSION_PAUSED: &str = "permission_paused";
const STATUS_ERROR: &str = "error";
const STATUS_CONFLICT: &str = "conflict";
const STATUS_DELETE_PENDING: &str = "delete_pending";
const DELIVERY_RETENTION_DAYS: i64 = 30;

type HmacSha256 = Hmac<Sha256>;

fn user_operation_lock(user_id: &str) -> Arc<tokio::sync::Mutex<()>> {
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
    webhook_push_secret_ciphertext: Option<Vec<u8>>,
    webhook_push_secret_nonce: Option<Vec<u8>>,
    webhook_push_callback_key: Option<String>,
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
    enabled: bool,
    include_own_releases: bool,
    callback_ready: bool,
    pat: PatStatus,
    summary: WebhookSummary,
    schedule: ScheduleStatus,
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
    enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct TaskEnqueueResponse {
    task_id: String,
    status: String,
    reused: bool,
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
               webhook_push_secret_ciphertext, webhook_push_secret_nonce,
               webhook_push_callback_key
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
        .send()
        .await
        .map_err(ApiError::internal)?;
    if response.status() != StatusCode::OK {
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
    if pat.owner_github_user_id != Some(github_user.id) || pat.owner_login.is_none() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_owner_mismatch",
            "PAT 所属 GitHub 账号未绑定到当前 OctoRill 账号。",
        ));
    }
    let allows_private = scopes.iter().any(|scope| scope == "repo");
    Ok((
        token,
        github_user.id,
        pat.owner_login.unwrap_or_default(),
        allows_private,
    ))
}

fn generate_secret() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

async fn ensure_secret_and_key(
    state: &AppState,
    user_id: &str,
) -> Result<(String, String), ApiError> {
    let config = load_user_config(state, user_id).await?;
    if let (Some(ciphertext), Some(nonce), Some(key)) = (
        config.webhook_push_secret_ciphertext,
        config.webhook_push_secret_nonce,
        config.webhook_push_callback_key,
    ) {
        let secret = state
            .encryption_key
            .decrypt_str(&ciphertext, &nonce)
            .map_err(ApiError::internal)?;
        return Ok((secret, key));
    }
    let secret = generate_secret();
    let key = crate::local_id::generate_local_id();
    let encrypted = state
        .encryption_key
        .encrypt_str(&secret)
        .map_err(ApiError::internal)?;
    let now = Utc::now().to_rfc3339();
    state
        .sqlite_writer
        .write_foreground("webhook_push_secret_seed", |_| async {
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
            .bind(&now)
            .bind(user_id)
            .execute(&state.pool)
            .await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
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
) -> Result<Vec<WebhookRepoStatus>, ApiError> {
    let owner_login = owner_login.unwrap_or("");
    sqlx::query_as::<_, WebhookRepoStatus>(
        r#"
        SELECT ob.repo_id,
               substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1) AS owner_login,
               substr(ob.repo_full_name, instr(ob.repo_full_name, '/') + 1) AS repo_name,
               ob.repo_full_name,
               CASE WHEN ob.is_private IS NULL THEN NULL ELSE ob.is_private != 0 END AS is_private,
               wr.hook_id,
               COALESCE(wr.status, 'unknown') AS status,
               wr.error_kind, wr.error_message,
               COALESCE(wr.permission_paused, 0) != 0 AS permission_paused,
               wr.last_checked_at, wr.last_registered_at
        FROM owned_repo_star_baselines ob
        LEFT JOIN webhook_push_repos wr
          ON wr.user_id = ob.user_id AND wr.repo_id = ob.repo_id
        WHERE ob.user_id = ?
          AND lower(substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)) = lower(?)
        ORDER BY lower(ob.repo_full_name)
        "#,
    )
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
            .filter(|repo| repo.status == STATUS_MISSING || repo.status == "unknown")
            .count(),
        permission_paused: repos.iter().filter(|repo| repo.permission_paused).count(),
        errors: repos
            .iter()
            .filter(|repo| repo.status == STATUS_ERROR || repo.status == STATUS_CONFLICT)
            .count(),
        removable: repos.iter().filter(|repo| repo.hook_id.is_some()).count(),
    }
}

pub async fn get_settings(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<WebhookPushSettingsResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let config = load_user_config(state.as_ref(), &user_id).await?;
    let pat = load_pat(state.as_ref(), &user_id).await?;
    let owner_login = pat.as_ref().and_then(|(row, _)| row.owner_login.as_deref());
    let repos = list_repo_statuses(state.as_ref(), &user_id, owner_login).await?;
    let schedule = runtime_config(state.as_ref()).await?;
    Ok(Json(WebhookPushSettingsResponse {
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
        repos,
    }))
}

pub async fn patch_settings(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(request): Json<PatchSettingsRequest>,
) -> Result<Json<Value>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let config = load_user_config(state.as_ref(), &user_id).await?;
    if request.enabled {
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
    let now = Utc::now().to_rfc3339();
    state
        .sqlite_writer
        .write_foreground("webhook_push_settings", |_| async {
            sqlx::query("UPDATE users SET webhook_push_enabled = ?, updated_at = ? WHERE id = ?")
                .bind(if request.enabled { 1_i64 } else { 0_i64 })
                .bind(&now)
                .bind(&user_id)
                .execute(&state.pool)
                .await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .map_err(ApiError::internal)?;
    let task = if request.enabled {
        Some(enqueue_manage(state.as_ref(), &user_id, OP_REGISTER, None, "enable").await?)
    } else {
        None
    };
    let task = task.map(task_response);
    Ok(Json(json!({
        "enabled": request.enabled,
        "task_id": task.as_ref().map(|task| task.task_id.as_str()),
        "status": task.as_ref().map(|task| task.status.as_str()),
        "reused": task.as_ref().is_some_and(|task| task.reused),
    })))
}

fn task_response(task: EnqueuedTask) -> TaskEnqueueResponse {
    TaskEnqueueResponse {
        task_id: task.task_id,
        status: task.status,
        reused: task.reused,
    }
}

async fn enqueue_manage(
    state: &AppState,
    user_id: &str,
    operation: &str,
    repo_id: Option<i64>,
    source: &str,
) -> Result<EnqueuedTask, ApiError> {
    jobs::enqueue_singleton_task_for_requester_and_payload(
        state,
        NewTask {
            task_type: jobs::TASK_WEBHOOK_PUSH_MANAGE.to_owned(),
            payload: json!({"user_id": user_id, "operation": operation, "repo_id": repo_id}),
            source: source.to_owned(),
            requested_by: Some(user_id.to_owned()),
            parent_task_id: None,
        },
    )
    .await
    .map_err(ApiError::internal)
}

async fn require_manage_enabled(state: &AppState, user_id: &str) -> Result<(), ApiError> {
    let config = load_user_config(state, user_id).await?;
    if config.include_own_releases == 0 || config.webhook_push_enabled == 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "webhook_push_disabled",
            "请先开启“我的发布”和“Webhook 推送”。",
        ));
    }
    Ok(())
}

pub async fn register_all(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<TaskEnqueueResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    require_manage_enabled(state.as_ref(), &user_id).await?;
    validate_pat(state.as_ref(), &user_id).await?;
    Ok(Json(task_response(
        enqueue_manage(state.as_ref(), &user_id, OP_REGISTER, None, "manual").await?,
    )))
}

pub async fn check_all(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<TaskEnqueueResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    require_manage_enabled(state.as_ref(), &user_id).await?;
    validate_pat(state.as_ref(), &user_id).await?;
    Ok(Json(task_response(
        enqueue_manage(state.as_ref(), &user_id, OP_CHECK, None, "manual").await?,
    )))
}

pub async fn delete_all(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<TaskEnqueueResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let config = load_user_config(state.as_ref(), &user_id).await?;
    if config.webhook_push_enabled != 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "disable_webhook_push_first",
            "请先关闭“Webhook 推送”，再全部删除 Webhook。",
        ));
    }
    let (_, owner_github_user_id, _, _) = validate_pat(state.as_ref(), &user_id).await?;
    ensure_delete_pat_owner(state.as_ref(), &user_id, owner_github_user_id).await?;
    state
        .sqlite_writer
        .write_foreground("webhook_push_delete_pending", |_| async {
            sqlx::query(
                "UPDATE webhook_push_repos SET status = ?, updated_at = ? WHERE user_id = ? AND hook_id IS NOT NULL",
            )
            .bind(STATUS_DELETE_PENDING)
            .bind(Utc::now().to_rfc3339())
            .bind(&user_id)
            .execute(&state.pool)
            .await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(task_response(
        enqueue_manage(state.as_ref(), &user_id, OP_DELETE, None, "manual").await?,
    )))
}

async fn ensure_delete_pat_owner(
    state: &AppState,
    user_id: &str,
    owner_github_user_id: i64,
) -> Result<(), ApiError> {
    let mismatched = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM webhook_push_repos WHERE user_id = ? AND hook_id IS NOT NULL AND owner_github_user_id IS NOT NULL AND owner_github_user_id != ?",
    )
    .bind(user_id)
    .bind(owner_github_user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if mismatched != 0 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_owner_mismatch_cleanup",
            "待删除的 Webhook 属于另一个 GitHub 账号，请恢复该账号的 classic PAT 后重试。",
        ));
    }
    Ok(())
}

pub async fn register_repo(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(repo_id): Path<i64>,
) -> Result<Json<TaskEnqueueResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    require_manage_enabled(state.as_ref(), &user_id).await?;
    validate_pat(state.as_ref(), &user_id).await?;
    Ok(Json(task_response(
        enqueue_manage(
            state.as_ref(),
            &user_id,
            OP_REGISTER,
            Some(repo_id),
            "manual",
        )
        .await?,
    )))
}

pub async fn check_repo(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(repo_id): Path<i64>,
) -> Result<Json<TaskEnqueueResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    require_manage_enabled(state.as_ref(), &user_id).await?;
    validate_pat(state.as_ref(), &user_id).await?;
    Ok(Json(task_response(
        enqueue_manage(state.as_ref(), &user_id, OP_CHECK, Some(repo_id), "manual").await?,
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
    .bind(user_id)
    .bind(Some(owner_github_user_id))
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
) -> Result<TargetRepo> {
    let url = state
        .github_rest_api_base
        .join(&format!("repositories/{}", repo.repo_id))
        .context("build GitHub repository identity URL")?;
    let response = state
        .github_rest_http
        .get(url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("load GitHub repository identity")?;
    if !response.status().is_success() {
        return Err(anyhow!(response_error(response).await.to_string()));
    }
    let identity = response
        .json::<GitHubRepoIdentity>()
        .await
        .context("decode GitHub repository identity")?;
    if identity.id != repo.repo_id || identity.owner.id != expected_owner_github_user_id {
        return Err(anyhow!(
            "GitHub PAT owner does not match the repository hook owner"
        ));
    }
    let (owner_login, repo_name) = identity
        .full_name
        .split_once('/')
        .context("GitHub repository full_name is invalid")?;
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
        message: err.to_string(),
    })?;
    let mut hooks = Vec::new();
    for page in 1_u32.. {
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
            .send()
            .await
            .map_err(|err| GitHubCallError {
                status: err.status(),
                rate_limited: false,
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
                    message: err.to_string(),
                })?;
        let is_last_page = page_hooks.len() < 100;
        hooks.extend(page_hooks);
        if is_last_page {
            return Ok(hooks);
        }
    }
    unreachable!("GitHub hook pagination exhausted the page number range")
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
        .send()
        .await
        .map_err(|err| GitHubCallError {
            status: err.status(),
            rate_limited: false,
            message: err.to_string(),
        })?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response.json().await.map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
        message: err.to_string(),
    })
}

async fn update_hook(
    state: &AppState,
    token: &str,
    repo: &TargetRepo,
    hook_id: i64,
    callback: &str,
    secret: &str,
) -> std::result::Result<GitHubHook, GitHubCallError> {
    let url = github_url(state, repo, &format!("/{hook_id}")).map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
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
            active: true,
            events: ["release"],
            config: HookRequestConfig {
                url: callback,
                content_type: "json",
                secret,
                insecure_ssl: "0",
            },
        })
        .send()
        .await
        .map_err(|err| GitHubCallError {
            status: err.status(),
            rate_limited: false,
            message: err.to_string(),
        })?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response.json().await.map_err(|err| GitHubCallError {
        status: None,
        rate_limited: false,
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
        message: err.to_string(),
    })?;
    let response = state
        .github_rest_http
        .delete(url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|err| GitHubCallError {
            status: err.status(),
            rate_limited: false,
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

async fn run_repo_operation(
    state: &AppState,
    user_id: &str,
    repo: &TargetRepo,
    operation: &str,
    token: &str,
    callback: &str,
    secret: &str,
) -> Result<&'static str> {
    if operation == OP_DELETE {
        let hook_id = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT hook_id FROM webhook_push_repos WHERE user_id = ? AND repo_id = ?",
        )
        .bind(user_id)
        .bind(repo.repo_id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
        let Some(hook_id) = hook_id else {
            return Ok("skipped");
        };
        match delete_hook(state, token, repo, hook_id).await {
            Ok(()) => {
                sqlx::query("DELETE FROM webhook_push_repos WHERE user_id = ? AND repo_id = ?")
                    .bind(user_id)
                    .bind(repo.repo_id)
                    .execute(&state.pool)
                    .await?;
                return Ok("deleted");
            }
            Err(error) => {
                let permission_paused = is_permission_error(&error);
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
                return Ok("failed");
            }
        }
    }

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
                callback,
                (status, None, Some(&error), false),
            )
            .await?;
            return Ok("failed");
        }
    };
    let matches = hooks
        .iter()
        .filter(|hook| {
            hook.config.url.as_deref() == Some(callback)
                && hook.events.iter().any(|event| event == "release")
        })
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        persist_repo_state(
            state,
            user_id,
            repo,
            callback,
            (STATUS_CONFLICT, None, None, false),
        )
        .await?;
        return Ok("conflict");
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
                callback,
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
            return Ok(if healthy { "registered" } else { "failed" });
        }
        persist_repo_state(
            state,
            user_id,
            repo,
            callback,
            (STATUS_MISSING, None, None, false),
        )
        .await?;
        return Ok("missing");
    }
    let result = if let Some(hook) = matches.first() {
        update_hook(state, token, repo, hook.id, callback, secret).await
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
            Ok("registered")
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
                (status, None, Some(&error), false),
            )
            .await?;
            Ok("failed")
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
) -> Result<Value> {
    while !try_acquire_user_operation_lease(state, user_id, task_id).await? {
        if jobs::is_task_cancel_requested(state, task_id).await? {
            return Ok(json!({"canceled": true}));
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let operation_lock = user_operation_lock(user_id);
    let _operation_guard = operation_lock.lock().await;
    let result =
        execute_for_user_locked(state, task_id, user_id, operation, repo_id, scheduled).await;
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
) -> Result<Value> {
    let config = load_user_config(state, user_id)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;
    if config.include_own_releases == 0 || config.webhook_push_enabled == 0 {
        if operation != OP_DELETE {
            return Ok(json!({"skipped": true, "reason": "disabled"}));
        }
    } else if operation == OP_DELETE {
        return Ok(json!({"skipped": true, "reason": "re_enabled"}));
    }
    let (token, owner_github_user_id, owner_login, allows_private, secret, callback) =
        if operation == OP_DELETE {
            let (pat, token) = load_pat(state, user_id)
                .await
                .map_err(|err| anyhow!(err.to_string()))?
                .ok_or_else(|| anyhow!("GitHub PAT is not configured"))?;
            let owner_github_user_id = pat
                .owner_github_user_id
                .ok_or_else(|| anyhow!("GitHub PAT owner id is not available"))?;
            let owner_login = pat
                .owner_login
                .ok_or_else(|| anyhow!("GitHub PAT owner is not available"))?;
            ensure_delete_pat_owner(state, user_id, owner_github_user_id)
                .await
                .map_err(|err| anyhow!(err.to_string()))?;
            (
                token,
                owner_github_user_id,
                owner_login,
                false,
                String::new(),
                String::new(),
            )
        } else {
            let (token, owner_github_user_id, owner_login, allows_private) =
                validate_pat(state, user_id)
                    .await
                    .map_err(|err| anyhow!(err.to_string()))?;
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
    if operation == OP_REGISTER
        && let Err(error) = sync::refresh_owned_repo_release_visibility(state, user_id).await
    {
        tracing::warn!(
            user_id,
            ?error,
            "webhook push: owned repo refresh failed; using cached baseline"
        );
    }
    let targets = if operation == OP_DELETE {
        sqlx::query_as::<_, TargetRepo>(
            r#"
            SELECT repo_id, owner_github_user_id, owner_login, repo_name, repo_full_name
            FROM webhook_push_repos WHERE user_id = ? AND hook_id IS NOT NULL
            ORDER BY lower(repo_full_name)
            "#,
        )
        .bind(user_id)
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
    for (index, repo) in targets.iter().enumerate() {
        if jobs::is_task_cancel_requested(state, task_id).await? {
            return Ok(json!({
                "operation": operation,
                "total": targets.len(),
                "counts": counts,
                "canceled": true,
            }));
        }
        renew_user_operation_lease(state, user_id, task_id).await?;
        let resolved_repo;
        let result = if operation == OP_DELETE {
            match resolve_delete_target(state, &token, owner_github_user_id, repo).await {
                Ok(resolved) => {
                    resolved_repo = resolved;
                    run_repo_operation(
                        state,
                        user_id,
                        &resolved_repo,
                        operation,
                        &token,
                        &callback,
                        &secret,
                    )
                    .await?
                }
                Err(error) => {
                    sqlx::query(
                        "UPDATE webhook_push_repos SET status = ?, error_kind = ?, error_message = ?, updated_at = ? WHERE user_id = ? AND repo_id = ?",
                    )
                    .bind(STATUS_ERROR)
                    .bind("owner_resolution")
                    .bind(error.to_string())
                    .bind(Utc::now().to_rfc3339())
                    .bind(user_id)
                    .bind(repo.repo_id)
                    .execute(&state.pool)
                    .await?;
                    "failed"
                }
            }
        } else {
            run_repo_operation(state, user_id, repo, operation, &token, &callback, &secret).await?
        };
        *counts.entry(result).or_default() += 1;
        jobs::append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "stage": operation, "repo_id": repo.repo_id, "repo": repo.repo_full_name,
                "index": index + 1, "total": targets.len(), "result": result,
            }),
        )
        .await?;
    }
    Ok(json!({"operation": operation, "total": targets.len(), "counts": counts}))
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
    execute_for_user(state, task_id, user_id, operation, repo_id, false).await
}

pub async fn execute_audit_task(state: &AppState, task_id: &str) -> Result<Value> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE admin_runtime_settings SET webhook_push_audit_last_started_at = ?, updated_at = ? WHERE id = 1")
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await?;
    let users = sqlx::query_scalar::<_, String>(
        "SELECT id FROM users WHERE include_own_releases != 0 AND webhook_push_enabled != 0 ORDER BY id",
    ).fetch_all(&state.pool).await?;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for user_id in &users {
        match execute_for_user(state, task_id, user_id, OP_REGISTER, None, true).await {
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
    repo_status: &str,
) -> bool {
    include_own_releases
        && enabled
        && event == "release"
        && payload.action.as_deref() == Some("published")
        && payload.release.as_ref().is_some_and(|item| !item.draft)
        && payload
            .repository
            .as_ref()
            .is_some_and(|item| item.id == expected_repo_id)
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
    let row = sqlx::query_as::<_, (String, i64, i64, Vec<u8>, Vec<u8>, i64, String)>(
        r#"
        SELECT u.id, u.include_own_releases, u.webhook_push_enabled,
               u.webhook_push_secret_ciphertext, u.webhook_push_secret_nonce,
               wr.repo_id, wr.status
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
    let should_queue =
        should_enqueue_release(row.1 != 0, row.2 != 0, event, &payload, row.5, &row.6);
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
            STATUS_REGISTERED,
        ));

        for candidate in [
            should_enqueue_release(false, true, "release", &payload, 42, STATUS_REGISTERED),
            should_enqueue_release(true, false, "release", &payload, 42, STATUS_REGISTERED),
            should_enqueue_release(true, true, "push", &payload, 42, STATUS_REGISTERED),
            should_enqueue_release(true, true, "release", &payload, 99, STATUS_REGISTERED),
            should_enqueue_release(true, true, "release", &payload, 42, STATUS_MISSING),
            should_enqueue_release(
                true,
                true,
                "release",
                &release_payload("edited", 42, false),
                42,
                STATUS_REGISTERED,
            ),
            should_enqueue_release(
                true,
                true,
                "release",
                &release_payload("published", 42, true),
                42,
                STATUS_REGISTERED,
            ),
        ] {
            assert!(!candidate);
        }
    }
}
