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
//! After an export is stored, the seat records its size, its modification
//! time and a hash of its bytes. The next pull stats each export and does
//! not read one whose size and time match. A changed time is hashed, and
//! the same bytes are not imported again. The run says how many exports
//! were skipped and how many were imported. On a terminal, each export
//! prints one line with its count. A pipe prints the summary lines only.
//! The seed is written by a
//! temporary name and a rename, and only when its bytes would change.
//! A pack that answers 503 is asked again once, after `Retry-After`, with
//! the same deadline. A second 503 or a 408 stops the pull. Each export is
//! stored before the next one is read, so a stop keeps the finished exports
//! and leaves the export in progress out. The next pull continues with
//! what is not stored yet. A handoff or a seed folder that has gone away,
//! or a write that does not return, stops the command: the line names the
//! folder and the command to run once the mount answers, and nothing is
//! reported as imported or pushed.

use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal;
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
    /// Exports whose size and modification time matched, or whose bytes
    /// hashed the same, so this run did not import them.
    pub exports_skipped: usize,
    /// Exports whose bytes were new to the seat's record of that file.
    pub exports_imported: usize,
}

impl Summary {
    /// The lines `ljos mesh pull` prints.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "imported {}, unchanged {}, skipped {} duplicates\nexports skipped {}, imported {}\n",
            self.imported,
            self.unchanged,
            self.skipped,
            self.exports_skipped,
            self.exports_imported
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

fn file_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Size and modification time of one export, enough to skip a read.
struct ExportMeta {
    size: u64,
    mtime_secs: i64,
    mtime_nanos: u32,
}

impl ExportMeta {
    fn of(meta: &std::fs::Metadata) -> std::io::Result<Self> {
        let modified = meta.modified()?;
        let (mtime_secs, mtime_nanos) = match modified.duration_since(std::time::UNIX_EPOCH) {
            Ok(dur) => (
                i64::try_from(dur.as_secs()).unwrap_or(i64::MAX),
                dur.subsec_nanos(),
            ),
            Err(err) => {
                let dur = err.duration();
                let secs = i64::try_from(dur.as_secs()).unwrap_or(i64::MAX);
                (-secs, dur.subsec_nanos())
            }
        };
        Ok(Self {
            size: meta.len(),
            mtime_secs,
            mtime_nanos,
        })
    }

    fn matches(&self, mark: &ExportMark) -> bool {
        self.size == mark.size
            && self.mtime_secs == mark.mtime_secs
            && self.mtime_nanos == mark.mtime_nanos
    }
}

/// What the seat recorded for one export after that export was stored.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExportMark {
    size: u64,
    mtime_secs: i64,
    mtime_nanos: u32,
    sha256: String,
    rows: usize,
}

const EXPORTS_FORMAT: &str = "ljos-mesh-exports-1";

fn marks_path(store: &SeatStore) -> PathBuf {
    store.dir.join("exports.json")
}

fn mark_from(item: &Value) -> Option<ExportMark> {
    let size = item.get("size")?.as_u64()?;
    let mtime_secs = item.get("mtime_secs")?.as_i64()?;
    let mtime_nanos = u32::try_from(item.get("mtime_nanos")?.as_u64()?).ok()?;
    let sha256 = item.get("sha256")?.as_str().unwrap_or("");
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let rows = usize::try_from(item.get("rows")?.as_u64()?).ok()?;
    Some(ExportMark {
        size,
        mtime_secs,
        mtime_nanos,
        sha256: sha256.to_string(),
        rows,
    })
}

fn load_marks(bytes: &[u8]) -> BTreeMap<String, ExportMark> {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return BTreeMap::new();
    };
    if value.get("format").and_then(Value::as_str) != Some(EXPORTS_FORMAT) {
        return BTreeMap::new();
    }
    let Some(files) = value.get("files").and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    let mut marks = BTreeMap::new();
    for (name, item) in files {
        if let Some(mark) = mark_from(item) {
            marks.insert(name.clone(), mark);
        }
    }
    marks
}

fn marks_document(marks: &BTreeMap<String, ExportMark>) -> Result<Vec<u8>> {
    let mut files = serde_json::Map::new();
    for (name, mark) in marks {
        files.insert(
            name.clone(),
            json!({
                "mtime_nanos": mark.mtime_nanos,
                "mtime_secs": mark.mtime_secs,
                "rows": mark.rows,
                "sha256": mark.sha256,
                "size": mark.size,
            }),
        );
    }
    let mut bytes = serde_json::to_vec(&json!({
        "files": Value::Object(files),
        "format": EXPORTS_FORMAT,
    }))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_marks(store: &SeatStore, share: &Share) -> Result<BTreeMap<String, ExportMark>> {
    match share.read_if_file(&marks_path(store)) {
        Ok(Some(bytes)) => Ok(load_marks(&bytes)),
        Ok(None) => Ok(BTreeMap::new()),
        Err(err) => Err(share.fail(&store.dir, err)),
    }
}

fn write_marks(
    store: &SeatStore,
    marks: &BTreeMap<String, ExportMark>,
    share: &Share,
) -> Result<()> {
    share.write_atomic(&marks_path(store), &marks_document(marks)?)
}

fn remember(marks: &mut BTreeMap<String, ExportMark>, name: &str, mark: ExportMark) -> bool {
    if marks.get(name) == Some(&mark) {
        return false;
    }
    marks.insert(name.to_string(), mark);
    true
}

/// Lane exports the pull reads. Tests substitute a reader that counts bodies.
trait ExportSource {
    fn list(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>>;
    fn meta(&self, path: &Path) -> std::io::Result<Option<ExportMeta>>;
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
}

struct ShareRead<'a> {
    share: &'a Share,
}

impl ExportSource for ShareRead<'_> {
    fn list(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        self.share.list_files(dir)
    }

    fn meta(&self, path: &Path) -> std::io::Result<Option<ExportMeta>> {
        let meta = self.share.symlink_meta(path)?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Ok(None);
        }
        ExportMeta::of(&meta).map(Some)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        Share::read(self.share, path)
    }
}

fn grow(into: &mut BTreeSet<String>, extra: &BTreeSet<String>) -> bool {
    let before = into.len();
    into.extend(extra.iter().cloned());
    into.len() != before
}

struct Fold {
    rows: usize,
    imported: usize,
    unchanged: usize,
    skipped: usize,
    seed_changed: bool,
}

struct Known {
    hashes: BTreeSet<String>,
    ids: BTreeSet<String>,
}

/// Merge one export into the seat. On `Err` the caller drops this export's
/// rows, so a stop does not leave a half-imported export in the seed.
fn fold_export(
    bytes: &[u8],
    lane: &str,
    have: &mut BTreeMap<String, Kept>,
    row_ids: &mut BTreeSet<String>,
    known_hashes: &mut BTreeSet<String>,
    baseline: &Known,
    post: &mut impl FnMut(&Kept) -> Result<()>,
) -> Result<Fold> {
    let mut fold = Fold {
        rows: 0,
        imported: 0,
        unchanged: 0,
        skipped: 0,
        seed_changed: false,
    };
    for value in values_of(bytes) {
        let Some(row) = kept_from_line(&value, lane) else {
            continue;
        };
        fold.rows += 1;
        let hash_known = known_hashes.contains(&row.content_sha256);
        let id_known = row.row_ids.iter().any(|id| row_ids.contains(id));
        if hash_known || id_known {
            let introduced = !baseline.hashes.contains(&row.content_sha256)
                && row.row_ids.iter().all(|id| !baseline.ids.contains(id));
            if hash_known && introduced {
                fold.skipped += 1;
            } else {
                fold.unchanged += 1;
            }
            if grow(row_ids, &row.row_ids) {
                fold.seed_changed = true;
            }
            if let Some(existing) = have.get_mut(&row.content_sha256) {
                let lanes = grow(&mut existing.lanes, &row.lanes);
                let entities = grow(&mut existing.entities, &row.entities);
                let ids = grow(&mut existing.row_ids, &row.row_ids);
                if lanes || entities || ids {
                    fold.seed_changed = true;
                }
            } else if hash_known {
                // Posted on a pull that stopped before this export was
                // stored. Record it once, and do not post it again.
                have.insert(row.content_sha256.clone(), row);
                fold.seed_changed = true;
            }
            continue;
        }
        post(&row)?;
        fold.imported += 1;
        known_hashes.insert(row.content_sha256.clone());
        if grow(row_ids, &row.row_ids) {
            fold.seed_changed = true;
        }
        have.insert(row.content_sha256.clone(), row);
        fold.seed_changed = true;
    }
    Ok(fold)
}

fn export_paths(dir: &Path, source: &impl ExportSource, share: &Share) -> Result<Vec<PathBuf>> {
    let listed = match source.list(dir) {
        Ok(files) => files,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            bail!(
                "mesh: {} does not exist. Create it, or pass a directory of <lane>.jsonl files",
                dir.display()
            );
        }
        Err(err) => return Err(share.fail(dir, err)),
    };
    let mut paths = Vec::new();
    for path in listed {
        if lane_of(&path).is_some() && path.file_name().and_then(|item| item.to_str()).is_some() {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
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

/// A progress line for one export, or nothing when `stderr` is not a terminal.
fn export_progress(
    tty: bool,
    done: usize,
    total: usize,
    name: &str,
    action: &str,
) -> Option<String> {
    if !tty {
        return None;
    }
    Some(format!("mesh pull: {name} {action} ({done}/{total})"))
}

fn pull_with(
    store: &SeatStore,
    from: &Path,
    already_hashes: &BTreeSet<String>,
    already_ids: &BTreeSet<String>,
    post: impl FnMut(&Kept) -> Result<()>,
) -> Result<Summary> {
    let share = Share::mesh_pull(&store.seat, from);
    let source = ShareRead { share: &share };
    let already = Known {
        hashes: already_hashes.clone(),
        ids: already_ids.clone(),
    };
    pull_body(store, from, &already, &share, &source, post, |_| Ok(()))
}

fn pull_body(
    store: &SeatStore,
    from: &Path,
    already: &Known,
    share: &Share,
    source: &impl ExportSource,
    mut post: impl FnMut(&Kept) -> Result<()>,
    mut halt: impl FnMut(usize) -> Result<()>,
) -> Result<Summary> {
    require_name(&store.seat, "seat")?;
    require_dir(from, share)?;
    let _dir = Held::exclusive(from, share)?;
    let _seat = Held::seat(&store.dir, share)?;
    let (mut have, mut row_ids) = read_seed(&store.seed_path(), share)?;
    row_ids.extend(already.ids.iter().cloned());
    let mut known_hashes = already.hashes.clone();
    known_hashes.extend(have.keys().cloned());
    let baseline = Known {
        hashes: known_hashes.clone(),
        ids: row_ids.clone(),
    };
    let mut marks = read_marks(store, share)?;
    let paths = export_paths(from, source, share)?;
    let total = paths.len();
    let tty = std::io::stderr().is_terminal();
    let mut seen = BTreeSet::new();
    let mut committed = 0usize;
    let mut finish = |done: &mut usize, name: &str, action: &str| -> Result<()> {
        *done += 1;
        if let Some(line) = export_progress(tty, *done, total, name, action) {
            eprintln!("{line}");
        }
        halt(*done)
    };
    let mut summary = Summary {
        imported: 0,
        unchanged: 0,
        skipped: 0,
        exports_skipped: 0,
        exports_imported: 0,
    };
    for path in &paths {
        let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
            continue;
        };
        let Some(lane) = lane_of(path) else {
            continue;
        };
        let name = name.to_string();
        seen.insert(name.clone());
        let meta = source.meta(path).map_err(|err| share.fail(from, err))?;
        let Some(meta) = meta else {
            continue;
        };
        if let Some(mark) = marks.get(&name) {
            if meta.matches(mark) {
                summary.unchanged += mark.rows;
                summary.exports_skipped += 1;
                finish(&mut committed, &name, "skipped")?;
                continue;
            }
        }
        let bytes = source.read(path).map_err(|err| share.fail(from, err))?;
        let digest = file_sha256(&bytes);
        if let Some(mark) = marks.get(&name).cloned() {
            if mark.sha256 == digest {
                summary.unchanged += mark.rows;
                summary.exports_skipped += 1;
                let rows = mark.rows;
                if remember(
                    &mut marks,
                    &name,
                    ExportMark {
                        size: meta.size,
                        mtime_secs: meta.mtime_secs,
                        mtime_nanos: meta.mtime_nanos,
                        sha256: digest,
                        rows,
                    },
                ) {
                    write_marks(store, &marks, share)?;
                }
                finish(&mut committed, &name, "skipped")?;
                continue;
            }
        }
        let fold = fold_export(
            &bytes,
            &lane,
            &mut have,
            &mut row_ids,
            &mut known_hashes,
            &baseline,
            &mut post,
        )?;
        summary.imported += fold.imported;
        summary.unchanged += fold.unchanged;
        summary.skipped += fold.skipped;
        summary.exports_imported += 1;
        if fold.seed_changed {
            write_seed(store, &have, &row_ids, share)?;
        }
        if remember(
            &mut marks,
            &name,
            ExportMark {
                size: meta.size,
                mtime_secs: meta.mtime_secs,
                mtime_nanos: meta.mtime_nanos,
                sha256: digest,
                rows: fold.rows,
            },
        ) {
            write_marks(store, &marks, share)?;
        }
        finish(
            &mut committed,
            &name,
            &format!("imported {}", fold.imported),
        )?;
    }
    let before = marks.len();
    marks.retain(|name, _| seen.contains(name));
    if marks.len() != before {
        write_marks(store, &marks, share)?;
    }
    let seed_there = match share.symlink_meta(&store.seed_path()) {
        Ok(meta) => meta.is_file() && !meta.file_type().is_symlink(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => return Err(share.fail(&store.dir, err)),
    };
    if !seed_there {
        write_seed(store, &have, &row_ids, share)?;
    }
    Ok(summary)
}

/// Import every lane file in `from` into `store`.
///
/// The summary counts new texts as imported, a text or a lane row id the
/// seat already holds as unchanged, and a repeated text in this pull as
/// skipped. A second pull of the same directory imports nothing and does
/// not read an export whose size and modification time are unchanged.
/// The second line counts those exports against the ones that were read
/// because their bytes were new.
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

    fn lane_body(lane: &str, texts: &[&str]) -> Vec<u8> {
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
        body.into_bytes()
    }

    fn write_lane(dir: &Path, lane: &str, texts: &[&str]) {
        std::fs::write(dir.join(format!("{lane}.jsonl")), lane_body(lane, texts)).unwrap();
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
                skipped: 1,
                exports_skipped: 0,
                exports_imported: 2,
            }
        );
        assert_eq!(
            summary.line(),
            "imported 3, unchanged 0, skipped 1 duplicates\nexports skipped 0, imported 2\n"
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
                skipped: 0,
                exports_skipped: 2,
                exports_imported: 0,
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
                skipped: 0,
                exports_skipped: 0,
                exports_imported: 1,
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
    fn a_deadline_keeps_finished_exports_and_drops_the_one_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        write_lane(&from, "lane-a", &["alpha kiln"]);
        write_lane(&from, "lane-b", &["beta kiln", "gamma kiln"]);
        let calls = AtomicU64::new(0);
        let err = pull_with(&store, &from, &BTreeSet::new(), &BTreeSet::new(), |_| {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            if n >= 1 {
                Err(anyhow::Error::new(packset_client::Error::Unavailable(
                    crate::PACK_DEADLINE_LINE.to_string(),
                )))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(crate::is_deadline_miss(&err), "{err:#}");
        let texts = texts_of(dir.path());
        assert!(texts.contains(&"alpha kiln".to_string()), "{texts:?}");
        assert!(!texts.iter().any(|text| text.contains("beta")), "{texts:?}");
        assert!(
            !texts.iter().any(|text| text.contains("gamma")),
            "{texts:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let mut known = BTreeSet::new();
        known.insert(content_hash("beta kiln"));
        let posts = AtomicU64::new(0);
        let again = pull_with(&store, &from, &known, &BTreeSet::new(), |row| {
            assert_ne!(row.text, "beta kiln");
            posts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert_eq!(posts.load(Ordering::SeqCst), 1, "beta was posted again");
        assert_eq!(again.imported, 1, "{again:?}");
        let texts = texts_of(dir.path());
        for lesson in ["alpha kiln", "beta kiln", "gamma kiln"] {
            assert_eq!(
                texts.iter().filter(|text| text.as_str() == lesson).count(),
                1,
                "{lesson} in {texts:?}"
            );
        }
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

    struct MemFile {
        bytes: Vec<u8>,
        mtime_secs: i64,
        mtime_nanos: u32,
    }

    /// Exports that live in memory. `read` records the file name so a test
    /// can show a later pull did not open the body.
    struct MemExports {
        files: std::sync::Mutex<BTreeMap<String, MemFile>>,
        reads: std::sync::Mutex<Vec<String>>,
    }

    impl MemExports {
        fn new() -> Self {
            Self {
                files: std::sync::Mutex::new(BTreeMap::new()),
                reads: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn insert(&self, name: &str, bytes: Vec<u8>, mtime_secs: i64) {
            self.files.lock().unwrap().insert(
                name.to_string(),
                MemFile {
                    bytes,
                    mtime_secs,
                    mtime_nanos: 0,
                },
            );
        }

        fn set_mtime(&self, name: &str, mtime_secs: i64) {
            self.files.lock().unwrap().get_mut(name).unwrap().mtime_secs = mtime_secs;
        }

        fn set_bytes(&self, name: &str, bytes: Vec<u8>, mtime_secs: i64) {
            let mut files = self.files.lock().unwrap();
            let file = files.get_mut(name).unwrap();
            file.bytes = bytes;
            file.mtime_secs = mtime_secs;
        }

        fn reads(&self) -> Vec<String> {
            self.reads.lock().unwrap().clone()
        }

        fn clear_reads(&self) {
            self.reads.lock().unwrap().clear();
        }
    }

    impl ExportSource for MemExports {
        fn list(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
            let files = self.files.lock().unwrap();
            Ok(files.keys().map(|name| dir.join(name)).collect())
        }

        fn meta(&self, path: &Path) -> std::io::Result<Option<ExportMeta>> {
            let name = path
                .file_name()
                .and_then(|item| item.to_str())
                .unwrap_or("");
            let files = self.files.lock().unwrap();
            let Some(file) = files.get(name) else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "missing export",
                ));
            };
            Ok(Some(ExportMeta {
                size: file.bytes.len() as u64,
                mtime_secs: file.mtime_secs,
                mtime_nanos: file.mtime_nanos,
            }))
        }

        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            let name = path
                .file_name()
                .and_then(|item| item.to_str())
                .unwrap_or("")
                .to_string();
            self.reads.lock().unwrap().push(name.clone());
            let files = self.files.lock().unwrap();
            files
                .get(&name)
                .map(|file| file.bytes.clone())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing export"))
        }
    }

    fn pull_mem(
        store: &SeatStore,
        from: &Path,
        files: &MemExports,
        post: impl FnMut(&Kept) -> Result<()>,
    ) -> Result<Summary> {
        pull_mem_halt(store, from, &BTreeSet::new(), files, post, |_| Ok(()))
    }

    fn pull_mem_halt(
        store: &SeatStore,
        from: &Path,
        already: &BTreeSet<String>,
        files: &MemExports,
        post: impl FnMut(&Kept) -> Result<()>,
        halt: impl FnMut(usize) -> Result<()>,
    ) -> Result<Summary> {
        let share = crate::share_io::Share::mesh_pull(&store.seat, from);
        let known = Known {
            hashes: already.clone(),
            ids: BTreeSet::new(),
        };
        pull_body(store, from, &known, &share, files, post, halt)
    }

    #[test]
    fn a_second_pull_reads_no_unchanged_export() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        let files = MemExports::new();
        files.insert(
            "lane-a.jsonl",
            lane_body("lane-a", &["alpha kiln", "shared kiln"]),
            10,
        );
        files.insert(
            "lane-b.jsonl",
            lane_body("lane-b", &["shared kiln", "beta kiln"]),
            11,
        );
        let first = pull_mem(&store, &from, &files, |_| Ok(())).unwrap();
        assert_eq!(first.exports_imported, 2, "{first:?}");
        assert!(!files.reads().is_empty());
        let seed = std::fs::read(store.seed_path()).unwrap();
        let written = std::fs::metadata(store.seed_path())
            .unwrap()
            .modified()
            .unwrap();
        files.clear_reads();
        let again = pull_mem(&store, &from, &files, |_| Ok(())).unwrap();
        assert!(
            files.reads().is_empty(),
            "unchanged exports were read: {:?}",
            files.reads()
        );
        assert_eq!(
            again,
            Summary {
                imported: 0,
                unchanged: 4,
                skipped: 0,
                exports_skipped: 2,
                exports_imported: 0,
            }
        );
        assert_eq!(std::fs::read(store.seed_path()).unwrap(), seed);
        assert_eq!(
            std::fs::metadata(store.seed_path())
                .unwrap()
                .modified()
                .unwrap(),
            written
        );
    }

    #[test]
    fn a_changed_export_is_read_and_reimported() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        let files = MemExports::new();
        files.insert("lane-a.jsonl", lane_body("lane-a", &["alpha kiln"]), 10);
        files.insert("lane-b.jsonl", lane_body("lane-b", &["beta kiln"]), 11);
        pull_mem(&store, &from, &files, |_| Ok(())).unwrap();
        files.set_bytes(
            "lane-b.jsonl",
            lane_body("lane-b", &["beta kiln", "gamma kiln"]),
            12,
        );
        files.clear_reads();
        let posts = AtomicU64::new(0);
        let again = pull_mem(&store, &from, &files, |row| {
            assert_eq!(row.text, "gamma kiln");
            posts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert_eq!(files.reads(), ["lane-b.jsonl"]);
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        assert_eq!(again.imported, 1, "{again:?}");
        assert_eq!(again.exports_imported, 1, "{again:?}");
        assert_eq!(again.exports_skipped, 1, "{again:?}");
        let texts = texts_of(dir.path());
        assert_eq!(texts.iter().filter(|text| *text == "gamma kiln").count(), 1);
        assert_eq!(texts.iter().filter(|text| *text == "beta kiln").count(), 1);
    }

    #[test]
    fn a_touched_export_with_the_same_bytes_does_not_duplicate_rows() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        let files = MemExports::new();
        files.insert("lane-a.jsonl", lane_body("lane-a", &["alpha kiln"]), 10);
        files.insert("lane-b.jsonl", lane_body("lane-b", &["beta kiln"]), 11);
        pull_mem(&store, &from, &files, |_| Ok(())).unwrap();
        let seed = std::fs::read(store.seed_path()).unwrap();
        files.set_mtime("lane-a.jsonl", 20);
        files.clear_reads();
        let posts = AtomicU64::new(0);
        let again = pull_mem(&store, &from, &files, |_| {
            posts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert_eq!(files.reads(), ["lane-a.jsonl"]);
        assert_eq!(posts.load(Ordering::SeqCst), 0);
        assert_eq!(again.imported, 0, "{again:?}");
        assert_eq!(again.exports_skipped, 2, "{again:?}");
        assert_eq!(again.exports_imported, 0, "{again:?}");
        assert_eq!(std::fs::read(store.seed_path()).unwrap(), seed);
        assert_eq!(texts_of(dir.path()).len(), 2);
        files.clear_reads();
        let third = pull_mem(&store, &from, &files, |_| Ok(())).unwrap();
        assert!(
            files.reads().is_empty(),
            "the new time was not recorded: {:?}",
            files.reads()
        );
        assert_eq!(third.exports_skipped, 2, "{third:?}");
    }

    #[test]
    fn a_pull_stopped_midway_resumes_without_a_half_export() {
        let dir = tempfile::tempdir().unwrap();
        let (from, store) = handoff(dir.path());
        let files = MemExports::new();
        files.insert("lane-a.jsonl", lane_body("lane-a", &["alpha kiln"]), 10);
        files.insert(
            "lane-b.jsonl",
            lane_body("lane-b", &["beta kiln", "gamma kiln"]),
            11,
        );
        let err = pull_mem_halt(
            &store,
            &from,
            &BTreeSet::new(),
            &files,
            |_| Ok(()),
            |done| {
                if done >= 1 {
                    bail!("stopped")
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("stopped"), "{err}");
        let texts = texts_of(dir.path());
        assert_eq!(texts, ["alpha kiln"]);
        assert!(
            !std::fs::read_to_string(marks_path(&store))
                .unwrap()
                .contains("lane-b.jsonl"),
            "the unfinished export was recorded"
        );
        files.clear_reads();
        let again = pull_mem(&store, &from, &files, |_| Ok(())).unwrap();
        assert_eq!(files.reads(), ["lane-b.jsonl"]);
        assert_eq!(again.imported, 2, "{again:?}");
        assert_eq!(again.exports_skipped, 1, "{again:?}");
        let texts = texts_of(dir.path());
        for lesson in ["alpha kiln", "beta kiln", "gamma kiln"] {
            assert_eq!(
                texts.iter().filter(|text| text.as_str() == lesson).count(),
                1,
                "{lesson} in {texts:?}"
            );
        }
    }

    #[test]
    fn progress_is_one_line_on_a_terminal_and_quiet_otherwise() {
        assert_eq!(
            export_progress(false, 1, 7, "lane-a.jsonl", "skipped"),
            None
        );
        assert_eq!(
            export_progress(true, 1, 7, "lane-a.jsonl", "imported 2").as_deref(),
            Some("mesh pull: lane-a.jsonl imported 2 (1/7)")
        );
        let skipped = export_progress(true, 2, 7, "lane-b.jsonl", "skipped").unwrap();
        assert_eq!(skipped, "mesh pull: lane-b.jsonl skipped (2/7)");
        assert!(!skipped.contains('\r'), "{skipped}");
    }
}
