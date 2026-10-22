//! Memory copies the pack did not take because it was down or did not know
//! this seat.
//!
//! `ljos note` writes the logbook line first, then a `note` atom to the
//! pack. When the pack is off, not answering, or answers 401, the fitted
//! copy waits here, one JSON line per copy, and the next note that reaches
//! the pack, `ljos finish`, `ljos doctor`, or the next session start sends
//! it. A refusal is not queued: the pack would refuse it again. A line left
//! from before that rule is cut to what the pack takes, or dropped, on the
//! next send. The queue holds one entry per issue, text, workspace, kind
//! and, for a lesson, the atoms it closes. packsetd answers an identical
//! issue and text with the claim it already holds, so a resend after a
//! lost answer stores nothing new.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

fn default_kind() -> String {
    "note".into()
}

/// One memory waiting for the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    pub issue: String,
    pub text: String,
    /// The pack workspace the note was meant for.
    pub workspace: String,
    /// Unix seconds when the pack did not take it.
    pub at: u64,
    /// `note` or `lesson`. A line written before this field reads as `note`.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Atoms a queued lesson closes. Empty for a note, and for a lesson
    /// that closes nothing. A line written before this field reads as empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<String>,
}

impl Queued {
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.issue.trim() == other.issue.trim()
            && self.text.trim() == other.text.trim()
            && self.workspace == other.workspace
            && self.kind == other.kind
            && self.supersedes == other.supersedes
    }
}

/// What one send says about a queued note.
pub enum Sent {
    /// The pack holds it, under this id.
    Kept(String),
    /// The pack refused it; the reason. The note leaves the queue.
    Refused(String),
    /// The pack did not answer; the note and every later one wait.
    Wait(String),
}

/// What a drain did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Drained {
    /// Issue and pack id of each note the pack now holds.
    pub kept: Vec<(String, String)>,
    /// Issue and reason of each note the pack refused.
    pub refused: Vec<(String, String)>,
    /// Notes still waiting.
    pub left: usize,
    /// Why the first waiting note did not go, when one did not.
    pub waiting_on: Option<String>,
}

/// The state dir that holds the queue: `$XDG_STATE_HOME/ljos`, else
/// `~/.local/state/ljos`.
#[must_use]
pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
}

fn queue_file(dir: &Path) -> PathBuf {
    dir.join("note-queue.jsonl")
}

fn lock(dir: &Path) -> crate::CommitLock {
    let _ = std::fs::create_dir_all(dir);
    crate::CommitLock::acquire_private(&dir.join("note-queue.lock"))
}

fn read(dir: &Path) -> Vec<Queued> {
    crate::complete_rows(&crate::read_state_file(&queue_file(dir)).unwrap_or_default())
}

fn write(dir: &Path, notes: &[Queued]) -> Result<()> {
    let path = queue_file(dir);
    if notes.is_empty() {
        if path.exists() {
            crate::rewrite_state_file(&path, b"")
                .with_context(|| format!("empty {}", path.display()))?;
        }
        return Ok(());
    }
    let mut body = String::new();
    for note in notes {
        body.push_str(&serde_json::to_string(note)?);
        body.push('\n');
    }
    crate::rewrite_state_file(&path, body.as_bytes())
        .with_context(|| format!("replace {}", path.display()))
}

/// The notes waiting in `dir`, oldest first.
#[must_use]
pub fn pending_in(dir: &Path) -> Vec<Queued> {
    let _held = lock(dir);
    read(dir)
}

/// Replace the waiting memories in `dir`.
///
/// # Errors
///
/// The queue file cannot be rewritten.
pub fn replace_in(dir: &Path, notes: Vec<Queued>) -> Result<()> {
    let _held = lock(dir);
    write(dir, &notes)
}

/// Add `note` to the queue in `dir`. `false` when the same copy already
/// waits there, including the atoms a lesson closes.
///
/// # Errors
///
/// The queue file cannot be written.
pub fn push_in(dir: &Path, note: Queued) -> Result<bool> {
    let _held = lock(dir);
    if read(dir).iter().any(|seen| seen.same(&note)) {
        return Ok(false);
    }
    let path = queue_file(dir);
    let mut line = serde_json::to_string(&note)?;
    line.push('\n');
    crate::append_locked_row(&path, line.as_bytes())
        .with_context(|| format!("append {}", path.display()))?;
    Ok(true)
}

/// Send each waiting note in `dir` through `send`, oldest first. A kept
/// or refused note leaves the queue; the first one that has to wait stops
/// the drain, and it and every later note stay, in order.
///
/// # Errors
///
/// The queue file cannot be rewritten.
pub fn drain_in(dir: &Path, mut send: impl FnMut(&Queued) -> Sent) -> Result<Drained> {
    let _held = lock(dir);
    let notes = read(dir);
    let mut done = Drained::default();
    if notes.is_empty() {
        return Ok(done);
    }
    let mut rest = Vec::new();
    for (at, note) in notes.iter().enumerate() {
        match send(note) {
            Sent::Kept(id) => done.kept.push((note.issue.clone(), id)),
            Sent::Refused(why) => done.refused.push((note.issue.clone(), why)),
            Sent::Wait(why) => {
                done.waiting_on = Some(why);
                rest.extend_from_slice(&notes[at..]);
                break;
            }
        }
    }
    done.left = rest.len();
    write(dir, &rest)?;
    Ok(done)
}

/// The lines a drain prints: what went, what the pack refused, and what
/// still waits. Empty when nothing was queued.
#[must_use]
pub fn drained_lines(done: &Drained) -> String {
    let mut out = String::new();
    if !done.kept.is_empty() {
        let ids: Vec<String> = done
            .kept
            .iter()
            .map(|(issue, id)| format!("{issue} {}", id.chars().take(8).collect::<String>()))
            .collect();
        out.push_str(&format!(
            "memory: sent {} queued note(s): {}\n",
            done.kept.len(),
            ids.join(", ")
        ));
    }
    for (issue, why) in &done.refused {
        out.push_str(&format!(
            "memory: the pack refused the queued note on {issue} ({why}); it stays on the logbook\n"
        ));
    }
    if done.left > 0 {
        out.push_str(&format!(
            "memory: {} note(s) still queued ({})\n",
            done.left,
            done.waiting_on
                .as_deref()
                .unwrap_or("the pack did not answer")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(issue: &str, text: &str) -> Queued {
        Queued {
            issue: issue.into(),
            text: text.into(),
            workspace: "ws".into(),
            at: 1,
            kind: "note".into(),
            supersedes: Vec::new(),
        }
    }

    fn dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/note-queue")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_old_line_without_kind_is_a_note() {
        let dir = dir("old");
        std::fs::write(
            dir.join("note-queue.jsonl"),
            "{\"issue\":\"p-1\",\"text\":\"one line\",\"workspace\":\"ws\",\"at\":1}\n",
        )
        .unwrap();
        let pending = pending_in(&dir);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, "note");
        assert_eq!(pending[0].text, "one line");
        assert!(pending[0].supersedes.is_empty());
    }

    #[test]
    fn a_lesson_that_closes_another_atom_is_a_different_copy() {
        let dir = dir("closes");
        let mut lesson = note("p-1", "one line");
        lesson.kind = "lesson".into();
        lesson.supersedes = vec!["atom-old".into()];
        assert!(push_in(&dir, lesson.clone()).unwrap());
        assert!(!push_in(&dir, lesson.clone()).unwrap());
        let mut other = lesson;
        other.supersedes = vec!["atom-new".into()];
        assert!(push_in(&dir, other).unwrap());
        assert_eq!(pending_in(&dir).len(), 2);
    }

    /// A copy still being appended is not yet on the queue; the ones
    /// before it are.
    #[test]
    fn a_torn_last_copy_leaves_the_queue() {
        let dir = dir("torn");
        let mut bytes =
            format!("{}\n", serde_json::to_string(&note("p-1", "kept")).unwrap()).into_bytes();
        bytes.extend_from_slice(b"{\"issue\":\"p-2\",\"text\":\"caf\xc3");
        std::fs::write(dir.join("note-queue.jsonl"), bytes).unwrap();
        let pending = pending_in(&dir);
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].text, "kept");
    }

    /// The queue is one JSON line per copy. writeln! of that line is two
    /// write calls, and the sidecar lock is skipped when it cannot be
    /// taken, so two copies can still land on one line.
    #[test]
    fn concurrent_queue_rows_each_parse() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir("concurrent");
        const THREADS: usize = 32;
        const EACH: usize = 4;
        let mut handles = Vec::with_capacity(THREADS);
        for thread in 0..THREADS {
            let dir = dir.clone();
            handles.push(std::thread::spawn(move || {
                for n in 0..EACH {
                    let mut queued = note(&format!("p-{thread}"), &format!("text {thread}-{n}"));
                    queued.at = (thread * EACH + n) as u64;
                    push_in(&dir, queued).unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().expect("queue writer");
        }
        let path = dir.join("note-queue.jsonl");
        let text = std::fs::read_to_string(&path).unwrap();
        let rows: Vec<Queued> = text
            .lines()
            .map(|line| {
                serde_json::from_str(line).unwrap_or_else(|err| {
                    panic!("queue row does not parse ({err}): {line}");
                })
            })
            .collect();
        assert_eq!(rows.len(), THREADS * EACH, "{text}");
        let mode = std::fs::symlink_metadata(&path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "queue mode {mode:o}");
    }

    /// A drain rewrites the queue. The file and the lock are ljos's own, so
    /// the rewrite lands mode 0600, not the 0644 a plain `tmp + rename`
    /// leaves, and a symlink swapped in for the queue file is refused rather
    /// than written through to its target.
    #[test]
    fn a_rewrite_stays_private_and_refuses_a_symlink() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir("rewrite");
        push_in(&dir, note("p-1", "a")).unwrap();
        push_in(&dir, note("p-2", "b")).unwrap();
        let done = drain_in(&dir, |n| match n.issue.as_str() {
            "p-1" => Sent::Kept("id".into()),
            _ => Sent::Wait("down".into()),
        })
        .unwrap();
        assert_eq!(done.left, 1);
        let path = queue_file(&dir);
        let mode = std::fs::symlink_metadata(&path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "queue mode after rewrite {mode:o}");
        let lock_mode = std::fs::symlink_metadata(dir.join("note-queue.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(lock_mode, 0o600, "lock mode {lock_mode:o}");

        let outside = dir.join("outside");
        std::fs::write(&outside, "kept\n").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(replace_in(&dir, vec![note("p-9", "forged")]).is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "kept\n");
    }

    #[test]
    fn the_same_note_waits_once() {
        let dir = dir("once");
        assert!(push_in(&dir, note("p-1", "one line")).unwrap());
        assert!(!push_in(&dir, note("p-1", " one line ")).unwrap());
        assert!(push_in(&dir, note("p-2", "one line")).unwrap());
        assert_eq!(pending_in(&dir).len(), 2);
    }

    #[test]
    fn a_drain_keeps_order_and_stops_at_the_first_wait() {
        let dir = dir("order");
        for (issue, text) in [("p-1", "a"), ("p-2", "b"), ("p-3", "c"), ("p-4", "d")] {
            push_in(&dir, note(issue, text)).unwrap();
        }
        let mut seen = Vec::new();
        let done = drain_in(&dir, |n| {
            seen.push(n.issue.clone());
            match n.issue.as_str() {
                "p-1" => Sent::Kept("id-1".into()),
                "p-2" => Sent::Refused("too long".into()),
                _ => Sent::Wait("connection refused".into()),
            }
        })
        .unwrap();
        assert_eq!(seen, ["p-1", "p-2", "p-3"]);
        assert_eq!(done.kept, [("p-1".to_string(), "id-1".to_string())]);
        assert_eq!(done.refused.len(), 1);
        assert_eq!(done.left, 2);
        let left: Vec<String> = pending_in(&dir).into_iter().map(|n| n.issue).collect();
        assert_eq!(left, ["p-3", "p-4"]);
        let lines = drained_lines(&done);
        assert!(lines.contains("sent 1 queued note(s): p-1 id-1"), "{lines}");
        assert!(
            lines.contains("2 note(s) still queued (connection refused)"),
            "{lines}"
        );
        let done = drain_in(&dir, |_| Sent::Kept("id".into())).unwrap();
        assert_eq!(done.kept.len(), 2);
        assert!(pending_in(&dir).is_empty());
        assert_eq!(
            drained_lines(&drain_in(&dir, |_| unreachable!()).unwrap()),
            ""
        );
    }
}
