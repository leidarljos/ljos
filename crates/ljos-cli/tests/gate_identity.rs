//! A hook call is judged as the conversation its payload names. Many
//! conversations of one user share one runtime directory and one
//! `ljos hookd`, and the hook's own environment may name another of them
//! or none. The sitting `ljos sitting` opened in a conversation's shell
//! lets that conversation's history writes through, and no other's.
#![cfg(target_os = "linux")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const ISSUE: &str = "proj-gate";
const MERGE: &str = "git merge --ff-only FETCH_HEAD";
const CONV_X: &str = "0199a1b2-c3d4-4e5f-8a6b-00000000000x";
const CONV_Y: &str = "0199a1b2-c3d4-4e5f-8a6b-00000000000y";
const CONV_Z: &str = "0199a1b2-c3d4-4e5f-8a6b-00000000000z";

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

/// One user's box: a stub tracker, a repo that is not the tracker, and
/// the runtime directory every conversation shares.
struct Box_ {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    home: PathBuf,
    run: PathBuf,
    repo: PathBuf,
    card: PathBuf,
}

impl Box_ {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("gi")
            .tempdir_in("/tmp")
            .unwrap();
        let root = dir.path().to_path_buf();
        let (bin, home, run, repo) = (
            root.join("bin"),
            root.join("home"),
            root.join("run"),
            root.join("repo"),
        );
        let card = root.join("card.json");
        for d in [&bin, &home, &run, &repo] {
            std::fs::create_dir_all(d).unwrap();
        }
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        script(
            &bin.join("claimdag"),
            "#!/bin/sh\ncase \"$1\" in\n  claim) echo gen=2 ;;\nesac\nexit 0\n",
        );
        script(
            &bin.join("vissue"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  show) if [ \"$2\" = {ISSUE} ] && [ -f '{c}' ]; then cat '{c}'; else echo '{{}}'; fi ;;\n  claims) echo '[]' ;;\n  claim) echo claimed ;;\n  *) echo '{{}}' ;;\nesac\nexit 0\n",
                c = card.display(),
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

    /// `PROGRAM ARGS` with this user's environment and `extra` on top.
    /// No conversation id unless `extra` names one.
    fn command(&self, program: &str, args: &[&str], extra: &[(&str, &str)]) -> Command {
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
            .env("CURSOR_AGENT", "1")
            .envs(extra.iter().copied())
            .current_dir(&self.repo)
            .stdin(Stdio::null());
        cmd
    }

    /// `ljos ARGS` as a tool call of conversation `conv` runs it.
    fn shell(&self, conv: &str, args: &[&str]) -> Output {
        let mut all = vec!["-w", env!("CARGO_BIN_EXE_ljos")];
        all.extend_from_slice(args);
        self.command("setsid", &all, &[("CURSOR_CONVERSATION_ID", conv)])
            .output()
            .unwrap()
    }

    /// The `ljos hook` answer to a `preToolUse` of `line` whose
    /// payload names `payload_conv`, in a hook process with `env`.
    fn hook(&self, payload_conv: &str, line: &str, env: &[(&str, &str)]) -> String {
        self.hook_with(&self.payload(Some(payload_conv.into()), line), env)
    }

    /// A `preToolUse` of `line` whose `conversation_id` is `conv`,
    /// or that has no such field when `conv` is `None`.
    fn payload(&self, conv: Option<serde_json::Value>, line: &str) -> String {
        let mut payload = serde_json::json!({
            "cursor_version": "2.4.0",
            "hook_event_name": "preToolUse",
            "session_id": "cursor-sess-0002bbbb",
            "cwd": self.repo,
            "tool_name": "Shell",
            "tool_input": {"command": line},
        });
        if let Some(conv) = conv {
            payload["conversation_id"] = conv;
        }
        payload.to_string()
    }

    fn hook_with(&self, payload: &str, env: &[(&str, &str)]) -> String {
        let mut cmd = self.command("setsid", &["-w", env!("CARGO_BIN_EXE_ljos"), "hook"], env);
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
        text_of(&child.wait_with_output().unwrap())
    }

    fn card(&self, by: &str) {
        std::fs::write(
            &self.card,
            serde_json::json!({"id": ISSUE, "state": "STARTED", "claimed_by": by}).to_string(),
        )
        .unwrap();
    }

    fn gate_row(&self, conv: &str) -> String {
        let said = text_of(&self.shell(conv, &["doctor", "--all"]));
        said.lines()
            .find(|line| line.contains("\tsitting gate\t"))
            .unwrap_or_else(|| panic!("no sitting gate row:\n{said}"))
            .to_string()
    }
}

/// `ljos hookd` started as conversation `conv` would start it, once its
/// socket answers. Killed on drop.
struct Hookd(Child);

impl Hookd {
    fn start(home: &Box_, conv: &str) -> Self {
        let child = home
            .command(
                env!("CARGO_BIN_EXE_ljos"),
                &["hookd"],
                &[("CURSOR_CONVERSATION_ID", conv)],
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let sock = home.run.join("ljos/hookd.sock");
        let started = Instant::now();
        while std::os::unix::net::UnixStream::connect(&sock).is_err() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "hookd never listened on {}",
                sock.display()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        Self(child)
    }

    fn still_serves(&mut self) -> bool {
        self.0.try_wait().unwrap().is_none()
    }
}

impl Drop for Hookd {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn holder(conv: &str) -> String {
    format!("inky@{}", ljos_cli::session_tag(conv))
}

fn refused(said: &str, what: &str) {
    assert!(
        said.contains("sitting-gate"),
        "{what} was not refused:\n{said}"
    );
    assert!(said.contains("holds no live sitting"), "{what}:\n{said}");
}

fn sits_nowhere(row: &str, what: &str) {
    assert!(row.contains("holds no live sitting"), "{what}:\n{row}");
}

fn allowed(said: &str, what: &str) {
    assert!(
        !said.contains("sitting-gate"),
        "{what} was refused:\n{said}"
    );
}

fn timed_calls(home: &Box_) -> usize {
    std::fs::read_to_string(home.run.join("ljos/hook-times.log"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("hook "))
        .count()
}

#[test]
fn a_hookd_started_by_one_conversation_judges_each_call_as_its_own() {
    let home = Box_::new();
    let mut hookd = Hookd::start(&home, CONV_Y);

    // `ljos sitting` takes the claim this writes once the whole seat
    // answers; the stub box has no pack, so the claim is taken directly.
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let row = home.gate_row(CONV_X);
    assert!(
        row.contains(&format!("this conversation sits on {ISSUE}")),
        "{row}"
    );
    sits_nowhere(&home.gate_row(CONV_Y), "the doctor of Y");

    allowed(
        &home.hook(CONV_X, MERGE, &[]),
        "X's merge, no id in the hook's env",
    );
    allowed(
        &home.hook(CONV_X, MERGE, &[("CURSOR_CONVERSATION_ID", CONV_Y)]),
        "X's merge, Y's id in the hook's env",
    );
    refused(
        &home.hook(CONV_Y, MERGE, &[("CURSOR_CONVERSATION_ID", CONV_Y)]),
        "Y's merge",
    );
    refused(&home.hook(CONV_Z, MERGE, &[]), "Z's merge");
    refused(
        &home.hook(CONV_Z, MERGE, &[("CURSOR_CONVERSATION_ID", CONV_X)]),
        "Z's merge, X's id in the hook's env",
    );

    assert!(hookd.still_serves(), "the hookd started by Y went away");
    assert!(
        timed_calls(&home) >= 5,
        "the calls did not go through hookd"
    );
}

/// The repro from the gatediag1 diagnosis: a live hold for conversation
/// A, written by hand, and A's payload in a hook whose environment does
/// or does not carry A's id.
#[test]
fn the_diag_repro_allows_a_without_its_id_in_the_hook_env() {
    const CONV_A: &str = "cursor-conv-0001aaaa";
    const CONV_B: &str = "cursor-conv-0009bbbb";
    let home = Box_::new();
    home.card(&holder(CONV_A));
    let mut sleeper = Command::new("sleep").arg("300").spawn().unwrap();
    let comm = std::fs::read_to_string(format!("/proc/{}/comm", sleeper.id())).unwrap();
    std::fs::create_dir_all(home.run.join("ljos")).unwrap();
    std::fs::write(
        home.run.join("ljos/hold-live"),
        format!(
            "{}\ninky\n{}\n{}\n2026-10-25T00:00:00Z\n{ISSUE}\n-\n",
            holder(CONV_A),
            sleeper.id(),
            comm.trim()
        ),
    )
    .unwrap();

    let row = home.gate_row(CONV_A);
    assert!(
        row.contains(&format!("this conversation sits on {ISSUE}")),
        "{row}"
    );
    sits_nowhere(&home.gate_row(CONV_B), "the doctor of B");
    allowed(
        &home.hook(CONV_A, MERGE, &[("CURSOR_CONVERSATION_ID", CONV_A)]),
        "A's merge with A's id in the env",
    );
    allowed(
        &home.hook(CONV_A, MERGE, &[]),
        "A's merge without an id in the env",
    );
    refused(&home.hook(CONV_B, MERGE, &[]), "B's merge");

    let _ = sleeper.kill();
    let _ = sleeper.wait();
}

/// A `conversation_id` that names nobody is refused, whatever id the
/// hook's own environment carries. Only a payload without the field
/// falls back to the environment.
#[test]
fn an_unusable_conversation_id_is_refused_not_read_from_the_env() {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let env = [("CURSOR_CONVERSATION_ID", CONV_X)];
    for bad in [
        serde_json::json!(""),
        serde_json::json!("   "),
        serde_json::json!("-"),
        serde_json::json!(null),
        serde_json::json!("null"),
        serde_json::json!("ab"),
        serde_json::json!(format!("{CONV_X}\n")),
        serde_json::json!(7),
    ] {
        let said = home.hook_with(&home.payload(Some(bad.clone()), MERGE), &env);
        refused(&said, &format!("a merge whose conversation_id is {bad}"));
    }
    allowed(
        &home.hook_with(&home.payload(None, MERGE), &env),
        "a merge with no conversation_id, X's id in the env",
    );
}
