//! A sitting on a decision ticket in a pack with no personas yet.
//!
//! The getting-started page files `Pick a codec` as a decision. On a new
//! seat the pack has no personas, which is not a failure. The sitting used
//! to print "the panel could not be written" and stored the refusal as an
//! attempt. It now says in one line that no panel is seated and names the
//! command that writes a persona.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The first file named `name` on this process's `PATH`.
fn found(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

struct Seat {
    dir: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn on(url: &str) -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = dir.path().join("bin");
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\n");
        for name in ["vissue", "packset", "packsetd"] {
            if let Some(real) = found(name) {
                std::os::unix::fs::symlink(real, bin.join(name)).unwrap();
            }
        }
        for sub in ["home", "run", "tracker/Software"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        std::fs::write(
            dir.path().join("home/.gitconfig"),
            "[user]\n\tname = Your Name\n\temail = you@example.com\n",
        )
        .unwrap();
        Self {
            dir,
            url: url.to_string(),
        }
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let home = self.home();
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(&home)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.dir.path().join("bin").display()),
            )
            .env("HOME", &home)
            .env("USER", "ada")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    fn decision(&self) -> String {
        let (code, said) = self.run(&[
            "file",
            "Pick a codec",
            "-p",
            "demo",
            "-t",
            "decision",
            "--options",
            "keep,drop",
        ]);
        assert_eq!(code, 0, "{said}");
        said.lines()
            .find(|line| line.starts_with("demo-") && !line.contains(' '))
            .unwrap_or_else(|| panic!("no id in:\n{said}"))
            .to_string()
    }
}

fn pack() -> Option<ljos_test_boot::Packsetd> {
    if tools::halt(tools::Tool::Packsetd) || tools::halt(tools::Tool::Vissue) {
        return None;
    }
    Some(ljos_test_boot::Packsetd::open())
}

#[test]
fn a_decision_on_a_pack_with_no_personas_skips_the_panel_in_one_line() {
    let Some(daemon) = pack() else {
        return;
    };
    let seat = Seat::on(daemon.url());
    let id = seat.decision();
    let (code, said) = seat.run(&["sitting", &id]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.contains(&format!(
            "{id} is a decision. The pack has no personas yet, so no panel is seated; `ljos persona NAME --anchor A --view ...` adds one."
        )),
        "{said}"
    );
    assert!(!said.contains("could not be written"), "{said}");
    assert!(!said.contains(concat!("ho", "lds no personas")), "{said}");
    assert!(
        !said.contains("A panel of personas on one bound recipe"),
        "the compact sitting printed the whole playbook:\n{said}"
    );
    let line = said
        .lines()
        .find(|line| line.contains("is a decision"))
        .unwrap();
    assert!(line.len() < 160, "{line}");
}

#[test]
fn a_decision_on_a_pack_with_a_persona_still_writes_the_panel() {
    let Some(daemon) = pack() else {
        return;
    };
    let seat = Seat::on(daemon.url());
    let (code, said) = seat.run(&[
        "persona",
        "editor",
        "--anchor",
        "0.5",
        "--view",
        "reads the codec change first",
        "--about",
        "codec",
    ]);
    assert_eq!(code, 0, "{said}");
    let id = seat.decision();
    let (code, said) = seat.run(&["sitting", &id]);
    assert_eq!(code, 0, "{said}");
    assert!(
        said.contains(&format!(
            "{id} is a decision. Run the panel before the work"
        )),
        "{said}"
    );
    assert!(!said.contains("no personas yet"), "{said}");
}
