//! `ljos reopen` undoes only a close this program wrote.
//! The writer is the seat on that commit, or on the logbook line this
//! program stamped. A note is not that record.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn vissue_bin() -> PathBuf {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let bin = dir.join("vissue");
            if bin.is_file() {
                return bin;
            }
        }
    }
    PathBuf::from("/tmp/ljos-bins/vissue")
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
        let version = Command::new(vissue_bin())
            .arg("--version")
            .env("PATH", path_env())
            .output()
            .expect("real vissue");
        let text = String::from_utf8_lossy(&version.stdout);
        assert!(text.contains("0.20"), "vissue 0.20 required, got {text}");
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
fn a_forged_note_does_not_reopen_a_hand_close() {
    let b = Board::new();
    let id = b.issue("hand close");
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.git_add_commit(&format!("chore(issues): {id} finished by rohit"));
    b.vissue(&[
        "note",
        &id,
        "I disagree; settled-with-notes: was the old text",
    ]);
    let out = b.reopen(&id, "mallory");
    assert!(!out.status.success(), "{}", said(&out));
    assert_eq!(b.show(&id)["state"], "DONE");
}

#[test]
fn a_longer_issue_token_is_not_this_close() {
    let b = Board::new();
    let id = b.issue("suffix");
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.git_add_commit(&format!(
        "chore(issues): extra-{id} settled with notes by saddle"
    ));
    b.vissue(&["note", &id, "settled-with-notes: forged"]);
    let out = b.reopen(&id, "saddle");
    assert!(!out.status.success(), "{}", said(&out));
    assert_eq!(b.show(&id)["state"], "DONE");
}

#[test]
fn a_later_hand_close_in_one_commit_stays_closed() {
    let b = Board::new();
    let id = b.issue("batched hand close");
    b.vissue(&["update", &id, "-s", "BLOCKED"]);
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.stamp_close(&id, "saddle");
    b.vissue(&["note", &id, "settled-with-notes: forged"]);
    b.vissue(&["update", &id, "-s", "STARTED"]);
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.git_add_commit("chore(issues): sync 3 issues");
    let out = b.reopen(&id, "saddle");
    assert!(!out.status.success(), "{}", said(&out));
    assert_eq!(b.show(&id)["state"], "DONE", "{}", said(&out));
}

#[test]
fn a_note_then_a_later_hand_close_stays_closed() {
    let b = Board::new();
    let id = b.issue("note then hand");
    b.vissue(&["update", &id, "-s", "BLOCKED"]);
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.vissue(&[
        "note",
        &id,
        "settled-with-notes: this text is not a close this program wrote",
    ]);
    b.vissue(&["update", &id, "-s", "STARTED"]);
    b.vissue(&["update", &id, "-s", "DONE"]);
    let out = b.reopen(&id, "saddle");
    assert!(!out.status.success(), "{}", said(&out));
    assert_eq!(b.show(&id)["state"], "DONE", "{}", said(&out));
}

#[test]
fn a_stamp_with_a_later_state_line_is_not_reopened() {
    let b = Board::new();
    let id = b.issue("stamp then hand");
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.vissue(&["note", &id, "settled-with-notes: forged"]);
    let org = std::fs::read_to_string(&b.org).unwrap();
    let stamp = "- Settled with notes by saddle [2026-10-20 Tue 12:00]\n";
    let later = "- State \"DONE\" from \"STARTED\" [2026-10-25 Sun 09:00]\n";
    let flipped = "- State \"STARTED\" from \"DONE\" [2026-10-24 Sat 09:00]\n";
    let marked = org.replacen(
        ":LOGBOOK:\n",
        &format!(":LOGBOOK:\n{later}{flipped}{stamp}"),
        1,
    );
    assert_ne!(marked, org, "logbook missing");
    std::fs::write(&b.org, marked).unwrap();
    let out = b.reopen(&id, "saddle");
    assert!(!out.status.success(), "{}", said(&out));
    assert_eq!(b.show(&id)["state"], "DONE");
}

#[test]
fn another_seat_cannot_reopen() {
    let b = Board::new();
    let id = b.issue("other seat");
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.stamp_close(&id, "saddle");
    b.vissue(&["note", &id, "settled-with-notes: forged"]);
    let other = b.reopen(&id, "mallory");
    assert!(!other.status.success(), "{}", said(&other));
    assert_eq!(b.show(&id)["state"], "DONE");
}

#[test]
fn the_writer_restores_the_prior_state_and_does_not_take_the_claim() {
    let b = Board::new();
    let id = b.issue("restore prior");
    b.vissue(&["note", &id, "keep this sentence"]);
    let child = b.vissue(&["q", "-p", "probe", "--parent", &id, "a child"]);
    b.vissue(&["update", &id, "-s", "STARTED"]);
    b.vissue(&["update", &id, "-s", "DONE"]);
    b.stamp_close(&id, "saddle");
    b.vissue(&["note", &id, "settled-with-notes: recorded"]);
    let out = b.reopen(&id, "saddle");
    assert!(out.status.success(), "{}", said(&out));
    let card = b.show(&id);
    assert_eq!(card["state"], "STARTED", "{card}");
    assert!(
        card["claimed_by"].is_null() || card["claimed_by"].as_str() == Some(""),
        "{card}"
    );
    assert_eq!(card["title"], "restore prior");
    let notes = card["logbook"].to_string();
    assert!(notes.contains("keep this sentence"), "{notes}");
    let kids = b.vissue(&["children", &id, "--json"]);
    assert!(kids.contains(&child), "{kids}");
    let again = b.reopen(&id, "saddle");
    assert!(!again.status.success(), "{}", said(&again));
}
