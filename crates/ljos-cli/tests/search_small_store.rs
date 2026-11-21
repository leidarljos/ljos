//! `ljos search "seal hinge"` on a store of two lessons.
//!
//! A word in every lesson is not common until four lessons carry it.
//! Two lessons that share `seal` still return the lesson that matches
//! that word. Four lessons that all carry `seal` leave the single-word
//! row out, and the line is `No close matches`.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const KILN: &str = "Seat the kiln door seal only when the clay is cold.";
const JAR: &str = "A wax seal keeps the jar shut.";

fn lessons(texts: &[String]) -> String {
    let hits: Vec<_> = texts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            serde_json::json!({
                "id": format!("lesson{i}"),
                "kind": "lesson",
                "text": text,
                "score": 0.9,
                "entities": ["seat:ada"],
            })
        })
        .collect();
    serde_json::json!({"hits": hits, "rank": "linear"}).to_string()
}

/// A pack whose encoder is off, so search ranks the returned rows by words.
fn pack(search_body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let search_body = search_body.clone();
            thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let body = if path == "/v1/status" {
                    r#"{"embedder":{"available":false}}"#.to_string()
                } else if path == "/v1/search" {
                    search_body
                } else if path == "/v1/atoms" {
                    r#"{"atoms":[]}"#.to_string()
                } else {
                    "ok".to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn search(url: &str, query: &str) -> (bool, String, String) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join("state")).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    std::fs::create_dir_all(root.path().join("tracker")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", query])
        .current_dir(root.path())
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("USER", "ada")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("VISSUE_ROOT", root.path().join("tracker"))
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("LJOS_SEARCH_WAIT", "0")
        .env("LJOS_SEARCH_WARMUP_WAIT", "0")
        .env("LJOS_SEARCH_BUSY_WAIT", "0")
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "2000")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("ljos");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn two_lessons_return_the_one_word_match() {
    let body = lessons(&[KILN.to_string(), JAR.to_string()]);
    let url = pack(body);
    let (ok, stdout, stderr) = search(&url, "seal hinge");
    assert!(ok, "{stdout}{stderr}");
    assert!(
        stdout.contains("kiln door seal"),
        "a store of two lessons should return the seal lesson:\n{stdout}{stderr}"
    );
    assert!(
        !stdout.contains("No close matches"),
        "a store of two lessons should not drop the one shared word:\n{stdout}"
    );
}

#[test]
fn four_lessons_drop_a_word_in_every_row() {
    let texts: Vec<String> = (0..4)
        .map(|i| format!("A wax seal keeps jar number {i} shut."))
        .collect();
    let url = pack(lessons(&texts));
    let (ok, stdout, stderr) = search(&url, "seal hinge");
    assert!(ok, "{stdout}{stderr}");
    assert!(
        stdout.contains("No close matches"),
        "four lessons that all carry seal should leave the row out:\n{stdout}{stderr}"
    );
    assert!(
        !stdout.contains("jar number"),
        "the single-word rows should not be printed:\n{stdout}"
    );
}
