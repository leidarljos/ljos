//! The seat keeps the book when the model never calls ljos.
//!
//! A harness hook files a claim on the first edit in a session and
//! repository, cites a deed after a git commit exits 0, and finishes at
//! Stop or the end of the session. The finish writes one lesson from the
//! sitting. `ljos-mcp` does the same on the first write and on shutdown,
//! for a client that has no hooks. A failure is written to the runtime
//! log and never becomes a deny: the agent's tool call goes on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

/// How long one habitat command may run from a hook. A runner that waits
/// on PreToolUse treats a slow hook as a refusal, so the book stays inside
/// this and a warm call does not spawn anything.
const COMMAND_DEADLINE: Duration = Duration::from_secs(2);

/// Unix seconds of the last MCP tool call. Zero means the server has not
/// been touched, and the idle watcher does not finish a session it has
/// not seen.
static LAST_ACTIVITY: AtomicU64 = AtomicU64::new(0);

/// Events that keep the book. A subagent stop and a bare argv line do not:
/// the parent session is the one that claimed.
const BOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "SessionEnd",
];

/// File tools. The first one in a session files the claim. Names are matched
/// without regard to case, so `edit` and `Edit` are the same tool.
const EDIT_TOOLS: &[&str] = &[
    "Edit",
    "Write",
    "MultiEdit",
    "apply_patch",
    "StrReplace",
    "ApplyPatch",
    "write_to_file",
    "replace_file_content",
    "edit_file",
    "NotebookEdit",
];

/// MCP tools that write a habitat. The claim, the sitting and the finish
/// are the bookkeeping verbs themselves, and `ljos_file` opens its own issue.
const MCP_WRITES: &[&str] = &[
    "ljos_remember",
    "ljos_prefer",
    "ljos_forget",
    "ljos_note",
    "ljos_deed",
    "ljos_rule",
    "ljos_vote",
    "ljos_send",
    "ljos_reply",
    "ljos_learn",
    "ljos_trust",
    "ljos_persona",
    "ljos_playbook",
    "ljos_graded",
    "ljos_consolidate",
    "ljos_calibrate",
    "ljos_habit",
];

/// One sitting's book: the prompt the title came from, and one row per
/// repository the sitting has touched. An MCP server spawned per call
/// keeps this under the seat, so the next process sees the open claim.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct Book {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    repos: BTreeMap<String, RepoBook>,
    /// Unix seconds of the last MCP call for this sitting. Zero means the
    /// sitting has not been touched, and an idle check does not finish it.
    #[serde(default)]
    last_unix: u64,
    /// `cwd` as the hook named it, to the repository key. The warm path is
    /// a lookup, not another `git rev-parse`.
    #[serde(default)]
    cwd_repo: BTreeMap<String, String>,
}

/// What this session has already recorded for one repository.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct RepoBook {
    #[serde(default)]
    issue: Option<String>,
    /// Commit shas already cited. The id is `deed-file-commit-<sha>`, so a
    /// second commit is a second deed, and the same sha is not cited twice.
    #[serde(default)]
    deeds: Vec<String>,
    /// Short notes from tool failures, for the lesson written at Stop.
    #[serde(default)]
    failures: Vec<String>,
    /// Short notes from guard asks, for the same lesson.
    #[serde(default)]
    asks: Vec<String>,
    /// The sitting's lesson was written. Stop and SessionEnd share one.
    #[serde(default)]
    lesson: bool,
    #[serde(default)]
    finished: bool,
    /// `vissue` failed once. Later edits stay on the warm path instead of
    /// asking again on every tool call.
    #[serde(default)]
    claim_failed: bool,
    #[serde(default)]
    recall: String,
}

/// Whether the guard and the pack daemon should stay out of this process.
/// A temp sandbox and `LJOS_OFF` are not seats: rtrash's tests delete
/// branches in a temp repository, and a daemon must not write under a temp
/// `HOME`. The book is not suspended here. A hook in a scratch directory
/// still files its claim.
#[must_use]
pub fn guard_suspended() -> bool {
    ljos_off() || home_or_cwd_is_temp()
}

/// `LJOS_OFF` set to one of the on-switches.
fn ljos_off() -> bool {
    matches!(
        std::env::var("LJOS_OFF").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn home_or_cwd_is_temp() -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let cwd = std::env::current_dir().ok();
    path_is_temp(home.as_deref()) || path_is_temp(cwd.as_deref())
}

/// Whether `path` is a temporary directory or sits inside one.
/// A missing path is judged as written, so `/tmpfoo` is not `/tmp`.
#[must_use]
pub fn path_is_temp(path: Option<&Path>) -> bool {
    let Some(path) = path else {
        return false;
    };
    if path.as_os_str().is_empty() {
        return false;
    }
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    temp_roots().iter().any(|root| path.starts_with(root))
}

fn temp_roots() -> Vec<PathBuf> {
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

/// Keep the book for this hook call. The string is recall to inject, and
/// only on session start and on a prompt. Empty when there is nothing to
/// say. Never an error: a habitat that did not answer is a line in the
/// book log.
#[must_use]
pub fn observe(raw: &str, call: &super::HookCall) -> (String, Vec<String>) {
    let raw = raw.to_string();
    let call = call.clone();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observe_inner(&raw, &call))) {
        Ok(text) => (text, Vec::new()),
        Err(_) => {
            log_line("bookkeep panicked");
            (String::new(), Vec::new())
        }
    }
}

fn observe_inner(raw: &str, call: &super::HookCall) -> String {
    if !BOOK_EVENTS.contains(&call.event.as_str()) {
        return String::new();
    }
    let value: Value = serde_json::from_str(raw.trim()).unwrap_or(Value::Null);
    let session = session_key(call);
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let path = book_path(&session);
    let mut book = load_book(&path);
    if call.event == "UserPromptSubmit" {
        let prompt = collapse(&call.cue);
        if !prompt.is_empty() {
            book.prompt = prompt.chars().take(4000).collect();
        }
    }
    let repo = repo_key(&mut book, &value);
    if call.event == "PostToolUseFailure" {
        note_failure(&mut book, &repo, &failure_text(&value, &call.cue));
    }
    if matches!(call.event.as_str(), "PreToolUse" | "PostToolUse") {
        let commit = is_git_commit(&call.cue);
        // A write tool claims even for a new file. A shell redirect claims
        // when it writes a file this repository already tracks. A commit
        // claims too, so a sitting that only commits still has a ticket.
        if is_edit(&value, &call.cue) || commit || writes_tracked(&repo, &call.cue) {
            let _ = ensure_claim(&mut book, &repo);
        }
        // The commit has not happened on the way in. Cite it on the way
        // out, and only when it exited 0, so a failed commit leaves no deed
        // and the sha is the one git just wrote.
        if call.event == "PostToolUse" && commit && tool_exit_ok(&value) {
            let _ = ensure_deed(&mut book, &repo);
        } else if call.event == "PostToolUse" && commit {
            note_failure(&mut book, &repo, "git commit exited non-zero");
        }
    }
    if matches!(call.event.as_str(), "Stop" | "SessionEnd") {
        finish_open(&mut book);
    }
    let recall = if matches!(call.event.as_str(), "SessionStart" | "UserPromptSubmit") {
        book.repos
            .get(&repo)
            .map(|row| row.recall.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };
    store_book(&path, &book);
    recall
}

/// Whether this MCP tool writes a habitat, so the server claims before it runs.
#[must_use]
pub fn mcp_tool_writes(name: &str) -> bool {
    MCP_WRITES.contains(&name)
}

/// File the session's claim before a write tool runs. The title comes from
/// the arguments when the session has not heard a prompt yet.
pub fn before_mcp_write(name: &str, args: Option<&serde_json::Map<String, Value>>) {
    if !mcp_tool_writes(name) {
        return;
    }
    let session = sitting_key();
    if let Some(title) = args.and_then(arg_title) {
        let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
        let path = book_path(&session);
        let mut book = load_book(&path);
        if book.prompt.trim().is_empty() {
            book.prompt = collapse(&title).chars().take(4000).collect();
        }
        store_book(&path, &book);
    }
    // A hook's warm edit after Stop stays warm. An MCP write after the
    // idle close files a new claim: the finished row is this sitting, and
    // the next write is the next one.
    reopen_finished(&session);
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let raw = serde_json::json!({
        "cwd": cwd,
        "session_id": session,
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
    })
    .to_string();
    let call = super::HookCall {
        event: "PreToolUse".into(),
        cue: "Write note".into(),
        session: Some(session.clone()),
        shape: super::HookShape::Asks,
    };
    let _ = observe(&raw, &call);
    stamp_sitting(&session, unix_secs());
}

/// Finish every claim this MCP process opened. Called when the server exits.
pub fn mcp_shutdown() {
    let session = sitting_key();
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let raw = serde_json::json!({
        "cwd": cwd,
        "session_id": session,
        "hook_event_name": "SessionEnd",
    })
    .to_string();
    let call = super::HookCall {
        event: "SessionEnd".into(),
        cue: String::new(),
        session: Some(session),
        shape: super::HookShape::Asks,
    };
    let _ = observe(&raw, &call);
    match super::sync::sync_repo_in_passing(false, true) {
        Ok(line) if !line.trim().is_empty() => log_line(line.trim()),
        Err(e) => log_line(&format!("sync: {e:#}")),
        Ok(_) => {}
    }
}

/// Note that a tool ran, so the idle watcher waits another window.
pub fn touch_activity() {
    LAST_ACTIVITY.store(unix_secs(), Ordering::Relaxed);
}

/// Tool calls this process has answered. A process that answered one call
/// and exited was spawned per call: finishing in `Drop` would close the
/// sitting before the next call. The book stays, and the next start
/// finishes it once the idle window has passed.
static MCP_CALLS: AtomicU64 = AtomicU64::new(0);

/// Count one tool call in this process, for [`ShutdownFinish`].
pub fn note_mcp_process_call() {
    MCP_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// Finish the seat's open claims when the last call is older than the
/// idle window, then stamp this call. A per-call server has no thread
/// left alive to notice the idle; the next process does.
pub fn resume_seat() {
    resume_seat_at(unix_secs());
}

fn resume_seat_at(now: u64) {
    let key = sitting_key();
    if let Some(limit) = idle_limit() {
        let book = load_book(&book_path(&key));
        let open = book
            .repos
            .values()
            .any(|row| row.issue.is_some() && !row.finished);
        if open && idle_due(now, book.last_unix, limit) {
            mcp_shutdown();
        }
    }
    stamp_sitting(&key, now);
}

fn stamp_sitting(key: &str, now: u64) {
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let path = book_path(key);
    let mut book = load_book(&path);
    book.last_unix = now;
    store_book(&path, &book);
}

/// How long a silent MCP server sits before it finishes. Unset is thirty
/// minutes. `0` and `off` leave the session open until the process exits.
#[must_use]
pub fn idle_limit() -> Option<Duration> {
    idle_limit_from(std::env::var("LJOS_MCP_IDLE").ok().as_deref())
}

/// [`idle_limit`] from an already-read value. `None` is the default window.
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

/// Whether `now` is at least `limit` past `last`. A `last` of zero has
/// never been touched, so it is not idle.
#[must_use]
pub fn idle_due(now: u64, last: u64, limit: Duration) -> bool {
    last > 0 && now.saturating_sub(last) >= limit.as_secs()
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Finish the open claims when no tool has run for [`idle_limit`]. One
/// thread for the process. A disabled window starts nothing.
pub fn spawn_idle_finish() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let Some(limit) = idle_limit() else {
            return;
        };
        touch_activity();
        let _ = std::thread::Builder::new()
            .name("ljos-mcp-idle".into())
            .spawn(move || {
                let tick = Duration::from_secs(30.min(limit.as_secs().max(1)));
                loop {
                    std::thread::sleep(tick);
                    let last = LAST_ACTIVITY.load(Ordering::Relaxed);
                    if idle_due(unix_secs(), last, limit) {
                        mcp_shutdown();
                        touch_activity();
                    }
                }
            });
    });
}

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

/// The issue `raw` names, or the one this session claimed when `raw` is
/// missing or a placeholder such as `none`.
#[must_use]
pub fn canonical_issue(raw: Option<&str>, session: Option<&str>) -> Option<String> {
    if let Some(id) = raw.map(str::trim).filter(|id| !blank_issue(id)) {
        return Some(id.to_string());
    }
    claimed_issue(session)
}

/// The unfinished issue this conversation's book holds. A new session does
/// not inherit another conversation's ticket.
#[must_use]
pub fn session_issue(session: Option<&str>) -> Option<String> {
    issue_in_sessions(session)
}

/// The issue to use when a tool did not name one: this session's claim,
/// the MCP sitting, or the newest unfinished claim in this directory.
#[must_use]
pub fn claimed_issue(session: Option<&str>) -> Option<String> {
    issue_in_sessions(session)
        .or_else(open_issue)
        .or_else(latest_cwd_issue)
}

fn issue_in_sessions(session: Option<&str>) -> Option<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut keys = Vec::new();
    let mut push = |raw: String| {
        let key = raw.trim().to_string();
        if key.is_empty() || !seen.insert(key.clone()) {
            return;
        }
        keys.push(key);
    };
    if let Some(session) = session {
        push(session.to_string());
    }
    if let Some(session) = crate::ballot_sig::call_session() {
        push(session);
    }
    if let Ok(session) = std::env::var("LJOS_HARNESS_SESSION") {
        push(session);
    }
    if let Ok(session) = std::env::var("LJOS_SESSION_ID") {
        push(session);
    }
    if let Some((id, _)) = super::session_actor() {
        push(id);
    }
    for key in &keys {
        if let Some(issue) = open_in_book(&load_book(&book_path(key))) {
            return Some(issue);
        }
    }
    None
}

fn open_in_book(book: &Book) -> Option<String> {
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    if let Some(key) = book.cwd_repo.get(&cwd) {
        if let Some(issue) = unfinished(book, key) {
            return Some(issue);
        }
    }
    let mut found = None;
    for key in book.repos.keys() {
        if let Some(issue) = unfinished(book, key) {
            if found.is_some() {
                return None;
            }
            found = Some(issue);
        }
    }
    found
}

/// The newest unfinished claim whose book names this working directory.
fn latest_cwd_issue() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let dir = super::runtime_dir();
    let mut hits = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("book-") || !name.ends_with(".json") {
            continue;
        }
        let book = load_book(&entry.path());
        let Some(issue) = issue_for_cwd(&book, &cwd) else {
            continue;
        };
        let when = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        hits.push((when, issue));
    }
    hits.sort_by_key(|hit| hit.0);
    hits.pop().map(|hit| hit.1)
}

fn issue_for_cwd(book: &Book, cwd: &Path) -> Option<String> {
    let cwd_s = cwd.display().to_string();
    if let Some(key) = book.cwd_repo.get(&cwd_s) {
        if let Some(issue) = unfinished(book, key) {
            return Some(issue);
        }
    }
    for (dir, key) in &book.cwd_repo {
        if paths_match(dir, cwd) {
            if let Some(issue) = unfinished(book, key) {
                return Some(issue);
            }
        }
    }
    for key in book.repos.keys() {
        if paths_match(key, cwd) {
            if let Some(issue) = unfinished(book, key) {
                return Some(issue);
            }
        }
    }
    None
}

fn paths_match(raw: &str, cwd: &Path) -> bool {
    let path = Path::new(raw);
    if path == cwd {
        return true;
    }
    match (
        std::fs::canonicalize(path).ok(),
        std::fs::canonicalize(cwd).ok(),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// The issue this MCP sitting still holds, for a finish that did not name one.
#[must_use]
pub fn open_issue() -> Option<String> {
    let book = load_book(&book_path(&sitting_key()));
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    if let Some(key) = book.cwd_repo.get(&cwd) {
        if let Some(issue) = unfinished(&book, key) {
            return Some(issue);
        }
    }
    book.repos
        .iter()
        .find_map(|(key, _)| unfinished(&book, key))
}

fn unfinished(book: &Book, key: &str) -> Option<String> {
    let row = book.repos.get(key)?;
    if row.finished {
        return None;
    }
    row.issue.clone()
}

/// The explicit finish closed `issue`. Shutdown then has nothing left to close.
pub fn note_finished(issue: &str) {
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let path = book_path(&sitting_key());
    let mut book = load_book(&path);
    let mut changed = false;
    for row in book.repos.values_mut() {
        if row.issue.as_deref() == Some(issue) && !row.finished {
            row.finished = true;
            changed = true;
        }
    }
    if changed {
        store_book(&path, &book);
    }
}

/// Recall and claim before repo work. `issue` claims that ticket. Without
/// it, `task` is filed under `repo` (or the working directory) and claimed.
/// The issue is the one shutdown and an issue-less finish close.
///
/// # Errors
///
/// `task` is empty, or the tracker did not file or claim.
pub fn start_work(issue: Option<&str>, repo: Option<&str>, task: &str) -> Result<String, String> {
    let task = collapse(task);
    if task.is_empty() {
        return Err("ljos_start: pass the task in one sentence".into());
    }
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let place = repo
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .unwrap_or(cwd.as_str())
        .to_string();
    let issue_id = match issue.map(str::trim).filter(|id| !blank_issue(id)) {
        Some(id) => {
            claim_existing(id)?;
            id.to_string()
        }
        None => file_and_claim(&project_for(&place), &task)?,
    };
    let persisted = super::persist_tracker(&issue_id, "claimed");
    if persisted.contains("could not") || persisted.contains("refused") {
        log_line(persisted.trim());
    }
    let recall = format!(
        "{}{}",
        recall_line(&issue_id),
        super::recall_memories(&issue_id)
    );
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let path = book_path(&sitting_key());
    let mut book = load_book(&path);
    book.last_unix = unix_secs();
    if book.prompt.trim().is_empty() {
        book.prompt = task.chars().take(4000).collect();
    }
    let row = book.repos.entry(place.clone()).or_default();
    row.issue = Some(issue_id.clone());
    row.finished = false;
    row.claim_failed = false;
    row.recall.clone_from(&recall);
    if !cwd.is_empty() {
        book.cwd_repo.insert(cwd, place);
    }
    store_book(&path, &book);
    drop(_held);
    Ok(format!("claimed {issue_id}\n{recall}"))
}

fn claim_existing(issue: &str) -> Result<(), String> {
    let assignee = super::resolve_assignee(None);
    if which::which("claimdag").is_ok() && super::claim(issue, &assignee).is_ok() {
        return Ok(());
    }
    run_timed(
        "vissue",
        &["claim", issue],
        &[("VISSUE_AGENT", assignee.as_str())],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn reopen_finished(session: &str) {
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let raw = serde_json::json!({ "cwd": cwd });
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let path = book_path(session);
    let mut book = load_book(&path);
    let repo = repo_key(&mut book, &raw);
    if book.repos.get(&repo).is_some_and(|row| row.finished) {
        book.repos.insert(repo, RepoBook::default());
    }
    store_book(&path, &book);
}

/// Finishes the MCP session's claims when the server process returns.
/// Bind it to a name. `let _ = ShutdownFinish` drops it at that statement.
pub struct ShutdownFinish;

impl Drop for ShutdownFinish {
    fn drop(&mut self) {
        // One call, then the process is gone: this was spawned per call.
        // The sitting stays on disk under the seat. The next start finishes
        // it once the idle window has passed. A server that answered more
        // than one call is the long-lived process, and its exit closes.
        if MCP_CALLS.load(Ordering::Relaxed) > 1 {
            mcp_shutdown();
        }
    }
}

/// The book key. An MCP call is keyed by the seat, so a server spawned
/// once per tool call keeps the claim and the idle clock. A hook, and an
/// MCP process with no seat yet, stay on the session id.
fn sitting_key() -> String {
    if let Some(name) = std::env::var("LJOS_SEAT")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
    {
        let slug = super::seat_slug(&name);
        if slug != "runner" && !super::omitted_actor_name(&slug) {
            return format!("seat:{slug}");
        }
    }
    mcp_session()
}

fn mcp_session() -> String {
    std::env::var("LJOS_SESSION_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("mcp-{}", std::process::id()))
}

fn arg_title(args: &serde_json::Map<String, Value>) -> Option<String> {
    for key in ["title", "text", "query", "body", "note"] {
        if let Some(text) = args.get(key).and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn session_key(call: &super::HookCall) -> String {
    call.session
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("LJOS_SESSION_ID")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| std::process::id().to_string())
}

fn book_path(session: &str) -> PathBuf {
    super::runtime_dir().join(format!("book-{}.json", super::session_tag(session)))
}

fn load_book(path: &Path) -> Book {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn store_book(path: &Path, book: &Book) {
    let Ok(text) = serde_json::to_string(book) else {
        return;
    };
    if std::fs::read_to_string(path).is_ok_and(|prev| prev == text) {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, text) {
        log_line(&format!("book: {e}"));
    }
}

pub(crate) fn log_line(line: &str) {
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

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The repository this call is about: `git rev-parse` once per cwd, then
/// the canonical directory when this cwd is not a checkout.
fn repo_key(book: &mut Book, raw: &Value) -> String {
    let cwd = raw["cwd"]
        .as_str()
        .or_else(|| raw["workspacePaths"][0].as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_default();
    if let Some(hit) = book.cwd_repo.get(&cwd) {
        return hit.clone();
    }
    let key = git_toplevel(Path::new(&cwd)).unwrap_or_else(|| {
        let path = PathBuf::from(&cwd);
        std::fs::canonicalize(&path)
            .unwrap_or(path)
            .display()
            .to_string()
    });
    if !cwd.is_empty() {
        book.cwd_repo.insert(cwd, key.clone());
    }
    key
}

fn git_toplevel(cwd: &Path) -> Option<String> {
    if cwd.as_os_str().is_empty() {
        return None;
    }
    let said = run_timed(
        "git",
        &[
            "-C",
            &cwd.display().to_string(),
            "rev-parse",
            "--show-toplevel",
        ],
        &[],
    )
    .ok()?;
    let top = said.stdout.trim();
    (!top.is_empty()).then(|| top.to_string())
}

fn ensure_claim(book: &mut Book, repo: &str) -> Result<(), String> {
    let row = book.repos.entry(repo.to_string()).or_default();
    if row.claim_failed {
        return Ok(());
    }
    if let Some(issue) = row.issue.clone() {
        if row.recall.is_empty() {
            row.recall = recall_line(&issue);
        }
        return Ok(());
    }
    // A hold from another repository is a different ticket. This sitting
    // files its own claim for the repository the tool just wrote.
    let title = title_from(&book.prompt, repo);
    let project = project_for(repo);
    match file_and_claim(&project, &title) {
        Ok(issue) => {
            let line = super::persist_tracker(&issue, "claimed");
            if line.contains("could not") || line.contains("refused") {
                log_line(line.trim());
            }
            let row = book.repos.entry(repo.to_string()).or_default();
            row.issue = Some(issue.clone());
            row.recall = recall_line(&issue);
            Ok(())
        }
        Err(e) => {
            log_line(&format!("claim: {e}"));
            book.repos.entry(repo.to_string()).or_default().claim_failed = true;
            Ok(())
        }
    }
}

fn file_and_claim(project: &str, title: &str) -> Result<String, String> {
    let said = run_timed("vissue", &["create", "-p", project, title, "-q"], &[])
        .map_err(|e| e.to_string())?;
    let issue = parse_issue_id(&said.stdout)
        .ok_or_else(|| format!("vissue create printed no issue id: {}", said.stdout.trim()))?;
    let assignee = super::resolve_assignee(None);
    if which::which("claimdag").is_ok() {
        if let Err(e) = super::claim(&issue, &assignee) {
            log_line(&format!("claim graph: {e:#}"));
            run_timed(
                "vissue",
                &["claim", &issue],
                &[("VISSUE_AGENT", assignee.as_str())],
            )
            .map_err(|e| e.to_string())?;
        }
    } else {
        run_timed(
            "vissue",
            &["claim", &issue],
            &[("VISSUE_AGENT", assignee.as_str())],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(issue)
}

fn ensure_deed(book: &mut Book, repo: &str) -> Result<(), String> {
    let sha = match head_sha(repo) {
        Ok(sha) => sha,
        Err(e) => {
            log_line(&format!("deed: {e}"));
            return Ok(());
        }
    };
    let row = book.repos.entry(repo.to_string()).or_default();
    if row.deeds.iter().any(|seen| seen == &sha) {
        return Ok(());
    }
    let Some(issue) = row.issue.clone() else {
        return Ok(());
    };
    match cite_deed(&issue, &sha) {
        Ok(()) => {
            book.repos
                .entry(repo.to_string())
                .or_default()
                .deeds
                .push(sha);
            Ok(())
        }
        Err(e) => {
            log_line(&format!("deed: {e}"));
            Ok(())
        }
    }
}

/// A file deed's accession has to start with `deed-file-`. The sha keeps
/// each commit's id distinct; `deed-file-commit` alone is one frozen deed.
fn deed_id(sha: &str) -> String {
    format!("deed-file-commit-{sha}")
}

fn cite_deed(issue: &str, sha: &str) -> Result<(), String> {
    let dir = super::runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("deed-{sha}.txt"));
    std::fs::write(&path, format!("{sha}\n")).map_err(|e| e.to_string())?;
    let seat = super::holder_name();
    let id = deed_id(sha);
    let said = run_timed(
        "deedar",
        &[
            "create",
            "file",
            "--id",
            &id,
            "--name",
            sha,
            "--path",
            &path.display().to_string(),
            "--agent",
            &seat,
        ],
        &[],
    )
    .map_err(|e| e.to_string())?;
    let accession = parse_deed(&said.stdout)
        .ok_or_else(|| format!("deedar printed no accession: {}", said.stdout.trim()))?;
    run_timed("vissue", &["deed", issue, "--add", &accession], &[]).map_err(|e| e.to_string())?;
    let line = super::persist_tracker(issue, "cited a deed");
    if line.contains("could not") || line.contains("refused") {
        return Err(line.trim().to_string());
    }
    Ok(())
}

fn head_sha(repo: &str) -> Result<String, String> {
    let said =
        run_timed("git", &["-C", repo, "rev-parse", "HEAD"], &[]).map_err(|e| e.to_string())?;
    let sha = said.stdout.trim().to_string();
    if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("HEAD is not a sha: {sha}"));
    }
    Ok(sha)
}

fn finish_open(book: &mut Book) {
    let repos: Vec<String> = book.repos.keys().cloned().collect();
    for repo in repos {
        let Some(row) = book.repos.get(&repo) else {
            continue;
        };
        if row.finished {
            continue;
        }
        let Some(issue) = row.issue.clone() else {
            continue;
        };
        if !row.lesson {
            let title = title_from(&book.prompt, &repo);
            let deeds = row.deeds.clone();
            let failures = row.failures.clone();
            let asks = row.asks.clone();
            match write_lesson(&issue, &title, &deeds, &failures, &asks) {
                Ok(()) => {
                    if let Some(row) = book.repos.get_mut(&repo) {
                        row.lesson = true;
                    }
                }
                Err(e) => log_line(&format!("lesson: {e}")),
            }
        }
        match close_issue(&issue) {
            Ok(()) => {
                if let Some(row) = book.repos.get_mut(&repo) {
                    row.finished = true;
                }
            }
            Err(e) => log_line(&format!("finish: {e}")),
        }
    }
}

/// The ticket is done, the claim is released, the tracker commit says
/// finished. The lesson was written just before this, from the sitting.
fn close_issue(issue: &str) -> Result<(), String> {
    run_timed("vissue", &["update", issue, "-s", "DONE"], &[]).map_err(|e| e.to_string())?;
    let assignee = super::resolve_assignee(None);
    if which::which("claimdag").is_ok() {
        if let Err(e) = super::release(issue, &assignee) {
            log_line(&format!("release: {e:#}"));
        }
    }
    let line = super::persist_tracker(issue, "finished");
    if line.contains("could not") || line.contains("refused") {
        return Err(line.trim().to_string());
    }
    Ok(())
}

/// Remember a guard ask on this repository. The book file is the only
/// write: a slow tracker must not sit on the path that is about to ask.
pub fn note_ask(raw: &str, call: &super::HookCall, reason: &str) {
    if !matches!(call.event.as_str(), "PreToolUse" | "argv") {
        return;
    }
    let reason = collapse(reason);
    if reason.is_empty() {
        return;
    }
    let value: Value = serde_json::from_str(raw.trim()).unwrap_or(Value::Null);
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let path = book_path(&session_key(call));
    let mut book = load_book(&path);
    let repo = repo_key(&mut book, &value);
    let row = book.repos.entry(repo).or_default();
    let short: String = reason.chars().take(160).collect();
    if row.asks.len() < 4 && !row.asks.iter().any(|seen| seen == &short) {
        row.asks.push(short);
    }
    store_book(&path, &book);
}

fn note_failure(book: &mut Book, repo: &str, text: &str) {
    let text = collapse(text);
    if text.is_empty() {
        return;
    }
    let short: String = text.chars().take(160).collect();
    let row = book.repos.entry(repo.to_string()).or_default();
    if row.failures.len() < 4 && !row.failures.iter().any(|seen| seen == &short) {
        row.failures.push(short);
    }
}

fn failure_text(raw: &Value, cue: &str) -> String {
    let err = raw
        .pointer("/tool_response/error")
        .and_then(Value::as_str)
        .or_else(|| raw.get("error").and_then(Value::as_str))
        .or_else(|| raw.get("error_message").and_then(Value::as_str))
        .unwrap_or("")
        .trim();
    if !err.is_empty() {
        return err.to_string();
    }
    if !cue.trim().is_empty() {
        return cue.trim().to_string();
    }
    "a tool failed".to_string()
}

/// One standing lesson, agent-derived, with the writer in the atom source.
/// Two short sentences: the ticket and its commits, then a failure or an
/// ask when the sitting had one. A pack that is off is an error here; the
/// caller logs it and still closes the ticket.
fn write_lesson(
    issue: &str,
    title: &str,
    shas: &[String],
    failures: &[String],
    asks: &[String],
) -> Result<(), String> {
    let text = lesson_text(title, issue, shas, failures, asks);
    super::packset_write_agent("Remember", &text, None, Some(false))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// At most two sentences, each under thirty words. The pack refuses a
/// third sentence and a sentence of thirty words or more.
fn lesson_text(
    title: &str,
    issue: &str,
    shas: &[String],
    failures: &[String],
    asks: &[String],
) -> String {
    let title = clip_words(&title.replace(['.', '!', '?'], " "), 8);
    let title = if title.is_empty() {
        "agent work".to_string()
    } else {
        title
    };
    let shas: Vec<String> = shas
        .iter()
        .take(3)
        .map(|sha| sha.chars().take(7).collect())
        .collect();
    let first = if shas.is_empty() {
        format!("{title} ({issue}) closed with no commit.")
    } else {
        format!("{title} ({issue}) committed {}.", shas.join(" "))
    };
    let fail = failures.first().map(|text| clip_words(text, 6));
    let ask = asks.first().map(|text| clip_words(text, 6));
    let second = match (fail, ask) {
        (Some(fail), Some(ask)) => {
            format!("A tool failed ({fail}) and the guard asked ({ask}).")
        }
        (Some(fail), None) => format!("A tool failed: {fail}."),
        (None, Some(ask)) => format!("The guard asked: {ask}."),
        (None, None) => String::new(),
    };
    let first = fit_sentence(&first);
    if second.is_empty() {
        first
    } else {
        format!("{first} {}", fit_sentence(&second))
    }
}

fn clip_words(text: &str, n: usize) -> String {
    text.split_whitespace()
        .take(n)
        .collect::<Vec<_>>()
        .join(" ")
}

fn fit_sentence(text: &str) -> String {
    let mut words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= 29 {
        return text.to_string();
    }
    words.truncate(28);
    let mut out = words.join(" ");
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

fn title_from(prompt: &str, repo: &str) -> String {
    let words: String = prompt
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");
    let title: String = words.chars().take(80).collect();
    if !title.is_empty() {
        return title;
    }
    Path::new(repo)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(|name| format!("Work in {name}"))
        .unwrap_or_else(|| "agent work".to_string())
}

fn project_for(repo: &str) -> String {
    if let Some(project) = std::env::var("LJOS_PROJECT")
        .ok()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
    {
        return project;
    }
    if let Some(project) = sole_project() {
        return project;
    }
    Path::new(repo)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("agent")
        .to_string()
}

/// The project directory when the tracker has exactly one.
fn sole_project() -> Option<String> {
    let root = std::env::var_os("VISSUE_ROOT")?;
    let root = PathBuf::from(root);
    let mut found = Vec::new();
    let mut consider = |dir: PathBuf| {
        if dir.join("issues.org").is_file() {
            if let Some(name) = dir.file_name().and_then(|s| s.to_str()) {
                found.push(name.to_string());
            }
        }
    };
    let entries = std::fs::read_dir(&root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == "Software" {
            if let Ok(children) = std::fs::read_dir(&path) {
                for child in children.flatten() {
                    consider(child.path());
                }
            }
        } else {
            consider(path);
        }
    }
    found.sort();
    found.dedup();
    (found.len() == 1).then(|| found.remove(0))
}

/// Recall for the claim this session filed. The pack is not searched:
/// that walk is what makes a cold prompt hook slow, and an issue with no
/// memories would then add nothing. The title is the working set the hook
/// can name without the pack.
fn recall_line(issue: &str) -> String {
    let head = format!("Recall for {issue} (`ljos recall {issue}` shows its full working set):");
    match super::tracker_show_json(issue) {
        Ok(card) => match card.get("title").and_then(Value::as_str) {
            Some(title) if !title.trim().is_empty() => format!("{head}\n{title}"),
            _ => head,
        },
        Err(_) => head,
    }
}

/// A post-tool result whose command finished cleanly. A missing exit on
/// PostToolUse is success. An error string, a non-zero exit, or a guard
/// refusal (`deny` or `ask`) is not, so a commit the guard stopped does
/// not cite the previous HEAD.
fn tool_exit_ok(raw: &Value) -> bool {
    if raw
        .pointer("/tool_response/interrupted")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return false;
    }
    if refusal_signal(raw) {
        return false;
    }
    let slots = [
        raw.get("exit_code"),
        raw.get("exit"),
        raw.pointer("/tool_response/exit_code"),
        raw.pointer("/tool_response/exit"),
        raw.pointer("/tool_response/exitCode"),
        raw.pointer("/tool_output/exit_code"),
    ];
    for slot in slots.into_iter().flatten() {
        if let Some(code) = slot.as_i64() {
            return code == 0;
        }
        if let Some(code) = slot.as_str() {
            return code.trim() == "0";
        }
    }
    raw.pointer("/tool_response/error")
        .and_then(Value::as_str)
        .is_none_or(|text| text.trim().is_empty())
}

/// A guard or a tool that already refused this call. A missing exit is
/// not one of these: silent PostToolUse stays a success.
fn refusal_signal(raw: &Value) -> bool {
    let decisions = [
        raw.get("permissionDecision"),
        raw.get("permission_decision"),
        raw.pointer("/hookSpecificOutput/permissionDecision"),
        raw.pointer("/tool_response/permissionDecision"),
        raw.get("decision"),
    ];
    for slot in decisions.into_iter().flatten() {
        if let Some(word) = slot.as_str() {
            if matches!(word.trim(), "deny" | "ask" | "block") {
                return true;
            }
        }
    }
    if raw.get("is_error").and_then(Value::as_bool) == Some(true)
        || raw.get("isError").and_then(Value::as_bool) == Some(true)
    {
        return true;
    }
    let errors = [
        raw.get("error"),
        raw.get("error_message"),
        raw.pointer("/tool_response/error"),
    ];
    errors.into_iter().flatten().any(|slot| {
        slot.as_str().is_some_and(|text| !text.trim().is_empty()) || slot.as_bool() == Some(true)
    })
}

/// `>` and `>>` targets on the line, not `2>&1` and not a quoted sentence.
fn redirect_targets(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for seg in super::nested_lines(line, 0)
        .iter()
        .flat_map(|l| super::raw_segments(l))
    {
        let words = super::shell_words(&seg);
        let mut i = 0;
        while i < words.len() {
            let word = words[i].as_str();
            if word == ">" || word == ">>" {
                if let Some(target) = words.get(i + 1) {
                    if let Some(target) = redirect_path(target) {
                        out.push(target);
                    }
                }
                i += 2;
                continue;
            }
            if let Some(target) = glued_redirect(word) {
                out.push(target);
            }
            i += 1;
        }
    }
    out
}

fn redirect_path(word: &str) -> Option<String> {
    let word = word.trim_matches(|c| c == '"' || c == '\'');
    if word.is_empty() || word.starts_with('&') {
        return None;
    }
    Some(word.to_string())
}

fn glued_redirect(word: &str) -> Option<String> {
    let rest = word.trim_start_matches(|c: char| c.is_ascii_digit());
    let body = rest
        .strip_prefix(">>")
        .or_else(|| rest.strip_prefix("&>"))
        .or_else(|| rest.strip_prefix('>'))?;
    redirect_path(body)
}

fn writes_tracked(repo: &str, line: &str) -> bool {
    redirect_targets(line)
        .into_iter()
        .any(|target| file_is_tracked(repo, &target))
}

fn file_is_tracked(repo: &str, target: &str) -> bool {
    let path = Path::new(target);
    let relative = if path.is_absolute() {
        path.strip_prefix(repo)
            .ok()
            .map(|p| p.display().to_string())
    } else {
        Some(target.trim_start_matches("./").to_string())
    };
    let Some(relative) = relative.filter(|p| !p.is_empty()) else {
        return false;
    };
    run_timed(
        "git",
        &["-C", repo, "ls-files", "--error-unmatch", "--", &relative],
        &[],
    )
    .is_ok()
}

fn is_edit(raw: &Value, cue: &str) -> bool {
    if edit_name(&tool_name(raw)) || edit_name(cue.split_whitespace().next().unwrap_or("")) {
        return true;
    }
    command_is_apply_patch(cue)
}

fn tool_name(raw: &Value) -> String {
    raw["tool_name"]
        .as_str()
        .or_else(|| raw["toolName"].as_str())
        .or_else(|| raw["toolCall"]["name"].as_str())
        .unwrap_or("")
        .to_string()
}

fn edit_name(name: &str) -> bool {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    EDIT_TOOLS
        .iter()
        .any(|tool| tool.eq_ignore_ascii_case(base))
}

fn command_is_apply_patch(line: &str) -> bool {
    super::nested_lines(line, 0)
        .iter()
        .flat_map(|l| super::raw_segments(l))
        .any(|seg| {
            let words = super::shell_words(&seg);
            let (at, _) = super::prefix_end(&words);
            words.get(at).is_some_and(|word| {
                word.rsplit('/')
                    .next()
                    .unwrap_or(word)
                    .eq_ignore_ascii_case("apply_patch")
            })
        })
}

/// A git commit, not `commit-tree` and not a command that merely mentions
/// the word. Global options that change the repository are skipped; any
/// other flag means this is not the commit subcommand.
fn is_git_commit(line: &str) -> bool {
    super::nested_lines(line, 0)
        .iter()
        .flat_map(|l| super::raw_segments(l))
        .any(|seg| {
            let words = super::shell_words(&seg);
            let (at, _) = super::prefix_end(&words);
            git_commit_argv(&words[at..])
        })
}

fn git_commit_argv(words: &[String]) -> bool {
    let Some(bin) = words.first() else {
        return false;
    };
    if bin.rsplit('/').next().unwrap_or(bin) != "git" {
        return false;
    }
    let mut i = 1;
    while i < words.len() {
        match words[i].as_str() {
            "-C" | "-c" | "--git-dir" | "--work-tree" => i += 2,
            flag if flag.starts_with("--git-dir=") || flag.starts_with("--work-tree=") => i += 1,
            "commit" => return true,
            _ => return false,
        }
    }
    false
}

fn parse_issue_id(text: &str) -> Option<String> {
    let mut found = None;
    for raw in text.split_whitespace() {
        let token = raw.trim_matches(|c: char| matches!(c, ',' | '.' | ':' | '"' | '\''));
        if is_issue_id(token) {
            found = Some(token.to_string());
        }
    }
    found
}

fn is_issue_id(token: &str) -> bool {
    let Some((prefix, tail)) = token.split_once('-') else {
        return false;
    };
    prefix
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && prefix.chars().all(|c| c.is_ascii_alphanumeric())
        && tail.len() >= 4
        && tail.chars().all(|c| c.is_ascii_alphanumeric())
        && !token
            .split_once('-')
            .is_some_and(|(_, rest)| rest.contains('-'))
}

fn parse_deed(text: &str) -> Option<String> {
    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, '=' | '"' | '\'' | ',')) {
        let Some(rest) = raw.strip_prefix("deed-") else {
            continue;
        };
        if !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Some(format!("deed-{rest}"));
        }
    }
    None
}

struct Said {
    stdout: String,
}

fn run_timed(bin: &str, args: &[&str], env: &[(&str, &str)]) -> anyhow::Result<Said> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let path = which::which(bin).map_err(|_| anyhow::anyhow!("{bin} not on PATH"))?;
    let mut cmd = Command::new(path);
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("{bin}: {e}"))?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_string(&mut stderr);
                }
                if !status.success() {
                    let why = if stderr.trim().is_empty() {
                        stdout.trim()
                    } else {
                        stderr.trim()
                    };
                    anyhow::bail!("{bin} exited {status}: {why}");
                }
                return Ok(Said { stdout });
            }
            Ok(None) if started.elapsed() > COMMAND_DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("{bin} exceeded 2s");
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => anyhow::bail!("{bin}: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_env_lock()
    }

    struct Restore {
        pairs: Vec<(String, Option<std::ffi::OsString>)>,
    }

    impl Restore {
        fn new(keys: &[&str]) -> Self {
            Self {
                pairs: keys
                    .iter()
                    .map(|k| ((*k).to_string(), std::env::var_os(k)))
                    .collect(),
            }
        }
    }

    impl Drop for Restore {
        fn drop(&mut self) {
            for (key, value) in &self.pairs {
                // SAFETY: the caller holds test_env_lock, dropped after this.
                unsafe {
                    match value {
                        Some(v) => std::env::set_var(key, v),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    #[test]
    fn a_temp_path_is_the_directory_not_a_prefix() {
        assert!(path_is_temp(Some(Path::new("/tmp/scratch"))));
        assert!(!path_is_temp(Some(Path::new("/tmpfoo"))));
        assert!(!path_is_temp(Some(Path::new("/home/ubuntu"))));
        assert!(!path_is_temp(None));
    }

    #[test]
    fn ljos_off_and_a_temp_home_suspend_the_guard() {
        let _g = lock();
        let _saved = Restore::new(&["HOME", "LJOS_OFF"]);
        unsafe {
            std::env::remove_var("LJOS_OFF");
            std::env::set_var("HOME", "/home/ubuntu");
        }
        assert!(!guard_suspended(), "a normal home is a seat");
        unsafe { std::env::set_var("LJOS_OFF", "1") };
        assert!(guard_suspended());
        unsafe {
            std::env::set_var("LJOS_OFF", "yes");
        }
        assert!(guard_suspended());
        unsafe {
            std::env::remove_var("LJOS_OFF");
            std::env::set_var("HOME", "/tmp/ljos-not-a-seat");
        }
        assert!(guard_suspended(), "a temp HOME is not a seat");
        unsafe { std::env::set_var("HOME", "/tmpfoo") };
        assert!(!guard_suspended(), "/tmpfoo is a name, not /tmp");
    }

    #[test]
    fn a_commit_is_the_commit_subcommand() {
        assert!(is_git_commit("git commit -m \"readme\""));
        assert!(is_git_commit("git -C /src commit -m readme"));
        assert!(is_git_commit("FOO=1 git --git-dir=/src/.git commit"));
        assert!(!is_git_commit("git commit-tree HEAD"));
        assert!(!is_git_commit("git status"));
        assert!(!is_git_commit("echo git commit"));
        assert!(command_is_apply_patch("apply_patch <<'EOF'\n***\nEOF"));
        assert!(!command_is_apply_patch("git apply patch"));
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn habitat_bin(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("vissue"),
            r#"#!/usr/bin/env python3
import os, sys, pathlib, re
root = pathlib.Path(os.environ["VISSUE_ROOT"])
log = root / "vissue.log"
args = sys.argv[1:]
with log.open("a") as fh:
    fh.write(" ".join(args) + "\n")
cmd, rest = args[0], args[1:]

def files():
    return list(root.rglob("issues.org"))

def rewrite(iid, state):
    pat = re.compile(r"(\* )[A-Z]+( \[#[A-Z]\] [^\n]*\n:PROPERTIES:\n:ID:\s+" + re.escape(iid) + r"\n)")
    for f in files():
        text = f.read_text()
        new, n = pat.subn(r"\1" + state + r"\2", text, count=1)
        if n:
            f.write_text(new)
            return
    sys.exit("no heading for " + iid)

if cmd == "create":
    proj, title, i = "agent", "agent work", 0
    while i < len(rest):
        if rest[i] == "-p" and i + 1 < len(rest):
            proj = rest[i + 1]
            i += 2
            continue
        if rest[i] == "-q":
            i += 1
            continue
        title = rest[i]
        i += 1
    seq = root / ".seq"
    n = int(seq.read_text()) + 1 if seq.exists() else 1
    seq.write_text(str(n))
    iid = f"{proj}-{n:04x}"
    dest = root / "Software" / proj
    dest.mkdir(parents=True, exist_ok=True)
    with (dest / "issues.org").open("a") as fh:
        fh.write(f"* TODO [#C] {title}\n:PROPERTIES:\n:ID:         {iid}\n:END:\n")
    print(iid)
elif cmd == "claim":
    rewrite(rest[0], "STARTED")
elif cmd == "update":
    state = rest[rest.index("-s") + 1]
    rewrite(rest[0], state)
elif cmd == "deed":
    iid, acc = rest[0], rest[rest.index("--add") + 1]
    needle = f":ID:         {iid}\n:END:\n"
    for f in files():
        text = f.read_text()
        if needle in text:
            f.write_text(text.replace(needle, needle + f"cited {acc}\n", 1))
            sys.exit(0)
    sys.exit("no issue " + iid)
elif cmd == "claims":
    print("[]")
elif cmd == "show":
    print("{}")
else:
    sys.exit("unknown " + cmd)
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("deedar"),
            r#"#!/usr/bin/env python3
import os, pathlib, sys
root = pathlib.Path(os.environ["VISSUE_ROOT"])
args = sys.argv[1:]
(root / "deedar.log").open("a").write(" ".join(args) + "\n")
ident = None
i = 0
while i < len(args):
    if args[i] == "--id" and i + 1 < len(args):
        ident = args[i + 1]
    if args[i] == "--path" and i + 1 < len(args):
        (root / "deedar-body.log").open("a").write(pathlib.Path(args[i + 1]).read_text())
    i += 1
if not ident:
    seq = root / ".deedseq"
    n = int(seq.read_text()) + 1 if seq.exists() else 1
    seq.write_text(str(n))
    ident = f"deed-{n:04x}"
print(f"id={ident}")
"#,
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        for name in ["vissue", "deedar"] {
            std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
    }

    fn scratch() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let tracker = dir.path().join("tracker");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&tracker).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        for root in [&tracker, &repo] {
            git(root, &["init", "-q"]);
            git(root, &["config", "user.email", "seat@example.invalid"]);
            git(root, &["config", "user.name", "seat"]);
            git(root, &["config", "core.hooksPath", "/dev/null"]);
        }
        std::fs::write(repo.join("README.md"), "seed\n").unwrap();
        git(&repo, &["add", "README.md"]);
        git(&repo, &["commit", "-q", "-m", "seed"]);
        habitat_bin(&dir.path().join("bin"));
        (dir, tracker, repo)
    }

    fn arm(dir: &Path, tracker: &Path) {
        let bin = dir.join("bin");
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut next = std::ffi::OsString::from(bin);
        next.push(":");
        next.push(path);
        unsafe {
            std::env::set_var("PATH", next);
            std::env::set_var("VISSUE_ROOT", tracker);
            std::env::set_var("VISSUE_NO_ROUTE", "1");
            std::env::set_var("LJOS_TRACKER_GIT", "commit");
            std::env::set_var("LJOS_PROJECT", "probe");
            std::env::set_var("XDG_RUNTIME_DIR", dir.join("run"));
            std::env::set_var("LJOS_IN_HOOK", "1");
            std::env::set_var("PACKSET_URL", "off");
            std::env::remove_var("ISSUE_ROOT");
            std::env::remove_var("LJOS_OFF");
            std::env::remove_var("LJOS_SESSION_ID");
        }
    }

    fn payload(event: &str, session: &str, cwd: &Path, extra: Value) -> (String, crate::HookCall) {
        let mut raw = serde_json::json!({
            "hook_event_name": event,
            "session_id": session,
            "cwd": cwd,
        });
        if let (Value::Object(dst), Value::Object(src)) = (&mut raw, extra) {
            for (k, v) in src {
                dst.insert(k, v);
            }
        }
        let text = raw.to_string();
        let call = crate::hook_call(&text);
        (text, call)
    }

    #[test]
    fn a_session_files_a_claim_a_deed_and_a_finish_without_being_asked() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "VISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_TRACKER_GIT",
            "LJOS_PROJECT",
            "XDG_RUNTIME_DIR",
            "LJOS_IN_HOOK",
            "PACKSET_URL",
            "ISSUE_ROOT",
            "LJOS_OFF",
            "LJOS_SESSION_ID",
        ]);
        let (dir, tracker, repo) = scratch();
        arm(dir.path(), &tracker);
        let session = "book-session-1";
        let (prompt_raw, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "Add a readme line"}),
        );
        assert!(
            !prompt_raw.contains("ljos"),
            "the prompt the test sends does not name the seat"
        );
        assert!(observe(&prompt_raw, &prompt_call).0.is_empty());

        let (edit_raw, edit_call) = payload(
            "PreToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "README.md"}}),
        );
        let _ = observe(&edit_raw, &edit_call);

        let (start_raw, start_call) = payload("SessionStart", session, &repo, Value::Null);
        let (recall, _) = observe(&start_raw, &start_call);
        assert!(recall.contains("Recall for probe-"), "{recall}");
        assert!(recall.contains("Add a readme line"), "{recall}");

        let (commit_raw, commit_call) = payload(
            "PreToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m \"readme\""}}),
        );
        let _ = observe(&commit_raw, &commit_call);
        let deedar_before = std::fs::read_to_string(tracker.join("deedar.log")).unwrap_or_default();
        assert!(
            deedar_before.is_empty(),
            "the pre-hook does not cite a commit that has not run\n{deedar_before}"
        );
        // The same commit on the way out cites the sha once.
        let (post_raw, post_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m \"readme\""}}),
        );
        let _ = observe(&post_raw, &post_call);
        let _ = observe(&post_raw, &post_call);

        let (stop_raw, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop_raw, &stop_call);
        let log_before = git_log(&tracker);
        let _ = observe(&stop_raw, &stop_call);
        let (end_raw, end_call) = payload("SessionEnd", session, &repo, Value::Null);
        let _ = observe(&end_raw, &end_call);
        let log = git_log(&tracker);
        assert_eq!(log, log_before, "a second stop does not finish again");
        assert!(log.contains("claimed"), "{log}");
        assert!(log.contains("cited a deed"), "{log}");
        assert!(log.contains("finished"), "{log}");
        assert_eq!(log.matches("finished").count(), 1, "{log}");
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(org.contains("Add a readme line"), "{org}");
        let sha = String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let sha = sha.trim();
        assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
        let body = std::fs::read_to_string(tracker.join("deedar-body.log")).unwrap();
        assert!(body.contains(sha), "{body}");
        assert_eq!(
            std::fs::read_to_string(tracker.join("deedar.log"))
                .unwrap()
                .lines()
                .count(),
            1
        );

        let vissue_log = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        let creates =
            vissue_log.matches("\ncreate").count() + usize::from(vissue_log.starts_with("create"));
        assert_eq!(creates, 1, "{vissue_log}");
        let started = std::time::Instant::now();
        let _ = observe(&edit_raw, &edit_call);
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "warm edit took {:?}",
            started.elapsed()
        );
        let after = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        assert_eq!(after, vissue_log, "a warm edit does not ask vissue");
    }

    #[test]
    fn a_missing_tracker_does_not_block_the_next_edit() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "VISSUE_ROOT",
            "XDG_RUNTIME_DIR",
            "LJOS_IN_HOOK",
            "PACKSET_URL",
            "LJOS_OFF",
            "LJOS_PROJECT",
        ]);
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        unsafe {
            std::env::set_var("PATH", "/usr/bin:/bin");
            std::env::set_var("XDG_RUNTIME_DIR", dir.path().join("run"));
            std::env::set_var("LJOS_IN_HOOK", "1");
            std::env::set_var("PACKSET_URL", "off");
            std::env::remove_var("LJOS_OFF");
            std::env::set_var("VISSUE_ROOT", dir.path().join("tracker"));
            std::env::set_var("LJOS_PROJECT", "probe");
        }
        let (raw, call) = payload(
            "PreToolUse",
            "book-missing",
            &repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "a.txt"}, "prompt": "Write the notes"}),
        );
        // The prompt is a separate event; the edit still has a title from it
        // when it was stored. Here the edit arrives with no prompt stored.
        let (text, _) = observe(&raw, &call);
        assert!(text.is_empty());
        let started = std::time::Instant::now();
        let (again, _) = observe(&raw, &call);
        assert!(again.is_empty());
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "second edit took {:?}",
            started.elapsed()
        );
        let logged =
            std::fs::read_to_string(dir.path().join("run/ljos/book.log")).unwrap_or_default();
        assert_eq!(logged.matches("claim:").count(), 1, "{logged}");
    }

    #[test]
    fn the_server_claims_on_a_write_and_finishes_when_it_stops() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "VISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_TRACKER_GIT",
            "LJOS_PROJECT",
            "XDG_RUNTIME_DIR",
            "LJOS_IN_HOOK",
            "PACKSET_URL",
            "LJOS_SESSION_ID",
            "LJOS_OFF",
        ]);
        let (dir, tracker, _repo) = scratch();
        arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SESSION_ID", "mcp-session-9");
        }
        assert!(!mcp_tool_writes("ljos_claim"));
        assert!(!mcp_tool_writes("ljos_file"));
        assert!(!mcp_tool_writes("ljos_finish"));
        assert!(mcp_tool_writes("ljos_remember"));
        let mut args = serde_json::Map::new();
        args.insert(
            "text".into(),
            Value::String("the readme stays short".into()),
        );
        before_mcp_write("ljos_remember", Some(&args));
        before_mcp_write("ljos_claim", Some(&args));
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(org.contains("the readme stays short"), "{org}");
        mcp_shutdown();
        let log = git_log(&tracker);
        assert!(log.contains("claimed"), "{log}");
        assert!(log.contains("finished"), "{log}");
        mcp_shutdown();
        assert_eq!(git_log(&tracker), log);
        let before = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        args.insert("text".into(), Value::String("the notes stay dated".into()));
        before_mcp_write("ljos_remember", Some(&args));
        let after = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        let creates =
            |log: &str| log.matches("\ncreate").count() + usize::from(log.starts_with("create"));
        assert_eq!(
            creates(&after),
            creates(&before) + 1,
            "a write after shutdown files a new claim\n{after}"
        );
    }

    #[test]
    fn a_per_call_server_keeps_one_sitting_for_the_seat() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "VISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_TRACKER_GIT",
            "LJOS_PROJECT",
            "XDG_RUNTIME_DIR",
            "LJOS_IN_HOOK",
            "PACKSET_URL",
            "LJOS_SESSION_ID",
            "LJOS_SEAT",
            "LJOS_MCP_IDLE",
            "LJOS_OFF",
        ]);
        let (dir, tracker, _repo) = scratch();
        arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SEAT", "Disk Saver");
            std::env::set_var("LJOS_SESSION_ID", "mcp-call-1");
            std::env::set_var("LJOS_MCP_IDLE", "30m");
        }
        let mut args = serde_json::Map::new();
        args.insert("text".into(), Value::String("note on rtrash-adz0".into()));
        before_mcp_write("ljos_note", Some(&args));
        unsafe {
            std::env::set_var("LJOS_SESSION_ID", "mcp-call-2");
        }
        args.insert(
            "text".into(),
            Value::String("still noting rtrash-adz0".into()),
        );
        before_mcp_write("ljos_note", Some(&args));
        let log = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        let creates =
            |text: &str| text.matches("\ncreate").count() + usize::from(text.starts_with("create"));
        assert_eq!(
            creates(&log),
            1,
            "a second call from the same seat does not file another claim\n{log}"
        );
        assert!(
            !git_log(&tracker).contains("finished"),
            "the per-call exit has not happened"
        );
        stamp_sitting(&sitting_key(), 1_000);
        resume_seat_at(1_000 + 30 * 60);
        let finished = git_log(&tracker);
        assert!(
            finished.contains("finished"),
            "the next start closes the idle sitting\n{finished}"
        );
        before_mcp_write("ljos_note", Some(&args));
        let after = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        assert_eq!(
            creates(&after),
            2,
            "a write after the idle close files the next claim\n{after}"
        );
    }

    #[test]
    fn idle_defaults_to_half_an_hour_and_zero_turns_it_off() {
        assert_eq!(idle_limit_from(None), Some(Duration::from_secs(1800)));
        assert_eq!(idle_limit_from(Some("")), Some(Duration::from_secs(1800)));
        assert_eq!(idle_limit_from(Some("0")), None);
        assert_eq!(idle_limit_from(Some("OFF")), None);
        assert_eq!(idle_limit_from(Some("15m")), Some(Duration::from_secs(900)));
        assert_eq!(
            idle_limit_from(Some("nope")),
            Some(Duration::from_secs(1800))
        );
        assert!(idle_due(2_000, 100, Duration::from_secs(1800)));
        assert!(!idle_due(1_000, 100, Duration::from_secs(1800)));
        assert!(!idle_due(5_000, 0, Duration::from_secs(1)));
    }

    #[test]
    fn an_empty_guard_is_refused() {
        let err = crate::guard_command(" ").unwrap_err().to_string();
        assert!(err.contains("pass the command"), "{err}");
        assert!(crate::command_refused("deny\tTCB required\n"));
        assert!(crate::command_refused("ask: wipe\n"));
        assert!(!crate::command_refused("echo hi\n"));
        assert!(!crate::command_refused("the person may ask later\n"));
    }

    #[test]
    fn a_call_seat_must_be_one_the_tracker_knows() {
        let _g = lock();
        let _saved = Restore::new(&["VISSUE_ROOT", "ISSUE_ROOT", "VISSUE_NO_ROUTE", "LJOS_SEAT"]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("vissue.toml"),
            "seats = [\"inky\", \"Disk Saver\", \"saddle\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("Software/probe")).unwrap();
        std::fs::write(
            dir.path().join("Software/probe/issues.org"),
            "* STARTED [#C] held\n:PROPERTIES:\n:ID:         probe-abcd\n:CLAIMED_BY: eessi@box\n:END:\n",
        )
        .unwrap();
        unsafe {
            std::env::set_var("VISSUE_ROOT", dir.path());
            std::env::set_var("VISSUE_NO_ROUTE", "1");
            std::env::remove_var("ISSUE_ROOT");
            std::env::remove_var("LJOS_SEAT");
        }
        let err = crate::resolve_call_seat(None).unwrap_err().to_string();
        assert!(err.contains("LJOS_SEAT"), "{err}");
        unsafe {
            std::env::set_var("LJOS_SEAT", "inky");
        }
        assert_eq!(crate::resolve_call_seat(None).unwrap().seat, "inky");
        assert_eq!(
            crate::resolve_call_seat(Some("Disk Saver")).unwrap().seat,
            "disk-saver"
        );
        let err = crate::resolve_call_seat(Some("sadle"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Did you mean saddle"), "{err}");
        assert!(err.contains("inky"), "{err}");
        assert!(err.contains("disk-saver"), "{err}");
        assert!(err.contains("eessi"), "{err}");
    }

    #[test]
    fn the_mcp_seat_follows_the_hook_seat() {
        let _g = lock();
        let _saved = Restore::new(&[
            "HOME",
            "LJOS_SEAT",
            "VISSUE_AGENT",
            "VISSUE_ROOT",
            "ISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "XDG_RUNTIME_DIR",
        ]);
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".config/ljos");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("env"), "LJOS_SEAT=free-claude\n").unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("vissue.toml"),
            "seats = [\"free-claude\", \"inky\"]\n",
        )
        .unwrap();
        unsafe {
            std::env::set_var("HOME", home.path());
            std::env::set_var("VISSUE_ROOT", dir.path());
            std::env::set_var("VISSUE_NO_ROUTE", "1");
            std::env::remove_var("ISSUE_ROOT");
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("VISSUE_AGENT");
        }
        assert_eq!(
            crate::resolve_call_seat(Some("claude")).unwrap().seat,
            "free-claude",
            "a harness name does not replace the seat in the config"
        );
        assert_eq!(crate::resolve_call_seat(None).unwrap().seat, "free-claude");
        unsafe {
            std::env::set_var("LJOS_SEAT", "free-claude");
        }
        assert_eq!(crate::resolve_call_seat(Some("inky")).unwrap().seat, "inky");
        unsafe {
            std::env::set_var("LJOS_SEAT", "inky");
        }
        assert_eq!(
            crate::resolve_call_seat(Some("claude")).unwrap().seat,
            "inky",
            "LJOS_SEAT wins over the harness id"
        );
    }

    #[test]
    fn a_tracker_with_no_seats_registers_the_harness_seat() {
        let _g = lock();
        let _saved = Restore::new(&[
            "VISSUE_ROOT",
            "ISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_SEAT",
            "VISSUE_AGENT",
            "XDG_RUNTIME_DIR",
            "HOME",
        ]);
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().join("run");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(dir.path().join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        unsafe {
            std::env::set_var("VISSUE_ROOT", dir.path());
            std::env::set_var("VISSUE_NO_ROUTE", "1");
            std::env::set_var("XDG_RUNTIME_DIR", &run);
            std::env::remove_var("ISSUE_ROOT");
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("VISSUE_AGENT");
            std::env::set_var("HOME", dir.path().join("home"));
        }
        let seat = crate::resolve_call_seat(Some("ljosbot")).unwrap();
        assert_eq!(seat.seat, "ljosbot");
        let text = std::fs::read_to_string(dir.path().join("vissue.toml")).unwrap();
        assert!(text.contains("ljosbot"), "{text}");
        assert!(text.contains("seats"), "{text}");
        assert!(text.contains("Software"), "{text}");
        let log = std::fs::read_to_string(run.join("ljos/book.log")).unwrap();
        assert!(log.contains("registered"), "{log}");
        assert!(log.contains("ljosbot"), "{log}");
    }

    #[test]
    fn an_issue_named_none_is_the_one_this_session_claimed() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "VISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_TRACKER_GIT",
            "LJOS_PROJECT",
            "XDG_RUNTIME_DIR",
            "LJOS_IN_HOOK",
            "PACKSET_URL",
            "ISSUE_ROOT",
            "LJOS_OFF",
            "LJOS_SESSION_ID",
            "LJOS_HARNESS_SESSION",
            "LJOS_SEAT",
        ]);
        let (dir, tracker, repo) = scratch();
        arm(dir.path(), &tracker);
        unsafe {
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("LJOS_HARNESS_SESSION");
        }
        let session = "book-none";
        let (edit_raw, edit_call) = payload(
            "PreToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        );
        let _ = observe(&edit_raw, &edit_call);
        let issue = canonical_issue(Some("none"), Some(session)).expect("the claim");
        assert!(issue.starts_with("probe-"), "{issue}");
        assert_eq!(
            canonical_issue(Some("NONE"), Some(session)).as_deref(),
            Some(issue.as_str())
        );
        assert_eq!(
            canonical_issue(Some("null"), Some(session)).as_deref(),
            Some(issue.as_str())
        );
        assert_eq!(
            canonical_issue(Some("other-1"), Some(session)).as_deref(),
            Some("other-1")
        );
        let line = crate::session_start_for(Some(session));
        assert!(
            line.contains(&format!("This session's issue is {issue}.")),
            "{line}"
        );
    }

    #[test]
    fn ljos_start_claims_the_task() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "VISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_TRACKER_GIT",
            "LJOS_PROJECT",
            "XDG_RUNTIME_DIR",
            "LJOS_IN_HOOK",
            "PACKSET_URL",
            "ISSUE_ROOT",
            "LJOS_OFF",
            "LJOS_SESSION_ID",
        ]);
        let (dir, tracker, repo) = scratch();
        arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SESSION_ID", "mcp-start-1");
        }
        let err = start_work(None, None, "  ").unwrap_err();
        assert!(err.contains("one sentence"), "{err}");
        let said = start_work(None, Some(repo.to_str().unwrap()), "Add a readme line").unwrap();
        assert!(said.contains("claimed probe-"), "{said}");
        assert!(said.contains("Add a readme line"), "{said}");
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(org.contains("Add a readme line"), "{org}");
        let held = open_issue().expect("the start filed an issue");
        assert!(said.contains(&held), "{said}");
        mcp_shutdown();
        assert!(git_log(&tracker).contains("finished"));
        assert!(open_issue().is_none());
    }

    #[test]
    fn a_second_commit_is_a_second_deed_and_a_failed_one_is_not() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "VISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_TRACKER_GIT",
            "LJOS_PROJECT",
            "XDG_RUNTIME_DIR",
            "LJOS_IN_HOOK",
            "PACKSET_URL",
            "ISSUE_ROOT",
            "LJOS_OFF",
            "LJOS_SESSION_ID",
            "LJOS_HOLD",
            "LJOS_SEAT",
        ]);
        let (dir, tracker, repo) = scratch();
        arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SEAT", "probe-seat");
            std::env::set_var("LJOS_HOLD", "book-holder");
        }
        let run = dir.path().join("run/ljos");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            run.join("hold-book-holder"),
            "book-holder\nprobe-seat\n999999\nbash\n2026-01-01T00:00:00Z\nother-9999\n-\n",
        )
        .unwrap();
        let session = "book-two-commits";
        let (prompt_raw, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "Add a readme line"}),
        );
        let _ = observe(&prompt_raw, &prompt_call);
        let (edit_raw, edit_call) = payload(
            "PreToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        );
        let _ = observe(&edit_raw, &edit_call);
        let vissue = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        assert!(!vissue.contains("other-9999"), "{vissue}");
        assert!(vissue.contains("create"), "{vissue}");
        fn commit(repo: &Path, name: &str) -> String {
            std::fs::write(repo.join(name), "x\n").unwrap();
            git(repo, &["add", name]);
            git(repo, &["commit", "-q", "-m", name]);
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        }
        let first = commit(&repo, "one.txt");
        let (post, post_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m one"}, "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&post, &post_call);
        let failed = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m boom"}, "tool_response": {"exit_code": 1}}),
        );
        let _ = observe(&failed.0, &failed.1);
        let second = commit(&repo, "two.txt");
        let (post2, post2_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m two"}}),
        );
        let _ = observe(&post2, &post2_call);
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(org.contains(&format!("deed-file-commit-{first}")), "{org}");
        assert!(org.contains(&format!("deed-file-commit-{second}")), "{org}");
        assert!(!org.contains("other-9999"), "{org}");
        let deeds = std::fs::read_to_string(tracker.join("deedar.log")).unwrap();
        assert_eq!(deeds.lines().count(), 2, "{deeds}");
        let (redir, redir_call) = payload(
            "PreToolUse",
            "book-redirect",
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "echo hi > README.md"}}),
        );
        let before = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        let _ = observe(&redir, &redir_call);
        let after = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        assert!(
            after.matches("create").count() > before.matches("create").count(),
            "a redirect onto a tracked file claims\n{after}"
        );
        let text = lesson_text(
            "Add a readme line",
            "probe-0001",
            &[first.clone(), second],
            &["git commit exited non-zero".into()],
            &["git-force-push: a force push rewrites history".into()],
        );
        let sentences: Vec<&str> = text.split(". ").collect();
        assert!(sentences.len() <= 2, "{text}");
        for sentence in text.split_inclusive('.') {
            let sentence = sentence.trim();
            if sentence.is_empty() {
                continue;
            }
            assert!(sentence.split_whitespace().count() < 30, "{sentence}");
        }
        assert!(text.contains("probe-0001"), "{text}");
        assert!(text.contains(&first[..7]), "{text}");
    }

    fn git_log(tracker: &Path) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(tracker)
            .args(["log", "--format=%s"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}
