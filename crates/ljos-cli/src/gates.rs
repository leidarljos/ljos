//! Commit, push, and merge gates for the hook and `ljos_guard_check`.
//!
//! The checks are mechanical. A line that is not a commit, a push, or a
//! merge is left to the seat guard and the pack's rules. A denial names
//! the command that makes the same line pass. A push is never refused for
//! a missing test record: it runs, and the hook passes the warning on.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

thread_local! {
    static GH_BIN: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// What the last judged push had to say without refusing it: its tip
    /// has no passing test record, or the gate settings did not parse.
    static PUSH_WARNING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The warning the last judged push left, once. The push ran either way.
#[must_use]
pub fn take_push_warning() -> Option<String> {
    PUSH_WARNING.with(|slot| slot.borrow_mut().take())
}

fn warn_push(text: String) {
    PUSH_WARNING.with(|slot| *slot.borrow_mut() = Some(text));
}

#[cfg(test)]
thread_local! {
    /// User and seat config for this thread, instead of [`super::config_base`].
    static LAYER_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    /// `LJOS_SEAT` for this thread, so a test can name a seat file without
    /// changing the process environment.
    static LAYER_SEAT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The commit, push, and merge gates alone. `None` means the line is not
/// one of those, or it already passes. A denial names the command that
/// makes the same line pass. The static guard calls this; it does not
/// call back into the static guard or policyd.
#[must_use]
pub fn command_gate(line: &str, cwd: Option<&Path>) -> Option<super::Rule> {
    PUSH_WARNING.with(|slot| *slot.borrow_mut() = None);
    let found = std::env::current_dir().ok();
    let base = cwd.or(found.as_deref());
    enforce(line, base, 0).err()
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

/// The static guard (seat, user rules, these gates, a dangerous push) and
/// then the pack's rules, the same composition the hook uses. `None` lets
/// the line run. A plain push runs. When the checkout sets `test` and the
/// tip has no passing record, [`take_push_warning`] says so. A force,
/// mirror or delete push asks, and so does a push onto a protected branch
/// of a repository whose owner is not in `owners`. A merge whose head sha
/// has no recorded checks asks too: the reason names that sha, and a local
/// evidence record when the checkout has one for it. A fine-grained token
/// with no Checks permission reads the REST fallback when the rollup is
/// denied, and the reason names which source it used.
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

/// Run the repo's configured test command and store a passing record for
/// the current tip. The push gate accepts only that record.
///
/// # Errors
///
/// The working directory is not a repository, `.ljos/gates.toml` has no
/// matching `test` command, the command fails, or HEAD moves while it runs.
pub fn record_evidence(cmd: &[String]) -> Result<String> {
    let cwd = std::env::current_dir().context(
        "evidence: no working directory. Run `ljos evidence record -- <test command>` from the repository checkout",
    )?;
    record_in(&cwd, cmd)
}

fn record_in(cwd: &Path, cmd: &[String]) -> Result<String> {
    if cmd.is_empty() || cmd.iter().all(|c| c == "--") {
        bail!(
            "evidence: pass the test command after `--`. Example: `ljos evidence record -- cargo test --workspace`"
        );
    }
    let root = repo_root(cwd).context(
        "evidence: run this inside a git repository. `ljos evidence record -- <test command>` stores the tip sha, and the command is the `test` line in `.ljos/gates.toml`",
    )?;
    let cfg = load_config(&root).map_err(|e| anyhow::anyhow!("evidence: {e}"))?;
    let want = cfg.test.as_deref().context(
        "evidence: `.ljos/gates.toml` has no `test` command. Add `test = \"cargo test --workspace\"` (or this repo's suite, such as `meson test --suite eon`) at the repository root, then run `ljos evidence record -- <that command>`",
    )?;
    let got = norm_cmd(&cmd.join(" "));
    if got != want {
        bail!(
            "evidence: `{got}` is not the test command in `.ljos/gates.toml`. Run `ljos evidence record -- {want}`"
        );
    }
    let before = head_sha(&root).with_context(|| {
        format!(
            "evidence: this repository has no commit yet. Commit first, then run `ljos evidence record -- {want}`"
        )
    })?;
    let status = Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(&root)
        .status()
        .with_context(|| {
            format!(
                "evidence: `{want}` did not start. Run `ljos evidence record -- {want}` once that command is on PATH"
            )
        })?;
    let after = head_sha(&root).unwrap_or_default();
    if after != before {
        bail!(
            "evidence: HEAD moved from {before} to {after} while `{want}` ran, so the run is not evidence for one commit. Commit first, then run `ljos evidence record -- {want}`"
        );
    }
    let code = status.code().unwrap_or(1);
    if !status.success() {
        bail!(
            "evidence: `{want}` exited {code}, so nothing was stored for {before}. Fix the failure, then run `ljos evidence record -- {want}`"
        );
    }
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    write_evidence(&root, &before, want, &ts)?;
    Ok(format!("evidence: {before} passed `{want}` at {ts}"))
}

#[derive(Debug, Default)]
struct Walk {
    branch: Option<String>,
    branch_set: bool,
    fresh: bool,
}

#[derive(Debug, Clone)]
struct Inv {
    cwd: PathBuf,
    env: Vec<(String, String)>,
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

#[derive(Debug, Clone)]
struct Person {
    name: String,
    email: String,
}

#[derive(Debug, Clone)]
struct GateConfig {
    test: Option<String>,
    signoff: bool,
    protected: Vec<String>,
    /// The accounts and organisations whose repositories are the person's
    /// own. Empty: every repository is.
    owners: Vec<String>,
    identity: Option<Person>,
    /// The file that set [`GateConfig::identity`].
    identity_from: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct Stored {
    command: String,
    exit: i32,
    sha: String,
    ts: String,
}

struct CommitFields {
    messages: Vec<String>,
    files: Vec<String>,
    reuse: Vec<String>,
    trailers: Vec<String>,
    author: Option<String>,
    signoff: bool,
    no_edit: bool,
    amend: bool,
    dry: bool,
    help: bool,
    hidden: bool,
}

struct ApiMerge {
    repo: Option<String>,
    spec: String,
}

fn deny(pattern: &str, token: &str, detail: &str) -> super::Rule {
    super::Rule {
        pattern: pattern.into(),
        verdict: "deny".into(),
        reason: join_sentences(&super::policyd_reason(token), detail),
    }
}

/// `base` and `detail` as two sentences. When `base` already ends with
/// `.`, `!` or `?`, that stop stays. Otherwise a full stop is added
/// before `detail`.
fn join_sentences(base: &str, detail: &str) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        return base.to_string();
    }
    let base = base.trim_end();
    if base.ends_with(['.', '!', '?']) {
        format!("{base} {detail}")
    } else {
        format!("{base}. {detail}")
    }
}

fn default_protected() -> Vec<String> {
    vec!["main".into(), "master".into()]
}

fn norm_cmd(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn branch_of(name: &str) -> String {
    name.trim().trim_start_matches("refs/heads/").to_string()
}

fn may_gate(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("git")
        || lower.contains("gh")
        || lower.contains("curl")
        || lower.contains("wget")
        || lower.contains("merge_pull_request")
        || lower.contains("mergepullrequest")
}

fn enforce(line: &str, cwd: Option<&Path>, depth: usize) -> Result<(), super::Rule> {
    if depth > 3 {
        return Err(deny(
            "push-gate",
            "git-push-evidence",
            "This command expands too many aliases. Run git commit, git push, or the merge directly.",
        ));
    }
    if !may_gate(line) {
        return Ok(());
    }
    if line.contains("ssh") {
        if let Some(rule) = remote_hidden(line) {
            return Err(rule);
        }
    }
    let base = cwd.unwrap_or_else(|| Path::new("."));
    let mut queue = invocations(line, base, 0);
    let mut walk = Walk::default();
    let mut i = 0;
    while i < queue.len() {
        let inv = queue[i].clone();
        i += 1;
        if inv.depth > 3 {
            return Err(deny(
                "push-gate",
                "git-push-evidence",
                "This command expands too many aliases. Run git commit, git push, or the merge directly.",
            ));
        }
        if let Some(git) = git_of(&inv) {
            if let Some(shell) = git.shell.clone() {
                if shell.trim().is_empty() {
                    continue;
                }
                let extras = invocations(&shell, &git.cwd, inv.depth.saturating_add(1));
                queue.splice(i..i, extras);
                continue;
            }
            handle_git(&git, &inv, &mut walk)?;
            continue;
        }
        if let Some(api) = api_merge(&inv.words) {
            let action = if api.spec.is_empty() {
                "merge_pull_request".to_string()
            } else {
                format!("merge_pull_request {}", api.spec)
            };
            review_pr(&inv.cwd, &api.spec, api.repo.as_deref(), None, &action)?;
        }
    }
    Ok(())
}

fn handle_git(git: &GitCmd, inv: &Inv, walk: &mut Walk) -> Result<(), super::Rule> {
    match git.sub.as_str() {
        "commit" => on_commit(git, inv, walk),
        "checkout" | "switch" => {
            if git.args.iter().any(|a| a == "-") {
                walk.branch = None;
                walk.branch_set = true;
            } else if let Some(branch) = switched_branch(&git.args) {
                walk.branch = Some(branch);
                walk.branch_set = true;
            }
            Ok(())
        }
        "push" => on_push(git, walk),
        _ => Ok(()),
    }
}

fn on_commit(git: &GitCmd, inv: &Inv, walk: &mut Walk) -> Result<(), super::Rule> {
    let fields = commit_fields(&git.args);
    if fields.help || fields.dry {
        return Ok(());
    }
    let cfg = repo_config(&git.cwd, "git-commit-identity")?;
    if let Some(person) = &cfg.identity {
        identity_ok(
            git,
            &inv.env,
            fields.author.as_deref(),
            person,
            cfg.identity_from.as_deref(),
        )?;
    }
    if fields.hidden {
        return Err(hidden_deny());
    }
    // `--amend --no-edit` keeps the message the last commit has, so there
    // is no new text to read for a trailer, and no repository to read it
    // from does not make it a hidden message.
    if fields.amend
        && fields.no_edit
        && fields.messages.is_empty()
        && fields.files.is_empty()
        && fields.reuse.is_empty()
        && fields.trailers.is_empty()
    {
        walk.fresh = true;
        return Ok(());
    }
    let message = if !fields.messages.is_empty() {
        fields.messages.join("\n\n")
    } else if !fields.files.is_empty() {
        read_files(&git.cwd, &fields.files).ok_or_else(hidden_deny)?
    } else if !fields.reuse.is_empty() {
        reuse_messages(&git.cwd, &fields.reuse).ok_or_else(hidden_deny)?
    } else if fields.no_edit {
        git_text(&git.cwd, &["log", "-1", "--format=%B"]).ok_or_else(hidden_deny)?
    } else {
        return Err(hidden_deny());
    };
    let mut message = message;
    for trailer in &fields.trailers {
        if has_subst(trailer) {
            return Err(hidden_deny());
        }
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str(trailer);
    }
    if let Some(what) = ai_marker(&message) {
        return Err(deny(
            "commit-gate",
            "git-commit-trailer",
            &format!(
                "Remove {what} from the message, then commit again with `git commit -m TEXT`."
            ),
        ));
    }
    if cfg.signoff && !fields.signoff {
        let Some(person) = &cfg.identity else {
            return Err(deny(
                "commit-gate",
                "git-commit-signoff",
                "Set `name` and `email` in the same file as `signoff`, then commit again.",
            ));
        };
        let line = signoff_line(person);
        if !has_signoff(&message, &line) {
            return Err(deny(
                "commit-gate",
                "git-commit-signoff",
                &format!("The message has no `{line}` line."),
            ));
        }
    }
    walk.fresh = true;
    Ok(())
}

fn on_push(git: &GitCmd, walk: &Walk) -> Result<(), super::Rule> {
    if is_dry(&git.args) || is_delete(&git.args) {
        return Ok(());
    }
    // No checkout has no tip and no gate file. A plain push is allowed. A
    // force push or a delete still asks in the push gate.
    let Some(root) = repo_root(&git.cwd) else {
        return Ok(());
    };
    let cfg = match load_config(&root) {
        Ok(cfg) => cfg,
        Err(e) => {
            warn_push(format!(
                "ljos ran this push without reading the gate settings: {e}"
            ));
            return Ok(());
        }
    };
    let current = if walk.branch_set {
        walk.branch.clone()
    } else {
        abbrev(&git.cwd)
    };
    let dests = push_dests(&git.cwd, &git.args, current.as_deref());
    if let Some(rule) = foreign_protected(&git.cwd, &git.args, current.as_deref(), &dests, &cfg) {
        return Err(rule);
    }
    // Evidence is a warning. A missing or stale record never holds a push,
    // and neither does a commit made on the same line.
    if let Some(test) = cfg.test.as_deref() {
        if let Some(why) = evidence_gap(&git.cwd, &git.args, current.as_deref(), walk.fresh, test) {
            warn_push(format!(
                "This push runs without a passing test record. {why} Run `ljos evidence record -- {test}` to record one."
            ));
        }
    }
    Ok(())
}

/// A push onto a protected branch of a repository the person does not
/// own asks. `owners` in `ljos.toml` or `.ljos/gates.toml` names the
/// accounts and organisations that are the person's own. With no `owners`
/// set, or a remote whose owner cannot be read, the push runs.
fn foreign_protected(
    cwd: &Path,
    args: &[String],
    current: Option<&str>,
    dests: &[String],
    cfg: &GateConfig,
) -> Option<super::Rule> {
    if cfg.owners.is_empty() {
        return None;
    }
    let branch = dests.iter().find(|d| is_protected(d, &cfg.protected))?;
    let url = push_url(cwd, args, current)?;
    let (owner, repo) = super::remote_slug(&url)?;
    if cfg.owners.iter().any(|o| o.eq_ignore_ascii_case(&owner)) {
        return None;
    }
    Some(super::Rule {
        pattern: "push-gate".into(),
        verdict: "ask".into(),
        reason: format!(
            "`{branch}` is a protected branch of {owner}/{repo}, and {owner} is not in `owners`; the agent asks you first"
        ),
    })
}

/// The URL a push goes to: a URL named in place of a remote, else the
/// named remote's push URL, else the current branch's remote, else
/// `origin`.
fn push_url(cwd: &Path, args: &[String], current: Option<&str>) -> Option<String> {
    let pos = positionals(args);
    let names = remotes(cwd);
    let remote = match pos.first() {
        Some(first) if names.iter().any(|r| r == first) => first.clone(),
        Some(first) if first.contains("://") || (first.contains('@') && first.contains(':')) => {
            return Some(first.clone());
        }
        _ => current
            .and_then(|c| git_config(cwd, &format!("branch.{c}.remote")))
            .unwrap_or_else(|| "origin".to_string()),
    };
    git_text(cwd, &["remote", "get-url", "--push", &remote]).filter(|u| !u.is_empty())
}

/// Why the commits a push publishes have no passing record for `test`,
/// or `None` when each one has.
fn evidence_gap(
    cwd: &Path,
    args: &[String],
    current: Option<&str>,
    fresh: bool,
    test: &str,
) -> Option<String> {
    let specs = refspecs(cwd, args);
    let pushes_head = specs.is_empty()
        || specs
            .iter()
            .filter_map(|spec| src_of(spec))
            .any(|src| pushing_current(&src, current));
    if fresh && pushes_head {
        return Some("The same command makes a new commit, so its tip has no record yet.".into());
    }
    let Some(shas) = shas_quietly(cwd, args, current) else {
        return Some("The commit this push publishes cannot be read.".into());
    };
    shas.iter()
        .find_map(|sha| evidence_problem(cwd, sha, test).map(|why| format!("Tip {sha}: {why}")))
}

/// The commits a push would publish, or `None` when one of them cannot be
/// read. An empty refspec is the current HEAD.
fn shas_quietly(cwd: &Path, args: &[String], current: Option<&str>) -> Option<Vec<String>> {
    let specs = refspecs(cwd, args);
    if specs.is_empty() {
        return head_sha(cwd).map(|sha| vec![sha]);
    }
    let mut out = Vec::new();
    for spec in specs {
        let Some(src) = src_of(&spec) else {
            continue;
        };
        let sha = if pushing_current(&src, current) {
            head_sha(cwd)?
        } else {
            rev(cwd, &src)?
        };
        out.push(sha);
    }
    Some(out)
}

/// What is wrong with the stored record for `sha`, or `None` when it is a
/// passing run of `test`.
fn evidence_problem(cwd: &Path, sha: &str, test: &str) -> Option<&'static str> {
    let Some(path) = evidence_file(cwd, sha) else {
        return Some("the tip sha cannot be stored as evidence.");
    };
    if !path.is_file() {
        return Some("no evidence file for this tip.");
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Some("the evidence file cannot be read.");
    };
    let Ok(rec) = serde_json::from_str::<Stored>(&text) else {
        return Some("the evidence file is unreadable.");
    };
    if rec.sha != sha {
        Some("the evidence file is for a different commit.")
    } else if rec.exit != 0 {
        Some("the stored run did not exit 0.")
    } else if norm_cmd(&rec.command) != test {
        Some("the stored command is not the `test` line in `.ljos/gates.toml`.")
    } else if rec.ts.trim().is_empty() {
        Some("the evidence file has no timestamp.")
    } else {
        None
    }
}

fn write_evidence(cwd: &Path, sha: &str, command: &str, ts: &str) -> Result<()> {
    let path = evidence_file(cwd, sha).context(
        "evidence: cannot find the git directory. Run `ljos evidence record -- <test command>` inside the repository",
    )?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::json!({
        "command": command,
        "exit": 0,
        "sha": sha,
        "ts": ts,
    });
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&body)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// A passing `ljos evidence record` for this exact sha: the file under
/// `.git/ljos-evidence/<sha>.json`, with that sha and exit 0.
fn evidence_recorded(cwd: &Path, sha: &str) -> bool {
    let Some(path) = evidence_file(cwd, sha) else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<Stored>(&text)
        .is_ok_and(|rec| rec.sha == sha && rec.exit == 0 && !rec.ts.trim().is_empty())
}

fn evidence_file(cwd: &Path, sha: &str) -> Option<PathBuf> {
    if sha.len() < 7 || sha.len() > 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let git_dir = git_text(cwd, &["rev-parse", "--git-dir"])?;
    let git_dir = PathBuf::from(git_dir.trim());
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        cwd.join(git_dir)
    };
    Some(git_dir.join("ljos-evidence").join(format!("{sha}.json")))
}

fn repo_config(cwd: &Path, token: &str) -> Result<GateConfig, super::Rule> {
    let pattern = match token {
        "git-merge-unready" => "merge-gate",
        "git-commit-identity"
        | "git-commit-message"
        | "git-commit-trailer"
        | "git-commit-signoff" => "commit-gate",
        _ => "push-gate",
    };
    let Some(root) = repo_root(cwd) else {
        return Err(deny(
            pattern,
            token,
            "Run this inside the repository checkout so the gate can read `.ljos/gates.toml` and the tip.",
        ));
    };
    load_config(&root).map_err(|e| deny(pattern, token, &e))
}

fn load_config(root: &Path) -> Result<GateConfig, String> {
    let mut cfg = GateConfig {
        test: None,
        signoff: false,
        protected: default_protected(),
        owners: Vec::new(),
        identity: None,
        identity_from: None,
    };
    for path in config_layers(root) {
        if path.is_file() {
            apply_gate_file(&mut cfg, &path)?;
        }
    }
    Ok(cfg)
}

/// User `ljos.toml`, then the seat's, then the repository's `ljos.toml`,
/// then `LJOS_CONFIG`, then `.ljos/gates.toml`. A later file overrides the
/// keys it sets. Identity is whatever the last of these files names. With
/// none of them naming a person, there is no identity gate.
fn config_layers(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(base) = user_config_base() {
        paths.push(base.join("ljos").join("ljos.toml"));
        if let Some(seat) = configured_seat() {
            paths.push(base.join("ljos").join("seats").join(seat).join("ljos.toml"));
        }
    }
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

fn apply_gate_file(cfg: &mut GateConfig, path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "`{}` cannot be read ({e}). Fix the file, then retry.",
            path.display()
        )
    })?;
    if text.trim().is_empty() {
        return Ok(());
    }
    let value: toml::Value = toml::from_str(&text).map_err(|e| {
        format!(
            "`{}` does not parse ({e}). Fix the file, then retry.",
            path.display()
        )
    })?;
    let Some(root) = value.as_table() else {
        return Err(format!(
            "`{}` is not a table. Fix the file, then retry.",
            path.display()
        ));
    };
    let gates = root.get("gates").and_then(toml::Value::as_table);
    let src = gates.unwrap_or(root);
    if src.contains_key("test") {
        let test = norm_cmd(&req_string(src, "test", path)?);
        cfg.test = (!test.is_empty()).then_some(test);
    }
    if src.contains_key("signoff") {
        cfg.signoff = req_bool(src, "signoff", path)?;
    }
    if src.contains_key("protected_branches") {
        cfg.protected = req_strings(src, "protected_branches", path)?
            .into_iter()
            .map(|name| branch_of(&name))
            .filter(|name| !name.is_empty())
            .collect();
    }
    if src.contains_key("owners") {
        cfg.owners = req_strings(src, "owners", path)?
            .into_iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
    }
    if let Some(identity) = root.get("identity").and_then(toml::Value::as_table) {
        set_identity(cfg, identity, path)?;
    } else if let Some(gates) = gates {
        if gates.contains_key("name") || gates.contains_key("email") {
            set_identity(cfg, gates, path)?;
        }
    } else if path.file_name().and_then(|name| name.to_str()) == Some("gates.toml")
        && (src.contains_key("name") || src.contains_key("email"))
    {
        set_identity(cfg, src, path)?;
    }
    Ok(())
}

fn set_identity(
    cfg: &mut GateConfig,
    table: &toml::map::Map<String, toml::Value>,
    path: &Path,
) -> Result<(), String> {
    let name = opt_string(table, "name", path)?;
    let email = opt_string(table, "email", path)?;
    match (name, email) {
        (None, None) => Ok(()),
        (Some(name), Some(email)) => {
            let name = name.trim().to_string();
            let email = email.trim().to_string();
            if name.is_empty() || email.is_empty() {
                return Err(identity_half(path));
            }
            cfg.identity = Some(Person { name, email });
            cfg.identity_from = Some(path.to_path_buf());
            Ok(())
        }
        _ => Err(identity_half(path)),
    }
}

fn identity_half(path: &Path) -> String {
    format!(
        "`{}` sets an identity with only one of `name` and `email`. Set both, then retry.",
        path.display()
    )
}

fn req_string(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    path: &Path,
) -> Result<String, String> {
    match table.get(key) {
        Some(toml::Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(format!(
            "`{}` key `{key}` must be a string. Fix the file, then retry.",
            path.display()
        )),
        None => Err(format!(
            "`{}` is missing `{key}`. Fix the file, then retry.",
            path.display()
        )),
    }
}

fn opt_string(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    path: &Path,
) -> Result<Option<String>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(_) => req_string(table, key, path).map(Some),
    }
}

fn req_bool(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    path: &Path,
) -> Result<bool, String> {
    match table.get(key) {
        Some(toml::Value::Boolean(value)) => Ok(*value),
        _ => Err(format!(
            "`{}` key `{key}` must be true or false. Fix the file, then retry.",
            path.display()
        )),
    }
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

fn is_protected(name: &str, protected: &[String]) -> bool {
    let name = branch_of(name);
    protected.iter().any(|p| p == &name)
}

/// The branches a `--force-with-lease` push asks before it replaces:
/// `main`, `master` and `develop`, and every branch `protected_branches`
/// names in the config layers [`config_layers`] reads (the user and seat
/// `ljos.toml`, the repository's `ljos.toml`, `LJOS_CONFIG` and
/// `.ljos/gates.toml`). Outside a checkout the user and seat files and
/// `LJOS_CONFIG` still count. `None` when one of them does not parse, so
/// the caller asks.
pub(crate) fn lease_protected(cwd: Option<&Path>) -> Option<Vec<String>> {
    let mut cfg = GateConfig {
        test: None,
        signoff: false,
        protected: Vec::new(),
        owners: Vec::new(),
        identity: None,
        identity_from: None,
    };
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
    let mut paths = Vec::new();
    if let Some(base) = user_config_base() {
        paths.push(base.join("ljos").join("ljos.toml"));
        if let Some(seat) = configured_seat() {
            paths.push(base.join("ljos").join("seats").join(seat).join("ljos.toml"));
        }
    }
    if let Some(path) = std::env::var_os("LJOS_CONFIG").filter(|path| !path.is_empty()) {
        paths.push(PathBuf::from(path));
    }
    paths
}

fn repo_root(cwd: &Path) -> Option<PathBuf> {
    git_text(cwd, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

fn head_sha(cwd: &Path) -> Option<String> {
    let sha = git_text(cwd, &["rev-parse", "HEAD"])?;
    (sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit())).then_some(sha)
}

fn rev(cwd: &Path, spec: &str) -> Option<String> {
    if has_subst(spec) {
        return None;
    }
    let expr = format!("{spec}^{{commit}}");
    let sha = git_text(cwd, &["rev-parse", "--verify", &expr])?;
    (sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit())).then_some(sha)
}

fn abbrev(cwd: &Path) -> Option<String> {
    let branch = git_text(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    (branch != "HEAD" && !branch.is_empty()).then_some(branch)
}

#[cfg(test)]
fn second_parent(cwd: &Path) -> Option<String> {
    git_text(cwd, &["rev-parse", "--verify", "HEAD^2"])
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

fn identity_ok(
    git: &GitCmd,
    env: &[(String, String)],
    author_flag: Option<&str>,
    person: &Person,
    from: Option<&Path>,
) -> Result<(), super::Rule> {
    let cfg = |key: &str| {
        git.config
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .or_else(|| git_config(&git.cwd, key))
            .unwrap_or_default()
    };
    let mut author_n = env_val(env, "GIT_AUTHOR_NAME").unwrap_or_else(|| cfg("user.name"));
    let mut author_e = env_val(env, "GIT_AUTHOR_EMAIL").unwrap_or_else(|| cfg("user.email"));
    let committer_n = env_val(env, "GIT_COMMITTER_NAME").unwrap_or_else(|| cfg("user.name"));
    let committer_e = env_val(env, "GIT_COMMITTER_EMAIL").unwrap_or_else(|| cfg("user.email"));
    if let Some(flag) = author_flag {
        let (name, email) = parse_author(flag);
        if !name.is_empty() {
            author_n = name;
        }
        if !email.is_empty() {
            author_e = email;
        }
    }
    let author_ok = same_person(&author_n, &author_e, person);
    let committer_ok = same_person(&committer_n, &committer_e, person);
    if author_ok && committer_ok {
        return Ok(());
    }
    let where_from = from
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "the gate config".to_string());
    Err(deny(
        "commit-gate",
        "git-commit-identity",
        &format!(
            "Configured identity is {} <{}> in {where_from}. Author is {} <{}>, committer is {} <{}>.",
            person.name,
            person.email,
            shown(&author_n),
            shown(&author_e),
            shown(&committer_n),
            shown(&committer_e)
        ),
    ))
}

fn same_person(name: &str, email: &str, person: &Person) -> bool {
    name.trim() == person.name && email.trim().eq_ignore_ascii_case(&person.email)
}

fn signoff_line(person: &Person) -> String {
    format!("Signed-off-by: {} <{}>", person.name, person.email)
}

fn shown(value: &str) -> &str {
    let value = value.trim();
    if value.is_empty() {
        "unset"
    } else {
        value
    }
}

fn parse_author(s: &str) -> (String, String) {
    let s = s.trim();
    if let Some((name, rest)) = s.rsplit_once('<') {
        let email = rest.trim().trim_end_matches('>').trim();
        (name.trim().to_string(), email.to_string())
    } else if s.contains('@') {
        (String::new(), s.to_string())
    } else {
        (s.to_string(), String::new())
    }
}

fn has_signoff(message: &str, line: &str) -> bool {
    message.lines().any(|row| row.trim() == line)
}

fn ai_marker(message: &str) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("co-authored-by") {
        return Some("Co-authored-by");
    }
    if message.to_ascii_uppercase().contains("CURSOR_AGENT") {
        return Some("CURSOR_AGENT");
    }
    if lower.contains("generated with") {
        return Some("Generated with");
    }
    if lower.contains("open in cursor") {
        return Some("Open in Cursor");
    }
    if lower.contains("generated-by:") || lower.contains("generated by cursor") {
        return Some("Generated-by");
    }
    if lower.contains("assisted-by:") {
        return Some("Assisted-by");
    }
    if lower.contains("made-with:") || lower.contains("made with cursor") {
        return Some("Made-with");
    }
    if lower.contains("helped-by:") {
        return Some("Helped-by");
    }
    if message.contains('🤖') {
        return Some("an AI footer");
    }
    None
}

fn hidden_deny() -> super::Rule {
    deny(
        "commit-gate",
        "git-commit-message",
        "Pass the message with `git commit -m TEXT` or `git commit -F FILE`.",
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

fn commit_fields(args: &[String]) -> CommitFields {
    let mut fields = CommitFields {
        messages: Vec::new(),
        files: Vec::new(),
        reuse: Vec::new(),
        trailers: Vec::new(),
        author: None,
        signoff: false,
        no_edit: false,
        amend: false,
        dry: false,
        help: false,
        hidden: false,
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            break;
        }
        if let Some(v) = a.strip_prefix("--message=") {
            fields.messages.push(v.to_string());
            i += 1;
            continue;
        }
        if let Some(v) = a.strip_prefix("--file=") {
            fields.files.push(v.to_string());
            i += 1;
            continue;
        }
        if let Some(v) = a.strip_prefix("--author=") {
            fields.author = Some(v.to_string());
            i += 1;
            continue;
        }
        if let Some(v) = a.strip_prefix("--trailer=") {
            fields.trailers.push(v.to_string());
            i += 1;
            continue;
        }
        if matches!(
            a.as_str(),
            "-m" | "--message"
                | "-F"
                | "--file"
                | "--author"
                | "--trailer"
                | "-C"
                | "--reuse-message"
                | "-c"
                | "--reedit-message"
                | "-t"
                | "--template"
        ) {
            let val = args.get(i + 1).cloned().unwrap_or_default();
            match a.as_str() {
                "-m" | "--message" => fields.messages.push(val),
                "-F" | "--file" => fields.files.push(val),
                "--author" => fields.author = Some(val),
                "--trailer" => fields.trailers.push(val),
                "-C" | "--reuse-message" => fields.reuse.push(val),
                _ => fields.hidden = true,
            }
            i += 2;
            continue;
        }
        match a.as_str() {
            "--signoff" => fields.signoff = true,
            "--no-edit" => fields.no_edit = true,
            "--amend" => fields.amend = true,
            "--dry-run" => fields.dry = true,
            "--help" | "-h" => fields.help = true,
            _ if a.starts_with("--") => {}
            _ if a.starts_with('-') && a.len() > 1 => {
                let chars: Vec<char> = a[1..].chars().collect();
                let mut k = 0;
                let mut ate = false;
                while k < chars.len() {
                    let inline: String = chars[k + 1..].iter().collect();
                    match chars[k] {
                        's' => fields.signoff = true,
                        'h' => fields.help = true,
                        'm' | 'F' | 'C' | 'c' | 't' => {
                            let (val, took) = if inline.is_empty() {
                                (args.get(i + 1).cloned().unwrap_or_default(), true)
                            } else {
                                (inline, false)
                            };
                            match chars[k] {
                                'm' => fields.messages.push(val),
                                'F' => fields.files.push(val),
                                'C' => fields.reuse.push(val),
                                _ => fields.hidden = true,
                            }
                            ate = took;
                            break;
                        }
                        _ => {}
                    }
                    k += 1;
                }
                if ate {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    fields
}

fn read_files(cwd: &Path, files: &[String]) -> Option<String> {
    let mut out = String::new();
    for file in files {
        let text = read_one(cwd, file)?;
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&text);
    }
    Some(out)
}

fn read_one(cwd: &Path, file: &str) -> Option<String> {
    if file == "-" || has_subst(file) {
        return None;
    }
    let path = if Path::new(file).is_absolute() {
        PathBuf::from(file)
    } else {
        cwd.join(file)
    };
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > 1_048_576 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn reuse_messages(cwd: &Path, revs: &[String]) -> Option<String> {
    let mut out = String::new();
    for rev in revs {
        if has_subst(rev) {
            return None;
        }
        let text = git_text(cwd, &["log", "-1", "--format=%B", "--", rev])?;
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&text);
    }
    Some(out)
}

fn is_dry(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "--dry-run"
            || a == "-n"
            || (a.starts_with('-') && !a.starts_with("--") && a.chars().skip(1).any(|c| c == 'n'))
    })
}

fn is_delete(args: &[String]) -> bool {
    if args.iter().any(|a| a == "--delete" || a == "-d") {
        return true;
    }
    let pos = positionals(args);
    !pos.is_empty()
        && pos
            .iter()
            .all(|p| p.strip_prefix('+').unwrap_or(p).starts_with(':'))
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

fn remotes(cwd: &Path) -> Vec<String> {
    git_text(cwd, &["remote"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn refspecs(cwd: &Path, args: &[String]) -> Vec<String> {
    let pos = positionals(args);
    if pos.is_empty() {
        return Vec::new();
    }
    let names = remotes(cwd);
    if names.iter().any(|r| r == &pos[0]) {
        return pos[1..].to_vec();
    }
    pos
}

fn push_dests(cwd: &Path, args: &[String], current: Option<&str>) -> Vec<String> {
    let specs = refspecs(cwd, args);
    if specs.is_empty() {
        return current.map(|c| vec![branch_of(c)]).unwrap_or_default();
    }
    specs.iter().filter_map(|s| dest_of(s, current)).collect()
}

fn dest_of(spec: &str, current: Option<&str>) -> Option<String> {
    let spec = spec.strip_prefix('+').unwrap_or(spec);
    if spec.is_empty() || spec.starts_with(':') {
        return None;
    }
    let dst = spec.split_once(':').map(|(_, d)| d).unwrap_or(spec);
    if dst.is_empty() || dst == "HEAD" {
        return current.map(branch_of);
    }
    Some(branch_of(dst))
}

fn src_of(spec: &str) -> Option<String> {
    let spec = spec.strip_prefix('+').unwrap_or(spec);
    if spec.is_empty() || spec.starts_with(':') {
        return None;
    }
    let src = spec.split_once(':').map(|(s, _)| s).unwrap_or(spec);
    if src.is_empty() {
        None
    } else {
        Some(src.to_string())
    }
}

fn pushing_current(src: &str, current: Option<&str>) -> bool {
    if src == "HEAD" {
        return true;
    }
    current.is_some_and(|c| branch_of(c) == branch_of(src))
}

fn switched_branch(args: &[String]) -> Option<String> {
    if is_help(args) {
        return None;
    }
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            return None;
        }
        if matches!(a.as_str(), "-b" | "-B" | "-c" | "-C" | "--orphan") {
            return args
                .get(i + 1)
                .map(|s| branch_of(s))
                .filter(|s| !s.is_empty());
        }
        if let Some(v) = a.strip_prefix("--orphan=") {
            return Some(branch_of(v)).filter(|s| !s.is_empty());
        }
        if matches!(a.as_str(), "--track" | "-t")
            && args.get(i + 1).is_some_and(|n| !n.starts_with('-'))
        {
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        if a == "." || a.contains('*') {
            return None;
        }
        return Some(branch_of(a));
    }
    None
}

fn is_help(args: &[String]) -> bool {
    args.iter().any(|a| a == "--help" || a == "-h")
}

fn review_pr(
    cwd: &Path,
    spec: &str,
    repo: Option<&str>,
    expect: Option<&str>,
    action: &str,
) -> Result<(), super::Rule> {
    let args = pr_view_args(spec, repo, "number,headRefOid,statusCheckRollup,url,state");
    match gh_read(cwd, &args) {
        Ok(view) => review_rollup(cwd, &view, expect, action),
        Err(err) if err.rollup_denied => review_pr_rest(cwd, spec, repo, expect, action),
        Err(err) => Err(merge_io(action, &err.text)),
    }
}

fn pr_view_args(spec: &str, repo: Option<&str>, fields: &str) -> Vec<String> {
    let mut args = vec!["pr".to_string(), "view".to_string()];
    if !spec.is_empty() {
        args.push(spec.to_string());
    }
    args.push("--json".into());
    args.push(fields.to_string());
    if let Some(repo) = repo {
        args.push("--repo".into());
        args.push(repo.to_string());
    }
    args
}

struct PrHead {
    number: u64,
    sha: String,
    url: String,
}

fn pr_head(view: &Value, action: &str) -> Result<PrHead, super::Rule> {
    let number = view["number"]
        .as_u64()
        .ok_or_else(|| merge_io(action, "the pull request number was missing"))?;
    let sha = view["headRefOid"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| merge_io(action, "the head sha was missing"))?
        .to_string();
    let url = view["url"].as_str().unwrap_or("").to_string();
    Ok(PrHead { number, sha, url })
}

fn same_head(
    action: &str,
    sha: &str,
    number: u64,
    expect: Option<&str>,
) -> Result<(), super::Rule> {
    let Some(expect) = expect else {
        return Ok(());
    };
    if sha == expect {
        return Ok(());
    }
    Err(merge_deny(
        action,
        expect,
        &number.to_string(),
        &format!("The pull request head is {sha}, but this merge publishes {expect}."),
    ))
}

fn review_rollup(
    cwd: &Path,
    view: &Value,
    expect: Option<&str>,
    action: &str,
) -> Result<(), super::Rule> {
    let head = pr_head(view, action)?;
    same_head(action, &head.sha, head.number, expect)?;
    let rollup = &view["statusCheckRollup"];
    if let Some(problem) = ci_problem(rollup) {
        return Err(merge_deny(
            action,
            &head.sha,
            &head.number.to_string(),
            &name_source(&problem, "rollup"),
        ));
    }
    let Some((owner, name)) = owner_repo(&head.url) else {
        return Err(merge_io(
            action,
            "the pull request URL did not name an owner and repository",
        ));
    };
    finish_review(
        cwd,
        &Reviewed {
            action,
            number: head.number,
            sha: &head.sha,
            owner: &owner,
            name: &name,
            absent: ci_absent(rollup),
            source: "rollup",
        },
    )
}

/// The rollup read was denied to this token. Workflow runs and the combined
/// commit status carry the same CI result. Both empty is the no-CI ask. A
/// failed read keeps the refusal that gh could not read the pull request.
fn review_pr_rest(
    cwd: &Path,
    spec: &str,
    repo: Option<&str>,
    expect: Option<&str>,
    action: &str,
) -> Result<(), super::Rule> {
    let args = pr_view_args(spec, repo, "number,headRefOid,url,state");
    let view = gh_json(cwd, &args).map_err(|e| merge_io(action, &e))?;
    let head = pr_head(&view, action)?;
    same_head(action, &head.sha, head.number, expect)?;
    let Some((owner, name)) = owner_repo(&head.url) else {
        return Err(merge_io(
            action,
            "the pull request URL did not name an owner and repository",
        ));
    };
    let runs = workflow_runs(cwd, &owner, &name, &head.sha).map_err(|e| merge_io(action, &e))?;
    let status = commit_status(cwd, &owner, &name, &head.sha).map_err(|e| merge_io(action, &e))?;
    let problem = rest_problem(&runs, &status).map_err(|e| merge_io(action, &e))?;
    if let Some(problem) = problem {
        return Err(merge_deny(
            action,
            &head.sha,
            &head.number.to_string(),
            &name_source(&problem, "REST fallback"),
        ));
    }
    finish_review(
        cwd,
        &Reviewed {
            action,
            number: head.number,
            sha: &head.sha,
            owner: &owner,
            name: &name,
            absent: rest_absent(&runs, &status),
            source: "REST fallback",
        },
    )
}

struct Reviewed<'a> {
    action: &'a str,
    number: u64,
    sha: &'a str,
    owner: &'a str,
    name: &'a str,
    absent: bool,
    source: &'a str,
}

fn finish_review(cwd: &Path, review: &Reviewed<'_>) -> Result<(), super::Rule> {
    let threads = bugbot_query(cwd, review.owner, review.name, review.number)
        .map_err(|e| merge_io(review.action, &e))?;
    if let Some(problem) = bugbot_problem(&threads).map_err(|e| merge_io(review.action, &e))? {
        return Err(merge_deny(
            review.action,
            review.sha,
            &review.number.to_string(),
            &name_source(&problem, review.source),
        ));
    }
    if review.absent {
        return Err(merge_ask(cwd, review.sha, review.source));
    }
    Ok(())
}

fn name_source(problem: &str, source: &str) -> String {
    format!("{problem}. Source: {source}.")
}

const RUN_PAGE: usize = 100;

fn workflow_runs(cwd: &Path, owner: &str, name: &str, sha: &str) -> Result<Vec<Value>, String> {
    let mut all = Vec::new();
    let mut page = 1u32;
    loop {
        let body = gh_json(
            cwd,
            &[
                "api".into(),
                format!("repos/{owner}/{name}/actions/runs"),
                "-f".into(),
                format!("head_sha={sha}"),
                "-f".into(),
                format!("per_page={RUN_PAGE}"),
                "-f".into(),
                format!("page={page}"),
            ],
        )?;
        let Some(runs) = body.get("workflow_runs").and_then(Value::as_array) else {
            return Err("the workflow run list was missing".into());
        };
        let total = body.get("total_count").and_then(Value::as_u64);
        if runs.is_empty() {
            if total.is_some_and(|t| (all.len() as u64) < t) {
                return Err("the workflow run list ended early".into());
            }
            break;
        }
        all.extend(runs.iter().cloned());
        let more = match total {
            Some(total) => (all.len() as u64) < total,
            None => runs.len() >= RUN_PAGE,
        };
        if !more {
            break;
        }
        if page == 100 {
            return Err("the workflow run list did not end".into());
        }
        page += 1;
    }
    Ok(all)
}

fn commit_status(cwd: &Path, owner: &str, name: &str, sha: &str) -> Result<Value, String> {
    gh_json(
        cwd,
        &[
            "api".into(),
            format!("repos/{owner}/{name}/commits/{sha}/status"),
        ],
    )
}

fn rest_absent(runs: &[Value], status: &Value) -> bool {
    runs.is_empty()
        && status
            .get("statuses")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
}

fn rest_problem(runs: &[Value], status: &Value) -> Result<Option<String>, String> {
    let mut bad = Vec::new();
    for run in runs {
        if let Some(fault) = run_fault(run) {
            bad.push(fault);
        }
    }
    if let Some(fault) = status_fault(status)? {
        bad.push(fault);
    }
    if bad.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("{} not green", bad.join(", "))))
    }
}

fn run_fault(run: &Value) -> Option<String> {
    let name = run["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("run");
    let status = run["status"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let conclusion = run["conclusion"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if matches!(
        status.as_str(),
        "queued" | "in_progress" | "in-progress" | "waiting" | "requested" | "pending"
    ) {
        return Some(format!("{name}={status} (pending)"));
    }
    if status == "completed" && matches!(conclusion.as_str(), "success" | "neutral" | "skipped") {
        return None;
    }
    let shown = if !conclusion.is_empty() {
        conclusion
    } else if !status.is_empty() {
        status
    } else {
        "unknown".to_string()
    };
    Some(format!("{name}={shown}"))
}

fn status_fault(status: &Value) -> Result<Option<String>, String> {
    let Some(list) = status.get("statuses").and_then(Value::as_array) else {
        return Err("the commit status list was missing".into());
    };
    if list.is_empty() {
        return Ok(None);
    }
    let state = status["state"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if state == "success" {
        return Ok(None);
    }
    let shown = if state.is_empty() {
        "unknown".to_string()
    } else {
        state
    };
    if shown == "pending" {
        Ok(Some(format!("commit status={shown} (pending)")))
    } else {
        Ok(Some(format!("commit status={shown}")))
    }
}

/// An ask for a head sha with no CI checks. A passing local record for
/// that exact sha is named. The merge still asks. `source` is `rollup` or
/// `REST fallback`.
fn merge_ask(cwd: &Path, sha: &str, source: &str) -> super::Rule {
    let mut detail = format!("no CI ran on head sha {sha}. Source: {source}.");
    if evidence_recorded(cwd, sha) {
        detail.push_str(&format!(" A local evidence record exists for {sha}."));
    }
    super::Rule {
        pattern: "merge-gate".into(),
        verdict: "ask".into(),
        reason: format!("git-merge-unready: {detail}"),
    }
}

fn merge_deny(action: &str, sha: &str, pr: &str, problem: &str) -> super::Rule {
    let checks = if pr.is_empty() {
        "gh pr checks".to_string()
    } else {
        format!("gh pr checks {pr}")
    };
    deny(
        "merge-gate",
        "git-merge-unready",
        &format!(
            "Refusing `{action}`. {problem} Head sha {sha}. Wait until `{checks}` is green and every Cursor Bugbot thread is resolved or has a reply, then run the merge again."
        ),
    )
}

fn merge_io(action: &str, err: &str) -> super::Rule {
    deny(
        "merge-gate",
        "git-merge-unready",
        &format!(
            "Refusing `{action}`. {err}. Sign in with `gh auth login` if gh could not read the pull request, wait until CI is green on the head sha and every Cursor Bugbot thread is resolved or has a reply, then run the merge again."
        ),
    )
}

/// No checks on the head sha: a null rollup or an empty list. That is an
/// ask in [`review_pr`], not a denial.
fn ci_absent(rollup: &Value) -> bool {
    rollup.is_null() || rollup.as_array().is_some_and(Vec::is_empty)
}

fn ci_problem(rollup: &Value) -> Option<String> {
    // No checks at all is an ask, decided in [`review_pr`]. A red or
    // pending check is a denial.
    if ci_absent(rollup) {
        return None;
    }
    let arr = rollup.as_array()?;
    let mut bad = Vec::new();
    for check in arr {
        let name = check["name"]
            .as_str()
            .or_else(|| check["context"].as_str())
            .unwrap_or("check");
        let status = check["status"].as_str().unwrap_or("").to_ascii_uppercase();
        let conclusion = check["conclusion"]
            .as_str()
            .unwrap_or("")
            .to_ascii_uppercase();
        let state = check["state"].as_str().unwrap_or("").to_ascii_uppercase();
        let shown = if !conclusion.is_empty() && (status.is_empty() || status == "COMPLETED") {
            conclusion
        } else if !status.is_empty() && status != "COMPLETED" {
            status
        } else if !state.is_empty() {
            state
        } else {
            "UNKNOWN".into()
        };
        if !matches!(shown.as_str(), "SUCCESS" | "NEUTRAL" | "SKIPPED") {
            bad.push(format!("{name}={shown}"));
        }
    }
    if bad.is_empty() {
        None
    } else {
        Some(format!("{} not green", bad.join(", ")))
    }
}

fn bugbot_query(cwd: &Path, owner: &str, name: &str, number: u64) -> Result<Value, String> {
    let query = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100){pageInfo{hasNextPage}nodes{isResolved comments(first:50){nodes{author{login}body}}}}}}}";
    gh_json(
        cwd,
        &[
            "api".into(),
            "graphql".into(),
            "-f".into(),
            format!("query={query}"),
            "-f".into(),
            format!("owner={owner}"),
            "-f".into(),
            format!("name={name}"),
            "-F".into(),
            format!("number={number}"),
        ],
    )
}

fn bugbot_problem(body: &Value) -> Result<Option<String>, String> {
    if let Some(errors) = body.get("errors") {
        if errors.as_array().is_some_and(|a| !a.is_empty()) {
            return Err(brief(&errors.to_string()));
        }
    }
    let Some(nodes) = body
        .pointer("/data/repository/pullRequest/reviewThreads/nodes")
        .and_then(Value::as_array)
    else {
        return Err("the Bugbot thread list was missing".into());
    };
    if body
        .pointer("/data/repository/pullRequest/reviewThreads/pageInfo/hasNextPage")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Err("more than 100 review threads came back".into());
    }
    let mut open = Vec::new();
    for node in nodes {
        let Some(comments) = node.pointer("/comments/nodes").and_then(Value::as_array) else {
            continue;
        };
        let Some(first) = comments.first() else {
            continue;
        };
        let login = first
            .pointer("/author/login")
            .and_then(Value::as_str)
            .unwrap_or("");
        let text = first["body"].as_str().unwrap_or("");
        if !is_bugbot(login, text) {
            continue;
        }
        if node["isResolved"].as_bool() == Some(true) {
            continue;
        }
        let reply = comments.iter().skip(1).any(|c| {
            let login = c
                .pointer("/author/login")
                .and_then(Value::as_str)
                .unwrap_or("");
            !bugbot_login(login)
        });
        if reply {
            continue;
        }
        let snippet: String = text.chars().take(80).collect();
        open.push(snippet);
    }
    if open.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!(
            "{} open Cursor Bugbot thread(s) with no reply ({})",
            open.len(),
            open.join(" | ")
        )))
    }
}

fn bugbot_login(login: &str) -> bool {
    let login = login.to_ascii_lowercase();
    login.contains("bugbot") || login == "cursor[bot]" || login.starts_with("cursor[")
}

fn is_bugbot(login: &str, body: &str) -> bool {
    bugbot_login(login) || body.contains("Bugbot") || body.contains("CURSOR_BUGBOT")
}

fn owner_repo(url: &str) -> Option<(String, String)> {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let _host = parts.next()?;
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.trim_end_matches(".git").to_string();
    (!owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
}

fn api_merge(words: &[String]) -> Option<ApiMerge> {
    if words.is_empty() {
        return None;
    }
    let base = super::base_name(&words[0]);
    match base {
        "gh" => gh_merge(words),
        "curl" | "wget" => words
            .iter()
            .find_map(|w| pulls_merge(w))
            .map(|(repo, num)| ApiMerge {
                repo: Some(repo),
                spec: num,
            }),
        "python" | "python3" | "node" | "ruby" | "perl" | "bun" => {
            if words.iter().any(|w| {
                w.contains("mergePullRequest")
                    || w.contains("merge_pull_request")
                    || pulls_merge(w).is_some()
            }) {
                if let Some((repo, num)) = words.iter().find_map(|w| pulls_merge(w)) {
                    Some(ApiMerge {
                        repo: Some(repo),
                        spec: num,
                    })
                } else {
                    Some(ApiMerge {
                        repo: None,
                        spec: String::new(),
                    })
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn gh_merge(words: &[String]) -> Option<ApiMerge> {
    if words.get(1).map(String::as_str) == Some("pr")
        && words.get(2).map(String::as_str) == Some("merge")
    {
        let (repo, spec) = gh_merge_args(words);
        return Some(ApiMerge { repo, spec });
    }
    if let Some((repo, num)) = words.iter().find_map(|w| pulls_merge(w)) {
        return Some(ApiMerge {
            repo: Some(repo),
            spec: num,
        });
    }
    if words
        .iter()
        .any(|w| w.contains("mergePullRequest") || w.contains("merge_pull_request"))
    {
        return Some(ApiMerge {
            repo: None,
            spec: String::new(),
        });
    }
    None
}

fn gh_merge_args(words: &[String]) -> (Option<String>, String) {
    let mut repo = None;
    let mut spec = String::new();
    let mut i = 3;
    while i < words.len() {
        let w = &words[i];
        if w == "--repo" || w == "-R" {
            repo = words.get(i + 1).cloned();
            i += 2;
            continue;
        }
        if let Some(v) = w.strip_prefix("--repo=") {
            repo = Some(v.to_string());
            i += 1;
            continue;
        }
        if w.starts_with('-') {
            i += 1;
            continue;
        }
        if spec.is_empty() {
            spec = w.clone();
        }
        i += 1;
    }
    (repo, spec)
}

fn pulls_merge(text: &str) -> Option<(String, String)> {
    let mut search = 0;
    while let Some(rel) = text[search..].find("/pulls/") {
        let at = search + rel;
        let rest = &text[at + "/pulls/".len()..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if num.is_empty() {
            search = at + "/pulls/".len();
            continue;
        }
        let after = &rest[num.len()..];
        if !after.starts_with("/merge") {
            search = at + "/pulls/".len();
            continue;
        }
        let before = &text[..at];
        let segs: Vec<&str> = before
            .rsplit('/')
            .filter(|s| !s.is_empty())
            .take(3)
            .collect();
        if segs.len() < 2 {
            return None;
        }
        let repo = segs[0].trim();
        let owner = segs[1].trim();
        if owner.is_empty() || repo.is_empty() || owner == "repos" {
            search = at + "/pulls/".len();
            continue;
        }
        return Some((format!("{owner}/{repo}"), num));
    }
    None
}

fn gh_program() -> PathBuf {
    GH_BIN
        .with(|g| g.borrow().clone())
        .unwrap_or_else(|| PathBuf::from("gh"))
}

struct GhFail {
    text: String,
    /// The rollup field was refused with the fine-grained token phrase.
    /// Any other failure leaves this false.
    rollup_denied: bool,
}

fn gh_json(cwd: &Path, args: &[String]) -> Result<Value, String> {
    gh_read(cwd, args).map_err(|err| err.text)
}

fn gh_read(cwd: &Path, args: &[String]) -> Result<Value, GhFail> {
    let mut cmd = Command::new(gh_program());
    cmd.current_dir(cwd).args(args);
    let out = run_out(&mut cmd, super::guard_timeout()).map_err(|text| GhFail {
        text,
        rollup_denied: false,
    })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let raw_out = String::from_utf8_lossy(&out.stdout);
        let rollup_denied = rollup_permission_denied(&err) || rollup_permission_denied(&raw_out);
        let text = brief(&err);
        let text = if text.is_empty() {
            format!("gh exited {}", out.status.code().unwrap_or(1))
        } else {
            text
        };
        return Err(GhFail {
            text,
            rollup_denied,
        });
    }
    serde_json::from_slice(&out.stdout).map_err(|e| GhFail {
        text: format!("gh did not return JSON ({e})"),
        rollup_denied: false,
    })
}

/// Fine-grained tokens have no Checks permission. `gh pr view` then fails
/// with this phrase when the json fields include `statusCheckRollup`.
/// A different failure is not this case.
fn rollup_permission_denied(err: &str) -> bool {
    let flat = err.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.to_ascii_lowercase()
        .contains("resource not accessible by personal access token")
}

fn brief(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text: String = one.chars().take(240).collect();
    if one.chars().count() > 240 {
        text.push_str("...");
    }
    text
}

fn run_out(cmd: &mut Command, deadline: Duration) -> Result<Output, String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().ok_or("gh returned no output")?;
    let mut stderr = child.stderr.take().ok_or("gh returned no errors")?;
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stderr, &mut buf);
        buf
    });
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_thread.join();
                let _ = err_thread.join();
                return Err("did not answer in time".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn remote_hidden(line: &str) -> Option<super::Rule> {
    remote_hidden_at(line, 0)
}

fn remote_hidden_at(line: &str, depth: usize) -> Option<super::Rule> {
    if depth > 4 {
        return Some(deny(
            "remote-gate",
            "git-remote-hidden",
            "Run the git command in this checkout.",
        ));
    }
    for sub in super::nested_lines(line, 0) {
        for seg in super::raw_segments(&sub) {
            let words = super::shell_words(&seg);
            let (at, _) = super::prefix_end(&words);
            if at >= words.len() {
                continue;
            }
            let cmd = &words[at..];
            if super::base_name(&cmd[0]) != "ssh" {
                continue;
            }
            let refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
            let Some(remote) = super::ssh_remote_command(&refs) else {
                continue;
            };
            if gate_command(&remote) || remote_hidden_at(&remote, depth + 1).is_some() {
                return Some(deny(
                    "remote-gate",
                    "git-remote-hidden",
                    "Run the commit or the merge in this checkout, where the gate can read it.",
                ));
            }
        }
    }
    None
}

fn gate_command(line: &str) -> bool {
    for inv in invocations(line, Path::new("."), 0) {
        if let Some(git) = git_of(&inv) {
            if git.shell.as_deref().is_some_and(gate_command) {
                return true;
            }
            // A push and a local merge are not held, so running them
            // elsewhere hides nothing. A commit's message and a forge
            // merge are.
            if git.sub == "commit" {
                return true;
            }
        }
        if api_merge(&inv.words).is_some() {
            return true;
        }
    }
    false
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
            let env = assignments(&words[..at]);
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
                env,
                words: cmd,
                depth,
            });
        }
    }
    out
}

fn assignments(words: &[String]) -> Vec<(String, String)> {
    words
        .iter()
        .filter_map(|w| {
            let (k, v) = w.split_once('=')?;
            if k.is_empty()
                || k.starts_with('-')
                || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return None;
            }
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

fn env_val(env: &[(String, String)], key: &str) -> Option<String> {
    env.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var(key).ok())
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

    fn tip(dir: &Path) -> String {
        head_sha(dir).unwrap()
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

    struct GhHold {
        _dir: tempfile::TempDir,
    }

    impl Drop for GhHold {
        fn drop(&mut self) {
            GH_BIN.with(|g| *g.borrow_mut() = None);
        }
    }

    fn stub_gh(view: &str, threads: &str, list: &str) -> GhHold {
        stub_gh_at(view, threads, list, false)
    }

    fn stub_gh_unreadable() -> GhHold {
        stub_gh_at("", "", "", true)
    }

    fn stub_gh_at(view: &str, threads: &str, list: &str, fail: bool) -> GhHold {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("view.json"), view).unwrap();
        std::fs::write(dir.path().join("threads.json"), threads).unwrap();
        std::fs::write(dir.path().join("list.json"), list).unwrap();
        if fail {
            std::fs::write(dir.path().join("fail"), "1").unwrap();
        }
        hold_gh(dir)
    }

    fn hold_gh(dir: tempfile::TempDir) -> GhHold {
        let script = r#"#!/bin/sh
here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
if [ -f "$here/fail" ]; then
  echo "HTTP 401: Bad credentials" >&2
  exit 1
fi
has_rollup=0
for a in "$@"; do
  case "$a" in
    *statusCheckRollup*) has_rollup=1 ;;
  esac
done
if [ "$has_rollup" -eq 1 ] && [ -f "$here/rollup-error" ]; then
  cat "$here/rollup-error" >&2
  exit 1
fi
if [ "$has_rollup" -eq 1 ] && [ -f "$here/rollup-error-out" ]; then
  cat "$here/rollup-error-out"
  exit 1
fi
case "$*" in
  *graphql*) cat "$here/threads.json" ;;
  *"pr list"*) cat "$here/list.json" ;;
  *"pr view"*)
    if [ "$has_rollup" -eq 0 ] && [ -f "$here/meta-error" ]; then
      cat "$here/meta-error" >&2
      exit 1
    fi
    cat "$here/view.json"
    ;;
  *actions/runs*)
    if [ -f "$here/runs-error" ]; then
      cat "$here/runs-error" >&2
      exit 1
    fi
    case "$*" in
      *repos/acme/widgets/actions/runs*) ;;
      *) echo "workflow runs were not read for the pull request repository" >&2; exit 1 ;;
    esac
    if [ -f "$here/want-sha" ]; then
      want=$(cat "$here/want-sha")
      found=0
      for a in "$@"; do
        case "$a" in
          "head_sha=$want") found=1 ;;
        esac
      done
      if [ "$found" -eq 0 ]; then
        echo "workflow runs were not read for the head sha" >&2
        exit 1
      fi
    fi
    page=1
    for a in "$@"; do
      case "$a" in
        page=*) page=${a#page=} ;;
      esac
    done
    if [ -f "$here/runs-$page.json" ]; then
      cat "$here/runs-$page.json"
    else
      echo "workflow run page $page failed" >&2
      exit 1
    fi
    ;;
  *commits/*/status*)
    if [ -f "$here/status-error" ]; then
      cat "$here/status-error" >&2
      exit 1
    fi
    if [ -f "$here/want-sha" ]; then
      want=$(cat "$here/want-sha")
      case "$*" in
        *"commits/$want/status"*) ;;
        *) echo "commit status was not read for the head sha" >&2; exit 1 ;;
      esac
    fi
    case "$*" in
      *repos/acme/widgets/commits/*) ;;
      *) echo "commit status was not read for the pull request repository" >&2; exit 1 ;;
    esac
    cat "$here/status.json"
    ;;
  *) echo "stub gh: $*" >&2; exit 1 ;;
esac
"#;
        let bin = dir.path().join("gh");
        std::fs::write(&bin, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        GH_BIN.with(|g| *g.borrow_mut() = Some(bin));
        GhHold { _dir: dir }
    }

    fn green_view(sha: &str) -> String {
        serde_json::json!({
            "number": 7,
            "headRefOid": sha,
            "state": "OPEN",
            "url": "https://github.com/acme/widgets/pull/7",
            "statusCheckRollup": [{
                "name": "ci",
                "status": "COMPLETED",
                "conclusion": "SUCCESS"
            }]
        })
        .to_string()
    }

    fn red_view(sha: &str) -> String {
        serde_json::json!({
            "number": 7,
            "headRefOid": sha,
            "state": "OPEN",
            "url": "https://github.com/acme/widgets/pull/7",
            "statusCheckRollup": [{
                "name": "ci",
                "status": "COMPLETED",
                "conclusion": "FAILURE"
            }]
        })
        .to_string()
    }

    fn rollup_view(sha: &str, rollup: serde_json::Value) -> String {
        serde_json::json!({
            "number": 7,
            "headRefOid": sha,
            "state": "OPEN",
            "url": "https://github.com/acme/widgets/pull/7",
            "statusCheckRollup": rollup,
        })
        .to_string()
    }

    fn pending_view(sha: &str) -> String {
        rollup_view(
            sha,
            serde_json::json!([{
                "name": "ci",
                "status": "IN_PROGRESS",
                "conclusion": null
            }]),
        )
    }

    const ROLLUP_DENIED: &str = "GraphQL: Resource not accessible by personal access token (repository.pullRequest.statusCheckRollup)\n";

    struct RestGh {
        sha: String,
        threads: String,
        list: String,
        pages: Vec<String>,
        status: String,
        rollup_error: Option<String>,
        rollup_stdout: Option<String>,
        meta_error: Option<String>,
        runs_error: Option<String>,
        status_error: Option<String>,
        view: Option<String>,
    }

    fn rest_gh(sha: &str, pages: Vec<String>, status: String) -> RestGh {
        RestGh {
            sha: sha.to_string(),
            threads: quiet_threads(),
            list: "[]".to_string(),
            pages,
            status,
            rollup_error: Some(ROLLUP_DENIED.to_string()),
            rollup_stdout: None,
            meta_error: None,
            runs_error: None,
            status_error: None,
            view: None,
        }
    }

    fn stub_rest(spec: &RestGh) -> GhHold {
        let dir = tempfile::tempdir().unwrap();
        let view = spec.view.clone().unwrap_or_else(|| meta_view(&spec.sha));
        let root = dir.path();
        std::fs::write(root.join("view.json"), view).unwrap();
        std::fs::write(root.join("threads.json"), &spec.threads).unwrap();
        std::fs::write(root.join("list.json"), &spec.list).unwrap();
        std::fs::write(root.join("status.json"), &spec.status).unwrap();
        std::fs::write(root.join("want-sha"), &spec.sha).unwrap();
        for (i, page) in spec.pages.iter().enumerate() {
            std::fs::write(root.join(format!("runs-{}.json", i + 1)), page).unwrap();
        }
        if let Some(text) = &spec.rollup_error {
            std::fs::write(root.join("rollup-error"), text).unwrap();
        }
        if let Some(text) = &spec.rollup_stdout {
            std::fs::write(root.join("rollup-error-out"), text).unwrap();
        }
        if let Some(text) = &spec.meta_error {
            std::fs::write(root.join("meta-error"), text).unwrap();
        }
        if let Some(text) = &spec.runs_error {
            std::fs::write(root.join("runs-error"), text).unwrap();
        }
        if let Some(text) = &spec.status_error {
            std::fs::write(root.join("status-error"), text).unwrap();
        }
        hold_gh(dir)
    }

    fn meta_view(sha: &str) -> String {
        serde_json::json!({
            "number": 7,
            "headRefOid": sha,
            "state": "OPEN",
            "url": "https://github.com/acme/widgets/pull/7",
        })
        .to_string()
    }

    fn workflow(name: &str, status: &str, conclusion: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "status": status,
            "conclusion": conclusion,
        })
    }

    fn run_page(runs: serde_json::Value) -> String {
        let n = runs.as_array().map(|a| a.len()).unwrap_or(0);
        run_page_total(n as u64, runs)
    }

    fn run_page_total(total: u64, runs: serde_json::Value) -> String {
        serde_json::json!({
            "total_count": total,
            "workflow_runs": runs,
        })
        .to_string()
    }

    fn no_runs() -> String {
        run_page(serde_json::json!([]))
    }

    fn commit_status_body(state: &str, statuses: serde_json::Value) -> String {
        let n = statuses.as_array().map(|a| a.len()).unwrap_or(0);
        serde_json::json!({
            "state": state,
            "statuses": statuses,
            "total_count": n,
        })
        .to_string()
    }

    fn no_statuses() -> String {
        commit_status_body("pending", serde_json::json!([]))
    }

    fn success_status() -> String {
        commit_status_body(
            "success",
            serde_json::json!([{"context": "ci", "state": "success"}]),
        )
    }

    fn status_state(state: &str) -> String {
        commit_status_body(
            state,
            serde_json::json!([{"context": "ci", "state": state}]),
        )
    }

    fn green_runs() -> String {
        run_page(serde_json::json!([
            workflow("unit", "completed", Some("success")),
            workflow("docs", "completed", Some("neutral")),
            workflow("nightly", "completed", Some("skipped")),
        ]))
    }

    fn merge_rule(dir: &Path) -> super::super::Rule {
        guard_verdict("gh pr merge 7", Some(dir), &[]).expect("the merge gate spoke")
    }

    fn assert_rest_deny(rule: &super::super::Rule, needle: &str) {
        assert_eq!(rule.verdict, "deny", "{}", rule.reason);
        assert_eq!(rule.pattern, "merge-gate");
        assert!(rule.reason.contains(needle), "{}", rule.reason);
        assert!(
            rule.reason.contains("Source: REST fallback"),
            "{}",
            rule.reason
        );
        assert!(!rule.reason.contains("no CI"), "{}", rule.reason);
        assert!(
            !rule.reason.contains("could not read the pull request"),
            "{}",
            rule.reason
        );
    }

    fn assert_unreadable(rule: &super::super::Rule) {
        assert_eq!(rule.verdict, "deny", "{}", rule.reason);
        assert_eq!(rule.pattern, "merge-gate");
        assert!(
            rule.reason.contains("could not read the pull request"),
            "{}",
            rule.reason
        );
        assert!(rule.reason.contains("gh auth login"), "{}", rule.reason);
        assert!(!rule.reason.contains("no CI"), "{}", rule.reason);
        assert!(!rule.reason.contains("CI is missing"), "{}", rule.reason);
        assert!(
            !rule.reason.contains("Source: REST fallback"),
            "{}",
            rule.reason
        );
        assert!(!rule.reason.contains("Source: rollup"), "{}", rule.reason);
    }

    fn assert_rest_ask(rule: &super::super::Rule, sha: &str) {
        assert_eq!(rule.verdict, "ask", "{}", rule.reason);
        assert_eq!(rule.pattern, "merge-gate");
        assert!(rule.reason.contains("no CI ran"), "{}", rule.reason);
        assert!(rule.reason.contains(sha), "{}", rule.reason);
        assert!(
            rule.reason.contains("Source: REST fallback"),
            "{}",
            rule.reason
        );
    }

    fn quiet_threads() -> String {
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": {
                        "reviewThreads": {
                            "pageInfo": {"hasNextPage": false},
                            "nodes": []
                        }
                    }
                }
            }
        })
        .to_string()
    }

    fn human_thread() -> String {
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": {
                        "reviewThreads": {
                            "pageInfo": {"hasNextPage": false},
                            "nodes": [{
                                "isResolved": false,
                                "comments": {"nodes": [{
                                    "author": {"login": "octocat"},
                                    "body": "please rename this"
                                }]}
                            }]
                        }
                    }
                }
            }
        })
        .to_string()
    }

    fn threads(resolved: bool, reply: bool) -> String {
        let mut comments = vec![serde_json::json!({
            "author": {"login": "cursor[bot]"},
            "body": "Bugbot: check the null"
        })];
        if reply {
            comments.push(serde_json::json!({
                "author": {"login": "rgoswami"},
                "body": "fixed"
            }));
        }
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": {
                        "reviewThreads": {
                            "pageInfo": {"hasNextPage": false},
                            "nodes": [{
                                "isResolved": resolved,
                                "comments": {"nodes": comments}
                            }]
                        }
                    }
                }
            }
        })
        .to_string()
    }

    fn payloads(command: &str) -> Vec<(&'static str, String)> {
        vec![
            (
                "claude",
                serde_json::json!({
                    "hook_event_name": "PreToolUse",
                    "tool_name": "Bash",
                    "tool_input": {"command": command}
                })
                .to_string(),
            ),
            (
                "codex",
                serde_json::json!({
                    "hook_event_name": "PreToolUse",
                    "session_id": "c-1",
                    "turn_id": "t-1",
                    "tool_name": "Bash",
                    "tool_input": {"command": command}
                })
                .to_string(),
            ),
            (
                "opencode",
                serde_json::json!({
                    "hook_event_name": "pre_tool_call",
                    "tool_name": "bash",
                    "tool_input": {"command": command},
                    "session_id": "h-1",
                    "extra": {}
                })
                .to_string(),
            ),
            (
                "agy",
                serde_json::json!({
                    "conversationId": "agy-1",
                    "toolCall": {"name": "run_command", "args": {"CommandLine": command}}
                })
                .to_string(),
            ),
            (
                "cursor-shell",
                serde_json::json!({
                    "cursor_version": "2.4.0",
                    "conversation_id": "c-1",
                    "hook_event_name": "beforeShellExecution",
                    "command": command
                })
                .to_string(),
            ),
            (
                "cursor-tool",
                serde_json::json!({
                    "cursor_version": "2.4.0",
                    "conversation_id": "c-1",
                    "hook_event_name": "preToolUse",
                    "tool_name": "Shell",
                    "tool_input": {"command": command}
                })
                .to_string(),
            ),
        ]
    }

    fn answers(command: &str, cwd: &Path, rules: &[super::super::Rule]) -> Vec<(String, String)> {
        payloads(command)
            .into_iter()
            .map(|(name, input)| {
                let call = super::super::hook_call(&input);
                assert_eq!(call.event, "PreToolUse", "{name}");
                assert_eq!(call.cue, command, "{name} cue {}", call.cue);
                let verdict = guard_verdict(&call.cue, Some(cwd), rules);
                let answer = super::super::hook_output_ruled(&call, "", verdict.as_ref());
                (name.to_string(), answer)
            })
            .collect()
    }

    fn refuses(answer: &str) -> bool {
        if super::super::answer_denies(answer) {
            return true;
        }
        // opencode's only refusal is `decision: block`.
        serde_json::from_str::<serde_json::Value>(answer.trim())
            .ok()
            .is_some_and(|v| v.get("decision").and_then(serde_json::Value::as_str) == Some("block"))
    }

    fn assert_denies(rows: &[(String, String)], needle: &str) {
        for (name, answer) in rows {
            assert!(refuses(answer), "{name} allowed: {answer}");
            assert!(answer.contains(needle), "{name} missing {needle}: {answer}");
        }
    }

    fn assert_allows(rows: &[(String, String)]) {
        for (name, answer) in rows {
            assert!(
                !super::super::answer_denies(answer),
                "{name} refused: {answer}"
            );
        }
    }

    #[test]
    fn ordinary_commands_are_not_blocked() {
        let dir = repo();
        for line in ["ls", "cargo test --workspace", "git status", "git diff"] {
            let rows = answers(line, dir.path(), &[]);
            assert_allows(&rows);
            assert!(
                guard_verdict(line, Some(dir.path()), &[]).is_none(),
                "{line}"
            );
        }
    }

    #[test]
    fn a_clean_commit_is_allowed_and_a_bad_one_is_refused_on_every_harness() {
        let dir = repo();
        let allow = answers("git commit -m \"note the seat\"", dir.path(), &[]);
        assert_allows(&allow);
        let refuse = answers(
            "git commit -m \"Co-authored-by: Cursor <cursoragent@cursor.com>\"",
            dir.path(),
            &[],
        );
        assert_denies(&refuse, "Co-authored-by");
        assert_denies(&refuse, "git commit -m TEXT");
    }

    /// The detail a gate adds after the rule's reason is its own sentence,
    /// in the verdict and in every harness's answer.
    #[test]
    fn a_refusal_puts_a_full_stop_before_its_detail() {
        let dir = repo();
        let line = "git commit -m \"Co-authored-by: Cursor <cursoragent@cursor.com>\"";
        let rule = guard_verdict(line, Some(dir.path()), &[]).expect("a trailer is refused");
        assert!(
            rule.reason
                .contains("`git commit -m TEXT`. Remove Co-authored-by from the message"),
            "{}",
            rule.reason
        );
        assert_denies(
            &answers(line, dir.path(), &[]),
            "`git commit -m TEXT`. Remove Co-authored-by",
        );
    }

    /// `git commit --amend --no-edit` keeps the last message, so it runs in
    /// a stock seat: in a repository with no commit yet (git refuses it
    /// there itself) and after one. New text still goes through the gate:
    /// `--amend -m`, `--amend --no-edit --trailer` and a bare `--amend`.
    #[test]
    fn an_amend_that_keeps_the_message_is_allowed() {
        let dir = repo();
        let cwd = dir.path();
        assert!(
            guard_verdict("git commit --amend --no-edit", Some(cwd), &[]).is_none(),
            "no commit yet"
        );
        seed(cwd);
        for line in [
            "git commit --amend --no-edit",
            "git commit --no-edit --amend",
            "git commit -q --amend --no-edit",
        ] {
            assert!(guard_verdict(line, Some(cwd), &[]).is_none(), "{line}");
        }
        let worded = guard_verdict(
            "git commit --amend -m \"Co-authored-by: Cursor <c@c>\"",
            Some(cwd),
            &[],
        )
        .expect("a new message is read");
        assert!(
            worded.reason.contains("Co-authored-by"),
            "{}",
            worded.reason
        );
        let trailer = guard_verdict(
            "git commit --amend --no-edit --trailer \"Co-authored-by: Cursor <c@c>\"",
            Some(cwd),
            &[],
        )
        .expect("a new trailer is read");
        assert!(
            trailer.reason.contains("Co-authored-by"),
            "{}",
            trailer.reason
        );
        let bare = guard_verdict("git commit --amend", Some(cwd), &[]).expect("an editor");
        assert!(
            bare.reason.contains("git commit -m TEXT"),
            "{}",
            bare.reason
        );
    }

    #[test]
    fn commit_identity_trailers_signoff_and_wrappers_say_what_passes() {
        let dir = repo();
        let cwd = dir.path();
        // A runner that exports a git identity would stand in for the
        // checkout's own, so the cases below read only what they name. The
        // lock `repo` holds puts the environment back when it drops.
        for key in [
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
        ] {
            crate::with_env_mutation(|| unsafe { std::env::remove_var(key) });
        }
        let foreign = "GIT_AUTHOR_NAME=rig GIT_AUTHOR_EMAIL=rig@example.invalid GIT_COMMITTER_NAME=rig GIT_COMMITTER_EMAIL=rig@example.invalid git commit -m \"note the seat\"";
        assert!(
            guard_verdict(foreign, Some(cwd), &[]).is_none(),
            "no identity configured, so a foreign author is allowed"
        );
        let generated =
            guard_verdict("git commit -m \"Generated with Cursor\"", Some(cwd), &[]).unwrap();
        assert!(
            generated.reason.contains("Generated with"),
            "{}",
            generated.reason
        );
        let footer = guard_verdict("git commit -m \"Open in Cursor\"", Some(cwd), &[]).unwrap();
        assert!(
            footer.reason.contains("Open in Cursor"),
            "{}",
            footer.reason
        );
        let agent = guard_verdict("git commit -m \"see CURSOR_AGENT\"", Some(cwd), &[]).unwrap();
        assert!(agent.reason.contains("CURSOR_AGENT"), "{}", agent.reason);
        let trailer = guard_verdict(
            "git commit -m \"note the seat\" --trailer \"Co-authored-by: Cursor <c@c>\"",
            Some(cwd),
            &[],
        )
        .unwrap();
        assert!(
            trailer.reason.contains("Co-authored-by"),
            "{}",
            trailer.reason
        );
        let hidden = guard_verdict("git commit", Some(cwd), &[]).unwrap();
        assert!(
            hidden.reason.contains("git commit -m TEXT"),
            "{}",
            hidden.reason
        );
        let wrapped = guard_verdict(
            "sh -c \"git commit -m 'Co-authored-by: Cursor <c@c>'\"",
            Some(cwd),
            &[],
        )
        .unwrap();
        assert!(
            wrapped.reason.contains("Co-authored-by"),
            "{}",
            wrapped.reason
        );
        let alias = guard_verdict(
            "git -c alias.ci=commit ci -m \"note the seat\"",
            Some(cwd),
            &[],
        );
        assert!(alias.is_none(), "{alias:?}");
        let line = format!("Signed-off-by: {AUTHOR_NAME} <{AUTHOR_EMAIL}>");
        gates(
            cwd,
            &format!("name = \"{AUTHOR_NAME}\"\nemail = \"{AUTHOR_EMAIL}\"\nsignoff = true\n"),
        );
        let unsigned = guard_verdict("git commit -m \"note the seat\"", Some(cwd), &[]).unwrap();
        assert!(unsigned.reason.contains(&line), "{}", unsigned.reason);
        assert!(
            !unsigned.reason.contains("git config"),
            "{}",
            unsigned.reason
        );
        assert!(guard_verdict("git commit -s -m \"note the seat\"", Some(cwd), &[]).is_none());
        let signed = format!("git commit -m \"note the seat\n\n{line}\"");
        assert!(guard_verdict(&signed, Some(cwd), &[]).is_none(), "{signed}");
        let wrong = guard_verdict(foreign, Some(cwd), &[]).unwrap();
        assert!(
            wrong.reason.contains("rig@example.invalid"),
            "{}",
            wrong.reason
        );
        assert!(wrong.reason.contains("Author is rig"), "{}", wrong.reason);
        assert!(
            wrong.reason.contains(".ljos/gates.toml"),
            "{}",
            wrong.reason
        );
        assert!(!wrong.reason.contains("git config"), "{}", wrong.reason);
        assert!(!wrong.reason.contains("user.name"), "{}", wrong.reason);
        assert!(!wrong.reason.contains("GIT_AUTHOR"), "{}", wrong.reason);
    }

    #[test]
    fn evidence_is_stored_only_for_the_configured_command_and_a_passing_run() {
        let dir = repo();
        seed(dir.path());
        gates(dir.path(), "test = \"true\"\n");
        let mismatch = record_in(dir.path(), &["false".into()]).unwrap_err();
        assert!(
            mismatch
                .to_string()
                .contains("ljos evidence record -- true"),
            "{mismatch}"
        );
        gates(dir.path(), "test = \"false\"\n");
        let failed = record_in(dir.path(), &["false".into()]).unwrap_err();
        assert!(failed.to_string().contains("exited"), "{failed}");
        assert!(evidence_file(dir.path(), &tip(dir.path())).is_none_or(|p| !p.is_file()));
        gates(dir.path(), "test = \"true\"\n");
        let said = record_in(dir.path(), &["true".into()]).unwrap();
        let sha = tip(dir.path());
        assert!(said.contains(&sha), "{said}");
        let path = evidence_file(dir.path(), &sha).unwrap();
        let body: Stored = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(body.command, "true");
        assert_eq!(body.exit, 0);
        assert_eq!(body.sha, sha);
        assert!(!body.ts.is_empty());
    }

    #[test]
    fn a_plain_push_without_evidence_runs_and_says_so() {
        let dir = repo();
        seed(dir.path());
        gates(dir.path(), "test = \"true\"\n");
        let rules = vec![super::super::Rule {
            pattern: "git push*".into(),
            verdict: "ask".into(),
            reason: "A push needs consent.".into(),
        }];
        let _ = take_push_warning();
        let unrecorded = answers("git push origin main", dir.path(), &rules);
        assert_allows(&unrecorded);
        let warned = take_push_warning().expect("a missing record is a warning");
        assert!(warned.contains("ljos evidence record -- true"), "{warned}");
        let sha = tip(dir.path());
        assert!(
            warned.starts_with(&format!(
                "This push runs without a passing test record. Tip {sha}:"
            )),
            "the warning names the missing record, then the tip: {warned}"
        );
        assert!(
            !warned.contains("could not read this command"),
            "the gate read this push: {warned}"
        );
        record_in(dir.path(), &["true".into()]).unwrap();
        let allowed = answers("git push origin main", dir.path(), &rules);
        assert_allows(&allowed);
        assert!(guard_verdict("git push origin main", Some(dir.path()), &rules).is_none());
        assert!(
            take_push_warning().is_none(),
            "a recorded tip has nothing to say"
        );
        let forced = guard_verdict("git push --force origin main", Some(dir.path()), &rules)
            .expect("force still asks");
        assert_eq!(forced.verdict, "ask");
        let claude = answers("git push --force origin main", dir.path(), &rules);
        let shell = claude.iter().find(|(n, _)| n == "cursor-shell").unwrap();
        assert!(shell.1.contains("\"permission\":\"ask\""), "{}", shell.1);
        std::fs::write(dir.path().join("g.txt"), "two\n").unwrap();
        git(dir.path(), &["add", "g.txt"]);
        let fresh = guard_verdict(
            "git commit -m \"note the seat\" && git push origin main",
            Some(dir.path()),
            &[],
        );
        assert!(
            fresh.is_none(),
            "commit and push on one line runs: {fresh:?}"
        );
        let said = take_push_warning().expect("the new commit has no record");
        assert!(said.contains("new commit"), "{said}");
        let remote = guard_verdict("ssh host 'git push origin main'", Some(dir.path()), &[]);
        assert!(
            remote.is_none(),
            "a push run elsewhere is a push: {remote:?}"
        );
        let commit = guard_verdict(
            "ssh host 'git commit -m \"Co-authored-by: Cursor <c@c>\"'",
            Some(dir.path()),
            &[],
        )
        .expect("a commit run elsewhere hides its message");
        assert!(
            commit.reason.contains("git-remote-hidden"),
            "{}",
            commit.reason
        );
    }

    #[test]
    fn a_broken_gate_file_warns_and_the_push_runs() {
        let dir = repo();
        seed(dir.path());
        gates(dir.path(), "test = [\n");
        let _ = take_push_warning();
        assert!(guard_verdict("git push origin main", Some(dir.path()), &[]).is_none());
        let said = take_push_warning().expect("the parse failure is a warning");
        assert!(said.contains("gates.toml"), "{said}");
    }

    #[test]
    fn a_merge_is_allowed_only_when_ci_is_green_and_bugbot_is_answered() {
        let dir = repo();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let _hold = stub_gh(&green_view(sha), &threads(true, false), "[]");
        let allowed = answers("gh pr merge 7", dir.path(), &[]);
        assert_allows(&allowed);
        drop(_hold);

        let _hold = stub_gh(&red_view(sha), &threads(true, false), "[]");
        let refused = answers("gh pr merge 7", dir.path(), &[]);
        assert_denies(&refused, "ci=FAILURE");
        assert_denies(&refused, "gh pr checks 7");
        drop(_hold);

        let _hold = stub_gh(&green_view(sha), &threads(false, false), "[]");
        let open = guard_verdict("gh pr merge 7", Some(dir.path()), &[]).unwrap();
        assert_eq!(open.verdict, "deny");
        assert!(open.reason.contains("Bugbot"), "{}", open.reason);
        drop(_hold);

        let _hold = stub_gh(&green_view(sha), &threads(false, true), "[]");
        assert!(guard_verdict("gh pr merge 7", Some(dir.path()), &[]).is_none());
        let api = "curl -X PUT https://api.github.com/repos/acme/widgets/pulls/7/merge";
        assert!(guard_verdict(api, Some(dir.path()), &[]).is_none(), "{api}");
    }

    /// A stub `gh` that fails every call: a push that reached for the
    /// forge would be refused with its error.
    fn silent_gh() -> GhHold {
        stub_gh("not json", "not json", "not json")
    }

    #[test]
    fn a_pending_check_on_a_merge_is_refused() {
        let dir = repo();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let _hold = stub_gh(&pending_view(sha), &quiet_threads(), "[]");
        let pending = guard_verdict("gh pr merge 7", Some(dir.path()), &[]).unwrap();
        assert_eq!(pending.verdict, "deny");
        assert!(
            pending.reason.contains("ci=IN_PROGRESS"),
            "{}",
            pending.reason
        );
        assert!(
            pending.reason.contains("git-merge-unready"),
            "{}",
            pending.reason
        );
        assert_denies(&answers("gh pr merge 7", dir.path(), &[]), "ci=IN_PROGRESS");
        drop(_hold);

        let state = rollup_view(
            sha,
            serde_json::json!([{
                "context": "build",
                "state": "PENDING"
            }]),
        );
        let _hold = stub_gh(&state, &quiet_threads(), "[]");
        let held = guard_verdict("gh pr merge 7", Some(dir.path()), &[]).unwrap();
        assert_eq!(held.verdict, "deny");
        assert!(held.reason.contains("build=PENDING"), "{}", held.reason);
    }

    #[test]
    fn a_head_sha_with_no_ci_asks_and_names_local_evidence() {
        let dir = repo();
        seed(dir.path());
        let sha = tip(dir.path());
        for rollup in [serde_json::Value::Null, serde_json::json!([])] {
            let _hold = stub_gh(&rollup_view(&sha, rollup), &quiet_threads(), "[]");
            let asked =
                guard_verdict("gh pr merge 7", Some(dir.path()), &[]).expect("no checks asks");
            assert_eq!(asked.verdict, "ask", "{}", asked.reason);
            assert_eq!(asked.pattern, "merge-gate");
            assert!(asked.reason.contains("no CI ran"), "{}", asked.reason);
            assert!(asked.reason.contains(&sha), "{}", asked.reason);
            assert!(!asked.reason.contains("evidence"), "{}", asked.reason);
            let rows = answers("gh pr merge 7", dir.path(), &[]);
            let shell = rows
                .iter()
                .find(|(name, _)| name == "cursor-shell")
                .unwrap();
            assert!(shell.1.contains("\"permission\":\"ask\""), "{}", shell.1);
            assert!(shell.1.contains("no CI ran"), "{}", shell.1);
            assert!(shell.1.contains(&sha), "{}", shell.1);
        }

        let bad = evidence_file(dir.path(), &sha).unwrap();
        if let Some(parent) = bad.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            &bad,
            serde_json::json!({
                "command": "true",
                "exit": 1,
                "sha": sha,
                "ts": "2026-10-20T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        let _hold = stub_gh(
            &rollup_view(&sha, serde_json::Value::Null),
            &quiet_threads(),
            "[]",
        );
        let failed = guard_verdict("gh pr merge 7", Some(dir.path()), &[]).unwrap();
        assert_eq!(failed.verdict, "ask");
        assert!(!failed.reason.contains("evidence"), "{}", failed.reason);
        drop(_hold);

        gates(dir.path(), "test = \"true\"\n");
        record_in(dir.path(), &["true".into()]).unwrap();
        let _hold = stub_gh(
            &rollup_view(&sha, serde_json::json!([])),
            &quiet_threads(),
            "[]",
        );
        let recorded = guard_verdict("gh pr merge 7", Some(dir.path()), &[]).unwrap();
        assert_eq!(recorded.verdict, "ask", "{}", recorded.reason);
        assert!(recorded.reason.contains("no CI ran"), "{}", recorded.reason);
        assert!(recorded.reason.contains(&sha), "{}", recorded.reason);
        assert!(
            recorded.reason.contains("local evidence record"),
            "{}",
            recorded.reason
        );
        assert!(
            recorded.reason.contains(&format!("exists for {sha}")),
            "{}",
            recorded.reason
        );
        drop(_hold);

        let other = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let _hold = stub_gh(
            &rollup_view(other, serde_json::Value::Null),
            &quiet_threads(),
            "[]",
        );
        let elsewhere = guard_verdict("gh pr merge 7", Some(dir.path()), &[]).unwrap();
        assert_eq!(elsewhere.verdict, "ask");
        assert!(elsewhere.reason.contains(other), "{}", elsewhere.reason);
        assert!(
            !elsewhere.reason.contains("evidence"),
            "{}",
            elsewhere.reason
        );
        drop(_hold);

        let _hold = stub_gh(&red_view(&sha), &quiet_threads(), "[]");
        let red = guard_verdict("gh pr merge 7", Some(dir.path()), &[]).unwrap();
        assert_eq!(red.verdict, "deny");
        assert!(red.reason.contains("ci=FAILURE"), "{}", red.reason);
        drop(_hold);

        let _hold = stub_gh(
            &rollup_view(&sha, serde_json::Value::Null),
            &threads(false, false),
            "[]",
        );
        let open = guard_verdict("gh pr merge 7", Some(dir.path()), &[]).unwrap();
        assert_eq!(open.verdict, "deny");
        assert!(open.reason.contains("Bugbot"), "{}", open.reason);
        assert!(!open.reason.contains("no CI ran"), "{}", open.reason);
    }

    #[test]
    fn an_unreadable_pull_request_is_refused_without_saying_ci_is_missing() {
        let dir = repo();
        let _hold = stub_gh_unreadable();
        let denied =
            guard_verdict("gh pr merge 7", Some(dir.path()), &[]).expect("unreadable pull request");
        assert_eq!(denied.verdict, "deny");
        assert_eq!(denied.pattern, "merge-gate");
        assert!(denied.reason.contains("gh auth login"), "{}", denied.reason);
        assert!(!denied.reason.contains("no CI"), "{}", denied.reason);
        assert!(
            !denied.reason.contains("CI is missing"),
            "{}",
            denied.reason
        );
        assert_denies(&answers("gh pr merge 7", dir.path(), &[]), "gh auth login");
    }

    #[test]
    fn a_denied_check_rollup_allows_a_green_rest_fallback() {
        let dir = repo();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let allow = |spec: &RestGh| {
            let _hold = stub_rest(spec);
            assert!(
                guard_verdict("gh pr merge 7", Some(dir.path()), &[]).is_none(),
                "green REST CI allows the merge"
            );
            let api = "curl -X PUT https://api.github.com/repos/acme/widgets/pulls/7/merge";
            assert!(guard_verdict(api, Some(dir.path()), &[]).is_none(), "{api}");
        };

        allow(&rest_gh(sha, vec![green_runs()], no_statuses()));
        allow(&rest_gh(sha, vec![green_runs()], success_status()));
        allow(&rest_gh(sha, vec![no_runs()], success_status()));
        allow(&rest_gh(
            sha,
            vec![run_page(serde_json::json!([workflow(
                "nightly",
                "completed",
                Some("skipped"),
            )]))],
            no_statuses(),
        ));
        allow(&rest_gh(
            sha,
            vec![
                run_page_total(
                    2,
                    serde_json::json!([workflow("unit", "completed", Some("success"))]),
                ),
                run_page_total(
                    2,
                    serde_json::json!([workflow("docs", "completed", Some("neutral"))]),
                ),
            ],
            success_status(),
        ));

        let mut wrapped = rest_gh(sha, vec![green_runs()], no_statuses());
        wrapped.rollup_error = Some(
            "GraphQL: Resource not accessible by\npersonal access token (repository.pullRequest.statusCheckRollup)\n"
                .into(),
        );
        allow(&wrapped);

        let mut stdout = rest_gh(sha, vec![green_runs()], no_statuses());
        stdout.rollup_error = None;
        stdout.rollup_stdout = Some(ROLLUP_DENIED.into());
        allow(&stdout);

        let mut bugbot = rest_gh(sha, vec![green_runs()], success_status());
        bugbot.threads = threads(false, false);
        let _hold = stub_rest(&bugbot);
        let open = merge_rule(dir.path());
        assert_eq!(open.verdict, "deny");
        assert!(open.reason.contains("Bugbot"), "{}", open.reason);
        assert!(
            open.reason.contains("Source: REST fallback"),
            "{}",
            open.reason
        );
        assert!(!open.reason.contains("no CI"), "{}", open.reason);
        drop(_hold);

        let mut human = rest_gh(sha, vec![green_runs()], success_status());
        human.threads = human_thread();
        let _hold = stub_rest(&human);
        assert!(
            guard_verdict("gh pr merge 7", Some(dir.path()), &[]).is_none(),
            "a thread that is not Bugbot does not refuse the merge"
        );
    }

    #[test]
    fn a_denied_check_rollup_refuses_a_bad_workflow_run() {
        let dir = repo();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        for conclusion in [
            "failure",
            "cancelled",
            "timed_out",
            "action_required",
            "stale",
        ] {
            let page = run_page(serde_json::json!([workflow(
                "ci",
                "completed",
                Some(conclusion),
            )]));
            let _hold = stub_rest(&rest_gh(sha, vec![page], success_status()));
            let denied = merge_rule(dir.path());
            assert_rest_deny(&denied, &format!("ci={conclusion}"));
            assert!(!denied.reason.contains("(pending)"), "{}", denied.reason);
        }

        let page = run_page(serde_json::json!([workflow("ci", "completed", None)]));
        let _hold = stub_rest(&rest_gh(sha, vec![page], success_status()));
        let denied = merge_rule(dir.path());
        assert_rest_deny(&denied, "ci=completed");
        drop(_hold);

        for status in ["queued", "in_progress", "in-progress", "waiting"] {
            let page = run_page(serde_json::json!([workflow("ci", status, None)]));
            let _hold = stub_rest(&rest_gh(sha, vec![page], success_status()));
            let denied = merge_rule(dir.path());
            assert_rest_deny(&denied, &format!("ci={status} (pending)"));
        }

        let page = run_page(serde_json::json!([
            workflow("unit", "completed", Some("failure")),
            workflow("lint", "queued", None),
        ]));
        let _hold = stub_rest(&rest_gh(sha, vec![page], success_status()));
        let denied = merge_rule(dir.path());
        assert_rest_deny(&denied, "unit=failure");
        assert!(
            denied.reason.contains("lint=queued (pending)"),
            "{}",
            denied.reason
        );
        drop(_hold);

        let pages = vec![
            run_page_total(
                2,
                serde_json::json!([workflow("unit", "completed", Some("success"))]),
            ),
            run_page_total(
                2,
                serde_json::json!([workflow("lint", "completed", Some("failure"))]),
            ),
        ];
        let _hold = stub_rest(&rest_gh(sha, pages, success_status()));
        let denied = merge_rule(dir.path());
        assert_rest_deny(&denied, "lint=failure");
        assert!(!denied.reason.contains("unit="), "{}", denied.reason);
        drop(_hold);

        let page = run_page(serde_json::json!([workflow(
            "ci",
            "completed",
            Some("failure"),
        )]));
        let _hold = stub_rest(&rest_gh(sha, vec![page], success_status()));
        assert_denies(
            &answers("gh pr merge 7", dir.path(), &[]),
            "Source: REST fallback",
        );
        assert_denies(&answers("gh pr merge 7", dir.path(), &[]), "ci=failure");
    }

    #[test]
    fn a_denied_check_rollup_refuses_a_bad_commit_status() {
        let dir = repo();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        for state in ["failure", "error"] {
            let _hold = stub_rest(&rest_gh(sha, vec![green_runs()], status_state(state)));
            let denied = merge_rule(dir.path());
            assert_rest_deny(&denied, &format!("commit status={state}"));
            assert!(!denied.reason.contains("(pending)"), "{}", denied.reason);
        }
        let _hold = stub_rest(&rest_gh(sha, vec![green_runs()], status_state("pending")));
        let denied = merge_rule(dir.path());
        assert_rest_deny(&denied, "commit status=pending (pending)");
        drop(_hold);

        let _hold = stub_rest(&rest_gh(sha, vec![no_runs()], status_state("failure")));
        let denied = merge_rule(dir.path());
        assert_rest_deny(&denied, "commit status=failure");
        drop(_hold);

        let mut missing = rest_gh(sha, vec![green_runs()], success_status());
        missing.status = serde_json::json!({
            "statuses": [{"context": "ci", "state": "success"}],
            "total_count": 1,
        })
        .to_string();
        let _hold = stub_rest(&missing);
        let denied = merge_rule(dir.path());
        assert_rest_deny(&denied, "commit status=unknown");
        drop(_hold);

        let page = run_page(serde_json::json!([workflow(
            "unit",
            "completed",
            Some("cancelled"),
        )]));
        let _hold = stub_rest(&rest_gh(sha, vec![page], status_state("error")));
        let denied = merge_rule(dir.path());
        assert_rest_deny(&denied, "unit=cancelled");
        assert!(
            denied.reason.contains("commit status=error"),
            "{}",
            denied.reason
        );
    }

    #[test]
    fn a_denied_check_rollup_with_no_runs_and_no_statuses_asks() {
        let dir = repo();
        seed(dir.path());
        let sha = tip(dir.path());
        let _hold = stub_rest(&rest_gh(&sha, vec![no_runs()], no_statuses()));
        let asked = merge_rule(dir.path());
        assert_rest_ask(&asked, &sha);
        assert!(!asked.reason.contains("evidence"), "{}", asked.reason);
        let rows = answers("gh pr merge 7", dir.path(), &[]);
        let shell = rows
            .iter()
            .find(|(name, _)| name == "cursor-shell")
            .unwrap();
        assert!(shell.1.contains("\"permission\":\"ask\""), "{}", shell.1);
        assert!(shell.1.contains("REST fallback"), "{}", shell.1);
        assert!(shell.1.contains(&sha), "{}", shell.1);
        drop(_hold);

        let bad = evidence_file(dir.path(), &sha).unwrap();
        if let Some(parent) = bad.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            &bad,
            serde_json::json!({
                "command": "true",
                "exit": 1,
                "sha": sha,
                "ts": "2026-10-20T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        let _hold = stub_rest(&rest_gh(&sha, vec![no_runs()], no_statuses()));
        let failed = merge_rule(dir.path());
        assert_rest_ask(&failed, &sha);
        assert!(!failed.reason.contains("evidence"), "{}", failed.reason);
        drop(_hold);

        gates(dir.path(), "test = \"true\"\n");
        record_in(dir.path(), &["true".into()]).unwrap();
        let _hold = stub_rest(&rest_gh(&sha, vec![no_runs()], no_statuses()));
        let recorded = merge_rule(dir.path());
        assert_rest_ask(&recorded, &sha);
        assert!(
            recorded.reason.contains("local evidence record"),
            "{}",
            recorded.reason
        );
        assert!(
            recorded.reason.contains(&format!("exists for {sha}")),
            "{}",
            recorded.reason
        );
        drop(_hold);

        let page = run_page(serde_json::json!([workflow(
            "ci",
            "completed",
            Some("failure"),
        )]));
        let _hold = stub_rest(&rest_gh(&sha, vec![page], no_statuses()));
        let red = merge_rule(dir.path());
        assert_rest_deny(&red, "ci=failure");
        assert!(!red.reason.contains("evidence"), "{}", red.reason);
        drop(_hold);

        let other = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let _hold = stub_rest(&rest_gh(other, vec![no_runs()], no_statuses()));
        let elsewhere = merge_rule(dir.path());
        assert_rest_ask(&elsewhere, other);
        assert!(
            !elsewhere.reason.contains("evidence"),
            "{}",
            elsewhere.reason
        );
        drop(_hold);

        let mut spec = rest_gh(&sha, vec![no_runs()], no_statuses());
        spec.threads = threads(false, false);
        let _hold = stub_rest(&spec);
        let open = merge_rule(dir.path());
        assert_eq!(open.verdict, "deny");
        assert!(open.reason.contains("Bugbot"), "{}", open.reason);
        assert!(
            open.reason.contains("Source: REST fallback"),
            "{}",
            open.reason
        );
        assert!(!open.reason.contains("no CI ran"), "{}", open.reason);
    }

    #[test]
    fn a_failed_rest_read_does_not_say_ci_is_missing() {
        let dir = repo();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let unreadable = |spec: &RestGh| {
            let _hold = stub_rest(spec);
            let denied = merge_rule(dir.path());
            assert_unreadable(&denied);
            denied.reason
        };

        let mut runs_fail = rest_gh(sha, vec![green_runs()], success_status());
        runs_fail.runs_error = Some("HTTP 502\n".into());
        unreadable(&runs_fail);

        let mut status_fail = rest_gh(sha, vec![green_runs()], success_status());
        status_fail.status_error = Some("HTTP 502\n".into());
        unreadable(&status_fail);

        let mut empty_status_fail = rest_gh(sha, vec![no_runs()], no_statuses());
        empty_status_fail.status_error = Some("HTTP 502\n".into());
        let reason = unreadable(&empty_status_fail);
        assert!(!reason.contains("no CI ran"), "{reason}");

        let mut meta_fail = rest_gh(sha, vec![green_runs()], success_status());
        meta_fail.meta_error = Some("HTTP 401: Bad credentials\n".into());
        let reason = unreadable(&meta_fail);
        assert!(reason.contains("Bad credentials"), "{reason}");

        let mut no_url = rest_gh(sha, vec![no_runs()], no_statuses());
        no_url.view = Some(
            serde_json::json!({
                "number": 7,
                "headRefOid": sha,
                "state": "OPEN",
            })
            .to_string(),
        );
        unreadable(&no_url);

        let bad_runs = rest_gh(sha, vec!["{}".into()], no_statuses());
        let reason = unreadable(&bad_runs);
        assert!(
            reason.contains("the workflow run list was missing"),
            "{reason}"
        );

        let mut bad_status = rest_gh(sha, vec![green_runs()], success_status());
        bad_status.status = r#"{"state":"success"}"#.into();
        let reason = unreadable(&bad_status);
        assert!(
            reason.contains("the commit status list was missing"),
            "{reason}"
        );

        let short = rest_gh(
            sha,
            vec![run_page_total(
                2,
                serde_json::json!([workflow("unit", "completed", Some("success"))]),
            )],
            success_status(),
        );
        let reason = unreadable(&short);
        assert!(reason.contains("workflow run page 2 failed"), "{reason}");

        for message in [
            "HTTP 401: Bad credentials\n",
            "Resource not accessible by integration\n",
            "HTTP 403: personal access token lacks a scope\n",
        ] {
            let mut spec = rest_gh(sha, vec![green_runs()], success_status());
            spec.rollup_error = Some(message.into());
            let reason = unreadable(&spec);
            assert!(reason.contains(message.trim()), "{reason}");
        }
    }

    #[test]
    fn a_push_of_a_merge_ignores_a_different_pull_request_head() {
        let dir = repo();
        seed(dir.path());
        git(dir.path(), &["checkout", "-q", "-b", "feature"]);
        std::fs::write(dir.path().join("g.txt"), "two\n").unwrap();
        git(dir.path(), &["add", "g.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "note the feature"]);
        git(dir.path(), &["checkout", "-q", "main"]);
        git(dir.path(), &["merge", "--no-ff", "--no-edit", "feature"]);
        let head = tip(dir.path());
        let other = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let mut spec = rest_gh(
            other,
            vec![run_page(serde_json::json!([workflow(
                "unit",
                "completed",
                Some("success"),
            )]))],
            success_status(),
        );
        spec.list = format!(r#"[{{"number":7,"headRefOid":"{head}","state":"OPEN"}}]"#);
        let _hold = stub_rest(&spec);
        // The forge head is `other` and this merge publishes `head`. A push
        // does not read the pull request, so the difference does not refuse it.
        assert!(
            guard_verdict("git push origin main", Some(dir.path()), &[]).is_none(),
            "a push does not read the pull request"
        );
    }

    #[test]
    fn a_check_rollup_message_names_the_rollup() {
        let dir = repo();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let _hold = stub_gh(&red_view(sha), &quiet_threads(), "[]");
        let red = merge_rule(dir.path());
        assert_eq!(red.verdict, "deny");
        assert!(red.reason.contains("ci=FAILURE"), "{}", red.reason);
        assert!(red.reason.contains("Source: rollup"), "{}", red.reason);
        assert!(!red.reason.contains("REST fallback"), "{}", red.reason);
        drop(_hold);

        let _hold = stub_gh(&pending_view(sha), &quiet_threads(), "[]");
        let pending = merge_rule(dir.path());
        assert!(
            pending.reason.contains("ci=IN_PROGRESS"),
            "{}",
            pending.reason
        );
        assert!(
            pending.reason.contains("Source: rollup"),
            "{}",
            pending.reason
        );
        drop(_hold);

        for rollup in [serde_json::Value::Null, serde_json::json!([])] {
            let _hold = stub_gh(&rollup_view(sha, rollup), &quiet_threads(), "[]");
            let asked = merge_rule(dir.path());
            assert_eq!(asked.verdict, "ask", "{}", asked.reason);
            assert!(asked.reason.contains("no CI ran"), "{}", asked.reason);
            assert!(asked.reason.contains("Source: rollup"), "{}", asked.reason);
            assert!(!asked.reason.contains("REST fallback"), "{}", asked.reason);
        }

        let _hold = stub_gh(&green_view(sha), &threads(false, false), "[]");
        let open = merge_rule(dir.path());
        assert!(open.reason.contains("Bugbot"), "{}", open.reason);
        assert!(open.reason.contains("Source: rollup"), "{}", open.reason);
    }

    #[test]
    fn no_bugbot_threads_lets_a_green_merge_through() {
        let dir = repo();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let _hold = stub_gh(&green_view(sha), &quiet_threads(), "[]");
        assert!(guard_verdict("gh pr merge 7", Some(dir.path()), &[]).is_none());
        assert_allows(&answers("gh pr merge 7", dir.path(), &[]));
        drop(_hold);

        let _hold = stub_gh(&green_view(sha), &human_thread(), "[]");
        assert!(
            guard_verdict("gh pr merge 7", Some(dir.path()), &[]).is_none(),
            "a thread that is not Bugbot does not refuse the merge"
        );
    }

    #[test]
    fn a_local_merge_pushed_to_the_persons_own_main_runs() {
        let dir = repo();
        seed(dir.path());
        git(dir.path(), &["checkout", "-q", "-b", "feature"]);
        std::fs::write(dir.path().join("g.txt"), "two\n").unwrap();
        git(dir.path(), &["add", "g.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "note the feature"]);
        gates(dir.path(), "test = \"true\"\n");
        record_in(dir.path(), &["true".into()]).unwrap();
        git(dir.path(), &["checkout", "-q", "main"]);
        let _hold = silent_gh();
        let line = "git merge feature && git push origin main";
        assert_allows(&answers(line, dir.path(), &[]));

        std::fs::write(dir.path().join("h.txt"), "three\n").unwrap();
        git(dir.path(), &["add", "h.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "note main"]);
        let diverged = guard_verdict(line, Some(dir.path()), &[]);
        assert!(
            diverged.is_none(),
            "a merge commit onto the person's main runs: {diverged:?}"
        );
    }

    #[test]
    fn pushing_a_merge_commit_does_not_wait_on_the_forge() {
        let dir = repo();
        seed(dir.path());
        git(dir.path(), &["checkout", "-q", "-b", "feature"]);
        std::fs::write(dir.path().join("g.txt"), "two\n").unwrap();
        git(dir.path(), &["add", "g.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "note the feature"]);
        git(dir.path(), &["checkout", "-q", "main"]);
        git(dir.path(), &["merge", "--no-ff", "--no-edit", "feature"]);
        let head = tip(dir.path());
        assert!(second_parent(dir.path()).is_some(), "{head}");
        let list = format!(r#"[{{"number":7,"headRefOid":"{head}","state":"OPEN"}}]"#);
        let _hold = stub_gh(&red_view(&head), &threads(false, false), &list);
        assert_allows(&answers("git push origin main", dir.path(), &[]));
        assert!(guard_verdict("git push origin main", Some(dir.path()), &[]).is_none());
    }

    #[test]
    fn a_protected_branch_of_someone_elses_repository_asks() {
        let dir = repo();
        seed(dir.path());
        let _hold = silent_gh();
        gates(dir.path(), "owners = [\"rgoswami\", \"leidarljos\"]\n");
        let asked = guard_verdict("git push origin main", Some(dir.path()), &[])
            .expect("acme is not one of the owners");
        assert_eq!(asked.verdict, "ask");
        assert_eq!(asked.pattern, "push-gate");
        assert!(asked.reason.contains("acme/widgets"), "{}", asked.reason);
        assert!(
            guard_verdict("git push origin HEAD:feature", Some(dir.path()), &[]).is_none(),
            "a feature branch of someone else's repository is an ordinary push"
        );
        gates(dir.path(), "owners = [\"ACME\"]\n");
        assert!(
            guard_verdict("git push origin main", Some(dir.path()), &[]).is_none(),
            "owners match without case"
        );
        gates(dir.path(), "signoff = false\n");
        assert!(
            guard_verdict("git push origin main", Some(dir.path()), &[]).is_none(),
            "no owners set: every repository is the person's"
        );
    }

    #[test]
    fn a_force_push_with_a_test_command_asks_and_is_not_an_evidence_refusal() {
        let dir = repo();
        seed(dir.path());
        git(dir.path(), &["checkout", "-q", "-b", "feature"]);
        std::fs::write(dir.path().join("g.txt"), "two\n").unwrap();
        git(dir.path(), &["add", "g.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "note the feature"]);
        git(dir.path(), &["checkout", "-q", "main"]);
        git(dir.path(), &["merge", "--no-ff", "--no-edit", "feature"]);
        git(dir.path(), &["branch", "-q", "merged"]);
        git(dir.path(), &["reset", "-q", "--hard", "HEAD^1"]);
        git(dir.path(), &["checkout", "-q", "merged"]);
        assert!(second_parent(dir.path()).is_some());
        gates(dir.path(), "test = \"true\"\n");
        let asked = guard_verdict("git push --force origin main", Some(dir.path()), &[]).unwrap();
        assert_eq!(asked.verdict, "ask", "{}", asked.reason);
        assert!(
            !asked.reason.contains("git-push-evidence"),
            "{}",
            asked.reason
        );
        assert!(
            !asked.reason.contains("git-merge-unready"),
            "{}",
            asked.reason
        );
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
    fn a_push_outside_a_checkout_is_not_an_evidence_denial() {
        let _env = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let plain = guard_verdict("git push origin main", Some(dir.path()), &[]);
        assert!(plain.is_none(), "{plain:?}");
        let forced = guard_verdict("git push --force origin main", Some(dir.path()), &[])
            .expect("a force push asks");
        assert_eq!(forced.verdict, "ask", "{}", forced.reason);
        assert_eq!(forced.pattern, "git-force-push");
        assert!(
            forced.reason.starts_with("git-force-push:"),
            "{}",
            forced.reason
        );
        assert!(
            !forced.reason.contains("git-push-evidence"),
            "{}",
            forced.reason
        );
    }

    #[test]
    fn a_plain_push_without_a_test_command_is_allowed_and_a_force_push_asks() {
        let dir = repo();
        seed(dir.path());
        let plain = guard_verdict("git push origin main", Some(dir.path()), &[]);
        assert!(plain.is_none(), "{plain:?}");
        let forced = guard_verdict("git push --force origin main", Some(dir.path()), &[])
            .expect("force still asks");
        assert_eq!(forced.verdict, "ask");
        assert_eq!(forced.pattern, "git-force-push");
        let leased = guard_verdict(
            "git push --force-with-lease origin HEAD:main",
            Some(dir.path()),
            &[],
        )
        .expect("a lease push asks");
        assert_eq!(leased.verdict, "ask");
        assert_eq!(leased.pattern, "git-force-push");
        assert!(
            guard_verdict("git push -u origin feature", Some(dir.path()), &[]).is_none(),
            "a push that sets the upstream is an ordinary push"
        );
        assert!(
            guard_verdict(
                "git push --force-if-includes origin main",
                Some(dir.path()),
                &[]
            )
            .is_none(),
            "force-if-includes is an ordinary push"
        );
        assert!(
            !forced.reason.contains("git-push-evidence"),
            "{}",
            forced.reason
        );
        gates(dir.path(), "signoff = false\n");
        assert!(
            guard_verdict("git push origin main", Some(dir.path()), &[]).is_none(),
            "a gates file with no test command does not ask for evidence"
        );
        gates(dir.path(), "test = \"true\"\n");
        let _ = take_push_warning();
        assert!(
            guard_verdict("git push origin main", Some(dir.path()), &[]).is_none(),
            "a test command with no record warns and the push runs"
        );
        let said = take_push_warning().expect("a warning");
        assert!(said.contains("ljos evidence record -- true"), "{said}");
        let sha = tip(dir.path());
        let path = evidence_file(dir.path(), &sha).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(r#"{{"command":"true","exit":1,"sha":"{sha}","ts":"2020-01-01T00:00:00Z"}}"#),
        )
        .unwrap();
        let _ = take_push_warning();
        assert!(
            guard_verdict("git push origin main", Some(dir.path()), &[]).is_none(),
            "a stale record warns and the push runs"
        );
        let stale = take_push_warning().expect("a failing record is a warning");
        assert!(stale.contains("did not exit 0"), "{stale}");
    }

    #[test]
    fn identity_comes_from_config_layers_and_the_example_is_not_the_default() {
        let dir = repo();
        let cwd = dir.path();
        let home = tempfile::tempdir().unwrap();
        let _layers = layers(home.path(), Some("box"));
        let foreign = "GIT_AUTHOR_NAME=rig GIT_AUTHOR_EMAIL=rig@example.invalid \
                       GIT_COMMITTER_NAME=rig GIT_COMMITTER_EMAIL=rig@example.invalid \
                       git commit -m \"note the seat\"";
        assert!(
            guard_verdict(foreign, Some(cwd), &[]).is_none(),
            "an empty seat config is not an identity gate"
        );
        write_toml(
            &home.path().join("ljos/ljos.toml"),
            "[identity]\nname = \"User Person\"\nemail = \"user@example.com\"\n",
        );
        let user = guard_verdict(foreign, Some(cwd), &[]).unwrap();
        assert!(user.reason.contains("user@example.com"), "{}", user.reason);
        assert!(user.reason.contains("ljos.toml"), "{}", user.reason);
        assert!(!user.reason.contains("git config"), "{}", user.reason);
        write_toml(
            &home.path().join("ljos/seats/box/ljos.toml"),
            "[identity]\nname = \"Seat Person\"\nemail = \"seat@example.com\"\n",
        );
        let seat = guard_verdict(foreign, Some(cwd), &[]).unwrap();
        assert!(seat.reason.contains("seat@example.com"), "{}", seat.reason);
        write_toml(
            &cwd.join("ljos.toml"),
            "[identity]\nname = \"Repo Person\"\nemail = \"repo@example.com\"\n",
        );
        let repo_id = guard_verdict(foreign, Some(cwd), &[]).unwrap();
        assert!(
            repo_id.reason.contains("repo@example.com"),
            "{}",
            repo_id.reason
        );
        gates(
            cwd,
            "name = \"Gate Person\"\nemail = \"gate@example.com\"\n",
        );
        let gate = guard_verdict(foreign, Some(cwd), &[]).unwrap();
        assert!(gate.reason.contains("gate@example.com"), "{}", gate.reason);
        assert!(gate.reason.contains(".ljos/gates.toml"), "{}", gate.reason);
        let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/ljos.toml");
        let text = std::fs::read_to_string(&example).unwrap();
        assert!(text.contains("Rohit Goswami"), "{text}");
        assert!(text.contains("rgoswami@ieee.org"), "{text}");
        write_toml(&home.path().join("ljos/ljos.toml"), &text);
        std::fs::remove_file(home.path().join("ljos/seats/box/ljos.toml")).unwrap();
        std::fs::remove_file(cwd.join("ljos.toml")).unwrap();
        std::fs::remove_file(cwd.join(".ljos/gates.toml")).unwrap();
        let from_example = guard_verdict(foreign, Some(cwd), &[]).unwrap();
        assert!(
            from_example.reason.contains("rgoswami@ieee.org"),
            "{}",
            from_example.reason
        );
        assert!(
            !from_example.reason.contains("git config"),
            "{}",
            from_example.reason
        );
        let rohit = "GIT_AUTHOR_NAME='Rohit Goswami' GIT_AUTHOR_EMAIL=rgoswami@ieee.org \
                     GIT_COMMITTER_NAME='Rohit Goswami' GIT_COMMITTER_EMAIL=rgoswami@ieee.org \
                     git commit -m \"note the seat\"";
        assert!(
            guard_verdict(rohit, Some(cwd), &[]).is_none(),
            "the configured person may commit"
        );
    }
}
