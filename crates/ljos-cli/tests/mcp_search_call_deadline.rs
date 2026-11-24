//! A long-lived `ljos mcp` gives each search and recall call its own start
//! room.
//!
//! The call deadline is `LJOS_CALL_TIMEOUT`. A first call answers at once.
//! The server then stays up past that deadline, the writer restarts, and a
//! new call still waits out the start and reaches the pack.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const OPENS_AFTER: Duration = Duration::from_millis(1000);
const HITS: &str = r#"{"hits":[{"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","text":"Seat the kiln door only when cold.","kind":"lesson","score":0.9}]}"#;

fn writer(restarted: Arc<Mutex<Option<Instant>>>, closed: Arc<AtomicBool>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            if closed.load(Ordering::Relaxed) {
                break;
            }
            let restarted = Arc::clone(&restarted);
            std::thread::spawn(move || {
                let mut sock = conn;
                let mut buf = [0u8; 8192];
                let n = sock.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/").to_string();
                let loading = restarted
                    .lock()
                    .unwrap()
                    .is_some_and(|at| at.elapsed() < OPENS_AFTER);
                let (status, body) = match (path.as_str(), loading) {
                    ("/health", true) => (200, "not-ready".to_string()),
                    ("/v1/search", true) => (
                        503,
                        r#"{"error":"the pack is busy while loading"}"#.to_string(),
                    ),
                    ("/v1/search", false) => {
                        // Slow enough that overlapping calls fill both
                        // runtime workers, so each worker serves a call.
                        std::thread::sleep(Duration::from_millis(400));
                        (200, HITS.to_string())
                    }
                    _ => (200, r#"{"line":"ok","ready":true,"atoms":[]}"#.to_string()),
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    url
}

/// A tracker with one ticket, and the programs a recall drives as stand-ins.
struct Tracker {
    dir: tempfile::TempDir,
    issue: String,
}

fn script(dir: &std::path::Path, name: &str, body: &str) {
    ljos_cli::install_executable(&dir.join(name), body).unwrap();
}

fn tracker() -> Tracker {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let tracker = root.join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    script(
        root,
        "vissue",
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"probe-sit\",\"title\":\"kiln door\",\"state\":\"TODO\"}}' ;;\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\nesac\nexit 0\n",
            tracker.display()
        ),
    );
    script(
        root,
        "claimdag",
        "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\nesac\necho '[]'\n",
    );
    script(root, "deedar", "#!/bin/sh\necho head\n");
    let layout = vissue_core::Layout::new(&tracker, "Software");
    vissue_core::ops::create(&layout, "probe", "kiln door", Default::default()).unwrap();
    let doc =
        vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe")).unwrap();
    let issue = doc.headings[0].id.clone();
    Tracker { dir, issue }
}

struct Session {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: mpsc::Receiver<String>,
    _home: tempfile::TempDir,
}

impl Session {
    fn open(url: &str) -> Self {
        Self::open_in(url, None)
    }

    fn open_in(url: &str, tracker: Option<&Tracker>) -> Self {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        let path = match tracker {
            Some(t) => format!("{}:/usr/bin:/bin", t.dir.path().display()),
            None => "/usr/bin:/bin".to_string(),
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut command = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        command
            .env_clear()
            .env("PATH", path)
            .env("HOME", root)
            .env("XDG_CONFIG_HOME", root.join(".config"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("PACKSET_URL", url)
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TOKEN", "probe-token")
            .env("PACKSET_WORKSPACE", "ada")
            .env("PACKSET_TIMEOUT_MS", "4000")
            .env("LJOS_CALL_TIMEOUT", "2")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "4")
            .env("LJOS_MCP_ORPHAN_IDLE", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_SEAT", "ada");
        if let Some(t) = tracker {
            command
                .env("VISSUE_ROOT", t.dir.path().join("tracker"))
                .env("VISSUE_NO_ROUTE", "1");
        }
        let mut child = command
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
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
            _home: home,
        };
        session.send(
            &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"probe","version":"0"}
            }}),
        );
        session.send(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        let init = session.next();
        assert_eq!(init["result"]["serverInfo"]["name"], "ljos", "{init}");
        session
    }

    fn send(&mut self, value: &serde_json::Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn next(&self) -> serde_json::Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(15))
            .unwrap_or_else(|err| panic!("no line ({err})"));
        serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}: {line}"))
    }

    fn ask(&mut self, id: u64) {
        self.ask_tool(id, "ljos_search", serde_json::json!({"query": "kiln"}));
    }

    fn ask_tool(&mut self, id: u64, name: &str, arguments: serde_json::Value) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }));
    }

    fn search(&mut self, id: u64) -> String {
        self.ask(id);
        text_of(&self.next())
    }
}

fn text_of(reply: &serde_json::Value) -> String {
    reply["result"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| reply.to_string())
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_server_up_past_the_call_deadline_still_waits_out_a_start() {
    let restarted = Arc::new(Mutex::new(None));
    let closed = Arc::new(AtomicBool::new(false));
    let url = writer(Arc::clone(&restarted), Arc::clone(&closed));
    let mut session = Session::open(&url);
    for id in 2..6 {
        session.ask(id);
    }
    for _ in 2..6 {
        let first = text_of(&session.next());
        assert!(first.contains("kiln door"), "an early call: {first}");
    }
    std::thread::sleep(Duration::from_secs(3));
    *restarted.lock().unwrap() = Some(Instant::now());
    let second = session.search(7);
    closed.store(true, Ordering::Relaxed);
    let _ = std::net::TcpStream::connect(url.trim_start_matches("http://"));
    assert!(
        second.contains("kiln door"),
        "a call after the deadline period got no start room: {second}"
    );
}

#[test]
fn a_recall_on_a_server_up_past_the_call_deadline_still_waits_out_a_start() {
    let restarted = Arc::new(Mutex::new(None));
    let closed = Arc::new(AtomicBool::new(false));
    let url = writer(Arc::clone(&restarted), Arc::clone(&closed));
    let tracker = tracker();
    let mut session = Session::open_in(&url, Some(&tracker));
    // Overlapping searches fill both runtime workers, so the recall below
    // runs on a worker that has already served a call. A deadline a call
    // left behind would be spent by now.
    for id in 2..6 {
        session.ask(id);
    }
    for _ in 2..6 {
        let early = text_of(&session.next());
        assert!(early.contains("kiln door"), "an early call: {early}");
    }
    std::thread::sleep(Duration::from_secs(3));
    *restarted.lock().unwrap() = Some(Instant::now());
    session.ask_tool(
        7,
        "ljos_recall",
        serde_json::json!({"issue": tracker.issue}),
    );
    let second = text_of(&session.next());
    closed.store(true, Ordering::Relaxed);
    let _ = std::net::TcpStream::connect(url.trim_start_matches("http://"));
    assert!(
        second.contains("Seat the kiln door only when cold."),
        "a recall after the deadline period got no start room: {second}"
    );
}
