//! `ljos statusline` at a terminal, and on a closed stdin.
//!
//! A runner pipes its status JSON. At a terminal the read used to wait
//! forever. A closed stdin is not a terminal, so the line is printed.

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;
#[path = "common/seat_env.rs"]
mod seat_env;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn wait_for(child: &mut std::process::Child, limit: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("ljos statusline did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn statusline_on_a_tty_prints_a_hint_and_exits() {
    let root = tempfile::tempdir().unwrap();
    let term = pty::Pty::open();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("statusline")
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .env("PACKSET_URL", "off");
    seat_env::clear(&mut cmd);
    term.attach_stdin(&mut cmd);
    term.attach_output(&mut cmd);
    let mut child = term.spawn(&mut cmd);
    let status = wait_for(&mut child, Duration::from_secs(5));
    term.finish_reading();
    let shown = term.shown();
    assert_eq!(status.code(), Some(2), "{shown}");
    assert!(
        shown.contains(
            "ljos: statusline reads the runner's status JSON on stdin, so pipe that JSON to it."
        ),
        "{shown}"
    );
}

#[test]
fn statusline_on_a_closed_stdin_prints_the_line() {
    let root = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("statusline")
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .env("PACKSET_URL", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    seat_env::clear(&mut cmd);
    let out = cmd.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{text}\n{err}");
    assert!(!err.contains("pipe that JSON"), "{err}");
    assert!(text.starts_with("ljos none · "), "{text}");
}
