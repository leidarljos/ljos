//! The sitting gate in a git hook, where a runner variable is the only
//! cause: the push runs, and the hook prints `sitting-gate`. The person
//! is not asked for a passphrase.
//!
//! Each case runs this test binary again as the hook process, in a
//! session of its own whose controlling terminal is a fresh pty. That
//! child keeps the key in a temporary directory and runs the hook the
//! way `ljos git-hook NAME` does. The child sets the runner variable
//! itself, since the test harness strips runner variables at load.

ljos_test_boot::harness!();

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[path = "common/pty.rs"]
mod pty;
#[path = "common/runner.rs"]
mod runner;

const PASS: &str = "correct horse battery";
const PROMPT: &str = "Type your person passphrase to run this git push as you: ";
const YES_PROMPT: &str = "Type yes to run this git push as yourself: ";
/// The child's key directory. Read by this test binary only.
const CHILD_DIR: &str = "PERSON_GATE_CHILD_DIR";
/// The runner variable the child sets on itself before the hook runs.
const CHILD_MARK: &str = "PERSON_GATE_CHILD_MARK";

/// The hook process. Inert unless the parent case started it.
#[test]
fn gate_child() {
    let Some(dir) = std::env::var_os(CHILD_DIR) else {
        return;
    };
    ljos_cli::person_key::keep_in_for_this_process(PathBuf::from(dir));
    if let Ok(mark) = std::env::var(CHILD_MARK) {
        // SAFETY: this child runs one test on one thread.
        unsafe { std::env::set_var(mark, "1") };
    }
    let code = match ljos_cli::git_hooks::run_named("pre-push") {
        Ok(()) => {
            println!("gate-child: allowed");
            0
        }
        Err(e) => {
            eprintln!("{e:#}");
            println!("gate-child: refused");
            1
        }
    };
    std::process::exit(code);
}

struct Case {
    home: tempfile::TempDir,
    keys: PathBuf,
    repo: PathBuf,
    /// A real commit on both sides, so the message check has nothing new to read.
    push: String,
}

impl Case {
    /// A git checkout, a home of its own, and a key for [`PASS`] unless
    /// `keyed` is false.
    fn new(keyed: bool) -> Self {
        let home = tempfile::tempdir().unwrap();
        let keys = home.path().join("keys");
        std::fs::create_dir_all(&keys).unwrap();
        if keyed {
            ljos_cli::person_key::keep(&keys.join("person-key"), PASS.as_bytes()).unwrap();
        }
        let repo = home.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(init.success());
        for (key, value) in [
            ("user.email", "t@example.invalid"),
            ("user.name", "t"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", "/dev/null"),
        ] {
            let out = Command::new("git")
                .args(["config", key, value])
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(out.status.success(), "{key}");
        }
        let made = Command::new("git")
            .args(["commit", "-q", "--allow-empty", "-m", "seed"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "{}",
            String::from_utf8_lossy(&made.stderr)
        );
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(head.status.success());
        let sha = String::from_utf8(head.stdout).unwrap();
        let sha = sha.trim();
        let push = format!("refs/heads/develop {sha} refs/heads/develop {sha}\n");
        Self {
            home,
            keys,
            repo,
            push,
        }
    }

    /// The child command, run directly or through `wrapper`.
    fn command(&self, wrapper: Option<&Path>, mark: Option<&str>) -> Command {
        let exe = std::env::current_exe().unwrap();
        let mut cmd = match wrapper {
            Some(runner) => {
                let mut cmd = Command::new(runner);
                cmd.args(["-c", "\"$@\"; exit $?", "claude"]).arg(&exe);
                cmd
            }
            None => Command::new(&exe),
        };
        cmd.args(["--exact", "gate_child", "--nocapture", "--test-threads=1"])
            .current_dir(&self.repo)
            .env(CHILD_DIR, &self.keys)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("LJOS_OFF")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(mark) = mark {
            cmd.env(CHILD_MARK, mark);
        }
        cmd
    }

    fn failures(&self) -> usize {
        std::fs::read_to_string(self.keys.join("person-key.failures"))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }
}

/// The child on a pty, with `stdin` the push refs. The hook does not
/// prompt, so this does not wait for one.
fn run_hook(cmd: &mut Command, stdin: &str) -> (Output, String) {
    let term = pty::Pty::open();
    let mut child = term.spawn(cmd.stdin(Stdio::piped()));
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = pty::wait_output(child, 30);
    (out, term.shown())
}

fn labelled(out: &Output) -> String {
    let said = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "held: {said}{err}");
    assert!(said.contains("gate-child: allowed"), "{said}{err}");
    assert!(err.contains("sitting-gate"), "{err}");
    err
}

fn push_is_labelled(case: &Case, cmd: &mut Command) {
    let (out, shown) = run_hook(cmd, &case.push);
    let err = labelled(&out);
    assert!(!shown.contains(PROMPT), "the hook asked: {shown}");
    assert!(!shown.contains(YES_PROMPT), "the hook asked: {shown}");
    assert!(!err.contains("passphrase"), "{err}");
    assert_eq!(case.failures(), 0);
}

#[test]
fn the_right_passphrase_on_the_terminal_lets_the_push_through() {
    let case = Case::new(true);
    push_is_labelled(&case, &mut case.command(None, Some("CURSOR_AGENT")));
}

#[test]
fn a_wrong_passphrase_is_refused_and_counted() {
    let case = Case::new(true);
    push_is_labelled(&case, &mut case.command(None, Some("CURSOR_AGENT")));
}

/// A string of wrong answers used to lock the prompt. The hook no longer
/// asks, so each push is the same label and the failure count stays empty.
#[test]
fn five_wrong_passphrases_lock_the_prompt_out() {
    let case = Case::new(true);
    for _ in 0..5 {
        push_is_labelled(&case, &mut case.command(None, Some("CURSOR_AGENT")));
    }
}

/// Nothing typed is not a refusal: the push runs with the sitting label.
#[test]
fn nothing_typed_is_refused() {
    let case = Case::new(true);
    push_is_labelled(&case, &mut case.command(None, Some("CURSOR_AGENT")));
}

/// A runner in the tree, with a key kept and a runner variable set, is
/// not asked. The push runs with the sitting label.
#[test]
fn a_hard_runner_is_never_asked() {
    let case = Case::new(true);
    let installed = runner::install_dir();
    let claude = installed.path().join("claude");
    runner::runner_at(&claude);
    let (out, shown) = run_hook(
        &mut case.command(Some(&claude), Some("CURSOR_AGENT")),
        &case.push,
    );
    let err = labelled(&out);
    assert!(
        !shown.contains("passphrase"),
        "a hard runner was asked: {shown}"
    );
    assert!(!err.contains("passphrase"), "{err}");
    assert!(err.contains("sitting-gate"), "{err}");
}

/// With no controlling terminal the push still runs. Nothing is asked.
#[test]
fn piped_stdin_without_a_terminal_cannot_answer() {
    use std::os::unix::process::CommandExt as _;
    let case = Case::new(true);
    let mut cmd = case.command(None, Some("CURSOR_AGENT"));
    // SAFETY: setsid is async-signal-safe; the child gets no terminal.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.stdin(Stdio::piped()).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(case.push.as_bytes())
        .unwrap();
    let out = pty::wait_output(child, 60);
    let err = labelled(&out);
    assert!(!err.contains(PROMPT), "{err}");
}

/// A passphrase on the pipe is not read. The push runs with the label.
#[test]
fn piped_stdin_with_a_terminal_cannot_answer() {
    let case = Case::new(true);
    push_is_labelled(&case, &mut case.command(None, Some("CURSOR_AGENT")));
}

/// No variable supplies a passphrase, and none is asked. The push runs.
#[test]
fn no_variable_supplies_the_passphrase() {
    let case = Case::new(true);
    let mut cmd = case.command(None, Some("CURSOR_AGENT"));
    for name in [
        "LJOS_PERSON_KEY",
        "LJOS_PASSPHRASE",
        "LJOS_PERSON_PASSPHRASE",
        "PERSON_KEY",
    ] {
        cmd.env(name, PASS);
    }
    push_is_labelled(&case, &mut cmd);
}

/// With no key kept, the hook does not ask for a yes. The push runs.
#[test]
fn no_key_asks_for_a_yes_and_a_passphrase_is_not_one() {
    let case = Case::new(false);
    push_is_labelled(&case, &mut case.command(None, Some("CURSOR_AGENT")));
}

/// With no key kept, the push runs without a yes at the terminal.
#[test]
fn no_key_and_a_yes_at_the_terminal_lets_the_push_through() {
    let case = Case::new(false);
    push_is_labelled(&case, &mut case.command(None, Some("CURSOR_AGENT")));
}

/// The hook does not take the terminal, so echo stays on and the push runs.
#[test]
fn an_interrupt_at_the_prompt_restores_echo() {
    let case = Case::new(true);
    let term = pty::Pty::open();
    let mut child = term.spawn(
        case.command(None, Some("CURSOR_AGENT"))
            .stdin(Stdio::piped()),
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(case.push.as_bytes())
        .unwrap();
    let out = pty::wait_output(child, 30);
    labelled(&out);
    assert!(term.echo_on(), "echo was turned off: {}", term.shown());
    assert!(!term.shown().contains(PROMPT), "{}", term.shown());
}
