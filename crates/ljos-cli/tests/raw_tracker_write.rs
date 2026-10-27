//! Under an agent runner the guard refuses a raw `vissue` verb that writes
//! and names the ljos verb to run in its place: the raw verb skips the
//! seat check, the parent's project and the tracker commit. Reads stay
//! allowed, and the `vissue` children ljos runs itself are not refused.

ljos_test_boot::harness!();
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// `ljos ARGS` the way an agent runner's tool call runs it: a runner
/// variable set and no controlling terminal, so no person is at it.
fn as_agent(home: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("LJOS_GUARD", "on")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("CURSOR_AGENT", "1")
        .env_remove("POLICYD_BIN")
        .env_remove("POLICYD_REQUIRED")
        .stdin(Stdio::null());
    // SAFETY: setsid is async-signal-safe and the closure touches nothing
    // of the parent's.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = std::fs::create_dir_all(home.join("run"));
    cmd
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn policy(home: &Path, line: &str) -> Output {
    as_agent(home, &["policy", "--fail-on-deny", line])
        .output()
        .unwrap()
}

#[test]
fn a_raw_tracker_write_under_a_runner_is_refused_with_the_ljos_verb() {
    let home = tempfile::tempdir().unwrap();
    for (line, instead) in [
        (
            r#"cd ~/Software && vissue create --parent ljos-gkq2 "a child""#,
            r#"ljos file "TITLE" --parent ljos-gkq2"#,
        ),
        (r#"vissue q -p ljos "a ticket""#, "ljos file"),
        ("vissue close ljos-ab12", "ljos finish ljos-ab12 --close"),
        (
            "vissue cancel ljos-ab12",
            "ljos finish ljos-ab12 --status cancelled --close",
        ),
        ("vissue claim ljos-ab12", "ljos sitting ljos-ab12"),
        ("vissue release ljos-ab12", "ljos release ljos-ab12"),
        (r#"vissue note ljos-ab12 "halfway""#, "ljos note ljos-ab12"),
        ("vissue append ljos-ab12 --file r.md", "ljos note ljos-ab12"),
        (
            "vissue vote ljos-ab12 --for ship --used none 2>&1",
            "ljos vote ljos-ab12 --for ship --used none`",
        ),
        ("/usr/local/bin/vissue refile ljos-ab12 tools", "ljos file"),
        (
            "VISSUE_AGENT=saddle vissue update ljos-ab12 -s done",
            "ljos finish ljos-ab12 --close",
        ),
        (
            "vissue update ljos-ab12 -s STARTED",
            "ljos sitting ljos-ab12",
        ),
        ("vissue update ljos-ab12 -t urgent", "ljos note ljos-ab12"),
    ] {
        let out = policy(home.path(), line);
        let said = text(&out);
        assert!(!out.status.success(), "{line} was allowed:\n{said}");
        assert!(said.contains("deny:"), "{line}:\n{said}");
        assert!(
            said.contains(&format!("Run `{instead}")),
            "{line} did not name {instead}:\n{said}"
        );
    }
}

#[test]
fn a_tracker_read_under_a_runner_stays_allowed() {
    let home = tempfile::tempdir().unwrap();
    for line in [
        "vissue show ljos-ab12",
        "vissue list -p ljos",
        "vissue ready -p ljos",
        "vissue tree ljos-gkq2",
        "vissue whoami",
        "vissue claims --by codex",
        "vissue vote ljos-ab12",
        "vissue vote ljos-ab12 --json",
        "echo vissue close ljos-ab12",
    ] {
        let out = policy(home.path(), line);
        let said = text(&out);
        assert!(out.status.success(), "{line} was refused:\n{said}");
        assert!(!said.contains("deny:"), "{line}:\n{said}");
    }
}

#[test]
fn the_runner_hook_refuses_a_raw_tracker_write() {
    let home = tempfile::tempdir().unwrap();
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "raw-tracker-write-0001",
        "cwd": home.path(),
        "tool_name": "Bash",
        "tool_input": {"command": "vissue close ljos-ab12"},
    });
    let mut hook = as_agent(home.path(), &["hook"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    hook.stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let out = hook.wait_with_output().unwrap();
    let said = text(&out);
    assert!(said.contains("deny"), "{said}");
    assert!(
        said.contains("Run `ljos finish ljos-ab12 --close` instead."),
        "{said}"
    );
}

/// ljos runs `vissue` itself for its own verbs, outside any runner's hook,
/// so a runner that may not run `vissue note` still notes through ljos.
#[test]
fn the_vissue_children_ljos_runs_are_not_refused() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = home.path().join("vissue.log");
    let stub = bin.join("vissue");
    std::fs::write(
        &stub,
        format!("#!/bin/sh\necho \"vissue $*\" >> '{}'\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = as_agent(home.path(), &["note", "ljos-ab12", "halfway", "there"])
        .env("PATH", path)
        .env("LJOS_SEAT", "rohit")
        .env("LJOS_TRACKER_GIT", "off")
        .output()
        .unwrap();
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        calls.contains("vissue note ljos-ab12 halfway there"),
        "{calls}\n{said}"
    );
}
