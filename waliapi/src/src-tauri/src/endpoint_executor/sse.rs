//! SSE framing + the streaming commit-barrier pump.
//!
//! Protocol conversion is deliberately absent from this module.  A selected
//! [`PreparedCodec`](crate::protocol::codec::PreparedCodec) creates the decoder
//! before entering the pump, so this type only owns framing, commit state and
//! decoder driving.

use crate::core::stream_supervisor::{StreamSupervisor, StreamTransitionError};
use crate::protocol::codec::StreamDecoder;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum PumpError {
    Protocol(String),
    Supervisor(String),
}

impl From<StreamTransitionError> for PumpError {
    fn from(error: StreamTransitionError) -> Self {
        Self::Supervisor(format!("{error:?}"))
    }
}

impl PumpError {
    pub fn message(&self) -> &str {
        match self {
            Self::Protocol(message) | Self::Supervisor(message) => message,
        }
    }
}

pub fn record_end(input: &[u8]) -> Option<usize> {
    crate::protocol::codec::sse::record_end(input)
}

pub fn parse_data_payload(record: &[u8]) -> Result<String, String> {
    crate::protocol::codec::sse::parse_data_payload(record).map_err(|error| error.message)
}

/// Validate only enough framing to retain the pre-commit failover barrier.
/// Full protocol validation belongs to the decoder factory selected at prepare
/// time and therefore runs inside [`StreamPumpCore::new`].
pub fn validate_native_first_record(record: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(record);
    if text.contains("event:") {
        return Ok(());
    }
    let payload = parse_data_payload(record)?;
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(());
    }
    serde_json::from_str::<Value>(&payload)
        .map(|_| ())
        .map_err(|error| format!("first SSE data frame is not valid JSON: {error}"))
}

/// A protocol-agnostic pump. The decoder is mandatory: identity directions use
/// the same path as conversions, which prevents an executor-side "native"
/// branch from bypassing validation or usage collection.
pub struct StreamPumpCore {
    supervisor: StreamSupervisor,
    decoder: Box<dyn StreamDecoder + Send + Sync>,
    first_frame: Vec<u8>,
    first_done: bool,
    terminal_registered: bool,
    finished: bool,
    accumulated_content: String,
    accumulated_reasoning: String,
    /// role from the first delta that carries one (defaults to "assistant").
    response_role: Option<String>,
    /// finish_reason captured from the terminal SSE record.
    finish_reason: Option<String>,
    /// tool_calls accumulated from stream deltas, keyed by index.
    tool_calls_map: std::collections::BTreeMap<i64, serde_json::Value>,
}

impl StreamPumpCore {
    /// Feed the complete first record and any bytes read past it into the same
    /// fresh decoder before committing downstream. This makes an invalid first
    /// response (including an identity response) retryable without leaking raw
    /// bytes to the client.
    pub fn new(
        supervisor: StreamSupervisor,
        mut decoder: Box<dyn StreamDecoder + Send + Sync>,
        first_frame: Vec<u8>,
        carry: Vec<u8>,
    ) -> Result<Self, PumpError> {
        let mut output = Vec::new();
        let mut accumulated_content = String::new();
        let mut accumulated_reasoning = String::new();
        let mut response_role: Option<String> = None;
        let mut finish_reason: Option<String> = None;
        let mut tool_calls_map = std::collections::BTreeMap::<i64, serde_json::Value>::new();
        for bytes in [&first_frame[..], &carry[..]] {
            if bytes.is_empty() {
                continue;
            }
            let events = decoder.feed(bytes).map_err(|error| {
                PumpError::Protocol(format!("upstream stream could not be decoded: {error}"))
            })?;
            for event in &events {
                output.extend_from_slice(event.as_bytes());
                accumulate_from_sse_event(event, &mut accumulated_content, &mut accumulated_reasoning, &mut response_role, &mut finish_reason, &mut tool_calls_map);
            }
        }
        Ok(Self {
            supervisor,
            decoder,
            first_frame: output,
            first_done: false,
            terminal_registered: false,
            finished: false,
            accumulated_content,
            accumulated_reasoning,
            response_role,
            finish_reason,
            tool_calls_map,
        })
    }

    pub fn start(&mut self) -> Result<Vec<u8>, PumpError> {
        if self.first_done {
            return Ok(Vec::new());
        }
        self.supervisor.commit_downstream()?;
        self.supervisor.begin_streaming()?;
        self.first_done = true;
        Ok(std::mem::take(&mut self.first_frame))
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<u8>, PumpError> {
        let mut output = self.start()?;
        let events = self.decoder.feed(bytes).map_err(|error| {
            PumpError::Protocol(format!("upstream stream could not be decoded: {error}"))
        })?;
        for event in &events {
            output.extend_from_slice(event.as_bytes());
            self.accumulate_from_sse(event);
        }
        Ok(output)
    }

    pub fn finish(&mut self) -> Result<Vec<u8>, PumpError> {
        if self.finished {
            return Ok(Vec::new());
        }
        self.finished = true;
        let mut output = self.start()?;
        let events = self.decoder.finish().map_err(|error| {
            PumpError::Protocol(format!(
                "upstream stream ended with an incomplete decode: {error}"
            ))
        })?;
        for event in &events {
            output.extend_from_slice(event.as_bytes());
            self.accumulate_from_sse(event);
        }
        // Decoder correctness includes protocol terminal validation. The pump
        // owns only the supervisor's exactly-once terminal transition.
        if !self.terminal_registered {
            self.terminal_registered = self.supervisor.register_terminal();
        }
        Ok(output)
    }

    pub fn committed(&self) -> bool {
        self.supervisor.committed()
    }

    pub fn terminated(&self) -> bool {
        self.supervisor.terminal_emitted()
    }

    pub fn usage(&self) -> (i64, i64, i64) {
        self.decoder.usage().map_or((0, 0, 0), |usage| {
            let prompt = usage.input_tokens as i64;
            let completion = usage.output_tokens as i64;
            (prompt, completion, prompt + completion)
        })
    }

    pub fn accumulated_content(&self) -> &str {
        &self.accumulated_content
    }

    /// Accumulate content/reasoning/tool_calls from a downstream SSE event string.
    fn accumulate_from_sse(&mut self, event: &str) {
        accumulate_from_sse_event(
            event,
            &mut self.accumulated_content,
            &mut self.accumulated_reasoning,
            &mut self.response_role,
            &mut self.finish_reason,
            &mut self.tool_calls_map,
        );
    }

    /// Build a `response_choices` JSON string suitable for the audit log,
    /// consuming accumulated stream content.  Returns `None` when nothing
    /// was accumulated (empty stream or error).
    pub fn build_response_choices(&self) -> Option<String> {
        let has_content = !self.accumulated_content.is_empty()
            || !self.accumulated_reasoning.is_empty()
            || !self.tool_calls_map.is_empty();
        if !has_content {
            return None;
        }
        let mut message = serde_json::json!({
            "role": self.response_role.as_deref().unwrap_or("assistant"),
        });
        if !self.accumulated_content.is_empty() {
            message["content"] = serde_json::json!(&self.accumulated_content);
        }
        if !self.accumulated_reasoning.is_empty() {
            message["reasoning_content"] = serde_json::json!(&self.accumulated_reasoning);
        }
        if !self.tool_calls_map.is_empty() {
            let tcs: Vec<serde_json::Value> = self.tool_calls_map.values().cloned().collect();
            message["tool_calls"] = serde_json::json!(tcs);
        }
        let choices = vec![serde_json::json!({
            "index": 0,
            "message": message,
            "finish_reason": self.finish_reason.as_deref().unwrap_or("stop"),
        })];
        Some(serde_json::to_string(&choices).unwrap_or_default())
    }

    #[allow(dead_code)]
    pub fn abort(&mut self, reason: impl Into<String>) -> Result<(), PumpError> {
        self.supervisor.abort(reason).map_err(PumpError::from)
    }

    #[allow(dead_code)]
    pub fn client_cancel(&mut self) -> Result<(), PumpError> {
        self.supervisor.client_cancel().map_err(PumpError::from)
    }
}

/// Parse a single SSE event string and accumulate content/reasoning/tool_calls.
/// Supports OpenAI Chat Completions, Anthropic Messages, and OpenAI Responses streaming formats.
fn accumulate_from_sse_event(
    event: &str,
    content: &mut String,
    reasoning: &mut String,
    role: &mut Option<String>,
    finish_reason: &mut Option<String>,
    tool_calls_map: &mut std::collections::BTreeMap<i64, serde_json::Value>,
) {
    // Each event may contain multiple `data:` lines.
    for line in event.lines() {
        let line = line.trim();
        if !line.starts_with("data:") {
            continue;
        }
        let payload = line[5..].trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // ── OpenAI Chat Completions streaming ──
        if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                // role (usually on the first delta)
                if let Some(r) = choice
                    .pointer("/delta/role")
                    .and_then(|r| r.as_str())
                {
                    if role.is_none() {
                        *role = Some(r.to_string());
                    }
                }
                // content
                if let Some(c) = choice
                    .pointer("/delta/content")
                    .and_then(|c| c.as_str())
                {
                    content.push_str(c);
                }
                // reasoning_content (DeepSeek / OpenAI o-series)
                if let Some(rc) = choice
                    .pointer("/delta/reasoning_content")
                    .and_then(|rc| rc.as_str())
                {
                    reasoning.push_str(rc);
                }
                // tool_calls
                if let Some(tcs) = choice
                    .pointer("/delta/tool_calls")
                    .and_then(|tc| tc.as_array())
                {
                    for tc in tcs {
                        let idx = tc
                            .get("index")
                            .and_then(|i| i.as_i64())
                            .unwrap_or(0);
                        let entry = tool_calls_map
                            .entry(idx)
                            .or_insert_with(|| serde_json::json!({"index": idx, "id": null, "type": "function", "function": {"name": "", "arguments": ""}}));
                        if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                            entry["id"] = serde_json::json!(id);
                        }
                        if let Some(t) = tc.pointer("/function/name").and_then(|n| n.as_str()) {
                            entry["function"]["name"] = serde_json::json!(t);
                        }
                        if let Some(args) = tc.pointer("/function/arguments").and_then(|a| a.as_str()) {
                            if let Some(existing) = entry.pointer("/function/arguments").and_then(|a| a.as_str()) {
                                entry["function"]["arguments"] = serde_json::json!(format!("{existing}{args}"));
                            } else {
                                entry["function"]["arguments"] = serde_json::json!(args);
                            }
                        }
                    }
                }
                // finish_reason
                if let Some(fr) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    *finish_reason = Some(fr.to_string());
                }
            }
            continue;
        }

        // ── Anthropic Messages streaming ──
        // event: content_block_delta / message_delta / content_block_start
        if let Some(et) = v.get("type").and_then(|t| t.as_str()) {
            match et {
                "content_block_start" => {
                    if let Some(block) = v.get("content_block") {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let idx = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            // Use a hash of the id as the key for tool calls
                            let key = idx.bytes().fold(0i64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as i64));
                            tool_calls_map.insert(key, serde_json::json!({
                                "index": key,
                                "id": idx,
                                "type": "function",
                                "function": {"name": block.get("name").and_then(|n| n.as_str()).unwrap_or(""), "arguments": ""}
                            }));
                        }
                    }
                }
                "content_block_delta" => {
                    if let Some(delta) = v.get("delta") {
                        if delta.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                            if let Some(t) = delta.get("text").and_then(|t| t.as_str()) {
                                content.push_str(t);
                            }
                        }
                        if delta.get("type").and_then(|t| t.as_str()) == Some("thinking_delta") {
                            if let Some(t) = delta.get("thinking").and_then(|t| t.as_str()) {
                                reasoning.push_str(t);
                            }
                        }
                        if delta.get("type").and_then(|t| t.as_str()) == Some("input_json_delta") {
                            if let Some(pj) = delta.get("partial_json").and_then(|j| j.as_str()) {
                                // Append to the most recent tool call's arguments
                                if let Some((&_k, _)) = tool_calls_map.last_key_value() {
                                    if let Some(entry) = tool_calls_map.get_mut(&_k) {
                                        let existing = entry.pointer("/function/arguments").and_then(|a| a.as_str()).unwrap_or("").to_string();
                                        entry["function"]["arguments"] = serde_json::json!(format!("{existing}{pj}"));
                                    }
                                }
                            }
                        }
                    }
                }
                "message_start" => {
                    if let Some(r) = v.pointer("/message/role").and_then(|r| r.as_str()) {
                        if role.is_none() {
                            *role = Some(r.to_string());
                        }
                    }
                }
                "message_delta" => {
                    if let Some(fr) = v.pointer("/delta/stop_reason").and_then(|s| s.as_str()) {
                        *finish_reason = Some(fr.to_string());
                    }
                }
                _ => {}
            }
            continue;
        }

        // ── OpenAI Responses API streaming ──
        // Events like response.output_item.done / response.completed
        if let Some(et) = v.get("type").and_then(|t| t.as_str()) {
            if et == "response.output_text.delta" || et == "response.text.delta" {
                if let Some(t) = v.get("delta").and_then(|d| d.as_str()) {
                    content.push_str(t);
                }
            } else if et == "response.completed" || et == "response.done" {
                // Try to extract from the final response object
                if let Some(resp) = v.get("response") {
                    if let Some(output) = resp.get("output").and_then(|o| o.as_array()) {
                        for item in output {
                            if let Some(c) = item.get("content").and_then(|c| c.as_array()) {
                                for part in c {
                                    if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                                        if content.is_empty() {
                                            content.push_str(t);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::codec::{DecodeError, Usage};

    struct Decoder {
        usage: Option<Usage>,
    }

    impl StreamDecoder for Decoder {
        fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, DecodeError> {
            let text = String::from_utf8_lossy(bytes);
            if text.contains("bad") {
                return Err(DecodeError::new("/", "bad upstream event"));
            }
            if text.contains("usage") {
                self.usage = Some(Usage {
                    input_tokens: 2,
                    output_tokens: 3,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                    usage_unknown: false,
                });
            }
            Ok(vec![format!("out:{text}")])
        }

        fn finish(&mut self) -> Result<Vec<String>, DecodeError> {
            Ok(vec!["done".into()])
        }

        fn usage(&self) -> Option<Usage> {
            self.usage
        }
    }

    fn supervisor() -> StreamSupervisor {
        let mut supervisor = StreamSupervisor::new();
        supervisor.begin_connect().unwrap();
        supervisor.on_upstream_headers().unwrap();
        supervisor.on_first_frame_validated().unwrap();
        supervisor
    }

    #[test]
    fn first_record_and_carry_are_decoded_before_commit() {
        let mut pump = StreamPumpCore::new(
            supervisor(),
            Box::new(Decoder { usage: None }),
            b"first".to_vec(),
            b"carry".to_vec(),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(pump.start().unwrap()).unwrap(),
            "out:firstout:carry"
        );
        assert!(pump.committed());
    }

    #[test]
    fn decoder_usage_reaches_the_pump() {
        let mut pump = StreamPumpCore::new(
            supervisor(),
            Box::new(Decoder { usage: None }),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        pump.push(b"usage").unwrap();
        assert_eq!(pump.usage(), (2, 3, 5));
        assert_eq!(pump.finish().unwrap(), b"done");
        assert!(pump.terminated());
    }

    #[test]
    fn first_decoder_failure_stays_precommit() {
        let result = StreamPumpCore::new(
            supervisor(),
            Box::new(Decoder { usage: None }),
            b"bad".to_vec(),
            Vec::new(),
        );
        let error = match result {
            Ok(_) => panic!("bad first event must fail before commit"),
            Err(error) => error,
        };
        assert!(error.message().contains("bad upstream event"));
    }
}
