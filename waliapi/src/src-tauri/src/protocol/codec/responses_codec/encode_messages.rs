use super::super::error::UnsupportedFeatures;
use super::super::report::ConversionContext;
use super::super::{chat, messages};
use super::encode_chat::encode_chat_to_responses;
use serde_json::Value;

/// Messages → Chat → Responses.  The first encoder remains the authoritative
/// validator for Anthropic-specific fields.
pub fn encode_messages_to_responses(
    body: &Value,
    model: &str,
) -> Result<(Value, ConversionContext), UnsupportedFeatures> {
    let (mut chat, _) = messages::encode_messages_to_chat(body, model)?;
    // The Messages→Chat leg synthesizes `stream_options.include_usage=true`
    // for a Chat upstream.  A Codex Responses account always emits usage in
    // `response.completed` and its strict backend allow-list intentionally has
    // no `stream_options`, so carrying that synthetic field would reject an
    // otherwise valid Messages stream before any account request is sent.
    if let Some(object) = chat.as_object_mut() {
        object.remove("stream_options");
    }
    encode_chat_to_responses(&chat, model)
}

/// Responses → Chat → Messages composition (V5 `responses_to_messages_v1`).
///
/// The first leg (`responses_to_openai`) is the authoritative Responses
/// validator; the second leg (`encode_chat_to_messages`) is the authoritative
/// Chat→Messages validator, which already maps `reasoning_effort` to Anthropic
/// thinking.  This wrapper raises the downstream `max_tokens` cap to 32000 when
/// the Responses request did not carry `max_output_tokens` (the legacy
/// `responses_via_chat` path keeps its 4096 default), and records the
/// codex-only top-level fields that have no Chat representation in the
/// ConversionReport.
///
/// Exercised only by the unit tests below; the direction strategies wire the
/// direct encoders, so non-test builds flag it as dead.
#[cfg_attr(not(test), allow(dead_code))]
pub fn encode_responses_to_messages(
    body: &Value,
    model: &str,
) -> Result<(Value, ConversionContext), UnsupportedFeatures> {
    let mut chat = crate::protocol::responses_to_openai(body)?;
    // V5 output cap: only override when the Responses request did not specify
    // one. A caller-supplied `max_output_tokens` is respected as-is (it was
    // already mapped to `max_tokens` by `responses_to_openai`).
    if body.get("max_output_tokens").is_none() {
        if let Some(object) = chat.as_object_mut() {
            object.insert("max_tokens".to_owned(), Value::from(32000u64));
        }
    }
    let (claude, mut context) = chat::encode_chat_to_messages(&chat, model)?;
    // Codex Responses fields with no Chat representation: dropped by
    // `responses_to_openai`; surface the drop in the ConversionReport.
    const DROPPED: &[&str] = &[
        "parallel_tool_calls",
        "store",
        "include",
        "prompt_cache_key",
        "client_metadata",
    ];
    if let Some(object) = body.as_object() {
        for key in DROPPED {
            if object.contains_key(*key) {
                context.normalized.push(format!("/{key}"));
            }
        }
    }
    Ok((claude, context))
}
