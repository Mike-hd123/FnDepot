use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// State for tracking streaming output items during OpenAI SSE → Responses SSE conversion.
///
/// Tracks both text message items and function_call items so we can emit
/// the complete Codex-compatible event chain:
///
/// For text message:
///   response.output_item.added → response.content_part.added →
///   response.output_text.delta → response.output_text.done →
///   response.content_part.done → response.output_item.done
///
/// For function_call:
///   response.output_item.added(type=function_call) →
///   response.function_call_arguments.delta →
///   response.function_call_arguments.done →
///   response.output_item.done
///
/// For reasoning (DeepSeek R1, OpenAI o1/o3, etc.):
///   response.output_item.added(type=reasoning) →
///   response.reasoning_summary_part.added →
///   response.reasoning_summary_text.delta (per chunk) →
///   response.reasoning_summary_text.done →
///   response.reasoning_summary_part.done →
///   response.output_item.done
#[derive(Default)]
pub struct StreamState {
    /// Whether the text message output_item.added has been sent.
    pub text_item_added: bool,
    /// Whether the text message output_item.done has been sent.
    pub text_item_done: bool,
    /// Whether the text content_part.added has been sent.
    pub text_part_added: bool,
    /// The output_index assigned to the text message item.
    pub text_output_index: u32,
    /// Next output_index to use for a new output item.
    pub next_output_index: u32,
    /// Whether the reasoning output_item.added has been sent.
    pub reasoning_item_added: bool,
    /// Whether the reasoning summary_part.added has been sent.
    pub reasoning_part_added: bool,
    /// Whether the reasoning summary_part.done has been sent.
    pub reasoning_part_done: bool,
    /// Whether the reasoning output_item.done has been sent.
    pub reasoning_item_done: bool,
    /// The output_index assigned to the reasoning item.
    pub reasoning_output_index: u32,
    /// Full concatenated reasoning text accumulated so far.
    pub accumulated_reasoning: String,
    /// Map from tool_call index → (output_index, call_id, name, accumulated_arguments, item_added_sent, arguments_done_sent)
    pub tool_calls: HashMap<u64, ToolCallState>,
    /// Whether any tool calls were seen in this stream.
    pub has_tool_calls: bool,
    /// Monotonic sequence number counter for all events.
    pub sequence_number: u64,
}

/// Per-tool-call streaming state.
#[derive(Clone)]
pub struct ToolCallState {
    pub output_index: u32,
    pub call_id: String,
    pub name: String,
    pub item_id: String,
    pub accumulated_arguments: String,
    pub item_added_sent: bool,
    pub arguments_done_sent: bool,
    pub output_item_done_sent: bool,
}

pub(super) fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Get the next sequence number from StreamState.
pub(super) fn next_seq(state: &mut StreamState) -> u64 {
    state.sequence_number += 1;
    state.sequence_number
}
