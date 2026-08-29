use super::super::error::{FeatureKind, UnsupportedFeatures};
use super::super::report::{ConversionContext, Usage};
use super::super::sse;
use super::decode::usage_from_responses;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct ResponsesChatState {
    pending: Vec<u8>,
    id: String,
    model: String,
    created: i64,
    role_emitted: bool,
    tool_calls: BTreeMap<u64, ToolCallState>,
    reasoning: String,
    pub(super) usage: Usage,
    terminal: bool,
}

#[derive(Default)]
struct ToolCallState {
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    final_arguments: Option<String>,
    completed: bool,
}

impl ResponsesChatState {
    pub(super) fn new(context: &ConversionContext) -> Self {
        Self {
            id: context.request_id.clone(),
            model: context.upstream_model.clone(),
            created: chrono::Utc::now().timestamp(),
            ..Self::default()
        }
    }
    fn chunk(&self, delta: Value, finish: Option<&str>) -> String {
        sse::data_frame(
            serde_json::json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":finish}]}),
        )
    }
    fn role(&mut self, output: &mut Vec<String>) {
        if !self.role_emitted {
            self.role_emitted = true;
            output.push(self.chunk(serde_json::json!({"role":"assistant"}), None));
        }
    }
    fn tool_index(&self, event: &Value) -> Option<u64> {
        event
            .get("output_index")
            .and_then(Value::as_u64)
            .or_else(|| {
                let item_id = event
                    .get("item_id")
                    .or_else(|| event.pointer("/item/id"))
                    .and_then(Value::as_str)?;
                self.tool_calls
                    .iter()
                    .find_map(|(index, call)| (call.item_id == item_id).then_some(*index))
            })
    }
    fn merge_tool_item(&mut self, index: u64, item: &Value) {
        let call = self.tool_calls.entry(index).or_default();
        if let Some(item_id) = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            call.item_id = item_id.to_string();
        }
        if let Some(call_id) = item
            .get("call_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            call.call_id = call_id.to_string();
        }
        if let Some(name) = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            call.name = name.to_string();
        }
    }
    fn merge_tool_event_fields(&mut self, index: u64, event: &Value) {
        let call = self.tool_calls.entry(index).or_default();
        if let Some(item_id) = event
            .get("item_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            call.item_id = item_id.to_string();
        }
        if let Some(call_id) = event
            .get("call_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            call.call_id = call_id.to_string();
        }
        if let Some(name) = event
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            call.name = name.to_string();
        }
    }
    fn complete_tool_call(
        &mut self,
        index: u64,
        arguments: &str,
    ) -> Result<Option<(String, String, String)>, UnsupportedFeatures> {
        let parsed: Value = serde_json::from_str(arguments).map_err(|_| {
            UnsupportedFeatures::single(
                FeatureKind::InvalidToolArguments,
                "/arguments",
                "Responses function call arguments are not valid JSON",
            )
        })?;
        if !parsed.is_object() {
            return Err(UnsupportedFeatures::single(
                FeatureKind::InvalidToolArguments,
                "/arguments",
                "Responses function call arguments must be a JSON object",
            ));
        }
        let call = self.tool_calls.entry(index).or_default();
        call.final_arguments = Some(arguments.to_string());
        if call.call_id.is_empty() || call.name.is_empty() {
            return Ok(None);
        }
        let remaining = if call.arguments.is_empty() {
            arguments.to_string()
        } else if arguments == call.arguments {
            String::new()
        } else if let Some(remaining) = arguments.strip_prefix(&call.arguments) {
            remaining.to_string()
        } else {
            return Err(UnsupportedFeatures::single(
                FeatureKind::InvalidToolArguments,
                "/arguments",
                "Responses function call arguments disagree with prior deltas",
            ));
        };
        call.arguments = arguments.to_string();
        call.completed = true;
        Ok((!remaining.is_empty()).then(|| (call.call_id.clone(), call.name.clone(), remaining)))
    }
    pub(super) fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, UnsupportedFeatures> {
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::new();
        while let Some(end) = sse::record_end(&self.pending) {
            let record: Vec<u8> = self.pending.drain(..end).collect();
            output.extend(self.record(&record)?);
        }
        Ok(output)
    }
    fn record(&mut self, record: &[u8]) -> Result<Vec<String>, UnsupportedFeatures> {
        let payload = sse::parse_data_payload(record)?;
        if payload.is_empty() || payload == "[DONE]" {
            return Ok(Vec::new());
        }
        let event: Value = serde_json::from_str(&payload).map_err(|_| {
            UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE data is not JSON",
            )
        })?;
        if event.get("type").and_then(Value::as_str) == Some("codex.rate_limits") {
            return Ok(vec![String::from_utf8_lossy(record).into_owned()]);
        }
        let mut output = Vec::new();
        match event.get("type").and_then(Value::as_str) {
            Some("response.created") | Some("response.in_progress") => {
                self.id = event
                    .pointer("/response/id")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.id)
                    .to_string();
                self.model = event
                    .pointer("/response/model")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.model)
                    .to_string();
                self.role(&mut output);
            }
            Some("response.output_item.added") => {
                self.role(&mut output);
                if event.pointer("/item/type").and_then(Value::as_str) == Some("function_call") {
                    let index = event
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(self.tool_calls.len() as u64);
                    self.merge_tool_item(index, event.get("item").unwrap_or(&Value::Null));
                }
            }
            Some("response.output_text.delta") => {
                self.role(&mut output);
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    output.push(self.chunk(serde_json::json!({"content":delta}), None));
                }
            }
            Some("response.function_call_arguments.delta") => {
                self.role(&mut output);
                let index = self.tool_index(&event).unwrap_or(0);
                self.merge_tool_event_fields(index, &event);
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    let (call_id, name) = {
                        let entry = self.tool_calls.entry(index).or_default();
                        entry.arguments.push_str(delta);
                        (entry.call_id.clone(), entry.name.clone())
                    };
                    output.push(self.chunk(serde_json::json!({"tool_calls":[{"index":index,"id":call_id,"type":"function","function":{"name":name,"arguments":delta}}]}), None));
                }
            }
            Some("response.function_call_arguments.done") => {
                self.role(&mut output);
                let index = self.tool_index(&event).unwrap_or(0);
                self.merge_tool_event_fields(index, &event);
                let arguments =
                    event
                        .get("arguments")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            UnsupportedFeatures::single(
                                FeatureKind::InvalidToolArguments,
                                "/arguments",
                                "Responses function call completion is missing arguments",
                            )
                        })?;
                if let Some((call_id, name, remaining)) =
                    self.complete_tool_call(index, arguments)?
                {
                    output.push(self.chunk(serde_json::json!({"tool_calls":[{"index":index,"id":call_id,"type":"function","function":{"name":name,"arguments":remaining}}]}), None));
                }
            }
            Some("response.output_item.done") => {
                self.role(&mut output);
                let item = event.get("item").unwrap_or(&Value::Null);
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let index = self
                        .tool_index(&event)
                        .unwrap_or(self.tool_calls.len() as u64);
                    self.merge_tool_item(index, item);
                    if self
                        .tool_calls
                        .get(&index)
                        .is_some_and(|call| call.completed)
                    {
                        return Ok(output);
                    }
                    let arguments = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| {
                            self.tool_calls
                                .get(&index)
                                .and_then(|call| call.final_arguments.clone())
                        })
                        .ok_or_else(|| {
                            UnsupportedFeatures::single(
                                FeatureKind::InvalidToolArguments,
                                "/item/arguments",
                                "Responses function call completion is missing arguments",
                            )
                        })?;
                    if let Some((call_id, name, remaining)) =
                        self.complete_tool_call(index, &arguments)?
                    {
                        output.push(self.chunk(serde_json::json!({"tool_calls":[{"index":index,"id":call_id,"type":"function","function":{"name":name,"arguments":remaining}}]}), None));
                    }
                }
            }
            Some("response.reasoning_summary_text.delta") => {
                self.role(&mut output);
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    self.reasoning.push_str(delta);
                    output.push(self.chunk(serde_json::json!({"reasoning_content":delta}), None));
                }
            }
            Some("response.completed") => {
                if self.terminal {
                    return Ok(Vec::new());
                }
                if self.tool_calls.values().any(|call| !call.completed) {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::InvalidToolArguments,
                        "/output",
                        "Responses stream completed with an incomplete function call",
                    ));
                }
                self.role(&mut output);
                self.usage = usage_from_responses(event.get("response").unwrap_or(&event));
                output.push(sse::data_frame(serde_json::json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[],"usage":{"prompt_tokens":self.usage.input_tokens,"completion_tokens":self.usage.output_tokens,"total_tokens":self.usage.input_tokens+self.usage.output_tokens}})));
                output.push(self.chunk(
                    Value::Object(Map::new()),
                    Some(if self.tool_calls.is_empty() {
                        "stop"
                    } else {
                        "tool_calls"
                    }),
                ));
                output.push("data: [DONE]\n\n".to_string());
                self.terminal = true;
            }
            Some("response.failed") | Some("response.incomplete") => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownEvent,
                    "/type",
                    "Responses upstream failed",
                ))
            }
            Some(_) => {}
            None => {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownEvent,
                    "/type",
                    "Responses SSE event missing type",
                ))
            }
        }
        Ok(output)
    }
    pub(super) fn finish(&mut self) -> Result<Vec<String>, UnsupportedFeatures> {
        if !self.pending.is_empty() {
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE ended mid-record",
            ));
        }
        if self.terminal {
            Ok(Vec::new())
        } else {
            Err(UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE ended without terminal event",
            ))
        }
    }
}

/// Derive a Responses-canonical `resp_…` id from the downstream request id.
///
/// The request encoder stamps `chatcmpl_<uuid>` on the context; reusing that
/// suffix keeps the Responses stream traceable to the request while retaining
/// the `resp_` prefix the Responses API expects.  When the streaming V5 path
/// passes an empty request id, fall back to a fresh uuid so every stream gets
/// unique, non-degenerate ids (mirrors the `resp_<uuid>` pattern used by the
/// legacy ResponsesViaChat pump).
pub(super) fn responses_response_id(request_id: &str) -> String {
    match request_id.strip_prefix("chatcmpl_") {
        Some(suffix) if !suffix.is_empty() => format!("resp_{suffix}"),
        _ if request_id.is_empty() => format!("resp_{}", uuid::Uuid::new_v4().simple()),
        _ => format!("resp_{request_id}"),
    }
}
