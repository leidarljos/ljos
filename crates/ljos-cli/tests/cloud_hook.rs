//! The pre-push hook is the destination check. A line filter cannot see
//! a push that a script, make, npm, or python starts, and it cannot see
//! a remote URL git rewrote. These tests push to a local bare origin and
//! require the hook's own refusal.

use std::fs;
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
        no_verify.reason.contains("outside the seat allowlist"),
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
            hit.reason.contains("outside the seat allowlist"),
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
        "git config --global --replace-all core.hooksPath /dev/null",
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
    allowed("git config --global user.name seat", &repo);
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
    assert!(row.state.contains("denies cursor/* pushes"), "{row:?}");
    assert!(row.state.contains("pre-push hook"), "{row:?}");
    assert_eq!(ljos_cli::doctor_word(&row), "ok");
    assert!(ljos_cli::cloud::cloud_seat());

    let hook = xdg.join("ljos/hooks/pre-push");
    fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    let changed = ljos_cli::cloud::guard_row();
    assert!(!changed.ok, "{changed:?}");
    assert!(
        changed.state.contains("denies cursor/* pushes"),
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

/// gh, curl, wget, and httpie are an allowlist against GitHub. A resolved
/// GET passes. A write, and a flag whose meaning is not on the command,
/// does not. Another host is not GitHub.
#[test]
fn github_clients_allow_only_a_resolved_get() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, _xdg) = seat_env(root.path());
    clear_ids();
    ljos_cli::cloud::up(false).unwrap();
    let here = root.path();

    for line in [
        "curl -sXDELETE https://api.github.com/user",
        "curl -s -d'{a:1}' https://api.github.com/user",
        "curl --data-ascii dumped https://api.github.com/repos/acme/widgets",
        "curl --oauth2-bearer tok -X POST https://api.github.com/user",
        "curl -u x:$TOKEN -X DELETE https://api.github.com/repos/acme/widgets/git/refs/heads/develop",
        "curl -H @headers https://api.github.com/user",
        "curl --netrc https://api.github.com/user",
        "curl -n https://api.github.com/user",
        "curl -K curlrc https://api.github.com/user",
        "curl -sXDELETE \"$URL\"",
        "wget --method DELETE https://api.github.com/user",
        "wget --method=delete https://api.github.com/repos/acme/widgets",
        "http DELETE https://api.github.com/user",
        "https --form POST https://api.github.com/user name=a",
        "gh workflow run ci.yml --ref main",
        "gh pr edit 7 --base develop",
        "gh api --method PATCH repos/acme/widgets -f default_branch=develop",
        "gh api --method POST repos/acme/widgets/actions/workflows/ci.yml/dispatches -f ref=main",
        "gh api repos/acme/widgets/pulls/7/update-branch -X POST",
        "gh api repos/acme/widgets/git/refs -fref=refs/heads/x -fsha=0123456789012345678901234567890123456789",
        "gh api repos/acme/widgets/issues -ftitle=t",
        "gh api repos/acme/widgets/issues -if title=x",
        "gh api graphql -fquery='mutation { deleteRef(input: {refId: \"R\"}) { clientMutationId } }'",
        "curl -X DELETE HTTPS://api.github.com/repos/acme/widgets/git/refs/heads/main",
        "curl -X DELETE https://x-access-token:$GH_TOKEN@api.github.com/repos/acme/widgets/git/refs/heads/main",
        "curl -X DELETE https://api.github.com./repos/acme/widgets/git/refs/heads/main",
        "curl --proto-default https -X DELETE api.github.com/repos/acme/widgets/git/refs/heads/main",
        "http DELETE api.github.com/repos/acme/widgets/git/refs/heads/main \"Authorization:token $GH_TOKEN\"",
        "curl -X POST --data-binary @pack https://x:$GH_TOKEN@github.com/acme/widgets.git/git-receive-pack",
        "curl -X PUT https://ghcr.io/v2/acme/widgets/manifests/latest",
        "curl -G -T file https://api.github.com/repos/acme/widgets",
        "curl -G -F a=b https://api.github.com/user",
        "wget -e method=DELETE https://api.github.com/user",
        "wget --execute=body_data=abc https://api.github.com/user",
    ] {
        let hit = refused(line, here);
        assert_eq!(hit.pattern, "cloud-cursor-push", "{line}: {}", hit.reason);
    }

    for line in [
        "curl --oauth2-bearer tok https://api.github.com/user",
        "curl -u x:$TOKEN https://api.github.com/user",
        "curl -fsSL https://api.github.com/user",
        "curl -fsSLO https://api.github.com/user",
        "curl -LO https://api.github.com/user",
        "curl -JLO https://api.github.com/user",
        "curl -C - -O https://api.github.com/user",
        "curl -i https://api.github.com/user",
        "curl -4 https://api.github.com/user",
        "curl --write-out '%{http_code}' https://api.github.com/user",
        "curl --retry-all-errors https://api.github.com/user",
        "gh run download 1",
        "gh label list",
        "gh cache list",
        "gh api graphql -f query='query { viewer { login } }'",
        "curl -s -H \"Authorization: Bearer $GH_TOKEN\" https://api.github.com/user",
        "curl -sXDELETE https://example.com/build",
        "curl -s --netrc https://example.com/build",
        "wget -q https://api.github.com/user",
        "http GET https://api.github.com/user",
        "https https://api.github.com/user",
        "gh api user --jq .login",
        "gh api repos/acme/widgets --jq .permissions.push",
        "gh api -X GET repos/acme/widgets/pulls -f per_page=5",
        "gh api --method HEAD repos/acme/widgets",
        "gh pr list",
        "gh pr diff 7",
        "gh auth status",
        "gh help api",
        "gh repo view acme/widgets",
        "python3 -c 'import urllib.request'",
        "cargo build",
        "make",
        "npm test",
    ] {
        allowed(line, here);
    }
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
fn cloud_up_keeps_the_recorded_repository() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_home, xdg) = seat_env(root.path());
    clear_ids();
    let (_origin, repo) = bare_repo(root.path(), "seat");
    let (_other_origin, other) = bare_repo(root.path(), "other");
    std::env::set_current_dir(&repo).unwrap();
    ljos_cli::cloud::up(false).unwrap();
    let allow = fs::read_to_string(xdg.join("ljos/push-allow")).unwrap();
    std::env::set_current_dir(&other).unwrap();
    let err = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(err.contains("already recorded"), "{err}");
    std::env::set_current_dir(root.path()).unwrap();
    let after = fs::read_to_string(xdg.join("ljos/push-allow")).unwrap();
    assert_eq!(allow, after, "a second cloud up rewrote the allowlist");
    refused("ljos cloud up --repoint", root.path());
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
fn a_token_and_a_client_config_hide_the_method() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, _xdg) = seat_env(root.path());
    clear_ids();
    ljos_cli::cloud::up(false).unwrap();
    let here = root.path();
    fs::write(home.join(".curlrc"), "request = DELETE\n").unwrap();
    refused(
        "curl -s -H \"Authorization: Bearer $GH_TOKEN\" https://api.github.com/user",
        here,
    );
    // The file sets the method. A token on the command is not required.
    refused("curl -s https://api.github.com/user", here);
    refused("echo hidden > ~/.curlrc", here);
    refused("echo hidden > ~/.wgetrc", here);
    refused(
        "CURL_HOME=/tmp/curlhome curl -H \"Authorization: Bearer $GH_TOKEN\" https://api.github.com/user",
        here,
    );
    refused(
        "WGETRC=/tmp/wgetrc wget -q https://api.github.com/user --header \"Authorization: Bearer $GH_TOKEN\"",
        here,
    );
}

/// A curlrc can carry both the method and the token. The command line
/// shows neither. `-q` or `--disable` as the first argument is the
/// switch that leaves the default file unread.
#[test]
fn a_curl_config_is_a_write_without_a_token_on_the_command() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, xdg) = seat_env(root.path());
    clear_ids();
    ljos_cli::cloud::up(false).unwrap();
    let here = root.path();
    let rc = "request = DELETE\nheader = Authorization: Bearer x\n";

    fs::write(home.join(".curlrc"), rc).unwrap();
    refused("curl -s https://api.github.com/user", here);
    allowed("curl -q -s https://api.github.com/user", here);
    allowed("curl --disable -s https://api.github.com/user", here);
    refused("curl -s -q https://api.github.com/user", here);
    refused("curl -s --disable https://api.github.com/user", here);

    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(home.join(".curlrc")).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(home.join(".curlrc"), perms.clone()).unwrap();
    refused("curl -s https://api.github.com/user", here);
    allowed("curl -q -s https://api.github.com/user", here);
    perms.set_mode(0o644);
    fs::set_permissions(home.join(".curlrc"), perms).unwrap();
    fs::remove_file(home.join(".curlrc")).unwrap();

    let dir = root.path().join("curlhome");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".curlrc"), rc).unwrap();
    refused(
        &format!(
            "CURL_HOME={} curl -s https://api.github.com/user",
            dir.display()
        ),
        here,
    );

    fs::write(xdg.join("curlrc"), rc).unwrap();
    refused("curl -s https://api.github.com/user", here);
    fs::remove_file(xdg.join("curlrc")).unwrap();

    let inner = root.path().join("inner.cfg");
    fs::write(&inner, rc).unwrap();
    fs::write(
        home.join(".curlrc"),
        format!("config = \"{}\"\n", inner.display()),
    )
    .unwrap();
    refused("curl -s https://api.github.com/user", here);
    fs::remove_file(home.join(".curlrc")).unwrap();

    let named = root.path().join("k.cfg");
    fs::write(
        &named,
        "url = \"https://api.github.com/user\"\nrequest = \"DELETE\"\nheader = \"Authorization: Bearer x\"\n",
    )
    .unwrap();
    refused(&format!("curl -K {}", named.display()), here);
    refused(&format!("curl --config {}", named.display()), here);
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
