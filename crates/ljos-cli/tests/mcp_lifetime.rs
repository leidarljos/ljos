//! An ljos-mcp ends when its input does or its client dies, and answers
//! what it already read first. A shell that only launched it is not the
//! client.
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

/// Gone, or exited and not yet reaped. A zombie still counts: the client
/// is dead, and `kill -0` would still succeed.
#[cfg(unix)]
fn process_ended(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            return true;
        };
        status
            .lines()
            .find_map(|line| line.strip_prefix("State:"))
            .and_then(|rest| rest.trim().chars().next())
            .is_some_and(|state| state == 'Z' || state == 'X')
    }
    #[cfg(not(target_os = "linux"))]
    {
        let Ok(out) = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
        else {
            return true;
        };
        if !out.status.success() {
            return true;
        }
        let stat = String::from_utf8_lossy(&out.stdout);
        let stat = stat.trim();
        stat.is_empty() || stat.starts_with('Z')
    }
}

#[cfg(unix)]
fn stop(pid: u32) {
    let _ = Command::new("kill").arg(pid.to_string()).status();
}

/// `python3` is the client. It is not a shell, so it is the ancestor the
/// server records. The test keeps the write end of stdin.
#[cfg(unix)]
const CLIENT_LAUNCH: &str = r#"
import sys, subprocess, time
mcp = sys.argv[1]
child = subprocess.Popen(
    [mcp], stdin=sys.stdin, stdout=sys.stdout, stderr=subprocess.DEVNULL
)
print(child.pid, flush=True)
sys.stdin.close()
time.sleep(600)
"#;

/// `python3` starts a shell that starts the server and then sleeps. The
/// shell is a launcher. Killing python leaves the shell, and the server
/// must still exit. The test keeps the write end of stdin.
#[cfg(unix)]
const LAUNCHER_LAUNCH: &str = r#"
import os, sys, subprocess, time
mcp = sys.argv[1]
os.dup2(1, 3)
sh = subprocess.Popen(
    ["/bin/sh", "-c", 'exec 4<&0; "$0" <&4 >&3 2>/dev/null & echo $!; exec 3>&-; exec 4>&-; sleep 600', mcp],
    stdin=sys.stdin,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    pass_fds=(3,),
)
mcp_pid = sh.stdout.readline().decode().strip()
print(f"{sh.pid} {mcp_pid}", flush=True)
sys.stdin.close()
time.sleep(600)
"#;

#[cfg(unix)]
fn python_client(home: &std::path::Path, script: &str) -> std::process::Child {
    let template = server(home);
    Command::new("python3")
        .args(["-c", script, env!("CARGO_BIN_EXE_ljos-mcp")])
        .env_clear()
        .envs(template.get_envs().filter_map(|(k, v)| Some((k, v?))))
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

#[cfg(unix)]
fn await_initialize(stdout: &mut impl std::io::BufRead) {
    let mut line = String::new();
    loop {
        line.clear();
        assert!(
            stdout.read_line(&mut line).unwrap() > 0,
            "the server closed before initialize"
        );
        if serde_json::from_str::<serde_json::Value>(&line).is_ok_and(|value| value["id"] == 1) {
            break;
        }
    }
}

/// `pid` has exited within `limit`, or the last look says it has not.
#[cfg(unix)]
fn ended_within(pid: u32, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if process_ended(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    process_ended(pid)
}

/// The client is python, not a shell. Killing it leaves stdin open in
/// this test. The server must exit.
#[test]
#[cfg(unix)]
fn the_clients_death_ends_the_server_with_stdin_still_open() {
    use std::io::BufRead as _;
    let home = tempfile::tempdir().unwrap();
    let mut child = python_client(home.path(), CLIENT_LAUNCH);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut err = child.stderr.take().unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let pid: u32 = line
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("the server's pid from {line:?}"));
    for line in initialize() {
        writeln!(stdin, "{line}").unwrap();
    }
    await_initialize(&mut stdout);
    child.kill().unwrap();
    child.wait().unwrap();
    let ended = ended_within(pid, Duration::from_secs(10));
    if !ended {
        stop(pid);
    }
    drop(stdin);
    let mut stderr = String::new();
    let _ = err.read_to_string(&mut stderr);
    assert!(ended, "the server outlived its client by 10 s\n{stderr}");
}

/// No request for the orphan window. An editor still above the server
/// keeps it; otherwise it exits. Stdin stays open either way.
#[test]
#[cfg(unix)]
fn an_orphan_exits_only_when_no_editor_holds_it() {
    let home = tempfile::tempdir().unwrap();
    let mut child = server(home.path())
        .env("LJOS_MCP_ORPHAN_IDLE", "2")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let watch = pid;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        stop(watch);
    });
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    for line in initialize() {
        writeln!(stdin, "{line}").unwrap();
    }
    await_initialize(&mut stdout);
    let held = ljos_cli::proc_tree::editor_above();
    if held {
        std::thread::sleep(Duration::from_secs(6));
        let alive = !process_ended(pid);
        let _ = child.kill();
        let _ = child.wait();
        drop(stdin);
        assert!(alive, "an editor above the server and it still exited");
    } else {
        let ended = ended_within(pid, Duration::from_secs(12));
        if !ended {
            let _ = child.kill();
        }
        let _ = child.wait();
        drop(stdin);
        assert!(ended, "no editor above the server and it stayed up");
    }
}

/// A shell between the client and the server stays up. The server still
/// exits, because the client it recorded was the process behind the shell.
#[test]
#[cfg(unix)]
fn a_surviving_launcher_does_not_keep_the_server() {
    use std::io::BufRead as _;
    let home = tempfile::tempdir().unwrap();
    let mut child = python_client(home.path(), LAUNCHER_LAUNCH);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut err = child.stderr.take().unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let mut ids = line.split_whitespace();
    let sh_pid: u32 = ids
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("the shell's pid from {line:?}"));
    let mcp_pid: u32 = ids
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("the server's pid from {line:?}"));
    for line in initialize() {
        writeln!(stdin, "{line}").unwrap();
    }
    await_initialize(&mut stdout);
    child.kill().unwrap();
    child.wait().unwrap();
    let ended = ended_within(mcp_pid, Duration::from_secs(10));
    let launcher_alive = !process_ended(sh_pid);
    if !ended {
        stop(mcp_pid);
    }
    stop(sh_pid);
    drop(stdin);
    let mut stderr = String::new();
    let _ = err.read_to_string(&mut stderr);
    assert!(
        ended,
        "the server outlived the client behind its launcher by 10 s\n{stderr}"
    );
    assert!(
        launcher_alive,
        "the launcher died with the client\n{stderr}"
    );
}
