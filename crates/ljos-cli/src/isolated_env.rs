//! PATH, HOME, and `XDG_*` for the calling thread.
//!
//! A test that needs a fake binary or a private home records the value here
//! and the lookup functions read it. The process environment is not edited,
//! so another thread's `which("cat")` still finds cat. Nothing here takes a
//! lock. With nothing recorded, a lookup is the process environment.
//!
//! Each record is its own frame. Dropping an older frame leaves a newer one
//! in place, so a guard that outlives the one it replaced does not leak the
//! old value onto the next test on this thread.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::Command;

struct Frame {
    id: u64,
    value: Option<OsString>,
}

thread_local! {
    static OVERLAY: RefCell<BTreeMap<OsString, Vec<Frame>>> =
        const { RefCell::new(BTreeMap::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

/// `PATH`, `HOME`, and any `XDG_*` key. Other variables stay on the process.
#[must_use]
pub fn is_isolated(key: &OsStr) -> bool {
    let Some(text) = key.to_str() else {
        return false;
    };
    text == "PATH" || text == "HOME" || text.starts_with("XDG_")
}

/// Puts this frame away when dropped. An older frame dropping first leaves
/// a newer record for the same key.
#[must_use = "the recorded environment lasts until this guard drops"]
pub struct Guard {
    key: OsString,
    id: u64,
}

impl Guard {
    /// Record a new value without moving the restore point.
    pub fn replace(&mut self, value: impl AsRef<OsStr>) {
        let value = Some(value.as_ref().to_os_string());
        let id = self.id;
        OVERLAY.with(|slot| {
            let mut map = slot.borrow_mut();
            let Some(frames) = map.get_mut(&self.key) else {
                return;
            };
            if let Some(frame) = frames.iter_mut().find(|frame| frame.id == id) {
                frame.value = value;
            }
        });
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.id == 0 {
            return;
        }
        OVERLAY.with(|slot| {
            let mut map = slot.borrow_mut();
            let Some(frames) = map.get_mut(&self.key) else {
                return;
            };
            frames.retain(|frame| frame.id != self.id);
            if frames.is_empty() {
                map.remove(&self.key);
            }
        });
    }
}

fn push(key: &OsStr, value: Option<OsString>) -> Guard {
    assert!(
        is_isolated(key),
        "only PATH, HOME, and XDG_* are recorded on the calling thread"
    );
    let id = NEXT_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });
    let owned = key.to_os_string();
    OVERLAY.with(|slot| {
        slot.borrow_mut()
            .entry(owned.clone())
            .or_default()
            .push(Frame { id, value });
    });
    Guard { key: owned, id }
}

/// Record `value` for `key` until the guard drops.
pub fn set(key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Guard {
    push(key.as_ref(), Some(value.as_ref().to_os_string()))
}

/// Record `key` as unset until the guard drops.
pub fn unset(key: impl AsRef<OsStr>) -> Guard {
    push(key.as_ref(), None)
}

/// Remember the current record and put it back when the guard drops.
/// The value is left as it is, so a later [`set`] can stack on top.
pub fn hold(key: impl AsRef<OsStr>) -> Guard {
    let key = key.as_ref();
    assert!(
        is_isolated(key),
        "only PATH, HOME, and XDG_* are recorded on the calling thread"
    );
    match top(key) {
        Some(value) => push(key, value),
        None => Guard {
            key: key.to_os_string(),
            id: 0,
        },
    }
}

/// `None` when this thread has not recorded `key`.
/// `Some(None)` when it removed `key`. `Some(Some(value))` when it set one.
fn top(key: &OsStr) -> Option<Option<OsString>> {
    if !is_isolated(key) {
        return None;
    }
    OVERLAY.with(|slot| {
        slot.borrow()
            .get(key)
            .and_then(|frames| frames.last().map(|frame| frame.value.clone()))
    })
}

/// `key` from this thread's record, otherwise the process environment.
pub(crate) fn env_os(key: impl AsRef<OsStr>) -> Option<OsString> {
    let key = key.as_ref();
    if let Some(over) = top(key) {
        return over;
    }
    std::env::var_os(key)
}

/// [`env_os`] as a Unicode string, with the same errors as [`std::env::var`].
pub(crate) fn env_var(key: impl AsRef<OsStr>) -> Result<String, std::env::VarError> {
    match env_os(key) {
        None => Err(std::env::VarError::NotPresent),
        Some(value) => value.into_string().map_err(std::env::VarError::NotUnicode),
    }
}

/// Resolve `bin` on this thread's `PATH` when one is recorded, otherwise
/// with [`which::which`].
pub(crate) fn which_bin(bin: &str) -> Result<PathBuf, which::Error> {
    if let Some(over) = top(OsStr::new("PATH")) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        return which::which_in(bin, Some(over.unwrap_or_default()), cwd);
    }
    which::which(bin)
}

/// Put a process variable back. `PATH`, `HOME`, and `XDG_*` are left
/// alone: a test records those on the calling thread, and writing them
/// here would race another test's `which("cat")`.
pub fn restore_process<V: AsRef<OsStr>>(key: impl AsRef<OsStr>, value: Option<V>) {
    let key = key.as_ref();
    if is_isolated(key) {
        return;
    }
    match &value {
        Some(value) => unsafe { std::env::set_var(key, value) },
        None => unsafe { std::env::remove_var(key) },
    }
}

/// Remove a process variable. Isolated keys are left as they are.
pub fn remove_process(key: impl AsRef<OsStr>) {
    restore_process(key, None::<&OsStr>);
}

/// Copy this thread's recorded PATH, HOME, and `XDG_*` onto `cmd`.
pub(crate) fn apply(cmd: &mut Command) {
    OVERLAY.with(|slot| {
        for (key, frames) in slot.borrow().iter() {
            let Some(frame) = frames.last() else {
                continue;
            };
            match &frame.value {
                Some(value) => {
                    cmd.env(key, value);
                }
                None => {
                    cmd.env_remove(key);
                }
            }
        }
    });
}

/// `dir` in front of this thread's PATH, kept until the guard drops.
#[cfg(test)]
pub(crate) fn prepend_path(dir: &std::path::Path) -> Guard {
    let mut next = OsString::from(dir);
    next.push(":");
    next.push(env_os("PATH").unwrap_or_default());
    set("PATH", next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_home_drops_without_hiding_the_newer_one() {
        let outer = set("HOME", "/tmp/ljos-outer-home");
        let inner = set("HOME", "/tmp/ljos-inner-home");
        assert_eq!(
            env_os("HOME").as_deref(),
            Some(OsStr::new("/tmp/ljos-inner-home"))
        );
        drop(outer);
        assert_eq!(
            env_os("HOME").as_deref(),
            Some(OsStr::new("/tmp/ljos-inner-home"))
        );
        drop(inner);
        let home = env_os("HOME").unwrap_or_default();
        assert!(!home.to_string_lossy().contains("ljos-outer-home"));
        assert!(!home.to_string_lossy().contains("ljos-inner-home"));
    }
}
