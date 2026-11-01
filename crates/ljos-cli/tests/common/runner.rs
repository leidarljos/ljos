//! A stand-in for a runner: a copy of the shell, so the kernel runs it
//! from a file of the runner's name, which is how `ljos hook` knows the
//! runner spawned it.

use std::path::Path;

/// A directory for stand-ins below `~/.local/bin` of the home this user's
/// passwd entry names, which is where ljos looks for an installed runner
/// whatever `HOME` says. Removed when dropped.
pub fn install_dir() -> tempfile::TempDir {
    let bin = ljos_cli::proc_tree::own_home()
        .expect("this user's passwd entry names a home")
        .join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    tempfile::Builder::new()
        .prefix(".ljos-runner-")
        .tempdir_in(&bin)
        .unwrap()
}

/// Copy `/bin/sh` to `path` unless it is there, and wait until it runs.
/// A test thread that forked while the copy was open holds it for writing
/// until that child execs, and an exec meanwhile fails as text file busy.
pub fn runner_at(path: &Path) {
    if path.exists() {
        return;
    }
    let tmp = path.with_extension("part");
    std::fs::copy("/bin/sh", &tmp).unwrap();
    std::fs::rename(&tmp, path).unwrap();
    for _ in 0..200 {
        match std::process::Command::new(path).args(["-c", ":"]).status() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return,
        }
    }
}
