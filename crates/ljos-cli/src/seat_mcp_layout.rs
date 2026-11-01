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

/// Claude Code names a plugin server's tools
/// `mcp__plugin_<plugin>_<server>__<tool>`. An install under the home
/// `.claude` directory is the seat.
#[test]
fn a_claude_plugin_install_names_the_seat() {
    let (_lock, dir) = review_env();
    let root = dir
        .path()
        .join(".claude/plugins/cache/leidarljos/ljos/0.33.0");
    put(
        &root.join(".claude-plugin/plugin.json"),
        include_str!("../../../.claude-plugin/plugin.json"),
    );
    put(
        &dir.path().join(".claude/plugins/installed_plugins.json"),
        &serde_json::json!({"version": 2, "plugins": {"ljos@leidarljos": [
            {"scope": "user", "installPath": root, "version": "0.33.0"}]}})
        .to_string(),
    );
    put(
        &dir.path().join(".claude/settings.json"),
        r#"{"enabledPlugins":{"ljos@leidarljos":true}}"#,
    );
    assert!(
        subagent::is_seat_mcp_tool("mcp__plugin_ljos_ljos__ljos_note"),
        "a plugin-installed seat lost its early answer: {:?}",
        cloud::seat_mcp_keys()
    );
}

/// `CLAUDE_CONFIG_DIR` is chosen by the process. A `.claude.json` there
/// does not name the seat, even when it lists `ljos-mcp`.
#[test]
fn claude_config_dir_does_not_name_the_seat() {
    let (_lock, dir) = review_env();
    let cfg = dir.path().join("claude-cfg");
    put(&cfg.join(".claude.json"), SEAT_ENTRY);
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &cfg) };
    assert!(
        !subagent::is_seat_mcp_tool("mcp__ljos__ljos_note"),
        "CLAUDE_CONFIG_DIR named the seat: {:?}",
        cloud::seat_mcp_keys()
    );
}

/// `CODEX_HOME` is chosen by the process. A `config.toml` there does
/// not name the seat.
#[test]
fn codex_home_does_not_name_the_seat() {
    let (_lock, dir) = review_env();
    let cfg = dir.path().join("codex-cfg");
    put(
        &cfg.join("config.toml"),
        "[mcp_servers.ljos]\ncommand = \"ljos-mcp\"\n",
    );
    unsafe { std::env::set_var("CODEX_HOME", &cfg) };
    assert!(
        !subagent::is_seat_mcp_tool("mcp__ljos__ljos_note"),
        "CODEX_HOME named the seat: {:?}",
        cloud::seat_mcp_keys()
    );
}

/// The home file may name `github` as the seat server. A Cursor payload
/// whose `command` is a different binary is still a foreign tool, and
/// the user rule applies.
#[test]
fn a_payload_command_that_is_not_the_seat_is_a_foreign_tool() {
    let (_lock, dir) = review_env();
    put(
        &dir.path().join(".cursor/mcp.json"),
        r#"{"mcpServers":{"github":{"command":"ljos-mcp","args":[]}}}"#,
    );
    put(
        &dir.path().join("config/ljos/user-rules.json"),
        r#"{"rules":[{"id":"ur-ocaisa","text":"never post or reply to Alan (ocaisa)","author":"rohit","reason":"he asked not to be pinged","deny_mention":{"handles":["ocaisa"],"names":["Alan"]},"deny_reply_to":{"handles":["ocaisa"],"names":["Alan"]},"written_at":"2026-10-25T00:00:00Z","updated_at":"2026-10-25T00:00:00Z","basis":"person"}]}"#,
    );
    let payload = serde_json::json!({
        "hook_event_name": "beforeMCPExecution",
        "tool_name": "mcp__github__ljos_issue_comment",
        "command": "github-mcp-server",
        "tool_input": {"owner": "o", "repo": "r", "issue_number": 1, "body": "cc @ocaisa"},
    })
    .to_string();
    let hit = user_rules::guard_payload(&payload).expect("foreign command skipped the rule");
    assert_eq!(hit.verdict, "deny", "{hit:?}");
}
