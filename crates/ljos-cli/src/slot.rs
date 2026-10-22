//! Launch slots. The seat that launches a cloud run leaves a `slot:` note
//! on the issue it hands the run: the seat the run will vote as, the model
//! it asked for, the run id and the public key the run minted on its
//! machine, signed under the launcher's pinned key. The slot names the
//! seat and its key before the tracker pins any key for it, so a writer
//! that pins the seat first under another key does not vote as the run.
//!
//! A pin alone is what any tracker writer can make, and the author date on
//! that commit is whatever the writer sets. A seat stands when its pinned
//! key was already committed when slots began in this tracker ([`stood`]):
//! in the parents of the commit that brought the first slot note, by
//! commit order. On a tracker that is not a git repository the names on a
//! list in this binary stand instead ([`standing`]). Any other seat counts
//! toward a push cite only under a slot on that issue ([`problems`]). The
//! slot has to already be in that issue's own entry in the commit that
//! introduced the pin. A copy of the line on another issue does not
//! count, and a git date is not read.

use anyhow::{bail, Context, Result};
use ed25519_dalek::SigningKey;
use std::collections::BTreeMap;

use crate::ballot_sig;

/// The note prefix a slot rides under.
pub const SLOT_NOTE: &str = "slot: ";

/// When the standing list was frozen: the slot change, 2026-10-21T23:48:25Z.
/// The list is the policy. This instant is not compared with a git time.
pub const SLOTS_SINCE: i64 = 1_792_626_505;

/// Seats that stood at [`SLOTS_SINCE`]. One name per line. A `#` line is a
/// comment.
const STANDING_SEATS: &str = include_str!("standing_seats.txt");

/// Whether `seat` is on the list that stands on a tracker that is not a
/// git repository ([`stands`]).
#[must_use]
pub fn standing(seat: &str) -> bool {
    let seat = seat.trim();
    !seat.is_empty()
        && STANDING_SEATS.lines().any(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#') && line == seat
        })
}

/// The text a slot note carries in a tracker file.
const SLOT_LINE: &str = "Note: \"slot: ";

/// Where slots began in this tracker, by commit order: the parents of each
/// commit that brought a slot note into a tree none of its parents held
/// one in. Empty when no slot is committed yet. `None` when the tracker is
/// not a git repository.
#[must_use]
pub fn starts() -> Option<Vec<Vec<String>>> {
    if ballot_sig::tracker_git(&["rev-parse", "--is-inside-work-tree"])? != "true" {
        return None;
    }
    let pick = format!("-S{SLOT_LINE}");
    let log = ballot_sig::tracker_git(&[
        "--no-pager",
        "log",
        "--full-history",
        "--format=%H %P",
        pick.as_str(),
    ])
    .unwrap_or_default();
    let held = |commit: &str| {
        ballot_sig::tracker_git(&["--no-pager", "grep", "-q", "-F", "-e", SLOT_LINE, commit])
            .is_some()
    };
    let mut out = Vec::new();
    for line in log.lines() {
        let mut parts = line.split_whitespace();
        let Some(commit) = parts.next() else {
            continue;
        };
        let parents: Vec<String> = parts.map(str::to_string).collect();
        if held(commit) && !parents.iter().any(|p| held(p)) {
            out.push(parents);
        }
    }
    Some(out)
}

/// Whether `seat`'s pin `key` was committed before slots began: its line
/// is in the pin file of every parent in `starts`. A slot in a root commit
/// leaves nothing before it. With no slot committed yet, a pin committed at
/// `HEAD` stood.
#[must_use]
pub fn stood(starts: &[Vec<String>], seat: &str, key: &str) -> bool {
    if starts.is_empty() {
        return pin_line_at("HEAD", seat, key);
    }
    starts
        .iter()
        .all(|parents| !parents.is_empty() && parents.iter().all(|p| pin_line_at(p, seat, key)))
}

/// Whether `seat`, pinned to `key`, needs no slot and may sign one: its
/// pin [`stood`]. A name alone does not stand on a git tracker, since the
/// first writer to sign as an unpinned name pins it. `starts` is `None` on
/// a tracker that is not a git repository, which keeps no order, and there
/// the list stands.
#[must_use]
pub fn stands(seat: &str, key: Option<&str>, starts: Option<&[Vec<String>]>) -> bool {
    match starts {
        None => standing(seat),
        Some(at) => key.is_some_and(|key| stood(at, seat, key)),
    }
}

/// A slot note's fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub seat: String,
    pub model: String,
    pub run: String,
    /// The public key the run minted, as hex. Its ballots count only when
    /// the tracker pins this key for `seat`.
    pub run_key: String,
    pub launcher: String,
    pub key: String,
    pub sig: String,
}

/// This tracker as a slot names it: its root commits, sorted and joined
/// with `,`. `-` on a tracker that is not a git repository.
#[must_use]
pub fn tracker_id() -> String {
    let roots = ballot_sig::tracker_git(&["rev-list", "--max-parents=0", "HEAD"]);
    let mut roots: Vec<&str> = roots.as_deref().unwrap_or("").split_whitespace().collect();
    roots.sort_unstable();
    if roots.is_empty() {
        "-".into()
    } else {
        roots.join(",")
    }
}

/// The bytes a slot signature covers. The tracker and the issue are in
/// them, so a slot copied to another issue, or to the same issue id on
/// another tracker, does not verify there.
#[must_use]
pub fn message(
    tracker: &str,
    issue: &str,
    seat: &str,
    model: &str,
    run: &str,
    run_key: &str,
    launcher: &str,
) -> Vec<u8> {
    format!(
        "ljos-slot/v3\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        tracker.trim(),
        issue.trim(),
        seat.trim(),
        model.trim(),
        run.trim(),
        run_key.trim(),
        launcher.trim()
    )
    .into_bytes()
}

/// `slot: SEAT model=MODEL run=RUN key=RUNKEY LAUNCHER KEYHEX SIGHEX`. A
/// slot without the run's key does not parse.
#[must_use]
pub fn parse(note: &str) -> Option<Slot> {
    let rest = note.trim().strip_prefix(SLOT_NOTE)?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let [seat, model, run, run_key, launcher, key, sig] = parts.as_slice() else {
        return None;
    };
    Some(Slot {
        seat: (*seat).to_string(),
        model: model.strip_prefix("model=")?.to_string(),
        run: run.strip_prefix("run=")?.to_string(),
        run_key: run_key.strip_prefix("key=")?.to_string(),
        launcher: (*launcher).to_string(),
        key: (*key).to_string(),
        sig: (*sig).to_string(),
    })
}

/// Whether `slot` is a valid signature by its key over this issue's slot
/// on `tracker` ([`tracker_id`]).
#[must_use]
pub fn verifies(tracker: &str, issue: &str, slot: &Slot) -> bool {
    ballot_sig::signature_ok(
        &slot.key,
        &slot.sig,
        &message(
            tracker,
            issue,
            &slot.seat,
            &slot.model,
            &slot.run,
            &slot.run_key,
            &slot.launcher,
        ),
    )
}

/// The note for a slot on `tracker` ([`tracker_id`]) signed by `key`.
/// Each field is one token.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn sign(
    key: &SigningKey,
    tracker: &str,
    issue: &str,
    seat: &str,
    model: &str,
    run: &str,
    run_key: &str,
    launcher: &str,
) -> String {
    let [seat, model, run, run_key, launcher] =
        [seat, model, run, run_key, launcher].map(ballot_sig::field);
    let (public, sig) = ballot_sig::signed_hex(
        key,
        &message(tracker, issue, &seat, &model, &run, &run_key, &launcher),
    );
    format!("{SLOT_NOTE}{seat} model={model} run={run} key={run_key} {launcher} {public} {sig}")
}

/// Sign a slot on `issue` for run `run` on `model`, as this process's
/// seat, and leave it on the issue. `seat` is the `LJOS_SEAT` the run
/// boots with; the run id is appended the way `ljos cloud up` appends it.
/// `run_key` is the public key `ljos cloud up` printed on the run.
///
/// # Errors
///
/// No seat names the launcher, the launcher has no pinned key or holds
/// another one, the run's seat is the launcher or is already pinned,
/// `run_key` is not a public key, or the tracker did not take the note. A
/// launcher that is not a standing seat is refused too: no slot it signs
/// would count.
pub fn write(issue: &str, seat: &str, model: &str, run: &str, run_key: &str) -> Result<String> {
    let launcher = ballot_sig::own_voter()
        .context("slot: no seat to sign as; set LJOS_SEAT to the launching seat")?;
    let run_key = run_key.trim().to_ascii_lowercase();
    let public = hex::decode(&run_key)
        .ok()
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .and_then(|b| ed25519_dalek::VerifyingKey::from_bytes(&b).ok());
    if public.is_none() {
        bail!("slot: --key {run_key} is not a public seat key; pass the hex `ljos cloud up` printed on the run");
    }
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
    if !stands(&launcher, Some(pinned), starts().as_deref()) {
        bail!(
            "slot: {launcher} is not a standing seat, so no slot it signs counts; a standing seat launches"
        );
    }
    let note = sign(
        &key,
        &tracker_id(),
        issue,
        &seat,
        model,
        run,
        &run_key,
        &launcher,
    );
    if ballot_sig::write_sig_note(issue, &note).is_err() {
        crate::run_captured("vissue", &["note", issue, &note])?;
    }
    Ok(format!(
        "slot: {seat} on {issue} for run {}, model {}, run key {}…, signed by {launcher} with key {}…\n",
        ballot_sig::field(run),
        ballot_sig::field(model),
        &run_key[..12],
        &mine[..12]
    ))
}

/// Why the run seats among `seats` do not count on `issue`. A seat that
/// `stands` needs no slot. Any other needs a slot in `notes` that names it
/// and its pinned key and verifies, signed under the pin of a seat that
/// stands.
/// `before_pin(note, voter)` is whether `note` was already in this
/// issue's own entry when `voter`'s pin was introduced. `false` refuses
/// the slot: the key it vouches for is not one the run minted after it.
/// A slot verifies only on `tracker` ([`tracker_id`]).
#[must_use]
pub fn problems(
    tracker: &str,
    issue: &str,
    seats: &[(&str, &str)],
    notes: &[String],
    pins: &BTreeMap<String, String>,
    stands: &dyn Fn(&str) -> bool,
    before_pin: &dyn Fn(&str, &str) -> bool,
) -> Vec<String> {
    let mut out = Vec::new();
    for (voter, _) in seats {
        if stands(voter) {
            continue;
        }
        let mut why = None;
        let mut counted = false;
        for (note, slot) in notes.iter().filter_map(|n| Some((n, parse(n)?))) {
            if slot.seat != *voter {
                continue;
            }
            let launcher = &slot.launcher;
            let refused = match pins.get(launcher) {
                _ if !verifies(tracker, issue, &slot) => {
                    Some(format!("{voter}'s slot does not verify"))
                }
                None => Some(format!(
                    "{voter}'s slot is from {launcher}, which has no pinned key on this tracker"
                )),
                Some(key) if *key != slot.key => Some(format!(
                    "{voter}'s slot is signed by a key other than {launcher}'s pinned one"
                )),
                Some(_) if !stands(launcher) => Some(format!(
                    "{voter}'s slot is from {launcher}, which is not a standing seat"
                )),
                Some(_) if pins.get(*voter) != Some(&slot.run_key) => Some(format!(
                    "{voter} is pinned to a key other than the one its slot names"
                )),
                Some(_) if !before_pin(note, voter) => {
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
                format!("{voter} is not a standing seat, and no slot on {issue} names it")
            }));
        }
    }
    out
}

/// Whether `note` was already in `issue`'s own entry when `seat`'s pin
/// `key` was introduced. `None` when the tracker is not a git repository.
/// `Some(false)` when the pin is not committed, or when any commit that
/// introduced it lacks `note` in this issue's entry. The same text on
/// another issue is not this entry. No author or committer date is read.
#[must_use]
pub fn recorded_before_pin(issue: &str, note: &str, seat: &str, key: &str) -> Option<bool> {
    if ballot_sig::tracker_git(&["rev-parse", "--is-inside-work-tree"])? != "true" {
        return None;
    }
    let note = note.trim();
    if note.is_empty() || seat.trim().is_empty() || key.trim().is_empty() {
        return Some(false);
    }
    let found = pin_introductions(seat, key);
    // No introduction means the pin is not in history. `all` of an empty
    // set is true, and an uncommitted pin would then count.
    if found.is_empty() {
        return Some(false);
    }
    Some(
        found
            .iter()
            .all(|commit| note_on_issue_at(commit, issue, note)),
    )
}

/// Commits that added the exact pin line `seat key`, whose parent trees
/// do not already hold it.
fn pin_introductions(seat: &str, key: &str) -> Vec<String> {
    let pick = format!("-S{seat} {key}");
    let log = ballot_sig::tracker_git(&[
        "--no-pager",
        "log",
        "--format=%H %P",
        pick.as_str(),
        "--",
        ballot_sig::PIN_FILE,
    ])
    .unwrap_or_default();
    let mut out = Vec::new();
    for line in log.lines() {
        let mut parts = line.split_whitespace();
        let Some(commit) = parts.next() else {
            continue;
        };
        let held_by_parent = parts.clone().any(|parent| pin_line_at(parent, seat, key));
        if pin_line_at(commit, seat, key) && !held_by_parent {
            out.push(commit.to_string());
        }
    }
    out
}

/// The pin lines some commit in this tracker's history held that `pins`
/// no longer does, as `seat key…`. A pin once committed stays: a line
/// changed in place re-pins a seat, and a line removed drops its ballots.
/// Empty on a tracker that is not a git repository.
#[must_use]
pub fn dropped_pins(pins: &BTreeMap<String, String>) -> Vec<String> {
    let log = ballot_sig::tracker_git(&[
        "--no-pager",
        "log",
        "--full-history",
        "--format=%H",
        "--",
        ballot_sig::PIN_FILE,
    ])
    .unwrap_or_default();
    let mut out = std::collections::BTreeSet::new();
    for commit in log.lines() {
        let Some(text) = file_at(commit.trim(), ballot_sig::PIN_FILE) else {
            continue;
        };
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some(seat), Some(key)) = (words.next(), words.next()) else {
                continue;
            };
            if pins.get(seat).map(String::as_str) != Some(key) {
                let short: String = key.chars().take(12).collect();
                out.insert(format!("{seat} {short}…"));
            }
        }
    }
    out.into_iter().collect()
}

fn pin_line_at(commit: &str, seat: &str, key: &str) -> bool {
    let Some(text) = file_at(commit, ballot_sig::PIN_FILE) else {
        return false;
    };
    text.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some(seat) && words.next() == Some(key) && words.next().is_none()
    })
}

fn file_at(commit: &str, path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    ballot_sig::tracker_git(&["--no-pager", "show", &format!("{commit}:{path}")])
}

fn note_on_issue_at(commit: &str, issue: &str, note: &str) -> bool {
    let pattern = format!(":ID:[[:space:]]+{}[[:space:]]*$", ere_escape(issue));
    let Some(hits) =
        ballot_sig::tracker_git(&["--no-pager", "grep", "-l", "-E", "-e", &pattern, commit])
    else {
        return false;
    };
    let prefix = format!("{commit}:");
    hits.lines().any(|hit| {
        let hit = hit.trim();
        let path = hit.strip_prefix(&prefix).unwrap_or(hit);
        file_at(commit, path)
            .is_some_and(|text| issue_entry(&text, issue).is_some_and(|block| block.contains(note)))
    })
}

/// The org entry whose `:ID:` is `issue`, from its heading through the
/// line before the next heading. A line of the same text in another entry
/// is not this one.
fn issue_entry(text: &str, issue: &str) -> Option<String> {
    let mut block = String::new();
    let mut matched = false;
    let mut in_entry = false;
    for line in text.lines() {
        if is_heading(line) {
            if matched {
                return Some(block);
            }
            block.clear();
            matched = false;
            in_entry = true;
        }
        if !in_entry {
            continue;
        }
        if !block.is_empty() {
            block.push('\n');
        }
        block.push_str(line);
        if id_line(line, issue) {
            matched = true;
        }
    }
    matched.then_some(block)
}

fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('*') else {
        return false;
    };
    rest.is_empty() || rest.starts_with(['*', ' ', '\t'])
}

fn id_line(line: &str, issue: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix(":ID:") else {
        return false;
    };
    rest.trim() == issue
}

/// `git grep -E` metacharacters, so an issue id is matched as itself.
fn ere_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if matches!(
            c,
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
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
            "t-1",
            "d-1",
            "review-cafebabe0001abcd",
            "grok 4",
            "bc-cafebabe0001abcd",
            &public(&key(4)),
            "ljosbot",
        );
        let slot = parse(&note).expect("a signed slot parses");
        assert_eq!(slot.seat, "review-cafebabe0001abcd");
        assert_eq!(slot.model, "grok_4");
        assert_eq!(slot.run, "bc-cafebabe0001abcd");
        assert_eq!(slot.run_key, public(&key(4)));
        assert_eq!(slot.launcher, "ljosbot");
        assert!(verifies("t-1", "d-1", &slot));
        assert!(
            !verifies("t-1", "d-2", &slot),
            "a slot lifted to another issue verified"
        );
        assert!(
            !verifies("t-2", "d-1", &slot),
            "a slot lifted to another tracker verified"
        );
        let mut moved = slot.clone();
        moved.model = "opus".into();
        assert!(
            !verifies("t-1", "d-1", &moved),
            "a rewritten model still verified"
        );
        let mut rekeyed = slot.clone();
        rekeyed.run_key = public(&key(3));
        assert!(
            !verifies("t-1", "d-1", &rekeyed),
            "a rewritten run key still verified"
        );
        let mut renamed = slot;
        renamed.seat = "review-other".into();
        assert!(
            !verifies("t-1", "d-1", &renamed),
            "a rewritten seat still verified"
        );
    }

    fn public(k: &SigningKey) -> String {
        hex::encode(k.verifying_key().to_bytes())
    }

    /// A slot for `seat` on `issue` of tracker `t-1` naming the run key
    /// [`check`] pins for `review-r1`, signed by `key(k)` as `launcher`.
    fn slotted(k: u8, issue: &str, seat: &str, launcher: &str) -> String {
        sign(
            &key(k),
            "t-1",
            issue,
            seat,
            "grok",
            "r1",
            &public(&key(4)),
            launcher,
        )
    }

    /// `ljosbot` and `inky` are standing seats. `mallory` and the run seat
    /// are not. `before_pin` is false only for the note `late` names: that
    /// line was not in this issue's entry when the run pin was introduced.
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
        let before_pin = |note: &str, _voter: &str| late != Some(note);
        problems(
            "t-1",
            "d-1",
            &[("inky", "ship"), ("review-r1", "ship")],
            notes,
            &pins,
            &standing,
            &before_pin,
        )
    }

    #[test]
    fn a_run_seat_counts_under_a_slot_from_a_standing_seat() {
        let slot = slotted(1, "d-1", "review-r1", "ljosbot");
        assert!(check(std::slice::from_ref(&slot), None).is_empty());
        let none = check(&[], None);
        assert_eq!(
            none,
            vec!["review-r1 is not a standing seat, and no slot on d-1 names it"]
        );
        let other = slotted(1, "d-1", "review-r2", "ljosbot");
        assert_eq!(check(&[other], None), none);
    }

    #[test]
    fn a_slot_that_does_not_stand_does_not_count() {
        let refused = |note: String, late: bool| {
            let p = check(std::slice::from_ref(&note), late.then_some(note.as_str()));
            assert_eq!(p.len(), 1, "{p:?}");
            p[0].clone()
        };
        let lifted = slotted(1, "d-2", "review-r1", "ljosbot");
        assert!(refused(lifted, false).contains("does not verify"));
        let forged = slotted(3, "d-1", "review-r1", "ljosbot");
        assert!(refused(forged, false).contains("other than ljosbot's pinned one"));
        let minted = slotted(3, "d-1", "review-r1", "mallory");
        assert!(refused(minted, false).contains("not a standing seat"));
        let own = slotted(4, "d-1", "review-r1", "review-r1");
        assert!(refused(own, false).contains("not a standing seat"));
        let stranger = slotted(5, "d-1", "review-r1", "nobody");
        assert!(refused(stranger, false).contains("no pinned key"));
        let rekeyed = sign(
            &key(1),
            "t-1",
            "d-1",
            "review-r1",
            "grok",
            "r1",
            &public(&key(3)),
            "ljosbot",
        );
        assert!(refused(rekeyed, false).contains("other than the one its slot names"));
        let late = slotted(1, "d-1", "review-r1", "ljosbot");
        assert!(refused(late, true).contains("pinned before its slot"));
    }

    #[test]
    fn one_good_slot_beside_a_forged_one_counts() {
        let forged = slotted(3, "d-1", "review-r1", "ljosbot");
        let good = slotted(2, "d-1", "review-r1", "inky");
        assert!(check(&[forged, good], None).is_empty());
    }

    #[test]
    fn a_seat_off_the_list_needs_a_slot_when_the_tracker_has_no_clock() {
        let pins: BTreeMap<String, String> = [("review-r1".to_string(), public(&key(4)))].into();
        let p = problems(
            "t-1",
            "d-1",
            &[("review-r1", "ship")],
            &[],
            &pins,
            &standing,
            &|_, _| true,
        );
        assert_eq!(
            p,
            vec!["review-r1 is not a standing seat, and no slot on d-1 names it"]
        );
    }

    #[test]
    fn the_standing_list_names_those_seats_and_skips_comments() {
        assert!(standing("ljosbot"));
        assert!(standing("inky"));
        assert!(standing("disk-saver"));
        assert!(!standing("mallory"));
        assert!(!standing("# Seats that stood"));
        assert!(!standing(""));
    }

    #[test]
    fn a_slot_line_in_another_issues_block_is_not_on_this_one() {
        let note = slotted(1, "d-1", "review-r1", "ljosbot");
        let other =
            format!("* TODO other\n:PROPERTIES:\n:ID: d-2\n:END:\n- Note: \"{note}\" [2020]\n");
        let this = "* TODO this\n:PROPERTIES:\n:ID: d-1\n:END:\n";
        let text = format!("{other}{this}");
        assert!(
            issue_entry(&text, "d-1").is_none_or(|block| !block.contains(&note)),
            "a line on d-2 counted as d-1"
        );
        assert!(
            issue_entry(&text, "d-2").is_some_and(|block| block.contains(&note)),
            "the line was not in d-2's own entry"
        );
        let with = format!("{text}- Note: \"{note}\" [2021]\n");
        assert!(issue_entry(&with, "d-1").is_some_and(|block| block.contains(&note)));
    }

    #[test]
    fn an_issue_id_is_a_literal_in_the_grep() {
        assert_eq!(ere_escape("a.b*"), r"a\.b\*");
        assert_eq!(ere_escape("probe-slot"), "probe-slot");
    }

    #[test]
    fn a_note_that_is_not_a_slot_does_not_parse() {
        assert_eq!(parse("slot: review model=m run=r ljosbot ab"), None);
        assert_eq!(
            parse("slot: review model=m run=r ljosbot ab cd"),
            None,
            "a slot that names no run key parsed"
        );
        assert_eq!(parse("slot: review m r ljosbot ab cd"), None);
        assert_eq!(parse("ballot-sig: a ship ab cd"), None);
    }
}
