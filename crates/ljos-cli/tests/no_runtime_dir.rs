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

use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

const WARNING: &str = "ljos: keeping no runtime records, ";
/// How long one command here may take. Each exits in a few seconds at
/// most, and the hook call's own bound is 4 seconds.
const DEADLINE: Duration = Duration::from_secs(20);
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

    /// Run `cmd` with `input` on stdin, and fail when it has not exited and
    /// closed its pipes within [`DEADLINE`]. A command that found a place
    /// for its records, such as a hook server waiting on its socket, would
    /// otherwise hold the test open.
    fn run(&self, mut cmd: Command, input: &str) -> (Output, Duration) {
        use std::os::unix::process::CommandExt as _;
        let started = Instant::now();
        let mut child = cmd.process_group(0).spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        match finish_by(child, started + DEADLINE) {
            Ok(out) => (out, started.elapsed()),
            Err(said) => {
                let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
                let left: Vec<_> = std::fs::read_dir(&self.open)
                    .unwrap()
                    .map(|entry| entry.unwrap().file_name())
                    .collect();
                panic!(
                    "`{}` was still running {DEADLINE:?} after it started, and was killed; {} holds {left:?}:\n{said}",
                    args.join(" "),
                    self.open.display(),
                );
            }
        }
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

/// The output of `child` once it has exited and closed its pipes, or, when
/// it has not by `by`, what it said before it was killed with its process
/// group.
fn finish_by(mut child: Child, by: Instant) -> Result<Output, String> {
    let (tx, said) = std::sync::mpsc::channel();
    let pipes: [(bool, Box<dyn Read + Send>); 2] = [
        (false, Box::new(child.stdout.take().unwrap())),
        (true, Box::new(child.stderr.take().unwrap())),
    ];
    for (err, mut pipe) in pipes {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n @ 1..) = pipe.read(&mut chunk) {
                if tx.send((err, chunk[..n].to_vec())).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut status = None;
    let mut open = true;
    while (status.is_none() || open) && Instant::now() < by {
        status = child.try_wait().unwrap();
        if !open {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        match said.recv_timeout(Duration::from_millis(20)) {
            Ok((false, bytes)) => stdout.extend(bytes),
            Ok((true, bytes)) => stderr.extend(bytes),
            Err(RecvTimeoutError::Disconnected) => open = false,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    if let (Some(status), false) = (status, open) {
        return Ok(Output {
            status,
            stdout,
            stderr,
        });
    }
    let group = libc::pid_t::try_from(child.id()).unwrap();
    // SAFETY: a negative pid names the process group the child leads. The
    // child is unreaped or a member still holds a pipe, so that group is
    // still the one this test started.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
    let _ = child.wait();
    while let Ok((err, bytes)) = said.recv_timeout(Duration::from_millis(200)) {
        if err { &mut stderr } else { &mut stdout }.extend(bytes);
    }
    Err(format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    ))
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

/// A private `TMPDIR` the runtime directory cannot be made in passes the
/// check that does not make it. The hook server still says in one line
/// that it has no place for its socket.
#[test]
fn the_hook_server_says_so_when_its_runtime_directory_cannot_be_made() {
    let seat = Seat::new();
    let closed = seat.root.path().join("closed");
    std::fs::create_dir(&closed).unwrap();
    ljos_cli::change_mode(&closed, 0o500).unwrap();
    let mut cmd = seat.ljos(false, &["hookd"]);
    cmd.env("TMPDIR", &closed);
    let (out, _) = seat.run(cmd, "");
    let err = seat.stderr_of(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert_eq!(
        err.trim_end(),
        format!(
            "ljos hookd: no runtime directory for the hook server's socket, no private directory could be made in {}; set XDG_RUNTIME_DIR or HOME to an absolute path",
            closed.canonicalize().unwrap().display()
        )
    );
    assert_eq!(entries(&closed), 0);
}

#[test]
fn doctor_fix_says_why_it_left_the_session_seat() {
    let seat = Seat::new();
    let mut cmd = seat.ljos(false, &["doctor", "--fix"]);
    cmd.env("CURSOR_CONVERSATION_ID", "nr-doctor-0199a1b2");
    let (out, _) = seat.run(cmd, "");
    let err = seat.stderr_of(&out);
    assert!(
        err.contains(&format!(
            "ljos: left session seat: no runtime directory for the session seat, {} {REASON}",
            seat.open.display()
        )),
        "{err}"
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
