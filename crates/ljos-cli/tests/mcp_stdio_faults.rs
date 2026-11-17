//! `ljos mcp` stays up over stdio. A failure is one line and names the command.
//!
//! The pack here is a stand-in on a free port. The show case uses the real
//! `vissue` and the real `ljos` binary.

ljos_test_boot::harness!();

#[path = "common/sibling.rs"]
mod sibling;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

fn pack(body_for: impl Fn(&str) -> (u16, &'static str) + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body_for = Arc::new(body_for);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let body_for = body_for.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let (code, payload) = body_for(path);
                let reason = if code == 503 {
                    "Service Unavailable"
                } else {
                    "OK"
                };
                let resp = format!(
                    "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

fn hits() -> String {
    pack(|path| {
        if path.starts_with("/v1/search") {
            (
                200,
                r#"{"hits":[{"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","text":"Seat the kiln door only when cold.","kind":"lesson","score":0.9}]}"#,
            )
        } else {
            (200, r#"{"line":"ok","ready":true,"atoms":[]}"#)
        }
    })
}

fn lexical_hits() -> String {
    pack(|path| {
        if path.starts_with("/v1/search") {
            (
                200,
                r#"{"hits":[{"id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","text":"The greeting prints only when GREET is yes.","kind":"lesson","score":0.4}]}"#,
            )
        } else if path.starts_with("/v1/status") {
            (200, r#"{"embedder":{"available":false}}"#)
        } else {
            (200, r#"{"ready":true,"atoms":[]}"#)
        }
    })
}

fn busy() -> String {
    pack(|_path| (503, r#"{"error":"busy","line":"busy, retrying"}"#))
}

struct Session {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: Receiver<String>,
    home: tempfile::TempDir,
}

impl Session {
    fn open(extra: &[(&str, &str)]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TOKEN", "probe-token")
            .env("PACKSET_WORKSPACE", "ada")
            .env("PACKSET_TIMEOUT_MS", "800")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("LJOS_MCP_ORPHAN_IDLE", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_SEAT", "ada")
            .current_dir(home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let mut line = String::new();
            while stdout.read_line(&mut line).unwrap_or(0) > 0 {
                if tx.send(line.clone()).is_err() {
                    break;
                }
                line.clear();
            }
        });
        let mut session = Self {
            child,
            stdin,
            lines,
            home,
        };
        session.send(
            &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"probe","version":"0"}
            }}),
        );
        session.send(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        let init = session.next(Duration::from_secs(8));
        assert_eq!(init["result"]["serverInfo"]["name"], "ljos", "{init}");
        session
    }

    fn send(&mut self, value: &serde_json::Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn send_raw(&mut self, text: &str) {
        self.stdin.write_all(text.as_bytes()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn next(&self, timeout: Duration) -> serde_json::Value {
        let line = self
            .lines
            .recv_timeout(timeout)
            .unwrap_or_else(|err| panic!("no line ({err}), alive={}", self.child.id()));
        serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}: {line}"))
    }

    fn alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.home.path();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text_of(value: &serde_json::Value) -> String {
    value["result"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| value.to_string())
}

fn first_line(value: &serde_json::Value) -> String {
    value["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn call(
    session: &mut Session,
    id: u64,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    session.send(&serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    }));
    session.next(Duration::from_secs(12))
}

#[test]
fn a_bad_line_names_the_command_and_the_server_stays_up() {
    let url = hits();
    let mut session = Session::open(&[("PACKSET_URL", &url)]);
    session.send_raw("not json\n");
    session.send_raw("{}\n");
    session.send(&serde_json::json!({"jsonrpc":"2.0","id":9,"method":"no/such"}));
    session.send(
        &serde_json::json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{
            "name":"ljos_nope","arguments":{}
        }}),
    );
    session.send(
        &serde_json::json!({"jsonrpc":"2.0","id":11,"method":"tools/call","params":{
            "name":"ljos_search","arguments":{"query":1}
        }}),
    );
    let mut saw_plain = false;
    let mut saw_invalid = false;
    let mut saw_method = false;
    let mut saw_tool = false;
    let mut saw_args = false;
    for _ in 0..5 {
        let value = session.next(Duration::from_secs(8));
        let raw = value.to_string();
        if raw.contains("ljos mcp --help") && raw.contains("not JSON") {
            saw_plain = true;
        }
        if raw.contains("ljos mcp --help") && raw.contains("not a JSON-RPC call") {
            saw_invalid = true;
        }
        if raw.contains("no/such") && raw.contains("ljos mcp --help") {
            saw_method = true;
        }
        if value["id"] == 10 {
            let line = first_line(&value);
            assert!(value["result"]["isError"] == true, "{value}");
            assert!(line.contains("run `ljos help`"), "{line}");
            assert!(!line.contains('\n'), "{line}");
            saw_tool = true;
        }
        if value["id"] == 11 {
            let line = first_line(&value);
            assert!(value["result"]["isError"] == true, "{value}");
            assert!(line.contains("ljos help search"), "{line}");
            assert!(!line.contains('\n'), "{line}");
            let help = Command::new(env!("CARGO_BIN_EXE_ljos"))
                .args(["help", "search"])
                .output()
                .unwrap();
            assert!(
                help.status.success(),
                "hinted command failed: {}{}",
                String::from_utf8_lossy(&help.stdout),
                String::from_utf8_lossy(&help.stderr)
            );
            saw_args = true;
        }
    }
    assert!(saw_plain, "a bad line did not name ljos mcp --help");
    assert!(saw_invalid, "an empty object did not name ljos mcp --help");
    assert!(saw_method, "an unknown method did not name ljos mcp --help");
    assert!(saw_tool, "an unknown tool did not name ljos help");
    assert!(saw_args, "bad arguments did not name ljos help search");
    let search = call(
        &mut session,
        12,
        "ljos_search",
        serde_json::json!({"query": "kiln"}),
    );
    assert_ne!(search["result"]["isError"], true, "{search}");
    assert!(session.alive(), "the server exited");
}

#[test]
fn a_missing_seat_and_a_missing_tracker_name_the_command() {
    let url = hits();
    let mut seated = Session::open(&[("PACKSET_URL", &url)]);
    // The open helper sets LJOS_SEAT. A write still needs the argument.
    let remember = call(
        &mut seated,
        2,
        "ljos_remember",
        serde_json::json!({"text": "Seat the kiln door only when cold."}),
    );
    let line = first_line(&remember);
    assert_eq!(remember["result"]["isError"], true, "{remember}");
    assert!(line.contains("`ljos seat`"), "{line}");
    assert!(!line.contains('\n'), "{line}");
    assert!(seated.alive());

    let mut bare = Session::open(&[("PACKSET_URL", &url)]);
    let filed = call(
        &mut bare,
        2,
        "ljos_file",
        serde_json::json!({"title": "Pick a codec", "project": "demo", "seat": "ada"}),
    );
    let line = text_of(&filed);
    assert_eq!(filed["result"]["isError"], true, "{filed}");
    assert!(line.contains("`ljos onboard`"), "{line}");
    assert!(bare.alive());
}

#[test]
fn a_word_search_names_the_missing_encoder() {
    let url = lexical_hits();
    let mut session = Session::open(&[("PACKSET_URL", &url)]);
    let search = call(
        &mut session,
        2,
        "ljos_search",
        serde_json::json!({"query": "greeting", "seat": "ada"}),
    );
    assert_ne!(search["result"]["isError"], true, "{search}");
    let reading = search["result"]["structuredContent"]["reading"]
        .as_str()
        .unwrap_or("");
    assert!(
        reading.contains("encoder not installed")
            && reading.contains("cargo binstall --locked packset-embed"),
        "{reading}"
    );
    assert!(
        !reading.contains("fraction") && !reading.contains("how many checks"),
        "{reading}"
    );
    assert!(session.alive());
}

#[test]
fn a_busy_pack_names_the_search_to_run_again() {
    let url = busy();
    let mut session = Session::open(&[("PACKSET_URL", &url)]);
    let search = call(
        &mut session,
        2,
        "ljos_search",
        serde_json::json!({"query": "kiln door"}),
    );
    let reading = search["result"]["structuredContent"]["reading"]
        .as_str()
        .unwrap_or("");
    assert!(
        reading.contains("The pack is busy") && reading.contains("ljos search \"kiln door\""),
        "{reading}"
    );
    let quoted = call(
        &mut session,
        4,
        "ljos_search",
        serde_json::json!({"query": "\""}),
    );
    let quoted = quoted["result"]["structuredContent"]["reading"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        quoted.contains("ljos search kiln") && !quoted.contains("QUERY"),
        "{quoted}"
    );
    assert!(session.alive());
    let remember = call(
        &mut session,
        3,
        "ljos_remember",
        serde_json::json!({"text": "Seat the kiln door only when cold.", "seat": "ada"}),
    );
    let line = first_line(&remember);
    assert!(
        line.contains("The pack is busy")
            && (line.contains("`ljos remember`") || line.contains("`ljos doctor`")),
        "{line}"
    );
    assert!(!line.contains('\n'), "{line}");
    assert!(session.alive());
}

#[test]
fn a_restarted_writer_is_reached_on_the_next_call() {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let serve = |stop: Arc<std::sync::atomic::AtomicBool>| {
        let mut waited = 0;
        let listener = loop {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => break listener,
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && waited < 100 => {
                    waited += 1;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("bind 127.0.0.1:{port}: {e}"),
            }
        };
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let Ok(mut stream) = stream else { break };
                std::thread::spawn(move || {
                    let mut buf = [0u8; 8192];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]);
                    let path = head.split_whitespace().nth(1).unwrap_or("/");
                    let payload = if path.starts_with("/v1/search") {
                        r#"{"hits":[{"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","text":"Seat the kiln door only when cold.","kind":"lesson","score":0.9}]}"#
                    } else {
                        r#"{"line":"ok","ready":true,"atoms":[]}"#
                    };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                });
            }
        })
    };
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let first = serve(stop.clone());
    let url = format!("http://127.0.0.1:{port}");
    let mut session = Session::open(&[("PACKSET_URL", &url)]);
    let before = call(
        &mut session,
        2,
        "ljos_search",
        serde_json::json!({"query": "kiln"}),
    );
    assert!(text_of(&before).contains("kiln door"), "{before}");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = std::net::TcpStream::connect(("127.0.0.1", port));
    let _ = first.join();
    std::thread::sleep(Duration::from_millis(50));
    let again = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let second = serve(again.clone());
    let after = call(
        &mut session,
        3,
        "ljos_search",
        serde_json::json!({"query": "kiln"}),
    );
    assert!(
        text_of(&after).contains("kiln door"),
        "the restarted writer was not reached: {after}"
    );
    assert!(session.alive());
    again.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = std::net::TcpStream::connect(("127.0.0.1", port));
    drop(second);
}

#[test]
fn a_quiet_client_keeps_the_server_and_a_long_row_is_cut() {
    let url = hits();
    let mut quiet = Session::open(&[("PACKSET_URL", &url), ("LJOS_MCP_ORPHAN_IDLE", "1")]);
    std::thread::sleep(Duration::from_secs(3));
    assert!(quiet.alive(), "a live client lost the server");
    quiet.send(&serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}));
    let listed = quiet.next(Duration::from_secs(8));
    assert!(listed["result"]["tools"].is_array(), "{listed}");

    let huge = "word ".repeat(20_000);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let payload = serde_json::json!({
        "hits": [{"id": "c".repeat(32), "text": huge, "kind": "lesson", "score": 1.0}],
        "line": "ok", "ready": true
    })
    .to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let payload = payload.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    let url = format!("http://{addr}");
    let mut session = Session::open(&[("PACKSET_URL", &url)]);
    let search = call(
        &mut session,
        2,
        "ljos_search",
        serde_json::json!({"query": "word"}),
    );
    let raw = search.to_string();
    assert!(raw.contains("A row was cut"), "{raw}");
    assert!(raw.contains("ljos search"), "{raw}");
    assert!(
        raw.len() < 20_000,
        "the tool result was {} bytes",
        raw.len()
    );
    let again = call(
        &mut session,
        3,
        "ljos_search",
        serde_json::json!({"query": "word"}),
    );
    assert!(again.get("result").is_some(), "{again}");
    assert!(session.alive());
}

#[test]
fn show_over_mcp_prints_the_ticket() {
    let vissue = sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"));
    let path = format!(
        "{}:{}:/usr/bin:/bin",
        vissue.parent().unwrap().display(),
        std::path::Path::new(env!("CARGO_BIN_EXE_ljos"))
            .parent()
            .unwrap()
            .display()
    );
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("tracker");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    let filed = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args([
            "file",
            "Pick a codec",
            "--top",
            "-p",
            "demo",
            "--body",
            "The door needs a codec.",
        ])
        .env_clear()
        .env("PATH", &path)
        .env("HOME", home.path())
        .env("VISSUE_ROOT", &root)
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("VISSUE_NO_ROUTE", "1")
        .output()
        .unwrap();
    assert!(
        filed.status.success(),
        "{}",
        String::from_utf8_lossy(&filed.stderr)
    );
    let id = String::from_utf8_lossy(&filed.stdout)
        .lines()
        .next()
        .unwrap()
        .trim()
        .to_string();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    cmd.env_clear()
        .env("PATH", &path)
        .env("HOME", home.path())
        .env("VISSUE_ROOT", &root)
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_MCP_ORPHAN_IDLE", "off")
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    for line in [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"probe","version":"0"}
        }}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"ljos_show","arguments":{"issue": id, "seat": "ada"}
        }}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    let mut shown = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(Duration::from_secs(1)) {
            if line.contains("\"id\":2") || line.contains("\"id\": 2") {
                shown = Some(line);
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let shown = shown.expect("ljos_show did not answer");
    assert!(shown.contains(&id), "{shown}");
    assert!(shown.contains("The door needs a codec"), "{shown}");
    assert!(!shown.contains("\"isError\":true"), "{shown}");
}
