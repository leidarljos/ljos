//! File operations on a folder that may be a network or FUSE mount.
//!
//! `mkdir` on a mount that has gone away reports
//! `Transport endpoint is not connected` and returns. A caller that skips
//! the failed files still prints the seed count and exits 0. A copy onto
//! the same mount can sit in the kernel until the process is killed.
//!
//! Each call here waits at most [`wait_bound`] (`LJOS_SHARE_WAIT`, 60
//! seconds). The bytes go into a temporary file in the destination folder
//! and take the real name by rename, so a half-written seed is never the
//! file a later pull reads. A failure is one line: the folder, the
//! operating system's words for the error, and the command to run once
//! the mount answers.

use std::fs::OpenOptions;
use std::io::{Error as IoError, Result as IoResult, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{bail, Result};

/// How long one share-folder operation may sit when `LJOS_SHARE_WAIT` is unset.
const DEFAULT_WAIT: Duration = Duration::from_secs(60);

/// Which call a test fault applies to. A fault that fails every call cannot
/// reach the per-file stat that used to be skipped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Io,
    Rename,
    Entry,
}

/// A test seam. `Code` fails the call. `Slow` sits past the wait and does
/// not touch the file. `Rename` fails only the rename, after the temporary
/// file is written. `Entry` fails a file's stat inside a directory listing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    Code(i32),
    Slow,
    Rename(i32),
    Entry(i32),
}

#[cfg(test)]
thread_local! {
    static LOCAL_FAULT: std::cell::Cell<Fault> = const { std::cell::Cell::new(Fault::None) };
}

/// The folder a failure names, and the command the line tells the person to run.
pub(crate) struct Share {
    kind: &'static str,
    again: String,
}

impl Share {
    /// `ljos mesh pull` reading or writing under `from`.
    pub(crate) fn mesh_pull(seat: &str, from: &Path) -> Self {
        Self {
            kind: "mesh",
            again: format!("ljos mesh pull --seat {seat} --from {}", from.display()),
        }
    }

    /// `ljos mesh push` writing `<lane>.jsonl` into `to`.
    pub(crate) fn mesh_push(seat: &str, lane: &str, to: &Path) -> Self {
        Self {
            kind: "mesh",
            again: format!(
                "ljos mesh push --seat {seat} --lane {lane} --to {}",
                to.display()
            ),
        }
    }

    /// `ljos memory export` writing `out`.
    pub(crate) fn memory_export(out: &Path) -> Self {
        Self {
            kind: "memory",
            again: format!("ljos memory export --out {}", out.display()),
        }
    }

    /// `ljos memory import` reading `path`.
    pub(crate) fn memory_import(path: &Path) -> Self {
        Self {
            kind: "memory",
            again: format!("ljos memory import {}", path.display()),
        }
    }

    /// One line for `err` in `folder`. The operating system's number stays out.
    pub(crate) fn fail(&self, folder: &Path, err: IoError) -> anyhow::Error {
        anyhow::anyhow!(
            "{}: {}: {}. Check the mount, then rerun `{}`.",
            self.kind,
            folder.display(),
            words(&err),
            self.again
        )
    }

    pub(crate) fn symlink_meta(&self, path: &Path) -> IoResult<std::fs::Metadata> {
        let path = path.to_path_buf();
        io(Op::Io, move || std::fs::symlink_metadata(&path))
    }

    pub(crate) fn read(&self, path: &Path) -> IoResult<Vec<u8>> {
        let path = path.to_path_buf();
        io(Op::Io, move || std::fs::read(&path))
    }

    /// The file's bytes, or `None` when `path` is not a file. `is_file`
    /// follows a symlink, which is what a seed read already did, and the
    /// whole check sits inside the wait.
    pub(crate) fn read_if_file(&self, path: &Path) -> IoResult<Option<Vec<u8>>> {
        let path = path.to_path_buf();
        io(Op::Io, move || {
            if !path.is_file() {
                return Ok(None);
            }
            std::fs::read(&path).map(Some)
        })
    }

    pub(crate) fn create_dir(&self, path: &Path) -> IoResult<()> {
        let path = path.to_path_buf();
        io(Op::Io, move || std::fs::create_dir_all(&path))
    }

    /// Read the directory and its files, or the first error that is not a
    /// file vanishing between the listing and the stat.
    pub(crate) fn list_files(&self, dir: &Path) -> IoResult<Vec<PathBuf>> {
        let dir = dir.to_path_buf();
        io(Op::Io, move || list_files(&dir))
    }

    /// Exclusive lock on `path`. The descriptor is returned so the lock
    /// outlives the thread that took it.
    pub(crate) fn lock_shared(&self, path: &Path) -> IoResult<std::fs::File> {
        let path = path.to_path_buf();
        io(Op::Io, move || lock_open(&path, false))
    }

    /// Create `path` and take an exclusive lock on it.
    pub(crate) fn lock_create(&self, path: &Path) -> IoResult<std::fs::File> {
        let path = path.to_path_buf();
        io(Op::Io, move || lock_open(&path, true))
    }

    /// Write `bytes` to `path`. A temporary file in the same folder is
    /// renamed into place. The previous file stays until that rename.
    ///
    /// # Errors
    ///
    /// The folder did not accept the create, the write or the rename, or
    /// the call sat past the wait. `path` is left unchanged.
    pub(crate) fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let parent = parent_of(path);
        if let Err(err) = self.create_dir(&parent) {
            return Err(self.fail(&parent, err));
        }
        match self.symlink_meta(path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if self.kind == "memory" {
                    bail!("memory: {} is a symlink", path.display());
                }
                bail!(
                    "mesh: {} is a symlink. Remove the link so the lane file can be written",
                    path.display()
                );
            }
            Ok(_) => match self.read(path) {
                Ok(have) if have == bytes => return Ok(()),
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(self.fail(&parent, err)),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(self.fail(&parent, err)),
        }
        let tmp = temp_path(path);
        let tmp_write = tmp.clone();
        let body = bytes.to_vec();
        if let Err(err) = io(Op::Io, move || write_new(&tmp_write, &body)) {
            let tmp_rm = tmp.clone();
            let _ = io(Op::Io, move || std::fs::remove_file(&tmp_rm));
            return Err(self.fail(&parent, err));
        }
        let tmp_mv = tmp.clone();
        let dest = path.to_path_buf();
        if let Err(err) = io(Op::Rename, move || std::fs::rename(&tmp_mv, &dest)) {
            let tmp_rm = tmp;
            let _ = io(Op::Io, move || std::fs::remove_file(&tmp_rm));
            return Err(self.fail(&parent, err));
        }
        Ok(())
    }
}

/// Stat one listing entry. A transport error is returned. A file that
/// vanished is `NotFound`, which the caller skips. Skipping a transport
/// error used to import nothing, print the seed count and exit 0.
fn entry_meta(path: &Path) -> IoResult<std::fs::Metadata> {
    if let Some(err) = immediate(env_fault(), Op::Entry) {
        return Err(err);
    }
    std::fs::symlink_metadata(path)
}

fn parent_of(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn list_files(dir: &Path) -> IoResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
            continue;
        };
        if name.starts_with('.') || !name.ends_with(".jsonl") || name.ends_with(".seed.jsonl") {
            continue;
        }
        let meta = match entry_meta(&path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        files.push(path);
    }
    files.sort();
    Ok(files)
}

fn lock_open(path: &Path, create: bool) -> IoResult<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let mut opts = OpenOptions::new();
    opts.read(true);
    if create {
        opts.write(true).create(true).truncate(false).mode(0o600);
    }
    let file = opts.open(path)?;
    // SAFETY: flock on a descriptor this function owns and returns.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(IoError::last_os_error());
    }
    Ok(file)
}

fn write_new(path: &Path, bytes: &[u8]) -> IoResult<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_EXCL)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let mut name = path
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("share"))
        .to_os_string();
    name.push(format!(".{}.{}.tmp", std::process::id(), n));
    path.with_file_name(name)
}

fn words(err: &IoError) -> String {
    let text = err.to_string();
    match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => text[..at].to_string(),
        _ => text,
    }
}

fn wait_bound() -> Duration {
    match std::env::var("LJOS_SHARE_WAIT") {
        Ok(raw) => parse_wait(raw.trim()).unwrap_or(DEFAULT_WAIT),
        Err(_) => DEFAULT_WAIT,
    }
}

fn parse_wait(raw: &str) -> Option<Duration> {
    if raw.is_empty() {
        return None;
    }
    let secs: f64 = raw.parse().ok()?;
    if !secs.is_finite() || secs <= 0.0 || secs > 86_400.0 {
        return None;
    }
    Some(Duration::from_secs_f64(secs))
}

fn seconds_words(bound: Duration) -> String {
    let ms = bound.as_millis();
    if ms.is_multiple_of(1000) {
        return format!("{} s", ms / 1000);
    }
    let text = format!("{:.3}", bound.as_secs_f64());
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text} s")
}

fn caller_fault() -> Fault {
    #[cfg(test)]
    {
        let local = LOCAL_FAULT.with(|cell| cell.get());
        if local != Fault::None {
            return local;
        }
    }
    env_fault()
}

fn env_fault() -> Fault {
    #[cfg(any(test, feature = "test-hooks"))]
    {
        let Ok(raw) = std::env::var("LJOS_SHARE_FAULT") else {
            return Fault::None;
        };
        match raw.trim() {
            "enotconn" => Fault::Code(libc::ENOTCONN),
            "eio" => Fault::Code(libc::EIO),
            "estale" => Fault::Code(libc::ESTALE),
            "slow" => Fault::Slow,
            "rename" => Fault::Rename(libc::EIO),
            "stat" => Fault::Entry(libc::ENOTCONN),
            _ => Fault::None,
        }
    }
    #[cfg(not(any(test, feature = "test-hooks")))]
    Fault::None
}

fn immediate(fault: Fault, op: Op) -> Option<IoError> {
    match fault {
        Fault::Code(code) => Some(IoError::from_raw_os_error(code)),
        Fault::Rename(code) if op == Op::Rename => Some(IoError::from_raw_os_error(code)),
        Fault::Entry(code) if op == Op::Entry => Some(IoError::from_raw_os_error(code)),
        Fault::None | Fault::Slow | Fault::Rename(_) | Fault::Entry(_) => None,
    }
}

fn io<T: Send + 'static>(
    op: Op,
    work: impl FnOnce() -> IoResult<T> + Send + 'static,
) -> IoResult<T> {
    let fault = caller_fault();
    if let Some(err) = immediate(fault, op) {
        return Err(err);
    }
    let bound = wait_bound();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("ljos-share".to_string())
        .spawn(move || {
            let result = if matches!(fault, Fault::Slow) {
                // The caller stops at `bound`. This thread must not write
                // the file after that, or a reported failure would still land.
                std::thread::sleep(bound + Duration::from_millis(250));
                Err(IoError::new(std::io::ErrorKind::TimedOut, "slow"))
            } else {
                work()
            };
            let _ = tx.send(result);
        })?;
    match rx.recv_timeout(bound) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(IoError::new(
            std::io::ErrorKind::TimedOut,
            format!("the folder did not answer within {}", seconds_words(bound)),
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(IoError::other("the folder read stopped"))
        }
    }
}

#[cfg(test)]
struct FaultArm;

#[cfg(test)]
impl FaultArm {
    fn arm(fault: Fault) -> Self {
        LOCAL_FAULT.with(|cell| cell.set(fault));
        Self
    }

    fn code(code: i32) -> Self {
        Self::arm(Fault::Code(code))
    }

    fn rename(code: i32) -> Self {
        Self::arm(Fault::Rename(code))
    }
}

#[cfg(test)]
impl Drop for FaultArm {
    fn drop(&mut self) {
        LOCAL_FAULT.with(|cell| cell.set(Fault::None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn share(dir: &Path) -> Share {
        Share::mesh_pull("box", dir)
    }

    #[test]
    fn the_line_uses_words_and_names_the_next_pull() {
        let dir = Path::new("/srv/handoff");
        let err = share(dir).fail(dir, IoError::from_raw_os_error(libc::ENOTCONN));
        let line = err.to_string();
        assert_eq!(
            line,
            "mesh: /srv/handoff: Transport endpoint is not connected. Check the mount, then rerun `ljos mesh pull --seat box --from /srv/handoff`."
        );
        assert!(!line.contains("os error"), "{line}");
        assert!(!line.contains('\n'), "{line}");
    }

    #[test]
    fn a_write_lands_by_rename_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seed.jsonl");
        share(dir.path()).write_atomic(&path, b"whole\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"whole\n");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["seed.jsonl"]);
    }

    #[test]
    fn a_failed_rename_keeps_the_previous_seed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seed.jsonl");
        let place = share(dir.path());
        place.write_atomic(&path, b"old\n").unwrap();
        let _fault = FaultArm::rename(libc::EIO);
        let err = place.write_atomic(&path, b"new\n").unwrap_err();
        drop(_fault);
        let line = err.to_string();
        assert!(line.contains("Input/output error"), "{line}");
        assert!(line.contains("Check the mount, then rerun"), "{line}");
        assert!(!line.contains("os error"), "{line}");
        assert_eq!(std::fs::read(&path).unwrap(), b"old\n");
        let temps: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(temps.is_empty(), "{temps:?}");
    }

    #[test]
    fn an_injected_transport_error_does_not_replace_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seed.jsonl");
        std::fs::write(&path, b"old\n").unwrap();
        let _fault = FaultArm::code(libc::ENOTCONN);
        let err = share(dir.path()).write_atomic(&path, b"new\n").unwrap_err();
        drop(_fault);
        assert!(
            err.to_string()
                .contains("Transport endpoint is not connected"),
            "{err}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"old\n");
    }
}
