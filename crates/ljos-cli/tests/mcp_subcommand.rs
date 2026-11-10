//! `ljos mcp` serves the same MCP server as `ljos-mcp`, for a client that
//! starts a server by its crate's name, and the list opens with the guard
//! and memory tools.

ljos_test_boot::harness!();
use std::io::Write as _;
use std::process::{Command, Stdio};

/// The tools `tools/list` returns when `LJOS_MCP_TOOLS` is unset.
/// `measurements/mcp-size.txt` records this set at 7,901 tokens.
const CORE: &[&str] = &[
    "ljos_rule",
    "ljos_prefer",
    "ljos_remember",
    "ljos_search",
    "ljos_start",
    "ljos_guard_check",
    "ljos_file",
    "ljos_options",
    "ljos_note",
    "ljos_recall",
    "ljos_sitting",
    "ljos_finish",
    "ljos_claim",
    "ljos_release",
    "ljos_send",
    "ljos_inbox",
    "ljos_reply",
    "ljos_vote",
    "ljos_consensus",
    "ljos_doctor",
];

/// Speak to `program` and return its stdout.
fn ask(program: &str, args: &[&str], tools: Option<&str>, methods: &[serde_json::Value]) -> String {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_RUNTIME_DIR", home.path())
        .env_remove("LJOS_MCP_TOOLS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(tools) = tools {
        cmd.env("LJOS_MCP_TOOLS", tools);
    }
    let mut child = cmd.spawn().unwrap();
    let mut lines = vec![
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    lines.extend(methods.iter().cloned());
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "exit {}\nstdout:\n{text}\nstderr:\n{err}",
        out.status
    );
    text
}

fn answers(text: &str) -> Vec<serde_json::Value> {
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn listed(text: &str) -> Vec<String> {
    let answers = answers(text);
    let list = answers.iter().find(|a| a["id"] == 2).expect(text);
    list["result"]["tools"]
        .as_array()
        .expect(text)
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn ljos_mcp_serves_and_lists_the_guard_first() {
    let text = ask(
        env!("CARGO_BIN_EXE_ljos"),
        &["mcp"],
        None,
        &[serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})],
    );
    let parsed = answers(&text);
    let init = parsed.iter().find(|a| a["id"] == 1).expect(&text);
    assert_eq!(init["result"]["serverInfo"]["name"], "ljos", "{text}");
    let names = listed(&text);
    assert_eq!(
        &names[..5],
        [
            "ljos_rule",
            "ljos_prefer",
            "ljos_remember",
            "ljos_search",
            "ljos_start"
        ],
        "{text}"
    );
    assert!(!names.iter().any(|n| n == "ljos_brief"), "{text}");
}

/// A client that starts `ljos-mcp` with no `LJOS_MCP_TOOLS` sees the core
/// set. `LJOS_MCP_TOOLS=all` is what lists the rest, and a call to a tool
/// the server did not list names that switch.
#[test]
fn the_default_list_is_the_core_set_and_all_is_opt_in() {
    let text = ask(
        env!("CARGO_BIN_EXE_ljos-mcp"),
        &[],
        None,
        &[
            serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
                "name": "ljos_brief", "arguments": {}
            }}),
        ],
    );
    let names = listed(&text);
    assert_eq!(names, CORE, "{text}");
    let parsed = answers(&text);
    let call = parsed.iter().find(|a| a["id"] == 3).expect(&text);
    let body = call.to_string();
    assert!(
        body.contains("ljos_brief is not in this server's tool set"),
        "{text}"
    );
    assert!(body.contains("LJOS_MCP_TOOLS=all"), "{text}");

    let every = listed(&ask(
        env!("CARGO_BIN_EXE_ljos-mcp"),
        &[],
        Some("all"),
        &[serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})],
    ));
    assert!(every.len() > CORE.len(), "{every:?}");
    assert_eq!(&every[..CORE.len()], CORE, "{every:?}");
    assert!(every.iter().any(|n| n == "ljos_brief"), "{every:?}");
    assert!(every.iter().any(|n| n == "ljos_bump_plan"), "{every:?}");
}

/// The registry entry names the crate, starts it as `ljos mcp`, and its name
/// is the one the README carries as the registry checks it on crates.io.
#[test]
fn server_json_and_the_readme_name_the_same_server() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // A packaged crate has neither file; the check is for the repository.
    let (Ok(entry), Ok(readme)) = (
        std::fs::read_to_string(root.join("server.json")),
        std::fs::read_to_string(root.join("README.md")),
    ) else {
        return;
    };
    let entry: serde_json::Value = serde_json::from_str(&entry).unwrap();
    let name = entry["name"].as_str().unwrap();
    assert_eq!(name, "io.github.leidarljos/ljos");
    let line = format!("- mcp-name: {name}\n");
    assert!(readme.contains(&line), "README lacks {line:?}");
    let package = &entry["packages"][0];
    assert_eq!(package["registryType"], "cargo");
    assert_eq!(package["identifier"], env!("CARGO_PKG_NAME"));
    assert_eq!(package["packageArguments"][0]["value"], "mcp");
    assert_eq!(package["transport"]["type"], "stdio");
}
