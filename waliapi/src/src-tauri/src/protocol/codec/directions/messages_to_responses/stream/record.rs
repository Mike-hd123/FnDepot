use super::super::super::super::{
    error::{FeatureKind, UnsupportedFeatures},
    sse,
};
use super::super::{bad, required};
use super::{ResponsesMessagesStream, StreamBlock};
use serde_json::Value;

impl ResponsesMessagesStream {
    pub(super) fn record(&mut self, r: &[u8]) -> Result<Vec<String>, UnsupportedFeatures> {
        let p = sse::parse_data_payload(r)?;
        if p.is_empty() || p == "[DONE]" {
            return Ok(Vec::new());
        }
        let e: Value = serde_json::from_str(&p).map_err(|_| {
            bad(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE data is not JSON",
            )
        })?;
        if e.get("type").and_then(Value::as_str) == Some("codex.rate_limits") {
            return Ok(vec![String::from_utf8_lossy(r).into_owned()]);
        }
        let ty = e.get("type").and_then(Value::as_str).ok_or_else(|| {
            bad(
                FeatureKind::UnknownEvent,
                "/type",
                "Responses SSE type is required",
            )
        })?;
        let mut out = Vec::new();
        match ty {
            "response.created" | "response.in_progress" => {
                if let Some(x) = e.get("response") {
                    self.id = x
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or(&self.id)
                        .into();
                    self.model = x
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or(&self.model)
                        .into()
                }
                self.start(&mut out)
            }
            "response.output_item.added" => {
                self.start(&mut out);
                let ix = Self::event_output_index(&e).unwrap_or(0);
                let item = e.get("item").unwrap_or(&Value::Null);
                let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
                let mut b = StreamBlock {
                    kind: kind.into(),
                    ..Default::default()
                };
                if kind == "function_call" {
                    b.id = required(item, "call_id", "/item")?.into();
                    b.name = required(item, "name", "/item")?.into();
                }
                self.blocks.insert(ix, b);
                let block = match kind {
                    "message" => serde_json::json!({"type":"text","text":""}),
                    "reasoning" => serde_json::json!({"type":"thinking","thinking":""}),
                    "function_call" => {
                        serde_json::json!({"type":"tool_use","id":self.blocks[&ix].id,"name":self.blocks[&ix].name,"input":{}})
                    }
                    _ => {
                        return Err(bad(
                            FeatureKind::UnknownEvent,
                            "/item/type",
                            "unsupported Responses output item",
                        ))
                    }
                };
                out.push(Self::frame("content_block_start",serde_json::json!({"type":"content_block_start","index":ix,"content_block":block})))
            }
            "response.output_text.delta" => {
                self.start(&mut out);
                let ix = self.output_index_or_infer(&e, "message").ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "text delta requires output_index or one open message item",
                    )
                })?;
                let delta = e.get("delta").and_then(Value::as_str).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/delta",
                        "output text delta is required",
                    )
                })?;
                let block = self.blocks.get_mut(&ix).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "text delta before message item",
                    )
                })?;
                if block.kind != "message" {
                    return Err(bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "output text delta targets a non-message item",
                    ));
                }
                block.text.push_str(delta);
                out.push(Self::frame("content_block_delta",serde_json::json!({"type":"content_block_delta","index":ix,"delta":{"type":"text_delta","text":delta}})))
            }
            "response.reasoning_summary_text.delta" => {
                self.start(&mut out);
                let ix = self.output_index_or_infer(&e, "reasoning").ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "reasoning delta requires output_index or one open reasoning item",
                    )
                })?;
                let delta = e.get("delta").and_then(Value::as_str).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/delta",
                        "reasoning summary delta is required",
                    )
                })?;
                let block = self.blocks.get_mut(&ix).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "reasoning delta before reasoning item",
                    )
                })?;
                if block.kind != "reasoning" {
                    return Err(bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "reasoning delta targets a non-reasoning item",
                    ));
                }
                block.text.push_str(delta);
                out.push(Self::frame("content_block_delta",serde_json::json!({"type":"content_block_delta","index":ix,"delta":{"type":"thinking_delta","thinking":delta}})))
            }
            // These are standard Responses lifecycle records.  Deltas carry
            // the incremental payload, while the `*.done` records verify (and
            // backfill for compatible backends) the final complete text.
            "response.content_part.added" => {
                self.start(&mut out);
                let (expected_kind, expected_part, ..) = Self::part_dispatch(&e)?;
                self.validate_part_lifecycle(
                    &e,
                    &expected_kind,
                    &[expected_part],
                    "content_index",
                )?;
            }
            "response.content_part.done" => {
                let (expected_kind, expected_part, thinking, refusal) = Self::part_dispatch(&e)?;
                let ix = self.validate_part_lifecycle(
                    &e,
                    &expected_kind,
                    &[expected_part],
                    "content_index",
                )?;
                let text_field = if refusal {
                    "/part/refusal"
                } else {
                    "/part/text"
                };
                let text = e
                    .pointer(text_field)
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        bad(
                            FeatureKind::UnknownEvent,
                            text_field,
                            if refusal {
                                "completed refusal text is required"
                            } else {
                                "completed output text is required"
                            },
                        )
                    })?;
                if refusal {
                    self.refused = true;
                }
                self.emit_completed_text(ix, &expected_kind, text, thinking, &mut out)?;
            }
            "response.output_text.done" => {
                let ix = self.output_index_for_block(&e, "message")?;
                let text = e.get("text").and_then(Value::as_str).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/text",
                        "completed output text is required",
                    )
                })?;
                self.emit_completed_text(ix, "message", text, false, &mut out)?;
            }
            "response.reasoning_summary_part.added" => {
                // The standard summary part type is `summary_text`;
                // `reasoning_summary_text` is retained for older compatible
                // providers that predate the canonical name.
                self.validate_part_lifecycle(
                    &e,
                    "reasoning",
                    &["summary_text", "reasoning_summary_text"],
                    "summary_index",
                )?;
            }
            "response.reasoning_summary_part.done" => {
                let ix = self.validate_part_lifecycle(
                    &e,
                    "reasoning",
                    &["summary_text", "reasoning_summary_text"],
                    "summary_index",
                )?;
                let text = e
                    .pointer("/part/text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        bad(
                            FeatureKind::UnknownEvent,
                            "/part/text",
                            "completed reasoning text is required",
                        )
                    })?;
                self.emit_completed_text(ix, "reasoning", text, true, &mut out)?;
            }
            "response.reasoning_summary_text.done" => {
                let ix = self.output_index_for_block(&e, "reasoning")?;
                let text = e.get("text").and_then(Value::as_str).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/text",
                        "completed reasoning text is required",
                    )
                })?;
                self.emit_completed_text(ix, "reasoning", text, true, &mut out)?;
            }
            // Raw chain-of-thought text (the reasoning item's `content`
            // array).  Compatible providers that opt out of reasoning
            // summaries stream this through `reasoning_text.delta/done`
            // indexed by `content_index`, plus `content_part.added/done`
            // with `part.type = "reasoning_text"`.
            "response.reasoning_text.delta" => {
                self.start(&mut out);
                let ix = self.output_index_or_infer(&e, "reasoning").ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "reasoning text delta requires output_index or one open reasoning item",
                    )
                })?;
                let delta = e.get("delta").and_then(Value::as_str).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/delta",
                        "reasoning text delta is required",
                    )
                })?;
                let block = self.blocks.get_mut(&ix).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "reasoning text delta before reasoning item",
                    )
                })?;
                if block.kind != "reasoning" {
                    return Err(bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "reasoning text delta targets a non-reasoning item",
                    ));
                }
                block.text.push_str(delta);
                out.push(Self::frame("content_block_delta",serde_json::json!({"type":"content_block_delta","index":ix,"delta":{"type":"thinking_delta","thinking":delta}})))
            }
            "response.reasoning_text.done" => {
                let ix = self.output_index_for_block(&e, "reasoning")?;
                let text = e.get("text").and_then(Value::as_str).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/text",
                        "completed reasoning text is required",
                    )
                })?;
                self.emit_completed_text(ix, "reasoning", text, true, &mut out)?;
            }
            "response.function_call_arguments.delta" => {
                let ix = self
                    .output_index_or_infer(&e, "function_call")
                    .ok_or_else(|| {
                        bad(
                            FeatureKind::UnknownEvent,
                            "/output_index",
                            "argument delta requires output_index or one open function item",
                        )
                    })?;
                let delta = e.get("delta").and_then(Value::as_str).unwrap_or("");
                let b = self.blocks.get_mut(&ix).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "argument delta before function item",
                    )
                })?;
                b.args.push_str(delta);
                out.push(Self::frame("content_block_delta",serde_json::json!({"type":"content_block_delta","index":ix,"delta":{"type":"input_json_delta","partial_json":delta}})))
            }
            "response.function_call_arguments.done" => {
                let ix = Self::event_output_index(&e).unwrap_or(0);
                let a = e.get("arguments").and_then(Value::as_str).ok_or_else(|| {
                    bad(
                        FeatureKind::InvalidToolArguments,
                        "/arguments",
                        "arguments are required",
                    )
                })?;
                let p: Value = serde_json::from_str(a).map_err(|_| {
                    bad(
                        FeatureKind::InvalidToolArguments,
                        "/arguments",
                        "arguments must be valid JSON",
                    )
                })?;
                if !p.is_object() {
                    return Err(bad(
                        FeatureKind::InvalidToolArguments,
                        "/arguments",
                        "arguments must be an object",
                    ));
                }
                if let Some(b) = self.blocks.get_mut(&ix) {
                    b.args = a.into();
                }
            }
            "response.output_item.done" => {
                // Keep terminal item indexing consistent with all prior
                // lifecycle frames. Some OpenAI-compatible Responses servers
                // serialize this final index as a numeric string.
                let ix = self.output_index_for_item_done(&e)?;
                // Some compatible Responses backends emit only the terminal
                // item record.  It is still a complete source item, so create
                // the corresponding target block instead of treating this as
                // a malformed ordering.  This is not an invented tool call:
                // id, name and validated arguments all come from `item`.
                if !self.blocks.contains_key(&ix) {
                    self.start(&mut out);
                    let item = e.get("item").unwrap_or(&Value::Null);
                    let kind = item.get("type").and_then(Value::as_str).ok_or_else(|| {
                        bad(
                            FeatureKind::UnknownEvent,
                            "/item/type",
                            "completed item type is required",
                        )
                    })?;
                    let mut synthesized = StreamBlock {
                        kind: kind.into(),
                        ..Default::default()
                    };
                    let content_block = match kind {
                        "message" => serde_json::json!({"type":"text","text":""}),
                        "reasoning" => serde_json::json!({"type":"thinking","thinking":""}),
                        "function_call" => {
                            synthesized.id = required(item, "call_id", "/item")?.into();
                            synthesized.name = required(item, "name", "/item")?.into();
                            let arguments = item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    bad(
                                        FeatureKind::InvalidToolArguments,
                                        "/item/arguments",
                                        "function arguments are required",
                                    )
                                })?;
                            let parsed: Value = serde_json::from_str(arguments).map_err(|_| {
                                bad(
                                    FeatureKind::InvalidToolArguments,
                                    "/item/arguments",
                                    "function arguments must be valid JSON",
                                )
                            })?;
                            if !parsed.is_object() {
                                return Err(bad(
                                    FeatureKind::InvalidToolArguments,
                                    "/item/arguments",
                                    "function arguments must be an object",
                                ));
                            }
                            synthesized.args = arguments.into();
                            serde_json::json!({"type":"tool_use","id":synthesized.id,"name":synthesized.name,"input":{}})
                        }
                        _ => {
                            return Err(bad(
                                FeatureKind::UnknownEvent,
                                "/item/type",
                                "unsupported completed output item",
                            ))
                        }
                    };
                    self.blocks.insert(ix, synthesized);
                    out.push(Self::frame("content_block_start", serde_json::json!({"type":"content_block_start","index":ix,"content_block":content_block})));
                    if kind == "function_call" {
                        let arguments = self.blocks[&ix].args.clone();
                        out.push(Self::frame("content_block_delta", serde_json::json!({"type":"content_block_delta","index":ix,"delta":{"type":"input_json_delta","partial_json":arguments}})));
                    }
                }
                let b = self.blocks.get_mut(&ix).ok_or_else(|| {
                    bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "item done without item start",
                    )
                })?;
                if b.closed {
                    return Err(bad(
                        FeatureKind::UnknownEvent,
                        "/output_index",
                        "duplicate output item completion",
                    ));
                }
                if b.kind == "function_call" && !b.args.is_empty() {
                    let p: Value = serde_json::from_str(&b.args).map_err(|_| {
                        bad(
                            FeatureKind::InvalidToolArguments,
                            "/arguments",
                            "arguments must be valid JSON",
                        )
                    })?;
                    if !p.is_object() {
                        return Err(bad(
                            FeatureKind::InvalidToolArguments,
                            "/arguments",
                            "arguments must be object",
                        ));
                    }
                }
                b.closed = true;
                out.push(Self::frame(
                    "content_block_stop",
                    serde_json::json!({"type":"content_block_stop","index":ix}),
                ))
            }
            "response.completed" => {
                let response = e.get("response").unwrap_or(&e);
                let status = response
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                self.emit_terminal(status, response, &mut out)?;
            }
            // A standalone `response.incomplete` event terminates a stream
            // truncated at max_output_tokens (DeepSeek's documented terminal
            // for truncation).  Unlike `response.failed` this is a normal
            // completion with a `max_tokens` stop reason, not an error.
            "response.incomplete" => {
                let response = e.get("response").unwrap_or(&e);
                self.emit_terminal("incomplete", response, &mut out)?;
            }
            "response.failed" => {
                return Err(bad(
                    FeatureKind::UnknownEvent,
                    "/type",
                    "Responses upstream reported failure",
                ))
            }
            _ => {
                return Err(bad(
                    FeatureKind::UnknownEvent,
                    "/type",
                    format!("unknown Responses SSE event {ty:?}"),
                ))
            }
        }
        Ok(out)
    }
}
