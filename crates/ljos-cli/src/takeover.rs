//! `ljos claim ID --takeover`: take a ticket from a holder that looks gone.
//!
//! A seat in another home leaves no hold record on this host, so the
//! tracker is the only record of who has the ticket and when it last moved.
//! The holder is judged stale when the claim has been quiet for at least the
//! lease (`LJOS_LEASE_SECS`, 900 seconds when unset) and no running
//! conversation on this host is named for the ticket. Anything else is a
//! live holder: the takeover refuses before it writes a byte.
//!
//! A takeover that goes ahead frees the old holder's claim-graph node, moves
//! the tracker claim to this name, claims the node here, and leaves a note on
//! the ticket naming the old holder. The old holder's generation is fenced
//! by the release, so a session that comes back cannot finish the work.

use anyhow::Result;
use serde_json::Value;
use vissue_core::model::CLAIM_RELEASED_NOTE;

use crate::policy_label;

/// The holder as the tracker names it, and the seat that name is for.
fn seat_of_holder(held: &str, node: &str) -> (String, String) {
    let old = crate::unscoped_holder(held, node).to_string();
    let (seat, _) = crate::holder_parts(&old);
    let seat = if seat.is_empty() { old.clone() } else { seat };
    (old, seat)
}

/// Seconds the claim on `card` has been quiet at `now`.
///
/// The newest of the claim stamp and every logbook line that is not the
/// release bookkeeping note. `None` when the card carries no readable time,
/// which a caller takes as not stale.
#[must_use]
pub(crate) fn quiet_secs(card: &Value, now: i64) -> Option<u64> {
    let mut newest = card["claimed_at"].as_str().and_then(crate::reap::org_ts);
    for entry in card["logbook"].as_array().into_iter().flatten() {
        let bookkeeping = entry["note"]
            .as_str()
            .is_some_and(|note| note.trim_start().starts_with(CLAIM_RELEASED_NOTE));
        if bookkeeping {
            continue;
        }
        if let Some(at) = entry["timestamp"].as_str().and_then(crate::reap::org_ts) {
            newest = Some(newest.map_or(at, |have| have.max(at)));
        }
    }
    newest.map(|at| u64::try_from(now.saturating_sub(at)).unwrap_or(0))
}

/// A quiet span as a few words: `20 minutes`, `5 hours`, `3 days`.
#[must_use]
pub(crate) fn span_words(secs: u64) -> String {
    let (n, unit) = match secs {
        0..=59 => (secs, "second"),
        60..=3599 => (secs / 60, "minute"),
        3600..=86_399 => (secs / 3600, "hour"),
        _ => (secs / 86_400, "day"),
    };
    if n == 1 {
        format!("1 {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// How long the open claim on `node` has been quiet, when that is at least
/// the lease. `None` when the lease is off, the claim moved inside the
/// lease, or the ticket carries no time to judge by. The caller has already
/// ruled out a running conversation on this host.
pub(crate) fn stale_for(node: &str) -> Option<u64> {
    let lease = crate::lease_secs();
    if lease == 0 {
        return None;
    }
    let card = crate::tracker_show_json(node).ok()?;
    quiet_secs(&card, now_secs()).filter(|quiet| *quiet >= lease)
}

/// The refusal a plain `ljos claim` prints for a holder judged stale. One
/// line: it says the holder looks stale and names the command that takes
/// the claim over.
#[must_use]
pub(crate) fn stale_refusal(node: &str, assignee: &str, held: &str, quiet: u64) -> String {
    let (old, seat) = seat_of_holder(held, node);
    format!(
        "claim: {node} is held by another ({old}, seat {seat}), not by {assignee}. The holder looks stale: the claim has been quiet {}, past the {}s lease, and no running conversation on this host is named for it. `ljos claim {node} --takeover` takes it over and notes {old} on the ticket.",
        span_words(quiet),
        crate::lease_secs(),
    )
}

fn refused(detail: String) -> anyhow::Error {
    policy_label::labelled("claim-conflict", detail)
}

/// Take `node` over from a holder that looks gone, as `assignee`.
///
/// A ticket nobody else has, or one this seat already has, is claimed the
/// ordinary way. Another seat's claim is taken only when the holder is
/// stale (see the module text). The checks all come before the first
/// write, so a refusal changes nothing.
///
/// # Errors
///
/// The holder is running, or the claim moved inside the lease, or the lease
/// is off, in which case nothing is written. Or the claim graph or the
/// tracker refuses a step of the takeover.
pub fn claim_takeover(node: &str, assignee: &str) -> Result<String> {
    if node.trim().is_empty() {
        anyhow::bail!(crate::EMPTY_CLAIM_ID);
    }
    let assignee = crate::claim_holder(assignee)?;
    let assignee = assignee.as_str();
    let Some(held) = crate::tracker_open_holder(node) else {
        return plain_claim(node, assignee);
    };
    if crate::same_claimed_seat(&held, assignee, node) {
        return plain_claim(node, assignee);
    }
    let (old, seat) = seat_of_holder(&held, node);
    let actor = crate::work_id(&crate::occupancy_scope(assignee, node));
    if let Some((_, live)) = crate::live_other_hold_on_ticket(node, &actor, assignee) {
        return Err(refused(format!(
            "claim: {node} is held by another ({}, seat {}, still running, since {}), so `--takeover` refuses it and changes nothing. `ljos send {} TEXT` reaches the holder.",
            live.assignee, live.seat, live.since_when(), live.seat
        )));
    }
    let lease = crate::lease_secs();
    if lease == 0 {
        return Err(refused(format!(
            "claim: {node} is held by another ({old}, seat {seat}), and `--takeover` is off while LJOS_LEASE_SECS is 0, so it changes nothing. `ljos send {seat} TEXT` reaches the holder."
        )));
    }
    let quiet = crate::tracker_show_json(node)
        .ok()
        .and_then(|card| quiet_secs(&card, now_secs()));
    let Some(quiet) = quiet.filter(|q| *q >= lease) else {
        let said = match quiet {
            Some(q) => format!(
                "the claim moved {} ago, inside the {lease}s lease",
                span_words(q)
            ),
            None => "the ticket carries no time to judge the claim by".to_string(),
        };
        return Err(refused(format!(
            "claim: {node} is held by another ({old}, seat {seat}), and {said}, so `--takeover` refuses it and changes nothing. `ljos send {seat} TEXT` reaches the holder."
        )));
    };

    let id = crate::node_for(node)?;
    free_old_graph_node(&id, &actor);
    crate::stamp_tracker_claim(node, assignee, true).map_err(|e| {
        e.context(format!(
            "claim: the tracker would not move {node} from {old} to {assignee}, so nothing was taken"
        ))
    })?;
    let mut out = crate::claim(node, assignee).map_err(|e| {
        e.context(format!(
            "claim: the tracker now names {assignee} on {node} but the claim graph refused; `ljos release {node}` hands it back"
        ))
    })?;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "took over {node} from {old} (seat {seat}), quiet {}\n",
        span_words(quiet)
    ));
    let note = format!(
        "took over from {old} (seat {seat}), claim quiet {}, past the {lease}s lease",
        span_words(quiet)
    );
    match crate::run_captured_as("vissue", &["note", node, &note], Some(assignee)) {
        Ok(_) => out.push_str(&format!("noted on {node}: {note}\n")),
        Err(e) => out.push_str(&format!(
            "note: {node} took over but the note failed ({e:#})\n"
        )),
    }
    out.push_str(&crate::persist_tracker(node, "took over"));
    Ok(out)
}

/// The ordinary claim, with the tracker commit `ljos claim ID` adds, for a
/// ticket that has no other holder to take it from.
fn plain_claim(node: &str, assignee: &str) -> Result<String> {
    let mut out = crate::claim(node, assignee)?;
    out.push_str(&crate::persist_tracker(node, "claimed"));
    Ok(out)
}

/// Hand back the claim-graph node when another actor still has it, so the
/// takeover claims it fresh. A graph that does not know the node, or a node
/// nobody has, is left as it is: the claim that follows says what is wrong.
fn free_old_graph_node(id: &str, actor: &str) {
    let Ok(got) = crate::run_captured("claimdag", &["get", id]) else {
        return;
    };
    let line = got.stdout.lines().next().unwrap_or_default();
    if !matches!(line.split_whitespace().nth(1), Some("claimed" | "running")) {
        return;
    }
    let Some(holder) = crate::holder_of(line) else {
        return;
    };
    if holder == actor {
        return;
    }
    if crate::run_captured("claimdag", &["release", id, "--actor", &holder]).is_ok() {
        crate::drop_hold(&holder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn card(claimed_at: &str, entries: &[(&str, &str)]) -> Value {
        let logbook: Vec<Value> = entries
            .iter()
            .map(|(note, at)| json!({"note": note, "timestamp": at}))
            .collect();
        json!({"claimed_at": claimed_at, "logbook": logbook})
    }

    fn at(stamp: &str) -> i64 {
        crate::reap::org_ts(stamp).unwrap()
    }

    #[test]
    fn quiet_runs_from_the_claim_stamp() {
        let _env_read = crate::test_env_read_lock();
        let c = card("[2026-11-18 Wed 05:00]", &[]);
        let now = at("[2026-11-18 Wed 06:00]");
        assert_eq!(quiet_secs(&c, now), Some(3600));
    }

    #[test]
    fn a_later_note_restarts_the_quiet_clock() {
        let _env_read = crate::test_env_read_lock();
        let c = card(
            "[2026-11-18 Wed 05:00]",
            &[("checked the codec", "[2026-11-18 Wed 05:50]")],
        );
        let now = at("[2026-11-18 Wed 06:00]");
        assert_eq!(quiet_secs(&c, now), Some(600));
    }

    #[test]
    fn the_release_bookkeeping_note_is_not_activity() {
        let _env_read = crate::test_env_read_lock();
        let c = card(
            "[2026-11-18 Wed 05:00]",
            &[("claim released: alice", "[2026-11-18 Wed 05:55]")],
        );
        let now = at("[2026-11-18 Wed 06:00]");
        assert_eq!(quiet_secs(&c, now), Some(3600));
    }

    #[test]
    fn a_card_with_no_time_has_no_quiet() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(quiet_secs(&json!({"logbook": []}), 0), None);
    }

    #[test]
    fn a_stamp_in_the_future_is_not_quiet() {
        let _env_read = crate::test_env_read_lock();
        let c = card("[2026-11-18 Wed 07:00]", &[]);
        let now = at("[2026-11-18 Wed 06:00]");
        assert_eq!(quiet_secs(&c, now), Some(0));
    }

    #[test]
    fn spans_read_in_one_unit() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(span_words(1), "1 second");
        assert_eq!(span_words(59), "59 seconds");
        assert_eq!(span_words(60), "1 minute");
        assert_eq!(span_words(1200), "20 minutes");
        assert_eq!(span_words(7200), "2 hours");
        assert_eq!(span_words(86_400), "1 day");
        assert_eq!(span_words(3 * 86_400 + 5), "3 days");
    }

    #[test]
    fn the_stale_refusal_names_one_takeover_command_on_one_line() {
        let _env_read = crate::test_env_read_lock();
        let said = stale_refusal("demo-aaaa", "brio", "acme", 7200);
        assert!(said.contains("The holder looks stale"), "{said}");
        assert!(said.contains("`ljos claim demo-aaaa --takeover`"), "{said}");
        assert!(said.contains("notes acme on the ticket"), "{said}");
        assert!(!said.contains('\n'), "{said}");
    }
}
