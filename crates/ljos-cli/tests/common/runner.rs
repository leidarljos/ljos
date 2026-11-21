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

/// Copy `/bin/sh` to `path` unless it is there, then start it once.
///
/// The copy runs in another process and exits before the rename, so a fork
/// in this process does not keep a write descriptor on `path`.
pub fn runner_at(path: &Path) {
    if path.exists() {
        return;
    }
    ljos_cli::copy_executable(Path::new("/bin/sh"), path).unwrap();
    let _ = std::process::Command::new(path).args(["-c", ":"]).status();
}
