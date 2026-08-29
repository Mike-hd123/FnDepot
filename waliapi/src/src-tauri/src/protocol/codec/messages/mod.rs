//! `messages_to_chat_v1` — Anthropic Messages → OpenAI Chat Completions.
//!
//! Covers request encoding, non-stream response decoding, and the streaming
//! (SSE) response decoding.  Extracts the strict conversion previously living
//! in `protocol::anthropic` without lowering its rejection policy: invalid tool
//! arguments fail, unknown roles/blocks are rejected, prompt-cache annotations
//! are stripped only when lossless, and usage is taken from real upstream
//! values.

mod decode;
mod encode;
mod message;
mod stream;

pub use decode::{decode_messages_response_to_chat, NonStreamResponseDecoder};
pub use encode::encode_messages_to_chat;
pub use stream::MessagesStreamDecoder;
// Facade contract: these pre-split public items stay reachable through
// `messages::` (zero public API change).  `usage_from_messages` has no in-crate
// consumer outside test builds — `mod protocol` is private, so rustc flags the
// re-export as unused.
#[allow(unused_imports)]
pub use decode::usage_from_messages;
pub use stream::MessagesSseState;
