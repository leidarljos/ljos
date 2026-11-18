//! What an agent that has never used ljos reads back from `ljos-mcp` when
//! it makes a typical mistake. Each test asserts the text a client shows.
//!
//! No pack, tracker or sibling tool is needed: every call here is answered
//! by the server itself.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
    _home: tempfile::TempDir,
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
            next_id: 1,
            _home: home,
        };
        let init = session.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "probe", "version": "0"}
            }),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "ljos", "{init}");
        session.send(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        session
    }

    fn send(&mut self, value: &serde_json::Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(12))
                .unwrap_or_else(|err| panic!("no answer to {method} ({err})"));
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            if value["id"] == id {
                return value;
            }
        }
    }

    fn call(&mut self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        self.request(
            "tools/call",
            serde_json::json!({"name": name, "arguments": arguments}),
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The text blocks of a tool result, one per line.
fn blocks(value: &serde_json::Value) -> Vec<String> {
    value["result"]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("no content in {value}"))
        .iter()
        .filter_map(|block| block["text"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn a_bad_argument_names_the_field_that_failed() {
    let mut session = Session::open(&[]);
    let missing = session.call("ljos_claim", serde_json::json!({"seat": "ada"}));
    assert_eq!(missing["result"]["isError"], true, "{missing}");
    assert_eq!(
        blocks(&missing)[0],
        "ljos_claim: the arguments do not match the tool (missing field node), so run `ljos help claim`."
    );
    let wrong = session.call("ljos_claim", serde_json::json!({"seat": "ada", "node": 7}));
    assert_eq!(
        blocks(&wrong)[0],
        "ljos_claim: the arguments do not match the tool (invalid type: integer 7, expected a string), so run `ljos help claim`."
    );
    let command = session.call("ljos_guard_check", serde_json::json!({}));
    assert_eq!(
        blocks(&command)[0],
        "ljos_guard_check: the arguments do not match the tool (missing field command), so run `ljos help policy`."
    );
}
