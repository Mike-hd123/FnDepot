//! Strategy port for a directed protocol codec.

use super::error::PrepareError;
use super::ports::{NonStreamDecoder, StreamDecoder};
use super::report::ConversionContext;
use super::types::{CodecId, Protocol};
use serde_json::Value;

/// A single, self-describing request direction and its inverse response
/// decoders. Implementations are stateless; per-request decoder state is
/// created only through the two factory methods.
pub trait CodecDirection: Send + Sync {
    fn id(&self) -> CodecId;
    fn downstream(&self) -> Protocol;
    fn upstream(&self) -> Protocol;

    fn encode_request(
        &self,
        request: &Value,
        mapped_model: &str,
    ) -> Result<(Value, ConversionContext), PrepareError>;

    fn new_response_decoder(
        &self,
        context: &ConversionContext,
    ) -> Box<dyn NonStreamDecoder + Send + Sync>;

    fn new_stream_response_decoder(
        &self,
        context: &ConversionContext,
    ) -> Box<dyn StreamDecoder + Send + Sync>;
}
