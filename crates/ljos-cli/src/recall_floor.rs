//! What a sitting shows of the seat's memory: the claims that bear on the
//! task at hand, and a review backlog as a count rather than as a page.
//!
//! A sitting used to print the pack's best-connected cluster whatever the
//! task was, so an audit of the guard opened with a disk audit, a note
//! about credentials and a seat-guard note as "what the seat remembers for
//! this task". The island is still the pack's own answer; this module is
//! the cut that keeps the rows naming the task's subject, and says how
//! many it left out so nothing disappears silently.

use serde_json::Value;

/// The lexical match a row must reach to count as bearing on the task.
///
/// [`crate::lexical_match_scores`] scores a text that names the cue's
/// subject above 0.5 and anything else below it, so this floor is the line
/// between the two: a row that shares one rare word with the title does
/// not reach it, and a row about the task does.
pub const ON_TASK_FLOOR: f64 = 0.5;

/// How many reviews are already due before a sitting prints the count
/// alone. Eight rows orient an agent; nine thousand are a census that
/// pushes the task's own sections off the screen.
pub const DUE_BACKLOG_AT: usize = 40;

/// `rows` that bear on `cue`, and how many were left out. The kept rows
/// stay in the order they came, which is the pack's own activation order.
#[must_use]
pub fn on_task(rows: &[Value], cue: &str) -> (Vec<Value>, usize) {
    let texts: Vec<&str> = rows
        .iter()
        .map(|row| row["text"].as_str().unwrap_or(""))
        .collect();
    let scores = crate::lexical_match_scores(&texts, cue);
    let kept: Vec<Value> = rows
        .iter()
        .zip(&scores)
        .filter(|(_, score)| **score >= ON_TASK_FLOOR)
        .map(|(row, _)| row.clone())
        .collect();
    let left_out = rows.len() - kept.len();
    (kept, left_out)
}

/// The line that accounts for the rows [`on_task`] cut, or nothing when it
/// cut none. `ljos island` prints the whole cluster.
#[must_use]
pub fn left_out_line(left_out: usize) -> String {
    if left_out == 0 {
        return String::new();
    }
    format!(
        "({left_out} activated {} not on this task left out; `ljos island \"...\"` prints the whole cluster)\n",
        if left_out == 1 { "memory" } else { "memories" }
    )
}

/// How many due rows a sitting prints: the soonest `cap` of them, and none
/// at all once the backlog reaches [`DUE_BACKLOG_AT`], where a page of
/// claims is not a review but a flood.
#[must_use]
pub fn due_rows_shown(due: usize, cap: usize) -> usize {
    if due >= DUE_BACKLOG_AT {
        0
    } else {
        due.min(cap)
    }
}

/// The line that stands in for the rows a backlog kept off the page, or
/// nothing when the rows were printed.
#[must_use]
pub fn backlog_line(due: usize, cap: usize) -> String {
    if due_rows_shown(due, cap) > 0 || due == 0 {
        return String::new();
    }
    format!("{due} reviews are due, too many to read here; `ljos due` shows the soonest {cap} to grade\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(text: &str) -> Value {
        serde_json::json!({ "id": text, "text": text })
    }

    /// The sitting's own complaint: an audit of the guard opened with
    /// memories about a disk audit and stored credentials.
    #[test]
    fn a_sitting_keeps_the_rows_about_its_own_task() {
        let rows = [
            row("The seat guard refuses a read of seats.toml, so adding a seat needs a hand edit."),
            row("The disk audit found 40 GB of old encoder downloads under the cache."),
            row("Pocket credentials live in the password manager, never in a repository."),
        ];
        let (kept, left_out) = on_task(&rows, "seat guard refuses reading seats.toml");
        let texts: Vec<&str> = kept.iter().map(|r| r["text"].as_str().unwrap()).collect();
        assert_eq!(texts.len(), 1, "{texts:?}");
        assert!(texts[0].contains("seat guard"), "{texts:?}");
        assert_eq!(left_out, 2);
        assert!(left_out_line(left_out).contains("2 activated memories"));
    }

    #[test]
    fn nothing_is_said_when_every_row_bears_on_the_task() {
        let rows = [row("The tracker writes go to the configured tracker root.")];
        let (kept, left_out) = on_task(&rows, "tracker root configured for writes");
        assert_eq!(kept.len(), 1);
        assert_eq!(left_out, 0);
        assert_eq!(left_out_line(left_out), "");
    }

    #[test]
    fn a_backlog_prints_its_count_and_no_rows() {
        assert_eq!(due_rows_shown(9549, 8), 0);
        let line = backlog_line(9549, 8);
        assert!(line.contains("9549 reviews are due"), "{line}");
        assert!(line.contains("ljos due"), "{line}");
    }

    #[test]
    fn a_short_list_is_still_printed_to_grade() {
        assert_eq!(due_rows_shown(3, 8), 3);
        assert_eq!(due_rows_shown(12, 8), 8);
        assert_eq!(backlog_line(3, 8), "");
        assert_eq!(backlog_line(0, 8), "");
    }
}
