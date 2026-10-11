//! Mail reaches a seat that is not at a prompt.
//!
//! Two seats share packsetd on a free port in a fresh home. The recipient
//! sets a wake command; a send starts it, and the command reads the
//! recipient's inbox. Interrupt mail holds a stop in the shape Claude Code
//! and Cursor send, once. A seat the pack has never heard from is refused
//! with the seat it most likely meant.

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("free port");
    listener.local_addr().expect("addr").port()
}

struct Home {
    dir: tempfile::TempDir,
    port: u16,
    path: String,
}

impl Home {
    /// One `ljos` run as `seat`, with only this home's environment.
    fn ljos(&self, seat: &str, args: &[&str], stdin: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args);
        self.env(&mut cmd);
        cmd.env("LJOS_SEAT", seat);
        run(cmd, stdin)
    }

    fn env(&self, cmd: &mut Command) {
        let home = self.dir.path();
        cmd.env_clear()
            .current_dir(home)
            .env("HOME", home)
            .env("PATH", &self.path)
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("PACKSET_PORT", self.port.to_string())
            .env("PACKSET_AUTH", "off")
            .env("LJOS_TRACKER_GIT", "off");
    }
}

fn run(mut cmd: Command, stdin: Option<&str>) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut pipe = child.stdin.take().expect("stdin");
    if let Some(text) = stdin {
        pipe.write_all(text.as_bytes()).expect("write stdin");
    }
    drop(pipe);
    child.wait_with_output().expect("wait")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn home() -> (Home, Daemon) {
    let Some(packsetd) = on_path("packsetd") else {
        panic!("packsetd is not on PATH; CI builds it from packset develop");
    };
    let dir = tempfile::tempdir().expect("home");
    for sub in [
        "tmp",
        "run",
        ".config",
        ".local/share",
        ".local/state",
        ".cache",
    ] {
        std::fs::create_dir_all(dir.path().join(sub)).expect("home dirs");
    }
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos"))
        .parent()
        .expect("bin dir");
    let path = std::env::join_paths([
        bin_dir,
        packsetd.parent().expect("packsetd dir"),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .expect("PATH")
    .into_string()
    .expect("utf-8 PATH");
    let home = Home {
        port: free_port(),
        path,
        dir,
    };
    let pack_home = home.dir.path().join(".local/share/packset");
    std::fs::create_dir_all(&pack_home).expect("pack home");
    let mut daemon = Command::new(&packsetd);
    daemon.args([
        "--port",
        &home.port.to_string(),
        "--home",
        pack_home.to_str().expect("utf-8"),
    ]);
    home.env(&mut daemon);
    let daemon = Daemon(
        daemon
            .env("PACKSET_HOME", &pack_home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn packsetd"),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while std::net::TcpStream::connect(("127.0.0.1", home.port)).is_err() {
        assert!(Instant::now() < deadline, "packsetd did not listen");
        std::thread::sleep(Duration::from_millis(50));
    }
    (home, daemon)
}

/// Wait for `path` to hold `needle`, and say how long it took.
fn wait_for(path: &Path, needle: &str, limit: Duration) -> Option<Duration> {
    let started = Instant::now();
    while started.elapsed() < limit {
        if std::fs::read_to_string(path).is_ok_and(|t| t.contains(needle)) {
            return Some(started.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

const SENDER: &str = "grok-bot";
const RECIPIENT: &str = "claude-review";

#[test]
fn a_send_wakes_the_recipient_and_an_interrupt_holds_its_stop() {
    let (home, _daemon) = home();
    // The recipient is known once it has written to the pack.
    let joined = home.ljos(RECIPIENT, &["group", "crew", "--add", RECIPIENT], None);
    assert!(joined.status.success(), "{}", text(&joined));

    let missed = home.ljos(SENDER, &["send", "claude", "hello"], None);
    assert!(!missed.status.success(), "{}", text(&missed));
    assert!(
        text(&missed).contains("Did you mean claude-review?"),
        "{}",
        text(&missed)
    );

    // A wake the guard refuses is not set.
    let refused = home.ljos(
        RECIPIENT,
        &["wake", "--command", "kubectl delete namespace prod"],
        None,
    );
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("the guard says"),
        "{}",
        text(&refused)
    );

    let script = home.dir.path().join("wake.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nljos inbox > \"$HOME/woke.tmp\" && mv \"$HOME/woke.tmp\" \"$HOME/woke.$LJOS_WAKE_INTERRUPT\"\n",
    )
    .expect("script");
    let set = home.ljos(
        RECIPIENT,
        &["wake", "--command", &format!("sh {}", script.display())],
        None,
    );
    assert!(set.status.success(), "{}", text(&set));

    let started = Instant::now();
    let sent = home.ljos(
        SENDER,
        &[
            "send",
            RECIPIENT,
            "Software-d9n6 is yours",
            "--issue",
            "Software-d9n6",
        ],
        None,
    );
    let send_took = started.elapsed();
    assert!(sent.status.success(), "{}", text(&sent));
    assert!(
        text(&sent).contains(&format!("waking {RECIPIENT}")),
        "{}",
        text(&sent)
    );
    let woke = wait_for(
        &home.dir.path().join("woke.0"),
        "Software-d9n6 is yours",
        Duration::from_secs(20),
    );
    let log = std::fs::read_to_string(home.dir.path().join(".local/state/ljos/mail/wake.log"))
        .unwrap_or_default();
    let woke = woke.unwrap_or_else(|| panic!("the wake did not read the inbox: {log}"));
    eprintln!(
        "measure: send returned in {} ms; the recipient's wake read the message {} ms after send returned",
        send_took.as_millis(),
        woke.as_millis()
    );

    // An interrupt holds the stop of a running session once, in each
    // runner's words.
    let urgent = home.ljos(
        SENDER,
        &[
            "send",
            RECIPIENT,
            "stop the release, CI is red",
            "--interrupt",
        ],
        None,
    );
    assert!(urgent.status.success(), "{}", text(&urgent));
    let claude_stop = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "claude-1",
        "cwd": home.dir.path(),
        "stop_hook_active": false,
    })
    .to_string();
    let held = home.ljos(RECIPIENT, &["hook"], Some(&claude_stop));
    assert!(held.status.success(), "{}", text(&held));
    let answer: Value =
        serde_json::from_slice(&held.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&held)));
    assert_eq!(answer["decision"], "block", "{answer}");
    let reason = answer["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("stop the release, CI is red"), "{reason}");
    assert!(!reason.contains("Software-d9n6 is yours"), "{reason}");
    eprintln!("measure: Claude Code Stop answer: {answer}");

    let again = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "claude-2",
        "cwd": home.dir.path(),
        "stop_hook_active": false,
    })
    .to_string();
    let quiet = home.ljos(RECIPIENT, &["hook"], Some(&again));
    assert!(
        !text(&quiet).contains("CI is red"),
        "a delivered letter holds the stop again: {}",
        text(&quiet)
    );

    let cursor = home.ljos(
        SENDER,
        &[
            "send",
            RECIPIENT,
            "rebase is off, merge develop",
            "--interrupt",
        ],
        None,
    );
    assert!(cursor.status.success(), "{}", text(&cursor));
    let cursor_stop = serde_json::json!({
        "hook_event_name": "stop",
        "cursor_version": "3.2.0",
        "conversation_id": "conv-9",
        "generation_id": "gen-9",
        "status": "completed",
        "loop_count": 0,
    })
    .to_string();
    let held = home.ljos(RECIPIENT, &["hook"], Some(&cursor_stop));
    let answer: Value =
        serde_json::from_slice(&held.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&held)));
    let follow = answer["followup_message"].as_str().unwrap_or_default();
    assert!(follow.contains("rebase is off, merge develop"), "{answer}");
    eprintln!("measure: Cursor stop answer: {answer}");
}
