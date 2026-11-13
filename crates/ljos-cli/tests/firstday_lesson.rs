//! A first lesson on an empty pack: a long sentence the pack refuses, a
//! paragraph split into claims, an edit, and recall with no ticket or no
//! lesson of its own.

ljos_test_boot::harness!();

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const LONG: &str = "The north bench kiln must be fired only after the door thermocouple reads below eighty degrees because a warmer door warps the seal and the next batch then cracks along the rim before it ever reaches glaze.";
const SHORT: &str = "Fire the kiln before dusk on the north bench.";
const PARA: &str = "Fire the kiln before dusk. Leave the door shut until the shelf is cold. Write the batch number on the shelf card. Sweep the floor after the last tray.";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Seat {
    root: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn on(url: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        std::fs::create_dir_all(root.path().join("tracker")).unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        // Keep the real tools when this process can see them. A stub is
        // only a stand-in for a tool the seat does not call.
        for name in ["vissue", "packset", "packsetd"] {
            if let Some(path) = std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join(name))
                    .find(|p| p.is_file())
            }) {
                let _ = std::os::unix::fs::symlink(&path, bin.join(name));
            }
        }
        if !bin.join("vissue").is_file() {
            script(
                &bin,
                "vissue",
                "#!/bin/sh\necho \"issue $2 not found\" >&2\nexit 1\n",
            );
        }
        Self {
            root,
            url: url.to_string(),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let root = self.root.path();
        let home = root.join("home");
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(root)
            .env_clear()
            .env("PATH", format!("{}:{path}", root.join("bin").display()))
            .env("HOME", &home)
            .env("USER", "ada")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("VISSUE_ROOT", root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        self.run_in(args, None)
    }

    fn run_in(&self, args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
        let mut cmd = self.command(args);
        if stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().expect("ljos");
        if let Some(text) = stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        }
        let out = child.wait_with_output().expect("ljos");
        (
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn pack() -> Option<ljos_test_boot::Packsetd> {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return None;
    }
    Some(ljos_test_boot::Packsetd::open())
}

#[test]
fn a_long_lesson_the_pack_refuses_is_not_described_as_shown_by_the_hook() {
    let Some(daemon) = pack() else {
        return;
    };
    let seat = Seat::on(daemon.url());
    let (code, stdout, stderr) = seat.run(&["remember", LONG]);
    let said = format!("{stdout}{stderr}");
    assert_eq!(code, 0, "{said}");
    assert!(stderr.contains("kept locally, not in the pack"), "{stderr}");
    assert!(stderr.contains("limit 29"), "{stderr}");
    assert!(
        stderr.contains("the hook does not show a lesson the pack refused"),
        "{stderr}"
    );
    assert!(
        !said.contains("the hook shows it from the next prompt"),
        "{said}"
    );
    let (code, search, search_err) = seat.run(&["search", "thermocouple"]);
    let found = format!("{search}{search_err}");
    assert_eq!(code, 0, "{found}");
    assert!(found.contains("thermocouple"), "{found}");
    assert!(found.contains("not in the pack"), "{found}");
    assert!(found.contains("kept locally"), "{found}");
    assert!(found.contains("local, not in the pack"), "{found}");
    let (code, hook, hook_err) = seat.run_in(
        &["hook", "--prompt", "--text"],
        Some("The door thermocouple reads below eighty degrees.\n"),
    );
    let heard = format!("{hook}{hook_err}");
    assert_eq!(code, 0, "{heard}");
    assert!(
        !hook.contains("thermocouple"),
        "the hook showed a lesson the pack refused:\n{heard}"
    );
}

#[test]
fn a_paragraph_says_why_it_was_stored_as_more_than_one_claim() {
    let Some(daemon) = pack() else {
        return;
    };
    let seat = Seat::on(daemon.url());
    let (code, stdout, stderr) = seat.run(&["remember", PARA]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stderr.contains("2 claims from one text"), "{stderr}");
    assert!(
        stderr.contains("A lesson is at most two sentences"),
        "{stderr}"
    );
    assert!(stderr.contains("`ljos search` lists them"), "{stderr}");
}

#[test]
fn an_edit_names_the_list_that_hides_a_kept_row() {
    let Some(daemon) = pack() else {
        return;
    };
    let seat = Seat::on(daemon.url());
    let (code, stdout, stderr) = seat.run(&["remember", SHORT]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let id = stdout.split('\t').next().unwrap_or("").trim().to_string();
    assert!(id.len() >= 8, "{stdout}");
    let (code, edited, err) = seat.run(&[
        "memories",
        "review",
        "--edit",
        &id,
        "--text",
        "Leave the kiln door shut until the shelf is cold.",
    ]);
    assert_eq!(code, 0, "{edited}{err}");
    assert!(edited.contains("leaves a kept row out"), "{edited}");
    assert!(edited.contains("`ljos memories list --all`"), "{edited}");
    assert!(edited.contains("`ljos forget "), "{edited}");
}

#[test]
fn a_missing_memory_id_names_the_list_and_search() {
    let Some(daemon) = pack() else {
        return;
    };
    let seat = Seat::on(daemon.url());
    let (code, stdout, stderr) = seat.run(&[
        "memories",
        "review",
        "--edit",
        "abcd1234abcd",
        "--text",
        "Leave the kiln cold.",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("no memory starts with abcd1234abcd"),
        "{stdout}"
    );
    assert!(
        stdout.contains("`ljos memories list --all --user`"),
        "{stdout}"
    );
    assert!(stdout.contains("`ljos search`"), "{stdout}");
}

#[test]
fn recall_of_an_unknown_id_names_search_and_file() {
    let Some(daemon) = pack() else {
        return;
    };
    let seat = Seat::on(daemon.url());
    let (code, stdout, stderr) = seat.run(&["recall", "kiln"]);
    assert_ne!(code, 0, "{stdout}{stderr}");
    let said = format!("{stdout}{stderr}");
    assert!(said.contains("issue kiln not found"), "{said}");
    assert!(said.contains("so run `ljos ready`"), "{said}");
}

#[test]
fn recall_of_an_issue_with_no_lesson_names_the_next_command() {
    let Some(daemon) = pack() else {
        return;
    };
    if !ljos_cli::tool_installed("vissue") {
        eprintln!("skipped: vissue is not installed");
        return;
    }
    let seat = Seat::on(daemon.url());
    let (code, filed, err) = seat.run(&["file", "Paint the cedar fence", "-p", "demo"]);
    assert_eq!(code, 0, "{filed}{err}");
    let id = filed.lines().next().unwrap_or("").trim();
    assert!(!id.is_empty(), "{filed}");
    let (code, recall, err) = seat.run(&["recall", id]);
    assert_eq!(code, 0, "{recall}{err}");
    assert!(
        recall.contains(&format!("no lesson on {id}")),
        "{recall}{err}"
    );
    assert!(recall.contains("`ljos remember \"TEXT\"`"), "{recall}");
}

#[test]
fn recall_says_when_a_lesson_is_related_and_not_filed_on_the_issue() {
    let Some(daemon) = pack() else {
        return;
    };
    if !ljos_cli::tool_installed("vissue") {
        eprintln!("skipped: vissue is not installed");
        return;
    }
    let seat = Seat::on(daemon.url());
    let (code, stdout, stderr) = seat.run(&["remember", SHORT]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, filed, err) = seat.run(&[
        "file",
        "Ship the fuse change",
        "--tags",
        "kiln,bench",
        "-p",
        "demo",
    ]);
    assert_eq!(code, 0, "{filed}{err}");
    let id = filed.lines().next().unwrap_or("").trim();
    let (code, recall, err) = seat.run(&["recall", id]);
    assert_eq!(code, 0, "{recall}{err}");
    assert!(recall.contains("(none filed on this issue)"), "{recall}");
    assert!(recall.contains("Related"), "{recall}");
    assert!(recall.contains("north bench"), "{recall}");
}
