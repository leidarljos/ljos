//! `ljos onboard` with no runner prints the MCP entry to paste, and that
//! output has to parse as JSON as it stands.
//!
//! A pipe under a temporary home sets up the tracker and deed store.
//! The setup lines go to stderr. `ljos file` then files a ticket.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn script(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    ljos_cli::install_executable(&path, body).unwrap();
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
        // No pack address or port: naming one keeps memory on in this
        // temporary home, and the onboard lines here are the memory-off ones.
        // TMPDIR is the temp dir the home was made in, so the home is a
        // temporary one wherever the process TMPDIR is.
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(home)
            .env_clear()
            .env("HOME", home)
            .env("PATH", path)
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.parent().unwrap())
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_SEAT", "ada")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
    }
}

#[test]
fn the_bare_onboard_prints_json_alone() {
    let seat = Seat::open();
    let out = seat.ljos(&["onboard"]);
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
    assert!(
        stderr.contains("The tracker and the deed store are set up."),
        "{text}"
    );
    assert!(
        !stdout.contains("The tracker"),
        "setup lines leaked onto stdout:\n{text}"
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
