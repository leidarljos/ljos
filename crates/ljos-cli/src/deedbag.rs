//! A cloud seat's finish writes the deeds it cited into the tracker.
//!
//! The box imports that bag after the machine is gone. `deedar export
//! --into` writes the deed frame and the inclusion receipt. Import also
//! requires the product bytes under `bytes/` and, when a deed supersedes
//! another or was made from one, a sidecar that names that deed and its
//! digest. Export on the import branch writes that two-line form, and
//! the host signature in its own file. Finish leaves a sidecar that
//! already names a sha256 digest, and writes the two-line form when
//! export left the store's one-line id or nothing. A carried deed made
//! from one this bag does not carry gets a tab-separated `sources` line
//! when export wrote none. A sibling directory would be another deed to
//! `deedar check`, which requires `proof.txt` in every folder.

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
    publish_bag(&root, &seat, scratch.path())?;
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

/// A link sidecar is `<id>\nsha256:<digest>\n`. Export writes that, and may
/// carry a signature on a later line. The store file is the id alone.
/// Copying it over the export drops the digest, and rewriting a file that
/// already names one would drop the signature. A missing sidecar, or that
/// one-line id, is written in the two-line form.
fn write_link(
    store: &Path,
    id: &str,
    name: &str,
    folder: &Path,
    digests: &mut HashMap<String, String>,
) -> Result<()> {
    let dest = folder.join(name);
    match file_kind(&dest)? {
        Kind::Symlink => bail!("{id} {name} is a symlink"),
        Kind::Other => bail!("{id} {name} is not a file"),
        Kind::File => {
            if link_keeps_digest(&dest)? {
                return Ok(());
            }
            let Some(linked) = read_lone_id(&dest)? else {
                return Ok(());
            };
            return write_two_line(&dest, &linked, digests);
        }
        Kind::Missing => {}
    }
    let src = store.join("deeds").join(format!("{id}.{name}"));
    let linked = match file_kind(&src)? {
        Kind::Missing | Kind::Other => return Ok(()),
        Kind::Symlink => bail!("{id}.{name} is a symlink"),
        Kind::File => match read_lone_id(&src)? {
            Some(linked) => linked,
            None => return Ok(()),
        },
    };
    write_two_line(&dest, &linked, digests)
}

fn write_two_line(dest: &Path, linked: &str, digests: &mut HashMap<String, String>) -> Result<()> {
    let digest = deed_digest(digests, linked)?;
    let body = format!("{linked}\n{digest}\n");
    std::fs::write(dest, body).with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

/// True when the file already names a deed and a sha256 digest. Later lines
/// stay, so a signature export wrote is not stripped.
fn link_keeps_digest(path: &Path) -> Result<bool> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut lines = raw.lines().map(str::trim).filter(|line| !line.is_empty());
    let (Some(id), Some(digest)) = (lines.next(), lines.next()) else {
        return Ok(false);
    };
    Ok(one_component(id).is_ok() && is_sha256_addr(digest))
}

/// The id in a one-line sidecar. Two lines that are not a digest are refused
/// rather than rewritten.
fn read_lone_id(path: &Path) -> Result<Option<String>> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut lines = raw.lines().map(str::trim).filter(|line| !line.is_empty());
    let Some(id) = lines.next() else {
        return Ok(None);
    };
    if lines.next().is_some() {
        bail!("{} is not an id and a sha256 digest", path.display());
    }
    one_component(id).with_context(|| format!("{} is not a deed id", path.display()))?;
    Ok(Some(id.to_string()))
}

/// `sources=` lines from `deedar get`: `deed:<id>`, then that deed's own
/// sources, one `<id><tab><digest>` a line. Path and url sources are not
/// deeds, so they have no sidecar line. A file export already wrote in
/// that form is left in place.
fn write_sources(
    sources: &str,
    folder: &Path,
    digests: &mut HashMap<String, String>,
) -> Result<()> {
    let dest = folder.join("sources");
    match file_kind(&dest)? {
        Kind::Symlink => bail!("sources is a symlink"),
        Kind::Other => bail!("sources is not a file"),
        Kind::File => {
            if sources_keep_digests(&dest)? {
                return Ok(());
            }
            bail!("sources is not a digest sidecar");
        }
        Kind::Missing => {}
    }
    let lines = source_lines(sources, digests)?;
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
    std::fs::write(&dest, body).context("writing sources")?;
    Ok(())
}

/// True when the file already binds at least one deed to a sha256 digest.
/// Other lines stay, so a signature export wrote is not stripped.
fn sources_keep_digests(path: &Path) -> Result<bool> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .any(|line| {
            line.split_once('\t')
                .is_some_and(|(id, digest)| one_component(id).is_ok() && is_sha256_addr(digest))
        }))
}

/// Every deed this one was made from, including a source's own sources.
/// Import looks each of them up in this file when the bag does not carry it.
fn source_lines(
    sources: &str,
    digests: &mut HashMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut lines = BTreeMap::new();
    let mut pending = deed_ids(sources)?;
    while let Some(id) = pending.pop() {
        if lines.contains_key(&id) {
            continue;
        }
        let digest = deed_digest(digests, &id)?;
        lines.insert(id.clone(), digest);
        let Some(text) = deed_text(&id)? else {
            bail!("source {id} is not in the store");
        };
        pending.extend(deed_ids(field(&text, "sources").unwrap_or(""))?);
    }
    Ok(lines)
}

fn deed_ids(sources: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for part in sources.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some(id) = part.strip_prefix("deed:") else {
            continue;
        };
        one_component(id).with_context(|| format!("source {part} is not a deed id"))?;
        out.push(id.to_string());
    }
    Ok(out)
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

/// Publish `scratch` as `.ljos/deeds/<seat>`. The deeds directory is opened,
/// or made, one component at a time without following a symlink, and every
/// later step uses that file descriptor: a symlink swapped in for `.ljos`
/// or `.ljos/deeds` after the open is not where the bag is written.
fn publish_bag(root: &Path, seat: &str, scratch: &Path) -> Result<()> {
    let dir = open_deeds(root)?;
    refuse_seat(&dir, root, seat)?;
    before_publish();
    publish(&dir, seat, scratch)
}

fn open_deeds(root: &Path) -> Result<std::fs::File> {
    crate::sync::open_dir_beneath(root, ".ljos/deeds", true)?
        .with_context(|| format!("sync: {} was not made", root.join(".ljos/deeds").display()))
}

fn refuse_seat(dir: &std::fs::File, root: &Path, seat: &str) -> Result<()> {
    let name = c_component(seat)?;
    if let Some(st) = stat_at(dir, &name)? {
        if !is_dir(st.st_mode) {
            bail!(
                "{} is not a directory; the deeds were not written",
                root.join(".ljos/deeds").join(seat).display()
            );
        }
    }
    Ok(())
}

/// Runs after the deeds directory is open and the seat entry has been
/// checked, and before anything is written. Tests swap a symlink in here.
fn before_publish() {
    #[cfg(test)]
    BEFORE_PUBLISH.with(|hook| {
        if let Some(mut run) = hook.borrow_mut().take() {
            run();
        }
    });
}

#[cfg(test)]
thread_local! {
    static BEFORE_PUBLISH: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Put `scratch` in `parent` under `seat`. The old bag stays until a
/// finished tree has been staged beside it: a copy that fails, including
/// one across filesystems, leaves `seat` as it was. `parent` is the open
/// deeds directory; staging, the swap and the publish are mkdirat, openat
/// and renameat on it, then an fsync of that directory.
fn publish(parent: &std::fs::File, seat: &str, scratch: &Path) -> Result<()> {
    let staged = c_component(&format!(".{seat}.incoming"))?;
    let backup = c_component(&format!(".{seat}.previous"))?;
    let dest = c_component(seat)?;
    let where_ = root_label(seat);
    clear_entry(parent, &staged).with_context(|| format!("could not clear {where_}"))?;
    // A crash after the seat moved aside and before the new tree took its
    // place leaves the published bag at the backup name. Put it back
    // before touching it.
    if stat_at(parent, &dest)?.is_none() && stat_at(parent, &backup)?.is_some() {
        rename_at(parent, &backup, parent, &dest)
            .with_context(|| format!("could not restore {where_}"))?;
    }
    clear_entry(parent, &backup).with_context(|| format!("could not clear {where_}"))?;

    stage_into(scratch, parent, &staged)?;

    let replacing = stat_at(parent, &dest)?.is_some();
    if replacing {
        if let Err(e) = rename_at(parent, &dest, parent, &backup) {
            let _ = clear_entry(parent, &staged);
            return Err(e).with_context(|| format!("could not move {where_} aside"));
        }
    }
    if let Err(e) = rename_at(parent, &staged, parent, &dest) {
        if replacing {
            let _ = rename_at(parent, &backup, parent, &dest);
        }
        let _ = clear_entry(parent, &staged);
        return Err(e).with_context(|| format!("could not place {where_}"));
    }
    if replacing {
        let _ = clear_entry(parent, &backup);
    }
    let _ = std::fs::remove_dir_all(scratch);
    parent
        .sync_all()
        .with_context(|| format!("could not record {where_}"))
}

fn root_label(seat: &str) -> String {
    format!(".ljos/deeds/{seat}")
}

fn c_component(name: &str) -> Result<std::ffi::CString> {
    if name.is_empty() || name.contains('/') || name.contains('\0') {
        bail!("{name} is not a folder name");
    }
    std::ffi::CString::new(name).map_err(|_| anyhow::anyhow!("NUL in `{name}`"))
}

fn is_dir(mode: libc::mode_t) -> bool {
    (mode & libc::S_IFMT) == libc::S_IFDIR
}

pub(crate) fn stat_at(dir: &std::fs::File, name: &std::ffi::CStr) -> Result<Option<libc::stat>> {
    use std::os::fd::AsRawFd;
    // SAFETY: dir is an open directory fd and name is NUL-terminated. stat
    // is zeroed and only read when fstatat fills it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc == 0 {
        return Ok(Some(st));
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(err.into())
    }
}

fn rename_at(
    from_dir: &std::fs::File,
    from: &std::ffi::CStr,
    to_dir: &std::fs::File,
    to: &std::ffi::CStr,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: both fds are open directories and both names are
    // NUL-terminated. renameat follows neither name.
    if unsafe {
        libc::renameat(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn open_subdir(dir: &std::fs::File, name: &std::ffi::CStr) -> Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    // SAFETY: dir is an open directory fd and name is NUL-terminated.
    // O_NOFOLLOW refuses a symlink swapped in for this entry.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fd is new, owned here, and opened with O_CLOEXEC.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

fn clear_entry(dir: &std::fs::File, name: &std::ffi::CStr) -> Result<()> {
    use std::os::fd::AsRawFd;
    let Some(st) = stat_at(dir, name)? else {
        return Ok(());
    };
    if is_dir(st.st_mode) {
        let child = open_subdir(dir, name)?;
        for entry in list_names(&child)? {
            clear_entry(&child, &entry)?;
        }
        // SAFETY: dir is an open directory fd and name is NUL-terminated.
        // AT_REMOVEDIR removes the directory itself, not a link target.
        if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    } else {
        // SAFETY: dir is an open directory fd and name is NUL-terminated.
        // A symlink is removed, not followed.
        if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

pub(crate) fn list_names(dir: &std::fs::File) -> Result<Vec<std::ffi::CString>> {
    use std::os::fd::AsRawFd;
    // SAFETY: dup makes an fd fdopendir can take. dir stays open.
    let fd = unsafe { libc::dup(dir.as_raw_fd()) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fd is an open directory fd owned here; fdopendir takes it.
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        let err = std::io::Error::last_os_error();
        // SAFETY: fdopendir failed, so fd is still ours to close.
        unsafe { libc::close(fd) };
        return Err(err.into());
    }
    let mut names = Vec::new();
    loop {
        // SAFETY: stream is open; readdir returns null at the end.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            break;
        }
        // SAFETY: d_name is NUL-terminated inside the entry readdir returned.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        names.push(name.to_owned());
    }
    // SAFETY: stream is open and closed once; this closes fd too.
    unsafe { libc::closedir(stream) };
    Ok(names)
}

fn stage_into(scratch: &Path, parent: &std::fs::File, staged: &std::ffi::CStr) -> Result<()> {
    // Same filesystem: the rename is the stage. Across filesystems the
    // copy has to finish before the seat is touched, or a failure would
    // publish nothing. The destination name is created from `parent`.
    if rename_scratch(scratch, parent, staged).is_ok() {
        return Ok(());
    }
    match copy_scratch(scratch, parent, staged) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = clear_entry(parent, staged);
            Err(e).context("could not stage the deed bag")
        }
    }
}

fn rename_scratch(scratch: &Path, parent: &std::fs::File, staged: &std::ffi::CStr) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let from_dir = scratch.parent().context("the deed bag has no directory")?;
    let from_name = scratch
        .file_name()
        .and_then(|n| n.to_str())
        .context("the deed bag has no name")?;
    let from = c_component(from_name)?;
    let src = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(from_dir)
        .with_context(|| format!("could not open {}", from_dir.display()))?;
    rename_at(&src, &from, parent, staged)?;
    Ok(())
}

fn copy_scratch(scratch: &Path, parent: &std::fs::File, staged: &std::ffi::CStr) -> Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: parent is an open directory fd and staged is NUL-terminated.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), staged.as_ptr(), 0o777) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let dest = open_subdir(parent, staged)?;
    copy_children(scratch, &dest)?;
    dest.sync_all()?;
    Ok(())
}

fn copy_children(from: &Path, dest: &std::fs::File) -> Result<()> {
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd};
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let name = entry.file_name();
        let Some(text) = name.to_str() else {
            bail!("{} is not a folder name", entry.path().display());
        };
        let cname = c_component(text)?;
        if kind.is_symlink() {
            bail!("{} is a symlink", entry.path().display());
        } else if kind.is_dir() {
            // SAFETY: dest is an open directory fd and cname is NUL-terminated.
            if unsafe { libc::mkdirat(dest.as_raw_fd(), cname.as_ptr(), 0o777) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let child = open_subdir(dest, &cname)?;
            copy_children(&entry.path(), &child)?;
            child.sync_all()?;
        } else if kind.is_file() {
            let bytes = std::fs::read(entry.path())?;
            // SAFETY: dest is an open directory fd and cname is NUL-terminated.
            // O_EXCL with O_NOFOLLOW makes a new file or fails.
            let fd = unsafe {
                libc::openat(
                    dest.as_raw_fd(),
                    cname.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o666,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: fd is new, owned here, and opened with O_CLOEXEC.
            let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
            file.write_all(&bytes)?;
            file.sync_all()?;
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

    /// Export already wrote id, digest, and a signature line. The store
    /// still holds the one-line id. Seam must not copy that over the export.
    #[test]
    fn an_exported_digest_sidecar_is_left_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let bag = dir.path().join("bag");
        let id = "deed-file-new";
        let prior = "deed-file-old";
        let digest = format!("sha256:{}", "ab".repeat(32));
        std::fs::create_dir_all(store.join("deeds")).unwrap();
        std::fs::write(
            store.join("deeds").join(format!("{id}.supersedes")),
            format!("{prior}\n"),
        )
        .unwrap();
        std::fs::write(
            store.join("deeds").join(format!("{prior}.successor")),
            format!("{id}\n"),
        )
        .unwrap();
        let folder = bag.join(id);
        let prior_folder = bag.join(prior);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(&prior_folder).unwrap();
        let signed = format!("{prior}\n{digest}\ned25519:keep\n");
        std::fs::write(folder.join("supersedes"), &signed).unwrap();
        let successor = format!("{id}\n{digest}\n");
        std::fs::write(prior_folder.join("successor"), &successor).unwrap();
        let sources = format!("{prior}\t{digest}\ned25519:keep\n");
        std::fs::write(folder.join("sources"), &sources).unwrap();
        seam(
            &store,
            &bag,
            &[
                Row {
                    id: id.to_string(),
                    hexes: Vec::new(),
                    sources: format!("deed:{prior}"),
                },
                Row {
                    id: prior.to_string(),
                    hexes: Vec::new(),
                    sources: String::new(),
                },
            ],
        )
        .unwrap_or_else(|e| panic!("seam rewrote an exported sidecar: {e:#}"));
        assert_eq!(
            std::fs::read(folder.join("supersedes")).unwrap(),
            signed.into_bytes()
        );
        assert_eq!(
            std::fs::read(prior_folder.join("successor")).unwrap(),
            successor.into_bytes()
        );
        assert_eq!(
            std::fs::read(folder.join("sources")).unwrap(),
            sources.into_bytes()
        );
    }

    #[test]
    fn a_symlinked_link_sidecar_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let bag = dir.path().join("bag");
        let id = "deed-file-note";
        let outside = dir.path().join("outside");
        std::fs::write(&outside, b"not a digest\n").unwrap();
        std::fs::create_dir_all(store.join("deeds")).unwrap();
        std::fs::write(
            store.join("deeds").join(format!("{id}.supersedes")),
            "deed-file-old\n",
        )
        .unwrap();
        std::fs::create_dir_all(bag.join(id)).unwrap();
        std::os::unix::fs::symlink(&outside, bag.join(id).join("supersedes")).unwrap();
        let err = seam(
            &store,
            &bag,
            &[Row {
                id: id.to_string(),
                hexes: Vec::new(),
                sources: String::new(),
            }],
        )
        .expect_err("a symlinked sidecar was followed");
        let text = format!("{err:#}");
        assert!(text.contains("symlink"), "{text}");
        assert_eq!(std::fs::read(&outside).unwrap(), b"not a digest\n");
        std::fs::remove_file(bag.join(id).join("supersedes")).unwrap();
        let digest = format!("sha256:{}", "ab".repeat(32));
        std::fs::write(
            bag.join(id).join("supersedes"),
            format!("deed-file-old\n{digest}\n"),
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, bag.join(id).join("sources")).unwrap();
        let err = seam(
            &store,
            &bag,
            &[Row {
                id: id.to_string(),
                hexes: Vec::new(),
                sources: "deed:deed-file-old".to_string(),
            }],
        )
        .expect_err("a symlinked sources file was followed");
        let text = format!("{err:#}");
        assert!(text.contains("symlink"), "{text}");
        assert_eq!(std::fs::read(&outside).unwrap(), b"not a digest\n");
    }

    #[test]
    fn a_symlinked_store_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let bag = dir.path().join("bag");
        let id = "deed-file-note";
        let outside = dir.path().join("outside");
        std::fs::write(&outside, b"deed-file-old\n").unwrap();
        std::fs::create_dir_all(store.join("deeds")).unwrap();
        std::os::unix::fs::symlink(
            &outside,
            store.join("deeds").join(format!("{id}.supersedes")),
        )
        .unwrap();
        std::fs::create_dir_all(bag.join(id)).unwrap();
        let err = seam(
            &store,
            &bag,
            &[Row {
                id: id.to_string(),
                hexes: Vec::new(),
                sources: String::new(),
            }],
        )
        .expect_err("a symlinked store link was copied");
        let text = format!("{err:#}");
        assert!(text.contains("symlink"), "{text}");
        assert!(bag.join(id).join("supersedes").symlink_metadata().is_err());
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
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("seat");
        std::fs::create_dir_all(dest.join("kept")).unwrap();
        std::fs::write(dest.join("kept").join("marker"), b"old bag").unwrap();
        // SAFETY: geteuid only reads this process's credentials.
        assert_ne!(
            unsafe { libc::geteuid() },
            0,
            "root can read a mode 000 file, so the copy would not fail"
        );
        // The parent cannot be written, so the rename that would stage
        // the bag on one filesystem fails and the copy runs. A second
        // filesystem is not required.
        let hold = tempfile::tempdir().unwrap();
        let parent_dir = hold.path().join("hold");
        std::fs::create_dir(&parent_dir).unwrap();
        let scratch = tempfile::tempdir_in(&parent_dir).unwrap();
        std::fs::create_dir_all(scratch.path().join("fresh")).unwrap();
        let blocked = scratch.path().join("fresh").join("blocked");
        std::fs::write(&blocked, b"new").unwrap();
        let mut perms = std::fs::metadata(&blocked).unwrap().permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&blocked, perms).unwrap();
        let mut dir_perms = std::fs::metadata(&parent_dir).unwrap().permissions();
        dir_perms.set_mode(0o555);
        std::fs::set_permissions(&parent_dir, dir_perms).unwrap();
        let parent = std::fs::File::open(dir.path()).unwrap();
        let err = publish(&parent, "seat", scratch.path());
        let mut dir_perms = std::fs::metadata(&parent_dir).unwrap().permissions();
        dir_perms.set_mode(0o755);
        std::fs::set_permissions(&parent_dir, dir_perms).unwrap();
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

    /// A peer commits `.ljos/deeds`, or `.ljos`, as a link to a directory
    /// outside the tracker. The bag was published wherever the link
    /// pointed.
    #[test]
    fn a_linked_deeds_directory_is_not_written_through() {
        let git = |cwd: &Path, args: &[&str]| {
            let out = crate::git_cmd()
                .arg("-C")
                .arg(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        for linked in [".ljos/deeds", ".ljos"] {
            let dir = tempfile::tempdir().unwrap();
            let outside = dir.path().join("outside");
            std::fs::create_dir_all(outside.join("deeds")).unwrap();
            let origin = dir.path().join("origin.git");
            let peer = dir.path().join("peer");
            let root = dir.path().join("tracker");
            git(
                dir.path(),
                &[
                    "init",
                    "-q",
                    "--bare",
                    "-b",
                    "main",
                    origin.to_str().unwrap(),
                ],
            );
            git(
                dir.path(),
                &[
                    "clone",
                    "-q",
                    origin.to_str().unwrap(),
                    peer.to_str().unwrap(),
                ],
            );
            for (key, value) in [
                ("user.email", "seat@example.invalid"),
                ("user.name", "seat"),
                ("core.hooksPath", "/dev/null"),
                ("commit.gpgsign", "false"),
            ] {
                git(&peer, &["config", key, value]);
            }
            let link = peer.join(linked);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            let target = if linked == ".ljos" {
                outside.clone()
            } else {
                outside.join("deeds")
            };
            std::os::unix::fs::symlink(&target, &link).unwrap();
            git(&peer, &["add", "-A"]);
            git(&peer, &["commit", "-q", "-m", "link"]);
            git(&peer, &["push", "-q", "-u", "origin", "HEAD"]);
            git(
                dir.path(),
                &[
                    "clone",
                    "-q",
                    origin.to_str().unwrap(),
                    root.to_str().unwrap(),
                ],
            );
            let scratch = dir.path().join("scratch");
            std::fs::create_dir_all(scratch.join("deed-1")).unwrap();
            std::fs::write(scratch.join("deed-1/proof.txt"), b"the new bag").unwrap();
            let placed = publish_bag(&root, "cloudseat", &scratch);
            assert!(!bag_holds(&outside, b"the new bag"), "{linked}: {placed:?}");
            let err = format!("{:#}", placed.unwrap_err());
            assert!(err.contains("symlink"), "{linked}: {err}");
        }
    }

    /// The deeds directory is a real directory when it is opened. A hook
    /// then swaps `.ljos` or `.ljos/deeds` for a symlink to a directory
    /// outside the tracker. The bag is written through the directory fd
    /// from the open, so the outside directory stays empty.
    #[test]
    fn a_deeds_directory_swapped_after_the_open_is_not_written_through() {
        for linked in [".ljos", ".ljos/deeds"] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("tracker");
            let outside = dir.path().join("outside");
            std::fs::create_dir_all(root.join(".ljos/deeds")).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            let scratch = dir.path().join("scratch");
            std::fs::create_dir_all(scratch.join("deed-1")).unwrap();
            std::fs::write(scratch.join("deed-1/proof.txt"), b"the new bag").unwrap();
            let hook_root = root.clone();
            let hook_outside = outside.clone();
            let which = linked.to_string();
            BEFORE_PUBLISH.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    let path = hook_root.join(&which);
                    let saved = hook_root.join(format!("{which}.real"));
                    if let Some(parent) = saved.parent() {
                        std::fs::create_dir_all(parent).unwrap();
                    }
                    std::fs::rename(&path, &saved).unwrap();
                    std::os::unix::fs::symlink(&hook_outside, &path).unwrap();
                }));
            });
            let placed = publish_bag(&root, "cloudseat", &scratch);
            BEFORE_PUBLISH.with(|hook| *hook.borrow_mut() = None);
            assert!(placed.is_ok(), "{linked}: {placed:?}");
            assert!(
                !bag_holds(&outside, b"the new bag"),
                "{linked}: bag landed outside the tracker"
            );
            let saved = root.join(format!("{linked}.real"));
            let kept = if linked == ".ljos" {
                saved.join("deeds")
            } else {
                saved
            };
            assert!(
                bag_holds(&kept, b"the new bag"),
                "{linked}: bag missing from the directory that was opened"
            );
        }
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
