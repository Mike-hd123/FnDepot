use super::super::error::{DecodeError, FeatureKind, UnsupportedFeatures};
use super::super::ports::StreamDecoder;
use super::super::report::{ConversionContext, Usage};
use super::super::sse;
use serde_json::Value;
use std::collections::BTreeMap;

// ===========================================================================
// Streaming: Messages SSE -> Chat SSE.
// ===========================================================================

#[derive(Default)]
struct MsgToolAccum {
    index: usize,
    arguments: String,
    completed: bool,
}

/// Per-request state for the Messages SSE → Chat SSE decoder.
#[derive(Default)]
pub struct MessagesSseState {
    pending: Vec<u8>,
    started: bool,
    ended: bool,
    text_content_index: Option<usize>,
    tools: BTreeMap<usize, MsgToolAccum>,
    next_tool_index: usize,
    stop_reason: Option<String>,
    usage: Usage,
    message_id: String,
    current_block: Option<String>,
    /// The mapped upstream model to emit in the synthesized Chat `role` frame.
    pub model: String,
}

impl MessagesSseState {
    /// Create the per-request state with the caller-provided model.
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Default::default()
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, UnsupportedFeatures> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = sse::record_end(&self.pending) {
            let record: Vec<u8> = self.pending.drain(..end).collect();
            let payload = sse::parse_data_payload(&record)?;
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let json: Value = serde_json::from_str(&payload).map_err(|e| {
                UnsupportedFeatures::single(
                    FeatureKind::UnknownEvent,
                    "/",
                    format!("Anthropic upstream emitted invalid SSE JSON: {e}"),
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
            if !payload.is_empty() && payload != "[DONE]" {
                let json: Value = serde_json::from_str(&payload).map_err(|e| {
                    UnsupportedFeatures::single(
                        FeatureKind::UnknownEvent,
                        "/",
                        format!("Anthropic upstream emitted invalid SSE JSON: {e}"),
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
        let ty = json.get("type").and_then(Value::as_str).ok_or_else(|| {
            UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/type",
                "Anthropic SSE frame missing type",
            )
        })?;
        match ty {
            "message_start" => {
                if !self.started {
                    self.started = true;
                    if let Some(msg) = json.get("message") {
                        if let Some(id) = msg.get("id").and_then(Value::as_str) {
                            self.message_id = id.to_string();
                        }
                        if let Some(u) = msg.get("usage") {
                            self.update_usage(u);
                        }
                    }
                    // Emit the Chat `role` frame now.
                    events.push(sse::data_frame(serde_json::json!({
                        "id": self.message_id,
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": null}]
                    })));
                }
                // a second message_start is ignored.
            }
            "content_block_start" => {
                let index = json.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let block = json.get("content_block").unwrap_or(&Value::Null);
                let bt = block.get("type").and_then(Value::as_str);
                self.current_block = Some(bt.unwrap_or("").to_string());
                match bt {
                    Some("text") => {
                        self.text_content_index = Some(index);
                    }
                    Some("thinking") | Some("redacted_thinking") => {
                        // Fail-open: reasoning is forwarded as OpenAI
                        // `reasoning_content` deltas.  `redacted_thinking` has
                        // no visible text (signature only) — its deltas are
                        // ignored but not rejected.  current_block is already
                        // recorded above, so text_delta-like handling below
                        // routes on the delta type, not the block type.
                    }
                    Some("tool_use") => {
                        // id and name are mandatory on a tool_use block; never
                        // emit an empty-id/empty-name tool call (R22).
                        let id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .ok_or_else(|| {
                                UnsupportedFeatures::single(
                                    FeatureKind::MissingToolField,
                                    "/content_block_start/content_block/id",
                                    "tool_use block missing id",
                                )
                            })?
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .ok_or_else(|| {
                                UnsupportedFeatures::single(
                                    FeatureKind::MissingToolField,
                                    "/content_block_start/content_block/name",
                                    "tool_use block missing name",
                                )
                            })?
                            .to_string();
                        let tool_index = self.next_tool_index;
                        self.next_tool_index += 1;
                        self.tools.insert(
                            index,
                            MsgToolAccum {
                                index: tool_index,
                                arguments: String::new(),
                                completed: false,
                            },
                        );
                        // Emit the Chat tool_calls delta immediately (id + name +
                        // empty arguments) so consumers see the call id early.
                        events.push(sse::data_frame(serde_json::json!({
                            "choices": [{"index": 0, "delta": {"tool_calls": [{
                                "index": tool_index,
                                "id": id,
                                "type": "function",
                                "function": {"name": name, "arguments": ""}
                            }]}, "finish_reason": null}]
                        })));
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let index = json.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delta = json.get("delta").unwrap_or(&Value::Null);
                let dt = delta.get("type").and_then(Value::as_str);
                match dt {
                    Some("text_delta") => {
                        let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        if !text.is_empty() {
                            events.push(sse::data_frame(serde_json::json!({
                                "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
                            })));
                        }
                    }
                    Some("input_json_delta") => {
                        let partial = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if let Some(tool) = self.tools.get_mut(&index) {
                            if !tool.completed {
                                tool.arguments.push_str(partial);
                            }
                        }
                    }
                    Some("thinking_delta") => {
                        let reasoning = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if !reasoning.is_empty() {
                            events.push(sse::data_frame(serde_json::json!({
                                "choices": [{"index": 0, "delta": {"reasoning_content": reasoning}, "finish_reason": null}]
                            })));
                        }
                    }
                    Some("signature_delta") => {
                        // Encrypted/reference signature — no usable text for
                        // the Chat downstream; drop fail-open.
                    }
                    _ => {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::UnknownEvent,
                            "/delta/type",
                            format!("unknown content_block_delta type {dt:?}"),
                        ))
                    }
                }
            }
            "content_block_stop" => {
                let index = json.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(tool) = self.tools.get_mut(&index) {
                    if !tool.completed {
                        // complete the tool call: emit the full accumulated
                        // arguments only when it is valid JSON (object).
                        let input: Value = serde_json::from_str(&tool.arguments).map_err(|e| {
                            UnsupportedFeatures::single(
                                FeatureKind::InvalidToolArguments,
                                "/content_block_delta/partial_json",
                                format!("tool arguments did not form valid JSON: {e}"),
                            )
                        })?;
                        if !input.is_object() {
                            return Err(UnsupportedFeatures::single(
                                FeatureKind::InvalidToolArguments,
                                "/content_block_delta/partial_json",
                                "tool arguments must decode to a JSON object",
                            ));
                        }
                        // Emit the remainder (if any) then nothing else needed:
                        // consumers already saw id/name; we must not re-send id.
                        // Send an arguments-only delta with full args.
                        events.push(sse::data_frame(serde_json::json!({
                            "choices": [{"index": 0, "delta": {"tool_calls": [{
                                "index": tool.index,
                                "function": {"arguments": tool.arguments}
                            }]}, "finish_reason": null}]
                        })));
                        tool.completed = true;
                    }
                }
            }
            "message_delta" => {
                if let Some(delta) = json.get("delta") {
                    if let Some(reason) = delta.get("stop_reason").and_then(Value::as_str) {
                        self.stop_reason = Some(reason.to_string());
                    }
                }
                if let Some(u) = json.get("usage") {
                    self.update_usage(u);
                }
            }
            "message_stop" => {
                // exactly-once termination handled by emit_final
            }
            "ping" => {}
            "error" => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownEvent,
                    "/type",
                    format!("Anthropic upstream error event: {}", json),
                ))
            }
            other => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownEvent,
                    "/type",
                    format!("unknown Anthropic SSE event type {other:?}"),
                ))
            }
        }
        Ok(())
    }

    fn update_usage(&mut self, u: &Value) {
        let input = u.get("input_tokens").and_then(Value::as_u64);
        let output = u.get("output_tokens").and_then(Value::as_u64);
        if let Some(i) = input {
            self.usage.input_tokens = i;
        }
        if let Some(o) = output {
            self.usage.output_tokens = o;
        }
        if let Some(c) = u.get("cache_creation_input_tokens").and_then(Value::as_u64) {
            self.usage.cache_creation_input_tokens = c;
        }
        if let Some(c) = u.get("cache_read_input_tokens").and_then(Value::as_u64) {
            self.usage.cache_read_input_tokens = c;
        }
        if input.is_none()
            && output.is_none()
            && self.usage.input_tokens == 0
            && self.usage.output_tokens == 0
        {
            self.usage.usage_unknown = true;
        }
    }

    fn emit_final(&mut self, events: &mut Vec<String>) -> Result<(), UnsupportedFeatures> {
        if self.ended {
            return Ok(());
        }
        if !self.started {
            // The upstream stream never delivered a message_start frame.  This
            // is a codec error (not an empty success) so the gateway can fail
            // over before committing the downstream response.
            self.ended = true;
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Anthropic upstream stream ended before any first frame (no message_start)",
            ));
        }
        // Validate any in-progress tool calls: Anthropic may send
        // content_block_stop for a tool_use with no deltas yet (empty object).
        for tool in self.tools.values() {
            if !tool.completed {
                // A tool_use that never saw content_block_stop is malformed.
                return Err(UnsupportedFeatures::single(
                    FeatureKind::MissingToolField,
                    "/content_block_start/content_block",
                    "Anthropic stream ended with an incomplete tool call",
                ));
            }
        }
        let finish_reason = match self.stop_reason.as_deref() {
            // Same normalization as the non-streaming path: stop-like reasons
            // (refusal / stop_sequence / pause_turn) collapse to `stop`, and a
            // context-window overrun behaves like `length` — never a hard error.
            Some("end_turn") | Some("refusal") | Some("stop_sequence") | Some("pause_turn") => {
                "stop"
            }
            Some("max_tokens") | Some("model_context_window_exceeded") => "length",
            Some("tool_use") => "tool_calls",
            Some(other) => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownFinishReason,
                    "/message_delta/delta/stop_reason",
                    format!("unknown Messages stop_reason {other:?}"),
                ))
            }
            None => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownFinishReason,
                    "/message_delta/delta/stop_reason",
                    "Anthropic stream ended without a stop_reason",
                ))
            }
        };
        // OpenAI streaming chunks must ALWAYS carry `choices` (Opencode etc.
        // reject a bare `{"usage":...}` frame), so the usage is merged into the
        // final finish_reason frame — the OpenAI-canonical shape.
        events.push(sse::data_frame(serde_json::json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": finish_reason}],
            "usage": {
                "prompt_tokens": self.usage.input_tokens,
                "completion_tokens": self.usage.output_tokens,
                "total_tokens": self.usage.input_tokens + self.usage.output_tokens,
                "prompt_tokens_details": {"cached_tokens": self.usage.cache_read_input_tokens},
                "cache_creation_input_tokens": self.usage.cache_creation_input_tokens,
                "cache_read_input_tokens": self.usage.cache_read_input_tokens,
            }
        })));
        events.push(sse::data_frame(Value::String("[DONE]".to_string())));
        self.ended = true;
        Ok(())
    }
}

pub struct MessagesStreamDecoder {
    state: MessagesSseState,
}

impl MessagesStreamDecoder {
    pub fn boxed(context: &ConversionContext) -> Box<dyn StreamDecoder + Send + Sync> {
        Box::new(MessagesStreamDecoder {
            state: MessagesSseState::new(&context.upstream_model),
        })
    }
}

impl StreamDecoder for MessagesStreamDecoder {
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
