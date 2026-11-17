//! `ljos sitting --plan` before `ljos onboard`.
//!
//! The plan used to create the tracker where the command ran, a
//! `Software/<project>/issues.org` under the home directory, and then stop
//! on a deed store that was not there. It now refuses first, names
//! `ljos onboard`, and writes no ticket.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Seat {
    root: tempfile::TempDir,
}

impl Seat {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["home", "run"] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn plan(&self, cwd: &Path, extra: &[(&str, &Path)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args([
            "sitting",
            "--plan",
            "Ship the codec",
            "--",
            "measure",
            "write",
        ])
        .current_dir(cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", self.home())
        .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .stdin(Stdio::null());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Every file or directory under `dir` that is a tracker board, a
/// `Software` prefix directory, or a `vissue` data directory.
fn tracker_traces(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "issues.org" || name == "Software" || name == "vissue" {
            found.push(path.clone());
        }
        if path.is_dir() {
            tracker_traces(&path, found);
        }
    }
}

#[test]
fn a_plan_before_onboard_names_it_and_writes_no_tracker() {
    let seat = Seat::new();
    let out = seat.plan(&seat.home(), &[]);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("`ljos onboard`"), "{text}");
    assert!(
        !text.contains("deed store does not answer"),
        "the plan reached the claim step:\n{text}"
    );
    assert!(
        out.stdout.is_empty(),
        "a refused plan printed ids:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let mut traces = Vec::new();
    tracker_traces(seat.root.path(), &mut traces);
    assert!(
        traces.is_empty(),
        "the refused plan wrote a tracker: {traces:?}\n{text}"
    );
}

/// A root the person named that is not a directory is refused by what that
/// root is, and nothing is made there.
#[test]
fn a_plan_in_a_named_root_that_is_missing_makes_no_tree() {
    let seat = Seat::new();
    let missing = seat.root.path().join("nowhere");
    let out = seat.plan(&seat.home(), &[("VISSUE_ROOT", missing.as_path())]);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("is not a tracker"), "{text}");
    assert!(!missing.exists(), "the root was created:\n{text}");
}
