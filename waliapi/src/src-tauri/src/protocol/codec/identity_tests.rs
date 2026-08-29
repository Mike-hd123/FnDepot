use super::*;

fn decoder() -> IdentityStreamDecoder {
    IdentityStreamDecoder {
        protocol: Protocol::Chat,
        pending: Vec::new(),
        usage: None,
        saw_input_usage: false,
        saw_output_usage: false,
        done: false,
    }
}

#[test]
fn ignores_empty_sse_records_after_done() {
    let mut decoder = decoder();
    assert_eq!(decoder.feed(b"data: [DONE]\n\n").unwrap().len(), 1);

    assert!(decoder.feed(b": keepalive\n\n").unwrap().is_empty());
    assert!(decoder.feed(b"event: ping\n\n").unwrap().is_empty());
    assert!(decoder.finish().is_ok());
}

#[test]
fn discards_any_trailing_record_after_done_but_preserves_its_usage() {
    let mut decoder = decoder();
    decoder.feed(b"data: [DONE]\n\n").unwrap();

    assert!(decoder
            .feed(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"extra\"}}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n"
            )
            .unwrap()
            .is_empty());
    assert_eq!(decoder.usage().unwrap().input_tokens, 2);
    assert_eq!(decoder.usage().unwrap().output_tokens, 3);
}

#[test]
fn discards_response_content_after_done_even_in_same_network_chunk() {
    let mut decoder = decoder();
    assert!(
        decoder
            .feed(b"data: [DONE]\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"extra\"}}]}\n\n")
            .unwrap()
            .len()
            == 1
    );
}

#[test]
fn native_chat_preserves_reasoning_text_in_assistant_history() {
    let request = serde_json::json!({
        "model": "client-model",
        "stream": true,
        "messages": [{
            "role": "assistant",
            "content": "answer",
            "reasoning_text": "private reasoning to continue the tool loop",
            "reasoning_content": "provider-compatible reasoning"
        }]
    });

    let (encoded, _) = CHAT_IDENTITY
        .encode_request(&request, "upstream-model")
        .expect("native Chat request must be encodable");

    assert_eq!(encoded["model"], "upstream-model");
    assert_eq!(
        encoded["messages"][0]["reasoning_text"],
        "private reasoning to continue the tool loop"
    );
    assert_eq!(
        encoded["messages"][0]["reasoning_content"],
        "provider-compatible reasoning"
    );
}

#[test]
fn native_messages_omitted_stream_is_pinned_false() {
    // A downstream non-stream Messages request omits `stream`; the identity
    // codec must pin `stream: false` so default-streaming upstreams (e.g.
    // anthropic proxies) do not return SSE into the non-stream facade.
    let request = serde_json::json!({
        "model": "client-model",
        "max_tokens": 256,
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (encoded, _) = MESSAGES_IDENTITY
        .encode_request(&request, "upstream-model")
        .expect("native Messages request must be encodable");

    assert_eq!(encoded["stream"], false);
    assert_eq!(encoded["model"], "upstream-model");
}

#[test]
fn native_explicit_stream_true_is_preserved() {
    let request = serde_json::json!({
        "model": "client-model",
        "stream": true,
        "messages": [{"role": "user", "content": "hi"}]
    });

    let (encoded, _) = MESSAGES_IDENTITY
        .encode_request(&request, "upstream-model")
        .expect("native Messages request must be encodable");

    assert_eq!(encoded["stream"], true);
}
