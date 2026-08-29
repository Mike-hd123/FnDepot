use super::*;

/// Split a record into two mid-JSON chunks (as TCP fragmentation does).
fn split_mid(record: &str) -> (String, String) {
    let mid = record.len() / 2;
    (record[..mid].to_string(), record[mid..].to_string())
}

#[test]
fn anthropic_stream_converts_to_openai_records() {
    let mut bridge = UpstreamSseBridge::for_upstream(true, "claude-3-5");
    let out = bridge
            .push(
                b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":5}}}\n\n",
            )
            .unwrap();
    // role frame first
    assert_eq!(out.len(), 1);
    let role = serde_json::from_str::<serde_json::Value>(out[0].trim_start_matches("data:").trim())
        .unwrap();
    assert_eq!(role["choices"][0]["delta"]["role"], "assistant");

    let out = bridge
            .push(
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"你\"}}\n\n"
                    .as_bytes(),
            )
            .unwrap();
    assert_eq!(out.len(), 1);
    let delta =
        serde_json::from_str::<serde_json::Value>(out[0].trim_start_matches("data:").trim())
            .unwrap();
    assert_eq!(delta["choices"][0]["delta"]["content"], "你");

    let out = bridge
            .push(
                b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n\
                 event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            )
            .unwrap();
    // finish() is what emits the final sequence; message_stop alone is a no-op.
    assert!(out.is_empty());

    let out = bridge.finish().unwrap();
    let joined = out.join("");
    assert!(joined.contains("\"finish_reason\":\"stop\""));
    assert!(joined.contains("\"usage\""));
    assert!(joined.contains("\"prompt_tokens\":5"));
    assert!(joined.contains("\"completion_tokens\":2"));
    assert!(joined.ends_with("data: [DONE]\n\n"));
}

#[test]
fn anthropic_done_is_normalized_to_unquoted() {
    let mut bridge = UpstreamSseBridge::for_upstream(true, "claude-3-5");
    bridge
            .push(
                b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{}}}\n\n",
            )
            .unwrap();
    bridge
            .push(
                b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n\n\
                 event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            )
            .unwrap();
    let out = bridge.finish().unwrap();
    assert_eq!(out.last().unwrap(), "data: [DONE]\n\n");
}

/// Regression (Opencode "Type validation failed ... expected array for
/// `choices`"): the final sequence must never contain a bare `{"usage":..}`
/// frame, because OpenAI-compat clients require every chunk to carry
/// `choices`.  Usage must be merged into the last finish_reason frame.
#[test]
fn final_sequence_merges_usage_into_finish_reason_frame() {
    let mut bridge = UpstreamSseBridge::for_upstream(true, "claude-3-5");
    bridge
            .push(
                b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":7}}}\n\n\
                 event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
            )
            .unwrap();
    bridge
            .push(
                b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n\
                 event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            )
            .unwrap();
    let out = bridge.finish().unwrap();
    // Exactly the finish_reason+usage frame, then [DONE] — no extra frames.
    assert_eq!(
        out.len(),
        2,
        "final sequence must be [finish+usage, DONE], got: {out:?}"
    );
    let last = out[0].trim_start_matches("data:").trim();
    let json: serde_json::Value = serde_json::from_str(last).unwrap();
    // Every non-[DONE] frame must carry `choices`.
    assert!(
        json.get("choices").is_some(),
        "final frame must carry choices (bare usage frame rejected by Opencode): {json}"
    );
    assert_eq!(json["choices"][0]["finish_reason"], "stop");
    assert_eq!(json["usage"]["prompt_tokens"], 7);
    assert_eq!(json["usage"]["completion_tokens"], 3);
    assert_eq!(out[1], "data: [DONE]\n\n");
}

/// Regression (Opencode "Type validation failed ... expected array for
/// `choices`" on a raw `content_block_delta` frame): a TCP/HTTP chunk that
/// ends in the middle of a UTF-8 codepoint (e.g. a 3-byte Chinese char in
/// `text`/`thinking`) makes `std::str::from_utf8` fail.  The OLD handler
/// treated that as "not valid UTF-8" and yielded the raw Anthropic record
/// straight to the OpenAI client.  The bridge must instead hold the partial
/// record in its byte buffer and convert it once the next chunk completes
/// the codepoint.
#[test]
fn anthropic_chunk_split_mid_codepoint_is_converted_not_leaked() {
    let mut bridge = UpstreamSseBridge::for_upstream(true, "claude-3-5");
    // A complete Anthropic text_delta record carrying a 2-char Chinese word.
    // 现 = e7 8e b0, 状 = e7 8a b6 (3 bytes each).
    let full = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"\xe7\x8e\xb0\xe7\x8a\xb6\"}}\n\n";
    let cut = full
        .windows(6)
        .position(|w| w == b"\xe7\x8e\xb0\xe7\x8a\xb6")
        .unwrap()
        + 2; // split 2 bytes INTO 现
    let (a, b) = full.split_at(cut);
    // The trigger: this exact boundary is INVALID as a &str...
    assert!(
        std::str::from_utf8(a).is_err(),
        "test setup: chunk a must end mid-codepoint so from_utf8 fails"
    );
    // ...yet the bridge must accept the bytes and buffer the record, NOT
    // return a raw frame.
    let out = bridge.push(a).unwrap();
    assert!(
        out.is_empty(),
        "partial record must be held, never surfaced as a raw frame: {out:?}"
    );
    let out = bridge.push(b).unwrap();
    assert_eq!(out.len(), 1, "completed record must convert: {out:?}");
    let json: serde_json::Value =
        serde_json::from_str(out[0].trim_start_matches("data:").trim()).unwrap();
    assert_eq!(json["choices"][0]["delta"]["content"], "现状");
    // Every emitted frame must carry `choices` (OpenAI-compat contract).
    assert!(
        json.get("choices").is_some(),
        "converted frame must carry choices: {json}"
    );
}

#[test]
fn anthropic_fragmented_records_are_reassembled() {
    let mut bridge = UpstreamSseBridge::for_upstream(true, "claude-3-5");
    // Feed record bytes split mid-JSON across push calls (the real upstream
    // fragmentation pattern that silently dropped content before the fix).
    let (a, b) = split_mid(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{}}}\n\n",
        );
    assert!(bridge.push(a.as_bytes()).unwrap().is_empty());
    let out = bridge.push(b.as_bytes()).unwrap();
    assert_eq!(out.len(), 1);

    let (c, d) = split_mid(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
        );
    assert!(bridge.push(c.as_bytes()).unwrap().is_empty());
    let out = bridge.push(d.as_bytes()).unwrap();
    assert_eq!(out.len(), 1);
    let delta =
        serde_json::from_str::<serde_json::Value>(out[0].trim_start_matches("data:").trim())
            .unwrap();
    assert_eq!(delta["choices"][0]["delta"]["content"], "hello");

    bridge
            .push(
                b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n\n\
                 event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            )
            .unwrap();
    let out = bridge.finish().unwrap();
    assert!(out.join("").ends_with("data: [DONE]\n\n"));
}

#[test]
fn openai_stream_passes_through_records_verbatim() {
    let mut bridge = UpstreamSseBridge::for_upstream(false, "");
    let record = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n";
    let out = bridge.push(record.as_bytes()).unwrap();
    assert_eq!(out, vec![record.to_string()]);
    assert!(bridge.finish().unwrap().is_empty());
}

/// The regression that motivated the bridge: converting Anthropic SSE to
/// OpenAI SSE records and feeding them through the Responses converter must
/// produce `response.output_text.delta` events (it previously produced
/// nothing because Anthropic frames have no `choices`).
#[test]
fn converted_anthropic_records_feed_the_responses_pipeline() {
    let mut bridge = UpstreamSseBridge::for_upstream(true, "claude-3-5");
    let mut records = bridge
            .push(
                b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{}}}\n\n\
                 event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
            )
            .unwrap();
    records.extend(
            bridge
                .push(
                    b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n\n\
                     event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                )
                .unwrap(),
        );
    records.extend(bridge.finish().unwrap());

    let mut state = crate::protocol::responses::StreamState::default();
    let mut accumulated = String::new();
    let mut saw_delta = false;
    let mut saw_completed = false;
    for record in &records {
        let events = crate::protocol::responses::convert_openai_sse_to_responses(
            record,
            "claude-3-5",
            "resp_abc",
            &accumulated,
            &mut state,
        );
        for ev in &events {
            if ev.contains("response.output_text.delta") {
                saw_delta = true;
            }
        }
        // Track content from OpenAI frames for the accumulated arg.
        for line in record.lines() {
            let trimmed = line.trim();
            if let Some(d) = trimmed.strip_prefix("data:") {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(d.trim()) {
                    if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                        if let Some(delta) = choices.first().and_then(|c| c.get("delta")) {
                            if let Some(t) = delta.get("content").and_then(|c| c.as_str()) {
                                accumulated.push_str(t);
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        saw_delta,
        "converted Anthropic frames must produce output_text.delta"
    );
    assert_eq!(accumulated, "hello");
    let _ = saw_completed;
}

/// The full Codex tool-call flow end to end: an Anthropic upstream that
/// decides to call a tool emits `content_block_start`(tool_use) →
/// `input_json_delta` → `content_block_stop` → `message_delta`(tool_use).
/// These must survive the bridge AND the Responses converter, producing the
/// exact `response.function_call*` event chain Codex expects. This was
/// completely broken before the fix: the upstream never received `tools`,
/// so it emitted DSML text instead of a real `tool_use` (session 019fe025).
#[test]
fn anthropic_tool_use_becomes_responses_function_call_chain() {
    let mut bridge = UpstreamSseBridge::for_upstream(true, "claude-3-5");
    let mut records = bridge
            .push(
                b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":10}}}\n\n",
            )
            .unwrap();
    records.extend(
            bridge
                .push(
                    b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"exec_command\"}}\n\n\
                     event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\":\\\"git sta\"}}\n\n",
                )
                .unwrap(),
        );
    records.extend(
            bridge
                .push(
                    b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"tus\\\"}\"}}\n\n\
                     event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                )
                .unwrap(),
        );
    records.extend(
            bridge
                .push(
                    b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":5}}\n\n\
                     event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                )
                .unwrap(),
        );
    records.extend(bridge.finish().unwrap());

    // First: the bridge must have produced an OpenAI tool_calls record.
    let all: String = records
        .iter()
        .map(|r| r.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        all.contains("\"tool_calls\"") && all.contains("exec_command"),
        "bridge must emit an OpenAI tool_calls record, got: {all}"
    );

    // Then: feed the whole stream through the Responses converter.
    let mut state = crate::protocol::responses::StreamState::default();
    let mut accumulated = String::new();
    let mut saw_function_call_added = false;
    let mut saw_args_delta = false;
    let mut saw_args_done = false;
    let mut saw_fc_done = false;
    let mut saw_text = String::new();
    for record in &records {
        let events = crate::protocol::responses::convert_openai_sse_to_responses(
            record,
            "claude-3-5",
            "resp_abc",
            &accumulated,
            &mut state,
        );
        for ev in &events {
            if ev.contains("\"type\":\"function_call\"") && ev.contains("output_item.added") {
                saw_function_call_added = true;
            }
            if ev.contains("response.function_call_arguments.delta") {
                saw_args_delta = true;
            }
            if ev.contains("response.function_call_arguments.done") {
                saw_args_done = true;
            }
            if ev.contains("\"type\":\"function_call\"") && ev.contains("output_item.done") {
                saw_fc_done = true;
            }
        }
        // Track content from OpenAI frames for the accumulated arg.
        for line in record.lines() {
            let trimmed = line.trim();
            if let Some(d) = trimmed.strip_prefix("data:") {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(d.trim()) {
                    if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                        if let Some(delta) = choices.first().and_then(|c| c.get("delta")) {
                            if let Some(t) = delta.get("content").and_then(|c| c.as_str()) {
                                accumulated.push_str(t);
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        saw_function_call_added,
        "must emit function_call output_item.added"
    );
    assert!(saw_args_delta, "must emit function_call_arguments.delta");
    assert!(saw_args_done, "must emit function_call_arguments.done");
    assert!(saw_fc_done, "must emit function_call output_item.done");
    let _ = saw_text;
    let _ = accumulated;
}
