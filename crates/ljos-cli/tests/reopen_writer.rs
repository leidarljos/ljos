//! `ljos reopen` puts a finished or cancelled ticket back to work, and
//! only a person does it. A close another seat or a hand wrote is
//! reopened like any other, so the writer of the close is not a reason
//! to refuse; the absence of a person is.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "common/sibling.rs"]
mod sibling;

fn vissue_bin() -> PathBuf {
    sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"))
}

fn path_env() -> String {
    let bin = vissue_bin();
    let dir = bin.parent().unwrap().display().to_string();
    let rest = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    format!("{dir}:/usr/bin:/bin:{rest}")
}

struct Board {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    git_cfg: PathBuf,
    org: PathBuf,
}

impl Board {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        let git_cfg = dir.path().join("gitconfig");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&git_cfg, "").unwrap();
        let _bin = vissue_bin();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "seat@example.invalid"]);
        git(&root, &["config", "user.name", "seat"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["config", "core.hooksPath", "/dev/null"]);
        let board = Self {
            _dir: dir,
            root,
            home,
            git_cfg,
            org: PathBuf::new(),
        };
        let id = board.vissue(&["q", "-p", "probe", "writer check"]);
        board.git_add_commit("seed");
        let org = board.root.join("Software/probe/issues.org");
        assert!(org.is_file(), "vissue did not write a board");
        let _ = id;
        let mut board = board;
        board.org = org;
        board
    }

    fn vissue(&self, args: &[&str]) -> String {
        let out = self.cmd("vissue", args);
        assert!(
            out.status.success(),
            "vissue {args:?}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn cmd(&self, bin: &str, args: &[&str]) -> Output {
        let program = if bin == "vissue" {
            vissue_bin()
        } else {
            PathBuf::from(bin)
        };
        Command::new(program)
            .args(args)
            .env_clear()
            .env("PATH", path_env())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("VISSUE_AGENT", "saddle@test")
            .env("GIT_CONFIG_GLOBAL", &self.git_cfg)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }

    fn git_add_commit(&self, subject: &str) {
        git(&self.root, &["add", "-A"]);
        let out = git(&self.root, &["commit", "-q", "-m", subject]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn issue(&self, title: &str) -> String {
        let id = self.vissue(&["q", "-p", "probe", title]);
        assert!(!id.is_empty(), "vissue q printed no id");
        id
    }

    fn show(&self, id: &str) -> serde_json::Value {
        let out = self.cmd("vissue", &["show", id, "--json"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn reopen(&self, id: &str, seat: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["reopen", id])
            .env_clear()
            .env("PATH", path_env())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("VISSUE_AGENT", format!("{seat}@test"))
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "commit")
            .env("PACKSET_URL", "off")
            .env("GIT_CONFIG_GLOBAL", &self.git_cfg)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }

    fn stamp_close(&self, id: &str, seat: &str) {
        let ok = ljos_cli::stamp_seat_logbook(&self.org, id, "settled with notes", seat);
        assert!(ok, "stamp missed {id}");
        self.git_add_commit(&format!("chore(issues): {id} settled with notes by {seat}"));
    }
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "seat")
        .env("GIT_AUTHOR_EMAIL", "seat@example.invalid")
        .env("GIT_COMMITTER_NAME", "seat")
        .env("GIT_COMMITTER_EMAIL", "seat@example.invalid")
        .output()
        .unwrap()
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_reopen_without_a_person_is_refused() -> Result<(), String> {
    sibling::require("vissue")?;
    let b = Board::new();
    let id = b.issue("restore prior");
    b.vissue(&["note", &id, "keep this sentence"]);
    let _child = b.vissue(&["q", "-p", "probe", "--parent", &id, "a child"]);
    b.vissue(&["update", &id, "-s", "STARTED"]);
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.stamp_close(&id, "saddle");
    let out = b.reopen(&id, "saddle");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("label reopen-warning"), "{text}");
    assert!(text.contains("a person reopens a ticket"), "{text}");
    assert_eq!(b.show(&id)["state"], "DONE");
    Ok(())
}

/// A close a person made by hand, with no tracker commit of this
/// program's, is a ticket like any other. The refusal names the missing
/// person, not the writer of the close.
#[test]
fn a_hand_close_is_not_refused_for_its_writer() -> Result<(), String> {
    sibling::require("vissue")?;
    let b = Board::new();
    let id = b.issue("hand close");
    b.vissue(&["update", &id, "-s", "STARTED"]);
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.git_add_commit(&format!("chore(issues): {id} finished by rohit"));
    let out = b.reopen(&id, "mallory");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("label reopen-warning"), "{text}");
    assert!(text.contains("a person reopens a ticket"), "{text}");
    assert!(!text.contains("no close this program wrote"), "{text}");
    assert!(!text.contains("was closed by"), "{text}");
    assert_eq!(b.show(&id)["state"], "DONE");
    Ok(())
}

/// A ticket that is neither finished nor cancelled has nothing to undo.
#[test]
fn an_open_ticket_is_not_reopened() -> Result<(), String> {
    sibling::require("vissue")?;
    let b = Board::new();
    let id = b.issue("still open");
    b.vissue(&["update", &id, "-s", "STARTED"]);
    let out = b.reopen(&id, "saddle");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("label reopen-warning"), "{text}");
    assert_eq!(b.show(&id)["state"], "STARTED");
    Ok(())
}
