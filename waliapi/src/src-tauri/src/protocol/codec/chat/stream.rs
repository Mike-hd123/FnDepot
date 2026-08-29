use super::super::error::{DecodeError, FeatureKind, UnsupportedFeatures};
use super::super::ports::StreamDecoder;
use super::super::report::{ConversionContext, Usage};
use super::super::sse;
use serde_json::Value;
use std::collections::BTreeMap;

// ===========================================================================
// Streaming: Chat SSE -> Messages SSE.
// ===========================================================================
#[derive(Default)]
struct ToolAccum {
    id: String,
    name: String,
    arguments: String,
    stopped: bool,
}

/// Per-request state for the Chat SSE → Messages SSE decoder.
#[derive(Default)]
pub struct ChatSseState {
    pending: Vec<u8>,
    started: bool,
    ended: bool,
    /// OpenAI-compatible providers commonly use this sentinel as the only
    /// terminal marker, omitting `choices[].finish_reason` entirely.
    saw_done: bool,
    /// A clean transport EOF after real assistant output is also a usable
    /// terminal signal for some OpenAI-compatible streaming providers.  Keep
    /// this separate from `started`: a role-only or usage-only frame must not
    /// turn a truncated response into a successful completion.
    saw_assistant_output: bool,
    finish_reason: Option<String>,
    usage: Usage,
    next_content_index: usize,
    open_text: Option<usize>,
    open_thinking: Option<usize>,
    tools: BTreeMap<usize, ToolAccum>,
    /// The mapped upstream model (from the PreparedAttempt) to emit in the
    /// synthesized `message_start` frame; the codec never re-maps models.
    pub model: String,
    /// Per-request downstream message id.
    pub message_id: String,
}

impl ChatSseState {
    /// Create the per-request state with the caller-provided model and id.
    pub fn new(model: &str, message_id: &str) -> Self {
        Self {
            model: model.to_string(),
            message_id: if message_id.is_empty() {
                format!("msg_{}", uuid::Uuid::new_v4().simple())
            } else {
                message_id.to_string()
            },
            ..Default::default()
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, UnsupportedFeatures> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = sse::record_end(&self.pending) {
            let record: Vec<u8> = self.pending.drain(..end).collect();
            let payload = sse::parse_data_payload(&record)?;
            if payload.is_empty() {
                continue;
            }
            if payload == "[DONE]" {
                self.saw_done = true;
                continue;
            }
            let json: Value = serde_json::from_str(&payload).map_err(|e| {
                UnsupportedFeatures::single(
                    FeatureKind::UnknownEvent,
                    "/",
                    format!("OpenAI upstream emitted invalid SSE JSON: {e}"),
                )
            })?;
            self.consume_json(json, &mut events)?;
        }
        Ok(events)
    }

    pub fn finish(&mut self) -> Result<Vec<String>, UnsupportedFeatures> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            let record = std::mem::take(&mut self.pending);
            let payload = sse::parse_data_payload(&record)?;
            if payload == "[DONE]" {
                self.saw_done = true;
            } else if !payload.is_empty() {
                let json: Value = serde_json::from_str(&payload).map_err(|e| {
                    UnsupportedFeatures::single(
                        FeatureKind::UnknownEvent,
                        "/",
                        format!("OpenAI upstream emitted invalid SSE JSON: {e}"),
                    )
                })?;
                self.consume_json(json, &mut events)?;
            }
        }
        self.emit_final(&mut events)?;
        Ok(events)
    }

    fn consume_json(
        &mut self,
        json: Value,
        events: &mut Vec<String>,
    ) -> Result<(), UnsupportedFeatures> {
        // usage may arrive as a standalone frame or on a choice frame.
        if let Some(u) = json.get("usage") {
            self.update_usage(u);
        }
        if !self.started {
            self.started = true;
            events.push(sse::event(
                "message_start",
                serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": self.message_id,
                        "type": "message",
                        "role": "assistant",
                        "model": self.model,
                        "content": [],
                        "stop_reason": null,
                        "stop_sequence": null,
                        "usage": {"input_tokens": self.usage.input_tokens, "output_tokens": 0}
                    }
                }),
            ));
        }
        for choice in json
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            // Fail-open: upstream reasoning is emitted as a Messages `thinking`
            // block (start/delta/stop), never rejected.  Some OpenAI-compat
            // providers surface it as `reasoning_content` (string) or a
            // `thinking` object; both are accepted.
            let reasoning_text = delta
                .get("reasoning_content")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .or_else(|| match delta.get("thinking") {
                    Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                    Some(Value::Object(m)) => m
                        .get("text")
                        .or_else(|| m.get("thinking"))
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                        .map(str::to_string),
                    _ => None,
                });
            if let Some(text) = reasoning_text {
                self.saw_assistant_output = true;
                let index = self.ensure_thinking(events);
                events.push(sse::event(
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "thinking_delta", "thinking": text}
                    }),
                ));
            }
            if let Some(text) = delta
                .get("content")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
            {
                self.saw_assistant_output = true;
                let index = self.ensure_text(events);
                events.push(sse::event(
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "text_delta", "text": text}
                    }),
                ));
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                if !calls.is_empty() {
                    self.saw_assistant_output = true;
                }
                for call in calls {
                    self.consume_tool_call(call)?;
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                if !reason.is_empty() && reason != "null" {
                    // Unknown finish reason is rejected at finalize (never
                    // downgraded), but we record it now.
                    self.finish_reason = Some(reason.to_string());
                }
            }
            if delta.get("refusal").and_then(Value::as_str).is_some() {
                self.finish_reason = Some("refusal".to_string());
            }
        }
        Ok(())
    }

    fn update_usage(&mut self, u: &Value) {
        let prompt = u.get("prompt_tokens").and_then(Value::as_u64);
        let completion = u.get("completion_tokens").and_then(Value::as_u64);
        if prompt.is_some() {
            self.usage.input_tokens = prompt.unwrap();
        }
        if completion.is_some() {
            self.usage.output_tokens = completion.unwrap();
        }
        if let Some(c) = u
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
        {
            self.usage.cache_read_input_tokens = c;
        }
        if let Some(c) = u.get("cache_creation_input_tokens").and_then(Value::as_u64) {
            self.usage.cache_creation_input_tokens = c;
        }
        if prompt.is_none() && completion.is_none() {
            // A bare usage frame with no real tokens is not a usable count.
            if self.usage.input_tokens == 0 && self.usage.output_tokens == 0 {
                self.usage.usage_unknown = true;
            }
        }
    }

    fn ensure_text(&mut self, events: &mut Vec<String>) -> usize {
        if let Some(index) = self.open_text {
            return index;
        }
        let index = self.next_content_index;
        self.next_content_index += 1;
        self.open_text = Some(index);
        events.push(sse::event(
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "text", "text": ""}
            }),
        ));
        index
    }

    fn ensure_thinking(&mut self, events: &mut Vec<String>) -> usize {
        if let Some(index) = self.open_thinking {
            return index;
        }
        let index = self.next_content_index;
        self.next_content_index += 1;
        self.open_thinking = Some(index);
        events.push(sse::event(
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "thinking", "thinking": ""}
            }),
        ));
        index
    }

    fn consume_tool_call(&mut self, call: &Value) -> Result<(), UnsupportedFeatures> {
        let source_index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        if let Some(id) = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            self.tools.entry(source_index).or_default().id = id.to_string();
        }
        if let Some(name) = call
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .or_else(|| call.get("name").and_then(Value::as_str))
            .filter(|name| !name.is_empty())
        {
            self.tools.entry(source_index).or_default().name = name.to_string();
        }
        if let Some(arguments) = call
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str)
            .or_else(|| call.get("arguments").and_then(Value::as_str))
        {
            self.tools
                .entry(source_index)
                .or_default()
                .arguments
                .push_str(arguments);
        }
        Ok(())
    }

    fn emit_final(&mut self, events: &mut Vec<String>) -> Result<(), UnsupportedFeatures> {
        if self.ended {
            return Ok(());
        }
        if !self.started {
            // The upstream stream never delivered a first frame.  This is a
            // codec error (not an empty success) so the gateway can fail over
            // before committing the downstream response.
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "OpenAI upstream stream ended before any first frame (no message_start emitted)",
            ));
        }
        if let Some(index) = self.open_text.take() {
            events.push(sse::event(
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop",
                    "index": index
                }),
            ));
        }
        if let Some(index) = self.open_thinking.take() {
            events.push(sse::event(
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop",
                    "index": index
                }),
            ));
        }
        for tool in self.tools.values_mut() {
            if tool.name.is_empty() {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::MissingToolField,
                    "/choices/0/delta/tool_calls",
                    "OpenAI stream ended with an incomplete tool call",
                ));
            }
            if tool.id.is_empty() {
                tool.id = format!("call_{}", uuid::Uuid::new_v4().simple());
            }
            let input: Value = serde_json::from_str(&tool.arguments).map_err(|e| {
                UnsupportedFeatures::single(
                    FeatureKind::InvalidToolArguments,
                    "/choices/0/delta/tool_calls",
                    format!("OpenAI stream ended with invalid tool arguments: {e}"),
                )
            })?;
            if !input.is_object() {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::InvalidToolArguments,
                    "/choices/0/delta/tool_calls",
                    "OpenAI stream tool arguments must decode to a JSON object",
                ));
            }
            let index = self.next_content_index;
            self.next_content_index += 1;
            events.push(sse::event("content_block_start", serde_json::json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "tool_use", "id": tool.id, "name": tool.name, "input": {}}
            })));
            events.push(sse::event(
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {"type": "input_json_delta", "partial_json": tool.arguments}
                }),
            ));
            tool.stopped = true;
            events.push(sse::event(
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop",
                    "index": index
                }),
            ));
        }
        let stop_reason = match self.finish_reason.as_deref() {
            Some("stop") => "end_turn",
            Some("length") => "max_tokens",
            Some("tool_calls") | Some("function_call") => "tool_use",
            Some("content_filter") | Some("refusal") => "refusal",
            Some(other) => {
                // CPA-compatible terminal fallback: the upstream has already
                // completed a valid Chat stream, but this gateway supplied a
                // provider-specific finish reason.  Anthropic has no lossless
                // representation for it, so finish the Message normally and
                // retain the original value in structured logs.
                tracing::warn!(
                    finish_reason = other,
                    fallback_stop_reason = "end_turn",
                    "unknown OpenAI Chat stream finish_reason mapped for Anthropic compatibility"
                );
                "end_turn"
            }
            None => {
                if !self.tools.is_empty() {
                    "tool_use"
                } else if self.saw_done || self.saw_assistant_output {
                    // `[DONE]` is the preferred positive terminal signal.
                    // Some OpenAI-compatible providers instead close a clean
                    // SSE response after sending real assistant output, but
                    // omit both `[DONE]` and the final choice frame.  There is
                    // no lossless Chat finish_reason to map, so complete as
                    // Anthropic `end_turn`. A network error, malformed final
                    // record, or role/usage-only EOF still remains an error.
                    tracing::warn!(
                        saw_done = self.saw_done,
                        saw_assistant_output = self.saw_assistant_output,
                        fallback_stop_reason = "end_turn",
                        "OpenAI Chat stream ended without finish_reason; accepting a valid terminal signal"
                    );
                    "end_turn"
                } else {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::UnknownFinishReason,
                        "/choices/0/finish_reason",
                        "OpenAI stream ended without a finish_reason",
                    ));
                }
            }
        };
        events.push(sse::event(
            "message_delta",
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": stop_reason, "stop_sequence": null},
                "usage": {
                    "input_tokens": self.usage.input_tokens,
                    "output_tokens": self.usage.output_tokens,
                    "cache_creation_input_tokens": self.usage.cache_creation_input_tokens,
                    "cache_read_input_tokens": self.usage.cache_read_input_tokens,
                }
            }),
        ));
        events.push(sse::event(
            "message_stop",
            serde_json::json!({"type": "message_stop"}),
        ));
        self.ended = true;
        Ok(())
    }
}

pub struct ChatStreamDecoder {
    state: ChatSseState,
}

impl ChatStreamDecoder {
    pub fn boxed(context: &ConversionContext) -> Box<dyn StreamDecoder + Send + Sync> {
        Box::new(ChatStreamDecoder {
            state: ChatSseState::new(&context.upstream_model, &context.request_id),
        })
    }
}

impl StreamDecoder for ChatStreamDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, DecodeError> {
        self.state.feed(bytes).map_err(DecodeError::from)
    }
    fn finish(&mut self) -> Result<Vec<String>, DecodeError> {
        self.state.finish().map_err(DecodeError::from)
    }
    fn usage(&self) -> Option<Usage> {
        Some(self.state.usage)
    }
}
