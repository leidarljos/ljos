//! Finish with a queued write queues the ticket seal as a real job and
//! performs it on the next ordinary call once the queued lesson is stored.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const LESSON: &str = "Cold metal seats the door.";
const QUEUED_LINE: &str = "Memory busy: queued 1 write, it will be saved on a later call.";
const SYNC_QUEUED_LINE: &str = "The sync step is queued and will be sent on the next call.";

fn read_request(sock: &mut std::net::TcpStream) -> (String, String) {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    let head_end = loop {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => return (String::from_utf8_lossy(&buf).into_owned(), String::new()),
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos;
                }
            }
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let len = head
        .lines()
        .find_map(|line| {
            let (name, val) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| val.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let body_start = head_end + 4;
    while buf.len() < body_start + len {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    let body_end = (body_start + len).min(buf.len());
    (
        head,
        String::from_utf8_lossy(&buf[body_start..body_end]).into_owned(),
    )
}

fn write_http(sock: &mut std::net::TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(resp.as_bytes());
}

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

enum PackMode {
    AlwaysEagain,
    AlwaysBusy503,
    TransientGateEagain(usize),
}

struct MockPack {
    url: String,
    calls: Arc<AtomicUsize>,
    posts: Arc<AtomicUsize>,
    healthy: Arc<AtomicBool>,
}

fn serve_pack(mode: PackMode) -> MockPack {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let calls = Arc::new(AtomicUsize::new(0));
    let posts = Arc::new(AtomicUsize::new(0));
    let healthy = Arc::new(AtomicBool::new(false));
    let stored: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    let calls_bg = Arc::clone(&calls);
    let posts_bg = Arc::clone(&posts);
    let healthy_bg = Arc::clone(&healthy);
    let stored_bg = Arc::clone(&stored);
    let mode = Arc::new(mode);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let calls_bg = Arc::clone(&calls_bg);
            let posts_bg = Arc::clone(&posts_bg);
            let healthy_bg = Arc::clone(&healthy_bg);
            let stored_bg = Arc::clone(&stored_bg);
            let mode = Arc::clone(&mode);
            thread::spawn(move || {
                let Ok(mut sock) = stream else { return };
                let (head, body) = read_request(&mut sock);
                let line = head.lines().next().unwrap_or("");
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                if path.starts_with("/v1/atoms") {
                    if healthy_bg.load(Ordering::SeqCst) {
                        if line.starts_with("POST") {
                            let n = posts_bg.fetch_add(1, Ordering::SeqCst) + 1;
                            let mut val: serde_json::Value = serde_json::from_str(&body)
                                .unwrap_or_else(|_| serde_json::json!({}));
                            let id = format!("atom-kiln-{n}");
                            val["id"] = serde_json::Value::String(id.clone());
                            stored_bg.lock().unwrap().push(val.clone());
                            write_http(&mut sock, 200, &val.to_string());
                        } else {
                            let atoms = stored_bg.lock().unwrap().clone();
                            let resp = serde_json::json!({ "atoms": atoms }).to_string();
                            write_http(&mut sock, 200, &resp);
                        }
                        return;
                    }
                    let n = calls_bg.fetch_add(1, Ordering::SeqCst);
                    match &*mode {
                        PackMode::AlwaysEagain => {
                            thread::sleep(Duration::from_millis(400));
                        }
                        PackMode::AlwaysBusy503 => {
                            write_http(
                                &mut sock,
                                503,
                                r#"{"error":"packsetd is busy: memory pressure; try again"}"#,
                            );
                        }
                        PackMode::TransientGateEagain(stalls) => {
                            if line.starts_with("GET /v1/atoms/")
                                && path.len() > "/v1/atoms/".len()
                                && n < *stalls
                            {
                                thread::sleep(Duration::from_millis(350));
                            } else if line.starts_with("POST") {
                                let p = posts_bg.fetch_add(1, Ordering::SeqCst) + 1;
                                let mut val: serde_json::Value = serde_json::from_str(&body)
                                    .unwrap_or_else(|_| serde_json::json!({}));
                                val["id"] = serde_json::Value::String(format!("atom-kiln-{p}"));
                                stored_bg.lock().unwrap().push(val.clone());
                                write_http(&mut sock, 200, &val.to_string());
                            } else {
                                let atoms = stored_bg.lock().unwrap().clone();
                                let resp = serde_json::json!({ "atoms": atoms }).to_string();
                                write_http(&mut sock, 200, &resp);
                            }
                        }
                    }
                } else if path == "/v1/search" {
                    write_http(&mut sock, 200, r#"{"hits":[]}"#);
                } else {
                    write_http(&mut sock, 200, "{}");
                }
            });
        }
    });
    MockPack {
        url: format!("http://127.0.0.1:{port}"),
        calls,
        posts,
        healthy,
    }
}

struct Seat {
    root: tempfile::TempDir,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let config = home.join(".config");
        let tracker = root.path().join("tracker");
        let project = tracker.join("Software/demo");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        std::fs::create_dir_all(root.path().join("state")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        std::fs::create_dir_all(tracker.join(".ljos")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        git(&tracker, &["init", "-q", "-b", "main"]);
        git(&tracker, &["config", "core.hooksPath", "/dev/null"]);
        git(&tracker, &["config", "user.name", "ada"]);
        git(&tracker, &["config", "user.email", "ada@example.invalid"]);
        std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        std::fs::write(
            tracker.join(".ljos/sync.toml"),
            "name = \"probe\"\nrecipients = [\"age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p\"]\n",
        )
        .unwrap();
        std::fs::write(
            config.join("ljos/sync.toml"),
            format!("repos = [\"{}\"]\n", tracker.display()),
        )
        .unwrap();
        std::fs::write(
            project.join("issues.org"),
            "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#B] Probe the kiln
:PROPERTIES:
:ID:         demo-busy
:CLAIMED_BY: ada
:CREATED:    [2026-06-01 Mon]
:END:
",
        )
        .unwrap();
        git(&tracker, &["add", "."]);
        git(&tracker, &["commit", "-q", "-m", "seed"]);
        let tracker_s = tracker.display().to_string();
        script(
            root.path(),
            "vissue",
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  identity) printf '%s\\n' 'vissue test' 'root={tracker_s}' 'prefix=Software' ;;\n  show) printf '%s\\n' '{{\"id\":\"demo-busy\",\"title\":\"Probe the kiln\",\"state\":\"STARTED\",\"claimed_by\":\"ada\"}}' ;;\n  recall) echo noted ;;\n  note|claim|update) echo ok ;;\n  ready|children) echo '[]' ;;\nesac\nexit 0\n",
            ),
        );
        script(
            root.path(),
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  list) echo '[]' ;;\n  *) echo gen=2 ;;\nesac\nexit 0\n",
        );
        script(root.path(), "deedar", "#!/bin/sh\necho head\nexit 0\n");
        Self { root }
    }

    fn run(&self, args: &[&str], pack: &str, timeout_ms: &str, auth: bool) -> std::process::Output {
        let home = self.root.path().join("home");
        let path = format!("{}:/usr/bin:/bin", self.root.path().display());
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("PACKSET_URL", pack)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", if auth { "on" } else { "off" })
            .env("PACKSET_TIMEOUT_MS", timeout_ms)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if auth {
            cmd.env("PACKSET_TOKEN", "test-token");
        }
        cmd.output().unwrap()
    }

    fn tracker(&self) -> PathBuf {
        self.root.path().join("tracker")
    }

    fn state_ljos(&self) -> PathBuf {
        self.root.path().join("state/ljos")
    }
}

fn text_of(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn sealed_logs(tracker: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let atoms = tracker.join(".ljos/atoms");
    if let Ok(hosts) = std::fs::read_dir(&atoms) {
        for host in hosts.flatten() {
            if let Ok(files) = std::fs::read_dir(host.path()) {
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("age") {
                        found.push(path);
                    }
                }
            }
        }
    }
    found
}

#[test]
fn finish_with_queued_write_and_eagain_sync_does_not_ask_for_ljos_sync() {
    let seat = Seat::open();
    let pack = serve_pack(PackMode::AlwaysEagain);
    let out = seat.run(
        &["finish", "demo-busy", "--lesson", LESSON],
        &pack.url,
        "150",
        false,
    );
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(QUEUED_LINE), "{said}");
    assert!(said.contains(SYNC_QUEUED_LINE), "{said}");
    assert!(
        !said.contains("The sync step did not finish"),
        "sync asked for manual sync:\n{said}"
    );
    assert!(
        !said.contains("ljos sync"),
        "sync asked for manual `ljos sync`:\n{said}"
    );
    assert!(
        sealed_logs(&seat.tracker()).is_empty(),
        "seal already ran during busy finish"
    );
    assert!(
        seat.state_ljos().join("seal-queue.jsonl").exists(),
        "finish did not write a queued seal job"
    );

    pack.healthy.store(true, Ordering::SeqCst);
    let next = seat.run(&["ready"], &pack.url, "500", false);
    let next_said = text_of(&next);
    assert!(next.status.success(), "{next_said}");
    assert_eq!(pack.posts.load(Ordering::SeqCst), 1, "{next_said}");
    assert!(
        next_said.contains("sync: sealed queued ticket(s): demo-busy"),
        "next ordinary call did not report performing queued seal:\n{next_said}"
    );
    assert!(
        !sealed_logs(&seat.tracker()).is_empty(),
        "next ordinary call did not write sealed .age log:\n{next_said}"
    );
    let log = git(&seat.tracker(), &["log", "-n", "1", "--oneline"]);
    assert!(log.contains("chore(sync):"), "missing sync commit: {log}");
    assert_eq!(
        std::fs::read_to_string(seat.state_ljos().join("seal-queue.jsonl")).unwrap_or_default(),
        "",
        "seal-queue.jsonl was not drained"
    );
}

#[test]
fn finish_with_queued_write_and_503_sync_does_not_ask_for_ljos_sync() {
    let seat = Seat::open();
    let pack = serve_pack(PackMode::AlwaysBusy503);
    let out = seat.run(
        &["finish", "demo-busy", "--lesson", LESSON],
        &pack.url,
        "200",
        false,
    );
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(QUEUED_LINE), "{said}");
    assert!(said.contains(SYNC_QUEUED_LINE), "{said}");
    assert!(
        !said.contains("The sync step did not finish"),
        "sync asked for manual sync:\n{said}"
    );
    assert!(
        !said.contains("ljos sync"),
        "sync asked for manual `ljos sync`:\n{said}"
    );
    assert!(
        seat.state_ljos().join("seal-queue.jsonl").exists(),
        "finish did not write a queued seal job"
    );

    pack.healthy.store(true, Ordering::SeqCst);
    let next = seat.run(&["ready"], &pack.url, "500", false);
    let next_said = text_of(&next);
    assert!(next.status.success(), "{next_said}");
    assert_eq!(pack.posts.load(Ordering::SeqCst), 1, "{next_said}");
    assert!(
        !sealed_logs(&seat.tracker()).is_empty(),
        "next ordinary call did not write sealed .age log:\n{next_said}"
    );
    let log = git(&seat.tracker(), &["log", "-n", "1", "--oneline"]);
    assert!(log.contains("chore(sync):"), "missing sync commit: {log}");
}

#[test]
fn finish_recovers_when_pack_answers_after_transient_eagain() {
    let seat = Seat::open();
    let pack = serve_pack(PackMode::TransientGateEagain(2));
    let out = seat.run(
        &["finish", "demo-busy", "--lesson", LESSON],
        &pack.url,
        "150",
        true,
    );
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        !said.contains(QUEUED_LINE),
        "finish queued despite transient recovery:\n{said}"
    );
    assert!(
        !said.contains("The sync step did not finish"),
        "sync failed despite recovery:\n{said}"
    );
    assert_eq!(pack.posts.load(Ordering::SeqCst), 1, "{said}");
    assert!(pack.calls.load(Ordering::SeqCst) >= 3, "{said}");
    assert!(
        !sealed_logs(&seat.tracker()).is_empty(),
        "finish did not seal log after transient recovery:\n{said}"
    );
}

#[test]
fn finish_without_queued_note_under_eagain_prints_doctor_line_and_shares_budget() {
    let seat = Seat::open();
    let pack = serve_pack(PackMode::AlwaysEagain);
    let started = std::time::Instant::now();
    let out = seat.run(&["finish", "demo-busy"], &pack.url, "200", false);
    let took = started.elapsed();
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("ljos doctor"),
        "expected clean ljos doctor line when unqueued finish hits EAGAIN:\n{said}"
    );
    assert!(
        !said.contains("Resource temporarily unavailable"),
        "finish printed raw/duplicated errno text:\n{said}"
    );
    assert!(
        !said.contains("packset is not on PATH"),
        "finish printed misleading PATH hint under EAGAIN:\n{said}"
    );
    assert!(
        took < Duration::from_millis(3800),
        "finish stacked per-roundtrip 2s EAGAIN stalls: {took:?} (calls={})\n{said}",
        pack.calls.load(Ordering::SeqCst)
    );
    assert!(
        pack.calls.load(Ordering::SeqCst) <= 8,
        "finish made too many server calls across shared budget: {}\n{said}",
        pack.calls.load(Ordering::SeqCst)
    );
}

#[test]
fn seal_ticket_in_passing_skips_checkout_without_tracker() {
    let empty = tempfile::tempdir().unwrap();
    let home = empty.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    let work = empty.path().join("not-a-tracker");
    std::fs::create_dir_all(&work).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["sync"])
        .current_dir(&work)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = text_of(&out);
    assert!(
        !work.join(".gitattributes").exists(),
        "sync wrote .gitattributes into non-tracker checkout:\n{said}"
    );
}

#[test]
fn queued_seal_with_missing_or_empty_root_does_not_seal_in_another_checkout() {
    let seat = Seat::open();
    let base_rev = git(&seat.tracker(), &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    let pack = serve_pack(PackMode::AlwaysBusy503);
    pack.healthy.store(true, Ordering::SeqCst);

    // Seed the mock pack with a lesson for demo-busy, then reset the tracker
    // checkout back to base_rev so seat.tracker() has no sealed logs yet.
    // Draining empty/missing-root seals must not fall back to seat.tracker().
    let seed = seat.run(
        &["finish", "demo-busy", "--lesson", LESSON],
        &pack.url,
        "500",
        false,
    );
    assert!(seed.status.success(), "{}", text_of(&seed));
    assert_eq!(pack.posts.load(Ordering::SeqCst), 1);
    git(&seat.tracker(), &["reset", "--hard", &base_rev]);
    git(&seat.tracker(), &["clean", "-fd"]);
    assert!(sealed_logs(&seat.tracker()).is_empty());

    std::fs::create_dir_all(seat.state_ljos()).unwrap();
    let missing_root = seat.root.path().join("vanished-checkout");
    let queue_lines = format!(
        "{}\n{}\n",
        serde_json::json!({
            "issue": "demo-busy",
            "root": "",
            "at": 1_700_000_000u64,
        }),
        serde_json::json!({
            "issue": "demo-busy",
            "root": missing_root.display().to_string(),
            "at": 1_700_000_001u64,
        }),
    );
    std::fs::write(seat.state_ljos().join("seal-queue.jsonl"), queue_lines).unwrap();

    let next = seat.run(&["ready"], &pack.url, "500", true);
    let next_said = text_of(&next);
    assert!(next.status.success(), "{next_said}");
    assert!(
        sealed_logs(&seat.tracker()).is_empty(),
        "queued seal with empty/missing root sealed into an unrelated checkout:\n{next_said}"
    );
    let leftover =
        std::fs::read_to_string(seat.state_ljos().join("seal-queue.jsonl")).unwrap_or_default();
    assert!(
        leftover.trim().is_empty(),
        "dropped empty/missing root seals should not stay queued: {leftover:?}"
    );
}

#[test]
fn queued_seal_reaches_all_configured_sync_roots() {
    let seat = Seat::open();
    let second = seat.root.path().join("tracker-two");
    let second_proj = second.join("Software/demo");
    std::fs::create_dir_all(second.join(".ljos")).unwrap();
    std::fs::create_dir_all(&second_proj).unwrap();
    git(&second, &["init", "-q", "-b", "main"]);
    git(&second, &["config", "core.hooksPath", "/dev/null"]);
    git(&second, &["config", "user.name", "ada"]);
    git(&second, &["config", "user.email", "ada@example.invalid"]);
    std::fs::write(second.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        second.join(".ljos/sync.toml"),
        "name = \"probe\"\nrecipients = [\"age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p\"]\n",
    )
    .unwrap();
    git(&second, &["add", "."]);
    git(&second, &["commit", "-q", "-m", "seed"]);

    let config_sync = seat.root.path().join("home/.config/ljos/sync.toml");
    std::fs::write(
        &config_sync,
        format!(
            "repos = [\"{}\", \"{}\"]\n",
            seat.tracker().display(),
            second.display()
        ),
    )
    .unwrap();

    let pack = serve_pack(PackMode::AlwaysBusy503);
    let out = seat.run(
        &["finish", "demo-busy", "--lesson", LESSON],
        &pack.url,
        "200",
        false,
    );
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    let queued =
        std::fs::read_to_string(seat.state_ljos().join("seal-queue.jsonl")).unwrap_or_default();
    assert!(
        queued.contains(&seat.tracker().display().to_string()),
        "missing first sync root in seal-queue.jsonl:\n{queued}"
    );
    assert!(
        queued.contains(&second.display().to_string()),
        "missing second sync root in seal-queue.jsonl:\n{queued}"
    );

    pack.healthy.store(true, Ordering::SeqCst);
    let next = seat.run(&["ready"], &pack.url, "500", false);
    let next_said = text_of(&next);
    assert!(next.status.success(), "{next_said}");
    assert!(
        next_said.contains("sync: sealed queued ticket(s): demo-busy"),
        "expected sealed line on drain:\n{next_said}"
    );
    assert!(
        !sealed_logs(&seat.tracker()).is_empty(),
        "first sync root was not sealed:\n{next_said}"
    );
    assert!(
        !sealed_logs(&second).is_empty(),
        "second sync root was not sealed:\n{next_said}"
    );
    let log2 = git(&second, &["log", "-n", "1", "--oneline"]);
    assert!(
        log2.contains("chore(sync):"),
        "missing sync commit in second root: {log2}"
    );
    assert_eq!(
        std::fs::read_to_string(seat.state_ljos().join("seal-queue.jsonl")).unwrap_or_default(),
        "",
        "seal-queue.jsonl was not drained across both roots"
    );
}

#[test]
fn queued_seal_not_reported_sealed_when_nothing_sealed() {
    let seat = Seat::open();
    let pack = serve_pack(PackMode::AlwaysBusy503);
    pack.healthy.store(true, Ordering::SeqCst);
    std::fs::create_dir_all(seat.state_ljos()).unwrap();
    let row = serde_json::json!({
        "issue": "demo-busy",
        "root": seat.tracker().display().to_string(),
        "at": 1_700_000_000u64,
    });
    std::fs::write(
        seat.state_ljos().join("seal-queue.jsonl"),
        format!("{row}\n"),
    )
    .unwrap();

    // With LJOS_TRACKER_GIT=off nothing can be sealed, so it must stay queued
    // and must not report "sealed queued ticket(s)".
    let home = seat.root.path().join("home");
    let path = format!("{}:/usr/bin:/bin", seat.root.path().display());
    let off_run = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["ready"])
        .current_dir(seat.root.path())
        .env_clear()
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", seat.root.path().join("state"))
        .env("XDG_RUNTIME_DIR", seat.root.path().join("run"))
        .env("VISSUE_ROOT", seat.tracker())
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PACKSET_URL", &pack.url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "500")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let off_said = text_of(&off_run);
    assert!(off_run.status.success(), "{off_said}");
    assert!(
        !off_said.contains("sync: sealed queued ticket(s)"),
        "reported sealed when LJOS_TRACKER_GIT=off:\n{off_said}"
    );
    assert!(
        !std::fs::read_to_string(seat.state_ljos().join("seal-queue.jsonl"))
            .unwrap_or_default()
            .trim()
            .is_empty(),
        "queued seal was dropped under LJOS_TRACKER_GIT=off:\n{off_said}"
    );

    // With LJOS_TRACKER_GIT=commit and 0 atoms in the pack for demo-busy,
    // nothing is sealed either, so drain must not report sealed.
    let empty_run = seat.run(&["ready"], &pack.url, "500", false);
    let empty_said = text_of(&empty_run);
    assert!(empty_run.status.success(), "{empty_said}");
    assert!(
        !empty_said.contains("sync: sealed queued ticket(s)"),
        "reported sealed when no atoms existed to seal:\n{empty_said}"
    );
    assert!(
        sealed_logs(&seat.tracker()).is_empty(),
        "unexpected .age log created when pack held 0 atoms:\n{empty_said}"
    );
    assert_eq!(
        std::fs::read_to_string(seat.state_ljos().join("seal-queue.jsonl")).unwrap_or_default(),
        "",
        "empty queued seal should be removed once checked"
    );
}

#[test]
fn finish_with_already_queued_seal_prints_one_coherent_sync_line() {
    let seat = Seat::open();
    let pack = serve_pack(PackMode::AlwaysBusy503);
    std::fs::create_dir_all(seat.state_ljos()).unwrap();
    let prior = serde_json::json!({
        "issue": "demo-prior",
        "root": seat.tracker().display().to_string(),
        "at": 1_700_000_000u64,
    });
    std::fs::write(
        seat.state_ljos().join("seal-queue.jsonl"),
        format!("{prior}\n"),
    )
    .unwrap();

    let out = seat.run(
        &["finish", "demo-busy", "--lesson", LESSON],
        &pack.url,
        "200",
        false,
    );
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        !said.contains("The sync step did not finish"),
        "finish emitted contradictory 'did not finish' alongside queued sync:\n{said}"
    );
    assert!(
        !said.contains("ljos sync"),
        "finish told caller to run ljos sync while queued:\n{said}"
    );
    assert_eq!(
        said.matches(SYNC_QUEUED_LINE).count(),
        1,
        "expected exactly one coherent queued sync line:\n{said}"
    );
}

#[test]
fn finish_without_queued_note_under_503_prints_exact_doctor_line() {
    let seat = Seat::open();
    let pack = serve_pack(PackMode::AlwaysBusy503);
    let out = seat.run(&["finish", "demo-busy"], &pack.url, "200", false);
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("The sync step could not reach the pack, so run `ljos doctor`."),
        "expected clean sync doctor sentence on unqueued 503 finish:\n{said}"
    );
    assert!(
        !said.contains("The tracker write could not be committed"),
        "finish doubled the doctor clause:\n{said}"
    );
    assert_eq!(
        said.matches("ljos doctor").count(),
        1,
        "finish printed ljos doctor more than once:\n{said}"
    );
}
