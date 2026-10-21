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
/// Any entry fails validation or signature verification, voter-key pinning
/// detects a mismatch, or two unreplaced ballots from one voter share an
/// hour.
pub fn check_entries(entries: &[SignedEntry]) -> Result<CheckReport> {
    let mut report = CheckReport {
        entries: entries.len(),
        ..CheckReport::default()
    };
    let (mut pins, pin_source) = crate::ballot_sig::pins_for_check()?;
    let tracker_pins = pins.clone();
    let mut ballots = Vec::new();
    for entry in entries {
        verify_entry(entry)?;
        match &entry.entry {
            Entry::Question(question) => {
                report.questions += 1;
                observe_seat_pin(&tracker_pins, &question.opened_by, &entry.signature.key)
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
                observe_seat_pin(&tracker_pins, &ballot.voter, &entry.signature.key)
                    .with_context(|| format!("pin source: {pin_source}"))?;
                crate::ballot_sig::observe_pin(&mut pins, &ballot.voter, &entry.signature.key)
                    .with_context(|| format!("pin source: {pin_source}"))?;
                ballots.push(ballot.clone());
            }
        }
    }
    report.questions_with_live_ballots = tally(&ballots)?.len();
    Ok(report)
}

fn observe_seat_pin(tracker_pins: &BTreeMap<String, String>, voter: &str, key: &str) -> Result<()> {
    let Some((_, seat_or_run)) = split_voter(voter) else {
        return Ok(());
    };
    if seat_or_run.starts_with("run-") {
        return Ok(());
    }
    if let Some(seat_key) = tracker_pins.get(seat_or_run).filter(|pinned| *pinned != key) {
        bail!(
            "{voter} is pinned through seat {seat_or_run} to key {}…, got {}…",
            &seat_key[..12.min(seat_key.len())],
            &key[..12.min(key.len())]
        );
    }
    Ok(())
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
/// A ballot named by another entry's `replaces` never counts, whatever
/// hour it was cast. Of what remains, the latest `cast_at` wins, and a
/// `withdrawn` stance contributes no vote. Two unreplaced ballots from
/// one voter in one hour are an error: file order is not a tie break.
///
/// # Errors
///
/// A `cast_at` is not an hour timestamp, or two unreplaced ballots from
/// one voter share an hour.
pub fn tally(ballots: &[BallotEntry]) -> Result<BTreeMap<String, BTreeMap<String, usize>>> {
    let by_id: BTreeMap<&str, &BallotEntry> = ballots
        .iter()
        .filter_map(|ballot| ballot.id.as_deref().map(|id| (id, ballot)))
        .collect();
    let mut replaced = BTreeSet::new();
    for ballot in ballots {
        let Some(target_id) = ballot.replaces.as_deref() else {
            continue;
        };
        let source_id = ballot.id.as_deref().unwrap_or("<missing-id>");
        let target = by_id.get(target_id).copied().ok_or_else(|| {
            anyhow!(
                "ballot {source_id} from {} on {} replaces {target_id}, but no ballot has that id",
                ballot.voter,
                ballot.question
            )
        })?;
        if target.voter != ballot.voter {
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
    let mut grouped: BTreeMap<(String, String), Vec<(i64, &BallotEntry)>> = BTreeMap::new();
    for ballot in ballots {
        if ballot.id.as_deref().is_some_and(|id| replaced.contains(id)) {
            continue;
        }
        let stamp = parse_hour_timestamp(&ballot.cast_at)
            .with_context(|| format!("ballot cast_at `{}`", ballot.cast_at))?
            .timestamp();
        grouped
            .entry((ballot.question.clone(), ballot.voter.clone()))
            .or_default()
            .push((stamp, ballot));
    }
    let mut counts: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for ((question, voter), group) in grouped {
        let mut hours: BTreeMap<i64, usize> = BTreeMap::new();
        for (stamp, _) in &group {
            *hours.entry(*stamp).or_default() += 1;
        }
        if hours.values().any(|count| *count > 1) {
            bail!("two unreplaced ballots from {voter} on {question} in the same hour");
        }
        let Some((_, ballot)) = group.iter().max_by_key(|(stamp, _)| *stamp) else {
            continue;
        };
        if ballot.stance == "withdrawn" {
            continue;
        }
        *counts
            .entry(question)
            .or_default()
            .entry(ballot.stance.clone())
            .or_default() += 1;
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
    if !is_hex_len(suffix, 8) {
        bail!("question id suffix must be 8 hex chars");
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
    // `run/<id>` names no identity. A run carries one: `<identity>/run-<id>`.
    if identity == "run" {
        bail!("voter id `{voter}` has no identity; a run is `<identity>/run-<id>`");
    }
    Ok(())
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
    fn a_one_byte_edit_fails_verification() {
        let _home = test_home();
        let mut entry = sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            8,
        );
        let Entry::Ballot(ballot) = &mut entry.entry else {
            panic!("ballot expected");
        };
        // `stance` feeds the ballot id. Recompute it so the failure is the
        // signature, not the id check that used to hide this edit.
        ballot.stance.replace_range(5..6, "u");
        ballot.id = Some(ballot_id(ballot).expect("id"));
        let err = verify_entry(&entry).expect_err("edited ballot");
        assert!(
            err.to_string().contains("does not verify"),
            "edit never reached signature verification: {err}"
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
        assert!(err.to_string().contains("pinned"), "{err}");
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
        assert!(err.to_string().contains("pinned"), "{err}");
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
        let a1 = as_ballot(sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        ));
        let a2 = as_ballot(sign_as(
            ballot_entry("alpha/seat-a", "withdrawn", "2026-10-21T11:00:00Z"),
            3,
        ));
        let b1 = as_ballot(sign_as(
            ballot_entry("beta/seat-b", "accept", "2026-10-21T10:00:00Z"),
            4,
        ));
        let read = tally(&[a1, a2, b1]).expect("tally");
        let by_stance = read.get("correct-3f9a1c2e").expect("question tally");
        assert_eq!(by_stance.get("accept"), Some(&1));
        assert_eq!(by_stance.get("withdrawn"), None);
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
        let err = tally(&[as_ballot(accept), as_ballot(reject)]).expect_err("same hour");
        assert!(err.to_string().contains("same hour"), "{err}");
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
        let cast_ballot = as_ballot(cast.clone());
        let withdrawal_ballot = as_ballot(withdrawal.clone());
        for (name, order) in [
            (
                "cast then withdrawal",
                vec![cast_ballot.clone(), withdrawal_ballot.clone()],
            ),
            (
                "withdrawal then cast",
                vec![withdrawal_ballot.clone(), cast_ballot.clone()],
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
        let missing = as_ballot(sign_as(
            with_replaces(
                ballot_entry("alpha/seat-a", "withdrawn", "2026-10-21T11:00:00Z"),
                "deadbeefdeadbeef",
            ),
            3,
        ));
        let err = tally(std::slice::from_ref(&missing)).expect_err("missing replace target");
        assert!(
            err_chain(&err).contains("replaces"),
            "missing replace target passed: {}",
            err_chain(&err)
        );

        let cast = as_ballot(sign_as(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            3,
        ));
        let cast_id = cast.id.clone().expect("id");
        let other_voter = as_ballot(sign_as(
            with_replaces(
                ballot_entry("beta/seat-b", "withdrawn", "2026-10-21T11:00:00Z"),
                &cast_id,
            ),
            4,
        ));
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
        let other_question = as_ballot(sign_as(other_question_unsigned, 3));
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

        let dup_stance =
            text.replacen("\"stance\":\"accept\",", "\"stance\":\"accept\",\"stance\":\"accept\",", 1);
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
        assert!(
            err_chain(&err).contains("duplicate"),
            "{}",
            err_chain(&err)
        );
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
        assert!(
            err_chain(&err).contains("seat-a"),
            "{}",
            err_chain(&err)
        );
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
