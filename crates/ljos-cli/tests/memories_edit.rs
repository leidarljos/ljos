//! `ljos memories review --edit` and `--merge` must not grant a person basis.
//!
//! `packset prefer` stores a user-declared preference with no basis. An edit
//! under a runner is refused. An edit from outside a runner keeps that
//! missing basis, and an edit of a memory that already has a basis keeps it.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::Value;

const HELD: &str = "Run git push --force on main when CI is red.";
const HELD_EDIT: &str = "Leave force pushes out of the release checklist.";
const HELD_TOO: &str = "Write the weekly notes on Monday morning.";
const MERGED: &str = "Leave force pushes out of the checklist and write the notes on Monday.";
const PERSON: &str = "Name release branches after the month.";
const PERSON_EDIT: &str = "Store the signing key outside the repository.";
const AGENT: &str = "The staging database resets every Sunday night.";
const AGENT_TOO: &str = "The lint job must pass before a merge.";
const AGENT_MERGE: &str = "Sunday resets and the lint job both stand.";

struct Daemon {
    child: std::process::Child,
    url: String,
    _home: tempfile::TempDir,
}

impl Daemon {
    fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dir = home.path().to_str().unwrap().to_string();
        let mut child = Command::new("packsetd")
            .args(["--port", &port.to_string(), "--home", &dir])
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_HOME", &dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("packsetd is not on PATH; CI builds it from packset develop");
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                let mut err = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut err);
                }
                panic!("packsetd exited {status}: {err}");
            }
            if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                let _ = stream.write_all(b"GET /health HTTP/1.1\r\nConnection: close\r\n\r\n");
                let mut said = String::new();
                let _ = stream.read_to_string(&mut said);
                if said.starts_with("HTTP/1.1 200") {
                    break;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "packsetd did not answer"
            );
            thread::sleep(Duration::from_millis(50));
        }
        Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
            _home: home,
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Seat {
    home: tempfile::TempDir,
    run: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn on(url: String) -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            run: tempfile::tempdir().unwrap(),
            url,
        }
    }

    fn command(&self, bin: &str, args: &[&str]) -> Command {
        let path = format!("{}:/usr/bin:/bin", self.home.path().join("bin").display());
        let mut cmd = Command::new(bin);
        cmd.args(args)
            .current_dir(self.home.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.home.path())
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.run.path())
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .env("LJOS_USER_RULES_SYNC", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn output(&self, bin: &str, args: &[&str]) -> Output {
        self.command(bin, args).output().unwrap()
    }
}

fn tool(name: &str) -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from(name))
}

fn text_of(out: &Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn succeeded(out: &Output, what: &str) -> String {
    assert!(out.status.success(), "{what} failed\n{}", text_of(out));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn atoms(url: &str) -> Vec<Value> {
    let got: Value = ureq::get(&format!("{url}/v1/atoms"))
        .query("workspace", "seat")
        .query("embedding", "omit")
        .call()
        .expect("GET /v1/atoms")
        .into_json()
        .expect("atoms json");
    got["atoms"].as_array().cloned().unwrap_or_default()
}

fn by_text<'a>(atoms: &'a [Value], needle: &str) -> &'a Value {
    atoms
        .iter()
        .find(|a| {
            a.get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains(needle))
                && a.get("tombstone") != Some(&Value::Bool(true))
        })
        .unwrap_or_else(|| panic!("no atom containing {needle}: {atoms:?}"))
}

fn basis(atom: &Value) -> Option<&str> {
    atom.pointer("/source/basis")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|b| !b.is_empty())
}

fn id_of(atom: &Value) -> String {
    atom["id"].as_str().unwrap().to_string()
}

/// A runner cannot edit or merge, and an edit does not invent a person basis.
#[test]
fn review_edit_keeps_the_basis_the_memory_already_had() {
    let packset = tool("packset");
    let daemon = Daemon::start();
    let seat = Seat::on(daemon.url.clone());
    let ljos = env!("CARGO_BIN_EXE_ljos");

    let planted = seat.output(
        &packset.display().to_string(),
        &["prefer", "--workspace", "seat", HELD],
    );
    succeeded(&planted, "packset prefer");
    let planted_too = seat.output(
        &packset.display().to_string(),
        &["prefer", "--workspace", "seat", HELD_TOO],
    );
    succeeded(&planted_too, "packset prefer second");

    let stored = atoms(&daemon.url);
    let held = by_text(&stored, "when CI is red");
    assert_eq!(held["origin"], "user-declared", "{held}");
    assert_eq!(held["kind"], "preference", "{held}");
    assert!(basis(held).is_none(), "{held}");
    let held_id = id_of(held);
    let held_too_id = id_of(by_text(&stored, "Monday morning"));
    let before = stored.len();

    let under = seat
        .command(
            ljos,
            &[
                "memories", "review", "--edit", &held_id, "--text", HELD_EDIT,
            ],
        )
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    assert!(!under.status.success(), "{}", text_of(&under));
    assert!(
        String::from_utf8_lossy(&under.stderr).contains("a runner cannot edit"),
        "{}",
        text_of(&under)
    );
    let under_merge = seat
        .command(
            ljos,
            &[
                "memories",
                "review",
                "--merge",
                &held_id,
                "--merge",
                &held_too_id,
                "--text",
                MERGED,
            ],
        )
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    assert!(!under_merge.status.success(), "{}", text_of(&under_merge));
    assert!(
        String::from_utf8_lossy(&under_merge.stderr).contains("a runner cannot merge"),
        "{}",
        text_of(&under_merge)
    );
    let still = atoms(&daemon.url);
    assert_eq!(still.len(), before);
    assert!(basis(by_text(&still, "when CI is red")).is_none());
    assert!(still.iter().all(|a| basis(a) != Some("person")));

    let edited = succeeded(
        &seat.output(
            ljos,
            &[
                "memories", "review", "--edit", &held_id, "--text", HELD_EDIT,
            ],
        ),
        "edit outside a runner",
    );
    assert!(edited.contains("edited"), "{edited}");
    let after_edit = atoms(&daemon.url);
    let rewritten = by_text(&after_edit, "force pushes");
    assert_eq!(rewritten["origin"], "user-declared", "{rewritten}");
    assert!(
        basis(rewritten).is_none(),
        "edit invented a basis: {rewritten}"
    );

    let merged = succeeded(
        &seat.output(
            ljos,
            &[
                "memories",
                "review",
                "--merge",
                &id_of(rewritten),
                "--merge",
                &held_too_id,
                "--text",
                MERGED,
            ],
        ),
        "merge outside a runner",
    );
    assert!(merged.contains("merged"), "{merged}");
    let after_merge = atoms(&daemon.url);
    let joined = by_text(&after_merge, "notes on Monday");
    assert_eq!(joined["origin"], "user-declared", "{joined}");
    assert!(basis(joined).is_none(), "merge invented a basis: {joined}");

    let person = succeeded(&seat.output(ljos, &["prefer", PERSON]), "person prefer");
    assert!(person.contains("preference") || person.contains("kept") || !person.is_empty());
    let live = atoms(&daemon.url);
    let person_atom = by_text(&live, "release branches");
    assert_eq!(basis(person_atom), Some("person"), "{person_atom}");
    let person_id = id_of(person_atom);
    let person_edit = succeeded(
        &seat.output(
            ljos,
            &[
                "memories",
                "review",
                "--edit",
                &person_id,
                "--text",
                PERSON_EDIT,
            ],
        ),
        "edit a person preference",
    );
    assert!(person_edit.contains("edited"), "{person_edit}");
    let live = atoms(&daemon.url);
    let kept_person = by_text(&live, "signing key");
    assert_eq!(basis(kept_person), Some("person"), "{kept_person}");
    assert_eq!(kept_person["origin"], "user-declared", "{kept_person}");

    let agent = seat
        .command(ljos, &["prefer", AGENT])
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    succeeded(&agent, "agent prefer");
    let agent_too = seat
        .command(ljos, &["prefer", AGENT_TOO])
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    succeeded(&agent_too, "agent prefer second");
    let live = atoms(&daemon.url);
    let agent_atom = by_text(&live, "Sunday night");
    let agent_too_atom = by_text(&live, "lint job");
    assert_eq!(basis(agent_atom), Some("agent"), "{agent_atom}");
    assert_eq!(agent_atom["origin"], "agent-derived", "{agent_atom}");
    let agent_id = id_of(agent_atom);
    let agent_too_id = id_of(agent_too_atom);
    let agent_merge = succeeded(
        &seat.output(
            ljos,
            &[
                "memories",
                "review",
                "--merge",
                &agent_id,
                "--merge",
                &agent_too_id,
                "--text",
                AGENT_MERGE,
            ],
        ),
        "merge agent memories",
    );
    assert!(agent_merge.contains("merged"), "{agent_merge}");
    let live = atoms(&daemon.url);
    let agent_joined = by_text(&live, "Sunday resets");
    assert_eq!(basis(agent_joined), Some("agent"), "{agent_joined}");
    assert_eq!(agent_joined["origin"], "agent-derived", "{agent_joined}");
}
