//! One publisher per Git object store, including linked worktrees.

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

thread_local! {
    static ACTIVE: RefCell<Option<Arc<File>>> = const { RefCell::new(None) };
}

/// A canonical object-store directory, independent of the caller's subdirectory.
pub(crate) fn common_dir(start: &Path) -> Result<PathBuf, String> {
    let mut cmd = crate::git_at(start);
    cmd.args(["rev-parse", "--git-common-dir"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::git_hooks::hook_child_env(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("publisher: Git directory: {e}"))?;
    if !out.status.success() {
        return Err("publisher: cannot resolve the Git common directory".into());
    }
    let text = std::str::from_utf8(&out.stdout)
        .map_err(|_| "publisher: Git directory is not UTF-8".to_string())?;
    let path = Path::new(text.trim());
    if path.as_os_str().is_empty() {
        return Err("publisher: Git common directory is empty".into());
    }
    std::fs::canonicalize(start.join(path))
        .map_err(|e| format!("publisher: cannot canonicalize the Git directory: {e}"))
}

/// Closing the final descriptor releases ownership. No explicit unlock is used:
/// a push child can retain the same open file description after its caller exits.
pub(crate) struct Publication {
    _file: Arc<File>,
    previous: Option<Arc<File>>,
    common: PathBuf,
}

impl Publication {
    pub(crate) fn common_dir(&self) -> &Path {
        &self.common
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        ACTIVE.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}

/// Refuse a second publisher and preserve its caller's queued commit state.
pub(crate) fn acquire(start: &Path) -> Result<Publication, String> {
    acquire_common(&common_dir(start)?)
}

fn acquire_common(common: &Path) -> Result<Publication, String> {
    // This name is not a Git *.lock file and is never unlinked for release.
    let path = common.join("ljos-publisher");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| format!("publisher: cannot open {}: {e}", path.display()))?;
    // The command's stdio setup cannot replace the ownership descriptor.
    // SAFETY: fcntl duplicates a descriptor owned here and returns a new one.
    let duplicate = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(format!(
            "publisher: cannot retain ownership descriptor: {}",
            io::Error::last_os_error()
        ));
    }
    drop(file);
    // SAFETY: duplicate is a successful new descriptor and has no other owner.
    let file = unsafe { File::from_raw_fd(duplicate) };
    if !file
        .metadata()
        .map_err(|e| format!("publisher: cannot inspect {}: {e}", path.display()))?
        .is_file()
    {
        return Err("publisher: ownership path is not a regular file".into());
    }
    // SAFETY: file owns this descriptor; flock does not access Rust memory.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            return Err(
                "publisher busy: another push of this object store is running; commits queued"
                    .into(),
            );
        }
        return Err(format!(
            "publisher: cannot lock {}: {error}",
            path.display()
        ));
    }
    let file = Arc::new(file);
    let previous = ACTIVE.with(|slot| slot.replace(Some(Arc::clone(&file))));
    Ok(Publication {
        _file: file,
        previous,
        common: common.to_path_buf(),
    })
}

/// Keep publication ownership in this command and its push children.
///
/// Only this command clears close-on-exec; unrelated worker threads retain
/// the standard library's descriptor isolation.
pub(crate) fn inherit(cmd: &mut Command) {
    let file = ACTIVE.with(|slot| slot.borrow().as_ref().map(Arc::clone));
    let Some(file) = file else {
        return;
    };
    // SAFETY: the child closure only reads an owned descriptor and calls fcntl.
    // It does not allocate, inspect the environment, or acquire a Rust lock.
    unsafe {
        cmd.pre_exec(move || {
            let fd = file.as_raw_fd();
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Barrier,
    };

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let status = crate::git_cmd()
            .args(["init", "-q"])
            .arg(root.path())
            .status()
            .unwrap();
        assert!(status.success());
        root
    }

    fn run_isolated_lock_case(name: &str) -> bool {
        const CASE: &str = "LJOS_TEST_ISOLATED_LOCK_CASE";
        if std::env::var_os(CASE).as_deref() == Some(std::ffi::OsStr::new(name)) {
            return false;
        }
        let _environment = crate::test_env_lock();
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--test-threads=1", "--nocapture"])
            .env(CASE, name)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let errors = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{said}\n{errors}");
        assert!(
            said.lines().any(|line| line
                .starts_with("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;")),
            "{said}\n{errors}"
        );
        true
    }

    #[test]
    fn project_subdirectories_and_symlinks_share_one_publication_owner() {
        if run_isolated_lock_case(concat!(
            "tracker_publish_lock::tests::",
            "project_subdirectories_and_symlinks_share_one_publication_owner"
        )) {
            return;
        }
        let _environment = crate::test_env_lock();
        let root = repository();
        let first = root.path().join("Software/First");
        let second = root.path().join("Software/Second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        assert_eq!(common_dir(&first).unwrap(), common_dir(&second).unwrap());
        assert_eq!(common_dir(&alias).unwrap(), common_dir(&second).unwrap());
        let held = acquire(&first).unwrap();
        assert!(acquire(&second).is_err());
        assert!(acquire(&alias).is_err());
        drop(held);
        assert!(acquire(&second).is_ok());
    }

    #[test]
    fn linked_worktrees_share_the_object_stores_publication_owner() {
        if run_isolated_lock_case(concat!(
            "tracker_publish_lock::tests::",
            "linked_worktrees_share_the_object_stores_publication_owner"
        )) {
            return;
        }
        let _environment = crate::test_env_lock();
        let root = repository();
        let linked_parent = tempfile::tempdir().unwrap();
        let linked = linked_parent.path().join("linked");
        for args in [
            vec!["config", "user.name", "publication test"],
            vec!["config", "user.email", "publication@example.invalid"],
            vec!["commit", "-q", "--allow-empty", "-m", "seed"],
            vec![
                "worktree",
                "add",
                "-q",
                "--detach",
                linked.to_str().unwrap(),
            ],
        ] {
            let status = crate::git_at(root.path()).args(args).status().unwrap();
            assert!(status.success());
        }
        assert_eq!(
            common_dir(root.path()).unwrap(),
            common_dir(&linked).unwrap()
        );
        let _held = acquire(root.path()).unwrap();
        assert!(acquire(&linked).is_err());
    }

    #[test]
    fn simultaneous_publishers_cannot_both_enter() {
        if run_isolated_lock_case(concat!(
            "tracker_publish_lock::tests::",
            "simultaneous_publishers_cannot_both_enter"
        )) {
            return;
        }
        let common = tempfile::tempdir().unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let held = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let barrier = Arc::clone(&barrier);
                let held = Arc::clone(&held);
                let common = common.path();
                scope.spawn(move || {
                    barrier.wait();
                    let ownership = acquire_common(common);
                    if ownership.is_ok() {
                        held.fetch_add(1, Ordering::SeqCst);
                    } else {
                        assert!(ownership
                            .as_ref()
                            .err()
                            .unwrap()
                            .starts_with("publisher busy:"));
                    }
                    // The owner retains its lock until every contender has tried.
                    barrier.wait();
                    drop(ownership);
                });
            }
        });
        assert_eq!(held.load(Ordering::SeqCst), 1);
        assert!(acquire_common(common.path()).is_ok());
    }

    #[test]
    fn explicit_and_implicit_publish_paths_share_the_same_owner() {
        if run_isolated_lock_case(concat!(
            "tracker_publish_lock::tests::",
            "explicit_and_implicit_publish_paths_share_the_same_owner"
        )) {
            return;
        }
        let _environment = crate::test_env_lock();
        let root = repository();
        let _held = acquire(root.path()).unwrap();
        for result in [
            crate::tracker_push::publish_explicit(root.path(), "origin", "main"),
            crate::tracker_push::publish(root.path()),
            crate::tracker_push::push_checkout(root.path()),
        ] {
            assert!(result.unwrap_err().starts_with("publisher busy:"));
        }
    }

    #[test]
    fn a_child_retains_ownership_after_the_callers_guard_is_dropped() {
        // A fresh process keeps sibling test-worker forks out of this ownership check.
        const CHILD: &str = "LJOS_TEST_PUBLICATION_CHILD";
        if std::env::var_os(CHILD).as_deref() != Some(std::ffi::OsStr::new("1")) {
            let _environment = crate::test_env_lock();
            let out = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    concat!(
                        "tracker_publish_lock::tests::",
                        "a_child_retains_ownership_after_the_callers_guard_is_dropped"
                    ),
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            let said = String::from_utf8_lossy(&out.stdout);
            let errors = String::from_utf8_lossy(&out.stderr);
            assert!(out.status.success(), "{said}\n{errors}");
            assert!(
                said.lines().any(|line| line
                    .starts_with("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;")),
                "{said}\n{errors}"
            );
            return;
        }
        let common = tempfile::tempdir().unwrap();
        let held = acquire_common(common.path()).unwrap();
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "printf 'holding\\n'; read release"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        inherit(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        drop(cmd);
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "holding\n");
        drop(held);
        let busy = acquire_common(common.path()).is_err();
        writeln!(child.stdin.take().unwrap(), "release").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(busy);
        assert!(acquire_common(common.path()).is_ok());
    }

    #[test]
    fn a_publisher_retains_ownership_after_its_caller_process_exits() {
        if run_isolated_lock_case(concat!(
            "tracker_publish_lock::tests::",
            "a_publisher_retains_ownership_after_its_caller_process_exits"
        )) {
            return;
        }
        use std::ffi::CString;
        use std::io::Read;
        use std::os::unix::ffi::OsStrExt;
        let common = tempfile::tempdir().unwrap();
        let fifo = common.path().join("release");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: fifo_c is a valid NUL-terminated pathname in this test's directory.
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        let held = acquire_common(common.path()).unwrap();
        let script = format!(
            r#"sh -c 'printf "holding\n"; read release < "$1"; printf "released\n"' sh {} &
exit 0"#,
            crate::tools::sh_quote(&fifo.display().to_string()),
        );
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &script]).stdout(Stdio::piped());
        inherit(&mut cmd);
        let mut caller = cmd.spawn().unwrap();
        drop(cmd);
        drop(held);
        let exited = caller.wait().unwrap();
        let mut reader = BufReader::new(caller.stdout.take().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let busy = acquire_common(common.path()).is_err();
        let mut release = OpenOptions::new().write(true).open(&fifo).unwrap();
        writeln!(release, "release").unwrap();
        drop(release);
        let mut tail = String::new();
        reader.read_to_string(&mut tail).unwrap();
        assert!(exited.success());
        assert_eq!(line, "holding\n");
        assert_eq!(tail, "released\n");
        assert!(busy);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match acquire_common(common.path()) {
                Ok(_owner) => break,
                Err(error) if std::time::Instant::now() < deadline => {
                    assert!(error.starts_with("publisher busy:"));
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("publisher ownership remained after exit: {error}"),
            }
        }
    }

    #[test]
    fn an_unrelated_command_does_not_inherit_publication_ownership() {
        if run_isolated_lock_case(concat!(
            "tracker_publish_lock::tests::",
            "an_unrelated_command_does_not_inherit_publication_ownership"
        )) {
            return;
        }
        let common = tempfile::tempdir().unwrap();
        let held = acquire_common(common.path()).unwrap();
        let mut child = Command::new("sh")
            .args(["-c", "printf 'running\\n'; read release"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "running\n");
        drop(held);
        let available = acquire_common(common.path()).is_ok();
        writeln!(child.stdin.take().unwrap(), "release").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(available);
    }

    #[test]
    fn a_symlink_at_the_ownership_path_is_refused() {
        if run_isolated_lock_case(concat!(
            "tracker_publish_lock::tests::",
            "a_symlink_at_the_ownership_path_is_refused"
        )) {
            return;
        }
        let common = tempfile::tempdir().unwrap();
        let target = common.path().join("target");
        std::fs::write(&target, "unchanged").unwrap();
        std::os::unix::fs::symlink(&target, common.path().join("ljos-publisher")).unwrap();
        assert!(acquire_common(common.path()).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "unchanged");
    }
}
