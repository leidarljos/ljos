//! A pack that answers slowly still contributes its memories. The time
//! spent answering is not charged to the side budget. Runs the built
//! `ljos` binary.
#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

#[path = "common/seat_root.rs"]
mod seat_root;

struct Hab {
    root: PathBuf,
    home: PathBuf,
    run: PathBuf,
    repo: PathBuf,
}

impl Drop for Hab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn hab(name: &str) -> Hab {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp).join(format!(
        "m5p-{name}-{}-{}",
        std::process::id(),
        nanos % 1_000_000_000
    ));
    let hab = Hab {
        home: root.join("home"),
        run: root.join("run"),
        repo: root.join("repo"),
        root,
    };
    for dir in [&hab.home, &hab.run, &hab.repo] {
        std::fs::create_dir_all(dir).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hab.run, std::fs::Permissions::from_mode(0o700)).unwrap();
    hab
}

fn atoms() -> String {
    let rows: Vec<_> = (0..5)
        .map(|i| {
            serde_json::json!({
                "id": format!("atom-{i}"),
                "kind": "lesson",
                "text": format!("lesson number {i} about readme lines"),
                "tags": ["lesson", "seat"],
                "created_at": "2026-10-20T00:00:00Z",
            })
        })
        .collect();
    serde_json::json!({"atoms": rows}).to_string()
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let rest = line
                .strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))?;
            rest.trim().parse().ok()
        })
        .unwrap_or(0)
}

fn slow_pack(delay: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let body_atoms = atoms();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let body_atoms = body_atoms.clone();
            std::thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut tmp) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    }
                    let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("/");
                    let path = path.split('?').next().unwrap_or("/");
                    let cl = content_length(&head);
                    while buf.len() < head_end + cl {
                        match sock.read(&mut tmp) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    }
                    std::thread::sleep(delay);
                    let body = if path == "/v1/status" {
                        r#"{"embedder":{"available":true,"answering":null}}"#.to_string()
                    } else if path == "/v1/atoms" {
                        body_atoms.clone()
                    } else {
                        r#"{"hits":[]}"#.to_string()
                    };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    if sock.write_all(resp.as_bytes()).is_err() {
                        return;
                    }
                    let rest = head_end + cl;
                    buf = buf.split_off(rest.min(buf.len()));
                }
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn session(url: &str, hab: &Hab) -> String {
    let payload = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-probe",
        "hook_event_name": "sessionStart",
        "session_id": "c-probe",
    })
    .to_string();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("hook")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &hab.home)
        .env("XDG_CONFIG_HOME", hab.home.join(".config"))
        .env("XDG_DATA_HOME", hab.home.join(".local/share"))
        .env("XDG_CACHE_HOME", hab.home.join(".cache"))
        .env("XDG_STATE_HOME", hab.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &hab.run)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_PROJECT", "probe")
        .env("LJOS_SEAT", "probe-seat")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("PACKSET_URL", url)
        .env("PACKSET_TOKEN", "t".repeat(32))
        .current_dir(&hab.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(payload.as_bytes()).unwrap();
    drop(stdin);
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() && started.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Delays of 0.15 s and 0.30 s are inside one read and are not charged,
/// so later reads still see the five memories.
#[test]
fn a_slow_pack_answer_still_shows_memories() {
    for delay in [Duration::from_millis(150), Duration::from_millis(300)] {
        let url = slow_pack(delay);
        let hab = hab("slow");
        let out = session(&url, &hab);
        assert!(
            out.contains("5 memories"),
            "delay {delay:?} dropped the memories: {out}"
        );
    }
}
