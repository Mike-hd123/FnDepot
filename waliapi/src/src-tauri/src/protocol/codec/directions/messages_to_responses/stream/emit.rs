use super::super::super::super::error::{FeatureKind, UnsupportedFeatures};
use super::super::bad;
use super::super::decode::usage;
use super::ResponsesMessagesStream;
use serde_json::Value;

impl ResponsesMessagesStream {
    /// Emit the downstream terminal frames for a finished Responses stream.
    ///
    /// `status` is the upstream final status (`completed`/`incomplete`); both
    /// terminal event forms (`response.completed` and the standalone
    /// `response.incomplete`) converge here so truncation maps to a
    /// `max_tokens` stop reason rather than a gateway error.
    pub(super) fn emit_terminal(
        &mut self,
        status: &str,
        response: &Value,
        out: &mut Vec<String>,
    ) -> Result<(), UnsupportedFeatures> {
        if self.terminal {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/type",
                "duplicate terminal Responses event",
            ));
        }
        if self.blocks.values().any(|b| !b.closed) {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/output",
                "response completed with open content block",
            ));
        }
        self.usage = usage(response);
        self.start(out);
        let stop = if self.refused {
            "refusal"
        } else {
            match status {
                "incomplete" => "max_tokens",
                "completed" => {
                    if self.blocks.values().any(|b| b.kind == "function_call") {
                        "tool_use"
                    } else {
                        "end_turn"
                    }
                }
                x => {
                    return Err(bad(
                        FeatureKind::UnknownFinishReason,
                        "/status",
                        format!("unsupported final status {x:?}"),
                    ))
                }
            }
        };
        out.push(Self::frame("message_delta",serde_json::json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},"usage":{"input_tokens":self.usage.input_tokens,"output_tokens":self.usage.output_tokens}})));
        out.push(Self::frame(
            "message_stop",
            serde_json::json!({"type":"message_stop"}),
        ));
        self.terminal = true;
        Ok(())
    }

    /// Map a `content_part.*` event's `part.type` to the target block kind,
    /// canonical part type, stream semantics, and refusal flag.
    ///
    /// `content_part.*` is *not* message-only: the part union also carries
    /// `reasoning_text` (a reasoning item's raw chain-of-thought content) and
    /// `refusal`.  Compatible providers that opt out of reasoning summaries
    /// (e.g. DeepSeek) stream raw CoT through this exact channel, so the
    /// dispatch must follow `part.type`, not the event name.
    pub(super) fn part_dispatch(
        event: &Value,
    ) -> Result<(&'static str, &'static str, bool, bool), UnsupportedFeatures> {
        let part = event.get("part").ok_or_else(|| {
            bad(
                FeatureKind::UnknownEvent,
                "/part",
                "part lifecycle requires part",
            )
        })?;
        match part.get("type").and_then(Value::as_str) {
            Some("output_text") => Ok(("message", "output_text", false, false)),
            Some("reasoning_text") => Ok(("reasoning", "reasoning_text", true, false)),
            Some("refusal") => Ok(("message", "refusal", false, true)),
            Some(x) => Err(bad(
                FeatureKind::UnknownBlock,
                "/part/type",
                format!("unexpected Responses part type {x:?}"),
            )),
            None => Err(bad(
                FeatureKind::UnknownBlock,
                "/part/type",
                "part type is required",
            )),
        }
    }

    pub(super) fn validate_part_lifecycle(
        &self,
        event: &Value,
        expected_kind: &str,
        allowed_part_types: &[&str],
        part_index_field: &str,
    ) -> Result<u64, UnsupportedFeatures> {
        let ix = self
            .output_index_or_infer(event, expected_kind)
            .ok_or_else(|| {
                bad(
                    FeatureKind::UnknownEvent,
                    "/output_index",
                    "part lifecycle requires output_index or one open matching item",
                )
            })?;
        if event.get(part_index_field).and_then(Value::as_u64) != Some(0) {
            return Err(bad(
                FeatureKind::UnknownEvent,
                format!("/{part_index_field}"),
                "only the first textual part is representable by this stream codec",
            ));
        }
        let block = self.blocks.get(&ix).ok_or_else(|| {
            bad(
                FeatureKind::UnknownEvent,
                "/output_index",
                "part lifecycle before output item",
            )
        })?;
        if block.kind != expected_kind {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/output_index",
                "part lifecycle targets the wrong output item type",
            ));
        }
        let part = event.get("part").ok_or_else(|| {
            bad(
                FeatureKind::UnknownEvent,
                "/part",
                "part lifecycle requires part",
            )
        })?;
        let part_type = part.get("type").and_then(Value::as_str).ok_or_else(|| {
            bad(
                FeatureKind::UnknownEvent,
                "/part/type",
                "part type is required",
            )
        })?;
        if !allowed_part_types.contains(&part_type) {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/part/type",
                "unexpected Responses part type",
            ));
        }
        Ok(ix)
    }

    pub(super) fn emit_completed_text(
        &mut self,
        ix: u64,
        expected_kind: &str,
        complete: &str,
        thinking: bool,
        out: &mut Vec<String>,
    ) -> Result<(), UnsupportedFeatures> {
        let block = self.blocks.get_mut(&ix).ok_or_else(|| {
            bad(
                FeatureKind::UnknownEvent,
                "/output_index",
                "text completion before output item",
            )
        })?;
        if block.kind != expected_kind {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/output_index",
                "text completion targets the wrong output item type",
            ));
        }
        let suffix = complete.strip_prefix(&block.text).ok_or_else(|| {
            bad(
                FeatureKind::UnknownEvent,
                "/text",
                "completed text conflicts with prior deltas",
            )
        })?;
        if !suffix.is_empty() {
            block.text.push_str(suffix);
            out.push(Self::frame("content_block_delta", serde_json::json!({
                "type":"content_block_delta", "index":ix,
                "delta": if thinking { serde_json::json!({"type":"thinking_delta", "thinking":suffix}) } else { serde_json::json!({"type":"text_delta", "text":suffix}) }
            })));
        }
        Ok(())
    }
}
