//! Strict Responses API codec used by auth-account upstreams.
//!
//! The Codex backend is always streamed.  This module consequently owns both
//! the request representation and the small, byte-framed Responses SSE state
//! machine used to express that stream as Chat (and, by composition, Messages).

mod accumulator;
mod chat_tools;
mod decode;
mod encode_chat;
mod encode_messages;
mod state;
mod stream;

#[cfg(test)]
mod tests;

pub use accumulator::ResponsesEventAccumulator;
pub use encode_chat::encode_chat_to_responses;
// Facade contract: these pre-split public items stay reachable through
// `responses_codec::` (zero public API change) even though no in-crate consumer
// currently imports them — `mod protocol` is private, so rustc flags the
// re-exports as unused.
#[allow(unused_imports)]
pub use decode::{
    decode_responses_response_to_chat, usage_from_responses, MessagesResponsesNonStreamDecoder,
    ResponsesMessagesNonStreamDecoder, ResponsesNonStreamDecoder,
};
#[allow(unused_imports)]
pub use encode_messages::{encode_messages_to_responses, encode_responses_to_messages};
#[allow(unused_imports)]
pub use stream::{
    ChatToResponsesStreamDecoder, MessagesResponsesStreamDecoder, ResponsesMessagesStreamDecoder,
    ResponsesStreamDecoder,
};
