//! A line read from the controlling terminal with echo off. Only
//! `/dev/tty` is read: stdin, a variable or an argument is never the
//! answer. The terminal's settings come back on every way out, a
//! signal included.

use std::cell::UnsafeCell;
use std::io::Write as _;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd as _;
use std::sync::atomic::{AtomicI32, Ordering};

use anyhow::{bail, Context, Result};

/// The longest line read, in bytes. A longer one is refused.
pub(crate) const MAX_LINE: usize = 1024;

/// Signals that end the process while the prompt is up. Each restores the
/// terminal first, then takes its default action.
const FATAL: &[libc::c_int] = &[libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];

/// Signals that would stop the process with echo off. They wait until
/// the prompt is down.
const STOPS: &[libc::c_int] = &[libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU];

/// Bytes locked in memory, and zeroed when dropped. The buffer is
/// [`MAX_LINE`] long from the start, so a long line never moves it and
/// leaves a copy behind.
pub struct Secret(crate::memlock::Locked);

impl Secret {
    fn new() -> Self {
        Self(crate::memlock::Locked::with_capacity(MAX_LINE))
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.0.capacity()
    }
}

/// The controlling terminal, opened for reading and writing.
///
/// # Errors
///
/// This process has no controlling terminal.
pub fn open() -> Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("no controlling terminal (/dev/tty does not open)")
}

/// Turn echo off on `tty`, which drops anything typed ahead, write
/// `prompt`, read one line, and end it. `None` when the terminal closed
/// before a line ended.
///
/// # Errors
///
/// The terminal's settings cannot be read or set, a write or a read
/// failed, or the line is longer than [`MAX_LINE`].
pub fn ask(tty: &mut std::fs::File, prompt: &str) -> Result<Option<Secret>> {
    let line = {
        let _quiet = Quiet::enter(tty.as_raw_fd())?;
        tty.write_all(prompt.as_bytes())?;
        tty.flush()?;
        read_line(tty.as_raw_fd())
    };
    let _ = tty.write_all(b"\n");
    line
}

fn read_line(fd: libc::c_int) -> Result<Option<Secret>> {
    let mut line = Secret::new();
    loop {
        let mut byte = 0u8;
        // SAFETY: one byte into a local the call may write.
        let n = unsafe { libc::read(fd, std::ptr::addr_of_mut!(byte).cast(), 1) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            // A pty whose other side closed reads EIO.
            if err.raw_os_error() == Some(libc::EIO) {
                return Ok(None);
            }
            return Err(err).context("reading the terminal");
        }
        if n == 0 {
            return Ok(None);
        }
        if byte == b'\n' || byte == b'\r' {
            return Ok(Some(line));
        }
        if !line.0.push(byte) {
            bail!("the line is longer than {MAX_LINE} bytes");
        }
    }
}

/// The settings to put back, readable from a signal handler.
struct Saved(UnsafeCell<MaybeUninit<libc::termios>>);

// SAFETY: written only while `RESTORE_FD` is -1, before it is published,
// and read only after a handler or `Quiet::drop` takes the fd.
unsafe impl Sync for Saved {}

static SAVED: Saved = Saved(UnsafeCell::new(MaybeUninit::uninit()));

/// The terminal a handler restores, or -1.
static RESTORE_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn restore_and_die(sig: libc::c_int) {
    let fd = RESTORE_FD.swap(-1, Ordering::SeqCst);
    // SAFETY: tcsetattr, signal and raise are async-signal-safe, and SAVED
    // was written before fd was published.
    unsafe {
        if fd >= 0 {
            libc::tcsetattr(fd, libc::TCSANOW, (*SAVED.0.get()).as_ptr());
        }
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// Echo off on one terminal until dropped, with the fatal signals set to
/// restore it and the stop signals held.
struct Quiet {
    fd: libc::c_int,
    saved: libc::termios,
    actions: Vec<(libc::c_int, libc::sigaction)>,
    mask: libc::sigset_t,
}

impl Quiet {
    fn enter(fd: libc::c_int) -> Result<Self> {
        // SAFETY: termios and sigset_t are plain data; every call below gets
        // valid pointers to locals or to SAVED, which no one else writes
        // while RESTORE_FD is -1.
        unsafe {
            let mut saved: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut saved) != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("reading the terminal's settings");
            }
            let mut held: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut held);
            for &sig in STOPS {
                libc::sigaddset(&mut held, sig);
            }
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_BLOCK, &held, &mut mask);
            (*SAVED.0.get()).write(saved);
            RESTORE_FD.store(fd, Ordering::SeqCst);
            let mut actions = Vec::with_capacity(FATAL.len());
            for &sig in FATAL {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = restore_and_die as extern "C" fn(libc::c_int) as usize;
                libc::sigemptyset(&mut action.sa_mask);
                let mut old: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(sig, &action, &mut old) == 0 {
                    actions.push((sig, old));
                }
            }
            let quiet = Self {
                fd,
                saved,
                actions,
                mask,
            };
            let mut off = saved;
            off.c_lflag &= !(libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
            // TCSANOW, not TCSAFLUSH. On macOS a flush waits until
            // output drains, and a pty with no session never drains,
            // so the prompt hangs. Type-ahead is dropped afterwards.
            if libc::tcsetattr(fd, libc::TCSANOW, &off) != 0 {
                return Err(std::io::Error::last_os_error()).context("turning echo off");
            }
            if libc::tcflush(fd, libc::TCIFLUSH) != 0 {
                return Err(std::io::Error::last_os_error()).context("dropping type-ahead");
            }
            Ok(quiet)
        }
    }
}

impl Drop for Quiet {
    fn drop(&mut self) {
        // SAFETY: the same terminal and the settings read from it; the
        // actions and the mask are the ones `enter` replaced.
        unsafe {
            RESTORE_FD.store(-1, Ordering::SeqCst);
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.saved);
            for (sig, old) in &self.actions {
                libc::sigaction(*sig, old, std::ptr::null_mut());
            }
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.mask, std::ptr::null_mut());
        }
    }
}
