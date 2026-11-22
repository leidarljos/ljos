//! `ljos doctor` on a fresh machine starts the default writer.

ljos_test_boot::harness!();
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "common/seat_root.rs"]
mod seat_root;

/// A seat home outside the temp roots. A temp HOME is a sandbox and the
/// daemon stays out of those, so this proof cannot live in one.
struct SeatHome(PathBuf);

impl SeatHome {
    fn new(name: &str) -> Self {
        let dir = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp)
            .join(format!("ljos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("seat home");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SeatHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn doctor_runs_packset_ensure_when_no_writer_answers() {
    let home = SeatHome::new("doctor-wake");
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let marker = home.path().join("ensure-called");
    let fake = bin.join("packset");
    ljos_cli::install_executable(
        &fake,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\n", marker.display()),
    )
    .expect("fake packset");
    // A port nothing listens on once the probe closes.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("probe")
        .local_addr()
        .expect("addr")
        .port();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let _ = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("PATH", path)
        .env("PACKSET_PORT", port.to_string())
        .env("PACKSET_TIMEOUT_MS", "300")
        .env("LJOS_NO_AUTOSTART", "1")
        .output()
        .expect("ljos doctor");
    let calls = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        calls.lines().any(|l| l.trim() == "ensure"),
        "doctor did not run `packset ensure`; calls: {calls:?}"
    );
}
