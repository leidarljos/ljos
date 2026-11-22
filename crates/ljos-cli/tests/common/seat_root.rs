//! Where a test puts a seat home. ljos stays out of a temp HOME, so the
//! root has to sit outside the temp roots. It also sits outside the
//! checkout, so a git walk from a scratch tree whose `.git` is missing
//! never reaches the checkout's `.git`.

use std::path::{Path, PathBuf};

/// Bytes a seat root may take. A test nests a server socket about 56
/// bytes below the root (`hq-<name>-<pid>-<n>/run/ljos/hookd.sock`),
/// and `sun_path` holds 107 bytes before its NUL on Linux and 103 on
/// macOS.
const ROOM: usize = 44;

/// The cache under `HOME`. There is no fallback inside the checkout: a
/// `HOME` that is unset, a temp root, or too long for a nested socket
/// stops the test with the reason. `is_temp` is `bookkeep::path_is_temp`.
///
/// # Panics
///
/// See [`pick_seat_root`].
pub fn seat_root(is_temp: fn(Option<&Path>) -> bool) -> PathBuf {
    pick_seat_root(ljos_test_boot::home_seats(), is_temp).unwrap_or_else(|why| panic!("{why}"))
}

/// `cache` when a seat home may go there, else why not.
///
/// # Errors
///
/// `cache` is missing, a temp root, longer than [`ROOM`] bytes, or
/// inside the checkout.
pub fn pick_seat_root(
    cache: Option<PathBuf>,
    is_temp: fn(Option<&Path>) -> bool,
) -> Result<PathBuf, String> {
    let Some(cache) = cache else {
        return Err("seat root: HOME is unset or empty; set HOME to a real home".into());
    };
    let why = if is_temp(Some(&cache)) {
        "is a temp root, and ljos stays out of a temp HOME"
    } else if cache.as_os_str().len() > ROOM {
        "leaves a nested socket path no room"
    } else if cache.starts_with(ljos_test_boot::checkout()) {
        "is inside the ljos checkout"
    } else {
        return Ok(cache);
    };
    Err(format!(
        "seat root: {} {why}; set HOME to a real home of at most {} bytes",
        cache.display(),
        ROOM - ".cache/ljos-test-seats".len() - 1
    ))
}

/// A new seat directory under [`seat_root`] that no other test shares.
pub fn seat_dir(is_temp: fn(Option<&Path>) -> bool, prefix: &str) -> PathBuf {
    ljos_test_boot::fresh_dir(&seat_root(is_temp), prefix)
}
