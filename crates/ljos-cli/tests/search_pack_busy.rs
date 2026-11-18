//! A packsetd that reports busy is asked again for a short time.
//!
//! The first searches answer 503 with the daemon's busy line. A writer
//! that then answers is ranked by meaning. A writer that stays busy past
//! that wait is one line, and the page is the word search. Words in a
//! hit are not a busy signal. `ljos remember` asks again, and when the
//! pack stays busy it stores nothing.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const HIT: &str = "d-SEAMS keeps the migrate video beside the cut.";
const BUSY: &str = "The memory pack was busy, so these rows are word matches; run the search again for ranked results.";
const BUSY_MISS: &str = "The memory pack was busy and no words matched.";
const WORD_QUERY: &str = "GPRidge video narration";
const BUSY_BODY: &str =
    r#"{"error":"packsetd is busy: every worker is answering and the queue is full; try again"}"#;

fn read_head(sock: &mut std::net::TcpStream) -> String {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn write_http(sock: &mut std::net::TcpStream, status: u16, body: &str, retry: bool) {
    let reason = if status == 503 {
        "Service Unavailable"
    } else {
        "OK"
    };
    let retry = if retry { "Retry-After: 1\r\n" } else { "" };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\n{retry}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(resp.as_bytes());
}

/// `busy` is how many `/v1/search` requests answer busy before the hit.
/// `usize::MAX` stays busy. Other paths answer at once. `searches` counts
/// the search requests.
fn serve(busy: usize) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let searches = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&searches);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let count = Arc::clone(&count);
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let head = read_head(&mut sock);
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                if path == "/v1/search" {
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    if n < busy {
                        write_http(&mut sock, 503, BUSY_BODY, false);
                    } else {
                        let body = format!(
                            r#"{{"hits":[{{"id":"seam","kind":"lesson","text":"{HIT}","score":0.91,"cosine":0.8,"entities":["seat:ada"]}}],"rank":"relevance"}}"#
                        );
                        write_http(&mut sock, 200, &body, false);
                    }
                } else if path == "/v1/status" {
                    write_http(
                        &mut sock,
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["relevance"]}}"#,
                        false,
                    );
                } else if path == "/v1/atoms" {
                    write_http(
                        &mut sock,
                        200,
                        r#"{"atoms":[
                            {"id":"tape-1","kind":"lesson","text":"The video catalog is on the shelf.","entities":["seat:ada"]},
                            {"id":"tape-2","kind":"lesson","text":"The video catalog is on the shelf.","entities":["seat:ada"]},
                            {"id":"tape-3","kind":"lesson","text":"The video catalog is on the shelf.","entities":["seat:ada"]},
                            {"id":"tape-4","kind":"lesson","text":"The video catalog is on the shelf.","entities":["seat:ada"]}
                        ]}"#,
                        false,
                    );
                } else {
                    write_http(&mut sock, 200, "ok", false);
                }
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), searches)
}

fn search(url: &str, home: &Path, args: &[&str]) -> (std::process::Output, Duration) {
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "seat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    (out, started.elapsed())
}

fn said(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_busy_once_writer_still_answers_by_meaning() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    let (url, searches) = serve(1);
    let (out, took) = search(&url, &home, &["search", "d-SEAMS migrate video"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(String::from_utf8_lossy(&out.stdout).contains(HIT), "{text}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(ljos_cli::FUSED_RANK_LINE),
        "{text}"
    );
    assert!(!text.contains(BUSY), "{text}");
    assert!(!text.contains("pack busy, try again"), "{text}");
    assert!(searches.load(Ordering::SeqCst) >= 2, "{text}");
    assert!(
        took < Duration::from_secs(3),
        "meaning search waited {took:?}\n{text}"
    );
}

#[test]
fn a_writer_that_stays_busy_uses_one_word_line() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    let (url, searches) = serve(usize::MAX);
    let (out, took) = search(&url, &home, &["search", WORD_QUERY]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let text = format!("{stdout}{stderr}");
    assert_eq!(out.status.code(), Some(1), "{text}");
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines, [BUSY_MISS], "{text}");
    assert!(stdout.trim().is_empty(), "{text}");
    assert!(!text.contains("pack busy, try again"), "{text}");
    assert!(
        !text.contains("the pack search did not answer in time"),
        "{text}"
    );
    assert!(!text.contains("The memory writer is busy"), "{text}");
    assert!(!text.contains("No close matches"), "{text}");
    assert!(!text.contains(BUSY), "{text}");
    assert!(searches.load(Ordering::SeqCst) > 2, "{text}");
    assert!(
        took < Duration::from_secs(4),
        "the busy wait ran past a few seconds: {took:?}\n{text}"
    );
    let (json, json_took) = search(&url, &home, &["search", "--json", WORD_QUERY]);
    let json_err = String::from_utf8_lossy(&json.stderr);
    let json_out = String::from_utf8_lossy(&json.stdout);
    assert_eq!(json.status.code(), Some(1), "{json_out}{json_err}");
    assert!(
        json_err.trim().is_empty(),
        "json search printed a second line: {json_err}"
    );
    let doc: serde_json::Value = serde_json::from_str(json_out.trim())
        .unwrap_or_else(|err| panic!("search json ({err}): {json_out}"));
    assert_eq!(doc["ranking"], "lexical", "{doc}");
    assert_eq!(doc["reason"], "memory search was busy", "{doc}");
    assert!(doc.get("reading").is_none(), "{doc}");
    assert!(
        !json_out.contains(BUSY) && !json_out.contains(BUSY_MISS),
        "{doc}"
    );
    assert!(!json_out.contains("pack busy, try again"), "{doc}");
    assert!(
        json_took < Duration::from_secs(4),
        "json search waited {json_took:?}"
    );
}

#[test]
fn words_in_a_hit_are_not_a_busy_pack() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let searches = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&searches);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let count = Arc::clone(&count);
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let head = read_head(&mut sock);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or(path);
                let body = if path == "/v1/search" {
                    count.fetch_add(1, Ordering::SeqCst);
                    r#"{"hits":[{"id":"seam","kind":"lesson","text":"packsetd is busy: every worker is answering and the queue is full; try again","score":0.91,"cosine":0.8,"entities":["seat:ada"]}],"rank":"relevance"}"#
                        .to_string()
                } else if path == "/v1/status" {
                    r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["relevance"]}}"#
                        .to_string()
                } else {
                    "ok".to_string()
                };
                write_http(&mut sock, 200, &body, false);
            });
        }
    });
    let url = format!("http://127.0.0.1:{port}");
    let (out, took) = search(&url, &home, &["search", "queue"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(ljos_cli::FUSED_RANK_LINE),
        "{text}"
    );
    assert!(text.contains("queue is full"), "{text}");
    assert!(!text.contains(BUSY), "{text}");
    assert!(!text.contains("Nothing was stored"), "{text}");
    assert_eq!(searches.load(Ordering::SeqCst), 1, "{text}");
    assert!(took < Duration::from_secs(3), "{took:?}\n{text}");
}

/// `busy` is how many `POST /v1/atoms` answers are 503 before a store.
fn serve_posts(busy: usize) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let posts = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&posts);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let count = Arc::clone(&count);
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let head = read_head(&mut sock);
                let line = head.lines().next().unwrap_or("");
                let path = line.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or(path);
                if line.starts_with("POST") && path == "/v1/atoms" {
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    if n < busy {
                        let body = r#"{"code":"busy","retry_ms":30,"error":"packsetd is busy: the write queue is full"}"#;
                        write_http(&mut sock, 503, body, false);
                    } else {
                        write_http(
                            &mut sock,
                            200,
                            r#"{"id":"atom-kiln","kind":"lesson","text":"The kiln dries slowly."}"#,
                            false,
                        );
                    }
                } else if path == "/v1/status" {
                    write_http(
                        &mut sock,
                        200,
                        r#"{"embedder":{"available":true,"answering":true}}"#,
                        false,
                    );
                } else {
                    write_http(&mut sock, 200, "ok", false);
                }
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), posts)
}

fn seat_home() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    root
}

#[test]
fn a_remember_that_was_busy_stores_the_lesson() {
    let root = seat_home();
    let home = root.path().join("home");
    let (url, posts) = serve_posts(2);
    let (out, took) = search(&url, &home, &["remember", "The kiln dries slowly."]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("The kiln dries slowly."),
        "{text}"
    );
    assert!(!text.contains("Nothing was stored"), "{text}");
    assert!(!text.contains("queued"), "{text}");
    assert!(posts.load(Ordering::SeqCst) >= 3, "{text}");
    assert!(took < Duration::from_secs(4), "{took:?}\n{text}");
}

#[test]
fn a_remember_that_stays_busy_stores_nothing() {
    let root = seat_home();
    let home = root.path().join("home");
    let (url, posts) = serve_posts(usize::MAX);
    let (out, took) = search(&url, &home, &["remember", "The kiln dries slowly."]);
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert_eq!(out.status.code(), Some(1), "{text}");
    let err = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = err.lines().filter(|line| !line.trim().is_empty()).collect();
    assert_eq!(
        lines,
        ["Nothing was stored. Run `ljos remember` again."],
        "{text}"
    );
    assert!(!text.contains("queued"), "{text}");
    let queue = home.join(".local/state/ljos/note-queue.jsonl");
    let queued = std::fs::read_to_string(&queue).unwrap_or_default();
    assert!(!queued.contains("kiln"), "{queued}");
    assert!(posts.load(Ordering::SeqCst) > 1, "{text}");
    assert!(
        took < Duration::from_secs(4),
        "remember waited {took:?}\n{text}"
    );
    assert!(
        took >= Duration::from_millis(400),
        "remember did not retry: {took:?}\n{text}"
    );
}

fn stub_vissue(bin: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(bin).unwrap();
    let path = bin.join("vissue");
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn recall(url: &str, home: &Path, issue: &str) -> (std::process::Output, Duration) {
    stub_vissue(&home.join("bin"));
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["recall", issue])
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", home.join("bin").display()),
        )
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .env(
            "PACKSET_TOKEN",
            "0e1d2c3b4a59687706f5e4d3c2b1a0900e1d2c3b4a59687706f5e4d3c2b1a090",
        )
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "seat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    (out, started.elapsed())
}

#[test]
fn a_recall_that_was_busy_still_matches_by_meaning() {
    let root = seat_home();
    let home = root.path().join("home");
    let (url, searches) = serve(2);
    let (out, took) = recall(&url, &home, "d-SEAMS");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains(HIT), "{text}");
    assert!(text.contains(ljos_cli::FUSED_RANK_LINE), "{text}");
    assert!(!text.contains(BUSY), "{text}");
    assert!(searches.load(Ordering::SeqCst) >= 3, "{text}");
    assert!(took < Duration::from_secs(4), "{took:?}\n{text}");
}

#[test]
fn a_recall_that_stays_busy_uses_one_word_line() {
    let root = seat_home();
    let home = root.path().join("home");
    let (url, searches) = serve(usize::MAX);
    let (out, took) = recall(&url, &home, "catalog");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains(BUSY), "{text}");
    assert!(!text.contains("pack busy, try again"), "{text}");
    assert!(text.contains("video catalog"), "{text}");
    assert!(searches.load(Ordering::SeqCst) > 1, "{text}");
    assert!(took < Duration::from_secs(4), "{took:?}\n{text}");
}

/// Accepts the connection, reads the request, and never writes an answer.
fn serve_silent() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let _ = read_head(&mut sock);
                thread::sleep(Duration::from_secs(30));
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn a_pack_that_accepts_and_stays_silent_uses_one_word_line() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    let url = serve_silent();
    let (out, took) = search(&url, &home, &["search", WORD_QUERY]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let text = format!("{stdout}{stderr}");
    assert_eq!(out.status.code(), Some(1), "{text}");
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines, [BUSY_MISS], "{text}");
    assert!(stdout.trim().is_empty(), "{text}");
    assert!(!text.contains("pack busy, try again"), "{text}");
    assert!(
        !text.contains("the pack search did not answer in time"),
        "{text}"
    );
    assert!(!text.contains("The memory writer is busy"), "{text}");
    assert!(!text.contains(BUSY), "{text}");
    // LJOS_SEARCH_BUSY_WAIT (2s) plus one short request (3s).
    assert!(
        took < Duration::from_secs(5),
        "a silent pack waited {took:?}, past the busy bound plus one request\n{text}"
    );
}

const LESSON: &str = "Seat the kiln door only when cold.";

/// Search stays busy. The atom list has one lesson the query names.
fn serve_lesson() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let head = read_head(&mut sock);
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                if path == "/v1/search" {
                    write_http(&mut sock, 503, BUSY_BODY, false);
                } else if path == "/v1/status" {
                    write_http(
                        &mut sock,
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true}}"#,
                        false,
                    );
                } else if path == "/v1/atoms" {
                    write_http(
                        &mut sock,
                        200,
                        r#"{"atoms":[{"id":"kiln1","kind":"lesson","text":"Seat the kiln door only when cold.","entities":["seat:ada"]}]}"#,
                        false,
                    );
                } else {
                    write_http(&mut sock, 200, "ok", false);
                }
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn search_fast(url: &str, home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("LJOS_SEARCH_BUSY_WAIT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

/// A busy pack with a word match prints one header, then the count and the row.
/// JSON keeps `ranking` and the short reason, with no prose.
#[test]
fn a_busy_pack_with_a_word_match_prints_one_line() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    let url = serve_lesson();
    let out = search_fast(&url, &home, &["search", "kiln door"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let text = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{text}");
    assert_eq!(stderr, format!("{BUSY}\n"), "{text}");
    assert_eq!(
        stdout,
        "shown 1 of 1.\nThe first row is the closest match.\n\nlesson, from an agent\n  Seat the kiln door only when cold.\n  place 1, id kiln1\n",
        "{text}"
    );
    assert!(!text.contains("pack busy, try again"), "{text}");
    assert!(!text.contains(BUSY_MISS), "{text}");
    assert!(stdout.contains(LESSON), "{text}");

    let json = search_fast(&url, &home, &["search", "--json", "kiln door"]);
    let json_err = String::from_utf8_lossy(&json.stderr);
    let json_out = String::from_utf8_lossy(&json.stdout);
    assert!(json.status.success(), "{json_out}{json_err}");
    assert!(json_err.trim().is_empty(), "{json_err}");
    let doc: serde_json::Value = serde_json::from_str(json_out.trim())
        .unwrap_or_else(|err| panic!("search json ({err}): {json_out}"));
    assert_eq!(doc["ranking"], "lexical", "{doc}");
    assert_eq!(doc["reason"], "memory search was busy", "{doc}");
    assert!(doc.get("reading").is_none(), "{doc}");
    assert_eq!(doc["hits"][0]["text"], LESSON, "{doc}");
    assert_eq!(doc["hits"][0]["id"], "kiln1", "{doc}");
    assert!(
        !json_out.contains(BUSY) && !json_out.contains(BUSY_MISS),
        "{doc}"
    );
    assert!(!json_out.contains("pack busy, try again"), "{doc}");

    let quiet = search_fast(&url, &home, &["search", "--porcelain", "kiln door"]);
    let quiet_out = String::from_utf8_lossy(&quiet.stdout).into_owned();
    let quiet_err = String::from_utf8_lossy(&quiet.stderr).into_owned();
    assert!(quiet.status.success(), "{quiet_out}{quiet_err}");
    assert!(quiet_err.is_empty(), "{quiet_err}");
    assert_eq!(
        quiet_out, "kiln1\tlesson\tSeat the kiln door only when cold.\n",
        "{quiet_out}"
    );
    assert!(!quiet_out.contains(BUSY), "{quiet_out}");
}
