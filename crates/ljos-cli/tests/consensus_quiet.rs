//! `ljos consensus --no-file` asks the pack more than once. A socket that
//! accepts and stays silent must not be waited out on every ask. `timeout`
//! is the harness: before the stop, the waits add up and the harness cuts
//! the run off.

ljos_test_boot::harness!();

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Accepts and reads one request, then stays silent.
fn silent_pack() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            std::thread::sleep(Duration::from_secs(60));
        }
    });
    format!("http://{addr}")
}

#[test]
fn a_quiet_pack_does_not_hold_consensus_for_every_read() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    script(
        dir.path(),
        "vissue",
        "#!/bin/sh\ncase \"$1\" in\n  vote) echo '[{\"agent\":\"ada\",\"choice\":\"ship\"}]' ;;\n  show) echo '{\"id\":\"demo-q\",\"title\":\"ship\"}' ;;\n  children) echo '[]' ;;\n  *) exit 1 ;;\nesac\n",
    );
    script(
        dir.path(),
        "ljos-consensus",
        "#!/bin/sh\necho '{\"options\":[\"ship\"],\"shares\":[1.0],\"settled\":true,\"converged\":true,\"tie\":false,\"agents\":[\"ada\"],\"influence\":[1.0],\"engine\":\"degroot\"}'\n",
    );
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let pack = silent_pack();
    // One read is two seconds. Five reads are ten. The harness is eight,
    // so a run that waits on every read is cut off.
    let out = Command::new("timeout")
        .args([
            "8",
            env!("CARGO_BIN_EXE_ljos"),
            "consensus",
            "demo-q",
            "--no-file",
        ])
        .current_dir(dir.path())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", &pack)
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_UNSIGNED_BALLOTS", "allow")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "quiet pack held consensus:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("push cite:"),
        "no reading:\n{stdout}\n{stderr}"
    );
}
