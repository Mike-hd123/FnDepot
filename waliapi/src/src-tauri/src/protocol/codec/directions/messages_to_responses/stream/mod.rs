use super::super::super::{
    error::{DecodeError, FeatureKind, UnsupportedFeatures},
    ports::StreamDecoder,
    report::{ConversionContext, Usage},
    sse,
};
use super::bad;
use serde_json::Value;
use std::collections::BTreeMap;

mod emit;
mod record;

pub(super) struct ResponsesMessagesStream {
    pending: Vec<u8>,
    id: String,
    model: String,
    started: bool,
    terminal: bool,
    usage: Usage,
    blocks: BTreeMap<u64, StreamBlock>,
    /// A `refusal` part was seen; the stream terminates with a refusal stop.
    refused: bool,
}
#[derive(Default)]
struct StreamBlock {
    kind: String,
    id: String,
    name: String,
    text: String,
    args: String,
    closed: bool,
}
impl ResponsesMessagesStream {
    pub(super) fn new(c: &ConversionContext) -> Self {
        Self {
            pending: Vec::new(),
            id: c.request_id.clone(),
            model: c.upstream_model.clone(),
            started: false,
            terminal: false,
            usage: Usage {
                usage_unknown: true,
                ..Usage::default()
            },
            blocks: BTreeMap::new(),
            refused: false,
        }
    }
    fn frame(t: &str, v: Value) -> String {
        sse::event(t, v)
    }
    fn start(&mut self, out: &mut Vec<String>) {
        if !self.started {
            self.started = true;
            out.push(Self::frame("message_start",serde_json::json!({"type":"message_start","message":{"id":self.id,"type":"message","role":"assistant","model":self.model,"content":[],"usage":{"input_tokens":0,"output_tokens":0}}})))
        }
    }

    /// Some OpenAI-compatible Responses backends serialize `output_index` as a
    /// JSON string, while others omit it from a terminal `*.done` frame after
    /// having identified the item in earlier lifecycle frames.
    fn event_output_index(event: &Value) -> Option<u64> {
        event.get("output_index").and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        })
    }

    /// Resolve an event to an item index.  A present, parseable `output_index`
    /// is authoritative; when omitted, the frame is mapped to the single open
    /// block of the declared kind — the same inference the terminal `*.done`
    /// frames already use.  `None` means the index is absent AND the target is
    /// ambiguous or has no open matching block, so the caller must fail closed.
    fn output_index_or_infer(&self, event: &Value, expected_kind: &str) -> Option<u64> {
        if let Some(index) = Self::event_output_index(event) {
            return Some(index);
        }
        let mut matching = self.blocks.iter().filter_map(|(index, block)| {
            (block.kind == expected_kind && !block.closed).then_some(*index)
        });
        let first = matching.next()?;
        matching.next().is_none().then_some(first)
    }

    /// Resolve a terminal text/reasoning frame to its open block.  Omitting an
    /// index is unambiguous only when exactly one matching block is open.
    fn output_index_for_block(
        &self,
        event: &Value,
        expected_kind: &str,
    ) -> Result<u64, UnsupportedFeatures> {
        if let Some(index) = Self::event_output_index(event) {
            return Ok(index);
        }
        let mut matching = self.blocks.iter().filter_map(|(index, block)| {
            (block.kind == expected_kind && !block.closed).then_some(*index)
        });
        let Some(index) = matching.next() else {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/output_index",
                "completion requires output_index or an open matching output item",
            ));
        };
        if matching.next().is_some() {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/output_index",
                "completion without output_index is ambiguous",
            ));
        }
        Ok(index)
    }

    /// Resolve an output-item completion. Compatible Responses providers may
    /// omit `output_index` on the terminal item frame; it is safe to infer only
    /// when one unclosed block has the type declared by `item.type`.
    fn output_index_for_item_done(&self, event: &Value) -> Result<u64, UnsupportedFeatures> {
        if let Some(index) = Self::event_output_index(event) {
            return Ok(index);
        }
        let expected_kind = event
            .pointer("/item/type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                bad(
                    FeatureKind::UnknownEvent,
                    "/output_index",
                    "item completion requires output_index or item.type",
                )
            })?;
        let mut matching = self.blocks.iter().filter_map(|(index, block)| {
            (block.kind == expected_kind && !block.closed).then_some(*index)
        });
        let Some(index) = matching.next() else {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/output_index",
                "item completion requires output_index or an open matching output item",
            ));
        };
        if matching.next().is_some() {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/output_index",
                "item completion without output_index is ambiguous",
            ));
        }
        Ok(index)
    }
}
impl StreamDecoder for ResponsesMessagesStream {
    fn feed(&mut self, b: &[u8]) -> Result<Vec<String>, DecodeError> {
        self.pending.extend_from_slice(b);
        let mut out = Vec::new();
        while let Some(end) = sse::record_end(&self.pending) {
            let r: Vec<u8> = self.pending.drain(..end).collect();
            out.extend(self.record(&r).map_err(DecodeError::from)?)
        }
        Ok(out)
    }
    fn finish(&mut self) -> Result<Vec<String>, DecodeError> {
        if !self.pending.is_empty() {
            return Err(DecodeError::from(bad(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE ended mid-record",
            )));
        }
        if !self.terminal {
            return Err(DecodeError::from(bad(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE ended without response.completed",
            )));
        }
        Ok(Vec::new())
    }
    fn usage(&self) -> Option<Usage> {
        Some(self.usage)
    }
}
