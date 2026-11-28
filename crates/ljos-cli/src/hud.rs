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
    path: Option<std::ffi::OsString>,
    exe: PathBuf,
    found: Option<PathBuf>,
}

static HUD_HIT: std::sync::Mutex<Option<HudHit>> = std::sync::Mutex::new(None);

/// Locate `ljos-hud`. Missing means exit 127.
///
/// The order is the base launcher's. A set `LJOS_HUD_BIN` is the trimmed
/// string and nothing else. The variable bytes are not rewritten. A
/// slash-free string is not looked up on `PATH` in this walk: the start
/// directory is stated, and only a regular file is returned, as that
/// same string, for `execvp`. Otherwise the sibling beside this binary,
/// then each `PATH` entry, including `.`, an empty component, and any
/// other relative entry. An unset `PATH` adds no entry. A path that
/// contains a slash is the first regular file, which is what base execs,
/// until a slash-free file has made the rest of the walk `execvp`.
/// `execvp` skips a file this user cannot execute and tries the next
/// entry. A stat that has not started is waited for. A stat that has
/// finished is kept, however old its stamp is. A stat that is still
/// running when the budget ends is skipped, and the walk tries the next
/// entry. It does not stop the command.
pub fn resolve_hud_bin() -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let hud = std::env::var_os(HUD_BIN_ENV).unwrap_or_default();
    let path = std::env::var_os("PATH");
    let exe = std::env::current_exe().unwrap_or_default();
    let mut held = HUD_HIT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(hit) = held.as_ref() {
        if hit.cwd == cwd && hit.hud == hud && hit.path == path && hit.exe == exe {
            return hit.found.clone();
        }
    }
    let found = walk_hud(&hud, path.as_deref());
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
/// Filing commands never call this, so they do not stat `PATH`. An unset
/// `PATH` adds no entry. An empty `PATH` is the current directory.
/// `statx` does not stop a lookup that has not returned, so each stat
/// runs on its own thread. The budget for that stat starts when the stat
/// starts. A stat that has not started is waited for. A stat that has
/// finished is the program. A stat that is still running and does not
/// answer is skipped and is not the program.
fn walk_hud(hud: &std::ffi::OsString, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(text) = hud.to_str() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            let chosen = PathBuf::from(trimmed);
            // Base returns this trimmed string and does not search `PATH`.
            // `is_file` of a slash-free name is the start directory. Only
            // a regular file is returned, and the string itself is what
            // `execvp` receives. A miss, or a stat that does not answer,
            // is not a program and does not fall through.
            if path_has_slash(&chosen) {
                return Some(chosen);
            }
            let stated = crate::cwd_probe::from_start(&chosen);
            return first_ready(vec![stated], false).map(|_| chosen);
        }
    }
    let mut cands = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        cands.push(dir.join("ljos-hud"));
    }
    if let Some(listed) = path {
        for dir in std::env::split_paths(listed) {
            cands.push(dir.join("ljos-hud"));
        }
    }
    first_ready(cands, false)
}

/// What a stated candidate is, in base's exec rules.
#[derive(Clone, Copy)]
enum HudFile {
    /// Regular file base would exec, or a file `execvp` can run.
    Yes,
    /// Not a regular file.
    No,
    /// Slash-free regular file this user cannot execute.
    Denied,
    /// Regular file with a slash that this user cannot execute.
    /// Base execs that file when it is the first regular file. `execvp`
    /// skips it and remembers `EACCES`.
    Blocked,
}

/// Where a candidate's stat is.
#[derive(Clone, Copy)]
enum Phase {
    /// The worker has not entered the stat.
    Pending,
    /// The stat has started.
    Running(std::time::Instant),
    Yes,
    No,
    Denied,
    Blocked,
    /// The stat started, was still running, and overran. A late answer
    /// is not a program. A stat that had already finished is not this.
    Skip,
}

enum Note {
    Started(usize, std::time::Instant),
    Done(usize, HudFile),
}

enum Step {
    Hit(usize),
    /// Wait for the next note. `None` means the blocking stat has not started.
    Hold(Option<std::time::Duration>),
    /// No executable program. The index is a slash-free non-executable file,
    /// which base still hands to `execvp` once nothing later is a program.
    Miss(Option<usize>),
}

/// First candidate base would exec, in order.
///
/// `bare` is an `execvp` search: a non-executable file is never the
/// program while a later executable exists. Every candidate is stated
/// together. An earlier stat that has not started still blocks a later
/// one. A stat that has finished is kept even when the stamp is already
/// older than the budget: that age is how late this thread read the
/// stamp, not a stat that is still running. Only a stat that is still
/// running when the budget ends is skipped, and a later one that did
/// answer can win.
fn first_ready(cands: Vec<PathBuf>, bare: bool) -> Option<PathBuf> {
    if cands.is_empty() {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    for (index, path) in cands.iter().cloned().enumerate() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Note::Started(index, std::time::Instant::now()));
            let _ = tx.send(Note::Done(index, accept_hud(&path)));
        });
    }
    drop(tx);
    collect(&cands, &rx, bare)
}

/// Read `rx` until one candidate is the program.
///
/// A `Done` already in the queue is the program, even when the matching
/// `Started` stamp is older than the budget. The budget waits out only a
/// stat that has not finished.
fn collect(cands: &[PathBuf], rx: &std::sync::mpsc::Receiver<Note>, bare: bool) -> Option<PathBuf> {
    let mut phase = vec![Phase::Pending; cands.len()];
    loop {
        match step(&mut phase, bare) {
            Step::Hit(index) => return Some(cands[index].clone()),
            Step::Miss(denied) => return denied.map(|index| cands[index].clone()),
            Step::Hold(None) => match rx.recv() {
                Ok(note) => apply_note(&mut phase, note),
                Err(_) => abandon(&mut phase),
            },
            Step::Hold(Some(limit)) => {
                if !limit.is_zero() {
                    match rx.recv_timeout(limit) {
                        Ok(note) => {
                            apply_note(&mut phase, note);
                            continue;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            abandon(&mut phase);
                            continue;
                        }
                    }
                }
                // The budget for a still-running stat is over. Take a result
                // that is already queued before skipping. A finished stat's
                // stamp is often older than the budget by the time this
                // thread reads it; that result is the program.
                loop {
                    match rx.try_recv() {
                        Ok(note) => apply_note(&mut phase, note),
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            skip_overdue(&mut phase);
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            abandon(&mut phase);
                            break;
                        }
                    }
                }
            }
        }
    }
}

fn apply_note(phase: &mut [Phase], note: Note) {
    match note {
        Note::Started(index, started) => {
            if index < phase.len() && matches!(phase[index], Phase::Pending) {
                phase[index] = Phase::Running(started);
            }
        }
        Note::Done(index, kind) => {
            if index >= phase.len() || matches!(phase[index], Phase::Skip) {
                return;
            }
            phase[index] = match kind {
                HudFile::Yes => Phase::Yes,
                HudFile::No => Phase::No,
                HudFile::Denied => Phase::Denied,
                HudFile::Blocked => Phase::Blocked,
            };
        }
    }
}

fn abandon(phase: &mut [Phase]) {
    for phase in phase.iter_mut() {
        if matches!(phase, Phase::Pending | Phase::Running(_)) {
            *phase = Phase::Skip;
        }
    }
}

/// Mark a stat that is still running and past the budget.
///
/// A finished stat is not running, so its age does not land here.
fn skip_overdue(phase: &mut [Phase]) {
    for slot in phase.iter_mut() {
        if let Phase::Running(started) = *slot {
            if started.elapsed() >= crate::cwd_probe::CWD_PROBE {
                *slot = Phase::Skip;
            }
        }
    }
}

fn step(phase: &mut [Phase], bare: bool) -> Step {
    let mut denied = None;
    // A slash-free non-executable file means base handed the bare name to
    // `execvp`, which skips every later `EACCES`, including a path that
    // contains a slash. `bare` is that search from the first entry.
    let mut execvp = bare;
    for (index, slot) in phase.iter().enumerate() {
        match *slot {
            Phase::Yes => return Step::Hit(index),
            Phase::No | Phase::Skip => continue,
            Phase::Denied => {
                execvp = true;
                if denied.is_none() {
                    denied = Some(index);
                }
            }
            Phase::Blocked => {
                if !execvp {
                    return Step::Hit(index);
                }
                if denied.is_none() {
                    denied = Some(index);
                }
            }
            Phase::Running(started) => {
                let left = crate::cwd_probe::CWD_PROBE.saturating_sub(started.elapsed());
                return Step::Hold(Some(left));
            }
            Phase::Pending => return Step::Hold(None),
        }
    }
    Step::Miss(denied)
}

/// Regular file, and executable by this user when `execvp` is searching.
///
/// Base's `is_file` accepts a mode `644` file. `Command` then `execve`s a
/// path that contains a slash, and `execvp`s a slash-free name. `execvp`
/// skips a file this user cannot execute, remembers `EACCES`, and tries
/// the next `PATH` entry. A directory, a fifo, and any other non-regular
/// file are not candidates.
fn accept_hud(path: &Path) -> HudFile {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        _ => return HudFile::No,
    }
    if user_can_exec(path) {
        HudFile::Yes
    } else if path_has_slash(path) {
        HudFile::Blocked
    } else {
        HudFile::Denied
    }
}

fn path_has_slash(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            std::path::Component::RootDir
                | std::path::Component::Prefix(_)
                | std::path::Component::CurDir
                | std::path::Component::ParentDir
        )
    }) || path.components().count() > 1
}

fn user_can_exec(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let flags = libc::AT_EACCESS;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let flags = 0;
        // SAFETY: `c_path` is NUL-terminated. The check does not create a file.
        unsafe { libc::faccessat(libc::AT_FDCWD, c_path.as_ptr(), libc::X_OK, flags) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        true
    }
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
    let bin = resolve_hud_bin().and_then(|path| {
        if path_has_slash(&path) {
            absolute_ready(&path)
        } else {
            // Already stated in the start directory. Canonicalizing it
            // would exec that file. `execvp` must see the slash-free string.
            Some(path)
        }
    });
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

    /// A stat that has finished is kept when its stamp is already older
    /// than the budget. A stat that is still running is not.
    #[test]
    fn a_finished_stat_with_an_old_stamp_is_the_program() {
        let _env_read = crate::test_env_read_lock();
        let path = PathBuf::from("ljos-hud");
        let started = std::time::Instant::now()
            .checked_sub(crate::cwd_probe::CWD_PROBE + std::time::Duration::from_millis(50))
            .expect("stamp in the past");
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Note::Started(0, started)).unwrap();
        tx.send(Note::Done(0, HudFile::Yes)).unwrap();
        drop(tx);
        assert_eq!(collect(std::slice::from_ref(&path), &rx, false), Some(path));

        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Note::Started(0, started)).unwrap();
        drop(tx);
        assert_eq!(
            collect(&[PathBuf::from("ljos-hud")], &rx, false),
            None,
            "a stat that has not finished is not a program once the budget is over"
        );
    }
}
