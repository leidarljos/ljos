//! The working directory is not a place to look when the command already
//! names its project, and it is not a place to wait.
//!
//! `ljos sitting` and `ljos file -p` were run from a FUSE mount. The first
//! stat of that directory, or a git walk up from it, sat there for minutes
//! before the tracker deadline started, and the command printed nothing.
//! A `-p` or a ticket id already names the project, so the tracker and the
//! seat are resolved without reading this directory. A network or FUSE
//! mount is left unread. Any other probe, git discovery included, gives
//! up after [`CWD_PROBE`].
//!
//! A git hook is not probed. Git has already opened that checkout and is
//! waiting on the hook. A stat thread that has not started is not a mount
//! that refused to answer.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, Once};
use std::time::Duration;

/// How long a probe of the working directory may take. A quiet local
/// `stat` or `git rev-parse` finishes well inside this. A mount that does
/// not answer is skipped.
pub(crate) const CWD_PROBE: Duration = Duration::from_millis(500);

/// The directory whose git walk and project inference are off. `None`
/// when this process may still read the directory it started in.
static SKIP_FROM: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Whether `dir` is the working directory this process must not walk.
///
/// A tracker that lives under that directory is still a tracker. The walk
/// stops when it reaches this directory, and it does not state it.
#[must_use]
pub(crate) fn skip_walk(dir: &Path) -> bool {
    let held = SKIP_FROM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.as_ref().is_some_and(|root| dir == root)
}

/// Whether git discovery from the process's working directory is off.
#[must_use]
pub(crate) fn walks_off() -> bool {
    SKIP_FROM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
}

/// Leave a working directory that must not be read, before the tracker or
/// the seat looks at it.
pub(crate) fn settle() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    // `ljos git-hook` runs while the parent git waits for this process.
    // The checkout is the one git already opened. Spending the probe
    // budget here, or leaving the directory because the stat thread has
    // not been scheduled yet, holds that commit. `file` and `sitting` are
    // the commands this probe is for.
    if command_is_git_hook(&args) {
        return;
    }
    let Ok(here) = process_cwd() else {
        return;
    };
    settle_at(&here, &args, true);
}

fn settle_at(here: &Path, args: &[String], move_process_cwd: bool) {
    let named = project_named(args);
    // A network or FUSE mount is left without a stat. getattr is the wait.
    if remote_or_fuse(here) {
        park(here, move_process_cwd);
        return;
    }
    if named {
        // `-p` or the ticket id already names the project. A tracker the
        // environment names is not this directory, so this directory is
        // not stated to find one. A child `vissue` states its working
        // directory even when that tracker is set, so the process leaves
        // first. The seat's configuration is left to `pin_root`: that
        // check is what refuses a product checkout, and it is a local
        // read when this directory is not a slow mount.
        if env_tracker().is_some_and(|root| root != here) {
            park(here, move_process_cwd);
            return;
        }
        // Nothing in the environment names a tracker. One stat asks
        // whether this directory is the tracker the seat is standing in.
        // It gives up when the mount does not answer.
        if timed_is_file(&here.join("vissue.toml")).is_none() {
            park(here, move_process_cwd);
        }
        return;
    }
    if timed_metadata(here).is_none() {
        park(here, move_process_cwd);
    }
}

/// `ISSUE_ROOT` or `VISSUE_ROOT` when one is set. The working directory
/// and the seat's configuration file are not read.
fn env_tracker() -> Option<PathBuf> {
    for key in ["ISSUE_ROOT", "VISSUE_ROOT"] {
        if let Ok(raw) = std::env::var(key) {
            let raw = raw.trim();
            if !raw.is_empty() {
                return Some(PathBuf::from(raw));
            }
        }
    }
    None
}

/// A path the person typed, read from the directory the process started
/// in. A process that left that directory would otherwise look for a
/// relative path in the empty one it parked in.
#[must_use]
pub(crate) fn from_start(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let held = SKIP_FROM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match held.as_ref() {
        Some(start) => start.join(path),
        None => path.to_path_buf(),
    }
}

fn remember_skip(dir: &Path) {
    *SKIP_FROM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir.to_path_buf());
}

/// A directory a detached or background child can keep after this process
/// removes the parked directory.
///
/// The tracker root when that directory is present. Otherwise `/`. The
/// parked directory is never returned: it is removed when this process
/// exits.
pub(crate) fn stable_child_dir() -> PathBuf {
    if let Some(root) = absolute_tracker() {
        if usable_dir(&root) {
            return root;
        }
    }
    PathBuf::from("/")
}

/// Whether `path` is the empty directory this process created and may
/// remove. A directory that only shares the `ljos-cwd-` name is not.
pub(crate) fn parked_work_dir(path: &Path) -> bool {
    parked_leaf(path)
}

/// Directory this process should read as its working directory.
///
/// `park` chdirs into a directory it may remove while the process is
/// still running. `getcwd` then fails. While that directory is still the
/// cwd, callers get the tracker: the directory the process left is not
/// stated. Once `getcwd` fails, callers get the directory captured before
/// the chdir.
pub(crate) fn process_cwd() -> std::io::Result<PathBuf> {
    let saved = {
        let held = SKIP_FROM
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        held.clone()
    };
    match std::env::current_dir() {
        Ok(path) if !parked_leaf(&path) => Ok(path),
        Ok(_) => Ok(parked_stand_in()),
        Err(err) => match saved {
            Some(start) => Ok(start),
            None => Err(err),
        },
    }
}

/// Tracker, or `/`, while the process cwd is still the parked directory.
///
/// The directory captured before `park` is not stated. A named command
/// must not wait on it.
fn parked_stand_in() -> PathBuf {
    static CACHED: Mutex<Option<PathBuf>> = Mutex::new(None);
    let mut slot = CACHED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(path) = slot.as_ref() {
        return path.clone();
    }
    let path = stable_child_dir();
    *slot = Some(path.clone());
    path
}

fn started_dir() -> Option<PathBuf> {
    let held = SKIP_FROM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(start) = held.as_ref() {
        return Some(start.clone());
    }
    drop(held);
    std::env::current_dir()
        .ok()
        .filter(|path| !parked_leaf(path))
}

/// Whether `path` is the directory this process parked in.
///
/// A final component that does not start with `ljos-cwd-` is not that
/// directory, so this does not stat it. The name is not enough. The
/// directory is ours only when its device and inode are the ones stored
/// at create. Before that directory exists, a working directory the
/// person named `ljos-cwd-` is not ours, and a relative `TMPDIR` there
/// is still used.
fn parked_leaf(path: &Path) -> bool {
    let ptr = PARKED_CWD.load(Ordering::SeqCst);
    if ptr.is_null() {
        return false;
    }
    if !path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("ljos-cwd-"))
    {
        return false;
    }
    // SAFETY: `ptr` was published by `hold_until_exit` and is dropped only
    // from the atexit handler. Callers run before that handler.
    let parked = unsafe { &*ptr };
    if !parked.armed {
        return false;
    }
    directory_id(path).is_some_and(|(dev, ino)| dev == parked.dev && ino == parked.ino)
}

fn directory_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = std::fs::symlink_metadata(path).ok()?;
    meta.file_type()
        .is_dir()
        .then_some((meta.dev(), meta.ino()))
}

fn usable_dir(path: &Path) -> bool {
    // The start directory was left because stating it waits. Do not state
    // it, or a path inside it, on the way out.
    if lives_under_start(path) {
        return false;
    }
    path.is_absolute() && !remote_or_fuse(path) && stated_dir(path) && searchable(path)
}

/// Whether `chdir` into `path` can succeed. A directory mode `644` answers
/// `is_dir` and still fails `chdir` with `EACCES`.
fn searchable(path: &Path) -> bool {
    let Some(c_path) = c_string(path) else {
        return false;
    };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let flags = libc::AT_EACCESS;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let flags = 0;
    // SAFETY: `c_path` is NUL-terminated. The check does not create a file
    // and does not write to the directory.
    unsafe { libc::faccessat(libc::AT_FDCWD, c_path.as_ptr(), libc::X_OK, flags) == 0 }
}

/// `true` when `path` is a directory and the stat returns within
/// [`CWD_PROBE`]. A mount that does not answer is not a place to put a
/// child: `chdir` would wait there.
fn stated_dir(path: &Path) -> bool {
    let path = path.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(path.is_dir());
    });
    rx.recv_timeout(CWD_PROBE).unwrap_or(false)
}

fn absolute_tracker() -> Option<PathBuf> {
    let root = env_tracker().or_else(crate::tracker_place::configured)?;
    if root.is_absolute() {
        return Some(root);
    }
    started_dir()
        .filter(|path| path.is_absolute())
        .map(|base| base.join(root))
}

/// Leave `here` for an empty directory on the local temp filesystem.
///
/// A tracker the environment already names is kept. A tracker the seat's
/// configuration names, when it is not inside a product checkout, is
/// named on the environment so the command still writes there. Staying
/// in `here` is what lets a later stat wait.
///
/// The empty directory is required. A child inherits the working directory,
/// and a relative path this process did not rewrite would otherwise be
/// read from `/` or from the tracker. The directory is one this process
/// just created, and it is removed when the process exits.
fn park(here: &Path, move_process_cwd: bool) {
    if !move_process_cwd {
        return;
    }
    // A second call would drop the first guard and remove the directory
    // this process just entered. Keep that guard.
    if !PARKED_CWD.load(Ordering::SeqCst).is_null() {
        return;
    }
    // The environment did not name a tracker. The seat's configuration
    // did, and it is not this directory. Name it so the command still
    // writes there after leaving. A tracker inside a product checkout is
    // left unset: `pin_root` and `aim_tracker` refuse that write.
    if env_tracker().is_none() {
        if let Some(root) = crate::tracker_place::configured() {
            if root != here
                && !remote_or_fuse(&root)
                && crate::tracker_place::product_checkout(&root).is_none()
            {
                std::env::set_var("VISSUE_ROOT", &root);
            }
        }
    }
    remember_skip(here);
    // Resolve before `create` and the chdir. A relative `TMPDIR` would
    // otherwise be read again inside the parked directory.
    resolve_relative_dirs();
    let Some(guard) = ParkedCwd::create() else {
        return;
    };
    if std::env::set_current_dir(guard.path()).is_err() {
        return;
    }
    guard.hold_until_exit();
}

/// The empty directory [`park`] created. Dropping it removes that directory
/// and nothing else. `std::process::exit` skips destructors, so the process
/// drops it from an `atexit` handler. An unwinding panic that leaves `main`
/// reaches that handler too. A caught panic does not: the command is still
/// using the directory.
struct ParkedCwd {
    path: PathBuf,
    dev: u64,
    ino: u64,
    /// Directory fd for the temp directory, opened before `chdir`. Removal
    /// uses it so a later replacement of that path is not the parent.
    /// Linux opens it with `O_PATH` and Darwin with `O_SEARCH`, so a
    /// directory without read permission still yields a fd. `None` when
    /// that open failed.
    parent: Option<std::os::unix::io::RawFd>,
    /// Set when this value is the guard that may remove `path`. A second
    /// guard is disarmed so its drop cannot remove the first directory.
    armed: bool,
}

impl ParkedCwd {
    /// `create_dir`, not `create_dir_all`. A path that already exists was
    /// not created here and is left in place.
    ///
    /// The stored path is absolute. A relative or empty `TMPDIR`, or one
    /// whose directory is missing, would otherwise be resolved again after
    /// `chdir` and the directory this process created would be left behind.
    fn create() -> Option<Self> {
        let parent_path = parked_parent()?;
        let parent = open_directory(&parent_path);
        let path = parent_path.join(format!("ljos-cwd-{}", std::process::id()));
        if std::fs::create_dir(&path).is_err() {
            close_fd(parent);
            return None;
        }
        use std::os::unix::fs::MetadataExt as _;
        let Some(meta) = std::fs::symlink_metadata(&path)
            .ok()
            .filter(|meta| meta.file_type().is_dir())
        else {
            let _ = std::fs::remove_dir(&path);
            close_fd(parent);
            return None;
        };
        Some(Self {
            dev: meta.dev(),
            ino: meta.ino(),
            path,
            parent,
            armed: true,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn hold_until_exit(self) {
        register_park_cleanup();
        let ptr = Box::into_raw(Box::new(self));
        if PARKED_CWD
            .compare_exchange(
                std::ptr::null_mut(),
                ptr,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            // SAFETY: `ptr` came from `Box::into_raw` and was not stored.
            // Disarming it keeps Drop from removing the first directory.
            unsafe {
                (*ptr).armed = false;
                drop(Box::from_raw(ptr));
            }
        } else {
            PARK_OWNER.store(std::process::id(), Ordering::SeqCst);
        }
    }

    /// Remove the parked name through the temp-directory fd when that name
    /// is still the directory created here. The stored path is not resolved
    /// again. When that fd is missing, the directory's own fd is checked
    /// the same way and the path is removed only if it still matches.
    fn unlink_at_parent(&self) {
        let Some(name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        let Ok(c_name) = std::ffi::CString::new(name) else {
            return;
        };
        if let Some(parent) = self.parent {
            self.unlink_name(parent, &c_name);
            return;
        }
        self.unlink_own();
    }

    fn unlink_name(&self, parent: std::os::unix::io::RawFd, c_name: &std::ffi::CStr) {
        // SAFETY: `parent` is the directory opened at create. `c_name` is
        // the single component created in it. The buffer is zeroed and only
        // read when `fstatat` fills it. `AT_SYMLINK_NOFOLLOW` does not follow
        // a symlink at this name.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc =
            unsafe { libc::fstatat(parent, c_name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
        if rc != 0 || !stat_matches(&st, self.dev, self.ino) {
            return;
        }
        // The residual race needs a writer in the parent dir (same uid
        // under sticky /tmp) and can only hit an empty dir.
        // SAFETY: same fd and name as the `fstatat` above.
        // `AT_REMOVEDIR` does not follow a symlink.
        unsafe {
            libc::unlinkat(parent, c_name.as_ptr(), libc::AT_REMOVEDIR);
        }
    }

    fn unlink_own(&self) {
        let Some(fd) = open_directory(&self.path) else {
            return;
        };
        // SAFETY: `fd` refers to `path`. The buffer is zeroed and only read
        // when `fstat` fills it.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::fstat(fd, &mut st) };
        if rc == 0 && stat_matches(&st, self.dev, self.ino) {
            if let Some(c_path) = c_string(&self.path) {
                // SAFETY: `c_path` is the directory this process created,
                // and the `fstat` above matched its device and inode.
                unsafe {
                    libc::rmdir(c_path.as_ptr());
                }
            }
        }
        close_fd(Some(fd));
    }
}

#[cfg_attr(
    target_os = "macos",
    allow(
        clippy::cast_sign_loss,
        reason = "Apple dev_t is i32; MetadataExt::dev casts that field to u64"
    )
)]
#[allow(
    clippy::unnecessary_cast,
    reason = "Apple dev_t is i32; other targets already use u64"
)]
fn stat_matches(st: &libc::stat, dev: u64, ino: u64) -> bool {
    (st.st_mode & libc::S_IFMT) == libc::S_IFDIR
        && (st.st_dev as u64) == dev
        && (st.st_ino as u64) == ino
}

fn c_string(path: &Path) -> Option<std::ffi::CString> {
    std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str())).ok()
}

fn close_fd(fd: Option<std::os::unix::io::RawFd>) {
    let Some(fd) = fd else {
        return;
    };
    // SAFETY: `fd` is an open descriptor this process owns. `close` only
    // releases it.
    unsafe {
        libc::close(fd);
    }
}

/// Open `path` as a directory fd.
///
/// Linux and Android use `O_PATH`, which does not need read permission.
/// Darwin tries `O_SEARCH`, then `O_RDONLY` with `O_DIRECTORY`.
fn open_directory(path: &Path) -> Option<std::os::unix::io::RawFd> {
    let c_path = c_string(path)?;
    for flags in directory_flags() {
        // SAFETY: `c_path` is NUL-terminated. The flags do not include
        // `O_CREAT`, so this does not create a file.
        #[expect(
            clippy::disallowed_methods,
            reason = "opens a directory fd to remove the parked cwd; it does not publish a file"
        )]
        let fd = unsafe { libc::open(c_path.as_ptr(), *flags, 0) };
        if fd >= 0 {
            return Some(fd);
        }
    }
    None
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn directory_flags() -> &'static [libc::c_int] {
    const FLAGS: [libc::c_int; 1] = [libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC];
    &FLAGS
}

#[cfg(target_os = "macos")]
fn directory_flags() -> &'static [libc::c_int] {
    const FLAGS: [libc::c_int; 2] = [
        libc::O_SEARCH | libc::O_CLOEXEC,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
    ];
    &FLAGS
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn directory_flags() -> &'static [libc::c_int] {
    const FLAGS: [libc::c_int; 1] = [libc::O_RDONLY | libc::O_CLOEXEC];
    &FLAGS
}

/// `TMPDIR` after [`resolve_relative_dirs`] turned a relative or empty
/// value into an absolute path. The home-under-temp check must keep
/// judging the value the process was given: a relative `TMPDIR` is not a
/// temp root. The path stored here is that absolute path, and only while
/// the variable still holds it.
static REWRITTEN_TMPDIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// `LJOS_HUD_BIN` after it was resolved to a file before the park. Later
/// checks use this and do not state the start directory again.
static RESOLVED_HUD: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Whether `TMPDIR` is the absolute path this process wrote for a relative
/// or empty value. An absolute `TMPDIR` the process was given stays a
/// temp root.
pub(crate) fn tmpdir_rewritten_from_relative() -> bool {
    let Some(raw) = std::env::var_os("TMPDIR") else {
        return false;
    };
    let held = REWRITTEN_TMPDIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.as_ref().is_some_and(|path| path.as_os_str() == raw)
}

/// HUD program found under the start directory before the park.
///
/// `LJOS_HUD_BIN`, the sibling beside this binary, and `PATH` are walked
/// once. A later lookup returns this path and does not state the start
/// directory again.
pub(crate) fn resolved_hud() -> Option<PathBuf> {
    RESOLVED_HUD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Whether `path` is the HUD program resolved before the park.
///
/// `Some(true)` when it is: the caller must not state it again. `Some(false)`
/// when stating `path` would read the start directory. `None` when the
/// caller may state `path` itself.
pub(crate) fn prepared_program(path: &Path) -> Option<bool> {
    let held = RESOLVED_HUD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if held.as_ref().is_some_and(|ready| ready == path) {
        return Some(true);
    }
    drop(held);
    lives_under_start(path).then_some(false)
}

fn lives_under_start(path: &Path) -> bool {
    let held = SKIP_FROM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.as_ref()
        .is_some_and(|start| path == start || path.starts_with(start))
}

/// Set on the one call that walks relative environment paths.
static ENV_RESOLVED: AtomicBool = AtomicBool::new(false);

/// Make `TMPDIR`, `VISSUE_ROOT`, `ISSUE_ROOT`, and `LJOS_HUD_BIN` safe to
/// use after this process leaves the directory it started in.
///
/// Called once, before the chdir into the parked directory. A fast command
/// does not park and does not call this, so those variables stay as given.
/// A relative value, and an absolute value that lies under the start
/// directory, is walked once. `..` after a symlink is that symlink's
/// parent. A missing final component of a tracker root is the parent's
/// resolved path plus that name, which is the path a create would use.
/// The walk gives up after [`CWD_PROBE`]. A path that does not answer, a
/// parent that does not resolve, or a `..` walk through a missing
/// component, is an error. A relative `TMPDIR` with no `..` that is not a
/// directory uses [`default_temp_dir`]. An empty `TMPDIR` uses that
/// default. An absolute value outside the start directory is left as set,
/// unless a symlink in that spelling lands inside the start directory. An
/// unset `TMPDIR` is left unset. `LJOS_HUD_BIN` keeps the bytes it was
/// given. The launcher trims a copy; the walk states that trimmed path
/// and does not write the variable back. When `LJOS_HUD_BIN` is unset,
/// the `ljos-hud` beside this binary and each absolute `PATH` entry are
/// stated on the same deadline.
fn resolve_relative_dirs() {
    let Some(start) = start_dir_for_env() else {
        return;
    };
    if !start.is_absolute() {
        return;
    }
    if ENV_RESOLVED.swap(true, Ordering::SeqCst) {
        return;
    }
    resolve_tmpdir(&start);
    // vissue reads `ISSUE_ROOT` when that variable is set and does not
    // read `VISSUE_ROOT`. An empty `ISSUE_ROOT` is set, so it does not
    // fall through. A missing `VISSUE_ROOT` is not an error in that case.
    match std::env::var_os("ISSUE_ROOT") {
        Some(raw) if !raw.is_empty() => resolve_root("ISSUE_ROOT", &start),
        Some(_) => {}
        None => resolve_root("VISSUE_ROOT", &start),
    }
    resolve_hud(&start);
}

fn start_dir_for_env() -> Option<PathBuf> {
    let held = SKIP_FROM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(start) = held.clone() {
        return Some(start);
    }
    drop(held);
    std::env::current_dir().ok()
}

fn resolve_tmpdir(start: &Path) {
    let Some(raw) = std::env::var_os("TMPDIR") else {
        return;
    };
    let given = PathBuf::from(&raw);
    if raw.is_empty() {
        adopt_tmpdir(default_temp_dir());
        return;
    }
    let Some(rel) = under_start(start, &given) else {
        return;
    };
    let wrote_relative = !given.is_absolute();
    match resolve_from_start(start, &rel, false) {
        Ok(hit) if hit.is_dir => {
            // A relative directory is rewritten. An absolute directory was
            // only stated: the value the process was given stays a temp root.
            if wrote_relative {
                adopt_tmpdir(hit.path);
            }
        }
        Ok(_) if wrote_relative => adopt_tmpdir(default_temp_dir()),
        Ok(_) => {}
        // The worker had not started. That is not a mount that hung.
        Err(Fault::Late) => {}
        Err(Fault::TimedOut) => refuse_env("TMPDIR", &raw),
        Err(Fault::Unresolvable) if climbs(&rel) => refuse_env("TMPDIR", &raw),
        Err(Fault::Unresolvable) if wrote_relative => adopt_tmpdir(default_temp_dir()),
        Err(Fault::Unresolvable) => {}
    }
}

fn resolve_root(key: &str, start: &Path) {
    let Some(raw) = std::env::var_os(key) else {
        return;
    };
    let given = PathBuf::from(&raw);
    if let Some(rel) = under_start(start, &given) {
        apply_root(key, &raw, resolve_from_start(start, &rel, true));
        return;
    }
    // An absolute spelling whose parent is a symlink into the start
    // directory is not a lexical prefix of getcwd. The leaf is still
    // that directory. A stat of the spelling that returns keeps the
    // text. A stat that does not answer is replaced with the physical
    // path, so a later stat does not wait on this thread.
    let Some(rel) = symlink_rel(start, &given) else {
        return;
    };
    match spelling_answered(&given) {
        Ok(()) | Err(Fault::Late) => {}
        Err(Fault::TimedOut) => match resolve_from_start(start, &rel, true) {
            Ok(hit) => {
                std::env::set_var(key, hit.path);
            }
            Err(Fault::Late) => {}
            Err(_) => refuse_env(key, &raw),
        },
        Err(Fault::Unresolvable) => refuse_env(key, &raw),
    }
}

fn apply_root(key: &str, raw: &std::ffi::OsStr, result: Result<Hit, Fault>) {
    match result {
        Ok(hit) => std::env::set_var(key, hit.path),
        Err(Fault::Late) => {}
        Err(_) => refuse_env(key, raw),
    }
}

fn resolve_hud(start: &Path) {
    if let Some(raw) = std::env::var_os("LJOS_HUD_BIN") {
        if let Some(given) = trimmed_hud(&raw) {
            resolve_hud_override(start, &raw, &given);
            return;
        }
    }
    resolve_hud_search(start);
}

/// `LJOS_HUD_BIN` with surrounding whitespace removed.
///
/// The launcher trims this variable. The walk has to see that same path,
/// or a trailing space is stated and the file itself never is. A value
/// that is empty after trimming is unset, and the sibling and `PATH`
/// search runs.
fn trimmed_hud(raw: &std::ffi::OsStr) -> Option<PathBuf> {
    let text = raw.to_str()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn resolve_hud_override(start: &Path, raw: &std::ffi::OsStr, given: &Path) {
    let Some(rel) = under_start(start, given) else {
        return;
    };
    // The variable keeps the bytes it was given. The launcher trims a
    // copy and execs that path. Remember that path when it is a regular
    // file so the launcher does not state the start directory again.
    let remembered = hud_absolute(start, given);
    let stated = joined_for_kernel(start, &rel);
    match stat_program(stated) {
        Ok(true) => remember_hud(remembered),
        Ok(false) | Err(Fault::Unresolvable) | Err(Fault::Late) => {}
        Err(Fault::TimedOut) => refuse_env("LJOS_HUD_BIN", raw),
    }
}

/// Absolute path the launcher will exec for `given`.
///
/// A relative value is joined to `start`, which is what [`from_start`]
/// returns after the park. The join does not state `start`.
fn hud_absolute(start: &Path, given: &Path) -> PathBuf {
    if given.is_absolute() {
        given.to_path_buf()
    } else {
        start.join(given)
    }
}

/// Sibling `ljos-hud`, then each `PATH` entry, when `LJOS_HUD_BIN` is unset.
///
/// A candidate is stated on the deadline, never on this thread. The first
/// regular file is remembered. A relative `PATH` entry is not a candidate:
/// after the park it is relative to the parked directory, which is where
/// the launcher looks, and it is not the start directory. A stat under
/// `start` that does not answer is an error. A stat outside `start` that
/// does not answer is not a program, and the search moves on.
fn resolve_hud_search(start: &Path) {
    if let Some(sibling) = sibling_hud() {
        if claim_hud(start, &sibling, "ljos-hud") {
            return;
        }
    }
    let Some(path) = std::env::var_os("PATH") else {
        return;
    };
    for dir in std::env::split_paths(&path) {
        if claim_hud(start, &dir.join("ljos-hud"), "PATH") {
            return;
        }
    }
}

fn sibling_hud() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("ljos-hud"))
}

/// `true` when `candidate` is the regular file the launcher would run.
///
/// `.`, an empty `PATH` component, and any other relative entry are not
/// resolved here. The launcher resolves them from the directory it is in
/// after the park.
fn claim_hud(start: &Path, candidate: &Path, key: &str) -> bool {
    if !candidate.is_absolute() {
        return false;
    }
    let inside = under_start(start, candidate).is_some();
    let stated = match under_start(start, candidate) {
        Some(rel) => joined_for_kernel(start, &rel),
        None => candidate.to_path_buf(),
    };
    match stat_program(stated) {
        Ok(true) => {
            remember_hud(candidate.to_path_buf());
            true
        }
        Ok(false) | Err(Fault::Unresolvable) | Err(Fault::Late) => false,
        Err(Fault::TimedOut) if inside => refuse_env(key, candidate.as_os_str()),
        Err(Fault::TimedOut) => false,
    }
}

fn remember_hud(path: PathBuf) {
    *RESOLVED_HUD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
}

/// `given` relative to `start` when the park must state it first.
///
/// A relative path is already that form. An absolute path that is `start`,
/// or a path inside it, may be slow to state after the park. An absolute
/// path outside `start` is `None` and stays as set. An empty value is
/// `None`: the caller has nothing to walk.
fn under_start(start: &Path, given: &Path) -> Option<PathBuf> {
    if given.as_os_str().is_empty() {
        return None;
    }
    if !given.is_absolute() {
        return Some(given.to_path_buf());
    }
    if given == start {
        return Some(PathBuf::new());
    }
    given.strip_prefix(start).ok().map(PathBuf::from)
}

fn adopt_tmpdir(path: PathBuf) {
    if !path.is_absolute() {
        return;
    }
    std::env::set_var("TMPDIR", &path);
    *REWRITTEN_TMPDIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
}

/// `..` is in `path`, so a missing component must not be popped away.
fn climbs(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

struct Hit {
    path: PathBuf,
    is_dir: bool,
}

enum Fault {
    /// The worker entered the walk and the walk did not return.
    TimedOut,
    /// The worker had not entered the walk. A late schedule, not a hung mount.
    Late,
    Unresolvable,
}

/// `start` joined with `rel` as the kernel will walk it. `..` stays in the
/// text. [`Path::join`] is not used: a parent component must not be dropped
/// before `realpath` sees the symlink it applies to.
fn joined_for_kernel(start: &Path, rel: &Path) -> PathBuf {
    let mut text = start.as_os_str().to_owned();
    text.push("/");
    text.push(rel.as_os_str());
    PathBuf::from(text)
}

/// Canonical path of `rel` from `start`, or why that walk failed.
///
/// The walk runs on another thread and stops after [`CWD_PROBE`]. When
/// `missing_leaf` is set and the final component is a normal name whose
/// parent resolves, the result is that parent plus the name. That is the
/// path a create would use, not a guess at the start directory. A `..`
/// component is not created that way. A parent that does not resolve is
/// `Fault::Unresolvable`, and a walk that does not answer is
/// `Fault::TimedOut`.
fn resolve_from_start(start: &Path, rel: &Path, missing_leaf: bool) -> Result<Hit, Fault> {
    let path = joined_for_kernel(start, rel);
    let create_leaf =
        missing_leaf && matches!(rel.components().next_back(), Some(Component::Normal(_)));
    bounded(move || walk_path(&path, create_leaf)).and_then(std::convert::identity)
}

/// What the worker thread has sent.
enum Msg<T> {
    /// The closure is running and is about to do its work.
    Started,
    /// The work returned.
    Done(T),
}

/// Run `work` on another thread and stop waiting after [`CWD_PROBE`].
///
/// `Err(Fault::Late)` means the worker had not entered `work`. That is a
/// late schedule, not a path that refused to answer. `Err(Fault::TimedOut)`
/// means the worker entered `work` and `work` did not return. The caller's
/// thread does not state the path.
fn bounded<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, Fault> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(Msg::Started);
        let _ = tx.send(Msg::Done(work()));
    });
    let started = match rx.recv_timeout(CWD_PROBE) {
        Ok(Msg::Started) => true,
        Ok(Msg::Done(value)) => return Ok(value),
        Err(_) => false,
    };
    if !started {
        return Err(Fault::Late);
    }
    match rx.recv_timeout(CWD_PROBE) {
        Ok(Msg::Done(value)) => Ok(value),
        _ => Err(Fault::TimedOut),
    }
}

/// Whether `path` is a regular file. The stat runs on the deadline.
///
/// `Ok(false)` is a path that was stated and is not a regular file. A
/// symlink to a device and a fifo are that. `Err(Fault::TimedOut)` is a
/// stat that does not answer. `Err(Fault::Late)` is a worker that had not
/// started.
fn stat_program(path: PathBuf) -> Result<bool, Fault> {
    bounded(move || match std::fs::metadata(&path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(_) => Err(Fault::Unresolvable),
    })
    .and_then(std::convert::identity)
}

/// `true` when a stat of `path` returns within the deadline.
///
/// The stat is of `path` itself. The parent is not canonicalized: a
/// symlink in the parent would otherwise be followed into the start
/// directory. An error from the stat is still an answer.
fn spelling_answered(path: &Path) -> Result<(), Fault> {
    let path = path.to_path_buf();
    bounded(move || {
        let _ = std::fs::metadata(&path);
    })
}

/// `given` relative to `start` when a symlink prefix lands in `start`.
///
/// The leaf is not stated. The start directory is not stated. `None` when
/// the prefix does not land there, or when that walk does not answer.
fn symlink_rel(start: &Path, given: &Path) -> Option<PathBuf> {
    let start = start.to_path_buf();
    let given = given.to_path_buf();
    bounded(move || symlink_into_start(&start, &given)).unwrap_or_default()
}

fn symlink_into_start(start: &Path, given: &Path) -> Option<PathBuf> {
    if !given.is_absolute() {
        return None;
    }
    let name = given.file_name()?;
    let parent = given.parent().filter(|dir| !dir.as_os_str().is_empty())?;
    let resolved = resolve_prefix(start, parent)?;
    if resolved == start {
        return Some(PathBuf::from(name));
    }
    let rest = resolved.strip_prefix(start).ok()?;
    let mut rel = PathBuf::from(rest);
    rel.push(name);
    Some(rel)
}

/// `path` with symlink components resolved, without stating `start`.
fn resolve_prefix(start: &Path, path: &Path) -> Option<PathBuf> {
    let mut pending: Vec<std::ffi::OsString> = path
        .components()
        .map(|comp| comp.as_os_str().to_os_string())
        .collect();
    let mut out = PathBuf::new();
    let mut guard = 0u32;
    while !pending.is_empty() {
        guard += 1;
        if guard > 64 {
            return None;
        }
        let comp = pending.remove(0);
        if comp == "/" {
            out.push("/");
            continue;
        }
        if comp == "." {
            continue;
        }
        if comp == ".." {
            if !out.pop() {
                return None;
            }
            continue;
        }
        out.push(&comp);
        if out == start || out.starts_with(start) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&out).ok()?;
        if !meta.file_type().is_symlink() {
            continue;
        }
        let target = std::fs::read_link(&out).ok()?;
        let rebuilt = if target.is_absolute() {
            target
        } else {
            out.parent().unwrap_or(Path::new("/")).join(target)
        };
        let mut front: Vec<std::ffi::OsString> = rebuilt
            .components()
            .map(|comp| comp.as_os_str().to_os_string())
            .collect();
        front.append(&mut pending);
        pending = front;
        out = PathBuf::new();
    }
    Some(out)
}

/// Whether `path` is a regular file, stated on the deadline.
///
/// `false` when the path is not a regular file, the stat does not answer,
/// or the worker had not started. The caller moves on. The stat does not
/// run on this thread.
pub(crate) fn program_file(path: &Path) -> bool {
    stat_program(path.to_path_buf()).unwrap_or_default()
}

/// Canonical `path`, or the parent's canonical path plus a missing final
/// name when `create_leaf` is set.
fn walk_path(path: &Path, create_leaf: bool) -> Result<Hit, Fault> {
    if let Ok(abs) = std::fs::canonicalize(path) {
        let is_dir = std::fs::metadata(&abs)
            .map(|meta| meta.is_dir())
            .unwrap_or(false);
        return Ok(Hit { path: abs, is_dir });
    }
    if !create_leaf {
        return Err(Fault::Unresolvable);
    }
    let Some(name) = path.file_name() else {
        return Err(Fault::Unresolvable);
    };
    let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) else {
        return Err(Fault::Unresolvable);
    };
    let Ok(dir) = std::fs::canonicalize(parent) else {
        return Err(Fault::Unresolvable);
    };
    // The start directory is a directory: this process began there.
    // `is_dir` would state that path, and the deadline would refuse a
    // missing leaf whose parent is the directory we already left unread.
    if !parent_is_start(&dir) && !dir.is_dir() {
        return Err(Fault::Unresolvable);
    }
    Ok(Hit {
        path: dir.join(name),
        is_dir: false,
    })
}

/// `dir` is the directory this process started in.
///
/// `realpath` of that directory does not state it. Comparing the two
/// paths is enough to know the parent of a missing leaf is a directory.
fn parent_is_start(dir: &Path) -> bool {
    let Some(start) = started_dir() else {
        return false;
    };
    if dir == start {
        return true;
    }
    std::fs::canonicalize(&start).is_ok_and(|canon| canon == dir)
}

fn refuse_env(key: &str, raw: &std::ffi::OsStr) -> ! {
    let shown = raw.to_string_lossy();
    crate::say!("ljos: {key}={shown} did not resolve against the start directory");
    std::process::exit(1);
}

/// Absolute temp directory for the parked cwd. An empty `TMPDIR`, or one
/// that does not name a directory, uses the platform default (`/tmp` when
/// `TMPDIR` is unset on Linux). A relative value was resolved before this
/// runs. A relative value that is still set is joined to the current
/// directory.
fn parked_parent() -> Option<PathBuf> {
    let raw = std::env::var_os("TMPDIR").unwrap_or_default();
    if !raw.is_empty() {
        let given = PathBuf::from(raw);
        let absolute = if given.is_absolute() {
            Some(given)
        } else {
            process_cwd().ok().map(|cwd| cwd.join(given))
        };
        if let Some(path) = absolute.filter(|path| path.is_dir()) {
            return Some(path);
        }
    }
    let fallback = default_temp_dir();
    fallback.is_dir().then_some(fallback)
}

fn default_temp_dir() -> PathBuf {
    // `temp_dir` returns `TMPDIR` as set, including an empty string (Rust
    // 1.89). When `TMPDIR` is unset it is already the platform default, and
    // reading it does not change the environment. A stat of a slow working
    // directory may still be running, so this does not clear `TMPDIR`.
    if std::env::var_os("TMPDIR").is_none() {
        return std::env::temp_dir();
    }
    #[cfg(target_os = "macos")]
    {
        darwin_user_temp().unwrap_or_else(|| PathBuf::from("/tmp"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from(if cfg!(target_os = "android") {
            "/data/local/tmp"
        } else {
            "/tmp"
        })
    }
}

#[cfg(target_os = "macos")]
fn darwin_user_temp() -> Option<PathBuf> {
    let mut buf = vec![0u8; 1024];
    // SAFETY: `buf` is writable and its length is the size passed to confstr.
    let mut n = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    if n > buf.len() {
        buf.resize(n, 0);
        // SAFETY: `buf` was resized to the size confstr reported, and that
        // length is what this second call passes.
        n = unsafe {
            libc::confstr(
                libc::_CS_DARWIN_USER_TEMP_DIR,
                buf.as_mut_ptr().cast(),
                buf.len(),
            )
        };
    }
    if n <= 1 || n > buf.len() {
        return None;
    }
    let end = n - 1;
    let text = std::str::from_utf8(&buf[..end]).ok()?;
    let path = PathBuf::from(text.trim_end_matches('\0'));
    path.is_absolute().then_some(path)
}

impl Drop for ParkedCwd {
    fn drop(&mut self) {
        if self.armed {
            self.unlink_at_parent();
        }
        close_fd(self.parent.take());
    }
}

/// The parked directory, held until exit. Null when this process did not
/// create one, or after it has been dropped.
static PARKED_CWD: AtomicPtr<ParkedCwd> = AtomicPtr::new(std::ptr::null_mut());

/// Process that armed [`PARKED_CWD`]. A fork copies the guard and this
/// handler; `exit` in the child must not remove the parent's directory.
static PARK_OWNER: AtomicU32 = AtomicU32::new(0);

/// Directory to use when leaving the parked cwd.
///
/// Relative environment paths were resolved once, before the park. This
/// does not state the start directory. `None` when the current directory
/// is not the parked directory this process created. Callers then keep
/// the directory they started in.
pub(crate) fn parked_replacement_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    if !parked_leaf(&cwd) {
        return None;
    }
    Some(stable_child_dir())
}

/// Remove the parked directory before a call that skips `atexit`.
///
/// `exec` replaces this process, so the handler registered at park does
/// not run. The directory is removed the same way that handler removes it.
pub(crate) fn release_parked_dir() {
    release_parked_cwd();
}

extern "C" fn release_parked_cwd() {
    // SAFETY: `getpid` takes no pointers and only reads the caller.
    let pid = unsafe { libc::getpid() };
    if u32::try_from(pid).ok() != Some(PARK_OWNER.load(Ordering::SeqCst)) {
        return;
    }
    let ptr = PARKED_CWD.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if ptr.is_null() {
        return;
    }
    // SAFETY: `ptr` came from `Box::into_raw` and this swap is the only
    // take. A second call finds null.
    drop(unsafe { Box::from_raw(ptr) });
}

fn register_park_cleanup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Signals, abort and _exit skip atexit, so the directory can remain.
        // SAFETY: the handler has C linkage, captures nothing, and only
        // drops the directory this process created.
        unsafe {
            libc::atexit(release_parked_cwd);
        }
    });
}

/// Whether `args`, after the program name, are `ljos git-hook`.
///
/// Flags may precede the subcommand. A `git-hook` after `--` is an
/// argument of another command.
#[must_use]
fn command_is_git_hook(args: &[String]) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return false;
        }
        if matches!(arg, "--seat" | "--assignee" | "--parent" | "--event") {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        return arg == "git-hook";
    }
    false
}

/// Whether `args`, after the program name, already name a project.
///
/// `-p` and `--project` name it. So does a ticket id on `sitting`, `note`,
/// `show`, and the other commands that take one: the project is the text
/// before the last `-`.
#[must_use]
pub(crate) fn project_named(args: &[String]) -> bool {
    let mut cmd = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            break;
        }
        if let Some(name) = arg.strip_prefix("--project=") {
            return !name.is_empty();
        }
        if arg == "-p" || arg == "--project" {
            return args
                .get(index + 1)
                .is_some_and(|value| !value.is_empty() && !value.starts_with('-'));
        }
        if matches!(arg, "--seat" | "--assignee" | "--parent" | "--event") {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        if cmd.is_none() {
            cmd = Some(arg);
            index += 1;
            continue;
        }
        if ticket_names_project(arg)
            && matches!(
                cmd,
                Some(
                    "sitting"
                        | "note"
                        | "show"
                        | "finish"
                        | "claim"
                        | "release"
                        | "reopen"
                        | "vote"
                        | "complete"
                        | "withdraw"
                )
            )
        {
            return true;
        }
        index += 1;
    }
    false
}

/// Whether `word` is an id whose project is the text before the last `-`.
fn ticket_names_project(word: &str) -> bool {
    let Some((project, suffix)) = word.rsplit_once('-') else {
        return false;
    };
    !project.is_empty()
        && !suffix.is_empty()
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase())
}

/// Whether `path` stands on a network or FUSE mount.
///
/// Read from `/proc/self/mountinfo`. The path itself is not stated: a
/// FUSE getattr is the wait this function exists to avoid.
#[must_use]
pub(crate) fn remote_or_fuse(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    slow_mount(&text, &path.to_string_lossy())
}

/// [`remote_or_fuse`] for one mountinfo document and one path.
#[must_use]
fn slow_mount(mountinfo: &str, path: &str) -> bool {
    let mut best: Option<(usize, bool)> = None;
    for line in mountinfo.lines() {
        let Some((pre, post)) = line.split_once(" - ") else {
            continue;
        };
        let Some(mount) = pre.split_whitespace().nth(4) else {
            continue;
        };
        let mount = unescape_mount(mount);
        let fstype = post.split_whitespace().next().unwrap_or("");
        let under = path == mount || mount == "/" || path.starts_with(&(mount.clone() + "/"));
        if !under {
            continue;
        }
        let slow = fs_is_slow(fstype);
        if best.as_ref().is_none_or(|(len, _)| mount.len() >= *len) {
            best = Some((mount.len(), slow));
        }
    }
    best.is_some_and(|(_, slow)| slow)
}

fn unescape_mount(mount: &str) -> String {
    mount.replace("\\040", " ").replace("\\011", "\t")
}

/// Whether `fstype` is a network or FUSE filesystem.
#[must_use]
fn fs_is_slow(fstype: &str) -> bool {
    let kind = fstype.to_ascii_lowercase();
    kind.starts_with("fuse")
        || kind.starts_with("nfs")
        || kind == "cifs"
        || kind.starts_with("smb")
        || kind.starts_with("9p")
        || kind.starts_with("ceph")
        || kind.contains("gluster")
        || kind == "afs"
        || kind == "lustre"
        || kind.contains("davfs")
        || kind == "sshfs"
        || kind == "coda"
        || kind.starts_with("ocfs")
}

/// `Some` when `path` could be stated in [`CWD_PROBE`], else `None`.
fn timed_metadata(path: &Path) -> Option<()> {
    timed_is_file(path).map(|_| ())
}

/// What the stat thread has reported.
enum Note {
    /// The thread is running and is about to state the path.
    Started,
    /// The stat returned.
    Stated(bool),
}

/// `Some` when `path` could be stated in [`CWD_PROBE`], or when the stat
/// thread had not started.
///
/// The stat runs on another thread so a mount that never answers does not
/// keep this thread. `None` is that case: the worker reached the stat and
/// the stat did not return. A worker that has not started has not stated
/// the path. That delay is a late schedule, not a mount that refused to
/// answer, and the caller stays in the directory.
fn timed_is_file(path: &Path) -> Option<bool> {
    let path = path.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(Note::Started);
        let _ = tx.send(Note::Stated(path.is_file()));
    });
    let started = match rx.recv_timeout(CWD_PROBE) {
        Ok(Note::Started) => true,
        Ok(Note::Stated(found)) => return Some(found),
        Err(_) => false,
    };
    let stated = if started {
        match rx.recv_timeout(CWD_PROBE) {
            Ok(Note::Stated(found)) => Some(found),
            _ => None,
        }
    } else {
        None
    };
    probe_answer(started, stated)
}

/// `None` only when the worker reached the stat and the stat did not
/// return. A worker that has not started has not seen a hung mount.
fn probe_answer(started: bool, stated: Option<bool>) -> Option<bool> {
    if !started {
        return Some(false);
    }
    stated
}

/// `git args` in `dir` (`None` is the working directory), or `None` when
/// git does not answer inside [`CWD_PROBE`].
#[must_use]
pub(crate) fn git_line(dir: Option<&Path>, args: &[&str]) -> Option<String> {
    use std::io::Read as _;
    use std::process::Stdio;
    let mut cmd = crate::git_cmd();
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_COUNT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= CWD_PROBE => {
                stop_group(&mut child);
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                stop_group(&mut child);
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut buf = String::new();
    let _ = out.read_to_string(&mut buf);
    // An empty answer is still an answer. `git cat-file -e` succeeds and
    // prints nothing, and that success is what the caller asks about.
    Some(buf.trim().to_string())
}

fn stop_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        // SAFETY: `pid` is the child's process group, started above with
        // `process_group(0)`. The signal does not go to this process.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
    let _ = child.kill();
    let until = std::time::Instant::now() + Duration::from_millis(200);
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if std::time::Instant::now() >= until => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    fn settle_for(here: &Path) {
        settle_at(here, &[], false);
    }

    static CWD_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn a_project_flag_or_a_ticket_id_names_the_project() {
        let _env_read = crate::test_env_read_lock();
        let args = |words: &[&str]| {
            words
                .iter()
                .map(|word| (*word).to_string())
                .collect::<Vec<_>>()
        };
        assert!(project_named(&args(&[
            "file", "-p", "ridley", "--top", "title"
        ])));
        assert!(project_named(&args(&[
            "file",
            "--project",
            "ridley",
            "title"
        ])));
        assert!(project_named(&args(&["file", "--project=ridley", "title"])));
        assert!(project_named(&args(&["sitting", "ridley-ab12"])));
        assert!(project_named(&args(&[
            "sitting",
            "--anyway",
            "ridley-ab12"
        ])));
        assert!(project_named(&args(&["note", "demo-n", "hello"])));
        assert!(!project_named(&args(&["file", "title"])));
        assert!(!project_named(&args(&["sitting"])));
        assert!(!project_named(&args(&["note", "hello"])));
        assert!(!project_named(&args(&["file", "-p"])));
    }

    #[test]
    fn a_git_hook_is_not_a_command_that_probes_its_directory() {
        let _env_read = crate::test_env_read_lock();
        let args = |words: &[&str]| {
            words
                .iter()
                .map(|word| (*word).to_string())
                .collect::<Vec<_>>()
        };
        assert!(command_is_git_hook(&args(&["git-hook", "pre-commit"])));
        assert!(command_is_git_hook(&args(&[
            "--seat",
            "inky",
            "git-hook",
            "pre-commit"
        ])));
        assert!(!command_is_git_hook(&args(&["file", "-p", "demo"])));
        assert!(!command_is_git_hook(&args(&["hook"])));
        assert!(!command_is_git_hook(&args(&["--", "git-hook"])));
    }

    #[test]
    fn a_stat_that_has_not_started_is_not_a_hung_mount() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(probe_answer(false, None), Some(false));
        assert_eq!(probe_answer(true, Some(true)), Some(true));
        assert_eq!(probe_answer(true, Some(false)), Some(false));
        assert_eq!(probe_answer(true, None), None);
    }

    #[test]
    fn a_fuse_or_network_mount_is_slow_and_the_root_disk_is_not() {
        let _env_read = crate::test_env_read_lock();
        let info = "\
36 35 98:0 / / rw,relatime shared:1 - overlay overlay rw
80 36 0:55 / /assets rw,nosuid shared:2 - fuse.portal portal rw
90 36 0:56 / /mnt/nfs rw shared:3 - nfs4 host:/export rw
";
        assert!(!slow_mount(info, "/workspace/ljos"));
        assert!(slow_mount(info, "/assets/seat/files"));
        assert!(slow_mount(info, "/mnt/nfs/tracker"));
        assert!(!slow_mount(info, "/tmp"));
    }

    #[test]
    fn a_gone_working_directory_is_ignored() {
        let _env_read = crate::test_env_read_lock();
        let _guard = CWD_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("gone");
        std::fs::create_dir_all(&gone).unwrap();
        std::fs::remove_dir_all(&gone).unwrap();
        let started = std::time::Instant::now();
        settle_for(&gone);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "settle waited on a gone working directory"
        );
    }

    #[test]
    fn an_unreadable_working_directory_is_ignored() {
        let _env_read = crate::test_env_read_lock();
        let _guard = CWD_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = tempfile::tempdir().unwrap();
        let unreadable = root.path().join("unreadable");
        std::fs::create_dir_all(&unreadable).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if unsafe { libc::geteuid() } == 0 {
                return;
            }
            struct Restore(std::path::PathBuf, u32);
            impl Drop for Restore {
                fn drop(&mut self) {
                    let _ = crate::change_mode(&self.0, self.1);
                }
            }
            let original = std::fs::metadata(&unreadable).unwrap().permissions().mode();
            let _restore = Restore(unreadable.clone(), original);
            #[expect(
                clippy::disallowed_methods,
                reason = "sets the mode of a path that is not a git hook"
            )]
            std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
            let started = std::time::Instant::now();
            settle_for(&unreadable);
            assert!(
                started.elapsed() < std::time::Duration::from_secs(3),
                "settle waited on an unreadable working directory"
            );
        }
        #[cfg(not(unix))]
        {
            let started = std::time::Instant::now();
            settle_for(&unreadable);
            assert!(
                started.elapsed() < std::time::Duration::from_secs(3),
                "settle waited on an unreadable working directory"
            );
        }
    }
}
