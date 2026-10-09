//! Origin-bound admission.
//!
//! A direct proposal, for a writer that grew the path beside the miner:
//!
//! ```json
//! {"schema":"inside.proposal/v1","id":"<stable>","workspace":"...","text":"...","kind":"lesson","origin":"agent-derived","level":"explicit","entities":[],"source":{},"direct":true}
//! ```
//!
//! `POST /v1/proposals` with `direct: true`. A 200 keeps it on the writer and
//! `POST /v1/proposals/accept` with `{"workspace","id"}` writes the atom.
//! A 403 whose body says `not allowed`, or a 404, means this writer still
//! mines and will not take a direct proposal. The proposal stays in
//! `$XDG_STATE_HOME/ljos/proposals.jsonl` and `ljos accept` posts the atom.
//! A live atom is never the fallback.
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

/// `Ok(None)` is a writer that still mines. `Ok(Some(id))` is the writer's id.
fn post_direct(client: &PacksetClient, id: &str, atom: &Value) -> Result<Option<String>> {
    let base = client.base();
    if proposal_is_local(base) {
        return Ok(None);
    }
    let url = format!("{base}/v1/proposals");
    let mut body = atom.clone();
    if let Some(map) = body.as_object_mut() {
        map.insert("schema".into(), Value::String("inside.proposal/v1".into()));
        map.insert("direct".into(), Value::Bool(true));
        map.insert("id".into(), Value::String(id.to_string()));
    }
    match ureq::post(&url).timeout(http_timeout()).send_json(body) {
        Ok(resp) => {
            let parsed: Value = resp.into_json().unwrap_or(Value::Null);
            remember_proposal_mode(base, true);
            let remote_id = parsed.get("id").and_then(Value::as_str).map(str::to_string);
            Ok(Some(remote_id.unwrap_or_else(|| id.to_string())))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let lower = text.to_lowercase();
            if code == 404 || (code == 403 && lower.contains("not allowed")) {
                remember_proposal_mode(base, false);
                return Ok(None);
            }
            bail!("propose: POST /v1/proposals failed: {code}: {text}");
        }
        Err(e) => bail!("propose: POST /v1/proposals failed: {e}"),
    }
}

/// File `atom` as an agent proposal. The atom is not posted.
///
/// The id is stable for one kind and one text, so the same lesson is one
/// open proposal. An open, accepted or satisfied row is returned as it stands.
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
    let remote_id = post_direct(client, &id, &atom)?;
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
    }
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
            return Ok(format!("{id} was already written by remember or prefer\n"));
        }
        "accepted" => return Ok(format!("{id} is already in the pack\n")),
        "open" => {}
        _ => bail!("accept: no open proposal {id}"),
    }
    let client = crate::pack()?;
    if rec.remote {
        let remote_id = rec.remote_id.clone().unwrap_or_else(|| rec.id.clone());
        let workspace = rec
            .atom
            .get("workspace")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let workspace = if workspace.is_empty() {
            client.workspace()
        } else {
            workspace
        };
        accept_on_writer(&client, &workspace, &remote_id)?;
    } else {
        let posted = post_kept(&client, &rec.atom)
            .with_context(|| format!("accept: POST /v1/atoms failed for {id}"))?;
        rec.status = "accepted".into();
        append(&rec)?;
        return Ok(format!(
            "accepted {id} as {} origin {ORIGIN_AGENT}{}\n",
            rec.kind,
            crate::revision_note(&posted)
        ));
    }
    rec.status = "accepted".into();
    append(&rec)?;
    Ok(format!(
        "accepted {id} as {} origin {ORIGIN_AGENT}\n",
        rec.kind
    ))
}
