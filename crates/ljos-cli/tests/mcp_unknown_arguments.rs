//! MCP tool calls refuse argument names the schema does not list.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn call_tool(name: &str, arguments: serde_json::Value) -> serde_json::Value {
    call_tool_with_env(name, arguments, &[])
}

fn call_tool_with_env(
    name: &str,
    arguments: serde_json::Value,
    extra_env: &[(&str, &str)],
) -> serde_json::Value {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TOKEN", "probe")
        .env("PACKSET_WORKSPACE", "ada")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"probe","version":"0"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name": name,
            "arguments": arguments,
        }}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();
    drop(stdin);
    let stdout = child.wait_with_output().unwrap().stdout;
    let text = String::from_utf8_lossy(&stdout);
    for line in BufReader::new(stdout.as_slice()).lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value["id"] == 2 {
            return value;
        }
    }
    panic!("no tools/call answer in:\n{text}");
}

fn text_blocks(answer: &serde_json::Value) -> Vec<String> {
    answer["result"]["content"]
        .as_array()
        .expect("content")
        .iter()
        .filter_map(|block| block["text"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn unknown_argument_on_ljos_note_writes_nothing_and_names_the_field() {
    let answer = call_tool(
        "ljos_note",
        serde_json::json!({"text": "probe", "bogus": "x"}),
    );
    let result = answer["result"].as_object().expect("result");
    assert_eq!(result["isError"], true);
    let blocks = text_blocks(&answer);
    assert_eq!(
        blocks[0],
        "ljos_note has no argument bogus; it takes issue, text, seat"
    );
    assert_eq!(
        blocks.last().unwrap(),
        "next: fix the line above and call ljos_note again"
    );
}

#[test]
fn ljos_note_accepts_id_as_an_alias_for_issue() {
    let answer = call_tool(
        "ljos_note",
        serde_json::json!({"id": "Software-8wki", "text": "probe"}),
    );
    let result = answer["result"].as_object().expect("result");
    assert_eq!(result["isError"], true);
    let blocks = text_blocks(&answer);
    assert!(
        blocks[0].contains("run `ljos onboard`"),
        "unexpected refusal: {}",
        blocks[0]
    );
}

#[test]
fn ljos_file_accepts_kind_as_an_alias_for_issue_kind() {
    let answer = call_tool(
        "ljos_file",
        serde_json::json!({"title": "probe", "project": "demo", "kind": "task"}),
    );
    let result = answer["result"].as_object().expect("result");
    assert_eq!(result["isError"], true);
    let blocks = text_blocks(&answer);
    assert!(
        blocks[0].contains("run `ljos onboard`"),
        "unexpected refusal: {}",
        blocks[0]
    );
}

#[test]
fn issue_and_id_alias_match_on_missing_tracker_refusals() {
    let issue = call_tool(
        "ljos_note",
        serde_json::json!({"issue": "Software-8wki", "text": "probe"}),
    );
    let id = call_tool(
        "ljos_note",
        serde_json::json!({"id": "Software-8wki", "text": "probe"}),
    );
    assert_eq!(text_blocks(&issue), text_blocks(&id));
    assert!(
        text_blocks(&issue)
            .first()
            .is_some_and(|line| line.contains("run `ljos onboard`")),
        "{:?}",
        text_blocks(&issue)
    );
}

#[test]
fn issue_and_id_alias_match_on_missing_seat_refusals() {
    let tracker = tempfile::tempdir().unwrap();
    std::fs::write(tracker.path().join("vissue.toml"), "seats = [\"ada\"]\n").unwrap();
    let root = tracker.path().to_str().unwrap().to_string();
    let issue = call_tool_with_env(
        "ljos_findings",
        serde_json::json!({"issue": "probe-a1b2"}),
        &[
            ("VISSUE_ROOT", &root),
            ("VISSUE_NO_ROUTE", "1"),
            ("LJOS_SEAT", ""),
        ],
    );
    let id = call_tool_with_env(
        "ljos_findings",
        serde_json::json!({"id": "probe-a1b2"}),
        &[
            ("VISSUE_ROOT", &root),
            ("VISSUE_NO_ROUTE", "1"),
            ("LJOS_SEAT", ""),
        ],
    );
    assert_eq!(text_blocks(&issue), text_blocks(&id));
    assert_eq!(issue["result"]["isError"], true);
}
