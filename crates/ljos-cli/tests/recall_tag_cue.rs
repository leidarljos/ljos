//! Recall asks the pack for an issue's tags on the same query as its id.
//!
//! The pack is a stand-in that records each search query. An issue titled
//! `Follow-up` and tagged `mail` and `wake` is searched twice. The id
//! query carries `mail wake`, and a memory only that query returns is
//! what `ljos recall` prints. An issue with no tags is searched twice,
//! for the id and the title.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

/// The memory the stand-in returns only when a query carries both tags.
const TAG_MEMORY: &str = "Courier retries a dropped letter before the quay opens.";

/// The token the seat shows. Any non-empty token lets the search be asked.
const TOKEN: &str = "0e1d2c3b4a59687706f5e4d3c2b1a0900e1d2c3b4a59687706f5e4d3c2b1a090";

fn read_http(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = match stream.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&tmp[..n]);
        let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        if buf.len() >= pos + 4 {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// The `q` values of `GET /v1/search`, in the order the pack was asked.
fn search_queries(req: &str) -> Vec<String> {
    let Some(path) = req.lines().next().and_then(|line| {
        let mut parts = line.split_whitespace();
        let method = parts.next()?;
        let target = parts.next()?;
        (method == "GET" && target.starts_with("/v1/search")).then_some(target)
    }) else {
        return Vec::new();
    };
    let Some(query) = path.split_once('?').map(|(_, q)| q) else {
        return Vec::new();
    };
    url::form_urlencoded::parse(query.as_bytes())
        .filter(|(key, _)| key == "q")
        .map(|(_, value)| value.into_owned())
        .collect()
}

/// A pack that logs every search query and returns [`TAG_MEMORY`] only
/// when the query carries both `mail` and `wake`. The listing is empty,
/// so a memory can reach `ljos recall` only by being a search hit.
fn stand_in(log: Arc<Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("stand-in port");
    let addr = listener.local_addr().expect("stand-in addr");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let log = Arc::clone(&log);
            thread::spawn(move || {
                let req = read_http(&mut stream);
                let path = req.lines().next().unwrap_or("");
                let mut queries = search_queries(&req);
                if !queries.is_empty() {
                    log.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .append(&mut queries);
                }
                let asked = path.split_whitespace().nth(1).unwrap_or("");
                // The proof reads an atom id nobody holds, twice, and both
                // have to come back 401 before a search carries the token.
                let (status, body) = if asked.starts_with("/v1/atoms/") {
                    ("401 Unauthorized", r#"{"error":"token"}"#.to_string())
                } else if asked.starts_with("/v1/search") {
                    let q = search_queries(&req).into_iter().next().unwrap_or_default();
                    let hits = if q.split_whitespace().any(|w| w == "mail")
                        && q.split_whitespace().any(|w| w == "wake")
                    {
                        serde_json::json!([{
                            "id": "tagmem000000000000000000000001",
                            "kind": "lesson",
                            "text": TAG_MEMORY,
                            "score": 1.0,
                            "origin": "agent-derived",
                            "entities": [],
                        }])
                    } else {
                        serde_json::json!([])
                    };
                    (
                        "200 OK",
                        serde_json::json!({"hits": hits, "rank": "relevance"}).to_string(),
                    )
                } else {
                    (
                        "200 OK",
                        serde_json::json!({"atoms": [], "hits": [], "embedder": {"available": false}})
                            .to_string(),
                    )
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

fn tracker(root: &Path) {
    let project = root.join("Software/cue");
    std::fs::create_dir_all(&project).expect("project dir");
    std::fs::write(
        project.join("issues.org"),
        "\
#+TITLE: cue issues
#+VISSUE: 1
#+CATEGORY: cue
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] Follow-up            :mail:wake:
:PROPERTIES:
:ID:         cue-tag1
:CREATED:    [2026-09-20 Sat]
:END:

* TODO [#B] Tidy the drawer
:PROPERTIES:
:ID:         cue-bare
:CREATED:    [2026-09-20 Sat]
:END:
",
    )
    .expect("issues.org");
}

fn stub_vissue(bin: &Path) {
    std::fs::create_dir_all(bin).expect("bin");
    let path = bin.join("vissue");
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("vissue stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

struct Seat {
    home: tempfile::TempDir,
    url: String,
    log: Arc<Mutex<Vec<String>>>,
}

impl Seat {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("seat home");
        tracker(home.path());
        stub_vissue(&home.path().join("bin"));
        let log = Arc::new(Mutex::new(Vec::new()));
        let url = stand_in(Arc::clone(&log));
        Self { home, url, log }
    }

    fn recall(&self, issue: &str) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["recall", issue])
            .current_dir(self.home.path())
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.home.path().join("bin").display()),
            )
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.home.path().join("run"))
            .env("VISSUE_ROOT", self.home.path())
            .env("ISSUE_ROOT", self.home.path())
            .env("VISSUE_NO_ROUTE", "1")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_TOKEN", TOKEN)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_CAPTURE", "off")
            .stdin(Stdio::null())
            .output()
            .expect("ljos recall");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{text}");
        text
    }

    fn queries(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[test]
fn a_tagged_issue_is_searched_for_its_tags() {
    let seat = Seat::new();
    let text = seat.recall("cue-tag1");
    let queries = seat.queries();
    assert_eq!(
        queries.len(),
        2,
        "a tagged issue was not searched twice: {queries:?}\n{text}"
    );
    assert!(
        queries[0].starts_with("cue-tag1"),
        "the first search is the id: {queries:?}"
    );
    assert!(
        queries[0].split_whitespace().any(|w| w == "mail")
            && queries[0].split_whitespace().any(|w| w == "wake"),
        "the id search does not carry the tags: {queries:?}\n{text}"
    );
    assert!(
        queries[1].starts_with("Follow-up"),
        "the second search is the title: {queries:?}"
    );
    assert!(
        text.contains(TAG_MEMORY),
        "the memory only the tagged query returns is missing:\n{text}"
    );
}

#[test]
fn an_issue_with_no_tags_is_searched_twice() {
    let seat = Seat::new();
    let text = seat.recall("cue-bare");
    let queries = seat.queries();
    assert_eq!(
        queries.len(),
        2,
        "an issue with no tags was not searched twice: {queries:?}\n{text}"
    );
    assert!(
        queries[0].starts_with("cue-bare"),
        "the first search is the id: {queries:?}"
    );
    assert!(
        queries[1].starts_with("Tidy the drawer"),
        "the second search is the title: {queries:?}"
    );
    assert!(
        !text.contains(TAG_MEMORY),
        "a tag-only memory was shown for an issue with no tags:\n{text}"
    );
}
