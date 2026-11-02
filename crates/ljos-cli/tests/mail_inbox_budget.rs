//! Inbox reads mail atoms only, marks them read, and gives up quickly
//! when the pack accepts a connection and then stays silent.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct State {
    messages: Vec<Value>,
    receipts: Vec<Value>,
    posts: Vec<Value>,
}

fn read_http(sock: &mut impl Read) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        match sock.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
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
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn kind_of(line: &str) -> Option<&str> {
    line.split(['?', '&', ' '])
        .find_map(|part| part.strip_prefix("kind="))
}

fn respond(sock: &mut impl Write, code: u16, body: &str) {
    let status = match code {
        200 => "200 OK",
        400 => "400 Bad Request",
        _ => "500 Internal Server Error",
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(resp.as_bytes());
}

/// A pack with two letters and a pile of lessons. An unfiltered listing
/// does not answer, which is the full scan inbox used to wait on.
fn serve_mail() -> (u16, Arc<Mutex<State>>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let state = Arc::new(Mutex::new(State {
        messages: vec![
            json!({
                "schema": "inside.atom/v1",
                "kind": "message",
                "level": "explicit",
                "text": "msg msg-inky\nfrom saddle\nto inky\n\nhello inky",
                "workspace": "seat",
                "ts": "2026-10-20T00:00:00Z",
                "entities": ["seat:saddle", "from:saddle", "to:inky", "msg:msg-inky"]
            }),
            json!({
                "schema": "inside.atom/v1",
                "kind": "message",
                "level": "explicit",
                "text": "msg msg-saddle\nfrom inky\nto saddle\n\nhello saddle",
                "workspace": "seat",
                "ts": "2026-10-21T00:00:00Z",
                "entities": ["seat:inky", "from:inky", "to:saddle", "msg:msg-saddle"]
            }),
        ],
        receipts: Vec::new(),
        posts: Vec::new(),
    }));
    let unfiltered = Arc::new(AtomicUsize::new(0));
    let held = Arc::clone(&state);
    let bare = Arc::clone(&unfiltered);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let req = read_http(&mut sock);
            let line = req.lines().next().unwrap_or("").to_string();
            if line.starts_with("GET /v1/status") || line.starts_with("GET /health") {
                respond(&mut sock, 200, r#"{"version":"0.13.0"}"#);
                continue;
            }
            if line.starts_with("GET /v1/atoms") {
                if kind_of(&line).is_none() && !line.contains("/v1/atoms/") {
                    bare.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_secs(8));
                    respond(&mut sock, 200, r#"{"atoms":[]}"#);
                    continue;
                }
                let kind = kind_of(&line).unwrap_or("");
                let guard = held.lock().expect("state");
                let atoms: Vec<&Value> = match kind {
                    "message" => guard.messages.iter().collect(),
                    "receipt" => guard.receipts.iter().collect(),
                    "group" => Vec::new(),
                    _ => Vec::new(),
                };
                let body = serde_json::json!({ "atoms": atoms }).to_string();
                drop(guard);
                respond(&mut sock, 200, &body);
                continue;
            }
            if line.starts_with("POST /v1/atoms") {
                let body = req.split("\r\n\r\n").nth(1).unwrap_or("");
                if let Ok(atom) = serde_json::from_str::<Value>(body) {
                    let mut guard = held.lock().expect("state");
                    guard.posts.push(atom.clone());
                    match atom["kind"].as_str() {
                        Some("receipt") => guard.receipts.push(atom),
                        Some("message") => guard.messages.push(atom),
                        _ => {}
                    }
                }
                respond(&mut sock, 200, r#"{"id":"posted","kind":"message"}"#);
                continue;
            }
            respond(&mut sock, 404, r#"{"error":"not found"}"#);
        }
    });
    (port, state, unfiltered)
}

/// Accepts and then stays silent. The count is accepted connections.
/// Health is the same socket: the stall is the answer, not the connect.
fn serve_stall() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let accepts = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&accepts);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            seen.fetch_add(1, Ordering::SeqCst);
            thread::spawn(move || {
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = read_http(&mut sock);
                thread::sleep(Duration::from_secs(30));
            });
        }
    });
    (port, accepts)
}

fn home() -> tempfile::TempDir {
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
    home
}

fn ljos(home: &std::path::Path, port: u16, seat: &str, args: &[&str]) -> (Output, Duration) {
    ljos_env(home, port, seat, args, None)
}

fn ljos_env(
    home: &std::path::Path,
    port: u16,
    seat: &str,
    args: &[&str],
    token: Option<&str>,
) -> (Output, Duration) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env_clear()
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.join("tmp"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PACKSET_URL", format!("http://127.0.0.1:{port}"))
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_SEAT", seat)
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_PACK_TIMEOUT_MS", "800")
        .env("LJOS_HOOK_PACK_TIMEOUT_MS", "600")
        .env("LJOS_PACK_CONNECT_MS", "200")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(token) = token {
        cmd.env("PACKSET_TOKEN", token);
    }
    let started = Instant::now();
    let out = cmd.output().expect("spawn ljos");
    (out, started.elapsed())
}

fn text_of(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn inbox_reads_mail_kinds_and_marks_them_for_that_seat() {
    let (port, state, unfiltered) = serve_mail();
    let home = home();
    let (first, took) = ljos(home.path(), port, "inky", &["inbox"]);
    let shown = text_of(&first);
    assert!(first.status.success(), "{shown}");
    assert!(
        took < Duration::from_secs(1),
        "inbox took {took:?}, which is the full listing"
    );
    assert!(shown.contains("msg-inky"), "{shown}");
    assert!(shown.contains("hello inky"), "{shown}");
    assert!(!shown.contains("msg-saddle"), "{shown}");
    assert_eq!(
        unfiltered.load(Ordering::SeqCst),
        0,
        "inbox listed every atom"
    );
    {
        let guard = state.lock().expect("state");
        assert!(
            guard.receipts.iter().any(|atom| {
                atom["entities"]
                    .as_array()
                    .is_some_and(|ents| ents.iter().any(|e| e.as_str() == Some("receipt:msg-inky")))
            }),
            "no receipt posted: {:?}",
            guard.posts
        );
    }

    let (again, again_took) = ljos(home.path(), port, "inky", &["inbox"]);
    let quiet = text_of(&again);
    assert!(again.status.success(), "{quiet}");
    assert!(again_took < Duration::from_secs(1), "{again_took:?}");
    assert!(quiet.contains("nothing unread"), "{quiet}");
    assert!(!quiet.contains("hello inky"), "{quiet}");

    let (all, _) = ljos(home.path(), port, "inky", &["inbox", "--all"]);
    let all_text = text_of(&all);
    assert!(all.status.success(), "{all_text}");
    assert!(all_text.contains("msg-inky"), "{all_text}");
    assert!(all_text.contains("read"), "{all_text}");

    let (reply, _) = ljos(home.path(), port, "inky", &["reply", "msg-inky", "got it"]);
    let replied = text_of(&reply);
    assert!(reply.status.success(), "{replied}");
    assert!(replied.contains("reply msg-inky"), "{replied}");

    let (other, _) = ljos(home.path(), port, "saddle", &["inbox"]);
    let saddle = text_of(&other);
    assert!(other.status.success(), "{saddle}");
    assert!(saddle.contains("msg-saddle"), "{saddle}");
    assert!(saddle.contains("hello saddle"), "{saddle}");
    assert_eq!(unfiltered.load(Ordering::SeqCst), 0);
}

/// The list is what the person asked for. A receipt write that sits on
/// the embedder used to spend the command budget and answer `pack busy`
/// with the mail already read.
#[test]
fn a_slow_receipt_does_not_hide_mail_the_pack_already_returned() {
    let (port, _posts) = serve_slow_post();
    let home = home();
    let (inbox, took) = ljos(home.path(), port, "inky", &["inbox"]);
    let said = text_of(&inbox);
    assert!(inbox.status.success(), "{said}");
    assert!(
        took < Duration::from_secs(3),
        "inbox waited {took:?} for a receipt"
    );
    assert!(said.contains("hello inky"), "{said}");
    assert!(!said.contains("pack busy"), "{said}");
}

/// GET answers at once. POST accepts the connection and does not answer.
fn serve_slow_post() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let posts = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&posts);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let posts = Arc::clone(&seen);
            thread::spawn(move || {
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let req = read_http(&mut sock);
                let line = req.lines().next().unwrap_or("");
                if line.starts_with("POST") {
                    posts.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_secs(5));
                    return;
                }
                let body = if line.starts_with("GET /v1/atoms") && !line.contains("/v1/atoms/") {
                    r#"{"atoms":[{"schema":"inside.atom/v1","kind":"message","level":"explicit","text":"msg msg-slow\nfrom saddle\nto inky\n\nhello inky","workspace":"seat","entities":["seat:saddle","from:saddle","to:inky","msg:msg-slow"]}]}"#
                } else {
                    r#"{"embedder":{"available":false}}"#
                };
                respond(&mut sock, 200, body);
            });
        }
    });
    (port, posts)
}

#[test]
fn a_stalled_pack_returns_inside_the_budget() {
    let (port, _) = serve_stall();
    let home = home();
    let (inbox, took) = ljos(home.path(), port, "inky", &["inbox"]);
    let said = text_of(&inbox);
    assert_eq!(inbox.status.code(), Some(75), "{said}");
    assert!(
        took < Duration::from_millis(1500),
        "inbox waited {took:?} on a silent pack"
    );
    assert!(said.contains("pack busy, try again"), "{said}");
    assert!(!said.contains("nothing unread"), "{said}");

    let (send, send_took) = ljos(home.path(), port, "inky", &["send", "saddle", "ping"]);
    let sent = text_of(&send);
    assert_eq!(send.status.code(), Some(75), "{sent}");
    assert!(
        send_took < Duration::from_millis(1500),
        "send waited {send_took:?}"
    );
    assert!(sent.contains("pack busy, try again"), "{sent}");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(["hook"])
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
        .env("LJOS_SEAT", "inky")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_HOOK_PACK_TIMEOUT_MS", "600")
        .env("LJOS_PACK_CONNECT_MS", "200")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn hook");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"stall-session-1","prompt":"hello from the stall test"}"#,
        )
        .expect("write stdin");
    let started = Instant::now();
    let out = child.wait_with_output().expect("hook");
    let hook_took = started.elapsed();
    let hook_text = text_of(&out);
    assert!(out.status.success(), "{hook_text}");
    assert!(
        hook_took < Duration::from_secs(2),
        "hook waited {hook_took:?}"
    );
    assert!(!hook_text.contains("pack busy"), "{hook_text}");
    assert!(
        !hook_text.contains("mail could not be checked"),
        "{hook_text}"
    );
}

/// A listener that accepts and never writes used to be asked again by the
/// token proof, each try on a fresh 30s budget. One command is one budget,
/// and a timeout is not started over.
#[test]
fn a_token_proof_does_not_restart_when_the_writer_stays_silent() {
    let (port, accepts) = serve_stall();
    let home = home();
    let (inbox, took) = ljos_env(home.path(), port, "inky", &["inbox"], Some("seat-token"));
    let said = text_of(&inbox);
    assert_eq!(inbox.status.code(), Some(75), "{said}");
    assert!(
        took < Duration::from_millis(1500),
        "inbox waited {took:?}; a restarted proof spends the budget again"
    );
    assert!(said.contains("pack busy, try again"), "{said}");
    let opened = accepts.load(Ordering::SeqCst);
    assert_eq!(
        opened, 1,
        "the proof opened {opened} sockets; a timeout is one attempt"
    );

    let (hook_port, hook_accepts) = serve_stall();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(["hook"])
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
        .env("PACKSET_URL", format!("http://127.0.0.1:{hook_port}"))
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TOKEN", "seat-token")
        .env("LJOS_SEAT", "inky")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_HOOK_PACK_TIMEOUT_MS", "600")
        .env("LJOS_PACK_CONNECT_MS", "200")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn hook");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"stall-token-1","prompt":"hello from the stall test"}"#,
        )
        .expect("write stdin");
    let started = Instant::now();
    let out = child.wait_with_output().expect("hook");
    let hook_took = started.elapsed();
    let hook_text = text_of(&out);
    assert!(out.status.success(), "{hook_text}");
    assert!(
        hook_took < Duration::from_secs(2),
        "hook waited {hook_took:?}"
    );
    let opened = hook_accepts.load(Ordering::SeqCst);
    assert!(
        opened <= 2,
        "the hook opened {opened} sockets on one silent writer"
    );
    assert!(!hook_text.contains("pack busy"), "{hook_text}");
}
