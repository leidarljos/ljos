//! A busy pack queues a lesson instead of dropping it.
//!
//! `ljos finish --lesson` leaves one queue row and one plain sentence.
//! `ljos note` and `ljos file` keep the tracker write and leave that row
//! where it is. `ljos sitting` says the recall is word matches. A later
//! call whose pack answers sends the row once, and a second send of the
//! same row does not store it again.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LESSON: &str = "Cold metal seats the door.";
const QUEUED: &str = "Memory busy: queued 1 write. A later ljos call from any seat will try to save it, and the pack may still refuse it.";
const WORDS: &str =
    "Memory is busy, so these results are ranked by words. If this persists, run `ljos doctor`.";

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    ljos_cli::install_executable(&path, body).unwrap();
}

/// Every request is 503, with no Retry-After, so a caller does not sleep.
fn busy_pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
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

/// A pack that stores each new lesson once and lists what it stored.
/// A post of a claim it holds, same text, kind, set, issue and agent, is
/// answered with that claim, as packsetd answers it. `posts` counts the
/// claims stored.
fn healthy_pack(posts: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stored = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let stored = Arc::clone(&stored);
            let posts = Arc::clone(&posts);
            let Ok(mut sock) = stream else { break };
            std::thread::spawn(move || {
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                let head = String::from_utf8_lossy(&buf[..at]);
                                let len = head.lines().find_map(|line| {
                                    line.split_once(':')
                                        .filter(|(name, _)| {
                                            name.eq_ignore_ascii_case("content-length")
                                        })
                                        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                                });
                                let have = buf.len() - (at + 4);
                                if len.is_none_or(|len| have >= len) {
                                    break;
                                }
                            }
                        }
                    }
                }
                let req = String::from_utf8_lossy(&buf);
                let line = req.lines().next().unwrap_or("");
                let body = if line.starts_with("POST") && line.contains("/v1/atoms") {
                    let raw = req.split("\r\n\r\n").nth(1).unwrap_or("");
                    let mut atom: serde_json::Value =
                        serde_json::from_str(raw).unwrap_or(serde_json::json!({}));
                    let mut all = stored.lock().unwrap();
                    let same = |held: &&serde_json::Value| {
                        ["text", "kind", "set", "issue", "agent"]
                            .iter()
                            .all(|key| held.get(*key) == atom.get(*key))
                    };
                    if let Some(held) = all.iter().find(same) {
                        held.to_string()
                    } else {
                        posts.fetch_add(1, Ordering::SeqCst);
                        if atom.get("id").and_then(|id| id.as_str()).is_none() {
                            atom["id"] = serde_json::json!("atom-kept");
                        }
                        all.push(atom.clone());
                        atom.to_string()
                    }
                } else if line.contains("/v1/atoms") {
                    let atoms = stored.lock().unwrap().clone();
                    serde_json::json!({"atoms": atoms}).to_string()
                } else if line.contains("/v1/search") {
                    r#"{"hits":[]}"#.to_string()
                } else {
                    "{}".to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

struct Seat {
    root: tempfile::TempDir,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let tracker = root.path().join("tracker");
        let project = tracker.join("Software/demo");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(root.path().join("state")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            project.join("issues.org"),
            "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#B] Probe the kiln
:PROPERTIES:
:ID:         demo-busy
:CLAIMED_BY: ada
:CREATED:    [2026-06-01 Mon]
:END:
",
        )
        .unwrap();
        let log = root.path().join("calls.log");
        let tracker_s = tracker.display().to_string();
        script(
            root.path(),
            "vissue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\ncase \"$1\" in\n  identity) printf '%s\\n' 'vissue test' 'root={tracker_s}' 'prefix=Software' ;;\n  create) printf '%s\\n' 'project: demo' 'demo-new  TODO  [#C]  filed' ;;\n  show) printf '%s\\n' '{{\"id\":\"demo-busy\",\"title\":\"Probe the kiln\",\"state\":\"STARTED\",\"claimed_by\":\"ada\"}}' ;;\n  recall) echo noted ;;\n  note|claim|update) echo ok ;;\n  children) echo '[]' ;;\nesac\nexit 0\n",
                log.display(),
            ),
        );
        script(
            root.path(),
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  list) echo '[]' ;;\n  *) echo gen=2 ;;\nesac\nexit 0\n",
        );
        script(root.path(), "deedar", "#!/bin/sh\necho head\nexit 0\n");
        Self { root }
    }

    fn run(&self, pack: &str, args: &[&str]) -> std::process::Output {
        let home = self.root.path().join("home");
        let path = format!("{}:/usr/bin:/bin", self.root.path().display());
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", pack)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
    }

    fn queue(&self) -> String {
        std::fs::read_to_string(self.root.path().join("state/ljos/note-queue.jsonl"))
            .unwrap_or_default()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.root.path().join("calls.log")).unwrap_or_default()
    }
}

fn text_of(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn queued_lesson_count(queue: &str) -> usize {
    queue
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|row| row["text"] == LESSON)
        .count()
}

#[test]
fn a_busy_pack_queues_the_lesson_and_a_healthy_pack_saves_it_once() {
    let seat = Seat::open();
    let busy = busy_pack();

    let finished = seat.run(&busy, &["finish", "demo-busy", "--lesson", LESSON]);
    let said = text_of(&finished);
    assert!(finished.status.success(), "{said}");
    assert!(
        said.contains("sitting ended; ticket demo-busy is still open"),
        "{said}"
    );
    assert!(said.contains(QUEUED), "{said}");
    assert!(!said.contains("memory skipped"), "{said}");
    let queued = seat.queue();
    assert!(
        queued.contains(LESSON),
        "finish left no queue row:\n{queued}"
    );
    assert!(queued.contains("\"kind\":\"lesson\""), "{queued}");

    let noted = seat.run(
        &busy,
        &["note", "demo-busy", "the door seats when it is cold"],
    );
    let said = text_of(&noted);
    assert!(noted.status.success(), "{said}");
    assert!(
        seat.calls().contains("the door seats when it is cold"),
        "note did not write the tracker:\n{}",
        seat.calls()
    );
    assert!(
        !said.contains("memory skipped"),
        "a note talked about skipped memory:\n{said}"
    );
    assert_eq!(
        queued_lesson_count(&seat.queue()),
        1,
        "note changed the queued lesson:\n{}",
        seat.queue()
    );

    let filed = seat.run(&busy, &["file", "--id-only", "File the door", "-p", "demo"]);
    let said = text_of(&filed);
    assert!(filed.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&filed.stdout);
    assert_eq!(
        stdout.lines().next().unwrap_or("").trim(),
        "demo-new",
        "{said}"
    );
    assert_eq!(
        queued_lesson_count(&seat.queue()),
        1,
        "file changed the queued lesson:\n{}",
        seat.queue()
    );

    let sat = seat.run(&busy, &["sitting", "demo-busy"]);
    let said = text_of(&sat);
    assert!(sat.status.success(), "{said}");
    assert!(said.contains(WORDS), "{said}");
    assert!(!said.contains("memory skipped"), "{said}");
    assert!(
        seat.calls().contains("vissue claim demo-busy"),
        "sitting did not stamp the tracker:\n{}",
        seat.calls()
    );
    let waiting = seat.queue();
    assert_eq!(
        queued_lesson_count(&waiting),
        1,
        "sitting dropped the queued lesson:\n{waiting}\n{said}"
    );

    let posts = Arc::new(AtomicUsize::new(0));
    let healthy = healthy_pack(Arc::clone(&posts));
    let search = seat.run(&healthy, &["search", "metal"]);
    let said = text_of(&search);
    assert!(search.status.success(), "{said}");
    assert_eq!(
        posts.load(Ordering::SeqCst),
        1,
        "replay did not save once:\n{said}"
    );
    assert!(
        !seat.queue().contains(LESSON),
        "the queue still holds the lesson:\n{}\n{said}",
        seat.queue()
    );

    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        seat.root.path().join("state/ljos/note-queue.jsonl"),
        &waiting,
    )
    .unwrap();
    let again = seat.run(&healthy, &["search", "metal"]);
    let said = text_of(&again);
    assert!(again.status.success(), "{said}");
    assert_eq!(
        posts.load(Ordering::SeqCst),
        1,
        "replaying the same lesson stored it twice:\n{said}"
    );
    assert!(
        !seat.queue().contains(LESSON),
        "the second replay left the lesson queued:\n{}\n{said}",
        seat.queue()
    );
}
