//! `ljos doctor` on a fresh machine starts the default writer.

use std::net::TcpListener;
use std::process::Command;

#[test]
fn doctor_runs_packset_ensure_when_no_writer_answers() {
    let home = tempfile::tempdir().expect("home");
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
    let _ = Command::new(env!("CARGO_BIN_EXE_ljos"))
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
