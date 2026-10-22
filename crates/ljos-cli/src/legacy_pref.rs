//! Preferences stored before a basis was stamped.
//!
//! poisonread made [`crate::person_stored`] require `source.basis` `person`.
//! A preference written before that has no basis, so it stopped showing.
//! The old store cannot tell a person's preference from an agent's soft
//! prose, so only a mark an old person-path writer actually left is
//! promoted, and only when the atom is older than the cutoff recorded in
//! the pack. Anything else stays unverified until the person keeps it.
//!
//! The cutoff is one receipt, entity `receipt:pref-basis-cutoff`, written
//! once by the first person-side run. Its server `ts` is the bound. An
//! atom at that time or after is not an older preference, whatever marks
//! it carries. `via:prompt` alone never promotes: the hook runs under the
//! runner, and a direct post can wear the tag. Search, the island, the
//! command gate, session start and any process under a runner do not
//! write the receipt and do not stamp a basis.
//!
//! One atom the pack refuses does not stop the rest. The receipt is
//! written before any stamp. A permanent refusal is recorded on it as
//! `source.refused` and is not tried again. A pack that did not answer
//! is tried on the next person-side run.
//!
//! The pack refuses an update that raises `source.basis`. A missing
//! basis is below `person`, and accept is the only rise it allows, so
//! this run does not climb. It closes the unmarked atom and writes a
//! new one born with basis `person`. The closed atom keeps the basis it
//! had. `source.pending` on the cutoff is that write, recorded before
//! the close, and the next person-side run finishes it.
//!
//! These marks are not proof the person typed the atom. A v0.32.1
//! `ljos prefer` with `CLAUDECODE=1` and no runner ancestor stored
//! harness, seat and session as the same word, and that shape is
//! promoted. The cutoff and the runner check are what this upgrade
//! trusts. The marks only choose which older atoms are candidates.
//!
//! The writers at v0.32.1, and the same writers through d47b69bd, left
//! these marks:
//!
//! * The prompt hook (`capture::save` on `UserPromptSubmit`) stamps entity
//!   `via:prompt` and origin `user-declared`, and no basis. It runs under
//!   the runner, so `source.session` may name that runner's holder. The
//!   tag is not promoted.
//! * A terminal `ljos prefer` or `ljos remember` (`post_claim_horizon`)
//!   stamps origin `user-declared`, `source.via` `ljos`, and no basis. With
//!   no runner ancestor and no runner session id, `source.harness` and
//!   `source.session` are the same word and `source.turn` is absent.
//!   `source.seat` is that same word from v0.30 on. v0.17 through v0.29
//!   left it out. `seat_from_tree` is none in that case, and no
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

use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, FixedOffset, Utc};
use serde_json::{json, Value};

use crate::admit::ORIGIN_USER;
use crate::{atoms_lean, pack, person_present, under_a_runner};

/// The receipt that records the cutoff. Its server `ts` is the bound.
const CUTOFF_ENTITY: &str = "receipt:pref-basis-cutoff";

/// Stable text, so a second post of the same receipt is the first one.
const CUTOFF_TEXT: &str = "Recorded the cutoff for preference basis migration.";

/// What a legacy atom is. `None` from [`legacy`] means it is not one: it
/// has a basis, it is not a preference or a lesson, or its origin was
/// never the person's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Legacy {
    /// A terminal write. The upgrade stamps basis `person` when the atom
    /// is older than the pack's cutoff.
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
    // The pack stamps `user-declared` on a post that left origin out, and
    // fills it on the way out when the record is blank. A body that
    // arrives without one was not stored that way, so it is not legacy.
    // An origin other than the person's never read as theirs.
    let origin = origin?;
    if !origin.eq_ignore_ascii_case(ORIGIN_USER) {
        return None;
    }
    // `via:prompt` is not read. The hook runs under the runner, and a
    // direct post can wear the tag.
    if terminal_write(atom) {
        Some(Legacy::Promote)
    } else {
        Some(Legacy::Hold)
    }
}

/// Preferences and lessons with no basis, older than the pack's cutoff,
/// that the upgrade did not promote. No cutoff means nothing is older yet.
/// Each row is (id, kind, text).
#[must_use]
pub(crate) fn awaiting(atoms: &[Value]) -> Vec<(String, String, String)> {
    let Some(cutoff) = earliest_cutoff(atoms) else {
        return Vec::new();
    };
    atoms
        .iter()
        .filter(|a| legacy(a) == Some(Legacy::Hold))
        .filter(|a| {
            matches!(
                a.get("kind").and_then(Value::as_str),
                Some("preference" | "lesson")
            )
        })
        .filter(|a| older_than(a, cutoff))
        .filter_map(|a| {
            let id = a.get("id").and_then(Value::as_str)?.trim();
            let kind = a.get("kind").and_then(Value::as_str)?.trim();
            let text = a.get("text").and_then(Value::as_str)?.trim();
            if id.is_empty() || kind.is_empty() || text.is_empty() {
                None
            } else {
                Some((id.to_string(), kind.to_string(), text.to_string()))
            }
        })
        .collect()
}

/// The sentence session start, the doctor and `ljos prefer --review` share.
///
/// Preferences and lessons are named on their own. A mix is memories, so
/// the count is not a count of preferences alone.
#[must_use]
pub(crate) fn review_sentence(held: &[(String, String, String)]) -> String {
    let n = held.len();
    let lessons = held.iter().filter(|(_, kind, _)| kind == "lesson").count();
    let (noun, verb) = if lessons > 0 && lessons < n {
        ("memories", "await")
    } else if lessons > 0 && n == 1 {
        ("lesson", "awaits")
    } else if lessons > 0 {
        ("lessons", "await")
    } else if n == 1 {
        ("preference", "awaits")
    } else {
        ("preferences", "await")
    };
    format!("{n} older {noun} {verb} review (`ljos prefer --review`)")
}

/// Stamp basis `person` on every legacy atom a terminal writer marked,
/// when that atom is older than this pack's cutoff.
///
/// The cutoff is a receipt written once, by the first run of this that
/// is not under a runner. A later run finds that receipt and does not
/// write another. Search, the island, the command gate and session start
/// do not call this. A pack that does not answer is left for the next run.
/// One refused update does not stop the receipt or the atoms after it.
pub fn migrate_once() {
    if under_a_runner() {
        return;
    }
    crate::with_env_mutation(|| {
        let _guard = gate().lock().unwrap_or_else(|e| e.into_inner());
        let _ = stamp_marked();
    });
}

/// List the preferences and lessons the upgrade held back.
///
/// # Errors
///
/// The pack did not answer.
pub fn list() -> Result<String> {
    migrate_once();
    let atoms = live()?;
    let rows = awaiting(&atoms);
    if rows.is_empty() {
        return Ok("no older preferences await review\n".to_string());
    }
    let mut out = format!("{}\n", review_sentence(&rows));
    for (id, kind, text) in &rows {
        let Some(atom) = atoms
            .iter()
            .find(|a| a.get("id").and_then(Value::as_str) == Some(id.as_str()))
        else {
            continue;
        };
        let short = id.get(..8).unwrap_or(id);
        out.push_str(&format!(
            "{short}  {}  {}  {}  {kind}  {text}\n  ljos prefer --review --keep {id}\n",
            shown_when(atom),
            shown_seat(atom),
            shown_source(atom),
        ));
    }
    Ok(out)
}

/// The day the pack stored the atom, or the writer's own clock when the
/// pack has no `ts`.
fn shown_when(atom: &Value) -> String {
    let raw = atom
        .get("ts")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            atom.pointer("/source/at")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or("")
        .trim();
    if raw.len() >= 10
        && raw.as_bytes().get(4) == Some(&b'-')
        && raw.as_bytes().get(7) == Some(&b'-')
    {
        raw[..10].to_string()
    } else if raw.is_empty() {
        "-".to_string()
    } else {
        raw.to_string()
    }
}

/// `source.seat`, else the harness, else a `seat:` entity.
fn shown_seat(atom: &Value) -> String {
    for key in ["seat", "harness"] {
        let word = source_field(atom, key);
        if !word.is_empty() {
            return word.to_string();
        }
    }
    entities(atom)
        .into_iter()
        .find_map(|e| e.strip_prefix("seat:").map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".to_string())
}

/// `via/harness/session`, dropping whichever the writer left out.
fn shown_source(atom: &Value) -> String {
    let bits: Vec<&str> = ["via", "harness", "session"]
        .into_iter()
        .map(|key| source_field(atom, key))
        .filter(|s| !s.is_empty())
        .collect();
    if bits.is_empty() {
        "-".to_string()
    } else {
        bits.join("/")
    }
}

fn source_field<'a>(atom: &'a Value, key: &str) -> &'a str {
    atom.get("source")
        .and_then(Value::as_object)
        .and_then(|s| s.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
}

/// Keep one held-back preference or lesson as the person's.
///
/// A runner is refused, and so is anything [`person_present`] does not
/// prove: a pipe, a pty a tool call opened, a process left by a double
/// fork. The person types this at a terminal of their own.
///
/// # Errors
///
/// The id is not a held-back preference or lesson, this process is under
/// a runner or the person is not at its terminal, or the pack refused the
/// update.
pub fn keep(id: &str) -> Result<String> {
    if under_a_runner() {
        bail!(
            "prefer: a runner cannot keep a preference or a lesson. \
             Run `ljos prefer --review --keep {id}` from a terminal of your own"
        );
    }
    if !person_present() {
        bail!(
            "prefer: keeping a preference or a lesson needs a terminal. \
             Run `ljos prefer --review --keep {id}` from a terminal of your own"
        );
    }
    migrate_once();
    let atoms = live()?;
    let rows = awaiting(&atoms);
    let ids: Vec<String> = rows.iter().map(|(id, _, _)| id.clone()).collect();
    let id = crate::resolve_short_id("prefer", id.trim(), &ids)?;
    let Some(atom) = atoms
        .iter()
        .find(|a| a.get("id").and_then(Value::as_str) == Some(id.as_str()))
    else {
        bail!("prefer: {id} is not an older preference or lesson awaiting review");
    };
    let kind = atom.get("kind").and_then(Value::as_str);
    if legacy(atom) != Some(Legacy::Hold) || !matches!(kind, Some("preference" | "lesson")) {
        bail!("prefer: {id} is not an older preference or lesson awaiting review");
    }
    let client = pack()?;
    match stamp_basis(&client, atom) {
        Update::Stamped => {}
        Update::Refused(code) => {
            bail!("prefer: could not keep {id}: the pack refused the update ({code})")
        }
        Update::Later => bail!("prefer: could not keep {id}: the pack did not answer"),
    }
    let noun = if kind == Some("lesson") {
        "lesson"
    } else {
        "preference"
    };
    let shown = live()
        .ok()
        .and_then(|atoms| {
            atoms.into_iter().find(|stored| {
                stored.get("text") == atom.get("text")
                    && stored.get("kind") == atom.get("kind")
                    && basis_of(stored) == Some("person")
            })
        })
        .and_then(|stored| atom_id(&stored))
        .unwrap_or_else(|| id.clone());
    Ok(format!("kept {id} as the person's {noun}, now {shown}\n"))
}

/// The shape `post_claim_horizon` left when no runner ancestor and no
/// runner session id were recorded.
///
/// v0.30 stamps `source.seat` with the same word as harness and session.
/// v0.17 through v0.29 left `source.seat` out and still wrote harness and
/// session as the login user. A seat that names a different word is not
/// this write.
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
    if harness.is_empty() || session != harness || session.contains('@') {
        return false;
    }
    if !seat.is_empty() && seat != harness {
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

/// How long one update may take. A hook is killed at 10s, so the listing
/// and this update stop at the hook's bound. `PACKSET_TIMEOUT_MS` still
/// shortens it.
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

/// What one `POST /v1/atoms/update` did.
enum Update {
    Stamped,
    /// The pack refused this atom and will refuse it the same way again.
    Refused(u16),
    /// The pack did not answer, or asked us to wait.
    Later,
}

fn stamp_marked() -> Result<usize> {
    let client = pack()?;
    let mut atoms = atoms_lean(&client, &client.workspace())?;
    // The receipt is written before any stamp, so a later refusal cannot
    // move the window. Atoms read above are the ones this run may stamp;
    // one posted after the receipt is newer than it.
    let mut bound = match earliest_bound(&atoms) {
        Some(bound) => bound,
        None => bound_of(&record_cutoff(&client)?)
            .context("prefer: the preference cutoff has no time")?,
    };
    // A previous run may have closed an atom and stopped before the
    // replacement was stored. Finish that before reading the live set
    // again, or the closed preference is no longer there to migrate.
    if replay_pending(&client, &bound)? {
        atoms = atoms_lean(&client, &client.workspace())?;
        bound = earliest_bound(&atoms).context("prefer: the preference cutoff has no time")?;
    }
    let already = refused_ids(&bound.source);
    let mut fresh = Vec::new();
    let mut later = false;
    let mut n = 0;
    for atom in &atoms {
        if legacy(atom) != Some(Legacy::Promote) || !older_than(atom, bound.at) {
            continue;
        }
        let Some(id) = atom_id(atom) else {
            continue;
        };
        if already.iter().any(|seen| seen == &id) {
            continue;
        }
        match stamp_basis(&client, atom) {
            Update::Stamped => n += 1,
            Update::Refused(_) => fresh.push(id),
            Update::Later => later = true,
        }
    }
    if !fresh.is_empty() {
        remember_refusals(&client, &bound, &fresh)?;
    }
    if later {
        bail!("prefer: the pack did not answer an update; it will be tried again");
    }
    Ok(n)
}

/// A 4xx the pack will give again for this body. Auth, a timeout and a
/// rate limit are not that: the next run may get through.
fn permanent(code: u16) -> bool {
    (400..500).contains(&code) && !matches!(code, 401 | 408 | 429)
}

/// The cutoff receipt this run stamps against: its time, its id, and the
/// source object that lists atoms the pack has already refused.
struct Bound {
    at: DateTime<FixedOffset>,
    id: String,
    source: Value,
}

/// The earliest cutoff receipt in `atoms`, compared as a time. A `Z`
/// stamp and a millisecond stamp are the same instant either way; a
/// string compare is not.
fn earliest_cutoff(atoms: &[Value]) -> Option<DateTime<FixedOffset>> {
    earliest_bound(atoms).map(|bound| bound.at)
}

fn earliest_bound(atoms: &[Value]) -> Option<Bound> {
    atoms
        .iter()
        .filter_map(bound_of)
        .min_by_key(|bound| bound.at)
}

fn bound_of(atom: &Value) -> Option<Bound> {
    if !is_cutoff(atom) {
        return None;
    }
    let at = instant_of(atom)?;
    let id = atom_id(atom)?;
    let source = atom
        .get("source")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    Some(Bound { at, id, source })
}

fn atom_id(atom: &Value) -> Option<String> {
    let id = atom.get("id").and_then(Value::as_str)?.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// Ids on `source.refused`. The pack refused an update of each one, so a
/// later run does not ask again.
fn refused_ids(source: &Value) -> Vec<String> {
    source
        .get("refused")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn is_cutoff(atom: &Value) -> bool {
    atom.get("kind").and_then(Value::as_str) == Some("receipt")
        && entities(atom).iter().any(|e| e == CUTOFF_ENTITY)
}

fn older_than(atom: &Value, cutoff: DateTime<FixedOffset>) -> bool {
    instant_of(atom).is_some_and(|ts| ts < cutoff)
}

fn instant_of(atom: &Value) -> Option<DateTime<FixedOffset>> {
    let raw = atom.get("ts").and_then(Value::as_str)?.trim();
    if raw.is_empty() {
        None
    } else {
        DateTime::parse_from_rfc3339(raw).ok()
    }
}

fn record_cutoff(client: &packset_client::PacksetClient) -> Result<Value> {
    let seat = crate::seat_name();
    let atom = json!({
        "schema": "inside.atom/v1",
        "kind": "receipt",
        "level": "explicit",
        "origin": ORIGIN_USER,
        "text": CUTOFF_TEXT,
        "workspace": client.workspace(),
        "entities": [format!("seat:{seat}"), CUTOFF_ENTITY],
        "source": crate::atom_source(),
    });
    client
        .post_atom(&atom)
        .context("prefer: could not record the preference cutoff")
}

/// Write `ids` onto the cutoff receipt. A later run skips them.
///
/// The source is read again first. A rewrite may have noted `pending`
/// on the receipt since `bound` was taken, and this must not wipe it.
fn remember_refusals(
    client: &packset_client::PacksetClient,
    bound: &Bound,
    ids: &[String],
) -> Result<()> {
    let mut source = cutoff_source(client)?.unwrap_or_else(|| bound.source.clone());
    let mut list = refused_ids(&source);
    for id in ids {
        if !list.iter().any(|seen| seen == id) {
            list.push(id.clone());
        }
    }
    source["refused"] = json!(list);
    match post_source(client, &bound.id, source) {
        Update::Stamped => Ok(()),
        Update::Refused(code) => {
            bail!("prefer: the pack refused to record a refused preference: {code}")
        }
        Update::Later => {
            bail!("prefer: the pack did not answer while recording a refused preference")
        }
    }
}

fn cutoff_source(client: &packset_client::PacksetClient) -> Result<Option<Value>> {
    let atoms = atoms_lean(client, &client.workspace())?;
    Ok(earliest_bound(&atoms).map(|bound| bound.source))
}

fn stamp_basis(client: &packset_client::PacksetClient, atom: &Value) -> Update {
    let Some(id) = atom_id(atom) else {
        return Update::Later;
    };
    let mut source = atom
        .get("source")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    source["basis"] = json!("person");
    match send_update(client, &id, json!({ "source": source })) {
        Sent::Stored => Update::Stamped,
        Sent::Permanent(_code, body) if basis_rose(&body) => bear_person(client, atom),
        Sent::Permanent(code, _) => Update::Refused(code),
        Sent::Retry => Update::Later,
    }
}

/// The pack said this update would raise `source.basis`, which only an
/// accept may do. The words are the pack's.
fn basis_rose(body: &str) -> bool {
    body.contains("cannot raise source.basis")
}

/// Close `atom` and write a new one born with basis `person`.
///
/// The cutoff records the new body before the close. A killed process
/// leaves that note, and the next person-side run finishes the write
/// instead of losing the preference.
fn bear_person(client: &packset_client::PacksetClient, atom: &Value) -> Update {
    let Some(id) = atom_id(atom) else {
        return Update::Later;
    };
    let successor = successor_of(client, atom);
    match note_pending(client, &id, &successor) {
        Update::Stamped => {}
        other => return other,
    }
    finish_born(client, &id, &successor)
}

/// Finish one note on the cutoff: the person atom is live, and the
/// unmarked one is not.
fn finish_born(client: &packset_client::PacksetClient, id: &str, successor: &Value) -> Update {
    let atoms = match atoms_lean(client, &client.workspace()) {
        Ok(atoms) => atoms,
        Err(_) => return Update::Later,
    };
    // Once the original is closed, the pack answers the successor's post
    // with any other live record that makes the same claim, so a record
    // like that, other than the copy itself, stops the raise here.
    if atoms.iter().any(|atom| {
        atom_id(atom).as_deref() != Some(id)
            && same_claim(atom, successor)
            && !copy_of(atom, successor)
    }) {
        let _ = drop_pending(client, id);
        return Update::Refused(SHADOWED);
    }
    if person_copy(&atoms, successor, id) {
        if let Some(old) = atoms
            .iter()
            .find(|atom| atom_id(atom).as_deref() == Some(id))
        {
            match close_predecessor(client, old) {
                Update::Stamped => {}
                other => return other,
            }
        }
        return drop_pending(client, id);
    }
    let closed = if let Some(old) = atoms
        .iter()
        .find(|atom| atom_id(atom).as_deref() == Some(id))
    {
        match close_predecessor(client, old) {
            Update::Stamped => true,
            Update::Refused(code) => {
                let _ = drop_pending(client, id);
                return Update::Refused(code);
            }
            Update::Later => return Update::Later,
        }
    } else {
        false
    };
    match post_successor(client, successor) {
        Update::Stamped => drop_pending(client, id),
        Update::Refused(code) => {
            if closed {
                let _ = reopen(client, id);
            }
            let _ = drop_pending(client, id);
            Update::Refused(code)
        }
        Update::Later => Update::Later,
    }
}

fn person_copy(atoms: &[Value], successor: &Value, old: &str) -> bool {
    atoms
        .iter()
        .any(|atom| atom_id(atom).as_deref() != Some(old) && copy_of(atom, successor))
}

/// The code a raise is refused with when another live record makes the
/// claim and would be taken for the person's copy.
const SHADOWED: u16 = 409;

/// The fields the pack compares when it answers a post with a live record
/// that already makes the claim.
fn same_claim(a: &Value, b: &Value) -> bool {
    [
        "text", "kind", "set", "pattern", "verdict", "from", "to", "about", "agent", "issue",
        "name",
    ]
    .iter()
    .all(|key| a.get(*key) == b.get(*key))
}

/// `atom` is the person's copy of `successor`: the same claim, entities
/// and clock, born with basis `person`.
fn copy_of(atom: &Value, successor: &Value) -> bool {
    same_claim(atom, successor)
        && ["entities", "ts", "valid_from"]
            .iter()
            .all(|key| atom.get(*key) == successor.get(*key))
        && basis_of(atom) == Some("person")
}

/// The same claim, born with basis `person`. No id and no floor: those
/// belong to the record the pack already holds. The clock stays, so a
/// lesson hash still names this claim.
fn successor_of(client: &packset_client::PacksetClient, atom: &Value) -> Value {
    let mut source = atom
        .get("source")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    source["basis"] = json!("person");
    let mut body = json!({
        "schema": "inside.atom/v1",
        "kind": atom.get("kind").cloned().unwrap_or(json!("preference")),
        "level": atom.get("level").cloned().unwrap_or(json!("explicit")),
        "origin": ORIGIN_USER,
        "text": atom.get("text").cloned().unwrap_or(json!("")),
        "workspace": client.workspace(),
        "entities": atom.get("entities").cloned().unwrap_or(json!([])),
        "source": source,
    });
    if let Some(set) = atom.get("set").filter(|set| !set.is_null()) {
        body["set"] = set.clone();
    }
    for key in ["ts", "valid_from"] {
        if let Some(stamp) = atom
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|stamp| !stamp.is_empty())
        {
            body[key] = json!(stamp);
        }
    }
    body
}

/// `valid_to` in the store's own stamp form, so a string compare against
/// the server's clock takes the atom out of the live set. The atom's own
/// `ts` is that instant when it parses, and it is already in the past:
/// the cutoff was written after it.
fn closed_stamp(atom: &Value) -> String {
    instant_of(atom)
        .map(|ts| {
            ts.with_timezone(&Utc)
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()
        })
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}

fn close_predecessor(client: &packset_client::PacksetClient, atom: &Value) -> Update {
    let Some(id) = atom_id(atom) else {
        return Update::Later;
    };
    post_fields(client, &id, json!({ "valid_to": closed_stamp(atom) }))
}

fn reopen(client: &packset_client::PacksetClient, id: &str) -> Update {
    post_fields(client, id, json!({ "valid_to": Value::Null }))
}

fn post_successor(client: &packset_client::PacksetClient, successor: &Value) -> Update {
    let url = format!("{}/v1/atoms", client.base());
    match packset_client::authorize(ureq::post(&url))
        .timeout(http_timeout())
        .send_json(successor)
    {
        Ok(resp) => match resp.into_json::<Value>() {
            Ok(body) if copy_of(body.get("atom").unwrap_or(&body), successor) => Update::Stamped,
            // Another record answered for the copy.
            Ok(body) if body.get("atom").unwrap_or(&body).get("id").is_some() => {
                Update::Refused(SHADOWED)
            }
            _ => Update::Later,
        },
        Err(ureq::Error::Status(code, _)) if permanent(code) => Update::Refused(code),
        Err(_) => Update::Later,
    }
}

/// Writes still owed on the cutoff, each `{id, atom}`.
fn pending_of(source: &Value) -> Vec<Value> {
    source
        .get("pending")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn note_pending(client: &packset_client::PacksetClient, id: &str, successor: &Value) -> Update {
    alter_cutoff(client, |source| {
        let mut list = pending_of(source);
        if !list
            .iter()
            .any(|item| item.get("id").and_then(Value::as_str) == Some(id))
        {
            list.push(json!({ "id": id, "atom": successor }));
        }
        source["pending"] = json!(list);
    })
}

fn drop_pending(client: &packset_client::PacksetClient, id: &str) -> Update {
    alter_cutoff(client, |source| {
        let list: Vec<Value> = pending_of(source)
            .into_iter()
            .filter(|item| item.get("id").and_then(Value::as_str) != Some(id))
            .collect();
        let Some(obj) = source.as_object_mut() else {
            return;
        };
        if list.is_empty() {
            obj.remove("pending");
        } else {
            obj.insert("pending".into(), json!(list));
        }
    })
}

fn alter_cutoff(client: &packset_client::PacksetClient, change: impl FnOnce(&mut Value)) -> Update {
    let atoms = match atoms_lean(client, &client.workspace()) {
        Ok(atoms) => atoms,
        Err(_) => return Update::Later,
    };
    let Some(mut bound) = earliest_bound(&atoms) else {
        return Update::Later;
    };
    change(&mut bound.source);
    post_source(client, &bound.id, bound.source)
}

/// Finish cutoff notes from a run that closed an atom and did not get to
/// post its replacement. Nothing owed is not a change.
fn replay_pending(client: &packset_client::PacksetClient, bound: &Bound) -> Result<bool> {
    let pending = pending_of(&bound.source);
    if pending.is_empty() {
        return Ok(false);
    }
    let mut later = false;
    for item in &pending {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(successor) = item.get("atom") else {
            continue;
        };
        if matches!(finish_born(client, id, successor), Update::Later) {
            later = true;
        }
    }
    if later {
        bail!("prefer: the pack did not answer an update; it will be tried again");
    }
    Ok(true)
}

fn post_source(client: &packset_client::PacksetClient, id: &str, source: Value) -> Update {
    post_fields(client, id, json!({ "source": source }))
}

fn post_fields(client: &packset_client::PacksetClient, id: &str, fields: Value) -> Update {
    match send_update(client, id, fields) {
        Sent::Stored => Update::Stamped,
        Sent::Permanent(code, _) => Update::Refused(code),
        Sent::Retry => Update::Later,
    }
}

/// What one `POST /v1/atoms/update` did, with the pack's body when it
/// refused. The body is how a basis rise is told from any other refusal.
enum Sent {
    Stored,
    Permanent(u16, String),
    Retry,
}

fn send_update(client: &packset_client::PacksetClient, id: &str, fields: Value) -> Sent {
    let url = format!("{}/v1/atoms/update", client.base());
    let body = json!({
        "workspace": client.workspace(),
        "id": id,
        "fields": fields,
    });
    match packset_client::authorize(ureq::post(&url))
        .timeout(http_timeout())
        .send_json(body)
    {
        Ok(_) => Sent::Stored,
        Err(ureq::Error::Status(code, resp)) if permanent(code) => {
            let text = resp.into_string().unwrap_or_default();
            Sent::Permanent(code, text)
        }
        Err(_) => Sent::Retry,
    }
}

fn gate() -> &'static Mutex<()> {
    static GATE: Mutex<()> = Mutex::new(());
    &GATE
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::sync::{Arc, Mutex};

    use super::*;

    fn atom(kind: &str, origin: &str, entities: &[&str], source: Value) -> Value {
        json!({
            "id": "0123456789abcdef0123456789abcdef",
            "kind": kind,
            "text": "Use tabs in Go files.",
            "origin": origin,
            "entities": entities,
            "source": source,
            "ts": "2026-10-20T00:00:00Z",
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
        // No origin. The pack stamps user-declared on the way in, so a
        // body that arrives without one was not stored. It is not legacy.
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
        // via:prompt beside a session: entity is a hook write and an
        // agent mark at once. The hook tag is not promoted.
        let mut hook_and_session = hook.clone();
        hook_and_session["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!("session:read-1"));
        // v0.17–v0.29 left seat out. Harness and session are the login user.
        let mut old_pref = typed.clone();
        old_pref["source"].as_object_mut().unwrap().remove("seat");
        old_pref["id"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa5");
        let mut old_lesson = remembered.clone();
        old_lesson["source"].as_object_mut().unwrap().remove("seat");
        old_lesson["id"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa6");
        let mut old_mismatch = old_pref.clone();
        old_mismatch["source"]["session"] = json!("other");
        old_mismatch["id"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa7");
        let mut runner_lesson = runner.clone();
        runner_lesson["kind"] = json!("lesson");
        runner_lesson["id"] = json!("ffffffffffffffffffffffffffffffff");

        let fixtures = [
            (&hook, Some(Legacy::Hold)),
            (&typed, Some(Legacy::Promote)),
            (&remembered, Some(Legacy::Promote)),
            (&old_pref, Some(Legacy::Promote)),
            (&old_lesson, Some(Legacy::Promote)),
            (&hook_and_session, Some(Legacy::Hold)),
            (&runner, Some(Legacy::Hold)),
            (&runner_lesson, Some(Legacy::Hold)),
            (&old_mismatch, Some(Legacy::Hold)),
            (&turned, Some(Legacy::Hold)),
            (&ancestor, Some(Legacy::Hold)),
            (&bare, None),
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
        assert_eq!((promote, hold, left), (4, 8, 5), "fixture split");
        runner["id"] = json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        // No cutoff receipt: nothing is older yet, so nothing awaits.
        assert!(awaiting(&[hook.clone(), typed.clone(), runner.clone()]).is_empty());
        let mut late = runner.clone();
        late["id"] = json!("cccccccccccccccccccccccccccccccc");
        late["ts"] = json!("2026-10-22T00:00:00Z");
        // A fraction sorts before `Z`, so this later instant looks older
        // as a string than the cutoff of the same second.
        let mut fraction = runner.clone();
        fraction["id"] = json!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
        fraction["ts"] = json!("2026-10-21T00:00:00.100Z");
        let cutoff = json!({
            "id": "dddddddddddddddddddddddddddddddd",
            "kind": "receipt",
            "text": CUTOFF_TEXT,
            "origin": "user-declared",
            "entities": ["seat:person", CUTOFF_ENTITY],
            "ts": "2026-10-21T00:00:00Z",
        });
        assert!("2026-10-21T00:00:00.100Z" < "2026-10-21T00:00:00Z");
        let waiting = awaiting(&[
            hook,
            typed,
            runner,
            runner_lesson.clone(),
            old_lesson.clone(),
            late,
            fraction,
            cutoff,
        ]);
        assert_eq!(waiting.len(), 3, "{waiting:?}");
        assert_eq!(waiting[0].0, "0123456789abcdef0123456789abcdef");
        assert_eq!(waiting[0].1, "preference");
        assert_eq!(waiting[1].0, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!(waiting[2].0, "ffffffffffffffffffffffffffffffff");
        assert_eq!(waiting[2].1, "lesson");
        let pref = (
            "id".to_string(),
            "preference".to_string(),
            "text".to_string(),
        );
        let lesson = ("id".to_string(), "lesson".to_string(), "text".to_string());
        assert!(review_sentence(std::slice::from_ref(&pref)).contains("1 older preference awaits"));
        assert!(review_sentence(&[pref.clone(), pref.clone()]).contains("preferences await"));
        assert!(review_sentence(std::slice::from_ref(&lesson)).contains("1 older lesson awaits"));
        assert!(review_sentence(&[pref, lesson.clone()]).contains("2 older memories await"));
        assert!(review_sentence(&[lesson.clone(), lesson]).contains("ljos prefer --review"));
    }

    #[test]
    fn the_review_line_names_date_seat_and_source() {
        let with_seat = json!({
            "ts": "2026-10-20T00:00:00.000Z",
            "source": {
                "seat": "person",
                "harness": "person",
                "session": "person",
                "via": "ljos",
                "at": "2026-10-19T00:00:00Z",
            },
            "entities": ["seat:other"],
        });
        assert_eq!(shown_when(&with_seat), "2026-10-20");
        assert_eq!(shown_seat(&with_seat), "person");
        assert_eq!(shown_source(&with_seat), "ljos/person/person");

        // No ts and no seat: the writer's clock and the harness.
        let old = json!({
            "source": {
                "harness": "person",
                "session": "person",
                "via": "ljos",
                "at": "2026-10-18T01:02:03Z",
            },
            "entities": ["seat:ignored"],
        });
        assert_eq!(shown_when(&old), "2026-10-18");
        assert_eq!(shown_seat(&old), "person");
        assert_eq!(shown_source(&old), "ljos/person/person");

        let entity_only = json!({
            "entities": ["horizon:standing", "seat:claude"],
        });
        assert_eq!(shown_when(&entity_only), "-");
        assert_eq!(shown_seat(&entity_only), "claude");
        assert_eq!(shown_source(&entity_only), "-");
    }

    #[test]
    fn a_retryable_status_is_not_a_permanent_refusal() {
        assert!(permanent(400));
        assert!(permanent(409));
        assert!(permanent(422));
        assert!(!permanent(401));
        assert!(!permanent(408));
        assert!(!permanent(429));
        assert!(!permanent(500));
        assert!(!permanent(503));
    }

    /// One atom the pack refuses, one it does not answer, and one it takes.
    /// The cutoff is written anyway. The refusal is recorded and not asked
    /// again. The one that did not answer is stamped on the next run, and
    /// the one that was taken is not stamped twice.
    #[test]
    fn a_refused_update_does_not_stop_the_rest_or_retry_forever() {
        const REFUSED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const STAMPED: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        const LATER: &str = "cccccccccccccccccccccccccccccccc";
        let pref = |id: &str, text: &str| {
            json!({
                "id": id,
                "kind": "preference",
                "text": text,
                "origin": "user-declared",
                "entities": ["seat:person", "horizon:standing"],
                "source": terminal_source(),
                "ts": "2020-01-01T00:00:00Z",
            })
        };
        let (url, pack) = serve(vec![
            pref(REFUSED, "Keep the refused note in its own file."),
            pref(STAMPED, "Keep the stamped note in its own file."),
            pref(LATER, "Keep the later note in its own file."),
        ]);
        pack.lock().unwrap().refuse.push(REFUSED.to_string());
        pack.lock().unwrap().later.push(LATER.to_string());

        with_pack(&url, || {
            let err = stamp_marked().unwrap_err();
            assert!(err.to_string().contains("did not answer"), "{err}");
        });
        {
            let pack = pack.lock().unwrap();
            let cutoff = pack
                .atoms
                .iter()
                .find(|a| is_cutoff(a))
                .expect("cutoff receipt");
            let refused = refused_ids(cutoff.get("source").unwrap_or(&Value::Null));
            assert_eq!(refused, vec![REFUSED.to_string()], "{cutoff}");
            assert!(basis_of(by_id(&pack.atoms, STAMPED)).is_some());
            assert!(basis_of(by_id(&pack.atoms, REFUSED)).is_none());
            assert!(basis_of(by_id(&pack.atoms, LATER)).is_none());
            assert_eq!(pack.updates.iter().filter(|id| *id == REFUSED).count(), 1);
            assert_eq!(pack.updates.iter().filter(|id| *id == STAMPED).count(), 1);
            assert_eq!(pack.updates.iter().filter(|id| *id == LATER).count(), 1);
        }

        with_pack(&url, || {
            let n = stamp_marked().expect("the pack answers the atom it missed");
            assert_eq!(n, 1);
        });
        let pack = pack.lock().unwrap();
        assert_eq!(basis_of(by_id(&pack.atoms, LATER)), Some("person"));
        assert!(basis_of(by_id(&pack.atoms, REFUSED)).is_none());
        assert_eq!(basis_of(by_id(&pack.atoms, STAMPED)), Some("person"));
        assert_eq!(pack.updates.iter().filter(|id| *id == REFUSED).count(), 1);
        assert_eq!(pack.updates.iter().filter(|id| *id == STAMPED).count(), 1);
        assert_eq!(pack.updates.iter().filter(|id| *id == LATER).count(), 2);
        assert_eq!(
            pack.atoms.iter().filter(|a| is_cutoff(a)).count(),
            1,
            "a second run wrote another cutoff"
        );
    }

    /// The pack refuses to raise a missing basis. The preference is closed
    /// and a new atom is born `person`. A second run does not write another.
    #[test]
    fn a_basis_the_pack_will_not_raise_is_born_on_a_new_atom() {
        const OLD: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let (url, pack) = serve(vec![json!({
            "id": OLD,
            "kind": "preference",
            "text": "Keep the born note in its own file.",
            "origin": "user-declared",
            "entities": ["seat:person", "horizon:standing"],
            "source": terminal_source(),
            "ts": "2020-01-01T00:00:00Z",
        })]);
        pack.lock().unwrap().rise.push(OLD.to_string());

        with_pack(&url, || {
            let n = stamp_marked().expect("the born atom is stored");
            assert_eq!(n, 1);
        });
        {
            let pack = pack.lock().unwrap();
            let old = by_id(&pack.atoms, OLD);
            assert!(
                old.get("valid_to")
                    .and_then(Value::as_str)
                    .is_some_and(|until| !until.is_empty()),
                "{old}"
            );
            assert!(basis_of(old).is_none(), "{old}");
            let born: Vec<&Value> = pack
                .atoms
                .iter()
                .filter(|atom| {
                    atom.get("text").and_then(Value::as_str)
                        == Some("Keep the born note in its own file.")
                        && basis_of(atom) == Some("person")
                        && atom.get("id").and_then(Value::as_str) != Some(OLD)
                })
                .collect();
            assert_eq!(born.len(), 1, "{:?}", pack.atoms);
            let cutoff = pack.atoms.iter().find(|a| is_cutoff(a)).expect("cutoff");
            assert!(cutoff.pointer("/source/pending").is_none(), "{cutoff}");
            assert!(refused_ids(cutoff.get("source").unwrap_or(&Value::Null)).is_empty());
            assert_eq!(pack.updates.iter().filter(|id| *id == OLD).count(), 2);
        }

        with_pack(&url, || {
            let n = stamp_marked().expect("a second run writes nothing");
            assert_eq!(n, 0);
        });
        let pack = pack.lock().unwrap();
        let born = pack
            .atoms
            .iter()
            .filter(|atom| basis_of(atom) == Some("person") && !is_cutoff(atom))
            .count();
        assert_eq!(born, 1);
        assert_eq!(pack.updates.iter().filter(|id| *id == OLD).count(), 2);
    }

    fn by_id<'a>(atoms: &'a [Value], id: &str) -> &'a Value {
        atoms
            .iter()
            .find(|a| a.get("id").and_then(Value::as_str) == Some(id))
            .unwrap_or_else(|| panic!("no {id}"))
    }

    fn with_pack(url: &str, f: impl FnOnce()) {
        crate::with_env_mutation(|| {
            let keys = [
                "PACKSET_URL",
                "PACKSET_WORKSPACE",
                "PACKSET_AUTH",
                "PACKSET_TIMEOUT_MS",
            ];
            let prev: Vec<(&'static str, Option<String>)> = keys
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect();
            unsafe {
                std::env::set_var("PACKSET_URL", url);
                std::env::set_var("PACKSET_WORKSPACE", "seat");
                std::env::set_var("PACKSET_AUTH", "off");
                std::env::set_var("PACKSET_TIMEOUT_MS", "2000");
            }
            struct Put(Vec<(&'static str, Option<String>)>);
            impl Drop for Put {
                fn drop(&mut self) {
                    for (key, value) in &self.0 {
                        unsafe {
                            match value {
                                Some(value) => std::env::set_var(key, value),
                                None => std::env::remove_var(key),
                            }
                        }
                    }
                }
            }
            let _put = Put(prev);
            f();
        });
    }

    struct Pack {
        atoms: Vec<Value>,
        updates: Vec<String>,
        refuse: Vec<String>,
        later: Vec<String>,
        /// Updates that set `source.basis` `person` are refused as a rise.
        rise: Vec<String>,
        seq: u64,
    }

    fn serve(atoms: Vec<Value>) -> (String, Arc<Mutex<Pack>>) {
        use std::io::Write;
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let pack = Arc::new(Mutex::new(Pack {
            atoms,
            updates: Vec::new(),
            refuse: Vec::new(),
            later: Vec::new(),
            rise: Vec::new(),
            seq: 1,
        }));
        let shared = Arc::clone(&pack);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut socket) = stream else { break };
                let req = read_req(&mut socket);
                let resp = answer(&req, &shared);
                let _ = socket.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}"), pack)
    }

    fn read_req(socket: &mut impl Read) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = socket.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let mut need = 0usize;
                for line in buf[..at].split(|b| *b == b'\n') {
                    let line = std::str::from_utf8(line).unwrap_or("").trim();
                    if let Some(v) = line
                        .split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                        .map(|(_, v)| v.trim())
                    {
                        need = v.parse().unwrap_or(0);
                    }
                }
                if buf.len().saturating_sub(at + 4) >= need {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn answer(req: &str, pack: &Mutex<Pack>) -> String {
        let line = req.lines().next().unwrap_or("");
        let body = req
            .split_once("\r\n\r\n")
            .map(|(_, rest)| rest)
            .unwrap_or("");
        let (code, payload) = if line.starts_with("GET /v1/atoms") {
            let pack = pack.lock().unwrap_or_else(|e| e.into_inner());
            let live: Vec<&Value> = pack
                .atoms
                .iter()
                .filter(|atom| {
                    atom.get("valid_to")
                        .and_then(Value::as_str)
                        .is_none_or(|until| until.is_empty())
                })
                .collect();
            (200, json!({"atoms": live}).to_string())
        } else if line.starts_with("POST /v1/atoms/update") {
            let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let id = parsed
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let mut pack = pack.lock().unwrap_or_else(|e| e.into_inner());
            pack.updates.push(id.clone());
            let sets_person = parsed
                .pointer("/fields/source/basis")
                .and_then(Value::as_str)
                == Some("person");
            if pack.refuse.iter().any(|seen| seen == &id) {
                (422, r#"{"error":"refused"}"#.to_string())
            } else if let Some(at) = pack.later.iter().position(|seen| seen == &id) {
                pack.later.remove(at);
                (503, r#"{"error":"later"}"#.to_string())
            } else if sets_person && pack.rise.iter().any(|seen| seen == &id) {
                (
                    400,
                    r#"{"error":"an update cannot raise source.basis from none to person"}"#
                        .to_string(),
                )
            } else if let Some(atom) = pack
                .atoms
                .iter_mut()
                .find(|atom| atom.get("id").and_then(Value::as_str) == Some(id.as_str()))
            {
                if let Some(fields) = parsed.get("fields").and_then(Value::as_object) {
                    for (key, value) in fields {
                        atom[key] = value.clone();
                    }
                }
                (200, atom.to_string())
            } else {
                (404, r#"{"error":"missing"}"#.to_string())
            }
        } else if line.starts_with("POST /v1/atoms") {
            let mut atom: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
            let mut pack = pack.lock().unwrap_or_else(|e| e.into_inner());
            let id = if atom.get("text").and_then(Value::as_str) == Some(CUTOFF_TEXT) {
                "dddddddddddddddddddddddddddddddd".to_string()
            } else {
                let id = format!("{:032x}", pack.seq);
                pack.seq += 1;
                id
            };
            atom["id"] = json!(id);
            if atom
                .get("ts")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                atom["ts"] = json!("2026-10-21T00:00:00Z");
            }
            pack.atoms.push(atom.clone());
            (200, atom.to_string())
        } else {
            (404, r#"{"error":"unexpected"}"#.to_string())
        };
        let status = match code {
            200 => "200 OK",
            400 => "400 Bad Request",
            404 => "404 Not Found",
            422 => "422 Unprocessable Entity",
            _ => "503 Service Unavailable",
        };
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    }
}
