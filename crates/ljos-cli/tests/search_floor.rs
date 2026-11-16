//! A local snapshot must not answer a long query with one unrelated lesson.
//!
//! The reported search was `ridley-core landing order rebase review` on a
//! seat whose snapshot held a JOSS editor note. Word overlap scored that
//! note 0.33. The page is empty unless `--min-score 0` is set.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const RIDLEY: &str = "ridley-core landing order rebase review";
const JOSS: &str =
    "The JOSS editor keeps review comments in document order beside the line they mark.";
const NARRATED: &str = "GPRidge narrated video set is the milestone for the ridge walk.";
const KILN: &str = "The damper sticks when the kiln is cold.";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn keep(dir: &Path, id: &str, text: &str) {
    let body = serde_json::json!({
        "id": id,
        "text": text,
        "kind": "lesson",
        "entities": ["seat:ada"],
    });
    let path = dir.join(format!("{id}.json"));
    std::fs::write(path, serde_json::to_vec_pretty(&body).unwrap()).unwrap();
}

/// A pack that has answered and holds nothing for this query. The rows
/// under test are the local snapshot.
fn empty_pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
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
                    r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                } else if path == "/v1/search" {
                    r#"{"hits":[],"rank":"relevance"}"#
                } else if path == "/v1/atoms" {
                    r#"{"atoms":[]}"#
                } else {
                    "ok"
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

struct Seat {
    root: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(home.join("state")).unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        script(
            &bin,
            "vissue",
            "#!/bin/sh\nprintf '%s\\n' 'vissue test' 'root=.' 'prefix=Software'\nexit 0\n",
        );
        let tracker = root.path().join("tracker");
        let kept = tracker.join(".ljos").join("memory").join("kept");
        std::fs::create_dir_all(&kept).unwrap();
        keep(&kept, "local:joss0001", JOSS);
        keep(&kept, "local:narr0001", NARRATED);
        keep(&kept, "local:kiln0001", KILN);
        keep(
            &kept,
            "local:desk0001",
            "The notes are on the desk by the window.",
        );
        Self {
            root,
            url: empty_pack(),
        }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let root = self.root.path();
        let home = root.join("home");
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(root)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.join("bin").display()),
            )
            .env("HOME", &home)
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("VISSUE_ROOT", root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("ljos")
    }
}

fn text(out: &std::process::Output) -> (String, String) {
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn an_off_topic_snapshot_row_is_not_a_search_answer() {
    let seat = Seat::open();
    let out = seat.run(&["search", RIDLEY]);
    let (stdout, stderr) = text(&out);
    assert!(
        out.status.success(),
        "empty search stays status 0:\n{stdout}{stderr}"
    );
    assert!(
        stdout.contains("No relevant matches for"),
        "{stdout}{stderr}"
    );
    assert!(stdout.contains(RIDLEY), "{stdout}");
    assert!(
        stdout.contains("--min-score 0"),
        "the hint names the flag:\n{stdout}"
    );
    assert!(!stdout.contains("JOSS"), "{stdout}");
    assert!(!stdout.contains("kiln"), "{stdout}");
    assert!(
        !stderr.contains("matched by"),
        "an empty page prints no match-mode line:\n{stderr}"
    );
    assert!(
        stdout.lines().all(|line| line.chars().count() <= 80),
        "the page stays inside the column:\n{stdout}"
    );
}

#[test]
fn min_score_zero_shows_the_weak_row_and_a_real_query_still_hits() {
    let seat = Seat::open();
    let weak = seat.run(&["search", "--min-score", "0", "--json", RIDLEY]);
    let (stdout, stderr) = text(&weak);
    assert!(weak.status.success(), "{stdout}{stderr}");
    let doc: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!("json: {err}\n{stdout}{stderr}");
    });
    let hits = doc["hits"].as_array().unwrap_or_else(|| panic!("{doc}"));
    assert!(
        hits.iter()
            .any(|hit| hit["text"].as_str().unwrap_or("").contains("JOSS")),
        "min-score 0 should show the weak lesson: {doc}"
    );
    let score = hits
        .iter()
        .find(|hit| hit["text"].as_str().unwrap_or("").contains("JOSS"))
        .and_then(|hit| hit["score"].as_f64())
        .unwrap_or(0.0);
    assert!(
        (score - 0.33).abs() < 0.01,
        "the reported word score is 0.33, got {score}"
    );

    let narrated = seat.run(&["search", "GPRidge video narration"]);
    let (stdout, stderr) = text(&narrated);
    assert!(narrated.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("narrated video"), "{stdout}{stderr}");
    assert!(!stdout.contains("JOSS"), "{stdout}");
    assert!(!stdout.contains("No relevant matches"), "{stdout}");

    let partial = seat.run(&["search", "narrated video"]);
    let (stdout, stderr) = text(&partial);
    assert!(partial.status.success(), "{stdout}{stderr}");
    assert!(
        stdout.contains(NARRATED) || stdout.contains("narrated video"),
        "{stdout}{stderr}"
    );

    let exact = seat.run(&["search", NARRATED]);
    let (stdout, stderr) = text(&exact);
    assert!(exact.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("narrated"), "{stdout}{stderr}");

    let kiln = seat.run(&["search", "kiln damper"]);
    let (stdout, stderr) = text(&kiln);
    assert!(kiln.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("damper"), "{stdout}{stderr}");
}

#[test]
fn json_names_the_floor_and_help_names_the_flag() {
    let seat = Seat::open();
    let out = seat.run(&["search", "--json", RIDLEY]);
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stdout}{stderr}");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect(&stdout);
    assert!(
        doc["hits"].as_array().is_some_and(|rows| rows.is_empty()),
        "{doc}"
    );
    let reading = doc["reading"].as_str().unwrap_or("");
    assert!(reading.contains("No relevant matches for"), "{doc}");
    assert!(reading.contains("--min-score 0"), "{doc}");
    let suggestion = doc["suggestion"].as_str().unwrap_or("");
    assert!(suggestion.contains("--min-score 0"), "{doc}");

    let help = seat.run(&["search", "--help"]);
    let (stdout, stderr) = text(&help);
    assert!(help.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("--min-score"), "{stdout}");
    assert!(stdout.contains("0.34"), "{stdout}");
}

#[test]
fn a_score_below_zero_is_refused() {
    let seat = Seat::open();
    let out = seat.run(&["search", "--min-score=-1", "kiln"]);
    let (stdout, stderr) = text(&out);
    assert!(!out.status.success(), "{stdout}{stderr}");
    assert!(
        stderr.contains("--min-score") || stdout.contains("--min-score"),
        "{stdout}{stderr}"
    );
}
