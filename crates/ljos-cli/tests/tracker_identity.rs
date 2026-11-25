//! A tracker commit or merge takes its identity from git config alone.
//!
//! The seat that calls ljos can export an author and a committer in
//! several ways: the `GIT_AUTHOR_*` and `GIT_COMMITTER_*` variables,
//! `EMAIL`, `user.*`, `author.*` or `committer.*` keys in
//! `GIT_CONFIG_PARAMETERS` or in `GIT_CONFIG_COUNT` pairs, and an include
//! named either way. A commit on another machine's tracker once came out
//! under the calling runner's name. Each test runs the built `ljos` binary
//! with one of those in its environment and reads back the commits it
//! made: a ticket commit, a tracker merge from `ljos inbox --sync`, and the
//! user-rules merge and commit from `ljos prefer --user --sync`.
#![cfg(unix)]

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The identity the tracker checkout's own config names.
const SEAT: &str = "seat <seat@example.invalid>";

/// Every leaked identity carries this, so a commit that took one shows it.
const LEAK: &str = "leak";

const RULES_SUBJECT: &str = "chore(sync): probe-host user rules";

const RULE: &str = r#"{"rules":[{"id":"ur-probe","text":"probe rule","author":"rohit","reason":"probe","written_at":"2026-10-25T00:00:00Z","updated_at":"2026-10-25T00:00:00Z","basis":"person"}]}"#;

/// One way the calling environment names a commit identity.
#[derive(Clone, Copy)]
enum Channel {
    IdentityEnv,
    Email,
    ParametersUser,
    ParametersAuthorCommitter,
    ParametersNotUtf8,
    ParametersInclude,
    CountedUser,
    CountedAuthorCommitter,
    CountedInclude,
}

fn os(s: &str) -> OsString {
    OsString::from(s)
}

impl Channel {
    /// Whether the tracker's config names an email. `EMAIL` is git's
    /// fallback for a missing `user.email`, so that channel needs none.
    fn repo_email(self) -> bool {
        !matches!(self, Self::Email)
    }

    /// The variables this channel exports. An include file is written
    /// under `dir`.
    fn env(self, dir: &Path) -> Vec<(OsString, OsString)> {
        let include = dir.join("leak.gitconfig");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            &include,
            "[user]\n\tname = include leak\n\temail = include@leak.invalid\n",
        )
        .unwrap();
        let include = include.into_os_string().into_string().unwrap();
        let pairs = |pairs: &[(&str, &str)]| -> Vec<(OsString, OsString)> {
            pairs.iter().map(|(k, v)| (os(k), os(v))).collect()
        };
        match self {
            Self::IdentityEnv => pairs(&[
                ("GIT_AUTHOR_NAME", "author leak"),
                ("GIT_AUTHOR_EMAIL", "author@leak.invalid"),
                ("GIT_COMMITTER_NAME", "committer leak"),
                ("GIT_COMMITTER_EMAIL", "committer@leak.invalid"),
            ]),
            Self::Email => pairs(&[("EMAIL", "email@leak.invalid")]),
            Self::ParametersUser => pairs(&[(
                "GIT_CONFIG_PARAMETERS",
                "'core.abbrev=9' 'user.name'='param leak' 'user.email=param@leak.invalid'",
            )]),
            Self::ParametersAuthorCommitter => pairs(&[(
                "GIT_CONFIG_PARAMETERS",
                "'author.name'='author leak' 'author.email'='author@leak.invalid' \
                 'committer.name=committer leak' 'committer.email=committer@leak.invalid'",
            )]),
            Self::ParametersNotUtf8 => vec![(
                os("GIT_CONFIG_PARAMETERS"),
                OsString::from_vec(
                    b"'ljos.probe=\xff' 'user.name=bytes leak' 'user.email'='bytes@leak.invalid'"
                        .to_vec(),
                ),
            )],
            Self::ParametersInclude => pairs(&[(
                "GIT_CONFIG_PARAMETERS",
                &format!("'include.path'='{include}'"),
            )]),
            Self::CountedUser => pairs(&[
                ("GIT_CONFIG_COUNT", "3"),
                ("GIT_CONFIG_KEY_0", "core.abbrev"),
                ("GIT_CONFIG_VALUE_0", "9"),
                ("GIT_CONFIG_KEY_1", "user.name"),
                ("GIT_CONFIG_VALUE_1", "counted leak"),
                ("GIT_CONFIG_KEY_2", "user.email"),
                ("GIT_CONFIG_VALUE_2", "counted@leak.invalid"),
            ]),
            Self::CountedAuthorCommitter => pairs(&[
                ("GIT_CONFIG_COUNT", "4"),
                ("GIT_CONFIG_KEY_0", "author.name"),
                ("GIT_CONFIG_VALUE_0", "author leak"),
                ("GIT_CONFIG_KEY_1", "author.email"),
                ("GIT_CONFIG_VALUE_1", "author@leak.invalid"),
                ("GIT_CONFIG_KEY_2", "committer.name"),
                ("GIT_CONFIG_VALUE_2", "committer leak"),
                ("GIT_CONFIG_KEY_3", "committer.email"),
                ("GIT_CONFIG_VALUE_3", "committer@leak.invalid"),
            ]),
            Self::CountedInclude => pairs(&[
                ("GIT_CONFIG_COUNT", "1"),
                ("GIT_CONFIG_KEY_0", "includeIf.gitdir:/.path"),
                ("GIT_CONFIG_VALUE_0", &include),
            ]),
        }
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn script(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    ljos_cli::install_executable(&dir.join(name), body).unwrap();
}

fn path_var(bin: &Path) -> String {
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
    )
}

/// A home, a run directory and stubs for the programs a write asks.
struct Scratch {
    dir: tempfile::TempDir,
    gitconfig: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let gitconfig = dir.path().join("gitconfig");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&gitconfig, "").unwrap();
        for sub in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        ljos_cli::change_mode(dir.path().join("run"), 0o700).unwrap();
        let bin = dir.path().join("bin");
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  get) echo 'not found' >&2; exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\n");
        script(&bin, "curl", "#!/bin/sh\nexit 1\n");
        Self { dir, gitconfig }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// git for the test's own setup and reads, with no hooks and its own
    /// identity, whatever the checkout's config says.
    fn git(&self, cwd: &Path, args: &[&str]) -> Output {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new("git")
            .args(["-c", "core.hooksPath=/dev/null"])
            .args([
                "-c",
                "user.name=setup",
                "-c",
                "user.email=setup@example.invalid",
            ])
            .args(["-c", "commit.gpgsign=false"])
            .arg("-C")
            .arg(cwd)
            .args(args)
            .env_clear()
            .env("PATH", path_var(&self.path("bin")))
            .env("HOME", self.path("home"))
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap();
    }

    fn must(&self, cwd: &Path, args: &[&str]) -> String {
        let out = self.git(cwd, args);
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The checkout's own identity: `seat`, with or without an email.
    fn seat_config(&self, repo: &Path, email: bool) {
        self.must(repo, &["config", "user.name", "seat"]);
        if email {
            self.must(repo, &["config", "user.email", "seat@example.invalid"]);
        }
        self.must(repo, &["config", "commit.gpgsign", "false"]);
    }

    fn write(&self, path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(path, body).unwrap();
    }

    fn commit_file(&self, repo: &Path, rel: &str, message: &str) {
        self.write(&repo.join(rel), &format!("{rel}\n"));
        self.must(repo, &["add", "-A"]);
        self.must(repo, &["commit", "-q", "-m", message]);
    }

    fn bare(&self, rel: &str) -> PathBuf {
        let dest = self.path(rel);
        self.must(
            self.dir.path(),
            &["init", "-q", "--bare", "-b", "main", dest.to_str().unwrap()],
        );
        self.must(&dest, &["config", "core.hooksPath", "/dev/null"]);
        dest
    }

    fn clone(&self, remote: &Path, rel: &str) -> PathBuf {
        let dest = self.path(rel);
        self.must(
            self.dir.path(),
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                dest.to_str().unwrap(),
            ],
        );
        dest
    }

    /// A tracker clone that committed while its remote moved on, so the
    /// next sync has to merge. `scope` names a sync scope for user rules.
    fn diverged(&self, email: bool, scope: bool) -> (PathBuf, PathBuf) {
        let remote = self.bare("tracker.git");
        let other = self.clone(&remote, "other");
        if scope {
            self.write(
                &other.join(".ljos/sync.toml"),
                &format!(
                    "name = \"probe\"\nrecipients = [\"{}\"]\n",
                    age_recipient(self.dir.path())
                ),
            );
            self.write(&other.join(".ljos/user-rules/other.json"), RULE);
        }
        self.commit_file(&other, "first", "first");
        self.must(&other, &["push", "-q", "origin", "HEAD:main"]);
        let tracker = self.clone(&remote, "tracker");
        self.seat_config(&tracker, email);
        self.commit_file(&tracker, "local", "local");
        self.commit_file(&other, "ahead", "upstream ahead");
        self.must(&other, &["push", "-q", "origin", "HEAD:main"]);
        (remote, tracker)
    }

    /// `ljos` with a home of its own, the tracker at `tracker`, and
    /// `channel`'s variables on top.
    fn ljos(&self, tracker: &Path, channel: Channel, args: &[&str]) -> Output {
        self.ljos_with(tracker, channel.env(self.dir.path()), args)
    }

    /// `ljos` as [`Scratch::ljos`] runs it, with `env` on top.
    fn ljos_with(&self, tracker: &Path, env: Vec<(OsString, OsString)>, args: &[&str]) -> Output {
        let home = self.path("home");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(&home)
            .env_clear()
            .env("PATH", path_var(&self.path("bin")))
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("VISSUE_ROOT", tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_LEASE_SECS", "0")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("LJOS_TEST_HOSTNAME", "probe-host")
            .env("PACKSET_URL", "off")
            .stdin(Stdio::null());
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }

    fn head(&self, repo: &Path) -> String {
        self.must(repo, &["rev-parse", "HEAD"])
    }

    /// `author | committer | parents | subject` of each commit ljos added
    /// on top of `since`, newest first. A merged branch's own commits are
    /// not ljos's, so only the first parent is followed.
    fn commits(&self, repo: &Path, since: &str) -> Vec<String> {
        let out = self.git(
            repo,
            &[
                "log",
                "--first-parent",
                "--format=%an <%ae> | %cn <%ce> | %p | %s",
                &format!("{since}..HEAD"),
            ],
        );
        if !out.status.success() {
            return Vec::new();
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|line| {
                let mut parts: Vec<String> = line.split(" | ").map(str::to_string).collect();
                parts[2] = parts[2].split(' ').count().to_string();
                parts.join(" | ")
            })
            .collect()
    }
}

fn age_recipient(dir: &Path) -> String {
    let key = dir.join("age.key");
    let _ = std::fs::remove_file(&key);
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let made = Command::new("age-keygen")
        .arg("-o")
        .arg(&key)
        .output()
        .expect("age-keygen is on PATH");
    assert!(made.status.success(), "age-keygen");
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let public = Command::new("age-keygen")
        .arg("-y")
        .arg(&key)
        .output()
        .expect("age-keygen -y");
    assert!(public.status.success(), "age-keygen -y");
    String::from_utf8_lossy(&public.stdout).trim().to_string()
}

/// No commit names a leaked identity, and, when the checkout names an
/// email, every one names the checkout's.
fn assert_seat(channel: Channel, commits: &[String], text: &str) {
    for commit in commits {
        assert!(
            !commit.contains(LEAK),
            "a commit took the caller's identity: {commit}\n{commits:#?}\n{text}"
        );
        if channel.repo_email() {
            assert!(
                commit.starts_with(&format!("{SEAT} | {SEAT} | ")),
                "{commit}\n{commits:#?}\n{text}"
            );
        }
    }
}

/// `ljos file` commits the new ticket.
fn ticket_commit(channel: Channel) {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let s = Scratch::new();
    let tracker = s.path("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    s.must(&tracker, &["init", "-q", "-b", "main"]);
    s.seat_config(&tracker, channel.repo_email());
    s.commit_file(&tracker, "first", "first");
    let before = s.head(&tracker);
    let out = s.ljos(&tracker, channel, &["file", "Pick a codec", "-p", "demo"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let made = s.commits(&tracker, &before);
    if channel.repo_email() {
        assert_eq!(made.len(), 1, "no ticket commit: {made:#?}\n{text}");
    }
    assert_seat(channel, &made, &text);
}

/// `ljos inbox --sync` fetches the tracker and merges its upstream.
fn tracker_merge(channel: Channel) {
    let s = Scratch::new();
    let (_remote, tracker) = s.diverged(channel.repo_email(), false);
    let before = s.head(&tracker);
    let out = s.ljos(&tracker, channel, &["inbox", "--sync"]);
    let text = said(&out);
    let made = s.commits(&tracker, &before);
    if channel.repo_email() {
        assert!(
            made.len() == 1 && made[0].contains(" | 2 | "),
            "no merge commit: {made:#?}\n{text}"
        );
    }
    assert_seat(channel, &made, &text);
}

/// `ljos prefer --user --sync` merges the tracker's upstream, commits this
/// host's user rules and pushes.
fn user_rules(channel: Channel) {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let s = Scratch::new();
    let (remote, tracker) = s.diverged(channel.repo_email(), true);
    let before = s.head(&tracker);
    let out = s.ljos(&tracker, channel, &["prefer", "--user", "--sync"]);
    let text = said(&out);
    let made = s.commits(&tracker, &before);
    if channel.repo_email() {
        assert!(out.status.success(), "{text}");
        assert!(
            made.len() == 2 && made[0].ends_with(RULES_SUBJECT) && made[1].contains(" | 2 | "),
            "no merge and user-rules commit: {made:#?}\n{text}"
        );
        assert_eq!(
            s.must(&remote, &["rev-parse", "main"]),
            s.head(&tracker),
            "the user rules were not pushed\n{text}"
        );
    }
    assert_seat(channel, &made, &text);
}

macro_rules! every_channel {
    ($path:ident) => {
        mod $path {
            use super::Channel;

            #[test]
            fn identity_env() {
                super::$path(Channel::IdentityEnv);
            }

            #[test]
            fn email() {
                super::$path(Channel::Email);
            }

            #[test]
            fn parameters_user() {
                super::$path(Channel::ParametersUser);
            }

            #[test]
            fn parameters_author_committer() {
                super::$path(Channel::ParametersAuthorCommitter);
            }

            #[test]
            fn parameters_not_utf8() {
                super::$path(Channel::ParametersNotUtf8);
            }

            #[test]
            fn parameters_include() {
                super::$path(Channel::ParametersInclude);
            }

            #[test]
            fn counted_user() {
                super::$path(Channel::CountedUser);
            }

            #[test]
            fn counted_author_committer() {
                super::$path(Channel::CountedAuthorCommitter);
            }

            #[test]
            fn counted_include() {
                super::$path(Channel::CountedInclude);
            }
        }
    };
}

every_channel!(ticket_commit);
every_channel!(tracker_merge);
every_channel!(user_rules);

/// A `GIT_CONFIG_COUNT` larger than the environment holds once made ljos
/// size a vector from it, and `ljos sync` died of the failed allocation.
/// The sync now commits nothing, and the doctor names the variable.
#[test]
fn a_huge_config_count_is_a_plain_refusal() {
    let s = Scratch::new();
    let tracker = s.path("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    s.must(&tracker, &["init", "-q", "-b", "main"]);
    s.seat_config(&tracker, true);
    s.commit_file(&tracker, "first", "first");
    let before = s.head(&tracker);
    let huge = || vec![(os("GIT_CONFIG_COUNT"), os("99999999999999"))];
    let out = s.ljos_with(&tracker, huge(), &["sync"]);
    let text = said(&out);
    assert_eq!(out.status.signal(), None, "ljos was killed\n{text}");
    assert!(s.commits(&tracker, &before).is_empty(), "{text}");
    let doctor = said(&s.ljos_with(&tracker, huge(), &["doctor", "--all"]));
    let row = doctor
        .lines()
        .find(|line| line.split('\t').nth(1) == Some("git identity"))
        .unwrap_or_else(|| panic!("no git identity row:\n{doctor}"));
    assert!(row.starts_with("warn\t"), "{row}");
    assert!(
        row.contains("GIT_CONFIG_COUNT=99999999999999 is more entries than git reads"),
        "{row}"
    );
}
