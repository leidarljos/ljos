//! A hook returns inside its wall budget when the pack accepts and never
//! answers, and when the transcript is large. The notes are written by
//! the detached session finish, after the hook has returned. With no
//! policyd to answer, a hook allows inside the guard's wait.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn blackhole() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn large_transcript(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("rollout.jsonl");
    let mut file = std::fs::File::create(&path).unwrap();
    let pad = "y".repeat(120_000);
    let start = 1_700_000_000_i64;
    for minute in 0..53 {
        let line = format!(
            "{{\"timestamp\":\"2026-10-11T09:{minute:02}:00Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call_output\",\"output\":\"minute {minute} {start} {pad}\"}}}}\n"
        );
        file.write_all(line.as_bytes()).unwrap();
    }
    let fact = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"The root cause was a shared temp dir between tests. Always give each test its own tempdir on this box.\"}]}}\n";
    file.write_all(fact.as_bytes()).unwrap();
    path
}

fn run_hook(
    home: &std::path::Path,
    url: &str,
    args: &[&str],
    payload: &str,
    extra: &[(&str, &str)],
) -> (Duration, std::process::Output) {
    let run = home.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("USER", "person")
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "30000")
        .env("LJOS_SEAT", "probe-seat")
        .env("LJOS_GUARD", "on")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("LJOS_HOOKD", "off")
        .env("VISSUE_NO_ROUTE", "1")
        .envs(extra.iter().copied())
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(payload.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (started.elapsed(), out)
}

fn assert_within(name: &str, took: Duration) {
    let budget = Duration::from_millis(ljos_cli::HOOK_WALL_BUDGET_MS + 400);
    assert!(
        took < budget,
        "{name} took {took:?}, past the {budget:?} wall budget"
    );
}

#[test]
fn a_large_transcript_and_a_quiet_pack_do_not_hold_the_hook() {
    let home = tempfile::tempdir().unwrap();
    let transcript = large_transcript(home.path());
    let len = std::fs::metadata(&transcript).unwrap().len();
    assert!(len > 5_000_000, "fixture is {len} bytes");
    let url = blackhole();
    let session_end = serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "budget-session",
        "transcript_path": transcript,
        "cwd": home.path(),
    })
    .to_string();
    let (took, out) = run_hook(home.path(), &url, &["hook"], &session_end, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_within("SessionEnd", took);

    let stop = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "budget-session",
        "transcript_path": transcript,
        "cwd": home.path(),
    })
    .to_string();
    let (took, out) = run_hook(home.path(), &url, &["hook"], &stop, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_within("Stop", took);

    let pre = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "budget-session",
        "cwd": home.path(),
        "tool_name": "Bash",
        "tool_input": {"command": "git commit -m \"save the work\""},
    })
    .to_string();
    let (took, out) = run_hook(home.path(), &url, &["hook"], &pre, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_within("PreToolUse git commit", took);

    let (took, out) = run_hook(
        home.path(),
        &url,
        &["policy", "--", "git", "commit", "-m", "save the work"],
        "",
        &[],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_within("policy git commit", took);
}

/// With no policyd to answer, a shell call is allowed inside the guard's
/// wait and never told deny or ask: when the binary is missing, and when
/// it hangs the way a client does whose daemon is not listening. The
/// Claude Code, Cursor and relayed Gemini hooks are each checked.
#[cfg(unix)]
#[test]
fn a_hook_with_no_policyd_listening_allows_inside_the_guard_wait() {
    use std::os::unix::fs::PermissionsExt;
    const GUARD_WAIT: Duration = Duration::from_millis(500);
    let home = tempfile::tempdir().unwrap();
    let hang = home.path().join("ljos-policyd");
    std::fs::write(&hang, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&hang, std::fs::Permissions::from_mode(0o755)).unwrap();
    let missing = home.path().join("gone/ljos-policyd");
    let wait = format!("{}", GUARD_WAIT.as_secs_f32());
    // Two seconds covers a debug binary starting on a loaded runner. The
    // bound stays under the five-second default wait, so a hook that
    // ignored `LJOS_GUARD_TIMEOUT` and waited out the hang would fail.
    let bound = GUARD_WAIT + Duration::from_secs(2);
    assert!(bound < ljos_cli::GUARD_TIMEOUT);
    let cwd = home.path().to_str().unwrap();
    let claude = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "no-policyd",
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_input": {"command": "echo hi"},
    })
    .to_string();
    let cursor = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "no-policyd",
        "hook_event_name": "beforeShellExecution",
        "command": "echo hi",
        "cwd": cwd,
    })
    .to_string();
    let gemini = serde_json::json!({
        "hook_event_name": "BeforeTool",
        "session_id": "no-policyd",
        "cwd": cwd,
        "tool_name": "run_shell_command",
        "tool_input": {"command": "echo hi"},
    })
    .to_string();
    let calls: [(&str, &[&str], &str); 3] = [
        ("claude", &["hook"], &claude),
        ("cursor", &["hook"], &cursor),
        ("gemini", &["hook", "--runner", "gemini"], &gemini),
    ];
    for (policyd, kind) in [(&hang, "hangs"), (&missing, "missing")] {
        for (name, args, payload) in calls {
            let (took, out) = run_hook(
                home.path(),
                "off",
                args,
                payload,
                &[
                    ("POLICYD_BIN", policyd.to_str().unwrap()),
                    ("LJOS_GUARD_TIMEOUT", wait.as_str()),
                ],
            );
            let stdout = String::from_utf8_lossy(&out.stdout);
            let said = format!("{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
            assert!(out.status.success(), "{kind} {name}: {said}");
            assert!(
                !ljos_cli::answer_denies(&stdout)
                    && !stdout.contains("\"ask\"")
                    && !stdout.contains("\"block\""),
                "{kind} {name}: {said}"
            );
            assert!(said.contains("allowed"), "{kind} {name}: {said}");
            assert!(
                took < bound,
                "{kind} {name} took {took:?}, past {bound:?}: {said}"
            );
        }
    }
}
