//! An MCP search appends one `recall.log` line for the seat the call named.
//!
//! `ljos search` writes `ts seat search query`. `ljos_search` writes that
//! same line. The seat is the call's `seat` argument, ahead of `LJOS_SEAT`.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::Duration;

/// A pack that answers at once, so the search returns and the log is the
/// thing under test.
fn pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = r#"{"hits":[],"atoms":[]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

#[test]
fn mcp_search_appends_one_recall_line_for_the_call_seat() {
    let home = tempfile::tempdir().unwrap();
    let tracker = home.path().join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::write(
        tracker.join("vissue.toml"),
        "seats = [\"saddle\", \"other\"]\n",
    )
    .unwrap();
    let state = home.path().join("state");
    let url = pack();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", &state)
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("LJOS_SEAT", "other")
        .env("PACKSET_URL", &url)
        .env("PACKSET_TOKEN", "probe-token")
        .env("PACKSET_WORKSPACE", "saddle")
        .env("PACKSET_TIMEOUT_MS", "800")
        .env("LJOS_SEARCH_WAIT", "0")
        .env("LJOS_SEARCH_WARMUP_WAIT", "0")
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "ljos_search",
            "arguments": {"query": "kiln fire", "seat": "saddle"}}}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "exit {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
    let answer = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value["id"] == 2)
        .unwrap_or_else(|| panic!("no answer to the search\n{stdout}\n{stderr}"));
    assert!(
        answer.get("error").is_none(),
        "search failed\n{answer}\n{stderr}"
    );

    let log = std::fs::read_to_string(state.join("ljos/recall.log"))
        .unwrap_or_else(|err| panic!("recall.log was not written: {err}\n{stderr}"));
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 1, "{log}");
    let mut parts = lines[0].splitn(4, ' ');
    let ts = parts.next().unwrap_or("");
    let seat = parts.next().unwrap_or("");
    let verb = parts.next().unwrap_or("");
    let what = parts.next().unwrap_or("");
    assert!(ts.ends_with('Z'), "{log}");
    assert_eq!(seat, "saddle", "{log}");
    assert_eq!(verb, "search", "{log}");
    assert_eq!(what, "kiln fire", "{log}");
}
