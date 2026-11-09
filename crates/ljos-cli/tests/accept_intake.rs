//! Several proposal ids in one `ljos accept`. Each id gets a line, the
//! proposal log records the outcome, and a second run says the same thing.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

const KEPT: &str = "Pin the north fuse before the dusk firing.";
const ALSO: &str = "Name the relay key before publishing the pack.";
const HARD: &str = "One. Two. Three.";

fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let body = if req.contains("POST /v1/atoms") {
                r#"{"id":"atom-kept","kind":"lesson","text":"kept","origin":"agent-derived","level":"explicit"}"#
            } else if req.contains("GET /v1/atoms") {
                "[]"
            } else {
                "{}"
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
}

impl Seat {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let pack = serve();
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

    fn plant(&self, id: &str, text: &str, seat: Option<&str>) {
        let path = self.proposals_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut row = serde_json::json!({
            "id": id,
            "status": "open",
            "remote": false,
            "text": text,
            "kind": "lesson",
            "origin": "agent-derived",
            "atom": {"kind": "lesson", "text": text, "level": "derived", "origin": "agent-derived"},
        });
        if let Some(seat) = seat {
            row["seat"] = serde_json::Value::String(seat.to_string());
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{row}").unwrap();
    }

    fn latest(&self) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(self.proposals_path()).unwrap_or_default();
        let mut rows = std::collections::BTreeMap::new();
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let Some(id) = value["id"].as_str() {
                rows.insert(id.to_string(), value);
            }
        }
        rows.into_values().collect()
    }
}

#[test]
fn several_ids_are_accepted_once_and_the_rerun_matches() {
    let seat = Seat::new();
    seat.plant("aaaa1111bbbb2222", KEPT, Some("ada"));
    seat.plant("cccc3333dddd4444", ALSO, None);
    seat.plant("eeee5555ffff6666", HARD, Some("ada"));

    std::env::set_var("XDG_STATE_HOME", seat.root.path().join("home/.state"));
    std::env::set_var("LJOS_SEAT", "ada");
    let filed = ljos_cli::admit::file_open("File the kiln note before dusk.", "lesson");
    assert!(filed.is_ok(), "{filed:?}");

    let listed = seat.ljos(&["proposals"]);
    assert!(listed.contains("seat ada"), "{listed}");
    assert!(listed.contains("seat unknown"), "{listed}");
    assert!(
        listed.contains("File the kiln note before dusk."),
        "{listed}"
    );

    let only = seat.ljos(&["proposals", "--seat", "ada", "--status", "open"]);
    assert!(only.contains(KEPT), "{only}");
    assert!(!only.contains(ALSO), "{only}");
    assert!(only.contains("seat ada"), "{only}");

    let first = seat.ljos(&["accept", "aaaa1111", "cccc3333", "eeee5555", "deadbeef"]);
    assert!(first.contains("accepted aaaa1111bbbb2222"), "{first}");
    assert!(first.contains("accepted cccc3333dddd4444"), "{first}");
    assert!(
        first.contains("refused eeee5555ffff6666:") && first.contains("3") && first.contains("2"),
        "{first}"
    );
    assert!(first.contains("not found deadbeef"), "{first}");

    let rows = seat.latest();
    let kept = rows.iter().find(|r| r["id"] == "aaaa1111bbbb2222").unwrap();
    assert_eq!(kept["status"], "accepted", "{kept}");
    assert!(kept["at"].as_str().unwrap_or("").contains('T'), "{kept}");
    let hard = rows.iter().find(|r| r["id"] == "eeee5555ffff6666").unwrap();
    assert_eq!(hard["status"], "refused", "{hard}");
    assert!(
        hard["reason"].as_str().unwrap_or("").contains("3"),
        "{hard}"
    );
    let filed_row = rows
        .iter()
        .find(|r| r["text"] == "File the kiln note before dusk.")
        .unwrap();
    assert_eq!(filed_row["seat"], "ada", "{filed_row}");

    let again = seat.ljos(&["accept", "aaaa1111", "cccc3333", "eeee5555", "deadbeef"]);
    assert!(
        again.contains("already in pack aaaa1111bbbb2222"),
        "{again}"
    );
    assert!(
        again.contains("already in pack cccc3333dddd4444"),
        "{again}"
    );
    assert!(again.contains("refused eeee5555ffff6666:"), "{again}");
    assert!(again.contains("not found deadbeef"), "{again}");
    let rows_again = seat.latest();
    assert_eq!(
        rows_again
            .iter()
            .find(|r| r["id"] == "aaaa1111bbbb2222")
            .unwrap()["status"],
        "accepted"
    );
    assert_eq!(
        rows_again
            .iter()
            .find(|r| r["id"] == "eeee5555ffff6666")
            .unwrap()["status"],
        "refused"
    );
}
