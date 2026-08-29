use crate::protocol::codec::chat;
use serde_json::json;

use super::support::reject_features;

// ===========================================================================
// chat_to_messages_v1 — non-stream response
// ===========================================================================

#[test]
fn chat_response_text_and_finish_mapping() {
    let body = json!({
        "id": "chatcmpl-1",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5}
    });
    let out = chat::decode_chat_response_to_messages(&body, &Default::default()).unwrap();
    assert_eq!(out["content"][0]["type"], "text");
    assert_eq!(out["content"][0]["text"], "hi");
    assert_eq!(out["stop_reason"], "end_turn");
    assert_eq!(out["usage"]["input_tokens"], 10);
    assert_eq!(out["usage"]["output_tokens"], 5);
}

#[test]
fn chat_response_reasoning_content_becomes_thinking_block() {
    // Fail-open (direction A, non-stream): reasoning_content is emitted as a
    // Messages `thinking` block before the text block, always kept even when
    // content is also present.
    let body = json!({
        "id": "chatcmpl-1",
        "choices": [{"index": 0, "message": {
            "role": "assistant",
            "reasoning_content": "chain",
            "content": "answer"
        }, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let out = chat::decode_chat_response_to_messages(&body, &Default::default()).unwrap();
    assert_eq!(out["content"][0]["type"], "thinking");
    assert_eq!(out["content"][0]["thinking"], "chain");
    assert_eq!(out["content"][1]["type"], "text");
    assert_eq!(out["content"][1]["text"], "answer");

    // `{text: ...}` object form of reasoning_content is unwrapped.
    let body = json!({
        "choices": [{"index": 0, "message": {
            "role": "assistant",
            "reasoning_content": {"text": "obj-chain"},
            "content": "answer"
        }, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let out = chat::decode_chat_response_to_messages(&body, &Default::default()).unwrap();
    assert_eq!(out["content"][0]["type"], "thinking");
    assert_eq!(out["content"][0]["thinking"], "obj-chain");
}

#[test]
fn chat_response_maps_length_and_tool_calls() {
    let body = json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": null, "tool_calls": [
            {"id": "call_1", "function": {"name": "run", "arguments": "{\"a\":1}"}}
        ]}, "finish_reason": "tool_calls"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let out = chat::decode_chat_response_to_messages(&body, &Default::default()).unwrap();
    assert_eq!(out["stop_reason"], "tool_use");
    assert_eq!(out["content"][0]["type"], "tool_use");
    assert_eq!(out["content"][0]["input"], json!({"a": 1}));
}

#[test]
fn chat_response_rejects_invalid_tool_arguments() {
    let body = json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": null, "tool_calls": [
            {"id": "call_1", "function": {"name": "run", "arguments": "{bad"}}
        ]}, "finish_reason": "tool_calls"}],
        "usage": {}
    });
    let e = chat::decode_chat_response_to_messages(&body, &Default::default()).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("invalid_tool_arguments")));
    // Array arguments must not become {}.
    let body = json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": null, "tool_calls": [
            {"id": "call_1", "function": {"name": "run", "arguments": "[]"}}
        ]}, "finish_reason": "tool_calls"}],
        "usage": {}
    });
    assert!(chat::decode_chat_response_to_messages(&body, &Default::default()).is_err());
}

#[test]
fn chat_response_unknown_finish_reason_never_becomes_stop() {
    let body = json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "x"}, "finish_reason": "content_steering"}],
        "usage": {}
    });
    let e = chat::decode_chat_response_to_messages(&body, &Default::default()).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("finish_reason")));
}

#[test]
fn chat_response_refusal_maps_to_refusal_not_stop() {
    let body = json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": null, "refusal": "no"}, "finish_reason": "content_filter"}],
        "usage": {}
    });
    let out = chat::decode_chat_response_to_messages(&body, &Default::default()).unwrap();
    assert_eq!(out["stop_reason"], "refusal");
}

#[test]
fn chat_response_no_finish_reason_with_tool_calls_is_tool_use() {
    let body = json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": null, "tool_calls": [
            {"id": "call_1", "function": {"name": "run", "arguments": "{}"}}
        ]}, "finish_reason": null}],
        "usage": {}
    });
    let out = chat::decode_chat_response_to_messages(&body, &Default::default()).unwrap();
    assert_eq!(out["stop_reason"], "tool_use");
}

// ===========================================================================
// chat_to_messages_v1 — streaming
// ===========================================================================

#[test]
fn chat_stream_arbitrary_fragmentation_and_tool_accumulation() {
    let mut state = chat::ChatSseState::default();
    let parts = [
        b"data: {\"choices\":[{\"delta\":{\"content\":\"h".as_slice(),
        "\u{00e9}".as_bytes(),
        b"\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"b\",\"function\":{\"name\":\"two\",\"arguments\":\"{\\\"b\\\":2}\"}},{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"one\",\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\r\n\r\n".as_slice(),
        b"data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\ndata: {\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n".as_slice(),
    ];
    let mut output = Vec::new();
    for part in parts {
        output.extend(state.feed(part).unwrap());
    }
    output.extend(state.finish().unwrap());
    let output = output.join("");
    assert!(output.contains("hé"));
    assert!(output.contains("\"id\":\"a\""));
    assert!(output.contains("\"id\":\"b\""));
    assert!(output.contains("\"input_tokens\":7"));
    assert!(output.contains("\"stop_sequence\":null"));
    let text_stop = output.find("content_block_stop").unwrap();
    let first_tool = output.find("\"type\":\"tool_use\"").unwrap();
    assert!(
        text_stop < first_tool,
        "text must stop before a tool block starts"
    );
    assert!(output.find("\"id\":\"a\"").unwrap() < output.find("\"id\":\"b\"").unwrap());
    assert_eq!(output.matches("event: message_stop").count(), 1);
}

#[test]
fn chat_stream_tool_call_without_upstream_id_gets_generated_id() {
    let mut state = chat::ChatSseState::default();
    let mut output = Vec::new();
    output.extend(state.feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"run\",\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n").unwrap());
    output.extend(state.feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n").unwrap());
    output.extend(state.finish().unwrap());
    let output = output.join("");
    assert!(output.contains("\"type\":\"tool_use\""));
    assert!(output.contains("\"name\":\"run\""));
    assert!(output.contains("\"id\":\"call_"));
    assert!(output.contains("\"partial_json\":\"{\\\"a\\\":1}\""));
    assert!(output.contains("\"stop_reason\":\"tool_use\""));
}

#[test]
fn chat_stream_tool_arguments_can_arrive_before_name() {
    let mut state = chat::ChatSseState::default();
    let mut output = Vec::new();
    output.extend(state.feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n").unwrap());
    output.extend(state.feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"run\",\"arguments\":\"1}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n").unwrap());
    output.extend(state.finish().unwrap());
    let output = output.join("");
    assert!(output.contains("\"id\":\"call_1\""));
    assert!(output.contains("\"name\":\"run\""));
    assert!(output.contains("\"partial_json\":\"{\\\"a\\\":1}\""));
    assert!(output.contains("\"stop_reason\":\"tool_use\""));
}

#[test]
fn chat_stream_empty_tool_name_delta_does_not_clear_name() {
    let mut state = chat::ChatSseState::default();
    let mut output = Vec::new();
    output.extend(state.feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"\"}}]}}]}\n\n").unwrap());
    output.extend(state.feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"\",\"arguments\":\"{\\\"path\\\":\\\"/tmp/x\\\"}\"}}]}}]}\n\n").unwrap());
    output.extend(state.feed(b"data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"tool_calls\"}]}\n\n").unwrap());
    output.extend(state.finish().unwrap());
    let output = output.join("");
    assert!(output.contains("\"id\":\"call_1\""));
    assert!(output.contains("\"name\":\"read\""));
    assert!(output.contains("\"partial_json\":\"{\\\"path\\\":\\\"/tmp/x\\\"}\""));
    assert!(output.contains("\"stop_reason\":\"tool_use\""));
}

#[test]
fn chat_stream_incomplete_tool_arguments_are_rejected() {
    let mut state = chat::ChatSseState::default();
    state.feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"run\",\"arguments\":\"{bad\"}}]}}]}\n\n").unwrap();
    let e = state.finish().unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("invalid_tool_arguments")));
}

#[test]
fn chat_stream_unknown_finish_reason_completes_as_end_turn() {
    // A provider-specific terminal finish reason after an otherwise valid Chat
    // stream is conservatively mapped to Anthropic `end_turn` rather than
    // aborting a committed stream (CPA compatibility).
    let mut state = chat::ChatSseState::default();
    let mut output = Vec::new();
    output.extend(
        state
            .feed(b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n")
            .unwrap(),
    );
    output.extend(
        state
            .feed(b"data: {\"choices\":[{\"finish_reason\":\"bizarre\"}]}\n\n")
            .unwrap(),
    );
    output.extend(state.finish().unwrap());
    let output = output.join("");
    assert!(output.contains("\"stop_reason\":\"end_turn\""));
    assert!(output.contains("event: message_stop"));
}

#[test]
fn chat_stream_first_frame_invalid_is_a_codec_error() {
    let mut state = chat::ChatSseState::default();
    let e = state.feed(b"data: {not-json}\n\n").unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unknown_event")));
}

#[test]
fn chat_stream_termination_exactly_once() {
    let mut state = chat::ChatSseState::default();
    state
        .feed(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"},\"finish_reason\":\"stop\"}]}\n\n",
        )
        .unwrap();
    let first = state.finish().unwrap();
    assert_eq!(
        first.iter().filter(|e| e.contains("message_stop")).count(),
        1
    );
    // finish() again is a no-op.
    let second = state.finish().unwrap();
    assert!(second.is_empty());
}

#[test]
fn chat_stream_empty_stream_is_a_codec_error_not_an_empty_success() {
    // F4: a stream that closes before any first frame must surface a codec
    // error (for pre-commit failover), never a silent empty Ok.
    let mut state = chat::ChatSseState::default();
    state.feed(b"").unwrap();
    let e = state.finish().unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unknown_event")));

    let mut state = chat::ChatSseState::default();
    state.feed(b"data: [DONE]\n\n").unwrap();
    let e = state.finish().unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unknown_event")));
}

#[test]
fn chat_stream_emits_prepared_model_and_request_id() {
    // F5: the streaming decoder must thread the mapped upstream model and the
    // per-request id from the PreparedAttempt context into the synthesized
    // message_start frame.
    let mut state = chat::ChatSseState::new("upstream-model-9", "req-42");
    let events = state
        .feed(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"},\"finish_reason\":\"stop\"}]}\n\n",
        )
        .unwrap();
    let joined = events.join("");
    assert!(joined.contains("\"model\":\"upstream-model-9\""));
    assert!(joined.contains("\"id\":\"req-42\""));
    state.finish().unwrap();
}
