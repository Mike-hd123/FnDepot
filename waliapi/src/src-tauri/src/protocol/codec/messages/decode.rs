use super::super::error::{DecodeError, FeatureKind, UnsupportedFeatures};
use super::super::identity;
use super::super::ports::{DecodedResponse, NonStreamDecoder};
use super::super::report::{ConversionContext, Usage};
use super::super::types;
use serde_json::Value;

// ===========================================================================
// Non-stream response decoding: Messages JSON -> Chat Completions JSON.
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
        let usage = identity::parse_usage(types::Protocol::Messages, body);
        decode_messages_response_to_chat(body, &self.context)
            .map(|body| DecodedResponse { body, usage })
            .map_err(DecodeError::from)
    }
}

/// Decode a non-stream Anthropic Messages response into Chat Completions.
pub fn decode_messages_response_to_chat(
    body: &Value,
    context: &ConversionContext,
) -> Result<Value, UnsupportedFeatures> {
    if body.get("type").and_then(Value::as_str) != Some("message") {
        return Err(UnsupportedFeatures::single(
            FeatureKind::UnknownEvent,
            "/type",
            "Messages response must have type=message",
        ));
    }
    let content = body
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/content",
                "Messages response missing content array",
            )
        })?;

    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for (i, block) in content.iter().enumerate() {
        let bp = format!("/content/{i}");
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                text.push_str(block.get("text").and_then(Value::as_str).unwrap_or(""));
            }
            Some("tool_use") => {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        UnsupportedFeatures::single(
                            FeatureKind::MissingToolField,
                            format!("{bp}/id"),
                            "tool_use block missing id",
                        )
                    })?;
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        UnsupportedFeatures::single(
                            FeatureKind::MissingToolField,
                            format!("{bp}/name"),
                            "tool_use block missing name",
                        )
                    })?;
                // A tool_use without `input` is malformed: never fabricate `{}`.
                let input = block.get("input").ok_or_else(|| {
                    UnsupportedFeatures::single(
                        FeatureKind::MissingToolField,
                        format!("{bp}/input"),
                        "tool_use block missing input",
                    )
                })?;
                if !input.is_object() {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::InvalidToolArguments,
                        format!("{bp}/input"),
                        "tool_use input must be a JSON object",
                    ));
                }
                let input = input.clone();
                let arguments = serde_json::to_string(&input).map_err(|e| {
                    UnsupportedFeatures::single(
                        FeatureKind::InvalidToolArguments,
                        format!("{bp}/input"),
                        format!("tool_use input could not be serialized: {e}"),
                    )
                })?;
                tool_calls.push(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {"name": name, "arguments": arguments}
                }));
            }
            Some("thinking") => {
                // Fail-open: reasoning is surfaced as OpenAI `reasoning_content`
                // (string), never rejected.  Only the visible text is kept; the
                // signature/encrypted forms are dropped.
                if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                    reasoning.push_str(t);
                }
            }
            Some("redacted_thinking") => {
                // No usable text; skip.
            }
            Some(other) => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownBlock,
                    format!("{bp}/type"),
                    format!("unsupported Messages response block type {other:?}"),
                ))
            }
            None => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownBlock,
                    format!("{bp}/type"),
                    "response content block missing type",
                ))
            }
        }
    }

    let stop_reason = body.get("stop_reason").and_then(Value::as_str);
    let finish_reason = match stop_reason {
        // `refusal`, `stop_sequence`, `pause_turn` all mean the model simply
        // stopped producing output (refusal text is carried in the content),
        // so map them to the OpenAI-canonical `stop` instead of erroring.
        Some("end_turn") | Some("refusal") | Some("stop_sequence") | Some("pause_turn") => "stop",
        // Context-window exhaustion is a truncation, exactly like `max_tokens`.
        Some("max_tokens") | Some("model_context_window_exceeded") => "length",
        Some("tool_use") => "tool_calls",
        Some(other) => {
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownFinishReason,
                "/stop_reason",
                format!("unknown Messages stop_reason {other:?}"),
            ))
        }
        None => {
            // Missing stop_reason: only safe when tool_use was emitted.
            if !tool_calls.is_empty() {
                "tool_calls"
            } else {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownFinishReason,
                    "/stop_reason",
                    "Messages response missing stop_reason",
                ));
            }
        }
    };

    let usage = usage_from_messages(body);
    let response_id = body
        .get("id")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| format!("chatcmpl_{}", uuid::Uuid::new_v4().simple()));

    let mut message = serde_json::json!({
        "role": "assistant",
        "content": if text.is_empty() { Value::Null } else { Value::String(text) },
    });
    if !reasoning.is_empty() {
        message["reasoning_content"] = Value::String(reasoning);
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }

    Ok(serde_json::json!({
        "id": response_id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": context.upstream_model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": usage.input_tokens,
            "completion_tokens": usage.output_tokens,
            "total_tokens": usage.input_tokens + usage.output_tokens,
            "prompt_tokens_details": {
                "cached_tokens": usage.cache_read_input_tokens
            },
            "cache_creation_input_tokens": usage.cache_creation_input_tokens,
            "cache_read_input_tokens": usage.cache_read_input_tokens,
        }
    }))
}

/// Extract real usage from a Messages response.  Cache tokens are surfaced in
/// OpenAI `usage` details without double-counting into input_tokens.
pub fn usage_from_messages(body: &Value) -> Usage {
    let input = body.pointer("/usage/input_tokens").and_then(Value::as_u64);
    let output = body.pointer("/usage/output_tokens").and_then(Value::as_u64);
    let cache_creation = body
        .pointer("/usage/cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = body
        .pointer("/usage/cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Usage {
        input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        cache_creation_input_tokens: cache_creation,
        cache_read_input_tokens: cache_read,
        usage_unknown: input.is_none() || output.is_none(),
    }
}
