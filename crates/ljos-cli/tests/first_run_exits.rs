//! A first sitting, a missing issue, and the exits that used to say nothing.
//!
//! A claim directory with no `work.bin` is an empty graph: the sitting
//! claims and the claim creates the snapshot. An issue the tracker does
//! not hold is one line and a nonzero exit, and the claim graph is not
//! asked to take it. `ljos remember` with no text and `ljos accept` of an
//! id no memory has are the same kind of answer. `ljos note TEXT` notes
//! on the issue this seat holds.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

/// Clap's status when the arguments could not be read.
const USAGE_STATUS: i32 = 2;

const MISSING: &str = "Nope-1234";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A pack that answers an empty roster, so a read does not wait on a writer.
fn pack() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf);
            let line = req.lines().next().unwrap_or("");
            let body = if line.contains("/v1/atoms") {
                r#"{"atoms":[]}"#
            } else if line.contains("/v1/activate") {
                r#"{"island":[],"fired":0}"#
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    port
}

struct Seat {
    root: tempfile::TempDir,
    log: PathBuf,
    claims: PathBuf,
    issue: String,
}

impl Seat {
    /// `graph` is how `claimdag list` answers before any snapshot exists.
    fn open(graph: Graph) -> Self {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("calls.log");
        let tracker = root.path().join("tracker");
        let claims = root.path().join("claims");
        std::fs::create_dir_all(tracker.join("Software")).unwrap();
        std::fs::create_dir_all(&claims).unwrap();
        std::fs::create_dir_all(root.path().join("home")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        let log_s = log.display().to_string();
        let tracker_s = tracker.display().to_string();
        script(
            root.path(),
            "vissue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{log_s}'\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"probe-sit\",\"title\":\"probe\",\"state\":\"TODO\"}}' ;;\n  recall) echo noted ;;\n  identity) printf '%s\\n' 'vissue test' 'root={tracker_s}' 'prefix=Software' ;;\n  claim) echo claimed ;;\n  note) echo noted ;;\nesac\nexit 0\n"
            ),
        );
        let list = match graph {
            Graph::Empty => {
                "list)\n    if [ ! -f \"$CLAIMDAG_DIR/work.bin\" ]; then\n      printf '%s\\n' \"no work graph at $CLAIMDAG_DIR: the directory holds no work.bin. Claim something to create it\" >&2\n      exit 1\n    fi\n    echo '[]'\n    ;;"
            }
            .to_string(),
            Graph::Answers => "list) echo '[]' ;;".to_string(),
        };
        script(
            root.path(),
            "claimdag",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  {list}\n  get) exit 1 ;;\n  upsert|claim)\n    mkdir -p \"$CLAIMDAG_DIR\"\n    : > \"$CLAIMDAG_DIR/work.bin\"\n    echo gen=2\n    ;;\nesac\nexit 0\n"
            ),
        );
        script(
            root.path(),
            "deedar",
            &format!("#!/bin/sh\nprintf '%s\\n' \"deedar $*\" >> '{log_s}'\necho head\nexit 0\n"),
        );
        let layout = vissue_core::Layout::new(&tracker, "Software");
        vissue_core::ops::create(&layout, "probe", "probe work", Default::default()).unwrap();
        let issue =
            vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe"))
                .unwrap()
                .headings[0]
                .id
                .clone();
        Self {
            root,
            log,
            claims,
            issue,
        }
    }

    fn hold(&self, issue: &str) {
        let run = self.root.path().join("run/ljos");
        std::fs::create_dir_all(&run).unwrap();
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
        let graph = std::fs::canonicalize(&self.claims).unwrap();
        std::fs::write(
            run.join("hold-sitting"),
            format!(
                "saddle\nsaddle\n{}\n{}\n2026-01-01T00:00:00Z\n{issue}\n{}\n",
                std::process::id(),
                comm.trim(),
                graph.display()
            ),
        )
        .unwrap();
    }

    fn command(&self) -> Command {
        let home = self.root.path().join("home");
        let path = format!("{}:/usr/bin:/bin", self.root.path().display());
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.current_dir(self.root.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("CLAIMDAG_DIR", &self.claims)
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_URL", format!("http://127.0.0.1:{}", pack()))
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

enum Graph {
    /// `claimdag list` refuses until a writer creates `work.bin`.
    Empty,
    /// `claimdag list` answers an empty graph.
    Answers,
}

fn text_of(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A fresh `CLAIMDAG_DIR` has a directory and no snapshot. The sitting
/// claims, and that claim is what creates `work.bin`.
#[test]
fn a_first_sitting_creates_the_claim_graph() {
    let seat = Seat::open(Graph::Empty);
    assert!(
        !seat.claims.join("work.bin").exists(),
        "the fixture started with a snapshot"
    );
    let out = seat.run(&["sitting", &seat.issue]);
    let said = text_of(&out);
    assert!(
        out.status.success(),
        "the first sitting refused:\n{said}\ncalls:\n{}",
        seat.calls()
    );
    assert!(
        !said.contains("nothing was claimed"),
        "the empty graph stopped the sitting:\n{said}"
    );
    assert!(
        !said.contains("Claim something to create it"),
        "doctor still tells the seat to claim by hand:\n{said}"
    );
    assert!(
        seat.calls().contains("claimdag upsert"),
        "the node was not minted:\n{}",
        seat.calls()
    );
    assert!(
        seat.calls().contains("claimdag claim"),
        "the issue was not claimed:\n{}",
        seat.calls()
    );
    assert!(
        seat.claims.join("work.bin").is_file(),
        "the sitting left the claim directory without a snapshot"
    );
    assert!(
        said.contains("is yours"),
        "a sitting that claimed did not say so:\n{said}"
    );
}

/// `Nope-1234` is not a ticket. The sitting says so, exits nonzero, and
/// does not claim it or say it is held.
#[test]
fn a_sitting_on_a_missing_issue_exits_without_claiming() {
    let seat = Seat::open(Graph::Answers);
    let out = seat.run(&["sitting", MISSING]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(
        stderr.trim(),
        format!("issue {MISSING} not found, so run `ljos ready`.")
    );
    assert!(
        !stdout.contains("is yours"),
        "a missing issue was called held:\n{stdout}"
    );
    assert!(
        !seat.calls().contains("claimdag claim") && !seat.calls().contains("claimdag upsert"),
        "a missing issue was claimed:\n{}",
        seat.calls()
    );
}

/// No lesson text is a usage error: one line, and nothing is stored.
#[test]
fn an_empty_remember_exits_with_one_line() {
    let seat = Seat::open(Graph::Answers);
    for args in [
        vec!["remember"],
        vec!["remember", "   "],
        vec!["remember", ""],
    ] {
        let out = seat.run(&args);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(USAGE_STATUS),
            "{args:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(stdout.is_empty(), "{args:?} wrote stdout:\n{stdout}");
        assert_eq!(
            stderr.lines().filter(|line| !line.is_empty()).count(),
            1,
            "{args:?} was not one line:\n{stderr}"
        );
        assert_eq!(
            stderr.trim(),
            "remember: the text is empty, so run `ljos remember \"TEXT\"`."
        );
    }
}

/// An id no memory starts with is one line. It is a label, not a failed exit.
#[test]
fn an_unknown_accept_id_exits_with_one_line() {
    let seat = Seat::open(Graph::Answers);
    let out = seat.run(&["accept", "deadbeef"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.is_empty(), "stderr:\n{stderr}");
    assert_eq!(
        stdout.lines().filter(|line| !line.is_empty()).count(),
        1,
        "not one line:\n{stdout}"
    );
    assert_eq!(stdout.trim(), "not found deadbeef");
}

/// `ljos note "did a thing"` notes on the held issue. The words are not
/// taken as an issue id.
#[test]
fn a_note_with_only_text_uses_the_held_issue() {
    let seat = Seat::open(Graph::Answers);
    seat.hold(&seat.issue);
    let out = seat.run(&["note", "did a thing"]);
    let said = text_of(&out);
    assert!(out.status.success(), "note failed:\n{said}");
    assert!(
        seat.calls()
            .contains(&format!("vissue note {} did a thing", seat.issue)),
        "the note did not land on the held issue:\n{}",
        seat.calls()
    );
    assert!(
        said.contains(&seat.issue),
        "the answer did not name the held issue:\n{said}"
    );
}
