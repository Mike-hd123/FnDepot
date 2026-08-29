use super::super::error::{DecodeError, FeatureKind, UnsupportedFeatures};
use super::super::ports::{DecodedResponse, NonStreamDecoder};
use super::super::report::{ConversionContext, Usage};
use super::super::{chat, identity, messages, types};
use serde_json::Value;

/// Convert a completed Responses object to a non-stream Chat completion.
pub fn decode_responses_response_to_chat(
    body: &Value,
    context: &ConversionContext,
) -> Result<Value, UnsupportedFeatures> {
    let response = body
        .get("response")
        .filter(|value| value.is_object())
        .unwrap_or(body);
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/output",
                "Responses response requires output array",
            )
        })?;
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for (index, item) in output.iter().enumerate() {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                if let Some(content) = item.get("content").and_then(Value::as_array) {
                    for part in content {
                        if matches!(
                            part.get("type").and_then(Value::as_str),
                            Some("output_text") | Some("text")
                        ) {
                            text.push_str(part.get("text").and_then(Value::as_str).unwrap_or(""));
                        }
                    }
                }
            }
            Some("function_call") => {
                let call_id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        UnsupportedFeatures::single(
                            FeatureKind::MissingToolField,
                            format!("/output/{index}/call_id"),
                            "function call requires call_id",
                        )
                    })?;
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        UnsupportedFeatures::single(
                            FeatureKind::MissingToolField,
                            format!("/output/{index}/name"),
                            "function call requires name",
                        )
                    })?;
                let arguments = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        UnsupportedFeatures::single(
                            FeatureKind::InvalidToolArguments,
                            format!("/output/{index}/arguments"),
                            "function call requires arguments",
                        )
                    })?;
                if !serde_json::from_str::<Value>(arguments).is_ok_and(|parsed| parsed.is_object())
                {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::InvalidToolArguments,
                        format!("/output/{index}/arguments"),
                        "function call arguments must be a valid JSON object",
                    ));
                }
                calls.push(serde_json::json!({"id":call_id,"type":"function","function":{"name":name,"arguments":arguments}}));
            }
            Some("reasoning") => {
                if let Some(summary) = item.get("summary").and_then(Value::as_array) {
                    for part in summary {
                        reasoning.push_str(part.get("text").and_then(Value::as_str).unwrap_or(""));
                    }
                }
            }
            Some(_) => {}
            None => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownEvent,
                    format!("/output/{index}/type"),
                    "output item missing type",
                ))
            }
        }
    }
    let usage = usage_from_responses(response);
    let mut message = serde_json::json!({"role":"assistant","content": if text.is_empty() { Value::Null } else { Value::String(text) }});
    if !reasoning.is_empty() {
        message["reasoning_content"] = Value::String(reasoning);
    }
    let has_tool_calls = !calls.is_empty();
    if !calls.is_empty() {
        message["tool_calls"] = Value::Array(calls);
    }
    let finish_reason = responses_finish_reason(response, has_tool_calls)?;
    Ok(serde_json::json!({
        "id": response.get("id").and_then(Value::as_str).unwrap_or(&context.request_id),
        "object":"chat.completion", "created":response.get("created_at").and_then(Value::as_i64).unwrap_or_else(|| chrono::Utc::now().timestamp()), "model":response.get("model").and_then(Value::as_str).unwrap_or(&context.upstream_model),
        "choices":[{"index":0,"message":message,"finish_reason":finish_reason}],
        "usage":{"prompt_tokens":usage.input_tokens,"completion_tokens":usage.output_tokens,"total_tokens":usage.input_tokens+usage.output_tokens}
    }))
}

fn responses_finish_reason(
    response: &Value,
    has_tool_calls: bool,
) -> Result<&'static str, UnsupportedFeatures> {
    match response.get("status").and_then(Value::as_str) {
        Some("completed") | None => Ok(if has_tool_calls { "tool_calls" } else { "stop" }),
        Some("incomplete") => match response
            .pointer("/incomplete_details/reason")
            .and_then(Value::as_str)
        {
            Some("max_output_tokens") | Some("max_tokens") | None => Ok("length"),
            Some("content_filter") | Some("safety") => Ok("content_filter"),
            Some(other) => Err(UnsupportedFeatures::single(
                FeatureKind::UnknownFinishReason,
                "/incomplete_details/reason",
                format!("unknown Responses incomplete reason {other:?}"),
            )),
        },
        Some("failed") => Err(UnsupportedFeatures::single(
            FeatureKind::UnknownEvent,
            "/status",
            "Responses response status is failed",
        )),
        Some(other) => Err(UnsupportedFeatures::single(
            FeatureKind::UnknownEvent,
            "/status",
            format!("Responses response has unsupported status {other:?}"),
        )),
    }
}

pub fn usage_from_responses(response: &Value) -> Usage {
    let input = response
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64);
    let output = response
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64);
    Usage {
        input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        usage_unknown: input.is_none() || output.is_none(),
        ..Usage::default()
    }
}

pub struct ResponsesNonStreamDecoder {
    context: ConversionContext,
}
impl ResponsesNonStreamDecoder {
    pub fn boxed(context: &ConversionContext) -> Box<dyn NonStreamDecoder + Send + Sync> {
        Box::new(Self {
            context: context.clone(),
        })
    }
}
impl NonStreamDecoder for ResponsesNonStreamDecoder {
    fn decode(&self, body: &Value) -> Result<DecodedResponse, DecodeError> {
        let usage = identity::parse_usage(types::Protocol::Responses, body);
        decode_responses_response_to_chat(body, &self.context)
            .map(|body| DecodedResponse { body, usage })
            .map_err(DecodeError::from)
    }
}

pub struct ResponsesMessagesNonStreamDecoder {
    pub(super) context: ConversionContext,
}
impl ResponsesMessagesNonStreamDecoder {
    pub fn boxed(context: &ConversionContext) -> Box<dyn NonStreamDecoder + Send + Sync> {
        Box::new(Self {
            context: context.clone(),
        })
    }
}
impl NonStreamDecoder for ResponsesMessagesNonStreamDecoder {
    fn decode(&self, body: &Value) -> Result<DecodedResponse, DecodeError> {
        let usage = identity::parse_usage(types::Protocol::Responses, body);
        let chat =
            decode_responses_response_to_chat(body, &self.context).map_err(DecodeError::from)?;
        chat::decode_chat_response_to_messages(&chat, &self.context)
            .map(|body| DecodedResponse { body, usage })
            .map_err(DecodeError::from)
    }
}

pub struct MessagesResponsesNonStreamDecoder {
    pub(super) context: ConversionContext,
}
impl MessagesResponsesNonStreamDecoder {
    pub fn boxed(context: &ConversionContext) -> Box<dyn NonStreamDecoder + Send + Sync> {
        Box::new(Self {
            context: context.clone(),
        })
    }
}
impl NonStreamDecoder for MessagesResponsesNonStreamDecoder {
    fn decode(&self, body: &Value) -> Result<DecodedResponse, DecodeError> {
        let usage = identity::parse_usage(types::Protocol::Messages, body);
        let chat = messages::decode_messages_response_to_chat(body, &self.context)
            .map_err(DecodeError::from)?;
        Ok(DecodedResponse {
            body: crate::protocol::openai_to_responses(&chat, &self.context.upstream_model),
            usage,
        })
    }
}
