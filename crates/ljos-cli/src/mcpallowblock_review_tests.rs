//! The byte scan that answers a Cursor `beforeReadFile` early must agree
//! with the JSON parser on which event a payload is.

use super::*;

/// serde keeps the last `hook_event_name`. The scan must not stop at the
/// first one and answer allow for a call the parser reads as preToolUse.
#[test]
fn a_second_event_key_is_not_read_as_a_file_read() {
    let dup = r#"{"cursor_version":"2.4.0","conversation_id":"c","hook_event_name":"beforeReadFile","hook_event_name":"preToolUse","tool_name":"Write","tool_input":{"file_path":"/h/.cursor/hooks.json","content":"{}"}}"#;
    assert_eq!(hook_call_as(dup, None).event, "PreToolUse");
    assert!(
        !cursor_read_allow_bytes(dup.as_bytes()),
        "the scan calls a preToolUse a read"
    );
    assert!(
        cursor_side_answer(dup, None).is_none(),
        "a preToolUse got the side answer"
    );
}

/// The same with a beforeMCPExecution GitHub write as the last event key.
#[test]
fn a_second_event_key_keeps_the_mcp_call() {
    let dup = r#"{"cursor_version":"2.4.0","hook_event_name":"beforeReadFile","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":"{}"}"#;
    assert_eq!(hook_call_as(dup, None).event, "beforeMCPExecution");
    assert!(!cursor_read_allow_bytes(dup.as_bytes()));
}
