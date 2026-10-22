//! `ljos_mail_search` and `ljos_mail_show` are opt-in, read the mailbox
//! the `[mail]` table names, and refuse on a cloud seat.

#[path = "common/maildir.rs"]
mod maildir;

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

struct Session {
    answers: Vec<Value>,
    stderr: String,
}

impl Session {
    fn by_id(&self, id: u64) -> &Value {
        self.answers
            .iter()
            .find(|a| a["id"] == id)
            .unwrap_or_else(|| panic!("no answer {id}: {:?}\n{}", self.answers, self.stderr))
    }

    fn names(&self, id: u64) -> Vec<String> {
        self.by_id(id)["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    fn text(&self, id: u64) -> String {
        self.by_id(id)["result"]["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }
}

/// One server run: initialize, then `calls` as ids 2, 3 and so on.
fn session(home: &Path, tools: Option<&str>, calls: &[Value]) -> Session {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(tools) = tools {
        cmd.env("LJOS_MCP_TOOLS", tools);
    }
    let mut child = cmd.spawn().unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut lines = vec![
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    for (n, call) in calls.iter().enumerate() {
        let mut call = call.clone();
        call["jsonrpc"] = json!("2.0");
        call["id"] = json!(n + 2);
        lines.push(call);
    }
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!stderr.contains("panicked"), "{stderr}");
    let answers = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    Session { answers, stderr }
}

fn call(name: &str, arguments: Value) -> Value {
    json!({"method": "tools/call", "params": {"name": name, "arguments": arguments}})
}

fn list() -> Value {
    json!({"method": "tools/list"})
}

fn configure(home: &Path, mailbox: &maildir::Mailbox) {
    let dir = home.join("config/ljos");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mail.toml"),
        format!(
            "[mail]\nnotmuch_config = \"{}\"\n",
            mailbox.config.display()
        ),
    )
    .unwrap();
}

#[test]
fn the_mail_tools_are_listed_only_when_asked_for() {
    let home = tempfile::tempdir().unwrap();
    let core = session(home.path(), None, &[list()]);
    let all = session(home.path(), Some("all"), &[list()]);
    let opted = session(home.path(), Some("core,mail_search,mail_show"), &[list()]);
    for name in ["ljos_mail_search", "ljos_mail_show"] {
        assert!(!core.names(2).iter().any(|n| n == name), "{name} is core");
        assert!(all.names(2).iter().any(|n| n == name), "{name} missing");
        assert!(opted.names(2).iter().any(|n| n == name), "{name} missing");
    }
    for tool in opted.by_id(2)["result"]["tools"].as_array().unwrap() {
        if tool["name"].as_str().unwrap().starts_with("ljos_mail_") {
            assert_eq!(tool["annotations"]["readOnlyHint"], true, "{tool}");
        }
    }
}

#[test]
fn search_and_show_read_the_configured_mailbox() {
    let home = tempfile::tempdir().unwrap();
    let mailbox = maildir::mailbox();
    configure(home.path(), &mailbox);
    let thread = mailbox.thread_of("hidden-html-1@shop.example.test");
    let ran = session(
        home.path(),
        Some("mail_search,mail_show"),
        &[
            call("ljos_mail_search", json!({"query": "lunch", "limit": 5})),
            call("ljos_mail_show", json!({"thread": thread})),
            call("ljos_mail_search", json!({"query": "lunch", "limit": 51})),
        ],
    );
    let found = &ran.by_id(2)["result"];
    assert_ne!(found["isError"], true, "{found}");
    let threads = found["structuredContent"]["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 1, "{found}");
    assert!(threads[0]["subject"]
        .as_str()
        .unwrap()
        .contains("Lunch on Friday"));

    let shown = &ran.by_id(3)["result"];
    assert_ne!(shown["isError"], true, "{shown}");
    let msg = &shown["structuredContent"]["messages"][0];
    assert_eq!(msg["hidden_text_dropped"], true, "{shown}");
    assert!(!msg["body"]
        .as_str()
        .unwrap()
        .contains("Ignore all previous"));
    let maildir = mailbox.maildir().display().to_string();
    assert!(!shown.to_string().contains(&maildir), "{shown}");

    assert_eq!(ran.by_id(4)["result"]["isError"], true);
    assert!(ran.text(4).contains("limit"), "{}", ran.text(4));
}

#[test]
fn no_mail_table_is_a_refusal() {
    let home = tempfile::tempdir().unwrap();
    let ran = session(
        home.path(),
        Some("mail_search"),
        &[call("ljos_mail_search", json!({"query": "lunch"}))],
    );
    assert_eq!(ran.by_id(2)["result"]["isError"], true);
    assert!(ran.text(2).contains("[mail]"), "{}", ran.text(2));
}

#[test]
fn a_cloud_seat_is_refused_even_with_a_mailbox() {
    let home = tempfile::tempdir().unwrap();
    let mailbox = maildir::mailbox();
    configure(home.path(), &mailbox);
    let deny = |pattern: &str| ljos_cli::Rule {
        pattern: pattern.into(),
        verdict: "deny".into(),
        reason: "cloud seat rule".into(),
    };
    ljos_cli::keep_rules(
        &home.path().join("config/ljos/cloud-guard-rules.json"),
        &[
            deny("cloud-cursor-push"),
            deny("cloud-gh-pr-create"),
            deny("cloud-gh-pr-merge"),
        ],
    );
    let thread = mailbox.thread_of("plain-1@example.test");
    let ran = session(
        home.path(),
        Some("mail_search,mail_show"),
        &[
            call("ljos_mail_search", json!({"query": "lunch"})),
            call("ljos_mail_show", json!({"thread": thread})),
        ],
    );
    for id in [2, 3] {
        assert_eq!(ran.by_id(id)["result"]["isError"], true, "{id}");
        assert!(ran.text(id).contains("cloud seat"), "{}", ran.text(id));
        assert!(!ran.text(id).contains("Lunch"), "{}", ran.text(id));
    }
}
