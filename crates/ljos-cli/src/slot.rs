//! Launch slots. The seat that launches a cloud run leaves a `slot:` note
//! on the issue it hands the run: the seat the run will vote as, the model
//! it asked for and the run id, signed under the launcher's pinned key.
//! The run mints its own key on its machine, so the slot names the seat
//! before any key is pinned for it.

use anyhow::{bail, Context, Result};
use ed25519_dalek::SigningKey;

use crate::ballot_sig;

/// The note prefix a slot rides under.
pub const SLOT_NOTE: &str = "slot: ";

/// A slot note's fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub seat: String,
    pub model: String,
    pub run: String,
    pub launcher: String,
    pub key: String,
    pub sig: String,
}

/// The bytes a slot signature covers. The issue is in them, so a slot
/// copied to another issue does not verify there.
#[must_use]
pub fn message(issue: &str, seat: &str, model: &str, run: &str, launcher: &str) -> Vec<u8> {
    format!(
        "ljos-slot/v1\n{}\n{}\n{}\n{}\n{}\n",
        issue.trim(),
        seat.trim(),
        model.trim(),
        run.trim(),
        launcher.trim()
    )
    .into_bytes()
}

/// `slot: SEAT model=MODEL run=RUN LAUNCHER KEYHEX SIGHEX`.
#[must_use]
pub fn parse(note: &str) -> Option<Slot> {
    let rest = note.trim().strip_prefix(SLOT_NOTE)?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let [seat, model, run, launcher, key, sig] = parts.as_slice() else {
        return None;
    };
    Some(Slot {
        seat: (*seat).to_string(),
        model: model.strip_prefix("model=")?.to_string(),
        run: run.strip_prefix("run=")?.to_string(),
        launcher: (*launcher).to_string(),
        key: (*key).to_string(),
        sig: (*sig).to_string(),
    })
}

/// Whether `slot` is a valid signature by its key over this issue's slot.
#[must_use]
pub fn verifies(issue: &str, slot: &Slot) -> bool {
    ballot_sig::signature_ok(
        &slot.key,
        &slot.sig,
        &message(issue, &slot.seat, &slot.model, &slot.run, &slot.launcher),
    )
}

/// The note for a slot signed by `key`. Each field is one token.
#[must_use]
pub fn sign(
    key: &SigningKey,
    issue: &str,
    seat: &str,
    model: &str,
    run: &str,
    launcher: &str,
) -> String {
    let [seat, model, run, launcher] = [seat, model, run, launcher].map(ballot_sig::field);
    let (public, sig) =
        ballot_sig::signed_hex(key, &message(issue, &seat, &model, &run, &launcher));
    format!("{SLOT_NOTE}{seat} model={model} run={run} {launcher} {public} {sig}")
}

/// Sign a slot on `issue` for run `run` on `model`, as this process's
/// seat, and leave it on the issue. `seat` is the `LJOS_SEAT` the run
/// boots with; the run id is appended the way `ljos cloud up` appends it.
///
/// # Errors
///
/// No seat names the launcher, the launcher has no pinned key or holds
/// another one, the run's seat is the launcher or is already pinned, or
/// the tracker did not take the note.
pub fn write(issue: &str, seat: &str, model: &str, run: &str) -> Result<String> {
    let launcher = ballot_sig::own_voter()
        .context("slot: no seat to sign as; set LJOS_SEAT to the launching seat")?;
    let seat = crate::cloud::run_seat(seat, run);
    if seat == launcher {
        bail!("slot: {seat} is the launching seat, and a seat does not slot itself");
    }
    let pins = ballot_sig::pins();
    let Some(pinned) = pins.get(&launcher) else {
        bail!(
            "slot: {launcher} has no pinned key on this tracker; a slot is signed under the key its ballots pinned"
        );
    };
    if pins.contains_key(&seat) {
        bail!(
            "slot: {seat} is already pinned, so its key is not one the run minted; slot a seat before its first ballot"
        );
    }
    let path = ballot_sig::seat_key_path(&launcher)
        .context("slot: no place for a seat key: HOME and XDG_DATA_HOME are unset")?;
    let key = ballot_sig::key_from_file(&path)?;
    let mine = hex::encode(key.verifying_key().to_bytes());
    if mine != *pinned {
        bail!(
            "slot: {launcher} is pinned to key {}…, and this key is {}…",
            &pinned[..12.min(pinned.len())],
            &mine[..12]
        );
    }
    let note = sign(&key, issue, &seat, model, run, &launcher);
    if ballot_sig::write_sig_note(issue, &note).is_err() {
        crate::run_captured("vissue", &["note", issue, &note])?;
    }
    Ok(format!(
        "slot: {seat} on {issue} for run {}, model {}, signed by {launcher} with key {}…\n",
        ballot_sig::field(run),
        ballot_sig::field(model),
        &mine[..12]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> SigningKey {
        SigningKey::from_bytes(&[b; 32])
    }

    #[test]
    fn a_slot_parses_whole_and_verifies_on_its_issue_only() {
        let note = sign(
            &key(1),
            "d-1",
            "review-cafebabe0001abcd",
            "grok 4",
            "bc-cafebabe0001abcd",
            "ljosbot",
        );
        let slot = parse(&note).expect("a signed slot parses");
        assert_eq!(slot.seat, "review-cafebabe0001abcd");
        assert_eq!(slot.model, "grok_4");
        assert_eq!(slot.run, "bc-cafebabe0001abcd");
        assert_eq!(slot.launcher, "ljosbot");
        assert!(verifies("d-1", &slot));
        assert!(
            !verifies("d-2", &slot),
            "a slot lifted to another issue verified"
        );
        let mut moved = slot.clone();
        moved.model = "opus".into();
        assert!(!verifies("d-1", &moved), "a rewritten model still verified");
        let mut renamed = slot;
        renamed.seat = "review-other".into();
        assert!(
            !verifies("d-1", &renamed),
            "a rewritten seat still verified"
        );
    }

    #[test]
    fn a_note_that_is_not_a_slot_does_not_parse() {
        assert_eq!(parse("slot: review model=m run=r ljosbot ab"), None);
        assert_eq!(parse("slot: review m r ljosbot ab cd"), None);
        assert_eq!(parse("ballot-sig: a ship ab cd"), None);
    }
}
