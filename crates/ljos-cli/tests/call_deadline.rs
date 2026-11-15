//! A sibling that sleeps is killed, and the verb says which part stalled.

ljos_test_boot::harness!();

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_sleeping_consensus_is_killed_and_named() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let pidfile = dir.path().join("sleep.pid");
    script(
        dir.path(),
        "vissue",
        "#!/bin/sh\ncase \"$1\" in\n  vote) echo '[{\"agent\":\"ada\",\"choice\":\"ship\"}]' ;;\n  show) echo '{\"id\":\"demo-q\",\"title\":\"ship\"}' ;;\n  children) echo '[]' ;;\n  *) exit 1 ;;\nesac\n",
    );
    let body = format!(
        "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nwait\n",
        pidfile.display()
    );
    script(dir.path(), "ljos-consensus", &body);
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
    );
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["consensus", "demo-q", "--no-file"])
        .current_dir(dir.path())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", "http://127.0.0.1:9")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_UNSIGNED_BALLOTS", "allow")
        .env("LJOS_CALL_TIMEOUT", "4")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        took < Duration::from_secs(8),
        "consensus waited {took:?}\n{said}"
    );
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("waiting on consensus..."), "{said}");
    assert!(
        said.contains("consensus did not answer in 4s; nothing was written"),
        "{said}"
    );
    let pid = std::fs::read_to_string(&pidfile).unwrap_or_default();
    assert!(!pid.trim().is_empty(), "the sleeper did not start\n{said}");
    let alive = Command::new("kill")
        .args(["-0", pid.trim()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(
        !alive.success(),
        "grandchild {} still running\n{said}",
        pid.trim()
    );
}
