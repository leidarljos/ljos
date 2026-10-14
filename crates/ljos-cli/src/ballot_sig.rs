//! Signed seat ballots. A seat signs each ballot it casts with its deedar
//! host key (`DEEDAR_HOST_SIGNING_KEY` or `~/.config/deedar/host.key`), and
//! the signature rides on the issue as a `ballot-sig:` note. The tracker
//! pins each seat's public key the first time it signs (`.ljos-seat-keys`
//! at the tracker root, committed), so a later ballot under that name signed
//! by another key is a mismatch. The push gate counts a seat ballot only
//! when its latest signature covers exactly that voter, issue and choice
//! under the pinned key.

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The note prefix a signature rides under.
pub const SIG_NOTE: &str = "ballot-sig: ";
/// The pin file at the tracker root.
pub const PIN_FILE: &str = ".ljos-seat-keys";

/// The bytes a ballot signature covers.
#[must_use]
pub fn message(issue: &str, voter: &str, choice: &str) -> Vec<u8> {
    format!(
        "ljos-ballot/v1\n{}\n{}\n{}\n",
        issue.trim(),
        voter.trim(),
        choice.trim()
    )
    .into_bytes()
}

/// A signature note's fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigNote {
    pub voter: String,
    pub choice: String,
    pub key: String,
    pub sig: String,
}

/// `ballot-sig: VOTER CHOICE KEYHEX SIGHEX`.
#[must_use]
pub fn parse_note(note: &str) -> Option<SigNote> {
    let rest = note.trim().strip_prefix(SIG_NOTE)?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let [voter, choice, key, sig] = parts.as_slice() else {
        return None;
    };
    Some(SigNote {
        voter: (*voter).to_string(),
        choice: (*choice).to_string(),
        key: (*key).to_string(),
        sig: (*sig).to_string(),
    })
}

fn key_from_file(path: &std::path::Path) -> Result<SigningKey> {
    let raw = std::fs::read(path).with_context(|| format!("seat key {}", path.display()))?;
    let seed: [u8; 32] = raw
        .get(..32)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| anyhow::anyhow!("seat key {} holds fewer than 32 bytes", path.display()))?;
    Ok(SigningKey::from_bytes(&seed))
}

/// The note text for a ballot signed by `key`.
#[must_use]
pub fn sign_with(key: &SigningKey, issue: &str, voter: &str, choice: &str) -> String {
    let sig = key.sign(&message(issue, voter, choice));
    format!(
        "{SIG_NOTE}{} {} {} {}",
        voter.trim(),
        choice.trim(),
        hex::encode(key.verifying_key().to_bytes()),
        hex::encode(sig.to_bytes())
    )
}

/// Whether `note` is a valid signature over this issue's ballot.
#[must_use]
pub fn verifies(issue: &str, note: &SigNote) -> bool {
    let (Ok(k), Ok(s)) = (hex::decode(&note.key), hex::decode(&note.sig)) else {
        return false;
    };
    let (Ok(k), Ok(s)) = (<[u8; 32]>::try_from(k), <[u8; 64]>::try_from(s)) else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_bytes(&k) else {
        return false;
    };
    vk.verify(
        &message(issue, &note.voter, &note.choice),
        &Signature::from_bytes(&s),
    )
    .is_ok()
}

/// The tracker root the pins live at.
fn pin_path() -> Option<PathBuf> {
    let root = std::env::var_os("VISSUE_ROOT").filter(|r| !r.is_empty())?;
    Some(PathBuf::from(root).join(PIN_FILE))
}

/// `voter keyhex` lines.
#[must_use]
pub fn parse_pins(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

/// The pinned keys, empty when there is no pin file.
#[must_use]
pub fn pins() -> BTreeMap<String, String> {
    pin_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_pins(&t))
        .unwrap_or_default()
}

/// Sign this seat's ballot and leave the note on the issue, pinning the key
/// on first use. A seat whose key differs from its pin is refused, and the
/// ballot stays unsigned (so the gate will not count it).
///
/// # Errors
///
/// No seat key, a key that does not match the pin, or a tracker write that
/// failed.
pub fn sign_ballot(issue: &str, voter: &str, choice: &str) -> Result<String> {
    let path = crate::host_key_path_pub()
        .ok_or_else(|| anyhow::anyhow!("no seat key: set DEEDAR_HOST_SIGNING_KEY or create ~/.config/deedar/host.key; the ballot is unsigned and no gate counts it"))?;
    let key = key_from_file(&path)?;
    let mine = hex::encode(key.verifying_key().to_bytes());
    let mut pinned = pins();
    match pinned.get(voter.trim()) {
        Some(k) if *k != mine => bail!(
            "seat {voter} is pinned to key {}…, and this key is {}…; the ballot is unsigned and no gate counts it",
            &k[..12.min(k.len())],
            &mine[..12]
        ),
        Some(_) => {}
        None => {
            if let Some(p) = pin_path() {
                pinned.insert(voter.trim().to_string(), mine.clone());
                let text: String = pinned.iter().map(|(v, k)| format!("{v} {k}\n")).collect();
                std::fs::write(&p, text)?;
                if let Some(root) = p.parent() {
                    let _ = std::process::Command::new("git")
                        .arg("-C")
                        .arg(root)
                        .args(["add", PIN_FILE])
                        .output();
                }
            }
        }
    }
    let note = sign_with(&key, issue, voter, choice);
    crate::run_captured("vissue", &["note", issue, &note])?;
    Ok(format!(
        "ballot signed by {voter} with key {}…\n",
        &mine[..12]
    ))
}

/// Problems with the seat ballots on `issue`: each `(voter, choice)` needs
/// its newest signature note to name that choice, verify, and use the
/// pinned key. `notes` are the issue's logbook notes, newest first.
#[must_use]
pub fn seat_problems(
    issue: &str,
    seats: &[(&str, &str)],
    notes: &[String],
    pins: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (voter, choice) in seats {
        let newest = notes
            .iter()
            .filter_map(|n| parse_note(n))
            .find(|n| n.voter == *voter);
        match newest {
            None => out.push(format!("{voter}'s ballot is unsigned")),
            Some(n) if n.choice != *choice => out.push(format!(
                "{voter}'s signature is for {} and the ballot says {choice}",
                n.choice
            )),
            Some(n) if !verifies(issue, &n) => {
                out.push(format!("{voter}'s signature does not verify"));
            }
            Some(n) => match pins.get(*voter) {
                Some(k) if *k == n.key => {}
                Some(_) => out.push(format!(
                    "{voter}'s ballot is signed by a key other than its pinned one"
                )),
                None => out.push(format!("{voter} has no pinned key on this tracker")),
            },
        }
    }
    out
}

/// The logbook notes of an issue JSON, newest first.
#[must_use]
pub fn notes_of(v: &Value) -> Vec<String> {
    v["logbook"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["note"].as_str().map(str::to_string))
        .collect()
}

/// Whether unsigned seat ballots are let through on purpose
/// (`LJOS_UNSIGNED_BALLOTS=allow`), for a tracker from before signing.
#[must_use]
pub fn unsigned_allowed() -> bool {
    std::env::var("LJOS_UNSIGNED_BALLOTS").is_ok_and(|v| v == "allow")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> SigningKey {
        SigningKey::from_bytes(&[b; 32])
    }

    #[test]
    fn a_signed_pinned_ballot_counts_and_every_forgery_does_not() {
        let alice = key(1);
        let mallory = key(2);
        let pins: BTreeMap<String, String> = [(
            "alice".to_string(),
            hex::encode(alice.verifying_key().to_bytes()),
        )]
        .into();
        let good = sign_with(&alice, "d-1", "alice", "ship");
        assert!(seat_problems(
            "d-1",
            &[("alice", "ship")],
            std::slice::from_ref(&good),
            &pins
        )
        .is_empty());
        // Unsigned.
        let p = seat_problems("d-1", &[("alice", "ship")], &[], &pins);
        assert!(p[0].contains("unsigned"), "{p:?}");
        // Signed by another key under alice's name.
        let forged = sign_with(&mallory, "d-1", "alice", "ship");
        let p = seat_problems("d-1", &[("alice", "ship")], &[forged], &pins);
        assert!(p[0].contains("other than its pinned"), "{p:?}");
        // The ballot changed after signing.
        let p = seat_problems(
            "d-1",
            &[("alice", "hold")],
            std::slice::from_ref(&good),
            &pins,
        );
        assert!(p[0].contains("is for ship"), "{p:?}");
        // A signature lifted from another issue.
        let p = seat_problems("d-2", &[("alice", "ship")], &[good], &pins);
        assert!(p[0].contains("does not verify"), "{p:?}");
        // No pin on this tracker.
        let lone = sign_with(&mallory, "d-1", "bob", "ship");
        let p = seat_problems("d-1", &[("bob", "ship")], &[lone], &pins);
        assert!(p[0].contains("no pinned key"), "{p:?}");
    }

    #[test]
    fn the_newest_signature_wins() {
        let a = key(3);
        let pins: BTreeMap<String, String> =
            [("a".to_string(), hex::encode(a.verifying_key().to_bytes()))].into();
        let new = sign_with(&a, "d-1", "a", "hold");
        let old = sign_with(&a, "d-1", "a", "ship");
        assert!(seat_problems("d-1", &[("a", "hold")], &[new, old], &pins).is_empty());
    }
}
