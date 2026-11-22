//! `ljos file --from FILE`: many tickets in one tracker commit.
//!
//! Every row is read and checked before anything is written. The writes go
//! through `vissue_core::ops::create`, as a plan's checklist does. Each
//! row goes where a single `ljos file` would put it. Every project
//! directory the batch writes is copied first, and a row the tracker
//! refuses puts them back, so the tracker is left as it was. The commit names the first
//! id and how many more.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// The most rows one `ljos file --from` takes.
pub const MAX_ROWS: usize = 10_000;

const COLUMNS: &[&str] = &["title", "body", "tags", "parent", "priority", "type"];

/// One ticket to file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub title: String,
    pub body: Option<String>,
    pub tags: Option<String>,
    pub parent: Option<String>,
    pub priority: Option<char>,
    pub kind: Option<String>,
}

/// Values a row leaves out: the `ljos file` flags given with `--from`.
#[derive(Debug, Clone, Default)]
pub struct Defaults {
    pub project: Option<String>,
    pub parent: Option<String>,
    pub kind: Option<String>,
    pub tags: Option<String>,
    pub priority: Option<String>,
}

fn unknown_column(line: usize, name: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "file --from: row {line}: unknown column {name:?}; the columns are {}",
        COLUMNS.join(", ")
    )
}

fn priority(line: usize, raw: &str) -> Result<Option<char>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    match raw {
        "A" | "B" | "C" | "a" | "b" | "c" => Ok(raw.chars().next().map(|c| c.to_ascii_uppercase())),
        other => bail!("file --from: row {line}: priority {other:?} is not A, B or C"),
    }
}

fn set(row: &mut Row, line: usize, column: &str, value: &str) -> Result<()> {
    let some = || {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    };
    match column {
        "title" => row.title = value.trim().to_string(),
        "body" => row.body = (!value.trim().is_empty()).then(|| value.trim_end().to_string()),
        "tags" => row.tags = some(),
        "parent" => row.parent = some(),
        "priority" => row.priority = priority(line, value)?,
        "type" => row.kind = some(),
        other => return Err(unknown_column(line, other)),
    }
    Ok(())
}

fn check_title(line: usize, row: &Row) -> Result<()> {
    if row.title.is_empty() {
        bail!("file --from: row {line}: the title is empty");
    }
    if row.title.contains(['\n', '\r']) {
        bail!("file --from: row {line}: the title has a line break");
    }
    Ok(())
}

/// Rows out of JSON Lines: one object per line, blank lines skipped. `tags`
/// may be a string or a list of strings.
///
/// # Errors
///
/// A line is not a JSON object, names an unknown column, or a row fails
/// its check.
pub fn parse_jsonl(text: &str) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for (at, raw) in text.lines().enumerate() {
        let line = at + 1;
        if raw.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| anyhow::anyhow!("file --from: row {line} is not JSON: {e}"))?;
        let Some(object) = value.as_object() else {
            bail!("file --from: row {line} is not a JSON object");
        };
        let mut row = Row::default();
        for (key, value) in object {
            let text = match value {
                serde_json::Value::Null => String::new(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(items) if key == "tags" => items
                    .iter()
                    .map(|item| {
                        item.as_str().map(str::to_string).ok_or_else(|| {
                            anyhow::anyhow!("file --from: row {line}: a tag is not a string")
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
                    .join(","),
                other => bail!("file --from: row {line}: {key} is {other}, not a string"),
            };
            set(&mut row, line, key, &text)?;
        }
        check_title(line, &row)?;
        rows.push(row);
    }
    Ok(rows)
}

/// Rows out of CSV with a header that names the columns.
///
/// # Errors
///
/// The CSV is refused, names an unknown column or no `title`, or a row
/// fails its check.
pub fn parse_csv_rows(what: &str, bytes: &[u8]) -> Result<Vec<Row>> {
    let csv = crate::table::parse_csv(what, bytes).map_err(|e| {
        anyhow::anyhow!(
            "file --from: {}",
            e.to_string().trim_start_matches("table: ")
        )
    })?;
    for (at, name) in csv.header.iter().enumerate() {
        if !COLUMNS.contains(&name.trim()) {
            return Err(unknown_column(1, name));
        }
        if let Some(first) = csv.header[..at]
            .iter()
            .position(|h| h.trim() == name.trim())
        {
            bail!(
                "file --from: {what}: header columns {} ({:?}) and {} ({name:?}) are both {}",
                first + 1,
                csv.header[first],
                at + 1,
                name.trim()
            );
        }
    }
    if !csv.header.iter().any(|h| h.trim() == "title") {
        bail!("file --from: {what} has no title column");
    }
    let mut rows = Vec::new();
    for (at, cells) in csv.rows.iter().enumerate() {
        let line = at + 2;
        let mut row = Row::default();
        for (name, cell) in csv.header.iter().zip(cells) {
            set(&mut row, line, name.trim(), cell)?;
        }
        check_title(line, &row)?;
        rows.push(row);
    }
    Ok(rows)
}

/// The rows of `path`: JSON Lines for `.jsonl` or `.ndjson`, CSV for
/// `.csv`, and otherwise JSON Lines when the first character is `{`.
/// `-` reads stdin.
///
/// # Errors
///
/// The file cannot be read, has no rows or more than [`MAX_ROWS`], or a
/// row is refused.
pub fn read_rows(path: &Path) -> Result<Vec<Row>> {
    let what = path.display().to_string();
    let bytes = if what == "-" {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf)
            .context("file --from: stdin")?;
        buf
    } else {
        std::fs::read(path).with_context(|| format!("file --from: could not read {what}"))?
    };
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let jsonl = match ext.as_str() {
        "jsonl" | "ndjson" => true,
        "csv" => false,
        _ => bytes
            .iter()
            .find(|b| !b.is_ascii_whitespace())
            .is_some_and(|b| *b == b'{'),
    };
    let rows = if jsonl {
        let text = std::str::from_utf8(&bytes).map_err(|e| {
            anyhow::anyhow!(
                "file --from: {what} is not UTF-8 (byte {})",
                e.valid_up_to()
            )
        })?;
        parse_jsonl(text)?
    } else {
        parse_csv_rows(&what, &bytes)?
    };
    if rows.is_empty() {
        bail!("file --from: {what} has no rows");
    }
    if rows.len() > MAX_ROWS {
        bail!(
            "file --from: {what} has {} rows, over the {MAX_ROWS} row cap",
            rows.len()
        );
    }
    Ok(rows)
}

/// A lock file is never removed or rewritten. A writer may hold it or wait
/// on it, and a new file under its name lets a second writer in beside the
/// first.
fn is_lock(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "lock")
}

/// Files by path, with their bytes.
type Files = BTreeMap<PathBuf, Vec<u8>>;

/// The files of the project directory `dir` that a create writes, with
/// their bytes: `issues.org` and each regular file in `issues/` but the
/// lock files. Nothing else there is read.
fn snapshot(dir: &Path) -> Result<Files> {
    let mut out = BTreeMap::new();
    let board = dir.join("issues.org");
    if std::fs::symlink_metadata(&board).is_ok_and(|meta| meta.is_file()) {
        out.insert(
            board.clone(),
            std::fs::read(&board).with_context(|| format!("reading {}", board.display()))?,
        );
    }
    let ledger = dir.join("issues");
    if !ledger.is_dir() {
        return Ok(out);
    }
    for entry in
        std::fs::read_dir(&ledger).with_context(|| format!("reading {}", ledger.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() && !is_lock(&entry.path()) {
            out.insert(entry.path(), std::fs::read(entry.path())?);
        }
    }
    Ok(out)
}

/// Put `dir` back to `before`: files written since are restored, files
/// made since are removed. Lock files are left as they are. The caller
/// holds the project's issues lock.
fn restore(dir: &Path, before: &Files) -> Result<()> {
    for (path, _) in snapshot(dir)? {
        if !before.contains_key(&path) {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    for (path, bytes) in before {
        if std::fs::read(path).ok().as_deref() != Some(bytes.as_slice()) {
            vissue_core::store::replace_file_atomically(path, &String::from_utf8_lossy(bytes))
                .with_context(|| format!("restoring {}", path.display()))?;
        }
    }
    Ok(())
}

/// A row with the project and parent it is filed under.
#[derive(Debug, Clone)]
pub struct Placed {
    pub row: Row,
    pub project: String,
    pub parent: Option<String>,
}

/// Where each row goes, by the rules a single `ljos file` follows: a row
/// with a parent, its own or `--parent`, is filed in that parent's project,
/// whatever `-p` names; one without is a child of `held`, the held issue,
/// unless `-p` or the checkout names another project,
/// and is otherwise filed in `-p`, else the checkout's project, else
/// `LJOS_PROJECT`. `--top` keeps a row with no parent at the top of its
/// project. The second value is each distinct line
/// [`crate::file_placement`] printed about a placement.
///
/// # Errors
///
/// A row has no project, or a project name is refused.
pub fn place_rows(
    rows: &[Row],
    defaults: &Defaults,
    top: bool,
    held: Option<String>,
) -> Result<(Vec<Placed>, Vec<String>)> {
    let no_hold = held.is_none();
    let mut placed = Vec::with_capacity(rows.len());
    let mut notes: Vec<String> = Vec::new();
    for (at, row) in rows.iter().enumerate() {
        let line = at + 1;
        let parent = row.parent.clone().or_else(|| defaults.parent.clone());
        let named_parent = parent.is_some();
        let top_row = top && !named_parent;
        let (parent, project, note) =
            crate::file_placement(top_row, parent, defaults.project.clone(), held.clone());
        if project.is_none() && parent.is_none() {
            if let Some(note) = note.as_deref().filter(|n| n.starts_with("project: ")) {
                bail!("file --from: row {line}: {}", note.trim_end());
            }
        }
        let project = project.or_else(|| {
            if !no_hold || top_row || named_parent || defaults.project.is_some() {
                return None;
            }
            std::env::var("LJOS_PROJECT")
                .ok()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        });
        let project = match project {
            Some(project) => project,
            None if top_row => crate::seat_tracker_project_no_cwd().with_context(|| {
                format!("file --from: row {line}: no project, so pass `-p NAME`")
            })?,
            None => bail!(
                "file --from: row {line}: no project, so pass `-p NAME` or give the row a parent"
            ),
        };
        if let Some(note) = note {
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
        placed.push(Placed {
            row: row.clone(),
            project,
            parent,
        });
    }
    Ok((placed, notes))
}

/// What `ljos file --from` filed.
pub struct Filed {
    pub ids: Vec<String>,
    /// Each project a row went to, in the order first used.
    pub projects: Vec<String>,
    pub persisted: String,
}

/// One project directory the batch writes, as it was and as this batch
/// last left it.
struct Board {
    project: String,
    issues: PathBuf,
    dir: PathBuf,
    before: Files,
    ours: Files,
    /// The board in `ours`, rendered as `vissue` writes it.
    board: String,
}

/// Run `f` under the issues lock of `issues`.
fn locked<R>(issues: &Path, f: impl FnOnce() -> Result<R>) -> Result<R> {
    crate::with_wall_bounded_issues_lock(issues, || f().map_err(vissue_core::Error::from))
        .map_err(anyhow::Error::from)
}

fn parsed(project: &str, issues: &Path) -> Result<vissue_core::IssueDoc> {
    vissue_core::IssueDoc::parse_file(project, issues)
        .with_context(|| format!("reading {}", issues.display()))
}

/// The board at `issues`, a single file or a ledger, rendered with its
/// cookies filled.
fn rendered(project: &str, issues: &Path) -> Result<String> {
    let mut doc = parsed(project, issues)?;
    doc.refresh_statistics();
    Ok(doc.render_string())
}

/// The whole of the file `vissue_core::ledger` writes for a new issue whose
/// heading renders as `heading`, under a ledger whose preamble is
/// `preamble`: the preamble and a blank line, `#+VISSUE_LEDGER:`, the lines
/// the heading takes in that file, the heading, and an empty log.
fn ledger_header(preamble: &str, heading: &str) -> String {
    let preamble = preamble.trim_end();
    let mut out = String::new();
    let mut start = 3;
    if !preamble.is_empty() {
        out.push_str(preamble);
        out.push_str("\n\n");
        start += preamble.lines().count() + 1;
    }
    let end = start + heading.lines().count().max(1) - 1;
    out.push_str(&format!(
        "#+VISSUE_LEDGER:\n#+VISSUE_LINES: {start} {end}\n{heading}"
    ));
    if !heading.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("#+VISSUE_LEDGER_LOG:\n");
    out
}

/// One row as it was handed to `vissue_core::ops::create`, with the id it
/// came back with.
struct Want<'a> {
    id: &'a str,
    title: &'a str,
    priority: Option<char>,
    kind: Option<&'a str>,
    tags: Option<&'a str>,
    parent: Option<&'a str>,
    body: Option<&'a str>,
    /// The tracker's configured default priority.
    cfg_priority: char,
    /// The ids on the tracker just before the create; with no body, none.
    known: HashSet<String>,
    /// Today as an inactive Org date, read before and after the create.
    days: [String; 2],
}

/// The first `[[id:ID]]` or `[[id:ID][label]]` in `body` whose id is in
/// `known`, read as `vissue_core::ops::create` reads it for
/// `DISCOVERED_FROM`.
fn first_existing_id_link(body: &str, known: &HashSet<String>) -> Option<String> {
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after_start = &rest[start + 2..];
        let end = after_start.find("]]")?;
        let raw = &after_start[..end];
        let target = raw.split_once("][").map_or(raw, |(target, _)| target);
        if let Some(id) = target.trim().strip_prefix("id:") {
            let id = id.trim();
            if known.contains(id) {
                return Some(id.to_string());
            }
        }
        rest = &after_start[end + 2..];
    }
    None
}

impl Want<'_> {
    /// The heading `ops::create` writes for this row at `priority`.
    /// `CREATED` is the day it ran, so it is read from `found`; `None` when
    /// `found` has no `CREATED` or it is not one of `days`.
    fn heading(
        &self,
        found: &vissue_core::IssueHeading,
        priority: char,
    ) -> Option<vissue_core::IssueHeading> {
        use vissue_core::{org::is_org_tag_char, props};
        let mut properties = BTreeMap::new();
        properties.insert("ID".to_string(), self.id.to_string());
        let created = found.properties.get("CREATED")?;
        if !self.days.contains(created) {
            return None;
        }
        properties.insert("CREATED".to_string(), created.clone());
        if let Some(from) = self
            .body
            .and_then(|body| first_existing_id_link(body, &self.known))
        {
            props::insert(&mut properties, props::DISCOVERED_FROM, from);
        }
        let mut org_tags: Vec<String> = Vec::new();
        if let Some(kind) = self.kind {
            props::insert(&mut properties, props::TYPE, kind.to_string());
            if !kind.is_empty() && kind.chars().all(is_org_tag_char) {
                org_tags.push(kind.to_string());
            }
        }
        if let Some(tags) = self.tags {
            let mut property_tags: Vec<String> = Vec::new();
            for tag in tags.split([',', ':']).map(str::trim) {
                if tag.is_empty() {
                    continue;
                }
                let list = if tag.chars().all(is_org_tag_char) {
                    &mut org_tags
                } else {
                    &mut property_tags
                };
                if !list.iter().any(|seen| seen == tag) {
                    list.push(tag.to_string());
                }
            }
            if !property_tags.is_empty() {
                properties.insert(
                    vissue_core::model::TAGS_PROPERTY.to_string(),
                    property_tags.join(","),
                );
            }
        }
        if let Some(parent) = self.parent {
            props::insert(&mut properties, props::PARENT, parent.to_string());
        }
        Some(vissue_core::IssueHeading {
            id: self.id.to_string(),
            title: self.title.to_string(),
            state: "TODO".into(),
            priority,
            properties,
            org_tags,
            statistics: None,
            property_order: Vec::new(),
            extra_drawers: Vec::new(),
            body: match self.body {
                Some(body) if !body.trim().is_empty() => format!("{}\n", body.trim_end()),
                _ => String::new(),
            },
            logbook: Vec::new(),
            line_start: 0,
            line_end: 0,
        })
    }
}

impl Board {
    /// After `want` was filed here, and under the issues lock: the directory
    /// and board if that create is the only change since `ours`, else
    /// `None`. A create changes one file: `issues.org`, rewritten as vissue
    /// renders it, or on a ledger a new file for the id that is byte for
    /// byte the header vissue writes for it. The new heading is the one this
    /// row asked for, with nothing
    /// on it, and the board without it is the board the batch last left.
    /// Anything else may be another writer's, and a roll back would lose it.
    fn only_filed(&self, want: &Want) -> Result<Option<(Files, String)>> {
        let now = snapshot(&self.dir)?;
        let mut changed = now
            .iter()
            .filter(|(path, bytes)| self.ours.get(*path) != Some(*bytes))
            .map(|(path, _)| path)
            .chain(self.ours.keys().filter(|path| !now.contains_key(*path)));
        let (Some(changed), None) = (changed.next(), changed.next()) else {
            return Ok(None);
        };
        let Some(text) = now.get(changed).map(|bytes| String::from_utf8_lossy(bytes)) else {
            return Ok(None);
        };
        let mut doc = parsed(&self.project, &self.issues)?;
        let Some(found) = doc.headings.iter().find(|h| h.id == want.id) else {
            return Ok(None);
        };
        let house_new = !self.ours.contains_key(&self.issues);
        let priority = want
            .priority
            .unwrap_or(if doc.priorities_are_named() || house_new {
                doc.priority_spec().default
            } else {
                want.cfg_priority
            });
        let Some(heading) = want.heading(found, priority).map(|h| h.render()) else {
            return Ok(None);
        };
        let clean = if vissue_core::ledger::is_ledger(&self.issues) {
            changed == doc.heading_path(want.id)
                && !self.ours.contains_key(changed)
                && text == ledger_header(&doc.preamble, &heading)
        } else {
            let mut whole = doc.clone();
            whole.refresh_statistics();
            *changed == self.issues && whole.render_string() == text
        };
        if !clean || heading != found.render() {
            return Ok(None);
        }
        doc.refresh_statistics();
        let board = doc.render_string();
        doc.remove(want.id);
        doc.refresh_statistics();
        if doc.render_string() != self.board {
            return Ok(None);
        }
        Ok(Some((now, board)))
    }
}

/// Put every board back as it was, holding all their issues locks from
/// the check to the last write. `Some(dir)` names a directory another
/// writer changed since this batch last wrote it; then nothing is put
/// back.
fn roll_back(boards: &[Board]) -> Result<Option<PathBuf>> {
    let paths: Vec<&Path> = boards.iter().map(|b| b.issues.as_path()).collect();
    let under_lock = || -> Result<Option<PathBuf>> {
        for b in boards {
            if snapshot(&b.dir)? != b.ours {
                return Ok(Some(b.dir.clone()));
            }
        }
        for b in boards {
            restore(&b.dir, &b.before)?;
        }
        Ok(None)
    };
    vissue_core::store::with_issues_locks(&paths, || under_lock().map_err(vissue_core::Error::from))
        .map_err(anyhow::Error::from)
}

/// File every placed row and commit the tracker once.
///
/// # Errors
///
/// A project is not in the tracker or is projected from another tracker,
/// or the tracker refuses a row. A refused row leaves the tracker as it
/// was, unless another writer changed a project file meanwhile.
pub fn file_rows(placed: &[Placed], defaults: &Defaults) -> Result<Filed> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    layout
        .require_tracker_to_create()
        .context("file --from: no tracker to file in, so run `ljos onboard`")?;
    let default_priority = match defaults.priority.as_deref() {
        Some(p) => priority(0, p)
            .map_err(|_| anyhow::anyhow!("file --from: --priority {p:?} is not A, B or C"))?,
        None => None,
    };
    let cfg_priority = vissue_core::VissueConfig::load(&layout)?
        .issues
        .default_priority;
    let projected = vissue_core::projection::boards(layout.root()).unwrap_or_default();
    let mut boards: Vec<Board> = Vec::new();
    let mut at_board = Vec::with_capacity(placed.len());
    for (at, place) in placed.iter().enumerate() {
        let line = at + 1;
        let named = crate::project_name(&place.project)
            .map_err(|e| anyhow::anyhow!("file --from: row {line}: {e}"))?;
        let project = vissue_core::store::resolve_existing_project_case(&layout, &named)
            .with_context(|| {
                format!("file --from: row {line}: no project {named:?} in the tracker")
            })?;
        if let Some(i) = boards.iter().position(|b| b.project == project) {
            at_board.push(i);
            continue;
        }
        if projected
            .iter()
            .any(|b| b.source != "self" && b.project.eq_ignore_ascii_case(&project))
        {
            bail!("file --from: row {line}: {project} is projected from another tracker; file its tickets with `ljos file` one at a time");
        }
        let issues = layout.project_issues_path(&project);
        let dir = issues
            .parent()
            .map(Path::to_path_buf)
            .context("file --from: the project file has no directory")?;
        let (before, board) = locked(&issues, || {
            Ok((snapshot(&dir)?, rendered(&project, &issues)?))
        })?;
        at_board.push(boards.len());
        boards.push(Board {
            project,
            issues,
            dir,
            ours: before.clone(),
            before,
            board,
        });
    }
    let mut ids: Vec<String> = Vec::with_capacity(placed.len());
    for (at, place) in placed.iter().enumerate() {
        let line = at + 1;
        let board = at_board[at];
        let row = &place.row;
        let kind = row.kind.clone().or_else(|| defaults.kind.clone());
        let tags = row.tags.clone().or_else(|| defaults.tags.clone());
        let known = if row.body.is_some() {
            vissue_core::store::collect_org_ids(&layout)?
        } else {
            HashSet::new()
        };
        let first_day = vissue_core::model::today_inactive_bracket();
        let made = vissue_core::ops::create(
            &layout,
            &boards[board].project,
            &row.title,
            vissue_core::ops::CreateOpts {
                priority: row.priority.or(default_priority),
                issue_type: kind.as_deref(),
                tags: tags.as_deref(),
                parent: place.parent.as_deref(),
                body: row.body.as_deref(),
                quiet: true,
                ..vissue_core::ops::CreateOpts::default()
            },
        );
        let days = [first_day, vissue_core::model::today_inactive_bracket()];
        match made {
            Ok(text) => {
                let id = text.trim().to_string();
                ids.push(id.clone());
                // Another writer that changes a project directory mid-batch
                // ends it, and its change is never put back over.
                let b = &boards[board];
                let want = Want {
                    id: &id,
                    title: &row.title,
                    priority: row.priority.or(default_priority),
                    kind: kind.as_deref(),
                    tags: tags.as_deref(),
                    parent: place.parent.as_deref(),
                    body: row.body.as_deref(),
                    cfg_priority,
                    known,
                    days,
                };
                let Some((ours, filed)) = locked(&b.issues, || b.only_filed(&want))? else {
                    bail!(
                        "file --from: another writer changed {} while row {line} was filed, so the batch stopped; the {} tickets filed are on the tracker and not committed: {}",
                        b.dir.display(),
                        ids.len(),
                        ids.join(" ")
                    );
                };
                boards[board].ours = ours;
                boards[board].board = filed;
            }
            Err(e) => {
                let undone = match roll_back(&boards) {
                    Ok(Some(dir)) => bail!(
                        "file --from: row {line}: {e}; another writer changed {} meanwhile, so the {} tickets filed are on the tracker and not committed: {}",
                        dir.display(),
                        ids.len(),
                        ids.join(" ")
                    ),
                    Ok(None) => format!(
                        "nothing was filed: the {} tickets written before it were taken back",
                        ids.len()
                    ),
                    Err(r) => format!("putting the tracker back failed: {r:#}"),
                };
                for b in &boards {
                    // A poller may have read the rows written so far;
                    // the log is append-only, so the undo is one more write.
                    let _ = vissue_core::events::emit_issues_write(
                        &vissue_core::events::events_dir(&layout),
                        &b.project,
                        &b.issues,
                    );
                }
                bail!("file --from: row {line}: {e}; {undone}");
            }
        }
    }
    let persisted = crate::persist_tracker_many(&ids, "filed");
    Ok(Filed {
        ids,
        projects: boards.into_iter().map(|b| b.project).collect(),
        persisted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_takes_a_tag_list_and_refuses_an_unknown_column() {
        let rows =
            parse_jsonl("{\"title\":\"a\",\"tags\":[\"x\",\"y\"],\"priority\":\"b\"}\n\n").unwrap();
        assert_eq!(rows[0].tags.as_deref(), Some("x,y"));
        assert_eq!(rows[0].priority, Some('B'));
        let e = parse_jsonl("{\"title\":\"a\",\"owner\":\"z\"}")
            .unwrap_err()
            .to_string();
        assert!(e.contains("row 1: unknown column \"owner\""), "{e}");
    }

    #[test]
    fn a_header_named_twice_once_with_spaces_is_refused() {
        let e = parse_csv_rows("s.csv", b"title,title \nkept,dropped\n")
            .unwrap_err()
            .to_string();
        assert_eq!(
            e,
            "file --from: s.csv: header columns 1 (\"title\") and 2 (\"title \") are both title"
        );
        let e = parse_csv_rows("s.csv", b"title, body,body\na,b,c\n")
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("header columns 2 (\" body\") and 3 (\"body\")"),
            "{e}"
        );
    }

    #[test]
    fn a_roll_back_keeps_the_lock_file_and_never_puts_back_over_another_writer() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("demo");
        std::fs::create_dir_all(&dir).unwrap();
        let issues = dir.join("issues.org");
        let read = || std::fs::read_to_string(&issues).unwrap();
        std::fs::write(&issues, "before\n").unwrap();
        let before = snapshot(&dir).unwrap();
        std::fs::write(&issues, "ours\n").unwrap();
        std::fs::write(dir.join("issues.org.lock"), "").unwrap();
        let board = Board {
            project: "demo".into(),
            issues: issues.clone(),
            dir: dir.clone(),
            before,
            ours: snapshot(&dir).unwrap(),
            board: String::new(),
        };
        std::fs::write(&issues, "theirs\n").unwrap();
        let boards = std::slice::from_ref(&board);
        assert_eq!(roll_back(boards).unwrap(), Some(dir.clone()));
        assert_eq!(read(), "theirs\n");
        std::fs::write(&issues, "ours\n").unwrap();
        assert_eq!(roll_back(boards).unwrap(), None);
        assert_eq!(read(), "before\n");
        assert!(dir.join("issues.org.lock").is_file());
    }

    #[test]
    fn a_snapshot_reads_only_the_files_a_create_writes() {
        let (_root, _layout, board) = demo(true);
        std::fs::write(board.dir.join("notes.txt"), "not the board\n").unwrap();
        std::fs::create_dir_all(board.dir.join("issues/deeper")).unwrap();
        std::fs::write(board.dir.join("issues/deeper/x.org"), "deeper\n").unwrap();
        let mut names: Vec<String> = snapshot(&board.dir)
            .unwrap()
            .keys()
            .map(|path| path.strip_prefix(&board.dir).unwrap().display().to_string())
            .collect();
        names.retain(|name| !name.starts_with("issues/demo-"));
        assert_eq!(names, ["issues/.ledger"]);
    }

    /// A tracker with project `demo` holding one issue, and the batch's
    /// board for it as `file_rows` first reads it.
    fn demo(ledger: bool) -> (tempfile::TempDir, vissue_core::Layout, Board) {
        let root = tempfile::tempdir().unwrap();
        let layout = vissue_core::Layout::new(root.path(), "tracker");
        let issues = layout.project_issues_path("demo");
        let dir = issues.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(dir.join("issues")).unwrap();
        if ledger {
            std::fs::write(dir.join("issues/.ledger"), "").unwrap();
        }
        file(&layout, "old");
        let before = snapshot(&dir).unwrap();
        let board = Board {
            project: "demo".into(),
            board: rendered("demo", &issues).unwrap(),
            issues,
            dir,
            ours: before.clone(),
            before,
        };
        (root, layout, board)
    }

    const BODY: &str = "why, see [[id:nowhere]]";
    const TAGS: &str = "x,needs-review";

    fn file(layout: &vissue_core::Layout, title: &str) -> String {
        let made = vissue_core::ops::create(
            layout,
            "demo",
            title,
            vissue_core::ops::CreateOpts {
                issue_type: Some("task"),
                tags: Some(TAGS),
                body: Some(BODY),
                quiet: true,
                ..vissue_core::ops::CreateOpts::default()
            },
        );
        made.unwrap().trim().to_string()
    }

    fn want<'a>(layout: &vissue_core::Layout, id: &'a str) -> Want<'a> {
        Want {
            id,
            title: "new",
            priority: None,
            kind: Some("task"),
            tags: Some(TAGS),
            parent: None,
            body: Some(BODY),
            cfg_priority: vissue_core::VissueConfig::load(layout)
                .unwrap()
                .issues
                .default_priority,
            known: vissue_core::store::collect_org_ids(layout).unwrap(),
            days: [
                chrono::Local::now() - chrono::Duration::minutes(1),
                chrono::Local::now(),
            ]
            .map(|at| at.format("[%Y-%m-%d %a]").to_string()),
        }
    }

    /// What `file_rows` does with a filed row and then a refused one:
    /// `None` when the filed row stopped the batch, else what `roll_back`
    /// said.
    fn refuse_next(board: &mut Board, want: &Want) -> Option<Option<PathBuf>> {
        let (ours, filed) = board.only_filed(want).unwrap()?;
        board.ours = ours;
        board.board = filed;
        Some(roll_back(std::slice::from_ref(board)).unwrap())
    }

    #[test]
    fn a_batch_takes_back_a_create_that_is_its_own() {
        for ledger in [false, true] {
            let (_root, layout, mut board) = demo(ledger);
            let id = file(&layout, "new");
            assert_eq!(refuse_next(&mut board, &want(&layout, &id)), Some(None));
            assert_eq!(
                snapshot(&board.dir).unwrap(),
                board.before,
                "ledger {ledger}"
            );
        }
    }

    #[test]
    fn a_batch_never_takes_back_a_file_another_writer_made_beside_the_board() {
        for ledger in [false, true] {
            let (_root, layout, mut board) = demo(ledger);
            let id = file(&layout, "new");
            let side = board.dir.join("issues/side.txt");
            std::fs::write(&side, "other writer\n").unwrap();
            assert_eq!(refuse_next(&mut board, &want(&layout, &id)), None);
            assert_eq!(std::fs::read_to_string(&side).unwrap(), "other writer\n");
            let doc = vissue_core::IssueDoc::parse_file("demo", &board.issues).unwrap();
            assert!(doc.known_ids().contains(&id), "ledger {ledger}");
        }
    }

    #[test]
    fn a_batch_never_takes_back_a_logbook_line_another_writer_put_on_the_new_heading() {
        let (_root, layout, mut board) = demo(false);
        let id = file(&layout, "new");
        let text = std::fs::read_to_string(&board.issues).unwrap();
        let at = text.find(id.as_str()).unwrap();
        let end = at + text[at..].find(":END:\n").unwrap() + ":END:\n".len();
        let line = "- Noted by other [2026-11-21 Sat 12:00]";
        let theirs = format!("{}:LOGBOOK:\n{line}\n:END:\n{}", &text[..end], &text[end..]);
        std::fs::write(&board.issues, &theirs).unwrap();
        assert_eq!(refuse_next(&mut board, &want(&layout, &id)), None);
        assert_eq!(std::fs::read_to_string(&board.issues).unwrap(), theirs);
    }

    #[test]
    fn a_batch_never_takes_back_a_ledger_record_another_writer_appended() {
        let (_root, layout, mut board) = demo(true);
        let id = file(&layout, "new");
        let doc = vissue_core::IssueDoc::parse_file("demo", &board.issues).unwrap();
        let path = doc.heading_path(&id).to_path_buf();
        let mut theirs = std::fs::read_to_string(&path).unwrap();
        theirs.push_str("%% later-op 3\nabc\n");
        std::fs::write(&path, &theirs).unwrap();
        assert_eq!(refuse_next(&mut board, &want(&layout, &id)), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), theirs);
    }

    #[test]
    fn a_batch_never_takes_back_a_ledger_file_another_writer_added_to_above_its_log() {
        let (_root, layout, board) = demo(true);
        let id = file(&layout, "new");
        let doc = vissue_core::IssueDoc::parse_file("demo", &board.issues).unwrap();
        let path = doc.heading_path(&id).to_path_buf();
        let text = std::fs::read_to_string(&path).unwrap();
        let theirs = text.replace(
            "\n#+VISSUE_LEDGER_LOG:\n",
            "\n* TODO Theirs\n:PROPERTIES:\n:ID:         demo-zzzz\n:END:\n#+VISSUE_LEDGER_LOG:\n",
        );
        assert_ne!(theirs, text);
        std::fs::write(&path, &theirs).unwrap();
        assert!(board.only_filed(&want(&layout, &id)).unwrap().is_none());
        std::fs::write(&path, format!("# theirs\n{text}")).unwrap();
        assert!(board.only_filed(&want(&layout, &id)).unwrap().is_none());
        std::fs::write(&path, &text).unwrap();
        assert!(board.only_filed(&want(&layout, &id)).unwrap().is_some());
    }

    #[test]
    fn a_batch_stops_on_a_new_heading_that_is_not_the_row() {
        let (_root, layout, board) = demo(false);
        let id = file(&layout, "new");
        let mut other = want(&layout, &id);
        other.title = "renamed";
        assert!(board.only_filed(&other).unwrap().is_none());
        other = want(&layout, &id);
        other.priority = Some('A');
        assert!(board.only_filed(&other).unwrap().is_none());
        assert!(board.only_filed(&want(&layout, &id)).unwrap().is_some());
    }

    #[test]
    fn a_link_with_a_space_after_id_is_the_row_s_own_discovered_from() {
        let (_root, layout, board) = demo(false);
        let old = vissue_core::IssueDoc::parse_file("demo", &board.issues).unwrap();
        let body = format!("found in [[id: {} ]]", old.headings[0].id);
        let made = vissue_core::ops::create(
            &layout,
            "demo",
            "new",
            vissue_core::ops::CreateOpts {
                issue_type: Some("task"),
                tags: Some(TAGS),
                body: Some(&body),
                quiet: true,
                ..vissue_core::ops::CreateOpts::default()
            },
        );
        let id = made.unwrap().trim().to_string();
        let mut row = want(&layout, &id);
        row.body = Some(&body);
        assert!(board.only_filed(&row).unwrap().is_some());
    }

    /// Set `key` on the heading `id` as another writer would, and write the
    /// board back as vissue renders it.
    fn forge(board: &Board, id: &str, key: &str, value: &str) {
        let mut doc = vissue_core::IssueDoc::parse_file("demo", &board.issues).unwrap();
        let at = doc.headings.iter().position(|h| h.id == id).unwrap();
        doc.headings[at]
            .properties
            .insert(key.to_string(), value.to_string());
        doc.write().unwrap();
    }

    #[test]
    fn a_forged_created_or_discovered_from_is_not_the_row_s_create() {
        let (_root, layout, board) = demo(false);
        let id = file(&layout, "new");
        forge(&board, &id, "CREATED", "[2001-01-01 Mon]");
        assert!(board.only_filed(&want(&layout, &id)).unwrap().is_none());

        let (_root, layout, board) = demo(false);
        let id = file(&layout, "new");
        forge(&board, &id, "DISCOVERED_FROM", "nowhere");
        assert!(board.only_filed(&want(&layout, &id)).unwrap().is_none());
    }

    #[test]
    fn csv_rows_name_the_line_of_a_bad_priority() {
        let e = parse_csv_rows("s.csv", b"title,priority\na,A\nb,Z\n")
            .unwrap_err()
            .to_string();
        assert_eq!(e, "file --from: row 3: priority \"Z\" is not A, B or C");
    }
}
