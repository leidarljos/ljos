//! Signed seat ballots. A seat signs each ballot it casts with its own
//! ed25519 key, made on its first vote at
//! `$XDG_DATA_HOME/ljos/seats/SEAT/ballot.key` (or `LJOS_SEAT_KEY`), so
//! seats sharing one HOME still sign apart. The signature rides on the
//! issue as a `ballot-sig:` note. A ballot the tracker committed before
//! signing began ([`SIGNING_SINCE`]) is legacy unsigned and still counts. The tracker
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

/// The commit time of the signing change: a ballot the tracker recorded
/// before it is legacy unsigned and still counts; one after needs a
/// signature.
pub const SIGNING_SINCE: i64 = 1_791_984_778;

/// Where `seat`'s ballot key lives: `LJOS_SEAT_KEY` when set, else
/// `$XDG_DATA_HOME/ljos/seats/SEAT/ballot.key` (`~/.local/share` when unset).
/// Seats that share one HOME still get one key each.
#[must_use]
pub fn seat_key_path(seat: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LJOS_SEAT_KEY").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let name: String = seat
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    Some(
        base.join("ljos")
            .join("seats")
            .join(name)
            .join("ballot.key"),
    )
}

/// The key at `path`, made on first use: 32 random bytes, mode 0600.
///
/// # Errors
///
/// The key cannot be read or written.
pub fn load_or_create(path: &std::path::Path) -> Result<SigningKey> {
    if !path.exists() {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let mut seed = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut seed)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("seat key {}", path.display()))?;
        std::io::Write::write_all(&mut f, &seed)?;
    }
    key_from_file(path)
}

/// When the tracker recorded `voter`'s ballot for `choice` on `issue`: the
/// newest commit that added or removed its `] VOTER: CHOICE` line in the
/// file holding the issue. `None` when the tracker is not a git repository
/// or the ballot is not committed yet.
#[must_use]
pub fn recorded_at(issue: &str, voter: &str, choice: &str) -> Option<i64> {
    let root = std::env::var_os("VISSUE_ROOT").filter(|r| !r.is_empty())?;
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let pattern = format!(":ID:[[:space:]]+{issue}[[:space:]]*$");
    let files = git(&["grep", "-l", "-E", &pattern])?;
    let file = files.lines().next()?.to_string();
    let needle = format!("] {}: {}", voter.trim(), choice.trim());
    git(&["log", "-1", "--format=%ct", "-S", &needle, "--", &file])?
        .parse()
        .ok()
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
    let path = seat_key_path(voter)
        .ok_or_else(|| anyhow::anyhow!("no place for a seat key: HOME and XDG_DATA_HOME are unset; the ballot is unsigned and no gate counts it"))?;
    let key = load_or_create(&path)?;
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
    seat_check(issue, seats, notes, pins, &|_, _| false).0
}

/// [`seat_problems`] with grandfathering: an unsigned ballot `legacy`
/// says the tracker recorded before signing began counts, and is named in
/// the second list as legacy unsigned.
#[must_use]
pub fn seat_check(
    issue: &str,
    seats: &[(&str, &str)],
    notes: &[String],
    pins: &BTreeMap<String, String>,
    legacy: &dyn Fn(&str, &str) -> bool,
) -> (Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    let mut old = Vec::new();
    for (voter, choice) in seats {
        let newest = notes
            .iter()
            .filter_map(|n| parse_note(n))
            .find(|n| n.voter == *voter);
        match newest {
            None if legacy(voter, choice) => old.push((*voter).to_string()),
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
    (out, old)
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
    fn an_old_unsigned_ballot_is_legacy_and_a_new_one_is_not() {
        let pins = BTreeMap::new();
        let legacy = |v: &str, _: &str| v == "inky";
        let (bad, old) = seat_check(
            "d-1",
            &[("inky", "ship"), ("bob", "ship")],
            &[],
            &pins,
            &legacy,
        );
        assert_eq!(old, vec!["inky".to_string()]);
        assert_eq!(bad, vec!["bob's ballot is unsigned".to_string()]);
    }

    #[test]
    fn seats_sharing_a_home_get_their_own_keys() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("ljos/seats/inky/ballot.key");
        let b = dir.path().join("ljos/seats/disk-saver/ballot.key");
        let ka = load_or_create(&a).unwrap();
        let kb = load_or_create(&b).unwrap();
        assert_ne!(ka.verifying_key(), kb.verifying_key());
        assert_eq!(
            load_or_create(&a).unwrap().verifying_key(),
            ka.verifying_key()
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&a).unwrap().permissions().mode() & 0o777,
            0o600
        );
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
