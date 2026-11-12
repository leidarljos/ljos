//! A sitting in commit mode claims when the tracker's remote cannot be
//! reached. Unpushed commits and uncommitted edits are a warning. They
//! are not a tracker that does not answer, and the sitting does not ask
//! the remote which branch it publishes.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn line_of(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf)
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

fn pack() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let body = if line_of(&buf).contains("/v1/activate") {
                r#"{"island":[],"fired":0}"#
            } else if line_of(&buf).contains("/v1/atoms") {
                r#"{"atoms":[]}"#
            } else {
                r#"{"embedder":{"available":false}}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    port
}

/// Accepts whatever connects and counts it. The sitting must not.
fn closed_remote() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            seen.fetch_add(1, Ordering::SeqCst);
            let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
            let mut buf = [0u8; 256];
            let _ = sock.read(&mut buf);
        }
    });
    (port, hits)
}

#[test]
fn a_sitting_claims_when_the_tracker_remote_cannot_be_reached() {
    let (remote_port, hits) = closed_remote();
    let root = tempfile::tempdir().unwrap();
    let log = root.path().join("calls.log");
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".cursor")).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    let tracker = root.path().join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    git(&tracker, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.email", "seat@example.invalid"),
        ("user.name", "seat"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/dev/null"),
    ] {
        git(&tracker, &["config", key, value]);
    }
    let layout = vissue_core::Layout::new(&tracker, "Software");
    vissue_core::ops::create(&layout, "probe", "probe work", Default::default()).unwrap();
    let doc =
        vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe")).unwrap();
    let issue = doc.headings[0].id.clone();
    git(&tracker, &["add", "."]);
    git(&tracker, &["commit", "-q", "-m", "seed"]);
    git(
        &tracker,
        &[
            "remote",
            "add",
            "origin",
            &format!("https://127.0.0.1:{remote_port}/private.git"),
        ],
    );
    git(
        &tracker,
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(&tracker, &["branch", "-u", "origin/main"]);
    for n in 0..200 {
        git(
            &tracker,
            &["commit", "-q", "--allow-empty", "-m", &format!("local {n}")],
        );
    }
    std::fs::write(tracker.join("Software/other.org"), "* TODO side\n").unwrap();
    std::fs::write(tracker.join("Software/sixth.org"), "* TODO sixth\n").unwrap();

    let show = format!(r#"{{"id":"{issue}","title":"probe work","state":"TODO","blocked_by":[]}}"#);
    script(
        root.path(),
        "vissue",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{log}'\ncase \"$1\" in\n  show) printf '%s\\n' '{show}' ;;\n  recall) echo noted ;;\n  identity) printf '%s\\n' 'vissue 0.22.0' 'root={tracker}' 'prefix=Software' ;;\nesac\nexit 0\n",
            log = log.display(),
            tracker = tracker.display(),
            show = show,
        ),
    );
    script(
        root.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"claimdag $*\" >> '{}'\ncase \"$1\" in\n  get) exit 1 ;;\nesac\necho '[]'\nexit 0\n",
            log.display()
        ),
    );
    script(
        root.path(),
        "deedar",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"deedar $*\" >> '{}'\necho head\nexit 0\n",
            log.display()
        ),
    );

    let path = format!("{}:/usr/bin:/bin", root.path().display());
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["sitting", &issue])
        .current_dir(root.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("VISSUE_AGENT", "box")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("LJOS_TRACKER_PUSH_WAIT", "0")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", format!("http://127.0.0.1:{}", pack()))
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        out.status.success(),
        "sitting refused in {took:?}:\n{said}\ncalls:\n{calls}"
    );
    assert!(
        took < Duration::from_secs(15),
        "sitting waited {took:?} on the remote:\n{said}"
    );
    assert!(
        !said.lines().any(|l| l.contains("unpushed")),
        "a quiet sitting leaves the drift for --verbose:\n{said}"
    );
    assert!(!said.contains("nothing was claimed"), "{said}");
    assert!(!said.contains("does not answer"), "{said}");
    assert!(
        calls.contains("claimdag claim"),
        "the issue was not claimed:\n{calls}\n{said}"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the sitting reached the tracker remote"
    );
}
