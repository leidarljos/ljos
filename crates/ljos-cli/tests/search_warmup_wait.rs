//! A short reported encoder load is waited out. A long one is not.
//!
//! The fake writer answers HTTP only. `remaining_ms` is the load it
//! reports. When `ready_after` is set, status stays cold until that
//! long after bind, and until the warm-up poll after the sizing reads.
//! The first warm-up poll stays cold, and so does every status read
//! before that loop. A search served while cold leaves the later
//! status cold. `None` stays cold.
//!
//! A tracker ticket whose title matches the query is on both pages:
//! the meaning page after a short load, and the word-match page when
//! the reported load is longer than the warm-up budget.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MEANING: &str = "the kiln schedule is the morning plan";
const WORD: &str = "the kiln log is a word match";
const WORD_LINE_60: &str = "These are word matches; the encoder is loading (about 60 s left).";
const WORD_LINE_3: &str = "These are word matches; the encoder is loading (about 3 s left).";
const WORD_LINE_4: &str = "These are word matches; the encoder is loading (about 4 s left).";
const TICKET_TITLE: &str = "GPRidge narrated video set";
const TICKET_ID: &str = "Software-6mrw";

struct Seat {
    url: String,
    home: tempfile::TempDir,
    /// Status polls this writer has answered.
    status_polls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let rest = line
                .strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))?;
            rest.trim().parse().ok()
        })
        .unwrap_or(0)
}

/// `remaining_ms` is what a cold status reports. `ready_after` is how
/// long after bind the writer may first answer ready, and only on a
/// warm-up poll after the sizing reads. `None` stays cold.
fn serve(remaining_ms: u64, ready_after: Option<Duration>) -> Seat {
    serve_as(remaining_ms, ready_after, false)
}

/// Status answers before the ready signal.
///
/// Three reads size the call and see the load. The next is the first
/// warm-up poll and stays cold. The one after that is the signal.
const COLD_STATUS_POLLS: usize = 4;

fn serve_as(remaining_ms: u64, ready_after: Option<Duration>, older_status: bool) -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let status_polls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let polls = std::sync::Arc::clone(&status_polls);
    let searched = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let opened = Instant::now();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let polls = std::sync::Arc::clone(&polls);
            let searched = std::sync::Arc::clone(&searched);
            thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let cl = content_length(&head);
                while buf.len() < head_end + cl {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let seen = if path == "/v1/status" {
                    polls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
                } else {
                    polls.load(std::sync::atomic::Ordering::SeqCst)
                };
                let searched_before = searched.load(std::sync::atomic::Ordering::SeqCst);
                if path == "/v1/search" {
                    searched.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                let due = ready_after.is_some_and(|delay| opened.elapsed() >= delay);
                let ready = older_status || (due && !searched_before && seen > COLD_STATUS_POLLS);
                let body = if path == "/v1/status" {
                    if older_status {
                        r#"{"embedder":{"available":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                            .to_string()
                    } else if ready {
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                            .to_string()
                    } else {
                        format!(
                            r#"{{"embedder":{{"available":true,"answering":null,"enabled":true,"remaining_ms":{remaining_ms}}},"search":{{"ranks":["fused","relevance"]}}}}"#
                        )
                    }
                } else if path == "/v1/search" {
                    if ready {
                        format!(
                            r#"{{"hits":[{{"id":"mean","kind":"lesson","text":"{MEANING}","score":0.91,"entities":["seat:travel"],"dense_rank":1,"cosine":0.84,"ballots":1,"of":3}}],"rank":"relevance"}}"#
                        )
                    } else {
                        format!(
                            r#"{{"hits":[{{"id":"word","kind":"lesson","text":"{WORD}","score":0.8,"entities":["seat:travel"]}}],"rank":"fused"}}"#
                        )
                    }
                } else if path == "/v1/atoms" {
                    r#"{"atoms":[{"id":"mean"}]}"#.to_string()
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
    });
    Seat {
        url: format!("http://127.0.0.1:{port}"),
        home,
        status_polls,
    }
}

#[test]
fn a_normal_search_without_an_answering_field_does_not_say_it_is_warming() {
    let seat = serve_as(0, None, true);
    let (out, _) = search(&seat, &["search", "kiln"], &[("LJOS_SEARCH_WAIT", "0")]);
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(stdout.contains(MEANING), "{said}");
    assert!(stderr.contains(ljos_cli::MEANING_LINE), "{said}");
    assert!(
        !stderr.contains("These are word matches")
            && !stderr.contains("ready shortly")
            && !stderr.contains("loading"),
        "{said}"
    );
}

fn search(seat: &Seat, args: &[&str], env: &[(&str, &str)]) -> (Output, Duration) {
    let home = seat.home.path();
    let started = Instant::now();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .current_dir(home)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home)
        .env("PACKSET_URL", &seat.url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("LJOS_HOOKD", "off")
        .env("NO_COLOR", "1")
        .env("LJOS_SEAT", "travel")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let out = cmd.output().unwrap();
    (out, started.elapsed())
}

fn text(out: &Output) -> (String, String, String) {
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let said = format!("{stdout}{stderr}");
    (stdout, stderr, said)
}

fn json_doc(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim()).unwrap_or_else(|err| panic!("{err}: {stdout}"))
}

fn assert_word_fallback(stdout: &str, stderr: &str, want: &str) {
    assert!(
        stderr.contains(want),
        "missing the word-match line:\n{stderr}"
    );
    assert!(
        !stderr.contains("rerun") && !stderr.contains("Matched by "),
        "{stderr}"
    );
    assert!(stdout.contains(WORD), "{stdout}");
    assert!(!stdout.contains(MEANING), "{stdout}");
}

/// The encoder is ready two seconds after the writer starts, and the
/// reported remaining time fits in the default warm-up budget.
#[test]
fn a_load_ready_after_two_seconds_is_ranked_by_meaning() {
    let seat = serve(3_000, Some(Duration::from_secs(2)));
    let (out, _) = search(&seat, &["search", "kiln"], &[("LJOS_SEARCH_WAIT", "0")]);
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        seat.status_polls.load(std::sync::atomic::Ordering::SeqCst) >= 3,
        "the search did not poll until the encoder answered"
    );
    assert!(stderr.contains(ljos_cli::MEANING_LINE), "{said}");
    assert!(
        !stderr.contains("word-match") && !stderr.contains("encoder is loading"),
        "a finished load still printed the warm-up line:\n{stderr}"
    );
    assert!(stdout.contains(MEANING), "{stdout}");
    assert!(!stdout.contains(WORD), "{stdout}");

    let seat = serve(3_000, Some(Duration::from_secs(2)));
    let (out, _) = search(
        &seat,
        &["search", "--json", "kiln"],
        &[("LJOS_SEARCH_WAIT", "0")],
    );
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        !stderr.contains("word-match") && !stderr.contains("Matched by "),
        "json printed the plain line: {stderr}"
    );
    let doc = json_doc(&stdout);
    assert_eq!(doc["ranking"], "semantic", "{doc}");
    assert!(doc.get("reason").is_none(), "{doc}");
    assert!(
        doc.get("waited_ms").is_some(),
        "the wait did not run: {doc}"
    );
    assert!(
        doc["hits"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|hit| hit["text"] == MEANING)),
        "{doc}"
    );
}

/// Sixty seconds does not fit in the warm-up budget, so the command
/// returns the word matches without waiting that load out.
#[test]
fn a_sixty_second_load_returns_word_matches_at_once() {
    let seat = serve(60_000, None);
    let (out, _) = search(&seat, &["search", "kiln"], &[("LJOS_SEARCH_WAIT", "0")]);
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        seat.status_polls.load(std::sync::atomic::Ordering::SeqCst) < 6,
        "a long load was polled until it was ready"
    );
    assert_word_fallback(&stdout, &stderr, WORD_LINE_60);

    let seat = serve(60_000, None);
    let (out, _) = search(
        &seat,
        &["search", "--json", "kiln"],
        &[("LJOS_SEARCH_WAIT", "0")],
    );
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        !stderr.contains("word-match") && !stderr.contains("rerun"),
        "json printed the plain line: {stderr}"
    );
    let doc = json_doc(&stdout);
    assert_eq!(doc["ranking"], "lexical", "{doc}");
    assert_eq!(doc["reason"], WORD_LINE_60, "{doc}");
    assert!(doc.get("waited_ms").is_none(), "{doc}");
    assert!(
        doc["hits"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|hit| hit["text"] == WORD)),
        "{doc}"
    );
}

/// `LJOS_SEARCH_WARMUP_WAIT=0` leaves a short load on the word-match path.
#[test]
fn a_warmup_wait_of_zero_does_not_wait() {
    let seat = serve(3_000, Some(Duration::from_secs(2)));
    let (out, _) = search(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WAIT", "0"), ("LJOS_SEARCH_WARMUP_WAIT", "0")],
    );
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        seat.status_polls.load(std::sync::atomic::Ordering::SeqCst) < 6,
        "wait=0 polled until the encoder was ready"
    );
    assert_word_fallback(&stdout, &stderr, WORD_LINE_3);
}

/// The warm-up poll stops at `LJOS_CALL_TIMEOUT` even when the reported
/// remaining time fits in the warm-up budget and `LJOS_SEARCH_WAIT` is long.
#[test]
fn the_warmup_wait_stops_inside_the_call_deadline() {
    let seat = serve(4_000, None);
    let env = [("LJOS_CALL_TIMEOUT", "2"), ("LJOS_SEARCH_WAIT", "30")];
    let (out, _) = search(&seat, &["search", "kiln"], &env);
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        seat.status_polls.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "the deadline path never asked the encoder"
    );
    assert_word_fallback(&stdout, &stderr, WORD_LINE_4);

    let seat = serve(4_000, None);
    let (out, _) = search(&seat, &["search", "--json", "kiln"], &env);
    let (stdout, _, said) = text(&out);
    assert!(out.status.success(), "{said}");
    let doc = json_doc(&stdout);
    assert_eq!(doc["ranking"], "lexical", "{doc}");
    assert_eq!(doc["reason"], WORD_LINE_4, "{doc}");
    assert!(
        doc.get("waited_ms").is_some(),
        "the deadline wait did not run: {doc}"
    );
}

fn write_ticket(seat: &Seat) -> String {
    let tracker = seat.home.path().join("tracker");
    std::fs::create_dir_all(tracker.join("Software/Software")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        tracker.join("Software/Software/issues.org"),
        format!(
            "\
#+TITLE: Software issues
#+VISSUE: 1
#+CATEGORY: Software
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] {TICKET_TITLE}
:PROPERTIES:
:ID:         {TICKET_ID}
:CREATED:    [2026-11-01 Sat]
:END:
"
        ),
    )
    .unwrap();
    tracker.display().to_string()
}

fn search_ticket(seat: &Seat, args: &[&str], extra: &[(&str, &str)]) -> (Output, Duration) {
    let root = write_ticket(seat);
    let mut env = vec![
        ("VISSUE_ROOT", root.as_str()),
        ("VISSUE_NO_ROUTE", "1"),
        ("LJOS_TRACKER_GIT", "off"),
        ("LJOS_SEARCH_WAIT", "0"),
    ];
    env.extend_from_slice(extra);
    search(seat, args, &env)
}

fn ticket_hit(doc: &serde_json::Value) -> &serde_json::Value {
    doc["hits"]
        .as_array()
        .and_then(|rows| rows.iter().find(|hit| hit["id"] == TICKET_ID))
        .unwrap_or_else(|| panic!("ticket missing: {doc}"))
}

/// A short load is waited out, then the ticket title is the first row
/// of the meaning page, beside `waited_ms`.
#[test]
fn a_short_load_keeps_the_ticket_title_on_the_meaning_path() {
    let seat = serve(3_000, Some(Duration::from_secs(2)));
    let (out, _) = search_ticket(&seat, &["search", TICKET_TITLE], &[]);
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        seat.status_polls.load(std::sync::atomic::Ordering::SeqCst) >= 3,
        "the search did not poll until the encoder answered"
    );
    assert!(stderr.contains(ljos_cli::FUSED_RANK_LINE), "{said}");
    assert!(
        !stderr.contains("word-match") && !stderr.contains("rerun"),
        "{stderr}"
    );
    assert!(
        stdout.contains(&format!("ticket {TICKET_ID}")),
        "the ticket id is missing:\n{stdout}"
    );
    assert!(stdout.contains(TICKET_TITLE), "{stdout}");
    let ticket_at = stdout.find(TICKET_ID).expect("ticket");
    if let Some(lesson_at) = stdout.find(MEANING) {
        assert!(ticket_at < lesson_at, "the title was not first:\n{stdout}");
    }

    let seat = serve(3_000, Some(Duration::from_secs(2)));
    let (out, _) = search_ticket(&seat, &["search", "--json", TICKET_TITLE], &[]);
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        !stderr.contains("word-match") && !stderr.contains("Matched by "),
        "json printed the plain line: {stderr}"
    );
    let doc = json_doc(&stdout);
    assert_eq!(doc["ranking"], "semantic", "{doc}");
    assert!(doc.get("reason").is_none(), "{doc}");
    assert!(
        doc.get("waited_ms").is_some(),
        "the wait did not run: {doc}"
    );
    let hit = ticket_hit(&doc);
    assert_eq!(hit["kind"], "ticket", "{doc}");
    assert_eq!(hit["text"], TICKET_TITLE, "{doc}");
    assert_eq!(hit["title"], TICKET_TITLE, "{doc}");
    assert_eq!(hit["place"], 1, "{doc}");
    assert!(hit.get("score").is_some(), "{doc}");
}

/// A load longer than the warm-up budget returns at once, and the
/// ticket title is still on the word-match page.
#[test]
fn a_long_load_keeps_the_ticket_title_with_the_word_matches() {
    let seat = serve(60_000, None);
    let (out, _) = search_ticket(&seat, &["search", TICKET_TITLE], &[]);
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        seat.status_polls.load(std::sync::atomic::Ordering::SeqCst) < 6,
        "a long load was polled until it was ready"
    );
    assert!(stderr.contains(WORD_LINE_60), "{stderr}");
    assert!(
        !stderr.contains("rerun") && !stderr.contains("Matched by "),
        "{stderr}"
    );
    assert!(
        stdout.contains(&format!("ticket {TICKET_ID}")),
        "the ticket id is missing:\n{stdout}"
    );
    assert!(stdout.contains(TICKET_TITLE), "{stdout}");
    assert!(!stdout.contains("No hits"), "{stdout}");

    let seat = serve(60_000, None);
    let (out, _) = search_ticket(&seat, &["search", "--json", TICKET_TITLE], &[]);
    let (stdout, stderr, said) = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        !stderr.contains("word-match") && !stderr.contains("rerun"),
        "json printed the plain line: {stderr}"
    );
    let doc = json_doc(&stdout);
    assert_eq!(doc["ranking"], "lexical", "{doc}");
    assert_eq!(doc["reason"], WORD_LINE_60, "{doc}");
    assert!(doc.get("waited_ms").is_none(), "{doc}");
    let hit = ticket_hit(&doc);
    assert_eq!(hit["kind"], "ticket", "{doc}");
    assert_eq!(hit["title"], TICKET_TITLE, "{doc}");
    assert_eq!(hit["text"], TICKET_TITLE, "{doc}");
    assert_eq!(hit["place"], 1, "{doc}");
}
