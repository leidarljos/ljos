//! `ljos prefer --user` and `ljos prefer --user --promote` against a fixture
//! lesson, and the guard on the commands those rules name.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

#[path = "common/terminal.rs"]
mod terminal;

fn read_http(stream: &mut impl Read) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = stream.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let mut need = 0usize;
            for line in buf[..at].split(|b| *b == b'\n') {
                let line = std::str::from_utf8(line).unwrap_or("").trim();
                if let Some(value) = line
                    .split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim())
                {
                    need = value.parse().unwrap_or(0);
                }
            }
            if buf.len().saturating_sub(at + 4) >= need {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// One fixture lesson. Records every request line. Nothing is deleted.
fn fixture_pack() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        let _ = listener.set_nonblocking(false);
        for _ in 0..8 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let req = read_http(&mut stream);
            let first = req.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(first.clone());
            let body = if first.contains("GET /v1/atoms") {
                r#"{"atoms":[{"id":"lesson-ocaisa","kind":"lesson","text":"Never post or reply to Alan (ocaisa).","due_at":"2026-10-18T00:00:00Z"}]}"#
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (format!("http://{addr}"), seen)
}

/// A variable an agent runner stamps on its shell, or one ljos reads to
/// tell which runner and session it is in. The person at a terminal has
/// none, whatever shell ran `cargo test`.
fn runner_var(key: &str) -> bool {
    [
        "CURSOR_",
        "CLAUDE",
        "CODEX_",
        "GROK_",
        "GEMINI_CLI",
        "OPENCODE_",
        "AMP_",
    ]
    .iter()
    .any(|p| key.starts_with(p))
        || [
            "CLOUD_AGENT_ID",
            "AGENT_TOOL_NAME",
            "LJOS_HARNESS_SESSION",
            "LJOS_SESSION_ID",
        ]
        .contains(&key)
        || (key != "XDG_SESSION_ID"
            && ["_SESSION_ID", "_THREAD_ID", "_CONVERSATION_ID"]
                .iter()
                .any(|s| key.ends_with(s)))
}

struct Home {
    root: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            root: tempfile::TempDir::new().unwrap(),
        }
    }

    fn ljos(&self) -> Command {
        self.program(env!("CARGO_BIN_EXE_ljos"))
    }

    /// `ljos ARGS` typed at a terminal of the person's own.
    fn typed(&self, args: &[&str]) -> Command {
        let mut cmd = self.program(terminal::PYTHON);
        cmd.args(terminal::shape(
            "terminal",
            env!("CARGO_BIN_EXE_ljos"),
            args,
        ));
        cmd
    }

    fn program(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env("HOME", self.root.path())
            .env("XDG_CONFIG_HOME", self.root.path().join(".config"))
            .env("XDG_DATA_HOME", self.root.path().join(".local/share"))
            .env("XDG_CACHE_HOME", self.root.path().join(".cache"))
            .env("XDG_STATE_HOME", self.root.path().join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("PACKSET_URL", "off")
            .env("LJOS_HOLD", "rohit")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_GUARD", "on")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env_remove("POLICYD_BIN")
            .env_remove("POLICYD_REQUIRED")
            .env_remove("LJOS_SEAT")
            .stdin(Stdio::null());
        for (key, _) in std::env::vars_os() {
            if key.to_str().is_some_and(runner_var) {
                cmd.env_remove(key);
            }
        }
        let _ = std::fs::create_dir_all(self.root.path().join("run"));
        cmd
    }

    fn rules(&self) -> std::path::PathBuf {
        self.root.path().join(".config/ljos/user-rules.json")
    }
}

fn output_text(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn prefer_user_is_read_by_a_second_seat_and_promote_copies_a_fixture_lesson() {
    let home = Home::new();
    let wrote = home
        .typed(&[
            "prefer",
            "--user",
            "--why",
            "he asked not to be pinged",
            "--deny-mention",
            "ocaisa",
            "--deny-name",
            "Alan",
            "--deny-reply-to",
            "ocaisa",
            "never post or reply to Alan (ocaisa)",
        ])
        .env("LJOS_SEAT", "claude")
        .output()
        .unwrap();
    let wrote_text = output_text(&wrote);
    assert!(wrote.status.success(), "{wrote_text}");
    assert!(wrote_text.contains("user rule"), "{wrote_text}");
    assert!(home.rules().is_file());

    let mut start = home
        .ljos()
        .args(["hook"])
        .env("LJOS_SEAT", "codex")
        .env("PACKSET_WORKSPACE", "other-seat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    start
        .stdin
        .take()
        .unwrap()
        .write_all(
            br#"{"hook_event_name":"SessionStart","session_id":"codex-1","source":"startup"}"#,
        )
        .unwrap();
    let started = start.wait_with_output().unwrap();
    let started_text = output_text(&started);
    assert!(started.status.success(), "{started_text}");
    assert!(
        started_text.contains("never post or reply to Alan"),
        "{started_text}"
    );

    let mut prompt = home
        .ljos()
        .args(["hook"])
        .env("LJOS_SEAT", "opencode")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    prompt
        .stdin
        .take()
        .unwrap()
        .write_all(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"opencode-1","prompt":"open an easybuild pull request"}"#,
        )
        .unwrap();
    let prompted = prompt.wait_with_output().unwrap();
    let prompted_text = output_text(&prompted);
    assert!(prompted.status.success(), "{prompted_text}");
    assert!(
        prompted_text.contains("never post or reply to Alan"),
        "{prompted_text}"
    );

    let (url, seen) = fixture_pack();
    let promoted = home
        .ljos()
        .args([
            "prefer",
            "--user",
            "--promote",
            "lesson-ocaisa",
            "--why",
            "Alan asked that this hold for every seat",
        ])
        .env("PACKSET_URL", &url)
        .env("PACKSET_TIMEOUT_MS", "2000")
        .output()
        .unwrap();
    let promoted_text = output_text(&promoted);
    assert!(promoted.status.success(), "{promoted_text}");
    assert!(
        promoted_text.contains("Never post or reply to Alan"),
        "{promoted_text}"
    );
    let raw = std::fs::read_to_string(home.rules()).unwrap();
    assert!(
        raw.contains("Never post or reply to Alan (ocaisa)."),
        "{raw}"
    );
    assert!(raw.contains("\"author\": \"rohit\""), "{raw}");
    assert!(
        raw.contains("Alan asked that this hold for every seat"),
        "{raw}"
    );
    assert!(!raw.contains("\"due_at\""), "{raw}");
    let lines = seen.lock().unwrap().clone();
    assert!(
        lines.iter().any(|line| line.contains("GET /v1/atoms")),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|line| !line.contains("DELETE")),
        "{lines:?}"
    );
}

fn policy(home: &Home, line: &str) -> std::process::Output {
    home.ljos()
        .args(["policy", "--fail-on-deny", line])
        .output()
        .unwrap()
}

#[test]
fn the_guard_denies_a_mention_and_a_reply_and_allows_another_comment() {
    let home = Home::new();
    let wrote = home
        .ljos()
        .args([
            "prefer",
            "--user",
            "--why",
            "he asked not to be pinged",
            "--deny-mention",
            "ocaisa",
            "--deny-name",
            "Alan",
            "--deny-reply-to",
            "ocaisa",
            "never post or reply to Alan (ocaisa)",
        ])
        .output()
        .unwrap();
    assert!(wrote.status.success(), "{}", output_text(&wrote));

    let mention = policy(&home, "gh pr comment 12 --body 'please ask @ocaisa'");
    let mention_text = output_text(&mention);
    assert!(!mention.status.success(), "{mention_text}");
    assert!(mention_text.contains("deny:"), "{mention_text}");

    let reply = policy(
        &home,
        "gh api --method POST repos/org/repo/issues/comments/7/replies -f body=seen -F in_reply_to_user=ocaisa",
    );
    let reply_text = output_text(&reply);
    assert!(!reply.status.success(), "{reply_text}");
    assert!(reply_text.contains("deny:"), "{reply_text}");

    let allowed = policy(&home, "gh pr comment 12 --body 'tests passed'");
    let allowed_text = output_text(&allowed);
    assert!(allowed.status.success(), "{allowed_text}");
    assert!(!allowed_text.contains("deny:"), "{allowed_text}");
}

/// The variables an agent runner stamps on the shell that runs the suite.
const RUNNER_ENV: &[(&str, &str)] = &[
    ("CURSOR_CONVERSATION_ID", "0199a1b2-c3d4-e5f6-runner"),
    ("CURSOR_AGENT", "1"),
    ("CLAUDECODE", "1"),
    ("CLAUDE_CODE_SESSION_ID", "0199a1b2-claude-session"),
    ("CODEX_THREAD_ID", "0199a1b2-codex-thread"),
];

/// Run one test of this file again in a child that carries `env`.
fn rerun(test: &str, env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", test, "--test-threads=1"]);
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.output().unwrap()
}

/// These tests write a user rule as the person at a terminal. A runner's
/// variables on the shell that runs `cargo test` must not make them an agent's.
#[test]
fn the_user_rule_tests_pass_under_a_runner_environment() {
    for test in [
        "prefer_user_is_read_by_a_second_seat_and_promote_copies_a_fixture_lesson",
        "the_guard_denies_a_mention_and_a_reply_and_allows_another_comment",
    ] {
        for env in [RUNNER_ENV, &[("CURSOR_CONVERSATION_ID", "x")]] {
            let out = rerun(test, env);
            let said = output_text(&out);
            assert!(out.status.success(), "{test} under {env:?}:\n{said}");
            assert!(said.contains("1 passed"), "{test} under {env:?}:\n{said}");
        }
    }
}
