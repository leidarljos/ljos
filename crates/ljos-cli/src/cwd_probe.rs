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

use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
    let Ok(here) = std::env::current_dir() else {
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

fn remember_skip(dir: &Path) {
    *SKIP_FROM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir.to_path_buf());
}

/// Leave `here` for an empty directory on the local temp filesystem.
///
/// A tracker the environment already names is kept. A tracker the seat's
/// configuration names, when it is not inside a product checkout, is
/// named on the environment so the command still writes there. Staying
/// in `here` is what lets a later stat wait.
fn park(here: &Path, move_process_cwd: bool) {
    if !move_process_cwd {
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
    let park = std::env::temp_dir().join(format!("ljos-cwd-{}", std::process::id()));
    if std::fs::create_dir_all(&park).is_ok() {
        let _ = std::env::set_current_dir(&park);
    }
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
        assert_eq!(probe_answer(false, None), Some(false));
        assert_eq!(probe_answer(true, Some(true)), Some(true));
        assert_eq!(probe_answer(true, Some(false)), Some(false));
        assert_eq!(probe_answer(true, None), None);
    }

    #[test]
    fn a_fuse_or_network_mount_is_slow_and_the_root_disk_is_not() {
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
