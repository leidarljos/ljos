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

/// Programs that copy, move or link the paths they are given.
const WRITING_PROGRAMS: &[&str] = &["cp", "mv", "install", "ln"];

/// Write `body` to `path`, a file that is not a git hook, without
/// following a symlink at `path`.
///
/// Code that names a git hook denies `clippy::disallowed_methods`, which
/// refuses `std::fs::write` and the other ways to put a file at a path.
/// This is how that code writes its other files.
///
/// # Errors
///
/// The file name of `path` is a git hook name, `path` is a symlink, or the
/// write fails.
pub fn write_file(path: impl AsRef<Path>, body: impl AsRef<[u8]>) -> Result<(), Error> {
    let path = path.as_ref();
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if crate::git_hooks::HOOK_NAMES.contains(&name) {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "write file: {} is named like a git hook; publish it with install_executable",
                    path.display()
                ),
            ));
        }
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?
            .write_all(body.as_ref())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, body.as_ref())
    }
}

/// A [`std::process::Command`] for `program`, which must not be a program
/// that copies, moves or links files.
///
/// Code that names a git hook denies `clippy::disallowed_methods`, which
/// refuses `Command::new`. This is how that code starts a program.
///
/// # Errors
///
/// The file name of `program` is `cp`, `mv`, `install` or `ln`.
pub fn program(program: impl AsRef<std::ffi::OsStr>) -> Result<std::process::Command, Error> {
    let program = program.as_ref();
    let name = Path::new(program).file_name().unwrap_or(program);
    if WRITING_PROGRAMS.iter().any(|writer| name == *writer) {
        return Err(Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "program: {} writes the paths it is given; publish a hook with install_executable",
                program.to_string_lossy()
            ),
        ));
    }
    Ok(std::process::Command::new(program))
}

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

    /// Mark the staged file `mode`, flush it, rename it onto `path` and
    /// check that the name now holds that file, both under a lock on the
    /// parent directory, then flush the directory so the new name survives
    /// a crash.
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
        let lock = DirLock::exclusive(&self.parent)
            .map_err(|err| named(err, "lock", &self.parent_path))?;
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
        // given to something else put at the name. Every publish into this
        // directory holds `lock` from its rename to this check, so no later
        // publish of ours can have replaced the file: anything else at the
        // name, a hard link to a file of ours included, was put there by
        // someone else.
        match stat_at(&self.parent, &leaf) {
            Ok(now) if same_file(&now, &staged) => {}
            _ => {
                return Err(Error::other(format!(
                    "install executable: {} was replaced after the rename",
                    path.display()
                )))
            }
        }
        drop(lock);
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

/// How many times [`DirLock::exclusive`] tries the lock, [`LOCK_PAUSE`]
/// apart, before it gives up.
#[cfg(unix)]
const LOCK_TRIES: u32 = 200;

#[cfg(unix)]
const LOCK_PAUSE: std::time::Duration = std::time::Duration::from_millis(25);

/// An exclusive `flock` on a directory descriptor, released on drop.
///
/// Another user who can open the directory can hold the lock too. A
/// publish there waits [`LOCK_TRIES`] times [`LOCK_PAUSE`] for them and
/// then fails.
#[cfg(unix)]
struct DirLock<'a>(&'a File);

#[cfg(unix)]
impl<'a> DirLock<'a> {
    fn exclusive(dir: &'a File) -> Result<Self, Error> {
        for _ in 0..LOCK_TRIES {
            // SAFETY: dir is an open descriptor.
            if unsafe { libc::flock(dir.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self(dir));
            }
            let err = Error::last_os_error();
            match err.kind() {
                std::io::ErrorKind::Interrupted => {}
                std::io::ErrorKind::WouldBlock => std::thread::sleep(LOCK_PAUSE),
                _ => return Err(err),
            }
        }
        Err(Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "another process held the lock for {} ms",
                LOCK_PAUSE.as_millis() * u128::from(LOCK_TRIES)
            ),
        ))
    }
}

#[cfg(unix)]
impl Drop for DirLock<'_> {
    /// A forked writer of another publish may still hold a copy of the
    /// descriptor, so the lock is released here rather than at close.
    fn drop(&mut self) {
        // SAFETY: the descriptor is open for the life of the borrow.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
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

    /// A hard link to another file of this user, put at the destination
    /// after the rename, is reported. A check that took any regular file of
    /// the same owner returned Ok with the linked `PWNED` body in place.
    #[test]
    fn a_hard_link_put_at_the_destination_after_the_rename_is_reported() {
        for step in ["install", "copy"] {
            let (dir, _, _, src) = arena(0o644, "victim");
            let dest = dir.path().join("stub");
            let evil = dir.path().join("evil-exec");
            std::fs::write(&evil, "#!/bin/sh\necho PWNED\n").unwrap();
            std::fs::set_permissions(&evil, std::fs::Permissions::from_mode(0o777)).unwrap();
            let (link, swap) = (dir.path().join("link"), dest.clone());
            AT_STEP.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |at, _: &Path| {
                    if at == Step::Renamed {
                        std::fs::hard_link(&evil, &link).unwrap();
                        std::fs::rename(&link, &swap).unwrap();
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
            assert_eq!(
                std::fs::read(&dest).unwrap(),
                b"#!/bin/sh\necho PWNED\n",
                "{step}: the link is what the check saw"
            );
        }
    }

    /// A second publish of the destination, started between the first
    /// one's rename and its check, waits for that check and then
    /// publishes. Without the lock its rename would land in that gap and
    /// the first publish would report the destination replaced.
    #[test]
    fn a_second_publish_waits_for_the_first_ones_check() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("stub");
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let second = std::rc::Rc::new(std::cell::RefCell::new(None));
        let waited = std::rc::Rc::new(std::cell::Cell::new(false));
        let (hook_done, hook_second, hook_waited) = (
            Arc::clone(&done),
            std::rc::Rc::clone(&second),
            std::rc::Rc::clone(&waited),
        );
        let target = dest.clone();
        AT_STEP.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |at, _: &Path| {
                if at != Step::Renamed || hook_second.borrow().is_some() {
                    return;
                }
                let (target, done) = (target.clone(), Arc::clone(&hook_done));
                *hook_second.borrow_mut() = Some(thread::spawn(move || {
                    let got = install_executable(&target, b"#!/bin/sh\necho second\n");
                    done.store(true, Ordering::SeqCst);
                    got
                }));
                thread::sleep(std::time::Duration::from_millis(300));
                hook_waited.set(!hook_done.load(Ordering::SeqCst));
            }));
        });
        let first = install_executable(&dest, OURS);
        AT_STEP.with(|hook| *hook.borrow_mut() = None);
        first.unwrap();
        let second = second.borrow_mut().take().expect("second publish started");
        second.join().unwrap().unwrap();
        assert!(
            waited.get(),
            "the second publish finished inside the first one's check"
        );
        assert_eq!(std::fs::read(&dest).unwrap(), b"#!/bin/sh\necho second\n");
    }

    /// Another descriptor holds the directory's lock and never lets go.
    /// A publish there gives up and names the directory instead of
    /// waiting forever.
    #[test]
    fn a_publish_gives_up_on_a_lock_that_is_never_released() {
        let dir = tempfile::tempdir().unwrap();
        let holder = File::open(dir.path()).unwrap();
        // SAFETY: holder is an open descriptor.
        assert_eq!(unsafe { libc::flock(holder.as_raw_fd(), libc::LOCK_EX) }, 0);
        let dest = dir.path().join("stub");
        let (tx, rx) = std::sync::mpsc::channel();
        let target = dest.clone();
        thread::spawn(move || {
            let _ = tx.send(install_executable(&target, OURS));
        });
        let bound = LOCK_PAUSE * LOCK_TRIES + std::time::Duration::from_secs(20);
        let got = rx
            .recv_timeout(bound)
            .unwrap_or_else(|_| panic!("the publish still waits for the lock after {bound:?}"));
        let err = got.expect_err("published while another descriptor held the lock");
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock, "{err}");
        let msg = err.to_string();
        assert!(
            msg.contains(&format!("lock {}", dir.path().display())),
            "{msg}"
        );
        assert!(!dest.exists());
        drop(holder);
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

    #[test]
    fn write_file_refuses_a_hook_name_and_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes.txt");
        write_file(&notes, "x").unwrap();
        assert_eq!(std::fs::read(&notes).unwrap(), b"x");
        for name in ["pre-push", "update", "post-commit"] {
            let err = write_file(dir.path().join("hooks").join(name), "x").unwrap_err();
            assert_eq!(
                err.kind(),
                std::io::ErrorKind::InvalidInput,
                "{name}: {err}"
            );
        }
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "SECRET").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(write_file(&link, "x").is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"SECRET");
    }

    #[test]
    fn program_refuses_a_program_that_writes_paths() {
        for name in ["cp", "/bin/mv", "install", "/usr/bin/ln"] {
            let err = program(name).unwrap_err();
            assert_eq!(
                err.kind(),
                std::io::ErrorKind::InvalidInput,
                "{name}: {err}"
            );
        }
        let out = program("true").unwrap().output().unwrap();
        assert!(out.status.success());
    }

    /// Files under `crates/` that may write an executable in this process,
    /// each with the reason it has to.
    const IN_PROCESS_EXECUTABLES: &[(&str, &str)] = &[];

    /// Where `clippy::disallowed_methods` is allowed or expected: file
    /// under `crates/`, function, and why the call there is not a hook
    /// published in place. `hook_write_forms.rs` expects the lint on every
    /// form it holds and is not listed.
    const ALLOWED_CALLS: &[(&str, &str, &str)] = &[
        (
            "ljos-cli/src/cloud_hook.rs",
            "unchain",
            "renames back the user's hook an earlier cloud up moved aside; no descriptor here wrote it",
        ),
        (
            "ljos-cli/src/git_hooks.rs",
            "ensure_executable",
            "opens a hook read-only with O_NOFOLLOW to fchmod it; nothing is written",
        ),
        (
            "ljos-cli/src/git_hooks.rs",
            "chain_through_link",
            "a test symlink to a target hook_at publishes",
        ),
    ];

    /// The lint attribute that puts code in a deny scope.
    const DENY: &str = "deny(clippy::disallowed_methods)";

    /// An attribute that sets one of `LEVELS` on one of `LINTS` turns the
    /// lint off or keeps it from failing a build.
    const LEVELS: &[&str] = &["allow(", "expect(", "warn("];
    const LINTS: &[&str] = &["clippy::disallowed_methods", "clippy::style", "clippy::all"];

    /// A file written in this process and then given an exec bit can be
    /// started while a forked child still has it open. Each of those goes
    /// through this module instead.
    #[test]
    fn executables_are_published_through_this_module() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut hits = Vec::new();
        for (rel, text) in crate_sources(crates) {
            if IN_PROCESS_EXECUTABLES.iter().any(|(path, _)| rel == *path) {
                continue;
            }
            for line in write_then_exec(&text) {
                hits.push(format!("crates/{rel}:{line}"));
            }
        }
        assert!(
            hits.is_empty(),
            "publish these with install_executable, install_executable_mode or copy_executable:\n{}",
            hits.join("\n")
        );
    }

    /// Clippy refuses the calls in `clippy.toml` inside a deny scope, by
    /// what each call resolves to. Which code is hook code is what clippy
    /// cannot tell, so every function that names a git hook must be in a
    /// deny scope: its own `#[deny(clippy::disallowed_methods)]`, one on a
    /// function around it, or `#![deny(..)]` on its file. A function that
    /// names a hook only as text takes the attribute too; it costs nothing
    /// there. Nothing may turn the lint back off except at `ALLOWED_CALLS`.
    #[test]
    fn hook_code_is_in_a_deny_scope() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let sources = crate_sources(crates);
        assert!(sources.len() > 100, "found {} sources", sources.len());
        let scan = scopes(&sources);
        let mut hits: Vec<String> = scan
            .outside
            .iter()
            .map(|(file, line, func)| {
                format!("crates/{}:{line} {func} names a git hook", sources[*file].0)
            })
            .collect();
        let mut allowed = vec![false; ALLOWED_CALLS.len()];
        for (file, line, func, attr) in &scan.overrides {
            let path = &sources[*file].0;
            if path.ends_with("/hook_write_forms.rs") && attr.starts_with("#[expect(") {
                continue;
            }
            match ALLOWED_CALLS
                .iter()
                .position(|(known, name, _)| known == path && name == func)
            {
                Some(n) => allowed[n] = true,
                None => hits.push(format!("crates/{path}:{line} {func} has {attr}")),
            }
        }
        assert!(
            hits.is_empty(),
            "put these in a {DENY} scope, or list an allow in ALLOWED_CALLS:\n{}",
            hits.join("\n")
        );
        let stale: Vec<_> = ALLOWED_CALLS
            .iter()
            .zip(&allowed)
            .filter(|(_, used)| !**used)
            .collect();
        assert!(
            stale.is_empty(),
            "ALLOWED_CALLS entries nothing matches: {stale:?}"
        );
    }

    #[test]
    fn the_scope_check_names_hook_code_outside_a_deny_scope() {
        let hooks = r#"
pub const HOOK: &str = "pre-push";
pub fn hook_path(dir: &Path) -> PathBuf {
    dir.join(HOOK)
}
#[deny(clippy::disallowed_methods)]
fn scoped(dir: &Path) {
    fn inner(dir: &Path) -> PathBuf {
        dir.join("post-commit")
    }
}
fn verb(issue: &str) {
    vissue(&["update", issue]);
    post("/v1/atoms/update");
}
fn update_hook(dir: &Path) {
    dir.join("update");
}
fn listed(dir: &Path) {
    for name in HOOK_NAMES {}
}
fn quiet(dir: &Path) {
    #[allow(
        dead_code,
        clippy::disallowed_methods,
        reason = "x"
    )]
    let _ = dir;
}
"#;
        let user = r#"
fn caller(dir: &Path) {
    let _ = crate::hooks::hook_path(dir);
}
fn other(dir: &Path) {
    let _ = dir.join("notes.txt");
}
"#;
        let denied = r#"
#![deny(clippy::disallowed_methods)]
fn put(dir: &Path) {
    let _ = dir.join("pre-commit");
}
"#;
        let sources = [
            ("ljos-cli/src/hooks.rs".to_string(), hooks.to_string()),
            ("ljos-cli/src/user.rs".to_string(), user.to_string()),
            ("ljos-cli/src/denied.rs".to_string(), denied.to_string()),
        ];
        let scan = scopes(&sources);
        let outside: Vec<_> = scan
            .outside
            .iter()
            .map(|(file, line, func)| (*file, *line, func.as_str()))
            .collect();
        assert_eq!(
            outside,
            vec![
                (0, 3, "hook_path"),
                (0, 16, "update_hook"),
                (0, 19, "listed"),
                (1, 2, "caller"),
            ]
        );
        let overrides: Vec<_> = scan
            .overrides
            .iter()
            .map(|(file, line, func, _)| (*file, *line, func.as_str()))
            .collect();
        assert_eq!(overrides, vec![(0, 23, "quiet")]);
    }

    /// The sources under `crates/` other than this file, by path under
    /// `crates/`.
    fn crate_sources(crates: &Path) -> Vec<(String, String)> {
        let mut files = Vec::new();
        rust_sources(crates, &mut files);
        files
            .iter()
            .filter(|file| !file.ends_with("exe_publish.rs"))
            .map(|file| {
                let rel = file.strip_prefix(crates).unwrap().to_string_lossy();
                (rel.to_string(), std::fs::read_to_string(file).unwrap())
            })
            .collect()
    }

    /// What `scopes` found: functions that name a hook outside a deny
    /// scope, and attributes that turn the lint off, each as file index,
    /// one-based line and function.
    struct Scopes {
        outside: Vec<(usize, usize, String)>,
        overrides: Vec<(usize, usize, String, String)>,
    }

    struct Source {
        text: String,
        code: String,
        literals: Vec<(usize, usize)>,
        lines: Vec<usize>,
        funcs: Vec<Func>,
    }

    struct Func {
        name: String,
        at: usize,
        body: (usize, usize),
        returns_path: bool,
    }

    /// A function names a hook when a string in it is a git hook file
    /// name, or it uses `HOOK_NAMES`, a constant that names a hook, or a
    /// function that returns a path and names a hook.
    fn scopes(sources: &[(String, String)]) -> Scopes {
        let srcs: Vec<Source> = sources
            .iter()
            .map(|(_, text)| {
                let (code, literals) = blank(text);
                let lines = std::iter::once(0)
                    .chain(text.match_indices('\n').map(|(at, _)| at + 1))
                    .collect();
                let funcs = functions(&code);
                Source {
                    text: text.clone(),
                    code,
                    literals,
                    lines,
                    funcs,
                }
            })
            .collect();
        let mut markers = vec!["HOOK_NAMES".to_string()];
        for src in &srcs {
            markers.extend(hook_consts(src));
        }
        loop {
            let more: Vec<String> = srcs
                .iter()
                .flat_map(|src| src.funcs.iter().map(move |f| (src, f)))
                .filter(|(src, f)| {
                    f.returns_path && !markers.contains(&f.name) && names_hook(src, f, &markers)
                })
                .map(|(_, f)| f.name.clone())
                .collect();
            if more.is_empty() {
                break;
            }
            markers.extend(more);
            markers.sort_unstable();
            markers.dedup();
        }
        let mut outside = Vec::new();
        let mut overrides = Vec::new();
        for (n, src) in srcs.iter().enumerate() {
            let line = |at: usize| src.lines.partition_point(|&start| start <= at);
            let attrs = lint_attrs(&src.code);
            let file_denied = attrs
                .iter()
                .any(|(_, attr)| attr.starts_with("#![") && attr.contains(DENY));
            let denied: Vec<&Func> = src
                .funcs
                .iter()
                .filter(|f| squeeze(attributes(&src.code, f.at)).contains(DENY))
                .collect();
            for f in &src.funcs {
                let scoped = file_denied
                    || denied
                        .iter()
                        .any(|d| d.body.0 <= f.body.0 && f.body.1 <= d.body.1);
                if !scoped && names_hook(src, f, &markers) {
                    outside.push((n, line(f.at), f.name.clone()));
                }
            }
            for (at, attr) in attrs {
                if !(LEVELS.iter().any(|level| attr.contains(level))
                    && LINTS.iter().any(|lint| attr.contains(lint)))
                {
                    continue;
                }
                let func = src
                    .funcs
                    .iter()
                    .filter(|f| f.body.0 <= at && at < f.body.1)
                    .min_by_key(|f| f.body.1 - f.body.0)
                    .or_else(|| src.funcs.iter().find(|f| f.at > at))
                    .map_or(String::new(), |f| f.name.clone());
                overrides.push((n, line(at), func, attr));
            }
        }
        outside.sort_unstable();
        overrides.sort_unstable();
        Scopes { outside, overrides }
    }

    fn names_hook(src: &Source, f: &Func, markers: &[String]) -> bool {
        let (s, e) = f.body;
        src.literals
            .iter()
            .any(|&(ls, le)| s <= ls && le <= e && is_hook_name(src, ls, le))
            || markers.iter().any(|m| has_word(&src.code[s..e], m))
    }

    /// Whether the string at `s..e` is a git hook file name. `update` is
    /// also a tracker verb and a part of URL paths, so it counts only as
    /// `hooks/update` or as the name given to `join` or `push`.
    fn is_hook_name(src: &Source, s: usize, e: usize) -> bool {
        let literal = &src.text[s..e];
        let leaf = literal.rsplit('/').next().unwrap_or(literal);
        if !crate::git_hooks::HOOK_NAMES.contains(&leaf) {
            return false;
        }
        leaf != "update"
            || literal.ends_with("hooks/update")
            || (literal == "update" && {
                let before = src.code[..s.saturating_sub(1)].trim_end();
                before.ends_with("join(") || before.ends_with("push(")
            })
    }

    /// Each attribute in `code`, `#[..]` or `#![..]`, with its offset and
    /// its text without whitespace.
    fn lint_attrs(code: &str) -> Vec<(usize, String)> {
        let b = code.as_bytes();
        let mut out = Vec::new();
        for (at, _) in code.match_indices('#') {
            let open = match b.get(at + 1) {
                Some(b'[') => at + 1,
                Some(b'!') if b.get(at + 2) == Some(&b'[') => at + 2,
                _ => continue,
            };
            let mut depth = 0usize;
            let Some(close) = (open..b.len()).find(|&j| {
                match b[j] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    _ => {}
                }
                depth == 0
            }) else {
                continue;
            };
            out.push((at, squeeze(&code[at..=close])));
        }
        out
    }

    /// The attributes, doc comments and blank lines just above the line
    /// at `at`, and that line up to `at`.
    fn attributes(code: &str, at: usize) -> &str {
        let mut start = code[..at].rfind('\n').map_or(0, |n| n + 1);
        while start > 0 {
            let prev = code[..start - 1].rfind('\n').map_or(0, |n| n + 1);
            let line = code[prev..start - 1].trim();
            if !(line.is_empty() || line.starts_with("#[")) {
                break;
            }
            start = prev;
        }
        &code[start..at]
    }

    /// Every `fn` with a body in `code`.
    fn functions(code: &str) -> Vec<Func> {
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
            let mut depth = 0usize;
            let Some(close) = (k..b.len()).find(|&j| {
                match b[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                depth == 0
            }) else {
                continue;
            };
            let Some(open) = (close..b.len()).find(|&j| {
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
            out.push(Func {
                name,
                at,
                body: (open, end + 1),
                returns_path: code[close..open].contains("Path"),
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
                let Some(end) = src.code[at..].find(';').map(|n| at + n) else {
                    continue;
                };
                if !name.is_empty()
                    && typed
                    && src
                        .literals
                        .iter()
                        .any(|&(s, e)| s > at && e <= end && is_hook_name(src, s, e))
                {
                    out.push(name);
                }
            }
        }
        out
    }

    #[test]
    fn the_site_scan_names_a_write_then_chmod() {
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
        let fine = r#"
fn dir(path: &Path) {
    std::fs::write(path.join("f"), "x").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(path.join("f"), std::fs::Permissions::from_mode(0o644)).unwrap();
}
"#;
        assert!(write_then_exec(fine).is_empty());
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
