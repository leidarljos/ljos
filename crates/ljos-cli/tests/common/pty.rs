//! A fresh pseudo-terminal that a child takes as its controlling
//! terminal, so `/dev/tty` in that child is this pty whether or not the
//! test host has a terminal. The test writes what the person types to
//! the master side and reads what the child shows there.

use std::io::Read as _;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Pty {
    master: std::fs::File,
    slave: Option<OwnedFd>,
    shown: Arc<Mutex<Vec<u8>>>,
}

impl Pty {
    #[must_use]
    pub fn open() -> Self {
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: openpty writes two descriptors into the locals; the name,
        // termios and winsize arguments may be null.
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(rc, 0, "openpty: {}", std::io::Error::last_os_error());
        // SAFETY: both descriptors are fresh and owned here alone.
        let (master, slave) = unsafe {
            libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
            (
                std::fs::File::from_raw_fd(master),
                OwnedFd::from_raw_fd(slave),
            )
        };
        Self {
            master,
            slave: Some(slave),
            shown: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Start `cmd` in a new session whose controlling terminal is this
    /// pty. Its stdin, stdout and stderr are whatever `cmd` sets; the
    /// slave side is not one of them unless the test asks.
    pub fn spawn(&mut self, cmd: &mut Command) -> Child {
        let slave = self.slave.take().expect("one child per pty");
        let fd = slave.as_raw_fd();
        // SAFETY: setsid, ioctl and close are async-signal-safe and touch
        // only this child.
        unsafe {
            cmd.pre_exec(move || {
                if libc::setsid() < 0 || libc::ioctl(fd, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::close(fd);
                Ok(())
            });
        }
        let child = cmd.spawn().expect("spawn on a pty");
        drop(slave);
        let mut master = self.master.try_clone().unwrap();
        let shown = Arc::clone(&self.shown);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = master.read(&mut buf) {
                if n == 0 {
                    break;
                }
                shown.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        child
    }

    /// What the child wrote to the terminal so far.
    #[must_use]
    pub fn shown(&self) -> String {
        String::from_utf8_lossy(&self.shown.lock().unwrap()).into_owned()
    }
}

/// Wait for `child` to exit, killing it after `secs` seconds.
pub fn wait_output(child: Child, secs: u64) -> std::process::Output {
    let pid = child.id();
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&done);
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if flag.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // SAFETY: a signal to a child this test started.
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    });
    let out = child.wait_with_output().unwrap();
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    out
}
