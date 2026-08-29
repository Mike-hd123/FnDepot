//! Error contract for the T04 codec.
//!
//! A conversion either succeeds or returns [`UnsupportedFeatures`].  The error
//! carries a stable error `code`, a human message, and one concrete
//! [`json_pointer`](serde_json::Value::pointer) per rejected field so the
//! gateway can fail closed with a 4xx *before* touching the upstream.

use serde::Serialize;
use std::fmt;

/// Stable error code returned by every fail-closed codec rejection.
///
/// This is the HTTP-level `error.code` value; it is stable across fields so a
/// client (and downstream tests) can match on it.  The precise feature is
/// identified by `features` + `json_pointers`.
pub const CODEC_UNSUPPORTED_FEATURE: &str = "unsupported_feature";
/// Error code used when a field is present but its *value* is not a
/// representable media form (for example an invalid image source).
pub const CODEC_UNSUPPORTED_MEDIA: &str = "unsupported_media";

/// Machine class of a rejected feature, for routing and error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKind {
    /// reasoning/thinking content that cannot be preserved safely.
    Thinking,
    /// structured output (`response_format` / JSON schema).
    StructuredOutput,
    /// provider built-in tools (web_search, computer_use, …).
    BuiltinTool,
    /// document/PDF or other file content blocks.
    Document,
    /// prompt cache / cache-control annotations that cannot be preserved.
    PromptCache,
    /// unknown message role.
    UnknownRole,
    /// unknown content block type.
    UnknownBlock,
    /// unknown SSE event or response object type.
    UnknownEvent,
    /// finish reason with no safe target-protocol semantic.
    UnknownFinishReason,
    /// invalid or non-object tool arguments (never rewritten to `{}`).
    InvalidToolArguments,
    /// missing tool id or name.
    MissingToolField,
    /// media that fails role/media-type/size validation.
    Media,
    /// anthropic beta feature with no OpenAI Chat equivalent.
    BetaFeature,
    /// top-level request field with no representable mapping.
    UnsupportedField,
}

impl FeatureKind {
    /// Stable per-kind error code suffix (e.g. `unsupported_feature.thinking`).
    pub fn code(self) -> &'static str {
        match self {
            FeatureKind::Thinking => "unsupported_feature.thinking",
            FeatureKind::StructuredOutput => "unsupported_feature.structured_output",
            FeatureKind::BuiltinTool => "unsupported_feature.builtin_tool",
            FeatureKind::Document => "unsupported_feature.document",
            FeatureKind::PromptCache => "unsupported_feature.prompt_cache",
            FeatureKind::UnknownRole => "unsupported_feature.unknown_role",
            FeatureKind::UnknownBlock => "unsupported_feature.unknown_block",
            FeatureKind::UnknownEvent => "unsupported_feature.unknown_event",
            FeatureKind::UnknownFinishReason => "unsupported_feature.finish_reason",
            FeatureKind::InvalidToolArguments => "unsupported_feature.invalid_tool_arguments",
            FeatureKind::MissingToolField => "unsupported_feature.missing_tool_field",
            FeatureKind::Media => CODEC_UNSUPPORTED_MEDIA,
            FeatureKind::BetaFeature => "unsupported_feature.beta_feature",
            FeatureKind::UnsupportedField => "unsupported_feature.field",
        }
    }
}

/// A single rejected feature occurrence: stable code + concrete JSON pointer.
#[derive(Debug, Clone, Serialize)]
pub struct RejectedField {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

/// Aggregate of every unsupported feature found while validating a request or
/// response.  All rejections happen before any upstream access.
#[derive(Debug, Clone, Serialize)]
pub struct UnsupportedFeatures {
    /// Stable codes of every rejected feature, sorted for determinism.
    pub features: Vec<String>,
    /// One concrete JSON pointer per rejected field.
    pub json_pointers: Vec<String>,
    /// Human-readable summary (already fully redacted-safe: no credentials).
    pub message: String,
    /// Per-feature detail (code + pointer + reason).
    #[serde(skip)]
    pub fields: Vec<RejectedField>,
}

impl UnsupportedFeatures {
    pub fn new(fields: Vec<RejectedField>) -> Self {
        let mut features: Vec<String> = fields.iter().map(|f| f.code.clone()).collect();
        features.sort();
        features.dedup();
        let json_pointers = fields.iter().map(|f| f.pointer.clone()).collect();
        let message = if fields.is_empty() {
            "unsupported feature".to_string()
        } else {
            let mut s = String::from("request uses feature(s) this codec cannot preserve: ");
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    s.push_str("; ");
                }
                s.push_str(&format!("{} ({})", f.pointer, f.code));
                if !f.message.is_empty() {
                    s.push_str(&format!(": {}", f.message));
                }
            }
            s
        };
        UnsupportedFeatures {
            features,
            json_pointers,
            message,
            fields,
        }
    }

    /// Build a single-feature rejection.
    pub fn single(
        kind: FeatureKind,
        pointer: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(vec![RejectedField {
            code: kind.code().to_string(),
            pointer: pointer.into(),
            message: message.into(),
        }])
    }

    /// Convenience for the dominant fail-closed outcome: a stable outer code
    /// plus the full pointer/code list for logging.
    pub fn code(&self) -> &'static str {
        CODEC_UNSUPPORTED_FEATURE
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// True when the rejection is a strict codec error (never used for
    /// transport errors).  Present so callers can distinguish "feature
    /// rejected" from "upstream could not be reached".
    pub fn rejected_before_upstream(&self) -> bool {
        !self.fields.is_empty()
    }
}

impl fmt::Display for UnsupportedFeatures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for UnsupportedFeatures {}

/// Any other codec failure that is not an unsupported-feature rejection.
///
/// Used for programming/transport errors inside the codec; the gateway treats
/// these as 502 (upstream_protocol_error) rather than a client 4xx.
#[derive(Debug, Clone)]
pub struct CodecError(pub String);

impl CodecError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "codec error: {}", self.0)
    }
}

impl std::error::Error for CodecError {}

impl From<String> for CodecError {
    fn from(value: String) -> Self {
        CodecError(value)
    }
}

/// A JSON-pointer-annotated validation error produced while inspecting an
/// upstream (post-commit) response.  The gateway converts this into a
/// target-protocol error event, never into a fake success.
#[derive(Debug, Clone)]
pub struct ResponseDecodeError {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

impl ResponseDecodeError {
    pub fn new(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: "response_decode_error".to_string(),
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ResponseDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}: {}", self.code, self.pointer, self.message)
    }
}

impl std::error::Error for ResponseDecodeError {}

/// Errors discovered while decoding an upstream response.
///
/// Preparation errors are deliberately kept separate: they describe caller
/// input and are returned before any upstream request.  A decode failure
/// instead describes an upstream protocol violation and must be classified by
/// callers as a retryable upstream failure (until a stream is committed).
pub type DecodeError = ResponseDecodeError;

/// Errors returned while preparing an upstream request.
///
/// This alias preserves the stable unsupported-feature payload used by the
/// existing HTTP boundary while making the phase distinction explicit at new
/// codec call sites.
pub type PrepareError = UnsupportedFeatures;

impl ResponseDecodeError {
    /// Convert a strict parser rejection into an upstream decode error without
    /// exposing it as a caller-side unsupported-feature response.
    pub fn from_unsupported(error: UnsupportedFeatures) -> Self {
        Self {
            code: "response_decode_error".to_string(),
            pointer: error
                .json_pointers
                .first()
                .cloned()
                .unwrap_or_else(|| "/".to_string()),
            message: error.message,
        }
    }
}

impl From<UnsupportedFeatures> for ResponseDecodeError {
    fn from(value: UnsupportedFeatures) -> Self {
        Self::from_unsupported(value)
    }
}
