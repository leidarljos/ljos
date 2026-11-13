//! Repairs `ljos doctor --fix` may make, and the rows that name them.
//!
//! Each repair stays inside the ljos directories and can be undone.
//! A missing state directory and a missing `ljos.toml` are created.
//! The config file sets no keys, so the defaults stand.
//! A lock or pid file whose recorded process is gone is renamed with a
//! `.bak` suffix. A file that is still in use, a key, a credential, a
//! policy file, a git hook and a runner hook are left where they are.
//! A pack writer that is not answering is started with `packset ensure`.
//! A runner session id with no session seat on record gets that seat written.
//!
//! Without `--fix`, each of these rows ends with `run ljos doctor --fix`.
//! `--fix` prints one line for each repair and one line for each problem
//! it could not repair, and that line names the one command to run.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Habitat;

const HINT: &str = "run ljos doctor --fix";
const CONFIG_TEXT: &str = "\
# ljos.toml\n\
# A key that is not set here keeps its default.\n\
";

/// Append the fix hint to a pack row `ljos doctor --fix` can repair.
#[must_use]
pub(crate) fn hint_pack(state: String) -> String {
    if state.contains(HINT) || !state.contains("packset ensure") || !pack_may_start() {
        return state;
    }
    format!("{state} {HINT}")
}

/// Whether the session record already names this process's session seat.
/// A later resolution leaves that record in place. Unit tests do not write
/// the record, so this check is only compiled into the seat binary.
#[cfg(not(test))]
#[must_use]
pub(crate) fn session_seat_is_recorded() -> bool {
    let Some(session) = crate::live_harness_session() else {
        return false;
    };
    recorded_seat(&session_path(&session.id)).as_deref() == Some(session.name.as_str())
}

/// The rows `ljos doctor` prints for repairs it can make.
#[must_use]
pub(crate) fn rows() -> Vec<Habitat> {
    let mut out = Vec::new();
    if let Some(row) = home_row() {
        out.push(row);
    }
    if let Some(row) = config_row() {
        out.push(row);
    }
    for stale in stale_files() {
        out.push(problem(
            "stale lock",
            format!(
                "{} names pid {}, which is not running",
                stale.path.display(),
                stale.pid
            ),
        ));
    }
    if let Some(row) = session_row() {
        out.push(row);
    }
    out
}

/// Make the repairs and print one line each. Problems that stay name the
/// one command that would repair them.
pub fn apply() {
    for line in repairs() {
        eprintln!("ljos: {line}");
    }
}

fn repairs() -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(line) = repair_home() {
        lines.push(line);
    }
    if let Some(line) = repair_config() {
        lines.push(line);
    }
    for stale in stale_files() {
        lines.push(repair_stale(&stale));
    }
    if let Some(line) = repair_pack() {
        lines.push(line);
    }
    if let Some(line) = repair_session() {
        lines.push(line);
    }
    lines
}

fn problem(name: &'static str, state: String) -> Habitat {
    Habitat {
        name,
        state: format!("{state}, so {HINT}"),
        ok: false,
    }
}

fn home_dir() -> Option<PathBuf> {
    let path = crate::state_base_dir();
    path.is_absolute().then_some(path)
}

fn config_file() -> Option<PathBuf> {
    Some(crate::config_base()?.join("ljos").join("ljos.toml"))
}

fn home_row() -> Option<Habitat> {
    let path = home_dir()?;
    if dir_ready(&path) {
        return None;
    }
    let why = if path.exists() {
        format!("{} is not a directory", path.display())
    } else {
        format!("{} is missing", path.display())
    };
    Some(problem("ljos home", why))
}

fn config_row() -> Option<Habitat> {
    let path = config_file()?;
    if path.is_file() {
        return None;
    }
    let why = if path.exists() {
        format!("{} is not a file", path.display())
    } else {
        format!("{} is missing", path.display())
    };
    Some(problem("ljos config", why))
}

fn session_row() -> Option<Habitat> {
    let session = unrecorded_session()?;
    Some(problem(
        "session seat",
        format!("{} has no seat recorded for {}", session.key, session.name),
    ))
}

fn repair_home() -> Option<String> {
    let path = home_dir()?;
    if dir_ready(&path) {
        return None;
    }
    match create_private_dir(&path) {
        Ok(()) => Some(format!("fixed ljos home: created {}", path.display())),
        Err(err) => Some(format!(
            "left ljos home: {} ({err}), so run `mkdir -p {}`",
            path.display(),
            sh_quote(&path)
        )),
    }
}

fn repair_config() -> Option<String> {
    let path = config_file()?;
    if path.is_file() {
        return None;
    }
    if path.exists() {
        return Some(format!(
            "left ljos config: {} is not a file, so run `mkdir -p {}`",
            path.display(),
            sh_quote(&path)
        ));
    }
    if let Some(dir) = path.parent() {
        if let Err(err) = create_private_dir(dir) {
            return Some(format!(
                "left ljos config: {} ({err}), so run `mkdir -p {}`",
                path.display(),
                sh_quote(dir)
            ));
        }
    }
    match write_private(&path, CONFIG_TEXT) {
        Ok(()) => Some(format!(
            "fixed ljos config: wrote {} with defaults",
            path.display()
        )),
        Err(err) => Some(format!(
            "left ljos config: {} ({err}), so run `mkdir -p {}`",
            path.display(),
            sh_quote(path.parent().unwrap_or(Path::new(".")))
        )),
    }
}

fn repair_stale(stale: &Stale) -> String {
    match move_aside(&stale.path) {
        Ok(dest) => format!(
            "fixed stale lock: moved {} aside to {}",
            stale.path.display(),
            dest.display()
        ),
        Err(err) => {
            let dest = stale.path.with_file_name(format!(
                "{}.bak",
                stale
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("lock")
            ));
            format!(
                "left stale lock: {} ({err}), so run `mv {} {}`",
                stale.path.display(),
                sh_quote(&stale.path),
                sh_quote(&dest)
            )
        }
    }
}

fn repair_pack() -> Option<String> {
    if url_blocks() || crate::bookkeep::ljos_is_off() {
        return None;
    }
    if !writer_down() {
        return None;
    }
    if !crate::on_path("packset") {
        return Some(
            "left pack: packset is not on PATH, so run `cargo install --locked packset`".into(),
        );
    }
    // The automatic start refuses a temporary home. `--fix` is the person
    // asking, so this runs the same `packset ensure` that start uses.
    match crate::run_captured("packset", &["ensure"]) {
        Ok(_) => Some("fixed pack: ran `packset ensure`".into()),
        Err(err) => {
            let why = err.to_string();
            let why = why.lines().next().unwrap_or("packset ensure failed");
            Some(format!("left pack: {why}, so run `packset ensure`"))
        }
    }
}

fn repair_session() -> Option<String> {
    let session = unrecorded_session()?;
    let path = session_path(&session.id);
    if let Some(dir) = path.parent() {
        if let Err(err) = create_private_dir(dir) {
            return Some(format!(
                "left session seat: {} ({err}), so run `mkdir -p {}`",
                path.display(),
                sh_quote(dir)
            ));
        }
    }
    let seat = crate::Seat {
        holder: crate::holder_for_session(&session.name, &session.id),
        seat: session.name.clone(),
        source: format!("{} in this process", session.key),
    };
    crate::write_record_ids(&path, &seat, std::slice::from_ref(&session.id));
    if recorded_seat(&path).as_deref() == Some(session.name.as_str()) {
        Some(format!(
            "fixed session seat: recorded {} for {}",
            session.name, session.key
        ))
    } else {
        Some(format!(
            "left session seat: {} was not written, so run `mkdir -p {}`",
            path.display(),
            sh_quote(path.parent().unwrap_or(Path::new(".")))
        ))
    }
}

struct Session {
    key: &'static str,
    id: String,
    name: String,
}

fn unrecorded_session() -> Option<Session> {
    if crate::named_var("LJOS_SEAT").is_some() {
        return None;
    }
    let session = crate::live_harness_session()?;
    let path = session_path(&session.id);
    if recorded_seat(&path).as_deref() == Some(session.name.as_str()) {
        return None;
    }
    Some(Session {
        key: session.key,
        id: session.id,
        name: session.name,
    })
}

fn session_path(id: &str) -> PathBuf {
    crate::runtime_dir_path().join(format!("session-{}", crate::session_tag(id)))
}

fn recorded_seat(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let seat = text.lines().next()?.trim();
    (!seat.is_empty()).then(|| seat.to_string())
}

struct Stale {
    path: PathBuf,
    pid: u32,
}

fn stale_files() -> Vec<Stale> {
    let mut found = Vec::new();
    for dir in scan_dirs() {
        walk(&dir, 0, &mut found);
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

fn scan_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = home_dir() {
        if real_dir(&path) {
            dirs.push(path);
        }
    }
    if let Some(path) = crate::config_base() {
        let path = path.join("ljos");
        if path.is_absolute() && real_dir(&path) {
            dirs.push(path);
        }
    }
    let runtime = crate::runtime_dir_path();
    if runtime.is_absolute() && real_dir(&runtime) {
        dirs.push(runtime);
    }
    dirs
}

fn walk(dir: &Path, depth: u32, found: &mut Vec<Stale>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(&path, depth + 1, found);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if let Some(pid) = stale_pid(&path, meta.len()) {
            found.push(Stale { path, pid });
        }
    }
}

fn stale_pid(path: &Path, len: u64) -> Option<u32> {
    if len > 256 || !lock_name(path) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let pid = text.split_whitespace().next()?.parse::<u32>().ok()?;
    (pid > 0 && process_gone(pid)).then_some(pid)
}

fn lock_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.contains(".bak") || sensitive_name(name) {
        return false;
    }
    name.ends_with(".pid") || name.ends_with(".lock") || name.starts_with("hookd.flight-")
}

fn sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".age")
        || name.ends_with(".key")
        || name.contains("token")
        || name.contains("secret")
        || name.contains("credential")
        || name.contains("passwd")
}

/// `ESRCH` is a process that is gone. `EPERM` is a process we must not move.
fn process_gone(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 asks whether `pid` exists. It does not deliver a signal.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return false;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn move_aside(path: &Path) -> std::io::Result<PathBuf> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "unnamed lock"))?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    for n in 0..100 {
        let dest = if n == 0 {
            dir.join(format!("{name}.bak"))
        } else {
            dir.join(format!("{name}.bak.{n}"))
        };
        if dest.exists() {
            continue;
        }
        std::fs::rename(path, &dest)?;
        return Ok(dest);
    }
    Err(std::io::Error::other("no unused .bak name"))
}

fn dir_ready(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

fn pack_may_start() -> bool {
    !url_blocks() && !crate::bookkeep::ljos_is_off() && crate::on_path("packset")
}

fn url_blocks() -> bool {
    std::env::var("PACKSET_URL")
        .ok()
        .is_some_and(|url| !url.trim().is_empty())
}

fn writer_down() -> bool {
    if url_blocks() || crate::bookkeep::ljos_is_off() {
        return false;
    }
    match crate::pack() {
        Ok(client) => client.health().is_err(),
        Err(_) => true,
    }
}

fn sh_quote(path: &Path) -> String {
    let text = path.display().to_string();
    if text.chars().any(|c| {
        !(c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+' | ':' | ','))
    }) {
        format!("'{}'", text.replace('\'', "'\\''"))
    } else {
        text
    }
}
