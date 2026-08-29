//! Request feature collection for routing/codec pre-checks (T03).
//!
//! These features are derived from the ORIGINAL downstream protocol JSON full
//! tree, before any conversion, so a codec/router can prove coverage of
//! Responses built-in tools, image URLs, file metadata, unknown content blocks
//! and unknown top-level fields — even when the converter would otherwise
//! skip or compress them.
//!
//! Base64 attachments are audited as metadata only (media type, declared
//! length, actual length, SHA-256).  The payload body is never scanned as
//! ordinary text and never persisted.

use super::gate::{Base64AttachmentMeta, DownstreamProtocol};
use super::scanner::ScanBudget;
use sha2::{Digest, Sha256};
use std::time::Instant;

/// Cap on collected routing/pre-check items per category.  Feature collection
/// is advisory metadata for a router/codec, not the security verdict — a
/// hostile tree must not be able to grow unbounded vectors.
const MAX_FEATURE_ITEMS: usize = 64;
/// Independent per-attachment cap for base64 payload SHA-256 hashing.  Hashing
/// is O(n) and must never be a DoS vector: above this cap the payload is
/// length-measured only (O(1)) and hashing is skipped.
pub const MAX_BASE64_HASH_BYTES: usize = 8 * 1024 * 1024;

/// Collect routing/codec pre-check features from the ORIGINAL protocol JSON.
///
/// `budget` is the SAME cumulative budget the gate scanner uses, so feature
/// collection cannot be driven past the request's byte/string-node/depth/time
/// budgets by a hostile tree (review finding 3).
pub fn collect_features(
    value: &serde_json::Value,
    protocol: DownstreamProtocol,
    safe_forward_headers: &[(String, String)],
    budget: &ScanBudget,
) -> super::gate::RequestFeatures {
    let mut features = super::gate::RequestFeatures::default();
    // Unknown top-level fields are traceable so a router can reject (or
    // preserve) them before upstream instead of silently dropping them.
    if let serde_json::Value::Object(map) = value {
        for (k, v) in map {
            if features.unknown_fields.len() >= MAX_FEATURE_ITEMS {
                break;
            }
            if !known_top_level_field(protocol, k) {
                if let Some(t) = v.as_str() {
                    if t.is_empty() {
                        features.unknown_fields.push(format!("{k} (empty)"));
                    } else {
                        features.unknown_fields.push(k.clone());
                    }
                } else {
                    features.unknown_fields.push(k.clone());
                }
            }
        }
    }
    let mut ctx = FeatureWalkCtx::new(budget);
    walk_features(value, "$", &mut features, &mut ctx);
    features.truncated = ctx.truncated;
    features.beta_headers = safe_forward_headers
        .iter()
        .filter(|(name, _)| name.to_ascii_lowercase().starts_with("anthropic-beta"))
        .map(|(name, value)| format!("{name}: {value}"))
        .collect();
    features.has_tools = has_tools(value);
    features
}

/// Top-level fields each downstream protocol accepts.  Unknown fields must be
/// preserved or rejected before upstream — they must never be silently
/// dropped (T00 decision 8 / T03 spec).
fn known_top_level_field(protocol: DownstreamProtocol, key: &str) -> bool {
    let common = [
        "model",
        "stream",
        "stream_options",
        "temperature",
        "top_p",
        "n",
        "stop",
        "max_tokens",
        "max_completion_tokens",
        "presence_penalty",
        "frequency_penalty",
        "logit_bias",
        "user",
        "seed",
        "extra_body",
        "metadata",
        "store",
        "reasoning",
        "parallel_tool_calls",
        "tool_choice",
        "tools",
        "response_format",
        "timeout",
        "trace_id",
        "route_group",
    ];
    if common.contains(&key) {
        return true;
    }
    match protocol {
        DownstreamProtocol::ChatCompletions => matches!(
            key,
            "messages"
                | "function_call"
                | "functions"
                | "logprobs"
                | "top_logprobs"
                | "modalities"
                | "audio"
                | "service_tier"
        ),
        DownstreamProtocol::Completions => matches!(
            key,
            "prompt" | "best_of" | "echo" | "logprobs" | "suffix" | "max_tokens"
        ),
        DownstreamProtocol::Responses => matches!(
            key,
            "input"
                | "instructions"
                | "previous_response_id"
                | "include"
                | "text"
                | "output"
                | "tools"
                | "builtin_tool"
                | "file_search"
                | "web_search"
                | "code_interpreter"
                | "computer_use"
                | "truncation"
                | "dimensions"
                | "store"
                | "parallel_tool_calls"
                | "reasoning"
        ),
        DownstreamProtocol::Messages | DownstreamProtocol::CountTokens => matches!(
            key,
            "messages"
                | "system"
                | "max_tokens"
                | "stop_sequences"
                | "temperature"
                | "top_p"
                | "top_k"
                | "metadata"
                | "tools"
                | "tool_choice"
                | "thinking"
                | "betas"
                | "service_tier"
        ),
        DownstreamProtocol::Embeddings => matches!(
            key,
            "input" | "encoding_format" | "dimensions" | "input_type"
        ),
        DownstreamProtocol::Images => matches!(
            key,
            "prompt" | "n" | "size" | "quality" | "style" | "response_format" | "user"
        ),
        DownstreamProtocol::Audio => matches!(
            key,
            "file"
                | "model"
                | "language"
                | "prompt"
                | "response_format"
                | "temperature"
                | "input"
                | "voice"
                | "speed"
                | "instructions"
        ),
    }
}

/// Bounded feature walk.  Runs under the same [`ScanBudget`] the gate scanner
/// uses so a hostile tree cannot drive unbounded recursion or unbounded vector
/// growth.  On over-budget the walk stops and [`RequestFeatures::truncated`] is
/// set (advisory only; the gate's own fail-closed scan is authoritative).
struct FeatureWalkCtx<'a> {
    budget: &'a ScanBudget,
    bytes_visited: usize,
    string_nodes: usize,
    depth: usize,
    started: Instant,
    truncated: bool,
}

impl<'a> FeatureWalkCtx<'a> {
    fn new(budget: &'a ScanBudget) -> Self {
        Self {
            budget,
            bytes_visited: 0,
            string_nodes: 0,
            depth: 0,
            started: Instant::now(),
            truncated: false,
        }
    }

    fn enter(&mut self, value: &serde_json::Value) -> bool {
        if self.truncated {
            return false;
        }
        if let serde_json::Value::String(s) = value {
            self.string_nodes += 1;
            self.bytes_visited = self.bytes_visited.saturating_add(s.len());
        } else {
            self.bytes_visited = self.bytes_visited.saturating_add(2);
        }
        if let Some(max) = self.budget.max_total_bytes {
            if self.bytes_visited > max {
                self.truncated = true;
                return false;
            }
        }
        if let Some(max) = self.budget.max_string_nodes {
            if self.string_nodes > max {
                self.truncated = true;
                return false;
            }
        }
        if let Some(max) = self.budget.max_depth {
            if self.depth >= max {
                self.truncated = true;
                return false;
            }
        }
        if let Some(max) = self.budget.max_elapsed {
            if self.started.elapsed() > max {
                self.truncated = true;
                return false;
            }
        }
        self.depth += 1;
        true
    }

    fn exit(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

fn walk_features(
    value: &serde_json::Value,
    path: &str,
    features: &mut super::gate::RequestFeatures,
    ctx: &mut FeatureWalkCtx,
) {
    if !ctx.enter(value) {
        return;
    }
    match value {
        serde_json::Value::String(s) => {
            if features.base64_attachments.len() < MAX_FEATURE_ITEMS {
                if let Some(meta) = parse_data_url(path, s) {
                    features.base64_attachments.push(meta);
                }
            }
            // Record external image URLs (only http/https, not data: URLs).
            if features.image_urls.len() < MAX_FEATURE_ITEMS
                && (s.starts_with("http://") || s.starts_with("https://"))
            {
                // Only surface image-looking URLs; keep the list bounded.
                let lower = s.to_ascii_lowercase();
                let is_image = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", "image/"]
                    .iter()
                    .any(|ext| lower.contains(ext));
                if is_image {
                    features.image_urls.push(s.clone());
                }
            }
        }
        serde_json::Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk_features(item, &format!("{}[{}]", path, i), features, ctx);
                if ctx.truncated {
                    break;
                }
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let child = if path == "$" {
                    format!("$.{}", k)
                } else {
                    format!("{}.{}", path, k)
                };
                // Responses built-in tools: {"type": "web_search_preview", ...}
                if k == "type" && features.builtin_tools.len() < MAX_FEATURE_ITEMS {
                    if let Some(t) = v.as_str() {
                        if is_responses_content_type(t) || is_responses_tool_type(t) {
                            features.builtin_tools.push(format!("{child}: {t}"));
                        }
                    }
                }
                walk_features(v, &child, features, ctx);
                if ctx.truncated {
                    break;
                }
            }
        }
        _ => {}
    }
    ctx.exit();
}

/// Detect Responses API tool / content-block types that must be traceable.
fn is_responses_tool_type(t: &str) -> bool {
    matches!(
        t,
        "web_search_preview"
            | "web_search"
            | "file_search"
            | "code_interpreter"
            | "computer_use_preview"
            | "computer_use"
            | "builtin_tool"
            | "function"
            | "local_shell"
            | "mcp_server"
    )
}

/// Detect Responses content block types (including unknown ones).
fn is_responses_content_type(t: &str) -> bool {
    matches!(
        t,
        "input_text"
            | "output_text"
            | "input_image"
            | "output_image"
            | "input_file"
            | "output_file"
            | "refusal"
            | "reasoning"
            | "computer_call"
    )
}

/// Whether the JSON contains function/tool definitions or calls.  Depth-bounded
/// so a hostile deep tree cannot recurse unboundedly.
fn has_tools(value: &serde_json::Value) -> bool {
    has_tools_depth(value, 0)
}

fn has_tools_depth(value: &serde_json::Value, depth: usize) -> bool {
    if depth > 256 {
        return false;
    }
    match value {
        serde_json::Value::String(s) => {
            let lower = s.to_ascii_lowercase();
            lower.contains("function") || lower.contains("tool")
        }
        serde_json::Value::Array(items) => items.iter().any(|i| has_tools_depth(i, depth + 1)),
        serde_json::Value::Object(map) => map.iter().any(|(k, v)| {
            let key = k.to_ascii_lowercase();
            key.contains("tools")
                || key.contains("tool")
                || (key == "type" && has_tools_depth(v, depth + 1))
                || has_tools_depth(v, depth + 1)
        }),
        _ => false,
    }
}

/// Parse a `data:<media_type>;base64,<payload>` string as attachment metadata.
/// Returns `None` for anything that is not a base64 data URL.  The payload is
/// only length/hash measured — never scanned as text.  SHA-256 is O(n), so it
/// is skipped above [`MAX_BASE64_HASH_BYTES`] (an independent per-attachment
/// cap; the length is still recorded in O(1)).
fn parse_data_url(path: &str, s: &str) -> Option<Base64AttachmentMeta> {
    let rest = s.strip_prefix("data:")?;
    let (header, payload) = rest.split_once(',')?;
    let (media_type, is_b64) = match header.rsplit_once(';') {
        Some((mt, enc)) => (mt, enc.eq_ignore_ascii_case("base64")),
        None => (header, false),
    };
    if !is_b64 {
        return None;
    }
    let sha256 = if payload.len() > MAX_BASE64_HASH_BYTES {
        // Oversized payload: length-metadata only, hashing skipped.
        "<skipped:oversized>".to_string()
    } else {
        hex::encode(Sha256::digest(payload.as_bytes()))
    };
    Some(Base64AttachmentMeta {
        pointer: path.to_string(),
        media_type: if media_type.is_empty() {
            "application/octet-stream".to_string()
        } else {
            media_type.to_string()
        },
        declared_len: payload.len(),
        actual_len: payload.len(),
        sha256,
    })
}
