//! Tables cited on a ticket, and the bytes of a stored deed.
//!
//! A table is a CSV file kept in `deedar` as a `file` deed named
//! `table ISSUE NAME vN` and cited on ISSUE. Adding the same name again
//! makes the next version, which supersedes the one before, so `ljos
//! current` on an old accession names the new one. A version is read back
//! through [`crate::deed_bytes::read_deed`], as `ljos deed --show` reads.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::deed_bytes::{deed_name, read_deed, sha256_hex, DeedBytes};

/// The largest CSV `ljos table --add` takes.
pub const MAX_TABLE_BYTES: usize = 8 * 1024 * 1024;

/// A CSV file that passed [`parse_csv`]: the header and the data rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Csv {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Read RFC 4180 CSV: a header row, then rows with as many fields.
///
/// `what` names the file in the refusal. A leading UTF-8 byte order mark is
/// dropped and a last line break is optional.
///
/// # Errors
///
/// The bytes are over [`MAX_TABLE_BYTES`], not UTF-8, empty, a quoted field
/// is not closed, a quote sits inside an unquoted field, a header cell is
/// empty or repeated, or a row has another number of fields than the header.
pub fn parse_csv(what: &str, bytes: &[u8]) -> Result<Csv> {
    if bytes.len() > MAX_TABLE_BYTES {
        bail!(
            "table: {what} is {} bytes, over the {MAX_TABLE_BYTES} byte cap",
            bytes.len()
        );
    }
    let text = std::str::from_utf8(bytes).map_err(|e| {
        anyhow::anyhow!(
            "table: {what} is not UTF-8 (byte {}); save it as UTF-8 CSV",
            e.valid_up_to()
        )
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.trim().is_empty() {
        bail!("table: {what} is empty; a table needs a header row");
    }
    let records = split_records(what, text)?;
    let mut records = records.into_iter();
    let Some((_, header)) = records.next() else {
        bail!("table: {what} is empty; a table needs a header row");
    };
    for (at, cell) in header.iter().enumerate() {
        if cell.trim().is_empty() {
            bail!("table: {what} header column {} is empty", at + 1);
        }
        if header[..at].iter().any(|seen| seen == cell) {
            bail!("table: {what} header names {cell:?} twice");
        }
    }
    let mut rows = Vec::new();
    for (line, row) in records {
        if row.len() != header.len() {
            bail!(
                "table: {what} line {line} has {} columns; the header has {}",
                row.len(),
                header.len()
            );
        }
        rows.push(row);
    }
    Ok(Csv { header, rows })
}

/// The records of `text`, each with the line it starts on.
fn split_records(what: &str, text: &str) -> Result<Vec<(usize, Vec<String>)>> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut line = 1usize;
    let mut start = 1usize;
    let mut quoted = false;
    let mut was_quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            '"' => bail!("table: {what} line {line} has a quote inside an unquoted field"),
            ',' => {
                record.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push((start, std::mem::take(&mut record)));
                was_quoted = false;
                line += 1;
                start = line;
            }
            _ if was_quoted => {
                bail!("table: {what} line {line} has text after a closing quote")
            }
            _ => field.push(c),
        }
    }
    if quoted {
        bail!("table: {what} line {start} opens a quoted field that is never closed");
    }
    if !field.is_empty() || !record.is_empty() || was_quoted {
        record.push(field);
        records.push((start, record));
    }
    Ok(records)
}

/// A table name: a letter or digit, then letters, digits, `.`, `_` or `-`.
///
/// # Errors
///
/// The name is empty, longer than 64 characters, or has another character.
pub fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !ok {
        bail!(
            "table: {name:?} is not a table name; use letters, digits, `.`, `_` and `-`, at most 64, so pass `--name NAME`"
        );
    }
    Ok(())
}

/// One version of a table cited on a ticket.
#[derive(Debug, Clone)]
pub struct Version {
    pub name: String,
    pub version: u32,
    pub accession: String,
}

fn deed_label(issue: &str, name: &str, version: u32) -> String {
    format!("table {issue} {name} v{version}")
}

/// `(name, version)` out of a deed named `table ISSUE NAME vN`.
fn parse_label(issue: &str, label: &str) -> Option<(String, u32)> {
    let rest = label
        .strip_prefix("table ")?
        .strip_prefix(issue)?
        .strip_prefix(' ')?;
    let (name, version) = rest.rsplit_once(" v")?;
    let version = version.parse().ok()?;
    check_name(name).ok()?;
    Some((name.to_string(), version))
}

/// Every table version `issue` cites, by name then version.
///
/// # Errors
///
/// The tracker does not have `issue`.
pub fn versions(issue: &str) -> Result<Vec<Version>> {
    let card = crate::tracker_show_json(issue)?;
    let mut out = Vec::new();
    for cited in card["deeds"].as_array().into_iter().flatten() {
        let Some(id) = cited.as_str().map(str::trim).filter(|id| !id.is_empty()) else {
            continue;
        };
        let Ok(got) = crate::run_captured("deedar", &["get", id]) else {
            continue;
        };
        if let Some((name, version)) = parse_label(issue, &deed_name(&got.stdout)) {
            out.push(Version {
                name,
                version,
                accession: id.to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
    Ok(out)
}

/// What `ljos table ISSUE --add FILE` did.
pub struct Added {
    pub accession: String,
    pub text: String,
    /// The logbook verb for the tracker commit.
    pub verb: String,
    /// False when the latest version already has these bytes.
    pub wrote: bool,
}

/// Validate `file`, store it in `deedar` and cite it on `issue`.
///
/// The tracker commit is the caller's, so it is one commit.
///
/// # Errors
///
/// The CSV is refused, the name is not a table name, the tracker does not
/// have `issue`, or `deedar` or `vissue` refuse the write.
pub fn add(issue: &str, file: &Path, name: Option<&str>) -> Result<Added> {
    let what = file.display().to_string();
    let bytes = std::fs::read(file).with_context(|| format!("table: could not read {what}"))?;
    let csv = parse_csv(&what, &bytes)?;
    let name = match name {
        Some(name) => name.trim().to_string(),
        None => file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    check_name(&name)?;
    let have = versions(issue)?;
    let latest = have
        .iter()
        .filter(|v| v.name == name)
        .max_by_key(|v| v.version);
    let digest = sha256_hex(&bytes);
    if let Some(latest) = latest {
        if read_deed(&latest.accession).is_ok_and(|old| old.sha256 == digest) {
            return Ok(Added {
                accession: latest.accession.clone(),
                text: format!(
                    "{issue}: table {name} v{} already has these bytes ({}); nothing written\n",
                    latest.version, latest.accession
                ),
                verb: String::new(),
                wrote: false,
            });
        }
    }
    let version = latest.map_or(1, |v| v.version + 1);
    let label = deed_label(issue, &name, version);
    let path = file.display().to_string();
    let seat = crate::seat_name();
    let mut argv = vec![
        "create", "file", "--name", &label, "--path", &path, "--media", "text/csv", "--agent",
        &seat,
    ];
    if let Some(latest) = latest {
        argv.push("--supersedes");
        argv.push(&latest.accession);
    }
    let made = crate::run_captured("deedar", &argv).context("table: deedar did not store it")?;
    let accession = made
        .stdout
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("accession="))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .context("table: deedar create printed no accession")?
        .to_string();
    let stored = crate::deed_bytes::field(&made.stdout, "paths").unwrap_or("");
    if stored.trim() != format!("sha256:{digest}") {
        bail!("table: {what} changed while it was stored ({stored}); add it again");
    }
    crate::run_captured("vissue", &["deed", issue, "--add", &accession])
        .with_context(|| format!("table: could not cite {accession} on {issue}"))?;
    Ok(Added {
        text: format!(
            "{issue}: table {name} v{version}, {} rows, {} columns, {accession}\n",
            csv.rows.len(),
            csv.header.len()
        ),
        verb: format!("added table {name} v{version}"),
        accession,
        wrote: true,
    })
}

/// The latest version of each table on `issue`, with its size.
///
/// # Errors
///
/// The tracker does not have `issue`.
pub fn list(issue: &str, json: bool) -> Result<String> {
    let all = versions(issue)?;
    let mut latest: Vec<&Version> = Vec::new();
    for v in &all {
        match latest.last_mut() {
            Some(last) if last.name == v.name => *last = v,
            _ => latest.push(v),
        }
    }
    let mut rows = Vec::new();
    for v in latest {
        let size = read_deed(&v.accession).and_then(|d| {
            parse_csv(&v.accession, &d.bytes).map(|c| (c.rows.len(), c.header.len()))
        });
        rows.push((v, size));
    }
    if json {
        let out: Vec<serde_json::Value> = rows
            .iter()
            .map(|(v, size)| {
                let mut row = serde_json::json!({
                    "name": v.name,
                    "version": v.version,
                    "accession": v.accession,
                });
                match size {
                    Ok((r, c)) => {
                        row["rows"] = (*r).into();
                        row["columns"] = (*c).into();
                    }
                    Err(e) => row["error"] = format!("{e:#}").into(),
                }
                row
            })
            .collect();
        return Ok(format!("{}\n", serde_json::Value::Array(out)));
    }
    if rows.is_empty() {
        return Ok(format!(
            "{issue}: no tables; `ljos table {issue} --add FILE.csv` adds one\n"
        ));
    }
    let width = rows
        .iter()
        .map(|(v, _)| v.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let mut out = format!(
        "{issue}: {} table{}\n{:<width$}  {:>6}  {:>7}  {:>7}  ACCESSION\n",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        "NAME",
        "ROWS",
        "COLUMNS",
        "VERSION"
    );
    let mut errors = String::new();
    for (v, size) in &rows {
        let (r, c) = match size {
            Ok((r, c)) => (r.to_string(), c.to_string()),
            Err(e) => {
                let _ = writeln!(errors, "{e:#}");
                ("?".into(), "?".into())
            }
        };
        let _ = writeln!(
            out,
            "{:<width$}  {r:>6}  {c:>7}  {:>7}  {}",
            v.name, v.version, v.accession
        );
    }
    out.push_str(&errors);
    Ok(out)
}

/// One version of a table: the latest, or `version`.
///
/// # Errors
///
/// The ticket has no table of that name or version, or its bytes fail
/// [`read_deed`].
pub fn show(issue: &str, name: &str, version: Option<u32>) -> Result<(Version, DeedBytes)> {
    let all = versions(issue)?;
    let named: Vec<&Version> = all.iter().filter(|v| v.name == name).collect();
    if named.is_empty() {
        bail!("table: {issue} has no table {name:?}; `ljos table {issue}` lists them");
    }
    let picked = match version {
        Some(n) => named
            .iter()
            .find(|v| v.version == n)
            .copied()
            .with_context(|| {
                format!(
                    "table: {issue} has no version {n} of {name}; it has v1 to v{}",
                    named.last().map_or(0, |v| v.version)
                )
            })?,
        None => named[named.len() - 1],
    };
    let bytes = read_deed(&picked.accession)?;
    Ok((picked.clone(), bytes))
}

/// A table as JSON: its name, version, accession, header and rows.
///
/// # Errors
///
/// The bytes are not a CSV [`parse_csv`] takes.
pub fn as_json(issue: &str, v: &Version, bytes: &[u8]) -> Result<String> {
    let csv = parse_csv(&v.accession, bytes)?;
    let out = serde_json::json!({
        "issue": issue,
        "name": v.name,
        "version": v.version,
        "accession": v.accession,
        "columns": csv.header,
        "rows": csv.rows,
    });
    Ok(format!("{out}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_fields_hold_commas_quotes_and_line_breaks() {
        let csv = parse_csv("t.csv", b"a,b\n\"x, y\",\"say \"\"hi\"\"\nthere\"\r\n").unwrap();
        assert_eq!(csv.header, ["a", "b"]);
        assert_eq!(
            csv.rows,
            [vec!["x, y".to_string(), "say \"hi\"\nthere".into()]]
        );
    }

    #[test]
    fn a_ragged_row_names_its_line_and_both_counts() {
        let e = parse_csv("t.csv", b"a,b\n1,2\n3\n")
            .unwrap_err()
            .to_string();
        assert_eq!(e, "table: t.csv line 3 has 1 columns; the header has 2");
    }

    #[test]
    fn a_label_reads_back_its_name_and_version() {
        assert_eq!(
            parse_label("p-1", &deed_label("p-1", "scores", 3)),
            Some(("scores".into(), 3))
        );
        assert_eq!(parse_label("p-2", &deed_label("p-1", "scores", 3)), None);
    }
}
