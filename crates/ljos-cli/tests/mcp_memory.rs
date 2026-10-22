//! An MCP remember lands in the pack `ljos memories list` reads, and a
//! search increments the use count that list prints.

#[path = "common/runner.rs"]
mod runner;

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
            } else if path.starts_with("GET /v1/search") {
                let hits: Vec<_> = atoms
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|atom| {
                        serde_json::json!({
                            "id": atom["id"],
                            "text": atom["text"],
                            "kind": atom["kind"],
                            "score": 1.0,
                            "entities": atom["entities"],
                        })
                    })
                    .collect();
                (200, serde_json::json!({"hits": hits}).to_string())
            } else if path.starts_with("GET /v1/atoms") {
                let stored = atoms.lock().unwrap().clone();
                (200, serde_json::json!({"atoms": stored}).to_string())
            } else {
                (404, r#"{"error":"missing"}"#.into())
            };
            let status = if code == 200 {
                "200 OK"
            } else {
                "404 Not Found"
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

#[test]
fn mcp_remember_is_what_the_cli_lists_and_a_search_counts() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join("state");
    let tracker = home.path().join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"saddle\"]\n").unwrap();
    let url = serve(Arc::new(Mutex::new(Vec::new())));
    let text = "Pin the review set before a release.";
    let prompt = format!("Please remember this for next time: {text}");
    let transcript = home.path().join(".claude/projects/-seat/person-1.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    let turn = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": prompt},
    });
    std::fs::write(&transcript, format!("{turn}\n")).unwrap();
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "person-1",
        "prompt": prompt,
        "transcript_path": transcript,
    })
    .to_string();
    // The runner spawns the prompt hook and starts the server, and the
    // prompt's session is the one that runner gave it. Runner variables
    // the suite itself was started with would name another runner and
    // session.
    let installed = runner::install_dir();
    let runner = installed.path().join("claude");
    runner::runner_at(&runner);
    let mut noted = Command::new(&runner)
        .args([
            "-c",
            "\"$@\"; exit $?",
            "claude",
            env!("CARGO_BIN_EXE_ljos"),
            "hook",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", &state)
        .env("XDG_RUNTIME_DIR", home.path())
        .env("VISSUE_ROOT", &tracker)
        .env("PACKSET_URL", &url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("LJOS_CAPTURE", "save")
        .env_remove("LJOS_UNATTENDED")
        .env_remove("LJOS_OFF")
        .env_remove("CURSOR_AGENT")
        .env_remove("CURSOR_CONVERSATION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    noted
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    drop(noted.stdin.take());
    let noted = noted.wait_with_output().unwrap();
    assert!(
        noted.status.success(),
        "prompt hook failed: {}",
        String::from_utf8_lossy(&noted.stderr)
    );
    let mut child = Command::new(&runner)
        .args([
            "-c",
            "\"$@\"; exit $?",
            "claude",
            env!("CARGO_BIN_EXE_ljos-mcp"),
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", &state)
        .env("XDG_RUNTIME_DIR", home.path())
        .env("VISSUE_ROOT", &tracker)
        .env("PACKSET_URL", &url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("LJOS_HARNESS_SESSION", "person-1")
        .env_remove("LJOS_SEAT")
        .env_remove("LJOS_OFF")
        .env_remove("CURSOR_AGENT")
        .env_remove("CURSOR_CONVERSATION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "ljos_remember",
            "arguments": {"seat": "saddle", "text": text, "transient": false}}}),
    ];
    {
        let stdin = child.stdin.as_mut().unwrap();
        for line in &lines {
            writeln!(stdin, "{line}").unwrap();
        }
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let answer = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|v| v["id"] == 2)
        .unwrap_or_else(|| panic!("no tool answer\n{stdout}\n{stderr}"));
    assert!(
        answer.get("error").is_none() && answer["result"]["isError"] != true,
        "{answer}\n{stderr}"
    );

    let list = || {
        let got = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["memories", "list", "--user"])
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env("XDG_STATE_HOME", &state)
            .env("XDG_RUNTIME_DIR", home.path())
            .env("VISSUE_ROOT", &tracker)
            .env("PACKSET_URL", &url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("LJOS_SEAT", "saddle")
            .output()
            .unwrap();
        (
            String::from_utf8_lossy(&got.stdout).into_owned(),
            String::from_utf8_lossy(&got.stderr).into_owned(),
        )
    };
    let (shown, err) = list();
    assert!(
        shown.contains(text),
        "the CLI list does not show the MCP write:\n{shown}\n{err}"
    );
    assert!(shown.contains("recalled 0x"), "{shown}");

    let searched = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", "review set"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", &state)
        .env("XDG_RUNTIME_DIR", home.path())
        .env("VISSUE_ROOT", &tracker)
        .env("PACKSET_URL", &url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("LJOS_SEAT", "saddle")
        .output()
        .unwrap();
    let search_out = String::from_utf8_lossy(&searched.stdout);
    assert!(
        searched.status.success() && search_out.contains(text),
        "search: {search_out}{}",
        String::from_utf8_lossy(&searched.stderr)
    );
    let (shown, err) = list();
    assert!(shown.contains("recalled 1x"), "{shown}\n{err}");
}
