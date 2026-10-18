//! Process environment.
//!
//! `set_var` and a concurrent `getenv` are a data race. Outside tests every
//! read and write goes through one lock. A test records `XDG_*` and the
//! activation token on the calling thread, so another test's read cannot
//! observe them and the process environment stays as the suite started it.

use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard};

thread_local! {
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn mutex() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

fn with<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(not(test))]
    let _hold = hold();
    f()
}

/// The environment lock, held until dropped.
#[must_use = "the environment lock releases when this guard drops"]
pub(crate) struct Hold {
    _guard: Option<MutexGuard<'static, ()>>,
    tracked: bool,
}

impl Drop for Hold {
    fn drop(&mut self) {
        if self.tracked {
            DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        }
    }
}

pub(crate) fn hold() -> Hold {
    if DEPTH.with(|depth| depth.get()) > 0 {
        DEPTH.with(|depth| depth.set(depth.get() + 1));
        return Hold {
            _guard: None,
            tracked: true,
        };
    }
    let guard = mutex().lock().unwrap_or_else(|err| err.into_inner());
    DEPTH.with(|depth| depth.set(1));
    Hold {
        _guard: Some(guard),
        tracked: true,
    }
}

pub(crate) fn var<K: AsRef<OsStr>>(key: K) -> Result<String, std::env::VarError> {
    let key = key.as_ref();
    #[cfg(test)]
    if let Some(over) = overlay_get(key) {
        return match over {
            None => Err(std::env::VarError::NotPresent),
            Some(value) => value.into_string().map_err(std::env::VarError::NotUnicode),
        };
    }
    let key = key.to_owned();
    with(|| std::env::var(key))
}

pub(crate) fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
    let key = key.as_ref();
    #[cfg(test)]
    if let Some(over) = overlay_get(key) {
        return over;
    }
    let key = key.to_owned();
    with(|| std::env::var_os(key))
}

#[cfg(test)]
pub(crate) fn vars_os() -> std::vec::IntoIter<(OsString, OsString)> {
    let mut map: std::collections::BTreeMap<OsString, OsString> =
        with(|| std::env::vars_os().collect());
    OVERLAY.with(|slot| {
        for (key, value) in slot.borrow().iter() {
            match value {
                Some(value) => {
                    map.insert(key.clone(), value.clone());
                }
                None => {
                    map.remove(key);
                }
            }
        }
    });
    map.into_iter().collect::<Vec<_>>().into_iter()
}

pub(crate) fn set_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    let key = key.as_ref().to_owned();
    let value = value.as_ref().to_owned();
    #[cfg(test)]
    {
        OVERLAY.with(|slot| {
            slot.borrow_mut().insert(key, Some(value));
        });
    }
    #[cfg(not(test))]
    {
        with(|| unsafe { std::env::set_var(key, value) });
    }
}

pub(crate) fn remove_var<K: AsRef<OsStr>>(key: K) {
    let key = key.as_ref().to_owned();
    #[cfg(test)]
    {
        OVERLAY.with(|slot| {
            slot.borrow_mut().insert(key, None);
        });
    }
    #[cfg(not(test))]
    {
        with(|| unsafe { std::env::remove_var(key) });
    }
}

#[cfg(test)]
thread_local! {
    static OVERLAY: std::cell::RefCell<std::collections::BTreeMap<OsString, Option<OsString>>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

#[cfg(test)]
fn overlay_get(key: &OsStr) -> Option<Option<OsString>> {
    OVERLAY.with(|slot| slot.borrow().get(key).cloned())
}
