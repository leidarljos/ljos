//! Terminal writes from v0.17 through v0.29, and a lesson a runner held.
//!
//! Those releases left no `source.seat`. Harness and session are the login
//! user, via is `ljos`, and there is no turn. A preference and a lesson of
//! that shape are stamped `person`. A lesson whose session names a runner
//! stays unverified, and `ljos prefer --review` counts and keeps it.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::Value;

#[path = "common/terminal.rs"]
mod terminal;

const VERSIONS: &[&str] = &["0.17.0", "0.22.0", "0.25.0", "0.29.0"];
const RUNNER_LESSON: &str = "The lint job must pass before a merge.";
const AGENT_LESSON: &str = "The deploy window closes at four on Friday.";

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
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
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
            assert!(
                std::time::Instant::now() < deadline,
                "packsetd did not answer"
            );
            thread::sleep(Duration::from_millis(50));
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

struct Seat {
    home: tempfile::TempDir,
    run: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn on(url: String) -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            run: tempfile::tempdir().unwrap(),
            url,
        }
    }

    fn command(&self, bin: &str, args: &[&str]) -> Command {
        let path = format!("{}:/usr/bin:/bin", self.home.path().join("bin").display());
        let mut cmd = Command::new(bin);
        cmd.args(args)
            .current_dir(self.home.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.home.path())
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.run.path())
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "8000")
            .env("LJOS_USER_RULES_SYNC", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// `ljos doctor` exits 0 only when the required binaries answer. The
    /// other commands in this test keep the stripped PATH.
    fn doctor(&self, bin: &str, workspace: &str) -> Output {
        stage_doctor_bins(self.home.path());
        self.command(bin, &["doctor"])
            .env("PACKSET_WORKSPACE", workspace)
            .env("PATH", doctor_path(self.home.path()))
            .output()
            .unwrap()
    }
}

/// Symlink this build's `ljos` and `ljos-mcp`, stub the two tools this
/// seat does not ship, and keep the parent's PATH so packset, packsetd,
/// deedar and ljos-policyd stay the ones already installed.
fn stage_doctor_bins(home: &Path) {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let mode = std::fs::Permissions::from_mode(0o755);
    let vissue = bin.join("vissue");
    std::fs::write(
        &vissue,
        "#!/bin/sh\nif [ \"$1\" = vote ]; then echo '--used --confidence'; else echo 'vissue 0.21.0'; fi\n",
    )
    .unwrap();
    std::fs::set_permissions(&vissue, mode.clone()).unwrap();
    let claimdag = bin.join("claimdag");
    std::fs::write(&claimdag, "#!/bin/sh\necho 'claimdag 0.8.0'\n").unwrap();
    std::fs::set_permissions(&claimdag, mode).unwrap();
    for (name, src) in [
        ("ljos", env!("CARGO_BIN_EXE_ljos")),
        ("ljos-mcp", env!("CARGO_BIN_EXE_ljos-mcp")),
    ] {
        let dest = bin.join(name);
        if !dest.exists() {
            symlink(src, dest).unwrap();
        }
    }
}

fn doctor_path(home: &Path) -> String {
    let parent = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
    format!("{}:{parent}", home.join("bin").display())
}

fn text_of(out: &Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn succeeded(out: &Output, what: &str) -> String {
    assert!(out.status.success(), "{what} failed\n{}", text_of(out));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn atoms(url: &str, workspace: &str) -> Vec<Value> {
    let got: Value = ureq::get(&format!("{url}/v1/atoms"))
        .query("workspace", workspace)
        .query("embedding", "omit")
        .call()
        .expect("GET /v1/atoms")
        .into_json()
        .expect("atoms json");
    got["atoms"].as_array().cloned().unwrap_or_default()
}

fn by_text<'a>(atoms: &'a [Value], needle: &str) -> &'a Value {
    atoms
        .iter()
        .find(|a| {
            a.get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains(needle))
                && a.get("tombstone") != Some(&Value::Bool(true))
        })
        .unwrap_or_else(|| panic!("no atom containing {needle}: {atoms:?}"))
}

fn basis(atom: &Value) -> Option<&str> {
    atom.pointer("/source/basis")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|b| !b.is_empty())
}

fn release_bin(version: &str) -> PathBuf {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/oldljos/{version}"));
    std::fs::create_dir_all(&dir).unwrap();
    let asset = format!("ljos-v{version}-x86_64-unknown-linux-gnu.tar.gz");
    let archive = dir.join(&asset);
    let sum = dir.join(format!("{asset}.sha256"));
    let base = format!("https://github.com/leidarljos/ljos/releases/download/v{version}");
    if !archive.is_file() || !sum.is_file() {
        fetch(&format!("{base}/{asset}"), &archive);
        fetch(&format!("{base}/{asset}.sha256"), &sum);
    }
    let want = std::fs::read_to_string(&sum).unwrap();
    let want = want.split_whitespace().next().expect("sha256 line");
    let hashed = Command::new("sha256sum")
        .arg(&archive)
        .output()
        .expect("sha256sum");
    let have = String::from_utf8_lossy(&hashed.stdout);
    let have = have.split_whitespace().next().unwrap_or("");
    assert_eq!(have, want, "v{version} archive checksum");
    let ljos = find_bin(&dir, "ljos");
    if ljos.is_file() {
        return ljos;
    }
    let status = Command::new("tar")
        .args([
            "-xzf",
            archive.to_str().unwrap(),
            "-C",
            dir.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "tar v{version}");
    find_bin(&dir, "ljos")
}

fn find_bin(dir: &Path, name: &str) -> PathBuf {
    if dir.join(name).is_file() {
        return dir.join(name);
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let nested = entry.path().join(name);
            if nested.is_file() {
                return nested;
            }
        }
    }
    dir.join(name)
}

fn fetch(url: &str, to: &Path) {
    let status = Command::new("curl")
        .args(["-fsSL", "-o", &to.display().to_string(), url])
        .status()
        .expect("curl");
    assert!(status.success(), "fetch {url}");
}

/// A parent whose comm is `systemd`, so the child reads as a person at a
/// terminal: the tree stops at the session, and the login user is the seat.
fn at_terminal(seat: &Seat, bin: &Path, cmd: &str, text: &str, workspace: &str) -> Output {
    const SCRIPT: &str = r#"
import ctypes, os, sys
libc = ctypes.CDLL(None)
libc.prctl.argtypes = [ctypes.c_int, ctypes.c_char_p]
libc.prctl.restype = ctypes.c_int
if libc.prctl(15, b"systemd") != 0:
    raise SystemExit(91)
pid = os.fork()
if pid == 0:
    os.execv(sys.argv[1], [sys.argv[1], sys.argv[2], sys.argv[3]])
_, status = os.waitpid(pid, 0)
raise SystemExit(os.waitstatus_to_exitcode(status))
"#;
    seat.command(
        "python3",
        &["-c", SCRIPT, &bin.display().to_string(), cmd, text],
    )
    .env("PACKSET_WORKSPACE", workspace)
    .output()
    .unwrap()
}

fn keep_on_a_terminal(seat: &Seat, bin: &str, id: &str, workspace: &str) -> Output {
    const SCRIPT: &str = r#"
import ctypes, os, pty, sys
libc = ctypes.CDLL(None)
libc.prctl.argtypes = [ctypes.c_int, ctypes.c_char_p]
libc.prctl.restype = ctypes.c_int
if libc.prctl(15, b"gnome-terminal-") != 0:
    raise SystemExit(91)
pid, fd = pty.fork()
if pid == 0:
    os.execv(sys.argv[1], [sys.argv[1], "prefer", "--review", "--keep", sys.argv[2]])
chunks = []
while True:
    try:
        data = os.read(fd, 4096)
    except OSError:
        break
    if not data:
        break
    chunks.append(data)
_, status = os.waitpid(pid, 0)
sys.stdout.buffer.write(b"".join(chunks))
raise SystemExit(os.waitstatus_to_exitcode(status))
"#;
    seat.command("python3", &["-c", SCRIPT, bin, id])
        .env("PACKSET_WORKSPACE", workspace)
        .output()
        .unwrap()
}

fn assert_terminal_shape(atom: &Value) {
    assert_eq!(atom["origin"], "user-declared", "{atom}");
    assert!(basis(atom).is_none(), "{atom}");
    let source = atom["source"].as_object().expect("source");
    assert!(
        source.get("seat").is_none() || source["seat"].as_str().unwrap_or("").is_empty(),
        "v0.17–v0.29 stamped a seat: {source:?}"
    );
    assert_eq!(source["harness"], source["session"], "{source:?}");
    assert_eq!(source["via"], "ljos", "{source:?}");
    assert!(source.get("turn").is_none(), "{source:?}");
    assert!(!source["session"].as_str().unwrap_or("").contains('@'));
}

/// v0.17 through v0.29 terminal writes gain a person basis. A runner's
/// lesson from that era is listed and kept, and a v0.32.1 runner lesson
/// that was stored as the agent's is left out of that list.
#[test]
fn terminal_writes_before_v0_30_are_promoted_and_a_held_lesson_is_listed() {
    let daemon = Daemon::start();
    let seat = Seat::on(daemon.url.clone());
    let new = env!("CARGO_BIN_EXE_ljos");

    for version in VERSIONS {
        let bin = release_bin(version);
        let workspace = format!("v{}", version.replace('.', ""));
        let pref = format!("Keep the {version} notes in one paper file.");
        let lesson = format!("The {version} staging host reboots on its own night.");
        let wrote = at_terminal(&seat, &bin, "prefer", &pref, &workspace);
        succeeded(&wrote, &format!("v{version} prefer"));
        let wrote = at_terminal(&seat, &bin, "remember", &lesson, &workspace);
        succeeded(&wrote, &format!("v{version} remember"));

        let stored = atoms(&daemon.url, &workspace);
        let preference = by_text(&stored, &format!("{version} notes"));
        let remembered = by_text(&stored, &format!("{version} staging"));
        assert_eq!(preference["kind"], "preference", "{preference}");
        assert_eq!(remembered["kind"], "lesson", "{remembered}");
        assert_terminal_shape(preference);
        assert_terminal_shape(remembered);

        let review = seat
            .command(
                terminal::PYTHON,
                &terminal::shape("terminal", new, &["prefer", "--review"]),
            )
            .env("PACKSET_WORKSPACE", &workspace)
            .output()
            .unwrap();
        let review = succeeded(&review, &format!("v{version} review"));
        assert!(
            review.contains("no legacy preferences to review"),
            "{review}"
        );
        let stored = atoms(&daemon.url, &workspace);
        assert_eq!(
            basis(by_text(&stored, &format!("{version} notes"))),
            Some("person"),
            "v{version} preference"
        );
        assert_eq!(
            basis(by_text(&stored, &format!("{version} staging"))),
            Some("person"),
            "v{version} lesson"
        );
    }

    let old = release_bin("0.29.0");
    let runner = seat
        .command(&old.display().to_string(), &["remember", RUNNER_LESSON])
        .env("PACKSET_WORKSPACE", "held")
        .env("CLAUDE_SESSION_ID", "runnerlesson01")
        .output()
        .unwrap();
    succeeded(&runner, "v0.29 runner remember");

    let current = release_bin("0.32.1");
    let agent = seat
        .command(&current.display().to_string(), &["remember", AGENT_LESSON])
        .env("PACKSET_WORKSPACE", "held")
        .env("CLAUDE_SESSION_ID", "agentlesson01")
        .output()
        .unwrap();
    succeeded(&agent, "v0.32.1 runner remember");

    let before = atoms(&daemon.url, "held");
    let held = by_text(&before, "lint job");
    assert_eq!(held["kind"], "lesson", "{held}");
    assert_eq!(held["origin"], "user-declared", "{held}");
    assert!(basis(held).is_none(), "{held}");
    assert!(held["source"]["session"]
        .as_str()
        .unwrap_or("")
        .contains('@'));
    let agent_atom = by_text(&before, "deploy window");
    assert_eq!(agent_atom["origin"], "agent-derived", "{agent_atom}");
    let held_id = held["id"].as_str().unwrap().to_string();

    let review = seat
        .command(
            terminal::PYTHON,
            &terminal::shape("terminal", new, &["prefer", "--review"]),
        )
        .env("PACKSET_WORKSPACE", "held")
        .output()
        .unwrap();
    let review = succeeded(&review, "review held lesson");
    assert!(review.contains("1 older lesson awaits review"), "{review}");
    assert!(review.contains("lint job"), "{review}");
    assert!(review.contains("lesson"), "{review}");
    assert!(review.contains(&held_id), "{review}");
    assert!(!review.contains("deploy window"), "{review}");
    let when = &held["ts"].as_str().unwrap_or("")[..10];
    let harness = held["source"]["harness"].as_str().unwrap_or("");
    let line = review
        .lines()
        .find(|l| l.contains("lint job"))
        .unwrap_or("");
    assert!(line.contains(when), "{line}");
    assert!(line.contains(harness), "seat falls back to harness: {line}");
    assert!(
        line.contains(held["source"]["session"].as_str().unwrap_or("missing")),
        "{line}"
    );
    let doctor = seat.doctor(new, "held");
    assert!(
        doctor.status.success(),
        "a held lesson must not fail the doctor\n{}",
        text_of(&doctor)
    );
    assert!(
        String::from_utf8_lossy(&doctor.stdout).contains("1 older lesson awaits review"),
        "{}",
        text_of(&doctor)
    );

    let after = atoms(&daemon.url, "held");
    assert!(basis(by_text(&after, "lint job")).is_none());
    assert_eq!(by_text(&after, "deploy window")["origin"], "agent-derived");
    assert_ne!(basis(by_text(&after, "deploy window")), Some("person"));

    let kept = keep_on_a_terminal(&seat, new, &held_id, "held");
    let kept = succeeded(&kept, "keep lesson");
    assert!(kept.contains("lesson"), "{kept}");
    assert_eq!(
        basis(by_text(&atoms(&daemon.url, "held"), "lint job")),
        Some("person")
    );
    let quiet = seat
        .command(new, &["prefer", "--review"])
        .env("PACKSET_WORKSPACE", "held")
        .output()
        .unwrap();
    let quiet = succeeded(&quiet, "review after keep");
    assert!(
        quiet.contains("no legacy preferences to review"),
        "{quiet}"
    );
}
