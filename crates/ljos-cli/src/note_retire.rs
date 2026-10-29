//! Retire the note memories an older `ljos note` left behind.
//!
//! `ljos note` once wrote the note to the tracker and a `note` atom to the
//! pack. The atom carried a review clock, so a seat that worked for a while
//! owed thousands of reviews on working notes nobody meant to learn. The
//! write is gone; the atoms it already made are not, and they keep coming
//! back as recalls and as due rows.
//!
//! This sweep tombstones them. The pack keeps the record and drops the index
//! entry, so the note stops being recalled and stops falling due, and the
//! tracker copy, which was always the one worth reading, stays. It runs once
//! per seat: a marker in the state dir says it is done.

use crate::note_queue::state_dir;
use crate::{atoms_lean, pack, NOTE_SOURCE_ENTITY};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The entity the old `ljos note` stamped on the atom it wrote. A note from
/// `ljos remember` or from session-end extraction names its own source and
/// is left alone.
pub fn from_ljos_note() -> String {
    format!("{NOTE_SOURCE_ENTITY}ljos-note")
}

/// The ids of the atoms the old `ljos note` wrote, newest listing first.
#[must_use]
pub fn written_by_note(atoms: &[Value]) -> Vec<String> {
    let marker = from_ljos_note();
    atoms
        .iter()
        .filter(|atom| {
            atom["entities"]
                .as_array()
                .is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some(&marker)))
        })
        .filter_map(|atom| atom["id"].as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// The file that says this seat has swept already.
#[must_use]
pub fn marker_path(dir: &Path) -> PathBuf {
    dir.join("note-atoms-retired")
}

/// Whether the sweep has run for this seat.
#[must_use]
pub fn swept(dir: &Path) -> bool {
    marker_path(dir).exists()
}

/// Say the sweep has run, and how much it retired. Best effort: a marker
/// that cannot be written leaves the sweep to run again, which is safe
/// because the retired atoms are no longer listed.
pub fn mark_swept(dir: &Path, retired: usize) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let _ = std::fs::write(marker_path(dir), format!("{retired}\n"));
}

/// What the sweep says it did.
#[must_use]
pub fn line(retired: usize, left: usize) -> String {
    let mut out = match retired {
        0 => "retire-notes: no note memories are left to retire\n".to_string(),
        1 => "retire-notes: retired 1 note memory from review\n".to_string(),
        n => format!("retire-notes: retired {n} note memories from review\n"),
    };
    if left > 0 {
        out.push_str(&format!(
            "retire-notes: {left} the pack would not retire; run it again\n"
        ));
    }
    out.push_str("the tracker keeps every note; only the memory copies are gone\n");
    out
}

/// Retire the note memories once, and say what happened. A second run says
/// the sweep is done and reads nothing.
///
/// # Errors
///
/// No pack on this seat, or a listing the pack will not answer.
pub fn sweep() -> Result<String> {
    let dir = state_dir();
    if swept(&dir) {
        return Ok("retire-notes: this seat has swept already\n".to_string());
    }
    let client = pack()?;
    let workspace = client.workspace();
    let atoms = atoms_lean(&client, &workspace).context("retire-notes: GET /v1/atoms failed")?;
    let ids = written_by_note(&atoms);
    let mut retired = 0;
    let mut left = 0;
    for id in &ids {
        if client.delete_atom(&workspace, id, None).is_ok() {
            retired += 1;
        } else {
            left += 1;
        }
    }
    if left == 0 {
        mark_swept(&dir, retired);
    }
    Ok(line(retired, left))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn atom(id: &str, source: &str) -> Value {
        json!({
            "id": id,
            "kind": "note",
            "text": "the build needs the vendor dir",
            "entities": ["seat:rg", format!("{NOTE_SOURCE_ENTITY}{source}")],
        })
    }

    #[test]
    fn the_notes_the_old_verb_wrote_are_the_ones_retired() {
        let atoms = vec![
            atom("a1", "ljos-note"),
            atom("a2", "ljos-remember"),
            atom("a3", "session-end-extract"),
            atom("a4", "ljos-note"),
        ];
        assert_eq!(written_by_note(&atoms), vec!["a1", "a4"]);
    }

    #[test]
    fn a_lesson_is_not_a_note_and_is_left_alone() {
        let atoms = vec![json!({
            "id": "l1",
            "kind": "lesson",
            "text": "ask before rewriting history",
            "entities": ["seat:rg", "horizon:standing"],
        })];
        assert!(written_by_note(&atoms).is_empty());
    }

    #[test]
    fn the_marker_stops_a_second_sweep() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!swept(dir.path()));
        mark_swept(dir.path(), 12);
        assert!(swept(dir.path()));
        assert_eq!(
            std::fs::read_to_string(marker_path(dir.path())).unwrap(),
            "12\n"
        );
    }

    #[test]
    fn the_report_counts_what_it_retired() {
        assert!(line(9513, 0).starts_with("retire-notes: retired 9513 note memories"));
        assert!(line(1, 0).contains("retired 1 note memory"));
        assert!(line(0, 0).contains("no note memories are left"));
        assert!(line(4, 2).contains("2 the pack would not retire"));
    }
}
