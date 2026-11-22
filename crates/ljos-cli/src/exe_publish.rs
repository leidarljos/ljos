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
        let stage = Stage::new(path)?;
        let tmp = stage.file();
        let script = format!("set -C && /bin/cat > \"$1\" && /bin/chmod {mode:o} \"$1\"");
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
            return Err(err);
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            return Err(writer_failed(&out.status, &out.stderr));
        }
        stage.settle(path)
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
        let stage = Stage::new(dest)?;
        let tmp = stage.file();
        let out = Command::new("/bin/cp")
            .arg("--")
            .arg(src)
            .arg(&tmp)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if !out.status.success() {
            return Err(writer_failed(&out.status, &out.stderr));
        }
        stage.regular()?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
                .map_err(|err| named(err, "mark executable", &tmp))?;
        }
        stage.settle(dest)
    }
    #[cfg(not(unix))]
    {
        std::fs::copy(src, dest)?;
        Ok(())
    }
}

/// A directory of this process's own, made beside the destination, that the
/// writer fills before the rename.
///
/// `mkdir` creates the name or fails, and never follows a symlink planted
/// there. Its mode is `0o700`, so no other user can put a name inside it
/// for the writer's redirect, `chmod` or `cp` to follow.
#[cfg(unix)]
struct Stage {
    dir: PathBuf,
}

#[cfg(unix)]
impl Stage {
    fn new(path: &Path) -> Result<Self, Error> {
        use std::hash::{BuildHasher, Hasher};
        use std::os::unix::fs::DirBuilderExt;
        let parent = dir_of(path).unwrap_or_else(|| Path::new("."));
        let mut last = None;
        for _ in 0..16 {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
            hasher.write_u32(std::process::id());
            let dir = parent.join(format!(".ljos-exe.{:016x}", hasher.finish()));
            match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => return Ok(Self { dir }),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => last = Some(err),
                Err(err) => return Err(named(err, "create a temporary directory in", parent)),
            }
        }
        let err = last.unwrap_or_else(|| Error::other("no free name"));
        Err(named(err, "create a temporary directory in", parent))
    }

    fn file(&self) -> PathBuf {
        self.dir.join("exe")
    }

    /// The writer left a regular file, not a link to somewhere else.
    fn regular(&self) -> Result<(), Error> {
        let tmp = self.file();
        let meta = std::fs::symlink_metadata(&tmp).map_err(|err| named(err, "read", &tmp))?;
        if meta.file_type().is_file() {
            Ok(())
        } else {
            Err(Error::other(format!(
                "install executable: {} is not a regular file",
                tmp.display()
            )))
        }
    }

    /// Flush the closed temporary file, rename it onto `path`, then flush
    /// the directory so the new name survives a crash.
    fn settle(self, path: &Path) -> Result<(), Error> {
        self.regular()?;
        let tmp = self.file();
        let _ = flush(&tmp);
        std::fs::rename(&tmp, path).map_err(|err| named(err, "rename onto", path))?;
        if let Some(parent) = dir_of(path) {
            let _ = flush(parent);
        }
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.file());
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// `err` with what was being done and the path it was done to.
#[cfg(unix)]
fn named(err: Error, what: &str, path: &Path) -> Error {
    Error::new(
        err.kind(),
        format!("install executable: {what} {}: {err}", path.display()),
    )
}

#[cfg(test)]
thread_local! {
    static FLUSHED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `sync_all` on `path`, a file or a directory. `settle` drops the error:
/// the rename has already happened or is about to, and a flush cannot
/// undo it.
#[cfg(unix)]
fn flush(path: &Path) -> Result<(), Error> {
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(test)]
    FLUSHED.with(|done| done.borrow_mut().push(path.to_path_buf()));
    Ok(())
}

fn dir_of(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Some(dir),
        _ => None,
    }
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

    /// A relative source can start with a dash. `cp` has to read it as a
    /// file, not an option. The working directory is the crate's.
    #[test]
    fn a_source_named_like_an_option_is_copied() {
        struct Gone(PathBuf);
        impl Drop for Gone {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let src = Gone(PathBuf::from(format!(
            "--ljos-copy-src-{}",
            std::process::id()
        )));
        install_executable(&src.0, b"#!/bin/sh\necho dashed\n").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        copy_executable(&src.0, &dest).unwrap();
        let out = Command::new(&dest).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, b"dashed\n");
    }

    /// The temporary file is flushed before the rename and the directory
    /// after it, for both a written and a copied executable.
    #[test]
    fn a_publish_flushes_the_file_then_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dest = dir.path().join("dest");
        for step in ["install", "copy"] {
            FLUSHED.with(|done| done.borrow_mut().clear());
            if step == "install" {
                install_executable(&src, b"#!/bin/sh\necho copied\n").unwrap();
            } else {
                copy_executable(&src, &dest).unwrap();
            }
            let done = FLUSHED.with(|done| done.borrow().clone());
            assert_eq!(done.len(), 2, "{step} flushed {done:?}");
            let stage = done[0].parent().unwrap();
            let name = stage.file_name().unwrap().to_string_lossy();
            assert!(name.starts_with(".ljos-exe."), "{step} flushed {done:?}");
            assert_eq!(stage.parent(), Some(dir.path()), "{step}");
            assert!(!stage.exists(), "{step} left {}", stage.display());
            assert_eq!(done[1], dir.path(), "{step} flushed {done:?}");
        }
    }

    /// Another user can leave symlinks at temporary names in a directory
    /// the publisher may create in but not unlink from. A publisher that
    /// writes or chmods a guessable name there rewrites the link's target.
    /// A directory this process cannot remove names from stands in for
    /// that one: every name the old `.ljos-exe.<pid>.<n>` scheme could pick
    /// links to `victim`.
    #[test]
    #[expect(
        clippy::disallowed_macros,
        reason = "a test writes its notes to the test harness, not to a person"
    )]
    fn a_planted_temp_symlink_is_not_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "SECRET").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
        let src = dir.path().join("src");
        install_executable(&src, b"#!/bin/sh\necho copied\n").unwrap();
        let shared = dir.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        let pid = std::process::id();
        for n in 0..NEXT.load(Ordering::Relaxed) + 4096 {
            std::os::unix::fs::symlink(&victim, shared.join(format!(".ljos-exe.{pid}.{n}")))
                .unwrap();
        }
        let lock = |mode| {
            std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        lock(0o555);
        if std::fs::File::create(shared.join("probe")).is_ok() {
            lock(0o755);
            eprintln!("skipped: this user writes into a 0555 directory");
            return;
        }
        let dest = shared.join("stub");
        let install = install_executable(&dest, b"#!/bin/sh\necho PWNED\n");
        let copy = copy_executable(&src, &dest);
        lock(0o755);
        let body = std::fs::read_to_string(&victim).unwrap();
        let mode = std::fs::metadata(&victim).unwrap().permissions().mode() & 0o7777;
        assert_eq!((body.as_str(), mode), ("SECRET", 0o644), "victim rewritten");
        assert!(!dest.exists());
        for (step, got) in [("install", install), ("copy", copy)] {
            let err = got.expect_err(step).to_string();
            assert!(
                err.contains(&shared.display().to_string()),
                "{step} error does not name the directory: {err}"
            );
        }
    }

    /// `/dev/null` opens but cannot be synced, so a flush that skips the
    /// sync returns Ok here.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_flush_reaches_the_sync() {
        let err = flush(Path::new("/dev/null")).unwrap_err();
        assert_eq!(err.raw_os_error(), Some(22), "{err}");
        assert!(flush(Path::new("/")).is_ok());
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

    /// Files under `crates/` that may write an executable in this process,
    /// each with the reason it has to.
    const IN_PROCESS_EXECUTABLES: &[(&str, &str)] = &[];

    /// Writes in a function that names a git hook whose file name the hook
    /// scan cannot see: file under `crates/`, function, target, and why it
    /// is not a hook.
    const NOT_A_HOOK: &[(&str, &str, &str, &str)] = &[
        (
            "ljos-cli/src/lib.rs",
            "finish",
            "issue",
            "an issue id; \"update\" here is a vissue verb",
        ),
        (
            "ljos-cli/src/lib.rs",
            "finish",
            "&deed_paths",
            "files the tracker lock guards; \"update\" here is a vissue verb",
        ),
        (
            "ljos-cli/src/lib.rs",
            "workflow_start",
            "&id",
            "an issue id for the tracker lock; \"update\" here is a vissue verb",
        ),
        (
            "ljos-cli/src/lib.rs",
            "workflow_start",
            "parent",
            "an issue id for the tracker lock; \"update\" here is a vissue verb",
        ),
        (
            "ljos-cli/src/lib.rs",
            "reopen",
            "issue",
            "an issue id for the tracker lock; \"update\" here is a vissue verb",
        ),
        (
            "ljos-cli/src/lib.rs",
            "bump_plan",
            "&first.id",
            "an issue id for the tracker lock; \"update\" here is a vissue verb",
        ),
        (
            "ljos-cli/src/offboard.rs",
            "finish",
            "&state.bytes",
            "store_backup names the backup after the sha256 of the bytes",
        ),
        (
            "ljos-cli/src/main.rs",
            "run_hook",
            "&input",
            "relay names an openhands-started marker after the session id",
        ),
        (
            "ljos-cli/tests/gate_identity.rs",
            "a_tmux_pane_is_not_judged_as_the_last_conversation_to_name_tmux",
            "&file",
            "the {who}.sh script",
        ),
        (
            "ljos-cli/tests/gate_identity.rs",
            "a_tmux_pane_is_not_judged_as_the_last_conversation_to_name_tmux",
            "&x_go",
            "the x.go file",
        ),
        (
            "ljos-cli/tests/gate_identity.rs",
            "a_tmux_pane_is_not_judged_as_the_last_conversation_to_name_tmux",
            "&z_go",
            "the z.go file",
        ),
        (
            "ljos-cli/tests/gate_identity.rs",
            "one_shot_bash_c_commit_from_unstamped_parent_is_allowed_when_sitting_is_live",
            "&hold",
            "a hold- file under the run directory",
        ),
        (
            "ljos-cli/tests/mail_wake_push.rs",
            "world",
            "&gitconfig",
            "the gitconfig file",
        ),
    ];

    /// A file written in this process and then given an exec bit, or a git
    /// hook rewritten where it lies, can be started while a forked child
    /// still has it open. Each of those goes through this module instead.
    #[test]
    fn executables_are_published_through_this_module() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut files = Vec::new();
        rust_sources(crates, &mut files);
        assert!(files.len() > 100, "found {} sources", files.len());
        let mut hits = Vec::new();
        let mut sources = Vec::new();
        for file in &files {
            let rel = file.strip_prefix(crates).unwrap().to_string_lossy();
            if rel.ends_with("exe_publish.rs")
                || IN_PROCESS_EXECUTABLES.iter().any(|(path, _)| rel == *path)
            {
                continue;
            }
            let text = std::fs::read_to_string(file).unwrap();
            for line in write_then_exec(&text) {
                hits.push(format!("crates/{rel}:{line}"));
            }
            sources.push((rel.to_string(), text));
        }
        let mut used = vec![false; NOT_A_HOOK.len()];
        for hit in hook_writes(&sources) {
            let path = &sources[hit.file].0;
            let known = NOT_A_HOOK.iter().position(|(file, func, target, _)| {
                file == path && *func == hit.func && *target == hit.target
            });
            match known {
                Some(n) => used[n] = true,
                None => {
                    hits.push(format!("crates/{path}:{} {}", hit.line, hit.target));
                    if let Some((file, line)) = hit.writer {
                        hits.push(format!("crates/{}:{line} writes it", sources[file].0));
                    }
                }
            }
        }
        assert!(
            hits.is_empty(),
            "publish these with install_executable, install_executable_mode or copy_executable:\n{}",
            hits.join("\n")
        );
        let stale: Vec<_> = NOT_A_HOOK
            .iter()
            .zip(&used)
            .filter(|(_, used)| !**used)
            .collect();
        assert!(
            stale.is_empty(),
            "NOT_A_HOOK entries no write matches: {stale:?}"
        );
    }

    #[test]
    fn the_site_scan_names_a_write_then_chmod_and_a_hook_rewrite() {
        let chmod = r##"
fn stub(bin: &Path) {
    fs::write(bin.join("a"), "#!/bin/sh\n").unwrap();
    fs::write(bin.join("conf"), "x").unwrap();
    for name in ["a"] {
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
}
fn other(dest: &Path) {
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)).unwrap();
}
"##;
        assert_eq!(write_then_exec(chmod), vec![3]);
        let open = r#"
fn server(path: &Path) {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(&path, perms).unwrap();
}
"#;
        assert_eq!(write_then_exec(open), vec![3]);
        let hook = r#"
fn strip(dir: &Path) {
    for name in HOOK_NAMES {
        let path = dir.join(name);
        fs::write(&path, "x")?;
    }
}
"#;
        assert_eq!(hook_lines(hook), vec![5]);
        let named = r#"
fn rewrite(ours: &Path, share: &Path) {
    std::fs::write(ours.join("pre-push"), &stale).unwrap();
    let path = share.join("ljos/git-hooks/pre-push");
    let stub = fs::read_to_string(&path).unwrap();
    fs::write(&path, stub).unwrap();
    let notes = share.join("notes.txt");
    fs::write(&notes, "x").unwrap();
}
fn later(path: &Path) {
    fs::write(&path, "x").unwrap();
}
"#;
        assert_eq!(hook_lines(named), vec![3, 6]);
        let fine = r#"
fn dir(path: &Path) {
    std::fs::write(path.join("f"), "x").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(path.join("f"), std::fs::Permissions::from_mode(0o644)).unwrap();
}
"#;
        assert!(write_then_exec(fine).is_empty());
        assert!(hook_lines(fine).is_empty());
    }

    #[test]
    fn the_site_scan_follows_a_hook_name_through_lets() {
        let chain = r#"
fn chain(dir: &Path) {
    let name = "pre-push";
    let path = dir.join(name);
    fs::write(&path, "x").unwrap();
    let other = "notes.txt";
    let note = dir.join(other);
    fs::write(&note, "x").unwrap();
    let path = dir.join("notes.txt");
    fs::write(&path, "x").unwrap();
}
"#;
        assert_eq!(hook_lines(chain), vec![5, 10]);
    }

    #[test]
    fn the_site_scan_names_a_helper_given_a_hook_path() {
        let helper = r##"
fn helper_write(path: &Path) {
    fs::write(path, "#!/bin/sh\n").unwrap();
}
fn install(dir: &Path) {
    helper_write(&dir.join("pre-push"));
}
fn note(path: &Path) {
    fs::write(path, "x").unwrap();
}
fn notes(dir: &Path) {
    note(&dir.join("notes.txt"));
}
"##;
        assert_eq!(hook_lines(helper), vec![3, 6]);
    }

    #[test]
    fn the_site_scan_names_a_hook_pushed_onto_a_path() {
        let pushed = r#"
fn pushed(dir: &Path, src: &Path) {
    let mut path = dir.join("notes");
    path.push("pre-push");
    fs::write(&path, "x").unwrap();
    let mut dest = dir.to_path_buf();
    dest.push("post-update");
    std::fs::copy(src, &dest).unwrap();
}
fn pushed_name(dir: &Path) {
    let name = "pre-push";
    let mut path = dir.join("hooks");
    path.push(name);
    fs::write(&path, "x").unwrap();
}
fn pushed_notes(dir: &Path) {
    let mut path = dir.to_path_buf();
    path.push("notes.txt");
    fs::write(&path, "x").unwrap();
}
"#;
        assert_eq!(hook_lines(pushed), vec![5, 8, 14]);
    }

    #[test]
    fn the_site_scan_names_a_method_or_path_call_given_a_hook_path() {
        let calls = r#"
struct Place;
impl Place {
    fn place(&self, path: &Path) {
        fs::write(path, "x").unwrap();
    }
    fn place_free(path: &Path) {
        fs::write(path, "x").unwrap();
    }
}
fn qualified_only(path: &Path) {
    fs::write(path, "x").unwrap();
}
fn no_caller(path: &Path) {
    fs::write(path, "x").unwrap();
}
fn install(place: &Place, dir: &Path) {
    place.place(&dir.join("pre-push"));
    Place::place(place, &dir.join("pre-push"));
    Place::place_free(&dir.join("pre-push"));
    crate::qualified_only(&dir.join("pre-push"));
    place.place(&dir.join("notes.txt"));
}
"#;
        assert_eq!(hook_lines(calls), vec![5, 8, 12, 18, 19, 20, 21]);
    }

    #[test]
    fn the_site_scan_names_a_hook_behind_a_format_placeholder() {
        let formatted = r#"
fn formatted(dir: &Path) {
    let name = "pre-push";
    fs::write(dir.join(format!("{name}")), "x").unwrap();
    fs::write(dir.join(format!("{name}.sample")), "x").unwrap();
    fs::write(dir.join(format!("pre-{name}")), "x").unwrap();
}
"#;
        assert_eq!(hook_lines(formatted), vec![4, 6]);
    }

    #[test]
    fn the_site_scan_names_a_hook_opened_through_file_options() {
        let opened = r#"
fn opened(dir: &Path) {
    let file = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(dir.join("commit-msg"))
        .unwrap();
}
"#;
        assert_eq!(hook_lines(opened), vec![7]);
    }

    #[test]
    fn the_site_scan_follows_a_helper_into_another_file() {
        let helper = r#"
pub fn other_write(path: &Path) {
    let path = path.to_path_buf();
    fs::write(&path, "x").unwrap();
}
pub fn pass_on(path: &Path) {
    other_write(path);
}
"#;
        let caller = r#"
fn install(dir: &Path) {
    crate::other::other_write(&dir.join("pre-push"));
    other::pass_on(&dir.join("pre-push"));
    other::other_write(&dir.join("notes.txt"));
}
"#;
        let sources = [
            ("ljos-cli/src/other.rs".to_string(), helper.to_string()),
            ("ljos-cli/src/install.rs".to_string(), caller.to_string()),
        ];
        let found: Vec<_> = hook_writes(&sources)
            .into_iter()
            .map(|hit| (hit.file, hit.line, hit.writer))
            .collect();
        assert_eq!(
            found,
            vec![(1, 3, Some((0, 4))), (1, 4, Some((0, 4)))],
            "{found:?}"
        );
    }

    #[test]
    fn the_site_scan_follows_a_hook_name_out_of_another_file() {
        let names = r#"
pub const HOOK: &str = "pre-push";
pub fn hook_path(dir: &Path) -> PathBuf {
    dir.join("pre-push")
}
pub fn notes_path(dir: &Path) -> PathBuf {
    dir.join("notes.txt")
}
"#;
        let user = r#"
fn put(dir: &Path) {
    fs::write(dir.join(names::HOOK), "x").unwrap();
    fs::write(crate::names::hook_path(dir), "x").unwrap();
    fs::write(crate::names::notes_path(dir), "x").unwrap();
}
"#;
        let sources = [
            ("ljos-cli/src/names.rs".to_string(), names.to_string()),
            ("ljos-cli/src/user.rs".to_string(), user.to_string()),
        ];
        let found: Vec<_> = hook_writes(&sources)
            .into_iter()
            .map(|hit| (hit.file, hit.line))
            .collect();
        assert_eq!(found, vec![(1, 3), (1, 4)]);
    }

    fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "target") {
                    rust_sources(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && path
                    .components()
                    .any(|part| part.as_os_str() == "src" || part.as_os_str() == "tests")
            {
                out.push(path);
            }
        }
    }

    /// One-based lines of in-process writes that `text` later marks
    /// executable in the same function.
    fn write_then_exec(text: &str) -> Vec<usize> {
        let code: Vec<&str> = text
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("//") {
                    ""
                } else {
                    line
                }
            })
            .collect();
        let joined = |from: usize, len: usize| code[from..(from + len).min(code.len())].join(" ");
        let mut hits = Vec::new();
        let mut writes: Vec<(usize, String)> = Vec::new();
        let mut lists: Vec<(String, Vec<String>)> = Vec::new();
        for (i, line) in code.iter().enumerate() {
            let head = line.trim_start();
            let item = ["pub(crate) ", "pub ", "async ", "const ", "unsafe "]
                .iter()
                .fold(head, |rest, pre| rest.strip_prefix(pre).unwrap_or(rest));
            if item.starts_with("fn ") {
                writes.clear();
                lists.clear();
            }
            let ahead = joined(i, 4);
            if let Some(rest) = head.strip_prefix("for ") {
                if let Some((name, list)) = rest.split_once(" in [") {
                    let names = list.split('"').skip(1).step_by(2);
                    lists.push((
                        name.trim().to_string(),
                        names.map(|lit| format!("\"{lit}\"")).collect(),
                    ));
                }
            }
            let target = if line.contains("fs::write(") {
                arg_after(&ahead, "fs::write(", 0)
            } else if line.contains("File::create(") {
                arg_after(&ahead, "File::create(", 0)
            } else if line.contains("fs::copy(") {
                arg_after(&ahead, "fs::copy(", 1)
            } else if line.contains("OpenOptions::new()") {
                arg_after(&joined(i, 8), ".open(", 0)
            } else {
                bare_write(line).and_then(|_| arg_after(&ahead, "write(", 0))
            };
            if let Some(target) = target.filter(|t| !t.is_empty()) {
                writes.push((i, target));
            }
            if line.contains("set_permissions(") {
                let (Some(target), Some(perm)) = (
                    arg_after(&ahead, "set_permissions(", 0),
                    arg_after(&ahead, "set_permissions(", 1),
                ) else {
                    continue;
                };
                let mode = mode_of(&perm).or_else(|| {
                    let set = format!("{perm}.set_mode(");
                    (i.saturating_sub(4)..i)
                        .rev()
                        .map(|at| code[at])
                        .find(|line| squeeze(line).contains(&set))
                        .and_then(mode_of)
                });
                if mode.is_some_and(|mode| mode & 0o111 != 0) {
                    for (at, written) in &writes {
                        if same_path(written, &target, &lists) {
                            hits.push(at + 1);
                        }
                    }
                }
            }
        }
        hits.sort_unstable();
        hits.dedup();
        hits
    }

    /// A write the hook scan flags: a raw write, or a call into a function
    /// that writes the path it is given, inside a function that names a git
    /// hook, where the file name written is not shown to be something else.
    #[derive(Debug)]
    struct HookWrite {
        file: usize,
        line: usize,
        func: String,
        target: String,
        /// For a call, the write it reaches.
        writer: Option<(usize, usize)>,
    }

    /// One-based lines `hook_writes` flags in one file, writers included.
    fn hook_lines(text: &str) -> Vec<usize> {
        let sources = [("ljos-cli/src/sample.rs".to_string(), text.to_string())];
        let mut lines: Vec<usize> = hook_writes(&sources)
            .into_iter()
            .flat_map(|hit| [Some(hit.line), hit.writer.map(|(_, line)| line)])
            .flatten()
            .collect();
        lines.sort_unstable();
        lines.dedup();
        lines
    }

    /// Calls that open a path for writing in this process, with the
    /// argument that names the path.
    const RAW_WRITES: &[(&str, usize)] = &[
        ("fs::write(", 0),
        ("fs::copy(", 1),
        ("File::create(", 0),
        ("File::create_new(", 0),
    ];

    struct Source<'a> {
        path: &'a str,
        text: &'a str,
        /// `text` with comments and the inside of literals blanked.
        code: String,
        literals: Vec<(usize, usize)>,
        lines: Vec<usize>,
    }

    struct Func {
        src: usize,
        name: String,
        params: Vec<String>,
        returns_path: bool,
        body: (usize, usize),
    }

    #[derive(Clone, Copy)]
    enum Kind {
        Bare,
        Method,
        Path,
    }

    struct Call {
        name: String,
        kind: Kind,
        args: Vec<(usize, usize)>,
        line: usize,
    }

    /// What a path's last component can be: shown not to be a hook name,
    /// or unknown and drawn from these parameters.
    #[derive(Clone)]
    enum Leaf {
        Safe,
        From(Vec<usize>),
    }

    struct Scan<'a> {
        srcs: Vec<Source<'a>>,
        funcs: Vec<Func>,
        named: std::collections::HashMap<String, Vec<usize>>,
        /// Function names whose every return ends in a file name that is
        /// not a hook.
        returns: std::cell::RefCell<std::collections::HashMap<String, bool>>,
        leaves: std::cell::RefCell<std::collections::HashMap<(usize, usize, usize, usize), Leaf>>,
    }

    /// Hook writes in `sources`, each a path under `crates/` and its text.
    ///
    /// A function names a hook when one of its literals ends in a hook name
    /// or it uses `HOOK_NAMES` or a constant that names one. In such a
    /// function every path given to a raw write, and every argument given
    /// to a function that writes that parameter, must end in a file name
    /// that is visibly not a hook. Anything else is flagged, so a hook name
    /// that reaches the write by any route the scan does not follow is
    /// flagged too.
    fn hook_writes(sources: &[(String, String)]) -> Vec<HookWrite> {
        let srcs: Vec<Source> = sources
            .iter()
            .map(|(path, text)| {
                let (code, literals) = blank(text);
                let lines = std::iter::once(0)
                    .chain(text.match_indices('\n').map(|(at, _)| at + 1))
                    .collect();
                Source {
                    path,
                    text,
                    code,
                    literals,
                    lines,
                }
            })
            .collect();
        let mut quals: Vec<String> = ["crate", "self", "super", "Self"]
            .map(String::from)
            .to_vec();
        let mut markers = vec!["HOOK_NAMES".to_string()];
        let mut funcs = Vec::new();
        for (n, src) in srcs.iter().enumerate() {
            let mut parts = src.path.split('/');
            quals.extend(parts.next().map(|krate| krate.replace('-', "_")));
            quals.extend(
                parts
                    .next_back()
                    .and_then(|file| file.strip_suffix(".rs"))
                    .map(String::from),
            );
            for item in ["struct", "enum", "trait", "mod", "type", "union"] {
                quals.extend(words_after(&src.code, item));
            }
            markers.extend(hook_consts(src));
            funcs.extend(functions(n, &src.code));
        }
        let mut named: std::collections::HashMap<String, Vec<usize>> = Default::default();
        for (n, f) in funcs.iter().enumerate() {
            named.entry(f.name.clone()).or_default().push(n);
        }
        let scan = Scan {
            srcs,
            funcs,
            named,
            returns: Default::default(),
            leaves: Default::default(),
        };
        let (srcs, funcs) = (&scan.srcs, &scan.funcs);
        let calls: Vec<Vec<Call>> = funcs
            .iter()
            .map(|f| calls_in(&srcs[f.src], f, &quals))
            .collect();
        let sites: Vec<Vec<((usize, usize), usize)>> =
            funcs.iter().map(|f| raw_sites(&srcs[f.src], f)).collect();
        // Parameter, whether a method, the defining file, and the raw write
        // the parameter reaches.
        type Writer = (usize, bool, usize, usize, usize);
        let mut writers: std::collections::HashMap<&str, Vec<Writer>> = Default::default();
        let mut found = Vec::new();
        for (n, f) in funcs.iter().enumerate() {
            for &((s, e), line) in &sites[n] {
                if let Leaf::From(params) = leaf(&scan, f, s, e, 0) {
                    found.extend(params.into_iter().map(|p| (n, p, f.src, line)));
                }
            }
        }
        while !found.is_empty() {
            for (n, param, file, line) in found.drain(..) {
                let f = &funcs[n];
                let known = writers.entry(f.name.as_str()).or_default();
                if !known
                    .iter()
                    .any(|w| (w.0, w.1, w.2) == (param, is_method(f), f.src))
                {
                    known.push((param, is_method(f), f.src, file, line));
                }
            }
            for (n, f) in funcs.iter().enumerate() {
                for call in &calls[n] {
                    for &(param, method, home, file, line) in
                        writers.get(call.name.as_str()).into_iter().flatten()
                    {
                        let Some((s, e)) = arg_for(&scan, f, call, home, param, method) else {
                            continue;
                        };
                        let Leaf::From(params) = leaf(&scan, f, s, e, 0) else {
                            continue;
                        };
                        for p in params {
                            let known = writers.get(f.name.as_str()).is_some_and(|known| {
                                known
                                    .iter()
                                    .any(|w| (w.0, w.1, w.2) == (p, is_method(f), f.src))
                            });
                            if !known
                                && !found
                                    .iter()
                                    .any(|x: &(usize, usize, usize, usize)| (x.0, x.1) == (n, p))
                            {
                                found.push((n, p, file, line));
                            }
                        }
                    }
                }
            }
        }
        let names_hook = |f: &Func, markers: &[String]| {
            let src = &srcs[f.src];
            let body = &src.code[f.body.0..f.body.1];
            src.literals
                .iter()
                .any(|&(s, e)| s >= f.body.0 && e <= f.body.1 && is_hook_leaf(&src.text[s..e]))
                || markers.iter().any(|m| has_word(body, m))
        };
        loop {
            let more: Vec<String> = funcs
                .iter()
                .filter(|f| f.returns_path && !markers.contains(&f.name))
                .filter(|f| names_hook(f, &markers) && !returns_safe(&scan, &f.name, 0))
                .map(|f| f.name.clone())
                .collect();
            if more.is_empty() {
                break;
            }
            markers.extend(more);
        }
        let mut hits = Vec::new();
        for (n, f) in funcs.iter().enumerate() {
            let src = &srcs[f.src];
            if !names_hook(f, &markers) {
                continue;
            }
            let hit = |s: usize, e: usize, line: usize, writer| HookWrite {
                file: f.src,
                line,
                func: f.name.clone(),
                target: squeeze(&src.text[s..e]),
                writer,
            };
            for &((s, e), line) in &sites[n] {
                if !matches!(leaf(&scan, f, s, e, 0), Leaf::Safe) {
                    hits.push(hit(s, e, line, None));
                }
            }
            for call in &calls[n] {
                for &(param, method, home, file, line) in
                    writers.get(call.name.as_str()).into_iter().flatten()
                {
                    let Some((s, e)) = arg_for(&scan, f, call, home, param, method) else {
                        continue;
                    };
                    if !matches!(leaf(&scan, f, s, e, 0), Leaf::Safe) {
                        hits.push(hit(s, e, call.line, Some((file, line))));
                    }
                }
            }
        }
        hits.sort_by_key(|hit| (hit.file, hit.line, hit.writer));
        hits.dedup_by_key(|hit| (hit.file, hit.line, hit.writer));
        hits
    }

    fn is_method(f: &Func) -> bool {
        f.params.first().is_some_and(|p| p == "self")
    }

    fn is_hook_leaf(literal: &str) -> bool {
        let leaf = literal.rsplit('/').next().unwrap_or(literal);
        crate::git_hooks::HOOK_NAMES.contains(&leaf)
    }

    /// Whether a file name, with `{..}` for a part filled in at run time,
    /// cannot be a hook name.
    fn not_a_hook(leaf: &str) -> bool {
        let hooks = crate::git_hooks::HOOK_NAMES;
        match (leaf.find('{'), leaf.rfind('}')) {
            (None, _) => !leaf.is_empty() && !hooks.contains(&leaf),
            (Some(open), Some(close)) if open < close => {
                let (head, tail) = (&leaf[..open], &leaf[close + 1..]);
                (!head.is_empty() && !hooks.iter().any(|h| h.starts_with(head)))
                    || (!tail.is_empty() && !hooks.iter().any(|h| h.ends_with(tail)))
            }
            _ => false,
        }
    }

    /// The last component of the path `src.text[s..e]` in `f` evaluates to.
    fn leaf(scan: &Scan, f: &Func, s: usize, e: usize, depth: usize) -> Leaf {
        let key = (f.src, s, e, depth);
        if let Some(known) = scan.leaves.borrow().get(&key) {
            return known.clone();
        }
        let found = leaf_of(scan, f, s, e, depth);
        scan.leaves.borrow_mut().insert(key, found.clone());
        found
    }

    fn leaf_of(scan: &Scan, f: &Func, s: usize, e: usize, depth: usize) -> Leaf {
        let src = &scan.srcs[f.src];
        let code = &src.code;
        let s = s + code[s..e].len() - code[s..e].trim_start_matches(['&', ' ', '\n', '\t']).len();
        let mut e = s + code[s..e].trim_end().len();
        while let Some(rest) = [
            ".to_path_buf()",
            ".clone()",
            ".as_path()",
            ".to_owned()",
            ".as_ref()",
            ".into()",
            ".unwrap()",
            "?",
        ]
        .iter()
        .find_map(|suffix| code[s..e].strip_suffix(suffix))
        {
            e = s + rest.trim_end().len();
        }
        let expr = &code[s..e];
        let from = |text: &str| Leaf::From(feeds(scan, f, text));
        if let Some(lit) = literal(src, s, e) {
            let name = lit.rsplit('/').next().unwrap_or(lit);
            return if not_a_hook(name) {
                Leaf::Safe
            } else {
                from(name)
            };
        }
        if let Some(open) = expr.strip_prefix("format!(").map(|_| s + 7) {
            let args = split_args(code, open, false).0;
            if let Some(lit) = args.first().and_then(|&(a, b)| literal(src, a, b)) {
                let name = lit.rsplit('/').next().unwrap_or(lit);
                if not_a_hook(name) {
                    return Leaf::Safe;
                }
                let rest = args[1..]
                    .iter()
                    .map(|&(a, b)| &code[a..b])
                    .collect::<Vec<_>>();
                return from(&format!("{name} {}", rest.join(" ")));
            }
        }
        if !expr.is_empty() && expr.chars().all(is_ident) {
            return binding(scan, f, expr, s, depth);
        }
        if let Some(open) = expr
            .ends_with(')')
            .then(|| open_of(code, s, e - 1))
            .flatten()
        {
            let callee = code[s..open].trim_end();
            let args = split_args(code, open, false).0;
            if [".join", ".with_file_name", "Path::new", "PathBuf::from"]
                .iter()
                .any(|name| callee.ends_with(name))
                && args.len() == 1
            {
                return leaf(scan, f, args[0].0, args[0].1, depth);
            }
            if callee.ends_with(".with_extension")
                && args.len() == 1
                && literal(src, args[0].0, args[0].1)
                    .is_some_and(|ext| !ext.is_empty() && !ext.contains('{'))
            {
                return Leaf::Safe;
            }
            let name = &callee[callee.trim_end_matches(is_ident).len()..];
            if !name.is_empty() && returns_safe(scan, name, depth) {
                return Leaf::Safe;
            }
        }
        from(expr)
    }

    /// Whether every function called `name` returns only paths whose last
    /// component `leaf` shows is not a hook.
    fn returns_safe(scan: &Scan, name: &str, depth: usize) -> bool {
        if let Some(&known) = scan.returns.borrow().get(name) {
            return known;
        }
        let Some(found) = scan.named.get(name) else {
            return false;
        };
        if depth >= 4 {
            return false;
        }
        scan.returns.borrow_mut().insert(name.to_string(), false);
        let safe = found.iter().all(|&n| {
            let f = &scan.funcs[n];
            let code = &scan.srcs[f.src].code;
            let (open, close) = (f.body.0 + 1, f.body.1 - 1);
            let mut depth_here = 0usize;
            let mut last = open;
            for (at, c) in code[open..close].char_indices() {
                match c {
                    '(' | '[' | '{' => depth_here += 1,
                    ')' | ']' | '}' => depth_here = depth_here.saturating_sub(1),
                    ';' if depth_here == 0 => last = open + at + 1,
                    _ => {}
                }
            }
            let mut tails = vec![(last, close)];
            for at in word_uses(&code[open..close], "return") {
                let value = open + at + "return".len();
                match statement_end(code, value) {
                    Some(end) if end < close => tails.push((value, end)),
                    _ => return false,
                }
            }
            tails.iter().all(|&(s, e)| {
                !code[s..e].trim().is_empty()
                    && matches!(leaf(scan, f, s, e, depth + 1), Leaf::Safe)
            })
        });
        scan.returns.borrow_mut().insert(name.to_string(), safe);
        safe
    }

    /// Parameters of `f` that `text` names, as a word or a `{name}`
    /// placeholder, or through the value of a `let` it names.
    fn feeds(scan: &Scan, f: &Func, text: &str) -> Vec<usize> {
        let code = &scan.srcs[f.src].code;
        let body = &code[f.body.0..f.body.1];
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| !is_ident(c))
                .filter(|w| !w.is_empty())
                .map(String::from)
                .collect()
        };
        let mut seen = words(text);
        let mut next = 0;
        while next < seen.len() {
            let word = seen[next].clone();
            next += 1;
            for at in word_uses(body, &word) {
                if let Some((s, e)) = let_value(code, f.body.0 + at, &word) {
                    for found in words(&code[s..e]) {
                        if !seen.contains(&found) {
                            seen.push(found);
                        }
                    }
                }
            }
        }
        params_in(f, &seen.join(" "))
    }

    /// The value span of `let name = value;` or `let mut name = value;`
    /// when `name` at `at` is bound there.
    fn let_value(code: &str, at: usize, name: &str) -> Option<(usize, usize)> {
        let before = code[..at].trim_end();
        let before = before
            .strip_suffix("mut")
            .filter(|rest| !rest.ends_with(is_ident))
            .map_or(before, str::trim_end);
        if !ends_with_word(before, "let") {
            return None;
        }
        let rest = &code[at + name.len()..];
        let value = at + name.len() + rest.find('=')? + 1;
        Some((value, statement_end(code, value)?))
    }

    /// What `v`, used at `at` in `f`, holds. Only a single `let` that is
    /// not `mut`, before `at`, with `v` otherwise used only as `&v` or
    /// `v.`, is followed; anything else may hold any path.
    fn binding(scan: &Scan, f: &Func, v: &str, at: usize, depth: usize) -> Leaf {
        let code = &scan.srcs[f.src].code;
        let fallback = || Leaf::From(feeds(scan, f, v));
        let uses = word_uses(&code[f.body.0..f.body.1], v)
            .into_iter()
            .map(|n| n + f.body.0)
            .collect::<Vec<_>>();
        let lets: Vec<usize> = uses
            .iter()
            .copied()
            .filter(|&n| {
                let before = code[..n].trim_end();
                ends_with_word(before, "let") || ends_with_word(before, "mut")
            })
            .collect();
        let [bound] = lets[..] else {
            return fallback();
        };
        if depth >= 4 || bound >= at || ends_with_word(code[..bound].trim_end(), "mut") {
            return fallback();
        }
        for &n in &uses {
            let before = code[..n].trim_end();
            let after = code[n + v.len()..].trim_start();
            let borrowed = before.strip_suffix('&').is_some_and(|rest| {
                let rest = rest.trim_end();
                !rest.ends_with('|') && !ends_with_word(rest, "for")
            });
            let method = after.starts_with('.') && !after.starts_with("..");
            let returned = n + v.len() + code[n + v.len()..].len() - after.len() + 1 == f.body.1;
            if n != bound && n != at && !borrowed && !method && !returned {
                return fallback();
            }
        }
        match let_value(code, bound, v) {
            Some((s, e)) => leaf(scan, f, s, e, depth + 1),
            None => fallback(),
        }
    }

    /// The text of a string literal that is exactly `src.text[s..e]`.
    fn literal<'a>(src: &'a Source, s: usize, e: usize) -> Option<&'a str> {
        let code = src.code[s..e].trim();
        let s = s + src.code[s..e].find(code)?;
        let e = s + code.len();
        (code.len() >= 2
            && code.starts_with('"')
            && code.ends_with('"')
            && !code[1..code.len() - 1].contains('"'))
        .then(|| &src.text[s + 1..e - 1])
    }

    /// Indices of the parameters of `f` that `text` names, as a word or
    /// as a `{name}` placeholder.
    fn params_in(f: &Func, text: &str) -> Vec<usize> {
        let words: Vec<&str> = text
            .split(|c: char| !is_ident(c))
            .filter(|w| !w.is_empty())
            .collect();
        f.params
            .iter()
            .enumerate()
            .filter(|(_, p)| *p != "self" && words.contains(&p.as_str()))
            .map(|(n, _)| n)
            .collect()
    }

    /// The span of the argument `call` in `f` passes for parameter `param`
    /// of a function defined in file `home`, if the call can reach it.
    fn arg_for(
        scan: &Scan,
        f: &Func,
        call: &Call,
        home: usize,
        param: usize,
        method: bool,
    ) -> Option<(usize, usize)> {
        let at = match (call.kind, method) {
            (Kind::Bare, true) | (Kind::Method, false) => return None,
            (Kind::Method, true) => param.checked_sub(1)?,
            _ => param,
        };
        if !reaches(
            &scan.srcs[f.src],
            scan.srcs[home].path,
            &call.name,
            call.kind,
        ) {
            return None;
        }
        call.args.get(at).copied().filter(|(s, e)| s < e)
    }

    /// Whether a call by `name` of `kind` in `src` can reach a function
    /// defined in `home`. A test file is its own crate: it reaches its own
    /// functions, the shared modules beside it, and library functions by
    /// path or through a `use`.
    fn reaches(src: &Source, home: &str, name: &str, kind: Kind) -> bool {
        if home.contains("/src/") {
            return src.path.contains("/src/")
                || !matches!(kind, Kind::Bare)
                || src
                    .code
                    .lines()
                    .any(|line| line.trim_start().starts_with("use ") && has_word(line, name));
        }
        let parts: Vec<&str> = home.split('/').collect();
        src.path == home
            || (parts.len() > 3
                && parts[1] == "tests"
                && src.path.starts_with(&format!("{}/tests/", parts[0])))
    }

    /// `text` with comments and the inside of string and char literals
    /// replaced by spaces, byte for byte, and the spans of the string
    /// literals' text.
    fn blank(text: &str) -> (String, Vec<(usize, usize)>) {
        let b = text.as_bytes();
        let mut out = b.to_vec();
        let mut literals = Vec::new();
        let wipe = |out: &mut Vec<u8>, from: usize, to: usize| {
            for c in &mut out[from..to.min(b.len())] {
                if *c != b'\n' {
                    *c = b' ';
                }
            }
        };
        let after_ident =
            |at: usize| at > 0 && (b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_');
        let mut i = 0;
        while i < b.len() {
            let raw = (!after_ident(i))
                .then(|| match b[i..] {
                    [b'r', ..] => Some(i + 1),
                    [b'b', b'r', ..] => Some(i + 2),
                    _ => None,
                })
                .flatten()
                .and_then(|at| {
                    let hashes = b[at..].iter().take_while(|&&c| c == b'#').count();
                    (b.get(at + hashes) == Some(&b'"')).then_some((at + hashes + 1, hashes))
                });
            if b[i..].starts_with(b"//") {
                let end = b[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(b.len(), |n| i + n);
                wipe(&mut out, i, end);
                i = end;
            } else if b[i..].starts_with(b"/*") {
                let (mut depth, mut j) = (0, i);
                while j < b.len() {
                    if b[j..].starts_with(b"/*") {
                        depth += 1;
                        j += 2;
                    } else if b[j..].starts_with(b"*/") {
                        depth -= 1;
                        j += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                wipe(&mut out, i, j);
                i = j;
            } else if let Some((open, hashes)) = raw {
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                let end = b[open..]
                    .windows(close.len())
                    .position(|w| w == close.as_slice())
                    .map_or(b.len(), |n| open + n);
                literals.push((open, end));
                wipe(&mut out, open, end);
                i = end + close.len();
            } else if b[i] == b'"'
                || (b[i] == b'b' && b.get(i + 1) == Some(&b'"') && !after_ident(i))
            {
                let open = i + if b[i] == b'b' { 2 } else { 1 };
                let mut j = open;
                while j < b.len() && b[j] != b'"' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                let end = j.min(b.len());
                literals.push((open, end));
                wipe(&mut out, open, end);
                i = end + 1;
            } else if b[i] == b'\'' {
                let end = if b.get(i + 1) == Some(&b'\\') {
                    (i + 3..(i + 12).min(b.len())).find(|&j| b[j] == b'\'')
                } else {
                    let len = match b.get(i + 1) {
                        Some(&c) if c < 0x80 => 1,
                        Some(&c) if c >= 0xf0 => 4,
                        Some(&c) if c >= 0xe0 => 3,
                        Some(_) => 2,
                        None => 0,
                    };
                    (len > 0 && b.get(i + 1) != Some(&b'\'') && b.get(i + 1 + len) == Some(&b'\''))
                        .then_some(i + 1 + len)
                };
                match end {
                    Some(end) => {
                        wipe(&mut out, i + 1, end);
                        i = end + 1;
                    }
                    None => i += 1,
                }
            } else {
                i += 1;
            }
        }
        (String::from_utf8_lossy(&out).into_owned(), literals)
    }

    /// Every `fn` with a body in `code`.
    fn functions(src: usize, code: &str) -> Vec<Func> {
        let b = code.as_bytes();
        let mut out = Vec::new();
        for at in word_uses(code, "fn") {
            let rest = &code[at + 2..];
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|&c| is_ident(c))
                .collect();
            if name.is_empty() {
                continue;
            }
            let mut k = at + 2 + (rest.len() - rest.trim_start().len()) + name.len();
            if b.get(k) == Some(&b'<') {
                let Some(close) = close_angle(code, k) else {
                    continue;
                };
                k = close + 1;
            }
            if b.get(k) != Some(&b'(') {
                continue;
            }
            let (params, close) = split_args(code, k, true);
            let mut depth = 0usize;
            let Some(open) = (close + 1..b.len()).find(|&j| {
                match b[j] {
                    b'(' | b'[' => depth += 1,
                    b')' | b']' => depth = depth.saturating_sub(1),
                    b'{' | b';' if depth == 0 => return true,
                    _ => {}
                }
                false
            }) else {
                continue;
            };
            if b[open] == b';' {
                continue;
            }
            let end = close_of(code, open).unwrap_or(b.len() - 1);
            let params = params
                .iter()
                .map(|&(s, e)| {
                    let pattern = code[s..e].split(':').next().unwrap_or("");
                    let name = pattern.split_whitespace().last().unwrap_or("");
                    name.trim_start_matches('&').to_string()
                })
                .collect();
            out.push(Func {
                src,
                name,
                params,
                returns_path: code[close..open].contains("Path"),
                body: (open, end + 1),
            });
        }
        out
    }

    /// Constants and statics whose value names a hook.
    fn hook_consts(src: &Source) -> Vec<String> {
        let mut out = Vec::new();
        for item in ["const", "static"] {
            for at in word_uses(&src.code, item) {
                let rest = src.code[at + item.len()..].trim_start();
                let rest = rest.strip_prefix("mut ").unwrap_or(rest).trim_start();
                let name: String = rest.chars().take_while(|&c| is_ident(c)).collect();
                let typed = rest[name.len()..].trim_start().starts_with(':');
                let Some(end) = statement_end(&src.code, at) else {
                    continue;
                };
                if !name.is_empty()
                    && typed
                    && !src.code[..at].ends_with('\'')
                    && src
                        .literals
                        .iter()
                        .any(|&(s, e)| s > at && e <= end && is_hook_leaf(&src.text[s..e]))
                {
                    out.push(name);
                }
            }
        }
        out
    }

    /// Spans of the paths `f` opens for writing with a raw call, and the
    /// line of each call.
    fn raw_sites(src: &Source, f: &Func) -> Vec<((usize, usize), usize)> {
        let code = &src.code;
        let body = &code[f.body.0..f.body.1];
        let mut out = Vec::new();
        let mut take = |open: usize, n: usize| {
            if let Some(&span) = split_args(code, open, false).0.get(n) {
                out.push((span, line_of(src, open)));
            }
        };
        for (api, n) in RAW_WRITES {
            for (at, _) in body.match_indices(api) {
                take(f.body.0 + at + api.len() - 1, *n);
            }
        }
        for (api, n) in [("write(", 0), ("copy(", 1)] {
            for (at, _) in body.match_indices(api) {
                let before = &body[..at];
                let lead = before.chars().next_back();
                if !lead.is_some_and(|c| is_ident(c) || matches!(c, '.' | ':'))
                    && !ends_with_word(before.trim_end(), "fn")
                {
                    take(f.body.0 + at + api.len() - 1, n);
                }
            }
        }
        let writable = [
            ".write(",
            ".append(",
            ".create(",
            ".create_new(",
            ".truncate(",
        ]
        .iter()
        .any(|flag| body.contains(flag));
        if writable && (body.contains("OpenOptions") || body.contains("File::options(")) {
            for (at, _) in body.match_indices(".open(") {
                take(f.body.0 + at + 5, 0);
            }
        }
        out
    }

    /// Calls in `f` by name, as `name(..)`, `.name(..)`, or `Path::name(..)`
    /// where `Path` is one of `quals`.
    fn calls_in(src: &Source, f: &Func, quals: &[String]) -> Vec<Call> {
        let code = &src.code;
        let mut out = Vec::new();
        for (at, _) in code[f.body.0..f.body.1].match_indices('(') {
            let open = f.body.0 + at;
            let mut end = code[..open].trim_end().len();
            if code[..end].ends_with('>') {
                let Some(lt) = (0..end).rev().find(|&j| code.as_bytes()[j] == b'<') else {
                    continue;
                };
                match code[..lt].strip_suffix("::") {
                    Some(rest) => end = rest.len(),
                    None => continue,
                }
            }
            let name_at = code[..end].trim_end_matches(is_ident).len();
            let name = &code[name_at..end];
            if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            let before = code[..name_at].trim_end();
            if ends_with_word(before, "fn") {
                continue;
            }
            let kind = if before.ends_with('.') {
                Kind::Method
            } else if let Some(rest) = before.strip_suffix("::") {
                let qual = &rest[rest.trim_end_matches(is_ident).len()..];
                if !quals.iter().any(|q| q == qual) {
                    continue;
                }
                Kind::Path
            } else {
                Kind::Bare
            };
            out.push(Call {
                name: name.to_string(),
                kind,
                args: split_args(code, open, false).0,
                line: line_of(src, open),
            });
        }
        out
    }

    fn line_of(src: &Source, at: usize) -> usize {
        src.lines.partition_point(|&start| start <= at)
    }

    /// Start offsets of `word` as a whole word in `text`.
    fn word_uses(text: &str, word: &str) -> Vec<usize> {
        text.match_indices(word)
            .map(|(at, _)| at)
            .filter(|&at| {
                !text[..at].ends_with(is_ident)
                    && !text[at + word.len()..].starts_with(is_ident)
                    && !text[..at].ends_with('.')
                    && !text[..at].ends_with("::")
            })
            .collect()
    }

    /// Whether `text` has `word` as a whole word, path-qualified or not.
    fn has_word(text: &str, word: &str) -> bool {
        text.match_indices(word).any(|(at, _)| {
            !text[..at].ends_with(is_ident) && !text[at + word.len()..].starts_with(is_ident)
        })
    }

    fn ends_with_word(text: &str, word: &str) -> bool {
        text.strip_suffix(word)
            .is_some_and(|rest| !rest.ends_with(is_ident))
    }

    fn words_after(code: &str, item: &str) -> Vec<String> {
        word_uses(code, item)
            .into_iter()
            .map(|at| {
                code[at + item.len()..]
                    .trim_start()
                    .chars()
                    .take_while(|&c| is_ident(c))
                    .collect::<String>()
            })
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// The `;` that ends the statement starting at `from`.
    fn statement_end(code: &str, from: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (n, c) in code[from..].char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.checked_sub(1)?,
                ';' if depth == 0 => return Some(from + n),
                _ => {}
            }
        }
        None
    }

    /// The `}` that closes the `{` at `open`.
    fn close_of(code: &str, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (n, c) in code[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + n);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The `(` that opens the `)` at `close`, not before `from`.
    fn open_of(code: &str, from: usize, close: usize) -> Option<usize> {
        let b = code.as_bytes();
        let mut depth = 0usize;
        for j in (from..=close).rev() {
            match b[j] {
                b')' | b']' | b'}' => depth += 1,
                b'(' | b'[' | b'{' => {
                    depth -= 1;
                    if depth == 0 {
                        return (b[j] == b'(').then_some(j);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn close_angle(code: &str, open: usize) -> Option<usize> {
        let b = code.as_bytes();
        let mut depth = 0usize;
        for j in open..b.len() {
            match b[j] {
                b'<' => depth += 1,
                b'>' if b[j - 1] != b'-' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The top-level argument spans of the call whose `(` is at `open`, and
    /// the offset of its `)`. `angles` also nests `<..>`, for parameter
    /// lists.
    fn split_args(code: &str, open: usize, angles: bool) -> (Vec<(usize, usize)>, usize) {
        let b = code.as_bytes();
        let mut spans = Vec::new();
        let mut depth = 0usize;
        let mut start = open + 1;
        let mut j = open + 1;
        while j < b.len() {
            match b[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b'<' if angles => depth += 1,
                b'>' if angles && b[j - 1] != b'-' => depth = depth.saturating_sub(1),
                b')' | b']' | b'}' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                b',' if depth == 0 => {
                    spans.push(trimmed(code, start, j));
                    start = j + 1;
                }
                _ => {}
            }
            j += 1;
        }
        let last = trimmed(code, start, j.min(b.len()));
        if last.0 < last.1 || !spans.is_empty() {
            spans.push(last);
        }
        (spans, j)
    }

    fn trimmed(code: &str, s: usize, e: usize) -> (usize, usize) {
        let text = &code[s..e];
        let s = s + text.len() - text.trim_start().len();
        (s, s + code[s..e].trim_end().len())
    }

    fn is_ident(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn bare_write(line: &str) -> Option<usize> {
        let at = line.find("write(")?;
        let before = line[..at].chars().next_back();
        match before {
            Some(c) if c.is_alphanumeric() || matches!(c, '_' | '.' | ':') => None,
            _ => Some(at),
        }
    }

    fn squeeze(text: &str) -> String {
        text.split_whitespace().collect()
    }

    /// The `n`th top-level argument after `open`, without spaces or a
    /// leading `&`.
    fn arg_after(text: &str, open: &str, n: usize) -> Option<String> {
        args_after(text, open)
            .get(n)
            .map(|arg| squeeze(arg).trim_start_matches('&').to_string())
    }

    /// The top-level arguments after the first `open` in `text`.
    fn args_after(text: &str, open: &str) -> Vec<String> {
        let Some(at) = text.find(open) else {
            return Vec::new();
        };
        let rest = &text[at + open.len()..];
        let mut args = vec![String::new()];
        let mut depth = 0usize;
        let mut quoted = false;
        let mut escaped = false;
        for c in rest.chars() {
            if quoted {
                quoted = escaped || c != '"';
                escaped = !escaped && c == '\\';
            } else if c == '"' {
                quoted = true;
            } else if matches!(c, '(' | '[' | '{') {
                depth += 1;
            } else if matches!(c, ')' | ']' | '}') {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            } else if c == ',' && depth == 0 {
                args.push(String::new());
                continue;
            }
            args.last_mut().unwrap().push(c);
        }
        args
    }

    fn mode_of(text: &str) -> Option<u32> {
        ["from_mode(0o", "set_mode(0o"].iter().find_map(|open| {
            let rest = &text[text.find(open)? + open.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_digit(8)).collect();
            u32::from_str_radix(&digits, 8).ok()
        })
    }

    /// `dir.join(name)` in a loop is the same path as `dir.join("a")` when
    /// `"a"` is one of the names the loop walks, or any name when the loop
    /// is not over a literal list.
    fn same_path(written: &str, target: &str, lists: &[(String, Vec<String>)]) -> bool {
        if written == target {
            return true;
        }
        let (Some((dir_a, leaf_a)), Some((dir_b, leaf_b))) =
            (written.split_once(".join("), target.split_once(".join("))
        else {
            return false;
        };
        if dir_a != dir_b {
            return false;
        }
        let leaf_a = leaf_a.trim_end_matches(')');
        let leaf_b = leaf_b.trim_end_matches(')');
        let (literal, name) = match (leaf_a.starts_with('"'), leaf_b.starts_with('"')) {
            (true, true) => return false,
            (true, false) => (leaf_a, leaf_b),
            (false, true) => (leaf_b, leaf_a),
            (false, false) => return leaf_a == leaf_b,
        };
        lists
            .iter()
            .rev()
            .find(|(bound, _)| bound == name)
            .is_none_or(|(_, names)| names.iter().any(|lit| lit == literal))
    }
}
