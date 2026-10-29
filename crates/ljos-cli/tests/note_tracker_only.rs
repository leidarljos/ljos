//! ljos-9c12: `ljos note` writes the ticket and nothing else.
//!
//! A working note belongs on the ticket's logbook. It used to be copied
//! into the pack as well, where it took a review clock and came back as a
//! recall. The tracker now keeps the note whole, the pack hears nothing,
//! and a pack that is down or refusing changes neither.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A pack that records every request line it is asked for and answers
/// 400 to each, so a write that still happens is both seen and loud.
fn serve(seen: Arc<Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            if let Some(line) = req.lines().next() {
                seen.lock().unwrap().push(line.to_string());
            }
            let body = r#"{"error":"the pack should not have been asked"}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

struct Hab {
    dir: tempfile::TempDir,
}

impl Hab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        script(
            dir.path(),
            "vissue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\nexit 0\n",
                log.display()
            ),
        );
        script(dir.path(), "claimdag", "#!/bin/sh\nexit 1\n");
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self { dir }
    }

    fn note(&self, pack: &str, text: &str) -> std::process::Output {
        let path = format!(
            "{}:{}",
            self.dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["note", "proj-aaaa", text])
            .env_clear()
            .env("PATH", &path)
            .env("HOME", self.dir.path().join("home"))
            .env(
                "XDG_CONFIG_HOME",
                self.dir.path().join("home").join(".config"),
            )
            .env(
                "XDG_STATE_HOME",
                self.dir.path().join("home").join(".state"),
            )
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .env("PACKSET_URL", pack)
            .output()
            .unwrap()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("calls.log")).unwrap_or_default()
    }

    fn state(&self) -> std::path::PathBuf {
        self.dir.path().join("home/.state/ljos")
    }
}

/// The note reaches the ticket word for word, and the pack is never asked
/// to keep a copy of it.
#[test]
fn a_note_goes_to_the_ticket_and_not_to_memory() {
    let hab = Hab::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = serve(Arc::clone(&seen));
    let text = "The parser kept the hyphen. The test pins it. The release ships it.";
    let out = hab.note(&url, text);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "ljos note failed: {stdout}{stderr}");
    assert!(
        hab.calls()
            .contains(&format!("vissue note proj-aaaa {text}")),
        "the note did not reach the ticket:\n{}",
        hab.calls()
    );
    let asked = seen.lock().unwrap().clone();
    assert!(
        !asked.iter().any(|line| line.contains("POST /v1/atoms")),
        "the note was written to the pack: {asked:?}"
    );
    assert!(
        !stdout.contains("noted in memory") && !stdout.contains("memory kept no copy"),
        "the note still talks about memory:\n{stdout}"
    );
}

/// No queue, no retry, no leftover state: the tracker took the note and
/// there is nothing else owed.
#[test]
fn a_pack_that_is_down_changes_nothing_about_a_note() {
    let hab = Hab::new();
    let long = format!("{}end", "word ".repeat(40));
    let out = hab.note("http://127.0.0.1:1", long.trim());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "ljos note failed: {stdout}");
    assert!(
        hab.calls()
            .contains(&format!("vissue note proj-aaaa {}", long.trim())),
        "{}",
        hab.calls()
    );
    assert!(
        !hab.state().join("note-queue.jsonl").exists(),
        "a note was left queued for memory"
    );
    assert!(!stdout.contains("queued the copy"), "{stdout}");
}
