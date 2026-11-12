//! A plan is one tracker commit, and a busy pack is one short read.
//!
//! The sitting used to ask the pack once per section, then sweep after
//! the read clock was gone. A writer that answered 503 held each of those
//! calls. The sitting now reads the pack once, inside a second.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Every request is 503, with no Retry-After, so a caller that retries
/// on its own long budget waits. The paths are what the sitting asked.
fn busy_pack(paths: Arc<Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
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
            if let Some(line) = req.lines().next() {
                paths.lock().unwrap().push(line.to_string());
            }
            let body = b"busy";
            let resp = format!(
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.write_all(body);
        }
    });
    format!("http://{addr}")
}

#[test]
fn a_busy_pack_costs_one_read_and_the_sitting_stays_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let tracker = dir.path().join("tracker");
    let home = dir.path().join("home");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let log_s = log.display();
    let tracker_s = tracker.display();
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{log_s}'\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"day-0001\",\"title\":\"Ship the codec\",\"state\":\"TODO\",\"org_tags\":[]}}' ;;\n  recall) echo noted ;;\n  identity) printf '%s\\n' 'vissue test' 'root={tracker_s}' 'prefix=Software' ;;\n  note|claim|update) echo ok ;;\n  children) echo '[]' ;;\nesac\nexit 0\n"
        ),
    );
    script(
        dir.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  list) echo '[]' ;;\n  get) exit 1 ;;\n  upsert|claim) echo gen=2 ;;\nesac\nexit 0\n"
        ),
    );
    script(dir.path(), "deedar", "#!/bin/sh\necho head\nexit 0\n");
    let layout = vissue_core::Layout::new(&tracker, "Software");
    vissue_core::ops::create(&layout, "day", "Ship the codec", Default::default()).unwrap();
    let issue = vissue_core::IssueDoc::parse_file("day", &layout.project_issues_path("day"))
        .unwrap()
        .headings[0]
        .id
        .clone();
    let paths = Arc::new(Mutex::new(Vec::new()));
    let pack = busy_pack(Arc::clone(&paths));
    let path = format!("{}:/usr/bin:/bin", dir.path().display());
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["sitting", &issue])
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", dir.path().join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", &pack)
        .env("PACKSET_WORKSPACE", "seat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let took = started.elapsed();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        took < Duration::from_secs(3),
        "sitting waited {took:?}\n{said}"
    );
    assert!(out.status.success(), "{said}");
    assert_eq!(
        said.matches("memory degraded: ranked by words").count(),
        1,
        "{said}"
    );
    assert!(!said.contains("GET /v1/atoms failed"), "{said}");
    assert!(!said.contains("packsetd busy, retrying"), "{said}");
    let asked = paths.lock().unwrap().clone();
    assert!(
        asked.iter().all(|line| line.contains("/v1/atoms")),
        "the sitting asked for more than the one listing: {asked:?}\n{said}"
    );
    assert!(!asked.is_empty(), "the sitting never read the pack\n{said}");
}
