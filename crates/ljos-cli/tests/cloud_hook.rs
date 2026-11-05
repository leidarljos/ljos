//! `ljos cloud up` records the push allowlist and installs no git hook.
//! These tests read the allowlist, the doctor row, and the shell guard's
//! lines against a seat with a local bare origin.

ljos_test_boot::harness!();

#[path = "common/guard_mode.rs"]
mod guard_mode;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());

struct Guard {
    saved: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    cwd: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl Guard {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let saved = std::env::vars_os().collect();
        let cwd = std::env::current_dir().unwrap();
        Self {
            saved,
            cwd,
            _lock: lock,
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.cwd);
        let keep: Vec<_> = self.saved.iter().map(|(k, _)| k.clone()).collect();
        for (k, _) in std::env::vars_os() {
            if !keep.iter().any(|have| have == &k) {
                std::env::remove_var(&k);
            }
        }
        for (k, v) in &self.saved {
            std::env::set_var(k, v);
        }
    }
}

fn sh_at(dir: &Path, script: &str) -> std::process::Output {
    guard_mode::command("sh")
        .current_dir(dir)
        .arg("-c")
        .arg(script)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("sh -c in {}: {e}", dir.display()))
}

fn text_of(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn sh_ok(dir: &Path, script: &str) -> String {
    let out = sh_at(dir, script);
    let text = text_of(&out);
    assert!(
        out.status.success(),
        "sh -c {script:?} in {}: {text}",
        dir.display()
    );
    text
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = guard_mode::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        text_of(&out)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn isolate_git(root: &Path) {
    let cfg = root.join("gitconfig");
    fs::write(&cfg, b"").unwrap();
    std::env::set_var("GIT_CONFIG_GLOBAL", &cfg);
    std::env::set_var("GIT_CONFIG_SYSTEM", &cfg);
    std::env::set_var("GIT_TERMINAL_PROMPT", "0");
    std::env::set_var("PATH", "/usr/bin:/bin");
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn seat_env(root: &Path) -> (PathBuf, PathBuf) {
    let home = root.join("home");
    let rel = root.join("rel");
    let xdg = root.join("xdg");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&xdg).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    let mut sums = String::new();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        let out = guard_mode::command("sha256sum")
            .arg(rel.join(name))
            .output()
            .unwrap();
        assert!(out.status.success());
        let hash = String::from_utf8(out.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        sums.push_str(&hash);
        sums.push_str("  ");
        sums.push_str(name);
        sums.push('\n');
    }
    fs::write(rel.join("SHA256SUMS"), sums).unwrap();
    isolate_git(root);
    std::env::set_var("HOME", &home);
    // Not under `/.config/ljos/`, so a write to the hook is the cloud
    // guard's refusal and not the seat-path refusal.
    std::env::set_var("XDG_CONFIG_HOME", &xdg);
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    std::env::set_var("LJOS_GUARD", "on");
    for key in [
        "CURSOR_AGENT",
        "CLAUDECODE",
        "GITHUB_ACTIONS",
        "RUNNER_OS",
        "RUNNER_TEMP",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GITHUB_PAT",
        "CURL_HOME",
        "WGETRC",
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ] {
        std::env::remove_var(key);
    }
    (home, xdg)
}

fn clear_ids() {
    for key in [
        "CURSOR_AGENT_ID",
        "CURSOR_AGENT",
        "CURSOR_CONVERSATION_ID",
        "CURSOR_AGENT_SOCKET",
        "CLOUD_AGENT_ID",
        "AGENT_ID",
        "CODESPACE_NAME",
        "GITHUB_RUN_ID",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CI",
        "BUILDKITE",
        "TF_BUILD",
        "CIRCLECI",
        "JENKINS_URL",
        "CODESPACES",
        "CLAUDE_CODE_AGENT_ID",
        "CODEX_THREAD_ID",
        "LJOS_SEAT",
        "LJOS_SEAT_KEY",
        "VISSUE_AGENT",
    ] {
        std::env::remove_var(key);
    }
}

/// A repo on `develop` with a bare origin beside it.
fn bare_repo(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let origin = root.join(format!("{name}.git"));
    git(
        root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "develop",
            origin.to_str().unwrap(),
        ],
    );
    let dir = root.join(name);
    git(
        root,
        &["init", "-q", "-b", "develop", dir.to_str().unwrap()],
    );
    for (key, value) in [
        ("user.email", "seat@example.invalid"),
        ("user.name", "seat"),
        ("commit.gpgsign", "false"),
    ] {
        git(&dir, &["config", key, value]);
    }
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "seed"]);
    git(&dir, &["remote", "add", "origin", origin.to_str().unwrap()]);
    (origin, dir)
}

fn has_ref(origin: &Path, name: &str) -> bool {
    let out = guard_mode::command("git")
        .arg("-C")
        .arg(origin)
        .args(["show-ref"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line.ends_with(name))
}

/// Every `core.hooksPath` git reads in `dir`, one `origin<TAB>value` line
/// each. `scope` is `--system`, `--global` or `--local`, or `None` for all.
fn hooks_paths(dir: &Path, scope: Option<&str>) -> String {
    let out = guard_mode::command("git")
        .arg("-C")
        .arg(dir)
        .arg("config")
        .args(scope)
        .args(["--show-origin", "--get-all", "core.hooksPath"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn cloud_refusal(line: &str, cwd: &Path) -> Option<ljos_cli::Rule> {
    ljos_cli::static_guard_in(line, Some(cwd))
        .filter(|rule| rule.reason.starts_with("cloud seat rule"))
}

fn refused(line: &str, cwd: &Path) -> ljos_cli::Rule {
    let hit = cloud_refusal(line, cwd).unwrap_or_else(|| panic!("`{line}` was allowed"));
    assert_eq!(hit.verdict, "deny", "{line}: {hit:?}");
    assert_eq!(hit.pattern, "cloud-cursor-push", "{line}: {hit:?}");
    hit
}

fn allowed(line: &str, cwd: &Path) {
    let hit = cloud_refusal(line, cwd);
    assert!(hit.is_none(), "`{line}` was refused: {hit:?}");
}

/// `cloud up` in a fresh home sets `core.hooksPath` at no level, and a
/// later run leaves the value a repository already had.
#[test]
fn cloud_up_sets_no_hooks_path() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    std::env::set_var("LJOS_SEAT", "probe");
    // The global level is the home's own `.gitconfig`, as on a fresh machine.
    std::env::remove_var("GIT_CONFIG_GLOBAL");
    let system = root.path().join("system-gitconfig");
    fs::write(&system, b"").unwrap();
    std::env::set_var("GIT_CONFIG_SYSTEM", &system);
    let (_origin, repo) = bare_repo(root.path(), "seat");
    std::env::set_current_dir(&repo).unwrap();
    let said = ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    for scope in [None, Some("--system"), Some("--global"), Some("--local")] {
        assert_eq!(hooks_paths(&repo, scope), "", "{scope:?}\n{said}");
    }
    assert!(!said.contains("pre-push"), "{said}");

    let booked = root.path().join("booked-hooks");
    fs::create_dir_all(&booked).unwrap();
    executable(&booked.join("pre-push"), "#!/bin/sh\nexit 0\n");
    git(
        &repo,
        &["config", "core.hooksPath", booked.to_str().unwrap()],
    );
    let before = hooks_paths(&repo, None);
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    assert_eq!(hooks_paths(&repo, None), before);
    assert_eq!(
        fs::read_to_string(booked.join("pre-push")).unwrap(),
        "#!/bin/sh\nexit 0\n"
    );
    assert_eq!(fs::read_dir(&booked).unwrap().count(), 1);
}

#[test]
fn shell_guard_refuses_disarming_the_hook() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, xdg) = seat_env(root.path());
    clear_ids();
    std::env::set_var("LJOS_SEAT", "probe");
    ljos_cli::cloud::up(false).unwrap();
    let (_origin, repo) = bare_repo(root.path(), "seat");
    let hook = xdg.join("ljos/hooks/pre-push");
    let hooks = xdg.join("ljos/hooks");

    let no_verify = refused("git push --no-verify origin develop", &repo);
    assert!(
        no_verify.reason.contains("pre-push hook"),
        "{}",
        no_verify.reason
    );
    assert!(
        no_verify
            .reason
            .contains("may push only its named branches"),
        "{}",
        no_verify.reason
    );
    assert!(
        !no_verify.reason.contains("never push to cursor"),
        "{}",
        no_verify.reason
    );
    for line in [
        "git push origin main",
        "git push origin :develop",
        "git push origin v9",
    ] {
        let hit = refused(line, &repo);
        assert!(
            hit.reason.contains("cloud seat rule"),
            "{line}: {}",
            hit.reason
        );
        assert!(
            !hit.reason.contains("never push to cursor"),
            "{line}: {}",
            hit.reason
        );
    }
    assert!(
        no_verify
            .reason
            .contains("a cloud seat may push only its named branches and develop"),
        "{}",
        no_verify.reason
    );
    for line in [
        "git -c core.hooksPath=/dev/null push origin develop",
        "git -c core.hooksPath=/tmp/keep push origin develop",
        "git --config-env core.hooksPath=HOOK push origin develop",
        "git clone --config core.hooksPath=/dev/null https://example.invalid/a.git",
        "git config --global --unset core.hooksPath",
        "git config --global core.hooksPath /tmp/x",
        "git config --global --replace-all core.hooksPath /dev/null",
        "git -c core.hooksPath=/tmp/x status",
        "cp /tmp/x ~/.gitconfig",
        "printf x > ~/.gitconfig",
        "git config --global include.path /tmp/evil-hooks.cfg",
        "git config --global --remove-section core",
        "export GIT_CONFIG_GLOBAL=/tmp/other-gitconfig",
        "export GIT_CONFIG_SYSTEM=/tmp/other-system",
        "export GIT_CONFIG_KEY_0=core.hooksPath GIT_CONFIG_VALUE_0=/dev/null",
        "GIT_CONFIG_PARAMETERS=\"'core.hooksPath'='/dev/null'\" git push origin develop",
        &format!("echo x >> {}", hook.display()),
        &format!("echo x >> {}", home.join(".gitconfig").display()),
        "echo x >> ~/.gitconfig",
        "echo x >> \"$HOME/.gitconfig\"",
        &format!("rm -f {}", hook.display()),
        &format!("chmod 644 {}", hook.display()),
        &format!("cp /etc/hostname {}", hook.display()),
        &format!("tee {} >/dev/null", hooks.join("pre-push").display()),
        "sed -i 's/a/b/' ~/.gitconfig",
        "perl -pi -e 's/a/b/' ~/.gitconfig",
        "printf '[core]\\nhooksPath = /dev/null\\n' >| ~/.gitconfig",
        "printf '[core]\\nhooksPath = /dev/null\\n' >> .git/config",
        "dd of=.git/config",
    ] {
        let hit = refused(line, &repo);
        assert!(
            hit.reason.contains("pre-push hook")
                || hit.reason.contains("core.hooksPath")
                || hit.reason.contains("GIT_CONFIG"),
            "{line}: {}",
            hit.reason
        );
    }

    allowed("git config core.hooksPath", &repo);
    allowed("git config --get core.hooksPath", &repo);
    allowed("git config core.hooksPath /dev/null", &repo);
    allowed("git config --replace-all core.hooksPath /dev/null", &repo);
    allowed("git -C /tmp config core.hooksPath /dev/null", &repo);
    allowed("git config --list", &repo);
    allowed("git config user.email seat@example.invalid", &repo);
    allowed("git config user.name seat", &repo);
    allowed("git config --global user.name seat", &repo);
    allowed("cat ~/.gitconfig", &repo);
    allowed("ls ~/.local/share/ljos/git-hooks", &repo);
    allowed("ls ~/.local/share/ljos/git-hooks/pre-push", &repo);
    allowed("echo kept >> /tmp/not-the-hook", &repo);
    allowed("git fetch origin", &repo);
    allowed("git pull --ff-only", &repo);
    allowed("cargo test --locked", &repo);
    allowed("make all", &repo);
    allowed("npm run build", &repo);

    let counted = refused(
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=remote.origin.push GIT_CONFIG_VALUE_0=HEAD:cursor/from-env git push origin HEAD",
        &repo,
    );
    assert!(
        counted.reason.contains("cursor/from-env"),
        "the refspec was judged as a hooksPath disarm: {}",
        counted.reason
    );
    assert!(
        !counted.reason.contains("core.hooksPath"),
        "{}",
        counted.reason
    );
}

/// Writes inside the git hooks directory run naming `writes-seat-file`,
/// as a write to a runner hook file does. Reads of that directory name
/// nothing. The directory follows `XDG_DATA_HOME`, `LJOS_GIT_HOOKS`, and
/// a global `core.hooksPath` that names `ljos/git-hooks`.
#[test]
fn a_write_to_the_git_hooks_directory_runs_named_like_a_runner_hook() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, _xdg) = seat_env(root.path());
    clear_ids();
    let hooks = home.join(".local/share/ljos/git-hooks");
    let pre = hooks.join("pre-push");
    let inside = hooks.join("x");

    // What the seat guard and the static guard each say as `line` runs.
    let said = |line: &str| {
        let _ = ljos_cli::take_guard_note();
        let seat = ljos_cli::seat_guard(line);
        let seat_said = ljos_cli::take_guard_note().unwrap_or_default();
        let via = ljos_cli::static_guard(line);
        let static_said = ljos_cli::take_guard_note().unwrap_or_default();
        assert!(
            seat.is_none() && via.is_none(),
            "refused: {line}: {seat:?} {via:?}"
        );
        [seat_said, static_said]
    };
    let named = |line: &str| {
        for note in said(line) {
            assert!(
                note.starts_with("allow\twarn\twrites-seat-file\t"),
                "{line}: {note}"
            );
        }
    };
    let allow = |line: &str| {
        assert_eq!(said(line), ["", ""], "{line}");
    };

    for line in [
        "touch ~/.cursor/hooks.json",
        "echo x > ~/.cursor/hooks.json",
        "touch ~/.claude/settings.json",
        "echo x > ~/.claude/settings.json",
        "ln -sf /tmp/x ~/.local/share/ljos/git-hooks/pre-push",
        "mkdir -p ~/.local/share/ljos/git-hooks/x",
        "touch ~/.local/share/ljos/git-hooks/pre-push",
        "rm -f ~/.local/share/ljos/git-hooks/pre-push",
        "cp /tmp/x ~/.local/share/ljos/git-hooks/pre-push",
        "mv /tmp/x ~/.local/share/ljos/git-hooks/pre-push",
        "printf x > ~/.local/share/ljos/git-hooks/pre-push",
        &format!("touch {}", pre.display()),
        &format!("mkdir -p {}", inside.display()),
        &format!("ln -sf /tmp/x {}", pre.display()),
        &format!("rm -rf {}", hooks.display()),
        "dd of=~/.local/share/ljos/git-hooks/pre-push",
        "cd ~/.local/share/ljos && touch git-hooks/pre-push",
        "cd ~/.local/share/ljos/git-hooks && touch pre-push",
    ] {
        named(line);
    }
    for line in [
        "cat ~/.cursor/hooks.json",
        "cat ~/.claude/settings.json",
        "ls ~/.local/share/ljos/git-hooks",
        "ls ~/.local/share/ljos/git-hooks/pre-push",
        "cat ~/.local/share/ljos/git-hooks/pre-push",
        "cat ~/.gitconfig",
        "git config --get core.hooksPath",
        "git config user.name seat",
        "mkdir -p ~/.local/share/ljos/other",
        "touch /tmp/not-a-hook",
        &format!("ls {}", hooks.display()),
        &format!("cat {}", pre.display()),
        &format!(
            "mkdir -p {}",
            home.join(".local/share/ljos/other").display()
        ),
    ] {
        allow(line);
    }

    let data = root.path().join("data");
    std::env::set_var("XDG_DATA_HOME", &data);
    named("touch $XDG_DATA_HOME/ljos/git-hooks/pre-push");
    named("mkdir -p ${XDG_DATA_HOME}/ljos/git-hooks/x");
    named(&format!(
        "touch {}",
        data.join("ljos/git-hooks/pre-push").display()
    ));
    allow("touch $XDG_DATA_HOME/ljos/other");
    allow("touch ~/.local/share/ljos/other");
    named("touch ~/.local/share/ljos/git-hooks/pre-push");
    std::env::remove_var("XDG_DATA_HOME");

    let custom = root.path().join("custom-hooks");
    std::env::set_var("LJOS_GIT_HOOKS", &custom);
    named("touch $LJOS_GIT_HOOKS/pre-push");
    named(&format!("mkdir -p {}", custom.join("x").display()));
    allow("touch /tmp/other-hooks/pre-push");
    std::env::remove_var("LJOS_GIT_HOOKS");

    let cfg = root.path().join("gitconfig");
    fs::write(&cfg, "[core]\n\thooksPath = /tmp/not-the-ljos-hooks\n").unwrap();
    allow("touch /tmp/not-the-ljos-hooks/pre-push");
    allow("mkdir -p /tmp/not-the-ljos-hooks/x");
    let booked = root.path().join("booked/ljos/git-hooks");
    fs::write(
        &cfg,
        format!("[core]\n\thooksPath = {}\n", booked.display()),
    )
    .unwrap();
    named(&format!("touch {}", booked.join("pre-push").display()));
    named(&format!("mkdir -p {}", booked.join("x").display()));
    allow("touch /tmp/not-the-ljos-hooks/pre-push");
    fs::write(&cfg, "").unwrap();

    std::env::set_var("LJOS_SEAT", "probe");
    ljos_cli::cloud::up(false).unwrap();
    let cloud_deny = |line: &str| {
        let hit = ljos_cli::static_guard(line).unwrap_or_else(|| panic!("allowed: {line}"));
        assert_eq!(hit.verdict, "deny", "{line}: {hit:?}");
        assert_eq!(hit.pattern, "cloud-cursor-push", "{line}: {hit:?}");
        assert!(
            hit.reason.contains("pre-push hook")
                || hit.reason.contains("core.hooksPath")
                || hit.reason.contains("GIT_CONFIG"),
            "{line}: {}",
            hit.reason
        );
    };
    for line in [
        "printf x > ~/.gitconfig",
        "cp /tmp/x ~/.gitconfig",
        "git config --global core.hooksPath /tmp/x",
        "git config --global --unset core.hooksPath",
        "git -c core.hooksPath=/tmp/x status",
        "git -c core.hooksPath=/dev/null status",
    ] {
        cloud_deny(line);
    }
    for line in [
        "cat ~/.gitconfig",
        "git config --get core.hooksPath",
        "git config user.name seat",
        "ls ~/.local/share/ljos/git-hooks",
    ] {
        allow(line);
    }
}

#[test]
fn doctor_reports_the_push_allowlist() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let missing = ljos_cli::cloud::guard_row();
    assert!(!missing.ok, "{missing:?}");
    assert_eq!(ljos_cli::doctor_word(&missing), "info", "{missing:?}");
    assert!(ljos_cli::healthy(std::slice::from_ref(&missing)));

    ljos_cli::cloud::up(false).unwrap();
    let row = ljos_cli::cloud::guard_row();
    assert!(row.ok, "{row:?}");
    assert!(row.state.contains("named branches and develop"), "{row:?}");
    assert!(!row.state.contains("hook"), "{row:?}");
    assert_eq!(ljos_cli::doctor_word(&row), "ok");
    assert!(ljos_cli::cloud::cloud_seat());

    sh_ok(
        root.path(),
        "git config --global core.hooksPath /tmp/not-the-ljos-hooks",
    );
    assert!(ljos_cli::cloud::guard_row().ok);
}

#[test]
fn a_write_to_the_seat_git_config_is_refused() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (_origin, repo) = bare_repo(root.path(), "seat");
    let (_other_origin, other) = bare_repo(root.path(), "other");
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();

    refused("git config core.hooksPath /dev/null", &repo);
    refused("git config include.path /tmp/evil-hooks.cfg", &repo);
    refused("git config --local --remove-section core", &repo);
    let other_cfg = format!("git -C {} config core.hooksPath /dev/null", other.display());
    allowed(&other_cfg, &repo);
}

#[test]
fn send_pack_checks_the_destination_repository() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    ljos_cli::cloud::up(false).unwrap();
    let here = root.path();
    refused("git send-pack /tmp/not-the-seat.git develop", here);
    refused("git send-pack other develop", here);
    allowed("git send-pack --dry-run origin develop", here);
    allowed("git push other develop", here);
}

fn hook_push(repo: &Path, local: &str, remote: &str) -> std::process::Output {
    let refs = format!("refs/heads/develop {local} refs/heads/develop {remote}\n");
    let mut child = guard_mode::command(env!("CARGO_BIN_EXE_ljos"))
        .current_dir(repo)
        .args(["git-hook", "pre-push", "origin", "origin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(refs.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// A commit-msg hook that appends a by-line is refused on push. The same
/// tree rebuilt with commit-tree, and a Tests paragraph, are allowed.
/// A by-line with no blank line before it is refused too.
#[test]
fn a_commit_message_hook_is_refused_on_push() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (origin, repo) = bare_repo(root.path(), "seat");
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    executable(
        &hooks.join("commit-msg"),
        "#!/bin/sh\nprintf '\\nCo-authored-by: Ada Lovelace <ada@example.com>\\n' >> \"$1\"\n",
    );
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "note"]);
    let body = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(
        body.lines().any(|line| line.starts_with("Co-authored-by:")),
        "{body}"
    );
    let bad = git(&repo, &["rev-parse", "HEAD"]);
    let parent = git(&repo, &["rev-parse", "HEAD^"]);

    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    ljos_cli::cloud::write_push_allowlist(&[origin.to_str().unwrap()], &[], &["develop"]).unwrap();

    let hooked = hook_push(&repo, bad.trim(), parent.trim());
    let hooked_text = text_of(&hooked);
    assert!(!hooked.status.success(), "{hooked_text}");
    assert!(hooked_text.contains("message is refused"), "{hooked_text}");

    sh_ok(
        &repo,
        "parent=$(git rev-parse HEAD^)\n\
         tree=$(git rev-parse 'HEAD^{tree}')\n\
         clean=$(git commit-tree \"$tree\" -p \"$parent\" -m note)\n\
         git reset -q --hard \"$clean\"\n",
    );
    let clean = git(&repo, &["rev-parse", "HEAD"]);
    sh_ok(&repo, "git push -q origin develop");
    assert!(has_ref(&origin, "refs/heads/develop"));
    let allowed = hook_push(&repo, clean.trim(), parent.trim());
    let allowed_text = text_of(&allowed);
    assert!(
        !allowed_text.contains("message is refused"),
        "{allowed_text}"
    );

    sh_ok(
        &repo,
        "parent=$(git rev-parse HEAD)\n\
         tree=$(git rev-parse 'HEAD^{tree}')\n\
         printf 'note\\nCo-authored-by: Ada Lovelace <ada@example.com>\\n' > msg\n\
         bad=$(git commit-tree \"$tree\" -p \"$parent\" -F msg)\n\
         git reset -q --hard \"$bad\"\n",
    );
    let glued = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(
        glued.contains("Co-authored-by:"),
        "the by line was not stored:\n{glued}"
    );
    assert!(
        !glued.contains("\n\nCo-authored-by:"),
        "a blank line slipped in:\n{glued}"
    );
    let glued_sha = git(&repo, &["rev-parse", "HEAD"]);
    let tip = git(&repo, &["rev-parse", "HEAD^"]);
    let glued_hook = hook_push(&repo, glued_sha.trim(), tip.trim());
    let glued_text = text_of(&glued_hook);
    assert!(
        glued_text.contains("message is refused"),
        "a by line with no blank line was allowed: {glued_text}"
    );

    sh_ok(
        &repo,
        "parent=$(git rev-parse HEAD^)\n\
         tree=$(git rev-parse 'HEAD^{tree}')\n\
         printf 'note\\n\\nTests: cargo test\\n' > msg\n\
         ok=$(git commit-tree \"$tree\" -p \"$parent\" -F msg)\n\
         git reset -q --hard \"$ok\"\n",
    );
    sh_ok(&repo, "git push -q origin develop");
    assert!(has_ref(&origin, "refs/heads/develop"));
}

/// A new branch on top of a refused commit that is already on a remote
/// is allowed. The check reads only commits the remotes do not have.
#[test]
fn a_new_branch_omits_a_commit_already_on_a_remote() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (origin, repo) = bare_repo(root.path(), "seat");
    sh_ok(
        &repo,
        "tree=$(git rev-parse 'HEAD^{tree}')\n\
         parent=$(git rev-parse HEAD)\n\
         printf 'note\\nCo-authored-by: Ada <ada@example.com>\\n' > msg\n\
         bad=$(git commit-tree \"$tree\" -p \"$parent\" -F msg)\n\
         git reset -q --hard \"$bad\"\n\
         git push -q -u origin develop\n",
    );
    assert!(has_ref(&origin, "refs/heads/develop"));
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    ljos_cli::cloud::write_push_allowlist(&[origin.to_str().unwrap()], &[], &["develop", "topic"])
        .unwrap();
    sh_ok(
        &repo,
        "git checkout -q -b topic\n\
         tree=$(git rev-parse 'HEAD^{tree}')\n\
         parent=$(git rev-parse HEAD)\n\
         clean=$(git commit-tree \"$tree\" -p \"$parent\" -m note)\n\
         git reset -q --hard \"$clean\"\n\
         git push -q -u origin topic\n",
    );
    assert!(has_ref(&origin, "refs/heads/topic"));
    let clean = git(&repo, &["rev-parse", "HEAD"]);
    let zero = "0000000000000000000000000000000000000000";
    let out = hook_push(&repo, clean.trim(), zero);
    let text = text_of(&out);
    assert!(
        !text.contains("message is refused"),
        "a new branch reread an old commit: {text}"
    );
}

/// A pre-push stdin line that is not a ref update is refused.
#[test]
fn a_pre_push_line_that_is_not_a_ref_is_refused() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (_origin, repo) = bare_repo(root.path(), "seat");
    let mut child = guard_mode::command(env!("CARGO_BIN_EXE_ljos"))
        .current_dir(&repo)
        .args(["git-hook", "pre-push"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"not a ref update\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = text_of(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("cannot read"), "{text}");
}

#[test]
fn advisory_a_commit_message_hook_is_refused_on_push() {
    let _mode = guard_mode::advisory();

    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (origin, repo) = bare_repo(root.path(), "seat");
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    executable(
        &hooks.join("commit-msg"),
        "#!/bin/sh\nprintf '\\nCo-authored-by: Ada Lovelace <ada@example.com>\\n' >> \"$1\"\n",
    );
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "note"]);
    let body = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(
        body.lines().any(|line| line.starts_with("Co-authored-by:")),
        "{body}"
    );
    let bad = git(&repo, &["rev-parse", "HEAD"]);
    let parent = git(&repo, &["rev-parse", "HEAD^"]);

    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    ljos_cli::cloud::write_push_allowlist(&[origin.to_str().unwrap()], &[], &["develop"]).unwrap();

    let hooked = hook_push(&repo, bad.trim(), parent.trim());
    let hooked_text = text_of(&hooked);
    assert!(hooked.status.success(), "{hooked_text}");
    assert!(hooked_text.contains("machine-trailer"), "{hooked_text}");

    sh_ok(
        &repo,
        "parent=$(git rev-parse HEAD^)\n\
         tree=$(git rev-parse 'HEAD^{tree}')\n\
         clean=$(git commit-tree \"$tree\" -p \"$parent\" -m note)\n\
         git reset -q --hard \"$clean\"\n",
    );
    let clean = git(&repo, &["rev-parse", "HEAD"]);
    sh_ok(&repo, "git push -q origin develop");
    assert!(has_ref(&origin, "refs/heads/develop"));
    let allowed = hook_push(&repo, clean.trim(), parent.trim());
    let allowed_text = text_of(&allowed);
    assert!(
        !allowed_text.contains("message is refused"),
        "{allowed_text}"
    );

    sh_ok(
        &repo,
        "parent=$(git rev-parse HEAD)\n\
         tree=$(git rev-parse 'HEAD^{tree}')\n\
         printf 'note\\nCo-authored-by: Ada Lovelace <ada@example.com>\\n' > msg\n\
         bad=$(git commit-tree \"$tree\" -p \"$parent\" -F msg)\n\
         git reset -q --hard \"$bad\"\n",
    );
    let glued = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(
        glued.contains("Co-authored-by:"),
        "the by line was not stored:\n{glued}"
    );
    assert!(
        !glued.contains("\n\nCo-authored-by:"),
        "a blank line slipped in:\n{glued}"
    );
    let glued_sha = git(&repo, &["rev-parse", "HEAD"]);
    let tip = git(&repo, &["rev-parse", "HEAD^"]);
    let glued_hook = hook_push(&repo, glued_sha.trim(), tip.trim());
    let glued_text = text_of(&glued_hook);
    assert!(
        glued_hook.status.success() && glued_text.contains("machine-trailer"),
        "a by line with no blank line was not labelled: {glued_text}"
    );

    sh_ok(
        &repo,
        "parent=$(git rev-parse HEAD^)\n\
         tree=$(git rev-parse 'HEAD^{tree}')\n\
         printf 'note\\n\\nTests: cargo test\\n' > msg\n\
         ok=$(git commit-tree \"$tree\" -p \"$parent\" -F msg)\n\
         git reset -q --hard \"$ok\"\n",
    );
    sh_ok(&repo, "git push -q origin develop");
    assert!(has_ref(&origin, "refs/heads/develop"));
}
