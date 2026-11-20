//! MCP tool calls refuse argument names the schema does not list.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
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
        .env("TMPDIR", std::env::temp_dir())
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

#[test]
fn id_and_issue_together_are_refused() {
    let answer = call_tool(
        "ljos_note",
        serde_json::json!({"id": "Software-8wki", "issue": "Software-9z9z", "text": "probe"}),
    );
    let result = answer["result"].as_object().expect("result");
    assert_eq!(result["isError"], true);
    let blocks = text_blocks(&answer);
    assert_eq!(
        blocks[0],
        "ljos_note has no argument id; it takes issue, text, seat"
    );
    assert_eq!(
        blocks.last().unwrap(),
        "next: fix the line above and call ljos_note again"
    );
}

#[test]
fn kind_and_issue_kind_together_are_refused_on_ljos_file() {
    let answer = call_tool(
        "ljos_file",
        serde_json::json!({
            "title": "Probe the glaze",
            "project": "demo",
            "kind": "task",
            "issue_kind": "bug",
        }),
    );
    let result = answer["result"].as_object().expect("result");
    assert_eq!(result["isError"], true);
    let blocks = text_blocks(&answer);
    assert_eq!(
        blocks[0],
        "ljos_file has no argument kind; it takes title, parent, top, project, issue_kind, tags, priority, body, options, seat"
    );
    assert_eq!(
        blocks.last().unwrap(),
        "next: fix the line above and call ljos_file again"
    );
}

fn git(root: &Path, config: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn tool_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .find(|dir| dir.join(name).is_file())
        .unwrap_or_else(|| panic!("{name} is not on PATH"))
}

/// A call that would file a ticket, refused for an unknown argument, does
/// not commit the tracker. The same call without that argument does.
#[test]
fn a_refused_write_leaves_the_tracker_head() {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let tracker = root.path().join("tracker");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"ada\"]\n").unwrap();
    let gitconfig = root.path().join("gitconfig");
    std::fs::write(&gitconfig, "[core]\n\thooksPath = /dev/null\n").unwrap();
    git(&tracker, &gitconfig, &["init", "-q"]);
    git(
        &tracker,
        &gitconfig,
        &["config", "user.name", "Ada Example"],
    );
    git(
        &tracker,
        &gitconfig,
        &["config", "user.email", "ada@example.invalid"],
    );
    git(
        &tracker,
        &gitconfig,
        &["config", "core.hooksPath", "/dev/null"],
    );
    git(&tracker, &gitconfig, &["add", "vissue.toml"]);
    git(&tracker, &gitconfig, &["commit", "-q", "-m", "seed"]);
    let seeded = git(&tracker, &gitconfig, &["rev-parse", "HEAD"]);

    let ljos = Path::new(env!("CARGO_BIN_EXE_ljos"))
        .parent()
        .unwrap()
        .display()
        .to_string();
    let vissue = tool_dir("vissue");
    let path = format!("{ljos}:{}:/usr/bin:/bin", vissue.display());
    let call = |arguments: serde_json::Value| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        cmd.env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_DATA_HOME", home.join("data"))
            .env("VISSUE_ROOT", &tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("LJOS_MCP_ORPHAN_IDLE", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(&tracker);
        call_with(cmd, "ljos_file", arguments)
    };

    let refused = call(serde_json::json!({
        "title": "Probe the glaze",
        "project": "kiln",
        "top": true,
        "seat": "ada",
        "bogus": "x",
    }));
    assert_eq!(refused["result"]["isError"], true);
    let blocks = text_blocks(&refused);
    assert!(
        blocks[0].starts_with("ljos_file has no argument bogus; it takes "),
        "{}",
        blocks[0]
    );
    assert_eq!(git(&tracker, &gitconfig, &["rev-parse", "HEAD"]), seeded);
    assert_eq!(
        git(&tracker, &gitconfig, &["status", "--porcelain"]),
        "",
        "a refused write changed the tracker worktree"
    );

    let filed = call(serde_json::json!({
        "title": "Probe the glaze",
        "project": "kiln",
        "top": true,
        "seat": "ada",
    }));
    assert_ne!(filed["result"]["isError"], true, "{filed}");
    let filed_text = text_blocks(&filed).join("\n");
    assert!(
        filed_text.contains("kiln-"),
        "the same call without the unknown argument did not file: {filed_text}"
    );
    assert_ne!(
        git(&tracker, &gitconfig, &["rev-parse", "HEAD"]),
        seeded,
        "a real file did not move the tracker, so the refusal check was empty"
    );
}

fn call_with(mut cmd: Command, name: &str, arguments: serde_json::Value) -> serde_json::Value {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
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
