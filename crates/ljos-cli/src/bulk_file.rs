//! `ljos file --from FILE`: many tickets in one tracker commit.
//!
//! Every row is read and checked before anything is written. The writes go
//! through `vissue_core::ops::create`, as a plan's checklist does. Each
//! row goes where a single `ljos file` would put it. Every project
//! directory the batch writes is copied first, and a row the tracker
//! refuses puts them back, so the tracker is left as it was. The commit names the first
//! id and how many more.

use std::collections::BTreeMap;
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

/// Every regular file under `dir` but the lock files, with its bytes.
fn snapshot(dir: &Path) -> Result<Files> {
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).with_context(|| format!("reading {}", at.display()))? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() && !is_lock(&entry.path()) {
                out.insert(entry.path(), std::fs::read(entry.path())?);
            }
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

/// The board at `issues`, a single file or a ledger, rendered with its
/// cookies filled and without the heading `drop`. `None` when `drop` is
/// not on it.
fn rendered(project: &str, issues: &Path, drop: Option<&str>) -> Result<Option<String>> {
    let mut doc = vissue_core::IssueDoc::parse_file(project, issues)
        .with_context(|| format!("reading {}", issues.display()))?;
    if let Some(id) = drop {
        if doc.remove(id).is_none() {
            return Ok(None);
        }
    }
    doc.refresh_statistics();
    Ok(Some(doc.render_string()))
}

impl Board {
    /// After `id` was filed here, and under the issues lock: the directory
    /// and board if `id` is the only change since `ours`, else `None`. A
    /// create writes `issues.org` or the ledger beside it and nothing else
    /// in the directory.
    fn only_filed(&self, id: &str) -> Result<Option<(Files, String)>> {
        let now = snapshot(&self.dir)?;
        let ledger = self.dir.join("issues");
        let board_file = |path: &Path| path == self.issues || path.starts_with(&ledger);
        let elsewhere = self
            .ours
            .iter()
            .any(|(path, bytes)| !board_file(path) && now.get(path) != Some(bytes))
            || now
                .keys()
                .any(|path| !board_file(path) && !self.ours.contains_key(path));
        if elsewhere
            || rendered(&self.project, &self.issues, Some(id))?.as_deref()
                != Some(self.board.as_str())
        {
            return Ok(None);
        }
        let board = rendered(&self.project, &self.issues, None)?.unwrap_or_default();
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
            Ok((
                snapshot(&dir)?,
                rendered(&project, &issues, None)?.unwrap_or_default(),
            ))
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
        match made {
            Ok(text) => {
                let id = text.trim().to_string();
                ids.push(id.clone());
                // Another writer that changes a project directory mid-batch
                // ends it, and its change is never put back over.
                let b = &boards[board];
                let Some((ours, filed)) = locked(&b.issues, || b.only_filed(&id))? else {
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
    fn csv_rows_name_the_line_of_a_bad_priority() {
        let e = parse_csv_rows("s.csv", b"title,priority\na,A\nb,Z\n")
            .unwrap_err()
            .to_string();
        assert_eq!(e, "file --from: row 3: priority \"Z\" is not A, B or C");
    }
}
