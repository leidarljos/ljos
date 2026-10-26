//! Review finding 2: an empty rules file mid-rewrite denies benign writes.
//! Fails on 508a7c54.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

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

/// Finding 2. Bots on one box share `user-rules.json`, and every
/// sessionStart can store it through the one `user-rules.json.tmp`. Two
/// stores at once rename a truncated temp file into place. A benign
/// GitHub comment must not be denied while that happens.
#[test]
fn concurrent_rule_stores_never_deny_a_benign_github_write() {
    let (_lock, _dir) = review_env();
    alan();
    let rows = user_rules::read_all().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stores = Arc::new(AtomicUsize::new(0));
    let writers: Vec<_> = (0..4)
        .map(|_| {
            let rows = rows.clone();
            let stop = Arc::clone(&stop);
            let stores = Arc::clone(&stores);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let _ = user_rules::replace_stored(&rows);
                    stores.fetch_add(1, Ordering::Relaxed);
                }
            })
        })
        .collect();
    let benign = r#"{"cursor_version":"2.4.0","conversation_id":"c","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"tests passed"}}"#;
    let started = std::time::Instant::now();
    let (mut calls, mut denied, mut sample) = (0usize, 0usize, String::new());
    while started.elapsed() < std::time::Duration::from_secs(3) {
        calls += 1;
        if let Some(hit) = user_rules::guard_payload(benign) {
            denied += 1;
            sample = hit.reason;
        }
    }
    stop.store(true, Ordering::Relaxed);
    for writer in writers {
        writer.join().unwrap();
    }
    assert_eq!(
        denied,
        0,
        "{denied} of {calls} benign GitHub writes denied during {} stores: {sample}",
        stores.load(Ordering::Relaxed)
    );
}
