//! Environment mutation used by HUD test guards.

/// Set one process variable while a test guard owns and restores it.
#[allow(
    clippy::disallowed_methods,
    reason = "the test env guards perform the mutation here"
)]
pub fn set_var(key: impl AsRef<std::ffi::OsStr>, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: callers serialize the test environment and restore the value.
    unsafe { std::env::set_var(key, value) };
}

/// Remove one process variable while a test guard owns and restores it.
#[allow(
    clippy::disallowed_methods,
    reason = "the test env guards perform the mutation here"
)]
pub fn remove_var(key: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: callers serialize the test environment and restore the value.
    unsafe { std::env::remove_var(key) };
}

/// Set one process variable from an existing unsafe test guard block.
///
/// # Safety
///
/// The caller must serialize the process environment and restore the value.
#[allow(
    clippy::disallowed_methods,
    reason = "the test env guards perform the mutation here"
)]
pub unsafe fn set_var_unchecked(
    key: impl AsRef<std::ffi::OsStr>,
    value: impl AsRef<std::ffi::OsStr>,
) {
    // SAFETY: the caller owns and restores the test environment.
    unsafe { std::env::set_var(key, value) };
}

/// Remove one process variable from an existing unsafe test guard block.
///
/// # Safety
///
/// The caller must serialize the process environment and restore the value.
#[allow(
    clippy::disallowed_methods,
    reason = "the test env guards perform the mutation here"
)]
pub unsafe fn remove_var_unchecked(key: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: the caller owns and restores the test environment.
    unsafe { std::env::remove_var(key) };
}
