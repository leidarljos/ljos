//! Record schema v1: questions and ballots, signed as canonical JSON.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, FixedOffset, Timelike};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The only accepted schema value for record entries.
pub const SCHEMA_V1: &str = "record/v1";
const SIGNING_LABEL: &str = "ljos-record-entry/v1";
const RESERVED_PLAIN_FIELDS: &[&str] = &["host", "session", "agent"];
const QUESTION_TYPES: &[&str] = &["correct", "locate", "change", "design", "lead"];

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

/// Sign one entry. Ballot entries get `id` filled when absent.
///
/// # Errors
///
/// The entry does not pass schema validation.
pub fn sign_entry(mut unsigned: UnsignedEntry, key: &SigningKey) -> Result<SignedEntry> {
    if unsigned.schema.trim().is_empty() {
        unsigned.schema = SCHEMA_V1.to_string();
    }
    if let Entry::Ballot(ballot) = &mut unsigned.entry {
        if ballot.id.is_none() {
            ballot.id = Some(ballot_id(ballot)?);
        }
    }
    validate_unsigned(&unsigned)?;
    let payload = signing_payload(&unsigned)?;
    let sig = key.sign(&payload);
    Ok(SignedEntry {
        schema: unsigned.schema,
        entry: unsigned.entry,
        signature: EntrySignature {
            key: hex::encode(key.verifying_key().to_bytes()),
            sig: hex::encode(sig.to_bytes()),
        },
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
    let key = decode_key(&entry.signature.key)?;
    let sig = decode_sig(&entry.signature.sig)?;
    key.verify(&payload, &sig)
        .map_err(|_| anyhow!("entry signature does not verify"))
}

/// Run schema + signature checks over many entries, including run-key pinning.
///
/// A voter id of the form `run/<run-id>` pins `<run-id>` to the first public
/// key seen for it. A later ballot from the same run id under another key is
/// a mismatch.
///
/// # Errors
///
/// Any entry fails validation or signature verification, or run-key pinning
/// detects a mismatch.
pub fn check_entries(entries: &[SignedEntry]) -> Result<CheckReport> {
    let mut report = CheckReport {
        entries: entries.len(),
        ..CheckReport::default()
    };
    let mut run_keys: BTreeMap<String, String> = BTreeMap::new();
    let mut ballots = Vec::new();
    for entry in entries {
        verify_entry(entry)?;
        match &entry.entry {
            Entry::Question(_) => report.questions += 1,
            Entry::Ballot(ballot) => {
                report.ballots += 1;
                if let Some(run_id) = run_id_of(&ballot.voter) {
                    if let Some(pinned) = run_keys.get(run_id) {
                        if pinned != &entry.signature.key {
                            bail!(
                                "run id {run_id} is pinned to key {}…, got {}…",
                                short_key(pinned),
                                short_key(&entry.signature.key)
                            );
                        }
                    } else {
                        run_keys.insert(run_id.to_string(), entry.signature.key.clone());
                    }
                }
                ballots.push(ballot.clone());
            }
        }
    }
    report.questions_with_live_ballots = tally(&ballots).len();
    Ok(report)
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
        let entry: SignedEntry = serde_json::from_str(&raw)
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

/// Latest ballot per voter and question. A latest `withdrawn` ballot does not
/// count.
#[must_use]
pub fn tally(ballots: &[BallotEntry]) -> BTreeMap<String, BTreeMap<String, usize>> {
    let mut latest: BTreeMap<(String, String), (i64, usize)> = BTreeMap::new();
    for (i, ballot) in ballots.iter().enumerate() {
        let key = (ballot.question.clone(), ballot.voter.clone());
        let stamp = parse_hour_timestamp(&ballot.cast_at)
            .map(|t| t.timestamp())
            .unwrap_or(i64::MIN);
        let candidate = (stamp, i);
        match latest.get(&key) {
            Some(current) if *current >= candidate => {}
            _ => {
                latest.insert(key, candidate);
            }
        }
    }
    let mut counts: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for ((question, _), (_, idx)) in latest {
        let ballot = &ballots[idx];
        if ballot.stance.eq_ignore_ascii_case("withdrawn") {
            continue;
        }
        let by_stance = counts.entry(question).or_default();
        *by_stance.entry(ballot.stance.clone()).or_default() += 1;
    }
    counts
}

fn validate_unsigned(unsigned: &UnsignedEntry) -> Result<()> {
    if unsigned.schema != SCHEMA_V1 {
        bail!(
            "record schema must be {SCHEMA_V1}, got {}",
            unsigned.schema.trim()
        );
    }
    let plain = serde_json::to_value(unsigned)?;
    reject_reserved_plain_fields(&plain)?;
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
    if ballot.stance.trim().is_empty() {
        bail!("ballot stance is required");
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
    Ok(())
}

fn parse_hour_timestamp(text: &str) -> Result<DateTime<FixedOffset>> {
    let parsed = DateTime::parse_from_rfc3339(text)
        .with_context(|| format!("timestamp `{text}` is not RFC 3339"))?;
    if parsed.minute() != 0 || parsed.second() != 0 || parsed.nanosecond() != 0 {
        bail!("timestamp `{text}` is not hour precision");
    }
    Ok(parsed)
}

fn run_id_of(voter: &str) -> Option<&str> {
    let (identity, seat_or_run) = split_voter(voter)?;
    (identity == "run").then_some(seat_or_run)
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

fn decode_key(hex_text: &str) -> Result<VerifyingKey> {
    let bytes = hex::decode(hex_text).context("signature key is not valid hex")?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("signature key must be 32 bytes"))?;
    VerifyingKey::from_bytes(&key).map_err(|_| anyhow!("signature key bytes are invalid"))
}

fn decode_sig(hex_text: &str) -> Result<ed25519_dalek::Signature> {
    let bytes = hex::decode(hex_text).context("signature is not valid hex")?;
    let sig: [u8; 64] = bytes
        .try_into()
        .map_err(|_| anyhow!("signature must be 64 bytes"))?;
    Ok(ed25519_dalek::Signature::from_bytes(&sig))
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
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn short_key(key: &str) -> &str {
    let n = 12.min(key.len());
    &key[..n]
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

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
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
        let entry = sign_entry(question_entry(), &key(7)).expect("signed");
        verify_entry(&entry).expect("valid signature");
    }

    #[test]
    fn a_one_byte_edit_fails_verification() {
        let mut entry = sign_entry(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            &key(8),
        )
        .expect("signed");
        let Entry::Ballot(ballot) = &mut entry.entry else {
            panic!("ballot expected");
        };
        ballot.stance.replace_range(5..6, "u");
        assert!(verify_entry(&entry).is_err());
    }

    #[test]
    fn a_second_key_under_one_run_id_is_a_mismatch() {
        let first = sign_entry(
            ballot_entry("run/run-101", "accept", "2026-10-21T10:00:00Z"),
            &key(1),
        )
        .expect("first signed");
        let second = sign_entry(
            ballot_entry("run/run-101", "reject", "2026-10-21T11:00:00Z"),
            &key(2),
        )
        .expect("second signed");
        let err = check_entries(&[first, second]).expect_err("must reject run key mismatch");
        assert!(err.to_string().contains("pinned"));
    }

    #[test]
    fn latest_ballot_per_voter_wins_and_withdrawn_does_not_count() {
        let a1 = as_ballot(
            sign_entry(
                ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
                &key(3),
            )
            .expect("signed"),
        );
        let a2 = as_ballot(
            sign_entry(
                ballot_entry("alpha/seat-a", "withdrawn", "2026-10-21T11:00:00Z"),
                &key(3),
            )
            .expect("signed"),
        );
        let b1 = as_ballot(
            sign_entry(
                ballot_entry("beta/seat-b", "accept", "2026-10-21T10:00:00Z"),
                &key(4),
            )
            .expect("signed"),
        );
        let read = tally(&[a1, a2, b1]);
        let by_stance = read.get("correct-3f9a1c2e").expect("question tally");
        assert_eq!(by_stance.get("accept"), Some(&1));
        assert_eq!(by_stance.get("withdrawn"), None);
    }

    #[test]
    fn check_dir_reads_and_verifies_json_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let q = sign_entry(question_entry(), &key(5)).expect("signed question");
        let b = sign_entry(
            ballot_entry("alpha/seat-a", "accept", "2026-10-21T10:00:00Z"),
            &key(5),
        )
        .expect("signed ballot");
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
}
