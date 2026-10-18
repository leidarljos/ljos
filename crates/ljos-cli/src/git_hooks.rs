//! Global git hooks that book a commit when the runner has no prompt hook.
//!
//! `post-commit` and `post-rewrite` call `ljos hook --git-commit`. The
//! commit is never failed. A tracker checkout, a claimdag or deedar store,
//! the middle of a rebase or merge, and a Claude Code, Codex, or opencode
//! session are left to their own hooks. An amend and a rebase book once.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

const HOOK_NAMES: [&str; 2] = ["post-commit", "post-rewrite"];

/// `install`, `uninstall`, or `status`.
///
/// # Errors
///
/// `action` is not one of those, or git cannot record `core.hooksPath`.
pub fn run(action: &str) -> Result<()> {
    match action {
        "install" => {
            let dir = install()?;
            println!("installed {}", dir.display());
            Ok(())
        }
        "uninstall" => {
            uninstall()?;
            println!("uninstalled");
            Ok(())
        }
        "status" => {
            print!("{}", status());
            Ok(())
        }
        other => bail!("git-hooks: {other} is not install, uninstall, or status"),
    }
}

/// Write the hook scripts and point `core.hooksPath` at them.
///
/// # Errors
///
/// The directory cannot be written, or `git config` fails.
pub fn install() -> Result<PathBuf> {
    install_at(&hooks_dir(), &[])
}

/// [`install`] into `dir`, with `git_env` on the child `git config`.
///
/// An empty `git_env` leaves that child with this process's environment.
/// `GIT_CONFIG_GLOBAL` and `HOME` belong here when the caller is keeping
/// the login config untouched.
///
/// # Errors
///
/// The directory cannot be written, or `git config` fails.
pub(crate) fn install_at(dir: &Path, git_env: &[(&str, &str)]) -> Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("git-hooks: {}", dir.display()))?;
    let bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ljos"));
    for name in HOOK_NAMES {
        let path = dir.join(name);
        fs::write(&path, hook_body(&bin, name))
            .with_context(|| format!("git-hooks: {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
    }
    let current = global_hooks_path(git_env);
    let ours = dir.display().to_string();
    if current.as_deref() != Some(ours.as_str()) {
        match current {
            Some(prev) => {
                fs::write(dir.join("previous-hooks-path"), prev)?;
            }
            None => {
                let _ = fs::remove_file(dir.join("previous-hooks-path"));
            }
        }
    }
    set_global_hooks_path(dir, git_env)?;
    Ok(dir.to_path_buf())
}

/// Remove the scripts and put `core.hooksPath` back.
///
/// # Errors
///
/// `git config` cannot restore or unset the path.
pub fn uninstall() -> Result<()> {
    let dir = hooks_dir();
    let current = global_hooks_path(&[]);
    let ours = dir.display().to_string();
    if current.as_deref() == Some(ours.as_str()) {
        let previous = fs::read_to_string(dir.join("previous-hooks-path"))
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        if let Some(prev) = previous {
            set_global_hooks_path(Path::new(&prev), &[])?;
        } else {
            unset_global_hooks_path(&[])?;
        }
    }
    for name in HOOK_NAMES {
        let _ = fs::remove_file(dir.join(name));
    }
    let _ = fs::remove_file(dir.join("previous-hooks-path"));
    Ok(())
}

/// Where the hooks are, and what `core.hooksPath` says.
#[must_use]
pub fn status() -> String {
    let dir = hooks_dir();
    let installed = HOOK_NAMES.iter().all(|name| dir.join(name).is_file());
    let configured = global_hooks_path(&[]).unwrap_or_else(|| "(unset)".into());
    let hooks = if installed {
        dir.display().to_string()
    } else {
        "not installed".into()
    };
    format!("hooks: {hooks}\ncore.hooksPath: {configured}\n")
}

fn hooks_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LJOS_GIT_HOOKS").filter(|value| !value.is_empty()) {
        return PathBuf::from(dir);
    }
    data_home().join("ljos").join("git-hooks")
}

fn data_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".local").join("share"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn hook_body(bin: &Path, name: &str) -> String {
    let bin = shell_quote(&bin.display().to_string());
    format!(
        "#!/bin/sh\n\
         # ljos books this commit. A failure here must not fail the commit.\n\
         case \",1,true,TRUE,yes,YES,on,ON,\" in\n\
         \t*,\"${{LJOS_OFF}}\",*) exit 0 ;;\n\
         esac\n\
         if [ -n \"${{LJOS_GIT_HOOK:-}}\" ]; then\n\
         \texit 0\n\
         fi\n\
         line=$({bin} hook --git-commit --git-hook {name} 2>/dev/null || true)\n\
         if [ -n \"$line\" ]; then\n\
         \tprintf '%s\\n' \"$line\" >&2\n\
         fi\n\
         git_dir=$(git rev-parse --git-dir 2>/dev/null || true)\n\
         if [ -n \"$git_dir\" ]; then\n\
         \tcase \"$git_dir\" in\n\
         \t\t/*) ;;\n\
         \t\t*) git_dir=$(pwd)/$git_dir ;;\n\
         \tesac\n\
         \trepo_hook=\"$git_dir/hooks/{name}\"\n\
         \tif [ -x \"$repo_hook\" ] && [ \"$repo_hook\" != \"$0\" ]; then\n\
         \t\t\"$repo_hook\" \"$@\" || true\n\
         \tfi\n\
         fi\n\
         exit 0\n"
    )
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Book the commit that just landed. Always succeeds: the commit stands.
///
/// # Errors
///
/// None. A habitat that did not answer is a line in the book log.
pub fn run_git_commit(hook: &str) -> Result<()> {
    if let Err(err) = book_commit(hook) {
        crate::bookkeep::log_line(&format!("git-hooks: {err:#}"));
    }
    Ok(())
}

fn book_commit(hook: &str) -> Result<()> {
    if std::env::var_os("LJOS_GIT_HOOK").is_some() || booking_off() {
        return Ok(());
    }
    if foreign_session() {
        return Ok(());
    }
    if hook != "post-rewrite" && mid_rewrite() {
        return Ok(());
    }
    let Some(repo) = git_line(&["rev-parse", "--show-toplevel"]) else {
        return Ok(());
    };
    let repo_path = PathBuf::from(&repo);
    if crate::bookkeep::path_is_temp(Some(&repo_path)) || skip_repo(&repo_path) {
        return Ok(());
    }
    let Some(head) = git_line(&["rev-parse", "HEAD"]) else {
        return Ok(());
    };
    if already_booked(&repo, &head) {
        return Ok(());
    }
    let subject = git_line(&["log", "-1", "--format=%s"]).unwrap_or_default();
    // A hook exports GIT_DIR. Later `git -C` on the tracker must not follow
    // it, and a nested commit must not re-enter this hook. Both ride on the
    // child git and ljos processes; this process keeps the environment it
    // was started with, so the rev-parse above still sees the checkout.
    let _children = HookChildren::enter();
    let session = crate::bookkeep::seat_session();
    let command = if subject.is_empty() {
        "git commit".to_string()
    } else {
        format!("git commit -m {}", shell_quote(&subject))
    };
    let raw = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": session,
        "cwd": repo,
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_response": {"exit_code": 0},
        "prompt": subject,
    })
    .to_string();
    let call = crate::HookCall {
        event: "PostToolUse".into(),
        cue: command,
        session: Some(session.clone()),
        shape: crate::HookShape::Asks,
    };
    let _ = crate::bookkeep::observe(&raw, &call);
    remember_booked(&repo, &head);
    if let Some(line) = crate::bookkeep::commit_recall_line(&session, &repo, &subject) {
        println!("{line}");
    }
    Ok(())
}

fn booking_off() -> bool {
    matches!(
        std::env::var("LJOS_OFF").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    ) || crate::bookkeep::guard_suspended()
}

fn foreign_session() -> bool {
    [
        "CLAUDECODE",
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
        "OPENCODE_SESSION_ID",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

fn mid_rewrite() -> bool {
    let Some(dir) = git_dir() else {
        return false;
    };
    dir.join("rebase-merge").is_dir()
        || dir.join("rebase-apply").is_dir()
        || dir.join("MERGE_HEAD").is_file()
}

fn skip_repo(repo: &Path) -> bool {
    for key in ["VISSUE_ROOT", "ISSUE_ROOT"] {
        if let Some(root) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            if same_dir(repo, Path::new(&root)) {
                return true;
            }
        }
    }
    is_store(repo)
}

fn is_store(repo: &Path) -> bool {
    let text = repo.to_string_lossy();
    if text.contains("/deedar/store") || text.contains("/claimdag/") || text.ends_with("/claimdag")
    {
        return true;
    }
    store_dirs()
        .iter()
        .any(|root| same_dir(repo, root) || repo_starts(repo, root))
}

fn store_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(url) = std::env::var("DEEDAR_URL") {
        if let Some(path) = url.strip_prefix("file://") {
            out.push(PathBuf::from(path));
        }
    }
    if let Some(data) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        out.push(PathBuf::from(data).join("deedar").join("store"));
    } else if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        out.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("deedar")
                .join("store"),
        );
    }
    out
}

fn same_dir(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn repo_starts(repo: &Path, root: &Path) -> bool {
    if !root.exists() {
        return false;
    }
    let repo = fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    repo.starts_with(root)
}

fn already_booked(repo: &str, head: &str) -> bool {
    let key = format!("{repo}\t{head}");
    fs::read_to_string(booked_path())
        .map(|text| text.lines().any(|line| line == key))
        .unwrap_or(false)
}

fn remember_booked(repo: &str, head: &str) {
    let path = booked_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let mut lines: Vec<String> = fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .filter(|line| !line.starts_with(&format!("{repo}\t")))
        .collect();
    lines.push(format!("{repo}\t{head}"));
    if lines.len() > 200 {
        lines.drain(0..lines.len() - 200);
    }
    let _ = fs::write(path, lines.join("\n") + "\n");
}

fn booked_path() -> PathBuf {
    crate::runtime_dir().join("git-hooks-booked")
}

fn git_dir() -> Option<PathBuf> {
    let raw = git_line(&["rev-parse", "--git-dir"])?;
    let path = PathBuf::from(&raw);
    if path.is_absolute() {
        Some(path)
    } else {
        std::env::current_dir().ok().map(|cwd| cwd.join(path))
    }
}

/// `GIT_*` values a hook exports. A later `git -C` follows `GIT_DIR` and
/// ignores `-C`, so a tracker commit would land in the checkout that just
/// finished.
const HOOK_GIT_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_PREFIX",
    "GIT_OBJECT_DIRECTORY",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
];

thread_local! {
    static HOOK_CHILDREN: Cell<bool> = const { Cell::new(false) };
}

/// While this guard is alive, child processes started through
/// [`hook_child_env`] carry `LJOS_GIT_HOOK=1` and do not inherit the hook's
/// `GIT_DIR`. Dropping it restores the previous flag. The process
/// environment is not changed.
#[must_use]
struct HookChildren {
    previous: bool,
}

impl HookChildren {
    fn enter() -> Self {
        let previous = HOOK_CHILDREN.with(Cell::get);
        HOOK_CHILDREN.with(|flag| flag.set(true));
        Self { previous }
    }
}

impl Drop for HookChildren {
    fn drop(&mut self) {
        HOOK_CHILDREN.with(|flag| flag.set(self.previous));
    }
}

/// Put the re-entry guard on a child `git` or `ljos`, and drop the hook's
/// git directory from that child. No effect unless a git hook is booking
/// a commit on this thread.
pub(crate) fn hook_child_env(cmd: &mut Command) {
    if !HOOK_CHILDREN.with(Cell::get) {
        return;
    }
    cmd.env("LJOS_GIT_HOOK", "1");
    for key in HOOK_GIT_ENV {
        cmd.env_remove(key);
    }
}

fn git_command(args: &[&str], git_env: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args);
    for (key, value) in git_env {
        cmd.env(key, value);
    }
    hook_child_env(&mut cmd);
    cmd
}

fn git_line(args: &[&str]) -> Option<String> {
    let out = git_command(args, &[]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn global_hooks_path(git_env: &[(&str, &str)]) -> Option<String> {
    let out = git_command(&["config", "--global", "--get", "core.hooksPath"], git_env)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn set_global_hooks_path(path: &Path, git_env: &[(&str, &str)]) -> Result<()> {
    let value = path.display().to_string();
    let status = git_command(&["config", "--global", "core.hooksPath", &value], git_env)
        .status()
        .context("git-hooks: git config")?;
    if !status.success() {
        bail!("git-hooks: could not set core.hooksPath");
    }
    Ok(())
}

fn unset_global_hooks_path(git_env: &[(&str, &str)]) -> Result<()> {
    let status = git_command(
        &["config", "--global", "--unset", "core.hooksPath"],
        git_env,
    )
    .status()
    .context("git-hooks: git config")?;
    if !status.success() {
        bail!("git-hooks: could not unset core.hooksPath");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn mapped<'a>(cmd: &'a Command, key: &str) -> Option<Option<&'a OsStr>> {
        cmd.get_envs()
            .find(|(name, _)| *name == OsStr::new(key))
            .map(|(_, value)| value)
    }

    #[test]
    fn hook_children_carry_the_guard_without_touching_the_process() {
        let before_hook = std::env::var_os("LJOS_GIT_HOOK");
        let before_dir = std::env::var_os("GIT_DIR");
        let mut plain = Command::new("true");
        plain.env("GIT_DIR", "keep");
        hook_child_env(&mut plain);
        assert_eq!(mapped(&plain, "GIT_DIR"), Some(Some(OsStr::new("keep"))));
        assert_eq!(mapped(&plain, "LJOS_GIT_HOOK"), None);

        let _guard = HookChildren::enter();
        let mut cmd = Command::new("true");
        cmd.env("GIT_DIR", "from-the-hook");
        cmd.env("PATH", "/usr/bin");
        hook_child_env(&mut cmd);
        assert_eq!(mapped(&cmd, "LJOS_GIT_HOOK"), Some(Some(OsStr::new("1"))));
        for key in HOOK_GIT_ENV {
            assert_eq!(mapped(&cmd, key), Some(None), "{key}");
        }
        assert_eq!(mapped(&cmd, "PATH"), Some(Some(OsStr::new("/usr/bin"))));
        assert_eq!(std::env::var_os("LJOS_GIT_HOOK"), before_hook);
        assert_eq!(std::env::var_os("GIT_DIR"), before_dir);
    }
}
