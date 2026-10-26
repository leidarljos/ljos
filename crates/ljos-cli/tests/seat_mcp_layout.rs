//! A plugin seat call is answered early. Runs the built `ljos` binary.
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

/// A plugin seat call is answered early. A working directory that is
/// gone does not deny the seat's own tool.
#[test]
fn a_plugin_seat_call_is_answered_early_even_from_a_gone_directory() {
    let hab = hab("plugin");
    let root = hab
        .home
        .join(".claude/plugins/cache/leidarljos/ljos/0.33.0");
    put(
        &root.join(".claude-plugin/plugin.json"),
        include_str!("../../../.claude-plugin/plugin.json"),
    );
    put(
        &hab.home.join(".claude/plugins/installed_plugins.json"),
        &serde_json::json!({"version": 2, "plugins": {"ljos@leidarljos": [
            {"scope": "user", "installPath": root, "version": "0.33.0"}]}})
        .to_string(),
    );
    put(
        &hab.home.join(".claude/settings.json"),
        r#"{"enabledPlugins":{"ljos@leidarljos":true}}"#,
    );
    put(&hab.run.join("ljos/hook-trace"), "");
    let payload = serde_json::json!({
        "session_id": "s-plugin",
        "transcript_path": "/x",
        "cwd": hab.root.join("removed-worktree"),
        "hook_event_name": "PreToolUse",
        "tool_name": "mcp__plugin_ljos_ljos__ljos_note",
        "tool_input": {"issue": "demo-1", "text": "progress"},
        "tool_use_id": "toolu_plugin",
    })
    .to_string();
    let (took, out) = call(hook(&hab, None), &payload, Duration::from_secs(5));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(
        !stdout.contains("\"deny\""),
        "the seat's own tool was refused: {stdout}"
    );
    let trace = std::fs::read_to_string(hab.run.join("ljos/hook-trace.jsonl")).unwrap_or_default();
    assert!(
        trace.is_empty(),
        "the seat call took the slow path ({took:?}): {trace}"
    );
}
