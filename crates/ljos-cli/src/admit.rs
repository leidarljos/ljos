//! Origin-bound admission.
//!
//! Every memory a seat writes is stored at once and labelled with whose it
//! is. `origin` on a live atom is `user-declared` (a person),
//! `agent-derived` (this seat's own agent), `tool-echo` (tool output),
//! `external` or `peer` (another seat), and `source.basis` is `person` or
//! `agent`. A 400 that names `origin` as unknown is retried once without
//! the field.
//!
//! Packset's memory levels are `explicit` and `derived`. `derived` is L0.
//! An agent's lesson from `ljos finish`, tool output and another seat are
//! stored there, and [`accept`] raises one to `explicit` and records the
//! review. Search reads neither label, so it finds the claim at once.
//!
//! A writer that holds agent writes answers 400 `held as proposal <id>`.
//! `POST /v1/proposals/accept` with `{"workspace","id"}` stores it there
//! and then, so nothing waits for a person.
//!
//! An older ljos kept such lessons as proposals in
//! `$XDG_STATE_HOME/ljos/proposals.jsonl` and stored one only on `ljos
//! accept`. [`import_open`] stores each open one once, labelled as it was
//! filed.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::quoted_yes::Yes;
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

/// The level an origin is stored at.
///
/// A person's words are `explicit`. An agent's lesson, tool output and
/// another seat start at L0 until [`accept`].
#[must_use]
pub fn admission_level(origin: &str) -> &'static str {
    match origin {
        ORIGIN_USER => LEVEL_EXPLICIT,
        _ => LEVEL_L0,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    id: String,
    /// `open`, `stored`, `refused`, `accepted` or `satisfied`.
    status: String,
    remote: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_id: Option<String>,
    /// The live atom this row was stored as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stored: Option<String>,
    text: String,
    kind: String,
    /// `user-declared`, `agent-derived`, `tool-echo` or `peer`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    origin: String,
    /// The person's words this acceptance rests on, where a seat carried
    /// them instead of the person typing in a terminal of their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grant: Option<Yes>,
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

fn proposals_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
        .join("proposals.jsonl")
}

/// The marker that says no proposal is left open to import.
fn imported_path() -> PathBuf {
    proposals_path().with_file_name("proposals-imported")
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

/// The id in `held as proposal <id>`.
fn held_proposal_id(text: &str) -> Option<String> {
    let marker = "held as proposal ";
    let rest = text.get(text.find(marker)? + marker.len()..)?;
    let id: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    (id.len() == 32).then_some(id)
}

/// Write `atom` as an agent's own memory, live at once, with origin
/// `agent-derived`. Nothing waits for a person. A text the pack's prose
/// gate would turn away is stored as shorter copies ([`write_fitted`]).
///
/// # Errors
///
/// An empty text, or the pack refusing the claim for a reason shorter
/// copies do not answer: a secret, an instruction to the model, an unknown
/// kind.
pub fn write_agent(client: &PacksetClient, atom: Value) -> Result<Value> {
    write_fitted(client, atom, true, write_agent_one)
}

/// Post `atom` with `post`: as written when the pack's prose gate takes it,
/// else as the copies [`crate::pack_copy::pack_copies`] cuts, each with the
/// same labels. The pack holds an agent's and a hook's claim (`held`) to
/// its sentence, grade and adverb limits and keeps any other with a
/// warning; here a limit is a warning, not a refusal. The answer is the
/// last copy's, with the gate's reason as `prose.warning` and the number
/// of copies as `copies`, so the caller says it in one line. A `note` is
/// not held to the gate.
fn write_fitted(
    client: &PacksetClient,
    atom: Value,
    held: bool,
    post: fn(&PacksetClient, Value) -> Result<Value>,
) -> Result<Value> {
    let text = atom["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        bail!("remember: empty text is not a claim");
    }
    let refused = (held && atom["kind"].as_str() != Some(packset_core::record::NOTE_KIND))
        .then(|| crate::pack_copy::gate_refusal(&text))
        .flatten();
    let Some(why) = refused else {
        return post(client, atom);
    };
    let copies = crate::pack_copy::pack_copies(&text);
    if copies.is_empty() {
        return post(client, atom);
    }
    let mut last = None;
    let mut first_err = None;
    for copy in &copies {
        let mut one = atom.clone();
        one["text"] = Value::String(copy.clone());
        match post(client, one) {
            Ok(posted) => last = Some(posted),
            Err(e) if crate::note_can_wait(&e) => return Err(e),
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    let Some(mut posted) = last else {
        return Err(
            first_err.unwrap_or_else(|| anyhow::anyhow!("remember: empty text is not a claim"))
        );
    };
    if posted.is_object() {
        posted["prose"]["warning"] = Value::String(why);
        posted["copies"] = Value::from(copies.len());
    }
    Ok(posted)
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
    post_live(client, atom)
}

/// Post `atom` as it is labelled. A writer that holds the write answers
/// `held as proposal ID`; that proposal is accepted on the spot, so the
/// claim is live there too, and the origin `atom` carries is put back.
fn post_live(client: &PacksetClient, atom: Value) -> Result<Value> {
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
            let origin = atom["origin"].as_str().unwrap_or(ORIGIN_AGENT);
            relabel(client, &workspace, accepted, origin)
        }
    }
}

/// Store `atom` at once, as its labels say: a known origin is kept, any
/// other is `agent-derived`, a claim that is not the person's has
/// `source.basis` `agent`, and the level is [`admission_level`]. `ljos
/// finish --lesson`, `ljos findings --remember` and the correction hook
/// write this way. Search finds the claim at once; [`accept`] raises it.
///
/// # Errors
///
/// An empty text, or the pack refusing the claim for a reason shorter
/// copies do not answer.
pub fn store_labelled(client: &PacksetClient, mut atom: Value) -> Result<Value> {
    if atom["text"].as_str().unwrap_or("").trim().is_empty() {
        bail!("store: empty text is not a claim");
    }
    if atom
        .get("workspace")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        atom["workspace"] = Value::String(client.workspace());
    }
    if !packset_core::atom::known_origin(atom["origin"].as_str().unwrap_or("")) {
        stamp_origin(&mut atom, ORIGIN_AGENT);
    }
    let origin = atom["origin"].as_str().unwrap_or(ORIGIN_AGENT).to_string();
    if origin != ORIGIN_USER && atom["source"]["basis"].as_str().is_none() {
        atom["source"]["basis"] = Value::String(crate::Basis::Agent.as_str().into());
    }
    atom["level"] = Value::String(admission_level(&origin).to_string());
    // The pack's `validate` asks with the hold on: its prose gate is for
    // what an agent or a hook writes, whether the writer holds or not.
    let held = packset_core::atom::needs_accept(&origin, atom["hook"] == Value::Bool(true), true);
    write_fitted(client, atom, held, post_live)
}

/// How long one hook call spends storing the proposals an older ljos left
/// open. The prompt's search after it must keep its fused pool, which
/// waits up to [`crate::HOOK_RERANK_BUDGET_MS`], or recall is lost. The
/// rest of [`crate::HOOK_WALL_BUDGET_MS`] is split evenly with the
/// cross-encoder stage. Each can be cut without harm: the rows left go
/// on the next call, and a cut rerank answers with the fused pool.
pub const IMPORT_SLICE_MS: u64 = (crate::HOOK_WALL_BUDGET_MS - crate::HOOK_RERANK_BUDGET_MS) / 2;

/// What one [`import_open`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Imported {
    /// Rows stored as live memories.
    pub stored: usize,
    /// Rows still open for a later call.
    pub left: usize,
}

/// Store each proposal an older ljos left open, once, as the labelled
/// memory its row holds ([`store_labelled`]), and mark the row `stored`
/// with the atom's id. A row the pack turns away is marked `refused` and
/// keeps its atom, so `ljos accept` can try it again; no row is dropped.
/// No post starts once `budget_ms` is spent, and a pack that does not
/// answer ends the call; the rows left stay open for the next one. Once
/// none is left, a marker beside the file spares later calls the read.
pub fn import_open(budget_ms: u64) -> Imported {
    let marker = imported_path();
    if marker.exists() || !proposals_path().exists() {
        return Imported::default();
    }
    let _g = gate();
    let open: Vec<Record> = read_latest()
        .into_values()
        .filter(|rec| rec.status == "open")
        .collect();
    let started = std::time::Instant::now();
    let budget = std::time::Duration::from_millis(budget_ms);
    let mut client = crate::pack().ok();
    let mut out = Imported::default();
    for mut rec in open {
        let Some(live) = client.as_ref().filter(|_| started.elapsed() < budget) else {
            out.left += 1;
            continue;
        };
        let stored = if rec.remote {
            import_held(live, &rec)
        } else {
            store_labelled(live, rec.atom.clone())
        };
        match stored {
            Ok(posted) => {
                rec.status = "stored".into();
                rec.stored = posted["id"].as_str().map(str::to_string);
            }
            Err(e) if crate::note_can_wait(&e) => {
                client = None;
                out.left += 1;
                continue;
            }
            Err(_) => rec.status = "refused".into(),
        }
        match append(&rec) {
            Ok(()) if rec.status == "stored" => out.stored += 1,
            Ok(()) => {}
            Err(_) => out.left += 1,
        }
    }
    if out.left == 0 {
        let _ = std::fs::write(&marker, format!("{}\n", crate::now_utc()));
    }
    out
}

/// A row whose atom a writer holds is accepted there, then given back the
/// origin and level it was filed under. A writer that no longer holds it
/// is posted the row's atom.
fn import_held(client: &PacksetClient, rec: &Record) -> Result<Value> {
    let workspace = workspace_of(client, rec);
    let remote_id = rec.remote_id.as_deref().unwrap_or(&rec.id);
    let accepted = match accept_on_writer(client, &workspace, remote_id) {
        Ok(accepted) => accepted,
        Err(e) if crate::note_can_wait(&e) => return Err(e),
        Err(_) => return store_labelled(client, rec.atom.clone()),
    };
    let origin = if rec.origin.is_empty() {
        ORIGIN_AGENT
    } else {
        rec.origin.as_str()
    };
    let Some(id) = accepted["id"].as_str().filter(|id| !id.is_empty()) else {
        bail!("import: the writer stored {remote_id} without an id");
    };
    update_fields(
        client,
        &workspace,
        id,
        serde_json::json!({"origin": origin, "level": admission_level(origin)}),
    )
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

fn accepted_line(rec: &Record, posted: &Value) -> String {
    let origin = posted
        .get("origin")
        .and_then(Value::as_str)
        .filter(|origin| !origin.is_empty())
        .unwrap_or(ORIGIN_AGENT);
    format!(
        "accepted {} as {} origin {origin}{}\n{}",
        rec.id,
        rec.kind,
        crate::revision_note(posted),
        rec.grant.as_ref().map(Yes::line).unwrap_or_default()
    )
}

/// Accept on a writer that held the write stores it as `user-declared`.
/// Put back the origin it was written with, or the memory list and every
/// reader that goes by origin would take it for the person's. One update,
/// on an atom just stored.
fn relabel(
    client: &PacksetClient,
    workspace: &str,
    accepted: Value,
    origin: &str,
) -> Result<Value> {
    let stored = accepted.get("origin").and_then(Value::as_str).unwrap_or("");
    if stored == origin {
        return Ok(accepted);
    }
    let Some(id) = accepted
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    else {
        bail!("accept stored the claim without an id, so its origin could not be put back to {origin}");
    };
    update_fields(client, workspace, id, serde_json::json!({"origin": origin})).with_context(|| {
        format!("the claim {id} was stored as {stored}; putting the origin back to {origin} failed")
    })
}

/// `POST /v1/atoms/update` of `fields` on atom `id`. The pack's answer.
fn update_fields(
    client: &PacksetClient,
    workspace: &str,
    id: &str,
    fields: Value,
) -> Result<Value> {
    let body = serde_json::json!({"workspace": workspace, "id": id, "fields": fields});
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    match client.roundtrip(
        "POST",
        "/v1/atoms/update",
        &[],
        Some(&bytes),
        true,
        http_timeout(),
    ) {
        Ok(answer) if (200..300).contains(&answer.status) => {
            serde_json::from_str(&answer.body).context("update: the pack's answer was not json")
        }
        Ok(answer) => bail!("update: {}: {}", answer.status, answer.body),
        Err(e) => bail!("update: {e}"),
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

/// Every acceptance that rests on a person's words a seat carried, by
/// proposal id. An audit reads these to see which lessons went in on a
/// quoted yes rather than from a terminal of the person's own.
#[must_use]
pub fn granted() -> Vec<(String, Yes)> {
    let mut out: Vec<(String, Yes)> = read_latest()
        .into_values()
        .filter(|rec| rec.status == "accepted")
        .filter_map(|rec| rec.grant.map(|yes| (rec.id, yes)))
        .collect();
    out.sort_by(|a, b| a.1.at.cmp(&b.1.at).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Accept a stored memory, or a proposal an older ljos filed. See
/// [`accept_on`].
pub fn accept(id: &str) -> Result<String> {
    accept_on(id, None)
}

/// [`accept`], recording the person's words the acceptance rests on.
///
/// `id` names a live memory, or a proposal row: an open one is stored
/// now at `explicit`, a stored one is accepted as the memory it became.
/// A satisfied or accepted row is already in the pack.
///
/// `yes` is a yes a seat carried from chat, see [`crate::quoted_yes`]. It
/// is kept on the accepted row, beside the acceptance it stands for, and
/// said back on the line the caller prints. `None` is the terminal path,
/// where the person ran the accept themselves.
///
/// # Errors
///
/// No proposal or memory has that id, a short id starts several, or the
/// pack or the state directory refuses the write.
pub fn accept_on(id: &str, yes: Option<&crate::quoted_yes::Yes>) -> Result<String> {
    let _g = gate();
    let id = id.trim();
    if id.is_empty() {
        bail!("accept: an id is required");
    }
    let rows = read_latest();
    let pending: Vec<String> = rows
        .values()
        .filter(|r| matches!(r.status.as_str(), "open" | "stored" | "refused"))
        .map(|r| r.id.clone())
        .collect();
    let id = crate::resolve_short_id("accept", id, &pending)?;
    let id = id.as_str();
    let Some(mut rec) = rows.get(id).cloned() else {
        return accept_memory(id, None, yes);
    };
    rec.grant = yes.cloned();
    match rec.status.as_str() {
        "satisfied" => {
            close_remote(&rec);
            return Ok(format!("{id} was already written by remember or prefer\n"));
        }
        "accepted" => return Ok(format!("{id} is already in the pack\n")),
        "stored" => {
            let stored = rec.stored.clone().unwrap_or_default();
            return accept_memory(&stored, Some(rec), yes);
        }
        "open" | "refused" => {}
        _ => bail!("accept: no open proposal {id}"),
    }
    let client = crate::pack()?;
    if rec.remote {
        let remote_id = rec.remote_id.clone().unwrap_or_else(|| rec.id.clone());
        let workspace = workspace_of(&client, &rec);
        let posted = accept_on_writer(&client, &workspace, &remote_id)?;
        rec.status = "accepted".into();
        append(&rec)?;
        return Ok(accepted_line(&rec, &posted));
    }
    // A person accepting the proposal is the promotion off L0. The origin
    // stays, so a reader can still see where the claim came from.
    promote_level(&mut rec.atom);
    match post_kept(&client, &rec.atom) {
        Ok(posted) => {
            rec.status = "accepted".into();
            rec.stored = posted["id"].as_str().map(str::to_string);
            append(&rec)?;
            Ok(accepted_line(&rec, &posted))
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
            Ok(accepted_line(&rec, &posted))
        }
    }
}

/// Accept live memory `id`, a whole id or the start of one: raise its level
/// to `explicit`, record a `keep` verdict for `ljos memories review`, and
/// keep the acceptance and the yes it rests on as a row. `rec` is the
/// proposal row an import stored it from.
fn accept_memory(id: &str, rec: Option<Record>, yes: Option<&Yes>) -> Result<String> {
    let client = crate::pack()?;
    let workspace = client.workspace();
    let atoms = crate::atoms_lean(&client, &workspace).context("accept: GET /v1/atoms failed")?;
    let all: Vec<crate::memories::Memory> = atoms
        .iter()
        .filter_map(crate::memories::Memory::of)
        .collect();
    let full = crate::memories::resolve(id, &all)
        .with_context(|| format!("accept: no open proposal or memory {id}"))?;
    let atom = atoms
        .iter()
        .find(|a| a["id"].as_str() == Some(full.as_str()))
        .cloned()
        .unwrap_or_default();
    let posted = if atom["level"].as_str() == Some(LEVEL_L0) {
        update_fields(
            &client,
            &workspace,
            &full,
            serde_json::json!({"level": LEVEL_EXPLICIT}),
        )?
    } else {
        atom.clone()
    };
    crate::memories::record(&full, "keep", None)?;
    let mut rec = rec.unwrap_or_else(|| Record {
        id: full.clone(),
        status: String::new(),
        remote: false,
        remote_id: None,
        stored: None,
        text: atom["text"].as_str().unwrap_or("").to_string(),
        kind: atom["kind"].as_str().unwrap_or("lesson").to_string(),
        origin: atom["origin"].as_str().unwrap_or("").to_string(),
        grant: None,
        atom,
    });
    rec.status = "accepted".into();
    rec.stored = Some(full);
    rec.grant = yes.cloned();
    append(&rec)?;
    Ok(accepted_line(&rec, &posted))
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
            stored: None,
            text: "use uv for python envs".into(),
            kind: "lesson".into(),
            origin: ORIGIN_AGENT.into(),
            grant: None,
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
