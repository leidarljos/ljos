//! ljos-vl5z: a person's yes, carried from chat by the seat that heard it.
//!
//! `ljos accept` wanted a terminal of the person's own, since a runner
//! that accepted its own proposal would be reviewing itself. A yes said in
//! chat now counts when the seat passes on the words, the time and its own
//! name, and the acceptance keeps all three. The terminal path still needs
//! no quote.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

const ID: &str = "9c1227b4f0a84e6b8d3f5a1c7e204bd1";
const TEXT: &str = "Pin the parser to the released grammar.";

/// A pack that answers one stored atom, and records what it was asked.
fn serve(seen: Arc<Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            if let Some(line) = req.lines().next() {
                seen.lock().unwrap().push(line.to_string());
            }
            let body = format!(
                r#"{{"id":"a1b2c3","kind":"lesson","text":"{TEXT}","origin":"agent-derived","level":"explicit"}}"#
            );
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

struct Seat {
    dir: tempfile::TempDir,
    pack: String,
    _seen: Arc<Mutex<Vec<String>>>,
}

impl Seat {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let pack = serve(Arc::clone(&seen));
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        let seat = Self {
            dir,
            pack,
            _seen: seen,
        };
        seat.file_a_proposal();
        seat
    }

    fn state(&self) -> PathBuf {
        self.dir.path().join("home/.state/ljos")
    }

    fn proposals(&self) -> PathBuf {
        self.state().join("proposals.jsonl")
    }

    /// One open proposal, as `ljos finish --lesson` leaves it: an agent's
    /// own lesson, naming no conversation.
    fn file_a_proposal(&self) {
        let row = serde_json::json!({
            "id": ID,
            "status": "open",
            "remote": false,
            "text": TEXT,
            "kind": "lesson",
            "origin": "agent-derived",
            "atom": {
                "kind": "lesson",
                "text": TEXT,
                "workspace": "seat",
                "level": "derived",
                "origin": "agent-derived",
                "entities": ["seat:saddle"],
                "source": {"seat": "saddle"},
            },
        });
        std::fs::create_dir_all(self.state()).unwrap();
        let mut line = serde_json::to_string(&row).unwrap();
        line.push('\n');
        std::fs::write(self.proposals(), line).unwrap();
    }

    fn rows(&self) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(self.proposals()).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn accept(&self, args: &[&str]) -> Output {
        let home = self.dir.path().join("home");
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .arg("accept")
            .arg(ID)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("LJOS_SEAT", "saddle")
            .env("PACKSET_URL", &self.pack)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_TIMEOUT_MS", "4000")
            // The conversation this runs in, so the refusal under test is
            // the one an agent accepting its own proposal meets.
            .env("ACMEAGENT_CONVERSATION_ID", "0199a1b2-c3d4-e5f6")
            .output()
            .unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn an_hour_ago() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 3600;
    ljos_cli::utc_at(secs)
}

/// The words, the time and the seat go in with the lesson, and the line
/// the seat prints says whose yes it was.
#[test]
fn a_quoted_yes_accepts_the_lesson_and_is_kept_with_it() {
    let seat = Seat::new();
    let at = an_hour_ago();
    let quote = "yes, keep that one about the parser grammar";
    let out = seat.accept(&["--quote", quote, "--at", &at, "--via", "inky"]);
    let text = said(&out);
    assert!(out.status.success(), "accept refused a quoted yes: {text}");
    assert!(text.contains("accepted"), "{text}");
    assert!(text.contains(quote), "the words are said back: {text}");
    assert!(text.contains("inky"), "the seat is named: {text}");

    let rows = seat.rows();
    let last = rows.last().expect("an accepted row");
    assert_eq!(last["status"], "accepted", "{last}");
    assert_eq!(last["grant"]["quote"], quote, "{last}");
    assert_eq!(last["grant"]["at"], at, "{last}");
    assert_eq!(last["grant"]["via"], "inky", "{last}");
    assert_eq!(
        rows[0].get("grant"),
        None,
        "the open row kept no grant: {}",
        rows[0]
    );
}

/// Without the words, the same call from the same conversation is the
/// refusal it always was, and it names the way through.
#[test]
fn an_agent_still_cannot_accept_its_own_proposal_unquoted() {
    let seat = Seat::new();
    let out = seat.accept(&[]);
    let text = said(&out);
    assert!(!out.status.success(), "accept wrote it anyway: {text}");
    assert!(text.contains("can't accept its own proposal"), "{text}");
    assert!(text.contains("--quote"), "{text}");
    assert_eq!(seat.rows().len(), 1, "the proposal is still open");
    assert_eq!(seat.rows()[0]["status"], "open");
}

/// A quote with no time and no seat records nothing, so it accepts
/// nothing.
#[test]
fn a_half_recorded_yes_writes_nothing() {
    let seat = Seat::new();
    let out = seat.accept(&["--quote", "yes, keep that one about the parser"]);
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("--at and --via"), "{text}");
    assert!(text.contains("Nothing was written"), "{text}");
    assert_eq!(seat.rows().len(), 1);
    assert_eq!(seat.rows()[0]["status"], "open");
}

/// The quote has to be the person's sentence. A bare yes, and a time
/// nobody has reached, are refused.
#[test]
fn a_yes_that_records_nothing_useful_is_refused() {
    let seat = Seat::new();
    let at = an_hour_ago();
    let out = seat.accept(&["--quote", "yes", "--at", &at, "--via", "inky"]);
    assert!(!out.status.success());
    let text = said(&out);
    assert!(text.contains("1 words"), "{text}");

    let ahead = ljos_cli::utc_at(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 86_400,
    );
    let out = seat.accept(&[
        "--quote",
        "yes, keep that one about the parser",
        "--at",
        &ahead,
        "--via",
        "inky",
    ]);
    assert!(!out.status.success());
    assert!(said(&out).contains("ahead of now"), "{}", said(&out));
    assert_eq!(seat.rows()[0]["status"], "open");
}
