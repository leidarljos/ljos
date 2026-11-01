//! `tools/list` answers when the home directory is empty.
//!
//! A fresh machine has no tracker, no seat file and no `LJOS_SEAT`. The
//! server still lists every tool. A call that needs a seat returns that
//! error and the process stays up.

ljos_test_boot::harness!();
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::Duration;

const EVERY_TOOL: &[&str] = &[
    "ljos_rule",
    "ljos_request_approval",
    "ljos_prefer",
    "ljos_remember",
    "ljos_search",
    "ljos_start",
    "ljos_guard_check",
    "ljos_file",
    "ljos_options",
    "ljos_note",
    "ljos_recall",
    "ljos_sitting",
    "ljos_finish",
    "ljos_claim",
    "ljos_release",
    "ljos_send",
    "ljos_inbox",
    "ljos_reply",
    "ljos_vote",
    "ljos_consensus",
    "ljos_doctor",
    "ljos_bump_plan",
    "ljos_calibrate",
    "ljos_cards",
    "ljos_complete",
    "ljos_conflicts",
    "ljos_consolidate",
    "ljos_current",
    "ljos_deed",
    "ljos_digest",
    "ljos_due",
    "ljos_evidence",
    "ljos_findings",
    "ljos_forget",
    "ljos_graded",
    "ljos_group",
    "ljos_habit",
    "ljos_handover",
    "ljos_island",
    "ljos_learn",
    "ljos_mail_search",
    "ljos_mail_show",
    "ljos_policy",
    "ljos_predict",
    "ljos_read",
    "ljos_receive",
    "ljos_timeline",
    "ljos_trust",
    "ljos_brief",
    "ljos_persona",
    "ljos_personas",
    "ljos_playbook",
    "ljos_playbooks",
];

fn standard_format(format: &str) -> bool {
    matches!(
        format,
        "date-time"
            | "date"
            | "time"
            | "duration"
            | "email"
            | "idn-email"
            | "hostname"
            | "idn-hostname"
            | "ipv4"
            | "ipv6"
            | "uri"
            | "uri-reference"
            | "iri"
            | "iri-reference"
            | "uuid"
            | "uri-template"
            | "json-pointer"
            | "relative-json-pointer"
            | "regex"
    )
}

fn assert_plain_schema(name: &str, value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            assert!(map.get("$schema").is_none(), "{name}: {value}");
            if let Some(format) = map.get("format").and_then(|value| value.as_str()) {
                assert!(standard_format(format), "{name} format {format}");
            }
            for child in map.values() {
                assert_plain_schema(name, child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                assert_plain_schema(name, item);
            }
        }
        _ => {}
    }
}

#[test]
fn tools_list_answers_with_an_empty_home_and_no_tracker() {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("LJOS_MCP_TOOLS", "all")
        .env_remove("VISSUE_ROOT")
        .env_remove("ISSUE_ROOT")
        .env_remove("LJOS_SEAT")
        .env_remove("VISSUE_AGENT")
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    // A server that never answers fails the test instead of hanging it.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "ljos_vote", "arguments": {"issue": "probe-abcd", "choice": "accept"}}}),
        serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {
            "name": "ljos_file", "arguments": {"title": "a child"}}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        !stderr.contains("panicked"),
        "stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    for line in stdout.lines().filter(|line| !line.is_empty()) {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|err| panic!("stdout is not JSON: {err}: {line}\nstderr:\n{stderr}"));
    }
    let answers: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let by_id = |id: u64| {
        answers
            .iter()
            .find(|answer| answer["id"] == id)
            .unwrap_or_else(|| panic!("no answer {id}\nstdout:\n{stdout}\nstderr:\n{stderr}"))
            .clone()
    };
    let init = by_id(1);
    assert!(init.get("error").is_none(), "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "ljos");
    let list = by_id(2);
    assert!(list.get("error").is_none(), "{list}");
    let tools = list["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, EVERY_TOOL, "stdout:\n{stdout}");
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let input = tool
            .get("inputSchema")
            .unwrap_or_else(|| panic!("{name} has no inputSchema"));
        assert_eq!(input["type"], "object", "{name}: {input}");
        assert_plain_schema(name, input);
        if let Some(output) = tool.get("outputSchema") {
            assert_eq!(output["type"], "object", "{name}: {output}");
            assert_plain_schema(name, output);
        }
    }
    let text = |value: &serde_json::Value| {
        value["result"]["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|block| block["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    for id in [3, 4] {
        let call = by_id(id);
        assert!(call.get("error").is_none(), "{call}");
        assert_eq!(call["result"]["isError"], true, "{call}");
        let said = text(&call);
        assert!(
            said.contains("no LJOS_SEAT"),
            "id {id}: {said}\nstderr:\n{stderr}"
        );
    }
}
