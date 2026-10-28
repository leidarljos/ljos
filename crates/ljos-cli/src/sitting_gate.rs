//! Under an agent runner, the git verbs that write history wait for a
//! sitting: a live claim this seat holds on an open tracker ticket. A push
//! onto a protected branch also waits for that ticket's seats to agree
//! ([`crate::protected_push_stands`]). On a cloud seat, when the only miss
//! is that ballot and the allowlist already accepts the push, the answer
//! asks for that one command. The person at their own terminal is never
//! asked. No variable switches the gate off: `LJOS_OFF` and a
//! temporary `HOME` keep the rest of the guard out, not this.

use std::path::Path;

use crate::gates::GitCall;
use crate::Rule;

/// The git verbs that write history: a commit, a merge, a rebase, a
/// cherry-pick, a revert, a patch applied with `am`, a pull, a push.
const WRITES: &[&str] = &[
    "commit",
    "merge",
    "rebase",
    "cherry-pick",
    "revert",
    "am",
    "pull",
    "push",
];

/// Flags that write nothing, or step out of a merge, rebase or pick
/// already under way.
const NO_WRITE: &[&str] = &["-h", "--help", "--abort", "--quit", "--dry-run"];

#[cfg(test)]
thread_local! {
    /// Whether this thread poses as a runner. Unit tests share one
    /// process environment, so the gate does not read it here.
    static RUNNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(not(test))]
fn runner() -> bool {
    crate::under_a_runner()
}

#[cfg(test)]
fn runner() -> bool {
    RUNNER.with(std::cell::Cell::get)
}

/// The refusal for `line` run from `cwd` under a runner: a git call that
/// writes history while this conversation holds no live sitting, or a
/// push that may land on a branch [`crate::gates::consensus_push`] names
/// while the sitting's ticket does not clear it. An ordinary local push
/// to one explicit branch asks when the only miss is a seat ballot. A cloud
/// push also requires the allowlist to accept its repository and branch.
/// The hook writes a one-use request for that exact command. `None` when no runner is
/// above this process, when the line writes no history, or when both
/// hold. An unattributed call, one whose payload and environment name
/// no conversation, is not checked for a commit, a merge, or a push onto
/// a topic branch: that write is a line in the book log. A push onto a
/// protected branch from such a call is refused, since no ticket's seats
/// can clear it.
#[must_use]
pub fn guard_line(line: &str, cwd: Option<&Path>) -> Option<Rule> {
    if !runner() {
        return None;
    }
    if crate::writes_hold_record(line, cwd) {
        return Some(refusal(
            "a tool command under a runner may not write a sitting record",
        ));
    }
    if crate::call_unattributed() {
        if history_or_unread(line, cwd) {
            crate::bookkeep::log_line("hook: unattributed call, no conversation id");
        }
        // No conversation names no sitting to judge a commit by. A push
        // onto a protected branch still waits for the seats on a ticket,
        // and a call that names nobody sits on none, so that push is
        // refused rather than let through unvoted.
        let calls = match crate::gates::git_calls(line, cwd) {
            Ok(calls) => calls,
            Err(why) => {
                return Some(refusal(&format!(
                    "{why}, so the sitting gate cannot tell what this line writes. \
                     Run the git command directly"
                )))
            }
        };
        for push in calls
            .iter()
            .filter(|c| c.sub == "push" && writes_history(c))
        {
            let cfg = match crate::gates::consensus_push(Some(&push.cwd)) {
                Ok(cfg) => cfg,
                Err(why) => return Some(refusal(&unread_settings(&why))),
            };
            if let Some(dest) = crate::gates::protected_push(push, line, &cfg.branches) {
                let dest = format!("may land on {dest}");
                return Some(refusal(&unattributed_push(&dest, &cfg.branches)));
            }
        }
        return None;
    }
    let calls = match crate::gates::git_calls(line, cwd) {
        Ok(calls) => calls,
        Err(why) => {
            return Some(refusal(&format!(
                "{why}, so the sitting gate cannot tell what this line writes. \
                 Run the git command directly"
            )))
        }
    };
    let first = calls.iter().find(|call| writes_history(call))?;
    let issue = match live_sitting() {
        Ok(issue) => issue,
        Err(why) => return Some(refusal(&no_sitting(&first.sub, &why))),
    };
    let pushes = calls
        .iter()
        .filter(|c| c.sub == "push" && writes_history(c));
    for push in pushes {
        let cfg = match crate::gates::consensus_push(Some(&push.cwd)) {
            Ok(cfg) => cfg,
            Err(why) => return Some(refusal(&unread_settings(&why))),
        };
        let Some(dest) = crate::gates::protected_push(push, line, &cfg.branches) else {
            continue;
        };
        if let Err(why) = crate::protected_push_stands(&issue, cfg.quorum) {
            let shown = format!("may land on {dest}");
            let detail = seats_wait(&shown, &cfg.branches, &issue, &why);
            // The person can consent to an ordinary push whose only missing
            // evidence is a seat ballot. A cloud push also obeys its allowlist.
            // A call that names no conversation has already returned above.
            if only_missing_seat_vote(&why)
                && plain_branch(&dest).is_some_and(|branch| {
                    if crate::cloud::cloud_seat() {
                        cloud_push_clear(line, cwd, push, branch)
                    } else {
                        calls.len() == 1 && local_push_clear(push, branch)
                    }
                })
            {
                return Some(asked(&detail));
            }
            return Some(refusal(&detail));
        }
    }
    None
}

/// Why `git VERB` waits for a sitting, from [`live_sitting`]'s `why`.
#[must_use]
pub fn no_sitting(verb: &str, why: &str) -> String {
    format!(
        "`git {verb}` writes history, and {why}. Run `ljos sitting ID` on the open ticket \
         this work belongs to (for a subagent when the parent holds ID, `ljos sitting ID` \
         opens a child ticket under ID, or run `ljos file \"TITLE\" --parent ID` then \
         `ljos sitting <child-id>`), then run it again. The person at their own terminal is not gated"
    )
}

/// Why a push that `dest` (`may land on BRANCH`) is refused for a call
/// that names no conversation: no ticket's seats can clear it.
#[must_use]
pub fn unattributed_push(dest: &str, branches: &[String]) -> String {
    let named: Vec<String> = branches.iter().map(|b| format!("`{b}`")).collect();
    format!(
        "`git push` {dest}, and a runner's push onto {} waits for the seats on the ticket \
         it sits on, but this call names no conversation, so it holds no live sitting and \
         sits on no ticket. Pass the conversation id in the hook payload or the session \
         environment, open a sitting with `ljos sitting ID`, and run it again. A push onto a \
         topic branch is not held. The person at their own terminal is not gated",
        named.join(", ")
    )
}

/// Why a push waits when [`crate::gates::consensus_push`] does not read.
#[must_use]
pub fn unread_settings(why: &str) -> String {
    format!("the settings that name the protected branches do not read: {why}")
}

/// Why a push that `dest` (`may land on BRANCH`) waits for the seats on
/// `issue`, from [`crate::protected_push_stands`]'s `why`.
#[must_use]
pub fn seats_wait(dest: &str, branches: &[String], issue: &str, why: &str) -> String {
    let named: Vec<String> = branches.iter().map(|b| format!("`{b}`")).collect();
    format!(
        "`git push` {dest}, and a runner's push onto {} waits for the seats on the ticket \
         it sits on: {why}. Run `ljos consensus {issue} --no-file` to see where they stand, \
         then run it again once they clear it. A push onto this seat's own topic branch \
         needs only the sitting. The person at their own terminal is not gated",
        named.join(", ")
    )
}

/// How the doctor's `sitting gate` row begins where no runner is above
/// this shell; [`crate::doctor_word`] reads that row as `info`.
pub const GATE_IDLE: &str = "not active here: no agent runner is above this shell";

/// The doctor's `sitting gate` row. Under a runner: the seat it holds,
/// the sitting it finds, and what a push onto a protected branch from
/// `cwd` waits for; settings that do not read fail the row, since every
/// such push is then refused. For the person, that nothing is gated.
#[must_use]
pub fn doctor_row(cwd: Option<&Path>) -> crate::Habitat {
    let row = |state: String, ok: bool| crate::Habitat {
        name: "sitting gate",
        state,
        ok,
    };
    if !runner() {
        return row(
            format!(
                "{GATE_IDLE}, so the person's git is not gated; under one, a commit, merge, \
                 rebase, cherry-pick or push waits for a live sitting"
            ),
            true,
        );
    }
    let seat = crate::seat_name();
    let cfg = match crate::gates::consensus_push(cwd) {
        Ok(cfg) => cfg,
        Err(why) => {
            return row(
                format!(
                    "active for {seat}, and {}, so every push onto a protected branch is refused",
                    unread_settings(&why)
                ),
                false,
            )
        }
    };
    let sitting = match live_sitting() {
        Ok(issue) => format!("this conversation sits on {issue}"),
        Err(why) => format!(
            "{why}, so a commit, merge, rebase, cherry-pick or push waits for `ljos sitting ID`"
        ),
    };
    let named: Vec<String> = cfg.branches.iter().map(|b| format!("`{b}`")).collect();
    row(
        format!(
            "active for {seat}: {sitting}; a push onto {} also waits for a quorum of {} seats \
             on that ticket",
            named.join(", "),
            cfg.quorum
        ),
        true,
    )
}

/// Whether the sitting check would have a decision on `line`: it writes
/// history, or it cannot be read as a git command.
fn history_or_unread(line: &str, cwd: Option<&Path>) -> bool {
    match crate::gates::git_calls(line, cwd) {
        Ok(calls) => calls.iter().any(writes_history),
        Err(_) => true,
    }
}

fn writes_history(call: &GitCall) -> bool {
    if !WRITES.contains(&call.sub.as_str()) {
        return false;
    }
    if call.args.iter().any(|a| NO_WRITE.contains(&a.as_str())) {
        return false;
    }
    !(call.sub == "push" && crate::gates::is_dry(&call.args))
}

/// The ticket this conversation sits on, or why it sits on none. The
/// claim is a hold record whose conversation still runs
/// ([`crate::live_held_issue`]), and the tracker reads its ticket as open
/// and claimed by this seat. A tracker that does not answer is no sitting.
///
/// # Errors
///
/// Why there is no live sitting, in words that follow "and".
pub fn live_sitting() -> Result<String, String> {
    if crate::call_conversation_unusable() {
        return Err(
            "the hook payload's conversation id names no conversation, so this conversation holds no live sitting"
                .into(),
        );
    }
    let candidates = crate::live_held_issues();
    if candidates.is_empty() {
        return Err("this conversation holds no live sitting".into());
    }
    let mut first_err = None;
    for (issue, hold) in candidates {
        let card = match crate::issue_record(&issue) {
            Ok(c) => c,
            Err(e) => {
                first_err.get_or_insert_with(|| {
                    format!("the tracker does not read {issue} ({e:#}), so its sitting cannot be checked")
                });
                continue;
            }
        };
        let state = card["state"].as_str().unwrap_or("").trim();
        if state.is_empty() || matches!(state, "DONE" | "CANCELLED") {
            let state = if state.is_empty() {
                "in no state"
            } else {
                state
            };
            first_err.get_or_insert_with(|| format!("{issue} is {state} on the tracker, not open"));
            continue;
        }
        let by = card["claimed_by"].as_str().unwrap_or("").trim();
        if !crate::same_seat_here(by, &hold.assignee)
            || !crate::same_conversation_tag(by, &hold.assignee)
        {
            first_err.get_or_insert_with(|| {
                if by.is_empty() {
                    format!("the tracker names no holder on {issue}")
                } else {
                    format!("the tracker has {issue} claimed by {by}, not by this seat")
                }
            });
            continue;
        }
        return Ok(issue);
    }
    Err(first_err.unwrap_or_else(|| "this conversation holds no live sitting".into()))
}

fn refusal(detail: &str) -> Rule {
    Rule {
        pattern: "sitting-gate".into(),
        verdict: "deny".into(),
        reason: format!("sitting-gate: {detail}"),
    }
}

fn asked(detail: &str) -> Rule {
    Rule {
        pattern: "sitting-gate".into(),
        verdict: "ask".into(),
        reason: format!("sitting-gate: {detail}"),
    }
}

/// The cite has no seat ballot. A split, a short quorum, a revise, or an
/// unreadable roster is a different miss and stays a refusal.
fn only_missing_seat_vote(why: &str) -> bool {
    why.contains("no seat voted on ")
}

/// `dest` from [`crate::gates::protected_push`] when it names one branch.
fn plain_branch(dest: &str) -> Option<&str> {
    let name = dest.strip_prefix('`')?.strip_suffix('`')?;
    (!name.is_empty() && !name.contains([' ', '\t', '\n', '`'])).then_some(name)
}

/// An ordinary push with one explicit destination. Options that change
/// ref selection, hooks or force semantics do not qualify for this consent path.
fn local_push_clear(push: &GitCall, branch: &str) -> bool {
    let mut positional = Vec::new();
    for arg in &push.args {
        if arg.starts_with('-') {
            if !matches!(
                arg.as_str(),
                "-u" | "--set-upstream"
                    | "-v"
                    | "--verbose"
                    | "-q"
                    | "--quiet"
                    | "--porcelain"
                    | "--progress"
                    | "--atomic"
            ) {
                return false;
            }
        } else {
            positional.push(arg.as_str());
        }
    }
    let [_, spec] = positional.as_slice() else {
        return false;
    };
    if spec.starts_with('+') || spec.contains('*') {
        return false;
    }
    let destination = match spec.split_once(':') {
        Some((source, destination)) if !source.is_empty() => destination,
        Some(_) => return false,
        None => spec,
    };
    destination
        .strip_prefix("refs/heads/")
        .or_else(|| destination.strip_prefix("heads/"))
        .unwrap_or(destination)
        == branch
}

/// The cloud rules would let this push through. The seat is a cloud seat,
/// the line forces nothing, deletes nothing, and updates no ref outside
/// the seat's branches, and the push allowlist names this repository and,
/// for a code repository, this branch.
fn cloud_push_clear(line: &str, cwd: Option<&Path>, push: &GitCall, branch: &str) -> bool {
    if !crate::cloud::cloud_seat() {
        return false;
    }
    if crate::cloud_guard::cursor_push(line, cwd).is_some() {
        return false;
    }
    let Some(url) = remote_url(push) else {
        return false;
    };
    allowlist_has(&url, branch)
}

fn remote_url(call: &GitCall) -> Option<String> {
    let name = remote_word(&call.args).unwrap_or_else(|| "origin".to_string());
    if explicit_repo(&name) {
        return Some(name);
    }
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&call.cwd)
        .args(["remote", "get-url", &name])
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

/// A repository written where a remote name would be: a URL, a path, or
/// an scp form. A short name is a remote in the checkout.
fn explicit_repo(name: &str) -> bool {
    name.contains("://")
        || name.starts_with('/')
        || name.starts_with('.')
        || name.starts_with('~')
        || name.contains(':')
}

/// The first refspec's remote, skipping options that take a value.
/// `None` is a bare push; the caller reads `origin`.
fn remote_word(args: &[String]) -> Option<String> {
    const VALUED: &[&str] = &[
        "-o",
        "--push-option",
        "--receive-pack",
        "--exec",
        "--repo",
        "--recurse-submodules",
    ];
    let mut i = 0;
    while i < args.len() {
        let word = args[i].as_str();
        if word == "--" {
            return args.get(i + 1).cloned();
        }
        if let Some(flags) = word.strip_prefix('-') {
            if !flags.is_empty()
                && !flags.starts_with('-')
                && flags.ends_with('o')
                && flags.chars().all(|c| c.is_ascii_alphabetic())
            {
                i += 2;
                continue;
            }
        }
        if VALUED.contains(&word) {
            i += 2;
            continue;
        }
        if word.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(word.to_string());
    }
    None
}

/// Whether the push allowlist names `url` and, when that repository is
/// code, `branch`. A tracker repository may update any branch. A missing
/// or unreadable file accepts nothing.
fn allowlist_has(url: &str, branch: &str) -> bool {
    let key = crate::cloud_hook::push_repo_key(url);
    if key.is_empty() {
        return false;
    }
    let Some(path) = crate::cloud_hook::allow_path() else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let mut kind = "";
    let mut listed = false;
    let mut closed = false;
    for line in text.lines() {
        let Some((tag, val)) = line.split_once('\t') else {
            continue;
        };
        match tag {
            "code" if val == key => kind = "code",
            "tracker" if val == key && kind != "code" => kind = "tracker",
            "branch" if val == branch => listed = true,
            "closed" => closed = true,
            _ => {}
        }
    }
    match kind {
        "tracker" => true,
        "code" => !closed && listed,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Posed;

    impl Posed {
        fn runner() -> Self {
            RUNNER.with(|r| r.set(true));
            Self
        }
    }

    impl Drop for Posed {
        fn drop(&mut self) {
            RUNNER.with(|r| r.set(false));
        }
    }

    #[test]
    fn an_idle_gate_informs_and_fails_no_doctor() {
        let row = doctor_row(None);
        assert_eq!(crate::doctor_word(&row), "info", "{}", row.state);
        assert!(crate::healthy(&[row]));
    }

    #[test]
    fn only_a_history_write_under_a_runner_is_judged() {
        assert!(guard_line("git commit -m x", None).is_none());
        let _posed = Posed::runner();
        for line in [
            "git status",
            "git log -1",
            "git fetch origin",
            "git push --dry-run origin main",
            "git rebase --abort",
            "git commit --help",
            "echo commit",
        ] {
            assert!(guard_line(line, None).is_none(), "{line}");
        }
        for line in [
            "git commit -m x",
            "git commit -n -m x",
            "git merge topic",
            "git rebase main",
            "git cherry-pick abc",
            "git revert abc",
            "git am x.patch",
            "git pull",
            "git push origin topic",
        ] {
            let calls = crate::gates::git_calls(line, None).unwrap();
            assert!(calls.iter().any(writes_history), "{line}");
        }
    }
}
