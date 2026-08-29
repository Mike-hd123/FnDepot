use crate::protocol::codec::error::FeatureKind;
use crate::protocol::codec::registry::CodecRegistry;
use crate::protocol::codec::types::Protocol;
use serde_json::json;

// ===========================================================================
// FeatureKind stable codes
// ===========================================================================

#[test]
fn feature_kind_stable_codes() {
    assert_eq!(FeatureKind::Thinking.code(), "unsupported_feature.thinking");
    assert_eq!(
        FeatureKind::StructuredOutput.code(),
        "unsupported_feature.structured_output"
    );
    assert_eq!(
        FeatureKind::BuiltinTool.code(),
        "unsupported_feature.builtin_tool"
    );
    assert_eq!(FeatureKind::Document.code(), "unsupported_feature.document");
    assert_eq!(
        FeatureKind::PromptCache.code(),
        "unsupported_feature.prompt_cache"
    );
    assert_eq!(
        FeatureKind::UnknownRole.code(),
        "unsupported_feature.unknown_role"
    );
    assert_eq!(
        FeatureKind::UnknownBlock.code(),
        "unsupported_feature.unknown_block"
    );
    assert_eq!(
        FeatureKind::UnknownEvent.code(),
        "unsupported_feature.unknown_event"
    );
    assert_eq!(
        FeatureKind::UnknownFinishReason.code(),
        "unsupported_feature.finish_reason"
    );
    assert_eq!(
        FeatureKind::InvalidToolArguments.code(),
        "unsupported_feature.invalid_tool_arguments"
    );
    assert_eq!(
        FeatureKind::MissingToolField.code(),
        "unsupported_feature.missing_tool_field"
    );
    assert_eq!(FeatureKind::Media.code(), "unsupported_media");
}

// ===========================================================================
// responses_to_messages_v1 — V5 codex Responses → Anthropic Messages
// ===========================================================================

#[test]
fn responses_to_messages_prepares_codex_request() {
    // Real codex 0.147.0 request shape (§1.1) — must survive the V5 registry
    // prepare path without a codex-field rejection.
    let request = serde_json::json!({
        "model": "deepseek-v4-flash-free",
        "instructions": "You are a helpful assistant.",
        "input": [
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
        ],
        "tools": [
            {"type": "function", "name": "list", "description": "list files", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}}
        ],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "reasoning": {"effort": "high"},
        // The provider-owned preflight forces this control to false before
        // transport; the codec rejects the side-effecting value itself.
        "store": false,
        "stream": true,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": "cache-key",
        "client_metadata": {"turn": "1"}
    });
    let prepared = CodecRegistry::responses_to_messages("oc/deepseek-v4-flash-free", &request)
        .expect("codex Responses request prepares through V5");
    let out = &prepared.encoded_request;
    assert_eq!(out["model"], "oc/deepseek-v4-flash-free");
    assert_eq!(out["stream"], true);
    assert_eq!(out["system"][0]["text"], "You are a helpful assistant.");
    assert_eq!(out["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        out["thinking"],
        serde_json::json!({"type": "enabled", "budget_tokens": 24576})
    );
    assert_eq!(out["max_tokens"], 32000);
    assert_eq!(out["tools"][0]["name"], "list");
    assert_eq!(
        out["tool_choice"],
        serde_json::json!({"type": "auto", "disable_parallel_tool_use": true})
    );
    // Dropped codex-only fields are recorded in the ConversionReport.
    for pointer in ["/parallel_tool_calls", "/store", "/include"] {
        assert!(
            prepared.report.normalized.iter().any(|p| p == pointer),
            "missing dropped-field record {pointer}"
        );
    }
}

#[test]
fn responses_identity_direction_prepares_native_codec() {
    let prepared = CodecRegistry::prepare_pair(
        Protocol::Responses,
        Protocol::Responses,
        "m",
        &json!({ "model": "m", "input": [] }),
    )
    .unwrap();
    assert!(prepared.codec.is_identity());
    assert_eq!(prepared.codec.label(), "native");
}
