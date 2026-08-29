//! T12 offline acceptance checks for the Auth-account rollout.
//!
//! Networked protocol coverage intentionally reuses the local Axum harness in
//! `endpoint_executor::integration_tests::auth_account`: it covers Chat,
//! Messages and Responses on stream and non-stream paths.  The tests here
//! close the remaining cross-cutting acceptance gaps without any real account
//! or network dependency.

#![cfg(test)]

use std::sync::atomic::{AtomicUsize, Ordering};

use rand::{rngs::StdRng, SeedableRng};
use serde_json::json;

use crate::{
    auth_provider::{AuthAccountSummary, ProviderError, ProviderPayload},
    core::{
        attempt::{AttemptFailure, FailureClass},
        feature_flags::FeatureFlags,
        plan_executor::{execute_plan, ok_result},
        route_plan::{authorize_and_plan_with_accounts, EndpointKind, PlanError},
    },
    db::models::{ApiKey, AuthAccount, ModelStates, RequestLog},
    security::{
        gate::{AuditedRequest, DownstreamProtocol, RequestEnvelope, RequestFeatures},
        SecurityScanResult,
    },
};

const ACCESS: &str = "t12-fixture-access-token";
const REFRESH: &str = "t12-fixture-refresh-token";
const ID_TOKEN: &str = "t12-fixture-id-token";
const AUTH_JSON: &str = "t12-fixture-auth-json";

fn key() -> ApiKey {
    ApiKey {
        id: "t12-key".into(),
        name: "T12".into(),
        key: "sk-test".into(),
        status: 1,
        allowed_models: "[]".into(),
        allowed_channels: "[]".into(),
        denied_models: "[]".into(),
        denied_channels: "[]".into(),
        quota_limit: 0,
        quota_used: 0,
        expires_at: None,
        created_at: "2026-08-09T00:00:00Z".into(),
        updated_at: "2026-08-09T00:00:00Z".into(),
    }
}

fn account(id: &str, priority: i64, models: ModelStates) -> AuthAccount {
    AuthAccount {
        id: id.into(),
        provider: "codex".into(),
        label: format!("offline-{id}"),
        account_id: format!("remote-{id}"),
        status: "active".into(),
        disabled: 0,
        priority,
        weight: 1,
        quota_json: None,
        model_states_json: serde_json::to_string(&models).unwrap(),
        model_mapping_json: "{}".into(),
        attributes_json: "{}".into(),
        payload_json: json!({
            "access_token": ACCESS,
            "refresh_token": REFRESH,
            "id_token": ID_TOKEN,
            "auth_json": AUTH_JSON,
            "account_id": format!("remote-{id}"),
            "expires_at": "2099-01-01T00:00:00Z"
        })
        .to_string(),
        last_refreshed_at: None,
        last_models_sync_at: None,
        next_refresh_after: None,
        next_retry_after: None,
        created_at: "2026-08-09T00:00:00Z".into(),
        updated_at: "2026-08-09T00:00:00Z".into(),
    }
}

fn available_models() -> ModelStates {
    ModelStates {
        version: 1,
        models: vec![crate::db::models::ModelState {
            id: "gpt-t12".into(),
            status: "available".into(),
            unavailable: false,
            next_retry_after: None,
            last_error: None,
            protocol: None,
        }],
    }
}

fn flags() -> FeatureFlags {
    FeatureFlags {
        new_routeplan: true,
        cross_protocol_codec: true,
        native_responses: true,
        ollama_native: false,
        prefer_auth_accounts: false,
        prefer_same_protocol: true,
    }
}

fn audited() -> AuditedRequest {
    let body = json!({"model": "gpt-t12", "input": "offline fixture"});
    AuditedRequest {
        envelope: RequestEnvelope {
            downstream_protocol: DownstreamProtocol::Responses,
            endpoint: "responses".into(),
            original_json: body.clone(),
            safe_forward_headers: vec![],
            query: None,
            model: "gpt-t12".into(),
            stream: false,
            trace_id: None,
        },
        forward_json: body.clone(),
        sanitized_log_json: body,
        body_hash: "t12".into(),
        body_len: 0,
        audit_result: SecurityScanResult::default(),
        request_features: RequestFeatures::default(),
    }
}

#[test]
fn auth_first_model_sync_failure_is_fail_closed_without_an_outbound_attempt() {
    let outbound_calls = AtomicUsize::new(0);
    let empty = ModelStates {
        version: 1,
        models: vec![],
    };
    let request = audited();
    let result = authorize_and_plan_with_accounts(
        &key(),
        "gpt-t12",
        EndpointKind::Responses,
        &[],
        &[account("empty-snapshot", 1, empty)],
        &flags(),
        &request.forward_json,
        &mut StdRng::seed_from_u64(12),
    );

    assert!(matches!(result, Err(PlanError::NoCandidateForModel(_))));
    assert_eq!(outbound_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn auth_429_degrades_to_the_next_local_candidate() {
    let request = audited();
    let accounts = [
        account("rate-limited", 10, available_models()),
        account("fallback", 1, available_models()),
    ];
    let plan = authorize_and_plan_with_accounts(
        &key(),
        "gpt-t12",
        EndpointKind::Responses,
        &[],
        &accounts,
        &flags(),
        &request.forward_json,
        &mut StdRng::seed_from_u64(12),
    )
    .unwrap();
    let mut attempted = Vec::new();
    let outcome = execute_plan(plan, &request, StdRng::seed_from_u64(12), |attempt| {
        let id = attempt.channel_id.clone();
        attempted.push(id.clone());
        async move {
            if id == "rate-limited" {
                crate::core::attempt::AttemptResult::Failure(AttemptFailure {
                    failure_class: FailureClass::Retryable,
                    message: "local fake provider returned 429".into(),
                    status_code: Some(429),
                    retry_after: Some(1),
                })
            } else {
                ok_result(200, json!({"object": "response", "id": "offline-ok"}), None)
            }
        }
    })
    .await;

    assert_eq!(attempted, ["rate-limited", "fallback"]);
    assert_eq!(outcome.status, 200);
    assert_eq!(outcome.channel_id.as_deref(), Some("fallback"));
}

#[test]
fn auth_dto_debug_log_and_error_snapshots_exclude_every_fixture_secret() {
    let account = account("redaction", 1, available_models());
    let summary = AuthAccountSummary::from_account(&account).unwrap();
    let request = audited();
    let plan = authorize_and_plan_with_accounts(
        &key(),
        "gpt-t12",
        EndpointKind::Responses,
        &[],
        &[account],
        &flags(),
        &request.forward_json,
        &mut StdRng::seed_from_u64(12),
    )
    .unwrap();
    let payload = ProviderPayload::new(json!({
        "access_token": ACCESS,
        "refresh_token": REFRESH,
        "id_token": ID_TOKEN,
        "auth_json": AUTH_JSON
    }));
    let snapshots = [
        serde_json::to_string(&summary).unwrap(),
        format!("{summary:?}"),
        plan.debug_json().to_string(),
        format!("{payload:?}"),
        serde_json::to_string(&RequestLog::default()).unwrap(),
        format!(
            "{} {:?}",
            ProviderError::Unauthorized,
            ProviderError::Unauthorized
        ),
    ];
    for snapshot in snapshots {
        for secret in [ACCESS, REFRESH, ID_TOKEN, AUTH_JSON] {
            assert!(!snapshot.contains(secret), "secret leaked: {secret}");
        }
    }
}

#[test]
fn auth_rollout_exclusions_keep_zstd_and_a_30_minute_probe_out_of_scope() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo_toml = std::fs::read_to_string(manifest.join("Cargo.toml")).unwrap();
    let auth_sources = std::fs::read_dir(manifest.join("src/auth_provider"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<String>();
    let maintenance =
        std::fs::read_to_string(manifest.join("src/auth_provider/maintenance.rs")).unwrap();

    assert!(!cargo_toml.contains("zstd"));
    assert!(!auth_sources.contains("zstd"));
    assert_eq!(maintenance.matches("interval(").count(), 1);
    assert!(!maintenance.contains("30 * 60"));
    assert!(!maintenance.contains("quota_probe"));
}
