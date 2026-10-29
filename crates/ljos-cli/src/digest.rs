//! One view of what the seats did.
//!
//! Rows come from the tracker git log, the issue files, `deedar evidence`,
//! the pack, and the guard log. Nothing here is a new store. The marker
//! `last` reads is written only when a plain digest asks for it, or when
//! the mark flag is set. The MCP tool never writes it.

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
    source: &'static str,
    identity: Option<String>,
    /// A git ballot line, dropped when a timed votes drawer already shows it.
    drop_if_vote: bool,
    /// An org stamp with a clock and no seconds. `last` keeps that minute.
    minute: bool,
    /// An org stamp with no clock. A timed git row replaces it.
    date_only: bool,
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
            source: "org",
            identity: None,
            drop_if_vote,
            minute: false,
            date_only: false,
        }
    }

    fn from(mut self, source: &'static str) -> Self {
        self.source = source;
        self
    }

    fn with_identity(mut self, at: i64, identity: &str) -> Self {
        self.identity = Some(format!("{at}\t{identity}"));
        self
    }

    fn key(&self) -> String {
        if let Some(identity) = &self.identity {
            return format!("{}\t{identity}", self.source);
        }
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.source, self.at, self.seat, self.kind, self.issue, self.text
        )
    }
}

/// How far back a digest looks. `exclusive` is `last`: events at the marker
/// itself were already shown.
struct Window {
    start: Option<i64>,
    exclusive: bool,
    seen: BTreeSet<String>,
    head: Option<String>,
}

impl Window {
    fn keeps(&self, at: i64, minute: bool) -> bool {
        match self.start {
            None => true,
            // A minute stamp names the whole minute. The marker sits
            // somewhere in it, so the event still belongs in `last`.
            Some(start) if self.exclusive && minute => at + 59 > start,
            Some(start) if self.exclusive => at > start,
            Some(start) => at >= start,
        }
    }

    fn keeps_row(&self, row: &Row) -> bool {
        self.keeps(row.at, row.minute) && (!self.exclusive || !self.seen.contains(&row.key()))
    }
}

/// How many memory rows one digest prints. The rest of the pack stays put.
const MEMORY_CAP: usize = 32;

/// `guard.jsonl` is rotated once it passes this size.
const GUARD_CAP: u64 = 64 * 1024;

/// What every seat did, newest first within the seat, seats newest first.
///
/// `since` is a day (`YYYY-MM-DD`), an RFC 3339 time, `YYYY-MM-DDTHH:MM`,
/// or `last`. `last` starts after the previous digest and says so when
/// there was none. A missing deed store or pack leaves those rows out.
/// A plain call stores the marker. JSON does not, unless `mark` is set.
/// The marker is the time this call started, so a write during the call
/// shows up next time.
///
/// # Errors
///
/// `since` is not a day, a time, or `last`. The tracker cannot be read.
/// The marker cannot be stored.
pub fn digest(since: Option<&str>, json: bool) -> Result<String> {
    digest_cli(since, json, !json)
}

/// The shell. `mark` forces the marker. A plain digest (no JSON) marks
/// even when `mark` is false.
///
/// # Errors
///
/// As [`digest`].
pub fn digest_cli(since: Option<&str>, json: bool, mark: bool) -> Result<String> {
    show(since, json, mark || !json, true)
}

/// The MCP tool. It never moves the marker, and a refused command is not
/// repeated: the row says that a command was refused.
///
/// # Errors
///
/// As [`digest`].
pub fn digest_for_tool(since: Option<&str>, json: bool) -> Result<String> {
    show(since, json, false, false)
}

fn show(since: Option<&str>, json: bool, mark: bool, guard_commands: bool) -> Result<String> {
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
    let cutoff = collect_deeds(&accessions, &window, &mut rows);
    collect_guard(&window, &mut rows, guard_commands);
    collect_memory(&window, &mut rows);
    dedup(&mut rows);
    if mark {
        store_marker(started, &root, &rows)?;
    }
    let banner = crate::tracker_sync::tracker_sync_line();
    if json {
        Ok(render_json(
            since_label.as_deref(),
            banner.as_deref(),
            &cutoff,
            &rows,
        ))
    } else {
        Ok(render_human(banner.as_deref(), &prelude, &cutoff, &rows))
    }
}

fn parse_since(since: Option<&str>) -> Result<(Window, Option<String>, String)> {
    let Some(raw) = since.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok((
            Window {
                start: None,
                exclusive: false,
                seen: BTreeSet::new(),
                head: None,
            },
            None,
            String::new(),
        ));
    };
    if raw == "last" {
        let marker = read_marker();
        let state = marker_state();
        let prelude = if marker.is_none() {
            "no earlier digest; showing everything\n".to_string()
        } else {
            String::new()
        };
        return Ok((
            Window {
                start: marker,
                exclusive: marker.is_some(),
                seen: state.seen.into_iter().collect(),
                head: state.head,
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
            seen: BTreeSet::new(),
            head: None,
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
    let new_commits = commits_since_marker(root, window);
    let mut cmd = crate::git_at(root);
    cmd.args(["log", "--format=%H%x09%at%x09%ct%x09%s"]);
    cmd.stdin(std::process::Stdio::null());
    crate::git_hooks::hook_child_env(&mut cmd);
    let Ok(out) = cmd.output() else {
        return;
    };
    if !out.status.success() {
        return;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut fields = line.splitn(4, '\t');
        let (Some(commit), Some(authored), Some(committed), Some(subject)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(authored), Ok(at)) = (
            authored.trim().parse::<i64>(),
            committed.trim().parse::<i64>(),
        ) else {
            continue;
        };
        let Some(act) = crate::parse_tracker_commit(subject) else {
            continue;
        };
        let Some((kind, text, drop_if_vote)) = classify(&act) else {
            continue;
        };
        let seat = fold_seat(&act.seat);
        let verb = act.verb.to_ascii_lowercase();
        if verb.starts_with("filed") {
            filed.insert(act.issue.clone());
        } else if text.starts_with("took ") {
            claims.insert((act.issue.clone(), seat.clone(), "took"));
        } else if text.starts_with("released ") {
            claims.insert((act.issue.clone(), seat.clone(), "released"));
        }
        let row = Row::new(at, seat, kind, text, act.issue, drop_if_vote)
            .from("git")
            .with_identity(authored, subject);
        push_git(
            rows,
            window,
            row,
            new_commits.as_ref().is_some_and(|new| new.contains(commit)),
        );
    }
    collect_introduced(root, window, &new_commits, rows, filed);
}

fn commits_since_marker(root: &Path, window: &Window) -> Option<BTreeSet<String>> {
    let head = window.head.as_deref()?;
    let range = format!("{head}..HEAD");
    let commits = crate::git_ok_stdout(root, &["rev-list", &range])?;
    Some(commits.lines().map(str::to_string).collect())
}

/// An issue id added by a commit that is not itself `filed ID`. The seat
/// on that commit is who filed it, and the author time replaces a
/// date-only `CREATED` stamp.
fn collect_introduced(
    root: &Path,
    window: &Window,
    new_commits: &Option<BTreeSet<String>>,
    rows: &mut Vec<Row>,
    filed: &mut BTreeSet<String>,
) {
    let mut cmd = crate::git_at(root);
    cmd.args([
        "log",
        "--reverse",
        "-p",
        "--unified=0",
        "--format=%x1e%H%x09%at%x09%ct%x09%s",
    ]);
    cmd.stdin(std::process::Stdio::null());
    crate::git_hooks::hook_child_env(&mut cmd);
    let Ok(out) = cmd.output() else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let mut authored = 0_i64;
    let mut at = 0_i64;
    let mut commit = String::new();
    let mut seat = String::new();
    let mut subject = String::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(rest) = line.strip_prefix('\u{1e}') {
            let mut fields = rest.splitn(4, '\t');
            let (Some(commit_hash), Some(author_stamp), Some(commit_stamp), Some(commit_subject)) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                seat.clear();
                continue;
            };
            commit = commit_hash.to_string();
            authored = author_stamp.trim().parse().unwrap_or(0);
            at = commit_stamp.trim().parse().unwrap_or(0);
            subject = commit_subject.to_string();
            seat = crate::parse_tracker_commit(commit_subject)
                .map(|act| fold_seat(&act.seat))
                .unwrap_or_default();
            continue;
        }
        if seat.is_empty() {
            continue;
        }
        let Some(id) = added_issue_id(line) else {
            continue;
        };
        if !filed.insert(id.to_string()) {
            continue;
        }
        push_git(
            rows,
            window,
            Row::new(at, seat.clone(), "ticket", format!("filed {id}"), id, false)
                .from("git")
                .with_identity(authored, &subject),
            new_commits
                .as_ref()
                .is_some_and(|new| new.contains(&commit)),
        );
    }
}

fn added_issue_id(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('+')?.trim();
    let rest = rest.strip_prefix(":ID:")?.trim();
    if rest.is_empty() || rest.contains(char::is_whitespace) {
        return None;
    }
    Some(rest)
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
    if verb.contains("settled with notes") {
        return Some(("consensus", format!("settled with notes on {issue}"), false));
    }
    if verb.contains("started a workflow") {
        return Some(("ticket", format!("started a workflow on {issue}"), false));
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
        let Some(stamp) = read_stamp(&entry.timestamp) else {
            continue;
        };
        if let Some(note) = entry.note.as_deref() {
            if note_row(id, note, &stamp, window, filed, claims, rows) {
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
                    stamped(stamp, seat, "ticket", format!("closed {id}"), id, false),
                );
            }
        }
    }
    if !filed.contains(id) {
        if let Some(raw) = heading.properties.get("CREATED") {
            if let Some(stamp) = read_stamp(raw) {
                // A date-only CREATED row has no seat. A git `filed` row
                // already took this id. What remains is credited only when
                // the stamp itself names a clock; a bare day stays out
                // rather than appearing at midnight under "tracker".
                if !stamp.date_only {
                    filed.insert(id.to_string());
                    push(
                        rows,
                        window,
                        stamped(
                            stamp,
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
    }
    for vote in vote_lines(heading) {
        push(
            rows,
            window,
            stamped(
                vote.stamp,
                vote.agent,
                "vote",
                format!("voted {id} for {}", vote.choice),
                id,
                false,
            ),
        );
    }
    if let (Some(who), Some(when)) = (heading.claimed_by(), heading.claimed_at()) {
        if let Some(stamp) = read_stamp(when) {
            let who = fold_seat(who);
            let key = (id.to_string(), who.clone(), "took");
            if claims.insert(key) {
                push(
                    rows,
                    window,
                    stamped(stamp, who, "claim", format!("took {id}"), id, false),
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
    stamp: &Stamp,
    window: &Window,
    filed: &mut BTreeSet<String>,
    claims: &mut BTreeSet<(String, String, &'static str)>,
    rows: &mut Vec<Row>,
) -> bool {
    let lower = note.trim().to_ascii_lowercase();
    if lower.starts_with("consensus") {
        let seat = act_of(note)
            .map(|(_, seat)| fold_seat(&seat))
            .unwrap_or_else(|| "tracker".into());
        push(
            rows,
            window,
            stamped(
                stamp.clone(),
                seat,
                "consensus",
                consensus_text(id, note),
                id,
                false,
            ),
        );
        return true;
    }
    let Some((verb, seat)) = act_of(note) else {
        return false;
    };
    let seat = fold_seat(&seat);
    let verb = verb.to_ascii_lowercase();
    if verb.starts_with("filed") {
        if filed.insert(id.to_string()) {
            push(
                rows,
                window,
                stamped(
                    stamp.clone(),
                    seat,
                    "ticket",
                    format!("filed {id}"),
                    id,
                    false,
                ),
            );
        }
        return true;
    }
    if verb.contains("settled with notes") {
        push(
            rows,
            window,
            stamped(
                stamp.clone(),
                seat,
                "consensus",
                format!("settled with notes on {id}"),
                id,
                false,
            ),
        );
        return true;
    }
    if verb.contains("started a workflow") {
        push(
            rows,
            window,
            stamped(
                stamp.clone(),
                seat,
                "ticket",
                format!("started a workflow on {id}"),
                id,
                false,
            ),
        );
        return true;
    }
    if verb.starts_with("claimed") {
        let key = (id.to_string(), seat.clone(), "took");
        if claims.insert(key) {
            push(
                rows,
                window,
                stamped(
                    stamp.clone(),
                    seat,
                    "claim",
                    format!("took {id}"),
                    id,
                    false,
                ),
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
                stamped(
                    stamp.clone(),
                    seat,
                    "claim",
                    format!("released {id}"),
                    id,
                    false,
                ),
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

struct DrawerVote {
    stamp: Stamp,
    agent: String,
    choice: String,
}

fn vote_lines(heading: &vissue_core::IssueHeading) -> Vec<DrawerVote> {
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
            if let Some(mut vote) = parse_vote(line) {
                if vote.stamp.date_only {
                    if let Some(stamp) = ballot_note_stamp(heading, &vote) {
                        vote.stamp = stamp;
                    }
                }
                out.push(vote);
            }
        }
    }
    out
}

fn ballot_note_stamp(heading: &vissue_core::IssueHeading, vote: &DrawerVote) -> Option<Stamp> {
    heading.logbook.iter().find_map(|entry| {
        let sig = crate::ballot_sig::parse_note(entry.note.as_deref()?)?;
        if fold_seat(&sig.voter) != vote.agent || sig.choice != vote.choice {
            return None;
        }
        read_stamp(&entry.timestamp)
    })
}

fn parse_vote(line: &str) -> Option<DrawerVote> {
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
    let stamp = read_stamp(&format!("[{stamp}]"))?;
    Some(DrawerVote {
        stamp,
        agent: fold_seat(agent),
        choice: choice.to_string(),
    })
}

fn drop_shadowed_votes(rows: &mut Vec<Row>) {
    let timed_drawer: BTreeSet<(String, String)> = rows
        .iter()
        .filter(|row| row.kind == "vote" && row.minute && !row.date_only)
        .map(|row| (row.issue.clone(), row.seat.clone()))
        .collect();
    let git_vote: BTreeSet<(String, String)> = rows
        .iter()
        .filter(|row| row.kind == "vote" && row.drop_if_vote)
        .map(|row| (row.issue.clone(), row.seat.clone()))
        .collect();
    rows.retain(|row| {
        let key = (row.issue.clone(), row.seat.clone());
        if row.drop_if_vote && timed_drawer.contains(&key) {
            return false;
        }
        if row.kind == "vote" && !row.drop_if_vote && row.date_only && git_vote.contains(&key) {
            return false;
        }
        true
    });
}

fn collect_deeds(accessions: &[String], window: &Window, rows: &mut Vec<Row>) -> String {
    if accessions.is_empty() || which::which("deedar").is_err() {
        return String::new();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
    let mut seen = BTreeSet::new();
    for (i, accession) in accessions.iter().enumerate() {
        if std::time::Instant::now() >= deadline {
            let left = accessions.len() - i;
            return format!("deeds: cutoff at 800 ms, {left} accessions left\n");
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
    String::new()
}

fn collect_memory(window: &Window, rows: &mut Vec<Row>) {
    let Ok(client) = crate::pack() else {
        return;
    };
    let Ok(atoms) = crate::atoms_lean_recent(&client, &client.workspace(), MEMORY_CAP) else {
        return;
    };
    push_memory(&atoms, window, rows);
}

fn push_memory(atoms: &[serde_json::Value], window: &Window, rows: &mut Vec<Row>) {
    let mut memories: Vec<(i64, crate::memories::Memory)> = atoms
        .iter()
        .filter_map(crate::memories::Memory::of)
        .filter_map(|memory| stamp_unix(&memory.at).map(|at| (at, memory)))
        .collect();
    memories.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    for (at, mem) in memories.into_iter().take(MEMORY_CAP) {
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
            )
            .from("memory")
            .with_identity(at, &mem.id),
        );
    }
}

fn collect_guard(window: &Window, rows: &mut Vec<Row>, commands: bool) {
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
        let body = redact_secrets(value["text"].as_str().unwrap_or("").trim());
        if body.is_empty() {
            continue;
        }
        let text = if !commands {
            if recorded == "rewrite" {
                "rewrote a command".to_string()
            } else {
                "refused a command".to_string()
            }
        } else if recorded == "rewrite" {
            format!("rewrote {body}")
        } else {
            format!("refused {body}")
        };
        push(rows, window, Row::new(at, seat, "guard", text, "", false));
    }
}

fn push(rows: &mut Vec<Row>, window: &Window, mut row: Row) {
    row.seat = fold_seat(&row.seat);
    if !window.keeps_row(&row) {
        return;
    }
    rows.push(row);
}

fn push_git(rows: &mut Vec<Row>, window: &Window, mut row: Row, new_commit: bool) {
    row.seat = fold_seat(&row.seat);
    if window.exclusive && window.seen.contains(&row.key()) {
        return;
    }
    if new_commit || window.keeps(row.at, row.minute) {
        rows.push(row);
    }
}

/// `seat@holder` is one seat. An address, which has a dot after `@`, stays.
fn fold_seat(seat: &str) -> String {
    let seat = seat.trim();
    if seat.is_empty() {
        return "seat".to_string();
    }
    if let Some((name, rest)) = seat.split_once('@') {
        if !name.is_empty() && !rest.is_empty() && !rest.contains('.') {
            return name.to_string();
        }
    }
    seat.to_string()
}

fn dedup(rows: &mut Vec<Row>) {
    let mut seen = BTreeSet::new();
    rows.retain(|row| seen.insert(row.key()));
    // A date-only or minute stamp and the git author time of the same
    // act land within a minute of each other. Keep the later instant only
    // when the rows came from different records of that event.
    rows.sort_by(|a, b| {
        a.seat
            .cmp(&b.seat)
            .then(a.kind.cmp(b.kind))
            .then(a.text.cmp(&b.text))
            .then(b.at.cmp(&a.at))
    });
    let mut kept: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows.drain(..) {
        if let Some(prev) = kept.last() {
            let same = prev.seat == row.seat && prev.kind == row.kind && prev.text == row.text;
            if same && prev.source != row.source && prev.at.saturating_sub(row.at) < 60 {
                continue;
            }
        }
        kept.push(row);
    }
    *rows = kept;
}

fn render_human(banner: Option<&str>, prelude: &str, cutoff: &str, rows: &[Row]) -> String {
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
    if !cutoff.is_empty() {
        out.push_str(cutoff);
        if !cutoff.ends_with('\n') {
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

fn render_json(since: Option<&str>, banner: Option<&str>, cutoff: &str, rows: &[Row]) -> String {
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
        "cutoff": if cutoff.is_empty() { serde_json::Value::Null } else { serde_json::json!(cutoff.trim()) },
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

#[derive(Clone)]
struct Stamp {
    at: i64,
    minute: bool,
    date_only: bool,
}

fn read_stamp(stamp: &str) -> Option<Stamp> {
    let at = stamp_unix(stamp)?;
    let s = stamp
        .trim()
        .trim_start_matches(['[', '<'])
        .trim_end_matches([']', '>']);
    let date_only = !has_clock(s);
    Some(Stamp {
        at,
        minute: !date_only && !has_seconds(s),
        date_only,
    })
}

fn has_clock(stamp: &str) -> bool {
    stamp.as_bytes().windows(5).any(|w| {
        w[0].is_ascii_digit()
            && w[1].is_ascii_digit()
            && w[2] == b':'
            && w[3].is_ascii_digit()
            && w[4].is_ascii_digit()
    })
}

fn has_seconds(stamp: &str) -> bool {
    stamp.as_bytes().windows(8).any(|w| {
        w[0].is_ascii_digit()
            && w[1].is_ascii_digit()
            && w[2] == b':'
            && w[3].is_ascii_digit()
            && w[4].is_ascii_digit()
            && w[5] == b':'
            && w[6].is_ascii_digit()
            && w[7].is_ascii_digit()
    })
}

fn stamped(
    stamp: Stamp,
    seat: String,
    kind: &'static str,
    text: String,
    issue: impl Into<String>,
    drop_if_vote: bool,
) -> Row {
    let mut row = Row::new(stamp.at, seat, kind, text, issue, drop_if_vote);
    row.minute = stamp.minute;
    row.date_only = stamp.date_only;
    row
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

fn marker_state_path() -> PathBuf {
    crate::note_queue::state_dir().join("digest-last-state.json")
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct MarkerState {
    head: Option<String>,
    seen: Vec<String>,
}

fn marker_state() -> MarkerState {
    std::fs::read_to_string(marker_state_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn read_marker() -> Option<i64> {
    std::fs::read_to_string(marker_path())
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

fn store_marker(at: i64, root: &Path, rows: &[Row]) -> Result<()> {
    let path = marker_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{at}\n"))?;
    let mut state = marker_state();
    let mut held: BTreeSet<String> = state.seen.iter().cloned().collect();
    for row in rows {
        let key = row.key();
        if held.insert(key.clone()) {
            state.seen.push(key);
        }
    }
    const SEEN_CAP: usize = 4096;
    if state.seen.len() > SEEN_CAP {
        state.seen.drain(..state.seen.len() - SEEN_CAP);
    }
    state.head = crate::git_ok_stdout(root, &["rev-parse", "HEAD"])
        .map(|head| head.trim().to_string())
        .filter(|head| !head.is_empty());
    std::fs::write(marker_state_path(), serde_json::to_vec(&state)?)?;
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
    let text = redact_secrets(&one_line(text));
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
    let _ = crate::append_locked_row_capped(
        &path,
        line.as_bytes(),
        crate::Durability::BestEffort,
        GUARD_CAP,
    );
}

/// Tokens and secret assignments are replaced before a row is stored.
fn redact_secrets(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut ranges = Vec::new();
    for (i, _) in text.char_indices() {
        let rest = &lower[i..];
        if identifier_boundary(&lower, i) {
            for prefix in ["github_pat_", "ghp_", "gho_", "ghs_", "xoxb-"] {
                if rest.starts_with(prefix) {
                    let end = i + secret_token_len(&text[i..]);
                    ranges.push((i, end));
                }
            }
            if let Some((value, end)) = assigned_secret(text, &lower, i) {
                ranges.push((value, end));
            }
        }
    }
    for flag in ["--password", "--user", "-u"] {
        flag_secret_ranges(text, &lower, flag, &mut ranges);
    }
    url_secret_ranges(text, &lower, &mut ranges);
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if start >= end {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end) in merged {
        out.push_str(&text[at..start]);
        out.push_str("[redacted]");
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

fn identifier_boundary(text: &str, at: usize) -> bool {
    at == 0
        || !text.as_bytes()[at - 1].is_ascii_alphanumeric()
            && !matches!(text.as_bytes()[at - 1], b'_' | b'-')
}

fn assigned_secret(text: &str, lower: &str, at: usize) -> Option<(usize, usize)> {
    let bytes = lower.as_bytes();
    let mut name_end = at;
    while bytes
        .get(name_end)
        .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(*b, b'_' | b'-'))
    {
        name_end += 1;
    }
    let name = &lower[at..name_end];
    let secret_name = name == "authorization"
        || name.contains("password")
        || name == "token"
        || name.ends_with("_token")
        || name.ends_with("-token")
        || name == "secret"
        || name.ends_with("_secret")
        || name.ends_with("-secret")
        || name == "api_key"
        || name == "api-key"
        || name.ends_with("_key")
        || name.ends_with("-key");
    if !secret_name {
        return None;
    }
    let mut value = name_end;
    while bytes.get(value).is_some_and(|b| matches!(*b, b' ' | b'\t')) {
        value += 1;
    }
    if !bytes.get(value).is_some_and(|b| matches!(*b, b'=' | b':')) {
        return None;
    }
    value += 1;
    while bytes.get(value).is_some_and(|b| matches!(*b, b' ' | b'\t')) {
        value += 1;
    }
    if name == "authorization" && lower[value..].starts_with("bearer") {
        let after = value + "bearer".len();
        if bytes.get(after).is_some_and(|b| matches!(*b, b' ' | b'\t')) {
            value = after;
            while bytes.get(value).is_some_and(|b| matches!(*b, b' ' | b'\t')) {
                value += 1;
            }
        }
    }
    let end = value + secret_token_len(&text[value..]);
    (end > value).then_some((value, end))
}

fn flag_secret_ranges(text: &str, lower: &str, flag: &str, ranges: &mut Vec<(usize, usize)>) {
    let mut from = 0;
    while let Some(offset) = lower[from..].find(flag) {
        let at = from + offset;
        let after = at + flag.len();
        let before_ok = at == 0
            || lower.as_bytes()[at - 1].is_ascii_whitespace()
            || matches!(lower.as_bytes()[at - 1], b'\'' | b'"');
        let after_ok = lower
            .as_bytes()
            .get(after)
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'=');
        if !before_ok || !after_ok {
            from = after;
            continue;
        }
        let mut value = after;
        while lower
            .as_bytes()
            .get(value)
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'=')
        {
            value += 1;
        }
        if lower
            .as_bytes()
            .get(value)
            .is_some_and(|b| matches!(*b, b'\'' | b'"'))
        {
            value += 1;
        }
        let end = value + secret_token_len(&text[value..]);
        if end > value {
            let start = if flag == "-u" || flag == "--user" {
                text[value..end]
                    .find(':')
                    .map_or(value, |colon| value + colon + 1)
            } else {
                value
            };
            ranges.push((start, end));
        }
        from = after;
    }
}

fn url_secret_ranges(text: &str, lower: &str, ranges: &mut Vec<(usize, usize)>) {
    let mut from = 0;
    while let Some(offset) = lower[from..].find("://") {
        let authority = from + offset + 3;
        let end = text[authority..]
            .char_indices()
            .find_map(|(i, c)| {
                (c.is_whitespace() || matches!(c, '/' | '\'' | '"')).then_some(authority + i)
            })
            .unwrap_or(text.len());
        if let Some(user_end) = text[authority..end].find('@').map(|i| authority + i) {
            if let Some(password) = text[authority..user_end]
                .find(':')
                .map(|i| authority + i + 1)
            {
                if password < user_end {
                    ranges.push((password, user_end));
                }
            }
        }
        from = authority;
    }
}

fn secret_token_len(text: &str) -> usize {
    text.chars()
        .take_while(|c| !c.is_whitespace() && !matches!(*c, '\'' | '"' | '/' | '@'))
        .map(char::len_utf8)
        .sum()
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
    fn guard_rotation_stays_inside_the_append_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(dir.path().join("guard.jsonl"));
        let mut writers = Vec::new();
        for writer in 0..8 {
            let path = path.clone();
            writers.push(std::thread::spawn(move || {
                for row in 0..50 {
                    let line = format!("{{\"writer\":{writer},\"row\":{row}}}\n");
                    crate::append_locked_row_capped(
                        &path,
                        line.as_bytes(),
                        crate::Durability::Durable,
                        512,
                    )
                    .unwrap();
                }
            }));
        }
        for writer in writers {
            writer.join().unwrap();
        }
        let text = std::fs::read_to_string(&*path).unwrap();
        assert!(text.len() <= 512, "guard log is {} bytes", text.len());
        assert!(
            text.lines()
                .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok()),
            "{text}"
        );
        crate::append_locked_row_capped(
            &path,
            b"{\"sentinel\":true}\n",
            crate::Durability::Durable,
            512,
        )
        .unwrap();
        let text = std::fs::read_to_string(&*path).unwrap();
        assert!(text.contains("\"sentinel\":true"), "{text}");
    }

    #[test]
    fn digest_answers_a_thousand_tickets_within_two_seconds() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = tracker(dir.path());
        let home = dir.path().join("home");
        let mut org = String::from("#+TITLE: bench\n#+TODO: TODO | DONE\n\n");
        for i in 0..1100 {
            if i == 0 {
                org.push_str(
                    "* TODO [#C] Ticket 0\n:PROPERTIES:\n:ID:         bench-0000\n:DEEDS:      deed-a deed-b deed-c deed-d\n:END:\n\n",
                );
            } else {
                org.push_str(&format!(
                    "* TODO [#C] Ticket {i}\n:PROPERTIES:\n:ID:         bench-{i:04}\n:END:\n\n"
                ));
            }
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
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let stub = bin.join("deedar");
        std::fs::write(
            &stub,
            "#!/bin/sh\nsleep 0.45\nprintf 'time=1760000000\\nproducedBy=ada\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (url, stored) = crate::serve_sync_pack(crate::SyncPack::Keep);
        for i in 0..40 {
            stored.lock().unwrap().push(serde_json::json!({
                "id": format!("mem{i}"),
                "kind": "fact",
                "text": format!("bench memory {i}"),
                "ts": format!("2026-10-28T18:{i:02}:00Z"),
                "origin": "user-declared",
                "source": {"seat": "ada", "at": format!("2026-10-28T18:{i:02}:00Z")}
            }));
        }
        let path = format!("{}:/usr/bin:/bin", bin.display());
        isolate(&root, &home, &path);
        unsafe { std::env::set_var("PACKSET_URL", &url) };
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
        let memories = text.lines().filter(|line| line.contains("memory:")).count();
        assert!(memories <= 32, "memory rows {memories}\n{text}");
        assert!(memories >= 1, "{text}");
        assert!(text.contains("memory: bench memory 39"), "{text}");
        assert!(!text.contains("memory: bench memory 0"), "{text}");
        assert!(text.contains("deeds: cutoff at 800 ms"), "{text}");
        assert!(text.contains("accessions left"), "{text}");
    }

    fn review_filed_tracker(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = tracker(dir);
        let home = dir.join("home");
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
        (root, home)
    }

    #[test]
    fn review_repro_a_plain_or_json_digest_moves_the_last_marker() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let _ = review_filed_tracker(dir.path());
        let _ = digest(None, true).unwrap();
        let text = digest(Some("last"), false).unwrap();
        assert!(text.contains("filed probe-b"), "{text}");
    }

    #[test]
    fn review_repro_a_rebased_commit_comes_back_in_last() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, _) = review_filed_tracker(dir.path());
        let first = digest(Some("last"), false).unwrap();
        assert!(first.contains("filed probe-b"), "{first}");
        let later = chrono::Utc::now() + chrono::Duration::seconds(5);
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["commit", "-q", "--amend", "--no-edit"])
            .env("GIT_COMMITTER_DATE", later.to_rfc3339())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let second = digest(Some("last"), false).unwrap();
        assert!(!second.contains("filed probe-b"), "{second}");
    }

    #[test]
    fn review_repro_last_drops_an_org_event_in_the_marker_minute() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = review_filed_tracker(dir.path());
        let now = chrono::Local::now();
        let stamp = now.format("[%Y-%m-%d %a %H:%M]").to_string();
        let org = format!(
            "* DONE [#C] One\n:PROPERTIES:\n:ID:         probe-b\n:END:\n:LOGBOOK:\n- State \"DONE\" from \"TODO\" {stamp}\n- Finished by alice {stamp}\n:END:\n"
        );
        std::fs::write(root.join("Software/probe/issues.org"), org).unwrap();
        let minute = stamp_unix(&stamp).unwrap();
        std::fs::create_dir_all(home.join("state/ljos")).unwrap();
        std::fs::write(
            home.join("state/ljos/digest-last"),
            format!("{}\n", minute + 30),
        )
        .unwrap();
        let text = digest(Some("last"), false).unwrap();
        assert!(text.contains("closed probe-b"), "{text}");
    }

    #[test]
    fn review_repro_history_keeps_git_time_one_seat_and_the_filer() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = tracker(dir.path());
        let home = dir.path().join("home");
        let org = "\
* TODO [#C] Side
:PROPERTIES:
:ID:         side-b
:CREATED:    [2020-06-01 Mon]
:END:

* STARTED [#C] Held
:PROPERTIES:
:ID:         held-c
:CREATED:    [2020-06-01 Mon]
:CLAIMED_BY: bounty-driftwatch@1id5b6d0rr
:CLAIMED_AT: [2020-06-02 Tue 09:33]
:END:

* DONE [#C] Settled
:PROPERTIES:
:ID:         w1sl
:CREATED:    [2020-06-01 Mon]
:END:
:LOGBOOK:
- Note: \"Settled with notes by bounty-driftwatch\" [2020-06-03 Wed 10:09]
- State \"DONE\" from \"TODO\" [2020-06-03 Wed 10:09]
:END:
:VOTES:
[2020-06-04 Thu] opus: REVISE used=none
:END:

* TODO [#C] Signed vote
:PROPERTIES:
:ID:         signed-vote
:CREATED:    [2020-06-01 Mon]
:END:
:LOGBOOK:
- Note: \"ballot-sig: grok REVISE key sig\" [2020-06-04 Thu 10:09]
:END:
:VOTES:
[2020-06-04 Thu] grok: REVISE used=none
:END:

* TODO [#C] Flow
:PROPERTIES:
:ID:         flow-d
:CREATED:    [2020-06-01 Mon]
:END:
:LOGBOOK:
- Note: \"Started a workflow by bounty-driftwatch\" [2020-06-05 Fri 08:36]
:END:
";
        std::fs::write(root.join("Software/probe/issues.org"), org).unwrap();
        git(&root, &["add", "."], "2020-06-01T11:00:00Z");
        git(
            &root,
            &[
                "commit",
                "-q",
                "-m",
                "chore(issues): other noted by bounty-driftwatch",
            ],
            "2020-06-01T11:00:00Z",
        );
        for (date, message) in [
            (
                "2020-06-02T09:33:10Z",
                "chore(issues): held-c claimed by bounty-driftwatch",
            ),
            (
                "2020-06-03T10:09:12Z",
                "chore(issues): w1sl settled with notes by bounty-driftwatch",
            ),
            (
                "2020-06-04T11:25:27Z",
                "chore(issues): w1sl ballot withdrawn by opus",
            ),
            (
                "2020-06-04T11:25:34Z",
                "chore(issues): w1sl ballot cast by opus",
            ),
            (
                "2020-06-05T08:36:43Z",
                "chore(issues): flow-d started a workflow by bounty-driftwatch",
            ),
        ] {
            git(
                &root,
                &["commit", "-q", "--allow-empty", "-m", message],
                date,
            );
        }
        isolate(&root, &home, "/usr/bin:/bin");
        unsafe { std::env::set_var("PACKSET_URL", "off") };
        let text = digest(None, false).unwrap();
        let bounty = seat_block(&text, "bounty-driftwatch");
        assert!(bounty.contains("filed side-b"), "{text}");
        assert!(!text.contains("\ntracker\n"), "{text}");
        assert_eq!(
            text.lines()
                .filter(|line| line.contains("took held-c"))
                .count(),
            1,
            "{text}"
        );
        assert!(bounty.contains("settled with notes on w1sl"), "{text}");
        assert!(bounty.contains("started a workflow on flow-d"), "{text}");
        let opus = seat_block(&text, "opus");
        let voted = opus.find("voted w1sl").expect(&text);
        let withdrew = opus.find("withdrew a ballot on w1sl").expect(&text);
        assert!(voted < withdrew, "{opus}");
        assert!(!opus.contains("voted w1sl for REVISE"), "{opus}");
        let clock = wall(
            chrono::DateTime::parse_from_rfc3339("2020-06-04T11:25:34Z")
                .unwrap()
                .timestamp(),
        );
        assert!(opus.contains(&clock), "{opus}");
        let grok = seat_block(&text, "grok");
        assert!(grok.contains("voted signed-vote for REVISE"), "{grok}");
        let signed_clock = wall(stamp_unix("[2020-06-04 Thu 10:09]").unwrap());
        assert!(grok.contains(&signed_clock), "{grok}");
        assert!(!grok.contains("00:00"), "{grok}");
    }

    #[test]
    fn review_repro_guard_redacts_tokens_rotates_and_hides_the_command() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = tracker(dir.path());
        let home = dir.path().join("home");
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
        unsafe {
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("LJOS_SEAT", "erin");
        }
        note_guard(
            "refusal",
            "erin",
            "curl -H 'Authorization: Bearer xyz' https://example.invalid",
        );
        note_guard(
            "refusal",
            "erin",
            "curl https://example.invalid token=abc ghp_secret123",
        );
        let stored = std::fs::read_to_string(home.join("data/ljos/guard.jsonl")).unwrap();
        assert!(!stored.contains("xyz"), "{stored}");
        assert!(!stored.contains("ghp_secret123"), "{stored}");
        assert!(!stored.contains("token=abc"), "{stored}");
        let shown = digest(None, true).unwrap();
        assert!(!shown.contains("xyz"), "{shown}");
        assert!(!shown.contains("ghp_secret123"), "{shown}");
        assert!(shown.contains("curl"), "{shown}");
        let tool = digest_for_tool(None, true).unwrap();
        assert!(!tool.contains("curl"), "{tool}");
        assert!(!tool.contains("xyz"), "{tool}");
        assert!(tool.contains("refused a command"), "{tool}");
        let filler = "x".repeat(200);
        for i in 0..400 {
            note_guard("refusal", "erin", &format!("cmd {i} {filler}"));
        }
        let grown = std::fs::metadata(home.join("data/ljos/guard.jsonl"))
            .unwrap()
            .len();
        assert!(grown <= 64 * 1024, "guard.jsonl grew to {grown}");
    }

    #[test]
    fn review_repro_the_tool_does_not_move_the_marker() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (_, home) = review_filed_tracker(dir.path());
        let marker = home.join("state/ljos/digest-last");
        let _ = digest_for_tool(None, false).unwrap();
        let _ = digest_for_tool(Some("last"), true).unwrap();
        assert!(!marker.exists(), "the tool wrote {}", marker.display());
        let text = digest(Some("last"), false).unwrap();
        assert!(text.contains("filed probe-b"), "{text}");
    }

    #[test]
    fn review2_a_second_last_repeats_an_event_in_the_marker_minute() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = review_filed_tracker(dir.path());
        let now = chrono::Local::now();
        let stamp = now.format("[%Y-%m-%d %a %H:%M]").to_string();
        let org = format!(
            "* DONE [#C] One\n:PROPERTIES:\n:ID:         probe-b\n:END:\n:LOGBOOK:\n- State \"DONE\" from \"TODO\" {stamp}\n- Finished by alice {stamp}\n:END:\n"
        );
        std::fs::write(root.join("Software/probe/issues.org"), org).unwrap();
        let minute = stamp_unix(&stamp).unwrap();
        std::fs::create_dir_all(home.join("state/ljos")).unwrap();
        std::fs::write(
            home.join("state/ljos/digest-last"),
            format!("{}\n", minute + 1),
        )
        .unwrap();
        let first = digest(Some("last"), false).unwrap();
        assert!(first.contains("closed probe-b"), "{first}");
        std::fs::write(
            home.join("state/ljos/digest-last"),
            format!("{}\n", minute + 30),
        )
        .unwrap();
        let second = digest(Some("last"), false).unwrap();
        eprintln!("REVIEW2 second:\n{second}");
        assert!(!second.contains("closed probe-b"), "shown twice:\n{second}");
    }

    #[test]
    fn review2_a_commit_authored_before_the_marker_never_shows_in_last() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = review_filed_tracker(dir.path());
        let shown = digest(Some("last"), false).unwrap();
        assert!(shown.contains("filed probe-b"), "{shown}");
        let marker: i64 = std::fs::read_to_string(home.join("state/ljos/digest-last"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::fs::write(
            root.join("Software/probe/issues.org"),
            "* TODO [#C] One\n:PROPERTIES:\n:ID:         probe-b\n:END:\n* TODO [#C] Two\n:PROPERTIES:\n:ID:         probe-c\n:END:\n",
        )
        .unwrap();
        let authored = chrono::DateTime::from_timestamp(marker - 600, 0)
            .unwrap()
            .to_rfc3339();
        let landed = chrono::DateTime::from_timestamp(marker + 5, 0)
            .unwrap()
            .to_rfc3339();
        for args in [
            vec!["add", "."],
            vec![
                "commit",
                "-q",
                "-m",
                "chore(issues): probe-c filed by carol",
            ],
        ] {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(&args)
                .env("GIT_AUTHOR_DATE", &authored)
                .env("GIT_COMMITTER_DATE", &landed)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let next = digest(Some("last"), false).unwrap();
        eprintln!("REVIEW2 next:\n{next}");
        assert!(
            next.contains("probe-c"),
            "a commit that landed after the marker is missing:\n{next}"
        );
    }

    #[test]
    fn last_uses_reachability_when_a_late_commit_has_old_dates() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = review_filed_tracker(dir.path());
        let _ = digest(Some("last"), false).unwrap();
        let marker: i64 = std::fs::read_to_string(home.join("state/ljos/digest-last"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::fs::write(
            root.join("Software/probe/issues.org"),
            "* TODO [#C] One\n:PROPERTIES:\n:ID:         probe-b\n:END:\n* TODO [#C] Two\n:PROPERTIES:\n:ID:         probe-old\n:END:\n",
        )
        .unwrap();
        let old = chrono::DateTime::from_timestamp(marker - 600, 0)
            .unwrap()
            .to_rfc3339();
        git(&root, &["add", "."], &old);
        git(
            &root,
            &[
                "commit",
                "-q",
                "-m",
                "chore(issues): probe-old filed by carol",
            ],
            &old,
        );
        let next = digest(Some("last"), false).unwrap();
        assert!(next.contains("probe-old"), "{next}");
    }

    #[test]
    fn review2_redaction_misses_common_secret_shapes() {
        let cases = [
            "curl -u alice:hunter2 https://api.example.com",
            "git clone https://alice:hunter2@github.com/acme/x",
            "mysql --password hunter2 db",
            "export GH=github_pat_11ABCDEFG0123456789",
            "export GH=ghs_abcdefghijklmnop0123",
            "aws s3 ls --profile x AWS_SECRET_ACCESS_KEY=abc123def",
            "curl -H 'X-Api-Key: sk-live-abc123' https://x",
            "slack xoxb-1234-5678-abcdef",
        ];
        let mut leaked = Vec::new();
        for case in cases {
            let got = redact_secrets(case);
            eprintln!("REVIEW2 {case:?} -> {got:?}");
            for secret in [
                "hunter2",
                "github_pat_11",
                "ghs_abc",
                "abc123def",
                "sk-live",
                "xoxb-1234",
            ] {
                if case.contains(secret) && got.contains(secret) {
                    leaked.push(format!("{case} -> {got}"));
                }
            }
        }
        assert!(leaked.is_empty(), "{}", leaked.join("\n"));
        assert_eq!(
            redact_secrets("curl -H 'X-Api-Key: sk-live-abc123' https://x"),
            "curl -H 'X-Api-Key: [redacted]' https://x"
        );
    }

    #[test]
    fn identical_acts_are_kept_when_they_are_separate_events() {
        let mut rows = vec![
            Row::new(
                10,
                "alice".into(),
                "vote",
                "voted probe-a".into(),
                "probe-a",
                false,
            ),
            Row::new(
                20,
                "alice".into(),
                "vote",
                "voted probe-a".into(),
                "probe-a",
                false,
            ),
        ];
        dedup(&mut rows);
        assert_eq!(rows.len(), 2);
    }
}
