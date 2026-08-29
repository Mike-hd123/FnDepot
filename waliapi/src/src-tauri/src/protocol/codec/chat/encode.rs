use super::super::error::{FeatureKind, UnsupportedFeatures};
use super::super::report::ConversionContext;
use super::super::request;
use super::message::convert_chat_message_to_anthropic;
use serde_json::{Map, Value};

/// Sampling parameters we can map 1:1 between Chat and Messages.
///
/// `n` is intentionally absent: Anthropic Messages only ever returns a single
/// completion, so `n > 1` cannot be preserved and must be rejected rather than
/// silently yielding one completion.
const SUPPORTED_TOP_LEVEL: &[&str] = &[
    "model",
    "messages",
    "max_tokens",
    "max_completion_tokens",
    "temperature",
    "top_p",
    "stream",
    "stop",
    "tools",
    "tool_choice",
    "reasoning_effort",
    "store",
    "stream_options",
];

/// Encode a Chat Completions request into an Anthropic Messages request.
///
/// `model` is the mapped upstream model decided by the caller; the codec never
/// re-maps models.
pub fn encode_chat_to_messages(
    body: &Value,
    model: &str,
) -> Result<(Value, ConversionContext), UnsupportedFeatures> {
    let mut out = Vec::new();
    // Controls accepted below are intentionally omitted from the Messages
    // request.  Keep them observable in the conversion report rather than
    // silently dropping them.
    let mut normalized = Vec::new();
    let mut messages_out: Vec<Value> = Vec::new();
    let mut system_parts: Vec<String> = Vec::new();
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    // ---- top-level feature scan ----
    if let Some(obj) = body.as_object() {
        for (key, value) in obj {
            if !SUPPORTED_TOP_LEVEL.contains(&key.as_str()) {
                // structured output (response_format / JSON-schema) has its own
                // stable code.
                let kind = if key == "response_format" {
                    FeatureKind::StructuredOutput
                } else {
                    FeatureKind::UnsupportedField
                };
                request::reject(
                    &mut out,
                    kind,
                    format!("/{key}"),
                    format!("Chat request field {key:?} is not supported by chat_to_messages_v1"),
                );
                continue;
            }
            match key.as_str() {
                // `store:false` is the OpenAI default and has no remote side
                // effect.  Anthropic Messages has no wire equivalent, so it is
                // safely normalized away.  `store:true` must remain rejected:
                // its persistence semantics cannot be preserved.
                "store" if value.as_bool() == Some(false) => {
                    normalized.push("/store".to_owned());
                }
                "store" => request::reject(
                    &mut out,
                    FeatureKind::UnsupportedField,
                    "/store",
                    "store must be false when converting Chat to Messages",
                ),
                // The Messages -> Chat stream decoder always emits observed
                // usage in the terminal Chat chunk.  That preserves the one
                // OpenAI Chat stream option we can represent.
                "stream_options" => {
                    normalize_stream_options(value, stream, &mut normalized, &mut out)
                }
                _ => {}
            }
            // Unknown finish reason never applies to requests; here we only
            // reject structural fields that are present with an unsupported
            // shape.
            if key == "tool_choice" {
                let _ = value;
            }
        }
    }

    // ---- messages ----
    let messages = body.get("messages").and_then(Value::as_array);
    let messages = match messages {
        Some(arr) => arr,
        None => {
            request::reject(
                &mut out,
                FeatureKind::UnsupportedField,
                "/messages",
                "Chat request requires a messages array",
            );
            return Err(UnsupportedFeatures::new(out));
        }
    };

    for (i, msg) in messages.iter().enumerate() {
        let mp = format!("/messages/{i}");
        if let Err(e) =
            convert_chat_message_to_anthropic(msg, &mp, &mut messages_out, &mut system_parts)
        {
            // merge rejections
            out.extend(e.fields);
        }
    }

    // ---- model ----
    // Chat Completions *requests* do not carry an `id` (that is a response
    // field), so derive a per-request conversation id from the caller instead.
    let request_id = format!("chatcmpl_{}", uuid::Uuid::new_v4().simple());

    // ---- sampling params ----
    let mut claude = Map::new();
    claude.insert("model".to_string(), Value::String(model.to_string()));
    // Anthropic Messages requires `max_tokens`.  When the Chat request omits it
    // we use a documented safe default (4096) so the upstream call is not
    // malformed; a per-model profile would live in the caller (PreparedAttempt)
    // and could override this via the request body.  Recorded as a deferred
    // choice in the T04 report (F8).
    claude.insert(
        "max_tokens".to_string(),
        body.get("max_tokens")
            .or_else(|| body.get("max_completion_tokens"))
            .and_then(Value::as_u64)
            .map(Value::from)
            .unwrap_or(Value::from(4096u64)),
    );
    if !system_parts.is_empty() {
        claude.insert(
            "system".to_string(),
            Value::Array(
                system_parts
                    .iter()
                    .map(|t| serde_json::json!({"type": "text", "text": t}))
                    .collect(),
            ),
        );
    }
    if let Some(t) = body.get("temperature") {
        if !t.is_null() {
            claude.insert("temperature".to_string(), t.clone());
        }
    }
    if let Some(t) = body.get("top_p") {
        if !t.is_null() {
            claude.insert("top_p".to_string(), t.clone());
        }
    }
    if let Some(stop) = body.get("stop") {
        let mapped = match stop {
            Value::String(s) => Value::Array(vec![Value::String(s.clone())]),
            Value::Array(a) => Value::Array(a.clone()),
            _ => {
                request::reject(
                    &mut out,
                    FeatureKind::UnsupportedField,
                    "/stop",
                    "stop must be a string or an array of strings",
                );
                Value::Null
            }
        };
        if !mapped.is_null() {
            claude.insert("stop_sequences".to_string(), mapped);
        }
    }
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let mut claude_tools = Vec::new();
        for (i, tool) in tools.iter().enumerate() {
            let tp = format!("/tools/{i}");
            match convert_chat_tool_to_anthropic(tool, &tp) {
                Ok(t) => claude_tools.push(t),
                Err(e) => out.extend(e.fields),
            }
        }
        if !claude_tools.is_empty() {
            claude.insert("tools".to_string(), Value::Array(claude_tools));
        }
    }
    if let Some(tc) = body.get("tool_choice") {
        match request::chat_tool_choice_to_anthropic(tc, "/tool_choice") {
            Ok(Some(v)) => {
                claude.insert("tool_choice".to_string(), v);
            }
            Ok(None) => {}
            Err(e) => out.extend(e.fields),
        }
    }
    claude.insert("stream".to_string(), Value::Bool(stream));

    // reasoning_effort -> thinking (fail-open mapping, CPA semantics).  Only
    // when the downstream asked for a reasoning effort; absent effort leaves
    // thinking unset so the upstream applies its own default.
    if let Some(effort) = body.get("reasoning_effort").and_then(Value::as_str) {
        let e = effort.to_ascii_lowercase();
        match e.as_str() {
            // Some upstream models always reason and reject
            // `thinking.type = disabled`.  Use their least-expensive accepted
            // effort instead of turning a valid Chat request into an upstream
            // 1210 error.
            "none" | "off" => {
                claude.insert(
                    "thinking".to_string(),
                    serde_json::json!({"type": "adaptive"}),
                );
                claude.insert(
                    "output_config".to_string(),
                    serde_json::json!({"effort": "low"}),
                );
            }
            "auto" => {
                claude.insert(
                    "thinking".to_string(),
                    serde_json::json!({"type": "adaptive"}),
                );
            }
            _ => {
                let mapped = crate::protocol::thinking::map_effort_to_claude(&e);
                claude.insert(
                    "thinking".to_string(),
                    serde_json::json!({"type": "adaptive"}),
                );
                claude.insert(
                    "output_config".to_string(),
                    serde_json::json!({"effort": mapped}),
                );
            }
        }
    }

    if !out.is_empty() {
        return Err(UnsupportedFeatures::new(out));
    }

    claude.insert("messages".to_string(), Value::Array(messages_out));
    let mut context = ConversionContext::new(request_id, model.to_string(), stream);
    context.normalized = normalized;
    Ok((Value::Object(claude), context))
}

/// Validate the Chat streaming control supported by the Messages response
/// decoder.  Any option other than `include_usage:true` would alter the Chat
/// response contract and is therefore rejected instead of silently dropped.
fn normalize_stream_options(
    value: &Value,
    stream: bool,
    normalized: &mut Vec<String>,
    out: &mut Vec<super::super::error::RejectedField>,
) {
    let Some(options) = value.as_object() else {
        request::reject(
            out,
            FeatureKind::UnsupportedField,
            "/stream_options",
            "stream_options must be an object",
        );
        return;
    };

    for key in options.keys() {
        if key != "include_usage" {
            request::reject(
                out,
                FeatureKind::UnsupportedField,
                format!("/stream_options/{key}"),
                format!("stream_options field {key:?} is not supported by chat_to_messages_v1"),
            );
        }
    }

    match options.get("include_usage") {
        Some(Value::Bool(true)) if stream => {
            normalized.push("/stream_options".to_owned());
        }
        Some(Value::Bool(true)) if !stream => request::reject(
            out,
            FeatureKind::UnsupportedField,
            "/stream_options",
            "stream_options.include_usage:true requires stream:true",
        ),
        Some(Value::Bool(false)) => request::reject(
            out,
            FeatureKind::UnsupportedField,
            "/stream_options/include_usage",
            "stream_options.include_usage:false cannot be preserved by chat_to_messages_v1",
        ),
        Some(_) => request::reject(
            out,
            FeatureKind::UnsupportedField,
            "/stream_options/include_usage",
            "stream_options.include_usage must be true",
        ),
        None => request::reject(
            out,
            FeatureKind::UnsupportedField,
            "/stream_options/include_usage",
            "stream_options.include_usage:true is required by chat_to_messages_v1",
        ),
    }
}
/// Convert a Chat `tools` array entry to an Anthropic tool.
fn convert_chat_tool_to_anthropic(
    tool: &Value,
    pointer: &str,
) -> Result<Value, UnsupportedFeatures> {
    let ty = tool.get("type").and_then(Value::as_str);
    if ty != Some("function") {
        return Err(UnsupportedFeatures::single(
            FeatureKind::BuiltinTool,
            format!("{pointer}/type"),
            format!("only function tools are supported, found {ty:?}"),
        ));
    }
    let f = tool.get("function").ok_or_else(|| {
        UnsupportedFeatures::single(
            FeatureKind::MissingToolField,
            format!("{pointer}/function"),
            "function tool missing function",
        )
    })?;
    let name = f
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            UnsupportedFeatures::single(
                FeatureKind::MissingToolField,
                format!("{pointer}/function/name"),
                "function tool missing name",
            )
        })?;
    let parameters = f.get("parameters").cloned().ok_or_else(|| {
        UnsupportedFeatures::single(
            FeatureKind::InvalidToolArguments,
            format!("{pointer}/function/parameters"),
            "function tool missing parameters",
        )
    })?;
    if !parameters.is_object() {
        return Err(UnsupportedFeatures::single(
            FeatureKind::InvalidToolArguments,
            format!("{pointer}/function/parameters"),
            "function tool parameters must be a JSON schema object",
        ));
    }
    let mut claude_tool = serde_json::json!({
        "name": name,
        "input_schema": parameters,
    });
    if let Some(desc) = f.get("description").and_then(Value::as_str) {
        if !desc.is_empty() {
            claude_tool["description"] = Value::String(desc.to_string());
        }
    }
    Ok(claude_tool)
}
