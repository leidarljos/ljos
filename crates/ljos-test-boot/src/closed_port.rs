//! A loopback port this process owns and does not listen on.
//!
//! Binding `127.0.0.1:0` and dropping the socket gives the port back to
//! the kernel. Another test can bind it before the check that called the
//! port closed. [`ClosedPort`] keeps the socket until it is dropped.
//! `SO_REUSEADDR` stays off, so a later bind of the same port fails.
//! Nothing is listening. On Linux the kernel resets the connect, which
//! is `ECONNREFUSED`. On other Unixes a bound socket that is not
//! listening drops the SYN, so the connect waits out its own timeout
//! and is `ETIMEDOUT` or `ECONNREFUSED`. The pack connect reports that
//! loopback timeout as a refusal after its 800 ms probe. The socket
//! stays bound and not listening, so a child can `listen` on the
//! inherited descriptor. The tests check that the connect fails fast,
//! and a second bind does not succeed.

use std::io;
use std::os::fd::RawFd;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// `SOCK_CLOEXEC` on Linux and Android, where `socket` applies it.
/// Elsewhere zero, and the caller sets `FD_CLOEXEC` with `fcntl`.
fn cloexec_flag() -> libc::c_int {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        libc::SOCK_CLOEXEC
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        0
    }
}

/// A `127.0.0.1` port this process holds closed.
#[must_use = "dropping this releases the port"]
pub struct ClosedPort {
    fd: RawFd,
    port: u16,
}

impl ClosedPort {
    /// Bind `127.0.0.1:0` and keep the socket.
    ///
    /// The descriptor is close-on-exec. [`Self::inherit`] clears that
    /// flag in the one child that should listen on it.
    ///
    /// # Errors
    ///
    /// The kernel refuses the socket or the bind.
    pub fn bind() -> io::Result<Self> {
        // SAFETY: the address pointer is live only for bind and
        // getsockname. `fd` is closed in Drop and nowhere else.
        unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | cloexec_flag(), 0);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            if cloexec_flag() == 0 {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
                    let err = io::Error::last_os_error();
                    libc::close(fd);
                    return Err(err);
                }
            }
            let mut addr: libc::sockaddr_in = std::mem::zeroed();
            addr.sin_family = libc::AF_INET as libc::sa_family_t;
            addr.sin_port = 0;
            addr.sin_addr = libc::in_addr {
                s_addr: libc::htonl(libc::INADDR_LOOPBACK),
            };
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "watchos",
            ))]
            {
                addr.sin_len = u8::try_from(std::mem::size_of::<libc::sockaddr_in>()).unwrap_or(16);
            }
            let len = libc::socklen_t::try_from(std::mem::size_of::<libc::sockaddr_in>())
                .unwrap_or(libc::socklen_t::MAX);
            if libc::bind(fd, (&addr as *const libc::sockaddr_in).cast(), len) != 0 {
                let err = io::Error::last_os_error();
                libc::close(fd);
                return Err(err);
            }
            let mut got = len;
            if libc::getsockname(fd, (&mut addr as *mut libc::sockaddr_in).cast(), &mut got) != 0 {
                let err = io::Error::last_os_error();
                libc::close(fd);
                return Err(err);
            }
            let port = libc::ntohs(addr.sin_port);
            if port == 0 {
                libc::close(fd);
                return Err(io::Error::other("bind returned port 0"));
            }
            Ok(Self { fd, port })
        }
    }

    /// The port the kernel assigned.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:<port>`.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The descriptor `cmd`'s child keeps across `exec`.
    ///
    /// Close-on-exec stays set on this process's descriptor. It is
    /// cleared in that child only, after fork and before exec, so a
    /// later child of this process does not inherit the socket. The
    /// number is the same in the child. The socket is still bound and
    /// not listening, so the child can `listen` on it without binding
    /// again.
    pub fn inherit(&self, cmd: &mut Command) -> i32 {
        let fd = self.fd;
        // SAFETY: `pre_exec` runs in the forked child before exec. The
        // closure only changes this socket's close-on-exec flag in that
        // child. `fd` stays open until this value is dropped.
        unsafe {
            cmd.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        fd
    }
}

impl Drop for ClosedPort {
    fn drop(&mut self) {
        if self.fd >= 0 {
            // SAFETY: `fd` is the socket `bind` opened. Drop is the only close.
            unsafe {
                libc::close(self.fd);
            }
            self.fd = -1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClosedPort;

    #[test]
    fn a_held_port_refuses_connect_and_a_second_bind() {
        let held = ClosedPort::bind().unwrap();
        assert_connect_fails(held.port());
        let taken = std::net::TcpListener::bind(("127.0.0.1", held.port())).unwrap_err();
        assert_eq!(taken.raw_os_error(), Some(libc::EADDRINUSE), "{taken}");
    }

    #[test]
    fn bind_is_close_on_exec_and_inherit_clears_it_for_one_child() {
        let held = ClosedPort::bind().unwrap();
        assert_ne!(fd_flags(held.fd) & libc::FD_CLOEXEC, 0);
        #[cfg(target_os = "linux")]
        assert!(!child_has_fd(held.fd), "exec kept the socket");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = std::process::Command::new("/bin/sh");
        let script = format!("test -e /dev/fd/{}", held.fd);
        cmd.args(["-c", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let fd = held.inherit(&mut cmd);
        assert_eq!(fd, held.fd);
        assert_ne!(fd_flags(held.fd) & libc::FD_CLOEXEC, 0);
        #[cfg(target_os = "linux")]
        {
            let status = cmd.status().unwrap();
            assert!(status.success(), "the armed child did not keep the socket");
            assert!(!child_has_fd(held.fd), "a later child inherited the socket");
        }
        assert_connect_fails(held.port());
    }

    /// A connect that stops on its own, short of the kernel's dropped-SYN
    /// wait, and inside a second. Linux still has to be `ECONNREFUSED`.
    /// Other Unixes may be that or `ETIMEDOUT`.
    fn assert_connect_fails(port: u16) {
        let started = std::time::Instant::now();
        let refused = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_millis(200),
        )
        .expect_err("a connect to a closed port should fail");
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "connect to a held port took {elapsed:?}"
        );
        let reset = refused.kind() == std::io::ErrorKind::ConnectionRefused
            || refused.raw_os_error() == Some(libc::ECONNREFUSED);
        #[cfg(target_os = "linux")]
        assert!(reset, "{refused}");
        #[cfg(not(target_os = "linux"))]
        {
            let timed_out = refused.kind() == std::io::ErrorKind::TimedOut
                || refused.raw_os_error() == Some(libc::ETIMEDOUT);
            assert!(reset || timed_out, "{refused}");
        }
    }

    fn fd_flags(fd: i32) -> i32 {
        // SAFETY: `fd` is the open socket this test owns.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0, "{}", std::io::Error::last_os_error());
        flags
    }

    #[cfg(target_os = "linux")]
    fn child_has_fd(fd: i32) -> bool {
        let script = format!("test -e /dev/fd/{fd}");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        status.success()
    }
}
