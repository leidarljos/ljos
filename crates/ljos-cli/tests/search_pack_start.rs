//! A pack that has not finished starting is not an empty search.
//!
//! The stub answers not-ready, then ok. A search that is still starting
//! when the warm-up budget runs out ranks by words and names that.
//! A real miss, after the pack has answered, uses the ordinary miss line.
//! Words in the query, or in a hit, are not a starting signal.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const HIT: &str = "the kiln schedule is the morning plan";
const STARTING: &str =
    "Memory is starting, so try again in a few seconds. If this persists, run `ljos doctor`.";
const STARTING_WORDS: &str =
    "Memory is starting, so these results are ranked by words. If this persists, run `ljos doctor`.";
const BUSY_WORDS: &str = "The memory pack was busy, so these rows are word matches; run the search again for ranked results.";
const TRIP: &str = "pack busy try again fallback word ranking";

struct Seat {
    url: String,
    home: tempfile::TempDir,
    requests: Arc<AtomicUsize>,
    /// Bodies of `POST /v1/atoms`, in the order the stub saw them.
    posts: Arc<std::sync::Mutex<Vec<String>>>,
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let rest = line
                .strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))?;
            rest.trim().parse().ok()
        })
        .unwrap_or(0)
}

/// `closed_healths` is how many `/health` answers are `not-ready` before
/// `ok`. `None` stays not-ready. `Some(0)` is open on the first poll.
/// `hits` is the search body once the pack has opened. While it is
/// starting, search says the pack is busy loading.
/// `starting_posts` is how many `POST /v1/atoms` stay `not-ready` after
/// the pack has opened. `0` stores the first post.
fn serve(closed_healths: Option<usize>, hits: &'static str) -> Seat {
    serve_posts(closed_healths, hits, 0)
}

fn serve_posts(closed_healths: Option<usize>, hits: &'static str, starting_posts: usize) -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&requests);
    let posts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let counted_posts = Arc::clone(&posts);
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let requests = Arc::clone(&counted);
            let posts = Arc::clone(&counted_posts);
            thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let cl = content_length(&head);
                while buf.len() < head_end + cl {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let seen = if path == "/health" {
                    requests.fetch_add(1, Ordering::SeqCst)
                } else {
                    requests.load(Ordering::SeqCst)
                };
                let open = match closed_healths {
                    None => false,
                    Some(n) => seen >= n,
                };
                let (status, body) = if path == "/health" {
                    if open {
                        (200, "ok".to_string())
                    } else {
                        (200, "not-ready".to_string())
                    }
                } else if path == "/v1/search" {
                    if open {
                        (200, hits.to_string())
                    } else {
                        (
                            503,
                            r#"{"error":"the pack is busy while loading"}"#.to_string(),
                        )
                    }
                } else if path == "/v1/atoms" && head.starts_with("POST") {
                    let raw = &buf[head_end..head_end + cl.min(buf.len().saturating_sub(head_end))];
                    let posted = String::from_utf8_lossy(raw).into_owned();
                    let n = {
                        let mut seen = posts.lock().unwrap_or_else(|err| err.into_inner());
                        seen.push(posted.clone());
                        seen.len()
                    };
                    // The first posts are the pack still starting. A later
                    // post is the stored lesson. Health being open is not
                    // enough: remember does not poll `/health`.
                    if open && n > starting_posts {
                        let text = serde_json::from_str::<serde_json::Value>(&posted)
                            .ok()
                            .and_then(|value| {
                                value
                                    .get("text")
                                    .and_then(|text| text.as_str())
                                    .map(str::to_owned)
                            })
                            .unwrap_or_default();
                        (
                            200,
                            serde_json::json!({
                                "id": "atom-kept",
                                "kind": "lesson",
                                "text": text,
                            })
                            .to_string(),
                        )
                    } else {
                        (503, r#"{"error":"not-ready"}"#.to_string())
                    }
                } else if path == "/v1/status" {
                    (
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true}}"#
                            .to_string(),
                    )
                } else if open {
                    (200, r#"{"ok":true,"atoms":[]}"#.to_string())
                } else {
                    (503, r#"{"error":"not-ready"}"#.to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    Seat {
        url: format!("http://127.0.0.1:{port}"),
        home,
        requests,
        posts,
    }
}

/// How long a reap may sit in `try_wait` after `SIGKILL`.
const REAP_BOUND: Duration = Duration::from_millis(200);

fn harness_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|err| err.into_inner())
}

/// Make this process the parent of every orphaned descendant.
///
/// A setsid grandchild is reparented here when its parent exits, instead of
/// to pid 1. Other targets have no child subreaper; the rest of the harness
/// still compiles.
fn become_subreaper() {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: PR_SET_CHILD_SUBREAPER takes an integer flag and no pointers.
        let rc = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) };
        assert!(
            rc == 0,
            "child subreaper: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[derive(Clone, Copy, Debug)]
struct ProcStat {
    state: char,
    ppid: u32,
    pgrp: u32,
    sid: u32,
}

fn proc_stat(pid: u32) -> Option<ProcStat> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1.trim_start();
    let mut fields = rest.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let ppid = fields.next()?.parse().ok()?;
    let pgrp = fields.next()?.parse().ok()?;
    let sid = fields.next()?.parse().ok()?;
    Some(ProcStat {
        state,
        ppid,
        pgrp,
        sid,
    })
}

fn comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn report_pid(pid: u32) -> Option<String> {
    let stat = proc_stat(pid)?;
    Some(format!(
        "{pid} {} {} ppid {} pgrp {} sid {}",
        stat.state,
        comm(pid),
        stat.ppid,
        stat.pgrp,
        stat.sid
    ))
}

/// Direct children of `pid`, from every thread's `/proc/<pid>/task/<tid>/children`.
fn child_pids(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return out;
    };
    for task in tasks.flatten() {
        let Ok(text) = std::fs::read_to_string(task.path().join("children")) else {
            continue;
        };
        for word in text.split_whitespace() {
            let Ok(child) = word.parse::<u32>() else {
                continue;
            };
            if child > 1 && child != pid {
                out.push(child);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn descendants(root: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack = child_pids(root);
    let mut steps = 0u32;
    while let Some(pid) = stack.pop() {
        steps += 1;
        if steps > 4096 || pid <= 1 || pid == root || out.contains(&pid) {
            continue;
        }
        out.push(pid);
        stack.extend(child_pids(pid));
    }
    out
}

/// Descendants of this process that were not already children at `before`.
///
/// A host `packsetd` is not our child, so it is not in this set. A process
/// the test spawned earlier is in `before` and is not signaled.
fn owned_pids(before: &[u32]) -> Vec<u32> {
    let mut keep = Vec::new();
    for &pid in before {
        if pid <= 1 || keep.contains(&pid) {
            continue;
        }
        keep.push(pid);
        for child in descendants(pid) {
            if !keep.contains(&child) {
                keep.push(child);
            }
        }
    }
    let self_pid = std::process::id();
    descendants(self_pid)
        .into_iter()
        .filter(|pid| *pid != self_pid && !keep.contains(pid))
        .collect()
}

fn left_owned(before: &[u32], leader: Option<u32>) -> Vec<String> {
    owned_pids(before)
        .into_iter()
        .filter(|pid| Some(*pid) != leader)
        .filter_map(report_pid)
        .collect()
}

fn still_alive(pid: u32) -> bool {
    matches!(
        proc_stat(pid),
        Some(stat) if stat.state != 'Z' && stat.state != 'X'
    )
}

/// `SIGKILL` for one pid this harness may signal.
///
/// The returned error is the errno from `kill`. `ESRCH` means the pid was
/// already gone. Pid `0`, pid `-1`, and pid 1 are refused.
fn raw_signal(pid: libc::pid_t, sig: i32) -> Result<(), i32> {
    if pid <= 1 {
        return Err(libc::EINVAL);
    }
    // SAFETY: `pid` is one descendant this harness spawned, or one child this
    // test spawned. It is never `0` or `-1`.
    let rc = unsafe { libc::kill(pid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::ESRCH))
    }
}

fn signal_one(pid: u32) {
    let Ok(raw) = libc::pid_t::try_from(pid) else {
        return;
    };
    if raw <= 1 || pid == std::process::id() {
        return;
    }
    let _ = raw_signal(raw, libc::SIGKILL);
}

/// Reap one direct child without blocking. A descendant that is not yet our
/// child returns false; the caller signals it and tries again.
fn wait_pid(pid: u32) -> bool {
    let Ok(raw) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if raw <= 1 {
        return false;
    }
    let mut status = 0;
    // SAFETY: `pid` is a direct child of this process when the call succeeds.
    // `WNOHANG` does not block.
    let rc = unsafe { libc::waitpid(raw, &mut status, libc::WNOHANG) };
    rc == raw
}

fn wait_until(child: &mut Child, bound: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + bound;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() >= deadline => return None,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => return None,
        }
    }
}

/// Signal and reap descendants this run started.
///
/// This is the only teardown. The leader's `Child` owns that pid's wait, so
/// `leader` is signaled and left for the caller. Every other owned pid is
/// waited here. The walk is the children of this process, recursively, minus
/// the pids in `before`.
fn reap_owned(before: &[u32], leader: Option<u32>) {
    let deadline = Instant::now() + REAP_BOUND;
    loop {
        let owned = owned_pids(before);
        if owned.is_empty() {
            return;
        }
        for pid in &owned {
            signal_one(*pid);
            if Some(*pid) != leader {
                let _ = wait_pid(*pid);
            }
        }
        if left_owned(before, leader).is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Reap `child` after the owned descendants are signaled.
///
/// The ceiling calls this before the reader grace, so a grandchild still
/// holding a pipe cannot keep the read waiting. The wait is bounded.
fn reap_leader(child: &mut Child, before: &[u32], found: Option<ExitStatus>) -> ExitStatus {
    let leader = child.id();
    reap_owned(before, Some(leader));
    if let Some(status) = found {
        return status;
    }
    wait_until(child, REAP_BOUND)
        .unwrap_or_else(|| panic!("leader {leader} still alive after SIGKILL"))
}

/// Calls [`reap_owned`] on the way out of [`run`].
struct GroupGuard {
    before: Vec<u32>,
    leader: u32,
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        reap_owned(&self.before, Some(self.leader));
    }
}

/// Setsid grandchild the harness starts beside the client.
///
/// The parent exits at once. The grandchild's cwd is `/`, its environment
/// is empty, and the socket it holds is `socket:[inode]`. After the parent
/// is gone its ppid is this process when the subreaper is set, and pid 1
/// when it is not.
struct SetsidGrand {
    pid: u32,
}

const SETSID_GRANDCHILD: &str = r#"
import os, socket, sys, fcntl
sock_path = sys.argv[1]
pid = os.fork()
if pid == 0:
    os.setsid()
    os.chdir("/")
    sock = socket.socket(socket.AF_UNIX)
    sock.bind(sock_path)
    sock.listen(1)
    keep = sock.fileno()
    flags = fcntl.fcntl(keep, fcntl.F_GETFD)
    fcntl.fcntl(keep, fcntl.F_SETFD, flags & ~fcntl.FD_CLOEXEC)
    null = os.open("/dev/null", os.O_RDWR)
    os.dup2(null, 0)
    os.dup2(null, 1)
    os.dup2(null, 2)
    os.close(null)
    for n in range(3, 256):
        if n == keep:
            continue
        try:
            os.close(n)
        except OSError:
            pass
    os.execve("/usr/bin/sleep", ["sleep", "30"], {})
else:
    os.write(1, b"%d\n" % pid)
    os.close(1)
    os._exit(0)
"#;

impl Drop for SetsidGrand {
    fn drop(&mut self) {
        // The survivor check has already run. This only stops the sleeper
        // when that check failed, so the next test does not inherit it.
        if proc_stat(self.pid).is_none() {
            return;
        }
        let mut before = child_pids(std::process::id());
        before.retain(|cand| *cand != self.pid);
        reap_owned(&before, None);
        signal_one(self.pid);
        let deadline = Instant::now() + REAP_BOUND;
        while proc_stat(self.pid).is_some() && Instant::now() < deadline {
            let _ = wait_pid(self.pid);
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl SetsidGrand {
    fn start(home: &Path) -> Self {
        let sock = home.join("leak.sock");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new("/usr/bin/python3");
        cmd.arg("-c")
            .arg(SETSID_GRANDCHILD)
            .arg(&sock)
            .process_group(0)
            .current_dir("/")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let mut line = String::new();
        {
            let stdout = child.stdout.take().unwrap();
            let mut reader = BufReader::new(stdout);
            reader.read_line(&mut line).unwrap();
        }
        let pid = line
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("setsid pid from {line:?}"));
        assert!(
            wait_until(&mut child, Duration::from_secs(2)).is_some(),
            "setsid parent did not exit"
        );
        let self_pid = std::process::id();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let stat = proc_stat(pid);
            let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok();
            let settled = stat.is_some_and(|stat| {
                stat.state == 'S' && stat.sid == pid && (stat.ppid == 1 || stat.ppid == self_pid)
            }) && comm(pid) == "sleep"
                && cwd.as_deref() == Some(Path::new("/"));
            if settled {
                break;
            }
            if Instant::now() >= deadline {
                panic!(
                    "setsid grandchild {pid} did not settle: {}",
                    report_pid(pid).unwrap_or_else(|| "gone".to_string())
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
        Self { pid }
    }
}

fn run(seat: &Seat, args: &[&str], env: &[(&str, &str)]) -> (Output, Duration) {
    let _gate = harness_lock();
    become_subreaper();
    let home = seat.home.path();
    // Children already alive are not this run's. The setsid grandchild and
    // the client are spawned after the snapshot, so the reap can see them
    // even after they leave the client's process group.
    let before = child_pids(std::process::id());
    let grand = SetsidGrand::start(home);
    let started = Instant::now();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .current_dir(home)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home)
        .env("PACKSET_URL", &seat.url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("NO_COLOR", "1")
        .env("LJOS_SEAT", "travel")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    // `output` returns only after the client exits. A search that never
    // does is killed at the four-second ceiling and reaped, so the
    // caller can fail with the elapsed time. Stdout and stderr are
    // drained on reader threads while the child runs. A full pipe would
    // otherwise stall the write, the ceiling would kill the child, and
    // the capture would stop at the pipe's capacity. The ceiling reaps
    // before the reader grace, so a grandchild still holding a pipe
    // cannot keep the read waiting. The reap is one bounded walk of the
    // children of this process.
    let mut child = cmd.process_group(0).spawn().unwrap();
    let leader = child.id();
    let guard = GroupGuard {
        before: before.clone(),
        leader,
    };
    let stdout_rx = spawn_drain(child.stdout.take());
    let stderr_rx = spawn_drain(child.stderr.take());
    let ceiling = Duration::from_secs(4);
    let mut stdout = None;
    let mut stderr = None;
    let status = loop {
        pull_pipe(&stdout_rx, &mut stdout);
        pull_pipe(&stderr_rx, &mut stderr);
        match child.try_wait() {
            Ok(Some(status)) if stdout.is_some() && stderr.is_some() => break status,
            Ok(found) if started.elapsed() >= ceiling => {
                let status = reap_leader(&mut child, &guard.before, found);
                await_closed(&mut stdout, &mut stderr, &stdout_rx, &stderr_rx);
                break status;
            }
            Ok(Some(_) | None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => panic!("leader {leader}: {err}"),
        }
    };
    let elapsed = started.elapsed();
    let output = Output {
        status,
        stdout: stdout.unwrap_or_default(),
        stderr: stderr.unwrap_or_default(),
    };
    drop(guard);
    drop(child);
    let left = left_owned(&before, None);
    let grand_left = report_pid(grand.pid);
    assert!(
        left.is_empty() && grand_left.is_none(),
        "a process this harness started is still alive:\n{}\nsetsid grandchild {}",
        left.join("\n"),
        grand_left.as_deref().unwrap_or("gone")
    );
    (output, elapsed)
}

fn await_closed(
    stdout: &mut Option<Vec<u8>>,
    stderr: &mut Option<Vec<u8>>,
    stdout_rx: &mpsc::Receiver<Vec<u8>>,
    stderr_rx: &mpsc::Receiver<Vec<u8>>,
) {
    let grace = Instant::now() + Duration::from_millis(200);
    loop {
        pull_pipe(stdout_rx, stdout);
        pull_pipe(stderr_rx, stderr);
        if (stdout.is_some() && stderr.is_some()) || Instant::now() >= grace {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn spawn_drain<R>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>>
where
    R: Read + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

fn pull_pipe(rx: &mpsc::Receiver<Vec<u8>>, slot: &mut Option<Vec<u8>>) {
    if slot.is_some() {
        return;
    }
    match rx.try_recv() {
        Ok(buf) => *slot = Some(buf),
        Err(mpsc::TryRecvError::Empty) => {}
        Err(mpsc::TryRecvError::Disconnected) => {
            panic!("the pipe reader ended before it sent");
        }
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn hits_body() -> &'static str {
    r#"{"hits":[{"id":"mean","kind":"lesson","text":"the kiln schedule is the morning plan","score":0.91,"entities":["seat:travel"],"dense_rank":1,"cosine":0.84}],"rank":"relevance"}"#
}

#[test]
fn a_search_waits_out_not_ready_and_then_prints_the_hit() {
    let _process = ljos_test_boot::env_lock();
    let seat = serve(Some(4), hits_body());
    let (out, elapsed) = run(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(HIT), "{said}");
    assert!(!said.contains("Nothing matched"), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}\n{said}");
    let seen = seat.requests.load(Ordering::SeqCst);
    assert!(
        seen > 4,
        "the client polled {seen} times while the pack was closed"
    );
}

#[test]
fn a_real_miss_after_the_pack_answers_uses_the_miss_line() {
    let _process = ljos_test_boot::env_lock();
    let seat = serve(Some(0), r#"{"hits":[],"rank":"relevance"}"#);
    let (out, _) = run(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("Nothing matched"), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(HIT), "{said}");
}

#[test]
fn a_search_that_stays_not_ready_ranks_by_words() {
    let _process = ljos_test_boot::env_lock();
    let seat = serve(None, hits_body());
    let (out, elapsed) = run(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "1"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}\n{said}");
    assert!(out.status.success(), "{said}");
    assert!(said.contains(STARTING_WORDS), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(HIT), "{said}");
    assert!(elapsed >= Duration::from_millis(400), "{elapsed:?}");
}

#[test]
fn a_warmup_wait_of_zero_does_not_wait_on_a_starting_pack() {
    let _process = ljos_test_boot::env_lock();
    let seat = serve(None, hits_body());
    let (out, elapsed) = run(
        &seat,
        &["search", "kiln"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "0"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(STARTING_WORDS), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}\n{said}");
    let polls = seat.requests.load(Ordering::SeqCst);
    assert!(
        polls < 4,
        "a zero warmup wait polled /health {polls} times\n{said}"
    );
}

#[test]
fn remember_retries_a_starting_pack_and_stores() {
    let _process = ljos_test_boot::env_lock();
    // Health is open. The first POST is still the pack starting, because
    // remember does not poll `/health`. The call retries, and that post
    // is the stored lesson.
    let seat = serve_posts(Some(0), hits_body(), 1);
    let (out, elapsed) = run(
        &seat,
        &["remember", "Cold metal seats the door."],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains("queued"), "{said}");
    assert!(said.contains("Cold metal seats the door."), "{said}");
    assert!(said.contains("atom-kep"), "{said}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}\n{said}");
    let posts = seat.posts.lock().unwrap_or_else(|err| err.into_inner());
    assert!(
        posts.len() >= 2,
        "remember posted once, with no retry:\n{posts:?}"
    );
    assert!(
        posts
            .iter()
            .any(|body| body.contains("Cold metal seats the door.")),
        "the stored post was not the lesson:\n{posts:?}"
    );
}

#[test]
fn remember_names_a_pack_that_stays_not_ready() {
    let _process = ljos_test_boot::env_lock();
    let seat = serve(None, hits_body());
    let (out, _) = run(
        &seat,
        &["remember", "Cold metal seats the door."],
        &[("LJOS_SEARCH_WARMUP_WAIT", "1")],
    );
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(said.contains(STARTING), "{said}");
    assert!(!said.contains("queued"), "{said}");
}

#[test]
fn the_mcp_search_returns_the_starting_sentence() {
    let _process = ljos_test_boot::env_lock();
    let seat = serve(None, hits_body());
    let home = seat.home.path();
    std::env::set_var("PACKSET_URL", &seat.url);
    std::env::set_var("PACKSET_WORKSPACE", "seat");
    std::env::set_var("PACKSET_AUTH", "off");
    std::env::set_var("LJOS_NO_AUTOSTART", "1");
    std::env::set_var("LJOS_SEARCH_WARMUP_WAIT", "1");
    std::env::set_var("LJOS_SEARCH_WAIT", "0");
    std::env::set_var("HOME", home);
    std::env::set_var("LJOS_SEAT", "travel");
    let err = ljos_cli::search_ranked("kiln", 5).unwrap_err();
    let said = format!("{err:#}");
    assert!(said.contains(STARTING), "{said}");
    assert!(!said.contains("Nothing matched"), "{said}");
}

fn form_decode(raw: &str) -> String {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = &raw[i + 1..i + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn asked_query(head: &str) -> String {
    let target = head.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, rest)| rest).unwrap_or("");
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "q" {
            return form_decode(value);
        }
    }
    String::new()
}

/// `echo` puts the query, and the old trigger words, in a 200 hit.
/// `phrase` answers 503 whose error string is those words.
/// `code` answers 503 with a JSON `code` of `starting`.
#[derive(Clone, Copy)]
enum Open {
    Echo,
    Phrase,
    Code,
}

fn open_pack(kind: Open) -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let kind = kind;
            thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let cl = content_length(&head);
                while buf.len() < head_end + cl {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let (status, body) = if path == "/health" {
                    (200, "packsetd ok".to_string())
                } else if path == "/v1/status" {
                    (
                        200,
                        r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                            .to_string(),
                    )
                } else if path == "/v1/search" {
                    match kind {
                        Open::Echo => {
                            let q = asked_query(&head);
                            let text =
                                format!("{q} not ready not-ready busy loading try again starting");
                            let body = serde_json::json!({
                                "hits": [{
                                    "id": "echo",
                                    "kind": "lesson",
                                    "text": text,
                                    "score": 0.91,
                                    "entities": ["seat:travel"],
                                    "dense_rank": 1,
                                    "cosine": 0.9
                                }],
                                "rank": "relevance"
                            })
                            .to_string();
                            (200, body)
                        }
                        Open::Phrase => (
                            503,
                            format!(
                                r#"{{"error":"{TRIP} not ready not-ready busy loading try again starting"}}"#
                            ),
                        ),
                        Open::Code => (
                            503,
                            r#"{"code":"starting","ready":false,"error":"the writer is opening"}"#
                                .to_string(),
                        ),
                    }
                } else if path == "/v1/atoms" && head.starts_with("POST") {
                    (
                        200,
                        r#"{"id":"atom-kept","kind":"lesson","text":"kept"}"#.to_string(),
                    )
                } else {
                    (200, r#"{"ok":true,"atoms":[]}"#.to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    Seat {
        url: format!("http://127.0.0.1:{port}"),
        home,
        requests: Arc::new(AtomicUsize::new(0)),
        posts: Arc::new(std::sync::Mutex::new(Vec::new())),
    }
}

#[test]
fn a_query_that_names_the_busy_words_is_not_a_starting_pack() {
    let _process = ljos_test_boot::env_lock();
    let seat = open_pack(Open::Echo);
    let (out, elapsed) = run(
        &seat,
        &["search", TRIP],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(TRIP), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(STARTING_WORDS), "{said}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}\n{said}");
}

#[test]
fn a_query_that_says_starting_is_not_a_starting_pack() {
    let _process = ljos_test_boot::env_lock();
    let seat = open_pack(Open::Echo);
    let (out, elapsed) = run(
        &seat,
        &["search", "memory is starting"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("memory is starting"), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(STARTING_WORDS), "{said}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}\n{said}");
}

#[test]
fn an_error_string_that_quotes_the_query_is_busy_not_starting() {
    let _process = ljos_test_boot::env_lock();
    let seat = open_pack(Open::Phrase);
    let (out, elapsed) = run(
        &seat,
        &["search", TRIP],
        &[("LJOS_SEARCH_WARMUP_WAIT", "8"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains(
            "Memory is busy, so these results are ranked by words. If this persists, run `ljos doctor`."
        ),
        "{said}"
    );
    assert!(!said.contains(BUSY_WORDS), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains(STARTING_WORDS), "{said}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}\n{said}");
}

#[test]
fn a_starting_code_ranks_by_words() {
    let _process = ljos_test_boot::env_lock();
    let seat = open_pack(Open::Code);
    let (out, elapsed) = run(
        &seat,
        &["search", "kiln schedule"],
        &[("LJOS_SEARCH_WARMUP_WAIT", "1"), ("LJOS_SEARCH_WAIT", "0")],
    );
    let said = text(&out);
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}\n{said}");
    assert!(out.status.success(), "{said}");
    assert!(said.contains(STARTING_WORDS), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(elapsed >= Duration::from_millis(400), "{elapsed:?}");
}

#[test]
fn remember_keeps_a_lesson_that_names_the_busy_words() {
    let _process = ljos_test_boot::env_lock();
    let seat = open_pack(Open::Echo);
    let lesson = format!("{TRIP} not ready busy loading starting");
    let (out, elapsed) = run(
        &seat,
        &["remember", lesson.as_str()],
        &[("LJOS_SEARCH_WARMUP_WAIT", "5")],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(!said.contains(STARTING), "{said}");
    assert!(!said.contains("queued"), "{said}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}\n{said}");
}

/// Holds sockets until dropped, then the child is reaped through [`reap_owned`].
struct ForeignHold {
    child: Child,
}

impl Drop for ForeignHold {
    fn drop(&mut self) {
        let pid = self.child.id();
        let mut before = child_pids(std::process::id());
        before.retain(|cand| *cand != pid);
        reap_owned(&before, Some(pid));
        let _ = wait_until(&mut self.child, REAP_BOUND);
    }
}

const BIND_SOCKET: &str = r#"
import os, socket, sys, time
sock = socket.socket(socket.AF_UNIX)
sock.bind(sys.argv[1])
sock.listen(1)
print(os.getpid(), flush=True)
time.sleep(120)
"#;

const HOLD_SOCKET: &str = r#"
import os, socket, time
sock = socket.socket()
sock.bind(("127.0.0.1", 0))
sock.listen(1)
print(os.getpid(), flush=True)
time.sleep(120)
"#;

fn spawn_foreign(script: &str, arg: Option<&std::ffi::OsStr>) -> (ForeignHold, u32) {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = Command::new("/usr/bin/python3");
    cmd.arg("-c")
        .arg(script)
        .process_group(0)
        .current_dir("/")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(arg) = arg {
        cmd.arg(arg);
    }
    let mut child = cmd.spawn().unwrap();
    let mut line = String::new();
    {
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        reader.read_line(&mut line).unwrap();
    }
    let pid = line
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("pid from {line:?}"));
    (ForeignHold { child }, pid)
}

/// A process this run did not start holds a socket, and another binds a
/// socket file under the test home. Both are already children when the reap
/// snapshots them, so neither is signaled. A listening inode is not consulted.
#[test]
fn a_foreign_process_holding_a_matching_inode_is_not_signaled() {
    let _process = ljos_test_boot::env_lock();
    let _gate = harness_lock();
    become_subreaper();
    let home = tempfile::TempDir::new().unwrap();
    let sock_path = home.path().join("probe.sock");
    let (binder, binder_pid) = spawn_foreign(BIND_SOCKET, Some(sock_path.as_os_str()));
    let (canary, canary_pid) = spawn_foreign(HOLD_SOCKET, None);
    let before = child_pids(std::process::id());
    assert!(
        before.contains(&binder_pid),
        "binder {binder_pid} is not a child of this process"
    );
    assert!(
        before.contains(&canary_pid),
        "canary {canary_pid} is not a child of this process"
    );
    reap_owned(&before, None);
    assert!(
        still_alive(binder_pid),
        "listener {binder_pid} was signaled, {}",
        report_pid(binder_pid).unwrap_or_else(|| "gone".to_string())
    );
    assert!(
        still_alive(canary_pid),
        "foreign pid {canary_pid} was signaled, {}",
        report_pid(canary_pid).unwrap_or_else(|| "gone".to_string())
    );
    drop(binder);
    drop(canary);
}
