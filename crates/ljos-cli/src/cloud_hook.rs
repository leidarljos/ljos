//! The pre-push hook `ljos cloud up` installs.
//!
//! Git runs it for every push, however the push was started, and passes
//! the remote URL after `insteadOf`. The allowlist is the repository and
//! the branch. A branch name on its own is not enough: `develop` on
//! another repository is refused, and so is a tag, a delete, or a push
//! that is not a fast-forward. The tracker is the other repository this
//! seat may update, on any branch, still without a force or a delete.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// The hook git runs. The bytes on disk have to match this; `ljos doctor`
/// treats any other text as a hook that was replaced.
pub(crate) const HOOK_SCRIPT: &str = include_str!("cloud_pre_push.sh");

const ALLOW_NAME: &str = "push-allow";

/// Where the hook and its allowlist live: `$XDG_CONFIG_HOME/ljos`, or
/// `~/.config/ljos`.
fn config_dir() -> Option<PathBuf> {
    crate::config_base().map(|base| base.join("ljos"))
}

pub(crate) fn hooks_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("hooks"))
}

fn allow_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(ALLOW_NAME))
}

/// Install the hook, record the repositories this process may push, and
/// point the global `core.hooksPath` at the hook directory.
pub(crate) fn ensure() -> Result<String> {
    let dir = install_script()?;
    let code = origin_url();
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
    let code_ref = code.as_deref().into_iter().collect::<Vec<_>>();
    let tracker_ref = tracker.as_deref().into_iter().collect::<Vec<_>>();
    let branch_ref = branches.iter().map(String::as_str).collect::<Vec<_>>();
    write_lists(&code_ref, &tracker_ref, &branch_ref, closed)?;
    point_hooks(&dir)?;
    Ok(format!("hook: {} is the pre-push allowlist", dir.display()))
}

/// Replace the allowlist. `code` and `tracker` are remote URLs as git
/// would be given them. Branch names are the short names (`develop`, not
/// `refs/heads/develop`).
pub(crate) fn write_push_allowlist(
    code: &[&str],
    tracker: &[&str],
    branches: &[&str],
) -> Result<()> {
    write_lists(code, tracker, branches, false)
}

fn install_script() -> Result<PathBuf> {
    let dir = hooks_dir().context("cloud up: no config home for the pre-push hook")?;
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join("pre-push");
    // `Path::is_file` follows a symlink. A symlink to a script is not the
    // hook this binary contains, so replace it.
    if let Ok(meta) = path.symlink_metadata() {
        if !meta.file_type().is_file() {
            let removed = if meta.file_type().is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            removed.with_context(|| format!("remove {}", path.display()))?;
        }
    }
    let tmp = dir.join(".pre-push.tmp");
    {
        let mut file =
            fs::File::create(&tmp).with_context(|| format!("write {}", tmp.display()))?;
        file.write_all(HOOK_SCRIPT.as_bytes())?;
    }
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    fs::rename(&tmp, &path).with_context(|| format!("install {}", path.display()))?;
    Ok(dir)
}

fn point_hooks(dir: &Path) -> Result<()> {
    let value = dir.display().to_string();
    let out = Command::new("git")
        .args(["config", "--global", "core.hooksPath", &value])
        .stdin(std::process::Stdio::null())
        .output()
        .context("git config core.hooksPath")?;
    if out.status.success() {
        return Ok(());
    }
    bail!(
        "git config core.hooksPath: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
}

fn origin_url() -> Option<String> {
    let out = Command::new("git")
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

fn write_lists(code: &[&str], tracker: &[&str], branches: &[&str], closed: bool) -> Result<()> {
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

/// What `ljos doctor` adds to the cloud-guard row when the hook is the
/// one `cloud up` installs. `Err` is the reason it is missing or changed.
pub(crate) fn hook_line() -> std::result::Result<String, String> {
    let dir = hooks_dir().ok_or_else(|| "pre-push hook missing; no config home".to_string())?;
    let path = dir.join("pre-push");
    let meta = fs::symlink_metadata(&path).map_err(|_| {
        format!(
            "pre-push hook missing at {}; run `ljos cloud up`",
            path.display()
        )
    })?;
    if !meta.file_type().is_file() {
        return Err(format!(
            "pre-push hook at {} changed; run `ljos cloud up`",
            path.display()
        ));
    }
    let text = fs::read(&path).map_err(|_| {
        format!(
            "pre-push hook at {} changed; run `ljos cloud up`",
            path.display()
        )
    })?;
    if text != HOOK_SCRIPT.as_bytes() {
        return Err(format!(
            "pre-push hook at {} changed; run `ljos cloud up`",
            path.display()
        ));
    }
    let configured = global_hooks_path().map_err(|err| {
        format!("pre-push hook not named by core.hooksPath ({err}); run `ljos cloud up`")
    })?;
    if !paths_same(Path::new(&configured), &dir) {
        return Err(format!(
            "pre-push hook not named by core.hooksPath ({configured}); run `ljos cloud up`"
        ));
    }
    Ok(format!("pre-push hook at {}", path.display()))
}

fn global_hooks_path() -> std::result::Result<String, String> {
    let out = Command::new("git")
        .args(["config", "--global", "--get", "core.hooksPath"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| err.to_string())?;
    if !out.status.success() {
        return Err("unset".to_string());
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if value.is_empty() {
        Err("unset".to_string())
    } else {
        Ok(value)
    }
}

/// The hook directory, the hook file, the allowlist, and the gitconfig
/// files that can name `core.hooksPath`. A write to one of these disarms
/// the hook.
pub(crate) fn protected_write_targets() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = hooks_dir() {
        out.push(dir.join("pre-push"));
        out.push(dir);
    }
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
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        out.push(PathBuf::from(&xdg).join("git/config"));
    }
    out
}

pub(crate) fn names_protected(target: &str, cwd: Option<&Path>) -> bool {
    let Some(expanded) = expand_target(target) else {
        return false;
    };
    let mut candidates = vec![PathBuf::from(&expanded)];
    if let Some(cwd) = cwd {
        if !Path::new(&expanded).is_absolute() {
            candidates.push(cwd.join(&expanded));
        }
    }
    let protected = protected_write_targets();
    for cand in &candidates {
        for have in &protected {
            if paths_same(cand, have) || path_under(cand, have) {
                return true;
            }
        }
    }
    false
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

/// The repository key the hook compares. `https`, `ssh`, and `git@host:`
/// forms of one GitHub repository share a key. Userinfo is dropped, so a
/// token in the URL is not part of the key and is not written down.
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
    let rest = rest.split_once('@').map(|(_, host)| host).unwrap_or(rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    host_path(host, path)
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

    fn shell_key(url: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pre-push");
        fs::write(&script, HOOK_SCRIPT).unwrap();
        let out = Command::new("sh")
            .arg(&script)
            .arg(url)
            .env("LJOS_HOOK_KEY", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "normalize {url}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

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
            assert_eq!(shell_key(url), want, "{url}");
        }
        assert_ne!(push_repo_key("https://github.com/other/ljos.git"), want);
        assert_eq!(
            shell_key("https://github.com/other/ljos.git"),
            push_repo_key("https://github.com/other/ljos.git")
        );
    }

    #[test]
    fn a_local_path_and_its_file_url_share_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        fs::create_dir(&origin).unwrap();
        let path = origin.display().to_string();
        let file = format!("file://{path}");
        assert_eq!(push_repo_key(&path), push_repo_key(&file));
        assert_eq!(shell_key(&path), push_repo_key(&path));
        assert!(!push_repo_key(&path).ends_with(".git"));
    }
}
