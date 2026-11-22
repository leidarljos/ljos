//! `ljos show` opens a memory by the id search and remember print.
//!
//! The full atom id, its eight hex characters, and the `local:` form each
//! open one memory. A shared prefix lists the candidates. A retired memory
//! stays readable. `--json` prints the same record. A ticket id still
//! opens the ticket.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

struct Home {
    root: PathBuf,
    url: String,
    full: String,
    other: String,
    alone: String,
    local_pack: String,
    local_disk: String,
    retired: String,
    later: String,
}

impl Home {
    fn open() -> Self {
        let full = format!("{}{}", "01234567", "89abcdef".repeat(7));
        let other = format!("{}{}", "01234567", "fedcba98".repeat(7));
        let alone = format!("{}{}", "89abcdef", "01234567".repeat(7));
        let local_pack = format!("local:f0ceee3d{}", "ab".repeat(28));
        let local_disk = format!("local:cafebabe{}", "cd".repeat(28));
        let retired = format!("{}{}", "abcdef01", "23456789".repeat(7));
        let later = format!("{}{}", "abcdef02", "23456789".repeat(7));
        let lesson = "The kiln holds a long lesson about the lexical default and the fuse that beat BM25 by two points on the turns the panel measured.";
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "run",
            "work",
            "tracker/Software/demo",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            root.join("tracker/Software/demo/issues.org"),
            "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#B] Fire the kiln
:PROPERTIES:
:ID:         demo-kiln
:CREATED:    [2026-06-01 Mon]
:END:
",
        )
        .unwrap();
        let kept = root.join("tracker/.ljos/memory/kept");
        std::fs::create_dir_all(&kept).unwrap();
        let disk_name = local_disk.trim_start_matches("local:");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            kept.join(format!("{disk_name}.json")),
            json!({
                "id": local_disk,
                "text": "A lesson kept on this machine.",
                "kind": "lesson",
                "origin": "agent-derived",
                "basis": "agent",
                "entities": ["seat:ada", "issue:proj-9c3d"],
                "seat": "ada",
                "at": "2026-11-02T08:30:00Z",
                "retired": false
            })
            .to_string(),
        )
        .unwrap();
        let atoms = json!({
            "atoms": [
                record(&full, lesson, "explicit", "user-declared", false, json!([later.clone()]), json!([])),
                record(&other, "Another lesson shares the prefix.", "explicit", "user-declared", false, json!([]), json!([])),
                record(&alone, "This lesson has its own prefix.", "explicit", "user-declared", false, json!([]), json!([])),
                record(&local_pack, "The local pack row stays whole.", "derived", "agent-derived", false, json!([]), json!([])),
                record(&retired, "The old fuse was Borda.", "derived", "agent-derived", true, json!([later.clone()]), json!([])),
                record(&later, "The fuse is CombMNZ.", "explicit", "user-declared", false, json!([]), json!([full.clone(), retired.clone()])),
            ]
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || serve(listener, atoms));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
            full,
            other,
            alone,
            local_pack,
            local_disk,
            retired,
            later,
        }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let home = self.root.join("home");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("TZ", "UTC")
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")));
    }
}

fn record(
    id: &str,
    text: &str,
    level: &str,
    origin: &str,
    retired: bool,
    links: Value,
    supersedes: Value,
) -> Value {
    json!({
        "id": id,
        "kind": "lesson",
        "level": level,
        "origin": origin,
        "text": text,
        "tombstone": retired,
        "entities": ["seat:alice", "issue:proj-1a2b"],
        "source": {
            "seat": "alice",
            "session": "sess-1",
            "at": "2026-11-01T12:00:00Z"
        },
        "links": links,
        "supersedes": supersedes
    })
}

fn serve(listener: TcpListener, atoms: Value) {
    for conn in listener.incoming().flatten() {
        let atoms = atoms.clone();
        thread::spawn(move || {
            let mut sock = conn;
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            while !buf.windows(4).any(|window| window == b"\r\n\r\n") {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                }
            }
            let head_end = buf
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap()
                + 4;
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let request = head.split_whitespace().nth(1).unwrap_or("/");
            let path = request.split('?').next().unwrap_or("/");
            let payload = if path == "/v1/atoms" {
                atoms.to_string()
            } else {
                r#"{"ok":true}"#.to_string()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        });
    }
}

fn said(out: &std::process::Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn page(home: &Home, id: &str) -> String {
    let out = home.run(&["show", id]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(out.stderr.is_empty(), "{text}");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn shows_lesson(page: &str, text: &str) {
    assert!(page.contains(text), "{page}");
    assert!(page.contains("kind: lesson"), "{page}");
    assert!(page.contains("seat: alice"), "{page}");
    assert!(page.contains("session: sess-1"), "{page}");
    assert!(page.contains("written: 2026-11-01 12:00 +00:00"), "{page}");
    assert!(page.contains("tickets: proj-1a2b"), "{page}");
    assert!(
        page.lines().all(|line| line.chars().count() <= 80),
        "{page}"
    );
}

#[test]
fn a_full_id_opens_the_memory() {
    let home = Home::open();
    let page = page(&home, &home.full);
    shows_lesson(&page, "The kiln holds a long lesson");
    assert!(page.contains("panel measured."), "{page}");
    assert!(page.contains("level: explicit"), "{page}");
    assert!(page.contains("origin: user-declared"), "{page}");
    assert!(page.contains("accepted: yes"), "{page}");
    assert!(page.contains("retired: no"), "{page}");
    assert!(
        page.contains(&format!("superseded by: {}", home.later)),
        "{page}"
    );
    assert!(page.contains(&format!("links: {}", home.later)), "{page}");
}

#[test]
fn a_short_prefix_opens_the_one_memory_it_names() {
    let home = Home::open();
    let page = page(&home, "89abcdef");
    shows_lesson(&page, "This lesson has its own prefix.");
    assert!(page.starts_with(&home.alone), "{page}");
}

#[test]
fn a_local_prefix_and_its_hex_tail_open_the_same_memory() {
    let home = Home::open();
    let from_local = page(&home, "local:f0ceee3d");
    let from_hex = page(&home, "f0ceee3d");
    shows_lesson(&from_local, "The local pack row stays whole.");
    assert_eq!(from_local, from_hex);
    assert!(from_local.contains("level: derived"), "{from_local}");
    assert!(from_local.contains("origin: agent-derived"), "{from_local}");
    assert!(from_local.contains("accepted: no"), "{from_local}");
    assert!(from_local.starts_with(&home.local_pack), "{from_local}");
}

#[test]
fn a_kept_local_id_opens_the_copy_on_disk() {
    let home = Home::open();
    let shown = page(&home, "local:cafebabe");
    assert!(shown.contains("A lesson kept on this machine."), "{shown}");
    assert!(shown.contains("seat: ada"), "{shown}");
    assert!(shown.contains("session: none"), "{shown}");
    assert!(shown.contains("tickets: proj-9c3d"), "{shown}");
    assert!(shown.contains("links: none"), "{shown}");
    assert!(shown.starts_with(&home.local_disk), "{shown}");
    let tail = page(&home, "cafebabe");
    assert_eq!(shown, tail);
}

#[test]
fn an_ambiguous_prefix_lists_the_candidates() {
    let home = Home::open();
    let out = home.run(&["show", "01234567"]);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    let line = String::from_utf8_lossy(&out.stderr);
    assert!(line.contains(&home.full), "{text}");
    assert!(line.contains(&home.other), "{text}");
    assert!(line.contains("`ljos show"), "{text}");
    assert!(out.stdout.is_empty(), "{text}");
}

#[test]
fn a_retired_memory_prints_its_text_and_json() {
    let home = Home::open();
    let page = page(&home, &home.retired);
    shows_lesson(&page, "The old fuse was Borda.");
    assert!(page.contains("accepted: no"), "{page}");
    assert!(page.contains("retired: yes"), "{page}");
    assert!(
        page.contains(&format!("superseded by: {}", home.later)),
        "{page}"
    );
    let out = home.run(&["show", "--json", &home.retired]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let body: Value = serde_json::from_slice(&out.stdout).expect(&text);
    assert_eq!(body["id"], home.retired);
    assert_eq!(body["text"], "The old fuse was Borda.");
    assert_eq!(body["kind"], "lesson");
    assert_eq!(body["level"], "derived");
    assert_eq!(body["origin"], "agent-derived");
    assert_eq!(body["seat"], "alice");
    assert_eq!(body["session"], "sess-1");
    assert_eq!(body["written"], "2026-11-01T12:00:00Z");
    assert_eq!(body["accepted"], false);
    assert_eq!(body["retired"], true);
    assert_eq!(body["superseded"], true);
    assert_eq!(body["superseded_by"][0], home.later);
    assert_eq!(body["tickets"][0], "proj-1a2b");
    assert_eq!(body["links"][0], home.later);
}

#[test]
fn an_unknown_id_names_search_and_ready() {
    let home = Home::open();
    let out = home.run(&["show", "demo-none"]);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).trim(),
        "issue demo-none not found. It is neither a ticket nor a memory. `ljos search` finds a memory. `ljos ready` lists the open tickets."
    );
}

#[test]
fn a_ticket_id_still_opens_the_ticket() {
    let home = Home::open();
    let page = page(&home, "demo-kiln");
    assert!(page.contains("demo-kiln"), "{page}");
    assert!(page.contains("Fire the kiln"), "{page}");
    assert!(page.contains("blockers: none"), "{page}");
    assert!(!page.contains("kind:"), "{page}");
}
