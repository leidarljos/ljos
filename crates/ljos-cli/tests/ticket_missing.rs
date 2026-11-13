//! A ticket id the tracker does not hold names the next command.
//!
//! `ljos show`, `ljos finish` and `ljos file --parent` used to stop at
//! "not found" and leave a new seat to guess.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

struct Home {
    dir: tempfile::TempDir,
    root: std::path::PathBuf,
    home: std::path::PathBuf,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        std::fs::create_dir_all(root.join("Software")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        Self { dir, root, home }
    }

    fn ljos(&self, args: &[&str]) -> std::process::Output {
        let path = format!("{}:/usr/bin:/bin", self.dir.path().display());
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_OFF", "1")
            .output()
            .unwrap()
    }
}

fn said(out: &std::process::Output) -> String {
    format!(
        "status {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn names_ready(out: &std::process::Output) {
    let text = said(out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("ljos ready"), "no next step:\n{text}");
    assert!(!text.contains("Caused by"), "{text}");
}

#[test]
fn show_of_an_unknown_id_names_ready() {
    let home = Home::new();
    names_ready(&home.ljos(&["show", "demo-none"]));
}

#[test]
fn finish_of_an_unknown_id_names_ready() {
    let home = Home::new();
    names_ready(&home.ljos(&[
        "finish",
        "demo-none",
        "--lesson",
        "The missing ticket needs a next step.",
    ]));
}

#[test]
fn file_parent_of_an_unknown_id_names_ready() {
    let home = Home::new();
    let stub = home.dir.path().join("vissue");
    std::fs::write(
        &stub,
        "#!/bin/sh\necho '--parent demo-none does not refer to any known id' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    names_ready(&home.ljos(&[
        "file",
        "Name the child",
        "--parent",
        "demo-none",
        "-p",
        "demo",
    ]));
}
