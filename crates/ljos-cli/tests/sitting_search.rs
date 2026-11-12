//! A pack that stalls on search does not swallow the claim.
//! The claim is printed before that read, and the sitting exits inside
//! the search budget.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Answers health, status and atom reads. `/v1/search` and `/v1/activate`
/// accept the request and then say nothing.
fn stall_search() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let searches = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&searches);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let count = Arc::clone(&count);
            thread::spawn(move || {
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
                let req = String::from_utf8_lossy(&buf);
                let line = req.lines().next().unwrap_or("");
                if line.contains("/v1/search") || line.contains("/v1/activate") {
                    count.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_secs(30));
                    return;
                }
                let body = if line.contains("/v1/atoms") {
                    r#"{"atoms":[]}"#
                } else if line.contains("/v1/status") || line.contains("/health") {
                    r#"{"embedder":{"available":false,"answering":false}}"#
                } else {
                    "{}"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    (port, searches)
}

#[test]
fn a_stalled_search_still_claims_and_exits_inside_the_budget() {
    let root = tempfile::tempdir().unwrap();
    let log = root.path().join("calls.log");
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".cursor")).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    std::fs::write(
        home.join(".cursor/mcp.json"),
        r#"{"mcpServers":{"ljos":{"command":"/no/such/ljos-mcp"}}}"#,
    )
    .unwrap();
    let tracker = root.path().join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    script(
        root.path(),
        "vissue",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"probe-sit\",\"title\":\"probe\",\"state\":\"TODO\"}}' ;;\n  recall) echo noted ;;\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\nesac\nexit 0\n",
            log.display(),
            tracker.display()
        ),
    );
    script(
        root.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"claimdag $*\" >> '{}'\ncase \"$1\" in\n  get) exit 1 ;;\nesac\necho '[]'\nexit 0\n",
            log.display()
        ),
    );
    script(
        root.path(),
        "deedar",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"deedar $*\" >> '{}'\necho head\nexit 0\n",
            log.display()
        ),
    );
    let layout = vissue_core::Layout::new(&tracker, "Software");
    vissue_core::ops::create(&layout, "probe", "probe work", Default::default()).unwrap();
    let doc =
        vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe")).unwrap();
    let issue = doc.headings[0].id.clone();
    let (port, searches) = stall_search();
    let path = format!("{}:/usr/bin:/bin", root.path().display());
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["sitting", &issue, "--verbose"])
        .current_dir(root.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", format!("http://127.0.0.1:{port}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let seen = Arc::new(Mutex::new(String::new()));
    let reading = Arc::clone(&seen);
    thread::spawn(move || {
        let mut stdout = stdout;
        let mut buf = [0u8; 1024];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => reading
                    .lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n])),
            }
        }
    });
    let started = Instant::now();
    let mut claimed_at = None;
    while started.elapsed() < Duration::from_secs(3) {
        if seen.lock().unwrap().contains("== claim") {
            claimed_at = Some(started.elapsed());
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(8) {
            let _ = child.kill();
            panic!(
                "sitting ran {:?} past the search budget; stdout:\n{}",
                started.elapsed(),
                seen.lock().unwrap()
            );
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = seen.lock().unwrap().clone();
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        claimed_at.is_some(),
        "claim was not printed before the search budget:\n{stdout}\ncalls:\n{calls}"
    );
    assert!(
        status.success(),
        "sitting refused:\n{stdout}\ncalls:\n{calls}"
    );
    assert!(
        calls.contains("claimdag claim"),
        "the issue was not claimed:\n{calls}"
    );
    assert_eq!(
        searches.load(Ordering::SeqCst),
        0,
        "a sitting reads the listing once and does not ask the stalling search:\n{stdout}"
    );
    assert!(
        !stdout.contains("ranked by words; the pack search did not answer in time"),
        "{stdout}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "sitting took {:?}",
        started.elapsed()
    );
}
