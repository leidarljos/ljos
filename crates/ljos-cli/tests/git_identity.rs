//! A seat whose git has no author identity.
//!
//! The tracker commits each write, and git refuses a commit it cannot sign
//! with a name and an email. `ljos doctor` used to print "ljos is ready"
//! and the first `ljos file` then said only "Author identity unknown". The
//! doctor now flags the missing identity, and both lines name the two
//! `git config` commands that fix it.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NAME_COMMAND: &str = "git config --global user.name \"Your Name\"";
const EMAIL_COMMAND: &str = "git config --global user.email \"you@example.com\"";

fn script(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    ljos_cli::install_executable(&path, body).unwrap();
}

/// The directory of the `vissue` the test run has on `PATH`.
fn vissue_dir() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .find(|dir| dir.join("vissue").is_file())
        .unwrap_or_else(|| PathBuf::from("/usr/bin"))
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A home with no git configuration, a tracker that is a git checkout, and
/// the deed store and claim graph stubbed.
struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    fn new() -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = dir.path().join("bin");
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  get) echo 'not found' >&2; exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\n");
        script(&bin, "curl", "#!/bin/sh\nexit 1\n");
        for sub in ["home", "run", "tracker/Software"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        let seat = Self { dir };
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(seat.tracker())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(init.status.success(), "{}", said(&init));
        seat
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn tracker(&self) -> PathBuf {
        self.dir.path().join("tracker")
    }

    /// `identity` is the global git configuration. `None` leaves git with
    /// no name and no email, and `user.useConfigOnly` keeps it from
    /// guessing them from the account and the host.
    fn command(&self, identity: Option<&str>) -> Command {
        let global = self.dir.path().join("gitconfig");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&global, identity.unwrap_or("")).unwrap();
        let path = format!(
            "{}:{}:/usr/bin:/bin",
            self.dir.path().join("bin").display(),
            vissue_dir().display()
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.current_dir(self.home())
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join(".config"))
            .env("XDG_DATA_HOME", self.home().join(".local/share"))
            .env("XDG_STATE_HOME", self.home().join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("GIT_CONFIG_GLOBAL", &global)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "user.useConfigOnly")
            .env("GIT_CONFIG_VALUE_0", "true")
            .env("VISSUE_ROOT", self.tracker())
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_LEASE_SECS", "0")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("PACKSET_URL", "off")
            .stdin(Stdio::null());
        cmd
    }

    fn ljos(&self, identity: Option<&str>, args: &[&str]) -> Output {
        self.command(identity).args(args).output().unwrap()
    }
}

fn row<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines()
        .find(|line| line.split('\t').nth(1) == Some(name))
}

#[test]
fn doctor_flags_a_missing_git_identity_and_names_the_commands() {
    let seat = Seat::new();
    let out = seat.ljos(None, &["doctor"]);
    let text = said(&out);
    assert!(text.contains("ljos is not ready"), "{text}");
    assert!(!text.contains("ljos is ready"), "{text}");
    let line = row(&text, "git identity").unwrap_or_else(|| panic!("no git identity row:\n{text}"));
    assert!(line.starts_with("warn\t"), "{line}");
    assert!(line.contains(NAME_COMMAND), "{line}");
    assert!(line.contains(EMAIL_COMMAND), "{line}");
}

#[test]
fn a_warning_row_gets_an_action_summary() {
    let rows = [ljos_cli::Habitat {
        name: "git identity",
        state: "set user.name and user.email".to_string(),
        ok: false,
    }];
    let text = ljos_cli::format_doctor_short(&rows);
    assert_eq!(
        text,
        "ljos needs action: 1 row needs action. Every check: `ljos doctor --all`.\n\
warn\tgit identity\tset user.name and user.email\n"
    );
    assert!(!text.contains("ljos is ready"), "{text}");
}

#[test]
fn doctor_says_nothing_of_git_identity_when_git_has_one() {
    let seat = Seat::new();
    let identity = "[user]\n\tname = Ada Example\n\temail = ada@example.com\n";
    let short = said(&seat.ljos(Some(identity), &["doctor"]));
    assert!(
        row(&short, "git identity").is_none(),
        "a row for a set identity is not an action:\n{short}"
    );
    let all = said(&seat.ljos(Some(identity), &["doctor", "--all"]));
    let line = row(&all, "git identity").unwrap_or_else(|| panic!("no git identity row:\n{all}"));
    assert!(line.starts_with("ok\t"), "{line}");
    assert!(
        !line.contains("Ada Example") && !line.contains("ada@example.com"),
        "the row names the person:\n{line}"
    );
}

/// A tracker commit drops the identity the environment exports, so the
/// row does not count one either: it warns as it would with none.
#[test]
fn an_identity_only_in_the_environment_is_not_one() {
    let exports: [&[(&str, &str)]; 2] = [
        &[
            ("GIT_AUTHOR_NAME", "Ada Example"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada Example"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ],
        &[(
            "GIT_CONFIG_PARAMETERS",
            "'user.name'='Ada Example' 'user.email=ada@example.com'",
        )],
    ];
    for export in exports {
        let seat = Seat::new();
        let mut cmd = seat.command(None);
        cmd.args(["doctor", "--all"]);
        for (key, value) in export {
            cmd.env(key, value);
        }
        let text = said(&cmd.output().unwrap());
        let line =
            row(&text, "git identity").unwrap_or_else(|| panic!("no git identity row:\n{text}"));
        assert!(line.starts_with("warn\t"), "{export:?}: {line}");
        assert!(line.contains(NAME_COMMAND), "{line}");
    }
}

/// An identity in git config is one, whatever the environment exports.
#[test]
fn an_identity_in_git_config_is_one_whatever_the_environment_says() {
    let seat = Seat::new();
    let identity = "[user]\n\tname = Ada Example\n\temail = ada@example.com\n";
    let out = seat
        .command(Some(identity))
        .args(["doctor", "--all"])
        .env("GIT_AUTHOR_NAME", "Other")
        .env("GIT_CONFIG_PARAMETERS", "'user.name=Other'")
        .output()
        .unwrap();
    let text = said(&out);
    let line = row(&text, "git identity").unwrap_or_else(|| panic!("no git identity row:\n{text}"));
    assert!(line.starts_with("ok\t"), "{line}");
}

#[test]
fn a_refused_tracker_commit_names_the_commands() {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let seat = Seat::new();
    let out = seat.ljos(None, &["file", "Pick a codec", "-p", "demo"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(!id.is_empty() && !id.contains(' '), "{text}");
    assert!(text.contains("Author identity unknown"), "{text}");
    assert!(text.contains(NAME_COMMAND), "{text}");
    assert!(text.contains(EMAIL_COMMAND), "{text}");
    assert!(
        std::fs::read_to_string(seat.tracker().join("Software/demo/issues.org"))
            .unwrap()
            .contains(&id),
        "the ticket is not on disk"
    );
}
