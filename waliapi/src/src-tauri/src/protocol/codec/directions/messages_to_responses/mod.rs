//! Direct Messages request -> Responses request codec.
//!
//! Request and response shapes are translated in one pass.  This is kept
//! independent from the other protocol direction so adding a field to either
//! wire format cannot accidentally reintroduce an intermediate conversion.

use super::super::{
    direction::CodecDirection,
    error::{FeatureKind, PrepareError, UnsupportedFeatures},
    ports::{NonStreamDecoder, StreamDecoder},
    report::ConversionContext,
    types::{CodecId, Protocol},
};
use serde_json::Value;

use decode::ResponsesMessageDecoder;
use stream::ResponsesMessagesStream;

mod decode;
mod encode;
mod stream;

// Facade re-export：`mod protocol` 是 crate 私有，此 re-export 保留一个无 crate 内
// 消费者的内部路径，rustc 会将其标记为未使用 —— 与模块树各 facade 的
// `#[allow(unused_imports)]` 约定一致。
#[allow(unused_imports)]
pub use decode::decode_response;
pub use encode::encode_request;

pub static MESSAGES_TO_RESPONSES_V2: MessagesToResponses = MessagesToResponses;
pub struct MessagesToResponses;
impl CodecDirection for MessagesToResponses {
    fn id(&self) -> CodecId {
        CodecId::MessagesToResponsesV2
    }
    fn downstream(&self) -> Protocol {
        Protocol::Messages
    }
    fn upstream(&self) -> Protocol {
        Protocol::Responses
    }
    fn encode_request(
        &self,
        r: &Value,
        m: &str,
    ) -> Result<(Value, ConversionContext), PrepareError> {
        encode_request(r, m)
    }
    fn new_response_decoder(
        &self,
        c: &ConversionContext,
    ) -> Box<dyn NonStreamDecoder + Send + Sync> {
        Box::new(ResponsesMessageDecoder { context: c.clone() })
    }
    fn new_stream_response_decoder(
        &self,
        c: &ConversionContext,
    ) -> Box<dyn StreamDecoder + Send + Sync> {
        Box::new(ResponsesMessagesStream::new(c))
    }
}
fn bad(k: FeatureKind, p: impl Into<String>, m: impl Into<String>) -> UnsupportedFeatures {
    UnsupportedFeatures::single(k, p, m)
}
fn required<'a>(v: &'a Value, k: &str, p: &str) -> Result<&'a str, UnsupportedFeatures> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            bad(
                FeatureKind::MissingToolField,
                format!("{p}/{k}"),
                format!("{k} is required"),
            )
        })
}

#[cfg(test)]
mod tests;
