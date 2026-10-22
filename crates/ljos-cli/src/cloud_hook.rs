//! The pre-push hook `ljos cloud up` installs.
//!
//! Git runs it for every push that does not pass `--no-verify`, however
//! that push was started, and passes the remote URL after `insteadOf`.
//! The allowlist is the repository and the branch. A branch name on its
//! own is not enough: `develop` on another repository is refused, and so
//! is a tag, a delete, or a push that is not a fast-forward. The tracker
//! is the other repository this seat may update, on any branch, still
//! without a force or a delete.
//!
//! The hook reads the allowlist next to its own directory. The pusher's
//! environment does not choose that path. `cloud up` records the seat
//! repository once and leaves it there.

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

pub(crate) fn allow_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(ALLOW_NAME))
}

/// Install the hook, record the repositories this process may push, and
/// point the global `core.hooksPath` at the hook directory. `repoint`
/// replaces a repository recorded by an earlier run. The shell guard
/// refuses that flag.
pub(crate) fn ensure(repoint: bool) -> Result<String> {
    let dir = install_script()?;
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
    // the seat record. Recording it would make doctor read this repo's
    // own hooksPath, which is not the cloud hook.
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
    let seat_path = if invoked_from_package_under_test() {
        None
    } else {
        seat.as_deref().map(PathBuf::from)
    };
    point_hooks(&dir, seat_path.as_deref())?;
    Ok(format!("hook: {} is the pre-push allowlist", dir.display()))
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
/// `Err` when the allowlist is missing, so a caller that cannot see the
/// hook fails closed.
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
/// is a seat and is recorded. The installed hook never reads this.
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
    let out = Command::new("git")
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
    write_executable(&path, HOOK_SCRIPT)?;
    Ok(dir)
}

fn write_executable(path: &Path, body: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("cloud up: hook path has no directory")?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(".pre-push.tmp");
    {
        let mut file =
            fs::File::create(&tmp).with_context(|| format!("write {}", tmp.display()))?;
        file.write_all(body.as_bytes())?;
    }
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    fs::rename(&tmp, path).with_context(|| format!("install {}", path.display()))?;
    Ok(())
}

fn point_hooks(our_dir: &Path, seat: Option<&Path>) -> Result<()> {
    if let Some(seat) = seat {
        if let Some(local) = local_hooks_path(seat) {
            let local_path = resolve_hooks_value(&local, seat);
            if paths_same(&local_path, our_dir) {
                return Ok(());
            }
            if local_path.is_dir() {
                chain_into(&local_path, our_dir)?;
                return Ok(());
            }
        }
        if let Ok(global) = global_hooks_path() {
            let global_path = PathBuf::from(&global);
            if paths_same(&global_path, our_dir) {
                return set_local(seat, our_dir);
            }
            if global_path.is_dir() {
                chain_into(&global_path, our_dir)?;
                return set_local(seat, &global_path);
            }
        }
        set_local(seat, our_dir)?;
        match global_hooks_path() {
            Err(_) => set_global(our_dir)?,
            Ok(value) => {
                let path = PathBuf::from(&value);
                if !paths_same(&path, our_dir) && !path.is_dir() {
                    set_global(our_dir)?;
                }
            }
        }
        return Ok(());
    }
    match global_hooks_path() {
        Ok(value) => {
            let path = PathBuf::from(&value);
            if paths_same(&path, our_dir) {
                return Ok(());
            }
            if path.is_dir() {
                return chain_into(&path, our_dir);
            }
            set_global(our_dir)
        }
        Err(_) => set_global(our_dir),
    }
}

fn set_global(dir: &Path) -> Result<()> {
    let value = dir.display().to_string();
    git_config(&["config", "--global", "core.hooksPath", &value])
}

fn set_local(seat: &Path, dir: &Path) -> Result<()> {
    let seat_s = seat.display().to_string();
    let value = dir.display().to_string();
    git_config(&["-C", &seat_s, "config", "--local", "core.hooksPath", &value])
}

fn git_config(args: &[&str]) -> Result<()> {
    let out = Command::new("git")
        .args(args)
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

fn local_hooks_path(seat: &Path) -> Option<String> {
    let seat_s = seat.display().to_string();
    let out = Command::new("git")
        .args([
            "-C",
            &seat_s,
            "config",
            "--local",
            "--get",
            "core.hooksPath",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8(out.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
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

/// Run the hook that already occupies `hooks_dir`, then the hook this
/// binary installs. A directory with no pre-push gets this script under
/// the name git already looks up, so the other hook files stay.
fn chain_into(hooks_dir: &Path, our_dir: &Path) -> Result<()> {
    fs::create_dir_all(hooks_dir).with_context(|| format!("create {}", hooks_dir.display()))?;
    let dest = hooks_dir.join("pre-push");
    let canon = canonical_hook_path(our_dir);
    let marker = format!("# ljos-cloud-chain {}", canon.display());
    if let Ok(text) = fs::read_to_string(&dest) {
        if text.lines().any(|line| line == marker) || text.as_bytes() == HOOK_SCRIPT.as_bytes() {
            return Ok(());
        }
    } else if !dest.exists() {
        return write_executable(&dest, HOOK_SCRIPT);
    }
    let prev = hooks_dir.join("pre-push.ljos-previous");
    if !prev.exists() {
        fs::rename(&dest, &prev).with_context(|| format!("keep {}", dest.display()))?;
    }
    write_executable(&dest, &chain_script(&prev, &canon, &marker))
}

fn chain_script(prev: &Path, canon: &Path, marker: &str) -> String {
    let prev_q = crate::tools::sh_quote(&prev.display().to_string());
    let canon_q = crate::tools::sh_quote(&canon.display().to_string());
    format!(
        r#"#!/bin/sh
{marker}
PATH=/usr/bin:/bin
export PATH
unset IFS
set -eu
prev={prev_q}
canon={canon_q}
tmp=$(/usr/bin/mktemp -p /tmp)
trap '/bin/rm -f "$tmp"' EXIT
/bin/cat > "$tmp"
"$prev" "${{1-}}" "${{2-}}" < "$tmp"
"$canon" "${{1-}}" "${{2-}}" < "$tmp"
"#
    )
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
    effective_hook_ok(&dir)?;
    Ok(format!("pre-push hook at {}", path.display()))
}

fn effective_hook_ok(our_dir: &Path) -> std::result::Result<(), String> {
    let (value, base) = effective_hooks_value()?;
    let resolved = resolve_hooks_value(&value, &base);
    if paths_same(&resolved, our_dir) {
        return Ok(());
    }
    let pre = resolved.join("pre-push");
    let text = fs::read(&pre).map_err(|_| {
        format!("pre-push hook not named by core.hooksPath ({value}); run `ljos cloud up`")
    })?;
    if text == HOOK_SCRIPT.as_bytes() {
        return Ok(());
    }
    let marker = format!(
        "# ljos-cloud-chain {}",
        canonical_hook_path(our_dir).display()
    );
    let body = String::from_utf8_lossy(&text);
    if body.lines().any(|line| line == marker) {
        return Ok(());
    }
    Err(format!(
        "pre-push hook not named by core.hooksPath ({value}); run `ljos cloud up`"
    ))
}

/// The hooks directory git would use for the recorded seat, or, when no
/// seat is recorded, the value visible outside this process's checkout.
fn effective_hooks_value() -> std::result::Result<(String, PathBuf), String> {
    let seat = recorded_seat().filter(|path| path.is_dir());
    let probe = if seat.is_none() {
        Some(probe_dir()?)
    } else {
        None
    };
    let mut cmd = Command::new("git");
    cmd.args(["config", "--show-origin", "--get", "core.hooksPath"]);
    cmd.stdin(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let base = if let Some(seat) = &seat {
        cmd.current_dir(seat);
        seat.clone()
    } else {
        let probe = probe.clone().expect("probe");
        cmd.current_dir(&probe);
        probe
    };
    let out = cmd.output().map_err(|err| err.to_string())?;
    if let Some(probe) = &probe {
        let _ = fs::remove_dir_all(probe);
    }
    if !out.status.success() {
        return Err("unset".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let line = text.lines().next().unwrap_or("").trim();
    let value = line
        .split_once('\t')
        .map(|(_, value)| value.trim())
        .unwrap_or(line);
    if value.is_empty() {
        Err("unset".to_string())
    } else {
        Ok((value.to_string(), base))
    }
}

fn probe_dir() -> std::result::Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path =
        std::env::temp_dir().join(format!("ljos-hooks-probe-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&path).map_err(|err| err.to_string())?;
    Ok(path)
}

fn resolve_hooks_value(value: &str, base: &Path) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn canonical_hook_path(our_dir: &Path) -> PathBuf {
    let path = our_dir.join("pre-push");
    fs::canonicalize(&path).unwrap_or(path)
}

thread_local! {
    /// Config files a curl or wget on the line being judged names.
    static LINE_CONFIGS: std::cell::RefCell<Vec<PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Judge a line with the config files it names protected, so a write on
/// the same line cannot change one between the guard's read and the
/// client's.
pub(crate) fn with_line_configs<T>(named: Vec<PathBuf>, judge: impl FnOnce() -> T) -> T {
    let before = LINE_CONFIGS.with(|cell| cell.replace(named));
    let out = judge();
    LINE_CONFIGS.with(|cell| cell.replace(before));
    out
}

/// The hook directory, the hook file, the allowlist, and the gitconfig
/// files that can name `core.hooksPath`. A write to one of these disarms
/// the hook. The recorded seat's `.git/config` is included. Any other
/// `.git/config` or `.gitconfig` is matched by name in `names_protected`.
/// The curl and wget configs are included too, with any a curl or wget on
/// the line names: a write to one changes a file the guard already read.
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
    LINE_CONFIGS.with(|named| out.extend(named.borrow().iter().cloned()));
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

    fn shell_key(url: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pre-push");
        let prefix = HOOK_SCRIPT
            .split_once("# ljos-push-gate\n")
            .expect("gate marker")
            .0;
        let body = format!("{prefix}# ljos-push-gate\nnormalize \"$1\"\n");
        fs::write(&script, body).unwrap();
        let out = Command::new("sh").arg(&script).arg(url).output().unwrap();
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
            assert_eq!(push_repo_key(&push_repo_key(url)), want, "{url}");
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

    #[test]
    fn an_at_in_the_path_is_not_userinfo() {
        let url = "git://127.0.0.1:19418/x@/home/ubuntu/seat/seat.git";
        let want = "127.0.0.1/x@/home/ubuntu/seat/seat";
        assert_eq!(push_repo_key(url), want);
        assert_eq!(shell_key(url), want);
        assert_ne!(
            push_repo_key(url),
            push_repo_key("/home/ubuntu/seat/seat.git")
        );
    }

    #[test]
    fn the_shipped_hook_has_no_environment_switch() {
        assert!(!HOOK_SCRIPT.contains("LJOS_HOOK_KEY"));
        assert!(!HOOK_SCRIPT.contains("XDG_CONFIG_HOME"));
        assert!(!HOOK_SCRIPT.contains("$HOME"));
        assert!(HOOK_SCRIPT.contains("# ljos-push-gate\n"));
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pre-push");
        fs::write(&script, HOOK_SCRIPT).unwrap();
        let out = Command::new("sh")
            .arg(&script)
            .arg("origin")
            .arg("https://example.invalid/a.git")
            .env("LJOS_HOOK_KEY", "1")
            .env("XDG_CONFIG_HOME", dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "the gate did not run: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("pre-push:"), "{err}");
    }
}
