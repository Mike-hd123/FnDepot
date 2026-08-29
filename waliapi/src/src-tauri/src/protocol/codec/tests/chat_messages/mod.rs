//! Tests for the strict Chat ↔ Messages codec (T04).
//!
//! These tests encode the WaLiAPI fail-closed contract: unsupported features
//! are rejected with a concrete JSON pointer and stable error code before any
//! upstream access; invalid tool arguments are never rewritten to `{}`; an
//! unknown finish reason is never downgraded to a normal stop/end_turn; SSE
//! arbitrary fragmentation is deterministic; termination happens exactly once.

mod support;

mod chat_request;
mod chat_response_stream;
mod messages_request;
mod messages_response_stream;
mod registry;
