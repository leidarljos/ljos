//! The pre-push hook is the destination check. A line filter cannot see
//! a push that a script, make, npm, or python starts, and it cannot see
//! a remote URL git rewrote. These tests push to a local bare origin and
//! require the hook's own refusal.

ljos_test_boot::harness!();
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
    Command::new("sh")
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

fn push_fails(dir: &Path, script: &str) {
    let out = sh_at(dir, script);
    let text = text_of(&out);
    assert!(!out.status.success(), "push was accepted: {script}\n{text}");
    assert!(
        text.contains("pre-push:"),
        "the refusal is not the pre-push hook: {script}\n{text}"
    );
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
        let out = Command::new("sha256sum")
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

/// A repo on `develop` with no local `core.hooksPath`, so the global hook
/// `cloud up` installed is the one git runs.
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

/// `isolate_git` leaves only `/usr/bin:/bin` on PATH, and a CI job does
/// not install node. The hook still has to refuse a push an npm recipe
/// starts, so the directory of a real `npm` is put back when one is
/// installed. Otherwise a script runs the `package.json` recipe the way
/// `npm run` does.
fn npm_prefix(root: &Path) -> PathBuf {
    let found = Command::new("sh")
        .arg("-c")
        .arg("command -v npm")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|path| !path.is_empty());
    if let Some(path) = found {
        return Path::new(&path)
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
    }
    let bin = root.join("npm-bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("npm"),
        r#"#!/bin/sh
set -eu
[ "${1:-}" = run ] || { echo "npm: only run" >&2; exit 1; }
name=${2:?}
script=$(python3 -c 'import json,sys; print(json.load(open("package.json"))["scripts"][sys.argv[1]])' "$name")
exec sh -c "$script"
"#,
    );
    bin
}

fn has_ref(origin: &Path, name: &str) -> bool {
    let out = Command::new("git")
        .arg("-C")
        .arg(origin)
        .args(["show-ref"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line.ends_with(name))
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

#[test]
fn hook_refuses_every_push_the_line_filter_cannot_see() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let npm = npm_prefix(root.path());
    let (_home, xdg) = seat_env(root.path());
    clear_ids();
    std::env::set_var("LJOS_SEAT", "probe");
    ljos_cli::cloud::up(false).unwrap();
    let seat = xdg.join("ljos/seats/probe");
    fs::create_dir_all(&seat).unwrap();
    fs::write(
        seat.join("ljos.toml"),
        "[cloud]\nbranches = [\n  \"feature-x\"\n]\n",
    )
    .unwrap();

    let (origin, repo) = bare_repo(root.path(), "seat");
    let (other_origin, other) = bare_repo(root.path(), "other");
    let (tracker, tracker_repo) = bare_repo(root.path(), "tracker");
    ljos_cli::cloud::write_push_allowlist(
        &[origin.to_str().unwrap()],
        &[tracker.to_str().unwrap()],
        &["develop", "feature-x"],
    )
    .unwrap();

    sh_ok(&repo, "git push -u origin develop");
    assert!(has_ref(&origin, "refs/heads/develop"));
    sh_ok(
        &repo,
        "git branch feature-x && git push -u origin feature-x",
    );
    assert!(has_ref(&origin, "refs/heads/feature-x"));
    sh_ok(
        &repo,
        "git fetch origin && git pull --ff-only origin develop",
    );
    sh_ok(
        &repo,
        "git commit -q --allow-empty -m next && git push origin develop",
    );
    assert!(has_ref(&origin, "refs/heads/develop"));

    sh_ok(
        &tracker_repo,
        "git branch -M main && git push -u origin main",
    );
    assert!(has_ref(&tracker, "refs/heads/main"));
    sh_ok(
        &tracker_repo,
        "git commit -q --allow-empty -m tracked && git push origin main",
    );

    git(&repo, &["branch", "main"]);
    git(&repo, &["branch", "other"]);
    let npm_push = format!("PATH=\"$PATH:{}\" npm run push", npm.display());
    let npm_build = format!("PATH=\"$PATH:{}\" npm run build", npm.display());
    executable(
        &repo.join("push.sh"),
        "#!/bin/sh\nexec git push origin main\n",
    );
    fs::write(repo.join("Makefile"), "push:\n\tgit push origin main\n").unwrap();
    fs::write(
        repo.join("package.json"),
        "{\"name\":\"t\",\"version\":\"0.0.0\",\"scripts\":{\"push\":\"git push origin main\",\"build\":\"echo built\"}}\n",
    )
    .unwrap();
    fs::write(repo.join("Build"), "all:\n\t@echo built\n").unwrap();

    for script in [
        "./push.sh",
        "f(){ \"$@\";}; f git push origin other",
        "$(echo git) push origin other",
        "make -f Makefile push",
        npm_push.as_str(),
        "python3 -c 'import os,sys; sys.exit(1 if os.system(\"git push origin main\") else 0)'",
        "export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=remote.origin.push GIT_CONFIG_VALUE_0=HEAD:refs/heads/main; git push",
        "git push origin v1",
        "git push origin :feature-x",
        "git push --force origin develop",
    ] {
        if script.starts_with("git push origin v1") {
            sh_ok(&repo, "git tag v1");
        }
        if script.starts_with("git push --force") {
            sh_ok(
                &repo,
                "git reset -q --hard HEAD~1 && git commit -q --allow-empty -m diverge",
            );
        }
        push_fails(&repo, script);
    }
    assert!(!has_ref(&origin, "refs/heads/main"), "main was created");
    assert!(!has_ref(&origin, "refs/heads/other"), "other was created");
    assert!(
        has_ref(&origin, "refs/heads/feature-x"),
        "feature-x was deleted"
    );
    assert!(!has_ref(&origin, "refs/tags/v1"), "the tag was pushed");

    push_fails(&other, "git push -u origin develop");
    assert!(!has_ref(&other_origin, "refs/heads/develop"));

    let rewritten = format!(
        "git config url.{}.pushInsteadOf {} && git push origin develop",
        other_origin.display(),
        origin.display()
    );
    push_fails(&repo, &rewritten);
    assert!(!has_ref(&other_origin, "refs/heads/develop"));

    push_fails(&tracker_repo, "git tag v2 && git push origin v2");
    push_fails(&tracker_repo, "git push origin :main");
    assert!(has_ref(&tracker, "refs/heads/main"));
    sh_ok(
        &tracker_repo,
        "git commit -q --allow-empty -m again && git push origin HEAD:refs/heads/side",
    );
    assert!(has_ref(&tracker, "refs/heads/side"));

    sh_ok(&repo, "make -f Build");
    sh_ok(&repo, &npm_build);
    allowed("cargo build --locked", &repo);
    allowed("make", &repo);
    allowed("npm test", &repo);
    allowed("git fetch origin", &repo);
    allowed("git pull --ff-only origin develop", &repo);
    allowed("git push origin develop", &repo);
    allowed("git push origin feature-x", &repo);
    allowed(
        "python3 -c 'import urllib.request; urllib.request.urlopen(\"https://example.invalid\")'",
        &repo,
    );
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
    assert!(
        hook.is_file(),
        "cloud up did not install {}",
        hook.display()
    );

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

/// Writes inside the git hooks directory are the same refusal as a write
/// to a runner hook file. Reads of that directory stay allowed. The
/// directory follows `XDG_DATA_HOME`, `LJOS_GIT_HOOKS`, and a global
/// `core.hooksPath` that names `ljos/git-hooks`.
#[test]
fn the_git_hooks_directory_is_refused_like_a_runner_hook() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, _xdg) = seat_env(root.path());
    clear_ids();
    let hooks = home.join(".local/share/ljos/git-hooks");
    let pre = hooks.join("pre-push");
    let inside = hooks.join("x");

    let deny = |line: &str| {
        let hit = ljos_cli::seat_guard(line).unwrap_or_else(|| panic!("allowed: {line}"));
        assert_eq!(hit.verdict, "deny", "{line}: {hit:?}");
        assert_eq!(hit.pattern, "seat-guard", "{line}: {hit:?}");
        let via = ljos_cli::static_guard(line).unwrap_or_else(|| panic!("static allowed: {line}"));
        assert_eq!(via.pattern, "seat-guard", "static {line}: {via:?}");
    };
    let allow = |line: &str| {
        assert!(
            ljos_cli::seat_guard(line).is_none(),
            "refused: {line}: {:?}",
            ljos_cli::seat_guard(line)
        );
        assert!(
            ljos_cli::static_guard(line).is_none(),
            "static refused: {line}: {:?}",
            ljos_cli::static_guard(line)
        );
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
        deny(line);
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
    deny("touch $XDG_DATA_HOME/ljos/git-hooks/pre-push");
    deny("mkdir -p ${XDG_DATA_HOME}/ljos/git-hooks/x");
    deny(&format!(
        "touch {}",
        data.join("ljos/git-hooks/pre-push").display()
    ));
    allow("touch $XDG_DATA_HOME/ljos/other");
    allow("touch ~/.local/share/ljos/other");
    deny("touch ~/.local/share/ljos/git-hooks/pre-push");
    std::env::remove_var("XDG_DATA_HOME");

    let custom = root.path().join("custom-hooks");
    std::env::set_var("LJOS_GIT_HOOKS", &custom);
    deny("touch $LJOS_GIT_HOOKS/pre-push");
    deny(&format!("mkdir -p {}", custom.join("x").display()));
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
    deny(&format!("touch {}", booked.join("pre-push").display()));
    deny(&format!("mkdir -p {}", booked.join("x").display()));
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
fn doctor_reports_a_missing_or_changed_pre_push_hook() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, xdg) = seat_env(root.path());
    clear_ids();
    let missing = ljos_cli::cloud::guard_row();
    assert!(!missing.ok, "{missing:?}");
    assert_eq!(ljos_cli::doctor_word(&missing), "info", "{missing:?}");
    assert!(ljos_cli::healthy(std::slice::from_ref(&missing)));

    ljos_cli::cloud::up(false).unwrap();
    let row = ljos_cli::cloud::guard_row();
    assert!(row.ok, "{row:?}");
    assert!(row.state.contains("named branches and develop"), "{row:?}");
    assert!(row.state.contains("pre-push hook"), "{row:?}");
    assert_eq!(ljos_cli::doctor_word(&row), "ok");
    assert!(ljos_cli::cloud::cloud_seat());

    let hook = xdg.join("ljos/hooks/pre-push");
    fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    let changed = ljos_cli::cloud::guard_row();
    assert!(!changed.ok, "{changed:?}");
    assert!(
        changed.state.contains("named branches and develop"),
        "{changed:?}"
    );
    assert!(changed.state.contains("pre-push hook"), "{changed:?}");
    assert!(changed.state.contains("changed"), "{changed:?}");
    assert_eq!(ljos_cli::doctor_word(&changed), "warn", "{changed:?}");
    assert!(ljos_cli::healthy(std::slice::from_ref(&changed)));
    assert!(ljos_cli::cloud::cloud_seat());

    fs::remove_file(&hook).unwrap();
    let gone = ljos_cli::cloud::guard_row();
    assert!(!gone.ok, "{gone:?}");
    assert!(gone.state.contains("missing"), "{gone:?}");
    assert_eq!(ljos_cli::doctor_word(&gone), "warn");

    ljos_cli::cloud::up(false).unwrap();
    assert!(ljos_cli::cloud::guard_row().ok);

    sh_ok(
        root.path(),
        "git config --global core.hooksPath /tmp/not-the-ljos-hooks",
    );
    let moved = ljos_cli::cloud::guard_row();
    assert!(!moved.ok, "{moved:?}");
    assert!(moved.state.contains("core.hooksPath"), "{moved:?}");
    assert_eq!(ljos_cli::doctor_word(&moved), "warn");
}

#[test]
fn a_closed_seat_still_lets_the_tracker_fast_forward() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, xdg) = seat_env(root.path());
    clear_ids();
    std::env::set_var("LJOS_SEAT", "probe");
    let (origin, repo) = bare_repo(root.path(), "seat");
    let (tracker, tracker_repo) = bare_repo(root.path(), "tracker");
    // The bare tracker has to have a commit before `cloud up` clones it.
    sh_ok(&tracker_repo, "git push -q -u origin develop");
    fs::create_dir_all(xdg.join("ljos/seats/probe")).unwrap();
    fs::write(xdg.join("ljos/seats/probe/ljos.toml"), "[\n").unwrap();
    std::env::set_var("LJOS_TRACKER_REMOTE", tracker.as_os_str());
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    let allow = fs::read_to_string(xdg.join("ljos/push-allow")).unwrap();
    assert!(
        allow.lines().any(|line| line == "closed\t1"),
        "allowlist did not fail closed:\n{allow}"
    );

    let out = sh_at(&repo, "git push -u origin develop");
    let said = text_of(&out);
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("pre-push:"), "{said}");
    assert!(said.contains("seat config could not be read"), "{said}");
    assert!(!has_ref(&origin, "refs/heads/develop"));

    sh_ok(
        &tracker_repo,
        "git branch -M main && git push -u origin main",
    );
    assert!(has_ref(&tracker, "refs/heads/main"));
    push_fails(&tracker_repo, "git push origin :main");
}

#[test]
fn the_hook_off_switch_does_not_let_a_push_land() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (origin, repo) = bare_repo(root.path(), "seat");
    let (other_origin, _other) = bare_repo(root.path(), "other");
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    ljos_cli::cloud::write_push_allowlist(&[origin.to_str().unwrap()], &[], &["develop"]).unwrap();
    git(
        &repo,
        &["remote", "add", "other", other_origin.to_str().unwrap()],
    );
    sh_ok(&repo, "LJOS_HOOK_KEY=1 git push -q -u origin develop");
    assert!(has_ref(&origin, "refs/heads/develop"));
    push_fails(&repo, "LJOS_HOOK_KEY=1 git push -q other develop");
    assert!(!has_ref(&other_origin, "refs/heads/develop"));
}

#[test]
fn the_hook_ignores_home_xdg_and_path() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (origin, repo) = bare_repo(root.path(), "seat");
    let (other_origin, _other) = bare_repo(root.path(), "other");
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    ljos_cli::cloud::write_push_allowlist(&[origin.to_str().unwrap()], &[], &["develop"]).unwrap();
    sh_ok(&repo, "git push -q -u origin develop");
    git(
        &repo,
        &["remote", "add", "other", other_origin.to_str().unwrap()],
    );

    let forged = root.path().join("forged");
    fs::create_dir_all(forged.join("ljos")).unwrap();
    fs::write(
        forged.join("ljos/push-allow"),
        format!("code\t{}\nbranch\tdevelop\n", other_origin.display()),
    )
    .unwrap();
    let fake = root.path().join("fakebin");
    fs::create_dir_all(&fake).unwrap();
    executable(&fake.join("readlink"), "#!/bin/sh\necho /no/such\n");
    executable(&fake.join("tr"), "#!/bin/sh\ncat\n");
    let empty = root.path().join("emptyhome");
    fs::create_dir_all(&empty).unwrap();
    for script in [
        &format!(
            "XDG_CONFIG_HOME={} git push -q other develop",
            forged.display()
        ),
        &format!("PATH={}:$PATH git push -q other develop", fake.display()),
        &format!(
            "HOME={} git -c user.name=x -c user.email=x@example.invalid push -q other develop",
            empty.display()
        ),
    ] {
        push_fails(&repo, script);
    }
    assert!(!has_ref(&other_origin, "refs/heads/develop"));
}

#[test]
fn doctor_reads_the_hooks_path_git_would_use() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (_origin, repo) = bare_repo(root.path(), "seat");
    let (_other_origin, other) = bare_repo(root.path(), "other");
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    let row = ljos_cli::cloud::guard_row();
    assert!(row.ok, "{row:?}");

    refused("git config core.hooksPath /dev/null", &repo);
    refused("git config include.path /tmp/evil-hooks.cfg", &repo);
    refused("git config --local --remove-section core", &repo);
    let other_cfg = format!("git -C {} config core.hooksPath /dev/null", other.display());
    allowed(&other_cfg, &repo);

    let evil = root.path().join("evil.cfg");
    fs::write(&evil, "[core]\n\thooksPath = /dev/null\n").unwrap();
    sh_ok(
        &repo,
        &format!("git config --local include.path {}", evil.display()),
    );
    let moved = ljos_cli::cloud::guard_row();
    assert!(!moved.ok, "{moved:?}");
    assert!(moved.state.contains("core.hooksPath"), "{moved:?}");
}

#[test]
fn cloud_up_chains_a_pre_push_already_booked() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (_origin, repo) = bare_repo(root.path(), "seat");
    let booked = root.path().join("booked-hooks");
    fs::create_dir_all(&booked).unwrap();
    executable(
        &booked.join("pre-push"),
        "#!/bin/sh\necho booked-hook >&2\nexit 1\n",
    );
    git(
        &repo,
        &["config", "core.hooksPath", booked.to_str().unwrap()],
    );
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    let now = fs::read_to_string(booked.join("pre-push")).unwrap();
    assert!(now.contains("# ljos-cloud-chain "), "{now}");
    let prev = fs::read_to_string(booked.join("pre-push.ljos-previous")).unwrap();
    assert!(prev.contains("booked-hook"), "{prev}");
    let configured = git(&repo, &["config", "--local", "--get", "core.hooksPath"]);
    assert_eq!(configured.trim(), booked.to_str().unwrap());
    let row = ljos_cli::cloud::guard_row();
    assert!(row.ok, "{row:?}");
    let out = sh_at(&repo, "git push origin develop");
    let text = text_of(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("booked-hook"), "{text}");
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
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

    push_fails(&repo, "git push origin develop");
    assert!(!has_ref(&origin, "refs/heads/develop"));
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
    push_fails(&repo, "git push origin develop");
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

/// Join `before` and `after` with a tab from `printf '\t'`.
fn tab_message(before: &str, after: &str) -> String {
    let out = Command::new("sh")
        .arg("-c")
        .arg("printf '%s' \"$1\"; printf '\\t'; printf '%s' \"$2\"")
        .arg("tab")
        .arg(before)
        .arg(after)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text_of(&out));
    String::from_utf8(out.stdout).unwrap()
}

fn message_push(repo: &Path, parent: &str, message: &str) -> std::process::Output {
    let msg = repo.join("push-msg");
    fs::write(&msg, message).unwrap();
    let script = format!(
        "tree=$(git rev-parse '{parent}^{{tree}}')\n\
         made=$(git commit-tree \"$tree\" -p {parent} -F push-msg)\n\
         git reset -q --hard \"$made\"\n\
         git push origin develop\n"
    );
    sh_at(repo, &script)
}

/// The cloud pre-push script refuses the same lines the other checks do,
/// and still allows a Tests paragraph, a note paragraph, and prose.
#[test]
fn the_cloud_pre_push_script_uses_the_line_rule() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    let (origin, repo) = bare_repo(root.path(), "seat");
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    std::env::set_current_dir(root.path()).unwrap();
    ljos_cli::cloud::write_push_allowlist(&[origin.to_str().unwrap()], &[], &["develop"]).unwrap();
    sh_ok(&repo, "git push -q -u origin develop");
    let parent = git(&repo, &["rev-parse", "HEAD"]);
    let parent = parent.trim().to_string();

    for message in [
        "note\nCo-authored-by : x\n",
        "note\n  Co-authored-by: x\n",
        "note\nMade-with : x\n",
        "note\n Made-with: x\n",
        "note\nCURSOR_AGENT=1\n",
        "note\n  CURSOR_AGENT=1\n",
        "note\nCURSOR_AGENT: 1\n",
        "note\ncursoragent\n",
        "note\nCursor Agent wrote this\n",
        "note\nsee cursoragent in the note\n",
        "note\nsee a cursor agent in the note\n",
        &tab_message("note\n", "Co-authored-by: A\n"),
        &tab_message("note\nCo-authored-by", ": A\n"),
        &tab_message("note\n", "CURSOR_AGENT=1\n"),
        &tab_message("note\nCURSOR_AGENT", "=1\n"),
    ] {
        let out = message_push(&repo, &parent, message);
        let text = text_of(&out);
        assert!(!out.status.success(), "accepted:\n{message}{text}");
        assert!(text.contains("pre-push:"), "{message}{text}");
        assert!(text.contains("message is refused"), "{message}{text}");
        sh_ok(&repo, &format!("git reset -q --hard {parent}"));
    }
    for message in [
        "note\n\nTests: cargo test\n",
        "note\n\nNote: kept\n",
        "The Signed-off-by line is gone.\n",
        "see CURSOR_AGENT in the prose\n",
        "The figures are generated with matplotlib.\n",
        "the plot was made with cursor tools.\n",
        "note\n\nTests: ran cargo test\n",
        "Ask the author by mail.\n",
    ] {
        let out = message_push(&repo, &parent, message);
        let text = text_of(&out);
        assert!(out.status.success(), "refused:\n{message}{text}");
        sh_ok(&repo, &format!("git reset -q --hard {parent}"));
        sh_ok(
            &origin,
            &format!("git update-ref refs/heads/develop {parent}"),
        );
    }
    let _ = origin;
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
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
