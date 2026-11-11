//! The bound on one MCP tool call.
//!
//! A pack socket, a git command, a habitat child and an advisory lock stop
//! when this runs out, and the line names the one that did. The default is
//! 30 seconds. `LJOS_MCP_TIMEOUT_MS` is milliseconds. `LJOS_MCP_TIMEOUT` is
//! whole seconds and is used when the millisecond form is unset.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The bound when neither environment variable is set.
pub const DEFAULT: Duration = Duration::from_secs(30);

struct State {
    deadline: Instant,
    stage: Mutex<String>,
    failed: Mutex<Option<String>>,
}

tokio::task_local! {
    static CALL: Arc<State>;
}

thread_local! {
    static THREAD: std::cell::RefCell<Option<Arc<State>>> =
        const { std::cell::RefCell::new(None) };
}

/// Restores the bound the thread had before [`Ticket::bind`].
pub struct ThreadGuard {
    previous: Option<Arc<State>>,
}

impl Drop for ThreadGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        THREAD.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// The bound captured on the calling task, for a `spawn_blocking` thread.
pub struct Ticket(Arc<State>);

impl Ticket {
    /// Make this thread see the call's bound until the guard drops.
    #[must_use]
    pub fn bind(self) -> ThreadGuard {
        bind_thread(self.0)
    }
}

fn bind_thread(state: Arc<State>) -> ThreadGuard {
    let previous = THREAD.with(|slot| slot.replace(Some(state)));
    ThreadGuard { previous }
}

fn current() -> Option<Arc<State>> {
    if let Some(state) = THREAD.with(|slot| slot.borrow().clone()) {
        return Some(state);
    }
    CALL.try_with(Arc::clone).ok()
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How long a tool call may run.
#[must_use]
pub fn budget() -> Duration {
    if let Some(ms) = positive("LJOS_MCP_TIMEOUT_MS") {
        return Duration::from_millis(ms);
    }
    if let Some(secs) = positive("LJOS_MCP_TIMEOUT") {
        return Duration::from_secs(secs);
    }
    DEFAULT
}

fn positive(key: &str) -> Option<u64> {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|n| *n > 0)
}

/// Whether this task or thread is inside a tool call's bound.
#[must_use]
pub fn armed() -> bool {
    current().is_some()
}

/// When the call must be done. `None` outside a tool call.
#[must_use]
pub fn deadline_instant() -> Option<Instant> {
    current().map(|state| state.deadline)
}

/// Time left in the call. `None` outside a tool call.
#[must_use]
pub fn remaining() -> Option<Duration> {
    deadline_instant().map(|deadline| deadline.saturating_duration_since(Instant::now()))
}

/// The bound on this task, so a blocking thread can take it with it.
#[must_use]
pub fn ticket() -> Option<Ticket> {
    current().map(Ticket)
}

/// Name the wait in progress. A timeout line uses the latest name.
pub fn note(what: &str) {
    if let Some(state) = current() {
        *locked(&state.stage) = what.to_string();
    }
}

/// Name `what` until the guard drops, unless the bound has already run out.
pub fn stage(what: &str) -> StageGuard {
    let previous = current().map(|state| locked(&state.stage).clone());
    note(what);
    StageGuard { previous }
}

/// Puts the previous stage back. A spent bound keeps the name it timed out on.
pub struct StageGuard {
    previous: Option<String>,
}

impl Drop for StageGuard {
    fn drop(&mut self) {
        if remaining().is_some_and(|left| left.is_zero()) {
            return;
        }
        if let Some(previous) = self.previous.take() {
            note(&previous);
        }
    }
}

/// Remember the first timeout line for this call.
pub fn mark_failed(line: impl Into<String>) {
    let line = line.into();
    if let Some(state) = current() {
        let mut slot = locked(&state.failed);
        if slot.is_none() {
            *slot = Some(line);
        }
    }
}

/// The timeout line a wait already recorded, if one has.
#[must_use]
pub fn failure() -> Option<String> {
    current().and_then(|state| locked(&state.failed).clone())
}

/// One line naming the wait that was in progress.
#[must_use]
pub fn timeout_line() -> String {
    let what = current()
        .map(|state| locked(&state.stage).clone())
        .filter(|what| !what.is_empty())
        .unwrap_or_else(|| "tool".to_string());
    format!("{what} timed out")
}

/// Run `fut` inside a fresh bound named `tool`.
pub async fn scope<F: std::future::Future>(tool: &str, fut: F) -> F::Output {
    let state = Arc::new(State {
        deadline: Instant::now() + budget(),
        stage: Mutex::new(tool.to_string()),
        failed: Mutex::new(None),
    });
    CALL.scope(state, fut).await
}

/// `spawn_blocking` that keeps this call's bound on the worker.
pub async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, tokio::task::JoinError> {
    let ticket = ticket();
    tokio::task::spawn_blocking(move || {
        let _guard = ticket.map(Ticket::bind);
        f()
    })
    .await
}

/// Arm a bound on this thread. Tests use this. The guard clears it.
#[cfg(test)]
#[must_use]
pub(crate) fn arm(total: Duration) -> ThreadGuard {
    bind_thread(Arc::new(State {
        deadline: Instant::now() + total,
        stage: Mutex::new(String::new()),
        failed: Mutex::new(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_budget_is_thirty_seconds_unless_configured() {
        let _g = crate::tests::env_guard();
        unsafe {
            std::env::remove_var("LJOS_MCP_TIMEOUT");
            std::env::remove_var("LJOS_MCP_TIMEOUT_MS");
        }
        assert_eq!(budget(), Duration::from_secs(30));
        unsafe { std::env::set_var("LJOS_MCP_TIMEOUT", "12") };
        assert_eq!(budget(), Duration::from_secs(12));
        unsafe { std::env::set_var("LJOS_MCP_TIMEOUT_MS", "250") };
        assert_eq!(budget(), Duration::from_millis(250));
    }

    /// A writer that accepts and then says nothing used to hold the call.
    #[test]
    fn a_silent_packsetd_stops_inside_the_tool_budget() {
        let _g = crate::tests::env_guard();
        let home = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                std::thread::sleep(Duration::from_secs(30));
            }
        });
        unsafe {
            std::env::set_var("HOME", home.path());
            std::env::set_var("PACKSET_URL", format!("http://{addr}"));
            std::env::remove_var("PACKSET_TOKEN");
            std::env::remove_var("PACKSET_TOKEN_FILE");
            std::env::remove_var("PACKSET_TIMEOUT_MS");
            std::env::remove_var("LJOS_IN_HOOK");
        }
        let _bound = arm(Duration::from_millis(400));
        let started = Instant::now();
        let err = crate::pack_link::PacksetClient::from_env()
            .expect("url")
            .status(None)
            .expect_err("a silent writer answers");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "waited {:?}",
            started.elapsed()
        );
        let text = err.to_string();
        assert!(text.contains("packsetd timed out"), "{text}");
    }

    /// A habitat child that never exits, and a git-shaped child, both stop.
    #[test]
    fn a_hung_subprocess_stops_inside_the_tool_budget() {
        let _g = crate::tests::env_guard();
        let _bound = arm(Duration::from_millis(400));
        let started = Instant::now();
        let err = crate::run_captured("sh", &["-c", "exec sleep 30"]).unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "habitat child waited {:?}",
            started.elapsed()
        );
        assert!(err.to_string().contains("sh timed out"), "{err}");

        let _bound = arm(Duration::from_millis(400));
        let started = Instant::now();
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", "exec sleep 30"]);
        let err = crate::bounded_output(&mut cmd).unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "git-shaped child waited {:?}",
            started.elapsed()
        );
        assert!(err.to_string().contains("sh timed out"), "{err}");
    }

    /// An exclusive lock another holder keeps is given up at the bound.
    #[test]
    fn a_held_lock_stops_inside_the_tool_budget() {
        let _g = crate::tests::env_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("held.lock");
        let held = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        {
            use std::os::unix::io::AsRawFd;
            assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
        }
        let _bound = arm(Duration::from_millis(300));
        let started = Instant::now();
        let _lock = crate::CommitLock::acquire(&path);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "lock waited {:?}",
            started.elapsed()
        );
        assert_eq!(failure().as_deref(), Some("lock timed out"));
    }
}
