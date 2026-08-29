use super::super::error::{DecodeError, FeatureKind, UnsupportedFeatures};
use super::super::identity;
use super::super::ports::{DecodedResponse, NonStreamDecoder};
use super::super::report::{ConversionContext, Usage};
use super::super::types;
use serde_json::Value;

// ===========================================================================
// Non-stream response decoding: Chat Completions JSON -> Messages JSON.
// ===========================================================================
pub struct NonStreamResponseDecoder {
    context: ConversionContext,
}

impl NonStreamResponseDecoder {
    pub fn boxed(context: &ConversionContext) -> Box<dyn NonStreamDecoder + Send + Sync> {
        Box::new(NonStreamResponseDecoder {
            context: context.clone(),
        })
    }
}

impl NonStreamDecoder for NonStreamResponseDecoder {
    fn decode(&self, body: &Value) -> Result<DecodedResponse, DecodeError> {
        let usage = identity::parse_usage(types::Protocol::Chat, body);
        decode_chat_response_to_messages(body, &self.context)
            .map(|body| DecodedResponse { body, usage })
            .map_err(DecodeError::from)
    }
}

/// Decode a non-stream Chat Completions response into Messages.
///
/// This is the strict implementation extracted from
/// `protocol::anthropic::openai_to_anthropic`; its rejection policy is
/// unchanged (invalid arguments fail, unknown finish reasons never downgrade).
pub fn decode_chat_response_to_messages(
    body: &Value,
    context: &ConversionContext,
) -> Result<Value, UnsupportedFeatures> {
    let choice = body.pointer("/choices/0").ok_or_else(|| {
        UnsupportedFeatures::single(
            FeatureKind::UnknownEvent,
            "/choices/0",
            "Chat response missing choices[0]",
        )
    })?;
    let message = choice.get("message").ok_or_else(|| {
        UnsupportedFeatures::single(
            FeatureKind::UnknownEvent,
            "/choices/0/message",
            "Chat response missing message",
        )
    })?;

    // Reasoning content from an OpenAI upstream is carried into Messages as a
    // `thinking` block (fail-open, CPA semantics: it is always preserved even
    // when `content` is non-empty).  `reasoning_content` may be a plain string
    // or an object `{"text": ...}`; `redacted_thinking` has no readable text.
    let reasoning_text = extract_reasoning_text(message);

    let content_text = match message.get("content") {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => {
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownBlock,
                "/choices/0/message/content",
                "Chat response has unsupported non-text message content",
            ))
        }
    };

    let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
    let has_tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|c| !c.is_empty());

    // Unknown finish reason must never become end_turn/stop.
    let stop_reason = match finish_reason {
        Some("stop") => "end_turn",
        Some("length") => "max_tokens",
        Some("tool_calls") | Some("function_call") => "tool_use",
        Some("content_filter") | Some("refusal") => "refusal",
        Some(other) => {
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownFinishReason,
                "/choices/0/finish_reason",
                format!("unknown Chat finish_reason {other:?}"),
            ))
        }
        None => {
            // No finish_reason: only safe to call it tool_use when there are
            // tool calls, otherwise the completion is incomplete.
            if has_tool_calls {
                "tool_use"
            } else {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownFinishReason,
                    "/choices/0/finish_reason",
                    "Chat response has no finish_reason and no tool_calls",
                ));
            }
        }
    };

    let usage = usage_from_chat(body);

    let mut content_blocks: Vec<Value> = Vec::new();
    // Thinking block precedes the text block, mirroring the assistant message
    // shape Claude produces natively.
    if !reasoning_text.is_empty() {
        content_blocks.push(serde_json::json!({"type": "thinking", "thinking": reasoning_text}));
    }
    if !content_text.is_empty() {
        content_blocks.push(serde_json::json!({"type": "text", "text": content_text}));
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for (i, tc) in tool_calls.iter().enumerate() {
            let cp = format!("/choices/0/message/tool_calls/{i}");
            let id = tc
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    UnsupportedFeatures::single(
                        FeatureKind::MissingToolField,
                        format!("{cp}/id"),
                        "Chat response tool call missing id",
                    )
                })?;
            let name = tc
                .pointer("/function/name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    UnsupportedFeatures::single(
                        FeatureKind::MissingToolField,
                        format!("{cp}/function/name"),
                        "Chat response tool call missing function.name",
                    )
                })?;
            let args_str = tc
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    UnsupportedFeatures::single(
                        FeatureKind::InvalidToolArguments,
                        format!("{cp}/function/arguments"),
                        "Chat response tool call missing function.arguments",
                    )
                })?;
            let input: Value = serde_json::from_str(args_str).map_err(|e| {
                UnsupportedFeatures::single(
                    FeatureKind::InvalidToolArguments,
                    format!("{cp}/function/arguments"),
                    format!("Chat response tool arguments are not valid JSON: {e}"),
                )
            })?;
            if !input.is_object() {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::InvalidToolArguments,
                    format!("{cp}/function/arguments"),
                    "Chat response tool arguments must decode to a JSON object",
                ));
            }
            content_blocks.push(serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }));
        }
    }
    if content_blocks.is_empty() {
        content_blocks.push(serde_json::json!({"type": "text", "text": ""}));
    }

    Ok(serde_json::json!({
        "id": body.get("id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("msg_{}", uuid::Uuid::new_v4().simple())),
        "type": "message",
        "role": "assistant",
        "model": context.upstream_model,
        "content": content_blocks,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
            "cache_creation_input_tokens": usage.cache_creation_input_tokens,
            "cache_read_input_tokens": usage.cache_read_input_tokens,
        }
    }))
}

/// Extract reasoning text from a Chat assistant message, supporting both the
/// plain-string and `{"text": ...}` shapes of `reasoning_content`, plus the
/// `thinking`/`reasoning` aliases some providers use.  Returns `""` when no
/// reasoning is present.
fn extract_reasoning_text(message: &Value) -> String {
    let candidate = message
        .get("reasoning_content")
        .or_else(|| message.get("thinking"))
        .or_else(|| message.get("reasoning"));
    let Some(c) = candidate else {
        return String::new();
    };
    match c {
        Value::String(s) => s.clone(),
        Value::Object(o) => o
            .get("text")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Extract real usage from a Chat response.  `usage_unknown` is surfaced to the
/// gateway via the report; a 0 is only ever a protocol-mandated placeholder.
pub fn usage_from_chat(body: &Value) -> Usage {
    let prompt = body.pointer("/usage/prompt_tokens").and_then(Value::as_u64);
    let completion = body
        .pointer("/usage/completion_tokens")
        .and_then(Value::as_u64);
    let cache_read = body
        .pointer("/usage/prompt_tokens_details/cached_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            body.pointer("/usage/cache_read_input_tokens")
                .and_then(Value::as_u64)
        })
        .unwrap_or(0);
    let cache_creation = body
        .pointer("/usage/cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Usage {
        input_tokens: prompt.unwrap_or(0),
        output_tokens: completion.unwrap_or(0),
        cache_creation_input_tokens: cache_creation,
        cache_read_input_tokens: cache_read,
        usage_unknown: prompt.is_none() || completion.is_none(),
    }
}
