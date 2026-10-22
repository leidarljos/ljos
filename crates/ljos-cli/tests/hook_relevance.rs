//! The prompt hook ranks what it hands over by how much of the prompt a
//! memory bears on, not by its age or one rare word.
//!
//! A fresh home, the hooks `ljos onboard --harness claude` writes, packsetd
//! on a free port. Lessons are posted with the stamp they were written at,
//! so the pack's fused score decays the old ones as it would in use.

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const FRESH: &str = "2026-10-21T09:00:00.000Z";
const JANUARY: &str = "2026-01-14T09:00:00.000Z";

const CI_SECRET: &str = "The registry rejected the token because the CI secret \
                         expired; refresh it from 1Password.";

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

struct Seat {
    dir: tempfile::TempDir,
    port: u16,
    path: String,
    hook: String,
    _daemon: Daemon,
}

impl Seat {
    fn env(&self, cmd: &mut Command) {
        env_for(cmd, self.dir.path(), &self.path, self.port);
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// A standing lesson written at `ts`.
    fn lesson(&self, text: &str, ts: &str) {
        let atom = serde_json::json!({
            "schema": "inside.atom/v1",
            "kind": "lesson",
            "level": "explicit",
            "text": text,
            "workspace": ljos_cli::SEAT_WORKSPACE,
            "entities": ["horizon:standing"],
            "ts": ts,
        });
        let posted = ureq::post(&format!("{}/v1/atoms", self.url()))
            .timeout(Duration::from_secs(10))
            .send_json(atom);
        assert!(posted.is_ok(), "post {text:?}: {posted:?}");
    }

    /// The context the prompt hook onboard wrote gives back, and how long
    /// the hook took.
    fn prompt(&self, session: &str, prompt: &str) -> (String, Duration) {
        let payload = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": session,
            "cwd": self.dir.path(),
            "prompt": prompt,
        })
        .to_string();
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", &self.hook]);
        self.env(&mut cmd);
        let started = Instant::now();
        let said = run(cmd, Some(&payload));
        let took = started.elapsed();
        assert!(said.status.success(), "{}: {}", self.hook, text(&said));
        if said.stdout.iter().all(u8::is_ascii_whitespace) {
            return (String::new(), took);
        }
        let out: Value = serde_json::from_slice(&said.stdout)
            .unwrap_or_else(|e| panic!("{e}: {} printed {}", self.hook, text(&said)));
        let context = out["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        (context, took)
    }
}

fn env_for(cmd: &mut Command, home: &Path, path: &str, port: u16) {
    cmd.env_clear()
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", path)
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.join("tmp"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PACKSET_PORT", port.to_string())
        .env("PACKSET_AUTH", "off")
        .env("LJOS_TRACKER_GIT", "off");
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

/// A fresh home with packsetd on a free port and the prompt hook `ljos
/// onboard --harness claude` wrote.
fn seat() -> Seat {
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
    let port = free_port();
    let pack_home = dir.path().join(".local/share/packset");
    std::fs::create_dir_all(&pack_home).expect("pack home");
    let mut daemon = Command::new(&packsetd);
    daemon.args([
        "--port",
        &port.to_string(),
        "--home",
        pack_home.to_str().expect("utf-8"),
    ]);
    env_for(&mut daemon, dir.path(), &path, port);
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
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "packsetd did not listen");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut onboard = Command::new(env!("CARGO_BIN_EXE_ljos"));
    onboard.args(["onboard", "--harness", "claude"]);
    env_for(&mut onboard, dir.path(), &path, port);
    let onboarded = run(onboard, None);
    let settings = dir.path().join(".claude/settings.json");
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(&settings).unwrap_or_else(|e| panic!("{e}: {}", text(&onboarded))),
    )
    .expect("settings.json");
    let hook = settings["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap_or_else(|| panic!("no UserPromptSubmit hook: {settings}"))
        .to_string();
    Seat {
        dir,
        port,
        path,
        hook,
        _daemon: daemon,
    }
}

/// The review's case: every lookalike names the prompt's subject, so a
/// flat score tied them all with the January lesson that answers it, and
/// the age-decayed fused score cut that lesson behind three fresh ones.
#[test]
fn the_lesson_that_covers_the_prompt_beats_fresher_lookalikes() {
    let seat = seat();
    seat.lesson(CI_SECRET, JANUARY);
    for lookalike in [
        "Figma rejected the export token for the design file; ask the design lead.",
        "The quokka sticker registry hands out one token per sticker pack.",
        "Host 13 rejected the deploy token after the reboot; re-add it there.",
        "The staging registry rejected a token once during the outage.",
    ] {
        seat.lesson(lookalike, FRESH);
    }
    let (context, took) = seat.prompt("registry-1", "the registry rejected my token");
    eprintln!("prompt hook took {took:?}");
    assert!(
        context.contains("CI secret expired"),
        "the hook left out the lesson that answers the prompt: {context}"
    );
}

/// A prompt that says `always` is not about every rule that says it.
#[test]
fn always_does_not_bring_in_a_rule_about_something_else() {
    let seat = seat();
    seat.lesson("Always squash commits before merging to main.", FRESH);
    seat.lesson(
        "Format the Rust code with cargo fmt, not rustfmt alone.",
        FRESH,
    );
    let (context, _) = seat.prompt("always-1", "always run the formatter");
    assert!(
        !context.contains("squash commits"),
        "a shared `always` brought in an unrelated rule: {context}"
    );
}
