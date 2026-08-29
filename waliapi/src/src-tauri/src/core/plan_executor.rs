//! Plan-executor facade (T05).
//!
//! Drives an [`AttemptFlow`] over a [`RoutePlan`], building each
//! [`PreparedAttempt`] (sampling the array model mapping exactly once) and
//! invoking a caller-supplied executor closure for the actual upstream send.
//!
//! Endpoint HTTP send details (reqwest, SSE transport, URL building) belong to
//! T06 — this module stays transport-agnostic and is fully testable with mock
//! executors.  Handlers behind the `new_routeplan` flag call
//! [`execute_plan`]; T06 replaces the stub executor with concrete ones.

use crate::core::attempt::{
    build_prepared_attempt, AttemptFailure, AttemptFlow, AttemptResult, AttemptSuccess,
    FailureClass, FlowStep, PreparedAttempt, TokenUsage,
};
use crate::core::route_plan::RoutePlan;
use crate::security::gate::AuditedRequest;
use rand::Rng;
use serde::Serialize;
use std::future::Future;
use std::time::Instant;

/// Outcome of running a plan to completion (success or exhausted budget).
///
/// T09 (design 11.4): carries the observability context of the winning attempt
/// (or the last attempted candidate) so the unified log writer can populate the
/// request-log fields: provider, identity_revision, codec_version.  These are
/// all taken from the SAME `PreparedAttempt`/`ChannelIdentity` that produced the
/// request body — body/logs/stats never diverge.
#[derive(Debug, Clone, Serialize)]
pub struct PlanExecution {
    pub status: u16,
    pub body: serde_json::Value,
    pub usage: Option<TokenUsage>,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub upstream_type: Option<String>,
    pub route_group: Option<String>,
    pub upstream_protocol: Option<String>,
    pub upstream_endpoint: Option<String>,
    pub upstream_model: Option<String>,
    /// T09: channel provider string of the attempt's channel (openai/deepseek/...).
    pub provider: Option<String>,
    /// T09: channel identity_revision at request time (0 = legacy-inferred).
    pub identity_revision: Option<i64>,
    /// T09: versioned codec label when a conversion ran (e.g. chat_to_messages_v1).
    pub codec_version: Option<String>,
    /// T06 M-2: safely-passthrough upstream response headers forwarded to the
    /// handler (credentials / hop-by-hop dropped by the executor).
    pub response_headers: Vec<(String, String)>,
    /// Total attempts actually executed (1-based for the first).
    pub attempts: usize,
    pub duration_ms: u64,
    pub last_failure: Option<AttemptFailure>,
}

/// Lightweight copy of the group/candidate routing metadata captured before the
/// attempt is handed to the executor (so the borrow on `flow` can be released).
#[derive(Clone)]
struct AttemptMeta {
    channel_id: String,
    channel_name: String,
    upstream_type: String,
    route_group: String,
    upstream_protocol: String,
    upstream_endpoint: String,
    /// Candidate metadata is generic: Auth accounts do not have a ChannelIdentity.
    provider: String,
    identity_revision: i64,
}

/// Run a non-stream plan to completion.
///
/// `executor` is called once per attempt with the fully-prepared attempt; it
/// returns the upstream result.  On success the plan returns immediately; on
/// failure the flow applies the retry/budget/group-transition rules.
pub async fn execute_plan<R, F, Fut>(
    plan: RoutePlan,
    audit: &AuditedRequest,
    mut rng: R,
    mut executor: F,
) -> PlanExecution
where
    R: Rng,
    F: FnMut(&PreparedAttempt) -> Fut,
    Fut: Future<Output = AttemptResult>,
{
    let started = Instant::now();
    let mut flow = AttemptFlow::new(plan);
    // `FlowStep::Halt` only carries the terminal status/message. Retain the
    // last selected candidate so an exhausted plan preserves log metadata.
    let mut last_attempt_meta: Option<AttemptMeta> = None;
    let mut last_attempt_codec_version: Option<String> = None;
    loop {
        match flow.next_step() {
            FlowStep::Execute {
                group_idx,
                candidate_idx,
                attempt_no,
            } => {
                let (built, meta) = {
                    let plan = flow.plan();
                    let group = &plan.groups[group_idx];
                    let candidate = &group.candidates[candidate_idx];
                    let meta = AttemptMeta {
                        channel_id: candidate.candidate.id().to_string(),
                        channel_name: candidate.candidate.name().to_string(),
                        upstream_type: candidate.candidate.upstream_type().to_string(),
                        route_group: group.id.clone(),
                        // Candidate-level protocol/endpoint (a native group may
                        // hold mixed upstream protocols when ollama_native is on;
                        // group-level mirrors only the first candidate).
                        upstream_protocol: candidate.upstream_protocol.as_str().to_string(),
                        upstream_endpoint: candidate.upstream_endpoint.clone(),
                        provider: candidate.candidate.provider(),
                        identity_revision: candidate.candidate.identity_revision(),
                    };
                    (
                        build_prepared_attempt(audit, group, candidate, &mut rng, attempt_no),
                        meta,
                    )
                };
                // T09 (design 11.4): codec_version comes from the SAME
                // PreparedAttempt that produced the request body.
                let codec_version = match &built {
                    Ok(attempt) => attempt.codec_version.clone(),
                    Err(_) => None,
                };
                last_attempt_meta = Some(meta.clone());
                last_attempt_codec_version = codec_version.clone();
                let result = match built {
                    // A construction failure (codec rejection) is already a
                    // full AttemptFailure carrying the rejected feature reason.
                    Err(failure) => failure,
                    Ok(attempt) => {
                        let upstream_model = attempt.upstream_model.clone();
                        match executor(&attempt).await {
                            AttemptResult::Success(mut s) => {
                                if s.upstream_model.is_none() {
                                    s.upstream_model = Some(upstream_model);
                                }
                                return PlanExecution {
                                    status: s.status,
                                    body: s.body,
                                    usage: s.usage,
                                    channel_id: Some(meta.channel_id),
                                    channel_name: Some(meta.channel_name),
                                    upstream_type: Some(meta.upstream_type),
                                    route_group: Some(meta.route_group),
                                    upstream_protocol: Some(meta.upstream_protocol),
                                    upstream_endpoint: Some(meta.upstream_endpoint),
                                    upstream_model: s.upstream_model,
                                    provider: Some(meta.provider),
                                    identity_revision: Some(meta.identity_revision),
                                    codec_version,
                                    response_headers: s.response_headers,
                                    attempts: attempt_no,
                                    duration_ms: started.elapsed().as_millis() as u64,
                                    last_failure: None,
                                };
                            }
                            AttemptResult::Failure(f) => f,
                        }
                    }
                };
                flow.record_failure(&result);
                if result.failure_class == FailureClass::CallerTerminal
                    || result.failure_class == FailureClass::CommittedStreamError
                {
                    // Honor an explicit status_code (e.g. the T06 stub's 501);
                    // fall back to the class's canonical terminal status (400
                    // for a real caller-terminal codec rejection).
                    let status = result.status_code.unwrap_or(terminal_status_of(&result));
                    return PlanExecution {
                        status,
                        body: serde_json::json!({ "error": { "message": result.message } }),
                        usage: None,
                        channel_id: Some(meta.channel_id),
                        channel_name: Some(meta.channel_name),
                        upstream_type: Some(meta.upstream_type),
                        route_group: Some(meta.route_group),
                        upstream_protocol: Some(meta.upstream_protocol),
                        upstream_endpoint: Some(meta.upstream_endpoint),
                        upstream_model: None,
                        provider: Some(meta.provider),
                        identity_revision: Some(meta.identity_revision),
                        codec_version,
                        response_headers: vec![],
                        attempts: attempt_no,
                        duration_ms: started.elapsed().as_millis() as u64,
                        last_failure: Some(result),
                    };
                }
                // Otherwise loop; `flow.next_step()` applies group transition / budget.
            }
            FlowStep::Halt { status, message } => {
                let meta = last_attempt_meta;
                return PlanExecution {
                    status,
                    body: serde_json::json!({ "error": { "message": message } }),
                    usage: None,
                    channel_id: meta.as_ref().map(|meta| meta.channel_id.clone()),
                    channel_name: meta.as_ref().map(|meta| meta.channel_name.clone()),
                    upstream_type: meta.as_ref().map(|meta| meta.upstream_type.clone()),
                    route_group: meta.as_ref().map(|meta| meta.route_group.clone()),
                    upstream_protocol: meta.as_ref().map(|meta| meta.upstream_protocol.clone()),
                    upstream_endpoint: meta.as_ref().map(|meta| meta.upstream_endpoint.clone()),
                    upstream_model: None,
                    provider: meta.as_ref().map(|meta| meta.provider.clone()),
                    identity_revision: meta.as_ref().map(|meta| meta.identity_revision),
                    codec_version: last_attempt_codec_version,
                    response_headers: vec![],
                    attempts: flow.attempts_used(),
                    duration_ms: started.elapsed().as_millis() as u64,
                    last_failure: flow.last_failure().cloned(),
                };
            }
        }
    }
}

fn terminal_status_of(failure: &AttemptFailure) -> u16 {
    crate::core::attempt::terminal_status(failure.failure_class)
}

/// Stub executor used by handlers behind the `new_routeplan` flag until T06
/// wires the real HTTP executors.  It always fails closed (never contacts an
/// upstream).
///
/// Deliberately a sync fn returning `std::future::ready` so the future does NOT
/// borrow the attempt (keeps the executor closure `'static`).
pub fn not_wired_executor(
    _attempt: &PreparedAttempt,
) -> impl std::future::Future<Output = AttemptResult> {
    std::future::ready(AttemptResult::Failure(AttemptFailure {
        failure_class: FailureClass::CallerTerminal,
        message: "route plan executor not wired yet (T06)".to_string(),
        status_code: Some(501),
        retry_after: None,
    }))
}

/// Convenience: build a successful non-stream result from a body + usage.
pub fn ok_result(status: u16, body: serde_json::Value, usage: Option<TokenUsage>) -> AttemptResult {
    AttemptResult::Success(AttemptSuccess {
        status,
        body,
        usage,
        downstream_events: None,
        upstream_model: None,
        response_headers: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::feature_flags::FeatureFlags;
    use crate::core::route_plan::{authorize_and_plan, EndpointKind};
    use crate::db::models::{ApiKey, Channel};
    use crate::security::gate::{DownstreamProtocol, RequestEnvelope, RequestFeatures};
    use crate::security::SecurityScanResult;
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use serde_json::json;

    fn api_key() -> ApiKey {
        ApiKey {
            id: "key-1".into(),
            name: "t".into(),
            key: "sk".into(),
            status: 1,
            allowed_models: "[]".into(),
            allowed_channels: "[]".into(),
            denied_models: "[]".into(),
            denied_channels: "[]".into(),
            quota_limit: 0,
            quota_used: 0,
            expires_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn channel(
        id: &str,
        channel_type: &str,
        base_url: &str,
        priority: i64,
        weight: i64,
    ) -> Channel {
        Channel {
            id: id.into(),
            name: format!("ch-{}", id),
            channel_type: channel_type.into(),
            base_url: base_url.into(),
            api_key: "sk-test".into(),
            models: json!(["m"]).to_string(),
            status: 1,
            priority,
            weight,
            config: "{}".into(),
            model_mapping: "{}".into(),
            timeout_secs: 30,
            protocol: None,
            provider: None,
            native_base_url: None,
            native_endpoints: None,
            preset_revision: None,
            identity_revision: 0,
            legacy_executor_override: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            last_test_at: None,
            last_test_ok: None,
        }
    }

    fn audited() -> AuditedRequest {
        AuditedRequest {
            envelope: RequestEnvelope {
                downstream_protocol: DownstreamProtocol::ChatCompletions,
                endpoint: "chat_completions".into(),
                original_json: json!({ "model": "m", "messages": [] }),
                safe_forward_headers: vec![],
                query: None,
                model: "m".into(),
                stream: false,
                trace_id: None,
            },
            forward_json: json!({ "model": "m", "messages": [] }),
            sanitized_log_json: json!({ "model": "m", "messages": [] }),
            body_hash: "h".into(),
            body_len: 0,
            audit_result: SecurityScanResult::default(),
            request_features: RequestFeatures::default(),
        }
    }

    fn plan_for(channels: Vec<Channel>) -> RoutePlan {
        let key = api_key();
        let mut rng = StdRng::seed_from_u64(7);
        authorize_and_plan(
            &key,
            "m",
            EndpointKind::ChatCompletions,
            &channels,
            &FeatureFlags::all_on(),
            &json!({}),
            &mut rng,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn retries_retryable_then_succeeds() {
        let c1 = channel("n1", "openai", "https://api.openai.com/v1", 10, 1);
        let c2 = channel("n2", "openai", "https://api.openai.com/v1", 5, 1);
        let plan = plan_for(vec![c1, c2]);
        let mut calls = 0;
        let out = execute_plan(plan, &audited(), StdRng::seed_from_u64(7), |attempt| {
            calls += 1;
            let is_first = attempt.channel_id == "n1";
            async move {
                if is_first {
                    AttemptResult::Failure(AttemptFailure {
                        failure_class: FailureClass::Retryable,
                        message: "502 from n1".into(),
                        status_code: Some(502),
                        retry_after: None,
                    })
                } else {
                    ok_result(200, json!({ "choices": [] }), None)
                }
            }
        })
        .await;
        assert_eq!(out.status, 200);
        assert_eq!(out.channel_id.as_deref(), Some("n2"));
        assert_eq!(out.attempts, 2);
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn caller_terminal_does_not_try_second_channel() {
        let c1 = channel("n1", "openai", "https://api.openai.com/v1", 10, 1);
        let c2 = channel("n2", "openai", "https://api.openai.com/v1", 5, 1);
        let plan = plan_for(vec![c1, c2]);
        let mut calls = 0;
        let out = execute_plan(plan, &audited(), StdRng::seed_from_u64(7), |_attempt| {
            calls += 1;
            async move {
                AttemptResult::Failure(AttemptFailure {
                    failure_class: FailureClass::CallerTerminal,
                    message: "400 from n1".into(),
                    status_code: Some(400),
                    retry_after: None,
                })
            }
        })
        .await;
        assert_eq!(out.status, 400);
        assert_eq!(calls, 1, "caller_terminal must not attempt another channel");
    }

    #[tokio::test]
    async fn exhausted_budget_halts_with_502() {
        let c1 = channel("n1", "openai", "https://api.openai.com/v1", 1, 1);
        let c2 = channel("n2", "openai", "https://api.openai.com/v1", 1, 1);
        let plan = plan_for(vec![c1, c2]);
        let out = execute_plan(
            plan,
            &audited(),
            StdRng::seed_from_u64(7),
            |_attempt| async move {
                AttemptResult::Failure(AttemptFailure {
                    failure_class: FailureClass::Retryable,
                    message: "always failing".into(),
                    status_code: Some(503),
                    retry_after: None,
                })
            },
        )
        .await;
        assert_eq!(out.status, 502);
        assert_eq!(
            out.last_failure.as_ref().unwrap().failure_class,
            FailureClass::Retryable
        );
    }

    #[tokio::test]
    async fn not_wired_stub_yields_501_not_400() {
        // F1: the T06 stub reports status_code 501 with CallerTerminal; the
        // short-circuit must honor the explicit code instead of forcing the
        // canonical 400 for CallerTerminal.
        let c1 = channel("n1", "openai", "https://api.openai.com/v1", 1, 1);
        let plan = plan_for(vec![c1]);
        let out = execute_plan(plan, &audited(), StdRng::seed_from_u64(7), |attempt| {
            not_wired_executor(attempt)
        })
        .await;
        assert_eq!(out.status, 501, "stub must surface 501, not 400");
        assert_eq!(out.attempts, 1);
        assert_eq!(
            out.last_failure.as_ref().unwrap().failure_class,
            FailureClass::CallerTerminal
        );
    }

    #[tokio::test]
    async fn success_plan_execution_carries_observability_context() {
        // T09 (design 11.4): PlanExecution must carry the SAME observability
        // context that produced the request body — provider / identity_revision
        // from the candidate identity, codec_version / upstream_model from the
        // PreparedAttempt.  A conversion attempt proves the codec label is
        // recorded for the log.
        let mut ant = channel("c1", "claude", "https://api.anthropic.com/v1", 10, 1);
        ant.model_mapping = json!({ "m": ["claude-sonnet-4-6", "claude-opus-4-6"] }).to_string();
        ant.identity_revision = 3;
        // Use a Chat request so the Anthropic channel lands in the conversion
        // group (codec chat_to_messages_v1).
        let key = api_key();
        let mut rng = StdRng::seed_from_u64(9);
        let plan = authorize_and_plan(
            &key,
            "m",
            EndpointKind::ChatCompletions,
            &[ant],
            &FeatureFlags::all_on(),
            &json!({}),
            &mut rng,
        )
        .unwrap();
        assert_eq!(
            plan.groups[0].tier,
            crate::core::route_plan::GroupTier::Conversion
        );
        let out = execute_plan(plan, &audited(), StdRng::seed_from_u64(9), |attempt| {
            let body = json!({ "content": [{"type": "text", "text": "hi"}] });
            let usage = TokenUsage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
            };
            let um = attempt.upstream_model.clone();
            async move {
                AttemptResult::Success(AttemptSuccess {
                    status: 200,
                    body,
                    usage: Some(usage),
                    downstream_events: None,
                    upstream_model: Some(um),
                    response_headers: vec![],
                })
            }
        })
        .await;
        assert_eq!(out.status, 200);
        // The sampled model was baked into the request body AND is the log's
        // upstream_model (single source, design 11.4).
        let mapped = ["claude-sonnet-4-6", "claude-opus-4-6"];
        assert!(
            mapped.contains(&out.upstream_model.as_deref().unwrap_or("")),
            "upstream_model must be one of the sampled array"
        );
        assert_eq!(out.codec_version.as_deref(), Some("chat_to_messages_v1"));
        // Provider + identity_revision come from the candidate identity.  The
        // legacy claude row resolves to provider=anthropic (canonical host).
        assert_eq!(out.provider.as_deref(), Some("anthropic"));
        assert_eq!(out.identity_revision, Some(3));
    }
}
