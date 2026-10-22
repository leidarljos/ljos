//! Where a test puts a seat home. ljos stays out of a temp HOME, so the
//! root has to sit outside the temp roots wherever the checkout does.

use std::path::{Path, PathBuf};

/// The checkout's `target`, or a cache under `HOME` when the checkout
/// itself sits in a temp root. `is_temp` is `bookkeep::path_is_temp`.
pub fn seat_root(is_temp: fn(Option<&Path>) -> bool) -> PathBuf {
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target");
    if !is_temp(Some(&target)) {
        return target;
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".cache/ljos-test-seats"))
        .filter(|root| !is_temp(Some(root)))
        .unwrap_or(target)
}
