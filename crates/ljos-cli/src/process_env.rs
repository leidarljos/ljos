//! Process environment.
//!
//! `std::env::set_var` and a concurrent `getenv` are a data race: a lookup
//! can observe a torn or empty `PATH` and report that `cat` is missing.
//! Outside tests every read and write in this process goes through one lock.
//! A nested call on the thread that already holds the lock does not take it
//! again.
//!
//! A test does not edit the process. It records the change on the calling
//! thread, and [`command`] copies that record onto the child. The process
//! `PATH` stays the one the suite started with, so another thread's
//! `which("cat")` still finds it. A test that needs a fake `vissue` or
//! `deedar` prepends that directory on the calling thread. The child is
//! started from the absolute path [`which`] returned.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

thread_local! {
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// pthread id of the holder this helper is allowed to read under, or 0.
    static HELPER_OF: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// pthread id of the thread that owns [`Hold`], or 0 when nobody does.
static HOLDER: AtomicU64 = AtomicU64::new(0);

fn tid() -> u64 {
    // SAFETY: `pthread_self` returns this thread's id and has no
    // preconditions. The value is stored only so a helper can see that the
    // thread which spawned it still owns the environment lock.
    unsafe { libc::pthread_self() as u64 }
}

fn mutex() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

fn with<R>(f: impl FnOnce() -> R) -> R {
    // Tests never edit the process environment, so a read does not take
    // the lock. Taking it here deadlocks a helper whose parent still holds
    // the lock across the join.
    #[cfg(not(test))]
    let _hold = hold();
    f()
}

/// The environment lock, held until dropped.
///
/// The outermost guard owns the mutex. A nested guard on the same thread
/// only counts the depth, so a read inside a caller that already holds the
/// lock does not deadlock.
#[must_use = "the environment lock releases when this guard drops"]
pub struct Hold {
    _guard: Option<MutexGuard<'static, ()>>,
    tracked: bool,
}

impl Drop for Hold {
    fn drop(&mut self) {
        if self.tracked {
            let left = DEPTH.with(|depth| {
                let next = depth.get().saturating_sub(1);
                depth.set(next);
                next
            });
            if left == 0 {
                HOLDER.store(0, Ordering::Release);
            }
        }
    }
}

fn parent_still_holds() -> bool {
    let of = HELPER_OF.with(|slot| slot.get());
    of != 0 && of == HOLDER.load(Ordering::Acquire)
}

/// Lock the process environment until the returned guard drops.
pub fn hold() -> Hold {
    if parent_still_holds() {
        return Hold {
            _guard: None,
            tracked: false,
        };
    }
    if DEPTH.with(|depth| depth.get()) > 0 {
        DEPTH.with(|depth| depth.set(depth.get() + 1));
        return Hold {
            _guard: None,
            tracked: true,
        };
    }
    let guard = mutex().lock().unwrap_or_else(|err| err.into_inner());
    HOLDER.store(tid(), Ordering::Release);
    DEPTH.with(|depth| depth.set(1));
    Hold {
        _guard: Some(guard),
        tracked: true,
    }
}

/// The pthread id of the holder on this thread, or 0 when this thread
/// does not hold the environment lock.
#[must_use]
pub fn current_holder() -> u64 {
    if DEPTH.with(|depth| depth.get()) == 0 {
        0
    } else {
        HOLDER.load(Ordering::Acquire)
    }
}

/// Run `f` on a helper thread spawned by `holder`.
///
/// While that thread still owns the environment lock, `f` reads without
/// taking it. Once the holder has released the lock, reads take it as usual.
pub fn helper<R>(holder: u64, f: impl FnOnce() -> R) -> R {
    let prev = HELPER_OF.with(|slot| slot.replace(holder));
    struct Reset(u64);
    impl Drop for Reset {
        fn drop(&mut self) {
            HELPER_OF.with(|slot| slot.set(self.0));
        }
    }
    let _reset = Reset(prev);
    f()
}

/// The calling thread's environment edits, captured so a helper can see them.
///
/// A helper does not inherit the caller's thread-local edits. Pass the
/// snapshot into [`bind_helper`] or [`with_overlay`].
#[cfg(test)]
#[derive(Clone)]
pub struct Overlay {
    map: std::collections::BTreeMap<OsString, Option<OsString>>,
}

/// No per-thread edits outside tests. The process environment is the record.
#[cfg(not(test))]
#[derive(Clone)]
pub struct Overlay;

/// Capture this thread's environment edits for a helper.
#[must_use]
pub fn overlay_snapshot() -> Overlay {
    #[cfg(test)]
    {
        Overlay {
            map: OVERLAY.with(|slot| slot.borrow().clone()),
        }
    }
    #[cfg(not(test))]
    {
        Overlay
    }
}

/// Run `f` with `overlay` installed on this thread, then put the previous
/// edits back.
pub fn with_overlay<R>(overlay: Overlay, f: impl FnOnce() -> R) -> R {
    #[cfg(test)]
    {
        let prev = OVERLAY.with(|slot| slot.replace(overlay.map));
        reconcile_vissue(&prev, &current_overlay());
        struct Reset(std::collections::BTreeMap<OsString, Option<OsString>>);
        impl Drop for Reset {
            fn drop(&mut self) {
                let installed = OVERLAY.with(|slot| slot.replace(std::mem::take(&mut self.0)));
                reconcile_vissue(&installed, &current_overlay());
            }
        }
        let _reset = Reset(prev);
        f()
    }
    #[cfg(not(test))]
    {
        let _ = overlay;
        f()
    }
}

/// [`helper`] on a thread that also sees `overlay`.
pub fn bind_helper<R>(holder: u64, overlay: Overlay, f: impl FnOnce() -> R) -> R {
    with_overlay(overlay, || helper(holder, f))
}

/// `$key`, or an error when it is unset or not Unicode.
pub fn var<K: AsRef<OsStr>>(key: K) -> Result<String, std::env::VarError> {
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

/// `$key` as an [`OsString`], when it is set.
pub fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
    let key = key.as_ref();
    #[cfg(test)]
    if let Some(over) = overlay_get(key) {
        return over;
    }
    let key = key.to_owned();
    with(|| std::env::var_os(key))
}

/// Every variable visible on this thread, copied under the lock outside tests.
pub fn vars() -> std::vec::IntoIter<(String, String)> {
    merged()
        .into_iter()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect::<Vec<_>>()
        .into_iter()
}

/// Every variable visible on this thread, copied under the lock outside tests.
pub fn vars_os() -> std::vec::IntoIter<(OsString, OsString)> {
    merged().into_iter()
}

fn merged() -> Vec<(OsString, OsString)> {
    #[cfg(test)]
    {
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
        map.into_iter().collect()
    }
    #[cfg(not(test))]
    {
        with(|| std::env::vars_os().collect())
    }
}

/// Set `$key` to `value`.
///
/// Outside tests the value is process-wide and the lock is the one every
/// read takes. A test records it on this thread only. A child sees it when
/// it is started with [`command`].
pub fn set_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    let key = key.as_ref().to_owned();
    let value = value.as_ref().to_owned();
    #[cfg(test)]
    {
        mirror_vissue(&key, Some(&value));
        OVERLAY.with(|slot| {
            slot.borrow_mut().insert(key, Some(value));
        });
    }
    #[cfg(not(test))]
    {
        // The lock above is what makes the process-wide edit exclusive with
        // the reads in this crate.
        with(|| unsafe { std::env::set_var(key, value) });
    }
}

/// Remove `$key`.
pub fn remove_var<K: AsRef<OsStr>>(key: K) {
    let key = key.as_ref().to_owned();
    #[cfg(test)]
    {
        mirror_vissue(&key, None);
        OVERLAY.with(|slot| {
            slot.borrow_mut().insert(key, None);
        });
    }
    #[cfg(not(test))]
    {
        with(|| unsafe { std::env::remove_var(key) });
    }
}

/// The temporary directory visible on this thread.
pub fn temp_dir() -> PathBuf {
    #[cfg(test)]
    {
        for key in ["TMPDIR", "TMP", "TEMP"] {
            if let Some(Some(value)) = overlay_get(OsStr::new(key)) {
                if !value.is_empty() {
                    return PathBuf::from(value);
                }
            }
        }
    }
    with(std::env::temp_dir)
}

/// A child process that sees this thread's environment edits.
///
/// Outside tests this is [`std::process::Command::new`]: the process
/// environment is already the record. A test copies its thread-local edits
/// onto the command, so the child sees `HOME` or `VISSUE_ROOT` without
/// those values being written into the process.
#[must_use]
pub fn command(program: impl AsRef<OsStr>) -> std::process::Command {
    #[cfg(test)]
    {
        let program = program.as_ref();
        let mut cmd = std::process::Command::new(program);
        // A host gitconfig that signs commits or arms fsmonitor holds
        // `git commit` past the push wait, and the commit then looks stuck.
        if std::path::Path::new(program)
            .file_name()
            .is_some_and(|name| name == "git")
        {
            cmd.args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.untrackedCache=false",
            ]);
        }
        apply_overlay(&mut cmd);
        cmd
    }
    #[cfg(not(test))]
    {
        std::process::Command::new(program)
    }
}

#[cfg(test)]
fn apply_overlay(cmd: &mut std::process::Command) {
    OVERLAY.with(|slot| {
        for (key, value) in slot.borrow().iter() {
            match value {
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

/// `binary_name` on the calling thread's path front, or on the `PATH`
/// this thread sees.
pub fn which<T: AsRef<OsStr>>(binary_name: T) -> Result<PathBuf, which::Error> {
    let name = binary_name.as_ref();
    if let Some(found) = scoped(name) {
        return found;
    }
    #[cfg(test)]
    if !has_separator(name) {
        if let Some(over) = overlay_get(OsStr::new("PATH")) {
            return match over {
                None => Err(which::Error::CannotFindBinaryPath),
                Some(path) => which_in_path(&path, name),
            };
        }
    }
    let owned = name.to_owned();
    with(|| which::which(owned))
}

fn has_separator(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.contains('/') || name.contains('\\')
}

fn scoped(name: &OsStr) -> Option<Result<PathBuf, which::Error>> {
    if has_separator(name) {
        return None;
    }
    #[cfg(test)]
    {
        let name = name.to_str()?;
        if is_hidden(name) {
            return Some(Err(which::Error::CannotFindBinaryPath));
        }
        if let Some(path) = front_hit(name) {
            return Some(Ok(path));
        }
    }
    None
}

#[cfg(test)]
thread_local! {
    static OVERLAY: std::cell::RefCell<std::collections::BTreeMap<OsString, Option<OsString>>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    static FRONT: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
    static HIDDEN: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn overlay_get(key: &OsStr) -> Option<Option<OsString>> {
    OVERLAY.with(|slot| slot.borrow().get(key).cloned())
}

#[cfg(test)]
fn current_overlay() -> std::collections::BTreeMap<OsString, Option<OsString>> {
    OVERLAY.with(|slot| slot.borrow().clone())
}

/// The tracker crate reads its own thread-local overlay, not ours. Keep
/// the two in step so `VISSUE_NO_ROUTE` reaches it without `setenv`.
#[cfg(test)]
fn mirror_vissue(key: &OsStr, value: Option<&OsStr>) {
    let Some(key) = key.to_str() else {
        return;
    };
    match value {
        Some(value) => {
            if let Some(value) = value.to_str() {
                vissue_core::process_env::override_var(key, Some(value));
            }
        }
        None => vissue_core::process_env::override_var(key, None),
    }
}

#[cfg(test)]
fn reconcile_vissue(
    before: &std::collections::BTreeMap<OsString, Option<OsString>>,
    after: &std::collections::BTreeMap<OsString, Option<OsString>>,
) {
    for key in before.keys() {
        if !after.contains_key(key) {
            if let Some(key) = key.to_str() {
                vissue_core::process_env::clear_override(key);
            }
        }
    }
    for (key, value) in after {
        mirror_vissue(key, value.as_deref());
    }
}

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
fn is_hidden(name: &str) -> bool {
    HIDDEN.with(|slot| slot.borrow().iter().any(|hidden| hidden == name))
}

#[cfg(test)]
fn front_hit(name: &str) -> Option<PathBuf> {
    FRONT.with(|slot| {
        slot.borrow().iter().rev().find_map(|dir| {
            let path = dir.join(name);
            is_exec(&path).then_some(path)
        })
    })
}

#[cfg(test)]
fn which_in_path(path: &OsStr, name: &OsStr) -> Result<PathBuf, which::Error> {
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(name);
        if is_exec(&candidate) {
            return Ok(candidate);
        }
    }
    Err(which::Error::CannotFindBinaryPath)
}

#[cfg(test)]
fn is_exec(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Directories searched before `PATH`, on this thread only.
/// Dropping the guard removes the directory it prepended.
#[cfg(test)]
#[must_use = "the directory leaves the front when this guard drops"]
pub struct PathFront {
    dir: PathBuf,
}

#[cfg(test)]
impl PathFront {
    /// Search `dir` before `PATH` until this guard drops.
    pub fn prepend(dir: &Path) -> Self {
        FRONT.with(|slot| slot.borrow_mut().push(dir.to_path_buf()));
        Self {
            dir: dir.to_path_buf(),
        }
    }
}

#[cfg(test)]
impl Drop for PathFront {
    fn drop(&mut self) {
        FRONT.with(|slot| {
            let mut front = slot.borrow_mut();
            if let Some(index) = front.iter().rposition(|dir| dir == &self.dir) {
                front.remove(index);
            }
        });
    }
}

/// Binary names [`which`] refuses on this thread, whatever `PATH` says.
#[cfg(test)]
#[must_use = "the names are visible again when this guard drops"]
pub struct HiddenBins {
    names: Vec<String>,
}

#[cfg(test)]
impl HiddenBins {
    /// Hide `names` from [`which`] until this guard drops.
    pub fn new(names: &[&str]) -> Self {
        let names: Vec<String> = names.iter().map(|name| (*name).to_string()).collect();
        HIDDEN.with(|slot| slot.borrow_mut().extend(names.iter().cloned()));
        Self { names }
    }
}

#[cfg(test)]
impl Drop for HiddenBins {
    fn drop(&mut self) {
        HIDDEN.with(|slot| {
            let mut hidden = slot.borrow_mut();
            for name in &self.names {
                if let Some(index) = hidden.iter().rposition(|have| have == name) {
                    hidden.remove(index);
                }
            }
        });
    }
}
