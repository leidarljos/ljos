//! A stand-in for a runner: a copy of the shell, so the kernel runs it
//! from a file of the runner's name, which is how `ljos hook` knows the
//! runner spawned it.

use std::path::Path;

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
