//! One packsetd for one test.
//!
//! The writer gets a temp home and `--port 0`. The kernel picks the port,
//! and the test reads back the port this process bound. Drop, a panic in
//! the test thread, and SIGINT all stop it. A probe that binds a port and
//! then drops the socket is the race this replaces: another test can take
//! that port before the writer binds.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

/// How long a test waits for `GET /health`. The writer opens its store
/// before it binds. Every start this helper replaces already waited 15s
/// for that, which is enough for a cold store open on the suite's machines.
const READY: Duration = Duration::from_secs(15);

/// Pause between listens checks. The socket shows up when the writer
/// binds; 50ms is the pause the old starts used, short of a spin and
/// well inside [`READY`].
const POLL: Duration = Duration::from_millis(50);

/// Bound on one `/health` exchange. The body is a short line on
/// loopback. One second is longer than that write and shorter than
/// [`READY`], so one stuck connection does not spend the whole wait.
const HEALTH_IO: Duration = Duration::from_secs(1);

/// POSIX signal numbers. Linux and macOS use the same values.
const SIGKILL: i32 = 9;
const SIGINT: i32 = 2;
const SIG_DFL: usize = 0;

/// `kill(pid, 0)` is the existence check. `ESRCH` (3) means the pid is gone.
/// Tests are the only callers; the library build has no use for it.
#[cfg(test)]
const ESRCH: i32 = 3;

/// Writers this process has started and not yet stopped.
///
/// A fixed length chosen for a large host is the wrong bound: `cargo test`
/// runs one test per available thread by default, and one test can hold
/// more writers than that, or be started with more threads. Each node is
/// one writer. A free node (pid 0) is reused. A new node is added only
/// when every node is taken, and it stays allocated so the SIGINT handler
/// can walk the list with atomics and `kill`.
struct Live {
    pid: AtomicU32,
    next: AtomicPtr<Live>,
}

static LIVE: AtomicPtr<Live> = AtomicPtr::new(std::ptr::null_mut());

/// The node whose pid this guard clears on drop.
struct Slot(NonNull<AtomicU32>);

// SAFETY: the node is never freed. Moving the pointer does not race: the
// pid is an atomic, and the signal handler and `Drop` both use that atomic.
unsafe impl Send for Slot {}

thread_local! {
    static MINE: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
    fn raise(sig: i32) -> i32;
}

/// A packsetd started for one test. Dropping it stops the process.
#[must_use = "dropping this stops packsetd"]
pub struct Packsetd {
    child: Child,
    slot: Option<Slot>,
    port: u16,
    url: String,
    home: PathBuf,
    pack_home: PathBuf,
    owned_home: Option<PathBuf>,
    log: Arc<Mutex<String>>,
}

/// Settings for [`Packsetd::spawn`].
pub struct Builder {
    program: Option<PathBuf>,
    cmd: Option<Command>,
    home: Option<PathBuf>,
    owned_home: Option<PathBuf>,
    pack_home: Option<PathBuf>,
    ready: Duration,
}

impl Packsetd {
    /// A writer with `PACKSET_AUTH=off` and a fresh temp home.
    pub fn open() -> Self {
        let mut started = Self::build();
        started.command().env("PACKSET_AUTH", "off");
        started.spawn()
    }

    /// A writer the caller can point at its own temp seat.
    pub fn build() -> Builder {
        Builder {
            program: None,
            cmd: None,
            home: None,
            owned_home: None,
            pack_home: None,
            ready: READY,
        }
    }

    /// The port the writer bound.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:{port}`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// `HOME` the writer was given.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The pack store (`--home` and `PACKSET_HOME`).
    pub fn pack_home(&self) -> &Path {
        &self.pack_home
    }

    /// The writer's process id.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Stderr captured so far.
    pub fn log(&self) -> String {
        self.log
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }
}

impl Drop for Packsetd {
    fn drop(&mut self) {
        let pid = self.child.id();
        if let Some(slot) = self.slot.take() {
            // SAFETY: the node is still allocated. Clearing the pid frees
            // the slot for the next writer.
            unsafe { slot.0.as_ref().store(0, Ordering::SeqCst) };
        }
        untrack(pid);
        stop_child(&mut self.child);
        if let Some(path) = self.owned_home.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

impl Builder {
    /// Run this binary instead of `packsetd` on `PATH`.
    ///
    /// Set this before [`Builder::command`].
    pub fn bin(&mut self, path: impl AsRef<Path>) -> &mut Self {
        assert!(
            self.cmd.is_none(),
            "set the packsetd binary before other command settings"
        );
        self.program = Some(path.as_ref().to_path_buf());
        self
    }

    /// Use `path` as `HOME`. The caller keeps the directory.
    pub fn home_at(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.owned_home = None;
        self.home = Some(path.as_ref().to_path_buf());
        self
    }

    /// Create a temp home, if this builder does not have one yet, and
    /// return it. The guard deletes that directory on drop.
    pub fn home_path(&mut self) -> &Path {
        if self.home.is_none() {
            let path = unique_dir();
            self.owned_home = Some(path.clone());
            self.home = Some(path);
        }
        self.home.as_deref().expect("home")
    }

    /// Store the pack at `path` (`--home` and `PACKSET_HOME`).
    pub fn pack_at(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.pack_home = Some(path.as_ref().to_path_buf());
        self
    }

    /// How long [`Builder::spawn`] waits for `/health`.
    pub fn ready_for(&mut self, limit: Duration) -> &mut Self {
        self.ready = limit;
        self
    }

    /// The command, for env and arguments. The spawn still sets `HOME`,
    /// `PACKSET_HOME`, `--port 0`, and `--home`.
    pub fn command(&mut self) -> &mut Command {
        if self.cmd.is_none() {
            let program = self.program.clone().unwrap_or_else(locate_or_panic);
            self.cmd = Some(Command::new(program));
        }
        self.cmd.as_mut().expect("command")
    }

    /// Start the writer and wait until it answers `/health`.
    pub fn spawn(mut self) -> Packsetd {
        install_handlers();
        let home = self.resolve_home();
        let pack = self.pack_home.clone().unwrap_or_else(|| home.clone());
        std::fs::create_dir_all(&pack)
            .unwrap_or_else(|err| panic!("pack home {}: {err}", pack.display()));
        let program = self
            .program
            .clone()
            .or_else(|| {
                self.cmd
                    .as_ref()
                    .and_then(|cmd| cmd.get_program().to_str().map(PathBuf::from))
            })
            .unwrap_or_else(locate_or_panic);
        let mut cmd = self.cmd.take().unwrap_or_else(|| Command::new(&program));
        // HOME is a directory this test created, or a temp seat the caller
        // already set. A writer that inherits the process home can write
        // into that seat's real files.
        cmd.env("HOME", &home);
        cmd.env("PACKSET_HOME", &pack);
        // Port 0 is bind's request for an unused port. The pinned writer
        // binds that port itself; it has no flag that takes a listener.
        // The port is read back from this process.
        cmd.arg("--port")
            .arg("0")
            .arg("--home")
            .arg(&pack)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Own process group, so SIGINT to the test does not depend on
            // the writer staying in the foreground group, and stopping the
            // writer also stops children it spawned.
            cmd.process_group(0);
        }
        let child = cmd
            .spawn()
            .unwrap_or_else(|err| panic!("spawn {}: {err}", program.display()));
        let pid = child.id();
        let slot = claim(pid);
        track(pid);
        let log = Arc::new(Mutex::new(String::new()));
        let mut started = Packsetd {
            child,
            slot: Some(slot),
            port: 0,
            url: String::new(),
            home,
            pack_home: pack,
            owned_home: self.owned_home.take(),
            log: Arc::clone(&log),
        };
        if let Some(stderr) = started.child.stderr.take() {
            let captured = Arc::clone(&log);
            std::thread::spawn(move || drain(stderr, captured));
        }
        started.wait_ready(self.ready);
        started
    }

    fn resolve_home(&mut self) -> PathBuf {
        if let Some(home) = &self.home {
            return home.clone();
        }
        if let Some(cmd) = &self.cmd {
            if let Some(value) = env_path(cmd, "HOME") {
                if Some(value.as_os_str()) != std::env::var_os("HOME").as_deref() {
                    self.home = Some(value.clone());
                    return value;
                }
            }
        }
        let path = unique_dir();
        self.owned_home = Some(path.clone());
        self.home = Some(path.clone());
        path
    }
}

impl Packsetd {
    fn wait_ready(&mut self, limit: Duration) {
        let deadline = Instant::now() + limit;
        let pid = self.child.id();
        loop {
            if let Some(status) = self.child.try_wait().expect("packsetd status") {
                panic!("packsetd exited {status}: {}", self.log());
            }
            if let Some(port) = bound_port(pid, &self.log()) {
                if health(port) {
                    self.port = port;
                    self.url = format!("http://127.0.0.1:{port}");
                    return;
                }
            }
            assert!(
                Instant::now() < deadline,
                "packsetd pid {pid} did not answer: {}",
                self.log()
            );
            std::thread::sleep(POLL);
        }
    }
}

fn drain(mut stderr: impl Read, log: Arc<Mutex<String>>) {
    let mut buf = [0u8; 1024];
    loop {
        match stderr.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                let mut guard = log.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.push_str(&text);
            }
        }
    }
}

fn env_path(cmd: &Command, key: &str) -> Option<PathBuf> {
    cmd.get_envs().find_map(|(name, value)| {
        if name == key {
            value.map(PathBuf::from)
        } else {
            None
        }
    })
}

/// `packsetd` on `PATH`, or the binary a local build installs at
/// `/tmp/packset-bin/packsetd` (the fallback `mail_wake_push` already used).
fn locate() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("packsetd"))
            .find(|candidate| candidate.is_file())
    }) {
        return Some(path);
    }
    let fallback = PathBuf::from("/tmp/packset-bin/packsetd");
    fallback.is_file().then_some(fallback)
}

fn locate_or_panic() -> PathBuf {
    locate().unwrap_or_else(|| panic!("packsetd is not on PATH; CI builds it from packset develop"))
}

fn unique_dir() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    // A name that already exists is a leftover. The next counter value
    // is a free name. A full disk is a different error and panics at once.
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ljos-packsetd-{}-{n}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return path,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => panic!("temp home {}: {err}", path.display()),
        }
    }
}

fn health(port: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(HEALTH_IO));
    let _ = stream.set_write_timeout(Some(HEALTH_IO));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut said = String::new();
    let _ = stream.read_to_string(&mut said);
    said.starts_with("HTTP/1.1 200")
}

/// The port `pid` is listening on.
///
/// The pinned writer prints the port it was asked for. `--port 0` therefore
/// prints `http://127.0.0.1:0`, which is not the port it bound. `/proc` is
/// the socket this process holds, and that is the port read back. A writer
/// that prints the bound port is accepted where `/proc/net/tcp` is absent.
fn bound_port(pid: u32, log: &str) -> Option<u16> {
    let from_proc = proc_listen_port(pid);
    let from_log = port_in_log(log);
    let proc_table = Path::new("/proc/net/tcp").is_file();
    match (from_proc, from_log) {
        (Some(proc_port), Some(log_port)) if proc_port == log_port => Some(proc_port),
        (Some(proc_port), None) => Some(proc_port),
        (None, Some(log_port)) if !proc_table => Some(log_port),
        _ => None,
    }
}

fn port_in_log(log: &str) -> Option<u16> {
    for line in log.lines() {
        let Some(rest) = line.split("listening on http://").nth(1) else {
            continue;
        };
        let hostport = rest.split_whitespace().next()?;
        let port = hostport.rsplit_once(':')?.1;
        let port = port.parse().ok()?;
        if port != 0 {
            return Some(port);
        }
    }
    None
}

fn proc_listen_port(pid: u32) -> Option<u16> {
    let mut inodes = BTreeSet::new();
    let entries = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let text = target.to_string_lossy();
        let Some(rest) = text.strip_prefix("socket:[") else {
            continue;
        };
        let Some(inode) = rest.strip_suffix(']') else {
            continue;
        };
        if let Ok(inode) = inode.parse::<u64>() {
            inodes.insert(inode);
        }
    }
    if inodes.is_empty() {
        return None;
    }
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        if let Some(port) = listen_port(&inodes, &text) {
            return Some(port);
        }
    }
    None
}

fn listen_port(inodes: &BTreeSet<u64>, table: &str) -> Option<u16> {
    for line in table.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let (Some(local), Some(state), Some(inode)) = (cols.get(1), cols.get(3), cols.get(9))
        else {
            continue;
        };
        if *state != "0A" {
            continue;
        }
        let Ok(inode) = inode.parse::<u64>() else {
            continue;
        };
        if !inodes.contains(&inode) {
            continue;
        }
        let Some((_, port)) = local.rsplit_once(':') else {
            continue;
        };
        if let Ok(port) = u16::from_str_radix(port, 16) {
            if port != 0 {
                return Some(port);
            }
        }
    }
    None
}

fn claim(pid: u32) -> Slot {
    let mut cur = LIVE.load(Ordering::SeqCst);
    while !cur.is_null() {
        // SAFETY: a published node is never freed.
        let node = unsafe { &*cur };
        if node
            .pid
            .compare_exchange(0, pid, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return Slot(NonNull::from(&node.pid));
        }
        cur = node.next.load(Ordering::SeqCst);
    }
    let node = Box::into_raw(Box::new(Live {
        pid: AtomicU32::new(pid),
        next: AtomicPtr::new(std::ptr::null_mut()),
    }));
    loop {
        let head = LIVE.load(Ordering::SeqCst);
        // SAFETY: `node` is unpublished until the swap below succeeds, so
        // only this thread writes `next`.
        unsafe { (*node).next.store(head, Ordering::SeqCst) };
        if LIVE
            .compare_exchange(head, node, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            // SAFETY: the node stays allocated, and `pid` is its first field.
            return Slot(NonNull::from(unsafe { &(*node).pid }));
        }
    }
}

fn track(pid: u32) {
    MINE.with(|mine| mine.borrow_mut().push(pid));
}

fn untrack(pid: u32) {
    MINE.with(|mine| mine.borrow_mut().retain(|held| *held != pid));
}

fn stop_child(child: &mut Child) {
    stop_pid(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

fn stop_pid(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    #[cfg(unix)]
    unsafe {
        // The negative id is the process group. A writer that is not a
        // group leader ignores that kill; the pid kill still lands.
        kill(-pid, SIGKILL);
        kill(pid, SIGKILL);
    }
}

fn stop_all() {
    let mut cur = LIVE.load(Ordering::SeqCst);
    while !cur.is_null() {
        // SAFETY: a published node is never freed. This runs in the SIGINT
        // handler, so it only loads atomics and calls `kill`.
        let node = unsafe { &*cur };
        let pid = node.pid.swap(0, Ordering::SeqCst);
        if pid != 0 {
            stop_pid(pid);
        }
        cur = node.next.load(Ordering::SeqCst);
    }
}

fn stop_this_thread() {
    let Ok(pids) = MINE.try_with(|mine| {
        mine.try_borrow()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }) else {
        return;
    };
    for pid in pids {
        stop_pid(pid);
    }
}

fn install_handlers() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        #[cfg(unix)]
        unsafe {
            signal(SIGINT, on_sigint as SigHandler as usize);
        }
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Unwind drops the guard too. This covers the panic that
            // fires after the pid is tracked and before that guard is
            // what the unwind drops, and it only stops writers this
            // thread started.
            stop_this_thread();
            previous(info);
        }));
    });
}

/// The C `sighandler_t`. Casting through it checks the handler's
/// signature before `signal` takes it as an address.
#[cfg(unix)]
type SigHandler = unsafe extern "C" fn(i32);

#[cfg(unix)]
unsafe extern "C" fn on_sigint(sig: i32) {
    unsafe {
        // Re-arm in case this libc clears the handler when it runs.
        signal(sig, on_sigint as SigHandler as usize);
        stop_all();
        signal(sig, SIG_DFL);
        raise(sig);
    }
}

/// Tests ask whether a writer pid is still running. The library stops
/// pids; it does not poll them.
#[cfg(test)]
fn alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    #[cfg(unix)]
    {
        // SAFETY: signal 0 delivers nothing. It reports whether `pid` exists.
        let rc = unsafe { kill(pid, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDIN_C: &str = r#"
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
int main(void) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return 1;
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof addr);
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = htons(0);
    if (bind(fd, (struct sockaddr *)&addr, sizeof addr) != 0) return 1;
    if (listen(fd, 16) != 0) return 1;
    struct sockaddr_in bound;
    socklen_t len = sizeof bound;
    if (getsockname(fd, (struct sockaddr *)&bound, &len) != 0) return 1;
    fprintf(stderr, "packsetd: listening on http://127.0.0.1:%u with 1 workers\n",
            ntohs(bound.sin_port));
    fflush(stderr);
    for (;;) {
        int c = accept(fd, 0, 0);
        if (c < 0) continue;
        char buf[1024];
        (void)read(c, buf, sizeof buf);
        const char *msg =
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\npacksetd ok";
        (void)write(c, msg, strlen(msg));
        close(c);
    }
}
"#;

    fn standin() -> PathBuf {
        static BIN: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        BIN.get_or_init(|| {
            let dir = unique_dir();
            let src = dir.join("standin.c");
            let bin = dir.join("ljos-writer-standin");
            std::fs::write(&src, STANDIN_C).unwrap();
            let out = Command::new("cc")
                .args(["-O2", "-o"])
                .arg(&bin)
                .arg(&src)
                .output()
                .expect("cc");
            assert!(
                out.status.success(),
                "cc standin: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            bin
        })
        .clone()
    }

    fn start_standin() -> Packsetd {
        let mut started = Packsetd::build();
        started.bin(standin());
        started.spawn()
    }

    #[test]
    fn the_writer_gets_a_temp_home_and_a_kernel_port_and_drop_stops_it() {
        let daemon = start_standin();
        let pid = daemon.id();
        assert_ne!(daemon.port(), 0, "the bound port was not read back");
        assert_ne!(daemon.port(), 8761, "the writer took the default port");
        let process_home = std::env::var_os("HOME");
        assert_ne!(
            Some(daemon.home().as_os_str()),
            process_home.as_deref(),
            "the writer inherited the process home"
        );
        let env = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
        let text = String::from_utf8_lossy(&env);
        let home = daemon.home().to_string_lossy();
        assert!(
            text.split('\0')
                .any(|entry| entry == format!("HOME={home}")),
            "HOME in the writer: {text}"
        );
        assert!(alive(pid), "the writer was not alive");
        drop(daemon);
        assert!(!alive(pid), "drop left writer pid {pid} alive");
    }

    #[test]
    fn two_writers_do_not_share_a_port() {
        let first = start_standin();
        let second = start_standin();
        assert_ne!(first.port(), second.port());
        assert_ne!(first.port(), 8761);
        assert_ne!(second.port(), 8761);
    }

    /// The default harness runs one test per available thread. This test
    /// holds one more writer than that and still stops every one on drop.
    #[test]
    fn more_writers_than_harness_threads_all_stop() {
        let threads = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1);
        let mut writers = Vec::new();
        for _ in 0..threads + 1 {
            writers.push(start_standin());
        }
        let mut ports = BTreeSet::new();
        for writer in &writers {
            assert_ne!(writer.port(), 0, "the bound port was not read back");
            assert_ne!(writer.port(), 8761, "a writer took the default port");
            assert!(ports.insert(writer.port()), "two writers shared a port");
            assert!(alive(writer.id()), "a writer exited during start");
        }
        let pids: Vec<u32> = writers.iter().map(|writer| writer.id()).collect();
        drop(writers);
        for pid in pids {
            assert!(!alive(pid), "writer pid {pid} was still alive after drop");
        }
    }

    #[test]
    fn a_panic_after_start_leaves_no_writer() {
        let dir = unique_dir();
        let pidfile = dir.join("pid");
        let pidfile_text = pidfile.to_string_lossy().into_owned();
        let standin = standin();
        let standin_text = standin.to_string_lossy().into_owned();
        let caught = std::panic::catch_unwind(|| panic_after(&standin_text, &pidfile_text));
        let pid = std::fs::read_to_string(&pidfile)
            .unwrap_or_default()
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        let dead = pid != 0 && !alive(pid);
        if pid != 0 && alive(pid) {
            stop_pid(pid);
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(caught.is_err(), "the test body did not panic");
        assert!(dead, "writer pid {pid} was still alive after the panic");
    }

    fn panic_after(standin: &str, pidfile: &str) {
        let mut started = Packsetd::build();
        started.bin(standin);
        let daemon = started.spawn();
        std::fs::write(pidfile, daemon.id().to_string()).unwrap();
        assert!(alive(daemon.id()), "the writer was not alive");
        panic!("after start");
    }

    #[test]
    fn a_startup_failure_leaves_no_writer() {
        let dir = unique_dir();
        let script = dir.join("sleep.sh");
        let pidfile = dir.join("pid");
        // The script ignores the `--port 0 --home` the helper appends.
        // `sleep 60` is longer than the two-second ready budget below, so
        // the process is still alive when the helper gives up. A process
        // that exited on its own would not show that the guard stopped it.
        std::fs::write(&script, "#!/bin/sh\necho $$ > \"$1\"\nexec sleep 60\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let script_text = script.to_string_lossy().into_owned();
        let pidfile_text = pidfile.to_string_lossy().into_owned();
        // Two seconds is long enough for the script to write its pid on
        // a loaded CPU, and short of the 15s the real writer is given.
        // This stand-in never listens, so the wait ends in the panic.
        let caught = std::panic::catch_unwind(|| {
            let mut started = Packsetd::build();
            started.bin(&script_text);
            started.ready_for(Duration::from_secs(2));
            started.command().arg(&pidfile_text);
            let daemon = started.spawn();
            panic!(
                "a writer that never listens was returned on port {}",
                daemon.port()
            );
        });
        let pid = std::fs::read_to_string(&pidfile)
            .unwrap_or_default()
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        let dead = pid != 0 && !alive(pid);
        if pid != 0 && alive(pid) {
            stop_pid(pid);
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(caught.is_err(), "a writer that never listens was returned");
        assert!(
            dead,
            "writer pid {pid} was still alive after the failed start"
        );
    }

    #[test]
    fn a_sigint_stops_the_writer() {
        if std::env::var_os("LJOS_PACKSETD_SIGINT_CHILD").is_some() {
            sigint_child();
            return;
        }
        sigint_parent();
    }

    fn sigint_child() {
        let standin = std::env::var("LJOS_WRITER_STANDIN").unwrap();
        let pidfile = std::env::var("LJOS_WRITER_PIDFILE").unwrap();
        let mut started = Packsetd::build();
        started.bin(&standin);
        let daemon = started.spawn();
        std::fs::write(&pidfile, daemon.id().to_string()).unwrap();
        // The parent signals this process inside its five-second wait.
        // Thirty seconds is only the fallback when that signal never arrives.
        std::thread::sleep(Duration::from_secs(30));
    }

    fn sigint_parent() {
        let dir = unique_dir();
        let pidfile = dir.join("pid");
        let exe = std::env::current_exe().unwrap();
        let mut child = Command::new(exe)
            .arg("packsetd::tests::a_sigint_stops_the_writer")
            .arg("--exact")
            .env("LJOS_PACKSETD_SIGINT_CHILD", "1")
            .env("LJOS_WRITER_STANDIN", standin())
            .env("LJOS_WRITER_PIDFILE", &pidfile)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Same budget as a writer start: the child has to compile nothing,
        // only spawn the stand-in and write the pid file.
        let deadline = Instant::now() + READY;
        while !pidfile.is_file() {
            if let Some(status) = child.try_wait().unwrap() {
                let err = read_stderr(&mut child);
                panic!("child exited {status} before the writer was up: {err}");
            }
            assert!(
                Instant::now() < deadline,
                "child did not start the writer: {}",
                read_stderr(&mut child)
            );
            std::thread::sleep(POLL);
        }
        let pid: u32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(alive(pid), "writer pid {pid} was not alive");
        #[cfg(unix)]
        unsafe {
            kill(i32::try_from(child.id()).unwrap(), SIGINT);
        }
        // Signal delivery is prompt. Five seconds is longer than that and
        // shorter than the child's thirty-second fallback, so a handler
        // that does not stop the process fails here.
        let signal_deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= signal_deadline {
                let _ = child.kill();
                let _ = child.wait();
                stop_pid(pid);
                panic!("child did not exit on SIGINT");
            }
            std::thread::sleep(POLL);
        }
        let dead = !alive(pid);
        if !dead {
            stop_pid(pid);
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(dead, "writer pid {pid} was still alive after SIGINT");
    }

    fn read_stderr(child: &mut Child) -> String {
        let mut err = String::new();
        if let Some(pipe) = child.stderr.as_mut() {
            let _ = pipe.read_to_string(&mut err);
        }
        err
    }

    #[test]
    fn a_panic_after_packsetd_starts_leaves_it_dead() {
        if locate().is_none() {
            eprintln!("skipped: packsetd is not installed");
            return;
        }
        let dir = unique_dir();
        let pidfile = dir.join("pid");
        let pidfile_text = pidfile.to_string_lossy().into_owned();
        let caught = std::panic::catch_unwind(|| panic_after_packsetd(&pidfile_text));
        let pid = std::fs::read_to_string(&pidfile)
            .unwrap_or_default()
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        let dead = pid != 0 && !alive(pid);
        if pid != 0 && alive(pid) {
            stop_pid(pid);
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(caught.is_err(), "the test body did not panic");
        assert!(dead, "packsetd pid {pid} was still alive after the panic");
    }

    fn panic_after_packsetd(pidfile: &str) {
        let daemon = Packsetd::open();
        std::fs::write(pidfile, daemon.id().to_string()).unwrap();
        assert_ne!(daemon.port(), 8761);
        assert!(alive(daemon.id()), "packsetd was not alive");
        panic!("after start");
    }
}
