//! `ljos file --from FILE`: many tickets in one tracker commit.
//!
//! Every row is read and checked before anything is written. The writes go
//! through `vissue_core::ops::create`, as a plan's checklist does. The
//! project's directory is copied first, and a row the tracker refuses puts
//! it back, so the tracker is left as it was. The commit names the first
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
    for name in &csv.header {
        if !COLUMNS.contains(&name.trim()) {
            return Err(unknown_column(1, name));
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

/// Every regular file under `dir`, with its bytes.
fn snapshot(dir: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
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
            } else if kind.is_file() {
                out.insert(entry.path(), std::fs::read(entry.path())?);
            }
        }
    }
    Ok(out)
}

/// Put `dir` back to `before`: files written since are restored, files
/// made since are removed.
fn restore(dir: &Path, before: &BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
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

/// What `ljos file --from` filed.
pub struct Filed {
    pub ids: Vec<String>,
    pub project: String,
    pub persisted: String,
}

/// File `rows` into one project and commit the tracker once.
///
/// The project is `--project`, else the project of the first parent named,
/// else `LJOS_PROJECT`.
///
/// # Errors
///
/// No project is known, the project is projected from another tracker, or
/// the tracker refuses a row. A refused row leaves the tracker as it was.
pub fn file_rows(rows: &[Row], defaults: &Defaults) -> Result<Filed> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    layout
        .require_tracker_to_create()
        .context("file --from: no tracker to file in, so run `ljos onboard`")?;
    let default_priority = match defaults.priority.as_deref() {
        Some(p) => priority(0, p)
            .map_err(|_| anyhow::anyhow!("file --from: --priority {p:?} is not A, B or C"))?,
        None => None,
    };
    let first_parent = rows
        .iter()
        .find_map(|r| r.parent.clone())
        .or_else(|| defaults.parent.clone());
    let named = match (&defaults.project, &first_parent) {
        (Some(project), _) => project.clone(),
        (None, Some(parent)) => {
            let hit = vissue_core::Router::load(layout.clone())?
                .find_by_id(parent)
                .map_err(|_| crate::unknown_ticket(parent))?;
            hit.project
        }
        (None, None) => std::env::var("LJOS_PROJECT")
            .ok()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .context("file --from: no project, so pass `-p NAME` or a parent column")?,
    };
    let project = crate::project_name(&named).map_err(anyhow::Error::msg)?;
    let project = vissue_core::store::resolve_existing_project_case(&layout, &project)
        .with_context(|| format!("file --from: no project {project:?} in the tracker"))?;
    if vissue_core::projection::boards(layout.root())
        .unwrap_or_default()
        .iter()
        .any(|b| b.source != "self" && b.project.eq_ignore_ascii_case(&project))
    {
        bail!("file --from: {project} is projected from another tracker; file its tickets with `ljos file` one at a time");
    }
    let issues = layout.project_issues_path(&project);
    let dir = issues
        .parent()
        .map(Path::to_path_buf)
        .context("file --from: the project file has no directory")?;
    let before = snapshot(&dir)?;
    let read = || std::fs::read(&issues).unwrap_or_default();
    // The bytes after this batch's last write. Another writer that changes
    // the file mid-batch ends it, and its row is never put back over.
    let mut ours = read();
    let mut ids = Vec::with_capacity(rows.len());
    for (at, row) in rows.iter().enumerate() {
        let line = at + 1;
        if read() != ours {
            bail!(
                "file --from: another writer changed {} before row {line}, so the batch stopped; the {} tickets filed are on the tracker and not committed: {}",
                issues.display(),
                ids.len(),
                ids.join(" ")
            );
        }
        let parent = row.parent.clone().or_else(|| defaults.parent.clone());
        let kind = row.kind.clone().or_else(|| defaults.kind.clone());
        let tags = row.tags.clone().or_else(|| defaults.tags.clone());
        let made = vissue_core::ops::create(
            &layout,
            &project,
            &row.title,
            vissue_core::ops::CreateOpts {
                priority: row.priority.or(default_priority),
                issue_type: kind.as_deref(),
                tags: tags.as_deref(),
                parent: parent.as_deref(),
                body: row.body.as_deref(),
                quiet: true,
                ..vissue_core::ops::CreateOpts::default()
            },
        );
        match made {
            Ok(text) => {
                ids.push(text.trim().to_string());
                ours = read();
            }
            Err(e) => {
                if read() != ours {
                    bail!(
                        "file --from: row {line}: {e}; another writer changed {} meanwhile, so the {} tickets filed are on the tracker and not committed: {}",
                        issues.display(),
                        ids.len(),
                        ids.join(" ")
                    );
                }
                let undone = match restore(&dir, &before) {
                    Ok(()) => {
                        // A poller may have read the rows written so far;
                        // the log is append-only, so the undo is one more write.
                        let _ = vissue_core::events::emit_issues_write(
                            &vissue_core::events::events_dir(&layout),
                            &project,
                            &issues,
                        );
                        format!(
                            "nothing was filed: the {} tickets written before it were taken back",
                            ids.len()
                        )
                    }
                    Err(r) => format!("putting {} back failed: {r:#}", dir.display()),
                };
                bail!("file --from: row {line}: {e}; {undone}");
            }
        }
    }
    let persisted = crate::persist_tracker_many(&ids, "filed");
    Ok(Filed {
        ids,
        project,
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
    fn csv_rows_name_the_line_of_a_bad_priority() {
        let e = parse_csv_rows("s.csv", b"title,priority\na,A\nb,Z\n")
            .unwrap_err()
            .to_string();
        assert_eq!(e, "file --from: row 3: priority \"Z\" is not A, B or C");
    }
}
