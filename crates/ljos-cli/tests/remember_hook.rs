//! A lesson the person types reaches the next prompt that bears on it.
//!
//! A fresh home, the hooks `ljos onboard --harness claude` writes, packsetd
//! on a free port: `ljos remember`, then a matching prompt through the
//! `UserPromptSubmit` command onboard put in `settings.json`.

ljos_test_boot::harness!();
#[path = "common/runner.rs"]
mod runner;
#[path = "common/terminal.rs"]
mod terminal;

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const LESSON: &str = "Tag a release in this repository with git push origin TAG, \
                      because follow-tags leaves the v-tags behind.";

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
    /// One `ljos` run with only this home's environment.
    fn ljos(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let home = self.dir.path();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args);
        self.env(&mut cmd, home);
        run(cmd, stdin)
    }

    /// [`Home::ljos`] typed at a terminal of the person's own.
    fn on_a_terminal(&self, args: &[&str]) -> Output {
        let home = self.dir.path();
        let mut cmd = Command::new(terminal::PYTHON);
        cmd.args(terminal::shape(
            "terminal",
            env!("CARGO_BIN_EXE_ljos"),
            args,
        ));
        self.env(&mut cmd, home);
        run(cmd, None)
    }

    fn env(&self, cmd: &mut Command, home: &Path) {
        // The pack asks for its token. A preference is this seat's only
        // when a read without the token is refused; the daemon and the
        // client share the token file under this home.
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

/// A fresh home with packsetd on a free port and the hooks `ljos onboard
/// --harness claude` wrote: the home, the daemon, and the prompt hook's
/// command.
fn seat() -> (Home, Daemon, String) {
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
    home.env(&mut daemon, home.dir.path());
    let _daemon = Daemon(
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

    let onboard = home.ljos(&["onboard", "--harness", "claude"], None);
    let settings = home.dir.path().join(".claude/settings.json");
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(&settings).unwrap_or_else(|e| panic!("{e}: {}", text(&onboard))),
    )
    .expect("settings.json");
    let hook = settings["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap_or_else(|| panic!("no UserPromptSubmit hook: {settings}"))
        .to_string();

    (home, _daemon, hook)
}

/// Send one prompt through the hook onboard wrote and return the context
/// it gave back.
fn prompt_through(home: &Home, hook: &str, session: &str, prompt: &str) -> String {
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": session,
        "cwd": home.dir.path(),
        "prompt": prompt,
    })
    .to_string();
    // Claude Code runs the command in `sh -c`; the hook keeps the prompt
    // only when that runner, a program the kernel runs from a file named
    // `claude`, opened the shell.
    let installed = runner::install_dir();
    let runner = installed.path().join("claude");
    runner::runner_at(&runner);
    let mut cmd = Command::new(&runner);
    cmd.args(["-c", "\"$@\"; exit $?", "claude", "/bin/sh", "-c", hook]);
    home.env(&mut cmd, home.dir.path());
    let said = run(cmd, Some(&payload));
    assert!(said.status.success(), "{hook}: {}", text(&said));
    // A hook with nothing to say prints nothing.
    if said.stdout.iter().all(u8::is_ascii_whitespace) {
        return String::new();
    }
    let out: Value = serde_json::from_slice(&said.stdout)
        .unwrap_or_else(|e| panic!("{e}: {hook} printed {}", text(&said)));
    out["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn a_remembered_lesson_reaches_the_prompt_hook_onboard_writes() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let (home, _daemon, hook) = seat();
    let remembered = home.on_a_terminal(&["remember", LESSON]);
    assert!(remembered.status.success(), "{}", text(&remembered));
    if text(&remembered).contains("episode:") || text(&remembered).contains("unreviewed note:") {
        // This test runs under an agent runner, so the words are the
        // agent's and are not a standing lesson. The person's path needs
        // a plain shell.
        eprintln!("skipped: cargo test ran under an agent runner");
        return;
    }
    assert!(
        text(&remembered).contains("standing:"),
        "{}",
        text(&remembered)
    );
    let found = home.ljos(&["search", "how do I tag a release"], None);
    assert!(text(&found).contains("follow-tags"), "{}", text(&found));

    for prompt in ["tag the 0.3.0 release", "how do I tag a release"] {
        let context = prompt_through(&home, &hook, &format!("cold-{}", prompt.len()), prompt);
        assert!(
            context.contains("follow-tags leaves the v-tags behind"),
            "{prompt:?}: the hook did not show the lesson: {context}"
        );
    }
}

/// "From now on always use bun, never yarn" said in chat is saved, in the
/// person's words, on the prompt that says it, and a later session's
/// prompt about yarn gets it back.
#[test]
fn a_choice_said_in_chat_is_saved_and_comes_back() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let (home, _daemon, hook) = seat();
    // The first write loads the pack's encoder; a session warms it first.
    let warm = home.ljos(&["prefer", "Run cargo clippy before every commit"], None);
    assert!(warm.status.success(), "{}", text(&warm));
    let said = "From now on always use bun, never yarn, in this repo";
    let context = prompt_through(&home, &hook, "chat-one", said);
    assert!(
        context.contains("Saved the person's preference"),
        "the hook did not save the stated choice: {context}"
    );
    let found = home.ljos(&["search", "bun yarn"], None);
    assert!(text(&found).contains(said), "{}", text(&found));
    assert!(text(&found).contains("preference"), "{}", text(&found));

    let context = prompt_through(
        &home,
        &hook,
        "chat-two",
        "add lodash as a dev dependency with yarn",
    );
    assert!(
        context.contains("always use bun, never yarn"),
        "the next session did not get the preference: {context}"
    );
}
