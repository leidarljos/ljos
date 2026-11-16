//! A two sentence lesson is one row. A longer lesson is split, and every
//! stored id and its text is printed.
//!
//! The pack does not treat `e.g.` as a sentence end. Remember used to,
//! stored two lessons, and the page showed only the second.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const TWO: &str = "Use a ring, e.g. the instanton. It converges.";
const FOUR: &str = "Fire the kiln before dusk. Leave the door shut until the shelf is cold. Write the batch number on the shelf card. Sweep the floor after the last tray.";

struct Seat {
    root: PathBuf,
    url: String,
    posts: Arc<Mutex<Vec<String>>>,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "work",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        let posts = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let recorded = Arc::clone(&posts);
        thread::spawn(move || serve(listener, recorded));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
            posts,
        }
    }

    fn remember(&self, text: &str) -> Output {
        let home = self.root.join("home");
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["remember", text])
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("USER", "ada")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("ljos remember: {err}"))
    }
}

fn serve(listener: TcpListener, posts: Arc<Mutex<Vec<String>>>) {
    let ids = [
        "4d2243d2aaaaaaaaaaaaaaaaaaaaaaaa",
        "0119a00abbbbbbbbbbbbbbbbbbbbbbbb",
    ];
    let next = Arc::new(Mutex::new(0usize));
    for conn in listener.incoming().flatten() {
        let posts = Arc::clone(&posts);
        let next = Arc::clone(&next);
        thread::spawn(move || {
            let mut sock = conn;
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 8192];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if request_complete(&buf) {
                            break;
                        }
                    }
                }
            }
            if buf.is_empty() {
                return;
            }
            let head_end = buf
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap_or(buf.len());
            let head = String::from_utf8_lossy(&buf[..head_end.min(buf.len())]).to_string();
            let request = head.split_whitespace().nth(1).unwrap_or("/");
            let path = request.split('?').next().unwrap_or("/");
            let method = head.split_whitespace().next().unwrap_or("");
            let raw_body = if head_end + 4 <= buf.len() {
                String::from_utf8_lossy(&buf[head_end + 4..]).into_owned()
            } else {
                String::new()
            };
            let body = if method == "POST" && path == "/v1/atoms" {
                let text = json_string(&raw_body, "text");
                posts.lock().unwrap().push(text.clone());
                let mut slot = next.lock().unwrap();
                let id = ids.get(*slot).copied().unwrap_or(ids[1]);
                *slot += 1;
                let kind = json_string(&raw_body, "kind");
                let kind = if kind.is_empty() { "lesson" } else { &kind };
                format!(
                    r#"{{"id":"{id}","kind":"{kind}","text":{}}}"#,
                    json_quote(&text)
                )
            } else if path == "/v1/status" {
                r#"{"embedder":{"available":true,"answering":true,"enabled":true}}"#.to_string()
            } else if path == "/v1/atoms" {
                r#"{"atoms":[]}"#.to_string()
            } else {
                r#"{"ok":true}"#.to_string()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        });
    }
}

fn request_complete(buf: &[u8]) -> bool {
    let Some(head_end) = buf.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&buf[..head_end]);
    let length = head.lines().find_map(|line| {
        let rest = line
            .split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))?
            .1
            .trim();
        rest.parse::<usize>().ok()
    });
    match length {
        Some(n) => buf.len() >= head_end + 4 + n,
        None => true,
    }
}

fn json_string(body: &str, key: &str) -> String {
    let needle = format!("\"{key}\"");
    let Some(at) = body.find(&needle) else {
        return String::new();
    };
    let rest = &body[at + needle.len()..];
    let Some(start) = rest.find('"') else {
        return String::new();
    };
    let mut out = String::new();
    let mut chars = rest[start + 1..].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    out
}

fn json_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_two_sentence_lesson_with_an_abbreviation_is_one_row() {
    let seat = Seat::open();
    let out = seat.remember(TWO);
    let report = said(&out);
    assert!(out.status.success(), "{report}");
    let posts = seat.posts.lock().unwrap().clone();
    assert_eq!(posts, vec![TWO.to_string()], "{report}\nposts: {posts:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(TWO),
        "the lesson was not printed whole:\n{report}"
    );
    assert!(
        !stdout.contains("stored as 2"),
        "a two sentence lesson was split:\n{report}"
    );
    assert!(
        stdout.contains("4d2243d2"),
        "the stored id was not printed:\n{report}"
    );
}

#[test]
fn a_longer_lesson_prints_every_stored_id_and_text() {
    let seat = Seat::open();
    let out = seat.remember(FOUR);
    let report = said(&out);
    assert!(out.status.success(), "{report}");
    let posts = seat.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 2, "{report}\nposts: {posts:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains(
            "remember: stored as 2 claims, because a lesson is at most two sentences, so run `ljos search`."
        ),
        "the split was not named:\n{report}"
    );
    for needle in [
        "4d2243d2",
        "0119a00a",
        "Fire the kiln before dusk.",
        "Leave the door shut until the shelf is cold.",
        "Write the batch number on the shelf card.",
        "Sweep the floor after the last tray.",
    ] {
        assert!(
            stdout.contains(needle),
            "stdout dropped {needle}:\n{report}"
        );
    }
    let first = stdout.find("4d2243d2").expect("first id");
    let second = stdout.find("0119a00a").expect("second id");
    assert!(first < second, "{stdout}");
    let dusk = stdout
        .find("Fire the kiln before dusk.")
        .expect("first text");
    let sweep = stdout
        .find("Sweep the floor after the last tray.")
        .expect("second text");
    assert!(dusk < sweep, "{stdout}");
}
