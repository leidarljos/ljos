//! Publish an executable without leaving a write descriptor in this process.
//!
//! A descriptor opened here is copied into a child at fork and stays open
//! until that child reaches exec. Starting the new file in that window fails
//! with ETXTBSY ("Text file busy"). The bytes are written by `/bin/sh`, which
//! exits before this process renames the temporary name into place, so a fork
//! here never sees that descriptor.

use std::io::{Error, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Write `body` to `path` and mark it executable (`0o755`).
///
/// # Errors
///
/// The directory cannot be written, or the short-lived writer fails.
pub fn install_executable(path: &Path, body: impl AsRef<[u8]>) -> Result<(), Error> {
    install_executable_mode(path, body, 0o755)
}

/// Write `body` to `path` and mark it with `mode`.
///
/// `mode` is the permission bits passed to `chmod`, masked to `0o7777`.
///
/// # Errors
///
/// The directory cannot be written, or the short-lived writer fails.
pub fn install_executable_mode(
    path: &Path,
    body: impl AsRef<[u8]>,
    mode: u32,
) -> Result<(), Error> {
    if let Some(parent) = dir_of(path) {
        std::fs::create_dir_all(parent)?;
    }
    let mode = mode & 0o7777;
    #[cfg(unix)]
    {
        let tmp = temp_name(path)?;
        let _ = std::fs::remove_file(&tmp);
        let script = format!("/bin/cat > \"$1\" && /bin/chmod {mode:o} \"$1\"");
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(&script)
            .arg("install-executable")
            .arg(&tmp)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let write_in = (|| {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| Error::other("install executable: writer has no stdin"))?;
            stdin.write_all(body.as_ref())?;
            stdin.flush()?;
            Ok(())
        })();
        if let Err(err) = write_in {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&tmp);
            return Err(err);
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(writer_failed(&out.status, &out.stderr));
        }
        if let Ok(file) = std::fs::File::open(&tmp) {
            let _ = file.sync_all();
        }
        if let Err(err) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(err);
        }
        if let Some(parent) = dir_of(path) {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        std::fs::write(path, body.as_ref())
    }
}

/// Copy `src` onto `dest` as an executable.
///
/// `cp` runs in another process and exits before the rename, so this process
/// never has `dest` open for writing.
///
/// # Errors
///
/// The source cannot be read, or the destination directory cannot be written.
pub fn copy_executable(src: &Path, dest: &Path) -> Result<(), Error> {
    if let Some(parent) = dir_of(dest) {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        let tmp = temp_name(dest)?;
        let _ = std::fs::remove_file(&tmp);
        let out = Command::new("/bin/cp")
            .arg(src)
            .arg(&tmp)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(writer_failed(&out.status, &out.stderr));
        }
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(err) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            {
                let _ = std::fs::remove_file(&tmp);
                return Err(err);
            }
        }
        if let Err(err) = std::fs::rename(&tmp, dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err(err);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::copy(src, dest)?;
        Ok(())
    }
}

fn dir_of(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Some(dir),
        _ => None,
    }
}

fn temp_name(path: &Path) -> Result<PathBuf, Error> {
    let parent = dir_of(path).unwrap_or_else(|| Path::new("."));
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".ljos-exe.{}.{n}", std::process::id())))
}

fn writer_failed(status: &std::process::ExitStatus, stderr: &[u8]) -> Error {
    let detail = String::from_utf8_lossy(stderr).trim().to_string();
    let msg = if detail.is_empty() {
        format!("install executable: writer exited {status}")
    } else {
        format!("install executable: {detail}")
    };
    Error::other(msg)
}

/// Linux and macOS both use 26 for ETXTBSY.
#[cfg(test)]
fn text_file_busy(err: &Error) -> bool {
    err.kind() == std::io::ErrorKind::ExecutableFileBusy
        || err.raw_os_error() == Some(26)
        || err
            .to_string()
            .to_ascii_lowercase()
            .contains("text file busy")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn a_published_stub_has_its_bytes_and_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stub");
        install_executable(&path, b"#!/bin/sh\necho hi\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"#!/bin/sh\necho hi\n");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
        let out = Command::new(&path).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, b"hi\n");
    }

    #[test]
    fn a_copied_executable_runs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        install_executable(&src, b"#!/bin/sh\necho copied\n").unwrap();
        let dest = dir.path().join("dest");
        copy_executable(&src, &dest).unwrap();
        let out = Command::new(&dest).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, b"copied\n");
    }

    /// Many threads publish one stub and start it. A writer that leaves a
    /// descriptor in this process makes `exec` and the next write fail with
    /// ETXTBSY. Both counts are zero when the publisher is the short-lived
    /// process above.
    #[test]
    #[expect(
        clippy::disallowed_macros,
        reason = "a test writes its notes to the test harness, not to a person"
    )]
    fn concurrent_publish_then_start_is_not_text_busy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sibling-stub");
        install_executable(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        let exec_busy = Arc::new(AtomicUsize::new(0));
        let write_busy = Arc::new(AtomicUsize::new(0));
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let path = path.clone();
            let exec_busy = Arc::clone(&exec_busy);
            let write_busy = Arc::clone(&write_busy);
            let spawns = Arc::clone(&spawns);
            threads.push(thread::spawn(move || {
                for _ in 0..40 {
                    match install_executable(&path, b"#!/bin/sh\nexit 0\n") {
                        Ok(()) => {}
                        Err(err) if text_file_busy(&err) => {
                            write_busy.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        Err(err) => panic!("{err}"),
                    }
                    spawns.fetch_add(1, Ordering::Relaxed);
                    match Command::new(&path).output() {
                        Ok(out) if out.status.success() => {}
                        Err(err) if text_file_busy(&err) => {
                            exec_busy.fetch_add(1, Ordering::Relaxed);
                        }
                        other => panic!("{other:?}"),
                    }
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let exec_busy = exec_busy.load(Ordering::Relaxed);
        let write_busy = write_busy.load(Ordering::Relaxed);
        let spawns = spawns.load(Ordering::Relaxed);
        eprintln!(
            "concurrent publish: spawns={spawns} exec_etxtbsy={exec_busy} write_etxtbsy={write_busy}"
        );
        assert_eq!(
            exec_busy + write_busy,
            0,
            "exec_etxtbsy={exec_busy} write_etxtbsy={write_busy} spawns={spawns}"
        );
    }
}
