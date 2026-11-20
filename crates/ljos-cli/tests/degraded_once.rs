//! A writer that is starting, busy or down does not empty a read, and the
//! command says so once.
//!
//! Each row of the table is one writer state asked by one command. The
//! rows that read from the local store keep a listing the prompt hook kept
//! on disk, and the writer never answers the pack in those rows.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const PACK_HIT: &str = "pack answered: the kiln schedule is the morning plan";
const LOCAL_HIT: &str = "local store: the kiln schedule is the morning plan";
const QUERY: &str = "kiln schedule";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Writer {
    Answering,
    Starting,
    Busy,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command_ {
    Search,
    Recall,
    Sitting,
}

fn expected_line(writer: Writer) -> Option<&'static str> {
    match writer {
        Writer::Answering => None,
        Writer::Starting => Some(
            "Memory is starting, so these results are ranked by words. If this persists, run `ljos doctor`.",
        ),
        Writer::Busy => Some(
            "Memory is busy, so these results are ranked by words. If this persists, run `ljos doctor`.",
        ),
        Writer::Down => Some(
            "Memory is not running, so these results are ranked by words. If this persists, run `ljos doctor`.",
        ),
    }
}

fn head_of(sock: &mut std::net::TcpStream) -> Option<String> {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let want = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|rest| rest.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while buf.len() < end + want {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    Some(head)
}

fn answer(writer: Writer, head: &str) -> (u16, String) {
    let path = head.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or("/");
    match writer {
        Writer::Answering => match path {
            "/health" => (200, "packsetd ok".into()),
            "/v1/status" => (
                200,
                r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                    .into(),
            ),
            "/v1/search" => (
                200,
                serde_json::json!({
                    "hits": [{
                        "id": "pack-kiln",
                        "kind": "lesson",
                        "text": PACK_HIT,
                        "score": 0.91,
                        "entities": ["seat:travel"],
                        "dense_rank": 1,
                        "cosine": 0.84,
                    }],
                    "rank": "relevance",
                })
                .to_string(),
            ),
            "/v1/atoms" => (200, r#"{"atoms":[]}"#.into()),
            _ => (200, "{}".into()),
        },
        Writer::Starting => {
            if path == "/health" {
                (200, "not-ready".into())
            } else {
                (503, r#"{"error":"the pack is busy while loading"}"#.into())
            }
        }
        Writer::Busy => {
            if path == "/health" {
                (200, "packsetd ok, busy".into())
            } else {
                (
                    503,
                    r#"{"error":"packsetd is busy: memory pressure; try again"}"#.into(),
                )
            }
        }
        Writer::Down => (503, "{}".into()),
    }
}

fn serve(writer: Writer) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    if writer == Writer::Down {
        drop(listener);
        return format!("http://127.0.0.1:{port}");
    }
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            thread::spawn(move || {
                let mut sock = conn;
                let Some(head) = head_of(&mut sock) else {
                    return;
                };
                let (status, body) = answer(writer, &head);
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Seat {
    root: tempfile::TempDir,
    issue: String,
}

impl Seat {
    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }
}

/// A tracker with one ticket titled like the query, the programs a
/// sitting drives as stand-ins, and a listing the hook kept on disk.
fn seat() -> Seat {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    let log = dir.join("calls.log");
    let home = dir.join("home");
    std::fs::create_dir_all(home.join(".cursor")).unwrap();
    std::fs::create_dir_all(dir.join("run")).unwrap();
    std::fs::write(
        home.join(".cursor/mcp.json"),
        r#"{"mcpServers":{"ljos":{"command":"/no/such/ljos-mcp"}}}"#,
    )
    .unwrap();
    let tracker = dir.join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    script(
        dir,
        "vissue",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"probe-sit\",\"title\":\"{QUERY}\",\"state\":\"TODO\"}}' ;;\n  recall) echo noted ;;\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\nesac\nexit 0\n",
            log.display(),
            tracker.display()
        ),
    );
    script(
        dir,
        "claimdag",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"claimdag $*\" >> '{}'\ncase \"$1\" in\n  get) exit 1 ;;\nesac\necho '[]'\nexit 0\n",
            log.display()
        ),
    );
    script(
        dir,
        "deedar",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"deedar $*\" >> '{}'\necho head\nexit 0\n",
            log.display()
        ),
    );
    let layout = vissue_core::Layout::new(&tracker, "Software");
    vissue_core::ops::create(&layout, "probe", QUERY, Default::default()).unwrap();
    let doc =
        vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe")).unwrap();
    let issue = doc.headings[0].id.clone();
    let share = dir.join("data/ljos");
    std::fs::create_dir_all(&share).unwrap();
    let kept = serde_json::json!({
        "stamp": "an-older-write",
        "trusted": true,
        "atoms": [{
            "id": "local-kiln",
            "kind": "lesson",
            "text": LOCAL_HIT,
            "entities": ["seat:travel"],
            "ts": "2026-11-01T08:00:00Z",
        }],
    });
    std::fs::write(
        share.join("listing-seat@travel-open.json"),
        kept.to_string(),
    )
    .unwrap();
    Seat { root, issue }
}

fn run(seat: &Seat, url: &str, command: Command_) -> (Output, Duration) {
    run_with(seat, url, command, &[])
}

fn run_with(
    seat: &Seat,
    url: &str,
    command: Command_,
    extra: &[(&str, &str)],
) -> (Output, Duration) {
    let dir = seat.root.path();
    let home = seat.home();
    let path = format!("{}:/usr/bin:/bin", dir.display());
    let args: Vec<&str> = match command {
        Command_::Search => vec!["search", QUERY],
        Command_::Recall => vec!["recall", seat.issue.as_str()],
        Command_::Sitting => vec!["sitting", seat.issue.as_str()],
    };
    let started = Instant::now();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("VISSUE_ROOT", dir.join("tracker"))
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "travel")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_SEARCH_WARMUP_WAIT", "1")
        .env("LJOS_SEARCH_WAIT", "0")
        .env("LJOS_SEARCH_BUSY_WAIT", "1")
        .env("NO_COLOR", "1")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TOKEN", "seat-token")
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_URL", url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let out = cmd.output().unwrap();
    (out, started.elapsed())
}

fn stderr_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn check(writer: Writer, command: Command_) {
    let seat = seat();
    let url = serve(writer);
    let (out, took) = run(&seat, &url, command);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let lines = stderr_lines(&out);
    let said = format!(
        "{writer:?} {command:?} took {took:?} status {:?}\nstdout:\n{stdout}\nstderr:\n{}",
        out.status.code(),
        lines.join("\n")
    );
    assert!(out.status.success(), "{said}");
    let hit = if writer == Writer::Answering {
        PACK_HIT
    } else {
        LOCAL_HIT
    };
    if writer == Writer::Answering && command != Command_::Search {
        // The fake writer lists no atoms, so recall and a sitting find the
        // hit through the search it answers.
        assert!(stdout.contains(PACK_HIT), "{said}");
    } else {
        assert!(stdout.contains(hit), "{said}");
    }
    match expected_line(writer) {
        None => assert!(
            lines
                .iter()
                .all(|line| !line.contains("Memory is") && !line.contains("ranked by words")),
            "{said}"
        ),
        Some(line) => {
            assert_eq!(lines.len(), 1, "{said}");
            assert_eq!(lines[0], line, "{said}");
        }
    }
    assert!(
        !stdout.contains("Memory is starting")
            && !stdout.contains("pack busy")
            && !stdout.contains("ranked by words")
            && !stdout.contains("word matches"),
        "{said}"
    );
    assert!(took < Duration::from_secs(8), "{said}");
}

macro_rules! cell {
    ($name:ident, $writer:expr, $command:expr) => {
        #[test]
        fn $name() {
            check($writer, $command);
        }
    };
}

cell!(search_answering, Writer::Answering, Command_::Search);
cell!(search_starting, Writer::Starting, Command_::Search);
cell!(search_busy, Writer::Busy, Command_::Search);
cell!(search_down, Writer::Down, Command_::Search);
cell!(recall_answering, Writer::Answering, Command_::Recall);
cell!(recall_starting, Writer::Starting, Command_::Recall);
cell!(recall_busy, Writer::Busy, Command_::Recall);
cell!(recall_down, Writer::Down, Command_::Recall);
cell!(sitting_answering, Writer::Answering, Command_::Sitting);
cell!(sitting_starting, Writer::Starting, Command_::Sitting);
cell!(sitting_busy, Writer::Busy, Command_::Sitting);
cell!(sitting_down, Writer::Down, Command_::Sitting);

#[test]
fn a_recall_stays_inside_one_call_deadline() {
    let seat = seat();
    let url = serve(Writer::Starting);
    let (out, took) = run_with(
        &seat,
        &url,
        Command_::Recall,
        &[("LJOS_CALL_TIMEOUT", "2"), ("LJOS_SEARCH_WARMUP_WAIT", "8")],
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let said = format!(
        "took {took:?} status {:?}\nstdout:\n{stdout}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{said}");
    assert!(stdout.contains(LOCAL_HIT), "{said}");
    assert!(took < Duration::from_millis(3000), "{said}");
}
