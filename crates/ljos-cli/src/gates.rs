//! Commit, push, and merge gates for the hook and `ljos_guard_check`.
//!
//! The checks are mechanical. A line that is not a commit, a push, or a
//! merge is left to the seat guard and the pack's rules. A denial names
//! the command that makes the same line pass.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

const PERSON_NAME: &str = "Rohit Goswami";
const PERSON_EMAIL: &str = "rgoswami@ieee.org";
const SIGNOFF_LINE: &str = "Signed-off-by: Rohit Goswami <rgoswami@ieee.org>";
const GH_DEADLINE: Duration = Duration::from_secs(15);

thread_local! {
    static GH_BIN: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// The seat guard, policyd, these gates, and the pack's rules, in that
/// order. `None` lets the line run. A plain push that already has test
/// evidence for its tip is not held for approval; a force-push still is.
#[must_use]
pub fn guard_verdict(line: &str, cwd: Option<&Path>, rules: &[super::Rule]) -> Option<super::Rule> {
    if let Some(rule) = super::seat_guard(line) {
        return Some(rule);
    }
    if let Some(rule) = super::tcb_verdict(line) {
        return Some(rule);
    }
    let found = std::env::current_dir().ok();
    let base = cwd.or(found.as_deref());
    let clear = match enforce(line, base, 0) {
        Ok(acc) => acc.clear(),
        Err(rule) => return Some(rule),
    };
    let ruled = super::redirect_seat_verb(
        super::gate_push(
            super::ruled_verdict(rules, line).as_ref(),
            line,
            base.and_then(Path::to_str),
        ),
        line,
    );
    if clear.lift {
        if let Some(rule) = &ruled {
            let about_push = rule.pattern.to_ascii_lowercase().contains("push");
            if rule.verdict == "ask" && (clear.only_pushes || about_push) {
                return None;
            }
        }
    }
    ruled
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
struct Acc {
    plain: bool,
    held: bool,
    other: bool,
}

impl Acc {
    fn clear(self) -> Clear {
        Clear {
            lift: self.plain && !self.held,
            only_pushes: self.plain && !self.held && !self.other,
        }
    }
}

struct Clear {
    lift: bool,
    only_pushes: bool,
}

#[derive(Debug, Default)]
struct Walk {
    acc: Acc,
    branch: Option<String>,
    branch_set: bool,
    fresh: bool,
    pending: Option<Pending>,
}

#[derive(Debug, Clone)]
struct Pending {
    branch: String,
    src: String,
    sha: Option<String>,
    not_ff: bool,
    missing: bool,
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
struct GateConfig {
    test: Option<String>,
    signoff: bool,
    protected: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GateFile {
    test: Option<String>,
    signoff: Option<bool>,
    protected_branches: Option<Vec<String>>,
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
    dry: bool,
    help: bool,
    hidden: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PushClass {
    Plain,
    Held,
    Skip,
}

struct ApiMerge {
    repo: Option<String>,
    spec: String,
}

fn deny(pattern: &str, token: &str, detail: &str) -> super::Rule {
    let base = super::policyd_reason(token);
    let reason = if detail.is_empty() {
        base
    } else {
        format!("{base} {detail}")
    };
    super::Rule {
        pattern: pattern.into(),
        verdict: "deny".into(),
        reason,
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

fn enforce(line: &str, cwd: Option<&Path>, depth: usize) -> Result<Acc, super::Rule> {
    if depth > 3 {
        return Err(deny(
            "push-gate",
            "git-push-evidence",
            "This command expands too many aliases. Run git commit, git push, or the merge directly.",
        ));
    }
    if !may_gate(line) {
        return Ok(Acc::default());
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
                    walk.acc.other = true;
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
            walk.acc.other = true;
            continue;
        }
        walk.acc.other = true;
    }
    Ok(walk.acc)
}

fn handle_git(git: &GitCmd, inv: &Inv, walk: &mut Walk) -> Result<(), super::Rule> {
    match git.sub.as_str() {
        "commit" => on_commit(git, inv, walk),
        "merge" => on_merge(git, walk),
        "checkout" | "switch" => {
            if git.args.iter().any(|a| a == "-") {
                walk.branch = None;
                walk.branch_set = true;
            } else if let Some(branch) = switched_branch(&git.args) {
                walk.branch = Some(branch);
                walk.branch_set = true;
            }
            walk.acc.other = true;
            Ok(())
        }
        "push" => on_push(git, walk),
        _ => {
            walk.acc.other = true;
            Ok(())
        }
    }
}

fn on_commit(git: &GitCmd, inv: &Inv, walk: &mut Walk) -> Result<(), super::Rule> {
    let fields = commit_fields(&git.args);
    if fields.help || fields.dry {
        return Ok(());
    }
    let cfg = repo_config(&git.cwd, "git-commit-identity")?;
    identity_ok(git, &inv.env, fields.author.as_deref())?;
    if fields.hidden {
        return Err(hidden_deny());
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
    if cfg.signoff && !fields.signoff && !has_signoff(&message) {
        return Err(deny(
            "commit-gate",
            "git-commit-signoff",
            "The message has no `Signed-off-by: Rohit Goswami <rgoswami@ieee.org>` line.",
        ));
    }
    walk.fresh = true;
    Ok(())
}

fn on_merge(git: &GitCmd, walk: &mut Walk) -> Result<(), super::Rule> {
    let Some(src) = merge_source(&git.args) else {
        return Ok(());
    };
    let target = if walk.branch_set {
        walk.branch.clone()
    } else {
        abbrev(&git.cwd)
    };
    let Some(target) = target else {
        return Ok(());
    };
    let cfg = repo_config(&git.cwd, "git-merge-unready")?;
    if !is_protected(&target, &cfg.protected) {
        return Ok(());
    }
    if has_subst(&src) {
        walk.pending = Some(Pending {
            branch: target,
            src,
            sha: None,
            not_ff: false,
            missing: true,
        });
        return Ok(());
    }
    let sha = rev(&git.cwd, &src);
    let already = sha
        .as_deref()
        .is_some_and(|_| is_ancestor(&git.cwd, &src, "HEAD"));
    if already {
        return Ok(());
    }
    let fast = sha
        .as_deref()
        .is_some_and(|_| is_ancestor(&git.cwd, "HEAD", &src));
    let not_ff = sha.is_some() && (forces_no_ff(&git.args) || !fast);
    walk.pending = Some(Pending {
        branch: target,
        src,
        sha,
        not_ff,
        missing: false,
    });
    // A missing source is `sha == None` and `missing` stays false until the
    // push. `missing` is set here when rev-parse failed.
    if let Some(p) = walk.pending.as_mut() {
        p.missing = p.sha.is_none();
    }
    Ok(())
}

fn on_push(git: &GitCmd, walk: &mut Walk) -> Result<(), super::Rule> {
    let class = push_class(&git.args);
    if class == PushClass::Skip {
        return Ok(());
    }
    let cfg = repo_config(&git.cwd, "git-push-evidence")?;
    let current = if walk.branch_set {
        walk.branch.clone()
    } else {
        abbrev(&git.cwd)
    };
    let dests = push_dests(&git.cwd, &git.args, current.as_deref());
    if let Some(pending) = walk.pending.clone() {
        if dests.iter().any(|d| d == &pending.branch) {
            let action = format!(
                "git merge {} into {} followed by push",
                pending.src, pending.branch
            );
            if pending.missing {
                return Err(deny(
                    "merge-gate",
                    "git-merge-unready",
                    &format!(
                        "Refusing `{action}`. Cannot read `{}`. Pass a branch that exists, wait until CI is green on its tip and every Cursor Bugbot thread is resolved or has a reply, then merge.",
                        pending.src
                    ),
                ));
            }
            if pending.not_ff {
                return Err(not_ff_deny(&pending.src, &pending.branch));
            }
            let sha = pending.sha.clone().unwrap_or_default();
            let spec = pr_branch(&git.cwd, &pending.src);
            review_pr(&git.cwd, &spec, None, Some(&sha), &action)?;
            evidence_ok(&git.cwd, &sha, &cfg)?;
            mark_push(&mut walk.acc, class);
            return Ok(());
        }
    }
    let specs = refspecs(&git.cwd, &git.args);
    if specs.is_empty() && walk.fresh {
        return Err(fresh_deny(cfg.test.as_deref()));
    }
    if hits_protected(&dests, &cfg.protected) && !walk.fresh {
        if let Some(head) = head_sha(&git.cwd) {
            if second_parent(&git.cwd).is_some() {
                let action = format!("git push of merge commit {head}");
                review_sha(&git.cwd, &head, &action)?;
            }
        }
    }
    let shas = published_shas(
        &git.cwd,
        &git.args,
        current.as_deref(),
        walk.fresh,
        cfg.test.as_deref(),
    )?;
    if shas.is_empty() {
        return Ok(());
    }
    for sha in &shas {
        evidence_ok(&git.cwd, sha, &cfg)?;
    }
    mark_push(&mut walk.acc, class);
    Ok(())
}

fn mark_push(acc: &mut Acc, class: PushClass) {
    match class {
        PushClass::Plain => acc.plain = true,
        PushClass::Held => acc.held = true,
        PushClass::Skip => {}
    }
}

fn published_shas(
    cwd: &Path,
    args: &[String],
    current: Option<&str>,
    fresh: bool,
    test: Option<&str>,
) -> Result<Vec<String>, super::Rule> {
    let specs = refspecs(cwd, args);
    if specs.is_empty() {
        if fresh {
            return Err(fresh_deny(test));
        }
        return head_sha(cwd).map(|s| vec![s]).ok_or_else(|| {
            deny(
                "push-gate",
                "git-push-evidence",
                "This repository has no commit yet. Commit, run `ljos evidence record -- <test command>`, then push that sha.",
            )
        });
    }
    let mut out = Vec::new();
    for spec in specs {
        let Some(src) = src_of(&spec) else {
            continue;
        };
        if pushing_current(&src, current) {
            if fresh {
                return Err(fresh_deny(test));
            }
            let sha = head_sha(cwd).ok_or_else(|| {
                deny(
                    "push-gate",
                    "git-push-evidence",
                    "This repository has no commit yet. Commit, run `ljos evidence record -- <test command>`, then push that sha.",
                )
            })?;
            out.push(sha);
        } else {
            let sha = rev(cwd, &src).ok_or_else(|| {
                deny(
                    "push-gate",
                    "git-push-evidence",
                    &format!(
                        "Cannot read `{src}`, so the tip this push publishes is unknown. Push a branch that exists, run `ljos evidence record -- <test command>` on its tip, then push that sha."
                    ),
                )
            })?;
            out.push(sha);
        }
    }
    Ok(out)
}

fn fresh_deny(test: Option<&str>) -> super::Rule {
    let run = match test {
        Some(test) => format!("Commit, run `ljos evidence record -- {test}`,"),
        None => "Add `test = \"cargo test --workspace\"` (or this repo's suite, such as `meson test --suite eon`) to `.ljos/gates.toml`, commit, run `ljos evidence record -- <that command>`,".to_string(),
    };
    deny(
        "push-gate",
        "git-push-evidence",
        &format!(
            "{run} then push that commit. This command creates a new commit, so the tip it would push has no evidence yet."
        ),
    )
}

fn not_ff_deny(src: &str, branch: &str) -> super::Rule {
    deny(
        "merge-gate",
        "git-merge-unready",
        &format!(
            "Refusing `git merge {src}` into `{branch}` followed by a push. This merge is not a fast-forward, so the commit that would be pushed has no CI. Run `git merge --ff-only {src}` once `{src}` contains `{branch}`, wait until `gh pr checks` is green on that sha and every Cursor Bugbot thread is resolved or has a reply, then push. Or merge on the forge with `gh pr merge` once those are true."
        ),
    )
}

fn evidence_ok(cwd: &Path, sha: &str, cfg: &GateConfig) -> Result<(), super::Rule> {
    let Some(test) = cfg.test.as_deref() else {
        return Err(evidence_deny(
            sha,
            None,
            "`.ljos/gates.toml` has no `test` command.",
        ));
    };
    let Some(path) = evidence_file(cwd, sha) else {
        return Err(evidence_deny(
            sha,
            Some(test),
            "The tip sha cannot be stored as evidence.",
        ));
    };
    if !path.is_file() {
        return Err(evidence_deny(
            sha,
            Some(test),
            "No evidence file for this tip.",
        ));
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => {
            return Err(evidence_deny(
                sha,
                Some(test),
                "The evidence file cannot be read.",
            ))
        }
    };
    let rec: Stored = match serde_json::from_str(&text) {
        Ok(rec) => rec,
        Err(_) => {
            return Err(evidence_deny(
                sha,
                Some(test),
                "The evidence file is unreadable.",
            ))
        }
    };
    let why = if rec.sha != sha {
        "The evidence file is for a different commit."
    } else if rec.exit != 0 {
        "The stored run did not exit 0."
    } else if norm_cmd(&rec.command) != test {
        "The stored command is not the `test` line in `.ljos/gates.toml`."
    } else if rec.ts.trim().is_empty() {
        "The evidence file has no timestamp."
    } else {
        return Ok(());
    };
    Err(evidence_deny(sha, Some(test), why))
}

fn evidence_deny(sha: &str, test: Option<&str>, why: &str) -> super::Rule {
    let run = match test {
        Some(test) => format!("Run `ljos evidence record -- {test}`, then push {sha}."),
        None => "Add `test = \"cargo test --workspace\"` (or this repo's suite, such as `meson test --suite eon`) to `.ljos/gates.toml`, run `ljos evidence record -- <that command>`, then push.".to_string(),
    };
    deny(
        "push-gate",
        "git-push-evidence",
        &format!("Tip {sha}. {why} {run}"),
    )
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
    let path = root.join(".ljos").join("gates.toml");
    if !path.is_file() {
        return Ok(GateConfig {
            test: None,
            signoff: false,
            protected: default_protected(),
        });
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!("`.ljos/gates.toml` cannot be read ({e}). Fix the file, then retry.")
    })?;
    let file: GateFile = toml::from_str(&text).map_err(|e| {
        format!("`.ljos/gates.toml` does not parse ({e}). Fix the file, then retry.")
    })?;
    let protected = match file.protected_branches {
        Some(names) => names
            .into_iter()
            .map(|s| branch_of(&s))
            .filter(|s| !s.is_empty())
            .collect(),
        None => default_protected(),
    };
    let test = file.test.map(|t| norm_cmd(&t)).filter(|t| !t.is_empty());
    Ok(GateConfig {
        test,
        signoff: file.signoff.unwrap_or(false),
        protected,
    })
}

fn is_protected(name: &str, protected: &[String]) -> bool {
    let name = branch_of(name);
    protected.iter().any(|p| p == &name)
}

fn hits_protected(dests: &[String], protected: &[String]) -> bool {
    dests.iter().any(|d| is_protected(d, protected))
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

fn second_parent(cwd: &Path) -> Option<String> {
    git_text(cwd, &["rev-parse", "--verify", "HEAD^2"])
}

fn git_config(cwd: &Path, key: &str) -> Option<String> {
    let value = git_text(cwd, &["config", "--get", key])?;
    (!value.is_empty()).then_some(value)
}

fn git_text(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string(),
    )
}

fn is_ancestor(cwd: &Path, ancestor: &str, descendant: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn identity_ok(
    git: &GitCmd,
    env: &[(String, String)],
    author_flag: Option<&str>,
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
    let author_ok = same_person(&author_n, &author_e);
    let committer_ok = same_person(&committer_n, &committer_e);
    if author_ok && committer_ok {
        return Ok(());
    }
    Err(deny(
        "commit-gate",
        "git-commit-identity",
        &format!(
            "Author is {} <{}>, committer is {} <{}>.",
            shown(&author_n),
            shown(&author_e),
            shown(&committer_n),
            shown(&committer_e)
        ),
    ))
}

fn same_person(name: &str, email: &str) -> bool {
    name.trim() == PERSON_NAME && email.trim().eq_ignore_ascii_case(PERSON_EMAIL)
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

fn has_signoff(message: &str) -> bool {
    message.lines().any(|line| line.trim() == SIGNOFF_LINE)
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

fn push_class(args: &[String]) -> PushClass {
    if is_dry(args) || is_delete(args) {
        PushClass::Skip
    } else if is_force(args) || publishes_tags(args) {
        PushClass::Held
    } else {
        PushClass::Plain
    }
}

fn is_force(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "--force"
            || a.starts_with("--force=")
            || a.starts_with("--force-with-lease")
            || (a.starts_with('+') && a.contains(':'))
            || (a.starts_with('-') && !a.starts_with("--") && a.chars().skip(1).any(|c| c == 'f'))
    })
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

fn publishes_tags(args: &[String]) -> bool {
    args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--tags" | "--follow-tags" | "--mirror" | "--all"
        ) || a.starts_with("refs/tags/")
            || a.starts_with("+refs/tags/")
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

fn pr_branch(cwd: &Path, src: &str) -> String {
    let names = remotes(cwd);
    if let Some((remote, rest)) = src.split_once('/') {
        if names.iter().any(|r| r == remote) && !rest.is_empty() {
            return rest.to_string();
        }
    }
    src.to_string()
}

fn merge_source(args: &[String]) -> Option<String> {
    if is_help(args) {
        return None;
    }
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "--abort" | "--continue" | "--quit"))
    {
        return None;
    }
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            return args.get(i + 1).cloned();
        }
        if matches!(
            a.as_str(),
            "-m" | "--message"
                | "-s"
                | "--strategy"
                | "-X"
                | "--strategy-option"
                | "-F"
                | "--file"
                | "--into"
        ) {
            i += 2;
            continue;
        }
        if a.starts_with("--message=")
            || a.starts_with("--strategy=")
            || a.starts_with("--into=")
            || a.starts_with("--file=")
            || a.starts_with("--strategy-option=")
        {
            i += 1;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(a.clone());
    }
    None
}

fn forces_no_ff(args: &[String]) -> bool {
    args.iter()
        .any(|a| a == "--no-ff" || a == "--squash" || a.starts_with("--no-ff"))
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

fn review_sha(cwd: &Path, sha: &str, action: &str) -> Result<(), super::Rule> {
    let list = gh_json(
        cwd,
        &[
            "pr".into(),
            "list".into(),
            "--search".into(),
            sha.into(),
            "--state".into(),
            "all".into(),
            "--json".into(),
            "number,headRefOid,state".into(),
            "--limit".into(),
            "20".into(),
        ],
    )
    .map_err(|e| merge_io(action, &e))?;
    let rows = list
        .as_array()
        .ok_or_else(|| merge_io(action, "the pull request list was missing"))?;
    let Some(hit) = rows.iter().find(|p| p["headRefOid"].as_str() == Some(sha)) else {
        return Err(merge_deny(
            action,
            sha,
            "",
            "No pull request has this head sha.",
        ));
    };
    let number = hit["number"].as_u64().unwrap_or(0).to_string();
    review_pr(cwd, &number, None, Some(sha), action)
}

fn review_pr(
    cwd: &Path,
    spec: &str,
    repo: Option<&str>,
    expect: Option<&str>,
    action: &str,
) -> Result<(), super::Rule> {
    let mut args = vec!["pr".to_string(), "view".to_string()];
    if !spec.is_empty() {
        args.push(spec.to_string());
    }
    args.push("--json".into());
    args.push("number,headRefOid,statusCheckRollup,url,state".into());
    if let Some(repo) = repo {
        args.push("--repo".into());
        args.push(repo.to_string());
    }
    let view = gh_json(cwd, &args).map_err(|e| merge_io(action, &e))?;
    let number = view["number"]
        .as_u64()
        .ok_or_else(|| merge_io(action, "the pull request number was missing"))?;
    let sha = view["headRefOid"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| merge_io(action, "the head sha was missing"))?
        .to_string();
    if let Some(expect) = expect {
        if sha != expect {
            return Err(merge_deny(
                action,
                expect,
                &number.to_string(),
                &format!("The pull request head is {sha}, but this merge publishes {expect}."),
            ));
        }
    }
    if let Some(problem) = ci_problem(&view["statusCheckRollup"]) {
        return Err(merge_deny(action, &sha, &number.to_string(), &problem));
    }
    let url = view["url"].as_str().unwrap_or("");
    let Some((owner, name)) = owner_repo(url) else {
        return Err(merge_io(
            action,
            "the pull request URL did not name an owner and repository",
        ));
    };
    let threads = bugbot_query(cwd, &owner, &name, number).map_err(|e| merge_io(action, &e))?;
    if let Some(problem) = bugbot_problem(&threads).map_err(|e| merge_io(action, &e))? {
        return Err(merge_deny(action, &sha, &number.to_string(), &problem));
    }
    Ok(())
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

fn ci_problem(rollup: &Value) -> Option<String> {
    if rollup.is_null() {
        return Some("no CI checks are recorded".into());
    }
    let arr = rollup.as_array()?;
    if arr.is_empty() {
        return Some("no CI checks are recorded".into());
    }
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

fn gh_json(cwd: &Path, args: &[String]) -> Result<Value, String> {
    let mut cmd = Command::new(gh_program());
    cmd.current_dir(cwd).args(args);
    let out = run_out(&mut cmd, GH_DEADLINE)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let text = brief(&err);
        return Err(if text.is_empty() {
            format!("gh exited {}", out.status.code().unwrap_or(1))
        } else {
            text
        });
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("gh did not return JSON ({e})"))
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
                    "Run the git command in this checkout, then record evidence or merge here.",
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
            if matches!(git.sub.as_str(), "commit" | "push" | "merge") {
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
    loop {
        let Some(value) = alias_of(&git) else {
            break;
        };
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

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", PERSON_NAME)
            .env("GIT_AUTHOR_EMAIL", PERSON_EMAIL)
            .env("GIT_COMMITTER_NAME", PERSON_NAME)
            .env("GIT_COMMITTER_EMAIL", PERSON_EMAIL)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "user.name", PERSON_NAME]);
        git(dir.path(), &["config", "user.email", PERSON_EMAIL]);
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
        dir
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

    struct GhHold {
        _dir: tempfile::TempDir,
    }

    impl Drop for GhHold {
        fn drop(&mut self) {
            GH_BIN.with(|g| *g.borrow_mut() = None);
        }
    }

    fn stub_gh(view: &str, threads: &str, list: &str) -> GhHold {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("view.json"), view).unwrap();
        std::fs::write(dir.path().join("threads.json"), threads).unwrap();
        std::fs::write(dir.path().join("list.json"), list).unwrap();
        let script = r#"#!/bin/sh
here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
case "$*" in
  *graphql*) cat "$here/threads.json" ;;
  *"pr list"*) cat "$here/list.json" ;;
  *"pr view"*) cat "$here/view.json" ;;
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

    #[test]
    fn commit_identity_trailers_signoff_and_wrappers_say_what_passes() {
        let dir = repo();
        let cwd = dir.path();
        let wrong = guard_verdict(
            "GIT_AUTHOR_NAME=Other GIT_AUTHOR_EMAIL=other@example.com git commit -m \"note the seat\"",
            Some(cwd),
            &[],
        )
        .unwrap();
        assert!(
            wrong.reason.contains("rgoswami@ieee.org"),
            "{}",
            wrong.reason
        );
        assert!(wrong.reason.contains("Author is Other"), "{}", wrong.reason);
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
        gates(cwd, "signoff = true\n");
        let unsigned = guard_verdict("git commit -m \"note the seat\"", Some(cwd), &[]).unwrap();
        assert!(
            unsigned.reason.contains(SIGNOFF_LINE),
            "{}",
            unsigned.reason
        );
        assert!(guard_verdict("git commit -s -m \"note the seat\"", Some(cwd), &[]).is_none());
        let signed = format!("git commit -m \"note the seat\n\n{SIGNOFF_LINE}\"");
        assert!(guard_verdict(&signed, Some(cwd), &[]).is_none(), "{signed}");
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
    fn a_plain_push_with_evidence_is_allowed_and_without_it_is_refused() {
        let dir = repo();
        seed(dir.path());
        gates(dir.path(), "test = \"true\"\n");
        let rules = vec![super::super::Rule {
            pattern: "git push*".into(),
            verdict: "ask".into(),
            reason: "A push needs consent.".into(),
        }];
        let refused = answers("git push origin main", dir.path(), &rules);
        assert_denies(&refused, "ljos evidence record -- true");
        record_in(dir.path(), &["true".into()]).unwrap();
        let allowed = answers("git push origin main", dir.path(), &rules);
        assert_allows(&allowed);
        assert!(guard_verdict("git push origin main", Some(dir.path()), &rules).is_none());
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
        )
        .unwrap();
        assert!(
            fresh.reason.contains("ljos evidence record -- true"),
            "{}",
            fresh.reason
        );
        let remote =
            guard_verdict("ssh host 'git push origin main'", Some(dir.path()), &[]).unwrap();
        assert!(
            remote.reason.contains("git-remote-hidden"),
            "{}",
            remote.reason
        );
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
        assert!(open.reason.contains("Bugbot"), "{}", open.reason);
        drop(_hold);

        let _hold = stub_gh(&green_view(sha), &threads(false, true), "[]");
        assert!(guard_verdict("gh pr merge 7", Some(dir.path()), &[]).is_none());
        let api = "curl -X PUT https://api.github.com/repos/acme/widgets/pulls/7/merge";
        assert!(guard_verdict(api, Some(dir.path()), &[]).is_none(), "{api}");
    }

    #[test]
    fn a_local_merge_pushed_to_a_protected_branch_checks_the_published_sha() {
        let dir = repo();
        seed(dir.path());
        git(dir.path(), &["checkout", "-q", "-b", "feature"]);
        std::fs::write(dir.path().join("g.txt"), "two\n").unwrap();
        git(dir.path(), &["add", "g.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "note the feature"]);
        let feature = tip(dir.path());
        gates(dir.path(), "test = \"true\"\n");
        record_in(dir.path(), &["true".into()]).unwrap();
        git(dir.path(), &["checkout", "-q", "main"]);
        let _hold = stub_gh(&green_view(&feature), &threads(true, false), "[]");
        let line = "git merge feature && git push origin main";
        let rows = answers(line, dir.path(), &[]);
        assert_allows(&rows);
        drop(_hold);

        std::fs::write(dir.path().join("h.txt"), "three\n").unwrap();
        git(dir.path(), &["add", "h.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "note main"]);
        let diverged = guard_verdict(line, Some(dir.path()), &[]).unwrap();
        assert!(
            diverged.reason.contains("not a fast-forward"),
            "{}",
            diverged.reason
        );
        assert!(
            diverged.reason.contains("git merge --ff-only feature"),
            "{}",
            diverged.reason
        );
    }

    #[test]
    fn pushing_a_merge_commit_uses_the_head_sha() {
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
        gates(dir.path(), "test = \"true\"\n");
        record_in(dir.path(), &["true".into()]).unwrap();
        let list = format!(r#"[{{"number":7,"headRefOid":"{head}","state":"OPEN"}}]"#);
        let _hold = stub_gh(&green_view(&head), &threads(true, false), &list);
        assert_allows(&answers("git push origin main", dir.path(), &[]));
        drop(_hold);

        let _hold = stub_gh(&red_view(&head), &threads(true, false), &list);
        let red = guard_verdict("git push origin main", Some(dir.path()), &[]).unwrap();
        assert!(red.reason.contains("git-merge-unready"), "{}", red.reason);
        assert!(red.reason.contains("ci=FAILURE"), "{}", red.reason);
    }
}
