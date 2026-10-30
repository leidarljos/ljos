//! Undo for a tracker mistake: put a closed ticket back to work, and
//! take back this seat's own ballot or note.
//!
//! Nothing is erased. A ticket that was finished by hand, by a program or
//! by another seat can be reopened, and the reopen is a dated note beside
//! the close it undoes. A note taken back stays on the logbook with a
//! withdrawal under it, so a reader sees both what was said and that it
//! was taken back.

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// The note a reopen leaves.
pub const REOPEN_NOTE: &str = "reopened: ";
/// The note a withdrawn logbook note leaves.
pub const NOTE_WITHDRAWN: &str = "note-withdrawn: ";
/// The state a reopen puts a ticket back to when the tracker does not say
/// which one it held.
pub const FRESH_STATE: &str = "TODO";

/// Whether `state` is a ticket that is finished or cancelled.
#[must_use]
pub fn is_closed(state: &str) -> bool {
    matches!(state.trim(), "DONE" | "CANCELLED")
}

/// The state the ticket held before it was closed, from the newest state
/// line that landed on `closed`. `None` when the logbook does not say.
#[must_use]
pub fn prior_state(v: &Value, closed: &str) -> Option<String> {
    v["logbook"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["to"].as_str() == Some(closed.trim()))
        .find_map(|e| e["from"].as_str())
        .map(str::trim)
        .filter(|from| !from.is_empty() && !is_closed(from))
        .map(str::to_string)
}

/// `reopened: SEAT from DONE`.
#[must_use]
pub fn reopen_note(seat: &str, from: &str) -> String {
    format!("{REOPEN_NOTE}{} from {}", seat.trim(), from.trim())
}

/// `note-withdrawn: SEAT TEXT`.
#[must_use]
pub fn withdrawn_note(seat: &str, text: &str) -> String {
    format!("{NOTE_WITHDRAWN}{} {}", seat.trim(), text.trim())
}

/// The seat and the text of a withdrawal note.
#[must_use]
pub fn parse_withdrawn_note(line: &str) -> Option<(String, String)> {
    let rest = line.trim().strip_prefix(NOTE_WITHDRAWN)?;
    let (seat, text) = rest.split_once(' ')?;
    let seat = seat.trim();
    let text = text.trim();
    (!seat.is_empty() && !text.is_empty()).then(|| (seat.to_string(), text.to_string()))
}

/// The note prefixes ljos keeps for itself. A seat takes back what it
/// wrote in its own words, not a signature or a quorum deadline, which
/// their own verbs maintain.
const MACHINERY: &[&str] = &[
    NOTE_WITHDRAWN,
    REOPEN_NOTE,
    "ballot-sig: ",
    "ballot-from: ",
    "ballot-withdrawn: ",
    "candidate-sig: ",
    "quorum: ",
    "quorum timed out: ",
    "playbook: ",
    "workflow: ",
];

/// Whether `note` is one of ljos's own bookkeeping lines.
#[must_use]
pub fn machinery(note: &str) -> bool {
    let note = note.trim();
    MACHINERY.iter().any(|prefix| note.starts_with(prefix))
}

/// The seat a logbook entry names as its writer, from the org line the
/// tracker kept. `None` when the line carries no attribution.
#[must_use]
pub fn author_of(entry: &Value) -> Option<String> {
    let raw = entry["raw"].as_str()?;
    crate::parse_logbook_act(raw).map(|(_, seat)| seat)
}

/// Whether two names are the same seat, as the tracker spells it.
#[must_use]
pub fn one_seat(a: &str, b: &str) -> bool {
    let a = crate::canonical_seat_name(a);
    !a.is_empty() && a == crate::canonical_seat_name(b)
}

/// The one note on the logbook that `which` names, whole or in part.
///
/// # Errors
///
/// No note matches, or more than one does and the text does not say which.
pub fn pick_note<'a>(logbook: &'a [Value], which: &str) -> Result<&'a Value> {
    let which = which.trim();
    if which.is_empty() {
        bail!("withdraw: name the note to take back; nothing was written");
    }
    let notes: Vec<&Value> = logbook
        .iter()
        .filter(|e| {
            e["note"]
                .as_str()
                .is_some_and(|note| !note.trim().is_empty() && !machinery(note))
        })
        .collect();
    let text = |e: &Value| e["note"].as_str().unwrap_or_default().trim().to_string();
    if let Some(exact) = notes.iter().find(|e| text(e) == which) {
        return Ok(exact);
    }
    let hits: Vec<&&Value> = notes.iter().filter(|e| text(e).contains(which)).collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => bail!("withdraw: no note on this ticket reads {which:?}; nothing was written"),
        many => bail!(
            "withdraw: {} notes read {which:?}; name one of them whole; nothing was written",
            many.len()
        ),
    }
}

/// Whether the logbook already took `text` back.
#[must_use]
pub fn already_withdrawn(logbook: &[Value], text: &str) -> bool {
    logbook
        .iter()
        .filter_map(|e| e["note"].as_str())
        .filter_map(parse_withdrawn_note)
        .any(|(_, said)| said == text.trim())
}

/// Take back this seat's own note on `issue`, as a note beside it.
///
/// # Errors
///
/// The tracker has no such ticket, the text names no note or more than
/// one, another seat wrote the note, or the tracker refused the write.
pub fn withdraw_note(issue: &str, seat: &str, which: &str) -> Result<String> {
    let issue = issue.trim();
    if issue.is_empty() {
        bail!("withdraw: an issue id is required; nothing was written");
    }
    let v = crate::issue_record(issue)?;
    let logbook: Vec<Value> = v["logbook"].as_array().cloned().unwrap_or_default();
    let entry = pick_note(&logbook, which)?;
    let text = entry["note"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    if let Some(wrote) = author_of(entry) {
        if !one_seat(seat, &wrote) {
            bail!(
                "withdraw: {wrote} wrote that note, and this seat is {seat}; nothing was written"
            );
        }
    }
    if already_withdrawn(&logbook, &text) {
        return Ok(format!("{issue}: that note was taken back already\n"));
    }
    let note = withdrawn_note(seat, &text);
    crate::run_captured("vissue", &["note", issue, &note])
        .with_context(|| format!("withdraw: the note on {issue} was not written"))?;
    let mut out = format!("{issue}: took back {text:?}\n");
    out.push_str("the note stays on the logbook, with the withdrawal under it\n");
    out.push_str(&crate::persist_tracker(issue, "note withdrawn"));
    Ok(out)
}

/// Take back `who`'s ballot on `issue`, and the forecast it carried.
/// A ballot already taken back still leaves its forecast to take back,
/// so a second withdrawal finishes the first.
///
/// # Errors
///
/// The tracker refused the withdrawal, or the forecast was not withdrawn.
pub fn withdraw_ballot(issue: &str, who: &str) -> Result<String> {
    let standing = crate::panel_gate::standing_ballot_issue(issue, who);
    // The ballot and the forecast are two writes. The first is said as
    // soon as it lands, so a reader of a failure in the second still
    // sees which half went through.
    match crate::run_captured_as("vissue", &["vote", issue, "--withdraw"], Some(who)) {
        Ok(said) => print!("{}", said.stdout),
        Err(e) if format!("{e:#}").contains("holds no ballot") => {
            println!("{issue}: {who} holds no ballot; withdrawing the forecast");
        }
        Err(e) => return Err(e),
    }
    if let Some(standing) = standing.as_deref() {
        if let Err(e) = crate::ballot_sig::note_withdrawn_on(standing, who, issue) {
            eprintln!("ljos withdraw: {e:#}");
        }
    }
    let n = crate::withdraw_prediction(issue, who)
        .with_context(|| format!("ballot withdrawn; the forecast for {who} was not"))?;
    let mut out = format!("{n} forecast(s) withdrawn for {who}\n");
    out.push_str(&crate::persist_tracker(issue, "ballot withdrawn"));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn note(text: &str) -> Value {
        json!({"timestamp": "[2026-10-20 Tue 09:00]", "note": text})
    }

    #[test]
    fn a_finished_or_cancelled_ticket_is_closed() {
        assert!(is_closed("DONE"));
        assert!(is_closed(" CANCELLED "));
        assert!(!is_closed("TODO"));
        assert!(!is_closed("STARTED"));
        assert!(!is_closed(""));
    }

    #[test]
    fn a_reopen_goes_back_to_the_state_the_close_left() {
        let v = json!({"logbook": [
            {"to": "DONE", "from": "STARTED", "timestamp": "[2026-10-20 Tue 09:00]"},
            {"to": "STARTED", "from": "TODO", "timestamp": "[2026-10-19 Mon 09:00]"},
        ]});
        assert_eq!(prior_state(&v, "DONE").as_deref(), Some("STARTED"));
        assert_eq!(prior_state(&v, "CANCELLED"), None);
        let hand = json!({"logbook": []});
        assert_eq!(prior_state(&hand, "DONE"), None);
        let looped = json!({"logbook": [{"to": "DONE", "from": "CANCELLED"}]});
        assert_eq!(prior_state(&looped, "DONE"), None);
    }

    #[test]
    fn an_undo_says_who_undid_it_and_what() {
        assert_eq!(reopen_note("saddle", "DONE"), "reopened: saddle from DONE");
        let line = withdrawn_note("saddle", "the parser keeps the hyphen");
        assert_eq!(line, "note-withdrawn: saddle the parser keeps the hyphen");
        let (seat, text) = parse_withdrawn_note(&line).unwrap();
        assert_eq!(seat, "saddle");
        assert_eq!(text, "the parser keeps the hyphen");
        assert!(parse_withdrawn_note("note-withdrawn: saddle").is_none());
        assert!(parse_withdrawn_note("ballot-sig: saddle ship").is_none());
    }

    #[test]
    fn ljos_bookkeeping_is_not_a_note_a_seat_takes_back() {
        assert!(machinery("ballot-sig: saddle ship a1"));
        assert!(machinery("quorum: 3 by 2026-10-30T00:00:00Z"));
        assert!(machinery("note-withdrawn: saddle a note"));
        assert!(!machinery("the parser keeps the hyphen"));
    }

    #[test]
    fn a_text_names_one_note_or_says_it_cannot() {
        let logbook = vec![
            note("the parser keeps the hyphen"),
            note("the parser drops the dash"),
            note("ballot-sig: saddle ship a1"),
        ];
        let picked = pick_note(&logbook, "keeps the hyphen").unwrap();
        assert_eq!(picked["note"], "the parser keeps the hyphen");
        let whole = pick_note(&logbook, "the parser drops the dash").unwrap();
        assert_eq!(whole["note"], "the parser drops the dash");
        let many = pick_note(&logbook, "the parser").unwrap_err().to_string();
        assert!(many.contains("2 notes read"), "{many}");
        let none = pick_note(&logbook, "the lexer").unwrap_err().to_string();
        assert!(none.contains("no note on this ticket"), "{none}");
        let sig = pick_note(&logbook, "ballot-sig").unwrap_err().to_string();
        assert!(sig.contains("no note on this ticket"), "{sig}");
    }

    #[test]
    fn a_note_already_taken_back_is_not_taken_back_twice() {
        let logbook = vec![
            note("note-withdrawn: saddle the parser keeps the hyphen"),
            note("the parser keeps the hyphen"),
        ];
        assert!(already_withdrawn(&logbook, "the parser keeps the hyphen"));
        assert!(!already_withdrawn(&logbook, "the parser drops the dash"));
    }

    #[test]
    fn a_logbook_line_names_the_seat_that_wrote_it() {
        let attributed = json!({
            "note": "the parser keeps the hyphen",
            "raw": "- Noted by saddle [2026-10-16 Fri 10:35]",
        });
        assert_eq!(author_of(&attributed).as_deref(), Some("saddle"));
        assert_eq!(author_of(&note("the parser keeps the hyphen")), None);
    }
}
