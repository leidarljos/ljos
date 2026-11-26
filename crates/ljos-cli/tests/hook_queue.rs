//! ljos-0re8: a burst of tool calls queues for the seat's hook instead of
//! stampeding it. Each call answers from its own process within a small,
//! fixed budget; the book, the claim it files and the tracker commit are
//! applied after the answer by one drainer.

#![allow(
    clippy::disallowed_macros,
    reason = "a test writes its notes to the test harness, not to a person"
)]
#![cfg(target_os = "linux")]

ljos_test_boot::harness!();
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[path = "common/pty.rs"]
mod pty;
#[path = "common/seat_root.rs"]
mod seat_root;

/// Each tracker call of the shim sleeps this long, standing in for a
/// tracker large enough that reading it takes a while.
const TRACKER_READ: &str = "0.15";

/// `RssAnon` + `RssShmem` while a burst call sleeps with stdin still open.
/// Above a hook that has only its own pages, and below an 8 MB buffer held
/// from the start of `main`.
const HOOK_ANON_AT_STDIN_KB: u64 = 4 * 1024;

/// Anonymous resident kilobytes a call may add after that read.
/// A multi-megabyte buffer kept in the call is over it.
const HOOK_ANON_GROWTH_KB: u64 = 1024;

/// Two baseline reads within this many kilobytes are the same set.
/// A short `/proc` read can disagree with the next by a page or two.
const ANON_BASELINE_SLACK_KB: u64 = 16;

/// A baseline under this has not finished loading. A hook blocked on
/// stdin holds more.
const ANON_BASELINE_FLOOR_KB: u64 = 512;

/// How long the watcher reads `/proc` without pausing after it closes
/// stdin, when the watcher and the child are on different CPUs.
/// A fill that is freed at once stays resident for less than a
/// millisecond, so a paused reader misses it.
const ANON_SPIN_MS: u64 = 3;

const ANON_DIED: &str = "exited before stdin was closed";
const ANON_NEVER: &str = "never blocked reading stdin";
const ANON_UNSETTLED: &str = "anonymous resident memory at the stdin read never settled";

const VISSUE: &str = r#"#!/usr/bin/env python3
import os, sys, time, pathlib, fcntl
root = pathlib.Path(os.environ["VISSUE_ROOT"])
args = sys.argv[1:]
with (root / "vissue.log").open("a") as fh:
    fh.write(" ".join(args) + "\n")
cmd, rest = args[0], args[1:]
time.sleep(float(os.environ.get("SHIM_TRACKER_READ", "0")))
if cmd == "create":
    lock = pathlib.Path(os.environ.get("XDG_RUNTIME_DIR", "/nonexistent")) / "ljos" / "book.lock"
    state = "absent"
    if lock.exists():
        with lock.open("a") as fh:
            try:
                fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
                fcntl.flock(fh, fcntl.LOCK_UN)
                state = "free"
            except BlockingIOError:
                state = "held"
    with (root / "book-lock.log").open("a") as fh:
        fh.write(state + "\n")
    proj = rest[rest.index("-p") + 1] if "-p" in rest else "probe"
    title = [a for a in rest if not a.startswith("-") and a != proj][-1]
    with (root / ".seq").open("a+") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        fh.seek(0)
        n = len(fh.read()) + 1
        fh.write("x")
    iid = f"{proj}-{n:04x}"
    dest = root / "Software" / proj
    dest.mkdir(parents=True, exist_ok=True)
    with (dest / "issues.org").open("a") as fh:
        fh.write(f"* TODO [#C] {title}\n:PROPERTIES:\n:ID:         {iid}\n:END:\n")
    print(iid)
elif cmd == "claim":
    with (root / "claims.log").open("a") as fh:
        fh.write(rest[0] + " " + os.environ.get("VISSUE_AGENT", "") + "\n")
    print("claimed")
elif cmd == "claims":
    print("[]")
else:
    print("{}")
"#;

/// Whether a Cursor answer is an allow that carries a rule's warning.
fn warns(said: &str) -> bool {
    said.contains("\"permission\":\"allow\"")
        && (said.contains("\"user_message\":\"warning: ")
            || said.contains("\"user_message\":\"ljos: user-rule:")
            || said.contains("\"user_message\":\"ljos: unverified-rule:"))
}

struct Hab {
    root: PathBuf,
    tracker: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    run: PathBuf,
    home: PathBuf,
}

impl Drop for Hab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(dir: &Path, args: &[&str]) {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn hab(name: &str) -> Hab {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Short: the server's socket lives under this root, and a socket path
    // has at most 103 bytes.
    let root = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp).join(format!(
        "hq-{name}-{}-{}",
        std::process::id(),
        nanos % 1_000_000_000
    ));
    let hab = Hab {
        tracker: root.join("tracker"),
        repo: root.join("repo"),
        bin: root.join("bin"),
        run: root.join("run"),
        home: root.join("home"),
        root,
    };
    for dir in [&hab.tracker, &hab.repo, &hab.bin, &hab.run, &hab.home] {
        std::fs::create_dir_all(dir).unwrap();
    }
    {
        ljos_cli::change_mode(&hab.root, 0o755).unwrap();
        ljos_cli::change_mode(&hab.run, 0o700).unwrap();
    }
    for dir in [&hab.tracker, &hab.repo] {
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.name", "Rohit Goswami"]);
        git(dir, &["config", "user.email", "rgoswami@ieee.org"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
        git(dir, &["config", "core.hooksPath", "/dev/null"]);
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(hab.repo.join("README.md"), "seed\n").unwrap();
    git(&hab.repo, &["add", "README.md"]);
    git(&hab.repo, &["commit", "-q", "-m", "seed"]);
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(hab.tracker.join(".keep"), "").unwrap();
    git(&hab.tracker, &["add", ".keep"]);
    git(&hab.tracker, &["commit", "-q", "-m", "seed"]);
    {
        let shim = hab.bin.join("vissue");
        ljos_cli::install_executable(&shim, VISSUE).unwrap();
    }
    hab
}

fn ljos(hab: &Hab, args: &[&str]) -> Command {
    ljos_under(hab, None, args)
}

/// `ljos ARGS`, run under `wrapper` (for `/usr/bin/time -v`) when given.
fn ljos_under(hab: &Hab, wrapper: Option<&[&str]>, args: &[&str]) -> Command {
    let mut cmd = match wrapper {
        Some([program, rest @ ..]) => {
            #[expect(
                clippy::disallowed_methods,
                reason = "starts a program outside git hook code"
            )]
            let mut cmd = Command::new(program);
            cmd.args(rest).arg(env!("CARGO_BIN_EXE_ljos"));
            cmd
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        _ => Command::new(env!("CARGO_BIN_EXE_ljos")),
    };
    cmd.args(args)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", hab.bin.display()))
        .env("HOME", &hab.home)
        .env("XDG_CONFIG_HOME", hab.home.join(".config"))
        .env("XDG_DATA_HOME", hab.home.join(".local/share"))
        .env("XDG_CACHE_HOME", hab.home.join(".cache"))
        .env("XDG_STATE_HOME", hab.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &hab.run)
        .env("VISSUE_ROOT", &hab.tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("LJOS_PROJECT", "probe")
        .env("LJOS_SEAT", "probe-seat")
        .env("PACKSET_URL", "off")
        .env("SHIM_TRACKER_READ", TRACKER_READ)
        .current_dir(&hab.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn feed(mut cmd: Command, payload: &str) -> std::process::Child {
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child
}

/// `RssAnon` + `RssShmem` at the stdin read, the peak from there through
/// exit, and how many readings saw the live child after stdin was closed.
struct AnonReading {
    baseline_kb: u64,
    peak_kb: u64,
    live_after_stdin: u32,
}

fn reread(file: &mut File, buf: &mut String) -> bool {
    buf.clear();
    file.seek(SeekFrom::Start(0)).is_ok() && file.read_to_string(buf).is_ok_and(|n| n > 0)
}

fn field_kb(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix(key))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Resident anonymous pages. File-backed pages are not in the sum, so the
/// executable's own code does not move it.
fn anon_resident_kb(status: &str) -> Option<u64> {
    Some(field_kb(status, "RssAnon:")?.saturating_add(field_kb(status, "RssShmem:")?))
}

fn proc_state(status: &str) -> Option<char> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("State:"))?
        .trim_start()
        .chars()
        .next()
}

/// `None` when the process is gone, a zombie, or has no anonymous fields.
fn live_anon(file: &mut File, buf: &mut String) -> Option<u64> {
    if !reread(file, buf) || matches!(proc_state(buf), Some('Z' | 'X')) {
        return None;
    }
    anon_resident_kb(buf)
}

fn note_live(peak: &mut u64, live: &mut u32, kb: Option<u64>) -> bool {
    let Some(kb) = kb else {
        return false;
    };
    *live += 1;
    *peak = (*peak).max(kb);
    true
}

/// Puts the affinity masks back to the values read before the split.
struct CpuSplit {
    child: libc::pid_t,
    child_mask: libc::cpu_set_t,
    self_mask: libc::cpu_set_t,
}

impl Drop for CpuSplit {
    fn drop(&mut self) {
        // A child that has already exited makes the kernel refuse the write.
        let _ = write_affinity(self.child, &self.child_mask);
        let _ = write_affinity(0, &self.self_mask);
    }
}

fn empty_set() -> libc::cpu_set_t {
    // SAFETY: `cpu_set_t` is a plain bitset. All zeros is an empty mask.
    unsafe { std::mem::zeroed() }
}

fn cpus_of(set: &libc::cpu_set_t) -> Vec<usize> {
    let width = usize::try_from(libc::CPU_SETSIZE).unwrap_or(0);
    let mut found = Vec::new();
    for cpu in 0..width {
        // SAFETY: `cpu` is below `CPU_SETSIZE`, so it names a bit in `set`.
        if unsafe { libc::CPU_ISSET(cpu, set) } {
            found.push(cpu);
        }
    }
    found
}

fn mask_of(cpus: &[usize]) -> libc::cpu_set_t {
    let mut set = empty_set();
    for cpu in cpus {
        // SAFETY: `cpu` was read from a mask, so it is below `CPU_SETSIZE`.
        unsafe { libc::CPU_SET(*cpu, &mut set) };
    }
    set
}

fn read_affinity(pid: libc::pid_t) -> Option<libc::cpu_set_t> {
    let mut mask = empty_set();
    let size = std::mem::size_of::<libc::cpu_set_t>();
    // SAFETY: `mask` is a writable `cpu_set_t` and `size` is its length.
    // `pid` is the hook, or 0 for this thread.
    let ok = unsafe { libc::sched_getaffinity(pid, size, &mut mask) == 0 };
    ok.then_some(mask)
}

fn write_affinity(pid: libc::pid_t, mask: &libc::cpu_set_t) -> bool {
    let size = std::mem::size_of::<libc::cpu_set_t>();
    // SAFETY: `mask` points at a filled `cpu_set_t`. `pid` is the hook, or
    // 0 for this thread. The kernel copies the mask before it returns.
    unsafe { libc::sched_setaffinity(pid, size, mask) == 0 }
}

/// Split the CPUs both masks already allow into two non-empty lists.
/// `None` when those masks share fewer than two CPUs.
fn pin_sets(child_cpus: &[usize], self_cpus: &[usize]) -> Option<(Vec<usize>, Vec<usize>)> {
    let mut shared: Vec<usize> = child_cpus
        .iter()
        .copied()
        .filter(|cpu| self_cpus.contains(cpu))
        .collect();
    if shared.len() < 2 {
        return None;
    }
    let mid = shared.len() / 2;
    let rest = shared.split_off(mid);
    Some((shared, rest))
}

fn separate_cpus(child: u32) -> Option<CpuSplit> {
    let child_pid = libc::pid_t::try_from(child).ok()?;
    let child_mask = read_affinity(child_pid)?;
    let self_mask = read_affinity(0)?;
    let (child_cpus, self_cpus) = pin_sets(&cpus_of(&child_mask), &cpus_of(&self_mask))?;
    let child_pin = mask_of(&child_cpus);
    let self_pin = mask_of(&self_cpus);
    if !write_affinity(child_pid, &child_pin) || !write_affinity(0, &self_pin) {
        let _ = write_affinity(child_pid, &child_mask);
        let _ = write_affinity(0, &self_mask);
        return None;
    }
    Some(CpuSplit {
        child: child_pid,
        child_mask,
        self_mask,
    })
}

/// Read `pid` until it sleeps with stdin open, close stdin from this
/// thread, and keep reading until the process exits.
///
/// The write follows a baseline, so this thread is already running when
/// the child is released. A child that exits first has no live reading.
/// Each reading is `RssAnon` + `RssShmem` at that instant. `/proc` keeps
/// no anonymous peak, so a fill counts only while a read still sees it.
/// The stretch with no pause is that window, and only when this thread
/// and the child are on different CPUs.
fn watch_anon(
    pid: u32,
    mut stdin: std::process::ChildStdin,
    payload: String,
) -> Result<AnonReading, &'static str> {
    let mut status = File::open(format!("/proc/{pid}/status")).map_err(|_| ANON_DIED)?;
    let mut buf = String::new();
    let started = Instant::now();
    let mut prev: Option<u64> = None;
    let mut settling = false;
    let baseline = loop {
        if started.elapsed() > Duration::from_secs(10) {
            return Err(if settling { ANON_UNSETTLED } else { ANON_NEVER });
        }
        if !reread(&mut status, &mut buf) {
            return Err(ANON_DIED);
        }
        let state = proc_state(&buf);
        if matches!(state, Some('Z' | 'X')) {
            return Err(ANON_DIED);
        }
        // Stdin is still open, so a sleep is that read. The wait-channel
        // file is not read: since Linux 6.15 the name is `anon_pipe_read`,
        // and a kernel that hides the name writes `0`.
        if state != Some('S') {
            prev = None;
            settling = false;
            // Yield, do not sleep. A sleep here is charged to the hook's
            // admission clock once the child is already in the read, and
            // under load that sleep runs long.
            std::thread::yield_now();
            continue;
        }
        settling = true;
        let Some(kb) = anon_resident_kb(&buf) else {
            prev = None;
            std::thread::yield_now();
            continue;
        };
        if kb < ANON_BASELINE_FLOOR_KB {
            prev = None;
            std::thread::yield_now();
            continue;
        }
        if let Some(earlier) = prev {
            if kb.abs_diff(earlier) <= ANON_BASELINE_SLACK_KB {
                break kb.max(earlier);
            }
            // The two reads disagreed. Give the child the CPU and try once more.
            prev = Some(kb);
            std::thread::yield_now();
        } else {
            // Second read in this same turn, so a stretched sleep cannot
            // split the pair.
            prev = Some(kb);
        }
    };
    let pin = separate_cpus(pid);
    if stdin.write_all(payload.as_bytes()).is_err() {
        return Err(ANON_DIED);
    }
    drop(stdin);
    let mut peak = baseline;
    let mut live = 0u32;
    let mut dead = false;
    if let Some(pin) = pin {
        let until = Instant::now() + Duration::from_millis(ANON_SPIN_MS);
        while Instant::now() < until {
            if !note_live(&mut peak, &mut live, live_anon(&mut status, &mut buf)) {
                dead = true;
                break;
            }
        }
        drop(pin);
    }
    if !dead {
        while note_live(&mut peak, &mut live, live_anon(&mut status, &mut buf)) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(AnonReading {
        baseline_kb: baseline,
        peak_kb: peak,
        live_after_stdin: live,
    })
}

/// The child's output, or a panic naming `what` when it runs past `limit`.
fn within(mut child: std::process::Child, limit: Duration, what: &str) -> Output {
    let started = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!(
                "{what}: no answer in {limit:?}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn cursor_shell(hab: &Hab, conversation: &str, command: &str) -> String {
    serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": conversation,
        "hook_event_name": "beforeShellExecution",
        "command": command,
        "cwd": hab.repo,
    })
    .to_string()
}

fn claude_edit(hab: &Hab, session: &str) -> String {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": session,
        "tool_name": "Edit",
        "tool_input": {
            "file_path": hab.repo.join("README.md"),
            "old_string": "seed",
            "new_string": "grown",
        },
        "cwd": hab.repo,
    })
    .to_string()
}

fn drain(hab: &Hab) {
    let out = within(
        feed(ljos(hab, &["hook", "--drain"]), ""),
        Duration::from_secs(60),
        "ljos hook --drain",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn creates(hab: &Hab) -> usize {
    std::fs::read_to_string(hab.tracker.join("vissue.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("create "))
        .count()
}

fn lock_file(path: &Path) -> std::fs::File {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    // SAFETY: flock on a descriptor `file` owns; it is let go on drop.
    assert_eq!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    file
}

fn wait_for(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Twenty tool calls at once, half of them the first write of a session.
/// Each write used to file its claim inside the call, under the book lock,
/// and the calls queued behind each other for seconds. Each call answers
/// within 200 ms once it holds a slot, and within the admission budget
/// end to end; how many run at once is the server's slot count, which
/// follows the box's cores.
///
/// Anonymous resident memory at the stdin read stays under 4 MB, and the
/// call adds under 1 MB after that. The figure is `RssAnon` plus
/// `RssShmem`. File-backed pages, including the debug executable, stay
/// out of it. A 9 MB `RssAnon` peak taken after stdin was written passed
/// a multi-megabyte buffer the hook kept, and a call that exited first
/// recorded 0.
#[test]
fn twenty_calls_at_once_each_answer_inside_200_ms_and_anon_cap() {
    let self_status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    if !self_status.contains("RssAnon:") || !self_status.contains("RssShmem:") {
        eprintln!("skipped: /proc/self/status has no RssAnon and RssShmem");
        return;
    }
    let hab = hab("twenty");
    let warm = within(
        feed(ljos(&hab, &["hook"]), &cursor_shell(&hab, "warm", "ls")),
        Duration::from_secs(10),
        "warm-up call",
    );
    assert!(warm.status.success());
    wait_for(&hab.run.join("ljos/hookd.sock"));
    drain(&hab);
    // Each watcher closes its own stdin once that call is blocked, so one
    // call is not charged for polling the other nineteen. The wait sits in
    // the admission clock, which starts before the read.
    let calls: Vec<_> = (0..20)
        .map(|i| {
            let payload = if i % 2 == 0 {
                claude_edit(&hab, &format!("burst-{i}"))
            } else {
                cursor_shell(&hab, &format!("burst-{i}"), "ls -la")
            };
            let started = Instant::now();
            let mut child = ljos(&hab, &["hook"]).spawn().unwrap();
            let pid = child.id();
            let stdin = child.stdin.take().unwrap();
            let watch = std::thread::spawn(move || watch_anon(pid, stdin, payload));
            (i, started, child, watch)
        })
        .collect();
    let mut rows = Vec::new();
    for (i, started, child, watch) in calls {
        let out = within(child, Duration::from_secs(10), "a burst call");
        let took = started.elapsed();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let reading = watch
            .join()
            .expect("anon watcher panicked")
            .unwrap_or_else(|err| panic!("call {i} {err}\nstdout:\n{stdout}\nstderr:\n{stderr}"));
        assert!(out.status.success(), "call {i}: {stdout}\n{stderr}");
        assert!(!stdout.contains("deny"), "call {i} was refused: {stdout}");
        if i % 2 == 1 {
            assert!(
                stdout.contains("\"permission\":\"allow\""),
                "call {i}: {stdout}"
            );
        }
        rows.push((i, took, reading));
    }
    let report: Vec<String> = rows
        .iter()
        .map(|(i, took, reading)| {
            format!(
                "call {i}: {} ms, anon {} KB at stdin, peak {} KB, grew {} KB, {} readings after stdin",
                took.as_millis(),
                reading.baseline_kb,
                reading.peak_kb,
                reading.peak_kb.saturating_sub(reading.baseline_kb),
                reading.live_after_stdin,
            )
        })
        .collect();
    eprintln!("{}", report.join("\n"));
    for (i, took, reading) in &rows {
        assert!(
            *took < Duration::from_millis(ljos_cli::hookd::ADMIT_MS),
            "call {i} took {took:?}, past the admission budget:\n{}",
            report.join("\n")
        );
        assert!(
            reading.live_after_stdin >= 1,
            "call {i} had no reading of the live child after stdin closed:\n{}",
            report.join("\n")
        );
        assert!(
            reading.baseline_kb < HOOK_ANON_AT_STDIN_KB,
            "call {i} held {} KB anonymous at stdin, past {HOOK_ANON_AT_STDIN_KB}:\n{}",
            reading.baseline_kb,
            report.join("\n")
        );
        let grew = reading.peak_kb.saturating_sub(reading.baseline_kb);
        assert!(
            grew < HOOK_ANON_GROWTH_KB,
            "call {i} anonymous resident memory grew {grew} KB after stdin, past {HOOK_ANON_GROWTH_KB}:\n{}",
            report.join("\n")
        );
    }
    let times = std::fs::read_to_string(hab.run.join("ljos/hook-times.log")).unwrap();
    let mut answered = 0;
    let mut work_max = 0u64;
    for line in times.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        if words.len() != 4 || words[0] != "hook" {
            continue;
        }
        let took: u64 = words[2].parse().unwrap();
        let waited: u64 = words[3].parse().unwrap();
        let work = took.saturating_sub(waited);
        assert!(
            work < 200,
            "a call answered in {work} ms past its {waited} ms slot wait:\n{times}"
        );
        work_max = work_max.max(work);
        answered += 1;
    }
    eprintln!("queue work max {work_max} ms over {answered} hook lines");
    assert!(
        answered >= 21,
        "{answered} hook timing lines for 21 calls:\n{times}"
    );
    drain(&hab);
    assert_eq!(creates(&hab), 0, "a first write does not file a claim");
    let times = std::fs::read_to_string(hab.run.join("ljos/hook-times.log")).unwrap();
    let mut gates = 0;
    for line in times.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        assert!(
            words.len() == 4
                && ["hook", "refused", "book"].contains(&words[0])
                && words[1..].iter().all(|w| w.parse::<u64>().is_ok()),
            "a torn timing line {line:?} in:\n{times}"
        );
        gates += usize::from(words[0] == "hook");
    }
    assert_eq!(gates, 21, "one timing line per gate call:\n{times}");
}

/// A pack that accepts and never answers costs a gate one short wait, and
/// the rules kept on disk still warn. The next call does not wait at all.
///
/// What is checked is how many times the pack was asked, which the
/// listener counts, so the result does not depend on how fast the machine
/// is. The only time bound is an absolute ceiling of twenty-five short
/// waits, there to catch a stall of seconds.
#[test]
fn a_hung_pack_costs_one_short_wait_and_the_kept_rules_still_warn() {
    let hab = hab("hungpack");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&accepted);
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming().flatten() {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            held.push(conn);
        }
    });
    let asked = || accepted.load(std::sync::atomic::Ordering::SeqCst);
    let rules = hab.home.join(".local/share/ljos/rules-probe.json");
    std::fs::create_dir_all(rules.parent().unwrap()).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        &rules,
        r#"[["curl*","deny","the person said no curl from this seat"]]"#,
    )
    .unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let call = |pack: &str, conversation: &str, command: &str| {
        let mut cmd = ljos(&hab, &["hook"]);
        cmd.env("PACKSET_URL", pack)
            .env("PACKSET_WORKSPACE", "probe");
        let started = Instant::now();
        let out = within(
            feed(cmd, &cursor_shell(&hab, conversation, command)),
            Duration::from_secs(30),
            command,
        );
        (
            started.elapsed(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    let (_, said, err) = call("off", "bare", "curl https://example.invalid");
    assert!(warns(&said), "{said}\n{err}");
    // One short wait is HOOK_PACK_MS. The ceiling is many of them, so a
    // loaded machine does not trip it and a stall of seconds still does.
    let pack_wait = Duration::from_millis(ljos_cli::HOOK_PACK_MS);
    let ceiling = pack_wait * 25;
    assert_eq!(asked(), 0, "a call with the pack off asked the pack");

    let (first, said, err) = call(&url, "hung-1", "ls");
    assert!(said.contains("\"permission\":\"allow\""), "{said}\n{err}");
    assert_eq!(
        asked(),
        1,
        "a hung pack was asked more than once by one gate, so it cost more than one wait\n{err}"
    );
    assert!(
        first < ceiling,
        "the first gate took {first:?} on a hung pack, past the {ceiling:?} ceiling\n{err}"
    );
    let (second, said, err) = call(&url, "hung-2", "ls");
    assert!(said.contains("\"permission\":\"allow\""), "{said}\n{err}");
    assert_eq!(asked(), 1, "a gate asked a pack already marked slow\n{err}");
    assert!(
        second < ceiling,
        "the next gate took {second:?} with the pack marked slow, past the {ceiling:?} ceiling\n{err}"
    );
    let (third, said, err) = call(&url, "hung-3", "curl https://example.invalid");
    assert!(warns(&said), "{said}\n{err}");
    assert!(said.contains("no curl"), "{said}\n{err}");
    assert_eq!(
        asked(),
        1,
        "a kept rule was judged by asking the pack\n{err}"
    );
    assert!(
        third < ceiling,
        "a kept rule took {third:?} to warn, past the {ceiling:?} ceiling\n{err}"
    );
}

/// A 2 MB read still exits 0 with one allow object. The body is not parsed.
#[test]
fn a_large_read_answers_without_waiting_on_the_body() {
    let hab = hab("bigread");
    let mut payload = String::from(
        r#"{"cursor_version":"2.4.0","conversation_id":"c-big","hook_event_name":"beforeReadFile","file_path":"/tmp/big","content":""#,
    );
    payload.push_str(&"x".repeat(2 * 1024 * 1024));
    payload.push_str(r#""}"#);
    for hookd in ["off", "on"] {
        let mut cmd = ljos(&hab, &["hook"]);
        if hookd == "off" {
            cmd.env("LJOS_HOOKD", "off");
        }
        let label = format!("2MB read LJOS_HOOKD={hookd}");
        let (took, out) = finished(feed(cmd, &payload), Duration::from_millis(1500), &label);
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(
            took < Duration::from_millis(1500),
            "{label} took {took:?}\n{stdout}\n{stderr}"
        );
        assert_eq!(out.status.code(), Some(0), "{label}: {stdout}\n{stderr}");
        assert_eq!(stdout, ALLOW, "{label}");
    }
}

/// A pack that accepts and never answers must not delay the two events
/// answered before the pack search. `LJOS_HOOKD=off` is the direct path.
/// The same calls with the queue left on answer there too.
#[test]
fn a_hung_pack_still_allows_mcp_and_read_with_the_queue_off() {
    let hab = hab("hungside");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming().flatten() {
            held.push(conn);
        }
    });
    let url = format!("http://127.0.0.1:{port}");
    let token = "t".repeat(32);
    for hookd in ["off", "on"] {
        for (what, payload) in [
            ("ljos_note", mcp_payload("ljos_note")),
            ("read", read_payload()),
        ] {
            let mut cmd = ljos(&hab, &["hook"]);
            cmd.env("PACKSET_URL", &url)
                .env("PACKSET_TOKEN", &token)
                .env("PACKSET_WORKSPACE", "probe");
            if hookd == "off" {
                cmd.env("LJOS_HOOKD", "off");
            }
            let label = format!("{what} LJOS_HOOKD={hookd}");
            let (took, out) = finished(feed(cmd, &payload), Duration::from_millis(1500), &label);
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            assert!(
                took < Duration::from_millis(1500),
                "{label} took {took:?}\n{stdout}\n{stderr}"
            );
            assert_eq!(out.status.code(), Some(0), "{label}: {stdout}\n{stderr}");
            assert_eq!(stdout, ALLOW, "{label}");
            assert!(
                stderr.trim().is_empty(),
                "{label} wrote on stderr: {stderr}"
            );
        }
    }
}

/// The server's queue is full: the gate is refused in the runner's JSON
/// with the reason, and exits 2, rather than run another hook or allow.
/// The queue fallback is one machine-output line on a narrow terminal.
#[test]
fn a_full_queue_refuses_the_gate_with_its_reason() {
    let hab = hab("full");
    let mut server = ljos(&hab, &["hookd"])
        .env("LJOS_HOOK_SLOTS", "1")
        .env("LJOS_HOOK_QUEUE", "1")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let sock = hab.run.join("ljos/hookd.sock");
    wait_for(&sock);
    let ask = |ms: u64| {
        let mut s = std::os::unix::net::UnixStream::connect(&sock).unwrap();
        writeln!(s, "wait {ms}").unwrap();
        s
    };
    let holder = ask(5000);
    let mut said = [0u8; 3];
    std::io::Read::read_exact(&mut &holder, &mut said).unwrap();
    assert_eq!(&said, b"go ");
    let _waiting = ask(5000);
    std::thread::sleep(Duration::from_millis(100));
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("COLUMNS", "20");
    let term = pty::Pty::open();
    term.set_winsize(20, 40);
    term.attach_stderr(&mut cmd);
    let mut child = term.spawn(&mut cmd);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(cursor_shell(&hab, "full", "ls").as_bytes())
        .unwrap();
    let out = term.wait_output(child, 5);
    term.finish_reading();
    let shown = term.shown().replace("\r\n", "\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{shown}");
    assert!(stdout.contains("\"permission\":\"allow\""), "{stdout}");
    let line = "ljos hook: hook queue: 1 hook calls are already waiting for a slot; the call is judged in this process";
    assert!(line.len() > 20);
    assert!(shown.lines().any(|shown| shown == line), "{shown}");
    let _ = server.kill();
    let _ = server.wait();
}

/// The server dies while a call waits in its queue. The call goes to its
/// seat's flight lock with what is left of its budget; when that lock
/// stays taken too, the gate is refused, not let through.
#[test]
fn a_server_killed_mid_wait_falls_back_and_fails_closed() {
    let hab = hab("killed");
    let mut server = ljos(&hab, &["hookd"])
        .env("LJOS_HOOK_SLOTS", "1")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let sock = hab.run.join("ljos/hookd.sock");
    wait_for(&sock);
    let holder = {
        let mut s = std::os::unix::net::UnixStream::connect(&sock).unwrap();
        writeln!(s, "wait 5000").unwrap();
        let mut said = [0u8; 3];
        std::io::Read::read_exact(&mut &s, &mut said).unwrap();
        s
    };
    let _starting = lock_file(&hab.run.join("ljos/hookd.start"));
    let flight = lock_file(&hab.run.join("ljos/hookd.flight-probe-seat"));
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOK_ADMIT_MS", "1500");
    let child = feed(cmd, &cursor_shell(&hab, "killed", "ls"));
    std::thread::sleep(Duration::from_millis(300));
    let _ = server.kill();
    let _ = server.wait();
    let out = within(child, Duration::from_secs(5), "a call whose server died");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(stdout.contains("\"permission\":\"allow\""), "{stdout}");
    assert!(
        stderr.contains("judged in this process") || stderr.contains("could not judge"),
        "{stderr}"
    );
    drop(holder);
    drop(flight);
    let out = within(
        feed(ljos(&hab, &["hook"]), &cursor_shell(&hab, "after", "ls")),
        Duration::from_secs(5),
        "a call with no server and a free lock",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"permission\":\"allow\""), "{stdout}");
}

/// Many seats wake at once and no hook server is listening. Different
/// seats proceed independently, calls for one seat queue briefly, and a
/// lock left by a process that has already exited is reclaimed. Every
/// call answers instead of treating the burst as a refusal.
#[test]
fn different_and_same_seat_calls_get_through_a_wake_burst() {
    let hab = hab("seats");
    let _starting = lock_file(&hab.run.join("ljos/hookd.start"));
    // The lock every seat used to share. Holding it must not slow another seat.
    let mut shared = lock_file(&hab.run.join("ljos/hookd.flight"));
    writeln!(shared, "{}", std::process::id()).unwrap();
    let mut stuck = lock_file(&hab.run.join("ljos/hookd.flight-stuck"));
    writeln!(stuck, "{}", std::process::id()).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut finished = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let dead = finished.id();
    finished.kill().unwrap();
    let _ = finished.wait();
    let gone = hab.run.join("ljos/hookd.flight-gone");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&gone, format!("{dead}\n")).unwrap();
    let leaked = lock_file(&gone);
    let mut seats = vec![
        "gone".to_string(),
        // This used to collapse onto the live `stuck` seat's lock.
        "s/t/u/c/k".to_string(),
    ];
    seats.extend((1..6).map(|i| format!("seat-{i}")));
    seats.extend((0..6).map(|_| "same-seat".to_string()));
    let started = Instant::now();
    let mut calls = Vec::new();
    for (i, seat) in seats.into_iter().enumerate() {
        let mut cmd = ljos(&hab, &["hook"]);
        cmd.env("LJOS_SEAT", &seat);
        cmd.env("LJOS_HOOK_ADMIT_MS", "4000");
        calls.push((
            seat,
            feed(cmd, &cursor_shell(&hab, &format!("wake-{i}"), "true")),
        ));
    }
    for (seat, child) in calls {
        let out = within(
            child,
            Duration::from_millis(1500),
            &format!("seat {seat} in a wake burst"),
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{seat} failed\n{stdout}\n{stderr}");
        assert!(
            stdout.contains("\"permission\":\"allow\""),
            "{seat}: {stdout}\n{stderr}"
        );
        assert!(
            !stderr.contains("stayed taken"),
            "{seat} waited on another seat's lock: {stderr}"
        );
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the seats queued on one lock for {:?}",
        started.elapsed()
    );
    drop(stuck);
    drop(leaked);
    drop(shared);
}

/// When a live process keeps one seat's fallback lock past the admission
/// budget, the diagnostic identifies it and gives both the immediate and
/// stale-lock recovery actions.
#[test]
fn a_live_flight_lock_refusal_names_its_holder_and_recovery() {
    use std::io::BufRead;

    let hab = hab("holder");
    let _starting = lock_file(&hab.run.join("ljos/hookd.start"));
    let path = hab.run.join("ljos/hookd.flight-held-seat");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let script = r#"
import fcntl, os, sys, time
with open(sys.argv[1], "a+") as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    lock.seek(0)
    lock.truncate()
    lock.write(str(os.getpid()) + "\n")
    lock.flush()
    print(os.getpid(), flush=True)
    time.sleep(30)
"#;
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut holder = Command::new("python3")
        .args(["-c", script])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(holder.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let pid = line.trim().parse::<u32>().unwrap();

    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_SEAT", "held-seat")
        .env("LJOS_HOOK_ADMIT_MS", "300");
    let out = within(
        feed(cmd, &cursor_shell(&hab, "held", "true")),
        Duration::from_secs(3),
        "a call behind a live holder",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("\"permission\":\"allow\""), "{stdout}");
    assert!(stderr.contains(&format!("process {pid}")), "{stderr}");
    assert!(stderr.contains("wait for it to finish"), "{stderr}");
    assert!(stderr.contains("`ljos doctor --fix`"), "{stderr}");
    let _ = holder.kill();
    let _ = holder.wait();
}

/// `ljos doctor` says how long gate calls took and how long they waited.
#[test]
fn doctor_reports_hook_p95_and_lock_waits() {
    let hab = hab("doctor");
    for i in 0..3 {
        let out = within(
            feed(
                ljos(&hab, &["hook"]),
                &cursor_shell(&hab, &format!("doc-{i}"), "ls"),
            ),
            Duration::from_secs(5),
            "a call",
        );
        assert!(out.status.success());
    }
    drain(&hab);
    let out = within(
        feed(ljos(&hab, &["doctor", "--all"]), ""),
        Duration::from_secs(60),
        "ljos doctor",
    );
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let line = said
        .lines()
        .find(|l| l.contains("hook latency"))
        .unwrap_or_else(|| panic!("no hook latency line:\n{said}"));
    assert!(line.contains("over 3 gate calls"), "{line}");
    assert!(line.contains("queue wait p95"), "{line}");
    assert!(line.contains("book lock wait p95"), "{line}");
}

/// A seat-tool shortcut must not trust only the tool name prefix.
/// Another server can mint `mcp__SERVER__ljos_NAME` too.
#[test]
fn a_spoofed_mcp_seat_tool_name_from_github_is_not_auto_allowed() {
    let hab = hab("spoof");
    let cfg = hab.home.join(".config/ljos");
    std::fs::create_dir_all(&cfg).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(cfg.join("user-rules.json"), "{bad json").unwrap();
    let payload = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "spoof-1",
        "hook_event_name": "PreToolUse",
        "tool_name": "mcp__github__ljos_note",
        "tool_input": {"body": "cc @ocaisa"},
        "session_id": "s-1",
        "tool_use_id": "t-1"
    })
    .to_string();
    let out = within(
        feed(ljos(&hab, &["hook"]), &payload),
        Duration::from_secs(5),
        "spoofed seat tool",
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        warns(&said),
        "a spoofed seat tool name skipped the rules: {said}"
    );
}

/// A fast allow on `beforeReadFile` must not break a writer with SIGPIPE.
#[test]
fn a_large_cursor_read_does_not_break_the_writer_pipe() {
    let hab = hab("sigpipe");
    let shell = format!(
        concat!(
            "python3 - <<'PY' | {bin} hook >/dev/null\n",
            "import sys\n",
            "sys.stdout.write('{{\\\"cursor_version\\\":\\\"2.4.0\\\",\\\"hook_event_name\\\":\\\"beforeReadFile\\\",\\\"file_path\\\":\\\"/tmp/big\\\",\\\"content\\\":\\\"')\n",
            "sys.stdout.write('a'*(8*1024*1024))\n",
            "sys.stdout.write('\\\\\"}}')\n",
            "PY\n",
            "echo writer:${{PIPESTATUS[0]}} hook:${{PIPESTATUS[1]}}\n",
        ),
        bin = env!("CARGO_BIN_EXE_ljos")
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new("bash")
        .arg("-lc")
        .arg(shell)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &hab.home)
        .env("XDG_CONFIG_HOME", hab.home.join(".config"))
        .env("XDG_RUNTIME_DIR", &hab.run)
        .env("PACKSET_URL", "off")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("writer:0"), "writer was broken: {said}");
}

const ALLOW: &str = "{\"permission\":\"allow\"}\n";

/// A server that accepts and then waits `delay` before it answers.
/// The count is how many connections arrived.
fn delaying_server(sock: &Path, delay: Duration) -> Arc<AtomicUsize> {
    let _ = std::fs::remove_file(sock);
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(sock).unwrap();
    let accepts = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&accepts);
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            count.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let mut conn = conn;
                let _ = writeln!(conn, "late");
                std::thread::sleep(Duration::from_secs(30));
            });
        }
    });
    accepts
}

fn mcp_payload(tool: &str) -> String {
    serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": format!("c-{tool}"),
        "hook_event_name": "beforeMCPExecution",
        "tool_name": tool,
        "tool_input": "{\"issue\":\"probe-1\",\"text\":\"note from the seat\"}",
        "mcp_server_name": "ljos",
        "command": "ljos-mcp",
    })
    .to_string()
}

fn read_payload() -> String {
    serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-read",
        "hook_event_name": "beforeReadFile",
        "file_path": "/tmp/readme",
        "content": "seed",
    })
    .to_string()
}

fn github_comment_payload() -> String {
    serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-gh",
        "hook_event_name": "beforeMCPExecution",
        "tool_name": "add_issue_comment",
        "mcp_server_name": "github",
        "tool_input": {"body": "please ask @ocaisa", "owner": "org", "repo": "repo"},
    })
    .to_string()
}

fn write_mention_rule(hab: &Hab) {
    let path = hab.home.join(".config/ljos/user-rules.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    #[expect(clippy::disallowed_methods, reason = "writes a file that is not a git hook")] std::fs::write(
        &path,
        r#"{"rules":[{"id":"ur-ocaisa","text":"never post or reply to Alan (ocaisa)","author":"rohit","reason":"he asked not to be pinged","deny_mention":{"handles":["ocaisa"],"names":["Alan"]},"deny_reply_to":{"handles":["ocaisa"],"names":["Alan"]},"written_at":"2026-10-25T00:00:00Z","updated_at":"2026-10-25T00:00:00Z","basis":"person"}]}"#,
    )
    .unwrap();
}

/// Wait until the child exits, or panic once `limit` has passed.
fn finished(mut child: std::process::Child, limit: Duration, what: &str) -> (Duration, Output) {
    let started = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            let took = started.elapsed();
            return (took, child.wait_with_output().unwrap());
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!(
                "{what}: still running after {limit:?}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn assert_one_allow(out: &Output, took: Duration, accepts: &AtomicUsize, what: &str) {
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        took < Duration::from_millis(1500),
        "{what} took {took:?}\n{stdout}\n{stderr}"
    );
    assert_eq!(out.status.code(), Some(0), "{what}: {stdout}\n{stderr}");
    assert_eq!(stdout, ALLOW, "{what}");
    assert!(stderr.trim().is_empty(), "{what} wrote on stderr: {stderr}");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(value["permission"], "allow", "{what}");
    assert_eq!(
        accepts.load(Ordering::SeqCst),
        0,
        "{what} talked to the server"
    );
}

/// A slow server used to print allow and exit 2. The call now answers
/// before it talks to the server.
#[test]
fn a_slow_server_still_allows_mcp_and_read_calls() {
    let hab = hab("slowmcp");
    let sock = hab.run.join("ljos/hookd.sock");
    let accepts = delaying_server(&sock, Duration::from_secs(8));
    std::thread::sleep(Duration::from_millis(20));
    for tool in ["ljos_note", "ljos_vote"] {
        let mut cmd = ljos(&hab, &["hook"]);
        cmd.env("LJOS_HOOK_ADMIT_MS", "4000");
        let (took, out) = finished(feed(cmd, &mcp_payload(tool)), Duration::from_secs(6), tool);
        assert_one_allow(&out, took, &accepts, tool);
    }
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOK_ADMIT_MS", "4000");
    let (took, out) = finished(feed(cmd, &read_payload()), Duration::from_secs(6), "read");
    assert_one_allow(&out, took, &accepts, "read");
}

/// The server stays silent past the runner's hook budget. The call
/// answers anyway, and does not load a seat to do it.
#[test]
fn a_silent_server_does_not_outrun_the_hook_budget() {
    let hab = hab("loadmcp");
    let sock = hab.run.join("ljos/hookd.sock");
    let accepts = delaying_server(&sock, Duration::from_secs(30));
    std::thread::sleep(Duration::from_millis(20));
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOK_ADMIT_MS", "20000");
    let (took, out) = finished(
        feed(cmd, &mcp_payload("ljos_note")),
        Duration::from_secs(2),
        "a call whose server stays silent",
    );
    assert_one_allow(&out, took, &accepts, "silent server");
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOK_ADMIT_MS", "20000");
    let (took, out) = finished(
        feed(cmd, &mcp_payload("ljos_vote")),
        Duration::from_secs(2),
        "a second call whose server stays silent",
    );
    assert_one_allow(&out, took, &accepts, "silent server vote");
}

/// A user rule still warns on the MCP call when the server is slow. The
/// allow is one JSON object and exits 0, so the runner shows the warning.
/// A shell call that gets no slot is allowed.
#[test]
fn a_slow_server_still_warns_on_a_user_rule_and_allows_a_shell_call() {
    let hab = hab("denymcp");
    write_mention_rule(&hab);
    let sock = hab.run.join("ljos/hookd.sock");
    let accepts = delaying_server(&sock, Duration::from_secs(8));
    std::thread::sleep(Duration::from_millis(20));
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOK_ADMIT_MS", "4000");
    let (took, out) = finished(
        feed(cmd, &github_comment_payload()),
        Duration::from_secs(6),
        "a mentioned comment",
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        took < Duration::from_millis(1500),
        "the warning took {took:?}\n{stdout}\n{stderr}"
    );
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(warns(&stdout), "{stdout}");
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert!(
        value["user_message"]
            .as_str()
            .unwrap_or("")
            .contains("ocaisa")
            || value["user_message"]
                .as_str()
                .unwrap_or("")
                .contains("Alan"),
        "{stdout}"
    );
    assert_eq!(stdout.lines().filter(|l| !l.trim().is_empty()).count(), 1);
    assert_eq!(
        accepts.load(Ordering::SeqCst),
        0,
        "the warning waited on the server"
    );

    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOK_ADMIT_MS", "800");
    let (took, out) = finished(
        feed(cmd, &cursor_shell(&hab, "shell", "ls")),
        Duration::from_secs(5),
        "a shell call with no slot",
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(stdout.contains("\"permission\":\"allow\""), "{stdout}");
    assert!(
        stderr.contains("did not answer") || stderr.contains("judged in this process"),
        "{stderr}"
    );
    assert!(took < Duration::from_secs(3), "shell answer took {took:?}");
    assert!(
        accepts.load(Ordering::SeqCst) >= 1,
        "the shell call did not ask the server"
    );
}

/// A rules file that cannot be parsed warns on the GitHub write and
/// allows a seat tool. A FIFO must not stall either call.
#[test]
fn a_bad_rules_file_warns_on_the_github_write_from_the_hook() {
    let hab = hab("badrules");
    let path = hab.home.join(".config/ljos/user-rules.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&path, "{\"rules\":[").unwrap();
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let (took, out) = finished(
        feed(cmd, &github_comment_payload()),
        Duration::from_millis(1500),
        "malformed rules",
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        took < Duration::from_millis(1500),
        "{took:?}\n{stdout}\n{stderr}"
    );
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(warns(&stdout), "{stdout}");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        value["user_message"]
            .as_str()
            .unwrap_or("")
            .contains("user-rules.json"),
        "{stdout}"
    );
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let (took, out) = finished(
        feed(cmd, &mcp_payload("ljos_note")),
        Duration::from_millis(1500),
        "note beside a bad rules file",
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), ALLOW);
    assert!(took < Duration::from_millis(1500));

    std::fs::remove_file(&path).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let made = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap()
        .success();
    assert!(made);
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let (took, out) = finished(
        feed(cmd, &github_comment_payload()),
        Duration::from_millis(1500),
        "fifo rules",
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        took < Duration::from_millis(1500),
        "fifo took {took:?}\n{stdout}"
    );
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(warns(&stdout), "{stdout}");
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let (took, out) = finished(
        feed(cmd, &read_payload()),
        Duration::from_millis(1500),
        "fifo read",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), ALLOW);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        took < Duration::from_millis(1500),
        "fifo read took {took:?}"
    );
}

/// the editor harness names a seat tool `mcp__SERVER__ljos_NAME` on preToolUse.
/// An empty answer blocks under a hand-set failClosed, so the hook prints allow.
#[test]
fn a_cursor_seat_tool_with_an_mcp_prefix_prints_allow() {
    let hab = hab("mcpname");
    for tool in ["mcp__ljos__ljos_note", "mcp__plugin_ljos_ljos__ljos_vote"] {
        let mut cmd = ljos(&hab, &["hook"]);
        cmd.env("LJOS_HOOKD", "off");
        let payload = serde_json::json!({
            "cursor_version": "2.4.0",
            "conversation_id": "c-mcpname",
            "hook_event_name": "preToolUse",
            "tool_name": tool,
            "tool_input": {"text": "x"},
            "cwd": hab.repo,
            "tool_use_id": "u-mcp",
        })
        .to_string();
        let (took, out) = finished(feed(cmd, &payload), Duration::from_secs(2), tool);
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        assert_eq!(out.status.code(), Some(0), "{tool}: {stdout}\n{stderr}");
        assert_eq!(stdout, ALLOW, "{tool}");
        assert!(
            took < Duration::from_millis(1500),
            "{tool} took {took:?}\n{stderr}"
        );
    }
}

/// The two early answers show up in the trace when the marker is present,
/// and nowhere when it is absent.
#[test]
fn mcp_and_read_answers_are_traced_when_the_marker_is_present() {
    let hab = hab("trace");
    let log = hab.run.join("ljos/hook-trace.jsonl");
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let (_took, out) = finished(
        feed(cmd, &mcp_payload("ljos_note")),
        Duration::from_secs(2),
        "untraced",
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!log.exists(), "a trace row was written with no marker");
    let dir = hab.run.join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(dir.join("hook-trace"), "").unwrap();
    for hookd in ["off", "on"] {
        for (what, payload) in [
            ("beforeMCPExecution", mcp_payload("ljos_note")),
            ("beforeReadFile", read_payload()),
        ] {
            let mut cmd = ljos(&hab, &["hook"]);
            if hookd == "off" {
                cmd.env("LJOS_HOOKD", "off");
            }
            let label = format!("{what} {hookd}");
            let (took, out) = finished(feed(cmd, &payload), Duration::from_secs(2), &label);
            assert_eq!(
                out.status.code(),
                Some(0),
                "{label}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(took < Duration::from_millis(1500), "{label} took {took:?}");
        }
    }
    let text = std::fs::read_to_string(&log).unwrap_or_else(|err| panic!("{err}"));
    for event in ["beforeMCPExecution", "beforeReadFile"] {
        let rows = text
            .lines()
            .filter(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .is_some_and(|value| value["event"] == event)
            })
            .count();
        assert!(rows >= 2, "{event} rows {rows} in {text}");
    }
}

fn bad_rules(hab: &Hab) {
    let cfg = hab.home.join(".config/ljos");
    std::fs::create_dir_all(&cfg).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(cfg.join("user-rules.json"), "{bad json").unwrap();
}

fn write_cursor_mcp(hab: &Hab, body: &str) {
    let path = hab.home.join(".cursor/mcp.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(path, body).unwrap();
}

fn pre_tool(tool: &str, server: Option<&str>) -> String {
    let mut value = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-seat",
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": {"body": "cc @ocaisa"},
        "session_id": "s-seat",
        "tool_use_id": "t-seat",
    });
    if let Some(server) = server {
        value["server"] = serde_json::json!(server);
    }
    value.to_string()
}

fn hook_stdout(hab: &Hab, payload: &str) -> String {
    let mut cmd = ljos(hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let (took, out) = finished(feed(cmd, payload), Duration::from_secs(5), "seat tool");
    assert!(
        took < Duration::from_secs(5),
        "took {took:?}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The configured server key is allowed. A different key, a different
/// case, and a server field that disagrees are not.
#[test]
fn only_the_configured_seat_server_skips_the_rules_file() {
    let hab = hab("seatkey");
    bad_rules(&hab);
    write_cursor_mcp(
        &hab,
        r#"{"mcpServers":{"ljos":{"command":"ljos-mcp"},"my__seat":{"command":"/bin/ljos-mcp"}}}"#,
    );
    let allow = hook_stdout(&hab, &pre_tool("mcp__ljos__ljos_note", None));
    assert_eq!(allow, ALLOW, "{allow}");
    let renamed = hook_stdout(&hab, &pre_tool("mcp__my__seat__ljos_note", None));
    assert_eq!(renamed, ALLOW, "{renamed}");
    let spoof = hook_stdout(&hab, &pre_tool("mcp__github__ljos_note", None));
    assert!(warns(&spoof), "{spoof}");
    let case = hook_stdout(&hab, &pre_tool("mcp__Ljos__ljos_note", None));
    assert!(
        !warns(&case),
        "a case mismatch was treated as GitHub: {case}"
    );
    assert!(case.contains("\"permission\":\"allow\""), "{case}");
    let disagree = hook_stdout(&hab, &pre_tool("mcp__ljos__ljos_note", Some("github")));
    assert!(warns(&disagree), "{disagree}");
    let plugin = hook_stdout(&hab, &pre_tool("mcp__plugin_ljos_ljos__ljos_vote", None));
    assert!(plugin.contains("\"permission\":\"allow\""), "{plugin}");
}

/// A server key that is itself `github` and launches the seat is the seat.
#[test]
fn a_seat_server_named_github_is_not_a_foreign_write() {
    let hab = hab("seatgh");
    bad_rules(&hab);
    write_cursor_mcp(
        &hab,
        r#"{"mcpServers":{"github":{"command":"ljos-mcp","args":[]}}}"#,
    );
    let said = hook_stdout(&hab, &pre_tool("mcp__github__ljos_note", Some("github")));
    assert_eq!(said, ALLOW, "{said}");
    let real = hook_stdout(
        &hab,
        &serde_json::json!({
            "cursor_version": "2.4.0",
            "conversation_id": "c-real",
            "hook_event_name": "PreToolUse",
            "tool_name": "mcp__github__add_issue_comment",
            "mcp_server_name": "github",
            "tool_input": {"body": "hello"},
            "session_id": "s-real",
            "tool_use_id": "t-real",
        })
        .to_string(),
    );
    assert!(warns(&real), "{real}");
}

/// `beforeMCPExecution` names the server in a field. A GitHub field on a
/// seat-shaped tool is still a GitHub write when the rules file cannot
/// be read. The seat's own server is not.
#[test]
fn a_before_mcp_server_field_is_not_taken_from_the_tool_name() {
    let hab = hab("mcpfield");
    bad_rules(&hab);
    let github = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-field",
        "hook_event_name": "beforeMCPExecution",
        "tool_name": "ljos_note",
        "server": "github",
        "tool_input": {"text": "x"},
    })
    .to_string();
    let said = hook_stdout(&hab, &github);
    assert!(warns(&said), "{said}");
    let seat = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-field",
        "hook_event_name": "beforeMCPExecution",
        "tool_name": "ljos_note",
        "server": "ljos",
        "command": "ljos-mcp",
        "tool_input": {"text": "x"},
    })
    .to_string();
    assert_eq!(hook_stdout(&hab, &seat), ALLOW);
}

/// A watch for the close that follows a read of `path`.
fn watch_rules_read(path: &Path) -> i32 {
    // SAFETY: a new inotify descriptor, or -1. Nothing else is touched.
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    assert!(fd >= 0, "inotify: {}", std::io::Error::last_os_error());
    let mut name = path.as_os_str().as_bytes().to_vec();
    name.push(0);
    // SAFETY: `name` is NUL-terminated and lives until the call returns.
    let wd = unsafe { libc::inotify_add_watch(fd, name.as_ptr().cast(), libc::IN_CLOSE_NOWRITE) };
    assert!(
        wd >= 0,
        "inotify watch: {}",
        std::io::Error::last_os_error()
    );
    fd
}

/// Block until the watch from [`watch_rules_read`] sees a read close.
/// Five seconds with no close is a hang, and the test says so.
fn wait_rules_read(fd: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut buf = [0u8; 4096];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(
            !left.is_zero(),
            "the hook did not close the rules file after reading it"
        );
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = i32::try_from(left.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
        // SAFETY: one pollfd for the inotify descriptor this test owns.
        let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
        if rc == 0 {
            continue;
        }
        assert!(rc > 0, "inotify poll: {}", std::io::Error::last_os_error());
        // SAFETY: `buf` is ours and `fd` is the inotify descriptor.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            break;
        }
        let err = std::io::Error::last_os_error();
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::Interrupted,
            "inotify read: {err}"
        );
    }
    // SAFETY: `fd` is the descriptor this test opened.
    unsafe { libc::close(fd) };
}

/// Truncate-then-write of user-rules must not deny covered GitHub writes.
/// A rewrite window is not a policy decision. The hook is restored once
/// it has read the empty file, so the window is the state it saw.
#[test]
fn a_rules_rewrite_window_does_not_deny_github_writes() {
    let hab = hab("rules-race");
    let cfg = hab.home.join(".config/ljos");
    std::fs::create_dir_all(&cfg).unwrap();
    let path = cfg.join("user-rules.json");
    let rules = "{\"rules\":[]}\n";
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&path, rules).unwrap();
    let watch = watch_rules_read(&path);
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&path, "").unwrap();

    let payload = github_comment_payload();
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let child = feed(cmd, &payload);
    wait_rules_read(watch);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"",
        "the hook did not read the empty rules file"
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&path, rules).unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"permission\":\"allow\""),
        "rewrite window denied the write: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// User rules that target MCP:<tool> names must fire on preToolUse.
#[test]
fn a_pretooluse_mcp_tool_name_reaches_user_rules() {
    let hab = hab("mcp-name-rule");
    let cfg = hab.home.join(".config/ljos");
    std::fs::create_dir_all(&cfg).unwrap();
    #[expect(clippy::disallowed_methods, reason = "writes a file that is not a git hook")] std::fs::write(
        cfg.join("user-rules.json"),
        r#"{"rules":[{"id":"ur-1","text":"deny mcp tool","author":"t","reason":"r","pattern":"MCP:add_issue_comment*","verdict":"deny","deny_mention":{"handles":[],"names":[]},"deny_reply_to":{"handles":[],"names":[]},"written_at":"2026-10-26T00:00:00Z","updated_at":"2026-10-26T00:00:00Z"}]}"#,
    )
    .unwrap();

    let payload = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-mcp-name",
        "hook_event_name": "preToolUse",
        "tool_name": "MCP:add_issue_comment",
        "tool_input": {"owner":"o","repo":"r","issue_number":1,"body":"hello"}
    })
    .to_string();

    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let (_took, out) = finished(feed(cmd, &payload), Duration::from_secs(3), "mcp-name");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(warns(&stdout), "{stdout}");
}

/// A `command` key in the tool input is an argument of the tool. It does
/// not replace `MCP:<tool>` in the cue the user rule matches.
#[test]
fn a_command_in_tool_input_does_not_replace_the_mcp_tool_name() {
    let hab = hab("mcp-command");
    let cfg = hab.home.join(".config/ljos");
    std::fs::create_dir_all(&cfg).unwrap();
    #[expect(clippy::disallowed_methods, reason = "writes a file that is not a git hook")] std::fs::write(
        cfg.join("user-rules.json"),
        r#"{"rules":[{"id":"ur-1","text":"deny mcp tool","author":"t","reason":"r","pattern":"MCP:add_issue_comment*","verdict":"deny","deny_mention":{"handles":[],"names":[]},"deny_reply_to":{"handles":[],"names":[]},"written_at":"2026-10-26T00:00:00Z","updated_at":"2026-10-26T00:00:00Z"}]}"#,
    )
    .unwrap();

    let payload = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-mcp-cmd",
        "hook_event_name": "preToolUse",
        "tool_name": "MCP:add_issue_comment",
        "tool_input": {"command": "echo hi", "owner":"o","repo":"r","issue_number":1,"body":"hello"}
    })
    .to_string();

    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOKD", "off");
    let (_took, out) = finished(feed(cmd, &payload), Duration::from_secs(3), "mcp-cmd");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        warns(&stdout),
        "tool_input.command replaced the MCP cue: {stdout}"
    );
}

/// Non-permission events should not wait seconds on a hung memory backend.
#[test]
fn ordinary_non_permission_events_do_not_stall_on_hung_pack() {
    let hab = hab("hung-non-gate");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming().flatten() {
            held.push(conn);
        }
    });
    let url = format!("http://127.0.0.1:{port}");
    let events = [
        (
            "sessionStart",
            r#"{"cursor_version":"2.4.0","conversation_id":"c","hook_event_name":"sessionStart"}"#,
        ),
        (
            "stop",
            r#"{"cursor_version":"2.4.0","conversation_id":"c","hook_event_name":"stop","status":"completed","loop_count":0}"#,
        ),
        (
            "beforeSubmitPrompt",
            r#"{"cursor_version":"2.4.0","conversation_id":"c","hook_event_name":"beforeSubmitPrompt","prompt":"hello"}"#,
        ),
        (
            "postToolUseFailure",
            r#"{"cursor_version":"2.4.0","conversation_id":"c","hook_event_name":"postToolUseFailure","tool_name":"Shell","tool_input":{"command":"ls"},"error_message":"exit 2"}"#,
        ),
    ];
    for (name, payload) in events {
        let mut cmd = ljos(&hab, &["hook"]);
        cmd.env("PACKSET_URL", &url)
            .env("PACKSET_WORKSPACE", "probe")
            .env("LJOS_HOOKD", "off");
        let (took, _out) = finished(feed(cmd, payload), Duration::from_secs(10), name);
        assert!(took < Duration::from_millis(500), "{name} took {took:?}");
    }
}
