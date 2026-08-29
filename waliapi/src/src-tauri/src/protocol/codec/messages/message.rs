use super::super::error::{FeatureKind, UnsupportedFeatures};
use super::super::request;
use serde_json::Value;

/// Convert one Anthropic message (content array or string) into zero or more
/// Chat messages.  A `tool_result` user message is split into the preceding
/// text (as a user message) and a `role: tool` message; an assistant message
/// with tool_use blocks becomes one assistant message with `tool_calls`.
pub(super) fn convert_anthropic_message_to_chat(
    msg: &Value,
    pointer: &str,
    normalized: &mut Vec<String>,
    require_tool_reasoning_content: bool,
) -> Result<Vec<Value>, UnsupportedFeatures> {
    let role = msg.get("role").and_then(Value::as_str).ok_or_else(|| {
        UnsupportedFeatures::single(
            FeatureKind::UnknownRole,
            format!("{pointer}/role"),
            "Messages message missing role",
        )
    })?;
    match role {
        "user" | "assistant" | "system" => {}
        other => {
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownRole,
                format!("{pointer}/role"),
                format!("unsupported Messages role {other:?}"),
            ))
        }
    }

    let content = msg.get("content");
    if role == "system" {
        let content = content.ok_or_else(|| {
            UnsupportedFeatures::single(
                FeatureKind::UnknownBlock,
                format!("{pointer}/content"),
                "system message missing content",
            )
        })?;
        let text =
            request::anthropic_system_to_chat(content, &format!("{pointer}/content"), normalized)?;
        return Ok(vec![serde_json::json!({"role": "system", "content": text})]);
    }

    let content_arr = content.and_then(Value::as_array);
    if let Some(items) = content_arr {
        let mut user_parts: Vec<Value> = Vec::new();
        let mut out = Vec::new();
        // Buffer the tool messages produced by tool_result blocks so they can be
        // inserted in order.  OpenAI requires a `role: tool` message to follow
        // the assistant tool_calls immediately; when a user message mixes
        // tool_result with preceding text (`[tool_result, text]` vs
        // `[text, tool_result]`), we must not let the text push a tool message
        // away from its assistant.  We therefore collect tool messages and emit
        // them ahead of any buffered text (option B in the review).
        let mut tool_messages: Vec<Value> = Vec::new();
        let flush_user = |parts: &mut Vec<Value>, out: &mut Vec<Value>| {
            if !parts.is_empty() {
                // Chat accepts a plain string when the user content is a single
                // text part; richer arrays are preserved as arrays.
                let content = if parts.len() == 1
                    && parts[0].get("type").and_then(Value::as_str) == Some("text")
                {
                    Value::String(
                        parts[0]
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    )
                } else {
                    Value::Array(std::mem::take(parts))
                };
                out.push(serde_json::json!({"role": "user", "content": content}));
            }
        };
        let mut assistant_text: Vec<String> = Vec::new();
        let mut assistant_reasoning = String::new();
        // Preserve the distinction between no thinking block and an explicitly
        // empty thinking block. Strict OpenAI-compatible thinking providers
        // validate the presence of `reasoning_content` on historical assistant
        // tool turns, even when the source block contains no readable text.
        let mut assistant_had_thinking = false;
        let mut tool_calls: Vec<Value> = Vec::new();
        for (bi, block) in items.iter().enumerate() {
            let bp = format!("{pointer}/content/{bi}");
            let bt = block.get("type").and_then(Value::as_str).unwrap_or("");
            match bt {
                "text" => {
                    let t = block.get("text").and_then(Value::as_str).unwrap_or("");
                    match role {
                        "user" => {
                            if !t.is_empty() {
                                user_parts.push(serde_json::json!({"type": "text", "text": t}));
                            }
                        }
                        _ => {
                            if !t.is_empty() {
                                assistant_text.push(t.to_string());
                            }
                        }
                    }
                }
                "image" => {
                    if role != "user" {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::Media,
                            bp,
                            "assistant image blocks have no safe Chat representation",
                        ));
                    }
                    let img = request::anthropic_image_to_chat(block, &bp)?;
                    user_parts.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": img,
                    }));
                }
                "tool_use" => {
                    if role != "assistant" {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::UnknownBlock,
                            bp,
                            "tool_use blocks must be in an assistant message",
                        ));
                    }
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            UnsupportedFeatures::single(
                                FeatureKind::MissingToolField,
                                format!("{bp}/id"),
                                "tool_use block missing id",
                            )
                        })?;
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            UnsupportedFeatures::single(
                                FeatureKind::MissingToolField,
                                format!("{bp}/name"),
                                "tool_use block missing name",
                            )
                        })?;
                    // A tool_use block without `input` is malformed: never
                    // fabricate `{}`.  `input: {}` is fine only when explicitly
                    // present.
                    let input = block.get("input").ok_or_else(|| {
                        UnsupportedFeatures::single(
                            FeatureKind::MissingToolField,
                            format!("{bp}/input"),
                            "tool_use block missing input",
                        )
                    })?;
                    if !input.is_object() {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::InvalidToolArguments,
                            format!("{bp}/input"),
                            "tool_use input must be a JSON object",
                        ));
                    }
                    let input = input.clone();
                    let arguments = serde_json::to_string(&input).map_err(|e| {
                        UnsupportedFeatures::single(
                            FeatureKind::InvalidToolArguments,
                            format!("{bp}/input"),
                            format!("tool_use input could not be serialized: {e}"),
                        )
                    })?;
                    tool_calls.push(serde_json::json!({
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": arguments}
                    }));
                }
                "tool_result" => {
                    if role != "user" {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::UnknownBlock,
                            bp,
                            "tool_result blocks must be in a user message",
                        ));
                    }
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            UnsupportedFeatures::single(
                                FeatureKind::MissingToolField,
                                format!("{bp}/tool_use_id"),
                                "tool_result missing tool_use_id",
                            )
                        })?;
                    let (text, is_error) = tool_result_to_chat_content(block, &bp)?;
                    let content = if is_error {
                        format!("Tool execution error:\n{text}")
                    } else {
                        text
                    };
                    tool_messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content
                    }));
                }
                "thinking" => {
                    // Fail-open: assistant reasoning is carried into the Chat
                    // message as `reasoning_content` (OpenAI non-stream field);
                    // reasoning on any other role is dropped — we never inject
                    // thinking into a user/system channel.  `redacted_thinking`
                    // has no readable text and is ignored.
                    if role == "assistant" {
                        assistant_had_thinking = true;
                        if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                            if !t.is_empty() {
                                assistant_reasoning.push_str(t);
                            }
                        }
                        normalized.push(bp);
                    }
                }
                "redacted_thinking" => {
                    // No readable text; nothing to forward.
                    normalized.push(bp);
                }
                "cache_control" => {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::PromptCache,
                        bp,
                        "cache_control blocks have no Chat equivalent",
                    ))
                }
                other => {
                    return Err(UnsupportedFeatures::single(
                        FeatureKind::UnknownBlock,
                        bp,
                        format!("unsupported Messages content block type {other:?}"),
                    ))
                }
            }
        }
        if role == "assistant" {
            let content = if assistant_text.is_empty() {
                Value::Null
            } else {
                Value::String(assistant_text.join(""))
            };
            // Reasoning content extracted from assistant `thinking` blocks
            // (fail-open mapping to OpenAI's non-stream reasoning_content).
            //
            // Some Messages clients omit the thinking block when replaying a
            // historical assistant tool-call turn. Thinking-mode OpenAI
            // upstreams nevertheless require the field to be present on that
            // turn. Preserve an explicit empty value as a compatibility
            // fallback; never fabricate non-empty reasoning.
            let reasoning = (assistant_had_thinking
                || (!tool_calls.is_empty() && require_tool_reasoning_content))
                .then_some(assistant_reasoning);
            if tool_calls.is_empty() && content.is_null() && reasoning.is_none() {
                return Err(UnsupportedFeatures::single(
                    FeatureKind::UnknownBlock,
                    pointer,
                    "assistant message is empty",
                ));
            }
            let mut assistant = serde_json::json!({"role": "assistant", "content": content});
            if let Some(r) = reasoning {
                assistant["reasoning_content"] = Value::String(r);
            }
            if !tool_calls.is_empty() {
                assistant["tool_calls"] = Value::Array(tool_calls);
            }
            out.push(assistant);
        } else {
            // Emit tool messages ahead of the buffered text so they stay
            // adjacent to the assistant tool_calls message they answer.
            out.append(&mut tool_messages);
            flush_user(&mut user_parts, &mut out);
        }
        Ok(out)
    } else if let Some(s) = content.and_then(Value::as_str) {
        Ok(vec![serde_json::json!({"role": role, "content": s})])
    } else if content.map(|c| c.is_null()).unwrap_or(true) {
        // Anthropic allows null content on assistant messages carrying only
        // tool_use elsewhere; a null-content message with nothing else is
        // dropped.
        Ok(vec![])
    } else {
        Err(UnsupportedFeatures::single(
            FeatureKind::UnknownBlock,
            format!("{pointer}/content"),
            "Messages content must be a string or an array of blocks",
        ))
    }
}

/// Reduce a `tool_result` block to Chat text + error flag.
fn tool_result_to_chat_content(
    block: &Value,
    pointer: &str,
) -> Result<(String, bool), UnsupportedFeatures> {
    let is_error = block
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let content = block.get("content");
    match content {
        None | Some(Value::Null) => Ok((String::new(), is_error)),
        Some(Value::String(s)) => Ok((s.clone(), is_error)),
        Some(Value::Array(items)) => {
            let mut text = String::new();
            for (i, item) in items.iter().enumerate() {
                let ip = format!("{pointer}/content/{i}");
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        text.push_str(item.get("text").and_then(Value::as_str).unwrap_or(""))
                    }
                    Some("image") => {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::Media,
                            ip,
                            "tool_result images are not representable in Chat for this version",
                        ))
                    }
                    _ => {
                        return Err(UnsupportedFeatures::single(
                            FeatureKind::UnknownBlock,
                            ip,
                            "tool_result content block must be text",
                        ))
                    }
                }
            }
            Ok((text, is_error))
        }
        _ => Err(UnsupportedFeatures::single(
            FeatureKind::UnknownBlock,
            format!("{pointer}/content"),
            "tool_result content must be a string or text blocks",
        )),
    }
}
