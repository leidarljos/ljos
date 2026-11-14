//! Every memory verb stores from a plain process, with no runner above it
//! and no terminal: the write is live at once and labelled with whose it
//! is. `prefer --user` is listed by `ljos rules`; `remember`, `finish
//! --lesson`, a choice a prompt states and a proposal an older ljos left
//! open are found by `ljos search` right after.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

struct Daemon {
    writer: ljos_test_boot::Packsetd,
}

impl Daemon {
    fn start() -> Self {
        Self {
            writer: ljos_test_boot::Packsetd::open(),
        }
    }

    fn port(&self) -> u16 {
        self.writer.port()
    }

    /// Every live atom whose text is `text`.
    fn atoms(&self, text: &str) -> Vec<Value> {
        let said = get(self.port(), "/v1/atoms?workspace=seat");
        let body = ljos_test_boot::http_body(&said).unwrap_or_else(|err| panic!("{err}: {said}"));
        let atoms: Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"));
        atoms["atoms"]
            .as_array()
            .map(|all| all.iter().filter(|a| a["text"] == text).cloned().collect())
            .unwrap_or_default()
    }

    /// The one live atom whose text is `text`.
    fn atom(&self, text: &str) -> Value {
        let mut found = self.atoms(text);
        assert_eq!(found.len(), 1, "{text:?}: {found:?}");
        found.remove(0)
    }
}

fn get(port: u16, path: &str) -> String {
    let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return String::new();
    };
    let _ =
        stream.write_all(format!("GET {path} HTTP/1.1\r\nConnection: close\r\n\r\n").as_bytes());
    let mut said = String::new();
    let _ = stream.read_to_string(&mut said);
    said
}

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A seat on `port` with a tracker holding one ticket and stand-ins for the
/// tracker, claim and deed tools.
struct Seat {
    root: tempfile::TempDir,
    port: u16,
    issue: String,
}

impl Seat {
    fn on(port: u16) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("state")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        let tracker = root.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software")).unwrap();
        let layout = vissue_core::Layout::new(&tracker, "Software");
        vissue_core::ops::create(&layout, "probe", "Release tags", Default::default()).unwrap();
        let doc = vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe"))
            .unwrap();
        let issue = doc.headings[0].id.clone();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        script(
            &bin,
            "vissue",
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"{issue}\",\"title\":\"Release tags\",\"state\":\"TODO\"}}' ;;\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\nesac\nexit 0\n",
                tracker.display()
            ),
        );
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\n  claim) echo gen=1 ;;\n  complete) echo done ;;\n  *) echo '[]' ;;\nesac\nexit 0\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\nexit 0\n");
        Self { root, port, issue }
    }

    /// `ljos ARGS` with only this seat's environment: no runner variable,
    /// no terminal, `stdin` piped in.
    fn ljos(&self, args: &[&str], stdin: &str) -> Output {
        let root = self.root.path();
        let home = root.join("home");
        let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(root)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.join("bin").display()),
            )
            .env("HOME", &home)
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("VISSUE_ROOT", root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", format!("http://127.0.0.1:{}", self.port))
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut pipe = child.stdin.take().unwrap();
        pipe.write_all(stdin.as_bytes()).unwrap();
        drop(pipe);
        child.wait_with_output().unwrap()
    }

    /// [`Seat::ljos`] that has to succeed. Its stdout and stderr.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.ljos(args, "");
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "ljos {args:?} failed: {said}");
        said
    }

    /// Run the hook on one event payload. What it printed.
    fn hook(&self, event: &str, prompt: &str) -> String {
        let payload = serde_json::json!({
            "hook_event_name": event,
            "session_id": "plain-1",
            "cwd": self.root.path(),
            "prompt": prompt,
        });
        let out = self.ljos(&["hook"], &payload.to_string());
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "hook {event} failed: {said}");
        said
    }

    /// `ljos search QUERY` finds `text`.
    fn finds(&self, query: &str, text: &str) {
        let found = self.ok(&["search", query]);
        assert!(
            found.contains(text),
            "search {query:?} missed {text:?}:\n{found}"
        );
    }
}

fn seat() -> Option<(Daemon, Seat)> {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return None;
    }
    let daemon = Daemon::start();
    let seat = Seat::on(daemon.port());
    Some((daemon, seat))
}

/// The agent's labels on a live atom: origin and basis.
fn assert_agents(atom: &Value) {
    assert_eq!(atom["origin"], "agent-derived", "{atom}");
    assert_eq!(atom["source"]["basis"], "agent", "{atom}");
}

#[test]
fn prefer_user_from_a_plain_process_is_listed_by_rules() {
    let Some((_daemon, seat)) = seat() else {
        return;
    };
    let rule = "Tag a release with git push origin TAG, never with follow-tags.";
    let said = seat.ok(&["prefer", "--user", "--why", "the release doc says so", rule]);
    assert!(!said.contains("refus"), "{said}");
    let listed = seat.ok(&["rules"]);
    let line = listed
        .lines()
        .find(|line| line.contains(rule))
        .unwrap_or_else(|| panic!("`ljos rules` does not list the rule:\n{listed}"));
    assert!(
        line.starts_with("unverified\t")
            && line
                .ends_with("written without the person at the terminal or in their latest prompt"),
        "the rule is not labelled as the agent's: {line}"
    );
}

#[test]
fn remember_from_a_plain_process_is_found_at_once_inside_a_sitting_too() {
    let Some((daemon, seat)) = seat() else {
        return;
    };
    let before = "Push a release tag by name, because follow-tags skips v-tags.";
    let said = seat.ok(&["remember", before]);
    assert!(!said.contains("unreviewed note"), "{said}");
    seat.finds("how do I push a release tag", before);
    let atom = daemon.atom(before);
    assert_agents(&atom);
    assert_eq!(atom["kind"], "lesson", "{atom}");

    seat.ok(&["sitting", &seat.issue]);
    let sitting = "Sign the release tag before pushing it to the mirror.";
    seat.ok(&["remember", sitting]);
    seat.finds("sign the release tag", sitting);
    let atom = daemon.atom(sitting);
    assert_agents(&atom);
    assert_eq!(
        atom["kind"], "lesson",
        "a sitting's remember is a lesson: {atom}"
    );
}

#[test]
fn finish_lesson_from_a_plain_process_is_found_at_once_as_unreviewed() {
    let Some((daemon, seat)) = seat() else {
        return;
    };
    let lesson = "Check the mirror for the signed tag after a release push.";
    seat.ok(&["sitting", &seat.issue]);
    let said = seat.ok(&["finish", &seat.issue, "--gen", "1", "--lesson", lesson]);
    assert!(said.contains("agent-derived, unreviewed"), "{said}");
    seat.finds("check the mirror for the tag", lesson);
    let atom = daemon.atom(lesson);
    assert_agents(&atom);
    assert_eq!(atom["level"], "derived", "{atom}");

    let id = atom["id"].as_str().unwrap();
    let accepted = seat.ok(&["accept", id]);
    assert!(accepted.contains("origin agent-derived"), "{accepted}");
    assert_eq!(daemon.atom(lesson)["level"], "explicit");
    seat.finds("check the mirror for the tag", lesson);
}

#[test]
fn a_choice_a_plain_prompt_states_is_stored_as_the_agents() {
    let Some((daemon, seat)) = seat() else {
        return;
    };
    let said = "From now on always use bun, never yarn, in this repo";
    let context = seat.hook("UserPromptSubmit", said);
    assert!(
        context.contains("Stored that sentence as an agent-derived preference"),
        "{context}"
    );
    seat.finds("bun yarn", said);
    assert_agents(&daemon.atom(said));
}

#[test]
fn a_proposal_an_older_ljos_left_open_is_found_after_session_start() {
    let Some((daemon, seat)) = seat() else {
        return;
    };
    let text = "Mirror pushes need the deploy key loaded first.";
    let atom = serde_json::json!({
        "kind": "lesson",
        "text": text,
        "workspace": "seat",
        "origin": "agent-derived",
        "level": "derived",
        "source": {"basis": "agent"},
    });
    let row = serde_json::json!({
        "id": "5d1c8e2a9b0f",
        "status": "open",
        "remote": false,
        "text": text,
        "kind": "lesson",
        "origin": "agent-derived",
        "atom": atom,
    });
    let rows = seat.root.path().join("home/state/ljos/proposals.jsonl");
    std::fs::create_dir_all(rows.parent().unwrap()).unwrap();
    std::fs::write(&rows, format!("{row}\n")).unwrap();

    seat.hook("SessionStart", "");
    seat.finds("mirror deploy key", text);
    assert_agents(&daemon.atom(text));
    seat.hook("SessionStart", "");
    assert_eq!(daemon.atoms(text).len(), 1, "imported once");
}
