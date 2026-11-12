//! `ljos-mcp --version` and `--help` print and exit before the server,
//! a pack connection, or a read of stdin. A wrapper in mcp.json that
//! sleeps must not be started.

ljos_test_boot::harness!();

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn run(flag: &str) -> (bool, String, Duration) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".cursor")).unwrap();
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
    let out = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .arg(flag)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    (out.status.success(), format!("{stdout}{stderr}"), took)
}

#[test]
fn version_exits_with_stdin_closed() {
    let (ok, said, took) = run("--version");
    assert!(
        took < Duration::from_secs(2),
        "--version took {took:?}:\n{said}"
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("ljos-mcp "),
        "the version line was missing:\n{said}"
    );
}

#[test]
fn help_exits_with_stdin_closed() {
    let (ok, said, took) = run("--help");
    assert!(
        took < Duration::from_secs(2),
        "--help took {took:?}:\n{said}"
    );
    assert!(ok, "{said}");
    assert!(said.contains("ljos-mcp "), "{said}");
    assert!(said.contains("--version"), "{said}");
}
