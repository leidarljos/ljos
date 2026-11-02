//! A lesson written again in other words reinforces the memory that holds
//! it, so a sitting's few recall lines show other lessons, not copies. A
//! person who says a derived lesson again, or the opposite of their own,
//! gets a memory of their own.

ljos_test_boot::harness!();
#[path = "common/terminal.rs"]
mod terminal;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const WORDINGS: [&str; 5] = [
    "Always run all of the database migrations before you start the server.",
    "Run the database migrations before the server starts.",
    "The database migrations go before any server start.",
    "Database migrations should run before your server starts.",
    "Database migrations have to run before any server start.",
];

const OTHERS: [&str; 3] = [
    "Back up the database before a migration that drops a column.",
    "Name each database migration file after the ticket it belongs to.",
    "The test database replays every migration each night at midnight.",
];

struct Daemon {
    child: std::process::Child,
    url: String,
    _home: tempfile::TempDir,
}

impl Daemon {
    fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dir = home.path().to_str().unwrap().to_string();
        let mut child = Command::new("packsetd")
            .args(["--port", &port.to_string(), "--home", &dir])
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_HOME", &dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("packsetd is not on PATH; CI builds it from packset develop");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                let mut err = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut err);
                }
                panic!("packsetd exited {status}: {err}");
            }
            if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                let _ = stream.write_all(b"GET /health HTTP/1.1\r\nConnection: close\r\n\r\n");
                let mut said = String::new();
                let _ = stream.read_to_string(&mut said);
                if said.starts_with("HTTP/1.1 200") {
                    break;
                }
            }
            assert!(Instant::now() < deadline, "packsetd did not answer");
            std::thread::sleep(Duration::from_millis(50));
        }
        Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
            _home: home,
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A seat on `url` with a tracker holding one ticket, `Database
/// migrations`, and stand-ins for the tracker, claim and deed tools.
struct Seat {
    root: tempfile::TempDir,
    url: String,
    issue: String,
}

impl Seat {
    fn on(url: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        std::fs::write(
            home.join(".cursor/mcp.json"),
            r#"{"mcpServers":{"ljos":{"command":"/no/such/ljos-mcp"}}}"#,
        )
        .unwrap();
        let tracker = root.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software")).unwrap();
        let layout = vissue_core::Layout::new(&tracker, "Software");
        vissue_core::ops::create(&layout, "probe", "Database migrations", Default::default())
            .unwrap();
        let doc = vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe"))
            .unwrap();
        let issue = doc.headings[0].id.clone();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        script(
            &bin,
            "vissue",
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"{issue}\",\"title\":\"Database migrations\",\"state\":\"TODO\"}}' ;;\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\nesac\nexit 0\n",
                tracker.display()
            ),
        );
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\nesac\necho '[]'\nexit 0\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\nexit 0\n");
        Self {
            root,
            url: url.to_string(),
            issue,
        }
    }

    fn command(&self, bin: &str, args: &[&str]) -> Command {
        let root = self.root.path();
        let home = root.join("home");
        let mut cmd = Command::new(bin);
        cmd.args(args)
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
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// `ljos remember TEXT` under a runner, with no sitting open: the
    /// seat's own standing lesson.
    fn remember(&self, text: &str) -> (String, String) {
        let out = self
            .command(env!("CARGO_BIN_EXE_ljos"), &["remember", text])
            .env("CLAUDECODE", "1")
            .output()
            .unwrap();
        succeeded(&out, text);
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn succeeded(out: &Output, what: &str) -> String {
    assert!(
        out.status.success(),
        "{what} failed: {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The lines under `== island`, up to the next section.
fn island(sitting: &str) -> Vec<&str> {
    sitting
        .lines()
        .skip_while(|l| !l.starts_with("== island"))
        .skip(1)
        .take_while(|l| !l.starts_with("== "))
        .collect()
}

#[test]
fn five_wordings_of_one_lesson_leave_room_for_three_others_at_a_sitting() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let daemon = Daemon::start();
    let seat = Seat::on(&daemon.url);

    let (ack, said) = seat.remember(WORDINGS[0]);
    assert!(!said.contains("reinforced"), "{said}");
    let held: String = ack.split('\t').next().unwrap().to_string();
    let short = &held[..8];
    for wording in &WORDINGS[1..] {
        let (ack, said) = seat.remember(wording);
        assert!(
            said.contains(&format!("reinforced {short} instead of adding a copy")),
            "{wording}\nstdout:\n{ack}\nstderr:\n{said}"
        );
        assert!(ack.starts_with(&held), "{ack}");
    }
    for other in OTHERS {
        let (_, said) = seat.remember(other);
        assert!(!said.contains("reinforced"), "{other}: {said}");
    }

    let listed = succeeded(
        &seat
            .command(env!("CARGO_BIN_EXE_ljos"), &["memories", "list"])
            .output()
            .unwrap(),
        "memories list",
    );
    let entry: Vec<&str> = listed
        .lines()
        .skip_while(|l| !l.starts_with(short))
        .take_while(|l| l.starts_with(short) || l.starts_with("    "))
        .collect();
    assert!(entry.iter().any(|l| l.contains("recalled 4x")), "{listed}");
    for wording in &WORDINGS[1..] {
        assert!(
            entry.contains(&format!("    also written: {wording}").as_str()),
            "{wording} is not kept with {short}:\n{listed}"
        );
    }

    let sitting = succeeded(
        &seat
            .command(
                env!("CARGO_BIN_EXE_ljos"),
                &["sitting", &seat.issue, "--verbose"],
            )
            .output()
            .unwrap(),
        "sitting",
    );
    let shown = island(&sitting);
    let copies = shown
        .iter()
        .filter(|l| WORDINGS.iter().any(|w| l.ends_with(w)))
        .count();
    assert_eq!(copies, 1, "{sitting}");
    for other in OTHERS {
        assert!(
            shown.iter().any(|l| l.ends_with(other)),
            "{other} is not among the recall lines:\n{sitting}"
        );
    }
}

#[test]
fn a_person_saying_it_again_or_the_opposite_gets_a_memory_of_their_own() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let daemon = Daemon::start();
    let seat = Seat::on(&daemon.url);
    seat.remember(WORDINGS[0]);
    let opposite = "Run the database migrations after the server starts.";
    for text in [WORDINGS[1], opposite] {
        let typed = terminal::shape("terminal", env!("CARGO_BIN_EXE_ljos"), &["remember", text]);
        let said = succeeded(
            &seat.command(terminal::PYTHON, &typed).output().unwrap(),
            "person remember",
        );
        assert!(!said.contains("reinforced"), "{text}: {said}");
        assert!(said.contains(text), "{said}");
    }
    let listed = succeeded(
        &seat
            .command(
                env!("CARGO_BIN_EXE_ljos"),
                &["memories", "list", "--user", "--json"],
            )
            .output()
            .unwrap(),
        "memories list",
    );
    let shown: Vec<serde_json::Value> = serde_json::from_str(&listed).unwrap();
    assert!(
        shown.iter().any(|m| m["text"] == opposite),
        "the person's lesson is not live:\n{listed}"
    );
    let derived = shown.iter().find(|m| m["text"] == WORDINGS[0]).unwrap();
    assert_eq!(derived["recalls"], 0, "{listed}");
    assert_eq!(derived["aliases"], serde_json::json!([]), "{listed}");
}
