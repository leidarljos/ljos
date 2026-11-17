//! A command verdict retains its budget when memory recall or admission is slow.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct QuietPack {
    address: String,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl QuietPack {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut held = Vec::new();
            while !stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => held.push(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            }
        });
        Self {
            address,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for QuietPack {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn command(home: &Path, mode: &str) -> Command {
    let run = home.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let checker = home.join("ljos-policyd");
    std::fs::write(
        &checker,
        "#!/bin/sh\nsleep 0.15\nprintf 'deny\\tfixture-policy-verdict\\n'\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&checker, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ljos"));
    command
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", home.display()))
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_RUNTIME_DIR", run)
        .env("LJOS_SEAT", "fixture-seat")
        .env("LJOS_GUARD", "on")
        .env("LJOS_GUARD_MODE", mode)
        .env("LJOS_TRACKER_GIT", "off")
        .env("POLICYD_BIN", checker)
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "30000")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn assert_verdict(output: Output, mode: &str, took: Duration) {
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("fixture-policy-verdict"),
        "the checker verdict was lost in {mode} mode: {stdout}\n{stderr}"
    );
    let warning = "warning: fixture-policy-verdict (ljos rule `ljos-policyd`)";
    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&stdout) {
        let verdict = &payload["hookSpecificOutput"];
        assert_eq!(verdict["hookEventName"], "PreToolUse");
        if mode == "strict" {
            assert_eq!(verdict["permissionDecision"], "deny", "{stdout}\n{stderr}");
            assert_eq!(
                verdict["permissionDecisionReason"],
                "fixture-policy-verdict (ljos rule `ljos-policyd`)",
                "{stdout}\n{stderr}"
            );
        } else {
            assert!(verdict.get("permissionDecision").is_none(), "{stdout}");
            assert_eq!(verdict["additionalContext"], warning, "{stdout}\n{stderr}");
        }
    } else {
        let expected = if mode == "strict" {
            "deny\tfixture-policy-verdict"
        } else {
            warning
        };
        assert!(
            stdout.lines().any(|line| line == expected),
            "the checker verdict changed: {stdout}\n{stderr}"
        );
    }
    assert!(
        !stdout.contains("guard is unhealthy") && !stderr.contains("could not judge"),
        "a responsive checker was treated as a fault: {stdout}\n{stderr}"
    );
    assert!(took < Duration::from_secs(4), "verdict took {took:?}");
}

fn recall_keeps_checker_budget(mode: &str) {
    let home = tempfile::tempdir().unwrap();
    let pack = QuietPack::start();
    let start = Instant::now();
    let output = command(home.path(), mode)
        .args(["policy", "--", "printf", "hello"])
        .env("PACKSET_URL", &pack.address)
        .env("PACKSET_WORKSPACE", "seat")
        .env("LJOS_POLICY_RECALL_WAIT", "30")
        .output()
        .unwrap();
    assert_verdict(output, mode, start.elapsed());
}

fn hook_memory_keeps_checker_budget(mode: &str) {
    let home = tempfile::tempdir().unwrap();
    let pack = QuietPack::start();
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "memory-budget-fixture",
        "cwd": home.path(),
        "tool_name": "Bash",
        "tool_input": {"command": "printf hello"}
    });
    let start = Instant::now();
    let mut child = command(home.path(), mode)
        .arg("hook")
        .env("PACKSET_URL", &pack.address)
        .env("PACKSET_WORKSPACE", "seat")
        .env("LJOS_HOOKD", "off")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_verdict(output, mode, start.elapsed());
}

fn admission_keeps_checker_budget(mode: &str) {
    let home = tempfile::tempdir().unwrap();
    let mut command = command(home.path(), mode);
    let socket = home.path().join("run/ljos/hookd.sock");
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(socket).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        assert!(request.starts_with("wait "), "{request}");
        std::thread::sleep(Duration::from_millis(2020));
        let _ = writeln!(stream, "late");
    });
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "budget-fixture",
        "cwd": home.path(),
        "tool_name": "Bash",
        "tool_input": {"command": "printf hello"}
    });
    let start = Instant::now();
    let mut child = command
        .arg("hook")
        .env("PACKSET_URL", "off")
        .env("LJOS_HOOK_ADMIT_MS", "2000")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let took = start.elapsed();
    server.join().unwrap();
    assert_verdict(output, mode, took);
}

#[test]
fn strict_policy_checks_before_waiting_for_recall() {
    recall_keeps_checker_budget("strict");
}

#[test]
fn advisory_policy_checks_before_waiting_for_recall() {
    recall_keeps_checker_budget("advisory");
}

#[test]
fn strict_hook_keeps_checker_time_after_admission() {
    admission_keeps_checker_budget("strict");
}

#[test]
fn advisory_hook_keeps_checker_time_after_admission() {
    admission_keeps_checker_budget("advisory");
}

#[test]
fn strict_hook_checks_before_reading_memory_rules() {
    hook_memory_keeps_checker_budget("strict");
}

#[test]
fn advisory_hook_checks_before_reading_memory_rules() {
    hook_memory_keeps_checker_budget("advisory");
}
