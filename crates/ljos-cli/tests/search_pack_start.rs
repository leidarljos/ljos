//! A pack that has not finished starting is not an empty search.
//!
//! The stub answers not-ready, then ok. A search that is still starting
//! when the warm-up budget runs out ranks by words and names that.
//! A real miss, after the pack has answered, uses the ordinary miss line.
//! Words in the query, or in a hit, are not a starting signal.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
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

/// Pids this harness spawned, and process groups it created.
///
/// A signal goes only to one of these pids, or to a process in one of
/// these groups. A descendant is recorded with its group, and that whole
/// group is killed, so an embed is not left behind.
struct Spawned {
    pids: Vec<u32>,
    groups: Vec<u32>,
}

impl Spawned {
    fn leader(pid: u32) -> Self {
        Self {
            pids: vec![pid],
            groups: vec![pid],
        }
    }

    fn none() -> Self {
        Self {
            pids: Vec::new(),
            groups: Vec::new(),
        }
    }

    fn owns(&self, pid: u32) -> bool {
        if self.pids.contains(&pid) {
            return true;
        }
        match pgrp(pid) {
            Some(group) => self.groups.contains(&group) || self.pids.contains(&group),
            None => false,
        }
    }

    fn adopt(&mut self, pid: u32) {
        if pid <= 1 || pid == std::process::id() {
            return;
        }
        if !self.pids.contains(&pid) {
            self.pids.push(pid);
        }
        if let Some(group) = pgrp(pid) {
            if group > 1 && !self.groups.contains(&group) {
                self.groups.push(group);
            }
        }
    }

    fn adopt_tree(&mut self) {
        for pid in proc_pids() {
            if self.owns(pid) || descends_from(pid, self) {
                self.adopt(pid);
            }
        }
    }
}

/// Kills the group this harness created on the way out of [`run`], then
/// stops a recorded pid only when it spawned that pid or created its group.
struct GroupGuard {
    group: libc::pid_t,
    spawned: Spawned,
    home: PathBuf,
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        // Every return drops this guard: a clean exit, pipes that have
        // already closed, and the ceiling. A second signal covers a path
        // that has not signaled yet. The result is not a reason to wait.
        // [`reap_leader`] checks it when the leader is still running.
        // The group is signaled even when the leader pid is already gone,
        // so an embed still in that group is not left behind.
        let _ = signal_group(self.group);
        stop_recorded(&self.home, &mut self.spawned);
    }
}

fn run(seat: &Seat, args: &[&str], env: &[(&str, &str)]) -> (Output, Duration) {
    let home = seat.home.path();
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
    // the capture would stop at the pipe's capacity. `GroupGuard` kills
    // the group on the way out, including a clean exit and pipes that
    // have already closed. The ceiling kills before the reader grace, so
    // a grandchild still holding a pipe cannot keep the read waiting.
    // The kill result is checked, and the reap is a bounded `try_wait`.
    let mut child = cmd.process_group(0).spawn().unwrap();
    let group = libc::pid_t::try_from(child.id()).unwrap();
    let guard = GroupGuard {
        group,
        spawned: Spawned::leader(child.id()),
        home: home.to_path_buf(),
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
                let status = reap_leader(&mut child, group, found);
                await_closed(&mut stdout, &mut stderr, &stdout_rx, &stderr_rx);
                break status;
            }
            Ok(Some(_) | None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => panic!("leader {group}: {err}"),
        }
    };
    let elapsed = started.elapsed();
    let output = Output {
        status,
        stdout: stdout.unwrap_or_default(),
        stderr: stderr.unwrap_or_default(),
    };
    drop(child);
    drop(guard);
    let left = alive_under(home);
    assert!(
        left.is_empty(),
        "a process started under {} is still alive:\n{}",
        home.display(),
        left.join("\n")
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

/// `SIGKILL` for `pid`. A negative pid is a process group.
///
/// The returned error is the errno from `kill`. `ESRCH` means the pid
/// was already gone.
fn raw_signal(pid: libc::pid_t, sig: i32) -> Result<(), i32> {
    // `kill(-1)` signals every process this user may signal, and
    // `kill(0)` signals this process's own group.
    if pid == 0 || pid == -1 {
        return Err(libc::EINVAL);
    }
    // SAFETY: `pid` is the leader this test spawned, that leader's
    // process group, or a daemon pid read from the test home.
    let rc = unsafe { libc::kill(pid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::ESRCH))
    }
}

fn signal_group(group: libc::pid_t) -> Result<(), i32> {
    if group <= 1 {
        return Err(libc::EINVAL);
    }
    raw_signal(-group, libc::SIGKILL)
}

/// Reap `child` after the group signal. A failed group kill means the
/// leader has left the group, so the leader pid is signaled while
/// `try_wait` still owns it. The wait is bounded either way.
fn reap_leader(child: &mut Child, group: libc::pid_t, found: Option<ExitStatus>) -> ExitStatus {
    let group_rc = signal_group(group);
    if let Some(status) = found {
        // The leader is already reaped. `ESRCH` means the group was
        // empty. There is no wait on this path.
        let _ = group_rc;
        return status;
    }
    if group_rc.is_err() {
        let _ = raw_signal(group, libc::SIGKILL);
    }
    wait_until(child, REAP_BOUND)
        .or_else(|| {
            let _ = raw_signal(group, libc::SIGKILL);
            wait_until(child, REAP_BOUND)
        })
        .unwrap_or_else(|| panic!("leader {group} still alive after SIGKILL"))
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

fn stop_recorded(home: &Path, spawned: &mut Spawned) {
    let deadline = Instant::now() + REAP_BOUND;
    loop {
        spawned.adopt_tree();
        let groups = spawned.groups.clone();
        for group in groups {
            let Ok(group) = libc::pid_t::try_from(group) else {
                continue;
            };
            let _ = signal_group(group);
        }
        let pids = recorded_pids(home, spawned);
        if pids.is_empty() && alive_under(home).is_empty() {
            return;
        }
        for pid in pids {
            stop_pid(pid, spawned);
        }
        if Instant::now() >= deadline {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Signals `pid` only when this harness spawned it or created its group.
///
/// The whole group is signaled. `packset-embed` publishes no pid file and
/// dies with the writer's group instead of being left behind by a signal
/// to the writer pid alone.
fn stop_pid(pid: u32, spawned: &Spawned) {
    if !may_signal(pid, spawned) {
        return;
    }
    let Ok(raw) = libc::pid_t::try_from(pid) else {
        return;
    };
    if raw <= 1 {
        return;
    }
    if let Some(group) = pgrp(pid) {
        let owned = spawned.groups.contains(&group)
            || spawned.pids.contains(&group)
            || spawned.pids.contains(&pid)
            || descends_from(pid, spawned);
        if owned {
            if let Ok(group) = libc::pid_t::try_from(group) {
                let _ = signal_group(group);
            }
        }
    }
    let _ = raw_signal(raw, libc::SIGKILL);
    // `ESRCH` means the pid is already gone. The caller polls the home
    // until [`REAP_BOUND`] and does not block in `wait`.
}

fn may_signal(pid: u32, spawned: &Spawned) -> bool {
    pid > 1 && pid != std::process::id() && (spawned.owns(pid) || descends_from(pid, spawned))
}

fn descends_from(pid: u32, spawned: &Spawned) -> bool {
    let mut cur = pid;
    for _ in 0..16 {
        let Some(parent) = ppid(cur) else {
            return false;
        };
        if parent <= 1 {
            return false;
        }
        if spawned.pids.contains(&parent) {
            return true;
        }
        cur = parent;
    }
    false
}

fn ppid(pid: u32) -> Option<u32> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1;
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

fn recorded_pids(home: &Path, spawned: &Spawned) -> Vec<u32> {
    let mut found = Vec::new();
    let mut stack = vec![home.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(path);
                continue;
            }
            note_record(&path, kind, spawned, &mut found);
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

fn note_record(path: &Path, kind: std::fs::FileType, spawned: &Spawned, found: &mut Vec<u32>) {
    if is_pid_name(path) {
        if let Some(pid) = read_pid_file(path) {
            if may_signal(pid, spawned) {
                found.push(pid);
            }
        }
        return;
    }
    if path.file_name().is_some_and(|name| name == "port") {
        if let Some(port) = read_port(path) {
            for pid in listeners(port) {
                if may_signal(pid, spawned) {
                    found.push(pid);
                }
            }
        }
        return;
    }
    if path.file_name().is_some_and(|name| name == "packsetd.lock") {
        if let Some(pid) = lock_holder(path) {
            if may_signal(pid, spawned) {
                found.push(pid);
            }
        }
        return;
    }
    if kind.is_socket() {
        if let Some(pid) = socket_owner(path) {
            if may_signal(pid, spawned) {
                found.push(pid);
            }
        }
    }
}

fn is_pid_name(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "pid")
        || path.extension().is_some_and(|ext| ext == "pid")
}

fn read_pid_file(path: &Path) -> Option<u32> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 32 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let pid = text.parse().ok()?;
    (pid > 1).then_some(pid)
}

fn read_port(path: &Path) -> Option<u16> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 16 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let port = text.parse().ok()?;
    (port > 0).then_some(port)
}

fn listeners(port: u16) -> Vec<u32> {
    let mut inodes = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 10 || cols[3] != "0A" {
                continue;
            }
            let Some((_, hex_port)) = cols[1].rsplit_once(':') else {
                continue;
            };
            let Ok(found) = u16::from_str_radix(hex_port, 16) else {
                continue;
            };
            if found != port {
                continue;
            }
            let Ok(inode) = cols[9].parse::<u64>() else {
                continue;
            };
            if inode != 0 {
                inodes.push(inode);
            }
        }
    }
    inodes.into_iter().filter_map(pid_for_inode).collect()
}

fn socket_owner(path: &Path) -> Option<u32> {
    pid_for_inode(listening_inode(path)?)
}

/// Inode of the socket bound at `path`, from the listening socket table.
///
/// That number is the sockfs inode in `socket:[inode]`. The directory
/// inode of the socket file is a different number and can name an
/// unrelated process.
fn listening_inode(path: &Path) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/net/unix").ok()?;
    let want = path.as_os_str().to_string_lossy();
    for line in text.lines().skip(1) {
        let mut cols = line.split_whitespace();
        let Some(inode) = cols.nth(6) else {
            continue;
        };
        let Some(listed) = cols.next() else {
            continue;
        };
        if listed != want.as_ref() {
            continue;
        }
        let Ok(inode) = inode.parse::<u64>() else {
            continue;
        };
        if inode != 0 {
            return Some(inode);
        }
    }
    None
}

fn pid_for_inode(inode: u64) -> Option<u32> {
    let needle = format!("socket:[{inode}]");
    for pid in proc_pids() {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if std::fs::read_link(fd.path()).is_ok_and(|target| target.to_string_lossy() == needle)
            {
                return Some(pid);
            }
        }
    }
    None
}

fn lock_holder(path: &Path) -> Option<u32> {
    for pid in proc_pids() {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if std::fs::read_link(fd.path()).is_ok_and(|target| same_path(&target, path)) {
                return Some(pid);
            }
        }
    }
    None
}

fn same_path(target: &Path, want: &Path) -> bool {
    if target == want {
        return true;
    }
    match (std::fs::canonicalize(target), std::fs::canonicalize(want)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn alive_under(home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for pid in proc_pids() {
        if !tied(pid, home) {
            continue;
        }
        let state = process_state(pid).unwrap_or('?');
        out.push(format!("{pid} {state} {}", comm(pid)));
    }
    out
}

/// A process this test started under `home`: its cwd, a home variable, or
/// an open file sits in that directory.
fn tied(pid: u32, home: &Path) -> bool {
    if pid <= 1 || pid == std::process::id() {
        return false;
    }
    match process_state(pid) {
        Some('Z' | 'X') | None => false,
        Some(_) => cwd_under(pid, home) || env_under(pid, home) || fd_under(pid, home),
    }
}

fn process_state(pid: u32) -> Option<char> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1.trim_start();
    rest.chars().next()
}

fn pgrp(pid: u32) -> Option<u32> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = text.rsplit_once(')')?.1;
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    let _ppid = fields.next()?;
    fields.next()?.parse().ok()
}

fn comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn cwd_under(pid: u32, home: &Path) -> bool {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .is_some_and(|cwd| cwd.starts_with(home))
}

fn env_under(pid: u32, home: &Path) -> bool {
    let Ok(bytes) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return false;
    };
    let home = home.as_os_str().as_encoded_bytes();
    for entry in bytes.split(|byte| *byte == 0) {
        let Some(eq) = entry.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        if !home_key(&entry[..eq]) {
            continue;
        }
        if path_under(&entry[eq + 1..], home) {
            return true;
        }
    }
    false
}

fn home_key(key: &[u8]) -> bool {
    matches!(
        key,
        b"HOME"
            | b"TMPDIR"
            | b"XDG_STATE_HOME"
            | b"XDG_RUNTIME_DIR"
            | b"XDG_CACHE_HOME"
            | b"XDG_DATA_HOME"
            | b"PACKSET_HOME"
            | b"PACKSET_LOG"
    )
}

fn path_under(value: &[u8], home: &[u8]) -> bool {
    value == home || (value.starts_with(home) && value.get(home.len()) == Some(&b'/'))
}

fn fd_under(pid: u32, home: &Path) -> bool {
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return false;
    };
    fds.flatten().any(|fd| {
        std::fs::read_link(fd.path())
            .ok()
            .is_some_and(|target| target.starts_with(home))
    })
}

fn proc_pids() -> Vec<u32> {
    let Ok(proc) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let self_pid = std::process::id();
    proc.flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .filter(|pid| *pid > 1 && *pid != self_pid)
        .collect()
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

/// Holds sockets until dropped, then the child is signaled and reaped.
struct ForeignHold {
    child: Child,
}

impl Drop for ForeignHold {
    fn drop(&mut self) {
        let pid = self.child.id();
        if let Ok(raw) = libc::pid_t::try_from(pid) {
            let _ = raw_signal(raw, libc::SIGKILL);
            let _ = signal_group(raw);
        }
        let _ = self.child.wait();
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

fn still_alive(pid: u32) -> bool {
    matches!(process_state(pid), Some(ch) if ch != 'Z' && ch != 'X')
}

/// A process this harness did not spawn holds a socket, and another binds
/// a socket file under the test home. The directory inode of that file is
/// not the listening socket. Neither process is signaled.
#[test]
fn a_foreign_process_holding_a_matching_inode_is_not_signaled() {
    let _process = ljos_test_boot::env_lock();
    let home = tempfile::TempDir::new().unwrap();
    let sock_path = home.path().join("probe.sock");
    let (binder, binder_pid) = spawn_foreign(BIND_SOCKET, Some(sock_path.as_os_str()));
    let (canary, canary_pid) = spawn_foreign(HOLD_SOCKET, None);
    let file_ino = std::fs::metadata(&sock_path).unwrap().ino();
    let listen = listening_inode(&sock_path);
    assert_ne!(listen, Some(file_ino));
    assert_eq!(socket_owner(&sock_path), Some(binder_pid));
    assert_ne!(socket_owner(&sock_path), Some(canary_pid));
    let mut spawned = Spawned::none();
    stop_recorded(home.path(), &mut spawned);
    assert!(
        still_alive(canary_pid),
        "foreign pid {canary_pid} holding a socket inode was signaled, state {:?}",
        process_state(canary_pid),
    );
    assert!(
        still_alive(binder_pid),
        "listener {binder_pid} was signaled, state {:?}",
        process_state(binder_pid),
    );
    // The directory inode is the old lookup key. A process that holds
    // `socket:[that]` is not one this harness may signal.
    if let Some(matched) = pid_for_inode(file_ino) {
        assert!(
            still_alive(matched),
            "pid {matched} matched directory inode {file_ino} and was signaled"
        );
    }
    drop(binder);
    drop(canary);
}
