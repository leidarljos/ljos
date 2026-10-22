//! An idle ljos-mcp gives back what a burst of calls took.
//!
//! The pack here lists twenty thousand atoms, so one `ljos_doctor` grows
//! the server by tens of MiB. With `LJOS_MCP_IDLE_DROP=1` its anonymous
//! memory falls back near where it started within a few seconds.
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn listing() -> String {
    let atoms: Vec<_> = (0..20_000)
        .map(|i| {
            serde_json::json!({
                "id": format!("{i:032x}"),
                "workspace": "probe",
                "kind": "preference",
                "text": format!("Run the release review set before tagging, item {i}."),
                "origin": "user-declared",
                "ts": "2026-10-22T00:00:00Z",
                "updated": "2026-10-22T00:00:00Z",
                "links": [],
                "review": {"reps": 0, "interval_s": 86400, "ease": 2.5},
            })
        })
        .collect();
    serde_json::json!({ "atoms": atoms }).to_string()
}

fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = listing();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut head = [0u8; 4096];
            let n = stream.read(&mut head).unwrap_or(0);
            let line = String::from_utf8_lossy(&head[..n]);
            let payload = if line.starts_with("GET /v1/atoms") {
                body.as_str()
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(payload.as_bytes());
        }
    });
    format!("http://{addr}")
}

/// Anonymous pages only: the code a call first runs stays mapped from the
/// binary and is not what the drop returns.
fn anon_kib(pid: u32) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    status
        .lines()
        .find_map(|line| line.strip_prefix("RssAnon:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse().ok())
        .unwrap()
}

#[test]
fn an_idle_server_returns_the_burst_to_the_system() {
    let home = tempfile::tempdir().unwrap();
    let tracker = home.path().join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"probe\"]\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("VISSUE_ROOT", &tracker)
        .env("LJOS_SEAT", "probe")
        .env("PACKSET_URL", serve())
        .env("PACKSET_WORKSPACE", "probe")
        .env("LJOS_MCP_IDLE_DROP", "1")
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut answer = |want: u64| loop {
        let mut line = String::new();
        assert!(stdout.read_line(&mut line).unwrap() > 0, "server closed");
        if serde_json::from_str::<serde_json::Value>(&line).is_ok_and(|value| value["id"] == want) {
            return;
        }
    };
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    answer(1);
    std::thread::sleep(Duration::from_secs(3));
    let idle = anon_kib(pid);
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "ljos_doctor", "arguments": {}}});
    writeln!(stdin, "{call}").unwrap();
    answer(2);
    let burst = anon_kib(pid);
    assert!(
        burst > idle + 20 * 1024,
        "the listing did not grow the server: {idle} KiB idle, {burst} KiB after"
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut now = burst;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        now = anon_kib(pid);
        if now < idle + 8 * 1024 {
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    assert!(
        now < idle + 8 * 1024,
        "still {now} KiB after the idle drop: {idle} KiB idle, {burst} KiB after the burst"
    );
}
