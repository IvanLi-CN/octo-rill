use anyhow::{Context, Result, anyhow};
use oauth2::{
    AuthUrl, ClientId, ClientSecret, EndpointNotSet, EndpointSet, RedirectUrl, TokenUrl,
    basic::BasicClient,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{net::IpAddr, sync::Arc, time::Duration};
use url::Url;
use uuid::Uuid;
use webauthn_rs::{
    Webauthn, WebauthnBuilder,
    prelude::{
        CreationChallengeResponse, CredentialID, DiscoverableAuthentication, DiscoverableKey,
        Passkey, PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential,
        RequestChallengeResponse,
    },
};
use webauthn_rs_core::{
    WebauthnCore,
    proto::{
        AttestationConveyancePreference, AuthenticationResult, AuthenticationState, COSEAlgorithm,
        CredProtect, Credential, CredentialProtectionPolicy, Mediation, RegistrationState,
        RequestAuthenticationExtensions, RequestRegistrationExtensions, UserVerificationPolicy,
    },
};

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
    pub webauthn: ConfiguredWebauthn,
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

#[derive(Debug, Clone)]
pub enum ConfiguredWebauthn {
    Standard(Webauthn),
    Loopback(LoopbackWebauthn),
}

#[derive(Debug, Clone)]
pub struct LoopbackWebauthn {
    core: WebauthnCore,
    algorithms: Vec<COSEAlgorithm>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PasskeyRegistrationSerde {
    rs: RegistrationState,
}

#[derive(Debug, Serialize, Deserialize)]
struct DiscoverableAuthenticationSerde {
    ast: AuthenticationState,
}

#[derive(Debug, Serialize, Deserialize)]
struct CredentialSerde {
    cred: Credential,
}

impl ConfiguredWebauthn {
    pub fn start_passkey_registration(
        &self,
        user_unique_id: Uuid,
        user_name: &str,
        user_display_name: &str,
        exclude_credentials: Option<Vec<CredentialID>>,
    ) -> Result<(CreationChallengeResponse, PasskeyRegistration)> {
        match self {
            Self::Standard(webauthn) => webauthn
                .start_passkey_registration(
                    user_unique_id,
                    user_name,
                    user_display_name,
                    exclude_credentials,
                )
                .context("failed to start passkey registration"),
            Self::Loopback(webauthn) => webauthn.start_passkey_registration(
                user_unique_id,
                user_name,
                user_display_name,
                exclude_credentials,
            ),
        }
    }

    pub fn finish_passkey_registration(
        &self,
        reg: &RegisterPublicKeyCredential,
        state: &PasskeyRegistration,
    ) -> Result<Passkey> {
        match self {
            Self::Standard(webauthn) => webauthn
                .finish_passkey_registration(reg, state)
                .context("failed to finish passkey registration"),
            Self::Loopback(webauthn) => webauthn.finish_passkey_registration(reg, state),
        }
    }

    pub fn start_discoverable_authentication(
        &self,
    ) -> Result<(RequestChallengeResponse, DiscoverableAuthentication)> {
        match self {
            Self::Standard(webauthn) => webauthn
                .start_discoverable_authentication()
                .context("failed to start discoverable authentication"),
            Self::Loopback(webauthn) => webauthn.start_discoverable_authentication(),
        }
    }

    pub fn identify_discoverable_authentication<'a>(
        &self,
        reg: &'a PublicKeyCredential,
    ) -> Result<(Uuid, &'a [u8])> {
        match self {
            Self::Standard(webauthn) => webauthn
                .identify_discoverable_authentication(reg)
                .context("failed to identify discoverable authentication"),
            Self::Loopback(_) => {
                let credential_id = reg.get_credential_id();
                let user_unique_id = reg
                    .get_user_unique_id()
                    .and_then(|bytes| Uuid::from_slice(bytes).ok())
                    .ok_or_else(|| anyhow!("invalid discoverable passkey user handle"))?;
                Ok((user_unique_id, credential_id))
            }
        }
    }

    pub fn finish_discoverable_authentication(
        &self,
        reg: &PublicKeyCredential,
        state: DiscoverableAuthentication,
        creds: &[DiscoverableKey],
    ) -> Result<AuthenticationResult> {
        match self {
            Self::Standard(webauthn) => webauthn
                .finish_discoverable_authentication(reg, state, creds)
                .context("failed to finish discoverable authentication"),
            Self::Loopback(webauthn) => {
                webauthn.finish_discoverable_authentication(reg, state, creds)
            }
        }
    }
}

impl LoopbackWebauthn {
    fn new(rp_id: &str, rp_origin: &Url) -> Self {
        Self {
            core: WebauthnCore::new_unsafe_experts_only(
                "OctoRill",
                rp_id,
                vec![rp_origin.to_owned()],
                Duration::from_secs(300),
                Some(false),
                Some(false),
            ),
            algorithms: COSEAlgorithm::secure_algs(),
        }
    }

    fn start_passkey_registration(
        &self,
        user_unique_id: Uuid,
        user_name: &str,
        user_display_name: &str,
        exclude_credentials: Option<Vec<CredentialID>>,
    ) -> Result<(CreationChallengeResponse, PasskeyRegistration)> {
        let extensions = Some(RequestRegistrationExtensions {
            cred_protect: Some(CredProtect {
                credential_protection_policy: CredentialProtectionPolicy::UserVerificationRequired,
                enforce_credential_protection_policy: Some(false),
            }),
            uvm: Some(true),
            cred_props: Some(true),
            min_pin_length: None,
            hmac_create_secret: None,
        });

        let builder = self
            .core
            .new_challenge_register_builder(user_unique_id.as_bytes(), user_name, user_display_name)
            .context("failed to build loopback passkey registration challenge")?
            .attestation(AttestationConveyancePreference::None)
            .credential_algorithms(self.algorithms.clone())
            .require_resident_key(true)
            .authenticator_attachment(None)
            .user_verification_policy(UserVerificationPolicy::Required)
            .reject_synchronised_authenticators(false)
            .exclude_credentials(exclude_credentials)
            .hints(None)
            .extensions(extensions);

        let (challenge, registration_state) = self
            .core
            .generate_challenge_register(builder)
            .context("failed to generate loopback passkey registration challenge")?;

        Ok((
            challenge,
            passkey_registration_from_state(registration_state)?,
        ))
    }

    fn finish_passkey_registration(
        &self,
        reg: &RegisterPublicKeyCredential,
        state: &PasskeyRegistration,
    ) -> Result<Passkey> {
        let registration_state = registration_state_from_passkey_registration(state)?;
        let credential = self
            .core
            .register_credential(reg, &registration_state, None)
            .context("failed to verify loopback passkey registration")?;
        passkey_from_credential(credential)
    }

    fn start_discoverable_authentication(
        &self,
    ) -> Result<(RequestChallengeResponse, DiscoverableAuthentication)> {
        let extensions = Some(RequestAuthenticationExtensions {
            appid: None,
            uvm: Some(true),
            hmac_get_secret: None,
        });

        let builder = self
            .core
            .new_challenge_authenticate_builder(
                Vec::with_capacity(0),
                Some(UserVerificationPolicy::Required),
            )
            .context("failed to build loopback discoverable authentication challenge")?
            .extensions(extensions)
            .allow_backup_eligible_upgrade(false)
            .hints(None);

        let (mut challenge, authentication_state) = self
            .core
            .generate_challenge_authenticate(builder)
            .context("failed to generate loopback discoverable authentication challenge")?;
        challenge.mediation = Some(Mediation::Conditional);

        Ok((
            challenge,
            discoverable_authentication_from_state(authentication_state)?,
        ))
    }

    fn finish_discoverable_authentication(
        &self,
        reg: &PublicKeyCredential,
        state: DiscoverableAuthentication,
        creds: &[DiscoverableKey],
    ) -> Result<AuthenticationResult> {
        let mut authentication_state =
            authentication_state_from_discoverable_authentication(state)?;
        let credentials = creds
            .iter()
            .map(credential_from_discoverable_key)
            .collect::<Result<Vec<_>>>()?;
        authentication_state.set_allowed_credentials(credentials);

        self.core
            .authenticate_credential(reg, &authentication_state)
            .context("failed to verify loopback discoverable authentication")
    }
}

fn passkey_registration_from_state(state: RegistrationState) -> Result<PasskeyRegistration> {
    serde_json::from_value(serde_json::to_value(PasskeyRegistrationSerde {
        rs: state,
    })?)
    .context("failed to build passkey registration wrapper")
}

fn registration_state_from_passkey_registration(
    registration: &PasskeyRegistration,
) -> Result<RegistrationState> {
    serde_json::from_value::<PasskeyRegistrationSerde>(serde_json::to_value(registration)?)
        .map(|value| value.rs)
        .context("failed to decode passkey registration state")
}

fn discoverable_authentication_from_state(
    state: AuthenticationState,
) -> Result<DiscoverableAuthentication> {
    serde_json::from_value(serde_json::to_value(DiscoverableAuthenticationSerde {
        ast: state,
    })?)
    .context("failed to build discoverable authentication wrapper")
}

fn authentication_state_from_discoverable_authentication(
    authentication: DiscoverableAuthentication,
) -> Result<AuthenticationState> {
    serde_json::from_value::<DiscoverableAuthenticationSerde>(serde_json::to_value(authentication)?)
        .map(|value| value.ast)
        .context("failed to decode discoverable authentication state")
}

fn passkey_from_credential(credential: Credential) -> Result<Passkey> {
    serde_json::from_value(serde_json::to_value(CredentialSerde { cred: credential })?)
        .context("failed to build passkey wrapper")
}

fn credential_from_discoverable_key(key: &DiscoverableKey) -> Result<Credential> {
    serde_json::from_value::<CredentialSerde>(serde_json::to_value(key)?)
        .map(|value| value.cred)
        .context("failed to decode discoverable key")
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

pub fn build_webauthn(config: &AppConfig) -> Result<ConfiguredWebauthn> {
    let rp_origin = normalize_origin(&config.public_base_url)?;
    let rp_id = rp_origin
        .host_str()
        .context("failed to derive webauthn rp id from public base url")?;

    if is_loopback_host(rp_id) {
        return Ok(ConfiguredWebauthn::Loopback(LoopbackWebauthn::new(
            rp_id, &rp_origin,
        )));
    }

    WebauthnBuilder::new(rp_id, &rp_origin)
        .context("failed to initialize webauthn rp builder")?
        .rp_name("OctoRill")
        .build()
        .map(ConfiguredWebauthn::Standard)
        .context("failed to build webauthn")
}

pub fn normalize_origin(url: &Url) -> Result<Url> {
    let origin = url.origin().ascii_serialization();
    Url::parse(&origin).context("failed to normalize origin url")
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::{ConfiguredWebauthn, build_webauthn};
    use crate::{
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
    };
    use std::{net::SocketAddr, path::PathBuf};
    use url::Url;
    use uuid::Uuid;
    use webauthn_rs_core::proto::ResidentKeyRequirement;

    fn test_config(public_base_url: &str) -> AppConfig {
        AppConfig {
            bind_addr: "127.0.0.1:58090"
                .parse::<SocketAddr>()
                .expect("parse bind addr"),
            public_base_url: Url::parse(public_base_url).expect("parse public base url"),
            database_url: "sqlite::memory:".to_owned(),
            static_dir: None,
            task_log_dir: PathBuf::from("/tmp/octo-rill-state-tests"),
            job_worker_concurrency: 1,
            encryption_key: EncryptionKey::from_base64(
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            )
            .expect("build encryption key"),
            github: GitHubOAuthConfig {
                client_id: "github-client-id".to_owned(),
                client_secret: "github-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/github/callback")
                    .expect("parse github redirect url"),
            },
            linuxdo: None,
            ai: None,
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: "Asia/Shanghai".to_owned(),
        }
    }

    #[test]
    fn build_webauthn_uses_loopback_runtime_for_localhost() {
        let config = test_config("http://localhost:58090");
        let webauthn = build_webauthn(&config).expect("build webauthn");
        assert!(matches!(webauthn, ConfiguredWebauthn::Loopback(_)));
    }

    #[test]
    fn build_webauthn_uses_loopback_runtime_for_loopback_ip() {
        let config = test_config("http://127.0.0.1:58090");
        let webauthn = build_webauthn(&config).expect("build webauthn");
        assert!(matches!(webauthn, ConfiguredWebauthn::Loopback(_)));
    }

    #[test]
    fn build_webauthn_uses_standard_runtime_for_domain_hosts() {
        let config = test_config("https://app.example.com");
        let webauthn = build_webauthn(&config).expect("build webauthn");
        assert!(matches!(webauthn, ConfiguredWebauthn::Standard(_)));
    }

    #[test]
    fn loopback_passkey_registration_requires_discoverable_credentials() {
        let config = test_config("http://127.0.0.1:58090");
        let webauthn = build_webauthn(&config).expect("build webauthn");
        let ConfiguredWebauthn::Loopback(webauthn) = webauthn else {
            panic!("expected loopback webauthn runtime");
        };

        let (challenge, _) = webauthn
            .start_passkey_registration(Uuid::nil(), "passkey-user", "Passkey User", None)
            .expect("start loopback passkey registration");

        let selection = challenge
            .public_key
            .authenticator_selection
            .expect("loopback challenge should include authenticator selection");

        assert_eq!(
            selection.resident_key,
            Some(ResidentKeyRequirement::Required)
        );
        assert!(selection.require_resident_key);
    }
}
