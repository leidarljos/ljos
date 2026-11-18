//! The three load-time statics the other tests get from the test-boot
//! crate's macro, written out so a test file can pull them in with one
//! `#[path]` module. Each keeps one address, so the linker keeps the
//! object that moves a test process out of a runner cgroup, drops runner
//! variables and canonicalises the temp directory before `main`.

use std::ffi::c_char;

#[used]
#[allow(dead_code)]
static LJOS_TEST_ENTER: unsafe extern "C" fn(*const c_char) -> i32 = ljos_test_boot::ENTER;

#[used]
#[allow(dead_code)]
static LJOS_TEST_SESSION: unsafe extern "C" fn() = ljos_test_boot::SESSION;

#[used]
#[allow(dead_code)]
static LJOS_TEST_CANON: unsafe extern "C" fn() = ljos_test_boot::CANON_TMP;
