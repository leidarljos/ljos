//! One admission point for `ljos hook`. Every tool call on a host used to
//! start a full hook process at once, so a burst of thirty calls from six
//! seats was thirty processes contending for the same CPUs and memory.
//! `ljos hookd`, one per user, hands out a fixed number of slots over a
//! unix socket, in the order the calls asked. A gate call that gets no
//! slot in time is refused: a gate that cannot judge must not allow.
//!
//! The judging stays in the calling process. The guard reads that
//! process's parent, session and cgroup to tell the runner's call from a
//! tool's, and a server judging on its behalf would have to take the
//! caller's word for all of them.
//!
//! When no server answers, the first call starts one, and the calls that
//! meet no server take one host-wide lock in turn. A missing server costs
//! throughput, never the cap.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The longest a call waits for a slot. The runner's gate deadline counts
/// from the call's start, so the wait and the work share that budget.
pub const ADMIT_MS: u64 = 3_000;
/// Calls waiting at once before the next one is refused outright.
pub const QUEUE_CAP: usize = 64;
/// A slot held this long is taken back, so a stopped call cannot keep it.
const HOLD: Duration = Duration::from_secs(10);
/// An idle server exits after this long; the next call starts another.
const IDLE: Duration = Duration::from_secs(900);
/// The timing log is cut back past this size.
const LOG_CAP: u64 = 256 * 1024;
/// The longest socket path a `sockaddr_un` takes everywhere ljos runs:
/// 104 bytes with the NUL on macOS, 108 on Linux.
const SOCKET_PATH_MAX: usize = 103;

static STARTED: OnceLock<Instant> = OnceLock::new();
static STDIN: Mutex<Option<Vec<u8>>> = Mutex::new(None);
static HELD: Mutex<Option<Held>> = Mutex::new(None);
static WAITED_MS: AtomicU64 = AtomicU64::new(0);

/// What keeps this call's turn: a slot from the server, or the
/// single-flight lock when there was no server. Dropping it gives the
/// turn to the next call.
enum Held {
    Slot(#[allow(dead_code)] UnixStream),
    Flight(#[allow(dead_code)] std::fs::File),
}

/// When this process started its hook call. Deadlines count from here,
/// so time spent in the queue is spent from the same budget.
#[must_use]
pub fn started() -> Instant {
    *STARTED.get_or_init(Instant::now)
}

/// What is left of `budget_ms`, counted from [`started`].
#[must_use]
pub fn left_of(budget_ms: u64) -> Duration {
    Duration::from_millis(budget_ms).saturating_sub(started().elapsed())
}

/// The stdin [`admit`] read to learn the call's event, for the hook to
/// judge. `None` when admission did not read it.
#[must_use]
pub fn take_stdin() -> Option<Vec<u8>> {
    STDIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// Wait for this hook call's turn before any heavy start-up. Returns at
/// once for anything but a plain `ljos hook`. A gate that gets no turn
/// within [`ADMIT_MS`] is refused in the runner's JSON and the process
/// exits; any other event exits quietly, since it has nothing to stop.
pub fn admit() {
    let _ = started();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !admitted_command(&args)
        || std::env::var_os("LJOS_IN_HOOK").is_some()
        || std::env::var("LJOS_HOOKD").is_ok_and(|v| v == "off")
    {
        return;
    }
    let mut bytes = Vec::new();
    if std::io::stdin().read_to_end(&mut bytes).is_err() {
        return;
    }
    let input = String::from_utf8_lossy(&bytes).into_owned();
    *STDIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(bytes);
    let event = flag_value(&args, "--event");
    let gate = !args.iter().any(|a| a == "--prompt")
        && crate::is_gate_event(&crate::hook_call_as(&input, event.as_deref()).event);
    match wait_turn() {
        Ok((held, waited)) => {
            WAITED_MS.store(waited, Ordering::Relaxed);
            *HELD
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(held);
        }
        Err(why) => {
            note_time("refused", started().elapsed(), Duration::ZERO);
            let why = format!("hook queue: {why}");
            if gate {
                if let Some(text) = crate::hook_failure_output(&input, event.as_deref(), &why) {
                    let mut out = std::io::stdout().lock();
                    let _ = out.write_all(text.as_bytes());
                    let _ = out.flush();
                }
                eprintln!("ljos hook: {why}; the call is refused");
                std::process::exit(crate::GATE_REFUSED_EXIT);
            }
            eprintln!("ljos hook: {why}; this event is skipped");
            std::process::exit(0);
        }
    }
}

/// Give this call's turn to the next one and log how long the call took.
/// The gate's answer calls this once it is written.
pub fn release() {
    let held = HELD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if held.is_some() {
        let waited = Duration::from_millis(WAITED_MS.load(Ordering::Relaxed));
        note_time("hook", started().elapsed(), waited);
    }
}

/// Whether `args` (after the program name) are a hook call that reads a
/// payload and answers it. The git and runner forms, `--text` and the
/// queue's own verbs are not admitted here.
fn admitted_command(args: &[String]) -> bool {
    if args.first().map(String::as_str) != Some("hook") {
        return false;
    }
    !args.iter().skip(1).any(|a| {
        let name = a.split('=').next().unwrap_or(a);
        matches!(
            name,
            "--text"
                | "--git-commit"
                | "--git-hook"
                | "--brief-once"
                | "--runner"
                | "--drain"
                | "--apply-book"
                | "-h"
                | "--help"
        )
    })
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == flag {
            return it.next().cloned();
        }
        if let Some(value) = arg.strip_prefix(flag).and_then(|r| r.strip_prefix('=')) {
            return Some(value.to_string());
        }
    }
    None
}

/// The runtime directory, made private when missing.
fn run_dir() -> PathBuf {
    let dir = crate::runtime_dir();
    if !dir.exists() {
        use std::os::unix::fs::DirBuilderExt as _;
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir);
    }
    dir
}

fn env_num(key: &str) -> Option<u64> {
    std::env::var(key).ok()?.trim().parse().ok()
}

fn admit_budget() -> Duration {
    Duration::from_millis(env_num("LJOS_HOOK_ADMIT_MS").unwrap_or(ADMIT_MS))
}

enum Ask {
    NoServer,
    Refused(String),
}

/// A turn from the server, or with no server, the single-flight lock.
/// The error says why no turn came within the budget.
fn wait_turn() -> Result<(Held, u64), String> {
    let dir = run_dir();
    let sock = dir.join("hookd.sock");
    let deadline = started() + admit_budget();
    // No server can listen on a path this long; do not wait for one.
    if sock.as_os_str().len() > SOCKET_PATH_MAX {
        return flight(&dir, deadline);
    }
    match ask(&sock, deadline) {
        Ok(turn) => return Ok(turn),
        Err(Ask::Refused(why)) => return Err(why),
        Err(Ask::NoServer) => {}
    }
    let _starting = start_server(&dir);
    let retry_until = (Instant::now() + Duration::from_millis(200)).min(deadline);
    while Instant::now() < retry_until {
        std::thread::sleep(Duration::from_millis(5));
        match ask(&sock, deadline) {
            Ok(turn) => return Ok(turn),
            Err(Ask::Refused(why)) => return Err(why),
            Err(Ask::NoServer) => {}
        }
    }
    flight(&dir, deadline)
}

fn ask(sock: &Path, deadline: Instant) -> Result<(Held, u64), Ask> {
    let mut stream = UnixStream::connect(sock).map_err(|_| Ask::NoServer)?;
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(Ask::Refused(format!(
            "no slot within {} ms",
            admit_budget().as_millis()
        )));
    }
    // The server answers `late` at the deadline itself; the margin is for
    // a server that stopped answering.
    let _ = stream.set_read_timeout(Some(left + Duration::from_millis(250)));
    writeln!(stream, "wait {}", left.as_millis()).map_err(|_| Ask::NoServer)?;
    let mut line = String::new();
    let mut reader = BufReader::new(stream.try_clone().map_err(|_| Ask::NoServer)?);
    match reader.read_line(&mut line) {
        // A server that went away before it answered: the remaining
        // budget goes to the single-flight lock.
        Ok(0) => Err(Ask::NoServer),
        Ok(_) => {
            let mut words = line.split_whitespace();
            match (words.next(), words.next()) {
                (Some("go"), waited) => Ok((
                    Held::Slot(stream),
                    waited.and_then(|w| w.parse().ok()).unwrap_or(0),
                )),
                (Some("full"), waiting) => Err(Ask::Refused(format!(
                    "{} hook calls are already waiting for a slot",
                    waiting.unwrap_or("too many")
                ))),
                _ => Err(Ask::Refused(format!(
                    "no slot came free within {} ms",
                    admit_budget().as_millis()
                ))),
            }
        }
        Err(_) => Err(Ask::Refused(format!(
            "the hook server did not answer within {} ms",
            admit_budget().as_millis()
        ))),
    }
}

fn open_lock(path: &Path) -> Option<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .ok()
}

fn try_flock(file: &std::fs::File) -> bool {
    // SAFETY: flock on a descriptor `file` owns; the lock goes with it.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

/// Start `ljos hookd` unless another call is starting one. The returned
/// lock is held while this call waits for the socket, so a burst starts
/// one server, not one each.
fn start_server(dir: &Path) -> Option<std::fs::File> {
    use std::os::unix::process::CommandExt as _;
    let lock = open_lock(&dir.join("hookd.start"))?;
    if !try_flock(&lock) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("hookd")
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for key in [
        "PATH",
        "HOME",
        "USER",
        "XDG_RUNTIME_DIR",
        "TMPDIR",
        "LJOS_HOOK_SLOTS",
        "LJOS_HOOK_QUEUE",
    ] {
        if let Some(val) = std::env::var_os(key) {
            cmd.env(key, val);
        }
    }
    // SAFETY: setsid is async-signal-safe and touches only the child.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = cmd.spawn();
    Some(lock)
}

fn flight(dir: &Path, deadline: Instant) -> Result<(Held, u64), String> {
    let asked = Instant::now();
    let Some(file) = open_lock(&dir.join("hookd.flight")) else {
        return Err("no hook server, and the single-flight lock cannot be opened".into());
    };
    loop {
        if try_flock(&file) {
            let waited = u64::try_from(asked.elapsed().as_millis()).unwrap_or(u64::MAX);
            return Ok((Held::Flight(file), waited));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "no hook server, and the single-flight lock stayed taken for {} ms",
                admit_budget().as_millis()
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// How many hook calls may run at once: `LJOS_HOOK_SLOTS`, else the CPU
/// count between 2 and 8.
fn slots() -> usize {
    env_num("LJOS_HOOK_SLOTS")
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map_or(2, std::num::NonZeroUsize::get)
                .clamp(2, 8)
        })
}

fn queue_cap() -> usize {
    env_num("LJOS_HOOK_QUEUE")
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(QUEUE_CAP)
}

struct Turns {
    running: usize,
    queue: std::collections::VecDeque<u64>,
    next: u64,
    last: Instant,
}

struct Gate {
    turns: Mutex<Turns>,
    moved: Condvar,
    slots: usize,
    cap: usize,
}

impl Gate {
    fn lock(&self) -> std::sync::MutexGuard<'_, Turns> {
        self.turns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn idle(&self) -> bool {
        let turns = self.lock();
        turns.running == 0 && turns.queue.is_empty() && turns.last.elapsed() > IDLE
    }

    /// One call: queue it, hand it a slot in order, and take the slot back
    /// when the caller closes the socket, exits or holds it past [`HOLD`].
    fn serve(&self, stream: &UnixStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
        let mut line = String::new();
        if BufReader::new(stream).read_line(&mut line).is_err() {
            return;
        }
        let ms = line
            .strip_prefix("wait ")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(ADMIT_MS)
            .min(crate::CURSOR_GATE_DEADLINE_MS);
        let asked = Instant::now();
        let deadline = asked + Duration::from_millis(ms);
        let mut turns = self.lock();
        turns.last = asked;
        if turns.queue.len() >= self.cap {
            let waiting = turns.queue.len();
            drop(turns);
            let _ = writeln!(&mut &*stream, "full {waiting}");
            return;
        }
        let ticket = turns.next;
        turns.next += 1;
        turns.queue.push_back(ticket);
        loop {
            if turns.queue.front() == Some(&ticket) && turns.running < self.slots {
                turns.queue.pop_front();
                turns.running += 1;
                break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                turns.queue.retain(|t| *t != ticket);
                drop(turns);
                self.moved.notify_all();
                let _ = writeln!(&mut &*stream, "late");
                return;
            }
            turns = self
                .moved
                .wait_timeout(turns, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        drop(turns);
        self.moved.notify_all();
        let waited = asked.elapsed().as_millis();
        if writeln!(&mut &*stream, "go {waited}").is_ok() {
            let _ = stream.set_read_timeout(Some(HOLD));
            let mut buf = [0u8; 64];
            while matches!((&mut &*stream).read(&mut buf), Ok(n) if n > 0) {}
        }
        let mut turns = self.lock();
        turns.running -= 1;
        turns.last = Instant::now();
        drop(turns);
        self.moved.notify_all();
    }
}

#[cfg(target_os = "linux")]
fn same_user(stream: &UnixStream) -> bool {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = libc::socklen_t::try_from(std::mem::size_of::<libc::ucred>()).unwrap_or(0);
    // SAFETY: SO_PEERCRED writes one ucred into `cred`, of `len` bytes.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::addr_of_mut!(cred).cast(),
            &mut len,
        )
    };
    // SAFETY: geteuid only reads the process identity.
    rc == 0 && cred.uid == unsafe { libc::geteuid() }
}

#[cfg(not(target_os = "linux"))]
fn same_user(_stream: &UnixStream) -> bool {
    true
}

/// `ljos hookd`: hand out hook slots on `hookd.sock` in the runtime
/// directory until idle. A second server finds the lock taken and exits
/// 0, so starting one is safe at any time. The server also exits when its
/// socket is removed or replaced.
#[must_use]
pub fn serve() -> i32 {
    let dir = run_dir();
    let Some(lock) = open_lock(&dir.join("hookd.lock")) else {
        eprintln!(
            "ljos hookd: cannot open {}",
            dir.join("hookd.lock").display()
        );
        return 1;
    };
    if !try_flock(&lock) {
        return 0;
    }
    let sock = dir.join("hookd.sock");
    let _ = std::fs::remove_file(&sock);
    let listener = match UnixListener::bind(&sock) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("ljos hookd: cannot listen on {}: {e}", sock.display());
            return 1;
        }
    };
    let _ = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600));
    let ino = std::fs::metadata(&sock).map(|m| m.ino()).unwrap_or(0);
    let gate = Arc::new(Gate {
        turns: Mutex::new(Turns {
            running: 0,
            queue: std::collections::VecDeque::new(),
            next: 0,
            last: Instant::now(),
        }),
        moved: Condvar::new(),
        slots: slots(),
        cap: queue_cap(),
    });
    {
        let gate = Arc::clone(&gate);
        let sock = sock.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            if !std::fs::metadata(&sock).is_ok_and(|m| m.ino() == ino) {
                std::process::exit(0);
            }
            if gate.idle() {
                let _ = std::fs::remove_file(&sock);
                std::process::exit(0);
            }
        });
    }
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        if !same_user(&stream) {
            continue;
        }
        let gate = Arc::clone(&gate);
        let _ = std::thread::Builder::new()
            .name("hookd-call".into())
            .stack_size(128 * 1024)
            .spawn(move || gate.serve(&stream));
    }
    drop(lock);
    0
}

fn times_path() -> PathBuf {
    crate::runtime_dir().join("hook-times.log")
}

/// Append one timing row: `kind`, the unix time, the call's duration and
/// the time it waited on a lock, in milliseconds. `kind` is `hook` for an
/// answered call, `refused` for one the queue turned away and `book` for
/// a bookkeeping job.
pub fn note_time(kind: &str, took: Duration, waited: Duration) {
    let path = times_path();
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_CAP) {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    let Some(mut file) = open_lock(&path) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // One write per line: calls append at once, and a short O_APPEND
    // write is not split by another writer's.
    let line = format!("{kind} {now} {} {}\n", took.as_millis(), waited.as_millis());
    let _ = file.write_all(line.as_bytes());
}

/// The 95th percentile of `values`, or `None` when there are none.
fn p95(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let rank = (values.len() * 95).div_ceil(100).max(1) - 1;
    Some(values[rank])
}

/// Hook latency and lock waits from the timing log, for `ljos doctor`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Timings {
    pub calls: usize,
    pub p95_ms: Option<u64>,
    pub admit_p95_ms: Option<u64>,
    pub refused: usize,
    pub book_jobs: usize,
    pub book_lock_p95_ms: Option<u64>,
}

/// [`Timings`] from the log text.
#[must_use]
pub fn timings_of(text: &str) -> Timings {
    let mut took = Vec::new();
    let mut admit = Vec::new();
    let mut book = Vec::new();
    let mut refused = 0;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let kind = words.next();
        let _at = words.next();
        let (Some(ms), Some(wait)) = (
            words.next().and_then(|w| w.parse::<u64>().ok()),
            words.next().and_then(|w| w.parse::<u64>().ok()),
        ) else {
            continue;
        };
        match kind {
            Some("hook") => {
                took.push(ms);
                admit.push(wait);
            }
            Some("refused") => refused += 1,
            Some("book") => book.push(wait),
            _ => {}
        }
    }
    Timings {
        calls: took.len(),
        p95_ms: p95(&mut took),
        admit_p95_ms: p95(&mut admit),
        refused,
        book_jobs: book.len(),
        book_lock_p95_ms: p95(&mut book),
    }
}

/// The doctor's line for hook latency and lock waits.
#[must_use]
pub fn timings_line() -> String {
    let path = times_path();
    let mut text = std::fs::read_to_string(path.with_extension("log.1")).unwrap_or_default();
    text.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
    let t = timings_of(&text);
    let ms = |v: Option<u64>| v.map_or_else(|| "-".to_string(), |v| format!("{v} ms"));
    if t.calls == 0 && t.refused == 0 && t.book_jobs == 0 {
        return "no hook calls timed yet".to_string();
    }
    format!(
        "p95 {} over {} gate calls; queue wait p95 {}; {} refused by the queue; \
         book lock wait p95 {} over {} jobs",
        ms(t.p95_ms),
        t.calls,
        ms(t.admit_p95_ms),
        t.refused,
        ms(t.book_lock_p95_ms),
        t.book_jobs
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_hook_is_admitted() {
        let args = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert!(admitted_command(&args(&["hook"])));
        assert!(admitted_command(&args(&["hook", "--event", "PreToolUse"])));
        assert!(admitted_command(&args(&["hook", "--prompt"])));
        assert!(!admitted_command(&args(&["hook", "--git-commit"])));
        assert!(!admitted_command(&args(&["hook", "--runner=gemini"])));
        assert!(!admitted_command(&args(&["hook", "--drain"])));
        assert!(!admitted_command(&args(&["note", "x"])));
        assert!(!admitted_command(&args(&[])));
    }

    #[test]
    fn the_event_flag_reads_both_spellings() {
        let args = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert_eq!(
            flag_value(&args(&["hook", "--event", "Stop"]), "--event").as_deref(),
            Some("Stop")
        );
        assert_eq!(
            flag_value(&args(&["hook", "--event=Stop"]), "--event").as_deref(),
            Some("Stop")
        );
        assert_eq!(flag_value(&args(&["hook"]), "--event"), None);
    }

    #[test]
    fn timings_read_p95_and_waits_from_the_log() {
        let mut text = String::new();
        for ms in 1..=100 {
            text.push_str(&format!("hook 0 {ms} {}\n", ms / 10));
        }
        text.push_str("refused 0 3000 0\nbook 0 40 900\nbook 0 10 5\ngarbage\n");
        let t = timings_of(&text);
        assert_eq!(t.calls, 100);
        assert_eq!(t.p95_ms, Some(95));
        assert_eq!(t.admit_p95_ms, Some(9));
        assert_eq!(t.refused, 1);
        assert_eq!(t.book_jobs, 2);
        assert_eq!(t.book_lock_p95_ms, Some(900));
        assert_eq!(timings_of(""), Timings::default());
    }

    #[test]
    fn a_full_queue_and_a_late_turn_are_refused_in_order() {
        let gate = Arc::new(Gate {
            turns: Mutex::new(Turns {
                running: 0,
                queue: std::collections::VecDeque::new(),
                next: 0,
                last: Instant::now(),
            }),
            moved: Condvar::new(),
            slots: 1,
            cap: 1,
        });
        let pair = || UnixStream::pair().unwrap();
        let call = |gate: &Arc<Gate>, ms: u64| {
            let (mine, theirs) = pair();
            let gate = Arc::clone(gate);
            std::thread::spawn(move || gate.serve(&theirs));
            writeln!(&mut &mine, "wait {ms}").unwrap();
            let mut line = String::new();
            BufReader::new(mine.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            (mine, line)
        };
        let (first, said) = call(&gate, 1000);
        assert!(said.starts_with("go "), "{said}");
        let (mine, theirs) = pair();
        {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || gate.serve(&theirs));
        }
        writeln!(&mut &mine, "wait 2000").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let (_third, said) = call(&gate, 1000);
        assert_eq!(said.trim(), "full 1");
        drop(first);
        let mut line = String::new();
        BufReader::new(mine.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(line.starts_with("go "), "{line}");
        let (_late, said) = call(&gate, 50);
        assert_eq!(said.trim(), "late");
    }
}
