use super::accumulator::ResponsesEventAccumulator;
use super::decode::{
    decode_responses_response_to_chat, MessagesResponsesNonStreamDecoder,
    ResponsesMessagesNonStreamDecoder,
};
use super::encode_chat::encode_chat_to_responses;
use super::encode_messages::{encode_messages_to_responses, encode_responses_to_messages};
use super::state::{responses_response_id, ResponsesChatState};
use super::stream::{
    ChatToResponsesStreamDecoder, MessagesResponsesStreamDecoder, ResponsesMessagesStreamDecoder,
    ResponsesStreamDecoder,
};
use crate::protocol::codec::ports::{NonStreamDecoder, StreamDecoder};
use crate::protocol::codec::report::ConversionContext;

#[test]
fn chat_request_encodes_function_call_and_text() {
    let request = serde_json::json!({"model":"ignored","messages":[{"role":"user","content":"hi"},{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"weather","arguments":"{}"}}]}],"tools":[{"type":"function","function":{"name":"weather","parameters":{"type":"object"}}}]});
    let (encoded, _) = encode_chat_to_responses(&request, "gpt-test").unwrap();
    assert_eq!(encoded["model"], "gpt-test");
    assert_eq!(encoded["input"][1]["type"], "function_call");
}

#[test]
fn chat_request_does_not_synthesize_unsupported_max_output_tokens() {
    let request = serde_json::json!({
        "model": "ignored",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 32000
    });
    let (encoded, context) = encode_chat_to_responses(&request, "gpt-test").unwrap();
    assert!(encoded.get("max_output_tokens").is_none());
    assert!(context.normalized.contains(&"/max_tokens".to_string()));
}

#[test]
fn responses_stream_terminal_usage_once_and_any_split() {
    let events = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"m\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}}\n\n"
    );
    let context = ConversionContext::new("chatcmpl_1", "m", true);
    let mut expected = None;
    for split in 0..=events.len() {
        let mut decoder = ResponsesStreamDecoder {
            state: ResponsesChatState::new(&context),
        };
        let mut actual = decoder.feed(&events.as_bytes()[..split]).unwrap();
        actual.extend(decoder.feed(&events.as_bytes()[split..]).unwrap());
        actual.extend(decoder.finish().unwrap());
        let joined = actual.concat();
        assert_eq!(joined.matches("[DONE]").count(), 1);
        assert_eq!(joined.matches("\"usage\"").count(), 1);
        if let Some(value) = &expected {
            assert_eq!(&joined, value);
        } else {
            expected = Some(joined);
        }
    }
}
#[test]
fn rate_limits_record_is_unchanged() {
    let record = "event: codex.rate_limits\ndata: {\"type\":\"codex.rate_limits\",\"x\":1}\n\n";
    let context = ConversionContext::new("x", "m", true);
    let mut decoder = ResponsesStreamDecoder {
        state: ResponsesChatState::new(&context),
    };
    assert_eq!(
        decoder.feed(record.as_bytes()).unwrap(),
        vec![record.to_string()]
    );
}

#[test]
fn responses_response_id_falls_back_to_uuid_when_empty() {
    // The streaming V5 path passes "" as the request id (driver.rs); this
    // must not degenerate every stream to the same "resp_" / "msg_" / "rs_"
    // ids.  Fall back to a fresh uuid so ids are unique and non-degenerate.
    let id = responses_response_id("");
    assert!(id.starts_with("resp_"), "unexpected id {id:?}");
    assert!(id.len() > "resp_".len(), "degenerate id {id:?}");
    assert_ne!(id, responses_response_id(""), "ids must be unique per call");
    // A stamped request id keeps its existing behavior.
    assert_eq!(responses_response_id("chatcmpl_abc"), "resp_abc");
    assert_eq!(responses_response_id("other"), "resp_other");
}

#[test]
fn accumulator_requires_completed_and_returns_final_response() {
    let mut accumulator = ResponsesEventAccumulator::default();
    accumulator
        .push(b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"output\":[]}}\n\n")
        .unwrap();
    assert_eq!(accumulator.finish().unwrap()["id"], "resp_1");
    assert!(ResponsesEventAccumulator::default().finish().is_err());
}

#[test]
fn accumulator_backfills_empty_completed_output_from_item_done() {
    let mut accumulator = ResponsesEventAccumulator::default();
    accumulator
        .push(br#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}}

data: {"type":"response.completed","response":{"id":"resp_1","model":"m","output":[],"usage":{"input_tokens":1,"output_tokens":1}}}

"#)
        .unwrap();
    let completed = accumulator.finish().unwrap();
    assert_eq!(completed["output"][0]["content"][0]["text"], "hello");
}

#[test]
fn non_stream_decoders_cover_responses_chat_and_messages() {
    let completed = serde_json::json!({
        "id":"resp_1", "model":"m", "status":"completed", "output":[
            {"type":"reasoning", "summary":[{"type":"summary_text", "text":"think"}]},
            {"type":"message", "content":[{"type":"output_text", "text":"answer"}]},
            {"type":"function_call", "call_id":"call_1", "name":"weather", "arguments":"{}"}
        ], "usage":{"input_tokens":2,"output_tokens":3}
    });
    let context = ConversionContext::new("chatcmpl_1", "m", false);
    let chat = decode_responses_response_to_chat(&completed, &context).unwrap();
    assert_eq!(chat["usage"]["total_tokens"], 5);
    assert_eq!(chat["choices"][0]["finish_reason"], "tool_calls");
    let messages = ResponsesMessagesNonStreamDecoder { context }
        .decode(&completed)
        .unwrap();
    assert_eq!(messages["type"], "message");
}

#[test]
fn non_stream_responses_incomplete_never_becomes_stop() {
    let incomplete = serde_json::json!({
        "id":"resp_1",
        "model":"m",
        "status":"incomplete",
        "incomplete_details":{"reason":"max_output_tokens"},
        "output":[{"type":"message", "content":[{"type":"output_text", "text":"partial"}]}],
        "usage":{"input_tokens":2,"output_tokens":3}
    });
    let context = ConversionContext::new("chatcmpl_1", "m", false);
    let chat = decode_responses_response_to_chat(&incomplete, &context).unwrap();
    assert_eq!(chat["choices"][0]["finish_reason"], "length");
    let messages = ResponsesMessagesNonStreamDecoder { context }
        .decode(&incomplete)
        .unwrap();
    assert_eq!(messages["stop_reason"], "max_tokens");
}

#[test]
fn non_stream_responses_failed_is_rejected() {
    let failed = serde_json::json!({
        "id":"resp_1",
        "model":"m",
        "status":"failed",
        "output":[],
        "usage":{"input_tokens":1,"output_tokens":0}
    });
    let context = ConversionContext::new("chatcmpl_1", "m", false);
    let error = decode_responses_response_to_chat(&failed, &context).unwrap_err();
    assert!(error.json_pointers.contains(&"/status".to_string()));
}

#[test]
fn stream_rejects_failed_and_missing_terminal() {
    let context = ConversionContext::new("chatcmpl_1", "m", true);
    let mut failed = ResponsesStreamDecoder {
        state: ResponsesChatState::new(&context),
    };
    assert!(failed
        .feed(b"data: {\"type\":\"response.failed\"}\n\n")
        .is_err());
    let mut incomplete = ResponsesStreamDecoder {
        state: ResponsesChatState::new(&context),
    };
    incomplete
        .feed(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n")
        .unwrap();
    assert!(incomplete.finish().is_err());
}

#[test]
fn stream_function_call_arguments_done_without_delta_emits_executable_tool_call() {
    let context = ConversionContext::new("chatcmpl_1", "m", true);
    let mut decoder = ResponsesStreamDecoder {
        state: ResponsesChatState::new(&context),
    };
    let mut output = decoder
        .feed(
            br#"data: {"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","arguments":"{\"city\":\"Shanghai\"}"}

data: {"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_1","name":"weather"}}

data: {"type":"response.completed","response":{"usage":{"input_tokens":1,"output_tokens":1}}}

"#,
        )
        .unwrap();
    output.extend(decoder.finish().unwrap());
    let output = output.concat();
    assert!(output.contains(r#""id":"call_1"#));
    assert!(output.contains(r#""name":"weather"#));
    assert!(output.contains("Shanghai"));
    assert!(output.contains(r#""finish_reason":"tool_calls"#));
}

#[test]
fn stream_function_call_delta_and_done_complete_without_duplicate_arguments() {
    let context = ConversionContext::new("chatcmpl_1", "m", true);
    let mut decoder = ResponsesStreamDecoder {
        state: ResponsesChatState::new(&context),
    };
    let output = decoder
        .feed(
            br#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_1","name":"weather"}}

data: {"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","delta":"{\"city\":"}

data: {"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","arguments":"{\"city\":\"Shanghai\"}"}

data: {"type":"response.completed","response":{"usage":{"input_tokens":1,"output_tokens":1}}}

"#,
        )
        .unwrap()
        .concat();
    assert_eq!(output.matches("Shanghai").count(), 1);
    assert!(output.contains(r#""finish_reason":"tool_calls"#));
}

#[test]
fn stream_rejects_invalid_done_function_call_arguments() {
    let context = ConversionContext::new("chatcmpl_1", "m", true);
    let mut decoder = ResponsesStreamDecoder {
        state: ResponsesChatState::new(&context),
    };
    assert!(decoder
        .feed(
            br#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_1","name":"weather"}}

data: {"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","arguments":"not-json"}

"#,
        )
        .is_err());
}

#[test]
fn messages_stream_composes_done_only_function_call() {
    let context = ConversionContext::new("msg_1", "m", true);
    let mut decoder = ResponsesMessagesStreamDecoder {
        chat: ResponsesStreamDecoder {
            state: ResponsesChatState::new(&context),
        },
        messages: crate::protocol::codec::chat::ChatStreamDecoder::boxed(&context),
    };
    let mut output = decoder
        .feed(
            br#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_1","name":"weather","arguments":"{\"city\":\"Shanghai\"}"}}

data: {"type":"response.completed","response":{"usage":{"input_tokens":1,"output_tokens":1}}}

"#,
        )
        .unwrap();
    output.extend(decoder.finish().unwrap());
    let output = output.concat();
    assert!(output.contains("content_block_start"));
    assert!(output.contains("tool_use"));
    assert!(output.contains("Shanghai"));
}

#[test]
fn unsupported_chat_field_fails_before_encoding() {
    let error = encode_chat_to_responses(
        &serde_json::json!({"model":"m", "messages":[], "unknown":true}),
        "m",
    )
    .unwrap_err();
    assert!(error.json_pointers.contains(&"/unknown".to_string()));
}

#[test]
fn chat_metadata_is_dropped_for_responses_backend() {
    let (encoded, context) = encode_chat_to_responses(
        &serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "metadata": {"user_id": "u1"}
        }),
        "m",
    )
    .unwrap();
    assert!(encoded.get("metadata").is_none());
    assert!(context.normalized.contains(&"/metadata".to_string()));
}

#[test]
fn chat_request_reasoning_content_becomes_responses_reasoning_item() {
    let (encoded, _) = encode_chat_to_responses(
        &serde_json::json!({
            "model": "m",
            "messages": [
                {"role": "assistant", "reasoning_content": "think", "content": "answer"},
                {"role": "user", "content": "continue"}
            ]
        }),
        "m",
    )
    .unwrap();
    assert_eq!(encoded["input"][0]["type"], "reasoning");
    assert_eq!(encoded["input"][0]["summary"][0]["text"], "think");
    assert_eq!(encoded["input"][1]["type"], "message");
    assert_eq!(encoded["input"][1]["role"], "assistant");
    assert_eq!(encoded["input"][1]["content"][0]["text"], "answer");
}

#[test]
fn messages_request_thinking_survives_messages_to_responses() {
    let (encoded, _) = encode_messages_to_responses(
        &serde_json::json!({
            "model": "m",
            "stream": true,
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "chain"},
                    {"type": "text", "text": "answer"}
                ]},
                {"role": "user", "content": "continue"}
            ]
        }),
        "m",
    )
    .unwrap();
    assert_eq!(encoded["input"][0]["type"], "reasoning");
    assert_eq!(encoded["input"][0]["summary"][0]["text"], "chain");
    assert_eq!(encoded["input"][1]["type"], "message");
    assert_eq!(encoded["input"][1]["role"], "assistant");
}

#[test]
fn chat_gpt5_options_map_or_drop_for_responses_backend() {
    let (encoded, context) = encode_chat_to_responses(
        &serde_json::json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 32000,
            "stream": true,
            "stream_options": {"include_usage": true},
            "reasoning_effort": "HIGH",
            "verbosity": "LOW"
        }),
        "gpt-5.5",
    )
    .unwrap();

    assert_eq!(encoded["model"], "gpt-5.5");
    assert!(encoded.get("reasoning").is_none());
    assert!(encoded.get("text").is_none());
    assert!(encoded.get("max_output_tokens").is_none());
    assert!(encoded.get("stream_options").is_none());
    assert!(context.normalized.contains(&"/max_tokens".to_string()));
    assert!(context.normalized.contains(&"/stream_options".to_string()));
    assert!(context
        .normalized
        .contains(&"/reasoning_effort".to_string()));
    assert!(context.normalized.contains(&"/verbosity".to_string()));
}

#[test]
fn encode_responses_to_messages_maps_codex_request() {
    // Real codex 0.147.0 request shape (§1.1): instructions + input +
    // tools + tool_choice + parallel_tool_calls + reasoning:{effort:high}
    // + store + stream + include + prompt_cache_key + client_metadata.
    let request = serde_json::json!({
        "model": "deepseek-v4-flash-free",
        "instructions": "You are a helpful assistant.",
        "input": [
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "hello"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "list files"}]}
        ],
        "tools": [
            {"type": "function", "name": "list", "description": "list files", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}}
        ],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "reasoning": {"effort": "high"},
        "store": true,
        "stream": true,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": "cache-key",
        "client_metadata": {"turn": "1"}
    });
    let (encoded, context) =
        encode_responses_to_messages(&request, "oc/deepseek-v4-flash-free").unwrap();

    assert_eq!(encoded["model"], "oc/deepseek-v4-flash-free");
    assert_eq!(encoded["stream"], true);
    // instructions -> top-level system text block
    assert_eq!(encoded["system"][0]["type"], "text");
    assert_eq!(encoded["system"][0]["text"], "You are a helpful assistant.");
    // input -> messages
    assert_eq!(encoded["messages"].as_array().unwrap().len(), 3);
    assert_eq!(encoded["messages"][0]["role"], "user");
    assert_eq!(encoded["messages"][0]["content"][0]["text"], "hi");
    // reasoning.effort=high -> adaptive thinking + output_config.effort=high
    assert_eq!(encoded["thinking"], serde_json::json!({"type": "adaptive"}));
    assert_eq!(encoded["output_config"]["effort"], "high");
    // no max_output_tokens carried -> V5 default cap
    assert_eq!(encoded["max_tokens"], 32000);
    // tools / tool_choice
    assert_eq!(encoded["tools"][0]["name"], "list");
    assert_eq!(
        encoded["tools"][0]["input_schema"]["properties"]["path"]["type"],
        "string"
    );
    assert_eq!(encoded["tool_choice"], "auto");
    // codex-only dropped fields are recorded in the ConversionReport
    for pointer in [
        "/parallel_tool_calls",
        "/store",
        "/include",
        "/prompt_cache_key",
        "/client_metadata",
    ] {
        assert!(
            context.normalized.contains(&pointer.to_string()),
            "missing dropped-field record {pointer}"
        );
    }
}

#[test]
fn encode_responses_to_messages_respects_max_output_tokens() {
    let request = serde_json::json!({
        "model": "m",
        "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
        "max_output_tokens": 2048
    });
    let (encoded, context) = encode_responses_to_messages(&request, "oc/model").unwrap();
    // Caller-supplied cap wins; the V5 32000 default must not apply.
    assert_eq!(encoded["max_tokens"], 2048);
    // No field from the dropped set present -> nothing recorded.
    assert!(context.normalized.is_empty());
}

// ------------------------------------------------------------------
// 路径① response direction: Messages → Responses.
// ------------------------------------------------------------------

/// Extract the ordered Responses event-type sequence from a joined stream:
/// `event:` field names plus the standalone `data: [DONE]` terminator.
fn responses_event_types(joined: &str) -> Vec<String> {
    let mut types = Vec::new();
    for line in joined.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("event:") {
            types.push(name.trim().to_string());
        } else if trimmed == "data: [DONE]" {
            types.push("[DONE]".to_string());
        }
    }
    types
}

/// A representative 9router Messages SSE stream: text + tool_use, ending in
/// tool_use with usage.  Covers message_start / content_block_start /
/// content_block_delta / content_block_stop / message_delta / message_stop / ping.
fn messages_responses_fixture_sse() -> &'static str {
    concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"model\":\"oc/m\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"weather\",\"input\":{}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\\\"Shanghai\\\"}\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"input_tokens\":5,\"output_tokens\":3}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        "event: ping\ndata: {\"type\":\"ping\"}\n\n",
    )
}

#[test]
fn messages_responses_non_stream_decodes_to_responses() {
    let messages = serde_json::json!({
        "id": "msg_01",
        "type": "message",
        "role": "assistant",
        "model": "oc/m",
        "content": [
            {"type": "text", "text": "Hello there"},
            {"type": "tool_use", "id": "toolu_1", "name": "weather", "input": {"city": "Shanghai"}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 5, "output_tokens": 3}
    });
    let context = ConversionContext::new("chatcmpl_1", "oc/m", false);
    let decoded = MessagesResponsesNonStreamDecoder { context }
        .decode(&messages)
        .unwrap();
    assert_eq!(decoded["object"], "response");
    assert_eq!(decoded["model"], "oc/m");
    assert_eq!(decoded["status"], "completed");
    assert_eq!(decoded["finish_reason"], "tool_calls");
    assert_eq!(decoded["usage"]["input_tokens"], 5);
    assert_eq!(decoded["usage"]["output_tokens"], 3);
    assert_eq!(decoded["usage"]["total_tokens"], 8);
    let output = decoded["output"].as_array().unwrap();
    assert!(
        output.iter().any(|i| i["type"] == "function_call"
            && i["name"] == "weather"
            && i["call_id"] == "toolu_1"),
        "function_call output item missing"
    );
    let text_item = output.iter().find(|i| i["type"] == "message").unwrap();
    assert_eq!(text_item["content"][0]["text"], "Hello there");
}

#[test]
fn messages_responses_stream_emits_full_event_sequence() {
    let context = ConversionContext::new("chatcmpl_1", "oc/m", true);
    let mut decoder = MessagesResponsesStreamDecoder::boxed(&context);
    let mut events = decoder
        .feed(messages_responses_fixture_sse().as_bytes())
        .unwrap();
    events.extend(decoder.finish().unwrap());
    let joined = events.concat();

    // The brief-required subsequence, in order: response.created /
    // response.output_item.added / response.output_text.delta /
    // response.function_call_arguments.delta / response.completed / [DONE].
    let types = responses_event_types(&joined);
    for required in [
        "response.created",
        "response.output_item.added",
        "response.output_text.delta",
        "response.function_call_arguments.delta",
        "response.completed",
        "[DONE]",
    ] {
        assert!(
            types.contains(&required.to_string()),
            "missing event {required:?} in {types:?}"
        );
    }
    let positions: Vec<usize> = [
        "response.created",
        "response.output_item.added",
        "response.output_text.delta",
        "response.function_call_arguments.delta",
        "response.completed",
        "[DONE]",
    ]
    .iter()
    .map(|t| types.iter().position(|x| x == t).unwrap())
    .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "events out of order: {types:?}"
    );

    // Content and tool arguments survive the whole chain.
    assert!(joined.contains("Hello"));
    assert!(joined.contains("Shanghai"));
    // Usage reaches response.completed.
    let completed = events
        .iter()
        .find(|e| e.contains("event: response.completed"))
        .unwrap();
    assert!(completed.contains(r#""input_tokens":5"#));
    assert!(completed.contains(r#""output_tokens":3"#));
}

#[test]
fn messages_responses_stream_is_deterministic_across_any_split() {
    let sse = messages_responses_fixture_sse();
    let context = ConversionContext::new("chatcmpl_1", "oc/m", true);
    let mut expected: Option<Vec<String>> = None;
    for split in 0..=sse.len() {
        let mut decoder = MessagesResponsesStreamDecoder::boxed(&context);
        let mut events = decoder.feed(&sse.as_bytes()[..split]).unwrap();
        events.extend(decoder.feed(&sse.as_bytes()[split..]).unwrap());
        events.extend(decoder.finish().unwrap());
        let joined = events.concat();
        assert_eq!(joined.matches("data: [DONE]").count(), 1);
        assert_eq!(joined.matches("event: response.completed").count(), 1);
        let types = responses_event_types(&joined);
        if let Some(previous) = &expected {
            assert_eq!(&types, previous, "split at byte {split} diverged");
        } else {
            expected = Some(types);
        }
    }
}

#[test]
fn chat_to_responses_stream_passes_through_rate_limits() {
    let context = ConversionContext::new("chatcmpl_1", "oc/m", true);
    let mut decoder = ChatToResponsesStreamDecoder::new(&context);
    let record = "event: codex.rate_limits\ndata: {\"type\":\"codex.rate_limits\",\"x\":1}\n\n";
    let events = decoder.feed(record.as_bytes()).unwrap();
    assert_eq!(events.len(), 2, "created preamble + rate_limits record");
    assert_eq!(events[1], record);
}

#[test]
fn chat_to_responses_stream_rejects_incomplete_finish() {
    let context = ConversionContext::new("chatcmpl_1", "oc/m", true);

    // Mid-record EOF fails closed.
    let mut mid_record = ChatToResponsesStreamDecoder::new(&context);
    mid_record.feed(b"data: {\"partial").unwrap();
    assert!(mid_record.finish().is_err());

    // A well-formed Chat stream that never delivered finish_reason is
    // incomplete and must fail for pre-commit failover.
    let mut truncated = ChatToResponsesStreamDecoder::new(&context);
    truncated
        .feed(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n")
        .unwrap();
    assert!(truncated.finish().is_err());
}
