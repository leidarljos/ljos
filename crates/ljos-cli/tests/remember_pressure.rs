//! A pack that answers 503 under memory pressure does not store the lesson.
//! A local prose refusal is not what the line says. A duplicate names
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
    remember_with(port, &["remember", text])
}

fn remember_with(port: u16, args: &[&str]) -> std::process::Output {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
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
fn a_pressure_503_stores_nothing_and_does_not_call_it_a_refusal() {
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
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(
        stderr.contains("Nothing was stored. Run `ljos remember` again."),
        "stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(
        !stderr.contains("would refuse this copy"),
        "load was reported as a content refusal:\n{stderr}"
    );
    assert!(
        !queued.contains(&text),
        "a busy pack stored a copy the command said was not stored:\n{queued}"
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

#[test]
fn json_for_one_claim_is_the_claim_object() {
    let (port, _posts) =
        serve_first_then_drop(r#"{"id":"abcdef0123456789","kind":"lesson","text":"pin the fuse"}"#);
    let out = remember_with(port, &["remember", "--json", "pin the fuse"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!(
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(out.status.success(), "{said}");
    let want = "{\n  \"id\": \"abcdef0123456789\",\n  \"kind\": \"lesson\",\n  \"text\": \"pin the fuse\",\n  \"prose\": {\n    \"grade\": null,\n    \"target\": 14,\n    \"flagged_sentence\": null\n  }\n}\n";
    assert_eq!(stdout.as_ref(), want, "{said}");
}

#[test]
fn json_for_one_duplicate_prints_the_existing_claim() {
    let port = serve(400, r#"{"error":"duplicate of abcdef0123456789"}"#);
    let out = remember_with(port, &["remember", "--json", "pin the fuse"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!(
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(out.status.success(), "{said}");
    let claim: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|err| panic!("{err}\n{said}"));
    assert_eq!(claim["id"], "abcdef0123456789", "{said}");
    assert_eq!(claim["duplicate"], true, "{said}");
    assert!(
        stderr.contains("already stored as abcdef0123456789"),
        "{said}"
    );
}

#[test]
fn json_prints_a_claims_document_when_every_chunk_is_already_stored() {
    let port = serve(400, r#"{"error":"duplicate of abcdef0123456789"}"#);
    let out = remember_with(port, &["remember", "--json", TWO_CLAIMS]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!(
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(out.status.success(), "{said}");
    assert!(
        stderr.contains("already stored as abcdef0123456789"),
        "{said}"
    );
    let page: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|err| panic!("stdout is not one JSON document: {err}\n{said}"));
    let claims = page["claims"]
        .as_array()
        .unwrap_or_else(|| panic!("stdout has no claims array:\n{said}"));
    assert_eq!(claims.len(), 2, "{said}");
    assert_eq!(page["stored"], 2, "{said}");
    assert_eq!(page["queued"], 0, "{said}");
    assert_eq!(claims[0]["id"], "abcdef0123456789", "{said}");
    assert_eq!(claims[0]["duplicate"], true, "{said}");
    assert_eq!(claims[1]["duplicate"], true, "{said}");
    assert_ne!(claims[0]["text"], claims[1]["text"], "{said}");
}

#[test]
fn a_fully_queued_split_says_queued() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let text = "Fire the kiln before dusk. Leave the door shut until the shelf is cold. Write the batch number on the shelf card.";
    let out = remember_with(port, &["remember", text]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!(
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(out.status.success(), "{said}");
    assert!(stderr.contains("remember: queued as 2 claims"), "{said}");
    assert!(!stderr.contains("stored as"), "{said}");
    assert!(!stdout.contains("stored as"), "{said}");
}

#[test]
fn json_for_one_queued_claim_is_one_document() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let out = remember_with(port, &["remember", "--json", "Fire the kiln before dusk."]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!(
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(out.status.success(), "{said}");
    let claim: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|err| panic!("{err}\n{said}"));
    for (key, want) in [
        ("queued", serde_json::json!(true)),
        ("kind", serde_json::json!("lesson")),
        ("text", serde_json::json!("Fire the kiln before dusk.")),
    ] {
        assert_eq!(claim[key], want, "{key}\n{said}");
    }
    assert!(claim.get("prose").is_some(), "{said}");
    assert!(claim.get("id").is_none(), "{said}");
    assert!(stderr.contains(ljos_cli::REMEMBER_QUEUED_LINE), "{said}");
    assert!(!stdout.contains("Memory busy:"), "{said}");

    let help = remember_with(port, &["remember", "--help"]);
    let help = String::from_utf8_lossy(&help.stdout);
    let help_line = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        help_line.contains("A queued claim has `queued`, `kind`, `text` and `prose`, and no `id`."),
        "{help}"
    );
}

/// A pack that stores the first post and drops the socket on every later
/// one. `posts` counts the requests it read.
fn serve_first_then_drop(
    first_body: &str,
) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let posts = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&posts);
    let first_body = first_body.to_string();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            if !buf.starts_with(b"POST ") {
                continue;
            }
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                let resp = format!(
                    "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{first_body}",
                    first_body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            }
        }
    });
    (port, posts)
}

const TWO_CLAIMS: &str = "Fire the kiln before dusk. Leave the door shut until the shelf is cold. Write the batch number on the shelf card.";

#[test]
fn a_split_that_stored_one_chunk_and_queued_the_other_says_both() {
    let (port, posts) = serve_first_then_drop(
        r#"{"id":"abcdef0123456789","kind":"lesson","text":"Fire the kiln before dusk. Leave the door shut until the shelf is cold."}"#,
    );
    let out = remember_with(port, &["remember", TWO_CLAIMS]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!(
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(
        posts.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "{said}"
    );
    assert!(out.status.success(), "{said}");
    let want = "remember: stored 1 claim and queued 1 claim";
    assert!(!stderr.contains(want), "{said}");
    assert!(stdout.contains(want), "{said}");
    assert_eq!(said.matches(want).count(), 1, "{said}");
    assert!(!stderr.contains("stored as 2"), "{said}");
    assert!(!stdout.contains("stored as 2"), "{said}");
    assert!(stdout.contains("id abcdef01"), "{said}");
    let queued = LAST_QUEUE.with(|slot| slot.borrow().clone());
    assert!(queued.contains("Write the batch number"), "{queued}");
    assert!(!queued.contains("Fire the kiln"), "{queued}");
}

#[test]
fn json_reports_how_many_chunks_were_stored_and_how_many_queued() {
    let (port, _posts) = serve_first_then_drop(
        r#"{"id":"abcdef0123456789","kind":"lesson","text":"Fire the kiln before dusk. Leave the door shut until the shelf is cold."}"#,
    );
    let out = remember_with(port, &["remember", "--json", TWO_CLAIMS]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!(
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(out.status.success(), "{said}");
    let page: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|err| panic!("stdout is not one JSON document: {err}\n{said}"));
    assert_eq!(page["stored"], 1, "{said}");
    assert_eq!(page["queued"], 1, "{said}");
    let claims = page["claims"].as_array().unwrap();
    assert_eq!(claims.len(), 2, "{said}");
    assert_eq!(claims[0]["id"], "abcdef0123456789", "{said}");
    assert_ne!(claims[0]["queued"], true, "{said}");
    assert_eq!(claims[1]["queued"], true, "{said}");
    assert!(!stderr.contains("stored as 2"), "{said}");
}
