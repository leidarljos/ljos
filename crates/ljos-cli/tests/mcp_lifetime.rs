//! An ljos-mcp ends when its input does or its parent dies, and answers
//! what it already read first.
//!
//! The pack here says when a search reaches it and answers seven seconds
//! later, longer than the five seconds rmcp waits for a handler once the
//! input ends. The client closes stdin as soon as the search is in
//! flight; the answer still arrives and the server exits cleanly.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Past rmcp's five-second drain after the end of input.
const SLOW_SEARCH: Duration = Duration::from_secs(7);

fn slow_pack(arrived: mpsc::Sender<()>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let arrived = arrived.clone();
            std::thread::spawn(move || {
                let mut head = [0u8; 4096];
                let n = stream.read(&mut head).unwrap_or(0);
                let line = String::from_utf8_lossy(&head[..n]);
                let payload = if line.starts_with("GET /v1/search") {
                    let _ = arrived.send(());
                    std::thread::sleep(SLOW_SEARCH);
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
    let (arrived, search_arrived) = mpsc::channel();
    let mut child = server(home.path())
        .env("PACKSET_URL", slow_pack(arrived))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (exited, watchdog) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        if watchdog.recv_timeout(Duration::from_secs(60)).is_err() {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    });
    let mut stdin = child.stdin.take().unwrap();
    for line in initialize() {
        writeln!(stdin, "{line}").unwrap();
    }
    let search = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "ljos_search", "arguments": {"query": "review set"}}});
    writeln!(stdin, "{search}").unwrap();
    search_arrived
        .recv_timeout(Duration::from_secs(30))
        .expect("the search never reached the pack");
    let started = Instant::now();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let took = started.elapsed();
    let _ = exited.send(());
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

/// `R` running, `Z` exited and not yet reaped, `None` reaped.
#[cfg(target_os = "linux")]
fn state(pid: u32) -> Option<char> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("State:"))
        .and_then(|rest| rest.trim().chars().next())
}

/// A shell starts the server with stdin on the test's pipe and is then
/// killed. The pipe stays open, so only the parent's death can end it.
#[test]
#[cfg(target_os = "linux")]
fn the_parents_death_ends_the_server_with_stdin_still_open() {
    use std::io::BufRead as _;
    let home = tempfile::tempdir().unwrap();
    let template = server(home.path());
    let mut child = Command::new("/bin/sh")
        .args([
            "-c",
            "exec 3<&0; \"$0\" <&3 3<&- & echo $!; exec sleep 600",
            env!("CARGO_BIN_EXE_ljos-mcp"),
        ])
        .env_clear()
        .envs(template.get_envs().filter_map(|(k, v)| Some((k, v?))))
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let pid: u32 = line.trim().parse().expect("the server's pid");
    for line in initialize() {
        writeln!(stdin, "{line}").unwrap();
    }
    loop {
        line.clear();
        assert!(stdout.read_line(&mut line).unwrap() > 0, "server closed");
        if serde_json::from_str::<serde_json::Value>(&line).is_ok_and(|value| value["id"] == 1) {
            break;
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && matches!(state(pid), Some(s) if s != 'Z') {
        std::thread::sleep(Duration::from_millis(100));
    }
    let left = state(pid);
    if matches!(left, Some(s) if s != 'Z') {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    drop(stdin);
    assert!(
        matches!(left, None | Some('Z')),
        "the server outlived its parent by 10 s: state {left:?}"
    );
}
