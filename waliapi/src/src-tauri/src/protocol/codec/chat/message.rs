use super::super::error::{FeatureKind, UnsupportedFeatures};
use super::super::request;
use serde_json::Value;

/// Convert one Chat message to an Anthropic message (and possibly system parts).
pub(super) fn convert_chat_message_to_anthropic(
    msg: &Value,
    pointer: &str,
    messages_out: &mut Vec<Value>,
    system_parts: &mut Vec<String>,
) -> Result<(), UnsupportedFeatures> {
    let role = msg.get("role").and_then(Value::as_str).ok_or_else(|| {
        UnsupportedFeatures::single(
            FeatureKind::UnknownRole,
            format!("{pointer}/role"),
            "Chat message missing role",
        )
    })?;

    match role {
        "system" | "developer" => {
            // Anthropic has a single top-level `system`; multiple Chat system
            // messages are concatenated in order (preserving order).
            match msg.get("content") {
                Some(Value::String(s)) => {
                    if !s.is_empty() {
                        system_parts.push(s.clone());
                    }
                }
                Some(Value::Array(blocks)) => {
                    for (bi, b) in blocks.iter().enumerate() {
                        match b.get("type").and_then(Value::as_str) {
                            Some("text") => {
                                let t = b.get("text").and_then(Value::as_str).unwrap_or("");
                                if !t.is_empty() {
                                    system_parts.push(t.to_string());
                                }
                            }
                            _ => {
                                return Err(UnsupportedFeatures::single(
                                    FeatureKind::UnknownBlock,
                                    format!("{pointer}/content/{bi}/type"),
                                    "system/developer content block must be text",
                                ))
                            }
                        }
                    }
                }
                _ => {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::UnknownBlock,
                        format!("{pointer}/content"),
                        "system/developer content must be a string or text blocks",
                    ))
                }
            }
            Ok(())
        }
        "user" => {
            let content = msg.get("content");
            let mut blocks: Vec<Value> = Vec::new();
            match content {
                Some(Value::String(s)) => {
                    if !s.is_empty() {
                        blocks.push(serde_json::json!({"type": "text", "text": s}));
                    }
                }
                Some(Value::Array(items)) => {
                    for (bi, item) in items.iter().enumerate() {
                        let bp = format!("{pointer}/content/{bi}");
                        match item.get("type").and_then(Value::as_str) {
                            Some("text") => {
                                let t = item.get("text").and_then(Value::as_str).unwrap_or("");
                                if !t.is_empty() {
                                    blocks.push(serde_json::json!({"type": "text", "text": t}));
                                }
                            }
                            Some("image_url") => {
                                // Chat image_url -> Anthropic image block.  A
                                // Chat `image_url` may carry either a data URL or
                                // a plain http(s) URL; both are validated to
                                // mirror the request-side image gate (R15).
                                let url = item
                                    .pointer("/image_url/url")
                                    .and_then(Value::as_str)
                                    .ok_or_else(|| {
                                        UnsupportedFeatures::single(
                                            FeatureKind::Media,
                                            format!("{bp}/image_url/url"),
                                            "image_url content block missing image_url.url",
                                        )
                                    })?;
                                let (_media_type, base64) = parse_data_url(url);
                                let source = if let Some((mt, data)) = base64 {
                                    if !mt.starts_with("image/") {
                                        return Err(UnsupportedFeatures::single(
                                            FeatureKind::Media,
                                            format!("{bp}/image_url/url"),
                                            format!("data URL media type {mt:?} is not an image"),
                                        ));
                                    }
                                    if data.len() > request::MAX_IMAGE_BYTES {
                                        return Err(UnsupportedFeatures::single(
                                            FeatureKind::Media,
                                            format!("{bp}/image_url/url"),
                                            "data URL image exceeds the size limit",
                                        ));
                                    }
                                    serde_json::json!({
                                        "type": "base64",
                                        "media_type": mt,
                                        "data": data
                                    })
                                } else {
                                    if !url.starts_with("http://") && !url.starts_with("https://") {
                                        return Err(UnsupportedFeatures::single(
                                            FeatureKind::Media,
                                            format!("{bp}/image_url/url"),
                                            "image_url url must be http(s) or a data URL",
                                        ));
                                    }
                                    serde_json::json!({"type": "url", "url": url})
                                };
                                blocks.push(serde_json::json!({
                                    "type": "image",
                                    "source": source,
                                }));
                            }
                            Some("input_text") | Some("output_text") => {
                                let t = item.get("text").and_then(Value::as_str).unwrap_or("");
                                if !t.is_empty() {
                                    blocks.push(serde_json::json!({"type": "text", "text": t}));
                                }
                            }
                            Some(other) => {
                                return Err(UnsupportedFeatures::single(
                                    FeatureKind::UnknownBlock,
                                    format!("{bp}/type"),
                                    format!("unsupported user content block type {other:?}"),
                                ))
                            }
                            None => {
                                return Err(UnsupportedFeatures::single(
                                    FeatureKind::UnknownBlock,
                                    format!("{bp}/type"),
                                    "user content block missing type",
                                ))
                            }
                        }
                    }
                }
                Some(Value::Null) | None => {
                    // empty user message — allowed (e.g. assistant tool_use continues)
                }
                _ => {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::UnknownBlock,
                        format!("{pointer}/content"),
                        "user content must be a string or an array of blocks",
                    ))
                }
            }
            if !blocks.is_empty() {
                messages_out.push(serde_json::json!({
                    "role": "user",
                    "content": Value::Array(blocks),
                }));
            }
            Ok(())
        }
        "assistant" => {
            let mut content_blocks: Vec<Value> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();
            let content = msg.get("content");
            match content {
                Some(Value::String(s)) => {
                    if !s.is_empty() {
                        content_blocks.push(serde_json::json!({"type": "text", "text": s}));
                    }
                }
                Some(Value::Array(items)) => {
                    for (bi, item) in items.iter().enumerate() {
                        let bp = format!("{pointer}/content/{bi}");
                        match item.get("type").and_then(Value::as_str) {
                            Some("text") => {
                                let t = item.get("text").and_then(Value::as_str).unwrap_or("");
                                if !t.is_empty() {
                                    content_blocks
                                        .push(serde_json::json!({"type": "text", "text": t}));
                                }
                            }
                            Some(other) => {
                                return Err(UnsupportedFeatures::single(
                                    FeatureKind::UnknownBlock,
                                    format!("{bp}/type"),
                                    format!("unsupported assistant content block type {other:?}"),
                                ))
                            }
                            None => {
                                return Err(UnsupportedFeatures::single(
                                    FeatureKind::UnknownBlock,
                                    format!("{bp}/type"),
                                    "assistant content block missing type",
                                ))
                            }
                        }
                    }
                }
                Some(Value::Null) | None => {}
                _ => {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::UnknownBlock,
                        format!("{pointer}/content"),
                        "assistant content must be a string or an array of blocks",
                    ))
                }
            }
            if let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) {
                for (ci, call) in calls.iter().enumerate() {
                    let cp = format!("{pointer}/tool_calls/{ci}");
                    let id = call
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            UnsupportedFeatures::single(
                                FeatureKind::MissingToolField,
                                format!("{cp}/id"),
                                "tool call missing id",
                            )
                        })?;
                    let name = call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            UnsupportedFeatures::single(
                                FeatureKind::MissingToolField,
                                format!("{cp}/function/name"),
                                "tool call missing function.name",
                            )
                        })?;
                    let args_str = call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            UnsupportedFeatures::single(
                                FeatureKind::InvalidToolArguments,
                                format!("{cp}/function/arguments"),
                                "tool call missing function.arguments",
                            )
                        })?;
                    let input: Value = serde_json::from_str(args_str).map_err(|e| {
                        UnsupportedFeatures::single(
                            FeatureKind::InvalidToolArguments,
                            format!("{cp}/function/arguments"),
                            format!("tool arguments are not valid JSON: {e}"),
                        )
                    })?;
                    if !input.is_object() {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::InvalidToolArguments,
                            format!("{cp}/function/arguments"),
                            "tool arguments must decode to a JSON object",
                        ));
                    }
                    tool_calls.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
            }
            let mut combined = content_blocks;
            combined.extend(tool_calls);
            if combined.is_empty() {
                // Anthropic rejects an assistant message with no content; drop
                // it only when there is truly nothing (matches existing bridge).
                return Ok(());
            }
            messages_out.push(serde_json::json!({
                "role": "assistant",
                "content": Value::Array(combined),
            }));
            Ok(())
        }
        "tool" => {
            let tool_call_id = msg
                .get("tool_call_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    UnsupportedFeatures::single(
                        FeatureKind::MissingToolField,
                        format!("{pointer}/tool_call_id"),
                        "tool message missing tool_call_id",
                    )
                })?;
            let content = msg.get("content");
            let text = match content {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(items)) => {
                    let mut t = String::new();
                    for (bi, item) in items.iter().enumerate() {
                        let bp = format!("{pointer}/content/{bi}");
                        match item.get("type").and_then(Value::as_str) {
                            Some("text") => t.push_str(item.get("text").and_then(Value::as_str).unwrap_or("")),
                            Some("image_url") => {
                                return Err(UnsupportedFeatures::single(
                                    FeatureKind::Media,
                                    format!("{bp}/type"),
                                    "tool_result images are not representable in Messages for this version",
                                ))
                            }
                            _ => {
                                return Err(UnsupportedFeatures::single(
                                    FeatureKind::UnknownBlock,
                                    format!("{bp}/type"),
                                    "tool message content block must be text",
                                ))
                            }
                        }
                    }
                    t
                }
                Some(Value::Null) | None => String::new(),
                _ => {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::UnknownBlock,
                        format!("{pointer}/content"),
                        "tool message content must be a string or text blocks",
                    ))
                }
            };
            let is_error = msg
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            // Canonical Anthropic tool_result is a *content block* inside the
            // user message's content array.  The message-level `tool_result`
            // key is not part of the Messages schema and the real API rejects
            // it with 400 invalid_request_error.
            let mut result_blocks: Vec<Value> = Vec::new();
            if !text.is_empty() {
                result_blocks.push(serde_json::json!({"type": "text", "text": text}));
            }
            let tool_result_block = serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_call_id,
                "content": Value::Array(result_blocks),
                "is_error": is_error,
            });
            // Anthropic requires all tool results for one assistant turn in a
            // SINGLE user message: aggregate consecutive tool results into the
            // same user message instead of one message per tool result.
            let appended = if let Some(last) = messages_out.last_mut() {
                if last.get("role").and_then(Value::as_str) == Some("user") {
                    if let Some(content_arr) = last.get_mut("content").and_then(Value::as_array_mut)
                    {
                        let is_tool_result = content_arr
                            .last()
                            .map(|b| b.get("type").and_then(Value::as_str))
                            == Some(Some("tool_result"));
                        if is_tool_result {
                            content_arr.push(tool_result_block.clone());
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if !appended {
                messages_out.push(serde_json::json!({
                    "role": "user",
                    "content": Value::Array(vec![tool_result_block]),
                }));
            }
            Ok(())
        }
        other => Err(UnsupportedFeatures::single(
            FeatureKind::UnknownRole,
            format!("{pointer}/role"),
            format!("unsupported Chat message role {other:?}"),
        )),
    }
}

/// Parse a `data:` URL into `(media_type, Option<(media_type, payload)>)`.
fn parse_data_url(url: &str) -> (Option<String>, Option<(String, String)>) {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some(semi) = rest.find(';') {
            let media_type = rest[..semi].to_string();
            let after = &rest[semi + 1..];
            if let Some(b64) = after.strip_prefix("base64,") {
                return (
                    Some(media_type.clone()),
                    Some((media_type, b64.to_string())),
                );
            }
            // e.g. data:image/png;charset=utf-8,...
            return (Some(media_type), None);
        }
        if let Some(comma) = rest.find(',') {
            let media_type = rest[..comma].to_string();
            return (Some(media_type), None);
        }
        (Some(rest.to_string()), None)
    } else {
        (None, None)
    }
}
