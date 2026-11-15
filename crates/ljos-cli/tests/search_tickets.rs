//! `ljos search` finds a tracker ticket by its title, and then by its notes.
//!
//! The pack can be warm or still loading its encoder. An unrelated query
//! stays empty. The row names the ticket id, which `ljos recall` accepts.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

const TITLE: &str = "GPRidge narrated video set";
const TICKET: &str = "Software-6mrw";
const NOTE: &str = "Milestone: the ridge walk is filmed.";
const LESSON: &str = "GPRidge narrated video set is filed beside the lesson.";

struct Seat {
    root: PathBuf,
    url: String,
}

impl Seat {
    fn open(warm: bool) -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "work",
            "tracker/Software/Software",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        std::fs::write(
            root.join("tracker/Software/Software/issues.org"),
            format!(
                "\
#+TITLE: Software issues
#+VISSUE: 1
#+CATEGORY: Software
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] {TITLE}
:PROPERTIES:
:ID:         {TICKET}
:CREATED:    [2026-11-01 Sat]
:END:
:LOGBOOK:
- Note: \"{NOTE}\" [2026-11-02 Sun 10:00]
:END:
"
            ),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || serve(listener, warm));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
        }
    }

    fn search(&self, args: &[&str]) -> Output {
        let home = self.root.join("home");
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")))
    }
}

fn serve(listener: TcpListener, warm: bool) {
    let status = if warm {
        r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
    } else {
        r#"{"embedder":{"available":true,"answering":null,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
    };
    let lesson = format!(
        r#"{{"hits":[{{"id":"lesson-1","kind":"lesson","text":"{LESSON}","score":0.4,"ts":"2026-11-01T00:00:00.000Z","entities":["seat:alice"],"ballots":1,"of":1}}],"rank":"relevance"}}"#
    );
    for conn in listener.incoming().flatten() {
        let status = status.to_string();
        let lesson = lesson.clone();
        thread::spawn(move || {
            let mut sock = conn;
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            while !buf.windows(4).any(|window| window == b"\r\n\r\n") {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                }
            }
            let head_end = buf
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap()
                + 4;
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let request = head.split_whitespace().nth(1).unwrap_or("/");
            let path = request.split('?').next().unwrap_or("/");
            let body = if path == "/v1/search" {
                lesson
            } else if path == "/v1/status" {
                status
            } else if path == "/v1/atoms" {
                r#"{"atoms":[{"id":"lesson-1","kind":"lesson","text":"kept"}]}"#.to_string()
            } else {
                r#"{"ok":true}"#.to_string()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        });
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

fn shows_ticket(out: &Output) {
    let said = text(out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("ticket {TICKET}")),
        "the ticket id is missing:\n{said}"
    );
    assert!(stdout.contains(TITLE), "the title is missing:\n{said}");
    assert!(
        !stdout.contains("ridge walk"),
        "the note was printed in full:\n{said}"
    );
}

fn hides_ticket(out: &Output) {
    let said = text(out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains(TICKET), "{said}");
    assert!(!stdout.contains("GPRidge"), "{said}");
    assert!(
        stdout.contains("Nothing matched") || stdout.contains("Nothing relevant"),
        "{said}"
    );
}

#[test]
fn an_exact_title_is_the_first_row_after_the_encoder_has_answered() {
    let seat = Seat::open(true);
    let out = seat.search(&["search", TITLE]);
    shows_ticket(&out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ticket_at = stdout.find(TICKET).expect("ticket");
    let lesson_at = stdout.find("filed beside").expect("lesson");
    assert!(ticket_at < lesson_at, "{stdout}");
    assert!(stdout.contains("place 1"), "{stdout}");
}

#[test]
fn a_subset_of_the_title_returns_the_ticket() {
    let seat = Seat::open(true);
    let out = seat.search(&["search", "narrated video"]);
    shows_ticket(&out);
}

#[test]
fn the_same_title_is_found_while_the_encoder_loads() {
    let seat = Seat::open(false);
    let out = seat.search(&["search", TITLE]);
    shows_ticket(&out);
    let said = text(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("These are word matches, and meaning search will be ready shortly.")
            && !stderr.contains("rerun"),
        "the warm-up line was missing:\n{said}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ticket_at = stdout.find(TICKET).expect("ticket");
    let lesson_at = stdout.find("filed beside").expect("lesson");
    assert!(ticket_at < lesson_at, "{stdout}");
}

#[test]
fn a_subset_is_found_while_the_encoder_loads() {
    let seat = Seat::open(false);
    let out = seat.search(&["search", "narrated video"]);
    shows_ticket(&out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("These are word matches, and meaning search will be ready shortly.")
            && !stderr.contains("rerun"),
        "the warm-up line was missing:\n{}",
        text(&out)
    );
}

#[test]
fn notes_match_after_the_title_and_the_note_stays_off_the_page() {
    let seat = Seat::open(true);
    let out = seat.search(&["search", "milestone filmed"]);
    shows_ticket(&out);
}

#[test]
fn an_unrelated_query_returns_nothing() {
    let seat = Seat::open(true);
    hides_ticket(&seat.search(&["search", "zzzznotaword"]));
    let cold = Seat::open(false);
    hides_ticket(&cold.search(&["search", "zzzznotaword"]));
}

#[test]
fn json_keeps_the_old_fields_and_adds_the_ticket_title() {
    let seat = Seat::open(true);
    let out = seat.search(&["search", TITLE, "--json"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let doc: serde_json::Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap())
        .unwrap_or_else(|err| panic!("{err}\n{said}"));
    assert_eq!(doc["ranking"], "semantic", "{said}");
    let hits = doc["hits"].as_array().expect("hits");
    assert!(hits.len() >= 2, "{said}");
    assert_eq!(hits[0]["id"], TICKET, "{said}");
    assert_eq!(hits[0]["kind"], "ticket", "{said}");
    assert_eq!(hits[0]["text"], TITLE, "{said}");
    assert_eq!(hits[0]["title"], TITLE, "{said}");
    assert_eq!(hits[0]["ticket"], TICKET, "{said}");
    assert_eq!(hits[0]["place"], 1, "{said}");
    assert!(hits[0].get("score").is_some(), "{said}");
    for key in ["id", "kind", "text", "score", "place"] {
        assert!(
            hits[1].get(key).is_some(),
            "{key} missing on the memory row:\n{said}"
        );
    }
    assert!(hits[1].get("title").is_none(), "{said}");
    assert_eq!(hits[1]["kind"], "lesson", "{said}");
}
