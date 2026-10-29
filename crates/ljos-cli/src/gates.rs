//! Git call reading for the one push check.
//!
//! A push onto a protected branch asks when the ticket has no settled
//! consensus. That check lives in the sitting gate and reads
//! [`consensus_push`] and [`protected_push`] here. Commit, merge, evidence
//! and foreign-owner gates are gone.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Nothing left for a push to warn about. The evidence gate is gone.
#[must_use]
pub fn take_push_warning() -> Option<String> {
    None
}

#[cfg(test)]
thread_local! {
    /// User and seat config for this thread, instead of [`super::config_base`].
    static LAYER_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    /// `LJOS_SEAT` for this thread, so a test can name a seat file without
    /// changing the process environment.
    static LAYER_SEAT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// No commit, merge, or evidence gate remains. Always `None`.
#[must_use]
pub fn command_gate(_line: &str, _cwd: Option<&Path>) -> Option<super::Rule> {
    None
}

/// One git call a shell line runs, its aliases expanded: the checkout it
/// runs in, the subcommand, its arguments, and each `-c KEY=VALUE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCall {
    pub cwd: PathBuf,
    pub sub: String,
    pub args: Vec<String>,
    pub config: Vec<(String, String)>,
}

/// Every git call `line` runs from `cwd`, the process directory when
/// absent. `cd DIR`, `git -C DIR`, `sh -c`, `$(...)` and a `!` alias's
/// shell are followed the way [`command_gate`] follows them.
///
/// # Errors
///
/// The aliases nest too deep to read, so what the line runs is not known.
pub fn git_calls(line: &str, cwd: Option<&Path>) -> Result<Vec<GitCall>, String> {
    let mut out = Vec::new();
    if !line.to_ascii_lowercase().contains("git") {
        return Ok(out);
    }
    let found = std::env::current_dir().ok();
    let base = cwd.or(found.as_deref()).unwrap_or_else(|| Path::new("."));
    let mut queue = invocations(line, base, 0);
    let mut i = 0;
    while i < queue.len() {
        let inv = queue[i].clone();
        i += 1;
        if inv.depth > 3 {
            return Err("its git aliases nest too deep to read".into());
        }
        let Some(git) = git_of(&inv) else {
            continue;
        };
        match git.shell {
            Some(shell) => {
                let extras = invocations(&shell, &git.cwd, inv.depth.saturating_add(1));
                queue.splice(i..i, extras);
            }
            None => out.push(GitCall {
                cwd: git.cwd,
                sub: git.sub,
                args: git.args,
                config: git.config,
            }),
        }
    }
    Ok(out)
}

/// The static guard and then the pack's rules. `None` lets the line run.
#[must_use]
pub fn guard_verdict(line: &str, cwd: Option<&Path>, rules: &[super::Rule]) -> Option<super::Rule> {
    let fast = super::static_guard_in(line, cwd);
    let here = cwd.and_then(Path::to_str).map(str::to_string).or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|dir| dir.into_os_string().into_string().ok())
    });
    let gated = super::redirect_seat_verb(
        super::gate_push(
            super::ruled_verdict(rules, line).as_ref(),
            line,
            here.as_deref(),
        ),
        line,
    );
    if gated.as_ref().is_some_and(|rule| rule.verdict == "deny") {
        return gated;
    }
    fast.or(gated)
}

#[derive(Debug, Clone)]
struct Inv {
    cwd: PathBuf,
    words: Vec<String>,
    depth: u8,
}

#[derive(Debug, Clone)]
struct GitCmd {
    cwd: PathBuf,
    config: Vec<(String, String)>,
    sub: String,
    args: Vec<String>,
    shell: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct GateConfig {
    protected: Vec<String>,
}

fn branch_of(name: &str) -> String {
    name.trim().trim_start_matches("refs/heads/").to_string()
}

/// User `ljos.toml`, then the seat's, then the repository's `ljos.toml`,
/// then `LJOS_CONFIG`, then `.ljos/gates.toml`. A later file overrides the
/// keys it sets. Identity is whatever the last of these files names. With
/// none of them naming a person, there is no identity gate.
fn config_layers(root: &Path) -> Vec<PathBuf> {
    let mut paths = person_layers();
    paths.push(root.join("ljos.toml"));
    if let Some(path) = std::env::var_os("LJOS_CONFIG").filter(|path| !path.is_empty()) {
        paths.push(PathBuf::from(path));
    }
    paths.push(root.join(".ljos").join("gates.toml"));
    paths
}

fn user_config_base() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = LAYER_HOME.with(|slot| slot.borrow().clone()) {
        return Some(path);
    }
    super::config_base()
}

fn configured_seat() -> Option<String> {
    #[cfg(test)]
    if let Some(name) = LAYER_SEAT.with(|slot| slot.borrow().clone()) {
        return clean_seat(&name);
    }
    std::env::var("LJOS_SEAT")
        .ok()
        .and_then(|name| clean_seat(&name))
}

fn clean_seat(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    Some(name.to_string())
}

fn read_layer(path: &Path) -> Result<Option<toml::map::Map<String, toml::Value>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "`{}` cannot be read ({e}). Fix the file, then retry.",
            path.display()
        )
    })?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value: toml::Value = toml::from_str(&text).map_err(|e| {
        format!(
            "`{}` does not parse ({e}). Fix the file, then retry.",
            path.display()
        )
    })?;
    match value {
        toml::Value::Table(root) => Ok(Some(root)),
        _ => Err(format!(
            "`{}` is not a table. Fix the file, then retry.",
            path.display()
        )),
    }
}

fn apply_gate_file(cfg: &mut GateConfig, path: &Path) -> Result<(), String> {
    let Some(root) = read_layer(path)? else {
        return Ok(());
    };
    let gates = root.get("gates").and_then(toml::Value::as_table);
    let src = gates.unwrap_or(&root);
    if src.contains_key("protected_branches") {
        cfg.protected = req_strings(src, "protected_branches", path)?
            .into_iter()
            .map(|name| branch_of(&name))
            .filter(|name| !name.is_empty())
            .collect();
    }
    Ok(())
}

fn req_strings(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    path: &Path,
) -> Result<Vec<String>, String> {
    let Some(items) = table.get(key).and_then(toml::Value::as_array) else {
        return Err(format!(
            "`{}` key `{key}` must be a list of strings. Fix the file, then retry.",
            path.display()
        ));
    };
    let mut out = Vec::new();
    for item in items {
        let Some(text) = item.as_str() else {
            return Err(format!(
                "`{}` key `{key}` must be a list of strings. Fix the file, then retry.",
                path.display()
            ));
        };
        out.push(text.to_string());
    }
    Ok(out)
}

/// The branches a `--force-with-lease` push asks before it replaces:
/// `main`, `master` and `develop`, and every branch `protected_branches`
/// names in the config layers [`config_layers`] reads (the user and seat
/// `ljos.toml`, the repository's `ljos.toml`, `LJOS_CONFIG` and
/// `.ljos/gates.toml`). Outside a checkout the user and seat files and
/// `LJOS_CONFIG` still count. `None` when one of them does not parse, so
/// the caller asks.
pub(crate) fn lease_protected(cwd: Option<&Path>) -> Option<Vec<String>> {
    let mut cfg = GateConfig::default();
    let layers = match cwd.and_then(repo_root) {
        Some(root) => config_layers(&root),
        None => user_layers(),
    };
    for path in layers {
        if path.is_file() {
            apply_gate_file(&mut cfg, &path).ok()?;
        }
    }
    let mut out: Vec<String> = ["main", "master", "develop"].map(String::from).to_vec();
    out.extend(cfg.protected);
    Some(out)
}

/// The config layers that do not live in a checkout: the user and seat
/// `ljos.toml`, then `LJOS_CONFIG`.
fn user_layers() -> Vec<PathBuf> {
    let mut paths = person_layers();
    if let Some(path) = std::env::var_os("LJOS_CONFIG").filter(|path| !path.is_empty()) {
        paths.push(PathBuf::from(path));
    }
    paths
}

/// The user and seat `ljos.toml`: the person's own files, which the seat
/// guard keeps a runner from writing.
fn person_layers() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(base) = user_config_base() {
        paths.push(base.join("ljos").join("ljos.toml"));
        if let Some(seat) = configured_seat() {
            paths.push(base.join("ljos").join("seats").join(seat).join("ljos.toml"));
        }
    }
    paths
}

/// What a runner's push onto one of `branches` waits for: `quorum` seats
/// agreeing on the ticket it sits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsensusPush {
    pub quorum: usize,
    pub branches: Vec<String>,
}

/// The [`ConsensusPush`] settings: `quorum` (2 seats unless set) and
/// `consensus_branches` (`develop` and `main` unless set), under `[gates]`
/// or at the top of a file. The user and seat `ljos.toml` set either. The
/// repository's `ljos.toml`, `LJOS_CONFIG` and `.ljos/gates.toml` only
/// raise the quorum and add branches, since a runner can write them.
///
/// # Errors
///
/// A layer that does not parse, or a key of the wrong kind.
pub fn consensus_push(cwd: Option<&Path>) -> Result<ConsensusPush, String> {
    let mut out = ConsensusPush {
        quorum: 2,
        branches: vec!["develop".into(), "main".into()],
    };
    for path in person_layers() {
        let (quorum, branches) = consensus_keys(&path)?;
        out.quorum = quorum.unwrap_or(out.quorum);
        out.branches = branches.unwrap_or(out.branches);
    }
    let root = cwd.and_then(repo_root);
    let mut rest: Vec<PathBuf> = root.iter().map(|r| r.join("ljos.toml")).collect();
    if let Some(path) = std::env::var_os("LJOS_CONFIG").filter(|path| !path.is_empty()) {
        rest.push(PathBuf::from(path));
    }
    rest.extend(root.iter().map(|r| r.join(".ljos").join("gates.toml")));
    for path in rest {
        let (quorum, branches) = consensus_keys(&path)?;
        out.quorum = out.quorum.max(quorum.unwrap_or(0));
        for branch in branches.unwrap_or_default() {
            if !out.branches.contains(&branch) {
                out.branches.push(branch);
            }
        }
    }
    Ok(out)
}

fn consensus_keys(path: &Path) -> Result<(Option<usize>, Option<Vec<String>>), String> {
    if !path.is_file() {
        return Ok((None, None));
    }
    let Some(root) = read_layer(path)? else {
        return Ok((None, None));
    };
    let src = root
        .get("gates")
        .and_then(toml::Value::as_table)
        .unwrap_or(&root);
    let quorum = match src.get("quorum") {
        None => None,
        Some(toml::Value::Integer(n)) if *n >= 1 => Some(usize::try_from(*n).unwrap_or(usize::MAX)),
        Some(_) => {
            return Err(format!(
                "`{}` key `quorum` must be a whole number of seats, 1 or more. Fix the file, then retry.",
                path.display()
            ))
        }
    };
    let branches = if src.contains_key("consensus_branches") {
        let names = req_strings(src, "consensus_branches", path)?;
        Some(
            names
                .iter()
                .map(|name| branch_of(name))
                .filter(|name| !name.is_empty())
                .collect(),
        )
    } else {
        None
    };
    Ok((quorum, branches))
}

/// Where `call`, a `git push` on `line`, lands that needs a consensus: a
/// branch in `branches`, in backticks, or why the push cannot be told
/// from one that does. `None` when each ref it updates is read and none
/// is in `branches`. The push is read the way git reads it: the first
/// positional is the repository; a bare push lands on the current branch
/// and its upstream unless `push.default` says otherwise; a branch named
/// alone lands on its upstream under `push.default=upstream`. A tag, or
/// any ref outside `refs/heads/`, is not a branch.
#[must_use]
pub fn protected_push(call: &GitCall, line: &str, branches: &[String]) -> Option<String> {
    let unread = |why: String| {
        Some(format!(
            "a branch the guard cannot tell from a protected one, since {why}"
        ))
    };
    let cwd = call.cwd.as_path();
    let args = &call.args;
    for word in [
        "GIT_CONFIG",
        "GIT_DIR",
        "--git-dir",
        "GIT_NAMESPACE",
        "--namespace",
    ] {
        if line.contains(word) {
            return unread(format!("the line sets `{word}`"));
        }
    }
    for (key, _) in &call.config {
        let key = key.to_ascii_lowercase();
        if ["remote.", "push.", "branch."]
            .iter()
            .any(|p| key.starts_with(p))
        {
            return unread(format!("it sets `{key}` for this push"));
        }
    }
    if args.iter().any(|a| has_subst(a)) {
        return unread("an argument is filled in when the line runs".into());
    }
    if let Some(all) = args
        .iter()
        .find(|a| matches!(a.as_str(), "--all" | "--mirror" | "--branches" | "--prune"))
    {
        return Some(format!("every branch `{all}` reaches"));
    }
    if let Some(flag) = args.iter().find(|a| {
        a.starts_with("--repo")
            || (a.starts_with("--recurse-submodules")
                && !a.ends_with("=check")
                && !a.ends_with("=no"))
    }) {
        return unread(format!("`{flag}` moves where it goes"));
    }
    if git_text(cwd, &["config", "--get-regexp", r"^remote\..*\.push$"]).is_some() {
        return unread("a remote in this checkout sets its own push refspecs".into());
    }
    let mode = git_config(cwd, "push.default")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let upstream = |branch: &str| git_config(cwd, &format!("branch.{branch}.merge"));
    let here = || -> Result<Vec<String>, String> {
        if ["checkout", "switch", "symbolic-ref"]
            .iter()
            .any(|w| line.contains(w))
        {
            return Err("the line may change branch before it pushes".into());
        }
        let head = git_text(cwd, &["symbolic-ref", "-q", "--short", "HEAD"])
            .filter(|head| !head.is_empty())
            .ok_or_else(|| "HEAD names no branch".to_string())?;
        let up = upstream(&head).filter(|_| mode != "current");
        Ok(std::iter::once(head).chain(up).collect())
    };
    let pos = positionals(args);
    let specs = pos.get(1..).unwrap_or_default();
    let mut dests = Vec::new();
    if specs.is_empty() {
        match mode.as_str() {
            "nothing" => return None,
            "matching" => {
                return Some("every branch both sides have, under `push.default=matching`".into())
            }
            _ => match here() {
                Ok(here) => dests.extend(here),
                Err(why) => return unread(why),
            },
        }
    }
    for spec in specs {
        let spec = spec.strip_prefix('+').unwrap_or(spec);
        if spec.contains('*') {
            let dst = spec.rsplit(':').next().unwrap_or(spec);
            if !dst.starts_with("refs/") || dst.starts_with("refs/heads/") {
                return Some(format!("the branches `{spec}` matches"));
            }
            continue;
        }
        match spec.split_once(':') {
            Some((_, "HEAD" | "@")) => {
                return unread(format!("`{spec}` lands on what the far side's HEAD names"))
            }
            Some((src, "")) => dests.push(src.to_string()),
            Some((_, dst)) => dests.push(dst.to_string()),
            None if matches!(spec, "HEAD" | "@") => match here() {
                Ok(here) => dests.extend(here),
                Err(why) => return unread(why),
            },
            None => {
                dests.push(spec.to_string());
                if matches!(mode.as_str(), "upstream" | "tracking") {
                    dests.extend(upstream(spec));
                }
            }
        }
    }
    dests.iter().find_map(|dest| {
        let name = dest
            .strip_prefix("refs/heads/")
            .or_else(|| dest.strip_prefix("heads/"))
            .unwrap_or(dest);
        branches
            .iter()
            .find(|b| b.eq_ignore_ascii_case(name))
            .map(|b| format!("`{b}`"))
    })
}

fn repo_root(cwd: &Path) -> Option<PathBuf> {
    git_text(cwd, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

fn git_config(cwd: &Path, key: &str) -> Option<String> {
    let value = git_text(cwd, &["config", "--get", key])?;
    (!value.is_empty()).then_some(value)
}

fn git_text(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::apply_git_command_env(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string(),
    )
}

fn has_subst(s: &str) -> bool {
    if s.contains('`') {
        return true;
    }
    let bytes = s.as_bytes();
    bytes.iter().enumerate().any(|(i, c)| {
        if *c != b'$' {
            return false;
        }
        matches!(
            bytes.get(i + 1).copied().unwrap_or(0),
            b'(' | b'{' | b'_' | b'a'..=b'z' | b'A'..=b'Z'
        )
    })
}

pub(crate) fn is_dry(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "--dry-run"
            || a == "-n"
            || (a.starts_with('-') && !a.starts_with("--") && a.chars().skip(1).any(|c| c == 'n'))
    })
}

fn positionals(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            out.extend(args[i + 1..].iter().cloned());
            break;
        }
        if matches!(
            a.as_str(),
            "-o" | "--push-option" | "--receive-pack" | "--exec" | "--repo"
        ) {
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

fn invocations(line: &str, base: &Path, depth: u8) -> Vec<Inv> {
    let mut cwd = base.to_path_buf();
    let mut out = Vec::new();
    for sub in super::nested_lines(line, 0) {
        for seg in super::raw_segments(&sub) {
            let words = super::shell_words(&seg);
            if words.is_empty() {
                continue;
            }
            let (at, _) = super::prefix_end(&words);
            if at >= words.len() {
                continue;
            }
            let cmd = words[at..].to_vec();
            if cmd.is_empty() {
                continue;
            }
            if super::base_name(&cmd[0]) == "cd" {
                if let Some(d) = cmd.get(1) {
                    if d != "-" && !d.starts_with('-') {
                        cwd = join_dir(&cwd, d);
                    }
                }
                continue;
            }
            out.push(Inv {
                cwd: cwd.clone(),
                words: cmd,
                depth,
            });
        }
    }
    out
}

fn join_dir(base: &Path, dir: &str) -> PathBuf {
    let path = Path::new(dir);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn parse_git(inv: &Inv) -> Option<GitCmd> {
    if super::base_name(&inv.words[0]) != "git" {
        return None;
    }
    let mut i = 1;
    let mut cwd = inv.cwd.clone();
    let mut config = Vec::new();
    while i < inv.words.len() {
        let w = inv.words[i].as_str();
        if w == "--" {
            i += 1;
            break;
        }
        if w == "-C" {
            if let Some(d) = inv.words.get(i + 1) {
                cwd = join_dir(&cwd, d);
            }
            i += 2;
            continue;
        }
        if w == "-c" {
            if let Some((k, v)) = inv.words.get(i + 1).and_then(|kv| kv.split_once('=')) {
                config.push((k.to_string(), v.to_string()));
            }
            i += 2;
            continue;
        }
        if let Some((k, v)) = w
            .strip_prefix("-c")
            .filter(|s| !s.is_empty() && s.contains('='))
            .and_then(|kv| kv.split_once('='))
        {
            config.push((k.to_string(), v.to_string()));
            i += 1;
            continue;
        }
        if matches!(
            w,
            "--git-dir"
                | "--work-tree"
                | "--namespace"
                | "--config-env"
                | "--exec-path"
                | "--super-prefix"
        ) {
            i += 2;
            continue;
        }
        if w.starts_with("--git-dir=")
            || w.starts_with("--work-tree=")
            || w.starts_with("--namespace=")
            || w.starts_with("--config-env=")
            || w.starts_with("--exec-path=")
            || w.starts_with("--super-prefix=")
        {
            i += 1;
            continue;
        }
        if w.starts_with('-') {
            i += 1;
            continue;
        }
        break;
    }
    let sub = inv.words.get(i)?.clone();
    let args = inv.words.get(i + 1..).unwrap_or(&[]).to_vec();
    Some(GitCmd {
        cwd,
        config,
        sub,
        args,
        shell: None,
    })
}

fn git_of(inv: &Inv) -> Option<GitCmd> {
    let mut git = parse_git(inv)?;
    let mut seen = std::collections::HashSet::new();
    while let Some(value) = alias_of(&git) {
        if !seen.insert(git.sub.clone()) {
            break;
        }
        if let Some(shell) = value.strip_prefix('!') {
            git.shell = Some(shell.trim().to_string());
            break;
        }
        let mut parts: Vec<String> = value.split_whitespace().map(str::to_string).collect();
        if parts.is_empty() {
            break;
        }
        git.sub = parts.remove(0);
        parts.append(&mut git.args);
        git.args = parts;
    }
    Some(git)
}

fn alias_of(git: &GitCmd) -> Option<String> {
    if matches!(
        git.sub.as_str(),
        "commit"
            | "push"
            | "merge"
            | "checkout"
            | "switch"
            | "status"
            | "diff"
            | "add"
            | "log"
            | "show"
            | "fetch"
            | "pull"
            | "rebase"
            | "stash"
            | "branch"
            | "tag"
            | "remote"
            | "init"
            | "clone"
            | "restore"
            | "reset"
            | "clean"
            | "rev-parse"
            | "config"
            | "help"
            | "version"
            | "blame"
            | "grep"
            | "mv"
            | "rm"
            | "apply"
            | "cherry-pick"
            | "revert"
            | "bisect"
            | "reflog"
    ) {
        return None;
    }
    let key = format!("alias.{}", git.sub);
    if let Some((_, value)) = git.config.iter().rev().find(|(k, _)| k == &key) {
        return Some(value.clone());
    }
    git_config(&git.cwd, &key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHOR_NAME: &str = "Ada Lovelace";
    const AUTHOR_EMAIL: &str = "ada@example.com";

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", AUTHOR_NAME)
            .env("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL)
            .env("GIT_COMMITTER_NAME", AUTHOR_NAME)
            .env("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// The checkout, and the environment lock. Gate tests read the process
    /// home: a temp home suspends the guard. Another test sets that home
    /// for its own case, so this lock is held until the checkout is dropped.
    struct Held {
        _env: crate::EnvLock,
        dir: tempfile::TempDir,
    }

    impl std::ops::Deref for Held {
        type Target = tempfile::TempDir;

        fn deref(&self) -> &Self::Target {
            &self.dir
        }
    }

    fn repo() -> Held {
        let env = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "user.name", AUTHOR_NAME]);
        git(dir.path(), &["config", "user.email", AUTHOR_EMAIL]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        git(
            dir.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://example.com/acme/widgets.git",
            ],
        );
        Held { _env: env, dir }
    }

    fn seed(dir: &Path) {
        std::fs::write(dir.join("f.txt"), "one\n").unwrap();
        git(dir, &["add", "f.txt"]);
        git(dir, &["commit", "-q", "-m", "note the seat"]);
    }

    fn gates(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir.join(".ljos")).unwrap();
        std::fs::write(dir.join(".ljos").join("gates.toml"), body).unwrap();
    }

    #[test]
    fn git_calls_follow_dirs_shells_and_aliases() {
        let dir = repo();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        git(root, &["config", "alias.ci", "commit -q"]);
        git(
            root,
            &["config", "alias.ship", "!git add . && git push origin HEAD"],
        );
        let calls = git_calls(
            "cd sub && git -C .. ci -m x; sh -c 'git ship'; echo $(git cherry-pick abc)",
            Some(root),
        )
        .unwrap();
        let subs: Vec<&str> = calls.iter().map(|c| c.sub.as_str()).collect();
        assert_eq!(subs, ["commit", "add", "push", "cherry-pick"], "{calls:?}");
        assert_eq!(calls[0].args, ["-q", "-m", "x"]);
        assert_eq!(calls[0].cwd, root.join("sub").join(".."));
        assert_eq!(calls[2].args, ["origin", "HEAD"]);
        assert!(git_calls("ls -la", Some(root)).unwrap().is_empty());

        for (name, next) in [("a", "b"), ("b", "c"), ("c", "d"), ("d", "e")] {
            git(
                root,
                &["config", &format!("alias.{name}"), &format!("!git {next}")],
            );
        }
        let deep = git_calls("git a", Some(root)).unwrap_err();
        assert!(deep.contains("too deep"), "{deep}");
    }

    struct Layers;

    impl Drop for Layers {
        fn drop(&mut self) {
            LAYER_HOME.with(|slot| *slot.borrow_mut() = None);
            LAYER_SEAT.with(|slot| *slot.borrow_mut() = None);
        }
    }

    fn layers(home: &Path, seat: Option<&str>) -> Layers {
        LAYER_HOME.with(|slot| *slot.borrow_mut() = Some(home.to_path_buf()));
        LAYER_SEAT.with(|slot| *slot.borrow_mut() = seat.map(str::to_string));
        Layers
    }

    fn write_toml(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn the_person_sets_the_consensus_push_and_a_checkout_only_tightens_it() {
        let dir = repo();
        let cwd = dir.path();
        let home = tempfile::tempdir().unwrap();
        let _layers = layers(home.path(), Some("box"));
        let read = || consensus_push(Some(cwd)).unwrap();
        let names = |all: &[&str]| all.iter().map(|b| (*b).to_string()).collect::<Vec<_>>();
        assert_eq!(read().quorum, 2);
        assert_eq!(read().branches, names(&["develop", "main"]));
        write_toml(
            &home.path().join("ljos/ljos.toml"),
            "[gates]\nquorum = 3\nconsensus_branches = [\"main\", \"release\"]\n",
        );
        assert_eq!(read().quorum, 3);
        assert_eq!(read().branches, names(&["main", "release"]));
        write_toml(
            &home.path().join("ljos/seats/box/ljos.toml"),
            "quorum = 1\n",
        );
        assert_eq!(read().quorum, 1, "the seat file is the person's too");
        write_toml(
            &cwd.join("ljos.toml"),
            "quorum = 1\nconsensus_branches = []\n",
        );
        gates(
            cwd,
            "quorum = 2\nconsensus_branches = [\"refs/heads/stable\"]\n",
        );
        assert_eq!(read().quorum, 2, "a checkout raises the quorum");
        assert_eq!(read().branches, names(&["main", "release", "stable"]));
        write_toml(&cwd.join("ljos.toml"), "quorum = 0\n");
        let zero = consensus_push(Some(cwd)).unwrap_err();
        assert!(zero.contains("`quorum`"), "{zero}");
        write_toml(&cwd.join("ljos.toml"), "quorum = [\n");
        let broken = consensus_push(Some(cwd)).unwrap_err();
        assert!(broken.contains("does not parse"), "{broken}");
    }

    #[test]
    fn a_push_lands_where_git_would_put_it_and_an_unread_one_is_protected() {
        let dir = repo();
        let cwd = dir.path();
        seed(cwd);
        git(cwd, &["checkout", "-q", "-b", "topic"]);
        let branches = ["develop".to_string(), "main".to_string()];
        let lands = |line: &str| {
            let calls = git_calls(line, Some(cwd)).unwrap();
            let push = calls.iter().find(|c| c.sub == "push").unwrap();
            protected_push(push, line, &branches)
        };
        let unread = |line: &str| {
            lands(line).is_some_and(|d| d.starts_with("a branch the guard cannot tell"))
        };
        for line in [
            "git push",
            "git push origin",
            "git push origin topic",
            "git push -u origin HEAD",
            "git push origin topic:refs/heads/topic",
            "git push origin v1:refs/tags/develop",
            "git push origin 'refs/tags/*:refs/tags/*'",
            "git push develop topic",
        ] {
            assert_eq!(lands(line), None, "{line}");
        }
        for (line, dest) in [
            ("git push origin HEAD:develop", "`develop`"),
            ("git push origin topic:refs/heads/develop", "`develop`"),
            ("git push origin +topic:heads/develop", "`develop`"),
            ("git push origin :develop", "`develop`"),
            ("git push --delete origin develop", "`develop`"),
            ("git push origin topic HEAD:Main", "`main`"),
            (
                "git push https://example.com/acme/widgets.git main",
                "`main`",
            ),
            ("git push --all origin", "every branch `--all` reaches"),
            (
                "git push origin 'refs/heads/*:refs/heads/*'",
                "the branches `refs/heads/*:refs/heads/*` matches",
            ),
        ] {
            assert_eq!(lands(line).as_deref(), Some(dest), "{line}");
        }
        for line in [
            "git checkout develop && git push",
            "GIT_DIR=../other git push origin topic",
            "git -c remote.origin.push=refs/heads/topic:refs/heads/develop push origin topic",
            "git push origin $BRANCH",
            "git push --repo=origin topic",
            "git push origin topic:HEAD",
        ] {
            assert!(unread(line), "{line}: {:?}", lands(line));
        }

        git(cwd, &["config", "branch.topic.merge", "refs/heads/develop"]);
        assert_eq!(lands("git push").as_deref(), Some("`develop`"));
        assert_eq!(lands("git push origin HEAD").as_deref(), Some("`develop`"));
        assert_eq!(
            lands("git push origin topic"),
            None,
            "simple names the branch"
        );
        git(cwd, &["config", "push.default", "upstream"]);
        assert_eq!(lands("git push origin topic").as_deref(), Some("`develop`"));
        git(cwd, &["config", "push.default", "current"]);
        assert_eq!(lands("git push"), None);
        git(cwd, &["config", "push.default", "nothing"]);
        assert_eq!(lands("git push"), None);
        git(cwd, &["config", "push.default", "matching"]);
        assert!(lands("git push")
            .unwrap()
            .starts_with("every branch both sides have"));
        git(cwd, &["config", "--unset", "push.default"]);
        git(
            cwd,
            &[
                "config",
                "remote.origin.push",
                "refs/heads/topic:refs/heads/develop",
            ],
        );
        assert!(unread("git push origin topic"));
        git(cwd, &["config", "--unset", "remote.origin.push"]);
        git(cwd, &["checkout", "-q", "--detach"]);
        assert!(unread("git push origin HEAD"));
        assert_eq!(lands("git push origin HEAD:refs/heads/topic"), None);
    }
}
