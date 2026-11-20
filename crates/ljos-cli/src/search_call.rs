//! One call's own deadline for the start wait and the reads after it.
//!
//! A command is one process, but an MCP server serves many calls from the
//! same threads. A deadline left by an earlier call would give a later one
//! no start room once the server had been up that long. The outermost
//! [`SearchCall`] on a thread starts a fresh deadline when it begins and
//! clears it on drop.
//! A guard inside it shares the outer deadline. A read outside any guard
//! gets a whole [`call_deadline::timeout`] and keeps none of it.

use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::call_deadline;

thread_local! {
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// The deepest nesting this thread reached since [`forget_deepest`].
    #[cfg(test)]
    static DEEPEST: Cell<u32> = const { Cell::new(0) };
}

/// A call's deadline, held for as long as the guard lives.
///
/// The guard counts on the thread that made it, so it stays on that thread:
/// a guard dropped on another thread would lower that thread's count and
/// leave this one's deadline in place for every later call.
///
/// ```compile_fail,E0277
/// fn needs_send<T: Send>() {}
/// needs_send::<ljos_cli::SearchCall>();
/// ```
///
/// ```compile_fail,E0277
/// fn needs_sync<T: Sync>() {}
/// needs_sync::<ljos_cli::SearchCall>();
/// ```
pub struct SearchCall(PhantomData<Rc<()>>);

impl SearchCall {
    #[must_use]
    pub fn enter() -> Self {
        DEPTH.with(|depth| {
            if depth.get() == 0 {
                DEADLINE.with(|cell| cell.set(Some(Instant::now() + call_deadline::timeout())));
            }
            depth.set(depth.get() + 1);
            #[cfg(test)]
            DEEPEST.with(|deepest| deepest.set(deepest.get().max(depth.get())));
        });
        Self(PhantomData)
    }
}

impl Drop for SearchCall {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            let left = depth.get().saturating_sub(1);
            depth.set(left);
            if left == 0 {
                DEADLINE.with(|cell| cell.set(None));
            }
        });
    }
}

/// Time left on this call's deadline, counted from where the outermost
/// [`SearchCall`] began, so the reads before a search spend it too.
pub(crate) fn left() -> Duration {
    match DEADLINE.with(Cell::get) {
        Some(deadline) => deadline.saturating_duration_since(Instant::now()),
        None => call_deadline::timeout(),
    }
}

/// How deep the guards nested on this thread since [`forget_deepest`].
#[cfg(test)]
pub(crate) fn deepest() -> u32 {
    DEEPEST.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn forget_deepest() {
    DEEPEST.with(|deepest| deepest.set(0));
}

#[cfg(test)]
fn armed() -> bool {
    DEADLINE.with(|cell| cell.get().is_some())
}

#[cfg(test)]
fn depth() -> u32 {
    DEPTH.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm() {
        let _ = left();
    }

    #[test]
    fn a_nested_guard_shares_the_deadline_and_the_outer_one_clears_it() {
        let outer = SearchCall::enter();
        arm();
        assert!(armed());
        {
            let _inner = SearchCall::enter();
            assert_eq!(depth(), 2);
            assert!(armed(), "an inner guard keeps the outer deadline");
        }
        assert_eq!(depth(), 1);
        assert!(armed(), "an inner guard's end keeps the outer deadline");
        drop(outer);
        assert_eq!(depth(), 0);
        assert!(!armed(), "the outermost guard's end clears the deadline");
    }

    #[test]
    fn a_new_call_gets_a_fresh_deadline() {
        let first = SearchCall::enter();
        let at = DEADLINE.with(Cell::get).unwrap();
        drop(first);
        assert!(!armed(), "the ended call left no deadline");
        std::thread::sleep(Duration::from_millis(5));
        let _second = SearchCall::enter();
        assert!(
            DEADLINE.with(Cell::get).unwrap() > at,
            "the next call starts its own deadline"
        );
    }

    #[test]
    fn the_time_before_a_read_is_spent_from_the_deadline() {
        let _call = SearchCall::enter();
        std::thread::sleep(Duration::from_millis(60));
        assert!(
            left() + Duration::from_millis(50) <= call_deadline::timeout(),
            "{:?} left of {:?}",
            left(),
            call_deadline::timeout()
        );
    }

    #[test]
    fn a_panic_unwinds_the_guard() {
        let caught = std::panic::catch_unwind(|| {
            let _call = SearchCall::enter();
            arm();
            panic!("a call that panics");
        });
        assert!(caught.is_err());
        assert_eq!(depth(), 0, "the unwind lowered the count");
        assert!(!armed(), "the unwind cleared the deadline");
    }

    #[test]
    fn an_early_return_ends_the_call() {
        fn call(early: bool) -> Option<u32> {
            let _call = SearchCall::enter();
            arm();
            if early {
                return None;
            }
            Some(depth())
        }
        assert_eq!(call(true), None);
        assert_eq!(depth(), 0);
        assert!(!armed());
        assert_eq!(call(false), Some(1));
        assert_eq!(depth(), 0);
    }

    #[test]
    fn a_read_outside_a_call_keeps_no_deadline() {
        assert_eq!(depth(), 0);
        let whole = left();
        assert!(!armed(), "a read outside a guard arms nothing");
        assert!(whole >= Duration::from_secs(1));
    }

    #[test]
    fn a_guard_on_one_thread_leaves_another_thread_alone() {
        let _call = SearchCall::enter();
        arm();
        let other = std::thread::spawn(|| {
            let _call = SearchCall::enter();
            arm();
            (depth(), armed())
        })
        .join()
        .unwrap();
        assert_eq!(other, (1, true));
        assert_eq!(depth(), 1);
        assert!(armed());
    }
}
