//! A search id is a prefix `ljos forget` accepts.
//!
//! Several memories share a short hex run. The id on the search page, in
//! JSON and in the MCP tool is long enough to name one of them, and
//! `ljos forget` retires that one.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

const QUERY: &str = "zephyrslate";

struct Home {
    root: PathBuf,
    url: String,
    full: Vec<String>,
    prefix: Vec<String>,
}

impl Home {
    fn open() -> Self {
        let shared = format!("45{}", "a".repeat(40));
        let full = vec![
            format!("{}{}", "a".repeat(8), "1".repeat(56)),
            format!("{}{}", "a".repeat(8), "2".repeat(56)),
            format!("{}{}", "b".repeat(8), "3".repeat(56)),
            format!("local:{shared}1111111111111111111111"),
            format!("local:{shared}2222222222222222222222"),
            format!("local:45{}3333333333333333333333", "b".repeat(40)),
        ];
        let prefix = vec![
            format!("{}1", "a".repeat(8)),
            format!("{}2", "a".repeat(8)),
            "b".repeat(8),
            format!("local:{shared}1"),
            format!("local:{shared}2"),
            "local:45bbbbbb".to_string(),
        ];
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/bin",
            "run",
            "work",
            "tracker",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let kept = root.join("tracker/.ljos/memory/kept");
        std::fs::create_dir_all(&kept).unwrap();
        let pack: Vec<Value> = full
            .iter()
            .enumerate()
            .map(|(index, id)| atom(id, index))
            .collect();
        for id in full.iter().filter(|id| id.starts_with("local:")) {
            let name = id.trim_start_matches("local:");
            std::fs::write(kept.join(format!("{name}.json")), kept_row(id)).unwrap();
        }
        let body = json!({"atoms": pack});
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || serve(listener, body));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
            full,
            prefix,
        }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let home = self.root.join("home");
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")))
    }
}

fn atom(id: &str, index: usize) -> Value {
    json!({
        "id": id,
        "kind": "lesson",
        "text": format!("{QUERY} pack note {index}"),
        "score": 0.9,
        "ts": "2026-11-01T12:00:00Z",
        "entities": ["seat:alice"],
        "origin": "agent-derived",
        "source": {"seat": "alice", "basis": "agent"},
        "dense_rank": 1,
        "cosine": 0.9,
        "ballots": 2,
        "of": 3,
    })
}

fn kept_row(id: &str) -> String {
    json!({
        "id": id,
        "text": format!("{QUERY} local note {id}"),
        "kind": "lesson",
        "origin": "agent-derived",
        "basis": "agent",
        "labels": [],
        "entities": ["seat:alice"],
        "machine": "test",
        "seat": "alice",
        "import_id": "",
        "source_id": "",
        "content_sha256": id.trim_start_matches("local:"),
        "origin_hash": "abc",
        "at": "2026-11-01T12:00:00Z",
        "diagnostic": "",
        "effect": "stored-local",
        "retired": false,
    })
    .to_string()
}

fn serve(listener: TcpListener, atoms: Value) {
    for conn in listener.incoming().flatten() {
        let atoms = atoms.clone();
        thread::spawn(move || {
            let mut sock = conn;
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                }
            }
            let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let length = content_length(&head);
            let mut body = buf[head_end..].to_vec();
            while body.len() < length {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => body.extend_from_slice(&tmp[..n]),
                }
            }
            body.truncate(length);
            let request = head.split_whitespace().nth(1).unwrap_or("/");
            let path = request.split('?').next().unwrap_or("/");
            let query = request.split_once('?').map(|(_, q)| q).unwrap_or("");
            let payload = if path == "/v1/search" {
                let q = form_get(query, "q");
                let hits: Vec<Value> = atoms["atoms"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|atom| {
                        atom["text"]
                            .as_str()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&q.to_lowercase())
                    })
                    .cloned()
                    .collect();
                json!({"hits": hits, "rank": "relevance"}).to_string()
            } else if path == "/v1/atoms" {
                atoms.to_string()
            } else if path == "/v1/atoms/delete" {
                let asked: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let id = asked["id"].as_str().unwrap_or("");
                let found = atoms["atoms"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|atom| atom["id"].as_str() == Some(id));
                match found {
                    Some(atom) => {
                        let mut atom = atom.clone();
                        atom["tombstone"] = Value::Bool(true);
                        atom.to_string()
                    }
                    None => json!({"error": format!("no current atom {id}")}).to_string(),
                }
            } else if path == "/v1/status" {
                r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                    .to_string()
            } else {
                r#"{"ok":true}"#.to_string()
            };
            let status = if path == "/v1/atoms/delete" && payload.contains("no current atom") {
                "400 Bad Request"
            } else {
                "200 OK"
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        });
    }
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0)
}

fn form_get(query: &str, key: &str) -> String {
    for pair in query.split('&') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if name == key {
            return value.replace('+', " ");
        }
    }
    String::new()
}

fn printed_ids(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.split(", id ").nth(1))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

fn mcp_ids(home: &Home) -> Vec<String> {
    let dir = home.root.join("home");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("mcp")
        .current_dir(home.root.join("work"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &dir)
        .env("XDG_CONFIG_HOME", dir.join(".config"))
        .env("XDG_DATA_HOME", dir.join(".local/share"))
        .env("XDG_STATE_HOME", dir.join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.root.join("run"))
        .env("TMPDIR", &home.root)
        .env("VISSUE_ROOT", home.root.join("tracker"))
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "alice")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("LJOS_SEARCH_WARMUP_WAIT", "0")
        .env("PACKSET_URL", &home.url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ljos mcp");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut lines = BufReader::new(stdout);
        let mut line = String::new();
        while lines.read_line(&mut line).unwrap_or(0) > 0 {
            if tx.send(std::mem::take(&mut line)).is_err() {
                break;
            }
        }
    });
    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "ljos_search", "arguments": {"query": QUERY}}
    });
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"probe","version":"0"}}}}}}"#
    )
    .unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    writeln!(stdin, "{call}").unwrap();
    stdin.flush().unwrap();
    let mut answer = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(line) => {
                let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                if value["id"] == 2 {
                    answer = Some(value);
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let answer = answer.expect("ljos_search did not answer");
    assert!(answer.get("error").is_none(), "{answer}");
    answer["result"]["structuredContent"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("no rows: {answer}"))
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn a_printed_search_id_is_the_one_forget_retires() {
    let home = Home::open();
    let text = home.run(&["search", QUERY, "-n", "10"]);
    let stdout = String::from_utf8_lossy(&text.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&text.stderr).into_owned();
    assert!(text.status.success(), "search failed\n{stdout}\n{stderr}");
    let mut found = printed_ids(&stdout);
    found.sort();
    let mut want = home.prefix.clone();
    want.sort();
    assert_eq!(found, want, "text ids\n{stdout}");
    for id in &found {
        let hex = id
            .chars()
            .rev()
            .take_while(|ch| ch.is_ascii_hexdigit())
            .count();
        assert!(hex >= 8, "{id}");
    }
    assert!(
        !stdout.contains(", id local:45\n") && !stdout.contains(", id local:45 "),
        "{stdout}"
    );

    let json = home.run(&["search", QUERY, "-n", "10", "--json"]);
    let json_out = String::from_utf8_lossy(&json.stdout).into_owned();
    assert!(json.status.success(), "json search failed\n{json_out}");
    let parsed: Value = serde_json::from_str(&json_out).expect("json");
    let mut json_ids: Vec<String> = parsed["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|hit| hit["id"].as_str().map(str::to_string))
        .collect();
    json_ids.sort();
    assert_eq!(json_ids, want, "json ids\n{json_out}");

    let mut mcp = mcp_ids(&home);
    mcp.sort();
    assert_eq!(mcp, want, "mcp ids");

    let ambiguous = home.run(&["forget", "local:45"]);
    let ambiguous_err = String::from_utf8_lossy(&ambiguous.stderr).into_owned();
    assert!(
        !ambiguous.status.success(),
        "local:45 was accepted\n{ambiguous_err}"
    );
    assert!(
        ambiguous_err.contains("starts") && ambiguous_err.contains("ids"),
        "{ambiguous_err}"
    );

    for (prefix, full) in home.prefix.iter().zip(&home.full) {
        let gone = home.run(&["forget", prefix]);
        let out = String::from_utf8_lossy(&gone.stdout).into_owned();
        let err = String::from_utf8_lossy(&gone.stderr).into_owned();
        assert!(gone.status.success(), "forget {prefix}\n{out}\n{err}");
        assert!(
            out.contains(full),
            "forget {prefix} did not retire {full}\n{out}"
        );
    }
}
