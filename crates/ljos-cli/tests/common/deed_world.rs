//! A home with a real `vissue` and `deedar`, and a git tracker in commit
//! mode, for the tests that store deeds and file tickets.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[path = "sibling.rs"]
mod sibling;

pub const ISSUE: &str = "demo-a1b2";

pub struct World {
    root: tempfile::TempDir,
    path: String,
}

impl World {
    pub fn new() -> Self {
        let vissue = sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"));
        let deedar = sibling::require("deedar").unwrap_or_else(|line| panic!("{line}"));
        let root = tempfile::tempdir().unwrap();
        for dir in [
            "home/.config/deedar",
            "home/.local/share/deedar/store",
            "home/.local/state",
            "run",
            "tracker/Software/demo",
        ] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        #[expect(clippy::disallowed_methods, reason = "writes a file that is not a git hook")] std::fs::write(
            root.path().join("gitconfig"),
            "[core]\n\thooksPath = /dev/null\n[user]\n\tname = Ada Example\n\temail = ada@example.invalid\n",
        )
        .unwrap();
        let mut dirs = vec![
            vissue.parent().unwrap().to_path_buf(),
            deedar.parent().unwrap().to_path_buf(),
        ];
        dirs.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
        let path = std::env::join_paths(dirs)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let world = Self { root, path };
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            world.tracker().join("Software/demo/issues.org"),
            format!("* TODO [#B] Scoreboard\n:PROPERTIES:\n:ID:         {ISSUE}\n:END:\n"),
        )
        .unwrap();
        world.git(&["init", "-q"]);
        world.git(&["add", "."]);
        world.git(&["commit", "-q", "-m", "seed"]);
        world
    }

    pub fn dir(&self) -> &Path {
        self.root.path()
    }

    pub fn tracker(&self) -> PathBuf {
        self.root.path().join("tracker")
    }

    pub fn store(&self) -> PathBuf {
        self.root.path().join("home/.local/share/deedar/store")
    }

    pub fn command(&self, program: &str) -> Command {
        let home = self.root.path().join("home");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env("PATH", &self.path)
            .env("HOME", &home)
            .env("USER", "ada")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("DEEDAR_URL", format!("file://{}", self.store().display()))
            .env("GIT_CONFIG_GLOBAL", self.root.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("VISSUE_ROOT", self.tracker())
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("LJOS_SYNC_HOST", "runner")
            .env("PACKSET_URL", "off")
            .current_dir(self.root.path())
            .stdin(Stdio::null());
        cmd
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = self
            .command("git")
            .arg("-C")
            .arg(self.tracker())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", said(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn commits(&self) -> usize {
        self.git(&["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }

    pub fn ljos(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .output()
            .unwrap()
    }

    pub fn write(&self, name: &str, text: impl AsRef<[u8]>) -> String {
        let path = self.root.path().join(name);
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    }

    /// The tracker's files and their bytes, to compare before and after.
    pub fn tracker_files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![self.tracker()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push((path.clone(), std::fs::read(&path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }
}

pub fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Send `calls` to `ljos-mcp` with every tool listed, and return the
/// answers by id.
pub fn mcp(world: &World, calls: &[serde_json::Value]) -> Vec<serde_json::Value> {
    use std::io::Write as _;
    let mut lines = vec![
        serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    lines.extend(calls.iter().cloned());
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_ljos"));
    let path = format!("{}:{}", bin.parent().unwrap().display(), world.path);
    let mut child = world
        .command(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env("PATH", path)
        .env("LJOS_MCP_TOOLS", "all")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(120));
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!stderr(&out).contains("panicked"), "{}", said(&out));
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The text blocks of the answer to call `id`.
pub fn answer(answers: &[serde_json::Value], id: u64) -> (bool, String) {
    let call = answers
        .iter()
        .find(|a| a["id"] == id)
        .unwrap_or_else(|| panic!("no answer {id}: {answers:?}"));
    assert!(call.get("error").is_none(), "{call}");
    let text = call["result"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    (call["result"]["isError"] == true, text)
}
