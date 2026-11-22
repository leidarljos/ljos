//! A tracker write that lands stays quiet.
//!
//! Commit mode and a push that landed print no tracker or sync line.
//! A refused commit is one sentence. `--verbose` prints the detail.
//! `--json` keeps `tracker_git` and `sync`.

ljos_test_boot::harness!();

#[path = "common/git.rs"]
mod isolated_git;

use std::path::PathBuf;
use std::process::{Command, Output};

struct Board {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    path: String,
    gitconfig: PathBuf,
}

impl Board {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(root.join("Software/probe")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("vissue"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(bin.join("claimdag"), "#!/bin/sh\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        for name in ["vissue", "claimdag"] {
            std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let gitconfig = dir.path().join("gitconfig");
        std::fs::write(&gitconfig, "[core]\n\thooksPath = /dev/null\n").unwrap();
        let board = Self {
            root,
            home,
            path: format!("{}:/usr/bin:/bin", bin.display()),
            gitconfig,
            _dir: dir,
        };
        board.git(&["init", "-q"]);
        board.git(&["config", "core.hooksPath", "/dev/null"]);
        let issues = board.root.join("Software/probe/issues.org");
        std::fs::write(
            &issues,
            "* TODO [#C] Probe\n:PROPERTIES:\n:ID:         probe-a1b2\n:END:\n",
        )
        .unwrap();
        board.git(&["add", "."]);
        board.git_as(&["commit", "-q", "-m", "seed"]);
        board
    }

    fn git(&self, args: &[&str]) {
        self.git_cmd(args, false);
    }

    fn git_as(&self, args: &[&str]) {
        self.git_cmd(args, true);
    }

    fn git_cmd(&self, args: &[&str], author: bool) {
        let mut cmd = isolated_git::command();
        cmd.arg("-C").arg(&self.root).args(args);
        cmd.env("GIT_CONFIG_GLOBAL", &self.gitconfig);
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        if author {
            cmd.env("GIT_AUTHOR_NAME", "Ada Example");
            cmd.env("GIT_AUTHOR_EMAIL", "ada@example.invalid");
            cmd.env("GIT_COMMITTER_NAME", "Ada Example");
            cmd.env("GIT_COMMITTER_EMAIL", "ada@example.invalid");
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn author(&self) {
        self.git(&["config", "user.name", "Ada Example"]);
        self.git(&["config", "user.email", "ada@example.invalid"]);
    }

    fn dirty(&self) {
        let issues = self.root.join("Software/probe/issues.org");
        let text = std::fs::read_to_string(&issues).unwrap();
        std::fs::write(&issues, text.replace("TODO", "STARTED")).unwrap();
    }

    fn ljos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("PACKSET_URL", "off")
            .env("LJOS_SYNC_HOST", "runner")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }
}

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn joined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn assert_quiet(out: &Output) {
    let said = joined(out);
    assert!(out.status.success(), "{}", text(out));
    assert!(!said.contains("tracker git:"), "{said}");
    assert!(!said.contains("sync:"), "{said}");
    assert!(!said.contains("not pushed"), "{said}");
    assert!(!said.contains(".ljos/atoms"), "{said}");
    assert!(!said.contains("seg-"), "{said}");
}

#[test]
fn a_commit_mode_note_prints_no_tracker_or_sync_line() {
    let board = Board::new();
    board.author();
    board.dirty();
    let out = board.ljos(&["note", "probe-a1b2", "the parser kept the hyphen"]);
    assert_quiet(&out);
    let log = isolated_git::command()
        .arg("-C")
        .arg(&board.root)
        .args(["log", "-1", "--format=%s"])
        .env("GIT_CONFIG_GLOBAL", &board.gitconfig)
        .output()
        .unwrap();
    let subject = String::from_utf8_lossy(&log.stdout);
    assert!(
        subject.contains("probe-a1b2 noted"),
        "the note was not committed:\n{subject}"
    );
}

#[test]
fn a_refused_commit_is_one_sentence() {
    let board = Board::new();
    board.dirty();
    let out = board.ljos(&["note", "probe-a1b2", "the parser kept the hyphen"]);
    // The seed commit named an author on the command. The checkout has
    // none, so the tracker commit is refused and the sentence names the setting.
    let said = joined(&out);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!said.contains("tracker git:"), "{said}");
    assert!(said.contains("The tracker commit was refused"), "{said}");
    assert!(said.contains("user.name"), "{said}");
    assert!(said.contains("run the command again"), "{said}");
}

#[test]
fn verbose_shows_the_tracker_line_and_json_keeps_the_fields() {
    let board = Board::new();
    board.author();
    board.dirty();
    let out = board.ljos(&[
        "note",
        "probe-a1b2",
        "the parser kept the hyphen",
        "--verbose",
    ]);
    let said = joined(&out);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        said.contains("tracker git: committed")
            && said.contains("not pushed (LJOS_TRACKER_GIT=commit)"),
        "{said}"
    );

    let board = Board::new();
    board.author();
    board.dirty();
    let out = board.ljos(&["note", "probe-a1b2", "the parser kept the hyphen", "--json"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let value: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|e| {
            panic!("--json was not JSON: {e}\n{said}");
        });
    let text_field = value["text"].as_str().unwrap_or("");
    assert!(!text_field.contains("tracker git:"), "{said}");
    assert!(!text_field.contains("sync:"), "{said}");
    assert!(
        value["tracker_git"][0]
            .as_str()
            .unwrap_or("")
            .contains("not pushed (LJOS_TRACKER_GIT=commit)"),
        "{said}"
    );
    assert!(value["sync"].as_array().is_some(), "{said}");
}
