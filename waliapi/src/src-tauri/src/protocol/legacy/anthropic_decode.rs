use serde_json::Value;

/// Convert an Anthropic Messages request to OpenAI Chat Completions.
///
/// This converter intentionally accepts only the intersection which can be
/// represented by Chat Completions. Native Anthropic channels must bypass it.
pub fn anthropic_to_openai(body: &Value) -> Result<Value, String> {
    // Fail-open (CLIProxyAPI semantics): thinking/output_config are mapped to
    // `reasoning_effort` below; container/context_management are dropped.  The
    // upstream provider adjudicates capability; we never reject thinking.
    let model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let messages = body
        .get("messages")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let max_tokens = body
        .get("max_tokens")
        .and_then(|m| m.as_u64())
        .unwrap_or(4096);
    let stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // Extract top-level system message and prepend it.
    let system = body
        .get("system")
        .map(anthropic_system_content_to_openai_text)
        .transpose()?;

    // Convert Anthropic message content (array format) to OpenAI string format
    let openai_messages = convert_anthropic_messages_to_openai(&messages, system)?;

    let mut openai_body = serde_json::json!({
        "model": model,
        "messages": openai_messages,
        "max_tokens": max_tokens,
        "stream": stream,
    });

    if let Some(temp) = body.get("temperature") {
        openai_body["temperature"] = temp.clone();
    }
    if let Some(top_p) = body.get("top_p") {
        openai_body["top_p"] = top_p.clone();
    }
    // Pass through top_k (OpenAI also supports this via some providers)
    if let Some(top_k) = body.get("top_k") {
        openai_body["top_k"] = top_k.clone();
    }
    // Pass through stop_sequences → stop
    if let Some(stop_seq) = body.get("stop_sequences") {
        openai_body["stop"] = stop_seq.clone();
    }
    if stream {
        let mut options = body
            .get("stream_options")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        if !options.is_object() {
            return Err("stream_options must be an object".to_string());
        }
        options["include_usage"] = Value::Bool(true);
        openai_body["stream_options"] = options;
    }
    // Convert Anthropic tools to OpenAI tools format
    // Anthropic: {"name": "xxx", "description": "xxx", "input_schema": {...}}
    // OpenAI: {"type": "function", "function": {"name": "xxx", "description": "xxx", "parameters": {...}}}
    // Also handles Anthropic built-in tools (web_search, computer_use, etc.) which are skipped.
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let mut openai_tools = Vec::new();
        for tool in tools {
            // `cache_control` on a custom tool is likewise an Anthropic
            // caching annotation and has no Chat Completions equivalent.
            // Get the tool type — Anthropic custom tools use "custom" or have no type field
            let tool_type = tool
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("custom");
            match tool_type {
                // Standard function tools (type "custom" or no type)
                "custom" | "" => {
                    let name = tool
                        .get("name")
                        .and_then(|n| n.as_str())
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| "Anthropic tool is missing its name".to_string())?;
                    let description = tool
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    let parameters = tool.get("input_schema").cloned().ok_or_else(|| {
                        format!("Anthropic tool '{}' is missing input_schema", name)
                    })?;
                    openai_tools.push(serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": name,
                            "description": description,
                            "parameters": parameters
                        }
                    }));
                }
                _ => {
                    return Err(
                        "Anthropic built-in tools require a native Anthropic Messages channel"
                            .to_string(),
                    )
                }
            }
        }
        if !openai_tools.is_empty() {
            openai_body["tools"] = Value::Array(openai_tools);
        }
    }

    // Convert tool_choice
    // Anthropic: {"type": "auto"} or {"type": "any"} or {"type": "tool", "name": "xxx"}
    // OpenAI: "auto" or "required" or {"type": "function", "function": {"name": "xxx"}}
    if let Some(tc) = body.get("tool_choice") {
        if let Some(tc_type) = tc.get("type").and_then(|t| t.as_str()) {
            let openai_tc = match tc_type {
                "auto" => Value::String("auto".to_string()),
                "any" => Value::String("required".to_string()),
                "tool" => {
                    let name = tc.get("name").and_then(|n| n.as_str()).filter(|s| !s.is_empty())
                        .ok_or_else(|| "Anthropic tool_choice type 'tool' is missing a name".to_string())?;
                    serde_json::json!({
                        "type": "function",
                        "function": {"name": name}
                    })
                }
                _ => return Err("unsupported Anthropic tool_choice requires a native Anthropic Messages channel".to_string()),
            };
            openai_body["tool_choice"] = openai_tc;
        } else if let Some(s) = tc.as_str() {
            let openai_tc = match s {
                "auto" => Value::String("auto".to_string()),
                "any" => Value::String("required".to_string()),
                "tool" => return Err("Anthropic tool_choice 'tool' requires a name".to_string()),
                _ => {
                    return Err(
                        "unsupported Anthropic tool_choice requires a native Anthropic Messages channel"
                            .to_string(),
                    )
                }
            };
            openai_body["tool_choice"] = openai_tc;
        } else {
            return Err(
                "unsupported Anthropic tool_choice requires a native Anthropic Messages channel"
                    .to_string(),
            );
        }
    }

    if body
        .get("tool_choice")
        .and_then(|choice| choice.get("disable_parallel_tool_use"))
        .and_then(|v| v.as_bool())
        == Some(true)
    {
        openai_body["parallel_tool_calls"] = Value::Bool(false);
    }

    // Fail-open thinking mapping: Anthropic `thinking` / `output_config` →
    // OpenAI `reasoning_effort` (CPA semantics).  Only set when the downstream
    // asked for thinking; otherwise leave unset so the upstream applies its own
    // default.  `container`/`context_management`/`context_management_config`
    // have no Chat equivalent and were dropped above (fail-open).
    if let Some(effort) = anthropic_thinking_to_reasoning_effort(body) {
        openai_body["reasoning_effort"] = Value::String(effort);
    }

    Ok(openai_body)
}

/// Map an Anthropic `thinking` config to an OpenAI `reasoning_effort` value.
///
/// `None` when the downstream did not ask for thinking (or asked for an
/// unrecognized type), in which case `reasoning_effort` is left unset.
fn anthropic_thinking_to_reasoning_effort(body: &Value) -> Option<String> {
    let thinking = body.get("thinking")?;
    if !thinking.is_object() {
        return None;
    }
    let ty = thinking.get("type").and_then(Value::as_str)?;
    match ty {
        "enabled" => match thinking.get("budget_tokens").and_then(Value::as_i64) {
            Some(budget) => crate::protocol::thinking::budget_to_level(budget).map(String::from),
            None => Some("auto".to_string()),
        },
        "adaptive" | "auto" => match body
            .get("output_config")
            .and_then(|oc| oc.get("effort"))
            .and_then(Value::as_str)
        {
            Some(effort) if !effort.trim().is_empty() => Some(effort.trim().to_ascii_lowercase()),
            _ => Some("xhigh".to_string()),
        },
        "disabled" => Some("none".to_string()),
        _ => None,
    }
}

/// Estimate structured Anthropic request size for the optional count_tokens endpoint.
#[allow(dead_code)]
pub fn estimate_anthropic_input_tokens(body: &Value) -> u64 {
    fn estimate(value: &Value) -> u64 {
        match value {
            Value::String(text) => ((text.chars().count() as u64) + 3) / 4,
            Value::Array(values) => values.iter().map(estimate).sum(),
            Value::Object(object) => object
                .iter()
                // Image source data is base64, not prompt text. Counting it would
                // overestimate by orders of magnitude on OpenAI-only channels.
                .filter(|(key, _)| !matches!(key.as_str(), "model" | "stream" | "data"))
                .map(|(_, value)| estimate(value))
                .sum(),
            _ => 0,
        }
    }
    estimate(body).max(1)
}

fn tool_result_to_openai_content(block: &Value) -> Result<String, String> {
    match block.get("content") {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(items)) => {
            let mut text = String::new();
            for item in items {
                match item.get("type").and_then(|v| v.as_str()) {
                    Some("text") => text.push_str(item.get("text").and_then(|v| v.as_str()).unwrap_or("")),
                    Some("image") => return Err("tool_result images require a native Anthropic Messages channel".to_string()),
                    _ => return Err("unsupported tool_result content requires a native Anthropic Messages channel".to_string()),
                }
            }
            Ok(text)
        }
        _ => Err("tool_result content must be text or text blocks".to_string()),
    }
}

fn anthropic_system_content_to_openai_text(value: &Value) -> Result<String, String> {
    if let Some(str_val) = value.as_str() {
        Ok(str_val.to_string())
    } else if let Some(arr) = value.as_array() {
        let mut texts = Vec::new();
        for block in arr {
            // Prompt caching changes Anthropic billing/cache behavior but not
            // the text content of a Chat Completions request.  It is safe to
            // drop this annotation on the OpenAI bridge; native channels still
            // receive the original body unchanged.
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => texts.push(
                    block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string(),
                ),
                Some("thinking") => {
                    // Fail-open: reasoning instructions on the system prompt
                    // are dropped (no Chat equivalent), not rejected.
                }
                Some("cache_control") => {
                    return Err(
                        "system cache_control blocks require a native Anthropic Messages channel"
                            .to_string(),
                    )
                }
                _ => {
                    return Err(
                        "unsupported non-text system content requires a native Anthropic Messages channel"
                            .to_string(),
                    )
                }
            }
        }
        Ok(texts.join(""))
    } else {
        Err("system must be text or an array of text blocks".to_string())
    }
}

/// Convert Anthropic messages array to OpenAI messages array.
/// Anthropic content can be string or array of content blocks.
/// Handles: text, tool_use (assistant), tool_result (user)
fn convert_anthropic_messages_to_openai(
    messages: &Value,
    system: Option<String>,
) -> Result<Value, String> {
    let mut msgs = Vec::new();

    // Prepend system message if present
    if let Some(sys) = system {
        msgs.push(serde_json::json!({"role": "system", "content": sys}));
    }

    if let Some(arr) = messages.as_array() {
        for msg in arr {
            let role = msg
                .get("role")
                .and_then(|r| r.as_str())
                .ok_or_else(|| "Anthropic message is missing role".to_string())?
                .to_string();
            if role != "user" && role != "assistant" && role != "system" {
                return Err("only user, assistant, and system Anthropic messages can be sent to OpenAI Chat Completions".to_string());
            }

            if role == "system" {
                let content = msg
                    .get("content")
                    .ok_or_else(|| "system message is missing content".to_string())?;
                msgs.push(serde_json::json!({
                    "role": "system",
                    "content": anthropic_system_content_to_openai_text(content)?,
                }));
                continue;
            }

            if let Some(content_arr) = msg.get("content").and_then(|c| c.as_array()) {
                let mut parts: Vec<Value> = Vec::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                let mut assistant_reasoning = String::new();
                let flush_user_parts = |parts: &mut Vec<Value>, msgs: &mut Vec<Value>| {
                    if !parts.is_empty() {
                        msgs.push(
                            serde_json::json!({"role": "user", "content": std::mem::take(parts)}),
                        );
                    }
                };
                for block in content_arr {
                    // Cache controls are annotations on otherwise supported
                    // blocks.  Strip them instead of rejecting an entire
                    // OpenAI-only Claude Code request.
                    match block.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                        "text" => parts.push(serde_json::json!({"type": "text", "text": block.get("text").and_then(|t| t.as_str()).unwrap_or("")})),
                        "image" => {
                            if role != "user" { return Err("OpenAI Chat Completions cannot safely encode assistant image blocks".to_string()); }
                            let source = block.get("source").ok_or_else(|| "Anthropic image block is missing source".to_string())?;
                            let url = match source.get("type").and_then(|v| v.as_str()) {
                                Some("url") => source.get("url").and_then(|v| v.as_str()).ok_or_else(|| "Anthropic image URL source is missing url".to_string())?.to_string(),
                                Some("base64") => format!("data:{};base64,{}", source.get("media_type").and_then(|v| v.as_str()).ok_or_else(|| "Anthropic base64 image is missing media_type".to_string())?, source.get("data").and_then(|v| v.as_str()).ok_or_else(|| "Anthropic base64 image is missing data".to_string())?),
                                _ => return Err("unsupported Anthropic image source requires a native channel".to_string()),
                            };
                            parts.push(serde_json::json!({"type": "image_url", "image_url": {"url": url}}));
                        }
                        "tool_use" => {
                            if role != "assistant" { return Err("tool_use blocks must be in an assistant message".to_string()); }
                            let id = block.get("id").and_then(|i| i.as_str()).filter(|s| !s.is_empty()).ok_or_else(|| "tool_use is missing id".to_string())?;
                            let name = block.get("name").and_then(|n| n.as_str()).filter(|s| !s.is_empty()).ok_or_else(|| "tool_use is missing name".to_string())?;
                            let input = block.get("input").ok_or_else(|| "tool_use is missing input".to_string())?;
                            if !input.is_object() {
                                return Err("tool_use input must be a JSON object".to_string());
                            }
                            let input = input.clone();
                            tool_calls.push(serde_json::json!({"id": id, "type": "function", "function": {"name": name, "arguments": serde_json::to_string(&input).map_err(|e| e.to_string())?}}));
                        }
                        "tool_result" => {
                            if role != "user" { return Err("tool_result blocks must be in a user message".to_string()); }
                            flush_user_parts(&mut parts, &mut msgs);
                            let tool_use_id = block.get("tool_use_id").and_then(|t| t.as_str()).filter(|s| !s.is_empty()).ok_or_else(|| "tool_result is missing tool_use_id".to_string())?;
                            let result_content = tool_result_to_openai_content(block)?;
                            let is_error = block.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                            msgs.push(serde_json::json!({"role": "tool", "tool_call_id": tool_use_id, "content": if is_error { format!("Tool execution error:\n{}", result_content) } else { result_content }}));
                        }
                        "thinking" => {
                            // Fail-open: assistant reasoning is carried into
                            // the Chat message as `reasoning_content` (OpenAI
                            // non-stream field).  Reasoning on any other role is
                            // dropped — we never inject thinking into a
                            // user/system channel.
                            if role == "assistant" {
                                if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                                    assistant_reasoning.push_str(t);
                                }
                            }
                        }
                        "redacted_thinking" => {
                            // Encrypted/signature form — no usable text; drop.
                        }
                        "cache_control" => return Err("Anthropic cache controls require a native Anthropic Messages channel".to_string()),
                        _ => return Err("unsupported Anthropic content block requires a native Anthropic Messages channel".to_string()),
                    }
                }
                if role == "assistant" {
                    let content = if parts.is_empty() {
                        Value::Null
                    } else if parts
                        .iter()
                        .all(|part| part.get("type").and_then(|v| v.as_str()) == Some("text"))
                    {
                        Value::String(
                            parts
                                .iter()
                                .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                                .collect::<String>(),
                        )
                    } else {
                        Value::Array(parts)
                    };
                    // Reasoning content extracted from assistant `thinking`
                    // blocks (fail-open mapping to OpenAI `reasoning_content`).
                    let reasoning = if assistant_reasoning.is_empty() {
                        None
                    } else {
                        Some(assistant_reasoning)
                    };
                    if tool_calls.is_empty() && content.is_null() && reasoning.is_none() {
                        return Err("assistant message is empty".to_string());
                    }
                    let mut assistant =
                        serde_json::json!({"role": "assistant", "content": content});
                    if let Some(r) = reasoning {
                        assistant["reasoning_content"] = Value::String(r);
                    }
                    if !tool_calls.is_empty() {
                        assistant["tool_calls"] = Value::Array(tool_calls);
                    }
                    msgs.push(assistant);
                } else {
                    flush_user_parts(&mut parts, &mut msgs);
                }
            } else if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
                msgs.push(serde_json::json!({
                    "role": role,
                    "content": s.to_string(),
                }));
            } else {
                msgs.push(serde_json::json!({
                    "role": role,
                    "content": msg.get("content").cloned().unwrap_or(Value::String(String::new())),
                }));
            }
        }
    } else {
        return Err("Anthropic messages must be an array".to_string());
    }

    Ok(Value::Array(msgs))
}
