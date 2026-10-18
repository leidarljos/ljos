//! `ljos doctor` on a fresh machine starts the default writer.

use std::net::TcpListener;
use std::path::{Path, PathBuf};

/// A seat home outside the temp roots. A temp HOME is a sandbox and the
/// daemon stays out of those, so this proof cannot live in one.
struct SeatHome(PathBuf);

impl SeatHome {
    fn new(name: &str) -> Self {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
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
    std::fs::write(
        &fake,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\n", marker.display()),
    )
    .expect("fake packset");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    // A port nothing listens on once the probe closes.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("probe")
        .local_addr()
        .expect("addr")
        .port();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let _ = ljos_cli::process_env::command(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env_clear()
        .env("HOME", home.path())
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
