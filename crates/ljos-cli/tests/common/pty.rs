//! A fresh pseudo-terminal that a child takes as its controlling
//! terminal, so `/dev/tty` in that child is this pty whether or not the
//! test host has a terminal. The test writes what the person types to
//! the master side and reads what the child shows there.

use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub struct Pty {
    master: std::fs::File,
    /// Held open here so the master never reads EIO while the child is
    /// between closing its copy and opening `/dev/tty`.
    slave: Mutex<Option<OwnedFd>>,
    shown: Arc<Mutex<Vec<u8>>>,
    reader_done: Arc<(Mutex<bool>, Condvar)>,
    reader: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[allow(dead_code)]
impl Pty {
    #[must_use]
    #[allow(
        clippy::disallowed_methods,
        reason = "owns the two descriptors openpty made"
    )]
    pub fn open() -> Self {
        let (mut master, mut slave) = (-1, -1);
        #[cfg(target_os = "macos")]
        let (termios, winsize) = (std::ptr::null_mut(), std::ptr::null_mut());
        #[cfg(not(target_os = "macos"))]
        let (termios, winsize) = (std::ptr::null(), std::ptr::null());
        // SAFETY: openpty writes two descriptors into the locals; the name,
        // termios and winsize arguments may be null.
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                termios,
                winsize,
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
            slave: Mutex::new(Some(slave)),
            shown: Arc::new(Mutex::new(Vec::new())),
            reader_done: Arc::new((Mutex::new(false), Condvar::new())),
            reader: Mutex::new(None),
        }
    }

    /// Point `cmd`'s stdout and stderr at this terminal, so `is_terminal`
    /// is true and the text is what a person at a prompt sees. Call this
    /// before [`Self::spawn`].
    pub fn attach_output(&self, cmd: &mut Command) {
        let guard = self.slave.lock().unwrap();
        let fd = guard.as_ref().expect("pty slave open").as_raw_fd();
        cmd.stdout(Self::stdio_dup(fd));
        cmd.stderr(Self::stdio_dup(fd));
    }

    /// Point only `cmd`'s stdout at this terminal. Call this before
    /// [`Self::spawn`].
    pub fn attach_stdout(&self, cmd: &mut Command) {
        let guard = self.slave.lock().unwrap();
        let fd = guard.as_ref().expect("pty slave open").as_raw_fd();
        cmd.stdout(Self::stdio_dup(fd));
    }

    /// Point only `cmd`'s stderr at this terminal. Call this before
    /// [`Self::spawn`].
    pub fn attach_stderr(&self, cmd: &mut Command) {
        let guard = self.slave.lock().unwrap();
        let fd = guard.as_ref().expect("pty slave open").as_raw_fd();
        cmd.stderr(Self::stdio_dup(fd));
    }

    /// Point `cmd`'s stdin at this terminal, so `is_terminal` is true.
    /// Call this before [`Self::spawn`].
    pub fn attach_stdin(&self, cmd: &mut Command) {
        let guard = self.slave.lock().unwrap();
        let fd = guard.as_ref().expect("pty slave open").as_raw_fd();
        cmd.stdin(Self::stdio_dup(fd));
    }

    /// Set terminal size so TIOCGWINSZ in the child sees these dimensions.
    pub fn set_winsize(&self, cols: u16, rows: u16) {
        // SAFETY: ioctl writes the provided winsize to the open pty master.
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            ws.ws_col = cols;
            ws.ws_row = rows;
            let rc = libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &ws);
            assert_eq!(rc, 0, "TIOCSWINSZ: {}", std::io::Error::last_os_error());
        }
    }

    /// A copy of `fd` owned by a `Stdio`. The original stays open.
    #[allow(
        clippy::disallowed_methods,
        reason = "owns the descriptor dup made, which the child gets"
    )]
    fn stdio_dup(fd: i32) -> Stdio {
        // SAFETY: `fd` is open. `dup` returns a new descriptor, which
        // `Stdio` owns and closes.
        let duped = unsafe { libc::dup(fd) };
        assert!(duped >= 0, "dup: {}", std::io::Error::last_os_error());
        unsafe { Stdio::from_raw_fd(duped) }
    }

    /// Start `cmd` in a new session whose controlling terminal is this
    /// pty. Its stdin, stdout and stderr are whatever `cmd` sets; the
    /// slave side is not one of them unless the test asks.
    pub fn spawn(&self, cmd: &mut Command) -> Child {
        let fd = self
            .slave
            .lock()
            .unwrap()
            .as_ref()
            .expect("pty slave open")
            .as_raw_fd();
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
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut master = self.master.try_clone().unwrap();
        let shown = Arc::clone(&self.shown);
        let reader_done = Arc::clone(&self.reader_done);
        let handle = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match master.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => shown.lock().unwrap().extend_from_slice(&buf[..n]),
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let (lock, cvar) = &*reader_done;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
        });
        *self.reader.lock().unwrap() = Some(handle);
        child
    }

    /// Close the parent's slave handle so the reader hits EOF once the
    /// child and its descendants close theirs, then wait until that
    /// reader has finished. A quiet gap is not the end of the screen.
    /// Thirty seconds is a hang backstop for a reader that never sees EOF.
    pub fn finish_reading(&self) {
        *self.slave.lock().unwrap() = None;
        let (lock, cvar) = &*self.reader_done;
        let guard = lock.lock().unwrap();
        let (guard, _) = cvar
            .wait_timeout_while(guard, Duration::from_secs(30), |done| !*done)
            .unwrap();
        let finished = *guard;
        drop(guard);
        if finished {
            if let Some(handle) = self.reader.lock().unwrap().take() {
                let _ = handle.join();
            }
        }
    }

    /// Wait for `child` to exit. When stdout or stderr use this pty,
    /// call [`Self::finish_reading`] before reading [`Self::shown`].
    pub fn wait_output(&self, child: Child, secs: u64) -> std::process::Output {
        wait_output(child, secs)
    }

    /// What the child wrote to the terminal so far.
    #[must_use]
    pub fn shown(&self) -> String {
        String::from_utf8_lossy(&self.shown.lock().unwrap()).into_owned()
    }

    /// Wait until the terminal shows `text`. False after `secs` seconds.
    #[must_use]
    pub fn wait_for(&self, text: &str, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if self.shown().contains(text) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Type `text` on the terminal, as the person would.
    pub fn type_line(&mut self, text: &str) {
        self.master.write_all(text.as_bytes()).unwrap();
        self.master.write_all(b"\n").unwrap();
        self.master.flush().unwrap();
    }

    /// End input with the terminal's end-of-file key on an empty line.
    pub fn send_eof(&mut self) {
        self.master.write_all(b"\x04").unwrap();
        self.master.flush().unwrap();
    }

    /// Press the terminal's interrupt key, which signals the foreground group.
    pub fn interrupt(&mut self) {
        self.master.write_all(b"\x03").unwrap();
        self.master.flush().unwrap();
    }

    /// Whether the terminal echoes what is typed. The master side reads
    /// the settings the child set on its side.
    #[must_use]
    pub fn echo_on(&self) -> bool {
        // SAFETY: termios is plain data, filled in from the pty master.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(self.master.as_raw_fd(), &mut t), 0);
            t.c_lflag & libc::ECHO != 0
        }
    }
}

/// Wait for `child` to exit, killing it after `secs` seconds.
fn wait_output(child: Child, secs: u64) -> std::process::Output {
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
