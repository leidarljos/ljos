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

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
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
/// A pack that does not answer is left for the next run.
pub fn migrate_once() {
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
        if stamp_marked().is_ok() {
            let _ = record();
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
/// or not a terminal, or the pack refused the update.
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
    let ids: Vec<String> = rows.iter().map(|(id, _)| id.clone()).collect();
    let id = crate::resolve_short_id("prefer", id.trim(), &ids)?;
    let Some(atom) = atoms
        .iter()
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
    stamp_basis(&client, atom).with_context(|| format!("prefer: could not keep {id}"))?;
    Ok(format!("kept {id} as the person's preference\n"))
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

fn stamp_marked() -> Result<usize> {
    let client = pack()?;
    let atoms = atoms_lean(&client, &client.workspace())?;
    let mut n = 0;
    for atom in &atoms {
        if legacy(atom) != Some(Legacy::Promote) {
            continue;
        }
        stamp_basis(&client, atom)?;
        n += 1;
    }
    Ok(n)
}

fn stamp_basis(client: &packset_client::PacksetClient, atom: &Value) -> Result<()> {
    let id = atom
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .context("legacy preference has no id")?;
    let mut source = atom
        .get("source")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    source["basis"] = json!("person");
    let url = format!("{}/v1/atoms/update", client.base());
    let body = json!({
        "workspace": client.workspace(),
        "id": id,
        "fields": {"source": source},
    });
    let timeout = http_timeout();
    match packset_client::authorize(ureq::post(&url))
        .timeout(timeout)
        .send_json(body)
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            bail!("POST /v1/atoms/update failed: {code}: {text}");
        }
        Err(e) => bail!("POST /v1/atoms/update failed: {e}"),
    }
}

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

fn record() -> Result<()> {
    let Some(path) = marker_path() else {
        return Ok(());
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
            .write_all(b"once\n")
            .context("recording the preference migration"),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
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
