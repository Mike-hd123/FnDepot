//! OpenAI Chat Completions SSE -> Anthropic Messages SSE codec.
//!
//! This module never handles native Anthropic streams. They are byte-for-byte
//! proxied by the handler, because parsing and re-emitting them loses forward
//! compatibility with Claude Code.

use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct AnthropicStreamState {
    pending: Vec<u8>,
    started: bool,
    ended: bool,
    finish_reason: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    next_content_index: usize,
    open_text: Option<usize>,
    open_thinking: Option<usize>,
    tools: BTreeMap<usize, ToolState>,
}

#[derive(Default)]
struct ToolState {
    content_index: Option<usize>,
    id: String,
    name: String,
    arguments: String,
    stopped: bool,
}

fn event(name: &str, value: Value) -> String {
    format!("event: {name}\ndata: {value}\n\n")
}

impl AnthropicStreamState {
    pub fn usage(&self) -> (i64, i64) {
        (self.input_tokens as i64, self.output_tokens as i64)
    }
    /// Feed arbitrary network bytes.  A TCP chunk may split a UTF-8 codepoint,
    /// an SSE field, or the CRLF event delimiter, so bytes are retained until a
    /// complete event is available.
    pub fn feed(
        &mut self,
        bytes: &[u8],
        model: &str,
        message_id: &str,
    ) -> Result<Vec<String>, String> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = sse_record_end(&self.pending) {
            let record: Vec<u8> = self.pending.drain(..end).collect();
            let payload = parse_sse_data(&record)?;
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let json: Value = serde_json::from_str(&payload)
                .map_err(|error| format!("OpenAI upstream emitted invalid SSE JSON: {error}"))?;
            self.consume_json(json, model, message_id, &mut events)?;
        }
        Ok(events)
    }

    /// Flush an EOF-terminated event and emit the exactly-once final sequence.
    pub fn finish(&mut self, model: &str, message_id: &str) -> Result<Vec<String>, String> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            let record = std::mem::take(&mut self.pending);
            let payload = parse_sse_data(&record)?;
            if !payload.is_empty() && payload != "[DONE]" {
                let json: Value = serde_json::from_str(&payload).map_err(|error| {
                    format!("OpenAI upstream emitted invalid SSE JSON: {error}")
                })?;
                self.consume_json(json, model, message_id, &mut events)?;
            }
        }
        self.emit_final(&mut events)?;
        Ok(events)
    }

    fn consume_json(
        &mut self,
        json: Value,
        model: &str,
        message_id: &str,
        events: &mut Vec<String>,
    ) -> Result<(), String> {
        self.update_usage(&json);
        if !self.started {
            self.started = true;
            events.push(event("message_start", serde_json::json!({
                "type": "message_start",
                "message": {"id": message_id, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": self.input_tokens, "output_tokens": 0}}
            })));
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
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .or_else(|| match delta.get("thinking") {
                    Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                    Some(Value::Object(m)) => m
                        .get("text")
                        .or_else(|| m.get("thinking"))
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string),
                    _ => None,
                });
            if let Some(text) = reasoning_text {
                let index = self.ensure_thinking(events);
                events.push(event("content_block_delta", serde_json::json!({"type":"content_block_delta", "index":index, "delta":{"type":"thinking_delta", "thinking":text}})));
            }
            if let Some(text) = delta
                .get("content")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                let index = self.ensure_text(events);
                events.push(event("content_block_delta", serde_json::json!({"type":"content_block_delta", "index":index, "delta":{"type":"text_delta", "text":text}})));
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    self.consume_tool_call(call)?;
                }
            }
            if let Some(reason) = choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .filter(|reason| !reason.is_empty() && *reason != "null")
            {
                self.finish_reason = Some(reason.to_string());
            }
            if delta.get("refusal").and_then(Value::as_str).is_some() {
                self.finish_reason = Some("content_filter".to_string());
            }
        }
        Ok(())
    }

    fn update_usage(&mut self, json: &Value) {
        if let Some(usage) = json.get("usage") {
            self.input_tokens = usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(self.input_tokens);
            self.output_tokens = usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(self.output_tokens);
        }
    }

    fn ensure_text(&mut self, events: &mut Vec<String>) -> usize {
        if let Some(index) = self.open_text {
            return index;
        }
        let index = self.next_content_index;
        self.next_content_index += 1;
        self.open_text = Some(index);
        events.push(event("content_block_start", serde_json::json!({"type":"content_block_start", "index":index, "content_block":{"type":"text", "text":""}})));
        index
    }

    fn ensure_thinking(&mut self, events: &mut Vec<String>) -> usize {
        if let Some(index) = self.open_thinking {
            return index;
        }
        let index = self.next_content_index;
        self.next_content_index += 1;
        self.open_thinking = Some(index);
        events.push(event("content_block_start", serde_json::json!({"type":"content_block_start", "index":index, "content_block":{"type":"thinking", "thinking":""}})));
        index
    }

    fn consume_tool_call(&mut self, call: &Value) -> Result<(), String> {
        let source_index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let id = call.get("id").and_then(Value::as_str);
        let name = call
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str);
        let arguments = call
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str);
        let tool = self.tools.entry(source_index).or_default();
        if let Some(id) = id {
            tool.id = id.to_string();
        }
        if let Some(name) = name {
            tool.name = name.to_string();
        }
        if let Some(arguments) = arguments {
            tool.arguments.push_str(arguments);
        }
        Ok(())
    }

    fn emit_final(&mut self, events: &mut Vec<String>) -> Result<(), String> {
        if self.ended || !self.started {
            return Ok(());
        }
        if let Some(index) = self.open_text.take() {
            events.push(event(
                "content_block_stop",
                serde_json::json!({"type":"content_block_stop", "index":index}),
            ));
        }
        if let Some(index) = self.open_thinking.take() {
            events.push(event(
                "content_block_stop",
                serde_json::json!({"type":"content_block_stop", "index":index}),
            ));
        }
        // OpenAI deltas for parallel calls may interleave. Anthropic content
        // blocks may not: serialize each complete tool block only after every
        // text block has stopped.
        for tool in self.tools.values_mut() {
            if tool.id.is_empty() || tool.name.is_empty() {
                return Err("OpenAI stream ended with an incomplete tool call".to_string());
            }
            let input: Value = serde_json::from_str(&tool.arguments).map_err(|error| {
                format!("OpenAI stream ended with invalid tool arguments: {error}")
            })?;
            if !input.is_object() {
                return Err("OpenAI stream tool arguments must decode to a JSON object".to_string());
            }
            let index = self.next_content_index;
            self.next_content_index += 1;
            tool.content_index = Some(index);
            events.push(event("content_block_start", serde_json::json!({"type":"content_block_start", "index":index, "content_block":{"type":"tool_use", "id":tool.id, "name":tool.name, "input":{}}})));
            events.push(event("content_block_delta", serde_json::json!({"type":"content_block_delta", "index":index, "delta":{"type":"input_json_delta", "partial_json":tool.arguments}})));
            tool.stopped = true;
            events.push(event(
                "content_block_stop",
                serde_json::json!({"type":"content_block_stop", "index":index}),
            ));
        }
        let stop_reason = match self.finish_reason.as_deref() {
            Some("length") => "max_tokens",
            Some("tool_calls") | Some("function_call") => "tool_use",
            Some("content_filter") => "refusal",
            None if !self.tools.is_empty() => "tool_use",
            _ => "end_turn",
        };
        events.push(event("message_delta", serde_json::json!({"type":"message_delta", "delta":{"stop_reason":stop_reason, "stop_sequence":null}, "usage":{"input_tokens":self.input_tokens, "output_tokens":self.output_tokens}})));
        events.push(event(
            "message_stop",
            serde_json::json!({"type":"message_stop"}),
        ));
        self.ended = true;
        Ok(())
    }
}

fn sse_record_end(input: &[u8]) -> Option<usize> {
    let crlf = input
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4);
    let lf = input
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| index + 2);
    match (crlf, lf) {
        (Some(crlf), Some(lf)) => Some(crlf.min(lf)),
        (Some(end), None) | (None, Some(end)) => Some(end),
        (None, None) => None,
    }
}

fn parse_sse_data(record: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(record)
        .map_err(|_| "OpenAI upstream SSE was not valid UTF-8".to_string())?;
    let mut lines = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(data) = line.strip_prefix("data:") {
            lines.push(data.strip_prefix(' ').unwrap_or(data));
        }
    }
    Ok(lines.join("\n"))
}

// Kept for the older OpenAI-only handlers while they remain available. The
// Anthropic Messages endpoint uses `feed`/`finish` directly above.
#[allow(dead_code)]
pub fn convert_openai_sse_to_anthropic(
    chunk: &str,
    model: &str,
    message_id: &str,
    state: &mut AnthropicStreamState,
) -> Vec<String> {
    state
        .feed(chunk.as_bytes(), model, message_id)
        .unwrap_or_default()
}

#[allow(dead_code)]
pub fn parse_usage_from_sse_chunk(text: &str) -> Option<(i64, i64, i64)> {
    for record in text.split("\n\n") {
        let data = parse_sse_data(record.as_bytes()).ok()?;
        if let Ok(json) = serde_json::from_str::<Value>(&data) {
            if let Some(usage) = json.get("usage") {
                let input = usage
                    .get("prompt_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let output = usage
                    .get("completion_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let total = usage
                    .get("total_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(input + output);
                if input > 0 || output > 0 || total > 0 {
                    return Some((input, output, total));
                }
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod anthropic_tests;
