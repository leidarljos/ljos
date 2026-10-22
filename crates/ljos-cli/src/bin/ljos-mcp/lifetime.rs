//! When the server stops. The end of stdin is held back from rmcp until
//! the tool calls already read have answered: rmcp waits five seconds for
//! a handler after the input ends and then drops its answer, and a pack
//! write may take thirty. The death of the client ends the input the same
//! way. A shell or a launcher that only started the server is not the
//! client: the nearest ancestor that is not one of those is, recorded by
//! pid and start time so a reused pid is not taken for it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::task::Waker;
use std::time::Duration;

/// How long the end of input waits for calls in flight. A pack request
/// gives up at 30 s unless `PACKSET_TIMEOUT_MS` says otherwise, and the
/// drain has to outlast that request.
pub const SETTLE: Duration = Duration::from_secs(35);

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static READER: Mutex<Option<Waker>> = Mutex::new(None);
static CLOSING: AtomicBool = AtomicBool::new(false);
static PRCTL_LOGGED: AtomicBool = AtomicBool::new(false);

/// End the input as if stdin had closed.
fn close() {
    CLOSING.store(true, Ordering::Release);
    wake_reader();
}

/// The input was ended by [`close`].
pub fn closing() -> bool {
    CLOSING.load(Ordering::Acquire)
}

/// A process recorded at start: the pid and the start time beside it.
#[derive(Clone, Copy)]
struct Watched {
    pid: u32,
    start: u64,
}

/// Names that only launch the server and are not the client. `node` is
/// here only when it is running npx; a node that is the editor is not.
const LAUNCHERS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "dash", "env", "npx", "uvx", "timeout", "nohup", "setsid",
    "script",
];

/// Whether `name` is one of [`LAUNCHERS`], comparing the file name.
fn launcher_name(name: &str) -> bool {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    LAUNCHERS.contains(&base)
}

/// `sudo -n`: non-interactive sudo, which waits in front of the client.
fn sudo_n(argv: &[String]) -> bool {
    argv.iter().any(|arg| {
        arg == "-n" || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains('n'))
    })
}

/// Whether this process is a shell or a wrapper the client started us
/// through, rather than the client.
fn is_launcher(comm: &str, argv: &[String]) -> bool {
    let argv0 = argv.first().map(String::as_str).unwrap_or("");
    let sudo = comm == "sudo" || argv0.rsplit(['/', '\\']).next() == Some("sudo");
    if sudo {
        return sudo_n(argv);
    }
    if launcher_name(comm) || launcher_name(argv0) {
        return true;
    }
    let node = comm == "node"
        || comm == "nodejs"
        || argv0
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|b| b == "node" || b == "nodejs");
    node && argv.iter().any(|arg| {
        let base = arg.rsplit(['/', '\\']).next().unwrap_or(arg);
        base == "npx" || base.starts_with("npx-")
    })
}

/// Arguments of `pid`, not including the empty terminator.
#[cfg(target_os = "linux")]
fn argv_of(pid: u32) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// The nearest ancestor that is not a shell or a wrapper, recorded with
/// the start time `/proc` reports. Init is not a client. `None` when
/// every ancestor up to init is a launcher, or `/proc` cannot be read.
#[cfg(target_os = "linux")]
fn client_ancestor() -> Option<Watched> {
    ljos_cli::proc_tree::ancestors()
        .into_iter()
        .find_map(|proc| {
            (proc.pid > 1 && !is_launcher(&proc.comm, &argv_of(proc.pid))).then_some(Watched {
                pid: proc.pid,
                start: proc.start,
            })
        })
}

/// Whether `watched` is still that same process.
#[cfg(target_os = "linux")]
fn still_there(watched: &Watched) -> bool {
    ljos_cli::proc_tree::alive(&ljos_cli::proc_tree::Proc {
        pid: watched.pid,
        start: watched.start,
        comm: String::new(),
    })
}

#[cfg(target_os = "macos")]
struct Info {
    ppid: u32,
    start: u64,
    comm: String,
}

#[cfg(target_os = "macos")]
fn inspect(pid: u32) -> Option<Info> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: the buffer is a zeroed proc_bsdinfo of the size passed.
    let n = unsafe {
        libc::proc_pidinfo(
            libc::c_int::try_from(pid).ok()?,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::addr_of_mut!(info).cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    let comm: Vec<u8> = info
        .pbi_comm
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| c.to_ne_bytes()[0])
        .collect();
    let start = info
        .pbi_start_tvsec
        .saturating_mul(1_000_000)
        .saturating_add(info.pbi_start_tvusec);
    Some(Info {
        ppid: info.pbi_ppid,
        start,
        comm: String::from_utf8_lossy(&comm).into_owned(),
    })
}

#[cfg(target_os = "macos")]
fn argv_of(pid: u32) -> Vec<String> {
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROCARGS2,
        libc::c_int::try_from(pid).unwrap_or(0),
    ];
    let mut size: libc::size_t = 0;
    // SAFETY: a null buffer asks sysctl for the size alone.
    let asked = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if asked != 0 || size < 4 {
        return Vec::new();
    }
    let mut buf = vec![0u8; size];
    // SAFETY: buf holds the size sysctl reported.
    let read = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return Vec::new();
    }
    buf.truncate(size);
    let Ok(argc) = buf
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .map(i32::from_ne_bytes)
        .and_then(|n| usize::try_from(n).ok())
    else {
        return Vec::new();
    };
    buf[4..]
        .split(|b| *b == 0)
        .skip(1)
        .skip_while(|w| w.is_empty())
        .take(argc)
        .map(|w| String::from_utf8_lossy(w).into_owned())
        .collect()
}

#[cfg(target_os = "macos")]
fn client_ancestor() -> Option<Watched> {
    // SAFETY: getppid has no preconditions.
    let mut pid = unsafe { libc::getppid() } as u32;
    for _ in 0..32 {
        if pid <= 1 {
            return None;
        }
        let info = inspect(pid)?;
        if !is_launcher(&info.comm, &argv_of(pid)) {
            return Some(Watched {
                pid,
                start: info.start,
            });
        }
        if info.ppid == pid {
            return None;
        }
        pid = info.ppid;
    }
    None
}

#[cfg(target_os = "macos")]
fn still_there(watched: &Watched) -> bool {
    inspect(watched.pid).is_some_and(|info| info.start == watched.start)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn client_ancestor() -> Option<Watched> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn still_there(_watched: &Watched) -> bool {
    true
}

/// Stdin has hung up and nothing is left to read. Pending bytes are the
/// reader's: a hangup with `POLLIN` still has a request in the pipe.
fn stdin_finished() -> bool {
    let mut fd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN | libc::POLLHUP,
        revents: 0,
    };
    // SAFETY: fd points at one pollfd owned by this frame; the timeout is 0.
    let n = unsafe { libc::poll(&mut fd, 1, 0) };
    if n <= 0 {
        return false;
    }
    let rev = fd.revents;
    if rev & libc::POLLIN != 0 {
        return false;
    }
    rev & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
}

/// Report one failed `PR_SET_PDEATHSIG`. Later failures stay quiet.
fn note_prctl(rc: i32) {
    if rc == 0 || PRCTL_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "ljos-mcp: PR_SET_PDEATHSIG failed ({}); watching the client by its pid",
        std::io::Error::last_os_error()
    );
}

/// Arm the parent-death signal so a parent thread that exits is noticed,
/// and say so once if the kernel refuses. The signal is not the decision:
/// the watched ancestor is. `SIGHUP` is swallowed, because its default
/// action would kill the process before that ancestor can be checked.
#[cfg(target_os = "linux")]
fn arm_pdeathsig() {
    if let Ok(mut hangup) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
        tokio::spawn(async move { while hangup.recv().await.is_some() {} });
    }
    // SAFETY: PR_SET_PDEATHSIG takes a signal number and no pointers.
    let rc = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGHUP) };
    note_prctl(rc);
}

/// A kqueue watching one process, or none when this platform has no
/// kqueue or the queue could not be opened.
enum Kqueue {
    #[cfg(target_os = "macos")]
    Armed(libc::c_int),
    /// The process was already gone when the queue was armed.
    #[cfg(target_os = "macos")]
    Exited,
    Idle,
}

impl Drop for Kqueue {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Self::Armed(fd) = self {
            // SAFETY: the fd was opened by kqueue and is still open.
            unsafe { libc::close(*fd) };
        }
    }
}

#[cfg(target_os = "macos")]
fn open_kqueue(pid: u32) -> Kqueue {
    // SAFETY: kqueue takes no arguments. kevent gets one change owned by
    // this frame and no event buffer: nevents 0 registers and returns.
    unsafe {
        let queue = libc::kqueue();
        if queue < 0 {
            return Kqueue::Idle;
        }
        let mut change: libc::kevent = std::mem::zeroed();
        change.ident = pid as libc::uintptr_t;
        change.filter = libc::EVFILT_PROC;
        change.flags = libc::EV_ADD | libc::EV_ONESHOT;
        change.fflags = libc::NOTE_EXIT;
        let n = libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null());
        if n < 0 {
            let gone = std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            libc::close(queue);
            return if gone { Kqueue::Exited } else { Kqueue::Idle };
        }
        Kqueue::Armed(queue)
    }
}

#[cfg(not(target_os = "macos"))]
fn open_kqueue(_pid: u32) -> Kqueue {
    Kqueue::Idle
}

/// Block up to 200 ms. `true` when the armed queue sees the process exit.
#[cfg(target_os = "macos")]
fn tick(queue: &Kqueue) -> bool {
    let Kqueue::Armed(queue) = queue else {
        if matches!(queue, Kqueue::Exited) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
        return false;
    };
    // SAFETY: queue is an open kqueue. The event and the timeout are owned
    // by this frame. A null changelist waits for an event already armed.
    unsafe {
        let mut event: libc::kevent = std::mem::zeroed();
        let mut timeout = libc::timespec {
            tv_sec: 0,
            tv_nsec: 200_000_000,
        };
        loop {
            let n = libc::kevent(*queue, std::ptr::null(), 0, &mut event, 1, &timeout);
            let errno = std::io::Error::last_os_error().raw_os_error();
            if n < 0 && errno == Some(libc::EINTR) {
                continue;
            }
            if n < 0 {
                return errno == Some(libc::ESRCH);
            }
            if n == 0 {
                return false;
            }
            if event.flags & libc::EV_ERROR != 0 {
                return event.data == libc::ESRCH as libc::intptr_t;
            }
            return event.fflags & libc::NOTE_EXIT != 0;
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn tick(_queue: &Kqueue) -> bool {
    std::thread::sleep(Duration::from_millis(200));
    false
}

fn watch(client: Option<Watched>) {
    let queue = client
        .as_ref()
        .map(|c| open_kqueue(c.pid))
        .unwrap_or(Kqueue::Idle);
    loop {
        if client.as_ref().is_some_and(|c| !still_there(c)) || stdin_finished() || tick(&queue) {
            close();
            return;
        }
    }
}

/// End the input when the client exits or stdin hangs up.
///
/// The client is the nearest ancestor that is not a shell or a wrapper
/// (`sh`, `npx`, `uvx`, `sudo -n`, and the rest of that list). Its pid
/// and start time are taken once, here. On Linux the parent-death signal
/// is armed as a hint and its failure is logged once; the decision is the
/// recorded ancestor, because a launcher can outlive the client. On macOS
/// the same ancestor is watched with `kqueue` `NOTE_EXIT`.
pub fn watch_parent() {
    #[cfg(target_os = "linux")]
    arm_pdeathsig();
    let client = client_ancestor();
    let _ = std::thread::Builder::new()
        .name("ljos-mcp-client".into())
        .stack_size(64 * 1024)
        .spawn(move || watch(client));
}

/// One tool call between the line being read and its answer being built.
pub struct InFlight(());

impl Drop for InFlight {
    fn drop(&mut self) {
        if IN_FLIGHT.fetch_sub(1, Ordering::AcqRel) == 1 {
            wake_reader();
        }
    }
}

/// Hold for the length of one tool call.
#[must_use]
pub fn call_in_flight() -> InFlight {
    IN_FLIGHT.fetch_add(1, Ordering::AcqRel);
    InFlight(())
}

/// No tool call is running.
pub fn settled() -> bool {
    IN_FLIGHT.load(Ordering::Acquire) == 0
}

/// Wake `waker` when the last call in flight ends.
pub fn park_reader(waker: &Waker) {
    let mut slot = READER.lock().unwrap_or_else(|err| err.into_inner());
    match slot.as_ref() {
        Some(held) if held.will_wake(waker) => {}
        _ => *slot = Some(waker.clone()),
    }
}

fn wake_reader() {
    let waker = READER.lock().unwrap_or_else(|err| err.into_inner()).take();
    if let Some(waker) = waker {
        waker.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_and_npx_are_launchers_and_an_editor_is_not() {
        assert!(is_launcher(
            "bash",
            &["bash".into(), "-lc".into(), "ljos-mcp".into()]
        ));
        assert!(is_launcher(
            "sh",
            &["/bin/sh".into(), "-c".into(), "ljos-mcp".into()]
        ));
        assert!(is_launcher("npx", &["npx".into(), "ljos-mcp".into()]));
        assert!(is_launcher("uvx", &["uvx".into(), "ljos-mcp".into()]));
        assert!(is_launcher(
            "node",
            &[
                "node".into(),
                "/usr/lib/node_modules/npm/bin/npx-cli.js".into()
            ]
        ));
        assert!(!is_launcher("node", &["node".into(), "server.js".into()]));
        assert!(is_launcher(
            "sudo",
            &["sudo".into(), "-n".into(), "ljos-mcp".into()]
        ));
        assert!(!is_launcher("sudo", &["sudo".into(), "ljos-mcp".into()]));
        assert!(!is_launcher("cursor", &["cursor".into()]));
        assert!(!is_launcher(
            "python3",
            &["python3".into(), "launch.py".into()]
        ));
    }

    #[test]
    fn a_failed_prctl_is_reported_once() {
        note_prctl(-1);
        note_prctl(-1);
        assert!(PRCTL_LOGGED.load(Ordering::Relaxed));
    }
}
