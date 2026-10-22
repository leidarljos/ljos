//! Preferences stored before a basis was stamped.
//!
//! poisonread made [`crate::person_stored`] require `source.basis` `person`.
//! A preference written before that has no basis, so it stopped showing.
//! The old store cannot tell a person's preference from an agent's soft
//! prose, so only a mark an old person-path writer actually left is
//! promoted. Anything else stays unverified until the person keeps it.
//!
//! The writers at v0.32.1, and the same writers through d47b69bd, left
//! these marks:
//!
//! * The prompt hook (`capture::save` on `UserPromptSubmit`) stamps entity
//!   `via:prompt` and origin `user-declared`, and no basis. It runs under
//!   the runner, so `source.session` may name that runner's holder.
//! * A terminal `ljos prefer` or `ljos remember` (`post_claim_horizon`)
//!   stamps origin `user-declared`, `source.via` `ljos`, and no basis. With
//!   no runner ancestor and no runner session id, `source.harness`,
//!   `source.seat` and `source.session` are the same word and `source.turn`
//!   is absent. `seat_from_tree` is none in that case, and no
//!   `*_SESSION_ID`, `*_THREAD_ID` or `*_CONVERSATION_ID` was recorded.
//! * A runner's `ljos prefer` records the runner: `source.session` contains
//!   `@` (the holder `holder_for_session` builds), or `source.turn` is set
//!   from a `*_TURN_ID`, or `source.harness` names the ancestor
//!   `seat_from_tree` or `seat_from_runner_env` found. MCP before a basis
//!   existed used the same writer under the runner, so the session id is
//!   its mark. `source.via` stayed `ljos`.
//! * MCP from the basis writer on stamps `source.basis` `person` or
//!   `agent`. A basis is already a decision and is left alone.
//! * An agent write and a finding stamp origin `agent-derived` or
//!   `tool-echo`. Session-end extraction stamps `via:session-end` and
//!   `session:`. None of those read as the person before, and they are
//!   not promoted.
//!
//! packset refuses an update that raises `source.basis` from none to
//! `person`, and accept takes only a write the pack itself held as a
//! proposal. A promoted atom is raised by storing a copy as the person's
//! and retiring the original. The copy keeps the text, kind, entities,
//! `ts` and `valid_from`, so a rule that cites the text and a lesson hash
//! read it as the same claim. Its id is new.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::admit::ORIGIN_USER;
use crate::{atoms_lean, pack, under_a_runner, SAID_ENTITY};

/// What a legacy atom is. `None` from [`legacy`] means it is not one: it
/// has a basis, it is not a preference or a lesson, or its origin was
/// never the person's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Legacy {
    /// Unambiguous person-path evidence. The upgrade stamps basis `person`.
    Promote,
    /// No such evidence. It stays unverified.
    Hold,
}

/// Whether `atom` is a preference or a lesson stored before a basis, and
/// which way the old writer's marks fall.
#[must_use]
pub(crate) fn legacy(atom: &Value) -> Option<Legacy> {
    if atom.get("tombstone") == Some(&Value::Bool(true)) {
        return None;
    }
    let kind = atom.get("kind").and_then(Value::as_str).unwrap_or("");
    if kind != "preference" && kind != "lesson" {
        return None;
    }
    if basis_of(atom).is_some() {
        return None;
    }
    let origin = atom
        .get("origin")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|o| !o.is_empty());
    // An origin other than the person's never read as theirs, basis or
    // not. `tool-echo`, `agent-derived`, `peer` and `external` stay out.
    if origin.is_some_and(|o| !o.eq_ignore_ascii_case(ORIGIN_USER)) {
        return None;
    }
    // A missing origin is the pack's default, not a writer saying the
    // person stood behind it. It waits for review.
    let persons_origin = origin.is_some_and(|o| o.eq_ignore_ascii_case(ORIGIN_USER));
    if persons_origin && (prompt_hook(atom) || terminal_write(atom)) {
        Some(Legacy::Promote)
    } else {
        Some(Legacy::Hold)
    }
}

/// Preferences with no basis that the upgrade did not promote.
#[must_use]
pub(crate) fn awaiting(atoms: &[Value]) -> Vec<(String, String)> {
    atoms
        .iter()
        .filter(|a| legacy(a) == Some(Legacy::Hold))
        .filter(|a| a.get("kind").and_then(Value::as_str) == Some("preference"))
        .filter_map(|a| {
            let id = a.get("id").and_then(Value::as_str)?.trim();
            let text = a.get("text").and_then(Value::as_str)?.trim();
            if id.is_empty() || text.is_empty() {
                None
            } else {
                Some((id.to_string(), text.to_string()))
            }
        })
        .collect()
}

/// The sentence session start, the doctor and `ljos prefer --review` share.
#[must_use]
pub(crate) fn review_sentence(n: usize) -> String {
    let (noun, verb) = if n == 1 {
        ("preference", "awaits")
    } else {
        ("preferences", "await")
    };
    format!("{n} older {noun} {verb} review (`ljos prefer --review`)")
}

/// Stamp basis `person` on every legacy atom the old person-path writers
/// marked. Recorded per pack, so a later run does nothing. The pack is
/// the one this process is configured for; nothing is fetched elsewhere.
/// A pack that does not answer is left for the next run. One the pack
/// refuses is written on the record and said once, by the run that wrote
/// it, and is not sent again. A hook leaves it to the next run outside one.
pub fn migrate_once() {
    // A raise is several requests, and the hook's cap can stop it between
    // the copy and the retirement.
    if std::env::var_os("LJOS_IN_HOOK").is_some() {
        return;
    }
    // A bounded pack read already holds the environment lock, then
    // searches, and the search calls this. The marker path takes that
    // same lock. The environment lock has to come first: taking the
    // gate first lets one search hold the gate while it waits for the
    // lock the other search still holds.
    crate::with_env_mutation(|| {
        let _guard = gate().lock().unwrap_or_else(|e| e.into_inner());
        if recorded() {
            return;
        }
        match finish_retired().and_then(|()| stamp_marked()) {
            Ok(refused) if refused.is_empty() => {
                let _ = record("once\n");
            }
            Ok(refused) => {
                let list = refused.join("\n");
                if record(&format!("refused\n{list}\n")).unwrap_or(false) {
                    let path = marker_path().map(|p| p.display().to_string());
                    eprintln!(
                        "ljos: the pack refused {} older {} as the person's, and the \
                         upgrade will not try again; remove {} to retry:\n{list}",
                        refused.len(),
                        if refused.len() == 1 {
                            "preference"
                        } else {
                            "preferences"
                        },
                        path.unwrap_or_default(),
                    );
                }
            }
            Err(_) => {}
        }
    });
}

/// List the preferences the upgrade held back.
///
/// # Errors
///
/// The pack did not answer.
pub fn list() -> Result<String> {
    migrate_once();
    let rows = awaiting(&live()?);
    if rows.is_empty() {
        return Ok("no older preferences await review\n".to_string());
    }
    let mut out = format!("{}\n", review_sentence(rows.len()));
    for (id, text) in &rows {
        let short = id.get(..8).unwrap_or(id);
        out.push_str(&format!(
            "{short}  {text}\n  ljos prefer --review --keep {id}\n"
        ));
    }
    Ok(out)
}

/// Keep one held-back preference as the person's.
///
/// A runner is refused, and so is a pipe. The person types this at a
/// terminal of their own.
///
/// # Errors
///
/// The id is not a held-back preference, this process is under a runner
/// or not a terminal, or the pack refused the person's copy.
pub fn keep(id: &str) -> Result<String> {
    if under_a_runner() {
        bail!(
            "prefer: a runner cannot keep a preference. \
             Run `ljos prefer --review --keep {id}` from a terminal of your own"
        );
    }
    if !std::io::stdin().is_terminal() {
        bail!(
            "prefer: keeping a preference needs a terminal. \
             Run `ljos prefer --review --keep {id}` from a terminal of your own"
        );
    }
    migrate_once();
    let atoms = live()?;
    let rows = awaiting(&atoms);
    let journaled = retired_unstored(Legacy::Hold);
    let ids: Vec<String> = rows
        .iter()
        .map(|(id, _)| id.clone())
        .chain(
            journaled
                .iter()
                .filter_map(|a| id_of(a).map(str::to_string)),
        )
        .collect();
    let id = crate::resolve_short_id("prefer", id.trim(), &ids)?;
    let Some(atom) = atoms
        .iter()
        .chain(&journaled)
        .find(|a| a.get("id").and_then(Value::as_str) == Some(id.as_str()))
    else {
        bail!("prefer: {id} is not an older preference awaiting review");
    };
    if legacy(atom) != Some(Legacy::Hold)
        || atom.get("kind").and_then(Value::as_str) != Some("preference")
    {
        bail!("prefer: {id} is not an older preference awaiting review");
    }
    let client = pack()?;
    let stored = finish(&client, atom).with_context(|| {
        format!("prefer: could not keep {id}; run `ljos prefer --review --keep {id}` again")
    })?;
    Ok(format!(
        "kept {id} as the person's preference, now {stored}\n"
    ))
}

fn prompt_hook(atom: &Value) -> bool {
    entities(atom).iter().any(|e| e == SAID_ENTITY) && !agent_mark(atom)
}

/// The shape `post_claim_horizon` left when no runner ancestor and no
/// runner session id were recorded.
fn terminal_write(atom: &Value) -> bool {
    let source = match atom.get("source").and_then(Value::as_object) {
        Some(s) => s,
        None => return false,
    };
    let via = source
        .get("via")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if via != "ljos" {
        return false;
    }
    let seat = field(source, "seat");
    let session = field(source, "session");
    let harness = field(source, "harness");
    if seat.is_empty() || session != seat || harness != seat || session.contains('@') {
        return false;
    }
    if !field(source, "turn").is_empty() {
        return false;
    }
    !agent_mark(atom)
}

/// A mark an agent path left on the atom: a session-end tag, or a
/// `session:` entity naming the runner's conversation.
fn agent_mark(atom: &Value) -> bool {
    entities(atom)
        .iter()
        .any(|e| e == "via:session-end" || e.starts_with("session:"))
}

fn field<'a>(source: &'a serde_json::Map<String, Value>, key: &str) -> &'a str {
    source.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

/// How long one request may take. A hook is killed at 10s, so the listing
/// and each request of a raise stop at the hook's bound.
/// `PACKSET_TIMEOUT_MS` still shortens it.
fn http_timeout() -> std::time::Duration {
    let from_env = std::env::var("PACKSET_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .map_or(
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis,
        );
    from_env.min(crate::atoms_timeout())
}

fn entities(atom: &Value) -> Vec<String> {
    atom.get("entities")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn basis_of(atom: &Value) -> Option<&str> {
    atom.pointer("/source/basis")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|b| !b.is_empty())
}

fn live() -> Result<Vec<Value>> {
    let client = pack()?;
    atoms_lean(&client, &client.workspace()).context("prefer: GET /v1/atoms failed")
}

/// Raise every promoted atom. One the pack refuses does not stop the
/// rest; each comes back as `id: why`.
fn stamp_marked() -> Result<Vec<String>> {
    let client = pack()?;
    let atoms = atoms_lean(&client, &client.workspace())?;
    let mut refused = Vec::new();
    for atom in &atoms {
        if legacy(atom) != Some(Legacy::Promote) {
            continue;
        }
        if let Err(e) = stamp_basis(&client, atom) {
            if e.downcast_ref::<Refused>().is_none() {
                return Err(e);
            }
            refused.push(format!("{}: {e:#}", id_of(atom).unwrap_or("?")));
        }
    }
    Ok(refused)
}

/// Store `atom` again as the person's and retire the original. Returns
/// the id the person's copy is stored under.
///
/// The copy goes in first under an id of its own, and the original is
/// retired only once the copy reads back live. The pack runs its checks
/// on a post before it answers a live text with the record that already
/// holds it, so a refusal there leaves the original as it was. A pack
/// that answers with the original cannot hold both; the original is then
/// written down before it is retired, so a run stopped after the delete
/// is finished by the next one.
fn stamp_basis(client: &packset_client::PacksetClient, atom: &Value) -> Result<String> {
    let id = id_of(atom).context("legacy preference has no id")?;
    let copy = person_copy(client, atom)?;
    let new_id = copy_id(id);
    let held = post(client, "/v1/atoms", &copy)?;
    if id_of(&held) == Some(new_id.as_str()) || fetch(client, &new_id)?.is_some() {
        confirm(client, atom, &new_id)?;
        retire(client, id)?;
        unjournal(id);
        return Ok(new_id);
    }
    // With the original gone the pack answers the copy with any other
    // live record that makes the same claim, so one must not exist.
    let live = atoms_lean(client, &client.workspace())?;
    if let Some(other) = live
        .iter()
        .find(|a| id_of(a) != Some(id) && same_claim(a, atom))
    {
        return Err(Refused(format!(
            "the pack holds {id}'s text under {} as well",
            id_of(other).unwrap_or("?")
        ))
        .into());
    }
    journal(atom)?;
    retire(client, id)?;
    store_copy(client, atom)
}

/// Store the copy of `atom`, whose original is already retired, and
/// strike it from the journal once the copy reads back.
fn store_copy(client: &packset_client::PacksetClient, atom: &Value) -> Result<String> {
    let id = id_of(atom).context("legacy preference has no id")?;
    let new_id = copy_id(id);
    let unstored = |e: anyhow::Error| anyhow!("{id} was retired before its copy was stored: {e:#}");
    if fetch(client, &new_id).map_err(unstored)?.is_none() {
        post(client, "/v1/atoms", &person_copy(client, atom)?).map_err(unstored)?;
    }
    confirm(client, atom, &new_id).map_err(unstored)?;
    unjournal(id);
    Ok(new_id)
}

/// Raise `atom`: from the live original when the pack still holds it,
/// else from the journal's record of it.
fn finish(client: &packset_client::PacksetClient, atom: &Value) -> Result<String> {
    let id = id_of(atom).context("legacy preference has no id")?;
    match fetch(client, id)? {
        Some(live) if legacy(&live) == legacy(atom) => stamp_basis(client, &live),
        Some(_) => {
            unjournal(id);
            bail!("{id} changed since its raise was written down")
        }
        None => store_copy(client, atom),
    }
}

/// Finish every promoted raise a run stopped after the retirement.
fn finish_retired() -> Result<()> {
    let pending = retired_unstored(Legacy::Promote);
    if pending.is_empty() {
        return Ok(());
    }
    let client = pack()?;
    for atom in &pending {
        finish(&client, atom)?;
    }
    Ok(())
}

fn person_copy(client: &packset_client::PacksetClient, atom: &Value) -> Result<Value> {
    let id = id_of(atom).context("legacy preference has no id")?;
    let mut copy = atom.clone();
    if let Some(map) = copy.as_object_mut() {
        for key in ["embedding", "links", "provenance_floor", "tombstone"] {
            map.remove(key);
        }
        if !map.get("source").is_some_and(Value::is_object) {
            map.insert("source".into(), json!({}));
        }
        map.insert("id".into(), json!(copy_id(id)));
        map.insert("workspace".into(), json!(client.workspace()));
    }
    crate::admit::stamp_person(&mut copy);
    Ok(copy)
}

/// The id the person's copy of `id` is stored under. Derived from the
/// original, so a rerun posts the same id and finds the copy it stored.
fn copy_id(id: &str) -> String {
    crate::work_id(&format!("pref-basis copy\n{id}"))
}

/// The copy reads back live under its own id, as the person's, with the
/// original's claim, entities and clock.
fn confirm(client: &packset_client::PacksetClient, atom: &Value, new_id: &str) -> Result<()> {
    let id = id_of(atom).unwrap_or("?");
    let Some(stored) = fetch(client, new_id)? else {
        bail!("the pack does not hold the copy of {id} under {new_id}");
    };
    if !crate::person_stored(&stored) {
        bail!("the pack holds {id}'s copy under a record that is not the person's");
    }
    if !same_claim(&stored, atom) {
        return Err(Refused(format!("{new_id} does not make {id}'s claim")).into());
    }
    if let Some(key) = ["entities", "ts", "valid_from"]
        .iter()
        .find(|key| stored.get(**key) != atom.get(**key))
    {
        return Err(Refused(format!("{new_id} differs from {id} in {key}")).into());
    }
    Ok(())
}

/// The fields the pack compares when it answers a post with a live
/// record that already makes the claim.
fn same_claim(a: &Value, b: &Value) -> bool {
    [
        "text", "kind", "set", "pattern", "verdict", "from", "to", "about", "agent", "issue",
        "name",
    ]
    .iter()
    .all(|key| a.get(*key) == b.get(*key))
}

fn retire(client: &packset_client::PacksetClient, id: &str) -> Result<()> {
    post(
        client,
        "/v1/atoms/delete",
        &json!({"workspace": client.workspace(), "id": id}),
    )
    .map(drop)
    // A copy may already be live, so a failed retirement is retried, not
    // recorded as a refusal.
    .map_err(|e| anyhow!("{e:#}"))
}

/// The live record `id` names, or none.
fn fetch(client: &packset_client::PacksetClient, id: &str) -> Result<Option<Value>> {
    let url = format!("{}/v1/atoms/{id}", client.base());
    match packset_client::authorize(ureq::get(&url))
        .query("workspace", &client.workspace())
        .timeout(http_timeout())
        .call()
    {
        Ok(resp) => resp
            .into_json()
            .map(Some)
            .with_context(|| format!("GET /v1/atoms/{id}: the answer is not JSON")),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => bail!("GET /v1/atoms/{id} failed: {e}"),
    }
}

/// Where a raise writes the original down before retiring it, one file
/// per id, until its copy reads back.
fn journal_dir() -> Option<PathBuf> {
    let client = pack().ok()?;
    let name = crate::work_id(&format!("{}\n{}", client.base(), client.workspace()));
    Some(state_dir().join("pref-basis-retired").join(name))
}

fn journal(atom: &Value) -> Result<()> {
    let id = id_of(atom).context("legacy preference has no id")?;
    let dir = journal_dir().context("no pack to write the raise down for")?;
    std::fs::create_dir_all(&dir)?;
    let body = serde_json::to_vec(atom)?;
    let tmp = dir.join(format!(".{id}"));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, dir.join(id)).context("writing the raise down")
}

fn unjournal(id: &str) {
    if let Some(dir) = journal_dir() {
        let _ = std::fs::remove_file(dir.join(id));
    }
}

/// The originals a raise retired before their copy read back, of one
/// kind of legacy. A file whose name is not its record's id is passed
/// over.
fn retired_unstored(want: Legacy) -> Vec<Value> {
    let Some(entries) = journal_dir().and_then(|d| std::fs::read_dir(d).ok()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let atom: Value = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            (id_of(&atom) == e.file_name().to_str() && legacy(&atom) == Some(want)).then_some(atom)
        })
        .collect()
}

fn id_of(atom: &Value) -> Option<&str> {
    atom.get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

fn post(client: &packset_client::PacksetClient, path: &str, body: &Value) -> Result<Value> {
    let url = format!("{}{path}", client.base());
    match packset_client::authorize(ureq::post(&url))
        .timeout(http_timeout())
        .send_json(body)
    {
        Ok(resp) => resp
            .into_json()
            .with_context(|| format!("POST {path}: the answer is not JSON")),
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let why = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error")?.as_str().map(str::to_string))
                .unwrap_or(text);
            let why = format!("POST {path} failed: {code}: {}", why.trim());
            if (400..500).contains(&code) && !matches!(code, 401 | 408 | 429) {
                return Err(Refused(why).into());
            }
            bail!(why);
        }
        Err(e) => bail!("POST {path} failed: {e}"),
    }
}

/// A 4xx the pack will answer again for the same body. Auth, a timeout
/// and a rate limit are not one: the next run may get through.
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

fn gate() -> &'static Mutex<()> {
    static GATE: Mutex<()> = Mutex::new(());
    &GATE
}

fn marker_path() -> Option<PathBuf> {
    let client = pack().ok()?;
    let name = crate::work_id(&format!("{}\n{}", client.base(), client.workspace()));
    Some(state_dir().join("pref-basis").join(name))
}

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
}

fn recorded() -> bool {
    marker_path().is_some_and(|p| p.is_file())
}

/// Write `body` as this pack's record. `Ok(false)` when another run wrote
/// it first, or there is no pack to name it after.
fn record(body: &str) -> Result<bool> {
    let Some(path) = marker_path() else {
        return Ok(false);
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut f) => f
            .write_all(body.as_bytes())
            .map(|()| true)
            .context("recording the preference migration"),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e).context("recording the preference migration"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(kind: &str, origin: &str, entities: &[&str], source: Value) -> Value {
        json!({
            "id": "0123456789abcdef0123456789abcdef",
            "kind": kind,
            "text": "Use tabs in Go files.",
            "origin": origin,
            "entities": entities,
            "source": source,
        })
    }

    fn terminal_source() -> Value {
        json!({
            "harness": "person",
            "seat": "person",
            "session": "person",
            "host": "box",
            "at": "2026-10-20T00:00:00Z",
            "via": "ljos",
        })
    }

    fn runner_source() -> Value {
        json!({
            "harness": "claude",
            "seat": "claude",
            "session": "claude@k3j9x0a1b2",
            "host": "box",
            "at": "2026-10-20T00:00:00Z",
            "via": "ljos",
        })
    }

    #[test]
    fn old_writers_fall_by_the_marks_they_left() {
        let hook = atom(
            "preference",
            "user-declared",
            &["seat:claude", "horizon:standing", "via:prompt"],
            runner_source(),
        );
        let typed = atom(
            "preference",
            "user-declared",
            &["seat:person", "horizon:standing"],
            terminal_source(),
        );
        let remembered = atom(
            "lesson",
            "user-declared",
            &["seat:person", "horizon:standing"],
            terminal_source(),
        );
        let mut runner = atom(
            "preference",
            "user-declared",
            &["seat:claude", "horizon:standing"],
            runner_source(),
        );
        let mut turned = typed.clone();
        turned["source"]["turn"] = json!("turn-1");
        turned["id"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2");
        let mut ancestor = typed.clone();
        ancestor["source"]["harness"] = json!("node");
        ancestor["id"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa3");
        let mut bare = typed.clone();
        bare.as_object_mut().unwrap().remove("source");
        bare.as_object_mut().unwrap().remove("origin");
        bare["id"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa4");
        let mut forged = hook.clone();
        forged["origin"] = json!("agent-derived");
        let mut planted = runner.clone();
        planted["source"]["basis"] = json!("agent");
        let mut already = hook.clone();
        already["source"]["basis"] = json!("person");
        let tool = atom("lesson", "tool-echo", &["seat:probe"], terminal_source());
        let mut ended = runner.clone();
        ended["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!("via:session-end"));
        ended["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!("session:read-1"));
        // via:prompt beside a session-end tag is both writers at once.
        // Neither mark is unambiguous, so it stays held back.
        let mut hook_and_session = hook.clone();
        hook_and_session["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!("session:read-1"));

        let fixtures = [
            (&hook, Some(Legacy::Promote)),
            (&typed, Some(Legacy::Promote)),
            (&remembered, Some(Legacy::Promote)),
            (&hook_and_session, Some(Legacy::Hold)),
            (&runner, Some(Legacy::Hold)),
            (&turned, Some(Legacy::Hold)),
            (&ancestor, Some(Legacy::Hold)),
            (&bare, Some(Legacy::Hold)),
            (&ended, Some(Legacy::Hold)),
            (&forged, None),
            (&planted, None),
            (&already, None),
            (&tool, None),
        ];
        let mut promote = 0;
        let mut hold = 0;
        let mut left = 0;
        for (atom, want) in &fixtures {
            assert_eq!(legacy(atom), *want, "{atom}");
            match want {
                Some(Legacy::Promote) => promote += 1,
                Some(Legacy::Hold) => hold += 1,
                None => left += 1,
            }
        }
        assert_eq!((promote, hold, left), (3, 6, 4), "fixture split");
        runner["id"] = json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        let waiting = awaiting(&[hook, typed, runner]);
        assert_eq!(waiting.len(), 1);
        assert!(review_sentence(1).contains("ljos prefer --review"));
        assert!(review_sentence(2).contains("preferences await"));
    }
}
