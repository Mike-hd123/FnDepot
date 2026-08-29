use super::*;

#[test]
fn handles_crlf_utf8_splits_parallel_tools_and_late_usage() {
    let mut state = AnthropicStreamState::default();
    let parts = [
            b"data: {\"choices\":[{\"delta\":{\"content\":\"h".as_slice(),
            "\u{00e9}".as_bytes(),
            b"\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"b\",\"function\":{\"name\":\"two\",\"arguments\":\"{\\\"b\\\":2}\"}},{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"one\",\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\r\n\r\n".as_slice(),
            b"data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\ndata: {\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n".as_slice(),
        ];
    let mut output = Vec::new();
    for part in parts {
        output.extend(state.feed(part, "model", "msg_1").unwrap());
    }
    output.extend(state.finish("model", "msg_1").unwrap());
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
fn reasoning_content_streams_as_thinking_block_fail_open() {
    let mut state = AnthropicStreamState::default();
    let events = [
            b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"se\"}}]}\n\n".as_ref(),
            b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"cret\"}}]}\n\n".as_ref(),
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n".as_ref(),
            b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n".as_ref(),
        ];
    let mut output = Vec::new();
    for frame in events {
        output.extend(state.feed(frame, "model", "msg_1").unwrap());
    }
    output.extend(state.finish("model", "msg_1").unwrap());
    let joined = output.join("");
    // serde_json here sorts object keys (no preserve_order), so assert on
    // order-independent fragments rather than `"type":"thinking"` adjacency.
    assert!(joined.contains("\"type\":\"thinking\""));
    assert!(joined.contains("\"thinking\":\"se\""));
    assert!(joined.contains("\"thinking\":\"cret\""));
    assert!(joined.contains("\"text\":\"hi\""));
    assert_eq!(
        output
            .iter()
            .filter(|e| e.contains("content_block_stop"))
            .count(),
        2,
        "thinking + text blocks both stop"
    );
}

#[test]
fn infers_tool_use_and_maps_refusal_when_chat_omits_finish_reason() {
    let mut state = AnthropicStreamState::default();
    let tool = b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"run\",\"arguments\":\"{}\"}}]}}]}\n\n";
    state.feed(tool, "model", "msg_1").unwrap();
    assert!(state
        .finish("model", "msg_1")
        .unwrap()
        .join("")
        .contains("\"stop_reason\":\"tool_use\""));

    let mut refusal = AnthropicStreamState::default();
    refusal
        .feed(
            b"data: {\"choices\":[{\"delta\":{\"refusal\":\"no\"}}]}\n\n",
            "model",
            "msg_2",
        )
        .unwrap();
    assert!(refusal
        .finish("model", "msg_2")
        .unwrap()
        .join("")
        .contains("\"stop_reason\":\"refusal\""));
}
