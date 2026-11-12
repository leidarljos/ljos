//! A doctor probe that does not exit is one row. It does not hold the
//! report. The hanging command is the mcp.json wrapper's `--version`.

ljos_test_boot::harness!();

use std::process::Stdio;
use std::time::{Duration, Instant};

#[test]
fn doctor_reports_a_hanging_version_probe_and_finishes() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".cursor")).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let wrapper = bin.join("ljos-mcp");
    std::fs::write(&wrapper, "#!/bin/sh\n# exec /no/such/ljos-mcp\nsleep 30\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(
        home.join(".cursor/mcp.json"),
        r#"{"mcpServers":{"ljos":{"command":"ljos-mcp"}}}"#,
    )
    .unwrap();
    let started = Instant::now();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("PACKSET_URL", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(
        took < Duration::from_secs(12),
        "doctor held for {took:?} on a hanging probe:\n{said}"
    );
    assert!(
        said.contains("timed out"),
        "doctor did not name the probe that hung:\n{said}"
    );
    assert!(!stdout.trim().is_empty(), "doctor printed nothing:\n{said}");
}
