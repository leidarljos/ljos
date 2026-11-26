//! A search spends one start budget, not one per wait.
//!
//! The stub's `/health` opens after a while and its search stays loading
//! throughout. The wait on `/health` and the wait on the search share the
//! `LJOS_SEARCH_WARMUP_WAIT` budget, so the last search request lands one
//! budget after the first probe, not one budget after `/health` opened.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// `/health` stays `not-ready` for this many polls, then opens.
/// The client's own pauses during those polls spend part of the one
/// warmup budget, so the searches that follow are fewer than a second
/// budget would allow.
const CLOSED_HEALTHS: usize = 5;
const BUDGET_SECS: &str = "2";

fn request_path(sock: &mut std::net::TcpStream) -> Option<String> {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let path = head.split_whitespace().nth(1)?.to_string();
    Some(path.split('?').next().unwrap_or("/").to_string())
}

#[test]
fn the_health_wait_and_the_search_wait_share_one_budget() {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let began = Instant::now();
    let probes: Arc<Mutex<Vec<(String, Duration)>>> = Arc::default();
    let seen = Arc::clone(&probes);
    let healths = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let seen = Arc::clone(&seen);
            let healths = Arc::clone(&healths);
            thread::spawn(move || {
                let mut sock = conn;
                let Some(path) = request_path(&mut sock) else {
                    return;
                };
                let at = began.elapsed();
                seen.lock().unwrap().push((path.clone(), at));
                let open = if path == "/health" {
                    healths.fetch_add(1, Ordering::SeqCst) >= CLOSED_HEALTHS
                } else {
                    healths.load(Ordering::SeqCst) >= CLOSED_HEALTHS
                };
                let loading = (
                    503,
                    r#"{"error":"the pack is busy while loading"}"#.to_string(),
                );
                let (status, body) = if path == "/v1/search" || !open {
                    if path == "/health" {
                        (200, "not-ready".to_string())
                    } else {
                        loading
                    }
                } else if path == "/health" {
                    (200, "ok".to_string())
                } else {
                    (200, r#"{"atoms":[]}"#.to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    let root = home.path();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", "kiln"])
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("TMPDIR", root)
        .env("PACKSET_URL", &url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_SEARCH_WARMUP_WAIT", BUDGET_SECS)
        .env("LJOS_SEARCH_WAIT", "0")
        .env("NO_COLOR", "1")
        .env("LJOS_SEAT", "travel")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the search did not finish within 20s");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }
    let probes = probes.lock().unwrap().clone();
    let searches = probes
        .iter()
        .filter(|(path, _)| path == "/v1/search")
        .count();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(
        (1..=8).contains(&searches),
        "one warmup budget of {BUDGET_SECS}s fits 7 searches after {CLOSED_HEALTHS} closed health polls; saw {searches}\nprobes: {probes:?}\n{said}"
    );
}
