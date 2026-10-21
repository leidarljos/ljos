//! Launch slots. The seat that launches a cloud run leaves a `slot:` note
//! on the issue it hands the run: the seat the run will vote as, the model
//! it asked for and the run id, signed under the launcher's pinned key.
//! The run mints its own key on its machine, so the slot names the seat
//! before any key is pinned for it.
//!
//! A pin alone is what any tracker writer can make, so a seat the tracker
//! first pinned after slots began ([`SLOTS_SINCE`]) counts toward a push
//! cite only under a slot on that issue ([`problems`]). The seats pinned
//! before then stand as they were. A tracker that is not a git repository
//! cannot say when a pin was made, and every seat on it stands.

use anyhow::{bail, Context, Result};
use ed25519_dalek::SigningKey;
use std::collections::BTreeMap;

use crate::ballot_sig;

/// The note prefix a slot rides under.
pub const SLOT_NOTE: &str = "slot: ";

/// The commit time of the slot change. A seat whose pin the tracker first
/// committed before it stands; one pinned since is a run seat and needs
/// a slot.
pub const SLOTS_SINCE: i64 = 1_792_626_505;

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
/// the tracker did not take the note. A launcher first pinned after slots
/// began is refused too: no slot it signs would count.
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
    if pinned_at(&launcher, pinned).is_some_and(|t| t >= SLOTS_SINCE) {
        bail!(
            "slot: {launcher} was first pinned after slots began, so no slot it signs counts; a seat pinned before then launches"
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

/// Why the run seats among `seats` do not count on `issue`. A seat first
/// pinned before [`SLOTS_SINCE`] needs no slot. A later one needs a slot
/// in `notes` that names it and verifies, signed under the pin of another
/// seat pinned before then, and recorded no later than its own pin, so
/// the key the slot vouches for is one the run minted after it. With
/// `pinned_at` the first commit of a seat's pin and `noted_at` that of a
/// note, `None` from `pinned_at` is a tracker that cannot say.
#[must_use]
pub fn problems(
    issue: &str,
    seats: &[(&str, &str)],
    notes: &[String],
    pins: &BTreeMap<String, String>,
    pinned_at: &dyn Fn(&str, &str) -> Option<i64>,
    noted_at: &dyn Fn(&str) -> Option<i64>,
) -> Vec<String> {
    let since = |seat: &str| pins.get(seat).and_then(|key| pinned_at(seat, key));
    let mut out = Vec::new();
    for (voter, _) in seats {
        let Some(pinned) = since(voter).filter(|t| *t >= SLOTS_SINCE) else {
            continue;
        };
        let mut why = None;
        let mut counted = false;
        for (note, slot) in notes.iter().filter_map(|n| Some((n, parse(n)?))) {
            if slot.seat != *voter {
                continue;
            }
            let launcher = &slot.launcher;
            let refused = match pins.get(launcher) {
                _ if !verifies(issue, &slot) => Some(format!("{voter}'s slot does not verify")),
                None => Some(format!(
                    "{voter}'s slot is from {launcher}, which has no pinned key on this tracker"
                )),
                Some(key) if *key != slot.key => Some(format!(
                    "{voter}'s slot is signed by a key other than {launcher}'s pinned one"
                )),
                Some(_) if since(launcher).is_some_and(|t| t >= SLOTS_SINCE) => Some(format!(
                    "{voter}'s slot is from {launcher}, itself first pinned after slots began"
                )),
                Some(_) if noted_at(note).is_some_and(|t| t > pinned) => {
                    Some(format!("{voter}'s key was pinned before its slot"))
                }
                Some(_) => None,
            };
            match refused {
                None => {
                    counted = true;
                    break;
                }
                Some(line) => {
                    why.get_or_insert(line);
                }
            }
        }
        if !counted {
            out.push(why.unwrap_or_else(|| {
                format!(
                    "{voter} was first pinned after slots began, and no slot on {issue} names it"
                )
            }));
        }
    }
    out
}

/// When the tracker first committed `seat`'s pin `key`, by author time,
/// which a rebase keeps: now when no commit holds it yet, `None` when the
/// tracker is not a git repository.
#[must_use]
pub fn pinned_at(seat: &str, key: &str) -> Option<i64> {
    first_committed(&format!("{seat} {key}"), &["--", ballot_sig::PIN_FILE])
}

/// When the tracker first committed `note`, read as [`pinned_at`] reads a
/// pin.
#[must_use]
pub fn noted_at(note: &str) -> Option<i64> {
    first_committed(note.trim(), &[])
}

fn first_committed(needle: &str, paths: &[&str]) -> Option<i64> {
    if ballot_sig::tracker_git(&["rev-parse", "--is-inside-work-tree"])? != "true" {
        return None;
    }
    let pick = format!("-S{needle}");
    let mut args = vec!["log", "--format=%at", pick.as_str()];
    args.extend_from_slice(paths);
    let first = ballot_sig::tracker_git(&args)
        .and_then(|times| times.lines().filter_map(|t| t.trim().parse().ok()).min());
    Some(first.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }))
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

    const BEFORE: i64 = SLOTS_SINCE - 86_400;
    const AFTER: i64 = SLOTS_SINCE + 60;

    fn public(k: &SigningKey) -> String {
        hex::encode(k.verifying_key().to_bytes())
    }

    /// `ljosbot` and `inky` pinned before slots began, `mallory` and the
    /// run seat after; the run's pin lands at `AFTER + 10`, and every
    /// note at `AFTER` unless `late` names it.
    fn check(notes: &[String], late: Option<&str>) -> Vec<String> {
        let pins: BTreeMap<String, String> = [
            ("ljosbot", public(&key(1))),
            ("inky", public(&key(2))),
            ("mallory", public(&key(3))),
            ("review-r1", public(&key(4))),
        ]
        .into_iter()
        .map(|(seat, k)| (seat.to_string(), k))
        .collect();
        let pinned_at = |seat: &str, _: &str| match seat {
            "ljosbot" | "inky" => Some(BEFORE),
            "review-r1" => Some(AFTER + 10),
            _ => Some(AFTER),
        };
        let noted_at = |note: &str| {
            Some(if late == Some(note) {
                AFTER + 20
            } else {
                AFTER
            })
        };
        problems(
            "d-1",
            &[("inky", "ship"), ("review-r1", "ship")],
            notes,
            &pins,
            &pinned_at,
            &noted_at,
        )
    }

    #[test]
    fn a_run_seat_counts_under_a_slot_from_a_standing_seat() {
        let slot = sign(&key(1), "d-1", "review-r1", "grok", "r1", "ljosbot");
        assert!(check(std::slice::from_ref(&slot), None).is_empty());
        let none = check(&[], None);
        assert_eq!(
            none,
            vec!["review-r1 was first pinned after slots began, and no slot on d-1 names it"]
        );
        let other = sign(&key(1), "d-1", "review-r2", "grok", "r2", "ljosbot");
        assert_eq!(check(&[other], None), none);
    }

    #[test]
    fn a_slot_that_does_not_stand_does_not_count() {
        let refused = |note: String, late: bool| {
            let p = check(std::slice::from_ref(&note), late.then_some(note.as_str()));
            assert_eq!(p.len(), 1, "{p:?}");
            p[0].clone()
        };
        let lifted = sign(&key(1), "d-2", "review-r1", "grok", "r1", "ljosbot");
        assert!(refused(lifted, false).contains("does not verify"));
        let forged = sign(&key(3), "d-1", "review-r1", "grok", "r1", "ljosbot");
        assert!(refused(forged, false).contains("other than ljosbot's pinned one"));
        let minted = sign(&key(3), "d-1", "review-r1", "grok", "r1", "mallory");
        assert!(refused(minted, false).contains("itself first pinned after slots began"));
        let own = sign(&key(4), "d-1", "review-r1", "grok", "r1", "review-r1");
        assert!(refused(own, false).contains("itself first pinned after slots began"));
        let stranger = sign(&key(5), "d-1", "review-r1", "grok", "r1", "nobody");
        assert!(refused(stranger, false).contains("no pinned key"));
        let late = sign(&key(1), "d-1", "review-r1", "grok", "r1", "ljosbot");
        assert!(refused(late, true).contains("pinned before its slot"));
    }

    #[test]
    fn one_good_slot_beside_a_forged_one_counts() {
        let forged = sign(&key(3), "d-1", "review-r1", "grok", "r1", "ljosbot");
        let good = sign(&key(2), "d-1", "review-r1", "grok", "r1", "inky");
        assert!(check(&[forged, good], None).is_empty());
    }

    #[test]
    fn a_tracker_that_cannot_order_pins_needs_no_slot() {
        let pins: BTreeMap<String, String> = [("review-r1".to_string(), public(&key(4)))].into();
        let p = problems(
            "d-1",
            &[("review-r1", "ship")],
            &[],
            &pins,
            &|_, _| None,
            &|_| None,
        );
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn a_note_that_is_not_a_slot_does_not_parse() {
        assert_eq!(parse("slot: review model=m run=r ljosbot ab"), None);
        assert_eq!(parse("slot: review m r ljosbot ab cd"), None);
        assert_eq!(parse("ballot-sig: a ship ab cd"), None);
    }
}
