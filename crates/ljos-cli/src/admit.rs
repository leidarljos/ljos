//! Origin-bound admission.
//!
//! An agent lesson is filed by posting the atom. A writer that admits by
//! origin answers 400 `held as proposal <id>` and keeps that proposal,
//! including the atom. `POST /v1/proposals/accept` with `{"workspace","id"}`
//! writes the atom and records the acceptance. The same text held again is
//! that proposal, so a retry does not leave another one.
//!
//! `POST /v1/proposals` is the miner. This path does not call it.
//!
//! A 400 that names `origin` as unknown, or a 404, means this writer does
//! not hold. The proposal stays in `$XDG_STATE_HOME/ljos/proposals.jsonl`
//! and `ljos accept` posts the atom. A writer that does not answer keeps
//! the proposal local for this call.
//!
//! `origin` on a live atom is `user-declared` (a person), `agent-derived`
//! (this seat's own agent), `tool-echo` (tool output), `external` or `peer`
//! (another seat). A 400 that names `origin` as unknown is retried once
//! without the field.
//!
//! Packset's memory levels are `explicit` and `derived`. `derived` is L0.
//! An agent's own lesson starts there. Tool output and another seat cannot
//! be stored above it until [`accept`] promotes the proposal to `explicit`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::PacksetClient;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ORIGIN_USER: &str = "user-declared";
pub const ORIGIN_AGENT: &str = "agent-derived";
pub const ORIGIN_TOOL: &str = "tool-echo";
pub const ORIGIN_PEER: &str = "peer";

/// Packset's lower memory level (`packset_core::record::LEVELS`). L0.
pub const LEVEL_L0: &str = "derived";
/// Packset's level for a claim a person stood behind.
pub const LEVEL_EXPLICIT: &str = "explicit";

/// The level an origin may be filed at.
///
/// A person's words are `explicit`. An agent's own lesson starts at L0.
/// Tool output and another seat are capped there until [`accept`].
#[must_use]
pub fn admission_level(origin: &str) -> &'static str {
    match origin {
        ORIGIN_USER => LEVEL_EXPLICIT,
        _ => LEVEL_L0,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filed {
    pub id: String,
    pub remote: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    id: String,
    status: String,
    remote: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_id: Option<String>,
    text: String,
    kind: String,
    /// `user-declared`, `agent-derived`, `tool-echo` or `peer`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    origin: String,
    atom: Value,
}

fn gate() -> std::sync::MutexGuard<'static, ()> {
    static GATE: Mutex<()> = Mutex::new(());
    GATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn origin_cache() -> &'static Mutex<BTreeMap<String, bool>> {
    static KEPT: std::sync::OnceLock<Mutex<BTreeMap<String, bool>>> = std::sync::OnceLock::new();
    KEPT.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn proposal_cache() -> &'static Mutex<BTreeMap<String, bool>> {
    static MODE: std::sync::OnceLock<Mutex<BTreeMap<String, bool>>> = std::sync::OnceLock::new();
    MODE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn proposals_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
        .join("proposals.jsonl")
}

fn http_timeout() -> std::time::Duration {
    std::env::var("PACKSET_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .map_or(
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis,
        )
}

pub fn stamp_origin(atom: &mut Value, origin: &str) {
    if let Some(map) = atom.as_object_mut() {
        map.insert("origin".into(), Value::String(origin.to_string()));
    }
}

/// Mark `atom` as the person's: origin `user-declared` and `source.basis`
/// `person`. Only a person-path writer calls this; the readers take the
/// basis, not the origin alone, as the person standing behind a claim.
pub fn stamp_person(atom: &mut Value) {
    stamp_origin(atom, ORIGIN_USER);
    atom["source"]["basis"] = Value::String(crate::Basis::Person.as_str().into());
}

fn strip_origin(atom: &mut Value) {
    if let Some(map) = atom.as_object_mut() {
        map.remove("origin");
    }
}

fn origin_rejected(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("origin")
        && (lower.contains("unknown")
            || lower.contains("unexpected")
            || lower.contains("not a field")
            || lower.contains("unrecognized"))
}

/// POST an atom. When this writer refuses `origin`, the field is dropped
/// and the post is tried once more. The answer is remembered per base URL.
pub fn post_kept(client: &PacksetClient, atom: &Value) -> Result<Value> {
    let base = client.base().to_string();
    let mut body = atom.clone();
    let known = origin_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&base)
        .copied();
    if known == Some(false) {
        strip_origin(&mut body);
    }
    match client.post_atom(&body) {
        Ok(v) => {
            if body.get("origin").is_some() {
                origin_cache()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(base, true);
            }
            Ok(v)
        }
        Err(e) => {
            let text = e.to_string();
            if body.get("origin").is_some() && origin_rejected(&text) {
                origin_cache()
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .insert(base, false);
                strip_origin(&mut body);
                return client.post_atom(&body).map_err(anyhow::Error::from);
            }
            Err(anyhow::Error::from(e))
        }
    }
}

fn read_latest() -> BTreeMap<String, Record> {
    let bytes = crate::read_state_file(&proposals_path()).unwrap_or_default();
    crate::complete_rows::<Record>(&bytes)
        .into_iter()
        .map(|rec| (rec.id.clone(), rec))
        .collect()
}

fn append(rec: &Record) -> Result<()> {
    let path = proposals_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(rec)?;
    line.push('\n');
    crate::append_locked_row(&path, line.as_bytes(), crate::Durability::Durable)
        .with_context(|| format!("admit: cannot write {}", path.display()))
}

fn proposal_is_local(base: &str) -> bool {
    proposal_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(base)
        .copied()
        == Some(false)
}

fn remember_proposal_mode(base: &str, remote: bool) {
    proposal_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(base.to_string(), remote);
}

/// The id in `held as proposal <id>`.
fn held_proposal_id(text: &str) -> Option<String> {
    let marker = "held as proposal ";
    let rest = text.get(text.find(marker)? + marker.len()..)?;
    let id: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    (id.len() == 32).then_some(id)
}

/// A writer that will not hold the atom, and did not store it.
fn filing_is_local(text: &str) -> bool {
    let lower = text.to_lowercase();
    origin_rejected(text)
        || lower.contains(": 404:")
        || lower.contains("status code 404")
        || (lower.contains("403") && lower.contains("not allowed"))
}

/// `Ok(None)` is a writer that does not hold. `Ok(Some(id))` is its proposal id.
fn post_direct(client: &PacksetClient, atom: &Value) -> Result<Option<String>> {
    let base = client.base();
    if proposal_is_local(base) {
        return Ok(None);
    }
    match client.post_atom(atom) {
        Ok(_) => {
            // This writer stored the atom. It does not hold proposals.
            remember_proposal_mode(base, false);
            Ok(None)
        }
        Err(e) => {
            let text = e.to_string();
            if let Some(id) = held_proposal_id(&text) {
                remember_proposal_mode(base, true);
                return Ok(Some(id));
            }
            if filing_is_local(&text) {
                remember_proposal_mode(base, false);
                return Ok(None);
            }
            // A writer that did not answer keeps this proposal local. The
            // next filing tries the writer again.
            if !text.contains("bad response") {
                return Ok(None);
            }
            bail!("propose: POST /v1/atoms failed: {text}");
        }
    }
}

/// Write `atom` as an agent's own memory, live at once.
///
/// It carries origin `agent-derived`. A writer from before agent writes
/// were live answers `held as proposal ID`; that proposal is accepted on
/// the spot, so the memory is live there too. Nothing waits for a person.
/// A lesson past the two-sentence cap or the character cap is posted as the
/// shorter copies [`crate::pack_copy::pack_copies`] cuts. A preference is
/// posted as written.
///
/// # Errors
///
/// An empty text, or the pack refusing the claim for a reason other than
/// holding it (a secret, the size cap, a third sentence, or the word
/// limit, grade or adverb ratio the pack holds an agent's claim to).
pub fn write_agent(client: &PacksetClient, atom: Value) -> Result<Value> {
    let text = atom["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        bail!("remember: empty text is not a claim");
    }
    if atom["kind"].as_str() == Some("lesson") && past_hard_cap(&text) {
        let copies = crate::pack_copy::pack_copies(&text);
        let unchanged = copies.is_empty() || (copies.len() == 1 && copies[0] == text);
        if !unchanged {
            let mut last = None;
            let mut first_err = None;
            for copy in &copies {
                let mut one = atom.clone();
                one["text"] = Value::String(copy.clone());
                match write_agent_one(client, one) {
                    Ok(posted) => last = Some(posted),
                    Err(e) if crate::note_can_wait(&e) => return Err(e),
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                    }
                }
            }
            if let Some(posted) = last {
                return Ok(posted);
            }
            return Err(
                first_err.unwrap_or_else(|| anyhow::anyhow!("remember: empty text is not a claim"))
            );
        }
    }
    write_agent_one(client, atom)
}

/// The two-sentence cap and the character cap. Word count, grade and
/// adverb ratio are the checks `prefer` stores with a warning.
fn past_hard_cap(text: &str) -> bool {
    packset_core::prose::sentences(text).len() > packset_core::prose::MAX_ATOM_SENTENCES
        || text.chars().count() > packset_core::record::TEXT_SOFT_CAP
}

/// One agent atom, with no further split.
///
/// Always origin `agent-derived` and `source.basis` `agent`, whatever the
/// prose. The pack keeps a person's long or hard sentence with a warning
/// and refuses an agent's; filing the agent's as the person's to get it
/// kept would hand it to every reader as the person's words.
pub(crate) fn write_agent_one(client: &PacksetClient, mut atom: Value) -> Result<Value> {
    let text = atom["text"].as_str().unwrap_or("").trim();
    if text.is_empty() {
        bail!("remember: empty text is not a claim");
    }
    stamp_origin(&mut atom, ORIGIN_AGENT);
    atom["source"]["basis"] = Value::String(crate::Basis::Agent.as_str().into());
    match post_kept(client, &atom) {
        Ok(posted) => Ok(posted),
        Err(e) => {
            let text = format!("{e:#}");
            let Some(id) = held_proposal_id(&text) else {
                return Err(e);
            };
            let workspace = atom
                .get("workspace")
                .and_then(Value::as_str)
                .filter(|w| !w.is_empty())
                .map_or_else(|| client.workspace(), str::to_string);
            let accepted = accept_on_writer(client, &workspace, &id)?;
            // Accept stores the atom as user-declared, and `ljos memories
            // list` then hides it. Put the origin back so the review list
            // the CLI reads is the store this write just landed in.
            relabel_agent(client, &workspace, accepted)
        }
    }
}

/// File `atom` as an agent proposal by posting it.
///
/// A held answer is the proposal. The id is stable for one kind and one
/// text, so the same lesson is one open proposal. An open, accepted or
/// satisfied row is returned as it stands. Origin is `agent-derived`.
pub fn propose_atom(client: &PacksetClient, atom: Value) -> Result<Filed> {
    file_proposal(client, atom, true)
}

/// File `atom` as a proposal and do not post it.
///
/// `ljos finish --lesson`, `ljos findings --remember` and the correction
/// hook use this. The claim stays out of the pack until [`accept`] promotes
/// it. A known origin on the atom is kept. The level is [`admission_level`]:
/// L0 (`derived`) for an agent, a tool echo and another seat, `explicit`
/// for the person's own words.
pub fn propose_admission(client: &PacksetClient, atom: Value) -> Result<Filed> {
    file_proposal(client, atom, false)
}

fn file_proposal(client: &PacksetClient, mut atom: Value, post: bool) -> Result<Filed> {
    let _g = gate();
    let text = atom["text"].as_str().unwrap_or("").trim().to_string();
    let kind = atom["kind"].as_str().unwrap_or("lesson").to_string();
    if text.is_empty() {
        bail!("propose: empty text is not a proposal");
    }
    if atom
        .get("workspace")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        if let Some(map) = atom.as_object_mut() {
            map.insert("workspace".into(), Value::String(client.workspace()));
        }
    }
    if post {
        stamp_origin(&mut atom, ORIGIN_AGENT);
    } else {
        let named = atom["origin"].as_str().unwrap_or("");
        if !packset_core::atom::known_origin(named) {
            stamp_origin(&mut atom, ORIGIN_AGENT);
        }
        let origin = atom["origin"].as_str().unwrap_or(ORIGIN_AGENT).to_string();
        if let Some(map) = atom.as_object_mut() {
            map.insert(
                "level".into(),
                Value::String(admission_level(&origin).to_string()),
            );
        }
    }
    let origin = atom["origin"].as_str().unwrap_or(ORIGIN_AGENT).to_string();
    let id = crate::work_id(&format!("{kind}\n{text}"));
    if let Some(existing) = read_latest().get(&id) {
        if matches!(existing.status.as_str(), "open" | "accepted" | "satisfied") {
            return Ok(Filed {
                id,
                remote: existing.remote,
            });
        }
    }
    let remote_id = if post {
        post_direct(client, &atom)?
    } else {
        None
    };
    let rec = Record {
        id: id.clone(),
        status: "open".into(),
        remote: remote_id.is_some(),
        remote_id,
        text,
        kind,
        origin,
        atom,
    };
    let remote = rec.remote;
    append(&rec)?;
    Ok(Filed { id, remote })
}

/// Raise a proposal's memory level from L0 to `explicit`.
fn promote_level(atom: &mut Value) {
    if let Some(map) = atom.as_object_mut() {
        map.insert("level".into(), Value::String(LEVEL_EXPLICIT.to_string()));
    }
}

/// The person's own text wrote the claim. An open proposal of those words
/// is satisfied, so a later accept does not post a second atom.
pub fn satisfy_text(text: &str) {
    let _g = gate();
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let open: Vec<Record> = read_latest()
        .into_values()
        .filter(|rec| rec.status == "open" && rec.text.trim() == text)
        .collect();
    for mut rec in open {
        rec.status = "satisfied".into();
        // A satisfied row that is not written leaves the proposal open, so a
        // later `ljos accept` reposts it. Name the log and the error, not the
        // proposal's text, which a row could carry a secret in.
        if let Err(e) = append(&rec) {
            eprintln!(
                "ljos: a satisfied proposal was not recorded in {}: {e:#}",
                proposals_path().display()
            );
        }
        close_remote(&rec);
    }
}

fn workspace_of(client: &PacksetClient, rec: &Record) -> String {
    let workspace = rec
        .atom
        .get("workspace")
        .and_then(Value::as_str)
        .unwrap_or("");
    if workspace.is_empty() {
        client.workspace()
    } else {
        workspace.to_string()
    }
}

/// Close a held proposal whose text is already a live claim. The writer's
/// duplicate rule keeps one atom when the text and kind match.
fn close_remote(rec: &Record) {
    if !rec.remote {
        return;
    }
    let Some(remote_id) = rec.remote_id.as_deref() else {
        return;
    };
    let Ok(client) = crate::pack() else {
        return;
    };
    let workspace = workspace_of(&client, rec);
    let _ = accept_on_writer(&client, &workspace, remote_id);
}

fn accepted_line(id: &str, kind: &str, posted: &Value) -> String {
    let origin = posted
        .get("origin")
        .and_then(Value::as_str)
        .filter(|origin| !origin.is_empty())
        .unwrap_or(ORIGIN_AGENT);
    format!(
        "accepted {id} as {kind} origin {origin}{}\n",
        crate::revision_note(posted)
    )
}

/// Accept rises the origin to `user-declared`. An agent write is reviewed
/// afterwards, so the live atom has to stay `agent-derived` or the default
/// memory list never shows it. One update, on an atom just stored.
fn relabel_agent(client: &PacksetClient, workspace: &str, accepted: Value) -> Result<Value> {
    let origin = accepted.get("origin").and_then(Value::as_str).unwrap_or("");
    if origin == ORIGIN_AGENT {
        return Ok(accepted);
    }
    let Some(id) = accepted
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    else {
        bail!(
            "accept stored the lesson without an id, so its origin could not be put back to agent-derived"
        );
    };
    let body = serde_json::json!({
        "workspace": workspace,
        "id": id,
        "fields": {"origin": ORIGIN_AGENT},
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    match client.roundtrip(
        "POST",
        "/v1/atoms/update",
        &[],
        Some(&bytes),
        true,
        http_timeout(),
    ) {
        Ok(answer) if (200..300).contains(&answer.status) => serde_json::from_str(&answer.body)
            .context("update: the pack's answer was not json"),
        Ok(answer) => bail!(
            "the lesson {id} was stored as {origin}; putting the origin back to agent-derived failed: {}: {}",
            answer.status,
            answer.body
        ),
        Err(e) => bail!(
            "the lesson {id} was stored as {origin}; putting the origin back to agent-derived failed: {e}"
        ),
    }
}

fn accept_on_writer(client: &PacksetClient, workspace: &str, id: &str) -> Result<Value> {
    let body = serde_json::json!({"workspace": workspace, "id": id});
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    match client.roundtrip(
        "POST",
        "/v1/proposals/accept",
        &[],
        Some(&bytes),
        true,
        http_timeout(),
    ) {
        Ok(answer) if (200..300).contains(&answer.status) => {
            serde_json::from_str(&answer.body).context("accept: the pack's answer was not json")
        }
        Ok(answer) => {
            bail!(
                "accept: POST /v1/proposals/accept failed: {}: {}",
                answer.status,
                answer.body
            )
        }
        Err(e) => bail!("accept: POST /v1/proposals/accept failed: {e}"),
    }
}

/// The full id of an open proposal a short id names; see
/// [`crate::resolve_short_id`].
///
/// # Errors
///
/// The short id starts more than one open proposal.
pub fn resolve_id(id: &str) -> Result<String> {
    let open: Vec<String> = read_latest()
        .into_values()
        .filter(|r| r.status == "open")
        .map(|r| r.id)
        .collect();
    crate::resolve_short_id("accept", id, &open)
}

/// The conversation that filed an open proposal: the holder its atom's
/// source names. `None` when the id is not on file or the atom names none.
#[must_use]
pub fn proposed_by(id: &str) -> Option<String> {
    let rec = read_latest().get(id.trim()).cloned()?;
    rec.atom["source"]["session"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Write one open proposal into the pack. A satisfied proposal was already
/// written by `remember` or `prefer`. An accepted one is already there.
pub fn accept(id: &str) -> Result<String> {
    let _g = gate();
    let id = id.trim();
    if id.is_empty() {
        bail!("accept: a proposal id is required");
    }
    let Some(mut rec) = read_latest().get(id).cloned() else {
        bail!("accept: no open proposal {id}");
    };
    match rec.status.as_str() {
        "satisfied" => {
            close_remote(&rec);
            return Ok(format!("{id} was already written by remember or prefer\n"));
        }
        "accepted" => return Ok(format!("{id} is already in the pack\n")),
        "open" => {}
        _ => bail!("accept: no open proposal {id}"),
    }
    let client = crate::pack()?;
    if rec.remote {
        let remote_id = rec.remote_id.clone().unwrap_or_else(|| rec.id.clone());
        let workspace = workspace_of(&client, &rec);
        let posted = accept_on_writer(&client, &workspace, &remote_id)?;
        rec.status = "accepted".into();
        append(&rec)?;
        return Ok(accepted_line(&rec.id, &rec.kind, &posted));
    }
    // A person accepting the proposal is the promotion off L0. Tool output
    // and another seat stay at `derived` until this line. The origin stays,
    // so a reader can still see where the claim came from.
    promote_level(&mut rec.atom);
    match post_kept(&client, &rec.atom) {
        Ok(posted) => {
            rec.status = "accepted".into();
            append(&rec)?;
            Ok(accepted_line(&rec.id, &rec.kind, &posted))
        }
        Err(e) => {
            let text = e.to_string();
            let Some(remote_id) = held_proposal_id(&text) else {
                return Err(e).with_context(|| format!("accept: POST /v1/atoms failed for {id}"));
            };
            // Filed while this writer was not holding, or the post is the
            // filing. Accept that proposal. Do not leave it open.
            let workspace = workspace_of(&client, &rec);
            let posted = accept_on_writer(&client, &workspace, &remote_id)?;
            rec.remote = true;
            rec.remote_id = Some(remote_id);
            rec.status = "accepted".into();
            append(&rec)?;
            Ok(accepted_line(&rec.id, &rec.kind, &posted))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn record(id: String) -> Record {
        Record {
            id,
            status: "proposed".into(),
            remote: false,
            remote_id: None,
            text: "use uv for python envs".into(),
            kind: "lesson".into(),
            origin: ORIGIN_AGENT.into(),
            atom: serde_json::json!({"text": "use uv for python envs"}),
        }
    }

    /// A held proposal carries the whole atom, so its row is private and
    /// written whole, like the other state logs.
    #[test]
    fn concurrent_proposal_rows_each_parse_and_stay_private() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", dir.path()) };
        const THREADS: usize = 16;
        const EACH: usize = 8;
        let handles: Vec<_> = (0..THREADS)
            .map(|thread| {
                std::thread::spawn(move || {
                    for n in 0..EACH {
                        append(&record(format!("{thread}-{n}"))).unwrap();
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("proposal writer");
        }
        let path = proposals_path();
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            serde_json::from_str::<Record>(line)
                .unwrap_or_else(|err| panic!("proposal row does not parse ({err}): {line}"));
        }
        assert_eq!(read_latest().len(), THREADS * EACH, "{text}");
        let mode = std::fs::symlink_metadata(&path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "proposals mode {mode:o}");
    }

    /// A proposal still being appended is not yet held; the proposals
    /// before it are.
    #[test]
    fn a_torn_last_proposal_leaves_the_others() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", dir.path()) };
        append(&record("a".into())).unwrap();
        let path = proposals_path();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{\"id\":\"b\",\"text\":\"caf\xc3");
        std::fs::write(&path, bytes).unwrap();
        let ids: Vec<String> = read_latest().into_keys().collect();
        assert_eq!(ids, ["a"]);
    }

    /// A proposals file swapped for a symlink is not written through.
    #[test]
    fn a_symlinked_proposals_file_is_refused() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", dir.path()) };
        let path = proposals_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let outside = dir.path().join("outside");
        std::fs::write(&outside, "kept\n").unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(append(&record("a".into())).is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "kept\n");
    }
}
