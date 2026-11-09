//! A preference a bot seat stores is injected on the next matching prompt.
//!
//! The seat is not a person at a terminal, so the preference is stored
//! agent-derived. A fresh hook session whose prompt matches still receives
//! it, and the origin is named on the line.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

const TOKEN: &str = "0e1d2c3b4a59687706f5e4d3c2b1a0900e1d2c3b4a59687706f5e4d3c2b1a090";
const SEAT: &str = "ljosbot";
const PREFERENCE: &str = "Use rtrash instead of rm for deleting files on the box.";
const LESSON: &str = "Run the linter before tagging a release.";
const LESSON_ID: &str = "c0ffee00c0ffee00c0ffee00c0ffee00";

fn read_http(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = match stream.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&tmp[..n]);
        let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let header = String::from_utf8_lossy(&buf[..pos]);
        let len = header
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if buf.len() >= pos + 4 + len {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn authorized(req: &str) -> bool {
    req.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value.trim() == format!("Bearer {TOKEN}")
        })
    })
}

fn reply(stream: &mut std::net::TcpStream, status: &str, payload: &str) {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(resp.as_bytes());
}

/// A pack that stores what this seat posts and answers the hook's listing.
/// The encoder has not answered, so the hook ranks the words it already holds.
fn serve(seed: Vec<serde_json::Value>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let atoms = Arc::new(Mutex::new(seed));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let atoms = Arc::clone(&atoms);
            thread::spawn(move || {
                let req = read_http(&mut stream);
                let path = req.lines().next().unwrap_or("");
                let body = req.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
                if !authorized(&req) && !path.starts_with("GET /health") {
                    reply(&mut stream, "401 Unauthorized", r#"{"error":"token"}"#);
                    return;
                }
                if path.starts_with("GET /v1/status") {
                    reply(
                        &mut stream,
                        "200 OK",
                        r#"{"embedder":{"available":true,"answering":null}}"#,
                    );
                    return;
                }
                if path.starts_with("POST /v1/atoms/update") {
                    let asked: serde_json::Value =
                        serde_json::from_str(body).unwrap_or(serde_json::json!({}));
                    let id = asked["id"].as_str().unwrap_or("");
                    let mut stored = atoms.lock().unwrap();
                    let Some(atom) = stored
                        .iter_mut()
                        .find(|atom| atom["id"].as_str() == Some(id))
                    else {
                        drop(stored);
                        reply(&mut stream, "404 Not Found", r#"{"error":"no atom"}"#);
                        return;
                    };
                    if let Some(fields) = asked.get("fields").and_then(|v| v.as_object()) {
                        let obj = atom.as_object_mut().unwrap();
                        for (key, value) in fields {
                            obj.insert(key.clone(), value.clone());
                        }
                    }
                    let payload = atom.to_string();
                    drop(stored);
                    reply(&mut stream, "200 OK", &payload);
                    return;
                }
                if path.starts_with("POST /v1/atoms") {
                    let mut atom: serde_json::Value =
                        serde_json::from_str(body).unwrap_or(serde_json::json!({}));
                    let mut stored = atoms.lock().unwrap();
                    if atom.get("id").and_then(|v| v.as_str()).is_none() {
                        atom["id"] = serde_json::json!(format!("{:032x}", stored.len() + 1));
                    }
                    stored.push(atom.clone());
                    drop(stored);
                    reply(&mut stream, "200 OK", &atom.to_string());
                    return;
                }
                if let Some(rest) = path.strip_prefix("GET /v1/atoms/") {
                    let id = rest.split(['?', ' ']).next().unwrap_or("");
                    let stored = atoms.lock().unwrap();
                    let found = stored.iter().find(|atom| atom["id"].as_str() == Some(id));
                    match found {
                        Some(atom) => {
                            let payload = atom.to_string();
                            drop(stored);
                            reply(&mut stream, "200 OK", &payload);
                        }
                        None => {
                            drop(stored);
                            reply(&mut stream, "404 Not Found", r#"{"error":"no atom"}"#);
                        }
                    }
                    return;
                }
                if path.starts_with("GET /v1/atoms") {
                    let stored = atoms.lock().unwrap();
                    let payload = serde_json::json!({"atoms": *stored}).to_string();
                    drop(stored);
                    reply(&mut stream, "200 OK", &payload);
                    return;
                }
                if path.starts_with("GET /v1/search") {
                    let stored = atoms.lock().unwrap();
                    let hits: Vec<serde_json::Value> = stored
                        .iter()
                        .map(|atom| {
                            let mut hit = atom.clone();
                            hit["field"] = serde_json::json!("atom");
                            hit["score"] = serde_json::json!(1.0);
                            hit
                        })
                        .collect();
                    drop(stored);
                    reply(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({"hits": hits}).to_string(),
                    );
                    return;
                }
                reply(&mut stream, "200 OK", r#"{"hits":[],"atoms":[]}"#);
            });
        }
    });
    format!("http://{addr}")
}

struct Seat {
    home: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn new(seed: Vec<serde_json::Value>) -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            url: serve(seed),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.home.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home.path())
            .env("USER", "seat")
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.home.path().join("run"))
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_TOKEN", TOKEN)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_CAPTURE", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", SEAT)
            .env("CURSOR_CONVERSATION_ID", "0199a1b2-c3d4-e5f6-bot")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn ljos(&self, args: &[&str]) -> Output {
        self.command(args).stdin(Stdio::null()).output().unwrap()
    }

    /// What a fresh prompt session is told, as plain text.
    fn prompt(&self, session: &str, prompt: &str) -> String {
        let payload = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": session,
            "prompt": prompt,
        })
        .to_string();
        let mut child = self
            .command(&["hook", "--prompt", "--text"])
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        drop(child.stdin.take());
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook failed\n{}", text_of(&out));
        format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
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

fn hidden_lesson() -> serde_json::Value {
    serde_json::json!({
        "id": LESSON_ID,
        "kind": "lesson",
        "text": LESSON,
        "workspace": "seat",
        "origin": "agent-derived",
        "level": "derived",
        "ts": "2026-11-08T15:00:00.000Z",
        "entities": ["seat:other-seat"],
        "source": {"seat": "other-seat", "basis": "agent", "at": "2026-11-08T15:00:00Z"},
    })
}

/// `ljos prefer` as a bot seat, then a new session whose prompt matches.
#[test]
fn a_bot_seat_preference_is_injected_on_a_fresh_prompt() {
    let seat = Seat::new(Vec::new());
    let stored = seat.ljos(&["prefer", PREFERENCE]);
    assert!(
        stored.status.success(),
        "prefer failed\n{}",
        text_of(&stored)
    );
    let note = seat.prompt(
        "fresh-pref-session-1",
        "delete files on the box with rtrash",
    );
    assert!(
        note.contains("rtrash"),
        "the fresh prompt did not receive the preference:\n{note}"
    );
    assert!(
        note.contains("agent-derived"),
        "the origin was not named beside the preference:\n{note}"
    );
    let lesson = note
        .lines()
        .filter(|line| !line.contains("label prompt-origin:"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !lesson.contains("the person's words"),
        "an agent preference was labelled as the person's:\n{note}"
    );
}

/// A lesson with no horizon stays off the prompt until `ljos accept`.
#[test]
fn accept_promotes_a_lesson_onto_the_prompt() {
    let seat = Seat::new(vec![hidden_lesson()]);
    let before = seat.prompt(
        "fresh-lesson-before",
        "run the linter before tagging a release",
    );
    assert!(
        !before.contains("linter"),
        "the lesson reached the prompt before accept:\n{before}"
    );
    let accepted = seat.ljos(&["accept", LESSON_ID]);
    assert!(
        accepted.status.success(),
        "accept failed\n{}",
        text_of(&accepted)
    );
    let said = text_of(&accepted);
    assert!(
        said.contains("agent-derived"),
        "accept rewrote the origin:\n{said}"
    );
    let after = seat.prompt(
        "fresh-lesson-after",
        "run the linter before tagging a release",
    );
    assert!(
        after.contains("linter"),
        "accept did not put the lesson on the next prompt:\n{after}"
    );
    assert!(
        after.contains("agent-derived"),
        "the accepted lesson hid its origin:\n{after}"
    );
}
