use serde_json::Value;

/// Convert an OpenAI Chat Completions response to Anthropic Messages format.
///
/// This deliberately fails instead of inventing tool input when an upstream
/// returns malformed function arguments. Claude Code uses those arguments to
/// execute local tools, so replacing bad JSON with `{}` is unsafe.
pub fn openai_to_anthropic(openai_resp: &Value, model: &str) -> Result<Value, String> {
    let choice = openai_resp
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first());

    let message = choice.and_then(|ch| ch.get("message"));

    let message = message
        .ok_or_else(|| "OpenAI response does not contain a completion message".to_string())?;
    // Fail-open (CPA semantics): upstream reasoning is surfaced as a Messages
    // `thinking` block, always kept (even when content is also present).  Only
    // the visible text is used; `{text: ...}` object form is unwrapped.
    let reasoning_text = message
        .get("reasoning_content")
        .and_then(|v| match v {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Object(m) => m
                .get("text")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from),
            _ => None,
        })
        .or_else(|| match message.get("thinking") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(Value::Object(m)) => m
                .get("thinking")
                .or_else(|| m.get("text"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from),
            _ => None,
        });
    let content_text = match message.get("content") {
        None | Some(Value::Null) => "",
        Some(Value::String(value)) => value,
        Some(_) => {
            return Err("OpenAI response has unsupported non-text message content".to_string())
        }
    };

    let finish_reason = choice
        .and_then(|ch| ch.get("finish_reason"))
        .and_then(|f| f.as_str())
        .unwrap_or("");

    // Chat Completions normally sets `tool_calls`, but some compatible
    // upstreams omit it.  The tool-call payload is less ambiguous than a
    // missing finish reason, so do not report a completed tool turn as an
    // ordinary end_turn.
    let has_tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty());
    let stop_reason = match finish_reason {
        "stop" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        "content_filter" => "refusal",
        _ if message.get("refusal").is_some() => "refusal",
        _ if has_tool_calls => "tool_use",
        _ => "end_turn",
    };

    let input_tokens = openai_resp
        .get("usage")
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    let output_tokens = openai_resp
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);

    // Build content array: thinking block (if any) + text blocks + tool_use
    let mut content_blocks = Vec::new();

    // Add thinking block first (reasoning precedes visible text)
    if let Some(rt) = reasoning_text.as_ref().filter(|s| !s.is_empty()) {
        content_blocks.push(serde_json::json!({
            "type": "thinking",
            "thinking": rt
        }));
    }

    // Add text block if present
    if !content_text.is_empty() {
        content_blocks.push(serde_json::json!({
            "type": "text",
            "text": content_text
        }));
    }

    // Add tool_use blocks for tool_calls
    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        for tc in tool_calls {
            let id = tc
                .get("id")
                .and_then(|i| i.as_str())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "OpenAI response tool call is missing its id".to_string())?;
            let func = tc.get("function");
            let name = func
                .and_then(|f| f.get("name").and_then(|n| n.as_str()))
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    "OpenAI response tool call is missing its function name".to_string()
                })?;
            let arguments_str = func
                .and_then(|f| f.get("arguments").and_then(|a| a.as_str()))
                .ok_or_else(|| {
                    "OpenAI response tool call is missing function arguments".to_string()
                })?;
            let input: Value = serde_json::from_str(arguments_str).map_err(|error| {
                format!(
                    "OpenAI response contained invalid tool arguments: {}",
                    error
                )
            })?;
            if !input.is_object() {
                return Err(
                    "OpenAI response tool arguments must decode to a JSON object".to_string(),
                );
            }

            content_blocks.push(serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }

    // If no content blocks at all, add empty text
    if content_blocks.is_empty() {
        content_blocks.push(serde_json::json!({
            "type": "text",
            "text": ""
        }));
    }

    Ok(serde_json::json!({
        "id": openai_resp.get("id").cloned().unwrap_or(Value::String(format!("msg_{}", uuid::Uuid::new_v4().simple()))),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content_blocks,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    }))
}
