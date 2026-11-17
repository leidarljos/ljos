//! Search keeps this seat and the named project ahead of weak rows from
//! other seats. A strong cross-seat match remains and names its seat.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const QUERY: &str = "readcon release 0.24.0 fuzz shared_pool";

fn pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut part = [0u8; 4096];
                while !buf.windows(4).any(|window| window == b"\r\n\r\n") {
                    match sock.read(&mut part) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&part[..n]),
                    }
                }
                let request = String::from_utf8_lossy(&buf);
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                let body = if path == "/v1/status" {
                    r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                } else if path == "/v1/search" {
                    r#"{"rank":"relevance","hits":[
{"id":"disk","kind":"lesson","text":"Disk saver release notes.","score":98,"cosine":0.30,"entities":["seat:disk-saver","issue:storage-old"]},
{"id":"hle","kind":"lesson","text":"The hlebot release checklist.","score":96,"cosine":0.32,"entities":["seat:hlebot","issue:hle-old"]},
{"id":"project","kind":"lesson","text":"Readcon fuzz jobs share the pool for release 0.24.0.","score":82,"cosine":0.79,"entities":["seat:bob","issue:readcon-old"]},
{"id":"own","kind":"lesson","text":"shared_pool is reset before the fuzz release job.","score":76,"cosine":0.74,"entities":["seat:alice","issue:tools-job"]},
{"id":"strong","kind":"lesson","text":"Release 0.24.0 fuzzing uses shared_pool cleanup in the worker.","score":91,"cosine":0.93,"entities":["seat:carol","issue:worker-fuzz"]}
]}"#
                } else if path == "/v1/atoms" {
                    r#"{"atoms":[{"id":"own","kind":"lesson","text":"stored"}]}"#
                } else {
                    r#"{"ok":true}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(response.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn this_seat_and_the_queries_project_lead_without_hiding_a_strong_other_seat() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    for dir in [
        home.join(".config"),
        home.join(".local/state"),
        home.join(".local/share"),
        root.path().join("run"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", QUERY, "-n", "5"])
        .current_dir(root.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("LJOS_SEAT", "alice")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_SEARCH_WAIT", "0")
        .env("LJOS_SEARCH_WARMUP_WAIT", "0")
        .env("PACKSET_URL", pack())
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{said}");
    assert!(!stdout.contains("Disk saver"), "{said}");
    assert!(!stdout.contains("hlebot"), "{said}");
    let own = stdout
        .find("shared_pool is reset")
        .unwrap_or_else(|| panic!("{said}"));
    let project = stdout
        .find("Readcon fuzz jobs")
        .unwrap_or_else(|| panic!("{said}"));
    let strong = stdout
        .find("Release 0.24.0 fuzzing")
        .unwrap_or_else(|| panic!("{said}"));
    assert!(own < project && project < strong, "{said}");
    assert!(stdout.contains("from bob"), "{said}");
    assert!(stdout.contains("from carol"), "{said}");
}
