//! Under an agent runner, a git verb that writes history waits for a
//! sitting: a live claim this seat holds on an open tracker ticket. The
//! refusal names `ljos sitting ID`. `LJOS_OFF` and a temporary `HOME` do
//! not lift it. The person at their own terminal is never refused.

#[path = "common/terminal.rs"]
mod terminal;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SESSION: &str = "5b0c2a71-9d1e-4f3a-8c6b-2e7f1a0d9c43";
const ISSUE: &str = "proj-gate";

fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn text_of(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

struct Seat {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    home: PathBuf,
    run: PathBuf,
    repo: PathBuf,
    card: PathBuf,
}

impl Seat {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let bin = root.join("bin");
        let home = root.join("home");
        let run = root.join("run");
        let repo = root.join("repo");
        let card = root.join("card.json");
        for d in [&bin, &home, &run, &repo] {
            std::fs::create_dir_all(d).unwrap();
        }
        script(
            &bin.join("claimdag"),
            "#!/bin/sh\ncase \"$1\" in\n  claim) echo gen=2 ;;\nesac\nexit 0\n",
        );
        script(
            &bin.join("vissue"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  show) if [ \"$2\" = {ISSUE} ] && [ -f '{}' ]; then cat '{}'; else echo '{{}}'; fi ;;\n  claim) echo claimed ;;\nesac\nexit 0\n",
                card.display(),
                card.display()
            ),
        );
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(init.status.success(), "{}", text_of(&init));
        Self {
            _dir: dir,
            bin,
            home,
            run,
            repo,
            card,
        }
    }

    fn command(&self, program: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("LJOS_SEAT", "inky")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("GROK_SESSION_ID", SESSION)
            .current_dir(&self.repo)
            .stdin(Stdio::null());
        cmd
    }

    /// `ljos ARGS` as a runner's tool call runs it: detached from any
    /// terminal, with the runner's session variable set.
    fn runner(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut all = vec!["-w", env!("CARGO_BIN_EXE_ljos")];
        all.extend_from_slice(args);
        let mut cmd = self.command("setsid", &all);
        cmd.envs(env.iter().copied());
        cmd.output().unwrap()
    }

    fn policy(&self, line: &str, env: &[(&str, &str)]) -> String {
        let out = self.runner(&["policy", line], env);
        assert!(out.status.success(), "{line}\n{}", text_of(&out));
        text_of(&out)
    }

    fn hook(&self, line: &str) -> String {
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "session_id": SESSION,
            "cwd": self.repo,
            "tool_name": "Bash",
            "tool_input": {"command": line},
        })
        .to_string();
        let mut cmd = self.command("setsid", &["-w", env!("CARGO_BIN_EXE_ljos"), "hook"]);
        let mut child = cmd
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
        assert!(out.status.success(), "{line}\n{}", text_of(&out));
        text_of(&out)
    }

    fn card(&self, state: &str, by: &str) {
        std::fs::write(
            &self.card,
            serde_json::json!({"id": ISSUE, "state": state, "claimed_by": by}).to_string(),
        )
        .unwrap();
    }

    fn hold_record(&self) -> PathBuf {
        std::fs::read_dir(self.run.join("ljos"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("hold-"))
            })
            .expect("the claim wrote a hold record")
    }
}

fn refused(said: &str, line: &str) {
    assert!(
        said.contains("sitting-gate"),
        "`{line}` was not refused:\n{said}"
    );
    assert!(said.contains("ljos sitting ID"), "{line}:\n{said}");
}

fn not_refused(said: &str, line: &str) {
    assert!(
        !said.contains("sitting-gate"),
        "`{line}` was refused:\n{said}"
    );
}

#[test]
fn a_runner_without_a_live_sitting_writes_no_history() {
    let seat = Seat::new();
    for line in [
        "git commit -m x",
        "git merge topic",
        "git rebase main",
        "git cherry-pick abc123",
        "git push origin topic",
        "cd . && git -C . commit --amend --no-edit",
    ] {
        let said = seat.policy(line, &[]);
        refused(&said, line);
        assert!(said.contains("holds no live sitting"), "{said}");
    }
    let said = seat.policy("git commit -m x", &[("LJOS_OFF", "1")]);
    refused(&said, "git commit with LJOS_OFF");
    let said = seat.policy("git commit -m x", &[("LJOS_GUARD", "on")]);
    refused(&said, "git commit with the guard on");
    for line in [
        "git status",
        "git log -1",
        "git push --dry-run origin topic",
    ] {
        not_refused(&seat.policy(line, &[]), line);
    }
    refused(&seat.hook("git commit -m x"), "the hook's git commit");

    let out = seat.runner(&["claim", ISSUE], &[]);
    assert!(out.status.success(), "{}", text_of(&out));
    let holder = format!("inky@{}", ljos_cli::session_tag(SESSION));
    seat.card("STARTED", &holder);
    for line in ["git commit -m x", "git push origin topic"] {
        not_refused(&seat.policy(line, &[]), line);
    }
    not_refused(&seat.hook("git commit -m x"), "the hook's git commit");

    seat.card("DONE", &holder);
    let said = seat.policy("git commit -m x", &[]);
    refused(&said, "a commit on a finished ticket");
    assert!(said.contains(&format!("{ISSUE} is DONE")), "{said}");

    seat.card("STARTED", "other@abc");
    let said = seat.policy("git commit -m x", &[]);
    refused(&said, "a commit on another seat's ticket");
    assert!(said.contains("claimed by other@abc"), "{said}");

    seat.card("STARTED", &holder);
    let record = seat.hold_record();
    let text = std::fs::read_to_string(&record).unwrap();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    lines[2] = "999999999".into();
    std::fs::write(&record, lines.join("\n") + "\n").unwrap();
    let said = seat.policy("git commit -m x", &[]);
    refused(&said, "a commit whose conversation is gone");
    assert!(said.contains("holds no live sitting"), "{said}");
}

#[test]
fn the_person_at_a_terminal_is_not_refused() {
    let seat = Seat::new();
    let ljos = env!("CARGO_BIN_EXE_ljos");
    for line in ["git commit -m x", "git push origin develop"] {
        let typed = terminal::shape("terminal", ljos, &["policy", line]);
        let out = seat.command(terminal::PYTHON, &typed).output().unwrap();
        assert!(out.status.success(), "{line}\n{}", text_of(&out));
        not_refused(&text_of(&out), line);
    }
}
