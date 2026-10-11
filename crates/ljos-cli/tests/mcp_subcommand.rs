//! `ljos mcp` serves the same MCP server as `ljos-mcp`, for a client that
//! starts a server by its crate's name, and the list opens with the guard
//! and memory tools.

use std::io::Write as _;
use std::process::{Command, Stdio};

#[test]
fn ljos_mcp_serves_and_lists_the_guard_first() {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("mcp")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_RUNTIME_DIR", home.path())
        .env_remove("LJOS_MCP_TOOLS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let answers: Vec<serde_json::Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let init = answers.iter().find(|a| a["id"] == 1).expect(&text);
    assert_eq!(init["result"]["serverInfo"]["name"], "ljos", "{text}");
    let list = answers.iter().find(|a| a["id"] == 2).expect(&text);
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect(&text)
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        names[..5],
        [
            "ljos_rule",
            "ljos_request_approval",
            "ljos_prefer",
            "ljos_remember",
            "ljos_search"
        ],
        "{text}"
    );
    assert!(!names.contains(&"ljos_brief"), "{text}");
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
