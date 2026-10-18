//! Memory copies of `ljos note` that the pack did not take.
//!
//! `ljos note` writes the logbook line first, then a `note` atom to the
//! pack. When the pack is off or not answering, the atom waits here, one
//! JSON line per note, and the next note that reaches the pack, `ljos
//! finish`, or `ljos doctor` sends it. A refusal is not queued: the pack
//! would refuse it again. The queue holds one entry per issue and text,
//! and packsetd answers an identical issue and text with the claim it
//! already holds, so a resend after a lost answer stores nothing new.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One note waiting for the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    pub issue: String,
    pub text: String,
    /// The pack workspace the note was meant for.
    pub workspace: String,
    /// Unix seconds when the pack did not take it.
    pub at: u64,
}

impl Queued {
    fn same(&self, other: &Self) -> bool {
        self.issue.trim() == other.issue.trim()
            && self.text.trim() == other.text.trim()
            && self.workspace == other.workspace
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
    crate::CommitLock::acquire(&dir.join("note-queue.lock"))
}

fn read(dir: &Path) -> Vec<Queued> {
    std::fs::read_to_string(queue_file(dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn write(dir: &Path, notes: &[Queued]) -> Result<()> {
    let path = queue_file(dir);
    if notes.is_empty() {
        if path.exists() {
            std::fs::write(&path, "").with_context(|| format!("empty {}", path.display()))?;
        }
        return Ok(());
    }
    let tmp = dir.join("note-queue.jsonl.tmp");
    let mut body = String::new();
    for note in notes {
        body.push_str(&serde_json::to_string(note)?);
        body.push('\n');
    }
    std::fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("replace {}", path.display()))
}

/// The notes waiting in `dir`, oldest first.
#[must_use]
pub fn pending_in(dir: &Path) -> Vec<Queued> {
    let _held = lock(dir);
    read(dir)
}

/// Add `note` to the queue in `dir`. `false` when the same issue and
/// text already wait there.
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
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&note)?)
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
