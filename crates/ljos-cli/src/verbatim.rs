//! A memory the pack's prose gate would have rewritten, stored as typed.
//!
//! The pack keeps a person's hard sentence and refuses an agent's. Cutting
//! the agent's sentence so the pack will take it changes the words (a comma
//! becomes a full stop). The seat keeps that sentence once, under the hash
//! of the text, and search shows one row when the same hash comes back from
//! the pack as well.

use std::path::PathBuf;

use anyhow::{Context, Result};
use packset_client::Hit;
use serde_json::Value;
use sha2::Digest as _;

use crate::is_atom_id;

/// sha256 of the text as typed, after trimming. Two memories with this
/// hash are one memory.
#[must_use]
pub(crate) fn content_hash(text: &str) -> String {
    hex::encode(sha2::Sha256::digest(text.trim().as_bytes()))
}

/// An atom id of the width [`is_atom_id`] accepts, taken from the front of
/// the content hash. The same text is the same id, so a second remember
/// finds the first.
fn id_for(text: &str) -> String {
    let hash = content_hash(text);
    let mut id = String::new();
    for c in hash.chars() {
        id.push(c);
        if is_atom_id(&id) {
            return id;
        }
    }
    id
}

fn dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
        .join("verbatim")
}

/// Store `atom` with its text unchanged and `warning` as the prose label.
/// A file already holding that text is the answer, not a second copy.
///
/// # Errors
///
/// The state directory cannot be created or the record cannot be written.
pub(crate) fn keep(atom: &Value, warning: &str) -> Result<Value> {
    let text = atom["text"].as_str().unwrap_or("").trim();
    let id = id_for(text);
    let path = dir().join(format!("{id}.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("verbatim: cannot create {}", parent.display()))?;
    }
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if let Ok(kept) = serde_json::from_str::<Value>(&existing) {
            if kept["text"].as_str().map(str::trim) == Some(text) {
                return Ok(kept);
            }
        }
    }
    let mut stored = atom.clone();
    if let Some(map) = stored.as_object_mut() {
        map.insert("id".into(), Value::String(id));
        map.insert("text".into(), Value::String(text.to_string()));
        let mut prose = map
            .get("prose")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        prose.insert("warning".into(), Value::String(warning.to_string()));
        map.insert("prose".into(), Value::Object(prose));
    }
    let bytes = serde_json::to_vec_pretty(&stored)?;
    std::fs::write(&path, bytes)
        .with_context(|| format!("verbatim: cannot write {}", path.display()))?;
    Ok(stored)
}

/// Every verbatim record on disk. A file that is not an atom is skipped.
pub(crate) fn records() -> Vec<Value> {
    let dir = dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if value["text"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty())
        {
            out.push(value);
        }
    }
    out
}

/// Drop later hits whose text hashes the same as an earlier one.
#[must_use]
pub(crate) fn dedup(hits: Vec<Hit>) -> Vec<Hit> {
    let mut seen = std::collections::BTreeSet::new();
    hits.into_iter()
        .filter(|hit| seen.insert(content_hash(&hit.text)))
        .collect()
}
