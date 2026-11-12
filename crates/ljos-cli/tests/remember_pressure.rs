//! A pack that answers 503 under memory pressure queues the lesson.
//! A local prose refusal is not a reason to drop it. A duplicate names
//! the atom the pack already holds.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

fn serve(status: u16, body: &str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = body.to_string();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let body = body.clone();
            thread::spawn(move || {
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let reason = if status == 503 {
                    "Service Unavailable"
                } else {
                    "Bad Request"
                };
                let retry = if status == 503 {
                    "Retry-After: 60\r\n"
                } else {
                    ""
                };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\n{retry}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    port
}

std::thread_local! {
    static LAST_QUEUE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn remember(port: u16, text: &str) -> std::process::Output {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["remember", text])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", format!("http://127.0.0.1:{port}"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let queued = std::fs::read_to_string(state.join("ljos/note-queue.jsonl")).unwrap_or_default();
    LAST_QUEUE.with(|slot| *slot.borrow_mut() = queued);
    out
}

#[test]
fn a_pressure_503_queues_a_lesson_the_gate_would_refuse() {
    let port = serve(
        503,
        r#"{"error":"packsetd is busy: memory pressure; try again"}"#,
    );
    let words = packset_core::prose::VERY_HARD_WORDS + 4;
    let text = "word ".repeat(words);
    let text = text.trim().to_string();
    assert!(
        !ljos_cli::pack_copy::pack_copy_ok(&text),
        "the local gate accepted the long sentence"
    );
    let out = remember(port, &text);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let queued = LAST_QUEUE.with(|slot| slot.borrow().clone());
    assert!(
        stderr.contains("the lesson is queued"),
        "stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(
        !stderr.contains("was not queued"),
        "the lesson was dropped:\n{stderr}"
    );
    assert!(
        !stderr.contains("would refuse this copy"),
        "load was reported as a content refusal:\n{stderr}"
    );
    assert!(
        queued.contains(&text),
        "the queue does not hold the lesson:\n{queued}"
    );
}

#[test]
fn a_duplicate_lesson_prints_the_atom_id() {
    let port = serve(400, r#"{"error":"duplicate of abcdef0123456789"}"#);
    let out = remember(port, "pin the fuse");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let said = format!("{stdout}{stderr}");
    assert!(
        said.contains("abcdef0123456789"),
        "the existing atom id was not printed:\n{said}"
    );
    assert!(
        stderr.contains("already stored as abcdef0123456789"),
        "{stderr}"
    );
    assert!(
        out.status.success(),
        "a duplicate exited as an error:\n{said}"
    );
    let queued = LAST_QUEUE.with(|slot| slot.borrow().clone());
    assert!(queued.is_empty(), "a duplicate was queued:\n{queued}");
}
