//! Memory a secret passes through, locked with `mlock` so it is not
//! written to swap. A page stays locked while any secret on it lives,
//! and a secret is zeroed before its pages are unlocked. When the limit
//! refuses the lock, the secret is kept unlocked and ljos says so once.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use zeroize::Zeroize as _;

/// How many live secrets lock each page, by its address.
static PAGES: Mutex<BTreeMap<usize, usize>> = Mutex::new(BTreeMap::new());

static WARNED: AtomicBool = AtomicBool::new(false);

fn page_size() -> usize {
    // SAFETY: sysconf reads a constant of the system.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(size)
        .ok()
        .filter(|&s| s > 0)
        .unwrap_or(4096)
}

/// The pages `len` bytes at `addr` lie on.
fn pages(addr: usize, len: usize) -> impl Iterator<Item = usize> {
    let size = page_size();
    (addr / size * size..addr + len).step_by(size)
}

/// Lock the pages of `len` bytes at `addr`. False when the system
/// refuses, after a warning the first time.
fn lock(addr: usize, len: usize) -> bool {
    let size = page_size();
    let mut counts = PAGES.lock().unwrap_or_else(PoisonError::into_inner);
    let first = addr / size * size;
    // SAFETY: the range covers memory this process owns; mlock only pins it.
    if unsafe { libc::mlock(first as *const libc::c_void, addr + len - first) } != 0 {
        let err = std::io::Error::last_os_error();
        if !WARNED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "ljos: the memory that holds the passphrase cannot be locked ({err}), so it \
                 may reach swap; raise `ulimit -l` to lock it"
            );
        }
        return false;
    }
    for page in pages(addr, len) {
        *counts.entry(page).or_insert(0) += 1;
    }
    true
}

/// Unlock the pages of `len` bytes at `addr` that no other secret locks.
fn unlock(addr: usize, len: usize) {
    let size = page_size();
    let mut counts = PAGES.lock().unwrap_or_else(PoisonError::into_inner);
    for page in pages(addr, len) {
        let Some(count) = counts.get_mut(&page) else {
            continue;
        };
        *count -= 1;
        if *count == 0 {
            counts.remove(&page);
            // SAFETY: one page this process locked and still maps.
            unsafe { libc::munlock(page as *const libc::c_void, size) };
        }
    }
}

/// Bytes locked in memory while they live, and zeroed then unlocked when
/// dropped. The buffer never grows past the capacity it was made with,
/// so it never moves and leaves no copy behind.
pub(crate) struct Locked {
    bytes: Vec<u8>,
    locked: bool,
}

impl Locked {
    /// An empty buffer for up to `cap` bytes.
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self::lock(Vec::with_capacity(cap))
    }

    /// `len` zero bytes.
    pub(crate) fn zeroed(len: usize) -> Self {
        Self::lock(vec![0; len])
    }

    fn lock(bytes: Vec<u8>) -> Self {
        let locked = bytes.capacity() > 0 && lock(bytes.as_ptr() as usize, bytes.capacity());
        Self { bytes, locked }
    }

    /// Add `byte` at the end. False, and nothing added, when the buffer
    /// is full.
    pub(crate) fn push(&mut self, byte: u8) -> bool {
        if self.bytes.len() == self.bytes.capacity() {
            return false;
        }
        self.bytes.push(byte);
        true
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    /// Whether the system locked the buffer.
    #[cfg(test)]
    pub(crate) fn is_locked(&self) -> bool {
        self.locked
    }
}

impl std::ops::Deref for Locked {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl std::ops::DerefMut for Locked {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        let (addr, cap) = (self.bytes.as_ptr() as usize, self.bytes.capacity());
        self.bytes.zeroize();
        if self.locked {
            unlock(addr, cap);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    const CHILD: &str = "LJOS_MEMLOCK_TEST_CHILD";

    /// The kilobytes this process has locked, from `/proc/self/status`.
    fn locked_kb() -> u64 {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        status
            .lines()
            .find_map(|l| l.strip_prefix("VmLck:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
            .unwrap()
    }

    /// Run in a process of its own, where no other test locks memory: a
    /// secret is locked while it lives and unlocked once dropped, a page
    /// two secrets share stays locked until both are gone, and under a
    /// limit of zero a secret still works unlocked, with one warning.
    fn child() {
        let base = locked_kb();
        let a = Locked::zeroed(32);
        let b = Locked::with_capacity(32);
        assert!(a.is_locked() && b.is_locked());
        assert!(locked_kb() > base, "nothing was locked");
        drop(a);
        assert!(locked_kb() > base, "a page b is on was unlocked");
        drop(b);
        assert_eq!(locked_kb(), base, "a dropped secret stayed locked");
        // SAFETY: reads and sets this process's own limit through a local.
        unsafe {
            let mut limit: libc::rlimit = std::mem::zeroed();
            assert_eq!(libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut limit), 0);
            limit.rlim_cur = 0;
            assert_eq!(libc::setrlimit(libc::RLIMIT_MEMLOCK, &limit), 0);
        }
        let mut c = Locked::with_capacity(4);
        let d = Locked::zeroed(16);
        if c.is_locked() {
            eprintln!("CAP_IPC_LOCK");
            return;
        }
        assert!(!d.is_locked());
        assert!(c.push(7) && c.push(8));
        assert_eq!(&c[..], &[7, 8]);
        assert_eq!(locked_kb(), base);
    }

    #[test]
    fn a_secret_is_locked_until_dropped_and_works_without_a_lock() {
        if std::env::var_os(CHILD).is_some() {
            child();
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "memlock::tests::a_secret_is_locked_until_dropped_and_works_without_a_lock",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{err}");
        if !err.contains("CAP_IPC_LOCK") {
            assert_eq!(err.matches("cannot be locked").count(), 1, "{err}");
        }
    }

    #[test]
    fn a_full_buffer_refuses_a_byte_and_keeps_its_place() {
        let mut s = Locked::with_capacity(2);
        let at = s.as_ptr();
        assert!(s.push(1) && s.push(2));
        assert!(!s.push(3));
        assert_eq!((&s[..], s.as_ptr(), s.capacity()), (&[1, 2][..], at, 2));
    }
}
