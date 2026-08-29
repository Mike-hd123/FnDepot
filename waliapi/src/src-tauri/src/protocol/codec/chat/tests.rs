use super::stream::ChatSseState;

#[test]
fn stream_unknown_finish_reason_completes_as_end_turn() {
    let mut state = ChatSseState::new("go-model", "msg_test");
    let events = state
            .feed(
                br#"data: {"id":"chatcmpl_test","model":"go-model","choices":[{"delta":{"role":"assistant","content":"ok"},"finish_reason":"completed"}]}

"#,
            )
            .expect("provider-specific finish reason must not abort the stream");
    let final_events = state.finish().expect("stream finalizes");
    let all = events.into_iter().chain(final_events).collect::<String>();

    assert!(all.contains("\"text\":\"ok\""));
    assert!(all.contains("\"stop_reason\":\"end_turn\""));
    assert!(all.contains("event: message_stop"));
}

#[test]
fn stream_done_without_finish_reason_completes_as_end_turn() {
    let mut state = ChatSseState::new("go-model", "msg_test");
    let events = state
            .feed(
                b"data: {\"id\":\"chatcmpl_test\",\"model\":\"go-model\",\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
            )
            .expect("a [DONE]-terminated stream must be accepted");
    let final_events = state.finish().expect("stream finalizes");
    let all = events.into_iter().chain(final_events).collect::<String>();
    assert!(all.contains("\"text\":\"ok\""));
    assert!(all.contains("\"stop_reason\":\"end_turn\""));
    assert!(all.contains("event: message_stop"));
}

#[test]
fn stream_output_without_terminal_marker_completes_as_end_turn() {
    let mut state = ChatSseState::new("go-model", "msg_test");
    let events = state
            .feed(
                b"data: {\"id\":\"chatcmpl_test\",\"model\":\"go-model\",\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
            )
            .expect("a content frame must decode");
    let final_events = state
        .finish()
        .expect("clean EOF after assistant output must finalize");
    let all = events.into_iter().chain(final_events).collect::<String>();
    assert!(all.contains("\"text\":\"ok\""));
    assert!(all.contains("\"stop_reason\":\"end_turn\""));
    assert!(all.contains("event: message_stop"));
}

#[test]
fn stream_role_only_without_terminal_marker_is_rejected() {
    let mut state = ChatSseState::new("go-model", "msg_test");
    state
            .feed(
                b"data: {\"id\":\"chatcmpl_test\",\"model\":\"go-model\",\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            )
            .expect("a role frame must decode");
    let error = state.finish().expect_err("role-only EOF must fail closed");
    assert_eq!(error.json_pointers, vec!["/choices/0/finish_reason"]);
}
