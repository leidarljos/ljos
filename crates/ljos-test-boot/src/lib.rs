//! Move a test process out of a host runner cgroup, and drop runner
//! variables, before `main`. The object is linked only into crates that
//! name this one. `ljos` does not.

use std::ffi::c_char;

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
