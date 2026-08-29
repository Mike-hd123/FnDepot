//! Response decoder ports exposed by the codec facade.

use super::error::DecodeError;
use super::report::Usage;
use serde_json::Value;
use std::ops::Deref;

/// A fully decoded upstream response and the usage observed while decoding it.
///
/// Returning usage with the body prevents consumers from reparsing the raw
/// provider response through protocol-specific side channels.
#[derive(Debug, Clone)]
pub struct DecodedResponse {
    pub body: Value,
    pub usage: Option<Usage>,
}

/// Temporary source compatibility for callers that consumed a decoder result
/// as raw JSON. New consumers should access `.body` explicitly.
impl Deref for DecodedResponse {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

/// Factory-produced decoder for one non-stream upstream response.
pub trait NonStreamDecoder: Send + Sync {
    fn decode(&self, body: &Value) -> Result<DecodedResponse, DecodeError>;
}

/// Factory-produced, stateful SSE decoder for one upstream stream.
pub trait StreamDecoder: Send + Sync {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, DecodeError>;
    fn finish(&mut self) -> Result<Vec<String>, DecodeError>;
    fn usage(&self) -> Option<Usage>;
}
