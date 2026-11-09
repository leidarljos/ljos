//! A search is a lookup. It does not raise the recall count. A prompt
//! that is handed the lesson does.

ljos_test_boot::harness!();

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const LESSON: &str = "Fire the zircon kiln before dusk on the north bench.";
const PROMPT: &str = "Fire the zircon kiln before dusk on the north bench today.";

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
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        script(
            &bin,
            "vissue",
            "#!/bin/sh\nprintf '%s\\n' 'vissue test' 'root=.' 'prefix=Software'\nexit 0\n",
        );
        script(&bin, "claimdag", "#!/bin/sh\nexit 1\n");
        script(&bin, "deedar", "#!/bin/sh\necho head\nexit 0\n");
        Self {
            root,
            url: url.to_string(),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let root = self.root.path();
        let home = root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(root)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.join("bin").display()),
            )
            .env("HOME", &home)
            .env("USER", "person")
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> String {
        let mut cmd = self.command(args);
        if stdin.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        let mut child = cmd.spawn().expect("ljos");
        if let Some(text) = stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        }
        let out = child.wait_with_output().expect("ljos");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{args:?}\n{text}");
        text
    }
}

fn counts(list: &str) -> (u64, u64) {
    let line = list
        .lines()
        .find(|line| line.contains("recalled ") && line.contains("looked up "))
        .unwrap_or_else(|| panic!("no counts in:\n{list}"));
    let number = |marker: &str| -> u64 {
        line.split(marker)
            .nth(1)
            .and_then(|rest| rest.split('x').next())
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("no {marker} in {line}"))
    };
    (number("recalled "), number("looked up "))
}

#[test]
fn a_search_is_a_lookup_and_a_prompt_is_a_recall() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let daemon = ljos_test_boot::Packsetd::open();
    let seat = Seat::on(daemon.url());
    let remembered = seat.run(&["remember", LESSON], None);
    assert!(remembered.contains(LESSON), "{remembered}");

    let before = seat.run(&["memories", "list", "--user"], None);
    assert!(before.contains(LESSON), "{before}");
    assert_eq!(counts(&before), (0, 0), "{before}");

    let searched = seat.run(&["search", "zircon kiln dusk"], None);
    assert!(searched.contains("zircon"), "{searched}");
    let after_search = seat.run(&["memories", "list", "--user"], None);
    assert_eq!(counts(&after_search), (0, 1), "{after_search}");

    let prompt = seat.run(&["hook", "--prompt", "--text"], Some(PROMPT));
    assert!(
        prompt.contains("zircon kiln"),
        "the prompt was not handed the lesson:\n{prompt}"
    );
    let after_prompt = seat.run(&["memories", "list", "--user"], None);
    let (recalls, lookups) = counts(&after_prompt);
    assert_eq!(lookups, 1, "{after_prompt}");
    assert!(recalls >= 1, "{after_prompt}");
}
