//! `ljos parent ISSUE PARENT` moves one ticket under a new parent.
//!
//! The command refuses unknown parents and cycle edges, and it records a
//! dated note on the moved ticket.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "common/sibling.rs"]
mod sibling;
#[path = "common/tools.rs"]
mod tools;

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
        Self {
            _dir: dir,
            root,
            home,
            git_cfg,
        }
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
            .env("TMPDIR", std::env::temp_dir())
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

    fn vissue(&self, args: &[&str]) -> String {
        let out = self.cmd("vissue", args);
        assert!(
            out.status.success(),
            "vissue {args:?}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn issue(&self, title: &str) -> String {
        let id = self.vissue(&["q", "-p", "probe", title]);
        assert!(!id.is_empty(), "vissue q printed no id");
        id
    }

    fn child(&self, parent: &str, title: &str) -> String {
        let id = self.vissue(&["q", "-p", "probe", "--parent", parent, title]);
        assert!(!id.is_empty(), "vissue q --parent printed no id");
        id
    }

    fn parent(&self, issue: &str) -> Option<String> {
        let out = self.cmd("vissue", &["show", issue, "--json"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["parent"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    fn note_texts(&self, issue: &str) -> Vec<String> {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["show", issue, "--json"])
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", path_env())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("VISSUE_AGENT", "saddle@test")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("GIT_CONFIG_GLOBAL", &self.git_cfg)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["notes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["text"].as_str().map(str::to_string))
            .collect()
    }

    fn ljos_parent(&self, issue: &str, parent: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["parent", issue, parent])
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", path_env())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("VISSUE_AGENT", "saddle@test")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("GIT_CONFIG_GLOBAL", &self.git_cfg)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .env("TMPDIR", std::env::temp_dir())
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
fn parent_moves_the_ticket_and_adds_a_note() -> Result<(), String> {
    if tools::halt(tools::Tool::Vissue) {
        return Ok(());
    }
    let b = Board::new();
    let old_parent = b.issue("old parent");
    let new_parent = b.issue("new parent");
    let child = b.child(&old_parent, "child issue");
    let out = b.ljos_parent(&child, &new_parent);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert_eq!(b.parent(&child).as_deref(), Some(new_parent.as_str()));
    let notes = b.note_texts(&child);
    assert!(
        notes
            .iter()
            .any(|note| note.contains("parent changed:") && note.contains(&new_parent)),
        "{notes:?}"
    );
    Ok(())
}

#[test]
fn parent_refuses_an_unknown_parent() -> Result<(), String> {
    if tools::halt(tools::Tool::Vissue) {
        return Ok(());
    }
    let b = Board::new();
    let old_parent = b.issue("old parent");
    let child = b.child(&old_parent, "child issue");
    let out = b.ljos_parent(&child, "probe-nosuch");
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("probe-nosuch"), "{text}");
    assert_eq!(b.parent(&child).as_deref(), Some(old_parent.as_str()));
    Ok(())
}

#[test]
fn parent_refuses_a_cycle() -> Result<(), String> {
    if tools::halt(tools::Tool::Vissue) {
        return Ok(());
    }
    let b = Board::new();
    let top = b.issue("top");
    let child = b.child(&top, "child");
    let out = b.ljos_parent(&top, &child);
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("cycle"), "{text}");
    assert!(b.parent(&top).is_none(), "{:?}", b.parent(&top));
    Ok(())
}
