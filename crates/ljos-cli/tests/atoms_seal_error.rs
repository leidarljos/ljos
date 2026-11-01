//! A scope with a recipient age refuses, and a log larger than the pipe
//! buffers, ends `ljos sync` with age's error. age exits before it reads
//! the plaintext, and the rest of the write used to end ljos with SIGPIPE
//! before it printed a line. Runs the built `ljos` binary, since the test
//! harness ignores SIGPIPE.
#![cfg(target_os = "linux")]

ljos_test_boot::harness!();
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

fn keygen(path: &Path) -> String {
    let made = Command::new("age-keygen")
        .arg("-o")
        .arg(path)
        .output()
        .expect("age-keygen must be installed");
    assert!(made.status.success(), "age-keygen");
    let public = Command::new("age-keygen")
        .arg("-y")
        .arg(path)
        .output()
        .unwrap();
    assert!(public.status.success(), "age-keygen -y");
    String::from_utf8_lossy(&public.stdout).trim().to_string()
}

fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A pack that lists `atoms` and answers anything else with `{}`.
fn pack(atoms: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buf[..at]).to_ascii_lowercase();
                let need: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|n| n.trim().parse().ok())
                    .unwrap_or(0);
                if buf.len() >= at + 4 + need {
                    break;
                }
            }
            let line = String::from_utf8_lossy(&buf);
            let answer = if line.starts_with("GET /v1/atoms") {
                atoms.clone()
            } else {
                "{}".to_string()
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn a_recipient_age_refuses_is_named_on_a_large_log() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let config = home.join(".config");
    std::fs::create_dir_all(config.join("ljos")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    let me = keygen(&config.join("ljos/age.key"));
    let repo = dir.path().join("tracker");
    std::fs::create_dir_all(repo.join(".ljos")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "core.hooksPath", "/dev/null"]);
    git(&repo, &["config", "user.name", "probe"]);
    git(&repo, &["config", "user.email", "probe@example.invalid"]);
    std::fs::write(
        repo.join(".ljos/sync.toml"),
        format!("name = \"probe\"\nrecipients = [\"{me}\", \"age1notakey\"]\n"),
    )
    .unwrap();
    std::fs::write(
        config.join("ljos/sync.toml"),
        format!("repos = [\"{}\"]\n", repo.display()),
    )
    .unwrap();
    let atoms: Vec<_> = (0..2000)
        .map(|n| {
            serde_json::json!({
                "id": format!("a{n:04}"),
                "kind": "lesson",
                "text": format!("note {n} {}", "x".repeat(200)),
                "entities": [],
            })
        })
        .collect();
    let url = pack(serde_json::json!({ "atoms": atoms }).to_string());
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("sync")
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("LJOS_TEST_HOSTNAME", "me")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("PACKSET_URL", url)
        .env("PACKSET_AUTH", "off")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(&repo)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.signal(),
        None,
        "ljos was killed by a signal: {text}"
    );
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("age1notakey"), "{text}");
}
