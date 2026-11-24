//! Hyphen-leading file text reaches the real sibling vissue as data.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::process::{Command, Output};

struct Tracker {
    root: tempfile::TempDir,
    tracker: std::path::PathBuf,
    home: std::path::PathBuf,
}

impl Tracker {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let tracker = root.path().join("tracker");
        let project = tracker.join("Software/demo");
        let home = root.path().join("home");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(home.join("run")).unwrap();
        ljos_cli::write_file(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        ljos_cli::write_file(
            project.join("issues.org"),
            "#+TITLE: demo\n#+VISSUE: 1\n#+CATEGORY: demo\n#+TODO: TODO STARTED | DONE CANCELLED\n",
        )
        .unwrap();
        Self {
            root,
            tracker,
            home,
        }
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = ljos_cli::program(program).unwrap();
        cmd.current_dir(self.root.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_PROJECT", "demo")
            .env("LJOS_SEAT", "rohit")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .stdin(std::process::Stdio::null());
        cmd
    }

    fn file(&self, args: &[&str]) -> serde_json::Value {
        let out = self
            .command(env!("CARGO_BIN_EXE_ljos"))
            .arg("file")
            .args(args)
            .output()
            .unwrap();
        assert_success(&out, args);
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert!(!id.is_empty(), "{args:?}: no issue id");
        let shown = self
            .command("vissue")
            .args(["show", &id, "--json"])
            .output()
            .unwrap();
        assert_success(&shown, &["vissue", "show", &id]);
        serde_json::from_slice(&shown.stdout).unwrap_or_else(|err| {
            panic!(
                "{args:?}: vissue show was not JSON: {err}\n{}",
                String::from_utf8_lossy(&shown.stdout)
            )
        })
    }

    fn ljos(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .output()
            .unwrap()
    }

    fn vissue_json(&self, args: &[&str]) -> serde_json::Value {
        let out = self.command("vissue").args(args).output().unwrap();
        assert_success(&out, args);
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

fn assert_success(out: &Output, args: &[&str]) {
    assert!(
        out.status.success(),
        "{args:?}: status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn tracker() -> Option<Tracker> {
    if tools::halt(tools::Tool::Vissue) {
        None
    } else {
        Some(Tracker::new())
    }
}

#[test]
fn file_binds_a_short_hyphen_title_for_vissue() {
    let Some(tracker) = tracker() else { return };
    let short = tracker.file(&["-p", "demo", "--", "-x"]);
    assert_eq!(short["title"], "-x", "{short}");
}

#[test]
fn file_binds_a_hyphen_body_passed_as_the_next_word_for_vissue() {
    let Some(tracker) = tracker() else { return };
    let list = tracker.file(&["title ok", "-p", "demo", "--body", "- first item"]);
    assert_eq!(list["body"], "- first item", "{list}");
}

#[test]
fn file_binds_an_equals_hyphen_body_for_vissue() {
    let Some(tracker) = tracker() else { return };
    let body = tracker.file(&["title ok", "-p", "demo", "--body=-x"]);
    assert_eq!(body["body"], "-x", "{body}");
}

#[test]
fn file_binds_a_numeric_hyphen_title_for_vissue() {
    let Some(tracker) = tracker() else { return };
    let numeric = tracker.file(&["--", "-5 degrees"]);
    assert_eq!(numeric["title"], "-5 degrees", "{numeric}");
}

#[test]
fn note_options_and_vote_forward_hyphen_leading_text_to_vissue() {
    let Some(tracker) = tracker() else { return };
    let issue = tracker.file(&["forward text", "-p", "demo"])["id"]
        .as_str()
        .unwrap()
        .to_string();

    for args in [
        &["note", &issue, "--", "-note text"][..],
        &["options", &issue, "--", "-left,right"][..],
        &["vote", &issue, "--for", "-left", "--used", "none"][..],
    ] {
        let out = tracker.ljos(args);
        assert_success(&out, args);
    }

    let shown = tracker.vissue_json(&["show", &issue, "--json"]);
    assert!(
        shown["body"]
            .as_str()
            .unwrap_or("")
            .contains("Options: -left, right"),
        "{shown}"
    );
    assert!(
        shown["logbook"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| row["note"].as_str() == Some("-note text")),
        "{shown}"
    );
    let ballots = tracker.vissue_json(&["vote", &issue, "--json"]);
    assert!(
        ballots
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| row["choice"].as_str() == Some("-left")),
        "{ballots}"
    );
}
