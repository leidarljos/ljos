//! A warming packsetd is up.
//!
//! `/health` is `packsetd warming`, with `loading` and `eta_s`. Search
//! already answers. ljos does not restart the daemon and does not say
//! memory is starting. Doctor shows warming and the ETA.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const HIT: &str = "the kiln schedule is the morning plan";
const STARTING: &str = "Memory is starting, so try again in a few seconds.";
const HEALTH: &str = "packsetd warming\nloading embedder\neta_s 4.5\n";

struct Seat {
    url: String,
    home: tempfile::TempDir,
    ensures: Arc<AtomicUsize>,
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

fn serve() -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let ensures = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&ensures);
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let count = Arc::clone(&count);
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
                if path == "/health" {
                    count.fetch_add(1, Ordering::SeqCst);
                }
                let body = if path == "/health" {
                    HEALTH.to_string()
                } else if path == "/v1/status" {
                    r#"{"embedder":{"available":true,"answering":true,"enabled":true},"live":1,"live_cap":0}"#
                        .to_string()
                } else if path == "/v1/search" {
                    format!(
                        r#"{{"hits":[{{"id":"kiln","kind":"lesson","text":"{HIT}","score":80,"entities":["seat:travel"],"ballots":2,"of":2}}],"rank":"fused","warming":["embedder"]}}"#
                    )
                } else if path == "/v1/atoms" && head.starts_with("POST") {
                    r#"{"id":"kept","kind":"lesson","text":"kept"}"#.to_string()
                } else if path == "/v1/atoms" {
                    r#"{"atoms":[]}"#.to_string()
                } else if path == "/stats" {
                    r#"{"ok":true,"line":"ok"}"#.to_string()
                } else {
                    r#"{"ok":true}"#.to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    Seat {
        url: format!("http://127.0.0.1:{port}"),
        home,
        ensures,
    }
}

fn run(seat: &Seat, args: &[&str]) -> std::process::Output {
    let home = seat.home.path();
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
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
        .unwrap()
}

#[test]
fn a_warming_daemon_stays_up_for_search_remember_and_doctor() {
    let seat = serve();
    let search = run(&seat, &["search", "kiln", "schedule"]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&search.stdout),
        String::from_utf8_lossy(&search.stderr)
    );
    assert!(search.status.success(), "{said}");
    assert!(said.contains(HIT), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(
        seat.ensures.load(Ordering::SeqCst) >= 1,
        "search did not read /health"
    );

    let remembered = run(&seat, &["remember", "Cold metal seats the kiln."]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&remembered.stdout),
        String::from_utf8_lossy(&remembered.stderr)
    );
    assert!(remembered.status.success(), "{said}");
    assert!(!said.contains(STARTING), "{said}");

    let doctor = run(&seat, &["doctor"]);
    let page = format!(
        "{}{}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    assert!(
        page.contains("warming\tmemory\t") && page.contains("about 4.5 s"),
        "{page}"
    );
    assert!(!page.contains(STARTING), "{page}");
    assert!(
        !page.contains("packset ensure"),
        "a warming daemon is not started again:\n{page}"
    );
}
