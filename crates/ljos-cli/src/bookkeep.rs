//! A note written after a commit.
//!
//! The claim book is gone. What stays is the guard's notion of a temporary
//! home, a durable file replace, and one line in the runtime log when a
//! commit lands.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;

static LAST_ACTIVITY: AtomicU64 = AtomicU64::new(0);
static DURABLE_WRITES: AtomicUsize = AtomicUsize::new(0);

/// Held for the body of a write that replaces a file.
pub struct DurableWrite(());

impl Drop for DurableWrite {
    fn drop(&mut self) {
        DURABLE_WRITES.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Mark one write in progress. Drop the guard when the write returns.
#[must_use]
pub fn durable_write() -> DurableWrite {
    DURABLE_WRITES.fetch_add(1, Ordering::AcqRel);
    DurableWrite(())
}

/// A [`durable_write`] guard is still held.
#[must_use]
pub fn durable_pending() -> bool {
    DURABLE_WRITES.load(Ordering::Acquire) > 0
}

/// MCP tools that write a habitat. The book no longer joins them to a claim.
const MCP_WRITES: &[&str] = &[
    "ljos_remember",
    "ljos_prefer",
    "ljos_forget",
    "ljos_deed",
    "ljos_rule",
    "ljos_vote",
    "ljos_learn",
    "ljos_trust",
    "ljos_persona",
    "ljos_playbook",
    "ljos_graded",
    "ljos_consolidate",
    "ljos_calibrate",
    "ljos_habit",
];

/// Whether this MCP tool writes a habitat.
#[must_use]
pub fn mcp_tool_writes(name: &str) -> bool {
    MCP_WRITES.contains(&name)
}

/// A write tool runs on its own. The book does not attach it to a claim.
pub fn before_mcp_write(_name: &str, _args: Option<&serde_json::Map<String, Value>>) {}

/// The server exit no longer finishes claims.
pub fn mcp_shutdown() {}

/// Note that a tool ran, so the idle watcher waits another window.
pub fn touch_activity() {
    LAST_ACTIVITY.store(now_secs(), Ordering::Relaxed);
}

/// Unix seconds of the last [`touch_activity`], or zero.
#[must_use]
pub fn last_activity() -> u64 {
    LAST_ACTIVITY.load(Ordering::Relaxed)
}

/// Unix seconds now.
#[must_use]
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Count one tool call in this process.
pub fn note_mcp_process_call() {}

/// The next process does not finish a claim book.
pub fn resume_seat() {}

/// How long a silent MCP server sits. Unset is thirty minutes.
#[must_use]
pub fn idle_limit() -> Option<Duration> {
    idle_limit_from(std::env::var("LJOS_MCP_IDLE").ok().as_deref())
}

/// [`idle_limit`] from an already-read value.
#[must_use]
pub fn idle_limit_from(raw: Option<&str>) -> Option<Duration> {
    let Some(raw) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
        return Some(Duration::from_secs(30 * 60));
    };
    let lowered = raw.to_ascii_lowercase();
    if matches!(lowered.as_str(), "0" | "off" | "false" | "no") {
        return None;
    }
    match super::reap::parse_idle(raw) {
        Ok(window) if window.is_zero() => None,
        Ok(window) => Some(window),
        Err(_) => Some(Duration::from_secs(30 * 60)),
    }
}

/// Whether `now` is at least `limit` past `last`.
#[must_use]
pub fn idle_due(now: u64, last: u64, limit: Duration) -> bool {
    last > 0 && now.saturating_sub(last) >= limit.as_secs()
}

/// The idle watcher does not finish a claim book.
pub fn spawn_idle_finish() {}

/// Words a model passes when it does not know the issue id.
#[must_use]
pub fn blank_issue(id: &str) -> bool {
    matches!(
        id.trim().to_ascii_lowercase().as_str(),
        "" | "none"
            | "null"
            | "nil"
            | "n/a"
            | "na"
            | "undefined"
            | "unknown"
            | "-"
            | "n.a."
            | "n.a"
    )
}

/// The issue `raw` names. A placeholder is not an issue. There is no book
/// to inherit one from.
#[must_use]
pub fn canonical_issue(raw: Option<&str>, _session: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|id| !blank_issue(id))
        .map(str::to_string)
}

/// No claim book, so a session holds no issue.
#[must_use]
pub fn session_issue(_session: Option<&str>) -> Option<String> {
    None
}

/// No claim book, so nothing is open.
#[must_use]
pub fn open_issue() -> Option<String> {
    None
}

/// Nothing to finish.
pub fn note_finished(_issue: &str) {}

/// The book does not file a claim. The task is the note.
///
/// # Errors
///
/// `task` is empty.
pub fn start_work(_issue: Option<&str>, _repo: Option<&str>, task: &str) -> Result<String, String> {
    let task = task.split_whitespace().collect::<Vec<_>>().join(" ");
    if task.is_empty() {
        return Err("ljos_start: pass the task in one sentence".into());
    }
    log_line(&format!("note: {task}"));
    Ok(format!("noted: {task}"))
}

/// Finishes nothing. Bind it so a drop is still a drop.
pub struct ShutdownFinish;

impl Drop for ShutdownFinish {
    fn drop(&mut self) {}
}

/// Append one line to the seat's `book.log`.
pub fn log_line(line: &str) {
    let dir = super::runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("book.log"))
    else {
        return;
    };
    use std::io::Write;
    let _ = writeln!(file, "{line}");
}

/// The seat name, or this process, for a caller that still asks.
#[must_use]
pub fn seat_session() -> String {
    std::env::var("LJOS_SEAT")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| std::process::id().to_string())
}

/// The issue argument when it is not a placeholder.
///
/// # Errors
///
/// The argument is missing or a placeholder.
pub fn note_issue(raw: Option<&str>) -> Result<String, String> {
    canonical_issue(raw, None).ok_or_else(|| "ljos_note: pass an issue id".into())
}

/// No claims to list.
#[must_use]
pub fn prompt_claims() -> String {
    String::new()
}

/// A commit message does not book an issue.
#[must_use]
pub fn named_open_issue(_message: &str) -> Option<String> {
    None
}

/// A guard ask is not written into a book.
pub fn note_ask(_raw: &str, _call: &super::HookCall, _reason: &str) {}

/// The SessionStart brief is not printed. The book does not keep one.
pub fn print_brief_once() {}

/// Nothing to recall onto a commit.
#[must_use]
pub fn commit_recall_line(_session: &str, _repo: &str, _subject: &str) -> Option<String> {
    None
}

/// One line after `head` lands in `repo`.
#[must_use]
pub fn note_after_commit(repo: &str, head: &str, subject: &str) -> String {
    let subject = subject.split_whitespace().collect::<Vec<_>>().join(" ");
    let short = head.chars().take(12).collect::<String>();
    let line = if subject.is_empty() {
        format!("note: {short} in {repo}")
    } else {
        format!("note: {short} {subject}")
    };
    log_line(&line);
    line
}

/// Whether the guard should stay out of this process.
#[must_use]
pub fn guard_suspended() -> bool {
    ljos_off() || (temp_home().is_some() && !guard_forced_on())
}

/// `LJOS_GUARD` set to one of the on-switches.
#[must_use]
pub fn guard_forced_on() -> bool {
    matches!(
        std::env::var("LJOS_GUARD").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn ljos_off() -> bool {
    matches!(
        std::env::var("LJOS_OFF").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

/// Whether `LJOS_OFF` is set to one of its on-switches.
#[must_use]
pub fn ljos_is_off() -> bool {
    ljos_off()
}

/// `HOME` when it is a temporary directory or sits inside one.
#[must_use]
pub fn temp_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| path_is_temp(Some(home.as_path())))
}

/// Why [`guard_suspended`] holds. `None` when the guard is on.
#[must_use]
pub fn guard_off_reason() -> Option<String> {
    if ljos_off() {
        return Some("off: LJOS_OFF is set".to_string());
    }
    if guard_forced_on() {
        return None;
    }
    temp_home().map(|home| {
        format!(
            "off: HOME ({}) is a temporary directory, and ljos stays out of test sandboxes. \
             `LJOS_GUARD=on` keeps it on",
            home.display()
        )
    })
}

/// Whether `path` is a temporary directory or sits inside one.
#[must_use]
pub fn path_is_temp(path: Option<&Path>) -> bool {
    let Some(path) = path else {
        return false;
    };
    if path.as_os_str().is_empty() {
        return false;
    }
    let path = canonical_existing_prefix(path);
    resolved_path_is_temp(&path)
}

/// `path` with the longest existing prefix resolved, and the missing
/// tail put back. `canonicalize` of a path that does not exist yet
/// returns the spelling unchanged, so `/tmp/scratch` would not match a
/// temp root of `/private/tmp` when `/tmp` is a symlink.
fn canonical_existing_prefix(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Ok(mut real) = std::fs::canonicalize(&cur) {
            for name in missing.iter().rev() {
                real.push(name);
            }
            return real;
        }
        let Some(name) = cur.file_name().map(|name| name.to_os_string()) else {
            return path.to_path_buf();
        };
        missing.push(name);
        if !cur.pop() {
            return path.to_path_buf();
        }
    }
}

/// [`path_is_temp`] for a path already resolved.
#[must_use]
pub fn resolved_path_is_temp(path: &Path) -> bool {
    temp_roots().iter().any(|root| path.starts_with(root))
}

#[cfg(test)]
thread_local! {
    /// Temp roots that stand in for the machine's on one test thread.
    pub(crate) static TEST_TEMP_ROOTS: std::cell::RefCell<Option<Vec<PathBuf>>> =
        const { std::cell::RefCell::new(None) };
}

fn temp_roots() -> Vec<PathBuf> {
    #[cfg(test)]
    if let Some(roots) = TEST_TEMP_ROOTS.with(|r| r.borrow().clone()) {
        return roots;
    }
    let mut roots = vec![
        std::env::temp_dir(),
        PathBuf::from("/tmp"),
        PathBuf::from("/var/tmp"),
        PathBuf::from("/private/tmp"),
        PathBuf::from("/dev/shm"),
    ];
    if let Some(tmp) = std::env::var_os("TMPDIR").filter(|t| !t.is_empty()) {
        roots.push(PathBuf::from(tmp));
    }
    roots
        .into_iter()
        .map(|root| std::fs::canonicalize(&root).unwrap_or(root))
        .collect()
}

/// Hook events are not booked.
#[must_use]
pub fn books(_event: &str) -> bool {
    false
}

/// A hook call writes a note only when it is a commit. Nothing else is booked.
#[must_use]
pub fn observe(raw: &str, call: &super::HookCall) -> (String, Vec<String>) {
    if call.event == "PostToolUse" && call.cue.contains("git commit") {
        let subject = call
            .cue
            .split_once("-m ")
            .map(|(_, rest)| rest.trim().trim_matches('"').to_string())
            .unwrap_or_default();
        let repo = serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|v| v.get("cwd").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        let _ = note_after_commit(&repo, "commit", &subject);
    }
    (String::new(), Vec::new())
}

/// Nothing to recall.
#[must_use]
pub fn recall_for(_raw: &str, _call: &super::HookCall) -> String {
    String::new()
}

pub(crate) type ActingFor = ((u32, u64), Option<String>);

pub(crate) fn act_for(_owner: Option<ActingFor>) {}

pub(crate) fn observe_queued(_raw: &str, _call: &super::HookCall) -> bool {
    false
}

pub(crate) fn session_key(call: &super::HookCall) -> String {
    call.session
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| std::process::id().to_string())
}

pub(crate) fn hook_owner() -> (u32, u64) {
    (0, 0)
}

pub(crate) fn note_ask_within(_raw: &str, _call: &super::HookCall, _reason: &str) -> bool {
    true
}

/// Write `bytes` beside `path` and rename them into place.
pub(crate) fn replace_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut tmp_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "note".into());
    tmp_name.push(format!(".tmp-{}-{stamp}", std::process::id()));
    let tmp = dir.join(tmp_name);
    #[cfg(unix)]
    let mode = existing_mode(path);
    {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut out = options.open(&tmp)?;
        out.write_all(bytes)?;
        out.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

#[cfg(unix)]
fn existing_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.file_type().is_file())
        .map(|meta| meta.permissions().mode() & 0o777)
        .unwrap_or(0o600)
}

/// Read a regular file, at most a megabyte.
pub(crate) fn read_text_bounded(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 1_048_576 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod temp_path_tests {
    use super::*;

    /// A path that is not created yet, under a symlink the temp root
    /// resolves through, is still inside that root.
    #[test]
    fn a_missing_path_under_a_symlinked_temp_is_temporary() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("private/tmp");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.path().join("tmp");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canon = std::fs::canonicalize(&link).unwrap();
        TEST_TEMP_ROOTS.with(|roots| roots.replace(Some(vec![canon.clone()])));
        let missing = link.join("scratch");
        assert!(
            path_is_temp(Some(&missing)),
            "{} against {}",
            missing.display(),
            canon.display()
        );
        TEST_TEMP_ROOTS.with(|roots| roots.replace(None));
    }
}
