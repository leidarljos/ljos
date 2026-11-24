//! Failures a new seat hits name the next command, in the text the command prints.
//!
//! A writer for another home on this seat's port answers 401. Search, doctor
//! and onboard name the command that starts this home's writer. A search that
//! shares no word with what is stored prints the no-hits line and does not
//! claim the rows were matched by words.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const FIX: &str = "run `packset stop && packset ensure` or set PACKSET_PORT to a free port and run `packset ensure`";

struct Seat {
    home: tempfile::TempDir,
    url: String,
}

fn respond(sock: &mut std::net::TcpStream, code: u16, body: &str) {
    let msg = format!(
        "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(msg.as_bytes());
}

fn read_head(sock: &mut std::net::TcpStream) -> String {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Health answers. Every other route refuses this home's token.
fn serve_refused() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            thread::spawn(move || {
                let mut sock = conn;
                let head = read_head(&mut sock);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                if path.starts_with("/health") {
                    respond(&mut sock, 200, "packsetd");
                } else {
                    respond(&mut sock, 401, r#"{"error":"token"}"#);
                }
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// The encoder is not installed. Search of "which fusion" has no hit.
/// The stored lesson shares no word with that query.
fn serve_words() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            thread::spawn(move || {
                let mut sock = conn;
                let head = read_head(&mut sock);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or(path);
                let query = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .split('?')
                    .nth(1)
                    .unwrap_or("");
                let body = if path == "/v1/status" {
                    r#"{"embedder":{"enabled":true,"binary":null,"available":false,"answering":null},"live":1,"live_cap":0}"#.to_string()
                } else if path == "/v1/search" && query.contains("lexical") {
                    r#"{"hits":[{"id":"lex","kind":"lesson","text":"The lexical default is BM25+.","score":0.8,"ts":"2026-01-01T00:00:00.000Z","entities":["horizon:standing","seat:travel"],"ballots":2,"of":2}],"rank":"fused"}"#.to_string()
                } else if path == "/v1/search" {
                    r#"{"hits":[],"rank":"fused"}"#.to_string()
                } else if path == "/v1/atoms" {
                    r#"{"atoms":[{"id":"lex","kind":"lesson","text":"The lexical default is BM25+.","ts":"2026-01-01T00:00:00.000Z","entities":["horizon:standing","seat:travel"]}]}"#.to_string()
                } else {
                    r#"{"ok":true}"#.to_string()
                };
                respond(&mut sock, 200, &body);
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn seat(url: &str) -> Seat {
    let home = tempfile::tempdir().unwrap();
    for dir in [".config", ".local/share", ".cache", "run", "tmp", "bin"] {
        std::fs::create_dir_all(home.path().join(dir)).unwrap();
    }
    let curl = home.path().join("bin/curl");
    ljos_cli::install_executable(
        &curl,
        "#!/bin/sh\necho 'curl: network is unreachable' >&2\nexit 6\n",
    )
    .unwrap();
    Seat {
        home,
        url: url.to_string(),
    }
}

fn run(seat: &Seat, args: &[&str]) -> (i32, String, String) {
    let home = seat.home.path();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", home.join("bin").display()),
        )
        .env("HOME", home)
        .env("USER", "ada")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home.join("tmp"))
        .env("PACKSET_URL", &seat.url)
        .env("PACKSET_TOKEN", "seat-token")
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("LJOS_SEARCH_WAIT", "0")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("LJOS_HOOKD", "off")
        .env("NO_COLOR", "1")
        .env("LJOS_SEAT", "travel")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn host(url: &str) -> &str {
    url.trim_start_matches("http://").trim_end_matches('/')
}

#[test]
fn a_writer_for_another_home_names_the_port_command() {
    let seat = seat(&serve_refused());
    let where_ = host(&seat.url);
    let search_line =
        format!("search: the writer on {where_} answered 401 for this home, so {FIX}.");
    let (code, stdout, stderr) = run(&seat, &["search", "lexical default"]);
    let said = format!("{stdout}{stderr}");
    assert_eq!(code, 1, "{said}");
    assert_eq!(stderr.trim(), search_line, "{said}");
    assert!(!stdout.contains("No hits"), "{said}");
    assert!(!said.contains("status code 401"), "{said}");

    let doctor_line = format!(
        "warn\tsearch\tthe writer answered 401 on /v1/search; recall is using stored memories, so {FIX}."
    );
    let (code, stdout, stderr) = run(&seat, &["doctor"]);
    let said = format!("{stdout}{stderr}");
    assert!(
        stdout.lines().any(|line| line == doctor_line),
        "doctor exited {code}\n{said}"
    );
    assert!(!stdout.contains("until that read is accepted"), "{said}");

    let (code, stdout, stderr) = run(
        &seat,
        &["onboard", "--harness", "example-host", "--dry-run"],
    );
    let said = format!("{stdout}{stderr}");
    let onboard_line =
        format!("no\tmemory\tthe writer on {where_} answered 401 for this home, so {FIX}.");
    assert!(
        stdout.lines().any(|line| line == onboard_line),
        "onboard exited {code}\n{said}"
    );
    assert!(!said.contains("memory writer is up"), "{said}");
    assert_ne!(
        code, 0,
        "onboard reported success while the writer refused this home\n{said}"
    );
}

#[test]
fn a_search_with_no_shared_word_prints_only_the_no_hits_line() {
    let seat = seat(&serve_words());
    let (code, stdout, stderr) = run(&seat, &["search", "which fusion"]);
    let said = format!("{stdout}{stderr}");
    assert_eq!(code, 0, "{said}");
    assert_eq!(
        stdout,
        "Nothing matched \"which fusion\". Try a shorter query, such as: ljos search fusion\n",
        "{said}"
    );
    assert!(
        !stderr.contains(ljos_cli::FUSED_RANK_LINE) && !stderr.contains("matched by words"),
        "an empty page prints no match-mode line:\n{said}"
    );
    assert!(
        !stderr.contains("encoder not installed") && !stderr.contains("cargo binstall"),
        "the encoder install line is not the next command when nothing matched:\n{said}"
    );

    let (code, stdout, stderr) = run(&seat, &["search", "lexical default"]);
    let said = format!("{stdout}{stderr}");
    assert_eq!(code, 0, "{said}");
    assert!(
        stderr.contains(
            "matched by words (encoder not installed), so read https://leidarljos.github.io/ljos/getting-started.html#install or run `ljos doctor`."
        ),
        "a page that shows rows keeps the install line:\n{said}"
    );
    assert!(stdout.contains("The lexical default is BM25+."), "{said}");
}
