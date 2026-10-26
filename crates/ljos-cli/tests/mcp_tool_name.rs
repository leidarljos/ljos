//! A cloud agent's GitHub write meets the user rule.
//! Runs the built `ljos` binary.
#![cfg(target_os = "linux")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "common/seat_root.rs"]
mod seat_root;

struct Hab {
    root: PathBuf,
    home: PathBuf,
    run: PathBuf,
    repo: PathBuf,
}

impl Drop for Hab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn hab(name: &str) -> Hab {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp).join(format!(
        "m3-{name}-{}-{}",
        std::process::id(),
        nanos % 1_000_000_000
    ));
    let hab = Hab {
        home: root.join("home"),
        run: root.join("run"),
        repo: root.join("repo"),
        root,
    };
    for dir in [&hab.home, &hab.run, &hab.repo] {
        std::fs::create_dir_all(dir).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hab.run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let git = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&hab.repo)
        .status()
        .unwrap();
    assert!(git.success());
    hab
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn hook(hab: &Hab, pack: Option<u16>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("hook")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &hab.home)
        .env("XDG_CONFIG_HOME", hab.home.join(".config"))
        .env("XDG_DATA_HOME", hab.home.join(".local/share"))
        .env("XDG_CACHE_HOME", hab.home.join(".cache"))
        .env("XDG_STATE_HOME", hab.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &hab.run)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_PROJECT", "probe")
        .env("LJOS_SEAT", "probe-seat")
        .env("LJOS_HOOKD", "off")
        .current_dir(&hab.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match pack {
        Some(port) => {
            cmd.env("PACKSET_URL", format!("http://127.0.0.1:{port}"))
                .env("PACKSET_TOKEN", "t".repeat(32));
        }
        None => {
            cmd.env("PACKSET_URL", "off");
        }
    }
    cmd
}

fn call(mut cmd: Command, payload: &str, limit: Duration) -> (Duration, Output) {
    let started = Instant::now();
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(payload.as_bytes());
    drop(stdin);
    while child.try_wait().unwrap().is_none() && started.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = started.elapsed();
    let _ = child.kill();
    (took, child.wait_with_output().unwrap())
}

const ALAN: &str = r#"{"rules":[{"id":"ur-ocaisa","text":"never post or reply to Alan (ocaisa)","author":"rohit","reason":"he asked not to be pinged","deny_mention":{"handles":["ocaisa"],"names":["Alan"]},"deny_reply_to":{"handles":["ocaisa"],"names":["Alan"]},"written_at":"2026-10-25T00:00:00Z","updated_at":"2026-10-25T00:00:00Z","basis":"person"}]}"#;

fn rules_path(hab: &Hab) -> PathBuf {
    hab.home.join(".config/ljos/user-rules.json")
}

/// A Cursor cloud agent's GitHub comment
/// arrives as preToolUse `MCP:add_issue_comment` with no server field.
/// The rule that denies `mcp__github__add_issue_comment` applies to it.
#[test]
fn a_cloud_agent_github_write_meets_the_user_rule() {
    let hab = hab("cloudmcp");
    put(&rules_path(&hab), ALAN);
    let payload = |tool: &str| {
        serde_json::json!({
            "cursor_version": "2.4.0",
            "conversation_id": "c-cloud",
            "hook_event_name": "preToolUse",
            "tool_name": tool,
            "tool_input": {"owner": "org", "repo": "repo", "issue_number": 7,
                "body": "please ask @ocaisa"},
            "cwd": hab.repo,
            "tool_use_id": "t-cloud",
        })
        .to_string()
    };
    let (_, named) = call(
        hook(&hab, None),
        &payload("mcp__github__add_issue_comment"),
        Duration::from_secs(5),
    );
    let named = String::from_utf8_lossy(&named.stdout).to_string();
    assert!(named.contains("\"deny\""), "control: {named}");
    let (_, cloud) = call(
        hook(&hab, None),
        &payload("MCP:add_issue_comment"),
        Duration::from_secs(5),
    );
    let cloud = String::from_utf8_lossy(&cloud.stdout).to_string();
    assert!(
        cloud.contains("\"deny\""),
        "MCP:add_issue_comment passed the rule: {cloud}"
    );
}
