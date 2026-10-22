//! Session-end extraction keeps assistant prose as unverified notes.
//!
//! A sentence an agent wrote in its own reply is the agent's reading of the
//! work, not a fact the person stated. Writing it as a lesson let it ride
//! onto later prompts as a standing claim. Extraction now writes a `note`
//! atom with agent provenance, so it stays out of prompt facts.

use std::io::{Read, Write};
use std::net::TcpListener;
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

    let input = serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "extract-1",
        "transcript_path": transcript,
    })
    .to_string();

    // SAFETY: a single-test integration binary; no other thread reads env.
    unsafe {
        std::env::set_var("HOME", home.path());
        std::env::set_var("XDG_CONFIG_HOME", home.path().join(".config"));
        std::env::set_var("XDG_STATE_HOME", home.path().join("state"));
        std::env::set_var("XDG_RUNTIME_DIR", home.path().join("run"));
        std::env::set_var("PACKSET_URL", &url);
        std::env::set_var("PACKSET_WORKSPACE", "seat");
        std::env::set_var("PACKSET_TIMEOUT_MS", "2000");
        std::env::remove_var("LJOS_EXTRACT");
        std::env::remove_var("LJOS_SEAT");
        std::env::remove_var("LJOS_OFF");
    }

    let written = ljos_cli::extract::at_session_end(&input, Some("extract-1"));
    assert!(written >= 1, "extraction wrote nothing");

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
