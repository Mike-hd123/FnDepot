//! `chat_to_messages_v1` — OpenAI Chat Completions → Anthropic Messages.
//!
//! Covers request encoding, non-stream response decoding, and the streaming
//! (SSE) response decoding.  Every conversion is fail-closed: unsupported
//! features are rejected with a stable code + JSON pointer before any upstream
//! access; invalid tool arguments are never rewritten to `{}`.  OpenAI-compatible
//! gateways occasionally emit provider-specific terminal finish reasons; once a
//! response has otherwise completed, those are conservatively represented as an
//! Anthropic `end_turn` rather than aborting a committed Claude Code stream.

mod decode;
mod encode;
mod message;
mod stream;

#[cfg(test)]
mod tests;

pub use decode::{decode_chat_response_to_messages, NonStreamResponseDecoder};
pub use encode::encode_chat_to_messages;
pub use stream::ChatStreamDecoder;
// Facade contract: these pre-split public items stay reachable through
// `chat::` (zero public API change).  `usage_from_chat` and `ChatSseState`
// have no in-crate consumer outside test builds — `mod protocol` is private,
// so rustc flags the re-exports as unused.
#[allow(unused_imports)]
pub use decode::usage_from_chat;
#[allow(unused_imports)]
pub use stream::ChatSseState;
