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
