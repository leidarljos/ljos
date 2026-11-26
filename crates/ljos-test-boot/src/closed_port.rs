//! A loopback port this process owns and does not listen on.
//!
//! Binding `127.0.0.1:0` and dropping the socket gives the port back to
//! the kernel. Another test can bind it before the check that called the
//! port closed. [`ClosedPort`] keeps the socket until it is dropped.
//! `SO_REUSEADDR` stays off, so a later bind of the same port fails, and
//! nothing is listening, so a connect is refused.

use std::io;
use std::os::fd::RawFd;

/// A `127.0.0.1` port this process holds closed.
#[must_use = "dropping this releases the port"]
pub struct ClosedPort {
    fd: RawFd,
    port: u16,
}

impl ClosedPort {
    /// Bind `127.0.0.1:0` and keep the socket.
    ///
    /// # Errors
    ///
    /// The kernel refuses the socket or the bind.
    pub fn bind() -> io::Result<Self> {
        // SAFETY: the address pointer is live only for bind and
        // getsockname. `fd` is closed in Drop and nowhere else.
        unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
            if fd < 0 {
                return Err(io::Error::last_os_error());
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

    /// The descriptor a child of this process keeps across `exec`.
    ///
    /// Close-on-exec is cleared. The number is the same in the child.
    /// The socket is still bound and not listening, so the child can
    /// `listen` on it without binding again.
    ///
    /// # Errors
    ///
    /// The kernel refuses the flag change.
    pub fn inherit(&self) -> io::Result<i32> {
        // SAFETY: `fd` is the socket this value owns. The calls read and
        // replace that descriptor's flags.
        unsafe {
            let flags = libc::fcntl(self.fd, libc::F_GETFD);
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::fcntl(self.fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(self.fd)
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
        let refused = std::net::TcpStream::connect(("127.0.0.1", held.port())).unwrap_err();
        assert_eq!(
            refused.raw_os_error(),
            Some(libc::ECONNREFUSED),
            "{refused}"
        );
        let taken = std::net::TcpListener::bind(("127.0.0.1", held.port())).unwrap_err();
        assert_eq!(taken.raw_os_error(), Some(libc::EADDRINUSE), "{taken}");
    }
}
