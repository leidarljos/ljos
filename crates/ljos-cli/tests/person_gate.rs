//! The sitting gate in a git hook, where a runner variable is the only
//! cause: the person is asked on the controlling terminal, and only a
//! passphrase typed there lets the call through.
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
const NOT_GATED: &str = "The person at their own terminal is not gated";
const PUSH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb refs/heads/develop refs/heads/develop\n";

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
        Self { home, keys, repo }
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

/// The child on a pty of its own with stdin a pipe that carries `stdin`,
/// `answer` typed at the prompt (`None` ends input there instead).
fn on_pty(cmd: &mut Command, stdin: &str, answer: Option<&str>) -> (Output, String) {
    let mut term = pty::Pty::open();
    let mut child = term.spawn(cmd.stdin(Stdio::piped()));
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let prompted = term.wait_for(PROMPT, 30);
    if prompted {
        match answer {
            Some(text) => term.type_line(text),
            None => term.send_eof(),
        }
    }
    let out = pty::wait_output(child, 120);
    (out, term.shown())
}

fn refused(out: &Output) -> String {
    let said = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "allowed: {said}{err}");
    assert!(said.contains("gate-child: refused"), "{said}{err}");
    err
}

#[test]
fn the_right_passphrase_on_the_terminal_lets_the_push_through() {
    let case = Case::new(true);
    let (out, shown) = on_pty(
        &mut case.command(None, Some("CURSOR_AGENT")),
        PUSH,
        Some(PASS),
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(said.contains("gate-child: allowed"), "{said}");
    assert!(
        shown.contains("ljos: this shell carries a runner variable (CURSOR_AGENT)."),
        "{shown}"
    );
    assert!(!shown.contains(PASS), "the passphrase was echoed: {shown}");
    assert_eq!(case.failures(), 0);
}

#[test]
fn a_wrong_passphrase_is_refused_and_counted() {
    let case = Case::new(true);
    let (out, _) = on_pty(
        &mut case.command(None, Some("CURSOR_AGENT")),
        PUSH,
        Some("not the passphrase"),
    );
    let err = refused(&out);
    assert!(err.contains("the passphrase did not match"), "{err}");
    assert_eq!(case.failures(), 1);
    for part in [
        "This shell carries CURSOR_AGENT, which a runner leaves on what it starts",
        "type your person passphrase when asked",
        "or run the push from your own machine",
    ] {
        assert!(err.contains(part), "{part:?} missing: {err}");
    }
    assert!(!err.contains(NOT_GATED), "{err}");
}

/// Five wrong answers, each waiting longer, and the sixth call is
/// refused without a prompt. This waits 2 + 4 + 8 + 16 + 32 seconds.
#[test]
fn five_wrong_passphrases_lock_the_prompt_out() {
    let case = Case::new(true);
    for i in 0..5 {
        let (out, shown) = on_pty(
            &mut case.command(None, Some("CURSOR_AGENT")),
            PUSH,
            Some("not the passphrase"),
        );
        let err = refused(&out);
        assert!(shown.contains(PROMPT), "try {i}: {shown}{err}");
    }
    assert_eq!(case.failures(), 5);
    let (out, shown) = on_pty(
        &mut case.command(None, Some("CURSOR_AGENT")),
        PUSH,
        Some(PASS),
    );
    let err = refused(&out);
    assert!(!shown.contains(PROMPT), "a lockout prompted: {shown}");
    assert!(err.contains("does not ask again until"), "{err}");
}

/// A runner without the passphrase: nothing typed is refused, and is
/// not a wrong answer.
#[test]
fn nothing_typed_is_refused() {
    let case = Case::new(true);
    let (out, shown) = on_pty(&mut case.command(None, Some("CURSOR_AGENT")), PUSH, None);
    assert!(shown.contains(PROMPT), "{shown}");
    let err = refused(&out);
    assert!(err.contains("nothing was typed"), "{err}");
    assert_eq!(case.failures(), 0);
}

/// A runner in the tree, with a key kept and a runner variable set, is
/// never asked.
#[test]
fn a_hard_runner_is_never_asked() {
    let case = Case::new(true);
    let installed = runner::install_dir();
    let claude = installed.path().join("claude");
    runner::runner_at(&claude);
    let (out, shown) = on_pty(
        &mut case.command(Some(&claude), Some("CURSOR_AGENT")),
        PUSH,
        Some(PASS),
    );
    let err = refused(&out);
    assert!(
        !shown.contains("passphrase"),
        "a hard runner was asked: {shown}"
    );
    assert!(!err.contains("passphrase"), "{err}");
    assert!(err.contains(NOT_GATED), "{err}");
}

/// The right passphrase on a piped stdin is not an answer: with no
/// controlling terminal the call is refused and nothing is asked.
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
        .write_all(format!("{PASS}\n{PASS}\n").as_bytes())
        .unwrap();
    let out = pty::wait_output(child, 60);
    let err = refused(&out);
    assert!(err.contains("no controlling terminal"), "{err}");
    assert!(!err.contains(PROMPT), "{err}");
}

/// With a terminal, the right passphrase on stdin and nothing typed on
/// the terminal is refused.
#[test]
fn piped_stdin_with_a_terminal_cannot_answer() {
    let case = Case::new(true);
    let (out, shown) = on_pty(
        &mut case.command(None, Some("CURSOR_AGENT")),
        &format!("{PASS}\n{PASS}\n"),
        None,
    );
    assert!(shown.contains(PROMPT), "{shown}");
    let err = refused(&out);
    assert!(err.contains("nothing was typed"), "{err}");
}

/// No variable supplies the answer: the right passphrase in any of these
/// names still leaves the prompt up, and nothing typed is refused.
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
    let (out, shown) = on_pty(&mut cmd, PUSH, None);
    assert!(shown.contains(PROMPT), "{shown}");
    let err = refused(&out);
    assert!(err.contains("nothing was typed"), "{err}");
}

/// With no key kept, the call is refused and nothing is asked.
#[test]
fn no_key_is_refused_without_a_prompt() {
    let case = Case::new(false);
    let (out, shown) = on_pty(
        &mut case.command(None, Some("CURSOR_AGENT")),
        PUSH,
        Some(PASS),
    );
    let err = refused(&out);
    assert!(!shown.contains(PROMPT), "{shown}");
    assert!(err.contains("No person passphrase is set"), "{err}");
    assert!(
        err.contains("`ljos person-key set` from your own machine"),
        "{err}"
    );
    assert!(err.contains("This shell carries CURSOR_AGENT"), "{err}");
    assert!(!err.contains(NOT_GATED), "{err}");
}

/// The interrupt key at the prompt ends the hook, and echo is back on.
#[test]
fn an_interrupt_at_the_prompt_restores_echo() {
    let case = Case::new(true);
    let mut term = pty::Pty::open();
    let mut child = term.spawn(
        case.command(None, Some("CURSOR_AGENT"))
            .stdin(Stdio::piped()),
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PUSH.as_bytes())
        .unwrap();
    assert!(term.wait_for(PROMPT, 30), "{}", term.shown());
    assert!(!term.echo_on(), "echo was on at the prompt");
    term.interrupt();
    let out = pty::wait_output(child, 30);
    assert!(!out.status.success());
    assert!(term.echo_on(), "echo stayed off after the interrupt");
}
