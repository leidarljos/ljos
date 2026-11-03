//! The push allowlist `ljos cloud up` records.
//!
//! The allowlist is the repository and the branch. A branch name on its
//! own is not enough: `develop` on another repository is not on it. The
//! tracker is the other repository this seat may update, on any branch.
//! `cloud up` records the seat repository once and leaves it there.
//!
//! `cloud up` installs no git hook and sets no `core.hooksPath`, so git
//! runs only the hooks the repository and the login already name.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const ALLOW_NAME: &str = "push-allow";

/// Where the allowlist lives: `$XDG_CONFIG_HOME/ljos`, or `~/.config/ljos`.
fn config_dir() -> Option<PathBuf> {
    crate::config_base().map(|base| base.join("ljos"))
}

pub(crate) fn hooks_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("hooks"))
}

pub(crate) fn allow_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(ALLOW_NAME))
}

/// Record the repositories this process may push. `repoint` replaces a
/// repository recorded by an earlier run. The shell guard refuses that
/// flag.
pub(crate) fn ensure(repoint: bool) -> Result<String> {
    let existing = read_lists();
    let new_key = origin_url().as_deref().and_then(key_or_empty);
    let code_owned = code_keys(&existing, new_key.as_deref(), repoint)?;
    let tracker = std::env::var("LJOS_TRACKER_REMOTE")
        .ok()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty());
    let (branches, closed) = match crate::cloud_guard::seat_allowlist() {
        Ok(set) => {
            let mut branches: Vec<String> = set.into_iter().collect();
            branches.sort();
            (branches, false)
        }
        Err(_) => (Vec::new(), true),
    };
    // A cargo test of this package keeps the workspace checkout out of
    // the seat record, so the guard does not take this checkout's own
    // `.git/config` for the seat's.
    let seat = if invoked_from_package_under_test() {
        existing.seat.clone()
    } else {
        work_tree()
            .map(|path| path.display().to_string())
            .or(existing.seat.clone())
    };
    let home = crate::home()
        .ok()
        .map(|path| path.display().to_string())
        .or(existing.home.clone());
    let code_ref = code_owned.iter().map(String::as_str).collect::<Vec<_>>();
    let tracker_ref = tracker.as_deref().into_iter().collect::<Vec<_>>();
    let branch_ref = branches.iter().map(String::as_str).collect::<Vec<_>>();
    write_lists(
        &code_ref,
        &tracker_ref,
        &branch_ref,
        closed,
        home.as_deref(),
        seat.as_deref(),
    )?;
    let path = allow_path().context("cloud up: no config home for the push allowlist")?;
    Ok(format!(
        "push allowlist: {}; no git hook is installed",
        path.display()
    ))
}

fn code_keys(existing: &StoredLists, new_key: Option<&str>, repoint: bool) -> Result<Vec<String>> {
    let Some(new_key) = new_key else {
        return Ok(existing.code.clone());
    };
    if !repoint && !existing.code.is_empty() && !existing.code.iter().any(|key| key == new_key) {
        let kept = existing
            .code
            .first()
            .map(String::as_str)
            .unwrap_or("another repository");
        bail!("cloud up: the seat repository is already recorded as {kept}; leaving it in place");
    }
    Ok(vec![new_key.to_string()])
}

/// Replace the allowlist's repositories and branches. A repository
/// recorded by `cloud up` is not pinned here: tests and a repaired
/// allowlist rewrite the code lines. The seat and home lines stay.
pub(crate) fn write_push_allowlist(
    code: &[&str],
    tracker: &[&str],
    branches: &[&str],
) -> Result<()> {
    let existing = read_lists();
    let home = crate::home()
        .ok()
        .map(|path| path.display().to_string())
        .or(existing.home);
    write_lists(
        code,
        tracker,
        branches,
        false,
        home.as_deref(),
        existing.seat.as_deref(),
    )
}

/// Whether `url` is a code or tracker repository on the allowlist.
/// `Err` when the allowlist is missing.
pub(crate) fn repo_on_allowlist(url: &str) -> std::result::Result<bool, String> {
    let key = push_repo_key(url);
    if key.is_empty() {
        return Err("the destination repository could not be read".to_string());
    }
    let path = allow_path().ok_or_else(|| "no push allowlist".to_string())?;
    let text = fs::read_to_string(&path).map_err(|_| "no push allowlist".to_string())?;
    Ok(text.lines().any(|line| {
        let Some((tag, val)) = line.split_once('\t') else {
            return false;
        };
        (tag == "code" || tag == "tracker") && val == key
    }))
}

/// The work tree `cloud up` recorded. Absent while a cargo test of this
/// package is still sitting in the workspace.
pub(crate) fn recorded_seat() -> Option<PathBuf> {
    let seat = read_lists().seat?;
    let path = PathBuf::from(seat);
    (!path.as_os_str().is_empty()).then_some(path)
}

/// Whether `dir` is the recorded seat, or a directory inside it.
pub(crate) fn dir_is_recorded_seat(dir: &Path) -> bool {
    let Some(seat) = recorded_seat() else {
        return false;
    };
    let dir = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        let Ok(cwd) = std::env::current_dir() else {
            return false;
        };
        cwd.join(dir)
    };
    let top = work_tree_of(&dir).unwrap_or(dir);
    paths_same(&top, &seat)
}

struct StoredLists {
    code: Vec<String>,
    home: Option<String>,
    seat: Option<String>,
}

fn read_lists() -> StoredLists {
    let mut out = StoredLists {
        code: Vec::new(),
        home: None,
        seat: None,
    };
    let Some(path) = allow_path() else {
        return out;
    };
    let Ok(text) = fs::read_to_string(path) else {
        return out;
    };
    for line in text.lines() {
        let Some((tag, val)) = line.split_once('\t') else {
            continue;
        };
        if val.is_empty() {
            continue;
        }
        match tag {
            "code" => out.code.push(val.to_string()),
            "home" if out.home.is_none() => out.home = Some(val.to_string()),
            "seat" if out.seat.is_none() => out.seat = Some(val.to_string()),
            _ => {}
        }
    }
    out
}

/// Cargo sets `CARGO_MANIFEST_DIR` while it runs this package's tests.
/// The test process's cwd is the workspace, a parent of the manifest, or
/// a directory inside it. A test that `set_current_dir`s into a fixture
/// is a seat and is recorded.
fn invoked_from_package_under_test() -> bool {
    let (Ok(manifest), Ok(cwd)) = (std::env::var("CARGO_MANIFEST_DIR"), std::env::current_dir())
    else {
        return false;
    };
    let manifest = PathBuf::from(manifest);
    cwd == manifest || cwd.starts_with(&manifest) || manifest.starts_with(&cwd)
}

fn work_tree() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| work_tree_of(&cwd))
}

fn work_tree_of(dir: &Path) -> Option<PathBuf> {
    let out = crate::git_cmd()
        .current_dir(dir)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn origin_url() -> Option<String> {
    let out = crate::git_cmd()
        .args(["remote", "get-url", "origin"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8(out.stdout).ok()?;
    let url = url.trim();
    (!url.is_empty()).then(|| url.to_string())
}

fn write_lists(
    code: &[&str],
    tracker: &[&str],
    branches: &[&str],
    closed: bool,
    home: Option<&str>,
    seat: Option<&str>,
) -> Result<()> {
    let path = allow_path().context("cloud up: no config home for the push allowlist")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut code_keys: Vec<String> = code.iter().filter_map(|url| key_or_empty(url)).collect();
    let mut tracker_keys: Vec<String> =
        tracker.iter().filter_map(|url| key_or_empty(url)).collect();
    code_keys.sort();
    code_keys.dedup();
    tracker_keys.sort();
    tracker_keys.dedup();
    let mut branch_names: Vec<&str> = branches
        .iter()
        .copied()
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.contains([' ', '\t', '\n']))
        .collect();
    branch_names.sort_unstable();
    branch_names.dedup();
    let mut body = String::from("# ljos cloud push allowlist\n");
    if let Some(home) = home.filter(|home| !home.is_empty() && !home.contains(['\n', '\t'])) {
        body.push_str("home\t");
        body.push_str(home);
        body.push('\n');
    }
    if let Some(seat) = seat.filter(|seat| !seat.is_empty() && !seat.contains(['\n', '\t'])) {
        body.push_str("seat\t");
        body.push_str(seat);
        body.push('\n');
    }
    for key in &code_keys {
        body.push_str("code\t");
        body.push_str(key);
        body.push('\n');
    }
    for key in &tracker_keys {
        body.push_str("tracker\t");
        body.push_str(key);
        body.push('\n');
    }
    if closed {
        body.push_str("closed\t1\n");
    }
    for name in &branch_names {
        body.push_str("branch\t");
        body.push_str(name);
        body.push('\n');
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("install {}", path.display()))?;
    Ok(())
}

fn key_or_empty(url: &str) -> Option<String> {
    let key = push_repo_key(url);
    (!key.is_empty()).then_some(key)
}

/// The hook directory, the hook file, the allowlist, the git hooks
/// directory ljos installs as `core.hooksPath`, and the gitconfig files
/// that can name that key. A write to one of these disarms the hook. The
/// recorded seat's `.git/config` is included. Any other `.git/config` or
/// `.gitconfig` is matched by name in `names_protected`. The curl and wget
/// configs are included too, with any a curl or wget on the line names: a
/// write to one changes a file the guard already read.
pub(crate) fn protected_write_targets() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = hooks_dir() {
        out.push(dir.join("pre-push"));
        out.push(dir);
    }
    out.extend(git_hooks_dirs());
    if let Some(path) = allow_path() {
        out.push(path);
    }
    if let Some(path) = std::env::var_os("GIT_CONFIG_GLOBAL") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            out.push(path);
        }
    }
    if let Some(path) = std::env::var_os("GIT_CONFIG_SYSTEM") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            out.push(path);
        }
    }
    if let Ok(home) = crate::home() {
        out.push(home.join(".gitconfig"));
        out.push(home.join(".config/git/config"));
        out.push(home.join(".curlrc"));
        out.push(home.join(".config/curlrc"));
        out.push(home.join(".wgetrc"));
    }
    if let Some(own) = crate::proc_tree::own_home() {
        out.push(own.join(".curlrc"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let xdg = PathBuf::from(xdg);
        out.push(xdg.join("git/config"));
        out.push(xdg.join(".curlrc"));
    }
    if let Some(path) = std::env::var_os("CURL_HOME") {
        let path = PathBuf::from(&path);
        if !path.as_os_str().is_empty() {
            out.push(path.join(".curlrc"));
            out.push(path.join(".config/curlrc"));
        }
    }
    for key in ["WGETRC", "SYSTEM_WGETRC"] {
        if let Some(path) = std::env::var_os(key) {
            let path = PathBuf::from(path);
            if !path.as_os_str().is_empty() {
                out.push(path);
            }
        }
    }
    if let Some(seat) = recorded_seat() {
        out.push(seat.join(".git/config"));
    }
    out
}

pub(crate) fn names_protected(target: &str, cwd: Option<&Path>) -> bool {
    let Some(expanded) = expand_target(target) else {
        return false;
    };
    if config_file_name(&expanded) {
        return true;
    }
    let mut candidates = vec![PathBuf::from(&expanded)];
    if let Some(cwd) = cwd {
        if !Path::new(&expanded).is_absolute() {
            candidates.push(cwd.join(&expanded));
        }
    }
    let protected = protected_write_targets();
    for cand in &candidates {
        if config_file_name(&cand.display().to_string()) {
            return true;
        }
        for have in &protected {
            if paths_same(cand, have) || path_under(cand, have) {
                return true;
            }
        }
    }
    false
}

/// Directories a write can replace the git hooks ljos installs.
///
/// `LJOS_GIT_HOOKS` is one of them when it is set. So is
/// `$XDG_DATA_HOME/ljos/git-hooks` when that variable is set, and
/// `~/.local/share/ljos/git-hooks`. The home path stays on the list when
/// `XDG_DATA_HOME` names another directory, because a line can still write
/// the home one. A global `core.hooksPath` is added when it names a
/// `ljos/git-hooks` directory. A path such as `/tmp` or `/dev/null` is not
/// a hooks directory and is left off the list.
pub(crate) fn git_hooks_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push_dir = |path: PathBuf| {
        if path.as_os_str().is_empty() {
            return;
        }
        if out.iter().any(|have| paths_same(have, &path)) {
            return;
        }
        out.push(path);
    };
    if let Some(dir) = std::env::var_os("LJOS_GIT_HOOKS").filter(|value| !value.is_empty()) {
        push_dir(PathBuf::from(dir));
    }
    if let Some(base) = crate::data_base() {
        push_dir(base.join("ljos").join("git-hooks"));
    }
    if let Ok(home) = crate::home() {
        push_dir(home.join(".local/share/ljos/git-hooks"));
    }
    if let Some(dir) = configured_hooks_dir() {
        if ljos_git_hooks_dir(&dir) {
            push_dir(dir);
        }
    }
    out
}

/// Whether `target` is a git hooks directory from [`git_hooks_dirs`], or a
/// path inside one. `~`, `$HOME`, `$XDG_DATA_HOME` and `$LJOS_GIT_HOOKS`
/// are filled in. A different `$` substitution is not a match.
pub(crate) fn in_git_hooks_dir(target: &str, cwd: Option<&Path>) -> bool {
    let Some(expanded) = expand_target(target) else {
        return false;
    };
    path_in_git_hooks(&expanded, cwd)
}

fn path_in_git_hooks(expanded: &str, cwd: Option<&Path>) -> bool {
    let mut candidates = vec![PathBuf::from(expanded)];
    if let Some(cwd) = cwd {
        if !Path::new(expanded).is_absolute() {
            candidates.push(cwd.join(expanded));
        }
    }
    let dirs = git_hooks_dirs();
    candidates.iter().any(|cand| {
        dirs.iter()
            .any(|dir| paths_same(cand, dir) || path_under(cand, dir))
    })
}

fn ljos_git_hooks_dir(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "git-hooks")
        && path.components().any(|part| part.as_os_str() == "ljos")
}

/// Global gitconfig files, highest precedence first. `GIT_CONFIG_GLOBAL`
/// replaces the others.
fn global_gitconfig_files() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("GIT_CONFIG_GLOBAL").filter(|value| !value.is_empty()) {
        return vec![PathBuf::from(path)];
    }
    let mut out = Vec::new();
    if let Ok(home) = crate::home() {
        out.push(home.join(".gitconfig"));
        out.push(home.join(".config/git/config"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(xdg).join("git/config");
        if !out.iter().any(|have| have == &path) {
            out.push(path);
        }
    }
    out
}

fn configured_hooks_dir() -> Option<PathBuf> {
    for file in global_gitconfig_files() {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        if let Some(value) = hooks_path_assignment(&text) {
            return expand_hooks_assignment(&value);
        }
    }
    None
}

fn hooks_path_assignment(text: &str) -> Option<String> {
    let mut in_core = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            in_core = inner.trim().eq_ignore_ascii_case("core");
            continue;
        }
        if !in_core {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("hooksPath") {
            continue;
        }
        let value = value.split('#').next().unwrap_or(value).trim();
        let value = value.trim_matches(|c| c == '"' || c == '\'');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn expand_hooks_assignment(value: &str) -> Option<PathBuf> {
    if value.contains(['$', '`']) {
        return None;
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return Some(crate::home().ok()?.join(rest));
    }
    Path::new(value).is_absolute().then(|| PathBuf::from(value))
}

/// A word that names a protected file, including a path inside a quoted
/// program argument.
pub(crate) fn word_hits_protected(word: &str, cwd: Option<&Path>) -> bool {
    if names_protected(word, cwd) {
        return true;
    }
    if let Some((_, rest)) = word.split_once('=') {
        if names_protected(rest, cwd) {
            return true;
        }
    }
    for part in word.split(|c: char| {
        matches!(
            c,
            '"' | '\'' | ' ' | ',' | '(' | ')' | ';' | '`' | '{' | '}'
        )
    }) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if names_protected(part, cwd) || config_file_name(part) {
            return true;
        }
    }
    for have in protected_write_targets() {
        let text = have.display().to_string();
        if text.len() > 3 && word.contains(&text) {
            return true;
        }
    }
    false
}

fn config_file_name(path: &str) -> bool {
    let path = path.trim().trim_matches(|c| c == '"' || c == '\'');
    let path = path.trim_end_matches([')', ';', ',', '\\']);
    path == ".gitconfig"
        || path.ends_with("/.gitconfig")
        || path == ".git/config"
        || path.ends_with(".git/config")
        || path == ".curlrc"
        || path.ends_with("/.curlrc")
        || path.ends_with("/.config/curlrc")
        || path == ".wgetrc"
        || path.ends_with("/.wgetrc")
}

fn path_under(path: &Path, dir: &Path) -> bool {
    let path = lexical(path);
    let dir = lexical(dir);
    path.starts_with(&dir)
        && path
            .strip_prefix(&dir)
            .is_ok_and(|rest| !rest.as_os_str().is_empty())
}

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn paths_same(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => lexical(a) == lexical(b) && a.is_absolute() && b.is_absolute(),
    }
}

/// A path the line writes, with `~` and `$HOME` filled in. `None` when a
/// substitution other than those two hides the path: the guard cannot
/// tell where it lands, and a later push still meets the hook.
fn expand_target(path: &str) -> Option<String> {
    let path = path.trim().trim_matches(|c| c == '"' || c == '\'');
    if path.is_empty() || path.contains('`') {
        return None;
    }
    let home = crate::home().ok();
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        home?.join(rest).display().to_string()
    } else if path == "~" || path == "$HOME" || path == "${HOME}" {
        home?.display().to_string()
    } else if let Some(rest) = strip_home_prefix(path) {
        home?.join(rest).display().to_string()
    } else if let Some(rest) = strip_xdg_prefix(path) {
        let xdg = std::env::var_os("XDG_CONFIG_HOME")?;
        PathBuf::from(xdg).join(rest).display().to_string()
    } else if let Some(rest) = strip_data_prefix(path) {
        let xdg = std::env::var_os("XDG_DATA_HOME")?;
        PathBuf::from(xdg).join(rest).display().to_string()
    } else if let Some(rest) = strip_hooks_prefix(path) {
        let base = std::env::var_os("LJOS_GIT_HOOKS")?;
        if rest.is_empty() {
            PathBuf::from(base).display().to_string()
        } else {
            PathBuf::from(base).join(rest).display().to_string()
        }
    } else if path.contains('$') {
        return None;
    } else {
        path.to_string()
    };
    if expanded.contains(['$', '`']) {
        None
    } else {
        Some(expanded)
    }
}

fn strip_home_prefix(path: &str) -> Option<&str> {
    path.strip_prefix("$HOME/")
        .or_else(|| path.strip_prefix("${HOME}/"))
}

fn strip_xdg_prefix(path: &str) -> Option<&str> {
    path.strip_prefix("$XDG_CONFIG_HOME/")
        .or_else(|| path.strip_prefix("${XDG_CONFIG_HOME}/"))
}

fn strip_data_prefix(path: &str) -> Option<&str> {
    path.strip_prefix("$XDG_DATA_HOME/")
        .or_else(|| path.strip_prefix("${XDG_DATA_HOME}/"))
}

fn strip_hooks_prefix(path: &str) -> Option<&str> {
    if path == "$LJOS_GIT_HOOKS" || path == "${LJOS_GIT_HOOKS}" {
        return Some("");
    }
    path.strip_prefix("$LJOS_GIT_HOOKS/")
        .or_else(|| path.strip_prefix("${LJOS_GIT_HOOKS}/"))
}

/// The repository key the allowlist records. `https`, `ssh`, and `git@host:`
/// forms of one GitHub repository share a key. Userinfo is the last `@`
/// in the authority, before the path, so a token is not part of the key
/// and an `@` in the path is not userinfo.
pub(crate) fn push_repo_key(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Some(rest) = raw.strip_prefix("file://") {
        return local_key(rest);
    }
    if raw.contains("://") {
        return url_key(raw);
    }
    if let Some((left, path)) = raw.split_once(':') {
        return scp_key(left, path);
    }
    // `write_lists` keys every code line, including one that is already
    // `host/path`. A second pass must not join that key onto the current
    // directory.
    if let Some((host, path)) = raw.split_once('/') {
        if !host.is_empty()
            && !host.starts_with('.')
            && (host.contains('.') || host.parse::<std::net::Ipv4Addr>().is_ok())
        {
            return host_path(host, path);
        }
    }
    local_key(raw)
}

fn local_key(path: &str) -> String {
    let path = match path.strip_prefix("~/") {
        Some(rest) => crate::home()
            .map(|home| home.join(rest).display().to_string())
            .unwrap_or_else(|_| path.to_string()),
        None if path == "~" => crate::home()
            .map(|home| home.display().to_string())
            .unwrap_or_else(|_| path.to_string()),
        None => path.to_string(),
    };
    let absolute = if Path::new(&path).is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path).display().to_string())
            .unwrap_or(path)
    };
    let canon = fs::canonicalize(&absolute)
        .map(|p| p.display().to_string())
        .unwrap_or(absolute);
    strip_git(&canon)
}

fn url_key(raw: &str) -> String {
    let rest = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    let (auth, path) = rest.split_once('/').unwrap_or((rest, ""));
    let auth = auth.rsplit_once('@').map(|(_, host)| host).unwrap_or(auth);
    host_path(auth, path)
}

fn scp_key(left: &str, path: &str) -> String {
    if path.starts_with('/') {
        return local_key(path);
    }
    let host = left.rsplit_once('@').map(|(_, host)| host).unwrap_or(left);
    host_path(host, path)
}

fn host_path(host: &str, path: &str) -> String {
    let host = host.split(':').next().unwrap_or(host);
    let path = strip_git(path.trim_end_matches('/'));
    let key = if path.is_empty() {
        host.to_string()
    } else {
        format!("{host}/{path}")
    };
    key.to_ascii_lowercase()
}

fn strip_git(path: &str) -> String {
    let path = path.trim_end_matches('/');
    path.strip_suffix(".git").unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_forms_of_one_repository_share_a_key() {
        let want = "github.com/leidarljos/ljos";
        for url in [
            "https://github.com/leidarljos/ljos.git",
            "https://github.com/leidarljos/ljos",
            "https://github.com/Leidarljos/Ljos.git",
            "https://x-access-token:secret@github.com/leidarljos/ljos.git",
            "git@github.com:leidarljos/ljos.git",
            "ssh://git@github.com/leidarljos/ljos.git",
            "https://github.com:443/leidarljos/ljos.git/",
        ] {
            assert_eq!(push_repo_key(url), want, "{url}");
            assert_eq!(push_repo_key(&push_repo_key(url)), want, "{url}");
        }
        assert_ne!(push_repo_key("https://github.com/other/ljos.git"), want);
    }

    #[test]
    fn a_local_path_and_its_file_url_share_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        fs::create_dir(&origin).unwrap();
        let path = origin.display().to_string();
        let file = format!("file://{path}");
        assert_eq!(push_repo_key(&path), push_repo_key(&file));
        assert!(!push_repo_key(&path).ends_with(".git"));
    }

    #[test]
    fn an_at_in_the_path_is_not_userinfo() {
        let url = "git://127.0.0.1:19418/x@/home/ubuntu/seat/seat.git";
        let want = "127.0.0.1/x@/home/ubuntu/seat/seat";
        assert_eq!(push_repo_key(url), want);
        assert_ne!(
            push_repo_key(url),
            push_repo_key("/home/ubuntu/seat/seat.git")
        );
    }

    #[test]
    fn a_core_hooks_path_assignment_is_read_from_that_section() {
        let text = "\
[user]
    name = someone
[core]
    hooksPath = ~/ljos/git-hooks # kept
[core \"other\"]
    hooksPath = /tmp/no
";
        assert_eq!(
            hooks_path_assignment(text).as_deref(),
            Some("~/ljos/git-hooks")
        );
        assert!(hooks_path_assignment("[core \"ssh\"]\nhooksPath = /tmp/no\n").is_none());
        assert!(hooks_path_assignment("[user]\nname = x\n").is_none());
    }
}
