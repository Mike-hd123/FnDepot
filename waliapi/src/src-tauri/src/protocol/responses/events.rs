use serde_json::Value;

use super::state::{now_ts, StreamState};

/// Create the initial response.created + response.in_progress events for Responses API stream.
/// Returns both events as a single string to write at stream start.
pub fn create_response_created_event(model: &str, response_id: &str) -> String {
    let created = now_ts();
    let response_obj = serde_json::json!({
        "id": response_id,
        "object": "response",
        "created_at": created,
        "status": "in_progress",
        "model": model,
        "output": [],
        "error": null,
        "incomplete_details": null,
        "instructions": null,
        "metadata": null,
        "parallel_tool_calls": false,
        "temperature": null,
        "tool_choice": "auto",
        "tools": [],
        "top_p": null,
        "truncation": null,
        "usage": null,
        "background": false,
        "completed_at": null
    });

    let created_event = serde_json::json!({
        "type": "response.created",
        "response": response_obj,
        "sequence_number": 0
    });

    let in_progress_event = serde_json::json!({
        "type": "response.in_progress",
        "response": response_obj,
        "sequence_number": 1
    });

    format!(
        "event: response.created\ndata: {}\n\nevent: response.in_progress\ndata: {}\n\n",
        created_event, in_progress_event
    )
}

/// Create synthetic closing events when upstream stream ends.
/// Emits closing events for any still-open items (text and/or tool calls),
/// then emits response.completed with usage.
///
/// This is called:
/// - When the upstream stream ends without a finish_reason (synthetic close)
/// - When the upstream stream ends with finish_reason but response.completed hasn't been sent yet
///   (because response.completed needs usage data which comes in the final chunk)
pub fn create_synthetic_completed_events(
    model: &str,
    response_id: &str,
    accumulated_content: &str,
    state: &StreamState,
    usage_prompt: i64,
    usage_completion: i64,
) -> Vec<String> {
    let mut events = Vec::new();
    let msg_id = format!(
        "msg_{}",
        response_id.strip_prefix("resp_").unwrap_or(response_id)
    );

    // We need a mutable state to track sequence numbers, but we receive &StreamState.
    // Use a local counter starting from the state's current sequence_number.
    let mut seq = state.sequence_number;

    macro_rules! next_seq {
        () => {{
            seq += 1;
            seq
        }};
    }

    // Close reasoning item if it was opened and not yet closed
    if state.reasoning_item_added && !state.reasoning_item_done {
        let reasoning_output_index = state.reasoning_output_index;
        let reasoning_id = format!(
            "rs_{}",
            response_id.strip_prefix("resp_").unwrap_or(response_id)
        );

        let s = next_seq!();
        let text_done = serde_json::json!({
            "type": "response.reasoning_summary_text.done",
            "item_id": reasoning_id,
            "output_index": reasoning_output_index,
            "summary_index": 0,
            "text": state.accumulated_reasoning,
            "sequence_number": s
        });
        events.push(format!(
            "event: response.reasoning_summary_text.done\ndata: {}\n\n",
            text_done
        ));

        let s = next_seq!();
        let part = serde_json::json!({
            "type": "reasoning_summary_text",
            "text": state.accumulated_reasoning
        });
        let part_done = serde_json::json!({
            "type": "response.reasoning_summary_part.done",
            "item_id": reasoning_id,
            "output_index": reasoning_output_index,
            "summary_index": 0,
            "part": part,
            "sequence_number": s
        });
        events.push(format!(
            "event: response.reasoning_summary_part.done\ndata: {}\n\n",
            part_done
        ));

        let s = next_seq!();
        let completed_item = serde_json::json!({
            "id": reasoning_id,
            "type": "reasoning",
            "status": "completed",
            "summary": [{
                "type": "summary_text",
                "text": state.accumulated_reasoning
            }],
            "content": []
        });
        let item_done = serde_json::json!({
            "type": "response.output_item.done",
            "output_index": reasoning_output_index,
            "item": completed_item,
            "sequence_number": s
        });
        events.push(format!(
            "event: response.output_item.done\ndata: {}\n\n",
            item_done
        ));
    }

    // Close text item if it was opened and not yet closed
    if state.text_item_added && !state.text_item_done {
        let text_output_index = state.text_output_index;

        let s = next_seq!();
        let text_done = serde_json::json!({
            "type": "response.output_text.done",
            "item_id": msg_id,
            "output_index": text_output_index,
            "content_index": 0,
            "text": accumulated_content,
            "sequence_number": s
        });
        events.push(format!(
            "event: response.output_text.done\ndata: {}\n\n",
            text_done
        ));

        let s = next_seq!();
        let part = serde_json::json!({
            "type": "output_text",
            "text": accumulated_content,
            "annotations": []
        });
        let part_done = serde_json::json!({
            "type": "response.content_part.done",
            "item_id": msg_id,
            "output_index": text_output_index,
            "content_index": 0,
            "part": part,
            "sequence_number": s
        });
        events.push(format!(
            "event: response.content_part.done\ndata: {}\n\n",
            part_done
        ));

        let s = next_seq!();
        let completed_item = serde_json::json!({
            "id": msg_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": accumulated_content,
                "annotations": []
            }]
        });
        let item_done = serde_json::json!({
            "type": "response.output_item.done",
            "output_index": text_output_index,
            "item": completed_item,
            "sequence_number": s
        });
        events.push(format!(
            "event: response.output_item.done\ndata: {}\n\n",
            item_done
        ));
    }

    // Close any still-open tool call items
    for (_, tc_state) in state.tool_calls.iter() {
        // Fallback: ensure call_id is never empty
        let effective_call_id = if tc_state.call_id.is_empty() {
            format!("call_{}", tc_state.output_index)
        } else {
            tc_state.call_id.clone()
        };
        if !tc_state.arguments_done_sent {
            let s = next_seq!();
            let args_done = serde_json::json!({
                "type": "response.function_call_arguments.done",
                "item_id": tc_state.item_id,
                "output_index": tc_state.output_index,
                "name": tc_state.name,
                "arguments": tc_state.accumulated_arguments,
                "sequence_number": s
            });
            events.push(format!(
                "event: response.function_call_arguments.done\ndata: {}\n\n",
                args_done
            ));
        }

        if !tc_state.output_item_done_sent {
            let s = next_seq!();
            let fc_completed = serde_json::json!({
                "id": tc_state.item_id,
                "type": "function_call",
                "status": "completed",
                "call_id": effective_call_id,
                "name": tc_state.name,
                "arguments": tc_state.accumulated_arguments
            });
            let item_done = serde_json::json!({
                "type": "response.output_item.done",
                "output_index": tc_state.output_index,
                "item": fc_completed,
                "sequence_number": s
            });
            events.push(format!(
                "event: response.output_item.done\ndata: {}\n\n",
                item_done
            ));
        }
    }

    // Build the output array for response.completed
    let mut output_items: Vec<Value> = Vec::new();

    // Add reasoning item to output if it was added
    if state.reasoning_item_added {
        let reasoning_id = format!(
            "rs_{}",
            response_id.strip_prefix("resp_").unwrap_or(response_id)
        );
        output_items.push(serde_json::json!({
            "id": reasoning_id,
            "type": "reasoning",
            "status": "completed",
            "summary": [{
                "type": "summary_text",
                "text": state.accumulated_reasoning
            }],
            "content": []
        }));
    }

    // Add text item to output if it was added
    if state.text_item_added {
        output_items.push(serde_json::json!({
            "id": msg_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": accumulated_content,
                "annotations": []
            }]
        }));
    }

    // Add tool call items to output
    for (_, tc_state) in state.tool_calls.iter() {
        let effective_call_id = if tc_state.call_id.is_empty() {
            format!("call_{}", tc_state.output_index)
        } else {
            tc_state.call_id.clone()
        };
        output_items.push(serde_json::json!({
            "id": tc_state.item_id,
            "type": "function_call",
            "status": "completed",
            "call_id": effective_call_id,
            "name": tc_state.name,
            "arguments": tc_state.accumulated_arguments
        }));
    }

    let s = next_seq!();
    // response.completed (with usage)
    let completed = serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": response_id,
            "object": "response",
            "created_at": now_ts(),
            "status": "completed",
            "model": model,
            "output": output_items,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "parallel_tool_calls": false,
            "temperature": null,
            "tool_choice": "auto",
            "tools": [],
            "top_p": null,
            "truncation": null,
            "background": false,
            "completed_at": now_ts(),
            "usage": {
                "input_tokens": usage_prompt,
                "input_tokens_details": {
                    "cached_tokens": 0,
                    "cache_write_tokens": 0
                },
                "output_tokens": usage_completion,
                "output_tokens_details": {
                    "reasoning_tokens": 0
                },
                "total_tokens": usage_prompt + usage_completion
            }
        },
        "sequence_number": s
    });
    events.push(format!(
        "event: response.completed\ndata: {}\n\n",
        completed
    ));

    events
}
