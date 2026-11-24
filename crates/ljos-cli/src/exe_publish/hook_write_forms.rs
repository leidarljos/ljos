//! Hook writes a text scan of call syntax missed, each as a gate wrote it.
//!
//! Every statement here that writes or starts a program carries
//! `#[expect(clippy::disallowed_methods, reason = "..")]`, with a reason
//! that names its form. Clippy resolves the call to the function it names,
//! so a renamed import, a typed or cast function value, a closure, a
//! builder made in another function and a macro body are all the call they
//! resolve to. If clippy stops naming one, its expectation is unfulfilled
//! and `cargo clippy -- -D warnings` fails.
//! The calls are probes: reverting a denylist entry makes its expectation
//! unfulfilled, which proves the gate notices the revert.
#![allow(dead_code)]

use std::fs::OpenOptions;
use std::io::Result;
use std::path::Path;
use std::process::Command;

mod renamed_libc {
    use libc::symlink as sl;
    use libc::{link as ln, rename as mv};

    fn install(hook: &std::ffi::CStr) {
        let src = c"/bin/true";
        #[expect(
            clippy::disallowed_methods,
            reason = "fixture: libc::symlink imported as sl"
        )]
        unsafe {
            sl(src.as_ptr(), hook.as_ptr())
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "fixture: libc::rename imported as mv"
        )]
        unsafe {
            mv(src.as_ptr(), hook.as_ptr())
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "fixture: libc::link imported as ln"
        )]
        unsafe {
            ln(src.as_ptr(), hook.as_ptr())
        };
    }

    fn flags_from_a_value(dir: libc::c_int, hook: &std::ffi::CStr) {
        let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
        #[expect(
            clippy::disallowed_methods,
            reason = "fixture: libc::openat with O_CREAT and O_TRUNC passed through a flags binding"
        )]
        let fd = unsafe { libc::openat(dir, hook.as_ptr(), flags, 0o755) };
        let _ = fd;
    }
}

#[cfg(unix)]
fn rustix_paths(src: &Path, hook: &Path) {
    use rustix::fs::{AtFlags, CWD};

    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: rustix::fs::symlink onto a hook path"
    )]
    rustix::fs::symlink(src, hook).ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: rustix::fs::symlinkat onto a hook path"
    )]
    rustix::fs::symlinkat(src, CWD, hook).ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: rustix::fs::rename onto a hook path"
    )]
    rustix::fs::rename(src, hook).ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: rustix::fs::link onto a hook path"
    )]
    rustix::fs::link(src, hook).ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: rustix::fs::linkat onto a hook path"
    )]
    rustix::fs::linkat(CWD, src, CWD, hook, AtFlags::empty()).ok();
    #[cfg(any(
        target_os = "android",
        target_os = "ios",
        target_os = "linux",
        target_os = "macos",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos"
    ))]
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: rustix::fs::renameat_with onto a hook path"
    )]
    rustix::fs::renameat_with(CWD, src, CWD, hook, rustix::fs::RenameFlags::NOREPLACE).ok();
}

fn typed_let_and_cast<'a>(hook: &'a Path, src: &'a Path) {
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: std::fs::rename bound to a let with a fn pointer type"
    )]
    let f: fn(&'a Path, &'a Path) -> Result<()> = std::fs::rename;
    f(src, hook).ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: std::fs::rename cast to a fn pointer"
    )]
    let g = std::fs::rename as fn(&'a Path, &'a Path) -> Result<()>;
    g(src, hook).ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: std::fs::rename named with turbofish type arguments"
    )]
    let h = std::fs::rename::<&Path, &Path>;
    h(src, hook).ok();
}

fn writable() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    options
}

fn split_builder(dir: &Path) {
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: OpenOptions::open on a writable builder made in another function"
    )]
    writable().open(dir.join("pre-push")).ok();
}

fn read_only_with_flags(dir: &Path) {
    use std::os::unix::fs::OpenOptionsExt;
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: OpenOptions::open read-only with O_TRUNC in custom_flags"
    )]
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_TRUNC)
        .open(dir.join("pre-push"))
        .ok();
}

const CP: &str = "/bin/cp";

fn program_not_a_literal(dir: &Path) {
    let bin = "cp";
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: Command::new with cp kept in a local binding"
    )]
    Command::new(bin)
        .arg("/bin/true")
        .arg(dir.join("pre-push"))
        .status()
        .ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: Command::new with cp kept in a const"
    )]
    Command::new(CP)
        .arg("/bin/true")
        .arg(dir.join("pre-push"))
        .status()
        .ok();
}

fn cp() -> Command {
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: Command::new with a literal cp, returned from a helper"
    )]
    let cp = Command::new("cp");
    cp
}

fn closure(dir: &Path) {
    let hook = dir.join("pre-push");
    let go = |a: &Path, b: &Path| {
        #[expect(
            clippy::disallowed_methods,
            reason = "fixture: std::fs::rename called inside a closure"
        )]
        std::fs::rename(a, b).ok();
    };
    go(Path::new("/bin/true"), &hook);
}

macro_rules! put {
    ($p:expr) => {
        std::fs::write($p, b"#!/bin/sh\n").ok()
    };
}
pub(crate) use put;

macro_rules! outer {
    ($p:expr) => {
        put!($p)
    };
}

fn path_qualified_macro(dir: &Path) {
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: std::fs::write in a macro called through self::"
    )]
    let _ = self::put!(dir.join("pre-push"));
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: std::fs::write in a macro called through its crate path"
    )]
    let _ = crate::exe_publish::hook_write_forms::put!(dir.join("pre-push"));
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: std::fs::write in a macro reached through another macro"
    )]
    let _ = outer!(dir.join("post-commit"));
}

fn aliases(dir: &Path) {
    use std::fs::{File as F, OpenOptions as Oo};
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: OpenOptions::open through OpenOptions imported as Oo"
    )]
    Oo::new().write(true).open(dir.join("pre-push")).ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: File::options().open through File imported as F"
    )]
    F::options().append(true).open(dir.join("pre-commit")).ok();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: File::create through File imported as F"
    )]
    F::create(dir.join("commit-msg")).ok();
}

fn function_value(dir: &Path) {
    fn apply(f: fn(std::path::PathBuf, &'static [u8]) -> Result<()>, path: std::path::PathBuf) {
        f(path, b"#!/bin/sh\n").ok();
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: std::fs::write passed as a fn value to a helper"
    )]
    apply(std::fs::write, dir.join("post-commit"));
}

fn persisted(dir: &Path) {
    let tmp = tempfile::NamedTempFile::new_in(dir).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "fixture: NamedTempFile::persist onto a hook path"
    )]
    tmp.persist(dir.join("pre-push")).ok();
}
