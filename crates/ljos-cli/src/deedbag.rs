//! A cloud seat's finish writes the deeds it cited into the tracker.
//!
//! The box imports that bag after the machine is gone. `deedar export
//! --into` writes the deed frame and the inclusion receipt. It has no flag
//! for the write-once file bytes or the supersedes sidecar, and a bag
//! without them is not enough to import. Finish copies both into the deed's
//! own folder after export. A sibling directory would be another deed to
//! `deedar check`, which requires `proof.txt` in every folder.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// What finish prints, and the checkout paths the tracker commit should
/// take. A machine `ljos cloud up` did not mark returns an empty line and
/// no paths, so a box finish writes nothing.
pub(crate) fn on_finish(issue: &str) -> (String, Vec<String>) {
    if !crate::cloud::cloud_seat() {
        return (String::new(), Vec::new());
    }
    match export_cited(issue) {
        Ok(done) => done,
        Err(e) => (format!("deeds: {e:#}\n"), Vec::new()),
    }
}

fn export_cited(issue: &str) -> Result<(String, Vec<String>)> {
    let seat = seat_dir_name()?;
    let cited = cited_ids(issue)?;
    if cited.is_empty() {
        return Ok((
            format!("deeds: none cited by {seat} on {issue}\n"),
            Vec::new(),
        ));
    }
    which::which("deedar").context("deedar is not on PATH")?;
    let rows = rows_for(&cited)?;
    if rows.is_empty() {
        return Ok((
            format!("deeds: none cited by {seat} on {issue}\n"),
            Vec::new(),
        ));
    }
    let root = tracker_root(issue)?;
    let scratch = Scratch::create(issue)?;
    export_into(scratch.path(), &rows)?;
    seam(&store_dir()?, scratch.path(), &rows)?;
    let bag = scratch.path().display().to_string();
    crate::run_captured("deedar", &["check", bag.as_str()]).context("deedar check")?;
    let dest = root.join(".ljos").join("deeds").join(&seat);
    publish(scratch.path(), &dest)?;
    let n = rows.len();
    Ok((
        format!("deeds: exported {n} deeds cited by {seat} into .ljos/deeds/{seat}\n"),
        vec![format!(".ljos/deeds/{seat}")],
    ))
}

struct Row {
    id: String,
    hexes: Vec<String>,
}

fn rows_for(cited: &[String]) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for id in cited {
        one_component(id)?;
        let Some(text) = deed_text(id)? else {
            continue;
        };
        let Some(produced) = field(&text, "producedBy") else {
            continue;
        };
        let producer = produced.split_whitespace().next().unwrap_or("");
        if !by_this_seat(producer) {
            continue;
        }
        rows.push(Row {
            id: id.clone(),
            hexes: hexes_of(field(&text, "paths").unwrap_or(""))?,
        });
    }
    Ok(rows)
}

fn cited_ids(issue: &str) -> Result<Vec<String>> {
    let card = crate::tracker_show_json(issue)?;
    let mut ids = Vec::new();
    for row in card["deeds"].as_array().into_iter().flatten() {
        let Some(id) = row.as_str().map(str::trim).filter(|id| !id.is_empty()) else {
            continue;
        };
        if !ids.iter().any(|have| have == id) {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

/// `deedar get` prints the cited deed. A missing deed is skipped. Anything
/// else, including a store that will not open, stops the export.
fn deed_text(id: &str) -> Result<Option<String>> {
    match crate::run_captured("deedar", &["get", id]) {
        Ok(said) => Ok(Some(said.stdout)),
        Err(e) => {
            let text = format!("{e:#}");
            if text.contains("deed not found") || text.contains("deed tombstoned") {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    text.lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
}

fn hexes_of(paths: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if paths.is_empty() {
        return Ok(out);
    }
    for part in paths.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some(hex) = part.strip_prefix("sha256:") else {
            bail!("deed path {part} is not a sha256 digest");
        };
        if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("deed path {part} is not a sha256 digest");
        }
        out.push(hex.to_string());
    }
    Ok(out)
}

/// The producer `deedar get` prints, matched to this seat. The tracker
/// records the accession and not who cited it, so the deed's producer is
/// the record that it was this seat's.
fn by_this_seat(producer: &str) -> bool {
    let producer = producer.trim();
    if producer.is_empty() {
        return false;
    }
    let seat = crate::seat_name();
    let holder = crate::holder_name();
    let slug = crate::tracker_seat();
    if producer == seat || producer == holder || producer == slug {
        return true;
    }
    let head = producer.split(['@', ':']).next().unwrap_or("");
    let head_slug = crate::seat_slug(head);
    head_slug == slug || head_slug == crate::seat_slug(&seat)
}

fn export_into(dir: &Path, rows: &[Row]) -> Result<()> {
    let into = dir.display().to_string();
    let mut args = vec!["export".to_string(), "--into".into(), into];
    for row in rows {
        args.push(row.id.clone());
    }
    crate::run_captured("deedar", &args).context("deedar export")?;
    Ok(())
}

fn seam(store: &Path, bag: &Path, rows: &[Row]) -> Result<()> {
    for row in rows {
        let folder = bag.join(&row.id);
        if !folder.is_dir() {
            bail!("export wrote no folder for {}", row.id);
        }
        for hex in &row.hexes {
            let src = store.join("bytes").join(hex);
            if !src.is_file() {
                bail!("file bytes sha256:{hex} are not in the store");
            }
            std::fs::copy(&src, folder.join(hex))
                .with_context(|| format!("copying sha256:{hex}"))?;
        }
        copy_link(store, &row.id, "supersedes", &folder)?;
        copy_link(store, &row.id, "successor", &folder)?;
    }
    Ok(())
}

fn copy_link(store: &Path, id: &str, name: &str, folder: &Path) -> Result<()> {
    let src = store.join("deeds").join(format!("{id}.{name}"));
    if !src.is_file() {
        return Ok(());
    }
    std::fs::copy(&src, folder.join(name)).with_context(|| format!("copying {id}.{name}"))?;
    Ok(())
}

fn store_dir() -> Result<PathBuf> {
    let raw = std::env::var("DEEDAR_URL").unwrap_or_default();
    let raw = raw.trim();
    if !raw.is_empty() {
        let path = PathBuf::from(raw.strip_prefix("file://").unwrap_or(raw));
        if !path.is_absolute() {
            bail!("DEEDAR_URL {raw} is not a file store");
        }
        return Ok(path);
    }
    Ok(crate::data_base()
        .context("no data home for the deed store")?
        .join("deedar")
        .join("store"))
}

fn tracker_root(issue: &str) -> Result<PathBuf> {
    let path = crate::tracker_issue_path(issue).map_err(|e| anyhow::anyhow!("{e}"))?;
    let parent = path.parent().context("the issue file has no directory")?;
    let inside = crate::git_ok_stdout(parent, &["rev-parse", "--is-inside-work-tree"]);
    if inside.as_deref().map(str::trim) != Some("true") {
        bail!("the tracker is not a git checkout");
    }
    Ok(crate::tracker_push::repo_root(parent))
}

fn seat_dir_name() -> Result<String> {
    let seat = crate::tracker_seat();
    one_component(&seat).context("the seat name is not a folder name")?;
    Ok(seat)
}

fn one_component(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        bail!("{name} is not a folder name");
    }
    Ok(())
}

fn publish(scratch: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    if dest.exists() {
        std::fs::remove_dir_all(dest)
            .with_context(|| format!("could not replace {}", dest.display()))?;
    }
    // The scratch dir may sit on another filesystem than the tracker.
    if std::fs::rename(scratch, dest).is_ok() {
        return Ok(());
    }
    if let Err(e) = copy_tree(scratch, dest) {
        let _ = std::fs::remove_dir_all(dest);
        return Err(e).with_context(|| format!("could not place {}", dest.display()));
    }
    let _ = std::fs::remove_dir_all(scratch);
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// Removed on drop when export fails before the bag is published. A rename
/// that already moved it leaves nothing to remove.
struct Scratch(PathBuf);

impl Scratch {
    fn create(issue: &str) -> Result<Self> {
        let dir = crate::runtime_dir().join(format!(
            "deed-export-{}-{}",
            std::process::id(),
            crate::seat_slug(issue)
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("could not clear {}", dir.display()))?;
        }
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
