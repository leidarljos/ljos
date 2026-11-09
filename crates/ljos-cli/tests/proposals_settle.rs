//! Open proposals the pack already holds are marked, then settled, and a
//! readability refusal names the sentence.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

const HELD: &str = "Pin the north fuse before the dusk firing.";
const FRESH: &str = "Name the relay key before publishing the pack.";
const LONG: &str = "git push needs to run inside the worktree before the pack is published and the seat is named in the log so the other machine can pull the same branch without a second.";
const HARD_REASON: &str = "atom sentence 1 is very hard to read: 33 words, the limit is 29; split it at a conjunction or drop a clause";

fn serve(atoms: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let body = if req.contains("GET /v1/atoms") {
                atoms.clone()
            } else if req.contains("POST /v1/atoms") {
                r#"{"id":"atom-kept","kind":"lesson","text":"kept","origin":"agent-derived"}"#
                    .to_string()
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

fn held_atoms() -> String {
    serde_json::json!({
        "atoms": [{
            "id": "atom-held",
            "kind": "lesson",
            "text": HELD,
            "origin": "agent-derived"
        }]
    })
    .to_string()
}

struct Seat {
    root: tempfile::TempDir,
    pack: String,
}

impl Seat {
    fn new(atoms: String) -> Self {
        let root = tempfile::tempdir().unwrap();
        let pack = serve(atoms);
        Self { root, pack }
    }

    fn ljos(&self, args: &[&str]) -> String {
        let home = self.root.path().join("home");
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
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
            .env("LJOS_TRACKER_GIT", "off")
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{args:?}\n{text}");
        text
    }

    fn proposals_path(&self) -> std::path::PathBuf {
        self.root.path().join("home/.state/ljos/proposals.jsonl")
    }

    fn plant(&self, id: &str, text: &str, status: &str, reason: &str) {
        let path = self.proposals_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let row = serde_json::json!({
            "id": id,
            "status": status,
            "remote": false,
            "text": text,
            "kind": "lesson",
            "origin": "agent-derived",
            "seat": "ada",
            "reason": reason,
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
        std::fs::read_to_string(self.proposals_path())
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
fn an_open_proposal_already_in_the_pack_is_marked_and_counted() {
    let seat = Seat::new(held_atoms());
    seat.plant("aaaa1111bbbb2222", HELD, "open", "");
    seat.plant("bbbb2222cccc3333", FRESH, "open", "");

    let listed = seat.ljos(&["proposals"]);
    assert!(listed.contains(FRESH), "{listed}");
    assert!(!listed.contains(HELD), "{listed}");
    assert!(
        listed.contains("2 open: 1 new, 1 already in pack (`--status in-pack` to list them)"),
        "{listed}"
    );
    let fresh_at = listed.find(FRESH).unwrap();
    let footer_at = listed.find("2 open:").unwrap();
    assert!(fresh_at < footer_at, "{listed}");

    let packed = seat.ljos(&["proposals", "--status", "in-pack"]);
    assert!(packed.contains("aaaa1111bbbb2222"), "{packed}");
    assert!(packed.contains("already in pack"), "{packed}");
    assert!(packed.contains(HELD), "{packed}");
    assert!(!packed.contains(FRESH), "{packed}");
}

#[test]
fn settle_records_the_outcome_once_and_deletes_nothing() {
    let seat = Seat::new(held_atoms());
    seat.plant("aaaa1111bbbb2222", HELD, "open", "");
    seat.plant("bbbb2222cccc3333", FRESH, "open", "");
    let before = seat.lines().len();

    let first = seat.ljos(&["proposals", "--settle"]);
    assert!(
        first.contains("already in pack aaaa1111bbbb2222"),
        "{first}"
    );
    assert!(first.contains("settled 1"), "{first}");
    assert!(!first.contains("bbbb2222cccc3333"), "{first}");
    let after = seat.lines();
    assert_eq!(after.len(), before + 1, "{after:?}");
    assert!(
        after
            .iter()
            .any(|line| line.contains("aaaa1111bbbb2222") && line.contains("\"open\"")),
        "{after:?}"
    );
    let settled = after
        .iter()
        .rev()
        .find(|line| line.contains("aaaa1111bbbb2222"))
        .unwrap();
    assert!(settled.contains("\"already-in-pack\""), "{settled}");
    assert!(settled.contains('T'), "{settled}");
    assert!(
        after
            .iter()
            .any(|line| line.contains("bbbb2222cccc3333") && line.contains("\"open\"")),
        "{after:?}"
    );

    let second = seat.ljos(&["proposals", "--settle"]);
    assert!(second.contains("settled 0"), "{second}");
    assert!(!second.contains("already in pack"), "{second}");
    assert_eq!(
        seat.lines().len(),
        after.len(),
        "a second settle wrote a row"
    );
}

#[test]
fn a_readability_refusal_quotes_the_sentence() {
    let seat = Seat::new(r#"{"atoms":[]}"#.to_string());
    seat.plant("fd17ee67aaaa1111", LONG, "refused", HARD_REASON);

    let listed = seat.ljos(&["proposals", "--status", "refused"]);
    let line = listed
        .lines()
        .find(|line| line.contains("fd17ee67aaaa1111"))
        .unwrap_or_else(|| panic!("{listed}"));
    assert!(line.contains("sentence has 33 words; limit 29:"), "{line}");
    let quote = quote_of(line);
    assert!(quote.starts_with("git push needs to run inside"), "{quote}");
    assert!(quote.ends_with("..."), "{quote}");
    assert!(quote.chars().count() <= 80, "{quote}");

    let refused = seat.ljos(&["accept", "fd17ee67"]);
    let line = refused
        .lines()
        .find(|line| line.starts_with("refused fd17ee67"))
        .unwrap_or_else(|| panic!("{refused}"));
    assert!(line.contains("sentence has 33 words; limit 29:"), "{line}");
    let quote = quote_of(line);
    assert!(quote.starts_with("git push needs to run inside"), "{quote}");
    assert!(quote.ends_with("..."), "{quote}");
    assert!(quote.chars().count() <= 80, "{quote}");
}
