//! `ljos hook` hands an `ljos-policyd` 0.6.0 rewrite to Claude Code and
//! Codex as an updated tool input. The policyd here prints the exact
//! bytes 0.6.0 prints for `python x.py` with a PEP 723 block.

ljos_test_boot::harness!();
use std::io::Write;
use std::os::unix::fs::PermissionsExt as _;
use std::process::{Command, Stdio};

const POLICYD: &str = r#"#!/bin/sh
case "$5" in
  "python x.py") printf "allow\trewrite\tbash -c 'uv run x.py'\n" ;;
  *) echo allow ;;
esac
"#;

fn hook(payload: &str) -> serde_json::Value {
    let home = tempfile::tempdir().unwrap();
    let work = home.path().join("w");
    std::fs::create_dir_all(&work).unwrap();
    let policyd = home.path().join("ljos-policyd");
    std::fs::write(&policyd, POLICYD).unwrap();
    std::fs::set_permissions(&policyd, std::fs::Permissions::from_mode(0o755)).unwrap();
    let payload = payload.replace("@W@", work.to_str().unwrap());
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("hook")
        .current_dir(&work)
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("POLICYD_BIN", &policyd)
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("LJOS_GUARD", "on")
        .env("LJOS_OFF", "0")
        .env_remove("POLICYD_REQUIRED")
        .env_remove("LJOS_HOOK_RELAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "ljos hook exited {:?}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8(out.stdout).unwrap();
    serde_json::from_str(printed.trim()).unwrap_or_else(|e| panic!("{e}: {printed}"))
}

#[test]
fn a_policyd_rewrite_reaches_claude_code_and_codex_as_updated_input() {
    let cases = [
        (
            "Claude Code",
            r#"{"session_id":"s-1","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"python x.py","description":"run it"},"cwd":"@W@"}"#,
        ),
        (
            "Codex",
            r#"{"session_id":"s-1","turn_id":"t-1","hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"c-1","tool_input":{"command":"python x.py"},"cwd":"@W@"}"#,
        ),
    ];
    for (runner, payload) in cases {
        let out = hook(payload);
        let specific = &out["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "allow", "{runner}: {out}");
        assert_eq!(
            specific["updatedInput"]["command"], "uv run x.py",
            "{runner}: {out}"
        );
    }
}
