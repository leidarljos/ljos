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

/// Locate `ljos-hud`. Missing means exit 127.
///
/// The order is the base launcher's. A set `LJOS_HUD_BIN` is the trimmed
/// string and nothing else. The variable bytes are not rewritten. A
/// slash-free string is not looked up on `PATH` in this walk. Otherwise
/// the sibling beside this binary, then each `PATH` entry, including `.`,
/// an empty component, and any other relative entry. An unset `PATH` adds
/// no entry. The first regular file is the one base's `is_file` returns.
/// `execvp` of a slash-free name then skips a file this user cannot
/// execute. A stat that has not started is waited for. A stat that has
/// finished is kept, however old its stamp is. A stat that is still
/// running when the budget ends is skipped, and the walk tries the next
/// entry. It does not stop the command. The path is not stated again.
pub fn resolve_hud_bin() -> Option<PathBuf> {
    if let Some(path) = override_path() {
        return Some(path);
    }
    sibling_or_path()
}

/// Trimmed `LJOS_HUD_BIN` when it is set and not only whitespace.
///
/// Base returns this string and does not search `PATH`. The bytes in the
/// environment stay as they were set.
fn override_path() -> Option<PathBuf> {
    let raw = std::env::var(HUD_BIN_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Sibling beside this binary, then each `PATH` entry, first regular file.
fn sibling_or_path() -> Option<PathBuf> {
    let mut cands = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        cands.push(dir.join("ljos-hud"));
    }
    if let Some(listed) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&listed) {
            cands.push(dir.join("ljos-hud"));
        }
    }
    first_ready(cands)
}

/// What a stated candidate is.
#[derive(Clone, Copy)]
enum HudFile {
    /// Regular file. Base's `is_file` accepts it, including mode `644`.
    Yes,
    /// Not a regular file. A directory, a fifo, and a symlink to a
    /// device are this. `execvp` would not run them either.
    No,
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
    /// The stat started, was still running, and overran. A late answer
    /// is not a program. A stat that had already finished is not this.
    Skip,
}

enum Note {
    Started(usize, std::time::Instant),
    Done(usize, HudFile),
}

enum Step {
    /// The program is this candidate.
    Hit(usize),
    /// Wait for the next note. `None` means the blocking stat has not started.
    Hold(Option<std::time::Duration>),
    /// No regular file.
    Miss,
}

/// First regular file, in order.
///
/// The next entry is stated only after the earlier one is not a regular
/// file. A hit stops the walk, and a later entry is not stated. `execve`
/// waits in `de_thread` until every other thread has left its syscall, so
/// a stat of that later entry would hold the exec. An earlier stat that
/// has not started still blocks a later one. A stat that has finished is
/// kept even when the stamp is already older than the budget: that age is
/// how late this thread read the stamp, not a stat that is still running.
/// Only a stat that is still running when the budget ends is skipped.
fn first_ready(cands: Vec<PathBuf>) -> Option<PathBuf> {
    if cands.is_empty() {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    collect(&cands, &tx, &rx)
}

/// Read `rx` until one candidate is a regular file.
///
/// Candidates are stated one at a time. A `Done` already in the queue is
/// the program, even when the matching `Started` stamp is older than the
/// budget. The budget waits out only a stat that has not finished.
fn collect(
    cands: &[PathBuf],
    tx: &std::sync::mpsc::Sender<Note>,
    rx: &std::sync::mpsc::Receiver<Note>,
) -> Option<PathBuf> {
    let mut phase = vec![Phase::Pending; cands.len()];
    let mut spawned = vec![false; cands.len()];
    loop {
        match step(&phase) {
            Step::Hit(index) => return Some(cands[index].clone()),
            Step::Miss => return None,
            Step::Hold(None) => {
                if !take_note(&mut phase, rx) && !start_next(cands, tx, &phase, &mut spawned) {
                    match rx.recv() {
                        Ok(note) => apply_note(&mut phase, note),
                        Err(_) => abandon(&mut phase),
                    }
                }
            }
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

/// Apply a note that is already queued. `false` when the queue is empty.
fn take_note(phase: &mut [Phase], rx: &std::sync::mpsc::Receiver<Note>) -> bool {
    match rx.try_recv() {
        Ok(note) => {
            apply_note(phase, note);
            true
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => false,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            abandon(phase);
            true
        }
    }
}

/// State the next entry that the walk has reached and has not stated.
///
/// A hit returns before this runs, so a later `PATH` entry is never
/// stated. `false` when the walk is waiting on a stat that already started.
fn start_next(
    cands: &[PathBuf],
    tx: &std::sync::mpsc::Sender<Note>,
    phase: &[Phase],
    spawned: &mut [bool],
) -> bool {
    for (index, slot) in phase.iter().enumerate() {
        match *slot {
            Phase::No | Phase::Skip => continue,
            Phase::Pending if !spawned[index] => {
                spawned[index] = true;
                let path = cands[index].clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Note::Started(index, std::time::Instant::now()));
                    let _ = tx.send(Note::Done(index, accept_hud(&path)));
                });
                return true;
            }
            Phase::Pending | Phase::Running(_) | Phase::Yes => return false,
        }
    }
    false
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

fn step(phase: &[Phase]) -> Step {
    for (index, slot) in phase.iter().enumerate() {
        match *slot {
            Phase::Yes => return Step::Hit(index),
            Phase::No | Phase::Skip => continue,
            Phase::Running(started) => {
                let left = crate::cwd_probe::CWD_PROBE.saturating_sub(started.elapsed());
                return Step::Hold(Some(left));
            }
            Phase::Pending => return Step::Hold(None),
        }
    }
    Step::Miss
}

/// Regular file, which is what base's `is_file` accepts.
///
/// Mode `644` is a regular file. A slash-free name is then handed to
/// `execvp`, which skips a file this user cannot execute and tries the
/// next `PATH` entry. A directory, a fifo, and any other non-regular
/// file are not candidates. This does not call `faccessat`: that check
/// accepts a script whose interpreter `execve` will reject, and exec of
/// the pathname then stops the search.
fn accept_hud(path: &Path) -> HudFile {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => HudFile::Yes,
        _ => HudFile::No,
    }
}

/// Whether `path` is a regular file, stated once on the deadline.
fn file_ready(path: &Path) -> bool {
    first_ready(vec![path.to_path_buf()]).is_some()
}

/// Hint printed when the launcher cannot find the HUD binary.
pub fn missing_bin_message() -> &'static str {
    "ljos-hud is not installed. Install it with:\n  cargo install ljos-hud"
}

/// Dispatch `ljos hud`. `--help` is answered by clap before this runs.
pub fn run(opts: HudLaunch) -> i32 {
    // Hide with no HUD binary is success (compositor bind miss).
    // One lookup serves `--hide` and `--foreground`. The path the walk
    // accepted is not stated again: a second stat with a deadline turned
    // a program base would run into "not installed".
    let bin = if let Some(path) = override_path() {
        file_ready(&path).then_some(path)
    } else {
        resolve_hud_bin()
    };
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
        // The cwd is the parked directory after settle, or the directory
        // the command started in when it did not park. Base execs from
        // that cwd for every spelling. `execve` keeps it.
        // A slash-free name is `execvp`: `ENOENT` and `EACCES` try the
        // next `PATH` entry, and `argv[0]` stays the name. A path that
        // contains a slash is that file, which is what base execs.
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
        assert_eq!(collect(std::slice::from_ref(&path), &tx, &rx), Some(path));

        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Note::Started(0, started)).unwrap();
        assert!(
            collect(&[PathBuf::from("ljos-hud")], &tx, &rx).is_none(),
            "a stat that has not finished is not a program once the budget is over"
        );
    }
}
