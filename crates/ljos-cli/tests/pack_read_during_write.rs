//! One process holds a pack write. Sittings and searches started while
//! that write is in progress still read the snapshot, inside the recall
//! budget, and do not fail with `pack busy`.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const LESSON: &str = "the pack keeps a lesson about ferrule";

/// While a POST is held, search answers 503 and the atom snapshot answers
/// at once. Other reads answer at once. Every POST, including the sitting's
/// review sweep, stays open for the unarmed pack budget. The listing has
/// to be on the pipe before that sweep can return.
fn serve_held_write() -> (u16, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let held = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&held);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let flag = Arc::clone(&flag);
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
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
                let lesson = format!(
                    r#"{{"atoms":[{{"id":"lesson-ferrule","kind":"lesson","text":"{LESSON}","origin":"user-declared","ts":"2026-11-02T22:00:00.000Z","source":{{"seat":"ada"}}}}]}}"#
                );
                let (status, body) = if line.starts_with("POST ") {
                    flag.store(true, Ordering::SeqCst);
                    thread::sleep(Duration::from_secs(30));
                    ("200 OK", "{}".to_string())
                } else if line.contains("/v1/search") {
                    (
                        "503 Service Unavailable",
                        r#"{"error":"every worker is answering"}"#.to_string(),
                    )
                } else if line.contains("/v1/atoms") && !line.contains("/v1/atoms/") {
                    ("200 OK", lesson)
                } else {
                    (
                        "200 OK",
                        r#"{"embedder":{"available":false,"answering":false}}"#.to_string(),
                    )
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    (port, held)
}

fn hold_write(port: u16) -> std::process::Child {
    Command::new("python3")
        .arg("-c")
        .arg(format!(
            "import socket\n\
             s = socket.create_connection(('127.0.0.1', {port}))\n\
             body = b'{{\"workspace\":\"seat\",\"text\":\"a long write\"}}'\n\
             head = 'POST /v1/atoms HTTP/1.1\\r\\nHost: 127.0.0.1:{port}\\r\\nContent-Type: application/json\\r\\nContent-Length: %d\\r\\nConnection: close\\r\\n\\r\\n' % len(body)\n\
             s.sendall(head.encode() + body)\n\
             s.settimeout(40)\n\
             \n\
             try:\n\
             \t s.recv(4096)\n\
             except Exception:\n\
             \t pass\n"
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

struct Reader {
    child: std::process::Child,
    started: Instant,
    stdout: Arc<std::sync::Mutex<String>>,
    stderr: Arc<std::sync::Mutex<String>>,
    out_thread: thread::JoinHandle<()>,
    err_thread: thread::JoinHandle<()>,
}

fn spawn_reader(cmd: Command) -> Reader {
    let mut cmd = cmd;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = Arc::new(std::sync::Mutex::new(String::new()));
    let err = Arc::new(std::sync::Mutex::new(String::new()));
    let out_slot = Arc::clone(&out);
    let err_slot = Arc::clone(&err);
    let out_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut buf = [0u8; 1024];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => out_slot
                    .lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n])),
            }
        }
    });
    let err_thread = thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = [0u8; 1024];
        loop {
            match stderr.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => err_slot
                    .lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n])),
            }
        }
    });
    Reader {
        child,
        started: Instant::now(),
        stdout: out,
        stderr: err,
        out_thread,
        err_thread,
    }
}

/// The same bound `a_stalled_search_still_claims_and_exits_inside_the_budget`
/// uses, so a sitting can claim and still finish the recall clock. The held
/// POST lasts the unarmed pack budget, which is longer, so a sweep that
/// still ran first would hide the lesson past this bound.
fn recall_bound() -> Duration {
    Duration::from_secs(8)
}

/// A sitting's listing is visible while its sweep is still blocked. A
/// search has to exit: it does not sweep.
fn watch(mut reader: Reader, what: &str, sitting: bool) -> (bool, String, String, Duration) {
    let bound = recall_bound();
    let (mut status, took) = loop {
        let stdout = reader.stdout.lock().unwrap().clone();
        let seen = sitting && stdout.contains(LESSON);
        let status = reader.child.try_wait().unwrap();
        if seen || status.is_some() || reader.started.elapsed() > bound {
            break (status, reader.started.elapsed());
        }
        thread::sleep(Duration::from_millis(20));
    };
    if status.is_none() {
        let _ = reader.child.kill();
        status = Some(reader.child.wait().unwrap());
    }
    let _ = reader.out_thread.join();
    let _ = reader.err_thread.join();
    let stdout = reader.stdout.lock().unwrap().clone();
    let stderr = reader.stderr.lock().unwrap().clone();
    if !sitting && status.is_none_or(|code| !code.success()) && took > bound {
        panic!("{what} ran past the recall budget:\n{stdout}{stderr}");
    }
    if sitting && !stdout.contains(LESSON) {
        panic!("{what} did not list the lesson inside the recall budget:\n{stdout}{stderr}");
    }
    (
        status.is_some_and(|code| code.success()) || (sitting && stdout.contains(LESSON)),
        stdout,
        stderr,
        took,
    )
}

#[test]
fn a_held_write_does_not_fail_readers() {
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
            "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"probe-sit\",\"title\":\"ferrule on the press\",\"state\":\"TODO\"}}' ;;\n  recall) echo noted ;;\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\nesac\nexit 0\n",
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
    let mut issues = Vec::new();
    for title in ["ferrule on the press", "ferrule on the jig"] {
        vissue_core::ops::create(&layout, "probe", title, Default::default()).unwrap();
        let doc = vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe"))
            .unwrap();
        issues.push(doc.headings.last().unwrap().id.clone());
    }
    let (port, held) = serve_held_write();
    let mut writer = hold_write(port);
    let started = Instant::now();
    while !held.load(Ordering::SeqCst) {
        if started.elapsed() > Duration::from_secs(3) {
            let _ = writer.kill();
            panic!("the held write never reached the pack");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let path = format!("{}:/usr/bin:/bin", root.path().display());
    let ljos = |args: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(root.path())
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_RUNTIME_DIR", root.path().join("run"))
            .env("VISSUE_ROOT", &tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_URL", format!("http://127.0.0.1:{port}"));
        cmd
    };
    let mut readers = Vec::new();
    for issue in &issues {
        readers.push((
            format!("sitting {issue}"),
            spawn_reader(ljos(&["sitting", issue, "--verbose"])),
        ));
    }
    for _ in 0..2 {
        readers.push((
            "search".to_string(),
            spawn_reader(ljos(&["search", "ferrule"])),
        ));
    }
    for (what, reader) in readers {
        let sitting = what.starts_with("sitting");
        let (ok, stdout, stderr, took) = watch(reader, &what, sitting);
        let said = format!("{stdout}{stderr}");
        assert!(ok, "{what} failed after {took:?}:\n{said}");
        assert!(
            took < recall_bound(),
            "{what} took {took:?}, past the recall budget:\n{said}"
        );
        assert!(!said.contains("pack busy"), "{what}:\n{said}");
        assert!(held.load(Ordering::SeqCst), "{what} waited out the write");
        if sitting {
            assert!(
                stdout.contains(LESSON),
                "{what} did not recall the lesson:\n{said}"
            );
        } else {
            assert!(
                stderr.contains("ranked by words; the pack search did not answer in time"),
                "{what}:\n{said}"
            );
            assert!(
                stdout.contains(LESSON),
                "{what} did not rank the snapshot:\n{said}"
            );
        }
    }
    let _ = writer.kill();
    let _ = writer.wait();
}
