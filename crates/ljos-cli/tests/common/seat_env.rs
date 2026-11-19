//! Drop the session and seat variables on a spawned `ljos`.
//!
//! The seat name is read from that process alone. These removes keep a
//! parent that still has the variables from naming the child.

use std::process::Command;

pub const KEYS: &[&str] = &[
    "AGENT_ID",
    "CLAUDE_CODE_SESSION_ID",
    "CLOUD_AGENT_ID",
    "CODEX_THREAD_ID",
    "CODEX_SESSION_ID",
    "CODESPACE_NAME",
    "CURSOR_AGENT_ID",
    "CURSOR_CONVERSATION_ID",
    "GITHUB_RUN_ID",
    "LJOS_SEAT",
];

pub fn clear(cmd: &mut Command) {
    for key in KEYS {
        cmd.env_remove(key);
    }
}
