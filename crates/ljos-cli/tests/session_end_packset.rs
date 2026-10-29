//! Session end stores facts in a real pack when it is asked to.
//!
//! `ljos hook` takes a SessionEnd payload. packsetd, from PATH, is the
//! pack, in its own home. A live sitting is a hold whose process is this
//! test. Extraction runs unless `LJOS_EXTRACT=off`. The stored note names
//! that issue, stays an agent note, and a later prompt does not repeat
//! the sentence.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::Value;

const FACT: &str = "The root cause was a shared temp directory between the two test processes.";
const ISSUE: &str = "probe-0001";
/// Unset: extraction is on. These calls do not set `LJOS_EXTRACT`.
const ASKED: &[(&str, &str)] = &[];

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
                let _ = stream.write_all(
                    b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
                );
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
    root: tempfile::TempDir,
}

impl Seat {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("home")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        Self { root }
    }

    fn home(&self) -> std::path::PathBuf {
        self.root.path().join("home")
    }

    fn run(&self) -> std::path::PathBuf {
        self.root.path().join("run")
    }

    /// A hold whose conversation is this test, the parent of `ljos hook`.
    fn sit(&self) {
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
        let dir = self.run().join("ljos");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hold-probe-sitting"),
            format!(
                "book-holder\nprobe-seat\n{}\n{}\n2026-01-01T00:00:00Z\n{ISSUE}\n-\n",
                std::process::id(),
                comm.trim()
            ),
        )
        .unwrap();
    }
}

fn transcript(home: &std::path::Path) -> std::path::PathBuf {
    let path = home.join("transcript.jsonl");
    std::fs::write(
        &path,
        format!(
            "{}\n",
            serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": FACT}]
                }
            })
        ),
    )
    .unwrap();
    path
}

fn hook(seat: &Seat, url: &str, payload: &str, extra: &[(&str, &str)]) -> Output {
    let home = seat.home();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(["hook"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("USER", "person")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", seat.run())
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "8000")
        .env("LJOS_SEAT", "probe-seat")
        .env("LJOS_HOLD", "book-holder")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("LJOS_EXTRACT_DEBUG", "1")
        .env("VISSUE_NO_ROUTE", "1")
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(payload.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn text_of(out: &Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn session_end(seat: &Seat, url: &str, extra: &[(&str, &str)]) -> Output {
    let path = transcript(&seat.home());
    let payload = serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "extract-session-1",
        "transcript_path": path,
        "cwd": seat.home(),
    })
    .to_string();
    hook(seat, url, &payload, extra)
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

fn fact_atom(stored: &[Value]) -> Option<&Value> {
    stored.iter().find(|atom| {
        atom["text"]
            .as_str()
            .is_some_and(|text| text.contains(FACT))
            && atom["tombstone"] != Value::Bool(true)
    })
}

fn no_http_refusal(stderr: &str) {
    assert!(
        !stderr.contains("400:") && !stderr.contains("note atom needs issue"),
        "debug reported a raw pack refusal:\n{stderr}"
    );
}

#[test]
fn session_end_stores_facts_on_the_open_sitting() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let pack = Daemon::start();
    let seat = Seat::new();
    seat.sit();
    let out = session_end(&seat, &pack.url, ASKED);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "hook failed\n{}", text_of(&out));
    no_http_refusal(&stderr);
    assert!(
        !stderr.contains("holds no sitting"),
        "a live sitting was treated as none:\n{stderr}"
    );
    assert!(
        stderr.contains("wrote 1"),
        "debug did not report the write:\n{stderr}"
    );
    let stored = atoms(&pack.url);
    let atom = fact_atom(&stored).unwrap_or_else(|| {
        panic!(
            "extracted fact was not stored\n{}\natoms: {stored:?}",
            text_of(&out)
        )
    });
    assert_eq!(atom["kind"], "note", "{atom}");
    assert_eq!(atom["issue"], ISSUE, "{atom}");
    assert_eq!(atom["source"]["basis"], "agent", "{atom}");
    assert_eq!(atom["origin"], "agent-derived", "{atom}");
    let entities = atom["entities"].to_string();
    assert!(
        entities.contains("note-basis:agent"),
        "the note is not marked agent: {atom}"
    );
    assert!(
        entities.contains("via:session-end"),
        "the note has no session-end tag: {atom}"
    );

    let prompt = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "extract-session-2",
        "cwd": seat.home(),
        "prompt": "How should the next change be tested?",
    })
    .to_string();
    let shown = hook(&seat, &pack.url, &prompt, &[]);
    assert!(
        shown.status.success(),
        "prompt hook failed\n{}",
        text_of(&shown)
    );
    let stdout = String::from_utf8_lossy(&shown.stdout);
    assert!(
        !stdout.contains("root cause") && !stdout.contains(FACT),
        "the prompt hook injected the note:\n{stdout}"
    );
}

#[test]
fn session_end_without_a_sitting_does_not_post() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let pack = Daemon::start();
    let seat = Seat::new();
    let out = session_end(&seat, &pack.url, ASKED);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "hook failed\n{}", text_of(&out));
    assert!(
        fact_atom(&atoms(&pack.url)).is_none(),
        "a note was stored with no sitting\n{stderr}"
    );
    assert!(
        stderr.contains("holds no sitting") && stderr.contains("only on a ticket"),
        "stderr did not say why the write was skipped:\n{stderr}"
    );
    no_http_refusal(&stderr);
    assert_eq!(
        stderr.matches("holds no sitting").count(),
        1,
        "more than one skip line:\n{stderr}"
    );
}

#[test]
fn session_end_extract_off_stores_nothing() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let pack = Daemon::start();
    let seat = Seat::new();
    seat.sit();
    let out = session_end(&seat, &pack.url, &[("LJOS_EXTRACT", "off")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "hook failed\n{}", text_of(&out));
    assert!(
        fact_atom(&atoms(&pack.url)).is_none(),
        "LJOS_EXTRACT=off still stored a note\n{stderr}"
    );
    assert!(
        !stderr.contains("holds no sitting"),
        "extract off still talked about a sitting:\n{stderr}"
    );
    no_http_refusal(&stderr);
}

/// With `LJOS_EXTRACT` unset, the sitting's note is written.
#[test]
fn session_end_stores_facts_when_extract_is_unset() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let pack = Daemon::start();
    let seat = Seat::new();
    seat.sit();
    let out = session_end(&seat, &pack.url, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "hook failed\n{}", text_of(&out));
    assert!(
        fact_atom(&atoms(&pack.url)).is_some(),
        "an unset session end stored no note\n{stderr}"
    );
    no_http_refusal(&stderr);
}
