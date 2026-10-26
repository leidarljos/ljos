//! Session start syncs user rules on the tracker budget. A fetch slower
//! than the side-event pack budget still arrives, and a healthy pack is
//! still read afterwards. Runs the built `ljos` binary.
#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[path = "common/seat_root.rs"]
mod seat_root;

struct Hab {
    root: PathBuf,
    home: PathBuf,
    run: PathBuf,
    repo: PathBuf,
}

impl Drop for Hab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn hab(name: &str) -> Hab {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp).join(format!(
        "m5s-{name}-{}-{}",
        std::process::id(),
        nanos % 1_000_000_000
    ));
    let hab = Hab {
        home: root.join("home"),
        run: root.join("run"),
        repo: root.join("repo"),
        root,
    };
    for dir in [&hab.home, &hab.run, &hab.repo] {
        std::fs::create_dir_all(dir).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hab.run, std::fs::Permissions::from_mode(0o700)).unwrap();
    hab
}

fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} in {}", cwd.display());
}

const ALAN: &str = r#"{"rules":[{"id":"ur-ocaisa","text":"never post or reply to Alan (ocaisa)","author":"rohit","reason":"he asked not to be pinged","deny_mention":{"handles":["ocaisa"],"names":["Alan"]},"deny_reply_to":{"handles":["ocaisa"],"names":["Alan"]},"written_at":"2026-10-25T00:00:00Z","updated_at":"2026-10-25T00:00:00Z","basis":"person"}]}"#;

fn ident(cwd: &Path) {
    git(cwd, &["config", "user.name", "seat"]);
    git(cwd, &["config", "user.email", "seat@example.invalid"]);
    git(cwd, &["config", "commit.gpgsign", "false"]);
}

/// A tracker whose fetch sleeps `delay` seconds, holding the Alan rule
/// in another machine's plaintext file.
fn tracker(h: &Hab, delay: &str) -> PathBuf {
    let pub_out = Command::new("age-keygen").output().expect("age-keygen");
    assert!(pub_out.status.success(), "age-keygen");
    let text = String::from_utf8_lossy(&pub_out.stdout);
    let rcpt = text
        .lines()
        .find_map(|line| line.strip_prefix("# public key: "))
        .expect("age public key")
        .to_string();
    let bare = h.root.join("remote.git");
    let status = Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&bare)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success());
    let other = h.root.join("other");
    let status = Command::new("git")
        .args(["clone", "-q"])
        .arg(&bare)
        .arg(&other)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success());
    ident(&other);
    std::fs::create_dir_all(other.join(".ljos")).unwrap();
    std::fs::write(
        other.join(".ljos/sync.toml"),
        format!("name = \"probe\"\nrecipients = [\"{rcpt}\"]\n"),
    )
    .unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "scope"]);
    git(&other, &["push", "-q", "origin", "HEAD:main"]);
    let tracker = h.root.join("tracker");
    let status = Command::new("git")
        .args(["clone", "-q"])
        .arg(&bare)
        .arg(&tracker)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success());
    ident(&tracker);
    std::fs::create_dir_all(other.join(".ljos/user-rules")).unwrap();
    std::fs::write(other.join(".ljos/user-rules/other.json"), ALAN).unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "rule"]);
    git(&other, &["push", "-q", "origin", "HEAD:main"]);
    if !delay.is_empty() && delay != "0" {
        git(
            &tracker,
            &[
                "config",
                "remote.origin.uploadpack",
                &format!("sleep {delay}; git-upload-pack"),
            ],
        );
    }
    tracker
}

fn hook(h: &Hab, tracker: &Path, pack: Option<String>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("hook")
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .env("HOME", &h.home)
        .env("XDG_CONFIG_HOME", h.home.join(".config"))
        .env("XDG_DATA_HOME", h.home.join(".local/share"))
        .env("XDG_CACHE_HOME", h.home.join(".cache"))
        .env("XDG_STATE_HOME", h.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &h.run)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_PROJECT", "probe")
        .env("LJOS_SEAT", "probe-seat")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_USER_RULES_TRACKER", tracker)
        .env("LJOS_USER_RULES_SYNC", "on")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(&h.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match pack {
        Some(url) => {
            cmd.env("PACKSET_URL", url)
                .env("PACKSET_TOKEN", "t".repeat(32));
        }
        None => {
            cmd.env("PACKSET_URL", "off");
        }
    }
    cmd
}

fn call(mut cmd: Command, payload: &str, limit: Duration) -> String {
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(payload.as_bytes()).unwrap();
    drop(stdin);
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() && started.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{stdout} stderr {}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn session_start() -> String {
    serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-probe",
        "hook_event_name": "sessionStart",
        "session_id": "c-probe",
    })
    .to_string()
}

fn covered() -> String {
    serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-probe",
        "hook_event_name": "beforeMCPExecution",
        "tool_name": "add_issue_comment",
        "mcp_server_name": "github",
        "tool_input": {"owner": "o", "repo": "r", "body": "cc @ocaisa"},
    })
    .to_string()
}

fn assert_synced(delay: &str) {
    let h = hab("sync");
    let tracker = tracker(&h, delay);
    let _start = call(
        hook(&h, &tracker, None),
        &session_start(),
        Duration::from_secs(8),
    );
    let rules =
        std::fs::read_to_string(h.home.join(".config/ljos/user-rules.json")).unwrap_or_default();
    assert!(
        rules.contains("ocaisa"),
        "delay {delay}: session start did not sync the rule: {rules}"
    );
    let wrote = call(hook(&h, &tracker, None), &covered(), Duration::from_secs(8));
    assert!(
        wrote.contains("deny"),
        "delay {delay}: covered write allowed after sync: {wrote}"
    );
}

/// A fetch of 0.35 s and 0.8 s is inside the 1.5 s tracker budget and
/// outside the 250 ms side budget. The rule arrives and the write is denied.
#[test]
fn a_slow_tracker_fetch_still_syncs_the_user_rule() {
    assert_synced("0.35");
    assert_synced("0.8");
}

fn atoms() -> String {
    let rows: Vec<_> = (0..5)
        .map(|i| {
            serde_json::json!({
                "id": format!("atom-{i}"),
                "kind": "lesson",
                "text": format!("lesson number {i} about readme lines"),
                "tags": ["lesson", "seat"],
                "created_at": "2026-10-20T00:00:00Z",
            })
        })
        .collect();
    serde_json::json!({"atoms": rows}).to_string()
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let rest = line
                .strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))?;
            rest.trim().parse().ok()
        })
        .unwrap_or(0)
}

/// A pack that answers every request. `hits` counts them.
fn healthy_pack(hits: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let body_atoms = atoms();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let hits = Arc::clone(&hits);
            let body_atoms = body_atoms.clone();
            std::thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut tmp) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    }
                    let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("/");
                    let path = path.split('?').next().unwrap_or("/");
                    let cl = content_length(&head);
                    while buf.len() < head_end + cl {
                        match sock.read(&mut tmp) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    }
                    hits.fetch_add(1, Ordering::SeqCst);
                    let body = if path == "/v1/status" {
                        r#"{"embedder":{"available":true,"answering":null}}"#.to_string()
                    } else if path == "/v1/atoms" {
                        body_atoms.clone()
                    } else {
                        r#"{"hits":[]}"#.to_string()
                    };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    if sock.write_all(resp.as_bytes()).is_err() {
                        return;
                    }
                    let rest = head_end + cl;
                    buf = buf.split_off(rest.min(buf.len()));
                }
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// A 0.35 s fetch and a pack that answers. The preface names the rule
/// and the session line still counts the five memories.
#[test]
fn a_slow_tracker_fetch_does_not_drop_memory() {
    let hits = Arc::new(AtomicUsize::new(0));
    let url = healthy_pack(Arc::clone(&hits));
    let h = hab("syncmem");
    let tracker = tracker(&h, "0.35");
    let start = call(
        hook(&h, &tracker, Some(url)),
        &session_start(),
        Duration::from_secs(8),
    );
    assert!(
        start.contains("ocaisa") || start.contains("Alan"),
        "preface dropped the rule: {start}"
    );
    assert!(
        start.contains("5 memories"),
        "pack was not read after the slow fetch ({} requests): {start}",
        hits.load(Ordering::SeqCst)
    );
    assert!(hits.load(Ordering::SeqCst) > 0, "the pack was never asked");
}
