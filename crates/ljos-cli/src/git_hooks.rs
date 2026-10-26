//! Global git hooks that book a commit when the runner has no prompt hook.
//!
//! When `core.hooksPath` is unset, `install` writes a stub for every
//! standard git hook and points the global config at ljos's directory. The
//! stub calls `ljos git-hook <name>`, then runs the repository's
//! `.git/hooks/<name>` or the previous path an older install recorded, with
//! the same arguments. `post-rewrite` copies stdin first, so `ljos git-hook`
//! and the chained hook both see the old and new shas.
//!
//! When `core.hooksPath` already names a directory, install leaves that
//! setting alone and appends a delimited block to each hook there. The
//! block calls `ljos git-hook <name>`. A missing hook file is created
//! executable. Text that was already in the file is kept. Uninstall removes
//! only that block, and deletes a file that held nothing else.
//!
//! `post-commit` and `post-rewrite` book the commit. Booking never fails
//! the hook. `pre-commit` and `pre-push` run the sitting gate under an
//! agent runner: a commit needs a live sitting, and a push onto a
//! protected branch also needs the sitting's ticket to clear it. That
//! refusal fails the hook. `pre-push` copies stdin like `post-rewrite`.
//!
//! `pre-commit install` follows `core.hooksPath` and would replace stubs
//! ljos owns. Install the framework into the repository, then let the stub
//! chain to it:
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

use anyhow::{anyhow, bail, Context, Result};

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

fn gates_git(name: &str) -> bool {
    matches!(name, "pre-commit" | "pre-push")
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

/// Install the hooks `ljos doctor` checks.
///
/// An unset `core.hooksPath` is pointed at ljos's hook directory. A path
/// that already names a directory is left alone, and each hook there gains
/// a block that calls `ljos git-hook`.
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
    let current = global_hooks_path(git_env);
    if let Some(path) = nonempty(current.as_deref()) {
        if !same_dir(Path::new(path), dir) {
            chain_user_hooks(Path::new(path))?;
            return Ok(PathBuf::from(path));
        }
    }
    write_owned_hooks(dir)?;
    if nonempty(current.as_deref()).is_none() {
        let _ = fs::remove_file(dir.join("previous-hooks-path"));
        set_global_hooks_path(dir, git_env)?;
    }
    Ok(dir.to_path_buf())
}

/// Remove ljos from the hooks git runs.
///
/// A directory ljos does not own loses only the ljos block. A file that
/// held nothing else is deleted. When ljos owns `core.hooksPath`, the
/// previous path is restored and the stubs are removed.
///
/// # Errors
///
/// `git config` cannot restore or unset the path, or a hook file cannot
/// be rewritten.
pub fn uninstall() -> Result<()> {
    let dir = hooks_dir();
    let current = global_hooks_path(&[]);
    match nonempty(current.as_deref()) {
        Some(path) if same_dir(Path::new(path), &dir) => {
            let previous = fs::read_to_string(dir.join("previous-hooks-path"))
                .ok()
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty());
            if let Some(prev) = previous {
                set_global_hooks_path(Path::new(&prev), &[])?;
            } else {
                unset_global_hooks_path(&[])?;
            }
            remove_owned_hooks(&dir);
        }
        Some(path) => strip_user_hooks(Path::new(path))?,
        None => remove_owned_hooks(&dir),
    }
    Ok(())
}

/// Where the hooks are, and what `core.hooksPath` says.
#[must_use]
pub fn status() -> String {
    let dir = hooks_dir();
    let configured_path = global_hooks_path(&[]);
    let configured = configured_path.clone().unwrap_or_else(|| "(unset)".into());
    let reachable = nonempty(configured_path.as_deref())
        .is_some_and(|path| hooks_reachable(Path::new(path), &dir));
    let hooks = if reachable {
        configured.clone()
    } else if owned_installed(&dir) {
        dir.display().to_string()
    } else {
        "not installed".into()
    };
    let reach = if reachable {
        "ljos hooks run".to_string()
    } else {
        format!("ljos hooks do not run; {HOOKS_FIX}")
    };
    format!("hooks: {hooks}\ncore.hooksPath: {configured}\n{reach}\n")
}

/// The start of the doctor's `git hooks` state when `core.hooksPath` is
/// unset. That row is `info`: a seat that has not onboarded books no
/// commits, and nothing else is wrong.
pub const HOOKS_UNSET: &str = "core.hooksPath is unset";

/// The one command that installs the hooks doctor checks.
pub const HOOKS_FIX: &str = "ljos git-hooks install";

/// The doctor's `git hooks` row. It passes when `ljos git-hook` runs from
/// the directory `core.hooksPath` names: ljos's own stubs, or a block in
/// each hook of a directory the user already set. It does not ask to take
/// that path over. An env file that sets `LJOS_GIT_HOOKS` to another
/// directory fails the row.
#[must_use]
pub fn doctor_row() -> crate::Habitat {
    let dir = hooks_dir();
    let (ok, state) = doctor_state(&dir, global_hooks_path(&[]).as_deref(), &config_ljos_dir());
    crate::Habitat {
        name: "git hooks",
        state,
        ok,
    }
}

/// [`doctor_row`] for a chosen hooks directory, the global `core.hooksPath`,
/// and the config directory that holds env files.
#[must_use]
pub fn doctor_state(dir: &Path, configured: Option<&str>, config_dir: &Path) -> (bool, String) {
    let (mut ok, mut state) = hooks_state(dir, configured);
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

/// [`doctor_row`]'s verdict for the onboard hooks at `ours` and the global
/// `core.hooksPath` git reports.
#[must_use]
pub fn hooks_state(ours: &Path, configured: Option<&str>) -> (bool, String) {
    let Some(path) = nonempty(configured) else {
        return (
            false,
            format!("{HOOKS_UNSET}, so commits are not booked; {HOOKS_FIX}"),
        );
    };
    let dir = Path::new(path);
    if hooks_reachable(dir, ours) {
        let state = if same_dir(dir, ours) {
            format!("core.hooksPath is {path}; commits are booked")
        } else {
            format!("core.hooksPath is {path}; ljos git-hook runs from those hooks")
        };
        return (true, state);
    }
    let state = if same_dir(dir, ours) {
        format!("core.hooksPath is {path}, but some hook stubs are missing there; {HOOKS_FIX}")
    } else {
        format!("core.hooksPath is {path}, and ljos is not chained there; {HOOKS_FIX}")
    };
    (false, state)
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

const BLOCK_BEGIN: &str = "# ljos:begin git-hook";
const BLOCK_END: &str = "# ljos:end git-hook";

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|path| !path.is_empty())
}

fn ljos_executable() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "ljos".to_string())
}

/// The block appended to a hook. `post-commit` and `post-rewrite` book
/// through `ljos git-hook`, and the `pre-commit` and `pre-push` blocks
/// fail the hook when it refuses. The absolute path is the fallback when
/// `ljos` is not on `PATH`; a `pre-commit` or `pre-push` with neither
/// runs nothing. `post-rewrite` and `pre-push` copy stdin when the caller
/// has not, so this block and a chained hook can both read the shas and
/// refs.
fn hook_block(name: &str) -> String {
    let bin = shell_quote(&ljos_executable());
    let (otherwise, fail) = if gates_git(name) {
        (format!("elif [ -x {bin} ]; then"), "exit 1")
    } else {
        ("else".to_string(), "true")
    };
    if matches!(name, "post-rewrite" | "pre-push") {
        return format!(
            "{BLOCK_BEGIN}\n\
             if [ -z \"${{_ljos_in:-}}\" ]; then\n\
             \t_ljos_in=$(mktemp 2>/dev/null || true)\n\
             \tif [ -n \"$_ljos_in\" ]; then\n\
             \t\tcat > \"$_ljos_in\" || true\n\
             \tfi\n\
             fi\n\
             if command -v ljos >/dev/null 2>&1; then\n\
             \tif [ -n \"${{_ljos_in:-}}\" ]; then\n\
             \t\tljos git-hook {name} \"$@\" < \"$_ljos_in\" >&2 || {fail}\n\
             \telse\n\
             \t\tljos git-hook {name} \"$@\" >&2 || {fail}\n\
             \tfi\n\
             {otherwise}\n\
             \tif [ -n \"${{_ljos_in:-}}\" ]; then\n\
             \t\t{bin} git-hook {name} \"$@\" < \"$_ljos_in\" >&2 || {fail}\n\
             \telse\n\
             \t\t{bin} git-hook {name} \"$@\" >&2 || {fail}\n\
             \tfi\n\
             fi\n\
             {BLOCK_END}\n"
        );
    }
    format!(
        "{BLOCK_BEGIN}\n\
         if command -v ljos >/dev/null 2>&1; then\n\
         \tljos git-hook {name} \"$@\" >&2 || {fail}\n\
         {otherwise}\n\
         \t{bin} git-hook {name} \"$@\" >&2 || {fail}\n\
         fi\n\
         {BLOCK_END}\n"
    )
}

fn has_block(text: &str, name: &str) -> bool {
    text.contains(&hook_block(name))
}

fn strip_block(text: &str) -> String {
    let Some(start) = text.find(BLOCK_BEGIN) else {
        return text.to_string();
    };
    let Some(rel) = text[start..].find(BLOCK_END) else {
        return text.to_string();
    };
    let mut end = start + rel + BLOCK_END.len();
    if text[end..].starts_with('\n') {
        end += 1;
    }
    let mut out = String::new();
    out.push_str(&text[..start]);
    out.push_str(&text[end..]);
    out
}

fn strip_all(text: &str) -> String {
    let mut out = text.to_string();
    loop {
        let next = strip_block(&out);
        if next == out {
            return out;
        }
        out = next;
    }
}

fn merge_block(text: &str, name: &str) -> String {
    if has_block(text, name) {
        return text.to_string();
    }
    let stripped = strip_all(text);
    let block = hook_block(name);
    if stripped.trim().is_empty() {
        return format!("#!/bin/sh\n{block}");
    }
    if stripped.ends_with('\n') {
        format!("{stripped}{block}")
    } else {
        format!("{stripped}\n{block}")
    }
}

fn block_only(text: &str) -> bool {
    matches!(text.trim(), "" | "#!/bin/sh")
}

fn hook_calls_ljos(text: &str, name: &str) -> bool {
    text.contains(&format!("ljos git-hook {name} "))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn ensure_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(path).with_context(|| format!("git-hooks: {}", path.display()))?;
        let mode = meta.permissions().mode();
        if mode & 0o111 == 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o111))?;
        }
    }
    Ok(())
}

fn owned_installed(dir: &Path) -> bool {
    HOOK_NAMES.iter().all(|name| {
        let path = dir.join(name);
        let Ok(text) = fs::read_to_string(&path) else {
            return false;
        };
        is_executable(&path) && (text.contains("#!/bin/sh") || hook_calls_ljos(&text, name))
    })
}

fn user_chained(dir: &Path) -> bool {
    HOOK_NAMES.iter().all(|name| {
        let path = dir.join(name);
        let Ok(text) = fs::read_to_string(&path) else {
            return false;
        };
        is_executable(&path) && hook_calls_ljos(&text, name)
    })
}

fn hooks_reachable(configured: &Path, ours: &Path) -> bool {
    if same_dir(configured, ours) {
        owned_installed(configured)
    } else {
        user_chained(configured)
    }
}

fn write_owned_hooks(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("git-hooks: {}", dir.display()))?;
    for name in HOOK_NAMES {
        let path = dir.join(name);
        fs::write(&path, hook_body(name))
            .with_context(|| format!("git-hooks: {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

fn chain_user_hooks(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("git-hooks: {}", dir.display()))?;
    for name in HOOK_NAMES {
        let path = dir.join(name);
        let existing = if path.is_file() {
            Some(fs::read(&path).with_context(|| format!("git-hooks: {}", path.display()))?)
        } else {
            None
        };
        if let Some(bytes) = &existing {
            if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
                eprintln!("ljos: {} is not a shell script; left it", path.display());
                continue;
            }
        }
        let text = existing
            .as_deref()
            .map(|bytes| std::str::from_utf8(bytes).unwrap_or(""))
            .unwrap_or("");
        let next = merge_block(text, name);
        if existing.is_none() || next != text {
            fs::write(&path, next).with_context(|| format!("git-hooks: {}", path.display()))?;
        }
        ensure_executable(&path)?;
    }
    Ok(())
}

fn strip_user_hooks(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for name in HOOK_NAMES {
        let path = dir.join(name);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if !text.contains(BLOCK_BEGIN) {
            continue;
        }
        let stripped = strip_all(text);
        if block_only(&stripped) {
            fs::remove_file(&path).with_context(|| format!("git-hooks: {}", path.display()))?;
        } else if stripped != text {
            fs::write(&path, stripped).with_context(|| format!("git-hooks: {}", path.display()))?;
        }
    }
    Ok(())
}

fn remove_owned_hooks(dir: &Path) {
    for name in HOOK_NAMES {
        let _ = fs::remove_file(dir.join(name));
    }
    let _ = fs::remove_file(dir.join("previous-hooks-path"));
}

fn hook_body(name: &str) -> String {
    let block = hook_block(name);
    format!(
        "#!/bin/sh\n\
         # ljos books the commit, then chains to this repository's\n\
         # .git/hooks/{name}, or to the core.hooksPath recorded in\n\
         # previous-hooks-path. Booking cannot fail the hook; a refusal\n\
         # from ljos in pre-commit or pre-push does. The chained hook's\n\
         # exit status is kept. post-rewrite and pre-push keep a copy of\n\
         # stdin for that hook.\n\
         #\n\
         # pre-commit install follows core.hooksPath and would replace this\n\
         # file. Install the framework into the repository, then re-run\n\
         # `ljos git-hooks install` if a stub was overwritten:\n\
         #   dir=$(git rev-parse --git-dir)\n\
         #   git config --local core.hooksPath \"$dir/hooks\"\n\
         #   pre-commit install\n\
         #   git config --local --unset core.hooksPath\n\
         {block}\
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

/// Run the ljos part of git hook `name`.
///
/// `post-commit` and `post-rewrite` book the commit. `pre-commit` and
/// `pre-push` run the sitting gate ([`gate`]). Any other name succeeds
/// and does nothing. Arguments git passed the hook are ignored.
///
/// # Errors
///
/// The sitting gate's refusal. Booking that did not land is a line in
/// the book log.
pub fn run_named(name: &str) -> Result<()> {
    let name = name.trim();
    if books_commit(name) {
        run_git_commit(name)
    } else if gates_git(name) {
        gate(name)
    } else {
        Ok(())
    }
}

/// The guard's sitting gate for a commit or push it did not see, such as
/// one a script runs: under a runner, `pre-commit` needs a live sitting,
/// and `pre-push` also needs that ticket to clear each ref it sends onto
/// a branch [`crate::gates::consensus_push`] names. A store or a tracker
/// checkout, which ljos writes itself, is not gated, and nor is the
/// person.
fn gate(name: &str) -> Result<()> {
    if !crate::under_a_runner() {
        return Ok(());
    }
    let repo = git_line(&["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .or_else(git_dir);
    if repo
        .as_deref()
        .is_some_and(|repo| is_store(repo) || holds_tracker(repo))
    {
        return Ok(());
    }
    // In a hook the process tree names the hook's own shell. The seat a
    // booking would take is the one the sitting was taken under. With a
    // conversation id in the environment only that conversation's holder
    // counts, or for a subagent its parent's records and process when
    // that process carries a conversation id. With no id it falls back
    // to the process match.
    let seat = crate::git_hook_seat();
    let _actor = seat.as_deref().map(crate::HookActor::enter);
    let verb = if name == "pre-push" { "push" } else { "commit" };
    let issue = crate::sitting_gate::live_sitting().map_err(|why| {
        anyhow!(
            "sitting-gate: {}",
            crate::sitting_gate::no_sitting(verb, &why)
        )
    })?;
    if name != "pre-push" {
        return Ok(());
    }
    let cfg = crate::gates::consensus_push(repo.as_deref()).map_err(|why| {
        anyhow!(
            "sitting-gate: {}",
            crate::sitting_gate::unread_settings(&why)
        )
    })?;
    let dest = match std::io::read_to_string(std::io::stdin()) {
        Ok(refs) => protected_ref(&refs, &cfg.branches),
        Err(e) => Some(format!("sends refs the hook cannot read ({e})")),
    };
    let Some(dest) = dest else {
        return Ok(());
    };
    crate::protected_push_stands(&issue, cfg.quorum)
        .map(drop)
        .map_err(|why| {
            let wait = crate::sitting_gate::seats_wait(&dest, &cfg.branches, &issue, &why);
            anyhow!("sitting-gate: {wait}")
        })
}

/// What a push whose pre-push stdin is `refs` sends onto a branch in
/// `branches`, as `sends BRANCH`, or `None` when no ref lands on one.
/// Git runs the hook with no lines when nothing is to be sent. A line
/// not in git's four-field form cannot be read, so it is taken to land
/// on one.
fn protected_ref(refs: &str, branches: &[String]) -> Option<String> {
    for line in refs.lines().filter(|l| !l.trim().is_empty()) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [_, _, remote, _] = fields[..] else {
            return Some(format!("lists `{line}`, which is not a ref git would send"));
        };
        let Some(branch) = remote.strip_prefix("refs/heads/") else {
            continue;
        };
        if let Some(b) = branches.iter().find(|b| b.eq_ignore_ascii_case(branch)) {
            return Some(format!("sends `{b}`"));
        }
    }
    None
}

/// Whether `repo` is a tracker checkout: it holds `vissue.toml`, or a
/// `Software/<project>/issues.org` as `vissue` reads a tracker root. A
/// bare `Software/` directory is any repository's.
fn holds_tracker(repo: &Path) -> bool {
    if repo.join("vissue.toml").is_file() {
        return true;
    }
    std::fs::read_dir(repo.join("Software")).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().join("issues.org").is_file())
    })
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
    // A commit in the tracker's checkout is a tracker write: never booked,
    // but its seat is checked as any other write's is.
    let tracker = tracker_repo(&repo_path);
    if !tracker && (crate::bookkeep::path_is_temp(Some(&repo_path)) || skip_repo(&repo_path)) {
        return Ok(());
    }
    let Some(head) = git_line(&["rev-parse", "HEAD"]) else {
        return Ok(());
    };
    // The process tree is not a seat. Without LJOS_SEAT, seats.toml, the
    // grokbot-<id8> fallback, or an agy or opencode conversation, the
    // commit stays unbooked; a subagent is told.
    let Some(seat) = crate::git_hook_seat() else {
        if tracker {
            refuse_tracker_commit(
                &head,
                "no seat is named; export LJOS_SEAT=<your seat> to write the tracker",
            );
        } else if crate::unseated_subagent() {
            // Stdout: the installed hook shows that line and drops stderr.
            println!(
                "ljos: commit {} not booked: this subagent has no seat; export LJOS_SEAT=<your seat> (the parent's) to book it",
                &head[..head.len().min(12)]
            );
        }
        return Ok(());
    };
    let _actor = crate::HookActor::enter(&seat);
    // Every path below writes the tracker: a repointed citation, a named
    // issue, an open sitting, a new claim.
    if let Err(e) = crate::write_seat("commit") {
        if tracker {
            refuse_tracker_commit(&head, &format!("{e:#}"));
        } else {
            println!(
                "ljos: commit {} not booked: {e:#}",
                &head[..head.len().min(12)]
            );
        }
        return Ok(());
    }
    if tracker {
        return Ok(());
    }
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
    // The tool calls before this commit may still have their book queued.
    crate::book_queue::catch_up();
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

/// Whether `repo` is the tracker's checkout, `VISSUE_ROOT` or `ISSUE_ROOT`.
fn tracker_repo(repo: &Path) -> bool {
    ["VISSUE_ROOT", "ISSUE_ROOT"].iter().any(|key| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .is_some_and(|root| useful_root(&root) && covered(repo, &root))
    })
}

/// A tracker commit no seat that may write the tracker made. It has landed
/// and a post-commit hook cannot take it back, so the refusal is said on
/// stdout, which the installed hook shows, and kept in the book log.
fn refuse_tracker_commit(head: &str, why: &str) {
    let line = format!(
        "ljos: tracker commit {} was not made by a seat that may write the tracker: {why}",
        &head[..head.len().min(12)]
    );
    println!("{line}");
    crate::bookkeep::log_line(&format!("git-hooks: {line}"));
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

/// Whether `repo` is a deed or claim store ljos writes itself, read from
/// the store settings; a path that merely contains `claimdag` is not one.
fn is_store(repo: &Path) -> bool {
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

#[cfg(test)]
pub(crate) fn with_hook_children_for_test<T>(run: impl FnOnce() -> T) -> T {
    let _guard = HookChildren::enter();
    run()
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
        let other = dir.path().join("user-hooks");
        std::fs::create_dir_all(&other).unwrap();
        let prior = "#!/bin/sh\nprintf 'user-hook\\n'\n";
        std::fs::write(other.join("post-commit"), prior).unwrap();
        let ours_s = ours.display().to_string();
        let other_s = other.display().to_string();

        let (ok, state) = super::hooks_state(&ours, None);
        assert!(!ok);
        assert!(state.starts_with(super::HOOKS_UNSET), "{state}");
        assert_eq!(state.matches(super::HOOKS_FIX).count(), 1, "{state}");

        let (ok, state) = super::hooks_state(&ours, Some(&other_s));
        assert!(!ok, "{state}");
        assert!(state.contains(&other_s), "{state}");
        assert!(!state.contains(&ours_s), "{state}");
        assert_eq!(state.matches(super::HOOKS_FIX).count(), 1, "{state}");

        super::chain_user_hooks(&other).unwrap();
        let (ok, state) = super::hooks_state(&ours, Some(&other_s));
        assert!(ok, "{state}");
        assert!(state.contains("ljos git-hook runs"), "{state}");
        let hooked = std::fs::read_to_string(other.join("post-commit")).unwrap();
        assert!(hooked.starts_with(prior), "{hooked}");
        assert_eq!(
            hooked.matches("# ljos:begin git-hook").count(),
            1,
            "{hooked}"
        );
        super::chain_user_hooks(&other).unwrap();
        assert_eq!(
            std::fs::read_to_string(other.join("post-commit")).unwrap(),
            hooked
        );
        assert!(other.join("pre-push").is_file());

        super::strip_user_hooks(&other).unwrap();
        assert_eq!(
            std::fs::read_to_string(other.join("post-commit")).unwrap(),
            prior
        );
        assert!(!other.join("pre-push").exists());

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
        let (ok, state) = super::doctor_state(&ours, None, &config);
        assert!(!ok, "{state}");
        assert!(
            !state.starts_with(super::HOOKS_UNSET),
            "a mismatch stayed info: {state}"
        );
        assert!(state.contains(&other_s), "{state}");
        assert!(state.contains(&ours_s), "{state}");

        std::fs::write(
            config.join("env.sh"),
            format!("{}\n", super::HOOKS_ENV_LINE),
        )
        .unwrap();
        assert!(super::env_hooks_drift(&config, &ours).is_none());
        let body = super::hook_body("post-rewrite");
        assert!(body.contains("mktemp"), "{body}");
        assert!(body.contains("git-hook post-rewrite"), "{body}");
        assert!(body.contains("< \"$_ljos_in\""), "{body}");
        let commit = super::hook_body("post-commit");
        assert!(!commit.contains("mktemp"), "{commit}");
        assert!(commit.contains("git-hook post-commit"), "{commit}");

        super::write_owned_hooks(&ours).unwrap();
        let (ok, state) = super::hooks_state(&ours, Some(&ours_s));
        assert!(ok, "{state}");
        std::fs::remove_file(ours.join("pre-push")).unwrap();
        let (ok, state) = super::hooks_state(&ours, Some(&ours_s));
        assert!(!ok, "{state}");
        assert!(state.contains(super::HOOKS_FIX), "{state}");
    }

    #[test]
    fn a_hook_that_does_not_book_succeeds() {
        super::run_named("post-checkout").unwrap();
    }

    #[test]
    fn a_pre_push_ref_list_names_the_protected_branch_it_sends() {
        let branches = vec!["develop".to_string(), "main".to_string()];
        let line = |dst: &str| format!("refs/heads/x 1111 {dst} 0000\n");
        assert_eq!(protected_ref("", &branches), None);
        assert_eq!(protected_ref(&line("refs/heads/topic"), &branches), None);
        assert_eq!(protected_ref(&line("refs/tags/main"), &branches), None);
        let both = line("refs/heads/topic") + &line("refs/heads/Main");
        assert_eq!(
            protected_ref(&both, &branches).as_deref(),
            Some("sends `main`")
        );
        let odd = protected_ref("refs/heads/x main\n", &branches).unwrap();
        assert!(odd.contains("not a ref git would send"), "{odd}");
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
