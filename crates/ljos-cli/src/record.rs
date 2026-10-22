//! Record schema v1: questions and ballots, signed as canonical JSON.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, FixedOffset};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The only accepted schema value for record entries.
pub const SCHEMA_V1: &str = "record/v1";
const SIGNING_LABEL: &str = "ljos-record-entry/v1";
const RESERVED_PLAIN_FIELDS: &[&str] = &["host", "session", "agent"];
const QUESTION_TYPES: &[&str] = &["correct", "locate", "change", "design", "lead"];
const BALLOT_STANCES: &[&str] = &["accept", "reject", "withdrawn"];

/// One signed entry in the plain record layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignedEntry {
    pub schema: String,
    #[serde(flatten)]
    pub entry: Entry,
    pub signature: EntrySignature,
}

impl SignedEntry {
    fn unsigned(&self) -> UnsignedEntry {
        UnsignedEntry {
            schema: self.schema.clone(),
            entry: self.entry.clone(),
        }
    }
}

/// A record entry before signing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnsignedEntry {
    pub schema: String,
    #[serde(flatten)]
    pub entry: Entry,
}

/// Signature material for one entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrySignature {
    pub key: String,
    pub sig: String,
}

/// Entry kinds in schema v1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Question(QuestionEntry),
    Ballot(BallotEntry),
}

/// A question entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub question_type: String,
    pub title: String,
    pub options: Vec<String>,
    pub anchors: Vec<String>,
    pub opened_by: String,
    pub opened_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dup_of: Option<String>,
}

/// A ballot entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BallotEntry {
    /// The canonical id: first 16 hex chars of sha256(canonical ballot JSON
    /// without this `id` field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub question: String,
    pub stance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    pub voter: String,
    pub commit: String,
    #[serde(default)]
    pub used: Vec<String>,
    pub lineage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<String>,
    pub cast_at: String,
}

/// Summary of `ljos record check`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckReport {
    pub files: usize,
    pub entries: usize,
    pub questions: usize,
    pub ballots: usize,
    pub questions_with_live_ballots: usize,
}

/// Sign one entry with `seat`'s ballot key.
///
/// The key is the file [`crate::ballot_sig::seat_key_path`] names, loaded
/// with [`crate::ballot_sig::load_or_create`], the same path a seat ballot
/// uses. Ballot entries get `id` filled when absent.
///
/// # Errors
///
/// The seat key cannot be loaded, or the entry does not pass schema
/// validation.
pub fn sign_entry(mut unsigned: UnsignedEntry, seat: &str) -> Result<SignedEntry> {
    if unsigned.schema.trim().is_empty() {
        unsigned.schema = SCHEMA_V1.to_string();
    }
    if let Entry::Ballot(ballot) = &mut unsigned.entry {
        if ballot.id.is_none() {
            ballot.id = Some(ballot_id(ballot)?);
        }
    }
    validate_unsigned(&unsigned)?;
    let (role, named) = match &unsigned.entry {
        Entry::Question(question) => ("opener", question.opened_by.as_str()),
        Entry::Ballot(ballot) => ("voter", ballot.voter.as_str()),
    };
    if seat.trim() != named {
        bail!("record signer `{seat}` must match entry {role} `{named}`");
    }
    let path = crate::ballot_sig::seat_key_path(seat)
        .context("no seat key path: HOME and XDG_DATA_HOME are unset")?;
    let key = crate::ballot_sig::load_or_create(&path)?;
    let payload = signing_payload(&unsigned)?;
    let (public, sig) = crate::ballot_sig::signed_hex(&key, &payload);
    Ok(SignedEntry {
        schema: unsigned.schema,
        entry: unsigned.entry,
        signature: EntrySignature { key: public, sig },
    })
}

/// Verify a signed entry and its schema rules.
///
/// # Errors
///
/// Schema validation failed, signature data was malformed, or signature
/// verification failed.
pub fn verify_entry(entry: &SignedEntry) -> Result<()> {
    let unsigned = entry.unsigned();
    validate_unsigned(&unsigned)?;
    let payload = signing_payload(&unsigned)?;
    if !crate::ballot_sig::is_hex_len(&entry.signature.key, 64) {
        bail!("signature key must be 64 hex chars");
    }
    if !crate::ballot_sig::is_hex_len(&entry.signature.sig, 128) {
        bail!("signature must be 128 hex chars");
    }
    if crate::ballot_sig::weak_key(&entry.signature.key) {
        bail!("signature key is a small-order point");
    }
    if !crate::ballot_sig::signature_ok(&entry.signature.key, &entry.signature.sig, &payload) {
        bail!("entry signature does not verify");
    }
    Ok(())
}

/// Run schema + signature checks over many entries, including voter-key pinning.
///
/// Every voter id is pinned, including a question's `opened_by` and a
/// seat. The tracker pin file wins when it already names that voter;
/// otherwise the first key seen in this check is the pin. A later entry
/// under another key is a mismatch. A run id is `<identity>/run-<id>`.
///
/// # Errors
///
/// Any entry fails validation or signature verification, a pin file voter
/// is not spelled the way the check looks it up, voter-key pinning detects
/// a mismatch, or two unreplaced ballots under one key share an hour.
pub fn check_entries(entries: &[SignedEntry]) -> Result<CheckReport> {
    let mut report = CheckReport {
        entries: entries.len(),
        ..CheckReport::default()
    };
    let (mut pins, pin_source) = crate::ballot_sig::pins_for_check()?;
    for voter in pins.keys() {
        validate_pin_voter(voter).with_context(|| format!("pin source: {pin_source}"))?;
    }
    let seat_pins = seat_pins(&pins).with_context(|| format!("pin source: {pin_source}"))?;
    for entry in entries {
        verify_entry(entry)?;
        match &entry.entry {
            Entry::Question(question) => {
                report.questions += 1;
                observe_seat_pin(&seat_pins, &question.opened_by, &entry.signature.key)
                    .with_context(|| format!("pin source: {pin_source}"))?;
                crate::ballot_sig::observe_pin(
                    &mut pins,
                    &question.opened_by,
                    &entry.signature.key,
                )
                .with_context(|| format!("pin source: {pin_source}"))?;
            }
            Entry::Ballot(ballot) => {
                report.ballots += 1;
                observe_seat_pin(&seat_pins, &ballot.voter, &entry.signature.key)
                    .with_context(|| format!("pin source: {pin_source}"))?;
                crate::ballot_sig::observe_pin(&mut pins, &ballot.voter, &entry.signature.key)
                    .with_context(|| format!("pin source: {pin_source}"))?;
            }
        }
    }
    report.questions_with_live_ballots = tally(entries)?.len();
    Ok(report)
}

/// Refuse a pin row the lookups would never match. Nothing is stripped:
/// a BOM or a zero-width space before `seat-a` would leave that seat's
/// pin unread. A row with `/` must be a voter id as written. A bare row
/// pins the seat [`voter_seat`] folds it to, so `inky@39u` pins `inky`
/// and `judge:model-1.5` pins `judge-model-1-5`.
fn validate_pin_voter(voter: &str) -> Result<()> {
    if !voter.bytes().all(|b| b.is_ascii_graphic()) {
        bail!("pin voter {voter:?} is not printable ASCII");
    }
    if voter.contains('/') {
        return validate_voter_id(voter).with_context(|| format!("pin voter `{voter}`"));
    }
    let seat = crate::seat_slug(voter);
    if valid_voter_part(voter) && seat != voter {
        bail!("pin voter `{voter}` is not a canonical seat; the seat is `{seat}`");
    }
    Ok(())
}

/// Pin rows by the seat each pins, as row to key. A bare row pins the
/// seat it folds to and a voter id row the seat after its `/`. A seat
/// holds one key: rows such as `inky`, `inky@39u` and `mallory/inky`
/// must pin the same key, or that seat would vote once under each.
fn seat_pins(
    pins: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
    let mut seats: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (row, key) in pins {
        let seat_part = split_voter(row).map_or(row.as_str(), |(_, seat)| seat);
        let Some(seat) = voter_seat(seat_part) else {
            continue;
        };
        let rows = seats.entry(seat.clone()).or_default();
        let fingerprint = crate::ballot_sig::key_fingerprint(key);
        if let Some((other, pinned)) = rows
            .iter()
            .find(|(_, pinned)| crate::ballot_sig::key_fingerprint(pinned) != fingerprint)
        {
            bail!(
                "seat {seat} is pinned to two keys, {}… ({other}) and {}… ({row}); a seat holds one key",
                &pinned[..12.min(pinned.len())],
                &key[..12.min(key.len())]
            );
        }
        rows.insert(row.clone(), key.clone());
    }
    Ok(seats)
}

/// Hold `voter` to its seat's pinned key. The voter id is already
/// canonical, so the part after `/` is the seat as [`seat_pins`] keys it.
fn observe_seat_pin(
    seat_pins: &BTreeMap<String, BTreeMap<String, String>>,
    voter: &str,
    key: &str,
) -> Result<()> {
    let Some((_, seat)) = split_voter(voter) else {
        return Ok(());
    };
    let Some(rows) = seat_pins.get(seat) else {
        return Ok(());
    };
    let fingerprint = crate::ballot_sig::key_fingerprint(key);
    if rows
        .values()
        .any(|pinned| crate::ballot_sig::key_fingerprint(pinned) == fingerprint)
    {
        return Ok(());
    }
    let pinned: Vec<String> = rows
        .iter()
        .map(|(row, pinned)| format!("{}… ({row})", &pinned[..12.min(pinned.len())]))
        .collect();
    bail!(
        "{voter} is pinned through seat {seat} to key {}, got {}…",
        pinned.join(" or "),
        &key[..12.min(key.len())]
    );
}

/// Validate every `.json` entry under `dir`.
///
/// # Errors
///
/// The directory cannot be read, no JSON entries are found, or any entry is
/// invalid.
pub fn check_dir(dir: &Path) -> Result<CheckReport> {
    let paths = entry_files(dir)?;
    if paths.is_empty() {
        bail!("record check: no .json entries under {}", dir.display());
    }
    let mut entries = Vec::with_capacity(paths.len());
    for path in &paths {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("record check: read {}", path.display()))?;
        let entry = parse_signed_json(&raw)
            .with_context(|| format!("record check: parse {}", path.display()))?;
        entries.push(entry);
    }
    let mut report = check_entries(&entries)?;
    report.files = paths.len();
    Ok(report)
}

/// Text form of a successful check.
#[must_use]
pub fn format_check_report(report: &CheckReport) -> String {
    format!(
        "record check: {} files, {} entries ({} questions, {} ballots), {} questions with live ballots\n",
        report.files,
        report.entries,
        report.questions,
        report.ballots,
        report.questions_with_live_ballots
    )
}

/// A tracker ballot that does not stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadBallot {
    /// The file that holds the ballot, under the tracker root.
    pub file: PathBuf,
    /// The ballot's line in that file, from 1.
    pub line: usize,
    pub issue: String,
    pub reason: String,
}

/// Summary of `ljos record check --tracker`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackerReport {
    /// Issues that hold a ballot.
    pub issues: usize,
    pub ballots: usize,
    /// `ISSUE VOTER` for each unsigned ballot committed before signing began.
    pub legacy: Vec<String>,
    pub bad: Vec<BadBallot>,
}

/// Hold every ballot on the tracker at `root` to the push gate's
/// signature check ([`crate::ballot_sig::seat_check`]): the newest
/// `ballot-sig:` note of its voter names its choice, verifies, and is
/// under the key `.ljos-seat-keys` pins for that voter and for no other
/// seat or voter on the issue, so one key does not count as several
/// voters. A persona's or a judge's ballot is held to it too, since every
/// cast signs. An unsigned ballot the tracker committed before signing
/// began is legacy, as at the push gate. Only files are read, and git for
/// the commit dates.
///
/// # Errors
///
/// The root holds no project, an issue file cannot be read or parsed, or
/// the pin file is malformed.
pub fn check_tracker(root: &Path) -> Result<TrackerReport> {
    let layout = vissue_core::Layout::resolve(Some(root), None)?;
    let projects = vissue_core::store::list_projects(&layout)?;
    if projects.is_empty() {
        bail!(
            "record check: no issues.org under {}",
            layout.projects_dir().display()
        );
    }
    let (pins, _) = crate::ballot_sig::pins_for_check_in(root)?;
    let mut report = TrackerReport::default();
    let mut texts: BTreeMap<PathBuf, String> = BTreeMap::new();
    for project in projects {
        let board = layout.project_issues_path(&project);
        let doc = vissue_core::IssueDoc::parse_file(&project, &board)?;
        for heading in &doc.headings {
            if !heading.extra_drawers.iter().any(|d| is_votes_drawer(d)) {
                continue;
            }
            let issue = heading.id.as_str();
            let ballots = vissue_core::ops::ballots(&layout, issue)?;
            if ballots.is_empty() {
                continue;
            }
            report.issues += 1;
            let notes: Vec<String> = heading
                .logbook
                .iter()
                .filter_map(|e| e.note.clone())
                .collect();
            let file = doc.heading_path(issue).to_path_buf();
            if !texts.contains_key(&file) {
                let text = std::fs::read_to_string(&file)
                    .with_context(|| format!("record check: read {}", file.display()))?;
                texts.insert(file.clone(), text);
            }
            let span = (file == board).then_some((heading.line_start, heading.line_end));
            let legacy = |voter: &str, choice: &str| {
                crate::ballot_sig::recorded_at_in(root, issue, voter, choice)
                    .is_some_and(|t| t < crate::ballot_sig::SIGNING_SINCE)
            };
            let cast: Vec<&str> = ballots.iter().map(|b| b.agent.as_str()).collect();
            for ballot in &ballots {
                report.ballots += 1;
                let seat = [(ballot.agent.as_str(), ballot.choice.as_str())];
                let (problems, old) =
                    crate::ballot_sig::seat_check(issue, &seat, &notes, &pins, &legacy);
                if !old.is_empty() {
                    report.legacy.push(format!("{issue} {}", ballot.agent));
                    continue;
                }
                let reason = problems
                    .into_iter()
                    .next()
                    .or_else(|| shared_key(&ballot.agent, &pins, &cast));
                let Some(reason) = reason else {
                    continue;
                };
                let line =
                    ballot_line(&texts[&file], span, &ballot.agent).unwrap_or(heading.line_start);
                report.bad.push(BadBallot {
                    file: file.strip_prefix(root).unwrap_or(&file).to_path_buf(),
                    line,
                    issue: issue.to_string(),
                    reason,
                });
            }
        }
    }
    Ok(report)
}

/// The other voters `pins` holds `voter`'s key for, as a reason. Keys are
/// told apart by [`crate::ballot_sig::key_fingerprint`]. Two rows that
/// [`voter_seat`] folds to one seat, such as `inky` and `inky@39u`, may
/// share a key, unless both are among `cast`, the voters on this issue.
fn shared_key(voter: &str, pins: &BTreeMap<String, String>, cast: &[&str]) -> Option<String> {
    let key = crate::ballot_sig::key_fingerprint(pins.get(voter)?);
    let seat = voter_seat(voter);
    let others: Vec<&str> = pins
        .iter()
        .filter(|(other, k)| {
            other.as_str() != voter
                && (voter_seat(other) != seat || cast.contains(&other.as_str()))
                && crate::ballot_sig::key_fingerprint(k) == key
        })
        .map(|(other, _)| other.as_str())
        .collect();
    (!others.is_empty()).then(|| {
        format!(
            "{voter}'s key is also pinned for {}, so one key counts as several voters",
            others.join(", ")
        )
    })
}

fn is_votes_drawer(drawer: &str) -> bool {
    drawer
        .lines()
        .next()
        .is_some_and(|l| l.trim().eq_ignore_ascii_case(":VOTES:"))
}

/// The line of `voter`'s ballot in a `:VOTES:` drawer of `text`, within
/// `span` when given. The last one, as a read keeps the last of two.
fn ballot_line(text: &str, span: Option<(usize, usize)>, voter: &str) -> Option<usize> {
    let (from, to) = span.unwrap_or((1, usize::MAX));
    let mut inside = false;
    let mut found = None;
    for (at, line) in text.lines().enumerate() {
        let at = at + 1;
        if at < from || at > to {
            continue;
        }
        let line = line.trim();
        if line.eq_ignore_ascii_case(":VOTES:") {
            inside = true;
        } else if line.eq_ignore_ascii_case(":END:") {
            inside = false;
        } else if inside && ballot_voter(line) == Some(voter) {
            found = Some(at);
        }
    }
    found
}

/// The voter of a `[STAMP] VOTER: CHOICE` drawer line.
fn ballot_voter(line: &str) -> Option<&str> {
    let (_, rest) = line.strip_prefix('[')?.split_once("] ")?;
    Some(rest.split_once(": ")?.0.trim())
}

/// Text form of a tracker check: one `FILE:LINE: ISSUE: REASON` line per
/// ballot that does not stand, then the count.
#[must_use]
pub fn format_tracker_report(report: &TrackerReport) -> String {
    let mut out = String::new();
    for bad in &report.bad {
        out.push_str(&format!(
            "{}:{}: {}: {}\n",
            bad.file.display(),
            bad.line,
            bad.issue,
            bad.reason
        ));
    }
    if report.bad.is_empty() {
        out.push_str(&format!(
            "record check: {} ballots on {} issues stand\n",
            report.ballots, report.issues
        ));
    } else {
        out.push_str(&format!(
            "record check: {} of {} ballots on {} issues do not stand\n",
            report.bad.len(),
            report.ballots,
            report.issues
        ));
    }
    if !report.legacy.is_empty() {
        out.push_str(&format!(
            "legacy unsigned, committed before signing began: {}\n",
            report.legacy.join(", ")
        ));
    }
    out
}

/// Canonical JSON bytes: recursive key sort, no insignificant whitespace.
///
/// # Errors
///
/// Serialization failed.
pub fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&canonical_value(value))?)
}

/// Ballot id from canonical ballot JSON without `id`.
///
/// # Errors
///
/// Serialization failed.
pub fn ballot_id(ballot: &BallotEntry) -> Result<String> {
    let mut value = serde_json::to_value(ballot)?;
    let Some(obj) = value.as_object_mut() else {
        bail!("ballot entry did not serialize as an object");
    };
    obj.remove("id");
    let hash = Sha256::digest(canonical_json(&value)?);
    let hex = hex::encode(hash);
    Ok(hex[..16].to_string())
}

/// Latest unreplaced ballot per voter and question.
///
/// Voters are counted per question, by signing key fingerprint. Copies
/// of one ballot under one key count as that ballot once. A ballot named
/// by another entry's `replaces` never counts. Of what remains, the
/// latest `cast_at` per key wins, and a latest `withdrawn` contributes no
/// vote. When one seat appears under a second key on the same question,
/// only the first key's ballot keeps counting. Two unreplaced ballots in
/// a voter's latest hour are ambiguous and that voter is dropped on that
/// question only; a tie in an earlier hour is superseded like any older
/// ballot.
///
/// # Errors
///
/// A `replaces` names no ballot, or one from another voter or question, or
/// a `cast_at` is not an hour timestamp.
pub fn tally(entries: &[SignedEntry]) -> Result<BTreeMap<String, BTreeMap<String, usize>>> {
    let mut copies = BTreeSet::new();
    let mut ballots: Vec<(&BallotEntry, String, Option<String>)> = Vec::new();
    for entry in entries {
        let Entry::Ballot(ballot) = &entry.entry else {
            continue;
        };
        let key = crate::ballot_sig::key_fingerprint(&entry.signature.key);
        // A second file with one ballot under one key is a copy, whatever
        // the hex case of its key and signature, not a second ballot.
        if !copies.insert((ballot_id(ballot)?, key.clone())) {
            continue;
        }
        let seat = split_voter(&ballot.voter).and_then(|(_, seat)| voter_seat(seat));
        ballots.push((ballot, key, seat));
    }
    let by_id: BTreeMap<&str, usize> = ballots
        .iter()
        .enumerate()
        .filter_map(|(at, (ballot, _, _))| ballot.id.as_deref().map(|id| (id, at)))
        .collect();
    let mut replaced = BTreeSet::new();
    for (ballot, key, _) in &ballots {
        let Some(target_id) = ballot.replaces.as_deref() else {
            continue;
        };
        let source_id = ballot.id.as_deref().unwrap_or("<missing-id>");
        let target_at = by_id.get(target_id).copied().ok_or_else(|| {
            anyhow!(
                "ballot {source_id} from {} on {} replaces {target_id}, but no ballot has that id",
                ballot.voter,
                ballot.question
            )
        })?;
        let (target, target_key, _) = &ballots[target_at];
        if target_key != key {
            bail!(
                "ballot {source_id} from {} on {} replaces {target_id} from {}; replaces must stay on the same voter",
                ballot.voter,
                ballot.question,
                target.voter
            );
        }
        if target.question != ballot.question {
            bail!(
                "ballot {source_id} from {} on {} replaces {target_id} on {}; replaces must stay on the same question",
                ballot.voter,
                ballot.question,
                target.question
            );
        }
        replaced.insert(target_id);
    }
    let mut grouped: BTreeMap<String, Vec<(usize, i64)>> = BTreeMap::new();
    for (at, (ballot, _, _)) in ballots.iter().enumerate() {
        if ballot.id.as_deref().is_some_and(|id| replaced.contains(id)) {
            continue;
        }
        let stamp = parse_hour_timestamp(&ballot.cast_at)
            .with_context(|| format!("ballot cast_at `{}`", ballot.cast_at))?
            .timestamp();
        grouped
            .entry(ballot.question.clone())
            .or_default()
            .push((at, stamp));
    }
    let mut counts: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (question, group) in grouped {
        let mut seat_keys: BTreeMap<String, String> = BTreeMap::new();
        let mut refused = BTreeSet::new();
        for (at, _) in &group {
            let Some(seat) = ballots[*at].2.as_deref() else {
                continue;
            };
            let key = ballots[*at].1.as_str();
            if let Some(held) = seat_keys.get(seat) {
                if held != key {
                    refused.insert(*at);
                }
            } else {
                seat_keys.insert(seat.to_string(), key.to_string());
            }
        }
        let mut by_key: BTreeMap<String, Vec<(i64, usize)>> = BTreeMap::new();
        for (at, stamp) in group {
            if refused.contains(&at) {
                continue;
            }
            by_key
                .entry(ballots[at].1.clone())
                .or_default()
                .push((stamp, at));
        }
        for (_, votes) in by_key {
            let Some(latest) = votes.iter().map(|(stamp, _)| *stamp).max() else {
                continue;
            };
            let mut current = votes.iter().filter(|(stamp, _)| *stamp == latest);
            let (Some((_, at)), None) = (current.next(), current.next()) else {
                continue;
            };
            let ballot = ballots[*at].0;
            if ballot.stance == "withdrawn" {
                continue;
            }
            *counts
                .entry(question.clone())
                .or_default()
                .entry(ballot.stance.clone())
                .or_default() += 1;
        }
    }
    Ok(counts)
}

fn validate_unsigned(unsigned: &UnsignedEntry) -> Result<()> {
    if unsigned.schema != SCHEMA_V1 {
        bail!(
            "record schema must be {SCHEMA_V1}, got {}",
            unsigned.schema.trim()
        );
    }
    match &unsigned.entry {
        Entry::Question(question) => validate_question(question),
        Entry::Ballot(ballot) => validate_ballot(ballot),
    }
}

fn validate_question(question: &QuestionEntry) -> Result<()> {
    validate_question_type(&question.question_type)?;
    validate_question_id(&question.id, Some(&question.question_type))?;
    if question.title.trim().is_empty() {
        bail!("question title is required");
    }
    if question.options.is_empty() {
        bail!("question options must not be empty");
    }
    if question.options.iter().any(|o| o.trim().is_empty()) {
        bail!("question options must not contain empty values");
    }
    if question.anchors.is_empty() {
        bail!("question anchors must not be empty");
    }
    for anchor in &question.anchors {
        validate_anchor(anchor)?;
    }
    validate_voter_id(&question.opened_by)?;
    parse_hour_timestamp(&question.opened_at)?;
    if let Some(dup_of) = &question.dup_of {
        validate_question_id(dup_of, None)?;
    }
    Ok(())
}

fn validate_ballot(ballot: &BallotEntry) -> Result<()> {
    let id = ballot.id.as_deref().context("ballot id is required")?;
    if !is_hex_len(id, 16) {
        bail!("ballot id must be 16 hex chars");
    }
    let computed = ballot_id(ballot)?;
    if id != computed {
        bail!("ballot id mismatch: expected {computed}, got {id}");
    }
    validate_question_id(&ballot.question, None)?;
    if !BALLOT_STANCES.contains(&ballot.stance.as_str()) {
        bail!("ballot stance must be one of `accept`, `reject` or `withdrawn`");
    }
    if let Some(confidence) = ballot.confidence {
        if !(confidence > 0.0 && confidence <= 1.0) {
            bail!("ballot confidence must be in (0, 1]");
        }
    }
    validate_voter_id(&ballot.voter)?;
    if !is_hex_len(&ballot.commit, 40) {
        bail!("ballot commit must be a full 40-hex SHA");
    }
    if ballot.used.is_empty() {
        bail!("ballot used must name one or more evidence refs, or `none`");
    }
    if ballot.used.iter().any(|u| u.trim().is_empty()) {
        bail!("ballot used must not contain empty entries");
    }
    if ballot.lineage.trim().is_empty() {
        bail!("ballot lineage is required");
    }
    if let Some(replaces) = &ballot.replaces {
        if !is_hex_len(replaces, 16) {
            bail!("ballot replaces must be a 16-hex ballot id");
        }
    }
    if let Some(expect) = &ballot.expect {
        if expect.trim().is_empty() {
            bail!("ballot expect must not be empty");
        }
    }
    parse_hour_timestamp(&ballot.cast_at)?;
    Ok(())
}

fn validate_question_type(kind: &str) -> Result<()> {
    if QUESTION_TYPES.contains(&kind) {
        Ok(())
    } else {
        bail!("unsupported question type `{kind}`")
    }
}

fn validate_question_id(id: &str, expected_type: Option<&str>) -> Result<()> {
    let (kind, suffix) = id
        .split_once('-')
        .context("question id must be `<type>-<8 hex>`")?;
    validate_question_type(kind)?;
    if suffix.len() != 8
        || !suffix
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        bail!("question id suffix must be 8 lowercase hex chars");
    }
    if let Some(expected_type) = expected_type {
        if expected_type != kind {
            bail!("question id type `{kind}` does not match field `{expected_type}`");
        }
    }
    Ok(())
}

fn validate_anchor(anchor: &str) -> Result<()> {
    let (kind, rest) = anchor
        .split_once(':')
        .context("anchor must be `<kind>:<value>`")?;
    if rest.trim().is_empty() {
        bail!("anchor `{anchor}` has an empty value");
    }
    match kind {
        "sha" => {
            let (repo, sha) = rest
                .rsplit_once('@')
                .context("sha anchor must be `sha:<repo>@<40-hex>`")?;
            if repo.trim().is_empty() || !is_hex_len(sha, 40) {
                bail!("sha anchor must be `sha:<repo>@<40-hex>`");
            }
        }
        "sym" => {
            if !rest.contains("::") {
                bail!("sym anchor must contain `::`");
            }
        }
        "ticket" | "name" => {}
        "pr" => {
            let (repo, num) = rest
                .rsplit_once('#')
                .context("pr anchor must be `pr:<repo>#<number>`")?;
            if repo.trim().is_empty() || num.parse::<u64>().is_err() {
                bail!("pr anchor must be `pr:<repo>#<number>`");
            }
        }
        _ => bail!("unsupported anchor kind `{kind}`"),
    }
    Ok(())
}

fn validate_voter_id(voter: &str) -> Result<()> {
    let (identity, seat_or_run) =
        split_voter(voter).ok_or_else(|| anyhow!("voter id must be `<identity>/<seat-or-run>`"))?;
    if !valid_voter_part(identity) || !valid_voter_part(seat_or_run) {
        bail!(
            "voter id must be `<identity>/<seat-or-run>` using ascii letters, digits, '-', '_' or '.'"
        );
    }
    let canonical = canonical_voter(identity, seat_or_run)
        .ok_or_else(|| anyhow!("voter id `{voter}` needs a letter or digit on each side of `/`"))?;
    // `run/<id>` names no identity. A run carries one: `<identity>/run-<id>`.
    if canonical.starts_with("run/") {
        bail!("voter id `{voter}` has no identity; a run is `<identity>/run-<id>`");
    }
    if canonical != voter {
        bail!("voter id `{voter}` must be written `{canonical}`; sign the entry under that id");
    }
    Ok(())
}

/// A voter id written one way: the identity as its slug and the rest as
/// [`voter_seat`]. `None` when a part has no letter or digit.
fn canonical_voter(identity: &str, seat_or_run: &str) -> Option<String> {
    Some(format!(
        "{}/{}",
        slug_of(identity)?,
        voter_seat(seat_or_run)?
    ))
}

/// The seat `name` sits as, read the way the tracker reads a holder: a
/// `@TAG`, an old pid suffix and an old seat name fold to the seat, so
/// `Inky`, `inky@39u` and `inky-n9o` are all `inky` and `ljos-bot` is
/// `ljosbot`. A run keeps its id and a judge its model, version and all:
/// `run-101` stays, and `judge:model-1.5` is `judge-model-1-5`. `None`
/// when the name has no letter or digit.
fn voter_seat(name: &str) -> Option<String> {
    let seat = slug_of(name.split_once('@').map_or(name, |(seat, _)| seat))?;
    if seat.starts_with("run-") || seat.starts_with("judge-") {
        return Some(seat);
    }
    Some(crate::seat_slug(&crate::holder_parts(&seat).0))
}

fn slug_of(part: &str) -> Option<String> {
    part.bytes()
        .any(|b| b.is_ascii_alphanumeric())
        .then(|| crate::seat_slug(part))
}

fn parse_hour_timestamp(text: &str) -> Result<DateTime<FixedOffset>> {
    // Local offsets and subseconds are distinct byte strings for one
    // instant, and each would mint its own ballot id. The plain layer
    // accepts a single UTC hour form.
    if !utc_hour_shape(text) {
        bail!("timestamp `{text}` must be YYYY-MM-DDTHH:00:00Z");
    }
    DateTime::parse_from_rfc3339(text)
        .with_context(|| format!("timestamp `{text}` must be YYYY-MM-DDTHH:00:00Z"))
}

fn utc_hour_shape(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && &b[14..16] == b"00"
        && &b[17..19] == b"00"
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..10].iter().all(u8::is_ascii_digit)
        && b[11..13].iter().all(u8::is_ascii_digit)
}

fn split_voter(voter: &str) -> Option<(&str, &str)> {
    let mut parts = voter.split('/');
    let identity = parts.next()?;
    let seat_or_run = parts.next()?;
    (parts.next().is_none() && !identity.is_empty() && !seat_or_run.is_empty())
        .then_some((identity, seat_or_run))
}

fn valid_voter_part(part: &str) -> bool {
    part.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn signing_payload(unsigned: &UnsignedEntry) -> Result<Vec<u8>> {
    let value = serde_json::to_value(unsigned)?;
    let canonical = canonical_json(&value)?;
    let mut payload = Vec::with_capacity(SIGNING_LABEL.len() + 1 + canonical.len());
    payload.extend_from_slice(SIGNING_LABEL.as_bytes());
    payload.push(b'\n');
    payload.extend_from_slice(&canonical);
    Ok(payload)
}

struct NoDuplicateKeys(Value);

impl<'de> Deserialize<'de> for NoDuplicateKeys {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ValueVisitor;

        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON value")
            }

            fn visit_bool<E>(self, value: bool) -> std::result::Result<Value, E> {
                Ok(Value::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Value, E> {
                Ok(Value::Number(value.into()))
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Value, E> {
                Ok(Value::Number(value.into()))
            }

            fn visit_f64<E>(self, value: f64) -> std::result::Result<Value, E>
            where
                E: de::Error,
            {
                let number = serde_json::Number::from_f64(value)
                    .ok_or_else(|| E::custom("JSON number is not finite"))?;
                Ok(Value::Number(number))
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Value, E> {
                Ok(Value::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> std::result::Result<Value, E> {
                Ok(Value::String(value))
            }

            fn visit_none<E>(self) -> std::result::Result<Value, E> {
                Ok(Value::Null)
            }

            fn visit_unit<E>(self) -> std::result::Result<Value, E> {
                Ok(Value::Null)
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(value) = seq.next_element::<NoDuplicateKeys>()? {
                    items.push(value.0);
                }
                Ok(Value::Array(items))
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut out = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if out.contains_key(&key) {
                        return Err(de::Error::custom(format!("duplicate key `{key}`")));
                    }
                    let value = map.next_value::<NoDuplicateKeys>()?;
                    out.insert(key, value.0);
                }
                Ok(Value::Object(out))
            }
        }

        deserializer.deserialize_any(ValueVisitor).map(Self)
    }
}

fn parse_json_without_duplicate_keys(raw: &str) -> Result<Value> {
    let mut de = serde_json::Deserializer::from_str(raw);
    let value = NoDuplicateKeys::deserialize(&mut de)
        .map(|value| value.0)
        .context("record entry is not JSON")?;
    de.end().context("record entry is not JSON")?;
    Ok(value)
}

/// Parse one record file. Unknown keys are not dropped before the reserved
/// name walk, and the canonical bytes without `signature` must be the typed
/// entry. A field added after signing then fails instead of riding along.
fn parse_signed_json(raw: &str) -> Result<SignedEntry> {
    let value = parse_json_without_duplicate_keys(raw)?;
    reject_reserved_plain_fields(&value)?;
    let entry: SignedEntry =
        serde_json::from_value(value.clone()).context("record entry does not match schema v1")?;
    require_signed_coverage(&value, &entry)?;
    Ok(entry)
}

fn require_signed_coverage(raw: &Value, entry: &SignedEntry) -> Result<()> {
    let Value::Object(map) = raw else {
        bail!("record entry must be a JSON object");
    };
    let mut unsigned = map.clone();
    unsigned.remove("signature");
    let raw_canon = canonical_json(&Value::Object(unsigned))?;
    let typed = serde_json::to_value(entry.unsigned())?;
    let typed_canon = canonical_json(&typed)?;
    if raw_canon != typed_canon {
        bail!("entry has a field the signature does not cover");
    }
    Ok(())
}

fn reject_reserved_plain_fields(value: &Value) -> Result<()> {
    reject_reserved_plain_fields_at(value, "$")
}

fn reject_reserved_plain_fields_at(value: &Value, path: &str) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if RESERVED_PLAIN_FIELDS.contains(&k.as_str()) {
                    bail!("plain layer field `{k}` is not allowed at {path}");
                }
                let next = format!("{path}.{k}");
                reject_reserved_plain_fields_at(v, &next)?;
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                let next = format!("{path}[{i}]");
                reject_reserved_plain_fields_at(item, &next)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for key in keys {
                out.insert(key.clone(), canonical_value(&map[&key]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        _ => value.clone(),
    }
}

fn is_hex_len(text: &str, len: usize) -> bool {
    crate::ballot_sig::is_hex_len(text, len)
}

fn entry_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(root) = pending.pop() {
        for row in std::fs::read_dir(&root)
            .with_context(|| format!("record check: read {}", root.display()))?
        {
            let row = row?;
            let path = row.path();
            let ty = row.file_type()?;
            if ty.is_dir() {
                pending.push(path);
            } else if ty.is_file()
                && path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hold a private seat-key directory for the rest of this test.
    fn test_home() -> (crate::EnvLock, tempfile::TempDir) {
        let lock = crate::test_env_lock();
        let home = tempfile::tempdir().expect("home");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe {
                std::env::set_var("XDG_DATA_HOME", home.path());
                std::env::remove_var("LJOS_SEAT_KEY");
                std::env::remove_var("LJOS_SEAT");
                std::env::remove_var("VISSUE_AGENT");
                std::env::remove_var("VISSUE_ROOT");
                std::env::remove_var("CURSOR_AGENT_ID");
            }
        });
        (lock, home)
    }

    fn sign_as(unsigned: UnsignedEntry, seed: u8) -> SignedEntry {
        let seat = signer_on_entry(&unsigned).to_string();
        let path = crate::ballot_sig::seat_key_path(&seat).expect("seat key path");
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("seat dir");
        }
        std::fs::write(&path, [seed; 32]).expect("seed");
        sign_entry(unsigned, &seat).unwrap_or_else(|err| panic!("seat-{seed}: {err:#}"))
    }

    /// Sign under the `[seed; 32]` key without the schema checks
    /// [`sign_entry`] runs first.
    fn sign_unchecked(mut unsigned: UnsignedEntry, seed: u8) -> SignedEntry {
        if let Entry::Ballot(ballot) = &mut unsigned.entry {
            ballot.id = Some(ballot_id(ballot).expect("id"));
        }
        let key = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        let payload = signing_payload(&unsigned).expect("payload");
        let (key, sig) = crate::ballot_sig::signed_hex(&key, &payload);
        SignedEntry {
            schema: unsigned.schema,
            entry: unsigned.entry,
            signature: EntrySignature { key, sig },
        }
    }

    fn signer_on_entry(unsigned: &UnsignedEntry) -> &str {
        match &unsigned.entry {
            Entry::Question(question) => &question.opened_by,
            Entry::Ballot(ballot) => &ballot.voter,
        }
    }

    fn question_entry() -> UnsignedEntry {
        UnsignedEntry {
            schema: SCHEMA_V1.to_string(),
            entry: Entry::Question(QuestionEntry {
                id: "correct-3f9a1c2e".to_string(),
                question_type: "correct".to_string(),
                title: "is this head mergeable".to_string(),
                options: vec!["accept".to_string(), "reject".to_string()],
                anchors: vec!["sha:owner/repo@0123456789abcdef0123456789abcdef01234567".to_string()],
                opened_by: "alpha/seat-a".to_string(),
                opened_at: "2026-10-21T09:00:00Z".to_string(),
                dup_of: None,
            }),
        }
    }

    fn ballot_entry(voter: &str, stance: &str, cast_at: &str) -> UnsignedEntry {
        UnsignedEntry {
            schema: SCHEMA_V1.to_string(),
            entry: Entry::Ballot(BallotEntry {
                id: None,
                question: "correct-3f9a1c2e".to_string(),
                stance: stance.to_string(),
                confidence: Some(0.9),
                voter: voter.to_string(),
                commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
                used: vec!["none".to_string()],
                lineage: "model:synthetic".to_string(),
                replaces: None,
                expect: None,
                cast_at: cast_at.to_string(),
            }),
        }
    }

    fn ballot_on_question(
        voter: &str,
        stance: &str,
        cast_at: &str,
        question: &str,
    ) -> UnsignedEntry {
        let mut entry = ballot_entry(voter, stance, cast_at);
        let Entry::Ballot(ballot) = &mut entry.entry else {
            panic!("ballot");
        };
        ballot.question = question.to_string();
        entry
    }

    fn as_ballot(entry: SignedEntry) -> BallotEntry {
        match entry.entry {
            Entry::Ballot(ballot) => ballot,
            Entry::Question(_) => panic!("expected a ballot entry"),
        }
    }

    #[test]
    fn a_signed_entry_verifies() {
        let _home = test_home();
        let entry = sign_as(question_entry(), 7);
        verify_entry(&entry).expect("valid signature");
    }

    #[test]
    fn a_record_entry_is_signed_with_the_seat_ballot_key() {
        let _home = test_home();
        let entry = sign_as(question_entry(), 7);
        let path = crate::ballot_sig::seat_key_path("alpha/seat-a").expect("path");
        let key = crate::ballot_sig::load_or_create(&path).expect("seat key");
        assert_eq!(
            entry.signature.key,
            hex::encode(key.verifying_key().to_bytes())
        );
        verify_entry(&entry).expect("verifies under the seat key");
    }

    #[test]
    fn each_voter_signs_with_its_own_key_file() {
        let _home = test_home();
        let at = "2026-10-21T10:00:00Z";
        let one = sign_entry(ballot_entry("a/b-c", "accept", at), "a/b-c").expect("a/b-c");
        let two = sign_entry(ballot_entry("a-b/c", "accept", at), "a-b/c").expect("a-b/c");
        assert_ne!(
            one.signature.key, two.signature.key,
            "a/b-c and a-b/c signed with one key file"
        );
        let names = ["a/b_c", "a_b/c", "a_b_c", "a:b_c", "a%2fb_c", "", ".", ".."];
        let files: BTreeSet<PathBuf> = names
            .iter()
            .map(|name| crate::ballot_sig::seat_key_file(name).expect("path"))
            .collect();
        assert_eq!(files.len(), names.len(), "{files:?}");
    }

    #[test]
    fn an_explicit_seat_key_is_used_as_given() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        let given = root.path().join("seat.key");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe {
                std::env::set_var("LJOS_SEAT_KEY", &given);
                std::env::set_var("CURSOR_AGENT_ID", "agent-run-1111");
            }
        });
        let path = crate::ballot_sig::seat_key_path("alpha/seat-a");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe {
                std::env::remove_var("CURSOR_AGENT_ID");
                std::env::remove_var("LJOS_SEAT_KEY");
            }
        });
        assert_eq!(
            path.as_deref(),
            Some(given.as_path()),
            "LJOS_SEAT_KEY was not used as given"
        );
    }

    #[test]
    fn sign_entry_refuses_a_signer_not_named_on_the_entry() {
        let _home = test_home();
        let err = sign_entry(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            "seat-8",
        )
        .expect_err("mismatched signer");
        assert!(
            err_chain(&err).contains("voter"),
            "mismatched signer passed: {}",
            err_chain(&err)
        );
    }

    #[test]
    fn sign_entry_refuses_a_voter_id_in_another_spelling() {
        let _home = test_home();
        let at = "2026-10-21T10:00:00Z";
        for (written, canonical) in [
            ("Alpha/seat-a", "alpha/seat-a"),
            ("alpha./seat-a", "alpha/seat-a"),
            ("alpha_/seat-a", "alpha/seat-a"),
            ("alpha/Seat-a", "alpha/seat-a"),
            ("Alpha/run-101", "alpha/run-101"),
            ("alpha./run-101", "alpha/run-101"),
            ("alpha_/run-101", "alpha/run-101"),
            ("alpha/Run-101", "alpha/run-101"),
            ("alpha/run-101.", "alpha/run-101"),
            ("alpha/run--101", "alpha/run-101"),
            ("alpha/Inky", "alpha/inky"),
            ("alpha/inky-39u", "alpha/inky"),
            ("alpha/inky-n9o", "alpha/inky"),
            ("alpha/disk-saver-t4t", "alpha/disk-saver"),
            ("alpha/ljos-bot", "alpha/ljosbot"),
            ("alpha/GPT-5", "alpha/gpt-5"),
            ("alpha/Ridley-CI-A-555001", "alpha/ridley-ci-a-555001"),
            ("alpha/judge-Model-1.5", "alpha/judge-model-1-5"),
        ] {
            let err = sign_entry(ballot_entry(written, "accept", at), written).expect_err(written);
            let text = err_chain(&err);
            assert!(
                text.contains(&format!("must be written `{canonical}`")),
                "{written}: {text}"
            );
            sign_entry(ballot_entry(canonical, "accept", at), canonical)
                .unwrap_or_else(|err| panic!("{canonical}: {err:#}"));
        }
        for (voter, said) in [
            ("Run/run-101", "no identity"),
            ("run./run-101", "no identity"),
            ("alpha/...", "letter or digit"),
            ("_/seat-a", "letter or digit"),
        ] {
            let err = sign_entry(ballot_entry(voter, "accept", at), voter).expect_err(voter);
            assert!(err_chain(&err).contains(said), "{voter}: {err:#}");
        }
    }

    #[test]
    fn a_version_suffix_stays_part_of_the_seat() {
        let _home = test_home();
        let at = "2026-10-21T10:00:00Z";
        let entries = ["alpha/gpt-5", "alpha/gpt"].map(|voter| {
            sign_entry(ballot_entry(voter, "accept", at), voter)
                .unwrap_or_else(|err| panic!("{voter}: {err:#}"))
        });
        check_entries(&entries).expect("two seats");
        let read = tally(&entries).expect("tally");
        assert_eq!(counted(&read, "accept"), Some(2), "{read:?}");
        let err = sign_entry(ballot_entry("alpha/gpt-39u", "accept", at), "alpha/gpt-39u")
            .expect_err("a pid tag folds");
        let text = err_chain(&err);
        assert!(text.contains("must be written `alpha/gpt`"), "{text}");
    }

    #[test]
    fn a_stored_ballot_in_another_spelling_names_its_voter_id() {
        let _home = test_home();
        // One seed stands in for the key file `Alpha/seat-a` and
        // `alpha/seat-a` share on a case-insensitive volume.
        for (written, canonical) in [
            ("Alpha/seat-a", "alpha/seat-a"),
            ("alpha./seat-a", "alpha/seat-a"),
            ("alpha_/run-101", "alpha/run-101"),
            ("alpha/Run-101", "alpha/run-101"),
            ("alpha/inky-39u", "alpha/inky"),
            ("alpha/ljos-bot", "alpha/ljosbot"),
        ] {
            let kept = sign_as(ballot_entry(canonical, "accept", "2026-10-21T10:00:00Z"), 1);
            let stored = sign_unchecked(ballot_entry(written, "accept", "2026-10-21T11:00:00Z"), 1);
            let entries = [kept, stored];
            let Err(err) = check_entries(&entries) else {
                panic!("{written} passed beside {canonical}: {:?}", tally(&entries));
            };
            let text = err_chain(&err);
            assert!(
                text.contains(&format!("`{written}` must be written `{canonical}`")),
                "{text}"
            );
        }
    }

    #[test]
    fn a_one_byte_edit_fails_verification() {
        let _home = test_home();
        let mut entry = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            8,
        );
        let Entry::Ballot(ballot) = &mut entry.entry else {
            panic!("ballot expected");
        };
        // `lineage` feeds the ballot id. Recompute it so the failure is the
        // signature, not the id check that used to hide this edit.
        ballot.lineage.push('x');
        ballot.id = Some(ballot_id(ballot).expect("id"));
        let err = verify_entry(&entry).expect_err("edited ballot");
        assert!(
            err.to_string().contains("does not verify"),
            "edit never reached signature verification: {err}"
        );
    }

    #[test]
    fn a_small_order_key_is_refused() {
        let _home = test_home();
        let key = format!("01{}", "00".repeat(31));
        let sig = format!("01{}", "00".repeat(63));
        for stance in ["accept", "reject"] {
            let mut unsigned = ballot_entry("alpha/seat-a", stance, "2026-10-21T10:00:00Z");
            let Entry::Ballot(ballot) = &mut unsigned.entry else {
                panic!("ballot");
            };
            ballot.id = Some(ballot_id(ballot).expect("id"));
            let forged = SignedEntry {
                schema: unsigned.schema,
                entry: unsigned.entry,
                signature: EntrySignature {
                    key: key.clone(),
                    sig: sig.clone(),
                },
            };
            let err = check_value(&serde_json::to_value(&forged).expect("value"))
                .expect_err("a small-order key passed record check");
            assert!(
                err_chain(&err).contains("small-order"),
                "{stance}: {}",
                err_chain(&err)
            );
        }
        assert!(
            !crate::ballot_sig::signature_ok(&key, &sig, b"any message"),
            "the identity key verified a signature"
        );
    }

    #[test]
    fn number_and_unicode_spellings_follow_the_typed_form() {
        let _home = test_home();
        let mut unsigned = ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z");
        let Entry::Ballot(ballot) = &mut unsigned.entry else {
            panic!("ballot");
        };
        ballot.confidence = Some(1.0);
        ballot.lineage = "café".to_string();
        let signed = sign_as(unsigned, 8);
        let id = as_ballot(signed.clone()).id.clone().expect("id");
        let text = serde_json::to_string(&signed).expect("json");
        let checked = |body: &str| {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::write(dir.path().join("entry.json"), body).expect("write");
            check_dir(dir.path())
        };
        let integer = checked(&text.replace("1.0", "1")).expect_err("integer 1");
        assert!(
            err_chain(&integer).contains("does not cover"),
            "{}",
            err_chain(&integer)
        );
        checked(&text.replace("1.0", "1e0")).expect("1e0");
        checked(&text.replace("1.0", "10E-1")).expect("10E-1");
        checked(&text.replace("café", "caf\\u00e9")).expect("unicode escape");
        let mut decomposed = ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z");
        let Entry::Ballot(ballot) = &mut decomposed.entry else {
            panic!("ballot");
        };
        ballot.confidence = Some(1.0);
        ballot.lineage = "cafe\u{0301}".to_string();
        let nfd = as_ballot(sign_as(decomposed, 8)).id.expect("nfd id");
        assert_ne!(id, nfd, "unicode normalisation changed the ballot id");
    }

    #[test]
    fn the_documented_ballot_id_matches_the_example() {
        let _home = test_home();
        let entry = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            8,
        );
        let id = as_ballot(entry).id.expect("id");
        let doc = include_str!("../doc/record-v1.md");
        assert!(
            doc.contains(&format!("\"id\": \"{id}\"")),
            "documented ballot id is stale; computed {id}"
        );
    }

    #[test]
    fn docs_name_first_seen_pin_scope() {
        let doc = include_str!("../doc/record-v1.md");
        let normalized = doc.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains(
                "none for its seat, the first key seen in this check is the pin for this check only"
            ),
            "record-v1.md must state that first-seen pins are check-local and yield to a seat pin"
        );
    }

    #[test]
    fn docs_name_the_deleted_withdrawal_limit() {
        let doc = include_str!("../doc/record-v1.md");
        let normalized = doc.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains("A deleted withdrawal revives the earlier ballot"),
            "record-v1.md must say the check cannot see a deleted withdrawal"
        );
    }

    #[test]
    fn docs_name_voter_pin_precedence_and_ambiguity_drop() {
        let doc = include_str!("../doc/record-v1.md");
        let normalized = doc.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains("a pin file row for a voter id also pins the seat after its `/`"),
            "record-v1.md must say a voter-id row pins its seat"
        );
        assert!(
            normalized.contains("are ambiguous and are dropped on that question only"),
            "record-v1.md must say same-hour ambiguity is local to one question"
        );
    }

    #[test]
    fn a_second_key_under_one_run_id_is_a_mismatch() {
        let _home = test_home();
        let first = sign_as(
            ballot_entry("alpha/run-101", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let second = sign_as(
            ballot_entry("alpha/run-101", "reject", "2026-10-21T11:00:00Z"),
            2,
        );
        let err = check_entries(&[first, second]).expect_err("must reject run key mismatch");
        let text = err_chain(&err);
        assert!(text.contains("pinned"), "{text}");
        assert!(text.contains("none: first-seen in this check"), "{text}");
    }

    #[test]
    fn a_seat_voter_is_pinned_to_one_key() {
        let _home = test_home();
        let first = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let second = sign_as(
            ballot_entry("alpha/seat-a", "reject", "2026-10-21T11:00:00Z"),
            2,
        );
        let err = check_entries(&[first, second]).expect_err("seat pin");
        assert!(err_chain(&err).contains("pinned"), "{}", err_chain(&err));
    }

    #[test]
    fn a_question_opener_is_pinned_with_the_voter() {
        let _home = test_home();
        let question = sign_as(question_entry(), 1);
        let ballot = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            2,
        );
        let err = check_entries(&[question, ballot]).expect_err("opener pin");
        assert!(err_chain(&err).contains("pinned"), "{}", err_chain(&err));
    }

    #[test]
    fn a_run_keeps_its_identity_and_one_key() {
        let _home = test_home();
        let err = sign_entry(
            ballot_entry("run/run-101", "accept", "2026-10-21T10:00:00Z"),
            "run/run-101",
        )
        .expect_err("bare run identity");
        assert!(err_chain(&err).contains("identity"), "{}", err_chain(&err));
        let first = sign_as(
            ballot_entry("alpha/run-101", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let second = sign_as(
            ballot_entry("alpha/run-101", "reject", "2026-10-21T11:00:00Z"),
            2,
        );
        let err = check_entries(&[first, second]).expect_err("run pin");
        assert!(err_chain(&err).contains("pinned"), "{}", err_chain(&err));
    }

    #[test]
    fn latest_ballot_per_voter_wins_and_withdrawn_does_not_count() {
        let _home = test_home();
        let a1 = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        );
        let a2 = sign_as(
            ballot_entry("alpha/seat-a", "withdrawn", "2026-10-21T11:00:00Z"),
            3,
        );
        let b1 = sign_as(
            ballot_entry("beta/seat-b", "accept", "2026-10-21T10:00:00Z"),
            4,
        );
        let read = tally(&[a1, a2, b1]).expect("tally");
        let by_stance = read.get("correct-3f9a1c2e").expect("question tally");
        assert_eq!(by_stance.get("accept"), Some(&1));
        assert_eq!(by_stance.get("withdrawn"), None);
    }

    #[test]
    fn a_question_id_in_capitals_is_refused() {
        let _home = test_home();
        let at = "2026-10-21T10:00:00Z";
        let lower = sign_as(ballot_entry("alpha/seat-a", "accept", at), 3);
        let mut upper = ballot_entry("alpha/seat-a", "accept", at);
        let Entry::Ballot(ballot) = &mut upper.entry else {
            panic!("ballot");
        };
        ballot.question = "correct-3F9A1C2E".to_string();
        let upper = sign_unchecked(upper, 3);
        let entries = [lower, upper];
        let Err(err) = check_entries(&entries) else {
            panic!("one voter counted twice: {:?}", tally(&entries));
        };
        assert!(err_chain(&err).contains("lowercase"), "{}", err_chain(&err));
    }

    #[test]
    fn two_unreplaced_ballots_in_one_hour_are_flagged() {
        let _home = test_home();
        let accept = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        );
        let reject = sign_as(
            ballot_entry("alpha/seat-a", "reject", "2026-10-21T10:00:00Z"),
            3,
        );
        let read = tally(&[accept, reject]).expect("same-hour ballots are dropped");
        assert!(read.is_empty(), "{read:?}");
    }

    #[test]
    fn a_later_ballot_stands_past_a_same_hour_pair() {
        let _home = test_home();
        let cast = |stance: &str, hour: u8| {
            sign_as(
                ballot_entry("alpha/seat-a", stance, &format!("2026-10-21T{hour}:00:00Z")),
                3,
            )
        };
        let entries = [cast("accept", 10), cast("reject", 10), cast("accept", 12)];
        let read = tally(&entries).expect("tally");
        assert_eq!(
            counted(&read, "accept"),
            Some(1),
            "a same-hour pair hid a later ballot: {read:?}"
        );
        assert_eq!(counted(&read, "reject"), None, "{read:?}");
        let entries = [cast("accept", 10), cast("accept", 12), cast("reject", 12)];
        let read = tally(&entries).expect("tally");
        assert!(read.is_empty(), "a latest same-hour pair counted: {read:?}");
    }

    fn counted(read: &BTreeMap<String, BTreeMap<String, usize>>, stance: &str) -> Option<usize> {
        read.get("correct-3f9a1c2e")
            .and_then(|stances| stances.get(stance))
            .copied()
    }

    #[test]
    fn a_replayed_ballot_does_not_erase_its_voter() {
        let _home = test_home();
        let cast = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        );
        let mut shouted = cast.clone();
        shouted.signature.key = shouted.signature.key.to_ascii_uppercase();
        shouted.signature.sig = shouted.signature.sig.to_ascii_uppercase();
        let later = sign_as(
            ballot_entry("alpha/seat-a", "reject", "2026-10-21T12:00:00Z"),
            3,
        );
        for (name, entries, stance) in [
            ("copy", vec![cast.clone(), cast.clone()], "accept"),
            ("hex case", vec![cast.clone(), shouted.clone()], "accept"),
            (
                "superseded copy",
                vec![cast.clone(), later.clone(), cast.clone()],
                "reject",
            ),
        ] {
            let report = check_entries(&entries).unwrap_or_else(|err| panic!("{name}: {err:#}"));
            let read = tally(&entries).expect("tally");
            assert_eq!(
                (report.questions_with_live_ballots, counted(&read, stance)),
                (1, Some(1)),
                "a replayed ballot erased its voter ({name}): {read:?}"
            );
        }
    }

    #[test]
    fn one_key_counts_once_on_a_question() {
        let _home = test_home();
        let entries = [
            sign_as(
                ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
                3,
            ),
            sign_as(
                ballot_entry("beta/seat-b", "accept", "2026-10-21T11:00:00Z"),
                3,
            ),
            sign_as(
                ballot_entry("alpha/run-101", "reject", "2026-10-21T12:00:00Z"),
                3,
            ),
            sign_as(
                ballot_entry("gamma/seat-c", "accept", "2026-10-21T10:00:00Z"),
                4,
            ),
        ];
        let report = check_entries(&entries).expect("check");
        assert_eq!(report.questions_with_live_ballots, 1);
        let read = tally(&entries).expect("tally");
        assert_eq!(counted(&read, "reject"), Some(1), "{read:?}");
        assert_eq!(counted(&read, "accept"), Some(1), "{read:?}");
    }

    #[test]
    fn distinct_persona_seats_with_distinct_keys_count_apart() {
        let _home = test_home();
        let entries = [
            sign_as(
                ballot_entry("ada/persona-a", "accept", "2026-10-21T10:00:00Z"),
                1,
            ),
            sign_as(
                ballot_entry("grace/persona-b", "reject", "2026-10-21T11:00:00Z"),
                2,
            ),
        ];
        let read = tally(&entries).expect("tally");
        assert_eq!(counted(&read, "accept"), Some(1), "{read:?}");
        assert_eq!(counted(&read, "reject"), Some(1), "{read:?}");
    }

    #[test]
    fn key_links_on_another_question_do_not_merge_voters_here() {
        let _home = test_home();
        let entries = [
            sign_as(
                ballot_on_question(
                    "alpha/seat-a",
                    "accept",
                    "2026-10-21T10:00:00Z",
                    "correct-3f9a1c2e",
                ),
                1,
            ),
            sign_as(
                ballot_on_question(
                    "beta/seat-b",
                    "reject",
                    "2026-10-21T11:00:00Z",
                    "correct-3f9a1c2e",
                ),
                2,
            ),
            sign_as(
                ballot_on_question(
                    "gamma/seat-a",
                    "accept",
                    "2026-10-21T12:00:00Z",
                    "change-deadbeef",
                ),
                2,
            ),
        ];
        let read = tally(&entries).expect("tally");
        let first = read.get("correct-3f9a1c2e").expect("first question");
        assert_eq!(first.get("accept"), Some(&1), "{read:?}");
        assert_eq!(first.get("reject"), Some(&1), "{read:?}");
    }

    #[test]
    fn spellings_of_one_voter_under_one_key_count_once() {
        let _home = test_home();
        let spellings = [
            "alpha/seat-a",
            "Alpha/seat-a",
            "alpha./seat-a",
            "alpha_/seat-a",
            "alpha/Seat-a",
            "alpha/run-101",
            "Alpha/run-101",
            "alpha./run-101",
            "alpha_/run-101",
            "alpha/inky",
            "alpha/inky-39u",
            "alpha/ljos-bot",
            "alpha/ljosbot",
            "alpha/judge-model-1-5",
        ];
        let entries: Vec<SignedEntry> = spellings
            .iter()
            .zip(1..)
            .map(|(voter, hour)| {
                let at = format!("2026-10-21T{hour:02}:00:00Z");
                sign_unchecked(ballot_entry(voter, "accept", &at), 3)
            })
            .collect();
        let read = tally(&entries).expect("tally");
        assert_eq!(counted(&read, "accept"), Some(1), "{read:?}");
    }

    #[test]
    fn one_seat_counts_once_under_two_keys() {
        let _home = test_home();
        let entries = [
            sign_as(
                ballot_entry("alpha/inky", "accept", "2026-10-21T10:00:00Z"),
                1,
            ),
            sign_as(
                ballot_entry("beta/inky", "reject", "2026-10-21T11:00:00Z"),
                2,
            ),
        ];
        check_entries(&entries).expect("no pin file");
        let read = tally(&entries).expect("tally");
        assert_eq!(counted(&read, "accept"), Some(1), "{read:?}");
        assert_eq!(counted(&read, "reject"), None, "{read:?}");
    }

    #[test]
    fn a_digit_suffix_is_a_seat_of_its_own() {
        let _home = test_home();
        let entries = [
            sign_as(
                ballot_entry("alpha/inky", "accept", "2026-10-21T10:00:00Z"),
                1,
            ),
            sign_as(
                ballot_entry("beta/inky-10", "accept", "2026-10-21T11:00:00Z"),
                2,
            ),
        ];
        check_entries(&entries).expect("two seats");
        let read = tally(&entries).expect("tally");
        assert_eq!(counted(&read, "accept"), Some(2), "{read:?}");
    }

    #[test]
    fn runs_and_judges_keep_their_ids_whole() {
        let _home = test_home();
        for (one, two) in [
            ("alpha/judge-gpt-oss-120b", "beta/judge-gpt-oss-20b"),
            ("alpha/judge-llama-3-70b", "beta/judge-llama-3-8b"),
            ("alpha/judge-qwen", "beta/judge-qwen-72"),
            ("alpha/run-pr-1234", "beta/run-pr-5678"),
            ("alpha/judge-grok-4", "beta/grok-4"),
        ] {
            let entries = [
                sign_as(ballot_entry(one, "accept", "2026-10-21T10:00:00Z"), 1),
                sign_as(ballot_entry(two, "accept", "2026-10-21T11:00:00Z"), 2),
            ];
            check_entries(&entries).unwrap_or_else(|err| panic!("{one} {two}: {err:#}"));
            let read = tally(&entries).expect("tally");
            assert_eq!(
                counted(&read, "accept"),
                Some(2),
                "{one} and {two} were merged into one voter"
            );
        }
    }

    #[test]
    fn one_key_under_two_voters_in_one_hour_is_flagged() {
        let _home = test_home();
        let at = "2026-10-21T10:00:00Z";
        let entries = [
            sign_as(ballot_entry("alpha/seat-a", "accept", at), 3),
            sign_as(ballot_entry("beta/seat-b", "reject", at), 3),
        ];
        let report = check_entries(&entries).expect("one key, one-hour ambiguity");
        assert_eq!(report.questions_with_live_ballots, 0, "{report:?}");
        let read = tally(&entries).expect("tally");
        assert!(read.is_empty(), "{read:?}");
    }

    #[test]
    fn a_key_in_capital_hex_is_the_same_key() {
        let _home = test_home();
        let lower = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        );
        let mut upper = sign_as(
            ballot_entry("beta/seat-b", "accept", "2026-10-21T11:00:00Z"),
            3,
        );
        upper.signature.key = upper.signature.key.to_ascii_uppercase();
        assert_ne!(lower.signature.key, upper.signature.key);
        let entries = [lower, upper];
        check_entries(&entries).expect("capital hex verifies");
        let read = tally(&entries).expect("tally");
        assert_eq!(counted(&read, "accept"), Some(1), "{read:?}");
    }

    #[test]
    fn a_key_with_a_small_order_part_is_the_same_key() {
        let _home = test_home();
        let seed = [3u8; 32];
        let first = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        );
        let mut order_two = [0xff; 32];
        order_two[0] = 0xec;
        order_two[31] = 0x7f;
        let order_two = ed25519_dalek::VerifyingKey::from_bytes(&order_two).expect("point");
        assert!(order_two.is_weak());
        let public = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key();
        let twin = (public.to_edwards() + order_two.to_edwards())
            .compress()
            .to_bytes();
        let twin = ed25519_dalek::VerifyingKey::from_bytes(&twin).expect("twin");
        assert!(!twin.is_weak());
        let secret = ed25519_dalek::hazmat::ExpandedSecretKey::from(&seed);
        // Under the twin a signature verifies only when its challenge is even.
        let second = (11..24)
            .find_map(|hour| {
                let at = format!("2026-10-21T{hour}:00:00Z");
                let mut unsigned = ballot_entry("beta/seat-b", "accept", &at);
                let Entry::Ballot(ballot) = &mut unsigned.entry else {
                    panic!("ballot");
                };
                ballot.id = Some(ballot_id(ballot).expect("id"));
                let payload = signing_payload(&unsigned).expect("payload");
                let sig = ed25519_dalek::hazmat::raw_sign::<sha2::Sha512>(&secret, &payload, &twin);
                twin.verify_strict(&payload, &sig)
                    .is_ok()
                    .then(|| SignedEntry {
                        schema: unsigned.schema,
                        entry: unsigned.entry,
                        signature: EntrySignature {
                            key: hex::encode(twin.to_bytes()),
                            sig: hex::encode(sig.to_bytes()),
                        },
                    })
            })
            .expect("a ballot the twin key verifies");
        let entries = [first, second];
        check_entries(&entries).expect("both keys verify");
        let read = tally(&entries).expect("tally");
        assert_eq!(counted(&read, "accept"), Some(1), "{read:?}");
    }

    fn with_replaces(mut entry: UnsignedEntry, id: &str) -> UnsignedEntry {
        let Entry::Ballot(ballot) = &mut entry.entry else {
            panic!("ballot");
        };
        ballot.replaces = Some(id.to_string());
        entry
    }

    #[test]
    fn a_same_hour_withdrawal_does_not_count_in_either_order() {
        let _home = test_home();
        let cast = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        );
        let cast_id = as_ballot(cast.clone()).id.clone().expect("id");
        let withdrawal = sign_as(
            with_replaces(
                ballot_entry("alpha/seat-a", "withdrawn", "2026-10-21T10:00:00Z"),
                &cast_id,
            ),
            3,
        );
        for (name, order) in [
            (
                "cast then withdrawal",
                vec![cast.clone(), withdrawal.clone()],
            ),
            (
                "withdrawal then cast",
                vec![withdrawal.clone(), cast.clone()],
            ),
        ] {
            let read = tally(&order).unwrap_or_else(|err| panic!("{name}: {err:#}"));
            let accepts = read
                .get("correct-3f9a1c2e")
                .and_then(|stances| stances.get("accept"))
                .copied();
            assert_eq!(accepts, None, "{name}: {read:?}");
        }
        for (cast_name, withdraw_name) in [("a.json", "b.json"), ("b.json", "a.json")] {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::write(
                dir.path().join(cast_name),
                serde_json::to_vec(&cast).expect("json"),
            )
            .expect("write cast");
            std::fs::write(
                dir.path().join(withdraw_name),
                serde_json::to_vec(&withdrawal).expect("json"),
            )
            .expect("write withdrawal");
            let report = check_dir(dir.path()).unwrap_or_else(|err| panic!("{cast_name}: {err:#}"));
            assert_eq!(
                report.questions_with_live_ballots, 0,
                "{cast_name} before {withdraw_name}"
            );
        }
    }

    #[test]
    fn replaces_must_target_the_same_voter_and_question() {
        let _home = test_home();
        let missing = sign_as(
            with_replaces(
                ballot_entry("alpha/seat-a", "withdrawn", "2026-10-21T11:00:00Z"),
                "deadbeefdeadbeef",
            ),
            3,
        );
        let err = tally(std::slice::from_ref(&missing)).expect_err("missing replace target");
        assert!(
            err_chain(&err).contains("replaces"),
            "missing replace target passed: {}",
            err_chain(&err)
        );

        let cast = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        );
        let cast_id = as_ballot(cast.clone()).id.expect("id");
        let same_key_other_voter = sign_as(
            with_replaces(
                ballot_entry("beta/seat-b", "withdrawn", "2026-10-21T11:00:00Z"),
                &cast_id,
            ),
            3,
        );
        let read = tally(&[cast.clone(), same_key_other_voter])
            .expect("one key is one voter across voter ids");
        assert_eq!(counted(&read, "accept"), None, "{read:?}");

        let other_voter = sign_as(
            with_replaces(
                ballot_entry("beta/seat-b", "withdrawn", "2026-10-21T11:00:00Z"),
                &cast_id,
            ),
            4,
        );
        let err = tally(&[cast.clone(), other_voter]).expect_err("cross-voter replace");
        assert!(
            err_chain(&err).contains("same voter"),
            "cross-voter replace passed: {}",
            err_chain(&err)
        );

        let mut other_question_unsigned = with_replaces(
            ballot_entry("alpha/seat-a", "withdrawn", "2026-10-21T11:00:00Z"),
            &cast_id,
        );
        let Entry::Ballot(ballot) = &mut other_question_unsigned.entry else {
            panic!("ballot");
        };
        ballot.question = "change-deadbeef".to_string();
        let other_question = sign_as(other_question_unsigned, 3);
        let err = tally(&[cast, other_question]).expect_err("cross-question replace");
        assert!(
            err_chain(&err).contains("same question"),
            "cross-question replace passed: {}",
            err_chain(&err)
        );
    }

    #[test]
    fn check_dir_reads_and_verifies_json_entries() {
        let _home = test_home();
        let dir = tempfile::tempdir().expect("tempdir");
        let q = sign_as(question_entry(), 5);
        let b = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            5,
        );
        std::fs::write(
            dir.path().join("question.json"),
            serde_json::to_string_pretty(&q).expect("json"),
        )
        .expect("write question");
        std::fs::write(
            dir.path().join("ballot.json"),
            serde_json::to_string_pretty(&b).expect("json"),
        )
        .expect("write ballot");
        let report = check_dir(dir.path()).expect("check");
        assert_eq!(report.files, 2);
        assert_eq!(report.entries, 2);
        assert_eq!(report.questions, 1);
        assert_eq!(report.ballots, 1);
        assert_eq!(report.questions_with_live_ballots, 1);
    }

    fn err_chain(err: &anyhow::Error) -> String {
        format!("{err:#}")
    }

    #[test]
    fn timestamps_must_be_utc_hours() {
        let _home = test_home();
        for bad in [
            "2026-10-21T10:00:00+05:30",
            "2026-10-21T04:00:00-05:00",
            "2026-10-21T10:00:00.000Z",
            "2026-10-21t10:00:00z",
            "2026-10-21 10:00:00Z",
            "2026-10-21T10:30:00Z",
        ] {
            let err = sign_entry(ballot_entry("alpha/seat-a", "accept", bad), "alpha/seat-a")
                .expect_err(bad);
            let text = err_chain(&err);
            assert!(
                text.contains("YYYY-MM-DDTHH:00:00Z"),
                "{bad} was accepted or the error hid the form: {text}"
            );
        }
        let mut opened = question_entry();
        let Entry::Question(question) = &mut opened.entry else {
            panic!("question");
        };
        question.opened_at = "2026-10-21T09:00:00+00:00".to_string();
        let err = sign_entry(opened, "alpha/seat-a").expect_err("offset opened_at");
        assert!(
            err_chain(&err).contains("YYYY-MM-DDTHH:00:00Z"),
            "{}",
            err_chain(&err)
        );
        sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            8,
        );
    }

    #[test]
    fn stance_spelling_must_be_exact_lowercase() {
        let _home = test_home();
        for bad in ["accept ", "accept\u{200b}", "Accept", "WITHDRAWN"] {
            let err = sign_entry(
                ballot_entry("alpha/seat-a", bad, "2026-10-21T10:00:00Z"),
                "alpha/seat-a",
            )
            .expect_err(bad);
            assert!(
                err_chain(&err).contains("stance"),
                "{bad} passed stance validation: {}",
                err_chain(&err)
            );
        }
        for ok in ["accept", "reject", "withdrawn"] {
            sign_entry(
                ballot_entry("alpha/seat-a", ok, "2026-10-21T10:00:00Z"),
                "alpha/seat-a",
            )
            .unwrap_or_else(|err| panic!("{ok}: {err:#}"));
        }
    }

    /// Reserved names and any key the signature does not cover are refused
    /// on the file, including inside `signature`.
    #[test]
    fn reserved_and_unsigned_fields_are_refused() {
        let _home = test_home();
        let signed = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            8,
        );
        let base = serde_json::to_value(&signed).expect("value");

        let mut host_in_sig = base.clone();
        host_in_sig["signature"]["host"] = serde_json::json!("box");
        let err = check_value(&host_in_sig).expect_err("host inside signature");
        assert!(
            err_chain(&err).contains("host"),
            "reserved host was accepted: {}",
            err_chain(&err)
        );

        let mut nested_agent = base.clone();
        nested_agent["signature"]["note"] = serde_json::json!({"agent": "runner"});
        let err = check_value(&nested_agent).expect_err("nested agent");
        assert!(
            err_chain(&err).contains("agent"),
            "nested agent was accepted: {}",
            err_chain(&err)
        );

        let mut session_in_array = base.clone();
        session_in_array["signature"]["meta"] = serde_json::json!([{"session": "s"}]);
        let err = check_value(&session_in_array).expect_err("session in array");
        assert!(
            err_chain(&err).contains("session"),
            "session in an array was accepted: {}",
            err_chain(&err)
        );

        let mut rationale = base.clone();
        rationale["rationale"] = serde_json::json!("because");
        let err = check_value(&rationale).expect_err("unsigned rationale");
        assert!(
            err_chain(&err).contains("does not cover"),
            "unsigned rationale was accepted: {}",
            err_chain(&err)
        );
    }

    #[test]
    fn unknown_signature_fields_are_refused() {
        let _home = test_home();
        let signed = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            8,
        );
        let base = serde_json::to_value(&signed).expect("value");
        for (field, value) in [
            ("rationale", serde_json::json!("because")),
            ("findings", serde_json::json!(["one"])),
            ("Host", serde_json::json!("box")),
        ] {
            let mut changed = base.clone();
            changed["signature"][field] = value;
            let err = check_value(&changed).expect_err(field);
            assert!(
                err_chain(&err).contains(field),
                "{field} passed under signature: {}",
                err_chain(&err)
            );
        }
        let mut nested = base.clone();
        nested["signature"]["note"] = serde_json::json!({"Session": "s"});
        let err = check_value(&nested).expect_err("nested note.Session");
        assert!(
            err_chain(&err).contains("note"),
            "nested note.Session passed under signature: {}",
            err_chain(&err)
        );
    }

    #[test]
    fn duplicate_json_keys_are_refused() {
        let _home = test_home();
        let signed = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            8,
        );
        let text = serde_json::to_string(&signed).expect("json");

        let dup_stance = text.replacen(
            "\"stance\":\"accept\",",
            "\"stance\":\"accept\",\"stance\":\"accept\",",
            1,
        );
        assert_ne!(dup_stance, text, "duplicate stance fixture");
        let err = check_raw(&dup_stance).expect_err("duplicate stance");
        assert!(
            err_chain(&err).contains("duplicate key `stance`"),
            "duplicate stance was not refused as duplicate: {}",
            err_chain(&err)
        );

        let sig = format!("\"sig\":\"{}\"", signed.signature.sig);
        let dup_sig = text.replacen(&sig, &format!("{sig},{sig}"), 1);
        assert_ne!(dup_sig, text, "duplicate signature fixture");
        let err = check_raw(&dup_sig).expect_err("duplicate signature");
        assert!(
            err_chain(&err).contains("duplicate key `sig`"),
            "duplicate signature key was not refused as duplicate: {}",
            err_chain(&err)
        );
    }

    #[test]
    fn a_pin_file_rejects_a_different_run_key() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let entry = sign_as(
            ballot_entry("alpha/run-101", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let other = sign_as(
            ballot_entry("beta/seat-b", "reject", "2026-10-21T11:00:00Z"),
            2,
        );
        let line = format!("alpha/run-101 {}\n", other.signature.key);
        let pin_path = root.path().join(crate::ballot_sig::PIN_FILE);
        std::fs::write(&pin_path, line).expect("pin");
        let err = check_entries(std::slice::from_ref(&entry)).expect_err("pinned key");
        let text = err_chain(&err);
        assert!(text.contains("pinned"), "{text}");
        assert!(text.contains(pin_path.to_string_lossy().as_ref()), "{text}");
    }

    #[test]
    fn unreadable_pin_file_is_refused() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pin_path = root.path().join(crate::ballot_sig::PIN_FILE);
        std::fs::write(&pin_path, "alpha/run-101 deadbeef\n").expect("pin");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&pin_path).expect("meta").permissions();
            perms.set_mode(0o000);
            std::fs::set_permissions(&pin_path, perms).expect("chmod");
        }
        let entry = sign_as(
            ballot_entry("alpha/run-101", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let err = check_entries(std::slice::from_ref(&entry)).expect_err("unreadable pin file");
        assert!(
            err_chain(&err).contains(pin_path.to_string_lossy().as_ref()),
            "{}",
            err_chain(&err)
        );
    }

    #[test]
    fn a_dangling_pin_file_link_is_refused() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pin_path = root.path().join(crate::ballot_sig::PIN_FILE);
        std::os::unix::fs::symlink(root.path().join("gone"), &pin_path).expect("symlink");
        let entry = sign_as(
            ballot_entry("alpha/run-101", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let err = check_entries(std::slice::from_ref(&entry))
            .expect_err("a dangling pin file link read as no pin file");
        assert!(
            err_chain(&err).contains(pin_path.to_string_lossy().as_ref()),
            "{}",
            err_chain(&err)
        );
    }

    #[test]
    fn duplicate_pin_voter_lines_are_refused() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let entry = sign_as(
            ballot_entry("alpha/run-101", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let line = format!(
            "alpha/run-101 {}\nalpha/run-101 {}\n",
            entry.signature.key, entry.signature.key
        );
        std::fs::write(root.path().join(crate::ballot_sig::PIN_FILE), line).expect("pin");
        let err = check_entries(std::slice::from_ref(&entry)).expect_err("duplicate pin voter");
        assert!(err_chain(&err).contains("duplicate"), "{}", err_chain(&err));
    }

    #[test]
    fn a_seat_voter_must_match_the_tracker_seat_pin() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pinned = sign_as(
            ballot_entry("beta/seat-b", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let entry = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            2,
        );
        let line = format!("seat-a {}\n", pinned.signature.key);
        std::fs::write(root.path().join(crate::ballot_sig::PIN_FILE), line).expect("pin");
        let err = check_entries(std::slice::from_ref(&entry)).expect_err("seat pin mismatch");
        assert!(err_chain(&err).contains("seat-a"), "{}", err_chain(&err));
    }

    #[test]
    fn a_voter_pin_row_cannot_give_a_seat_a_second_key() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let at = "2026-10-21T10:00:00Z";
        let owner = sign_as(ballot_entry("alpha/inky", "accept", at), 1);
        let mallory = sign_as(ballot_entry("mallory/inky", "reject", at), 9);
        let (k1, k9) = (&owner.signature.key, &mallory.signature.key);
        let pin_path = root.path().join(crate::ballot_sig::PIN_FILE);
        for rows in [
            format!("inky {k1}\nmallory/inky {k9}\n"),
            format!("alpha/inky {k1}\nmallory/inky {k9}\n"),
        ] {
            std::fs::write(&pin_path, &rows).expect("pin");
            let Err(err) = check_entries(std::slice::from_ref(&mallory)) else {
                panic!("a second key voted under a pinned seat: {rows}");
            };
            assert!(
                err_chain(&err).contains("seat inky is pinned to two keys"),
                "{}",
                err_chain(&err)
            );
        }
    }

    #[test]
    fn a_seat_pin_accepts_same_key_in_capital_hex() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pinned = sign_as(
            ballot_entry("beta/seat-b", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let mut entry = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T11:00:00Z"),
            1,
        );
        let pin_path = root.path().join(crate::ballot_sig::PIN_FILE);
        std::fs::write(&pin_path, format!("seat-a {}\n", pinned.signature.key)).expect("pin");
        entry.signature.key = entry.signature.key.to_ascii_uppercase();
        check_entries(std::slice::from_ref(&entry))
            .unwrap_or_else(|err| panic!("seat pin should match key hex case: {err:#}"));
    }

    #[test]
    fn a_pin_voter_that_is_not_canonical_is_refused() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pinned = sign_as(
            ballot_entry("beta/seat-b", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let entry = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            2,
        );
        let key = &pinned.signature.key;
        let pin_path = root.path().join(crate::ballot_sig::PIN_FILE);
        for voter in [
            "\u{feff}seat-a",
            "\u{200b}seat-a",
            "Seat-a",
            "seat_a",
            "Alpha/run-101",
        ] {
            std::fs::write(&pin_path, format!("{voter} {key}\n")).expect("pin");
            let Err(err) = check_entries(std::slice::from_ref(&entry)) else {
                panic!("the {voter:?} pin row was skipped");
            };
            assert!(
                err_chain(&err).contains("pin voter"),
                "{voter:?}: {}",
                err_chain(&err)
            );
        }
    }

    #[test]
    fn holder_judge_and_legacy_pin_rows_hold_their_seat() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let at = "2026-10-21T10:00:00Z";
        let key = sign_as(ballot_entry("beta/seat-b", "accept", at), 1)
            .signature
            .key;
        let rows: String = [
            "inky@39u",
            "disk-saver-t4t",
            "ljos-bot",
            "judge:model-1.5",
            "seat-b",
            "alpha/run-101",
        ]
        .iter()
        .map(|row| format!("{row} {key}\n"))
        .collect();
        std::fs::write(root.path().join(crate::ballot_sig::PIN_FILE), rows).expect("pin");
        for (voter, seat) in [
            ("alpha/inky", "inky"),
            ("alpha/disk-saver", "disk-saver"),
            ("alpha/ljosbot", "ljosbot"),
            ("alpha/judge-model-1-5", "judge-model-1-5"),
        ] {
            let other = sign_as(ballot_entry(voter, "accept", at), 2);
            let err = check_entries(std::slice::from_ref(&other))
                .expect_err("a second key voted under a pinned seat's alias");
            assert!(
                err_chain(&err).contains(&format!("{voter} is pinned through seat {seat}")),
                "{voter}: {}",
                err_chain(&err)
            );
            let held = sign_as(ballot_entry(voter, "accept", at), 1);
            check_entries(std::slice::from_ref(&held))
                .unwrap_or_else(|err| panic!("{voter} under its pinned key: {err:#}"));
        }
        let free = sign_as(ballot_entry("alpha/seat-a", "accept", at), 2);
        check_entries(std::slice::from_ref(&free)).expect("no row folds to seat-a");
    }

    #[test]
    fn a_seat_pin_does_not_hold_a_run_or_judge_of_that_name() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pinned = sign_as(
            ballot_entry("beta/seat-b", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        std::fs::write(
            root.path().join(crate::ballot_sig::PIN_FILE),
            format!("inky {}\n", pinned.signature.key),
        )
        .expect("pin");
        for voter in ["alpha/run-inky", "alpha/judge-inky"] {
            let other = sign_as(ballot_entry(voter, "accept", "2026-10-21T11:00:00Z"), 2);
            check_entries(std::slice::from_ref(&other))
                .unwrap_or_else(|err| panic!("{voter} was held to seat inky: {err:#}"));
        }
    }

    #[test]
    fn a_seat_pinned_to_two_keys_under_two_names_is_refused() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pin_path = root.path().join(crate::ballot_sig::PIN_FILE);
        let at = "2026-10-21T10:00:00Z";
        let bare = sign_as(ballot_entry("alpha/inky", "accept", at), 1);
        let tagged = sign_as(ballot_entry("beta/inky", "accept", at), 2);
        let (key1, key2) = (&bare.signature.key, &tagged.signature.key);
        std::fs::write(&pin_path, format!("inky {key1}\ninky@39u {key2}\n")).expect("pin");
        let entries = [bare.clone(), tagged.clone()];
        let Err(err) = check_entries(&entries) else {
            panic!("one seat voted under two keys: {:?}", tally(&entries));
        };
        let text = err_chain(&err);
        assert!(text.contains("seat inky is pinned to two keys"), "{text}");
        let read = tally(&entries).expect("one seat keeps its first key");
        assert_eq!(counted(&read, "accept"), Some(1), "{read:?}");
        let later = sign_as(
            ballot_entry("beta/inky", "accept", "2026-10-21T11:00:00Z"),
            2,
        );
        let read = tally(&[bare.clone(), later]).expect("tally");
        assert_eq!(counted(&read, "accept"), Some(1), "{read:?}");
        std::fs::write(&pin_path, format!("inky {key1}\ninky@39u {key1}\n")).expect("pin");
        check_entries(std::slice::from_ref(&bare)).expect("one key under both names");
        let err = check_entries(std::slice::from_ref(&tagged)).expect_err("an unpinned key");
        let text = err_chain(&err);
        assert!(
            text.contains("(inky)") && text.contains("(inky@39u)"),
            "{text}"
        );
    }

    #[test]
    fn a_run_seat_with_a_pin_row_is_held_to_it() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pinned = sign_as(
            ballot_entry("beta/seat-b", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let entry = sign_as(
            ballot_entry("alpha/run-101", "accept", "2026-10-21T10:00:00Z"),
            2,
        );
        let line = format!("run-101 {}\n", pinned.signature.key);
        std::fs::write(root.path().join(crate::ballot_sig::PIN_FILE), line).expect("pin");
        let err = check_entries(std::slice::from_ref(&entry))
            .expect_err("alpha/run-101 passed beside a run-101 pin row");
        assert!(
            err_chain(&err).contains("through seat run-101"),
            "{}",
            err_chain(&err)
        );
    }

    #[test]
    fn a_voter_seat_part_must_match_its_slug() {
        let _home = test_home();
        let root = tempfile::tempdir().expect("root");
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("VISSUE_ROOT", root.path()) };
        });
        let pinned = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            1,
        );
        let line = format!("seat-a {}\n", pinned.signature.key);
        std::fs::write(root.path().join(crate::ballot_sig::PIN_FILE), line).expect("pin");
        let other = sign_unchecked(
            ballot_entry("alpha/Seat-a", "accept", "2026-10-21T11:00:00Z"),
            1,
        );
        let entries = [pinned, other];
        match check_entries(&entries) {
            Ok(_) => panic!(
                "alpha/Seat-a passed beside alpha/seat-a: {:?}",
                tally(&entries)
            ),
            Err(err) => {
                let text = err_chain(&err);
                assert!(text.contains("must be written `alpha/seat-a`"), "{text}");
            }
        }
    }

    fn check_value(value: &serde_json::Value) -> anyhow::Result<CheckReport> {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("entry.json"),
            serde_json::to_vec(value).expect("json"),
        )
        .expect("write");
        check_dir(dir.path())
    }

    fn check_raw(raw: &str) -> anyhow::Result<CheckReport> {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("entry.json"), raw).expect("write");
        check_dir(dir.path())
    }
}
