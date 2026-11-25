//! Whole-file tracker writes through a temporary that cleans up after itself.
//!
//! A board or pin file is replaced by writing `.NAME.tmp.PID-NANOS-SEQ`
//! beside it and renaming that over it. A write that fails, panics or
//! returns early drops the guard, and the drop removes the temporary. A
//! process killed between the write and the rename leaves one behind;
//! [`sweep_stale`] removes those once they are an hour old and the
//! process named in them is gone.

use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The sweep leaves a temporary younger than this even when its writer is
/// gone. Age alone never removes one: a writer stopped for an hour, or a
/// clock stepped forward, still owns its temporary.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Directories this process has already swept.
static SWEPT: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// A temporary file that is removed on drop unless it was renamed.
pub(crate) struct TempFile {
    path: PathBuf,
    published: bool,
}

impl TempFile {
    /// A fresh temporary beside `target`, named so [`is_temp_name`] knows it.
    /// A dotfile target keeps its one leading dot: the temporary of
    /// `.ljos-commit.pending` is `.ljos-commit.pending.tmp.SUFFIX`, which
    /// the `info/exclude` line `/.ljos-commit.*.tmp.*` matches.
    fn beside(target: &Path) -> std::io::Result<(Self, std::fs::File)> {
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let base = target
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.trim_start_matches('.'))
            .filter(|s| !s.is_empty())
            .unwrap_or("output");
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".{base}.tmp.{}-{nanos}-{seq}", std::process::id()));
        #[expect(
            clippy::disallowed_methods,
            reason = "opens a path that is not a git hook"
        )]
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok((
            Self {
                path,
                published: false,
            },
            file,
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn publish(mut self, target: &Path) -> std::io::Result<()> {
        #[expect(
            clippy::disallowed_methods,
            reason = "renames a path that is not a git hook"
        )]
        std::fs::rename(&self.path, target)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.published {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Replace `path` with `body` through a synced temporary and a rename.
///
/// # Errors
///
/// The directory cannot be made, the temporary cannot be written or
/// synced, or the rename fails. The temporary is gone in every case.
pub(crate) fn replace_file(path: &Path, body: &str) -> std::io::Result<()> {
    replace_file_with(path, body, |_| Ok(()))
}

/// [`replace_file`] with `before_rename` run on the synced temporary, so a
/// test can fail the write at the last step.
pub(crate) fn replace_file_with(
    path: &Path,
    body: &str,
    before_rename: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let (tmp, mut file) = TempFile::beside(path)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    drop(file);
    before_rename(tmp.path())?;
    tmp.publish(path)
}

/// Whether `name` is a temporary this module or vissue writes:
/// `.NAME.tmp.SUFFIX`.
pub(crate) fn is_temp_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('.') else {
        return false;
    };
    rest.find(".tmp.")
        .is_some_and(|at| at > 0 && at + ".tmp.".len() < rest.len())
}

/// The writer pid in a temporary's name: the digits after `.tmp.` up to
/// the first `-`. Both this module and vissue put it there.
pub(crate) fn temp_writer_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix('.')?;
    let at = rest.find(".tmp.")?;
    let suffix = &rest[at + ".tmp.".len()..];
    let pid = suffix.split('-').next()?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid.parse().ok().filter(|pid| *pid > 0)
}

/// Whether a process with `pid` exists. One owned by another user still
/// counts: `kill` then fails with `EPERM`.
fn writer_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only probes for the process; nothing is delivered.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Remove temporaries in `dir` older than [`STALE_AFTER`] whose writer pid
/// is no longer running. Returns how many went. A file with no pid in its
/// name, or that cannot be read or removed, is left.
pub(crate) fn sweep_stale(dir: &Path) -> usize {
    sweep_older_than(dir, STALE_AFTER)
}

/// [`sweep_stale`] once per directory per process. A tracker write opens
/// the same directory many times in a sitting.
pub(crate) fn sweep_stale_once(dir: &Path) -> usize {
    let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    {
        let mut seen = SWEPT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !seen.get_or_insert_with(HashSet::new).insert(key) {
            return 0;
        }
    }
    sweep_stale(dir)
}

fn sweep_older_than(dir: &Path, age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut gone = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|name| is_temp_name(name)) else {
            continue;
        };
        match temp_writer_pid(name) {
            Some(pid) if !writer_alive(pid) => {}
            _ => continue,
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let old = meta
            .modified()
            .ok()
            .and_then(|at| now.duration_since(at).ok())
            .is_some_and(|since| since >= age);
        if old && std::fs::remove_file(entry.path()).is_ok() {
            gone += 1;
        }
    }
    gone
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn temps(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| is_temp_name(name))
            .collect()
    }

    #[test]
    fn a_write_replaces_the_file_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issues.org");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, "old\n").unwrap();
        replace_file(&path, "new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        assert!(temps(dir.path()).is_empty());
    }

    /// The temporary of a dotfile keeps one leading dot, so the pending
    /// list's matches `/.ljos-commit.*.tmp.*` in `info/exclude`.
    #[test]
    fn a_dotfile_temporary_has_one_leading_dot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ljos-commit.pending");
        let mut seen = String::new();
        replace_file_with(&path, "a\n", |tmp| {
            seen = tmp.file_name().unwrap().to_string_lossy().into_owned();
            Ok(())
        })
        .unwrap();
        assert!(seen.starts_with(".ljos-commit.pending.tmp."), "{seen}");
        assert!(is_temp_name(&seen), "{seen}");
        assert_eq!(temp_writer_pid(&seen), Some(std::process::id()));
    }

    #[test]
    fn a_failed_rename_step_leaves_the_old_file_and_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issues.org");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, "old\n").unwrap();
        let mut seen = PathBuf::new();
        let err = replace_file_with(&path, "new\n", |tmp| {
            seen = tmp.to_path_buf();
            assert!(tmp.is_file());
            Err(std::io::Error::other("injected"))
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "injected");
        assert!(!seen.exists(), "{} was left", seen.display());
        assert!(temps(dir.path()).is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
    }

    #[test]
    fn a_panic_mid_write_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issues.org");
        let caught = std::panic::catch_unwind(|| {
            let _ = replace_file_with(&path, "new\n", |_| panic!("injected"));
        });
        assert!(caught.is_err());
        assert!(temps(dir.path()).is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn a_rename_onto_a_directory_fails_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issues.org");
        std::fs::create_dir(&path).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(path.join("keep"), "x").unwrap();
        assert!(replace_file(&path, "new\n").is_err());
        assert!(temps(dir.path()).is_empty());
    }

    /// The pid of a process that has exited and been reaped.
    pub(crate) fn dead_pid() -> u32 {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    fn age(path: &Path, by: Duration) {
        #[expect(
            clippy::disallowed_methods,
            reason = "opens a path that is not a git hook"
        )]
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - by)
            .unwrap();
    }

    #[test]
    fn the_sweep_takes_old_temporaries_and_leaves_young_ones_and_others() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dead_pid();
        let old = dir.path().join(format!(".issues.org.tmp.{gone}-2-3"));
        let young = dir.path().join(format!(".issues.org.tmp.{gone}-5-6"));
        let board = dir.path().join("issues.org");
        let dotted = dir.path().join(".ljos-commit.pending");
        let bare = dir.path().join(".tmp.x");
        for path in [&old, &young, &board, &dotted, &bare] {
            #[expect(
                clippy::disallowed_methods,
                reason = "writes a file that is not a git hook"
            )]
            std::fs::write(path, "x").unwrap();
        }
        for path in [&old, &board, &dotted, &bare] {
            age(path, STALE_AFTER + Duration::from_secs(5));
        }
        assert_eq!(sweep_stale(dir.path()), 1);
        assert!(!old.exists());
        for path in [&young, &board, &dotted, &bare] {
            assert!(path.exists(), "{} was swept", path.display());
        }
    }

    /// A writer that is still running after an hour, stopped or slow or
    /// behind a clock step, keeps its temporary. So does one whose name
    /// carries no pid. A dead writer's goes.
    #[test]
    fn the_sweep_leaves_an_old_temporary_whose_writer_is_alive() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir
            .path()
            .join(format!(".issues.org.tmp.{}-1-0", std::process::id()));
        let init = dir.path().join(".issues.org.tmp.1-1-0");
        let vissue = dir
            .path()
            .join(format!(".issues.org.tmp.{}-7", std::process::id()));
        let unnamed = dir.path().join(".issues.org.tmp.x-1-0");
        let dead = dir
            .path()
            .join(format!(".issues.org.tmp.{}-1-0", dead_pid()));
        for path in [&mine, &init, &vissue, &unnamed, &dead] {
            #[expect(
                clippy::disallowed_methods,
                reason = "writes a file that is not a git hook"
            )]
            std::fs::write(path, "half a board").unwrap();
            age(path, STALE_AFTER * 3);
        }
        assert_eq!(sweep_stale(dir.path()), 1);
        for path in [&mine, &init, &vissue, &unnamed] {
            assert!(path.exists(), "{} was swept", path.display());
        }
        assert!(!dead.exists());
    }

    #[test]
    fn the_writer_pid_is_the_number_before_the_first_dash() {
        assert_eq!(temp_writer_pid(".issues.org.tmp.123-45-6"), Some(123));
        assert_eq!(temp_writer_pid(".pins.tmp.77-1"), Some(77));
        assert_eq!(temp_writer_pid(".pins.tmp.77"), Some(77));
        assert_eq!(temp_writer_pid(".pins.tmp.x-1"), None);
        assert_eq!(temp_writer_pid(".pins.tmp.0-1"), None);
        assert_eq!(temp_writer_pid(".pins.tmp.-1"), None);
        assert_eq!(temp_writer_pid("pins.tmp.1-1"), None);
    }

    #[test]
    fn temporary_names_are_dot_name_tmp_suffix() {
        assert!(is_temp_name(".issues.org.tmp.12-34"));
        assert!(is_temp_name(".pins.tmp.1"));
        assert!(!is_temp_name("issues.org.tmp.1"));
        assert!(!is_temp_name(".tmp.1"));
        assert!(!is_temp_name(".issues.org.tmp."));
        assert!(!is_temp_name(".issues.org"));
    }
}
