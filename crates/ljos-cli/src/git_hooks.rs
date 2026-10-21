//! Global git hooks that book a commit when the runner has no prompt hook.
//!
//! `install` writes a chaining stub for every standard git hook. The stub
//! execs the repository's `.git/hooks/<name>` or, when that file is absent,
//! the `core.hooksPath` that was set before install, with the same arguments
//! and stdin. `post-commit` and `post-rewrite` also call `ljos hook
//! --git-commit`. Booking never fails the hook.
//!
//! `pre-commit install` follows `core.hooksPath` and would replace these
//! stubs. Install the framework into the repository, then let the stub chain
//! to it:
//!
//! ```text
//! dir=$(git rev-parse --git-dir)
//! git config --local core.hooksPath "$dir/hooks"
//! pre-commit install
//! git config --local --unset core.hooksPath
//! ```
//!
//! A commit is booked only when a seat resolves from `LJOS_SEAT`, then
//! `seats.toml` through `CURSOR_CONVERSATION_ID`, then `grokbot-<id8>`,
//! then an agy or opencode conversation id. The process tree is not a seat.
//! The holder is `SEAT@TAG`, the same name `ljos claim` uses. Paths in
//! `[git_hooks] skip` of `ljos.toml`, plus `/tmp`, `/var/tmp`, `/dev/shm`,
//! the tracker, `CLAIMDAG_DIR` and the deedar store, are not booked.
//! An amend or a rebase moves the citation onto the new sha.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Every hook git looks up by name. A global `core.hooksPath` hides the
/// repository's copy of each of these, so the stub has to exist for each.
const HOOK_NAMES: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

fn books_commit(name: &str) -> bool {
    matches!(name, "post-commit" | "post-rewrite")
}

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

/// The start of the doctor's `git hooks` state when `core.hooksPath` is
/// unset. That row is `info`: a seat that has not onboarded books no
/// commits, and nothing else is wrong.
pub const HOOKS_UNSET: &str = "core.hooksPath is unset";

/// The doctor's `git hooks` row: whether the global `core.hooksPath`
/// names the hooks `ljos onboard` installs, and whether they are there.
/// A path that names another directory books a commit only if that
/// directory runs ljos, and an env file or a note that names it drifts
/// from what onboard wrote, so the row says which path is set and which
/// one onboard installs.
#[must_use]
pub fn doctor_row() -> crate::Habitat {
    let dir = hooks_dir();
    let installed = HOOK_NAMES.iter().all(|name| dir.join(name).is_file());
    let (ok, state) = doctor_state(
        &dir,
        global_hooks_path(&[]).as_deref(),
        installed,
        &config_ljos_dir(),
    );
    crate::Habitat {
        name: "git hooks",
        state,
        ok,
    }
}

/// [`doctor_row`] for a chosen hooks directory, the global `core.hooksPath`,
/// and the config directory that holds env files.
#[must_use]
pub fn doctor_state(
    dir: &Path,
    configured: Option<&str>,
    installed: bool,
    config_dir: &Path,
) -> (bool, String) {
    let (mut ok, mut state) = hooks_state(dir, configured, installed);
    if let Some(drift) = env_hooks_drift(config_dir, dir) {
        ok = false;
        // A mismatch is a failed row. The unset-path prefix would keep it `info`.
        state = if state.starts_with(HOOKS_UNSET) {
            drift
        } else {
            format!("{state}; {drift}")
        };
    }
    (ok, state)
}

/// An env file under `config_dir` that sets `LJOS_GIT_HOOKS` to a directory
/// other than `onboard_dir`. The expression [`HOOKS_ENV_LINE`] writes is the
/// same directory and is not a mismatch. `None` when every assignment agrees.
#[must_use]
pub fn env_hooks_drift(config_dir: &Path, onboard_dir: &Path) -> Option<String> {
    let mut files = Vec::new();
    for name in ["env.sh", "env"] {
        let path = config_dir.join(name);
        if path.is_file() {
            files.push(path);
        }
    }
    if let Ok(entries) = fs::read_dir(config_dir) {
        let mut more: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("env"))
            .collect();
        more.sort();
        files.extend(more);
    }
    let ours = onboard_dir.display().to_string();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            let Some(value) = hooks_assignment(line) else {
                continue;
            };
            if assignment_agrees(&value, onboard_dir) {
                continue;
            }
            return Some(format!(
                "{} sets LJOS_GIT_HOOKS to {value}, not {ours}, the hooks `ljos onboard` installs",
                path.display()
            ));
        }
    }
    None
}

fn hooks_assignment(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let rest = line.strip_prefix("export ").unwrap_or(line).trim();
    let rest = rest.strip_prefix("LJOS_GIT_HOOKS=")?;
    let value = unquote_assignment(rest.trim());
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn unquote_assignment(value: &str) -> String {
    let value = value.trim();
    if let Some(inner) = value
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
    {
        return inner.to_string();
    }
    if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|text| text.strip_suffix('\''))
    {
        return inner.to_string();
    }
    value.split_whitespace().next().unwrap_or("").to_string()
}

fn assignment_agrees(value: &str, onboard: &Path) -> bool {
    if value.contains("${XDG_DATA_HOME:-$HOME/.local/share}/ljos/git-hooks") {
        return true;
    }
    if value.contains('$') || value.contains('`') {
        return false;
    }
    same_dir(Path::new(value), onboard)
}

fn config_ljos_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("ljos")
}

/// The `LJOS_GIT_HOOKS` line `ljos onboard` writes into a shell runner's
/// env file. It names the same directory [`install`] writes.
pub const HOOKS_ENV_LINE: &str = "\
# Git hooks directory. `ljos git-hooks install` writes the stubs here.\n\
export LJOS_GIT_HOOKS=\"${LJOS_GIT_HOOKS:-${XDG_DATA_HOME:-$HOME/.local/share}/ljos/git-hooks}\"";

/// [`doctor_row`]'s verdict for the onboard hooks at `dir`, the global
/// `core.hooksPath` git reports, and whether every stub is in `dir`.
#[must_use]
pub fn hooks_state(dir: &Path, configured: Option<&str>, installed: bool) -> (bool, String) {
    let ours = dir.display().to_string();
    match configured.map(str::trim).filter(|path| !path.is_empty()) {
        None => (
            false,
            format!(
                "{HOOKS_UNSET}, so commits are not booked; `ljos git-hooks install` points it at {ours}"
            ),
        ),
        Some(path) if !same_dir(Path::new(path), dir) => (
            false,
            format!(
                "core.hooksPath is {path}, not {ours}, the hooks `ljos onboard` installs; `ljos git-hooks install` points it back"
            ),
        ),
        Some(_) if !installed => (
            false,
            format!(
                "core.hooksPath is {ours}, but some hook stubs are missing there; `ljos git-hooks install` writes them"
            ),
        ),
        Some(_) => (true, format!("core.hooksPath is {ours}; commits are booked")),
    }
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
    // post-rewrite's stdin is the old and new shas. Copy it so both ljos
    // and the repository hook can read it. Other hooks leave stdin alone.
    let capture = if name == "post-rewrite" {
        "\
         _ljos_in=$(mktemp 2>/dev/null || true)\n\
         if [ -n \"$_ljos_in\" ]; then\n\
         \tcat > \"$_ljos_in\" || true\n\
         fi\n"
    } else {
        ""
    };
    let book = if books_commit(name) {
        let read = if name == "post-rewrite" {
            format!(
                "\t\t\tif [ -n \"$_ljos_in\" ]; then\n\
                 \t\t\t\tline=$({bin} hook --git-commit --git-hook {name} < \"$_ljos_in\" 2>/dev/null || true)\n\
                 \t\t\telse\n\
                 \t\t\t\tline=$({bin} hook --git-commit --git-hook {name} 2>/dev/null || true)\n\
                 \t\t\tfi\n"
            )
        } else {
            format!("\t\t\tline=$({bin} hook --git-commit --git-hook {name} 2>/dev/null || true)\n")
        };
        format!(
            "\t\tif [ -z \"${{LJOS_GIT_HOOK:-}}\" ]; then\n\
             {read}\
             \t\t\tif [ -n \"$line\" ]; then\n\
             \t\t\t\tprintf '%s\\n' \"$line\" >&2\n\
             \t\t\tfi\n\
             \t\tfi\n"
        )
    } else {
        String::new()
    };
    format!(
        "#!/bin/sh\n\
         # ljos chains to this repository's .git/hooks/{name}, or to the\n\
         # core.hooksPath that was set before `ljos git-hooks install`.\n\
         # post-commit and post-rewrite also book the commit. Booking cannot\n\
         # fail the hook. The repository hook's exit status is kept.\n\
         # post-rewrite reads old and new shas on stdin and keeps a copy for\n\
         # the repository hook.\n\
         #\n\
         # pre-commit install follows core.hooksPath and would replace this\n\
         # file. Install the framework into the repository, then re-run\n\
         # `ljos git-hooks install` if a stub was overwritten:\n\
         #   dir=$(git rev-parse --git-dir)\n\
         #   git config --local core.hooksPath \"$dir/hooks\"\n\
         #   pre-commit install\n\
         #   git config --local --unset core.hooksPath\n\
         {capture}\
         case \",1,true,TRUE,yes,YES,on,ON,\" in\n\
         \t*,\"${{LJOS_OFF}}\",*) ;;\n\
         \t*)\n\
         {book}\
         \t\t;;\n\
         esac\n\
         git_dir=$(git rev-parse --git-dir 2>/dev/null || true)\n\
         if [ -n \"$git_dir\" ]; then\n\
         \tcase \"$git_dir\" in\n\
         \t\t/*) ;;\n\
         \t\t*) git_dir=$(pwd)/$git_dir ;;\n\
         \tesac\n\
         \trepo_hook=\"$git_dir/hooks/{name}\"\n\
         \tif [ -x \"$repo_hook\" ] && [ \"$repo_hook\" != \"$0\" ]; then\n\
         \t\tif [ -n \"${{_ljos_in:-}}\" ]; then\n\
         \t\t\texec \"$repo_hook\" \"$@\" < \"$_ljos_in\"\n\
         \t\tfi\n\
         \t\texec \"$repo_hook\" \"$@\"\n\
         \tfi\n\
         fi\n\
         here=$(dirname \"$0\")\n\
         here=$(CDPATH= cd -- \"$here\" && pwd) || exit 0\n\
         prev=$(sed -n '1s/[[:space:]]*$//p' \"$here/previous-hooks-path\" 2>/dev/null || true)\n\
         if [ -n \"$prev\" ] && [ \"$prev\" != \"$here\" ]; then\n\
         \tprev_hook=\"$prev/{name}\"\n\
         \tif [ -x \"$prev_hook\" ] && [ \"$prev_hook\" != \"$0\" ]; then\n\
         \t\tif [ -n \"${{_ljos_in:-}}\" ]; then\n\
         \t\t\texec \"$prev_hook\" \"$@\" < \"$_ljos_in\"\n\
         \t\tfi\n\
         \t\texec \"$prev_hook\" \"$@\"\n\
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
    // The process tree is not a seat. Without LJOS_SEAT, seats.toml, the
    // grokbot-<id8> fallback, or an agy or opencode conversation, the
    // commit stays unbooked; a subagent is told.
    let Some(seat) = crate::git_hook_seat() else {
        if crate::unseated_subagent() {
            // Stdout: the installed hook shows that line and drops stderr.
            println!(
                "ljos: commit {} not booked: this subagent has no seat; export LJOS_SEAT=<your seat> (the parent's) to book it",
                &head[..head.len().min(12)]
            );
        }
        return Ok(());
    };
    let _actor = crate::HookActor::enter(&seat);
    let _children = HookChildren::enter();
    // An amend or a rebase lists old and new shas. Move the citation
    // before the "already booked" return: post-commit may have cited the
    // new HEAD already, and the old sha would otherwise stay on the issue.
    if hook == "post-rewrite" {
        crate::bookkeep::repoint_commits(&repo, &rewrite_pairs());
    }
    if already_booked(&repo, &head) {
        return Ok(());
    }
    let subject = git_line(&["log", "-1", "--format=%s"]).unwrap_or_default();
    // A hook exports GIT_DIR. Later `git -C` on the tracker must not follow
    // it, and a nested commit must not re-enter this hook. Both ride on the
    // child git and ljos processes; this process keeps the environment it
    // was started with, so the rev-parse above still sees the checkout.
    // Finish idle rows in other checkouts before this commit is cited into
    // an open sitting. That path returns without observe, which is the
    // other place the idle sweep runs.
    let session = crate::harness_book_session().unwrap_or_else(crate::bookkeep::seat_session);
    crate::bookkeep::sweep_idle_sittings(&session, &repo);
    // A subject or body that names an open issue (`ljos-policyd-32fx: ...`,
    // `Fixes ljos-xsor`) books the commit on that issue. A new issue is
    // filed only when the message names none the tracker holds open.
    let message = git_line(&["log", "-1", "--format=%B"]).unwrap_or_default();
    if let Some(issue) = crate::bookkeep::named_open_issue(&message) {
        crate::bookkeep::book_named_commit(&repo, &head, &issue);
        remember_booked(&repo, &head);
        if let Some(line) = crate::bookkeep::commit_recall_line(&session, &repo, &subject) {
            println!("{line}");
        }
        return Ok(());
    }
    // The commit already belongs to an open sitting in this checkout.
    // Booking it on the seat's own session would open a second book.
    if crate::bookkeep::cite_into_open_sitting(&repo) {
        remember_booked(&repo, &head);
        return Ok(());
    }
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
        "ljos_git_hook": true,
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
    // OpenCode's shell sets OPENCODE_SESSION_ID and never calls the plugin's
    // tool.execute.after, so the git hook is the booker. Claude and Codex
    // book from their own hooks.
    ["CLAUDECODE", "CODEX_THREAD_ID", "CODEX_SESSION_ID"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

fn rewrite_pairs() -> Vec<(String, String)> {
    use std::io::{IsTerminal, Read};
    if std::io::stdin().is_terminal() {
        return Vec::new();
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return Vec::new();
    }
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let old = parts.next()?;
            let new = parts.next()?;
            if commit_sha(old) && commit_sha(new) {
                Some((old.to_string(), new.to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn commit_sha(text: &str) -> bool {
    (text.len() == 40 || text.len() == 64) && text.chars().all(|c| c.is_ascii_hexdigit())
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
    if is_store(repo) {
        return true;
    }
    skip_roots().iter().any(|root| covered(repo, root))
}

/// Paths a commit under which is not booked. The defaults always apply.
/// `[git_hooks] skip` in `ljos.toml` adds box-specific paths, and a
/// reinstall rewrites the hook scripts without touching that file.
fn skip_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/tmp"),
        PathBuf::from("/var/tmp"),
        PathBuf::from("/dev/shm"),
    ];
    for key in ["VISSUE_ROOT", "ISSUE_ROOT", "CLAIMDAG_DIR"] {
        if let Some(path) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            roots.push(PathBuf::from(path));
        }
    }
    roots.extend(store_dirs());
    for path in config_files() {
        roots.extend(configured_skips(&path));
    }
    roots.retain(|path| useful_root(path));
    roots
}

fn useful_root(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let text = text.trim();
    !text.is_empty() && text != "/" && text != "."
}

fn config_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        });
    if let Some(base) = base {
        out.push(base.join("ljos").join("ljos.toml"));
        if let Some(seat) = std::env::var("LJOS_SEAT")
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty() && !name.contains('/') && !name.contains(".."))
        {
            out.push(base.join("ljos").join("seats").join(seat).join("ljos.toml"));
        }
    }
    if let Some(path) = std::env::var_os("LJOS_CONFIG").filter(|value| !value.is_empty()) {
        out.push(PathBuf::from(path));
    }
    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd.join("ljos.toml"));
    }
    out
}

fn configured_skips(path: &Path) -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return Vec::new();
    };
    let Some(list) = value
        .get("git_hooks")
        .and_then(|table| table.get("skip"))
        .and_then(toml::Value::as_array)
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(toml::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn covered(repo: &Path, root: &Path) -> bool {
    let repo = canonical_or(repo);
    let root = canonical_or(root);
    repo == root || repo.starts_with(&root)
}

fn canonical_or(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
    fn the_doctor_names_a_hooks_path_that_is_not_the_onboard_one() {
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join("ljos").join("git-hooks");
        std::fs::create_dir_all(&ours).unwrap();
        let other = dir.path().join("leidarljos").join("git-hooks");
        std::fs::create_dir_all(&other).unwrap();
        let ours_s = ours.display().to_string();
        let other_s = other.display().to_string();

        let (ok, state) = super::hooks_state(&ours, None, true);
        assert!(!ok);
        assert!(state.starts_with(super::HOOKS_UNSET), "{state}");

        let (ok, state) = super::hooks_state(&ours, Some(&other_s), true);
        assert!(!ok);
        assert!(
            state.contains(&other_s) && state.contains(&ours_s),
            "{state}"
        );

        let (ok, state) = super::hooks_state(&ours, Some(&ours_s), false);
        assert!(!ok, "{state}");

        let (ok, state) = super::hooks_state(&ours, Some(&ours_s), true);
        assert!(ok, "{state}");

        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("env.sh"),
            format!("export LJOS_GIT_HOOKS={}\n", other.display()),
        )
        .unwrap();
        let drift = super::env_hooks_drift(&config, &ours).expect("a drifted env file");
        assert!(drift.contains(&other_s), "{drift}");
        assert!(drift.contains(&ours_s), "{drift}");
        let (ok, state) = super::doctor_state(&ours, None, true, &config);
        assert!(!ok, "{state}");
        assert!(
            !state.starts_with(super::HOOKS_UNSET),
            "a mismatch stayed info: {state}"
        );
        assert!(state.contains("leidarljos"), "{state}");
        assert!(state.contains(&ours_s), "{state}");

        std::fs::write(
            config.join("env.sh"),
            format!("{}\n", super::HOOKS_ENV_LINE),
        )
        .unwrap();
        assert!(super::env_hooks_drift(&config, &ours).is_none());
        let body = super::hook_body(Path::new("/usr/bin/ljos"), "post-rewrite");
        assert!(body.contains("mktemp"), "{body}");
        assert!(body.contains("--git-hook post-rewrite"), "{body}");
        assert!(body.contains("< \"$_ljos_in\""), "{body}");
        let commit = super::hook_body(Path::new("/usr/bin/ljos"), "post-commit");
        assert!(!commit.contains("mktemp"), "{commit}");
        assert!(commit.contains("--git-hook post-commit"), "{commit}");
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
