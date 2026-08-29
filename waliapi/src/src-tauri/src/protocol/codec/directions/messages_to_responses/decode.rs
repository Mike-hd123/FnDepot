use super::super::super::{
    error::{DecodeError, FeatureKind, UnsupportedFeatures},
    ports::{DecodedResponse, NonStreamDecoder},
    report::{ConversionContext, Usage},
};
use super::{bad, required};
use serde_json::Value;

pub(super) struct ResponsesMessageDecoder {
    pub(super) context: ConversionContext,
}
impl NonStreamDecoder for ResponsesMessageDecoder {
    fn decode(&self, b: &Value) -> Result<DecodedResponse, DecodeError> {
        decode_response(b, &self.context).map_err(DecodeError::from)
    }
}
pub fn decode_response(
    body: &Value,
    c: &ConversionContext,
) -> Result<DecodedResponse, UnsupportedFeatures> {
    let r = body
        .get("response")
        .filter(|v| v.is_object())
        .unwrap_or(body);
    let output = r.get("output").and_then(Value::as_array).ok_or_else(|| {
        bad(
            FeatureKind::UnknownEvent,
            "/output",
            "Responses body requires output array",
        )
    })?;
    let mut content = Vec::new();
    for (i, item) in output.iter().enumerate() {
        let p = format!("/output/{i}");
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        bad(
                            FeatureKind::UnknownBlock,
                            format!("{p}/content"),
                            "message content is required",
                        )
                    })?
                {
                    match part.get("type").and_then(Value::as_str){Some("output_text")|Some("text")=>content.push(serde_json::json!({"type":"text","text":part.get("text").and_then(Value::as_str).unwrap_or("")})),Some(x)=>return Err(bad(FeatureKind::UnknownBlock,format!("{p}/content/type"),format!("response content {x:?} is unsupported"))),None=>return Err(bad(FeatureKind::UnknownBlock,format!("{p}/content/type"),"content type is required"))}
                }
            }
            Some("reasoning") => {
                let text = item
                    // `content/reasoning_text` is the replayable Responses
                    // reasoning representation. Retain `summary` only as a
                    // compatibility fallback for older providers.
                    .get("content")
                    .or_else(|| item.get("summary"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|x| {
                        matches!(
                            x.get("type").and_then(Value::as_str),
                            Some("reasoning_text" | "summary_text")
                        )
                    })
                    .filter_map(|x| x.get("text").and_then(Value::as_str))
                    .collect::<String>();
                if !text.is_empty() {
                    content.push(serde_json::json!({"type":"thinking","thinking":text}));
                }
            }
            Some("function_call") => {
                let id = required(item, "call_id", &p)?;
                let name = required(item, "name", &p)?;
                let args = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        bad(
                            FeatureKind::InvalidToolArguments,
                            format!("{p}/arguments"),
                            "arguments are required",
                        )
                    })?;
                let input: Value = serde_json::from_str(args).map_err(|_| {
                    bad(
                        FeatureKind::InvalidToolArguments,
                        format!("{p}/arguments"),
                        "arguments must be valid JSON",
                    )
                })?;
                if !input.is_object() {
                    return Err(bad(
                        FeatureKind::InvalidToolArguments,
                        format!("{p}/arguments"),
                        "arguments must be an object",
                    ));
                }
                content
                    .push(serde_json::json!({"type":"tool_use","id":id,"name":name,"input":input}));
            }
            Some(x) => {
                return Err(bad(
                    FeatureKind::UnknownEvent,
                    format!("{p}/type"),
                    format!("response item {x:?} is unsupported"),
                ))
            }
            None => {
                return Err(bad(
                    FeatureKind::UnknownEvent,
                    format!("{p}/type"),
                    "response item type is required",
                ))
            }
        }
    }
    if content.is_empty() {
        content.push(serde_json::json!({"type":"text","text":""}));
    }
    let usage = usage(r);
    let (status, stop) = match r.get("status").and_then(Value::as_str) {
        Some("completed") | None => (
            "message",
            if content
                .iter()
                .any(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
            {
                "tool_use"
            } else {
                "end_turn"
            },
        ),
        Some("incomplete") => (
            "message",
            match r
                .pointer("/incomplete_details/reason")
                .and_then(Value::as_str)
            {
                Some("max_output_tokens") | Some("max_tokens") | None => "max_tokens",
                Some("content_filter") | Some("safety") => "refusal",
                Some(x) => {
                    return Err(bad(
                        FeatureKind::UnknownFinishReason,
                        "/incomplete_details/reason",
                        format!("unknown incomplete reason {x:?}"),
                    ))
                }
            },
        ),
        Some("failed") => {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/status",
                "Responses response failed",
            ))
        }
        Some(x) => {
            return Err(bad(
                FeatureKind::UnknownEvent,
                "/status",
                format!("unknown status {x:?}"),
            ))
        }
    };
    Ok(DecodedResponse {
        body: serde_json::json!({"id":r.get("id").and_then(Value::as_str).unwrap_or(&c.request_id),"type":status,"role":"assistant","model":r.get("model").and_then(Value::as_str).unwrap_or(&c.upstream_model),"content":content,"stop_reason":stop,"stop_sequence":null,"usage":{"input_tokens":usage.input_tokens,"output_tokens":usage.output_tokens,"cache_creation_input_tokens":usage.cache_creation_input_tokens,"cache_read_input_tokens":usage.cache_read_input_tokens}}),
        usage: Some(usage),
    })
}
pub(super) fn usage(r: &Value) -> Usage {
    let i = r.pointer("/usage/input_tokens").and_then(Value::as_u64);
    let o = r.pointer("/usage/output_tokens").and_then(Value::as_u64);
    Usage {
        input_tokens: i.unwrap_or(0),
        output_tokens: o.unwrap_or(0),
        cache_creation_input_tokens: r
            .pointer("/usage/cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_input_tokens: r
            .pointer("/usage/cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        usage_unknown: i.is_none() || o.is_none(),
    }
}
