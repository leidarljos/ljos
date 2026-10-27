//! A relay that cannot read its stdin used to exit 1 and print nothing.
//! Nine runners treat that as an allow. A tool call is denied in the
//! runner's own contract. A prompt still exits 1.

ljos_test_boot::harness!();
use std::io::Write;
use std::process::{Command, Stdio};

fn hook(runner: &str, event: Option<&str>, bytes: &[u8]) -> std::process::Output {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("hook").arg("--runner").arg(runner);
    if let Some(event) = event {
        cmd.arg("--event").arg(event);
    }
    cmd.env("HOME", home.path())
        .env("LJOS_OFF", "1")
        .env_remove("CURSOR_AGENT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let out = child.wait_with_output().unwrap();
    // Keep the temp dir until the process has exited.
    drop(home);
    out
}

#[test]
fn invalid_utf8_on_a_tool_hook_denies() {
    let wind = hook("windsurf", None, &[0xff, 0xfe]);
    let err = String::from_utf8_lossy(&wind.stderr);
    assert_eq!(
        wind.status.code(),
        Some(2),
        "stdout {} stderr {err}",
        String::from_utf8_lossy(&wind.stdout)
    );
    assert!(err.contains("utf-8") || err.contains("utf8"), "{err}");

    let gemini = hook("gemini", None, &[0xff, 0xfe]);
    let out = String::from_utf8_lossy(&gemini.stdout);
    assert_eq!(
        gemini.status.code(),
        Some(0),
        "exit 2 alone allows on Gemini: {out}"
    );
    assert!(out.contains(r#""decision":"deny""#), "{out}");
}

#[test]
fn invalid_utf8_on_a_prompt_hook_is_not_a_deny() {
    let out = hook("gemini", Some("BeforeAgent"), &[0xff, 0xfe]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}
