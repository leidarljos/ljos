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
//! `origin` on a live atom is `user-declared`, `agent-derived` or `peer`.
//! A 400 that names `origin` as unknown is retried once without the field.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use packset_client::PacksetClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ORIGIN_USER: &str = "user-declared";
pub const ORIGIN_AGENT: &str = "agent-derived";
pub const ORIGIN_PEER: &str = "peer";

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
    let text = std::fs::read_to_string(proposals_path()).unwrap_or_default();
    let mut by_id = BTreeMap::new();
    for line in text.lines() {
        if let Ok(rec) = serde_json::from_str::<Record>(line) {
            by_id.insert(rec.id.clone(), rec);
        }
    }
    by_id
}

fn append(rec: &Record) -> Result<()> {
    let path = proposals_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(rec)?;
    line.push('\n');
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
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

/// File `atom` as an agent proposal by posting it.
///
/// A held answer is the proposal. The id is stable for one kind and one
/// text, so the same lesson is one open proposal. An open, accepted or
/// satisfied row is returned as it stands.
pub fn propose_atom(client: &PacksetClient, mut atom: Value) -> Result<Filed> {
    let _g = gate();
    let text = atom["text"].as_str().unwrap_or("").trim().to_string();
    let kind = atom["kind"].as_str().unwrap_or("lesson").to_string();
    if text.is_empty() {
        bail!("propose: empty text is not a proposal");
    }
    stamp_origin(&mut atom, ORIGIN_AGENT);
    let id = crate::work_id(&format!("{kind}\n{text}"));
    if let Some(existing) = read_latest().get(&id) {
        if matches!(existing.status.as_str(), "open" | "accepted" | "satisfied") {
            return Ok(Filed {
                id,
                remote: existing.remote,
            });
        }
    }
    let remote_id = post_direct(client, &atom)?;
    let rec = Record {
        id: id.clone(),
        status: "open".into(),
        remote: remote_id.is_some(),
        remote_id,
        text,
        kind,
        atom,
    };
    let remote = rec.remote;
    append(&rec)?;
    Ok(Filed { id, remote })
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
        let _ = append(&rec);
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

fn accept_on_writer(client: &PacksetClient, workspace: &str, id: &str) -> Result<Value> {
    let url = format!("{}/v1/proposals/accept", client.base());
    let body = serde_json::json!({"workspace": workspace, "id": id});
    match ureq::post(&url).timeout(http_timeout()).send_json(body) {
        Ok(resp) => resp
            .into_json()
            .context("accept: the pack's answer was not json"),
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            bail!("accept: POST /v1/proposals/accept failed: {code}: {text}");
        }
        Err(e) => bail!("accept: POST /v1/proposals/accept failed: {e}"),
    }
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
