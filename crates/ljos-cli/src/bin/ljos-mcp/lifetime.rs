//! When the server stops. The end of stdin is held back from rmcp until
//! the tool calls already read have answered: rmcp waits five seconds for
//! a handler after the input ends and then drops its answer, and a pack
//! write may take thirty. The death of the process that started the
//! server ends the input the same way, since a client that dies can leave
//! stdin open in a grandchild.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::task::Waker;
use std::time::Duration;

use rmcp::model::RequestId;

/// How long the end of input waits for calls in flight. A pack request
/// gives up at 30 s unless `PACKSET_TIMEOUT_MS` says otherwise.
pub const SETTLE: Duration = Duration::from_secs(35);

static IN_FLIGHT: Mutex<Vec<RequestId>> = Mutex::new(Vec::new());
static READER: Mutex<Option<Waker>> = Mutex::new(None);
static CLOSING: AtomicBool = AtomicBool::new(false);

/// End the input as if stdin had closed.
fn close() {
    CLOSING.store(true, Ordering::Release);
    wake_reader();
}

/// The input was ended by [`close`].
pub fn closing() -> bool {
    CLOSING.load(Ordering::Acquire)
}

/// End the input when the parent process exits. A parent of 1 or 0 is
/// already init or outside this namespace, with nothing to watch.
///
/// Linux delivers `PR_SET_PDEATHSIG` when the parent *thread* exits, and
/// a runner may spawn from a pool thread that leaves while the runner
/// lives, so the hangup only counts once `getppid` has changed.
#[cfg(target_os = "linux")]
pub fn watch_parent() {
    // SAFETY: getppid has no preconditions.
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        return;
    }
    let Ok(mut hangup) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
    else {
        return;
    };
    // SAFETY: PR_SET_PDEATHSIG takes a signal number and no pointers.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGHUP);
    }
    let gone = move || unsafe { libc::getppid() } != parent;
    if gone() {
        close();
        return;
    }
    tokio::spawn(async move {
        while hangup.recv().await.is_some() {
            if gone() {
                close();
                return;
            }
        }
    });
}

/// End the input when the parent process exits, seen through kqueue.
#[cfg(target_os = "macos")]
pub fn watch_parent() {
    // SAFETY: getppid has no preconditions.
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("ljos-mcp-parent".into())
        .stack_size(64 * 1024)
        .spawn(move || {
            if parent_exit(parent) {
                close();
            }
        });
}

/// Block until `parent` exits. `false` when kqueue cannot watch it.
#[cfg(target_os = "macos")]
fn parent_exit(parent: libc::pid_t) -> bool {
    // SAFETY: kqueue takes no arguments; kevent gets one change and room
    // for one event, both zeroed structs owned by this frame.
    unsafe {
        let queue = libc::kqueue();
        if queue < 0 {
            return false;
        }
        let mut change: libc::kevent = std::mem::zeroed();
        change.ident = parent as libc::uintptr_t;
        change.filter = libc::EVFILT_PROC;
        change.flags = libc::EV_ADD | libc::EV_ONESHOT;
        change.fflags = libc::NOTE_EXIT;
        let exited = loop {
            let mut event: libc::kevent = std::mem::zeroed();
            let n = libc::kevent(queue, &change, 1, &mut event, 1, std::ptr::null());
            let errno = std::io::Error::last_os_error().raw_os_error();
            if n < 0 && errno == Some(libc::EINTR) {
                continue;
            }
            if n < 0 {
                break errno == Some(libc::ESRCH);
            }
            if n == 0 {
                continue;
            }
            if event.flags & libc::EV_ERROR != 0 {
                break event.data == libc::ESRCH as libc::intptr_t;
            }
            break event.fflags & libc::NOTE_EXIT != 0;
        };
        libc::close(queue);
        exited
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn watch_parent() {}

fn in_flight() -> std::sync::MutexGuard<'static, Vec<RequestId>> {
    IN_FLIGHT.lock().unwrap_or_else(|err| err.into_inner())
}

/// A tool call's line was read. rmcp starts the handler on a task of its
/// own, which may run after the reader has already seen the end of input,
/// so the call counts from here rather than from the handler.
pub fn call_read(id: &RequestId) {
    let mut calls = in_flight();
    if !calls.contains(id) {
        calls.push(id.clone());
    }
}

/// One tool call between the line being read and its answer being built.
pub struct InFlight(RequestId);

impl Drop for InFlight {
    fn drop(&mut self) {
        let mut calls = in_flight();
        if let Some(pos) = calls.iter().position(|id| *id == self.0) {
            calls.swap_remove(pos);
        }
        let settled = calls.is_empty();
        drop(calls);
        if settled {
            wake_reader();
        }
    }
}

/// Hold for the length of one tool call.
#[must_use]
pub fn call_in_flight(id: &RequestId) -> InFlight {
    call_read(id);
    InFlight(id.clone())
}

/// No tool call is read and unanswered.
pub fn settled() -> bool {
    in_flight().is_empty()
}

#[cfg(test)]
pub fn forget_calls() {
    in_flight().clear();
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
