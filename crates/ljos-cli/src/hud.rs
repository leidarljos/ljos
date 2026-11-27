//! `ljos hud` — exec the separate `ljos-hud` binary.
//!
//! The musl CLI stays thin: this module never depends on iced. The HUD is a
//! glibc workspace crate (`crates/ljos-hud`) and is not in a musl tarball.

use std::path::{Path, PathBuf};

/// Env override for the HUD binary this launcher execs.
pub const HUD_BIN_ENV: &str = "LJOS_HUD_BIN";

/// Flags forwarded to `ljos-hud`.
#[derive(Debug, Clone, Default)]
pub struct HudLaunch {
    /// Stay on the terminal.
    pub foreground: bool,
    /// Show or hide a running HUD.
    pub toggle: bool,
    /// Show a running HUD.
    pub show: bool,
    /// Hide a running HUD.
    pub hide: bool,
    /// Write a user-local .desktop launcher and Sway overlay include.
    pub install_desktop: bool,
}

/// One answer from [`resolve_hud_bin`], for this directory and environment.
///
/// A second call, including `--hide` after the lookup that `--foreground`
/// would have used, returns this path. A stat that did not answer on the
/// first walk is not stated again.
struct HudHit {
    cwd: PathBuf,
    hud: std::ffi::OsString,
    path: std::ffi::OsString,
    exe: PathBuf,
    found: Option<PathBuf>,
}

static HUD_HIT: std::sync::Mutex<Option<HudHit>> = std::sync::Mutex::new(None);

/// Locate `ljos-hud`. Missing means exit 127.
///
/// The order is the base launcher's. A set `LJOS_HUD_BIN` is that path
/// and nothing else; the bytes of the variable are not rewritten. Otherwise
/// the sibling beside this binary, then each `PATH` entry, including `.`,
/// an empty component, and any other relative entry. A candidate is a
/// regular file. A stat that does not answer is not that file, and the
/// walk tries the next entry. It does not stop the command.
pub fn resolve_hud_bin() -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let hud = std::env::var_os(HUD_BIN_ENV).unwrap_or_default();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let exe = std::env::current_exe().unwrap_or_default();
    let mut held = HUD_HIT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(hit) = held.as_ref() {
        if hit.cwd == cwd && hit.hud == hud && hit.path == path && hit.exe == exe {
            return hit.found.clone();
        }
    }
    let found = walk_hud(&hud, &path);
    *held = Some(HudHit {
        cwd,
        hud,
        path,
        exe,
        found: found.clone(),
    });
    found
}

/// `LJOS_HUD_BIN`, then the sibling, then `PATH`, in that order.
///
/// Filing commands never call this, so they do not stat `PATH`.
/// The sibling and `PATH` share one [`crate::cwd_probe::CWD_PROBE`].
/// `statx` with `AT_STATX_DONT_SYNC` only skips a cached server refresh,
/// and a lookup that has not returned still blocks, so the wait is
/// abandoned instead. An entry that has not answered is skipped. The
/// next entry does not start a new budget.
fn walk_hud(hud: &std::ffi::OsString, path: &std::ffi::OsString) -> Option<PathBuf> {
    if let Some(text) = hud.to_str() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    let mut cands = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        cands.push(dir.join("ljos-hud"));
    }
    for dir in std::env::split_paths(path) {
        cands.push(dir.join("ljos-hud"));
    }
    first_ready(cands)
}

/// First candidate that is a regular file, in order, within one budget.
///
/// Every candidate is stated together. An earlier stat that does not
/// answer is not a program, so a later one that did answer can win.
/// The wait is not restarted per entry.
fn first_ready(cands: Vec<PathBuf>) -> Option<PathBuf> {
    if cands.is_empty() {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    for (index, path) in cands.iter().cloned().enumerate() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let ready = std::fs::metadata(&path)
                .map(|meta| meta.is_file())
                .unwrap_or(false);
            let _ = tx.send((index, ready));
        });
    }
    drop(tx);
    let deadline = std::time::Instant::now() + crate::cwd_probe::CWD_PROBE;
    let mut answered = vec![None; cands.len()];
    while std::time::Instant::now() < deadline {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match rx.recv_timeout(left) {
            Ok((index, ready)) => {
                if index < answered.len() {
                    answered[index] = Some(ready);
                }
                if let Some(hit) = pick_ready(&cands, &answered, false) {
                    return Some(hit);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    pick_ready(&cands, &answered, true)
}

/// The earliest regular file. While the budget is open, an unanswered
/// earlier candidate still blocks a later one. When the budget is closed,
/// an unanswered candidate is skipped.
fn pick_ready(
    cands: &[PathBuf],
    answered: &[Option<bool>],
    budget_closed: bool,
) -> Option<PathBuf> {
    for (index, path) in cands.iter().enumerate() {
        match answered.get(index).copied().flatten() {
            Some(true) => return Some(path.clone()),
            Some(false) => continue,
            None if budget_closed => continue,
            None => return None,
        }
    }
    None
}

/// `path` when it is a regular file, as an absolute path.
///
/// A relative candidate is resolved from the current directory, which is
/// the parked directory when this process has already moved. The absolute
/// path is what `exec` uses after that directory is removed.
fn absolute_ready(path: &Path) -> Option<PathBuf> {
    if !crate::cwd_probe::program_file(path) {
        return None;
    }
    std::fs::canonicalize(path).ok()
}

/// Hint printed when the launcher cannot find the HUD binary.
pub fn missing_bin_message() -> &'static str {
    "ljos-hud is not installed. Install it with:\n  cargo install ljos-hud"
}

/// Dispatch `ljos hud`. `--help` is answered by clap before this runs.
pub fn run(opts: HudLaunch) -> i32 {
    // Hide with no HUD binary is success (compositor bind miss).
    // One lookup serves `--hide` and `--foreground`.
    let bin = resolve_hud_bin().and_then(|path| absolute_ready(&path));
    if opts.hide && !opts.show && !opts.toggle && !opts.install_desktop && bin.is_none() {
        return 0;
    }
    let Some(bin) = bin else {
        say!("{}", missing_bin_message());
        return 127;
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = std::process::Command::new(&bin);
    if opts.foreground {
        cmd.arg("--foreground");
    }
    if opts.toggle {
        cmd.arg("--toggle");
    } else if opts.show {
        cmd.arg("--show");
    } else if opts.hide {
        cmd.arg("--hide");
    }
    if opts.install_desktop {
        cmd.arg("--install-desktop");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // `exec` skips `atexit`. Leave only when this process is still in
        // the parked directory it created. A fast local directory is not
        // that directory: the new image keeps it, and a relative `TMPDIR`,
        // `VISSUE_ROOT`, or HUD path still names the start directory. The
        // replacement is searchable, or `/` when the tracker is not.
        if let Some(stand) = crate::cwd_probe::parked_replacement_dir() {
            let _ = std::env::set_current_dir(&stand);
            crate::cwd_probe::release_parked_dir();
            cmd.current_dir(&stand);
        }
        let err = cmd.exec();
        say!("ljos: exec {}: {err}", bin.display());
        1
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(st) => st.code().unwrap_or(1),
            Err(e) => {
                say!("ljos: spawn {}: {e}", bin.display());
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests below set `LJOS_HUD_BIN`. They take the same environment
    /// lock as the rest of the crate, and the guard puts the variable back.
    fn hud_env() -> crate::EnvLock {
        crate::test_env_lock()
    }

    fn missing_hud() -> PathBuf {
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").expect("private runtime dir"))
            .join("ljos-hud-missing")
    }

    #[test]
    fn missing_bin_is_127() {
        let _env_read = crate::test_env_read_lock();
        let _env = hud_env();
        let missing = missing_hud();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(HUD_BIN_ENV, &missing);
        }
        let code = run(HudLaunch {
            foreground: true,
            ..HudLaunch::default()
        });
        assert_eq!(code, 127);
        assert!(missing_bin_message().contains("ljos-hud"));
    }

    #[test]
    fn hide_without_binary_is_zero() {
        let _env_read = crate::test_env_read_lock();
        let _env = hud_env();
        let missing = missing_hud();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(HUD_BIN_ENV, &missing);
        }
        let code = run(HudLaunch {
            hide: true,
            ..HudLaunch::default()
        });
        assert_eq!(code, 0);
    }

    #[test]
    fn launcher_lives_in_the_cli() {
        let _env_read = crate::test_env_read_lock();
        let src = include_str!("main.rs");
        assert!(src.contains("Cmd::Hud"));
        assert!(src.contains("ljos-hud"));
        assert!(src.contains("install_desktop"));
        let cargo = include_str!("../Cargo.toml");
        assert!(!cargo.contains("iced"), "ljos crate must not link iced");
        assert!(!cargo.contains("icedtea"));
        assert!(!cargo.contains("ljos-hud"));
    }

    #[test]
    fn resolve_honors_override() {
        let _env_read = crate::test_env_read_lock();
        let _env = hud_env();
        let path = missing_hud();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(HUD_BIN_ENV, path.to_str().unwrap());
        }
        assert_eq!(resolve_hud_bin(), Some(path));
    }
}
