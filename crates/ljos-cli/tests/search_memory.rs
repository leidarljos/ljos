//! A cold encoder is asked again once it has answered, and the ranking
//! line names the rows that were printed. A lesson the pack did not take
//! is still named when search runs.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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

struct Pack {
    url: String,
    searches: Arc<AtomicUsize>,
}

/// `warm_after_search` flips the encoder to answered once a search has
/// been seen. Otherwise status fails after that search, so the wait
/// stops at once and the first, word-ranked, answer is what stands.
fn pack(warm_after_search: bool, post_status: u16) -> Pack {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let searches = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&searches);
    let warm = Arc::new(AtomicBool::new(warm_after_search));
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let count = Arc::clone(&count);
            let warm = Arc::clone(&warm);
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
                let seen = count.load(Ordering::SeqCst);
                let (status, body) = if path == "/v1/search" {
                    count.fetch_add(1, Ordering::SeqCst);
                    let body = if seen == 0 {
                        r#"{"hits":[{"id":"other","kind":"lesson","text":"a ferry timetable for the docks","score":0.2,"entities":["seat:other"]}],"rank":"fused"}"#
                    } else {
                        r#"{"hits":[{"id":"oak","kind":"lesson","text":"Oak Tree on Nov 23 is the morning plan.","score":0.91,"entities":["seat:travel"],"dense_rank":1,"cosine":0.84}],"rank":"relevance"}"#
                    };
                    (200, body.to_string())
                } else if path == "/v1/status" {
                    if seen > 0 && !warm.load(Ordering::SeqCst) {
                        (500, r#"{"error":"still loading"}"#.to_string())
                    } else if seen > 0 {
                        (
                            200,
                            r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                                .to_string(),
                        )
                    } else {
                        (
                            200,
                            r#"{"embedder":{"available":true,"answering":null,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                                .to_string(),
                        )
                    }
                } else if path == "/v1/atoms" && head.starts_with("POST") {
                    (post_status, r#"{"error":"busy"}"#.to_string())
                } else {
                    (200, "ok".to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    Pack {
        url: format!("http://127.0.0.1:{port}"),
        searches,
    }
}

fn search(pack: &Pack, home: &Path, state: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", "Carrollton Nov 23 morning timing"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("LJOS_SEAT", "travel")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", &pack.url)
        .env("PACKSET_WORKSPACE", "seat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

#[test]
fn a_cold_encoder_is_asked_again_and_the_line_says_semantic() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let pack = pack(true, 200);
    let out = search(&pack, &home, &state);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{said}");
    assert!(
        pack.searches.load(Ordering::SeqCst) >= 2,
        "the cold search was not retried: {said}"
    );
    assert!(stderr.contains("ranking is semantic"), "{said}");
    assert!(
        stdout.contains("Oak Tree on Nov 23 is the morning plan."),
        "{said}"
    );
    assert!(
        !stdout.contains("ferry timetable"),
        "the word-ranked row was printed: {said}"
    );
}

/// An encoder the writer reports as not installed is not "still loading".
/// `--no-embed` leaves packset in this state, and the line is the one the
/// install notes promise.
#[test]
fn a_missing_encoder_is_unavailable_not_still_loading() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
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
                let body = if path == "/v1/search" {
                    r#"{"hits":[],"rank":"fused"}"#
                } else if path == "/v1/status" {
                    r#"{"embedder":{"enabled":true,"binary":null,"available":false,"answering":null}}"#
                } else {
                    "ok"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    let url = format!("http://127.0.0.1:{port}");
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", "lexical default"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("LJOS_SEAT", "travel")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", &url)
        .env("PACKSET_WORKSPACE", "seat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{said}");
    assert!(
        stderr.contains("ranking is lexical: encoder unavailable"),
        "{said}"
    );
    assert!(
        !stderr.contains("still loading"),
        "a missing encoder was described as loading: {said}"
    );
}

#[test]
fn a_search_that_stays_cold_says_the_rows_were_ranked_by_words() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let pack = pack(false, 200);
    let out = search(&pack, &home, &state);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{said}");
    assert!(
        stderr.contains("ranking is lexical: encoder is still loading"),
        "{said}"
    );
    assert!(
        !stdout.contains("ferry timetable"),
        "a floor score was printed as a hit: {said}"
    );
    assert!(
        stdout.contains("No hits for \"Carrollton Nov 23 morning timing\""),
        "{said}"
    );
    assert!(stdout.contains("`ljos search Carrollton`"), "{said}");
}

#[test]
fn search_says_when_a_queued_lesson_is_still_waiting() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    let queue_dir = state.join("ljos");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&queue_dir).unwrap();
    let lesson = "The Oak Tree stop is the morning plan.";
    std::fs::write(
        queue_dir.join("note-queue.jsonl"),
        format!(
            "{}\n",
            serde_json::json!({
                "issue": "",
                "text": lesson,
                "workspace": "seat",
                "at": 1,
                "kind": "lesson",
                "seat": "travel"
            })
        ),
    )
    .unwrap();
    let pack = pack(true, 503);
    let out = search(&pack, &home, &state);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("still queued"),
        "search did not say the lesson is waiting:\n{stderr}\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        stderr.contains("the lesson is safe and will be sent automatically"),
        "search did not say the queued lesson is safe:\n{stderr}"
    );
    let left = std::fs::read_to_string(queue_dir.join("note-queue.jsonl")).unwrap_or_default();
    assert!(
        left.contains(lesson),
        "the lesson left the queue without being stored:\n{left}"
    );
}

fn serve_search(search_body: &'static str, search_status: u16, atoms: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
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
                let head = String::from_utf8_lossy(&buf);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let (status, body, extra) = if path == "/v1/search" {
                    (search_status, search_body, "Retry-After: 0\r\n")
                } else if path == "/v1/atoms" {
                    (200, atoms, "")
                } else if path == "/v1/status" {
                    (
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#,
                        "",
                    )
                } else {
                    (200, "ok", "")
                };
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn search_as(url: &str, home: &Path, state: &Path, query: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", query])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("LJOS_SEAT", "dseams-paper")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "seat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

const RINGS: &str = "related work watch rings-statistics-matter";
const OWN_RINGS: &str = "The rings statistics matter note is the one to watch.";
const DISK_WATCH: &str = "Watch the disk fill before the copy.";

#[test]
fn unrelated_cross_seat_hits_are_not_a_search_answer() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let body = format!(
        r#"{{"hits":[
            {{"id":"disk","kind":"lesson","text":"{DISK_WATCH}","score":0.41,"cosine":0.41,"entities":["seat:disk-saver"]}},
            {{"id":"travel","kind":"lesson","text":"A related stop on the morning train.","score":0.40,"cosine":0.40,"entities":["seat:travel"]}},
            {{"id":"own","kind":"lesson","text":"{OWN_RINGS}","score":0.55,"cosine":0.55,"entities":["seat:dseams-paper"]}}
        ],"rank":"relevance"}}"#
    );
    let url = serve_search(Box::leak(body.into_boxed_str()), 200, r#"{"atoms":[]}"#);
    let out = search_as(&url, &home, &state, RINGS);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{said}");
    assert!(stdout.contains(OWN_RINGS), "{said}");
    assert!(!stdout.contains("disk fill"), "{said}");
    assert!(!stdout.contains("morning train"), "{said}");
    assert!(stderr.contains("ranking is semantic"), "{said}");
}

#[test]
fn a_busy_pack_still_ranks_this_seats_lesson_by_words() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let atoms = format!(
        r#"{{"atoms":[
            {{"id":"own","kind":"lesson","text":"{OWN_RINGS}","entities":["seat:dseams-paper"]}},
            {{"id":"disk","kind":"lesson","text":"{DISK_WATCH}","entities":["seat:disk-saver"]}}
        ]}}"#
    );
    let url = serve_search(
        r#"{"error":"packsetd is busy"}"#,
        503,
        Box::leak(atoms.into_boxed_str()),
    );
    let out = search_as(&url, &home, &state, RINGS);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{said}");
    assert!(stdout.contains(OWN_RINGS), "{said}");
    assert!(!stdout.contains("disk fill"), "{said}");
    assert!(
        stderr.contains("ranking is lexical"),
        "the busy pack did not fall back to words: {said}"
    );
}
