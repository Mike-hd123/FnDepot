//! T07 — Channel draft connectivity test (`test_channel_draft`).
//!
//! Save-time endpoint probes for an UNSAVED channel draft.  This module is the
//! single home for:
//!
//! * the input/output DTOs (field names MUST match design 5.2 / `src/types`);
//! * the irreversible `draft_fingerprint` (SHA-256 over protocol/provider/
//!   canonical URL/models/endpoints/timeout + the *hash* of the API key — the
//!   plaintext key is never hashed into the returned value or logged);
//! * the draft URL SSRF policy (http/https, host, loopback exception for
//!   Ollama/custom, private/link-local/reserved ranges always blocked);
//! * per-endpoint minimal probes that REUSE the T06 `dispatch_executor`
//!   (URL construction, auth schemes, timeouts, version headers, native
//!   passthrough and legacy Gemini override) — no second URL builder;
//! * the short-lived in-process test-run receipt store used to validate
//!   `test_run_id + draft_fingerprint + force_save` at save time;
//! * the force-save validation contract.
//!
//! No DB channel is created/updated here, no quota is counted, and no
//! production request log is written.  Only sanitized diagnostic messages are
//! produced (they never contain the API key or a full request body).

use crate::core::attempt::{AttemptFailure, AttemptResult, FailureClass, PreparedAttempt};
use crate::core::channel_identity::{
    resolve_channel_identity, ChannelIdentity, ChannelIdentityRow,
};
use crate::core::route_plan::EndpointKind;
use crate::db::models::{now_iso, Channel};
use crate::db::repository::Repository;
use crate::endpoint_executor::dispatch_executor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Public DTOs (field names MUST match `src/types/index.ts` exactly)
// ---------------------------------------------------------------------------

/// One independent endpoint test result (design 5.2 + T07 wrapper).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftEndpointTestResult {
    pub endpoint: String,
    /// `"passed" | "failed" | "skipped"`
    pub status: String,
    /// `"network" | "timeout" | "authentication" | "endpoint_unsupported" |
    ///  "model" | "request" | "protocol" | "unknown"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Sanitized message (never contains the API key or a full request body).
    pub message: String,
    pub latency_ms: u64,
    pub tested_model: Option<String>,
    pub cost_possible: bool,
}

/// Whole-run result (T07 API contract).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftChannelTestResult {
    pub draft_fingerprint: String,
    pub tested_at: String,
    pub test_run_id: String,
    pub results: Vec<DraftEndpointTestResult>,
}

/// Input: the complete but UNSAVED channel draft.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DraftChannelTestInput {
    /// Present in the edit scenario so a blank API key can be filled from the
    /// existing channel server-side (never returned to the frontend).
    pub id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: String,
    pub base_url: String,
    pub api_key: String,
    /// Explicit clear of the stored key on edit (T02 semantics).  A blank
    /// `api_key` WITHOUT this flag means "keep the existing key" on an edit —
    /// the draft test resolves the effective key exactly like the save path.
    #[serde(default)]
    pub clear_api_key: Option<bool>,
    pub models: Vec<String>,
    pub priority: Option<i64>,
    pub weight: Option<i64>,
    pub config: Option<Value>,
    pub model_mapping: Option<Value>,
    pub timeout_secs: Option<i64>,
    pub protocol: Option<String>,
    pub provider: Option<String>,
    pub native_base_url: Option<String>,
    pub native_endpoints: Option<Vec<String>>,
    pub preset_revision: Option<String>,
    pub legacy_executor_override: Option<String>,
}

// ---------------------------------------------------------------------------
// Draft fingerprint
// ---------------------------------------------------------------------------

/// Normalize a draft model list: trim whitespace, drop empties, dedupe while
/// preserving input order (design 3.3).
fn normalize_models(models: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    models
        .iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty() && seen.insert(m.clone()))
        .collect()
}

/// Irreversible fingerprint covering every connection-affecting draft field.
///
/// The plaintext API key is NEVER included — only `SHA-256(key)` — so the
/// returned hex string cannot leak the key.  A change to protocol/provider/
/// URL/models/endpoints/timeout/key invalidates the fingerprint (T07 save
/// decision), which is what makes an old test run unusable for a new draft.
pub fn compute_draft_fingerprint(
    protocol: &str,
    provider: &str,
    native_base_url: &str,
    native_endpoints: &[String],
    models: &[String],
    timeout_secs: i64,
    api_key: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(protocol.trim().as_bytes());
    h.update(b"\n");
    h.update(provider.trim().as_bytes());
    h.update(b"\n");
    // Canonicalize the URL root only (trim + trailing slash); path is preserved.
    h.update(native_base_url.trim().trim_end_matches('/').as_bytes());
    h.update(b"\n");
    for ep in native_endpoints {
        h.update(ep.trim().as_bytes());
        h.update(b"\n");
    }
    for m in normalize_models(models) {
        h.update(m.as_bytes());
        h.update(b"\n");
    }
    h.update(timeout_secs.to_string().as_bytes());
    h.update(b"\n");
    h.update(hex::encode(Sha256::digest(api_key.as_bytes())).as_bytes());
    hex::encode(h.finalize())
}

// ---------------------------------------------------------------------------
// Draft identity (mirrors the repository's plan_channel_identity semantics)
// ---------------------------------------------------------------------------

/// Build the normalized identity for an unsaved draft.  When all new identity
/// fields are present they are trusted verbatim (revision 1); otherwise the
/// legacy `type/base_url/config` fields drive live inference (revision 0).
#[allow(clippy::too_many_arguments)]
pub fn build_draft_identity(
    protocol: &Option<String>,
    provider: &Option<String>,
    native_base_url: &Option<String>,
    native_endpoints: &Option<Vec<String>>,
    legacy_type: &str,
    legacy_base_url: &str,
    config: &Value,
    legacy_override: &Option<String>,
) -> ChannelIdentity {
    let all_present = protocol
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        && provider
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        && native_base_url
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        && native_endpoints
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
    let row = ChannelIdentityRow {
        channel_type: legacy_type.to_string(),
        base_url: legacy_base_url.to_string(),
        config: config.clone(),
        protocol: protocol.clone(),
        provider: provider.clone(),
        native_base_url: native_base_url.clone(),
        native_endpoints: native_endpoints
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string())),
        preset_revision: None,
        identity_revision: if all_present { 1 } else { 0 },
        legacy_executor_override: legacy_override.clone(),
    };
    resolve_channel_identity(&row)
}

/// Compute the fingerprint the SAVE command must match, from the effective
/// draft fields (None fields already resolved to current values by the caller).
#[allow(clippy::too_many_arguments)]
pub fn fingerprint_for_draft(
    protocol: Option<&str>,
    provider: Option<&str>,
    native_base_url: Option<&str>,
    native_endpoints: Option<&[String]>,
    models: &[String],
    timeout_secs: i64,
    api_key: &str,
    legacy_type: &str,
    legacy_base_url: &str,
    config: &Value,
    legacy_override: Option<&str>,
) -> String {
    let identity = build_draft_identity(
        &protocol.map(|s| s.to_string()),
        &provider.map(|s| s.to_string()),
        &native_base_url.map(|s| s.to_string()),
        &native_endpoints.map(|v| v.to_vec()),
        legacy_type,
        legacy_base_url,
        config,
        &legacy_override.map(|s| s.to_string()),
    );
    compute_draft_fingerprint(
        &identity.protocol,
        &identity.provider,
        &identity.native_base_url,
        &identity.native_endpoints,
        models,
        timeout_secs,
        api_key,
    )
}

// ---------------------------------------------------------------------------
// Draft URL SSRF policy
// ---------------------------------------------------------------------------

/// Validate a draft URL against the T07 SSRF policy:
///
/// * scheme must be `http`/`https`;
/// * a host must be present;
/// * loopback (`localhost`, `127.*`, `::1`) is allowed ONLY for the `ollama`
///   protocol and for `ollama`/`custom` providers (T07 spec: "localhost 对
///   Ollama/custom 是合法例外");
/// * every other private / link-local / reserved range (10/8, 172.16/12,
///   192.168/16, 169.254/16, CGNAT 100.64/10, 0/8, 224/4, 240/4, ::1,
///   fc00::/7, fe80::/10, IPv4-mapped) is always blocked.
///
/// IP literals are range-checked directly; hostnames are resolved best-effort.
/// On DNS lookup failure we fail OPEN — the probe then simply fails to connect
/// and is reported as a network error, so no internal target becomes reachable.
pub async fn validate_draft_url(
    native_base_url: &str,
    protocol: &str,
    provider: &str,
) -> Result<(), String> {
    let url = reqwest::Url::parse(native_base_url.trim())
        .map_err(|_| "Base URL 不是合法 URL".to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("Base URL 必须为 http(s) 地址".to_string());
    }
    let Some(host) = url.host_str() else {
        return Err("Base URL 缺少主机名".to_string());
    };
    if host.trim().is_empty() {
        return Err("Base URL 缺少主机名".to_string());
    }
    let host_lower = host.to_lowercase();
    let allow_loopback = protocol == "ollama" || provider == "ollama" || provider == "custom";
    // RFC1918 私网段仅对自定义/Ollama 渠道放行（自建/内网网关属自定义渠道）；
    // 云元数据 169.254.169.254、组播与保留段始终拦截。
    let allow_private = allow_loopback;
    let literal_loopback =
        host_lower == "localhost" || host_lower == "::1" || host_lower.starts_with("127.");
    if literal_loopback {
        if allow_loopback {
            return Ok(());
        }
        tracing::warn!(
            host = %host_lower,
            protocol = %protocol,
            provider = %provider,
            "SSRF 拒绝：非 Ollama/自定义渠道指向本机回环地址"
        );
        return Err("SSRF 策略：非 Ollama/自定义渠道不允许指向本机回环地址".to_string());
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => {
            if is_blocked_ip(ip, allow_loopback, allow_private) {
                tracing::warn!(
                    host = %host_lower,
                    ip = %ip,
                    protocol = %protocol,
                    provider = %provider,
                    "SSRF 拒绝：目标为被禁止的私网/保留网段"
                );
                return Err(format!("SSRF 策略：目标 {host} 属于被禁止的私网/保留网段"));
            }
            Ok(())
        }
        Err(_) => {
            let port = url.port_or_known_default().unwrap_or(443);
            let ips = tokio::net::lookup_host((host, port))
                .await
                .map(|it| it.map(|a| a.ip()).collect::<Vec<_>>())
                .unwrap_or_default();
            if ips
                .iter()
                .any(|ip| is_blocked_ip(*ip, allow_loopback, allow_private))
            {
                tracing::warn!(
                    host = %host_lower,
                    protocol = %protocol,
                    provider = %provider,
                    resolved_ips = ?ips,
                    "SSRF 拒绝：域名解析到被禁止的私网/保留网段"
                );
                return Err(format!("SSRF 策略：{host} 解析到被禁止的私网/保留网段"));
            }
            Ok(())
        }
    }
}

fn is_blocked_ip(ip: IpAddr, allow_loopback: bool, allow_private: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let a = v4.octets()[0];
            let b = v4.octets()[1];
            if a == 127 {
                return !allow_loopback;
            }
            // RFC1918 私网段：仅 custom/ollama 放行，其他渠道仍拦截（SSRF）。
            if a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168) {
                return !allow_private;
            }
            // 云元数据 / 链路本地、0/8、广播、组播、保留段：始终拦截。
            if (a == 169 && b == 254)
                || (a == 100 && (64..=127).contains(&b))
                || a == 0
                || a == 255
                || (224..=239).contains(&a)
                || (240..=254).contains(&a)
            {
                return true;
            }
            false
        }
        IpAddr::V6(v6) => {
            if v6 == Ipv6Addr::LOCALHOST {
                return !allow_loopback;
            }
            if v6.is_loopback() {
                return !allow_loopback;
            }
            let segments = v6.segments();
            if segments[0] & 0xfe00 == 0xfc00 {
                return !allow_private; // fc00::/7 ULA，私网段同规则
            }
            if segments[0] & 0xffc0 == 0xfe80 {
                return true; // fe80::/10 link-local
            }
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_ip(IpAddr::V4(v4), allow_loopback, allow_private);
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Probe construction + executor reuse seam
// ---------------------------------------------------------------------------

/// Resolve the effective API key for a draft test.
///
/// The ordering mirrors the save-time resolution in `update_channel` exactly,
/// so the test-time fingerprint matches the save-time fingerprint and the T07
/// save gate can never deadlock on the always-on UI:
///
/// * a non-blank `api_key` wins;
/// * `clear_api_key == Some(true)` (explicit clear) yields the empty key;
/// * an edit (id present) with a blank key that is NOT cleared is filled from
///   the existing channel's stored key — for ALL protocols, Ollama included
///   (a blank key on an Ollama edit means "keep existing", matching the save
///   path, never the empty key);
/// * a create (no id) with a blank key yields the empty key — the Ollama
///   explicit-empty case remains legal, and other protocols simply have no key.
pub async fn resolve_draft_api_key(
    input: &DraftChannelTestInput,
    repo: &Repository,
) -> Result<String, String> {
    if !input.api_key.trim().is_empty() {
        return Ok(input.api_key.clone());
    }
    if input.clear_api_key == Some(true) {
        return Ok(String::new());
    }
    if let Some(id) = input.id.as_deref() {
        if !id.trim().is_empty() {
            let ch = repo
                .get_channel(id)
                .await
                .map_err(|e| format!("读取渠道失败：{e}"))?;
            return Ok(ch.api_key);
        }
    }
    Ok(String::new())
}

/// The minimal upstream probe request per endpoint (stream=false, first model,
/// minimum output).  These are REAL inference requests for Chat / Responses /
/// Messages / /api/chat — never a `/models` substitute (T07 test strategy).
fn probe_body(endpoint: &str, model: &str) -> Value {
    match endpoint {
        "chat_completions" => json!({
            "model": model,
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 1,
            "stream": false,
        }),
        "responses" => json!({
            "model": model,
            "input": "ping",
            "max_output_tokens": 1,
            "stream": false,
        }),
        "messages" => json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "ping" }],
            // 探测显式请求非流式：Anthropic Messages 默认非流式，但显式声明
            // stream:false 可避免部分网关因「客户端未声明」而强制以 SSE 返回。
            "stream": false,
        }),
        "count_tokens" => json!({
            "model": model,
            "messages": [{ "role": "user", "content": "ping" }],
        }),
        "embeddings" => json!({
            "model": model,
            "input": "ping",
        }),
        "api_chat" => json!({
            "model": model,
            "messages": [{ "role": "user", "content": "ping" }],
            "stream": false,
        }),
        _ => json!({
            "model": model,
            "messages": [{ "role": "user", "content": "ping" }],
        }),
    }
}

/// The downstream `EndpointKind` for the probe.  For native probes (codec
/// version `None`) the downstream kind is not used for decoding, so the Ollama
/// `/api/chat` and the legacy Gemini override both map onto `ChatCompletions`.
fn endpoint_kind(endpoint: &str) -> EndpointKind {
    match endpoint {
        "responses" => EndpointKind::Responses,
        "messages" => EndpointKind::Messages,
        "count_tokens" => EndpointKind::CountTokens,
        "embeddings" => EndpointKind::Embeddings,
        _ => EndpointKind::ChatCompletions,
    }
}

/// Build an in-memory `Channel` for the executor.  The T06 executor only reads
/// `api_key` + `timeout_secs` from the channel (URL/auth/version all come from
/// the attempt + identity), so the remaining fields are faithful but inert.
#[allow(clippy::too_many_arguments)]
fn draft_channel(input: &DraftChannelTestInput, api_key: &str, timeout_secs: i64) -> Channel {
    let models = serde_json::to_string(&input.models).unwrap_or_else(|_| "[]".to_string());
    let config = input
        .config
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());
    let model_mapping = input
        .model_mapping
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());
    Channel {
        id: input.id.clone().unwrap_or_else(|| "draft".to_string()),
        name: input.name.clone(),
        channel_type: input.channel_type.clone(),
        base_url: input.base_url.clone(),
        api_key: api_key.to_string(),
        models,
        status: 1,
        priority: input.priority.unwrap_or(0),
        weight: input.weight.unwrap_or(1),
        config,
        model_mapping,
        timeout_secs: timeout_secs.max(1),
        protocol: input.protocol.clone(),
        provider: input.provider.clone(),
        native_base_url: input.native_base_url.clone(),
        native_endpoints: input
            .native_endpoints
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string())),
        preset_revision: input.preset_revision.clone(),
        identity_revision: 1,
        legacy_executor_override: input.legacy_executor_override.clone(),
        created_at: now_iso(),
        updated_at: now_iso(),
        last_test_at: None,
        last_test_ok: None,
    }
}

/// Test timing policy.  The draft test timeout is INDEPENDENT of the channel's
/// production `timeout_secs` and has a total cap (T07 security requirements).
#[derive(Debug, Clone)]
pub struct DraftTestConfig {
    /// Per-endpoint probe timeout (the executor's reqwest client uses this).
    pub per_probe_timeout: Duration,
    /// Extra headroom on the wrapper so the executor's own timeout wins the
    /// classification whenever it fires first.
    pub probe_wait_margin: Duration,
    /// Total cap for the whole run (all endpoints).
    pub total_cap: Duration,
}

impl Default for DraftTestConfig {
    fn default() -> Self {
        Self {
            per_probe_timeout: Duration::from_secs(15),
            probe_wait_margin: Duration::from_secs(5),
            total_cap: Duration::from_secs(60),
        }
    }
}

// ---------------------------------------------------------------------------
// Result classification
// ---------------------------------------------------------------------------

/// Strip the API key from any surfaced message.  The raw key AND its
/// percent-encoded form are both redacted (the legacy Gemini executor embeds
/// `?key=<urlencoded>` in the URL, which a reqwest transport error would echo).
fn sanitize_message(msg: &str, api_key: &str) -> String {
    if api_key.is_empty() {
        return msg.chars().take(300).collect();
    }
    let mut s = msg.to_string();
    s = s.replace(api_key, "***");
    // Minimal form-url encoding matching endpoint_executor::urlencoding.
    let enc = api_key
        .replace('%', "%25")
        .replace('&', "%26")
        .replace('+', "%2B")
        .replace('=', "%3D")
        .replace('?', "%3F");
    if enc != api_key {
        s = s.replace(&enc, "***");
    }
    s.chars().take(300).collect()
}

fn looks_like_model_error(lower: &str) -> bool {
    (lower.contains("model")
        && (lower.contains("not found")
            || lower.contains("does not exist")
            || lower.contains("no such model")
            || lower.contains("invalid model")
            || lower.contains("model_not_found")
            || lower.contains("unknown model")))
        || lower.contains("模型")
}

fn failed_result(
    endpoint: &str,
    model: Option<&str>,
    category: &str,
    message: &str,
    elapsed: Duration,
    api_key: &str,
    cost_possible: bool,
) -> DraftEndpointTestResult {
    DraftEndpointTestResult {
        endpoint: endpoint.to_string(),
        status: "failed".to_string(),
        category: Some(category.to_string()),
        message: sanitize_message(message, api_key),
        latency_ms: elapsed.as_millis() as u64,
        tested_model: model.map(|m| m.to_string()),
        cost_possible,
    }
}

/// Map a T06 `AttemptFailure` onto a draft-test category + sanitized message.
fn classify_failure(
    endpoint: &str,
    model: &str,
    f: AttemptFailure,
    elapsed: Duration,
    api_key: &str,
    config: &DraftTestConfig,
) -> DraftEndpointTestResult {
    let msg = f.message.clone();
    let lower = msg.to_lowercase();
    let (category, base) = match f.failure_class {
        FailureClass::ChannelAuthTerminal => (
            "authentication",
            "鉴权失败：API Key 无效或无权访问该端点".to_string(),
        ),
        FailureClass::EndpointUnsupported => (
            "endpoint_unsupported",
            format!("上游不支持或未开通该端点（{endpoint}）；建议取消勾选后重试"),
        ),
        FailureClass::CallerTerminal => {
            if looks_like_model_error(&lower) {
                (
                    "model",
                    "模型错误：请检查模型名称是否正确、是否有权使用".to_string(),
                )
            } else {
                (
                    "request",
                    "请求被上游拒绝；请检查模型与参数配置".to_string(),
                )
            }
        }
        FailureClass::Retryable => {
            if lower.contains("timed out")
                || lower.contains("timeout")
                || elapsed >= config.per_probe_timeout
            {
                (
                    "timeout",
                    "上游响应超时（测试超时与生产超时相互独立）".to_string(),
                )
            } else {
                (
                    "network",
                    "网络不可达：请检查网络与 Base URL 可达性".to_string(),
                )
            }
        }
        FailureClass::UpstreamProtocolError => (
            "protocol",
            "上游返回了无法解析的响应；请确认该端点提供对应协议".to_string(),
        ),
        FailureClass::CommittedStreamError => {
            ("protocol", "流式提交后出错（草稿测试不应发生）".to_string())
        }
    };
    let detail = sanitize_message(&msg, api_key);
    let full = if detail.is_empty() || detail == base {
        base
    } else {
        format!("{base}（{detail}）")
    };
    DraftEndpointTestResult {
        endpoint: endpoint.to_string(),
        status: "failed".to_string(),
        category: Some(category.to_string()),
        message: full.chars().take(400).collect(),
        latency_ms: elapsed.as_millis() as u64,
        tested_model: Some(model.to_string()),
        cost_possible: true,
    }
}

/// Run one endpoint probe through the T06 executor.
async fn probe_endpoint(
    endpoint: &str,
    identity: &ChannelIdentity,
    channel: &Channel,
    model: &str,
    config: &DraftTestConfig,
) -> DraftEndpointTestResult {
    let started = Instant::now();
    let attempt = PreparedAttempt {
        channel_id: channel.id.clone(),
        channel_name: channel.name.clone(),
        upstream_type: "channel".to_string(),
        route_group: format!("draft_test/{endpoint}"),
        upstream_protocol: identity.protocol.clone(),
        upstream_endpoint: endpoint.to_string(),
        upstream_model: model.to_string(),
        native_base_url: identity.native_base_url.clone(),
        auth_provider: None,
        auth_non_stream_framing: None,
        codec_version: None,
        prepared_codec: None,
        encoded_body: probe_body(endpoint, model),
        conversion_report: None,
        is_retry: false,
        attempt_no: 1,
    };
    let outcome = tokio::time::timeout(
        config.per_probe_timeout + config.probe_wait_margin,
        dispatch_executor(
            endpoint_kind(endpoint),
            &attempt,
            channel,
            identity,
            &[],
            None,
        ),
    )
    .await;
    let elapsed = started.elapsed();
    match outcome {
        Err(_) => failed_result(
            endpoint,
            Some(model),
            "timeout",
            &format!("测试超时：{:?} 内未收到上游响应", config.per_probe_timeout),
            config.per_probe_timeout,
            &channel.api_key,
            true,
        ),
        Ok(AttemptResult::Success(_)) => DraftEndpointTestResult {
            endpoint: endpoint.to_string(),
            status: "passed".to_string(),
            category: None,
            message: "连接成功：上游已返回 200 响应".to_string(),
            latency_ms: elapsed.as_millis() as u64,
            tested_model: Some(model.to_string()),
            cost_possible: true,
        },
        Ok(AttemptResult::Failure(f)) => {
            classify_failure(endpoint, model, f, elapsed, &channel.api_key, config)
        }
    }
}

// ---------------------------------------------------------------------------
// Receipt store + force-save validation
// ---------------------------------------------------------------------------

/// An in-process test-run receipt.  Short-lived (TTL), keyed by `test_run_id`,
/// bound to exactly ONE `draft_fingerprint`.  Process restart clears the store
/// → every receipt expires and saving requires re-testing.
#[derive(Debug, Clone)]
pub struct StoredReceipt {
    pub test_run_id: String,
    pub draft_fingerprint: String,
    pub tested_at: String,
    pub expires_at: Instant,
    pub all_passed: bool,
    pub endpoint_count: usize,
}

/// Short-lived, in-process store of test-run receipts.
pub struct TestReceiptStore {
    inner: Mutex<HashMap<String, StoredReceipt>>,
    ttl: Duration,
}

impl TestReceiptStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn store(&self, receipt: StoredReceipt) {
        self.inner
            .lock()
            .unwrap()
            .insert(receipt.test_run_id.clone(), receipt);
    }

    /// Look up a receipt, dropping it if expired.
    pub fn lookup(&self, test_run_id: &str) -> Option<StoredReceipt> {
        let mut guard = self.inner.lock().unwrap();
        let expired = guard
            .get(test_run_id)
            .map(|r| r.expires_at < Instant::now())
            .unwrap_or(false);
        if expired {
            guard.remove(test_run_id);
            return None;
        }
        guard.get(test_run_id).cloned()
    }
}

/// Outcome of a successful save-receipt validation.
#[derive(Debug, Clone)]
pub struct SaveReceiptCheck {
    pub all_passed: bool,
    pub tested_at: String,
}

/// Validate the save-time receipt carried by the new UI payload.
///
/// * Legacy payload without `test_run_id` → `Ok(None)` (old compat path, no
///   test gating).
/// * Receipt must exist, not be expired, and its fingerprint must equal BOTH
///   the frontend-returned fingerprint AND the fingerprint recomputed from the
///   current draft (single-draft consistency + "草稿已改变" invalidation).
/// * A normal save requires every selected endpoint to have passed;
///   `force_save` only requires the same draft to have completed one test.
pub fn validate_save_receipt(
    store: &TestReceiptStore,
    test_run_id: Option<&str>,
    received_fingerprint: Option<&str>,
    computed_fingerprint: &str,
    force_save: bool,
) -> Result<Option<SaveReceiptCheck>, String> {
    let Some(run_id) = test_run_id else {
        return Ok(None);
    };
    let Some(recv_fp) = received_fingerprint else {
        return Err("缺少 draft_fingerprint，无法校验测试记录".to_string());
    };
    let Some(receipt) = store.lookup(run_id) else {
        return Err("测试记录不存在或已过期，请重新测试后再保存".to_string());
    };
    if recv_fp != computed_fingerprint || receipt.draft_fingerprint != computed_fingerprint {
        return Err(
            "草稿已改变（protocol/provider/URL/模型/端点/Key/timeout 与测试时不一致），请重新测试后再保存"
                .to_string(),
        );
    }
    if !force_save && !receipt.all_passed {
        return Err(
            "存在未通过的端点测试；如确认仍要保存，请点击「仍然保存」（强制保存）".to_string(),
        );
    }
    Ok(Some(SaveReceiptCheck {
        all_passed: receipt.all_passed,
        tested_at: receipt.tested_at.clone(),
    }))
}

// ---------------------------------------------------------------------------
// Test runner
// ---------------------------------------------------------------------------

/// Run the full draft test.  Never writes to the DB, never counts quota, never
/// writes a production request log — it only probes the real upstream endpoints
/// (which may incur negligible cost) and records a short-lived in-process
/// receipt for the save step.
pub async fn run_draft_test(
    input: &DraftChannelTestInput,
    api_key: &str,
    store: &TestReceiptStore,
    config: &DraftTestConfig,
) -> Result<DraftChannelTestResult, String> {
    let protocol = input
        .protocol
        .as_deref()
        .unwrap_or("openai")
        .trim()
        .to_string();
    let provider = input
        .provider
        .as_deref()
        .unwrap_or("custom")
        .trim()
        .to_string();

    let identity = build_draft_identity(
        &input.protocol,
        &input.provider,
        &input.native_base_url,
        &input.native_endpoints,
        &input.channel_type,
        &input.base_url,
        input
            .config
            .as_ref()
            .unwrap_or(&Value::Object(Default::default())),
        &input.legacy_executor_override,
    );
    if identity.native_base_url.trim().is_empty() {
        return Err("Base URL 不能为空，无法测试".to_string());
    }
    validate_draft_url(&identity.native_base_url, &protocol, &provider).await?;

    let gemini_override = identity.legacy_executor_override.as_deref() == Some("gemini_native");
    let mut endpoints: Vec<String> = identity.native_endpoints.clone();
    if gemini_override && endpoints.is_empty() {
        // Legacy Gemini has no native endpoint list; probe the override executor.
        endpoints.push("chat_completions".to_string());
    }
    // count_tokens 是 T06 legacy 推断附加的计费/规划能力端点（对 legacy claude
    // 注入 ["messages","count_tokens"]），不是连通性端点：探测 /messages 已足以
    // 证明网关可达，故保存前草稿测试绝不请求 /messages/count_tokens。指纹（下方
    // compute_draft_fingerprint）仍覆盖完整端点集，因此测试与保存两侧指纹一致，
    // 保存时 receipt 校验不会被误判为「草稿已改变」。
    endpoints.retain(|ep| ep != "count_tokens");

    // OpenAI local rejection (T07): at least one of Chat / Responses must be
    // selected, otherwise reject WITHOUT testing.  The check honors the draft's
    // explicit endpoint list when present (an explicitly-empty list is a
    // rejection, never silently re-inferred).
    if protocol == "openai" && !gemini_override {
        let selected: Vec<String> = input
            .native_endpoints
            .clone()
            .unwrap_or_else(|| endpoints.clone());
        if !selected
            .iter()
            .any(|e| e == "chat_completions" || e == "responses")
        {
            return Err(
                "OpenAI 协议必须至少勾选 Chat Completions 或 Responses 才能测试".to_string(),
            );
        }
    }

    let models = normalize_models(&input.models);
    let channel = draft_channel(input, api_key, config.per_probe_timeout.as_secs() as i64);

    let probe_loop = async {
        let mut results = Vec::with_capacity(endpoints.len());
        for endpoint in &endpoints {
            if models.is_empty() {
                results.push(DraftEndpointTestResult {
                    endpoint: endpoint.clone(),
                    status: "skipped".to_string(),
                    category: None,
                    message: "未验证：未选择模型，无法执行推理探测；建议先添加模型后重试"
                        .to_string(),
                    latency_ms: 0,
                    tested_model: None,
                    cost_possible: false,
                });
                continue;
            }
            results.push(probe_endpoint(endpoint, &identity, &channel, &models[0], config).await);
        }
        results
    };

    let results = match tokio::time::timeout(config.total_cap, probe_loop).await {
        Ok(r) => r,
        Err(_) => {
            let mut results = Vec::with_capacity(endpoints.len());
            for endpoint in &endpoints {
                results.push(DraftEndpointTestResult {
                    endpoint: endpoint.clone(),
                    status: "skipped".to_string(),
                    category: Some("timeout".to_string()),
                    message: "测试总时间超过上限，已跳过后续端点".to_string(),
                    latency_ms: 0,
                    tested_model: models.first().cloned(),
                    cost_possible: false,
                });
            }
            results
        }
    };

    let fingerprint = compute_draft_fingerprint(
        &identity.protocol,
        &identity.provider,
        &identity.native_base_url,
        &identity.native_endpoints,
        &models,
        input.timeout_secs.unwrap_or(60),
        api_key,
    );
    let test_run_id = uuid::Uuid::new_v4().to_string();
    let tested_at = now_iso();
    let all_passed = !results.is_empty() && results.iter().all(|r| r.status == "passed");

    store.store(StoredReceipt {
        test_run_id: test_run_id.clone(),
        draft_fingerprint: fingerprint.clone(),
        tested_at: tested_at.clone(),
        expires_at: Instant::now() + store.ttl(),
        all_passed,
        endpoint_count: results.len(),
    });

    Ok(DraftChannelTestResult {
        draft_fingerprint: fingerprint,
        tested_at,
        test_run_id,
        results,
    })
}

// ---------------------------------------------------------------------------
// Tests (mock upstream; never contacts a real paid endpoint)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod channel_draft_test {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc as StdArc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex as TokioMutex;

    #[derive(Debug, Clone)]
    struct CapturedRequest {
        path_and_query: String,
        headers: Vec<(String, String)>,
    }

    struct MockUpstream {
        addr: std::net::SocketAddr,
        received: StdArc<TokioMutex<Vec<CapturedRequest>>>,
        _handle: tokio::task::JoinHandle<()>,
    }

    /// Boot a mock upstream. `handler(path_and_query) -> (status, body)`.
    /// The handler is async so a test can delay a response (`tokio::time::sleep`)
    /// to exercise the timeout path without blocking the runtime.
    async fn start_mock<H>(handler: H) -> MockUpstream
    where
        H: Fn(&str) -> Pin<Box<dyn Future<Output = (u16, Vec<u8>)> + Send + 'static>>
            + Send
            + Sync
            + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let received = StdArc::new(TokioMutex::new(Vec::new()));
        let recv = received.clone();
        let handler = StdArc::new(handler);
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let recv = recv.clone();
                let handler = handler.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let mut header_end = None;
                    let mut content_length = 0usize;
                    loop {
                        match socket.read(&mut tmp).await {
                            Ok(0) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&tmp[..n]);
                                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                    header_end = Some(pos);
                                    let block = String::from_utf8_lossy(&buf[..pos]).to_string();
                                    for line in block.split("\r\n") {
                                        if let Some((k, v)) = line.split_once(':') {
                                            if k.trim().eq_ignore_ascii_case("content-length") {
                                                content_length = v.trim().parse().unwrap_or(0);
                                            }
                                        }
                                    }
                                    break;
                                }
                            }
                            Err(_) => return,
                        }
                    }
                    let Some(header_end) = header_end else { return };
                    let request_line = String::from_utf8_lossy(&buf[..header_end])
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let path_and_query = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .to_string();
                    let body_start = header_end + 4;
                    while buf.len() < body_start + content_length {
                        match socket.read(&mut tmp).await {
                            Ok(0) => break,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                            Err(_) => return,
                        }
                    }
                    let block = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let mut headers = Vec::new();
                    for line in block.split("\r\n").skip(1) {
                        if let Some((k, v)) = line.split_once(':') {
                            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
                        }
                    }
                    let (status, resp_body) = handler(&path_and_query).await;
                    recv.lock().await.push(CapturedRequest {
                        path_and_query,
                        headers,
                    });
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let _ = socket
                        .write_all(
                            format!(
                                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                resp_body.len()
                            )
                            .as_bytes(),
                        )
                        .await;
                    let _ = socket.write_all(&resp_body).await;
                });
            }
        });
        MockUpstream {
            addr,
            received,
            _handle: handle,
        }
    }

    impl MockUpstream {
        async fn captured(&self) -> Vec<CapturedRequest> {
            self.received.lock().await.clone()
        }
    }

    fn openai_chat_success() -> Vec<u8> {
        br#"{"id":"chatcmpl-1","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#.to_vec()
    }

    fn openai_responses_success() -> Vec<u8> {
        br#"{"id":"resp_1","object":"response","output":[],"model":"gpt-4o","status":"completed"}"#
            .to_vec()
    }

    fn anthropic_messages_success() -> Vec<u8> {
        br#"{"type":"message","id":"msg_1","role":"assistant","model":"claude-sonnet-4-6","content":[{"type":"text","text":"hi"}],"stop_reason":"end_turn","usage":{"input_tokens":4,"output_tokens":2}}"#.to_vec()
    }

    fn store() -> TestReceiptStore {
        TestReceiptStore::new(Duration::from_secs(600))
    }

    fn cfg() -> DraftTestConfig {
        DraftTestConfig {
            per_probe_timeout: Duration::from_millis(500),
            probe_wait_margin: Duration::from_millis(200),
            total_cap: Duration::from_secs(10),
        }
    }

    fn draft(
        protocol: &str,
        provider: &str,
        base: &str,
        endpoints: &[&str],
        models: &[&str],
        key: &str,
    ) -> DraftChannelTestInput {
        DraftChannelTestInput {
            id: None,
            name: "test-channel".to_string(),
            channel_type: match protocol {
                "anthropic" => "claude".to_string(),
                _ => "openai".to_string(),
            },
            base_url: base.to_string(),
            api_key: key.to_string(),
            clear_api_key: None,
            models: models.iter().map(|s| s.to_string()).collect(),
            priority: None,
            weight: None,
            config: None,
            model_mapping: None,
            timeout_secs: Some(30),
            protocol: Some(protocol.to_string()),
            provider: Some(provider.to_string()),
            native_base_url: Some(base.to_string()),
            native_endpoints: Some(endpoints.iter().map(|s| s.to_string()).collect()),
            preset_revision: None,
            legacy_executor_override: None,
        }
    }

    // --- dual-endpoint / per-endpoint probes ---

    #[tokio::test]
    async fn openai_dual_endpoint_produces_two_independent_results() {
        let mock = start_mock(|path: &str| {
            if path.ends_with("/chat/completions") {
                Box::pin(async move { (200, openai_chat_success()) })
            } else if path.ends_with("/responses") {
                Box::pin(async move { (200, openai_responses_success()) })
            } else {
                Box::pin(async move { (404, br#"{"error":{"message":"not found"}}"#.to_vec()) })
            }
        })
        .await;
        let base = format!("http://{}/v1", mock.addr);
        let input = draft(
            "openai",
            "custom",
            &base,
            &["chat_completions", "responses"],
            &["gpt-4o"],
            "sk-test",
        );
        let store = store();
        let result = run_draft_test(&input, "sk-test", &store, &cfg())
            .await
            .unwrap();
        assert_eq!(result.results.len(), 2);
        let chat = result
            .results
            .iter()
            .find(|r| r.endpoint == "chat_completions")
            .unwrap();
        let resp = result
            .results
            .iter()
            .find(|r| r.endpoint == "responses")
            .unwrap();
        assert_eq!(chat.status, "passed");
        assert_eq!(resp.status, "passed");
        assert!(chat.cost_possible && resp.cost_possible);
        let captured = mock.captured().await;
        assert_eq!(
            captured.len(),
            2,
            "each endpoint must be probed independently"
        );
        assert!(captured
            .iter()
            .any(|r| r.path_and_query == "/v1/chat/completions"));
        assert!(captured.iter().any(|r| r.path_and_query == "/v1/responses"));
        // Receipt is stored and matches the returned fingerprint.
        let receipt = store.lookup(&result.test_run_id).unwrap();
        assert!(receipt.all_passed);
        assert_eq!(receipt.draft_fingerprint, result.draft_fingerprint);
    }

    #[tokio::test]
    async fn anthropic_messages_probe_uses_version_header_and_x_api_key() {
        let mock = start_mock(|_path: &str| {
            let body = anthropic_messages_success();
            Box::pin(async move { (200, body) })
        })
        .await;
        // base 带 /v1（main 分支约定），executor 端点只补 /messages → 最终 /v1/messages。
        let base = format!("http://{}/v1", mock.addr);
        // provider=custom keeps the loopback mock legal under the SSRF policy
        // while still exercising the anthropic protocol's x-api-key + version
        // header wiring (auth scheme is derived from the protocol).
        let input = draft(
            "anthropic",
            "custom",
            &base,
            &["messages"],
            &["claude-sonnet-4-6"],
            "sk-ant-xyz",
        );
        let result = run_draft_test(&input, "sk-ant-xyz", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(result.results[0].status, "passed");
        let captured = mock.captured().await;
        assert_eq!(captured[0].path_and_query, "/v1/messages");
        assert!(captured[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "sk-ant-xyz"));
        assert!(captured[0]
            .headers
            .iter()
            .any(|(k, v)| k == "anthropic-version" && v == "2023-06-01"));
    }

    #[tokio::test]
    async fn anthropic_draft_skips_count_tokens_probe() {
        // count_tokens 是 T06 legacy 推断附加的能力端点，不是连通性端点：
        // 保存前草稿测试只探测 /messages，绝不请求 /messages/count_tokens；
        // 指纹仍覆盖完整端点集（messages + count_tokens），保证保存时
        // receipt 校验（test/save 两侧指纹一致）通过。
        let mock = start_mock(|_path: &str| {
            let body = anthropic_messages_success();
            Box::pin(async move { (200, body) })
        })
        .await;
        let base = format!("http://{}/v1", mock.addr);
        let input = draft(
            "anthropic",
            "custom",
            &base,
            &["messages", "count_tokens"],
            &["claude-sonnet-4-6"],
            "sk-ant-xyz",
        );
        let result = run_draft_test(&input, "sk-ant-xyz", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1, "count_tokens 必须被排除出探测列表");
        assert_eq!(result.results[0].endpoint, "messages");
        let captured = mock.captured().await;
        assert_eq!(captured.len(), 1, "count_tokens must not be probed");
        assert_eq!(captured[0].path_and_query, "/v1/messages");
        // 指纹仍覆盖完整端点集（含 count_tokens），与保存路径一致。
        let full = compute_draft_fingerprint(
            "anthropic",
            "custom",
            base.trim_end_matches('/'),
            &["messages".to_string(), "count_tokens".to_string()],
            &["claude-sonnet-4-6".to_string()],
            30,
            "sk-ant-xyz",
        );
        assert_eq!(result.draft_fingerprint, full);
    }

    #[tokio::test]
    async fn anthropic_2xx_non_json_body_is_protocol_error() {
        // 复现 Bug 1：上游返回 2xx 但 body 无法解析为 JSON（如 HTML 拦截页、
        // 空 body 或未解压的 gzip 字节），必须归类为 protocol 错误，且诊断
        // 日志要能抓到 Content-Type / body 摘要，而不是只显示一行通用错误。
        let mock = start_mock(|_path: &str| {
            Box::pin(async move { (200, b"<html><body>Bad Gateway</body></html>".to_vec()) })
        })
        .await;
        let base = format!("http://{}", mock.addr);
        let input = draft(
            "anthropic",
            "custom",
            &base,
            &["messages"],
            &["claude-sonnet-4-6"],
            "sk-ant-xyz",
        );
        let result = run_draft_test(&input, "sk-ant-xyz", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(result.results[0].status, "failed");
        assert_eq!(result.results[0].category.as_deref(), Some("protocol"));
        assert!(
            result.results[0].message.contains("无法解析"),
            "should surface the protocol error: {}",
            result.results[0].message
        );
    }

    #[tokio::test]
    async fn anthropic_gateway_that_always_streams_probe_passes() {
        // Bug 1 真实场景：探测请求 `stream:false`，但网关忽略该字段、总是以
        // SSE 流返回（Anthropic 合法流式格式）。探测不得因为 body 是 SSE 帧
        // 而非单条 JSON 就报协议错误 —— 应提取首个 data: JSON 帧判定为通过。
        let sse_body = br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","role":"assistant","model":"big-pickle","content":[]}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}

event: message_stop
data: {"type":"message_stop"}

"#;
        let mock = start_mock(|_path: &str| {
            let body = sse_body.to_vec();
            Box::pin(async move { (200, body) })
        })
        .await;
        let base = format!("http://{}", mock.addr);
        let input = draft(
            "anthropic",
            "custom",
            &base,
            &["messages"],
            &["big-pickle"],
            "sk-ant-xyz",
        );
        let result = run_draft_test(&input, "sk-ant-xyz", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(
            result.results[0].status, "passed",
            "SSE-only Anthropic gateway must pass the probe: {}",
            result.results[0].message
        );
    }

    #[tokio::test]
    async fn ollama_empty_key_is_legal() {
        let mock = start_mock(|_path: &str| {
            Box::pin(async move {
                (200, br#"{"model":"llama3.1","message":{"role":"assistant","content":"hi"},"done":true}"#.to_vec())
            })
        })
        .await;
        let base = format!("http://{}", mock.addr);
        let input = draft("ollama", "ollama", &base, &["api_chat"], &["llama3.1"], "");
        let result = run_draft_test(&input, "", &store(), &cfg()).await.unwrap();
        assert_eq!(result.results[0].status, "passed");
        let captured = mock.captured().await;
        assert_eq!(captured[0].path_and_query, "/api/chat");
        assert!(
            !captured[0]
                .headers
                .iter()
                .any(|(k, _)| k == "authorization"),
            "empty Ollama key must send no auth header"
        );
    }

    // --- failure classification ---

    #[tokio::test]
    async fn auth_failure_is_authentication() {
        let mock = start_mock(|_path: &str| {
            Box::pin(async move { (401, br#"{"error":{"message":"invalid api key"}}"#.to_vec()) })
        })
        .await;
        let base = format!("http://{}/v1", mock.addr);
        let input = draft(
            "openai",
            "custom",
            &base,
            &["chat_completions"],
            &["gpt-4o"],
            "sk-wrong",
        );
        let result = run_draft_test(&input, "sk-wrong", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(result.results[0].status, "failed");
        assert_eq!(
            result.results[0].category.as_deref(),
            Some("authentication")
        );
        assert!(
            !result.results[0].message.contains("sk-wrong"),
            "message must not leak the API key"
        );
    }

    #[tokio::test]
    async fn path_missing_404_is_endpoint_unsupported() {
        let mock = start_mock(|_path: &str| {
            Box::pin(async move {
                (
                    404,
                    br#"{"error":{"message":"endpoint does not exist"}}"#.to_vec(),
                )
            })
        })
        .await;
        let base = format!("http://{}/v1", mock.addr);
        let input = draft(
            "openai",
            "custom",
            &base,
            &["responses"],
            &["gpt-4o"],
            "sk-test",
        );
        let result = run_draft_test(&input, "sk-test", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(
            result.results[0].category.as_deref(),
            Some("endpoint_unsupported")
        );
    }

    #[tokio::test]
    async fn slow_upstream_is_timeout() {
        let mock = start_mock(|_path: &str| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                (200, openai_chat_success())
            })
        })
        .await;
        let base = format!("http://{}/v1", mock.addr);
        let input = draft(
            "openai",
            "custom",
            &base,
            &["chat_completions"],
            &["gpt-4o"],
            "sk-test",
        );
        let result = run_draft_test(&input, "sk-test", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(result.results[0].status, "failed");
        assert_eq!(result.results[0].category.as_deref(), Some("timeout"));
    }

    #[tokio::test]
    async fn model_error_400_is_model_category() {
        let mock = start_mock(|_path: &str| {
            Box::pin(async move {
                (
                    400,
                    br#"{"error":{"message":"The model 'gpt-4o' does not exist"}}"#.to_vec(),
                )
            })
        })
        .await;
        let base = format!("http://{}/v1", mock.addr);
        let input = draft(
            "openai",
            "custom",
            &base,
            &["chat_completions"],
            &["gpt-4o"],
            "sk-test",
        );
        let result = run_draft_test(&input, "sk-test", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(result.results[0].category.as_deref(), Some("model"));
    }

    // --- local rejection / skip ---

    #[tokio::test]
    async fn openai_no_chat_or_responses_is_local_rejection() {
        let store = store();
        let input = draft(
            "openai",
            "custom",
            "http://127.0.0.1:1/v1",
            &[],
            &["gpt-4o"],
            "sk-test",
        );
        let err = run_draft_test(&input, "sk-test", &store, &cfg())
            .await
            .unwrap_err();
        assert!(err.contains("至少勾选"), "local rejection message: {err}");
    }

    #[tokio::test]
    async fn empty_models_skips_without_probe() {
        let mock =
            start_mock(|_path: &str| Box::pin(async move { (200, openai_chat_success()) })).await;
        let base = format!("http://{}/v1", mock.addr);
        let input = draft(
            "openai",
            "custom",
            &base,
            &["chat_completions"],
            &[],
            "sk-test",
        );
        let result = run_draft_test(&input, "sk-test", &store(), &cfg())
            .await
            .unwrap();
        assert_eq!(result.results[0].status, "skipped");
        assert!(!result.results[0].cost_possible);
        assert!(
            mock.captured().await.is_empty(),
            "no probe must be sent without a model"
        );
    }

    // --- fingerprint ---

    #[test]
    fn fingerprint_is_deterministic_and_never_leaks_key() {
        let fp1 = compute_draft_fingerprint(
            "openai",
            "custom",
            "https://api.openai.com/v1",
            &["chat_completions".to_string()],
            &["gpt-4o".to_string()],
            30,
            "sk-super-secret-1234567890",
        );
        assert_eq!(fp1.len(), 64);
        assert!(
            !fp1.contains("sk-super-secret"),
            "plaintext key must never appear"
        );
        // Deterministic: same inputs → same fingerprint.
        let fp2 = compute_draft_fingerprint(
            "openai",
            "custom",
            "https://api.openai.com/v1/", // trailing slash normalized
            &["chat_completions".to_string()],
            &["gpt-4o".to_string()],
            30,
            "sk-super-secret-1234567890",
        );
        assert_eq!(fp1, fp2);
        // Different key → different fingerprint.
        let fp3 = compute_draft_fingerprint(
            "openai",
            "custom",
            "https://api.openai.com/v1",
            &["chat_completions".to_string()],
            &["gpt-4o".to_string()],
            30,
            "sk-other-key",
        );
        assert_ne!(fp1, fp3);
        // A connection field change invalidates the fingerprint.
        let fp_endpoint = compute_draft_fingerprint(
            "openai",
            "custom",
            "https://api.openai.com/v1",
            &["responses".to_string()],
            &["gpt-4o".to_string()],
            30,
            "sk-super-secret-1234567890",
        );
        assert_ne!(fp1, fp_endpoint, "endpoint set change must invalidate");
    }

    #[test]
    fn fingerprint_uses_identity_resolution_for_legacy_inference() {
        // An Anthropic legacy root (base contains /v1) is normalized so the
        // save-time fingerprint matches the test-time one after identity build.
        let fp = fingerprint_for_draft(
            None,
            None,
            None,
            None,
            &["m".to_string()],
            30,
            "k",
            "claude",
            "https://api.anthropic.com/v1",
            &json!({}),
            None,
        );
        assert!(!fp.is_empty());
    }

    // --- SSRF ---

    #[tokio::test]
    async fn ssrf_blocks_private_and_non_http() {
        // Private IP blocked for a non-custom provider.
        assert!(
            validate_draft_url("http://192.168.1.1/v1", "openai", "openai")
                .await
                .is_err()
        );
        // Loopback blocked for a non-custom/non-ollama provider.
        assert!(
            validate_draft_url("http://127.0.0.1:11434/v1", "openai", "openai")
                .await
                .is_err()
        );
        // Link-local (cloud metadata) always blocked.
        assert!(validate_draft_url(
            "http://169.254.169.254/latest/meta-data",
            "openai",
            "custom"
        )
        .await
        .is_err());
        // Non-http scheme rejected.
        assert!(
            validate_draft_url("ftp://example.com/v1", "openai", "custom")
                .await
                .is_err()
        );
        // Missing host rejected (empty authority).
        assert!(validate_draft_url("http://:80/v1", "openai", "custom")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn ssrf_allows_loopback_for_ollama_and_custom() {
        assert!(
            validate_draft_url("http://localhost:11434", "ollama", "ollama")
                .await
                .is_ok()
        );
        assert!(
            validate_draft_url("http://127.0.0.1:8000/v1", "openai", "custom")
                .await
                .is_ok()
        );
        assert!(
            validate_draft_url("http://localhost:11434/v1", "openai", "ollama")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn ssrf_allows_private_lan_for_ollama_and_custom() {
        // 内网（hosts 域名解析到私网段 / 直接私网 IP）对 custom/ollama 放行。
        assert!(
            validate_draft_url("http://192.168.1.10/v1", "openai", "custom")
                .await
                .is_ok()
        );
        assert!(validate_draft_url("http://10.0.0.5/v1", "openai", "custom")
            .await
            .is_ok());
        assert!(
            validate_draft_url("http://172.16.3.7/v1", "openai", "ollama")
                .await
                .is_ok()
        );
        // 非 custom/ollama 渠道仍拦截私网。
        assert!(
            validate_draft_url("http://192.168.1.10/v1", "openai", "openai")
                .await
                .is_err()
        );
        // CGNAT / 云元数据 / 组播始终拦截，即使 custom 也不放行。
        assert!(
            validate_draft_url("http://100.64.0.1/v1", "openai", "custom")
                .await
                .is_err()
        );
        assert!(validate_draft_url(
            "http://169.254.169.254/latest/meta-data",
            "openai",
            "custom"
        )
        .await
        .is_err());
        assert!(
            validate_draft_url("http://224.0.0.1/v1", "openai", "custom")
                .await
                .is_err()
        );
    }

    // --- force-save / receipt ---

    #[test]
    fn force_save_receipt_validation_rules() {
        let store = TestReceiptStore::new(Duration::from_secs(600));
        let fp = "fp-1";
        store.store(StoredReceipt {
            test_run_id: "r1".into(),
            draft_fingerprint: fp.into(),
            tested_at: "2026-08-05T00:00:00.000Z".into(),
            expires_at: Instant::now() + Duration::from_secs(600),
            all_passed: false,
            endpoint_count: 2,
        });

        // Normal save with a failed test → reject.
        let err = validate_save_receipt(&store, Some("r1"), Some(fp), fp, false).unwrap_err();
        assert!(err.contains("未通过"), "{err}");
        // Force-save with the same draft → allowed (tested at least once).
        let check = validate_save_receipt(&store, Some("r1"), Some(fp), fp, true)
            .unwrap()
            .expect("force save with valid receipt");
        assert!(!check.all_passed);
        // Frontend fingerprint does not match → reject.
        let err =
            validate_save_receipt(&store, Some("r1"), Some("different"), fp, true).unwrap_err();
        assert!(err.contains("草稿已改变"), "{err}");
        // Recomputed fingerprint differs (draft changed) → reject.
        let err = validate_save_receipt(&store, Some("r1"), Some(fp), "fp-draft-changed", true)
            .unwrap_err();
        assert!(err.contains("草稿已改变"), "{err}");
        // Unknown run id → reject (expired / never tested).
        let err = validate_save_receipt(&store, Some("nope"), Some(fp), fp, true).unwrap_err();
        assert!(err.contains("过期"), "{err}");
        // Missing draft_fingerprint → reject.
        let err = validate_save_receipt(&store, Some("r1"), None, fp, true).unwrap_err();
        assert!(err.contains("draft_fingerprint"), "{err}");
        // Legacy payload without test_run_id → Ok(None).
        assert!(validate_save_receipt(&store, None, None, fp, false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn expired_receipt_is_invalidated() {
        let store = TestReceiptStore::new(Duration::from_secs(600));
        store.store(StoredReceipt {
            test_run_id: "r2".into(),
            draft_fingerprint: "fp".into(),
            tested_at: "t".into(),
            expires_at: Instant::now() - Duration::from_secs(1),
            all_passed: true,
            endpoint_count: 1,
        });
        let err = validate_save_receipt(&store, Some("r2"), Some("fp"), "fp", true).unwrap_err();
        assert!(err.contains("过期"), "{err}");
        assert!(
            store.lookup("r2").is_none(),
            "expired receipt must be dropped"
        );
    }

    #[test]
    fn sanitize_redacts_raw_and_urlencoded_key() {
        let msg = "error sending request for url (https://x/generateContent?key=k%26ey&alt=sse)";
        let out = sanitize_message(msg, "k&ey");
        assert!(
            !out.contains("k%26ey"),
            "urlencoded key must be redacted: {out}"
        );
        assert!(!out.contains("k&ey"));
        assert!(out.contains("***"));
        // Upstream body that echoes the raw key is also redacted.
        let echoed = sanitize_message("bad key: sk-abcdef123456", "sk-abcdef123456");
        assert!(!echoed.contains("sk-abcdef123456"));
    }

    #[test]
    fn all_passed_receipt_allows_normal_save() {
        let store = TestReceiptStore::new(Duration::from_secs(600));
        store.store(StoredReceipt {
            test_run_id: "r3".into(),
            draft_fingerprint: "fp".into(),
            tested_at: "t".into(),
            expires_at: Instant::now() + Duration::from_secs(600),
            all_passed: true,
            endpoint_count: 1,
        });
        let check = validate_save_receipt(&store, Some("r3"), Some("fp"), "fp", false)
            .unwrap()
            .expect("all-passed normal save");
        assert!(check.all_passed);
    }

    // --- draft API key resolution (mirrors the save path; Ollama edit fix) ---

    async fn fresh_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory db");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate fresh db");
        pool
    }

    /// A stored Ollama channel whose key must be reused when the draft leaves
    /// the key blank without an explicit clear (the deadlock scenario).
    fn stored_ollama_channel(id: &str, key: &str) -> Channel {
        Channel {
            id: id.to_string(),
            name: format!("ch-{id}"),
            channel_type: "openai".to_string(),
            base_url: "http://localhost:11434".to_string(),
            api_key: key.to_string(),
            models: "[]".to_string(),
            status: 1,
            priority: 0,
            weight: 1,
            config: "{}".to_string(),
            model_mapping: "{}".to_string(),
            timeout_secs: 30,
            protocol: Some("ollama".to_string()),
            provider: Some("ollama".to_string()),
            native_base_url: Some("http://localhost:11434".to_string()),
            native_endpoints: Some("[\"api_chat\"]".to_string()),
            preset_revision: Some("2026-08-04".to_string()),
            identity_revision: 1,
            legacy_executor_override: None,
            created_at: now_iso(),
            updated_at: now_iso(),
            last_test_at: None,
            last_test_ok: None,
        }
    }

    async fn insert_channel(pool: &sqlx::SqlitePool, c: &Channel) {
        sqlx::query(
            "INSERT INTO channels (id, name, type, base_url, api_key, models, status, priority, weight, config, model_mapping, timeout_secs, protocol, provider, native_base_url, native_endpoints, preset_revision, identity_revision, legacy_executor_override, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
        )
        .bind(&c.id)
        .bind(&c.name)
        .bind(&c.channel_type)
        .bind(&c.base_url)
        .bind(&c.api_key)
        .bind(&c.models)
        .bind(c.status)
        .bind(c.priority)
        .bind(c.weight)
        .bind(&c.config)
        .bind(&c.model_mapping)
        .bind(c.timeout_secs)
        .bind(&c.protocol)
        .bind(&c.provider)
        .bind(&c.native_base_url)
        .bind(&c.native_endpoints)
        .bind(&c.preset_revision)
        .bind(c.identity_revision)
        .bind(&c.legacy_executor_override)
        .bind(&c.created_at)
        .bind(&c.updated_at)
        .execute(pool)
        .await
        .expect("insert channel");
    }

    #[tokio::test]
    async fn draft_key_ollama_edit_blank_not_cleared_uses_stored_key() {
        let pool = fresh_pool().await;
        insert_channel(&pool, &stored_ollama_channel("ch-ollama-1", "sk-stored")).await;
        let repo = Repository::new(pool);
        // Ollama edit, blank key, clear NOT requested (clear_api_key absent).
        let mut input = draft(
            "ollama",
            "ollama",
            "http://localhost:11434",
            &["api_chat"],
            &["llama3.1"],
            "",
        );
        input.id = Some("ch-ollama-1".to_string());
        let key = resolve_draft_api_key(&input, &repo).await.unwrap();
        assert_eq!(
            key, "sk-stored",
            "Ollama edit + blank key + not cleared must reuse the stored key \
             (mirrors the save path), not the empty key"
        );
    }

    #[tokio::test]
    async fn draft_key_ollama_edit_blank_explicit_clear_is_empty() {
        let pool = fresh_pool().await;
        insert_channel(&pool, &stored_ollama_channel("ch-ollama-2", "sk-stored")).await;
        let repo = Repository::new(pool);
        // Ollama edit, blank key, explicit clear requested.
        let mut input = draft(
            "ollama",
            "ollama",
            "http://localhost:11434",
            &["api_chat"],
            &["llama3.1"],
            "",
        );
        input.id = Some("ch-ollama-2".to_string());
        input.clear_api_key = Some(true);
        let key = resolve_draft_api_key(&input, &repo).await.unwrap();
        assert_eq!(
            key, "",
            "explicit clear must yield the empty key even though a stored key exists"
        );
    }

    #[tokio::test]
    async fn draft_key_ollama_create_blank_is_explicit_empty() {
        let pool = fresh_pool().await;
        let repo = Repository::new(pool);
        // Ollama CREATE (id None), blank key, no clear → explicit-empty legal.
        let input = draft(
            "ollama",
            "ollama",
            "http://localhost:11434",
            &["api_chat"],
            &["llama3.1"],
            "",
        );
        let key = resolve_draft_api_key(&input, &repo).await.unwrap();
        assert_eq!(
            key, "",
            "Ollama create with a blank key stays the legal explicit-empty case"
        );
    }

    #[tokio::test]
    async fn draft_key_openai_edit_blank_not_cleared_uses_stored_key() {
        let pool = fresh_pool().await;
        let channel = Channel {
            id: "ch-openai-1".to_string(),
            name: "ch-openai-1".to_string(),
            channel_type: "openai".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "sk-existing".to_string(),
            models: "[]".to_string(),
            status: 1,
            priority: 0,
            weight: 1,
            config: "{}".to_string(),
            model_mapping: "{}".to_string(),
            timeout_secs: 30,
            protocol: Some("openai".to_string()),
            provider: Some("openai".to_string()),
            native_base_url: Some("https://api.openai.com/v1".to_string()),
            native_endpoints: Some("[\"chat_completions\"]".to_string()),
            preset_revision: Some("2026-08-04".to_string()),
            identity_revision: 1,
            legacy_executor_override: None,
            created_at: now_iso(),
            updated_at: now_iso(),
            last_test_at: None,
            last_test_ok: None,
        };
        insert_channel(&pool, &channel).await;
        let repo = Repository::new(pool);
        let mut input = draft(
            "openai",
            "openai",
            "https://api.openai.com/v1",
            &["chat_completions"],
            &["gpt-4o"],
            "",
        );
        input.id = Some("ch-openai-1".to_string());
        let key = resolve_draft_api_key(&input, &repo).await.unwrap();
        assert_eq!(
            key, "sk-existing",
            "non-Ollama edit + blank key + not cleared still reuses the stored key"
        );
    }
}
