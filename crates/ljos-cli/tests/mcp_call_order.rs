//! A recall a client sends right after a note sees that note.
//!
//! In willma-20261020f w7 and w11, opencode sent `ljos_note` and then
//! `ljos_recall` on one issue 7 ms apart, as one batch. The server ran the
//! two at once, the recall answered first, and it did not list the note.
//! Here `ljos note` takes a second before it stores the note, and the client
//! sends the recall without waiting for the note's answer.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_recall_sent_after_a_note_lists_the_note() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    let tracker = home.path().join("tracker");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"saddle\"]\n").unwrap();
    let notes = home.path().join("notes.txt");
    // `ljos note ID TEXT` stores the note a second after it starts.
    script(
        &bin,
        "ljos",
        r#"#!/bin/sh
if [ "$1" = note ]; then
  sleep 1
  printf '[note, today] %s\n' "$3" >> "$NOTES"
  echo "$2: noted"
  exit 0
fi
exit 0
"#,
    );
    // `vissue recall ID` lists the notes stored so far.
    script(
        &bin,
        "vissue",
        r#"#!/bin/sh
case "$1" in
  recall) echo "$2 STARTED"; cat "$NOTES" 2>/dev/null ;;
  show) echo '{}' ;;
  claims) echo '[]' ;;
esac
exit 0
"#,
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_RUNTIME_DIR", home.path())
        .env("VISSUE_ROOT", &tracker)
        .env("PACKSET_URL", "off")
        .env("NOTES", &notes)
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
            "clientInfo": {"name": "opencode", "version": "1.19.3"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        call(
            10,
            "ljos_note",
            serde_json::json!({"seat": "saddle", "issue": "work-btha",
                "text": "kebab_case reuses slugify"}),
        ),
        call(
            11,
            "ljos_recall",
            serde_json::json!({"seat": "saddle", "issue": "work-btha"}),
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
        if answers.len() == 2 {
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    let text = |id: u64| {
        let answer = answers
            .get(&id)
            .unwrap_or_else(|| panic!("no answer to {id}: {answers:?}"));
        assert!(
            answer.get("error").is_none() && answer["result"]["isError"] != true,
            "{answer}"
        );
        answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    assert!(text(10).contains("noted"), "{}", text(10));
    let recall = text(11);
    assert!(
        recall.contains("[note, today] kebab_case reuses slugify"),
        "the recall did not wait for the note:\n{recall}"
    );
}
