//! `ljos note` and `ljos finish` with no id use the open sitting.
//!
//! An empty id is refused before the tracker is asked. A named ticket
//! that is not the open sitting is still written, and one stderr line
//! names both. `--quiet` leaves that line off.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const OPEN: &str = "anneal-1mr1";
const OTHER: &str = "anneal-ngea";

const ORG: &str = "\
#+TITLE: anneal
#+VISSUE: 1
#+CATEGORY: anneal
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#C] Hold the melt
:PROPERTIES:
:ID:         anneal-1mr1
:CLAIMED_BY: saddle
:END:

* STARTED [#C] Note the wrong ticket
:PROPERTIES:
:ID:         anneal-ngea
:CLAIMED_BY: saddle
:END:
";

struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
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
        script(
            dir.path(),
            "claimdag",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"claimdag $*\" >> '{}'\ncase \"$1\" in\n  get) printf '%s\\n' 'gen=1 state=claimed' ;;\nesac\nexit 0\n",
                log.display()
            ),
        );
        for name in ["home", "run", "tmp", "tracker/Software/anneal"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        std::fs::write(
            dir.path().join("tracker/vissue.toml"),
            "prefix = \"Software\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("tracker/Software/anneal/issues.org"), ORG).unwrap();
        Self { dir }
    }

    /// A hold this conversation matches by seat name. The pid is not read,
    /// so the same file works on Linux and macOS.
    fn hold(&self, issue: &str) {
        let run = self.dir.path().join("run/ljos");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            run.join("hold-sitting"),
            format!("saddle\nsaddle\n1\nljos\n2026-01-01T00:00:00Z\n{issue}\n-\n"),
        )
        .unwrap();
    }

    fn org(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("tracker/Software/anneal/issues.org")).unwrap()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("calls.log")).unwrap_or_default()
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.dir.path().join("home");
        let path = format!("{}:/usr/bin:/bin", self.dir.path().display());
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("TMPDIR", self.dir.path().join("tmp"))
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("PACKSET_AUTH", "off")
            .current_dir(self.dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }
}

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}\n",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn stderr_line(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

#[test]
fn a_note_with_no_id_uses_the_open_sitting() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let out = seat.run(&["note", "the melt held"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        seat.calls()
            .contains(&format!("vissue note {OPEN} -- the melt held")),
        "the note missed the open sitting:\n{}\n{text}",
        seat.calls()
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("your open sitting"),
        "{text}"
    );
}

#[test]
fn a_finish_with_no_id_uses_the_open_sitting() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let before = seat.org();
    let out = seat.run(&["finish", "--lesson", "The melt held when it was cold."]);
    let text = said(&out);
    assert!(out.status.success(), "{text}calls:\n{}", seat.calls());
    assert!(
        text.contains(&format!("sitting ended; ticket {OPEN} is still open")),
        "{text}"
    );
    assert!(
        !seat.calls().contains(OTHER),
        "finish wrote the other ticket:\n{}\n{text}",
        seat.calls()
    );
    assert_ne!(seat.org(), before, "finish did not write the tracker");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("your open sitting"),
        "{text}"
    );
}

#[test]
fn a_note_with_no_sitting_names_sitting() {
    let seat = Seat::new();
    let before = seat.org();
    let out = seat.run(&["note", "the melt held"]);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(out.stdout.is_empty(), "{text}");
    assert_eq!(stderr_line(&out), ljos_cli::NO_OPEN_SITTING);
    assert!(
        !seat.calls().contains("vissue note"),
        "a note was written with no sitting:\n{}",
        seat.calls()
    );
    assert_eq!(seat.org(), before);
}

#[test]
fn a_finish_with_no_sitting_names_sitting() {
    let seat = Seat::new();
    let before = seat.org();
    let out = seat.run(&["finish", "--lesson", "The melt held when it was cold."]);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(out.stdout.is_empty(), "{text}");
    assert_eq!(stderr_line(&out), ljos_cli::NO_OPEN_SITTING);
    assert!(
        !seat.calls().contains("claimdag"),
        "finish asked the claim graph with no sitting:\n{}",
        seat.calls()
    );
    assert_eq!(seat.org(), before);
}

/// `ljos finish --close` after the sitting ended has no ticket to read.
/// The line does not send the person back to `ljos sitting`, which would
/// claim the ticket again; it names the form that closes by id.
#[test]
fn a_close_with_no_sitting_names_the_form_that_takes_an_id() {
    let seat = Seat::new();
    let before = seat.org();
    let out = seat.run(&["finish", "--close"]);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(out.stdout.is_empty(), "{text}");
    assert_eq!(
        stderr_line(&out),
        "no open sitting, so run `ljos finish ID --close` with the id of the ticket to close."
    );
    assert!(!seat.calls().contains("vissue update"), "{}", seat.calls());
    assert_eq!(seat.org(), before);
}

#[test]
fn an_empty_note_id_never_touches_the_tracker() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let before = seat.org();
    for args in [
        &["note", "", "the melt held"][..],
        &["note", "   ", "the melt held"][..],
        &["note", ""][..],
        &["note", " \t "][..],
    ] {
        let out = seat.run(args);
        let text = said(&out);
        assert_eq!(out.status.code(), Some(1), "{args:?}\n{text}");
        assert!(out.stdout.is_empty(), "{args:?}\n{text}");
        assert_eq!(stderr_line(&out), ljos_cli::EMPTY_TICKET_ID, "{args:?}");
    }
    assert_eq!(
        seat.calls(),
        "",
        "a blank id reached a sibling:\n{}",
        seat.calls()
    );
    assert_eq!(seat.org(), before);
}

#[test]
fn an_empty_finish_id_never_touches_the_tracker() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let before = seat.org();
    for args in [
        &["finish", ""][..],
        &["finish", "   "][..],
        &["finish", "", "--lesson", "The melt held when it was cold."][..],
        &["finish", " \t ", "--close"][..],
    ] {
        let out = seat.run(args);
        let text = said(&out);
        assert_eq!(out.status.code(), Some(1), "{args:?}\n{text}");
        assert!(out.stdout.is_empty(), "{args:?}\n{text}");
        assert_eq!(stderr_line(&out), ljos_cli::EMPTY_TICKET_ID, "{args:?}");
        assert!(!text.contains("finished"), "{args:?}\n{text}");
        assert!(!text.contains("--close"), "{args:?}\n{text}");
    }
    assert_eq!(
        seat.calls(),
        "",
        "a blank id reached a sibling:\n{}",
        seat.calls()
    );
    assert_eq!(seat.org(), before);
}

#[test]
fn a_note_on_another_ticket_warns_and_still_writes() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let out = seat.run(&["note", OTHER, "the melt held"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}calls:\n{}", seat.calls());
    assert!(
        seat.calls()
            .contains(&format!("vissue note {OTHER} -- the melt held")),
        "the named ticket was not written:\n{}",
        seat.calls()
    );
    assert!(
        !seat.calls().contains(&format!("vissue note {OPEN}")),
        "the open sitting took the note:\n{}",
        seat.calls()
    );
    assert_eq!(
        stderr_line(&out),
        ljos_cli::other_sitting_line(OTHER, OPEN),
        "{text}"
    );
}

#[test]
fn a_finish_on_another_ticket_warns_and_still_writes() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let out = seat.run(&[
        "finish",
        OTHER,
        "--lesson",
        "The melt held when it was cold.",
    ]);
    let text = said(&out);
    assert!(out.status.success(), "{text}calls:\n{}", seat.calls());
    assert!(
        text.contains(&format!("sitting ended; ticket {OTHER} is still open")),
        "{text}"
    );
    assert!(
        seat.org().contains(&format!("ID:         {OPEN}")),
        "the open sitting left the tracker:\n{}",
        seat.org()
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.lines()
            .any(|line| line == ljos_cli::other_finish_line(OTHER, OPEN)),
        "{text}"
    );
    assert!(
        !err.contains("note written"),
        "finish called itself a note:\n{text}"
    );
}

#[test]
fn quiet_hides_the_other_sitting_line() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let note = seat.run(&["note", "--quiet", OTHER, "the melt held"]);
    let note_text = said(&note);
    assert!(note.status.success(), "{note_text}calls:\n{}", seat.calls());
    assert!(
        seat.calls()
            .contains(&format!("vissue note {OTHER} -- the melt held")),
        "{}",
        seat.calls()
    );
    assert!(
        !String::from_utf8_lossy(&note.stderr).contains("your open sitting"),
        "{note_text}"
    );
    let done = seat.run(&[
        "finish",
        "--quiet",
        OTHER,
        "--lesson",
        "The melt held when it was cold.",
    ]);
    let done_text = said(&done);
    assert!(done.status.success(), "{done_text}calls:\n{}", seat.calls());
    assert!(
        done_text.contains(&format!("sitting ended; ticket {OTHER} is still open")),
        "{done_text}"
    );
    assert!(
        !String::from_utf8_lossy(&done.stderr).contains("your open sitting"),
        "{done_text}"
    );
}

#[test]
fn a_note_on_the_open_sitting_does_not_warn() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let out = seat.run(&["note", OPEN, "the melt held"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        seat.calls()
            .contains(&format!("vissue note {OPEN} -- the melt held")),
        "{}",
        seat.calls()
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("your open sitting"),
        "{text}"
    );
}

#[test]
fn a_spaced_note_is_not_an_empty_id() {
    let seat = Seat::new();
    seat.hold(OPEN);
    let out = seat.run(&["note", "  the melt held"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        seat.calls().contains("vissue note anneal-1mr1"),
        "a spaced note was taken for a blank id:\n{}",
        seat.calls()
    );
    assert!(!seat.calls().contains("vissue note  "), "{}", seat.calls());
}
