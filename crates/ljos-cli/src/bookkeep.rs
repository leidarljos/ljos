//! The seat keeps the book when the model never calls ljos.
//!
//! A harness hook files a claim on the first edit in a session and
//! repository, cites a deed when it sees a git commit, and finishes at
//! Stop or the end of the session. `ljos-mcp` does the same on the first
//! write and on shutdown, for a client that has no hooks. A failure is
//! written to the runtime log and never becomes a deny: the agent's tool
//! call goes on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

/// How long one habitat command may run from a hook. A runner that waits
/// on PreToolUse treats a slow hook as a refusal, so the book stays inside
/// this and a warm call does not spawn anything.
const COMMAND_DEADLINE: Duration = Duration::from_secs(2);

/// Events that keep the book. A subagent stop and a bare argv line do not:
/// the parent session is the one that claimed.
const BOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
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

/// One session's book: the prompt the title came from, and one row per
/// repository the session has touched.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct Book {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    repos: BTreeMap<String, RepoBook>,
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
    /// Commit lines already cited, so PreToolUse and PostToolUse of the
    /// same command record one deed.
    #[serde(default)]
    deeds: Vec<String>,
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
    if matches!(call.event.as_str(), "PreToolUse" | "PostToolUse") {
        let commit = is_git_commit(&call.cue);
        if is_edit(&value, &call.cue) || commit {
            let _ = ensure_claim(&mut book, &repo);
        }
        if commit {
            let _ = ensure_deed(&mut book, &repo, &call.cue);
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
    let session = mcp_session();
    if let Some(title) = args.and_then(arg_title) {
        let _held = super::CommitLock::acquire(&super::runtime_dir().join("book.lock"));
        let path = book_path(&session);
        let mut book = load_book(&path);
        if book.prompt.trim().is_empty() {
            book.prompt = collapse(&title).chars().take(4000).collect();
        }
        store_book(&path, &book);
    }
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
        session: Some(session),
        shape: super::HookShape::Asks,
    };
    let _ = observe(&raw, &call);
}

/// Finish every claim this MCP process opened. Called when the server exits.
pub fn mcp_shutdown() {
    let session = mcp_session();
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
}

/// Finishes the MCP session's claims when the server process returns.
/// Bind it to a name. `let _ = ShutdownFinish` drops it at that statement.
pub struct ShutdownFinish;

impl Drop for ShutdownFinish {
    fn drop(&mut self) {
        mcp_shutdown();
    }
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

fn log_line(line: &str) {
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
    if let Some(issue) = super::held_issue() {
        let row = book.repos.entry(repo.to_string()).or_default();
        row.issue = Some(issue.clone());
        row.recall = recall_line(&issue);
        return Ok(());
    }
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

fn ensure_deed(book: &mut Book, repo: &str, cue: &str) -> Result<(), String> {
    let key = collapse(cue);
    if key.is_empty() {
        return Ok(());
    }
    let row = book.repos.entry(repo.to_string()).or_default();
    if row.deeds.iter().any(|seen| seen == &key) {
        return Ok(());
    }
    let Some(issue) = row.issue.clone() else {
        return Ok(());
    };
    match cite_deed(&issue, &key) {
        Ok(()) => {
            book.repos
                .entry(repo.to_string())
                .or_default()
                .deeds
                .push(key);
            Ok(())
        }
        Err(e) => {
            log_line(&format!("deed: {e}"));
            Ok(())
        }
    }
}

fn cite_deed(issue: &str, command: &str) -> Result<(), String> {
    let dir = super::runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("deed-{}.txt", super::session_tag(command)));
    std::fs::write(&path, command).map_err(|e| e.to_string())?;
    let seat = super::holder_name();
    let said = run_timed(
        "deedar",
        &[
            "create",
            "file",
            "--name",
            "commit",
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

/// A light close: the ticket is done, the claim is released, the tracker
/// commit says finished. The full `ljos finish` also syncs and writes a
/// lesson, which is the wrong weight for a hook the runner is about to kill.
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
seq = root / ".deedseq"
n = int(seq.read_text()) + 1 if seq.exists() else 1
seq.write_text(str(n))
print(f"id=deed-{n:04x}")
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
        // The same commit on the way out cites nothing a second time.
        let (post_raw, post_call) = payload(
            "PostToolUse",
            session,
            &repo,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m \"readme\""}}),
        );
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
