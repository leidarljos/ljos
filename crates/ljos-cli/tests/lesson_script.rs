//! A lesson stored with no terminal, as a script or cron would run it.
//!
//! There is no tty and no runner. `ljos search` labels the row
//! `lesson, from an agent`, the same words a runner prints.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::path::Path;
use std::process::{Command, Stdio};

const LESSON: &str = "The kiln door seal sets only when the clay is cold.";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Seat {
    root: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn on(url: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        std::fs::create_dir_all(root.path().join("tracker")).unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["vissue", "packset", "packsetd"] {
            if let Some(path) = std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join(name))
                    .find(|p| p.is_file())
            }) {
                let _ = std::os::unix::fs::symlink(&path, bin.join(name));
            }
        }
        if !bin.join("vissue").is_file() {
            script(
                &bin,
                "vissue",
                "#!/bin/sh\necho \"issue $2 not found\" >&2\nexit 1\n",
            );
        }
        Self {
            root,
            url: url.to_string(),
        }
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let root = self.root.path();
        let home = root.join("home");
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(root)
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .env("PATH", format!("{}:{path}", root.join("bin").display()))
            .env("HOME", &home)
            .env("USER", "ada")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("VISSUE_ROOT", root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("ljos");
        (
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

#[test]
fn a_script_stores_a_lesson_from_an_agent() {
    if tools::halt(tools::Tool::Packsetd) {
        return;
    }
    let daemon = ljos_test_boot::Packsetd::open();
    let seat = Seat::on(daemon.url());
    let (code, stdout, stderr) = seat.run(&["remember", LESSON]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, search, err) = seat.run(&["search", "kiln door seal"]);
    let found = format!("{search}{err}");
    assert_eq!(code, 0, "{found}");
    assert!(
        search.contains("lesson, from an agent"),
        "a run with no terminal should name an agent:\n{found}"
    );
    assert!(
        !search.contains("lesson, today"),
        "a run with no terminal is not the terminal row:\n{found}"
    );
    assert!(search.contains("kiln door seal"), "{found}");
}
