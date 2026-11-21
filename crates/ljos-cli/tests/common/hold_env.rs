//! Process-wide HOME, XDG, and VISSUE_ROOT isolation for integration tests
//! that invoke library entry points directly.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub struct HoldEnv {
    _guard: MutexGuard<'static, ()>,
    _dir: Option<tempfile::TempDir>,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl HoldEnv {
    fn lock() -> MutexGuard<'static, ()> {
        static ENV: Mutex<()> = Mutex::new(());
        ENV.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn apply(
        guard: MutexGuard<'static, ()>,
        dir: Option<tempfile::TempDir>,
        home: &Path,
        runtime: &Path,
        tracker: &Path,
    ) -> Self {
        let vars: [(&'static str, PathBuf); 7] = [
            ("HOME", home.to_path_buf()),
            ("XDG_CONFIG_HOME", home.join(".config")),
            ("XDG_DATA_HOME", home.join(".local/share")),
            ("XDG_STATE_HOME", home.join(".local/state")),
            ("XDG_CACHE_HOME", home.join(".cache")),
            ("XDG_RUNTIME_DIR", runtime.to_path_buf()),
            ("VISSUE_ROOT", tracker.to_path_buf()),
        ];
        let mut saved = Vec::with_capacity(vars.len());
        for (k, v) in &vars {
            saved.push((*k, std::env::var_os(k)));
            #[allow(unused_unsafe)]
            unsafe {
                std::env::set_var(k, v);
            }
        }
        Self {
            _guard: guard,
            _dir: dir,
            saved,
        }
    }

    #[allow(dead_code)]
    pub fn isolated() -> Self {
        let guard = Self::lock();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        Self::apply(guard, Some(dir), &p, &p.join("run"), &p.join("tracker"))
    }

    #[allow(dead_code)]
    pub fn in_home(root: &Path) -> Self {
        let guard = Self::lock();
        Self::apply(
            guard,
            None,
            &root.join("home"),
            &root.join("run"),
            &root.join("tracker"),
        )
    }

    #[allow(dead_code)]
    pub fn in_paths(home: &Path, runtime: &Path, tracker: &Path) -> Self {
        let guard = Self::lock();
        Self::apply(guard, None, home, runtime, tracker)
    }
}

impl Drop for HoldEnv {
    fn drop(&mut self) {
        for (k, prev) in &self.saved {
            #[allow(unused_unsafe)]
            unsafe {
                match prev {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}
