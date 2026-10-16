//! One harness session is one voice. A ballot and a design candidate are
//! bound to the session that wrote them. That session is the harness id,
//! the holder, or the MCP connection, not a nonce minted per process.
//! The consensus gate refuses to settle when one session authored more
//! than one ballot, when a session both filed a candidate and voted, or
//! when every ballot traces to one session. Seats minted inside a session
//! do not multiply its vote.
//!
//! A design candidate whose session had already read another candidate is
//! flagged: the round was not blind.

use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::ballot_sig;

/// The read log at the tracker root: one line
/// `TIME HARNESS NONCE ISSUE` per recall.
pub const READS_FILE: &str = ".ljos-reads";

/// How a ballot traces to a session. An empty harness and nonce is a
/// legacy ballot from before binding, and the gate leaves those alone
/// when every ballot is unbound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BallotTrace {
    pub voter: String,
    pub harness: String,
    pub nonce: String,
}

/// A design candidate a session filed, and the other candidates that
/// session had already read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateTrace {
    pub id: String,
    pub harness: String,
    pub nonce: String,
    pub saw: Vec<String>,
}

/// The session a binding counts as. A harness id groups every process
/// that inherited it, including a worker that spawned one `ljos vote` per
/// seat. With no harness id, the process nonce is the session, so one
/// process still cannot mint several seats.
#[must_use]
pub fn session_key(harness: &str, nonce: &str) -> String {
    let h = harness.trim();
    if h.is_empty() || h == "-" {
        let n = nonce.trim();
        if n.is_empty() || n == "-" {
            String::new()
        } else {
            format!("nonce:{n}")
        }
    } else {
        h.to_string()
    }
}

/// Why a settle is refused, or `None` when the ballots come from distinct
/// sessions and no author voted. The sentence is the whole refusal.
#[must_use]
pub fn refusal(
    issue: &str,
    ballots: &[BallotTrace],
    candidates: &[CandidateTrace],
) -> Option<String> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for b in ballots {
        let key = session_key(&b.harness, &b.nonce);
        if key.is_empty() {
            continue;
        }
        groups.entry(key).or_default().push(b.voter.clone());
    }
    let mut reasons = Vec::new();
    for (session, voters) in &groups {
        if voters.len() < 2 {
            continue;
        }
        let mut names = voters.clone();
        names.sort();
        names.dedup();
        reasons.push(format!(
            "one session authored more than one ballot: {session} cast {} ({})",
            voters.len(),
            names.join(", ")
        ));
    }
    let mut authored = Vec::new();
    for c in candidates {
        let key = session_key(&c.harness, &c.nonce);
        if key.is_empty() || !groups.contains_key(&key) {
            continue;
        }
        authored.push(format!(
            "session {key} authored candidate {} and voted on {issue}",
            c.id
        ));
    }
    authored.sort();
    reasons.extend(authored);
    let bound = ballots
        .iter()
        .filter(|b| !session_key(&b.harness, &b.nonce).is_empty())
        .count();
    if bound >= 2 && bound == ballots.len() && groups.len() == 1 {
        let session = groups.keys().next().map(String::as_str).unwrap_or("");
        reasons.push(format!("all ballots trace to one session ({session})"));
    }
    if reasons.is_empty() {
        return None;
    }
    Some(format!(
        "refuses to settle {issue}: {}. Seats minted inside a session do not multiply its vote",
        reasons.join("; ")
    ))
}

/// The flag for a candidate whose session read another candidate first.
#[must_use]
pub fn blind_sentence(id: &str, session: &str, saw: &[String]) -> String {
    format!(
        "design {id} is not blind: session {session} read {} before filing it",
        saw.join(", ")
    )
}

/// Flags for candidates that were not filed blind. A candidate with an
/// empty read list is quiet.
#[must_use]
pub fn blind_lines(candidates: &[CandidateTrace]) -> Vec<String> {
    let mut out = Vec::new();
    for c in candidates {
        if c.saw.is_empty() {
            continue;
        }
        let session = session_key(&c.harness, &c.nonce);
        let session = if session.is_empty() {
            c.harness.clone()
        } else {
            session
        };
        out.push(blind_sentence(&c.id, &session, &c.saw));
    }
    out
}

/// Issue ids in `reads` (session key, issue) that are other candidates.
#[must_use]
pub fn reads_that_spoil(
    session: &str,
    reads: &[(String, String)],
    others: &[String],
) -> Vec<String> {
    let mut saw: Vec<String> = reads
        .iter()
        .filter(|(key, issue)| key == session && others.iter().any(|o| o == issue))
        .map(|(_, issue)| issue.clone())
        .collect();
    saw.sort();
    saw.dedup();
    saw
}

fn newest_sig<'a>(notes: &'a [String], voter: &str) -> Option<ballot_sig::SigNote> {
    notes.iter().find_map(|n| {
        let note = ballot_sig::parse_note(n)?;
        (note.voter == voter).then_some(note)
    })
}

/// Traces for the ballots on an issue, from its signature notes. A ballot
/// with no v2 note is unbound.
#[must_use]
pub fn ballot_traces(ballots: &[Value], notes: &[String]) -> Vec<BallotTrace> {
    ballots
        .iter()
        .filter_map(|b| {
            let voter = b["agent"].as_str()?.trim().to_string();
            if voter.is_empty() {
                return None;
            }
            let (harness, nonce) = newest_sig(notes, &voter)
                .map(|n| (n.harness.unwrap_or_default(), n.nonce.unwrap_or_default()))
                .unwrap_or_default();
            Some(BallotTrace {
                voter,
                harness,
                nonce,
            })
        })
        .collect()
}

fn verified_candidate(note: &str) -> Option<CandidateTrace> {
    let parsed = ballot_sig::parse_candidate(note)?;
    if !ballot_sig::candidate_verifies(&parsed) {
        return None;
    }
    Some(CandidateTrace {
        id: parsed.issue,
        harness: parsed.harness,
        nonce: parsed.nonce,
        saw: parsed.saw,
    })
}

/// Candidate bindings in `notes`, signatures checked.
#[must_use]
pub fn candidates_in(notes: &[String]) -> Vec<CandidateTrace> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for note in notes {
        if let Some(c) = verified_candidate(note) {
            if seen.insert(c.id.clone()) {
                out.push(c);
            }
        }
    }
    out
}

fn parent_of(v: &Value) -> Option<String> {
    v.get("parent")
        .or_else(|| v.get("PARENT"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn children_of(issue: &str) -> Vec<String> {
    let Ok(said) = crate::run_captured("vissue", &["children", issue, "--json"]) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<Value>>(&said.stdout)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect()
}

/// Notes that carry candidate bindings for `issue`: its own, its parent's,
/// and each design child's.
fn candidate_notes(issue: &str) -> Vec<String> {
    let mut notes = Vec::new();
    let mut ids = vec![issue.to_string()];
    if let Ok(v) = crate::issue_record(issue) {
        if let Some(p) = parent_of(&v) {
            ids.push(p);
        }
    }
    let mut extra = Vec::new();
    for id in &ids {
        extra.extend(children_of(id));
    }
    ids.extend(extra);
    ids.sort();
    ids.dedup();
    for id in ids {
        if let Ok(v) = crate::issue_record(&id) {
            notes.extend(ballot_sig::notes_of(&v));
        }
    }
    notes
}

/// Why this connection must not cast on `issue`, from logbook notes already
/// written. The session key is the connection. The voter name is not it.
#[must_use]
pub fn session_cast_refusal(
    issue: &str,
    key: &str,
    _voter: &str,
    notes: &[String],
) -> Option<String> {
    let key = key.trim();
    if key.is_empty() {
        return Some(format!(
            "a ballot on {issue} needs this connection's session. Nothing was cast"
        ));
    }
    for note in notes {
        if let Some(sig) = ballot_sig::parse_note(note) {
            let other = session_key(
                sig.harness.as_deref().unwrap_or(""),
                sig.nonce.as_deref().unwrap_or(""),
            );
            // The first ballot this session cast on the question stands.
            // A later one, including the same persona recast an hour on,
            // is a second voice. The voter name is not the session.
            if other == key {
                return Some(format!(
                    "this session already cast a ballot on {issue} ({key}). A seat argument does not make another voice. Nothing was cast"
                ));
            }
        }
        if let Some(candidate) = ballot_sig::parse_candidate(note) {
            let other = session_key(&candidate.harness, &candidate.nonce);
            if other == key {
                return Some(format!(
                    "this session authored candidate {} and its vote on {issue} is refused. A seat argument does not make another voice. Nothing was cast",
                    candidate.issue
                ));
            }
        }
    }
    None
}

static CAST: std::sync::Mutex<std::collections::BTreeSet<String>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// A claim on this connection's one ballot for an issue. Dropped without
/// [`SessionBallot::keep`], the claim is released so a failed cast can be
/// tried again. Kept, a later seat argument in this process is refused.
#[derive(Debug)]
pub struct SessionBallot {
    mark: String,
    kept: bool,
}

impl SessionBallot {
    /// The cast landed. A later call from this connection is refused.
    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for SessionBallot {
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        if let Ok(mut held) = CAST.lock() {
            held.remove(&self.mark);
        }
    }
}

/// Reserve this connection's ballot on `issue` for `voter`. The session is
/// [`ballot_sig::identity`], which a pinned MCP connection fixes for the
/// process. A later ballot from this session is refused, whoever it names,
/// including the same persona in a new process once the note is on the
/// tracker. This process also holds the one claim across the gap before
/// that note is written.
///
/// # Errors
///
/// This connection already cast on `issue`, this process already holds the
/// claim, or it authored a candidate tied to `issue`.
pub fn claim_session_ballot(issue: &str, voter: &str) -> Result<SessionBallot> {
    let id = ballot_sig::identity();
    let key = session_key(&id.harness, &id.nonce);
    let notes = candidate_notes(issue);
    let own = crate::issue_record(issue)
        .map(|v| ballot_sig::notes_of(&v))
        .unwrap_or_default();
    let mut all = notes;
    all.extend(own);
    if let Some(why) = session_cast_refusal(issue, &key, voter, &all) {
        bail!("{why}");
    }
    let mark = format!("{}\n{key}", issue.trim());
    let mut held = CAST.lock().unwrap_or_else(|e| e.into_inner());
    if !held.insert(mark.clone()) {
        bail!(
            "this session already cast a ballot on {issue} ({key}). A seat argument does not make another voice. Nothing was cast"
        );
    }
    drop(held);
    Ok(SessionBallot { mark, kept: false })
}

/// The refusal for `issue` as the counted ballots hold it now, or `None`
/// when one session's later ballots were left out of the tally, the
/// ballots are unbound, or the sessions are distinct and no author voted.
/// A second ballot is ignored by [`counted_ballots`], not settled as a
/// second voice.
#[must_use]
pub fn settle_refusal(issue: &str) -> Option<String> {
    let ballots = crate::ballot_rows(issue);
    if ballots.is_empty() {
        return None;
    }
    let notes = crate::issue_record(issue)
        .map(|v| ballot_sig::notes_of(&v))
        .unwrap_or_default();
    let traces = ballot_traces(&ballots, &notes);
    if traces
        .iter()
        .all(|t| session_key(&t.harness, &t.nonce).is_empty())
    {
        return None;
    }
    let candidates = candidates_in(&candidate_notes(issue));
    refusal(issue, &traces, &candidates)
}

/// Blind-design flags for the candidates tied to `issue`.
#[must_use]
pub fn blind_lines_for(issue: &str) -> Vec<String> {
    blind_lines(&candidates_in(&candidate_notes(issue)))
}

/// Sessions that filed a design candidate tied to `issue`. Their ballots
/// do not count, and a later cast is refused.
fn author_sessions(issue: &str) -> std::collections::BTreeSet<String> {
    candidates_in(&candidate_notes(issue))
        .into_iter()
        .map(|c| session_key(&c.harness, &c.nonce))
        .filter(|key| !key.is_empty())
        .collect()
}

/// One counted ballot, and the ballots the tally left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredBallot {
    pub voter: String,
    pub session: String,
    /// `same session` or `authored a candidate`.
    pub why: &'static str,
}

/// The ballots that count, and the ones a shared session or an author
/// session pushed out. `notes` are newest first, as the logbook stores
/// them. The oldest bound note for a session is the ballot that counts.
/// A voter with no session binding is kept: a tracker from before binding.
#[must_use]
pub fn tally_of(
    rows: &[Value],
    notes: &[String],
    authors: &std::collections::BTreeSet<String>,
) -> (Vec<Value>, Vec<IgnoredBallot>) {
    let sigs: Vec<ballot_sig::SigNote> = notes
        .iter()
        .filter_map(|n| ballot_sig::parse_note(n))
        .collect();
    let oldest: Vec<&ballot_sig::SigNote> = sigs.iter().rev().collect();
    let mut first: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut voter_session: BTreeMap<String, String> = BTreeMap::new();
    let mut session_order = Vec::new();
    for sig in &oldest {
        let key = session_key(
            sig.harness.as_deref().unwrap_or(""),
            sig.nonce.as_deref().unwrap_or(""),
        );
        if key.is_empty() {
            continue;
        }
        voter_session
            .entry(sig.voter.clone())
            .or_insert_with(|| key.clone());
        if first.contains_key(&key) {
            continue;
        }
        first.insert(key.clone(), (sig.voter.clone(), sig.choice.clone()));
        if !authors.contains(&key) {
            session_order.push(key);
        }
    }
    let mut counted = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for key in &session_order {
        let Some((voter, choice)) = first.get(key) else {
            continue;
        };
        if !seen.insert(voter.clone()) {
            continue;
        }
        let mut row = rows
            .iter()
            .find(|r| r["agent"].as_str().map(str::trim) == Some(voter.as_str()))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({ "agent": voter }));
        if let Some(obj) = row.as_object_mut() {
            obj.insert("choice".into(), Value::String(choice.clone()));
        }
        counted.push(row);
    }
    let mut ignored = Vec::new();
    for row in rows {
        let voter = row["agent"].as_str().unwrap_or("").trim();
        if voter.is_empty() || seen.contains(voter) {
            continue;
        }
        if let Some(key) = voter_session.get(voter) {
            let why = if authors.contains(key) {
                "authored a candidate"
            } else {
                "same session"
            };
            ignored.push(IgnoredBallot {
                voter: voter.to_string(),
                session: key.clone(),
                why,
            });
            continue;
        }
        counted.push(row.clone());
        seen.insert(voter.to_string());
    }
    (counted, ignored)
}

/// [`tally_of`] for the notes and candidate bindings on `issue`.
#[must_use]
pub fn counted_ballots(issue: &str, rows: &[Value]) -> Vec<Value> {
    let notes = crate::issue_record(issue)
        .map(|v| ballot_sig::notes_of(&v))
        .unwrap_or_default();
    tally_of(rows, &notes, &author_sessions(issue)).0
}

/// The tally a reader sees: the ballots that count, then the ones left
/// out because the session had already voted or had filed a candidate.
///
/// # Errors
///
/// The tracker did not list the ballots.
pub fn tally_report(issue: &str) -> Result<String> {
    let raw = crate::raw_ballot_rows(issue)?;
    let notes = crate::issue_record(issue)
        .map(|v| ballot_sig::notes_of(&v))
        .unwrap_or_default();
    let (counted, ignored) = tally_of(&raw, &notes, &author_sessions(issue));
    let mut out = String::new();
    let n = counted.len();
    let word = if n == 1 {
        "ballot counts"
    } else {
        "ballots count"
    };
    out.push_str(&format!("{issue}: {n} {word}\n"));
    for row in &counted {
        let agent = row["agent"].as_str().unwrap_or("").trim();
        let choice = row["choice"].as_str().unwrap_or("").trim();
        let raw_choice = raw
            .iter()
            .find(|r| r["agent"].as_str().map(str::trim) == Some(agent))
            .and_then(|r| r["choice"].as_str())
            .unwrap_or("");
        if !raw_choice.is_empty() && raw_choice != choice {
            out.push_str(&format!(
                "  {agent}  {choice}  (a later ballot on this session is ignored)\n"
            ));
        } else {
            out.push_str(&format!("  {agent}  {choice}\n"));
        }
    }
    if !ignored.is_empty() {
        out.push_str("ignored, one session is one ballot:\n");
        for item in &ignored {
            out.push_str(&format!(
                "  {}  {}  ({})\n",
                item.voter, item.session, item.why
            ));
        }
    }
    Ok(out)
}

fn reads_path() -> Option<PathBuf> {
    let root = std::env::var_os("VISSUE_ROOT").filter(|r| !r.is_empty())?;
    Some(PathBuf::from(root).join(READS_FILE))
}

/// Record that this session read tracker item `issue`. A recall and an
/// MCP read both come through here. Best effort.
pub fn note_tracker_read(issue: &str) {
    let issue = ballot_sig::field(issue);
    if issue == "-" {
        return;
    }
    let Some(path) = reads_path() else {
        return;
    };
    let id = ballot_sig::identity();
    let line = format!(
        "{} {} {} {issue}\n",
        crate::now_utc(),
        ballot_sig::field(&id.harness),
        ballot_sig::field(&id.nonce)
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// `(session key, issue)` rows of the read log.
#[must_use]
pub fn read_log(path: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let _time = parts.next()?;
            let harness = parts.next()?;
            let nonce = parts.next()?;
            let issue = parts.next()?;
            Some((session_key(harness, nonce), issue.to_string()))
        })
        .collect()
}

fn tag_list(v: &Value, key: &str) -> Vec<String> {
    match &v[key] {
        Value::Array(items) => items
            .iter()
            .filter_map(|t| t.as_str().map(str::to_string))
            .collect(),
        Value::String(s) => s.split(',').map(|t| t.trim().to_string()).collect(),
        _ => Vec::new(),
    }
}

fn issue_title_and_tags(issue: &str) -> (String, String) {
    let Ok(v) = crate::issue_record(issue) else {
        return (String::new(), String::new());
    };
    let title = v["title"].as_str().unwrap_or("").to_string();
    let mut tags = tag_list(&v, "tags");
    tags.extend(tag_list(&v, "org_tags"));
    (title, tags.join(","))
}

/// A design candidate: a `design:` or `candidate:` title, or the
/// `stage-design` tag.
#[must_use]
pub fn is_design_candidate(title: &str, tags: Option<&str>) -> bool {
    let t = title.trim().to_ascii_lowercase();
    t.starts_with("design:")
        || t.starts_with("candidate:")
        || tags
            .unwrap_or("")
            .split(',')
            .any(|tag| tag.trim() == "stage-design")
}

fn sibling_designs(parent: Option<&str>, self_id: &str) -> Vec<String> {
    let Some(parent) = parent.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    children_of(parent)
        .into_iter()
        .filter(|id| {
            if id == self_id {
                return false;
            }
            let (title, tags) = issue_title_and_tags(id);
            is_design_candidate(&title, Some(tags.as_str()))
        })
        .collect()
}

/// Bind a newly filed design candidate to this session, and flag it when
/// the session had already read another candidate. Other titles are left
/// alone.
///
/// # Errors
///
/// The seat key cannot be used, or the tracker refuses the note.
pub fn record_candidate(
    id: &str,
    title: &str,
    parent: Option<&str>,
    tags: Option<&str>,
) -> Result<String> {
    if !is_design_candidate(title, tags) {
        return Ok(String::new());
    }
    let ident = ballot_sig::identity();
    let key = session_key(&ident.harness, &ident.nonce);
    let others = sibling_designs(parent, id);
    let reads = reads_path().map(|p| read_log(&p)).unwrap_or_default();
    let saw = reads_that_spoil(&key, &reads, &others);
    let seat = crate::seat_name();
    let note = ballot_sig::sign_candidate(id, &seat, &ident, &saw)?;
    crate::run_captured("vissue", &["note", id, &note])?;
    let mut out = format!("candidate {id} bound to session {key}\n");
    if !saw.is_empty() {
        let line = blind_sentence(id, &key, &saw);
        let _ = crate::run_captured(
            "vissue",
            &["note", id, &format!("design-seen: {id} {}", saw.join(" "))],
        );
        out.push_str(&format!("blind design: {line}\n"));
    }
    Ok(out)
}

/// `cursor-agent -p` or `claude -p`, or `NAME -p` for another harness.
#[must_use]
pub fn harness_argv(harness: &str, prompt: &str) -> Vec<String> {
    match harness.trim() {
        "claude" => vec![
            "claude".into(),
            "-p".into(),
            prompt.into(),
            "--max-turns".into(),
            "6".into(),
            "--allowedTools".into(),
            "Bash(ljos:*)".into(),
            "Bash(vissue:*)".into(),
        ],
        "cursor" | "cursor-agent" | "" => {
            vec!["cursor-agent".into(), "-p".into(), prompt.into()]
        }
        other => vec![other.into(), "-p".into(), prompt.into()],
    }
}

fn harness_name(passed: Option<&str>) -> String {
    passed
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("LJOS_PANEL_HARNESS")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "cursor-agent".into())
}

/// What one spawned member runs. `LJOS_MEMBER_BIN` is the test stand-in.
#[must_use]
pub fn member_argv(harness: &str, prompt: &str) -> Vec<String> {
    if let Some(bin) = std::env::var("LJOS_MEMBER_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return vec![bin, "-p".into(), prompt.to_string()];
    }
    harness_argv(harness, prompt)
}

/// The only text a designer is given: this issue, not the other candidates.
#[must_use]
pub fn designer_prompt(issue: &str, n: usize, of: usize, brief: &str) -> String {
    format!(
        "You are designer {n} of {of} on {issue}, in your own harness session.\n\
         The brief below is the only description you are given. Do not read another design, \
         `vissue children`, `vissue vote`, or `ljos consensus`. Do not vote.\n\
         File one candidate and stop:\n\
         `ljos file \"design: TITLE\" --parent {issue} -t task --body \"YOUR DESIGN\"`\n\
         \n\
         == brief\n\
         {brief}\n"
    )
}

fn issue_brief(issue: &str) -> String {
    match crate::issue_record(issue) {
        Ok(v) => {
            let title = v["title"].as_str().unwrap_or("");
            let body = v["body"].as_str().unwrap_or("");
            let text = format!("{title}\n{body}");
            let text = text.trim();
            if text.is_empty() {
                format!("Issue {issue}.")
            } else {
                text.to_string()
            }
        }
        Err(_) => format!("Issue {issue}."),
    }
}

fn persona_names(issue: &str) -> Vec<String> {
    if let Ok(raw) = std::env::var("LJOS_PANEL_PERSONAS") {
        let names: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if !names.is_empty() {
            return names;
        }
    }
    if crate::bound_playbook(issue).is_none() {
        return Vec::new();
    }
    let all = crate::personas_from_pack().unwrap_or_default();
    crate::panel_personas(issue, &all)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn fresh_session(issue: &str, role: &str) -> String {
    use std::io::Read;
    let mut buf = [0u8; 4];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf));
    format!(
        "panel-{}-{}-{}",
        ballot_sig::field(issue),
        ballot_sig::field(role),
        hex::encode(buf)
    )
}

#[derive(Clone)]
struct Job {
    role: String,
    session: String,
    prompt: String,
    argv: Vec<String>,
}

fn plan(issue: &str, harness: &str, designers: usize, personas: &[String]) -> Vec<Job> {
    let brief = issue_brief(issue);
    let mut jobs = Vec::new();
    for n in 1..=designers {
        let role = format!("designer-{n}");
        let prompt = designer_prompt(issue, n, designers, &brief);
        jobs.push(Job {
            session: fresh_session(issue, &role),
            argv: member_argv(harness, &prompt),
            role,
            prompt,
        });
    }
    for name in personas {
        let prompt = match crate::brief(name, issue) {
            Ok(text) => crate::decision_member_task(&text, name, issue),
            Err(_) => format!(
                "You are {name}, in your own harness session. Cast exactly one ballot on {issue} \
                 and stop. Do not read `vissue vote {issue}` or another persona's ballot first.\n\
                 `ljos vote {issue} --for OPTION --expect OPTION --as {name} --confidence 0.7 --used none`.\n"
            ),
        };
        jobs.push(Job {
            session: fresh_session(issue, name),
            argv: member_argv(harness, &prompt),
            role: name.clone(),
            prompt,
        });
    }
    jobs
}

fn format_plan(issue: &str, harness: &str, jobs: &[Job], dry_run: bool) -> String {
    let mut out = format!(
        "panel run {issue}: {} harness sessions ({harness})\n",
        jobs.len()
    );
    for job in jobs {
        let kind = if job.role.starts_with("designer-") {
            "designer, brief only"
        } else {
            "persona"
        };
        out.push_str(&format!("  {} session {} {kind}\n", job.role, job.session));
        if dry_run {
            out.push_str(&format!(
                "  argv: {}\n",
                job.argv[..job.argv.len().min(2)].join(" ")
            ));
            out.push_str(&job.prompt);
            if !job.prompt.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

fn run_job(job: &Job) -> String {
    if job.argv.is_empty() {
        return format!("{} did not start: no argv\n", job.role);
    }
    let mut cmd = Command::new(&job.argv[0]);
    cmd.args(&job.argv[1..])
        .env("LJOS_HARNESS_SESSION", &job.session)
        .env("LJOS_SESSION_ID", &job.session)
        .env("SEAT_SESSION_ID", &job.session)
        .env("LJOS_PANEL_CHILD", "1")
        .env("LJOS_SEAT", &job.role)
        .env("VISSUE_AGENT", &job.role)
        .env_remove("LJOS_PROCESS_NONCE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match cmd.output() {
        Ok(out) => {
            let code = out.status.code().unwrap_or(1);
            format!("{} session {} exited {code}\n", job.role, job.session)
        }
        Err(e) => format!("{} session {} did not start: {e}\n", job.role, job.session),
    }
}

/// Spawn each designer and each persona as its own harness session.
/// Designers receive only the brief. `dry_run` prints the plan and starts
/// nothing. The harness is `cursor-agent` or `claude` (`--harness`, or
/// `LJOS_PANEL_HARNESS`, default `cursor-agent`).
///
/// # Errors
///
/// There is nobody to spawn.
pub fn panel_run(
    issue: &str,
    harness: Option<&str>,
    designers: usize,
    dry_run: bool,
) -> Result<String> {
    if std::env::var_os("LJOS_PANEL_CHILD").is_some() {
        return Ok(format!(
            "panel run: {issue} is already inside a panel member; nothing spawned\n"
        ));
    }
    let harness = harness_name(harness);
    let personas = persona_names(issue);
    if designers == 0 && personas.is_empty() {
        anyhow::bail!(
            "panel run: {issue} has no designers and no personas; pass --designers N or seat a persona"
        );
    }
    let jobs = plan(issue, &harness, designers, &personas);
    let mut out = format_plan(issue, &harness, &jobs, dry_run);
    if dry_run {
        out.push_str("dry run: nothing spawned\n");
        return Ok(out);
    }
    let at_once = crate::panel_concurrency().max(1);
    for chunk in jobs.chunks(at_once) {
        let mut handles = Vec::new();
        for job in chunk {
            let job = job.clone();
            handles.push(std::thread::spawn(move || run_job(&job)));
        }
        for handle in handles {
            match handle.join() {
                Ok(line) => out.push_str(&line),
                Err(_) => out.push_str("a panel member thread stopped\n"),
            }
        }
    }
    Ok(out)
}

/// The issue body a designer would be handed, for a test that checks the
/// prompt carries no other candidate.
#[must_use]
pub fn designer_prompt_for(issue: &str, n: usize, of: usize) -> String {
    designer_prompt(issue, n, of, &issue_brief(issue))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat(voter: &str, harness: &str) -> BallotTrace {
        BallotTrace {
            voter: voter.into(),
            harness: harness.into(),
            nonce: format!("n-{voter}"),
        }
    }

    #[test]
    fn one_connection_cannot_cast_again_by_naming_another_seat() {
        let notes = [
            "ballot-sig: alice ship ab cd sess-shared-0001 n-alice".to_string(),
            "candidate-sig: demo-d001 alice sess-author-0001 n-author - ab cd".to_string(),
        ];
        let again = session_cast_refusal("demo-ind1", "sess-shared-0001", "bea", &notes).unwrap();
        assert!(
            again.contains("already cast") && again.contains("seat argument"),
            "{again}"
        );
        let recast =
            session_cast_refusal("demo-ind1", "sess-shared-0001", "alice", &notes).unwrap();
        assert!(
            recast.contains("already cast") && recast.contains("Nothing was cast"),
            "{recast}"
        );
        let author =
            session_cast_refusal("demo-vote", "sess-author-0001", "alice", &notes).unwrap();
        assert!(author.contains("authored candidate demo-d001"), "{author}");
        assert!(session_cast_refusal("demo-ind1", "sess-bob-0001", "bob", &notes).is_none());
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VISSUE_ROOT", dir.path());
        std::env::set_var("LJOS_HARNESS_SESSION", "sess-live-0001");
        std::env::set_var("LJOS_PROCESS_NONCE", "nonce-live-0001");
        let first = claim_session_ballot("probe-seat-1", "saddle").unwrap();
        let second = claim_session_ballot("probe-seat-1", "other").unwrap_err();
        assert!(second.to_string().contains("already cast"), "{second:#}");
        first.keep();
        std::env::remove_var("VISSUE_ROOT");
        std::env::remove_var("LJOS_HARNESS_SESSION");
        std::env::remove_var("LJOS_PROCESS_NONCE");
    }

    #[test]
    fn five_personas_one_sand_session_count_as_the_first_ballot() {
        let session = "sand-subagent-fcdb9c57-9988-7d91-4311-9fc5e42f7529";
        let personas = ["academic", "transit", "safety", "cat-owner", "finance"];
        let mut notes = Vec::new();
        // Newest first, as the logbook stores them: the 11:46 recast, then
        // the 10:35 ballots, academic's first cast last.
        for persona in personas.iter().rev() {
            notes.push(format!(
                "ballot-sig: {persona} bus ab cd {session} n-{persona}-2"
            ));
        }
        for persona in personas.iter().rev() {
            notes.push(format!(
                "ballot-sig: {persona} rail ab cd {session} n-{persona}-1"
            ));
        }
        let rows: Vec<Value> = personas
            .iter()
            .map(|persona| serde_json::json!({"agent": persona, "choice": "bus"}))
            .collect();
        let (counted, ignored) = tally_of(&rows, &notes, &std::collections::BTreeSet::new());
        assert_eq!(counted.len(), 1, "{counted:?}");
        assert_eq!(counted[0]["agent"], "academic");
        assert_eq!(counted[0]["choice"], "rail");
        let ignored_names: Vec<&str> = ignored.iter().map(|item| item.voter.as_str()).collect();
        assert_eq!(
            ignored_names,
            vec!["transit", "safety", "cat-owner", "finance"]
        );
        assert!(ignored
            .iter()
            .all(|item| item.session == session && item.why == "same session"));
        let mut authors = std::collections::BTreeSet::new();
        authors.insert(session.to_string());
        let (none, dropped) = tally_of(&rows, &notes, &authors);
        assert!(none.is_empty(), "{none:?}");
        assert_eq!(dropped.len(), 5, "{dropped:?}");
        assert!(dropped
            .iter()
            .all(|item| item.why == "authored a candidate"));
    }

    #[test]
    fn one_session_of_n_seats_is_refused_with_the_whole_sentence() {
        let ballots = vec![
            seat("carol", "sess-shared-0001"),
            seat("alice", "sess-shared-0001"),
            seat("bob", "sess-shared-0001"),
        ];
        // Distinct nonces, one harness session: a worker that spawned one
        // process per seat still counts as one session.
        let msg = refusal("demo-ind1", &ballots, &[]).unwrap();
        assert_eq!(
            msg,
            "refuses to settle demo-ind1: one session authored more than one ballot: \
             sess-shared-0001 cast 3 (alice, bob, carol); all ballots trace to one session \
             (sess-shared-0001). Seats minted inside a session do not multiply its vote"
        );
    }

    #[test]
    fn n_sessions_settle() {
        let ballots = vec![
            seat("alice", "sess-alice-0001"),
            seat("bob", "sess-bob-0001"),
            seat("carol", "sess-carol-0001"),
        ];
        assert_eq!(refusal("demo-ind2", &ballots, &[]), None);
    }

    #[test]
    fn an_author_who_votes_is_refused_even_when_the_other_ballots_are_not_theirs() {
        let ballots = vec![
            seat("alice", "sess-alice-0001"),
            seat("bob", "sess-bob-0001"),
        ];
        let candidates = vec![CandidateTrace {
            id: "demo-d001".into(),
            harness: "sess-alice-0001".into(),
            nonce: "n-alice".into(),
            saw: vec![],
        }];
        let msg = refusal("demo-vote", &ballots, &candidates).unwrap();
        assert_eq!(
            msg,
            "refuses to settle demo-vote: session sess-alice-0001 authored candidate demo-d001 \
             and voted on demo-vote. Seats minted inside a session do not multiply its vote"
        );
    }

    #[test]
    fn a_session_that_read_another_candidate_is_flagged() {
        let reads = vec![
            ("sess-1".into(), "demo-b".into()),
            ("sess-2".into(), "demo-a".into()),
        ];
        let saw = reads_that_spoil("sess-1", &reads, &["demo-a".into(), "demo-b".into()]);
        assert_eq!(saw, vec!["demo-b".to_string()]);
        let line = blind_sentence("demo-a", "sess-1", &saw);
        assert_eq!(
            line,
            "design demo-a is not blind: session sess-1 read demo-b before filing it"
        );
        let quiet = reads_that_spoil("sess-2", &reads, &["demo-b".into()]);
        assert!(quiet.is_empty(), "{quiet:?}");
    }

    #[test]
    fn a_designer_prompt_is_only_the_brief() {
        let prompt = designer_prompt("demo-1", 1, 3, "Ship the cache.\nKeep the ledger.");
        assert!(prompt.contains("== brief"), "{prompt}");
        assert!(prompt.contains("Ship the cache."), "{prompt}");
        assert!(prompt.contains("Do not read another design"), "{prompt}");
        assert!(prompt.contains("Do not vote"), "{prompt}");
        assert!(!prompt.contains("OTHER-CANDIDATE-BODY"), "{prompt}");
        let argv = harness_argv("cursor-agent", "hello");
        assert_eq!(argv, vec!["cursor-agent", "-p", "hello"]);
        let claude = harness_argv("claude", "hello");
        assert_eq!(&claude[..3], ["claude", "-p", "hello"]);
    }

    #[test]
    fn unbound_ballots_are_left_alone() {
        let ballots = vec![
            BallotTrace {
                voter: "alice".into(),
                harness: String::new(),
                nonce: String::new(),
            },
            BallotTrace {
                voter: "bob".into(),
                harness: "-".into(),
                nonce: "-".into(),
            },
        ];
        assert_eq!(refusal("old-tracker", &ballots, &[]), None);
    }

    #[test]
    fn the_same_nonce_without_a_harness_is_one_session() {
        let ballots = vec![
            BallotTrace {
                voter: "alice".into(),
                harness: "-".into(),
                nonce: "same-nonce".into(),
            },
            BallotTrace {
                voter: "bob".into(),
                harness: "-".into(),
                nonce: "same-nonce".into(),
            },
        ];
        let msg = refusal("demo-n", &ballots, &[]).unwrap();
        assert!(
            msg.contains(
                "one session authored more than one ballot: nonce:same-nonce cast 2 (alice, bob)"
            ),
            "{msg}"
        );
        assert!(
            msg.contains("all ballots trace to one session (nonce:same-nonce)"),
            "{msg}"
        );
    }
}
