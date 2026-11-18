//! `ljos mcp` answers `initialize` and `tools/list` with no tracker, no
//! pack writer, no claim graph and no seat, from a checkout that is not
//! a tracker. A note on an issue says how to name the tracker.

ljos_test_boot::harness!();

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The tools `tools/list` returns when `LJOS_MCP_TOOLS` is unset.
const CORE: &[&str] = &[
    "ljos_rule",
    "ljos_prefer",
    "ljos_remember",
    "ljos_search",
    "ljos_start",
    "ljos_guard_check",
    "ljos_file",
    "ljos_options",
    "ljos_note",
    "ljos_recall",
    "ljos_show",
    "ljos_ready",
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
];

const MISSING_TRACKER: &str = "No tracker found, so run `ljos onboard`. To make one by hand, read https://leidarljos.github.io/ljos/howto-by-hand.html";

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Speak `initialize`, `tools/list` and one note to `program`.
fn session(program: &str, args: &[&str]) -> (String, String, Duration) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let work = root.path().join("work");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(work.join("Software")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&work)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(git.status.success(), "git init: {git:?}");
    let log = root.path().join("side.log");
    let log_path = log.display().to_string();
    for name in [
        "git", "packset", "packsetd", "claimdag", "vissue", "curl", "wget",
    ] {
        script(
            &bin,
            name,
            &format!("#!/bin/sh\nprintf '%s\\n' \"{name} $*\" >> '{log_path}'\nexit 1\n"),
        );
    }
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", &home)
        .current_dir(&work)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "ljos_note", "arguments": {"issue": "probe-2f5j", "text": "the seat had no tracker"}}}),
    ];
    let started = Instant::now();
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let elapsed = started.elapsed();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let side = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        side.is_empty(),
        "discovery ran a side program:\n{side}\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(
        elapsed < Duration::from_secs(8),
        "initialize took {elapsed:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    (stdout, stderr, elapsed)
}

fn answers(stdout: &str, stderr: &str) -> Vec<serde_json::Value> {
    let mut found = Vec::new();
    for line in stdout.lines().filter(|line| !line.is_empty()) {
        let value = serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|err| panic!("stdout is not JSON: {err}: {line}\nstderr:\n{stderr}"));
        found.push(value);
    }
    found
}

fn check(program: &str, args: &[&str]) {
    let (stdout, stderr, _) = session(program, args);
    let parsed = answers(&stdout, &stderr);
    let by_id = |id: u64| {
        parsed
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
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, CORE, "stdout:\n{stdout}");
    let note = by_id(3);
    assert!(note.get("error").is_none(), "{note}");
    assert_eq!(note["result"]["isError"], true, "{note}");
    let text = note["result"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    assert_eq!(text.trim(), MISSING_TRACKER, "{text}");
}

#[test]
fn ljos_mcp_answers_with_no_tracker_pack_or_seat() {
    check(env!("CARGO_BIN_EXE_ljos"), &["mcp"]);
}

#[test]
fn the_mcp_binary_answers_with_no_tracker_pack_or_seat() {
    check(env!("CARGO_BIN_EXE_ljos-mcp"), &[]);
}
