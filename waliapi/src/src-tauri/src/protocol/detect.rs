use serde_json::Value;

/// Extract API key from either `Authorization: Bearer xxx` or `x-api-key: xxx` header.
pub fn extract_api_key(headers: &axum::http::HeaderMap) -> Option<String> {
    // Try Authorization: Bearer xxx first
    if let Some(auth) = headers.get("authorization").and_then(|h| h.to_str().ok()) {
        if let Some(key) = auth.strip_prefix("Bearer ") {
            let trimmed = key.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    // Fall back to x-api-key
    if let Some(key) = headers.get("x-api-key").and_then(|h| h.to_str().ok()) {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Detect if a request is in Anthropic format by checking headers and body.
#[allow(dead_code)]
pub fn is_anthropic_request(headers: &axum::http::HeaderMap, body: &Value) -> bool {
    // Check for anthropic-version header
    if headers.contains_key("anthropic-version") {
        return true;
    }
    // Check for x-api-key without Authorization Bearer
    if headers.contains_key("x-api-key") && !headers.contains_key("authorization") {
        return true;
    }
    // Check body: Anthropic format uses "max_tokens" but not "messages" with OpenAI structure
    // Actually both use "messages", so rely on headers primarily.
    // As a fallback, check if body has "max_tokens" but not "model" (unlikely to help).
    // The header-based detection is the primary signal.
    let _ = body;
    false
}

/// Detect if a request targets the Responses API format.
#[allow(dead_code)]
pub fn is_responses_request(body: &Value) -> bool {
    // Responses API uses "input" instead of "messages"
    body.get("input").is_some() && body.get("messages").is_none()
}
