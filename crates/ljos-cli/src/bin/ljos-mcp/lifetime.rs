//! When the server stops. The end of stdin is held back from rmcp until
//! the tool calls already read have answered: rmcp waits five seconds for
//! a handler after the input ends and then drops its answer, and a pack
//! write may take thirty.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::task::Waker;
use std::time::Duration;

/// How long the end of input waits for calls in flight. A pack request
/// gives up at 30 s unless `PACKSET_TIMEOUT_MS` says otherwise.
pub const SETTLE: Duration = Duration::from_secs(35);

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static READER: Mutex<Option<Waker>> = Mutex::new(None);

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
