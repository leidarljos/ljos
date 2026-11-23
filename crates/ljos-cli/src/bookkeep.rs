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
/// The issue `ljos_start` filed and claimed in this process.
static OPEN_ISSUE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

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
    "ljos_table",
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

/// The issue `raw` names. A placeholder is not an issue. A blank argument
/// is the issue [`open_issue`] holds for this conversation.
#[must_use]
pub fn canonical_issue(raw: Option<&str>, _session: Option<&str>) -> Option<String> {
    if let Some(id) = raw
        .map(str::trim)
        .filter(|id| !blank_issue(id))
        .map(str::to_string)
    {
        return Some(id);
    }
    open_issue()
}

/// The issue this conversation's `ljos_start` opened.
#[must_use]
pub fn session_issue(_session: Option<&str>) -> Option<String> {
    open_issue()
}

/// The issue [`start_work`] filed and claimed, until [`note_finished`].
#[must_use]
pub fn open_issue() -> Option<String> {
    OPEN_ISSUE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Remember `id` as the issue this conversation writes on.
pub fn remember_issue(id: &str) {
    let id = id.trim();
    if id.is_empty() {
        return;
    }
    *OPEN_ISSUE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id.to_string());
}

/// Drop the issue [`start_work`] opened, once that issue is finished.
pub fn note_finished(issue: &str) {
    let mut slot = OPEN_ISSUE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.as_deref() == Some(issue) {
        *slot = None;
    }
}

/// File one issue for `task` when this conversation has none, claim it, and
/// remember it. A later write in the same conversation uses that issue.
///
/// # Errors
///
/// `task` is empty, the named issue is not in the tracker, or the tracker
/// cannot take a new issue.
pub fn start_work(issue: Option<&str>, repo: Option<&str>, task: &str) -> Result<String, String> {
    let task = task.split_whitespace().collect::<Vec<_>>().join(" ");
    if task.is_empty() {
        return Err("ljos_start: pass the task in one sentence".into());
    }
    let (id, fresh) = if let Some(id) = open_issue() {
        (id, false)
    } else if let Some(id) = issue
        .map(str::trim)
        .filter(|id| !blank_issue(id))
        .map(str::to_string)
    {
        if super::tracker_show_json(&id).is_err() {
            return Err(format!("issue {id} not found"));
        }
        (id, true)
    } else {
        (file_task(&task, repo)?, true)
    };
    remember_issue(&id);
    let noted = if fresh {
        String::new()
    } else {
        super::attach_to_issue(&id, &task).unwrap_or_else(|err| format!("note: {err:#}\n"))
    };
    let claimed = match super::claim(&id, &super::holder_name()) {
        Ok(text) => text,
        Err(err) => format!("claim: {err:#}\n"),
    };
    log_line(&format!("start: {id} {task}"));
    Ok(format!("claimed {id}\n{noted}{claimed}"))
}

/// File `task` as one issue in the seat's tracker. The project is the last
/// path component of `repo`, else `work`.
fn file_task(task: &str, repo: Option<&str>) -> Result<String, String> {
    super::tracker_place::pin_root();
    super::tracker_place::aim_tracker()?;
    let layout =
        vissue_core::Layout::resolve(None, None).map_err(|err| format!("ljos_start: {err}"))?;
    if let Some(repo) = super::tracker_place::product_checkout(layout.root()) {
        return Err(super::tracker_place::product_write_refusal(&repo));
    }
    let project = repo
        .map(std::path::Path::new)
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("work")
        .to_string();
    let project = super::project_name(&project)?;
    let dir = layout.projects_dir().join(&project);
    std::fs::create_dir_all(&dir).map_err(|err| format!("ljos_start: {err}"))?;
    let made = vissue_core::ops::create(
        &layout,
        &project,
        task,
        vissue_core::ops::CreateOpts {
            quiet: true,
            issue_type: Some("task"),
            ..vissue_core::ops::CreateOpts::default()
        },
    )
    .map_err(|err| format!("ljos_start: {err}"))?;
    let id = made.trim().to_string();
    if id.is_empty() || !id.contains('-') {
        return Err("ljos_start: filing printed no id".into());
    }
    let _ = super::persist_tracker(&id, "filed");
    Ok(id)
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
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
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

/// The issue argument when it is not a placeholder. A missing or blank
/// argument is the issue this seat holds.
///
/// # Errors
///
/// The argument is a placeholder, or it is blank and this seat holds no issue.
pub fn note_issue(raw: Option<&str>) -> Result<String, String> {
    if let Some(id) = canonical_issue(raw, None) {
        return Ok(id);
    }
    super::held_issue().ok_or_else(|| "ljos_note: pass an issue id".into())
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
        let shown = home.display().to_string();
        format!(
            "off: HOME ({}) is a temporary directory, and ljos stays out of test sandboxes. \
             `LJOS_GUARD=on` keeps it on",
            shown.escape_debug()
        )
    })
}

/// Whether `path` is a temporary directory or sits inside one.
#[must_use]
pub fn path_is_temp(path: Option<&Path>) -> bool {
    path.is_some_and(|path| path_is_temp_in(path, &temp_roots()))
}

/// [`path_is_temp`] against the given temp roots.
fn path_is_temp_in(path: &Path, roots: &[PathBuf]) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    let path = canonical_existing_prefix(path);
    roots.iter().any(|root| path.starts_with(root))
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
                if name == "." {
                    continue;
                }
                if name == ".." {
                    real.pop();
                } else {
                    real.push(name);
                }
            }
            return real;
        }
        let seg = match cur.components().next_back() {
            Some(std::path::Component::Normal(name)) => name.to_os_string(),
            Some(std::path::Component::ParentDir) => std::ffi::OsString::from(".."),
            Some(std::path::Component::CurDir) => std::ffi::OsString::from("."),
            _ => return path.to_path_buf(),
        };
        missing.push(seg);
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

fn per_session_tmpdir(raw: &std::ffi::OsStr, passwd_home: Option<&Path>) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    let candidate = Path::new(raw);
    if !candidate.is_absolute() {
        return None;
    }
    let resolved = canonical_existing_prefix(candidate);
    if resolved.components().count() < 3 {
        return None;
    }
    // Without a passwd home there is no telling a sandbox TMPDIR from one
    // that holds the person's own home, so TMPDIR is not a temp root.
    let pw = canonical_existing_prefix(passwd_home?);
    if pw.starts_with(&resolved) {
        return None;
    }
    Some(resolved)
}

pub(crate) fn temp_roots() -> Vec<PathBuf> {
    #[cfg(test)]
    if let Some(roots) = TEST_TEMP_ROOTS.with(|r| r.borrow().clone()) {
        return roots;
    }
    temp_roots_for(
        std::env::var_os("TMPDIR").as_deref(),
        crate::proc_tree::own_home().as_deref(),
    )
}

/// The temp roots for a `TMPDIR` value and a passwd home.
fn temp_roots_for(tmpdir: Option<&std::ffi::OsStr>, passwd_home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = vec![
        #[cfg(not(unix))]
        std::env::temp_dir(),
        PathBuf::from("/tmp"),
        PathBuf::from("/var/tmp"),
        PathBuf::from("/private/tmp"),
        PathBuf::from("/dev/shm"),
    ];
    if let Some(tmp) = tmpdir.and_then(|t| per_session_tmpdir(t, passwd_home)) {
        roots.push(tmp);
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
        #[expect(
            clippy::disallowed_methods,
            reason = "opens a path that is not a git hook"
        )]
        let mut out = options.open(&tmp)?;
        out.write_all(bytes)?;
        out.sync_all()?;
    }
    #[cfg(unix)]
    {
        crate::change_mode(&tmp, mode & 0o666)?;
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "renames a path that is not a git hook"
    )]
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
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
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

    #[test]
    fn guard_off_reason_escapes_home_newlines() {
        let _env = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home\nrun-this");
        std::fs::create_dir_all(&home).unwrap();
        crate::with_env_mutation(|| unsafe {
            std::env::set_var("HOME", &home);
            std::env::remove_var("LJOS_OFF");
            std::env::remove_var("LJOS_GUARD");
        });
        let reason = guard_off_reason().unwrap();
        assert!(!reason.contains('\n'), "{reason:?}");
        assert!(reason.contains("\\nrun-this"), "{reason}");
    }
}
