//! A Cursor read answered early must not cost other events, break the
//! writer's pipe, or read a call as a different event than the parser.
#![cfg(target_os = "linux")]

ljos_test_boot::harness!();
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[path = "common/seat_root.rs"]
mod seat_root;

const ALLOW: &str = "{\"permission\":\"allow\"}\n";

struct Hab {
    root: PathBuf,
    home: PathBuf,
    run: PathBuf,
    repo: PathBuf,
}

impl Drop for Hab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn hab(name: &str) -> Hab {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp).join(format!(
        "hr-{name}-{}-{}",
        std::process::id(),
        nanos % 1_000_000_000
    ));
    let hab = Hab {
        home: root.join("home"),
        run: root.join("run"),
        repo: root.join("repo"),
        root,
    };
    for dir in [&hab.home, &hab.run, &hab.repo] {
        std::fs::create_dir_all(dir).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hab.run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let git = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&hab.repo)
        .status()
        .unwrap();
    assert!(git.success());
    hab
}

fn hook(hab: &Hab, queue: bool) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("hook")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &hab.home)
        .env("XDG_CONFIG_HOME", hab.home.join(".config"))
        .env("XDG_DATA_HOME", hab.home.join(".local/share"))
        .env("XDG_CACHE_HOME", hab.home.join(".cache"))
        .env("XDG_STATE_HOME", hab.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &hab.run)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_PROJECT", "probe")
        .env("LJOS_SEAT", "probe-seat")
        .env("PACKSET_URL", "off")
        .current_dir(&hab.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !queue {
        cmd.env("LJOS_HOOKD", "off");
    }
    cmd
}

/// Feed `payload` the way a hook runner does: one blocking write, then
/// close. The write's own result is returned, not swallowed.
fn call(
    mut cmd: Command,
    payload: &[u8],
    limit: Duration,
) -> (Duration, Output, std::io::Result<()>) {
    let started = Instant::now();
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let body = payload.to_vec();
    let writer = std::thread::spawn(move || {
        let wrote = stdin.write_all(&body);
        drop(stdin);
        wrote
    });
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = started.elapsed();
    let out = child.wait_with_output().unwrap();
    (took, out, writer.join().unwrap())
}

/// Feed `payload` and wait until the hook exits. The wait is the process.
/// Fifteen seconds with no exit is a hang, and the test says so.
fn call_wait(mut cmd: Command, payload: &[u8]) -> (Duration, Output, std::io::Result<()>) {
    let started = Instant::now();
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let body = payload.to_vec();
    let writer = std::thread::spawn(move || {
        let wrote = stdin.write_all(&body);
        drop(stdin);
        wrote
    });
    let pid = child.id();
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if flag.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // SAFETY: a signal to the hook this test started.
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    });
    let out = child.wait_with_output().unwrap();
    done.store(true, Ordering::Relaxed);
    let took = started.elapsed();
    assert!(
        took < Duration::from_secs(15),
        "hook did not exit within 15s\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (took, out, writer.join().unwrap())
}

fn cursor(event: &str, extra: serde_json::Value) -> String {
    let mut v = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-review",
        "hook_event_name": event,
    });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v.to_string()
}

fn permission(out: &Output) -> String {
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<serde_json::Value>(text.trim())
        .ok()
        .and_then(|v| v["permission"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("no permission in {text:?}"))
}

/// The early read scan used to run after each 4 KiB chunk over the whole
/// buffer. A large payload that is not a read then costs time quadratic
/// in its size. With the queue off, three seconds is the one-pass bound.
/// With the queue on, the wait is the process: admission is not part of
/// that bound.
#[test]
fn a_large_tool_payload_is_read_in_one_pass() {
    let hab = hab("onepass");
    let body = "x".repeat(8 * 1024 * 1024);
    let cases = [
        (
            "preToolUse Write",
            cursor(
                "preToolUse",
                serde_json::json!({"tool_name": "Write", "tool_use_id": "u1",
                    "tool_input": {"file_path": hab.repo.join("big.txt"), "content": body}}),
            ),
        ),
        (
            "postToolUse Read",
            cursor(
                "postToolUse",
                serde_json::json!({"tool_name": "Read", "tool_use_id": "u2",
                    "tool_input": {"file_path": "/w/a"}, "tool_output": body}),
            ),
        ),
        (
            "beforeMCPExecution",
            cursor(
                "beforeMCPExecution",
                serde_json::json!({"tool_name": "ljos_note", "mcp_server_name": "ljos",
                    "tool_input": serde_json::json!({"text": body}).to_string()}),
            ),
        ),
    ];
    for queue in [false, true] {
        for (what, payload) in &cases {
            let label = format!("{what} queue={queue}");
            let (took, out, wrote) = call_wait(hook(&hab, queue), payload.as_bytes());
            assert!(wrote.is_ok(), "{label}: the writer saw {wrote:?}");
            assert_eq!(
                out.status.code(),
                Some(0),
                "{label}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            if !queue {
                assert!(
                    took < Duration::from_secs(3),
                    "{label} took {took:?}\n{}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }
}

/// A runner writes the whole payload and then reads the answer. Exiting
/// with stdin unread makes that write fail with EPIPE, which a Node runner
/// without an 'error' listener raises as an uncaught exception.
#[test]
fn a_large_read_leaves_no_broken_pipe_for_the_writer() {
    let hab = hab("epipe");
    let payload = cursor(
        "beforeReadFile",
        serde_json::json!({"file_path": "/w/a", "content": "x".repeat(2 * 1024 * 1024)}),
    );
    for queue in [false, true] {
        let label = format!("2 MB read queue={queue}");
        let (took, out, wrote) = call(
            hook(&hab, queue),
            payload.as_bytes(),
            Duration::from_secs(5),
        );
        assert_eq!(out.status.code(), Some(0), "{label}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), ALLOW, "{label}");
        assert!(took < Duration::from_millis(1500), "{label} took {took:?}");
        assert!(wrote.is_ok(), "{label}: the writer saw {wrote:?}");
    }
}

/// A payload whose last `hook_event_name` is preToolUse is a preToolUse to
/// the parser. The seat guard denies a write to the hooks file. The early
/// read answer must not allow it because an earlier key said
/// beforeReadFile, before or after a large field.
#[test]
fn a_second_event_key_still_meets_the_seat_guard() {
    let hab = hab("dupkey");
    let hooks = hab.home.join(".cursor/hooks.json");
    let tail = serde_json::json!({
        "hook_event_name": "preToolUse",
        "tool_name": "Write",
        "tool_use_id": "u-dup",
        "cwd": hab.repo,
        "tool_input": {"file_path": hooks, "content": "{}"},
    })
    .to_string();
    let head = r#"{"cursor_version":"2.4.0","conversation_id":"c-dup","hook_event_name":"beforeReadFile","#;
    let short = format!("{head}{}", &tail[1..]);
    let long = format!(
        "{head}\"content\":\"{}\",{}",
        "x".repeat(300 * 1024),
        &tail[1..]
    );
    for (what, payload) in [("short", short), ("after 300 KB", long)] {
        for queue in [false, true] {
            let label = format!("{what} queue={queue}");
            let (_, out, _) = call(
                hook(&hab, queue),
                payload.as_bytes(),
                Duration::from_secs(10),
            );
            assert_eq!(permission(&out), "deny", "{label}");
        }
    }
}

/// Cursor registers beforeMCPExecution with failClosed, so exit 1 with no
/// output blocks the call. The queue path answers invalid UTF-8 from a
/// lossy copy; the direct path must answer too.
#[test]
fn a_non_utf8_mcp_call_still_answers_on_the_direct_path() {
    let hab = hab("utf8");
    let text = cursor(
        "beforeMCPExecution",
        serde_json::json!({"tool_name": "ljos_note", "mcp_server_name": "ljos", "tool_input": "@BAD@"}),
    );
    let at = text.find("@BAD@").unwrap();
    let mut payload = text.as_bytes()[..at].to_vec();
    payload.push(0xff);
    payload.extend_from_slice(&text.as_bytes()[at + 5..]);
    for queue in [false, true] {
        let label = format!("non-utf8 ljos_note queue={queue}");
        let (_, out, _) = call(hook(&hab, queue), &payload, Duration::from_secs(10));
        assert_eq!(
            out.status.code(),
            Some(0),
            "{label}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), ALLOW, "{label}");
    }
}
