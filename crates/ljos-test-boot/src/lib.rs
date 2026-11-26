//! Move a test process out of a host runner cgroup, and drop runner
//! variables, before `main`. The object is linked only into crates that
//! name this one. `ljos` does not.
//!
//! [`Packsetd`] starts the pack writer for a test and stops it again.

mod packsetd;

pub use packsetd::{reap_listener, Builder, Packsetd};

use std::ffi::c_char;

static PROCESS_STATE: std::sync::RwLock<()> = std::sync::RwLock::new(());
type ReadLock = std::sync::RwLockReadGuard<'static, ()>;
type ReadSlot = std::cell::RefCell<Option<ReadLock>>;
type ReadHandle = std::rc::Rc<ReadSlot>;
type WeakReadHandle = std::rc::Weak<ReadSlot>;

/// Shared access to process-global state for a test that only reads it.
#[must_use = "hold the guard for every process-global read"]
pub struct EnvReadGuard {
    held: ReadHandle,
}

/// Exclusive access to process-global state for a test that changes it.
#[must_use = "hold the guard until process-global state is restored"]
pub struct EnvWriteGuard {
    held: Option<std::sync::RwLockWriteGuard<'static, ()>>,
    restore: Option<WeakReadHandle>,
}

thread_local! {
    static TEST_READER: std::cell::RefCell<Option<WeakReadHandle>> =
        const { std::cell::RefCell::new(None) };
}

/// Hold process environment, current directory and other process-global
/// paths still while a test reads them.
pub fn env_read_lock() -> EnvReadGuard {
    let held = std::rc::Rc::new(std::cell::RefCell::new(Some(
        PROCESS_STATE
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )));
    TEST_READER.with(|slot| *slot.borrow_mut() = Some(std::rc::Rc::downgrade(&held)));
    EnvReadGuard { held }
}

/// Exclude every process-global reader while a test changes process state.
pub fn env_write_lock() -> EnvWriteGuard {
    let restore = TEST_READER.with(|slot| slot.borrow().clone());
    if let Some(held) = restore.as_ref().and_then(std::rc::Weak::upgrade) {
        drop(held.borrow_mut().take());
    }
    EnvWriteGuard {
        held: Some(
            PROCESS_STATE
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        ),
        restore,
    }
}

impl Drop for EnvReadGuard {
    fn drop(&mut self) {
        TEST_READER.with(|slot| {
            let ours = slot
                .borrow()
                .as_ref()
                .and_then(std::rc::Weak::upgrade)
                .is_some_and(|held| std::rc::Rc::ptr_eq(&held, &self.held));
            if ours {
                slot.borrow_mut().take();
            }
        });
    }
}

impl Drop for EnvWriteGuard {
    fn drop(&mut self) {
        drop(self.held.take());
        if let Some(held) = self.restore.as_ref().and_then(std::rc::Weak::upgrade) {
            *held.borrow_mut() = Some(
                PROCESS_STATE
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }
    }
}

/// An exclusive test environment restored before another test can enter.
#[must_use = "dropping the guard restores the environment and cwd"]
pub struct TestEnv {
    _guard: EnvWriteGuard,
    vars: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    cwd: Option<std::path::PathBuf>,
}

/// Snapshot and exclusively hold the process environment and current
/// directory. Integration tests use this instead of file-local mutexes.
pub fn env_lock() -> TestEnv {
    let guard = env_write_lock();
    let vars = std::env::vars_os().collect();
    let cwd = std::env::current_dir().ok();
    TestEnv {
        _guard: guard,
        vars,
        cwd,
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let keep: std::collections::HashSet<_> =
            self.vars.iter().map(|(key, _)| key.clone()).collect();
        for (key, value) in &self.vars {
            // SAFETY: TestEnv holds the process-global writer lock.
            unsafe { std::env::set_var(key, value) };
        }
        let added: Vec<_> = std::env::vars_os()
            .map(|(key, _)| key)
            .filter(|key| !keep.contains(key))
            .collect();
        for key in added {
            // SAFETY: TestEnv holds the process-global writer lock.
            unsafe { std::env::remove_var(key) };
        }
        if let Some(cwd) = &self.cwd {
            let _ = std::env::set_current_dir(cwd);
        }
    }
}

#[cfg(target_os = "linux")]
extern "C" {
    pub fn ljos_test_enter_cgroup(dir: *const c_char) -> i32;
    pub fn ljos_test_session_cgroup();
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn ljos_test_enter_cgroup(_dir: *const c_char) -> i32 {
    -1
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn ljos_test_session_cgroup() {}

/// Address of the mover. A test binary keeps a pointer to it so the
/// linker pulls the object, constructor included.
#[doc(hidden)]
pub const ENTER: unsafe extern "C" fn(*const c_char) -> i32 = ljos_test_enter_cgroup;

/// Address of the load-time setup. Referencing it keeps that function.
#[doc(hidden)]
pub const SESSION: unsafe extern "C" fn() = ljos_test_session_cgroup;

/// The payload of one HTTP response.
///
/// packsetd 0.17.9 and later stream a listing with chunked transfer
/// encoding. The bytes after the header are then a chunk size, the JSON,
/// and a closing zero. A reader that parses those bytes as JSON stops on
/// the size. A response with a content length is the body as written.
///
/// # Errors
///
/// The text has no header, a chunk size is not hex, or a chunk is short.
pub fn http_body(raw: &str) -> Result<String, String> {
    let bytes = raw.as_bytes();
    let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err(format!("no http body: {raw}"));
    };
    let head = &raw[..at];
    let mut rest = &bytes[at + 4..];
    let chunked = head.lines().any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
    });
    if !chunked {
        return String::from_utf8(rest.to_vec()).map_err(|err| err.to_string());
    }
    let mut out = Vec::new();
    loop {
        let Some(split) = rest.windows(2).position(|w| w == b"\r\n") else {
            return Err(format!("short chunk: {raw}"));
        };
        let size_line = std::str::from_utf8(&rest[..split]).map_err(|err| err.to_string())?;
        let size_token = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_token, 16)
            .map_err(|err| format!("chunk size {size_line:?}: {err}"))?;
        rest = &rest[split + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size + 2 {
            return Err(format!("truncated chunk of {size}"));
        }
        out.extend_from_slice(&rest[..size]);
        if &rest[size..size + 2] != b"\r\n" {
            return Err("chunk missing CRLF".to_string());
        }
        rest = &rest[size + 2..];
    }
    String::from_utf8(out).map_err(|err| err.to_string())
}

/// Move this process into `dir`.
///
/// # Errors
///
/// The directory cannot be created, or the kernel rejects the process
/// write. `errno` from that write is returned.
pub fn enter(dir: &str) -> std::io::Result<()> {
    let path = std::ffi::CString::new(dir).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cgroup path contains a nul",
        )
    })?;
    // SAFETY: `path` is a NUL-terminated string. The mover only reads it
    // for the duration of the call.
    let rc = unsafe { ljos_test_enter_cgroup(path.as_ptr()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Pull the test object into this binary. Invoke once at crate root.
#[macro_export]
macro_rules! harness {
    () => {
        #[used]
        #[allow(dead_code)]
        static LJOS_TEST_HARNESS: unsafe extern "C" fn(*const std::ffi::c_char) -> i32 =
            $crate::ENTER;
        #[used]
        #[allow(dead_code)]
        static LJOS_TEST_SESSION: unsafe extern "C" fn() = $crate::SESSION;
        #[used]
        #[allow(dead_code)]
        static LJOS_TEST_CANON: unsafe extern "C" fn() = $crate::CANON_TMP;
    };
}

#[cfg(test)]
#[used]
#[allow(dead_code)]
static LJOS_TEST_HARNESS: unsafe extern "C" fn(*const c_char) -> i32 = ENTER;

#[cfg(test)]
#[used]
#[allow(dead_code)]
static LJOS_TEST_SESSION: unsafe extern "C" fn() = SESSION;

extern "C" {
    fn ljos_test_canon_tmpdir_keep();
}

/// Address of the temp-dir setup. Referencing it keeps that object,
/// constructor included.
#[doc(hidden)]
pub const CANON_TMP: unsafe extern "C" fn() = ljos_test_canon_tmpdir_keep;

#[used]
#[allow(dead_code)]
static LJOS_TEST_CANON_TMP: unsafe extern "C" fn() = CANON_TMP;

#[cfg(test)]
mod tests {
    #[test]
    fn this_process_left_the_host_cgroup() {
        let text = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
        let runners = ["cursor", "claude", "codex"];
        let names_runner = text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| word.len() >= 4 && runners.contains(&word));
        assert!(
            !names_runner,
            "test process still sits in a runner cgroup: {text}"
        );
    }
}
