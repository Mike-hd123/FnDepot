use crate::protocol::codec::registry::CodecRegistry;
use serde_json::json;

use super::support::reject_features;

// ===========================================================================
// chat_to_messages_v1 — request encoding
// ===========================================================================

#[test]
fn chat_request_text_system_and_sampling() {
    let body = json!({
        "model": "public-model",
        "max_tokens": 128,
        "temperature": 0.7,
        "top_p": 0.9,
        "stop": ["END"],
        "stream": false,
        "messages": [
            {"role": "system", "content": "be brief"},
            {"role": "developer", "content": "follow up"},
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"}
        ]
    });
    let prepared = CodecRegistry::chat_to_messages("upstream-model", &body).unwrap();
    let out = &prepared.encoded_request;
    assert_eq!(out["model"], "upstream-model");
    assert_eq!(out["max_tokens"], 128);
    assert_eq!(out["temperature"], 0.7);
    assert_eq!(out["top_p"], 0.9);
    assert_eq!(out["stop_sequences"], json!(["END"]));
    assert_eq!(out["stream"], false);
    // system and developer are ordered and hoisted to top-level system.
    let system = out["system"].as_array().unwrap();
    assert_eq!(system[0]["text"], "be brief");
    assert_eq!(system[1]["text"], "follow up");
    // messages contain only user/assistant.
    let msgs = out["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(prepared.codec.context().upstream_model, "upstream-model");
}

#[test]
fn chat_request_function_tools_and_choice() {
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "weather",
                "description": "get weather",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
            }
        }],
        "tool_choice": {"type": "function", "function": {"name": "weather"}}
    });
    let prepared = CodecRegistry::chat_to_messages("m", &body).unwrap();
    let out = &prepared.encoded_request;
    assert_eq!(out["tools"][0]["name"], "weather");
    assert_eq!(out["tools"][0]["input_schema"]["type"], "object");
    assert_eq!(
        out["tool_choice"],
        json!({"type": "tool", "name": "weather"})
    );
}

#[test]
fn chat_request_normalizes_store_false_and_stream_usage() {
    // Exact OpenAI Chat request shape emitted by the client that routes through
    // an Anthropic Messages-only channel.
    let body = json!({
        "model": "deepseek-v4-flash",
        "messages": [{"role": "user", "content": "hi"}],
        "stream": true,
        "store": false,
        "stream_options": {"include_usage": true}
    });

    let prepared = CodecRegistry::chat_to_messages("oc/deepseek-v4-flash-free", &body).unwrap();
    let out = &prepared.encoded_request;
    assert_eq!(out["model"], "oc/deepseek-v4-flash-free");
    assert_eq!(out["stream"], true);
    assert!(out.get("store").is_none());
    assert!(out.get("stream_options").is_none());
    assert_eq!(
        prepared.report.normalized,
        vec!["/store", "/stream_options"]
    );
}

#[test]
fn chat_request_rejects_unrepresentable_store_and_stream_options() {
    let store_true = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "store": true
    });
    let error = CodecRegistry::chat_to_messages("m", &store_true).unwrap_err();
    assert!(error.json_pointers.iter().any(|p| p == "/store"));

    let include_usage_false = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "stream": true,
        "stream_options": {"include_usage": false}
    });
    let error = CodecRegistry::chat_to_messages("m", &include_usage_false).unwrap_err();
    assert!(error
        .json_pointers
        .iter()
        .any(|p| p == "/stream_options/include_usage"));

    let unknown_option = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "stream": true,
        "stream_options": {"include_usage": true, "future_option": true}
    });
    let error = CodecRegistry::chat_to_messages("m", &unknown_option).unwrap_err();
    assert!(error
        .json_pointers
        .iter()
        .any(|p| p == "/stream_options/future_option"));
}

#[test]
fn chat_request_tool_calls_and_results_are_strict() {
    let body = json!({
        "model": "m",
        "messages": [
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "run", "arguments": "{\"a\":1}"}}
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "done"}
        ]
    });
    let prepared = CodecRegistry::chat_to_messages("m", &body).unwrap();
    let out = &prepared.encoded_request;
    let msgs = out["messages"].as_array().unwrap();
    // assistant -> tool_use block
    assert_eq!(msgs[0]["content"][0]["type"], "tool_use");
    assert_eq!(msgs[0]["content"][0]["id"], "call_1");
    assert_eq!(msgs[0]["content"][0]["input"], json!({"a": 1}));
    // tool -> user tool_result as a content block (canonical Anthropic shape).
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[1]["content"][0]["type"], "tool_result");
    assert_eq!(msgs[1]["content"][0]["tool_use_id"], "call_1");
    assert_eq!(msgs[1]["content"][0]["content"][0]["type"], "text");
    assert_eq!(msgs[1]["content"][0]["content"][0]["text"], "done");
    assert!(
        msgs[1].get("tool_result").is_none(),
        "no message-level tool_result key"
    );
}

#[test]
fn chat_request_consecutive_tool_results_aggregate_into_one_user_message() {
    let body = json!({
        "model": "m",
        "messages": [
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "a", "arguments": "{}"}},
                {"id": "call_2", "type": "function", "function": {"name": "b", "arguments": "{}"}}
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "first"},
            {"role": "tool", "tool_call_id": "call_2", "content": "second"}
        ]
    });
    let prepared = CodecRegistry::chat_to_messages("m", &body).unwrap();
    let msgs = prepared.encoded_request["messages"].as_array().unwrap();
    // assistant + a SINGLE user message carrying both tool_result blocks.
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "assistant");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[1]["content"].as_array().unwrap().len(), 2);
    assert_eq!(msgs[1]["content"][0]["tool_use_id"], "call_1");
    assert_eq!(msgs[1]["content"][1]["tool_use_id"], "call_2");
    assert_eq!(msgs[1]["content"][0]["content"][0]["text"], "first");
    assert_eq!(msgs[1]["content"][1]["content"][0]["text"], "second");
}

#[test]
fn chat_request_user_images() {
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,aGVsbG8="}}
        ]}]
    });
    let prepared = CodecRegistry::chat_to_messages("m", &body).unwrap();
    let out = &prepared.encoded_request;
    assert_eq!(out["messages"][0]["content"][0]["type"], "image");
    assert_eq!(out["messages"][0]["content"][0]["source"]["type"], "base64");
    assert_eq!(
        out["messages"][0]["content"][0]["source"]["media_type"],
        "image/png"
    );
    // F2: no non-canonical `_media_type` key on the image block.
    assert!(out["messages"][0]["content"][0]
        .get("_media_type")
        .is_none());
}

#[test]
fn chat_request_rejects_invalid_images() {
    // R15: Chat image_url must be a valid image — non-image media type, or a
    // non-http(s) url, is rejected rather than forwarded.
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"url": "data:application/octet-stream;base64,aGVsbG8="}}
        ]}]
    });
    let e = CodecRegistry::chat_to_messages("m", &body).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unsupported_media")));

    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"url": "javascript:alert(1)"}}
        ]}]
    });
    let e = CodecRegistry::chat_to_messages("m", &body).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unsupported_media")));
}

#[test]
fn chat_request_rejects_n_gt_1_instead_of_silently_dropping() {
    // `n` is not in the support matrix: Messages always returns one completion,
    // so n>1 must be rejected (never silently yield a single completion).
    let body = json!({
        "model": "m",
        "n": 2,
        "messages": [{"role": "user", "content": "u"}]
    });
    let e = CodecRegistry::chat_to_messages("m", &body).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unsupported_feature.field")));
    assert!(e.json_pointers.iter().any(|p| p == "/n"));
}

#[test]
fn chat_request_rejects_thinking_and_structured_output() {
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "response_format": {"type": "json_schema", "json_schema": {}}
    });
    let e = CodecRegistry::chat_to_messages("m", &body).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("structured_output")));
    assert!(e.json_pointers.iter().any(|p| p == "/response_format"));
}

#[test]
fn chat_request_reasoning_effort_maps_to_thinking() {
    // CPA ConvertOpenAIRequestToClaude + MapToClaudeEffort, exercised directly.
    // none/off cannot disable thinking on always-thinking upstream models;
    // fall back to their least-expensive supported effort.
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "reasoning_effort": "none"
    });
    let out = &CodecRegistry::chat_to_messages("m", &body)
        .unwrap()
        .encoded_request;
    assert_eq!(out["thinking"], json!({"type": "adaptive"}));
    assert_eq!(out["output_config"], json!({"effort": "low"}));

    // auto -> adaptive (no budget)
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "reasoning_effort": "auto"
    });
    let out = &CodecRegistry::chat_to_messages("m", &body)
        .unwrap()
        .encoded_request;
    assert_eq!(out["thinking"], json!({"type": "adaptive"}));
    assert!(out.get("output_config").is_none());

    // medium -> adaptive + output_config.effort=medium
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "reasoning_effort": "medium"
    });
    let out = &CodecRegistry::chat_to_messages("m", &body)
        .unwrap()
        .encoded_request;
    assert_eq!(out["thinking"], json!({"type": "adaptive"}));
    assert_eq!(out["output_config"], json!({"effort": "medium"}));

    // xhigh (no model registry) -> collapses to high (MapToClaudeEffort)
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "reasoning_effort": "xhigh"
    });
    let out = &CodecRegistry::chat_to_messages("m", &body)
        .unwrap()
        .encoded_request;
    assert_eq!(out["output_config"], json!({"effort": "high"}));
}

#[test]
fn chat_request_rejects_unknown_role_and_builtin_tool() {
    let body = json!({
        "model": "m",
        "messages": [{"role": "system", "content": "x"}, {"role": "tool", "tool_call_id": "t", "content": "x"}]
    });
    // No prior assistant tool_call -> tool message without id should fail (but
    // here id is present; role tool without matching assistant tool is a
    // strictness case).  This must not invent an assistant message.
    let body = json!({
        "model": "m",
        "messages": [
            {"role": "function", "content": "x"}
        ]
    });
    let e = CodecRegistry::chat_to_messages("m", &body).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unknown_role")));

    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "u"}],
        "tools": [{"type": "web_search", "function": {"name": "x"}}]
    });
    let e = CodecRegistry::chat_to_messages("m", &body).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("builtin_tool")));
}

#[test]
fn chat_request_rejects_invalid_tool_arguments_never_rewrites() {
    let body = json!({
        "model": "m",
        "messages": [
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "run", "arguments": "{bad"}}
            ]}
        ]
    });
    let e = CodecRegistry::chat_to_messages("m", &body).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("invalid_tool_arguments")));
    // The non-object argument case (array) must also fail, not become {}.
    let body = json!({
        "model": "m",
        "messages": [
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "run", "arguments": "[]"}}
            ]}
        ]
    });
    assert!(CodecRegistry::chat_to_messages("m", &body).is_err());
}
