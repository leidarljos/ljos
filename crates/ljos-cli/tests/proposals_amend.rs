//! A refused proposal can be amended. The original text stays, accept
//! stores the revision, and a repeat with no new amendment does not check
//! again.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

const SHORT: &str = "Pin the north fuse before the dusk firing.";
const LONG: &str = "git push needs to run inside the worktree before the pack is published and the seat is named in the log so the other machine can pull the same branch without a second.";

fn serve(posted: std::path::PathBuf) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let body = if req.contains("POST /v1/atoms") {
                if let Some(body) = req.split("\r\n\r\n").nth(1) {
                    let _ = std::fs::write(&posted, body);
                }
                r#"{"id":"atom-kept","kind":"lesson","text":"kept","origin":"agent-derived"}"#
                    .to_string()
            } else if req.contains("GET /v1/atoms") {
                r#"{"atoms":[]}"#.to_string()
            } else {
                "{}".to_string()
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

struct Seat {
    root: tempfile::TempDir,
    pack: String,
    posted: std::path::PathBuf,
}

impl Seat {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let posted = root.path().join("posted.json");
        let pack = serve(posted.clone());
        Self { root, pack, posted }
    }

    fn ljos(&self, args: &[&str]) -> String {
        self.run(args, None)
    }

    fn ljos_stdin(&self, args: &[&str], stdin: &str) -> String {
        self.run(args, Some(stdin))
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> String {
        let home = self.root.path().join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("LJOS_SEAT", "ada")
            .env("PACKSET_URL", &self.pack)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "4000")
            .env("LJOS_TRACKER_GIT", "off");
        if stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(text) = stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        }
        let out = child.wait_with_output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{args:?}\n{text}");
        text
    }

    fn plant(&self, id: &str, text: &str) {
        let path = self.root.path().join("home/.state/ljos/proposals.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let row = serde_json::json!({
            "id": id,
            "status": "open",
            "remote": false,
            "text": text,
            "kind": "lesson",
            "origin": "agent-derived",
            "seat": "ada",
            "atom": {"kind": "lesson", "text": text, "level": "derived", "origin": "agent-derived"},
        });
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{row}").unwrap();
    }

    fn lines(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.path().join("home/.state/ljos/proposals.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn quote_of(line: &str) -> String {
    let start = line.find('"').expect(line);
    let rest = &line[start + 1..];
    let end = rest.find('"').expect(line);
    rest[..end].to_string()
}

#[test]
fn an_amendment_that_passes_is_what_accept_stores() {
    let seat = Seat::new();
    seat.plant("aaaa1111bbbb2222", LONG);
    let amended = seat.ljos_stdin(&["proposals", "amend", "aaaa1111", "--text", "-"], SHORT);
    assert!(
        amended.contains("ok aaaa1111bbbb2222: passes (ready for ljos accept)"),
        "{amended}"
    );
    let rows = seat.lines();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows[0].contains(LONG), "{rows:?}");
    assert!(
        rows[1].contains(LONG),
        "the original text was replaced: {rows:?}"
    );
    assert!(rows[1].contains(SHORT), "{rows:?}");
    assert!(rows[1].contains("\"seat\":\"ada\""), "{rows:?}");

    let accepted = seat.ljos(&["accept", "aaaa1111"]);
    assert!(accepted.contains("accepted aaaa1111bbbb2222"), "{accepted}");
    let posted = std::fs::read_to_string(&seat.posted).unwrap();
    assert!(posted.contains(SHORT), "{posted}");
    assert!(!posted.contains(LONG), "{posted}");
    assert!(posted.contains("provenance:amended"), "{posted}");
}

#[test]
fn an_amendment_that_is_still_too_long_quotes_the_sentence() {
    let seat = Seat::new();
    seat.plant("bbbb2222cccc3333", SHORT);
    let amended = seat.ljos(&["proposals", "amend", "bbbb2222", "--text", LONG]);
    let line = amended
        .lines()
        .find(|line| line.starts_with("refused bbbb2222"))
        .unwrap_or_else(|| panic!("{amended}"));
    assert!(
        line.contains("sentence has 33 words; 33 vs limit 29:"),
        "{line}"
    );
    let quote = quote_of(line);
    assert!(quote.starts_with("git push needs to run inside"), "{quote}");
    assert!(quote.ends_with("..."), "{quote}");
    assert!(quote.chars().count() <= 80, "{quote}");

    let listed = seat.ljos(&["proposals", "--status", "refused"]);
    assert!(listed.contains("sentence has 33 words"), "{listed}");
    assert!(listed.contains("git push needs to run inside"), "{listed}");
    assert!(
        listed.contains("amend it with ljos proposals amend"),
        "{listed}"
    );
    let rows = seat.lines();
    assert!(rows[0].contains(SHORT), "{rows:?}");
    assert!(rows.last().unwrap().contains(SHORT), "{rows:?}");
}

#[test]
fn a_repeated_accept_of_an_unchanged_refusal_does_not_check_again() {
    let seat = Seat::new();
    seat.plant("cccc3333dddd4444", LONG);
    let first = seat.ljos(&["accept", "cccc3333"]);
    assert!(
        first.contains("refused cccc3333dddd4444:") && first.contains("33") && first.contains("29"),
        "{first}"
    );
    let after = seat.lines().len();
    let again = seat.ljos(&["accept", "cccc3333"]);
    assert!(
        again.contains("unchanged since last refusal at "),
        "{again}"
    );
    assert!(
        again.contains("amend it with ljos proposals amend"),
        "{again}"
    );
    assert!(
        !again.contains("sentence has 33 words"),
        "the check ran again: {again}"
    );
    assert_eq!(seat.lines().len(), after, "a repeat wrote a row");
    assert!(!seat.posted.exists(), "a refused accept posted the atom");
}
