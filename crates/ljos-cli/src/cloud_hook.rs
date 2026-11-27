//! The push allowlist `ljos cloud up` records.
//!
//! The allowlist is the repository and the branch. A branch name on its
//! own is not enough: `develop` on another repository is not on it. The
//! tracker is the other repository this seat may update, on any branch.
//! `cloud up` records the seat repository once and leaves it there.
//!
//! `cloud up` installs no git hook and sets no `core.hooksPath`, so git
//! runs only the hooks the repository and the login already name. It
//! takes back the hook and the hooks paths an earlier `cloud up` set.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const ALLOW_NAME: &str = "push-allow";

/// The first two lines of the pre-push script an earlier `cloud up` wrote
/// to [`hooks_dir`], and to a booked hooks directory that had no
/// pre-push. Every version of that script in this history opens with them.
const OLD_HOOK_HEAD: &str =
    "#!/bin/sh\n# Destination allowlist a cloud seat's git runs on every push.\n";

/// The second line of the stub an earlier `cloud up` wrote over a booked
/// pre-push, and the name it moved that hook to.
const CHAIN_MARKER: &str = "# ljos-cloud-chain ";
const CHAIN_PREVIOUS: &str = "pre-push.ljos-previous";

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
    let mut lines = vec![format!(
        "push allowlist: {}; no git hook is installed",
        path.display()
    )];
    let seat_path = seat
        .as_deref()
        .filter(|_| !invoked_from_package_under_test())
        .map(Path::new);
    lines.extend(retire_old_hooks(seat_path));
    Ok(lines.join("\n"))
}

/// Take back what an earlier `cloud up` set: a global or seat-local
/// `core.hooksPath` that names [`hooks_dir`], the pre-push script there,
/// and the stub it wrote into a hooks directory a path already named.
/// Any other value or file stays as it is. One line per step taken.
fn retire_old_hooks(seat: Option<&Path>) -> Vec<String> {
    let Some(ours) = hooks_dir() else {
        return Vec::new();
    };
    let mut said = Vec::new();
    let mut booked = Vec::new();
    for (scope, at) in [("global", None), ("local", seat)] {
        if scope == "local" && at.is_none() {
            continue;
        }
        for value in hooks_path_values(scope, at) {
            let dir = resolve_hooks_value(&value, at);
            if !paths_same(&dir, &ours) {
                booked.push(dir);
            } else if unset_hooks_value(scope, at, &value) {
                said.push(format!(
                    "hooks: unset the {scope} core.hooksPath {value} an earlier cloud up set"
                ));
            }
        }
    }
    for dir in booked {
        said.extend(unchain(&dir));
    }
    let script = ours.join("pre-push");
    let removed = {
        let _lock = crate::exe_publish::HeldDirectory::open(&ours).ok();
        old_hook_script(&script) && fs::remove_file(&script).is_ok()
    };
    if removed {
        said.push(format!(
            "hooks: removed {}, which an earlier cloud up installed",
            script.display()
        ));
    }
    let _ = fs::remove_dir(&ours);
    said
}

fn hooks_path_values(scope: &str, seat: Option<&Path>) -> Vec<String> {
    let mut cmd = crate::git_cmd();
    if let Some(seat) = seat {
        cmd.arg("-C").arg(seat);
    }
    let Ok(out) = cmd
        .args([
            "config",
            &format!("--{scope}"),
            "--get-all",
            "core.hooksPath",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

/// git reads a relative `core.hooksPath` from the top of the work tree.
fn resolve_hooks_value(value: &str, seat: Option<&Path>) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Ok(home) = crate::home() {
            return home.join(rest);
        }
    }
    match seat {
        Some(seat) if Path::new(value).is_relative() => seat.join(value),
        _ => PathBuf::from(value),
    }
}

fn unset_hooks_value(scope: &str, seat: Option<&Path>, value: &str) -> bool {
    let mut cmd = crate::git_cmd();
    if let Some(seat) = seat {
        cmd.arg("-C").arg(seat);
    }
    cmd.args([
        "config",
        &format!("--{scope}"),
        "--fixed-value",
        "--unset-all",
        "core.hooksPath",
        value,
    ])
    .stdin(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .status()
    .is_ok_and(|status| status.success())
}

fn old_hook_script(path: &Path) -> bool {
    if !path.symlink_metadata().is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    fs::read_to_string(path).is_ok_and(|text| text.starts_with(OLD_HOOK_HEAD))
}

/// The stub ran the hook it moved aside, then the old script. Putting
/// that hook back under its own name is what the directory held before.
fn unchain(dir: &Path) -> Option<String> {
    let _lock = crate::exe_publish::HeldDirectory::open(dir).ok()?;
    let hook = dir.join("pre-push");
    if old_hook_script(&hook) {
        fs::remove_file(&hook).ok()?;
        return Some(format!(
            "hooks: removed {}, the copy an earlier cloud up wrote",
            hook.display()
        ));
    }
    let text = fs::read_to_string(&hook).ok()?;
    let mut lines = text.lines();
    let stub = lines.next().is_some_and(|line| line.starts_with("#!"))
        && lines
            .next()
            .is_some_and(|line| line.starts_with(CHAIN_MARKER))
        && text.lines().last() == Some(r#""$canon" "${1-}" "${2-}" < "$tmp""#);
    if !stub {
        return None;
    }
    let previous = dir.join(CHAIN_PREVIOUS);
    if previous.is_file() {
        #[expect(
            clippy::disallowed_methods,
            reason = "puts back the user's hook an earlier cloud up moved aside; no descriptor here wrote it"
        )]
        fs::rename(&previous, &hook).ok()?;
        return Some(format!(
            "hooks: put {} back as {}, the hook an earlier cloud up chained",
            previous.display(),
            hook.display()
        ));
    }
    fs::remove_file(&hook).ok()?;
    Some(format!(
        "hooks: removed the chain stub {} an earlier cloud up wrote",
        hook.display()
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
        let Ok(cwd) = crate::process_cwd() else {
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
    let (Ok(manifest), Ok(cwd)) = (std::env::var("CARGO_MANIFEST_DIR"), crate::process_cwd())
    else {
        return false;
    };
    let manifest = PathBuf::from(manifest);
    cwd == manifest || cwd.starts_with(&manifest) || manifest.starts_with(&cwd)
}

fn work_tree() -> Option<PathBuf> {
    crate::process_cwd().ok().and_then(|cwd| work_tree_of(&cwd))
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
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
    #[expect(
        clippy::disallowed_methods,
        reason = "renames a path that is not a git hook"
    )]
    fs::rename(&tmp, &path).with_context(|| format!("install {}", path.display()))?;
    Ok(())
}

fn key_or_empty(url: &str) -> Option<String> {
    let key = push_repo_key(url);
    (!key.is_empty()).then_some(key)
}

/// The allowlist and the gitconfig files that can name `core.hooksPath`.
/// A write to one of these changes which hooks git runs, and the shell
/// guard labels it. The recorded seat's `.git/config` is included. Any
/// other `.git/config` or `.gitconfig` is matched by name in
/// `names_protected`. The curl and wget configs are included too, with any
/// a curl or wget on the line names: a write to one changes a file the
/// guard already read. Curl's own files are [`curl_config_files`]. A write
/// inside a git hooks directory is the seat guard's `writes-seat-file`, so
/// it is not listed here.
pub(crate) fn protected_write_targets() -> Vec<PathBuf> {
    let mut out = Vec::new();
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
        out.push(home.join(".wgetrc"));
    }
    // An empty value is unset, as `env_dir` skips it. Git then reads
    // `$HOME/.config/git/config`, which is already listed, and a relative
    // `git/config` is not added.
    if let Some(xdg) = env_dir("XDG_CONFIG_HOME") {
        out.push(xdg.join("git/config"));
    }
    out.extend(curl_config_files());
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

/// Default curl configs any curl from 7.x through 8.23 opens.
///
/// `src/tool_homedir.c` through 7.80.0 returns one directory and
/// `src/tool_parsecfg.c` opens the config there. The directory is
/// `$CURL_HOME`, then `$XDG_CONFIG_HOME` when `.curlrc` is present
/// (7.73.0, `4be1f8dc01013e4dee2b99026cd3b806ea7253c4`), then `$HOME`,
/// then `getpwuid`, then on Windows `%APPDATA%` and
/// `%USERPROFILE%\Application Data`. The file is `.curlrc`. Through
/// 7.65, `DOT_CHAR` in `lib/curl_setup.h` is `_` under `WIN32` and
/// under `MSDOS`, and `src/tool_parsecfg.c` opens `DOT_CHAR "curlrc"`.
/// The executable-directory search is `WIN32` only, so MS-DOS opens
/// the home `_curlrc` and not one beside the executable. ljos has no
/// DOS target, and that home path stays on this list. From 7.66.0
/// Windows opens `.curlrc` and then `_curlrc`
/// (`862393243d16870347e56195e93dc47274e32601`), including beside the
/// curl executable, and every other host asks for `.curlrc`.
/// `src/tool_findfile.c` from 7.81.0
/// (`2be1aa619bca699e2850baf53c7cc3561f1c1ac2`) checks each directory
/// and adds `%USERPROFILE%` on its own. 7.82.0
/// (`764e4f066d5719e68fa0d6b0b0d9efa0625c5c15`) also opens
/// `$CURL_HOME/.config/curlrc`, or `$HOME/.config/curlrc` when `CURL_HOME`
/// is unset, and only when `XDG_CONFIG_HOME` is unset. 8.10.0
/// (`26e9d3a896bba8212f7ece8b53ef91397a5088ea`) through 8.23.0 opens
/// `$XDG_CONFIG_HOME/curlrc` with no leading dot. That entry clears
/// `dotscore`, so the directories after it get no `_curlrc` check and
/// both `.config/curlrc` entries are skipped: those files are still read
/// only when `XDG_CONFIG_HOME` is unset (`findfile` in
/// `src/tool_findfile.c` at the `curl-8_23_0` tag). An empty variable is
/// skipped, as `curl_getenv` skips it. [`curl_config_name`] matches
/// `.curlrc` and `_curlrc` in any directory. `_curlrc` is matched on
/// every host, not only where `cfg!(windows)` is set: Windows users run
/// the Linux build under WSL, and Windows `curl.exe` still opens
/// `_curlrc`, including beside its executable. A false positive on that
/// name is cheap. That match is how the executable's own directory is
/// covered.
fn curl_config_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = crate::home()
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
    {
        push_curl_pair(&mut out, &home);
        push_curl_dotconfig(&mut out, &home);
    }
    if let Some(own) = crate::proc_tree::own_home().filter(|path| !path.as_os_str().is_empty()) {
        push_curl_pair(&mut out, &own);
    }
    if let Some(xdg) = env_dir("XDG_CONFIG_HOME") {
        // Dotted from 7.73.0 through 8.9.x, undotted from 8.10.0, and
        // `_curlrc` on Windows from 7.81.0 through 8.9.x.
        out.push(xdg.join(".curlrc"));
        out.push(xdg.join("curlrc"));
        out.push(xdg.join("_curlrc"));
    }
    if let Some(dir) = env_dir("CURL_HOME") {
        push_curl_pair(&mut out, &dir);
        push_curl_dotconfig(&mut out, &dir);
    }
    if let Some(dir) = env_dir("USERPROFILE") {
        push_curl_pair(&mut out, &dir);
        push_curl_pair(&mut out, &dir.join("Application Data"));
    }
    if let Some(dir) = env_dir("APPDATA") {
        push_curl_pair(&mut out, &dir);
    }
    out
}

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn push_curl_pair(out: &mut Vec<PathBuf>, dir: &Path) {
    out.push(dir.join(".curlrc"));
    out.push(dir.join("_curlrc"));
}

fn push_curl_dotconfig(out: &mut Vec<PathBuf>, dir: &Path) {
    out.push(dir.join(".config").join("curlrc"));
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
/// path inside one. `~` and the variables [`expand_target`] fills in are
/// filled in. A different `$` substitution is not a match.
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
        || curl_config_name(path)
        || path == ".wgetrc"
        || path.ends_with("/.wgetrc")
}

/// A curl config named by its file. `.curlrc` is the name
/// `src/tool_parsecfg.c` opens. A Windows curl also opens `_curlrc`,
/// after `.curlrc` and beside the executable
/// (`862393243d16870347e56195e93dc47274e32601`). `CURLRC_DOTSCORE` in
/// `src/tool_findfile.h` is 2 only on Windows (`WIN32` at 7.82.0,
/// `_WIN32` at 8.9.1 and 8.23.0). Curl 7.65 also opens `_curlrc` on
/// MS-DOS: `DOT_CHAR` in `lib/curl_setup.h` is `_` under `MSDOS` as
/// well as `WIN32`, and the executable-directory search is `WIN32`
/// only, so that build opens the home file. The name match applies on
/// every host anyway. A Linux build of ljos under WSL can run Windows
/// `curl.exe`, which still reads `_curlrc`, and `cfg!(windows)` would
/// describe this binary rather than that curl. A false positive is
/// cheap. `curlrc` with
/// no dot is `$XDG_CONFIG_HOME/curlrc` from 8.10.0
/// (`26e9d3a896bba8212f7ece8b53ef91397a5088ea`) and `.config/curlrc` from
/// 7.82.0 (`764e4f066d5719e68fa0d6b0b0d9efa0625c5c15`); those directories
/// are [`curl_config_files`].
fn curl_config_name(path: &str) -> bool {
    path == ".curlrc"
        || path.ends_with("/.curlrc")
        || path.ends_with("\\.curlrc")
        || path == "_curlrc"
        || path.ends_with("/_curlrc")
        || path.ends_with("\\_curlrc")
        || path.ends_with("/.config/curlrc")
        || path.ends_with("\\.config\\curlrc")
        || path.ends_with("/.config\\curlrc")
        || path.ends_with("\\.config/curlrc")
        || path == ".config/curlrc"
        || path == ".config\\curlrc"
        || xdg_undotted_curlrc(path)
}

fn xdg_undotted_curlrc(path: &str) -> bool {
    let Some(xdg) = env_dir("XDG_CONFIG_HOME") else {
        return false;
    };
    Path::new(path) == xdg.join("curlrc")
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

/// A path the line writes, with `~`, `$HOME`, `$XDG_CONFIG_HOME`,
/// `$XDG_DATA_HOME`, `$LJOS_GIT_HOOKS`, `$CURL_HOME`, `$USERPROFILE` and
/// `$APPDATA` filled in. An empty `$XDG_CONFIG_HOME` or `$XDG_DATA_HOME`
/// is unset, as for `$CURL_HOME`, `$USERPROFILE` and `$APPDATA`. `None`
/// when another `$` substitution hides the path: the guard cannot tell
/// where it lands, so it says nothing about that write.
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
        join_env_rest(&home?, rest)
    } else if let Some(rest) = strip_xdg_prefix(path) {
        join_env_rest(&env_dir("XDG_CONFIG_HOME")?, rest)
    } else if let Some(rest) = strip_data_prefix(path) {
        join_env_rest(&env_dir("XDG_DATA_HOME")?, rest)
    } else if let Some(rest) = strip_curl_home_prefix(path) {
        join_env_rest(&env_dir("CURL_HOME")?, rest)
    } else if let Some(rest) = strip_userprofile_prefix(path) {
        join_env_rest(&env_dir("USERPROFILE")?, rest)
    } else if let Some(rest) = strip_appdata_prefix(path) {
        join_env_rest(&env_dir("APPDATA")?, rest)
    } else if let Some(rest) = strip_hooks_prefix(path) {
        let base = std::env::var_os("LJOS_GIT_HOOKS")?;
        if rest.is_empty() {
            PathBuf::from(base).display().to_string()
        } else {
            join_env_rest(&PathBuf::from(base), rest)
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
    strip_var_prefix(path, "$HOME", "${HOME}")
}

fn strip_xdg_prefix(path: &str) -> Option<&str> {
    strip_var_prefix(path, "$XDG_CONFIG_HOME", "${XDG_CONFIG_HOME}")
}

fn strip_curl_home_prefix(path: &str) -> Option<&str> {
    strip_var_prefix(path, "$CURL_HOME", "${CURL_HOME}")
}

fn strip_userprofile_prefix(path: &str) -> Option<&str> {
    strip_var_prefix(path, "$USERPROFILE", "${USERPROFILE}")
}

fn strip_appdata_prefix(path: &str) -> Option<&str> {
    strip_var_prefix(path, "$APPDATA", "${APPDATA}")
}

fn strip_var_prefix<'a>(path: &'a str, bare: &str, braced: &str) -> Option<&'a str> {
    for base in [bare, braced] {
        for sep in ['/', '\\'] {
            let mut prefix = String::with_capacity(base.len() + 1);
            prefix.push_str(base);
            prefix.push(sep);
            if let Some(rest) = path.strip_prefix(&prefix) {
                return Some(rest);
            }
        }
    }
    None
}

/// `rest` may use `\` the way a Windows curl path does. Join keeps `/`
/// so the result matches [`curl_config_files`] on every host.
fn join_env_rest(base: &Path, rest: &str) -> String {
    if rest.contains('\\') {
        base.join(rest.replace('\\', "/")).display().to_string()
    } else {
        base.join(rest).display().to_string()
    }
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
        crate::process_cwd()
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
        let _env_read = crate::test_env_read_lock();
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
        let _env_read = crate::test_env_read_lock();
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
        let _env_read = crate::test_env_read_lock();
        let url = "git://127.0.0.1:19418/x@/tmp/seat/seat.git";
        let want = "127.0.0.1/x@/tmp/seat/seat";
        assert_eq!(push_repo_key(url), want);
        assert_ne!(push_repo_key(url), push_repo_key("/tmp/seat/seat.git"));
    }

    #[test]
    fn a_core_hooks_path_assignment_is_read_from_that_section() {
        let _env_read = crate::test_env_read_lock();
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

    /// Every default config curl 7.x through 8.23 opens is a protected
    /// write. `$XDG_CONFIG_HOME/curlrc` is the file curl 8.10.0 reads and
    /// the list used to omit.
    #[test]
    fn every_curl_config_curl_7_through_8_23_reads_is_a_protected_write() {
        let _env = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let xdg = root.path().join("xdg");
        let curl_home = root.path().join("curl-home");
        let passwd = root.path().join("passwd");
        let profile = root.path().join("profile");
        let appdata = root.path().join("appdata");
        let _own = crate::proc_tree::HoldOwnHome::set(&passwd);
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &xdg);
            std::env::set_var("CURL_HOME", &curl_home);
            std::env::set_var("USERPROFILE", &profile);
            std::env::set_var("APPDATA", &appdata);
        }

        let want = [
            home.join(".curlrc"),
            home.join("_curlrc"),
            home.join(".config").join("curlrc"),
            xdg.join(".curlrc"),
            xdg.join("curlrc"),
            xdg.join("_curlrc"),
            curl_home.join(".curlrc"),
            curl_home.join("_curlrc"),
            curl_home.join(".config").join("curlrc"),
            passwd.join(".curlrc"),
            passwd.join("_curlrc"),
            profile.join(".curlrc"),
            profile.join("_curlrc"),
            profile.join("Application Data").join(".curlrc"),
            profile.join("Application Data").join("_curlrc"),
            appdata.join(".curlrc"),
            appdata.join("_curlrc"),
        ];
        let got = protected_write_targets();
        for path in &want {
            assert!(
                got.iter().any(|have| have == path),
                "missing {}",
                path.display()
            );
        }

        let lines = [
            "touch $XDG_CONFIG_HOME/curlrc",
            "touch $XDG_CONFIG_HOME/.curlrc",
            "touch $XDG_CONFIG_HOME/_curlrc",
            "touch $CURL_HOME/.curlrc",
            "touch $CURL_HOME/_curlrc",
            "touch $CURL_HOME/.config/curlrc",
            "touch $HOME/.curlrc",
            "touch $HOME/_curlrc",
            "touch $HOME/.config/curlrc",
            "touch $APPDATA/.curlrc",
            "touch $APPDATA/_curlrc",
            // `\\` in double quotes is one backslash, so the word is `$APPDATA\_curlrc`.
            r#"touch "$APPDATA\\_curlrc""#,
            "touch $USERPROFILE/.curlrc",
            "touch $USERPROFILE/_curlrc",
            "touch \"$USERPROFILE/Application Data/.curlrc\"",
            "touch \"$USERPROFILE/Application Data/_curlrc\"",
            "touch ~/.curlrc",
            "touch ~/.config/curlrc",
        ];
        for line in lines {
            let (_push, hooks) = crate::cloud_guard::push_and_hooks(line, None);
            assert!(hooks, "{line}");
        }
        for path in [
            passwd.join(".curlrc"),
            passwd.join("_curlrc"),
            xdg.join("curlrc"),
        ] {
            let line = format!("touch {}", path.display());
            let (_push, hooks) = crate::cloud_guard::push_and_hooks(&line, None);
            assert!(hooks, "{line}");
        }
        assert!(names_protected(r"C:\Users\a\.curlrc", None));
        assert!(names_protected(r"C:\Users\a\_curlrc", None));
        assert!(names_protected(
            r"C:\Users\a\Application Data\_curlrc",
            None
        ));
        assert!(!names_protected("/not/the/xdg/curlrc", None));
        let (_push, hooks) =
            crate::cloud_guard::push_and_hooks("cat $XDG_CONFIG_HOME/curlrc", None);
        assert!(!hooks, "a read was taken for a write");
        let (_push, hooks) = crate::cloud_guard::push_and_hooks("touch /not/the/xdg/curlrc", None);
        assert!(
            !hooks,
            "a file named curlrc outside the curl dirs was protected"
        );

        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        for path in curl_config_files() {
            if path.file_name().is_some_and(|name| name == "curlrc") {
                assert!(
                    path.parent().is_some_and(|dir| dir.ends_with(".config")),
                    "{}",
                    path.display()
                );
            }
        }
        assert!(!names_protected("$XDG_CONFIG_HOME/curlrc", None));
    }

    /// `_curlrc` is a curl name on every host. A Linux build under WSL
    /// still sits next to Windows `curl.exe`, and that curl opens the name.
    /// An empty `$XDG_CONFIG_HOME` is unset, so `$XDG_CONFIG_HOME/_curlrc`
    /// is not the relative name `_curlrc` in the working directory. That
    /// relative name would be protected by the every-host match.
    #[test]
    fn an_underscore_curlrc_is_a_curl_name_on_every_platform() {
        let _env = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let dir = root.path().join("bin");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", "");
            std::env::remove_var("CURL_HOME");
        }
        for word in ["$XDG_CONFIG_HOME/_curlrc", r"$XDG_CONFIG_HOME\_curlrc"] {
            assert_eq!(expand_target(word), None, "{word}");
            assert!(!names_protected(word, Some(&dir)), "{word}");
        }
        let xdg = root.path().join("xdg");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &xdg) };
        assert_eq!(
            expand_target("$XDG_CONFIG_HOME/_curlrc"),
            Some(xdg.join("_curlrc").display().to_string())
        );
        assert!(names_protected("$XDG_CONFIG_HOME/_curlrc", None));
        for path in [
            "_curlrc".to_string(),
            dir.join("_curlrc").display().to_string(),
            r"C:\Program Files\curl\_curlrc".to_string(),
        ] {
            assert!(config_file_name(&path), "{path}");
            assert!(names_protected(&path, None), "{path}");
        }
        let line = format!("touch {}", dir.join("_curlrc").display());
        let (_push, hooks) = crate::cloud_guard::push_and_hooks(&line, None);
        assert!(hooks, "{line}");
        let line = format!("touch {}", dir.join(".curlrc").display());
        let (_push, hooks) = crate::cloud_guard::push_and_hooks(&line, None);
        assert!(hooks, "{line}");
    }

    /// An empty `XDG_CONFIG_HOME` is unset to the guard, as it is to curl,
    /// so `$XDG_CONFIG_HOME/curlrc` is not read as `curlrc` in the working
    /// directory.
    #[test]
    fn an_empty_xdg_config_home_is_not_filled_in() {
        let _env = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let dotconfig = home.join(".config");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", "");
            std::env::remove_var("CURL_HOME");
        }
        for word in [
            "$XDG_CONFIG_HOME/curlrc",
            "${XDG_CONFIG_HOME}/curlrc",
            r"$XDG_CONFIG_HOME\curlrc",
        ] {
            assert_eq!(expand_target(word), None, "{word}");
            assert!(!names_protected(word, Some(&dotconfig)), "{word}");
        }

        let xdg = root.path().join("xdg");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &xdg) };
        assert_eq!(
            expand_target("$XDG_CONFIG_HOME/curlrc"),
            Some(xdg.join("curlrc").display().to_string())
        );
        assert!(names_protected("$XDG_CONFIG_HOME/curlrc", None));
    }

    /// An empty `XDG_DATA_HOME` is unset, as `env_dir` skips an empty
    /// value. The written path is not a relative file in the working
    /// directory. `data_base` also ignores a value that is not absolute
    /// and falls back to `~/.local/share`.
    #[test]
    fn an_empty_xdg_data_home_is_not_filled_in() {
        let _env = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_DATA_HOME", "");
        }
        for word in [
            "$XDG_DATA_HOME/ljos/git-hooks",
            "${XDG_DATA_HOME}/ljos/git-hooks",
        ] {
            assert_eq!(expand_target(word), None, "{word}");
            assert!(!names_protected(word, Some(&home)), "{word}");
        }

        let data = root.path().join("data");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe { std::env::set_var("XDG_DATA_HOME", &data) };
        assert_eq!(
            expand_target("$XDG_DATA_HOME/ljos/git-hooks"),
            Some(data.join("ljos/git-hooks").display().to_string())
        );
        assert_eq!(
            expand_target("${XDG_DATA_HOME}/ljos/git-hooks"),
            Some(data.join("ljos/git-hooks").display().to_string())
        );
    }

    /// An empty `XDG_CONFIG_HOME` does not add a relative `git/config`.
    /// Git reads `$HOME/.config/git/config` in that case.
    #[test]
    fn an_empty_xdg_config_home_adds_no_relative_git_config() {
        let _env = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", "");
        }
        let got = protected_write_targets();
        assert!(got
            .iter()
            .any(|path| path == &home.join(".config/git/config")));
        assert!(
            !got.iter()
                .any(|path| path == std::path::Path::new("git/config")),
            "an empty XDG_CONFIG_HOME added a relative git/config"
        );
        assert!(!names_protected("git/config", None));
        assert!(!names_protected("$XDG_CONFIG_HOME/git/config", None));

        let xdg = root.path().join("xdg");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &xdg) };
        let got = protected_write_targets();
        assert!(got.iter().any(|path| path == &xdg.join("git/config")));
        assert!(names_protected("$XDG_CONFIG_HOME/git/config", None));
    }

    /// Curl 7.65 opened the home `_curlrc` on MS-DOS (`DOT_CHAR` under
    /// `MSDOS`) and did not search beside the executable. That search is
    /// `WIN32` only. ljos has no DOS target, and the home path stays on
    /// [`curl_config_files`].
    #[test]
    fn msdos_home_underscore_curlrc_stays_on_the_list() {
        let _env_read = crate::test_env_read_lock();
        let notes = include_str!("cloud_hook.rs")
            .split("mod tests")
            .next()
            .unwrap();
        assert!(
            notes.contains("under `MSDOS`"),
            "the curl note does not record MS-DOS DOT_CHAR"
        );
        assert!(
            notes.contains("executable-directory search is `WIN32` only"),
            "the curl note does not record the WIN32-only executable search"
        );
        let _env = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        // SAFETY: test_env_lock serialises environment changes and restores them.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("CURL_HOME");
            std::env::remove_var("USERPROFILE");
            std::env::remove_var("APPDATA");
        }
        let got = curl_config_files();
        assert!(
            got.iter().any(|path| path == &home.join("_curlrc")),
            "home _curlrc missing from {}",
            got.iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let exe = std::env::current_exe().unwrap();
        let beside = exe.parent().unwrap().join("_curlrc");
        assert!(
            !got.iter().any(|path| path == &beside),
            "executable-directory _curlrc was enumerated at {}",
            beside.display()
        );
    }
}
