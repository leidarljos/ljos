//! A pack that has not finished starting is not an empty search.
//!
//! The stub answers not-ready, then ok. A search that is still starting
//! when the warm-up budget runs out ranks by words and names that.
//! A real miss, after the pack has answered, uses the ordinary miss line.
//! Words in the query, or in a hit, are not a starting signal.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const HIT: &str = "the kiln schedule is the morning plan";
const STARTING: &str =
    "Memory is starting, so try again in a few seconds. If this persists, run `ljos doctor`.";
const STARTING_WORDS: &str =
    "Memory is starting, so these results are ranked by words. If this persists, run `ljos doctor`.";
const BUSY_WORDS: &str = "The memory pack was busy, so these rows are word matches; run the search again for ranked results.";
const TRIP: &str = "pack busy try again fallback word ranking";

struct Seat {
    url: String,
    home: tempfile::TempDir,
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

/// `ready_after` is when `/health` flips from `not-ready` to `ok`, measured
/// from bind. `None` stays not-ready. `hits` is the search body once the
/// pack has opened. While it is starting, search says the pack is busy
/// loading.
fn serve(ready_after: Option<Duration>, hits: &'static str) -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let started = Instant::now();
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
                let cl = content_length(&head);
                while buf.len() < head_end + cl {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let open = ready_after.is_some_and(|delay| started.elapsed() >= delay);
                let (status, body) = if path == "/health" {
                    if open {
                        (200, "ok".to_string())
                    } else {
                        (200, "not-ready".to_string())
                    }
                } else if path == "/v1/search" {
                    if open {
                        (200, hits.to_string())
                    } else {
                        (
                            503,
                            r#"{"error":"the pack is busy while loading"}"#.to_string(),
                        )
                    }
                } else if path == "/v1/atoms" && head.starts_with("POST") {
                    if open {
                        (
                            200,
                            r#"{"id":"atom-kept","kind":"lesson","text":"kept"}"#.to_string(),
                        )
                    } else {
                        (503, r#"{"error":"not-ready"}"#.to_string())
                    }
                } else if path == "/v1/status" {
                    (
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true}}"#
                            .to_string(),
                    )
                } else if open {
                    (200, r#"{"ok":true,"atoms":[]}"#.to_string())
                } else {
                    (503, r#"{"error":"not-ready"}"#.to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    Seat {
        url: format!("http://127.0.0.1:{port}"),
        home,
    }
}

fn run(seat: &Seat, args: &[&str], env: &[(&str, &str)]) -> (Output, Duration) {
    let home = seat.home.path();
    let started = Instant::now();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .current_dir(home)
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
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
        .env("LJOS_TRACKER_GIT", "off")
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

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn hits_body() -> &'static str {
    r#"{"hits":[{"id":"mean","kind":"lesson","text":"the kiln schedule is the morning plan","score":0.91,"entities":["seat:travel"],"dense_rank":1,"cosine":0.84}],"rank":"relevance"}"#
}

#[test]
fn a_search_waits_out_not_ready_and_then_prints_the_hit() {
    let seat = serve(Some(Duration::from_millis(400)), hits_body());
    let (out, elapsed) = run(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(HIT), "{said}");
    assert!(!said.contains("Nothing matched"), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}\n{said}");
    assert!(elapsed >= Duration::from_millis(300), "{elapsed:?}");
}

#[test]
fn a_real_miss_after_the_pack_answers_uses_the_miss_line() {
    let seat = serve(
        Some(Duration::from_millis(200)),
        r#"{"hits":[],"rank":"relevance"}"#,
    );
    let (out, _) = run(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("Nothing matched"), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(HIT), "{said}");
}

#[test]
fn a_search_that_stays_not_ready_ranks_by_words() {
    let seat = serve(None, hits_body());
    let (out, elapsed) = run(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "1"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(STARTING_WORDS), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(HIT), "{said}");
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}\n{said}");
    assert!(elapsed >= Duration::from_millis(400), "{elapsed:?}");
}

#[test]
fn a_warmup_wait_of_zero_does_not_wait_on_a_starting_pack() {
    let seat = serve(Some(Duration::from_secs(30)), hits_body());
    let (out, elapsed) = run(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "0"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(STARTING_WORDS), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}\n{said}");
}

#[test]
fn remember_waits_for_the_pack_to_open() {
    let seat = serve(Some(Duration::from_millis(300)), hits_body());
    let (out, elapsed) = run(
        &seat,
        &["remember", "Cold metal seats the door."],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains("queued"), "{said}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}\n{said}");
}

#[test]
fn remember_names_a_pack_that_stays_not_ready() {
    let seat = serve(None, hits_body());
    let (out, _) = run(
        &seat,
        &["remember", "Cold metal seats the door."],
        &[("LJOS_SEARCH_WARMUP_WAIT", "1")],
    );
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(said.contains(STARTING), "{said}");
    assert!(!said.contains("queued"), "{said}");
}

#[test]
fn the_mcp_search_returns_the_starting_sentence() {
    let seat = serve(None, hits_body());
    let home = seat.home.path();
    std::env::set_var("PACKSET_URL", &seat.url);
    std::env::set_var("PACKSET_WORKSPACE", "seat");
    std::env::set_var("PACKSET_AUTH", "off");
    std::env::set_var("LJOS_NO_AUTOSTART", "1");
    std::env::set_var("LJOS_SEARCH_WARMUP_WAIT", "1");
    std::env::set_var("LJOS_SEARCH_WAIT", "0");
    std::env::set_var("HOME", home);
    std::env::set_var("LJOS_SEAT", "travel");
    let err = ljos_cli::search_ranked("kiln", 5).unwrap_err();
    let said = format!("{err:#}");
    assert!(said.contains(STARTING), "{said}");
    assert!(!said.contains("Nothing matched"), "{said}");
}

fn form_decode(raw: &str) -> String {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = &raw[i + 1..i + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn asked_query(head: &str) -> String {
    let target = head.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, rest)| rest).unwrap_or("");
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "q" {
            return form_decode(value);
        }
    }
    String::new()
}

/// `echo` puts the query, and the old trigger words, in a 200 hit.
/// `phrase` answers 503 whose error string is those words.
/// `code` answers 503 with a JSON `code` of `starting`.
#[derive(Clone, Copy)]
enum Open {
    Echo,
    Phrase,
    Code,
}

fn open_pack(kind: Open) -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let kind = kind;
            thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
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
                let (status, body) = if path == "/health" {
                    (200, "packsetd ok".to_string())
                } else if path == "/v1/status" {
                    (
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                            .to_string(),
                    )
                } else if path == "/v1/search" {
                    match kind {
                        Open::Echo => {
                            let q = asked_query(&head);
                            let text =
                                format!("{q} not ready not-ready busy loading try again starting");
                            let body = serde_json::json!({
                                "hits": [{
                                    "id": "echo",
                                    "kind": "lesson",
                                    "text": text,
                                    "score": 0.91,
                                    "entities": ["seat:travel"],
                                    "dense_rank": 1,
                                    "cosine": 0.9
                                }],
                                "rank": "relevance"
                            })
                            .to_string();
                            (200, body)
                        }
                        Open::Phrase => (
                            503,
                            format!(
                                r#"{{"error":"{TRIP} not ready not-ready busy loading try again starting"}}"#
                            ),
                        ),
                        Open::Code => (
                            503,
                            r#"{"code":"starting","ready":false,"error":"the writer is opening"}"#
                                .to_string(),
                        ),
                    }
                } else if path == "/v1/atoms" && head.starts_with("POST") {
                    (
                        200,
                        r#"{"id":"atom-kept","kind":"lesson","text":"kept"}"#.to_string(),
                    )
                } else {
                    (200, r#"{"ok":true,"atoms":[]}"#.to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    Seat {
        url: format!("http://127.0.0.1:{port}"),
        home,
    }
}

#[test]
fn a_query_that_names_the_busy_words_is_not_a_starting_pack() {
    let seat = open_pack(Open::Echo);
    let (out, elapsed) = run(
        &seat,
        &["search", TRIP],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(TRIP), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(STARTING_WORDS), "{said}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}\n{said}");
}

#[test]
fn a_query_that_says_starting_is_not_a_starting_pack() {
    let seat = open_pack(Open::Echo);
    let (out, elapsed) = run(
        &seat,
        &["search", "memory is starting"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("memory is starting"), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(STARTING_WORDS), "{said}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}\n{said}");
}

#[test]
fn an_error_string_that_quotes_the_query_is_busy_not_starting() {
    let seat = open_pack(Open::Phrase);
    let (out, elapsed) = run(
        &seat,
        &["search", TRIP],
        &[("LJOS_SEARCH_WARMUP_WAIT", "8"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains(
            "Memory is busy, so these results are ranked by words. If this persists, run `ljos doctor`."
        ),
        "{said}"
    );
    assert!(!said.contains(BUSY_WORDS), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(STARTING_WORDS), "{said}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}\n{said}");
}

#[test]
fn a_starting_code_ranks_by_words() {
    let seat = open_pack(Open::Code);
    let (out, elapsed) = run(
        &seat,
        &["search", "kiln schedule"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "1"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(STARTING_WORDS), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}\n{said}");
    assert!(elapsed >= Duration::from_millis(400), "{elapsed:?}");
}

#[test]
fn remember_keeps_a_lesson_that_names_the_busy_words() {
    let seat = open_pack(Open::Echo);
    let lesson = format!("{TRIP} not ready busy loading starting");
    let (out, elapsed) = run(
        &seat,
        &["remember", lesson.as_str()],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains("queued"), "{said}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}\n{said}");
}
