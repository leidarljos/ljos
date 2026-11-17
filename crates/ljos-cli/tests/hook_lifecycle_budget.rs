//! Lifecycle hooks share their deadline with credential lookups and audit workers.

ljos_test_boot::harness!();

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Fixture {
    home: tempfile::TempDir,
    payload: String,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        for path in ["config/ljos", "run", "state", "data"] {
            std::fs::create_dir_all(home.path().join(path)).unwrap();
        }
        let transcript = home.path().join("transcript.jsonl");
        let final_message = "The parser update is implemented and all component tests pass. The requested behavior and outputs are verified.";
        let entries = [
            serde_json::json!({"type":"user","message":{"content":"Implement the parser update and test it."}}),
            serde_json::json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"seat","name":"Bash","input":{"command":"ljos recall fixture"}}]}}),
            serde_json::json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"test","name":"Bash","input":{"command":"cargo test"}}]}}),
            serde_json::json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"test","content":"test result: FAILED. 1 passed; 1 failed"}]}}),
            serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":final_message}]}}),
        ];
        std::fs::write(
            &transcript,
            entries
                .iter()
                .map(|entry| format!("{entry}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let payload = serde_json::json!({
            "hook_event_name":"Stop",
            "session_id":"lifecycle-budget-fixture",
            "cwd":home.path(),
            "transcript_path":transcript,
            "last_assistant_message":final_message
        })
        .to_string();
        Self { home, payload }
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.home.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn configure(&self, body: &str) {
        std::fs::write(self.home.path().join("config/ljos/jev.toml"), body).unwrap();
    }

    fn run(&self) -> (Duration, Output) {
        let start = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .arg("hook")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_DATA_HOME", self.home.path().join("data"))
            .env("XDG_RUNTIME_DIR", self.home.path().join("run"))
            .env("LJOS_GUARD", "on")
            .env("LJOS_GUARD_MODE", "strict")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .current_dir(self.home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(self.payload.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        (start.elapsed(), output)
    }
}

fn bounded(took: Duration, output: &Output) {
    assert!(output.status.success(), "{output:?}");
    let cap = Duration::from_millis(ljos_cli::HOOK_WALL_BUDGET_MS + 400);
    assert!(
        took < cap,
        "lifecycle hook took {took:?}, beyond {cap:?}: {output:?}"
    );
}

#[test]
fn stop_does_not_repeat_slow_key_waits_past_its_deadline() {
    let fixture = Fixture::new();
    let key = fixture.script("key", "exec sleep 30");
    fixture.configure(&format!("enabled = true\nkey_cmd = [{key:?}]\n"));
    let (took, output) = fixture.run();
    bounded(took, &output);
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn stop_audit_workers_share_the_hook_deadline() {
    let fixture = Fixture::new();
    let judge = fixture.script("judge", "cat >/dev/null\nexec sleep 30");
    fixture.configure(&format!(
        "enabled = true\ncache_days = 0\n[route]\naudit = ['first', 'second']\n[judges.first]\nbackend = 'command'\ncommand = [{judge:?}]\nbudget_ms = 30000\n[judges.second]\nbackend = 'command'\ncommand = [{judge:?}]\nbudget_ms = 30000\n"
    ));
    let (took, output) = fixture.run();
    bounded(took, &output);
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn a_responsive_audit_still_blocks_a_false_green_claim() {
    let fixture = Fixture::new();
    let judge = fixture.script(
        "judge",
        "cat >/dev/null\nprintf '%s\\n' '{\"claims_complete\":0.99,\"tests_green\":0.0,\"deferral\":0.0}'",
    );
    fixture.configure(&format!(
        "enabled = true\nbackend = 'command'\ncommand = [{judge:?}]\ncache_days = 0\n"
    ));
    let (took, output) = fixture.run();
    bounded(took, &output);
    let verdict: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(verdict["decision"], "block");
    assert!(verdict["reason"]
        .as_str()
        .unwrap()
        .contains("last test run shown is red"));
}
