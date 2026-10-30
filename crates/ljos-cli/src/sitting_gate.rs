//! Under an agent runner, the git verbs that write history wait for a
//! sitting: a live claim this seat holds on an open tracker ticket. A push
//! onto a protected branch asks until that ticket has a settled consensus.
//! The person at their own terminal is never asked. No variable switches
//! the gate off: `LJOS_OFF` and a temporary `HOME` keep the rest of the
//! guard out, not this.

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

#[cfg(not(test))]
fn cause() -> Option<crate::RunnerCause> {
    crate::runner_cause()
}

/// A posed runner is a hard one, so a unit test never prompts.
#[cfg(test)]
fn cause() -> Option<crate::RunnerCause> {
    runner().then_some(crate::RunnerCause::Hard)
}

/// The refusal for `line` run from `cwd` under a runner: a git call that
/// writes history while this conversation holds no live sitting, or a
/// push that may land on a branch [`crate::gates::consensus_push`] names
/// while the sitting's ticket does not clear it. A cloud seat whose push
/// the allowlist already accepts, and that is not a force or a delete,
/// asks instead when the only miss is a seat ballot: the hook then writes
/// a one-use request for that exact command. `None` when no runner is
/// above this process, when the line writes no history, or when both
/// hold. An unattributed call, one whose payload and environment name
/// no conversation, is not checked for a commit, a merge, or a push onto
/// a topic branch: that write is a line in the book log. A push onto a
/// protected branch from such a call is refused, since no ticket's seats
/// can clear it. When a runner variable is the only cause and the line
/// is refused, the person is asked for their passphrase on the
/// controlling terminal; a right answer lets this one line through.
#[must_use]
pub fn guard_line(line: &str, cwd: Option<&Path>) -> Option<Rule> {
    let rule = judge_line(line, cwd)?;
    if rule.verdict != "deny" || cause() != Some(crate::RunnerCause::MarkOnly) {
        return Some(rule);
    }
    let verb = crate::gates::git_calls(line, cwd)
        .ok()
        .and_then(|calls| calls.into_iter().find(writes_history))
        .map_or_else(|| "command".to_string(), |call| call.sub);
    let what = if verb == "command" {
        verb.clone()
    } else {
        format!("git {verb}")
    };
    let marks = crate::runner_marks();
    match crate::person_key::ask_person(&marks, &what) {
        Ok(()) => None,
        Err(why) => Some(refusal(&for_marks(
            rule.reason.trim_start_matches("sitting-gate: "),
            &marks,
            &verb,
            crate::person_key::is_set(),
            &why,
        ))),
    }
}

fn judge_line(line: &str, cwd: Option<&Path>) -> Option<Rule> {
    if !runner() {
        return None;
    }
    if crate::writes_hold_record(line, cwd) {
        return Some(refusal(
            "a tool command under a runner may not write a sitting record",
        ));
    }
    let calls = match crate::gates::git_calls(line, cwd) {
        Ok(calls) => calls,
        Err(why) => {
            let note = format!("ljos: the guard could not judge ({why}); the call is allowed");
            eprintln!("{note}");
            crate::bookkeep::log_line(&note);
            return None;
        }
    };
    if let Some(rule) = protected_branch_ask(&calls, line) {
        return Some(rule);
    }
    if crate::call_unattributed() && history_or_unread(line, cwd) {
        crate::bookkeep::log_line("hook: unattributed call, no conversation id");
    }
    None
}

/// The open sitting this conversation already holds. A policy check does
/// not file a ticket.
pub(crate) fn open_or_name_sitting() -> Option<String> {
    if let Some(issue) = crate::bookkeep::open_issue() {
        return Some(issue);
    }
    crate::live_held_issue().map(|(issue, _)| issue)
}

/// A force, a delete, or a mirror push is not this check. Later rules
/// still judge those.
fn plain_protected_push(args: &[String]) -> bool {
    !args.iter().any(|arg| {
        arg == "--force"
            || arg == "--force-with-lease"
            || arg.starts_with("--force")
            || arg == "--delete"
            || arg == "--mirror"
            || arg.starts_with(':')
    })
}

/// A push onto a protected branch asks when the sitting has no settled
/// consensus, and the ask names the refusal after that sentence. One
/// active seat settles on its own vote. Two or more use the ordinary
/// quorum. A conversation id is not read here.
fn protected_branch_ask(calls: &[GitCall], line: &str) -> Option<Rule> {
    for push in calls
        .iter()
        .filter(|call| call.sub == "push" && writes_history(call))
        .filter(|call| plain_protected_push(&call.args))
    {
        let cfg = match crate::gates::consensus_push(Some(&push.cwd)) {
            Ok(cfg) => cfg,
            Err(why) => return Some(asked(&unread_settings(&why))),
        };
        let Some(dest) = crate::gates::protected_push(push, line, &cfg.branches) else {
            continue;
        };
        let Some(issue) = crate::seat_held_issue() else {
            return Some(asked(&protected_ask(&dest, &cfg.branches)));
        };
        if let Err(reason) = crate::protected_push_refusal(&issue, &crate::seat_name(), cfg.quorum)
        {
            let why = format!("the sitting has no settled consensus: {reason}");
            return Some(asked(&seats_wait(&dest, &cfg.branches, &issue, &why)));
        }
    }
    None
}

fn protected_ask(dest: &str, branches: &[String]) -> String {
    let named: Vec<String> = branches
        .iter()
        .map(|branch| format!("`{branch}`"))
        .collect();
    format!(
        "`git push` may land on {dest}, and a push onto {} asks until the sitting's ticket \
         has a settled consensus. One active seat settles on its own vote. Two or more seats \
         use the ordinary quorum. Run `ljos consensus` on the ticket, then push again",
        named.join(", ")
    )
}

/// Why `git VERB` waits for a sitting, from [`live_sitting`]'s `why`.
#[must_use]
pub fn no_sitting(verb: &str, why: &str) -> String {
    format!(
        "`git {verb}` writes history, and {why}. Run `ljos sitting ID` on the open ticket \
         this work belongs to (for a subagent when the parent holds ID, `ljos sitting ID` \
         opens a child ticket under ID, or run `ljos file \"TITLE\" --parent ID` then \
         `ljos sitting <child-id>`), then run it again. {NOT_GATED}"
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
         topic branch is not held. {NOT_GATED}",
        named.join(", ")
    )
}

/// How a refusal under a runner in the tree or the scope ends.
const NOT_GATED: &str = "The person at their own terminal is not gated";

/// `detail`, a refusal from this module, for a call a runner variable
/// alone refused: the closing sentence names the variables in `marks`
/// and the two ways out. `has_key` is whether a person passphrase is
/// kept, and `why` is what the prompt said when one was.
#[must_use]
pub fn for_marks(detail: &str, marks: &[String], verb: &str, has_key: bool, why: &str) -> String {
    let base = detail.trim_end();
    let base = base.strip_suffix(NOT_GATED).unwrap_or(base).trim_end();
    let names = marks.join(", ");
    let cause = format!(
        "This shell carries {names}, which a runner leaves on what it starts, and nothing \
         above it shows a terminal ljos can attribute to you."
    );
    let out = if has_key {
        format!(
            "If this is you: type your person passphrase when asked (set one with \
             `ljos person-key set` from your own machine), or run the {verb} from your own \
             machine. The prompt did not let it through: {why}."
        )
    } else {
        format!(
            "No person passphrase is set, so ljos did not ask for one. If this is you: set \
             one with `ljos person-key set` from your own machine and run it again, or run \
             the {verb} from your own machine."
        )
    };
    format!("{base} {cause} {out}")
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
         needs only the sitting. {NOT_GATED}",
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
            "{why}; a commit, merge, rebase, cherry-pick or push opens a sitting or names the open one"
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
    if crate::push_slow() {
        return Err(crate::push_slow_line().to_string());
    }
    if candidates.is_empty() {
        return Err("this conversation holds no live sitting".into());
    }
    let mut first_err = None;
    for (issue, hold) in candidates {
        let card = match crate::held_issue_record(&issue) {
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

    /// A refusal a runner variable alone caused names the variable and
    /// both ways out, and drops the closing sentence a hard runner gets.
    #[test]
    fn a_mark_only_refusal_names_the_variable_and_the_ways_out() {
        let hard = no_sitting("push", "this conversation holds no live sitting");
        assert!(hard.ends_with(NOT_GATED), "{hard}");
        let marks = vec!["CURSOR_AGENT".to_string()];
        let keyed = for_marks(&hard, &marks, "push", true, "the passphrase did not match");
        assert!(!keyed.contains(NOT_GATED), "{keyed}");
        assert!(keyed.starts_with("`git push` writes history"), "{keyed}");
        for part in [
            "This shell carries CURSOR_AGENT, which a runner leaves on what it starts",
            "type your person passphrase when asked",
            "`ljos person-key set` from your own machine",
            "or run the push from your own machine",
            "The prompt did not let it through: the passphrase did not match.",
        ] {
            assert!(keyed.contains(part), "{part:?} missing: {keyed}");
        }
        let unkeyed = for_marks(&hard, &marks, "push", false, "no person passphrase is set");
        assert!(!unkeyed.contains(NOT_GATED), "{unkeyed}");
        for part in [
            "No person passphrase is set, so ljos did not ask for one",
            "set one with `ljos person-key set` from your own machine",
        ] {
            assert!(unkeyed.contains(part), "{part:?} missing: {unkeyed}");
        }
        let two = vec!["CLAUDECODE".to_string(), "CURSOR_AGENT".to_string()];
        let both = for_marks(
            &seats_wait("may land on `main`", &[], "x-1", "no seat voted on x-1"),
            &two,
            "push",
            true,
            "nothing was typed at the passphrase prompt",
        );
        assert!(
            both.contains("This shell carries CLAUDECODE, CURSOR_AGENT,"),
            "{both}"
        );
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

    fn row(agent: &str, choice: &str) -> serde_json::Value {
        serde_json::json!({"agent": agent, "choice": choice})
    }

    /// One active seat: that seat's own vote settles, so a protected push
    /// is not asked. Two seats: the ordinary quorum still has to be met.
    #[test]
    fn one_active_seat_settles_on_its_own_vote() {
        let one = vec!["ada".to_string()];
        let voted = vec![row("ada", "ship")];
        let none = std::collections::BTreeSet::new();
        let own = |agent: &str| Some(agent.to_string());
        assert!(crate::protected_push_settled(
            &one, &voted, &none, "ada", 2, &own
        ));
        assert!(crate::solo_vote_settles("ada", &voted));
        let quiet = vec![row("ada", "")];
        assert!(!crate::protected_push_settled(
            &one, &quiet, &none, "ada", 2, &own
        ));
        assert!(!crate::protected_push_settled(
            &one,
            &[],
            &none,
            "ada",
            2,
            &own
        ));
    }

    #[test]
    fn two_active_seats_keep_the_quorum() {
        let two = vec!["ada".to_string(), "bea".to_string()];
        let only_ada = vec![row("ada", "ship")];
        let none = std::collections::BTreeSet::new();
        let own = |agent: &str| Some(agent.to_string());
        let same = |_: &str| Some("one-model".to_string());
        assert!(
            !crate::protected_push_settled(&two, &only_ada, &none, "ada", 2, &own),
            "one vote of two seats is not a quorum"
        );
        let both = vec![row("ada", "ship"), row("bea", "ship")];
        assert!(crate::protected_push_settled(
            &two, &both, &none, "ada", 2, &own
        ));
        assert!(
            !crate::protected_push_settled(&two, &both, &none, "ada", 2, &same),
            "two seats in one model family do not settle a protected push"
        );
        let mut personas = std::collections::BTreeSet::new();
        personas.insert("persona-1".to_string());
        let posed = vec![row("ada", "ship"), row("persona-1", "ship")];
        assert!(
            !crate::protected_push_settled(&two, &posed, &personas, "ada", 2, &own),
            "a persona ballot is not a second seat"
        );
        let split = vec![row("ada", "ship"), row("bea", "hold")];
        assert!(!crate::protected_push_settled(
            &two, &split, &none, "ada", 2, &own
        ));
    }

    /// A gates file that does not parse asks. The push is not a hard denial.
    #[test]
    fn a_gates_file_that_does_not_parse_asks() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = crate::git_cmd()
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        git(&["init", "-q", "-b", "develop"]);
        std::fs::create_dir_all(dir.path().join(".ljos")).unwrap();
        std::fs::write(dir.path().join(".ljos/gates.toml"), "quorum = [\n").unwrap();
        let _posed = Posed::runner();
        let rule = guard_line("git push origin develop", Some(dir.path())).unwrap();
        assert_eq!(rule.verdict, "ask");
        assert!(rule.reason.contains("do not read"), "{}", rule.reason);
    }

    /// A git alias nest the guard cannot read allows the line and warns.
    #[test]
    fn an_unreadable_git_line_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let out = crate::git_cmd()
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .output()
            .unwrap();
        assert!(out.status.success());
        for (name, next) in [("a", "b"), ("b", "c"), ("c", "d"), ("d", "e")] {
            let out = crate::git_cmd()
                .arg("-C")
                .arg(dir.path())
                .args(["config", &format!("alias.{name}"), &format!("!git {next}")])
                .output()
                .unwrap();
            assert!(out.status.success());
        }
        let _posed = Posed::runner();
        assert!(guard_line("git a", Some(dir.path())).is_none());
    }
}
