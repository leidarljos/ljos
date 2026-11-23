//! Publish executable and secret files through one held directory descriptor.
//!
//! Threat model: ljos defends against path tricks: a symlink or another entry
//! left at a path, a predictable temporary name, a crash midway, and a stale
//! stage. It refuses to follow links. It does not defend against a concurrent
//! hostile process running as the same user. Such a process can already
//! rewrite the binary, hooks, and `~/.config` directly. Descriptor-relative
//! operations, `O_EXCL`, `O_NOFOLLOW`, and file and directory `fsync` calls
//! protect the in-scope cases.
//!
//! A directory the user named (`core.hooksPath`, an upgrade `--dir`, or a
//! config directory) is trusted when it is a symlink. ljos opens that
//! directory once, follows the symlink, and holds the directory by
//! descriptor. `O_NOFOLLOW` stays on the names ljos creates or replaces.
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

#![allow(
    clippy::disallowed_types,
    reason = "this module is the sole owner of filesystem publication modes"
)]

#[cfg(unix)]
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::Error;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
/// The name of the file inside a stage directory.
#[cfg(unix)]
const EXE: &CStr = c"exe";

/// Programs that copy, move or link the paths they are given, or change
/// their modes.
const WRITING_PROGRAMS: &[&str] = &["cp", "mv", "install", "ln", "chmod"];

/// Write `body` to `path`, a file that is not a git hook, without
/// following a symlink at `path`.
///
/// Every crate denies `clippy::disallowed_methods`, which refuses
/// `std::fs::write` and the other ways to put a file at a path. This is
/// the writer it points to.
///
/// # Errors
///
/// The file name of `path` is a git hook name, `path` is a symlink, or the
/// write fails.
pub fn write_file(path: impl AsRef<Path>, body: impl AsRef<[u8]>) -> Result<(), Error> {
    use std::io::Write;
    create_data_file(path.as_ref())?.write_all(body.as_ref())
}

/// Copy the bytes of `src` to `dest`, a file that is not a git hook,
/// without following a symlink at `dest`. Unlike `std::fs::copy`, the copy
/// does not take the mode of `src`, so it never gains an execute bit.
///
/// # Errors
///
/// As [`write_file`], or `src` cannot be read.
pub fn copy_file(src: impl AsRef<Path>, dest: impl AsRef<Path>) -> Result<u64, Error> {
    let mut from = File::open(src.as_ref())?;
    let mut to = create_data_file(dest.as_ref())?;
    std::io::copy(&mut from, &mut to)
}

/// Open `path` for writing, created or truncated, refusing a git hook name
/// and a symlink at `path`.
fn create_data_file(path: &Path) -> Result<File, Error> {
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
    #[expect(
        clippy::disallowed_methods,
        reason = "the writer the lint points to; a hook name is refused above and O_NOFOLLOW refuses a symlink"
    )]
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "the writer the lint points to; a hook name is refused above"
    )]
    #[cfg(not(unix))]
    {
        File::create(path)
    }
}

/// A [`std::process::Command`] for `program`, which must not be a program
/// that copies, moves or links files or changes their modes.
///
/// Every crate denies `clippy::disallowed_methods`, which refuses
/// `Command::new`. This is the constructor it points to.
///
/// # Errors
///
/// The file name of `program` is `cp`, `mv`, `install`, `ln` or `chmod`.
pub fn program(program: impl AsRef<std::ffi::OsStr>) -> Result<std::process::Command, Error> {
    let program = program.as_ref();
    let name = Path::new(program).file_name().unwrap_or(program);
    if WRITING_PROGRAMS.iter().any(|writer| name == *writer) {
        return Err(Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "program: {} writes the paths it is given or changes their modes; publish a hook with install_executable",
                program.to_string_lossy()
            ),
        ));
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "the constructor the lint points to; cp, mv, install, ln and chmod are refused above"
    )]
    return Ok(std::process::Command::new(program));
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
/// [`Mode::OfSource`] for bytes, the directory cannot be written, or the
/// short-lived writer fails. Then nothing at `dest` changed. When the
/// directory sync after the rename fails, the error says the file is in
/// place.
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
        stage.settle(dest, mode)?.durable
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

/// Create a new executable copy at `dest`, refusing any existing entry.
///
/// This is for rollback files, whose existing contents must never be run or
/// replaced. An unpredictable temporary name is opened with
/// `O_CREAT|O_EXCL|O_NOFOLLOW`, flushed, and renamed to the final name without
/// replacing an entry there.
///
/// # Errors
///
/// The source is not regular, the destination exists, or a write, mode change,
/// rename, or sync fails. When the directory sync after the rename fails,
/// the error says the copy is in place.
#[cfg(unix)]
pub fn copy_executable_new(src: &Path, dest: &Path) -> Result<(), Error> {
    use std::io::Write;
    use std::os::fd::OwnedFd;

    let mut source = File::open(src).map_err(|err| named(err, "read", src))?;
    let source_stat = stat(&source).map_err(|err| named(err, "read", src))?;
    if source_stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(Error::other(format!(
            "install executable: {} is not a regular file",
            src.display()
        )));
    }
    if let Some(parent) = dir_of(dest) {
        std::fs::create_dir_all(parent)?;
    }
    let parent_path = dir_of(dest).unwrap_or_else(|| Path::new("."));
    let parent = open_dir(parent_path).map_err(|err| named(err, "open", parent_path))?;
    let final_leaf = leaf_of(dest, "create")?;
    let (leaf, fd): (CString, OwnedFd) = loop {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random)
            .map_err(|err| Error::other(format!("get random rollback name: {err}")))?;
        let leaf = CString::new(format!(".ljos-rollback.{}", hex::encode(random)))
            .map_err(Error::other)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "creates a rollback file with O_EXCL and O_NOFOLLOW in the destination directory"
        )]
        match rustix::fs::openat(
            &parent,
            leaf.as_c_str(),
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0o600),
        ) {
            Ok(fd) => break (leaf, fd),
            Err(err) if err == rustix::io::Errno::EXIST => continue,
            Err(err) => {
                return Err(named(
                    Error::from_raw_os_error(err.raw_os_error()),
                    "create a temporary rollback for",
                    dest,
                ))
            }
        }
    };
    let mut fresh = FreshEntry {
        parent,
        parent_path: parent_path.to_path_buf(),
        leaf,
        file: File::from(fd),
        committed: false,
    };
    std::io::copy(&mut source, &mut fresh.file).map_err(|err| named(err, "copy", src))?;
    let mode = (source_stat.st_mode as u32) & 0o777;
    #[expect(
        clippy::disallowed_methods,
        reason = "sets the fresh rollback descriptor mode before its directory sync"
    )]
    rustix::fs::fchmod(&fresh.file, rustix::fs::Mode::from_raw_mode(mode)).map_err(|err| {
        named(
            Error::from_raw_os_error(err.raw_os_error()),
            "mark executable",
            dest,
        )
    })?;
    fresh
        .file
        .flush()
        .map_err(|err| named(err, "write", dest))?;
    flush(&fresh.file, dest).map_err(|err| named(err, "flush", dest))?;
    fresh.commit(&final_leaf, dest)?;
    fresh.committed = true;
    flush(&fresh.parent, &fresh.parent_path)
        .map_err(|err| not_durable(err, dest, &fresh.parent_path))
}

/// A rename that took effect.
///
/// `durable` is the sync of the directory after it. An error there does
/// not undo the rename: the new name is in place, but a crash may bring
/// back what was there before.
#[cfg(unix)]
#[must_use = "the rename took effect even when `durable` is an error"]
#[derive(Debug)]
pub struct Moved {
    pub durable: Result<(), Error>,
}

/// Make `dest` a symlink to `target` with one rename, replacing whatever
/// entry is at `dest` and never following it. A crash leaves the old entry
/// or the new link.
///
/// # Errors
///
/// `dest` has no file name, or the link cannot be made or renamed onto
/// `dest`. Then nothing at `dest` changed. A failed sync after the rename
/// is [`Moved::durable`].
#[cfg(unix)]
pub fn publish_link(dest: &Path, target: &Path) -> Result<Moved, Error> {
    let leaf = leaf_of(dest, "install executable: link")?;
    if let Some(parent) = dir_of(dest) {
        std::fs::create_dir_all(parent)?;
    }
    let stage = Stage::new(dest)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "makes the new link inside the stage directory this process made"
    )]
    rustix::fs::symlinkat(target, stage.dir(), EXE).map_err(|err| {
        named(
            Error::from_raw_os_error(err.raw_os_error()),
            "link",
            &stage.file(),
        )
    })?;
    stage.rename_onto(&leaf, dest)
}

#[cfg(not(unix))]
pub fn copy_executable_new(src: &Path, dest: &Path) -> Result<(), Error> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .and_then(|mut to| {
            let mut from = File::open(src)?;
            std::io::copy(&mut from, &mut to).map(|_| ())
        })
}

#[cfg(unix)]
struct FreshEntry {
    parent: File,
    parent_path: PathBuf,
    leaf: CString,
    file: File,
    committed: bool,
}

#[cfg(unix)]
impl FreshEntry {
    fn commit(&self, final_leaf: &CStr, path: &Path) -> Result<(), Error> {
        rename_noreplace(&self.parent, &self.leaf, final_leaf)
            .map_err(|err| named(Error::from_raw_os_error(err.raw_os_error()), "create", path))
    }
}

#[cfg(unix)]
impl Drop for FreshEntry {
    fn drop(&mut self) {
        if !self.committed {
            let _ = rustix::fs::unlinkat(
                &self.parent,
                self.leaf.as_c_str(),
                rustix::fs::AtFlags::empty(),
            );
        }
    }
}

#[cfg(unix)]
fn rename_noreplace(parent: &File, old: &CStr, new: &CStr) -> rustix::io::Result<()> {
    #[cfg(any(
        target_os = "android",
        target_os = "ios",
        target_os = "linux",
        target_os = "macos",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos"
    ))]
    {
        #[expect(
            clippy::disallowed_methods,
            reason = "publishes a flushed rollback without replacing its final name"
        )]
        match rustix::fs::renameat_with(
            parent,
            old,
            parent,
            new,
            rustix::fs::RenameFlags::NOREPLACE,
        ) {
            Ok(()) => return Ok(()),
            Err(rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL) => {}
            Err(err) => return Err(err),
        }
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "fallback publishes a flushed rollback without replacing its final name"
    )]
    rustix::fs::linkat(parent, old, parent, new, rustix::fs::AtFlags::empty())?;
    rustix::fs::unlinkat(parent, old, rustix::fs::AtFlags::empty())
}

/// One regular file read through a descriptor relative to [`HeldDirectory`].
#[cfg(unix)]
pub(crate) struct DirectoryEntry {
    pub(crate) bytes: Vec<u8>,
    pub(crate) mode: u32,
}

/// A destination directory opened once and held by descriptor.
///
/// A symlink at the directory itself is followed. That directory is one the
/// user configured. Hook installation then binds each read, replacement,
/// removal, and directory sync to the descriptor, and does not follow a
/// symlink at a name inside it.
///
/// Opening takes an exclusive `flock` on the directory, held until the
/// value is dropped. Every ljos path that edits a hook opens the hooks
/// directory this way, so two of them take turns: each reads the hooks
/// after the other's writes and its own writes lose nothing. A program
/// that does not take the lock is not ordered with ljos.
#[cfg(unix)]
pub(crate) struct HeldDirectory {
    file: File,
    path: PathBuf,
}

#[cfg(unix)]
impl HeldDirectory {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let file = open_dir(path).map_err(|err| named(err, "open", path))?;
        loop {
            match rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive) {
                Ok(()) => break,
                Err(rustix::io::Errno::INTR) => continue,
                Err(err) => {
                    return Err(named(
                        Error::from_raw_os_error(err.raw_os_error()),
                        "lock",
                        path,
                    ))
                }
            }
        }
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn read(&self, name: &str) -> Result<Option<DirectoryEntry>, Error> {
        use std::io::Read;
        use std::os::unix::ffi::OsStrExt;
        let name = CString::new(name).map_err(Error::other)?;
        let mut file = match open_at(
            &self.file,
            &name,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        ) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) if err.raw_os_error() == Some(libc::ELOOP) => {
                let link = self.path.join(name.to_string_lossy().as_ref());
                let target =
                    rustix::fs::readlinkat(&self.file, &name, Vec::new()).map_err(|read| {
                        named(
                            Error::from_raw_os_error(read.raw_os_error()),
                            "read link",
                            &link,
                        )
                    })?;
                let target = Path::new(std::ffi::OsStr::from_bytes(target.to_bytes()));
                return Err(Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "install executable: {} is a symbolic link to {}; replace the link with a regular file",
                        link.display(),
                        target.display()
                    ),
                ));
            }
            Err(err) => {
                return Err(named(
                    err,
                    "open",
                    &self.path.join(name.to_string_lossy().as_ref()),
                ))
            }
        };
        let st = stat(&file).map_err(|err| named(err, "read", &self.path))?;
        if st.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "install executable: {} is not a regular file",
                    self.path.join(name.to_string_lossy().as_ref()).display()
                ),
            ));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(DirectoryEntry {
            bytes,
            mode: st.st_mode & 0o777,
        }))
    }

    pub(crate) fn publish(&self, name: &str, body: &[u8], mode: u32) -> Result<(), Error> {
        let path = self.path.join(name);
        let stage = Stage::in_dir(&self.file, &self.path)?;
        stage.fill(Fill::Bytes(body))?;
        #[cfg(test)]
        before_entry_mutation(&path);
        stage.settle(&path, mode & 0o777)?.durable
    }

    pub(crate) fn remove(&self, name: &str) -> Result<(), Error> {
        let name = CString::new(name).map_err(Error::other)?;
        #[cfg(test)]
        before_entry_mutation(&self.path.join(name.to_string_lossy().as_ref()));
        rustix::fs::unlinkat(&self.file, name.as_c_str(), rustix::fs::AtFlags::empty())
            .map_err(|err| Error::from_raw_os_error(err.raw_os_error()))?;
        flush(&self.file, &self.path).map_err(|err| named(err, "flush", &self.path))
    }

    /// `true` when `name` is a symlink. A missing name is `false`.
    pub(crate) fn is_symlink(&self, name: &str) -> Result<bool, Error> {
        let name_c = CString::new(name).map_err(Error::other)?;
        match stat_at(&self.file, &name_c) {
            Ok(st) => Ok(st.st_mode & libc::S_IFMT == libc::S_IFLNK),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(named(err, "read", &self.path.join(name))),
        }
    }

    /// The text of `name`, following a symlink at that name.
    ///
    /// An entry that is missing, not a regular file, or not UTF-8 is empty.
    /// Uninstall uses that to leave a linked hook whose target has no ljos
    /// block.
    pub(crate) fn followed_text(&self, name: &str) -> Result<String, Error> {
        use std::io::Read;
        let name_c = CString::new(name).map_err(Error::other)?;
        let path = self.path.join(name);
        let mut file = match open_at(&self.file, &name_c, libc::O_RDONLY | libc::O_NONBLOCK) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
            Err(err) => return Err(named(err, "open", &path)),
        };
        let st = stat(&file).map_err(|err| named(err, "read", &path))?;
        if st.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Ok(String::new());
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|err| named(err, "read", &path))?;
        Ok(String::from_utf8(bytes).unwrap_or_default())
    }
}

#[cfg(all(test, unix))]
type EntryHook = Box<dyn FnMut(&Path)>;

#[cfg(all(test, unix))]
thread_local! {
    static BEFORE_ENTRY_MUTATION: std::cell::RefCell<Option<EntryHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
fn before_entry_mutation(path: &Path) {
    BEFORE_ENTRY_MUTATION.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(path);
        }
    });
}

#[cfg(all(test, unix))]
pub(crate) fn set_entry_hook(hook: Option<EntryHook>) {
    BEFORE_ENTRY_MUTATION.with(|slot| *slot.borrow_mut() = hook);
}

/// Set the permission bits of the file or directory at `path` to `mode`,
/// masked to `0o7777`, without following a symlink at `path`.
///
/// This and [`change_mode_of`] are the only mode changes outside
/// [`publish`]. A directory takes any mode. Anything else may drop bits
/// but never gains an execute bit, nor setuid or setgid while it can be
/// executed: a file that is to be executed is published.
///
/// # Errors
///
/// `path` has no file name or is a symlink, the change would make a file
/// executable, or the change fails.
pub fn change_mode(path: impl AsRef<Path>, mode: u32) -> Result<(), Error> {
    let path = path.as_ref();
    #[cfg(unix)]
    {
        let leaf = leaf_of(path, "change the mode of")?;
        let parent_path = dir_of(path).unwrap_or_else(|| Path::new("."));
        let parent = open_dir(parent_path).map_err(|err| moded(err, "open", parent_path))?;
        let st = stat_at(&parent, &leaf).map_err(|err| moded(err, "read", path))?;
        if st.st_mode & libc::S_IFMT == libc::S_IFLNK {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "change mode: {} is a symlink, and a mode is never changed through one",
                    path.display()
                ),
            ));
        }
        match open_at(
            &parent,
            &leaf,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        ) {
            Ok(file) => fchmod_checked(&file, mode, path),
            Err(err) => Err(moded(err, "open", path)),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// [`change_mode`] on an open descriptor, with the same refusal.
///
/// # Errors
///
/// The change would make a file executable, or it fails.
pub fn change_mode_of(file: &std::fs::File, mode: u32) -> Result<(), Error> {
    #[cfg(unix)]
    {
        fchmod_checked(file, mode, Path::new("an open file"))
    }
    #[cfg(not(unix))]
    {
        let _ = (file, mode);
        Ok(())
    }
}

/// Give the file at `path` exactly `mode`, setuid and setgid included,
/// for a test fixture [`change_mode`] refuses to make.
#[cfg(all(test, unix))]
pub(crate) fn fixture_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    #[expect(
        clippy::disallowed_methods,
        reason = "a test fixture's mode, which may carry setuid with an execute bit"
    )]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
}

#[cfg(unix)]
fn fchmod_checked(file: &File, mode: u32, path: &Path) -> Result<(), Error> {
    let st = stat(file).map_err(|err| moded(err, "read", path))?;
    refuse_exec(&st, mode, path)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "the descriptor mode change change_mode and change_mode_of make after refuse_exec"
    )]
    rustix::fs::fchmod(file, rustix::fs::Mode::from_raw_mode(mode & 0o7777)).map_err(|err| {
        moded(
            Error::from_raw_os_error(err.raw_os_error()),
            "change the mode of",
            path,
        )
    })
}

/// Refuse a `mode` that gives `st`, if it is not a directory, an execute
/// bit it lacks, or setuid or setgid with any execute bit.
#[cfg(unix)]
fn refuse_exec(st: &libc::stat, mode: u32, path: &Path) -> Result<(), Error> {
    if st.st_mode & libc::S_IFMT == libc::S_IFDIR {
        return Ok(());
    }
    #[allow(
        clippy::useless_conversion,
        reason = "st_mode is a u16 on macOS and a u32 on Linux"
    )]
    let now = u32::from(st.st_mode) & 0o7777;
    let mode = mode & 0o7777;
    let added = mode & !now;
    if added & 0o111 != 0 || (mode & 0o111 != 0 && added & 0o6000 != 0) {
        return Err(Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "change mode: {} is not a directory, and {mode:o} would let it be executed with a bit it lacks ({now:o}); publish it with ljos_cli::publish",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn leaf_of(path: &Path, what: &str) -> Result<CString, Error> {
    path.file_name()
        .and_then(|name| CString::new(name.as_encoded_bytes()).ok())
        .ok_or_else(|| {
            Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{what} {}: no file name", path.display()),
            )
        })
}

/// Open `path` as a directory and follow a symlink at `path`.
///
/// The path is a directory the user configured. The descriptor is the
/// directory itself, so later names are opened relative to it.
#[cfg(unix)]
fn open_dir(path: &Path) -> Result<File, Error> {
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a directory the user configured and follows a symlink at it"
    )]
    let opened = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map(File::from)
    .map_err(|err| Error::from_raw_os_error(err.raw_os_error()))?;
    Ok(opened)
}

#[cfg(unix)]
fn moded(err: Error, what: &str, path: &Path) -> Error {
    Error::new(
        err.kind(),
        format!("change mode: {what} {}: {err}", path.display()),
    )
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
    dir: Option<File>,
    name: CString,
    path: PathBuf,
}

#[cfg(unix)]
impl Stage {
    fn new(path: &Path) -> Result<Self, Error> {
        let parent_path = dir_of(path).unwrap_or_else(|| Path::new("."));
        let parent = open_dir(parent_path).map_err(|err| named(err, "open", parent_path))?;
        Self::in_dir(&parent, parent_path)
    }

    fn in_dir(parent: &File, parent_path: &Path) -> Result<Self, Error> {
        let parent = parent
            .try_clone()
            .map_err(|err| named(err, "duplicate", parent_path))?;
        let parent_path = parent_path.to_path_buf();
        let mut last = None;
        for _ in 0..16 {
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random)
                .map_err(|err| Error::other(format!("get random stage name: {err}")))?;
            let name = format!(".ljos-stage.{}", hex::encode(random));
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
            // Own cleanup immediately after creation. Every later error drops
            // this guard and removes the directory through `parent`.
            let mut stage = Self {
                parent,
                parent_path,
                dir: None,
                name,
                path,
            };
            let dir = open_at(
                &stage.parent,
                &stage.name,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
            .map_err(|err| named(err, "open", &stage.path))?;
            let held = stat(&dir).map_err(|err| named(err, "read", &stage.path))?;
            // SAFETY: geteuid has no preconditions.
            let me = unsafe { libc::geteuid() };
            if held.st_mode & libc::S_IFMT != libc::S_IFDIR
                || held.st_uid != me
                || held.st_mode & 0o077 != 0
            {
                return Err(Error::other(format!(
                    "install executable: {} was replaced before it was opened",
                    stage.path.display()
                )));
            }
            stage.dir = Some(dir);
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

    fn dir(&self) -> &File {
        self.dir.as_ref().expect("stage directory opened")
    }

    /// Create the staged file in a forked child and write `fill` to it.
    ///
    /// The child makes only async-signal-safe calls: this process may have
    /// other threads, and one of them may hold a lock the child would wait
    /// on forever. It exits with 0 or the errno of the call that failed.
    fn fill(&self, fill: Fill) -> Result<(), Error> {
        let stage = self.dir().as_raw_fd();
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

    /// Mark the staged file `mode`, flush it, and [`Stage::rename_onto`]
    /// `path`.
    fn settle(self, path: &Path, mode: u32) -> Result<Moved, Error> {
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
            self.dir(),
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
        #[expect(
            clippy::disallowed_methods,
            reason = "the mode of the staged file, through its descriptor, before the rename"
        )]
        rustix::fs::fchmod(&file, rustix::fs::Mode::from_raw_mode(mode)).map_err(|err| {
            named(
                Error::from_raw_os_error(err.raw_os_error()),
                "mark executable",
                &tmp,
            )
        })?;
        flush(&file, &tmp).map_err(|err| named(err, "flush", &tmp))?;
        drop(file);
        #[cfg(test)]
        at_step(Step::Moving, &self.path);
        self.rename_onto(&leaf, path)
    }

    /// Rename the staged entry onto `leaf`, the name of `path`, then flush
    /// the directory so the new name survives a crash.
    fn rename_onto(&self, leaf: &CStr, path: &Path) -> Result<Moved, Error> {
        #[expect(
            clippy::disallowed_methods,
            reason = "renames the staged entry from the held stage directory onto the destination name"
        )]
        rustix::fs::renameat(self.dir(), EXE, &self.parent, leaf).map_err(|err| {
            named(
                Error::from_raw_os_error(err.raw_os_error()),
                "rename onto",
                path,
            )
        })?;
        Ok(Moved {
            durable: flush(&self.parent, &self.parent_path)
                .map_err(|err| not_durable(err, path, &self.parent_path)),
        })
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
        if let Some(dir) = &self.dir {
            let _ = rustix::fs::unlinkat(dir, EXE, rustix::fs::AtFlags::empty());
        } else {
            // Opening the directory was the first fallible operation after
            // mkdirat. Removal by name cannot follow a replacement symlink.
            let _ = rustix::fs::unlinkat(
                &self.parent,
                self.name.as_c_str(),
                rustix::fs::AtFlags::REMOVEDIR,
            );
        }
        if let (Some(dir), Ok(named)) = (&self.dir, stat_at(&self.parent, &self.name)) {
            let Ok(held) = stat(dir) else {
                return;
            };
            if same_file(&held, &named) {
                let _ = rustix::fs::unlinkat(
                    &self.parent,
                    self.name.as_c_str(),
                    rustix::fs::AtFlags::REMOVEDIR,
                );
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
    #[expect(
        clippy::disallowed_methods,
        reason = "creates the staged file with O_EXCL and O_NOFOLLOW in the stage directory this process made"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a name relative to a held directory descriptor"
    )]
    let opened = rustix::fs::openat(
        dir,
        name,
        rustix::fs::OFlags::from_bits_retain((flags | libc::O_CLOEXEC) as u32),
        rustix::fs::Mode::empty(),
    )
    .map(File::from)
    .map_err(|err| Error::from_raw_os_error(err.raw_os_error()))?;
    Ok(opened)
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

/// The payload of an [`Error`] from a directory sync after a rename took
/// effect. [`renamed`] finds it.
#[derive(Debug)]
struct NotDurable(String);

impl std::fmt::Display for NotDurable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotDurable {}

/// `err` from syncing `dir` after a rename onto `path` took effect.
#[cfg(unix)]
fn not_durable(err: Error, path: &Path, dir: &Path) -> Error {
    Error::new(
        err.kind(),
        NotDurable(format!(
            "install executable: {} is in place, but syncing {} failed, so a crash may bring back what was there: {err}",
            path.display(),
            dir.display()
        )),
    )
}

/// `true` when `err` came after the rename took effect: the new name is in
/// place and only its directory sync failed.
pub(crate) fn renamed(err: &Error) -> bool {
    err.get_ref().is_some_and(|inner| inner.is::<NotDurable>())
}

#[cfg(test)]
thread_local! {
    static FLUSHED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
    /// When set, a flush of a staged file fails with EIO.
    static FAIL_STAGED_FLUSH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A flush of one of these directories fails with EIO.
    static FAIL_FLUSH_OF: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Make a flush of each of `dirs` on this thread fail with EIO, until the
/// next call.
#[cfg(test)]
pub(crate) fn fail_flush_of(dirs: Vec<PathBuf>) {
    FAIL_FLUSH_OF.with(|fail| *fail.borrow_mut() = dirs);
}

/// `fsync` on `file`, a file or directory opened at `path`. A directory
/// sync after a rename is reported in [`Moved::durable`], since it cannot
/// undo the rename.
#[cfg(unix)]
fn flush(file: &File, path: &Path) -> Result<(), Error> {
    #[cfg(test)]
    if FAIL_STAGED_FLUSH.get() && path.file_name() == Some(std::ffi::OsStr::new("exe")) {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    #[cfg(test)]
    if FAIL_FLUSH_OF.with(|fail| fail.borrow().iter().any(|dir| dir == path)) {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    rustix::fs::fsync(file).map_err(|err| Error::from_raw_os_error(err.raw_os_error()))?;
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
mod hook_write_forms;

#[cfg(test)]
mod mode_check;

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
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
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
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
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
        for (from, want) in [
            (0o700, 0o700),
            (0o750, 0o750),
            (0o644, 0o644),
            (0o4755, 0o755),
        ] {
            install_executable_mode(&src, OURS, 0o700).unwrap();
            fixture_mode(&src, from);
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
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
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
            assert!(name.starts_with(".ljos-stage."), "{step} flushed {done:?}");
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
                .filter(|name| name.to_string_lossy().starts_with(".ljos-stage."))
                .collect();
            assert!(stages.is_empty(), "{step}: left {stages:?}");
        }
    }

    /// A destination-directory sync error is returned after the rename,
    /// and says the file is in place.
    #[test]
    fn a_failed_directory_flush_fails_the_publish_without_a_stage() {
        struct Unset;
        impl Drop for Unset {
            fn drop(&mut self) {
                fail_flush_of(Vec::new());
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        let _unset = Unset;
        fail_flush_of(vec![dir.path().to_path_buf()]);
        let err = install_executable(&dest, OURS).unwrap_err();
        assert!(
            err.to_string().contains("is in place, but syncing"),
            "{err}"
        );
        assert!(renamed(&err), "{err}");
        assert_eq!(std::fs::read(&dest).unwrap(), OURS);
        let stages: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(".ljos-stage."))
            .collect();
        assert!(stages.is_empty(), "left {stages:?}");
    }

    /// Another user can leave symlinks at temporary names in a directory
    /// the publisher may create in but not unlink from. A publisher that
    /// writes or chmods a guessable name there rewrites the link's target.
    /// A directory this process cannot remove names from stands in for
    /// that one: every name the old `.ljos-exe.<pid>.<n>` scheme could pick
    /// links to `victim`.
    #[test]
    fn a_planted_temp_symlink_is_not_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&victim, "SECRET").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "sets the mode of a path that is not a git hook"
        )]
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
        let src = dir.path().join("src");
        install_executable(&src, b"#!/bin/sh\necho copied\n").unwrap();
        let shared = dir.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        let pid = std::process::id();
        for n in 0..4096 {
            #[expect(
                clippy::disallowed_methods,
                reason = "links a path that is not a git hook"
            )]
            std::os::unix::fs::symlink(&victim, shared.join(format!(".ljos-exe.{pid}.{n}")))
                .unwrap();
        }
        let lock = |mode| {
            #[expect(
                clippy::disallowed_methods,
                reason = "sets the mode of a path that is not a git hook"
            )]
            std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        lock(0o555);
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
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
                #[expect(
                    clippy::disallowed_methods,
                    reason = "renames a path that is not a git hook"
                )]
                std::fs::rename(stage, &aside).unwrap();
                #[expect(
                    clippy::disallowed_methods,
                    reason = "links a path that is not a git hook"
                )]
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
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&victim, "SECRET").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "sets the mode of a path that is not a git hook"
        )]
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(mode)).unwrap();
        let attack = dir.path().join("attack");
        std::fs::create_dir(&attack).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
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
            #[expect(
                clippy::disallowed_methods,
                reason = "writes a file that is not a git hook"
            )]
            std::fs::write(dir.path().join("evil"), "#!/bin/sh\necho PWNED\n").unwrap();
            let dest = dir.path().join("stub");
            let (got, asides) = swapped_at(Step::Moving, &attack, || publish(step, &src, &dest));
            got.unwrap_or_else(|err| panic!("{step}: {err}"));
            assert_eq!(asides.len(), 1, "{step}");
            assert_published(step, &dest);
            #[expect(
                clippy::disallowed_methods,
                reason = "starts a program outside git hook code"
            )]
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

    /// A failed publish removes the staged file through the held directory
    /// and leaves a name someone else put at the stage's name, and what it
    /// links to.
    #[test]
    fn a_failed_publish_cleans_only_what_it_made() {
        for step in ["install", "copy"] {
            let (dir, victim, attack, src) = arena(0o644, "victim");
            let dest = dir.path().join("stub");
            std::fs::create_dir(&dest).unwrap();
            #[expect(
                clippy::disallowed_methods,
                reason = "writes a file that is not a git hook"
            )]
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
                .find(|path| path.to_string_lossy().contains(".ljos-stage."))
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
                .filter(|name| name.to_string_lossy().starts_with(".ljos-stage."))
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
            #[expect(
                clippy::disallowed_methods,
                reason = "starts a program outside git hook code"
            )]
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
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&victim, "SECRET").unwrap();
        let link = dir.path().join("link");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(write_file(&link, "x").is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"SECRET");
    }

    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    }

    #[test]
    fn a_mode_change_never_makes_a_file_executable() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        write_file(&data, "x").unwrap();
        change_mode(&data, 0o600).unwrap();
        for mode in [0o700, 0o610, 0o601, 0o4700] {
            let err = change_mode(&data, mode).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{mode:o}");
            let err = change_mode_of(&File::open(&data).unwrap(), mode).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{mode:o}");
        }
        assert_eq!(mode_of(&data), 0o600);
        change_mode(&data, 0o2600).unwrap();
        assert_eq!(mode_of(&data), 0o2600);

        let exe = dir.path().join("exe");
        install_executable_mode(&exe, OURS, 0o755).unwrap();
        change_mode(&exe, 0o757).unwrap();
        change_mode_of(&File::open(&exe).unwrap(), 0o700).unwrap();
        assert_eq!(mode_of(&exe), 0o700);
        let err = change_mode(&exe, 0o4700).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        for mode in [0o755, 0o1777, 0o000] {
            change_mode(&sub, mode).unwrap();
            assert_eq!(mode_of(&sub), mode);
        }
    }

    #[test]
    fn a_mode_change_follows_no_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        write_file(&victim, "keep").unwrap();
        change_mode(&victim, 0o600).unwrap();
        let link = dir.path().join("link");
        #[expect(
            clippy::disallowed_methods,
            reason = "plants a link that is not a git hook"
        )]
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        let err = change_mode(&link, 0o644).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{err}");
        assert_eq!(mode_of(&victim), 0o600);
    }

    #[test]
    fn a_copied_file_takes_no_execute_bit_and_follows_no_link() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        install_executable_mode(&src, OURS, 0o755).unwrap();
        let dest = dir.path().join("dest");
        assert_eq!(copy_file(&src, &dest).unwrap(), OURS.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), OURS);
        assert_eq!(mode_of(&dest) & 0o111, 0, "{:o}", mode_of(&dest));
        let victim = dir.path().join("victim");
        write_file(&victim, "keep").unwrap();
        let link = dir.path().join("link");
        #[expect(
            clippy::disallowed_methods,
            reason = "plants a link that is not a git hook"
        )]
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(copy_file(&src, &link).is_err());
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        assert!(copy_file(&src, dir.path().join("pre-push")).is_err());
    }

    #[test]
    fn program_refuses_a_program_that_writes_paths() {
        for name in ["cp", "/bin/mv", "install", "/usr/bin/ln", "/bin/chmod"] {
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

    /// Clippy refuses the calls in `clippy.toml` only where
    /// `clippy::disallowed_methods` is denied, so the workspace denies it
    /// and every member takes the workspace lints.
    #[test]
    fn every_crate_denies_the_hook_write_methods() {
        let read = |path: &Path| -> toml::Table {
            std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()))
                .parse()
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()))
        };
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let out = program(cargo)
            .unwrap()
            .args([
                "metadata",
                "--no-deps",
                "--offline",
                "--format-version",
                "1",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let root = Path::new(meta["workspace_root"].as_str().unwrap());
        let workspace = read(&root.join("Cargo.toml"));
        let clippy = read(&root.join("clippy.toml"));
        let denied: std::collections::BTreeSet<_> = clippy["disallowed-methods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect();
        for path in [
            "std::fs::hard_link",
            "std::os::unix::fs::symlink",
            "rustix::fs::symlink",
            "rustix::fs::symlinkat",
            "rustix::fs::rename",
            "rustix::fs::renameat_with",
            "rustix::fs::link",
            "rustix::fs::linkat",
        ] {
            assert!(denied.contains(path), "{path} is not denied in clippy.toml");
        }
        for lint in ["disallowed_methods", "disallowed_types"] {
            let level = &workspace["workspace"]["lints"]["clippy"][lint];
            let level = level
                .as_str()
                .or_else(|| level.get("level").and_then(toml::Value::as_str));
            assert!(
                matches!(level, Some("deny" | "forbid")),
                "[workspace.lints.clippy] {lint} is {level:?}"
            );
        }
        let members = meta["workspace_members"].as_array().unwrap();
        let packages = meta["packages"].as_array().unwrap();
        assert_eq!(members.len(), packages.len());
        assert!(packages.len() >= 4, "found {} members", packages.len());
        for package in packages {
            let manifest = Path::new(package["manifest_path"].as_str().unwrap());
            let lints = read(manifest).get("lints").cloned();
            assert_eq!(
                lints,
                Some(toml::Value::Table(toml::Table::from_iter([(
                    "workspace".to_string(),
                    toml::Value::Boolean(true),
                )]))),
                "{} must have [lints] workspace = true and nothing else",
                manifest.display()
            );
        }
    }
}
