//! Where a test makes scratch trees, and the `git` a test runs in them.
//!
//! A scratch repository whose `.git` is missing or half made is not a
//! repository, so git walks up from it. Above the checkout's `target` is
//! the ljos checkout, and a `git config` there lands in its `.git/config`.
//! Every scratch root is a ceiling for the git this module builds, so the
//! walk ends at the root and git reports that it found no repository.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// The variable git reads its ceilings from.
pub const GIT_CEILING: &str = "GIT_CEILING_DIRECTORIES";

/// The checkout this crate was built from.
#[must_use]
pub fn checkout() -> PathBuf {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::canonicalize(&crates).unwrap_or(crates)
}

/// Seat roots under `HOME`, which sit outside the checkout.
#[must_use]
pub fn home_seats() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".cache/ljos-test-seats"))
}

/// Seat roots inside the checkout. Used only when `HOME` has no room.
#[must_use]
pub fn checkout_seats() -> PathBuf {
    checkout().join("crates/target")
}

/// Every directory a test makes scratch trees under, plus any ceiling the
/// process inherited, joined for `GIT_CEILING_DIRECTORIES`.
///
/// # Panics
///
/// A root contains the path separator.
#[must_use]
pub fn git_ceiling() -> OsString {
    let mut roots: Vec<PathBuf> = std::env::var_os(GIT_CEILING)
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    let checkout = checkout();
    let found = [
        Some(std::env::temp_dir()),
        Some(PathBuf::from("/tmp")),
        home_seats(),
        Some(checkout.join("crates/target")),
        Some(checkout.join("target")),
    ];
    for root in found.into_iter().flatten() {
        // git drops a relative ceiling, and a spelling with a symlink in it
        // does not match the directory git walks through.
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        if root.is_absolute() && !roots.contains(&root) {
            roots.push(root);
        }
    }
    std::env::join_paths(roots).expect("a scratch root contains the path separator")
}

/// `git` from `PATH` with [`git_ceiling`] set.
#[must_use]
pub fn git() -> Command {
    git_at("git")
}

/// `program` as git, with [`git_ceiling`] set. `env_clear` drops the
/// ceiling, so a caller that clears the environment sets
/// [`GIT_CEILING`] to [`git_ceiling`] again after it.
#[must_use]
pub fn git_at(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.env(GIT_CEILING, git_ceiling());
    cmd
}

/// A new directory under `root`, mode `0o755`, named
/// `{prefix}-{pid}-{n}`. No other call in any process gets the same one:
/// the directory is made with `create_dir`, which fails when it exists,
/// and a name already taken moves on to the next `n`. Nothing is removed,
/// so a test never deletes a tree another test is still building.
///
/// # Panics
///
/// `root` cannot be made, or a directory under it cannot.
#[must_use]
pub fn fresh_dir(root: &Path, prefix: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(root)
        .unwrap_or_else(|err| panic!("scratch root {}: {err}", root.display()));
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = root.join(format!("{prefix}-{}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
                        .unwrap_or_else(|err| panic!("chmod {}: {err}", dir.display()));
                }
                return dir;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => panic!("scratch dir {}: {err}", dir.display()),
        }
    }
}
