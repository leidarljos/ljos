//! Seat keys from the seat's own MCP config.

use super::*;

fn review_env() -> (EnvLock, tempfile::TempDir) {
    let lock = test_env_lock();
    let dir = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
        std::env::set_var("HOME", dir.path());
        std::env::set_var("LJOS_GUARD", "on");
        std::env::set_var("LJOS_HOLD", "rohit");
        std::env::set_var("LJOS_SEAT", "claude");
        std::env::set_var("PACKSET_URL", "off");
        std::env::set_var("PACKSET_WORKSPACE", "seat-a");
        std::env::set_var("LJOS_USER_RULES_SYNC", "off");
        for key in ["LJOS_MCP_CONFIG", "CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
            std::env::remove_var(key);
        }
    }
    PERSON_AT_TERMINAL.with(|p| p.set(true));
    (lock, dir)
}

fn put(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const SEAT_ENTRY: &str = r#"{"mcpServers":{"ljos":{"command":"ljos-mcp","args":[]}}}"#;

/// The shipped plugin starts the seat as `ljos-plugin` with `ljos-mcp`
/// among its arguments. That entry is the seat server.
#[test]
fn the_plugin_launcher_entry_is_a_seat_server() {
    let (_lock, dir) = review_env();
    put(
        &dir.path().join(".cursor/mcp.json"),
        r#"{"mcpServers":{"ljos":{"command":"/opt/ljos/bin/ljos-plugin","args":["ljos-mcp"]}}}"#,
    );
    assert!(
        subagent::is_seat_mcp_tool("mcp__ljos__ljos_note"),
        "the plugin launcher entry is not a seat key: {:?}",
        cloud::seat_mcp_keys()
    );
}

/// Claude Code names a tool after the server key with every character
/// outside `[A-Za-z0-9_-]` replaced by `_`.
#[test]
fn a_punctuated_seat_key_matches_its_normalised_tool_name() {
    let (_lock, dir) = review_env();
    put(
        &dir.path().join(".claude.json"),
        r#"{"mcpServers":{"ljos.seat":{"command":"ljos-mcp","args":[]}}}"#,
    );
    assert!(
        subagent::is_seat_mcp_tool("mcp__ljos_seat__ljos_note"),
        "{:?}",
        cloud::seat_mcp_keys()
    );
}

/// `~/.claude.json` stores per-project history and grows past the read
/// cap. The seat key in it still counts.
#[test]
fn a_large_claude_json_still_names_the_seat() {
    let (_lock, dir) = review_env();
    let history: Vec<_> = (0..1500)
        .map(|i| serde_json::json!({"display": format!("{i} {}", "x".repeat(1000))}))
        .collect();
    let doc = serde_json::json!({
        "mcpServers": {"ljos": {"command": "ljos-mcp", "args": []}},
        "projects": {"/w": {"history": history}},
    });
    put(&dir.path().join(".claude.json"), &doc.to_string());
    assert!(
        subagent::is_seat_mcp_tool("mcp__ljos__ljos_note"),
        "a large ~/.claude.json hid the seat key: {:?}",
        cloud::seat_mcp_keys()
    );
}

/// A Cursor file that does not parse must not hide the seat key in
/// `~/.claude.json`.
#[test]
fn a_cursor_file_ljos_cannot_parse_does_not_hide_the_claude_seat() {
    let (_lock, dir) = review_env();
    put(&dir.path().join(".claude.json"), SEAT_ENTRY);
    put(
        &dir.path().join(".cursor/mcp.json"),
        "{\n  // remote GitHub server\n  \"mcpServers\": {\"github\": {\"url\": \"https://api.githubcopilot.com/mcp/\"}}\n}\n",
    );
    assert!(
        subagent::is_seat_mcp_tool("mcp__ljos__ljos_note"),
        "{:?}",
        cloud::seat_mcp_keys()
    );
}
