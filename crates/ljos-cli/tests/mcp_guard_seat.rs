//! `ljos_guard_check` answers with its verdict whatever the seat.
//!
//! Codex on a laptop passed `seat: codex` to a tracker that listed no seat
//! by that name. The server refused the call before the guard ran, with
//! "no seat named codex on this tracker" and a list of thread ids, so the
//! agent could run nothing. The guard acts under no seat, so an unknown or
//! missing seat must not stop it. A tool that acts under the seat still
//! refuses a name the call made up, in one short line with named seats.

ljos_test_boot::harness!();
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

const THREAD_ID: &str = "01a0dab3-895e-7533-93ca-c9807d5cb680";

#[test]
fn the_guard_answers_for_a_seat_the_tracker_does_not_know() {
    let home = tempfile::tempdir().unwrap();
    let tracker = home.path().join("tracker");
    std::fs::create_dir_all(tracker.join("Software/probe")).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"inky\"]\n").unwrap();
    std::fs::write(
        tracker.join("Software/probe/issues.org"),
        format!(
            "* STARTED [#C] held\n:PROPERTIES:\n:ID:         probe-abcd\n:CLAIMED_BY: {THREAD_ID}\n:END:\n"
        ),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_RUNTIME_DIR", home.path())
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    // A server that never answers fails the test instead of hanging it.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let call = |id: u32, name: &str, args: serde_json::Value| {
        serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": args}})
    };
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "codex-mcp-client", "version": "0.50.0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        call(
            10,
            "ljos_guard_check",
            serde_json::json!({"seat": "nobody", "command": "git status"}),
        ),
        call(
            11,
            "ljos_guard_check",
            serde_json::json!({"command": "rm -rf /srv/data"}),
        ),
        call(
            12,
            "ljos_claim",
            serde_json::json!({"seat": "nobody", "node": "probe-abcd"}),
        ),
        call(
            13,
            "ljos_guard_check",
            serde_json::json!({"seat": "codex", "command": "git status"}),
        ),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();
    let mut answers = std::collections::BTreeMap::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(id) = value["id"].as_u64().filter(|id| *id >= 10) {
            answers.insert(id, value);
        }
        if answers.len() == 4 {
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    let answer = |id: u64| {
        answers
            .get(&id)
            .unwrap_or_else(|| panic!("no answer to {id}: {answers:?}"))
            .clone()
    };
    let text = |value: &serde_json::Value| {
        value["result"]["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    let verdict = |value: &serde_json::Value| {
        assert!(
            value.get("error").is_none() && value["result"]["isError"] != true,
            "the guard was refused: {value}"
        );
        if let Some(v) = value["result"]["structuredContent"]["verdict"].as_str() {
            return v.to_string();
        }
        let first = value["result"]["content"][0]["text"].as_str().unwrap_or("");
        serde_json::from_str::<serde_json::Value>(first).unwrap()["verdict"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    assert_eq!(verdict(&answer(10)), "allow", "an unknown seat");
    assert_eq!(verdict(&answer(13)), "allow", "a harness id");
    // With no seat at all the guard still reads the line. A delete outside
    // the tree is held or refused by the rules, never by the seat.
    let held = answer(11);
    let said = text(&held);
    assert!(!said.contains("no seat named"), "{said}");
    assert!(!said.contains("pass seat"), "{said}");
    assert!(said.contains("/srv/data"), "{held}");
    let claim = answer(12);
    assert_eq!(claim["result"]["isError"], true, "{claim}");
    let said = text(&claim);
    assert!(said.contains("no seat named nobody"), "{said}");
    assert!(said.contains("inky"), "{said}");
    assert!(!said.contains(THREAD_ID), "{said}");
    assert!(!said.contains("01a0dab3"), "{said}");
}
