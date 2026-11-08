//! Loop verbs return while a sidecar is missing or a pack accepts and
//! stays silent. Each one used to sit on a thirty-second read, or on a
//! helper that never exited.

ljos_test_boot::harness!();

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

fn home_of(dir: &Path) -> std::path::PathBuf {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    home
}

fn ljos(dir: &Path, pack: &str, args: &[&str]) -> (std::process::Output, Duration) {
    let home = home_of(dir);
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
    );
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .current_dir(dir)
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", pack)
        .env("PACKSET_AUTH", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (out, started.elapsed())
}

fn text_of(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_silent_pack_does_not_hold_a_sitting() {
    let dir = tempfile::tempdir().unwrap();
    let pack = silent_pack();
    let (out, took) = ljos(dir.path(), &pack, &["sitting", "demo-sit"]);
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(15),
        "sitting waited {took:?}\n{said}"
    );
    assert!(!out.status.success(), "{said}");
}

#[test]
fn a_sitting_dies_when_the_pack_probe_is_signalled() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_of(dir.path());
    let pack = silent_pack();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["sitting", "demo-sig"])
        .current_dir(dir.path())
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", &pack)
        .env("PACKSET_AUTH", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("sitting ignored SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success(), "signal status {status}");
}

#[test]
fn a_silent_pack_does_not_hold_a_search() {
    let dir = tempfile::tempdir().unwrap();
    let pack = silent_pack();
    for args in [vec!["search", "32148"], vec!["search", "--rerank", "32148"]] {
        let (out, took) = ljos(dir.path(), &pack, &args);
        let said = text_of(&out);
        assert!(
            took < Duration::from_secs(8),
            "{} waited {took:?}\n{said}",
            args.join(" ")
        );
        assert!(out.status.success(), "{said}");
        assert!(
            said.contains("ranked by words; the pack search did not answer in time"),
            "{said}"
        );
        assert!(said.contains("pack busy"), "{said}");
    }
}

#[test]
fn a_silent_pack_does_not_hold_a_remember() {
    let dir = tempfile::tempdir().unwrap();
    let pack = silent_pack();
    let (out, took) = ljos(dir.path(), &pack, &["remember", "tabs not spaces"]);
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(8),
        "remember waited {took:?}\n{said}"
    );
    assert_eq!(out.status.code(), Some(75), "{said}");
    assert!(said.contains("pack busy, try again"), "{said}");
}

#[test]
fn a_silent_pack_does_not_hold_a_consensus() {
    let dir = tempfile::tempdir().unwrap();
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
    let pack = silent_pack();
    let home = home_of(dir.path());
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
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", &pack)
        .env("PACKSET_AUTH", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_UNSIGNED_BALLOTS", "allow")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(8),
        "consensus waited {took:?}\n{said}"
    );
    assert!(out.status.success(), "{said}");
}

#[test]
fn a_note_returns_when_claimdag_never_answers() {
    let dir = tempfile::tempdir().unwrap();
    let noted = dir.path().join("noted");
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            noted.display()
        ),
    );
    script(dir.path(), "claimdag", "#!/bin/sh\nexec sleep 60\n");
    let (out, took) = ljos(
        dir.path(),
        "http://127.0.0.1:9",
        &["note", "demo-n", "hello"],
    );
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(8),
        "note waited {took:?}\n{said}"
    );
    assert!(out.status.success(), "{said}");
    let wrote = std::fs::read_to_string(&noted).unwrap_or_default();
    assert!(wrote.contains("note"), "{wrote}");
}

#[test]
fn a_note_returns_when_the_tracker_does_not_answer() {
    let dir = tempfile::tempdir().unwrap();
    script(dir.path(), "vissue", "#!/bin/sh\nexec sleep 60\n");
    let home = home_of(dir.path());
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
    );
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["note", "demo-n", "hello"])
        .current_dir(dir.path())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", "http://127.0.0.1:9")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_TRACKER_LOCK_WAIT", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(8),
        "note waited {took:?}\n{said}"
    );
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("did not answer in time"), "{said}");
}
