//! `ljos mesh pull` and `ljos mesh push`.
//!
//! A handoff directory holds one JSONL file per lane. `pull` reads those
//! files into a seat, keeps one copy of each text, and writes a seed whose
//! bytes follow from the texts and the lanes. `push` writes the seat's
//! memories back to that directory as one lane file.
//!
//! The directory and the seat each take an exclusive lock for the whole
//! run, so two runs wait and the seed keeps a single copy of each text.
//! A pack that answers 503 is asked again once, after `Retry-After`, with
//! the same deadline. A second 503 or a 408 stops the pull. Rows already
//! stored stay in the seed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::memory_port::FORMAT;

/// Where a seat keeps the seed `pull` writes.
#[derive(Clone, Debug)]
pub struct SeatStore {
    dir: PathBuf,
    seat: String,
}

impl SeatStore {
    /// The seat's mesh directory under the tracker memory home.
    ///
    /// # Errors
    ///
    /// `seat` is empty or names a path.
    pub fn open(seat: &str) -> Result<Self> {
        let seat = require_name(seat, "seat")?;
        let place = crate::memory_port::Place::for_seat(&seat);
        let dir = place
            .home
            .join("mesh")
            .join(crate::memory_port::encode_name(&seat));
        Ok(Self { dir, seat })
    }

    fn seed_path(&self) -> PathBuf {
        self.dir.join("seed.jsonl")
    }

    #[cfg(test)]
    fn at(dir: PathBuf, seat: &str) -> Self {
        Self {
            dir,
            seat: seat.to_string(),
        }
    }
}

/// What one `pull` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub imported: usize,
    pub skipped: usize,
}

impl Summary {
    /// The line `ljos mesh pull` prints.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "imported {}, skipped {} duplicates\n",
            self.imported, self.skipped
        )
    }
}

#[derive(Clone, Debug)]
struct Kept {
    content_sha256: String,
    text: String,
    kind: String,
    lanes: BTreeSet<String>,
    entities: BTreeSet<String>,
}

struct Incoming {
    text: String,
    kind: String,
    lanes: BTreeSet<String>,
    entities: BTreeSet<String>,
    copies: usize,
}

struct Held {
    file: std::fs::File,
}

impl Held {
    fn exclusive(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .open(path)
            .with_context(|| format!("mesh: cannot open {}", path.display()))?;
        // SAFETY: flock on a descriptor this struct owns until drop.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            bail!("mesh: cannot lock {}: {err}", path.display());
        }
        Ok(Self { file })
    }

    fn seat(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("mesh: cannot create {}", dir.display()))?;
        let path = dir.join("write.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("mesh: cannot open {}", path.display()))?;
        // SAFETY: flock on a descriptor this struct owns until drop.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            bail!("mesh: cannot lock {}: {err}", path.display());
        }
        Ok(Self { file })
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: flock on a descriptor this struct owns.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn require_name(name: &str, what: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        bail!("mesh: {what} {name:?} is not a single name");
    }
    Ok(name.to_string())
}

fn require_dir(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)
        .with_context(|| format!("mesh: cannot read {}", path.display()))?;
    if meta.file_type().is_symlink() {
        bail!("mesh: {} is a symlink", path.display());
    }
    if !meta.is_dir() {
        bail!("mesh: {} is not a directory", path.display());
    }
    Ok(())
}

fn content_hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn clean_lane(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        None
    } else {
        Some(name.to_string())
    }
}

fn lane_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".jsonl")?;
    clean_lane(stem)
}

fn entities_of(seat: &str, lanes: &BTreeSet<String>, extra: &BTreeSet<String>) -> Vec<String> {
    let mut all: BTreeSet<String> = extra
        .iter()
        .filter(|tag| !tag.starts_with("lane:") && !tag.starts_with("seat:"))
        .cloned()
        .collect();
    for lane in lanes {
        all.insert(format!("lane:{lane}"));
    }
    all.insert(format!("seat:{seat}"));
    all.into_iter().collect()
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn kind_of(value: &Value) -> Option<String> {
    match value.get("kind").and_then(Value::as_str) {
        Some(kind) if crate::memories::REVIEW_KINDS.contains(&kind) => Some(kind.to_string()),
        Some(_) => None,
        None => Some("lesson".to_string()),
    }
}

/// One lesson from a JSON object. `file_lane` is the handoff file's name.
/// Lane tags already on the object stay, so a file that was itself a push
/// keeps the lanes it gathered.
fn kept_from_line(value: &Value, file_lane: &str) -> Option<Kept> {
    if value["tombstone"] == Value::Bool(true) {
        return None;
    }
    let text = value["text"].as_str().unwrap_or("").trim();
    if text.is_empty() || crate::pack_copy::reject_unsafe(text).is_err() {
        return None;
    }
    let kind = kind_of(value)?;
    let mut lanes = BTreeSet::new();
    if let Some(name) = clean_lane(file_lane) {
        lanes.insert(name);
    }
    if let Some(name) = value["lane"].as_str().and_then(clean_lane) {
        lanes.insert(name);
    }
    for item in string_list(&value["lanes"]) {
        if let Some(name) = clean_lane(&item) {
            lanes.insert(name);
        }
    }
    let mut entities = BTreeSet::new();
    for tag in string_list(&value["entities"]) {
        if let Some(name) = tag.strip_prefix("lane:") {
            if let Some(name) = clean_lane(name) {
                lanes.insert(name);
            }
            continue;
        }
        if tag.starts_with("seat:") {
            continue;
        }
        entities.insert(tag);
    }
    if lanes.is_empty() {
        return None;
    }
    Some(Kept {
        content_sha256: content_hash(text),
        text: text.to_string(),
        kind,
        lanes,
        entities,
    })
}

fn values_of(bytes: &[u8]) -> Vec<Value> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let record = value["record"].as_str().unwrap_or("");
        if record == "export" || record == "seed" || value.get("text").is_none() {
            continue;
        }
        out.push(value);
    }
    out
}

fn lane_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("mesh: cannot read {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("mesh: cannot read {}", dir.display()))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
            continue;
        };
        if name.starts_with('.') || !name.ends_with(".jsonl") || name.ends_with(".seed.jsonl") {
            continue;
        }
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        files.push(path);
    }
    files.sort();
    Ok(files)
}

fn read_seed(path: &Path) -> Result<BTreeMap<String, Kept>> {
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let bytes =
        std::fs::read(path).with_context(|| format!("mesh: cannot read {}", path.display()))?;
    let mut rows = BTreeMap::new();
    for value in values_of(&bytes) {
        let Some(row) = kept_from_line(&value, "") else {
            continue;
        };
        rows.insert(row.content_sha256.clone(), row);
    }
    Ok(rows)
}

fn row_value(seat: &str, row: &Kept) -> Value {
    json!({
        "format": FORMAT,
        "kind": row.kind,
        "text": row.text,
        "origin": crate::admit::ORIGIN_PEER,
        "content_sha256": row.content_sha256,
        "lanes": row.lanes.iter().cloned().collect::<Vec<_>>(),
        "entities": entities_of(seat, &row.lanes, &row.entities),
    })
}

fn document(
    record: &str,
    seat: &str,
    machine: &str,
    rows: &BTreeMap<String, Kept>,
) -> Result<Vec<u8>> {
    let mut lanes = BTreeSet::new();
    for row in rows.values() {
        lanes.extend(row.lanes.iter().cloned());
    }
    let header = json!({
        "format": FORMAT,
        "record": record,
        "machine": machine,
        "seat": seat,
        "lanes": lanes.into_iter().collect::<Vec<_>>(),
    });
    let mut lines = vec![serde_json::to_string(&header)?];
    for row in rows.values() {
        lines.push(serde_json::to_string(&row_value(seat, row))?);
    }
    let mut body = lines.join("\n");
    body.push('\n');
    Ok(body.into_bytes())
}

fn unique() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mesh: cannot create {}", parent.display()))?;
    }
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        bail!("mesh: {} is a symlink", path.display());
    }
    if std::fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    let mut tmp_name = path
        .file_name()
        .context("mesh: path has no file name")?
        .to_os_string();
    tmp_name.push(format!(".{}.{}.tmp", std::process::id(), unique()));
    let tmp = path.with_file_name(tmp_name);
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .with_context(|| format!("mesh: cannot write {}", tmp.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("mesh: cannot place {}", path.display()));
    }
    Ok(())
}

fn write_seed(store: &SeatStore, rows: &BTreeMap<String, Kept>) -> Result<()> {
    let bytes = document("seed", &store.seat, &store.seat, rows)?;
    write_bytes(&store.seed_path(), &bytes)
}

fn merge_row(rows: &mut BTreeMap<String, Kept>, row: Kept) {
    match rows.get_mut(&row.content_sha256) {
        Some(have) => {
            have.lanes.extend(row.lanes);
            have.entities.extend(row.entities);
        }
        None => {
            rows.insert(row.content_sha256.clone(), row);
        }
    }
}

fn pack_transport(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<packset_client::Error>()
            .is_some_and(|inner| {
                matches!(
                    inner,
                    packset_client::Error::Io(_) | packset_client::Error::Http(_)
                )
            })
    })
}

fn stop_for_admission(err: &anyhow::Error) -> bool {
    crate::is_write_shed(err)
        || crate::is_deadline_miss(err)
        || crate::is_pack_busy(err)
        || pack_transport(err)
}

fn post_row(seat: &str, row: &Kept) -> Result<()> {
    let Ok(client) = crate::pack() else {
        return Ok(());
    };
    let lanes: Vec<&str> = row.lanes.iter().map(String::as_str).collect();
    let lane = lanes.first().copied().unwrap_or("");
    let atom = json!({
        "schema": "inside.atom/v1",
        "kind": row.kind,
        "text": row.text,
        "origin": crate::admit::ORIGIN_PEER,
        "entities": entities_of(seat, &row.lanes, &row.entities),
        "source": {
            "lane": lane,
            "lanes": lanes,
            "machine": lane,
            "seat": seat,
            "basis": "agent",
        },
        "workspace": client.workspace(),
    });
    match crate::admit::store_labelled(&client, atom) {
        Ok(_) => Ok(()),
        Err(err) if stop_for_admission(&err) => Err(err),
        Err(_) => Ok(()),
    }
}

fn pull_with(
    store: &SeatStore,
    from: &Path,
    mut post: impl FnMut(&Kept) -> Result<()>,
) -> Result<Summary> {
    require_name(&store.seat, "seat")?;
    require_dir(from)?;
    let _dir = Held::exclusive(from)?;
    let _seat = Held::seat(&store.dir)?;
    let mut have = read_seed(&store.seed_path())?;
    let mut incoming: BTreeMap<String, Incoming> = BTreeMap::new();
    for path in lane_files(from)? {
        let Some(lane) = lane_of(&path) else {
            continue;
        };
        let bytes = std::fs::read(&path)
            .with_context(|| format!("mesh: cannot read {}", path.display()))?;
        for value in values_of(&bytes) {
            let Some(row) = kept_from_line(&value, &lane) else {
                continue;
            };
            let slot = incoming
                .entry(row.content_sha256.clone())
                .or_insert_with(|| Incoming {
                    text: row.text.clone(),
                    kind: row.kind.clone(),
                    lanes: BTreeSet::new(),
                    entities: BTreeSet::new(),
                    copies: 0,
                });
            slot.copies += 1;
            slot.lanes.extend(row.lanes);
            slot.entities.extend(row.entities);
        }
    }
    let mut imported = 0usize;
    let mut skipped = 0usize;
    for (hash, inc) in incoming {
        if have.contains_key(&hash) {
            skipped += inc.copies;
            continue;
        }
        let row = Kept {
            content_sha256: hash.clone(),
            text: inc.text,
            kind: inc.kind,
            lanes: inc.lanes,
            entities: inc.entities,
        };
        if let Err(err) = post(&row) {
            let _ = write_seed(store, &have);
            return Err(err);
        }
        imported += 1;
        skipped += inc.copies.saturating_sub(1);
        have.insert(hash, row);
    }
    write_seed(store, &have)?;
    Ok(Summary { imported, skipped })
}

/// Import every lane file in `from` into `store`.
///
/// The summary counts new texts as imported and repeated texts as skipped.
/// A second pull of the same directory imports nothing.
///
/// # Errors
///
/// The directory is missing, or the pack sheds the write or passes its deadline.
pub fn pull(store: &SeatStore, from: &Path) -> Result<Summary> {
    let _deadline = crate::pack_link::CommandDeadline::enter(crate::IMPORT_ADMISSION);
    let seat = store.seat.clone();
    pull_with(store, from, |row| post_row(&seat, row))
}

fn push_with(store: &SeatStore, to: &Path, lane: &str, extra: &[Value]) -> Result<String> {
    require_name(&store.seat, "seat")?;
    let lane = require_name(lane, "lane")?;
    if std::fs::symlink_metadata(to).is_ok_and(|meta| meta.file_type().is_symlink()) {
        bail!("mesh: {} is a symlink", to.display());
    }
    std::fs::create_dir_all(to).with_context(|| format!("mesh: cannot create {}", to.display()))?;
    require_dir(to)?;
    let _dir = Held::exclusive(to)?;
    let _seat = Held::seat(&store.dir)?;
    let mut rows = read_seed(&store.seed_path())?;
    for value in extra {
        if let Some(row) = kept_from_line(value, &lane) {
            merge_row(&mut rows, row);
        }
    }
    let dest = to.join(format!("{lane}.jsonl"));
    let bytes = document("export", &store.seat, &lane, &rows)?;
    write_bytes(&dest, &bytes)?;
    Ok(format!(
        "pushed {} memories to {}\n",
        rows.len(),
        dest.display()
    ))
}

/// Write this seat's seed and its stored lessons to `to` as `<lane>.jsonl`.
///
/// # Errors
///
/// `lane` is not a file name, or the directory cannot be written.
pub fn push(store: &SeatStore, to: &Path, lane: &str) -> Result<String> {
    let place = crate::memory_port::Place::for_seat(&store.seat);
    let extra = crate::memory_port::portable_rows(&place);
    push_with(store, to, lane, &extra)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn write_lane(dir: &Path, lane: &str, texts: &[&str]) {
        let mut lines = vec![json!({
            "format": FORMAT,
            "record": "export",
            "machine": lane,
            "seat": lane,
        })
        .to_string()];
        for text in texts {
            lines.push(
                json!({
                    "format": FORMAT,
                    "kind": "lesson",
                    "text": text,
                })
                .to_string(),
            );
        }
        let mut body = lines.join("\n");
        body.push('\n');
        std::fs::write(dir.join(format!("{lane}.jsonl")), body).unwrap();
    }

    fn handoff(dir: &Path) -> (PathBuf, SeatStore) {
        let from = dir.join("handoff");
        std::fs::create_dir(&from).unwrap();
        let store = SeatStore::at(dir.join("seat"), "box");
        (from, store)
    }

    fn texts_of(dir: &Path) -> Vec<String> {
        let body = std::fs::read_to_string(dir.join("seat").join("seed.jsonl")).unwrap();
        body.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|value| value["text"].as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn pull_dedups_by_text_and_tags_each_lane() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        write_lane(&from, "lane-a", &["alpha kiln", "shared kiln"]);
        write_lane(&from, "lane-b", &["shared kiln", "beta kiln"]);
        let summary = pull_with(&store, &from, |_| Ok(())).unwrap();
        assert_eq!(
            summary,
            Summary {
                imported: 3,
                skipped: 1
            }
        );
        assert_eq!(summary.line(), "imported 3, skipped 1 duplicates\n");
        let mut texts = texts_of(dir.path());
        texts.sort();
        assert_eq!(texts, ["alpha kiln", "beta kiln", "shared kiln"]);
        let body = std::fs::read_to_string(store.seed_path()).unwrap();
        let shared = body
            .lines()
            .find(|line| line.contains("shared kiln"))
            .unwrap();
        let row: Value = serde_json::from_str(shared).unwrap();
        let tags = string_list(&row["entities"]);
        assert!(tags.contains(&"lane:lane-a".to_string()), "{row}");
        assert!(tags.contains(&"lane:lane-b".to_string()), "{row}");
        assert!(tags.contains(&"seat:box".to_string()), "{row}");
    }

    #[test]
    fn a_second_pull_imports_nothing_and_leaves_the_seed() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        write_lane(&from, "lane-a", &["alpha kiln", "shared kiln"]);
        write_lane(&from, "lane-b", &["shared kiln", "beta kiln"]);
        pull_with(&store, &from, |_| Ok(())).unwrap();
        let first = std::fs::read(store.seed_path()).unwrap();
        let again = pull_with(&store, &from, |_| Ok(())).unwrap();
        assert_eq!(
            again,
            Summary {
                imported: 0,
                skipped: 4
            }
        );
        assert_eq!(std::fs::read(store.seed_path()).unwrap(), first);
        assert!(!first.windows(11).any(|w| w == b"exported_at"));
    }

    #[test]
    fn concurrent_pulls_import_each_text_once() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        write_lane(&from, "lane-a", &["alpha kiln", "shared kiln"]);
        write_lane(&from, "lane-b", &["shared kiln", "beta kiln"]);
        let gate = Arc::new(Barrier::new(2));
        let run = |gate: Arc<Barrier>, store: SeatStore, from: PathBuf| {
            thread::spawn(move || {
                gate.wait();
                pull_with(&store, &from, |_| Ok(())).unwrap()
            })
        };
        let left = run(Arc::clone(&gate), store.clone(), from.clone());
        let right = run(gate, store, from);
        let left = left.join().unwrap();
        let right = right.join().unwrap();
        assert_eq!(left.imported + right.imported, 3, "{left:?} {right:?}");
        assert_eq!(left.skipped + right.skipped, 5, "{left:?} {right:?}");
        assert_eq!(texts_of(dir.path()).len(), 3);
    }

    #[test]
    fn a_deadline_keeps_the_rows_already_stored() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        write_lane(&from, "lane-a", &["alpha kiln", "beta kiln", "gamma kiln"]);
        let calls = AtomicU64::new(0);
        let err = pull_with(&store, &from, |_| {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            if n == 1 {
                Err(anyhow::Error::new(packset_client::Error::Unavailable(
                    crate::PACK_DEADLINE_LINE.to_string(),
                )))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(crate::is_deadline_miss(&err), "{err:#}");
        assert_eq!(texts_of(dir.path()).len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn push_writes_a_stable_lane_file() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        write_lane(&from, "lane-a", &["alpha kiln", "shared kiln"]);
        write_lane(&from, "lane-b", &["shared kiln"]);
        pull_with(&store, &from, |_| Ok(())).unwrap();
        let dest = dir.path().join("out");
        let line = push_with(&store, &dest, "box", &[]).unwrap();
        assert!(line.starts_with("pushed 2 memories to "), "{line}");
        let first = std::fs::read(dest.join("box.jsonl")).unwrap();
        let again = push_with(&store, &dest, "box", &[]).unwrap();
        assert_eq!(again, line);
        assert_eq!(std::fs::read(dest.join("box.jsonl")).unwrap(), first);
        let body = String::from_utf8(first).unwrap();
        assert!(body
            .lines()
            .next()
            .unwrap()
            .contains("\"record\":\"export\""));
        assert!(body.contains("alpha kiln") && body.contains("shared kiln"));
    }

    #[test]
    fn concurrent_pushes_leave_both_lane_files() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");
        std::fs::create_dir(&dest).unwrap();
        let gate = Arc::new(Barrier::new(2));
        let spawn = |lane: &'static str, text: &'static str, gate: Arc<Barrier>| {
            let dest = dest.clone();
            let root = dir.path().join(lane);
            thread::spawn(move || {
                let store = SeatStore::at(root, "box");
                gate.wait();
                let extra = vec![json!({"kind": "lesson", "text": text})];
                push_with(&store, &dest, lane, &extra).unwrap()
            })
        };
        let left = spawn("lane-a", "alpha kiln", Arc::clone(&gate));
        let right = spawn("lane-b", "beta kiln", gate);
        left.join().unwrap();
        right.join().unwrap();
        let alpha = std::fs::read_to_string(dest.join("lane-a.jsonl")).unwrap();
        let beta = std::fs::read_to_string(dest.join("lane-b.jsonl")).unwrap();
        assert!(alpha.contains("alpha kiln"), "{alpha}");
        assert!(beta.contains("beta kiln"), "{beta}");
        assert!(!alpha.contains("beta kiln"), "{alpha}");
    }
}
