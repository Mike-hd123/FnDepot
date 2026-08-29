//! Codex-specific local OAuth and `auth.json` helpers.
//!
//! This module deliberately does not implement [`crate::auth_provider::Provider`]: the
//! backend-api implementation owns that complete trait implementation (T4), while it
//! reuses these login/import/refresh primitives.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use oauth2::{
    basic::{BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenType},
    reqwest as oauth_reqwest, AuthType, AuthUrl, AuthorizationCode, Client, ClientId, CsrfToken,
    EmptyExtraTokenFields, ExtraTokenFields, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    RefreshToken, Scope, StandardRevocableToken, StandardTokenIntrospectionResponse,
    StandardTokenResponse, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{oneshot, watch, Mutex};

use super::{LoginResult, ProviderError, ProviderPayload, RefreshedPayload};

pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const CODEX_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const CODEX_IMPORT_NOTICE: &str = "本机 Codex 仍维持原登录态，双方 token 不自动同步。";
/// Registered loopback redirects used by the official Codex CLI.
const CODEX_CALLBACK_PORT: u16 = 1455;
const CODEX_CALLBACK_FALLBACK_PORT: u16 = 1457;

/// OAuth and token endpoints are configurable only to support local offline tests.
/// Production callers must use [`CodexLogin::new`].
#[derive(Clone)]
pub struct CodexLogin {
    authorize_url: String,
    token_url: String,
    timeout: Duration,
    callback_ports: (u16, u16),
}

/// Production browser opener. Commands construct this from their `AppHandle`; tests inject a
/// small [`super::LoginRuntime`] instead, so OAuth coverage never opens a real browser.
#[derive(Clone)]
pub struct TauriLoginRuntime {
    app: tauri::AppHandle,
}

impl TauriLoginRuntime {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl super::LoginRuntime for TauriLoginRuntime {
    async fn open_browser(&self, url: &str) -> Result<(), ProviderError> {
        use tauri_plugin_opener::OpenerExt;

        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|_| ProviderError::LoginFailed)
    }

    async fn set_step(&self, _step: super::LoginStep) {}

    async fn present_device_authorization(
        &self,
        _verification_url: &str,
        _user_code: &str,
        _expires_at: Option<String>,
    ) -> Result<(), ProviderError> {
        // Codex uses loopback callback authorization and has no device code.
        Err(ProviderError::LoginFailed)
    }

    fn is_cancelled(&self) -> bool {
        false
    }

    async fn cancelled(&self) {
        std::future::pending::<()>().await;
    }
}

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
struct CodexTokenFields {
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

impl ExtraTokenFields for CodexTokenFields {}

type CodexOauthClient = Client<
    BasicErrorResponse,
    StandardTokenResponse<CodexTokenFields, BasicTokenType>,
    StandardTokenIntrospectionResponse<EmptyExtraTokenFields, BasicTokenType>,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

impl Default for CodexLogin {
    fn default() -> Self {
        Self::new()
    }
}

/// Abstraction over the two cancel sources the Codex flow can select on: the
/// command-layer `watch` receiver (legacy/tests) and the generic `LoginRuntime`
/// cancel surface that device-flow providers share.
#[async_trait::async_trait]
trait LoginCancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
    async fn wait_cancelled(&self);
}

struct ReceiverCancel<'a> {
    cancellation: &'a mut watch::Receiver<bool>,
}

#[async_trait::async_trait]
impl LoginCancellation for ReceiverCancel<'_> {
    fn is_cancelled(&self) -> bool {
        *self.cancellation.borrow()
    }
    async fn wait_cancelled(&self) {
        let mut receiver = self.cancellation.clone();
        let _ = receiver.changed().await;
    }
}

struct RuntimeCancel<'a> {
    runtime: &'a dyn super::LoginRuntime,
}

#[async_trait::async_trait]
impl LoginCancellation for RuntimeCancel<'_> {
    fn is_cancelled(&self) -> bool {
        self.runtime.is_cancelled()
    }
    async fn wait_cancelled(&self) {
        self.runtime.cancelled().await;
    }
}

impl CodexLogin {
    pub fn new() -> Self {
        Self {
            authorize_url: CODEX_AUTHORIZE_URL.to_owned(),
            token_url: CODEX_TOKEN_URL.to_owned(),
            timeout: Duration::from_secs(5 * 60),
            callback_ports: (CODEX_CALLBACK_PORT, CODEX_CALLBACK_FALLBACK_PORT),
        }
    }

    pub fn with_endpoints(authorize_url: impl Into<String>, token_url: impl Into<String>) -> Self {
        Self {
            authorize_url: authorize_url.into(),
            token_url: token_url.into(),
            timeout: Duration::from_secs(5 * 60),
            callback_ports: (CODEX_CALLBACK_PORT, CODEX_CALLBACK_FALLBACK_PORT),
        }
    }

    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[cfg(test)]
    fn with_callback_ports(mut self, primary: u16, fallback: u16) -> Self {
        self.callback_ports = (primary, fallback);
        self
    }

    /// Build the exact browser URL from a caller-provided callback/state/verifier.
    /// Public only so the provider integration can expose the same deterministic PKCE flow.
    pub fn authorization_url(&self, redirect_uri: &str, state: &str, verifier: &str) -> String {
        let client = self.oauth_client(redirect_uri);
        let (url, _) = client
            .authorize_url(|| CsrfToken::new(state.to_owned()))
            .add_scope(Scope::new("openid".to_owned()))
            .add_scope(Scope::new("profile".to_owned()))
            .add_scope(Scope::new("email".to_owned()))
            .add_scope(Scope::new("offline_access".to_owned()))
            .add_scope(Scope::new("api.connectors.read".to_owned()))
            .add_scope(Scope::new("api.connectors.invoke".to_owned()))
            .add_extra_param("id_token_add_organizations", "true")
            .add_extra_param("codex_cli_simplified_flow", "true")
            .add_extra_param("originator", "codex_cli_rs")
            .set_pkce_challenge(PkceCodeChallenge::from_code_verifier_sha256(
                &PkceCodeVerifier::new(verifier.to_owned()),
            ))
            .url();
        url.to_string()
    }

    /// Start a one-shot loopback listener before opening the browser, then exchange exactly
    /// one matching callback code. The spawned server is aborted on every result path.
    ///
    /// Cancellation comes from the runtime, so the command layer never needs to
    /// hold the receiver itself.
    pub async fn login(
        &self,
        runtime: &dyn super::LoginRuntime,
    ) -> Result<LoginResult, ProviderError> {
        let cancel = RuntimeCancel { runtime };
        self.login_flow(runtime, &cancel).await
    }

    /// The command layer owns the cancellation sender, while this method owns
    /// the loopback server.  Selecting on the signal makes cancel release the
    /// listener immediately instead of merely abandoning the UI promise.
    pub async fn login_cancellable(
        &self,
        runtime: &dyn super::LoginRuntime,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<LoginResult, ProviderError> {
        let cancel = ReceiverCancel { cancellation };
        self.login_flow(runtime, &cancel).await
    }

    async fn login_flow(
        &self,
        runtime: &dyn super::LoginRuntime,
        cancel: &(dyn LoginCancellation + Sync),
    ) -> Result<LoginResult, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::LoginCancelled);
        }
        runtime.set_step(super::LoginStep::Preparing).await;
        let listener = bind_callback_listener(self.callback_ports)
            .await
            .map_err(|_| ProviderError::LoginFailed)?;
        let port = listener
            .local_addr()
            .map_err(|_| ProviderError::LoginFailed)?
            .port();
        let redirect_uri = format!("http://localhost:{port}/auth/callback");
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let state = CsrfToken::new_random();
        let callback_state = CallbackState::new(state.secret().to_owned());
        let app = Router::new()
            .route("/auth/callback", get(oauth_callback))
            .with_state(callback_state.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = self.oauth_client(&redirect_uri);
        let (browser_url, _) = client
            .authorize_url(|| state.clone())
            .add_scope(Scope::new("openid".to_owned()))
            .add_scope(Scope::new("profile".to_owned()))
            .add_scope(Scope::new("email".to_owned()))
            .add_scope(Scope::new("offline_access".to_owned()))
            .add_scope(Scope::new("api.connectors.read".to_owned()))
            .add_scope(Scope::new("api.connectors.invoke".to_owned()))
            .add_extra_param("id_token_add_organizations", "true")
            .add_extra_param("codex_cli_simplified_flow", "true")
            .add_extra_param("originator", "codex_cli_rs")
            .set_pkce_challenge(pkce_challenge)
            .url();

        let result = async {
            runtime.set_step(super::LoginStep::Authorizing).await;
            runtime
                .open_browser(browser_url.as_str())
                .await
                .map_err(|_| ProviderError::BrowserOpenFailed)?;
            if cancel.is_cancelled() {
                return Err(ProviderError::LoginCancelled);
            }
            runtime.set_step(super::LoginStep::Waiting).await;
            let callback = tokio::select! {
                _ = cancel.wait_cancelled() => Err(ProviderError::LoginCancelled),
                callback = tokio::time::timeout(self.timeout, callback_state.receive()) => {
                    callback.map_err(|_| ProviderError::LoginTimeout)?
                }
            }?;
            if cancel.is_cancelled() {
                return Err(ProviderError::LoginCancelled);
            }
            runtime.set_step(super::LoginStep::Exchanging).await;
            tokio::select! {
                _ = cancel.wait_cancelled() => Err(ProviderError::LoginCancelled),
                result = self.exchange_code(&redirect_uri, callback, pkce_verifier) => result,
            }
        }
        .await;
        server.abort();
        let _ = server.await;
        result
    }

    /// Parse the real, nested Codex CLI file. An expired JWT access token is refreshed before
    /// it becomes an import result; refresh tokens stay opaque strings throughout.
    pub async fn import_auth_json(&self, bytes: &[u8]) -> Result<LoginResult, ProviderError> {
        self.import_auth_json_with_format(bytes, AuthFileFormat::Codex)
            .await
    }

    /// Format-aware variant of [`Self::import_auth_json`].  `sub2api` files are
    /// multi-account; use [`Self::import_codex_accounts`] for them.
    pub async fn import_auth_json_with_format(
        &self,
        bytes: &[u8],
        format: AuthFileFormat,
    ) -> Result<LoginResult, ProviderError> {
        let parsed: Value =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::ImportFailed)?;
        let (payload, last_refresh) = match format {
            AuthFileFormat::Codex => {
                let last_refresh = parsed
                    .get("last_refresh")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                (parse_auth_json(&parsed)?, last_refresh)
            }
            AuthFileFormat::Cpa => {
                // CLIProxyAPI keeps its own bookkeeping in `last_refresh`; both
                // formats agree on that field name.
                let last_refresh = parsed
                    .get("last_refresh")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                (parse_cpa_auth_json(&parsed)?, last_refresh)
            }
            AuthFileFormat::Sub2api => {
                return Err(ProviderError::ImportFailed);
            }
        };
        self.import_codex_account(CodexAccountPayload { label: None, payload })
            .await
            .map(|result| LoginResult {
                last_refreshed_at: last_refresh.or(result.last_refreshed_at),
                ..result
            })
    }

    /// Shared tail of every import path: refresh an expired token, then turn
    /// the normalized payload into a login result.
    pub async fn import_codex_account(
        &self,
        account: CodexAccountPayload,
    ) -> Result<LoginResult, ProviderError> {
        let (payload, refreshed_at) = if token_expired(&account.payload) {
            let refreshed = self.refresh_payload(&account.payload).await?;
            (refreshed.payload, refreshed.last_refreshed_at)
        } else {
            (account.payload, None)
        };
        login_result_from_payload(payload, refreshed_at)
    }

    /// Extract every Codex-usable account from a sub2api admin-data export.
    /// Returns the importable accounts plus the total entry count so the
    /// caller can report how many were skipped (non-Codex platforms,
    /// credential-less entries, and per-account failures).
    pub fn parse_sub2api_accounts(
        bytes: &[u8],
    ) -> Result<(Vec<CodexAccountPayload>, usize), ProviderError> {
        let parsed: Value =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::ImportFailed)?;
        let total = parsed
            .get("accounts")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let accounts = sub2api_codex_accounts(&parsed)?;
        Ok((accounts, total))
    }

    /// Refresh a token using the OAuth crate. ID/account information is retained from the
    /// persisted payload because OAuth refresh responses need not repeat either field.
    pub async fn refresh_payload(
        &self,
        payload: &ProviderPayload,
    ) -> Result<RefreshedPayload, ProviderError> {
        let refresh = required_string(
            payload.as_value(),
            "refresh_token",
            ProviderError::InvalidPayload,
        )?;
        let client = self.oauth_client("http://localhost/unused");
        let http_client = oauth_http_client()?;
        let response = client
            .exchange_refresh_token(&RefreshToken::new(refresh))
            .request_async(&http_client)
            .await
            .map_err(|_| ProviderError::Unauthorized)?;
        let access_token = response.access_token().secret().to_owned();
        let refresh_token = response
            .refresh_token()
            .map(|token| token.secret().to_owned())
            .or_else(|| {
                payload
                    .as_value()
                    .get("refresh_token")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .ok_or(ProviderError::InvalidPayload)?;
        let id_token = required_string(
            payload.as_value(),
            "id_token",
            ProviderError::InvalidPayload,
        )?;
        let account_id = required_string(
            payload.as_value(),
            "account_id",
            ProviderError::InvalidPayload,
        )?;
        let expires_at = expires_at_from_response(response.expires_in(), &access_token);
        Ok(RefreshedPayload {
            payload: ProviderPayload::new(json!({
                "version": 1,
                "access_token": access_token,
                "refresh_token": refresh_token,
                "id_token": id_token,
                "account_id": account_id,
                "expires_at": expires_at,
            })),
            last_refreshed_at: Some(Utc::now().to_rfc3339()),
            next_refresh_after: None,
            next_retry_after: None,
        })
    }

    /// Return the Codex CLI location without reading secrets. `$CODEX_HOME` takes precedence.
    pub fn default_auth_json_path() -> Result<PathBuf, ProviderError> {
        if let Some(home) = std::env::var_os("CODEX_HOME") {
            return Ok(PathBuf::from(home).join("auth.json"));
        }
        dirs::home_dir()
            .map(|path| path.join(".codex").join("auth.json"))
            .ok_or(ProviderError::Storage)
    }

    /// Atomically export a Codex auth JSON file. The destination's directory is
    /// never created implicitly; an existing destination is backed up first.
    pub fn write_auth_json(
        path: &Path,
        payload: &ProviderPayload,
    ) -> Result<AuthJsonWriteResult, ProviderError> {
        write_auth_json_with_rename(path, payload, |from, to| fs::rename(from, to))
    }

    fn oauth_client(&self, redirect_uri: &str) -> CodexOauthClient {
        Client::new(ClientId::new(CODEX_CLIENT_ID.to_owned()))
            .set_auth_type(AuthType::RequestBody)
            .set_auth_uri(
                AuthUrl::new(self.authorize_url.clone())
                    .expect("configured OAuth authorization URL"),
            )
            .set_token_uri(
                TokenUrl::new(self.token_url.clone()).expect("configured OAuth token URL"),
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_uri.to_owned()).expect("loopback redirect URL"),
            )
    }

    async fn exchange_code(
        &self,
        redirect_uri: &str,
        code: String,
        verifier: PkceCodeVerifier,
    ) -> Result<LoginResult, ProviderError> {
        // The OAuth crate performs the sole code exchange. The standard response preserves
        // access/refresh fields; Codex account metadata is carried in its JWT id token.
        let http_client = oauth_http_client()?;
        let response = self
            .oauth_client(redirect_uri)
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&http_client)
            .await
            .map_err(|_| ProviderError::TokenExchangeFailed)?;

        let id_token = response
            .extra_fields()
            .id_token
            .clone()
            .ok_or(ProviderError::TokenExchangeFailed)?;
        let account_id = response
            .extra_fields()
            .account_id
            .clone()
            .or_else(|| account_id_from_claims(&id_token))
            .ok_or(ProviderError::TokenExchangeFailed)?;
        let refresh_token = response
            .refresh_token()
            .map(|token| token.secret().to_owned())
            .ok_or(ProviderError::TokenExchangeFailed)?;
        let payload = ProviderPayload::new(json!({
            "version": 1,
            "access_token": response.access_token().secret(),
            "refresh_token": refresh_token,
            "id_token": id_token,
            "account_id": account_id,
            "expires_at": expires_at_from_response(response.expires_in(), response.access_token().secret()),
        }));
        login_result_from_payload(payload, Some(Utc::now().to_rfc3339()))
    }
}

async fn bind_callback_listener(ports: (u16, u16)) -> io::Result<tokio::net::TcpListener> {
    match tokio::net::TcpListener::bind(("127.0.0.1", ports.0)).await {
        Ok(listener) => Ok(listener),
        Err(_) if ports.1 != ports.0 => tokio::net::TcpListener::bind(("127.0.0.1", ports.1)).await,
        Err(error) => Err(error),
    }
}

fn oauth_http_client() -> Result<oauth_reqwest::Client, ProviderError> {
    oauth_reqwest::ClientBuilder::new()
        .redirect(oauth_reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ProviderError::LoginFailed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthJsonWriteResult {
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

#[derive(Clone)]
struct CallbackState {
    expected_state: String,
    used: Arc<AtomicBool>,
    sender: Arc<Mutex<Option<oneshot::Sender<Result<String, ProviderError>>>>>,
    receiver: Arc<Mutex<Option<oneshot::Receiver<Result<String, ProviderError>>>>>,
}

impl CallbackState {
    fn new(expected_state: String) -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            expected_state,
            used: Arc::new(AtomicBool::new(false)),
            sender: Arc::new(Mutex::new(Some(sender))),
            receiver: Arc::new(Mutex::new(Some(receiver))),
        }
    }

    async fn receive(&self) -> Result<String, ProviderError> {
        let receiver = self
            .receiver
            .lock()
            .await
            .take()
            .ok_or(ProviderError::LoginFailed)?;
        receiver.await.map_err(|_| ProviderError::LoginFailed)?
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn oauth_callback(
    State(callback): State<CallbackState>,
    Query(query): Query<CallbackQuery>,
) -> impl IntoResponse {
    if query.state.as_deref() != Some(callback.expected_state.as_str()) {
        return (StatusCode::BAD_REQUEST, "invalid OAuth state");
    }
    if callback.used.swap(true, Ordering::SeqCst) {
        return (StatusCode::CONFLICT, "OAuth callback already used");
    }
    let result = match (query.error, query.code) {
        (Some(_), _) => Err(ProviderError::CallbackFailed),
        (None, Some(code)) if !code.is_empty() => Ok(code),
        _ => Err(ProviderError::CallbackFailed),
    };
    if let Some(sender) = callback.sender.lock().await.take() {
        let _ = sender.send(result);
    }
    (StatusCode::OK, "Codex login complete; return to WaLiAPI.")
}

/// Detect a sub2api export document (`type: "sub2api-data"`).
fn is_sub2api_doc(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|ty| ty == "sub2api-data")
}

/// Parse a single sub2api account entry into a provider payload.
///
/// Field mapping: `account_id` is sourced from `credentials.chatgpt_account_id`
/// (sub2api's name for the same field). `expires_at` prefers the explicit
/// `credentials.expires_at` timestamp, falling back to the JWT `exp` claim.
fn parse_sub2api_account(account: &Value) -> Result<ProviderPayload, ProviderError> {
    let credentials = account
        .get("credentials")
        .ok_or(ProviderError::ImportFailed)?;
    let id_token = required_string(credentials, "id_token", ProviderError::ImportFailed)?;
    let access_token = required_string(credentials, "access_token", ProviderError::ImportFailed)?;
    let refresh_token = required_string(credentials, "refresh_token", ProviderError::ImportFailed)?;
    let account_id = required_string(
        credentials,
        "chatgpt_account_id",
        ProviderError::ImportFailed,
    )?;
    let expires_at = credentials
        .get("expires_at")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            expires_at_from_jwt(
                credentials
                    .get("access_token")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
        });
    let mut payload = json!({
        "version": 1,
        "id_token": id_token,
        "access_token": access_token,
        "refresh_token": refresh_token,
        "account_id": account_id,
    });
    if let Some(expires_at) = expires_at {
        payload["expires_at"] = json!(expires_at);
    }
    Ok(ProviderPayload::new(payload))
}

fn parse_auth_json(value: &Value) -> Result<ProviderPayload, ProviderError> {
    // sub2api exports carry multiple accounts; import the first one here. The
    // command layer splits the full document so every account is imported.
    if is_sub2api_doc(value) {
        let accounts = value.get("accounts").and_then(Value::as_array);
        let account = accounts
            .and_then(|accounts| accounts.first())
            .ok_or(ProviderError::ImportFailed)?;
        return parse_sub2api_account(account);
    }
    let auth_mode = required_string(value, "auth_mode", ProviderError::ImportFailed)?;
    if auth_mode != "chatgpt" || !value.get("OPENAI_API_KEY").is_some() {
        return Err(ProviderError::ImportFailed);
    }
    let _last_refresh = required_string(value, "last_refresh", ProviderError::ImportFailed)?;
    let tokens = value.get("tokens").ok_or(ProviderError::ImportFailed)?;
    codex_payload_from_tokens(tokens)
}

/// CLIProxyAPI (`auths/*.json`) stores the same Codex token set flattened to
/// the top level.  `type` must say `codex`; its extra bookkeeping fields
/// (`email`, `expired`) are ignored.
fn parse_cpa_auth_json(value: &Value) -> Result<ProviderPayload, ProviderError> {
    if value.get("type").and_then(Value::as_str) != Some("codex") {
        return Err(ProviderError::ImportFailed);
    }
    codex_payload_from_tokens(value)
}

/// One normalized Codex credential set extracted from an auth file, with the
/// optional display label the source format carries (sub2api account names).
#[derive(Debug)]
pub struct CodexAccountPayload {
    pub label: Option<String>,
    pub payload: ProviderPayload,
}

/// Supported third-party auth-file layouts for the import dropdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthFileFormat {
    /// Native Codex CLI `auth.json` (nested under `tokens`).
    Codex,
    /// CLIProxyAPI flat token file (`auths/*.json`, `type: "codex"`).
    Cpa,
    /// sub2api admin-data export (multi-account, `accounts[].platform`).
    Sub2api,
}

impl AuthFileFormat {
    pub fn from_name(name: &str) -> Result<Self, ProviderError> {
        match name.trim().to_ascii_lowercase().as_str() {
            "codex" => Ok(Self::Codex),
            "cpa" => Ok(Self::Cpa),
            "sub2api" => Ok(Self::Sub2api),
            _ => Err(ProviderError::ImportFailed),
        }
    }
}

fn codex_payload_from_tokens(tokens: &Value) -> Result<ProviderPayload, ProviderError> {
    let id_token = required_string(tokens, "id_token", ProviderError::ImportFailed)?;
    let access_token = required_string(tokens, "access_token", ProviderError::ImportFailed)?;
    let refresh_token = required_string(tokens, "refresh_token", ProviderError::ImportFailed)?;
    let account_id = required_string(tokens, "account_id", ProviderError::ImportFailed)?;
    Ok(ProviderPayload::new(json!({
        "version": 1,
        "id_token": id_token,
        "access_token": access_token,
        "refresh_token": refresh_token,
        "account_id": account_id,
        "expires_at": expires_at_from_jwt(tokens.get("access_token").and_then(Value::as_str).unwrap_or_default()),
    })))
}

/// Extract every importable Codex account from a parsed sub2api admin-data
/// export.  Only `platform == "openai"` accounts whose credentials carry the
/// full four-field token set qualify; everything else is skipped (the command
/// layer surfaces the skip count).
fn sub2api_codex_accounts(
    value: &Value,
) -> Result<Vec<CodexAccountPayload>, ProviderError> {
    let mut accounts = Vec::new();
    for entry in value.get("accounts").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        if entry.get("platform").and_then(Value::as_str) != Some("openai") {
            continue;
        }
        let credentials = match entry.get("credentials").and_then(Value::as_object) {
            Some(credentials) => credentials,
            None => continue,
        };
        // sub2api stores the ChatGPT account id as `chatgpt_account_id`
        // (openai_oauth_service.go); normalize it to the `account_id` key the
        // shared token extractor expects.  An explicit `account_id` wins.
        let mut normalized = credentials.clone();
        if !normalized.contains_key("account_id") {
            if let Some(id) = normalized.get("chatgpt_account_id") {
                normalized.insert("account_id".to_owned(), id.clone());
            }
        }
        let Ok(payload) = codex_payload_from_tokens(&Value::Object(normalized)) else {
            continue;
        };
        let label = entry
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned);
        accounts.push(CodexAccountPayload { label, payload });
    }
    Ok(accounts)
}

fn login_result_from_payload(
    payload: ProviderPayload,
    last_refreshed_at: Option<String>,
) -> Result<LoginResult, ProviderError> {
    let account_id = required_string(
        payload.as_value(),
        "account_id",
        ProviderError::InvalidPayload,
    )?;
    let id_token = required_string(
        payload.as_value(),
        "id_token",
        ProviderError::InvalidPayload,
    )?;
    let claims = jwt_claims(&id_token).unwrap_or(Value::Null);
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let plan_type = claims
        .get("plan_type")
        .or_else(|| claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let label = email.clone().unwrap_or_else(|| "ChatGPT".to_owned());
    Ok(LoginResult {
        account_id,
        label,
        attributes: json!({"email": email, "plan_type": plan_type}),
        payload,
        last_refreshed_at,
        next_refresh_after: None,
        next_retry_after: None,
    })
}

fn token_expired(payload: &ProviderPayload) -> bool {
    payload
        .expires_at()
        .is_some_and(|expires_at| expires_at <= Utc::now())
}

fn expires_at_from_response(expires_in: Option<Duration>, access_token: &str) -> String {
    expires_at_from_jwt(access_token).unwrap_or_else(|| {
        let ttl = expires_in
            .and_then(|duration| ChronoDuration::from_std(duration).ok())
            .unwrap_or_else(|| ChronoDuration::minutes(5));
        (Utc::now() + ttl).to_rfc3339()
    })
}

fn expires_at_from_jwt(token: &str) -> Option<String> {
    let exp = jwt_claims(token)?.get("exp")?.as_i64()?;
    DateTime::from_timestamp(exp, 0).map(|value| value.to_rfc3339())
}

fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn account_id_from_claims(id_token: &str) -> Option<String> {
    let claims = jwt_claims(id_token)?;
    claims
        .get("account_id")
        .or_else(|| claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn required_string(
    value: &Value,
    key: &str,
    error: ProviderError,
) -> Result<String, ProviderError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or(error)
}

/// Serialize the Codex auth.json document for an account payload.
/// Shared by the atomic file exporter and the Web 管理面板 content export.
fn encode_auth_json(payload: &ProviderPayload) -> Result<Vec<u8>, ProviderError> {
    let id_token = required_string(
        payload.as_value(),
        "id_token",
        ProviderError::InvalidPayload,
    )?;
    let access_token = required_string(
        payload.as_value(),
        "access_token",
        ProviderError::InvalidPayload,
    )?;
    let refresh_token = required_string(
        payload.as_value(),
        "refresh_token",
        ProviderError::InvalidPayload,
    )?;
    let account_id = required_string(
        payload.as_value(),
        "account_id",
        ProviderError::InvalidPayload,
    )?;
    serde_json::to_vec_pretty(&json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": Value::Null,
        "tokens": {
            "id_token": id_token,
            "access_token": access_token,
            "refresh_token": refresh_token,
            "account_id": account_id,
        },
        "last_refresh": Utc::now().to_rfc3339(),
    }))
    .map_err(|_| ProviderError::Storage)
}

impl CodexLogin {
    /// Web 管理面板用：导出 auth.json 内容（不落盘）。
    pub fn export_auth_json_content(payload: &ProviderPayload) -> Result<String, ProviderError> {
        let bytes = encode_auth_json(payload)?;
        String::from_utf8(bytes).map_err(|_| ProviderError::Storage)
    }
}

fn write_auth_json_with_rename<F>(
    path: &Path,
    payload: &ProviderPayload,
    rename: F,
) -> Result<AuthJsonWriteResult, ProviderError>
where
    F: Fn(&Path, &Path) -> io::Result<()>,
{
    let parent = path
        .parent()
        .filter(|parent| parent.is_dir())
        .ok_or(ProviderError::Storage)?;
    let encoded = encode_auth_json(payload)?;
    let stamp = Utc::now().format("%Y%m%d%H%M%S%f");
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ProviderError::Storage)?;
    let backup_path = path
        .exists()
        .then(|| parent.join(format!("{filename}.bak-{stamp}")));
    let temporary_path = parent.join(format!(".{filename}.tmp-{stamp}"));

    if let Some(backup_path) = &backup_path {
        fs::copy(path, backup_path).map_err(|_| ProviderError::Storage)?;
        set_private_permissions(backup_path).map_err(|_| ProviderError::Storage)?;
    }
    let write_result = (|| -> io::Result<()> {
        let mut temporary = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        temporary.write_all(&encoded)?;
        temporary.sync_all()?;
        set_private_permissions(&temporary_path)?;
        rename(&temporary_path, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
        return Err(ProviderError::Storage);
    }
    Ok(AuthJsonWriteResult {
        path: path.to_owned(),
        backup_path,
    })
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_permissions(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    use axum::{
        extract::{Form, State},
        routing::post,
    };
    use serde_json::json;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    const ACCESS: &str = "fixture-access-token";
    const REFRESH: &str = "opaque-refresh-token-not-a-jwt";
    const ID: &str = "fixture-id-token";

    fn jwt(payload: Value) -> String {
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
        )
    }

    #[test]
    fn pkce_challenge_is_s256_and_auth_url_has_required_parameters() {
        let login = CodexLogin::new();
        // oauth2 correctly rejects PKCE verifiers shorter than RFC 7636's
        // 43-byte minimum.  Keep this fixture valid while testing the URL
        // construction, rather than bypassing the production PKCE type.
        let verifier = "v".repeat(43);
        let url =
            login.authorization_url("http://localhost:1455/auth/callback", "state", &verifier);
        let parsed = reqwest::Url::parse(&url).unwrap();
        let params = parsed.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(
            params
                .get("code_challenge_method")
                .map(|value| value.as_ref()),
            Some("S256")
        );
        assert_eq!(
            params.get("client_id").map(|value| value.as_ref()),
            Some(CODEX_CLIENT_ID)
        );
        assert_eq!(
            params.get("redirect_uri").map(|value| value.as_ref()),
            Some("http://localhost:1455/auth/callback")
        );
        assert_eq!(
            params.get("response_type").map(|value| value.as_ref()),
            Some("code")
        );
        assert_eq!(
            params.get("scope").map(|value| value.as_ref()),
            Some("openid profile email offline_access api.connectors.read api.connectors.invoke")
        );
        assert_eq!(
            params.get("prompt"),
            None,
            "official Codex URL has no prompt parameter"
        );
        assert_eq!(
            params.get("originator").map(|value| value.as_ref()),
            Some("codex_cli_rs")
        );
        let fallback_url =
            login.authorization_url("http://localhost:1457/auth/callback", "state", &verifier);
        let fallback = reqwest::Url::parse(&fallback_url).unwrap();
        assert_eq!(
            fallback
                .query_pairs()
                .find(|(key, _)| key == "redirect_uri")
                .map(|(_, value)| value.into_owned()),
            Some("http://localhost:1457/auth/callback".to_owned())
        );
        assert_eq!(
            params
                .get("id_token_add_organizations")
                .map(|value| value.as_ref()),
            Some("true")
        );
        assert_eq!(
            params
                .get("codex_cli_simplified_flow")
                .map(|value| value.as_ref()),
            Some("true")
        );
    }

    #[test]
    fn login_result_label_uses_account_name_without_provider_prefix() {
        let payload = ProviderPayload::new(json!({
            "id_token": jwt(json!({"email": "person@example.test", "plan_type": "plus"})),
            "access_token": ACCESS,
            "refresh_token": REFRESH,
            "account_id": "account-1"
        }));
        let result = login_result_from_payload(payload, None).unwrap();
        assert_eq!(result.label, "person@example.test");

        let payload_without_email = ProviderPayload::new(json!({
            "id_token": jwt(json!({"plan_type": "plus"})),
            "access_token": ACCESS,
            "refresh_token": REFRESH,
            "account_id": "account-1"
        }));
        let result_without_email = login_result_from_payload(payload_without_email, None).unwrap();
        assert_eq!(result_without_email.label, "ChatGPT");
    }

    #[derive(Clone)]
    struct MockState {
        token_hits: Arc<AtomicUsize>,
    }

    async fn token_handler(
        State(state): State<MockState>,
        Form(form): Form<HashMap<String, String>>,
    ) -> axum::Json<Value> {
        state.token_hits.fetch_add(1, Ordering::SeqCst);
        assert!(matches!(
            form.get("grant_type").map(String::as_str),
            Some("authorization_code") | Some("refresh_token")
        ));
        assert_eq!(
            form.get("client_id").map(String::as_str),
            Some(CODEX_CLIENT_ID)
        );
        axum::Json(json!({
            "access_token": jwt(json!({"exp": 4_102_444_800_i64})),
            "refresh_token": REFRESH,
            "id_token": jwt(json!({"email": "person@example.test", "plan_type": "plus"})),
            "account_id": "account-1",
            "token_type": "Bearer"
        }))
    }

    async fn mock_oauth() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/token", post(token_handler))
            .with_state(MockState {
                token_hits: hits.clone(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/token"), hits, server)
    }

    struct CallbackRuntime;
    #[async_trait::async_trait]
    impl super::super::LoginRuntime for CallbackRuntime {
        async fn open_browser(&self, url: &str) -> Result<(), ProviderError> {
            let parsed = reqwest::Url::parse(url).unwrap();
            let params = parsed.query_pairs().collect::<HashMap<_, _>>();
            assert_eq!(params.get("prompt"), None);
            assert_eq!(
                params
                    .get("id_token_add_organizations")
                    .map(|value| value.as_ref()),
                Some("true")
            );
            assert_eq!(
                params
                    .get("codex_cli_simplified_flow")
                    .map(|value| value.as_ref()),
                Some("true")
            );
            assert_eq!(
                params.get("originator").map(|value| value.as_ref()),
                Some("codex_cli_rs")
            );
            let redirect = parsed
                .query_pairs()
                .find(|(key, _)| key == "redirect_uri")
                .unwrap()
                .1
                .to_string();
            let state = parsed
                .query_pairs()
                .find(|(key, _)| key == "state")
                .unwrap()
                .1
                .to_string();
            let client = reqwest::Client::new();
            let wrong = format!("{redirect}?code=wrong&state=wrong");
            assert_eq!(
                client.get(wrong).send().await.unwrap().status(),
                StatusCode::BAD_REQUEST
            );
            let correct = format!("{redirect}?code=right&state={state}");
            assert_eq!(
                client.get(correct).send().await.unwrap().status(),
                StatusCode::OK
            );
            let second = format!("{redirect}?code=second&state={state}");
            assert_eq!(
                client.get(second).send().await.unwrap().status(),
                StatusCode::CONFLICT
            );
            Ok(())
        }
        async fn set_step(&self, _step: super::super::LoginStep) {}
        async fn present_device_authorization(
            &self,
            _url: &str,
            _code: &str,
            _expires_at: Option<String>,
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        fn is_cancelled(&self) -> bool {
            false
        }
        async fn cancelled(&self) {
            std::future::pending::<()>().await;
        }
    }

    #[tokio::test]
    async fn callback_listens_before_browser_and_only_matching_state_exchanges_once() {
        let (token_url, hits, server) = mock_oauth().await;
        let login = CodexLogin::with_endpoints("http://127.0.0.1/authorize", token_url)
            .with_callback_ports(0, 0);
        let result = login.login(&CallbackRuntime).await.unwrap();
        assert_eq!(result.account_id, "account-1");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn timeout_releases_loopback_port() {
        #[derive(Default)]
        struct CaptureRuntime(Arc<Mutex<Option<String>>>);
        #[async_trait::async_trait]
        impl super::super::LoginRuntime for CaptureRuntime {
            async fn open_browser(&self, url: &str) -> Result<(), ProviderError> {
                *self.0.lock().await = Some(url.to_owned());
                Ok(())
            }
            async fn set_step(&self, _step: super::super::LoginStep) {}
            async fn present_device_authorization(
                &self,
                _url: &str,
                _code: &str,
                _expires_at: Option<String>,
            ) -> Result<(), ProviderError> {
                Ok(())
            }
            fn is_cancelled(&self) -> bool {
                false
            }
            async fn cancelled(&self) {
                std::future::pending::<()>().await;
            }
        }
        let capture = CaptureRuntime::default();
        let login = CodexLogin::new()
            .with_timeout(Duration::from_millis(10))
            .with_callback_ports(0, 0);
        assert_eq!(
            login.login(&capture).await.unwrap_err(),
            ProviderError::LoginTimeout
        );
        let url = capture.0.lock().await.take().unwrap();
        let parsed = reqwest::Url::parse(&url).unwrap();
        let redirect = parsed
            .query_pairs()
            .find(|(key, _)| key == "redirect_uri")
            .unwrap()
            .1
            .to_string();
        let redirect_url = reqwest::Url::parse(&redirect).unwrap();
        let port = redirect_url.port().unwrap();
        // The redirect deliberately uses `localhost`, which is a hostname and
        // therefore not parseable as a SocketAddr.  Rebinding loopback on the
        // captured port proves the one-shot listener was released.
        tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn callback_listener_falls_back_to_second_registered_port_only() {
        assert_eq!(CODEX_CALLBACK_PORT, 1455);
        assert_eq!(CODEX_CALLBACK_FALLBACK_PORT, 1457);

        // Use ephemeral test ports to avoid competing with a real Codex CLI,
        // while verifying the exact primary-then-fallback algorithm.
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let primary = occupied.local_addr().unwrap().port();
        let available = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fallback = available.local_addr().unwrap().port();
        drop(available);

        let listener = bind_callback_listener((primary, fallback)).await.unwrap();
        assert_eq!(listener.local_addr().unwrap().port(), fallback);
        drop(occupied);
    }

    #[tokio::test]
    async fn cancellation_aborts_listener_and_skips_token_exchange() {
        struct CancellingRuntime {
            cancellation: watch::Sender<bool>,
        }
        #[async_trait::async_trait]
        impl super::super::LoginRuntime for CancellingRuntime {
            async fn open_browser(&self, _: &str) -> Result<(), ProviderError> {
                let _ = self.cancellation.send(true);
                Ok(())
            }
            async fn set_step(&self, _step: super::super::LoginStep) {}
            async fn present_device_authorization(
                &self,
                _url: &str,
                _code: &str,
                _expires_at: Option<String>,
            ) -> Result<(), ProviderError> {
                Ok(())
            }
            fn is_cancelled(&self) -> bool {
                *self.cancellation.borrow()
            }
            async fn cancelled(&self) {
                let mut receiver = self.cancellation.subscribe();
                // Cancel is delivered through `send(true)`; wait for it.
                while !*receiver.borrow() {
                    if receiver.changed().await.is_err() {
                        return;
                    }
                }
            }
        }

        let (token_url, hits, server) = mock_oauth().await;
        let login = CodexLogin::with_endpoints("http://127.0.0.1/authorize", token_url)
            .with_callback_ports(0, 0);
        let (sender, mut receiver) = watch::channel(false);
        let result = login
            .login_cancellable(
                &CancellingRuntime {
                    cancellation: sender,
                },
                &mut receiver,
            )
            .await;
        assert_eq!(result.unwrap_err(), ProviderError::LoginCancelled);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "cancel must skip exchange");
        server.abort();
    }

    #[tokio::test]
    async fn nested_auth_json_import_keeps_opaque_refresh_and_refreshes_expired_access() {
        let (token_url, hits, server) = mock_oauth().await;
        let login = CodexLogin::with_endpoints("http://127.0.0.1/authorize", token_url);
        let expired = jwt(json!({"exp": 1, "email": "person@example.test", "plan_type": "plus"}));
        let fixture = json!({
            "auth_mode": "chatgpt", "OPENAI_API_KEY": null, "last_refresh": "2026-08-09T00:00:00Z",
            "tokens": {"id_token": expired, "access_token": expired, "refresh_token": REFRESH, "account_id": "account-1"}
        });
        let imported = login
            .import_auth_json(&serde_json::to_vec(&fixture).unwrap())
            .await
            .unwrap();
        assert_eq!(imported.account_id, "account-1");
        assert_eq!(imported.payload.as_value()["refresh_token"], REFRESH);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let mut missing = fixture;
        missing["tokens"]
            .as_object_mut()
            .unwrap()
            .remove("account_id");
        assert_eq!(
            login
                .import_auth_json(&serde_json::to_vec(&missing).unwrap())
                .await
                .unwrap_err(),
            ProviderError::ImportFailed
        );
        server.abort();
    }

    #[test]
    fn sub2api_export_first_account_is_parsed_as_codex_payload() {
        let fixture = json!({
            "type": "sub2api-data",
            "version": 1,
            "accounts": [
                {
                    "platform": "openai",
                    "type": "oauth",
                    "credentials": {
                        "id_token": "id-token",
                        "access_token": "access-token",
                        "refresh_token": "refresh-token",
                        "chatgpt_account_id": "account-1"
                    }
                },
                {
                    "platform": "anthropic",
                    "type": "oauth",
                    "credentials": {}
                }
            ]
        });
        let payload = parse_auth_json(&fixture).unwrap();
        let value = payload.as_value();
        assert_eq!(value["account_id"], "account-1");
        assert_eq!(value["refresh_token"], "refresh-token");
        assert_eq!(value["access_token"], "access-token");
        assert_eq!(value["id_token"], "id-token");
    }

    #[test]
    fn sub2api_export_without_accounts_is_rejected() {
        let fixture = json!({"type": "sub2api-data", "accounts": []});
        assert_eq!(
            parse_auth_json(&fixture).unwrap_err(),
            ProviderError::ImportFailed
        );
    }

    #[test]
    fn sub2api_account_with_expires_at_is_preserved() {
        let fixture = json!({
            "type": "sub2api-data",
            "accounts": [{
                "platform": "openai",
                "type": "oauth",
                "credentials": {
                    "id_token": "id-token",
                    "access_token": "access-token",
                    "refresh_token": "refresh-token",
                    "chatgpt_account_id": "account-1",
                    "expires_at": "2099-01-01T00:00:00Z"
                }
            }]
        });
        let payload = parse_auth_json(&fixture).unwrap();
        assert_eq!(payload.as_value()["expires_at"], "2099-01-01T00:00:00Z");
    }

    #[test]
    fn sub2api_account_id_falls_back_to_account_id_field() {
        let fixture = json!({
            "type": "sub2api-data",
            "accounts": [{
                "platform": "openai",
                "type": "oauth",
                "credentials": {
                    "id_token": "id-token",
                    "access_token": "access-token",
                    "refresh_token": "refresh-token",
                    "account_id": "fallback-acct"
                }
            }]
        });
        // chatgpt_account_id is missing → should fail (required field)
        assert_eq!(
            parse_auth_json(&fixture).unwrap_err(),
            ProviderError::ImportFailed
        );
    }

    #[test]
    fn export_is_nested_private_backed_up_and_preserves_old_file_if_rename_fails() {
        let directory =
            std::env::temp_dir().join(format!("waliapi-codex-login-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("auth.json");
        let old = b"old auth bytes";
        fs::write(&path, old).unwrap();
        let payload = ProviderPayload::new(
            json!({"id_token": ID, "access_token": ACCESS, "refresh_token": REFRESH, "account_id": "account-1"}),
        );
        let result = CodexLogin::write_auth_json(&path, &payload).unwrap();
        let written: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["tokens"]["refresh_token"], REFRESH);
        assert_eq!(written["auth_mode"], "chatgpt");
        assert_eq!(fs::read(result.backup_path.as_ref().unwrap()).unwrap(), old);
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let bytes_before_failure = fs::read(&path).unwrap();
        assert_eq!(
            write_auth_json_with_rename(&path, &payload, |_, _| Err(io::Error::other(
                "injected rename failure"
            )))
            .unwrap_err(),
            ProviderError::Storage
        );
        assert_eq!(fs::read(&path).unwrap(), bytes_before_failure);
        let new_path = directory.join("exported-auth.json");
        let result = CodexLogin::write_auth_json(&new_path, &payload).unwrap();
        let written: Value = serde_json::from_slice(&fs::read(&new_path).unwrap()).unwrap();
        assert_eq!(written["tokens"]["account_id"], "account-1");
        assert!(result.backup_path.is_none());
        fs::remove_dir_all(&directory).unwrap();
    }

    /// CLIProxyAPI `auths/*.json` fixture: the Codex tokens flattened to the
    /// top level plus its own bookkeeping fields (`type`, `email`, `expired`).
    fn cpa_fixture() -> Value {
        json!({
            "id_token": ID, "access_token": ACCESS, "refresh_token": REFRESH,
            "account_id": "cpa-account-1", "last_refresh": "2026-08-20T00:00:00Z",
            "email": "cpa@example.test", "type": "codex", "expired": "2026-09-01T00:00:00Z"
        })
    }

    #[test]
    fn cpa_flat_auth_imports_into_the_same_payload_shape() {
        let parsed: Value = cpa_fixture();
        let payload = parse_cpa_auth_json(&parsed).unwrap();
        assert_eq!(payload.as_value()["account_id"], "cpa-account-1");
        assert_eq!(payload.as_value()["refresh_token"], REFRESH);
        assert_eq!(payload.as_value()["access_token"], ACCESS);
        assert_eq!(payload.as_value()["id_token"], ID);
    }

    #[test]
    fn cpa_rejects_non_codex_type_and_missing_fields() {
        let mut wrong_type = cpa_fixture();
        wrong_type["type"] = json!("claude");
        assert_eq!(
            parse_cpa_auth_json(&wrong_type).unwrap_err(),
            ProviderError::ImportFailed
        );
        let mut missing = cpa_fixture();
        missing.as_object_mut().unwrap().remove("refresh_token");
        assert_eq!(
            parse_cpa_auth_json(&missing).unwrap_err(),
            ProviderError::ImportFailed
        );
    }

    #[test]
    fn sub2api_openai_accounts_are_extracted_and_others_skipped() {
        // Synthetic normalization-variant fixture: real sub2api exports carry
        // `chatgpt_account_id` + `type: "sub2api-data"` (see the regression
        // test below); an explicit `account_id` must win over normalization.
        let fixture = json!({
            "type": "accounts", "version": 1,
            "accounts": [
                {
                    "name": "codex-main",
                    "platform": "openai", "type": "oauth",
                    "credentials": {
                        "access_token": ACCESS, "refresh_token": REFRESH,
                        "id_token": ID, "account_id": "sub2api-a"
                    }
                },
                {
                    "name": "claude-side",
                    "platform": "anthropic", "type": "oauth",
                    "credentials": {"session_key": "sk-ant-..."}
                },
                {
                    "name": "no-credentials",
                    "platform": "openai", "type": "oauth",
                    "credentials": {}
                }
            ]
        });
        let accounts = sub2api_codex_accounts(&fixture).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].label.as_deref(), Some("codex-main"));
        assert_eq!(accounts[0].payload.as_value()["account_id"], "sub2api-a");
    }

    #[test]
    fn sub2api_real_export_uses_chatgpt_account_id() {
        // Real sub2api admin-data export shape (backend
        // openai_oauth_service.go writes `chatgpt_account_id`, not
        // `account_id`; extra bookkeeping keys like email/expires_at are
        // ignored).  Regression: the sole account used to be silently
        // skipped, surfacing as "导入失败" for a valid file.
        let fixture = json!({
            "type": "sub2api-data", "version": 1,
            "exported_at": "2026-08-26T08:31:47+00:00",
            "accounts": [
                {
                    "name": "jennifersimon497104t@outlook.com_40刀",
                    "platform": "openai", "type": "oauth",
                    "credentials": {
                        "access_token": ACCESS, "refresh_token": REFRESH,
                        "id_token": ID,
                        "chatgpt_account_id": "real-export-id",
                        "email": "jennifersimon497104t@outlook.com",
                        "expires_at": "2026-08-26T09:31:47+00:00"
                    }
                }
            ]
        });
        let accounts = sub2api_codex_accounts(&fixture).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].payload.as_value()["account_id"], "real-export-id");
        assert_eq!(
            accounts[0].label.as_deref(),
            Some("jennifersimon497104t@outlook.com_40刀")
        );
    }

    #[test]
    fn sub2api_without_any_importable_account_reports_skip_count() {
        let only_claude = json!({
            "accounts": [{"name": "x", "platform": "anthropic", "type": "oauth", "credentials": {}}]
        });
        assert!(sub2api_codex_accounts(&only_claude).unwrap().is_empty());
        assert!(sub2api_codex_accounts(&json!({"accounts": []}))
            .unwrap()
            .is_empty());
        assert!(sub2api_codex_accounts(&json!({})).unwrap().is_empty());
    }

    #[tokio::test]
    async fn import_dispatches_by_format() {
        let (token_url, _hits, server) = mock_oauth().await;
        let login = CodexLogin::with_endpoints("http://127.0.0.1/authorize", token_url);

        // codex format (default) still works.
        let nested = json!({
            "auth_mode": "chatgpt", "OPENAI_API_KEY": null, "last_refresh": "2026-08-20T00:00:00Z",
            "tokens": {"id_token": ID, "access_token": ACCESS, "refresh_token": REFRESH, "account_id": "fmt-codex"}
        });
        let imported = login
            .import_auth_json_with_format(
                &serde_json::to_vec(&nested).unwrap(),
                AuthFileFormat::Codex,
            )
            .await
            .unwrap();
        assert_eq!(imported.account_id, "fmt-codex");

        // cpa flat format maps onto the same payload.
        let imported = login
            .import_auth_json_with_format(
                &serde_json::to_vec(&cpa_fixture()).unwrap(),
                AuthFileFormat::Cpa,
            )
            .await
            .unwrap();
        assert_eq!(imported.account_id, "cpa-account-1");

        // Unknown format is rejected as a plain parse failure.
        assert!(matches!(
            AuthFileFormat::from_name("yaml"),
            Err(ProviderError::ImportFailed)
        ));
        server.abort();
    }

    #[tokio::test]
    async fn sub2api_import_refreshes_expired_token_and_keeps_label() {
        let (token_url, hits, server) = mock_oauth().await;
        let login = CodexLogin::with_endpoints("http://127.0.0.1/authorize", token_url);
        let expired = jwt(json!({"exp": 1, "email": "person@example.test", "plan_type": "plus"}));
        let fixture = json!({
            "accounts": [
                {"name": "expired-one", "platform": "openai", "type": "oauth",
                 "credentials": {"access_token": expired, "refresh_token": REFRESH,
                                 "id_token": expired, "account_id": "sub2api-exp"}}
            ]
        });
        let mut accounts = sub2api_codex_accounts(&fixture).unwrap();
        assert_eq!(accounts.len(), 1);
        let result = login
            .import_codex_account(accounts.remove(0))
            .await
            .unwrap();
        assert_eq!(result.account_id, "sub2api-exp");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "expired token must refresh");
        assert_eq!(result.label, "person@example.test");
        server.abort();
    }
}
