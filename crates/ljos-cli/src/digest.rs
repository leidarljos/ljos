//! One view of what the seats did.
//!
//! Rows come from the tracker git log, the issue files, `deedar evidence`,
//! the pack, and the guard log. Nothing here is a new store. The only file
//! this writes is the marker `last` reads, under the state directory.

use anyhow::{bail, Result};
use chrono::TimeZone;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::TrackerAct;

/// One thing a seat did.
struct Row {
    at: i64,
    seat: String,
    kind: &'static str,
    text: String,
    issue: String,
    /// A git ballot line, dropped when the votes drawer already shows it.
    drop_if_vote: bool,
}

impl Row {
    fn new(
        at: i64,
        seat: String,
        kind: &'static str,
        text: String,
        issue: impl Into<String>,
        drop_if_vote: bool,
    ) -> Self {
        Self {
            at,
            seat,
            kind,
            text,
            issue: issue.into(),
            drop_if_vote,
        }
    }
}

/// How far back a digest looks. `exclusive` is `last`: events at the marker
/// itself were already shown.
struct Window {
    start: Option<i64>,
    exclusive: bool,
}

impl Window {
    fn keeps(&self, at: i64) -> bool {
        match self.start {
            None => true,
            Some(start) if self.exclusive => at > start,
            Some(start) => at >= start,
        }
    }
}

/// What every seat did, newest first within the seat, seats newest first.
///
/// `since` is a day (`YYYY-MM-DD`), an RFC 3339 time, `YYYY-MM-DDTHH:MM`,
/// or `last`. `last` starts after the previous digest and says so when
/// there was none. A missing deed store or pack leaves those rows out.
/// The marker is the time this call started, so a write during the call
/// shows up next time.
///
/// # Errors
///
/// `since` is not a day, a time, or `last`. The tracker cannot be read.
/// The marker cannot be stored.
pub fn digest(since: Option<&str>, json: bool) -> Result<String> {
    let started = unix_now();
    let (window, since_label, prelude) = parse_since(since)?;
    let layout = vissue_core::Layout::resolve(None, None).map_err(|e| anyhow::anyhow!("{e}"))?;
    let root = layout.root().to_path_buf();
    let mut rows = Vec::new();
    let mut filed = BTreeSet::new();
    let mut claims = BTreeSet::new();
    collect_git(&root, &window, &mut rows, &mut filed, &mut claims);
    let headings = vissue_core::store::load_all(&layout).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut accessions = Vec::new();
    for (_project, heading) in &headings {
        collect_heading(
            heading,
            &window,
            &mut filed,
            &mut claims,
            &mut rows,
            &mut accessions,
        );
    }
    drop_shadowed_votes(&mut rows);
    collect_deeds(&accessions, &window, &mut rows);
    collect_guard(&window, &mut rows);
    collect_memory(&window, &mut rows);
    dedup(&mut rows);
    store_marker(started)?;
    let banner: Option<String> = None;
    if json {
        Ok(render_json(
            since_label.as_deref(),
            banner.as_deref(),
            &rows,
        ))
    } else {
        Ok(render_human(banner.as_deref(), &prelude, &rows))
    }
}

fn parse_since(since: Option<&str>) -> Result<(Window, Option<String>, String)> {
    let Some(raw) = since.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok((
            Window {
                start: None,
                exclusive: false,
            },
            None,
            String::new(),
        ));
    };
    if raw == "last" {
        let marker = read_marker();
        let prelude = if marker.is_none() {
            "no earlier digest; showing everything\n".to_string()
        } else {
            String::new()
        };
        return Ok((
            Window {
                start: marker,
                exclusive: marker.is_some(),
            },
            Some("last".to_string()),
            prelude,
        ));
    }
    let Some(start) = since_unix(raw) else {
        bail!("digest: since is a day (YYYY-MM-DD), a time, or last");
    };
    Ok((
        Window {
            start: Some(start),
            exclusive: false,
        },
        Some(raw.to_string()),
        String::new(),
    ))
}

fn since_unix(raw: &str) -> Option<i64> {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(t.timestamp());
    }
    if raw.len() == 10 || (raw.len() >= 16 && raw.as_bytes().get(10) == Some(&b'T')) {
        return stamp_unix(raw);
    }
    None
}

fn collect_git(
    root: &Path,
    window: &Window,
    rows: &mut Vec<Row>,
    filed: &mut BTreeSet<String>,
    claims: &mut BTreeSet<(String, String, &'static str)>,
) {
    let mut cmd = crate::git_at(root);
    cmd.args(["log", "--format=%ct%x09%s"]);
    cmd.stdin(std::process::Stdio::null());
    crate::git_hooks::hook_child_env(&mut cmd);
    let Ok(out) = cmd.output() else {
        return;
    };
    if !out.status.success() {
        return;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((stamp, subject)) = line.split_once('\t') else {
            continue;
        };
        let Ok(at) = stamp.trim().parse::<i64>() else {
            continue;
        };
        let Some(act) = crate::parse_tracker_commit(subject) else {
            continue;
        };
        let Some((kind, text, drop_if_vote)) = classify(&act) else {
            continue;
        };
        let verb = act.verb.to_ascii_lowercase();
        if verb.starts_with("filed") {
            filed.insert(act.issue.clone());
        } else if text.starts_with("took ") {
            claims.insert((act.issue.clone(), act.seat.clone(), "took"));
        } else if text.starts_with("released ") {
            claims.insert((act.issue.clone(), act.seat.clone(), "released"));
        }
        if !window.keeps(at) {
            continue;
        }
        rows.push(Row::new(at, act.seat, kind, text, act.issue, drop_if_vote));
    }
}

fn classify(act: &TrackerAct) -> Option<(&'static str, String, bool)> {
    let verb = act.verb.to_ascii_lowercase();
    let issue = act.issue.as_str();
    if verb.contains("consensus") {
        return Some((
            "consensus",
            format!("consensus {issue} {}", act.verb),
            false,
        ));
    }
    if verb.starts_with("filed") {
        return Some(("ticket", format!("filed {issue}"), false));
    }
    if verb == "claimed" || verb.starts_with("claimed ") {
        return Some(("claim", format!("took {issue}"), false));
    }
    if verb == "released"
        || verb == "release"
        || verb.starts_with("released")
        || verb.starts_with("release ")
    {
        return Some(("claim", format!("released {issue}"), false));
    }
    if verb.starts_with("finished") {
        return Some(("ticket", format!("finished {issue}"), false));
    }
    if verb.contains("ballot") {
        let text = if verb.contains("withdraw") {
            format!("withdrew a ballot on {issue}")
        } else {
            format!("voted {issue}")
        };
        return Some(("vote", text, !verb.contains("withdraw")));
    }
    if verb.contains("deed") {
        let text = if verb.contains("removed") {
            format!("removed a deed citation on {issue}")
        } else {
            format!("cited a deed on {issue}")
        };
        return Some(("deed", text, false));
    }
    None
}

fn collect_heading(
    heading: &vissue_core::IssueHeading,
    window: &Window,
    filed: &mut BTreeSet<String>,
    claims: &mut BTreeSet<(String, String, &'static str)>,
    rows: &mut Vec<Row>,
    accessions: &mut Vec<String>,
) {
    let id = heading.id.as_str();
    for entry in &heading.logbook {
        let Some(at) = stamp_unix(&entry.timestamp) else {
            continue;
        };
        if let Some(note) = entry.note.as_deref() {
            if note_row(id, note, at, window, filed, claims, rows) {
                continue;
            }
        }
        if let Some(to) = entry.to_state.as_deref() {
            if to == "DONE" || to == "CANCELLED" {
                let seat =
                    seat_of_stamp(heading, &entry.timestamp).unwrap_or_else(|| "tracker".into());
                push(
                    rows,
                    window,
                    Row::new(at, seat, "ticket", format!("closed {id}"), id, false),
                );
            }
        }
    }
    if !filed.contains(id) {
        if let Some(stamp) = heading.properties.get("CREATED") {
            if let Some(at) = stamp_unix(stamp) {
                filed.insert(id.to_string());
                push(
                    rows,
                    window,
                    Row::new(
                        at,
                        "tracker".into(),
                        "ticket",
                        format!("filed {id}"),
                        id,
                        false,
                    ),
                );
            }
        }
    }
    for (at, agent, choice) in vote_lines(heading) {
        push(
            rows,
            window,
            Row::new(
                at,
                agent,
                "vote",
                format!("voted {id} for {choice}"),
                id,
                false,
            ),
        );
    }
    if let (Some(who), Some(when)) = (heading.claimed_by(), heading.claimed_at()) {
        if let Some(at) = stamp_unix(when) {
            let key = (id.to_string(), who.to_string(), "took");
            if claims.insert(key) {
                push(
                    rows,
                    window,
                    Row::new(
                        at,
                        who.to_string(),
                        "claim",
                        format!("took {id}"),
                        id,
                        false,
                    ),
                );
            }
        }
    }
    accessions.extend(heading.deeds());
}

/// `true` when `note` was a digest row (or a duplicate of one), so the
/// state flip on the same line is not also read. Notes are not state flips.
fn note_row(
    id: &str,
    note: &str,
    at: i64,
    window: &Window,
    filed: &mut BTreeSet<String>,
    claims: &mut BTreeSet<(String, String, &'static str)>,
    rows: &mut Vec<Row>,
) -> bool {
    let lower = note.trim().to_ascii_lowercase();
    if lower.starts_with("consensus") {
        let seat = act_of(note)
            .map(|(_, seat)| seat)
            .unwrap_or_else(|| "tracker".into());
        push(
            rows,
            window,
            Row::new(at, seat, "consensus", consensus_text(id, note), id, false),
        );
        return true;
    }
    let Some((verb, seat)) = act_of(note) else {
        return false;
    };
    let verb = verb.to_ascii_lowercase();
    if verb.starts_with("filed") {
        if filed.insert(id.to_string()) {
            push(
                rows,
                window,
                Row::new(at, seat, "ticket", format!("filed {id}"), id, false),
            );
        }
        return true;
    }
    if verb.starts_with("claimed") {
        let key = (id.to_string(), seat.clone(), "took");
        if claims.insert(key) {
            push(
                rows,
                window,
                Row::new(at, seat, "claim", format!("took {id}"), id, false),
            );
        }
        return true;
    }
    if verb == "release" || verb.starts_with("released") || verb.starts_with("release ") {
        let key = (id.to_string(), seat.clone(), "released");
        if claims.insert(key) {
            push(
                rows,
                window,
                Row::new(at, seat, "claim", format!("released {id}"), id, false),
            );
        }
        return true;
    }
    // A seat stamp such as "Finished by alice" names who closed the issue.
    // It is not a second event beside the state flip and the git subject.
    true
}

fn consensus_text(issue: &str, note: &str) -> String {
    let body = note.trim();
    let body = body
        .get(.."consensus".len())
        .filter(|head| head.eq_ignore_ascii_case("consensus"))
        .map(|_| body["consensus".len()..].trim())
        .unwrap_or(body);
    let body = body.split(" by ").next().unwrap_or(body).trim();
    if body.is_empty() {
        format!("consensus {issue}")
    } else {
        format!("consensus {issue} {body}")
    }
}

fn act_of(note: &str) -> Option<(String, String)> {
    crate::parse_logbook_act(&format!("- {note}"))
}

fn seat_of_stamp(heading: &vissue_core::IssueHeading, stamp: &str) -> Option<String> {
    heading.logbook.iter().find_map(|entry| {
        if entry.timestamp != stamp {
            return None;
        }
        act_of(entry.note.as_deref()?).map(|(_, seat)| seat)
    })
}

fn vote_lines(heading: &vissue_core::IssueHeading) -> Vec<(i64, String, String)> {
    let mut out = Vec::new();
    for drawer in &heading.extra_drawers {
        let mut lines = drawer.lines();
        let Some(first) = lines.next() else {
            continue;
        };
        if !first.trim().eq_ignore_ascii_case(":VOTES:") {
            continue;
        }
        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.eq_ignore_ascii_case(":END:") {
                continue;
            }
            if let Some(vote) = parse_vote(line) {
                out.push(vote);
            }
        }
    }
    out
}

fn parse_vote(line: &str) -> Option<(i64, String, String)> {
    let (stamp, rest) = line.strip_prefix('[')?.split_once("] ")?;
    let (agent, choice) = rest.split_once(": ")?;
    let agent = agent.trim();
    if agent.is_empty() {
        return None;
    }
    let choice = choice.split(" used=").next().unwrap_or(choice).trim();
    let choice = choice.split(" confidence=").next().unwrap_or(choice).trim();
    if choice.is_empty() {
        return None;
    }
    let at = stamp_unix(&format!("[{stamp}]"))?;
    Some((at, agent.to_string(), choice.to_string()))
}

fn drop_shadowed_votes(rows: &mut Vec<Row>) {
    let shown: BTreeSet<(String, String)> = rows
        .iter()
        .filter(|row| row.kind == "vote" && !row.drop_if_vote)
        .map(|row| (row.issue.clone(), row.seat.clone()))
        .collect();
    rows.retain(|row| !row.drop_if_vote || !shown.contains(&(row.issue.clone(), row.seat.clone())));
}

fn collect_deeds(accessions: &[String], window: &Window, rows: &mut Vec<Row>) {
    if accessions.is_empty() || which::which("deedar").is_err() {
        return;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
    let mut seen = BTreeSet::new();
    for accession in accessions {
        if std::time::Instant::now() >= deadline {
            break;
        }
        if !seen.insert(accession.clone()) {
            continue;
        }
        let Ok(said) = crate::run_captured_within(
            "deedar",
            &["evidence", accession.as_str()],
            None,
            std::time::Duration::from_millis(400),
        ) else {
            continue;
        };
        let Some(ev) = crate::deed_event(accession, &said.stdout, crate::local_offset) else {
            continue;
        };
        let Some(at) = said
            .stdout
            .lines()
            .find_map(|line| line.strip_prefix("time="))
            .and_then(|t| t.trim().parse::<i64>().ok())
        else {
            continue;
        };
        let by = said
            .stdout
            .lines()
            .find_map(|line| line.strip_prefix("producedBy="))
            .map(str::trim)
            .unwrap_or("");
        let seat = if by.is_empty() || by == "-" {
            "seat".into()
        } else {
            by.to_string()
        };
        push(rows, window, Row::new(at, seat, "deed", ev.text, "", false));
    }
}

fn collect_memory(window: &Window, rows: &mut Vec<Row>) {
    let Ok(client) = crate::pack() else {
        return;
    };
    let Ok(atoms) = crate::atoms_lean(&client, &client.workspace()) else {
        return;
    };
    push_memory(&atoms, window, rows);
}

fn push_memory(atoms: &[serde_json::Value], window: &Window, rows: &mut Vec<Row>) {
    for atom in atoms {
        let Some(mem) = crate::memories::Memory::of(atom) else {
            continue;
        };
        let Some(at) = stamp_unix(&mem.at) else {
            continue;
        };
        let seat = if mem.seat.trim().is_empty() {
            "seat".into()
        } else {
            mem.seat
        };
        push(
            rows,
            window,
            Row::new(
                at,
                seat,
                "memory",
                format!("memory: {}", crate::capture::excerpt_of(&mem.text)),
                "",
                false,
            ),
        );
    }
}

fn collect_guard(window: &Window, rows: &mut Vec<Row>) {
    let Some(path) = guard_path() else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let recorded = value["kind"].as_str().unwrap_or("");
        if recorded != "refusal" && recorded != "rewrite" {
            continue;
        }
        let Some(at) = value["ts"].as_str().and_then(stamp_unix) else {
            continue;
        };
        let seat = value["seat"].as_str().unwrap_or("").trim();
        let seat = if seat.is_empty() {
            "seat".into()
        } else {
            seat.to_string()
        };
        let body = value["text"].as_str().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let text = if recorded == "rewrite" {
            format!("rewrote {body}")
        } else {
            format!("refused {body}")
        };
        push(rows, window, Row::new(at, seat, "guard", text, "", false));
    }
}

fn push(rows: &mut Vec<Row>, window: &Window, mut row: Row) {
    if !window.keeps(row.at) {
        return;
    }
    let seat = row.seat.trim();
    row.seat = if seat.is_empty() {
        "seat".to_string()
    } else {
        seat.to_string()
    };
    rows.push(row);
}

fn dedup(rows: &mut Vec<Row>) {
    let mut seen = BTreeSet::new();
    rows.retain(|row| seen.insert((row.at, row.seat.clone(), row.kind, row.text.clone())));
}

fn render_human(banner: Option<&str>, prelude: &str, rows: &[Row]) -> String {
    let mut out = String::new();
    if let Some(banner) = banner {
        out.push_str(banner);
        if !banner.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    if !prelude.is_empty() {
        out.push_str(prelude);
        if !prelude.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    if rows.is_empty() {
        out.push_str("nothing in that window\n");
        return out;
    }
    for (seat, events) in grouped(rows) {
        out.push_str(&seat);
        out.push('\n');
        for event in events {
            out.push_str(&format!("  {}  {}\n", wall(event.at), event.text));
        }
    }
    out
}

fn render_json(since: Option<&str>, banner: Option<&str>, rows: &[Row]) -> String {
    let seats: Vec<serde_json::Value> = grouped(rows)
        .into_iter()
        .map(|(seat, events)| {
            let events: Vec<serde_json::Value> = events
                .into_iter()
                .map(|event| {
                    serde_json::json!({
                        "at": rfc3339(event.at),
                        "kind": event.kind,
                        "text": event.text,
                    })
                })
                .collect();
            serde_json::json!({ "seat": seat, "events": events })
        })
        .collect();
    let value = serde_json::json!({
        "since": since,
        "sync": banner.map(str::trim_end),
        "seats": seats,
    });
    format!("{value}\n")
}

fn grouped(rows: &[Row]) -> Vec<(String, Vec<&Row>)> {
    let mut seats: Vec<(String, Vec<&Row>)> = Vec::new();
    for row in rows {
        match seats.iter_mut().find(|(seat, _)| seat == &row.seat) {
            Some((_, events)) => events.push(row),
            None => seats.push((row.seat.clone(), vec![row])),
        }
    }
    for (_, events) in &mut seats {
        events.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.text.cmp(&b.text)));
    }
    seats.sort_by(|a, b| {
        let an = a.1.first().map(|row| row.at).unwrap_or(0);
        let bn = b.1.first().map(|row| row.at).unwrap_or(0);
        bn.cmp(&an).then_with(|| a.0.cmp(&b.0))
    });
    seats
}

fn wall(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "unknown".into())
}

fn rfc3339(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn stamp_unix(stamp: &str) -> Option<i64> {
    let s = stamp
        .trim()
        .trim_start_matches(['[', '<'])
        .trim_end_matches([']', '>']);
    if s.is_empty() {
        return None;
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(t.timestamp());
    }
    let date = s.get(..10)?;
    let day = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let clock = s[10..]
        .split(['T', ' '])
        .find(|part| part.len() >= 5 && part.as_bytes().get(2) == Some(&b':'));
    let (hour, min, sec) = if let Some(clock) = clock {
        let hour = clock.get(..2)?.parse().ok()?;
        let min = clock.get(3..5)?.parse().ok()?;
        let sec = clock.get(6..8).and_then(|t| t.parse().ok()).unwrap_or(0);
        (hour, min, sec)
    } else {
        (0, 0, 0)
    };
    let naive = day.and_hms_opt(hour, min, sec)?;
    Some(match chrono::Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) => t.timestamp(),
        chrono::LocalResult::Ambiguous(earlier, _) => earlier.timestamp(),
        chrono::LocalResult::None => naive.and_utc().timestamp(),
    })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn marker_path() -> PathBuf {
    crate::note_queue::state_dir().join("digest-last")
}

fn read_marker() -> Option<i64> {
    std::fs::read_to_string(marker_path())
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

fn store_marker(at: i64) -> Result<()> {
    let path = marker_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{at}\n"))?;
    Ok(())
}

fn guard_path() -> Option<PathBuf> {
    crate::data_base().map(|base| base.join("ljos").join("guard.jsonl"))
}

fn one_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 240 {
        return flat;
    }
    let mut cut: String = flat.chars().take(239).collect();
    cut.push('…');
    cut
}

/// Append one guard row. A row that cannot be written is skipped.
#[inline(never)]
pub fn note_guard(kind: &str, seat: &str, text: &str) {
    if kind != "refusal" && kind != "rewrite" {
        return;
    }
    let Some(path) = guard_path() else {
        return;
    };
    let text = one_line(text);
    if text.is_empty() {
        return;
    }
    let seat = seat.trim();
    let seat = if seat.is_empty() { "seat" } else { seat };
    let row = serde_json::json!({
        "ts": crate::now_utc(),
        "kind": kind,
        "seat": seat,
        "text": text,
    });
    let Ok(mut line) = serde_json::to_string(&row) else {
        return;
    };
    line.push('\n');
    let _ = path.parent().map(std::fs::create_dir_all);
    let _ = crate::append_locked_row(&path, line.as_bytes(), crate::Durability::BestEffort);
}

/// A policyd allow that rewrites the line. The new line is the row.
#[inline(never)]
pub(crate) fn note_policyd_line(text: &str) {
    let Some(rest) = text.trim().strip_prefix("allow\trewrite\t") else {
        return;
    };
    let seat = std::env::var("LJOS_SEAT")
        .ok()
        .filter(|seat| !seat.trim().is_empty())
        .unwrap_or_else(|| "seat".into());
    note_guard("rewrite", &seat, rest);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn git(dir: &std::path::Path, args: &[&str], date: &str) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn isolate(root: &std::path::Path, home: &std::path::Path, path: &str) {
        unsafe {
            std::env::set_var("HOME", home);
            std::env::set_var("XDG_STATE_HOME", home.join("state"));
            std::env::set_var("XDG_DATA_HOME", home.join("data"));
            std::env::set_var("XDG_CONFIG_HOME", home.join("config"));
            std::env::set_var("VISSUE_ROOT", root);
            std::env::remove_var("ISSUE_ROOT");
            std::env::remove_var("VISSUE_PREFIX");
            std::env::set_var("PATH", path);
            std::env::remove_var("LJOS_TRACKER_GIT");
            std::env::remove_var("LJOS_SEAT");
        }
    }

    fn tracker(dir: &std::path::Path) -> std::path::PathBuf {
        let root = dir.join("tracker");
        std::fs::create_dir_all(root.join("Software/probe")).unwrap();
        git(&root, &["init", "-q", "-b", "main"], "2020-01-01T00:00:00Z");
        git(
            &root,
            &["config", "user.email", "seat@example.invalid"],
            "2020-01-01T00:00:00Z",
        );
        git(
            &root,
            &["config", "user.name", "seat"],
            "2020-01-01T00:00:00Z",
        );
        git(
            &root,
            &["config", "commit.gpgsign", "false"],
            "2020-01-01T00:00:00Z",
        );
        git(
            &root,
            &["config", "core.hooksPath", "/dev/null"],
            "2020-01-01T00:00:00Z",
        );
        root
    }

    fn seat_block(text: &str, seat: &str) -> String {
        let mut out = String::new();
        let mut on = false;
        for line in text.lines() {
            if !on && line == seat {
                on = true;
                continue;
            }
            if on {
                if line.starts_with(' ') {
                    out.push_str(line);
                    out.push('\n');
                } else if !line.is_empty() {
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn digest_groups_seats_newest_first() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = tracker(dir.path());
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let stub = bin.join("deedar");
        std::fs::write(
            &stub,
            "#!/bin/sh\nprintf 'time=1760000000\\nproducedBy=ada\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let org = "\
* DONE [#C] Close the probe
:PROPERTIES:
:ID:         probe-a
:DEEDS:      deed-1
:END:
:LOGBOOK:
- State \"DONE\" from \"TODO\" [2026-10-29 Thu 09:14]
- Finished by alice [2026-10-29 Thu 09:14]
- consensus ship by carol [2026-10-29 Thu 08:00]
- Released by alice [2026-10-29 Thu 07:00]
:END:
:VOTES:
[2026-10-29 Thu 09:02] alice: ship used=none
:END:

* TODO [#C] File the other
:PROPERTIES:
:ID:         probe-b
:END:
";
        std::fs::write(root.join("Software/probe/issues.org"), org).unwrap();
        git(&root, &["add", "."], "2020-01-01T00:00:00Z");
        git(
            &root,
            &["commit", "-q", "-m", "seed"],
            "2020-01-01T00:00:00Z",
        );
        for (date, message) in [
            (
                "2020-06-01T00:00:00Z",
                "chore(issues): probe-b filed by bob",
            ),
            (
                "2020-06-02T00:00:00Z",
                "chore(issues): probe-a claimed by alice",
            ),
            (
                "2020-06-03T00:00:00Z",
                "chore(issues): probe-a ballot cast by alice",
            ),
            (
                "2020-06-04T00:00:00Z",
                "chore(issues): probe-a finished by alice",
            ),
            (
                "2020-06-05T00:00:00Z",
                "chore(issues): probe-a cited a deed by ada",
            ),
        ] {
            git(
                &root,
                &["commit", "-q", "--allow-empty", "-m", message],
                date,
            );
        }
        let guard = home.join("data/ljos");
        std::fs::create_dir_all(&guard).unwrap();
        std::fs::write(
            guard.join("guard.jsonl"),
            "{\"ts\":\"2026-10-27T12:00:00Z\",\"kind\":\"refusal\",\"seat\":\"erin\",\"text\":\"rm -rf /tmp/probe\"}\n\
{\"ts\":\"2026-10-27T12:05:00Z\",\"kind\":\"rewrite\",\"seat\":\"erin\",\"text\":\"git status\"}\n",
        )
        .unwrap();
        let (url, stored) = crate::serve_sync_pack(crate::SyncPack::Keep);
        stored.lock().unwrap().push(serde_json::json!({
            "id": "mem1",
            "kind": "fact",
            "text": "the rail is steel",
            "ts": "2026-10-28T18:00:00Z",
            "origin": "user-declared",
            "source": {"seat": "bob", "at": "2026-10-28T18:00:00Z"}
        }));
        let path = format!("{}:/usr/bin:/bin", bin.display());
        isolate(&root, &home, &path);
        unsafe { std::env::set_var("PACKSET_URL", &url) };

        let text = digest(None, false).unwrap();
        let first = text
            .lines()
            .find(|line| {
                !line.is_empty() && !line.starts_with(' ') && !line.starts_with("tracker:")
            })
            .unwrap_or("");
        assert_eq!(first, "alice", "{text}");
        let alice = seat_block(&text, "alice");
        let closed = alice.find("closed probe-a").expect(&text);
        let voted = alice.find("voted probe-a for ship").expect(&text);
        assert!(closed < voted, "{alice}");
        assert!(alice.contains("took probe-a"), "{alice}");
        assert!(alice.contains("released probe-a"), "{alice}");
        assert!(alice.contains("finished probe-a"), "{alice}");
        assert_eq!(
            text.lines()
                .filter(|line| line.contains("voted probe-a"))
                .count(),
            1,
            "{text}"
        );
        assert!(text.contains("filed probe-b"), "{text}");
        assert!(text.contains("consensus probe-a ship"), "{text}");
        assert!(
            seat_block(&text, "carol").contains("consensus probe-a ship"),
            "{text}"
        );
        assert!(text.contains("cited a deed on probe-a"), "{text}");
        assert!(text.contains("deed-1 produced by ada"), "{text}");
        let erin = seat_block(&text, "erin");
        assert!(erin.contains("refused rm -rf /tmp/probe"), "{erin}");
        assert!(erin.contains("rewrote git status"), "{erin}");
        assert!(text.contains("memory: the rail is steel"), "{text}");
        let fresh = erin.lines().next().unwrap_or("");
        assert!(
            fresh.contains("rewrote") || fresh.contains("refused"),
            "{erin}"
        );

        let value: serde_json::Value = serde_json::from_str(&digest(None, true).unwrap()).unwrap();
        assert!(value["since"].is_null(), "{value}");
        assert!(value["sync"].is_null(), "{value}");
        let kinds: BTreeSet<&str> = value["seats"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|seat| seat["events"].as_array().unwrap())
            .filter_map(|event| event["kind"].as_str())
            .collect();
        for kind in [
            "ticket",
            "vote",
            "consensus",
            "deed",
            "claim",
            "guard",
            "memory",
        ] {
            assert!(kinds.contains(kind), "{kind} missing in {value}");
        }

        let future = digest(Some("2999-01-01"), false).unwrap();
        assert!(future.contains("nothing in that window"), "{future}");
        let past = digest(Some("2000-01-01"), false).unwrap();
        assert!(past.contains("filed probe-b"), "{past}");
    }

    #[test]
    fn last_remembers_the_previous_run() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = tracker(dir.path());
        let home = dir.path().join("home");
        std::fs::create_dir_all(root.join("Software/probe")).unwrap();
        std::fs::write(
            root.join("Software/probe/issues.org"),
            "* TODO [#C] One\n:PROPERTIES:\n:ID:         probe-b\n:END:\n",
        )
        .unwrap();
        git(&root, &["add", "."], "2020-01-01T00:00:00Z");
        git(
            &root,
            &["commit", "-q", "-m", "chore(issues): probe-b filed by bob"],
            "2020-01-01T00:00:00Z",
        );
        isolate(&root, &home, "/usr/bin:/bin");
        unsafe { std::env::set_var("PACKSET_URL", "off") };
        let first = digest(Some("last"), false).unwrap();
        assert!(first.contains("no earlier digest"), "{first}");
        assert!(first.contains("filed probe-b"), "{first}");
        let marker = std::fs::read_to_string(home.join("state/ljos/digest-last")).unwrap();
        assert!(marker.trim().parse::<i64>().is_ok(), "{marker}");
        let second = digest(Some("last"), false).unwrap();
        assert!(!second.contains("filed probe-b"), "{second}");
        assert!(second.contains("nothing in that window"), "{second}");
        assert!(digest(Some("noon"), false).is_err());
    }

    #[test]
    fn a_guard_row_is_one_json_line() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_DATA_HOME", home.join("data"));
            std::env::set_var("LJOS_SEAT", "erin");
        }
        note_guard("refusal", "erin", "rm -rf /tmp/probe");
        note_policyd_line("allow\trewrite\tgit status");
        note_policyd_line("allow\tgit status");
        let text = std::fs::read_to_string(home.join("data/ljos/guard.jsonl")).unwrap();
        let rows: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 2, "{text}");
        assert_eq!(rows[0]["kind"], "refusal");
        assert_eq!(rows[1]["kind"], "rewrite");
        assert_eq!(rows[1]["text"], "git status");
    }

    #[test]
    fn digest_answers_a_thousand_tickets_within_two_seconds() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = tracker(dir.path());
        let home = dir.path().join("home");
        let mut org = String::from("#+TITLE: bench\n#+TODO: TODO | DONE\n\n");
        for i in 0..1100 {
            org.push_str(&format!(
                "* TODO [#C] Ticket {i}\n:PROPERTIES:\n:ID:         bench-{i:04}\n:END:\n\n"
            ));
        }
        std::fs::write(root.join("Software/probe/issues.org"), org).unwrap();
        git(&root, &["add", "."], "2020-01-01T00:00:00Z");
        git(
            &root,
            &[
                "commit",
                "-q",
                "-m",
                "chore(issues): bench-0042 filed by ada",
            ],
            "2020-01-01T00:00:00Z",
        );
        isolate(&root, &home, "/usr/bin:/bin");
        unsafe { std::env::set_var("PACKSET_URL", "off") };
        let started = std::time::Instant::now();
        let text = digest(None, false).unwrap();
        let took = started.elapsed();
        assert!(
            took < std::time::Duration::from_secs(2),
            "digest took {took:?}"
        );
        assert!(text.contains("filed bench-0042"), "{text}");
        assert!(
            text.contains("ada\n") || text.starts_with("ada\n"),
            "{text}"
        );
    }
}
