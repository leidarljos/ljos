//! A ballot key and a handover key have separate jobs.
//!
//! Voting creates the seat ballot key. The later handover uses the host key,
//! so the deed store in the same home accepts the manifest signature.

ljos_test_boot::harness!();

#[path = "common/sibling.rs"]
mod sibling;

use std::path::PathBuf;
use std::process::{Command, Output};

struct Home {
    root: tempfile::TempDir,
    path: String,
}

impl Home {
    fn new() -> Self {
        let vissue = sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"));
        let deedar = sibling::require("deedar").unwrap_or_else(|line| panic!("{line}"));
        let root = tempfile::tempdir().unwrap();
        for dir in [
            "home/.config/deedar",
            "home/.local/share/deedar/store",
            "home/.local/state",
            "run",
            "tracker/Software",
        ] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        std::fs::write(root.path().join("home/.config/deedar/host.key"), [9u8; 32]).unwrap();
        let mut dirs = vec![
            vissue.parent().unwrap().to_path_buf(),
            deedar.parent().unwrap().to_path_buf(),
        ];
        dirs.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(dirs)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let home = Self { root, path };
        let accepted = home.deedar(&["host", "accept"]);
        assert!(accepted.status.success(), "{}", said(&accepted));
        home
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn command(&self, program: &str) -> Command {
        let home = self.home();
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &self.path)
            .env("HOME", &home)
            .env("USER", "ada")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env(
                "DEEDAR_URL",
                format!(
                    "file://{}",
                    home.join(".local/share/deedar/store").display()
                ),
            )
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("PACKSET_URL", "off")
            .current_dir(self.root.path());
        cmd
    }

    fn deedar(&self, args: &[&str]) -> Output {
        self.command("deedar").args(args).output().unwrap()
    }

    fn ljos(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .output()
            .unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn vote_then_handover_then_receive_succeeds_in_one_home() {
    let home = Home::new();
    let filed = home.ljos(&[
        "file",
        "Pick a codec",
        "-p",
        "demo",
        "-t",
        "decision",
        "--options",
        "keep,drop",
    ]);
    assert!(filed.status.success(), "{}", said(&filed));
    let issue = String::from_utf8_lossy(&filed.stdout).trim().to_string();

    let voted = home.ljos(&["vote", &issue, "--for", "keep", "--used", "none"]);
    assert!(voted.status.success(), "{}", said(&voted));
    assert!(home
        .home()
        .join(".local/share/ljos/seats/alice/ballot.key")
        .is_file());

    let bag = home.root.path().join("bag");
    let bag_arg = bag.display().to_string();
    let sent = home.ljos(&["handover", "--out", &bag_arg, "--issue", &issue]);
    assert!(sent.status.success(), "{}", said(&sent));

    let received = home.ljos(&["receive", &bag_arg]);
    assert!(received.status.success(), "{}", said(&received));
    assert!(
        !String::from_utf8_lossy(&received.stderr)
            .contains("which is not a key this reader accepts"),
        "{}",
        said(&received)
    );
}
