//! An ljos-mcp ends when its input does, and answers what it already
//! read first.
//!
//! The pack here takes seven seconds to answer a search, longer than the
//! five seconds rmcp waits for a handler once the input ends. The client
//! sends the search and closes stdin at once; the answer still arrives
//! and the server exits cleanly.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn slow_pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            std::thread::spawn(move || {
                let mut head = [0u8; 4096];
                let n = stream.read(&mut head).unwrap_or(0);
                let line = String::from_utf8_lossy(&head[..n]);
                let payload = if line.starts_with("GET /v1/search") {
                    std::thread::sleep(Duration::from_secs(7));
                    r#"{"hits":[{"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","text":"Pin the review set before a release.","kind":"preference","score":1.0}]}"#
                } else {
                    "{}"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

fn server(home: &std::path::Path) -> Command {
    let tracker = home.join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"probe\"]\n").unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("VISSUE_ROOT", &tracker)
        .env("LJOS_SEAT", "probe")
        .env("PACKSET_WORKSPACE", "probe")
        .current_dir(home);
    cmd
}

fn initialize() -> [serde_json::Value; 2] {
    [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ]
}

#[test]
fn stdin_eof_answers_the_call_in_flight_then_exits() {
    let home = tempfile::tempdir().unwrap();
    let mut child = server(home.path())
        .env("PACKSET_URL", slow_pack())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    for line in initialize() {
        writeln!(stdin, "{line}").unwrap();
    }
    let search = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "ljos_search", "arguments": {"query": "review set"}}});
    writeln!(stdin, "{search}").unwrap();
    let started = Instant::now();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let took = started.elapsed();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "{:?}\n{stdout}", out.status);
    assert!(took < Duration::from_secs(45), "took {took:?}");
    let answer = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value["id"] == 2)
        .unwrap_or_else(|| panic!("no answer to the search after {took:?}:\n{stdout}"));
    assert!(
        answer.to_string().contains("Pin the review set"),
        "{answer}"
    );
}
