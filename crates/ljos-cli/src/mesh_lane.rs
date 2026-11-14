//! `ljos mesh pull` and `ljos mesh push`.
//!
//! A handoff directory holds one JSONL file per lane. `pull` reads those
//! files into a seat, keeps one copy of each text, and writes a seed whose
//! bytes follow from the texts and the lanes. A text or a lane row id the
//! seat already holds is unchanged. New rows land in the seat's mesh slot
//! as `imported,unintegrated`, and `pull` then integrates them, which is
//! the same step as `ljos memory integrate --all`. `push` writes the
//! seat's memories back to that directory as one lane file.
//!
//! The directory and the seat each take an exclusive lock for the whole
//! run, so two runs wait and the seed keeps a single copy of each text.
//! A pack that answers 503 is asked again once, after `Retry-After`, with
//! the same deadline. A second 503 or a 408 stops the pull. Rows already
//! stored stay in the seed. A handoff or a seed folder that has gone away,
//! or a write that does not return, stops the command: the line names the
//! folder and the command to run once the mount answers, and nothing is
//! reported as imported or pushed.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::share_io::Share;
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
    pub unchanged: usize,
    pub skipped: usize,
}

impl Summary {
    /// The line `ljos mesh pull` prints.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "imported {}, unchanged {}, skipped {} duplicates\n",
            self.imported, self.unchanged, self.skipped
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
    /// `lane` + tab + the export's row id. Empty when the line names none.
    row_ids: BTreeSet<String>,
}

struct Incoming {
    text: String,
    kind: String,
    lanes: BTreeSet<String>,
    entities: BTreeSet<String>,
    row_ids: BTreeSet<String>,
    copies: usize,
}

struct Held {
    file: std::fs::File,
}

impl Held {
    fn exclusive(path: &Path, share: &Share) -> Result<Self> {
        share
            .lock_shared(path)
            .map(|file| Self { file })
            .map_err(|err| share.fail(path, err))
    }

    fn seat(dir: &Path, share: &Share) -> Result<Self> {
        share.create_dir(dir).map_err(|err| share.fail(dir, err))?;
        let path = dir.join("write.lock");
        share
            .lock_create(&path)
            .map(|file| Self { file })
            .map_err(|err| share.fail(dir, err))
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
        bail!("mesh: {what} {name:?} is not a single name. Pass --{what} NAME with no slash");
    }
    Ok(name.to_string())
}

fn require_dir(path: &Path, share: &Share) -> Result<()> {
    let meta = match share.symlink_meta(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            bail!(
                "mesh: {} does not exist. Create it, or pass a directory of <lane>.jsonl files",
                path.display()
            );
        }
        Err(err) => return Err(share.fail(path, err)),
    };
    if meta.file_type().is_symlink() {
        bail!(
            "mesh: {} is a symlink. Pass the directory itself, not a link",
            path.display()
        );
    }
    if !meta.is_dir() {
        bail!(
            "mesh: {} is not a directory. Pass a directory of <lane>.jsonl files",
            path.display()
        );
    }
    Ok(())
}

fn content_hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// The id a lane line already has: the pack atom, then `row_id`, then `id`.
fn raw_row_id(value: &Value) -> Option<String> {
    for key in ["atom_id", "row_id", "id"] {
        let Some(id) = value.get(key).and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if id.is_empty() || id.contains('\t') || id.contains('\n') || id.contains('\0') {
            continue;
        }
        return Some(id.to_string());
    }
    None
}

fn row_key(lane: &str, id: &str) -> String {
    format!("{lane}\t{id}")
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
    let mut row_ids = BTreeSet::new();
    for id in string_list(&value["row_ids"]) {
        if id.contains('\t') && !id.contains('\n') {
            row_ids.insert(id);
        }
    }
    if let Some(id) = raw_row_id(value) {
        for lane in &lanes {
            row_ids.insert(row_key(lane, &id));
        }
    }
    Some(Kept {
        content_sha256: content_hash(text),
        text: text.to_string(),
        kind,
        lanes,
        entities,
        row_ids,
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

fn lane_files(dir: &Path, share: &Share) -> Result<Vec<PathBuf>> {
    match share.list_files(dir) {
        Ok(files) => Ok(files),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            bail!(
                "mesh: {} does not exist. Create it, or pass a directory of <lane>.jsonl files",
                dir.display()
            );
        }
        Err(err) => Err(share.fail(dir, err)),
    }
}

fn read_seed(path: &Path, share: &Share) -> Result<(BTreeMap<String, Kept>, BTreeSet<String>)> {
    let folder = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let bytes = match share.read_if_file(path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok((BTreeMap::new(), BTreeSet::new())),
        Err(err) => return Err(share.fail(folder, err)),
    };
    let mut rows = BTreeMap::new();
    let mut row_ids = BTreeSet::new();
    for value in values_of(&bytes) {
        let Some(row) = kept_from_line(&value, "") else {
            continue;
        };
        row_ids.extend(row.row_ids.iter().cloned());
        rows.insert(row.content_sha256.clone(), row);
    }
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let record = value["record"].as_str().unwrap_or("");
        if record == "seed" || record == "export" {
            row_ids.extend(string_list(&value["row_ids"]));
        }
    }
    Ok((rows, row_ids))
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
        "row_ids": row.row_ids.iter().cloned().collect::<Vec<_>>(),
    })
}

fn document(
    record: &str,
    seat: &str,
    machine: &str,
    rows: &BTreeMap<String, Kept>,
    extra_ids: &BTreeSet<String>,
) -> Result<Vec<u8>> {
    let mut lanes = BTreeSet::new();
    let mut row_ids = extra_ids.clone();
    for row in rows.values() {
        lanes.extend(row.lanes.iter().cloned());
        row_ids.extend(row.row_ids.iter().cloned());
    }
    let header = json!({
        "format": FORMAT,
        "record": record,
        "machine": machine,
        "seat": seat,
        "lanes": lanes.into_iter().collect::<Vec<_>>(),
        "row_ids": row_ids.into_iter().collect::<Vec<_>>(),
    });
    let mut lines = vec![serde_json::to_string(&header)?];
    for row in rows.values() {
        lines.push(serde_json::to_string(&row_value(seat, row))?);
    }
    let mut body = lines.join("\n");
    body.push('\n');
    Ok(body.into_bytes())
}

fn write_seed(
    store: &SeatStore,
    rows: &BTreeMap<String, Kept>,
    row_ids: &BTreeSet<String>,
    share: &Share,
) -> Result<()> {
    let bytes = document("seed", &store.seat, &store.seat, rows, row_ids)?;
    share.write_atomic(&store.seed_path(), &bytes)
}

fn merge_row(rows: &mut BTreeMap<String, Kept>, row: Kept) {
    match rows.get_mut(&row.content_sha256) {
        Some(have) => {
            have.lanes.extend(row.lanes);
            have.entities.extend(row.entities);
            have.row_ids.extend(row.row_ids);
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

fn post_row(seat: &str, row: &Kept) -> Result<Option<String>> {
    let Ok(client) = crate::pack() else {
        return Ok(None);
    };
    let lanes: Vec<&str> = row.lanes.iter().map(String::as_str).collect();
    let lane = lanes.first().copied().unwrap_or("");
    let mut atom = json!({
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
    if atom["entities"].as_array().is_none_or(|tags| {
        !tags.iter().any(|tag| {
            tag.as_str() == Some("horizon:standing") || tag.as_str() == Some("horizon:transient")
        })
    }) {
        atom["entities"]
            .as_array_mut()
            .expect("entities")
            .push(json!("horizon:standing"));
    }
    match crate::admit::store_labelled(&client, atom) {
        Ok(posted) => Ok(posted["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_string)),
        Err(err) if stop_for_admission(&err) => Err(err),
        Err(_) => Ok(None),
    }
}

fn pull_with(
    store: &SeatStore,
    from: &Path,
    already_hashes: &BTreeSet<String>,
    already_ids: &BTreeSet<String>,
    mut post: impl FnMut(&Kept) -> Result<()>,
) -> Result<Summary> {
    require_name(&store.seat, "seat")?;
    let share = Share::mesh_pull(&store.seat, from);
    require_dir(from, &share)?;
    let _dir = Held::exclusive(from, &share)?;
    let _seat = Held::seat(&store.dir, &share)?;
    let (mut have, mut row_ids) = read_seed(&store.seed_path(), &share)?;
    row_ids.extend(already_ids.iter().cloned());
    let mut known_hashes = already_hashes.clone();
    known_hashes.extend(have.keys().cloned());
    let mut incoming: BTreeMap<String, Incoming> = BTreeMap::new();
    for path in lane_files(from, &share)? {
        let Some(lane) = lane_of(&path) else {
            continue;
        };
        let bytes = share.read(&path).map_err(|err| share.fail(from, err))?;
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
                    row_ids: BTreeSet::new(),
                    copies: 0,
                });
            slot.copies += 1;
            slot.lanes.extend(row.lanes);
            slot.entities.extend(row.entities);
            slot.row_ids.extend(row.row_ids);
        }
    }
    let mut imported = 0usize;
    let mut unchanged = 0usize;
    let mut skipped = 0usize;
    for (hash, inc) in incoming {
        let id_known = inc.row_ids.iter().any(|id| row_ids.contains(id));
        if known_hashes.contains(&hash) || id_known {
            unchanged += inc.copies;
            row_ids.extend(inc.row_ids.iter().cloned());
            if let Some(existing) = have.get_mut(&hash) {
                existing.lanes.extend(inc.lanes);
                existing.entities.extend(inc.entities);
                existing.row_ids.extend(inc.row_ids);
            }
            continue;
        }
        let row = Kept {
            content_sha256: hash.clone(),
            text: inc.text,
            kind: inc.kind,
            lanes: inc.lanes,
            entities: inc.entities,
            row_ids: inc.row_ids,
        };
        if let Err(err) = post(&row) {
            let _ = write_seed(store, &have, &row_ids, &share);
            return Err(err);
        }
        imported += 1;
        skipped += inc.copies.saturating_sub(1);
        row_ids.extend(row.row_ids.iter().cloned());
        known_hashes.insert(hash.clone());
        have.insert(hash, row);
    }
    write_seed(store, &have, &row_ids, &share)?;
    Ok(Summary {
        imported,
        unchanged,
        skipped,
    })
}

/// Import every lane file in `from` into `store`.
///
/// The summary counts new texts as imported, a text or a lane row id the
/// seat already holds as unchanged, and a repeated text in this pull as
/// skipped. A second pull of the same directory imports nothing.
///
/// After the seed is written, `pull` integrates every import in the seat.
/// That is [`crate::memory_port::integrate`] with `--all`: each
/// `imported,unintegrated` row is deduplicated against the seat's memories,
/// posted once with `lane:` and `seat:` tags, and relabelled
/// `imported,integrated`.
///
/// # Errors
///
/// The directory is missing, the handoff or the seed folder did not take a
/// read or a write, or the pack sheds the write or passes its deadline.
/// The summary is returned only after the seed is in place.
pub fn pull(store: &SeatStore, from: &Path) -> Result<Summary> {
    let _deadline = crate::pack_link::CommandDeadline::enter(crate::IMPORT_ADMISSION);
    let place = crate::memory_port::Place::for_seat(&store.seat);
    let (mut hashes, ids) = crate::memory_port::known_mesh(&place);
    if let Ok(atoms) = crate::memories::live_atoms() {
        for atom in atoms {
            if let Some(text) = atom["text"]
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                hashes.insert(content_hash(text));
            }
        }
    }
    let seat = store.seat.clone();
    let summary = pull_with(store, from, &hashes, &ids, |row| {
        // Stage only after the post is accepted or the seat has no writer.
        // A refused connection used to stage the row first, then stop, and
        // the next pull counted that hash as unchanged and left it out of
        // the seed.
        let mesh_row = crate::memory_port::MeshRow {
            text: row.text.clone(),
            kind: row.kind.clone(),
            origin: crate::admit::ORIGIN_PEER.into(),
            content_sha256: row.content_sha256.clone(),
            source_id: row
                .row_ids
                .iter()
                .find_map(|key| key.split_once('\t').map(|(_, id)| id.to_string()))
                .unwrap_or_default(),
            lanes: row.lanes.clone(),
            entities: row.entities.clone(),
        };
        match post_row(&seat, row) {
            Ok(posted) => {
                crate::memory_port::stage_mesh_rows(&place, &[mesh_row])?;
                if let Some(id) = posted {
                    crate::memory_port::mark_mesh_posted(&place, &row.content_sha256, &id)?;
                }
                Ok(())
            }
            Err(err) => Err(pull_stopped(&seat, from, err)),
        }
    })
    .map_err(|err| pull_stopped(&store.seat, from, err))?;
    crate::memory_port::integrate(&place, None, true, false)
        .map_err(|err| pull_stopped(&store.seat, from, err))?;
    Ok(summary)
}

/// One line when a post stops the pull. The source error is dropped so the
/// line is not replaced by a bare connection failure, and so it does not
/// grow a cause chain.
fn pull_stopped(seat: &str, from: &Path, err: anyhow::Error) -> anyhow::Error {
    if !stop_for_admission(&err) {
        return err;
    }
    let cause = if crate::is_deadline_miss(&err) {
        crate::PACK_DEADLINE_LINE.to_string()
    } else if crate::is_write_shed(&err) || crate::is_read_shed(&err) {
        crate::pack_fault_line(&err)
    } else {
        crate::pack_down_line()
    };
    anyhow::anyhow!(
        "mesh: the pull stopped: {cause}, and lessons already stored stay in the seed with rows still missing, so run `ljos mesh pull --seat {seat} --from {}`.",
        from.display()
    )
}

fn push_with(store: &SeatStore, to: &Path, lane: &str, extra: &[Value]) -> Result<String> {
    require_name(&store.seat, "seat")?;
    let lane = require_name(lane, "lane")?;
    let share = Share::mesh_push(&store.seat, &lane, to);
    match share.symlink_meta(to) {
        Ok(meta) if meta.file_type().is_symlink() => {
            bail!(
                "mesh: {} is a symlink. Pass the directory itself, not a link",
                to.display()
            );
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(share.fail(to, err)),
    }
    share.create_dir(to).map_err(|err| share.fail(to, err))?;
    require_dir(to, &share)?;
    let _dir = Held::exclusive(to, &share)?;
    let _seat = Held::seat(&store.dir, &share)?;
    let (mut rows, row_ids) = read_seed(&store.seed_path(), &share)?;
    for value in extra {
        if let Some(row) = kept_from_line(value, &lane) {
            merge_row(&mut rows, row);
        }
    }
    let dest = to.join(format!("{lane}.jsonl"));
    let bytes = document("export", &store.seat, &lane, &rows, &row_ids)?;
    share.write_atomic(&dest, &bytes)?;
    let queued = crate::note_queue::pending_in(&crate::note_queue::state_dir())
        .iter()
        .filter(|note| note.kind == "lesson")
        .count();
    Ok(push_summary(rows.len(), &dest, queued))
}

/// What `ljos mesh push` prints. A queue of lessons and an empty pack is
/// the queue, not `pushed 0 memories`.
fn push_summary(rows: usize, dest: &Path, queued: usize) -> String {
    if rows == 0 && queued > 0 {
        let word = if queued == 1 { "lesson" } else { "lessons" };
        return format!("{queued} {word} queued, not pushed\n");
    }
    let mut line = format!("pushed {rows} memories to {}", dest.display());
    if queued > 0 {
        let word = if queued == 1 { "lesson" } else { "lessons" };
        line.push_str(&format!("; {queued} {word} still queued"));
    }
    line.push('\n');
    line
}

/// Write this seat's seed and its stored lessons to `to` as `<lane>.jsonl`.
///
/// # Errors
///
/// `lane` is not a file name, or the directory did not take the file.
pub fn push(store: &SeatStore, to: &Path, lane: &str) -> Result<String> {
    let place = crate::memory_port::Place::for_seat(&store.seat);
    let extra = crate::memory_port::portable_rows(&place);
    push_with(store, to, lane, &extra)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
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
        let summary = pull_with(
            &store,
            &from,
            &BTreeSet::new(),
            &BTreeSet::new(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(
            summary,
            Summary {
                imported: 3,
                unchanged: 0,
                skipped: 1
            }
        );
        assert_eq!(
            summary.line(),
            "imported 3, unchanged 0, skipped 1 duplicates\n"
        );
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
        pull_with(
            &store,
            &from,
            &BTreeSet::new(),
            &BTreeSet::new(),
            |_| Ok(()),
        )
        .unwrap();
        let first = std::fs::read(store.seed_path()).unwrap();
        let again = pull_with(
            &store,
            &from,
            &BTreeSet::new(),
            &BTreeSet::new(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(
            again,
            Summary {
                imported: 0,
                unchanged: 4,
                skipped: 0
            }
        );
        assert_eq!(std::fs::read(store.seed_path()).unwrap(), first);
        assert!(!first.windows(11).any(|w| w == b"exported_at"));
    }

    #[test]
    fn a_known_lane_row_id_is_unchanged_when_the_text_changes() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        let write = |text: &str| {
            let body = format!(
                "{}\n{}\n",
                json!({
                    "format": FORMAT,
                    "record": "export",
                    "machine": "brine",
                    "seat": "brine",
                }),
                json!({
                    "format": FORMAT,
                    "kind": "lesson",
                    "text": text,
                    "atom_id": "row-7",
                })
            );
            std::fs::write(from.join("brine.jsonl"), body).unwrap();
        };
        write("alpha kiln");
        let first = pull_with(
            &store,
            &from,
            &BTreeSet::new(),
            &BTreeSet::new(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(first.imported, 1);
        write("alpha kiln moved");
        let posts = AtomicU64::new(0);
        let again = pull_with(&store, &from, &BTreeSet::new(), &BTreeSet::new(), |_| {
            posts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            again,
            Summary {
                imported: 0,
                unchanged: 1,
                skipped: 0
            }
        );
        assert_eq!(posts.load(Ordering::SeqCst), 0);
        let body = std::fs::read_to_string(store.seed_path()).unwrap();
        assert!(body.contains("alpha kiln"), "{body}");
        assert!(!body.contains("moved"), "{body}");
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
                pull_with(
                    &store,
                    &from,
                    &BTreeSet::new(),
                    &BTreeSet::new(),
                    |_| Ok(()),
                )
                .unwrap()
            })
        };
        let left = run(Arc::clone(&gate), store.clone(), from.clone());
        let right = run(gate, store, from);
        let left = left.join().unwrap();
        let right = right.join().unwrap();
        assert_eq!(left.imported + right.imported, 3, "{left:?} {right:?}");
        assert_eq!(
            left.skipped + right.skipped + left.unchanged + right.unchanged,
            5,
            "{left:?} {right:?}"
        );
        assert_eq!(texts_of(dir.path()).len(), 3);
    }

    #[test]
    fn a_deadline_keeps_the_rows_already_stored() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        write_lane(&from, "lane-a", &["alpha kiln", "beta kiln", "gamma kiln"]);
        let calls = AtomicU64::new(0);
        let err = pull_with(&store, &from, &BTreeSet::new(), &BTreeSet::new(), |_| {
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
        pull_with(
            &store,
            &from,
            &BTreeSet::new(),
            &BTreeSet::new(),
            |_| Ok(()),
        )
        .unwrap();
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
