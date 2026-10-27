//! The git `ljos prefer --user --sync` runs to share user rules, under a
//! caller's exported repository. Runs the built `ljos` binary with a
//! temporary home and global git config.
#![cfg(unix)]

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const HOST: &str = "probe-host";
const SUBJECT: &str = "chore(sync): probe-host user rules";

const RULE: &str = r#"{"rules":[{"id":"ur-probe","text":"probe rule","author":"rohit","reason":"probe","written_at":"2026-10-25T00:00:00Z","updated_at":"2026-10-25T00:00:00Z","basis":"person"}]}"#;

struct Scratch {
    dir: tempfile::TempDir,
    gitconfig: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let gitconfig = dir.path().join("gitconfig");
        std::fs::write(&gitconfig, "").unwrap();
        for sub in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(
            dir.path().join("run"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        Self { dir, gitconfig }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// git for the test's own setup and reads. No hook runs, whatever the
    /// global file names.
    fn git(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new("git")
            .arg("-c")
            .arg("core.hooksPath=/dev/null")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .env_clear()
            .env("PATH", path_var())
            .env("HOME", self.path("home"))
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
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

    fn clone(&self, remote: &Path, rel: &str, name: &str) -> PathBuf {
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
        self.must(&dest, &["config", "user.name", name]);
        self.must(
            &dest,
            &["config", "user.email", &format!("{name}@example.invalid")],
        );
        self.must(&dest, &["config", "commit.gpgsign", "false"]);
        dest
    }

    fn commit_all(&self, cwd: &Path, message: &str) {
        self.must(cwd, &["add", "-A"]);
        self.must(cwd, &["commit", "-q", "-m", message]);
        self.must(cwd, &["push", "-q", "origin", "HEAD:main"]);
    }

    fn bare(&self, rel: &str) -> PathBuf {
        let dest = self.path(rel);
        self.must(
            self.dir.path(),
            &["init", "-q", "--bare", "-b", "main", dest.to_str().unwrap()],
        );
        dest
    }

    /// A tracker clone whose remote is one commit ahead of it. The other
    /// machine's clone names a scope and holds that machine's rule.
    fn tracker(&self) -> (PathBuf, PathBuf) {
        let remote = self.bare("tracker.git");
        let other = self.clone(&remote, "other", "other");
        std::fs::create_dir_all(other.join(".ljos/user-rules")).unwrap();
        std::fs::write(
            other.join(".ljos/sync.toml"),
            format!(
                "name = \"probe\"\nrecipients = [\"{}\"]\n",
                age_recipient(self.dir.path())
            ),
        )
        .unwrap();
        std::fs::write(other.join(".ljos/user-rules/other.json"), RULE).unwrap();
        self.commit_all(&other, "scope");
        let tracker = self.clone(&remote, "tracker", "seat");
        std::fs::write(other.join("ahead"), "ahead\n").unwrap();
        self.commit_all(&other, "upstream ahead");
        (remote, tracker)
    }

    /// A repository of the caller's own, pushed to its own remote.
    fn caller(&self) -> (PathBuf, PathBuf) {
        let remote = self.bare("caller.git");
        let caller = self.clone(&remote, "caller", "caller");
        std::fs::write(caller.join("README"), "caller\n").unwrap();
        self.must(&caller, &["add", "README"]);
        self.must(&caller, &["commit", "-q", "-m", "caller"]);
        self.must(&caller, &["push", "-q", "-u", "origin", "HEAD:main"]);
        (remote, caller)
    }

    /// `ljos prefer --user --sync` against `tracker`, pushing.
    fn exchange(&self, tracker: &Path, env: &[(&str, &Path)]) -> Output {
        let home = self.path("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(["prefer", "--user", "--sync"])
            .current_dir(&home)
            .env_clear()
            .env("PATH", path_var())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_TEST_HOSTNAME", HOST)
            .env("LJOS_USER_RULES_TRACKER", tracker)
            .env("LJOS_USER_RULES_SYNC", "on")
            .env("LJOS_TRACKER_GIT", "push")
            .stdin(Stdio::null());
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }
}

fn path_var() -> String {
    std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn age_recipient(dir: &Path) -> String {
    let key = dir.join("age.key");
    let _ = std::fs::remove_file(&key);
    let made = Command::new("age-keygen")
        .arg("-o")
        .arg(&key)
        .output()
        .expect("age-keygen is on PATH");
    assert!(made.status.success(), "age-keygen");
    let public = Command::new("age-keygen")
        .arg("-y")
        .arg(&key)
        .output()
        .expect("age-keygen -y");
    assert!(public.status.success(), "age-keygen -y");
    String::from_utf8_lossy(&public.stdout).trim().to_string()
}

/// What a write to the caller's repository would change: its HEAD, its
/// index, its work tree, its exclude file and its remote.
fn caller_state(s: &Scratch, caller: &Path, remote: &Path) -> Vec<String> {
    vec![
        s.must(caller, &["rev-parse", "HEAD"]),
        s.must(caller, &["ls-files", "--stage"]),
        s.must(caller, &["status", "--porcelain", "--ignored"]),
        std::fs::read_to_string(caller.join(".git/info/exclude")).unwrap_or_default(),
        s.must(remote, &["rev-parse", "main"]),
    ]
}

fn assert_tracker_shared(s: &Scratch, tracker: &Path, remote: &Path, text: &str) {
    let subjects = s.must(tracker, &["log", "--format=%s"]);
    assert_eq!(subjects.lines().next(), Some(SUBJECT), "{subjects}\n{text}");
    assert!(
        subjects.lines().any(|line| line == "upstream ahead"),
        "the tracker did not merge its upstream: {subjects}\n{text}"
    );
    assert_eq!(
        s.must(remote, &["rev-parse", "main"]),
        s.must(tracker, &["rev-parse", "HEAD"]),
        "the user rules were not pushed\n{text}"
    );
}

/// A hook exports `GIT_DIR`, and a worktree script may also export
/// `GIT_WORK_TREE`. Either way the caller's repository gains no commit,
/// no staged file, no lock line and no push, and the tracker gets the
/// user-rules commit.
#[test]
fn an_exported_git_dir_does_not_take_the_user_rules_commit() {
    let s = Scratch::new();
    let (caller_remote, caller) = s.caller();
    let git_dir = caller.join(".git");
    let exports: [&[(&str, &Path)]; 2] = [
        &[("GIT_DIR", &git_dir)],
        &[("GIT_DIR", &git_dir), ("GIT_WORK_TREE", &caller)],
    ];
    for (round, export) in exports.into_iter().enumerate() {
        let (remote, tracker) = s.tracker();
        let before = caller_state(&s, &caller, &caller_remote);
        let out = s.exchange(&tracker, export);
        let text = said(&out);
        assert!(out.status.success(), "round {round}: {text}");
        assert_eq!(
            caller_state(&s, &caller, &caller_remote),
            before,
            "round {round}: the caller's repository changed\n{text}"
        );
        assert!(!caller.join(".ljos-commit.lock").exists(), "{text}");
        assert!(!caller.join(".ljos").exists(), "{text}");
        assert_tracker_shared(&s, &tracker, &remote, &text);
        for dir in ["tracker", "tracker.git", "other"] {
            std::fs::remove_dir_all(s.path(dir)).unwrap();
        }
    }
}
