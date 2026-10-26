//! Review finding 4 (open issue 8): `MCP:<tool>` reaches no user rule.
//! Fails on 508a7c54.

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

const ALAN: &str = r#"{"rules":[{"id":"ur-ocaisa","text":"never post or reply to Alan (ocaisa)","author":"rohit","reason":"he asked not to be pinged","deny_mention":{"handles":["ocaisa"],"names":["Alan"]},"deny_reply_to":{"handles":["ocaisa"],"names":["Alan"]},"written_at":"2026-10-25T00:00:00Z","updated_at":"2026-10-25T00:00:00Z","basis":"person"}]}"#;

fn alan() {
    put(&user_rules::rules_path().unwrap(), ALAN);
}

/// Finding 4 (open issue 8). Cursor names an HTTP MCP tool `MCP:<tool>`
/// on preToolUse and sends no server field. A GitHub comment that names
/// @ocaisa is the same write as `mcp__github__add_issue_comment`.
#[test]
fn a_cursor_mcp_github_write_on_pre_tool_use_meets_the_user_rule() {
    let (_lock, _dir) = review_env();
    alan();
    let payload = |tool: &str| {
        serde_json::json!({
            "cursor_version": "2.4.0",
            "conversation_id": "c-cloud",
            "hook_event_name": "preToolUse",
            "tool_name": tool,
            "tool_input": {"owner": "org", "repo": "repo", "issue_number": 7, "body": "please ask @ocaisa"},
            "tool_use_id": "t-cloud",
        })
        .to_string()
    };
    let named = user_rules::guard_payload(&payload("mcp__github__add_issue_comment"));
    assert_eq!(named.map(|r| r.verdict).as_deref(), Some("deny"), "control");
    let cloud = user_rules::guard_payload(&payload("MCP:add_issue_comment"));
    assert_eq!(
        cloud.map(|r| r.verdict).as_deref(),
        Some("deny"),
        "MCP:add_issue_comment reached no user rule"
    );
}
