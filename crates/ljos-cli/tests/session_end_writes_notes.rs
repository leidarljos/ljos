//! Session-end extraction keeps assistant prose as unverified notes.
//!
//! A sentence an agent wrote in its own reply is the agent's reading of the
//! work, not a fact the person stated. Writing it as a lesson let it ride
//! onto later prompts as a standing claim. Extraction now writes a `note`
//! atom with agent provenance, so it stays out of prompt facts.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

fn read_http(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = match stream.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&tmp[..n]);
        let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let header = String::from_utf8_lossy(&buf[..pos]);
        let len = header
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if buf.len() >= pos + 4 + len {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// A pack that records every posted atom and answers the writes with a note
/// body, so the extraction completes.
fn serve(atoms: Arc<Mutex<Vec<serde_json::Value>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let req = read_http(&mut stream);
            let path = req.lines().next().unwrap_or("");
            let body = req.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
            let (code, payload) = if path.starts_with("POST /v1/atoms") {
                let mut atom: serde_json::Value =
                    serde_json::from_str(body).unwrap_or(serde_json::json!({}));
                if atom.get("id").and_then(|v| v.as_str()).is_none() {
                    atom["id"] = serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
                }
                atoms.lock().unwrap().push(atom.clone());
                (200, atom.to_string())
            } else if path.starts_with("GET /v1/atoms") {
                let stored = atoms.lock().unwrap().clone();
                (200, serde_json::json!({"atoms": stored}).to_string())
            } else {
                (200, "{}".into())
            };
            let resp = format!(
                "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                code,
                payload.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

#[test]
fn session_end_extraction_writes_notes_not_lessons() {
    let home = tempfile::tempdir().unwrap();
    let transcript = home.path().join("transcript.jsonl");
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"Fix the build."}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The root cause was a shared temp dir between tests. Always give each test its own tempdir on this box."}]}}"#,
            "\n",
        ),
    )
    .unwrap();
    let atoms = Arc::new(Mutex::new(Vec::new()));
    let url = serve(Arc::clone(&atoms));
    let run = home.path().join("run");
    std::fs::create_dir_all(run.join("ljos")).unwrap();
    let pid = std::process::id();
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap();
    std::fs::write(
        run.join("ljos/hold-probe-sitting"),
        format!(
            "book-holder\nprobe-seat\n{pid}\n{}\n2026-01-01T00:00:00Z\nprobe-0001\n-\n",
            comm.trim()
        ),
    )
    .unwrap();

    let input = serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "extract-session-1",
        "transcript_path": transcript,
        "cwd": home.path(),
    })
    .to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["hook"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("USER", "person")
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("PACKSET_URL", &url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("LJOS_SEAT", "probe-seat")
        .env("LJOS_HOLD", "book-holder")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("VISSUE_NO_ROUTE", "1")
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "hook failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let stored = atoms.lock().unwrap().clone();
    let posted: Vec<&serde_json::Value> = stored
        .iter()
        .filter(|a| a["text"].as_str().is_some_and(|t| t.contains("root cause")))
        .collect();
    assert!(
        !posted.is_empty(),
        "the sentence was not posted: {stored:?}"
    );
    for atom in posted {
        assert_eq!(atom["kind"], "note", "extraction wrote a non-note: {atom}");
        assert_eq!(
            atom["issue"], "probe-0001",
            "the note names no sitting: {atom}"
        );
        assert_eq!(
            atom["source"]["basis"], "agent",
            "a session-end note is not agent-basis: {atom}"
        );
        let entities = atom["entities"].to_string();
        assert!(
            entities.contains("note-source:"),
            "the note lacks provenance: {atom}"
        );
    }
}
