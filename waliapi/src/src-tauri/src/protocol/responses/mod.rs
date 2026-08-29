mod assembler;
mod convert;
mod events;
mod state;
mod usage;

pub use assembler::ResponsesSseAssembler;
pub use convert::convert_openai_sse_to_responses;
pub use events::{create_response_created_event, create_synthetic_completed_events};
pub use state::StreamState;
// Facade contract: ToolCallState stays reachable as `protocol::responses::ToolCallState`
// (zero public API change), even though no in-crate consumer currently imports it —
// `mod protocol` is private, so rustc flags the re-export as unused.
#[allow(unused_imports)]
pub use state::ToolCallState;
pub use usage::parse_usage_from_sse_chunk;

#[cfg(test)]
mod tests;
