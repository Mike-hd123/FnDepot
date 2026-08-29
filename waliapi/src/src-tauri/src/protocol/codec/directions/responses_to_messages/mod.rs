//! Direct Responses request -> Messages request codec.
//!
//! This module deliberately owns both halves of the direction: the request
//! encoder consumes Responses, while its decoders consume Messages.  Keeping
//! the mapping here avoids a lossy intermediate representation and, in
//! particular, preserves function-call ids and output ordering.

use super::super::{
    direction::CodecDirection,
    error::{FeatureKind, PrepareError, UnsupportedFeatures},
    ports::{NonStreamDecoder, StreamDecoder},
    report::ConversionContext,
    types::{CodecId, Protocol},
};
use serde_json::Value;

use decode::MessagesResponseDecoder;
use stream::MessagesResponsesStream;

mod decode;
mod encode;
mod stream;

// Facade re-export：`mod protocol` 是 crate 私有，此 re-export 保留一个无 crate 内
// 消费者的内部路径，rustc 会将其标记为未使用 —— 与模块树各 facade 的
// `#[allow(unused_imports)]` 约定一致。
#[allow(unused_imports)]
pub use decode::decode_messages_response;
pub use encode::encode_request;

pub static RESPONSES_TO_MESSAGES_V2: ResponsesToMessages = ResponsesToMessages;

pub struct ResponsesToMessages;

impl CodecDirection for ResponsesToMessages {
    fn id(&self) -> CodecId {
        CodecId::ResponsesToMessagesV2
    }
    fn downstream(&self) -> Protocol {
        Protocol::Responses
    }
    fn upstream(&self) -> Protocol {
        Protocol::Messages
    }

    fn encode_request(
        &self,
        request: &Value,
        mapped_model: &str,
    ) -> Result<(Value, ConversionContext), PrepareError> {
        encode_request(request, mapped_model)
    }
    fn new_response_decoder(
        &self,
        context: &ConversionContext,
    ) -> Box<dyn NonStreamDecoder + Send + Sync> {
        Box::new(MessagesResponseDecoder {
            context: context.clone(),
        })
    }
    fn new_stream_response_decoder(
        &self,
        context: &ConversionContext,
    ) -> Box<dyn StreamDecoder + Send + Sync> {
        Box::new(MessagesResponsesStream::new(context))
    }
}

fn unsupported(
    kind: FeatureKind,
    pointer: impl Into<String>,
    message: impl Into<String>,
) -> UnsupportedFeatures {
    UnsupportedFeatures::single(kind, pointer, message)
}

fn required<'a>(
    value: &'a Value,
    field: &str,
    pointer: &str,
) -> Result<&'a str, UnsupportedFeatures> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            unsupported(
                FeatureKind::MissingToolField,
                format!("{pointer}/{field}"),
                format!("{field} is required"),
            )
        })
}

#[cfg(test)]
mod tests;
