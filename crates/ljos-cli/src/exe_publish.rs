//! Publish an executable without leaving a write descriptor in this process.
//!
//! A descriptor opened here is copied into a child at fork and stays open
//! until that child reaches exec. Starting the new file in that window fails
//! with ETXTBSY ("Text file busy"). The bytes are written by a forked child
//! that exits before this process renames the temporary name into place, so
//! a fork here never sees that descriptor.
//!
//! The temporary name is in a directory this process makes and then holds
//! by descriptor. The parent directory may let another user rename that
//! directory and put a symlink at its name. Every step after the `mkdir` is
//! relative to the held descriptors, so such a swap moves nothing this
//! process writes, changes the mode of, or renames.

#[cfg(unix)]
use std::ffi::{CStr, CString};
#[cfg(unix)]
use std::fs::File;
use std::io::Error;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
static NEXT: AtomicU64 = AtomicU64::new(1);

/// The name of the file inside a stage directory.
#[cfg(unix)]
const EXE: &CStr = c"exe";

/// What [`publish`] puts at the destination.
#[derive(Clone, Copy, Debug)]
pub enum Content<'a> {
    /// These bytes.
    Bytes(&'a [u8]),
    /// The bytes of this regular file, read through one descriptor.
    CopyOf(&'a Path),
}

/// The permission bits [`publish`] gives the file. Either way they are
/// masked to `0o777`, so a published file never carries setuid, setgid or
/// sticky.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// These bits.
    Exactly(u32),
    /// The bits of the file [`Content::CopyOf`] names, read from the
    /// descriptor the copy reads, so a copy never gains a bit its source
    /// lacks.
    OfSource,
}

/// Put `content` at `dest` with `mode`. Every file this crate writes that
/// may then be executed is written here.
///
/// A forked child creates an unguessable name with
/// `O_CREAT|O_EXCL|O_NOFOLLOW`, relative to a private directory this
/// process made beside `dest` and holds by descriptor, and writes it. This
/// process then `fchmod`s and `fsync`s a descriptor of that file and
/// `renameat`s it onto the name `dest`. A symlink at `dest` is replaced,
/// never followed.
///
/// # Errors
///
/// The source is not a regular file or cannot be read, `mode` is
/// [`Mode::OfSource`] for bytes, the directory cannot be written, the
/// short-lived writer fails, or `dest` was replaced after the rename.
pub fn publish(dest: &Path, content: Content<'_>, mode: Mode) -> Result<(), Error> {
    if let (Mode::OfSource, Content::Bytes(_)) = (mode, content) {
        return Err(Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "install executable: {} has no source to take a mode from",
                dest.display()
            ),
        ));
    }
    if let Some(parent) = dir_of(dest) {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let from = match content {
            Content::CopyOf(src) => {
                let file = File::open(src).map_err(|err| named(err, "read", src))?;
                let meta = file.metadata().map_err(|err| named(err, "read", src))?;
                if !meta.file_type().is_file() {
                    return Err(Error::other(format!(
                        "install executable: {} is not a regular file",
                        src.display()
                    )));
                }
                Some((src, file, meta.permissions().mode()))
            }
            Content::Bytes(_) => None,
        };
        let mode = match (mode, &from) {
            (Mode::Exactly(bits), _) => bits,
            (Mode::OfSource, Some((_, _, source))) => *source,
            (Mode::OfSource, None) => unreachable!("bytes with Mode::OfSource are refused above"),
        } & 0o777;
        let stage = Stage::new(dest)?;
        match (&content, &from) {
            (_, Some((src, file, _))) => stage
                .fill(Fill::Copy(file))
                .map_err(|err| named(err, "copy", src))?,
            (Content::Bytes(body), None) => stage.fill(Fill::Bytes(body))?,
            (Content::CopyOf(_), None) => unreachable!("a copy opens its source above"),
        }
        stage.settle(dest, mode)
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        match content {
            Content::Bytes(body) => std::fs::write(dest, body),
            Content::CopyOf(src) => std::fs::copy(src, dest).map(|_| ()),
        }
    }
}

/// [`publish`] `body` at `path` with mode `0o755`.
///
/// # Errors
///
/// As [`publish`].
pub fn install_executable(path: &Path, body: impl AsRef<[u8]>) -> Result<(), Error> {
    publish(path, Content::Bytes(body.as_ref()), Mode::Exactly(0o755))
}

/// [`publish`] `body` at `path` with `mode`, masked to `0o777`.
///
/// # Errors
///
/// As [`publish`].
pub fn install_executable_mode(
    path: &Path,
    body: impl AsRef<[u8]>,
    mode: u32,
) -> Result<(), Error> {
    publish(path, Content::Bytes(body.as_ref()), Mode::Exactly(mode))
}

/// [`publish`] a copy of `src` at `dest` with `src`'s own mode, masked to
/// `0o777`.
///
/// # Errors
///
/// As [`publish`].
pub fn copy_executable(src: &Path, dest: &Path) -> Result<(), Error> {
    publish(dest, Content::CopyOf(src), Mode::OfSource)
}

/// What the writer puts in the staged file.
#[cfg(unix)]
enum Fill<'a> {
    Bytes(&'a [u8]),
    Copy(&'a File),
}

/// A directory of this process's own, made beside the destination and held
/// by descriptor, that the writer fills before the rename.
///
/// Its mode is `0o700`, so no other user can put a name inside it. The
/// owner of the parent can still rename it or put something else at its
/// name, so its name is used only to make it, to open it once, and to
/// remove it when that name is still this directory.
#[cfg(unix)]
struct Stage {
    parent: File,
    parent_path: PathBuf,
    dir: File,
    name: CString,
    path: PathBuf,
}

#[cfg(unix)]
impl Stage {
    fn new(path: &Path) -> Result<Self, Error> {
        use std::hash::{BuildHasher, Hasher};
        use std::os::unix::fs::OpenOptionsExt;
        let parent_path = dir_of(path).unwrap_or_else(|| Path::new(".")).to_path_buf();
        let parent = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(&parent_path)
            .map_err(|err| named(err, "open", &parent_path))?;
        let mut last = None;
        for _ in 0..16 {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
            hasher.write_u32(std::process::id());
            let name = format!(".ljos-exe.{:016x}", hasher.finish());
            let name = CString::new(name).map_err(Error::other)?;
            // SAFETY: parent is an open directory and name is NUL-terminated.
            // mkdirat creates the name or fails and follows no symlink there.
            if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                let err = Error::last_os_error();
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    last = Some(err);
                    continue;
                }
                return Err(named(err, "create a temporary directory in", &parent_path));
            }
            let path = parent_path.join(name.to_string_lossy().as_ref());
            let dir = open_at(
                &parent,
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
            .map_err(|err| named(err, "open", &path))?;
            let held = stat(&dir).map_err(|err| named(err, "read", &path))?;
            // SAFETY: geteuid has no preconditions.
            let me = unsafe { libc::geteuid() };
            if held.st_mode & libc::S_IFMT != libc::S_IFDIR
                || held.st_uid != me
                || held.st_mode & 0o077 != 0
            {
                return Err(Error::other(format!(
                    "install executable: {} was replaced before it was opened",
                    path.display()
                )));
            }
            {
                use std::os::unix::fs::PermissionsExt;
                dir.set_permissions(std::fs::Permissions::from_mode(0o700))
                    .map_err(|err| named(err, "mark private", &path))?;
            }
            let stage = Self {
                parent,
                parent_path,
                dir,
                name,
                path,
            };
            #[cfg(test)]
            at_step(Step::Staged, &stage.path);
            return Ok(stage);
        }
        let err = last.unwrap_or_else(|| Error::other("no free name"));
        Err(named(err, "create a temporary directory in", &parent_path))
    }

    fn file(&self) -> PathBuf {
        self.path.join("exe")
    }

    /// Create the staged file in a forked child and write `fill` to it.
    ///
    /// The child makes only async-signal-safe calls: this process may have
    /// other threads, and one of them may hold a lock the child would wait
    /// on forever. It exits with 0 or the errno of the call that failed.
    fn fill(&self, fill: Fill) -> Result<(), Error> {
        let stage = self.dir.as_raw_fd();
        // SAFETY: the child runs `write_staged` and `_exit` only, both of
        // which make async-signal-safe calls on memory fork copied.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(named(
                Error::last_os_error(),
                "fork a writer for",
                &self.path,
            ));
        }
        if pid == 0 {
            let code = write_staged(stage, &fill);
            // SAFETY: _exit ends the child without running this process's
            // destructors or atexit handlers.
            unsafe { libc::_exit(code) };
        }
        let mut status = 0;
        loop {
            // SAFETY: pid is the child forked above and status is writable.
            if unsafe { libc::waitpid(pid, &mut status, 0) } == pid {
                break;
            }
            let err = Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                return Err(named(err, "wait for the writer of", &self.path));
            }
        }
        if !libc::WIFEXITED(status) {
            return Err(Error::other(format!(
                "install executable: the writer of {} was stopped by a signal",
                self.path.display()
            )));
        }
        match libc::WEXITSTATUS(status) {
            0 => Ok(()),
            code => Err(named(Error::from_raw_os_error(code), "write", &self.file())),
        }
    }

    /// Mark the staged file `mode`, flush it, rename it onto `path`, check
    /// that the name now holds that file, then flush the directory so the
    /// new name survives a crash.
    fn settle(self, path: &Path, mode: u32) -> Result<(), Error> {
        use std::os::unix::fs::PermissionsExt;
        let leaf = path
            .file_name()
            .and_then(|name| CString::new(name.as_encoded_bytes()).ok())
            .ok_or_else(|| {
                Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("install executable: {} has no file name", path.display()),
                )
            })?;
        #[cfg(test)]
        at_step(Step::Written, &self.path);
        let tmp = self.file();
        let file = open_at(
            &self.dir,
            EXE,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
        .map_err(|err| named(err, "open", &tmp))?;
        let staged = stat(&file).map_err(|err| named(err, "read", &tmp))?;
        if staged.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(Error::other(format!(
                "install executable: {} is not a regular file",
                tmp.display()
            )));
        }
        file.set_permissions(std::fs::Permissions::from_mode(mode))
            .map_err(|err| named(err, "mark executable", &tmp))?;
        flush(&file, &tmp).map_err(|err| named(err, "flush", &tmp))?;
        #[cfg(test)]
        at_step(Step::Moving, &self.path);
        // SAFETY: both descriptors are open directories and both names are
        // NUL-terminated. renameat follows neither name.
        if unsafe {
            libc::renameat(
                self.dir.as_raw_fd(),
                EXE.as_ptr(),
                self.parent.as_raw_fd(),
                leaf.as_ptr(),
            )
        } != 0
        {
            return Err(named(Error::last_os_error(), "rename onto", path));
        }
        #[cfg(test)]
        at_step(Step::Renamed, &self.path);
        // `file` stays open until this check, so its inode number cannot be
        // given to something else put at the name. A later publish of the
        // same path by this user may already have replaced the file;
        // anything else at the name was put there by someone else.
        match stat_at(&self.parent, &leaf) {
            Ok(now)
                if same_file(&now, &staged)
                    || (now.st_mode & libc::S_IFMT == libc::S_IFREG
                        && now.st_uid == staged.st_uid) => {}
            _ => {
                return Err(Error::other(format!(
                    "install executable: {} was replaced after the rename",
                    path.display()
                )))
            }
        }
        drop(file);
        let _ = flush(&self.parent, &self.parent_path);
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for Stage {
    /// Remove what this stage made through the held descriptors. The stage
    /// directory's name is removed only while it is still that directory: a
    /// symlink or a directory put there by someone else is theirs.
    fn drop(&mut self) {
        // SAFETY: dir is an open directory and EXE is NUL-terminated. No other
        // user can make a name in a 0700 directory this process owns.
        unsafe { libc::unlinkat(self.dir.as_raw_fd(), EXE.as_ptr(), 0) };
        if let (Ok(held), Ok(named)) = (stat(&self.dir), stat_at(&self.parent, &self.name)) {
            if same_file(&held, &named) {
                // SAFETY: parent is an open directory and name is
                // NUL-terminated. AT_REMOVEDIR removes only an empty directory.
                unsafe {
                    libc::unlinkat(
                        self.parent.as_raw_fd(),
                        self.name.as_ptr(),
                        libc::AT_REMOVEDIR,
                    )
                };
            }
        }
    }
}

/// The child half of [`Stage::fill`]: create `exe` in `stage`, refusing a
/// name that is already there, and write `fill` to it. Returns 0 or an
/// errno.
#[cfg(unix)]
fn write_staged(stage: libc::c_int, fill: &Fill) -> libc::c_int {
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mode: libc::c_uint = 0o600;
    // SAFETY: stage is an open directory and EXE is NUL-terminated.
    let fd = unsafe { libc::openat(stage, EXE.as_ptr(), flags, mode) };
    if fd < 0 {
        return errno();
    }
    let wrote = match fill {
        Fill::Bytes(body) => write_all(fd, body),
        Fill::Copy(from) => {
            let src = from.as_raw_fd();
            let mut buf = [0u8; 16 * 1024];
            loop {
                // SAFETY: src is open for reading and buf is writable for its
                // whole length.
                let n = unsafe { libc::read(src, buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 {
                    if errno() == libc::EINTR {
                        continue;
                    }
                    break errno();
                }
                if n == 0 {
                    break 0;
                }
                let got = usize::try_from(n).unwrap_or(0);
                let code = write_all(fd, &buf[..got]);
                if code != 0 {
                    break code;
                }
            }
        }
    };
    // SAFETY: fd was opened above and is closed once.
    let closed = unsafe { libc::close(fd) };
    if wrote != 0 {
        wrote
    } else if closed != 0 {
        errno()
    } else {
        0
    }
}

#[cfg(unix)]
fn write_all(fd: libc::c_int, mut bytes: &[u8]) -> libc::c_int {
    while !bytes.is_empty() {
        // SAFETY: fd is open for writing and bytes is readable for its length.
        #[expect(
            clippy::disallowed_methods,
            reason = "writes the staged file through a descriptor this child opened, never stdout or stderr"
        )]
        let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if n < 0 {
            if errno() == libc::EINTR {
                continue;
            }
            return errno();
        }
        bytes = &bytes[usize::try_from(n).unwrap_or(0)..];
    }
    0
}

/// The calling thread's errno, as an exit code. Reading it allocates
/// nothing, so the child of a fork may call it.
#[cfg(unix)]
fn errno() -> libc::c_int {
    match Error::last_os_error().raw_os_error() {
        Some(code) if (1..=255).contains(&code) => code,
        _ => libc::EIO,
    }
}

#[cfg(unix)]
fn open_at(dir: &File, name: &CStr, flags: libc::c_int) -> Result<File, Error> {
    // SAFETY: dir is an open directory and name is NUL-terminated.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(Error::last_os_error());
    }
    // SAFETY: fd was just opened and nothing else owns it.
    #[expect(
        clippy::disallowed_methods,
        reason = "owns a descriptor this function opened, never stdout or stderr"
    )]
    let file = unsafe { File::from_raw_fd(fd) };
    Ok(file)
}

#[cfg(unix)]
fn stat(file: &File) -> Result<libc::stat, Error> {
    // SAFETY: st is zeroed and only read after fstat fills it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: file is an open descriptor and st is writable.
    if unsafe { libc::fstat(file.as_raw_fd(), &mut st) } != 0 {
        return Err(Error::last_os_error());
    }
    Ok(st)
}

/// `fstatat` of `name` in `dir`, not following a symlink there.
#[cfg(unix)]
fn stat_at(dir: &File, name: &CStr) -> Result<libc::stat, Error> {
    // SAFETY: st is zeroed and only read after fstatat fills it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: dir is an open directory, name is NUL-terminated and st is
    // writable.
    let rc = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc != 0 {
        return Err(Error::last_os_error());
    }
    Ok(st)
}

#[cfg(unix)]
fn same_file(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev && a.st_ino == b.st_ino
}

/// Points in a publish where a test stands in for another user who renames
/// the stage directory and puts something else at its name.
#[cfg(all(test, unix))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// The stage directory is made and held.
    Staged,
    /// The writer has exited; the mode is not set yet.
    Written,
    /// The staged file is checked and flushed; the rename is next.
    Moving,
    /// The rename is done; the name is checked next.
    Renamed,
}

#[cfg(all(test, unix))]
type StepHook = Box<dyn FnMut(Step, &Path)>;

#[cfg(all(test, unix))]
thread_local! {
    static AT_STEP: std::cell::RefCell<Option<StepHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
fn at_step(step: Step, stage: &Path) {
    AT_STEP.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(step, stage);
        }
    });
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
    /// When set, a flush of a staged file fails with EIO.
    static FAIL_STAGED_FLUSH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `sync_all` on `file`, a file or a directory, opened at `path`. `settle`
/// returns the staged file's error before the rename. It drops the
/// directory's error after the rename, which a flush cannot undo.
#[cfg(unix)]
fn flush(file: &File, path: &Path) -> Result<(), Error> {
    #[cfg(test)]
    if FAIL_STAGED_FLUSH.get() && path.file_name() == Some(std::ffi::OsStr::new("exe")) {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    file.sync_all()?;
    #[cfg(test)]
    FLUSHED.with(|done| done.borrow_mut().push(path.to_path_buf()));
    #[cfg(not(test))]
    let _ = path;
    Ok(())
}

fn dir_of(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Some(dir),
        _ => None,
    }
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
    fn a_published_mode_drops_setuid_setgid_and_sticky() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stub");
        for (asked, want) in [(0o4755, 0o755), (0o2750, 0o750), (0o7700, 0o700)] {
            install_executable_mode(&path, b"#!/bin/sh\n", asked).unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
            assert_eq!(mode, want, "asked {asked:o}, got {mode:o}");
        }
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

    /// A copy takes its source's bits from the descriptor it reads. A
    /// 0700 source used to come back 0755.
    #[test]
    fn a_copy_keeps_its_sources_mode_and_drops_setuid() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dest = dir.path().join("dest");
        for (from, want) in [(0o700, 0o700), (0o750, 0o750), (0o644, 0o644), (0o4755, 0o755)] {
            install_executable_mode(&src, OURS, 0o700).unwrap();
            File::open(&src)
                .unwrap()
                .set_permissions(std::fs::Permissions::from_mode(from))
                .unwrap();
            copy_executable(&src, &dest).unwrap();
            let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o7777;
            assert_eq!(mode, want, "a {from:o} source came back {mode:o}");
            assert_eq!(std::fs::read(&dest).unwrap(), OURS);
        }
    }

    #[test]
    fn a_copy_of_something_that_is_not_a_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        let err = copy_executable(dir.path(), &dest).unwrap_err();
        assert!(err.to_string().contains("not a regular file"), "{err}");
        assert!(!dest.exists());
        let err = super::publish(&dest, Content::Bytes(OURS), Mode::OfSource).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{err}");
        assert!(!dest.exists());
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

    /// The staged file's flush fails. The publish returns that error,
    /// renames nothing onto the destination and removes its stage.
    #[test]
    fn a_failed_flush_of_the_staged_file_fails_the_publish() {
        struct Unset;
        impl Drop for Unset {
            fn drop(&mut self) {
                FAIL_STAGED_FLUSH.set(false);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        install_executable(&src, OURS).unwrap();
        let dest = dir.path().join("dest");
        let _unset = Unset;
        FAIL_STAGED_FLUSH.set(true);
        for step in ["install", "copy"] {
            let err = publish(step, &src, &dest).expect_err(step);
            let msg = err.to_string();
            assert!(msg.contains("install executable: flush "), "{step}: {msg}");
            assert!(!dest.exists(), "{step}: published after a failed flush");
            let stages: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .filter(|name| name.to_string_lossy().starts_with(".ljos-exe."))
                .collect();
            assert!(stages.is_empty(), "{step}: left {stages:?}");
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

    const OURS: &[u8] = b"#!/bin/sh\necho ours\n";

    /// Run `publish` while, at `when`, the stage directory is renamed aside
    /// and a symlink to `attack` is put at its name, as the owner of a
    /// sticky parent directory can do to another user's directory. Returns
    /// what `publish` returned and where each stage directory went.
    fn swapped_at<T>(when: Step, attack: &Path, publish: impl FnOnce() -> T) -> (T, Vec<PathBuf>) {
        struct Unhook;
        impl Drop for Unhook {
            fn drop(&mut self) {
                AT_STEP.with(|hook| *hook.borrow_mut() = None);
            }
        }
        let asides = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = std::rc::Rc::clone(&asides);
        let attack = attack.to_path_buf();
        AT_STEP.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |step, stage: &Path| {
                if step != when {
                    return;
                }
                let aside = stage.with_file_name(format!("aside-{}", seen.borrow().len()));
                std::fs::rename(stage, &aside).unwrap();
                std::os::unix::fs::symlink(&attack, stage).unwrap();
                seen.borrow_mut().push(aside);
            }));
        });
        let _unhook = Unhook;
        let got = publish();
        let asides = asides.borrow().clone();
        (got, asides)
    }

    /// A directory with `victim` (`SECRET`, `mode`), an `attack` directory
    /// whose `exe` links to `target` (a name in the same directory), and a
    /// copy source `src` with `OURS`.
    fn arena(mode: u32, target: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "SECRET").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(mode)).unwrap();
        let attack = dir.path().join("attack");
        std::fs::create_dir(&attack).unwrap();
        std::os::unix::fs::symlink(dir.path().join(target), attack.join("exe")).unwrap();
        let src = dir.path().join("src");
        install_executable(&src, OURS).unwrap();
        (dir, victim, attack, src)
    }

    fn publish(step: &str, src: &Path, dest: &Path) -> Result<(), Error> {
        if step == "install" {
            install_executable(dest, OURS)
        } else {
            copy_executable(src, dest)
        }
    }

    fn victim_state(victim: &Path) -> (String, u32) {
        let body = std::fs::read_to_string(victim).unwrap();
        let mode = std::fs::symlink_metadata(victim)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        (body, mode)
    }

    /// `dest` is a regular file, not a link, with `OURS` and mode `0o755`.
    fn assert_published(step: &str, dest: &Path) {
        let meta = std::fs::symlink_metadata(dest).unwrap();
        assert!(meta.file_type().is_file(), "{step}: {dest:?} is {meta:?}");
        assert_eq!(meta.permissions().mode() & 0o7777, 0o755, "{step}");
        assert_eq!(std::fs::read(dest).unwrap(), OURS, "{step}");
    }

    /// The stage directory is swapped for a symlink to a directory whose
    /// `exe` links to a victim, before the writer runs. Writing by path
    /// rewrote the victim; the held directory is still the one written.
    #[test]
    fn a_stage_swapped_before_the_write_is_not_written_through() {
        for step in ["copy", "install"] {
            let (dir, victim, attack, src) = arena(0o644, "victim");
            let dest = dir.path().join("stub");
            let (got, asides) = swapped_at(Step::Staged, &attack, || publish(step, &src, &dest));
            assert_eq!(
                victim_state(&victim),
                ("SECRET".to_string(), 0o644),
                "{step}: victim rewritten"
            );
            got.unwrap_or_else(|err| panic!("{step}: {err}"));
            assert_eq!(asides.len(), 1, "{step}");
            assert_published(step, &dest);
        }
    }

    /// The swap comes after the writer and before the mode is set. A
    /// `chmod` by path made a `0600` victim `0755`.
    #[test]
    fn a_stage_swapped_before_the_mode_is_set_is_not_chmod_through() {
        for step in ["copy", "install"] {
            let (dir, victim, attack, src) = arena(0o600, "victim");
            let dest = dir.path().join("stub");
            let (got, asides) = swapped_at(Step::Written, &attack, || publish(step, &src, &dest));
            assert_eq!(
                victim_state(&victim),
                ("SECRET".to_string(), 0o600),
                "{step}: victim's mode changed"
            );
            got.unwrap_or_else(|err| panic!("{step}: {err}"));
            assert_eq!(asides.len(), 1, "{step}");
            assert_published(step, &dest);
        }
    }

    /// The swap comes after the staged file was checked and before the
    /// rename. Renaming by path moved the planted link onto the
    /// destination and returned Ok.
    #[test]
    fn a_stage_swapped_before_the_rename_publishes_the_staged_file() {
        for step in ["install", "copy"] {
            let (dir, _, attack, src) = arena(0o644, "evil");
            std::fs::write(dir.path().join("evil"), "#!/bin/sh\necho PWNED\n").unwrap();
            let dest = dir.path().join("stub");
            let (got, asides) = swapped_at(Step::Moving, &attack, || publish(step, &src, &dest));
            got.unwrap_or_else(|err| panic!("{step}: {err}"));
            assert_eq!(asides.len(), 1, "{step}");
            assert_published(step, &dest);
            let out = Command::new(&dest).output().unwrap();
            assert_eq!(out.stdout, b"ours\n", "{step}");
            assert!(
                std::fs::symlink_metadata(attack.join("exe"))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{step}"
            );
        }
    }

    /// A symlink put at the destination after the rename is reported, not
    /// taken for the published file.
    #[test]
    fn a_destination_replaced_after_the_rename_is_reported() {
        for step in ["install", "copy"] {
            let (dir, _, _, src) = arena(0o644, "victim");
            let dest = dir.path().join("stub");
            let evil = dir.path().join("evil");
            let swap = dest.clone();
            AT_STEP.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |at, _: &Path| {
                    if at == Step::Renamed {
                        std::fs::remove_file(&swap).unwrap();
                        std::os::unix::fs::symlink(&evil, &swap).unwrap();
                    }
                }));
            });
            let got = publish(step, &src, &dest);
            AT_STEP.with(|hook| *hook.borrow_mut() = None);
            let err = got.expect_err(step).to_string();
            assert!(
                err.contains("was replaced after the rename"),
                "{step}: {err}"
            );
        }
    }

    /// A failed publish removes the staged file through the held directory
    /// and leaves a name someone else put at the stage's name, and what it
    /// links to.
    #[test]
    fn a_failed_publish_cleans_only_what_it_made() {
        for step in ["install", "copy"] {
            let (dir, victim, attack, src) = arena(0o644, "victim");
            let dest = dir.path().join("stub");
            std::fs::create_dir(&dest).unwrap();
            std::fs::write(dest.join("keep"), "x").unwrap();
            let (got, asides) = swapped_at(Step::Staged, &attack, || publish(step, &src, &dest));
            assert!(got.is_err(), "{step}: renamed onto a directory");
            let [aside] = &asides[..] else {
                panic!("{step}: {asides:?}");
            };
            let left: Vec<_> = std::fs::read_dir(aside).unwrap().collect();
            assert!(left.is_empty(), "{step}: {left:?} left in the stage");
            let planted = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| path.to_string_lossy().contains(".ljos-exe."))
                .unwrap_or_else(|| panic!("{step}: the planted stage name was removed"));
            assert_eq!(std::fs::read_link(&planted).unwrap(), attack, "{step}");
            assert!(
                std::fs::symlink_metadata(attack.join("exe"))
                    .is_ok_and(|meta| meta.file_type().is_symlink()),
                "{step}: the planted exe was removed"
            );
            assert_eq!(victim_state(&victim), ("SECRET".to_string(), 0o644));

            std::fs::remove_file(&planted).unwrap();
            assert!(publish(step, &src, &dest).is_err(), "{step}");
            let stages: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .filter(|name| name.to_string_lossy().starts_with(".ljos-exe."))
                .collect();
            assert!(stages.is_empty(), "{step}: left {stages:?}");
        }
    }

    /// `/dev/null` opens but cannot be synced, so a flush that skips the
    /// sync returns Ok here.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_flush_reaches_the_sync() {
        let open = |path: &str| (std::fs::File::open(path).unwrap(), PathBuf::from(path));
        let (null, at) = open("/dev/null");
        let err = flush(&null, &at).unwrap_err();
        assert_eq!(err.raw_os_error(), Some(22), "{err}");
        let (root, at) = open("/");
        assert!(flush(&root, &at).is_ok());
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
    /// scan cannot see, or that put back a hook this process never wrote:
    /// file under `crates/`, function, target, and why it is not a hook
    /// publish.
    const NOT_A_HOOK: &[(&str, &str, &str, &str)] = &[
        (
            "ljos-cli/src/cloud_hook.rs",
            "unchain",
            "&hook",
            "renames back the user's hook an earlier cloud up moved aside; no descriptor here wrote it",
        ),
        (
            "ljos-cli/src/git_hooks.rs",
            "chain_through_link",
            "&hook",
            "a test symlink to a target hook_at publishes",
        ),
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
    fn the_site_scan_names_a_hook_put_by_rename_or_link() {
        let put = r#"
fn put(dir: &Path, tmp: &Path) {
    std::fs::rename(tmp, dir.join("pre-push")).unwrap();
    std::fs::hard_link(tmp, dir.join("pre-commit")).unwrap();
    std::os::unix::fs::symlink(tmp, dir.join("post-commit")).unwrap();
    std::fs::rename(tmp, dir.join("notes.txt")).unwrap();
}
"#;
        assert_eq!(hook_lines(put), vec![3, 4, 5]);
    }

    #[test]
    fn the_site_scan_names_a_rename_or_link_by_a_used_name() {
        let aliased = r#"
use std::fs::{hard_link as link, rename as mv};
use std::os::unix::fs as unix_fs;
use std::os::unix::fs::symlink;
use std::fs as files;
fn put(dir: &Path, tmp: &Path) {
    mv(tmp, dir.join("pre-push")).unwrap();
    link(tmp, dir.join("pre-commit")).unwrap();
    symlink(tmp, dir.join("post-commit")).unwrap();
    unix_fs::symlink(tmp, dir.join("pre-push")).unwrap();
    files::rename(tmp, dir.join("pre-commit")).unwrap();
    rename(tmp, dir.join("post-commit")).unwrap();
    mv(dir.join("pre-push"), dir.join("notes.txt")).unwrap();
    let _ = std::fs::symlink_metadata(dir.join("pre-push"));
}
"#;
        assert_eq!(hook_lines(aliased), vec![7, 8, 9, 10, 11, 12]);
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

    /// Calls that open a path for writing in this process, or put a file
    /// at a path by rename or link, with the argument that names the path.
    /// Each also matches its `std::`-qualified form.
    const RAW_WRITES: &[(&str, usize)] = &[
        ("fs::write(", 0),
        ("fs::copy(", 1),
        ("fs::rename(", 1),
        ("fs::hard_link(", 1),
        ("fs::soft_link(", 1),
        ("fs::symlink(", 1),
        ("fs::symlink_file(", 1),
        ("fs::symlink_dir(", 1),
        ("File::create(", 0),
        ("File::create_new(", 0),
    ];

    /// The `fs::` calls of `RAW_WRITES` by their bare name, as a `use`
    /// brings them in.
    const BARE_WRITES: &[(&str, usize)] = &[
        ("write", 0),
        ("copy", 1),
        ("rename", 1),
        ("hard_link", 1),
        ("soft_link", 1),
        ("symlink", 1),
        ("symlink_file", 1),
        ("symlink_dir", 1),
    ];

    /// The write calls `code` can make, each with the argument that names
    /// the path and whether it is a bare name: `RAW_WRITES`, `BARE_WRITES`,
    /// and any of those, or `fs` or `File`, renamed by `use .. as ..`.
    fn write_apis(code: &str) -> Vec<(String, usize, bool)> {
        let mut out: Vec<_> = RAW_WRITES
            .iter()
            .map(|&(api, n)| (api.to_string(), n, false))
            .chain(
                BARE_WRITES
                    .iter()
                    .map(|&(name, n)| (format!("{name}("), n, true)),
            )
            .collect();
        for at in word_uses(code, "use") {
            let Some(end) = statement_end(code, at) else {
                continue;
            };
            let mut prefixes = vec![String::new()];
            let mut part = String::new();
            let mut items = Vec::new();
            for c in code[at + "use".len()..end].chars().chain([',']) {
                let prefix = prefixes.last().cloned().unwrap_or_default();
                match c {
                    '{' => {
                        prefixes.push(format!("{prefix}{}", part.trim()));
                        part.clear();
                    }
                    ',' | '}' => {
                        items.push((prefix, std::mem::take(&mut part)));
                        if c == '}' && prefixes.len() > 1 {
                            prefixes.pop();
                        }
                    }
                    _ => part.push(c),
                }
            }
            for (prefix, item) in items {
                let words: Vec<&str> = item.split_whitespace().collect();
                let Some(pos) = words.iter().position(|w| *w == "as") else {
                    continue;
                };
                let Some(&alias) = words.get(pos + 1) else {
                    continue;
                };
                if alias == "_" || !alias.chars().all(is_ident) {
                    continue;
                }
                let path = format!("{prefix}{}", words[..pos].concat());
                let last = path.rsplit("::").next().unwrap_or("");
                if path.contains("fs::") {
                    if let Some(&(_, n)) = BARE_WRITES.iter().find(|(name, _)| *name == last) {
                        out.push((format!("{alias}("), n, true));
                    }
                }
                for &(api, n) in RAW_WRITES {
                    let (module, call) = api.split_once("::").unwrap_or((api, ""));
                    if module == last {
                        out.push((format!("{alias}::{call}"), n, false));
                    }
                }
            }
        }
        out
    }

    struct Source<'a> {
        path: &'a str,
        text: &'a str,
        /// `text` with comments and the inside of literals blanked.
        code: String,
        literals: Vec<(usize, usize)>,
        lines: Vec<usize>,
        /// `write_apis` of `code`.
        writes: Vec<(String, usize, bool)>,
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
                let writes = write_apis(&code);
                Source {
                    path,
                    text,
                    code,
                    literals,
                    lines,
                    writes,
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
        for (api, n, bare) in &src.writes {
            for (at, _) in body.match_indices(api.as_str()) {
                let before = &body[..at];
                let lead = before.chars().next_back();
                if !bare
                    || (!lead.is_some_and(|c| is_ident(c) || matches!(c, '.' | ':'))
                        && !ends_with_word(before.trim_end(), "fn"))
                {
                    take(f.body.0 + at + api.len() - 1, *n);
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
