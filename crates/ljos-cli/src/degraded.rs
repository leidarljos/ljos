//! The one place a read decides that memory is degraded and says so.
//!
//! `search`, `recall` and `sitting` read the pack writer. When it is
//! starting, busy or down they still answer from the words of what is on
//! this machine, and the command prints one line on stderr, whichever call
//! site met the failure first and however many times it retried. The line
//! is the same in every command and names `ljos doctor`.

use std::cell::Cell;

/// What the writer was doing when a read could not use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// It answered that it has not opened yet.
    Starting,
    /// It is up and shed the call, or did not answer in time.
    Busy,
    /// Nothing is listening, or it closed the connection.
    Down,
}

pub const STARTING_LINE: &str = "Memory is starting, so these results are ranked by words. If this persists, run `ljos doctor`.";
pub const BUSY_LINE: &str =
    "Memory is busy, so these results are ranked by words. If this persists, run `ljos doctor`.";
pub const DOWN_LINE: &str = "Memory is not running, so these results are ranked by words. If this persists, run `ljos doctor`.";

/// What a command with no word fallback prints when the writer is
/// starting: a dated search, or a write. It reads as [`STARTING_LINE`] does.
pub const STARTING_REFUSAL: &str =
    "Memory is starting, so try again in a few seconds. If this persists, run `ljos doctor`.";

impl State {
    /// The sentence for this state.
    #[must_use]
    pub const fn line(self) -> &'static str {
        match self {
            Self::Starting => STARTING_LINE,
            Self::Busy => BUSY_LINE,
            Self::Down => DOWN_LINE,
        }
    }

    /// The ranking reason a word fallback in this state carries.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Starting => crate::MEMORY_STARTING_REASON,
            Self::Busy => crate::MEMORY_SEARCH_BUSY_REASON,
            Self::Down => crate::WRITER_DOWN_REASON,
        }
    }

    /// The state a ranking reason names, when it names one.
    #[must_use]
    pub fn of_reason(reason: &str) -> Option<Self> {
        match reason {
            crate::MEMORY_STARTING_REASON => Some(Self::Starting),
            crate::MEMORY_SEARCH_BUSY_REASON | crate::WRITER_BUSY_REASON => Some(Self::Busy),
            crate::WRITER_DOWN_REASON => Some(Self::Down),
            _ => None,
        }
    }
}

/// The state behind a failed pack call, or `None` when the failure is not
/// the writer being unavailable: a refused token, a bad query, packs turned
/// off on purpose.
#[must_use]
pub fn of_error(err: &anyhow::Error) -> Option<State> {
    // A refused token names the command that replaces the writer. The
    // writer answered, so it is not a degraded one.
    if format!("{err:#}").contains(crate::WRITER_PORT_FIX) {
        return None;
    }
    if crate::is_memory_starting(err) {
        return Some(State::Starting);
    }
    // The writer answered "busy" while it loads, on a route that does not
    // say it is starting. Its `/health` does, so ask it.
    if (crate::is_read_shed(err) || crate::is_reported_busy(err) || crate::is_pack_busy(err))
        && crate::health_still_starting() == Some(true)
    {
        return Some(State::Starting);
    }
    if crate::is_read_shed(err)
        || crate::is_reported_busy(err)
        || crate::is_pack_busy(err)
        || crate::is_deadline_miss(err)
    {
        return Some(State::Busy);
    }
    if crate::pack_turned_off(err) {
        return None;
    }
    let closed = format!("{err:#}")
        .to_ascii_lowercase()
        .contains("closed the socket");
    if crate::pack_unreachable(err) || closed || crate::pack_down_message(err).is_some() {
        return Some(State::Down);
    }
    crate::pack_memory_down(err).then_some(State::Busy)
}

thread_local! {
    static SAID: Cell<bool> = const { Cell::new(false) };
}

/// Print the line for `state` on stderr, unless this command already said
/// one. The first state wins. Returns whether it printed.
pub fn say(state: State) -> bool {
    if SAID.with(|said| said.replace(true)) {
        return false;
    }
    say!("{}", state.line());
    true
}

/// Whether this command has already said a degraded line.
#[must_use]
pub fn said() -> bool {
    SAID.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn forget() {
    SAID.with(|said| said.set(false));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_says_one_line_whatever_the_state() {
        forget();
        assert!(!said());
        assert!(say(State::Starting));
        assert!(!say(State::Starting));
        assert!(!say(State::Busy));
        assert!(!say(State::Down));
        assert!(said());
        forget();
    }

    #[test]
    fn every_line_names_the_doctor_and_none_runs_over_a_line() {
        for state in [State::Starting, State::Busy, State::Down] {
            let line = state.line();
            assert!(line.contains("run `ljos doctor`"), "{line}");
            assert!(!line.contains('\n'), "{line}");
            assert_eq!(State::of_reason(state.reason()), Some(state));
        }
    }

    #[test]
    fn a_refused_token_is_not_a_degraded_writer() {
        let err = anyhow::anyhow!("the writer refused this seat's token");
        assert_eq!(of_error(&err), None);
    }
}
