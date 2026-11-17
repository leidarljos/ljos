//! `ljos onboard` with non-TTY stdout prints the setup steps on stdout
//! without raw `ljos mcp` JSON. `--harness json` prints the MCP entry to
//! paste, parsed as JSON as it stands.
//!
//! A non-TTY `ljos onboard` still sets up the tracker, the memory writer
//! and the deed store, so `ljos file` then files a ticket.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn script(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

struct Seat {
    root: tempfile::TempDir,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap();
        for sub in [
            ".config",
            ".local/share",
            ".local/state",
            ".cache",
            "run",
            "tmp",
            "bin",
        ] {
            std::fs::create_dir_all(root.path().join(sub)).unwrap();
        }
        let bin = root.path().join("bin");
        script(
            &bin,
            "packset",
            "#!/bin/sh\n[ \"$1\" = ensure ] && echo 'PACKSET_URL=http://127.0.0.1:9' && exit 0\nexit 0\n",
        );
        script(
            &bin,
            "deedar",
            "#!/bin/sh\n[ \"$1 $2\" = \"host accept\" ] && echo 'already a signer' && exit 0\nexit 0\n",
        );
        Self { root }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        let home = self.root.path();
        let bins = Path::new(env!("CARGO_BIN_EXE_ljos")).parent().unwrap();
        let mut paths = vec![home.join("bin"), bins.to_path_buf()];
        if let Ok(existing) = std::env::var("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        paths.push(PathBuf::from("/usr/bin"));
        paths.push(PathBuf::from("/bin"));
        let path = std::env::join_paths(paths).unwrap();
        // A writer another test left on 8761 answers 401 for this home.
        // A free port makes the step run the stub `packset ensure`.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(home)
            .env_clear()
            .env("HOME", home)
            .env("PATH", path)
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_SEAT", "ada")
            .env("PACKSET_PORT", port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }
}

#[test]
fn non_tty_onboard_prints_ok_lines_without_json() {
    let seat = Seat::open();
    let out = seat.ljos(&["onboard"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stdout.contains("The tracker, the memory writer and the deed store are set up."),
        "{text}"
    );
    assert!(
        !stdout.contains("\"mcpServers\"")
            && !stdout
                .lines()
                .any(|line| line.trim_start().starts_with('{')),
        "raw MCP JSON leaked onto non-TTY stdout:\n{text}"
    );
    assert!(
        !stderr.contains("The tracker"),
        "setup lines leaked onto stderr:\n{text}"
    );
    assert!(
        !stderr.contains("mcpServers"),
        "raw MCP JSON leaked onto stderr:\n{text}"
    );
}

#[test]
fn onboard_with_harness_json_prints_json_alone() {
    let seat = Seat::open();
    let out = seat.ljos(&["onboard", "--harness", "json"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let entry: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"));
    let command = entry["mcpServers"]["ljos"]["command"]
        .as_str()
        .unwrap_or_default();
    assert!(command.ends_with("ljos-mcp"), "{text}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.is_empty(), "{text}");
}

#[test]
fn non_tty_onboard_with_detected_runner_prints_ok_lines_without_json() {
    let seat = Seat::open();
    let bin = seat.root.path().join("bin");
    script(&bin, "claude", "#!/bin/sh\nexit 0\n");
    let out = seat.ljos(&["onboard"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stdout.contains("ok\trunners\tfound claude"), "{text}");
    assert!(
        !stdout.contains("\"mcpServers\"")
            && !stdout
                .lines()
                .any(|line| line.trim_start().starts_with('{')),
        "raw MCP JSON leaked onto non-TTY stdout:\n{text}"
    );
    assert!(
        !stderr.contains("ok\t") && !stderr.contains("mcpServers"),
        "unexpected stderr:\n{text}"
    );
}

#[test]
fn a_piped_onboard_lets_the_next_file_land() {
    let seat = Seat::open();
    let onboard = seat.ljos(&["onboard"]);
    let text = said(&onboard);
    assert!(onboard.status.success(), "{text}");
    let filed = seat.ljos(&["file", "Pick a codec", "-p", "demo"]);
    let filed_text = said(&filed);
    assert!(filed.status.success(), "{filed_text}");
    let id = String::from_utf8(filed.stdout)
        .unwrap()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    assert!(
        id.starts_with("demo-"),
        "file did not print a ticket id:\n{filed_text}"
    );
    assert!(!filed_text.contains("is not a tracker"), "{filed_text}");
}
