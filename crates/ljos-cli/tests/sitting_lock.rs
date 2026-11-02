//! A sitting announces itself before another tracker writer can hold it.
//! The issue stays TODO when an issue or tracker-git lock outlasts the
//! claim budget, and moves to STARTED as soon as a writer that finishes
//! inside the budget releases the issue lock.

ljos_test_boot::harness!();

use std::io::{BufRead, Read};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

// One second leaves ample process-startup slack while enforcing the
// requirement that the first line arrive within a couple of seconds.
const FIRST_LINE_LIMIT: Duration = Duration::from_secs(1);
// A one-second budget keeps the held-lock test fast while exercising expiry.
const HELD_CLAIM_BUDGET_SECS: u64 = 1;
// The one-second claim budget gets two seconds for process teardown and
// loaded test-runner scheduling before the assertion calls it unbounded.
const HELD_LOCK_LIMIT: Duration = Duration::from_secs(3);
// Keep the lock long enough for the sitting to encounter it, without using
// a meaningful share of the released-lock test's five-second budget.
const RELEASE_AFTER: Duration = Duration::from_millis(200);
// Five seconds leaves the release comfortably inside the active budget.
const RELEASED_CLAIM_BUDGET_SECS: u64 = 5;
// A ten-millisecond status poll adds negligible slack to the wall assertions.
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Board {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    run: PathBuf,
    issue: String,
}

impl Board {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        let run = dir.path().join("run");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&run).unwrap();
        let layout = vissue_core::Layout::new(&root, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        vissue_core::ops::create(&layout, "probe", "wait for the writer", Default::default())
            .unwrap();
        let initialized = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-q", "-b", "main"])
            .status()
            .unwrap();
        assert!(initialized.success());
        let issue =
            vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe"))
                .unwrap()
                .headings[0]
                .id
                .clone();

        script(
            dir.path(),
            "vissue",
            &format!(
                r#"#!/bin/sh
case "$1" in
  identity) printf '%s\n' 'vissue test' 'root={}' 'prefix=Software' ;;
  claim)
    sed -i '0,/^\* TODO /s//\* STARTED /' "$VISSUE_ROOT/Software/probe/issues.org"
    printf 'tracker: %s STARTED under saddle\n' "$2"
    ;;
  note) printf '%s: noted\n' "$2" ;;
  recall) echo recalled ;;
  show) echo '{{}}' ;;
  *) echo '[]' ;;
esac
"#,
                root.display()
            ),
        );
        script(
            dir.path(),
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\n",
        );
        script(dir.path(), "deedar", "#!/bin/sh\necho head\n");

        Self {
            _dir: dir,
            root,
            home,
            run,
            issue,
        }
    }

    fn issue_path(&self) -> PathBuf {
        self.root.join("Software/probe/issues.org")
    }

    fn lock_path(&self) -> PathBuf {
        let mut name = self.issue_path().into_os_string();
        name.push(".lock");
        PathBuf::from(name)
    }

    fn tracker_lock_path(&self) -> PathBuf {
        self.root.join(ljos_cli::TRACKER_LOCK)
    }

    fn hold_lock(path: &Path) -> std::fs::File {
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)
            .unwrap();
        // SAFETY: the file stays open until the test drops it.
        assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);
        lock
    }

    fn hold_issue_lock(&self) -> std::fs::File {
        Self::hold_lock(&self.lock_path())
    }

    fn hold_tracker_lock(&self) -> std::fs::File {
        Self::hold_lock(&self.tracker_lock_path())
    }

    fn sitting(&self, budget_secs: u64) -> Child {
        self.sitting_with_tracker(budget_secs, "off")
    }

    fn sitting_with_tracker(&self, budget_secs: u64, tracker_git: &str) -> Child {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["sitting", &self.issue, "--verbose"])
            .current_dir(self._dir.path())
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self._dir.path().display()),
            )
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", tracker_git)
            .env("LJOS_HOOKD", "off")
            .env("LJOS_LEASE_SECS", "0")
            .env("LJOS_SITTING_CLAIM_WAIT", budget_secs.to_string())
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn state(&self) -> String {
        vissue_core::IssueDoc::parse_file("probe", &self.issue_path())
            .unwrap()
            .headings
            .into_iter()
            .find(|heading| heading.id == self.issue)
            .unwrap()
            .state
    }
}

fn first_line(child: &mut Child) -> (String, std::io::BufReader<std::process::ChildStdout>) {
    let stdout = child.stdout.take().unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut stdout = std::io::BufReader::new(stdout);
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        sent.send((line, stdout)).unwrap();
    });
    match received.recv_timeout(FIRST_LINE_LIMIT) {
        Ok(first) => first,
        Err(why) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("first line missed its {FIRST_LINE_LIMIT:?} limit: {why}");
        }
    }
}

fn finish(
    mut child: Child,
    mut stdout: std::io::BufReader<std::process::ChildStdout>,
    limit: Duration,
) -> (ExitStatus, String, String) {
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait().unwrap() {
            Some(status) => break status,
            None if Instant::now() < deadline => std::thread::sleep(PROCESS_POLL_INTERVAL),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("sitting missed its {limit:?} exit limit");
            }
        }
    };
    let mut out = String::new();
    stdout.read_to_string(&mut out).unwrap();
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    (status, out, err)
}

#[test]
fn a_held_vissue_lock_prints_at_once_and_exits_inside_the_budget() {
    let board = Board::new();
    let _held = board.hold_issue_lock();
    let started = Instant::now();
    let mut child = board.sitting(HELD_CLAIM_BUDGET_SECS);
    let (line, stdout) = first_line(&mut child);
    assert_eq!(
        line,
        format!("sitting {}: checking blockers\n", board.issue)
    );
    assert!(
        started.elapsed() < FIRST_LINE_LIMIT,
        "first line took {:?}",
        started.elapsed()
    );
    let (status, out, err) = finish(
        child,
        stdout,
        HELD_LOCK_LIMIT.saturating_sub(started.elapsed()),
    );
    assert_eq!(status.code(), Some(1), "stdout:\n{out}\nstderr:\n{err}");
    assert!(
        started.elapsed() < HELD_LOCK_LIMIT,
        "held lock took {:?}",
        started.elapsed()
    );
    assert!(err.contains("vissue lock"), "{err}");
    assert!(
        err.contains(&board.lock_path().display().to_string()),
        "{err}"
    );
    assert!(err.contains("nothing was claimed"), "{err}");
    assert!(!out.contains("== claim"), "{out}");
    assert_eq!(board.state(), "TODO", "{out}\n{err}");
}

#[test]
fn a_held_tracker_git_lock_exits_before_the_claim() {
    let board = Board::new();
    let _held = board.hold_tracker_lock();
    let started = Instant::now();
    let mut child = board.sitting_with_tracker(HELD_CLAIM_BUDGET_SECS, "commit");
    let (line, stdout) = first_line(&mut child);
    assert_eq!(
        line,
        format!("sitting {}: checking blockers\n", board.issue)
    );
    let (status, out, err) = finish(
        child,
        stdout,
        HELD_LOCK_LIMIT.saturating_sub(started.elapsed()),
    );
    assert_eq!(status.code(), Some(1), "stdout:\n{out}\nstderr:\n{err}");
    assert!(
        started.elapsed() < HELD_LOCK_LIMIT,
        "held lock took {:?}",
        started.elapsed()
    );
    assert!(err.contains("tracker git lock"), "{err}");
    assert!(
        err.contains(&board.tracker_lock_path().display().to_string()),
        "{err}"
    );
    assert!(err.contains("nothing was claimed"), "{err}");
    assert!(!out.contains("== claim"), "{out}");
    assert_eq!(board.state(), "TODO", "{out}\n{err}");
}

#[test]
fn a_vissue_lock_released_inside_the_budget_claims_the_ticket() {
    let board = Board::new();
    let held = board.hold_issue_lock();
    let started = Instant::now();
    let mut child = board.sitting(RELEASED_CLAIM_BUDGET_SECS);
    let (line, stdout) = first_line(&mut child);
    assert_eq!(
        line,
        format!("sitting {}: checking blockers\n", board.issue)
    );
    std::thread::sleep(RELEASE_AFTER);
    drop(held);
    let released_limit = Duration::from_secs(RELEASED_CLAIM_BUDGET_SECS);
    let (status, out, err) = finish(
        child,
        stdout,
        released_limit.saturating_sub(started.elapsed()),
    );
    assert!(status.success(), "stdout:\n{out}\nstderr:\n{err}");
    assert!(
        started.elapsed() < released_limit,
        "released lock took {:?}",
        started.elapsed()
    );
    assert!(out.contains("== claim"), "{out}");
    assert_eq!(board.state(), "STARTED", "{out}\n{err}");
}
