use super::*;
use async_trait::async_trait;

pub struct ClaudeAdaptor;

#[async_trait]
impl Adaptor for ClaudeAdaptor {
    fn channel_type(&self) -> &'static str {
        "claude"
    }
    fn default_models(&self) -> Vec<&'static str> {
        vec![
            "claude-sonnet-4-20250514",
            "claude-3-7-sonnet-20250219",
            "claude-3-5-haiku-20241022",
        ]
    }
    fn default_base_url(&self) -> &str {
        "https://api.anthropic.com/v1"
    }

    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error> {
        let start = std::time::Instant::now();
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": config.models.first().map(|s| s.as_str()).unwrap_or("claude-3-5-haiku-20241022"),
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let client = reqwest::Client::new();
        match client
            .post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(r) => {
                let latency = start.elapsed().as_millis() as u64;
                let status = r.status();
                if status.is_success() {
                    Ok(TestResult {
                        success: true,
                        message: "连接成功".to_string(),
                        latency_ms: latency,
                    })
                } else {
                    let body = r.text().await.unwrap_or_default();
                    let err_msg = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| {
                            v.get("error")
                                .and_then(|e| e.get("message"))
                                .and_then(|m| m.as_str())
                                .map(String::from)
                        })
                        .unwrap_or(body);
                    Ok(TestResult {
                        success: false,
                        message: format!("HTTP {} {}", status.as_u16(), err_msg),
                        latency_ms: latency,
                    })
                }
            }
            Err(e) => Ok(TestResult {
                success: false,
                message: format!("连接失败: {}", e),
                latency_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }

    async fn forward(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<(u16, serde_json::Value, Option<TokenUsage>), anyhow::Error> {
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));
        let openai_body = &request.body;

        // Convert OpenAI format to Claude format
        let model = openai_body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("claude-3-5-haiku-20241022");
        let messages = openai_body
            .get("messages")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));
        let max_tokens = openai_body
            .get("max_tokens")
            .and_then(|m| m.as_u64())
            .unwrap_or(4096);
        let temperature = openai_body.get("temperature").cloned();
        let stream = openai_body
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        // Extract system message if present
        let (system, claude_messages) = convert_openai_messages_to_claude(&messages);

        let mut claude_body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": claude_messages,
            "stream": stream,
        });
        let claude_tools = convert_openai_tools_to_claude(
            openai_body.get("tools").unwrap_or(&serde_json::Value::Null),
        );
        if let Some(arr) = claude_tools.as_array() {
            if !arr.is_empty() {
                claude_body["tools"] = claude_tools;
            }
        }
        if let Some(sys) = system {
            claude_body["system"] = serde_json::Value::String(sys);
        }
        if let Some(temp) = temperature {
            claude_body["temperature"] = temp;
        }

        let client = crate::adaptor::blocking_client(config.timeout_secs);
        let resp = client
            .post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&claude_body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let claude_json: serde_json::Value = resp.json().await?;;

        // Convert Claude response to OpenAI format
        let openai_response = convert_claude_to_openai(&claude_json, model);
        let usage = openai_response.get("usage").and_then(|u| {
            Some(TokenUsage {
                prompt_tokens: u.get("prompt_tokens")?.as_u64()?,
                completion_tokens: u.get("completion_tokens")?.as_u64()?,
                total_tokens: u.get("total_tokens")?.as_u64()?,
            })
        });

        Ok((status, openai_response, usage))
    }

    async fn forward_stream(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<reqwest::Response, anyhow::Error> {
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));
        let openai_body = &request.body;
        let model = openai_body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("claude-3-5-haiku-20241022");
        let messages = openai_body
            .get("messages")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));
        let max_tokens = openai_body
            .get("max_tokens")
            .and_then(|m| m.as_u64())
            .unwrap_or(4096);
        let (system, claude_messages) = convert_openai_messages_to_claude(&messages);

        let mut claude_body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": claude_messages,
            "stream": true,
        });
        let claude_tools = convert_openai_tools_to_claude(
            openai_body.get("tools").unwrap_or(&serde_json::Value::Null),
        );
        if let Some(arr) = claude_tools.as_array() {
            if !arr.is_empty() {
                claude_body["tools"] = claude_tools;
            }
        }
        if let Some(sys) = system {
            claude_body["system"] = serde_json::Value::String(sys);
        }

        let client = crate::adaptor::streaming_client();
        let resp = client
            .post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&claude_body)
            .send()
            .await?;

        Ok(resp)
    }
}

/// Convert OpenAI Chat messages to Anthropic Messages format.
///
/// Handles the three shapes that tool-using sessions produce:
/// - `system` → extracted `system` string (deduplicated, last one wins);
/// - `assistant` with `tool_calls` → a content ARRAY of `text` + `tool_use`
///   blocks (Anthropic requires this shape for tool calls — a bare string
///   content cannot carry `tool_use`);
/// - `tool` role → `user` + `tool_result` block (Anthropic has no `tool`
///   role; the result is a `tool_result` content block on a `user` message,
///   paired to the originating call via `tool_use_id`).
fn convert_openai_messages_to_claude(
    messages: &serde_json::Value,
) -> (Option<String>, serde_json::Value) {
    let msgs = match messages.as_array() {
        Some(arr) => arr,
        None => return (None, serde_json::Value::Array(vec![])),
    };

    let mut system = None;
    let mut claude_msgs = Vec::new();

    for msg in msgs {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");

        if role == "system" {
            if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
                system = Some(s.to_string());
            }
            continue;
        }

        // assistant: text content (optional) + tool_calls (optional) →
        // content array of text + tool_use blocks.
        if role == "assistant" {
            let tool_calls = msg
                .get("tool_calls")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default();
            let content = msg
                .get("content")
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            let mut blocks: Vec<serde_json::Value> = Vec::new();

            // Collapse text content (string, or array of {type:"text"}) to one
            // text block so it can sit beside tool_use blocks.
            let text = match &content {
                serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
                serde_json::Value::Null => None,
                serde_json::Value::Array(arr) => {
                    let mut t = String::new();
                    for block in arr {
                        if block.get("type").and_then(|x| x.as_str()) == Some("text") {
                            if let Some(s) = block.get("text").and_then(|x| x.as_str()) {
                                t.push_str(s);
                            }
                        }
                    }
                    if t.is_empty() {
                        None
                    } else {
                        Some(t)
                    }
                }
                _ => None,
            };
            if let Some(t) = text {
                blocks.push(serde_json::json!({"type": "text", "text": t}));
            }

            for tc in &tool_calls {
                let id = tc
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                // Skip malformed tool calls rather than sending an invalid block.
                if id.is_empty() || name.is_empty() {
                    continue;
                }
                let raw_args = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("{}");
                let input: serde_json::Value =
                    serde_json::from_str(raw_args).unwrap_or(serde_json::json!({}));
                blocks.push(serde_json::json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                }));
            }

            // Skip assistant messages with neither text nor tool calls.
            if blocks.is_empty() {
                continue;
            }
            claude_msgs.push(serde_json::json!({
                "role": "assistant",
                "content": blocks,
            }));
            continue;
        }

        // tool result → user + tool_result block.
        if role == "tool" {
            let tool_use_id = msg
                .get("tool_call_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if tool_use_id.is_empty() {
                continue;
            }
            let tool_content = match msg
                .get("content")
                .cloned()
                .unwrap_or(serde_json::Value::Null)
            {
                serde_json::Value::String(s) => s,
                serde_json::Value::Null => String::new(),
                serde_json::Value::Array(arr) => arr
                    .iter()
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(""),
                other => other.to_string(),
            };
            claude_msgs.push(serde_json::json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": tool_content,
                }],
            }));
            continue;
        }

        // plain user message.
        let content = msg
            .get("content")
            .cloned()
            .unwrap_or(serde_json::Value::String(String::new()));
        claude_msgs.push(serde_json::json!({
            "role": "user",
            "content": content,
        }));
    }

    (system, serde_json::Value::Array(claude_msgs))
}

/// Convert Chat Completions `tools` (nested `{type:"function",function:{...}}`)
/// to Anthropic Messages `tools` (flat `{name, description, input_schema}`).
/// Non-function tools (namespace / web_search / built-in) are dropped —
/// Anthropic only supports function tools.
fn convert_openai_tools_to_claude(tools: &serde_json::Value) -> serde_json::Value {
    let arr = match tools.as_array() {
        Some(a) => a,
        None => return serde_json::Value::Array(vec![]),
    };
    let claude_tools: Vec<serde_json::Value> = arr
        .iter()
        .filter_map(|t| {
            if t.get("type").and_then(|x| x.as_str()) != Some("function") {
                return None;
            }
            let f = t.get("function").unwrap_or(&serde_json::Value::Null);
            let name = f.get("name").and_then(|x| x.as_str()).unwrap_or("");
            if name.is_empty() {
                return None;
            }
            let mut tool = serde_json::json!({
                "name": name,
                "input_schema": f.get("parameters").cloned().unwrap_or_else(|| {
                    serde_json::json!({"type": "object", "properties": {}})
                }),
            });
            if let Some(desc) = f.get("description").and_then(|d| d.as_str()) {
                if !desc.is_empty() {
                    tool["description"] = serde_json::Value::String(desc.to_string());
                }
            }
            Some(tool)
        })
        .collect();
    serde_json::Value::Array(claude_tools)
}

fn convert_claude_to_openai(claude_json: &serde_json::Value, model: &str) -> serde_json::Value {
    let content_blocks = claude_json
        .get("content")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let content = content_blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");

    // Anthropic `tool_use` blocks → OpenAI `tool_calls` on the assistant
    // message, so tool-using responses survive the non-streaming conversion.
    let tool_calls: Vec<serde_json::Value> = content_blocks
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .enumerate()
        .map(|(i, block)| {
            serde_json::json!({
                "id": block.get("id").cloned().unwrap_or_else(|| {
                    serde_json::Value::String(format!("call_{}", i))
                }),
                "type": "function",
                "function": {
                    "name": block.get("name").cloned().unwrap_or(serde_json::Value::Null),
                    "arguments": block
                        .get("input")
                        .cloned()
                        .unwrap_or(serde_json::json!({}))
                        .to_string(),
                }
            })
        })
        .collect();

    let prompt_tokens = claude_json
        .get("usage")
        .and_then(|u| u.get("input_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    let completion_tokens = claude_json
        .get("usage")
        .and_then(|u| u.get("output_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);

    let mut message = serde_json::json!({
        "role": "assistant",
        "content": content,
    });
    if !tool_calls.is_empty() {
        message["tool_calls"] = serde_json::Value::Array(tool_calls);
    }
    // stop_reason tool_use → finish_reason tool_calls
    let finish_reason =
        if claude_json.get("stop_reason").and_then(|s| s.as_str()) == Some("tool_use") {
            "tool_calls"
        } else {
            "stop"
        };

    serde_json::json!({
        "id": claude_json.get("id").cloned().unwrap_or(serde_json::Value::String("chatcmpl-converted".to_string())),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason,
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_assistant_tool_calls_become_tool_use_blocks() {
        let messages = serde_json::json!([
            {"role": "system", "content": "You are Codex."},
            {"role": "user", "content": "git status"},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_A", "type": "function", "function": {"name": "exec_command", "arguments": "{\"cmd\":\"git status\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "call_A", "content": " M src-tauri/src/adaptor/claude.rs"}
        ]);
        let (system, claude_msgs) = convert_openai_messages_to_claude(&messages);
        assert_eq!(system.as_deref(), Some("You are Codex."));
        let msgs = claude_msgs.as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        // assistant: content ARRAY with tool_use block
        assert_eq!(msgs[1]["role"], "assistant");
        let assistant_content = msgs[1]["content"].as_array().unwrap();
        assert_eq!(assistant_content[0]["type"], "tool_use");
        assert_eq!(assistant_content[0]["id"], "call_A");
        assert_eq!(assistant_content[0]["name"], "exec_command");
        assert_eq!(assistant_content[0]["input"]["cmd"], "git status");
        // tool result: user + tool_result block paired by tool_use_id
        assert_eq!(msgs[2]["role"], "user");
        let tool_result = msgs[2]["content"][0].clone();
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "call_A");
        assert_eq!(tool_result["content"], " M src-tauri/src/adaptor/claude.rs");
    }

    #[test]
    fn messages_text_tool_call_mixed_content() {
        let messages = serde_json::json!([
            {"role": "assistant", "content": "Let me look.", "tool_calls": [
                {"id": "call_A", "type": "function", "function": {"name": "read", "arguments": "{\"path\":\"/tmp/x\"}"}}
            ]}
        ]);
        let (_, claude_msgs) = convert_openai_messages_to_claude(&messages);
        let msgs = claude_msgs.as_array().unwrap();
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Let me look.");
        assert_eq!(content[1]["type"], "tool_use");
    }

    #[test]
    fn tools_nested_to_claude_flat_and_drops_non_function() {
        let tools = serde_json::json!([
            {"type": "function", "function": {"name": "exec_command", "description": "Run a shell command", "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}}}},
            {"type": "namespace", "name": "multi_agent_v1", "namespace": "multi_agent_v1"},
            {"type": "web_search", "name": "web_search", "external_web_access": {}}
        ]);
        let claude_tools = convert_openai_tools_to_claude(&tools);
        let arr = claude_tools.as_array().unwrap();
        assert_eq!(
            arr.len(),
            1,
            "namespace/web_search must be dropped for Anthropic"
        );
        assert_eq!(arr[0]["name"], "exec_command");
        assert_eq!(arr[0]["description"], "Run a shell command");
        assert_eq!(
            arr[0]["input_schema"]["properties"]["cmd"]["type"],
            "string"
        );
    }

    #[test]
    fn claude_response_tool_use_becomes_openai_tool_calls() {
        let claude_json = serde_json::json!({
            "id": "msg_1",
            "content": [
                {"type": "text", "text": "Running now."},
                {"type": "tool_use", "id": "toolu_1", "name": "exec_command", "input": {"cmd": "git status"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });
        let openai = convert_claude_to_openai(&claude_json, "claude-3-5");
        let choice = &openai["choices"][0];
        assert_eq!(choice["finish_reason"], "tool_calls");
        let message = &choice["message"];
        assert_eq!(message["content"], "Running now.");
        let tc = &message["tool_calls"][0];
        assert_eq!(tc["id"], "toolu_1");
        assert_eq!(tc["function"]["name"], "exec_command");
        let args: serde_json::Value =
            serde_json::from_str(tc["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["cmd"], "git status");
    }
}
