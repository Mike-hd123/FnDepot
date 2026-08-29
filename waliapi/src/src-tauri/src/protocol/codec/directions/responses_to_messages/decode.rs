use super::super::super::{
    error::{DecodeError, FeatureKind, UnsupportedFeatures},
    ports::{DecodedResponse, NonStreamDecoder},
    report::{ConversionContext, Usage},
};
use super::{required, unsupported};
use serde_json::Value;

pub(super) struct MessagesResponseDecoder {
    pub(super) context: ConversionContext,
}
impl NonStreamDecoder for MessagesResponseDecoder {
    fn decode(&self, body: &Value) -> Result<DecodedResponse, DecodeError> {
        decode_messages_response(body, &self.context).map_err(DecodeError::from)
    }
}

pub fn decode_messages_response(
    body: &Value,
    context: &ConversionContext,
) -> Result<DecodedResponse, UnsupportedFeatures> {
    if body.get("type").and_then(Value::as_str) != Some("message") {
        return Err(unsupported(
            FeatureKind::UnknownEvent,
            "/type",
            "Messages response must have type=message",
        ));
    }
    let content = body
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            unsupported(
                FeatureKind::UnknownEvent,
                "/content",
                "Messages response requires content array",
            )
        })?;
    let mut output = Vec::new();
    for (i, block) in content.iter().enumerate() {
        let p = format!("/content/{i}");
        match block.get("type").and_then(Value::as_str) { Some("text") => output.push(serde_json::json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":block.get("text").and_then(Value::as_str).unwrap_or("")}]})), Some("thinking") => { if let Some(text)=block.get("thinking").and_then(Value::as_str) { output.push(serde_json::json!({"type":"reasoning","summary":[{"type":"summary_text","text":text}]})); } }, Some("redacted_thinking") => {}, Some("tool_use") => { let id=required(block,"id",&p)?; let name=required(block,"name",&p)?; let input=block.get("input").ok_or_else(|| unsupported(FeatureKind::MissingToolField,format!("{p}/input"),"tool input is required"))?; if !input.is_object() { return Err(unsupported(FeatureKind::InvalidToolArguments,format!("{p}/input"),"tool input must be an object")); } output.push(serde_json::json!({"type":"function_call","call_id":id,"name":name,"arguments":serde_json::to_string(input).map_err(|_| unsupported(FeatureKind::InvalidToolArguments,format!("{p}/input"),"tool input could not be serialized"))?})); }, Some(other)=>return Err(unsupported(FeatureKind::UnknownBlock,format!("{p}/type"),format!("Messages response block {other:?} is unsupported"))), None=>return Err(unsupported(FeatureKind::UnknownBlock,format!("{p}/type"),"content block type is required")) }
    }
    let usage = usage_from_messages(body);
    let status = match body.get("stop_reason").and_then(Value::as_str) {
        Some("end_turn" | "stop_sequence" | "refusal" | "pause_turn") => "completed",
        Some("tool_use") => "completed",
        Some("max_tokens" | "model_context_window_exceeded") => "incomplete",
        Some(other) => {
            return Err(unsupported(
                FeatureKind::UnknownFinishReason,
                "/stop_reason",
                format!("unknown Messages stop reason {other:?}"),
            ))
        }
        None => {
            return Err(unsupported(
                FeatureKind::UnknownFinishReason,
                "/stop_reason",
                "Messages response is missing stop_reason",
            ))
        }
    };
    let mut response = serde_json::json!({"id":body.get("id").and_then(Value::as_str).unwrap_or(&context.request_id),"object":"response","model":body.get("model").and_then(Value::as_str).unwrap_or(&context.upstream_model),"status":status,"output":output,"usage":{"input_tokens":usage.input_tokens,"output_tokens":usage.output_tokens,"total_tokens":usage.input_tokens+usage.output_tokens}});
    if status == "incomplete" {
        response["incomplete_details"] = serde_json::json!({"reason":"max_output_tokens"});
    }
    Ok(DecodedResponse {
        body: response,
        usage: Some(usage),
    })
}
fn usage_from_messages(value: &Value) -> Usage {
    let input = value.pointer("/usage/input_tokens").and_then(Value::as_u64);
    let output = value
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64);
    Usage {
        input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        cache_creation_input_tokens: value
            .pointer("/usage/cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_input_tokens: value
            .pointer("/usage/cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        usage_unknown: input.is_none() || output.is_none(),
    }
}

pub(super) fn merge_usage(usage: &mut Usage, value: &Value) {
    if let Some(input) = value.get("input_tokens").and_then(Value::as_u64) {
        usage.input_tokens = input;
    }
    if let Some(output) = value.get("output_tokens").and_then(Value::as_u64) {
        usage.output_tokens = output;
    }
    if let Some(cache_creation) = value
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
    {
        usage.cache_creation_input_tokens = cache_creation;
    }
    if let Some(cache_read) = value.get("cache_read_input_tokens").and_then(Value::as_u64) {
        usage.cache_read_input_tokens = cache_read;
    }
    usage.usage_unknown =
        value.get("input_tokens").is_none() || value.get("output_tokens").is_none();
}
