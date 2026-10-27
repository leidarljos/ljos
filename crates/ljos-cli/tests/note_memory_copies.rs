//! `ljos note` keeps the ticket text whole and stores a memory copy the
//! pack will take. A long sentence and a third sentence are split. A copy
//! the pack refuses is not left queued. A copy that could not be sent
//! waits as the fitted text and goes on the next note.

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

fn serve(on: impl Fn(&str) -> (u16, String) + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = stream.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let mut need = 0usize;
                for line in buf[..at].split(|b| *b == b'\n') {
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
            let req = String::from_utf8_lossy(&buf).into_owned();
            let (code, body) = on(&req);
            let status = if code == 200 {
                "200 OK"
            } else {
                "400 Bad Request"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

    fn queue(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("home/.state/ljos/note-queue.jsonl"))
            .unwrap_or_default()
    }
}

fn posted_texts(seen: &Mutex<Vec<String>>) -> Vec<String> {
    seen.lock().unwrap().clone()
}

fn gate(seen: Arc<Mutex<Vec<String>>>) -> impl Fn(&str) -> (u16, String) + Send + Sync {
    move |req: &str| {
        let line = req.lines().next().unwrap_or("");
        if !line.starts_with("POST /v1/atoms") {
            return (200, "{}".into());
        }
        let body = req.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        let atom: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
        let text = atom["text"].as_str().unwrap_or("").to_string();
        if !ljos_cli::pack_copy::pack_copy_ok(&text) {
            return (
                400,
                r#"{"error":"atom has 3 sentences; one claim is at most 2"}"#.into(),
            );
        }
        seen.lock().unwrap().push(text);
        (200, r#"{"id":"atom-note","kind":"note"}"#.into())
    }
}

#[test]
fn a_long_sentence_is_stored_in_copies_and_the_ticket_keeps_it() {
    let hab = Hab::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = serve(gate(Arc::clone(&seen)));
    let long = format!("{}end", "word ".repeat(40));
    let out = hab.note(&url, long.trim());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "ljos note failed: {stdout} {out:?}");
    assert!(
        hab.calls()
            .contains(&format!("vissue note proj-aaaa {}", long.trim())),
        "the note did not reach the ticket:\n{}",
        hab.calls()
    );
    assert!(stdout.contains("noted in memory"), "{stdout}");
    let stored = posted_texts(&seen);
    assert!(!stored.is_empty(), "no memory copy was posted\n{stdout}");
    assert!(stored
        .iter()
        .all(|text| text != long.trim() && ljos_cli::pack_copy::pack_copy_ok(text)));
    assert!(!hab.queue().contains(long.trim()), "{}", hab.queue());
}

#[test]
fn three_sentences_are_stored_in_copies_and_the_ticket_keeps_them() {
    let hab = Hab::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = serve(gate(Arc::clone(&seen)));
    let three = "The parser kept the hyphen. The test pins it. The release ships it.";
    let out = hab.note(&url, three);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "ljos note failed: {stdout} {out:?}");
    assert!(
        hab.calls()
            .contains(&format!("vissue note proj-aaaa {three}")),
        "{}",
        hab.calls()
    );
    assert!(stdout.contains("noted in memory"), "{stdout}");
    let stored = posted_texts(&seen);
    assert!(stored.len() > 1, "{stored:?}\n{stdout}");
    assert!(stored
        .iter()
        .all(|text| text != three && ljos_cli::pack_copy::pack_copy_ok(text)));
    assert!(!hab.queue().contains(three), "{}", hab.queue());
}

#[test]
fn a_refused_copy_is_not_left_queued_and_the_ticket_still_gets_the_note() {
    let hab = Hab::new();
    let url = serve(|_| {
        (
            400,
            r#"{"error":"atom sentence 1 is very hard to read: 38 words, the limit is 29"}"#.into(),
        )
    });
    let long = "This single sentence runs on for well past the limit the pack sets for one memory because a weak model wrote it in one breath without a full stop anywhere along the way to the end of it";
    let out = hab.note(&url, long);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "ljos note failed: {stdout} {out:?}");
    assert!(
        hab.calls()
            .contains(&format!("vissue note proj-aaaa {long}")),
        "{}",
        hab.calls()
    );
    assert!(
        stdout.contains("warning: memory kept no copy of the note"),
        "{stdout}"
    );
    assert!(stdout.contains("very hard to read"), "{stdout}");
    assert!(stdout.contains("is on proj-aaaa's logbook"), "{stdout}");
    let queue = hab.queue();
    assert!(!queue.contains(long), "{queue}");
    assert!(queue.trim().is_empty(), "{queue}");
}

#[test]
fn a_note_the_pack_did_not_answer_waits_as_fitted_copies() {
    let hab = Hab::new();
    let long = format!("{}end", "word ".repeat(40));
    let out = hab.note("http://127.0.0.1:1", long.trim());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "ljos note failed: {stdout} {out:?}");
    assert!(
        hab.calls()
            .contains(&format!("vissue note proj-aaaa {}", long.trim())),
        "{}",
        hab.calls()
    );
    assert!(stdout.contains("memory: queued the copy"), "{stdout}");
    let queue = hab.queue();
    assert!(!queue.contains(long.trim()), "{queue}");
    assert!(!queue.trim().is_empty(), "{queue}");
    for line in queue.lines() {
        let item: serde_json::Value = serde_json::from_str(line).unwrap();
        let text = item["text"].as_str().unwrap();
        assert!(ljos_cli::pack_copy::pack_copy_ok(text), "{text}");
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = serve(gate(Arc::clone(&seen)));
    let again = hab.note(&url, "the pack is back");
    let stdout = String::from_utf8_lossy(&again.stdout);
    assert!(again.status.success(), "{stdout} {again:?}");
    assert!(hab.queue().trim().is_empty(), "{}", hab.queue());
    let stored = posted_texts(&seen);
    assert!(
        stored.iter().any(|text| text == "the pack is back"),
        "{stored:?}"
    );
    assert!(
        stored.len() > 1,
        "the queued copies were not sent: {stored:?}"
    );
}
