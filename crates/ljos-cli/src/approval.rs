//! Human consent for an ask verdict through a client form or trusted prompt.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{hook_output_ruled, HookCall, Rule};

/// How long a request waits for the person. A quarter of an hour was
/// shorter than the time it took a person to see the ask and get to a
/// terminal, so the id in the hook's message was dead on arrival.
const TTL_SECONDS: u64 = 24 * 60 * 60;
const MAX_REQUESTS: usize = 256;
/// The word that names the newest waiting request.
pub const LATEST: &str = "latest";
const MAX_STORE_BYTES: u64 = 2 * 1024 * 1024;

const TAG_WORDS: [&str; 32] = [
    "ash", "bay", "birch", "cedar", "elm", "fern", "fir", "grove", "hazel", "holly", "ivy",
    "larch", "leaf", "lime", "maple", "moss", "oak", "olive", "palm", "pine", "plum", "reed",
    "river", "rowan", "sage", "shore", "stone", "teak", "thorn", "vale", "wave", "yew",
];

/// A readable label for a full request id. The store checks label collisions;
/// the label carries no consent and does not replace the request's scope.
fn request_tag(id: &str) -> Option<String> {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let word = usize::from(u8::from_str_radix(&id[..2], 16).ok()?) % TAG_WORDS.len();
    Some(format!(
        "{}-{}",
        TAG_WORDS[word],
        id[2..8].to_ascii_lowercase()
    ))
}

fn valid_reference(reference: &str) -> bool {
    if reference == LATEST {
        return true;
    }
    if reference.len() == 32 && reference.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return true;
    }
    reference.split_once('-').is_some_and(|(word, suffix)| {
        TAG_WORDS.contains(&word)
            && suffix.len() == 6
            && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Scope {
    /// The conversation that asked. Empty when the hook named none.
    /// An empty value matches only another empty value, and only on the
    /// command and directory, so the request is not attached to a
    /// conversation that happens to be open.
    session: String,
    /// The seat that raised the request. The approval tool matches this
    /// plus the directory plus the request id, so a different conversation
    /// string from the same session still grants. It is not part of the
    /// one-use hook retry's equality. Task reuse also requires this seat.
    /// A named conversation stays bound to the
    /// command, directory, conversation, tool and rule. A request with no
    /// conversation stays bound to the command and directory. A seat rename
    /// between the ask and the retry does not drop the grant.
    #[serde(default)]
    seat: String,
    shape: String,
    tool: String,
    cwd: PathBuf,
    command: String,
    pattern: String,
    reason: String,
    #[serde(default)]
    task_push: Option<TaskPush>,
}

impl PartialEq for Scope {
    fn eq(&self, other: &Self) -> bool {
        // No conversation was stored. The same command and directory
        // match from any later caller that also names no conversation.
        // A caller that names one does not match.
        if self.session.is_empty() || other.session.is_empty() {
            return self.session.is_empty()
                && other.session.is_empty()
                && self.cwd == other.cwd
                && self.command == other.command;
        }
        self.session == other.session
            && self.shape == other.shape
            && self.tool == other.tool
            && self.cwd == other.cwd
            && self.command == other.command
            && self.pattern == other.pattern
            && self.reason == other.reason
    }
}

impl Eq for Scope {}

impl Scope {
    fn from_hook(input: &str, call: &HookCall, rule: &Rule) -> Result<Self> {
        let value: Value = serde_json::from_str(input)?;
        // A missing conversation id still records the request. The empty
        // session is not filled from another conversation.
        let session = call
            .session
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        let tool = value["tool_name"]
            .as_str()
            .or_else(|| value["toolName"].as_str())
            .filter(|s| !s.is_empty())
            .context("approval needs a tool name")?;
        let base = hook_directory(&value)?;
        let args = value
            .get("tool_input")
            .filter(|v| !v.is_null())
            .or_else(|| value.get("toolInput"));
        let directory = args
            .and_then(|args| {
                args.get("workdir")
                    .or_else(|| args.get("cwd"))
                    .or_else(|| args.get("working_directory"))
            })
            .filter(|v| !v.is_null());
        let cwd = match directory {
            Some(v) => base.join(v.as_str().context("invalid tool working directory")?),
            None => base,
        };
        let cwd = fs::canonicalize(cwd).context("approval working directory is unavailable")?;
        if !cwd.is_dir() || call.cue.is_empty() {
            bail!("approval needs a directory and a command");
        }
        let task_push = (!session.is_empty() && task_execution_supported(&value))
            .then(|| task_push_context(&call.cue, &cwd))
            .flatten();
        Ok(Self {
            session: session.into(),
            seat: crate::seat_name(),
            shape: format!("{:?}", call.shape),
            tool: tool.into(),
            cwd,
            command: call.cue.clone(),
            pattern: rule.pattern.clone(),
            reason: rule.reason.clone(),
            task_push,
        })
    }
}

/// The directory the hook named. A missing, empty or relative `cwd` is
/// resolved against the first absolute `workspace_roots` entry, or else
/// this process's current directory. Another session does not supply it.
fn hook_directory(value: &Value) -> Result<PathBuf> {
    let raw = value["cwd"].as_str().unwrap_or("");
    let given = Path::new(raw);
    if !raw.is_empty() && given.is_absolute() {
        return Ok(given.to_path_buf());
    }
    let anchor = value
        .get("workspace_roots")
        .and_then(Value::as_array)
        .and_then(|roots| {
            roots.iter().find_map(|entry| {
                let text = entry.as_str().filter(|text| !text.is_empty())?;
                let path = Path::new(text);
                path.is_absolute().then(|| path.to_path_buf())
            })
        })
        .or_else(|| std::env::current_dir().ok());
    let Some(anchor) = anchor else {
        bail!("approval needs an absolute working directory");
    };
    if raw.is_empty() {
        Ok(anchor)
    } else {
        Ok(anchor.join(given))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Request {
    id: String,
    scope: Scope,
    created: u64,
    approved: bool,
    #[serde(default)]
    for_task: bool,
}

impl Request {
    fn fresh(&self, now: u64) -> bool {
        now.checked_sub(self.created)
            .is_some_and(|age| age < TTL_SECONDS)
    }
}

struct Store {
    file: File,
    requests: Vec<Request>,
}

impl Drop for Store {
    fn drop(&mut self) {
        // A child can inherit this file description until exec. Unlock it
        // explicitly so that child cannot retain the completed transaction.
        // SAFETY: the descriptor belongs to this store and is still open.
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

impl Store {
    fn open(root: &Path, now: u64) -> Result<Self> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)?;
        let dir = fs::symlink_metadata(root)?;
        // SAFETY: geteuid has no arguments and only reads the process identity.
        let uid = unsafe { libc::geteuid() };
        if !dir.is_dir() || dir.uid() != uid || dir.mode() & 0o077 != 0 {
            bail!("approval directory must be owned by this user with mode 0700");
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(root.join("requests.json"))?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            bail!("approval store must be owned by this user with mode 0600");
        }
        // The descriptor owns the lock until close. A busy store refuses the
        // request so the hook can answer within its caller's deadline.
        // SAFETY: the descriptor belongs to the open file and stays live here.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!(
                "approval store is busy: {}",
                std::io::Error::last_os_error()
            );
        }
        if file.metadata()?.len() > MAX_STORE_BYTES {
            bail!("approval store exceeds its size limit");
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut requests: Vec<Request> = if bytes.is_empty() {
            Vec::new()
        } else {
            serde_json::from_slice(&bytes).context("invalid approval store")?
        };
        requests.retain(|request| request.fresh(now));
        Ok(Self { file, requests })
    }

    fn save(&mut self) -> Result<()> {
        let bytes = serde_json::to_vec(&self.requests)?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            bail!("approval store exceeds its size limit");
        }
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&bytes)?;
        self.file.set_len(bytes.len() as u64)?;
        self.file.sync_data()?;
        Ok(())
    }

    fn request_or_consume(&mut self, scope: Scope, now: u64) -> Result<Option<String>> {
        if let Some(index) = self.requests.iter().position(|request| {
            request.scope == scope
                && if request.for_task {
                    task_push_matches(&request.scope, &scope)
                } else {
                    request.approved || request.scope.task_push == scope.task_push
                }
        }) {
            if self.requests[index].approved || self.requests[index].for_task {
                if !self.requests[index].for_task {
                    self.requests.remove(index);
                    self.save()?;
                }
                return Ok(None);
            }
            return Ok(Some(self.requests[index].id.clone()));
        }
        if self.requests.len() >= MAX_REQUESTS {
            bail!("too many pending approvals; expired requests clear after a day");
        }
        let id = self.allocate_id(&mut File::open("/dev/urandom")?)?;
        self.requests.push(Request {
            id: id.clone(),
            scope,
            created: now,
            approved: false,
            for_task: false,
        });
        self.save()?;
        Ok(Some(id))
    }

    fn allocate_id(&self, random: &mut impl Read) -> Result<String> {
        for _ in 0..32 {
            let mut bytes = [0u8; 16];
            random.read_exact(&mut bytes)?;
            let id: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            let tag = request_tag(&id).expect("a generated request id is hexadecimal");
            if self.requests.iter().any(|request| {
                request.id == id || request_tag(&request.id).as_deref() == Some(tag.as_str())
            }) {
                continue;
            }
            return Ok(id);
        }
        bail!("could not allocate a unique approval tag; retry the command")
    }

    fn request_index(&self, reference: &str) -> Result<usize> {
        let reference = reference.trim().to_ascii_lowercase();
        if !valid_reference(&reference) {
            bail!("use the short approval tag or full request id printed by the hook, or `latest`");
        }
        if reference == LATEST {
            // The newest request still waiting: the one the person just saw.
            return self
                .requests
                .iter()
                .enumerate()
                .filter(|(_, request)| !request.approved && !request.for_task)
                .max_by_key(|(_, request)| request.created)
                .map(|(index, _)| index)
                .context("no approval request is waiting; retry the original command");
        }
        let mut matches = self.requests.iter().enumerate().filter(|(_, request)| {
            request.id.eq_ignore_ascii_case(&reference)
                || request_tag(&request.id).as_deref() == Some(reference.as_str())
        });
        let (index, _) = matches.next().context(
            "approval request is unknown, expired, or consumed; retry the original command",
        )?;
        if matches.next().is_some() {
            bail!("approval tag is ambiguous; use the full request id");
        }
        Ok(index)
    }

    fn pending(&self, id: &str, session: &str) -> Result<Request> {
        self.pending_for(id, session, "", Path::new(""))
    }

    /// A grant from this session. The conversation string may differ between
    /// the hook and the approval tool; the seat, the directory and the
    /// request id are the match.
    fn pending_for(&self, id: &str, session: &str, seat: &str, cwd: &Path) -> Result<Request> {
        let request = &self.requests[self.request_index(id)?];
        // A request with no conversation stays that way. The consent form
        // may show it. The caller's conversation is not written onto it.
        let no_conversation = request.scope.session.is_empty();
        let conversation_ok =
            request.scope.session == session || same_conversation(&request.scope.session, session);
        let place_ok =
            !seat.is_empty() && request.scope.seat == seat && same_dir(&request.scope.cwd, cwd);
        if !no_conversation && !conversation_ok && !place_ok {
            bail!("approval belongs to a different conversation");
        }
        if request.approved || request.for_task {
            bail!("request is already approved; retry the original command");
        }
        Ok(request.clone())
    }

    fn confirm(&mut self, pending: &Request) -> Result<String> {
        let current = self.pending(&pending.id, &pending.scope.session)?;
        if current.scope != pending.scope || current.created != pending.created {
            bail!("approval request changed while awaiting consent");
        }
        self.approve(&pending.id)
    }

    fn confirm_task(&mut self, pending: &Request) -> Result<String> {
        let current = self.pending(&pending.id, &pending.scope.session)?;
        if !task_push_matches(&current.scope, &pending.scope) || current.created != pending.created
        {
            bail!("task approval request changed while awaiting consent");
        }
        self.approve_with_task(&pending.id, true)
    }

    fn approve(&mut self, id: &str) -> Result<String> {
        self.approve_with_task(id, false)
    }

    fn approve_with_task(&mut self, id: &str, for_task: bool) -> Result<String> {
        let index = self.request_index(id)?;
        let request = &mut self.requests[index];
        if request.approved || request.for_task {
            bail!("request is already approved; retry the original command");
        }
        let title = if for_task {
            let task = request
                .scope
                .task_push
                .as_ref()
                .context("this request has no ordinary push task scope")?;
            if request.scope.session.is_empty() || request.scope.seat.is_empty() {
                bail!("task approval needs a named conversation and seat");
            }
            format!("Approved for task {}", task.task.issue)
        } else {
            "Approved once".to_string()
        };
        // The one-use bit stays false for task consent. A reader without
        // task binding checks cannot spend this consent.
        request.approved = !for_task;
        request.for_task = for_task;
        let answer = format!(
            "{title}: {}\nDirectory: {}\nConversation: {}\nRetry the original command.\n",
            request.scope.command,
            request.scope.cwd.display(),
            request.scope.session
        );
        self.save()?;
        Ok(answer)
    }
}

impl Store {
    /// [`Store::approve`] for a request this conversation raised: an id
    /// from another conversation's request, quoted into this one, grants
    /// nothing.
    fn approve_in(&mut self, id: &str, sessions: &[String]) -> Result<String> {
        self.pending_in(id, sessions)?;
        self.approve(id)
    }

    fn pending_in(&self, id: &str, sessions: &[String]) -> Result<Request> {
        let request = &self.requests[self.request_index(id)?];
        // Chat text never grants a request that was not asked in a
        // conversation, even when the session list contains an empty id.
        if request.scope.session.is_empty()
            || !sessions
                .iter()
                .any(|session| same_conversation(&request.scope.session, session))
        {
            bail!("approval {id} was not asked in this conversation");
        }
        Ok(request.clone())
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    if a.as_os_str().is_empty() || b.as_os_str().is_empty() {
        return false;
    }
    let left = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let right = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    left == right
}

/// Two spellings of one conversation. A runner sends `session_id` on a tool
/// call and `sessionId` on the prompt, or nests the id in a path. A short
/// name from another conversation does not match.
fn same_conversation(stored: &str, given: &str) -> bool {
    if stored == given {
        return true;
    }
    let (short, long) = if stored.len() <= given.len() {
        (stored, given)
    } else {
        (given, stored)
    };
    short.len() >= 32 && long.contains(short)
}

fn stamped_session_ids() -> Vec<String> {
    std::env::vars()
        .filter(|(key, value)| {
            (key.ends_with("_SESSION_ID")
                || key.ends_with("_THREAD_ID")
                || key.ends_with("_CONVERSATION_ID"))
                && key != "XDG_SESSION_ID"
                && key != "BLE_SESSION_ID"
                && value.trim().len() >= 8
        })
        .map(|(_, value)| value.trim().to_string())
        .collect()
}

/// The references a person approves: `approve TAG` or `approve ID`.
/// The verb and reference are case-insensitive; partial references grant nothing.
fn approved_ids(prompt: &str) -> Vec<String> {
    let prompt = prompt.trim();
    if let Some(body) = prompt.strip_prefix("<send_user_message_question_reply>") {
        let Some(body) = body.strip_suffix("</send_user_message_question_reply>") else {
            return Vec::new();
        };
        let Ok(replies) = serde_json::from_str::<Vec<Value>>(body) else {
            return Vec::new();
        };
        // The answer is the person's text. The question can quote a request
        // without expressing consent, including when the answer declines it.
        return replies
            .iter()
            .filter_map(|reply| reply.get("answer").and_then(Value::as_str))
            .flat_map(approval_directives)
            .collect();
    }
    approval_directives(prompt)
}

fn approval_directives(prompt: &str) -> Vec<String> {
    prompt
        .trim()
        .lines()
        .take(1)
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            if !words.next()?.eq_ignore_ascii_case("approve") {
                return None;
            }
            let reference = words
                .next()?
                .trim_start_matches(['`', '\"', '\''])
                .trim_end_matches(['`', '\"', '\'', '.', ',', ';', ':', '!'])
                .to_ascii_lowercase();
            valid_reference(&reference).then_some(reference)
        })
        .collect()
}

/// Consent given in the chat. A prompt reaches the hook as what the person
/// submitted, a channel no agent writes, so `approve ID` there grants the
/// request this conversation raised, as `ljos approve ID` in a terminal
/// does. The seat guard refuses typing an approval into a pane. `None` when
/// the prompt approves nothing; otherwise one line per id, granted or why
/// not.
pub fn approve_from_prompt(prompt: &str, session: Option<&str>) -> Option<String> {
    let mut sessions = Vec::new();
    if let Some(session) = session.map(str::trim).filter(|s| !s.is_empty()) {
        sessions.push(session.to_string());
    }
    for id in stamped_session_ids() {
        if !sessions.iter().any(|have| have == &id) {
            sessions.push(id);
        }
    }
    approve_from_prompt_at(prompt, &sessions, &root(), now())
}

fn approve_from_prompt_at(
    prompt: &str,
    sessions: &[String],
    root: &Path,
    now: Result<u64>,
) -> Option<String> {
    let ids = approved_ids(prompt);
    let task_ids = task_approval_ids(prompt);
    if ids.is_empty() {
        return None;
    }
    let mut said = String::new();
    for id in ids {
        let line = (|| {
            if sessions.iter().all(|session| session.is_empty()) {
                anyhow::bail!("approval needs a conversation id");
            }
            let stamp = now
                .as_ref()
                .map_err(|e| anyhow::anyhow!("{e:#}"))
                .copied()?;
            if task_ids.contains(&id) {
                let pending = Store::open(root, stamp)?.pending_in(&id, sessions)?;
                if !task_push_is_current(&pending.scope) {
                    bail!("task approval is unavailable or its push scope changed; command remains blocked");
                }
                Store::open(root, stamp)?.confirm_task(&pending)
            } else {
                Store::open(root, stamp)?.approve_in(&id, sessions)
            }
        })();
        match line {
            Ok(text) => said.push_str(&format!("The person approved in this chat. {text}")),
            Err(e) => said.push_str(&format!("approve {id}: {e:#}\n")),
        }
    }
    Some(said)
}

fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn root() -> PathBuf {
    crate::runtime_dir().join("approvals")
}

/// A person's terminal may grant, including one whose environment still
/// carries a runner's conversation id. An agent tool call has no terminal;
/// the seat guard also refuses the command itself.
fn terminal_consent(is_terminal: bool) -> Result<()> {
    if is_terminal {
        return Ok(());
    }
    bail!(
        "approve: consent is the person's. Use the connected client's \
         ljos_request_approval form or a trusted chat approval. The CLI fallback \
         requires the person's terminal"
    )
}

/// Record explicit consent for the pending request named by the hook.
/// The grant permits one matching attempt within the request's lifetime.
pub fn approve(id: &str) -> Result<String> {
    // SAFETY: isatty reads one descriptor's mode and cannot fail.
    let terminal = unsafe { libc::isatty(0) } == 1;
    terminal_consent(terminal)?;
    Store::open(&root(), now()?)?.approve(id)
}

/// Ask the connected client's user to consent to an existing request.
/// Tool arguments identify the request; only the client's form response grants it.
pub async fn request_approval(id: &str, peer: &rmcp::Peer<rmcp::RoleServer>) -> Result<String> {
    let session = crate::conversation_id().unwrap_or_else(crate::holder_name);
    let seat = crate::seat_name();
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|dir| fs::canonicalize(dir).ok())
        .unwrap_or_else(|| PathBuf::from("."));
    request_approval_at(id, &session, &seat, &cwd, peer, &root()).await
}

async fn request_approval_at(
    id: &str,
    session: &str,
    seat: &str,
    cwd: &Path,
    peer: &rmcp::Peer<rmcp::RoleServer>,
    root: &Path,
) -> Result<String> {
    use rmcp::model::{ElicitRequestParams, ElicitationSchema};

    let started = now()?;
    let pending = Store::open(root, started)?.pending_for(id, session, seat, cwd)?;
    let info = peer
        .peer_info()
        .context("approval client has not initialized")?;
    let supports_form = info.capabilities.elicitation.as_ref().is_some_and(|cap| {
        // The original elicitation capability is an empty object and means form.
        cap.form.is_some() || cap.url.is_none()
    });
    let tag = request_tag(&pending.id).unwrap_or_else(|| pending.id.clone());
    let offer_task = task_push_is_current(&pending.scope);
    if !supports_form {
        bail!("{}", chat_fallback(&pending, &tag, offer_task));
    }
    let schema = ElicitationSchema::from_json_schema(
        approval_schema(offer_task)
            .as_object()
            .expect("object schema")
            .clone(),
    )?;
    let task_detail = task_summary(&pending, offer_task);
    let message = format!(
        "Allow one attempt of this command?\nCommand (JSON): {}\nDirectory: {}\nConversation: {}\nRule: {}\nReason: {}\nRequest: {}\nThe grant expires 24 hours after the request was created.{task_detail}",
        serde_json::to_string(&pending.scope.command)?,
        serde_json::to_string(&pending.scope.cwd)?,
        pending.scope.session, pending.scope.pattern, pending.scope.reason,
        tag
    );
    // No store lock spans the user interaction. Confirmation reopens it and
    // checks expiry, identity and contents before granting the one retry.
    let remaining = TTL_SECONDS.saturating_sub(started.saturating_sub(pending.created));
    let answer = peer
        .create_elicitation_with_timeout(
            ElicitRequestParams::FormElicitationParams {
                meta: None,
                message,
                requested_schema: schema,
            },
            Some(std::time::Duration::from_secs(remaining)),
        )
        .await
        .context("client confirmation failed; command remains blocked")?;
    if !consents(&answer) {
        bail!("consent was not granted; command remains blocked");
    }
    let for_task = task_choice(&answer)?;
    if for_task {
        if !offer_task || !task_push_is_current(&pending.scope) {
            bail!(
                "task approval is unavailable or its push scope changed; command remains blocked"
            );
        }
        Store::open(root, now()?)?.confirm_task(&pending)
    } else {
        Store::open(root, now()?)?.confirm(&pending)
    }
}

fn consents(answer: &rmcp::model::ElicitResult) -> bool {
    answer.action == rmcp::model::ElicitationAction::Accept
        && answer.content.as_ref().and_then(|v| v.get("approve")) == Some(&Value::Bool(true))
}

/// Apply explicit consent only to an ask verdict. Denials reach the caller
/// unchanged, including denials from the policy binary.
pub fn hook_output(input: &str, call: &HookCall, context: &str, verdict: Option<&Rule>) -> String {
    let mut granted = false;
    let out = hook_output_at(input, call, context, verdict, &root(), now(), &mut granted);
    if let (true, Some(rule)) = (granted, verdict) {
        crate::log_bypass("approval", "human-approved command", &call.cue, rule);
    }
    if crate::answer_denies(&out) {
        if let Some(rule) = verdict {
            let seat = std::env::var("LJOS_SEAT")
                .ok()
                .filter(|seat| !seat.trim().is_empty())
                .unwrap_or_else(|| "seat".into());
            crate::digest::note_guard("refusal", &seat, &rule.reason);
        }
    }
    out
}

fn hook_output_at(
    input: &str,
    call: &HookCall,
    context: &str,
    verdict: Option<&Rule>,
    root: &Path,
    now: Result<u64>,
    granted: &mut bool,
) -> String {
    let Some(rule) = verdict.filter(|rule| rule.verdict == "ask") else {
        return hook_output_ruled(call, context, verdict);
    };
    // A runner that shows a prompt keeps the ask. Codex and Cursor
    // `preToolUse` do not: an ask is a failed hook there and the tool
    // runs, so the grant below is rendered as a deny.
    if call.event != "PreToolUse" || call.shape.asks() {
        return hook_output_ruled(call, context, verdict);
    }
    let result = (|| -> Result<Option<(bool, String)>> {
        let scope = Scope::from_hook(input, call, rule)?;
        let no_conversation = scope.session.is_empty();
        let now = now?;
        match Store::open(root, now)?.request_or_consume(scope, now)? {
            None => Ok(None),
            Some(id) => Ok(Some((no_conversation, id))),
        }
    })();
    match result {
        Ok(None) => {
            *granted = true;
            hook_output_ruled(call, context, None)
        }
        result => {
            let mut pending = rule.clone();
            let detail = match result {
                Ok(Some((no_conversation, id))) => {
                    let tag = request_tag(&id).unwrap_or(id);
                    if no_conversation {
                        format!("Call the ljos_request_approval MCP tool with id {tag} for a consent form, or run `ljos approve {tag}` in your own terminal. Retry only after consent is recorded. The default grant is one attempt of this command in this directory and expires 24 hours after the request. A consent form can offer reuse for an active ordinary-push task.")
                    } else {
                        format!("Reply `approve {tag}` in this conversation, call the ljos_request_approval MCP tool with id {tag} for a consent form, or run `ljos approve {tag}` in your own terminal. Retry only after consent is recorded. The default grant is one attempt of this command in this directory and expires 24 hours after the request. A consent form can offer reuse for an active ordinary-push task.")
                    }
                }
                Err(error) => format!(
                    "Approval could not be recorded: {error:#}. The command remains blocked."
                ),
                Ok(None) => unreachable!(),
            };
            pending.reason = format!("{} {detail}", rule.reason);
            hook_output_ruled(call, context, Some(&pending))
        }
    }
}

/// The live claim generation that the caller is working under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TaskLease {
    issue: String,
    holder: String,
    since: String,
    graph: String,
    generation: u64,
}

/// The destination and local branch of one ordinary branch push.
/// Remote addresses are fingerprinted rather than copied into consent records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TaskPush {
    task: TaskLease,
    common: PathBuf,
    remote: String,
    source: String,
    destination: String,
    addresses: String,
    configuration: String,
}

fn branch_name(name: &str) -> Option<&str> {
    let name = name.strip_prefix("refs/heads/").unwrap_or(name);
    if name.is_empty()
        || name.starts_with('-')
        || name.starts_with("refs/")
        || name.contains("..")
        || name.split('/').any(|part| {
            part.is_empty()
                || part.starts_with('.')
                || part.ends_with('.')
                || part.ends_with(".lock")
        })
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
    {
        return None;
    }
    Some(name)
}

/// Exact command reuse covers one plain branch push, with no shell expansion,
/// wrappers, executable options, force, deletion, tags or extra commands.
fn ordinary_push(command: &str) -> Option<(String, String, String)> {
    if !command
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b" /_-.:".contains(&byte))
    {
        return None;
    }
    let mut words = command.split_whitespace();
    if words.next()? != "git" || words.next()? != "push" {
        return None;
    }
    let mut rest: Vec<_> = words.collect();
    while rest
        .first()
        .is_some_and(|word| matches!(*word, "-q" | "--quiet" | "-u" | "--set-upstream"))
    {
        rest.remove(0);
    }
    let [remote, refspec] = rest.as_slice() else {
        return None;
    };
    if remote.is_empty()
        || remote.starts_with('-')
        || !remote
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        return None;
    }
    let (source, destination) = refspec.split_once(':').unwrap_or((refspec, refspec));
    if source != "HEAD" {
        branch_name(source)?;
    }
    let destination = branch_name(destination)?;
    if destination == "HEAD" {
        return None;
    }
    Some((
        remote.to_string(),
        source.to_string(),
        format!("refs/heads/{destination}"),
    ))
}

fn approval_git(cwd: &Path, args: &[&str]) -> Option<std::process::Output> {
    let mut command = crate::git_at(cwd);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::git_hooks::hook_child_env(&mut command);
    crate::no_git_prompt(&mut command);
    crate::bounded_output(&mut command).ok()
}

fn approval_git_text(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = approval_git(cwd, args)?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn config_allows_task_push(cwd: &Path, key: &str, wanted: &[&str]) -> bool {
    let Some(output) = approval_git(cwd, &["config", "--get", key]) else {
        return false;
    };
    output.status.code() == Some(1)
        || (output.status.success()
            && std::str::from_utf8(&output.stdout)
                .ok()
                .is_some_and(|text| wanted.contains(&text.trim().to_ascii_lowercase().as_str())))
}

fn graph_task_generation(text: &str, node: &str, actor: &str) -> Option<u64> {
    let words: Vec<_> = text.split_whitespace().collect();
    if words.first().copied() != Some(node)
        || !matches!(words.get(1).copied(), Some("claimed" | "running"))
    {
        return None;
    }
    let actors: Vec<_> = words
        .iter()
        .filter_map(|word| word.strip_prefix("assignee="))
        .collect();
    let generations: Vec<_> = words
        .iter()
        .filter_map(|word| word.strip_prefix("gen="))
        .collect();
    if actors.as_slice() != [actor] || generations.len() != 1 {
        return None;
    }
    generations[0].parse().ok()
}

fn task_push_target(command: &str, cwd: &Path, task: TaskLease) -> Option<TaskPush> {
    use sha2::{Digest as _, Sha256};
    let (remote, source, destination) = ordinary_push(command)?;
    if !config_allows_task_push(cwd, "push.followTags", &["false", "no", "off", "0"])
        || !config_allows_task_push(
            cwd,
            &format!("remote.{remote}.mirror"),
            &["false", "no", "off", "0"],
        )
        || !config_allows_task_push(cwd, "push.recurseSubmodules", &["false", "no", "off", "0"])
    {
        return None;
    }
    let common = approval_git_text(cwd, &["rev-parse", "--git-common-dir"])?;
    let common = fs::canonicalize(cwd.join(common.trim())).ok()?;
    let source = approval_git_text(
        cwd,
        &["rev-parse", "--symbolic-full-name", "--verify", &source],
    )?;
    let source = source.trim().to_string();
    if !source.starts_with("refs/heads/") || branch_name(&source).is_none() {
        return None;
    }
    let addresses = approval_git_text(cwd, &["remote", "get-url", "--push", "--all", &remote])?;
    if addresses.trim().is_empty() {
        return None;
    }
    let addresses = format!("{:x}", Sha256::digest(addresses.as_bytes()));
    let pattern = format!(
        "^(remote\\.{}\\.|push\\.|core\\.(sshcommand|gitproxy)$|protocol\\.)",
        remote.replace('.', "\\.")
    );
    let config = approval_git(cwd, &["config", "--null", "--get-regexp", &pattern])?;
    if !config.status.success() && config.status.code() != Some(1) {
        return None;
    }
    let configuration = format!("{:x}", Sha256::digest(&config.stdout));
    Some(TaskPush {
        task,
        common,
        remote,
        source,
        destination,
        addresses,
        configuration,
    })
}

fn task_push_context(command: &str, cwd: &Path) -> Option<TaskPush> {
    if crate::cloud::cloud_seat() || ordinary_push(command).is_none() {
        return None;
    }
    let (issue, hold) = crate::live_held_issue()?;
    if issue.is_empty()
        || hold.assignee.is_empty()
        || hold.since.is_empty()
        || hold.assignee != crate::holder_name()
        || hold.seat != crate::seat_name()
    {
        return None;
    }
    let graph = crate::claim_graph_scope();
    let node = crate::work_id(&issue);
    let actor = crate::work_id(&crate::occupancy_scope(&hold.assignee, &issue));
    let claim = crate::run_captured("claimdag", &["get", &node])
        .ok()?
        .stdout;
    let generation = graph_task_generation(&claim, &node, &actor)?;
    task_push_target(
        command,
        cwd,
        TaskLease {
            issue,
            holder: hold.assignee,
            since: hold.since,
            graph,
            generation,
        },
    )
}

fn task_execution_supported(input: &Value) -> bool {
    if crate::capture::not_the_person(&input.to_string()).is_some() {
        return false;
    }
    [Some(input), input.get("tool_input"), input.get("toolInput")]
        .into_iter()
        .flatten()
        .all(|value| {
            if ["workdir", "cwd", "working_directory"]
                .iter()
                .filter(|key| value.get(**key).is_some_and(|value| !value.is_null()))
                .count()
                > 1
            {
                return false;
            }
            [
                "env",
                "environment",
                "shell",
                "executable",
                "interpreter",
                "stdin",
                "script",
                "login",
                "tty",
            ]
            .iter()
            .all(|key| value.get(*key).is_none_or(Value::is_null))
        })
}

fn task_push_is_current(scope: &Scope) -> bool {
    !scope.session.is_empty()
        && !scope.seat.is_empty()
        && scope.task_push.is_some()
        && task_push_context(&scope.command, &scope.cwd) == scope.task_push
}

fn task_push_matches(left: &Scope, right: &Scope) -> bool {
    left == right
        && !left.session.is_empty()
        && !left.seat.is_empty()
        && left.seat == right.seat
        && left.task_push.is_some()
        && left.task_push == right.task_push
}

fn approval_schema(offer_task: bool) -> Value {
    let mut schema = serde_json::json!({
        "type": "object",
        "properties": {
            "approve": {
                "type": "boolean", "title": "Allow this command",
                "description": "Consent to the exact command and directory shown above.",
                "default": false
            }
        },
        "required": ["approve"]
    });
    if offer_task {
        schema["properties"]["for_task"] = serde_json::json!({
            "type": "boolean", "title": "Repeat this exact push for this task",
            "description": "Also allow matching ordinary branch pushes while the displayed task lease is active. The exact command, directory, conversation, rule, local branch and remote address binding must match. The grant expires with the request. Git checks still apply.",
            "default": false
        });
    }
    schema
}

fn task_summary(pending: &Request, offer_task: bool) -> String {
    pending.scope.task_push.as_ref().filter(|_| offer_task).map_or_else(String::new, |push| {
        format!("\nOptional task scope: {}\nLocal branch: {}\nRemote: {}\nDestination: {}\nReuse requires this exact command, directory, conversation, rule, task lease and remote address binding. Git checks still apply.", push.task.issue, push.source, push.remote, push.destination)
    })
}

fn chat_fallback(pending: &Request, tag: &str, offer_task: bool) -> String {
    if pending.scope.session.is_empty() {
        return format!("This client does not advertise consent forms and this request has no named conversation for trusted chat consent. Connect a client that supports form elicitation and request {tag}. The command remains blocked until the person approves.");
    }
    let task = if offer_task {
        format!(
            " For the displayed task scope, explicitly reply `approve {tag} for task`.{}",
            task_summary(pending, true)
        )
    } else {
        String::new()
    };
    format!("This client does not advertise consent forms. A trusted prompt hook can record `approve {tag}` in this conversation.{task} Otherwise connect a client that supports form elicitation. The command remains blocked until the person approves.")
}

fn task_choice(answer: &rmcp::model::ElicitResult) -> Result<bool> {
    match answer
        .content
        .as_ref()
        .and_then(|value| value.get("for_task"))
    {
        None | Some(Value::Bool(false)) => Ok(false),
        Some(Value::Bool(true)) => Ok(true),
        _ => bail!("invalid task choice; command remains blocked"),
    }
}

fn task_approval_ids(prompt: &str) -> Vec<String> {
    if let Some(body) = prompt
        .trim()
        .strip_prefix("<send_user_message_question_reply>")
    {
        let Some(body) = body.strip_suffix("</send_user_message_question_reply>") else {
            return Vec::new();
        };
        return serde_json::from_str::<Vec<Value>>(body)
            .unwrap_or_default()
            .iter()
            .filter_map(|reply| reply.get("answer").and_then(Value::as_str))
            .flat_map(task_approval_ids)
            .collect();
    }
    let words: Vec<_> = prompt
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    if words.len() == 4
        && words[2].eq_ignore_ascii_case("for")
        && words[3].eq_ignore_ascii_case("task")
    {
        approval_directives(prompt)
    } else {
        Vec::new()
    }
}
#[cfg(test)]
mod task_approval_tests {
    use super::*;

    fn scoped(cwd: &Path) -> Scope {
        Scope {
            session: "conversation-one".into(),
            seat: "owner-seat".into(),
            shape: "Fallback".into(),
            tool: "Bash".into(),
            cwd: cwd.into(),
            command: "git push origin HEAD:refs/heads/topic".into(),
            pattern: "git push*".into(),
            reason: "Publication needs consent.".into(),
            task_push: Some(TaskPush {
                task: TaskLease {
                    issue: "project-abcd".into(),
                    holder: "owner-holder".into(),
                    since: "lease-one".into(),
                    graph: "graph-one".into(),
                    generation: 7,
                },
                common: cwd.join(".git"),
                remote: "origin".into(),
                source: "refs/heads/topic".into(),
                destination: "refs/heads/topic".into(),
                addresses: "address-fingerprint-one".into(),
                configuration: "configuration-one".into(),
            }),
        }
    }

    fn grant(root: &Path, scope: Scope) -> String {
        let mut store = Store::open(root, 100).unwrap();
        let id = store.request_or_consume(scope, 100).unwrap().unwrap();
        let pending = store.pending(&id, "conversation-one").unwrap();
        store.confirm_task(&pending).unwrap();
        id
    }

    #[test]
    fn task_pushes_accept_only_plain_explicit_single_branch_commands() {
        for command in [
            "git push origin main",
            "git push -q -u origin HEAD:refs/heads/topic",
            "git push --quiet --set-upstream origin refs/heads/topic",
        ] {
            assert!(ordinary_push(command).is_some(), "{command}");
        }
        for command in [
            "git push",
            "git push origin",
            "git push --force origin main",
            "git push --force-with-lease origin main",
            "git push --delete origin main",
            "git push origin :main",
            "git push origin +main:main",
            "git push --tags origin main",
            "git push origin refs/tags/v1",
            "git push origin main:refs/notes/test",
            "git push origin main other",
            "git push origin main; touch result",
            "git push origin main && touch result",
            "git push origin $(cat target)",
            "git push origin `cat target`",
            "git push origin 'main'",
            "git -c core.hooksPath=/dev/null push origin main",
            "git -C other push origin main",
            "env git push origin main",
            "sh -c git push origin main",
            "git push https://example.invalid/repo main",
            "git push origin HEAD",
            "git push origin main\ngit push origin other",
            "git push origin main:../bad",
            "git push origin main:refs/heads/.bad",
            "git push origin main:refs/heads/a.lock",
        ] {
            assert!(ordinary_push(command).is_none(), "{command}");
        }
    }

    #[test]
    fn a_task_choice_is_optional_false_by_default_and_not_a_tool_argument() {
        let once = approval_schema(false);
        assert_eq!(once["properties"]["approve"]["default"], false);
        assert!(once["properties"].get("for_task").is_none());
        let task = approval_schema(true);
        assert_eq!(task["properties"]["for_task"]["type"], "boolean");
        assert_eq!(task["properties"]["for_task"]["default"], false);
        assert_eq!(task["required"], serde_json::json!(["approve"]));
        let mut answer = rmcp::model::ElicitResult::new(rmcp::model::ElicitationAction::Accept);
        for content in [
            serde_json::json!({"approve":true}),
            serde_json::json!({"approve":true,"for_task":false}),
        ] {
            answer.content = Some(content);
            assert!(consents(&answer));
            assert!(!task_choice(&answer).unwrap());
        }
        answer.content = Some(serde_json::json!({"approve":false,"for_task":true}));
        assert!(!consents(&answer));
        answer.content = Some(serde_json::json!({"approve":true,"for_task":"true"}));
        assert!(task_choice(&answer).is_err());
        answer.content = Some(serde_json::json!({"approve":true,"for_task":true}));
        assert!(consents(&answer));
        assert!(task_choice(&answer).unwrap());
    }

    #[test]
    fn explicit_task_consent_repeats_the_exact_push_without_consuming_it() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let scope = scoped(temp.path());
        grant(&root, scope.clone());
        for stamp in 101..105 {
            assert!(Store::open(&root, stamp)
                .unwrap()
                .request_or_consume(scope.clone(), stamp)
                .unwrap()
                .is_none());
        }
        let store = Store::open(&root, 105).unwrap();
        assert_eq!(store.requests.len(), 1);
        assert!(!store.requests[0].approved && store.requests[0].for_task);
    }

    #[test]
    fn task_consent_is_bound_to_every_command_and_lease_target_field() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let scope = scoped(temp.path());
        grant(&root, scope.clone());
        for field in [
            "session",
            "seat",
            "shape",
            "tool",
            "cwd",
            "command",
            "pattern",
            "reason",
            "issue",
            "holder",
            "since",
            "graph",
            "generation",
            "common",
            "remote",
            "source",
            "destination",
            "addresses",
            "configuration",
            "missing-task",
        ] {
            let mut other = scope.clone();
            match field {
                "session" => other.session.push_str("-other"),
                "seat" => other.seat.push_str("-other"),
                "shape" => other.shape.push_str("-other"),
                "tool" => other.tool.push_str("-other"),
                "cwd" => other.cwd = temp.path().join("elsewhere"),
                "command" => other.command.push_str(" --force"),
                "pattern" => other.pattern.push_str("-other"),
                "reason" => other.reason.push_str("-other"),
                "missing-task" => other.task_push = None,
                field => {
                    let task = other.task_push.as_mut().unwrap();
                    match field {
                        "issue" => task.task.issue.push_str("-other"),
                        "holder" => task.task.holder.push_str("-other"),
                        "since" => task.task.since.push_str("-other"),
                        "graph" => task.task.graph.push_str("-other"),
                        "generation" => task.task.generation += 1,
                        "common" => task.common = temp.path().join("other.git"),
                        "remote" => task.remote.push_str("-other"),
                        "source" => task.source.push_str("-other"),
                        "destination" => task.destination.push_str("-other"),
                        "addresses" => task.addresses.push_str("-other"),
                        "configuration" => task.configuration.push_str("-other"),
                        _ => unreachable!(),
                    }
                }
            }
            assert!(
                Store::open(&root, 101)
                    .unwrap()
                    .request_or_consume(other, 101)
                    .unwrap()
                    .is_some(),
                "{field}"
            );
        }
        assert!(Store::open(&root, 102)
            .unwrap()
            .request_or_consume(scope, 102)
            .unwrap()
            .is_none());
    }

    #[test]
    fn task_consent_expires_with_the_existing_request_lifetime() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        for stamp in [99, 100 + TTL_SECONDS] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("approvals");
            let scope = scoped(temp.path());
            let id = grant(&root, scope.clone());
            let replacement = Store::open(&root, stamp)
                .unwrap()
                .request_or_consume(scope, stamp)
                .unwrap()
                .unwrap();
            assert_ne!(replacement, id);
            let store = Store::open(&root, stamp).unwrap();
            assert!(!store.requests[0].approved && !store.requests[0].for_task);
        }
    }

    #[test]
    fn a_task_confirmation_rechecks_the_pending_scope_without_granting_changes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let mut store = Store::open(&root, 100).unwrap();
        let id = store
            .request_or_consume(scoped(temp.path()), 100)
            .unwrap()
            .unwrap();
        let pending = store.pending(&id, "conversation-one").unwrap();
        let mut changed = pending.clone();
        changed.scope.task_push.as_mut().unwrap().task.generation += 1;
        assert!(store.confirm_task(&changed).is_err());
        assert!(!store.requests[0].approved);
        let mut changed = pending.clone();
        changed
            .scope
            .task_push
            .as_mut()
            .unwrap()
            .addresses
            .push_str("-other");
        assert!(store.confirm_task(&changed).is_err());
        assert!(!store.requests[0].approved);
        store.confirm_task(&pending).unwrap();
        assert!(store.confirm_task(&pending).is_err());
    }

    #[test]
    fn stored_one_use_grants_remain_one_use_without_new_scope_fields() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let scope = scoped(temp.path());
        let mut store = Store::open(&root, 100).unwrap();
        let id = store
            .request_or_consume(scope.clone(), 100)
            .unwrap()
            .unwrap();
        store.approve(&id).unwrap();
        let mut value = serde_json::to_value(&store.requests[0]).unwrap();
        value.as_object_mut().unwrap().remove("for_task");
        value["scope"].as_object_mut().unwrap().remove("task_push");
        store.requests[0] = serde_json::from_value(value).unwrap();
        store.save().unwrap();
        drop(store);
        assert!(Store::open(&root, 101)
            .unwrap()
            .request_or_consume(scope.clone(), 101)
            .unwrap()
            .is_none());
        assert!(Store::open(&root, 102)
            .unwrap()
            .request_or_consume(scope, 102)
            .unwrap()
            .is_some());
    }

    #[test]
    fn task_chat_choice_reads_only_the_answer_and_requires_an_explicit_suffix() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            task_approval_ids(&format!("approve {id} for task")),
            vec![id.to_string()]
        );
        assert!(task_approval_ids(&format!("approve {id}")).is_empty());
        assert!(task_approval_ids(&format!("approve {id} for tasks")).is_empty());
        let wrapped = format!(
            "<send_user_message_question_reply>{}</send_user_message_question_reply>",
            serde_json::json!([{"question":format!("approve {id} for task"),"answer":"decline"}])
        );
        assert!(task_approval_ids(&wrapped).is_empty());
        let wrapped = format!(
            "<send_user_message_question_reply>{}</send_user_message_question_reply>",
            serde_json::json!([{"question":"scope?","answer":format!("approve {id} for task")}])
        );
        assert_eq!(task_approval_ids(&wrapped), vec![id.to_string()]);
    }

    #[test]
    fn authoritative_release_or_reclaim_invalidates_an_unchanged_local_hold() {
        let temp = tempfile::tempdir().unwrap();
        let mut scope = scoped(temp.path());
        let node = "0123456789abcdef0123456789abcdef";
        let actor = "abcdef0123456789abcdef0123456789";
        let original = format!("{node} claimed task gen=7 assignee={actor}");
        assert_eq!(graph_task_generation(&original, node, actor), Some(7));
        for status in ["ready", "done", "failed", "cancelled"] {
            let released = format!("{node} {status} task gen=8 assignee={actor}");
            assert_eq!(graph_task_generation(&released, node, actor), None);
        }
        let other = format!("{node} claimed task gen=8 assignee=other");
        assert_eq!(graph_task_generation(&other, node, actor), None);
        let reclaimed = format!("{node} claimed task gen=9 assignee={actor}");
        let original_scope = scope.clone();
        scope.task_push.as_mut().unwrap().task.generation =
            graph_task_generation(&reclaimed, node, actor).unwrap();
        assert!(!task_push_matches(&original_scope, &scope));
        for malformed in [
            format!("other claimed task gen=7 assignee={actor}"),
            format!("{node} claimed task gen=7 gen=8 assignee={actor}"),
            format!("{node} claimed task gen=7 assignee={actor} assignee={actor}"),
        ] {
            assert_eq!(graph_task_generation(&malformed, node, actor), None);
        }
    }

    #[test]
    fn a_changed_task_or_remote_mints_a_new_pending_consent_form() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        for field in ["generation", "addresses"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("approvals");
            let scope = scoped(temp.path());
            let mut store = Store::open(&root, 100).unwrap();
            let first = store
                .request_or_consume(scope.clone(), 100)
                .unwrap()
                .unwrap();
            let pending = store.pending(&first, "conversation-one").unwrap();
            let mut changed = scope.clone();
            let push = changed.task_push.as_mut().unwrap();
            if field == "generation" {
                push.task.generation += 1;
            } else {
                push.addresses.push_str("-other");
            }
            let second = store
                .request_or_consume(changed.clone(), 101)
                .unwrap()
                .unwrap();
            assert_ne!(first, second, "{field}");
            assert_eq!(
                store.request_or_consume(changed.clone(), 102).unwrap(),
                Some(second.clone())
            );
            let mut wrong_form = pending.clone();
            wrong_form.id = second.clone();
            assert!(store.confirm_task(&wrong_form).is_err());
            assert!(!store.requests.iter().any(|request| request.approved));
            store.confirm_task(&pending).unwrap();
            assert_eq!(
                store.request_or_consume(changed, 103).unwrap(),
                Some(second)
            );
        }
    }

    #[test]
    fn unsupported_forms_offer_chat_scope_only_when_the_task_is_eligible() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let mut store = Store::open(&root, 100).unwrap();
        let id = store
            .request_or_consume(scoped(temp.path()), 100)
            .unwrap()
            .unwrap();
        let pending = store.pending(&id, "conversation-one").unwrap();
        let once = chat_fallback(&pending, "olive-123abc", false);
        assert!(once.contains("trusted prompt hook"));
        assert!(once.contains("approve olive-123abc"));
        assert!(!once.contains("for task"));
        assert!(!once.contains("terminal"));
        let task = chat_fallback(&pending, "olive-123abc", true);
        assert!(task.contains("approve olive-123abc for task"));
        assert!(task.contains("project-abcd"));
        assert!(task.contains("refs/heads/topic"));
        assert!(task.contains("command remains blocked"));
        assert!(!store.requests[0].approved);
        let mut anonymous = pending.clone();
        anonymous.scope.session.clear();
        let unsupported = chat_fallback(&anonymous, "olive-123abc", false);
        assert!(!unsupported.contains("approve olive-123abc"));
        assert!(unsupported.contains("no named conversation"));
        assert!(unsupported.contains("supports form elicitation"));
    }

    #[test]
    fn local_git_configuration_and_branch_changes_invalidate_task_targets() {
        let _environment = crate::test_env_lock();
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path();
        let git = |args: &[&str]| {
            let output = crate::git_at(cwd).args(args).output().unwrap();
            assert!(
                output.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        for args in [
            vec!["init", "-q", "--initial-branch=main"],
            vec!["config", "user.name", "publication test"],
            vec!["config", "user.email", "publication@example.invalid"],
            vec!["commit", "-q", "--allow-empty", "-m", "seed"],
            vec!["remote", "add", "origin", "https://example.invalid/one"],
        ] {
            git(&args);
        }
        let lease = scoped(cwd).task_push.unwrap().task;
        let command = "git push origin HEAD:refs/heads/topic";
        let original = task_push_target(command, cwd, lease.clone()).unwrap();
        git(&["config", "remote.origin.receivepack", "git-receive-pack"]);
        let configured = task_push_target(command, cwd, lease.clone()).unwrap();
        assert_eq!(configured.addresses, original.addresses);
        assert_ne!(configured.configuration, original.configuration);
        git(&["config", "--unset", "remote.origin.receivepack"]);
        git(&["config", "core.sshCommand", "ssh -o BatchMode=yes"]);
        assert_ne!(
            task_push_target(command, cwd, lease.clone())
                .unwrap()
                .configuration,
            original.configuration
        );
        git(&["config", "--unset", "core.sshCommand"]);
        for (key, value) in [
            ("push.followTags", "true"),
            ("remote.origin.mirror", "true"),
            ("push.recurseSubmodules", "on-demand"),
        ] {
            git(&["config", key, value]);
            assert!(
                task_push_target(command, cwd, lease.clone()).is_none(),
                "{key}"
            );
            git(&["config", "--unset", key]);
        }
        git(&[
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://example.invalid/two",
        ]);
        assert_ne!(
            task_push_target(command, cwd, lease.clone())
                .unwrap()
                .addresses,
            original.addresses
        );
        git(&[
            "remote",
            "set-url",
            "--add",
            "--push",
            "origin",
            "https://example.invalid/three",
        ]);
        let multiple = task_push_target(command, cwd, lease.clone()).unwrap();
        assert_ne!(multiple.addresses, original.addresses);
        git(&["checkout", "-q", "-b", "other"]);
        assert_ne!(
            task_push_target(command, cwd, lease.clone())
                .unwrap()
                .source,
            original.source
        );
        git(&["checkout", "-q", "--detach"]);
        assert!(task_push_target(command, cwd, lease).is_none());
    }

    #[test]
    fn per_call_execution_overrides_cannot_reuse_an_ordinary_push_task() {
        let _environment = crate::test_env_lock();
        std::env::remove_var("LJOS_UNATTENDED");
        let plain =
            serde_json::json!({"tool_input":{"command":"git push origin main", "workdir":"/tmp"}});
        assert!(task_execution_supported(&plain));
        for marker in ["agent_id", "agentId"] {
            let mut child = plain.clone();
            child[marker] = serde_json::json!("child-one");
            assert!(!task_execution_supported(&child), "{marker}");
        }
        for field in [
            "env",
            "environment",
            "shell",
            "executable",
            "interpreter",
            "stdin",
            "script",
            "login",
            "tty",
        ] {
            for envelope in ["tool_input", "toolInput"] {
                let mut overridden = plain.clone();
                overridden[envelope] = serde_json::json!({});
                overridden[envelope][field] = serde_json::json!("override");
                assert!(!task_execution_supported(&overridden), "{envelope}.{field}");
            }
            let mut overridden = plain.clone();
            overridden[field] = serde_json::json!("override");
            assert!(!task_execution_supported(&overridden), "{field}");
        }
    }

    #[test]
    fn a_working_directory_alias_is_resolved_before_request_scoping() {
        let temp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let input = serde_json::json!({"hook_event_name":"PreToolUse", "session_id":"fixture-session", "cwd":temp.path(), "tool_name":"Bash", "tool_input":{"command":"printf fixture", "working_directory":other.path()}}).to_string();
        let call = crate::hook_call(&input);
        let rule = Rule {
            pattern: "fixture".into(),
            verdict: "ask".into(),
            reason: "Fixture".into(),
        };
        let scope = Scope::from_hook(&input, &call, &rule).unwrap();
        assert_eq!(scope.cwd, fs::canonicalize(other.path()).unwrap());
    }

    #[test]
    fn a_task_grant_never_decodes_as_approved_in_the_one_use_format() {
        #[derive(Deserialize)]
        struct OldScope {
            session: String,
            #[serde(default)]
            seat: String,
            shape: String,
            tool: String,
            cwd: PathBuf,
            command: String,
            pattern: String,
            reason: String,
        }
        #[derive(Deserialize)]
        struct OldRequest {
            id: String,
            scope: OldScope,
            created: u64,
            approved: bool,
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let scope = scoped(temp.path());
        let id = grant(&root, scope.clone());
        let bytes = fs::read(root.join("requests.json")).unwrap();
        let decoded: Vec<OldRequest> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.len(), 1);
        let old = &decoded[0];
        assert!(!old.approved);
        assert_eq!(old.id, id);
        assert_eq!(old.created, 100);
        assert_eq!(old.scope.session, scope.session);
        assert_eq!(old.scope.seat, scope.seat);
        assert_eq!(old.scope.shape, scope.shape);
        assert_eq!(old.scope.tool, scope.tool);
        assert_eq!(old.scope.cwd, scope.cwd);
        assert_eq!(old.scope.command, scope.command);
        assert_eq!(old.scope.pattern, scope.pattern);
        assert_eq!(old.scope.reason, scope.reason);
        assert!(!decoded.iter().any(|request| request.approved));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook_call;

    #[tokio::test]
    async fn approval_uses_the_full_call_id_with_a_named_holder() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        crate::as_thread(Some("conversation-1".into()), async {
            let session = crate::conversation_id().unwrap();
            assert_eq!(session, "conversation-1");
            assert_ne!(session, crate::holder_name());
            assert!(Store::open(&root, 101)
                .unwrap()
                .pending(&id, &session)
                .is_ok());
            crate::as_thread(Some("conversation-2".into()), async {
                let other = crate::conversation_id().unwrap();
                assert!(Store::open(&root, 102)
                    .unwrap()
                    .pending(&id, &other)
                    .is_err());
            })
            .await;
            assert_eq!(crate::conversation_id().as_deref(), Some("conversation-1"));
        })
        .await;
    }

    fn ask() -> Rule {
        Rule {
            pattern: "git push*".into(),
            verdict: "ask".into(),
            reason: "A push needs consent.".into(),
        }
    }

    fn one(id: &str) -> Vec<String> {
        vec![id.to_string()]
    }

    fn input(cwd: &Path) -> String {
        serde_json::json!({"hook_event_name":"PreToolUse", "turn_id":"turn-1",
            "session_id":"conversation-1", "cwd":cwd, "tool_name":"Bash",
            "tool_input":{"command":"git push origin main"}})
        .to_string()
    }

    fn output(input: &str, rule: &Rule, root: &Path, now: u64) -> String {
        hook_output_at(
            input,
            &hook_call(input),
            "",
            Some(rule),
            root,
            Ok(now),
            &mut false,
        )
    }

    fn request_id(output: &str) -> String {
        let value: Value = serde_json::from_str(output).unwrap();
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
        let reason = value["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap();
        reason
            .split("ljos approve ")
            .nth(1)
            .unwrap()
            .split('`')
            .next()
            .unwrap()
            .into()
    }

    #[test]
    fn a_question_quoted_in_a_reply_cannot_grant_consent() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let tag = request_id(&output(&input, &ask(), &root, 100));
        let full = Store::open(&root, 101).unwrap().requests[0].id.clone();
        let packet = |answer: &str| {
            format!(
                "<send_user_message_question_reply>\n{}\n</send_user_message_question_reply>",
                serde_json::json!([{"answer": answer,
                "question": format!("Reply approve {full} to permit the command."),
                "questionItemId": "confirmation-1"}]),
            )
        };
        for answer in [
            "no".to_string(),
            "ok".to_string(),
            format!("approve {}", &full[..26]),
            format!("do not approve {full}"),
            format!("no\napprove {full}"),
        ] {
            let result =
                approve_from_prompt_at(&packet(&answer), &one("conversation-1"), &root, Ok(102));
            assert!(result.is_none(), "{answer}: {result:?}");
            assert!(!Store::open(&root, 103).unwrap().requests[0].approved);
        }
        for quoted in [
            format!("Do not approve {full}"),
            format!("The question says approve {full}"),
            format!("> approve {tag}"),
        ] {
            assert!(
                approve_from_prompt_at(&quoted, &one("conversation-1"), &root, Ok(104)).is_none()
            );
        }
        let result = approve_from_prompt_at(
            &packet(&format!("approve {tag} and retry")),
            &one("conversation-1"),
            &root,
            Ok(105),
        )
        .unwrap();
        assert!(result.contains("Approved once"), "{result}");
        assert!(output(&input, &ask(), &root, 106).is_empty());
    }

    #[test]
    fn short_tags_and_full_ids_share_one_grant() {
        for use_tag in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("approvals");
            let input = input(temp.path());
            let tag = request_id(&output(&input, &ask(), &root, 100));
            assert!(tag.len() <= 12 && tag.contains('-'), "{tag}");
            let full = Store::open(&root, 101).unwrap().requests[0].id.clone();
            assert_eq!(full.len(), 32);
            let reference = if use_tag { &tag } else { &full };
            let answer = approve_from_prompt_at(
                &format!("APPROVE {}.", reference.to_ascii_uppercase()),
                &one("conversation-1"),
                &root,
                Ok(102),
            )
            .unwrap();
            assert!(answer.contains("Approved once"), "{answer}");
            assert!(output(&input, &ask(), &root, 103).is_empty());
            let mut store = Store::open(&root, 104).unwrap();
            assert!(store.approve(&tag).is_err());
            assert!(store.approve(&full).is_err());
        }
    }

    #[test]
    fn short_tag_references_match_exactly() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let tag = request_id(&output(&input, &ask(), &root, 100));
        let mut store = Store::open(&root, 101).unwrap();
        let mut changed = tag.clone();
        let last = changed.pop().unwrap();
        changed.push(if last == '0' { '1' } else { '0' });
        for invalid in [
            tag[..tag.len() - 1].to_string(),
            format!("{tag}0"),
            changed,
            store.requests[0].id[..31].to_string(),
            "é".repeat(16),
        ] {
            assert!(
                store.pending(&invalid, "conversation-1").is_err(),
                "{invalid}"
            );
            assert!(store.approve(&invalid).is_err(), "{invalid}");
            assert!(!store.requests[0].approved);
        }
    }

    #[test]
    fn allocation_resamples_a_short_tag_collision() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        request_id(&output(&input, &ask(), &root, 100));
        let mut store = Store::open(&root, 101).unwrap();
        store.requests[0].id = "00112233000000000000000000000000".into();
        let mut bytes = vec![0u8; 32];
        bytes[..4].copy_from_slice(&[0x20, 0x11, 0x22, 0x33]);
        bytes[16] = 1;
        let allocated = store.allocate_id(&mut std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(allocated, "01000000000000000000000000000000");
        assert_eq!(store.requests.len(), 1);
        assert!(!store.requests[0].approved);
        store.requests[0].id = "0".repeat(32);
        assert!(store
            .allocate_id(&mut std::io::Cursor::new(vec![0u8; 16 * 32]))
            .is_err());
        assert_eq!(store.requests.len(), 1);
    }

    #[test]
    fn approval_allows_one_matching_retry_and_cannot_be_replayed() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        assert_eq!(request_id(&output(&input, &ask(), &root, 101)), id);
        Store::open(&root, 102).unwrap().approve(&id).unwrap();
        assert!(output(&input, &ask(), &root, 103).is_empty());
        assert!(Store::open(&root, 104).unwrap().approve(&id).is_err());
        let other = request_id(&output(&input, &ask(), &root, 105));
        assert_ne!(id, other);
        assert!(Store::open(&root, 106).unwrap().approve(&id).is_err());
    }

    #[test]
    fn the_person_approves_in_the_chat_for_this_conversation_only() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        assert!(
            approve_from_prompt_at("carry on", &one("conversation-1"), &root, Ok(101)).is_none()
        );
        let elsewhere = approve_from_prompt_at(
            &format!("approve {id}"),
            &one("conversation-2"),
            &root,
            Ok(101),
        )
        .unwrap();
        assert!(
            elsewhere.contains("not asked in this conversation"),
            "{elsewhere}"
        );
        assert_eq!(
            request_id(&output(&input, &ask(), &root, 102)),
            id,
            "still pending"
        );
        let said = approve_from_prompt_at(
            &format!("Approve `{id}`, then retry."),
            &one("conversation-1"),
            &root,
            Ok(103),
        )
        .unwrap();
        assert!(said.contains("Approved once"), "{said}");
        assert!(
            output(&input, &ask(), &root, 104).is_empty(),
            "the retry runs once"
        );
        assert_ne!(request_id(&output(&input, &ask(), &root, 105)), id);
    }

    #[test]
    fn a_longer_spelling_of_the_same_conversation_grants() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let cwd = temp.path();
        let session = "01a10b08-49eb-73e3-9a01-87e9d1f6ae2e";
        let body = serde_json::json!({"hook_event_name":"PreToolUse", "turn_id":"turn-1",
            "session_id": session, "cwd": cwd, "tool_name":"Bash",
            "tool_input":{"command":"git push origin main"}})
        .to_string();
        let id = request_id(&output(&body, &ask(), &root, 100));
        let said = approve_from_prompt_at(
            &format!("approve {id}"),
            &one(&format!("/tmp/work/{session}/prompt")),
            &root,
            Ok(101),
        )
        .unwrap();
        assert!(said.contains("Approved once"), "{said}");
    }

    #[test]
    fn a_chat_reply_to_an_expired_request_reports_expiry() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        let said = approve_from_prompt_at(
            &format!("approve {id}"),
            &one("conversation-1"),
            &root,
            Ok(100 + TTL_SECONDS),
        )
        .unwrap();
        assert!(said.contains("unknown, expired, or consumed"), "{said}");
        assert!(!said.contains("not asked in this conversation"), "{said}");
        assert_ne!(
            request_id(&output(&input, &ask(), &root, 101 + TTL_SECONDS)),
            id
        );
    }

    #[test]
    fn grants_are_bound_to_command_directory_conversation_tool_and_rule() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let other = tempfile::tempdir().unwrap();
        let original = input(temp.path());
        let id = request_id(&output(&original, &ask(), &root, 100));
        Store::open(&root, 101).unwrap().approve(&id).unwrap();
        for (pointer, replacement) in [
            ("/tool_input/command", "git push fork main"),
            ("/cwd", other.path().to_str().unwrap()),
            ("/session_id", "conversation-2"),
            ("/tool_name", "terminal"),
        ] {
            let mut changed: Value = serde_json::from_str(&original).unwrap();
            *changed.pointer_mut(pointer).unwrap() = replacement.into();
            assert_ne!(
                request_id(&output(&changed.to_string(), &ask(), &root, 102)),
                id
            );
        }
        for rule in [
            Rule {
                pattern: "git *".into(),
                ..ask()
            },
            Rule {
                reason: "A different approval requirement.".into(),
                ..ask()
            },
        ] {
            assert_ne!(request_id(&output(&original, &rule, &root, 103)), id);
        }
        assert!(output(&original, &ask(), &root, 104).is_empty());
    }

    #[test]
    fn a_tool_workdir_is_part_of_the_scope() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let other = tempfile::tempdir().unwrap();
        let original = input(temp.path());
        let id = request_id(&output(&original, &ask(), &root, 100));
        Store::open(&root, 101).unwrap().approve(&id).unwrap();
        let mut changed: Value = serde_json::from_str(&original).unwrap();
        changed["tool_input"]["workdir"] = serde_json::json!(other.path());
        assert_ne!(
            request_id(&output(&changed.to_string(), &ask(), &root, 102)),
            id
        );
        assert!(output(&original, &ask(), &root, 103).is_empty());
    }

    #[test]
    fn expired_and_future_dated_requests_do_not_authorize() {
        for approval_time in [99, 100 + TTL_SECONDS] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("approvals");
            let input = input(temp.path());
            let id = request_id(&output(&input, &ask(), &root, 100));
            assert!(Store::open(&root, approval_time)
                .unwrap()
                .approve(&id)
                .is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        Store::open(&root, 101).unwrap().approve(&id).unwrap();
        assert_ne!(
            request_id(&output(&input, &ask(), &root, 100 + TTL_SECONDS)),
            id
        );
    }

    #[test]
    fn deny_verdicts_win_over_an_approved_request() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        Store::open(&root, 101).unwrap().approve(&id).unwrap();
        for pattern in ["ljos-policyd", "git push*"] {
            let deny = Rule {
                pattern: pattern.into(),
                verdict: "deny".into(),
                reason: "Denied.".into(),
            };
            assert_eq!(
                output(&input, &deny, &root, 102),
                hook_output_ruled(&hook_call(&input), "", Some(&deny))
            );
        }
        assert!(output(&input, &ask(), &root, 103).is_empty());
    }

    #[test]
    fn a_native_ask_does_not_create_a_local_grant() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let mut value: Value = serde_json::from_str(&input(temp.path())).unwrap();
        value.as_object_mut().unwrap().remove("turn_id");
        let input = value.to_string();
        let response: Value = serde_json::from_str(&output(&input, &ask(), &root, 100)).unwrap();
        assert_eq!(response["hookSpecificOutput"]["permissionDecision"], "ask");
        assert!(!root.exists());
    }

    #[test]
    fn a_grok_ask_is_the_permission_prompt_and_mints_no_request() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = serde_json::json!({
            "hookEventName": "pre_tool_use",
            "hook_event_name": "PreToolUse",
            "sessionId": "conversation-1",
            "cwd": temp.path(),
            "toolName": "run_terminal_command",
            "toolInput": {"command": "git push origin main"}
        })
        .to_string();
        let call = hook_call(&input);
        assert_eq!(call.shape, crate::HookShape::CamelCase);
        assert!(call.shape.asks());
        let response: Value = serde_json::from_str(&output(&input, &ask(), &root, 100)).unwrap();
        assert_eq!(response["decision"], "ask");
        assert_eq!(response["hookSpecificOutput"]["permissionDecision"], "ask");
        let reason = response["reason"].as_str().unwrap();
        assert!(!reason.contains("ljos approve"));
        assert!(!root.exists());
    }

    #[test]
    fn approval_does_not_depend_on_the_retry_turn_id() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        Store::open(&root, 101).unwrap().approve(&id).unwrap();
        let mut retry: Value = serde_json::from_str(&input).unwrap();
        retry["turn_id"] = "turn-2".into();
        assert!(output(&retry.to_string(), &ask(), &root, 102).is_empty());
    }

    #[test]
    fn missing_scope_and_corrupt_state_keep_the_command_blocked() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let original = input(temp.path());
        let mut value: Value = serde_json::from_str(&original).unwrap();
        value.as_object_mut().unwrap().remove("tool_name");
        let response: Value =
            serde_json::from_str(&output(&value.to_string(), &ask(), &root, 100)).unwrap();
        assert_eq!(response["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(!root.exists());
        request_id(&output(&original, &ask(), &root, 101));
        fs::write(root.join("requests.json"), "not json").unwrap();
        let response: Value = serde_json::from_str(&output(&original, &ask(), &root, 102)).unwrap();
        assert_eq!(response["hookSpecificOutput"]["permissionDecision"], "deny");
    }

    /// A relative `cwd`, and a payload with no `cwd`, each record one
    /// request for the directory they resolve to.
    #[test]
    fn a_relative_or_missing_cwd_records_the_resolved_directory() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        let sub = work.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let work = fs::canonicalize(&work).unwrap();
        let sub = fs::canonicalize(&sub).unwrap();
        let command = ": > map.tsv";
        let payload = |cwd: Option<&str>, roots: Option<Vec<&Path>>| {
            let mut value = serde_json::json!({
                "hook_event_name": "PreToolUse",
                "turn_id": "turn-1",
                "session_id": "conversation-1",
                "tool_name": "Bash",
                "tool_input": {"command": command}
            });
            if let Some(cwd) = cwd {
                value["cwd"] = cwd.into();
            }
            if let Some(roots) = roots {
                value["workspace_roots"] = roots
                    .iter()
                    .map(|path| Value::String(path.display().to_string()))
                    .collect();
            }
            value.to_string()
        };
        let recorded = |input: &str, dir: &Path| {
            let root = dir.join("approvals");
            request_id(&output(input, &ask(), &root, 100));
            let store = Store::open(&root, 101).unwrap();
            assert_eq!(store.requests.len(), 1);
            assert_eq!(store.requests[0].scope.command, command);
            assert_eq!(store.requests[0].scope.session, "conversation-1");
            store.requests[0].scope.cwd.clone()
        };
        let relative = payload(
            Some("sub"),
            Some(vec![Path::new("not/absolute"), work.as_path()]),
        );
        assert_eq!(
            recorded(&relative, &temp.path().join("relative")),
            sub,
            "relative cwd"
        );
        let missing = payload(None, Some(vec![work.as_path()]));
        assert_eq!(
            recorded(&missing, &temp.path().join("missing")),
            work,
            "missing cwd"
        );
        let here = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        let fallback = payload(None, None);
        assert_eq!(
            recorded(&fallback, &temp.path().join("fallback")),
            here,
            "missing cwd uses this process"
        );
    }

    #[test]
    fn a_duplicate_descriptor_does_not_retain_a_finished_transaction() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let store = Store::open(&root, 100).unwrap();
        let inherited = store.file.try_clone().unwrap();
        drop(store);
        let next = Store::open(&root, 101).unwrap();
        assert!(next.requests.is_empty());
        drop(inherited);
    }

    #[test]
    fn simultaneous_retries_consume_at_most_one_grant() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        Store::open(&root, 101).unwrap().approve(&id).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let root = root.clone();
                let input = input.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    output(&input, &ask(), &root, 102).is_empty()
                })
            })
            .collect();
        let allowed = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|allowed| *allowed)
            .count();
        assert_eq!(allowed, 1);
    }
    #[test]
    fn client_consent_requires_accept_and_a_true_boolean() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        use rmcp::model::{ElicitResult, ElicitationAction};
        for action in [
            ElicitationAction::Accept,
            ElicitationAction::Decline,
            ElicitationAction::Cancel,
        ] {
            for content in [
                None,
                Some(serde_json::json!({})),
                Some(serde_json::json!({"approve":false})),
                Some(serde_json::json!({"approve":"true"})),
                Some(serde_json::json!({"approve":1})),
                Some(serde_json::json!({"approve":true})),
            ] {
                let mut answer = ElicitResult::new(action.clone());
                answer.content = content.clone();
                assert_eq!(
                    consents(&answer),
                    action == ElicitationAction::Accept
                        && content == Some(serde_json::json!({"approve":true}))
                );
            }
        }
    }

    #[test]
    fn a_grant_matches_seat_and_directory_when_the_conversation_id_differs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        let seat = crate::seat_name();
        let cwd = fs::canonicalize(temp.path()).unwrap();
        let store = Store::open(&root, 101).unwrap();
        assert!(store.pending(&id, "not-this-conversation").is_err());
        store
            .pending_for(&id, "not-this-conversation", &seat, &cwd)
            .expect("the same seat, directory and request id grant");
        assert!(store
            .pending_for(&id, "not-this-conversation", "someone-else", &cwd)
            .is_err());
        let elsewhere = tempfile::tempdir().unwrap();
        assert!(store
            .pending_for(
                &id,
                "not-this-conversation",
                &seat,
                &fs::canonicalize(elsewhere.path()).unwrap(),
            )
            .is_err());
    }

    #[test]
    fn a_terminal_grants_even_when_a_runner_is_in_the_environment() {
        assert!(terminal_consent(true).is_ok());
        let err = terminal_consent(false).unwrap_err().to_string();
        assert!(err.contains("terminal"), "{err}");
    }

    #[test]
    fn a_confirmation_rechecks_conversation_contents_expiry_and_single_use() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let input = input(temp.path());
        let id = request_id(&output(&input, &ask(), &root, 100));
        let pending = {
            let store = Store::open(&root, 101).unwrap();
            assert!(store.pending(&id, "conversation-2").is_err());
            store.pending(&id, "conversation-1").unwrap()
        };
        // A dialog holds no lock, so another hook can inspect the same request.
        assert_eq!(request_id(&output(&input, &ask(), &root, 102)), id);
        for changed in ["command", "directory", "rule", "created"] {
            let mut snapshot = pending.clone();
            match changed {
                "command" => snapshot.scope.command.push_str(" --force"),
                "directory" => snapshot.scope.cwd = temp.path().join("elsewhere"),
                "rule" => snapshot.scope.reason.push_str(" changed"),
                "created" => snapshot.created += 1,
                _ => unreachable!(),
            }
            assert!(Store::open(&root, 103).unwrap().confirm(&snapshot).is_err());
        }
        Store::open(&root, 104).unwrap().confirm(&pending).unwrap();
        assert!(Store::open(&root, 105).unwrap().confirm(&pending).is_err());
        assert!(output(&input, &ask(), &root, 106).is_empty());
        assert!(Store::open(&root, 107).unwrap().confirm(&pending).is_err());

        let id = request_id(&output(&input, &ask(), &root, 200));
        let pending = Store::open(&root, 201)
            .unwrap()
            .pending(&id, "conversation-1")
            .unwrap();
        assert!(Store::open(&root, 200 + TTL_SECONDS)
            .unwrap()
            .confirm(&pending)
            .is_err());
        assert_ne!(
            request_id(&output(&input, &ask(), &root, 200 + TTL_SECONDS)),
            id
        );
    }

    /// An ask whose payload names no conversation still records one request
    /// for that command and directory. Chat text and the hook payload do
    /// not grant it. The consent form can, from any conversation, and the
    /// stored conversation stays empty. Any later caller with no
    /// conversation then runs that command once.
    #[test]
    fn an_ask_with_no_conversation_is_one_use_for_that_command() {
        let _guard_mode = crate::guard_mode::for_test(crate::guard_mode::GuardMode::Strict);

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("approvals");
        let cwd = temp.path();
        let command = "git push --force origin topic";
        let rule = Rule {
            pattern: "git-force-push".into(),
            verdict: "ask".into(),
            reason: crate::policyd_reason("git-force-push"),
        };
        let cursor = serde_json::json!({
            "conversation_id": "",
            "generation_id": "",
            "session_id": "",
            "transcript_path": null,
            "cursor_version": "1.0.0",
            "cwd": cwd,
            "hook_event_name": "preToolUse",
            "command": command,
            "tool_name": "Shell",
            "tool_input": {"command": command},
        })
        .to_string();
        let first = output(&cursor, &rule, &root, 100);
        let value: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(value["permission"], "deny", "{first}");
        let reason = value["user_message"].as_str().unwrap().to_string();
        assert!(
            !reason.contains("approval needs a conversation id"),
            "{reason}"
        );
        assert!(!reason.contains("leave it for them"), "{reason}");
        let tag = reason
            .split("ljos approve ")
            .nth(1)
            .and_then(|rest| rest.split('`').next())
            .unwrap()
            .to_string();
        assert!(
            reason.contains(&format!("ljos_request_approval MCP tool with id {tag}")),
            "{reason}"
        );
        let store = Store::open(&root, 101).unwrap();
        assert_eq!(store.requests.len(), 1);
        assert_eq!(store.requests[0].scope.session, "");
        assert_eq!(store.requests[0].scope.command, command);
        assert_eq!(store.requests[0].scope.cwd, fs::canonicalize(cwd).unwrap());
        assert!(!store.requests[0].approved);
        let full = store.requests[0].id.clone();
        drop(store);

        // A different runner, still with no conversation, is the same request.
        let codex = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "turn_id": "turn-other",
            "cwd": cwd,
            "tool_name": "Bash",
            "tool_input": {"command": command},
        })
        .to_string();
        let second = output(&codex, &rule, &root, 102);
        let second_v: Value = serde_json::from_str(&second).unwrap();
        let second_reason = second_v["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap();
        assert!(second_reason.contains(&tag), "{second_reason}");
        assert_eq!(Store::open(&root, 103).unwrap().requests.len(), 1);

        let chat = approve_from_prompt_at(
            &format!("approve {tag}"),
            &["conversation-1".into(), String::new()],
            &root,
            Ok(104),
        )
        .unwrap();
        assert!(chat.contains("not asked in this conversation"), "{chat}");
        assert!(!Store::open(&root, 105).unwrap().requests[0].approved);

        let mut hooked: Value = serde_json::from_str(&cursor).unwrap();
        hooked["tool_input"]["approve"] = Value::Bool(true);
        hooked["prompt"] = Value::String(format!("approve {full}"));
        let still = output(&hooked.to_string(), &rule, &root, 106);
        assert!(still.contains(&tag), "{still}");
        assert!(!Store::open(&root, 107).unwrap().requests[0].approved);

        let elsewhere = tempfile::tempdir().unwrap();
        let pending = Store::open(&root, 108)
            .unwrap()
            .pending_for(&tag, "conversation-9", "other-seat", elsewhere.path())
            .unwrap();
        assert_eq!(pending.scope.session, "");
        let said = Store::open(&root, 109).unwrap().confirm(&pending).unwrap();
        assert!(said.contains("Approved once"), "{said}");
        let granted = Store::open(&root, 110).unwrap();
        assert!(granted.requests[0].approved);
        assert_eq!(granted.requests[0].scope.session, "");
        drop(granted);

        let named = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "turn_id": "turn-1",
            "session_id": "conversation-1",
            "cwd": cwd,
            "tool_name": "Bash",
            "tool_input": {"command": command},
        })
        .to_string();
        let named_out = output(&named, &rule, &root, 111);
        assert!(named_out.contains("ljos approve "), "{named_out}");
        let named_store = Store::open(&root, 112).unwrap();
        assert!(named_store
            .requests
            .iter()
            .any(|request| request.scope.session.is_empty() && request.approved));
        assert!(named_store
            .requests
            .iter()
            .any(|request| { request.scope.session == "conversation-1" && !request.approved }));
        drop(named_store);

        let other_dir = tempfile::tempdir().unwrap();
        let mut other_cwd: Value = serde_json::from_str(&cursor).unwrap();
        other_cwd["cwd"] = other_dir.path().to_str().unwrap().into();
        let other_place = output(&other_cwd.to_string(), &rule, &root, 113);
        assert!(
            other_place.contains("\"permission\":\"deny\""),
            "{other_place}"
        );
        assert!(Store::open(&root, 114)
            .unwrap()
            .requests
            .iter()
            .any(|request| {
                request.scope.session.is_empty()
                    && request.approved
                    && request.scope.command == command
                    && request.scope.cwd == fs::canonicalize(cwd).unwrap()
            }));

        let mut other_cmd: Value = serde_json::from_str(&cursor).unwrap();
        other_cmd["command"] = "git push --force origin other".into();
        other_cmd["tool_input"]["command"] = "git push --force origin other".into();
        let other_line = output(&other_cmd.to_string(), &rule, &root, 115);
        assert!(
            other_line.contains("\"permission\":\"deny\""),
            "{other_line}"
        );
        assert!(Store::open(&root, 116)
            .unwrap()
            .requests
            .iter()
            .any(|request| {
                request.scope.session.is_empty()
                    && request.approved
                    && request.scope.command == command
            }));

        assert!(
            output(&codex, &rule, &root, 117).is_empty(),
            "the other unattributed caller did not spend the grant"
        );
        let again = output(&cursor, &rule, &root, 118);
        assert!(again.contains("\"permission\":\"deny\""), "{again}");
        let again_v: Value = serde_json::from_str(&again).unwrap();
        let again_tag = again_v["user_message"]
            .as_str()
            .unwrap()
            .split("ljos approve ")
            .nth(1)
            .unwrap()
            .split('`')
            .next()
            .unwrap();
        assert_ne!(again_tag, tag);
    }
}
