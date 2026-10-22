//! An older preference raised to the person's whether or not the pack
//! lets an update raise `source.basis`.
//!
//! packsetd from 5be0c03 answers 400 to an update that raises
//! `source.basis` from none to `person`; one from before takes it. The
//! upgrade stores a copy as the person's and retires the original on
//! either, so the text a rule cites and a lesson's hash read the same.
//! A refusal is said once and kept, so later runs do not retry it.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

const PREFERENCE: &str = "Keep the release notes in one file.";
const LESSON: &str = "The release notes live beside the changelog.";
const DUMP: &str = "Paste the ```stdout``` of the release build into the notes.";

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

    fn post(&self, path: &str, body: &Value) -> Value {
        ureq::post(&format!("{}{path}", self.url))
            .send_json(body.clone())
            .unwrap_or_else(|e| panic!("POST {path}: {e}"))
            .into_json()
            .unwrap()
    }

    /// A preference or lesson as a terminal `ljos prefer` or `ljos
    /// remember` stored it before a basis existed.
    fn legacy(&self, kind: &str, text: &str) -> Value {
        self.post(
            "/v1/atoms",
            &json!({
                "schema": "inside.atom/v1",
                "workspace": "seat",
                "kind": kind,
                "level": "explicit",
                "origin": "user-declared",
                "text": text,
                "entities": ["seat:person", "horizon:standing"],
                "source": {
                    "harness": "person",
                    "seat": "person",
                    "session": "person",
                    "host": "box",
                    "via": "ljos",
                },
            }),
        )
    }

    fn atoms(&self) -> Vec<Value> {
        let got: Value = ureq::get(&format!("{}/v1/atoms", self.url))
            .query("workspace", "seat")
            .query("embedding", "omit")
            .call()
            .expect("GET /v1/atoms")
            .into_json()
            .expect("atoms json");
        got["atoms"].as_array().cloned().unwrap_or_default()
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
    fn on(daemon: &Daemon) -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            run: tempfile::tempdir().unwrap(),
            url: daemon.url.clone(),
        }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.home.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home.path())
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.run.path())
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .env("LJOS_USER_RULES_SYNC", "off")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn markers(&self) -> Vec<PathBuf> {
        let dir = self.home.path().join("state/ljos/pref-basis");
        std::fs::read_dir(dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }
}

fn text_of(out: &Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn reviewed(seat: &Seat) -> Output {
    let out = seat.ljos(&["prefer", "--review"]);
    assert!(out.status.success(), "{}", text_of(&out));
    out
}

fn live_with<'a>(atoms: &'a [Value], text: &str) -> Vec<&'a Value> {
    atoms.iter().filter(|a| a["text"] == text).collect()
}

fn basis(atom: &Value) -> Option<&str> {
    atom.pointer("/source/basis").and_then(Value::as_str)
}

fn without_basis(atom: &Value) -> Value {
    let mut source = atom["source"].clone();
    if let Some(map) = source.as_object_mut() {
        map.remove("basis");
    }
    source
}

/// The upgrade stores the person's copy: the same words, kind, entities
/// and clock, one live record per text, and the rule that cites the
/// preference still names it.
#[test]
fn an_older_preference_is_raised_by_a_person_copy() {
    let daemon = Daemon::start();
    let seat = Seat::on(&daemon);
    let preference = daemon.legacy("preference", PREFERENCE);
    let lesson = daemon.legacy("lesson", LESSON);
    let rule = daemon.post(
        "/v1/atoms",
        &json!({
            "schema": "inside.atom/v1",
            "workspace": "seat",
            "kind": "rule",
            "level": "explicit",
            "origin": "user-declared",
            "text": PREFERENCE,
            "pattern": "git push --force*",
            "verdict": "deny",
            "source": {"via": "ljos", "basis": "person"},
        }),
    );
    let before = daemon.atoms();

    let out = reviewed(&seat);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("no older preferences await review"), "{said}");

    let after = daemon.atoms();
    assert_eq!(before.len(), after.len(), "{after:?}");
    for (old, text) in [(&preference, PREFERENCE), (&lesson, LESSON)] {
        let held = live_with(&after, text);
        let held: Vec<&&Value> = held.iter().filter(|a| a["kind"] == old["kind"]).collect();
        assert_eq!(held.len(), 1, "{after:?}");
        let new = held[0];
        assert_eq!(basis(new), Some("person"), "{new}");
        assert_eq!(new["origin"], "user-declared", "{new}");
        for key in ["kind", "level", "entities", "ts", "valid_from"] {
            assert_eq!(new[key], old[key], "{key}: {new}");
        }
        assert_eq!(without_basis(new), old["source"], "{new}");
    }
    let kept_rule = after
        .iter()
        .find(|a| a["id"] == rule["id"])
        .unwrap_or_else(|| panic!("the rule is gone: {after:?}"));
    assert_eq!(kept_rule["text"], PREFERENCE);

    let mut raised = lesson.clone();
    raised["source"]["basis"] = json!("person");
    let new_lesson = live_with(&after, LESSON)[0];
    assert_eq!(
        ljos_cli::user_rules::lesson_hash(new_lesson),
        ljos_cli::user_rules::lesson_hash(&raised),
        "{new_lesson}"
    );

    let marks = seat.markers();
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!(std::fs::read_to_string(&marks[0]).unwrap(), "once\n");
    std::fs::remove_file(&marks[0]).unwrap();
    reviewed(&seat);
    let again = daemon.atoms();
    let ids = |atoms: &[Value]| {
        let mut ids: Vec<String> = atoms
            .iter()
            .filter_map(|a| a["id"].as_str().map(str::to_string))
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(ids(&after), ids(&again), "a second run stamped again");
}

/// A record the pack will not take back is left as it was, said once on
/// stderr and written down, and the next run does not send it again. The
/// other older preference is still raised.
#[test]
fn a_refused_raise_is_said_once_and_not_retried() {
    let daemon = Daemon::start();
    let seat = Seat::on(&daemon);
    let dumped = daemon.legacy("preference", "Paste the build output into the notes.");
    daemon.post(
        "/v1/atoms/update",
        &json!({"workspace": "seat", "id": dumped["id"], "fields": {"text": DUMP}}),
    );
    daemon.legacy("preference", PREFERENCE);

    let first = reviewed(&seat);
    let err = String::from_utf8_lossy(&first.stderr);
    assert_eq!(err.matches("will not try again").count(), 1, "{err}");
    let id = dumped["id"].as_str().unwrap();
    assert!(err.contains(id), "{err}");
    assert!(err.contains("tool dump"), "{err}");
    assert!(
        !err.contains("```stdout```"),
        "the refusal names the text: {err}"
    );

    let after = daemon.atoms();
    let left = live_with(&after, DUMP);
    assert_eq!(left.len(), 1, "{after:?}");
    assert_eq!(left[0]["id"], dumped["id"], "{after:?}");
    assert_eq!(basis(left[0]), None, "{}", left[0]);
    let raised = live_with(&after, PREFERENCE);
    assert_eq!(raised.len(), 1, "{after:?}");
    assert_eq!(basis(raised[0]), Some("person"), "{}", raised[0]);

    let marks = seat.markers();
    assert_eq!(marks.len(), 1, "{marks:?}");
    let written = std::fs::read_to_string(&marks[0]).unwrap();
    assert!(written.starts_with("refused\n"), "{written}");
    assert!(written.contains(id), "{written}");

    let second = reviewed(&seat);
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(!err.contains("will not try again"), "{err}");
    assert_eq!(daemon.atoms(), after, "a second run sent the raise again");
    assert_eq!(std::fs::read_to_string(&marks[0]).unwrap(), written);
}
