//! A cloud seat's finish writes the deeds it cited into the tracker.
//!
//! The box imports that bag after the machine is gone. `deedar export
//! --into` writes the deed frame and the inclusion receipt. Import also
//! requires the product bytes under `bytes/` and, when a deed supersedes
//! another or was made from one, a sidecar that names that deed and its
//! digest. Finish writes those beside the export. A sibling directory
//! would be another deed to `deedar check`, which requires `proof.txt`
//! in every folder.

use std::collections::{BTreeMap, HashMap};
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
    sources: String,
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
            sources: field(&text, "sources").unwrap_or("").to_string(),
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
        // Import accepts only a content address: 64 lowercase hex digits.
        // Anything shorter, longer or upper case is a different name.
        if !is_sha256_hex(hex) {
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
    let mut digests = HashMap::new();
    for row in rows {
        let folder = bag.join(&row.id);
        if !folder.is_dir() {
            bail!("export wrote no folder for {}", row.id);
        }
        for hex in &row.hexes {
            copy_payload(store, hex, &folder)?;
        }
        write_link(store, &row.id, "supersedes", &folder, &mut digests)?;
        write_link(store, &row.id, "successor", &folder, &mut digests)?;
        write_sources(&row.sources, &folder, &mut digests)?;
    }
    Ok(())
}

/// Product bytes live at `<deed>/bytes/<hex>`. The importer hashes the file
/// and requires the name to be that digest. `is_file` follows a symlink, so
/// the kind comes from `symlink_metadata` and a link is refused.
fn copy_payload(store: &Path, hex: &str, folder: &Path) -> Result<()> {
    let src = store.join("bytes").join(hex);
    match file_kind(&src)? {
        Kind::File => {}
        Kind::Symlink => bail!("file bytes sha256:{hex} are a symlink"),
        Kind::Missing | Kind::Other => {
            bail!("file bytes sha256:{hex} are not in the store");
        }
    }
    let dir = folder.join("bytes");
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    std::fs::copy(&src, dir.join(hex)).with_context(|| format!("copying sha256:{hex}"))?;
    Ok(())
}

/// A link sidecar is the deed id and the digest `deedar log prove` records
/// for it. The store file names the id only, which import will not take.
fn write_link(
    store: &Path,
    id: &str,
    name: &str,
    folder: &Path,
    digests: &mut HashMap<String, String>,
) -> Result<()> {
    let src = store.join("deeds").join(format!("{id}.{name}"));
    match file_kind(&src)? {
        Kind::Missing | Kind::Other => return Ok(()),
        Kind::Symlink => bail!("{id}.{name} is a symlink"),
        Kind::File => {}
    }
    let raw = std::fs::read_to_string(&src).with_context(|| format!("reading {id}.{name}"))?;
    let linked = raw.trim();
    if linked.is_empty() {
        return Ok(());
    }
    one_component(linked).with_context(|| format!("{id}.{name} is not a deed id"))?;
    let digest = deed_digest(digests, linked)?;
    let body = format!("{linked}\n{digest}\n");
    std::fs::write(folder.join(name), body).with_context(|| format!("writing {id} {name}"))?;
    Ok(())
}

/// `sources=` lines from `deedar get`: `deed:<id>`, one digest a line.
/// Path and url sources are not deeds, so they have no sidecar line.
fn write_sources(
    sources: &str,
    folder: &Path,
    digests: &mut HashMap<String, String>,
) -> Result<()> {
    let mut lines = BTreeMap::new();
    for part in sources.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some(id) = part.strip_prefix("deed:") else {
            continue;
        };
        one_component(id).with_context(|| format!("source {part} is not a deed id"))?;
        let digest = deed_digest(digests, id)?;
        lines.insert(id.to_string(), digest);
    }
    if lines.is_empty() {
        return Ok(());
    }
    let mut body = String::new();
    for (id, digest) in lines {
        body.push_str(&id);
        body.push('\t');
        body.push_str(&digest);
        body.push('\n');
    }
    std::fs::write(folder.join("sources"), body).context("writing sources")?;
    Ok(())
}

fn deed_digest(cache: &mut HashMap<String, String>, id: &str) -> Result<String> {
    if let Some(have) = cache.get(id) {
        return Ok(have.clone());
    }
    let said = crate::run_captured("deedar", &["log", "prove", id])
        .with_context(|| format!("deedar log prove {id}"))?;
    let digest = said
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("digest="))
        .map(str::trim)
        .filter(|text| is_sha256_addr(text))
        .with_context(|| format!("deedar log prove {id} did not report a sha256 digest"))?
        .to_string();
    cache.insert(id.to_string(), digest.clone());
    Ok(digest)
}

fn is_sha256_addr(text: &str) -> bool {
    let Some(hex) = text.strip_prefix("sha256:") else {
        return false;
    };
    is_sha256_hex(hex)
}

fn is_sha256_hex(hex: &str) -> bool {
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

enum Kind {
    Missing,
    Symlink,
    Other,
    File,
}

fn file_kind(path: &Path) -> Result<Kind> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            let kind = meta.file_type();
            if kind.is_symlink() {
                Ok(Kind::Symlink)
            } else if kind.is_file() {
                Ok(Kind::File)
            } else {
                Ok(Kind::Other)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Kind::Missing),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
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

/// Put `scratch` at `dest`. The old bag stays until a finished tree has been
/// staged beside it: a copy that fails, including one across filesystems,
/// leaves `dest` as it was.
fn publish(scratch: &Path, dest: &Path) -> Result<()> {
    let parent = dest.parent().context("the deed bag has no directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("could not create {}", parent.display()))?;
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .context("the deed bag has no name")?;
    let staged = parent.join(format!(".{name}.incoming"));
    let backup = parent.join(format!(".{name}.previous"));
    clear_dir(&staged)?;
    // A crash after dest moved aside and before the new tree took its place
    // leaves the published bag at `backup`. Put it back before touching it.
    if !dest.exists() && backup.exists() {
        std::fs::rename(&backup, dest)
            .with_context(|| format!("could not restore {}", dest.display()))?;
    }
    clear_dir(&backup)?;

    stage_into(scratch, &staged)?;

    let replacing = dest.exists();
    if replacing {
        if let Err(e) = std::fs::rename(dest, &backup) {
            let _ = std::fs::remove_dir_all(&staged);
            return Err(e).with_context(|| format!("could not move {} aside", dest.display()));
        }
    }
    if let Err(e) = std::fs::rename(&staged, dest) {
        if replacing {
            let _ = std::fs::rename(&backup, dest);
        }
        let _ = std::fs::remove_dir_all(&staged);
        return Err(e).with_context(|| format!("could not place {}", dest.display()));
    }
    if replacing {
        let _ = std::fs::remove_dir_all(&backup);
    }
    let _ = std::fs::remove_dir_all(scratch);
    Ok(())
}

fn clear_dir(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("could not clear {}", path.display()))?;
    }
    Ok(())
}

fn stage_into(scratch: &Path, staged: &Path) -> Result<()> {
    // Same filesystem: the rename is the stage. Across filesystems the
    // copy has to finish before dest is touched, or a failure would
    // publish nothing.
    if std::fs::rename(scratch, staged).is_ok() {
        return Ok(());
    }
    match copy_tree(scratch, staged) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_dir_all(staged);
            Err(e).context("could not stage the deed bag")
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("{} is a symlink", entry.path().display());
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &dest)?;
        } else {
            bail!("{} is not a regular file", entry.path().display());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uppercase_and_short_hex_are_refused() {
        let good = "ab".repeat(32);
        assert_eq!(
            hexes_of(&format!("sha256:{good}")).unwrap(),
            vec![good.clone()]
        );
        let upper = hexes_of(&format!("sha256:{}", "AB".repeat(32)));
        assert!(upper.is_err(), "uppercase hex was accepted");
        let short = hexes_of("sha256:abcd");
        assert!(short.is_err(), "short hex was accepted");
        let long = hexes_of(&format!("sha256:{good}aa"));
        assert!(long.is_err(), "a 66 digit hex was accepted");
    }

    #[test]
    fn a_symlinked_store_entry_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let bag = dir.path().join("bag");
        let id = "deed-file-note";
        let hex = "cd".repeat(32);
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, b"secret outside the store").unwrap();
        std::fs::create_dir_all(store.join("bytes")).unwrap();
        std::os::unix::fs::symlink(&outside, store.join("bytes").join(&hex)).unwrap();
        std::fs::create_dir_all(bag.join(id)).unwrap();
        let err = seam(
            &store,
            &bag,
            &[Row {
                id: id.to_string(),
                hexes: vec![hex.clone()],
                sources: String::new(),
            }],
        )
        .expect_err("a symlinked store entry was copied");
        let text = format!("{err:#}");
        assert!(text.contains("symlink"), "{text}");
        assert!(
            std::fs::symlink_metadata(bag.join(id).join("bytes").join(&hex)).is_err(),
            "{text}"
        );
        assert!(
            std::fs::symlink_metadata(bag.join(id).join(&hex)).is_err(),
            "{text}"
        );
        assert!(
            !bag_holds(&bag, b"secret outside the store"),
            "symlink target was copied into the bag"
        );
    }

    #[test]
    fn a_failed_publish_keeps_the_old_bag() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("seat");
        std::fs::create_dir_all(dest.join("kept")).unwrap();
        std::fs::write(dest.join("kept").join("marker"), b"old bag").unwrap();
        let here = std::fs::metadata(dir.path()).unwrap().dev();
        let shm = std::fs::metadata("/dev/shm")
            .unwrap_or_else(|e| panic!("/dev/shm: {e}"))
            .dev();
        assert_ne!(
            here, shm,
            "/dev/shm is not a separate filesystem, so the copy path cannot fail on its own"
        );
        // SAFETY: geteuid only reads this process's credentials.
        assert_ne!(
            unsafe { libc::geteuid() },
            0,
            "root can read a mode 000 file, so the copy would not fail"
        );
        let scratch = tempfile::tempdir_in("/dev/shm").unwrap();
        std::fs::create_dir_all(scratch.path().join("fresh")).unwrap();
        let blocked = scratch.path().join("fresh").join("blocked");
        std::fs::write(&blocked, b"new").unwrap();
        let mut perms = std::fs::metadata(&blocked).unwrap().permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&blocked, perms).unwrap();
        let err = publish(scratch.path(), &dest);
        let mut perms = std::fs::metadata(&blocked).unwrap().permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&blocked, perms).unwrap();
        let err = err.expect_err("publish of an unreadable tree succeeded");
        let text = format!("{err:#}");
        assert_eq!(
            std::fs::read(dest.join("kept").join("marker")).unwrap(),
            b"old bag",
            "{text}"
        );
        assert!(!dest.join("fresh").exists(), "{text}");
        assert!(
            !dir.path().join(".seat.incoming").exists(),
            "staged tree left behind: {text}"
        );
        assert!(
            !dir.path().join(".seat.previous").exists(),
            "old bag was moved aside: {text}"
        );
    }

    fn bag_holds(dir: &Path, secret: &[u8]) -> bool {
        let Ok(read) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let kind = entry.file_type();
            if kind.as_ref().is_ok_and(|k| k.is_dir()) && bag_holds(&path, secret) {
                return true;
            }
            if std::fs::read(&path).ok().as_deref() == Some(secret) {
                return true;
            }
        }
        false
    }
}
