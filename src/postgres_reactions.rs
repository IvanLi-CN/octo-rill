//! PostgreSQL reaction PAT and release reaction endpoints.
//!
//! Reaction PATs live encrypted in `public.reaction_pat_tokens`; live reaction
//! state is read from and mutated through GitHub, while cached counts are kept
//! in `public.repo_releases`.

use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_sessions::Session;

use crate::{error::ApiError, postgres_server::PostgresServerState};

const SESSION_KEY_USER_ID: &str = "user_id";

fn invalid_api_key() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_api_key",
        "invalid API key",
    )
}

async fn require_user_id(session: &Session) -> Result<String, ApiError> {
    match session.get::<String>(SESSION_KEY_USER_ID).await {
        Ok(Some(user_id)) => Ok(user_id),
        Ok(None) => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "not logged in",
        )),
        Err(error) => Err(ApiError::internal(error)),
    }
}

async fn require_active_user_id(
    state: &PostgresServerState,
    session: &Session,
) -> Result<String, ApiError> {
    let user_id = require_user_id(session).await?;
    let row = sqlx::query_as::<_, (i64, Option<String>)>(
        "SELECT is_disabled, paused_at FROM public.users WHERE id = $1",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some((is_disabled, paused_at)) = row else {
        let _ = session.clear().await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "session user not found",
        ));
    };
    if is_disabled != 0 {
        let _ = session.clear().await;
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }
    if paused_at.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_paused",
            "account is paused",
        ));
    }
    if let Err(error) = sqlx::query("UPDATE public.users SET last_active_at = $1 WHERE id = $2")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&user_id)
        .execute(&state.runtime.pool)
        .await
    {
        tracing::warn!(%error, user_id, "PostgreSQL last_active_at touch failed");
    }
    Ok(user_id)
}

async fn authenticate_api_key(
    state: &PostgresServerState,
    api_key: &str,
) -> Result<String, ApiError> {
    if !api_key.starts_with(crate::api_keys::API_KEY_PREFIX) {
        return Err(invalid_api_key());
    }
    let key_hash = crate::api_keys::hash_api_key(api_key);
    let row = sqlx::query_as::<_, (String, i64, Option<String>)>(
        "SELECT k.user_id, u.is_disabled, u.paused_at FROM public.user_api_keys k JOIN public.users u ON u.id = k.user_id WHERE k.key_hash = $1 AND k.revoked_at IS NULL LIMIT 1",
    )
    .bind(&key_hash)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some((user_id, is_disabled, paused_at)) = row else {
        return Err(invalid_api_key());
    };
    if is_disabled != 0 {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }
    if paused_at.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_paused",
            "account is paused",
        ));
    }
    Ok(user_id)
}

async fn require_business_user_id(
    state: &PostgresServerState,
    session: &Session,
    headers: &HeaderMap,
) -> Result<String, ApiError> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let value = value.to_str().map_err(|_| invalid_api_key())?;
        let value = value.trim_start();
        let mut auth_parts = value.splitn(2, char::is_whitespace);
        let scheme = auth_parts.next().unwrap_or("");
        if scheme.eq_ignore_ascii_case("Bearer") {
            let api_key = auth_parts.next().unwrap_or("").trim();
            if !api_key.starts_with(crate::api_keys::API_KEY_PREFIX) {
                return Err(invalid_api_key());
            }
            return authenticate_api_key(state, api_key).await;
        }
        return require_active_user_id(state, session).await;
    }
    require_active_user_id(state, session).await
}

#[derive(Debug, Serialize)]
pub(crate) struct ReactionTokenCheckSummary {
    state: String,
    message: Option<String>,
    checked_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct ReactionTokenOwnerSummary {
    github_connection_id: String,
    github_user_id: i64,
    login: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReactionTokenStatusResponse {
    configured: bool,
    masked_token: Option<String>,
    check: ReactionTokenCheckSummary,
    owner: Option<ReactionTokenOwnerSummary>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReactionTokenRequest {
    token: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReactionTokenCheckResponse {
    state: String,
    message: String,
    owner: Option<ReactionTokenOwnerSummary>,
    #[serde(skip)]
    allows_private_repos: Option<bool>,
}

fn mask_pat_token(token: &str) -> String {
    let trimmed = token.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "********".to_owned();
    }
    let head = chars[..4].iter().collect::<String>();
    let tail = chars[chars.len().saturating_sub(4)..]
        .iter()
        .collect::<String>();
    format!("{head}...{tail}")
}

fn has_repo_scope(scopes: &str) -> bool {
    scopes
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .any(|scope| scope == "repo")
}

fn has_public_repo_scope(scopes: &str) -> bool {
    scopes
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .any(|scope| scope == "public_repo")
}

fn is_rate_limit_message(message: &str) -> bool {
    message.to_ascii_lowercase().contains("rate limit")
}

fn is_reauth_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("bad credentials") || lower.contains("requires authentication")
}

fn is_access_restricted_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("resource not accessible by integration")
        || lower.contains("saml")
        || lower.contains("oauth app access restrictions")
}

fn github_reauth_required_error() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "reauth_required",
        "repo scope required; re-login via GitHub OAuth",
    )
}

fn github_rate_limited_error() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "github rate limit exceeded; retry later",
    )
}

fn github_access_restricted_error() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "forbidden",
        "github denied reaction access for this repository (OAuth app restrictions or org policy)",
    )
}

fn github_graphql_http_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> Option<ApiError> {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Some(github_reauth_required_error());
    }
    if status != reqwest::StatusCode::FORBIDDEN {
        return None;
    }

    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    if remaining == Some("0") || is_rate_limit_message(body) {
        return Some(github_rate_limited_error());
    }
    if is_reauth_message(body) {
        return Some(github_reauth_required_error());
    }
    if is_access_restricted_message(body) {
        return Some(github_access_restricted_error());
    }
    None
}

fn github_graphql_errors_to_api_error(errors: &[GraphQlError]) -> Option<ApiError> {
    if errors
        .iter()
        .any(|error| is_rate_limit_message(&error.message))
    {
        return Some(github_rate_limited_error());
    }
    if errors.iter().any(|error| is_reauth_message(&error.message)) {
        return Some(github_reauth_required_error());
    }
    if errors
        .iter()
        .any(|error| is_access_restricted_message(&error.message))
    {
        return Some(github_access_restricted_error());
    }
    None
}

async fn check_reaction_pat_with_github(
    state: &PostgresServerState,
    token: &str,
    user_id: Option<&str>,
) -> Result<ReactionTokenCheckResponse, ApiError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(ApiError::bad_request("token is required"));
    }

    let url = format!("{}/user", crate::github_api::rest_base());
    let response = state
        .http
        .get(url)
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(ApiError::internal)?;

    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await.map_err(ApiError::internal)?;

    if status == reqwest::StatusCode::OK {
        let github_user =
            serde_json::from_str::<serde_json::Value>(&body).map_err(ApiError::internal)?;
        let github_user_id = github_user
            .get("id")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| ApiError::internal("github user id missing from PAT check"))?;
        let github_login = github_user
            .get("login")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ApiError::internal("github login missing from PAT check"))?
            .to_owned();
        let scopes = headers
            .get("x-oauth-scopes")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if !scopes.is_empty() && !has_repo_scope(scopes) && !has_public_repo_scope(scopes) {
            return Ok(ReactionTokenCheckResponse {
                state: "invalid".to_owned(),
                message: "classic PAT needs public_repo (public) or repo (private)".to_owned(),
                owner: None,
                allows_private_repos: None,
            });
        }

        let owner = if let Some(user_id) = user_id {
            let owner = sqlx::query_as::<_, (String, i64, String)>(
                r#"
                SELECT id, github_user_id, login
                FROM public.github_connections
                WHERE user_id = $1
                  AND github_user_id = $2
                LIMIT 1
                "#,
            )
            .bind(user_id)
            .bind(github_user_id)
            .fetch_optional(&state.runtime.pool)
            .await
            .map_err(ApiError::internal)?;

            let Some((github_connection_id, github_user_id, login)) = owner else {
                return Ok(ReactionTokenCheckResponse {
                    state: "invalid".to_owned(),
                    message:
                        "PAT owner is not bound to the current OctoRill account; bind that GitHub account first"
                            .to_owned(),
                    owner: None,
                    allows_private_repos: None,
                });
            };
            Some(ReactionTokenOwnerSummary {
                github_connection_id,
                github_user_id,
                login,
            })
        } else {
            Some(ReactionTokenOwnerSummary {
                github_connection_id: String::new(),
                github_user_id,
                login: github_login,
            })
        };

        return Ok(ReactionTokenCheckResponse {
            state: "valid".to_owned(),
            message: "token is valid".to_owned(),
            owner,
            allows_private_repos: (!scopes.is_empty()).then(|| has_repo_scope(scopes)),
        });
    }

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(ReactionTokenCheckResponse {
            state: "invalid".to_owned(),
            message: "token is invalid or expired".to_owned(),
            owner: None,
            allows_private_repos: None,
        });
    }

    if status == reqwest::StatusCode::FORBIDDEN {
        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            .map(str::trim);
        if remaining == Some("0") || is_rate_limit_message(&body) {
            return Err(github_rate_limited_error());
        }
        return Ok(ReactionTokenCheckResponse {
            state: "invalid".to_owned(),
            message: "token cannot access GitHub user API; check PAT permissions".to_owned(),
            owner: None,
            allows_private_repos: None,
        });
    }

    Err(ApiError::new(
        StatusCode::BAD_GATEWAY,
        "github_unavailable",
        format!("github check failed with status {status}"),
    ))
}

async fn load_reaction_pat_token(
    state: &PostgresServerState,
    user_id: &str,
) -> Result<Option<String>, ApiError> {
    let row = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        "SELECT token_ciphertext, token_nonce FROM public.reaction_pat_tokens WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;

    row.map(|(ciphertext, nonce)| {
        state
            .encryption_key
            .decrypt_str(&ciphertext, &nonce)
            .map_err(|_| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "pat_invalid",
                    "PAT is invalid or expired",
                )
            })
    })
    .transpose()
}

async fn persist_reaction_pat_check_result(
    state: &PostgresServerState,
    user_id: &str,
    check_state: &str,
    check_message: Option<&str>,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE public.reaction_pat_tokens
        SET last_check_state = $1,
            last_check_message = $2,
            last_checked_at = $3,
            webhook_push_allows_private_repos = CASE
              WHEN $4 = 'valid' THEN webhook_push_allows_private_repos
              ELSE NULL
            END,
            updated_at = $5
        WHERE user_id = $6
        "#,
    )
    .bind(check_state)
    .bind(check_message)
    .bind(now.as_str())
    .bind(check_state)
    .bind(now.as_str())
    .bind(user_id)
    .execute(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn clear_reaction_pat_scope_observation(
    state: &PostgresServerState,
    user_id: &str,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE public.reaction_pat_tokens SET webhook_push_allows_private_repos = NULL, updated_at = $1 WHERE user_id = $2",
    )
    .bind(now.as_str())
    .bind(user_id)
    .execute(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

pub(crate) async fn check_reaction_token(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<ReactionTokenRequest>,
) -> Result<Json<ReactionTokenCheckResponse>, ApiError> {
    let user_id = require_active_user_id(&state, &session).await?;
    let checked =
        check_reaction_pat_with_github(&state, request.token.as_str(), Some(user_id.as_str()))
            .await?;
    if checked.state != "valid" {
        clear_reaction_pat_scope_observation(&state, &user_id).await?;
    }
    Ok(Json(checked))
}

pub(crate) async fn upsert_reaction_token(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<ReactionTokenRequest>,
) -> Result<Json<ReactionTokenStatusResponse>, ApiError> {
    let user_id = require_active_user_id(&state, &session).await?;
    let token = request.token.trim();
    if token.is_empty() {
        return Err(ApiError::bad_request("token is required"));
    }

    let checked = check_reaction_pat_with_github(&state, token, Some(user_id.as_str())).await?;
    if checked.state != "valid" {
        clear_reaction_pat_scope_observation(&state, &user_id).await?;
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_invalid",
            checked.message,
        ));
    }
    let owner = checked.owner.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_invalid",
            "PAT owner is not bound to the current OctoRill account",
        )
    })?;

    let now = chrono::Utc::now().to_rfc3339();
    let encrypted = state
        .encryption_key
        .encrypt_str(token)
        .map_err(ApiError::internal)?;
    let masked = mask_pat_token(token);
    let allows_private_repos = checked.allows_private_repos.map(i64::from);

    sqlx::query(
        r#"
        INSERT INTO public.reaction_pat_tokens (
          user_id, token_ciphertext, token_nonce, masked_token,
          last_check_state, last_check_message, last_checked_at, updated_at,
          owner_github_connection_id, owner_github_user_id, owner_login,
          webhook_push_allows_private_repos
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT(user_id) DO UPDATE SET
          token_ciphertext = excluded.token_ciphertext,
          token_nonce = excluded.token_nonce,
          masked_token = excluded.masked_token,
          last_check_state = excluded.last_check_state,
          last_check_message = excluded.last_check_message,
          last_checked_at = excluded.last_checked_at,
          updated_at = excluded.updated_at,
          owner_github_connection_id = excluded.owner_github_connection_id,
          owner_github_user_id = excluded.owner_github_user_id,
          owner_login = excluded.owner_login,
          webhook_push_allows_private_repos = excluded.webhook_push_allows_private_repos
        "#,
    )
    .bind(user_id.as_str())
    .bind(&encrypted.ciphertext)
    .bind(&encrypted.nonce)
    .bind(&masked)
    .bind("valid")
    .bind("token is valid")
    .bind(&now)
    .bind(&now)
    .bind(owner.github_connection_id.as_str())
    .bind(owner.github_user_id)
    .bind(owner.login.as_str())
    .bind(allows_private_repos)
    .execute(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(ReactionTokenStatusResponse {
        configured: true,
        masked_token: Some(masked),
        check: ReactionTokenCheckSummary {
            state: "valid".to_owned(),
            message: Some("token is valid".to_owned()),
            checked_at: Some(now),
        },
        owner: Some(owner),
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct FeedReactionRefreshRequest {
    release_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FeedReactionRefreshResponse {
    items: Vec<FeedReactionRefreshItem>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FeedReactionRefreshItem {
    release_id: String,
    reactions: ReleaseReactions,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToggleReleaseReactionRequest {
    release_id: String,
    content: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ToggleReleaseReactionResponse {
    release_id: String,
    reactions: ReleaseReactions,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct ReleaseReactions {
    counts: ReleaseReactionCounts,
    viewer: ReleaseReactionViewer,
    status: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub(crate) struct ReleaseReactionCounts {
    plus1: i64,
    laugh: i64,
    heart: i64,
    hooray: i64,
    rocket: i64,
    eyes: i64,
}

#[derive(Debug, Serialize, Clone, Default)]
pub(crate) struct ReleaseReactionViewer {
    plus1: bool,
    laugh: bool,
    heart: bool,
    hooray: bool,
    rocket: bool,
    eyes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseReactionContent {
    Plus1,
    Laugh,
    Heart,
    Hooray,
    Rocket,
    Eyes,
}

impl ReleaseReactionContent {
    fn from_client_str(raw: &str) -> Option<Self> {
        match raw {
            "plus1" => Some(Self::Plus1),
            "laugh" => Some(Self::Laugh),
            "heart" => Some(Self::Heart),
            "hooray" => Some(Self::Hooray),
            "rocket" => Some(Self::Rocket),
            "eyes" => Some(Self::Eyes),
            _ => None,
        }
    }

    fn as_graphql_enum(self) -> &'static str {
        match self {
            Self::Plus1 => "THUMBS_UP",
            Self::Laugh => "LAUGH",
            Self::Heart => "HEART",
            Self::Hooray => "HOORAY",
            Self::Rocket => "ROCKET",
            Self::Eyes => "EYES",
        }
    }
}

#[derive(Debug, FromRow)]
struct ReleaseReactionRow {
    release_id: i64,
    node_id: Option<String>,
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
#[serde(rename_all = "camelCase")]
struct GraphQlReleaseReactionsData {
    nodes: Vec<Option<GraphQlReleaseNode>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReleaseNode {
    id: String,
    reaction_groups: Vec<GraphQlReactionGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReactionGroup {
    content: String,
    viewer_has_reacted: bool,
    reactors: GraphQlReactors,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReactors {
    total_count: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddReactionData {
    add_reaction: Option<GraphQlMutationPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveReactionData {
    remove_reaction: Option<GraphQlMutationPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlMutationPayload {
    subject: Option<GraphQlReleaseSubject>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReleaseSubject {
    id: String,
    reaction_groups: Vec<GraphQlReactionGroup>,
}

#[derive(Debug, Clone)]
struct LiveReleaseReactions {
    counts: ReleaseReactionCounts,
    viewer: ReleaseReactionViewer,
}

fn apply_group_to_reactions(
    counts: &mut ReleaseReactionCounts,
    viewer: &mut ReleaseReactionViewer,
    content: &str,
    total_count: i64,
    viewer_has_reacted: bool,
) {
    match content {
        "THUMBS_UP" => {
            counts.plus1 = total_count;
            viewer.plus1 = viewer_has_reacted;
        }
        "LAUGH" => {
            counts.laugh = total_count;
            viewer.laugh = viewer_has_reacted;
        }
        "HEART" => {
            counts.heart = total_count;
            viewer.heart = viewer_has_reacted;
        }
        "HOORAY" => {
            counts.hooray = total_count;
            viewer.hooray = viewer_has_reacted;
        }
        "ROCKET" => {
            counts.rocket = total_count;
            viewer.rocket = viewer_has_reacted;
        }
        "EYES" => {
            counts.eyes = total_count;
            viewer.eyes = viewer_has_reacted;
        }
        _ => {}
    }
}

fn counts_from_groups(groups: &[GraphQlReactionGroup]) -> ReleaseReactionCounts {
    let mut counts = ReleaseReactionCounts::default();
    let mut viewer = ReleaseReactionViewer::default();
    for group in groups {
        apply_group_to_reactions(
            &mut counts,
            &mut viewer,
            group.content.as_str(),
            group.reactors.total_count,
            group.viewer_has_reacted,
        );
    }
    counts
}

fn viewer_from_groups(groups: &[GraphQlReactionGroup]) -> ReleaseReactionViewer {
    let mut counts = ReleaseReactionCounts::default();
    let mut viewer = ReleaseReactionViewer::default();
    for group in groups {
        apply_group_to_reactions(
            &mut counts,
            &mut viewer,
            group.content.as_str(),
            group.reactors.total_count,
            group.viewer_has_reacted,
        );
    }
    viewer
}

async fn fetch_live_release_reactions(
    state: &PostgresServerState,
    access_token: &str,
    node_ids: &[String],
) -> Result<HashMap<String, LiveReleaseReactions>, ApiError> {
    if node_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let query = r#"
      query($ids: [ID!]!) {
        nodes(ids: $ids) {
          ... on Release {
            id
            reactionGroups {
              content
              viewerHasReacted
              reactors(first: 1) {
                totalCount
              }
            }
          }
        }
      }
    "#;
    let payload = serde_json::json!({
        "query": query,
        "variables": { "ids": node_ids },
    });

    let url = crate::github_api::graphql_url();
    let response = state
        .http
        .post(url)
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&payload)
        .send()
        .await
        .map_err(ApiError::internal)?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let headers = response.headers().clone();
        let body = response.text().await.map_err(ApiError::internal)?;
        if let Some(error) = github_graphql_http_error(status, &headers, &body) {
            return Err(error);
        }
        return Err(ApiError::internal(format!(
            "github graphql returned {status}: {body}"
        )));
    }

    let graphql = response
        .error_for_status()
        .map_err(ApiError::internal)?
        .json::<GraphQlResponse<GraphQlReleaseReactionsData>>()
        .await
        .map_err(ApiError::internal)?;

    let GraphQlResponse { data, errors } = graphql;
    if let Some(errors) = errors
        && !errors.is_empty()
        && data.is_none()
    {
        if let Some(error) = github_graphql_errors_to_api_error(&errors) {
            return Err(error);
        }
        let message = errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ApiError::internal(format!(
            "github graphql error: {message}"
        )));
    }

    let mut out = HashMap::new();
    for node in data
        .map(|data| data.nodes)
        .unwrap_or_default()
        .into_iter()
        .flatten()
    {
        out.insert(
            node.id,
            LiveReleaseReactions {
                counts: counts_from_groups(&node.reaction_groups),
                viewer: viewer_from_groups(&node.reaction_groups),
            },
        );
    }
    Ok(out)
}

async fn mutate_release_reaction(
    state: &PostgresServerState,
    access_token: &str,
    node_id: &str,
    content: ReleaseReactionContent,
    currently_reacted: bool,
) -> Result<LiveReleaseReactions, ApiError> {
    let (query, key) = if currently_reacted {
        (
            r#"
            mutation($input: RemoveReactionInput!) {
              removeReaction(input: $input) {
                subject {
                  ... on Release {
                    id
                    reactionGroups {
                      content
                      viewerHasReacted
                      reactors(first: 1) {
                        totalCount
                      }
                    }
                  }
                }
              }
            }
            "#,
            "removeReaction",
        )
    } else {
        (
            r#"
            mutation($input: AddReactionInput!) {
              addReaction(input: $input) {
                subject {
                  ... on Release {
                    id
                    reactionGroups {
                      content
                      viewerHasReacted
                      reactors(first: 1) {
                        totalCount
                      }
                    }
                  }
                }
              }
            }
            "#,
            "addReaction",
        )
    };

    let payload = serde_json::json!({
        "query": query,
        "variables": {
            "input": {
                "subjectId": node_id,
                "content": content.as_graphql_enum(),
            }
        }
    });

    let url = crate::github_api::graphql_url();
    let response = state
        .http
        .post(url)
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&payload)
        .send()
        .await
        .map_err(ApiError::internal)?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let headers = response.headers().clone();
        let body = response.text().await.map_err(ApiError::internal)?;
        if let Some(error) = github_graphql_http_error(status, &headers, &body) {
            return Err(error);
        }
        return Err(ApiError::internal(format!(
            "github graphql returned {status}: {body}"
        )));
    }

    let body = response
        .error_for_status()
        .map_err(ApiError::internal)?
        .text()
        .await
        .map_err(ApiError::internal)?;

    let (subject, missing_payload) = if key == "removeReaction" {
        let parsed = serde_json::from_str::<GraphQlResponse<RemoveReactionData>>(&body)
            .map_err(ApiError::internal)?;
        if let Some(errors) = parsed.errors
            && !errors.is_empty()
        {
            if let Some(error) = github_graphql_errors_to_api_error(&errors) {
                return Err(error);
            }
            let message = errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ApiError::internal(format!(
                "github graphql error: {message}"
            )));
        }
        let Some(data) = parsed.data else {
            return Err(ApiError::internal("missing graphql data"));
        };
        let Some(payload) = data.remove_reaction else {
            return Err(ApiError::internal("missing removeReaction payload"));
        };
        (payload.subject, "missing mutation subject")
    } else {
        let parsed = serde_json::from_str::<GraphQlResponse<AddReactionData>>(&body)
            .map_err(ApiError::internal)?;
        if let Some(errors) = parsed.errors
            && !errors.is_empty()
        {
            if let Some(error) = github_graphql_errors_to_api_error(&errors) {
                return Err(error);
            }
            let message = errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ApiError::internal(format!(
                "github graphql error: {message}"
            )));
        }
        let Some(data) = parsed.data else {
            return Err(ApiError::internal("missing graphql data"));
        };
        let Some(payload) = data.add_reaction else {
            return Err(ApiError::internal("missing addReaction payload"));
        };
        (payload.subject, "missing mutation subject")
    };

    let Some(subject) = subject else {
        return Err(ApiError::internal(missing_payload));
    };
    let _subject_id = subject.id;
    Ok(LiveReleaseReactions {
        counts: counts_from_groups(&subject.reaction_groups),
        viewer: viewer_from_groups(&subject.reaction_groups),
    })
}

async fn fetch_visible_release_reaction_rows(
    state: &PostgresServerState,
    user_id: &str,
    release_ids: &[i64],
) -> Result<Vec<ReleaseReactionRow>, ApiError> {
    if release_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = (0..release_ids.len())
        .map(|index| format!("${}", index + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT rr.release_id, rr.node_id
        FROM public.repo_releases rr
        JOIN public.user_release_visible_repos sr
          ON sr.user_id = $1 AND sr.repo_id = rr.repo_id
        WHERE rr.release_id IN ({placeholders})
        "#
    );

    let mut query = sqlx::query_as::<_, ReleaseReactionRow>(&sql).bind(user_id);
    for release_id in release_ids {
        query = query.bind(release_id);
    }
    query
        .fetch_all(&state.runtime.pool)
        .await
        .map_err(ApiError::internal)
}

async fn persist_release_reaction_counts(
    state: &PostgresServerState,
    release_id: i64,
    counts: &ReleaseReactionCounts,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE public.repo_releases
        SET react_plus1 = $1,
            react_laugh = $2,
            react_heart = $3,
            react_hooray = $4,
            react_rocket = $5,
            react_eyes = $6,
            updated_at = $7
        WHERE release_id = $8
        "#,
    )
    .bind(counts.plus1)
    .bind(counts.laugh)
    .bind(counts.heart)
    .bind(counts.hooray)
    .bind(counts.rocket)
    .bind(counts.eyes)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(release_id)
    .execute(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn persist_release_reaction_counts_after_remote_mutation(
    state: &PostgresServerState,
    release_id: i64,
    counts: &ReleaseReactionCounts,
) {
    match persist_release_reaction_counts(state, release_id, counts).await {
        Ok(()) => {}
        Err(error) => tracing::warn!(
            event = "feed.reactions.remote_mutation_succeeded_local_persist_failed",
            release_id,
            error_kind = "persist_failed",
            api_error_code = error.code(),
            error = %error,
            "remote reaction mutation succeeded but local reaction counts were not persisted"
        ),
    }
}

fn parse_unique_release_ids(raw_ids: &[String], max_items: usize) -> Result<Vec<i64>, ApiError> {
    if raw_ids.is_empty() {
        return Err(ApiError::bad_request("release_ids is required"));
    }
    if raw_ids.len() > max_items {
        return Err(ApiError::bad_request(format!(
            "release_ids supports at most {max_items} items"
        )));
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_ids {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ApiError::bad_request("release_id is required"));
        }
        let release_id = trimmed
            .parse::<i64>()
            .map_err(|_| ApiError::bad_request("release_id must be an integer string"))?;
        if seen.insert(release_id) {
            out.push(release_id);
        }
    }
    if out.is_empty() {
        return Err(ApiError::bad_request("release_ids is required"));
    }
    Ok(out)
}

pub(crate) async fn toggle_release_reaction(
    State(state): State<PostgresServerState>,
    session: Session,
    headers: HeaderMap,
    Json(request): Json<ToggleReleaseReactionRequest>,
) -> Result<Json<ToggleReleaseReactionResponse>, ApiError> {
    let user_id = require_business_user_id(&state, &session, &headers).await?;
    let release_id_raw = request.release_id.trim();
    if release_id_raw.is_empty() {
        return Err(ApiError::bad_request("release_id is required"));
    }
    let release_id = release_id_raw
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("release_id must be an integer string"))?;
    let Some(content) = ReleaseReactionContent::from_client_str(request.content.trim()) else {
        return Err(ApiError::bad_request("invalid reaction content"));
    };

    let token = match load_reaction_pat_token(&state, &user_id).await {
        Ok(Some(token)) => token,
        Ok(None) => {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "pat_required",
                "release reactions require a GitHub PAT",
            ));
        }
        Err(error) if error.code() == "pat_invalid" => {
            persist_reaction_pat_check_result(
                &state,
                &user_id,
                "invalid",
                Some("PAT is invalid or expired"),
            )
            .await?;
            return Err(error);
        }
        Err(error) => return Err(error),
    };

    let row = sqlx::query_as::<_, ReleaseReactionRow>(
        r#"
        SELECT rr.release_id, rr.node_id
        FROM public.repo_releases rr
        JOIN public.user_release_visible_repos sr
          ON sr.user_id = $1 AND sr.repo_id = rr.repo_id
        WHERE rr.release_id = $2
        "#,
    )
    .bind(&user_id)
    .bind(release_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "release not found",
        ));
    };

    let Some(node_id) = row
        .node_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "sync_required",
            "release reaction data is not ready; sync releases first",
        ));
    };

    let current = match fetch_live_release_reactions(&state, &token, &[node_id.to_owned()]).await {
        Ok(value) => value,
        Err(error) if error.code() == "reauth_required" => {
            persist_reaction_pat_check_result(
                &state,
                &user_id,
                "invalid",
                Some("PAT is invalid or expired"),
            )
            .await?;
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "pat_invalid",
                "PAT is invalid or expired",
            ));
        }
        Err(error) => return Err(error),
    };
    let Some(current_reactions) = current.get(node_id) else {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "pat_forbidden",
            "PAT cannot access this release repository; check token repository access",
        ));
    };
    let currently_reacted = match content {
        ReleaseReactionContent::Plus1 => current_reactions.viewer.plus1,
        ReleaseReactionContent::Laugh => current_reactions.viewer.laugh,
        ReleaseReactionContent::Heart => current_reactions.viewer.heart,
        ReleaseReactionContent::Hooray => current_reactions.viewer.hooray,
        ReleaseReactionContent::Rocket => current_reactions.viewer.rocket,
        ReleaseReactionContent::Eyes => current_reactions.viewer.eyes,
    };

    // The live read already proved the PAT, so persist that state before the
    // remote mutation. A failed local write must not turn a completed mutation
    // into an ambiguous client-visible error.
    persist_reaction_pat_check_result(&state, &user_id, "valid", Some("PAT is valid")).await?;
    let updated =
        match mutate_release_reaction(&state, &token, node_id, content, currently_reacted).await {
            Ok(value) => value,
            Err(error) if error.code() == "reauth_required" => {
                persist_reaction_pat_check_result(
                    &state,
                    &user_id,
                    "invalid",
                    Some("PAT is invalid or expired"),
                )
                .await?;
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "pat_invalid",
                    "PAT is invalid or expired",
                ));
            }
            Err(error) => return Err(error),
        };
    persist_release_reaction_counts_after_remote_mutation(&state, row.release_id, &updated.counts)
        .await;

    Ok(Json(ToggleReleaseReactionResponse {
        release_id: row.release_id.to_string(),
        reactions: ReleaseReactions {
            counts: updated.counts,
            viewer: updated.viewer,
            status: "ready".to_owned(),
        },
    }))
}

pub(crate) async fn refresh_feed_reactions(
    State(state): State<PostgresServerState>,
    session: Session,
    headers: HeaderMap,
    Json(request): Json<FeedReactionRefreshRequest>,
) -> Result<Json<FeedReactionRefreshResponse>, ApiError> {
    let started_at = Instant::now();
    let user_id = require_business_user_id(&state, &session, &headers).await?;
    let release_ids = parse_unique_release_ids(&request.release_ids, 100)?;

    let db_started_at = Instant::now();
    let rows = fetch_visible_release_reaction_rows(&state, &user_id, &release_ids).await?;
    let db_elapsed = db_started_at.elapsed();

    let mut node_ids = rows
        .iter()
        .filter_map(|row| row.node_id.as_deref().map(str::trim))
        .filter(|node_id| !node_id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    node_ids.sort();
    node_ids.dedup();

    if node_ids.is_empty() {
        tracing::info!(
            db_ms = db_elapsed.as_millis() as u64,
            github_ms = 0_u64,
            persist_ms = 0_u64,
            total_ms = started_at.elapsed().as_millis() as u64,
            release_count = release_ids.len(),
            refreshed_count = 0_usize,
            "feed reaction refresh skipped without release node ids"
        );
        return Ok(Json(FeedReactionRefreshResponse { items: Vec::new() }));
    }

    let token = match load_reaction_pat_token(&state, &user_id).await {
        Ok(Some(token)) => token,
        Ok(None) => {
            tracing::info!(
                db_ms = db_elapsed.as_millis() as u64,
                github_ms = 0_u64,
                persist_ms = 0_u64,
                total_ms = started_at.elapsed().as_millis() as u64,
                release_count = release_ids.len(),
                refreshed_count = 0_usize,
                "feed reaction refresh skipped without configured PAT"
            );
            return Ok(Json(FeedReactionRefreshResponse { items: Vec::new() }));
        }
        Err(error) if error.code() == "pat_invalid" => {
            persist_reaction_pat_check_result(
                &state,
                &user_id,
                "invalid",
                Some("PAT is invalid or expired"),
            )
            .await?;
            return Err(error);
        }
        Err(error) => return Err(error),
    };

    let github_started_at = Instant::now();
    let live = match fetch_live_release_reactions(&state, &token, &node_ids).await {
        Ok(live) => live,
        Err(error) if error.code() == "reauth_required" => {
            persist_reaction_pat_check_result(
                &state,
                &user_id,
                "invalid",
                Some("PAT is invalid or expired"),
            )
            .await?;
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "pat_invalid",
                "PAT is invalid or expired",
            ));
        }
        Err(error) => return Err(error),
    };
    let github_elapsed = github_started_at.elapsed();

    let persist_started_at = Instant::now();
    let mut items = Vec::new();
    for row in rows {
        let Some(node_id) = row
            .node_id
            .as_deref()
            .map(str::trim)
            .filter(|node_id| !node_id.is_empty())
        else {
            continue;
        };
        let Some(reaction) = live.get(node_id) else {
            continue;
        };
        persist_release_reaction_counts(&state, row.release_id, &reaction.counts).await?;
        items.push(FeedReactionRefreshItem {
            release_id: row.release_id.to_string(),
            reactions: ReleaseReactions {
                counts: reaction.counts.clone(),
                viewer: reaction.viewer.clone(),
                status: "ready".to_owned(),
            },
        });
    }
    let persist_elapsed = persist_started_at.elapsed();

    tracing::info!(
        db_ms = db_elapsed.as_millis() as u64,
        github_ms = github_elapsed.as_millis() as u64,
        persist_ms = persist_elapsed.as_millis() as u64,
        total_ms = started_at.elapsed().as_millis() as u64,
        release_count = release_ids.len(),
        refreshed_count = items.len(),
        "feed reaction refresh completed outside feed hot path"
    );

    Ok(Json(FeedReactionRefreshResponse { items }))
}
