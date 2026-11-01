//! Where a test puts a seat home. ljos stays out of a temp HOME, so the
//! root has to sit outside the temp roots wherever the checkout does.

use std::path::{Path, PathBuf};

/// Bytes a seat root may take. A test nests a server socket about 56
/// bytes below the root (`hq-<name>-<pid>-<nanos>/run/ljos/hookd.sock`),
/// and `sun_path` holds 107 bytes before its NUL on Linux and 103 on
/// macOS.
const ROOM: usize = 44;

/// The checkout's `target`, or a cache under `HOME` when the checkout
/// itself sits in a temp root or its path leaves a nested socket no room.
/// `is_temp` is `bookkeep::path_is_temp`.
pub fn seat_root(is_temp: fn(Option<&Path>) -> bool) -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target = manifest.parent().map_or_else(
        || manifest.join("../target"),
        |crates| crates.join("target"),
    );
    let fits = |root: &Path| root.as_os_str().len() <= ROOM;
    if !is_temp(Some(&target)) && fits(&target) {
        return target;
    }
    let cache = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".cache/ljos-test-seats"))
        .filter(|root| !is_temp(Some(root)));
    match cache {
        Some(cache) if is_temp(Some(&target)) || fits(&cache) => cache,
        _ => target,
    }
}
