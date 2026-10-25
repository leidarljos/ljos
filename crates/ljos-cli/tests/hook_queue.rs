//! ljos-0re8: a burst of tool calls queues for the seat's hook instead of
//! stampeding it. Each call answers from its own process within a small,
//! fixed budget; the book, the claim it files and the tracker commit are
//! applied after the answer by one drainer.
#![cfg(target_os = "linux")]

use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[path = "common/seat_root.rs"]
mod seat_root;

/// Each tracker call of the shim sleeps this long, standing in for a
/// tracker large enough that reading it takes a while.
const TRACKER_READ: &str = "0.15";

const TIME: &str = "/usr/bin/time";

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
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hab.root, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&hab.run, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    for dir in [&hab.tracker, &hab.repo] {
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.name", "Rohit Goswami"]);
        git(dir, &["config", "user.email", "rgoswami@ieee.org"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
        git(dir, &["config", "core.hooksPath", "/dev/null"]);
    }
    std::fs::write(hab.repo.join("README.md"), "seed\n").unwrap();
    git(&hab.repo, &["add", "README.md"]);
    git(&hab.repo, &["commit", "-q", "-m", "seed"]);
    std::fs::write(hab.tracker.join(".keep"), "").unwrap();
    git(&hab.tracker, &["add", ".keep"]);
    git(&hab.tracker, &["commit", "-q", "-m", "seed"]);
    {
        use std::os::unix::fs::PermissionsExt;
        let shim = hab.bin.join("vissue");
        std::fs::write(&shim, VISSUE).unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
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
            let mut cmd = Command::new(program);
            cmd.args(rest).arg(env!("CARGO_BIN_EXE_ljos"));
            cmd
        }
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

fn commits(dir: &Path) -> Vec<String> {
    let out = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(dir)
        .args(["log", "--format=%s"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

fn lock_file(path: &Path) -> std::fs::File {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
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

/// Peak resident set size in kilobytes, from `/usr/bin/time -v`.
fn max_rss_kb(stderr: &str) -> u64 {
    stderr
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("Maximum resident set size (kbytes):")
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or_else(|| panic!("no rss line from {TIME} -v:\n{stderr}"))
}

/// Twenty tool calls at once, half of them the first write of a session.
/// On develop each write filed its claim inside the call, under the book
/// lock, and the calls queued behind each other for seconds.
#[test]
fn twenty_calls_at_once_each_answer_inside_200_ms_and_30_mb() {
    assert!(
        Path::new(TIME).is_file(),
        "{TIME} is needed to measure each call; install GNU time"
    );
    let hab = hab("twenty");
    let warm = within(
        feed(ljos(&hab, &["hook"]), &cursor_shell(&hab, "warm", "ls")),
        Duration::from_secs(10),
        "warm-up call",
    );
    assert!(warm.status.success());
    wait_for(&hab.run.join("ljos/hookd.sock"));
    drain(&hab);
    let calls: Vec<_> = (0..20)
        .map(|i| {
            let payload = if i % 2 == 0 {
                claude_edit(&hab, &format!("burst-{i}"))
            } else {
                cursor_shell(&hab, &format!("burst-{i}"), "ls -la")
            };
            let started = Instant::now();
            let child = feed(ljos_under(&hab, Some(&[TIME, "-v"]), &["hook"]), &payload);
            (i, started, child)
        })
        .collect();
    let mut rows = Vec::new();
    for (i, started, child) in calls {
        let out = within(child, Duration::from_secs(10), "a burst call");
        let took = started.elapsed();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(out.status.success(), "call {i}: {stdout}\n{stderr}");
        assert!(!stdout.contains("deny"), "call {i} was refused: {stdout}");
        if i % 2 == 1 {
            assert!(
                stdout.contains("\"permission\":\"allow\""),
                "call {i}: {stdout}"
            );
        }
        rows.push((i, took, max_rss_kb(&stderr)));
    }
    let report: Vec<String> = rows
        .iter()
        .map(|(i, took, kb)| format!("call {i}: {} ms, {kb} KB", took.as_millis()))
        .collect();
    eprintln!("{}", report.join("\n"));
    for (i, took, kb) in &rows {
        assert!(
            *took < Duration::from_millis(200),
            "call {i} took {took:?}:\n{}",
            report.join("\n")
        );
        assert!(*kb < 30 * 1024, "call {i} peaked at {kb} KB");
    }
    drain(&hab);
    assert_eq!(creates(&hab), 10, "every first write still files its claim");
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

/// A call made while another process holds the book lock still gets its
/// answer at once; its book waits and is applied once the lock is free.
#[test]
fn a_call_gets_its_answer_while_another_process_holds_the_book_lock() {
    let hab = hab("booklock");
    let held = lock_file(&hab.run.join("ljos/book.lock"));
    for (what, payload) in [
        ("edit", claude_edit(&hab, "locked-out")),
        ("shell", cursor_shell(&hab, "locked-out-shell", "ls")),
    ] {
        let started = Instant::now();
        let out = within(
            feed(ljos(&hab, &["hook"]), &payload),
            Duration::from_secs(5),
            what,
        );
        assert!(out.status.success(), "{what}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{what} took {:?} behind the book lock",
            started.elapsed()
        );
        if what == "shell" {
            let said = String::from_utf8_lossy(&out.stdout);
            assert!(said.contains("\"permission\":\"allow\""), "{said}");
        }
    }
    assert_eq!(creates(&hab), 0, "nothing books while the lock is held");
    drop(held);
    drain(&hab);
    assert_eq!(
        creates(&hab),
        1,
        "the queued write claimed once the lock was free"
    );
}

/// Filing a claim reads the tracker, so a queued claim is filed with the
/// book lock let go and no other bookkeeper on the host waits on it.
#[test]
fn a_queued_claim_is_filed_with_the_book_lock_free() {
    let hab = hab("claimfree");
    let out = within(
        feed(ljos(&hab, &["hook"]), &claude_edit(&hab, "claimer")),
        Duration::from_secs(5),
        "a write",
    );
    assert!(out.status.success());
    drain(&hab);
    assert_eq!(creates(&hab), 1);
    let seen = std::fs::read_to_string(hab.tracker.join("book-lock.log")).unwrap_or_default();
    assert_eq!(
        seen.trim(),
        "free",
        "the claim was filed under the book lock"
    );
}

/// A pack that accepts and never answers costs a gate one short wait, and
/// the rules kept on disk still deny. The next call does not wait at all.
#[test]
fn a_hung_pack_costs_one_short_wait_and_the_kept_rules_still_deny() {
    let hab = hab("hungpack");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming().flatten() {
            held.push(conn);
        }
    });
    let rules = hab.home.join(".local/share/ljos/rules-probe.json");
    std::fs::create_dir_all(rules.parent().unwrap()).unwrap();
    std::fs::write(
        &rules,
        r#"[["curl*","deny","the person said no curl from this seat"]]"#,
    )
    .unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let call = |conversation: &str, command: &str| {
        let mut cmd = ljos(&hab, &["hook"]);
        cmd.env("PACKSET_URL", &url)
            .env("PACKSET_WORKSPACE", "probe");
        let started = Instant::now();
        let out = within(
            feed(cmd, &cursor_shell(&hab, conversation, command)),
            Duration::from_secs(10),
            command,
        );
        (
            started.elapsed(),
            String::from_utf8_lossy(&out.stdout).to_string(),
        )
    };
    let (first, said) = call("hung-1", "ls");
    assert!(said.contains("\"permission\":\"allow\""), "{said}");
    assert!(
        first < Duration::from_millis(1000),
        "the first gate waited {first:?} on a hung pack"
    );
    let (second, said) = call("hung-2", "ls");
    assert!(said.contains("\"permission\":\"allow\""), "{said}");
    assert!(
        second < Duration::from_millis(300),
        "the next gate waited {second:?} on a pack already marked slow"
    );
    let (third, said) = call("hung-3", "curl https://example.invalid");
    assert!(said.contains("\"permission\":\"deny\""), "{said}");
    assert!(said.contains("no curl"), "{said}");
    assert!(
        third < Duration::from_millis(300),
        "a kept rule took {third:?} to deny"
    );
}

/// The server's queue is full: the gate is refused in the runner's JSON
/// with the reason, and exits 2, rather than run another hook or allow.
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
    let out = within(
        feed(ljos(&hab, &["hook"]), &cursor_shell(&hab, "full", "ls")),
        Duration::from_secs(5),
        "a call to a full queue",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stdout}\n{stderr}");
    assert!(stdout.contains("\"permission\":\"deny\""), "{stdout}");
    assert!(stdout.contains("already waiting for a slot"), "{stdout}");
    let _ = server.kill();
    let _ = server.wait();
}

/// The server dies while a call waits in its queue. The call goes to the
/// single-flight lock with what is left of its budget; when that lock
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
    let flight = lock_file(&hab.run.join("ljos/hookd.flight"));
    let mut cmd = ljos(&hab, &["hook"]);
    cmd.env("LJOS_HOOK_ADMIT_MS", "1500");
    let child = feed(cmd, &cursor_shell(&hab, "killed", "ls"));
    std::thread::sleep(Duration::from_millis(300));
    let _ = server.kill();
    let _ = server.wait();
    let out = within(child, Duration::from_secs(5), "a call whose server died");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(2), "{stdout}");
    assert!(stdout.contains("\"permission\":\"deny\""), "{stdout}");
    assert!(
        stdout.contains("single-flight lock stayed taken"),
        "{stdout}"
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

/// A read-only call makes no tracker commit, and the claims a burst of
/// writes files are committed together, once per tracker file.
#[test]
fn reads_commit_nothing_and_queued_claims_commit_once() {
    let hab = hab("batch");
    let before = commits(&hab.tracker).len();
    let out = within(
        feed(
            ljos(&hab, &["hook"]),
            &cursor_shell(&hab, "reader", "cat README.md"),
        ),
        Duration::from_secs(5),
        "a read",
    );
    assert!(out.status.success());
    drain(&hab);
    assert_eq!(
        commits(&hab.tracker).len(),
        before,
        "a read committed the tracker"
    );
    let draining = lock_file(&hab.run.join("ljos/bookq.lock"));
    for i in 0..4 {
        let out = within(
            feed(
                ljos(&hab, &["hook"]),
                &claude_edit(&hab, &format!("writer-{i}")),
            ),
            Duration::from_secs(5),
            "a write",
        );
        assert!(out.status.success());
    }
    drop(draining);
    drain(&hab);
    assert_eq!(creates(&hab), 4);
    let made = commits(&hab.tracker);
    assert_eq!(made.len(), before + 1, "{made:?}");
    for n in 1..=4 {
        assert!(
            made[0].contains(&format!("probe-{n:04x} claimed")),
            "{made:?}"
        );
    }
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

/// One drainer applies queued calls of two conversations, each as the
/// conversation its payload names: each claim is booked under its own
/// holder, though both hook processes, and so the drainer, carry a third
/// conversation's id.
#[test]
fn the_drainer_books_each_queued_call_as_its_payloads_conversation() {
    const X: &str = "0199a1b2-c3d4-4e5f-8a6b-00000000000x";
    const Y: &str = "0199a1b2-c3d4-4e5f-8a6b-00000000000y";
    const W: &str = "0199a1b2-c3d4-4e5f-8a6b-00000000000w";
    let hab = hab("perjob");
    let draining = lock_file(&hab.run.join("ljos/bookq.lock"));
    for conv in [X, Y] {
        let payload = serde_json::json!({
            "cursor_version": "2.4.0",
            "conversation_id": conv,
            "session_id": "cursor-sess-0002bbbb",
            "hook_event_name": "preToolUse",
            "tool_name": "Write",
            "tool_input": {"file_path": hab.repo.join("README.md"), "content": "grown"},
            "cwd": hab.repo,
        })
        .to_string();
        let mut cmd = ljos(&hab, &["hook"]);
        cmd.env("CURSOR_CONVERSATION_ID", W);
        let out = within(
            feed(cmd, &payload),
            Duration::from_secs(5),
            "a queued write",
        );
        assert!(out.status.success());
    }
    drop(draining);
    drain(&hab);
    assert_eq!(creates(&hab), 2);
    let book = |conv: &str| {
        let tag = ljos_cli::session_tag(conv);
        std::fs::read_to_string(hab.run.join(format!("ljos/book-{tag}.json"))).unwrap_or_default()
    };
    assert!(
        book(X).contains("\"issue\":\"probe-0001\""),
        "X's book: {}",
        book(X)
    );
    assert!(
        book(Y).contains("\"issue\":\"probe-0002\""),
        "Y's book: {}",
        book(Y)
    );
    assert!(
        book(W).is_empty(),
        "the env's conversation booked: {}",
        book(W)
    );
    let mut claims: Vec<String> = std::fs::read_to_string(hab.tracker.join("claims.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    claims.sort();
    assert_eq!(
        claims,
        [
            format!("probe-0001 probe-seat@{}", ljos_cli::session_tag(X)),
            format!("probe-0002 probe-seat@{}", ljos_cli::session_tag(Y)),
        ],
        "the holders the drainer claimed as"
    );
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
    std::fs::write(
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

/// A user-rule denial still refuses the MCP call when the server is slow.
/// A shell call that gets no slot is still refused.
#[test]
fn a_slow_server_still_refuses_a_real_denial_and_a_shell_call() {
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
        "denial took {took:?}\n{stdout}\n{stderr}"
    );
    assert_eq!(out.status.code(), Some(2), "{stdout}\n{stderr}");
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(value["permission"], "deny", "{stdout}");
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
        "the denial waited on the server"
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
    assert_eq!(out.status.code(), Some(2), "{stdout}\n{stderr}");
    assert!(stdout.contains("\"permission\":\"deny\""), "{stdout}");
    assert!(took < Duration::from_secs(3), "shell refusal took {took:?}");
    assert!(
        accepts.load(Ordering::SeqCst) >= 1,
        "the shell call did not ask the server"
    );
}
