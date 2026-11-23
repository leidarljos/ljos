//! A ticket this seat claimed, whose body and notes contain the query,
//! ranks above other projects' tickets that only share a generic word.
//!
//! The encoder ranks titles. `sign-in` then matches every sign-in title,
//! and the claimed ticket, whose title does not say those words, used to
//! fall off the page. The header says this seat's rows come first.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

const QUERY: &str = "usglobalmail sign-in preflight";
const OURS: &str = "wanderlog-mcp-n9dr";
const OTHERS: &[&str] = &[
    "Software-7o1a",
    "Software-8umw",
    "Software-vawq",
    "Software-yjda",
];

const ISSUES: &str = "\
#+TITLE: Software issues
#+VISSUE: 1
#+CATEGORY: Software
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] USGM archive
:PROPERTIES:
:ID:         wanderlog-mcp-n9dr
:CLAIMED_BY: alice
:END:
:LOGBOOK:
- Note: \"usglobalmail sign-in preflight noted today\" [2026-11-16 Mon 09:00]
:END:
The usglobalmail sign-in preflight is the archive check.

* TODO [#B] Post missing Tello listings via the sign-in browser
:PROPERTIES:
:ID:         Software-7o1a
:CLAIMED_BY: bob
:END:

* TODO [#C] Referral codes on the sign-in page
:PROPERTIES:
:ID:         Software-8umw
:CLAIMED_BY: bob
:END:

* TODO [#B] Odysee sign-in link relay
:PROPERTIES:
:ID:         Software-vawq
:CLAIMED_BY: bob
:END:

* TODO [#C] Dimer paper sign-in text
:PROPERTIES:
:ID:         Software-yjda
:CLAIMED_BY: bob
:END:
";

struct Seat {
    root: PathBuf,
    url: String,
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
            "tracker/Software/Software",
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
        std::fs::write(root.join("tracker/Software/Software/issues.org"), ISSUES).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || serve(listener));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
        }
    }

    fn search(&self, args: &[&str]) -> Output {
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
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
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
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")));
    }
}

fn hit(id: &str, text: &str, score: &str, rank: u32) -> String {
    format!(
        r#"{{"id":"{id}","kind":"ticket","text":"{text}","score":{score},"cosine":0.91,"dense_rank":{rank},"entities":["seat:bob"],"ballots":1,"of":1}}"#
    )
}

fn serve(listener: TcpListener) {
    let hits = format!(
        r#"{{"hits":[{},{},{},{}],"rank":"relevance"}}"#,
        hit(
            "Software-7o1a",
            "Post missing Tello listings via the sign-in browser",
            "0.93",
            1
        ),
        hit(
            "Software-8umw",
            "Referral codes on the sign-in page",
            "0.88",
            2
        ),
        hit("Software-vawq", "Odysee sign-in link relay", "0.84", 3),
        hit("Software-yjda", "Dimer paper sign-in text", "0.80", 4),
    );
    let status = r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#;
    for conn in listener.incoming().flatten() {
        let hits = hits.clone();
        let status = status.to_string();
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
            let body = if path == "/v1/search" {
                hits
            } else if path == "/v1/status" {
                status
            } else if path == "/v1/atoms" {
                r#"{"atoms":[{"id":"lesson-1","kind":"lesson","text":"kept"}]}"#.to_string()
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

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_claimed_ticket_with_matching_notes_outranks_generic_sign_in_titles() {
    let seat = Seat::open();
    let out = seat.search(&["search", QUERY, "-n", "4"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(ljos_cli::FUSED_RANK_LINE),
        "the page was not ranked by meaning:\n{said}"
    );
    assert!(
        stdout.contains("This seat's rows come first."),
        "the header does not say what the order does:\n{said}"
    );
    let ours = stdout
        .find(OURS)
        .unwrap_or_else(|| panic!("the claimed ticket was not on the page:\n{said}"));
    assert!(
        stdout.contains("USGM archive"),
        "the row did not keep the title:\n{said}"
    );
    let mut seen_other = false;
    for id in OTHERS {
        if let Some(at) = stdout.find(id) {
            seen_other = true;
            assert!(ours < at, "{id} ranked above the claimed ticket:\n{said}");
        }
    }
    assert!(
        seen_other,
        "an other project ticket should still be on the page, below the claim:\n{said}"
    );
}
