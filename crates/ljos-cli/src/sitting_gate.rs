//! Under an agent runner, the git verbs that write history wait for a
//! sitting: a live claim this seat holds on an open tracker ticket. A push
//! onto a protected branch also waits for that ticket's seats to agree
//! ([`crate::protected_push_stands`]). The person at their own terminal is
//! never asked. No variable switches the gate off: `LJOS_OFF` and a
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
/// while the sitting's ticket does not clear it. `None` when no runner is
/// above this process, when the line writes no history, or when both
/// hold.
#[must_use]
pub fn guard_line(line: &str, cwd: Option<&Path>) -> Option<Rule> {
    if !runner() {
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
            let dest = format!("may land on {dest}");
            return Some(refusal(&seats_wait(&dest, &cfg.branches, &issue, &why)));
        }
    }
    None
}

/// Why `git VERB` waits for a sitting, from [`live_sitting`]'s `why`.
#[must_use]
pub fn no_sitting(verb: &str, why: &str) -> String {
    format!(
        "`git {verb}` writes history, and {why}. Run `ljos sitting ID` on the open ticket \
         this work belongs to, then run it again. The person at their own terminal is not gated"
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
    let Some((issue, hold)) = crate::live_held_issue() else {
        return Err("this conversation holds no live sitting".into());
    };
    let card = crate::issue_record(&issue).map_err(|e| {
        format!("the tracker does not read {issue} ({e:#}), so its sitting cannot be checked")
    })?;
    let state = card["state"].as_str().unwrap_or("").trim();
    if state.is_empty() || matches!(state, "DONE" | "CANCELLED") {
        let state = if state.is_empty() {
            "in no state"
        } else {
            state
        };
        return Err(format!("{issue} is {state} on the tracker, not open"));
    }
    let by = card["claimed_by"].as_str().unwrap_or("").trim();
    if !crate::same_seat_here(by, &hold.assignee) {
        return Err(if by.is_empty() {
            format!("the tracker names no holder on {issue}")
        } else {
            format!("the tracker has {issue} claimed by {by}, not by this seat")
        });
    }
    Ok(issue)
}

fn refusal(detail: &str) -> Rule {
    Rule {
        pattern: "sitting-gate".into(),
        verdict: "deny".into(),
        reason: format!("sitting-gate: {detail}"),
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
