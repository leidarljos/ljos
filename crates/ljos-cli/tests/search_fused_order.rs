//! The page follows the pack's fused order.
//!
//! `dense_rank` is the encoder's order. It can disagree with the fused
//! list. This seat's rows come before another seat's, and that is the
//! only move.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const MORNING: &str = "the kiln schedule is the morning plan";
const WARMUP: &str = "kiln schedule warmup restart";
const ADA: &str = "Ada kiln schedule for the docks";

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

fn fused_body() -> String {
    format!(
        r#"{{"hits":[
            {{"id":"ada","kind":"lesson","text":"{ADA}","score":90,"cosine":0.95,"dense_rank":1,"entities":["seat:ada"],"ballots":1,"of":1}},
            {{"id":"morning","kind":"lesson","text":"{MORNING}","score":80,"cosine":0.21,"dense_rank":3,"entities":["seat:travel"],"ballots":2,"of":2}},
            {{"id":"warmup","kind":"lesson","text":"{WARMUP}","score":70,"cosine":0.40,"dense_rank":2,"entities":["seat:travel"],"ballots":2,"of":2}}
        ],"rank":"fused"}}"#
    )
}

fn serve() -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = fused_body();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let hits = hits.clone();
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
                let (status, body) = if path == "/health" {
                    (200, "packsetd ok".to_string())
                } else if path == "/v1/status" {
                    (
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                            .to_string(),
                    )
                } else if path == "/v1/search" && head.contains("rank=") {
                    (
                        400,
                        r#"{"error":"rank must be fused or relevance"}"#.to_string(),
                    )
                } else if path == "/v1/search" {
                    (200, hits)
                } else if path == "/v1/atoms" {
                    (200, r#"{"atoms":[]}"#.to_string())
                } else {
                    (200, r#"{"ok":true}"#.to_string())
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
fn the_page_follows_fused_order_when_dense_rank_disagrees() {
    let seat = serve();
    let home = seat.home.path();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", "kiln", "schedule"])
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
        .env("LJOS_SEARCH_WAIT", "0")
        .env("LJOS_SEARCH_WARMUP_WAIT", "0")
        .env("NO_COLOR", "1")
        .env("LJOS_SEAT", "travel")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{said}");
    let morning = stdout.find(MORNING).expect(&said);
    let warmup = stdout.find(WARMUP).expect(&said);
    let ada = stdout.find(ADA).expect(&said);
    assert!(
        morning < warmup && warmup < ada,
        "fused order puts this seat's earlier row first:\n{stdout}"
    );
    assert!(
        stderr.contains(ljos_cli::MEANING_AND_WORDS_LINE),
        "the line should name the fused rank:\n{stderr}"
    );
    assert!(
        !stderr.contains("matched by meaning"),
        "dense rank is not the order:\n{stderr}"
    );
}
