//! A user with no directory safe for runtime records: `TMPDIR` is one every
//! user can write that has no sticky bit, and neither `HOME` nor
//! `XDG_RUNTIME_DIR` is set. A command that can go on without the records
//! warns once and goes on. One that cannot says why in one line. None of
//! them leaves anything in that `TMPDIR`, panics, or prints the error the
//! path under `/dev/null` gives.
#![cfg(target_os = "linux")]

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const WARNING: &str = "ljos: keeping no runtime records, ";
const REASON: &str =
    "is not safe from other users; set XDG_RUNTIME_DIR or HOME to an absolute path";

const VISSUE: &str = r#"#!/bin/sh
case "$1 $3" in
  show*) echo '{}' ;;
  'vote --for') echo "[{\"agent\":\"$VISSUE_AGENT\",\"choice\":\"$4\"}]" > "$SHIM_DRAWER" ;;
  'vote --json') cat "$SHIM_DRAWER" 2>/dev/null || echo '[]' ;;
esac
exit 0
"#;

struct Seat {
    root: tempfile::TempDir,
    open: PathBuf,
    bin: PathBuf,
}

impl Seat {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let open = root.path().join("open");
        let bin = root.path().join("bin");
        for dir in [&open, &bin, &root.path().join("repo")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        ljos_cli::change_mode(&open, 0o777).unwrap();
        ljos_cli::install_executable(&bin.join("vissue"), VISSUE).unwrap();
        Self { root, open, bin }
    }

    /// `ljos ARGS` as this user, under `sh` when `shell`, so the call has a
    /// shell above it as a person's would.
    fn ljos(&self, shell: bool, args: &[&str]) -> Command {
        let ljos = env!("CARGO_BIN_EXE_ljos");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = if shell {
            let mut cmd = Command::new("/bin/sh");
            cmd.args(["-c", "\"$0\" \"$@\"; exit $?", ljos]);
            cmd
        } else {
            Command::new(ljos)
        };
        cmd.args(args)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("TMPDIR", &self.open)
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env("SHIM_DRAWER", self.root.path().join("drawer.json"))
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .current_dir(self.root.path().join("repo"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        seat_env::clear(&mut cmd);
        cmd
    }

    fn run(&self, mut cmd: Command, input: &str) -> (Output, Duration) {
        let started = Instant::now();
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (out, started.elapsed())
    }

    /// The checks every command here meets, and its stderr.
    fn stderr_of(&self, out: &Output) -> String {
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        let said = format!("{}{err}", String::from_utf8_lossy(&out.stdout));
        for raw in ["panicked", "Not a directory", "os error", "/dev/null"] {
            assert!(!said.contains(raw), "{raw}:\n{said}");
        }
        assert_eq!(entries(&self.open), 0, "{said}");
        assert!(
            err.matches(WARNING).count() <= 1,
            "the warning is said once:\n{err}"
        );
        err
    }
}

fn entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

#[test]
fn the_hook_server_says_in_one_line_that_it_has_no_place_for_its_socket() {
    let seat = Seat::new();
    let (out, _) = seat.run(seat.ljos(false, &["hookd"]), "");
    let err = seat.stderr_of(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert_eq!(
        err.trim_end(),
        format!(
            "ljos hookd: no runtime directory for the hook server's socket, {} {REASON}",
            seat.open.display()
        )
    );
}

#[test]
fn a_hook_call_is_judged_at_once_with_no_queue_to_wait_in() {
    let seat = Seat::new();
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "nr-1",
        "tool_name": "Bash",
        "tool_input": {"command": "ls"},
        "cwd": seat.root.path().join("repo"),
    })
    .to_string();
    let mut cmd = seat.ljos(false, &["hook"]);
    cmd.env("LJOS_HOOK_ADMIT_MS", "8000");
    let (out, took) = seat.run(cmd, &payload);
    let err = seat.stderr_of(&out);
    assert!(out.status.success(), "{err}");
    assert!(!err.contains("hook queue"), "{err}");
    assert!(took < Duration::from_secs(4), "{took:?}\n{err}");
}

#[test]
fn the_book_drain_has_nothing_to_drain() {
    let seat = Seat::new();
    let (out, _) = seat.run(seat.ljos(false, &["hook", "--drain"]), "");
    let err = seat.stderr_of(&out);
    assert!(out.status.success(), "{err}");
}

#[test]
fn a_vote_is_recorded_as_its_own_voice() {
    let seat = Seat::new();
    let (out, _) = seat.run(
        seat.ljos(
            true,
            &[
                "vote",
                "proj-aaaa",
                "--for",
                "a",
                "--used",
                "none",
                "--as",
                "reviewer",
            ],
        ),
        "",
    );
    let err = seat.stderr_of(&out);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}{err}");
    assert!(
        said.contains("recorded on the tracker: reviewer for a"),
        "{said}{err}"
    );
}

#[test]
fn a_decision_prompt_says_why_its_panel_did_not_open() {
    let seat = Seat::new();
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "nr-2",
        "prompt": "what do we think about tabs",
        "cwd": seat.root.path().join("repo"),
    })
    .to_string();
    let mut cmd = seat.ljos(false, &["hook"]);
    cmd.env("LJOS_SEAT", "probe");
    let (out, _) = seat.run(cmd, &payload);
    let err = seat.stderr_of(&out);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}{err}");
    assert!(
        said.contains(&format!(
            "panel did not open: no runtime directory for the panel's prompt, {} {REASON}",
            seat.open.display()
        )),
        "{said}"
    );
    assert!(said.contains("Do not pick an answer"), "{said}");
}

#[test]
fn the_panel_opener_says_in_one_line_that_it_has_no_place_for_the_briefs() {
    let seat = Seat::new();
    let prompt = seat.root.path().join("prompt.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&prompt, "what do we think about tabs").unwrap();
    let log = seat.root.path().join("opener.log");
    let (out, _) = seat.run(
        seat.ljos(
            false,
            &[
                "open-panel",
                "--prompt-file",
                &prompt.display().to_string(),
                "--log",
                &log.display().to_string(),
            ],
        ),
        "",
    );
    let err = seat.stderr_of(&out);
    assert!(!out.status.success(), "{err}");
    assert_eq!(
        err.trim_end(),
        format!(
            "no runtime directory for the panel's briefs, {} {REASON}",
            seat.open.display()
        )
    );
}
