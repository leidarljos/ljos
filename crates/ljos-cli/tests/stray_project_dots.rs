//! A project name that is a dot path names no project.
//!
//! A project is one directory under the tracker's prefix. `.` and `..` passed
//! the name check, so the ticket went to `Software/issues.org` or to
//! `issues.org` beside the prefix directory, outside any project. So did a
//! name that folds to dots under NFKC: U+2024 is `.` and two U+FF0E are `..`.
//! `ljos file` and the MCP `ljos_file` tool refuse each of them, and no file
//! or directory is written.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

/// Names that are no project. Each one is tried on the CLI and over MCP.
const NAMES: &[&str] = &[".", "..", "./", "a/..", "", "\u{2024}", "\u{ff0e}\u{ff0e}"];

struct Place {
    _root: tempfile::TempDir,
    product: PathBuf,
    tracker: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    run: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let out = ljos_test_boot::git()
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=seat",
            "-c",
            "user.email=seat@example.test",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn tree(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            out.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                walk(&path, root, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

impl Place {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let product = root.path().join("product");
        let tracker = root.path().join("tracker");
        let bin = root.path().join("bin");
        let home = root.path().join("home");
        let run = root.path().join("run");
        for dir in [&product, &tracker, &bin, &home, &run] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
        git(&product, &["init", "-q", "-b", "main"]);
        std::fs::write(product.join("README"), "a product\n").unwrap();
        git(&product, &["add", "README"]);
        git(&product, &["commit", "-q", "-m", "product"]);
        git(&tracker, &["init", "-q", "-b", "main"]);
        let layout = vissue_core::Layout::new(&tracker, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        vissue_core::ops::create(
            &layout,
            "demo",
            "an existing ticket",
            vissue_core::CreateOpts {
                quiet: true,
                ..vissue_core::CreateOpts::default()
            },
        )
        .unwrap();
        git(&tracker, &["add", "-A"]);
        git(&tracker, &["commit", "-q", "-m", "tracker"]);
        Self {
            _root: root,
            product,
            tracker,
            bin,
            home,
            run,
        }
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(&self.product)
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    self.bin.display(),
                    vissue_dir().display()
                ),
            )
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("VISSUE_NO_ROUTE", "1")
            .env("VISSUE_ROOT", &self.tracker)
            .stdin(Stdio::null());
        cmd
    }

    fn ljos(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .output()
            .unwrap()
    }

    /// One `ljos_file` call to a fresh `ljos-mcp`: `(isError, text)`.
    fn mcp_file(&self, project: &str) -> (bool, String) {
        let mut child = self
            .command(env!("CARGO_BIN_EXE_ljos-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(40));
            let _ = Command::new("kill").arg(pid.to_string()).status();
        });
        let mut stdin = child.stdin.take().unwrap();
        for line in [
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"probe","version":"0"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"ljos_file",
                "arguments":{"seat":"saddle","title":"stray ticket","project":project}}}),
        ] {
            writeln!(stdin, "{line}").unwrap();
        }
        stdin.flush().unwrap();
        drop(stdin);
        let mut answer = None;
        for line in BufReader::new(child.stdout.take().unwrap()).lines() {
            let Ok(line) = line else { break };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if value["id"] == 2 {
                answer = Some(value);
                break;
            }
        }
        let _ = child.wait();
        let answer = answer.expect("no answer from ljos_file");
        let text = answer["result"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let failed = answer["result"]["isError"] == true || answer.get("error").is_some();
        (failed, text)
    }

    fn untouched(&self, before: &[String]) -> Result<(), String> {
        if tree(&self.tracker) != before {
            return Err("it wrote the tracker".into());
        }
        for stray in ["issues.org", "Software/issues.org"] {
            if self.tracker.join(stray).exists() {
                return Err(format!("it wrote {stray}"));
            }
        }
        Ok(())
    }
}

fn vissue_dir() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .find(|dir| dir.join("vissue").is_file())
        .unwrap_or_default()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

fn refused_by_the_cli(name: &str) -> Result<(), String> {
    let place = Place::new();
    let before = tree(&place.tracker);
    let out = place.ljos(&["file", "stray ticket", "-p", name]);
    let said = stderr(&out);
    if out.status.code() != Some(1) || !out.stdout.is_empty() || said.lines().count() != 1 {
        return Err(format!("status {}, stderr {said:?}", out.status));
    }
    place.untouched(&before)
}

fn refused_by_mcp(name: &str) -> Result<(), String> {
    let place = Place::new();
    let before = tree(&place.tracker);
    let (failed, text) = place.mcp_file(name);
    if !failed || !text.contains("project:") {
        return Err(format!("the call answered {text:?}"));
    }
    place.untouched(&before)
}

fn every_name(check: fn(&str) -> Result<(), String>) {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let failures: Vec<String> = NAMES
        .iter()
        .filter_map(|name| check(name).err().map(|why| format!("{name:?}: {why}")))
        .collect();
    assert!(failures.is_empty(), "not refused:\n{}", failures.join("\n"));
}

#[test]
fn a_dot_project_is_refused_by_the_cli_and_writes_nothing() {
    every_name(refused_by_the_cli);
}

#[test]
fn a_dot_project_is_refused_by_mcp_and_writes_nothing() {
    every_name(refused_by_mcp);
}
