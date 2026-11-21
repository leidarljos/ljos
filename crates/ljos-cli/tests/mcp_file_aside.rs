//! `ljos_file` answers with the new id and the ticket's line, and no path
//! on the person's disk.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn tool_dir(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).find(|dir| dir.join(name).is_file())
}

#[test]
fn filing_a_ticket_does_not_print_the_tracker_file_path() {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let tracker = home.path().join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"ada\"]\n").unwrap();
    let ljos = Path::new(env!("CARGO_BIN_EXE_ljos")).parent().unwrap();
    let vissue = tool_dir("vissue").expect("vissue");
    let path = format!("{}:{}:/usr/bin:/bin", ljos.display(), vissue.display());
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("TMPDIR", std::env::temp_dir())
        .env("PATH", path)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("LJOS_MCP_ORPHAN_IDLE", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |value: serde_json::Value| writeln!(stdin, "{value}").unwrap();
    send(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}
    }));
    send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "ljos_file", "arguments": {
            "title": "glaze crawls", "project": "kiln", "top": true, "seat": "ada"}}
    }));
    let mut answer = None;
    let mut line = String::new();
    while stdout.read_line(&mut line).unwrap() > 0 {
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        if value["id"] == 2 {
            answer = Some(value);
            break;
        }
        line.clear();
    }
    let _ = child.kill();
    let _ = child.wait();
    let answer = answer.expect("an answer to ljos_file");
    let said = answer["result"]["structuredContent"].clone();
    assert!(
        said["text"].as_str().unwrap().starts_with("kiln-"),
        "{said}"
    );
    let aside = said["aside"].as_str().unwrap_or("");
    assert!(aside.contains("glaze crawls"), "{said}");
    assert!(!aside.contains("file: "), "{said}");
    assert!(
        !aside.contains(home.path().to_str().unwrap()),
        "the aside names the person's disk: {said}"
    );
}
