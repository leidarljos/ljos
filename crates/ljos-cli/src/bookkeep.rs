//! The seat keeps the book when the model never calls ljos.
//!
//! A harness hook files a claim on the first edit in a session and
//! repository, cites a deed when a git commit has landed, and finishes at
//! Stop or the end of the session. The finish writes one lesson from the
//! sitting. `ljos-mcp` does the same on the first write and on shutdown,
//! for a client that has no hooks. A failure is written to the runtime
//! log and never becomes a deny: the agent's tool call goes on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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
/// without regard to case, so `edit` and `Edit` are the same tool. agy 1.4.3
/// writes a new file with `write_file` (arguments `Path`, `Content`,
/// `Overwrite`) and changes one with `edit_file`.
const EDIT_TOOLS: &[&str] = &[
    "Edit",
    "Write",
    "MultiEdit",
    "apply_patch",
    "StrReplace",
    "ApplyPatch",
    "write_to_file",
    "write_file",
    "replace_file_content",
    "edit_file",
    "NotebookEdit",
];

/// MCP tools that write a habitat. The claim, the sitting and the finish
/// are the bookkeeping verbs themselves, and `ljos_file` opens its own issue.
/// Before one of these runs the server joins the issue this seat holds
/// ([`before_mcp_write`]); it never files one. Mail (`ljos_send`,
/// `ljos_reply`) and `ljos_note` are not on the list.
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

/// One sitting's book: the prompt the title came from, and one row per
/// repository the sitting has touched. An MCP server spawned per call
/// keeps this under the seat, so the next process sees the open claim.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct Book {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    repos: BTreeMap<String, RepoBook>,
    /// Unix seconds of the last call for this sitting. Zero means the
    /// sitting has not been touched, and an idle check does not finish it.
    /// This is not a row's age: a commit in one repository refreshes it
    /// while another repository's row stays idle.
    #[serde(default)]
    last_unix: u64,
    /// Repository `prompt` was recorded for. A git-hook claim uses the
    /// prompt as its title only when the commit is in this checkout and
    /// the sitting is still open.
    #[serde(default)]
    prompt_repo: String,
    /// Unix seconds when `prompt` was recorded. Zero means it was not stamped.
    #[serde(default)]
    prompt_unix: u64,
    /// Pid of the runner that last wrote this book. A later session finishes
    /// the claim when that pid is gone, which is how a timeout (the runner
    /// exits 124 before Stop) still closes.
    #[serde(default)]
    owner_pid: u32,
    /// When `owner_pid` started, in clock ticks after boot, so a pid the
    /// kernel has since handed to another process counts as gone. Zero is
    /// a book written before this was kept, or a system with no `/proc`:
    /// the pid alone is checked then.
    #[serde(default)]
    owner_start: u64,
    /// `cwd` as the hook named it, to the repository key. The warm path is
    /// a lookup, not another `git rev-parse`.
    #[serde(default)]
    cwd_repo: BTreeMap<String, String>,
    /// The last repository a git hook booked for this seat. An MCP claim
    /// with no `repo` argument files there instead of under the server's cwd.
    #[serde(default)]
    last_repo: String,
}

/// What this session has already recorded for one repository.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct RepoBook {
    #[serde(default)]
    issue: Option<String>,
    /// `ljos_start` claimed this row's issue. A hook in another sitting
    /// takes over only such a row. Any other live row, one this sitting
    /// filed or one it adopted from a hold record, is that conversation's.
    #[serde(default)]
    started: bool,
    /// This sitting filed the issue: a hook on the first edit or commit, or
    /// `ljos_start` with no ticket named. A commit naming another open issue
    /// before any deed lands here takes the row over, and this issue is
    /// cancelled. Starting again on the same issue leaves `filed` as it is.
    /// A rebind sets it for the issue that start claims.
    #[serde(default)]
    filed: bool,
    /// Commit shas already cited. The id is `deed-file-commit-<sha>`, so a
    /// second commit is a second deed, and the same sha is not cited twice.
    #[serde(default)]
    deeds: Vec<String>,
    /// Cited shas found in HEAD by a sweep. A sweep asks git once for a
    /// sha, not on every prompt.
    #[serde(default)]
    landed: Vec<String>,
    /// The HEAD those shas were found in.
    #[serde(default)]
    landed_head: Option<String>,
    /// Short notes from tool failures, for the lesson written at Stop.
    #[serde(default)]
    failures: Vec<String>,
    /// Short notes from guard asks, for the same lesson.
    #[serde(default)]
    asks: Vec<String>,
    /// The sitting's lesson was written. Stop and SessionEnd share one.
    #[serde(default)]
    lesson: bool,
    /// The pack's id for that lesson. A rewrite supersedes it.
    #[serde(default)]
    lesson_id: Option<String>,
    /// The title that lesson was written under. A later prompt is another
    /// turn and does not replace it; the rewrite closes the old atom by id.
    #[serde(default)]
    lesson_title: Option<String>,
    #[serde(default)]
    finished: bool,
    /// `vissue` failed once. Later edits stay on the warm path instead of
    /// asking again on every tool call.
    #[serde(default)]
    claim_failed: bool,
    #[serde(default)]
    recall: String,
    /// HEAD the first time this sitting saw the repository, before a commit
    /// ran. A deed is cited only when HEAD has moved past it. A rebind to
    /// another issue moves this to HEAD then, so the previous issue's
    /// commits are not cited on the new one.
    #[serde(default)]
    start_sha: Option<String>,
    /// The prompt this sitting is working from. agy sends no title of its
    /// own; the words come from the prompt payload.
    #[serde(default)]
    title: Option<String>,
    /// Unix seconds of the last hook call that touched this repository.
    /// Zero means the row has no clock of its own. The book's `last_unix`
    /// is the last touch of whichever repository the seat used, not this one.
    #[serde(default)]
    last_unix: u64,
    /// The issue is one a commit named, not one this sitting filed. The
    /// sweep hands it back with a release instead of closing it: the
    /// commit cited work on someone's open ticket, it did not accept it.
    /// Starting again on the same issue keeps `named`. Only a rebind to a
    /// different issue clears it.
    #[serde(default)]
    named: bool,
    /// Other open issues this row's commits named while it held `issue`.
    /// The sweep releases each of them too.
    #[serde(default)]
    also: Vec<String>,
    /// Shas already cited on an issue in `also`, so they are not cited on
    /// `issue` as well.
    #[serde(default)]
    elsewhere: Vec<String>,
}

/// Whether the guard and the pack daemon should stay out of this process.
/// `LJOS_OFF` and a temporary `HOME` are not seats: a test sandbox sets
/// its own home, and a daemon must not write under it. A temporary working
/// directory under a real home is a seat, so `cd /tmp && rm -rf ~/proj` is
/// judged. `LJOS_GUARD=on` keeps the guard on under a temporary home. The
/// book is not suspended here. A hook in a scratch directory still files
/// its claim.
#[must_use]
pub fn guard_suspended() -> bool {
    ljos_off() || (temp_home().is_some() && !guard_forced_on())
}

/// `LJOS_GUARD` set to one of the on-switches: the guard judges commands
/// under a temporary `HOME` too. Tests use it in place of a home moved out
/// of `/tmp`. `LJOS_OFF` still wins.
#[must_use]
pub fn guard_forced_on() -> bool {
    matches!(
        std::env::var("LJOS_GUARD").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

/// `LJOS_OFF` set to one of the on-switches.
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

/// Why [`guard_suspended`] holds in this process, in words a person reads
/// in `ljos doctor`. `None` when the guard is on.
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
    let session = session_key(call);
    let value: Value = serde_json::from_str(raw.trim()).unwrap_or(Value::Null);
    let _held = super::hook_timed("book_lock", || {
        super::CommitLock::acquire(&super::runtime_dir().join("book.lock"))
    });
    // A runner killed before Stop leaves its claims open. The next session
    // finishes those whose pid is gone, and replaces a deed whose commit is
    // no longer in the checkout with the one that is.
    if matches!(call.event.as_str(), "SessionStart" | "UserPromptSubmit") {
        let deadline = std::time::Instant::now() + SWEEP_BUDGET;
        super::hook_timed("finish_orphans", || finish_orphans(&session, deadline));
        super::hook_timed("refresh_landed_deeds", || refresh_landed_deeds(deadline));
    }
    // A sitting that only commits never sees SessionStart. Close every row
    // that has been idle, including one in another repository, before this
    // call stamps the book again.
    let path = book_path(&session);
    let repo_hint = {
        let mut scout = load_book(&path);
        repo_key(&mut scout, &value)
    };
    let hint = (!repo_hint.is_empty()).then_some(repo_hint.as_str());
    close_idle_sittings(unix_secs(), Some(session.as_str()), hint);
    let mut book = load_book(&path);
    (book.owner_pid, book.owner_start) = next_owner(
        &call.event,
        (book.owner_pid, book.owner_start),
        hook_owner(),
    );
    let mut prompt_set = false;
    if call.event == "UserPromptSubmit" {
        let prompt = collapse(&call.cue);
        if !prompt.is_empty() {
            book.prompt = prompt.chars().take(4000).collect();
            prompt_set = true;
        }
    }
    // agy's first model call often runs before the transcript has the
    // turn. A later payload still names that file, or carries `prompt`.
    if book.prompt.trim().is_empty() {
        let prompt = collapse(&super::prompt_of(&value));
        if !prompt.is_empty() {
            book.prompt = prompt.chars().take(4000).collect();
            prompt_set = true;
        }
    }
    let repo = repo_key(&mut book, &value);
    // Stop and SessionEnd close what the sitting opened. In a checkout the
    // book has no row for, they open nothing: an MCP server that answered
    // only guard checks used to leave a row with no issue at its exit.
    let ending_elsewhere =
        matches!(call.event.as_str(), "Stop" | "SessionEnd") && !book.repos.contains_key(&repo);
    let now = unix_secs();
    if prompt_set {
        bind_prompt(&mut book, &repo, now);
    }
    // `ljos hook --git-commit` puts the commit subject in `prompt`. That
    // subject titles the claim when this record has no live prompt for
    // the checkout. A prompt from the current sitting in this repository
    // still wins, and is left on the book for that sitting.
    let git_hook = value
        .get("ljos_git_hook")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let subject_title = git_hook && !prompt_live_for(&book, &repo, now);
    let offered = if subject_title {
        super::prompt_of(&value)
    } else {
        String::new()
    };
    if subject_title {
        // An issue already open in this checkout keeps the title it was
        // filed under. The subject is for the claim this commit opens.
        let open = book
            .repos
            .get(&repo)
            .is_some_and(|row| row.issue.is_some() && !row.finished);
        if !open {
            let subject = collapse(&offered);
            if !subject.is_empty() && !repo.is_empty() {
                let row = book.repos.entry(repo.clone()).or_default();
                hold_lesson_title(row);
                row.title = Some(subject.chars().take(4000).collect());
            }
        }
    } else if !ending_elsewhere {
        remember_title(&mut book, &repo);
    }
    let commit = is_git_commit(&call.cue);
    // A clean PostToolUse is the commit that just ran. Record the starting
    // HEAD on every other call, including the ask that stopped the command,
    // so the after hook cannot cite the commit the sitting began on.
    // A missing exit code is unknown, never a failure: Claude omits it on
    // a commit that landed, and Codex leaves it in the transcript. Read
    // the baseline before this call can record one.
    let exit = if call.event == "PostToolUse" {
        command_exit(&value)
    } else {
        CommandExit::Absent
    };
    let start = book.repos.get(&repo).and_then(|row| row.start_sha.clone());
    let landed = call.event == "PostToolUse"
        && commit
        && commit_landed(exit, &value, &repo, start.as_deref());
    // A commit that did not land must not become the baseline, and a
    // failed one must not finish the sitting or replace the start already
    // recorded. note_start writes the sha only when this row has none.
    if !landed && !ending_elsewhere {
        note_start(&mut book, &repo);
    }
    if call.event == "PostToolUseFailure"
        || (call.event == "PostToolUse" && command_failed(exit) && !commit)
    {
        note_failure(&mut book, &repo, &failure_text(&value, &call.cue));
    }
    if matches!(call.event.as_str(), "PreToolUse" | "PostToolUse") {
        // A write tool claims even for a new file. A shell redirect claims
        // when it writes a file inside this work tree, tracked or not.
        // A commit claims too, so a sitting that only commits still has a ticket.
        // A harness commit whose command names an open issue claims that
        // issue once it lands, through the git hook's path, not a new one.
        let names_open = commit && !git_hook && named_open_issue(&call.cue).is_some();
        if is_edit(&value, &call.cue)
            || (commit && !names_open)
            || writes_worktree(&repo, &call.cue)
        {
            let offered = subject_title.then_some(offered.as_str());
            let _ = ensure_claim(&mut book, &path, &repo, offered);
        }
        // The commit has not happened on the way in. Cite it on the way
        // out when the evidence says it landed, every new sha since the
        // recorded start. An explicit non-zero exit is the failure. A
        // missing exit, with nothing else that says the commit landed, is
        // not one.
        if landed {
            let named = if git_hook { None } else { named_head(&repo) };
            if let Some((issue, head)) = named {
                // An `ljos_start` row for this checkout comes back first, so
                // the named commit sees the issue the sitting holds.
                if book.repos.get(&repo).is_none_or(|row| row.issue.is_none()) {
                    let _ = take_started_row(&mut book, &path, &repo);
                }
                book_named_in(&mut book, &repo, &repo, &head, &issue);
            } else if let Some(issue) = named_open_issue(&call.cue) {
                // The harness command named an open issue the landed message
                // does not. Book that issue. Filing a new one leaves the
                // named ticket unclaimed and this sitting with a second id.
                let head = head_sha(&repo).unwrap_or_default();
                if !head.is_empty() {
                    if book.repos.get(&repo).is_none_or(|row| row.issue.is_none()) {
                        let _ = take_started_row(&mut book, &path, &repo);
                    }
                    book_named_in(&mut book, &repo, &repo, &head, &issue);
                }
            }
            let _ = ensure_deed(&mut book, &repo);
        } else if call.event == "PostToolUse" && commit && command_failed(exit) {
            note_failure(&mut book, &repo, "git commit exited non-zero");
        }
    }
    if matches!(call.event.as_str(), "Stop" | "SessionEnd") {
        cite_written_commits(&mut book);
        finish_open(&mut book);
        drop_unclaimed_rows(&mut book);
    }
    // A prompt, a read, or a shutdown that never claimed must not leave a
    // row whose issue is null. The commit's before-hook is the exception:
    // it records HEAD so the after-hook can tell the new sha from the one
    // the sitting began on.
    let keep_baseline = commit && !landed;
    if !keep_baseline {
        drop_unclaimed_rows(&mut book);
    }
    let held = if matches!(call.event.as_str(), "SessionStart" | "UserPromptSubmit") {
        book.repos.get(&repo).map(|row| {
            (
                row.issue.clone().filter(|_| !row.finished),
                row.recall.clone(),
            )
        })
    } else {
        None
    };
    if landed && !repo.is_empty() {
        book.last_repo.clone_from(&repo);
    }
    stamp_touch(&mut book, &repo, unix_secs());
    store_book(&path, &book);
    // The book is stored; the pack read below must not hold other hooks
    // on the lock.
    drop(_held);
    match held {
        Some((Some(issue), cached)) => claimed_recall(&issue, &cached),
        Some((None, cached)) => cached,
        None => String::new(),
    }
}

/// The recall a SessionStart or prompt hook hands the model for the issue
/// this checkout holds: the title line and [`super::recall_memories`], the
/// same memories `ljos recall` lists, read now rather than at the claim. A
/// seat with no pack keeps the line stored at the claim.
fn claimed_recall(issue: &str, cached: &str) -> String {
    if super::pack().is_err() {
        return cached.to_string();
    }
    format!(
        "{}{}",
        recall_line(issue),
        super::recall_memories_for_prompt(issue)
    )
}

/// Whether this MCP tool writes a habitat, so the server joins the held issue first.
#[must_use]
pub fn mcp_tool_writes(name: &str) -> bool {
    MCP_WRITES.contains(&name)
}

/// Attach a write tool's call to the issue this seat holds, before it runs.
/// A write tool (`ljos_remember`, `ljos_prefer`, ...) never files or claims
/// an issue: its text titled the claims it used to file. With an issue held
/// in the checkout, by this book or another book of this seat (a hook's
/// edit, `ljos_start`), the call joins that sitting. With none, it runs
/// unattached.
pub fn before_mcp_write(name: &str, args: Option<&serde_json::Map<String, Value>>) {
    if !mcp_tool_writes(name) {
        return;
    }
    let session = sitting_key();
    let explicit = args.and_then(|map| map.get("repo")).and_then(Value::as_str);
    let Some(place) = place_for_claim(explicit) else {
        log_line(&format!("{name}: no checkout here; ran unattached"));
        return;
    };
    {
        let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
        let path = book_path(&session);
        let mut book = load_book(&path);
        let open = book
            .repos
            .get(&place)
            .is_some_and(|row| row.issue.is_some() && !row.finished);
        if open || take_held_issue(&mut book, &path, &place) {
            store_book(&path, &book);
        } else {
            log_line(&format!("{name}: nothing held in {place}; ran unattached"));
        }
    }
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
    let drained = super::drain_tracker_pushes();
    if !drained.trim().is_empty() {
        log_line(drained.trim());
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
    {
        let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
        reap_stale_locked(Some(key.as_str()), now, None);
    }
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

/// How long a sitting may go without a hook call before the next call
/// finishes it. Unset is two hours. `0` and `off` leave it open.
fn sitting_idle() -> Option<Duration> {
    sitting_idle_from(std::env::var("LJOS_SITTING_IDLE").ok().as_deref())
}

fn sitting_idle_from(raw: Option<&str>) -> Option<Duration> {
    let Some(raw) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
        return Some(Duration::from_secs(2 * 60 * 60));
    };
    let lowered = raw.to_ascii_lowercase();
    if matches!(lowered.as_str(), "0" | "off" | "false" | "no") {
        return None;
    }
    match super::reap::parse_idle(raw) {
        Ok(window) if window.is_zero() => None,
        Ok(window) => Some(window),
        Err(_) => Some(Duration::from_secs(2 * 60 * 60)),
    }
}

/// Finish idle rows on the next hook call.
///
/// Each repository has its own clock. A fresh `last_unix` on the book,
/// from a commit in one checkout, does not keep another checkout's row
/// open. `session` and `current` name the book and repository this call
/// is in. A row that predates the per-repository clock is finished when
/// this call is in a different repository: the book's clock was refreshed
/// by that other checkout, not by the row.
pub(crate) fn sweep_idle_sittings(session: &str, current: &str) {
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let current = (!current.is_empty()).then_some(current);
    close_idle_sittings(unix_secs(), Some(session), current);
}

/// Finish rows idle longer than [`sitting_idle`], and drop the finished
/// ones so the next call in that repository files its own claim.
fn close_idle_sittings(now: u64, session: Option<&str>, current: Option<&str>) {
    let Some(limit) = sitting_idle() else {
        return;
    };
    let current_path = session.map(book_path);
    for path in book_files() {
        let mut book = load_book(&path);
        let here = current_path.as_ref().is_some_and(|want| want == &path);
        let hint = if here { current } else { None };
        let idle: Vec<String> = book
            .repos
            .iter()
            .filter(|(repo, row)| row_is_idle(now, &book, repo, row, limit, hint))
            .map(|(repo, _)| repo.clone())
            .collect();
        let unclaimed = book.repos.len();
        let touched = book.last_unix;
        book.repos.retain(|_, row| {
            !unclaimed_row(row) || !idle_due(now, row.last_unix.max(touched), limit)
        });
        let dropped = book.repos.len() != unclaimed;
        if idle.is_empty() {
            if dropped {
                book.cwd_repo
                    .retain(|_, repo| book.repos.contains_key(repo));
                store_book(&path, &book);
            }
            continue;
        }
        finish_rows(&mut book, &idle);
        book.repos.retain(|_, row| !row.finished);
        book.cwd_repo
            .retain(|_, repo| book.repos.contains_key(repo));
        release_spent_prompt(&mut book);
        store_book(&path, &book);
    }
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
    let named = repo.map(str::trim).filter(|path| !path.is_empty());
    let Some(place) = place_for_claim(named) else {
        return Err(if named.is_some() {
            "ljos_start: repo is not a git checkout".into()
        } else {
            "ljos_start: pass repo, a git checkout".into()
        });
    };
    let asked = issue.map(str::trim).filter(|id| !blank_issue(id));
    if asked.is_none() {
        if let Some(said) = start_on_held(&place, &task, &cwd) {
            return Ok(said);
        }
        if let Some(said) = start_on_recorded_hold(&place, &task, &cwd) {
            return Ok(said);
        }
    }
    let (issue_id, filed_here) = match asked {
        Some(id) => {
            claim_existing(id)?;
            (id.to_string(), false)
        }
        None => (file_and_claim(&project_for(&place), &task)?, true),
    };
    let persisted = super::persist_tracker(&issue_id, "claimed");
    if persisted.contains("could not") || persisted.contains("refused") {
        log_line(persisted.trim());
    }
    let recall = format!(
        "{}{}",
        recall_line(&issue_id),
        super::recall_memories_for_prompt(&issue_id)
    );
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let path = book_path(&sitting_key());
    let mut book = load_book(&path);
    let now = unix_secs();
    book.last_unix = now;
    if book.prompt.trim().is_empty() {
        book.prompt = task.chars().take(4000).collect();
        bind_prompt(&mut book, &place, now);
    }
    let row = book.repos.entry(place.clone()).or_default();
    row.last_unix = now;
    let same_issue = row.issue.as_deref() == Some(issue_id.as_str());
    let rebound = row.issue.is_some() && !same_issue;
    clear_issue_fields_if_issue_changes(row, &issue_id);
    row.issue = Some(issue_id.clone());
    // A later start on the issue this row already holds does not change
    // how the sitting got it. Only a rebind resets `named` and `filed`.
    if !same_issue {
        row.filed = filed_here;
        row.named = false;
    }
    row.finished = false;
    row.started = true;
    row.claim_failed = false;
    row.recall.clone_from(&recall);
    if rebound {
        // Commits already made belong to the issue this row just left.
        // HEAD now is the baseline, so the next citation starts after them.
        row.start_sha = head_sha(&place).ok();
    }
    if !cwd.is_empty() {
        book.cwd_repo.insert(cwd, place);
    }
    store_book(&path, &book);
    drop(_held);
    Ok(format!("claimed {issue_id}\n{recall}"))
}

fn claim_existing(issue: &str) -> Result<(), String> {
    let assignee =
        super::claim_holder(&super::resolve_assignee(None)).map_err(|e| e.to_string())?;
    if which::which("claimdag").is_ok() && super::claim(issue, &assignee).is_ok() {
        return Ok(());
    }
    let first = run_timed(
        "vissue",
        &["claim", issue],
        &[("VISSUE_AGENT", assignee.as_str())],
    );
    match first {
        Ok(_) => Ok(()),
        // The same seat under another name (the hook's tag, the server's
        // bare seat) holds it: this conversation takes its own claim back.
        Err(e)
            if e.to_string().contains("pass --force")
                && tracker_holder(issue).is_some_and(|by| {
                    super::same_seat_here(&by, &assignee) || booking_alias(&by, &assignee)
                }) =>
        {
            run_timed(
                "vissue",
                &["claim", issue, "--force"],
                &[("VISSUE_AGENT", assignee.as_str())],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
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
    let named = super::hook_actor().or_else(|| {
        std::env::var("LJOS_SEAT")
            .ok()
            .map(|raw| raw.trim().to_string())
            .filter(|raw| !raw.is_empty())
    });
    if let Some(name) = named {
        if !super::is_raw_subagent_name(&name) {
            let slug = super::canonical_seat_name(&name);
            if !slug.is_empty() && slug != "runner" && !super::omitted_actor_name(&slug) {
                return format!("seat:{slug}");
            }
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

fn env_session(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn session_key(call: &super::HookCall) -> String {
    call.session
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| env_session("LJOS_SESSION_ID"))
        .or_else(|| env_session("OPENCODE_SESSION_ID"))
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
        return;
    }
    // The directory of books is the slow lookup. The file names the book
    // that holds each live issue, so the next claim reads one path.
    sync_claim_index(path, book);
}

/// Append one line to the seat's `book.log` under the runtime directory.
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

/// Where a new claim is filed. An explicit path is used only when it is a
/// checkout: it does not fall back to the working directory. With no path,
/// the last repository this seat's git hook booked, then the working
/// directory when that is a checkout.
fn place_for_claim(explicit: Option<&str>) -> Option<String> {
    place_for_claim_in(
        explicit,
        last_booked_repo().as_deref(),
        std::env::current_dir().ok().as_deref(),
    )
}

fn place_for_claim_in(
    explicit: Option<&str>,
    last: Option<&str>,
    cwd: Option<&Path>,
) -> Option<String> {
    if let Some(path) = explicit.map(str::trim).filter(|path| !path.is_empty()) {
        return git_toplevel(Path::new(path));
    }
    if let Some(path) = last.map(str::trim).filter(|path| !path.is_empty()) {
        if let Some(top) = git_toplevel(Path::new(path)) {
            return Some(top);
        }
    }
    cwd.and_then(git_toplevel)
}

/// The checkout the git hook last booked for this seat.
fn last_booked_repo() -> Option<String> {
    let book = load_book(&book_path(&sitting_key()));
    let repo = book.last_repo.trim();
    if repo.is_empty() {
        None
    } else {
        Some(repo.to_string())
    }
}

/// The book key for this seat, shared by the git hook and the MCP server.
#[must_use]
pub fn seat_session() -> String {
    sitting_key()
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

fn ensure_claim(
    book: &mut Book,
    own: &Path,
    repo: &str,
    offered: Option<&str>,
) -> Result<(), String> {
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
    // The seat may already hold an issue in this checkout under another
    // book: `ljos_start` in the MCP server keys its book by the seat, the
    // harness hook by the harness session. That issue is this sitting's.
    if take_held_issue(book, own, repo) {
        return Ok(());
    }
    // A hold from another repository is a different ticket. This sitting
    // files its own claim for the repository the tool just wrote.
    // A git hook passes the commit subject when the saved prompt is not
    // this sitting's.
    let title = match offered.map(str::trim).filter(|text| !text.is_empty()) {
        Some(subject) => title_from(subject, repo),
        None => title_from(&book.prompt, repo),
    };
    let project = project_for(repo);
    match file_and_claim(&project, &title) {
        Ok(issue) => {
            let line = super::persist_tracker(&issue, "claimed");
            if line.contains("could not") || line.contains("refused") {
                log_line(line.trim());
            }
            let row = book.repos.entry(repo.to_string()).or_default();
            clear_issue_fields_if_issue_changes(row, &issue);
            row.issue = Some(issue.clone());
            row.filed = true;
            row.named = false;
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

/// `ljos_start` with no issue, when this seat already holds one in the
/// checkout `place`: a hook claimed it for an edit, or an earlier start
/// did. That row moves to the server's book, marked as started, so an
/// issue-less finish closes it and the next hook call takes it back. The
/// task goes on the issue as a note when it differs from the title the
/// hook filed it under. `None` when nothing is held there.
fn start_on_held(place: &str, task: &str, cwd: &str) -> Option<String> {
    let held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let own = book_path(&sitting_key());
    let mut found: Option<((u64, u128), PathBuf, String, String)> = None;
    for path in book_files() {
        let other = load_book(&path);
        let rank = row_rank(&path, &other);
        for (key, row) in &other.repos {
            let Some(issue) = row.issue.as_deref() else {
                continue;
            };
            if row.finished || !same_checkout(key, place) {
                continue;
            }
            if found.as_ref().is_some_and(|(when, ..)| *when > rank) {
                continue;
            }
            if this_seat_booked(issue) {
                found = Some((rank, path.clone(), key.clone(), issue.to_string()));
            }
        }
    }
    let (_, path, key, issue) = found?;
    let mut book = load_book(&own);
    let moved = if path == own {
        book.repos.remove(&key).unwrap_or_default()
    } else {
        let mut other = load_book(&path);
        let moved = other.repos.remove(&key).unwrap_or_default();
        store_book(&path, &other);
        moved
    };
    let filed = moved.filed;
    let now = unix_secs();
    book.last_unix = now;
    if book.prompt.trim().is_empty() {
        book.prompt = task.chars().take(4000).collect();
        bind_prompt(&mut book, place, now);
    }
    adopt_row(&mut book, place, &issue, moved);
    let row = book.repos.entry(place.to_string()).or_default();
    row.started = true;
    row.last_unix = now;
    if !cwd.is_empty() {
        book.cwd_repo.insert(cwd.to_string(), place.to_string());
    }
    store_book(&own, &book);
    drop(held);
    log_line(&format!("start: kept {issue}, already held in {place}"));
    if filed && !same_words(task, &issue_title(&issue)) {
        let note = format!("ljos_start task: {task}");
        match run_timed("vissue", &["note", &issue, &note], &[]) {
            Ok(_) => {
                let line = super::persist_tracker(&issue, "noted");
                if line.contains("could not") || line.contains("refused") {
                    log_line(line.trim());
                }
            }
            Err(e) => log_line(&format!("start: note on {issue}: {e}")),
        }
    }
    let recall = format!(
        "{}{}",
        recall_line(&issue),
        super::recall_memories_for_prompt(&issue)
    );
    Some(format!("claimed {issue}\n{recall}"))
}

/// `ljos_start` when this seat already holds an issue for the checkout and
/// no book row names it: `ljos claim` left a hold record. Filing a second
/// issue would put two STARTED tickets on one sitting.
fn start_on_recorded_hold(place: &str, task: &str, cwd: &str) -> Option<String> {
    let project = project_for(place);
    let issue = super::held_issue().filter(|id| {
        id.rsplit_once('-').is_some_and(|(p, _)| p == project) && this_seat_booked(id)
    })?;
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let own = book_path(&sitting_key());
    if owned_elsewhere(&own, &issue) {
        return None;
    }
    let mut book = load_book(&own);
    let now = unix_secs();
    book.last_unix = now;
    if book.prompt.trim().is_empty() {
        book.prompt = task.chars().take(4000).collect();
        bind_prompt(&mut book, place, now);
    }
    adopt_row(&mut book, place, &issue, RepoBook::default());
    let row = book.repos.entry(place.to_string()).or_default();
    row.started = true;
    row.last_unix = now;
    if !cwd.is_empty() {
        book.cwd_repo.insert(cwd.to_string(), place.to_string());
    }
    store_book(&own, &book);
    drop(_held);
    log_line(&format!("start: kept {issue}, already held in {place}"));
    let recall = format!(
        "{}{}",
        recall_line(&issue),
        super::recall_memories_for_prompt(&issue)
    );
    Some(format!("claimed {issue}\n{recall}"))
}

/// The issue a note attaches to. A blank argument, or prose that is not an
/// id, is the live issue this seat holds. A note never files an issue, so
/// there is no title to take from the note text.
///
/// # Errors
///
/// Nothing is held, and the argument is not an issue id.
pub fn note_issue(raw: Option<&str>) -> Result<String, String> {
    if let Some(id) = raw.map(str::trim).filter(|id| is_issue_id(id)) {
        return Ok(id.to_string());
    }
    if let Some(id) = claimed_issue(None).filter(|id| this_seat_booked(id) || held_by_this_seat(id))
    {
        return Ok(id);
    }
    if let Some(id) = live_issue_for_seat() {
        return Ok(id);
    }
    Err(
        "nothing is held to note on: pass issue, or ljos_start first; a note never files an issue"
            .into(),
    )
}

/// The newest live issue this seat holds, in any book. `claimed_issue`
/// only sees the working directory's book, and a hook's checkout is not
/// always this process's cwd.
fn live_issue_for_seat() -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    for path in book_files() {
        let book = load_book(&path);
        for row in book.repos.values() {
            let Some(issue) = row.issue.as_deref() else {
                continue;
            };
            if row.finished || !this_seat_booked(issue) {
                continue;
            }
            let rank = row.last_unix.max(book.last_unix);
            let take = best.as_ref().is_none_or(|(when, _)| rank >= *when);
            if take {
                best = Some((rank, issue.to_string()));
            }
        }
    }
    best.map(|(_, issue)| issue)
}

/// The title the tracker holds for `issue`, or nothing.
fn issue_title(issue: &str) -> String {
    super::issue_record(issue)
        .ok()
        .and_then(|v| v["title"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Whether two titles say the same words, case and punctuation aside.
fn same_words(a: &str, b: &str) -> bool {
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    words(a) == words(b)
}

/// Put the issue this seat already holds in `repo` on this book's row, and
/// say whether there was one. A row another book keeps for the checkout
/// (`ljos_start`, a named commit) moves here whole, so one book finishes
/// it and the other does not finish it a second time without its deeds.
/// Else a claim the seat's hold record names in this checkout's project
/// (`ljos claim`) is taken as the row's issue. Either issue has to be
/// STARTED in the tracker under this holder, in either form: the bare
/// seat or `SEAT@TAG`.
///
/// A hold record is shared by every conversation of the runner. A live
/// row in another conversation's book stays there, whether that row was
/// filed or adopted. Only a started sitting moves, through
/// [`take_started_row`]. The hook already holds `book.lock`; taking it
/// again on this thread would wait on itself, so the decision and the
/// write share [`claim_lock`].
fn take_held_issue(book: &mut Book, own: &Path, repo: &str) -> bool {
    let _claim = claim_lock();
    if take_started_row(book, own, repo) {
        store_book(own, book);
        return true;
    }
    let project = project_for(repo);
    let Some(issue) = super::held_issue().filter(|id| {
        id.rsplit_once('-').is_some_and(|(p, _)| p == project) && this_seat_booked(id)
    }) else {
        return false;
    };
    if owned_elsewhere(own, &issue) {
        return false;
    }
    adopt_row(book, repo, &issue, RepoBook::default());
    store_book(own, book);
    log_line(&format!("claim: kept {issue}, held by this seat"));
    true
}

/// Exclusive lock for the adopt decision. Distinct from `book.lock`,
/// which the caller may already hold.
fn claim_lock() -> super::CommitLock {
    super::CommitLock::acquire(&super::runtime_dir().join("claim.lock"))
}

/// How many times the claim check read the book directory. The indexed
/// book and the seat's book answer without that read.
#[cfg(test)]
static CLAIM_DIR_SCANS: AtomicU64 = AtomicU64::new(0);

/// The file that names the book holding `issue`.
fn claim_index(issue: &str) -> PathBuf {
    super::runtime_dir().join(format!("claim-at-{}", super::session_tag(issue)))
}

fn remember_owner(issue: &str, path: &Path) {
    let index = claim_index(issue);
    if let Some(dir) = index.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(index, format!("{}\n", path.display()));
}

fn remembered_owner(issue: &str) -> Option<PathBuf> {
    let text = std::fs::read_to_string(claim_index(issue)).ok()?;
    let path = text.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn sync_claim_index(path: &Path, book: &Book) {
    for row in book.repos.values() {
        let Some(issue) = row
            .issue
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        if row.finished {
            if remembered_owner(issue).as_deref() == Some(path) {
                let _ = std::fs::remove_file(claim_index(issue));
            }
        } else {
            remember_owner(issue, path);
        }
    }
}

/// A row that still holds `issue`. Filed and adopted rows both count;
/// a finished row does not.
fn live_claim(row: &RepoBook, issue: &str) -> bool {
    !row.finished && row.issue.as_deref() == Some(issue)
}

fn book_holds_live(path: &Path, issue: &str) -> bool {
    path.is_file()
        && load_book(path)
            .repos
            .values()
            .any(|row| live_claim(row, issue))
}

/// Whether another conversation's book holds `issue` live.
///
/// The index written with the book is one file. The seat's book is the
/// other path this process can name without listing the directory. The
/// directory is read only when neither of those holds the issue.
fn owned_elsewhere(own: &Path, issue: &str) -> bool {
    if let Some(path) = remembered_owner(issue) {
        if path != own && book_holds_live(&path, issue) {
            return true;
        }
    }
    let seat = book_path(&sitting_key());
    if seat != own && book_holds_live(&seat, issue) {
        remember_owner(issue, &seat);
        return true;
    }
    #[cfg(test)]
    CLAIM_DIR_SCANS.fetch_add(1, Ordering::Relaxed);
    for path in book_files() {
        if path != own && path != seat && book_holds_live(&path, issue) {
            remember_owner(issue, &path);
            return true;
        }
    }
    false
}

/// The book scan of [`take_held_issue`]: an `ljos_start` row another book
/// keeps for the checkout moves to `book`.
fn take_started_row(book: &mut Book, own: &Path, repo: &str) -> bool {
    let mut found: Option<((u64, u128), PathBuf, String, String)> = None;
    for path in book_files() {
        if path == own {
            continue;
        }
        let other = load_book(&path);
        let rank = row_rank(&path, &other);
        for (key, row) in &other.repos {
            let Some(issue) = row.issue.as_deref() else {
                continue;
            };
            if row.finished || !row.started || !same_checkout(key, repo) {
                continue;
            }
            if found.as_ref().is_some_and(|(when, ..)| *when > rank) {
                continue;
            }
            if this_seat_booked(issue) {
                found = Some((rank, path.clone(), key.clone(), issue.to_string()));
            }
        }
    }
    if let Some((_, path, key, issue)) = found {
        let mut other = load_book(&path);
        let moved = other.repos.remove(&key).unwrap_or_default();
        store_book(&path, &other);
        adopt_row(book, repo, &issue, moved);
        log_line(&format!("claim: kept {issue}, already held in {repo}"));
        return true;
    }
    false
}

/// Whether the tracker has `issue` STARTED and claimed by this seat. The
/// callers look at one checkout, so a holder of this seat under another
/// tag is this conversation: the git hook books `SEAT@TAG` (`SEAT-TAG`
/// before 0.32) and `ljos_start` books the bare seat ([`super::same_seat`]).
/// A tracker that records no holder answers by the state alone.
fn held_by_this_seat(issue: &str) -> bool {
    let Ok(v) = super::issue_record(issue) else {
        return false;
    };
    if v["id"].as_str() != Some(issue) || v["state"].as_str() != Some("STARTED") {
        return false;
    }
    match v["claimed_by"].as_str().map(str::trim) {
        Some(by) if !by.is_empty() => super::same_seat_here(by, &super::holder_name()),
        _ => true,
    }
}

/// [`held_by_this_seat`], and also the two holder strings one sitting
/// stamps: the hook's `SEAT@TAG` and the MCP server's raw conversation id.
/// Those are not the same seat to [`super::same_seat`], so a write after
/// `ljos_start` filed a second issue. A different seat's name still does not
/// match.
fn this_seat_booked(issue: &str) -> bool {
    if held_by_this_seat(issue) {
        return true;
    }
    let Some(by) = tracker_holder(issue) else {
        return false;
    };
    let Ok(v) = super::issue_record(issue) else {
        return false;
    };
    v["state"].as_str() == Some("STARTED") && booking_alias(&by, &super::holder_name())
}

fn seat_slug_now() -> Option<String> {
    let named = super::hook_actor().or_else(|| {
        std::env::var("LJOS_SEAT")
            .ok()
            .map(|raw| raw.trim().to_string())
            .filter(|raw| !raw.is_empty())
    })?;
    let slug = super::seat_slug(&named);
    if slug == "runner" || super::omitted_actor_name(&slug) {
        None
    } else {
        Some(slug)
    }
}

/// A conversation id a runner stamped, as opposed to a bare seat name.
/// It carries a digit, so `someone-else` is not one and `01a0dab3-…` is.
fn raw_conversation(name: &str) -> bool {
    let name = name.trim();
    name.len() >= 8
        && !name.contains('@')
        && super::holder_parts(name).1.is_none()
        && name.chars().any(|c| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Whether `claimed` and `holder` are the hook's name and the MCP server's
/// name for one sitting. [`super::same_seat`] already joins `SEAT` and
/// `SEAT@TAG`. This joins either of those with the raw conversation id the
/// server stamps when `LJOS_SESSION_ID` is the only session variable, or
/// when `LJOS_HOLD` is that id.
fn booking_alias(claimed: &str, holder: &str) -> bool {
    if super::same_holder(claimed, holder) || super::same_seat_here(claimed, holder) {
        return true;
    }
    let Some(seat) = seat_slug_now() else {
        return false;
    };
    let seat_of = |name: &str| super::seat_slug(&super::holder_parts(name).0);
    (raw_conversation(claimed) && seat_of(holder) == seat)
        || (raw_conversation(holder) && seat_of(claimed) == seat)
}

/// The tracker's holder of `issue`, when the record names one.
fn tracker_holder(issue: &str) -> Option<String> {
    super::issue_record(issue).ok().and_then(|v| {
        v["claimed_by"]
            .as_str()
            .map(str::trim)
            .filter(|by| !by.is_empty())
            .map(str::to_string)
    })
}

/// Fold `moved`, another book's row for the checkout, into this book's
/// row, which then holds `issue`.
fn adopt_row(book: &mut Book, repo: &str, issue: &str, moved: RepoBook) {
    let row = book.repos.entry(repo.to_string()).or_default();
    let rebound = row.issue.as_deref().is_some_and(|old| old != issue);
    clear_issue_fields_if_issue_changes(row, issue);
    row.issue = Some(issue.to_string());
    row.finished = false;
    row.claim_failed = false;
    row.recall = if moved.recall.is_empty() {
        recall_line(issue)
    } else {
        moved.recall
    };
    if rebound {
        row.start_sha = head_sha(repo).ok();
    } else if moved.start_sha.is_some() {
        row.start_sha = moved.start_sha;
    }
    if row.title.is_none() {
        row.title = moved.title;
    }
    for sha in moved.deeds {
        if !row.deeds.contains(&sha) {
            row.deeds.push(sha);
        }
    }
    for sha in moved.landed {
        if !row.landed.contains(&sha) {
            row.landed.push(sha);
        }
    }
    row.failures.extend(moved.failures);
    row.asks.extend(moved.asks);
    row.named |= moved.named;
    row.filed = moved.filed;
    for other in moved.also {
        if !row.also.contains(&other) {
            row.also.push(other);
        }
    }
    for sha in moved.elsewhere {
        if !row.elsewhere.contains(&sha) {
            row.elsewhere.push(sha);
        }
    }
}

fn file_and_claim(project: &str, title: &str) -> Result<String, String> {
    let assignee =
        super::claim_holder(&super::resolve_assignee(None)).map_err(|e| e.to_string())?;
    let said = run_timed(
        "vissue",
        &["create", "-p", project, title, "-q"],
        &[("VISSUE_AGENT", assignee.as_str())],
    )
    .map_err(|e| e.to_string())?;
    let issue = parse_issue_id(&said.stdout)
        .ok_or_else(|| format!("vissue create printed no issue id: {}", said.stdout.trim()))?;
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
    cite_row(book, repo, repo)
}

/// Cite every commit that landed after the row's baseline. A compound
/// command can write more than one. No baseline means this post is the
/// first time the sitting saw the repository, so the sha is the one git wrote.
fn cite_row(book: &mut Book, row_key: &str, git_repo: &str) -> Result<(), String> {
    cite_row_where(book, row_key, git_repo, |_| true)
}

/// [`cite_row`] for the landed shas `keep` accepts.
fn cite_row_where(
    book: &mut Book,
    row_key: &str,
    git_repo: &str,
    keep: impl Fn(&str) -> bool,
) -> Result<(), String> {
    let head = match head_sha(git_repo) {
        Ok(sha) => sha,
        Err(e) => {
            log_line(&format!("deed: {e}"));
            return Ok(());
        }
    };
    let start = book
        .repos
        .get(row_key)
        .and_then(|row| row.start_sha.clone());
    let Some(issue) = book.repos.get(row_key).and_then(|row| row.issue.clone()) else {
        return Ok(());
    };
    for sha in commits_since(git_repo, start.as_deref(), &head)
        .into_iter()
        .filter(|sha| keep(sha))
    {
        if book.repos.get(row_key).is_some_and(|row| {
            row.deeds
                .iter()
                .chain(&row.elsewhere)
                .any(|seen| seen == &sha)
        }) {
            continue;
        }
        match cite_deed(&issue, &sha) {
            Ok(()) => {
                book.repos
                    .entry(row_key.to_string())
                    .or_default()
                    .deeds
                    .push(sha);
            }
            Err(e) => {
                log_line(&format!("deed: {e}"));
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Commits strictly after `start`, oldest first. Without a baseline, the
/// current HEAD is the commit this post is about.
fn commits_since(repo: &str, start: Option<&str>, head: &str) -> Vec<String> {
    let Some(start) = start.map(str::trim).filter(|sha| !sha.is_empty()) else {
        return vec![head.to_string()];
    };
    if start == head {
        return Vec::new();
    }
    let range = format!("{start}..{head}");
    let shas = run_timed("git", &["-C", repo, "rev-list", "--reverse", &range], &[])
        .ok()
        .map(|said| {
            said.stdout
                .lines()
                .map(str::trim)
                .filter(|sha| sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit()))
                .map(str::to_string)
                .collect::<Vec<_>>()
        });
    match shas {
        Some(shas) if !shas.is_empty() => shas,
        _ => vec![head.to_string()],
    }
}

/// Cite each open row's commits that no post named: a script or an alias
/// commits with no `git commit` on the command line, and a commit chained
/// before a step that fails exits non-zero. Only the shas HEAD's reflog
/// says this checkout wrote since the row's start count, so a pull, a
/// merge, or a branch switch brings in no deed.
fn cite_written_commits(book: &mut Book) {
    let rows: Vec<(String, String)> = book
        .repos
        .iter()
        .filter(|(_, row)| row.issue.is_some() && !row.finished)
        .filter_map(|(key, row)| Some((key.clone(), row.start_sha.clone()?)))
        .collect();
    for (repo, start) in rows {
        let written = written_since(&repo, &start);
        if !written.is_empty() {
            let _ = cite_row_where(book, &repo, &repo, |sha| written.iter().any(|w| w == sha));
        }
    }
}

/// Shas HEAD's reflog records as a commit, a cherry-pick, or a revert
/// since HEAD was last at `start`, newest first. Empty when the reflog
/// does not reach `start`: an expired or disabled reflog cites nothing.
fn written_since(repo: &str, start: &str) -> Vec<String> {
    let Ok(said) = run_timed(
        "git",
        &[
            "-C",
            repo,
            "reflog",
            "show",
            "-n",
            "200",
            "--format=%H %gs",
            "HEAD",
        ],
        &[],
    ) else {
        return Vec::new();
    };
    let mut written = Vec::new();
    for line in said.stdout.lines() {
        let (sha, subject) = line.split_once(' ').unwrap_or((line, ""));
        if sha == start.trim() {
            return written;
        }
        let verb = subject.split([':', ' ']).next().unwrap_or("");
        if matches!(verb, "commit" | "cherry-pick" | "revert") {
            written.push(sha.to_string());
        }
    }
    Vec::new()
}

/// Cite `repo`'s new commits on the open sitting that already holds it.
/// A git hook books here instead of opening a seat book whose start is the
/// commit that just landed. False when no sitting holds the checkout.
pub(crate) fn cite_into_open_sitting(repo: &str) -> bool {
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let Some((path, key)) = open_row_for_repo(repo) else {
        return false;
    };
    let mut book = load_book(&path);
    let open = book
        .repos
        .get(&key)
        .is_some_and(|row| !row.finished && row.issue.is_some());
    if !open {
        return false;
    }
    // The failure notes and the baseline stay. A retry cites the new sha
    // on this issue; it does not finish the row or replace its start.
    let _ = cite_row(&mut book, &key, repo);
    stamp_touch(&mut book, &key, unix_secs());
    store_book(&path, &book);
    true
}

fn open_row_for_repo(repo: &str) -> Option<(PathBuf, String)> {
    let mut best: Option<((u64, u128), PathBuf, String)> = None;
    for path in book_files() {
        let book = load_book(&path);
        let rank = row_rank(&path, &book);
        for (key, row) in &book.repos {
            if row.finished || row.issue.is_none() || !same_checkout(key, repo) {
                continue;
            }
            let take = best.as_ref().is_none_or(|(when, _, _)| rank >= *when);
            if take {
                best = Some((rank, path.clone(), key.clone()));
            }
        }
    }
    best.map(|(_, path, key)| (path, key))
}

fn row_rank(path: &Path, book: &Book) -> (u64, u128) {
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|when| when.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    (book.last_unix, modified)
}

fn same_checkout(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
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
        &[("VISSUE_AGENT", seat.as_str())],
    )
    .map_err(|e| e.to_string())?;
    let accession = parse_deed(&said.stdout)
        .ok_or_else(|| format!("deedar printed no accession: {}", said.stdout.trim()))?;
    run_timed(
        "vissue",
        &["deed", issue, "--add", &accession],
        &[("VISSUE_AGENT", seat.as_str())],
    )
    .map_err(|e| e.to_string())?;
    let line = super::persist_tracker(issue, "cited a deed");
    if line.contains("could not") || line.contains("refused") {
        return Err(line.trim().to_string());
    }
    Ok(())
}

/// Drop the citation of `sha` from `issue`. Absent is success: an amend
/// that already moved the deed must not fail the hook.
fn drop_citation(issue: &str, sha: &str) -> Result<(), String> {
    let seat = super::holder_name();
    let id = deed_id(sha);
    run_timed(
        "vissue",
        &["deed", issue, "--remove", &id],
        &[("VISSUE_AGENT", seat.as_str())],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn null_sha(sha: &str) -> bool {
    !sha.is_empty() && sha.chars().all(|c| c == '0')
}

struct LessonRewrite {
    issue: String,
    title: String,
    deeds: Vec<String>,
    failures: Vec<String>,
    asks: Vec<String>,
    replaces: Option<String>,
}

/// Move citations off commits that an amend or a rebase replaced.
/// An all-zero new sha drops the old citation and cites nothing.
/// A sha already in the book is not cited again.
pub(crate) fn repoint_commits(repo: &str, pairs: &[(String, String)]) {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .filter(|(old, new)| old != new && !old.is_empty() && !new.is_empty())
        .cloned()
        .collect();
    if pairs.is_empty() || repo.is_empty() {
        return;
    }
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    for path in book_files() {
        let mut book = load_book(&path);
        let keys: Vec<String> = book
            .repos
            .iter()
            .filter(|(key, row)| row.issue.is_some() && same_checkout(key, repo))
            .map(|(key, _)| key.clone())
            .collect();
        let mut changed = false;
        let mut rewrites = Vec::new();
        for key in keys {
            if let Some(rewrite) = repoint_row(&mut book, &key, &pairs) {
                changed = true;
                if let Some(rewrite) = rewrite {
                    rewrites.push(rewrite);
                }
            }
        }
        if changed {
            store_book(&path, &book);
        }
        for rewrite in rewrites {
            match write_lesson(
                &rewrite.issue,
                &rewrite.title,
                &rewrite.deeds,
                &rewrite.failures,
                &rewrite.asks,
                rewrite.replaces.as_deref(),
            ) {
                Ok(id) => {
                    if let Some(row) = book
                        .repos
                        .values_mut()
                        .find(|row| row.issue.as_deref() == Some(rewrite.issue.as_str()))
                    {
                        lesson_written(row, &rewrite.title, id);
                    }
                    store_book(&path, &book);
                }
                Err(e) => log_line(&format!("lesson: {e}")),
            }
        }
    }
}

/// `Some(None)` when the deeds moved and no lesson was waiting.
/// `Some(Some(...))` when a finished lesson has to be written again.
fn repoint_row(
    book: &mut Book,
    key: &str,
    pairs: &[(String, String)],
) -> Option<Option<LessonRewrite>> {
    let row = book.repos.get(key)?;
    let issue = row.issue.clone()?;
    let mut deeds = row.deeds.clone();
    let mut touched = false;
    for (old, new) in pairs {
        if !deeds.iter().any(|sha| sha == old) {
            continue;
        }
        deeds.retain(|sha| sha != old);
        if let Err(e) = drop_citation(&issue, old) {
            log_line(&format!("deed: {e}"));
        }
        touched = true;
        if !null_sha(new) && !deeds.iter().any(|sha| sha == new) {
            match cite_deed(&issue, new) {
                Ok(()) => deeds.push(new.clone()),
                Err(e) => log_line(&format!("deed: {e}")),
            }
        }
    }
    if !touched {
        return None;
    }
    let rewrite = {
        let row = book.repos.get(key)?;
        if row.finished && row.lesson {
            let title = lesson_title_of(book, key, row);
            Some(LessonRewrite {
                issue: issue.clone(),
                title,
                deeds: deeds.clone(),
                failures: row.failures.clone(),
                asks: row.asks.clone(),
                replaces: row.lesson_id.clone(),
            })
        } else {
            None
        }
    };
    if let Some(row) = book.repos.get_mut(key) {
        row.deeds.clone_from(&deeds);
        row.landed.retain(|sha| deeds.contains(sha));
        row.landed_head = None;
        if rewrite.is_some() {
            row.lesson = false;
        }
    }
    Some(rewrite)
}

/// A short-lived hook must not replace the runner that owns the sitting.
/// Session start and a new prompt are that runner. A pid of zero has never
/// been set, so the first call of any kind may set it. Each owner is a pid
/// and its start time.
fn next_owner(event: &str, current: (u32, u64), incoming: (u32, u64)) -> (u32, u64) {
    if current.0 == 0 || matches!(event, "SessionStart" | "UserPromptSubmit") {
        incoming
    } else {
        current
    }
}

/// The runner that spawned this hook, as its pid and start time. When it
/// is gone, the next session finishes the claims this book still has
/// open. A runner starts the hook command through `sh -c` or a wrapper
/// that exits with the hook, so the parent alone is gone by the next
/// call. A terminal, a multiplexer or init outlives every sitting under
/// it and never owns one: when one of those ran the hook itself, the hook
/// is the owner, and its sitting is orphaned once it exits. With no
/// `/proc` (macOS) the walk is empty and the parent is the owner.
fn hook_owner() -> (u32, u64) {
    let chain = super::proc_tree::ancestors();
    if chain.is_empty() {
        // SAFETY: getppid only reads the parent pid.
        return (unsafe { libc::getppid() as u32 }, 0);
    }
    super::proc_tree::runner(&chain)
        .cloned()
        .or_else(|| super::proc_tree::of(std::process::id()))
        .map_or((std::process::id(), 0), |owner| (owner.pid, owner.start))
}

/// Whether the runner that owns `book` still runs: the same pid with the
/// same start time, and not a zombie. A book with no start time checks
/// the pid alone.
fn owner_alive(book: &Book) -> bool {
    if book.owner_start == 0 {
        return super::pid_alive(book.owner_pid);
    }
    super::proc_tree::alive(&super::proc_tree::Proc {
        pid: book.owner_pid,
        start: book.owner_start,
        comm: String::new(),
    })
}
/// How long the sweeps over past sittings may take in one hook call. They
/// walk every book the seat has kept, and the landed check runs git once per
/// cited commit, so the walk grows with the seat's history. Books the walk
/// does not reach are the next call's, most recently written first.
const SWEEP_BUDGET: std::time::Duration = std::time::Duration::from_millis(1000);

/// Finish every other sitting whose runner has exited, and every row that
/// has sat past [`sitting_idle`]. A timeout kills the runner (exit 124)
/// before Stop and often leaves the parent pid alive, so liveness alone
/// never releases the claim. Age does. A sitting that never claimed still
/// counts when a tool failed or the guard asked and the lesson has not
/// been written. The caller holds `book.lock`.
fn finish_orphans(current: &str, deadline: std::time::Instant) {
    reap_stale_locked(Some(current), unix_secs(), Some(deadline));
}

fn reap_stale_locked(current: Option<&str>, now: u64, deadline: Option<std::time::Instant>) {
    let keep = current.map(book_path);
    let limit = sitting_idle();
    for path in book_files() {
        if keep.as_ref().is_some_and(|want| want == &path) {
            continue;
        }
        if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            break;
        }
        let mut book = load_book(&path);
        let open = book.repos.values().any(sitting_open);
        if !open {
            continue;
        }
        let dead = book.owner_pid != 0 && !owner_alive(&book);
        if dead {
            finish_open(&mut book);
            store_book(&path, &book);
            continue;
        }
        let Some(limit) = limit else {
            continue;
        };
        let idle: Vec<String> = book
            .repos
            .iter()
            .filter(|(repo, row)| row_is_idle(now, &book, repo, row, limit, None))
            .map(|(repo, _)| repo.clone())
            .collect();
        if idle.is_empty() {
            continue;
        }
        finish_rows(&mut book, &idle);
        book.repos.retain(|_, row| !row.finished);
        book.cwd_repo
            .retain(|_, repo| book.repos.contains_key(repo));
        release_spent_prompt(&mut book);
        store_book(&path, &book);
    }
}

/// Replace a cited commit that is not in HEAD with the commit that is, and
/// write the lesson again when it already named the one that did not land.
fn refresh_landed_deeds(deadline: std::time::Instant) {
    let mut heads = Heads::new();
    for path in book_files() {
        if std::time::Instant::now() >= deadline {
            break;
        }
        let mut book = load_book(&path);
        let repos: Vec<String> = book.repos.keys().cloned().collect();
        let mut changed = false;
        let mut cached = false;
        for repo in repos {
            changed |= reconcile_deeds(&mut book, &repo, deadline, &mut heads, &mut cached);
        }
        if !changed {
            if cached {
                store_book(&path, &book);
            }
            continue;
        }
        let repos: Vec<String> = book.repos.keys().cloned().collect();
        for repo in repos {
            let Some(row) = book.repos.get(&repo) else {
                continue;
            };
            if !row.finished || row.lesson {
                continue;
            }
            let Some(issue) = row.issue.clone() else {
                continue;
            };
            let title = lesson_title_of(&book, &repo, row);
            let deeds = row.deeds.clone();
            let failures = row.failures.clone();
            let asks = row.asks.clone();
            let replaces = row.lesson_id.clone();
            match write_lesson(
                &issue,
                &title,
                &deeds,
                &failures,
                &asks,
                replaces.as_deref(),
            ) {
                Ok(id) => {
                    if let Some(row) = book.repos.get_mut(&repo) {
                        lesson_written(row, &title, id);
                    }
                }
                Err(e) => log_line(&format!("lesson: {e}")),
            }
        }
        store_book(&path, &book);
    }
}

/// Unfinished claims already on disk, for a prompt that has not waited on
/// the pack. Empty when none are held, so a quiet seat does not say `none`.
#[must_use]
pub fn prompt_claims() -> String {
    let mut ids = Vec::new();
    for path in book_files() {
        let book = load_book(&path);
        for row in book.repos.values() {
            if row.finished {
                continue;
            }
            if let Some(issue) = row
                .issue
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                ids.push(issue.to_string());
            }
        }
    }
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        String::new()
    } else {
        format!("claims: {}", ids.join(" "))
    }
}

/// The books, most recently written first: a sitting that ended a moment ago
/// is the one a sweep is likely to find work in.
fn book_files() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(super::runtime_dir()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("book-") && name.ends_with(".json") {
            let written = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            out.push((written, entry.path()));
        }
    }
    out.sort_by_key(|row| std::cmp::Reverse(row.0));
    out.into_iter().map(|(_, path)| path).collect()
}

/// HEAD of each checkout, asked once per sweep: the books of a seat share a
/// few checkouts, and a row's cached answer stands while its HEAD does.
type Heads = std::collections::HashMap<String, Option<String>>;

/// Drop deed shas that are not ancestors of HEAD, then cite HEAD when the
/// sitting had cited one that never landed. `false` when nothing changed,
/// or when git could not say. The shas found in HEAD are remembered with
/// the HEAD they were found in, so a later sweep asks git again only when
/// HEAD has moved (an amend, a rebase); `cached` says a row grew.
fn reconcile_deeds(
    book: &mut Book,
    repo: &str,
    deadline: std::time::Instant,
    heads: &mut Heads,
    cached: &mut bool,
) -> bool {
    let Some(row) = book.repos.get(repo) else {
        return false;
    };
    if row.deeds.is_empty() || row.issue.is_none() {
        return false;
    }
    let head = heads
        .entry(repo.to_string())
        .or_insert_with(|| head_sha(repo).ok())
        .clone();
    let same_head = head.is_some() && row.landed_head == head;
    let mut kept = Vec::new();
    for sha in &row.deeds {
        if same_head && row.landed.contains(sha) {
            kept.push(sha.clone());
            continue;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        match sha_landed(repo, sha) {
            Some(true) => kept.push(sha.clone()),
            Some(false) => {}
            None => return false,
        }
    }
    let issue = row.issue.clone();
    let dropped: Vec<String> = row
        .deeds
        .iter()
        .filter(|sha| !kept.contains(sha))
        .cloned()
        .collect();
    let all_landed = kept.len() == row.deeds.len();
    if head.is_some() && !(same_head && all_landed) {
        if let Some(row) = book.repos.get_mut(repo) {
            row.landed.clone_from(&kept);
            row.landed_head = head;
        }
        *cached = true;
    }
    if all_landed {
        return false;
    }
    if let Some(row) = book.repos.get_mut(repo) {
        row.deeds.clone_from(&kept);
        row.lesson = false;
        row.landed.retain(|sha| kept.contains(sha));
    }
    if let Some(issue) = issue {
        for sha in &dropped {
            if let Err(e) = drop_citation(&issue, sha) {
                log_line(&format!("deed: {e}"));
            }
        }
    }
    let _ = ensure_deed(book, repo);
    true
}

/// `Some` when git could answer. `None` when this directory is not a
/// checkout, so a missing git does not wipe a deed.
fn sha_landed(repo: &str, sha: &str) -> Option<bool> {
    match run_timed(
        "git",
        &["-C", repo, "merge-base", "--is-ancestor", sha, "HEAD"],
        &[],
    ) {
        Ok(_) => Some(true),
        Err(e) => {
            let text = format!("{e:#}");
            if text.contains("not a git repository") || text.contains("not on PATH") {
                None
            } else {
                Some(false)
            }
        }
    }
}

/// Remember HEAD the first time this sitting sees the repository.
fn note_start(book: &mut Book, repo: &str) {
    if repo.is_empty() {
        return;
    }
    if book
        .repos
        .get(repo)
        .and_then(|row| row.start_sha.as_deref())
        .is_some()
    {
        return;
    }
    let Ok(sha) = head_sha(repo) else {
        return;
    };
    book.repos.entry(repo.to_string()).or_default().start_sha = Some(sha);
}

/// A claim still open, or failures and asks with no lesson yet. A sitting
/// that never claimed still has a lesson to write when a tool failed or
/// the guard asked.
fn sitting_open(row: &RepoBook) -> bool {
    let notes = !row.failures.is_empty() || !row.asks.is_empty();
    (row.issue.is_some() && !row.finished) || (notes && !row.lesson)
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

/// A row that never held an issue and has nothing to say: the prompt hook
/// opened it with a start sha and a title, and no edit or commit claimed.
/// A row with failures or asks stays, since its lesson and the failures it
/// booked are the sitting's record.
fn unclaimed_row(row: &RepoBook) -> bool {
    row.issue.is_none()
        && !row.claim_failed
        && row.deeds.is_empty()
        && row.failures.is_empty()
        && row.asks.is_empty()
}

/// Drop the rows of a sitting that ended without a claim, so no book keeps
/// a row whose issue is null. The next prompt opens a fresh one.
fn drop_unclaimed_rows(book: &mut Book) {
    book.repos.retain(|_, row| !unclaimed_row(row));
    book.cwd_repo
        .retain(|_, repo| book.repos.contains_key(repo));
}

fn finish_open(book: &mut Book) {
    let repos: Vec<String> = book.repos.keys().cloned().collect();
    finish_rows(book, &repos);
    book.prompt.clear();
    book.prompt_repo.clear();
    book.prompt_unix = 0;
}

fn finish_rows(book: &mut Book, repos: &[String]) {
    for repo in repos {
        let mut cached = false;
        let in_a_minute = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let _ = reconcile_deeds(book, repo, in_a_minute, &mut Heads::new(), &mut cached);
        let Some(row) = book.repos.get(repo) else {
            continue;
        };
        let issue = row.issue.clone();
        let notes = !row.failures.is_empty() || !row.asks.is_empty();
        let finished = row.finished;
        // A claim closes with a lesson. So does a sitting that only failed
        // or was asked: the next session otherwise has nothing to recall.
        if !row.lesson && (issue.is_some() || notes) {
            let title = lesson_title_of(book, repo, row);
            let deeds = row.deeds.clone();
            let failures = row.failures.clone();
            let asks = row.asks.clone();
            let replaces = row.lesson_id.clone();
            let label = issue.clone().unwrap_or_default();
            match write_lesson(
                &label,
                &title,
                &deeds,
                &failures,
                &asks,
                replaces.as_deref(),
            ) {
                Ok(id) => {
                    if let Some(row) = book.repos.get_mut(repo) {
                        lesson_written(row, &title, id);
                    }
                }
                Err(e) => log_line(&format!("lesson: {e}")),
            }
        }
        let also = book
            .repos
            .get_mut(repo)
            .map(|row| std::mem::take(&mut row.also))
            .unwrap_or_default();
        for other in &also {
            if let Err(e) = hand_back(other) {
                log_line(&format!("finish: {e}"));
            }
        }
        let Some(issue) = issue else {
            continue;
        };
        if finished {
            continue;
        }
        let named = book.repos.get(repo).is_some_and(|row| row.named);
        let done = if named {
            hand_back(&issue)
        } else {
            close_issue(&issue)
        };
        match done {
            Ok(()) => {
                if let Some(row) = book.repos.get_mut(repo) {
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

/// Release a claim the git hook took on an issue a commit named. The
/// issue goes back to the state it had before the claim, as `ljos release`
/// leaves it, and stays open: naming a ticket in a commit is not accepting
/// the work on it.
fn hand_back(issue: &str) -> Result<(), String> {
    let assignee = super::resolve_assignee(None);
    if which::which("claimdag").is_ok() {
        if let Err(e) = super::release(issue, &assignee) {
            log_line(&format!("release: {e:#}"));
            if let Err(e) = super::unstamp_tracker(issue, &assignee, true) {
                log_line(&format!("release: {e:#}"));
            }
        }
    } else if let Err(e) = super::unstamp_tracker(issue, &assignee, true) {
        log_line(&format!("release: {e:#}"));
    }
    let line = super::persist_tracker(issue, "released");
    if line.contains("could not") || line.contains("refused") {
        return Err(line.trim().to_string());
    }
    Ok(())
}

/// The most ids a commit message is checked for. Each one is a tracker read.
const NAMED_IDS_MAX: usize = 8;

/// The first id in `message` that names an issue the tracker holds and
/// has not closed: `ljos-policyd-32fx: ...` in a subject, `Fixes ljos-xsor`
/// in a body. A word shaped like an id (`fix-hook`) that the tracker does
/// not know is not one, and a DONE or CANCELLED issue is not reopened.
#[must_use]
pub fn named_open_issue(message: &str) -> Option<String> {
    named_open_issue_with(message, |id| {
        super::issue_record(id).ok().and_then(|v| {
            (v["id"].as_str() == Some(id)).then(|| v["state"].as_str().unwrap_or("").to_string())
        })
    })
}

/// [`named_open_issue`] with the tracker read passed in: `state` gives the
/// issue's state, `None` when the tracker does not hold it.
fn named_open_issue_with(message: &str, state: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    for raw in message.split_whitespace() {
        let token = raw.trim_matches(|c: char| {
            matches!(
                c,
                ',' | '.'
                    | ':'
                    | ';'
                    | '!'
                    | '?'
                    | '"'
                    | '\''
                    | '`'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '#'
            )
        });
        if !is_issue_id(token) || seen.iter().any(|s| s == token) {
            continue;
        }
        if seen.len() >= NAMED_IDS_MAX {
            break;
        }
        seen.push(token.to_string());
        if !matches!(state(token).as_deref(), Some("DONE" | "CANCELLED") | None) {
            return Some(token.to_string());
        }
    }
    None
}

/// Book the commit at `head` in `repo` on `issue`, an open issue its
/// message named, instead of filing a new one. The sitting that already
/// holds the checkout keeps its row; otherwise the seat's book does. The
/// claim is taken unless that row already holds the issue, and the deed is
/// cited on the issue. A row holding another issue keeps it, and the named
/// one is released with it at the end of the sitting.
pub(crate) fn book_named_commit(repo: &str, head: &str, issue: &str) {
    let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
    let (path, key) =
        open_row_for_repo(repo).unwrap_or_else(|| (book_path(&sitting_key()), repo.to_string()));
    let mut book = load_book(&path);
    book_named_in(&mut book, &key, repo, head, issue);
    store_book(&path, &book);
}

/// The commit `repo`'s HEAD message names an open issue: that id and the
/// sha. A harness hook reads it once the commit has landed.
fn named_head(repo: &str) -> Option<(String, String)> {
    let said = run_timed("git", &["-C", repo, "log", "-1", "--format=%H%n%B"], &[]).ok()?;
    let (sha, message) = said.stdout.split_once('\n')?;
    let sha = sha.trim();
    if sha.is_empty() {
        return None;
    }
    named_open_issue(message).map(|issue| (issue, sha.to_string()))
}

/// [`book_named_commit`] on a book already loaded under the book lock, at
/// row `key`. The git hook and a harness hook both book through this, so a
/// commit naming an open issue is claimed and cited the same way whichever
/// of them saw it.
fn book_named_in(book: &mut Book, key: &str, repo: &str, head: &str, issue: &str) {
    let key = key.to_string();
    let now = unix_secs();
    let row = book.repos.entry(key.clone()).or_default();
    let open = row.issue.clone().filter(|_| !row.finished);
    let holds = open.as_deref() == Some(issue);
    let held_also = row.also.iter().any(|other| other == issue);
    // The issue a hook filed for this sitting's first edit, with no deed
    // yet, was a placeholder: the commit says which ticket the work is on.
    // It is cancelled before the named one is claimed, so a claim graph
    // that holds one node per assignee takes the named one.
    let placeholder = open
        .clone()
        .filter(|held| held != issue && !held_also && empty_filed_row(row, held));
    if let Some(auto) = &placeholder {
        cancel_placeholder(auto, issue);
    }
    let open = if placeholder.is_some() { None } else { open };
    if !holds && !held_also {
        match claim_existing(issue) {
            Ok(()) => {
                let line = super::persist_tracker(issue, "claimed");
                if line.contains("could not") || line.contains("refused") {
                    log_line(line.trim());
                }
            }
            Err(e) => log_line(&format!("claim: {issue}: {e}")),
        }
    }
    let cited = cite_deed(issue, head);
    if let Err(e) = &cited {
        log_line(&format!("deed: {e}"));
    }
    let row = book.repos.entry(key.clone()).or_default();
    match open {
        Some(ref held) if held == issue => {
            if cited.is_ok() && !row.deeds.iter().any(|seen| seen == head) {
                row.deeds.push(head.to_string());
            }
        }
        Some(_) => {
            if !held_also {
                row.also.push(issue.to_string());
            }
            if !row.elsewhere.iter().any(|seen| seen == head) {
                row.elsewhere.push(head.to_string());
            }
        }
        None if placeholder.is_some() => {
            // The cancelled ticket was a placeholder for this sitting.
            // Failures, asks, the title and commits a sweep already found
            // stay. Only the lesson was about that ticket.
            row.lesson = false;
            row.lesson_id = None;
            row.lesson_title = None;
            row.issue = Some(issue.to_string());
            row.named = true;
            row.filed = false;
            row.started = false;
            row.finished = false;
            row.recall = recall_line(issue);
            row.elsewhere.retain(|seen| seen != head);
            if cited.is_ok() && !row.deeds.iter().any(|seen| seen == head) {
                row.deeds.push(head.to_string());
            }
        }
        None => {
            *row = RepoBook {
                issue: Some(issue.to_string()),
                named: true,
                deeds: if cited.is_ok() {
                    vec![head.to_string()]
                } else {
                    Vec::new()
                },
                start_sha: Some(head.to_string()),
                recall: recall_line(issue),
                ..RepoBook::default()
            };
        }
    }
    book.last_unix = now;
    if !repo.is_empty() {
        book.last_repo = repo.to_string();
    }
    stamp_touch(book, &key, now);
}

/// Whether `row` holds `held` as a placeholder: a hook filed it, no commit
/// is booked on it here or in the tracker, and the tracker has it STARTED
/// under this holder. An issue someone else holds, or one with a deed, is
/// real work and stays.
fn empty_filed_row(row: &RepoBook, held: &str) -> bool {
    if !row.filed || row.named || row.started || !row.deeds.is_empty() {
        return false;
    }
    if !this_seat_booked(held) {
        return false;
    }
    super::issue_record(held)
        .is_ok_and(|v| v["deeds"].as_array().is_none_or(|deeds| deeds.is_empty()))
}

/// Cancel `auto`, the placeholder a hook filed, now that a commit named
/// `named`. The claim is released first, and the tracker commit and the
/// log say why. A claim graph's release hands the issue back to TODO, so
/// the cancel takes STARTED or TODO and leaves any other state alone.
fn cancel_placeholder(auto: &str, named: &str) {
    let assignee = super::resolve_assignee(None);
    if which::which("claimdag").is_ok() {
        if let Err(e) = super::release(auto, &assignee) {
            log_line(&format!("release: {e:#}"));
        }
    }
    let state = super::issue_record(auto)
        .ok()
        .and_then(|v| v["state"].as_str().map(str::to_string))
        .unwrap_or_default();
    if !matches!(state.as_str(), "STARTED" | "TODO") {
        log_line(&format!(
            "claim: left {auto} {state}, not a placeholder now"
        ));
        return;
    }
    match run_timed(
        "vissue",
        &["update", auto, "-s", "CANCELLED", "--if-state", &state],
        &[],
    ) {
        Ok(_) => {
            let line = super::persist_tracker(auto, "cancelled");
            if line.contains("could not") || line.contains("refused") {
                log_line(line.trim());
            }
            log_line(&format!(
                "claim: cancelled {auto}, filed for this sitting with no deed; the commit named {named}"
            ));
        }
        Err(e) => log_line(&format!("claim: could not cancel {auto}: {e}")),
    }
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
    // The ask is the call that did not run. Remember HEAD now, because the
    // after hook is a later process and must not cite this commit.
    note_start(&mut book, &repo);
    let now = unix_secs();
    let row = book.repos.entry(repo).or_default();
    row.last_unix = now;
    let short: String = reason.chars().take(160).collect();
    if row.asks.len() < 4 && !row.asks.iter().any(|seen| seen == &short) {
        row.asks.push(short);
    }
    book.last_unix = now;
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
/// Two short sentences: the ticket and its commits, then the failing
/// command and the guard's rule when the sitting had them. The full title,
/// rule and command also ride in `source`, which the prose gate does not
/// read. The issue id rides in the `issue` field, so a later lesson for
/// another issue does not close this one. A pack that is off is an error
/// here; the caller logs it and still closes the ticket. `replaces` is the
/// id of the lesson this sitting wrote before, which the pack closes.
/// The title stays the one that lesson was written under. A later prompt
/// is another turn, and supersedes is what closes the old atom. The new
/// lesson's id comes back for the next rewrite.
fn write_lesson(
    issue: &str,
    title: &str,
    shas: &[String],
    failures: &[String],
    asks: &[String],
    replaces: Option<&str>,
) -> Result<Option<String>, String> {
    let text = lesson_text(title, issue, shas, failures, asks);
    super::packset_write_agent_sourced(
        "Remember",
        &text,
        Some(false),
        title,
        &join_notes(asks),
        &join_notes(failures),
        issue,
        replaces,
    )
    .map(|posted| posted["id"].as_str().map(str::to_string))
    .map_err(|e| noted_err(&e))
}

/// The sitting's notes, one per line, for an atom field the prose gate
/// does not read.
fn join_notes(notes: &[String]) -> String {
    notes
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The error and its causes, so a refused `POST /v1/atoms` keeps the HTTP
/// status and the body. `Display` on the context alone drops both.
fn noted_err(err: &anyhow::Error) -> String {
    let mut out = String::new();
    for cause in err.chain() {
        let text = cause.to_string();
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str(": ");
        }
        out.push_str(&text);
    }
    if out.is_empty() {
        err.to_string()
    } else {
        out
    }
}

/// At most two short sentences. The pack refuses a third sentence, a
/// sentence of thirty words or more, and a readability grade past 14 once
/// the text has twelve words. The title keeps the words a later prompt can
/// match: stopwords and filler go first, and a word is shortened only when
/// the gate still refuses. The rule name and the failing command's verb
/// stay whole. A title that is only filler still leaves a word.
fn lesson_text(
    title: &str,
    issue: &str,
    shas: &[String],
    failures: &[String],
    asks: &[String],
) -> String {
    let issue = issue.trim();
    let shas: Vec<String> = shas
        .iter()
        .take(3)
        .map(|sha| sha.chars().take(7).collect::<String>())
        .filter(|sha| !sha.is_empty())
        .collect();
    let verb = failures.first().map(|text| command_verb(text));
    let rule = asks.iter().find_map(|ask| rule_name(ask));
    let second = second_sentence(verb.as_deref(), rule.as_deref());
    let mut words = title_words(title);
    let mut shortening = true;
    for _ in 0..256 {
        let text = lesson_sentence(&words, issue, &shas, &second);
        if lesson_prose_ok(&text) {
            return text;
        }
        if shortening && shorten_longest(&mut words) {
            continue;
        }
        shortening = false;
        if words.len() <= 1 {
            return text;
        }
        let drop_at = words
            .iter()
            .enumerate()
            .min_by_key(|(i, word)| (word.chars().count(), *i))
            .map(|(i, _)| i)
            .unwrap_or(0);
        words.remove(drop_at);
    }
    lesson_sentence(&words, issue, &shas, &second)
}

/// Runners whose next word is the command a failure should name.
const COMMAND_RUNNERS: &[&str] = &[
    "git", "cargo", "npm", "pnpm", "yarn", "bun", "npx", "pip", "uv", "go", "make", "python",
    "python3", "node",
];

/// Words that describe a failure and are not the command that failed.
fn status_word(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "exited" | "failed" | "failure" | "error" | "nonzero" | "non-zero"
    )
}

/// The verb of a failing command: `git commit exited non-zero` is `commit`,
/// and `cargo test` is `test`. An error that is not a command names its
/// first distinctive word.
fn command_verb(text: &str) -> String {
    let words = super::shell_words(text);
    if let Some(verb) = runner_subcommand(&words) {
        return verb;
    }
    words
        .into_iter()
        .find(|word| !super::is_lesson_filler(word) && !status_word(word))
        .map(|word| word.to_ascii_lowercase())
        .unwrap_or_else(|| "tool".to_string())
}

/// The subcommand of `git`, `cargo`, `npm` and the other [`COMMAND_RUNNERS`].
fn runner_subcommand(words: &[String]) -> Option<String> {
    let mut i = 0usize;
    if words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("sudo"))
    {
        i += 1;
    }
    let bin = words.get(i)?.rsplit('/').next().unwrap_or("");
    let bin_name = bin.to_ascii_lowercase();
    if !COMMAND_RUNNERS.contains(&bin_name.as_str()) {
        return None;
    }
    i += 1;
    if bin_name == "git" {
        while i < words.len() {
            match words[i].as_str() {
                "-C" | "-c" | "--git-dir" | "--work-tree" => i += 2,
                flag if flag.starts_with("--git-dir=") || flag.starts_with("--work-tree=") => {
                    i += 1;
                }
                flag if flag.starts_with('-') => i += 1,
                other => return Some(other.to_ascii_lowercase()),
            }
        }
        return Some(bin_name);
    }
    Some(
        words
            .iter()
            .skip(i)
            .find(|word| !word.starts_with('-'))
            .map_or(bin_name, |word| word.to_ascii_lowercase()),
    )
}

/// Words of an ask that the sentence already says, so the rule name is the
/// rest: `the guard asked before a force push` names `force push`.
const ASK_BOILER: &[&str] = &["guard", "asked", "about", "tool", "failed", "failure"];

/// One rule token: letters, digits, hyphens, and not a filler word.
fn rule_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && token.chars().any(|c| c.is_ascii_alphabetic())
        && !super::is_lesson_filler(token)
}

/// The guard's rule, as `git-force-push` from `git-force-push: a force push…`.
/// Without that token, the first distinctive words of the ask.
fn rule_name(ask: &str) -> Option<String> {
    let ask = ask.trim();
    if ask.is_empty() {
        return None;
    }
    if let Some((head, _)) = ask.split_once(':') {
        let head = head.trim();
        if rule_token(head) {
            return Some(head.to_string());
        }
    }
    for raw in ask.split_whitespace() {
        let token = raw.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if token.contains('-') && rule_token(token) {
            return Some(token.to_string());
        }
    }
    let named: Vec<String> = title_words(ask)
        .into_iter()
        .filter(|word| !ASK_BOILER.contains(&word.to_ascii_lowercase().as_str()))
        .take(2)
        .collect();
    if named.is_empty() {
        None
    } else {
        Some(named.join(" "))
    }
}

/// `Commit failed and the guard asked about git-force-push.` The rule token
/// and the verb are not shortened with the title.
fn second_sentence(verb: Option<&str>, rule: Option<&str>) -> String {
    match (verb, rule) {
        (Some(verb), Some(rule)) => {
            format!(
                "{} failed and the guard asked about {rule}.",
                capitalize_word(verb)
            )
        }
        (Some(verb), None) => format!("{} failed.", capitalize_word(verb)),
        (None, Some(rule)) => format!("The guard asked about {rule}."),
        (None, None) => String::new(),
    }
}

fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Distinctive words, in the order the prompt said them. When every word is
/// filler, the first few raw tokens, so the title cannot vanish.
fn title_words(title: &str) -> Vec<String> {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty() && !super::is_lesson_filler(word))
        .map(str::to_string)
        .collect();
    if !words.is_empty() {
        return words;
    }
    title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(3)
        .map(str::to_string)
        .collect()
}

/// One character off the longest title word, and not below four characters.
/// The rule name and the command verb are not in this list.
fn shorten_longest(words: &mut [String]) -> bool {
    let Some(at) = words
        .iter()
        .enumerate()
        .filter(|(_, word)| word.chars().count() > 4)
        .max_by_key(|(i, word)| (word.chars().count(), *i))
        .map(|(i, _)| i)
    else {
        return false;
    };
    words[at].pop();
    true
}

fn lesson_sentence(words: &[String], issue: &str, shas: &[String], second: &str) -> String {
    let head = words.join(" ");
    let issue = issue.trim();
    let first = if issue.is_empty() {
        // No ticket was filed. The lesson is a sentence about the work,
        // then what went wrong.
        if shas.is_empty() {
            if head.is_empty() {
                String::new()
            } else {
                format!("The session worked on {head}.")
            }
        } else if head.is_empty() {
            format!("Committed {}.", shas.join(" "))
        } else {
            format!("{head} committed {}.", shas.join(" "))
        }
    } else if shas.is_empty() {
        if head.is_empty() {
            format!("Work {issue} closed.")
        } else {
            format!("{head} ({issue}) closed.")
        }
    } else if head.is_empty() {
        format!("Work {issue} committed {}.", shas.join(" "))
    } else {
        format!("{head} ({issue}) committed {}.", shas.join(" "))
    };
    if second.is_empty() {
        first
    } else if first.is_empty() {
        second.to_string()
    } else {
        format!("{first} {second}")
    }
}

/// The pack's prose gate for one atom. A lesson that fails it is not stored,
/// and the next session then has nothing to recall.
fn lesson_prose_ok(text: &str) -> bool {
    packset_core::prose_refuse(text, packset_core::ProseRole::Atom).is_ok()
}

/// Record that `prompt` belongs to `repo`. A later git hook in another
/// checkout can tell the words are not its title.
fn bind_prompt(book: &mut Book, repo: &str, now: u64) {
    if repo.is_empty() || book.prompt.trim().is_empty() {
        return;
    }
    book.prompt_repo = repo.to_string();
    book.prompt_unix = now;
}

/// A row that moves to another issue must drop issue-bound state from the
/// prior issue. A rewrite must not carry old deeds, failures or lesson links
/// onto the new ticket. The commit baseline goes with them. The caller
/// plants HEAD at the rebind, so a later citation does not walk the
/// previous issue's range.
fn clear_issue_fields_if_issue_changes(row: &mut RepoBook, issue: &str) {
    if row.issue.as_deref() == Some(issue) {
        return;
    }
    if row.issue.is_none() {
        row.lesson = false;
        row.lesson_id = None;
        row.lesson_title = None;
        return;
    }
    row.deeds.clear();
    row.landed.clear();
    row.landed_head = None;
    row.failures.clear();
    row.asks.clear();
    row.lesson = false;
    row.lesson_id = None;
    row.lesson_title = None;
    row.title = None;
    row.named = false;
    row.also.clear();
    row.elsewhere.clear();
    row.filed = false;
    row.start_sha = None;
}

/// Copy `row.title` into `lesson_title` once, when a lesson already exists.
/// The row title may then follow a later prompt. The rewrite does not.
fn hold_lesson_title(row: &mut RepoBook) {
    if !(row.lesson || row.lesson_id.is_some()) {
        return;
    }
    if row
        .lesson_title
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        if let Some(title) = row.title.clone().filter(|title| !title.trim().is_empty()) {
            row.lesson_title = Some(title);
        }
    }
}

/// Remember the title a lesson was stored under, the first time it is written.
fn lesson_written(row: &mut RepoBook, title: &str, id: Option<String>) {
    if row
        .lesson_title
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
        && !title.trim().is_empty()
    {
        row.lesson_title = Some(title.to_string());
    }
    row.lesson = true;
    row.lesson_id = id;
}

/// The title a rewrite keeps. A lesson that already exists uses the title
/// it was written under, not the prompt that arrived afterwards.
fn lesson_title_of(book: &Book, repo: &str, row: &RepoBook) -> String {
    let kept = if row.lesson || row.lesson_id.is_some() {
        row.lesson_title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
    } else {
        None
    };
    let heard = match kept {
        Some(title) => title.to_string(),
        None => heard_title(book, repo, row),
    };
    title_from(&heard, repo)
}

/// The prompt the lesson and the claim use for `repo`. The book's prompt
/// is that text only when it was recorded for this checkout.
fn heard_title(book: &Book, repo: &str, row: &RepoBook) -> String {
    if prompt_belongs_to(book, repo) {
        let prompt = book.prompt.trim();
        if !prompt.is_empty() {
            return prompt.to_string();
        }
    }
    row.title.clone().unwrap_or_default()
}

/// Whether the saved prompt is the live one for `repo`. A git hook may
/// title a claim with it. Another repository's prompt, or one from a
/// sitting that has already gone idle, may not.
fn prompt_live_for(book: &Book, repo: &str, now: u64) -> bool {
    if !prompt_belongs_to(book, repo) {
        return false;
    }
    let Some(limit) = sitting_idle() else {
        return true;
    };
    let earlier = book.repos.iter().any(|(key, row)| {
        same_checkout(key, repo) && row_is_idle(now, book, key, row, limit, Some(repo))
    });
    if earlier {
        return false;
    }
    let open = book
        .repos
        .iter()
        .any(|(key, row)| same_checkout(key, repo) && !row.finished && sitting_open(row));
    if open {
        return true;
    }
    // The prompt was recorded for this checkout and no claim is open.
    // An old stamp is the previous sitting; a fresh one is this sitting,
    // still waiting for its first claim.
    let stamped = if book.prompt_unix > 0 {
        book.prompt_unix
    } else {
        book.last_unix
    };
    !idle_due(now, stamped, limit)
}

fn prompt_belongs_to(book: &Book, repo: &str) -> bool {
    let prompt = book.prompt.trim();
    if prompt.is_empty() || repo.is_empty() {
        return false;
    }
    let recorded = book.prompt_repo.trim();
    if !recorded.is_empty() {
        return same_checkout(recorded, repo);
    }
    book.repos.iter().any(|(key, row)| {
        !row.finished
            && same_checkout(key, repo)
            && row
                .title
                .as_deref()
                .is_some_and(|title| title.trim() == prompt)
    })
}

/// Drop the prompt once the repository it belongs to no longer has an open row.
fn release_spent_prompt(book: &mut Book) {
    if book.prompt.trim().is_empty() {
        return;
    }
    let recorded = book.prompt_repo.trim();
    let still = if !recorded.is_empty() {
        book.repos.keys().any(|repo| same_checkout(repo, recorded))
    } else {
        let prompt = book.prompt.trim();
        book.repos.values().any(|row| {
            !row.finished
                && row
                    .title
                    .as_deref()
                    .is_some_and(|title| title.trim() == prompt)
        })
    };
    if !still {
        book.prompt.clear();
        book.prompt_repo.clear();
        book.prompt_unix = 0;
    }
}

/// Whether `row` has been idle past `limit`. A row clock wins. A row with
/// no clock of its own follows the book when that clock is idle, and is
/// idle on a call in a different repository when the book clock is fresh:
/// that fresh clock is the other repository's touch.
fn row_is_idle(
    now: u64,
    book: &Book,
    repo: &str,
    row: &RepoBook,
    limit: Duration,
    current: Option<&str>,
) -> bool {
    if !sitting_open(row) {
        return false;
    }
    if row.last_unix > 0 {
        return idle_due(now, row.last_unix, limit);
    }
    if idle_due(now, book.last_unix, limit) {
        return true;
    }
    matches!(
        current,
        Some(cur) if !cur.is_empty() && book.last_unix > 0 && !same_checkout(cur, repo)
    )
}

fn stamp_touch(book: &mut Book, repo: &str, now: u64) {
    if !repo.is_empty() {
        if let Some(row) = book.repos.get_mut(repo) {
            row.last_unix = now;
        }
    }
    book.last_unix = now;
}

/// Keep the prompt on the repository row. A later claim reads it from
/// there. A lesson already written keeps the title it was stored under.
/// An empty prompt leaves a title that was already stored.
fn remember_title(book: &mut Book, repo: &str) {
    let prompt: String = collapse(&book.prompt).chars().take(4000).collect();
    if prompt.is_empty() || repo.is_empty() {
        return;
    }
    let row = book.repos.entry(repo.to_string()).or_default();
    hold_lesson_title(row);
    row.title = Some(prompt);
}

fn title_from(prompt: &str, repo: &str) -> String {
    let forced = std::env::var("LJOS_PROJECT").ok();
    let tracker = configured_tracker();
    title_at(prompt, repo, forced.as_deref(), tracker.as_deref())
}

/// [`title_from`] with the project inputs passed in. With no prompt the
/// title names the project the issue files under, as [`project_for_at`]
/// picks it, so a clone in `bench-w18b-tfn7` is "Work in bench".
fn title_at(prompt: &str, repo: &str, forced: Option<&str>, tracker: Option<&Path>) -> String {
    let words: String = prompt
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");
    let title: String = words.chars().take(80).collect();
    if !title.is_empty() {
        return title;
    }
    if repo.trim().is_empty() {
        return "agent work".to_string();
    }
    let project = project_for_at(repo, forced, tracker);
    if project.trim().is_empty() {
        "agent work".to_string()
    } else {
        format!("Work in {project}")
    }
}

fn project_for(repo: &str) -> String {
    let forced = std::env::var("LJOS_PROJECT").ok();
    let tracker = configured_tracker();
    project_for_at(repo, forced.as_deref(), tracker.as_deref())
}

/// The tracker vissue would use: `ISSUE_ROOT`, then `VISSUE_ROOT`, then
/// `root` in the vissue config. A hook often has neither variable, and
/// the config is how vissue itself finds the tracker. A root that is not
/// a directory is ignored, as vissue ignores it.
fn configured_tracker() -> Option<PathBuf> {
    for key in ["ISSUE_ROOT", "VISSUE_ROOT"] {
        if let Some(path) = env_path(key) {
            return Some(expand_home_path(&path));
        }
    }
    let path = vissue_config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let root = tracker_root_from_config(&text)?;
    root.is_dir().then_some(root)
}

fn env_path(key: &str) -> Option<PathBuf> {
    let value = std::env::var_os(key)?;
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn vissue_config_path() -> Option<PathBuf> {
    if let Some(path) = env_path("VISSUE_CONFIG") {
        return Some(path);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(base.join("vissue").join("config.toml"))
}

/// Top-level `root` in a vissue config. A `root` under a `[table]` is a
/// different key. A leading `~` is this user's home.
fn tracker_root_from_config(text: &str) -> Option<PathBuf> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            return None;
        }
        let (key, _) = line.split_once('=')?;
        if key.trim() != "root" {
            continue;
        }
        let value: toml::Value = toml::from_str(line).ok()?;
        let root = value.get("root").and_then(toml::Value::as_str)?;
        if root.is_empty() {
            return None;
        }
        return Some(expand_home(root));
    }
    None
}

fn expand_home(raw: &str) -> PathBuf {
    if raw == "~" || raw.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) {
            let rest = raw.strip_prefix("~/").unwrap_or("");
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}

fn expand_home_path(path: &Path) -> PathBuf {
    path.to_str()
        .map(expand_home)
        .unwrap_or_else(|| path.to_path_buf())
}

fn project_for_at(repo: &str, forced: Option<&str>, tracker: Option<&Path>) -> String {
    if let Some(project) = forced.map(str::trim).filter(|project| !project.is_empty()) {
        return project.to_string();
    }
    let remote = git_remote_url(repo);
    if let Some(tracker) = tracker {
        if let Some(project) = match_remote_project(repo, tracker) {
            return project;
        }
    }
    // A clone in a folder of its own name (`bench-w18b-tfn7`) is still the
    // repository its origin names. The folder is the project only when the
    // checkout has no origin.
    if let Some(name) = remote.as_deref().and_then(remote_repo_name) {
        return name;
    }
    if let Some(tracker) = tracker {
        if let Some(project) = sole_project_in(tracker) {
            return project;
        }
    }
    Path::new(repo)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("agent")
        .to_string()
}

/// The project whose recorded remote is this checkout's, else the project
/// named like the repository in that URL. A checkout called `workspace`
/// then files on the existing project instead of a new basename.
fn match_remote_project(repo: &str, tracker: &Path) -> Option<String> {
    let url = git_remote_url(repo)?;
    let want = normalize_remote(&url);
    if want.is_empty() {
        return None;
    }
    let projects = projects_in(tracker);
    for (name, dir) in &projects {
        if project_records_remote(dir, &want) {
            return Some(name.clone());
        }
        if tracker_project_remote(tracker, name).is_some_and(|url| normalize_remote(&url) == want) {
            return Some(name.clone());
        }
    }
    let slug = remote_slug(&want);
    projects
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&slug))
        .map(|(name, _)| name)
}

/// The repository name in an origin URL, as the forge spells it:
/// `https://github.com/leidarljos/bench.git` and `git@github.com:leidarljos/bench`
/// are `bench`. vissue names a project by its directory under the tracker,
/// and `vissue create -p` matches an existing one whatever its case.
/// A local path or `file:` origin names no forge repository, and the
/// folder stays the fallback there.
fn remote_repo_name(url: &str) -> Option<String> {
    let raw = url.trim().trim_end_matches('/');
    if raw.starts_with(['/', '.', '~']) || raw.starts_with("file:") {
        return None;
    }
    let raw = raw.strip_suffix(".git").unwrap_or(raw);
    let name = raw.rsplit(['/', ':']).next()?.trim();
    let ok = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok.then(|| name.to_string())
}

fn git_remote_url(repo: &str) -> Option<String> {
    if repo.is_empty() {
        return None;
    }
    // The checkout's config names the remote. `git remote get-url` applies
    // insteadOf, follows GIT_DIR instead of -C, and can sit on a credential
    // helper until the hook's deadline, after which the folder name was
    // used as the project. The file is the remote the forge has.
    if let Some(url) = origin_from_checkout(repo) {
        return Some(url);
    }
    git_remote_from_command(repo)
}

/// `remote.origin.url` in the checkout's git config. A worktree keeps
/// remotes in the common git dir, named by `commondir`, not in the
/// worktree git dir. A `.git` file's `gitdir:` is followed.
fn origin_from_checkout(repo: &str) -> Option<String> {
    let git_dir = checkout_git_dir(Path::new(repo))?;
    let common = common_git_dir(&git_dir);
    let config = common.join("config");
    if config.is_file() {
        return origin_url_in(&config);
    }
    origin_url_in(&git_dir.join("config"))
}

fn checkout_git_dir(repo: &Path) -> Option<PathBuf> {
    let dot = repo.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    if !dot.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("gitdir:"))?;
    let raw = line.trim().trim_start_matches("gitdir:").trim();
    if raw.is_empty() {
        return None;
    }
    Some(join_git_path(repo, raw))
}

fn common_git_dir(git_dir: &Path) -> PathBuf {
    let Ok(text) = std::fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let raw = text.trim();
    if raw.is_empty() {
        git_dir.to_path_buf()
    } else {
        join_git_path(git_dir, raw)
    }
}

fn join_git_path(base: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    std::fs::canonicalize(&joined).unwrap_or(joined)
}

fn origin_url_in(path: &Path) -> Option<String> {
    origin_url_in_config(&std::fs::read_to_string(path).ok()?)
}

/// The `url` of `[remote "origin"]` in a git config. An included file is
/// left to git; this reads only the config in front of it.
fn origin_url_in_config(text: &str) -> Option<String> {
    let mut origin = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            origin = section_is_origin(line);
            continue;
        }
        if !origin {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("url") {
            continue;
        }
        let value = unquote_config(value.trim());
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

fn section_is_origin(header: &str) -> bool {
    let inner = header
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    let mut parts = inner.split_whitespace();
    if !parts
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("remote"))
    {
        return false;
    }
    let Some(name) = parts.next() else {
        return false;
    };
    name.trim_matches(|c| c == '"' || c == '\'')
        .eq_ignore_ascii_case("origin")
        && parts.next().is_none()
}

fn unquote_config(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        return raw[1..raw.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }
    raw.to_string()
}

/// `git config --get remote.origin.url`, with the hook's `GIT_DIR` removed
/// so `-C` is the checkout. `remote get-url` rewrites the URL through
/// insteadOf and can hide the repository name. An included config is the
/// case the file read above does not see.
fn git_remote_from_command(repo: &str) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let path = which::which("git").ok()?;
    let mut cmd = Command::new(path);
    cmd.args(["-C", repo, "config", "--get", "remote.origin.url"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // A test may have named a decoy on this command. Drop it with the
    // variables a hook would have exported, so `-C` is the checkout.
    crate::apply_git_command_env(&mut cmd);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
    ] {
        cmd.env_remove(key);
    }
    let mut child = cmd.spawn().ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut stdout = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }
                let url = stdout.lines().next().unwrap_or("").trim();
                return (!url.is_empty()).then(|| url.to_string());
            }
            Ok(None) if started.elapsed() > COMMAND_DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return None,
        }
    }
}

fn normalize_remote(raw: &str) -> String {
    let mut raw = raw.trim().trim_end_matches('/').to_string();
    if let Some(stripped) = raw.strip_suffix(".git") {
        raw = stripped.to_string();
    }
    for prefix in ["ssh://", "git://", "https://", "http://"] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            raw = rest.to_string();
            break;
        }
    }
    // userinfo@host, including a token a credential helper wrote in front
    // of the host. The host and the path are the remote.
    if let Some((user, host)) = raw.split_once('@') {
        // A token from a credential helper can itself contain a slash.
        // Drop it when the left side is userinfo, and keep an `@` that
        // sits in the path (`example.com/user@host/repo`).
        if looks_like_userinfo(user)
            && (host.contains('/') || host.contains(':') || host.contains('.'))
        {
            raw = host.to_string();
        }
    }
    if let Some((host, path)) = raw.split_once(':') {
        if !host.contains('/') {
            raw = format!("{host}/{}", path.trim_start_matches('/'));
        }
    }
    raw.to_ascii_lowercase()
}

/// Userinfo in front of a host: `git`, or `x-access-token:abc/def`.
/// A slash that appears before any colon is a path, not a token.
fn looks_like_userinfo(user: &str) -> bool {
    if user.is_empty() || user.contains(' ') {
        return false;
    }
    match user.find('/') {
        None => true,
        Some(slash) => user[..slash].contains(':'),
    }
}

fn remote_slug(normalized: &str) -> String {
    normalized
        .rsplit('/')
        .next()
        .unwrap_or(normalized)
        .to_string()
}

fn project_records_remote(dir: &Path, want: &str) -> bool {
    recorded_remotes(dir)
        .iter()
        .any(|url| normalize_remote(url) == want)
}

/// Remotes this project recorded on purpose. A `#+REMOTE:` header, a
/// one-url line in `remote.url` or `repo.url`, or a `remote` field in
/// `project.toml`. A URL in a note is not a record.
fn recorded_remotes(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(dir.join("issues.org")) {
        for line in text.lines() {
            if let Some(url) = header_remote(line) {
                out.push(url);
            }
        }
    }
    for name in ["remote.url", "repo.url"] {
        let Ok(text) = std::fs::read_to_string(dir.join(name)) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.contains(char::is_whitespace) {
                continue;
            }
            out.push(line.to_string());
        }
    }
    out.extend(toml_remotes(&dir.join("project.toml")));
    out
}

/// `#+REMOTE: url` in an org file. The rest of the file is notes.
fn header_remote(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("#+")?;
    let (key, value) = rest.split_once(':')?;
    if !key.eq_ignore_ascii_case("REMOTE") {
        return None;
    }
    let value = value.trim().trim_matches('"').trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn toml_remotes(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    push_remote(&mut out, &value);
    if let Some(project) = value.get("project") {
        push_remote(&mut out, project);
    }
    out
}

fn push_remote(out: &mut Vec<String>, value: &toml::Value) {
    let Some(url) = value.get("remote").and_then(toml::Value::as_str) else {
        return;
    };
    let url = url.trim();
    if !url.is_empty() {
        out.push(url.to_string());
    }
}

/// `remote` under `[projects.<name>]` in the tracker's `vissue.toml`.
fn tracker_project_remote(tracker: &Path, project: &str) -> Option<String> {
    let text = std::fs::read_to_string(tracker.join("vissue.toml")).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    let url = value
        .get("projects")?
        .get(project)?
        .get("remote")?
        .as_str()?
        .trim();
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}

fn projects_in(root: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut consider = |dir: PathBuf| {
        if dir.join("issues.org").is_file() {
            if let Some(name) = dir.file_name().and_then(|name| name.to_str()) {
                found.push((name.to_string(), dir));
            }
        }
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
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
    found
}

fn sole_project_in(root: &Path) -> Option<String> {
    let mut found: Vec<String> = projects_in(root)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
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

/// The SessionStart brief, once per conversation, on stderr. A second call
/// in the same conversation prints nothing. A temp sandbox and `LJOS_OFF`
/// print nothing. This does not wait on the pack when it is off.
pub fn print_brief_once() {
    if guard_suspended() {
        return;
    }
    let path = brief_mark(&brief_key());
    if path.exists() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .is_err()
    {
        return;
    }
    let text = seat_brief();
    if text.is_empty() {
        return;
    }
    eprintln!("{text}");
    let _ = std::io::Write::flush(&mut std::io::stderr());
}

fn brief_key() -> String {
    let seat = std::env::var("LJOS_SEAT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "seat".into());
    let convo = std::env::var("CURSOR_CONVERSATION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("LJOS_SHELL_SESSION")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(process_sid);
    format!("{seat}:{convo}")
}

fn process_sid() -> String {
    // SAFETY: getsid reads this process's session id.
    let sid = unsafe { libc::getsid(0) };
    if sid > 0 {
        sid.to_string()
    } else {
        std::process::id().to_string()
    }
}

fn brief_mark(key: &str) -> PathBuf {
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    super::runtime_dir().join("brief").join(safe)
}

fn seat_brief() -> String {
    format!(
        "claims: {}\nrecall: {}\nmail: {}",
        held_claims_line(),
        brief_recall_line(),
        brief_mail_line()
    )
}

fn held_claims_line() -> String {
    let book = load_book(&book_path(&sitting_key()));
    let mut ids: Vec<String> = book
        .repos
        .values()
        .filter(|row| !row.finished)
        .filter_map(|row| row.issue.clone())
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        "none".into()
    } else {
        ids.join(" ")
    }
}

fn brief_recall_line() -> String {
    let book = load_book(&book_path(&sitting_key()));
    for row in book.repos.values() {
        if row.finished {
            continue;
        }
        if let Some(title) = row
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            return one_line(title);
        }
        if !row.recall.trim().is_empty() {
            return one_line(&row.recall);
        }
    }
    "none".into()
}

fn brief_mail_line() -> String {
    let url = std::env::var("PACKSET_URL").unwrap_or_default();
    let url = url.trim();
    if url.is_empty() || url == "off" {
        return "none".into();
    }
    let (text, _) = super::mail::prompt_note();
    let line = one_line(&text);
    if line.is_empty() || line.contains("nothing unread") {
        "none".into()
    } else {
        line
    }
}

/// One line for the commit hook when the sitting's title bears on the
/// message. Empty when nothing filed matches.
#[must_use]
pub fn commit_recall_line(session: &str, repo: &str, subject: &str) -> Option<String> {
    let book = load_book(&book_path(session));
    let row = book.repos.get(repo).filter(|row| row.issue.is_some())?;
    let title = row
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| subject.trim().to_string());
    let body = format!("{title}\n{}", row.recall);
    if !relevant(subject, &body) {
        return None;
    }
    let line = one_line(&title);
    if line.is_empty() {
        None
    } else {
        Some(format!("recall: {line}"))
    }
}

fn relevant(subject: &str, body: &str) -> bool {
    let subject = subject.trim();
    if subject.is_empty() {
        return false;
    }
    let body = body.to_ascii_lowercase();
    if body.contains(&subject.to_ascii_lowercase()) {
        return true;
    }
    subject
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 4)
        .any(|word| body.contains(&word.to_ascii_lowercase()))
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How a PostToolUse names the command's exit. Codex 0.163 sends the
/// output as a string and records `exit_code` in the transcript, under the
/// same `call_id` as the hook's `tool_use_id`.
#[derive(Clone, Copy)]
enum CommandExit {
    /// A code from the payload or the transcript.
    Code(i64),
    /// The output was text, and neither the payload nor the transcript
    /// named a code. Unknown is not a failure. Other evidence decides.
    Unknown,
    /// The runner sent an object, or omitted the output, and named no code.
    Absent,
}

/// A code the transcript or the payload named, and it was not zero.
fn command_failed(exit: CommandExit) -> bool {
    matches!(exit, CommandExit::Code(code) if code != 0)
}

/// Whether this commit landed. An explicit code wins, including a zero
/// beside an error string and a non-zero beside a banner. A missing code
/// is unknown: a git operation, a `[branch sha]` line, then HEAD moving
/// past the recorded start. A guard refusal or an interrupt blocks only
/// that missing-code path, so a command that never ran is not a deed.
fn commit_landed(exit: CommandExit, raw: &Value, repo: &str, start: Option<&str>) -> bool {
    match exit {
        CommandExit::Code(0) => return true,
        CommandExit::Code(_) => return false,
        CommandExit::Unknown | CommandExit::Absent => {}
    }
    if raw
        .pointer("/tool_response/interrupted")
        .and_then(Value::as_bool)
        == Some(true)
        || permission_block(raw)
    {
        return false;
    }
    git_operation_committed(raw) || stdout_has_commit_banner(raw) || head_moved(repo, start)
}

/// `gitOperation.commit` names a sha or a kind that means the commit was
/// written. Claude sends this beside the stdout, and omits the exit code.
fn git_operation_committed(raw: &Value) -> bool {
    [
        "/gitOperation",
        "/git_operation",
        "/tool_response/gitOperation",
        "/tool_response/git_operation",
        "/toolResponse/gitOperation",
    ]
    .iter()
    .any(|slot| raw.pointer(slot).is_some_and(commit_record_landed))
}

fn commit_record_landed(value: &Value) -> bool {
    if value.get("commit").is_some_and(commit_record_landed) {
        return true;
    }
    let sha = value
        .get("sha")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if sha_token(sha) {
        return true;
    }
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    matches!(kind, "committed" | "amended" | "cherry-picked")
}

fn sha_token(text: &str) -> bool {
    (7..=40).contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit())
}

fn branch_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
}

/// Git's `[branch sha]` line, including a root commit and a detached HEAD.
fn stdout_has_commit_banner(raw: &Value) -> bool {
    commit_output(raw)
        .iter()
        .any(|text| text.lines().any(line_has_commit_banner))
}

fn commit_output(raw: &Value) -> Vec<&str> {
    let mut out = Vec::new();
    for key in [
        "tool_response",
        "toolResponse",
        "stdout",
        "stderr",
        "output",
    ] {
        if let Some(text) = raw.get(key).and_then(Value::as_str) {
            out.push(text);
        }
    }
    for slot in [
        "/tool_response/stdout",
        "/tool_response/output",
        "/tool_response/stderr",
        "/toolResponse/stdout",
        "/toolResponse/output",
        "/toolResponse/stderr",
    ] {
        if let Some(text) = raw.pointer(slot).and_then(Value::as_str) {
            out.push(text);
        }
    }
    out
}

fn line_has_commit_banner(line: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = line[from..].find('[') {
        let start = from + rel + 1;
        let Some(end_rel) = line[start..].find(']') else {
            break;
        };
        if banner_inside(&line[start..start + end_rel]) {
            return true;
        }
        from = start;
    }
    false
}

fn banner_inside(inside: &str) -> bool {
    let mut rest = inside.trim();
    if rest.is_empty() {
        return false;
    }
    if let Some(after) = rest.strip_prefix("detached HEAD") {
        rest = after.trim_start();
    } else {
        let Some((branch, after)) = rest.split_once(|c: char| c.is_whitespace()) else {
            return false;
        };
        if !branch_token(branch) {
            return false;
        }
        rest = after.trim_start();
    }
    if let Some(after) = rest.strip_prefix("(root-commit)") {
        rest = after.trim_start();
    }
    let Some((sha, tail)) = rest.split_once(|c: char| c.is_whitespace()) else {
        return sha_token(rest);
    };
    tail.trim().is_empty() && sha_token(sha)
}

/// HEAD is past the sha recorded before this commit. No baseline means
/// this post has not been shown an earlier commit, so movement is not
/// evidence by itself.
fn head_moved(repo: &str, start: Option<&str>) -> bool {
    let Some(start) = start.map(str::trim).filter(|sha| !sha.is_empty()) else {
        return false;
    };
    head_sha(repo).is_ok_and(|head| head != start)
}

/// The exit code for this call. The payload wins. Otherwise the Codex
/// transcript at `transcript_path` is read, time-bounded, and matched on
/// `tool_use_id`. A text `tool_response` with no code is [`CommandExit::Unknown`].
fn command_exit(raw: &Value) -> CommandExit {
    if let Some(code) = explicit_exit(raw) {
        return CommandExit::Code(code);
    }
    if let Some(code) = transcript_exit(raw) {
        return CommandExit::Code(code);
    }
    if response_is_text(raw) {
        return CommandExit::Unknown;
    }
    CommandExit::Absent
}

/// Codex sends the shell output as a string. Other runners send an object,
/// or omit the field, and a missing code there is still success.
fn response_is_text(raw: &Value) -> bool {
    raw.get("tool_response")
        .or_else(|| raw.get("toolResponse"))
        .is_some_and(Value::is_string)
}

fn tool_use_id(raw: &Value) -> Option<&str> {
    ["tool_use_id", "toolUseId", "tool_call_id", "toolCallId"]
        .iter()
        .find_map(|key| raw.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// `exit_code` for `tool_use_id` in the Codex transcript. Codex 0.163 puts
/// the code on a `CommandExecution` item whose `id` is the call, and only
/// as text (`Process exited with code N`) on the `function_call_output`
/// with that `call_id`. The read is the tail of the file and gives up at
/// [`TRANSCRIPT_DEADLINE`], so a stuck transcript cannot hold the hook.
fn transcript_exit(raw: &Value) -> Option<i64> {
    let path = raw
        .get("transcript_path")
        .and_then(Value::as_str)
        .or_else(|| raw.get("transcriptPath").and_then(Value::as_str))
        .map(str::trim)
        .filter(|path| !path.is_empty())?;
    let call_id = tool_use_id(raw)?;
    let text = read_transcript_tail(&super::expand(path))?;
    exit_for_call(&text, call_id)
}

/// How long a transcript read may take. The same budget as one habitat
/// command: a hook that waits longer is treated as a refusal by the runner.
const TRANSCRIPT_DEADLINE: Duration = COMMAND_DEADLINE;

/// Bytes read from the end of a transcript. The call that just finished is
/// the last one, and a command's output can be long.
const TRANSCRIPT_TAIL_BYTES: u64 = 1024 * 1024;

fn read_transcript_tail(path: &Path) -> Option<String> {
    read_bounded(path, TRANSCRIPT_TAIL_BYTES, true)
}

/// The transcript's tail, bounded in bytes like the exit lookup. Regular
/// files remain readable regardless of their size, so polling alone cannot
/// limit the work. A missing prompt in this window leaves the stored one in use.
pub(crate) fn read_text_bounded(path: &Path) -> Option<String> {
    read_transcript_tail(path)
}

/// Open `path` without blocking and read at most `max_bytes`. A regular
/// file is ready at once. A fifo with no writer reports hangup at once.
/// `tail` keeps the end of a regular file and drops the partial first line.
fn read_bounded(path: &Path, max_bytes: u64, tail: bool) -> Option<String> {
    use std::os::unix::fs::FileTypeExt;
    let mut file = open_nonblock(path)?;
    let kind = file.metadata().ok().map(|meta| meta.file_type());
    // A fifo with no writer reads as immediate EOF. Poll does not say so
    // on this kernel: POLLHUP shows up only after a writer has opened the
    // fifo, so waiting on POLLIN spends the whole deadline.
    if kind.as_ref().is_some_and(|ty| ty.is_fifo()) {
        return read_available(&mut file, max_bytes, Duration::ZERO);
    }
    if kind.as_ref().is_some_and(|ty| ty.is_file()) {
        if tail {
            return read_file_tail(&mut file, max_bytes);
        }
        return read_available(&mut file, max_bytes, TRANSCRIPT_DEADLINE);
    }
    read_available(&mut file, max_bytes, Duration::ZERO)
}

fn open_nonblock(path: &Path) -> Option<std::fs::File> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::FromRawFd;
    let bytes = path.as_os_str().as_bytes();
    let c_path = std::ffi::CString::new(bytes).ok()?;
    // SAFETY: `c_path` is NUL-terminated and lives until `open` returns.
    let fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return None;
    }
    // SAFETY: `fd` is open and this function owns it.
    Some(unsafe { std::fs::File::from_raw_fd(fd) })
}

fn wait_readable(fd: i32, deadline: Duration) -> bool {
    use std::io::ErrorKind;
    let start = Instant::now();
    loop {
        let elapsed = start.elapsed();
        let left = deadline.saturating_sub(elapsed);
        let ms = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
        let mut polled = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `polled` is one valid `pollfd` and lives until `poll` returns.
        let rc = unsafe { libc::poll(&mut polled, 1, ms) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted && start.elapsed() < deadline {
                continue;
            }
            return false;
        }
        if rc == 0 {
            return false;
        }
        let rev = polled.revents;
        if (rev & libc::POLLIN) != 0 {
            return true;
        }
        if (rev & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)) != 0 {
            return false;
        }
        if start.elapsed() >= deadline {
            return false;
        }
    }
}

fn read_file_tail(file: &mut std::fs::File, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.take(max_bytes).read_to_end(&mut buf).ok()?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        if let Some(pos) = text.find('\n') {
            text.drain(..=pos);
        }
    }
    Some(text)
}

fn read_available(file: &mut std::fs::File, max_bytes: u64, deadline: Duration) -> Option<String> {
    use std::io::{ErrorKind, Read};
    use std::os::unix::io::AsRawFd;
    let start = Instant::now();
    let cap = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    while buf.len() < cap {
        let room = cap - buf.len();
        let nread = room.min(tmp.len());
        match file.read(&mut tmp[..nread]) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if !deadline.is_zero() && start.elapsed() >= deadline {
                    break;
                }
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => {
                if !deadline.is_zero() && start.elapsed() >= deadline {
                    break;
                }
                continue;
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                let left = deadline.saturating_sub(start.elapsed());
                if left.is_zero() || !wait_readable(file.as_raw_fd(), left) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    if buf.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// A code read from the transcript. The command item wins. The sentence in
/// the function output is only used when that item is absent.
struct TranscriptExit {
    item: Option<i64>,
    spoken: Option<i64>,
}

impl TranscriptExit {
    fn code(self) -> Option<i64> {
        self.item.or(self.spoken)
    }
}

/// The exit code for `call_id`. A later `CommandExecution` for the same id
/// replaces an earlier one. `Process exited with code N` does not replace
/// a code the item already named.
fn exit_for_call(text: &str, call_id: &str) -> Option<i64> {
    let mut found = TranscriptExit {
        item: None,
        spoken: None,
    };
    for line in text.lines() {
        note_line_exit(line, call_id, &mut found);
    }
    found.code()
}

fn note_line_exit(line: &str, call_id: &str, found: &mut TranscriptExit) {
    if !line.contains(call_id) {
        return;
    }
    if let Ok(value) = serde_json::from_str::<Value>(line) {
        walk_exit(&value, call_id, found);
    } else {
        exit_in_text(line, call_id, found);
    }
}

fn walk_exit(value: &Value, call_id: &str, found: &mut TranscriptExit) {
    match value {
        Value::Array(items) => {
            for item in items {
                walk_exit(item, call_id, found);
            }
        }
        Value::Object(map) => {
            if let Some(code) = command_execution_code(map, call_id) {
                found.item = Some(code);
            }
            if let Some(code) = function_output_code(map, call_id) {
                found.spoken = Some(code);
            }
            for child in map.values() {
                walk_exit(child, call_id, found);
            }
        }
        _ => {}
    }
}

/// `item.type == CommandExecution` and `item.id` is the hook's `tool_use_id`.
fn command_execution_code(map: &serde_json::Map<String, Value>, call_id: &str) -> Option<i64> {
    if map.get("type").and_then(Value::as_str) != Some("CommandExecution") {
        return None;
    }
    if map.get("id").and_then(Value::as_str) != Some(call_id) {
        return None;
    }
    ["exit_code", "exitCode"]
        .iter()
        .find_map(|key| map.get(*key).and_then(exit_number))
}

/// The matching `function_call_output`. The code is the first
/// `Process exited with code N` in its `output`, which is the harness
/// header rather than a line the command printed later.
fn function_output_code(map: &serde_json::Map<String, Value>, call_id: &str) -> Option<i64> {
    if map.get("type").and_then(Value::as_str) != Some("function_call_output") {
        return None;
    }
    let matched = map
        .get("call_id")
        .or_else(|| map.get("callId"))
        .and_then(Value::as_str)
        == Some(call_id);
    if !matched {
        return None;
    }
    map.get("output")
        .and_then(Value::as_str)
        .and_then(process_exit_code)
}

fn process_exit_code(text: &str) -> Option<i64> {
    let marker = "Process exited with code ";
    let rest = text.get(text.find(marker)? + marker.len()..)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn exit_number(slot: &Value) -> Option<i64> {
    if let Some(code) = slot.as_i64() {
        return Some(code);
    }
    slot.as_str().and_then(|text| text.trim().parse().ok())
}

/// A tail that starts in the middle of a line is not valid JSON. The item
/// still names `CommandExecution`, its `id`, and `exit_code`. A cut-off
/// function output still says `Process exited with code N` after `call_id`.
fn exit_in_text(line: &str, call_id: &str, found: &mut TranscriptExit) {
    let mut from = 0;
    while let Some(rel) = line[from..].find("CommandExecution") {
        let start = from + rel;
        let rest = start + "CommandExecution".len();
        let end = line[rest..]
            .find("CommandExecution")
            .map(|rel| rest + rel)
            .unwrap_or(line.len());
        let window = &line[start..end];
        if find_key(window, "\"id\"", call_id).is_some() {
            if let Some(code) = exit_code_in(window) {
                found.item = Some(code);
            }
        }
        from = rest;
    }
    let mut from = 0;
    while let Some(start) = find_key(&line[from..], "\"call_id\"", call_id) {
        let start = from + start;
        let after = start + "\"call_id\"".len();
        let end = line[after..]
            .find("\"call_id\"")
            .map(|rel| after + rel)
            .unwrap_or(line.len());
        if let Some(code) = process_exit_code(&line[start..end]) {
            found.spoken = Some(code);
        }
        from = after;
    }
}

fn find_key(text: &str, key: &str, want: &str) -> Option<usize> {
    let mut from = 0;
    while from < text.len() {
        let Some(rel) = text[from..].find(key) else {
            break;
        };
        let at = from + rel;
        let after = at + key.len();
        if json_string_is(text.get(after..).unwrap_or(""), want) {
            return Some(at);
        }
        from = after;
    }
    None
}

fn json_string_is(text: &str, want: &str) -> bool {
    let rest = text.trim_start();
    let Some(rest) = rest.strip_prefix(':') else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix('"') else {
        return false;
    };
    rest.strip_prefix(want)
        .is_some_and(|rest| rest.starts_with('"'))
}

fn exit_code_in(text: &str) -> Option<i64> {
    for key in ["\"exit_code\"", "\"exitCode\""] {
        let Some(at) = text.find(key) else {
            continue;
        };
        let rest = text[at + key.len()..].trim_start();
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        if let Some(code) = parse_exit_token(rest.trim_start()) {
            return Some(code);
        }
    }
    None
}

fn parse_exit_token(rest: &str) -> Option<i64> {
    let digits = |text: &str| -> Option<i64> {
        let num: String = text
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        if num.is_empty() {
            None
        } else {
            num.parse().ok()
        }
    };
    if let Some(rest) = rest.strip_prefix('"') {
        return digits(rest);
    }
    digits(rest)
}

fn explicit_exit(raw: &Value) -> Option<i64> {
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
            return Some(code);
        }
        if let Some(code) = slot.as_str() {
            return code.trim().parse().ok();
        }
    }
    None
}

/// A guard that already refused this call. A missing exit is not one of
/// these. An error string is not a guard decision: a commit is judged from
/// its exit code, then from what git wrote.
fn permission_block(raw: &Value) -> bool {
    let decisions = [
        raw.get("permissionDecision"),
        raw.get("permission_decision"),
        raw.pointer("/hookSpecificOutput/permissionDecision"),
        raw.pointer("/tool_response/permissionDecision"),
        raw.get("decision"),
    ];
    decisions.into_iter().flatten().any(|slot| {
        slot.as_str()
            .is_some_and(|word| matches!(word.trim(), "deny" | "ask" | "block"))
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

/// Whether `line` redirects into a file inside `repo`. A new file is not
/// tracked yet; `git ls-files` used to wait for a later git command before
/// the sitting had a claim. A path outside the checkout, a home path, or
/// a glob is not this work tree.
fn writes_worktree(repo: &str, line: &str) -> bool {
    let repo = Path::new(repo);
    if repo.as_os_str().is_empty() {
        return false;
    }
    redirect_targets(line)
        .into_iter()
        .any(|target| path_in_worktree(repo, &target))
}

fn path_in_worktree(repo: &Path, target: &str) -> bool {
    if target.is_empty()
        || target.starts_with('~')
        || target.contains('$')
        || target.contains('*')
        || target.starts_with('&')
    {
        return false;
    }
    let target = Path::new(target);
    let joined = if target.is_absolute() {
        target.to_path_buf()
    } else {
        repo.join(target)
    };
    let repo = lexical_norm(repo);
    let joined = lexical_norm(&joined);
    joined.starts_with(&repo) && joined != repo
}

/// `..` and `.` collapsed, without asking the filesystem. A missing file
/// is still inside the work tree when its path says so.
fn lexical_norm(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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

/// A vissue id, as `vissue-core` reads one in `is_vissue_id`: `{project}-{suffix}`
/// with the suffix after the last hyphen. `generate_id` mints that suffix from
/// `ID_ALPHABET` (`0-9a-z`) at `issues.id_length`, which defaults to 4 and is
/// at least 2. The project is the rest, so it may contain hyphens and digits
/// (`ljos-policyd-c0jc`, `lgio-1792329616-q4uh`). A nested project may contain
/// `/` (`acme/cli-1a2b`).
fn is_issue_id(token: &str) -> bool {
    let Some((project, suffix)) = token.rsplit_once('-') else {
        return false;
    };
    issue_project(project) && issue_suffix(suffix)
}

fn issue_project(project: &str) -> bool {
    let mut chars = project.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && project
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/'))
        && !project.ends_with('-')
        && !project.ends_with('/')
        && !project.contains("--")
}

fn issue_suffix(suffix: &str) -> bool {
    // Shorter than the default mint stays out. A longer configured
    // `id_length` is still the same id.
    suffix.len() >= 4
        && suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
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
    crate::git_hooks::hook_child_env(&mut cmd);
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
    use std::sync::{Arc, Mutex};

    fn lock() -> crate::EnvLock {
        crate::test_env_lock()
    }

    /// A lesson already stored keeps the title it was written under when a
    /// later prompt rewrites it. The pack closes the earlier lesson by id.
    #[test]
    fn a_rewritten_lesson_keeps_the_title_it_was_written_under() {
        let _lock = lock();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        // SAFETY: lock() restores the environment when the test returns.
        unsafe { std::env::set_var("XDG_STATE_HOME", &state) };
        let proj = tracker.join("Software/probe");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("issues.org"),
            "* TODO [#C] Add a slugify function\n:PROPERTIES:\n:ID:         probe-00aa\n:END:\n",
        )
        .unwrap();
        let repo_s = git_toplevel(&repo).expect("toplevel");
        let old = head_sha(&repo_s).expect("head");
        let new = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
        let session = "jld5-title-session";
        let now = unix_secs();
        let mut book = Book {
            prompt: "Add a slugify function".into(),
            prompt_repo: repo_s.clone(),
            prompt_unix: now,
            last_unix: now,
            ..Book::default()
        };
        book.repos.insert(
            repo_s.clone(),
            RepoBook {
                issue: Some("probe-00aa".into()),
                title: Some("Add a slugify function".into()),
                deeds: vec![old.clone()],
                finished: true,
                lesson: true,
                lesson_id: Some("atom-old".into()),
                last_unix: now,
                ..RepoBook::default()
            },
        );
        store_book(&book_path(session), &book);

        let (raw, call) = payload(
            "UserPromptSubmit",
            session,
            Path::new(&repo_s),
            serde_json::json!({"prompt": "Reword the message to say something else entirely"}),
        );
        let _ = observe(&raw, &call);
        repoint_commits(&repo_s, &[(old, new)]);

        let queued =
            std::fs::read_to_string(state.join("ljos/note-queue.jsonl")).unwrap_or_default();
        assert!(
            queued.contains("slugify"),
            "the rewrite dropped the original title\n{queued}"
        );
        assert!(
            !queued.contains("Reword"),
            "the rewrite took the later prompt\n{queued}"
        );
        assert!(
            queued.contains("atom-old"),
            "the queued rewrite names nothing it closes\n{queued}"
        );
    }

    /// A row that once held another issue does not carry that issue's lesson
    /// title or superseded atom into the next issue's rewrite. The rewrite
    /// names the second issue's own title.
    #[test]
    fn a_second_issue_does_not_rewrite_with_the_first_issue_lesson() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "VISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_TRACKER_GIT",
            "LJOS_PROJECT",
            "XDG_RUNTIME_DIR",
            "XDG_STATE_HOME",
            "LJOS_IN_HOOK",
            "PACKSET_URL",
            "ISSUE_ROOT",
            "LJOS_OFF",
            "LJOS_SESSION_ID",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        // SAFETY: lock() restores the environment when the test returns.
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &state);
            std::env::set_var("LJOS_SESSION_ID", "jld5-second-issue");
        }
        let proj = tracker.join("Software/probe");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("issues.org"),
            "\
* TODO [#C] Add a slugify function
:PROPERTIES:
:ID:         probe-00aa
:END:
* TODO [#C] Now add a kebab-case helper
:PROPERTIES:
:ID:         probe-00ab
:END:
",
        )
        .unwrap();
        let repo_s = git_toplevel(&repo).expect("toplevel");
        let old = head_sha(&repo_s).expect("head");
        let new = "cccccccccccccccccccccccccccccccccccccccc".to_string();
        let now = unix_secs();
        let mut book = Book {
            prompt: "Add a slugify function".into(),
            prompt_repo: repo_s.clone(),
            prompt_unix: now,
            last_unix: now,
            ..Book::default()
        };
        book.repos.insert(
            repo_s.clone(),
            RepoBook {
                issue: Some("probe-00aa".into()),
                title: Some("Add a slugify function".into()),
                deeds: vec![old.clone()],
                finished: true,
                lesson: true,
                lesson_id: Some("atom-a".into()),
                lesson_title: Some("Add a slugify function".into()),
                last_unix: now,
                ..RepoBook::default()
            },
        );
        store_book(&book_path("jld5-second-issue"), &book);

        let said = start_work(
            Some("probe-00ab"),
            Some(repo.to_str().unwrap()),
            "Now add a kebab-case helper",
        )
        .expect("start the second issue");
        assert!(said.contains("probe-00ab"), "{said}");

        let path = book_path("jld5-second-issue");
        let mut switched = load_book(&path);
        switched.prompt = "Now add a kebab-case helper".into();
        bind_prompt(&mut switched, &repo_s, unix_secs());
        let row = switched.repos.get_mut(&repo_s).expect("repo row");
        row.title = Some("Now add a kebab-case helper".into());
        row.deeds = vec![old.clone()];
        row.finished = true;
        row.lesson = true;
        store_book(&path, &switched);

        repoint_commits(&repo_s, &[(old, new)]);
        let queued =
            std::fs::read_to_string(state.join("ljos/note-queue.jsonl")).unwrap_or_default();
        assert!(
            queued.contains("kebab case helper"),
            "the second issue title was lost\n{queued}"
        );
        assert!(
            !queued.contains("slugify"),
            "the first issue title leaked into the rewrite\n{queued}"
        );
        assert!(
            !queued.contains("atom-a"),
            "the second issue rewrite superseded the first issue atom\n{queued}"
        );
    }

    /// A sitting that moves from one issue to another must not book the
    /// first issue's commits on the second. The baseline stays at the old
    /// HEAD otherwise, and the next citation walks that range again.
    #[test]
    fn a_rebind_does_not_cite_the_previous_issues_commits() {
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
            "LJOS_SEAT",
            "LJOS_HARNESS_SESSION",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "zdnr4a-rebind";
        unsafe {
            std::env::set_var("LJOS_SESSION_ID", session);
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("LJOS_HARNESS_SESSION");
        }
        let proj = tracker.join("Software/probe");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("issues.org"),
            "\
* TODO [#C] First issue
:PROPERTIES:
:ID:         probe-00aa
:END:
* TODO [#C] Second issue
:PROPERTIES:
:ID:         probe-00ab
:END:
",
        )
        .unwrap();
        let repo_s = git_toplevel(&repo).expect("toplevel");
        let seed = head_sha(&repo_s).expect("seed");
        let first = land_commit(&repo, "first.txt", "work for the first issue");
        let now = unix_secs();
        let mut book = Book {
            last_unix: now,
            ..Book::default()
        };
        book.repos.insert(
            repo_s.clone(),
            RepoBook {
                issue: Some("probe-00aa".into()),
                start_sha: Some(seed),
                deeds: vec![first.clone()],
                started: true,
                last_unix: now,
                ..RepoBook::default()
            },
        );
        store_book(&book_path(session), &book);

        let said = start_work(
            Some("probe-00ab"),
            Some(repo.to_str().unwrap()),
            "Continue on the second issue",
        )
        .expect("rebind");
        assert!(said.contains("probe-00ab"), "{said}");
        assert!(
            cite_into_open_sitting(&repo_s),
            "the open sitting was not cited"
        );
        let row = load_book(&book_path(session))
            .repos
            .get(&repo_s)
            .cloned()
            .expect("row");
        assert_eq!(row.issue.as_deref(), Some("probe-00ab"));
        assert!(
            !row.deeds.iter().any(|sha| sha == &first),
            "rebind cited the previous issue's commit: {:?}",
            row.deeds
        );
        let later = land_commit(&repo, "later.txt", "work on the second issue");
        assert!(cite_into_open_sitting(&repo_s));
        let row = load_book(&book_path(session))
            .repos
            .get(&repo_s)
            .cloned()
            .expect("row after later commit");
        assert!(
            row.deeds.iter().any(|sha| sha == &later),
            "the commit after the rebind was not cited: {:?}",
            row.deeds
        );
        assert!(
            !row.deeds.iter().any(|sha| sha == &first),
            "the later citation still carried the previous commit: {:?}",
            row.deeds
        );
    }

    /// Folding another issue into a row must not keep the baseline the
    /// previous issue was citing from.
    #[test]
    fn adopt_row_rebind_does_not_cite_the_previous_issues_commits() {
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
            "LJOS_SEAT",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let proj = tracker.join("Software/probe");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("issues.org"),
            "\
* TODO [#C] First issue
:PROPERTIES:
:ID:         probe-00aa
:END:
* TODO [#C] Second issue
:PROPERTIES:
:ID:         probe-00ab
:END:
",
        )
        .unwrap();
        let repo_s = git_toplevel(&repo).expect("toplevel");
        let seed = head_sha(&repo_s).expect("seed");
        let first = land_commit(&repo, "first.txt", "work for the first issue");
        let mut book = Book::default();
        book.repos.insert(
            repo_s.clone(),
            RepoBook {
                issue: Some("probe-00aa".into()),
                start_sha: Some(seed.clone()),
                deeds: vec![first.clone()],
                ..RepoBook::default()
            },
        );
        adopt_row(
            &mut book,
            &repo_s,
            "probe-00ab",
            RepoBook {
                start_sha: Some(seed),
                ..RepoBook::default()
            },
        );
        cite_row(&mut book, &repo_s, &repo_s).expect("cite");
        let row = book.repos.get(&repo_s).expect("row");
        assert_eq!(row.issue.as_deref(), Some("probe-00ab"));
        assert!(
            !row.deeds.iter().any(|sha| sha == &first),
            "adopt cited the previous issue's commit: {:?}",
            row.deeds
        );
    }

    /// Starting the issue a sitting already holds must not clear `named`
    /// or `filed`. Finish then releases a named ticket instead of closing
    /// it DONE. A real rebind still resets both flags.
    #[test]
    fn start_work_on_the_same_issue_keeps_named_and_filed() {
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
            "LJOS_SEAT",
            "LJOS_HARNESS_SESSION",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "zdnr4a-same-issue";
        unsafe {
            std::env::set_var("LJOS_SESSION_ID", session);
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("LJOS_HARNESS_SESSION");
        }
        let proj = tracker.join("Software/probe");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("issues.org"),
            "\
* TODO [#C] Named ticket
:PROPERTIES:
:ID:         probe-00aa
:END:
* TODO [#C] Other ticket
:PROPERTIES:
:ID:         probe-00ab
:END:
",
        )
        .unwrap();
        let repo_s = git_toplevel(&repo).expect("toplevel");
        let mut book = Book::default();
        book.repos.insert(
            repo_s.clone(),
            RepoBook {
                issue: Some("probe-00aa".into()),
                named: true,
                filed: true,
                started: true,
                ..RepoBook::default()
            },
        );
        store_book(&book_path(session), &book);
        start_work(
            Some("probe-00aa"),
            Some(repo.to_str().unwrap()),
            "Keep working on the named ticket",
        )
        .expect("start the same issue");
        let row = load_book(&book_path(session))
            .repos
            .get(&repo_s)
            .cloned()
            .expect("row");
        assert!(row.named, "same-issue start cleared named");
        assert!(row.filed, "same-issue start cleared filed");
        let mut book = load_book(&book_path(session));
        finish_open(&mut book);
        assert_ne!(
            issue_state(&tracker, "probe-00aa"),
            "DONE",
            "a named ticket was closed instead of handed back"
        );

        let mut again = Book::default();
        again.repos.insert(
            repo_s.clone(),
            RepoBook {
                issue: Some("probe-00aa".into()),
                named: true,
                filed: true,
                started: true,
                ..RepoBook::default()
            },
        );
        store_book(&book_path(session), &again);
        start_work(
            Some("probe-00ab"),
            Some(repo.to_str().unwrap()),
            "Move the sitting to the other ticket",
        )
        .expect("rebind");
        let row = load_book(&book_path(session))
            .repos
            .get(&repo_s)
            .cloned()
            .expect("rebound row");
        assert!(!row.named, "a rebind kept named");
        assert!(!row.filed, "a rebind kept filed");
    }

    /// A fresh claim on a row that had no issue clears stale lesson linkage
    /// before it binds the new issue.
    #[test]
    fn ensure_claim_clears_stale_lesson_before_setting_new_issue() {
        let _g = lock();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let repo_s = git_toplevel(&repo).expect("repo toplevel");
        let mut book = Book {
            prompt: "write a kebab-case helper".into(),
            ..Book::default()
        };
        book.repos.insert(
            repo_s.clone(),
            RepoBook {
                lesson: true,
                lesson_id: Some("atom-old".into()),
                lesson_title: Some("old lesson".into()),
                deeds: vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()],
                failures: vec!["old failure".into()],
                asks: vec!["old ask".into()],
                title: Some("old title".into()),
                issue: None,
                ..RepoBook::default()
            },
        );
        ensure_claim(&mut book, &book_path("ensure-claim-clear"), &repo_s, None).unwrap();
        let row = book.repos.get(&repo_s).expect("repo row");
        let issue = row.issue.clone().expect("new issue");
        assert!(issue.starts_with("probe-"), "{issue}");
        assert!(!row.lesson);
        assert!(row.lesson_id.is_none());
        assert!(row.lesson_title.is_none());
    }

    /// willma-20261020f w4, w8 and w12: agy 1.4.3's `write_file` filed no
    /// claim, and the claim came later with `edit_file` or a shell command.
    /// Every agy file tool those cells sent is an edit; a read is not.
    #[test]
    fn every_agy_file_tool_is_an_edit() {
        for name in [
            "write_file",
            "edit_file",
            "write_to_file",
            "replace_file_content",
        ] {
            let raw =
                serde_json::json!({"toolCall": {"name": name, "args": {"Path": "/w/slug.py"}}});
            assert!(is_edit(&raw, name), "{name}");
        }
        let read = serde_json::json!({"toolCall": {"name": "view_file", "args": {}}});
        assert!(!is_edit(&read, "view_file"));
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
    fn a_commit_names_the_first_open_issue_the_tracker_holds() {
        let state = |id: &str| match id {
            "ljos-policyd-32fx" | "ljos-xsor" => Some("TODO".to_string()),
            "ljos-cjd8" => Some("STARTED".to_string()),
            "ljos-done" => Some("DONE".to_string()),
            _ => None,
        };
        let pick = |m: &str| named_open_issue_with(m, state);
        assert_eq!(
            pick("ljos-policyd-32fx: claim the named id").as_deref(),
            Some("ljos-policyd-32fx")
        );
        assert_eq!(
            pick("tidy the hook\n\nFixes ljos-xsor.").as_deref(),
            Some("ljos-xsor")
        );
        assert_eq!(pick("(ljos-cjd8) reopen").as_deref(), Some("ljos-cjd8"));
        assert_eq!(
            pick("fix-hook: ljos-done then ljos-xsor").as_deref(),
            Some("ljos-xsor")
        );
        assert!(pick("fix-hook: ljos-done and ljos-gone").is_none());
        assert!(pick("no id here").is_none());
    }

    #[test]
    fn a_hyphenated_project_id_is_a_vissue_id() {
        for id in [
            "ljos-policyd-c0jc",
            "lgio-1792329616-q4uh",
            "probe-0001",
            "acme/cli-1a2b",
        ] {
            assert_eq!(parse_issue_id(id).as_deref(), Some(id), "{id}");
        }
        assert_eq!(
            parse_issue_id("created ljos-policyd-c0jc.").as_deref(),
            Some("ljos-policyd-c0jc")
        );
        assert!(parse_issue_id("ljos-policyd-C0JC").is_none());
        assert!(parse_issue_id("not-an-id").is_none());
        assert!(parse_issue_id("well").is_none());
    }

    #[test]
    fn a_bookkeep_test_does_not_read_the_login_user_rules() {
        let home = Path::new("/home/someone");
        let got = super::super::resolve_config_base(None, Some(home)).expect("isolated base");
        assert_ne!(got, home.join(".config"));
        assert!(!got.starts_with(home), "{got:?}");
        let rules = super::super::user_rules::rules_path().expect("rules path");
        if std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .is_none()
        {
            let login = std::env::var_os("HOME").map(PathBuf::from);
            if let Some(login) = login {
                assert!(!rules.starts_with(&login), "{rules:?} is the login config");
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
        let _saved = Restore::new(&["HOME", "LJOS_OFF", "LJOS_GUARD"]);
        unsafe {
            std::env::remove_var("LJOS_OFF");
            std::env::remove_var("LJOS_GUARD");
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
        unsafe { std::env::set_var("LJOS_GUARD", "on") };
        assert!(!guard_suspended(), "LJOS_GUARD=on judges a temp HOME");
        assert!(guard_off_reason().is_none());
        unsafe { std::env::set_var("LJOS_OFF", "1") };
        assert!(guard_suspended(), "LJOS_OFF wins over LJOS_GUARD");
        unsafe {
            std::env::remove_var("LJOS_OFF");
            std::env::remove_var("LJOS_GUARD");
            std::env::set_var("HOME", "/tmpfoo");
        }
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

    #[test]
    fn a_prompt_field_does_not_hide_the_commit_command() {
        let raw = r#"{"hook_event_name":"PostToolUse","prompt":"please commit the readme","tool_input":{"command":"git commit -m readme"}}"#;
        let call = crate::hook_call(raw);
        assert!(
            is_git_commit(&call.cue),
            "the prompt hid the commit: {}",
            call.cue
        );
    }

    #[test]
    fn owner_pid_stays_put_on_a_tool_hook() {
        assert_eq!(next_owner("SessionStart", (0, 0), (10, 1)), (10, 1));
        assert_eq!(next_owner("UserPromptSubmit", (10, 1), (42, 2)), (42, 2));
        assert_eq!(next_owner("PostToolUse", (10, 1), (99, 3)), (10, 1));
        assert_eq!(next_owner("PreToolUse", (10, 1), (99, 3)), (10, 1));
        assert_eq!(next_owner("Stop", (10, 1), (99, 3)), (10, 1));
        assert_eq!(next_owner("PostToolUse", (0, 0), (99, 3)), (99, 3));
    }

    #[test]
    fn an_owner_pid_that_started_again_is_gone() {
        let me = crate::proc_tree::of(std::process::id()).unwrap();
        let owned = |pid, start| Book {
            owner_pid: pid,
            owner_start: start,
            ..Book::default()
        };
        assert!(owner_alive(&owned(me.pid, me.start)));
        assert!(!owner_alive(&owned(me.pid, me.start + 1)), "a reused pid");
        assert!(owner_alive(&owned(me.pid, 0)), "a book with no start");
    }

    #[test]
    fn an_old_row_idles_while_the_book_clock_is_fresh() {
        let limit = Duration::from_secs(2 * 60 * 60);
        let now = 1_000_000_u64;
        let mut book = Book {
            last_unix: now,
            ..Book::default()
        };
        book.repos.insert(
            "/repo-a".into(),
            RepoBook {
                issue: Some("probe-aaaa".into()),
                last_unix: 10,
                ..RepoBook::default()
            },
        );
        book.repos.insert(
            "/repo-b".into(),
            RepoBook {
                issue: Some("probe-bbbb".into()),
                last_unix: now,
                ..RepoBook::default()
            },
        );
        let idle = row_is_idle(
            now,
            &book,
            "/repo-a",
            book.repos.get("/repo-a").unwrap(),
            limit,
            Some("/repo-b"),
        );
        let fresh = row_is_idle(
            now,
            &book,
            "/repo-b",
            book.repos.get("/repo-b").unwrap(),
            limit,
            Some("/repo-b"),
        );
        assert!(idle, "repo A was last touched at 10");
        assert!(!fresh, "repo B was touched with the book");

        let unclocked = RepoBook {
            issue: Some("probe-cccc".into()),
            ..RepoBook::default()
        };
        assert!(
            row_is_idle(now, &book, "/repo-c", &unclocked, limit, Some("/repo-b")),
            "a row with no clock is not the checkout that refreshed the book"
        );
        assert!(
            !row_is_idle(now, &book, "/repo-b", &unclocked, limit, Some("/repo-b")),
            "the current checkout keeps the fresh book clock"
        );
    }

    #[test]
    fn a_prompt_for_another_repo_is_not_a_live_title() {
        let _g = lock();
        let _saved = Restore::new(&["LJOS_SITTING_IDLE"]);
        unsafe { std::env::remove_var("LJOS_SITTING_IDLE") };
        let now = 1_000_000_u64;
        let mut book = Book {
            prompt: "notes: add a second line".into(),
            prompt_repo: "/other".into(),
            prompt_unix: now,
            last_unix: now,
            ..Book::default()
        };
        assert!(
            !prompt_live_for(&book, "/repo", now),
            "the prompt belongs to /other"
        );
        book.prompt_repo = "/repo".into();
        book.repos.insert(
            "/repo".into(),
            RepoBook {
                issue: Some("probe-aaaa".into()),
                title: Some(book.prompt.clone()),
                last_unix: now,
                ..RepoBook::default()
            },
        );
        assert!(
            prompt_live_for(&book, "/repo", now),
            "the current sitting in this checkout keeps its prompt"
        );
        book.repos.get_mut("/repo").unwrap().last_unix = 10;
        assert!(
            !prompt_live_for(&book, "/repo", now),
            "an earlier sitting's prompt does not title the next claim"
        );
    }

    #[test]
    fn a_commit_only_sitting_idles_after_two_hours() {
        assert_eq!(
            sitting_idle_from(None),
            Some(Duration::from_secs(2 * 60 * 60))
        );
        assert_eq!(
            sitting_idle_from(Some("")),
            Some(Duration::from_secs(2 * 60 * 60))
        );
        assert_eq!(sitting_idle_from(Some("0")), None);
        assert_eq!(sitting_idle_from(Some("off")), None);
        assert_eq!(
            sitting_idle_from(Some("30m")),
            Some(Duration::from_secs(1800))
        );
        assert!(idle_due(10_000, 10, Duration::from_secs(2 * 60 * 60)));
        assert!(!idle_due(100, 10, Duration::from_secs(2 * 60 * 60)));
        assert!(!idle_due(10_000, 0, Duration::from_secs(1)));
    }

    #[test]
    fn a_claim_uses_a_checkout_or_the_last_booked_repo() {
        // git_toplevel looks git up with which(), so this holds the same
        // lock as the other PATH readers.
        let _g = lock();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ljos-place-test");
        let repo = root.join("repo");
        let other = root.join("other");
        let plain = root.join("plain");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&other, &["init", "-q", "-b", "main"]);
        let top = git_toplevel(&repo).unwrap();
        let other_top = git_toplevel(&other).unwrap();
        assert_eq!(
            place_for_claim_in(Some(repo.to_str().unwrap()), None, Some(&plain)).as_deref(),
            Some(top.as_str())
        );
        assert_eq!(
            place_for_claim_in(Some(plain.to_str().unwrap()), Some(&other_top), Some(&repo)),
            None,
            "a named path that is not a checkout does not fall back"
        );
        assert_eq!(
            place_for_claim_in(None, Some(&other_top), Some(&plain)).as_deref(),
            Some(other_top.as_str())
        );
        assert_eq!(place_for_claim_in(None, None, Some(&plain)), None);
        assert_eq!(
            place_for_claim_in(None, None, Some(&repo)).as_deref(),
            Some(top.as_str())
        );
    }

    #[test]
    fn project_for_matches_the_remote_before_the_basename() {
        // The remote lookup runs git through which(), so this holds the
        // same lock as the other PATH readers.
        let _g = lock();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ljos-project-test");
        let tracker = root.join("tracker");
        let repo = root.join("workspace");
        std::fs::create_dir_all(tracker.join("Software/ljos")).unwrap();
        std::fs::create_dir_all(tracker.join("Software/other")).unwrap();
        std::fs::write(
            tracker.join("Software/ljos/issues.org"),
            "#+REMOTE: https://github.com/acme/ljos.git\n",
        )
        .unwrap();
        std::fs::write(tracker.join("Software/other/issues.org"), "* TODO other\n").unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &["remote", "add", "origin", "git@github.com:acme/ljos.git"],
        );
        assert_eq!(
            project_for_at(repo.to_str().unwrap(), None, Some(&tracker)),
            "ljos"
        );
        assert_eq!(
            project_for_at(repo.to_str().unwrap(), Some("probe"), Some(&tracker)),
            "probe"
        );
        let bare = root.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "-q", "-b", "main"]);
        assert_eq!(
            project_for_at(bare.to_str().unwrap(), None, Some(&tracker)),
            "bare",
            "no remote and more than one project uses the basename"
        );
    }

    /// A clone in a folder named for the job (`bench-w18b-tfn7`) files on
    /// the repository its origin names, even before the tracker holds a
    /// project of that name. The folder is the fallback only with no origin.
    #[test]
    fn a_clone_folder_is_not_the_project_when_origin_names_the_repo() {
        let _g = lock();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ljos-clone-folder");
        let tracker = root.join("tracker");
        std::fs::create_dir_all(tracker.join("Software/ljos")).unwrap();
        std::fs::write(tracker.join("Software/ljos/issues.org"), "* TODO ljos\n").unwrap();
        let repo = root.join("bench-w18b-tfn7");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/leidarljos/bench.git",
            ],
        );
        assert_eq!(
            project_for_at(repo.to_str().unwrap(), None, Some(&tracker)),
            "bench"
        );
        assert_eq!(
            title_at("", repo.to_str().unwrap(), None, Some(&tracker)),
            "Work in bench"
        );
        assert_eq!(title_at("  ", "", None, Some(&tracker)), "agent work");
        let mixed = root.join("orpc-clone");
        std::fs::create_dir_all(tracker.join("Software/OmniPotentRPC")).unwrap();
        std::fs::write(
            tracker.join("Software/OmniPotentRPC/issues.org"),
            "* TODO orpc\n",
        )
        .unwrap();
        std::fs::create_dir_all(&mixed).unwrap();
        git(&mixed, &["init", "-q", "-b", "main"]);
        git(
            &mixed,
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:HaoZeke/omnipotentrpc.git",
            ],
        );
        assert_eq!(
            project_for_at(mixed.to_str().unwrap(), None, Some(&tracker)),
            "OmniPotentRPC",
            "an existing project matches whatever its case"
        );
        let bare = root.join("scratch-folder");
        std::fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "-q", "-b", "main"]);
        assert_eq!(
            project_for_at(bare.to_str().unwrap(), None, Some(&tracker)),
            "scratch-folder"
        );
        for (url, name) in [
            ("https://github.com/leidarljos/bench.git", Some("bench")),
            (
                "git@github.com:leidarljos/ljos-policyd",
                Some("ljos-policyd"),
            ),
            (
                "https://github.com/HaoZeke/haozeke.github.io/",
                Some("haozeke.github.io"),
            ),
            ("/srv/git/bench.git", None),
            ("file:///srv/git/bench.git", None),
            ("", None),
        ] {
            assert_eq!(remote_repo_name(url).as_deref(), name, "{url}");
        }
    }

    /// A checkout that really tracks `target/arm` is still that checkout.
    /// The row key and saved prompt repository must stay on the repo root.
    #[test]
    fn a_real_target_subtree_keeps_the_checkout_root() {
        let _g = lock();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("qemu");
        let arm = root.join("target/arm");
        std::fs::create_dir_all(&arm).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.email", "seat@example.invalid"]);
        git(&root, &["config", "user.name", "seat"]);
        std::fs::write(arm.join("cpu.c"), "int cpu(void) { return 0; }\n").unwrap();
        git(&root, &["add", "target/arm/cpu.c"]);
        git(&root, &["commit", "-q", "-m", "seed"]);

        let root_s = root.canonicalize().unwrap().display().to_string();
        assert_eq!(git_toplevel(&arm).as_deref(), Some(root_s.as_str()));

        let mut book = Book::default();
        let raw = serde_json::json!({"cwd": arm.display().to_string()});
        assert_eq!(repo_key(&mut book, &raw), root_s);
    }

    /// A job folder such as `ljos-rohit-main-audit` is not the project.
    /// The origin is read from the git config, including a worktree's
    /// common dir, even when `git` on PATH fails and `GIT_DIR` names
    /// another checkout. A recorded remote still wins when the tracker
    /// root comes from the vissue config and no variable names it.
    #[test]
    fn a_job_folder_files_on_the_remote_or_the_tracker() {
        use std::os::unix::fs::PermissionsExt;
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "HOME",
            "XDG_CONFIG_HOME",
            "VISSUE_ROOT",
            "ISSUE_ROOT",
            "VISSUE_CONFIG",
            "LJOS_PROJECT",
            "GIT_DIR",
            "GIT_WORK_TREE",
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ljos-job-folder");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("git"), "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let real_path = std::env::var("PATH").unwrap_or_default();
        set_var("PATH", &format!("{}:{}", bin.display(), real_path));
        for key in [
            "VISSUE_ROOT",
            "ISSUE_ROOT",
            "VISSUE_CONFIG",
            "LJOS_PROJECT",
            "GIT_DIR",
            "GIT_WORK_TREE",
        ] {
            unsafe { std::env::remove_var(key) };
        }

        let repo = root.join("ljos-rohit-main-audit");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(
            repo.join(".git/config"),
            "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = https://github.com/leidarljos/ljos.git\n",
        )
        .unwrap();
        assert_eq!(project_for_at(repo.to_str().unwrap(), None, None), "ljos");
        assert_eq!(
            title_at("", repo.to_str().unwrap(), None, None),
            "Work in ljos"
        );

        let main = root.join("main");
        let common = main.join(".git");
        let wt = common.join("worktrees/audit");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(
            common.join("config"),
            "[remote \"origin\"]\n\turl = \"https://github.com/leidarljos/ljos.git\"\n",
        )
        .unwrap();
        std::fs::write(wt.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            wt.join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/acme/decoy.git\n",
        )
        .unwrap();
        let linked = root.join("ljos-rohit-main-audit-wt");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(linked.join(".git"), format!("gitdir: {}\n", wt.display())).unwrap();
        assert_eq!(
            project_for_at(linked.to_str().unwrap(), None, None),
            "ljos",
            "a worktree config must not hide the common remote"
        );

        let tracker = root.join("tracker");
        std::fs::create_dir_all(tracker.join("Software/atlas")).unwrap();
        std::fs::write(
            tracker.join("Software/atlas/issues.org"),
            "#+REMOTE: https://github.com/acme/widget.git\n* TODO atlas\n",
        )
        .unwrap();
        std::fs::create_dir_all(tracker.join("Software/other")).unwrap();
        std::fs::write(tracker.join("Software/other/issues.org"), "* TODO other\n").unwrap();
        let home = root.join("home");
        let config = home.join(".config/vissue");
        std::fs::create_dir_all(&config).unwrap();
        let line = format!(
            "root = {}\n",
            toml::Value::String(tracker.display().to_string())
        );
        std::fs::write(config.join("config.toml"), &line).unwrap();
        set_var("HOME", home.to_str().unwrap());
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        let widget = root.join("widget-checkout");
        std::fs::create_dir_all(widget.join(".git")).unwrap();
        std::fs::write(
            widget.join(".git/config"),
            "[remote \"origin\"]\n\turl = https://github.com/acme/widget.git\n",
        )
        .unwrap();
        assert_eq!(project_for(widget.to_str().unwrap()), "atlas");
        assert_eq!(title_from("", widget.to_str().unwrap()), "Work in atlas");
        assert_eq!(
            tracker_root_from_config("root = \"~/vault\"\n").as_deref(),
            Some(home.join("vault").as_path())
        );
        assert!(tracker_root_from_config("[store]\nroot = \"/nope\"\n").is_none());

        // An origin that lives in an included file is still the checkout's,
        // and GIT_DIR must not send the lookup at a different repository.
        // `remote get-url` would also apply insteadOf and hide the name.
        unsafe { std::env::remove_var("HOME") };
        set_var("PATH", &real_path);
        let included = root.join("included");
        std::fs::create_dir_all(&included).unwrap();
        git(&included, &["init", "-q", "-b", "main"]);
        std::fs::write(
            included.join(".git/remotes"),
            "[remote \"origin\"]\n\turl = https://github.com/leidarljos/ljos.git\n",
        )
        .unwrap();
        git(&included, &["config", "--local", "include.path", "remotes"]);
        let decoy = root.join("decoy");
        std::fs::create_dir_all(&decoy).unwrap();
        git(&decoy, &["init", "-q", "-b", "main"]);
        git(
            &decoy,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widget.git",
            ],
        );
        let before_dir = std::env::var_os("GIT_DIR");
        {
            let _decoy = crate::GitCommandEnv::set([("GIT_DIR", decoy.join(".git").as_os_str())]);
            assert_eq!(
                project_for_at(included.to_str().unwrap(), None, None),
                "ljos",
                "GIT_DIR named the decoy"
            );
        }
        assert_eq!(std::env::var_os("GIT_DIR"), before_dir);
        let rewritten = root.join("bench-w18b-tfn7");
        std::fs::create_dir_all(&rewritten).unwrap();
        git(&rewritten, &["init", "-q", "-b", "main"]);
        git(
            &rewritten,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/leidarljos/bench.git",
            ],
        );
        git(
            &rewritten,
            &[
                "config",
                "--local",
                "url./srv/git/.insteadOf",
                "https://github.com/leidarljos/",
            ],
        );
        assert_eq!(
            project_for_at(rewritten.to_str().unwrap(), None, None),
            "bench",
            "insteadOf rewrote the origin into a path, so the folder became the project"
        );
        assert_eq!(
            title_at("", rewritten.to_str().unwrap(), None, None),
            "Work in bench"
        );
        assert_eq!(
            normalize_remote("https://x-access-token:abc/def@github.com/leidarljos/ljos.git"),
            "github.com/leidarljos/ljos"
        );
        assert_eq!(
            normalize_remote("example.com/user@host/repo"),
            "example.com/user@host/repo"
        );
    }

    /// A URL in a project's notes is not its remote. promo's board mentions
    /// the ljos repository; the commit still files on the slug, or on the
    /// project whose metadata records that remote.
    #[test]
    fn a_decoy_note_does_not_take_the_project() {
        // The remote lookup runs git through which(), so this holds the
        // same lock as the other PATH readers.
        let _g = lock();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ljos-decoy-remote");
        let tracker = root.join("tracker");
        let repo = root.join("workspace");
        std::fs::create_dir_all(tracker.join("Software/ljos")).unwrap();
        std::fs::create_dir_all(tracker.join("Software/promo")).unwrap();
        std::fs::write(tracker.join("Software/ljos/issues.org"), "* TODO ljos\n").unwrap();
        std::fs::write(
            tracker.join("Software/promo/issues.org"),
            "The notes mention https://github.com/leidarljos/ljos as the tool.\n* TODO promo\n",
        )
        .unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:leidarljos/ljos.git",
            ],
        );
        assert_eq!(
            project_for_at(repo.to_str().unwrap(), None, Some(&tracker)),
            "ljos",
            "a note that names the url took the project"
        );

        let atlas = tracker.join("Software/atlas");
        std::fs::create_dir_all(&atlas).unwrap();
        std::fs::write(atlas.join("issues.org"), "* TODO atlas\n").unwrap();
        std::fs::write(
            atlas.join("project.toml"),
            "remote = \"ssh://git@github.com/leidarljos/ljos.git\"\n",
        )
        .unwrap();
        std::fs::remove_dir_all(tracker.join("Software/ljos")).unwrap();
        assert_eq!(
            project_for_at(repo.to_str().unwrap(), None, Some(&tracker)),
            "atlas",
            "the recorded remote lost to a note"
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
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
    iid = rest[0]
    if "--remove" in rest:
        acc = rest[rest.index("--remove") + 1]
        line = f"cited {acc}\n"
        for f in files():
            text = f.read_text()
            if f":ID:         {iid}\n" in text:
                f.write_text(text.replace(line, "", 1))
                sys.exit(0)
        sys.exit(0)
    acc = rest[rest.index("--add") + 1]
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
            // The host signs every commit, and that signer takes a lock the
            // whole machine shares. A transcript budget must not include it.
            git(root, &["config", "commit.gpgsign", "false"]);
            git(root, &["config", "core.fsmonitor", "false"]);
        }
        std::fs::write(repo.join("README.md"), "seed\n").unwrap();
        git(&repo, &["add", "README.md"]);
        git(&repo, &["commit", "-q", "-m", "seed"]);
        habitat_bin(&dir.path().join("bin"));
        (dir, tracker, repo)
    }

    fn arm(dir: &Path, tracker: &Path) -> crate::PathGuard {
        let bins = crate::prepend_path(&dir.join("bin"));
        unsafe {
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
        bins
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

    fn creates(tracker: &Path) -> usize {
        let log = std::fs::read_to_string(tracker.join("vissue.log")).unwrap_or_default();
        log.matches("\ncreate").count() + usize::from(log.starts_with("create"))
    }

    /// A redirect into this checkout claims on the call that writes it, for
    /// every harness shell shape, including a file git does not track yet.
    /// A redirect outside the checkout does not.
    fn assert_redirect_claims(repo: &Path, tracker: &Path) {
        let cwd = repo.display().to_string();
        let repo_s = cwd.clone();
        let claim = |session: &str, raw: Value, file: &str, want: bool| {
            let text = raw.to_string();
            let call = crate::hook_call(&text);
            assert!(
                matches!(call.event.as_str(), "PreToolUse" | "PostToolUse"),
                "{session}: {}",
                call.event
            );
            assert!(
                call.cue.contains(file),
                "{session} dropped the redirect: {}",
                call.cue
            );
            assert_eq!(
                writes_worktree(&repo_s, &call.cue),
                want,
                "{session} worktree {}",
                call.cue
            );
            let before = creates(tracker);
            let _ = observe(&text, &call);
            assert_eq!(
                creates(tracker) > before,
                want,
                "{session} claim {before} -> {}",
                creates(tracker)
            );
        };
        claim(
            "claude-pre",
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "session_id": "redir-claude-pre",
                "cwd": cwd,
                "tool_name": "Bash",
                "tool_input": {"command": "echo hi > kebab_claude.py"}
            }),
            "kebab_claude.py",
            true,
        );
        claim(
            "opencode-post",
            serde_json::json!({
                "hook_event_name": "PostToolUse",
                "session_id": "redir-opencode-post",
                "cwd": cwd,
                "tool_name": "Bash",
                "tool_input": {"command": "cat > fresh_opencode.txt"},
                "tool_response": {"exit_code": 0}
            }),
            "fresh_opencode.txt",
            true,
        );
        claim(
            "codex-pre",
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "session_id": "redir-codex-pre",
                "turn_id": "t-redir-pre",
                "cwd": cwd,
                "tool_name": "Bash",
                "tool_input": {"command": "echo hi > kebab_codex.py"}
            }),
            "kebab_codex.py",
            true,
        );
        claim(
            "codex-post",
            serde_json::json!({
                "hook_event_name": "PostToolUse",
                "session_id": "redir-codex-post",
                "turn_id": "t-redir-post",
                "cwd": cwd,
                "tool_name": "Bash",
                "tool_input": {"command": "cat > fresh_codex.txt"},
                "tool_response": {"exit_code": 0}
            }),
            "fresh_codex.txt",
            true,
        );
        claim(
            "agy-pre",
            serde_json::json!({
                "conversationId": "redir-agy-pre",
                "cwd": cwd,
                "toolCall": {"name": "run_command", "args": {"CommandLine": "echo hi > kebab_agy.py"}}
            }),
            "kebab_agy.py",
            true,
        );
        claim(
            "agy-post",
            serde_json::json!({
                "conversationId": "redir-agy-post",
                "cwd": cwd,
                "error": "",
                "toolCall": {"name": "run_command", "args": {"CommandLine": "cat > fresh_agy.txt"}}
            }),
            "fresh_agy.txt",
            true,
        );
        claim(
            "cursor-shell",
            serde_json::json!({
                "cursor_version": "3.2.0",
                "conversation_id": "redir-cursor-shell",
                "hook_event_name": "beforeShellExecution",
                "command": "echo hi > kebab_cursor.py",
                "cwd": cwd
            }),
            "kebab_cursor.py",
            true,
        );
        claim(
            "cursor-post",
            serde_json::json!({
                "cursor_version": "3.2.0",
                "conversation_id": "redir-cursor-post",
                "hook_event_name": "postToolUse",
                "cwd": cwd,
                "tool_name": "Shell",
                "tool_input": {"command": "cat > fresh_cursor.txt"}
            }),
            "fresh_cursor.txt",
            true,
        );
        claim(
            "grok-pre",
            serde_json::json!({
                "hookEventName": "preToolUse",
                "sessionId": "redir-grok-pre",
                "cwd": cwd,
                "toolName": "run_terminal_command",
                "toolInput": {"command": "echo hi > kebab_grok.py"}
            }),
            "kebab_grok.py",
            true,
        );
        claim(
            "grok-post",
            serde_json::json!({
                "hookEventName": "postToolUse",
                "sessionId": "redir-grok-post",
                "cwd": cwd,
                "toolInput": {"command": "cat > fresh_grok.txt"}
            }),
            "fresh_grok.txt",
            true,
        );
        claim(
            "gemini-pre",
            serde_json::json!({
                "hook_event_name": "pre_tool_call",
                "session_id": "redir-gem-pre",
                "cwd": cwd,
                "tool_name": "terminal",
                "tool_input": {"command": "echo hi>kebab_gem.py"}
            }),
            "kebab_gem.py",
            true,
        );
        claim(
            "gemini-post",
            serde_json::json!({
                "hook_event_name": "post_tool_call",
                "session_id": "redir-gem-post",
                "cwd": cwd,
                "tool_name": "terminal",
                "tool_input": {"command": "echo more >> fresh_gem.txt"}
            }),
            "fresh_gem.txt",
            true,
        );
        claim(
            "outside-tmp",
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "session_id": "redir-outside-tmp",
                "cwd": cwd,
                "tool_name": "Bash",
                "tool_input": {"command": "echo hi > /tmp/ljos-not-in-the-worktree.txt"}
            }),
            "/tmp/ljos-not-in-the-worktree.txt",
            false,
        );
        claim(
            "outside-parent",
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "session_id": "redir-outside-parent",
                "cwd": cwd,
                "tool_name": "Bash",
                "tool_input": {"command": "echo hi > ../outside.txt"}
            }),
            "../outside.txt",
            false,
        );
        claim(
            "outside-home",
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "session_id": "redir-outside-home",
                "cwd": cwd,
                "tool_name": "Bash",
                "tool_input": {"command": "echo hi > \"$HOME/.bashrc\""}
            }),
            "$HOME/.bashrc",
            false,
        );
        claim(
            "outside-tilde",
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "session_id": "redir-outside-tilde",
                "cwd": cwd,
                "tool_name": "Bash",
                "tool_input": {"command": "echo hi > ~/notes.txt"}
            }),
            "~/notes.txt",
            false,
        );
    }

    #[test]
    fn an_ending_sitting_opens_no_row_in_a_checkout_it_never_worked_in() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        // The shape an MCP server leaves at exit after answering only
        // guard checks: SessionEnd on the seat's book, which holds no row.
        let session = "seat:probe-guard";
        for event in ["Stop", "SessionEnd"] {
            let (raw, call) = payload(event, session, &repo, Value::Null);
            let _ = observe(&raw, &call);
        }
        let book = load_book(&book_path(session));
        assert!(
            book.repos.is_empty(),
            "no row with a null issue: {:?}",
            book.repos.keys().collect::<Vec<_>>()
        );
        assert_eq!(creates(&tracker), 0);
    }

    #[test]
    fn a_shell_redirect_into_the_work_tree_claims_on_that_call() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        assert_redirect_claims(&repo, &tracker);
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
        let _bins = arm(dir.path(), &tracker);
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
        // The commit runs between the hooks and moves HEAD past the sha this
        // sitting started on. The post cites that new sha.
        commit_file(&repo, "README.md", "hello from the session\n");
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
        // Hide vissue, which lives outside the coreutils directories.
        // cat and sh stay on PATH.
        let _bins = crate::set_path(crate::path_without(&["vissue"]));
        unsafe {
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

    /// A write tool files and claims nothing. With nothing held it runs
    /// unattached; after a hook's edit claimed an issue in the checkout,
    /// it joins that issue and still files nothing.
    #[test]
    fn a_write_tool_files_no_issue_and_joins_the_held_one() {
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
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SESSION_ID", "mcp-session-9");
        }
        assert!(!mcp_tool_writes("ljos_claim"));
        assert!(!mcp_tool_writes("ljos_file"));
        assert!(!mcp_tool_writes("ljos_finish"));
        assert!(mcp_tool_writes("ljos_remember"));
        assert!(mcp_tool_writes("ljos_prefer"));
        let mut args = serde_json::Map::new();
        args.insert(
            "text".into(),
            Value::String("the readme stays short".into()),
        );
        args.insert("repo".into(), Value::String(repo.display().to_string()));
        for tool in MCP_WRITES {
            before_mcp_write(tool, Some(&args));
        }
        assert_eq!(creates(&tracker), 0, "a write tool filed an issue");
        let logged =
            std::fs::read_to_string(dir.path().join("run/ljos/book.log")).unwrap_or_default();
        assert!(logged.contains("ran unattached"), "{logged}");
        let session = "hook-edit-then-remember";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        assert_eq!(creates(&tracker), 1);
        args.insert("text".into(), Value::String("the notes stay dated".into()));
        before_mcp_write("ljos_remember", Some(&args));
        before_mcp_write("ljos_prefer", Some(&args));
        assert_eq!(creates(&tracker), 1, "a write tool filed a second issue");
    }

    #[test]
    fn mail_and_notes_from_the_server_file_no_claim() {
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
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SESSION_ID", "mcp-mail-1");
        }
        assert!(!mcp_tool_writes("ljos_send"));
        assert!(!mcp_tool_writes("ljos_reply"));
        let mut args = serde_json::Map::new();
        args.insert("to".into(), Value::String("disk-saver".into()));
        args.insert(
            "text".into(),
            Value::String(
                "Hourly poke from Inky: pick up your outstanding queue now and make progress"
                    .into(),
            ),
        );
        before_mcp_write("ljos_send", Some(&args));
        before_mcp_write("ljos_reply", Some(&args));
        // A note names its issue; it never files one titled by its text
        // (willma-20261018e w4 work-d3pl, w11 work-0v1j).
        before_mcp_write("ljos_note", Some(&args));
        let log = std::fs::read_to_string(tracker.join("vissue.log")).unwrap_or_default();
        assert!(
            !log.contains("create"),
            "mail or a note filed an issue\n{log}"
        );
        let book = std::fs::read_to_string(book_path(&sitting_key())).unwrap_or_default();
        assert!(
            !book.contains("Hourly poke"),
            "mail text became the prompt\n{book}"
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

    /// Codex onboarded with `LJOS_SEAT=codex` on a tracker whose only
    /// codex claims were `codex-vax0` (the `SEAT-TAG` of ljos 0.31) and
    /// bare thread ids. Every call failed with "no seat named codex" and
    /// a list of thread ids. A harness id and a name from the environment
    /// are the seat; only a name the call made up is refused, briefly.
    #[test]
    fn a_harness_or_environment_seat_is_never_refused() {
        let _g = lock();
        let _saved = Restore::new(&[
            "HOME",
            "VISSUE_ROOT",
            "ISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_SEAT",
            "VISSUE_AGENT",
            "XDG_RUNTIME_DIR",
        ]);
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().join("run");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            dir.path().join("vissue.toml"),
            "seats = [\"inky\", \"free-claude\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("Software/probe")).unwrap();
        std::fs::write(
            dir.path().join("Software/probe/issues.org"),
            "* STARTED [#C] one\n:PROPERTIES:\n:ID:         probe-abcd\n:CLAIMED_BY: codex-vax0\n:END:\n\
             * STARTED [#C] two\n:PROPERTIES:\n:ID:         probe-efgh\n\
             :CLAIMED_BY: 01a0dab3-895e-7533-93ca-c9807d5cb680\n:END:\n",
        )
        .unwrap();
        unsafe {
            std::env::set_var("HOME", dir.path().join("home"));
            std::env::set_var("VISSUE_ROOT", dir.path());
            std::env::set_var("VISSUE_NO_ROUTE", "1");
            std::env::set_var("XDG_RUNTIME_DIR", &run);
            std::env::remove_var("ISSUE_ROOT");
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("VISSUE_AGENT");
        }
        let known = crate::tracker_known_seats();
        assert!(known.contains("codex"), "{known:?}");
        assert!(!known.contains("codex-vax0"), "{known:?}");
        assert!(!known.iter().any(|s| s.contains("01a0dab3")), "{known:?}");
        let seat = |name: &str| crate::resolve_call_seat(Some(name)).unwrap().seat;
        assert_eq!(seat("codex"), "codex", "a legacy holder names the seat");
        assert_eq!(seat("claude"), "free-claude", "the harness's one seat here");
        assert_eq!(seat("opencode"), "opencode", "a harness with no seat here");
        unsafe {
            std::env::set_var("LJOS_SEAT", "newbot");
        }
        assert_eq!(crate::resolve_call_seat(None).unwrap().seat, "newbot");
        assert_eq!(
            seat("codex"),
            "newbot",
            "LJOS_SEAT wins over the harness id"
        );
        unsafe {
            std::env::remove_var("LJOS_SEAT");
        }
        let err = crate::resolve_call_seat(Some("nobody"))
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("no seat named nobody on this tracker."),
            "{err}"
        );
        assert!(err.contains("codex, free-claude, inky"), "{err}");
        assert!(err.contains("LJOS_SEAT"), "{err}");
        assert!(!err.contains("01a0dab3"), "{err}");
        assert!(!err.contains("codex-vax0"), "{err}");
    }

    /// A roster that still lists a raw holder, and a `CLAIMED_BY` of one,
    /// names the seat. The subagent name is not a seat. Neither file is
    /// rewritten.
    #[test]
    fn a_raw_holder_on_the_roster_is_the_seat_and_the_file_stays() {
        let _g = lock();
        let _saved = Restore::new(&[
            "HOME",
            "XDG_CONFIG_HOME",
            "VISSUE_ROOT",
            "ISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "LJOS_SEAT",
            "VISSUE_AGENT",
            "LJOS_HOLD",
        ]);
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let toml = "seats = [\"ljos-bot\", \"inky-n9o\", \"grokbot-sand-sub\", \"disk-saver\"]\n";
        let org = "* STARTED [#C] one\n:PROPERTIES:\n:ID:         probe-abcd\n:CLAIMED_BY: disk-saver-t4t\n:END:\n\
             * STARTED [#C] two\n:PROPERTIES:\n:ID:         probe-efgh\n:CLAIMED_BY: grokbot-sand-sub\n:END:\n";
        let toml_path = dir.path().join("vissue.toml");
        let org_path = dir.path().join("issues.org");
        std::fs::write(&toml_path, toml).unwrap();
        std::fs::write(&org_path, org).unwrap();
        let before_toml = std::fs::read(&toml_path).unwrap();
        let before_org = std::fs::read(&org_path).unwrap();
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
            std::env::set_var("VISSUE_ROOT", dir.path());
            std::env::set_var("VISSUE_NO_ROUTE", "1");
            std::env::remove_var("ISSUE_ROOT");
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("VISSUE_AGENT");
            std::env::remove_var("LJOS_HOLD");
        }
        let known = crate::tracker_known_seats();
        assert!(known.contains("ljosbot"), "{known:?}");
        assert!(known.contains("inky"), "{known:?}");
        assert!(known.contains("disk-saver"), "{known:?}");
        assert!(!known.contains("ljos-bot"), "{known:?}");
        assert!(!known.contains("inky-n9o"), "{known:?}");
        assert!(!known.contains("disk-saver-t4t"), "{known:?}");
        assert!(!known.contains("grokbot-sand-sub"), "{known:?}");
        assert_eq!(
            crate::resolve_call_seat(Some("ljos-bot")).unwrap().seat,
            "ljosbot"
        );
        assert_eq!(
            crate::resolve_call_seat(Some("inky-n9o")).unwrap().seat,
            "inky"
        );
        let err = crate::resolve_call_seat(Some("grokbot-sand-sub"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no seat of its own"), "{err}");
        assert_eq!(std::fs::read(&toml_path).unwrap(), before_toml);
        assert_eq!(std::fs::read(&org_path).unwrap(), before_org);
    }

    /// Filing goes through `vissue` when the claim graph is absent. A
    /// subagent name writes nothing. `ljos-bot` is filed as `ljosbot`.
    #[test]
    fn filing_a_claim_refuses_a_subagent_name_and_uses_the_seat() {
        let _g = lock();
        let _saved = Restore::new(&[
            "PATH",
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_RUNTIME_DIR",
            "LJOS_SEAT",
            "VISSUE_AGENT",
            "LJOS_HOLD",
        ]);
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("vissue.log");
        let log_s = log.display();
        let body = format!(
            "#!/bin/sh\necho \"$* VISSUE_AGENT=${{VISSUE_AGENT:-}}\" >> '{log_s}'\ncase \"$1\" in\n  create) echo proj-abcd ;;\n  claim) echo claimed ;;\nesac\nexit 0\n"
        );
        let bin = dir.path().join("vissue");
        std::fs::write(&bin, body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        unsafe {
            std::env::set_var("PATH", dir.path());
            std::env::set_var("HOME", dir.path().join("home"));
            std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
            std::env::set_var("XDG_RUNTIME_DIR", dir.path().join("run"));
            std::env::remove_var("VISSUE_AGENT");
            std::env::remove_var("LJOS_HOLD");
            std::env::set_var("LJOS_SEAT", "grokbot-sand-sub");
        }
        let err = file_and_claim("proj", "a title").unwrap_err();
        assert!(err.contains("no seat of its own"), "{err}");
        let wrote = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(wrote.is_empty(), "{wrote}");

        unsafe {
            std::env::set_var("LJOS_SEAT", "ljos-bot");
        }
        let issue = file_and_claim("proj", "a title").unwrap();
        assert_eq!(issue, "proj-abcd");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.contains("create"), "{calls}");
        assert!(calls.contains("claim proj-abcd"), "{calls}");
        for line in calls.lines() {
            let Some(agent) = line.split("VISSUE_AGENT=").nth(1) else {
                continue;
            };
            let agent = agent.trim();
            assert!(
                agent == "ljosbot" || agent.starts_with("ljosbot@"),
                "{calls}"
            );
            assert!(!agent.contains("ljos-bot"), "{calls}");
        }
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
        let _bins = arm(dir.path(), &tracker);
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
        let _bins = arm(dir.path(), &tracker);
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

    /// `ljos_start` in the MCP server books under the seat's key and the
    /// harness hook under the harness session. The first write after the
    /// start stays on the started issue instead of filing a second one,
    /// and the issue is finished once, by the book it moved to.
    #[test]
    fn a_write_after_ljos_start_keeps_the_started_issue() {
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
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("LJOS_HARNESS_SESSION");
            std::env::set_var("LJOS_SESSION_ID", "mcp-start-held");
        }
        let said = start_work(
            None,
            Some(repo.to_str().unwrap()),
            "Add slugify and its test",
        )
        .unwrap();
        let started = said
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("claimed "))
            .expect("the start claimed")
            .to_string();
        assert_eq!(creates(&tracker), 1);

        let session = "harness-after-start";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        assert_eq!(creates(&tracker), 1, "the write filed a second issue");
        let book = load_book(&book_path(session));
        let held: Vec<_> = book
            .repos
            .values()
            .filter_map(|r| r.issue.clone())
            .collect();
        assert_eq!(held, vec![started.clone()]);
        assert!(
            load_book(&book_path(&sitting_key()))
                .repos
                .values()
                .all(|r| r.issue.is_none()),
            "the start's book still holds the issue"
        );

        // The server's shutdown has nothing left to finish.
        mcp_shutdown();
        assert!(
            !git_log(&tracker).contains("finished"),
            "{}",
            git_log(&tracker)
        );
        let (stop_raw, stop_call) = payload("Stop", session, &repo, serde_json::json!({}));
        let _ = observe(&stop_raw, &stop_call);
        let log = git_log(&tracker);
        assert_eq!(
            log.matches(&format!("{started} finished")).count(),
            1,
            "{log}"
        );
    }

    /// A STARTED row another holder keeps in the same checkout is that
    /// holder's ticket: the write files its own.
    #[test]
    fn a_write_does_not_take_another_holders_issue() {
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
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("LJOS_HARNESS_SESSION");
            std::env::set_var("LJOS_SESSION_ID", "mcp-start-other");
        }
        let said = start_work(
            None,
            Some(repo.to_str().unwrap()),
            "Add slugify and its test",
        )
        .unwrap();
        let started = said
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("claimed "))
            .expect("the start claimed")
            .to_string();
        let org_path = tracker.join("Software/probe/issues.org");
        let org = std::fs::read_to_string(&org_path).unwrap();
        let id_line = format!(":ID:         {started}\n");
        std::fs::write(
            &org_path,
            org.replace(
                &id_line,
                &format!("{id_line}:CLAIMED_BY: someone-else@zz9\n"),
            ),
        )
        .unwrap();

        let session = "harness-other-holder";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        assert_eq!(
            creates(&tracker),
            2,
            "the write took another holder's issue"
        );
        let book = load_book(&book_path(session));
        assert!(book
            .repos
            .values()
            .all(|r| r.issue.as_deref() != Some(started.as_str())));
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
        let _bins = arm(dir.path(), &tracker);
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
        assert!(text.contains("git-force-push"), "{text}");
        assert!(text.contains("Commit failed"), "{text}");
        assert_lesson(&text);
    }

    /// ljos-myow: a refused lesson names the HTTP status and the response
    /// body, and a lesson the pack will keep is one its prose gate accepts.
    #[test]
    fn lesson_post_names_status_and_passes_the_prose_gate() {
        let hard =
            "Implement asynchronous reconciliation of cryptographic attestations for the seat";
        let text = lesson_text(
            hard,
            "probe-0001",
            &["abcdef0123456789".into()],
            &["the writer said the sentence was far too long to store".into()],
            &["the guard asked before a force push".into()],
        );
        assert_lesson(&text);
        assert!(text.contains("probe-0001"), "{text}");
        assert!(text.contains("abcdef0"), "{text}");
        assert!(text.contains("Implement"), "{text}");
        assert!(text.contains("seat"), "the title does not vanish: {text}");
        assert!(text.contains("Writer failed"), "{text}");
        assert!(
            !text.contains(hard),
            "a title the gate would refuse is shortened: {text}"
        );
        let plain = lesson_text(
            "Add a readme line",
            "probe-0001",
            &[],
            &["readme test failed".into()],
            &["git push --force origin main".into()],
        );
        assert_lesson(&plain);
        assert!(
            plain.contains("readme") && plain.contains("line"),
            "the words the next prompt recalls stay: {plain}"
        );

        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let refused = serve_pack(|_| {
            (
                400,
                r#"{"error":"readability grade 19.4 exceeds 14"}"#.into(),
            )
        });
        set_var("PACKSET_URL", &refused);
        set_var("PACKSET_TIMEOUT_MS", "2000");
        let session = "myow-refused";
        prompt_and_edit(&repo, session, hard);
        commit_file(&repo, "README.md", "hello from the lesson\n");
        let (post, post_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m land"}, "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&post, &post_call);
        let (stop, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);
        let log = std::fs::read_to_string(dir.path().join("run/ljos/book.log")).unwrap();
        assert!(log.contains("Remember: POST /v1/atoms failed"), "{log}");
        assert!(log.contains("400:"), "{log}");
        assert!(log.contains("readability grade 19.4 exceeds 14"), "{log}");
        let refused_book = book_json(dir.path(), session);
        assert_eq!(
            refused_book["repos"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .unwrap()["lesson"],
            false,
            "{refused_book}"
        );

        let texts = Arc::new(Mutex::new(Vec::<String>::new()));
        let kept = serve_pack({
            let texts = Arc::clone(&texts);
            move |req| keep_atom(req, &texts)
        });
        set_var("PACKSET_URL", &kept);
        let session = "myow-kept";
        prompt_and_edit(&repo, session, hard);
        let sha = commit_file(&repo, "NOTES.md", "hello from the kept lesson\n");
        let (post, post_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m land"}, "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&post, &post_call);
        let (stop, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);
        let stored = texts.lock().unwrap().clone();
        let lesson = stored.last().expect("the lesson was posted");
        assert_lesson(lesson);
        assert!(lesson.contains(&sha[..7]), "{lesson}");
        assert!(!lesson.contains(hard), "{lesson}");
        let kept_book = book_json(dir.path(), session);
        assert_eq!(
            kept_book["repos"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .unwrap()["lesson"],
            true,
            "{kept_book}"
        );
    }

    /// ljos-myow: the lesson is built from the words a later prompt can match.
    /// Stopwords go first, the rule keeps its name, and the next session's
    /// cold encoder still finds the lesson by those words.
    #[test]
    fn slugify_lesson_keeps_the_word_and_the_rule_for_the_next_session() {
        let title = "In this git repo add a function slugify";
        let ask = "git-force-push: a force push rewrites history";
        let asked = lesson_text(title, "probe-0001", &[], &[], &[ask.into()]);
        assert_lesson(&asked);
        assert!(
            asked.contains("The guard asked about git-force-push."),
            "{asked}"
        );
        assert!(
            asked.contains("function") && asked.contains("slugify"),
            "{asked}"
        );
        assert!(
            asked.split_whitespace().any(|word| word == "git"),
            "{asked}"
        );
        for gone in ["this", "repo", "add"] {
            assert!(
                !asked
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|word| word.eq_ignore_ascii_case(gone)),
                "{gone} is filler and stays out: {asked}"
            );
        }
        let faded = lesson_text("In this repo", "probe-0001", &[], &[], &[ask.into()]);
        assert_lesson(&faded);
        assert!(
            faded.contains("repo"),
            "a filler title still leaves a word: {faded}"
        );
        assert!(!faded.starts_with("Work "), "{faded}");
        let crowded = lesson_text(
            "slugify cryptography reconciliation asynchronous attestation implementation verification",
            "probe-0001",
            &[],
            &[],
            &[ask.into()],
        );
        assert_lesson(&crowded);
        assert!(
            crowded.contains("slugify"),
            "a longer neighbour is shortened before the recall word: {crowded}"
        );

        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let marker = dir.path().join("run/ljos/encoder-warming");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();
        let atoms = Arc::new(Mutex::new(Vec::<Value>::new()));
        let pack = serve_pack({
            let atoms = Arc::clone(&atoms);
            move |req| {
                let path = req.lines().next().unwrap_or("");
                if path.contains("POST /v1/atoms") {
                    let body = req
                        .split_once("\r\n\r\n")
                        .map(|(_, body)| body)
                        .unwrap_or("");
                    if let Ok(atom) = serde_json::from_str::<Value>(body) {
                        atoms.lock().unwrap().push(atom);
                    }
                    return (
                        200,
                        r#"{"id":"atom-slugify","kind":"lesson","origin":"agent-derived"}"#.into(),
                    );
                }
                if path.contains("GET /v1/status") {
                    return (
                        200,
                        r#"{"embedder":{"available":true,"answering":null}}"#.into(),
                    );
                }
                if path.contains("GET /v1/atoms") {
                    let list = atoms.lock().unwrap().clone();
                    return (200, serde_json::json!({"atoms": list}).to_string());
                }
                (200, "{}".into())
            }
        });
        set_var("PACKSET_URL", &pack);
        set_var("PACKSET_TIMEOUT_MS", "2000");
        let session = "slugify-1";
        prompt_and_edit(&repo, session, title);
        let (failed, failed_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "git commit -m slug"},
                "tool_response": {"exit_code": 1}
            }),
        );
        let _ = observe(&failed, &failed_call);
        let (ask_raw, ask_call) = payload(
            "PreToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git push --force origin main"}}),
        );
        note_ask(&ask_raw, &ask_call, ask);
        let (stop, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);
        let stored = atoms.lock().unwrap().clone();
        let lesson = stored.last().expect("the lesson was posted");
        let prose = lesson["text"].as_str().unwrap_or("");
        assert!(prose.contains("slugify"), "{prose}");
        assert!(prose.contains("function"), "{prose}");
        assert!(prose.contains("git-force-push"), "{prose}");
        assert!(prose.contains("Commit failed"), "{prose}");
        assert_eq!(lesson["source"]["title"], title);
        assert!(
            lesson["source"]["rule"]
                .as_str()
                .unwrap_or("")
                .contains("git-force-push"),
            "{lesson}"
        );
        assert!(
            lesson["source"]["command"]
                .as_str()
                .unwrap_or("")
                .contains("git commit exited non-zero"),
            "{lesson}"
        );
        assert!(
            lesson["entities"].to_string().contains("horizon:standing"),
            "{lesson}"
        );
        let start = crate::session_start_for(Some("slugify-2"));
        assert!(!start.contains("no memories here yet"), "{start}");
        let (_raw, call) = payload(
            "UserPromptSubmit",
            "slugify-2",
            &repo,
            serde_json::json!({"prompt": "add a function slugify to the helper"}),
        );
        let (note, _) = crate::hook_note(&call, 4);
        assert!(note.contains("Remembered, oldest first"), "{note}");
        assert!(note.contains("slugify"), "{note}");
        assert!(note.contains("git-force-push"), "{note}");
    }

    /// A lesson is already stored and the encoder has never answered. No
    /// warm-up marker stands yet. The first prompt of the next session
    /// ranks that lesson from the atoms and recalls it; the warming note
    /// is what a prompt with nothing to recall hears. The marker is
    /// written after the atoms are read, so the load does not sit in
    /// front of the listing.
    #[test]
    fn the_first_prompt_recalls_a_lesson_while_the_encoder_is_cold() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let marker = dir.path().join("run/ljos/encoder-warming");
        assert!(!marker.exists(), "a fresh home has not started warming");
        let atoms = Arc::new(Mutex::new(vec![
            serde_json::json!({
                "id": "atom-kebab",
                "kind": "lesson",
                "text": "The helper keeps kebab case.",
                "entities": ["horizon:standing"],
                "ts": "2026-10-17T00:00:00Z",
                "origin": "agent-derived",
                "source": {"title": "keep kebab case in the helper"}
            }),
            serde_json::json!({
                "id": "atom-deploy",
                "kind": "lesson",
                "text": "The deploy script restarts the service.",
                "entities": ["horizon:standing"],
                "ts": "2026-10-17T00:00:01Z",
                "origin": "agent-derived"
            }),
        ]));
        let pack = serve_pack({
            let atoms = Arc::clone(&atoms);
            move |req| {
                let path = req.lines().next().unwrap_or("");
                if path.contains("GET /v1/status") {
                    return (
                        200,
                        r#"{"embedder":{"available":true,"answering":null}}"#.into(),
                    );
                }
                if path.contains("GET /v1/atoms") {
                    let list = atoms.lock().unwrap().clone();
                    return (200, serde_json::json!({"atoms": list}).to_string());
                }
                (200, "{}".into())
            }
        });
        set_var("PACKSET_URL", &pack);
        set_var("PACKSET_TIMEOUT_MS", "2000");
        let (_raw, call) = payload(
            "UserPromptSubmit",
            "cold-session-2",
            &repo,
            serde_json::json!({"prompt": "Please update test_kebab.py and run pytest."}),
        );
        let (note, _) = crate::hook_note(&call, 4);
        assert!(
            note.contains("Remembered, oldest first"),
            "the first prompt recalled nothing: {note}"
        );
        assert!(note.contains("kebab"), "{note}");
        assert!(
            !note.contains(crate::ENCODER_WARMING_NOTE),
            "the warming note hid the lesson: {note}"
        );
        assert!(marker.exists(), "warming starts after the atoms are read");
        let (_raw, other) = payload(
            "UserPromptSubmit",
            "cold-unrelated",
            &repo,
            serde_json::json!({"prompt": "Deploy the payments service tonight."}),
        );
        let (other_note, _) = crate::hook_note(&other, 4);
        assert!(
            !other_note.contains("kebab"),
            "an unrelated prompt was handed the lesson: {other_note}"
        );
    }

    /// A session that only met a force-push ask writes a sentence about that
    /// ask. The prompt's words stay, and the lesson is not a keyword list.
    #[test]
    fn an_ask_only_session_lesson_is_a_sentence() {
        let prompt = "git function slugify slug lowercases";
        let ask = "git-force-push: it rewrites shared history, so the agent asks you first";
        let keyword = "git function slugify slug lowercases. The guard asked about git-force-push.";
        let text = lesson_text(prompt, "", &[], &[], &[ask.into()]);
        assert_lesson(&text);
        assert_ne!(text, keyword);
        assert!(text.contains("worked"), "{text}");
        assert!(text.contains("asked"), "{text}");
        assert!(text.contains("slugify"), "{text}");
        assert!(text.contains("lowercases"), "{text}");
        assert!(
            text.contains("The guard asked about git-force-push."),
            "{text}"
        );
        assert!(!text.contains("closed"), "{text}");

        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let atoms = Arc::new(Mutex::new(Vec::<Value>::new()));
        let pack = serve_pack({
            let atoms = Arc::clone(&atoms);
            move |req| {
                let path = req.lines().next().unwrap_or("");
                if path.contains("POST /v1/atoms") {
                    let body = req
                        .split_once("\r\n\r\n")
                        .map(|(_, body)| body)
                        .unwrap_or("");
                    if let Ok(atom) = serde_json::from_str::<Value>(body) {
                        atoms.lock().unwrap().push(atom);
                    }
                    return (
                        200,
                        r#"{"id":"atom-ask","kind":"lesson","origin":"agent-derived"}"#.into(),
                    );
                }
                (200, "{}".into())
            }
        });
        set_var("PACKSET_URL", &pack);
        set_var("PACKSET_TIMEOUT_MS", "2000");
        let session = "ask-only-sentence";
        let (prompt_raw, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": prompt}),
        );
        let _ = observe(&prompt_raw, &prompt_call);
        let (ask_raw, ask_call) = payload(
            "PreToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git push --force origin main"}}),
        );
        note_ask(&ask_raw, &ask_call, ask);
        let (stop, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);
        let stored = atoms.lock().unwrap().clone();
        let lesson = stored.last().expect("the lesson was posted");
        let prose = lesson["text"].as_str().unwrap_or("");
        assert_lesson(prose);
        assert_ne!(prose, keyword);
        assert!(prose.contains("worked"), "{prose}");
        assert!(prose.contains("slugify"), "{prose}");
        assert!(prose.contains("lowercases"), "{prose}");
        assert!(prose.contains("git-force-push"), "{prose}");
        assert!(!prose.contains("closed"), "{prose}");
    }

    /// A commit the guard only asked about never ran. The after hook still
    /// looks like a clean PostToolUse, and HEAD is the commit the sitting
    /// started on. That sha is not a deed. A later commit that moves HEAD is.
    #[test]
    fn an_unrun_commit_is_not_a_deed() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "unrun-commit";
        prompt_and_edit(&repo, session, "Add a readme line");
        let seed = head_sha_of(&repo);
        let command = "git add README.md\ngit commit -m land\ngit push --force origin main";
        let (ask_raw, ask_call) = payload(
            "PreToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": command}}),
        );
        note_ask(
            &ask_raw,
            &ask_call,
            "git-force-push: a force push rewrites history",
        );
        let cwd = repo.to_str().unwrap();
        let asked = serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "toolCall": {"args": {"CommandLine": command}},
            "error": "",
        });
        let asked_text = asked.to_string();
        let asked_call = crate::hook_call(&asked_text);
        assert_eq!(asked_call.event, "PostToolUse");
        assert!(is_git_commit(&asked_call.cue), "{}", asked_call.cue);
        let _ = observe(&asked_text, &asked_call);
        assert_eq!(head_sha_of(&repo), seed);
        let deeds = std::fs::read_to_string(tracker.join("deedar.log")).unwrap_or_default();
        assert!(deeds.is_empty(), "{deeds}");
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(!org.contains("cited"), "{org}");
        assert!(
            !dir.path()
                .join(format!("run/ljos/deed-{seed}.txt"))
                .exists(),
            "the starting commit was cited"
        );
        let sha = commit_file(&repo, "README.md", "hello from the commit that ran\n");
        assert_ne!(sha, seed);
        let (post, post_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m land"}, "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&post, &post_call);
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
        assert!(!org.contains(&format!("deed-file-commit-{seed}")), "{org}");
        let book = book_json(dir.path(), session);
        assert_eq!(deeds_of(&book), vec![sha], "{book}");
    }

    /// Failed commands and a guard ask are a lesson even when the sitting
    /// never claimed. The next session can recall it.
    #[test]
    fn a_sitting_without_a_claim_still_writes_a_lesson() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let marker = dir.path().join("run/ljos/encoder-warming");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();
        let atoms = Arc::new(Mutex::new(Vec::<Value>::new()));
        let pack = serve_pack({
            let atoms = Arc::clone(&atoms);
            move |req| {
                let path = req.lines().next().unwrap_or("");
                if path.contains("POST /v1/atoms") {
                    let body = req
                        .split_once("\r\n\r\n")
                        .map(|(_, body)| body)
                        .unwrap_or("");
                    if let Ok(atom) = serde_json::from_str::<Value>(body) {
                        atoms.lock().unwrap().push(atom);
                    }
                    return (
                        200,
                        r#"{"id":"atom-fail","kind":"lesson","origin":"agent-derived"}"#.into(),
                    );
                }
                if path.contains("GET /v1/status") {
                    return (
                        200,
                        r#"{"embedder":{"available":true,"answering":null}}"#.into(),
                    );
                }
                if path.contains("GET /v1/atoms") {
                    let list = atoms.lock().unwrap().clone();
                    return (200, serde_json::json!({"atoms": list}).to_string());
                }
                (200, "{}".into())
            }
        });
        set_var("PACKSET_URL", &pack);
        set_var("PACKSET_TIMEOUT_MS", "2000");

        let session = "failures-only";
        let (prompt, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "Fix the slugify helper"}),
        );
        let _ = observe(&prompt, &prompt_call);
        for _ in 0..11 {
            let (failed, failed_call) = payload(
                "PostToolUseFailure",
                session,
                &repo,
                serde_json::json!({
                    "tool_name": "Bash",
                    "tool_input": {"command": "cargo test"},
                    "tool_response": {"error": "slugify helper failed"}
                }),
            );
            let _ = observe(&failed, &failed_call);
        }
        let (stop, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);

        let asked = "asks-only";
        let (prompt, prompt_call) = payload(
            "UserPromptSubmit",
            asked,
            &repo,
            serde_json::json!({"prompt": "Check the release script"}),
        );
        let _ = observe(&prompt, &prompt_call);
        let (ask_raw, ask_call) = payload(
            "PreToolUse",
            asked,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git push --force origin main"}}),
        );
        note_ask(
            &ask_raw,
            &ask_call,
            "git-force-push: a force push rewrites history",
        );
        let (stop, stop_call) = payload("Stop", asked, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);

        let vissue = std::fs::read_to_string(tracker.join("vissue.log")).unwrap_or_default();
        assert!(!vissue.contains("create"), "{vissue}");
        let stored = atoms.lock().unwrap().clone();
        assert_eq!(stored.len(), 2, "{stored:?}");
        let failed = stored[0]["text"].as_str().unwrap_or("");
        assert!(failed.contains("slugify"), "{failed}");
        assert!(!failed.contains("closed"), "{failed}");
        assert_lesson(failed);
        let guard = stored[1]["text"].as_str().unwrap_or("");
        assert!(guard.contains("git-force-push"), "{guard}");
        assert!(!guard.contains("closed"), "{guard}");
        assert_lesson(guard);
        let book = book_json(dir.path(), session);
        let row = book["repos"].as_object().unwrap().values().next().unwrap();
        assert!(row["issue"].is_null(), "{book}");
        assert_eq!(row["lesson"], true, "{book}");

        let start = crate::session_start_for(Some("failures-next"));
        assert!(!start.contains("no memories here yet"), "{start}");
        let (_raw, call) = payload(
            "UserPromptSubmit",
            "failures-next",
            &repo,
            serde_json::json!({"prompt": "repair the slugify helper"}),
        );
        let (note, _) = crate::hook_note(&call, 4);
        assert!(note.contains("Remembered, oldest first"), "{note}");
        assert!(note.contains("slugify"), "{note}");
    }

    /// Codex 0.163 names the call as `CommandExecution.id`. The matching
    /// `function_call_output` only spells the code in its text, and that
    /// sentence does not replace a code the item already named.
    #[test]
    fn a_codex_transcript_exit_matches_the_call_id() {
        let id = "exec_command_058e85yevw32";
        let other = "exec_command_q3kpac3wpztr";
        let text = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            codex_event(other, None),
            codex_event(other, Some(127)),
            function_output_line(other, 127),
            codex_event(id, None),
            codex_event(id, Some(1)),
            function_output_line(id, 1),
        );
        assert_eq!(exit_for_call(&text, id), Some(1));
        assert_eq!(exit_for_call(&text, other), Some(127));
        assert_eq!(exit_for_call(&text, "exec_command_missing"), None);
        let item_wins = format!(
            "{}\n{}",
            codex_event(id, Some(1)),
            function_output_line(id, 127),
        );
        assert_eq!(exit_for_call(&item_wins, id), Some(1));
        assert_eq!(exit_for_call(&function_output_line(id, 1), id), Some(1));
        let torn_item = codex_event(id, Some(1));
        let torn_item = &torn_item[..torn_item.len() - 1];
        assert_eq!(exit_for_call(torn_item, id), Some(1));
        let torn_output = function_output_line(id, 255);
        let phrase = "Process exited with code 255";
        let end = torn_output.find(phrase).unwrap() + phrase.len();
        assert_eq!(exit_for_call(&torn_output[..end], id), Some(255));
    }

    /// A Codex 0.163 PostToolUse whose `tool_response` is text books the
    /// failure from the transcript the hook was given.
    #[test]
    fn a_real_codex_post_books_the_exit_failure() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "01a14bc1-59ae-7d43-9a16-bc94c6a0ef56";
        let id = "exec_command_058e85yevw32";
        let other = "exec_command_q3kpac3wpztr";
        let transcript = dir.path().join("rollout.jsonl");
        std::fs::write(
            &transcript,
            format!(
                "{}\n{}\n{}\n{}\n{}\n",
                codex_event(other, None),
                codex_event(other, Some(127)),
                function_output_line(other, 127),
                codex_event(id, Some(1)),
                function_output_line(id, 1),
            ),
        )
        .unwrap();
        let command = "/usr/bin/python3 -c 'import slug; slug.kebab_case(\"Hello World\")'";
        let raw = serde_json::json!({
            "session_id": session,
            "turn_id": "01a14bc1-59d4-7390-a148-c38ad5e58020",
            "transcript_path": transcript,
            "cwd": repo,
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "tool_response": "ModuleNotFoundError: No module named 'slug'\n",
            "tool_use_id": id,
        });
        let text = serde_json::to_string(&raw).unwrap();
        let call = crate::hook_call(&text);
        assert_eq!(call.event, "PostToolUse");
        assert!(call.cue.contains("python3"), "{}", call.cue);
        let _ = observe(&text, &call);
        let book = book_json(dir.path(), session);
        let row = book["repos"].as_object().unwrap().values().next().unwrap();
        let failures = row["failures"].as_array().unwrap();
        assert!(
            failures
                .iter()
                .any(|note| note.as_str().unwrap_or("").contains("python3")),
            "{book}"
        );
        assert!(
            failures
                .iter()
                .any(|note| note.as_str().unwrap_or("").contains("slug")),
            "{book}"
        );
    }

    /// A string `tool_response` plus a transcript exit of 127 is a failure,
    /// and the lesson keeps it.
    #[test]
    fn a_codex_string_response_with_exit_127_is_a_lesson_failure() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let atoms = arm_lessons(dir.path());
        let session = "codex-exit-127";
        let id = "exec_command_n38zthdamfmc";
        let transcript = dir.path().join("rollout.jsonl");
        std::fs::write(
            &transcript,
            format!(
                "{}\n{}\n{}\n",
                codex_event("exec_command_other", Some(0)),
                codex_event(id, None),
                codex_event(id, Some(127)),
            ),
        )
        .unwrap();
        let (prompt, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "Fix the slugify helper"}),
        );
        let _ = observe(&prompt, &prompt_call);
        let (failed, failed_call) = codex_post(
            session,
            &repo,
            id,
            "python slugify",
            "/bin/bash: line 1: python: command not found\r\n",
            &transcript,
        );
        let _ = observe(&failed, &failed_call);
        let (stop, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);

        let book = book_json(dir.path(), session);
        let row = book["repos"].as_object().unwrap().values().next().unwrap();
        let failures = row["failures"].as_array().unwrap();
        assert!(
            failures
                .iter()
                .any(|note| note.as_str().unwrap_or("").contains("python")),
            "{book}"
        );
        let stored = atoms.lock().unwrap().clone();
        let lesson = stored.last().expect("the lesson was posted");
        let prose = lesson["text"].as_str().unwrap_or("");
        assert!(prose.contains("slugify"), "{prose}");
        assert!(prose.contains("failed"), "{prose}");
        assert!(
            lesson["source"]["command"]
                .as_str()
                .unwrap_or("")
                .contains("python"),
            "{lesson}"
        );
        assert_lesson(prose);
    }

    /// Commands that only fail, on a sitting that never claimed, still leave
    /// a lesson. Codex reports those failures as PostToolUse with a string body.
    #[test]
    fn an_unclaimed_codex_session_with_failures_writes_a_lesson() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let atoms = arm_lessons(dir.path());
        let session = "codex-failures-only";
        let transcript = dir.path().join("rollout.jsonl");
        let first = "exec_command_n38zthdamfmc";
        let second = "exec_command_modmissing";
        std::fs::write(
            &transcript,
            format!(
                "{}\n{}\n",
                codex_event(first, Some(127)),
                codex_event(second, Some(1)),
            ),
        )
        .unwrap();
        let (prompt, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "Fix the slugify helper"}),
        );
        let _ = observe(&prompt, &prompt_call);
        let (failed, failed_call) = codex_post(
            session,
            &repo,
            first,
            "python slugify",
            "/bin/bash: line 1: python: command not found\r\n",
            &transcript,
        );
        let _ = observe(&failed, &failed_call);
        let (again, again_call) = codex_post(
            session,
            &repo,
            second,
            "python -m slugify",
            "ModuleNotFoundError: No module named 'slugify'\r\n",
            &transcript,
        );
        let _ = observe(&again, &again_call);
        let (stop, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);

        let vissue = std::fs::read_to_string(tracker.join("vissue.log")).unwrap_or_default();
        assert!(!vissue.contains("create"), "{vissue}");
        let stored = atoms.lock().unwrap().clone();
        assert_eq!(stored.len(), 1, "{stored:?}");
        let prose = stored[0]["text"].as_str().unwrap_or("");
        assert!(prose.contains("slugify"), "{prose}");
        assert!(prose.contains("failed"), "{prose}");
        assert!(!prose.contains("closed"), "{prose}");
        assert_lesson(prose);
        let book = book_json(dir.path(), session);
        let row = book["repos"].as_object().unwrap().values().next().unwrap();
        assert!(row["issue"].is_null(), "{book}");
        assert_eq!(row["lesson"], true, "{book}");
        assert!(row["failures"].as_array().unwrap().len() >= 2, "{book}");

        let start = crate::session_start_for(Some("codex-failures-next"));
        assert!(!start.contains("no memories here yet"), "{start}");
        let (_raw, call) = payload(
            "UserPromptSubmit",
            "codex-failures-next",
            &repo,
            serde_json::json!({"prompt": "repair the slugify helper"}),
        );
        let (note, _) = crate::hook_note(&call, 4);
        assert!(note.contains("Remembered, oldest first"), "{note}");
        assert!(note.contains("slugify"), "{note}");
    }

    /// A git commit whose payload has no exit field is not a success. The
    /// sitting's current HEAD is not a deed, including when this post is the
    /// first time the sitting saw the repository. A later commit whose
    /// transcript says 0, and whose HEAD moved, is cited.
    #[test]
    fn a_failed_codex_commit_with_no_exit_field_cites_no_deed() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "codex-commit";
        let seed = head_sha_of(&repo);
        let transcript = dir.path().join("rollout.jsonl");
        // Another call exited 0. This commit's id is not in the file, and
        // the payload has no exit field, so the code stays unknown.
        std::fs::write(
            &transcript,
            format!("{}\n", codex_event("exec_command_other", Some(0))),
        )
        .unwrap();
        let (failed, failed_call) = codex_post(
            session,
            &repo,
            "exec_command_n38zthdamfmc",
            "git commit -m nope",
            "error: nothing to commit\r\n",
            &transcript,
        );
        assert_eq!(failed_call.event, "PostToolUse");
        assert!(is_git_commit(&failed_call.cue), "{}", failed_call.cue);
        assert!(explicit_exit(&serde_json::from_str(&failed).unwrap()).is_none());
        let _ = observe(&failed, &failed_call);
        assert_eq!(head_sha_of(&repo), seed);
        let deeds = std::fs::read_to_string(tracker.join("deedar.log")).unwrap_or_default();
        assert!(deeds.is_empty(), "{deeds}");
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(!org.contains("cited"), "{org}");
        assert!(
            !dir.path()
                .join(format!("run/ljos/deed-{seed}.txt"))
                .exists(),
            "the sitting's HEAD was cited"
        );
        let book = book_json(dir.path(), session);
        assert!(deeds_of(&book).is_empty(), "{book}");

        let sha = commit_file(&repo, "README.md", "hello from codex\n");
        assert_ne!(sha, seed);
        std::fs::write(
            &transcript,
            format!("{}\n", codex_event("exec_command_ok", Some(0))),
        )
        .unwrap();
        let (post, post_call) = codex_post(
            session,
            &repo,
            "exec_command_ok",
            "git commit -m land",
            "1 file changed\r\n",
            &transcript,
        );
        let _ = observe(&post, &post_call);
        let book = book_json(dir.path(), session);
        assert_eq!(deeds_of(&book), vec![sha], "{book}");
    }

    /// A transcript that does not answer inside the hook budget is not exit 0.
    #[test]
    fn a_stuck_codex_transcript_is_not_a_clean_commit() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "codex-stuck";
        let seed = head_sha_of(&repo);
        let (prompt, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "Commit the readme"}),
        );
        let _ = observe(&prompt, &prompt_call);
        let fifo = dir.path().join("stuck.jsonl");
        let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: mkfifo takes a NUL-terminated path and a mode. The path
        // lives until the call returns, and the mode is a plain permission.
        let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        assert_eq!(rc, 0, "mkfifo");
        let (failed, failed_call) = codex_post(
            session,
            &repo,
            "exec_command_n38zthdamfmc",
            "git commit -m nope",
            "error: nothing to commit\r\n",
            &fifo,
        );
        let started = std::time::Instant::now();
        let _ = observe(&failed, &failed_call);
        assert!(
            started.elapsed() < TRANSCRIPT_DEADLINE + Duration::from_secs(2),
            "the transcript read held the hook for {:?}",
            started.elapsed()
        );
        assert!(deeds_of(&book_json(dir.path(), session)).is_empty());
        assert_eq!(head_sha_of(&repo), seed);
        let deeds = std::fs::read_to_string(tracker.join("deedar.log")).unwrap_or_default();
        assert!(deeds.is_empty(), "{deeds}");
    }

    /// A timed-out agy sitting never gets Stop. Its prompt arrives on the
    /// payload after the first model call, and the lesson keeps those words
    /// so the next prompt can recall them.
    #[test]
    fn a_timed_out_agy_prompt_is_recalled() {
        let _g = lock();
        let asked = serde_json::json!({
            "conversationId": "agy-prompt-event",
            "prompt": "Fix the slugify helper",
        });
        let asked_call = crate::hook_call_as(&asked.to_string(), Some("beforeSubmitPrompt"));
        assert_eq!(asked_call.event, "UserPromptSubmit");
        assert!(asked_call.cue.contains("slugify"), "{}", asked_call.cue);

        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let marker = dir.path().join("run/ljos/encoder-warming");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();
        let atoms = Arc::new(Mutex::new(Vec::<Value>::new()));
        let pack = serve_pack({
            let atoms = Arc::clone(&atoms);
            move |req| {
                let path = req.lines().next().unwrap_or("");
                if path.contains("POST /v1/atoms") {
                    let body = req
                        .split_once("\r\n\r\n")
                        .map(|(_, body)| body)
                        .unwrap_or("");
                    if let Ok(atom) = serde_json::from_str::<Value>(body) {
                        atoms.lock().unwrap().push(atom);
                    }
                    return (
                        200,
                        r#"{"id":"atom-agy","kind":"lesson","origin":"agent-derived"}"#.into(),
                    );
                }
                if path.contains("GET /v1/status") {
                    return (
                        200,
                        r#"{"embedder":{"available":true,"answering":null}}"#.into(),
                    );
                }
                if path.contains("GET /v1/atoms") {
                    let list = atoms.lock().unwrap().clone();
                    return (200, serde_json::json!({"atoms": list}).to_string());
                }
                (200, "{}".into())
            }
        });
        set_var("PACKSET_URL", &pack);
        set_var("PACKSET_TIMEOUT_MS", "2000");

        let session = "agy-timeout";
        let cwd = repo.to_str().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        std::fs::write(
            &transcript,
            "{\"type\":\"PLANNER_RESPONSE\",\"text\":\"working\"}\n",
        )
        .unwrap();
        let early = serde_json::json!({
            "invocationNum": 0,
            "conversationId": session,
            "cwd": cwd,
            "workspacePaths": [cwd],
            "transcriptPath": transcript,
        });
        let early_text = early.to_string();
        let early_call = crate::hook_call(&early_text);
        assert_eq!(early_call.event, "UserPromptSubmit");
        assert!(early_call.cue.is_empty(), "{}", early_call.cue);
        let _ = observe(&early_text, &early_call);

        std::fs::write(
            &transcript,
            "{\"type\":\"USER_INPUT\",\"userInput\":{\"items\":[{\"text\":\"Fix the slugify helper\"}]}}\n",
        )
        .unwrap();
        let edit = serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "workspacePaths": [cwd],
            "transcriptPath": transcript,
            "toolCall": {"name": "write_to_file", "args": {"TargetFile": "README.md", "CodeContent": "x"}},
        });
        let edit_text = edit.to_string();
        let edit_call = crate::hook_call(&edit_text);
        assert_eq!(edit_call.event, "PreToolUse");
        let _ = observe(&edit_text, &edit_call);
        set_owner(dir.path(), session, dead_pid());

        let (next, next_call) = payload(
            "UserPromptSubmit",
            "agy-next",
            &repo,
            serde_json::json!({"prompt": "repair the slugify helper"}),
        );
        let _ = observe(&next, &next_call);

        let book = book_json(dir.path(), session);
        let row = book["repos"].as_object().unwrap().values().next().unwrap();
        assert!(
            row["title"].as_str().unwrap_or("").contains("slugify"),
            "{book}"
        );
        assert_eq!(row["lesson"], true, "{book}");
        assert_eq!(row["finished"], true, "{book}");
        let stored = atoms.lock().unwrap().clone();
        let lesson = stored
            .last()
            .and_then(|atom| atom["text"].as_str())
            .unwrap_or("");
        assert!(lesson.contains("slugify"), "{lesson}");
        assert_lesson(lesson);

        let start = crate::session_start_for(Some("agy-next"));
        assert!(!start.contains("no memories here yet"), "{start}");
        let (note, _) = crate::hook_note(&next_call, 4);
        assert!(note.contains("Remembered, oldest first"), "{note}");
        assert!(note.contains("slugify"), "{note}");
    }

    /// ljos-6x14: agy sends no event name. Onboarding tells the after-command
    /// hook `--event PostToolUse`, and a payload that carries `error` is
    /// that same return when the flag is absent. Either way the commit is cited.
    #[test]
    fn agy_commit_without_an_event_name_is_cited() {
        let spec = crate::named_hook_spec("ljos");
        let command = spec["PostToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(command.contains("--event PostToolUse"), "{command}");
        let entering = r#"{"conversationId":"agy","toolCall":{"args":{"CommandLine":"git add README.md && git commit -m land"}}}"#;
        let before = crate::hook_call(entering);
        assert_eq!(before.event, "PreToolUse");
        assert_eq!(before.cue, "git add README.md && git commit -m land");
        let returning = r#"{"conversationId":"agy","toolCall":{"args":{"CommandLine":"git add README.md && git commit -m land"}},"error":""}"#;
        let after = crate::hook_call(returning);
        assert_eq!(after.event, "PostToolUse");
        assert_eq!(after.cue, before.cue);

        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "agy-deed";
        let cwd = repo.to_str().unwrap();
        let (prompt, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "Add a readme line"}),
        );
        let _ = observe(&prompt, &prompt_call);
        let pre = serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "toolCall": {"args": {"CommandLine": "git add README.md && git commit -m land"}},
        });
        let pre_text = pre.to_string();
        let pre_call = crate::hook_call(&pre_text);
        assert_eq!(pre_call.event, "PreToolUse");
        let _ = observe(&pre_text, &pre_call);
        assert!(
            std::fs::read_to_string(tracker.join("deedar.log"))
                .unwrap_or_default()
                .is_empty(),
            "the way in does not cite a commit that has not run"
        );
        let sha = commit_file(&repo, "README.md", "hello from agy\n");
        let post = serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "toolCall": {"args": {"CommandLine": "git add README.md && git commit -m land"}},
            "error": "",
        });
        let post_text = post.to_string();
        let post_call = crate::hook_call(&post_text);
        assert_eq!(post_call.event, "PostToolUse");
        let _ = observe(&post_text, &post_call);
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
        let book = book_json(dir.path(), session);
        let deeds = deeds_of(&book);
        assert_eq!(deeds, vec![sha], "{book}");
    }

    /// ljos-qpiw: git's own stderr on exit 0 is still a commit to cite.
    /// A non-zero exit is not. A missing exit whose HEAD moved is.
    #[test]
    fn successful_commit_with_stderr_text_is_cited() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, _tracker, repo) = scratch();
        let _bins = arm(dir.path(), &_tracker);
        let session = "qpiw";
        prompt_and_edit(&repo, session, "Add a readme line");
        let sha = commit_file(&repo, "README.md", "hello from cloudflare\n");
        let (post, post_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({
                "tool_name": "bash",
                "tool_input": {"command": "git add README.md && git commit -m 'land a line'"},
                "tool_response": {
                    "output": "1 file changed, 1 insertion(+)",
                    "error": "1 file changed, 1 insertion(+)",
                    "exit_code": 0
                }
            }),
        );
        let _ = observe(&post, &post_call);
        let book = book_json(dir.path(), session);
        assert_eq!(deeds_of(&book), vec![sha.clone()], "{book}");
        git(
            &repo,
            &["commit", "--allow-empty", "-m", "a failed attempt"],
        );
        let missed = head_sha_of(&repo);
        let (failed, failed_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({
                "tool_name": "bash",
                "tool_input": {"command": "git commit --allow-empty -m 'a failed attempt'"},
                "tool_response": {"error": "nothing to commit", "exit_code": 1}
            }),
        );
        let _ = observe(&failed, &failed_call);
        let book = book_json(dir.path(), session);
        let deeds = deeds_of(&book);
        assert!(deeds.contains(&sha), "{book}");
        assert!(!deeds.contains(&missed), "{book}");
        let quiet = commit_file(&repo, "quiet.txt", "no exit field\n");
        let (stderr_only, stderr_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({
                "tool_name": "bash",
                "tool_input": {"command": "git commit -m quiet"},
                "tool_response": {"error": "1 file changed, 1 insertion(+)"}
            }),
        );
        let _ = observe(&stderr_only, &stderr_call);
        let book = book_json(dir.path(), session);
        assert!(
            deeds_of(&book).contains(&quiet),
            "HEAD moved and no exit code is a landed commit: {book}"
        );
    }

    /// ljos-tha1: a session killed before Stop leaves its claim open.
    /// The next session finishes a book whose runner pid is dead.
    #[test]
    fn dead_runner_claims_finish_at_the_next_session() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let gone = dead_pid();
        for session in ["dead-a", "dead-b"] {
            prompt_and_edit(&repo, session, "Add a readme line");
            set_owner(dir.path(), session, gone);
        }
        prompt_and_edit(&repo, "live-one", "still running");
        let (start, start_call) = payload(
            "UserPromptSubmit",
            "next-sitting",
            &repo,
            serde_json::json!({"prompt": "a later sitting"}),
        );
        let _ = observe(&start, &start_call);
        for session in ["dead-a", "dead-b"] {
            let book = book_json(dir.path(), session);
            let row = book["repos"].as_object().unwrap().values().next().unwrap();
            assert_eq!(row["finished"], true, "{session}: {book}");
        }
        let live = book_json(dir.path(), "live-one");
        let row = live["repos"].as_object().unwrap().values().next().unwrap();
        assert_eq!(row["finished"], false, "{live}");
        assert_eq!(
            git_log(&tracker).matches("finished").count(),
            2,
            "{}",
            git_log(&tracker)
        );
    }

    /// ljos-gfiz: a cited commit that never landed is replaced by the
    /// commit that did, and the lesson names that one.
    #[test]
    fn deed_follows_the_commit_that_landed() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let texts = Arc::new(Mutex::new(Vec::<String>::new()));
        let kept = serve_pack({
            let texts = Arc::clone(&texts);
            move |req| keep_atom(req, &texts)
        });
        set_var("PACKSET_URL", &kept);
        set_var("PACKSET_TIMEOUT_MS", "2000");
        let session = "gfiz";
        prompt_and_edit(&repo, session, "Add a readme line");
        let abandoned = commit_file(&repo, "README.md", "hello from the first sha\n");
        let (post, post_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m 'land a line'"}, "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&post, &post_call);
        let (stop, stop_call) = payload("Stop", session, &repo, Value::Null);
        let _ = observe(&stop, &stop_call);
        git(&repo, &["commit", "--amend", "-m", "landed"]);
        let landed = head_sha_of(&repo);
        assert_ne!(abandoned, landed);
        let (next, next_call) = payload(
            "UserPromptSubmit",
            "gfiz-next",
            &repo,
            serde_json::json!({"prompt": "next sitting"}),
        );
        let _ = observe(&next, &next_call);
        let book = book_json(dir.path(), session);
        assert_eq!(deeds_of(&book), vec![landed.clone()], "{book}");
        let stored = texts.lock().unwrap().clone();
        let lesson = stored.last().expect("a lesson was posted");
        assert!(lesson.contains(&landed[..7]), "{lesson}");
        assert!(!lesson.contains(&abandoned[..7]), "{lesson}");
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(org.contains(&format!("deed-file-commit-{landed}")), "{org}");
        assert!(
            !org.contains(&format!("deed-file-commit-{abandoned}")),
            "the replaced sha is still cited:\n{org}"
        );
    }

    /// ljos-6x14: a PostToolUse whose `toolCall` is null names no command.
    /// The before-hook's command is the commit, and the deed is cited.
    #[test]
    fn agy_post_without_a_tool_call_cites_the_remembered_commit() {
        let bare = r#"{"conversationId":"agy-shape","stepIdx":2,"error":""}"#;
        assert_eq!(crate::hook_call(bare).event, "PostToolUse");
        let null_call = r#"{"conversationId":"agy-shape","stepIdx":3,"toolCall":null}"#;
        assert_eq!(crate::hook_call(null_call).event, "PostToolUse");

        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "agy-null-tool";
        let cwd = repo.to_str().unwrap();
        let (prompt, prompt_call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "Add a readme line"}),
        );
        let _ = observe(&prompt, &prompt_call);
        let pre = serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "toolCall": {"args": {"CommandLine": "git add README.md && git commit -m land"}},
        });
        let pre_text = pre.to_string();
        let pre_call = crate::hook_call(&pre_text);
        assert_eq!(pre_call.event, "PreToolUse");
        assert!(pre_call.cue.contains("git commit"), "{}", pre_call.cue);
        let _ = observe(&pre_text, &pre_call);
        let sha = commit_file(&repo, "README.md", "hello from a null toolCall\n");
        let post = serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "stepIdx": 4,
            "toolCall": null,
        });
        let post_text = post.to_string();
        let post_call = crate::hook_call(&post_text);
        assert_eq!(post_call.event, "PostToolUse");
        assert!(
            post_call.cue.contains("git commit"),
            "the after payload lost the command: {}",
            post_call.cue
        );
        let _ = observe(&post_text, &post_call);
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
        let book = book_json(dir.path(), session);
        assert_eq!(deeds_of(&book), vec![sha], "{book}");
    }

    /// A prompt answers inside the sweep budget however many past sittings
    /// the seat keeps, and the sweeps still reach every book: a sha found in
    /// HEAD is remembered, so a later pass does not ask git again.
    #[test]
    fn a_prompt_does_not_wait_on_the_seats_history() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let head = head_sha_of(&repo);
        let repo_key = repo.display().to_string();
        let sittings = 300;
        for n in 0..sittings {
            let mut book = Book::default();
            book.repos.insert(
                repo_key.clone(),
                RepoBook {
                    issue: Some("probe-aaaa".into()),
                    deeds: vec![head.clone()],
                    finished: true,
                    lesson: true,
                    ..RepoBook::default()
                },
            );
            let path = book_path(&format!("old-{n}"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            store_book(&path, &book);
        }
        let landed = || {
            (0..sittings)
                .filter(|n| {
                    let book = load_book(&book_path(&format!("old-{n}")));
                    book.repos.values().all(|row| !row.landed.is_empty())
                })
                .count()
        };
        assert_eq!(landed(), 0);
        let mut passes = 0;
        while landed() < sittings && passes < 60 {
            let (raw, call) = payload(
                "UserPromptSubmit",
                "now",
                &repo,
                serde_json::json!({"prompt": "summarize the failing build"}),
            );
            let started = std::time::Instant::now();
            let _ = observe(&raw, &call);
            let took = started.elapsed();
            assert!(
                took < std::time::Duration::from_millis(2500),
                "pass {passes} took {took:?}"
            );
            passes += 1;
        }
        assert_eq!(landed(), sittings, "the sweep reached every book");
    }

    fn saved_env() -> Restore {
        Restore::new(&[
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
            "PACKSET_TIMEOUT_MS",
        ])
    }

    fn set_var(key: &str, value: &str) {
        unsafe { std::env::set_var(key, value) };
    }

    /// A Codex 0.163 transcript line, trimmed to the fields the exit lookup
    /// reads. `Some` is the completed `CommandExecution` item (`id`, not
    /// `call_id`). `None` is the `function_call`, which names the id and
    /// carries no code.
    fn codex_event(call_id: &str, exit_code: Option<i64>) -> String {
        match exit_code {
            Some(code) => serde_json::json!({
                "timestamp": "2026-10-17T21:25:30.203Z",
                "type": "event_msg",
                "payload": {
                    "type": "item_completed",
                    "item": {
                        "type": "CommandExecution",
                        "id": call_id,
                        "command": ["/bin/bash", "-lc", "true"],
                        "status": if code == 0 { "completed" } else { "failed" },
                        "exit_code": code,
                    }
                }
            })
            .to_string(),
            None => serde_json::json!({
                "timestamp": "2026-10-17T21:25:30.129Z",
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "name": "exec_command",
                    "call_id": call_id,
                    "arguments": "{}",
                }
            })
            .to_string(),
        }
    }

    /// The `function_call_output` whose `call_id` matches. The code is only
    /// the harness sentence, as Codex 0.163 writes it.
    fn function_output_line(call_id: &str, code: i64) -> String {
        serde_json::json!({
            "timestamp": "2026-10-17T21:25:30.218Z",
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": call_id,
                "output": format!(
                    "Chunk ID: 13ab48\nWall time: 0.0000 seconds\nProcess exited with code {code}\nOriginal token count: 46\nOutput:\nModuleNotFoundError: No module named 'slug'\n"
                ),
            }
        })
        .to_string()
    }

    fn codex_post(
        session: &str,
        repo: &Path,
        tool_use_id: &str,
        command: &str,
        response: &str,
        transcript: &Path,
    ) -> (String, crate::HookCall) {
        payload(
            "PostToolUse",
            session,
            repo,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_use_id": tool_use_id,
                "tool_input": {"command": command},
                "tool_response": response,
                "transcript_path": transcript,
            }),
        )
    }

    fn arm_lessons(dir: &Path) -> Arc<Mutex<Vec<Value>>> {
        let marker = dir.join("run/ljos/encoder-warming");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();
        let atoms = Arc::new(Mutex::new(Vec::<Value>::new()));
        let pack = serve_lessons(Arc::clone(&atoms));
        set_var("PACKSET_URL", &pack);
        set_var("PACKSET_TIMEOUT_MS", "2000");
        atoms
    }

    fn serve_lessons(atoms: Arc<Mutex<Vec<Value>>>) -> String {
        serve_pack(move |req| {
            let path = req.lines().next().unwrap_or("");
            if path.contains("POST /v1/atoms") {
                let body = req
                    .split_once("\r\n\r\n")
                    .map(|(_, body)| body)
                    .unwrap_or("");
                if let Ok(atom) = serde_json::from_str::<Value>(body) {
                    atoms.lock().unwrap().push(atom);
                }
                return (
                    200,
                    r#"{"id":"atom-codex","kind":"lesson","origin":"agent-derived"}"#.into(),
                );
            }
            if path.contains("GET /v1/status") {
                return (
                    200,
                    r#"{"embedder":{"available":true,"answering":null}}"#.into(),
                );
            }
            if path.contains("GET /v1/atoms") {
                let list = atoms.lock().unwrap().clone();
                return (200, serde_json::json!({"atoms": list}).to_string());
            }
            (200, "{}".into())
        })
    }

    fn head_sha_of(repo: &Path) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn commit_file(repo: &Path, name: &str, body: &str) -> String {
        std::fs::write(repo.join(name), body).unwrap();
        git(repo, &["add", name]);
        git(repo, &["commit", "-q", "-m", name]);
        head_sha_of(repo)
    }

    fn prompt_and_edit(repo: &Path, session: &str, prompt: &str) {
        let (raw, call) = payload(
            "UserPromptSubmit",
            session,
            repo,
            serde_json::json!({"prompt": prompt}),
        );
        let _ = observe(&raw, &call);
        let (raw, call) = payload(
            "PreToolUse",
            session,
            repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        );
        let _ = observe(&raw, &call);
    }

    fn book_json(dir: &Path, session: &str) -> Value {
        let path = dir.join(format!(
            "run/ljos/book-{}.json",
            crate::session_tag(session)
        ));
        serde_json::from_str(
            &std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("no book for {session} at {}", path.display())),
        )
        .unwrap()
    }

    fn deeds_of(book: &Value) -> Vec<String> {
        book["repos"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|row| {
                row["deeds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_string))
            })
            .collect()
    }

    /// Make `pid` the runner that owns `session`'s book, under the start
    /// time it runs with now. A pid that is gone has none.
    fn set_owner(dir: &Path, session: &str, pid: u32) {
        let start = crate::proc_tree::of(pid).map_or(0, |p| p.start);
        set_owner_at(dir, session, pid, start);
    }

    fn set_owner_at(dir: &Path, session: &str, pid: u32, start: u64) {
        let path = dir.join(format!(
            "run/ljos/book-{}.json",
            crate::session_tag(session)
        ));
        let mut book: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        book["owner_pid"] = serde_json::json!(pid);
        book["owner_start"] = serde_json::json!(start);
        std::fs::write(&path, serde_json::to_string(&book).unwrap()).unwrap();
    }

    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        let _ = child.wait();
        assert!(!crate::pid_alive(pid), "pid {pid} is still alive");
        pid
    }

    fn assert_lesson(text: &str) {
        let mut atom = serde_json::json!({
            "kind": "lesson",
            "text": text,
            "workspace": "seat",
            "origin": "agent-derived",
        });
        let map = atom.as_object_mut().expect("atom");
        if let Err(e) = packset_core::record::validate(map) {
            panic!("{text}: {e}");
        }
    }

    fn serve_pack(on: impl Fn(&str) -> (u16, String) + Send + 'static) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    break;
                };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                while let Ok(n) = stream.read(&mut tmp) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let mut need = 0usize;
                    for line in buf[..at].split(|b| *b == b'\n') {
                        let line = std::str::from_utf8(line).unwrap_or("").trim();
                        let Some((name, value)) = line.split_once(':') else {
                            continue;
                        };
                        if name.eq_ignore_ascii_case("content-length") {
                            need = value.trim().parse().unwrap_or(0);
                        }
                    }
                    if buf.len().saturating_sub(at + 4) >= need {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf).into_owned();
                let (code, body) = on(&req);
                let status = if code == 200 {
                    "200 OK"
                } else {
                    "400 Bad Request"
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    fn keep_atom(req: &str, texts: &Mutex<Vec<String>>) -> (u16, String) {
        let path = req.lines().next().unwrap_or("");
        let body = req
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or("");
        if path.contains("POST /v1/atoms") {
            let atom: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            if let Some(text) = atom["text"].as_str() {
                texts.lock().unwrap().push(text.to_string());
            }
            return (
                200,
                r#"{"id":"atom-1","kind":"lesson","origin":"agent-derived"}"#.into(),
            );
        }
        (200, "{}".into())
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

    /// File an issue in the stub tracker the way a person would, outside
    /// any sitting, and give its id.
    fn file_by_hand(title: &str) -> String {
        let out = std::process::Command::new("vissue")
            .args(["create", "-p", "probe", title, "-q"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn land_commit(repo: &Path, file: &str, message: &str) -> String {
        std::fs::write(repo.join(file), "x\n").unwrap();
        git(repo, &["add", file]);
        git(repo, &["commit", "-q", "-m", message]);
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn issue_state(tracker: &Path, issue: &str) -> String {
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        let at = org.find(&format!(":ID:         {issue}\n")).unwrap();
        let head = org[..at].rfind("* ").unwrap();
        org[head + 2..]
            .split_whitespace()
            .next()
            .unwrap()
            .to_string()
    }

    fn hook_row(session: &str, repo: &Path) -> RepoBook {
        load_book(&book_path(session))
            .repos
            .get(repo.to_str().unwrap())
            .cloned()
            .unwrap_or_default()
    }

    /// The willma w1 shape: session 2's first edit files a placeholder,
    /// then the commit names the reopened session 1 issue. The row moves to
    /// the named issue with the sha in `deeds`, and the placeholder is
    /// cancelled.
    #[test]
    fn a_named_commit_takes_over_the_empty_issue_the_edit_filed() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let named = file_by_hand("Add slugify function and tests");
        let session = "harness-named-takeover";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        let auto = hook_row(session, &repo)
            .issue
            .expect("the edit filed a claim");
        assert_ne!(auto, named);
        let message = format!("{named}: add kebab_case");
        let sha = land_commit(&repo, "slug.py", &message);
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash",
                "tool_input": {"command": format!("git commit -m \"{message}\"")},
                "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&raw, &call);
        let row = hook_row(session, &repo);
        assert_eq!(row.issue.as_deref(), Some(named.as_str()));
        assert_eq!(row.deeds, vec![sha.clone()]);
        assert!(row.elsewhere.is_empty(), "{:?}", row.elsewhere);
        assert!(row.also.is_empty(), "{:?}", row.also);
        assert!(row.named && !row.filed);
        assert_eq!(issue_state(&tracker, &auto), "CANCELLED");
        let log =
            std::fs::read_to_string(crate::runtime_dir().join("book.log")).unwrap_or_default();
        assert!(log.contains(&format!("claim: cancelled {auto}")), "{log}");
        assert_eq!(creates(&tracker), 2, "the commit filed no third issue");
    }

    /// Taking over a placeholder keeps the sitting's notes. The commit
    /// names the real ticket; it does not wipe failures, asks, the title
    /// or commits the sweep already found.
    #[test]
    fn a_named_commit_keeps_the_placeholder_sittings_notes() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let named = file_by_hand("Add slugify function and tests");
        let session = "harness-named-keeps-notes";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        let auto = hook_row(session, &repo)
            .issue
            .expect("the edit filed a claim");
        let path = book_path(session);
        let mut book = load_book(&path);
        let key = book
            .repos
            .iter()
            .find(|(_, row)| row.issue.as_deref() == Some(auto.as_str()))
            .map(|(key, _)| key.clone())
            .expect("placeholder row");
        let landed = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let landed_at = head_sha(&key).expect("head");
        {
            let row = book.repos.get_mut(&key).expect("row");
            row.failures = vec!["cargo test exited 101".into()];
            row.asks = vec!["guard asked about the test".into()];
            row.title = Some("slugify the names".into());
            row.landed = vec![landed.clone()];
            row.landed_head = Some(landed_at);
        }
        store_book(&path, &book);
        let message = format!("{named}: add kebab_case");
        let sha = land_commit(&repo, "slug.py", &message);
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": format!("git commit -m \"{message}\"")},
                "tool_response": {"exit_code": 0}
            }),
        );
        let _ = observe(&raw, &call);
        let row = hook_row(session, &repo);
        assert_eq!(row.issue.as_deref(), Some(named.as_str()));
        assert!(row.deeds.contains(&sha), "{:?}", row.deeds);
        assert_eq!(
            row.failures,
            vec!["cargo test exited 101".to_string()],
            "placeholder takeover dropped the failure notes: {:?}",
            row.failures
        );
        assert_eq!(row.asks, vec!["guard asked about the test".to_string()]);
        assert_eq!(row.title.as_deref(), Some("slugify the names"));
        assert_eq!(row.landed, vec![landed]);
    }

    /// A placeholder that already has a deed is real work: the named
    /// commit is booked beside it, and it is not cancelled.
    #[test]
    fn a_named_commit_keeps_a_filed_issue_that_has_a_deed() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let named = file_by_hand("Add slugify function and tests");
        let session = "harness-named-keeps";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        let auto = hook_row(session, &repo).issue.unwrap();
        let first = land_commit(&repo, "one.txt", "add one");
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash",
                "tool_input": {"command": "git commit -m \"add one\""},
                "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&raw, &call);
        assert_eq!(hook_row(session, &repo).deeds, vec![first]);
        let message = format!("{named}: add kebab_case");
        let sha = land_commit(&repo, "two.txt", &message);
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash",
                "tool_input": {"command": format!("git commit -m \"{message}\"")},
                "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&raw, &call);
        let row = hook_row(session, &repo);
        assert_eq!(row.issue.as_deref(), Some(auto.as_str()));
        assert_eq!(row.also, vec![named]);
        assert!(row.elsewhere.contains(&sha));
        assert_eq!(issue_state(&tracker, &auto), "STARTED");
    }

    /// A placeholder another holder has on the tracker is theirs: the
    /// named commit leaves it alone.
    #[test]
    fn a_named_commit_leaves_a_filed_issue_another_holder_has() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let named = file_by_hand("Add slugify function and tests");
        let session = "harness-named-other";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        let auto = hook_row(session, &repo).issue.unwrap();
        let org_path = tracker.join("Software/probe/issues.org");
        let org = std::fs::read_to_string(&org_path).unwrap();
        let needle = format!(":ID:         {auto}\n");
        std::fs::write(
            &org_path,
            org.replace(&needle, &format!("{needle}:CLAIMED_BY: someone-else@zz9\n")),
        )
        .unwrap();
        let message = format!("{named}: add kebab_case");
        let _ = land_commit(&repo, "slug.py", &message);
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash",
                "tool_input": {"command": format!("git commit -m \"{message}\"")},
                "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&raw, &call);
        assert_eq!(
            hook_row(session, &repo).issue.as_deref(),
            Some(auto.as_str())
        );
        assert_eq!(issue_state(&tracker, &auto), "STARTED");
    }

    /// The willma w1 session 2 shape: the edit filed an issue, then
    /// `ljos_start` ran with a task of its own. The start keeps that issue,
    /// files nothing, and notes the task on it.
    #[test]
    fn ljos_start_keeps_the_issue_the_edit_already_filed() {
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
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("LJOS_HARNESS_SESSION");
        }
        let session = "harness-edit-then-start";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        let auto = hook_row(session, &repo).issue.unwrap();
        assert_eq!(creates(&tracker), 1);
        unsafe { std::env::set_var("LJOS_SESSION_ID", "mcp-after-edit") };
        let said = start_work(
            None,
            Some(repo.to_str().unwrap()),
            "add kebab_case function and test",
        )
        .unwrap();
        assert_eq!(
            said.lines().next(),
            Some(format!("claimed {auto}").as_str())
        );
        assert_eq!(creates(&tracker), 1, "ljos_start filed a second issue");
        let vissue = std::fs::read_to_string(tracker.join("vissue.log")).unwrap();
        assert!(
            vissue.contains(&format!("note {auto} ljos_start task: add kebab_case")),
            "{vissue}"
        );
        assert!(hook_row(session, &repo).issue.is_none());
        let start = load_book(&book_path(&sitting_key()));
        let row = start.repos.get(repo.to_str().unwrap()).unwrap();
        assert_eq!(row.issue.as_deref(), Some(auto.as_str()));
        assert!(row.started && row.filed);

        // The next write takes it back, and nothing is filed.
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "test_kebab.py"}}),
        );
        let _ = observe(&raw, &call);
        assert_eq!(
            hook_row(session, &repo).issue.as_deref(),
            Some(auto.as_str())
        );
        assert_eq!(creates(&tracker), 1);
    }

    fn plant_hold(dir: &Path, holder: &str, seat: &str, issue: &str) {
        let run = dir.join("run/ljos");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            run.join(format!("hold-{holder}")),
            format!("{holder}\n{seat}\n999999\nbash\n2026-10-20T00:00:00Z\n{issue}\n-\n"),
        )
        .unwrap();
    }

    fn issue_of(session: &str, repo: &Path) -> Option<String> {
        hook_row(session, repo).issue
    }

    /// Two hook conversations, one seat, one shared hold record. The claim
    /// conversation A filed stays in A's book. B files its own.
    #[test]
    fn a_second_conversation_of_one_seat_does_not_take_the_first_claim() {
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
            "CLAIMDAG_DIR",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SEAT", "codex");
            std::env::set_var("LJOS_HOLD", "codex-conv");
            std::env::remove_var("CLAIMDAG_DIR");
        }
        let (raw, call) = payload(
            "PostToolUse",
            "conv-a",
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "README.md"}}),
        );
        let _ = observe(&raw, &call);
        let issue_a = issue_of("conv-a", &repo).expect("conversation A filed");
        assert!(
            held_by_this_seat(&issue_a),
            "the planted hold would not be eligible"
        );
        plant_hold(dir.path(), "codex-conv", "codex", &issue_a);
        assert_eq!(
            crate::held_issue().as_deref(),
            Some(issue_a.as_str()),
            "the shared hold did not name A's claim"
        );
        CLAIM_DIR_SCANS.store(0, Ordering::SeqCst);
        let (raw, call) = payload(
            "PostToolUse",
            "conv-b",
            &repo,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "NOTES.md"}}),
        );
        let _ = observe(&raw, &call);
        let issue_b = issue_of("conv-b", &repo).expect("conversation B filed its own");
        assert_ne!(issue_b, issue_a, "B took A's claim");
        assert_eq!(issue_of("conv-a", &repo).as_deref(), Some(issue_a.as_str()));
        assert_eq!(creates(&tracker), 2);
        let log = std::fs::read_to_string(dir.path().join("run/ljos/book.log")).unwrap_or_default();
        assert!(
            !log.contains(&format!("claim: kept {issue_a}, held by this seat")),
            "{log}"
        );
        assert_eq!(
            CLAIM_DIR_SCANS.load(Ordering::SeqCst),
            0,
            "B read every book instead of the index"
        );
    }

    /// A row adopted from the hold record has `filed` clear. It still
    /// belongs to the conversation that adopted it.
    #[test]
    fn an_adopted_row_stays_with_its_conversation() {
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
            "CLAIMDAG_DIR",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SEAT", "codex");
            std::env::set_var("LJOS_HOLD", "codex-conv");
            std::env::remove_var("CLAIMDAG_DIR");
        }
        let issue = "probe-0ad0";
        let org = tracker.join("Software/probe");
        std::fs::create_dir_all(&org).unwrap();
        std::fs::write(
            org.join("issues.org"),
            format!("* STARTED [#C] adopted\n:PROPERTIES:\n:ID:         {issue}\n:END:\n"),
        )
        .unwrap();
        plant_hold(dir.path(), "codex-conv", "codex", issue);
        assert!(held_by_this_seat(issue), "the adopted issue is not held");
        assert_eq!(crate::held_issue().as_deref(), Some(issue));
        let repo_key = repo.display().to_string();
        let mut adopted = Book::default();
        adopted.repos.insert(
            repo_key.clone(),
            RepoBook {
                issue: Some(issue.to_string()),
                filed: false,
                started: false,
                finished: false,
                ..RepoBook::default()
            },
        );
        let path_a = book_path("conv-adopted");
        std::fs::create_dir_all(path_a.parent().unwrap()).unwrap();
        // Not store_book: that would write the index. The row has to be
        // found by reading the book, the way a row adopted before the
        // index existed is found.
        std::fs::write(&path_a, serde_json::to_string(&adopted).unwrap()).unwrap();
        CLAIM_DIR_SCANS.store(0, Ordering::SeqCst);
        let path_b = book_path("conv-later");
        let mut book_b = Book::default();
        assert!(
            !take_held_issue(&mut book_b, &path_b, &repo_key),
            "the adopted row was taken"
        );
        assert!(book_b.repos.values().all(|row| row.issue.is_none()));
        assert_eq!(
            load_book(&path_a).repos[&repo_key].issue.as_deref(),
            Some(issue)
        );
        assert!(
            CLAIM_DIR_SCANS.load(Ordering::SeqCst) >= 1,
            "an unindexed adopted row was not read from the books"
        );
    }

    /// Two hooks decide together. One adopts; the other sees that row.
    #[test]
    fn two_hooks_cannot_both_adopt_one_claim() {
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
            "CLAIMDAG_DIR",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SEAT", "codex");
            std::env::set_var("LJOS_HOLD", "codex-conv");
            std::env::remove_var("CLAIMDAG_DIR");
        }
        let issue = "probe-0ace";
        let org = tracker.join("Software/probe");
        std::fs::create_dir_all(&org).unwrap();
        std::fs::write(
            org.join("issues.org"),
            format!("* STARTED [#C] race\n:PROPERTIES:\n:ID:         {issue}\n:END:\n"),
        )
        .unwrap();
        plant_hold(dir.path(), "codex-conv", "codex", issue);
        assert!(held_by_this_seat(issue));
        assert_eq!(crate::held_issue().as_deref(), Some(issue));
        let repo_key = repo.display().to_string();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut guards = Vec::new();
        for session in ["race-conv-a", "race-conv-b"] {
            let barrier = std::sync::Arc::clone(&barrier);
            let repo_key = repo_key.clone();
            let session = session.to_string();
            guards.push(std::thread::spawn(move || {
                barrier.wait();
                let path = book_path(&session);
                let mut book = Book::default();
                let adopted = take_held_issue(&mut book, &path, &repo_key);
                (session, adopted)
            }));
        }
        let mut adopted = Vec::new();
        for guard in guards {
            let (session, took) = guard.join().expect("adopt thread");
            if took {
                adopted.push(session);
            }
        }
        assert_eq!(adopted.len(), 1, "both hooks adopted: {adopted:?}");
        let holders: Vec<&str> = ["race-conv-a", "race-conv-b"]
            .into_iter()
            .filter(|session| {
                load_book(&book_path(session))
                    .repos
                    .values()
                    .any(|row| row.issue.as_deref() == Some(issue))
            })
            .collect();
        assert_eq!(holders, [adopted[0].as_str()]);
    }

    /// The willma w5 shape: a prompt, then the session ends before any
    /// edit. No row with a null issue is left.
    #[test]
    fn a_sitting_that_never_claimed_leaves_no_row() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "harness-never-claimed";
        let (raw, call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "In this git repo, add a function slugify(s) to slug.py"}),
        );
        let _ = observe(&raw, &call);
        assert!(load_book(&book_path(session))
            .repos
            .values()
            .all(|row| row.issue.is_none()));
        let (raw, call) = payload("SessionEnd", session, &repo, serde_json::json!({}));
        let _ = observe(&raw, &call);
        let book = load_book(&book_path(session));
        assert!(book.repos.is_empty(), "{:?}", book.repos);
        assert_eq!(creates(&tracker), 0);
    }

    #[test]
    fn prompt_reads_are_bounded_by_bytes_on_large_regular_files() {
        use std::io::{Seek, SeekFrom, Write};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long-chat.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        file.seek(SeekFrom::Start(64 * 1024 * 1024)).unwrap();
        file.write_all(b"\n{\"type\":\"user_message\",\"message\":\"check the saddle\"}\n")
            .unwrap();
        drop(file);
        let tail = read_text_bounded(&path).unwrap();
        assert!(tail.len() <= TRANSCRIPT_TAIL_BYTES as usize);
        assert_eq!(super::super::last_user_text(&tail), "check the saddle");
    }

    /// The idle sweep drops a row that never claimed, once it is idle.
    #[test]
    fn the_idle_sweep_drops_a_row_that_never_claimed() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let mut book = Book::default();
        book.repos.insert(
            repo.to_str().unwrap().to_string(),
            RepoBook {
                start_sha: Some("1bfeb804".into()),
                title: Some("slugify".into()),
                last_unix: 1_000,
                ..RepoBook::default()
            },
        );
        book.last_unix = 1_000;
        let path = book_path("harness-idle-null");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        store_book(&path, &book);
        close_idle_sittings(1_000 + 10 * 24 * 3600, None, None);
        assert!(load_book(&path).repos.is_empty());
        assert_eq!(creates(&tracker), 0);
    }

    /// A claim graph's release hands the placeholder back to TODO before
    /// the cancel runs. The cancel takes it from TODO too, and leaves an
    /// issue that is DONE by then alone.
    #[test]
    fn a_placeholder_released_to_todo_is_still_cancelled() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, _repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let released = file_by_hand("Work in repo");
        assert_eq!(issue_state(&tracker, &released), "TODO");
        cancel_placeholder(&released, "probe-named");
        assert_eq!(issue_state(&tracker, &released), "CANCELLED");
        let done = file_by_hand("Work in repo again");
        let _ = std::process::Command::new("vissue")
            .args(["update", &done, "-s", "DONE"])
            .output()
            .unwrap();
        cancel_placeholder(&done, "probe-named");
        assert_eq!(issue_state(&tracker, &done), "DONE");
    }

    fn stamp_claimant(tracker: &Path, issue: &str, holder: &str) {
        let org_path = tracker.join("Software/probe/issues.org");
        let org = std::fs::read_to_string(&org_path).unwrap();
        let needle = format!(":ID:         {issue}\n");
        assert!(org.contains(&needle), "{org}");
        std::fs::write(
            &org_path,
            org.replace(&needle, &format!("{needle}:CLAIMED_BY: {holder}\n")),
        )
        .unwrap();
    }

    /// ljos-2di9 / ljos-3ocl: `ljos_start` books under the seat, and the
    /// hook under the harness session, and the two stamp different holder
    /// strings. The write joins the started issue.
    #[test]
    fn a_write_after_ljos_start_joins_when_the_holders_differ() {
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
            "LJOS_HOLD",
            "CODEX_THREAD_ID",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let uuid = "01a0dab3-895e-7533-93ca-c9807d5cb680";
        unsafe {
            std::env::set_var("LJOS_SEAT", "codex");
            std::env::set_var("LJOS_HOLD", uuid);
            std::env::remove_var("LJOS_HARNESS_SESSION");
            std::env::remove_var("CODEX_THREAD_ID");
            std::env::remove_var("LJOS_SESSION_ID");
        }
        let said = start_work(
            None,
            Some(repo.to_str().unwrap()),
            "Add slugify and its test",
        )
        .unwrap();
        let started = said
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("claimed "))
            .expect("the start claimed")
            .to_string();
        assert_eq!(creates(&tracker), 1);
        let seat = book_path(&sitting_key());
        assert_eq!(
            load_book(&seat)
                .repos
                .values()
                .find_map(|r| r.issue.clone()),
            Some(started.clone())
        );
        stamp_claimant(&tracker, &started, uuid);
        unsafe {
            std::env::remove_var("LJOS_HOLD");
            std::env::set_var("CODEX_THREAD_ID", "thread-2di9-mcp-hook");
        }
        let holder = crate::holder_name();
        assert!(
            !crate::same_seat_here(uuid, &holder),
            "the two holders are one seat to same_seat, so this does not exercise the alias: {holder}"
        );
        assert!(
            booking_alias(uuid, &holder),
            "holder {holder} is not the hook's name for this conversation"
        );
        assert!(this_seat_booked(&started));
        let session = "harness-2di9";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        assert_eq!(creates(&tracker), 1, "the write filed a second issue");
        assert_eq!(
            hook_row(session, &repo).issue.as_deref(),
            Some(started.as_str())
        );
        assert!(
            load_book(&seat)
                .repos
                .values()
                .all(|row| row.issue.as_deref() != Some(started.as_str())),
            "the seat book still holds the issue"
        );
    }

    /// ljos-6yav: `ljos_start` files nothing when this seat already holds an
    /// issue for the checkout. The hold record is the claim; the tracker
    /// names the hook's conversation id, which is not `SEAT@TAG`.
    #[test]
    fn ljos_start_keeps_the_issue_the_seat_already_holds() {
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
            "LJOS_HOLD",
            "CODEX_THREAD_ID",
            "CLAIMDAG_DIR",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let uuid = "01a0dab3-895e-7533-93ca-c9807d5cb680";
        let issue = "probe-6yav";
        unsafe {
            std::env::set_var("LJOS_SEAT", "codex");
            std::env::remove_var("LJOS_HOLD");
            std::env::remove_var("LJOS_HARNESS_SESSION");
            std::env::remove_var("LJOS_SESSION_ID");
            std::env::remove_var("CLAIMDAG_DIR");
            std::env::set_var("CODEX_THREAD_ID", "thread-6yav-hold");
        }
        let holder = crate::holder_name();
        assert!(
            !crate::same_seat_here(uuid, &holder),
            "same_seat already joins these, so the hold path is not the bug: {holder}"
        );
        let org = tracker.join("Software/probe");
        std::fs::create_dir_all(&org).unwrap();
        std::fs::write(
            org.join("issues.org"),
            format!(
                "* STARTED [#C] held already\n:PROPERTIES:\n:ID:         {issue}\n:CLAIMED_BY: {uuid}\n:END:\n"
            ),
        )
        .unwrap();
        plant_hold(dir.path(), &holder, "codex", issue);
        assert_eq!(crate::held_issue().as_deref(), Some(issue));
        assert!(this_seat_booked(issue), "the held issue is not this seat's");
        let said = start_work(None, Some(repo.to_str().unwrap()), "add the slug").unwrap();
        assert_eq!(
            said.lines().next(),
            Some(format!("claimed {issue}").as_str())
        );
        assert_eq!(creates(&tracker), 0, "ljos_start filed a second issue");
        let row = load_book(&book_path(&sitting_key()))
            .repos
            .get(repo.to_str().unwrap())
            .cloned()
            .unwrap_or_default();
        assert_eq!(row.issue.as_deref(), Some(issue));
        assert!(row.started);
    }

    /// ljos-4bfz: a note whose argument is prose attaches to the issue this
    /// seat holds. It does not file, and the prose is not the issue id.
    #[test]
    fn a_note_attaches_to_the_held_issue_and_never_files() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let prose = "slugify stays ascii";
        let err = note_issue(Some(prose)).unwrap_err();
        assert!(err.contains("never files"), "{err}");
        assert_eq!(creates(&tracker), 0);
        let session = "harness-4bfz";
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        let held = hook_row(session, &repo).issue.expect("the edit filed");
        assert_eq!(creates(&tracker), 1);
        assert_eq!(note_issue(Some(prose)).as_deref(), Ok(held.as_str()));
        assert_eq!(note_issue(None).as_deref(), Ok(held.as_str()));
        assert_eq!(creates(&tracker), 1, "the note filed an issue");
        let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
        assert!(!org.contains(prose), "the note became a title:\n{org}");
        assert!(org.contains(&held), "{org}");
    }

    /// ljos-98du: the harness command names an open issue and the commit
    /// message does not. The sitting books that issue. It does not file.
    #[test]
    fn a_harness_commit_that_names_an_open_issue_books_it() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let named = file_by_hand("Add slugify function and tests");
        assert_eq!(issue_state(&tracker, &named), "TODO");
        let _ = land_commit(&repo, "slug.py", "wip only");
        let session = "harness-98du";
        let command = format!("git commit -m \"wip only\" {named}");
        let (raw, call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash",
                "tool_input": {"command": command},
                "tool_response": {"exit_code": 0}}),
        );
        let _ = observe(&raw, &call);
        assert_eq!(creates(&tracker), 1, "the commit filed a new issue");
        assert_eq!(
            hook_row(session, &repo).issue.as_deref(),
            Some(named.as_str())
        );
    }

    /// ljos-5a7b: a prompt and a read, and no claim. The book keeps no row
    /// whose issue is null.
    #[test]
    fn a_prompt_and_a_read_leave_no_row() {
        let _g = lock();
        let _saved = saved_env();
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        let session = "harness-5a7b";
        let (raw, call) = payload(
            "UserPromptSubmit",
            session,
            &repo,
            serde_json::json!({"prompt": "In this git repo, add a function slugify(s) to slug.py"}),
        );
        let _ = observe(&raw, &call);
        assert!(
            load_book(&book_path(session)).repos.is_empty(),
            "the prompt left a row"
        );
        let (raw, call) = payload(
            "PreToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Read", "tool_input": {"file_path": "slug.py"}}),
        );
        let _ = observe(&raw, &call);
        assert!(
            load_book(&book_path(session)).repos.is_empty(),
            "the read left a row"
        );
        assert_eq!(creates(&tracker), 0);
    }

    /// Software-njbz: resuming the seat and shutting the MCP server down
    /// leave no row whose issue is null.
    #[test]
    fn mcp_shutdown_opens_no_null_issue_row() {
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
            "LJOS_SEAT",
            "LJOS_HOLD",
        ]);
        let (dir, tracker, _repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::set_var("LJOS_SEAT", "codex");
            std::env::remove_var("LJOS_HOLD");
            std::env::remove_var("LJOS_SESSION_ID");
        }
        resume_seat();
        mcp_shutdown();
        for path in book_files() {
            let book = load_book(&path);
            assert!(
                book.repos.values().all(|row| row.issue.is_some()),
                "{} has a null issue: {:?}",
                path.display(),
                book.repos
            );
        }
        assert_eq!(creates(&tracker), 0);
    }

    /// ljos-tha1: a timeout kills the runner and leaves the parent alive, so
    /// the next prompt never comes. A later MCP call finishes a dead owner
    /// at once, finishes a live owner whose row is old, and leaves a fresh
    /// live claim. An owner pid another process now runs under is dead.
    #[test]
    fn a_timed_out_claim_is_reaped_without_another_prompt() {
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
            "LJOS_SEAT",
            "LJOS_HOLD",
            "LJOS_SITTING_IDLE",
            "LJOS_MCP_IDLE",
        ]);
        let (dir, tracker, repo) = scratch();
        let _bins = arm(dir.path(), &tracker);
        unsafe {
            std::env::remove_var("LJOS_SEAT");
            std::env::remove_var("LJOS_HOLD");
            std::env::remove_var("LJOS_SESSION_ID");
            std::env::remove_var("LJOS_SITTING_IDLE");
            std::env::remove_var("LJOS_MCP_IDLE");
        }
        prompt_and_edit(&repo, "tha1-dead", "Add a readme line");
        prompt_and_edit(&repo, "tha1-old", "Add a readme line");
        prompt_and_edit(&repo, "tha1-live", "Add a readme line");
        prompt_and_edit(&repo, "tha1-reused", "Add a readme line");
        let dead_issue = hook_row("tha1-dead", &repo).issue.unwrap();
        let old_issue = hook_row("tha1-old", &repo).issue.unwrap();
        let live_issue = hook_row("tha1-live", &repo).issue.unwrap();
        let reused_issue = hook_row("tha1-reused", &repo).issue.unwrap();
        let me = crate::proc_tree::of(std::process::id()).unwrap();
        set_owner(dir.path(), "tha1-dead", dead_pid());
        set_owner(dir.path(), "tha1-live", me.pid);
        set_owner_at(dir.path(), "tha1-reused", me.pid, me.start + 1);
        let path = book_path("tha1-old");
        let mut book = load_book(&path);
        book.owner_pid = me.pid;
        book.owner_start = me.start;
        book.last_unix = 1;
        for row in book.repos.values_mut() {
            row.last_unix = 1;
        }
        store_book(&path, &book);
        resume_seat();
        let dead = load_book(&book_path("tha1-dead"));
        let dead_row = dead.repos.values().next().expect("the dead row stays");
        assert!(dead_row.finished, "a dead owner was not finished");
        assert_eq!(dead_row.issue.as_deref(), Some(dead_issue.as_str()));
        assert_eq!(issue_state(&tracker, &dead_issue), "DONE");
        assert!(
            load_book(&book_path("tha1-old")).repos.is_empty(),
            "an old row was kept"
        );
        assert_eq!(issue_state(&tracker, &old_issue), "DONE");
        let live = hook_row("tha1-live", &repo);
        assert!(!live.finished);
        assert_eq!(live.issue.as_deref(), Some(live_issue.as_str()));
        assert_eq!(issue_state(&tracker, &live_issue), "STARTED");
        let reused = load_book(&book_path("tha1-reused"));
        let reused_row = reused.repos.values().next().expect("the reused row stays");
        assert!(reused_row.finished, "a reused owner pid was taken as live");
        assert_eq!(issue_state(&tracker, &reused_issue), "DONE");
        assert_eq!(
            git_log(&tracker).matches("finished").count(),
            3,
            "{}",
            git_log(&tracker)
        );
        assert_eq!(creates(&tracker), 4);
    }
}
