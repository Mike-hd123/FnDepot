//! Streaming SSE bridge: normalize any upstream protocol into OpenAI Chat SSE
//! records so downstream handlers (Responses / Chat) share one pipeline.
//!
//! - OpenAI-compatible upstreams (`openai` / `deepseek` / `custom`) already
//!   speak OpenAI SSE; we only reassemble fragmented records.
//! - Anthropic upstreams (`claude` channels, `protocol = anthropic`) speak
//!   Anthropic SSE; we convert via [`MessagesSseState`] (Anthropic SSE →
//!   OpenAI SSE), which already handles record reassembly, tool calls,
//!   reasoning deltas, usage, and the exactly-once final sequence.
//!
//! Callers feed raw upstream chunks and receive complete OpenAI
//! `data: {json}\n\n` records.  A channel declared Anthropic that actually
//! emits OpenAI bytes (or vice versa) surfaces as an `Err` on `push`/`finish`
//! so the gateway fails visibly instead of silently dropping content.

use crate::protocol::codec::messages::MessagesSseState;
use crate::protocol::responses::ResponsesSseAssembler;

/// Whether an upstream channel speaks Anthropic SSE (requires conversion).
///
/// `channel_type == "claude"` is the historical signal; the `protocol`
/// identity column (T02) is authoritative when present.
pub fn is_anthropic_upstream(channel_type: &str, protocol: Option<&str>) -> bool {
    channel_type.eq_ignore_ascii_case("claude")
        || protocol
            .map(|p| p.eq_ignore_ascii_case("anthropic"))
            .unwrap_or(false)
}

/// Bridge producing complete OpenAI `data: {json}\n\n` records from raw
/// upstream bytes, independent of the upstream protocol.
pub enum UpstreamSseBridge {
    /// OpenAI-compatible upstream: reassemble records as-is.
    OpenAi(ResponsesSseAssembler),
    /// Anthropic upstream: convert Anthropic SSE → OpenAI SSE records.
    Anthropic(MessagesSseState),
}

impl UpstreamSseBridge {
    /// Create the bridge for an upstream.  `model` is used only on the
    /// Anthropic path, for the synthesized Chat `role` frame.
    pub fn for_upstream(is_anthropic: bool, model: &str) -> Self {
        if is_anthropic {
            Self::Anthropic(MessagesSseState::new(model))
        } else {
            Self::OpenAi(ResponsesSseAssembler::new())
        }
    }

    /// Feed one upstream chunk; returns every complete OpenAI
    /// `data: {json}\n\n` record produced so far.
    ///
    /// Takes raw bytes, not `&str`: a TCP/HTTP chunk boundary can fall in the
    /// middle of a UTF-8 codepoint (very common with 3-byte CJK text), so the
    /// caller must not gate on `str::from_utf8`.  Both variants buffer bytes
    /// internally and only decode COMPLETE records, so a mid-codepoint split is
    /// reassembled across calls — never dropped and never escaped raw.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, String> {
        match self {
            Self::OpenAi(assembler) => Ok(assembler.push(chunk)),
            Self::Anthropic(state) => state.feed(chunk).map_err(|e| e.to_string()),
        }
    }

    /// Flush end-of-stream.  For OpenAI upstreams this is a trailing-EOF
    /// record; for Anthropic it is the exactly-once final sequence
    /// (finish_reason + usage in the last `choices` frame, then `[DONE]`).
    /// The usage is merged into the finish_reason frame (not a bare usage-only
    /// frame) because OpenAI-compat clients like Opencode reject chunks that
    /// lack `choices`.
    pub fn finish(&mut self) -> Result<Vec<String>, String> {
        match self {
            Self::OpenAi(assembler) => Ok(assembler.flush()),
            Self::Anthropic(state) => {
                let records = state.finish().map_err(|e| e.to_string())?;
                // MessagesSseState emits the terminator as `data: "[DONE]"`
                // (serde-quoted); OpenAI clients only recognize the unquoted
                // `data: [DONE]`, so normalize before handing downstream.
                Ok(records.into_iter().map(normalize_done).collect())
            }
        }
    }
}

/// Normalize a trailing `[DONE]` record to the OpenAI-canonical unquoted form.
fn normalize_done(record: String) -> String {
    let body = record.trim_end_matches('\n').trim_end_matches('\r');
    if let Some(data) = body.strip_prefix("data:") {
        let payload = data.trim();
        if payload == "[DONE]" || payload == "\"[DONE]\"" {
            return "data: [DONE]\n\n".to_string();
        }
    }
    record
}

#[cfg(test)]
#[path = "sse_bridge_tests.rs"]
mod sse_bridge_tests;
