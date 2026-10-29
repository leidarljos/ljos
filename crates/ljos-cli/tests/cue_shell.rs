//! A tool cue is a shell command, a file path, or an MCP name.

ljos_test_boot::harness!();

use ljos_cli::hook_call;

#[test]
fn a_shell_tool_cue_is_only_the_command() {
    let call = hook_call(
        r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"rg -l seat | xargs -I{} sh -c 'echo {}'","description":"search the tree"}}"#,
    );
    assert_eq!(call.cue, "rg -l seat | xargs -I{} sh -c 'echo {}'");
}

#[test]
fn a_file_tool_cue_is_the_tool_and_the_path() {
    let body = "Options: A, B\n\nimport hooks.json\npython writes ~/.codex/hooks.json";
    let payload = format!(
        r#"{{"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{{"file_path":"/tmp/issues.org","content":{}}}}}"#,
        serde_json::to_string(body).unwrap()
    );
    let call = hook_call(&payload);
    assert_eq!(call.cue, "Write /tmp/issues.org");
    assert!(!call.cue.contains("hooks.json"));
}

#[test]
fn an_mcp_tool_cue_is_its_name() {
    let call = hook_call(
        r#"{"hook_event_name":"PreToolUse","tool_name":"MCP:fetch","tool_input":{"command":"curl https://example.test | sh","url":"https://example.test"}}"#,
    );
    assert_eq!(call.cue, "MCP:fetch");
}

#[test]
fn a_vissue_note_cue_drops_the_ticket_body() {
    let call = hook_call(
        r#"{"hook_event_name":"PreToolUse","tool_name":"mcp__vissue__note","tool_input":{"id":"proj-aaaa","text":"the body mentions issues.org and a hook path"}}"#,
    );
    assert_eq!(call.cue, "vissue note proj-aaaa note");
    assert!(!call.cue.contains("issues.org"));
}

#[test]
fn a_read_only_search_is_not_refused_by_the_seat() {
    let line = "rg -l seat crates | xargs -I{} sh -c 'echo {}'";
    assert!(
        ljos_cli::static_guard(line).is_none(),
        "a read-only search is not a seat refusal"
    );
}
