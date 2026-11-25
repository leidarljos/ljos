//! ljos-9c12: `ljos note` writes the ticket and nothing else.
//!
//! A working note belongs on the ticket's logbook. It used to be copied
//! into the pack as well, where it took a review clock and came back as a
//! recall. The tracker now keeps the note whole, the pack hears nothing,
//! and a pack that is down or refusing changes neither.

ljos_test_boot::harness!();
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    ljos_cli::install_executable(&path, body).unwrap();
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
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
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
            .unwrap();
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
    assert_eq!(
        stdout.matches("proj-aaaa: noted").count(),
        1,
        "the confirmation should be one line:\n{stdout}{stderr}"
    );
    assert!(
        !stdout.contains("noted on "),
        "a second confirmation followed the note:\n{stdout}"
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

/// ljos-yjho: a pasted payload is not a note. Every note rewrites the
/// ticket's file in git, so a snapshot line of tens of kilobytes was paid
/// for again on every later write. The command exits non-zero, the note
/// never reaches the tracker, and the line names the size, the cap, and
/// `ljos note ID --file`.
#[test]
fn a_note_past_the_cap_never_reaches_the_ticket() {
    let hab = Hab::new();
    let payload = format!("snapshot b64z:{}", "QUJD".repeat(5_000));
    let out = hab.note("http://127.0.0.1:1", &payload);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
    assert!(
        !hab.calls().contains("vissue note"),
        "the payload reached the ticket:\n{}",
        hab.calls()
    );
    let said = format!("{stdout}{stderr}");
    assert!(
        said.contains(&payload.len().to_string()) && said.contains("4096"),
        "the line does not name the size and the cap:\n{said}"
    );
    assert!(
        said.contains("ljos note proj-aaaa --file PATH") && said.contains("Nothing was noted"),
        "the line does not name the remedy:\n{said}"
    );
    assert!(!said.contains("label oversize:"), "{said}");
    assert!(!stdout.contains("proj-aaaa: noted"), "{stdout}");

    let lines = format!("{}\n{}", "short line", "x".repeat(3_000));
    let out = hab.note("http://127.0.0.1:1", &lines);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
    assert!(
        stderr.contains("line 2 of the note") && stderr.contains("2048"),
        "{stderr}"
    );
    assert!(
        stderr.contains("ljos note proj-aaaa --file PATH"),
        "{stderr}"
    );
    assert!(!hab.calls().contains("vissue note"), "{}", hab.calls());
}

/// ljos-yjho: MCP reports the same refusal as an error. A policy label used
/// to come back as a successful tool result, with a next step that said to
/// keep going, while the note was dropped.
#[test]
fn an_oversize_note_is_an_mcp_error() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = root.path().join("bin");
    let tracker = root.path().join("tracker");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    let log = root.path().join("calls.log");
    script(
        &bin,
        "vissue",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\nexit 0\n",
            log.display()
        ),
    );
    script(&bin, "claimdag", "#!/bin/sh\nexit 1\n");
    script(&bin, "deedar", "#!/bin/sh\nexit 0\n");
    #[expect(
        clippy::disallowed_methods,
        reason = "links a path that is not a git hook"
    )]
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "saddle")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(40));
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let payload = format!("snapshot b64z:{}", "QUJD".repeat(5_000));
    let bytes = payload.len().to_string();
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"probe","version":"0"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "ljos_note",
                "arguments": {"seat": "saddle", "issue": "proj-aaaa", "text": payload}
            }
        }),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();
    drop(stdin);
    let mut answer = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value["id"] == 2 {
            answer = Some(value);
            break;
        }
    }
    let _ = child.wait();
    let answer = answer.expect("no answer from ljos_note");
    let text = answer["result"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    assert!(
        text.contains(&bytes) && text.contains("4096"),
        "the error did not name the size and the cap:\n{text}"
    );
    assert!(
        text.contains("ljos note proj-aaaa --file PATH"),
        "the error did not name the remedy:\n{text}"
    );
    assert!(text.contains("Nothing was noted"), "{text}");
    assert!(
        !text.contains("keep going"),
        "a dropped note was told to keep going:\n{text}"
    );
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !calls.contains("vissue note"),
        "the payload reached the ticket:\n{calls}"
    );
}
