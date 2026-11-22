//! Move a test process out of a host runner cgroup, and drop runner
//! variables, before `main`. The object is linked only into crates that
//! name this one. `ljos` does not.
//!
//! [`Packsetd`] starts the pack writer for a test and stops it again.
//!
//! [`git`] and [`fresh_dir`] are the git and the scratch directories a
//! test uses.

mod packsetd;
mod scratch;

pub use packsetd::{reap_listener, Builder, Packsetd};
pub use scratch::{
    build_target, checkout, checkout_seats, fresh_dir, git, git_at, git_ceiling, home_seats,
    no_login_git_config, GIT_CEILING,
};

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
    fn ljos_test_boot_home() -> *const c_char;
}

/// `HOME` as this process had it before `main`, symlinks resolved.
/// `None` when it was unset or empty.
#[must_use]
pub fn boot_home() -> Option<&'static std::path::Path> {
    static HOME: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        // SAFETY: the pointer is null or names a NUL-terminated buffer the
        // constructor filled before main and nothing writes again.
        let ptr = unsafe { ljos_test_boot_home() };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: as above.
        let bytes = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_bytes();
        #[cfg(unix)]
        let home = {
            use std::os::unix::ffi::OsStrExt;
            std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
        };
        #[cfg(not(unix))]
        let home = std::path::PathBuf::from(String::from_utf8_lossy(bytes).into_owned());
        Some(home)
    })
    .as_deref()
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
    /// Held by every test here that changes this process's environment.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct PutEnv(&'static str, Option<std::ffi::OsString>);

    impl PutEnv {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let old = std::env::var_os(key);
            // SAFETY: the caller holds ENV.
            unsafe { std::env::set_var(key, value) };
            Self(key, old)
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var_os(key);
            // SAFETY: the caller holds ENV.
            unsafe { std::env::remove_var(key) };
            Self(key, old)
        }
    }

    impl Drop for PutEnv {
        fn drop(&mut self) {
            // SAFETY: the guard is dropped before ENV.
            unsafe {
                match &self.1 {
                    Some(v) => std::env::set_var(self.0, v),
                    None => std::env::remove_var(self.0),
                }
            }
        }
    }

    #[test]
    fn a_home_set_mid_run_moves_no_seat_root_or_ceiling() {
        let _env = ENV
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let seats = crate::home_seats();
        let ceiling = crate::git_ceiling();
        let temp = tempfile_dir("home-mid-run");
        let _home = PutEnv::set("HOME", &temp);
        assert_eq!(crate::home_seats(), seats, "HOME is {}", temp.display());
        assert_eq!(crate::git_ceiling(), ceiling);
        let moved =
            std::env::split_paths(&crate::git_ceiling()).any(|root| root.starts_with(&temp));
        assert!(!moved, "a ceiling followed HOME to {}", temp.display());
        let _ = std::fs::remove_dir_all(&temp);
    }

    /// A `~/.gitconfig` that signs every commit with a program that is not
    /// there fails a plain `git commit`, and not one from [`crate::git`].
    #[test]
    fn a_hostile_home_gitconfig_does_not_reach_the_scratch_git() {
        let _env = ENV
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = tempfile_dir("hostile-gitconfig");
        let home = root.join("home");
        std::fs::create_dir_all(home.join(".config/git")).unwrap();
        let hostile = "[commit]\n\tgpgsign = true\n[gpg]\n\tprogram = /nonexistent/ljos-gpg\n";
        std::fs::write(home.join(".gitconfig"), hostile).unwrap();
        std::fs::write(home.join(".config/git/config"), hostile).unwrap();
        let _home = PutEnv::set("HOME", &home);
        let _xdg = PutEnv::set("XDG_CONFIG_HOME", home.join(".config"));
        let _global = PutEnv::remove("GIT_CONFIG_GLOBAL");
        let _system = PutEnv::remove("GIT_CONFIG_SYSTEM");
        let _nosystem = PutEnv::remove("GIT_CONFIG_NOSYSTEM");
        let repo = root.join("repo");
        let commit = |mut git: std::process::Command| {
            git.arg("-C")
                .arg(&repo)
                .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid"])
                .args(["commit", "-q", "--allow-empty", "-m", "seed"])
                .output()
                .unwrap()
        };
        let init = crate::git()
            .args(["init", "-q"])
            .arg(&repo)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let mut plain = std::process::Command::new("git");
        plain.env(crate::GIT_CEILING, crate::git_ceiling());
        let refused = commit(plain);
        assert!(
            !refused.status.success(),
            "the hostile config did not apply: {refused:?}"
        );
        for (what, git) in [("git", crate::git()), ("git_at", crate::git_at("git"))] {
            let out = commit(git);
            assert!(out.status.success(), "{what}: {out:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A global or system config the process names is the one git reads.
    #[test]
    fn a_git_config_the_process_names_is_kept() {
        let _env = ENV
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _global = PutEnv::set("GIT_CONFIG_GLOBAL", "/tmp/named-global");
        let _system = PutEnv::set("GIT_CONFIG_SYSTEM", "/tmp/named-system");
        let cmd = crate::git();
        let set: Vec<_> = cmd.get_envs().map(|(k, _)| k.to_owned()).collect();
        assert!(!set.iter().any(|k| k == "GIT_CONFIG_GLOBAL"), "{set:?}");
        assert!(!set.iter().any(|k| k == "GIT_CONFIG_NOSYSTEM"), "{set:?}");
    }

    /// The target directory this binary runs from is a ceiling, wherever
    /// `CARGO_TARGET_DIR` put it.
    #[test]
    fn the_build_target_of_this_binary_is_a_ceiling() {
        let exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let target = crate::build_target().expect("a test binary runs from a target directory");
        assert!(exe.starts_with(&target), "{}", target.display());
        assert!(target.join("CACHEDIR.TAG").is_file() || target.join(".rustc_info.json").is_file());
        let ceilings: Vec<_> = std::env::split_paths(&crate::git_ceiling()).collect();
        assert!(ceilings.contains(&target), "{ceilings:?}");
    }

    fn tempfile_dir(prefix: &str) -> std::path::PathBuf {
        crate::fresh_dir(&std::env::temp_dir(), &format!("ljos-test-boot-{prefix}"))
    }

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
