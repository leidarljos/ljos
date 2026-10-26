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

    /// Shells a process `runner` starts one after another, as a runner
    /// starts each of its conversations' tool calls under itself. The
    /// runner runs with `env`; each step's shell is started with the
    /// conversation id the step names, or the runner's own. `$LJOS` is the
    /// binary and `$PAYLOAD` a file holding `payload`. Says what every
    /// shell said, and the last one's exit status as `EXIT n`.
    fn under(
        &self,
        runner: &Path,
        env: &[(&str, &str)],
        payload: &str,
        steps: &[(Option<&str>, &str)],
    ) -> String {
        let file = self.run.join("payload.json");
        std::fs::write(&file, payload).unwrap();
        let mut all = vec![("LJOS", env!("CARGO_BIN_EXE_ljos"))];
        let file = file.display().to_string();
        all.push(("PAYLOAD", &file));
        all.extend_from_slice(env);
        let steps = serde_json::to_string(steps).unwrap();
        let out = self
            .command(runner.to_str().unwrap(), &["-c", RUNNER, &steps], &all)
            .output()
            .unwrap();
        text_of(&out)
    }

    /// `sh -c LINE` started with `env`, as a runner starts one tool call.
    fn sh(&self, env: &[(&str, &str)], line: &str) -> Output {
        let mut all = vec![("LJOS", env!("CARGO_BIN_EXE_ljos"))];
        all.extend_from_slice(env);
        self.command("sh", &["-c", line], &all).output().unwrap()
    }

    /// `bash -c LINE` started with `env`, including `SHLVL` as bash sets it.
    fn bash(&self, env: &[(&str, &str)], line: &str) -> Output {
        let mut all = vec![("LJOS", env!("CARGO_BIN_EXE_ljos"))];
        all.extend_from_slice(env);
        self.command("bash", &["-c", line], &all).output().unwrap()
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

const RUNNER: &str = "
import json, os, subprocess, sys
rc = 0
for conv, line in json.loads(sys.argv[1]):
    env = dict(os.environ)
    if conv is not None:
        env['CURSOR_CONVERSATION_ID'] = conv
    rc = subprocess.call(['sh', '-c', line], env=env)
print('EXIT', rc)
";

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

/// A hook call or shell that names its conversation is not judged as the
/// holder `LJOS_HOLD` names in the environment.
#[test]
fn a_named_call_ignores_ljos_hold_in_the_hook_env() {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let hold = holder(CONV_X);
    let env = [("LJOS_HOLD", hold.as_str())];
    refused(
        &home.hook(CONV_Z, MERGE, &env),
        "Z's merge with X's LJOS_HOLD",
    );
    let mut z_hold = home.command(
        "setsid",
        &[
            "-w",
            env!("CARGO_BIN_EXE_ljos"),
            "policy",
            "--",
            "git",
            "merge",
            "--ff-only",
            "FETCH_HEAD",
        ],
        &[
            ("CURSOR_CONVERSATION_ID", CONV_Z),
            ("LJOS_HOLD", hold.as_str()),
        ],
    );
    let said = text_of(&z_hold.output().unwrap());
    assert!(said.contains("deny: sitting-gate"), "{said}");
    let mut z_gh = home.command(
        "setsid",
        &["-w", env!("CARGO_BIN_EXE_ljos"), "git-hook", "pre-commit"],
        &[
            ("CURSOR_CONVERSATION_ID", CONV_Z),
            ("LJOS_HOLD", hold.as_str()),
        ],
    );
    let out = z_gh.output().unwrap();
    assert!(
        !out.status.success() && text_of(&out).contains("sitting-gate"),
        "{}",
        text_of(&out)
    );
}

/// An unbound `ljos hook` call (a `PreToolUse` payload with no
/// `conversation_id`, as `force_push_sample` runs) honors `LJOS_HOLD`
/// even when the hook process inherits `CURSOR_AGENT` or
/// `CURSOR_CONVERSATION_ID` from an outer shell.
#[test]
fn an_unbound_hook_call_with_ljos_hold_honors_the_configured_holder() {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let hold = holder(CONV_X);
    let unbound = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": MERGE},
        "cwd": home.repo,
    })
    .to_string();
    allowed(
        &home.hook_with(
            &unbound,
            &[
                ("LJOS_HOLD", hold.as_str()),
                ("CURSOR_AGENT", "1"),
                ("CURSOR_CONVERSATION_ID", CONV_Z),
            ],
        ),
        "unbound PreToolUse hook with LJOS_HOLD=X under outer CURSOR_CONVERSATION_ID=Z",
    );
}

/// The gate's conversation comes from `conversation_id` alone. A payload
/// whose `session_id` or `sessionId` is X's conversation id but that has
/// no `conversation_id` is judged by the hook's environment, here Y's.
#[test]
fn a_session_id_never_names_the_gates_conversation() {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    for key in ["session_id", "sessionId"] {
        let payload = serde_json::json!({
            "cursor_version": "2.4.0",
            "hook_event_name": "preToolUse",
            key: CONV_X,
            "cwd": home.repo,
            "tool_name": "Shell",
            "tool_input": {"command": MERGE},
        })
        .to_string();
        refused(
            &home.hook_with(&payload, &[("CURSOR_CONVERSATION_ID", CONV_Y)]),
            &format!("a merge whose {key} is X's id, Y's id in the env"),
        );
    }
}

/// A subagent has its own conversation id, `sand-sub...`, and no parent
/// in its payload. The runner starts its tools under the process that
/// opened the parent's sitting; that process, whose own environment names
/// X and whose pid X's hold recorded, lets it in, for the shell hook and
/// the git hook alike. Any other id is judged by its name alone, so Z is
/// not let in for being a child of X's process. A shared parent that
/// names no conversation, or a wrapper such as `ljos`, links nobody, so
/// stranger Z and Z's subagent under that shared parent are refused.
#[test]
fn a_subagent_writes_under_its_parents_sitting_and_z_does_not() {
    const SUB: &str = "sand-subagent-0001cccc";
    let home = Box_::new();
    home.card(&holder(CONV_X));
    let python = PathBuf::from("/usr/bin/python3");
    let x = [("CURSOR_CONVERSATION_ID", CONV_X)];
    let claim = format!("\"$LJOS\" claim {ISSUE} >/dev/null 2>&1");
    let hook = "\"$LJOS\" hook < \"$PAYLOAD\"";
    let commit = "\"$LJOS\" git-hook pre-commit";

    for (shell, what) in [(CONV_X, "the parent's"), (CONV_Y, "another conversation's")] {
        let said = home.under(
            &python,
            &x,
            &home.payload(Some(SUB.into()), MERGE),
            &[(None, &claim), (Some(shell), hook)],
        );
        allowed(
            &said,
            &format!("the subagent's merge, {what} id in the hook env"),
        );
    }
    let said = home.under(&python, &x, "", &[(None, &claim), (Some(SUB), commit)]);
    assert!(
        said.contains("EXIT 0"),
        "the subagent's commit was refused:\n{said}"
    );
    let said = home.under(&python, &x, "", &[(None, &claim), (Some(CONV_Z), commit)]);
    assert!(
        said.contains("sitting-gate") && said.contains("holds no live sitting"),
        "Z's commit under X's process was not refused:\n{said}"
    );
    refused(
        &home.under(
            &python,
            &x,
            &home.payload(Some(CONV_Z.into()), MERGE),
            &[(None, &claim), (None, hook)],
        ),
        "Z's merge under X's process",
    );

    // Shared id-less parent: X claims in its shell, then Z's subagent and Z
    // run under the same shared parent and are both refused.
    refused(
        &home.under(
            &python,
            &[],
            &home.payload(Some(SUB.into()), MERGE),
            &[(Some(CONV_X), &claim), (Some(CONV_Z), hook)],
        ),
        "Z's subagent merge under a shared parent that names no conversation",
    );
    refused(
        &home.under(
            &python,
            &[],
            &home.payload(Some(CONV_Z.into()), MERGE),
            &[(Some(CONV_X), &claim), (Some(CONV_Z), hook)],
        ),
        "Z's merge under a shared parent that names no conversation",
    );
    let said = home.under(
        &python,
        &[],
        "",
        &[(Some(CONV_X), &claim), (Some(SUB), commit)],
    );
    assert!(
        said.contains("sitting-gate"),
        "Z's subagent commit under a shared parent that names no conversation was not refused:\n{said}"
    );

    let fake = home.run.join("fake");
    std::fs::create_dir_all(&fake).unwrap();
    std::os::unix::fs::symlink(&python, fake.join("ljos")).unwrap();
    let forge = format!(
        "mkdir -p \"$XDG_RUNTIME_DIR/ljos\"; printf '%s\\n' '{}' inky \"$PPID\" ljos 2026-10-25T00:00:00Z {ISSUE} - > \"$XDG_RUNTIME_DIR/ljos/hold-fake\"",
        holder(CONV_X)
    );
    let said = home.under(
        &fake.join("ljos"),
        &x,
        &home.payload(Some(SUB.into()), MERGE),
        &[(None, &forge), (None, hook)],
    );
    refused(
        &said,
        "a subagent's merge under an ljos process that names X",
    );
}

/// A Claude Code hook names its conversation by `session_id` and the
/// conversation's shells by the stamp in the environment. A subagent's
/// call carries the parent's `session_id` and an `agent_id`, and passes
/// on the parent's sitting. A `session_id` that names nobody is refused,
/// whatever the environment says; a payload with no session key at all
/// is judged by the environment, as before.
#[test]
fn a_claude_subagent_passes_and_a_session_id_that_names_nobody_is_refused() {
    const P: &str = "claude-sess-0001aaaa";
    let home = Box_::new();
    let p = [("CLAUDE_SESSION_ID", P)];
    let sat = home
        .command(
            "setsid",
            &["-w", env!("CARGO_BIN_EXE_ljos"), "claim", ISSUE],
            &p,
        )
        .output()
        .unwrap();
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(P));
    let claude = |session: Option<serde_json::Value>| {
        let mut payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "agent_id": "agent-0001",
            "tool_name": "Bash",
            "tool_input": {"command": MERGE},
            "cwd": home.repo,
        });
        if let Some(session) = session {
            payload["session_id"] = session;
        }
        payload.to_string()
    };
    allowed(
        &home.hook_with(&claude(Some(P.into())), &p),
        "the Claude subagent's merge",
    );
    allowed(
        &home.hook_with(&claude(None), &p),
        "a Claude merge with no session key",
    );
    for bad in [
        serde_json::json!(""),
        serde_json::json!("-"),
        serde_json::json!(null),
        serde_json::json!("ab"),
    ] {
        refused(
            &home.hook_with(&claude(Some(bad.clone())), &p),
            &format!("a Claude merge whose session_id is {bad}"),
        );
    }

    // Claude Task subagent when the shell has no session id stamped:
    // the parent sits under the runner process (comm=python3), and the
    // subagent's merge passes through ancestry.
    let home2 = Box_::new();
    let python = PathBuf::from("/usr/bin/python3");
    let claim = format!("\"$LJOS\" claim {ISSUE} >/dev/null 2>&1");
    let hook = "\"$LJOS\" hook < \"$PAYLOAD\"";
    let sub_payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": P,
        "agent_id": "agent-0001",
        "tool_name": "Bash",
        "tool_input": {"command": MERGE},
        "cwd": home2.repo,
    })
    .to_string();
    home2.card("inky");
    let said = home2.under(
        &python,
        &[("CLAUDECODE", "1")],
        &sub_payload,
        &[(None, &claim), (None, hook)],
    );
    allowed(
        &said,
        "a Claude Task subagent merge when the shell carries no session id",
    );
}

/// The record a hook keeps of a shell call, for the `ljos` line it runs,
/// is filed under the session the gate and the ballot read the call as:
/// a call's `conversation_id`, not its `session_id`, and a Claude
/// call's `session_id` with its `agent_id`.
#[test]
fn a_hooks_call_record_is_filed_under_the_calls_conversation() {
    let home = Box_::new();
    let calls = home.run.join("ljos/agent-calls/shell");
    let mut cursor: serde_json::Value =
        serde_json::from_str(&home.payload(Some(CONV_X.into()), "ljos doctor")).unwrap();
    cursor["tool_use_id"] = "tu-00000001".into();
    home.hook_with(&cursor.to_string(), &[]);
    assert!(calls.join(CONV_X).is_dir(), "no record under X");
    assert!(
        !calls.join("cursor-sess-0002bbbb").exists(),
        "a record under the session_id"
    );

    const P: &str = "claude-sess-0001aaaa";
    let claude = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": P,
        "agent_id": "agent-0001",
        "tool_use_id": "tu-00000002",
        "tool_name": "Bash",
        "tool_input": {"command": "ljos doctor"},
        "cwd": home.repo,
    });
    home.hook_with(&claude.to_string(), &[]);
    let kept: String = std::fs::read_dir(calls.join(P))
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect();
    assert!(kept.contains("agent-0001"), "{kept}");
}

/// What `ljos seat` says this process holds under.
fn seat_holder(said: &str) -> String {
    said.lines()
        .find_map(|line| line.strip_prefix("holder\t"))
        .unwrap_or_else(|| panic!("no holder line:\n{said}"))
        .to_string()
}

/// A process cannot speak for another conversation by setting a variable
/// in its own environment, in an inline `bash -c` override, via `exec env`,
/// or in an intermediate non-shell program that spawns or `os.execve`s
/// `git commit`. `LJOS_HARNESS_SESSION` names no conversation at all.
#[test]
fn a_processs_own_env_does_not_outrank_its_shells_stamp() {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let hooks = home.run.join("githooks");
    std::fs::create_dir_all(&hooks).unwrap();
    script(
        &hooks.join("pre-commit"),
        "#!/bin/sh\nexec \"$LJOS\" git-hook pre-commit\n",
    );
    let commit = format!(
        "git -c core.hooksPath={} -c user.name=t -c user.email=t@e commit -q --allow-empty -m t",
        hooks.display()
    );
    let x = [("CURSOR_CONVERSATION_ID", CONV_X)];
    let z = [("CURSOR_CONVERSATION_ID", CONV_Z)];

    let own = text_of(&home.sh(&x, "\"$LJOS\" doctor --all"));
    assert!(
        own.contains(&format!("this conversation sits on {ISSUE}")),
        "{own}"
    );
    let own = home.sh(&x, &commit);
    assert!(own.status.success(), "X's own commit: {}", text_of(&own));
    let own_bash = home.bash(&x, "\"$LJOS\" git-hook pre-commit");
    assert!(
        own_bash.status.success(),
        "X's bash git-hook: {}",
        text_of(&own_bash)
    );

    for forge in [
        format!("CURSOR_CONVERSATION_ID={CONV_X}"),
        format!("export CURSOR_CONVERSATION_ID={CONV_X};"),
        format!("LJOS_HARNESS_SESSION={CONV_X}"),
        format!("CURSOR_CONVERSATION_ID={CONV_X} LJOS_HARNESS_SESSION={CONV_X}"),
        "env -u CURSOR_CONVERSATION_ID".to_string(),
        "CURSOR_CONVERSATION_ID=sand-subagent-0009zzzz".to_string(),
    ] {
        let said = text_of(&home.sh(&z, &format!("{forge} \"$LJOS\" doctor --all")));
        sits_nowhere(&said, &format!("Z's doctor with {forge}"));
        let said = text_of(&home.sh(
            &z,
            &format!("{forge} \"$LJOS\" policy -- git merge --ff-only FETCH_HEAD"),
        ));
        assert!(
            said.contains("deny: sitting-gate"),
            "Z's policy with {forge}:\n{said}"
        );
        let said = text_of(&home.sh(&z, &format!("{forge} \"$LJOS\" seat")));
        assert_eq!(seat_holder(&said), holder(CONV_Z), "Z's seat with {forge}");
        let out = home.sh(&z, &format!("{forge} {commit}"));
        assert!(
            !out.status.success() && text_of(&out).contains("sitting-gate"),
            "Z's commit with {forge} was not refused:\n{}",
            text_of(&out)
        );
        let bash_line = if forge.starts_with("export ") {
            format!("{forge} \"$LJOS\" git-hook pre-commit; exit $?")
        } else {
            format!("{forge} \"$LJOS\" git-hook pre-commit")
        };
        let out_bash = home.bash(&z, &bash_line);
        assert!(
            !out_bash.status.success() && text_of(&out_bash).contains("sitting-gate"),
            "Z's bash git-hook with {forge} was not refused:\n{}",
            text_of(&out_bash)
        );
    }

    let exec_env = home.bash(
        &z,
        &format!("exec env CURSOR_CONVERSATION_ID={CONV_X} \"$LJOS\" git-hook pre-commit"),
    );
    assert!(
        !exec_env.status.success() && text_of(&exec_env).contains("sitting-gate"),
        "Z's bash exec env git-hook was not refused:\n{}",
        text_of(&exec_env)
    );

    // Intermediate Python wrapper under Z's shell that sets
    // CURSOR_CONVERSATION_ID=X (or removes it, or sets sand-sub) and calls
    // ljos git-hook, ljos policy, or git commit (both spawn and os.execve).
    for py_env in [
        format!("e['CURSOR_CONVERSATION_ID'] = '{CONV_X}'"),
        "e.pop('CURSOR_CONVERSATION_ID', None)".to_string(),
        "e['CURSOR_CONVERSATION_ID'] = 'sand-subagent-0009zzzz'".to_string(),
    ] {
        let py_hook = format!(
            "/usr/bin/python3 -c \"import os, subprocess, sys; e=dict(os.environ); {py_env}; r=subprocess.run([e['LJOS'], 'git-hook', 'pre-commit'], env=e, capture_output=True, text=True); sys.stderr.write(r.stderr); sys.exit(r.returncode)\""
        );
        let out = home.sh(&z, &py_hook);
        assert!(
            !out.status.success() && text_of(&out).contains("sitting-gate"),
            "Python wrapper git-hook pre-commit ({py_env}) was not refused:\n{}",
            text_of(&out)
        );

        let py_pol = format!(
            "/usr/bin/python3 -c \"import os, subprocess, sys; e=dict(os.environ); {py_env}; r=subprocess.run([e['LJOS'], 'policy', '--', 'git', 'merge', '--ff-only', 'FETCH_HEAD'], env=e, capture_output=True, text=True); sys.stdout.write(r.stdout); sys.exit(r.returncode)\""
        );
        let said = text_of(&home.sh(&z, &py_pol));
        assert!(
            said.contains("deny: sitting-gate"),
            "Python wrapper policy ({py_env}) was not refused:\n{said}"
        );

        let py_commit_spawn = format!(
            "exec /usr/bin/python3 -c \"import os, subprocess, sys; e=dict(os.environ); {py_env}; r=subprocess.run(['git', '-c', 'core.hooksPath={}', '-c', 'user.name=t', '-c', 'user.email=t@e', 'commit', '-q', '--allow-empty', '-m', 't'], env=e, capture_output=True, text=True); sys.stderr.write(r.stderr); sys.exit(r.returncode)\"",
            hooks.display()
        );
        let out = home.bash(&z, &py_commit_spawn);
        assert!(
            !out.status.success() && text_of(&out).contains("sitting-gate"),
            "Python spawn git commit ({py_env}) was not refused:\n{}",
            text_of(&out)
        );

        let py_commit_exec = format!(
            "exec /usr/bin/python3 -c \"import os; e=dict(os.environ); {py_env}; os.execvpe('git', ['git', '-c', 'core.hooksPath={}', '-c', 'user.name=t', '-c', 'user.email=t@e', 'commit', '-q', '--allow-empty', '-m', 't'], e)\"",
            hooks.display()
        );
        let out = home.bash(&z, &py_commit_exec);
        assert!(
            !out.status.success() && text_of(&out).contains("sitting-gate"),
            "Python execve git commit ({py_env}) was not refused:\n{}",
            text_of(&out)
        );
    }

    // X's own spawned git commit from Python is allowed.
    let x_spawn = format!(
        "exec /usr/bin/python3 -c \"import os, subprocess, sys; e=dict(os.environ); e['CURSOR_CONVERSATION_ID']='{CONV_X}'; r=subprocess.run(['git', '-c', 'core.hooksPath={}', '-c', 'user.name=t', '-c', 'user.email=t@e', 'commit', '-q', '--allow-empty', '-m', 't'], env=e, capture_output=True, text=True); sys.stderr.write(r.stderr); sys.exit(r.returncode)\"",
        hooks.display()
    );
    let out = home.bash(&x, &x_spawn);
    assert!(
        out.status.success(),
        "X's Python spawn git commit was refused:\n{}",
        text_of(&out)
    );

    refused(
        &home.hook_with(
            &home.payload(Some("".into()), MERGE),
            &[("LJOS_HARNESS_SESSION", CONV_X)],
        ),
        "an empty conversation_id, X in LJOS_HARNESS_SESSION",
    );
    refused(
        &home.hook_with(
            &home.payload(None, MERGE),
            &[
                ("LJOS_HARNESS_SESSION", CONV_X),
                ("CURSOR_CONVERSATION_ID", CONV_Z),
            ],
        ),
        "no conversation_id, X in LJOS_HARNESS_SESSION, Z's id in the env",
    );
}

/// A `hold-*` record forged with another conversation's tag does not pass
/// when the tracker row carries the real conversation's tag, and a shell
/// command that redirects into a `hold-*` file in the runtime directory is
/// refused by the command check.
#[test]
fn a_forged_sitting_record_with_another_conversations_tag_is_refused() {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));

    let write_cmd = format!("printf 'x' > {}/ljos/hold-zz", home.run.display());
    let shell_payload = serde_json::json!({
        "cursor_version": "2.4.0",
        "hook_event_name": "beforeShellExecution",
        "conversation_id": CONV_Z,
        "cwd": home.repo,
        "command": write_cmd,
    })
    .to_string();
    let said = home.hook_with(&shell_payload, &[]);
    assert!(
        said.contains("sitting-gate") && said.contains("\"permission\":\"deny\""),
        "a shell write to a hold-* record under XDG_RUNTIME_DIR/ljos was not refused:\n{said}"
    );

    let real = std::fs::read_dir(home.run.join("ljos"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("hold-"))
        })
        .unwrap();
    let lines: Vec<String> = std::fs::read_to_string(&real)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    let mut forged = lines;
    forged[0] = holder(CONV_Z);
    let forged_path = home.run.join("ljos/hold-zz");
    std::fs::write(&forged_path, forged.join("\n") + "\n").unwrap();
    let said = home.hook(CONV_Z, MERGE, &[]);
    assert!(
        said.contains("sitting-gate") && said.contains(&format!("claimed by {}", holder(CONV_X))),
        "Z's merge after writing hold-zz with Z's tag against X's tracker claim:\n{said}"
    );
    let _ = std::fs::remove_file(&forged_path);
}

/// When `ljos-mcp` runs without a conversation id in its environment,
/// `ljos_sitting` claims as `SEAT@TAG` when `_meta` names the
/// conversation and as the bare seat when `_meta` does not; in both
/// cases, the conversation's own merge is allowed and another
/// conversation's merge is refused.
#[test]
fn an_mcp_sitting_covers_the_conversations_merge_with_and_without_meta() {
    let home = Box_::new();
    let org_dir = home.run.join("tracker/Software/proj");
    std::fs::create_dir_all(&org_dir).unwrap();
    let seed_tracker = || {
        std::fs::write(
            org_dir.join("issues.org"),
            format!(
                "#+TITLE: proj issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n* TODO [#C] the merge\n:PROPERTIES:\n:ID:         {ISSUE}\n:CREATED:    [2026-10-25 Sun]\n:END:\n"
            ),
        )
        .unwrap();
    };
    seed_tracker();
    let cache = home.home.join(".cache/ljos");
    std::fs::create_dir_all(&cache).unwrap();
    for name in [
        "ljos",
        "ljos-policyd",
        "vissue-cli",
        "vissue-mcp",
        "deedar-cli",
        "claimdag-cli",
        "packset",
        "packset-embed",
    ] {
        std::fs::write(cache.join(format!("crate-{name}")), "99.0.0\n").unwrap();
    }
    for name in ["deedar", "packset", "packsetd", "ljos-policyd"] {
        script(
            &home.bin.join(name),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  --version|version) echo '{name} 99.0.0 (phronesis)' ;;\n  *) echo ok ;;\nesac\nexit 0\n"
            ),
        );
    }
    script(
        &home.bin.join("claimdag"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'claimdag 99.0.0' ;;\n  list) echo '[]' ;;\n  claim) echo gen=2 ;;\n  *) echo ok ;;\nesac\nexit 0\n",
    );
    let card_path = home.card.display().to_string();
    script(
        &home.bin.join("vissue"),
        &format!(
            "#!/usr/bin/env python3\nimport json, os, pathlib, sys\n\
             card = pathlib.Path({card_path:?})\n\
             cmd = sys.argv[1] if len(sys.argv) > 1 else ''\n\
             if cmd == '--version':\n\tprint('vissue 99.0.0')\n\
             elif cmd == 'vote':\n\tprint('--used --confidence')\n\
             elif cmd == 'identity':\n\tprint('vissue 99.0.0\\nroot=' + os.environ.get('VISSUE_ROOT','') + '\\nprefix=Software')\n\
             elif cmd == 'claim':\n\tby = os.environ.get('VISSUE_AGENT', '')\n\tcard.write_text(json.dumps({{'id': {ISSUE:?}, 'state': 'STARTED', 'claimed_by': by}}))\n\tprint('claimed')\n\
             elif cmd == 'show':\n\tprint(card.read_text() if card.exists() else json.dumps({{'id': {ISSUE:?}, 'title': 'the merge', 'state': 'TODO'}}))\n\
             elif cmd == 'claims':\n\tprint('[]')\n\
             else:\n\tprint('{{}}')\n"
        ),
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 2048];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body =
                r#"{"atoms":[],"island":[],"fired":0,"live":0,"embedder":{"available":false}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = std::io::Write::write_all(&mut stream, resp.as_bytes());
        }
    });
    let pack_url = format!("http://{addr}");

    let mcp_call = |meta: Option<serde_json::Value>| -> (serde_json::Value, Child) {
        let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos")).parent().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        cmd.env_clear()
            .env(
                "PATH",
                format!("{}:{}:/usr/bin:/bin", home.bin.display(), bin_dir.display()),
            )
            .env("HOME", &home.home)
            .env("XDG_CONFIG_HOME", home.home.join(".config"))
            .env("XDG_STATE_HOME", home.home.join(".local/state"))
            .env("XDG_DATA_HOME", home.home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.home.join(".cache"))
            .env("XDG_RUNTIME_DIR", &home.run)
            .env("VISSUE_ROOT", home.run.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "inky")
            .env("LJOS_PROJECT", "proj")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", &pack_url)
            .env("PACKSET_WORKSPACE", "seat")
            .current_dir(&home.repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let mut params = serde_json::json!({
            "name": "ljos_sitting",
            "arguments": {"issue": ISSUE}
        });
        if let Some(m) = meta {
            params["_meta"] = m;
        }
        let reqs = [
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "cursor", "version": "2.4.0"}}
            }),
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": params}),
        ];
        {
            let stdin = child.stdin.as_mut().unwrap();
            for r in &reqs {
                writeln!(stdin, "{r}").unwrap();
            }
            stdin.flush().unwrap();
        }
        use std::io::BufRead;
        let stdout = child.stdout.take().unwrap();
        let mut reader = std::io::BufReader::new(stdout);
        let mut answer = serde_json::Value::Null;
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap() > 0 {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if v.get("id") == Some(&serde_json::json!(2)) {
                    answer = v;
                    break;
                }
            }
            line.clear();
        }
        (answer, child)
    };

    // 1. Without _meta: sitting succeeds, X's merge is allowed, and Y's is refused.
    let (no_meta, mut srv1) = mcp_call(None);
    let no_meta_str = no_meta.to_string();
    assert!(
        no_meta["result"]["isError"] != true && no_meta.get("error").is_none(),
        "expected success without _meta: {no_meta_str}"
    );
    allowed(
        &home.hook(CONV_X, MERGE, &[]),
        "X's merge after MCP sitting without _meta",
    );
    refused(
        &home.hook(CONV_Y, MERGE, &[]),
        "Y's merge after X's MCP sitting without _meta",
    );
    let _ = srv1.kill();
    let _ = srv1.wait();

    // Clean hold-* records and re-seed before testing _meta.
    for entry in std::fs::read_dir(home.run.join("ljos")).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with("hold-") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let _ = std::fs::remove_file(&home.card);
    seed_tracker();

    // 2. With _meta.conversation_id = CONV_X: claims as inky@tag(CONV_X), hook allows X and refuses Y.
    let (with_meta, mut srv2) = mcp_call(Some(serde_json::json!({"conversation_id": CONV_X})));
    let with_meta_str = with_meta.to_string();
    assert!(
        with_meta["result"]["isError"] != true && with_meta.get("error").is_none(),
        "expected success with _meta conversation_id: {with_meta_str}"
    );
    let card_text = std::fs::read_to_string(&home.card).unwrap();
    assert!(
        card_text.contains(&holder(CONV_X)),
        "expected CLAIMED_BY {}: {card_text}",
        holder(CONV_X)
    );
    allowed(
        &home.hook(CONV_X, MERGE, &[]),
        "X's merge after MCP sitting with _meta.conversation_id",
    );
    refused(
        &home.hook(CONV_Y, MERGE, &[]),
        "Y's merge after X's MCP sitting with _meta.conversation_id",
    );
    let _ = srv2.kill();
    let _ = srv2.wait();
}

/// A shell that carries both `CODEX_THREAD_ID` and
/// `CURSOR_CONVERSATION_ID` holds under the Codex thread: a Codex thread
/// names its conversation first. A hook bound to the other id therefore
/// does not see that sitting.
#[test]
fn a_shell_with_a_codex_thread_holds_under_the_thread() {
    const T: &str = "codex-thread-00001111";
    let home = Box_::new();
    let both = [("CURSOR_CONVERSATION_ID", CONV_X), ("CODEX_THREAD_ID", T)];
    let said = text_of(&home.sh(&both, "\"$LJOS\" seat"));
    assert_eq!(seat_holder(&said), holder(T));
    let sat = home.sh(&both, &format!("\"$LJOS\" claim {ISSUE}"));
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(T));
    refused(
        &home.hook(CONV_X, MERGE, &[]),
        "X's merge after a sitting under the Codex thread",
    );
}

/// A tool command under a runner may not write a sitting record. The
/// records that bind a conversation for the paths that have no payload,
/// `session-bare-SEAT`, `tmux-hook-conv` and `first-hook-conv`, decide
/// whose sitting a call is judged by, so a shell line that writes one is
/// refused like a line that writes `hold-*`.
#[test]
fn a_tool_command_may_not_write_the_records_that_bind_a_conversation() {
    let home = Box_::new();
    for name in [
        "session-bare-inky",
        "tmux-hook-conv",
        "tmux-sock-abc",
        "tmux-pane-abc-1",
        "first-hook-conv",
    ] {
        let cmd = format!(
            "printf '%s\\n' inky {} > {}/ljos/{name}",
            holder(CONV_X),
            home.run.display()
        );
        let payload = serde_json::json!({
            "cursor_version": "2.4.0",
            "hook_event_name": "beforeShellExecution",
            "conversation_id": CONV_Z,
            "cwd": home.repo,
            "command": cmd,
        })
        .to_string();
        let said = home.hook_with(&payload, &[]);
        assert!(
            said.contains("\"permission\":\"deny\""),
            "a shell write to {name} was not refused:\n{said}"
        );
    }
}

const RV_RUNNER_ANY: &str = "
import json, os, subprocess, sys
rc = 0
shell = os.environ['RV_SHELL'].split()
for extra, line in json.loads(sys.argv[1]):
    env = dict(os.environ)
    env.update(extra)
    rc = subprocess.call(shell + [line], env=env)
print('EXIT', rc)
";

impl Box_ {
    /// `bash_runner` with the shell the runner starts each step in named
    /// by `shell` (`bash -c`, `sh -c`, `zsh -f -c`).
    fn rv_runner(
        &self,
        comm: &str,
        shell: &str,
        unset: &[&str],
        env: &[(&str, &str)],
        steps: &[(Vec<(&str, &str)>, &str)],
    ) -> String {
        let link = self.bin.join(comm);
        if !link.exists() {
            std::os::unix::fs::symlink("/usr/bin/python3", &link).unwrap();
        }
        let steps: Vec<(std::collections::BTreeMap<&str, &str>, &str)> = steps
            .iter()
            .map(|(e, l)| (e.iter().copied().collect(), *l))
            .collect();
        let steps = serde_json::to_string(&steps).unwrap();
        let mut args: Vec<String> = Vec::new();
        for u in unset {
            args.push("-u".into());
            args.push((*u).into());
        }
        let link = link.display().to_string();
        args.extend([link, "-c".into(), RV_RUNNER_ANY.into(), steps]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut all = vec![("LJOS", env!("CARGO_BIN_EXE_ljos")), ("RV_SHELL", shell)];
        all.extend_from_slice(env);
        text_of(&self.command("env", &args, &all).output().unwrap())
    }
}

fn rv_sat_x() -> (Box_, String) {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let commit = pre_commit_hooks(&home);
    (home, commit)
}

fn pre_commit_hooks(home: &Box_) -> String {
    let hooks = home.run.join("githooks");
    std::fs::create_dir_all(&hooks).unwrap();
    script(
        &hooks.join("pre-commit"),
        "#!/bin/sh\nexec \"$LJOS\" git-hook pre-commit\n",
    );
    format!(
        "git -c core.hooksPath={} -c user.name=t -c user.email=t@e commit -q --allow-empty -m t",
        hooks.display()
    )
}

/// RV10. A shell started without `XDG_RUNTIME_DIR` exports it later (a
/// container with no `/run/user/UID`). Its own commits go through. No
/// variable of the review corpus (`BOX`) is in its environment.
#[test]
fn rv10_a_shell_that_exports_its_runtime_dir_late_keeps_its_stamp() {
    let (home, commit) = rv_sat_x();
    let run = home.run.display().to_string();
    let x = vec![("CURSOR_AGENT", "1"), ("CURSOR_CONVERSATION_ID", CONV_X)];
    for line in [
        format!("export XDG_RUNTIME_DIR={run}; {commit}; true"),
        format!("export XDG_RUNTIME_DIR={run}; ( {commit} ); true"),
        format!("export XDG_RUNTIME_DIR={run}; timeout 60 {commit}; true"),
        format!("export XDG_RUNTIME_DIR={run}; bash -c '{commit}'; true"),
        format!("export XDG_RUNTIME_DIR={run}; env A=1 {commit}; true"),
    ] {
        let said = home.rv_runner(
            "node",
            "bash -c",
            &["CURSOR_AGENT", "XDG_RUNTIME_DIR"],
            &[],
            &[(x.clone(), &line)],
        );
        assert!(
            said.contains("EXIT 0") && !said.contains("sitting-gate"),
            "X's own `{line}` was refused:\n{said}"
        );
    }
    let override_line =
        format!("export XDG_RUNTIME_DIR={run}; CURSOR_CONVERSATION_ID={CONV_Z} {commit}; true");
    let said = home.rv_runner(
        "node",
        "bash -c",
        &["CURSOR_AGENT", "XDG_RUNTIME_DIR"],
        &[],
        &[(x, &override_line)],
    );
    assert!(
        said.contains("EXIT 0") && !said.contains("sitting-gate"),
        "X's shell that exported XDG_RUNTIME_DIR late lost its stamp to an inline override without BOX:\n{said}"
    );
}

/// Under a shared `tmux` server, a conversation's own commit in a new
/// window passes and another conversation's commit in a new window is
/// refused, whether the sitting conversation or the other started the
/// `tmux` server.
#[test]
fn a_tmux_window_allows_the_sitting_conversation_and_refuses_another() {
    let Ok(tmux) = which::which("tmux").or_else(|_| {
        let p = PathBuf::from("/exec-daemon/tmux");
        p.is_file().then_some(p).ok_or(())
    }) else {
        return;
    };
    for first in [CONV_X, CONV_Z] {
        let home = Box_::new();
        let sat = home.shell(CONV_X, &["claim", ISSUE]);
        assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
        home.card(&holder(CONV_X));
        let hooks = home.run.join("githooks");
        std::fs::create_dir_all(&hooks).unwrap();
        script(
            &hooks.join("pre-commit"),
            &format!(
                "#!/bin/sh\nexec {:?} git-hook pre-commit\n",
                env!("CARGO_BIN_EXE_ljos")
            ),
        );
        let sock = home.run.join("tmux.sock");
        let tmux_s = format!("{} -S {}", tmux.display(), sock.display());
        let start = home.bash(
            &[("CURSOR_CONVERSATION_ID", first)],
            &format!("{tmux_s} new-session -d -s base 'sleep 60'"),
        );
        assert!(start.status.success(), "tmux start: {}", text_of(&start));

        let z_done = home.run.join("z.done");
        let z_err = home.run.join("z.err");
        let z_cmd = format!(
            "{tmux_s} new-window -d 'git -c core.hooksPath={} -c user.name=t -c user.email=t@e commit -q --allow-empty -m z 2>{}; echo $? > {}'",
            hooks.display(),
            z_err.display(),
            z_done.display()
        );
        refused(
            &home.hook(CONV_Z, &z_cmd, &[("CURSOR_CONVERSATION_ID", CONV_Z)]),
            "Z's tmux new-window commit hook",
        );
        let _ = home.bash(&[("CURSOR_CONVERSATION_ID", CONV_Z)], &z_cmd);
        let t0 = Instant::now();
        while !z_done.exists() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let z_rc = std::fs::read_to_string(&z_done).unwrap_or_default();
        assert_ne!(
            z_rc.trim(),
            "0",
            "Z's tmux commit succeeded (server started by {first}): {}",
            std::fs::read_to_string(&z_err).unwrap_or_default()
        );

        let x_done = home.run.join("x.done");
        let x_err = home.run.join("x.err");
        let x_cmd = format!(
            "{tmux_s} new-window -d 'git -c core.hooksPath={} -c user.name=t -c user.email=t@e commit -q --allow-empty -m x 2>{}; echo $? > {}'",
            hooks.display(),
            x_err.display(),
            x_done.display()
        );
        allowed(
            &home.hook(CONV_X, &x_cmd, &[("CURSOR_CONVERSATION_ID", CONV_X)]),
            "X's tmux new-window commit hook",
        );
        let _ = home.bash(&[("CURSOR_CONVERSATION_ID", CONV_X)], &x_cmd);
        let t0 = Instant::now();
        while !x_done.exists() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let x_rc = std::fs::read_to_string(&x_done).unwrap_or_default();
        assert_eq!(
            x_rc.trim(),
            "0",
            "X's tmux commit failed (server started by {first}): {}",
            std::fs::read_to_string(&x_err).unwrap_or_default()
        );

        let _ = home.bash(
            &[("CURSOR_CONVERSATION_ID", first)],
            &format!("{tmux_s} kill-server"),
        );
    }
}

/// One tmux server serves two conversations. A pane's commit is judged
/// as the conversation whose pane it is, not as whichever conversation's
/// tool call last mentioned tmux. X's waiting commit must pass after Z
/// ran an unrelated command naming tmux, and Z's waiting commit must not
/// pass on X's sitting after X listed its windows.
#[test]
fn a_tmux_pane_is_not_judged_as_the_last_conversation_to_name_tmux() {
    let Ok(tmux) = which::which("tmux").or_else(|_| {
        let p = PathBuf::from("/exec-daemon/tmux");
        p.is_file().then_some(p).ok_or(())
    }) else {
        return;
    };
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let hooks = home.run.join("githooks");
    std::fs::create_dir_all(&hooks).unwrap();
    script(
        &hooks.join("pre-commit"),
        &format!(
            "#!/bin/sh\nexec {:?} git-hook pre-commit\n",
            env!("CARGO_BIN_EXE_ljos")
        ),
    );
    let sock = home.run.join("tmux.sock");
    let tmux_s = format!("{} -S {}", tmux.display(), sock.display());
    let x = [("CURSOR_CONVERSATION_ID", CONV_X)];
    let z = [("CURSOR_CONVERSATION_ID", CONV_Z)];
    let wait_then_commit = |who: &str| {
        let go = home.run.join(format!("{who}.go"));
        let done = home.run.join(format!("{who}.done"));
        let body = format!(
            "while [ ! -f {go} ]; do sleep 0.05; done\ngit -c core.hooksPath={h} -c user.name=t -c user.email=t@e commit -q --allow-empty -m {who} 2>{done}.err\necho $? > {done}\n",
            go = go.display(),
            h = hooks.display(),
            done = done.display(),
        );
        let file = home.run.join(format!("{who}.sh"));
        std::fs::write(&file, body).unwrap();
        (go, done, file)
    };
    let wait_rc = |done: &Path| {
        let t0 = Instant::now();
        while !done.exists() && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::read_to_string(done)
            .unwrap_or_default()
            .trim()
            .to_string()
    };

    // X starts the server and a window that commits once told to.
    let start = home.bash(&x, &format!("{tmux_s} new-session -d -s x 'sleep 60'"));
    assert!(start.status.success(), "tmux start: {}", text_of(&start));
    let (x_go, x_done, x_sh) = wait_then_commit("x");
    let x_cmd = format!("{tmux_s} new-window -d 'sh {}'", x_sh.display());
    allowed(&home.hook(CONV_X, &x_cmd, &x), "X's tmux window");
    let _ = home.bash(&x, &x_cmd);
    // Z reads a file whose name mentions tmux. Nothing of Z's runs in X's pane.
    let _ = home.hook(CONV_Z, "cat ~/.tmux.conf", &z);
    std::fs::write(&x_go, "").unwrap();
    assert_eq!(
        wait_rc(&x_done),
        "0",
        "X's own pane commit was refused after Z named tmux: {}",
        std::fs::read_to_string(home.run.join("x.done.err")).unwrap_or_default()
    );

    // Z starts a tmux server of its own on another socket, then X lists
    // the windows of X's server.
    let (z_go, z_done, z_sh) = wait_then_commit("z");
    let z_sock = home.run.join("tmux-z.sock");
    let z_cmd = format!(
        "{} -S {} new-session -d -s z 'sh {}'",
        tmux.display(),
        z_sock.display(),
        z_sh.display()
    );
    let _ = home.hook(CONV_Z, &z_cmd, &z);
    let _ = home.bash(&z, &z_cmd);
    let _ = home.hook(CONV_X, &format!("{tmux_s} list-windows"), &x);
    std::fs::write(&z_go, "").unwrap();
    let z_rc = wait_rc(&z_done);
    let z_err = std::fs::read_to_string(home.run.join("z.done.err")).unwrap_or_default();
    assert!(
        z_rc != "0" && z_err.contains("sitting-gate"),
        "Z's pane on its own tmux server committed on X's sitting after X listed X's windows: rc={z_rc} {z_err}"
    );
    let _ = home.bash(&x, &format!("{tmux_s} kill-server"));
    let _ = home.bash(
        &z,
        &format!("{} -S {} kill-server", tmux.display(), z_sock.display()),
    );
}

fn rv_tmux() -> PathBuf {
    which::which("tmux")
        .or_else(|_| {
            let p = PathBuf::from("/exec-daemon/tmux");
            p.is_file().then_some(p).ok_or(())
        })
        .expect("tmux is needed")
}

fn rv_wait(done: &Path) -> String {
    let t0 = Instant::now();
    while !done.exists() && t0.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::read_to_string(done)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// A pane script that waits for `WHO.go`, commits, and writes its exit
/// status to `WHO.done`.
fn rv_pane_script(home: &Box_, hooks: &Path, who: &str) -> (PathBuf, PathBuf, PathBuf) {
    let go = home.run.join(format!("{who}.go"));
    let done = home.run.join(format!("{who}.done"));
    let body = format!(
        "while [ ! -f {go} ]; do sleep 0.05; done\ngit -c core.hooksPath={h} -c user.name=t -c user.email=t@e commit -q --allow-empty -m {who} 2>{done}.err\necho $? > {done}\n",
        go = go.display(),
        h = hooks.display(),
        done = done.display(),
    );
    let file = home.run.join(format!("{who}.sh"));
    std::fs::write(&file, body).unwrap();
    (go, done, file)
}

fn rv_git_hooks(home: &Box_) -> PathBuf {
    let hooks = home.run.join("githooks");
    std::fs::create_dir_all(&hooks).unwrap();
    script(
        &hooks.join("pre-commit"),
        &format!(
            "#!/bin/sh\nexec {:?} git-hook pre-commit\n",
            env!("CARGO_BIN_EXE_ljos")
        ),
    );
    hooks
}

/// RV1. Two conversations share one tmux server, as two bots that both
/// run `tmux new -d` on the default socket do. X's own pane commit is
/// still X's after Z opens a session of its own on that server.
#[test]
fn rv1_a_pane_on_a_shared_tmux_server_stays_its_conversations() {
    let tmux = rv_tmux();
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let hooks = rv_git_hooks(&home);
    let sock = home.run.join("tmux.sock");
    let tmux_s = format!("{} -S {}", tmux.display(), sock.display());
    let x = [("CURSOR_CONVERSATION_ID", CONV_X)];
    let z = [("CURSOR_CONVERSATION_ID", CONV_Z)];
    let (go, done, file) = rv_pane_script(&home, &hooks, "x");
    let x_cmd = format!("{tmux_s} new-session -d -s x 'sh {}'", file.display());
    allowed(&home.hook(CONV_X, &x_cmd, &x), "X's tmux session");
    let _ = home.bash(&x, &x_cmd);
    let z_cmd = format!("{tmux_s} new-session -d -s z 'sleep 30'");
    allowed(&home.hook(CONV_Z, &z_cmd, &z), "Z's tmux session");
    let _ = home.bash(&z, &z_cmd);
    std::fs::write(&go, "").unwrap();
    let rc = rv_wait(&done);
    let _ = home.bash(&x, &format!("{tmux_s} kill-server"));
    assert_eq!(
        rc,
        "0",
        "X's own pane commit was refused after Z opened a session on the same server: {}",
        std::fs::read_to_string(home.run.join("x.done.err")).unwrap_or_default()
    );
}

/// RV2. The reverse of RV1: Z's pane on the shared server does not
/// commit on X's sitting because X opened a window after Z's pane
/// started.
#[test]
fn rv2_a_pane_on_a_shared_tmux_server_does_not_borrow_the_last_openers_sitting() {
    let tmux = rv_tmux();
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let hooks = rv_git_hooks(&home);
    let sock = home.run.join("tmux.sock");
    let tmux_s = format!("{} -S {}", tmux.display(), sock.display());
    let x = [("CURSOR_CONVERSATION_ID", CONV_X)];
    let z = [("CURSOR_CONVERSATION_ID", CONV_Z)];
    let (go, done, file) = rv_pane_script(&home, &hooks, "z");
    let z_cmd = format!("{tmux_s} new-session -d -s z 'sh {}'", file.display());
    let _ = home.hook(CONV_Z, &z_cmd, &z);
    let _ = home.bash(&z, &z_cmd);
    let x_cmd = format!("{tmux_s} new-window -d -t z 'sleep 30'");
    let _ = home.hook(CONV_X, &x_cmd, &x);
    let _ = home.bash(&x, &x_cmd);
    std::fs::write(&go, "").unwrap();
    let rc = rv_wait(&done);
    let err = std::fs::read_to_string(home.run.join("z.done.err")).unwrap_or_default();
    let _ = home.bash(&x, &format!("{tmux_s} kill-server"));
    assert!(
        rc != "0" && err.contains("sitting-gate"),
        "Z's pane committed on X's sitting after X opened a window: rc={rc} {err}"
    );
}

const RUNNER_BASH: &str = "
import json, os, subprocess, sys
rc = 0
for extra, line in json.loads(sys.argv[1]):
    env = dict(os.environ)
    env.update(extra)
    rc = subprocess.call(['bash', '-c', line], env=env)
print('EXIT', rc)
";

impl Box_ {
    /// A runner named `comm` (a python3 symlink) started without the
    /// variables in `unset`, that runs each step as `bash -c LINE` with
    /// the step's variables on top, as a runner starts one tool call.
    fn bash_runner(
        &self,
        comm: &str,
        unset: &[&str],
        env: &[(&str, &str)],
        steps: &[(Vec<(&str, &str)>, &str)],
    ) -> String {
        let link = self.bin.join(comm);
        if !link.exists() {
            std::os::unix::fs::symlink("/usr/bin/python3", &link).unwrap();
        }
        let steps: Vec<(std::collections::BTreeMap<&str, &str>, &str)> = steps
            .iter()
            .map(|(e, l)| (e.iter().copied().collect(), *l))
            .collect();
        let steps = serde_json::to_string(&steps).unwrap();
        let mut args: Vec<String> = Vec::new();
        for u in unset {
            args.push("-u".into());
            args.push((*u).into());
        }
        let link = link.display().to_string();
        args.extend([link, "-c".into(), RUNNER_BASH.into(), steps]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut all = vec![("LJOS", env!("CARGO_BIN_EXE_ljos"))];
        all.extend_from_slice(env);
        text_of(&self.command("env", &args, &all).output().unwrap())
    }
}

/// A runner that is not stamped (a node daemon, `CURSOR_AGENT` unset)
/// starts each tool call as `bash -c LINE` with the conversation's id.
/// Bash execs the last simple command of the line, so `git` is the
/// runner's child. The sitting conversation's own commit goes through.
#[test]
fn a_one_shot_bash_c_commit_from_an_unstamped_runner_is_allowed() {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));
    let commit = pre_commit_hooks(&home);
    let x = vec![("CURSOR_AGENT", "1"), ("CURSOR_CONVERSATION_ID", CONV_X)];
    for line in [
        commit.clone(),
        format!("cd . && {commit}"),
        format!("true; {commit}"),
        format!("GIT_EDITOR=true {commit}"),
    ] {
        let said = home.bash_runner("node", &["CURSOR_AGENT"], &[], &[(x.clone(), &line)]);
        assert!(
            said.contains("EXIT 0") && !said.contains("sitting-gate"),
            "X's own `bash -c '{line}'` was refused:\n{said}"
        );
    }
}

/// L1 regression guard: with a live sitting, an ordinary one-shot
/// `bash -c 'git commit ...'` should not be refused solely because the
/// immediate parent process carries no conversation id.
#[test]
fn one_shot_bash_c_commit_from_unstamped_parent_is_allowed_when_sitting_is_live() {
    let home = Box_::new();
    let sat = home.shell(CONV_X, &["claim", ISSUE]);
    assert!(sat.status.success(), "ljos claim: {}", text_of(&sat));
    home.card(&holder(CONV_X));

    let hooks = home.run.join("githooks");
    std::fs::create_dir_all(&hooks).unwrap();
    script(
        &hooks.join("pre-commit"),
        &format!(
            "#!/bin/sh\nexec {:?} git-hook pre-commit\n",
            env!("CARGO_BIN_EXE_ljos")
        ),
    );

    // Keep hold live as an active conversation shell would.
    let mut sleeper = Command::new("sleep").arg("300").spawn().unwrap();
    let pid = sleeper.id();
    let hold = std::fs::read_dir(home.run.join("ljos"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("hold-")
        })
        .unwrap();
    let mut lines: Vec<String> = std::fs::read_to_string(&hold)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    lines[2] = pid.to_string();
    lines[3] = "sleep".into();
    std::fs::write(&hold, lines.join("\n") + "\n").unwrap();

    std::fs::write(home.repo.join("f.txt"), "x\n").unwrap();
    let add = home
        .command("git", &["add", "f.txt"], &[])
        .output()
        .unwrap();
    assert!(add.status.success(), "{}", text_of(&add));

    // Parent intentionally unstamped; command provides conversation id.
    let out = home
        .command(
            "bash",
            &[
                "-c",
                &format!(
                    "CURSOR_CONVERSATION_ID={CONV_X} git -c core.hooksPath={} -c user.name=t -c user.email=t@e commit -q --allow-empty -m t",
                    hooks.display()
                ),
            ],
            &[],
        )
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ordinary one-shot bash commit was refused:\n{}",
        text_of(&out)
    );

    let _ = sleeper.kill();
    let _ = sleeper.wait();
}

/// RV6. The runner was started from a shell that is itself nested (a
/// tmux pane, an ssh login, a terminal inside an editor), so its
/// environment carries `SHLVL=2`. Bash passes `SHLVL` through to the
/// command it execs. X's own one-shot commit goes through.
#[test]
fn rv6_a_one_shot_commit_from_a_runner_started_at_shlvl_2_is_allowed() {
    let (home, commit) = rv_sat_x();
    let x = vec![("CURSOR_AGENT", "1"), ("CURSOR_CONVERSATION_ID", CONV_X)];
    for (comm, unset) in [("node", vec!["CURSOR_AGENT"]), ("cursor-agent", vec![])] {
        for line in [commit.clone(), format!("cd . && {commit}")] {
            let said = home.rv_runner(
                comm,
                "bash -c",
                &unset,
                &[("SHLVL", "2")],
                &[(x.clone(), &line)],
            );
            assert!(
                said.contains("EXIT 0") && !said.contains("sitting-gate"),
                "X's own `bash -c '{line}'` under {comm} at SHLVL=2 was refused:\n{said}"
            );
        }
    }
}

/// RV7. A runner that starts each tool call as `/bin/sh -c LINE` (node's
/// `child_process.exec` does). Ubuntu's `/bin/sh` is dash, which execs
/// the last command too but sets no `_`. X's own commit goes through.
#[test]
fn rv7_a_one_shot_sh_c_commit_from_an_unstamped_runner_is_allowed() {
    let (home, commit) = rv_sat_x();
    let x = vec![("CURSOR_AGENT", "1"), ("CURSOR_CONVERSATION_ID", CONV_X)];
    for line in [commit.clone(), format!("cd . && {commit}")] {
        let said = home.rv_runner(
            "node",
            "/bin/sh -c",
            &["CURSOR_AGENT"],
            &[],
            &[(x.clone(), &line)],
        );
        assert!(
            said.contains("EXIT 0") && !said.contains("sitting-gate"),
            "X's own `sh -c '{line}'` was refused:\n{said}"
        );
    }
}

/// RV8. Ordinary wrappers in a one-shot line under an unstamped runner:
/// bash execs the wrapper, and the wrapper runs git. X's own commit goes
/// through.
#[test]
fn rv8_a_one_shot_commit_behind_an_ordinary_wrapper_is_allowed() {
    let (home, commit) = rv_sat_x();
    let x = vec![("CURSOR_AGENT", "1"), ("CURSOR_CONVERSATION_ID", CONV_X)];
    for line in [
        format!("timeout 60 {commit}"),
        format!("nohup {commit} >/dev/null"),
        format!("stdbuf -oL {commit}"),
        format!("ionice -c3 {commit}"),
        format!("flock {} {commit}", home.run.join("l").display()),
    ] {
        let said = home.rv_runner(
            "node",
            "bash -c",
            &["CURSOR_AGENT"],
            &[],
            &[(x.clone(), &line)],
        );
        assert!(
            said.contains("EXIT 0") && !said.contains("sitting-gate"),
            "X's own `bash -c '{line}'` was refused:\n{said}"
        );
    }
}

/// RV9. A script that execs git with another conversation's id without
/// changing `_` is refused because `_` names python. A hidden script
/// under an unstamped runner that also rewrites `_` to `/usr/bin/git`
/// matches bash's exec layout and is a known limit at the run layer, as
/// on base.
#[test]
fn rv9_a_script_that_execs_git_with_anothers_id_does_not_commit_on_its_sitting() {
    let (home, commit) = rv_sat_x();
    let z = vec![("CURSOR_AGENT", "1"), ("CURSOR_CONVERSATION_ID", CONV_Z)];
    let argv: Vec<String> = commit.split(' ').map(str::to_string).collect();
    let argv = serde_json::to_string(&argv).unwrap();
    let forge = home.run.join("forge.py");
    std::fs::write(
        &forge,
        format!(
            "import os\ne = dict(os.environ)\ne['CURSOR_CONVERSATION_ID'] = {CONV_X:?}\nos.execvpe('git', {argv}, e)\n"
        ),
    )
    .unwrap();
    let line = format!("python3 {}", forge.display());
    let said = home.rv_runner("node", "bash -c", &["CURSOR_AGENT"], &[], &[(z, &line)]);
    assert!(
        !said.contains("EXIT 0"),
        "Z's script committed on X's sitting:\n{said}"
    );
}

/// RV12. An id-less `node` runner that starts each tool call as
/// `zsh -f -c LINE` (zsh is the login shell on macOS). X's `ljos seat`
/// names X, and X's own one-shot commit goes through.
#[test]
fn rv12_a_one_shot_zsh_c_call_from_an_unstamped_runner_is_the_conversations() {
    let home = Box_::new();
    let x = vec![("CURSOR_AGENT", "1"), ("CURSOR_CONVERSATION_ID", CONV_X)];
    let said = home.rv_runner(
        "node",
        "zsh -f -c",
        &["CURSOR_AGENT"],
        &[],
        &[(x.clone(), "\"$LJOS\" seat")],
    );
    assert!(
        said.contains(&holder(CONV_X)),
        "X's `zsh -c 'ljos seat'` did not name X:\n{said}"
    );
    let (home, commit) = rv_sat_x();
    let said = home.rv_runner(
        "node",
        "zsh -f -c",
        &["CURSOR_AGENT"],
        &[],
        &[(x.clone(), &commit)],
    );
    assert!(
        said.contains("EXIT 0") && !said.contains("sitting-gate"),
        "X's own `zsh -c '{commit}'` was refused:\n{said}"
    );
}

/// RV13. A runner started from a nested shell (`SHLVL=2`) runs X's
/// one-shot `env VAR=x git commit`. X's own commit goes through.
#[test]
fn rv13_a_one_shot_env_prefixed_commit_at_shlvl_2_is_allowed() {
    let (home, commit) = rv_sat_x();
    let x = vec![("CURSOR_AGENT", "1"), ("CURSOR_CONVERSATION_ID", CONV_X)];
    let line = format!("env GIT_AUTHOR_NAME=x {commit}");
    let said = home.rv_runner(
        "node",
        "bash -c",
        &["CURSOR_AGENT"],
        &[("SHLVL", "2")],
        &[(x.clone(), &line)],
    );
    assert!(
        said.contains("EXIT 0") && !said.contains("sitting-gate"),
        "X's own `bash -c '{line}'` at SHLVL=2 was refused:\n{said}"
    );
}

/// A `cursor-agent` runner whose own environment carries `CURSOR_AGENT`
/// but no conversation id starts `bash -c 'ljos seat'` with X's id. The
/// seat answers for X, as the payload hook of X's next call does.
#[test]
fn a_one_shot_seat_under_a_cursor_agent_without_an_id_is_the_conversations() {
    let home = Box_::new();
    let x = vec![("CURSOR_CONVERSATION_ID", CONV_X)];
    let said = home.bash_runner("cursor-agent", &[], &[], &[(x, "\"$LJOS\" seat")]);
    assert_eq!(seat_holder(&said), holder(CONV_X), "{said}");
}

/// A runner started from another conversation's shell (a bot that runs
/// a second agent) does not sit as the outer conversation, and the outer
/// conversation does not lend it its holder.
#[test]
fn a_runner_nested_in_another_conversations_shell_sits_as_its_own() {
    let home = Box_::new();
    let link = home.bin.join("cursor-agent");
    std::os::unix::fs::symlink("/usr/bin/python3", &link).unwrap();
    let steps = serde_json::to_string(&[(None::<&str>, "\"$LJOS\" seat; true")]).unwrap();
    let inner = home.command(
        "sh",
        &[
            "-c",
            &format!(
                "CURSOR_CONVERSATION_ID={CONV_X} {} -c {} {}; true",
                link.display(),
                shell_words(RUNNER),
                shell_words(&steps)
            ),
        ],
        &[
            ("LJOS", env!("CARGO_BIN_EXE_ljos")),
            (
                "CURSOR_CONVERSATION_ID",
                "0199a1b2-c3d4-4e5f-8a6b-0000000000ou",
            ),
        ],
    );
    let said = text_of(&{ inner }.output().unwrap());
    assert_eq!(seat_holder(&said), holder(CONV_X), "{said}");
}

fn shell_words(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

const RV_OUTER: &str = "0199a1b2-c3d4-4e5f-8a6b-0000000000ee";

impl Box_ {
    /// `rv_runner` started from conversation `RV_OUTER`'s shell, the way
    /// a bot launches a second runner (`node`, an MCP host, a CI script)
    /// from its own terminal. The runner's environment is the box's with
    /// no conversation id.
    fn rv_runner_nested(
        &self,
        comm: &str,
        shell: &str,
        steps: &[(Vec<(&str, &str)>, &str)],
    ) -> String {
        let link = self.bin.join(comm);
        if !link.exists() {
            std::os::unix::fs::symlink("/usr/bin/python3", &link).unwrap();
        }
        let steps: Vec<(std::collections::BTreeMap<&str, &str>, &str)> = steps
            .iter()
            .map(|(e, l)| (e.iter().copied().collect(), *l))
            .collect();
        let steps = serde_json::to_string(&steps).unwrap();
        let inner = self.command(
            "true",
            &[],
            &[("LJOS", env!("CARGO_BIN_EXE_ljos")), ("RV_SHELL", shell)],
        );
        let mut args: Vec<String> = vec![
            "-c".into(),
            "\"$@\"; true".into(),
            "bash".into(),
            "env".into(),
            "-i".into(),
        ];
        for (k, v) in inner.get_envs() {
            let (Some(k), Some(v)) = (k.to_str(), v.and_then(|v| v.to_str())) else {
                continue;
            };
            if k != "CURSOR_AGENT" {
                args.push(format!("{k}={v}"));
            }
        }
        args.extend([
            link.display().to_string(),
            "-c".into(),
            RV_RUNNER_ANY.into(),
            steps,
        ]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        text_of(
            &self
                .command("bash", &args, &[("CURSOR_CONVERSATION_ID", RV_OUTER)])
                .output()
                .unwrap(),
        )
    }
}

/// RV11. An id-less `node` runner started from conversation OUTER's
/// shell runs X's tool calls as `bash -c`, each with X's id. X's
/// `ljos seat` and `ljos claim` are X's, not OUTER's.
#[test]
fn rv11_an_unstamped_runner_in_another_conversations_shell_sits_as_the_call() {
    let home = Box_::new();
    let x = vec![("CURSOR_CONVERSATION_ID", CONV_X)];
    let said = home.rv_runner_nested(
        "node",
        "bash -c",
        &[
            (x.clone(), "\"$LJOS\" seat"),
            (x.clone(), &format!("\"$LJOS\" claim {ISSUE}")),
        ],
    );
    assert!(
        said.contains(&holder(CONV_X)) && !said.contains(&holder(RV_OUTER)),
        "X's seat and claim under a runner nested in OUTER's shell:\n{said}"
    );
}
