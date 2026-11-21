//! Whole-file tracker writes through a temporary that cleans up after itself.
//!
//! A board or pin file is replaced by writing `.NAME.tmp.PID-NANOS-SEQ`
//! beside it and renaming that over it. A write that fails, panics or
//! returns early drops the guard, and the drop removes the temporary. A
//! process killed between the write and the rename leaves one behind;
//! [`sweep_stale`] removes those once they are an hour old.

use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A temporary younger than this may belong to a writer that is still
/// running, so the sweep leaves it.
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
    fn beside(target: &Path) -> std::io::Result<(Self, std::fs::File)> {
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let base = target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".{base}.tmp.{}-{nanos}-{seq}", std::process::id()));
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

/// Remove temporaries in `dir` older than [`STALE_AFTER`]. Returns how
/// many went. A file that cannot be read or removed is left.
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
        if !name.to_str().is_some_and(is_temp_name) {
            continue;
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
mod tests {
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
        std::fs::write(&path, "old\n").unwrap();
        replace_file(&path, "new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn a_failed_rename_step_leaves_the_old_file_and_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issues.org");
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
        std::fs::write(path.join("keep"), "x").unwrap();
        assert!(replace_file(&path, "new\n").is_err());
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn the_sweep_takes_old_temporaries_and_leaves_young_ones_and_others() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join(".issues.org.tmp.1-2-3");
        let young = dir.path().join(".issues.org.tmp.4-5-6");
        let board = dir.path().join("issues.org");
        let dotted = dir.path().join(".ljos-commit.pending");
        let bare = dir.path().join(".tmp.x");
        for path in [&old, &young, &board, &dotted, &bare] {
            std::fs::write(path, "x").unwrap();
        }
        let hour_ago = SystemTime::now() - STALE_AFTER - Duration::from_secs(5);
        for path in [&old, &board, &dotted, &bare] {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(hour_ago)
                .unwrap();
        }
        assert_eq!(sweep_stale(dir.path()), 1);
        assert!(!old.exists());
        for path in [&young, &board, &dotted, &bare] {
            assert!(path.exists(), "{} was swept", path.display());
        }
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
