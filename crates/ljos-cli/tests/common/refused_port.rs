//! A port this process holds and does not listen on.
//!
//! A connect is refused, and nothing else can bind the port. The kernel's
//! ephemeral range does not include the default pack port.

use std::os::raw::{c_int, c_void};

const AF_INET: c_int = 2;
const SOCK_STREAM: c_int = 1;

#[cfg(target_os = "linux")]
#[repr(C)]
struct SockAddr {
    family: u16,
    port: u16,
    addr: u32,
    zero: [u8; 8],
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct SockAddr {
    len: u8,
    family: u8,
    port: u16,
    addr: u32,
    zero: [u8; 8],
}

extern "C" {
    fn socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, addr: *const c_void, len: u32) -> c_int;
    fn getsockname(fd: c_int, addr: *mut c_void, len: *mut u32) -> c_int;
    fn close(fd: c_int) -> c_int;
}

pub struct RefusedPort {
    fd: c_int,
    port: u16,
}

impl RefusedPort {
    pub fn reserve() -> Self {
        // SAFETY: the address is a sockaddr_in for 127.0.0.1 and an
        // ephemeral port. The descriptor stays open until drop, which is
        // what keeps the port. Nothing is listening, so a connect is refused.
        unsafe {
            let fd = socket(AF_INET, SOCK_STREAM, 0);
            assert!(fd >= 0, "socket: {}", last_err());
            let mut addr = empty_addr();
            let len = u32::try_from(std::mem::size_of::<SockAddr>()).unwrap();
            if bind(fd, &addr as *const SockAddr as *const c_void, len) != 0 {
                let err = last_err();
                close(fd);
                panic!("bind: {err}");
            }
            let mut got = len;
            if getsockname(fd, &mut addr as *mut SockAddr as *mut c_void, &mut got) != 0 {
                let err = last_err();
                close(fd);
                panic!("getsockname: {err}");
            }
            let port = u16::from_be(addr.port);
            assert_ne!(port, 0, "the kernel did not assign a port");
            assert_ne!(
                port, 8761,
                "a down-writer test does not use the default port"
            );
            Self { fd, port }
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for RefusedPort {
    fn drop(&mut self) {
        // SAFETY: `fd` is the socket `reserve` opened.
        unsafe {
            close(self.fd);
        }
    }
}

#[cfg(target_os = "linux")]
fn empty_addr() -> SockAddr {
    SockAddr {
        family: AF_INET as u16,
        port: 0,
        addr: u32::from_ne_bytes([127, 0, 0, 1]),
        zero: [0; 8],
    }
}

#[cfg(target_os = "macos")]
fn empty_addr() -> SockAddr {
    SockAddr {
        len: u8::try_from(std::mem::size_of::<SockAddr>()).unwrap(),
        family: AF_INET as u8,
        port: 0,
        addr: u32::from_ne_bytes([127, 0, 0, 1]),
        zero: [0; 8],
    }
}

fn last_err() -> String {
    std::io::Error::last_os_error().to_string()
}
