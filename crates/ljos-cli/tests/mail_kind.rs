//! `ljos inbox` exits non-zero when the writer cannot keep a message.
//!
//! An empty message is a tool dump on every packset, refused before the
//! kind, so a stub that answers that for an empty body and `unknown atom
//! kind: message` for any other body is an old pack with no version. The
//! inbox must name the refusal. It must not print an empty inbox.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

struct Hits {
    empty: usize,
    refused: usize,
}

fn read_http(sock: &mut impl Read) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = sock.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = &buf[..at];
            let mut need = 0usize;
            for line in headers.split(|b| *b == b'\n') {
                let line = std::str::from_utf8(line).unwrap_or("").trim();
                if let Some(v) = line
                    .split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .map(|(_, v)| v.trim())
                {
                    need = v.parse().unwrap_or(0);
                }
            }
            if buf.len().saturating_sub(at + 4) >= need {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// A writer with no version. An empty message is a tool dump. Any other
/// message is refused as an unknown kind, which is what packset 0.12.1
/// answers a real letter.
fn serve_old() -> (u16, Arc<Mutex<Hits>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits = Arc::new(Mutex::new(Hits {
        empty: 0,
        refused: 0,
    }));
    let seen = Arc::clone(&hits);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            sock.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let req = read_http(&mut sock);
            let line = req.lines().next().unwrap_or("");
            let (code, body): (u16, &str) =
                if line.starts_with("GET /v1/status") || line.starts_with("GET /v1/atoms") {
                    if line.starts_with("GET /v1/atoms") {
                        (200, r#"{"atoms":[]}"#)
                    } else {
                        (200, "{}")
                    }
                } else if line.starts_with("POST /v1/atoms") {
                    let mut hits = seen.lock().expect("hits");
                    if req.contains("\"text\":\"\"") {
                        hits.empty += 1;
                        (400, r#"{"error":"tool dump is attach, not an atom"}"#)
                    } else {
                        hits.refused += 1;
                        (400, r#"{"error":"unknown atom kind: message"}"#)
                    }
                } else {
                    (404, r#"{"error":"not found"}"#)
                };
            let status = if code == 200 {
                "200 OK"
            } else {
                "400 Bad Request"
            };
            let status = if code == 404 { "404 Not Found" } else { status };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    (port, hits)
}

fn inbox(port: u16) -> Output {
    let home = tempfile::tempdir().expect("home");
    for sub in [
        "tmp",
        ".config",
        ".local/share",
        ".local/state",
        ".cache",
        "run",
    ] {
        std::fs::create_dir_all(home.path().join(sub)).expect("dirs");
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(["inbox"])
        .env_clear()
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.path().join("tmp"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("PACKSET_URL", format!("http://127.0.0.1:{port}"))
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("LJOS_SEAT", "inky")
        .env("LJOS_TRACKER_GIT", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().expect("spawn ljos");
    // Keep the home until the child has exited. `output` waits.
    drop(home);
    out
}

#[test]
fn inbox_exits_when_the_pack_rejects_kind_message() {
    let (port, hits) = serve_old();
    let out = inbox(port);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !out.status.success(),
        "inbox exited {:?}: {said}",
        out.status.code()
    );
    assert!(said.contains("does not keep mail"), "{said}");
    assert!(said.contains("kind message"), "{said}");
    assert!(said.contains("0.13.0"), "{said}");
    assert!(
        !said.contains("nothing unread"),
        "an empty inbox hides the refusal: {said}"
    );
    let hits = hits.lock().expect("hits");
    assert_eq!(hits.empty, 0, "an empty probe cannot see the kind");
    assert_eq!(hits.refused, 1, "the kind was asked once");
}
