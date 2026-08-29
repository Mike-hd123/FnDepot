pub mod anthropic;
pub mod codec;
mod detect;
mod legacy;
pub mod responses;
pub mod sse_bridge;
pub mod thinking;

// Facade re-exports of plan-mandated kept functions (零公共 API 变化; is_anthropic_request /
// is_responses_request / estimate_anthropic_input_tokens 保持现状，不得删除或降可见性)。
// `mod protocol` 是 crate 私有，无 crate 内消费者的 re-export 会触发 unused_imports，
// 与 responses/codec 各 facade 的 `#[allow(unused_imports)]` 约定一致。
#[allow(unused_imports)]
pub use detect::{extract_api_key, is_anthropic_request, is_responses_request};
#[allow(unused_imports)]
pub use legacy::{
    anthropic_to_openai, estimate_anthropic_input_tokens, openai_to_anthropic, openai_to_responses,
    responses_to_openai,
};
