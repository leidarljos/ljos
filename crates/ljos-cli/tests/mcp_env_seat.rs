//! `ljos mcp` explains that `LJOS_SEAT` applies to reads only when a
//! write omits `seat`, and executes multi-seat writes when `seat` is passed.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;
#[path = "common/sibling.rs"]
mod sibling;

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn call_mcp(
    path: &str,
    home: &Path,
    tracker: &Path,
    pack_url: &str,
    pack_home: &Path,
    seat: Option<&str>,
    calls: &[(&str, serde_json::Value)],
) -> Vec<serde_json::Value> {
    call_mcp_env(path, home, tracker, pack_url, pack_home, seat, &[], calls)
}

#[allow(clippy::too_many_arguments)]
fn call_mcp_env(
    path: &str,
    home: &Path,
    tracker: &Path,
    pack_url: &str,
    pack_home: &Path,
    seat: Option<&str>,
    env: &[(&str, &str)],
    calls: &[(&str, serde_json::Value)],
) -> Vec<serde_json::Value> {
    let runtime = home.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    cmd.env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("VISSUE_ROOT", tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", pack_url)
        .env("PACKSET_HOME", pack_home)
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_MCP_TOOLS", "all")
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    seat_env::clear(&mut cmd);
    if let Some(seat) = seat {
        cmd.env("LJOS_SEAT", seat);
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let read_response = |stdout: &mut BufReader<_>, id| {
        let mut line = String::new();
        loop {
            line.clear();
            assert_ne!(
                stdout.read_line(&mut line).unwrap(),
                0,
                "no response for {id}"
            );
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if value["id"] == id {
                break value;
            }
        }
    };
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "probe", "version": "0"}
            }
        })
    )
    .unwrap();
    stdin.flush().unwrap();
    let initialized = read_response(&mut stdout, 1);
    assert!(initialized["result"].is_object(), "{initialized}");
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();
    let mut answers = Vec::with_capacity(calls.len());
    for (idx, (name, args)) in calls.iter().enumerate() {
        let id = idx + 2;
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": name, "arguments": args}
            })
        )
        .unwrap();
        stdin.flush().unwrap();
        answers.push(read_response(&mut stdout, id));
    }
    drop(stdin);
    drop(stdout);
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(!stderr.contains("panicked"), "stderr:\n{stderr}");
    answers
}

fn result_text(answer: &serde_json::Value) -> String {
    answer["result"]["structuredContent"]["text"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            answer["result"]["content"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()
        })
}

#[test]
fn mcp_write_refusal_explains_ljos_seat_and_explicit_seat_succeeds() {
    let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
    let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));
    let pack = ljos_test_boot::Packsetd::open();

    let dir = tempfile::tempdir().unwrap();
    let alice_home = dir.path().join("alice");
    let bob_home = dir.path().join("bob");
    let tracker = dir.path().join("tracker");
    std::fs::create_dir_all(&alice_home).unwrap();
    std::fs::create_dir_all(&bob_home).unwrap();
    std::fs::create_dir_all(tracker.join("Software/demo")).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        tracker.join("Software/demo/issues.org"),
        "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] Pick a codec
:PROPERTIES:
:ID:         demo-env1
:END:

Options: keep, drop
",
    )
    .unwrap();

    let siblings = vissue.parent().unwrap();
    assert_eq!(claimdag.parent(), Some(siblings));
    let path = format!("{}:/usr/bin:/bin", siblings.display());

    // Even with LJOS_SEAT set, a write without `seat` is refused and explains why.
    let no_arg = call_mcp(
        &path,
        &alice_home,
        &tracker,
        pack.url(),
        pack.pack_home(),
        Some("alice"),
        &[("ljos_claim", serde_json::json!({"node": "demo-env1"}))],
    );
    assert_eq!(no_arg.len(), 1, "{no_arg:?}");
    assert_eq!(no_arg[0]["result"]["isError"], true, "{no_arg:?}");
    let refusal = result_text(&no_arg[0]);
    assert!(
        refusal.contains("LJOS_SEAT") && refusal.contains("seat"),
        "{refusal}"
    );

    // Passing `seat` explicitly with `LJOS_SEAT` set succeeds across multi-seat MCP tools.
    let alice_answers = call_mcp(
        &path,
        &alice_home,
        &tracker,
        pack.url(),
        pack.pack_home(),
        Some("alice"),
        &[
            (
                "ljos_claim",
                serde_json::json!({"seat": "alice", "node": "demo-env1"}),
            ),
            (
                "ljos_send",
                serde_json::json!({"seat": "alice", "to": "bob", "text": "please review", "issue": "demo-env1", "new_seat": true}),
            ),
            (
                "ljos_vote",
                serde_json::json!({"seat": "alice", "issue": "demo-env1", "choice": "keep", "used": "none"}),
            ),
        ],
    );
    assert_eq!(alice_answers.len(), 3, "{alice_answers:?}");
    for ans in &alice_answers {
        assert_ne!(ans["result"]["isError"], true, "{ans}");
    }
    let send_text = result_text(&alice_answers[1]);
    let msg_id = send_text
        .split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("no msg id in {send_text:?}"))
        .to_string();

    let bob_inbox = call_mcp(
        &path,
        &bob_home,
        &tracker,
        pack.url(),
        pack.pack_home(),
        Some("bob"),
        &[("ljos_inbox", serde_json::json!({"seat": "bob"}))],
    );
    assert_eq!(bob_inbox.len(), 1, "{bob_inbox:?}");
    assert_ne!(bob_inbox[0]["result"]["isError"], true, "{bob_inbox:?}");
    assert!(
        result_text(&bob_inbox[0]).contains("please review"),
        "{bob_inbox:?}"
    );

    let bob_reply = call_mcp(
        &path,
        &bob_home,
        &tracker,
        pack.url(),
        pack.pack_home(),
        Some("bob"),
        &[(
            "ljos_reply",
            serde_json::json!({"seat": "bob", "id": msg_id, "text": "looks good"}),
        )],
    );
    assert_eq!(bob_reply.len(), 1, "{bob_reply:?}");
    assert_ne!(bob_reply[0]["result"]["isError"], true, "{bob_reply:?}");
    assert!(
        result_text(&bob_reply[0]).contains("from bob to alice"),
        "{bob_reply:?}"
    );
}

#[test]
fn ljos_vote_schema_describes_issue_options() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"}}
        })
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    )
    .unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let list = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|v| v["id"] == 2)
        .unwrap_or_else(|| panic!("no tools/list in {stdout}"));
    let tools = list["result"]["tools"].as_array().unwrap();
    let vote = tools
        .iter()
        .find(|t| t["name"] == "ljos_vote")
        .expect("ljos_vote");
    let desc = vote["inputSchema"]["properties"]["choice"]["description"]
        .as_str()
        .unwrap_or("");
    assert!(
        desc.contains("`Options:`") && desc.contains("`ljos_options`"),
        "ljos_vote choice schema does not say where the options come from: {desc:?}"
    );
    assert!(
        !desc.contains("--options"),
        "ljos_vote choice schema names a command line flag: {desc:?}"
    );
}

fn listed_tools() -> Vec<serde_json::Value> {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir.path())
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"}}
        }),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let list = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|v| v["id"] == 2)
        .unwrap_or_else(|| panic!("no tools/list in {stdout}"));
    list["result"]["tools"].as_array().unwrap().clone()
}

/// A stamped conversation with `LJOS_SEAT` set writes under that seat with
/// no `seat` argument, so a tool must not say `LJOS_SEAT` is unused.
#[test]
fn a_stamped_conversation_writes_under_ljos_seat_and_the_schema_says_so() {
    let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
    let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));
    let pack = ljos_test_boot::Packsetd::open();

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("alice");
    let tracker = dir.path().join("tracker");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(tracker.join("Software/demo")).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        tracker.join("Software/demo/issues.org"),
        "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] Pick a codec
:PROPERTIES:
:ID:         demo-st4m
:END:
",
    )
    .unwrap();
    let siblings = vissue.parent().unwrap();
    assert_eq!(claimdag.parent(), Some(siblings));
    let path = format!("{}:/usr/bin:/bin", siblings.display());

    let answers = call_mcp_env(
        &path,
        &home,
        &tracker,
        pack.url(),
        pack.pack_home(),
        Some("alice"),
        &[
            (
                "CURSOR_CONVERSATION_ID",
                "bc-872164c1-e14f-56d0-a32e-fc0bd5afad41",
            ),
            ("CURSOR_AGENT", "1"),
        ],
        &[("ljos_claim", serde_json::json!({"node": "demo-st4m"}))],
    );
    assert_eq!(answers.len(), 1, "{answers:?}");
    let text = result_text(&answers[0]);
    assert_ne!(answers[0]["result"]["isError"], true, "{text}");
    assert!(!text.contains("applies to reads only"), "{text}");
    let org = std::fs::read_to_string(tracker.join("Software/demo/issues.org")).unwrap();
    assert!(
        org.contains(":CLAIMED_BY: alice"),
        "the claim was not written under LJOS_SEAT:\n{org}"
    );

    for tool in listed_tools() {
        let name = tool["name"].as_str().unwrap_or("");
        if !matches!(name, "ljos_claim" | "ljos_release" | "ljos_sitting") {
            continue;
        }
        let said = tool["inputSchema"]["properties"]["assignee"]["description"]
            .as_str()
            .unwrap_or("");
        assert!(
            said.contains("LJOS_SEAT") && !said.contains("not used"),
            "{name} assignee says LJOS_SEAT is unused on writes, which a stamped conversation shows is false: {said:?}"
        );
        assert!(!said.contains("seats.toml"), "{name}: {said:?}");
    }
}
