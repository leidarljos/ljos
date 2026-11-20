//! Transient EAGAIN retries on loopback and non-idempotent write guard.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[path = "common/seat_root.rs"]
mod seat_root;

struct RealSeatHome(PathBuf);

impl RealSeatHome {
    fn new(name: &str) -> Self {
        let dir = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp)
            .join(format!("ljos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".config")).expect("config");
        std::fs::create_dir_all(dir.join(".local/state")).expect("state");
        std::fs::create_dir_all(dir.join("run")).expect("run");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for RealSeatHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HIT: &str = "d-SEAMS keeps the migrate video beside the cut.";
const LESSON: &str = "Cold metal seats the door.";
const QUEUED_LINE: &str = "Memory busy: queued 1 write, it will be saved on a later call.";
const BUSY_MISS: &str = "The memory pack was busy and no words matched.";

fn read_request(sock: &mut std::net::TcpStream) -> (String, String) {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    let head_end = loop {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => return (String::from_utf8_lossy(&buf).into_owned(), String::new()),
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos;
                }
            }
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let len = head
        .lines()
        .find_map(|line| {
            let (name, val) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| val.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
        }
    }
    body.truncate(len);
    (head, String::from_utf8_lossy(&body).into_owned())
}

fn read_head(sock: &mut std::net::TcpStream) -> String {
    read_request(sock).0
}

fn write_http(sock: &mut std::net::TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(resp.as_bytes());
}

/// Stall `stalls` matching requests past the socket timeout so the client
/// observes EAGAIN (`WouldBlock`), then answer 200 OK.
fn serve_stalling(
    target_path: &'static str,
    stalls: usize,
    stall_for: Duration,
    ok_body: &'static str,
) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&hits);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let count = Arc::clone(&count);
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let head = read_head(&mut sock);
                let line = head.lines().next().unwrap_or("");
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                if path == target_path {
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    if n < stalls {
                        thread::sleep(stall_for);
                        return;
                    }
                    write_http(&mut sock, 200, ok_body);
                } else if path == "/v1/status" {
                    write_http(
                        &mut sock,
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["relevance"]}}"#,
                    );
                } else if path == "/v1/atoms" && line.starts_with("GET") {
                    write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                } else {
                    write_http(&mut sock, 200, "ok");
                }
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), hits)
}

fn seat_home() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    root
}

fn run_ljos_env(
    url: &str,
    home: &Path,
    args: &[&str],
    timeout_ms: &str,
    extra: &[(&str, &str)],
) -> (std::process::Output, Duration) {
    let started = Instant::now();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
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
        .env("PACKSET_TIMEOUT_MS", timeout_ms)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    (out, started.elapsed())
}

fn run_ljos(
    url: &str,
    home: &Path,
    args: &[&str],
    timeout_ms: &str,
) -> (std::process::Output, Duration) {
    run_ljos_env(url, home, args, timeout_ms, &[])
}

fn said(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn eagain_search_recovers_after_transient_stalls() {
    let root = seat_home();
    let home = root.path().join("home");
    let body = Box::leak(
        format!(
            r#"{{"hits":[{{"id":"seam","kind":"lesson","text":"{HIT}","score":0.91,"cosine":0.8,"entities":["seat:ada"],"ballots":1,"of":3}}],"rank":"relevance"}}"#
        )
        .into_boxed_str(),
    );
    let (url, calls) = serve_stalling("/v1/search", 2, Duration::from_millis(350), body);
    let (out, took) = run_ljos(&url, &home, &["search", "d-SEAMS migrate video"], "150");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(HIT),
        "search did not return ranked hit:\n{text}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(ljos_cli::MEANING_LINE),
        "search did not rank by meaning:\n{text}"
    );
    assert!(calls.load(Ordering::SeqCst) >= 3, "{text}");
    assert!(took < Duration::from_secs(4), "took {took:?}\n{text}");
}

#[test]
fn eagain_search_that_stays_busy_stops_bounded_and_prints_busy_line() {
    let root = seat_home();
    let home = root.path().join("home");
    let (url, calls) = serve_stalling("/v1/search", usize::MAX, Duration::from_millis(400), "{}");
    let (out, took) = run_ljos(&url, &home, &["search", "unrelated xyzzy query"], "150");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = format!("{stdout}{stderr}");
    assert_eq!(out.status.code(), Some(1), "{text}");
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines, [BUSY_MISS], "{text}");
    assert!(stdout.trim().is_empty(), "{text}");
    assert!(calls.load(Ordering::SeqCst) >= 2, "did not retry: {text}");
    assert!(took < Duration::from_secs(5), "took {took:?}\n{text}");
}

#[test]
fn withheld_response_post_is_never_sent_twice() {
    let root = seat_home();
    let home = root.path().join("home");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let gate_calls = Arc::new(AtomicUsize::new(0));
    let post_count = Arc::new(AtomicUsize::new(0));
    let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let gate_bg = Arc::clone(&gate_calls);
    let post_count_bg = Arc::clone(&post_count);
    let bodies_bg = Arc::clone(&bodies);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let gate_bg = Arc::clone(&gate_bg);
            let post_count_bg = Arc::clone(&post_count_bg);
            let bodies_bg = Arc::clone(&bodies_bg);
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let (head, body) = read_request(&mut sock);
                let line = head.lines().next().unwrap_or("");
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                if line.starts_with("GET /v1/atoms/") && path.len() > "/v1/atoms/".len() {
                    let n = gate_bg.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        // First pre-wire gate probe stalls past socket timeout.
                        thread::sleep(Duration::from_millis(350));
                        return;
                    }
                    write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                } else if line.starts_with("POST /v1/atoms") {
                    post_count_bg.fetch_add(1, Ordering::SeqCst);
                    bodies_bg.lock().unwrap().push(body);
                    // Processed the POST, withhold the response past socket timeout.
                    thread::sleep(Duration::from_millis(400));
                } else if line.starts_with("GET /v1/atoms") {
                    write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                } else {
                    write_http(&mut sock, 200, "ok");
                }
            });
        }
    });
    let url = format!("http://127.0.0.1:{port}");
    let (out, took) = run_ljos_env(
        &url,
        &home,
        &["remember", LESSON],
        "150",
        &[("PACKSET_AUTH", "on"), ("PACKSET_TOKEN", "probe-token")],
    );
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(QUEUED_LINE),
        "{text}"
    );
    assert!(
        gate_calls.load(Ordering::SeqCst) >= 2,
        "pre-wire gate stall did not retry:\n{text}"
    );
    assert_eq!(
        post_count.load(Ordering::SeqCst),
        1,
        "non-idempotent POST was sent more than once (bodies: {:?})\n{text}",
        *bodies.lock().unwrap()
    );
    assert!(took < Duration::from_secs(3), "took {took:?}\n{text}");
}

#[test]
fn doctor_against_stalling_writer_does_not_claim_packset_is_missing_from_path() {
    let root = seat_home();
    let home = root.path().join("home");
    let (url, _calls) = serve_stalling("/health", usize::MAX, Duration::from_millis(400), "ok");
    let (out, _took) = run_ljos(&url, &home, &["doctor"], "150");
    let text = said(&out);
    assert!(
        !text.contains("packset is not on PATH"),
        "stalling writer must not claim packset is missing from PATH:\n{text}"
    );
    assert!(
        text.contains("pack busy, try again"),
        "doctor should name busy/stall on EAGAIN:\n{text}"
    );

    // Temp HOME against a 500 on /health must not fall into BASE's
    // temp_home() branch and claim `packset is not on PATH`.
    let err_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let err_port = err_listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in err_listener.incoming() {
            let Ok(mut sock) = stream else { break };
            thread::spawn(move || {
                let (head, _) = read_request(&mut sock);
                let line = head.lines().next().unwrap_or("");
                if line.starts_with("GET /v1/atoms") {
                    write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                } else {
                    write_http(&mut sock, 500, r#"{"error":"boom"}"#);
                }
            });
        }
    });
    let err_url = format!("http://127.0.0.1:{err_port}");
    let (err_out, _) = run_ljos(&err_url, &home, &["doctor"], "150");
    let err_text = said(&err_out);
    assert!(
        !err_text.contains("packset is not on PATH"),
        "temp HOME doctor must not claim packset is missing from PATH on 500:\n{err_text}"
    );

    // When `packsetd` is on PATH without `packset`, a down writer must name
    // `start \`packsetd\`` rather than `packset is not on PATH`.
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake_daemon = bin.join("packsetd");
    std::fs::write(&fake_daemon, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_daemon, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let closed_port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let (down_out, _) = run_ljos_env(
        &format!("http://127.0.0.1:{closed_port}"),
        &home,
        &["doctor"],
        "150",
        &[("PATH", &path)],
    );
    let down_text = said(&down_out);
    assert!(
        !down_text.contains("packset is not on PATH"),
        "doctor with packsetd on PATH must not claim packset is missing:\n{down_text}"
    );
    assert!(
        down_text.contains("start `packsetd`"),
        "doctor with packsetd on PATH should advise starting packsetd:\n{down_text}"
    );
}

#[test]
fn withheld_response_prefer_is_never_sent_twice() {
    let home = RealSeatHome::new("withheld-prefer");
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let marker = home.path().join("ensure-called");
    let fake = bin.join("packset");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\n", marker.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let post_count = Arc::new(AtomicUsize::new(0));
    let post_bg = Arc::clone(&post_count);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let post_bg = Arc::clone(&post_bg);
            thread::spawn(move || {
                let (head, _body) = read_request(&mut sock);
                let line = head.lines().next().unwrap_or("");
                if line.starts_with("POST /v1/atoms") {
                    post_bg.fetch_add(1, Ordering::SeqCst);
                    // Read full body and withhold the reply past the timeout.
                    thread::sleep(Duration::from_millis(400));
                } else if line.starts_with("GET /v1/atoms") {
                    write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                } else {
                    write_http(&mut sock, 200, "ok");
                }
            });
        }
    });

    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["prefer", "Always verify the seat log before merging."])
        .env_clear()
        .env("PATH", path)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_PORT", port.to_string())
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_TIMEOUT_MS", "150")
        .env("LJOS_NO_AUTOSTART", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let text = said(&out);
    assert_eq!(
        post_count.load(Ordering::SeqCst),
        1,
        "ljos prefer must never resend POST /v1/atoms after bytes leave the wire:\n{text}"
    );
    let ensure_calls = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        ensure_calls.trim().is_empty(),
        "with_writer must not call packset ensure after a post-wire write IO error: {ensure_calls:?}\n{text}"
    );
}

#[test]
fn failed_prefer_prints_errno_once_including_verbose() {
    let root = seat_home();
    let home = root.path().join("home");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            thread::spawn(move || {
                let mut first = [0u8; 4];
                if sock.read(&mut first).is_err() {
                    return;
                }
                if &first == b"POST" {
                    // Close with unread request bytes in the socket buffer so Linux
                    // emits TCP RST -> ConnectionReset (os error 104) after write bytes
                    // leave for the wire.
                    let mut rest = [0u8; 16];
                    let _ = sock.read(&mut rest);
                    drop(sock);
                } else {
                    let head = read_head(&mut sock);
                    if head.contains("/v1/atoms") {
                        write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                    } else {
                        write_http(&mut sock, 200, "ok");
                    }
                }
            });
        }
    });

    for extra in [&[][..], &[("LJOS_DEBUG", "1")][..]] {
        let (out, _took) = run_ljos_env(
            &format!("http://127.0.0.1:{port}"),
            &home,
            &["prefer", "Always keep error lines short."],
            "150",
            extra,
        );
        let text = said(&out);
        assert!(!out.status.success(), "{text}");
        let phrase = if text.contains("Connection reset by peer") {
            "Connection reset by peer"
        } else if text.contains("Broken pipe") {
            "Broken pipe"
        } else if text.contains("the pack closed the socket") {
            "the pack closed the socket"
        } else {
            panic!("expected post-wire IO failure text in ({extra:?}):\n{text}");
        };
        assert_eq!(
            text.matches(phrase).count(),
            1,
            "failed prefer ({extra:?}) printed '{phrase}' more than once:\n{text}"
        );
    }
}

#[test]
fn queued_replay_holds_instead_of_posting_when_listing_returns_503() {
    let root = seat_home();
    let home = root.path().join("home");
    let state_ljos = home.join(".local/state/ljos");
    std::fs::create_dir_all(&state_ljos).unwrap();
    let row = serde_json::json!({
        "issue": "demo-503",
        "text": LESSON,
        "workspace": "seat",
        "at": 1_700_000_000u64,
        "kind": "lesson",
        "seat": "ada",
    });
    std::fs::write(state_ljos.join("note-queue.jsonl"), format!("{row}\n")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let listing_503 = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let post_count = Arc::new(AtomicUsize::new(0));
    let listing_bg = Arc::clone(&listing_503);
    let post_bg = Arc::clone(&post_count);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let listing_bg = Arc::clone(&listing_bg);
            let post_bg = Arc::clone(&post_bg);
            thread::spawn(move || {
                let (head, _body) = read_request(&mut sock);
                let line = head.lines().next().unwrap_or("");
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                if line.starts_with("POST /v1/atoms") {
                    post_bg.fetch_add(1, Ordering::SeqCst);
                    write_http(
                        &mut sock,
                        200,
                        r#"{"id":"atom-503","kind":"lesson","text":"Cold metal seats the door."}"#,
                    );
                } else if line.starts_with("GET /v1/atoms") && path == "/v1/atoms" {
                    if listing_bg.load(Ordering::SeqCst) {
                        write_http(&mut sock, 503, r#"{"error":"busy"}"#);
                    } else {
                        write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                    }
                } else if path == "/v1/search" {
                    write_http(&mut sock, 200, r#"{"hits":[]}"#);
                } else {
                    write_http(&mut sock, 200, "ok");
                }
            });
        }
    });

    let url = format!("http://127.0.0.1:{port}");
    let (out, _took) = run_ljos(&url, &home, &["search", "kiln"], "200");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert_eq!(
        post_count.load(Ordering::SeqCst),
        0,
        "queue replay POSTed despite 503 on duplicate-check listing:\n{text}"
    );
    let held = std::fs::read_to_string(state_ljos.join("note-queue.jsonl")).unwrap_or_default();
    assert!(
        held.contains(LESSON),
        "queued item was dropped instead of held on 503 listing:\n{held}\n{text}"
    );

    listing_503.store(false, Ordering::SeqCst);
    let (replay, _took) = run_ljos(&url, &home, &["search", "kiln"], "200");
    let replay_text = said(&replay);
    assert!(replay.status.success(), "{replay_text}");
    assert_eq!(
        post_count.load(Ordering::SeqCst),
        1,
        "queued item did not replay once listing recovered:\n{replay_text}"
    );
    let drained = std::fs::read_to_string(state_ljos.join("note-queue.jsonl")).unwrap_or_default();
    assert!(
        drained.trim().is_empty(),
        "note-queue.jsonl was not drained after recovery:\n{drained}\n{replay_text}"
    );
}

#[test]
fn concurrent_mcp_blocking_calls_do_not_leak_quiet_across_requests_or_ports() {
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let silent_addr = silent.local_addr().unwrap();
    let silent_accepts = Arc::new(AtomicUsize::new(0));
    let silent_bg = Arc::clone(&silent_accepts);
    thread::spawn(move || {
        for stream in silent.incoming() {
            let Ok(sock) = stream else { break };
            silent_bg.fetch_add(1, Ordering::SeqCst);
            thread::spawn(move || {
                let _keep = sock;
                thread::sleep(Duration::from_secs(5));
            });
        }
    });

    let healthy = TcpListener::bind("127.0.0.1:0").unwrap();
    let healthy_addr = healthy.local_addr().unwrap();
    let healthy_accepts = Arc::new(AtomicUsize::new(0));
    let healthy_bg = Arc::clone(&healthy_accepts);
    thread::spawn(move || {
        for stream in healthy.incoming() {
            let Ok(mut sock) = stream else { break };
            healthy_bg.fetch_add(1, Ordering::SeqCst);
            thread::spawn(move || {
                let _ = read_head(&mut sock);
                write_http(&mut sock, 200, r#"{"atoms":[]}"#);
            });
        }
    });

    let silent_url = format!("http://{silent_addr}");
    let healthy_url = format!("http://{healthy_addr}");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap();

    // Ensure PACKSET_AUTH=off / PACKSET_TIMEOUT_MS=150 for PacksetClient calls.
    unsafe {
        std::env::set_var("PACKSET_AUTH", "off");
        std::env::set_var("PACKSET_TIMEOUT_MS", "150");
    }

    let (req_a_done, req_b_done) = rt.block_on(async move {
        let ready = Arc::new(tokio::sync::Notify::new());
        let ready_a = Arc::clone(&ready);
        let silent_url_a = silent_url.clone();
        let healthy_url_a = healthy_url.clone();

        let fut_a = ljos_cli::tool_bound::scope("req_a", async move {
            ljos_cli::tool_bound::blocking(move || {
                let first = ljos_cli::PacksetClient::new(silent_url_a.clone())
                    .atoms_of_kind("seat", "lesson");
                assert!(first.is_err(), "silent pack must fail");
                // Request A has now marked silent_addr quiet inside its own QuietPack.
                // Wake Request B while Request A still holds its QuietPack guard.
                ready_a.notify_one();
                thread::sleep(Duration::from_millis(250));
                // Same request calling a different healthy port must still succeed.
                let same_req_other_port = ljos_cli::PacksetClient::new(healthy_url_a)
                    .atoms_of_kind("seat", "lesson");
                assert!(
                    same_req_other_port.is_ok(),
                    "quiet on silent port must not silence healthy port on same thread: {same_req_other_port:?}"
                );
                true
            })
            .await
            .unwrap()
        });

        let fut_b = ljos_cli::tool_bound::scope("req_b", async move {
            ready.notified().await;
            ljos_cli::tool_bound::blocking(move || {
                let res = ljos_cli::PacksetClient::new(healthy_url)
                    .atoms_of_kind("seat", "lesson");
                assert!(
                    res.is_ok(),
                    "concurrent MCP request to healthy pack must not fail from another request's quiet state: {res:?}"
                );
                true
            })
            .await
            .unwrap()
        });

        tokio::join!(fut_a, fut_b)
    });

    assert!(req_a_done);
    assert!(req_b_done);
    assert!(silent_accepts.load(Ordering::SeqCst) >= 1);
    assert!(
        healthy_accepts.load(Ordering::SeqCst) >= 2,
        "healthy pack should receive sockets from both requests (accepts={})",
        healthy_accepts.load(Ordering::SeqCst)
    );
}

#[test]
fn remember_recovers_from_transient_connection_refused_on_loopback() {
    let root = seat_home();
    let home = root.path().join("home");
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let posts = Arc::new(AtomicUsize::new(0));
    let posts_bg = Arc::clone(&posts);
    let bg = thread::spawn(move || {
        thread::sleep(Duration::from_millis(160));
        let listener = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let posts_bg = Arc::clone(&posts_bg);
            thread::spawn(move || {
                let (head, _body) = read_request(&mut sock);
                let line = head.lines().next().unwrap_or("");
                if line.starts_with("POST /v1/atoms") {
                    posts_bg.fetch_add(1, Ordering::SeqCst);
                    write_http(
                        &mut sock,
                        200,
                        r#"{"id":"atom-refused","kind":"lesson","text":"Cold metal seats the door."}"#,
                    );
                } else if line.starts_with("GET /v1/atoms") {
                    write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                } else {
                    write_http(&mut sock, 200, "ok");
                }
            });
        }
    });

    let url = format!("http://127.0.0.1:{port}");
    let (out, took) = run_ljos(&url, &home, &["remember", LESSON], "200");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(LESSON),
        "remember did not recover after loopback ConnectionRefused:\n{text}"
    );
    assert_eq!(posts.load(Ordering::SeqCst), 1, "{text}");
    assert!(took < Duration::from_secs(4), "took {took:?}\n{text}");
    drop(bg);
}

#[test]
fn eagain_remember_recovers_after_prewire_gate_stalls() {
    let root = seat_home();
    let home = root.path().join("home");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let gate_stalls = Arc::new(AtomicUsize::new(0));
    let post_count = Arc::new(AtomicUsize::new(0));
    let gate_bg = Arc::clone(&gate_stalls);
    let post_bg = Arc::clone(&post_count);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let gate_bg = Arc::clone(&gate_bg);
            let post_bg = Arc::clone(&post_bg);
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let (head, _body) = read_request(&mut sock);
                let line = head.lines().next().unwrap_or("");
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                if line.starts_with("GET /v1/atoms/") && path.len() > "/v1/atoms/".len() {
                    let n = gate_bg.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        thread::sleep(Duration::from_millis(350));
                        return;
                    }
                    // Bare server: no bearer needed after pre-wire gate recovers.
                    write_http(&mut sock, 200, r#"{"atoms":[]}"#);
                } else if line.starts_with("POST /v1/atoms") {
                    post_bg.fetch_add(1, Ordering::SeqCst);
                    write_http(
                        &mut sock,
                        200,
                        r#"{"id":"atom-kiln","kind":"lesson","text":"Cold metal seats the door."}"#,
                    );
                } else {
                    write_http(&mut sock, 200, "ok");
                }
            });
        }
    });
    let url = format!("http://127.0.0.1:{port}");
    let (out, took) = run_ljos_env(
        &url,
        &home,
        &["remember", LESSON],
        "150",
        &[("PACKSET_AUTH", "on"), ("PACKSET_TOKEN", "probe-token")],
    );
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(LESSON),
        "remember did not store the lesson:\n{text}"
    );
    assert!(
        !text.contains("queued"),
        "remember queued despite pre-wire recovery:\n{text}"
    );
    assert_eq!(post_count.load(Ordering::SeqCst), 1, "{text}");
    assert!(gate_stalls.load(Ordering::SeqCst) >= 3, "{text}");
    assert!(took < Duration::from_secs(4), "took {took:?}\n{text}");
}
