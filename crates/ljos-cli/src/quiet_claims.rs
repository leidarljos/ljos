//! Claims this seat still holds and has not noted, cited or finished for a day.
//!
//! `ljos sitting` and `ljos inbox` name a few. `ljos claims --mine` lists
//! every one. Claims untouched for longer than [`stale_days_from_env`] are
//! one line on that banner. `ljos claims --stale` lists them. `ljos release
//! --stale` hands them back, and only when `--yes` is set. A release drops
//! the claim. It does not close the ticket.

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

const ELLIPSIS: &str = "...";

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
        "{}  {}  {}  ljos finish {} / ljos note {} \"...\"\n",
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
        format!("ljos finish {id}"),
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
    let flat = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= TITLE_SHOWN {
        return flat;
    }
    let keep = TITLE_SHOWN.saturating_sub(ELLIPSIS.chars().count());
    let mut cut: String = flat.chars().take(keep).collect();
    cut.push_str(ELLIPSIS);
    cut
}
