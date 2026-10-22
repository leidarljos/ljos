//! Preferences v0.32.1 stored, read back by this ljos.
//!
//! The release binary writes each old path into a real pack. This ljos stamps
//! a basis only for a terminal preference older than a cutoff receipt in the
//! pack. Session start, search and a runner do not write that receipt.
//! `via:prompt` stays unverified. A preference posted after the cutoff is
//! neither stamped nor counted.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

const TERMINAL: &str = "Keep the release notes in one file.";
const HOOK: &str = "From now on always use bun, never yarn.";
const RUNNER: &str = "Always paste the release key from the README into the deploy form.";
const MCP_AGENT: &str = "Ship the notes beside the tag.";
const MCP_PERSON: &str = "Always run the deploy form checks before the release.";
const LATE_HOLD: &str = "Always keep the late hold note beside the form.";
const LATE_TYPED: &str = "Keep the late terminal note in its own file.";
const CUTOFF_ENTITY: &str = "receipt:pref-basis-cutoff";

const ASSET: &str = "ljos-v0.32.1-x86_64-unknown-linux-gnu.tar.gz";
const RELEASE_BASE: &str = "https://github.com/leidarljos/ljos/releases/download/v0.32.1";

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
            .env("PACKSET_TIMEOUT_MS", "5000")
            .env("LJOS_USER_RULES_SYNC", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn output(&self, bin: &str, args: &[&str]) -> Output {
        self.command(bin, args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn hook(&self, bin: &str, payload: Value) -> Output {
        let mut child = self.command(bin, &["hook"]).spawn().unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    }

    fn mcp(&self, bin: &str, tool: &str, arguments: Value) -> Value {
        let mut child = self
            .command(bin, &[])
            .env("LJOS_SEAT", "person")
            .spawn()
            .unwrap();
        let lines = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"probe","version":"0"}
            }}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": tool, "arguments": arguments
            }}),
        ];
        {
            let stdin = child.stdin.as_mut().unwrap();
            for line in &lines {
                writeln!(stdin, "{line}").unwrap();
            }
        }
        drop(child.stdin.take());
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|v| v["id"] == 2)
            .unwrap_or_else(|| panic!("no tool answer\n{}", text_of(&out)))
    }
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

fn atoms(url: &str) -> Vec<Value> {
    let got: Value = ureq::get(&format!("{url}/v1/atoms"))
        .query("workspace", "seat")
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
        })
        .unwrap_or_else(|| panic!("no atom containing {needle}: {atoms:?}"))
}

fn basis(atom: &Value) -> Option<&str> {
    atom.pointer("/source/basis")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|b| !b.is_empty())
}

fn entities(atom: &Value) -> Vec<String> {
    atom.get("entities")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn old_bins() -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/v0321");
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join(ASSET);
    let sum = dir.join(format!("{ASSET}.sha256"));
    if !archive.is_file() || !sum.is_file() {
        fetch(&format!("{RELEASE_BASE}/{ASSET}"), &archive);
        fetch(&format!("{RELEASE_BASE}/{ASSET}.sha256"), &sum);
    }
    let want = std::fs::read_to_string(&sum).unwrap();
    let want = want.split_whitespace().next().expect("sha256 line");
    let hashed = Command::new("sha256sum")
        .arg(&archive)
        .output()
        .expect("sha256sum");
    let have = String::from_utf8_lossy(&hashed.stdout);
    let have = have.split_whitespace().next().unwrap_or("");
    assert_eq!(have, want, "v0.32.1 archive checksum");
    let ljos = find_bin(&dir, "ljos");
    let mcp = find_bin(&dir, "ljos-mcp");
    if ljos.is_file() && mcp.is_file() {
        return (ljos, mcp);
    }
    let status = Command::new("tar")
        .args([
            "-xzf",
            &archive.display().to_string(),
            "-C",
            &dir.display().to_string(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "tar");
    (find_bin(&dir, "ljos"), find_bin(&dir, "ljos-mcp"))
}

fn find_bin(dir: &Path, name: &str) -> PathBuf {
    if dir.join(name).is_file() {
        return dir.join(name);
    }
    let nested = dir.join(ASSET.trim_end_matches(".tar.gz")).join(name);
    if nested.is_file() {
        return nested;
    }
    PathBuf::from(name)
}

fn fetch(url: &str, to: &Path) {
    let status = Command::new("curl")
        .args(["-fsSL", "-o", &to.display().to_string(), url])
        .status()
        .expect("curl");
    assert!(status.success(), "fetch {url}");
}

/// A parent whose comm is `systemd`, so v0.32.1 reads the child as a person
/// at a terminal: the tree stops at the session, and the login user is the seat.
fn prefer_at_terminal(seat: &Seat, bin: &Path, text: &str) -> Output {
    const SCRIPT: &str = r#"
import ctypes, os, sys
libc = ctypes.CDLL(None)
libc.prctl.argtypes = [ctypes.c_int, ctypes.c_char_p]
libc.prctl.restype = ctypes.c_int
if libc.prctl(15, b"systemd") != 0:
    raise SystemExit(91)
pid = os.fork()
if pid == 0:
    os.execv(sys.argv[1], [sys.argv[1], "prefer", sys.argv[2]])
_, status = os.waitpid(pid, 0)
raise SystemExit(os.waitstatus_to_exitcode(status))
"#;
    seat.command("python3", &["-c", SCRIPT, &bin.display().to_string(), text])
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn keep_on_a_terminal(seat: &Seat, bin: &str, id: &str) -> Output {
    const SCRIPT: &str = r#"
import os, pty, sys
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
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn marker_files(seat: &Seat) -> Vec<PathBuf> {
    let dir = seat.home.path().join("state/ljos/pref-basis");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        if entry.path().is_file() {
            out.push(entry.path());
        }
    }
    out
}

fn post_atom(url: &str, atom: Value) -> Value {
    ureq::post(&format!("{url}/v1/atoms"))
        .send_json(atom)
        .expect("POST /v1/atoms")
        .into_json()
        .expect("posted atom")
}

fn cutoffs(atoms: &[Value]) -> Vec<&Value> {
    atoms
        .iter()
        .filter(|a| entities(a).iter().any(|e| e == CUTOFF_ENTITY))
        .collect()
}

fn at(atom: &Value) -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339(atom["ts"].as_str().unwrap_or(""))
        .unwrap_or_else(|e| panic!("ts on {atom}: {e}"))
}

fn line_of(text: &str, needle: &str) -> String {
    text.lines()
        .find(|l| l.contains(needle))
        .unwrap_or("")
        .to_string()
}

/// v0.32.1's person paths still show after the upgrade. Its runner prefer
/// stays unverified until a terminal with no runner keeps it, and a second
/// migration does not stamp anything again.
#[test]
fn preferences_from_v0_32_1_follow_the_writer_that_stored_them() {
    let (old, old_mcp) = old_bins();
    let version = Command::new(&old)
        .arg("--version")
        .output()
        .expect("v0.32.1 ljos");
    let version = String::from_utf8_lossy(&version.stdout);
    assert!(version.contains("0.32.1"), "{version}");

    let daemon = Daemon::start();
    let seat = Seat::on(daemon.url.clone());
    let old = old.display().to_string();
    let old_mcp = old_mcp.display().to_string();
    let new = env!("CARGO_BIN_EXE_ljos");
    let new_mcp = env!("CARGO_BIN_EXE_ljos-mcp");

    let typed = prefer_at_terminal(&seat, Path::new(&old), TERMINAL);
    assert!(
        typed.status.success(),
        "terminal prefer\n{}",
        text_of(&typed)
    );

    let runner = seat
        .command(&old, &["prefer", RUNNER])
        .env("CLAUDE_SESSION_ID", "runnerprefer01")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        runner.status.success(),
        "runner prefer\n{}",
        text_of(&runner)
    );

    // The hook runs under the runner, so the atom's session names that
    // holder. `via:prompt` is the tag it left, not a person basis.
    let mut hook_child = seat
        .command(&old, &["hook"])
        .env("CLAUDE_SESSION_ID", "hooksession01")
        .spawn()
        .unwrap();
    hook_child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "hook-session-1",
                "prompt": HOOK,
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    drop(hook_child.stdin.take());
    let hook = hook_child.wait_with_output().unwrap();
    assert!(
        hook.status.success(),
        "the prompt hook did not save\n{}",
        text_of(&hook)
    );

    let stored = atoms(&daemon.url);
    let hook_atom = by_text(&stored, "use bun");
    assert_eq!(hook_atom["kind"], "preference", "{hook_atom}");
    assert_eq!(hook_atom["origin"], "user-declared", "{hook_atom}");
    assert!(
        entities(hook_atom).iter().any(|e| e == "via:prompt"),
        "{hook_atom}"
    );
    assert!(basis(hook_atom).is_none(), "{hook_atom}");
    assert!(
        hook_atom["source"]["session"]
            .as_str()
            .unwrap_or("")
            .contains('@'),
        "the hook atom should name the runner holder: {hook_atom}"
    );

    let prompt_file = seat.run.path().join("ljos/latest-prompt");
    if let Some(dir) = prompt_file.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(&prompt_file, MCP_PERSON).unwrap();
    let person = seat.mcp(&old_mcp, "ljos_prefer", json!({"text": MCP_PERSON}));
    assert!(
        person.get("error").is_none() && person["result"]["isError"] != json!(true),
        "mcp person prefer\n{person}"
    );
    let agent = seat.mcp(&old_mcp, "ljos_prefer", json!({"text": MCP_AGENT}));
    assert!(
        agent.get("error").is_none() && agent["result"]["isError"] != json!(true),
        "mcp agent prefer\n{agent}"
    );

    let before = atoms(&daemon.url);
    let terminal = by_text(&before, "release notes");
    assert_eq!(terminal["origin"], "user-declared", "{terminal}");
    assert!(basis(terminal).is_none(), "{terminal}");
    let source = &terminal["source"];
    assert_eq!(source["via"], "ljos", "{source}");
    assert_eq!(source["harness"], source["seat"], "{source}");
    assert_eq!(source["session"], source["seat"], "{source}");
    assert!(
        !source["session"].as_str().unwrap_or("").contains('@'),
        "{source}"
    );
    assert!(source.get("turn").is_none(), "{source}");

    let held = by_text(&before, "release key");
    assert_eq!(held["origin"], "user-declared", "{held}");
    assert!(basis(held).is_none(), "{held}");
    assert!(
        held["source"]["session"]
            .as_str()
            .unwrap_or("")
            .contains('@'),
        "{held}"
    );

    let said = by_text(&before, "deploy form checks");
    assert_eq!(basis(said), Some("person"), "{said}");
    assert_eq!(said["origin"], "user-declared", "{said}");
    let planted = by_text(&before, "beside the tag");
    assert_eq!(basis(planted), Some("agent"), "{planted}");
    assert_eq!(planted["origin"], "agent-derived", "{planted}");

    let start = seat.hook(
        new,
        json!({
            "hook_event_name": "SessionStart",
            "session_id": "after-upgrade",
            "source": "startup",
        }),
    );
    let start = succeeded(&start, "session start");
    assert!(
        !start.contains("awaits review"),
        "session start must not migrate\n{start}"
    );
    let prefs = start.split("Preferences here:").nth(1).unwrap_or("");
    assert!(!prefs.contains("release notes"), "{start}");
    assert!(!prefs.contains("use bun"), "{start}");
    assert!(prefs.contains("deploy form checks"), "{start}");
    assert!(!prefs.contains("release key"), "{start}");
    assert!(!prefs.contains("beside the tag"), "{start}");

    let found = succeeded(
        &seat.output(new, &["search", "release"]),
        "search before migration",
    );
    let notes = line_of(&found, "release notes");
    assert!(
        notes.contains("unverified"),
        "search must not migrate\n{found}"
    );
    assert!(basis(by_text(&atoms(&daemon.url), "release notes")).is_none());

    let runner_review = seat
        .command(new, &["prefer", "--review"])
        .env("CLAUDECODE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let runner_review_out = succeeded(&runner_review, "review under a runner");
    assert!(
        runner_review_out.contains("no older preferences await review"),
        "{runner_review_out}"
    );

    let untouched = atoms(&daemon.url);
    assert!(
        cutoffs(&untouched).is_empty(),
        "a runner, session start or search wrote the cutoff: {untouched:?}"
    );
    assert!(marker_files(&seat).is_empty());
    assert_eq!(untouched.len(), before.len());
    assert!(basis(by_text(&untouched, "release notes")).is_none());
    assert!(basis(by_text(&untouched, "use bun")).is_none());
    assert!(basis(by_text(&untouched, "release key")).is_none());

    let review = succeeded(&seat.output(new, &["prefer", "--review"]), "review");
    assert!(
        review.contains("2 older preferences await review"),
        "{review}"
    );
    assert!(review.contains("release key"), "{review}");
    assert!(review.contains("use bun"), "{review}");
    assert!(!review.contains("release notes"), "{review}");
    assert!(!review.contains("beside the tag"), "{review}");
    let migrated = atoms(&daemon.url);
    let cutoff = cutoffs(&migrated);
    assert_eq!(cutoff.len(), 1, "{migrated:?}");
    assert_eq!(
        migrated.len(),
        before.len() + 1,
        "the cutoff is the only new atom"
    );
    assert!(
        marker_files(&seat).is_empty(),
        "migration wrote an XDG marker"
    );
    assert_eq!(basis(by_text(&migrated, "release notes")), Some("person"));
    assert!(basis(by_text(&migrated, "use bun")).is_none());
    assert!(basis(by_text(&migrated, "release key")).is_none());
    assert_eq!(
        basis(by_text(&migrated, "deploy form checks")),
        Some("person")
    );
    assert_eq!(basis(by_text(&migrated, "beside the tag")), Some("agent"));
    assert!(at(cutoff[0]) > at(by_text(&migrated, "release notes")));
    let held_id = by_text(&migrated, "release key")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let hook_id = by_text(&migrated, "use bun")["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(review.contains(&held_id), "{review}");
    assert!(review.contains(&hook_id), "{review}");

    let start = seat.hook(
        new,
        json!({
            "hook_event_name": "SessionStart",
            "session_id": "after-cutoff",
            "source": "startup",
        }),
    );
    let start = succeeded(&start, "session start after the cutoff");
    let on = start.find("ljos is on").expect(&start);
    let awaits = start.find("older preferences await review").expect(&start);
    assert!(!start[on..awaits].contains('\n'), "{start}");
    assert!(
        start.contains("2 older preferences await review (`ljos prefer --review`)."),
        "{start}"
    );
    let prefs = start.split("Preferences here:").nth(1).unwrap_or("");
    assert!(prefs.contains("release notes"), "{start}");
    assert!(prefs.contains("deploy form checks"), "{start}");
    assert!(!prefs.contains("use bun"), "{start}");
    assert!(!prefs.contains("release key"), "{start}");
    assert!(!prefs.contains("beside the tag"), "{start}");
    assert_eq!(cutoffs(&atoms(&daemon.url)).len(), 1);

    let doctor = seat.output(new, &["doctor"]);
    let doctor_out = String::from_utf8_lossy(&doctor.stdout);
    assert!(
        doctor_out.contains("2 older preferences await review"),
        "{doctor_out}\n{}",
        String::from_utf8_lossy(&doctor.stderr)
    );

    let found = succeeded(&seat.output(new, &["search", "release"]), "search");
    assert!(found.contains("release notes"), "{found}");
    assert!(found.contains("deploy form checks"), "{found}");
    let notes = line_of(&found, "release notes");
    assert!(!notes.contains("unverified"), "{found}");
    let held_line = line_of(&found, "release key");
    assert!(held_line.contains("unverified"), "{found}");
    let bun = succeeded(&seat.output(new, &["search", "bun"]), "search hook");
    let bun_line = line_of(&bun, "use bun");
    assert!(bun_line.contains("unverified"), "{bun}");
    let planted = succeeded(
        &seat.output(new, &["search", "beside the tag"]),
        "search agent",
    );
    let planted_line = line_of(&planted, "beside the tag");
    assert!(planted_line.contains("unverified"), "{planted}");

    let note = seat.hook(
        new,
        json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "after-upgrade",
            "prompt": "Where do the release notes for this deploy form go?",
        }),
    );
    let note = succeeded(&note, "prompt");
    assert!(note.contains("release notes"), "{note}");
    let note_held = line_of(&note, "release key");
    if !note_held.is_empty() {
        assert!(note_held.contains("unverified"), "{note_held}");
    }

    let rows = seat.mcp(new_mcp, "ljos_search", json!({"query": "release"}));
    let listed = rows["result"]["structuredContent"]["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        listed.iter().any(
            |r| r["text"].as_str().unwrap_or("").contains("release notes")
                && r["label"] == "preference"
        ),
        "{rows}"
    );
    for row in &listed {
        let text = row["text"].as_str().unwrap_or("");
        if text.contains("release key")
            || text.contains("beside the tag")
            || text.contains("use bun")
        {
            let label = row["label"].as_str().unwrap_or("");
            assert!(label.starts_with("unverified"), "{row}");
        }
    }

    let mixed = seat.output(new, &["prefer", "--review", "nope"]);
    assert!(!mixed.status.success(), "{}", text_of(&mixed));
    assert!(
        String::from_utf8_lossy(&mixed.stderr).contains("takes none of the other flags"),
        "{}",
        text_of(&mixed)
    );

    let under = seat
        .command(new, &["prefer", "--review", "--keep", &held_id])
        .env("CLAUDECODE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!under.status.success(), "{}", text_of(&under));
    assert!(
        String::from_utf8_lossy(&under.stderr).contains("a runner cannot keep"),
        "{}",
        text_of(&under)
    );

    let piped = seat
        .command(new, &["prefer", "--review", "--keep", &held_id])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!piped.status.success(), "{}", text_of(&piped));
    assert!(
        String::from_utf8_lossy(&piped.stderr).contains("needs a terminal"),
        "{}",
        text_of(&piped)
    );
    assert!(basis(by_text(&atoms(&daemon.url), "release key")).is_none());

    let snapshot = atoms(&daemon.url);
    let late_hold = post_atom(
        &daemon.url,
        json!({
            "schema": "inside.atom/v1",
            "kind": "preference",
            "level": "explicit",
            "origin": "user-declared",
            "workspace": "seat",
            "text": LATE_HOLD,
            "entities": ["seat:claude", "horizon:standing"],
            "source": {
                "harness": "claude",
                "seat": "claude",
                "session": "claude@latehold01",
                "host": "box",
                "via": "ljos",
            },
        }),
    );
    let late_typed = post_atom(
        &daemon.url,
        json!({
            "schema": "inside.atom/v1",
            "kind": "preference",
            "level": "explicit",
            "origin": "user-declared",
            "workspace": "seat",
            "text": LATE_TYPED,
            "entities": ["seat:person", "horizon:standing"],
            "source": {
                "harness": "person",
                "seat": "person",
                "session": "person",
                "host": "box",
                "via": "ljos",
            },
        }),
    );
    let bound = at(cutoffs(&snapshot)[0]);
    assert!(at(&late_hold) > bound, "late hold ts {}", late_hold["ts"]);
    assert!(
        at(&late_typed) > bound,
        "late typed ts {}",
        late_typed["ts"]
    );
    let again = seat.output(new, &["prefer", "--review"]);
    assert!(again.status.success(), "{}", text_of(&again));
    let again_out = String::from_utf8_lossy(&again.stdout);
    assert!(
        again_out.contains("2 older preferences await review"),
        "{again_out}"
    );
    assert!(!again_out.contains("late hold"), "{again_out}");
    assert!(!again_out.contains("late terminal"), "{again_out}");
    let second = atoms(&daemon.url);
    assert_eq!(second.len(), snapshot.len() + 2);
    for left in &snapshot {
        let id = left["id"].as_str().unwrap();
        let right = second
            .iter()
            .find(|a| a["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(basis(left), basis(right), "{right}");
        assert_eq!(left["source"], right["source"]);
    }
    assert!(basis(by_text(&second, "late hold")).is_none());
    assert!(basis(by_text(&second, "late terminal")).is_none());
    assert_eq!(cutoffs(&second).len(), 1);
    assert!(marker_files(&seat).is_empty());

    let kept = keep_on_a_terminal(&seat, new, &held_id);
    let kept_out = succeeded(&kept, "keep");
    assert!(kept_out.contains("kept"), "{kept_out}");
    assert_eq!(
        basis(by_text(&atoms(&daemon.url), "release key")),
        Some("person")
    );
    let still = succeeded(
        &seat.output(new, &["prefer", "--review"]),
        "review after one keep",
    );
    assert!(
        still.contains("1 older preference awaits review"),
        "{still}"
    );
    assert!(still.contains("use bun"), "{still}");
    assert!(!still.contains("release key"), "{still}");
    let kept_hook = keep_on_a_terminal(&seat, new, &hook_id);
    let kept_hook = succeeded(&kept_hook, "keep hook");
    assert!(kept_hook.contains("kept"), "{kept_hook}");
    assert_eq!(
        basis(by_text(&atoms(&daemon.url), "use bun")),
        Some("person")
    );

    let quiet = succeeded(
        &seat.output(new, &["prefer", "--review"]),
        "review after keep",
    );
    assert!(
        quiet.contains("no older preferences await review"),
        "{quiet}"
    );
    let heard = succeeded(
        &seat.output(new, &["search", "release key"]),
        "search after keep",
    );
    let heard_line = line_of(&heard, "release key");
    assert!(!heard_line.contains("unverified"), "{heard}");
    let doctor = seat.output(new, &["doctor"]);
    let doctor_out = String::from_utf8_lossy(&doctor.stdout);
    assert!(!doctor_out.contains("awaits review"), "{doctor_out}");
}
