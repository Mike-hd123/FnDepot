//! Kimi Code OAuth 2.0 Device Authorization Grant (RFC 8628).
//!
//! Kimi Code does not use Codex's loopback callback, state or PKCE.  Login is a
//! pure polling loop against the fixed OAuth host; the only interactivity is
//! the UI surfacing of `verification_uri_complete` + `user_code` and an
//! optional browser open.  This module owns OAuth/refresh/HTTP-state
//! classification only — never protocol conversion, which stays in the codec
//! registry.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use serde_json::json;

use super::{
    LoginResult, LoginRuntime, LoginStep, ProviderError, ProviderPayload, RefreshedPayload,
};

pub const KIMI_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
pub const KIMI_OAUTH_HOST: &str = "https://auth.kimi.com";
pub const KIMI_DEVICE_AUTH_PATH: &str = "/api/oauth/device_authorization";
pub const KIMI_TOKEN_PATH: &str = "/api/oauth/token";
pub const KIMI_LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const KIMI_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Header values fixed by the official Kimi transport; the renderer/downstream
/// can never override them.
const KIMI_USER_AGENT: &str = "kimi-code-cli/1.0 (WaLiAPI)";
const KIMI_CHECK_INTERVAL: u64 = 5;

/// Server-side device authorization response (RFC 8628 §3.2).
#[derive(Deserialize)]
struct DeviceAuthorizationResponse {
    user_code: String,
    device_code: String,
    #[serde(default)]
    verification_uri: Option<String>,
    verification_uri_complete: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

/// Server-side token pollination response (RFC 8628 §3.4 / OAuth token).
#[derive(Deserialize)]
struct TokenWireResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<serde_json::Value>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

/// A hand-written Debug that reveals only field presence, never the values.
/// Raw wire responses must never reach logs or panic output.
impl std::fmt::Debug for DeviceAuthorizationResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceAuthorizationResponse")
            .field("user_code_present", &!self.user_code.is_empty())
            .field("device_code_present", &!self.device_code.is_empty())
            .field("verification_uri_present", &self.verification_uri.is_some())
            .field(
                "verification_uri_complete_present",
                &!self.verification_uri_complete.is_empty(),
            )
            .field("expires_in_present", &self.expires_in.is_some())
            .field("interval_present", &self.interval.is_some())
            .finish()
    }
}

impl std::fmt::Debug for TokenWireResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenWireResponse")
            .field("access_token_present", &self.access_token.is_some())
            .field("refresh_token_present", &self.refresh_token.is_some())
            .field("expires_in_present", &self.expires_in.is_some())
            .field("scope_present", &self.scope.is_some())
            .field("token_type_present", &self.token_type.is_some())
            .field("error_present", &self.error.is_some())
            .field(
                "error_description_present",
                &self.error_description.is_some(),
            )
            .finish()
    }
}

/// Fully validated OAuth material.  Never serialized, never Debug-printed.
#[derive(Clone)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub scope: String,
    pub token_type: String,
}

#[derive(Clone)]
pub struct KimiLogin {
    client: reqwest::Client,
    device_auth_url: String,
    token_url: String,
}

impl Default for KimiLogin {
    fn default() -> Self {
        Self::new()
    }
}

impl KimiLogin {
    /// Production constructor: fixed HTTPS endpoints only.
    pub fn new() -> Self {
        Self::with_endpoints(
            format!("{KIMI_OAUTH_HOST}{KIMI_DEVICE_AUTH_PATH}"),
            format!("{KIMI_OAUTH_HOST}{KIMI_TOKEN_PATH}"),
        )
    }

    /// Test constructor: local mock URLs are allowed here so coverage never
    /// touches the real Kimi service.
    pub fn with_endpoints(
        device_auth_url: impl Into<String>,
        token_url: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(KIMI_HTTP_TIMEOUT)
                .build()
                .expect("kimi login http client"),
            device_auth_url: device_auth_url.into(),
            token_url: token_url.into(),
        }
    }

    /// RFC 8628 device authorization request body.
    fn device_auth_form() -> Vec<(&'static str, &'static str)> {
        vec![("client_id", KIMI_CLIENT_ID)]
    }

    /// RFC 8628 token polling request body.
    fn token_poll_form(device_code: &str) -> Vec<(&'static str, String)> {
        vec![
            ("client_id", KIMI_CLIENT_ID.to_owned()),
            ("device_code", device_code.to_owned()),
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code".to_owned(),
            ),
        ]
    }

    /// RFC 7001 refresh request body.
    fn refresh_form(refresh_token: &str) -> Vec<(&'static str, String)> {
        vec![
            ("client_id", KIMI_CLIENT_ID.to_owned()),
            ("grant_type", "refresh_token".to_owned()),
            ("refresh_token", refresh_token.to_owned()),
        ]
    }

    /// Common OAuth headers.  `include_device` adds the fixed
    /// `X-Msh-Device-Id` header used by the official Kimi transport.
    fn base_headers(include_device: bool, device_id: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static(KIMI_USER_AGENT));
        if include_device {
            let value = HeaderValue::from_str(device_id).unwrap_or_else(|_| {
                HeaderValue::from_static("00000000-0000-0000-0000-000000000000")
            });
            headers.insert(HeaderName::from_static("x-msh-device-id"), value);
        }
        headers
    }

    /// Run the complete device login loop.
    pub async fn login(
        &self,
        runtime: &dyn LoginRuntime,
        device_id: &str,
    ) -> Result<LoginResult, ProviderError> {
        let deadline = tokio::time::Instant::now() + KIMI_LOGIN_TIMEOUT;
        // Outer loop re-issues device authorization if the code expires.
        loop {
            if runtime.is_cancelled() {
                return Err(ProviderError::LoginCancelled);
            }
            runtime.set_step(LoginStep::Preparing).await;

            let device = self
                .request_device_authorization(runtime, device_id)
                .await?;
            if device.verification_uri_complete.is_empty() {
                return Err(ProviderError::DeviceAuthorizationFailed);
            }
            let verification_url = device.verification_uri_complete.clone();
            let interval = device.interval.unwrap_or(KIMI_CHECK_INTERVAL).max(1);
            let mut current_interval = interval;

            // UI surface FIRST, so a browser failure still shows URL + code.
            // `expires_at` reflects the server's `expires_in` so the UI can
            // show a real countdown instead of an already-expired timestamp.
            let expires_at = device
                .expires_in
                .map(|seconds| (Utc::now() + ChronoDuration::seconds(seconds as i64)).to_rfc3339());
            runtime
                .present_device_authorization(&verification_url, &device.user_code, expires_at)
                .await?;

            // Browser failure is non-fatal for Kimi; user can open manually.
            match runtime.open_browser(&verification_url).await {
                Ok(()) => {}
                Err(error) => {
                    if !runtime.is_cancelled() {
                        tracing::warn!("could not open Kimi authorization browser: {error}");
                    }
                }
            }

            // Poll until success, expiry, or terminal failure.
            match self
                .poll_device_token(
                    runtime,
                    &device.device_code,
                    &mut current_interval,
                    deadline,
                )
                .await
            {
                Ok(tokens) => {
                    runtime.set_step(LoginStep::Exchanging).await;
                    return Ok(self.login_result(tokens, device_id));
                }
                Err(TokenPollError::Expired) => {
                    // Re-issue a fresh device code; same overall deadline.
                    continue;
                }
                Err(TokenPollError::Denied) => {
                    return Err(ProviderError::AuthorizationDenied);
                }
                Err(TokenPollError::Timeout) => return Err(ProviderError::LoginTimeout),
                Err(TokenPollError::Cancelled) => return Err(ProviderError::LoginCancelled),
                Err(TokenPollError::ClientError) => {
                    return Err(ProviderError::TokenExchangeFailed);
                }
            }
        }
    }

    async fn request_device_authorization(
        &self,
        runtime: &dyn LoginRuntime,
        device_id: &str,
    ) -> Result<DeviceAuthorizationResponse, ProviderError> {
        if runtime.is_cancelled() {
            return Err(ProviderError::LoginCancelled);
        }
        runtime.set_step(LoginStep::Authorizing).await;
        let response = self
            .client
            .post(&self.device_auth_url)
            .headers(Self::base_headers(true, device_id))
            .form(&Self::device_auth_form())
            .send()
            .await
            .map_err(|_| ProviderError::Retryable)?;
        if !response.status().is_success() {
            return Err(ProviderError::DeviceAuthorizationFailed);
        }
        serde_json::from_slice(
            &response
                .bytes()
                .await
                .map_err(|_| ProviderError::Retryable)?,
        )
        .map_err(|_| ProviderError::DeviceAuthorizationFailed)
    }

    async fn poll_device_token(
        &self,
        runtime: &dyn LoginRuntime,
        device_code: &str,
        interval: &mut u64,
        deadline: tokio::time::Instant,
    ) -> Result<OAuthTokens, TokenPollError> {
        loop {
            if runtime.is_cancelled() {
                return Err(TokenPollError::Cancelled);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(TokenPollError::Timeout);
            }
            runtime.set_step(LoginStep::Waiting).await;
            let response = match self
                .client
                .post(&self.token_url)
                .headers(Self::base_headers(false, ""))
                .form(&Self::token_poll_form(device_code))
                .send()
                .await
            {
                Ok(response) => response,
                Err(_) => {
                    if runtime.is_cancelled() {
                        return Err(TokenPollError::Cancelled);
                    }
                    if tokio::time::Instant::now() >= deadline {
                        return Err(TokenPollError::Timeout);
                    }
                    poll_sleep(runtime, *interval).await;
                    continue;
                }
            };

            if response.status() == reqwest::StatusCode::OK {
                let wire: TokenWireResponse = match response.json().await {
                    Ok(value) => value,
                    Err(_) => return Err(TokenPollError::ClientError),
                };
                return match parse_success_tokens(wire) {
                    Ok(tokens) => Ok(tokens),
                    Err(()) => Err(TokenPollError::ClientError),
                };
            }

            if response.status() == reqwest::StatusCode::BAD_REQUEST {
                let wire: Option<TokenWireResponse> = response.json().await.ok();
                let error = wire.and_then(|w| w.error.clone()).unwrap_or_default();
                match error.as_str() {
                    "authorization_pending" => poll_sleep(runtime, *interval).await,
                    "slow_down" => {
                        *interval += 5;
                        poll_sleep(runtime, *interval).await;
                    }
                    "expired_token" => return Err(TokenPollError::Expired),
                    "access_denied" => return Err(TokenPollError::Denied),
                    _ => return Err(TokenPollError::ClientError),
                }
                continue;
            }

            if response.status().is_server_error() {
                poll_sleep(runtime, *interval).await;
                continue;
            }

            return Err(TokenPollError::ClientError);
        }
    }

    fn login_result(&self, tokens: OAuthTokens, device_id: &str) -> LoginResult {
        let now = Utc::now();
        let expires_at = (now + ChronoDuration::seconds(tokens.expires_in as i64)).to_rfc3339();
        LoginResult {
            account_id: device_id.to_owned(),
            label: "Kimi Code".to_owned(),
            attributes: json!({
                "plan_type": "kimi_code",
                "identity_source": "local_device_id"
            }),
            payload: ProviderPayload::new(json!({
                "version": 1,
                "access_token": tokens.access_token,
                "refresh_token": tokens.refresh_token,
                "token_type": token_type_value(&tokens.token_type),
                "scope": tokens.scope,
                "expires_at": expires_at,
                "expires_in": tokens.expires_in,
                "device_id": device_id,
            })),
            last_refreshed_at: Some(now.to_rfc3339()),
            next_refresh_after: None,
            next_retry_after: None,
        }
    }

    pub async fn refresh_payload(
        &self,
        payload: &ProviderPayload,
        device_id: &str,
    ) -> Result<RefreshedPayload, ProviderError> {
        let refresh_token = required_refresh(payload)?;
        let mut attempt = 0;
        loop {
            match self.refresh_once(&refresh_token).await {
                Ok(tokens) => {
                    let now = Utc::now();
                    let expires_at =
                        (now + ChronoDuration::seconds(tokens.expires_in as i64)).to_rfc3339();
                    return Ok(RefreshedPayload {
                        payload: ProviderPayload::new(json!({
                            "version": 1,
                            "access_token": tokens.access_token,
                            "refresh_token": tokens.refresh_token,
                            "token_type": token_type_value(&tokens.token_type),
                            "scope": tokens.scope,
                            "expires_at": expires_at,
                            "expires_in": tokens.expires_in,
                            "device_id": device_id,
                        })),
                        last_refreshed_at: Some(now.to_rfc3339()),
                        next_refresh_after: None,
                        next_retry_after: None,
                    });
                }
                Err(RefreshError::Unauthorized) => return Err(ProviderError::Unauthorized),
                Err(RefreshError::PaymentRequired) => {
                    return Err(ProviderError::PaymentRequired)
                }
                Err(RefreshError::Retryable) => {
                    attempt += 1;
                    if attempt >= 3 {
                        return Err(ProviderError::Retryable);
                    }
                    tokio::time::sleep(Duration::from_secs(attempt)).await;
                }
                Err(RefreshError::Protocol) => return Err(ProviderError::Protocol),
            }
        }
    }

    async fn refresh_once(&self, refresh_token: &str) -> Result<OAuthTokens, RefreshError> {
        let response = self
            .client
            .post(&self.token_url)
            .headers(Self::base_headers(false, ""))
            .form(&Self::refresh_form(refresh_token))
            .send()
            .await
            .map_err(|_| RefreshError::Retryable)?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(RefreshError::Unauthorized);
        }
        if status == reqwest::StatusCode::PAYMENT_REQUIRED {
            return Err(RefreshError::PaymentRequired);
        }
        if status.is_success() {
            let wire: TokenWireResponse = match response.json().await {
                Ok(wire) => wire,
                Err(_) => return Err(RefreshError::Protocol),
            };
            return match parse_success_tokens(wire) {
                Ok(tokens) => Ok(tokens),
                Err(()) => Err(RefreshError::Protocol),
            };
        }
        let wire: TokenWireResponse = response.json().await.unwrap_or(TokenWireResponse {
            access_token: None,
            refresh_token: None,
            expires_in: None,
            scope: None,
            token_type: None,
            error: None,
            error_description: None,
        });
        if wire.error.as_deref() == Some("invalid_grant") {
            return Err(RefreshError::Unauthorized);
        }
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(RefreshError::Retryable);
        }
        Err(RefreshError::Protocol)
    }
}

impl Default for OAuthTokens {
    fn default() -> Self {
        Self {
            access_token: String::new(),
            refresh_token: String::new(),
            expires_in: 3600,
            scope: String::new(),
            token_type: "Bearer".into(),
        }
    }
}

fn token_type_value(token_type: &str) -> String {
    if token_type.is_empty() {
        "Bearer".to_owned()
    } else {
        token_type.to_owned()
    }
}

fn required_refresh(payload: &ProviderPayload) -> Result<String, ProviderError> {
    payload
        .as_value()
        .get("refresh_token")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or(ProviderError::InvalidPayload)
}

fn parse_success_tokens(wire: TokenWireResponse) -> Result<OAuthTokens, ()> {
    let access_token = wire.access_token.filter(|s| !s.is_empty()).ok_or(())?;
    let refresh_token = wire.refresh_token.filter(|s| !s.is_empty()).ok_or(())?;
    let expires_in = match wire.expires_in {
        Some(serde_json::Value::Number(number)) => number.as_u64().filter(|v| *v > 0).ok_or(())?,
        None => 3600,
        _ => return Err(()),
    };
    Ok(OAuthTokens {
        access_token,
        refresh_token,
        expires_in,
        scope: wire.scope.unwrap_or_default(),
        token_type: wire.token_type.unwrap_or_else(|| "Bearer".into()),
    })
}

enum TokenPollError {
    Timeout,
    Cancelled,
    Expired,
    Denied,
    ClientError,
}

enum RefreshError {
    Unauthorized,
    /// Upstream reports the paid subscription cannot be used.  Terminal.
    PaymentRequired,
    Retryable,
    Protocol,
}

/// Sleep for one poll interval, but immediately return if runtime cancels.
async fn poll_sleep(runtime: &dyn LoginRuntime, interval: u64) {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
        _ = runtime.cancelled() => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use axum::{extract::State, routing::post, Json, Router};
    use reqwest::header::HeaderMap;
    use serde_json::{json, Value};
    use tokio::sync::watch;

    use super::*;
    use crate::auth_provider::{LoginRuntime, LoginStep};

    const DEVICE_ID: &str = "d1e2b3a4c5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f";
    const ACCESS: &str = "fixture-access-token";
    const REFRESH: &str = "fixture-refresh-token";

    #[derive(Clone, Default)]
    struct MockState {
        seen: Arc<Mutex<Vec<String>>>,
        headers: Arc<Mutex<Vec<HeaderMap>>>,
        device_hits: Arc<AtomicUsize>,
        token_hits: Arc<AtomicUsize>,
        /// Monotonic timestamps of every token poll, so a test can assert the
        /// RFC 8628 slow_down +5s interval bump is actually honored.
        token_times: Arc<Mutex<Vec<std::time::Instant>>>,
        /// When set, every token poll returns `authorization_pending` — the
        /// device never authorizes.  Used to drive timeout/cancel tests.
        always_pending: Arc<std::sync::atomic::AtomicBool>,
        /// Token responses served in order; the last one is repeated.
        queue: Arc<Mutex<VecDeque<(u16, Value)>>>,
        device_codes: Arc<Mutex<VecDeque<String>>>,
    }

    async fn device(
        State(state): State<MockState>,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> (axum::http::StatusCode, Json<Value>) {
        state.device_hits.fetch_add(1, Ordering::SeqCst);
        state.headers.lock().unwrap().push(headers.clone());
        state
            .seen
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(&body).to_string());
        let mut codes = state.device_codes.lock().unwrap();
        let code = codes
            .pop_front()
            .unwrap_or_else(|| "device-code-0".to_owned());
        (
            axum::http::StatusCode::OK,
            Json(json!({
                "device_code": code,
                "user_code": "ABCD-EFGH",
                "verification_uri": "https://auth.kimi.com/verify",
                "verification_uri_complete": "https://auth.kimi.com/verify?user_code=ABCD-EFGH",
                "expires_in": 1800,
                "interval": 1,
            })),
        )
    }

    async fn token(
        State(state): State<MockState>,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> (axum::http::StatusCode, Json<Value>) {
        state.token_hits.fetch_add(1, Ordering::SeqCst);
        state.token_times.lock().unwrap().push(std::time::Instant::now());
        state.headers.lock().unwrap().push(headers.clone());
        state
            .seen
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(&body).to_string());
        if state.always_pending.load(std::sync::atomic::Ordering::SeqCst) {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error": "authorization_pending"})),
            );
        }
        let mut queue = state.queue.lock().unwrap();
        if queue.is_empty() {
            // Default final success.
            return (
                axum::http::StatusCode::OK,
                Json(json!({
                    "access_token": ACCESS,
                    "refresh_token": REFRESH,
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "scope": "openid"
                })),
            );
        }
        let (status, value) = queue.pop_front().unwrap();
        (
            axum::http::StatusCode::from_u16(status).unwrap(),
            Json(value),
        )
    }

    async fn mock(_kimi: bool) -> (KimiLogin, MockState) {
        let state = MockState::default();
        let app = Router::new()
            .route("/device", post(device))
            .route("/token", post(token))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let login = KimiLogin::with_endpoints(
            format!("http://{addr}/device"),
            format!("http://{addr}/token"),
        );
        (login, state)
    }

    #[derive(Clone)]
    struct TestRuntime {
        steps: Arc<Mutex<Vec<String>>>,
        shown: Arc<Mutex<Vec<(String, String)>>>,
        expires: Arc<Mutex<Vec<Option<String>>>>,
        cancel: Arc<watch::Sender<bool>>,
        // Kept alive so `send(true)` actually updates the value even with no
        // other observer; dropping the only receiver makes watch::send a no-op.
        _rx: Arc<watch::Receiver<bool>>,
    }
    impl Default for TestRuntime {
        fn default() -> Self {
            let (tx, rx) = watch::channel(false);
            Self {
                steps: Arc::new(Mutex::new(Vec::new())),
                shown: Arc::new(Mutex::new(Vec::new())),
                expires: Arc::new(Mutex::new(Vec::new())),
                cancel: Arc::new(tx),
                _rx: Arc::new(rx),
            }
        }
    }
    impl TestRuntime {
        fn cancel(&self) {
            let _ = self.cancel.send(true);
        }
    }
    #[async_trait::async_trait]
    impl LoginRuntime for TestRuntime {
        async fn open_browser(&self, _url: &str) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn set_step(&self, step: LoginStep) {
            self.steps.lock().unwrap().push(step.as_str().to_owned());
        }
        async fn present_device_authorization(
            &self,
            url: &str,
            user_code: &str,
            expires_at: Option<String>,
        ) -> Result<(), ProviderError> {
            self.shown
                .lock()
                .unwrap()
                .push((url.to_owned(), user_code.to_owned()));
            self.expires.lock().unwrap().push(expires_at);
            Ok(())
        }
        fn is_cancelled(&self) -> bool {
            *self.cancel.borrow()
        }
        async fn cancelled(&self) {
            let mut receiver = self.cancel.subscribe();
            while !*receiver.borrow() {
                if receiver.changed().await.is_err() {
                    return;
                }
            }
        }
    }

    fn ok(access: &str, refresh: &str) -> Value {
        json!({
            "access_token": access,
            "refresh_token": refresh,
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "openid"
        })
    }

    #[tokio::test]
    async fn device_auth_form_has_only_client_id_and_fixed_headers() {
        let (login, state) = mock(true).await;
        login
            .login(&TestRuntime::default(), DEVICE_ID)
            .await
            .unwrap();
        let seen = state.seen.lock().unwrap();
        assert_eq!(seen[0], format!("client_id={KIMI_CLIENT_ID}"));
        assert_eq!(state.device_hits.load(Ordering::SeqCst), 1);
        let headers = state.headers.lock().unwrap();
        let device_header = headers[0].clone();
        assert_eq!(
            device_header.get("content-type").unwrap().to_str().unwrap(),
            "application/x-www-form-urlencoded"
        );
        assert_eq!(
            device_header.get("accept").unwrap().to_str().unwrap(),
            "application/json"
        );
        assert_eq!(
            device_header
                .get("x-msh-device-id")
                .unwrap()
                .to_str()
                .unwrap(),
            DEVICE_ID
        );
        // Token poll has no device header (request[1] is the first token poll).
        let token_header = headers[1].clone();
        assert!(!token_header.contains_key("x-msh-device-id"));
    }

    #[tokio::test]
    async fn login_pending_then_success() {
        let (login, state) = mock(true).await;
        state
            .queue
            .lock()
            .unwrap()
            .push_back((400, json!({"error": "authorization_pending"})));
        state
            .queue
            .lock()
            .unwrap()
            .push_back((400, json!({"error": "authorization_pending"})));
        let result = login
            .login(&TestRuntime::default(), DEVICE_ID)
            .await
            .unwrap();
        assert_eq!(result.account_id, DEVICE_ID);
        assert_eq!(result.payload.as_value()["access_token"], ACCESS);
        assert_eq!(result.payload.as_value()["device_id"], DEVICE_ID);
        // 1 device auth + 2 pending + 1 success = 4 token hits.
        assert_eq!(state.token_hits.load(Ordering::SeqCst), 3);
        assert_eq!(state.device_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn login_slow_down_then_success() {
        let (login, state) = mock(true).await;
        state
            .queue
            .lock()
            .unwrap()
            .push_back((400, json!({"error": "slow_down"})));
        let result = login
            .login(&TestRuntime::default(), DEVICE_ID)
            .await
            .unwrap();
        assert_eq!(result.account_id, DEVICE_ID);
        // slow_down bumps interval so poll #2 uses interval+5.
        assert_eq!(state.token_hits.load(Ordering::SeqCst), 2);
        // The mock device interval is 1s; slow_down must add 5s, so the gap
        // between the two token polls is ~6s (5..=7 to absorb clock jitter).
        let times = state.token_times.lock().unwrap();
        assert_eq!(times.len(), 2);
        let gap = times[1].saturating_duration_since(times[0]).as_secs();
        assert!(
            (5..=7).contains(&gap),
            "slow_down interval bump not honored: token poll gap was {gap}s"
        );
    }

    #[tokio::test]
    async fn poll_hits_deadline_after_pending() {
        let (login, state) = mock(true).await;
        // The device never authorizes and every poll is pending; a zero poll
        // interval plus a very short deadline means the poll loop exits via the
        // deadline.
        state
            .always_pending
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut interval = 0;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(50);
        let result = login
            .poll_device_token(&TestRuntime::default(), "code", &mut interval, deadline)
            .await;
        assert!(
            matches!(result, Err(TokenPollError::Timeout)),
            "expected Timeout, got a different poll outcome"
        );
    }

    #[tokio::test]
    async fn cancel_during_polling_exits_promptly() {
        let (login, state) = mock(true).await;
        // First poll returns pending; then a cancellation arrives while the
        // poll sleep is pending, which must interrupt the sleep (not wait for
        // the full interval).
        state
            .queue
            .lock()
            .unwrap()
            .push_back((400, json!({"error": "authorization_pending"})));
        let runtime = TestRuntime::default();
        let handle = tokio::spawn({
            let login = login.clone();
            let runtime = runtime.clone();
            async move { login.login(&runtime, DEVICE_ID).await }
        });
        // Give the login time to reach the first pending poll and start sleeping.
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        runtime.cancel();
        let result = handle.await.unwrap().unwrap_err();
        assert_eq!(result, ProviderError::LoginCancelled);
    }

    #[tokio::test]
    async fn device_authorization_ui_surfaces_real_expiry_not_deadline() {
        let (login, _state) = mock(true).await;
        let runtime = TestRuntime::default();
        login.login(&runtime, DEVICE_ID).await.unwrap();
        // The mock `/device` responds `expires_in: 1800`.  The UI must receive an
        // `expires_at` roughly `now + 1800s`, not `now` (the historic bug where
        // `expires_in` was only used as a presence check).
        let expires = runtime.expires.lock().unwrap();
        let Some(Some(expires_at)) = expires.last().cloned() else {
            panic!("present_device_authorization was never called with an expires_at");
        };
        let parsed = chrono::DateTime::parse_from_rfc3339(&expires_at)
            .unwrap()
            .with_timezone(&Utc);
        let window = (chrono::Utc::now() - parsed).num_seconds().unsigned_abs();
        // 1800s expected; the bug produced ~0s.  Allow ±60s of clock skew.
        assert!(
            (1750..=1850).contains(&window),
            "expires_at was {window}s from now (expected ~1800s)"
        );
    }

    #[tokio::test]
    async fn login_expired_token_reissues_device_code() {
        let (login, state) = mock(true).await;
        state
            .device_codes
            .lock()
            .unwrap()
            .push_front("code-1".to_owned());
        state
            .device_codes
            .lock()
            .unwrap()
            .push_back("code-2".to_owned());
        state
            .queue
            .lock()
            .unwrap()
            .push_back((400, json!({"error": "expired_token"})));
        let result = login
            .login(&TestRuntime::default(), DEVICE_ID)
            .await
            .unwrap();
        assert_eq!(result.account_id, DEVICE_ID);
        assert_eq!(state.device_hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn login_access_denied_returns_stable_error() {
        let (login, state) = mock(true).await;
        state
            .queue
            .lock()
            .unwrap()
            .push_back((400, json!({"error": "access_denied"})));
        assert_eq!(
            login
                .login(&TestRuntime::default(), DEVICE_ID)
                .await
                .unwrap_err(),
            ProviderError::AuthorizationDenied
        );
    }

    #[tokio::test]
    async fn login_cancelled_before_any_request() {
        let (login, _state) = mock(true).await;
        let runtime = TestRuntime::default();
        runtime.cancel();
        assert_eq!(
            login.login(&runtime, DEVICE_ID).await.unwrap_err(),
            ProviderError::LoginCancelled
        );
    }

    #[tokio::test]
    async fn login_payload_excludes_device_and_user_code_and_url() {
        let (login, _state) = mock(true).await;
        let result = login
            .login(&TestRuntime::default(), DEVICE_ID)
            .await
            .unwrap();
        let payload = result.payload.as_value().to_string();
        for forbidden in ["device-code", "ABCD-EFGH", "auth.kimi.com/verify"] {
            assert!(!payload.contains(forbidden), "payload leaked {forbidden}");
        }
    }

    #[tokio::test]
    async fn refresh_rotates_and_preserves_device_id() {
        let (login, state) = mock(true).await;
        state.queue.lock().unwrap().push_back((
            200,
            json!({
                "access_token": "rotated-access",
                "refresh_token": "rotated-refresh",
                "token_type": "Bearer",
                "expires_in": 7200,
                "scope": ""
            }),
        ));
        let payload = ProviderPayload::new(json!({
            "version": 1,
            "access_token": "old",
            "refresh_token": REFRESH,
            "expires_at": "2030-01-01T00:00:00Z"
        }));
        let refreshed = login.refresh_payload(&payload, DEVICE_ID).await.unwrap();
        assert_eq!(
            refreshed.payload.as_value()["access_token"],
            "rotated-access"
        );
        assert_eq!(
            refreshed.payload.as_value()["refresh_token"],
            "rotated-refresh"
        );
        assert_eq!(refreshed.payload.as_value()["device_id"], DEVICE_ID);
        // Confirm the refresh form.
        let seen = state.seen.lock().unwrap();
        assert!(
            seen[0].starts_with(&format!(
                "client_id={KIMI_CLIENT_ID}&grant_type=refresh_token&refresh_token={REFRESH}"
            )) || seen[0].starts_with(&format!(
                "grant_type=refresh_token&client_id={KIMI_CLIENT_ID}&refresh_token={REFRESH}"
            )) || seen[0].contains("grant_type=refresh_token")
                && seen[0].contains(&format!("refresh_token={REFRESH}"))
        );
        assert!(seen[0].contains(&format!("client_id={KIMI_CLIENT_ID}")));
    }

    #[tokio::test]
    async fn refresh_401_403_and_invalid_grant_are_unauthorized() {
        for (status, body) in [
            (401, Value::Null),
            (403, Value::Null),
            (400, json!({"error": "invalid_grant"})),
        ] {
            let (login, state) = mock(true).await;
            state.queue.lock().unwrap().push_back((status, body));
            let payload = ProviderPayload::new(json!({"refresh_token": REFRESH}));
            assert_eq!(
                login
                    .refresh_payload(&payload, DEVICE_ID)
                    .await
                    .unwrap_err(),
                ProviderError::Unauthorized
            );
        }
    }

    #[tokio::test]
    async fn refresh_402_maps_to_payment_required() {
        let (login, state) = mock(true).await;
        state.queue.lock().unwrap().push_back((402, Value::Null));
        let payload = ProviderPayload::new(json!({"refresh_token": REFRESH}));
        assert_eq!(
            login
                .refresh_payload(&payload, DEVICE_ID)
                .await
                .unwrap_err(),
            ProviderError::PaymentRequired
        );
    }

    #[tokio::test]
    async fn refresh_5xx_retries_three_times_then_fails() {
        let (login, state) = mock(true).await;
        for _ in 0..3 {
            state
                .queue
                .lock()
                .unwrap()
                .push_back((503, "overload".into()));
        }
        let payload = ProviderPayload::new(json!({"refresh_token": REFRESH}));
        assert_eq!(
            login
                .refresh_payload(&payload, DEVICE_ID)
                .await
                .unwrap_err(),
            ProviderError::Retryable
        );
        assert_eq!(state.token_hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn refresh_success_after_one_5xx_retry() {
        let (login, state) = mock(true).await;
        state
            .queue
            .lock()
            .unwrap()
            .push_back((503, "overload".into()));
        state
            .queue
            .lock()
            .unwrap()
            .push_back((200, ok("ok-access", "ok-refresh")));
        let payload = ProviderPayload::new(json!({"refresh_token": REFRESH}));
        let refreshed = login.refresh_payload(&payload, DEVICE_ID).await.unwrap();
        assert_eq!(refreshed.payload.as_value()["access_token"], "ok-access");
        assert_eq!(state.token_hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn refresh_connection_error_retries_and_fails_as_retryable() {
        // A URL with a closed listener yields a connect error.
        let state = MockState::default();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let login = KimiLogin::with_endpoints(
            format!("http://{addr}/device"),
            format!("http://{addr}/token"),
        );
        // Give the connection time to be refused instead of racing.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let payload = ProviderPayload::new(json!({"refresh_token": REFRESH}));
        let _ = state;
        let result = login.refresh_payload(&payload, DEVICE_ID).await;
        // 3 attempts then Retryable.
        assert_eq!(result.unwrap_err(), ProviderError::Retryable);
    }

    #[tokio::test]
    async fn wire_debug_prints_presence_only() {
        let wire = DeviceAuthorizationResponse {
            user_code: "ABCD-EFGH".into(),
            device_code: "device-secret".into(),
            verification_uri: Some("https://auth.kimi.com/v".into()),
            verification_uri_complete: "https://auth.kimi.com/v?code=ABCD-EFGH".into(),
            expires_in: Some(1800),
            interval: Some(1),
        };
        let rendered = format!("{wire:?}");
        assert!(rendered.contains("user_code_present: true"));
        assert!(!rendered.contains("ABCD-EFGH"));
        assert!(!rendered.contains("device-secret"));

        let token = TokenWireResponse {
            access_token: Some(ACCESS.into()),
            refresh_token: Some(REFRESH.into()),
            expires_in: Some(serde_json::json!(3600)),
            scope: Some("openid".into()),
            token_type: Some("Bearer".into()),
            error: None,
            error_description: None,
        };
        let rendered = format!("{token:?}");
        assert!(rendered.contains("access_token_present: true"));
        assert!(!rendered.contains(ACCESS));
        assert!(!rendered.contains(REFRESH));
    }

    #[test]
    fn constants_are_fixed() {
        assert_eq!(KIMI_CLIENT_ID, "17e5f671-d194-4dfb-9706-5516cb48c098");
        assert_eq!(KIMI_OAUTH_HOST, "https://auth.kimi.com");
        assert_eq!(KIMI_DEVICE_AUTH_PATH, "/api/oauth/device_authorization");
        assert_eq!(KIMI_TOKEN_PATH, "/api/oauth/token");
        assert_eq!(KIMI_LOGIN_TIMEOUT, Duration::from_secs(15 * 60));
    }
}
