//! Remember requests and their writer-down queue path.

use super::*;

fn remember_queued_body(text: &str) -> Value {
    serde_json::json!({
        "queued": true,
        "kind": "lesson",
        "text": text,
    })
}

/// Whether the pack being down or slow is why `err` happened, and the
/// lesson can wait. A content refusal and packs turned off are not this.
pub(super) fn remember_should_queue(err: &anyhow::Error) -> bool {
    if pack_turned_off(err) {
        return false;
    }
    if let Some(write) = err
        .chain()
        .find_map(|cause| cause.downcast_ref::<PackWriteError>())
    {
        return write.queueable();
    }
    is_pack_busy(err) || is_deadline_miss(err) || writer_unreachable(err)
}

/// Which writer a remember goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RememberRoute {
    /// The person typed it at their own terminal ([`packset_write_as`]).
    Typed,
    /// An MCP remember, or words from the person's latest prompt
    /// ([`packset_write_mcp`]). The basis is decided when the call is made.
    Said,
    /// The agent's own words ([`packset_write_agent`]).
    Agent,
}

/// One remember, whole. A lesson queued because the pack did not answer
/// keeps this record, and the later send writes it from the record alone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RememberRequest {
    pub text: String,
    pub route: RememberRoute,
    /// Whose words they are. [`Basis::Person`] for a typed remember and
    /// [`Basis::Agent`] for an agent's.
    pub basis: Basis,
    pub persona: Option<String>,
    /// The horizon the writer is asked for: `Some(true)` transient,
    /// `Some(false)` standing, `None` the writer's default.
    pub horizon: Option<bool>,
    /// The tracker project `ljos remember -p` records.
    pub project: Option<String>,
    /// The `valid_from:` and `valid_to:` entities of the claim.
    pub validity: Vec<String>,
}

thread_local! {
    static WRITING_REMEMBER: std::cell::RefCell<Option<RememberRequest>> =
        const { std::cell::RefCell::new(None) };
}

impl RememberRequest {
    /// A remember the person typed at their own terminal.
    #[must_use]
    pub fn typed(text: &str, persona: Option<&str>, horizon: Option<bool>) -> Self {
        Self::new(text, RememberRoute::Typed, Basis::Person, persona, horizon)
    }

    /// An MCP remember, or the words of the person's latest prompt. The
    /// basis is read from the prompt now, not when a queued copy is sent.
    #[must_use]
    pub fn said(text: &str, persona: Option<&str>, horizon: Option<bool>) -> Self {
        Self::new(text, RememberRoute::Said, Basis::of(text), persona, horizon)
    }

    /// A remember in the agent's own words.
    #[must_use]
    pub fn agent(text: &str, persona: Option<&str>, horizon: Option<bool>) -> Self {
        Self::new(text, RememberRoute::Agent, Basis::Agent, persona, horizon)
    }

    /// The project and validity window are the ones this thread set with
    /// [`RememberProject`] and [`set_validity`]. The window is taken, so
    /// the next request does not carry it.
    fn new(
        text: &str,
        route: RememberRoute,
        basis: Basis,
        persona: Option<&str>,
        horizon: Option<bool>,
    ) -> Self {
        Self {
            text: text.trim().to_string(),
            route,
            basis,
            persona: persona
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
            horizon,
            project: lesson_project_entity(),
            validity: take_validity(),
        }
    }

    /// Post this remember through its writer.
    ///
    /// # Errors
    ///
    /// The writer's error.
    pub fn write(&self) -> Result<Value> {
        let _scope = self.enter();
        let persona = self.persona.as_deref();
        match self.route {
            RememberRoute::Typed => packset_write_as("Remember", &self.text, persona, self.horizon),
            RememberRoute::Said => {
                packset_write_mcp_as("Remember", &self.text, persona, self.horizon, self.basis)
            }
            RememberRoute::Agent => {
                packset_write_agent("Remember", &self.text, persona, self.horizon)
            }
        }
    }

    /// The atom this remember writes into `workspace`, before the writer
    /// stamps anything at post time.
    pub(super) fn atom(&self, workspace: &str) -> Result<Value> {
        let _scope = self.enter();
        let persona = self.persona.as_deref();
        match self.route {
            RememberRoute::Typed => {
                typed_atom("Remember", &self.text, workspace, persona, self.horizon)
            }
            RememberRoute::Said => mcp_claim_atom(
                "Remember",
                &self.text,
                workspace,
                persona,
                self.horizon,
                self.basis,
            ),
            RememberRoute::Agent => {
                agent_atom("Remember", &self.text, workspace, persona, self.horizon)
            }
        }
    }

    /// Set this request's project, validity window and record for the
    /// writes made before the guard drops. A lesson queued meanwhile
    /// keeps the record.
    pub(super) fn enter(&self) -> RememberScope {
        set_validity_tags(self.validity.clone());
        let previous = WRITING_REMEMBER.with(|cell| cell.replace(Some(self.clone())));
        RememberScope {
            _project: RememberProject::set(self.project.clone()),
            previous,
        }
    }

    /// The record for a queued copy whose text is `text`.
    pub(super) fn for_copy(&self, text: &str) -> Self {
        Self {
            text: text.trim().to_string(),
            ..self.clone()
        }
    }
}

#[must_use = "the request applies until this guard drops"]
pub(super) struct RememberScope {
    _project: RememberProject,
    previous: Option<RememberRequest>,
}

impl Drop for RememberScope {
    fn drop(&mut self) {
        // A write that stopped before building its atom leaves the window
        // set, and the next claim would carry it.
        let _ = take_validity();
        let previous = self.previous.take();
        WRITING_REMEMBER.with(|cell| cell.replace(previous));
    }
}

/// The remember this thread is writing, as the record for a queued copy
/// of `text`.
pub(super) fn writing_remember(text: &str) -> Option<RememberRequest> {
    WRITING_REMEMBER.with(|cell| cell.borrow().as_ref().map(|request| request.for_copy(text)))
}

/// What one remember did with its lesson.
#[derive(Debug)]
pub enum RememberOutcome {
    /// The pack took the lesson, or already held it. The body is its answer.
    Stored(Value),
    /// The pack did not answer and the lesson waits in the queue. The body
    /// is the queued lesson.
    Queued(Value),
    /// The pack did not answer and the queue did not take the lesson.
    /// Nothing was stored. The error is the queue's.
    NotQueued(anyhow::Error),
}

/// The one line for [`RememberOutcome::NotQueued`]: the queue's error,
/// then [`REMEMBER_NOT_STORED_LINE`].
#[must_use]
pub fn remember_not_queued_line(cause: &anyhow::Error) -> String {
    format!(
        "remember: the lesson could not be queued ({}). {REMEMBER_NOT_STORED_LINE}",
        flatten_line(&format!("{cause:#}"))
    )
}

/// Post one lesson. The client waits out one `Retry-After` on a 503.
/// A pack that reports busy is asked again inside [`search_busy_wait`].
/// When it stays busy, nothing is stored and the error is
/// [`REMEMBER_NOT_STORED_LINE`]. A pack that is not answering queues the
/// whole request and the outcome is [`RememberOutcome::Queued`]. When that
/// queue write fails, the outcome is [`RememberOutcome::NotQueued`].
/// Packs turned off, and a refusal of the text, stay errors.
///
/// # Errors
///
/// The pack stayed busy, refused the text, or packs are turned off.
pub fn remember_with_retry(request: &RememberRequest) -> Result<RememberOutcome> {
    remember_with_post(request, || request.write())
}

pub(super) fn remember_with_post(
    request: &RememberRequest,
    mut post: impl FnMut() -> Result<Value>,
) -> Result<RememberOutcome> {
    let text = request.text.as_str();
    let _call = SearchCall::enter();
    // A pack that has not opened yet is waited out on the search warm-up
    // budget. The short post budget starts only after that, so a pack that
    // is already open and busy is asked again inside the busy bound.
    answer_after_start(std::time::Instant::now(), || {
        let _retry = pack_link::RememberRetry::enter();
        let _deadline = pack_link::CommandDeadline::enter(REMEMBER_BUDGET);
        let _clock = pack_link::PackClock::enter(REMEMBER_BUDGET);
        let _quiet = pack_link::QuietPack::enter();
        // Do not start a writer from this call. Twenty-six of those at once
        // is what pushed the box into memory pressure.
        let _skip = SkipWriterEnsure::enter();
        match post() {
            Ok(body) => Ok(RememberOutcome::Stored(body)),
            Err(err) if existing_atom_id(&err).is_some() => {
                let id = existing_atom_id(&err).unwrap_or_default();
                say!("remember: already stored as {id}, so run `ljos search`.");
                Ok(RememberOutcome::Stored(serde_json::json!({
                    "id": id,
                    "kind": "lesson",
                    "text": text,
                    "duplicate": true,
                })))
            }
            Err(err) if is_reported_busy(&err) => Err(anyhow::Error::msg(REMEMBER_NOT_STORED_LINE)),
            Err(err) if is_memory_starting(&err) => Err(err),
            Err(err) if pack_turned_off(&err) => Err(err),
            Err(err) if remember_should_queue(&err) => {
                // A sandbox home does not queue a lesson nobody will send.
                // The line says why memory is off and which HOME to use.
                if let Some(line) = sandbox_memory_off() {
                    return Err(anyhow::Error::msg(line));
                }
                // Load has not judged the words. Keep the sentence as given,
                // even when a local prose check would turn it away.
                Ok(match queue_busy_lesson(request) {
                    Ok(_) => RememberOutcome::Queued(remember_queued_body(text)),
                    Err(cause) => RememberOutcome::NotQueued(cause),
                })
            }
            Err(err) => Err(err),
        }
    })
}

/// A typed 503, shed, or pressure error. A passed deadline is not load:
/// the caller already returned it.
pub(super) fn pack_under_load(err: &anyhow::Error) -> bool {
    if is_deadline_miss(err) {
        return false;
    }
    err.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<PackWriteError>(),
            Some(
                PackWriteError::BusyOrQueuedByServer(_)
                    | PackWriteError::MemoryPressure(_)
                    | PackWriteError::Unavailable503(_)
            )
        )
    }) || is_pack_busy(err)
        || is_write_shed(err)
        || is_read_shed(err)
}

/// Keep `text` for the next ljos that reaches the pack, and say so.
///
/// A busy or pressure answer has not judged the words. The copy is
/// queued. An empty text is not a lesson. A duplicate the pack already
/// named is not queued: [`remember_with_retry`] prints that atom id.
/// `1` when a busy writer queued the lesson, `2` when the queue refused it.
static BUSY_QUEUE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Queue `request` and set [`BUSY_QUEUE`]. The error is the queue write's.
fn queue_busy_lesson(request: &RememberRequest) -> Result<bool> {
    let queued = queue_lesson_under_load(request);
    let mark = if queued.is_ok() { 1 } else { 2 };
    BUSY_QUEUE.store(mark, std::sync::atomic::Ordering::Relaxed);
    queued
}

/// The line for a lesson the busy writer's queue did not take.
const BUSY_NOT_QUEUED_LINE: &str =
    "pack busy: the lesson was not queued. Retry later with `ljos remember` once the writer answers.";

/// The shed line, and that the lesson was queued, on one line.
/// A lesson that was queued is the one plain sentence. The fault is the
/// line when nothing was queued.
#[must_use]
pub fn shed_queue_line(fault: &str) -> String {
    match BUSY_QUEUE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => REMEMBER_QUEUED_LINE.to_string(),
        2 => format!("{fault}. {BUSY_NOT_QUEUED_LINE}"),
        _ => fault.to_string(),
    }
}

/// The one line for a lesson queued while the writer was busy.
/// `None` when this process did not queue one.
#[must_use]
pub fn busy_queue_line() -> Option<&'static str> {
    match BUSY_QUEUE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(REMEMBER_QUEUED_LINE),
        2 => Some(BUSY_NOT_QUEUED_LINE),
        _ => None,
    }
}

/// Queue `request` because the pack is under load. The local atom gate is
/// not applied: that gate is a guess, and a 503 has not refused the copy.
fn queue_lesson_under_load(request: &RememberRequest) -> Result<bool> {
    queue_unjudged_lesson("", &request.text, Some(request))
}

/// Queue one lesson the pack did not take. `issue` is empty for a
/// remember that names no ticket. `remember` is the whole request when a
/// remember queued it. The same row already waiting is not a second row.
pub(super) fn queue_unjudged_lesson(
    issue: &str,
    text: &str,
    remember: Option<&RememberRequest>,
) -> Result<bool> {
    let text = text.trim();
    if text.is_empty() {
        memory_port::keep_empty("lesson");
        return Err(policy_label::refused(
            "empty-input",
            "remember: empty text is not a claim",
            1,
        ));
    }
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    note_queue::push_in(
        &note_queue::state_dir(),
        note_queue::Queued {
            issue: issue.trim().to_string(),
            text: text.to_string(),
            workspace: memory_workspace(""),
            at,
            kind: "lesson".into(),
            supersedes: Vec::new(),
            seat: seat_name(),
            home: String::new(),
            project: remember
                .map_or_else(lesson_project_entity, |request| request.project.clone())
                .unwrap_or_default(),
            remember: remember.map(|request| request.for_copy(text)),
        },
    )
}
