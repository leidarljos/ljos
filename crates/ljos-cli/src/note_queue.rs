//! Memory copies the pack did not take because it was down or did not know
//! this seat.
//!
//! `ljos note` writes the logbook line first, then a `note` atom to the
//! pack. When the pack is off, not answering, or answers 401, the fitted
//! copy waits here, one JSON line per copy, and the next note that reaches
//! the pack, `ljos finish`, or the next session start sends it. `ljos
//! doctor` only counts it. A copy the pack would refuse is kept in the
//! memory slot with its label and leaves the queue. A line left from
//! before that rule is cut to what the pack takes on the next send, and
//! the original stays in the slot. A note that has to wait stays queued.
//! Later notes are still tried. The queue holds one entry per issue, text,
//! workspace, kind
//! and, for a lesson, the atoms it closes. The seat that queued the copy
//! stays on the row, so a later seat does not take it. packsetd answers an
//! identical issue and text with the claim it already holds, so a resend
//! after a lost answer stores nothing new.

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
    /// The seat that queued the copy. Empty on a line written before this
    /// field. The seat that drains the queue does not replace it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub seat: String,
    /// The memory slot (`<tracker>/.ljos/memory`) the copy belongs to.
    /// Empty on a line written before this field.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub home: String,
}

impl Queued {
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.issue.trim() == other.issue.trim()
            && self.text.trim() == other.text.trim()
            && self.workspace == other.workspace
            && self.kind == other.kind
            && self.supersedes == other.supersedes
            && self.seat == other.seat
            && self.home == other.home
    }
}

/// What one send says about a queued note.
pub enum Sent {
    /// The pack holds it, under this id.
    Kept(String),
    /// The pack refused it; the reason. The note leaves the queue.
    /// The caller keeps the text in the memory slot.
    Refused(String),
    /// The pack did not answer. The note stays queued. Later notes are
    /// still tried, so one waiting row does not stall the rest.
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
    /// Queue bytes that were not a note, moved out before a rewrite.
    pub set_aside: Vec<SetAside>,
}

/// Bytes of the queue a rewrite moved to a file beside it rather than
/// drop: complete lines that did not parse and a last line without its
/// newline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetAside {
    pub path: PathBuf,
    pub bytes: usize,
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

fn lock_file(dir: &Path) -> PathBuf {
    dir.join("note-queue.lock")
}

fn lock(dir: &Path) -> crate::CommitLock {
    let _ = std::fs::create_dir_all(dir);
    crate::CommitLock::acquire_private(&lock_file(dir))
}

/// The queue lock in `dir` when another ljos holds it now, queueing or
/// sending; tried without waiting for it.
#[must_use]
pub fn busy_in(dir: &Path) -> Option<PathBuf> {
    let lock = lock_file(dir);
    crate::CommitLock::is_held_private(&lock).then_some(lock)
}

fn read(dir: &Path) -> Vec<Queued> {
    read_split(dir).0
}

/// The queue's notes, and its bytes that are not one: each complete line
/// that does not parse, and a last line without its newline. Under the
/// lock no writer is part way through that last line; every append holds
/// it.
fn read_split(dir: &Path) -> (Vec<Queued>, Vec<u8>) {
    let bytes = crate::read_state_file(&queue_file(dir)).unwrap_or_default();
    let mut notes = Vec::new();
    let mut unread = Vec::new();
    for line in bytes.split_inclusive(|&b| b == b'\n') {
        match line.strip_suffix(b"\n").map(serde_json::from_slice) {
            Some(Ok(note)) => notes.push(note),
            _ if line == b"\n" => {}
            _ => unread.extend_from_slice(line),
        }
    }
    (notes, unread)
}

/// Where a rewrite on this day puts the queue bytes it could not read.
fn damaged_file(dir: &Path) -> PathBuf {
    dir.join(format!(
        "note-queue.jsonl.{}.damaged",
        chrono::Utc::now().format("%Y-%m-%d")
    ))
}

/// [`set_aside_at`] for `dir`, opened the way every state-log open is: a
/// link in place of `dir` is refused and lists nothing.
#[must_use]
pub fn set_aside_in(dir: &Path) -> Vec<(PathBuf, u64)> {
    match crate::open_state_dir(&queue_file(dir)) {
        Ok((fd, _)) => set_aside_at(&fd, dir),
        Err(_) => Vec::new(),
    }
}

/// The damaged files a rewrite left in the state dir open as `fd`, named
/// below `dir`, with their sizes, oldest day first. Each name is listed
/// and stated through `fd`, without following a link and without opening
/// one, so name and size come from the same directory; a link or
/// anything else that is not a regular file is left out, since a send
/// never writes through one.
#[must_use]
pub fn set_aside_at(fd: &std::fs::File, dir: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(names) = crate::deedbag::list_names(fd) else {
        return Vec::new();
    };
    let mut found: Vec<(PathBuf, u64)> = names
        .iter()
        .filter_map(|name| {
            let text = name.to_str().ok()?;
            text.strip_prefix("note-queue.jsonl.")?
                .strip_suffix(".damaged")?;
            let st = crate::deedbag::stat_at(fd, name).ok()??;
            let size = u64::try_from(st.st_size).ok()?;
            ((st.st_mode & libc::S_IFMT) == libc::S_IFREG).then(|| (dir.join(text), size))
        })
        .collect();
    found.sort();
    found
}

/// Add `unread` to the day's damaged file and put it on disk, before the
/// rewrite that leaves those bytes out of the queue.
fn set_aside(dir: &Path, unread: &[u8]) -> Result<Option<SetAside>> {
    if unread.is_empty() {
        return Ok(None);
    }
    let path = damaged_file(dir);
    crate::append_locked_row(&path, unread, crate::Durability::Durable)
        .and_then(|()| crate::sync_state_file(&path))
        .with_context(|| format!("set aside unreadable queue bytes in {}", path.display()))?;
    Ok(Some(SetAside {
        path,
        bytes: unread.len(),
    }))
}

/// Rewrite the queue to `notes`. `unread`, what [`read_split`] could not
/// read, is set aside first, so a rewrite drops no byte it could not
/// parse.
fn write(dir: &Path, notes: &[Queued], unread: &[u8]) -> Result<Option<SetAside>> {
    let _durable = crate::bookkeep::durable_write();
    let aside = set_aside(dir, unread)?;
    let path = queue_file(dir);
    if notes.is_empty() {
        // Opened, not stat'ed through the path: a dangling link is refused
        // instead of read as no queue.
        let present = crate::open_state_dir(&path)
            .and_then(|(fd, name)| crate::open_regular_at(&fd, &name, &path, libc::O_RDONLY, 0));
        match present {
            Ok(_) => crate::rewrite_state_file(&path, b"")
                .with_context(|| format!("empty {}", path.display()))?,
            Err(err) if err.raw_os_error() == Some(libc::ENOENT) => {}
            Err(err) => return Err(err).with_context(|| format!("empty {}", path.display())),
        }
        return Ok(aside);
    }
    let mut body = String::new();
    for note in notes {
        body.push_str(&serde_json::to_string(note)?);
        body.push('\n');
    }
    crate::rewrite_state_file(&path, body.as_bytes())
        .with_context(|| format!("replace {}", path.display()))?;
    Ok(aside)
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
    let (_, unread) = read_split(dir);
    write(dir, &notes, &unread).map(drop)
}

/// Fit the waiting memories in `dir` with `fit` and set aside the bytes
/// that are not a memory, rewriting the queue only when either changes
/// it. Reading and rewriting under one lock keeps a note queued between
/// them.
///
/// # Errors
///
/// The queue file cannot be rewritten, or the bytes cannot be set aside.
pub fn tidy_in(
    dir: &Path,
    fit: impl FnOnce(Vec<Queued>) -> Vec<Queued>,
) -> Result<Option<SetAside>> {
    let _held = lock(dir);
    let (notes, unread) = read_split(dir);
    let fitted = fit(notes.clone());
    if fitted == notes && unread.is_empty() {
        return Ok(None);
    }
    write(dir, &fitted, &unread)
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
    crate::append_locked_row(&path, line.as_bytes(), crate::Durability::Durable)
        .with_context(|| format!("append {}", path.display()))?;
    Ok(true)
}

/// Send each waiting note in `dir` through `send`, oldest first. A kept
/// or refused note leaves the queue. A note that has to wait stays, and
/// the drain still tries every later note.
///
/// # Errors
///
/// The queue file cannot be rewritten.
pub fn drain_in(dir: &Path, mut send: impl FnMut(&Queued) -> Sent) -> Result<Drained> {
    // The queue lock is the file's. Holding it across the pack post is
    // what made `ljos finish` wait out another seat's lesson. The post
    // runs outside the lock. The rewrite takes the lock again and drops
    // only the notes this drain finished, so a note queued during the
    // post stays.
    let held = lock(dir);
    let (notes, unread) = read_split(dir);
    let mut done = Drained::default();
    if notes.is_empty() {
        if !unread.is_empty() {
            done.set_aside.extend(write(dir, &[], &unread)?);
        }
        return Ok(done);
    }
    drop(held);
    let mut finished = Vec::with_capacity(notes.len());
    for note in &notes {
        match send(note) {
            Sent::Kept(id) => {
                done.kept.push((note.issue.clone(), id));
                finished.push(note);
            }
            Sent::Refused(why) => {
                done.refused.push((note.issue.clone(), why));
                finished.push(note);
            }
            Sent::Wait(why) => {
                if done.waiting_on.is_none() {
                    done.waiting_on = Some(why);
                }
            }
        }
    }
    let _held = lock(dir);
    let (mut rest, unread) = read_split(dir);
    for note in finished {
        if let Some(at) = rest.iter().position(|row| row.same(note)) {
            rest.remove(at);
        }
    }
    done.left = rest.len();
    done.set_aside.extend(write(dir, &rest, &unread)?);
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
    for aside in &done.set_aside {
        out.push_str(&format!(
            "memory: moved {} byte(s) of the note queue that are not a note to {}; they were not sent\n",
            aside.bytes,
            aside.path.display()
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
            seat: String::new(),
            home: String::new(),
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
        assert!(pending[0].seat.is_empty());
        assert!(pending[0].home.is_empty());
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

    /// Emptying a queue that is a dangling link is refused, and the link
    /// stays for doctor to name.
    #[test]
    fn emptying_a_dangling_linked_queue_is_refused() {
        let dir = dir("empty-dangling");
        let path = queue_file(&dir);
        std::os::unix::fs::symlink(dir.join("gone"), &path).unwrap();
        assert!(replace_in(&dir, Vec::new()).is_err());
        assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
    }

    /// A drain rewrites the queue with the notes left. A line that does
    /// not parse and a last line without its newline, even one that
    /// would, go to the day's damaged file byte for byte, not out with
    /// the rewrite.
    #[test]
    fn a_drain_sets_aside_what_it_cannot_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir("aside");
        let sent = format!("{}\n", serde_json::to_string(&note("p-1", "sent")).unwrap());
        let waits = format!(
            "{}\n",
            serde_json::to_string(&note("p-2", "waits")).unwrap()
        );
        let cut: &[u8] = b"{\"issue\":\"p-3\",\"te\n";
        let tail: &[u8] =
            br#"{"issue":"p-4","text":"no newline","workspace":"ws","at":1,"kind":"note"}"#;
        let mut bytes = sent.into_bytes();
        bytes.extend_from_slice(cut);
        bytes.extend_from_slice(waits.as_bytes());
        bytes.extend_from_slice(tail);
        std::fs::write(queue_file(&dir), &bytes).unwrap();
        let done = drain_in(&dir, |n| match n.issue.as_str() {
            "p-1" => Sent::Kept("id".into()),
            _ => Sent::Wait("down".into()),
        })
        .unwrap();
        assert_eq!(done.left, 1, "{done:?}");
        assert_eq!(std::fs::read_to_string(queue_file(&dir)).unwrap(), waits);
        let unread = [cut, tail].concat();
        let [aside] = done.set_aside.as_slice() else {
            panic!("{done:?}");
        };
        assert_eq!(aside.bytes, unread.len());
        assert_eq!(std::fs::read(&aside.path).unwrap(), unread);
        let name = aside.path.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with("note-queue.jsonl.") && name.ends_with(".damaged"),
            "{name}"
        );
        let mode = std::fs::metadata(&aside.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "damaged file mode {mode:o}");
        let lines = drained_lines(&done);
        assert!(lines.contains(&aside.path.display().to_string()), "{lines}");
    }

    /// A queue of nothing but bytes that are not a note is emptied into
    /// the damaged file, and the next tidy finds nothing to move.
    #[test]
    fn a_tidy_sets_aside_a_queue_with_no_note_in_it() {
        let dir = dir("tidy");
        std::fs::write(queue_file(&dir), b"{\"issue\":\"p-1\",\"te").unwrap();
        let aside = tidy_in(&dir, |notes| notes).unwrap().expect("set aside");
        assert_eq!(
            std::fs::read(&aside.path).unwrap(),
            b"{\"issue\":\"p-1\",\"te"
        );
        assert_eq!(std::fs::read(queue_file(&dir)).unwrap(), b"");
        assert_eq!(tidy_in(&dir, |notes| notes).unwrap(), None);
    }

    /// A damaged name that is a link, to a file outside the state dir or
    /// to nothing, is not listed, and its target is neither read nor
    /// changed.
    #[test]
    fn a_linked_damaged_name_is_not_followed() {
        let dir = dir("linked");
        let real = dir.join("note-queue.jsonl.2026-01-01.damaged");
        std::fs::write(&real, b"cut").unwrap();
        let outside = dir.parent().unwrap().join("linked-outside");
        std::fs::write(&outside, b"not ljos's own").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("note-queue.jsonl.2026-01-02.damaged"))
            .unwrap();
        std::os::unix::fs::symlink(
            dir.join("gone"),
            dir.join("note-queue.jsonl.2026-01-03.damaged"),
        )
        .unwrap();
        assert_eq!(set_aside_in(&dir), [(real, 3)]);
        assert_eq!(std::fs::read(&outside).unwrap(), b"not ljos's own");
        assert!(!dir.join("gone").exists());
    }

    /// A state dir swapped for a link to a dir that holds a damaged file
    /// lists nothing from that dir.
    #[test]
    fn a_linked_state_dir_lists_nothing_from_outside() {
        let root = dir("linked-dir");
        let outside = root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("note-queue.jsonl.2026-01-01.damaged"), b"x").unwrap();
        let state = root.join("ljos");
        std::os::unix::fs::symlink(&outside, &state).unwrap();
        assert_eq!(set_aside_in(&state), []);
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
    fn a_drain_keeps_waiting_rows_and_tries_later_ones() {
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
                "p-3" => Sent::Wait("connection refused".into()),
                _ => Sent::Kept("id-4".into()),
            }
        })
        .unwrap();
        assert_eq!(seen, ["p-1", "p-2", "p-3", "p-4"]);
        assert_eq!(
            done.kept,
            [
                ("p-1".to_string(), "id-1".to_string()),
                ("p-4".to_string(), "id-4".to_string())
            ]
        );
        assert_eq!(done.refused.len(), 1);
        assert_eq!(done.left, 1);
        let left: Vec<String> = pending_in(&dir).into_iter().map(|n| n.issue).collect();
        assert_eq!(left, ["p-3"]);
        let lines = drained_lines(&done);
        assert!(
            lines.contains("sent 2 queued note(s): p-1 id-1, p-4 id-4"),
            "{lines}"
        );
        assert!(
            lines.contains("1 note(s) still queued (connection refused)"),
            "{lines}"
        );
        let done = drain_in(&dir, |_| Sent::Kept("id".into())).unwrap();
        assert_eq!(done.kept.len(), 1);
        assert!(pending_in(&dir).is_empty());
        assert_eq!(
            drained_lines(&drain_in(&dir, |_| unreachable!()).unwrap()),
            ""
        );
    }

    /// The queue file stays locked only around its own read and rewrite.
    /// Another seat can queue a note while this one is asking the pack.
    #[test]
    fn a_drain_does_not_hold_the_queue_lock_while_the_pack_is_asked() {
        let dir = dir("lock-during-send");
        push_in(&dir, note("p-1", "held")).unwrap();
        let done = drain_in(&dir, |_| {
            assert!(
                busy_in(&dir).is_none(),
                "the queue lock was held while the pack was asked"
            );
            Sent::Kept("id".into())
        })
        .unwrap();
        assert_eq!(done.kept, [("p-1".to_string(), "id".to_string())]);
        assert!(pending_in(&dir).is_empty());
    }
}
