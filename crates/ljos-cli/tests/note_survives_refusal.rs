//! `ljos note` keeps the note on the ticket when the pack refuses its
//! memory copy. The pack's prose gate answers 400 to an agent sentence
//! past its word limit; a weak model writes such notes, and the note must
//! not be lost. The refusal is a warning and the command succeeds.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A pack that refuses every request the way the prose gate does.
fn refusing_pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let body = r#"{"error":"atom sentence 1 is very hard to read: 38 words, the limit is 29; split it at a conjunction or drop a clause"}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

#[test]
fn a_refused_memory_copy_still_leaves_the_note_on_the_ticket() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\nexit 0\n",
            log.display()
        ),
    );
    script(dir.path(), "claimdag", "#!/bin/sh\nexit 1\n");
    let home = dir.path().join("home");
    let runtime = dir.path().join("run");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let long = "This single sentence runs on for well past the limit the pack sets for one memory because a weak model wrote it in one breath without a full stop anywhere along the way to the end of it";
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["note", "proj-aaaa", long])
        .env_clear()
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("VISSUE_ROOT", dir.path().join("tracker"))
        .env("LJOS_SEAT", "saddle")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", refusing_pack())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "ljos note failed: {out:?}");
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        calls.contains(&format!("vissue note proj-aaaa {long}")),
        "the note did not reach the ticket:\n{calls}"
    );
    assert!(
        stdout.contains("warning: memory kept no copy of the note"),
        "{stdout}"
    );
    assert!(stdout.contains("is on proj-aaaa's logbook"), "{stdout}");
}
