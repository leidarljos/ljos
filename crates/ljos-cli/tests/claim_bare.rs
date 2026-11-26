//! `ljos claim` with no id takes the first ready ticket in `ljos ready`
//! order, every time and not only before the claim graph exists. A ticket
//! another seat holds is passed over, a seat that already holds one gets
//! the busy refusal, and a host with no tracker names `ljos onboard`.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;
#[path = "common/sibling.rs"]
mod sibling;

use std::ffi::CString;
use std::io::Read as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const NO_READY_TICKET: &str =
    "claim: no ticket is ready on this seat, so file one with `ljos file \"TITLE\" -p PROJECT`";

/// A claim stamp from today, so no held ticket here is long-stale.
fn today_stamp() -> String {
    let out = ljos_cli::program("date")
        .unwrap()
        .arg("+[%Y-%m-%d %a %H:%M]")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// One host: a shared tracker and claim graph, and a home for each seat.
struct World {
    _dir: tempfile::TempDir,
    path: String,
    tracker: PathBuf,
    homes: PathBuf,
    graph: PathBuf,
}

impl World {
    /// `tickets` are ids, each TODO, or STARTED under the named holder.
    fn new(tickets: &[(&str, Option<&str>)]) -> World {
        let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
        let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
        let siblings = vissue.parent().unwrap();
        assert_eq!(claimdag.parent(), Some(siblings));
        let dir = tempfile::tempdir().unwrap();
        let tracker = dir.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software/demo")).unwrap();
        ljos_cli::write_file(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let at = today_stamp();
        let mut org = String::from(
            "#+TITLE: demo\n#+VISSUE: 1\n#+CATEGORY: demo\n#+TODO: TODO STARTED | DONE CANCELLED\n\n",
        );
        for (id, held) in tickets {
            match held {
                Some(who) => org.push_str(&format!(
                    "* STARTED [#C] Ticket {id}\n:PROPERTIES:\n:ID:         {id}\n:CLAIMED_BY: {who}\n:CLAIMED_AT: {at}\n:END:\n\n"
                )),
                None => org.push_str(&format!(
                    "* TODO [#C] Ticket {id}\n:PROPERTIES:\n:ID:         {id}\n:END:\n\n"
                )),
            }
        }
        ljos_cli::write_file(tracker.join("Software/demo/issues.org"), org).unwrap();
        World {
            path: format!("{}:/usr/bin:/bin", siblings.display()),
            tracker,
            homes: dir.path().join("homes"),
            graph: dir.path().join("claims"),
            _dir: dir,
        }
    }

    fn cmd(&self, bin: &Path, seat: &str) -> Command {
        let home = self.homes.join(seat);
        let runtime = home.join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        let mut cmd = ljos_cli::program(bin).unwrap();
        cmd.env_clear()
            .env("PATH", &self.path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("CLAIMDAG_DIR", &self.graph)
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .current_dir(&home)
            .stdin(Stdio::null());
        seat_env::clear(&mut cmd);
        cmd.env("LJOS_SEAT", seat);
        cmd
    }

    fn ljos(&self, seat: &str, args: &[&str]) -> Output {
        self.cmd(Path::new(env!("CARGO_BIN_EXE_ljos")), seat)
            .args(args)
            .output()
            .unwrap()
    }

    /// The id `ljos ready` lists first for `seat`.
    fn first_ready(&self, seat: &str) -> String {
        let out = self.ljos(seat, &["ready", "--json"]);
        assert!(out.status.success(), "{}", said(&out));
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        doc["rows"][0]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("no ready row: {doc}"))
            .to_string()
    }

    fn close(&self, seat: &str, id: &str) {
        let out = self
            .cmd(Path::new("vissue"), seat)
            .args(["update", id, "--state", "DONE"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", said(&out));
    }

    fn org(&self) -> String {
        std::fs::read_to_string(self.tracker.join("Software/demo/issues.org")).unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The id a successful bare claim names first on stdout.
fn claimed(out: &Output) -> String {
    assert!(out.status.success(), "{}", said(out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("nothing claimed: {}", said(out)))
        .to_string()
}

fn fifo(path: &Path) {
    let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: c_path is a NUL-terminated path and mode has no invalid bits.
    let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    assert_eq!(
        rc,
        0,
        "mkfifo {}: {}",
        path.display(),
        std::io::Error::last_os_error()
    );
}

/// How long the whole race test may take, from its start.
const RACE_DEADLINE: Duration = Duration::from_secs(60);

/// How long a seat killed at the deadline gets to be reaped, and how long
/// its pipes get to close after the deadline.
const REAP_GRACE: Duration = Duration::from_secs(5);

/// Kill `child` and everything it started. Each seat runs in a process
/// group of its own, so the gated claimdag shim goes with it. Call it only
/// before `child` is reaped, while its pid still names that group.
fn kill_group(child: &Child) {
    let group = libc::pid_t::try_from(child.id()).unwrap();
    // SAFETY: a negative pid names the process group the child leads.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
}

/// `child`'s status once it is reaped, or `None` when it is still not
/// reaped at `by`.
fn reap_by(child: &mut Child, by: Instant) -> Option<std::process::ExitStatus> {
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        if Instant::now() >= by {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A seat's stdout and stderr, read on threads that send what they read.
struct Pipes {
    said: std::sync::mpsc::Receiver<(bool, Vec<u8>)>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Pipes {
    fn take(child: &mut Child) -> Self {
        let (tx, said) = std::sync::mpsc::channel();
        let drain = |pipe: Box<dyn std::io::Read + Send>, err: bool| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut pipe = pipe;
                let mut chunk = [0u8; 8192];
                while let Ok(n @ 1..) = pipe.read(&mut chunk) {
                    if tx.send((err, chunk[..n].to_vec())).is_err() {
                        break;
                    }
                }
            });
        };
        if let Some(pipe) = child.stdout.take() {
            drain(Box::new(pipe), false);
        }
        if let Some(pipe) = child.stderr.take() {
            drain(Box::new(pipe), true);
        }
        Self {
            said,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    /// Read until both pipes close or `by` passes. True when both closed.
    fn drain_by(&mut self, by: Instant) -> bool {
        loop {
            let left = by.saturating_duration_since(Instant::now());
            match self.said.recv_timeout(left) {
                Ok((false, bytes)) => self.stdout.extend(bytes),
                Ok((true, bytes)) => self.stderr.extend(bytes),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return true,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return false,
            }
        }
    }

    fn text(&self) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        )
    }
}

/// The output of `child`, or a failure naming `what` when it has not
/// exited and closed its pipes by `deadline`.
///
/// Nothing here waits past `deadline` plus [`REAP_GRACE`]: a seat still
/// running at the deadline is killed with its group and reaped within the
/// grace, and a pipe some escaped process still holds is given up on.
fn finish(mut child: Child, deadline: Instant, what: &str) -> Output {
    let mut pipes = Pipes::take(&mut child);
    let status = match reap_by(&mut child, deadline) {
        Some(status) => status,
        None => {
            kill_group(&child);
            let reaped = reap_by(&mut child, Instant::now() + REAP_GRACE);
            pipes.drain_by(Instant::now() + REAP_GRACE);
            match reaped {
                Some(_) => panic!(
                    "{what} did not exit within {RACE_DEADLINE:?}:\n{}",
                    pipes.text()
                ),
                None => panic!(
                    "{what} was not reaped {REAP_GRACE:?} after it was killed:\n{}",
                    pipes.text()
                ),
            }
        }
    };
    if !pipes.drain_by(deadline.max(Instant::now()) + REAP_GRACE) {
        panic!(
            "{what} exited but its pipes stayed open past the deadline:\n{}",
            pipes.text()
        );
    }
    Output {
        status,
        stdout: pipes.stdout,
        stderr: pipes.stderr,
    }
}

/// Run `finish` on a thread, and fail when it has not returned in `limit`.
/// The panic message `finish` gave, if any, is returned.
fn finish_returns_within(child: Child, deadline: Instant, limit: Duration) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            finish(child, deadline, "the probe")
        }));
        let _ = tx.send(caught.err().map(|err| {
            err.downcast_ref::<String>()
                .cloned()
                .or_else(|| err.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default()
        }));
    });
    rx.recv_timeout(limit)
        .unwrap_or_else(|_| panic!("finish did not return within {limit:?}"))
}

fn sh(script: &str) -> Command {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a shell probe for the race test's deadline"
    )]
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn kill_pid(pid: u32) {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: kill with a positive pid signals only that process.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
}

#[test]
fn the_race_deadline_gives_up_on_a_pipe_an_escaped_process_holds() {
    let child = sh("setsid sleep 300 & echo \"held $!\"; exit 0")
        .process_group(0)
        .spawn()
        .unwrap();
    let started = Instant::now();
    let failed = finish_returns_within(
        child,
        Instant::now() + Duration::from_millis(200),
        Duration::from_secs(30),
    )
    .expect("finish passed with a pipe still held");
    if let Some(pid) = failed
        .split_whitespace()
        .skip_while(|word| *word != "held")
        .nth(1)
        .and_then(|pid| pid.parse().ok())
    {
        kill_pid(pid);
    }
    assert!(failed.contains("pipes stayed open"), "{failed}");
    assert!(
        started.elapsed() < REAP_GRACE * 2,
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn the_race_deadline_does_not_wait_forever_on_a_seat_the_kill_missed() {
    let child = sh("exec sleep 300").spawn().unwrap();
    let pid = child.id();
    let started = Instant::now();
    let failed = finish_returns_within(
        child,
        Instant::now() + Duration::from_millis(200),
        Duration::from_secs(30),
    );
    kill_pid(pid);
    let failed = failed.expect("finish passed with the seat still running");
    assert!(failed.contains("not reaped"), "{failed}");
    assert!(
        started.elapsed() < REAP_GRACE * 3,
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn simultaneous_bare_claims_pass_over_a_node_claimed_during_mint() {
    let deadline = Instant::now() + RACE_DEADLINE;
    let world = World::new(&[("demo-aa01", None), ("demo-bb02", None)]);
    let gate = tempfile::tempdir().unwrap();
    let waiting_fifo = gate.path().join("waiting");
    let release_fifo = gate.path().join("release");
    fifo(&waiting_fifo);
    fifo(&release_fifo);
    let real_claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let race_node = ljos_cli::work_id("demo-aa01");
    ljos_cli::install_executable(
        &gate.path().join("claimdag"),
        r#"#!/bin/sh
if [ "${LJOS_SEAT:-}" = seat0 ] && [ "${1:-}" = upsert ]; then
  case " $* " in
    *" --id $RACE_NODE "*)
      printf x > "$WAITING_FIFO"
      IFS= read -r _ < "$RELEASE_FIFO"
      ;;
  esac
fi
"$REAL_CLAIMDAG" "$@"
rc=$?
if [ "${LJOS_SEAT:-}" != seat0 ] &&
   [ "${1:-}" = claim ] &&
   [ "${2:-}" = "$RACE_NODE" ] &&
   [ "$rc" -eq 0 ]; then
  printf x > "$RELEASE_FIFO"
fi
exit "$rc"
"#,
    )
    .unwrap();
    let path = format!("{}:{}", gate.path().display(), world.path);
    let configure = |cmd: &mut Command| {
        cmd.arg("claim")
            .env("PATH", &path)
            .env("REAL_CLAIMDAG", &real_claimdag)
            .env("RACE_NODE", &race_node)
            .env("WAITING_FIFO", &waiting_fifo)
            .env("RELEASE_FIFO", &release_fifo)
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    };

    #[expect(
        clippy::disallowed_methods,
        reason = "opens a FIFO read-write so setup does not block before the child starts"
    )]
    let mut waiting = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&waiting_fifo)
        .unwrap();
    let mut slow_cmd = world.cmd(Path::new(env!("CARGO_BIN_EXE_ljos")), "seat0");
    configure(&mut slow_cmd);
    let mut slow = slow_cmd.spawn().unwrap();
    let (gate_tx, gate_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut signal = [0u8; 1];
        let _ = gate_tx.send(waiting.read_exact(&mut signal));
    });
    loop {
        match gate_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(read) => {
                read.unwrap();
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("the gate reader stopped: {err}"),
        }
        if slow.try_wait().unwrap().is_some() {
            let out = finish(slow, deadline, "seat0");
            panic!(
                "seat0 exited before it reached the upsert gate:\n{}",
                said(&out)
            );
        }
        if Instant::now() >= deadline {
            let out = finish(slow, deadline, "seat0 waiting for the upsert gate");
            panic!("seat0 exited at the deadline:\n{}", said(&out));
        }
    }

    let mut fast_cmd = world.cmd(Path::new(env!("CARGO_BIN_EXE_ljos")), "seat1");
    configure(&mut fast_cmd);
    let fast = fast_cmd.spawn().unwrap();
    let fast = finish(fast, deadline, "seat1");
    let slow = finish(slow, deadline, "seat0");
    assert_eq!(claimed(&fast), "demo-aa01", "{}", said(&fast));
    assert_eq!(claimed(&slow), "demo-bb02", "{}", said(&slow));
}

#[test]
fn another_seat_takes_the_next_ticket() {
    let world = World::new(&[
        ("demo-aa01", None),
        ("demo-bb02", None),
        ("demo-cc03", None),
    ]);
    let first = world.first_ready("alice");
    assert_eq!(first, "demo-aa01");
    assert_eq!(claimed(&world.ljos("alice", &["claim"])), first);
    let next = world.ljos("bob", &["claim"]);
    assert_eq!(claimed(&next), "demo-bb02", "{}", said(&next));
    let org = world.org();
    assert!(org.contains(":CLAIMED_BY: alice"), "{org}");
    assert!(org.contains(":CLAIMED_BY: bob"), "{org}");
}

#[test]
fn the_same_seat_is_refused_while_it_holds_one_then_takes_the_next() {
    let world = World::new(&[
        ("demo-aa01", None),
        ("demo-bb02", None),
        ("demo-cc03", None),
    ]);
    assert_eq!(claimed(&world.ljos("alice", &["claim"])), "demo-aa01");

    let busy = world.ljos("alice", &["claim"]);
    let text = said(&busy);
    assert!(
        text.contains("label claim-conflict:")
            && text.contains("alice already holds a live node")
            && text.contains("holds demo-aa01:")
            && text.contains("`ljos release demo-aa01 --assignee alice`"),
        "{text}"
    );
    assert_eq!(world.org().matches(":CLAIMED_BY: alice").count(), 1);

    let released = world.ljos("alice", &["release", "demo-aa01"]);
    assert!(released.status.success(), "{}", said(&released));
    let first = world.first_ready("alice");
    assert_eq!(claimed(&world.ljos("alice", &["claim"])), first);

    world.close("alice", &first);
    let next = world.ljos("alice", &["claim"]);
    assert_eq!(claimed(&next), "demo-bb02", "{}", said(&next));
}

#[test]
fn every_ticket_held_by_another_seat_gives_one_sentence() {
    let world = World::new(&[("demo-aa01", Some("carol")), ("demo-bb02", Some("carol"))]);
    let out = world.ljos("bob", &["claim"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stdout}{stderr}");
    assert!(stdout.trim().is_empty(), "{stdout}");
    let last = stderr.lines().last().unwrap_or("");
    assert_eq!(last, NO_READY_TICKET, "{stderr}");
    assert!(!world.org().contains(":CLAIMED_BY: bob"), "{}", world.org());
}

#[test]
fn a_home_that_was_never_onboarded_names_ljos_onboard() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("run")).unwrap();
    let mut cmd = ljos_cli::program(env!("CARGO_BIN_EXE_ljos")).unwrap();
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PACKSET_URL", "off")
        .current_dir(&home)
        .stdin(Stdio::null());
    seat_env::clear(&mut cmd);
    cmd.env("LJOS_SEAT", "alice");
    let out = cmd.arg("claim").output().unwrap();
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("`ljos onboard`"), "{text}");
    assert!(!text.contains("ljos file"), "{text}");
}
