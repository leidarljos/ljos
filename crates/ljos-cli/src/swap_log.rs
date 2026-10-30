//! What a person sees, and what is kept, when the guard swaps a command.
//!
//! The guard can answer a call with another call to run in its place:
//! `uv run pytest` for a bare `pytest`, under a project that pins its
//! environment. The swap is a good one, and it was also silent. The line
//! that ran was not the line anyone typed, nothing said so, and no log
//! held the pair, so a session where a command behaved oddly could not be
//! read back afterwards.
//!
//! This module is the visible half of that answer. It reads the verdict,
//! says in one line what ran instead of what was asked for, keeps a row
//! per swap, and reads the `rewrite` switch a repository sets to turn the
//! whole thing off and run its lines as written.

use crate::{append_locked_row, data_base, now_utc, Durability};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The word the guard's verdict carries when it hands back another call:
/// `allow`, `rewrite`, and the line to run, tab separated.
pub const REWRITE: &str = "rewrite";

/// The key a repository sets to `false` to run its lines as written. It
/// sits at the top of `ljos.toml` or under `[gates]`.
pub const SWITCH: &str = "rewrite";

/// The line the guard hands back in place of the one asked for, and the
/// line a person reads about the swap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swap {
    /// The call to run instead.
    pub line: String,
    /// The one line that says so.
    pub note: String,
}

/// The line a `allow\trewrite\t<line>` verdict hands back, or `None` when
/// the verdict is an ordinary allow or carries no line. A verdict whose
/// line is only whitespace is no rewrite: there is nothing to run.
#[must_use]
pub fn rewritten(verdict: &str) -> Option<String> {
    crate::rewrite::from_policyd(verdict)
}

/// The one line a person reads when a call was swapped. It names both
/// calls and the switch that turns the swapping off, and it is one line
/// whatever the calls looked like: a runner shows a note as the reason
/// text, where a newline reads as a second note.
#[must_use]
pub fn note(asked: &str, ran: &str) -> String {
    format!(
        "ljos ran `{}` in place of `{}`. The guard asked for the swap. \
         Set `{SWITCH} = false` in this repository's ljos.toml to run lines as written.",
        one_line(ran),
        one_line(asked)
    )
}

/// `$XDG_DATA_HOME/ljos/rewrite.jsonl`: one row per swap, beside the
/// guard's bypass log. See [`rows_in`] for their shape.
#[must_use]
pub fn log_path() -> Option<PathBuf> {
    Some(data_base()?.join("ljos").join("rewrite.jsonl"))
}

/// One row of the rewrite log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// When the guard answered, RFC 3339 UTC.
    pub at: String,
    /// The call as it was asked for.
    pub asked: String,
    /// The call the guard handed back.
    pub ran: String,
    /// Whether it ran: false where the `rewrite` switch is off, and the
    /// asked-for call ran instead.
    pub applied: bool,
    /// The seat the call was made under, where one is set.
    pub seat: Option<String>,
}

impl Row {
    fn of(value: &Value) -> Option<Self> {
        Some(Self {
            at: value["ts"].as_str()?.to_string(),
            asked: value["asked"].as_str()?.to_string(),
            ran: value["ran"].as_str()?.to_string(),
            applied: value["applied"].as_bool()?,
            seat: value["seat"].as_str().map(str::to_string),
        })
    }

    fn value(&self) -> Value {
        serde_json::json!({
            "ts": self.at,
            "asked": self.asked,
            "ran": self.ran,
            "applied": self.applied,
            "seat": self.seat,
        })
    }
}

/// The rows in `path`, oldest first. A line that does not parse is
/// skipped: the log is appended to under a lock by several seats, and one
/// torn row does not hide the rest.
#[must_use]
pub fn rows_in(path: &Path) -> Vec<Row> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| Row::of(&value))
        .collect()
}

/// The rows of this seat's rewrite log, oldest first.
#[must_use]
pub fn rows() -> Vec<Row> {
    log_path().map(|path| rows_in(&path)).unwrap_or_default()
}

/// Keep a row for one swap. Best effort: a row that cannot be written
/// does not hold up the call, and the note is shown either way.
pub fn log_swap(asked: &str, ran: &str, applied: bool) {
    let Some(path) = log_path() else {
        return;
    };
    let row = Row {
        at: now_utc(),
        asked: one_line(asked),
        ran: one_line(ran),
        applied,
        seat: std::env::var("LJOS_SEAT").ok(),
    };
    let Ok(mut line) = serde_json::to_string(&row.value()) else {
        return;
    };
    line.push('\n');
    let _ = path.parent().map(std::fs::create_dir_all);
    let _ = append_locked_row(&path, line.as_bytes(), Durability::BestEffort);
}

/// Whether the guard may hand back another call for a command run in
/// `cwd`. True unless a config layer sets `rewrite = false`.
///
/// The layers are the ones the gates read: the user's `ljos.toml`, the
/// seat's, the repository's, `LJOS_CONFIG`, then `.ljos/gates.toml`, a
/// later file overriding an earlier one. A layer that does not parse
/// leaves the swapping off, since a config the seat cannot read is not
/// one that asked for it.
#[must_use]
pub fn allowed_in(cwd: Option<&Path>) -> bool {
    let mut on = true;
    for path in crate::gates::rewrite_layers(cwd) {
        if !path.is_file() {
            continue;
        }
        match crate::gates::rewrite_key(&path) {
            Ok(Some(set)) => on = set,
            Ok(None) => {}
            Err(_) => return false,
        }
    }
    on
}

thread_local! {
    static HELD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Forget a swap the last check held, so a call the guard did not rewrite
/// does not pick up the one before it.
pub fn clear() {
    HELD.with(|slot| *slot.borrow_mut() = None);
}

/// The call the guard handed back, once. The hook gives it to a runner as
/// `updatedInput`; a runner without that field has the note instead.
#[must_use]
pub fn take_line() -> Option<String> {
    HELD.with(|slot| slot.borrow_mut().take())
}

/// What the guard's `verdict` on `asked` changes about the call, where it
/// changes anything: the line to run, which [`take_line`] hands on, and
/// the note to show. Each swap, and each one the switch declined, leaves
/// a row in the log.
///
/// `cwd` is the directory the call would run in, which is where the
/// `rewrite` switch is read.
pub fn apply_rewrite(asked: &str, verdict: &str, cwd: Option<&Path>) -> Option<Swap> {
    let ran = rewritten(verdict)?;
    // A verdict that hands back the same line changes nothing, and saying
    // so would be a note about nothing.
    if one_line(&ran) == one_line(asked) {
        return None;
    }
    if !allowed_in(cwd) {
        log_swap(asked, &ran, false);
        return None;
    }
    log_swap(asked, &ran, true);
    let swap = Swap {
        note: note(asked, &ran),
        line: ran,
    };
    HELD.with(|slot| *slot.borrow_mut() = Some(swap.line.clone()));
    Some(swap)
}

fn one_line(text: &str) -> String {
    // A backslash before a newline is how a shell line is written over
    // several lines. It is not part of the call, and leaving it in reads
    // as a stray argument.
    text.replace("\\\r\n", " ")
        .replace("\\\n", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rewrite_verdict_names_the_call_to_run() {
        assert_eq!(
            rewritten("allow\trewrite\tuv run pytest -q").as_deref(),
            Some("uv run pytest -q")
        );
        assert_eq!(rewritten("allow"), None);
        assert_eq!(rewritten("allow\tthe ljos guard is unhealthy"), None);
        assert_eq!(rewritten("allow\trewrite\t   "), None);
        assert_eq!(rewritten("deny\trewrite\tuv run pytest"), None);
    }

    /// A runner shows the note as one piece of reason text. A command
    /// spread over several lines must not break it into two notes.
    #[test]
    fn the_note_is_one_line_and_names_both_calls() {
        let line = note("pytest \\\n  -q", "uv run pytest \\\n  -q");
        assert!(!line.contains('\n'), "{line}");
        assert!(line.contains("`uv run pytest -q`"), "{line}");
        assert!(line.contains("`pytest -q`"), "{line}");
        assert!(line.contains("rewrite = false"), "{line}");
    }

    #[test]
    fn a_kept_row_reads_back_as_it_was_written() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rewrite.jsonl");
        let row = Row {
            at: "2026-10-28T10:00:00Z".into(),
            asked: "pytest".into(),
            ran: "uv run pytest".into(),
            applied: true,
            seat: Some("claude".into()),
        };
        let mut text = serde_json::to_string(&row.value()).unwrap();
        text.push('\n');
        text.push_str("{not json}\n");
        std::fs::write(&path, text).unwrap();
        assert_eq!(rows_in(&path), vec![row]);
    }

    #[test]
    fn a_verdict_that_hands_back_the_same_call_is_no_swap() {
        assert_eq!(
            apply_rewrite("uv run pytest", "allow\trewrite\tuv run pytest", None),
            None
        );
        assert!(take_line().is_none());
    }

    fn fake_policyd(dir: &Path, verdict: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.join("policyd");
        std::fs::write(&path, format!("#!/bin/sh\nprintf '{verdict}\\n'\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn git_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let out = crate::git_cmd()
            .args(["init", "-q"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }

    /// The whole seam, as a person meets it: the guard hands back another
    /// call, the note says which, the log keeps the pair, and the
    /// repository's `rewrite = false` runs the line as written.
    #[test]
    fn a_swapped_call_is_said_once_and_kept_until_the_repo_turns_it_off() {
        let _lock = crate::test_env_lock();
        let home = tempfile::TempDir::new().unwrap();
        let repo = home.path().join("proj");
        git_repo(&repo);
        let bin = fake_policyd(home.path(), "allow\\trewrite\\tuv run pytest -q");
        // SAFETY: under the lock every environment-reading test takes,
        // which puts each name back when it drops.
        unsafe {
            std::env::set_var("HOME", home.path());
            std::env::set_var("XDG_DATA_HOME", home.path().join("data"));
            std::env::set_var("XDG_CONFIG_HOME", home.path().join("config"));
            std::env::set_var("POLICYD_BIN", &bin);
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("LJOS_GUARD", "on");
            std::env::set_var("LJOS_OFF", "0");
            std::env::set_var("LJOS_SEAT", "claude");
            for key in ["LJOS_GUARD_FAIL_OPEN", "POLICYD_REQUIRED", "LJOS_CONFIG"] {
                std::env::remove_var(key);
            }
        }
        let _ = crate::take_guard_note();
        assert!(
            crate::tcb_verdict_in("pytest -q", Some(&repo)).is_none(),
            "a rewrite is an allow"
        );
        let note = crate::take_guard_note().expect("the swap is said");
        assert!(note.contains("`uv run pytest -q`"), "{note}");
        assert!(note.contains("`pytest -q`"), "{note}");
        assert_eq!(take_line().as_deref(), Some("uv run pytest -q"));
        let kept = rows();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!(kept[0].applied, "{kept:?}");
        assert_eq!(kept[0].seat.as_deref(), Some("claude"));

        std::fs::write(repo.join("ljos.toml"), "rewrite = false\n").unwrap();
        assert!(
            crate::tcb_verdict_in("pytest -q", Some(&repo)).is_none(),
            "the line still runs, as it was written"
        );
        assert!(crate::take_guard_note().is_none(), "nothing was swapped");
        assert!(take_line().is_none());
        let kept = rows();
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert!(!kept[1].applied, "the switch declined it: {kept:?}");
    }
}
