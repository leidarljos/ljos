//! `ljos search` and `ljos remember` take `-p`, the same project a ticket lives in.
//!
//! Search keeps that project's tickets and lessons, and a lesson that names
//! no project. Remember records the project on the lesson. An unknown name
//! exits 1 and names the projects. A verb with no project names the verbs
//! that take `-p`.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const FINANCE_TICKET: &str = "finance-6mrw";
const GARDEN_TICKET: &str = "garden-6mrw";
const VERBS: &str = "The verbs that take -p are calibrate, claims, file, list, ready, remember, search and sitting.";

struct Seat {
    root: PathBuf,
    url: String,
    posts: Arc<Mutex<Vec<String>>>,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "work",
            "tracker/Software/finance",
            "tracker/Software/garden",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        write_issue(
            &root.join("tracker/Software/finance/issues.org"),
            "finance",
            FINANCE_TICKET,
            "kiln finance ledger",
        );
        write_issue(
            &root.join("tracker/Software/garden/issues.org"),
            "garden",
            GARDEN_TICKET,
            "kiln garden ledger",
        );
        let posts = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let recorded = Arc::clone(&posts);
        thread::spawn(move || serve(listener, recorded));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
            posts,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
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
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("LJOS_SEARCH_BUSY_WAIT", "0")
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

fn write_issue(path: &std::path::Path, project: &str, id: &str, title: &str) {
    let body = format!(
        "\
#+TITLE: {project} issues
#+VISSUE: 1
#+CATEGORY: {project}
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] {title}
:PROPERTIES:
:ID:         {id}
:CREATED:    [2026-11-01 Sat]
:END:
"
    );
    std::fs::write(path, body).unwrap();
}

fn hit(id: &str, text: &str, entities: &[&str]) -> String {
    let entities = entities
        .iter()
        .map(|entity| format!("\"{entity}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"id":"{id}","kind":"lesson","text":"{text}","score":90,"ts":"2026-11-01T00:00:00.000Z","entities":[{entities}],"ballots":1,"of":1}}"#
    )
}

fn search_body() -> String {
    let hits = [
        hit(
            "lesson-finance",
            "kiln finance lesson",
            &["seat:alice", "project:finance"],
        ),
        hit(
            "lesson-garden",
            "kiln garden lesson",
            &["seat:alice", "project:garden"],
        ),
        hit("lesson-global", "kiln global lesson", &["seat:alice"]),
        hit(
            "lesson-finance-issue",
            "kiln finance issue lesson",
            &["seat:alice", "issue:finance-yyyy"],
        ),
        hit(
            "lesson-garden-issue",
            "kiln garden issue lesson",
            &["seat:alice", "issue:garden-zzzz"],
        ),
    ]
    .join(",");
    format!(r#"{{"hits":[{hits}],"rank":"relevance"}}"#)
}

fn serve(listener: TcpListener, posts: Arc<Mutex<Vec<String>>>) {
    let lessons = search_body();
    let status = r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#;
    for conn in listener.incoming().flatten() {
        let lessons = lessons.clone();
        let status = status.to_string();
        let posts = Arc::clone(&posts);
        thread::spawn(move || {
            let mut sock = conn;
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if request_complete(&buf) {
                            break;
                        }
                    }
                }
            }
            if buf.is_empty() {
                return;
            }
            let head_end = buf
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap_or(buf.len());
            let head = String::from_utf8_lossy(&buf[..head_end.min(buf.len())]).to_string();
            let request = head.split_whitespace().nth(1).unwrap_or("/");
            let path = request.split('?').next().unwrap_or("/");
            let method = head.split_whitespace().next().unwrap_or("");
            let raw_body = if head_end + 4 <= buf.len() {
                String::from_utf8_lossy(&buf[head_end + 4..]).into_owned()
            } else {
                String::new()
            };
            let body = if method == "POST" && path == "/v1/atoms" {
                posts.lock().unwrap().push(raw_body);
                r#"{"id":"atom-1","kind":"lesson","text":"stored"}"#.to_string()
            } else if path == "/v1/search" {
                lessons
            } else if path == "/v1/status" {
                status
            } else if path == "/v1/atoms" {
                r#"{"atoms":[]}"#.to_string()
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

fn request_complete(buf: &[u8]) -> bool {
    let Some(head_end) = buf.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&buf[..head_end]);
    let length = head.lines().find_map(|line| {
        let rest = line
            .split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))?
            .1
            .trim();
        rest.parse::<usize>().ok()
    });
    match length {
        Some(n) => buf.len() >= head_end + 4 + n,
        None => true,
    }
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn page_keeps_finance(out: &Output) {
    let dump = said(out);
    assert!(out.status.success(), "{dump}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(FINANCE_TICKET), "{dump}");
    assert!(stdout.contains("kiln finance lesson"), "{dump}");
    assert!(stdout.contains("kiln global lesson"), "{dump}");
    assert!(stdout.contains("kiln finance issue lesson"), "{dump}");
    assert!(!stdout.contains(GARDEN_TICKET), "{dump}");
    assert!(!stdout.contains("kiln garden"), "{dump}");
}

#[test]
fn search_limits_rows_to_one_project() {
    let seat = Seat::open();
    let scoped = seat.run(&[
        "search",
        "kiln",
        "-p",
        "Finance",
        "--min-score",
        "0",
        "-n",
        "20",
    ]);
    page_keeps_finance(&scoped);
    let long = seat.run(&[
        "search",
        "--project",
        "finance",
        "--min-score",
        "0",
        "-n",
        "20",
        "kiln",
    ]);
    page_keeps_finance(&long);
    let open = seat.run(&["search", "kiln", "--min-score", "0", "-n", "20"]);
    let dump = said(&open);
    assert!(open.status.success(), "{dump}");
    let stdout = String::from_utf8_lossy(&open.stdout);
    assert!(stdout.contains(FINANCE_TICKET), "{dump}");
    assert!(stdout.contains(GARDEN_TICKET), "{dump}");
    assert!(stdout.contains("kiln garden lesson"), "{dump}");
}

#[test]
fn an_unknown_project_names_the_projects() {
    let seat = Seat::open();
    let line = "project: boats is not a project. The projects are finance, garden.";
    for args in [
        &["search", "kiln", "-p", "boats"][..],
        &[
            "remember",
            "The kiln door stays shut.",
            "--project",
            "boats",
        ][..],
    ] {
        let out = seat.run(args);
        let dump = said(&out);
        assert_eq!(out.status.code(), Some(1), "{dump}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.lines().any(|row| row == line),
            "expected one line {line:?}\n{dump}"
        );
    }
    assert!(
        seat.posts.lock().unwrap().is_empty(),
        "remember wrote a lesson for an unknown project"
    );
}

#[test]
fn remember_records_the_project_on_the_lesson() {
    let seat = Seat::open();
    let out = seat.run(&["remember", "-p", "Finance", "The kiln door stays shut."]);
    let dump = said(&out);
    assert!(out.status.success(), "{dump}");
    let posts = seat.posts.lock().unwrap();
    assert_eq!(posts.len(), 1, "{posts:?}\n{dump}");
    assert!(
        posts[0].contains("project:finance"),
        "the lesson has no project tag:\n{}\n{dump}",
        posts[0]
    );
    drop(posts);
    let plain = seat.run(&["remember", "The kiln door stays open."]);
    let dump = said(&plain);
    assert!(plain.status.success(), "{dump}");
    let posts = seat.posts.lock().unwrap();
    assert_eq!(posts.len(), 2, "{posts:?}");
    assert!(
        !posts[1].contains("project:"),
        "a lesson with no -p gained a project tag:\n{}",
        posts[1]
    );
}

#[test]
fn a_verb_without_a_project_names_the_verbs_that_take_one() {
    let seat = Seat::open();
    for verb in ["note", "claim", "finish", "recall", "show"] {
        let out = seat.run(&[verb, "-p", "finance"]);
        let dump = said(&out);
        assert_eq!(out.status.code(), Some(2), "{verb}\n{dump}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        let line = format!("ljos: {verb} has no flag -p. {VERBS}");
        assert!(
            stderr.lines().any(|row| row == line),
            "expected {line:?}\n{dump}"
        );
    }
    let sitting = seat.run(&["sitting", FINANCE_TICKET, "-p", "finance"]);
    let dump = said(&sitting);
    assert_eq!(sitting.status.code(), Some(2), "{dump}");
    let stderr = String::from_utf8_lossy(&sitting.stderr);
    let line = format!("ljos: sitting takes -p with --plan. {VERBS}");
    assert!(
        stderr.lines().any(|row| row == line),
        "expected {line:?}\n{dump}"
    );
    let bare = seat.run(&["sitting", "-p", "finance"]);
    let dump = said(&bare);
    assert_eq!(bare.status.code(), Some(2), "{dump}");
    let stderr = String::from_utf8_lossy(&bare.stderr);
    assert!(
        stderr.lines().any(|row| row == line),
        "expected {line:?}\n{dump}"
    );
}
