use crate::protocol::codec::chat;
use crate::protocol::codec::messages;
use serde_json::json;

use super::support::reject_features;

#[test]
fn messages_response_tool_use_requires_input_not_fabricated() {
    // R8/R21 response side: a non-stream tool_use without `input` is rejected,
    // not fabricated as `{}`.
    let body = json!({
        "id": "msg_1", "type": "message",
        "content": [{"type": "tool_use", "id": "c", "name": "run"}],
        "stop_reason": "tool_use", "usage": {}
    });
    let e = messages::decode_messages_response_to_chat(&body, &Default::default()).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("missing_tool_field")));
    assert!(e.json_pointers.iter().any(|p| p.ends_with("/input")));
}

// ===========================================================================
// messages_to_chat_v1 — non-stream response
// ===========================================================================

#[test]
fn messages_response_text_and_tool_use() {
    let body = json!({
        "id": "msg_1",
        "type": "message",
        "content": [
            {"type": "text", "text": "hello"},
            {"type": "tool_use", "id": "call_1", "name": "run", "input": {"a": 1}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let out = messages::decode_messages_response_to_chat(&body, &Default::default()).unwrap();
    assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(out["choices"][0]["message"]["content"], "hello");
    assert_eq!(
        out["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"a\":1}"
    );
    assert_eq!(out["usage"]["prompt_tokens"], 10);
    assert_eq!(out["usage"]["completion_tokens"], 5);
}

#[test]
fn messages_response_maps_stop_reasons() {
    let base = |stop: &str| {
        json!({
            "id": "msg_1", "type": "message", "content": [{"type": "text", "text": "x"}],
            "stop_reason": stop, "usage": {"input_tokens": 1, "output_tokens": 1}
        })
    };
    assert_eq!(
        messages::decode_messages_response_to_chat(&base("end_turn"), &Default::default()).unwrap()
            ["choices"][0]["finish_reason"],
        "stop"
    );
    assert_eq!(
        messages::decode_messages_response_to_chat(&base("max_tokens"), &Default::default())
            .unwrap()["choices"][0]["finish_reason"],
        "length"
    );
    // Stop-like reasons collapse to `stop` (refusal text rides in content).
    for stop in ["refusal", "stop_sequence", "pause_turn"] {
        assert_eq!(
            messages::decode_messages_response_to_chat(&base(stop), &Default::default()).unwrap()
                ["choices"][0]["finish_reason"],
            "stop",
            "stop_reason {stop:?} should map to stop, not error"
        );
    }
    // Context-window exhaustion behaves like max_tokens (truncation).
    assert_eq!(
        messages::decode_messages_response_to_chat(
            &base("model_context_window_exceeded"),
            &Default::default()
        )
        .unwrap()["choices"][0]["finish_reason"],
        "length"
    );
    // Unknown stop reason is rejected, never mapped to stop.
    let e = messages::decode_messages_response_to_chat(&base("budget_forced"), &Default::default())
        .unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("finish_reason")));
}

#[test]
fn messages_response_thinking_fail_open_and_bad_input_rejected() {
    // Fail-open: a Messages response `thinking` block is surfaced as OpenAI
    // `reasoning_content`, never rejected.
    let body = json!({
        "id": "msg_1", "type": "message",
        "content": [{"type": "thinking", "thinking": "..."}],
        "stop_reason": "end_turn", "usage": {}
    });
    let out = messages::decode_messages_response_to_chat(&body, &Default::default()).unwrap();
    assert_eq!(out["choices"][0]["message"]["reasoning_content"], "...");
    // reasoning only -> content stays null (no fabricated empty text)
    assert!(out["choices"][0]["message"]["content"].is_null());

    let body = json!({
        "id": "msg_1", "type": "message",
        "content": [{"type": "tool_use", "id": "c", "name": "run", "input": [1]}],
        "stop_reason": "tool_use", "usage": {}
    });
    let e = messages::decode_messages_response_to_chat(&body, &Default::default()).unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("invalid_tool_arguments")));
}

// ===========================================================================
// messages_to_chat_v1 — streaming
// ===========================================================================

#[test]
fn messages_stream_text_and_tool_deltas() {
    let mut state = messages::MessagesSseState::default();
    let mut events = Vec::new();
    events.extend(state.feed(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":5}}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n").unwrap());
    events.extend(state.feed(b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n").unwrap());
    events.extend(
        state
            .feed(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")
            .unwrap(),
    );
    events.extend(state.finish().unwrap());
    let joined = events.join("");
    assert!(joined.contains("\"role\":\"assistant\""));
    assert!(joined.contains("\"content\":\"hel\""));
    assert!(joined.contains("\"content\":\"lo\""));
    assert!(joined.contains("\"finish_reason\":\"stop\""));
    assert!(joined.contains("\"prompt_tokens\":5"));
    assert!(joined.contains("\"completion_tokens\":2"));
    assert_eq!(events.iter().filter(|e| e.contains("[DONE]")).count(), 1);
}

#[test]
fn messages_stream_stop_like_reasons_normalize_not_error() {
    // Streaming path must treat stop-like / context-window stop reasons as a
    // normal completion, never a hard codec error (retry instead of interrupt).
    let stream = |stop_reason: &str| {
        let mut state = messages::MessagesSseState::default();
        let mut events = Vec::new();
        events.extend(
            state
                .feed(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":5}}}\n\n")
                .unwrap(),
        );
        events.extend(
            state
                .feed(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n")
                .unwrap(),
        );
        events.extend(
            state
                .feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n")
                .unwrap(),
        );
        events.extend(
            state
                .feed(b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n")
                .unwrap(),
        );
        let delta = format!(
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{stop_reason}\"}},\"usage\":{{\"output_tokens\":2}}}}\n\n"
        );
        events.extend(state.feed(delta.as_bytes()).unwrap());
        events.extend(
            state
                .feed(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")
                .unwrap(),
        );
        events.extend(state.finish().unwrap());
        events.join("")
    };

    assert!(stream("refusal").contains("\"finish_reason\":\"stop\""));
    assert!(stream("pause_turn").contains("\"finish_reason\":\"stop\""));
    assert!(stream("stop_sequence").contains("\"finish_reason\":\"stop\""));
    assert!(stream("model_context_window_exceeded").contains("\"finish_reason\":\"length\""));
}

#[test]
fn messages_stream_thinking_fail_open_to_reasoning_content() {
    // Fail-open (direction B, streaming): a Messages `thinking` block is
    // surfaced as OpenAI `reasoning_content` deltas, never rejected.
    let mut state = messages::MessagesSseState::default();
    let mut events = Vec::new();
    events.extend(state.feed(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":5}}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"se\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"cret\"}}\n\n").unwrap());
    // signature_delta carries no visible text; dropped fail-open.
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"abc\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n").unwrap());
    events.extend(state.feed(b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n").unwrap());
    events.extend(
        state
            .feed(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")
            .unwrap(),
    );
    events.extend(state.finish().unwrap());
    let joined = events.join("");
    assert!(joined.contains("\"reasoning_content\":\"se\""));
    assert!(joined.contains("\"reasoning_content\":\"cret\""));
    assert!(joined.contains("\"finish_reason\":\"stop\""));
    assert!(!joined.contains("\"content\":\"se\"") || joined.contains("\"reasoning_content\""));
}

#[test]
fn chat_stream_reasoning_fail_open_to_thinking_block() {
    // Fail-open (direction A, streaming): a Chat `reasoning_content` delta is
    // emitted as a Messages `thinking` block, never rejected.
    let mut state = chat::ChatSseState::new("up-model", "msg_1");
    let mut events = Vec::new();
    events.extend(
        state
            .feed(b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"se\"}}]}\n\n")
            .unwrap(),
    );
    events.extend(
        state
            .feed(b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"cret\"}}]}\n\n")
            .unwrap(),
    );
    events.extend(
        state
            .feed(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n")
            .unwrap(),
    );
    events.extend(state.feed(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n").unwrap());
    events.extend(state.finish().unwrap());
    let joined = events.join("");
    // serde_json here sorts object keys (no preserve_order), so assert on
    // order-independent fragments rather than `"type":"thinking"` adjacency.
    assert!(joined.contains("\"type\":\"thinking\""));
    assert!(joined.contains("\"thinking\":\"se\""));
    assert!(joined.contains("\"thinking\":\"cret\""));
    assert!(joined.contains("\"text\":\"hi\""));
    // both blocks are stopped exactly once.
    assert_eq!(
        events
            .iter()
            .filter(|e| e.contains("content_block_stop"))
            .count(),
        2,
        "thinking + text blocks both stop"
    );
}

#[test]
fn messages_stream_tool_calls_accumulate_by_index() {
    let mut state = messages::MessagesSseState::default();
    let mut events = Vec::new();
    events.extend(state.feed(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{}}}\n\n").unwrap());
    // Two parallel tool blocks (index 0 and 1); deltas interleave.
    events.extend(state.feed(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_a\",\"name\":\"one\",\"input\":{}}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"a\\\"\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_b\",\"name\":\"two\",\"input\":{}}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"b\\\":2}\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\":1}\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n").unwrap());
    events.extend(state.feed(b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{}}\n\n").unwrap());
    events.extend(
        state
            .feed(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")
            .unwrap(),
    );
    events.extend(state.finish().unwrap());
    let joined = events.join("");
    assert!(joined.contains("\"id\":\"call_a\""));
    assert!(joined.contains("\"name\":\"one\""));
    assert!(joined.contains("\"id\":\"call_b\""));
    assert!(joined.contains("\"name\":\"two\""));
    assert!(joined.contains("\"arguments\":\"{\\\"a\\\":1}\""));
    assert!(joined.contains("\"arguments\":\"{\\\"b\\\":2}\""));
    assert!(joined.contains("\"finish_reason\":\"tool_calls\""));
    assert_eq!(events.iter().filter(|e| e.contains("[DONE]")).count(), 1);
}

#[test]
fn messages_stream_invalid_tool_json_is_rejected() {
    let mut state = messages::MessagesSseState::default();
    state.feed(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{}}}\n\n").unwrap();
    state.feed(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"c\",\"name\":\"run\",\"input\":{}}}\n\n").unwrap();
    state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{bad\"}}\n\n").unwrap();
    // content_block_stop validates the accumulated arguments and must reject the
    // malformed JSON rather than invent `{}`.
    let e = state
        .feed(b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n")
        .unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("invalid_tool_arguments")));
}

#[test]
fn messages_stream_unknown_event_is_a_codec_error() {
    let mut state = messages::MessagesSseState::default();
    let e = state
        .feed(b"event: wat\ndata: {\"type\":\"wat\"}\n\n")
        .unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unknown_event")));
}

#[test]
fn messages_stream_fragmented_utf8_and_crlf() {
    let mut state = messages::MessagesSseState::default();
    let mut events = Vec::new();
    let payload = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{}}}\r\n\r\n";
    for chunk in payload.as_bytes().chunks(5) {
        events.extend(state.feed(chunk).unwrap());
    }
    events.extend(state.feed("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\r\n\r\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"h\u{00e9}\"}}\r\n\r\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\r\n\r\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\r\n\r\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\r\n\r\n".as_bytes()).unwrap());
    events.extend(state.finish().unwrap());
    assert!(events.join("").contains("h\u{00e9}"));
}

#[test]
fn messages_stream_empty_stream_is_a_codec_error_not_an_empty_success() {
    // F4: a Messages stream that closes before any message_start frame must
    // surface a codec error for pre-commit failover, never an empty Ok.
    let mut state = messages::MessagesSseState::default();
    state.feed(b"").unwrap();
    let e = state.finish().unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unknown_event")));

    let mut state = messages::MessagesSseState::default();
    state
        .feed(b"event: ping\ndata: {\"type\":\"ping\"}\n\n")
        .unwrap();
    let e = state.finish().unwrap_err();
    assert!(reject_features(&e)
        .iter()
        .any(|c| c.contains("unknown_event")));
}

#[test]
fn messages_stream_emits_prepared_model() {
    // F5: the Messages→Chat streaming decoder must thread the mapped upstream
    // model from the PreparedAttempt context into the synthesized Chat role
    // frame (never a hardcoded empty model).
    let mut state = messages::MessagesSseState::new("upstream-model-9");
    let mut events = Vec::new();
    events.extend(state.feed(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{}}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n").unwrap());
    events.extend(state.feed(b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n").unwrap());
    events.extend(state.feed(b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n\n").unwrap());
    events.extend(
        state
            .feed(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n")
            .unwrap(),
    );
    events.extend(state.finish().unwrap());
    let joined = events.join("");
    assert!(joined.contains("\"model\":\"upstream-model-9\""));
}
