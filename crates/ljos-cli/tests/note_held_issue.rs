//! `ljos note` with no id notes on the issue this seat holds.
//!
//! An id `vissue create` mints still names the issue. A shorter hyphenated
//! word does not. With nothing held and no id, the note is stored with
//! `unattached-note` and the tracker is not asked. `ljos_note` with a blank
//! issue does the same.

ljos_test_boot::harness!();
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Hab {
    dir: tempfile::TempDir,
}

impl Hab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        script(
            dir.path(),
            "vissue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\nexit 0\n",
                log.display()
            ),
        );
        script(dir.path(), "claimdag", "#!/bin/sh\nexit 1\n");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), dir.path().join("ljos")).unwrap();
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self { dir }
    }

    /// A hold record this process's child can read: the seat line is
    /// `saddle`, and the pid is this test, which is the parent of `ljos`
    /// and of `ljos-mcp`.
    fn hold(&self, issue: &str) {
        let run = self.dir.path().join("run/ljos");
        std::fs::create_dir_all(&run).unwrap();
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
        std::fs::write(
            run.join("hold-sitting"),
            format!(
                "saddle\nsaddle\n{}\n{}\n2026-01-01T00:00:00Z\n{issue}\n-\n",
                std::process::id(),
                comm.trim()
            ),
        )
        .unwrap();
    }

    fn command(&self, bin: &str) -> Command {
        let path = format!("{}:/usr/bin:/bin", self.dir.path().display());
        let mut cmd = Command::new(bin);
        cmd.env_clear()
            .env("PATH", &path)
            .env("HOME", self.dir.path().join("home"))
            .env(
                "XDG_CONFIG_HOME",
                self.dir.path().join("home").join(".config"),
            )
            .env(
                "XDG_STATE_HOME",
                self.dir.path().join("home").join(".state"),
            )
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .env("PACKSET_URL", "off")
            .current_dir(self.dir.path());
        cmd
    }

    fn note(&self, args: &[&str]) -> std::process::Output {
        let mut cmd = self.command(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args);
        cmd.output().unwrap()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("calls.log")).unwrap_or_default()
    }
}

/// A hold on `proj-aaaa` and a note with no id. The tracker is asked for
/// that issue, and stdout names it.
#[test]
fn a_note_with_no_id_goes_on_the_held_issue() {
    let hab = Hab::new();
    hab.hold("proj-aaaa");
    let out = hab.note(&["note", "the", "parser", "kept", "the", "hyphen"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "ljos note failed: {stdout}{stderr}");
    assert!(
        hab.calls()
            .contains("vissue note proj-aaaa the parser kept the hyphen"),
        "the note did not reach the held issue:\n{}",
        hab.calls()
    );
    assert!(
        stdout.contains("proj-aaaa"),
        "stdout did not name the issue that took the note:\n{stdout}"
    );
}

/// The same hold, and a first word that is an id. The note goes on that
/// id, not on the hold.
#[test]
fn an_id_shaped_first_word_selects_that_issue() {
    let hab = Hab::new();
    hab.hold("proj-aaaa");
    let out = hab.note(&["note", "proj-bbbb", "some", "text"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "ljos note failed: {stdout}{stderr}");
    assert!(
        hab.calls().contains("vissue note proj-bbbb some text"),
        "the named id was not the one noted:\n{}",
        hab.calls()
    );
    assert!(
        !hab.calls().contains("vissue note proj-aaaa"),
        "the hold took a note that named another id:\n{}",
        hab.calls()
    );
}

/// `ljos-hud` is a hyphenated word, three letters after the dash. `vissue
/// create` mints four. The whole line is the note on the hold.
#[test]
fn a_hyphenated_word_shorter_than_a_minted_id_stays_text() {
    let hab = Hab::new();
    hab.hold("proj-aaaa");
    let out = hab.note(&["note", "ljos-hud", "kept", "the", "hyphen"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "ljos note failed: {stdout}{stderr}");
    assert!(
        hab.calls()
            .contains("vissue note proj-aaaa ljos-hud kept the hyphen"),
        "ljos-hud was taken for an id:\n{}",
        hab.calls()
    );
    assert!(
        stdout.contains("proj-aaaa"),
        "stdout did not name the held issue:\n{stdout}"
    );
}

/// No hold and no id. The note is stored with `unattached-note` and the
/// tracker is not asked, the same as `ljos_note` with no issue.
#[test]
fn a_note_with_no_hold_and_no_id_is_stored_unattached() {
    let hab = Hab::new();
    let started = Instant::now();
    let out = hab.note(&["note", "some", "text"]);
    let took = started.elapsed();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "ljos note failed: {stdout}{stderr}");
    assert!(
        took < Duration::from_secs(2),
        "a missing issue took {took:?}: {stderr}"
    );
    assert!(
        !hab.calls().contains("vissue note"),
        "a note was asked for with nothing held:\n{}",
        hab.calls()
    );
    assert!(
        stdout.contains("unattached-note"),
        "the note was not stored unattached:\n{stdout}"
    );
}

/// `ljos_note` with no issue, while a hold is on `proj-aaaa`. The tracker
/// is asked for that issue.
#[test]
fn a_blank_mcp_note_goes_on_the_held_issue() {
    let hab = Hab::new();
    hab.hold("proj-aaaa");
    let mut child = hab
        .command(env!("CARGO_BIN_EXE_ljos-mcp"))
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
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ljos_note","arguments":{"text":"x"}}}"#,
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "ljos-mcp failed: {stdout}{stderr}");
    assert!(
        hab.calls().contains("vissue note proj-aaaa x"),
        "ljos_note did not note on the hold:\ncalls:\n{}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        hab.calls()
    );
}

/// No issue and no hold. The note is stored with `unattached-note` and the
/// call succeeds. The tracker is not asked.
#[test]
fn a_note_with_no_issue_is_stored_unattached() {
    let hab = Hab::new();
    let mut child = hab
        .command(env!("CARGO_BIN_EXE_ljos-mcp"))
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
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ljos_note","arguments":{"text":"zircon without a ticket"}}}"#,
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "ljos-mcp failed: {stdout}{stderr}");
    assert!(
        stdout.contains("unattached-note"),
        "the note was not stored unattached:\n{stdout}\n{stderr}"
    );
    assert!(
        !stdout.contains("pass an issue id"),
        "the call still asked for an issue:\n{stdout}"
    );
    assert!(
        !stdout.contains("\"isError\":true"),
        "an unattached note was an error:\n{stdout}"
    );
    assert!(
        !hab.calls().contains("vissue note"),
        "the tracker was asked for a note with no issue:\n{}",
        hab.calls()
    );
    let mut labelled = false;
    for entry in walkdir_files(hab.dir.path()) {
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        if text.contains("unattached-note") && text.contains("zircon without a ticket") {
            labelled = true;
            break;
        }
    }
    assert!(labelled, "no kept row carried unattached-note");
}

fn walkdir_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}
