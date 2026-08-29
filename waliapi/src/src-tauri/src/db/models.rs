use chrono::Utc;
use serde::{Deserialize, Serialize};

/// A single API key belonging to a channel (migration 023: channel_api_keys).
/// Multiple keys per channel enable load balancing and failover.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChannelApiKey {
    pub id: String,
    pub channel_id: String,
    pub api_key: String,
    pub weight: i64,
    pub status: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Input for creating/updating a channel API key entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelApiKeyInput {
    pub api_key: String,
    #[serde(default)]
    pub weight: Option<i64>,
    #[serde(default)]
    pub status: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub channel_type: String,
    pub base_url: String,
    pub api_key: String,
    pub models: String,
    pub status: i64,
    pub priority: i64,
    pub weight: i64,
    pub config: String,
    pub model_mapping: String,
    pub timeout_secs: i64,
    // --- T02 protocol identity columns (migration 015) ---
    pub protocol: Option<String>,
    pub provider: Option<String>,
    pub native_base_url: Option<String>,
    pub native_endpoints: Option<String>,
    pub preset_revision: Option<String>,
    pub identity_revision: i64,
    pub legacy_executor_override: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_test_at: Option<String>,
    pub last_test_ok: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateChannelInput {
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub priority: Option<i64>,
    pub weight: Option<i64>,
    pub config: Option<serde_json::Value>,
    pub model_mapping: Option<serde_json::Value>,
    pub timeout_secs: Option<i64>,
    // --- T02 protocol identity fields (all Option + serde(default)) ---
    // Missing => legacy inference from type/base_url/config.
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub native_base_url: Option<String>,
    /// Serialized JSON array of endpoint strings; missing => legacy inference.
    #[serde(default)]
    pub native_endpoints: Option<Vec<String>>,
    #[serde(default)]
    pub preset_revision: Option<String>,
    #[serde(default)]
    pub legacy_executor_override: Option<String>,
    // --- T07 draft-test receipt. Backend validates these against the current
    // draft when present; force_save saves despite failed/skipped tests as long
    // as the same draft was tested at least once. Legacy payloads omit them. ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_save: Option<bool>,
    // --- Multi-key: additional API keys for load balancing (migration 023) ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_keys: Option<Vec<ChannelApiKeyInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateChannelInput {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub channel_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub models: Option<Vec<String>>,
    pub status: Option<i64>,
    pub priority: Option<i64>,
    pub weight: Option<i64>,
    pub config: Option<serde_json::Value>,
    pub model_mapping: Option<serde_json::Value>,
    pub timeout_secs: Option<i64>,
    // --- T02 protocol identity fields. None = keep current value. ---
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub native_base_url: Option<String>,
    /// None = keep; explicit empty Vec is REJECTED (must be non-empty or absent).
    #[serde(default)]
    pub native_endpoints: Option<Vec<String>>,
    #[serde(default)]
    pub preset_revision: Option<String>,
    #[serde(default)]
    pub legacy_executor_override: Option<String>,
    /// Distinguish "edit leave-blank = keep key" from "Ollama explicitly clear
    /// key": true => persist an empty api_key (clears the stored key).
    #[serde(default)]
    pub clear_api_key: Option<bool>,
    // --- T07 draft-test receipt (see CreateChannelInput). ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_save: Option<bool>,
    // --- Multi-key: replacement for extra keys (full replace semantics) ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_keys: Option<Vec<ChannelApiKeyInput>>,
}

/// Import-write input (T09).  Unlike `CreateChannelInput` (whose repository
/// writer hard-codes status=1 and a default timeout), this input carries the
/// full business field set so import/export round-trips are per-field exact:
/// status, priority, weight, timeout_secs, config unknown keys, URL, key,
/// models and array model_mapping.  Identity columns are `Option`: a v1 /
/// legacy import passes `None` (identity_revision 0) so the resolver live-infers;
/// a v2 import passes the validated identity verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImportChannelInput {
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub status: i64,
    pub priority: i64,
    pub weight: i64,
    pub config: serde_json::Value,
    pub model_mapping: serde_json::Value,
    pub timeout_secs: i64,
    // --- T02 protocol identity columns (None => legacy-infer on read) ---
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub native_base_url: Option<String>,
    #[serde(default)]
    pub native_endpoints: Option<Vec<String>>,
    #[serde(default)]
    pub preset_revision: Option<String>,
    #[serde(default)]
    pub identity_revision: i64,
    #[serde(default)]
    pub legacy_executor_override: Option<String>,
    // --- test-status fields (preserved so an exported test badge survives) ---
    #[serde(default)]
    pub last_test_at: Option<String>,
    #[serde(default)]
    pub last_test_ok: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key: String,
    pub status: i64,
    pub allowed_models: String,
    pub allowed_channels: String,
    pub denied_models: String,
    pub denied_channels: String,
    pub quota_limit: i64,
    pub quota_used: i64,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyInput {
    pub name: String,
    pub allowed_models: Option<Vec<String>>,
    pub allowed_channels: Option<Vec<String>>,
    pub denied_models: Option<Vec<String>>,
    pub denied_channels: Option<Vec<String>>,
    pub quota_limit: Option<i64>,
    pub expires_at: Option<String>,
}

/// A single model in an Auth Account's provider-synchronized snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelState {
    pub id: String,
    pub status: String,
    pub unavailable: bool,
    pub next_retry_after: Option<String>,
    pub last_error: Option<String>,
    /// Per-model wire protocol metadata sourced only from the provider's
    /// `/models` catalog (e.g. `kimi` or `anthropic`).  Backward-compatible:
    /// old snapshots serialize without this field and deserialize to `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelStates {
    pub version: i64,
    pub models: Vec<ModelState>,
}

impl Default for ModelStates {
    fn default() -> Self {
        Self {
            version: 1,
            models: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaWindow {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<i64>,
    pub reset_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaLimit {
    pub limit_id: String,
    pub limit_name: Option<String>,
    pub primary: Option<QuotaWindow>,
    pub secondary: Option<QuotaWindow>,
    pub credits: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaState {
    pub version: i64,
    pub exceeded: bool,
    pub reason: Option<String>,
    pub next_recover_at: Option<String>,
    pub backoff_level: i64,
    pub limits: Vec<QuotaLimit>,
}

impl Default for QuotaState {
    fn default() -> Self {
        Self {
            version: 1,
            exceeded: false,
            reason: None,
            next_recover_at: None,
            backoff_level: 0,
            limits: Vec::new(),
        }
    }
}

/// Persisted generic provider account. `payload_json` is intentionally kept in
/// the database model only; command DTOs must expose a redacted summary.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuthAccount {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub account_id: String,
    pub status: String,
    pub disabled: i64,
    pub priority: i64,
    pub weight: i64,
    pub quota_json: Option<String>,
    pub model_states_json: String,
    pub model_mapping_json: String,
    pub attributes_json: String,
    pub payload_json: String,
    pub last_refreshed_at: Option<String>,
    pub last_models_sync_at: Option<String>,
    pub next_refresh_after: Option<String>,
    pub next_retry_after: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl AuthAccount {
    pub fn model_states(&self) -> Result<ModelStates, serde_json::Error> {
        serde_json::from_str(&self.model_states_json)
    }

    pub fn quota_state(&self) -> Result<Option<QuotaState>, serde_json::Error> {
        self.quota_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
    }

    pub fn model_mapping(&self) -> Result<serde_json::Value, serde_json::Error> {
        if self.model_mapping_json.is_empty() {
            return Ok(serde_json::json!({}));
        }
        serde_json::from_str(&self.model_mapping_json)
    }
}

/// Login/import input used for an atomic provider/account-id upsert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthAccountUpsert {
    pub provider: String,
    pub label: String,
    pub account_id: String,
    pub attributes: serde_json::Value,
    pub payload: serde_json::Value,
    pub last_refreshed_at: Option<String>,
    pub next_refresh_after: Option<String>,
    pub next_retry_after: Option<String>,
}

/// A persisted request-log row.  All T09 observability columns are NULLABLE
/// (migration 016) so legacy rows and old queries keep working. Its manual
/// `Default` supplies `upstream_type = "channel"` for legacy write paths.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RequestLog {
    pub id: String,
    pub seq: Option<i64>,
    pub api_key_id: Option<String>,
    pub api_key_name: Option<String>,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub model: String,
    pub upstream_model: Option<String>,
    pub mode: String,
    pub status_code: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub duration_ms: i64,
    pub error_message: Option<String>,
    pub is_stream: i64,
    pub is_retry: i64,
    pub created_at: String,
    pub request_body: Option<String>,
    pub response_choices: Option<String>,
    pub risk_level: String,
    pub risk_score: i64,
    pub risk_summary: Option<String>,
    pub security_action: String,
    pub sanitized: i64,
    pub blocked_reason: Option<String>,
    pub trace_id: Option<String>,
    // --- T09 observability (migration 016; all nullable) ---
    pub downstream_protocol: Option<String>,
    pub downstream_endpoint: Option<String>,
    pub route_group: Option<String>,
    pub upstream_protocol: Option<String>,
    pub upstream_endpoint: Option<String>,
    pub provider: Option<String>,
    pub codec_version: Option<String>,
    pub failure_class: Option<String>,
    pub identity_revision: Option<i64>,
    pub client_cancelled: Option<i64>,
    pub stream_committed: Option<i64>,
    /// `channel` for legacy API channels and `auth_account` for provider
    /// accounts. The database default makes upgraded historical rows channel.
    pub upstream_type: String,
}

impl Default for RequestLog {
    fn default() -> Self {
        Self {
            id: String::new(),
            seq: None,
            api_key_id: None,
            api_key_name: None,
            channel_id: None,
            channel_name: None,
            model: String::new(),
            upstream_model: None,
            mode: String::new(),
            status_code: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            duration_ms: 0,
            error_message: None,
            is_stream: 0,
            is_retry: 0,
            created_at: String::new(),
            request_body: None,
            response_choices: None,
            risk_level: String::new(),
            risk_score: 0,
            risk_summary: None,
            security_action: String::new(),
            sanitized: 0,
            blocked_reason: None,
            trace_id: None,
            downstream_protocol: None,
            downstream_endpoint: None,
            route_group: None,
            upstream_protocol: None,
            upstream_endpoint: None,
            provider: None,
            codec_version: None,
            failure_class: None,
            identity_revision: None,
            client_cancelled: None,
            stream_committed: None,
            upstream_type: "channel".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardStats {
    pub today_requests: i64,
    pub today_total_tokens: i64,
    pub active_channels: i64,
    pub avg_latency_ms: f64,
    pub total_channels: i64,
    pub total_api_keys: i64,
    pub total_requests: i64,
    pub total_tokens: i64,
    pub total_knowledge_bases: i64,
    pub total_kb_documents: i64,
    pub total_kb_chunks: i64,
    pub total_wiki_projects: i64,
    pub total_wiki_pages: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LogStats {
    pub date: String,
    pub count: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChannelStats {
    pub channel_id: String,
    pub total_calls: i64,
    pub success_calls: i64,
    pub failed_calls: i64,
    pub total_tokens: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub avg_latency_ms: f64,
    pub last_call_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiKeyStats {
    pub api_key_id: String,
    pub total_calls: i64,
    pub success_calls: i64,
    pub failed_calls: i64,
    pub total_tokens: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub avg_latency_ms: f64,
    pub last_call_at: Option<String>,
}

pub fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RequestSecurityFinding {
    pub id: String,
    pub log_id: String,
    pub phase: String,
    pub category: String,
    pub rule_id: String,
    pub severity: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub evidence_masked: Option<String>,
    pub evidence_hash: Option<String>,
    pub action: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_codex_snapshot_without_protocol_deserializes() {
        let json = r#"{"id":"gpt-5","status":"available","unavailable":false,"next_retry_after":null,"last_error":null}"#;
        let model: ModelState = serde_json::from_str(json).expect("old snapshot must parse");
        assert_eq!(model.id, "gpt-5");
        assert_eq!(model.protocol, None);
    }

    #[test]
    fn model_protocol_serialization_round_trip() {
        let model = ModelState {
            id: "kimi-k2.5".into(),
            status: "available".into(),
            unavailable: false,
            next_retry_after: None,
            last_error: None,
            protocol: Some("kimi".into()),
        };
        let json = serde_json::to_string(&model).unwrap();
        let parsed: ModelState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.protocol.as_deref(), Some("kimi"));
    }

    #[test]
    fn model_protocol_omitted_when_none() {
        let bare = ModelState {
            id: "gpt-5".into(),
            status: "available".into(),
            unavailable: false,
            next_retry_after: None,
            last_error: None,
            protocol: None,
        };
        let json = serde_json::to_string(&bare).unwrap();
        assert!(
            !json.contains("protocol"),
            "None protocol must not serialize"
        );
    }

    #[test]
    fn model_states_round_trip_with_protocol() {
        let states = ModelStates {
            version: 1,
            models: vec![ModelState {
                id: "kimi-a".into(),
                status: "available".into(),
                unavailable: false,
                next_retry_after: None,
                last_error: None,
                protocol: Some("anthropic".into()),
            }],
        };
        let json = serde_json::to_string(&states).unwrap();
        let parsed: ModelStates = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.models[0].protocol.as_deref(), Some("anthropic"));
    }
}
