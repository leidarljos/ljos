//! Claims this seat still holds and has not noted, cited or finished for a day.
//!
//! `ljos sitting` and `ljos inbox` name a few. `ljos claims --mine` lists
//! every one. Claims untouched for longer than [`stale_days_from_env`] are
//! one line on that banner. `ljos claims --stale` lists them. `ljos release
//! --stale --yes` hands that older pile back. `ljos release --stale` releases
//! the claims a sitting would hand back: quiet past the lease, held by this
//! seat, and not kept by a conversation that is still running. A release
//! drops the claim. It does not close the ticket.

use chrono::{DateTime, Local, NaiveDate};
use vissue_core::views::IssueRec;
use vissue_core::LogEntry;

use anyhow::{bail, Context, Result};

/// Quiet claims a sitting or an inbox prints before the `+N more` line.
///
/// A sitting already ends on the claim it just took. Three forgotten
/// claims are enough to see that the seat is holding a pile; the rest of
/// that pile is one line pointing at `ljos claims --mine`, so the sitting
/// does not grow with the tracker.
const QUIET_SHOWN: usize = 3;

/// Whole local days since the last note, deed or finish before a claim is named.
///
/// The tracker's hygiene census uses `stale_claim_days`, seven by default,
/// and a sitting does not read that report. A sitting is a day's work: a
/// claim touched today is still in hand, and one with nothing since
/// yesterday is what the next sitting or inbox should put back in front of
/// the seat. The count is whole days because the tracker stamps claims as
/// org dates, the same calendar `claim_age_days` uses.
const QUIET_DAYS: i64 = 1;

/// Whole local days untouched before a claim leaves the banner rows.
///
/// `LJOS_STALE_DAYS` replaces it. A value that is not a number of days,
/// or is negative, leaves the default, so a sitting still prints.
const DEFAULT_STALE_DAYS: i64 = 7;

/// Characters of the title kept on one quiet-claim line.
///
/// The line also carries the id, the age and both commands. 48 characters
/// is a heading the seat can still tell apart from the next ticket, and
/// the last three of those characters are `...` when the title is longer.
const TITLE_SHOWN: usize = 48;

/// The lines `ljos sitting` prints, from the project parse that opened `issue`.
///
/// [`crate::issue_spot::find`] already parsed that project to name the
/// ticket. The scan walks those headings and does not open another project
/// or start `vissue`. An unoptimized walk of 500 headings took 5.8–6.2 ms
/// over five runs. Empty when the seat holds nothing quiet, or the ticket
/// is not in the tracker.
#[must_use]
pub fn beside_sitting(issue: &str, seat: &str) -> String {
    let Some(spot) = crate::issue_spot::find(issue) else {
        return String::new();
    };
    render(spot.recs.as_slice(), seat, Some(QUIET_SHOWN))
}

/// The lines `ljos inbox` prints: every quiet claim on this tracker, capped.
///
/// An inbox has no ticket, so it cannot reuse one project's parse. A
/// tracker that cannot be read leaves the mail listing as it was.
#[must_use]
pub fn beside_inbox(seat: &str) -> String {
    let Ok(recs) = load_tracker() else {
        return String::new();
    };
    render(&recs, seat, Some(QUIET_SHOWN))
}

/// Every quiet claim this seat holds, one line each.
///
/// # Errors
///
/// The tracker cannot be read.
pub fn list_mine() -> Result<String> {
    let recs = load_tracker().context("claims --mine: the tracker could not be read")?;
    Ok(render(&recs, &crate::seat_name(), None))
}

/// Claims this seat holds that have been untouched for `days` or longer.
///
/// `days` of `None` reads [`stale_days_from_env`]. Oldest first. Each line
/// names the age and the command that finishes, releases or notes it.
/// `json` prints one array.
///
/// # Errors
///
/// `--days` is negative, or the tracker cannot be read.
pub fn list_stale(days: Option<i64>, json: bool) -> Result<String> {
    let threshold = threshold_of("claims", days)?;
    let recs = load_tracker().context("claims --stale: the tracker could not be read")?;
    let today = Local::now().date_naive();
    let quiet = quiet_rows(&recs, &crate::seat_name(), today);
    let rows = stale_of(&quiet, today, threshold);
    if json {
        let body: Vec<serde_json::Value> = rows.iter().map(|row| stale_json(row, today)).collect();
        return Ok(format!("{}\n", serde_json::to_string(&body)?));
    }
    Ok(render_stale_lines(&rows, threshold))
}

/// Hand back every claim [`list_stale`] would name.
///
/// Without `yes`, the text names each claim and the command that releases
/// them, and nothing is written. With `yes`, each claim is released the
/// way `ljos release ID` releases one: the claim drops and a STARTED
/// ticket goes back to TODO. The ticket is not closed. An empty list
/// returns before any release, lock or pack call.
///
/// # Errors
///
/// `--days` is negative, or the tracker cannot be read. A release that
/// the claim graph or the tracker refuses is a line in the text.
pub fn release_stale(assignee: &str, days: Option<i64>, yes: bool) -> Result<String> {
    let threshold = threshold_of("release", days)?;
    let recs = load_tracker().context("release --stale: the tracker could not be read")?;
    let today = Local::now().date_naive();
    let quiet = quiet_rows(&recs, assignee, today);
    let rows = stale_of(&quiet, today, threshold);
    if rows.is_empty() {
        return Ok(format!("no claims untouched for {threshold}+ days\n"));
    }
    if !yes {
        let mut out = render_release_preview(&rows);
        let n = rows.len();
        let noun = if n == 1 { "claim" } else { "claims" };
        out.push_str(&format!(
            "{n} {noun} would go back to TODO. {} releases them.\n",
            yes_command(days)
        ));
        return Ok(out);
    }
    // The queue is empty on a seat that has nothing waiting, and that
    // path does not open the pack. A release with rows to hand back is
    // the one that may send a lesson already queued.
    crate::flush_queued_lessons();
    let mut out = String::new();
    let mut released = 0usize;
    for row in &rows {
        match crate::release(&row.id, assignee) {
            Ok(said) => {
                out.push_str(&said);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&crate::persist_tracker(&row.id, "released"));
                released += 1;
            }
            Err(err) => {
                out.push_str(&format!("release: {} stayed claimed ({err:#})\n", row.id));
            }
        }
    }
    let n = rows.len();
    let noun = if n == 1 { "claim" } else { "claims" };
    if released == n {
        out.push_str(&format!(
            "released {n} {noun}. Each ticket is TODO again.\n"
        ));
    } else {
        out.push_str(&format!(
            "released {released} of {n} {noun}. Each handed-back ticket is TODO again.\n"
        ));
    }
    Ok(out)
}

/// `LJOS_STALE_DAYS`, or [`DEFAULT_STALE_DAYS`] when that value is unset
/// or not a count of days.
#[must_use]
pub fn stale_days_from_env() -> i64 {
    std::env::var("LJOS_STALE_DAYS")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| raw.parse().ok())
        .filter(|days: &i64| *days >= 0)
        .unwrap_or(DEFAULT_STALE_DAYS)
}

fn load_tracker() -> Result<Vec<IssueRec>> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    layout.require_tracker()?;
    Ok(vissue_core::catalog::load_recs(&layout)?)
}

fn render(recs: &[IssueRec], seat: &str, limit: Option<usize>) -> String {
    let today = Local::now().date_naive();
    let rows = quiet_rows(recs, seat, today);
    // A banner (a cap) folds the untouched pile into one line. `claims
    // --mine` still lists every quiet claim, so a seat that asks for the
    // whole list still sees it.
    let banner = limit.is_some();
    let threshold = stale_days_from_env();
    let (stale, recent): (Vec<&Quiet>, Vec<&Quiet>) = if banner {
        rows.iter()
            .partition(|row| is_stale(row.day, today, threshold))
    } else {
        (Vec::new(), rows.iter().collect())
    };
    if recent.is_empty() && stale.is_empty() {
        return if banner {
            String::new()
        } else {
            "no quiet claims\n".to_string()
        };
    }
    let show = limit.unwrap_or(recent.len()).min(recent.len());
    let mut out = String::new();
    for row in recent.iter().take(show) {
        out.push_str(&quiet_line(row));
    }
    if let Some(limit) = limit {
        let rest = recent.len().saturating_sub(limit);
        if rest > 0 {
            out.push_str(&format!("+{rest} more (ljos claims --mine)\n"));
        }
    }
    if !stale.is_empty() {
        let n = stale.len();
        let noun = if n == 1 { "claim" } else { "claims" };
        out.push_str(&format!(
            "{n} {noun} untouched for {threshold}+ days: ljos claims --stale\n"
        ));
    }
    out
}

fn quiet_line(row: &Quiet) -> String {
    format!(
        "{}  {}  {}  ljos finish {} --lesson \"...\" / ljos note {} \"...\"\n",
        row.id,
        short_title(&row.title),
        row.age,
        row.id,
        row.id,
    )
}

fn stale_of(rows: &[Quiet], today: NaiveDate, threshold: i64) -> Vec<&Quiet> {
    rows.iter()
        .filter(|row| is_stale(row.day, today, threshold))
        .collect()
}

fn render_stale_lines(rows: &[&Quiet], threshold: i64) -> String {
    if rows.is_empty() {
        return format!("no claims untouched for {threshold}+ days\n");
    }
    let mut out = String::new();
    for row in rows {
        let (finish, release, note) = action_commands(&row.id);
        out.push_str(&format!(
            "{}  {}  {}  {finish} / {release} / {note}\n",
            row.id,
            short_title(&row.title),
            row.age,
        ));
    }
    out
}

fn render_release_preview(rows: &[&Quiet]) -> String {
    let mut out = String::new();
    for row in rows {
        out.push_str(&format!(
            "{}  {}  {}  ljos release {}\n",
            row.id,
            short_title(&row.title),
            row.age,
            row.id,
        ));
    }
    out
}

fn stale_json(row: &Quiet, today: NaiveDate) -> serde_json::Value {
    let (finish, release, note) = action_commands(&row.id);
    serde_json::json!({
        "id": row.id,
        "title": row.title,
        "age": row.age,
        "age_days": age_days(row.day, today),
        "finish": finish,
        "release": release,
        "note": note,
    })
}

fn action_commands(id: &str) -> (String, String, String) {
    (
        format!("ljos finish {id} --lesson \"...\""),
        format!("ljos release {id}"),
        format!("ljos note {id} \"...\""),
    )
}

fn yes_command(days: Option<i64>) -> String {
    match days {
        Some(days) => format!("ljos release --stale --days {days} --yes"),
        None => "ljos release --stale --yes".to_string(),
    }
}

fn threshold_of(verb: &str, days: Option<i64>) -> Result<i64> {
    match days {
        Some(days) if days >= 0 => Ok(days),
        Some(_) => bail!("{verb}: --days wants a number of days, 0 or more"),
        None => Ok(stale_days_from_env()),
    }
}

fn is_stale(day: Option<NaiveDate>, today: NaiveDate, threshold: i64) -> bool {
    match day {
        Some(day) => (today - day).num_days() >= threshold,
        None => true,
    }
}

fn age_days(day: Option<NaiveDate>, today: NaiveDate) -> Option<i64> {
    day.map(|day| (today - day).num_days())
}

struct Quiet {
    id: String,
    title: String,
    age: String,
    day: Option<NaiveDate>,
}

fn quiet_rows(recs: &[IssueRec], seat: &str, today: NaiveDate) -> Vec<Quiet> {
    let want = crate::canonical_seat_name(seat);
    if want.is_empty() {
        return Vec::new();
    }
    let mut rows: Vec<Quiet> = recs
        .iter()
        .filter(|rec| held_live(&rec.heading, &want))
        .filter_map(|rec| {
            let day = last_touch(rec);
            quiet_for(day, today).then(|| Quiet {
                id: rec.heading.id.clone(),
                title: rec.heading.title.clone(),
                age: age_words(day, today),
                day,
            })
        })
        .collect();
    rows.sort_by(|a, b| a.day.cmp(&b.day).then(a.id.cmp(&b.id)));
    rows
}

/// A claim with no stamp is quiet: there is no day on which to call it fresh.
fn quiet_for(day: Option<NaiveDate>, today: NaiveDate) -> bool {
    match day {
        Some(day) => (today - day).num_days() >= QUIET_DAYS,
        None => true,
    }
}

fn held_live(heading: &vissue_core::IssueHeading, seat: &str) -> bool {
    matches!(heading.state.as_str(), "STARTED" | "BLOCKED")
        && heading
            .claimed_by()
            .is_some_and(|who| crate::canonical_seat_name(who) == seat)
}

/// Newest day the seat noted, cited a deed, or finished, else the claim day.
fn last_touch(rec: &IssueRec) -> Option<NaiveDate> {
    let mut best = rec
        .heading
        .claimed_at()
        .and_then(vissue_core::model::parse_stamp_date);
    for entry in &rec.heading.logbook {
        if !counts_as_touch(entry) {
            continue;
        }
        let Some(day) = vissue_core::model::parse_stamp_date(&entry.timestamp) else {
            continue;
        };
        best = Some(best.map_or(day, |have| have.max(day)));
    }
    if let Some(day) = deed_file_day(rec) {
        best = Some(best.map_or(day, |have| have.max(day)));
    }
    best
}

fn counts_as_touch(entry: &LogEntry) -> bool {
    if entry.is_bookkeeping() {
        return false;
    }
    if matches!(entry.to_state.as_deref(), Some("DONE" | "CANCELLED")) {
        return true;
    }
    if entry
        .note
        .as_deref()
        .is_some_and(|note| !note.trim().is_empty())
    {
        return true;
    }
    let Some(text) = crate::logbook_text(entry) else {
        return false;
    };
    let lower = text.to_ascii_lowercase();
    lower.starts_with("finish") || lower.contains("deed")
}

/// The day a per-issue ledger file was last written, when it cites a deed.
///
/// `:DEEDS:` stores accessions and no time. A ledger file belongs to one
/// issue, so the day it was written is the day the citation landed. A
/// shared `issues.org` is rewritten for every ticket in the project, and
/// its modification day is not this deed's.
fn deed_file_day(rec: &IssueRec) -> Option<NaiveDate> {
    if rec.heading.deeds().is_empty() {
        return None;
    }
    let name = rec.path.file_name()?.to_str()?;
    if !name.contains(rec.heading.id.as_str()) {
        return None;
    }
    let modified = rec.path.metadata().ok()?.modified().ok()?;
    let stamped: DateTime<Local> = DateTime::from(modified);
    Some(stamped.date_naive())
}

fn age_words(day: Option<NaiveDate>, today: NaiveDate) -> String {
    let Some(day) = day else {
        return "undated".to_string();
    };
    let then = day.format("%Y-%m-%d").to_string();
    let now = today.format("%Y-%m-%d").to_string();
    let age = crate::age_of(Some(&then), &now);
    if age.is_empty() {
        "undated".to_string()
    } else {
        age
    }
}

fn short_title(title: &str) -> String {
    crate::scan::fit_title(title, TITLE_SHOWN)
}

/// `900`, `15m`, `6h`, `2d`. A bare number is seconds, the unit `LJOS_LEASE_SECS` uses.
///
/// # Errors
///
/// The text is not a duration, or it is zero.
pub fn parse_older_than(raw: &str) -> Result<u64> {
    let raw = raw.trim();
    let split = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (n, unit) = raw.split_at(split);
    if n.is_empty() {
        bail!("release: --older-than wants a duration such as 900, 15m, 6h or 2d");
    }
    let n: u64 = n.parse().map_err(|_| {
        anyhow::anyhow!("release: --older-than wants a duration such as 900, 15m, 6h or 2d")
    })?;
    let secs = match unit {
        "" | "s" => n,
        "m" => n.saturating_mul(60),
        "h" => n.saturating_mul(3600),
        "d" => n.saturating_mul(86_400),
        _ => bail!("release: --older-than wants a duration such as 900, 15m, 6h or 2d"),
    };
    if secs == 0 {
        bail!("release: --older-than wants a duration longer than 0");
    }
    Ok(secs)
}

/// One claim [`release_quiet`] may hand back.
struct QuietHeld {
    /// Tracker id when the claim names one, else the claim-graph id.
    label: String,
    ticket: String,
    /// Claim-graph node. Empty when the tracker holds the claim and the graph does not.
    graph_id: String,
    /// Actor the graph recorded. Empty when there is no graph node.
    actor: String,
    /// Name [`crate::unstamp_tracker`] matches against `CLAIMED_BY`.
    holder: String,
    /// Older holder of this seat that the current session took over from, if different.
    old_holder: Option<String>,
    quiet: u64,
}

/// Release every claim this seat holds that has been quiet for at least
/// `older_than` seconds and whose conversation is not running on this host.
///
/// `older_than` of `None` is [`crate::lease_secs`], the window a sitting
/// uses before it hands a claim back. Zero from that variable releases
/// nothing: a sitting with `LJOS_LEASE_SECS=0` hands nothing back either.
/// A hold whose process is still the program that wrote it stays, the
/// same check as `kept <id>: ... is still running`. Another seat's claim
/// stays. The ticket goes back to the state it had before the claim, or
/// stays as it is when that state was not STARTED. It is not closed.
///
/// `--dry-run` prints the same count and writes nothing. `--verbose`
/// prints one line per released ticket, and one line for each claim a
/// running conversation still holds.
///
/// # Errors
///
/// The tracker cannot be read, or the claim graph does not answer. Nothing
/// is released in either case.
pub fn release_quiet(
    seat: &str,
    older_than: Option<u64>,
    dry_run: bool,
    verbose: bool,
) -> Result<String> {
    let threshold = older_than.unwrap_or_else(crate::lease_secs);
    if older_than.is_none() && threshold == 0 {
        return Ok(summary_line(0, 0, dry_run));
    }
    let recs = load_tracker().context("release --stale: the tracker could not be read")?;
    let listed = match crate::run_captured("claimdag", &["list", "--json"]) {
        Ok(said) => said.stdout,
        Err(err) => {
            let text = format!("{err:#}");
            if crate::call_deadline::stalled(&text) {
                bail!(
                    "{}",
                    crate::call_deadline::stall_line(
                        "claimdag",
                        &["list", "--json"],
                        crate::call_deadline::timeout(),
                    )
                );
            }
            bail!("release: the claim graph did not answer");
        }
    };
    let rows: Vec<serde_json::Value> = serde_json::from_str(listed.trim())
        .map_err(|_| anyhow::anyhow!("release: the claim graph did not answer"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (mut release, mut kept) = classify(&recs, &rows, seat, now, threshold);
    release.sort_by(|a, b| b.quiet.cmp(&a.quiet).then(a.label.cmp(&b.label)));
    kept.sort_by(|a, b| b.quiet.cmp(&a.quiet).then(a.label.cmp(&b.label)));
    if dry_run {
        return Ok(render_quiet(&release, &kept, verbose));
    }
    let mut failed = String::new();
    let mut done = Vec::new();
    for item in &release {
        match hand_back(item) {
            Ok(()) => done.push(item),
            Err(err) => {
                failed.push_str(&format!(
                    "release: {} stayed claimed ({err:#})\n",
                    item.label
                ));
            }
        }
    }
    let mut out = failed;
    out.push_str(&render_quiet_done(&done, &kept, verbose));
    Ok(out)
}

fn render_quiet(release: &[QuietHeld], kept: &[QuietHeld], verbose: bool) -> String {
    if verbose {
        let mut out = String::new();
        for item in release {
            out.push_str(&format!("would release {}\n", item.label));
        }
        for item in kept {
            out.push_str(&format!(
                "kept {}: held by a running conversation\n",
                item.label
            ));
        }
        if out.is_empty() {
            out.push_str("would release 0 quiet claims\n");
        }
        return out;
    }
    summary_line(release.len(), kept.len(), true)
}

fn render_quiet_done(released: &[&QuietHeld], kept: &[QuietHeld], verbose: bool) -> String {
    if verbose {
        let mut out = String::new();
        for item in released {
            if let Some(old) = &item.old_holder {
                out.push_str(&format!("released {} (took over from {old})\n", item.label));
            } else {
                out.push_str(&format!("released {}\n", item.label));
            }
        }
        for item in kept {
            out.push_str(&format!(
                "kept {}: held by a running conversation\n",
                item.label
            ));
        }
        if out.is_empty() {
            out.push_str("released 0 quiet claims\n");
        }
        return out;
    }
    let mut out = String::new();
    for item in released {
        if let Some(old) = &item.old_holder {
            out.push_str(&format!("took over {} from {old}\n", item.label));
        }
    }
    out.push_str(&summary_line(released.len(), kept.len(), false));
    out
}

fn summary_line(released: usize, kept: usize, dry_run: bool) -> String {
    let verb = if dry_run { "would release" } else { "released" };
    let rel = format!(
        "{verb} {}",
        count_claims(released, "quiet claim", "quiet claims")
    );
    if kept == 0 {
        return format!("{rel}\n");
    }
    let held = if kept == 1 {
        "kept 1 held by a running conversation".to_string()
    } else {
        format!("kept {kept} held by running conversations")
    };
    format!("{rel}; {held}\n")
}

fn count_claims(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn classify(
    recs: &[IssueRec],
    rows: &[serde_json::Value],
    seat: &str,
    now: u64,
    threshold: u64,
) -> (Vec<QuietHeld>, Vec<QuietHeld>) {
    let mut release = Vec::new();
    let mut kept = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let current = crate::holder_name();
    for row in rows {
        let Some(node) = graph_node(row) else {
            continue;
        };
        if !node.ticket.is_empty() {
            seen.insert(node.ticket.clone());
        }
        let claimed = claimed_on(recs, &node.ticket);
        let hold = crate::read_hold(&node.actor);
        if !ours(seat, claimed, hold.as_ref()) {
            continue;
        }
        let Some(quiet) = node.updated.map(|t| now.saturating_sub(t)) else {
            continue;
        };
        if quiet < threshold {
            continue;
        }
        let holder = claimed
            .map(str::to_string)
            .or_else(|| {
                hold.as_ref()
                    .map(|h| crate::unscoped_holder(&h.assignee, &node.ticket).to_string())
            })
            .unwrap_or_else(|| seat.to_string());
        let old_holder = crate::took_over_from(&holder, &current, &node.ticket);
        let label = if node.ticket.is_empty() {
            node.id.clone()
        } else {
            node.ticket.clone()
        };
        let ticket_live = !node.ticket.is_empty()
            && hold_on_ticket(&node.ticket).is_some_and(|(_, h)| crate::hold_alive(&h));
        let alive = hold.as_ref().is_some_and(crate::hold_alive) || ticket_live;
        let item = QuietHeld {
            label,
            ticket: node.ticket,
            graph_id: node.id,
            actor: node.actor,
            holder,
            old_holder,
            quiet,
        };
        if alive {
            kept.push(item);
        } else {
            release.push(item);
        }
    }
    let want = crate::canonical_seat_name(seat);
    for rec in recs {
        if !held_live(&rec.heading, &want) || seen.contains(&rec.heading.id) {
            continue;
        }
        let quiet = claimed_quiet(rec, now);
        if quiet < threshold {
            continue;
        }
        let named = hold_on_ticket(&rec.heading.id);
        if let Some((_, hold)) = &named {
            if !ours(seat, rec.heading.claimed_by(), Some(hold)) {
                continue;
            }
        }
        let holder = rec.heading.claimed_by().unwrap_or(seat).to_string();
        let old_holder = crate::took_over_from(&holder, &current, &rec.heading.id);
        let item = QuietHeld {
            label: rec.heading.id.clone(),
            ticket: rec.heading.id.clone(),
            graph_id: String::new(),
            actor: named
                .as_ref()
                .map(|(actor, _)| actor.clone())
                .unwrap_or_default(),
            holder,
            old_holder,
            quiet,
        };
        if named
            .as_ref()
            .is_some_and(|(_, hold)| crate::hold_alive(hold))
        {
            kept.push(item);
        } else {
            release.push(item);
        }
    }
    (release, kept)
}

struct GraphNode {
    id: String,
    actor: String,
    ticket: String,
    updated: Option<u64>,
}

fn graph_node(row: &serde_json::Value) -> Option<GraphNode> {
    if !matches!(row["status"].as_str(), Some("claimed" | "running")) {
        return None;
    }
    let id = row["id"].as_str()?.to_string();
    let actor = row["assignee"].as_str()?.to_string();
    if actor.len() != 32 || actor.bytes().all(|b| b == b'0') {
        return None;
    }
    Some(GraphNode {
        id,
        actor,
        ticket: row["summary"].as_str().unwrap_or("").trim().to_string(),
        updated: row["updated_unix"].as_u64(),
    })
}

fn claimed_on<'a>(recs: &'a [IssueRec], id: &str) -> Option<&'a str> {
    if id.is_empty() {
        return None;
    }
    recs.iter()
        .find(|rec| rec.heading.id == id)
        .and_then(|rec| rec.heading.claimed_by())
}

/// This seat's claim. A ticket another seat holds is theirs even when a
/// hold record here names a different conversation. A hold for another
/// seat is theirs. No hold and no tracker row is not this seat's.
fn ours(seat: &str, claimed_by: Option<&str>, hold: Option<&crate::Hold>) -> bool {
    let want = crate::canonical_seat_name(seat);
    if want.is_empty() {
        return false;
    }
    if let Some(who) = claimed_by.map(crate::canonical_seat_name) {
        if !who.is_empty() && who != want {
            return false;
        }
    }
    if let Some(hold) = hold {
        let hold_seat = crate::canonical_seat_name(&hold.seat);
        let hold_who = crate::canonical_seat_name(&hold.assignee);
        if hold_seat == want || hold_who == want {
            return true;
        }
        if !hold_seat.is_empty() || !hold_who.is_empty() {
            return false;
        }
    }
    claimed_by
        .map(crate::canonical_seat_name)
        .is_some_and(|who| who == want)
}

fn claimed_quiet(rec: &IssueRec, now: u64) -> u64 {
    match rec.heading.claimed_at().and_then(crate::reap::org_ts) {
        Some(t) if t > 0 => now.saturating_sub(t as u64),
        _ => u64::MAX,
    }
}

/// The hold record whose sixth line names `ticket`. An alive one wins, so
/// one live conversation keeps the claim.
fn hold_on_ticket(ticket: &str) -> Option<(String, crate::Hold)> {
    let entries = std::fs::read_dir(crate::runtime_dir()).ok()?;
    let graph = crate::claim_graph_scope();
    let mut dead = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(actor) = name.strip_prefix("hold-") else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let mut lines = text.lines().map(str::trim);
        if lines.nth(5) != Some(ticket) {
            continue;
        }
        if lines.next().is_some_and(|g| !g.is_empty() && g != graph) {
            continue;
        }
        let Some(hold) = crate::read_hold(actor) else {
            continue;
        };
        if crate::hold_alive(&hold) {
            return Some((actor.to_string(), hold));
        }
        dead = Some((actor.to_string(), hold));
    }
    dead
}

fn hand_back(item: &QuietHeld) -> Result<()> {
    if !item.graph_id.is_empty() {
        crate::run_captured(
            "claimdag",
            &["release", &item.graph_id, "--actor", &item.actor],
        )?;
        if !item.actor.is_empty() {
            crate::drop_hold(&item.actor);
        }
    } else if !item.actor.is_empty() {
        crate::drop_hold(&item.actor);
    }
    if !item.ticket.is_empty() {
        crate::unstamp_tracker(&item.ticket, &item.holder, true)?;
        crate::drop_playbook(&item.ticket);
        let git = crate::persist_tracker(&item.ticket, "released");
        if !git.trim().is_empty() && !crate::routine_git_line(git.trim()) {
            bail!("{}", git.trim());
        }
    }
    Ok(())
}

#[cfg(test)]
mod duration_tests {
    use super::*;

    #[test]
    fn older_than_reads_seconds_and_units() {
        assert_eq!(parse_older_than("900").unwrap(), 900);
        assert_eq!(parse_older_than("15m").unwrap(), 900);
        assert_eq!(parse_older_than("6h").unwrap(), 21_600);
        assert_eq!(parse_older_than("2d").unwrap(), 172_800);
        assert_eq!(parse_older_than("45s").unwrap(), 45);
        assert!(parse_older_than("0").is_err());
        assert!(parse_older_than("6x").is_err());
        assert!(parse_older_than("h").is_err());
    }

    #[test]
    fn the_summary_counts_released_and_kept() {
        assert_eq!(
            summary_line(12, 3, false),
            "released 12 quiet claims; kept 3 held by running conversations\n"
        );
        assert_eq!(
            summary_line(1, 1, true),
            "would release 1 quiet claim; kept 1 held by a running conversation\n"
        );
        assert_eq!(summary_line(0, 0, false), "released 0 quiet claims\n");
        assert_eq!(summary_line(2, 0, true), "would release 2 quiet claims\n");
    }
}
