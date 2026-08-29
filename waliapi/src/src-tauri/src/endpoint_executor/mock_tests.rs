//! Per-preset mock upstream tests (T06 acceptance).
//!
//! No real paid endpoint is ever contacted: every test boots a local
//! `tokio::net::TcpListener` that captures the request (method, path+query,
//! headers, body) and serves a canned JSON / SSE response, then drives the real
//! executor against it.  This verifies final URL, auth header, body shape and
//! streaming passthrough for each preset without network access.

#![cfg(test)]

use crate::core::channel_identity::ChannelIdentity;
use crate::core::route_plan::EndpointKind;
use crate::db::models::Channel;
use crate::endpoint_executor::{
    dispatch_executor, dispatch_stream_executor, endpoint_path, final_url, StreamAttemptResult,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
struct CapturedRequest {
    method: String,
    path_and_query: String,
    headers: Vec<(String, String)>,
    body: String,
}

struct MockUpstream {
    addr: std::net::SocketAddr,
    received: Arc<Mutex<Vec<CapturedRequest>>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockUpstream {
    /// Boot a mock that responds with `response_body` (raw bytes) on every
    /// request.  `response_status` defaults to 200.
    async fn start(response_body: Vec<u8>, response_status: u16) -> MockUpstream {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let recv = received.clone();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let recv = recv.clone();
                let response_body = response_body.clone();
                let response_status = response_status;
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    // Read headers (up to the \r\n\r\n separator).
                    let mut header_end = None;
                    loop {
                        match socket.read(&mut tmp).await {
                            Ok(0) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&tmp[..n]);
                                if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                                    header_end = Some(pos);
                                    break;
                                }
                                if buf.len() > 1024 * 1024 {
                                    break;
                                }
                            }
                            Err(_) => return,
                        }
                    }
                    let Some(header_end) = header_end else { return };
                    let header_block = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let mut lines = header_block.split("\r\n");
                    let request_line = lines.next().unwrap_or("");
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or("").to_string();
                    let path_and_query = parts.next().unwrap_or("").to_string();
                    let mut headers = Vec::new();
                    let mut content_length = 0usize;
                    for line in lines {
                        if let Some((k, v)) = line.split_once(':') {
                            let k = k.trim().to_ascii_lowercase();
                            let v = v.trim().to_string();
                            if k == "content-length" {
                                content_length = v.parse().unwrap_or(0);
                            }
                            headers.push((k, v));
                        }
                    }
                    // Read body.
                    let body_start = header_end + 4;
                    while buf.len() < body_start + content_length {
                        match socket.read(&mut tmp).await {
                            Ok(0) => break,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                            Err(_) => return,
                        }
                    }
                    let body = String::from_utf8_lossy(
                        &buf[body_start..body_start + content_length.min(buf.len() - body_start)],
                    )
                    .to_string();
                    recv.lock().await.push(CapturedRequest {
                        method,
                        path_and_query,
                        headers,
                        body,
                    });
                    let reason = if response_status == 200 {
                        "OK"
                    } else {
                        "Error"
                    };
                    let _ = socket
                        .write_all(
                            format!(
                                "HTTP/1.1 {response_status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                response_body.len()
                            )
                            .as_bytes(),
                        )
                        .await;
                    let _ = socket.write_all(&response_body).await;
                });
            }
        });
        MockUpstream {
            addr,
            received,
            _handle: handle,
        }
    }

    async fn captured(&self) -> Vec<CapturedRequest> {
        self.received.lock().await.clone()
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Build the executor inputs: an identity derived from protocol + endpoints.
fn identity(protocol: &str, endpoints: &[&str], legacy_override: Option<&str>) -> ChannelIdentity {
    ChannelIdentity {
        protocol: protocol.to_string(),
        provider: "custom".to_string(),
        native_base_url: String::new(),
        native_endpoints: endpoints.iter().map(|e| e.to_string()).collect(),
        identity_revision: 1,
        legacy_executor_override: legacy_override.map(|s| s.to_string()),
        executor_kind: "chat_completions".to_string(),
        inferred: false,
    }
}

fn channel(base_url: &str, api_key: &str) -> Channel {
    Channel {
        id: "ch-test".into(),
        name: "test".into(),
        channel_type: "openai".into(),
        base_url: base_url.into(),
        api_key: api_key.into(),
        models: "[\"m\"]".into(),
        status: 1,
        priority: 1,
        weight: 1,
        config: "{}".into(),
        model_mapping: "{}".into(),
        timeout_secs: 10,
        protocol: Some("openai".into()),
        provider: Some("custom".into()),
        native_base_url: Some(base_url.into()),
        native_endpoints: Some("[\"chat_completions\"]".into()),
        preset_revision: Some("test".into()),
        identity_revision: 1,
        legacy_executor_override: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        last_test_at: None,
        last_test_ok: None,
    }
}

fn prepared(
    base: &str,
    protocol: &str,
    endpoint: &str,
    model: &str,
    body: Value,
    codec_version: Option<&str>,
) -> crate::core::attempt::PreparedAttempt {
    use crate::protocol::codec::{CodecRegistry, Protocol};

    // These executor tests construct attempts directly, bypassing route-plan
    // preparation. Mirror the production prepared handle so they exercise the
    // same decoder-factory path instead of an obsolete string dispatch.
    let protocol_codec = match endpoint {
        "embeddings" | "count_tokens" => None,
        _ => {
            let (downstream, upstream) = match codec_version {
                Some("chat_to_messages_v1") => (Protocol::Chat, Protocol::Messages),
                Some("messages_to_chat_v1") => (Protocol::Messages, Protocol::Chat),
                Some("chat_to_responses_v1") => (Protocol::Chat, Protocol::Responses),
                Some("messages_to_responses_v2") => (Protocol::Messages, Protocol::Responses),
                Some("responses_to_messages_v2") => (Protocol::Responses, Protocol::Messages),
                Some("responses_to_chat_v1") => (Protocol::Responses, Protocol::Chat),
                _ => match endpoint {
                    "messages" => (Protocol::Messages, Protocol::Messages),
                    "responses" => (Protocol::Responses, Protocol::Responses),
                    // Chat-compatible OpenAI and Ollama test endpoints share
                    // the typed identity decoder.
                    _ => (Protocol::Chat, Protocol::Chat),
                },
            };
            Some(
                CodecRegistry::prepare_pair(downstream, upstream, model, &body)
                    .expect("test attempt request must prepare")
                    .codec,
            )
        }
    };
    crate::core::attempt::PreparedAttempt {
        channel_id: "ch-test".into(),
        channel_name: "test".into(),
        upstream_type: "channel".into(),
        route_group: format!("{}_g1_native", endpoint),
        upstream_protocol: protocol.to_string(),
        upstream_endpoint: endpoint.to_string(),
        upstream_model: model.to_string(),
        native_base_url: base.to_string(),
        auth_provider: None,
        auth_non_stream_framing: None,
        codec_version: codec_version.map(|s| s.to_string()),
        prepared_codec: protocol_codec,
        encoded_body: body,
        conversion_report: None,
        is_retry: false,
        attempt_no: 1,
    }
}

fn openai_success() -> Vec<u8> {
    br#"{"id":"chatcmpl-1","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#.to_vec()
}

// ── OpenAI Chat ────────────────────────────────────────────────────────────

#[tokio::test]
async fn openai_chat_url_auth_body() {
    let mock = MockUpstream::start(openai_success(), 200).await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "chat_completions",
        "gpt-4o",
        json!({"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"stream":false}),
        None,
    );
    let id = identity("openai", &["chat_completions"], None);
    let ch = channel(&base, "sk-123");
    let result =
        dispatch_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None).await;
    let captured = mock.captured().await;
    assert_eq!(captured.len(), 1);
    let req = &captured[0];
    assert_eq!(req.method, "POST");
    assert_eq!(req.path_and_query, "/v1/chat/completions");
    assert!(req
        .headers
        .iter()
        .any(|(k, v)| k == "authorization" && v == "Bearer sk-123"));
    let body: Value = serde_json::from_str(&req.body).unwrap();
    assert_eq!(body["model"], "gpt-4o");
    match result {
        crate::core::attempt::AttemptResult::Success(s) => {
            assert_eq!(s.status, 200);
            assert_eq!(s.usage.as_ref().unwrap().total_tokens, 7);
        }
        _ => panic!("expected success"),
    }
}

/// A 2xx status alone is not a successful non-stream response.  Providers
/// occasionally close a fallback request without writing a body; retain that
/// as a retryable protocol failure and include enough transport context in the
/// persisted diagnostic to identify the upstream behaviour.
#[tokio::test]
async fn openai_chat_empty_2xx_body_is_diagnostic_protocol_failure() {
    let mock = MockUpstream::start(Vec::new(), 200).await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "chat_completions",
        "gpt-4o",
        json!({"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"stream":false}),
        Some("messages_to_chat_v1"),
    );
    let id = identity("openai", &["chat_completions"], None);
    let ch = channel(&base, "sk-123");

    let result =
        dispatch_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None).await;
    match result {
        crate::core::attempt::AttemptResult::Failure(failure) => {
            assert_eq!(
                failure.failure_class,
                crate::core::attempt::FailureClass::UpstreamProtocolError
            );
            assert!(failure.message.contains("HTTP 200, 0 bytes"));
            assert!(failure.message.contains("content-type application/json"));
            assert!(failure
                .message
                .contains("upstream returned an undecodable body"));
        }
        _ => panic!("an empty 2xx body must not become a successful response"),
    }
}

// ── OpenAI Responses (native passthrough) ─────────────────────────────────

#[tokio::test]
async fn openai_responses_url_and_passthrough() {
    let mock = MockUpstream::start(
        br#"{"id":"resp_1","object":"response","output":[],"model":"gpt-4o","status":"completed"}"#
            .to_vec(),
        200,
    )
    .await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "responses",
        "gpt-4o",
        json!({"model":"gpt-4o","input":"hi"}),
        None,
    );
    let id = identity("openai", &["responses"], None);
    let ch = channel(&base, "sk-123");
    let result = dispatch_executor(EndpointKind::Responses, &attempt, &ch, &id, &[], None).await;
    let req = &mock.captured().await[0];
    assert_eq!(req.path_and_query, "/v1/responses");
    match result {
        crate::core::attempt::AttemptResult::Success(s) => {
            // Native passthrough: body is the raw upstream Responses JSON.
            assert_eq!(s.body["object"], "response");
        }
        _ => panic!("expected success"),
    }
}

// ── Embeddings ────────────────────────────────────────────────────────────

#[tokio::test]
async fn embeddings_url_auth() {
    let mock = MockUpstream::start(
        br#"{"object":"list","data":[{"embedding":[0.1,0.2]}],"usage":{"prompt_tokens":4,"total_tokens":4}}"#.to_vec(),
        200,
    )
    .await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "embeddings",
        "text-embedding-3-small",
        json!({"model":"text-embedding-3-small","input":"hello"}),
        None,
    );
    let id = identity("openai", &["embeddings"], None);
    let ch = channel(&base, "sk-123");
    let result = dispatch_executor(EndpointKind::Embeddings, &attempt, &ch, &id, &[], None).await;
    let req = &mock.captured().await[0];
    assert_eq!(req.path_and_query, "/v1/embeddings");
    assert!(req
        .headers
        .iter()
        .any(|(k, v)| k == "authorization" && v == "Bearer sk-123"));
    match result {
        crate::core::attempt::AttemptResult::Success(s) => assert_eq!(s.status, 200),
        _ => panic!("expected success"),
    }
}

// ── Anthropic Messages ────────────────────────────────────────────────────

#[tokio::test]
async fn anthropic_messages_url_auth_version() {
    let mock = MockUpstream::start(
        br#"{"type":"message","id":"msg_1","role":"assistant","model":"claude-sonnet-4-6","content":[{"type":"text","text":"hi"}],"stop_reason":"end_turn","usage":{"input_tokens":4,"output_tokens":2}}"#.to_vec(),
        200,
    )
    .await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "anthropic",
        "messages",
        "claude-sonnet-4-6",
        json!({"model":"claude-sonnet-4-6","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}),
        None,
    );
    let id = identity("anthropic", &["messages"], None);
    let ch = channel(&base, "sk-ant-xyz");
    let result = dispatch_executor(EndpointKind::Messages, &attempt, &ch, &id, &[], None).await;
    let req = &mock.captured().await[0];
    assert_eq!(req.path_and_query, "/v1/messages");
    assert!(req
        .headers
        .iter()
        .any(|(k, v)| k == "x-api-key" && v == "sk-ant-xyz"));
    assert!(req
        .headers
        .iter()
        .any(|(k, v)| k == "anthropic-version" && v == "2023-06-01"));
    assert!(!req.headers.iter().any(|(k, _)| k == "authorization"));
    match result {
        crate::core::attempt::AttemptResult::Success(s) => assert_eq!(s.status, 200),
        _ => panic!("expected success"),
    }
}

// ── Anthropic Count Tokens ────────────────────────────────────────────────

#[tokio::test]
async fn anthropic_count_tokens_url() {
    let mock = MockUpstream::start(br#"{"input_tokens":7}"#.to_vec(), 200).await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "anthropic",
        "count_tokens",
        "claude-sonnet-4-6",
        json!({"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"hi"}]}),
        None,
    );
    let id = identity("anthropic", &["messages", "count_tokens"], None);
    let ch = channel(&base, "sk-ant-xyz");
    let result = dispatch_executor(EndpointKind::CountTokens, &attempt, &ch, &id, &[], None).await;
    let req = &mock.captured().await[0];
    assert_eq!(req.path_and_query, "/v1/messages/count_tokens");
    match result {
        crate::core::attempt::AttemptResult::Success(s) => assert_eq!(s.status, 200),
        _ => panic!("expected success"),
    }
}

/// A safe forward query string is appended to the final upstream URL.
#[tokio::test]
async fn anthropic_messages_forwards_safe_query() {
    let mock = MockUpstream::start(
        br#"{"type":"message","id":"msg_1","role":"assistant","model":"claude-sonnet-4-6","content":[{"type":"text","text":"hi"}],"stop_reason":"end_turn","usage":{"input_tokens":4,"output_tokens":2}}"#.to_vec(),
        200,
    )
    .await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "anthropic",
        "messages",
        "claude-sonnet-4-6",
        json!({"model":"claude-sonnet-4-6","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}),
        None,
    );
    let id = identity("anthropic", &["messages"], None);
    let ch = channel(&base, "sk-ant-xyz");
    let result = dispatch_executor(
        EndpointKind::Messages,
        &attempt,
        &ch,
        &id,
        &[],
        Some("beta=true&org=acme"),
    )
    .await;
    let req = &mock.captured().await[0];
    assert!(req.path_and_query.starts_with("/v1/messages?"));
    assert!(req.path_and_query.contains("beta=true"));
    assert!(req.path_and_query.contains("org=acme"));
    match result {
        crate::core::attempt::AttemptResult::Success(_) => {}
        _ => panic!("expected success"),
    }
}

// ── Ollama native /api/chat (empty key → no auth header) ──────────────────

#[tokio::test]
async fn ollama_api_chat_url_no_auth_when_key_empty() {
    let mock = MockUpstream::start(
        br#"{"model":"llama3.1","message":{"role":"assistant","content":"hi"},"done":true}"#
            .to_vec(),
        200,
    )
    .await;
    let base = format!("http://{}", mock.addr);
    let attempt = prepared(
        &base,
        "ollama",
        "api_chat",
        "llama3.1",
        json!({"model":"llama3.1","messages":[{"role":"user","content":"hi"}],"stream":false}),
        None,
    );
    let id = identity("ollama", &["api_chat"], None);
    let ch = channel(&base, "");
    let result =
        dispatch_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None).await;
    let req = &mock.captured().await[0];
    assert_eq!(req.path_and_query, "/api/chat");
    assert!(!req.headers.iter().any(|(k, _)| k == "authorization"));
    match result {
        crate::core::attempt::AttemptResult::Success(s) => assert_eq!(s.status, 200),
        _ => panic!("expected success"),
    }
}

#[tokio::test]
async fn ollama_api_chat_bearer_when_key_present() {
    let mock = MockUpstream::start(
        br#"{"model":"llama3.1","message":{"role":"assistant","content":"hi"},"done":true}"#
            .to_vec(),
        200,
    )
    .await;
    let base = format!("http://{}", mock.addr);
    let attempt = prepared(
        &base,
        "ollama",
        "api_chat",
        "llama3.1",
        json!({"model":"llama3.1","messages":[{"role":"user","content":"hi"}]}),
        None,
    );
    let id = identity("ollama", &["api_chat"], None);
    let ch = channel(&base, "proxy-key");
    let result =
        dispatch_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None).await;
    let req = &mock.captured().await[0];
    assert_eq!(req.path_and_query, "/api/chat");
    assert!(req
        .headers
        .iter()
        .any(|(k, v)| k == "authorization" && v == "Bearer proxy-key"));
    match result {
        crate::core::attempt::AttemptResult::Success(_) => {}
        _ => panic!("expected success"),
    }
}

// ── Legacy Gemini override ────────────────────────────────────────────────

#[tokio::test]
async fn gemini_override_url_query_key_and_conversion() {
    let mock = MockUpstream::start(
        br#"{"candidates":[{"content":{"parts":[{"text":"hi gemini"}]}}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}"#.to_vec(),
        200,
    )
    .await;
    let base = format!("http://{}", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "chat_completions",
        "gemini-2.0-flash",
        json!({"model":"gemini-2.0-flash","messages":[{"role":"user","content":"hi"}],"stream":false}),
        None,
    );
    let id = identity("openai", &[], Some("gemini_native"));
    let ch = channel(&base, "gkey-123");
    let result =
        dispatch_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None).await;
    let req = &mock.captured().await[0];
    assert!(req.path_and_query.contains(":generateContent"));
    assert!(req.path_and_query.contains("key=gkey-123"));
    let body: Value = serde_json::from_str(&req.body).unwrap();
    assert_eq!(body["contents"][0]["parts"][0]["text"], "hi");
    // Response is converted back to OpenAI Chat.
    match result {
        crate::core::attempt::AttemptResult::Success(s) => {
            assert_eq!(s.body["choices"][0]["message"]["content"], "hi gemini");
        }
        _ => panic!("expected success"),
    }
}

// ── Streaming: OpenAI Chat SSE raw passthrough ────────────────────────────

#[tokio::test]
async fn openai_stream_native_passthrough() {
    let sse = b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\ndata: [DONE]\n\n";
    let mock = MockUpstream::start(sse.to_vec(), 200).await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "chat_completions",
        "gpt-4o",
        json!({"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"stream":true}),
        None,
    );
    let id = identity("openai", &["chat_completions"], None);
    let ch = channel(&base, "sk-123");
    let result =
        dispatch_stream_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None)
            .await;
    let StreamAttemptResult::Connected(upstream) = result else {
        panic!("expected connected")
    };
    // First-frame validation happens in the driver; here we just confirm the
    // executor returned a connected stream with the raw body.
    let mut collected = Vec::new();
    let mut stream = upstream.body;
    use futures_util::StreamExt;
    while let Some(Ok(chunk)) = stream.next().await {
        collected.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&collected);
    assert!(text.contains("data: {"));
    assert!(text.contains("[DONE]"));
    let req = &mock.captured().await[0];
    assert_eq!(req.path_and_query, "/v1/chat/completions");
    let body: Value = serde_json::from_str(&req.body).unwrap();
    assert_eq!(body["stream"], true);
}

// ── Error fidelity ────────────────────────────────────────────────────────

#[tokio::test]
async fn upstream_401_is_channel_auth_not_local_key_error() {
    let mock =
        MockUpstream::start(br#"{"error":{"message":"invalid api key"}}"#.to_vec(), 401).await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "chat_completions",
        "gpt-4o",
        json!({"model":"gpt-4o","messages":[]}),
        None,
    );
    let id = identity("openai", &["chat_completions"], None);
    let ch = channel(&base, "sk-wrong");
    let result =
        dispatch_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None).await;
    match result {
        crate::core::attempt::AttemptResult::Failure(f) => {
            assert_eq!(
                f.failure_class,
                crate::core::attempt::FailureClass::ChannelAuthTerminal
            );
            // The downstream status must be 502 (gateway holds channel creds),
            // NOT a 401 that blames the caller's key.
            assert_eq!(f.status_code, Some(502));
        }
        _ => panic!("expected failure"),
    }
}

#[tokio::test]
async fn upstream_404_model_not_found_is_not_endpoint_unsupported() {
    let mock = MockUpstream::start(
        br#"{"error":{"message":"model 'gpt-4o' not found"}}"#.to_vec(),
        404,
    )
    .await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "chat_completions",
        "gpt-4o",
        json!({"model":"gpt-4o","messages":[]}),
        None,
    );
    let id = identity("openai", &["chat_completions"], None);
    let ch = channel(&base, "sk-123");
    let result =
        dispatch_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None).await;
    match result {
        crate::core::attempt::AttemptResult::Failure(f) => {
            assert_eq!(
                f.failure_class,
                crate::core::attempt::FailureClass::Retryable
            );
        }
        _ => panic!("expected failure"),
    }
}

#[tokio::test]
async fn upstream_404_proven_path_missing_is_endpoint_unsupported() {
    let mock = MockUpstream::start(
        br#"{"error":{"message":"endpoint does not exist"}}"#.to_vec(),
        404,
    )
    .await;
    let base = format!("http://{}/v1", mock.addr);
    let attempt = prepared(
        &base,
        "openai",
        "responses",
        "gpt-4o",
        json!({"model":"gpt-4o","input":"hi"}),
        None,
    );
    let id = identity("openai", &["responses"], None);
    let ch = channel(&base, "sk-123");
    let result = dispatch_executor(EndpointKind::Responses, &attempt, &ch, &id, &[], None).await;
    match result {
        crate::core::attempt::AttemptResult::Failure(f) => {
            assert_eq!(
                f.failure_class,
                crate::core::attempt::FailureClass::EndpointUnsupported
            );
        }
        _ => panic!("expected failure"),
    }
}

// ── Conversion: Chat G2 (downstream Chat, upstream Messages) ──────────────

#[tokio::test]
async fn chat_to_messages_conversion_non_stream() {
    let mock = MockUpstream::start(
        br#"{"type":"message","id":"msg_1","role":"assistant","model":"claude-sonnet-4-6","content":[{"type":"text","text":"hello from claude"}],"stop_reason":"end_turn","usage":{"input_tokens":5,"output_tokens":3}}"#.to_vec(),
        200,
    )
    .await;
    let base = format!("http://{}", mock.addr);
    let attempt = prepared(
        &base,
        "anthropic",
        "messages",
        "claude-sonnet-4-6",
        json!({"model":"claude-sonnet-4-6","max_tokens":64,"messages":[{"role":"user","content":"hi"}],"stream":false}),
        Some("chat_to_messages_v1"),
    );
    let id = identity("anthropic", &["messages"], None);
    let ch = channel(&base, "sk-ant-xyz");
    let result =
        dispatch_executor(EndpointKind::ChatCompletions, &attempt, &ch, &id, &[], None).await;
    match result {
        crate::core::attempt::AttemptResult::Success(s) => {
            // The upstream Messages body is decoded back into OpenAI Chat.
            assert_eq!(s.body["object"], "chat.completion");
            assert_eq!(
                s.body["choices"][0]["message"]["content"],
                "hello from claude"
            );
            assert_eq!(s.body["model"], "claude-sonnet-4-6");
        }
        _ => panic!("expected success"),
    }
}

#[test]
fn final_url_matrix_is_covered() {
    assert_eq!(
        final_url("https://api.openai.com/v1", "chat/completions", None),
        "https://api.openai.com/v1/chat/completions"
    );
    assert_eq!(
        final_url("https://api.openai.com/v1", "responses", None),
        "https://api.openai.com/v1/responses"
    );
    assert_eq!(
        final_url("https://api.anthropic.com/v1", "/messages", None),
        "https://api.anthropic.com/v1/messages"
    );
    assert_eq!(
        final_url("http://localhost:11434", "api/chat", None),
        "http://localhost:11434/api/chat"
    );
    assert_eq!(
        endpoint_path("anthropic", "count_tokens"),
        "/messages/count_tokens"
    );
    assert_eq!(endpoint_path("openai", "embeddings"), "embeddings");
}
