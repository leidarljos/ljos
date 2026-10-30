//! A ballot records the persona, the seat or session that cast it, and
//! the parent session. Ballots that share a session id, a parent, a
//! calling shell or a harness root are all recorded. They count as one
//! voice, the latest when they disagree. Consensus names the ballots it
//! folded and why. A subagent of that root counts with its parent.
//! A session that filed a design candidate does not also vote.
//!
//! A design candidate whose session had already read another candidate is
//! flagged: the round was not blind.

use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::ballot_sig;
use crate::proc_tree;

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

/// Why a settle is refused, or `None` when no author of a candidate also
/// voted. Persona ballots that share a parent are counted, not refused.
/// The sentence is the whole refusal.
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
    if authored.is_empty() {
        return None;
    }
    Some(format!(
        "refuses to settle {issue}: {}",
        authored.join("; ")
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

fn newest_sig(notes: &[String], voter: &str) -> Option<ballot_sig::SigNote> {
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

fn family_issue_ids(issue: &str) -> Vec<String> {
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
    ids
}

fn notes_on(issue: &str) -> Vec<String> {
    crate::issue_record(issue)
        .map(|v| ballot_sig::notes_of(&v))
        .unwrap_or_default()
}

/// Notes that carry candidate bindings for `issue`: its own, its parent's,
/// and each design child's.
fn candidate_notes(issue: &str) -> Vec<String> {
    let mut notes = Vec::new();
    for id in family_issue_ids(issue) {
        notes.extend(notes_on(&id));
    }
    notes
}

fn family_notes(issue: &str) -> Vec<(String, String)> {
    family_issue_ids(issue)
        .into_iter()
        .filter(|id| id != issue)
        .flat_map(|id| {
            notes_on(&id)
                .into_iter()
                .map(move |note| (id.clone(), note))
        })
        .collect()
}

/// Per `(issue, voter)`, where in `notes` (newest first) its newest
/// `ballot-withdrawn:` note sits. A note with no issue keeps the legacy
/// meaning: it applies to the issue whose logbook holds it (`home`).
fn withdrawal_cuts(notes: &[String], home: &str) -> BTreeMap<(String, String), usize> {
    let mut cuts = BTreeMap::new();
    for (at, note) in notes.iter().enumerate() {
        if let Some(w) = ballot_sig::parse_withdrawn(note) {
            let issue = w.issue.unwrap_or_else(|| home.to_string());
            cuts.entry((issue, w.voter)).or_insert(at);
        }
    }
    cuts
}

fn withdrawn_at(
    cuts: &BTreeMap<(String, String), usize>,
    issue: &str,
    voter: &str,
) -> Option<usize> {
    if issue.is_empty() {
        let field = ballot_sig::field(voter);
        return cuts
            .iter()
            .find_map(|((_issue, v), at)| (v == voter || v == &field).then_some(*at));
    }
    let named = (issue.to_string(), voter.to_string());
    let field = (issue.to_string(), ballot_sig::field(voter));
    cuts.get(&named).or_else(|| cuts.get(&field)).copied()
}

fn same_voter(signed: &str, voter: &str) -> bool {
    signed == voter.trim() || signed == ballot_sig::field(voter)
}

fn matching_ballot_issue(
    issue: &str,
    voter: &str,
    own: &[String],
    family: &[(String, String)],
) -> Option<String> {
    if voter.trim().is_empty() {
        return None;
    }
    let mut by_issue: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, note) in family {
        by_issue.entry(id.clone()).or_default().push(note.clone());
    }
    for (home, notes) in std::iter::once((issue.to_string(), own.to_vec())).chain(by_issue) {
        let cuts = withdrawal_cuts(&notes, &home);
        for (at, note) in notes.iter().enumerate() {
            let Some(sig) = ballot_sig::parse_note(note) else {
                continue;
            };
            if !ballot_sig::verifies(issue, &sig) {
                continue;
            }
            if !same_voter(&sig.voter, voter) {
                continue;
            }
            if withdrawn_at(&cuts, issue, &sig.voter).is_some_and(|cut| cut < at) {
                continue;
            }
            return Some(home);
        }
    }
    None
}

/// How `voter` replaces the ballot it holds on `issue`.
#[must_use]
pub fn replace_hint(issue: &str, voter: &str) -> String {
    let voter = ballot_sig::field(voter);
    format!(
        "To replace it, withdraw it and cast again: `ljos vote {issue} --withdraw --as {voter}` (ljos_vote with withdraw), then `ljos vote {issue} --for OPTION --used ... --as {voter}`."
    )
}

/// Why this connection must not cast on `issue`. `own` is that issue's
/// logbook. `family` is the parent and the design children. A signature
/// counts from either, but only when it verifies for `issue`: a ballot on
/// a sibling is another question. A withdrawal is read from whichever
/// logbook carries that signature.
/// candidate counts from either. The session key is the connection. The
/// voter name is not it.
#[must_use]
fn session_cast_refusal_on(
    issue: &str,
    key: &str,
    voter: &str,
    own: &[String],
    family: &[(String, String)],
) -> Option<String> {
    let key = key.trim();
    if key.is_empty() {
        return Some(format!(
            "a ballot on {issue} needs this connection's session. Nothing was cast"
        ));
    }
    let mut family_by_issue: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, note) in family {
        family_by_issue
            .entry(id.clone())
            .or_default()
            .push(note.clone());
    }
    let signed = own
        .iter()
        .enumerate()
        .map(|(at, note)| (issue, Some(at), note));
    let elsewhere = family_by_issue.iter().flat_map(|(home, notes)| {
        notes
            .iter()
            .enumerate()
            .map(move |(at, note)| (home.as_str(), Some(at), note))
    });
    for (home, at, note) in signed.chain(elsewhere) {
        let Some(sig) = ballot_sig::parse_note(note) else {
            continue;
        };
        if !ballot_sig::verifies(issue, &sig) {
            continue;
        }
        let other = session_key(
            sig.harness.as_deref().unwrap_or(""),
            sig.nonce.as_deref().unwrap_or(""),
        );
        // Another persona from this session is recorded. The same voter
        // replaces a ballot by withdrawing it first.
        if other != key || !same_voter(&sig.voter, voter) {
            continue;
        }
        let session = if sig.seat_only() {
            format!("{key}, {}", ballot_sig::unverified_sentence(key))
        } else {
            key.to_string()
        };
        let cuts = if home == issue {
            withdrawal_cuts(own, issue)
        } else {
            let notes = family_by_issue.get(home).cloned().unwrap_or_default();
            withdrawal_cuts(&notes, home)
        };
        if at.is_some_and(|at| withdrawn_at(&cuts, issue, &sig.voter).is_some_and(|cut| cut < at)) {
            continue;
        }
        return Some(format!(
            "this session already cast a ballot on {issue} ({session}): {}'s, for {}. Nothing was cast. {}",
            sig.voter,
            sig.choice,
            replace_hint(issue, voter)
        ));
    }
    for note in own.iter().chain(family.iter().map(|(_, note)| note)) {
        let Some(candidate) = ballot_sig::parse_candidate(note) else {
            continue;
        };
        let other = session_key(&candidate.harness, &candidate.nonce);
        if other == key {
            return Some(format!(
                "this session authored candidate {} and its vote on {issue} is refused. Nothing was cast.",
                candidate.issue
            ));
        }
    }
    None
}

/// Why this connection must not cast on `issue`. `own` is that issue's
/// logbook. `family` is the parent and the design children.
#[must_use]
pub fn session_cast_refusal(
    issue: &str,
    key: &str,
    voter: &str,
    own: &[String],
    family: &[(String, String)],
) -> Option<String> {
    let family: Vec<(String, String)> = family
        .iter()
        .filter(|(_, note)| !own.contains(note))
        .cloned()
        .collect();
    session_cast_refusal_on(issue, key, voter, own, &family)
}

/// The issue in this issue family where `voter` still has this session's
/// ballot on `issue`, if any.
#[must_use]
pub fn standing_ballot_issue(issue: &str, voter: &str) -> Option<String> {
    let own = notes_on(issue);
    let family = family_notes(issue);
    matching_ballot_issue(issue, voter, &own, &family)
}

static CAST: std::sync::Mutex<std::collections::BTreeSet<String>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// The processes a cast on this machine runs under, each a
/// [`proc_tree::token`]: the shell that ran this `ljos`, and the harness
/// whose hook saw its line. The environment names the session; it cannot
/// cheaply change either of these.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct CastTree {
    shell: Option<String>,
    harness: Option<String>,
}

impl CastTree {
    /// This process's tree. Only the `ljos` executable has one, matched by
    /// its resolved path, so a link or another argv0 still binds. The MCP
    /// server answers a parent and its subagents alike, and names each
    /// call's session itself.
    fn here() -> Self {
        let mine = proc_tree::exe(std::process::id());
        if !mine
            .as_ref()
            .is_some_and(|path| proc_tree::is_ljos_exe(path))
        {
            return Self::default();
        }
        let chain = proc_tree::ancestors();
        Self {
            shell: proc_tree::cast_shell(&chain, mine.as_deref()).map(proc_tree::token),
            harness: crate::subagent::taken_harness().map(|p| proc_tree::token(&p)),
        }
    }
}

/// One cast recorded under `ballot-trees/ISSUE` in the runtime directory:
/// `SHELL HARNESS SESSION STATE`, tab-separated, `-` for a process not
/// known. The state is `cast`, or the token of the process that holds the
/// claim until its cast lands.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeRow {
    shell: String,
    harness: String,
    session: String,
    state: String,
}

const CAST_STATE: &str = "cast";

impl TreeRow {
    fn parse(line: &str) -> Option<Self> {
        let mut f = line.split('\t');
        let row = Self {
            shell: f.next()?.to_string(),
            harness: f.next()?.to_string(),
            session: f.next()?.to_string(),
            state: f.next()?.to_string(),
        };
        f.next().is_none().then_some(row)
    }

    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.shell, self.harness, self.session, self.state
        )
    }

    /// Whether the row can still bind a cast: a process it names runs,
    /// and its claim was kept or its holder is still at it.
    fn live(&self) -> bool {
        let runs = |t: &str| t != "-" && proc_tree::token_alive(t);
        (runs(&self.shell) || runs(&self.harness))
            && (self.state == CAST_STATE || runs(&self.state))
    }

    /// The earlier voice when a cast as `key` under `tree` repeats this
    /// row. A shell runs one agent's commands, so any other session from
    /// it is that voice. A runner row binds another root session. A
    /// subagent of the same root is not bound here, and the parent fold
    /// counts it with that root. The pair is `(why, session key)`:
    /// `shell` or `harness`, and this row's session.
    fn binds(&self, key: &str, tree: &CastTree) -> Option<(&'static str, String)> {
        let same = |mine: &Option<String>, theirs: &str| mine.as_deref() == Some(theirs);
        if same(&tree.shell, &self.shell) && self.session != key {
            Some(("shell", self.session.clone()))
        } else if same(&tree.harness, &self.harness)
            && session_root(&self.session) != session_root(key)
        {
            Some(("harness", self.session.clone()))
        } else {
            None
        }
    }
}

fn session_root(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

fn voice_of(
    why: &str,
    session: &str,
    claimed: &ballot_sig::SessionIdentity,
) -> ballot_sig::VoiceBind {
    let (harness, nonce) = session
        .strip_prefix("nonce:")
        .map(|nonce| ("-".to_string(), nonce.to_string()))
        .unwrap_or_else(|| (session.to_string(), "-".to_string()));
    ballot_sig::VoiceBind {
        harness,
        nonce,
        claimed_harness: claimed.harness.clone(),
        claimed_nonce: claimed.nonce.clone(),
        why: why.to_string(),
    }
}

fn bound_of(
    rows: &[TreeRow],
    key: &str,
    tree: &CastTree,
    claimed: &ballot_sig::SessionIdentity,
) -> Option<ballot_sig::VoiceBind> {
    let (why, session) = rows.iter().find_map(|row| row.binds(key, tree))?;
    Some(voice_of(why, &session, claimed))
}

/// The voice `claimed` folds into on `issue`, when a live row shares its
/// shell or its harness root. `None` when this process has no tree, or
/// no row matches.
pub(crate) fn peek_bound(
    issue: &str,
    claimed: &ballot_sig::SessionIdentity,
) -> Option<ballot_sig::VoiceBind> {
    let tree = CastTree::here();
    if tree.shell.is_none() && tree.harness.is_none() {
        return None;
    }
    let key = session_key(&claimed.harness, &claimed.nonce);
    let text = std::fs::read_to_string(tree_path(issue)).unwrap_or_default();
    let rows: Vec<TreeRow> = text
        .lines()
        .filter_map(TreeRow::parse)
        .filter(TreeRow::live)
        .collect();
    bound_of(&rows, &key, &tree, claimed)
}

/// The rows of `issue` on this tracker: another tracker's issue of the
/// same name is another question.
fn tree_path(issue: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let tracker = std::env::var_os("VISSUE_ROOT").unwrap_or_default();
    let tracker = hex::encode(Sha256::digest(tracker.as_encoded_bytes()));
    crate::runtime_dir()
        .join("ballot-trees")
        .join(&tracker[..16])
        .join(crate::subagent::token(issue))
}

/// Rewrite the rows of `path` under its lock. `edit` sees the rows still
/// live and may refuse; what it leaves is written back.
fn edit_rows<R>(path: &Path, edit: impl FnOnce(&mut Vec<TreeRow>) -> Result<R>) -> Result<R> {
    let _durable = crate::bookkeep::durable_write();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _lock = crate::CommitLock::acquire(&path.with_extension("lock"));
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut rows: Vec<TreeRow> = text
        .lines()
        .filter_map(TreeRow::parse)
        .filter(TreeRow::live)
        .collect();
    let out = edit(&mut rows)?;
    let body: String = rows.iter().map(|r| r.line() + "\n").collect();
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    Ok(out)
}

/// This process's row on an issue, held until the cast lands or fails.
#[derive(Debug)]
struct TreeHold {
    path: PathBuf,
    row: TreeRow,
}

/// Record this process's row. A shared shell or harness does not refuse
/// the cast. It binds the ballot to the earlier row's session, which is
/// the voice both ballots count as. The row stores that voice.
fn hold_tree(
    issue: &str,
    claimed: &ballot_sig::SessionIdentity,
    tree: &CastTree,
) -> Result<Option<TreeHold>> {
    if tree.shell.is_none() && tree.harness.is_none() {
        ballot_sig::clear_voice_bind();
        return Ok(None);
    }
    let key = session_key(&claimed.harness, &claimed.nonce);
    let me =
        proc_tree::of(std::process::id()).map_or_else(|| "-".to_string(), |p| proc_tree::token(&p));
    let path = tree_path(issue);
    let row = edit_rows(&path, |rows| {
        let bound = bound_of(rows, &key, tree, claimed);
        let session = bound
            .as_ref()
            .map(|bind| session_key(&bind.harness, &bind.nonce))
            .filter(|session| !session.is_empty())
            .unwrap_or_else(|| key.clone());
        if let Some(bound) = bound {
            ballot_sig::set_voice_bind(bound);
        } else {
            ballot_sig::clear_voice_bind();
        }
        let row = TreeRow {
            shell: tree.shell.clone().unwrap_or_else(|| "-".into()),
            harness: tree.harness.clone().unwrap_or_else(|| "-".into()),
            session,
            state: me,
        };
        rows.push(row.clone());
        Ok(row)
    })?;
    Ok(Some(TreeHold { path, row }))
}

impl TreeHold {
    /// Mark the row cast, or drop it when the cast did not land.
    fn settle(&self, kept: bool) {
        let _ = edit_rows(&self.path, |rows| {
            if let Some(at) = rows.iter().position(|r| *r == self.row) {
                if kept {
                    rows[at].state = CAST_STATE.into();
                } else {
                    rows.remove(at);
                }
            }
            Ok(())
        });
    }
}

/// A claim on one voter's ballot for an issue. Dropped without
/// [`SessionBallot::keep`], the claim is released so a failed cast can be
/// tried again. Kept, that same voter in this process is refused.
#[derive(Debug)]
pub struct SessionBallot {
    mark: String,
    kept: bool,
    tree: Option<TreeHold>,
}

impl SessionBallot {
    /// The cast landed. That same voter in this process is refused later.
    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for SessionBallot {
    fn drop(&mut self) {
        if let Some(tree) = &self.tree {
            tree.settle(self.kept);
        }
        // `keep` drops this value before the note is signed. The voice
        // bind has to outlive that drop; `sign_ballot` clears it.
        if !self.kept {
            ballot_sig::end_cast();
        }
        if self.kept {
            return;
        }
        if let Ok(mut held) = CAST.lock() {
            held.remove(&self.mark);
        }
    }
}

/// The choice `voter`'s row in the drawer holds on `issue`, if any.
fn held_choice(issue: &str, voter: &str) -> Option<String> {
    crate::raw_ballot_rows(issue)
        .ok()?
        .iter()
        .find(|row| {
            row["agent"]
                .as_str()
                .is_some_and(|agent| same_voter(agent.trim(), voter))
        })
        .map(|row| row["choice"].as_str().unwrap_or("").trim().to_string())
}

/// Reserve `voter`'s ballot on `issue`. Another persona from this session
/// is recorded with it. The same voter withdraws before casting again.
/// This process holds that voter's claim across the gap before the note
/// is written.
///
/// # Errors
///
/// This voter already holds a ballot on `issue`, this process already
/// holds that claim, or it authored a candidate tied to `issue`.
pub fn claim_session_ballot(issue: &str, voter: &str) -> Result<SessionBallot> {
    ballot_sig::begin_cast(issue);
    let claimed = ballot_sig::claimed_identity();
    let id = ballot_sig::identity();
    let key = session_key(&id.harness, &id.nonce);
    let own = notes_on(issue);
    let family = family_notes(issue);
    if let Some(why) = session_cast_refusal_on(issue, &key, voter, &own, &family) {
        ballot_sig::end_cast();
        bail!("{why}");
    }
    if let Some(choice) = held_choice(issue, voter) {
        ballot_sig::end_cast();
        bail!(
            "{voter} already holds a ballot on {issue} for {choice}. Nothing was cast. {}",
            replace_hint(issue, voter)
        );
    }
    let mark = format!("{}\n{voter}", issue.trim());
    let mut held = CAST.lock().unwrap_or_else(|e| e.into_inner());
    if !held.insert(mark.clone()) {
        ballot_sig::end_cast();
        bail!(
            "{voter} already cast a ballot on {issue} in this process. Nothing was cast. {}",
            replace_hint(issue, voter)
        );
    }
    drop(held);
    let mut ballot = SessionBallot {
        mark,
        kept: false,
        tree: None,
    };
    ballot.tree = hold_tree(issue, &claimed, &CastTree::here())?;
    Ok(ballot)
}

/// The refusal for `issue` when a session that filed a candidate also
/// voted, or `None` when the ballots can be weighed. Persona ballots that
/// share a parent are not a refusal.
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
    /// `authored a candidate`.
    pub why: &'static str,
    /// Only the seat named that session.
    pub unverified: bool,
}

/// The ballots that count, the ones an author session pushed out, and
/// the lines that say which ballots were folded into one voice.
struct Tally {
    counted: Vec<Value>,
    ignored: Vec<IgnoredBallot>,
    folds: Vec<String>,
}

/// One voter's sessions, from the signatures still in force. `newest` is
/// the index of that voter's latest standing note. Notes are newest first.
struct VoiceLink {
    keys: Vec<String>,
    parents: Vec<String>,
    /// `(why, voice session)` from a tree-bound note. `why` is `shell` or
    /// `harness`.
    links: Vec<(String, String)>,
    newest: usize,
}

fn voice_links(sigs: &[(bool, ballot_sig::SigNote)]) -> BTreeMap<String, VoiceLink> {
    let mut links = BTreeMap::new();
    for (at, (standing, sig)) in sigs.iter().enumerate() {
        if !standing {
            continue;
        }
        let harness = sig.harness.clone().unwrap_or_default();
        let nonce = sig.nonce.clone().unwrap_or_default();
        let key = session_key(&harness, &nonce);
        let parent = ballot_sig::parent_session(&harness);
        let link = links.entry(sig.voter.clone()).or_insert_with(|| VoiceLink {
            keys: Vec::new(),
            parents: Vec::new(),
            links: Vec::new(),
            newest: at,
        });
        if !key.is_empty() && !link.keys.contains(&key) {
            link.keys.push(key);
        }
        if !parent.is_empty() && !link.parents.contains(&parent) {
            link.parents.push(parent);
        }
        if let Some(why) = sig
            .link
            .clone()
            .filter(|why| why == "shell" || why == "harness")
        {
            let voice = sig.harness.clone().unwrap_or_default();
            let pair = (why, voice);
            if !pair.1.is_empty() && !link.links.contains(&pair) {
                link.links.push(pair);
            }
        }
    }
    links
}

fn find_voice(uf: &mut [usize], mut i: usize) -> usize {
    while uf[i] != i {
        uf[i] = uf[uf[i]];
        i = uf[i];
    }
    i
}

fn unite_voice(uf: &mut [usize], a: usize, b: usize) {
    let a = find_voice(uf, a);
    let b = find_voice(uf, b);
    if a != b {
        uf[b] = a;
    }
}

fn remember_link(at: &mut BTreeMap<String, usize>, uf: &mut [usize], i: usize, name: &str) {
    if let Some(j) = at.get(name).copied() {
        unite_voice(uf, i, j);
    } else {
        at.insert(name.to_string(), i);
    }
}

/// Why these ballots are one voice: one shell, one harness root, one
/// session id, or one parent.
fn fold_why(members: &[usize], rows: &[Value], links: &BTreeMap<String, VoiceLink>) -> String {
    let mut keys = std::collections::BTreeSet::new();
    let mut parents = std::collections::BTreeSet::new();
    let mut shells = std::collections::BTreeSet::new();
    let mut harnesses = std::collections::BTreeSet::new();
    for &i in members {
        let voter = rows[i]["agent"].as_str().unwrap_or("").trim();
        if let Some(link) = links.get(voter) {
            keys.extend(link.keys.iter().cloned());
            parents.extend(link.parents.iter().cloned());
            for (why, voice) in &link.links {
                if why == "shell" {
                    shells.insert(voice.clone());
                } else if why == "harness" {
                    harnesses.insert(voice.clone());
                }
            }
        }
    }
    if !shells.is_empty() {
        return format!(
            "same shell {}",
            shells.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    if !harnesses.is_empty() {
        return format!(
            "same harness {}",
            harnesses.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    if keys.len() == 1 {
        let key = keys.iter().next().map(String::as_str).unwrap_or("");
        format!("same session {key}")
    } else if !parents.is_empty() {
        format!(
            "same parent {}",
            parents.into_iter().collect::<Vec<_>>().join(", ")
        )
    } else {
        format!(
            "same session {}",
            keys.into_iter().collect::<Vec<_>>().join(", ")
        )
    }
}

/// Keep one row per session. A session is a shared session id or a shared
/// parent, whichever seat name or key the ballots carry. The kept row is
/// the latest ballot. The others stay recorded and are named here.
fn fold_one_voice(
    rows: Vec<Value>,
    links: &BTreeMap<String, VoiceLink>,
) -> (Vec<Value>, Vec<String>) {
    let n = rows.len();
    let mut uf: Vec<usize> = (0..n).collect();
    let mut key_at = BTreeMap::new();
    let mut parent_at = BTreeMap::new();
    for (i, row) in rows.iter().enumerate() {
        let voter = row["agent"].as_str().unwrap_or("").trim();
        let Some(link) = links.get(voter) else {
            continue;
        };
        for key in &link.keys {
            remember_link(&mut key_at, &mut uf, i, key);
        }
        for parent in &link.parents {
            remember_link(&mut parent_at, &mut uf, i, parent);
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        groups.entry(find_voice(&mut uf, i)).or_default().push(i);
    }
    let mut drop = std::collections::BTreeSet::new();
    let mut lines = Vec::new();
    for members in groups.values() {
        if members.len() < 2 {
            continue;
        }
        let keeper = members
            .iter()
            .copied()
            .min_by_key(|&i| {
                let voter = rows[i]["agent"].as_str().unwrap_or("").trim();
                links
                    .get(voter)
                    .map(|link| link.newest)
                    .unwrap_or(usize::MAX)
            })
            .unwrap_or(members[0]);
        let mut names = Vec::new();
        for &i in members {
            let voter = rows[i]["agent"].as_str().unwrap_or("").trim().to_string();
            if i != keeper {
                drop.insert(i);
            }
            names.push(voter);
        }
        names.sort();
        let counted = rows[keeper]["agent"].as_str().unwrap_or("").trim();
        let why = fold_why(members, &rows, links);
        lines.push(format!(
            "folded into one voice ({why}): {}",
            names.join(", ")
        ));
        lines.push(format!("counted {counted}"));
    }
    let counted = rows
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !drop.contains(i))
        .map(|(_, row)| row)
        .collect();
    (counted, lines)
}

/// The ballots that count, and the ones an author session pushed out.
/// `notes` are newest first, as the logbook stores them. Ballots that
/// share a session id or a parent count as one voice, the latest when
/// they disagree. A voter's counted choice is the oldest signature still
/// in force, and only while the drawer has a choice that signature set
/// names. A voter who withdrew and cast nothing since is dropped. A voter
/// with no session binding is kept.
#[must_use]
pub fn tally_of(
    issue: &str,
    rows: &[Value],
    notes: &[String],
    authors: &std::collections::BTreeSet<String>,
) -> (Vec<Value>, Vec<IgnoredBallot>) {
    let tally = tally_voices(issue, rows, notes, authors);
    (tally.counted, tally.ignored)
}

fn tally_voices(
    issue: &str,
    rows: &[Value],
    notes: &[String],
    authors: &std::collections::BTreeSet<String>,
) -> Tally {
    let cuts = withdrawal_cuts(notes, issue);
    let sigs: Vec<(bool, ballot_sig::SigNote)> = notes
        .iter()
        .enumerate()
        .filter_map(|(at, n)| {
            let sig = ballot_sig::parse_note(n)?;
            let standing = withdrawn_at(&cuts, issue, &sig.voter).is_none_or(|cut| at < cut);
            Some((standing, sig))
        })
        .collect();
    // Oldest binding first: session, oldest standing choice, seat-only.
    let mut bound: BTreeMap<String, (String, Option<String>, bool)> = BTreeMap::new();
    for (standing, sig) in sigs.iter().rev() {
        let key = session_key(
            sig.harness.as_deref().unwrap_or(""),
            sig.nonce.as_deref().unwrap_or(""),
        );
        if key.is_empty() {
            continue;
        }
        let entry = bound
            .entry(sig.voter.clone())
            .or_insert_with(|| (key, None, sig.seat_only()));
        if entry.1.is_none() && *standing {
            entry.1 = Some(sig.choice.clone());
        }
    }
    let mut counted = Vec::new();
    let mut ignored = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        let voter = row["agent"].as_str().unwrap_or("").trim();
        if voter.is_empty() || !seen.insert(voter.to_string()) {
            continue;
        }
        let any_standing = sigs
            .iter()
            .any(|(standing, sig)| *standing && sig.voter == voter);
        if withdrawn_at(&cuts, issue, voter).is_some() && !any_standing {
            continue;
        }
        if let Some((key, oldest, seat_only)) = bound.get(voter) {
            if authors.contains(key) {
                ignored.push(IgnoredBallot {
                    voter: voter.to_string(),
                    session: key.clone(),
                    why: "authored a candidate",
                    unverified: *seat_only,
                });
                continue;
            }
            let Some(choice) = oldest.clone() else {
                continue;
            };
            let drawer = row["choice"].as_str().unwrap_or("").trim();
            let signed = sigs
                .iter()
                .any(|(standing, sig)| *standing && sig.voter == voter && sig.choice == drawer);
            if !signed {
                continue;
            }
            let mut row = row.clone();
            if let Some(obj) = row.as_object_mut() {
                obj.insert("choice".into(), Value::String(choice));
            }
            counted.push(row);
            continue;
        }
        counted.push(row.clone());
    }
    let links = voice_links(&sigs);
    let (counted, folds) = fold_one_voice(counted, &links);
    Tally {
        counted,
        ignored,
        folds,
    }
}

/// [`tally_of`] for the notes and candidate bindings on `issue`.
#[must_use]
pub fn counted_ballots(issue: &str, rows: &[Value]) -> Vec<Value> {
    let notes = crate::issue_record(issue)
        .map(|v| ballot_sig::notes_of(&v))
        .unwrap_or_default();
    tally_of(issue, rows, &notes, &author_sessions(issue)).0
}

/// The seat of each voter whose first bound ballot only that seat named
/// the session of. `notes` are newest first.
fn seat_only_voters(notes: &[String]) -> BTreeMap<String, String> {
    let mut first = BTreeMap::new();
    for sig in notes.iter().rev().filter_map(|n| ballot_sig::parse_note(n)) {
        if sig.harness.is_none() {
            continue;
        }
        let seat = sig
            .seat_only()
            .then(|| sig.harness.clone().unwrap_or_default());
        first.entry(sig.voter).or_insert(seat);
    }
    first
        .into_iter()
        .filter_map(|(voter, seat)| Some((voter, seat?)))
        .collect()
}

/// The tally a reader sees: the ballots that count, then the ones left
/// out because the session had already voted or had filed a candidate.
/// A session only the seat named is said to be unverified.
///
/// # Errors
///
/// The tracker did not list the ballots.
pub fn tally_report(issue: &str) -> Result<String> {
    let raw = crate::raw_ballot_rows(issue)?;
    let notes = crate::issue_record(issue)
        .map(|v| ballot_sig::notes_of(&v))
        .unwrap_or_default();
    let tally = tally_voices(issue, &raw, &notes, &author_sessions(issue));
    let counted = &tally.counted;
    let ignored = &tally.ignored;
    let seat_only = seat_only_voters(&notes);
    let mut unverified = std::collections::BTreeSet::new();
    let mut out = String::new();
    let n = counted.len();
    let word = if n == 1 {
        "ballot counts"
    } else {
        "ballots count"
    };
    out.push_str(&format!("{issue}: {n} {word}\n"));
    for row in counted {
        let agent = row["agent"].as_str().unwrap_or("").trim();
        let choice = row["choice"].as_str().unwrap_or("").trim();
        let raw_choice = raw
            .iter()
            .find(|r| r["agent"].as_str().map(str::trim) == Some(agent))
            .and_then(|r| r["choice"].as_str())
            .unwrap_or("");
        let mut tags = Vec::new();
        if !raw_choice.is_empty() && raw_choice != choice {
            tags.push("a later ballot on this session is ignored");
        }
        if let Some(seat) = seat_only.get(agent) {
            unverified.insert(seat.clone());
            tags.push("unverified session");
        }
        if tags.is_empty() {
            out.push_str(&format!("  {agent}  {choice}\n"));
        } else {
            out.push_str(&format!("  {agent}  {choice}  ({})\n", tags.join("; ")));
        }
    }
    if !ignored.is_empty() {
        out.push_str("ignored, the session filed a candidate:\n");
        for item in ignored {
            let unverified_tag = if item.unverified {
                unverified.insert(item.session.clone());
                ", unverified"
            } else {
                ""
            };
            out.push_str(&format!(
                "  {}  {}  ({}{unverified_tag})\n",
                item.voter, item.session, item.why
            ));
        }
    }
    for seat in &unverified {
        out.push_str(&format!("{}\n", ballot_sig::unverified_sentence(seat)));
    }
    out.push_str(&provenance_text(&notes, &raw));
    for line in &tally.folds {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// Caster and parent for `voter`. A `ballot-from:` note wins. Otherwise
/// the signature's session is the caster and its parent is the parent.
fn provenance_of(notes: &[String], voter: &str) -> Option<(String, String)> {
    if let Some((caster, persona)) = ballot_sig::parse_caster_persona_voter(voter) {
        for note in notes {
            if let Some((named, from, parent)) = ballot_sig::parse_from(note) {
                if named == persona && from == caster && parent != "-" {
                    return Some((from, parent));
                }
            }
        }
    }
    for note in notes {
        if let Some((persona, caster, parent)) = ballot_sig::parse_from(note) {
            if persona == voter && parent != "-" {
                return Some((caster, parent));
            }
        }
    }
    let mut found = None;
    for note in notes.iter().rev() {
        let Some(sig) = ballot_sig::parse_note(note) else {
            continue;
        };
        if sig.voter != voter {
            continue;
        }
        if found.is_some() {
            continue;
        }
        let harness = sig.harness.clone().unwrap_or_default();
        let parent = ballot_sig::parent_session(&harness);
        if parent.is_empty() {
            continue;
        }
        let caster = if harness.is_empty() {
            parent.clone()
        } else {
            harness
        };
        found = Some((caster, parent));
    }
    found
}

/// Lines naming each counted ballot's caster and parent, then each parent
/// that more than one ballot shares.
#[must_use]
pub fn provenance_text(notes: &[String], rows: &[Value]) -> String {
    let mut lines = Vec::new();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        let voter = row["agent"].as_str().unwrap_or("").trim();
        if voter.is_empty() {
            continue;
        }
        let Some((caster, parent)) = provenance_of(notes, voter) else {
            continue;
        };
        let label = ballot_sig::parse_caster_persona_voter(voter)
            .map(|(_, persona)| persona)
            .unwrap_or_else(|| voter.to_string());
        lines.push(format!("  {label} cast by {caster}, parent {parent}"));
        groups.entry(parent).or_default().push(label);
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::from("provenance:\n");
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    for (parent, mut names) in groups {
        if names.len() < 2 {
            continue;
        }
        names.sort();
        names.dedup();
        out.push_str(&format!("shared parent {parent}: {}\n", names.join(", ")));
    }
    out
}

/// Provenance for every recorded ballot on `issue`, then the ballots
/// consensus folded into one voice and why.
#[must_use]
pub fn panel_provenance(issue: &str) -> String {
    let notes = crate::issue_record(issue)
        .map(|v| ballot_sig::notes_of(&v))
        .unwrap_or_default();
    let raw = crate::raw_ballot_rows(issue).unwrap_or_default();
    let mut out = provenance_text(&notes, &raw);
    let tally = tally_voices(issue, &raw, &notes, &author_sessions(issue));
    for line in tally.folds {
        out.push_str(&line);
        out.push('\n');
    }
    out
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

/// Headless argv for `harness` on `prompt`.
#[must_use]
pub fn harness_argv(harness: &str, prompt: &str) -> Vec<String> {
    let trimmed = harness.trim();
    if trimmed.is_empty() || trimmed == "cursor" || trimmed == "cursor-agent" {
        return vec!["cursor-agent".into(), "-p".into(), prompt.into()];
    }
    if let Some(template) = crate::panel_runner::template_for_runner(trimmed, false, false) {
        let vars = crate::tools::Vars::default()
            .with("prompt", prompt)
            .with("cwd", ".")
            .with("persona", "member");
        return crate::tools::fill(&template, &vars);
    }
    vec![trimmed.into(), "-p".into(), prompt.into()]
}

/// What one spawned member runs. `LJOS_MEMBER_BIN` is the test stand-in.
#[must_use]
pub fn member_argv(harness: &str, prompt: &str) -> Vec<String> {
    crate::panel_runner::run_argv_for(harness, prompt, "member")
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
            argv: crate::panel_runner::run_argv_for(harness, &prompt, &role),
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
            argv: crate::panel_runner::run_argv_for(harness, &prompt, name),
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

/// One session run to its end. A session that ran and failed is an error
/// like one that never started: neither cast a ballot.
fn run_job(job: &Job) -> Result<String> {
    if job.argv.is_empty() {
        anyhow::bail!("{} did not start: no argv", job.role);
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
        Ok(out) if out.status.success() => {
            Ok(format!("{} session {} exited 0\n", job.role, job.session))
        }
        Ok(out) => anyhow::bail!(
            "{} session {} exited {}",
            job.role,
            job.session,
            out.status.code().unwrap_or(1)
        ),
        Err(e) => anyhow::bail!(
            "{} session {} did not start ({}): {e}",
            job.role,
            job.session,
            job.argv[0]
        ),
    }
}

/// Spawn each designer and each persona as its own harness session.
/// Designers receive only the brief. `dry_run` prints the plan and starts
/// nothing. Picks from the runner CLIs installed on `PATH`, or uses a
/// configured command template (`--harness`, `LJOS_PANEL_HARNESS`,
/// `LJOS_PANEL_COMMAND`, `harnesses.toml`, or `ljos.toml`).
///
/// # Errors
///
/// There is nobody to spawn, or no runner CLI is available.
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
    let personas = persona_names(issue);
    if designers == 0 && personas.is_empty() {
        anyhow::bail!(
            "panel run: {issue} has no designers and no personas; pass --designers N or seat a persona"
        );
    }
    let harness =
        crate::panel_runner::resolve_panel_run_harness_with(harness, dry_run, &crate::on_path)?;
    let jobs = plan(issue, &harness, designers, &personas);
    let mut out = format_plan(issue, &harness, &jobs, dry_run);
    if dry_run {
        out.push_str("dry run: nothing spawned\n");
        return Ok(out);
    }
    let at_once = crate::panel_concurrency().max(1);
    let mut failed = 0;
    for chunk in jobs.chunks(at_once) {
        let mut handles = Vec::new();
        for job in chunk {
            let job = job.clone();
            handles.push(std::thread::spawn(move || run_job(&job)));
        }
        for handle in handles {
            match handle.join() {
                Ok(Ok(line)) => out.push_str(&line),
                Ok(Err(err)) => {
                    failed += 1;
                    out.push_str(&format!("{err:#}\n"));
                }
                Err(_) => {
                    failed += 1;
                    out.push_str("a panel member thread stopped\n");
                }
            }
        }
    }
    let ran = jobs.len() - failed;
    if failed > 0 {
        anyhow::bail!(
            "{out}panel run {issue}: opened {ran} of {} sessions",
            jobs.len()
        );
    }
    out.push_str(&format!(
        "panel run {issue}: opened {ran} of {} sessions\n",
        jobs.len()
    ));
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

    #[test]
    fn a_shell_casts_as_one_session_and_a_harness_as_one_root() {
        let row = TreeRow {
            shell: "b:10:1".into(),
            harness: "b:20:2".into(),
            session: "sess-root:agent-a".into(),
            state: CAST_STATE.into(),
        };
        assert_eq!(TreeRow::parse(&row.line()), Some(row.clone()));
        let tree = |shell: &str, harness: &str| CastTree {
            shell: Some(shell.into()),
            harness: Some(harness.into()),
        };
        assert_eq!(
            row.binds("sess-root:agent-b", &tree("b:10:1", "b:30:3")),
            Some(("shell", "sess-root:agent-a".to_string())),
            "the same shell binds the later session to the earlier voice"
        );
        assert_eq!(
            row.binds("sess-root:agent-a", &tree("b:10:1", "b:20:2")),
            None,
            "the same session is the cast gate's to judge"
        );
        assert_eq!(
            row.binds("sess-root:agent-b", &tree("b:11:1", "b:20:2")),
            None,
            "a subagent of the same root counts with its parent"
        );
        assert_eq!(
            row.binds("sess-other", &tree("b:11:1", "b:20:2")),
            Some(("harness", "sess-root:agent-a".to_string())),
            "another root session on the same harness is the earlier voice"
        );
        assert_eq!(row.binds("sess-other", &CastTree::default()), None);
        assert!(!row.live(), "a row naming no running process is dropped");
    }

    #[test]
    fn a_linked_ballot_folds_into_the_earlier_voice() {
        let k = ed25519_dalek::SigningKey::from_bytes(&[8; 32]);
        let first = ballot_sig::sign_bound(&k, "demo-tree", "alice", "ship", "sess-a", "-");
        let second = ballot_sig::sign_linked(
            &k,
            "demo-tree",
            "bea",
            "hold",
            ballot_sig::LinkedFields {
                harness: "sess-a",
                nonce: "-",
                claimed_harness: "sess-b",
                claimed_nonce: "-",
                link: "shell",
            },
        );
        let rows = [
            serde_json::json!({"agent": "alice", "choice": "ship"}),
            serde_json::json!({"agent": "bea", "choice": "hold"}),
        ];
        let tally = tally_voices(
            "demo-tree",
            &rows,
            &[second, first],
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(tally.counted.len(), 1, "{:?}", tally.counted);
        assert!(
            tally
                .folds
                .iter()
                .any(|line| line.contains("folded into one voice (same shell sess-a)")),
            "{:?}",
            tally.folds
        );
        let apart = [
            ballot_sig::sign_bound(&k, "demo-tree", "carol", "ship", "sess-c", "-"),
            ballot_sig::sign_bound(&k, "demo-tree", "dora", "ship", "sess-d", "-"),
        ];
        let rows = [
            serde_json::json!({"agent": "carol", "choice": "ship"}),
            serde_json::json!({"agent": "dora", "choice": "ship"}),
        ];
        let (counted, _) = tally_of(
            "demo-tree",
            &rows,
            &apart,
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(counted.len(), 2, "{counted:?}");
    }

    fn seat(voter: &str, harness: &str) -> BallotTrace {
        BallotTrace {
            voter: voter.into(),
            harness: harness.into(),
            nonce: format!("n-{voter}"),
        }
    }

    #[test]
    fn one_connection_cannot_cast_again_by_naming_another_seat() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[3; 32]);
        let ballot = ballot_sig::sign_bound(
            &key,
            "demo-ind1",
            "alice",
            "ship",
            "sess-shared-0001",
            "n-alice",
        );
        let own = [ballot];
        let family = [(
            "demo-vote".to_string(),
            "candidate-sig: demo-d001 alice sess-author-0001 n-author - ab cd".to_string(),
        )];
        assert!(
            session_cast_refusal("demo-ind1", "sess-shared-0001", "bea", &own, &family).is_none(),
            "another persona from this session is recorded"
        );
        let recast =
            session_cast_refusal("demo-ind1", "sess-shared-0001", "alice", &own, &family).unwrap();
        assert!(
            recast.contains("already cast") && recast.contains("Nothing was cast"),
            "{recast}"
        );
        let author =
            session_cast_refusal("demo-vote", "sess-author-0001", "alice", &[], &family).unwrap();
        assert!(author.contains("authored candidate demo-d001"), "{author}");
        let own_candidate =
            ["candidate-sig: demo-d001 alice sess-author-0001 n-author - ab cd".to_string()];
        let on_own = session_cast_refusal(
            "demo-vote",
            "sess-author-0001",
            "alice",
            &own_candidate,
            &[],
        )
        .unwrap();
        assert!(on_own.contains("authored candidate demo-d001"), "{on_own}");
        assert!(session_cast_refusal("demo-ind1", "sess-bob-0001", "bob", &own, &family).is_none());
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VISSUE_ROOT", dir.path());
        std::env::set_var("LJOS_HARNESS_SESSION", "sess-live-0001");
        std::env::set_var("LJOS_PROCESS_NONCE", "nonce-live-0001");
        let first = claim_session_ballot("probe-seat-1", "saddle").unwrap();
        let second = claim_session_ballot("probe-seat-1", "other");
        assert!(second.is_ok(), "{second:?}");
        let again = claim_session_ballot("probe-seat-1", "saddle").unwrap_err();
        assert!(again.to_string().contains("already cast"), "{again:#}");
        first.keep();
        std::env::remove_var("VISSUE_ROOT");
        std::env::remove_var("LJOS_HARNESS_SESSION");
        std::env::remove_var("LJOS_PROCESS_NONCE");
    }

    #[test]
    fn a_signature_for_this_issue_on_its_family_still_counts() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[4; 32]);
        let on_b = ballot_sig::sign_bound(&key, "demo-b", "saddle", "ship", "sess-one", "-");
        let stray = [("parent-b".to_string(), on_b.clone())];
        assert!(
            session_cast_refusal("demo-b", "sess-one", "bea", &[], &stray).is_none(),
            "another persona is recorded beside saddle"
        );
        let withdrawn = ballot_sig::withdrawn_line("saddle", "sess-one", "-");
        let own = [withdrawn, on_b.clone()];
        assert!(
            session_cast_refusal("demo-b", "sess-one", "saddle", &own, &stray).is_none(),
            "the issue's own copy in the family is read once, with its withdrawal"
        );
    }

    #[test]
    fn a_parent_cut_for_a_child_issue_is_read_on_the_parent_index() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[6; 32]);
        let issue = "child-a";
        let parent = "parent-1";
        let own = [ballot_sig::sign_bound(
            &key, issue, "bea", "ship", "sess-bea", "-",
        )];
        let sig = ballot_sig::sign_bound(&key, issue, "saddle", "ship", "sess-saddle", "-");
        let cut = ballot_sig::withdrawn_line_on("saddle", "sess-saddle", "-", Some(issue));
        let family = [(parent.to_string(), cut), (parent.to_string(), sig)];
        assert_eq!(
            session_cast_refusal(issue, "sess-saddle", "saddle", &own, &family),
            None,
            "a parent cut for child-a must not be indexed against child-a's own notes"
        );
    }

    #[test]
    fn a_choice_with_a_space_is_still_this_sessions_one_ballot() {
        // The panel eval's unscored case on 7e21d3c: session spaced-1 cast
        // `beside binary` as seat-a, then `xdg home`, and both landed.
        let k = ed25519_dalek::SigningKey::from_bytes(&[9; 32]);
        let first =
            ballot_sig::sign_bound(&k, "panel-spcd", "seat-a", "beside binary", "spaced-1", "-");
        let again = session_cast_refusal(
            "panel-spcd",
            "spaced-1",
            "seat-a",
            std::slice::from_ref(&first),
            &[],
        )
        .expect("a second cast from spaced-1 was let through");
        assert!(
            again.contains("already cast") && again.contains("Nothing was cast"),
            "{again}"
        );
        assert!(session_cast_refusal(
            "panel-spcd",
            "other-sess",
            "seat-b",
            std::slice::from_ref(&first),
            &[],
        )
        .is_none());
        // The drawer already has both, as the eval left it. The oldest
        // signature is the choice that counts.
        let later = ballot_sig::sign_bound(&k, "panel-spcd", "seat-a", "xdg home", "spaced-1", "-");
        let rows = [serde_json::json!({"agent": "seat-a", "choice": "xdg home"})];
        let (counted, _) = tally_of(
            "panel-spcd",
            &rows,
            &[later, first],
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(counted.len(), 1, "{counted:?}");
        assert_eq!(counted[0]["choice"], "beside binary");
        // A note that does not parse, and does not verify, does not bind.
        let odd = "ballot-sig: seat-a beside binary ab cd spaced-1 -".to_string();
        assert!(session_cast_refusal("panel-spcd", "spaced-1", "seat-a", &[odd], &[]).is_none());
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
        let tally = tally_voices(
            "travel-rx1d",
            &rows,
            &notes,
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(tally.counted.len(), 1, "{:?}", tally.counted);
        assert_eq!(tally.counted[0]["agent"], "finance");
        assert_eq!(tally.counted[0]["choice"], "rail");
        assert!(tally.ignored.is_empty(), "{:?}", tally.ignored);
        let said = provenance_text(&notes, &rows);
        assert!(
            said.contains(&format!("shared parent {session}:")),
            "{said}"
        );
        let folded = tally.folds.join("\n");
        assert!(
            folded.contains(&format!("same session {session}"))
                && personas.iter().all(|persona| folded.contains(persona)),
            "{folded}"
        );
        let mut authors = std::collections::BTreeSet::new();
        authors.insert(session.to_string());
        let (none, dropped) = tally_of("travel-rx1d", &rows, &notes, &authors);
        assert!(none.is_empty(), "{none:?}");
        assert_eq!(dropped.len(), 5, "{dropped:?}");
        assert!(dropped
            .iter()
            .all(|item| item.why == "authored a candidate"));
    }

    fn note_of(voter: &str, choice: &str, harness: &str) -> String {
        format!("ballot-sig: {voter} {choice} ab cd {harness} -")
    }

    #[test]
    fn one_session_that_switches_seats_counts_the_latest_ballot() {
        let session = "sess-switch-0001";
        let notes = vec![
            note_of("bob", "hold", session),
            note_of("alice", "ship", session),
        ];
        let rows = [
            serde_json::json!({"agent": "alice", "choice": "ship"}),
            serde_json::json!({"agent": "bob", "choice": "hold"}),
        ];
        let tally = tally_voices(
            "demo-switch",
            &rows,
            &notes,
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(tally.counted.len(), 1, "{:?}", tally.counted);
        assert_eq!(tally.counted[0]["agent"], "bob");
        assert_eq!(tally.counted[0]["choice"], "hold");
        let folded = tally.folds.join("\n");
        assert!(
            folded.contains(&format!("same session {session}"))
                && folded.contains("alice")
                && folded.contains("bob")
                && folded.contains("counted bob"),
            "{folded}"
        );
    }

    #[test]
    fn three_subagents_of_one_parent_are_one_voice() {
        let parent = "sess-parent-0001";
        let notes = vec![
            note_of("cee", "ship", &format!("{parent}:agent-c")),
            note_of("bee", "hold", &format!("{parent}:agent-b")),
            note_of("aye", "ship", &format!("{parent}:agent-a")),
        ];
        let rows = [
            serde_json::json!({"agent": "aye", "choice": "ship"}),
            serde_json::json!({"agent": "bee", "choice": "hold"}),
            serde_json::json!({"agent": "cee", "choice": "ship"}),
        ];
        let tally = tally_voices(
            "demo-three",
            &rows,
            &notes,
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(tally.counted.len(), 1, "{:?}", tally.counted);
        assert_eq!(tally.counted[0]["agent"], "cee");
        assert_eq!(tally.counted[0]["choice"], "ship");
        let folded = tally.folds.join("\n");
        assert!(
            folded.contains(&format!("same parent {parent}"))
                && folded.contains("aye")
                && folded.contains("bee")
                && folded.contains("cee"),
            "{folded}"
        );
    }

    #[test]
    fn two_sessions_count_as_two_voices() {
        let notes = vec![
            note_of("bob", "hold", "sess-bob-0001"),
            note_of("alice", "ship", "sess-alice-0001"),
        ];
        let rows = [
            serde_json::json!({"agent": "alice", "choice": "ship"}),
            serde_json::json!({"agent": "bob", "choice": "hold"}),
        ];
        let tally = tally_voices(
            "demo-two",
            &rows,
            &notes,
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(tally.counted.len(), 2, "{:?}", tally.counted);
        assert!(tally.folds.is_empty(), "{:?}", tally.folds);
    }

    #[test]
    fn one_session_of_n_seats_is_weighed() {
        let ballots = vec![
            seat("carol", "sess-shared-0001"),
            seat("alice", "sess-shared-0001"),
            seat("bob", "sess-shared-0001"),
        ];
        assert_eq!(refusal("demo-ind1", &ballots, &[]), None);
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
             and voted on demo-vote"
                .to_string()
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
        assert_eq!(session_key("-", "same-nonce"), "nonce:same-nonce");
        assert_eq!(refusal("demo-n", &ballots, &[]), None);
    }

    #[test]
    fn a_seat_only_session_keeps_the_seat_as_its_key() {
        assert_eq!(
            session_key("ljosbot", ballot_sig::SEAT_NONCE),
            session_key("ljosbot", "-")
        );
        let key = ed25519_dalek::SigningKey::from_bytes(&[9; 32]);
        // A ballot the seat cast before the marker still holds its session.
        let old = ballot_sig::sign_bound(&key, "demo-s", "alice", "ship", "ljosbot", "-");
        assert!(
            session_cast_refusal("demo-s", "ljosbot", "bea", std::slice::from_ref(&old), &[])
                .is_none()
        );
        let again = session_cast_refusal("demo-s", "ljosbot", "alice", &[old], &[]).unwrap();
        assert!(again.contains("already cast"), "{again}");
        assert!(!again.contains("unverified"), "{again}");
        let marked = ballot_sig::sign_bound(
            &key,
            "demo-s",
            "alice",
            "ship",
            "ljosbot",
            ballot_sig::SEAT_NONCE,
        );
        assert!(session_cast_refusal(
            "demo-s",
            "ljosbot",
            "bea",
            std::slice::from_ref(&marked),
            &[],
        )
        .is_none());
        let again = session_cast_refusal("demo-s", "ljosbot", "alice", &[marked], &[]).unwrap();
        assert!(
            again.contains("already cast")
                && again.contains(&ballot_sig::unverified_sentence("ljosbot")),
            "{again}"
        );
    }

    #[test]
    fn a_siblings_signature_does_not_block_this_issue() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let sibling =
            ballot_sig::sign_bound(&key, "rgpot-9ot3", "saddle", "ship", "sess-saddle", "-");
        assert!(session_cast_refusal(
            "eOn-1stn",
            "sess-saddle",
            "saddle",
            &[],
            &[("rgpot-9ot3".to_string(), sibling.clone())],
        )
        .is_none());
        assert!(
            session_cast_refusal(
                "eOn-1stn",
                "sess-saddle",
                "saddle",
                std::slice::from_ref(&sibling),
                &[],
            )
            .is_none(),
            "a signature for another issue does not verify here"
        );
        let here = ballot_sig::sign_bound(&key, "eOn-1stn", "saddle", "ship", "sess-saddle", "-");
        let blocked = session_cast_refusal(
            "eOn-1stn",
            "sess-saddle",
            "saddle",
            std::slice::from_ref(&here),
            &[],
        )
        .unwrap();
        assert!(
            blocked.contains("already cast") && blocked.contains("Nothing was cast"),
            "{blocked}"
        );
    }

    #[test]
    fn a_withdrawn_ballot_drops_out_of_the_tally_until_its_voter_casts_again() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[8; 32]);
        let sig = ballot_sig::sign_bound(&key, "demo-w", "reviewer", "ship", "sess-r", "-");
        let authors = std::collections::BTreeSet::new();
        let (gone, _) = tally_of("demo-w", &[], std::slice::from_ref(&sig), &authors);
        assert!(gone.is_empty(), "{gone:?}");
        let rows = [serde_json::json!({"agent": "reviewer", "choice": "ship"})];
        let note = ballot_sig::withdrawn_line("reviewer", "sess-r", "-");
        let taken_back = [note.clone(), sig.clone()];
        let (counted, ignored) = tally_of("demo-w", &rows, &taken_back, &authors);
        assert!(counted.is_empty(), "{counted:?}");
        assert!(ignored.is_empty(), "{ignored:?}");
        assert!(
            session_cast_refusal("demo-w", "sess-r", "editor", &taken_back, &[]).is_none(),
            "another persona is recorded after a withdrawal"
        );
        // The voter that withdrew casts again, and that ballot counts.
        assert_eq!(
            session_cast_refusal("demo-w", "sess-r", "reviewer", &taken_back, &[]),
            None
        );
        let recast = ballot_sig::sign_bound(&key, "demo-w", "reviewer", "hold", "sess-r", "-");
        let notes = [recast, note, sig];
        let rows = [serde_json::json!({"agent": "reviewer", "choice": "hold"})];
        let (counted, _) = tally_of("demo-w", &rows, &notes, &authors);
        assert_eq!(counted.len(), 1, "{counted:?}");
        assert_eq!(counted[0]["choice"], "hold");
        // A third cast with no withdrawal says how to replace the ballot.
        let again = session_cast_refusal("demo-w", "sess-r", "reviewer", &notes, &[]).unwrap();
        assert!(
            again.contains("already cast a ballot on demo-w (sess-r): reviewer's, for hold.")
                && again.contains("`ljos vote demo-w --withdraw --as reviewer`")
                && again.contains("Nothing was cast"),
            "{again}"
        );
    }

    #[test]
    fn a_withdrawn_seat_ballot_does_not_decide_a_push() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[4; 32]);
        let ship = ballot_sig::sign_bound(&key, "cite-1", "seat-a", "ship", "sess-a", "-");
        let hold = ballot_sig::sign_bound(&key, "cite-1", "seat-b", "hold", "sess-b", "-");
        let withdrawn = ballot_sig::withdrawn_line("seat-a", "sess-a", "-");
        let rows = [
            serde_json::json!({"agent": "seat-a", "choice": "ship"}),
            serde_json::json!({"agent": "seat-b", "choice": "hold"}),
        ];
        let notes = vec![withdrawn, hold, ship];
        let authors = std::collections::BTreeSet::new();
        let (counted, _) = tally_of("cite-1", &rows, &notes, &authors);
        assert_eq!(counted.len(), 1, "{counted:?}");
        assert_eq!(counted[0]["agent"], "seat-b");
        assert_eq!(counted[0]["choice"], "hold");
        let personas = std::collections::BTreeSet::new();
        let (words, json) = crate::push_cite_reading_signed(
            "cite-1",
            &counted,
            &personas,
            "seat-c",
            Ok(Default::default()),
        );
        assert!(words.contains("decides hold"), "{words}");
        assert!(!words.contains("split"), "{words}");
        assert_eq!(json["push_cite"]["stands"].as_bool(), Some(true), "{json}");
        let (only, _) = tally_of("cite-1", &rows[..1], &notes, &authors);
        assert!(only.is_empty(), "{only:?}");
        let (words, json) = crate::push_cite_reading_signed(
            "cite-1",
            &only,
            &personas,
            "seat-c",
            Ok(Default::default()),
        );
        assert!(
            words.contains("does not stand") && words.contains("no seat voted"),
            "{words}"
        );
        assert_eq!(json["push_cite"]["stands"].as_bool(), Some(false), "{json}");
    }
}
