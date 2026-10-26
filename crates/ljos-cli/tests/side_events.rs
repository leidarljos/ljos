//! Side events answer while the pack hangs. Runs the built `ljos` binary.
#![cfg(target_os = "linux")]

use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "common/seat_root.rs"]
mod seat_root;

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
        "m3-{name}-{}-{}",
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

fn hook(hab: &Hab, pack: Option<u16>) -> Command {
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
        .env("LJOS_HOOKD", "off")
        .current_dir(&hab.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match pack {
        Some(port) => {
            cmd.env("PACKSET_URL", format!("http://127.0.0.1:{port}"))
                .env("PACKSET_TOKEN", "t".repeat(32));
        }
        None => {
            cmd.env("PACKSET_URL", "off");
        }
    }
    cmd
}

fn call(mut cmd: Command, payload: &str, limit: Duration) -> (Duration, Output) {
    let started = Instant::now();
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(payload.as_bytes());
    drop(stdin);
    while child.try_wait().unwrap().is_none() && started.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = started.elapsed();
    let _ = child.kill();
    (took, child.wait_with_output().unwrap())
}

/// A pack that accepts and never answers.
fn hung_pack() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming().flatten() {
            held.push(conn);
        }
    });
    port
}

/// Events that decide nothing answer inside 3 s while the pack hangs,
/// under the shortest timeout onboard registers for them (5 s).
#[test]
fn side_events_do_not_wait_out_a_hung_pack() {
    let port = hung_pack();
    let events = [
        ("sessionStart", serde_json::json!({"session_id": "c-side"})),
        ("beforeSubmitPrompt", serde_json::json!({"prompt": "hi"})),
        (
            "postToolUseFailure",
            serde_json::json!({"tool_name": "Shell", "tool_input": {"command": "false"},
                "error_message": "e", "failure_type": "error", "tool_use_id": "t-side"}),
        ),
        (
            "stop",
            serde_json::json!({"status": "completed", "loop_count": 0}),
        ),
        ("preCompact", serde_json::json!({"trigger": "auto"})),
        ("sessionEnd", serde_json::json!({"reason": "x"})),
    ];
    let mut slow = Vec::new();
    for (event, extra) in events {
        let hab = hab("side");
        let mut v = serde_json::json!({
            "cursor_version": "2.4.0",
            "conversation_id": "c-side",
            "hook_event_name": event,
        });
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        let (took, out) = call(
            hook(&hab, Some(port)),
            &v.to_string(),
            Duration::from_secs(20),
        );
        if took > Duration::from_secs(3) || out.status.code() != Some(0) {
            slow.push(format!("{event}: {took:?} rc={:?}", out.status.code()));
        }
    }
    assert!(
        slow.is_empty(),
        "side events waited on a hung pack: {slow:?}"
    );
}
