use super::super::error::{DecodeError, FeatureKind, UnsupportedFeatures};
use super::super::messages;
use super::super::ports::StreamDecoder;
use super::super::report::{ConversionContext, Usage};
use super::super::sse;
use super::state::{responses_response_id, ResponsesChatState};
use serde_json::Value;

pub struct ResponsesStreamDecoder {
    pub(super) state: ResponsesChatState,
}
impl ResponsesStreamDecoder {
    pub fn boxed(context: &ConversionContext) -> Box<dyn StreamDecoder + Send + Sync> {
        Box::new(Self {
            state: ResponsesChatState::new(context),
        })
    }
}
impl StreamDecoder for ResponsesStreamDecoder {
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

/// Composition-only streaming decoder: Responses → Chat SSE then the existing
/// Chat → Messages state machine.  There is intentionally no second direct
/// Responses → Messages protocol machine.
///
/// Exercised only by the unit tests below; direction strategies wire the direct
/// decoders, so non-test builds flag it as dead.
#[cfg_attr(not(test), allow(dead_code))]
pub struct ResponsesMessagesStreamDecoder {
    pub(super) chat: ResponsesStreamDecoder,
    pub(super) messages: Box<dyn StreamDecoder + Send + Sync>,
}
impl StreamDecoder for ResponsesMessagesStreamDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, DecodeError> {
        let events = self.chat.feed(bytes)?;
        let mut output = Vec::new();
        for event in events {
            if event.contains("codex.rate_limits") {
                output.push(event);
            } else {
                output.extend(self.messages.feed(event.as_bytes())?);
            }
        }
        Ok(output)
    }
    fn finish(&mut self) -> Result<Vec<String>, DecodeError> {
        let events = self.chat.finish()?;
        let mut output = Vec::new();
        for event in events {
            if event.contains("codex.rate_limits") {
                output.push(event);
            } else {
                output.extend(self.messages.feed(event.as_bytes())?);
            }
        }
        output.extend(self.messages.finish()?);
        Ok(output)
    }
    fn usage(&self) -> Option<Usage> {
        self.chat.usage()
    }
}

// ===========================================================================
// 路径① response direction: Messages → Responses.
// ===========================================================================

/// Chat SSE → Responses SSE streaming decoder.
///
/// Wraps `responses::convert_openai_sse_to_responses` (which owns the
/// per-stream item state) plus the byte-framed record buffer needed to survive
/// TCP splits, mirroring `encode_responses_buffered` in the legacy
/// ResponsesViaChat pump.  Emits `response.created`/`response.in_progress`
/// once before the first converted record; forwards `codex.rate_limits`
/// records verbatim; and at `finish()` synthesizes `response.completed` +
/// `[DONE]` via `create_synthetic_completed_events`.  An upstream Chat stream
/// that ends mid-record or without a terminal `finish_reason` fails closed so
/// the gateway can fail over before committing the downstream response.
pub struct ChatToResponsesStreamDecoder {
    pending: Vec<u8>,
    state: crate::protocol::responses::StreamState,
    model: String,
    response_id: String,
    accumulated_content: String,
    usage: Usage,
    started: bool,
    terminal_seen: bool,
    done: bool,
}

impl ChatToResponsesStreamDecoder {
    pub fn new(context: &ConversionContext) -> Self {
        Self {
            pending: Vec::new(),
            state: crate::protocol::responses::StreamState::default(),
            model: context.upstream_model.clone(),
            response_id: responses_response_id(&context.request_id),
            accumulated_content: String::new(),
            usage: Usage {
                usage_unknown: true,
                ..Usage::default()
            },
            started: false,
            terminal_seen: false,
            done: false,
        }
    }
    pub fn boxed(context: &ConversionContext) -> Box<dyn StreamDecoder + Send + Sync> {
        Box::new(Self::new(context))
    }
    /// Emit the Responses preamble exactly once, before the first record.
    fn ensure_created(&mut self, output: &mut Vec<String>) {
        if !self.started {
            self.started = true;
            output.push(crate::protocol::responses::create_response_created_event(
                &self.model,
                &self.response_id,
            ));
        }
    }
    /// Convert one complete Chat SSE record into Responses SSE events.
    fn record(&mut self, record: &[u8]) -> Result<Vec<String>, UnsupportedFeatures> {
        let payload = sse::parse_data_payload(record)?;
        if payload.is_empty() || payload == "[DONE]" {
            return Ok(Vec::new());
        }
        let json: Value = serde_json::from_str(&payload).map_err(|_| {
            UnsupportedFeatures::single(FeatureKind::UnknownEvent, "/", "Chat SSE data is not JSON")
        })?;
        // `codex.rate_limits` has no Chat/Responses representation — forward the
        // raw record so the downstream client still observes the quota signal.
        // Forward-compatible defensive code: a standard Anthropic upstream never
        // emits this OpenAI-specific event, and in the real Messages→Responses
        // composition `MessagesSseState` rejects it before this branch could fire
        // (only the direct unit tests below exercise it).
        if json.get("type").and_then(Value::as_str) == Some("codex.rate_limits") {
            return Ok(vec![String::from_utf8_lossy(record).into_owned()]);
        }
        self.accumulate(&json);
        let text = String::from_utf8_lossy(record);
        Ok(crate::protocol::responses::convert_openai_sse_to_responses(
            &text,
            &self.model,
            &self.response_id,
            &self.accumulated_content,
            &mut self.state,
        ))
    }
    /// Accumulate text / usage / finish-reason observables from a Chat SSE
    /// record (mirrors `encode_responses_chunk`'s accumulation).
    fn accumulate(&mut self, json: &Value) {
        if let Some(content) = json
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            self.accumulated_content.push_str(content);
        }
        if let Some(usage) = json.get("usage") {
            if let Some(prompt) = usage.get("prompt_tokens").and_then(Value::as_u64) {
                self.usage.input_tokens = prompt;
            }
            if let Some(completion) = usage.get("completion_tokens").and_then(Value::as_u64) {
                self.usage.output_tokens = completion;
            }
            self.usage.usage_unknown = false;
        }
        if json
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            .is_some_and(|finish| !finish.is_empty())
        {
            self.terminal_seen = true;
        }
    }
}

impl StreamDecoder for ChatToResponsesStreamDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, DecodeError> {
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::new();
        while let Some(end) = sse::record_end(&self.pending) {
            self.ensure_created(&mut output);
            let record: Vec<u8> = self.pending.drain(..end).collect();
            output.extend(self.record(&record).map_err(DecodeError::from)?);
        }
        Ok(output)
    }
    fn finish(&mut self) -> Result<Vec<String>, DecodeError> {
        let mut output = Vec::new();
        self.ensure_created(&mut output);
        if !self.pending.is_empty() {
            return Err(DecodeError::from(UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Chat SSE ended mid-record",
            )));
        }
        if !self.terminal_seen {
            return Err(DecodeError::from(UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Chat SSE ended without a terminal finish_reason",
            )));
        }
        if self.done {
            return Ok(Vec::new());
        }
        self.done = true;
        output.extend(
            crate::protocol::responses::create_synthetic_completed_events(
                &self.model,
                &self.response_id,
                &self.accumulated_content,
                &self.state,
                self.usage.input_tokens as i64,
                self.usage.output_tokens as i64,
            ),
        );
        output.push("data: [DONE]\n\n".to_string());
        Ok(output)
    }
    fn usage(&self) -> Option<Usage> {
        Some(self.usage)
    }
}

/// Composition-only streaming decoder: Messages SSE → Chat SSE then the new
/// Chat SSE → Responses SSE machine (mirror of `ResponsesMessagesStreamDecoder`
/// in reverse).  There is intentionally no second direct Messages → Responses
/// protocol machine.
///
/// The `codex.rate_limits` passthroughs in `feed`/`finish` are forward-compatible
/// defensive code: a standard Anthropic upstream never emits this OpenAI-specific
/// event, and `MessagesSseState` rejects it before the Messages leg could forward
/// it here.
pub struct MessagesResponsesStreamDecoder {
    messages: Box<dyn StreamDecoder + Send + Sync>,
    chat: ChatToResponsesStreamDecoder,
}
impl MessagesResponsesStreamDecoder {
    pub fn boxed(context: &ConversionContext) -> Box<dyn StreamDecoder + Send + Sync> {
        Box::new(Self {
            messages: messages::MessagesStreamDecoder::boxed(context),
            chat: ChatToResponsesStreamDecoder::new(context),
        })
    }
}
impl StreamDecoder for MessagesResponsesStreamDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, DecodeError> {
        let events = self.messages.feed(bytes)?;
        let mut output = Vec::new();
        for event in events {
            if event.contains("codex.rate_limits") {
                output.push(event);
            } else {
                output.extend(self.chat.feed(event.as_bytes())?);
            }
        }
        Ok(output)
    }
    fn finish(&mut self) -> Result<Vec<String>, DecodeError> {
        let events = self.messages.finish()?;
        let mut output = Vec::new();
        for event in events {
            if event.contains("codex.rate_limits") {
                output.push(event);
            } else {
                output.extend(self.chat.feed(event.as_bytes())?);
            }
        }
        output.extend(self.chat.finish()?);
        Ok(output)
    }
    fn usage(&self) -> Option<Usage> {
        self.messages.usage()
    }
}
