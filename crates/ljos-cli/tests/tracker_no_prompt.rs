//! ljos-4c67: a tracker remote that wants a credential costs a sitting
//! and a note nothing. Git is never let ask, on a terminal or through an
//! askpass program. The write is committed to the local tracker, one line
//! says the remote was not reached, the verb exits 0, and a later
//! `ljos sync` with a reachable remote pushes what was saved.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// The tracker push wait each run is given, ljos's default (`push_wait`):
/// half the timeout of about ten seconds a runner wraps a sitting in.
const PUSH_WAIT_SECS: u64 = 5;
/// That runner timeout, two push waits. A remote that fails at once
/// leaves both verbs far inside it; a prompt never returns.
const BUDGET: Duration = Duration::from_secs(2 * PUSH_WAIT_SECS);
/// How long the 401 server waits for git's request, half a push wait: git
/// writes it as soon as it connects.
const REQUEST_WAIT: Duration = Duration::from_secs(PUSH_WAIT_SECS / 2);
/// What one read of that request takes. git's first request is a GET of
/// `info/refs` with a few headers, a few hundred bytes; one page holds it.
const REQUEST_BYTES: usize = 4096;

const NOT_REACHED: &str = "the remote was not reached (";
const NEXT_SYNC: &str = "), so the next sync pushes it";

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|bin| bin.is_file())
}

fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Answers every request 401 with a Basic challenge, as a private
/// repository does to a clone or push that brought no token.
fn private_remote() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/private.git", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            seen.fetch_add(1, Ordering::SeqCst);
            let _ = sock.set_read_timeout(Some(REQUEST_WAIT));
            let mut buf = [0u8; REQUEST_BYTES];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"tracker\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    (url, hits)
}

struct Board {
    _dir: tempfile::TempDir,
    root: PathBuf,
    tracker: PathBuf,
    reachable: PathBuf,
    gitconfig: PathBuf,
    asked: PathBuf,
    path: String,
    vissue: PathBuf,
}

impl Board {
    fn new(vissue: &Path) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let stubs = root.join("stubs");
        for sub in ["stubs", "home/run", "seed"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        let asked = root.join("asked");
        // Twice the budget: an askpass that ran holds the verb past it.
        let askpass = root.join("askpass");
        script(
            &askpass,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nsleep {}\necho nobody\n",
                asked.display(),
                BUDGET.as_secs() * 2
            ),
        );
        let gitconfig = root.join("gitconfig");
        std::fs::write(
            &gitconfig,
            format!(
                "[user]\n\tname = seat\n\temail = seat@example.invalid\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n[core]\n\taskPass = {}\n",
                askpass.display()
            ),
        )
        .unwrap();
        script(
            &stubs.join("claimdag"),
            "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\nesac\necho '[]'\nexit 0\n",
        );
        script(&stubs.join("deedar"), "#!/bin/sh\necho head\nexit 0\n");
        let path = format!(
            "{}:{}:/usr/bin:/bin",
            stubs.display(),
            vissue.parent().unwrap().display()
        );
        let reachable = root.join("reachable.git");
        let tracker = root.join("tracker");
        let board = Self {
            _dir: dir,
            root,
            tracker,
            reachable,
            gitconfig,
            asked,
            path,
            vissue: vissue.to_path_buf(),
        };
        board.git(&board.root, &["init", "-q", "--bare", "reachable.git"]);
        let seed = board.root.join("seed");
        board.git(&seed, &["init", "-q"]);
        board.git(&seed, &["config", "core.hooksPath", "/dev/null"]);
        board
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn env(&self, cmd: &mut Command, root: &Path) {
        let home = self.root.join("home");
        cmd.env_clear()
            .env("PATH", &self.path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("VISSUE_ROOT", root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("VISSUE_AGENT", "ada")
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            // A login that lets git ask: both askpass variables name the
            // program, and the terminal prompt is allowed.
            .env("GIT_TERMINAL_PROMPT", "1")
            .env("GIT_ASKPASS", self.root.join("askpass"))
            .env("SSH_ASKPASS", self.root.join("askpass"));
    }

    /// Two tickets on the seed, pushed to the reachable remote and cloned,
    /// then the clone's origin pointed at `unreachable`.
    fn tickets(&self, unreachable: &str) -> (String, String) {
        let seed = self.root.join("seed");
        let file = |title: &str| {
            let mut cmd = Command::new(&self.vissue);
            self.env(&mut cmd, &seed);
            let out = cmd
                .args(["q", "-p", "probe", title])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "vissue q: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let pushed = file("sat in push mode");
        let waited = file("sat in the default mode");
        self.git(&seed, &["add", "-A"]);
        self.git(&seed, &["commit", "-q", "-m", "seed"]);
        let reachable = self.reachable.to_str().unwrap();
        self.git(&seed, &["push", "-q", reachable, "HEAD:refs/heads/main"]);
        self.git(
            &self.root,
            &["clone", "-q", reachable, self.tracker.to_str().unwrap()],
        );
        self.git(&self.tracker, &["config", "core.hooksPath", "/dev/null"]);
        self.git(&self.tracker, &["remote", "set-url", "origin", unreachable]);
        (pushed, waited)
    }

    /// `ljos` in the tracker with `mode` as `LJOS_TRACKER_GIT` (unset when
    /// `None`). Stdin is a pipe held open and never written, so a read of
    /// it waits out the budget. Past the budget the run is killed.
    fn ljos(&self, mode: Option<&str>, args: &[&str]) -> (Output, Duration) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        self.env(&mut cmd, &self.tracker);
        cmd.args(args)
            .current_dir(&self.tracker)
            .env("LJOS_SEAT", "ada")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_URL", "http://127.0.0.1:1")
            .env("LJOS_TRACKER_PUSH_WAIT", PUSH_WAIT_SECS.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(mode) = mode {
            cmd.env("LJOS_TRACKER_GIT", mode);
        }
        let started = Instant::now();
        let mut child = cmd.spawn().unwrap();
        let held = child.stdin.take();
        let pid = child.id();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        let Ok(out) = rx.recv_timeout(BUDGET) else {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
            panic!(
                "ljos {} ran past {BUDGET:?}; askpass calls: {:?}",
                args.join(" "),
                std::fs::read_to_string(&self.asked).unwrap_or_default()
            );
        };
        drop(held);
        (out.unwrap(), started.elapsed())
    }

    fn head_file(&self) -> String {
        self.git(&self.tracker, &["show", "HEAD:Software/probe/issues.org"])
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The verb exited 0 in time and said once that the remote was not reached.
fn saved_locally(board: &Board, mode: Option<&str>, args: &[&str]) -> String {
    let (out, took) = board.ljos(mode, args);
    let said = text(&out);
    assert!(
        out.status.success(),
        "ljos {} failed in {took:?}:\n{said}",
        args.join(" ")
    );
    let lines: Vec<&str> = said.lines().filter(|l| l.contains(NOT_REACHED)).collect();
    assert_eq!(
        lines.len(),
        1,
        "ljos {} (LJOS_TRACKER_GIT={mode:?}) did not say once that the remote was not reached:\n{said}",
        args.join(" ")
    );
    assert!(
        lines[0].starts_with("tracker git: committed ") && lines[0].ends_with(NEXT_SYNC),
        "{said}"
    );
    said
}

fn saves_and_later_syncs(board: &Board, unreachable: &str) {
    let (pushed, waited) = board.tickets(unreachable);
    let notes = [
        (Some("push"), "the parser kept the hyphen"),
        (None, "the release ships the hyphen"),
    ];
    for (mode, note) in notes {
        saved_locally(board, mode, &["note", &pushed, note]);
        assert!(
            board.head_file().contains(note),
            "the note is not in the local tracker's HEAD:\n{}",
            board.head_file()
        );
    }
    for (mode, issue) in [(Some("push"), &pushed), (None, &waited)] {
        let said = saved_locally(board, mode, &["sitting", issue]);
        assert!(said.contains(&format!("{issue} is yours")), "{said}");
    }
    assert!(
        !board.asked.exists(),
        "git ran the askpass program: {}",
        std::fs::read_to_string(&board.asked).unwrap_or_default()
    );
    let (out, _) = board.ljos(None, &["doctor"]);
    let said = text(&out);
    let unpushed = said
        .split(" unpushed")
        .next()
        .and_then(|before| before.rsplit(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse::<u64>().ok());
    assert!(
        unpushed.is_some_and(|n| n > 0),
        "doctor does not count the commits:\n{said}"
    );
    assert!(!said.contains("last push refused"), "{said}");

    board.git(
        &board.tracker,
        &[
            "remote",
            "set-url",
            "origin",
            board.reachable.to_str().unwrap(),
        ],
    );
    let before = board.git(&board.reachable, &["rev-parse", "main"]);
    let (out, _) = board.ljos(Some("commit"), &["sync"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("sync: waiting commits not pushed (LJOS_TRACKER_GIT=commit)"),
        "{said}"
    );
    assert_eq!(board.git(&board.reachable, &["rev-parse", "main"]), before);
    let (out, _) = board.ljos(Some("push"), &["sync"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("sync: pushed pending commits"), "{said}");
    let remote = Command::new("git")
        .arg("--git-dir")
        .arg(&board.reachable)
        .args(["show", "main:Software/probe/issues.org"])
        .env("GIT_CONFIG_GLOBAL", &board.gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let remote = String::from_utf8_lossy(&remote.stdout);
    for (_, note) in notes {
        assert!(
            remote.contains(note),
            "the sync did not carry the note:\n{remote}"
        );
    }
    let log = board.git(&board.reachable, &["log", "--format=%s", "main"]);
    for issue in [&pushed, &waited] {
        assert!(
            log.contains(&format!("{issue} claimed by ada")),
            "the sync did not carry the claim on {issue}:\n{log}"
        );
    }
}

/// vissue from PATH. Off CI a missing one skips the test; in CI it fails,
/// so a sibling build that was dropped does not pass every test here.
fn vissue() -> Option<PathBuf> {
    let found = on_path("vissue");
    assert!(
        found.is_some() || std::env::var_os("CI").is_none(),
        "vissue missing in CI"
    );
    if found.is_none() {
        eprintln!("skip: vissue is not on PATH");
    }
    found
}

#[test]
fn a_remote_that_answers_401_costs_a_note_and_a_sitting_nothing() {
    let Some(vissue) = vissue() else {
        return;
    };
    let board = Board::new(&vissue);
    let (url, hits) = private_remote();
    saves_and_later_syncs(&board, &url);
    assert!(
        hits.load(Ordering::SeqCst) > 0,
        "the 401 remote was never asked"
    );
}

#[test]
fn a_file_url_that_does_not_exist_costs_a_note_and_a_sitting_nothing() {
    let Some(vissue) = vissue() else {
        return;
    };
    let board = Board::new(&vissue);
    let missing = format!("file://{}", board.root.join("missing.git").display());
    saves_and_later_syncs(&board, &missing);
}

/// An ssh that writes its arguments to `log`, answers git's `-G` probe as
/// OpenSSH does, and finds the port closed.
fn ssh_recorder(path: &Path, log: &Path) {
    script(
        path,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \" $* \" in *\" -G \"*) exit 0 ;; esac\n\
             echo 'ssh: connect to host 127.0.0.1 port 1: Connection refused' >&2\nexit 255\n",
            log.display()
        ),
    );
}

#[test]
fn ssh_runs_in_batch_mode_and_a_set_ssh_command_still_runs() {
    let Some(vissue) = vissue() else {
        return;
    };
    let board = Board::new(&vissue);
    let ssh_log = board.root.join("ssh.log");
    ssh_recorder(&board.root.join("stubs/ssh"), &ssh_log);
    let (pushed, waited) = board.tickets("ssh://git@127.0.0.1:1/tracker.git");
    saved_locally(
        &board,
        Some("push"),
        &["note", &pushed, "the key has a passphrase"],
    );
    saved_locally(&board, None, &["sitting", &waited]);
    let calls = std::fs::read_to_string(&ssh_log).unwrap_or_default();
    let connects: Vec<&str> = calls
        .lines()
        .filter(|l| !l.split(' ').any(|w| w == "-G"))
        .collect();
    assert!(!connects.is_empty(), "git never ran ssh:\n{calls}");
    for call in &connects {
        assert!(
            call.starts_with("-o BatchMode=yes "),
            "ssh ran without batch mode: {call}"
        );
    }

    let own_log = board.root.join("own-ssh.log");
    let own = board.root.join("own-ssh");
    ssh_recorder(&own, &own_log);
    std::fs::remove_file(&ssh_log).unwrap();
    board.git(
        &board.tracker,
        &["config", "core.sshCommand", own.to_str().unwrap()],
    );
    saved_locally(
        &board,
        Some("push"),
        &["note", &pushed, "the second key signs in"],
    );
    let calls = std::fs::read_to_string(&own_log).unwrap_or_default();
    assert!(
        calls.lines().any(|l| l.contains("127.0.0.1")),
        "core.sshCommand did not run:\n{calls}"
    );
    assert!(!ssh_log.exists(), "ssh ran over core.sshCommand");
}
