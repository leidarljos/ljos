//! Where a test puts a seat home. ljos stays out of a temp HOME, so the
//! root has to sit outside the temp roots. It also sits outside the
//! checkout when it can, so a git walk from a scratch tree whose `.git`
//! is missing never reaches the checkout's `.git`.

use std::path::{Path, PathBuf};

/// Bytes a seat root may take. A test nests a server socket about 56
/// bytes below the root (`hq-<name>-<pid>-<n>/run/ljos/hookd.sock`),
/// and `sun_path` holds 107 bytes before its NUL on Linux and 103 on
/// macOS.
const ROOM: usize = 44;

/// A cache under `HOME`, or the checkout's `crates/target` when `HOME` is
/// a temp root or its path leaves a nested socket no room. A git call in
/// the checkout fallback is held back by `ljos_test_boot::git_ceiling`.
/// `is_temp` is `bookkeep::path_is_temp`.
pub fn seat_root(is_temp: fn(Option<&Path>) -> bool) -> PathBuf {
    let target = ljos_test_boot::checkout_seats();
    let fits = |root: &Path| root.as_os_str().len() <= ROOM;
    let cache = ljos_test_boot::home_seats().filter(|root| !is_temp(Some(root)));
    match cache {
        Some(cache) if fits(&cache) || is_temp(Some(&target)) => cache,
        _ => target,
    }
}

/// A new seat directory under [`seat_root`] that no other test shares.
pub fn seat_dir(is_temp: fn(Option<&Path>) -> bool, prefix: &str) -> PathBuf {
    ljos_test_boot::fresh_dir(&seat_root(is_temp), prefix)
}
