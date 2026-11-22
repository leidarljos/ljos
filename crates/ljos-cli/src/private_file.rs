//! The one way a kept file is read or written under the seat's data home.
//!
//! A kept listing is the seat's memory, so a file is read only when it is
//! what this user wrote: not a link, not a pipe, not a device, not shared
//! with another name, and not in a directory another user can change. Each
//! check is made on the descriptor that is then read.
//!
//! The directories that lead to that directory are checked too, see
//! [`trusted_ancestors`], because another user who can rename one of them
//! can put a directory of their own in its place.

use std::fs::{File, Metadata, OpenOptions};
use std::io::Read as _;
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Component, Path, PathBuf};

/// Mode bits that let a group or another user change a file or directory.
const OTHERS_WRITE: u32 = 0o022;
/// The one bit a file shared through a tracker may not carry.
const WORLD_WRITE: u32 = 0o002;
/// The sticky bit: in such a directory only an entry's owner can rename it.
const STICKY: u32 = 0o1000;
/// Links followed on the way to a kept directory before the path is refused.
const MAX_LINKS: usize = 40;

fn me() -> u32 {
    // SAFETY: geteuid takes no arguments and cannot fail.
    unsafe { libc::geteuid() }
}

/// Whether an entry on the way to a kept directory, with owner `uid` and
/// `mode`, can be changed by nobody but `me` and root. A link must be owned
/// by `me` or root. A directory must be owned by `me` or root, and be
/// writable by no group or other user unless it is sticky, as `/tmp` is.
fn trusted_entry(uid: u32, mode: u32, link: bool, me: u32) -> bool {
    (uid == me || uid == 0) && (link || mode & OTHERS_WRITE == 0 || mode & STICKY != 0)
}

/// Whether every directory and link on the way to `dir`, `dir` included,
/// passes [`trusted_entry`].
///
/// The path is walked from `/` one name at a time without following a link
/// blind: a link is checked and then its target is walked the same way, so
/// a `$XDG_DATA_HOME` that is a link is used only when the link and every
/// directory it leads through pass. The directories above `$HOME` are the
/// system's and are not checked; a path outside `$HOME` is checked from
/// `/`. An entry that passes cannot be renamed by another user, so the
/// path still names the same directory when it is opened.
fn trusted_ancestors(dir: &Path) -> bool {
    let me = me();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .and_then(|home| std::fs::canonicalize(home).ok());
    let checked = |at: &Path| {
        home.as_ref()
            .is_none_or(|home| !home.starts_with(at) || home == at)
    };
    let trusted = |at: &Path, meta: &Metadata, link: bool| {
        !checked(at) || trusted_entry(meta.uid(), meta.mode(), link, me)
    };
    if !dir.is_absolute() {
        return false;
    }
    let mut at = PathBuf::from("/");
    match std::fs::symlink_metadata(&at) {
        Ok(meta) if trusted(&at, &meta, false) => {}
        _ => return false,
    }
    let mut pending: Vec<std::ffi::OsString> = Vec::new();
    let push = |pending: &mut Vec<std::ffi::OsString>, path: &Path| {
        for part in path.components().rev() {
            if let Component::Normal(_) | Component::ParentDir = part {
                pending.push(part.as_os_str().to_owned());
            }
        }
    };
    push(&mut pending, dir);
    let mut links = 0;
    while let Some(part) = pending.pop() {
        if part == ".." {
            at.pop();
            continue;
        }
        let next = at.join(&part);
        let Ok(meta) = std::fs::symlink_metadata(&next) else {
            return false;
        };
        if meta.file_type().is_symlink() {
            links += 1;
            if links > MAX_LINKS || !trusted(&next, &meta, true) {
                return false;
            }
            let Ok(target) = std::fs::read_link(&next) else {
                return false;
            };
            if target.is_absolute() {
                at = PathBuf::from("/");
            }
            push(&mut pending, &target);
            continue;
        }
        if !meta.is_dir() || !trusted(&next, &meta, false) {
            return false;
        }
        at = next;
    }
    true
}

/// Open the directory `dir` for use as the home of kept files, or `None`
/// when it is not one this user owns and alone can change.
///
/// The directory is opened without following a link, then checked through
/// that descriptor: owned by this user, and not writable by a group or
/// another user. The directories above it must pass [`trusted_ancestors`].
fn open_private_dir(dir: &Path) -> Option<File> {
    if !trusted_ancestors(dir.parent()?) {
        return None;
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(dir)
        .ok()?;
    let meta = file.metadata().ok()?;
    (meta.is_dir() && meta.uid() == me() && meta.mode() & OTHERS_WRITE == 0).then_some(file)
}

/// Make `dir` the home of kept files, for a write.
///
/// A missing directory is made `0700`. One that exists must be a real
/// directory of this user, not a link. One this user owns that a group or
/// another user can write has those bits taken off, and anything else is
/// refused. The directories above it must pass [`trusted_ancestors`]; they
/// are made `0700` when missing and are never changed. The result is the
/// directory opened and checked.
pub(crate) fn ensure_private_dir(dir: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::DirBuilderExt as _;
    let refused = |why: &str| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{}: {why}", dir.display()),
        )
    };
    let parent = dir
        .parent()
        .ok_or_else(|| refused("not a directory of this user"))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    if !trusted_ancestors(parent) {
        return Err(refused(
            "a directory or link above it is one another user can change",
        ));
    }
    if let Err(err) = std::fs::symlink_metadata(dir) {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err);
        }
        std::fs::DirBuilder::new().mode(0o700).create(dir)?;
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(dir)
        .map_err(|_| refused("not a directory of this user"))?;
    let meta = file.metadata()?;
    if !meta.is_dir() || meta.uid() != me() {
        return Err(refused("not a directory of this user"));
    }
    if meta.mode() & OTHERS_WRITE != 0 {
        crate::change_mode_of(&file, meta.mode() & 0o7777 & !OTHERS_WRITE)?;
    }
    Ok(file)
}

/// Open `name` in the directory `dir` for reading, without following a
/// link and without waiting on a pipe. `None` when it cannot be opened.
#[allow(
    clippy::disallowed_methods,
    reason = "owns a descriptor this function opened, never stdout or stderr"
)]
fn open_in(dir: &File, name: &std::ffi::OsStr) -> Option<File> {
    use std::os::unix::ffi::OsStrExt as _;
    let name = std::ffi::CString::new(name.as_bytes()).ok()?;
    let flags =
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC;
    // SAFETY: `dir` is an open descriptor and `name` is a valid C string.
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return None;
    }
    // SAFETY: openat returned this descriptor and nothing else owns it.
    Some(unsafe { File::from_raw_fd(fd) })
}

/// Open `name` in `dir` as [`open_in`] does, when it is a regular file
/// with no other name.
fn open_plain_in(dir: &File, name: &std::ffi::OsStr) -> Option<(File, Metadata)> {
    let file = open_in(dir, name)?;
    let meta = file.metadata().ok()?;
    (meta.is_file() && meta.nlink() == 1).then_some((file, meta))
}

/// Whether `meta` is a file this user owns that no other user can write.
fn own_private_file(meta: &Metadata) -> bool {
    meta.uid() == me() && meta.mode() & OTHERS_WRITE == 0
}

/// Open a kept file for reading, or `None` when it is not one this user
/// owns and alone can write. This is the one way a kept listing or its
/// sidecar is read.
///
/// The directory is checked first, see [`open_private_dir`]. The file is
/// opened from that directory without following a link, and without
/// waiting on a pipe, then checked through the descriptor it opened: a
/// regular file, owned by this user, writable by no other user, with one
/// name. Checking a path and then opening it again would read whatever
/// the path names by then.
pub(crate) fn open_private_file(path: &Path) -> Option<(File, Metadata)> {
    let dir = open_private_dir(path.parent()?)?;
    let (file, meta) = open_plain_in(&dir, path.file_name()?)?;
    own_private_file(&meta).then_some((file, meta))
}

/// Open the directory `name` in `dir`, without following a link and
/// without waiting on a pipe. `None` when it is not a directory.
#[allow(
    clippy::disallowed_methods,
    reason = "owns a descriptor this function opened, never stdout or stderr"
)]
fn open_dir_in(dir: &File, name: &std::ffi::OsStr) -> Option<File> {
    use std::os::unix::ffi::OsStrExt as _;
    let name = std::ffi::CString::new(name.as_bytes()).ok()?;
    let flags =
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    // SAFETY: `dir` is an open descriptor and `name` is a valid C string.
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return None;
    }
    // SAFETY: openat returned this descriptor and nothing else owns it.
    Some(unsafe { File::from_raw_fd(fd) })
}

/// The bytes of a file in the tracker's memory slot `root`, or `None` when
/// it is not a plain file there. This is how a search, `ljos forget`,
/// `ljos show` and `ljos memory integrate` read the slot's index, kept
/// rows, receipts and watermark.
///
/// `path` must lie under `root`. Each directory from `root` down is opened
/// from the one before it, none of them a link, so a link in any name
/// below `root` leaves the file unread. The file is then opened without
/// following a link or waiting on a pipe, and read only when it is a
/// regular file with one name that not every user can write. It may
/// belong to the tracker's group. The slot's directories are made with the
/// person's umask, so their mode is not checked.
pub(crate) fn read_slot_file(root: &Path, path: &Path) -> Option<Vec<u8>> {
    let rel = path.strip_prefix(root).ok()?;
    let name = rel.file_name()?;
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let mut dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(root)
        .ok()?;
    for part in rel.parent()?.components() {
        let Component::Normal(part) = part else {
            return None;
        };
        dir = open_dir_in(&dir, part)?;
    }
    let (mut file, meta) = open_plain_in(&dir, name)?;
    (meta.mode() & WORLD_WRITE == 0).then_some(())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The text of a slot file, see [`read_slot_file`].
pub(crate) fn read_slot_text(root: &Path, path: &Path) -> Option<String> {
    String::from_utf8(read_slot_file(root, path)?).ok()
}

/// The whole text of the kept file at `path`, see [`open_private_file`].
pub(crate) fn read_private_file(path: &Path) -> Option<String> {
    read_open_file(open_private_file(path)?.0)
}

/// The text of a file shared through the tracker, or `None` when it is not
/// a plain file.
///
/// A tracker is a checkout, so its files are not always this user's, and a
/// group that shares it may write them. This opens without following a
/// link or waiting on a pipe, and reads a regular file that is not
/// writable by every user.
pub(crate) fn read_shared_file(path: &Path) -> Option<String> {
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path.parent()?)
        .ok()?;
    let file = open_in(&dir, path.file_name()?)?;
    let meta = file.metadata().ok()?;
    (meta.is_file() && meta.mode() & WORLD_WRITE == 0).then_some(())?;
    read_open_file(file)
}

pub(crate) fn read_open_file(mut file: File) -> Option<String> {
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::Permissions;
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    fn plain(dir: &Path, name: &str, mode: u32) -> std::path::PathBuf {
        let path = dir.join(name);
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, "kept").unwrap();
        crate::change_mode(&path, mode).unwrap();
        path
    }

    fn fifo(path: &Path) {
        let name = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
        // SAFETY: the name is a valid C string and mkfifo keeps no pointer.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    }

    fn home() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        crate::change_mode(root.path(), 0o700).unwrap();
        root
    }

    #[test]
    fn a_kept_file_is_read_only_when_it_is_a_plain_file_of_this_user() {
        let root = home();
        let dir = root.path();
        let file = plain(dir, "plain.json", 0o600);
        assert_eq!(read_private_file(&file).as_deref(), Some("kept"));
        for mode in [0o620, 0o602, 0o666] {
            crate::change_mode(&file, mode).unwrap();
            assert!(read_private_file(&file).is_none(), "mode {mode:o}");
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "sets the mode of a path that is not a git hook"
        )]
        std::fs::set_permissions(&file, Permissions::from_mode(0o600)).unwrap();
        let link = dir.join("link.json");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&file, &link).unwrap();
        assert!(read_private_file(&link).is_none(), "a link");
        let zero = dir.join("zero.json");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink("/dev/zero", &zero).unwrap();
        assert!(read_private_file(&zero).is_none(), "a link to a device");
        let pipe = dir.join("pipe.json");
        fifo(&pipe);
        assert!(read_private_file(&pipe).is_none(), "a pipe");
        assert!(read_private_file(&dir.join("none.json")).is_none());
    }

    #[test]
    fn a_kept_file_with_another_name_is_not_read() {
        let root = home();
        let file = plain(root.path(), "alice.json", 0o600);
        assert!(read_private_file(&file).is_some());
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        std::fs::hard_link(&file, root.path().join("bob.json")).unwrap();
        assert!(read_private_file(&file).is_none(), "the first name");
        assert!(
            read_private_file(&root.path().join("bob.json")).is_none(),
            "the second name"
        );
    }

    #[test]
    fn a_kept_file_in_a_directory_others_can_write_is_not_read() {
        let root = home();
        let dir = root.path().join("ljos");
        std::fs::create_dir(&dir).unwrap();
        let file = plain(&dir, "a.json", 0o600);
        for mode in [0o770, 0o707, 0o777] {
            crate::change_mode(&dir, mode).unwrap();
            assert!(read_private_file(&file).is_none(), "directory {mode:o}");
        }
        crate::change_mode(&dir, 0o755).unwrap();
        assert!(read_private_file(&file).is_some(), "a directory of 755");
    }

    #[test]
    fn a_kept_file_in_a_directory_that_is_a_link_is_not_read() {
        let root = home();
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        plain(&real, "a.json", 0o600);
        let link = root.path().join("ljos");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&real, &link).unwrap();
        assert!(read_private_file(&link.join("a.json")).is_none());
        assert!(read_private_file(&real.join("a.json")).is_some());
    }

    #[test]
    fn a_directory_for_a_write_is_made_tightened_or_refused() {
        let root = home();
        let made = root.path().join("made");
        ensure_private_dir(&made).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&made), 0o700, "a directory made for the purpose");
        let open = root.path().join("open");
        std::fs::create_dir(&open).unwrap();
        crate::change_mode(&open, 0o777).unwrap();
        ensure_private_dir(&open).unwrap();
        assert_eq!(mode(&open), 0o755, "group and other write taken off");
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = root.path().join("link");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&real, &link).unwrap();
        assert!(ensure_private_dir(&link).is_err(), "a link");
        let file = plain(root.path(), "file", 0o600);
        assert!(ensure_private_dir(&file).is_err(), "a file");
    }

    #[test]
    fn an_entry_on_the_way_is_trusted_only_when_no_other_user_can_change_it() {
        let me = 1000;
        assert!(trusted_entry(me, 0o40755, false, me));
        assert!(trusted_entry(0, 0o40755, false, me), "a directory of root");
        assert!(trusted_entry(0, 0o41777, false, me), "a sticky /tmp");
        assert!(trusted_entry(me, 0o120777, true, me), "a link of this user");
        assert!(trusted_entry(0, 0o120777, true, me), "a link of root");
        assert!(
            !trusted_entry(1001, 0o40755, false, me),
            "another user's directory"
        );
        assert!(
            !trusted_entry(1001, 0o120777, true, me),
            "another user's link"
        );
        for mode in [0o40777, 0o40775, 0o40757] {
            assert!(!trusted_entry(me, mode, false, me), "directory {mode:o}");
        }
    }

    #[test]
    fn a_kept_file_under_a_parent_others_can_write_is_not_read_or_written() {
        let root = home();
        let data = root.path().join("data");
        let dir = data.join("ljos");
        std::fs::create_dir_all(&dir).unwrap();
        crate::change_mode(&dir, 0o700).unwrap();
        let file = plain(&dir, "a.json", 0o600);
        assert!(read_private_file(&file).is_some());
        for mode in [0o777, 0o770, 0o707] {
            crate::change_mode(&data, mode).unwrap();
            assert!(read_private_file(&file).is_none(), "parent {mode:o}");
            assert!(ensure_private_dir(&dir).is_err(), "parent {mode:o}");
            assert!(
                ensure_private_dir(&data.join("new")).is_err(),
                "parent {mode:o}"
            );
            assert!(!data.join("new").exists(), "made under parent {mode:o}");
        }
        crate::change_mode(&data, 0o1777).unwrap();
        assert!(read_private_file(&file).is_some(), "a sticky parent");
    }

    #[test]
    fn a_linked_data_home_is_followed_only_through_directories_no_other_user_can_change() {
        let root = home();
        let good = root.path().join("good");
        std::fs::create_dir_all(good.join("ljos")).unwrap();
        let file = plain(&good.join("ljos"), "a.json", 0o600);
        let mine = root.path().join("mine");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&good, &mine).unwrap();
        assert!(
            read_private_file(&mine.join("ljos/a.json")).is_some(),
            "a link of this user"
        );
        let relative = root.path().join("relative");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink("good", &relative).unwrap();
        assert!(
            read_private_file(&relative.join("ljos/a.json")).is_some(),
            "a relative link"
        );

        let open = root.path().join("open");
        std::fs::create_dir(&open).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&good, open.join("link")).unwrap();
        crate::change_mode(&open, 0o777).unwrap();
        assert!(
            read_private_file(&open.join("link/ljos/a.json")).is_none(),
            "a link in a 777 directory"
        );
        assert!(ensure_private_dir(&open.join("link/ljos")).is_err());

        let behind = root.path().join("behind");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(open.join("data"), &behind).unwrap();
        std::fs::create_dir_all(open.join("data/ljos")).unwrap();
        let hidden = plain(&open.join("data/ljos"), "a.json", 0o600);
        assert!(
            read_private_file(&hidden).is_none(),
            "under a 777 directory"
        );
        assert!(
            read_private_file(&behind.join("ljos/a.json")).is_none(),
            "a link to a 777 directory"
        );
        assert!(read_private_file(&file).is_some());
    }

    #[test]
    fn a_slot_file_is_read_only_when_it_is_a_plain_file_with_one_name() {
        let root = home();
        let top = root.path();
        let slot = root.path().join("slot");
        std::fs::create_dir(&slot).unwrap();
        crate::change_mode(&slot, 0o775).unwrap();
        let index = plain(&slot, "index.jsonl", 0o664);
        assert_eq!(read_slot_file(top, &index).as_deref(), Some(&b"kept"[..]));
        #[expect(
            clippy::disallowed_methods,
            reason = "sets the mode of a path that is not a git hook"
        )]
        std::fs::set_permissions(&index, Permissions::from_mode(0o666)).unwrap();
        assert!(
            read_slot_file(top, &index).is_none(),
            "a file every user can write"
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "sets the mode of a path that is not a git hook"
        )]
        std::fs::set_permissions(&index, Permissions::from_mode(0o644)).unwrap();
        let link = slot.join("link.jsonl");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&index, &link).unwrap();
        assert!(read_slot_file(top, &link).is_none(), "a link");
        let pipe = slot.join("pipe.jsonl");
        fifo(&pipe);
        assert!(read_slot_file(top, &pipe).is_none(), "a pipe");
        std::fs::create_dir(slot.join("dir.jsonl")).unwrap();
        assert!(
            read_slot_file(top, &slot.join("dir.jsonl")).is_none(),
            "a directory"
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        std::fs::hard_link(&index, slot.join("other.jsonl")).unwrap();
        assert!(
            read_slot_file(top, &index).is_none(),
            "a file with another name"
        );
        let solo = plain(&slot, "solo.jsonl", 0o644);
        assert!(read_slot_file(top, &solo).is_some());
        let linked_slot = root.path().join("linked");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&slot, &linked_slot).unwrap();
        assert!(
            read_slot_file(top, &linked_slot.join("solo.jsonl")).is_none(),
            "a slot directory that is a link"
        );
    }

    #[test]
    fn a_slot_file_is_not_read_through_a_link_above_its_directory() {
        let root = home();
        let top = root.path().join("memory");
        let slot = top.join("imports").join("bob").join("imp1");
        std::fs::create_dir_all(&slot).unwrap();
        for dir in [
            top.as_path(),
            &top.join("imports"),
            &top.join("imports/bob"),
            &slot,
        ] {
            crate::change_mode(dir, 0o775).unwrap();
        }
        let index = plain(&slot, "index.jsonl", 0o664);
        assert!(
            read_slot_file(&top, &index).is_some(),
            "a slot its group can write"
        );

        let elsewhere = root.path().join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("imp1")).unwrap();
        plain(&elsewhere.join("imp1"), "index.jsonl", 0o644);
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&elsewhere, top.join("imports").join("eve")).unwrap();
        assert!(
            read_slot_file(&top, &top.join("imports/eve/imp1/index.jsonl")).is_none(),
            "a seat directory that is a link"
        );
        assert!(
            read_slot_file(&top, &top.join("imports/bob/../eve/imp1/index.jsonl")).is_none(),
            "a path that leaves the slot"
        );
        fifo(&top.join("imports").join("hole"));
        assert!(
            read_slot_file(&top, &top.join("imports/hole/index.jsonl")).is_none(),
            "a pipe where a directory belongs"
        );
        assert!(
            read_slot_file(&slot, &top.join("kept/a.json")).is_none(),
            "a path outside the root"
        );
    }

    #[test]
    fn a_shared_file_is_read_when_it_is_plain_and_not_world_writable() {
        let root = home();
        let dir = root.path();
        let file = plain(dir, "row.json", 0o664);
        assert_eq!(read_shared_file(&file).as_deref(), Some("kept"));
        #[expect(
            clippy::disallowed_methods,
            reason = "sets the mode of a path that is not a git hook"
        )]
        std::fs::set_permissions(&file, Permissions::from_mode(0o666)).unwrap();
        assert!(
            read_shared_file(&file).is_none(),
            "a file every user can write"
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "sets the mode of a path that is not a git hook"
        )]
        std::fs::set_permissions(&file, Permissions::from_mode(0o644)).unwrap();
        let link = dir.join("link.json");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&file, &link).unwrap();
        assert!(read_shared_file(&link).is_none(), "a link");
        let pipe = dir.join("pipe.json");
        fifo(&pipe);
        assert!(read_shared_file(&pipe).is_none(), "a pipe");
        assert!(read_shared_file(&dir.join("none.json")).is_none());
    }
}
