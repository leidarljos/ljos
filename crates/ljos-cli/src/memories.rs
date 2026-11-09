//! An agent's memories, written first and reviewed afterwards.
//!
//! An agent's `ljos remember`, sitting or not, stores a standing lesson at
//! once, origin `agent-derived`, with a `source` naming the seat, the
//! harness, the session, the time and, when there is one, the reason.
//! `ljos finish --lesson`, `ljos findings --remember` and the prompt
//! hook's correction store theirs at once too, labelled the same way, and
//! `ljos accept` raises one to `explicit`. A person, or a seat they ask,
//! reviews afterwards: `ljos memories review` lists the unreviewed ones
//! newest first and keeps, edits, merges or drops each, and `ljos memories
//! tui` browses and prunes them in the terminal.
//!
//! A verdict is a line in `$XDG_STATE_HOME/ljos/memory-reviews.jsonl`. A
//! drop, an edit and a merge also retire the claim in the pack, which
//! tombstones it, so `ljos forget` history still shows it was held.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Kinds a review holds: the claims recall shows as lessons or rules.
pub const REVIEW_KINDS: &[&str] = &[
    "lesson",
    "preference",
    "rule",
    "fact",
    "summary",
    "note",
    "episode",
];

/// Why an agent wrote a memory now: `LJOS_MEMORY_WHY` when the runner or
/// the agent set it, else the opening of the person's latest prompt.
#[must_use]
pub fn write_reason() -> Option<String> {
    std::env::var("LJOS_MEMORY_WHY")
        .ok()
        .map(|w| crate::capture::excerpt_of(&w))
        .filter(|w| !w.is_empty())
        .or_else(crate::capture::latest_prompt_excerpt)
}

/// One memory as a review shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Memory {
    pub id: String,
    pub text: String,
    pub kind: String,
    pub origin: String,
    /// `source.basis` as stored. Empty when the writer left none.
    pub basis: String,
    pub seat: String,
    pub harness: String,
    pub session: String,
    pub host: String,
    /// RFC 3339 UTC, the write's time.
    pub at: String,
    pub reason: Option<String>,
    /// Times a prompt or `ljos recall` handed it over, plus graded review reps.
    pub recalls: u64,
    /// Times `ljos search` or the search tool handed it over.
    pub lookups: u64,
    /// Uses recorded before a search and a recall were counted apart.
    /// The file is not rewritten: the two cannot be told apart in it.
    pub earlier: u64,
    /// The last verdict on it, if any.
    pub verdict: Option<String>,
    /// Later wordings of the same lesson that reinforced it.
    pub aliases: Vec<String>,
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// The id as a listing shows it.
#[must_use]
pub fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

impl Memory {
    /// The memory an atom is, when it is a kind a review holds.
    #[must_use]
    pub fn of(atom: &Value) -> Option<Self> {
        let kind = s(&atom["kind"]);
        if !REVIEW_KINDS.contains(&kind.as_str()) || atom["tombstone"] == Value::Bool(true) {
            return None;
        }
        let id = s(&atom["id"]);
        if id.is_empty() {
            return None;
        }
        let src = &atom["source"];
        let entity_seat = atom["entities"].as_array().and_then(|e| {
            e.iter()
                .filter_map(Value::as_str)
                .find_map(|t| t.strip_prefix(crate::SEAT_ENTITY))
                .map(str::to_string)
        });
        let seat = Some(s(&src["seat"]))
            .filter(|x| !x.is_empty())
            .or(entity_seat)
            .unwrap_or_else(|| s(&src["harness"]));
        let at = Some(s(&src["at"]))
            .filter(|x| !x.is_empty())
            .unwrap_or_else(|| s(&atom["ts"]));
        let origin = Some(s(&atom["origin"]))
            .filter(|x| !x.is_empty())
            .unwrap_or_else(|| crate::admit::ORIGIN_USER.to_string());
        Some(Self {
            id,
            text: s(&atom["text"]),
            kind,
            origin,
            basis: s(&src["basis"]),
            harness: Some(s(&src["harness"]))
                .filter(|x| !x.is_empty())
                .unwrap_or_else(|| seat.clone()),
            seat,
            session: s(&src["session"]),
            host: s(&src["host"]),
            at,
            reason: src["reason"].as_str().map(str::to_string),
            recalls: atom["review"]["reps"].as_u64().unwrap_or(0),
            lookups: 0,
            earlier: 0,
            verdict: None,
            aliases: aliases_of(atom).map(str::to_string).collect(),
        })
    }

    #[must_use]
    pub fn short_id(&self) -> &str {
        short_id(&self.id)
    }

    /// The date part of [`Self::at`].
    #[must_use]
    pub fn day(&self) -> &str {
        self.at.get(..10).unwrap_or(&self.at)
    }

    #[must_use]
    pub fn is_agents(&self) -> bool {
        self.origin != crate::admit::ORIGIN_USER
    }
}

/// One verdict on a memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub id: String,
    /// `keep`, `drop`, `edit` or `merge`.
    pub verdict: String,
    pub at: String,
    pub by: String,
    /// The claim an edit or a merge wrote in its place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
}

/// The file the verdicts are kept in.
#[must_use]
pub fn reviews_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos/memory-reviews.jsonl")
}

/// The latest verdict per id.
#[must_use]
pub fn reviews() -> BTreeMap<String, Review> {
    let bytes = crate::read_state_file(&reviews_path()).unwrap_or_default();
    crate::complete_rows::<Review>(&bytes)
        .into_iter()
        .map(|r| (r.id.clone(), r))
        .collect()
}

/// Append one verdict.
///
/// # Errors
///
/// The state directory cannot be written.
pub fn record(id: &str, verdict: &str, into: Option<&str>) -> Result<Review> {
    let r = Review {
        id: id.to_string(),
        verdict: verdict.to_string(),
        at: crate::now_utc(),
        by: crate::seat_name(),
        into: into.map(str::to_string),
    };
    let path = reviews_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_string(&r)?;
    line.push('\n');
    crate::append_locked_row(&path, line.as_bytes(), crate::Durability::Durable)
        .with_context(|| format!("memories: cannot write {}", path.display()))?;
    Ok(r)
}

/// What a listing shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub seat: Option<String>,
    /// `YYYY-MM-DD`: written on or after this day.
    pub since: Option<String>,
    /// Reviewed ones too, not only those still unreviewed.
    pub reviewed: bool,
    /// The person's own claims too, not only the agents'.
    pub user: bool,
}

impl Filter {
    #[must_use]
    pub fn admits(&self, m: &Memory) -> bool {
        (self.user || m.is_agents())
            && (self.reviewed || m.verdict.is_none())
            && self.seat.as_deref().is_none_or(|seat| m.seat == seat)
            && self.since.as_deref().is_none_or(|d| m.day() >= d)
    }
}

/// Uses recorded before a search and a recall were counted apart.
/// Left as written. A new recall goes to [`recall_counts_path`] and a
/// search to [`lookup_counts_path`].
#[must_use]
pub fn recall_uses_path() -> PathBuf {
    state_file("recall-uses.json")
}

/// Recalls from a prompt or `ljos recall`, one per atom id.
#[must_use]
pub fn recall_counts_path() -> PathBuf {
    state_file("recalls.json")
}

/// Lookups from `ljos search` or the search tool, one per atom id.
#[must_use]
pub fn lookup_counts_path() -> PathBuf {
    state_file("lookups.json")
}

fn state_file(name: &str) -> PathBuf {
    reviews_path()
        .parent()
        .map(|dir| dir.join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn read_counts(path: &Path) -> BTreeMap<String, u64> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// The mixed uses still in [`recall_uses_path`].
#[must_use]
pub fn recall_uses() -> BTreeMap<String, u64> {
    read_counts(&recall_uses_path())
}

/// Recalls recorded since search and recall were counted apart.
#[must_use]
pub fn recall_counts() -> BTreeMap<String, u64> {
    read_counts(&recall_counts_path())
}

/// Lookups recorded since search and recall were counted apart.
#[must_use]
pub fn lookup_counts() -> BTreeMap<String, u64> {
    read_counts(&lookup_counts_path())
}

fn bump_counts(path: &Path, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut uses = read_counts(path);
    for id in ids {
        if id.is_empty() {
            continue;
        }
        *uses.entry(id.clone()).or_default() += 1;
    }
    if let Ok(text) = serde_json::to_string(&uses) {
        let _ = std::fs::write(path, text);
    }
}

/// Count one recall of each id. A pack that is down still records it
/// locally, so the next `ljos memories list` shows it. A search is
/// [`bump_lookups`], not a recall.
pub fn bump_recalls(ids: &[String]) {
    bump_counts(&recall_counts_path(), ids);
}

/// Count one lookup of each id a search handed back.
pub fn bump_lookups(ids: &[String]) {
    bump_counts(&lookup_counts_path(), ids);
}

/// How a listing names the two counts. An older mixed total is labelled
/// and left as it stands.
#[must_use]
pub fn use_label(m: &Memory) -> String {
    let mut line = format!("recalled {}x  looked up {}x", m.recalls, m.lookups);
    if m.earlier > 0 {
        line.push_str(&format!(
            "  earlier {}x (recall and lookup were one count)",
            m.earlier
        ));
    }
    line
}

/// The memories in `atoms` that `filter` admits, newest first, each with
/// its last verdict from `reviews`.
#[must_use]
pub fn list(atoms: &[Value], reviews: &BTreeMap<String, Review>, filter: &Filter) -> Vec<Memory> {
    let recalls = recall_counts();
    let lookups = lookup_counts();
    let earlier = recall_uses();
    let mut out: Vec<Memory> = atoms
        .iter()
        .filter_map(Memory::of)
        .map(|mut m| {
            m.verdict = reviews.get(&m.id).map(|r| r.verdict.clone());
            m.recalls = m
                .recalls
                .saturating_add(recalls.get(&m.id).copied().unwrap_or(0));
            m.lookups = lookups.get(&m.id).copied().unwrap_or(0);
            m.earlier = earlier.get(&m.id).copied().unwrap_or(0);
            m
        })
        .filter(|m| filter.admits(m))
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));
    out
}

/// The live atoms of this seat's workspace.
///
/// # Errors
///
/// The pack does not answer.
pub fn live_atoms() -> Result<Vec<Value>> {
    let client = crate::pack()?;
    let ws = client.workspace();
    crate::atoms_lean(&client, &ws)
}

/// The full id of the one memory `short` starts.
///
/// # Errors
///
/// None or more than one memory starts with it.
pub fn resolve(short: &str, all: &[Memory]) -> Result<String> {
    let short = short.trim();
    let hits: Vec<&Memory> = all.iter().filter(|m| m.id.starts_with(short)).collect();
    match hits.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => Err(crate::policy_label::labelled(
            "review-input-warning",
            format!("memories: no memory starts with {short}"),
        )),
        _ => Err(crate::policy_label::labelled(
            "review-input-warning",
            format!(
                "memories: {short} starts {} memories; give more of the id",
                hits.len()
            ),
        )),
    }
}

fn find<'a>(id: &str, all: &'a [Memory]) -> Result<&'a Memory> {
    let full = resolve(id, all)?;
    all.iter()
        .find(|m| m.id == full)
        .context("memories: the id vanished")
}

/// Keep a memory as it stands.
///
/// # Errors
///
/// An id that names no memory, or the state directory.
pub fn keep(id: &str, all: &[Memory]) -> Result<String> {
    let m = find(id, all)?;
    record(&m.id, "keep", None)?;
    Ok(format!("kept {}\n", m.short_id()))
}

/// Drop a memory: the pack tombstones it.
///
/// # Errors
///
/// An id that names no memory, or the pack refusing.
pub fn drop_memory(id: &str, all: &[Memory]) -> Result<String> {
    let m = find(id, all)?;
    // The verdict is the durable record of the drop. Write it first: if the
    // pack then does not answer, the claim is still live and marked dropped,
    // not tombstoned with no verdict to show for it.
    record(&m.id, "drop", None)?;
    crate::packset_forget(&m.id, None)?;
    Ok(format!("dropped {}\n", m.short_id()))
}

/// Write `text` in place of `olds`, carrying the first one's provenance
/// and naming what it replaced, then retire the old claims.
///
/// The new claim keeps the origin and the basis the memory already had.
/// A missing basis is left missing: origin `user-declared` is not a
/// person basis, and this does not stamp one. Under a runner the new text
/// is the agent's, so it is written as `agent-derived`.
fn replace(olds: &[&Memory], text: &str, verdict: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        bail!("memories: {verdict} needs a text");
    }
    let first = olds.first().context("memories: nothing to replace")?;
    let client = crate::pack()?;
    let ws = client.workspace();
    let mut atom = crate::atom_body(&first.kind, text, &ws);
    atom["source"]["seat"] = Value::String(first.seat.clone());
    atom["source"]["harness"] = Value::String(first.harness.clone());
    atom["source"]["session"] = Value::String(first.session.clone());
    atom["source"]["reviewed_by"] = Value::String(crate::seat_name());
    atom["source"]["replaces"] =
        Value::Array(olds.iter().map(|m| Value::String(m.id.clone())).collect());
    if let Some(reason) = &first.reason {
        atom["source"]["reason"] = Value::String(reason.clone());
    }
    crate::add_entities(&mut atom, ["horizon:standing".to_string()]);
    let posted = if first.is_agents() || crate::under_a_runner() {
        crate::admit::write_agent(&client, atom)?
    } else {
        atom["origin"] = Value::String(first.origin.clone());
        if !first.basis.is_empty() {
            atom["source"]["basis"] = Value::String(first.basis.clone());
        }
        crate::admit::post_kept(&client, &atom)?
    };
    let new_id = s(&posted["id"]);
    for m in olds {
        // Record the verdict before retiring the old claim, so a pack that
        // stops answering mid-replace leaves the old one live and marked,
        // not forgotten with no verdict naming what replaced it.
        record(&m.id, verdict, Some(&new_id))?;
        if m.id != new_id {
            crate::packset_forget(&m.id, None)?;
        }
    }
    if !new_id.is_empty() {
        record(&new_id, "keep", None)?;
    }
    Ok(format!(
        "{verdict} {} into {}\n",
        olds.iter()
            .map(|m| m.short_id())
            .collect::<Vec<_>>()
            .join(", "),
        new_id.get(..8).unwrap_or(&new_id)
    ))
}

/// Rewrite one memory's text. The replacement keeps the basis the memory
/// already had, or is the agent's under a runner.
///
/// # Errors
///
/// An id that names no memory, an empty text, or the pack refusing.
pub fn edit(id: &str, text: &str, all: &[Memory]) -> Result<String> {
    let m = find(id, all)?;
    replace(&[m], text, "edited")
}

/// Merge memories into one. Without `text` the merged text is theirs,
/// joined in the order given. The replacement keeps the basis the first
/// memory already had, or is the agent's under a runner.
///
/// # Errors
///
/// Fewer than two ids, an id that names no memory, or the pack refusing.
pub fn merge(ids: &[String], text: Option<&str>, all: &[Memory]) -> Result<String> {
    if ids.len() < 2 {
        bail!("memories: merge takes two ids or more");
    }
    let olds: Vec<&Memory> = ids.iter().map(|id| find(id, all)).collect::<Result<_>>()?;
    let joined;
    let text = if let Some(t) = text {
        t
    } else {
        joined = olds
            .iter()
            .map(|m| m.text.trim())
            .collect::<Vec<_>>()
            .join(" ");
        &joined
    };
    replace(&olds, text, "merged")
}

/// The memory as the review prints it: id, kind, text, then provenance.
#[must_use]
pub fn describe(m: &Memory) -> String {
    let mut out = format!("{}  {}  {}\n", m.short_id(), m.kind, m.text);
    out.push_str(&format!(
        "    seat {}  harness {}  session {}  at {}  origin {}  {}{}\n",
        dash(&m.seat),
        dash(&m.harness),
        dash(&m.session),
        dash(&m.at),
        m.origin,
        use_label(m),
        m.verdict
            .as_deref()
            .map(|v| format!("  verdict {v}"))
            .unwrap_or_default()
    ));
    if let Some(reason) = &m.reason {
        out.push_str(&format!("    why: {reason}\n"));
    }
    for alias in &m.aliases {
        out.push_str(&format!("    also written: {alias}\n"));
    }
    out
}

fn dash(x: &str) -> &str {
    if x.is_empty() {
        "-"
    } else {
        x
    }
}

/// One stale memory a prune would drop, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stale {
    pub id: String,
    pub text: String,
    pub why: String,
}

fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Memories a prune would drop: a later one with the same words (the
/// newest is kept), and one never recalled that is older than `days` as
/// of `today` (`YYYY-MM-DD`). A kept verdict protects a memory.
#[must_use]
pub fn stale(all: &[Memory], days: i64, today: &str) -> Vec<Stale> {
    let mut out = Vec::new();
    let mut seen: BTreeMap<String, &Memory> = BTreeMap::new();
    let mut newest_first: Vec<&Memory> = all.iter().collect();
    newest_first.sort_by(|a, b| b.at.cmp(&a.at));
    let mut dropped = BTreeSet::new();
    for m in &newest_first {
        let key = format!("{}\n{}", m.kind, fold(&m.text));
        if let Some(newer) = seen.get(&key) {
            if m.verdict.as_deref() != Some("keep") {
                out.push(Stale {
                    id: m.id.clone(),
                    text: m.text.clone(),
                    why: format!("duplicate of {}", newer.short_id()),
                });
                dropped.insert(m.id.clone());
            }
        } else {
            seen.insert(key, m);
        }
    }
    let cutoff = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .ok()
        .map(|d| {
            (d - chrono::Duration::days(days))
                .format("%Y-%m-%d")
                .to_string()
        });
    if let Some(cutoff) = cutoff {
        for m in &newest_first {
            if dropped.contains(&m.id) || m.verdict.as_deref() == Some("keep") {
                continue;
            }
            if m.recalls == 0 && m.earlier == 0 && !m.at.is_empty() && m.day() < cutoff.as_str() {
                out.push(Stale {
                    id: m.id.clone(),
                    text: m.text.clone(),
                    why: format!("never recalled since {}", m.day()),
                });
            }
        }
    }
    out
}

/// Two wordings of one lesson share a two-thirds supermajority of their
/// content words, as [`crate::prompt_words`] reads them, counted over both
/// together, so a match needs most of each wording's words. The fixture
/// sweep in `measurements/lesson-duplicates.txt` shows no wrong merge
/// anywhere from 0.60 to 0.90, so the result does not hinge on the exact
/// value.
const SAME_LESSON: f64 = 2.0 / 3.0;

/// Cardinal words. `within two seconds` and `within five seconds` share
/// every other word and are two lessons.
const NUMBER_WORDS: &[&str] = &[
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty",
    "ninety", "hundred", "thousand", "million", "billion",
];

/// Words that set an order, a condition, a comparison, a version or a
/// presence. [`crate::prompt_words`] drops most of them as stop words, so
/// `after` for `before`, or `old` added, leaves the content words as they
/// were and says something else.
const RELATION_WORDS: &[&str] = &[
    "before", "after", "until", "unless", "then", "since", "first", "last", "earlier", "later",
    "with", "without", "new", "old", "only", "more", "less", "fewer", "most", "least", "over",
    "under", "above", "below", "into", "from",
];

/// The relation words that order two steps: what stands ahead of one is
/// the step that comes first.
const ORDER_WORDS: &[&str] = &["before", "after", "until", "then"];

fn content(text: &str) -> BTreeSet<String> {
    crate::prompt_words(text).iter().cloned().collect()
}

fn overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// What a rewording keeps: the numbers and relation words, and the
/// content words ahead of the first order word.
fn fixed_parts(text: &str) -> (BTreeSet<String>, Option<BTreeSet<String>>) {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let fixed = words
        .iter()
        .filter(|w| {
            w.chars().any(|c| c.is_ascii_digit())
                || NUMBER_WORDS.contains(&w.as_str())
                || RELATION_WORDS.contains(&w.as_str())
        })
        .cloned()
        .collect();
    let ahead = words
        .iter()
        .position(|w| ORDER_WORDS.contains(&w.as_str()))
        .map(|at| content(&words[..at].join(" ")));
    (fixed, ahead)
}

/// Whether `new` says what `held` says in other words: they share
/// [`SAME_LESSON`] of their content words, keep the same
/// [`fixed_parts`], forbid the same thing or nothing, and do not trade
/// one content word for another, which is the same claim about another
/// object. A miss leaves two memories; a wrong match loses a lesson, so
/// each check refuses when unsure.
#[must_use]
pub fn same_lesson(new: &str, held: &str) -> bool {
    let (a, b) = (content(new), content(held));
    if overlap(&a, &b) < SAME_LESSON
        || (a.difference(&b).count() == 1 && b.difference(&a).count() == 1)
        || fixed_parts(new) != fixed_parts(held)
    {
        return false;
    }
    let forbids = |t: &str| crate::command_prefs::forbidden_part(t).map(|p| content(&p));
    match (forbids(new), forbids(held)) {
        (None, None) => true,
        (Some(x), Some(y)) => overlap(&x, &y) >= SAME_LESSON,
        _ => false,
    }
}

fn aliases_of(atom: &Value) -> impl Iterator<Item = &str> {
    atom["aliases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// The live memory `atom` says again, reinforced in its place: graded
/// recalled, as `ljos graded` does, with the new wording kept in its
/// `aliases`. Only a memory of the same kind and set counts, and not one
/// the pack closes itself when `atom` is posted: a person's rewrite still
/// revises. A person's words reinforce only a person's memory; folded
/// into a derived one they would lose the `explicit` level they are filed
/// at ([`crate::admit::admission_level`]). `None` sends the caller on to
/// post `atom` as before, and so does a pack that does not answer, so the
/// post says what went wrong.
pub fn reinforce_held(client: &crate::pack_link::PacksetClient, atom: &Value) -> Option<Value> {
    let text = atom["text"].as_str()?.trim();
    let new = atom.as_object()?;
    // An alias skips the pack's secret check on `text`; the post makes it.
    packset_core::record::reject_unsafe(text).ok()?;
    let ws = atom["workspace"]
        .as_str()
        .map_or_else(|| client.workspace(), str::to_string);
    let kind = atom["kind"].as_str()?;
    let (held, _) = crate::atoms_listed_of(client, &ws, true, Some(kind), None, false).ok()?;
    let now = crate::now_utc();
    let set = |a: &Value| a["set"].as_str().unwrap_or("").to_string();
    let persons = |a: &Value| a["origin"].as_str() == Some(crate::admit::ORIGIN_USER);
    let found = held.iter().find(|h| {
        h["kind"] == atom["kind"]
            && set(h) == set(atom)
            && (persons(h) || !persons(atom))
            && crate::live_now(h, &now)
            && h.as_object()
                .is_some_and(|old| !packset_core::record::replaces(new, old))
            && std::iter::once(h["text"].as_str().unwrap_or(""))
                .chain(aliases_of(h))
                .any(|w| same_lesson(text, w))
    })?;
    let id = found["id"].as_str().filter(|id| !id.is_empty())?;
    let mut out = client.grade(&ws, id, true).ok()?;
    let said = std::iter::once(found["text"].as_str().unwrap_or(""))
        .chain(aliases_of(found))
        .any(|w| fold(w) == fold(text));
    if !said {
        let mut aliases: Vec<Value> = aliases_of(found).map(Value::from).collect();
        aliases.push(Value::from(text));
        if let Ok(updated) = client.update_atom(&ws, id, &serde_json::json!({"aliases": aliases})) {
            out = updated;
        }
    }
    out["reinforced"] = Value::Bool(true);
    Some(out)
}

/// The line a write prints when it reinforced a memory instead of adding
/// one. `None` for a write that added its own.
#[must_use]
pub fn reinforced_line(body: &Value) -> Option<String> {
    if body["reinforced"] != Value::Bool(true) {
        return None;
    }
    let id = s(&body["id"]);
    Some(format!(
        "reinforced {} instead of adding a copy, recalled {}x: {}",
        id.get(..8).unwrap_or(&id),
        body["review"]["reps"].as_u64().unwrap_or(0),
        s(&body["text"]).trim()
    ))
}

// ---- the terminal viewer -------------------------------------------------

/// What the viewer shows: the pure state its frame is drawn from.
#[derive(Debug, Clone, Default)]
pub struct View {
    pub all: Vec<Memory>,
    pub seat: Option<String>,
    pub since: Option<String>,
    pub show_reviewed: bool,
    pub cursor: usize,
    pub marked: BTreeSet<String>,
    pub status: String,
}

impl View {
    #[must_use]
    pub fn new(all: Vec<Memory>) -> Self {
        Self {
            all,
            status: "j/k move  a keep  d drop  e edit  m mark  M merge  s seat  f since  v reviewed  q quit".into(),
            ..Self::default()
        }
    }

    fn filter(&self) -> Filter {
        Filter {
            seat: self.seat.clone(),
            since: self.since.clone(),
            reviewed: self.show_reviewed,
            user: true,
        }
    }

    /// The rows the filters admit, in listing order.
    #[must_use]
    pub fn rows(&self) -> Vec<&Memory> {
        let f = self.filter();
        self.all.iter().filter(|m| f.admits(m)).collect()
    }

    #[must_use]
    pub fn current(&self) -> Option<&Memory> {
        self.rows().get(self.cursor).copied()
    }

    pub fn clamp(&mut self) {
        let n = self.rows().len();
        self.cursor = self.cursor.min(n.saturating_sub(1));
    }

    pub fn down(&mut self) {
        self.cursor += 1;
        self.clamp();
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// The seats present, sorted.
    #[must_use]
    pub fn seats(&self) -> Vec<String> {
        self.all
            .iter()
            .map(|m| m.seat.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Step the seat filter: all, then each seat in turn.
    pub fn cycle_seat(&mut self) {
        let seats = self.seats();
        self.seat = match &self.seat {
            None => seats.first().cloned(),
            Some(cur) => seats
                .iter()
                .position(|s| s == cur)
                .and_then(|i| seats.get(i + 1).cloned()),
        };
        self.cursor = 0;
    }

    pub fn toggle_mark(&mut self) {
        if let Some(id) = self.current().map(|m| m.id.clone()) {
            if !self.marked.remove(&id) {
                self.marked.insert(id);
            }
        }
    }

    /// Note a verdict on `id` in the view, as the file now says.
    pub fn set_verdict(&mut self, id: &str, verdict: &str) {
        for m in &mut self.all {
            if m.id == id {
                m.verdict = Some(verdict.to_string());
            }
        }
        self.clamp();
    }
}

fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The frame for `view` in a `width` by `height` terminal, as plain lines:
/// a header with the filters, the list (the cursor row starts with `>`, a
/// marked row with `*`), the selected memory's provenance, and the key line.
#[must_use]
pub fn render(view: &View, width: usize, height: usize) -> Vec<String> {
    let width = width.max(20);
    let height = height.max(8);
    let rows = view.rows();
    let mut out = vec![clip(
        &format!(
            "ljos memories  {} shown of {}  seat {}  since {}  {}",
            rows.len(),
            view.all.len(),
            view.seat.as_deref().unwrap_or("all"),
            view.since.as_deref().unwrap_or("any"),
            if view.show_reviewed {
                "all verdicts"
            } else {
                "unreviewed"
            }
        ),
        width,
    )];
    out.push("─".repeat(width));
    // Header 2, detail 4, status 2.
    let list_h = height.saturating_sub(8).max(1);
    let start = view.cursor.saturating_sub(list_h.saturating_sub(1));
    for (i, m) in rows.iter().enumerate().skip(start).take(list_h) {
        let pointer = if i == view.cursor { '>' } else { ' ' };
        let mark = if view.marked.contains(&m.id) {
            '*'
        } else {
            ' '
        };
        let verdict = m.verdict.as_deref().unwrap_or("new");
        out.push(clip(
            &format!(
                "{pointer}{mark} {} {} {:<12} {:<5} {}",
                m.short_id(),
                m.day(),
                clip(&m.seat, 12),
                clip(verdict, 5),
                m.text
            ),
            width,
        ));
    }
    if rows.is_empty() {
        out.push("  nothing to review".into());
    }
    while out.len() < 2 + list_h {
        out.push(String::new());
    }
    out.push("─".repeat(width));
    match view.current() {
        Some(m) => {
            out.push(clip(&m.text, width));
            out.push(clip(
                &format!(
                    "seat {}  harness {}  session {}  host {}",
                    dash(&m.seat),
                    dash(&m.harness),
                    dash(&m.session),
                    dash(&m.host)
                ),
                width,
            ));
            out.push(clip(
                &format!(
                    "at {}  origin {}  why: {}  {}",
                    dash(&m.at),
                    m.origin,
                    m.reason.as_deref().unwrap_or("-"),
                    use_label(m)
                ),
                width,
            ));
        }
        None => out.extend([String::new(), String::new(), String::new()]),
    }
    out.push("─".repeat(width));
    out.push(clip(&view.status, width));
    out.truncate(height);
    out
}

/// One key the viewer acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Char(char),
    Other,
}

/// What a key asks the loop to do beyond changing the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    None,
    Quit,
    Keep(String),
    Drop(String),
    /// Ask for a new text, then edit.
    Edit(String),
    Merge(Vec<String>),
    /// Ask for a date.
    Since,
}

/// Apply one key to the view; the side effect, if any, is returned.
pub fn on_key(view: &mut View, key: Key) -> Act {
    let current = view.current().map(|m| m.id.clone());
    match key {
        Key::Up | Key::Char('k') => view.up(),
        Key::Down | Key::Char('j') => view.down(),
        Key::Char('q') => return Act::Quit,
        Key::Char('s') => view.cycle_seat(),
        Key::Char('v') => {
            view.show_reviewed = !view.show_reviewed;
            view.cursor = 0;
        }
        Key::Char('m') => view.toggle_mark(),
        Key::Char('f') => return Act::Since,
        Key::Char('a') => return current.map_or(Act::None, Act::Keep),
        Key::Char('d') => return current.map_or(Act::None, Act::Drop),
        Key::Char('e') => return current.map_or(Act::None, Act::Edit),
        Key::Char('M') => {
            if view.marked.len() >= 2 {
                return Act::Merge(view.marked.iter().cloned().collect());
            }
            view.status = "mark two memories or more with m, then M".into();
        }
        _ => {}
    }
    Act::None
}

/// The terminal in raw mode until dropped.
struct Raw {
    saved: libc::termios,
}

impl Raw {
    fn on() -> Result<Self> {
        // SAFETY: tcgetattr/tcsetattr on stdin with a zeroed struct the
        // call fills; the saved state is restored on drop.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                bail!("memories tui: stdin is not a terminal");
            }
            let saved = t;
            t.c_lflag &= !(libc::ICANON | libc::ECHO);
            t.c_cc[libc::VMIN] = 1;
            t.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSANOW, &t);
            Ok(Self { saved })
        }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        // SAFETY: restores the attributes read in `on`.
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.saved);
        }
        print!("\x1b[?25h\x1b[?1049l");
        let _ = std::io::stdout().flush();
    }
}

fn term_size() -> (usize, usize) {
    // SAFETY: TIOCGWINSZ fills a winsize on a terminal fd.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return (usize::from(ws.ws_col), usize::from(ws.ws_row));
        }
    }
    (100, 30)
}

fn read_key() -> Key {
    use std::io::Read;
    let mut b = [0u8; 1];
    if std::io::stdin().read(&mut b).unwrap_or(0) == 0 {
        return Key::Char('q');
    }
    if b[0] == 0x1b {
        let mut seq = [0u8; 2];
        if std::io::stdin().read_exact(&mut seq).is_ok() && seq[0] == b'[' {
            return match seq[1] {
                b'A' => Key::Up,
                b'B' => Key::Down,
                _ => Key::Other,
            };
        }
        return Key::Other;
    }
    Key::Char(char::from(b[0]))
}

fn ask_line(raw: &Raw, prompt: &str) -> String {
    // SAFETY: back to the saved (cooked) mode for one line of input.
    unsafe {
        libc::tcsetattr(0, libc::TCSANOW, &raw.saved);
    }
    let (_, h) = term_size();
    print!("\x1b[{h};1H\x1b[2K\x1b[?25h{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    // SAFETY: raw again, as `Raw::on` set it.
    unsafe {
        let mut t = raw.saved;
        t.c_lflag &= !(libc::ICANON | libc::ECHO);
        libc::tcsetattr(0, libc::TCSANOW, &t);
    }
    line.trim().to_string()
}

fn draw(view: &View) {
    let (w, h) = term_size();
    let mut frame = String::from("\x1b[?25l\x1b[H\x1b[2J");
    for (i, line) in render(view, w, h).iter().enumerate() {
        if line.starts_with('>') {
            frame.push_str(&format!("\x1b[{};1H\x1b[7m{line}\x1b[0m", i + 1));
        } else {
            frame.push_str(&format!("\x1b[{};1H{line}", i + 1));
        }
    }
    print!("{frame}");
    let _ = std::io::stdout().flush();
}

/// Run the viewer over the live pack until `q`.
///
/// # Errors
///
/// No terminal, or the pack not answering.
pub fn tui(filter: &Filter) -> Result<()> {
    let atoms = live_atoms()?;
    let mut view = View::new(list(
        &atoms,
        &reviews(),
        &Filter {
            reviewed: true,
            user: filter.user,
            ..Filter::default()
        },
    ));
    view.seat.clone_from(&filter.seat);
    view.since.clone_from(&filter.since);
    view.show_reviewed = filter.reviewed;
    let raw = Raw::on()?;
    print!("\x1b[?1049h");
    loop {
        draw(&view);
        let act = on_key(&mut view, read_key());
        let all = view.all.clone();
        let done = match act {
            Act::None => continue,
            Act::Quit => break,
            Act::Keep(id) => keep(&id, &all).map(|l| (vec![id], "keep", l)),
            Act::Drop(id) => drop_memory(&id, &all).map(|l| (vec![id], "drop", l)),
            Act::Edit(id) => {
                let text = ask_line(&raw, "new text (empty cancels): ");
                if text.is_empty() {
                    continue;
                }
                edit(&id, &text, &all).map(|l| (vec![id], "edit", l))
            }
            Act::Merge(ids) => {
                let text = ask_line(&raw, "merged text (empty joins them): ");
                let text = (!text.is_empty()).then_some(text);
                merge(&ids, text.as_deref(), &all).map(|l| (ids, "merge", l))
            }
            Act::Since => {
                let d = ask_line(&raw, "since YYYY-MM-DD (empty clears): ");
                view.since = (!d.is_empty()).then_some(d);
                view.cursor = 0;
                continue;
            }
        };
        match done {
            Ok((ids, verdict, line)) => {
                for id in &ids {
                    view.set_verdict(id, verdict);
                }
                view.marked.clear();
                view.status = line.trim().to_string();
            }
            Err(e) => view.status = format!("{e:#}"),
        }
    }
    drop(raw);
    Ok(())
}

/// The question-by-question review over stdin: each unreviewed memory,
/// newest first, then `k`eep, `e`dit, `m`erge with another id, `d`rop,
/// `s`kip or `q`uit.
///
/// # Errors
///
/// The pack not answering.
pub fn review_interactive(all: &[Memory]) -> Result<String> {
    let mut out = String::new();
    let stdin = std::io::stdin();
    for m in all.iter().filter(|m| m.verdict.is_none()) {
        print!(
            "{}[k]eep [e]dit [m]erge [d]rop [s]kip [q]uit? ",
            describe(m)
        );
        std::io::stdout().flush()?;
        let mut answer = String::new();
        if stdin.lock().read_line(&mut answer)? == 0 {
            break;
        }
        let said = match answer.trim() {
            "k" | "keep" => keep(&m.id, all),
            "d" | "drop" => drop_memory(&m.id, all),
            "e" | "edit" => {
                print!("new text: ");
                std::io::stdout().flush()?;
                let mut t = String::new();
                stdin.lock().read_line(&mut t)?;
                edit(&m.id, &t, all)
            }
            "m" | "merge" => {
                print!("merge with id: ");
                std::io::stdout().flush()?;
                let mut other = String::new();
                stdin.lock().read_line(&mut other)?;
                merge(&[m.id.clone(), other.trim().to_string()], None, all)
            }
            "q" | "quit" => break,
            _ => continue,
        };
        match said {
            Ok(line) => {
                print!("{line}");
                out.push_str(&line);
            }
            Err(e) => println!("{e:#}"),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn atom(id: &str, text: &str, seat: &str, at: &str, origin: &str) -> Value {
        json!({
            "id": id, "kind": "lesson", "text": text, "origin": origin,
            "entities": [format!("seat:{seat}")],
            "source": {"seat": seat, "harness": "codex", "session": format!("{seat}@s1"),
                       "host": "box", "at": at, "reason": "the user said so", "via": "ljos"},
        })
    }

    fn sample() -> Vec<Memory> {
        let atoms = vec![
            atom(
                "a1aaaaaaaaaaaaaa",
                "Run the linter first.",
                "codex",
                "2026-10-12T10:00:00Z",
                "agent-derived",
            ),
            atom(
                "b2bbbbbbbbbbbbbb",
                "Use uv for scripts.",
                "claude-code",
                "2026-10-14T09:00:00Z",
                "agent-derived",
            ),
            atom(
                "c3cccccccccccccc",
                "Typed by the person.",
                "rohit",
                "2026-10-13T09:00:00Z",
                "user-declared",
            ),
            json!({"id": "d4", "kind": "trust", "text": "x"}),
        ];
        list(
            &atoms,
            &BTreeMap::new(),
            &Filter {
                user: true,
                ..Filter::default()
            },
        )
    }

    #[test]
    fn provenance_is_read_from_the_source() {
        let m = Memory::of(&atom(
            "a1aaaaaaaaaaaaaa",
            "t",
            "codex",
            "2026-10-12T10:00:00Z",
            "agent-derived",
        ))
        .unwrap();
        assert_eq!(m.seat, "codex");
        assert_eq!(m.harness, "codex");
        assert_eq!(m.session, "codex@s1");
        assert_eq!(m.at, "2026-10-12T10:00:00Z");
        assert_eq!(m.reason.as_deref(), Some("the user said so"));
        assert!(m.is_agents());
        assert!(m.basis.is_empty());
        let mut stamped = atom(
            "a1aaaaaaaaaaaaaa",
            "t",
            "codex",
            "2026-10-12T10:00:00Z",
            "user-declared",
        );
        stamped["source"]["basis"] = json!("person");
        let stamped = Memory::of(&stamped).unwrap();
        assert_eq!(stamped.basis, "person");
        assert!(!stamped.is_agents());
    }

    #[test]
    fn the_listing_is_newest_first_agents_only_by_default_and_hides_reviewed() {
        let atoms: Vec<Value> = vec![
            atom(
                "a1aaaaaaaaaaaaaa",
                "old",
                "codex",
                "2026-10-12T10:00:00Z",
                "agent-derived",
            ),
            atom(
                "b2bbbbbbbbbbbbbb",
                "new",
                "codex",
                "2026-10-14T10:00:00Z",
                "agent-derived",
            ),
            atom(
                "c3cccccccccccccc",
                "mine",
                "rohit",
                "2026-10-15T10:00:00Z",
                "user-declared",
            ),
        ];
        let got = list(&atoms, &BTreeMap::new(), &Filter::default());
        assert_eq!(
            got.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
            ["new", "old"]
        );
        let mut rev = BTreeMap::new();
        rev.insert(
            "b2bbbbbbbbbbbbbb".to_string(),
            Review {
                id: "b2bbbbbbbbbbbbbb".into(),
                verdict: "keep".into(),
                at: String::new(),
                by: String::new(),
                into: None,
            },
        );
        let got = list(&atoms, &rev, &Filter::default());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "old");
        let since = Filter {
            since: Some("2026-10-13".into()),
            reviewed: true,
            ..Filter::default()
        };
        assert_eq!(list(&atoms, &rev, &since).len(), 1);
    }

    #[test]
    fn keep_records_a_verdict_and_an_ambiguous_id_is_refused() {
        let _g = crate::tests::env_guard();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var_os("XDG_STATE_HOME");
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let all = sample();
        assert!(keep("a1", &all).unwrap().starts_with("kept a1aaaaaa"));
        let r = reviews();
        assert_eq!(r["a1aaaaaaaaaaaaaa"].verdict, "keep");
        assert!(keep("zz", &all).is_err());
        let mut twin = all.clone();
        twin.push(Memory {
            id: "a1aaaaaaaaaaaaab".into(),
            ..all[0].clone()
        });
        assert!(resolve("a1", &twin).is_err());
        match old {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
    }

    #[test]
    fn stale_names_duplicates_and_old_never_recalled_ones_but_not_kept_ones() {
        // sample() counts recalls from the state dir. Another test bumps
        // the same atom id, so this one keeps its own directory.
        let _env = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let mut all = sample();
        let mut dup = all[1].clone();
        dup.id = "e5eeeeeeeeeeeeee".into();
        dup.text = "use UV for scripts".into();
        dup.at = "2026-10-01T00:00:00Z".into();
        all.push(dup);
        let got = stale(&all, 30, "2026-10-20");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].id, "e5eeeeeeeeeeeeee");
        assert!(got[0].why.starts_with("duplicate of b2bbbbbb"));
        let got = stale(&all, 3, "2026-10-20");
        assert!(got
            .iter()
            .any(|s| s.id == "a1aaaaaaaaaaaaaa" && s.why.starts_with("never recalled")));
        for m in &mut all {
            if m.id == "a1aaaaaaaaaaaaaa" {
                m.verdict = Some("keep".into());
            }
        }
        assert!(stale(&all, 3, "2026-10-20")
            .iter()
            .all(|s| s.id != "a1aaaaaaaaaaaaaa"));
    }

    #[test]
    fn the_viewer_frame_shows_rows_cursor_marks_and_provenance() {
        let mut view = View::new(sample());
        let frame = render(&view, 90, 14);
        assert_eq!(frame.len(), 14);
        assert!(
            frame[0].starts_with("ljos memories  3 shown of 3  seat all"),
            "{}",
            frame[0]
        );
        assert!(
            frame[2].starts_with("> ") && frame[2].contains("Use uv for scripts."),
            "{frame:#?}"
        );
        assert!(frame
            .iter()
            .any(|l| l.contains("harness codex") && l.contains("session claude-code@s1")));
        assert!(frame.iter().any(|l| l.contains("why: the user said so")));
        on_key(&mut view, Key::Down);
        on_key(&mut view, Key::Char('m'));
        let frame = render(&view, 90, 14);
        assert!(frame[3].starts_with(">*"), "{frame:#?}");
        assert_eq!(on_key(&mut view, Key::Char('M')), Act::None);
        on_key(&mut view, Key::Up);
        on_key(&mut view, Key::Char('m'));
        assert!(matches!(on_key(&mut view, Key::Char('M')), Act::Merge(ids) if ids.len() == 2));
        assert_eq!(
            on_key(&mut view, Key::Char('d')),
            Act::Drop("b2bbbbbbbbbbbbbb".into())
        );
        view.set_verdict("b2bbbbbbbbbbbbbb", "drop");
        assert_eq!(
            view.rows().len(),
            2,
            "a reviewed row leaves the unreviewed view"
        );
        on_key(&mut view, Key::Char('s'));
        assert_eq!(view.seat.as_deref(), Some("claude-code"));
        assert!(render(&view, 90, 14)
            .iter()
            .any(|l| l.contains("nothing to review")));
        on_key(&mut view, Key::Char('s'));
        assert_eq!(view.seat.as_deref(), Some("codex"));
        assert_eq!(view.rows().len(), 1);
        assert_eq!(on_key(&mut view, Key::Char('q')), Act::Quit);
    }

    #[test]
    fn a_prompt_excerpt_redacts_keys() {
        let got = crate::capture::excerpt_of(
            "from now on use key=abc and sk-live123 then ghp_AAAAAAAAAAAAAAAAAAAAAAA1 ok",
        );
        assert!(
            !got.contains("abc") && !got.contains("sk-live") && !got.contains("ghp_"),
            "{got}"
        );
        assert!(got.starts_with("from now on use [redacted]"));
    }

    /// A line an older ljos tore, bytes that are not UTF-8 among them, is
    /// one lost verdict. The verdicts after it still read, and so does a
    /// file whose last verdict is still being written.
    #[test]
    fn a_torn_review_line_keeps_the_verdicts_after_it() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let path = reviews_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let row = |id: &str| format!(r#"{{"id":"{id}","verdict":"keep","at":"t","by":"s"}}"#);
        let mut bytes = format!("{}\n", row("a")).into_bytes();
        bytes.extend_from_slice(b"{\"id\":\"b\",\"verdict\":\"caf\xc3\n");
        bytes.extend_from_slice(format!("{}\n", row("c")).as_bytes());
        bytes.extend_from_slice(b"{\"id\":\"d\",\"verdict\":\"caf\xc3");
        std::fs::write(&path, bytes).unwrap();
        let got: Vec<String> = reviews().into_keys().collect();
        assert_eq!(got, ["a", "c"]);
    }

    /// A verdict is one JSON line. Two reviews landing together used to
    /// split that line across write calls, so a later read dropped it.
    #[test]
    fn concurrent_review_rows_each_parse() {
        use std::os::unix::fs::PermissionsExt;
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        const THREADS: usize = 32;
        const EACH: usize = 4;
        let mut handles = Vec::with_capacity(THREADS);
        for thread in 0..THREADS {
            handles.push(std::thread::spawn(move || {
                for n in 0..EACH {
                    record(&format!("id-{thread}-{n}"), "keep", None).unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().expect("review writer");
        }
        let path = reviews_path();
        let text = std::fs::read_to_string(&path).unwrap();
        let rows: Vec<Review> = text
            .lines()
            .map(|line| {
                serde_json::from_str(line).unwrap_or_else(|err| {
                    panic!("review row does not parse ({err}): {line}");
                })
            })
            .collect();
        assert_eq!(rows.len(), THREADS * EACH, "{text}");
        let mode = std::fs::symlink_metadata(&path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "review log mode {mode:o}");
    }

    /// A verdict is durable: a second process holding the lock longer than
    /// the best-effort skip does not make the write drop the verdict; it
    /// waits the lock out and records it.
    #[test]
    fn a_durable_verdict_waits_for_a_held_lock() {
        use std::os::unix::io::AsRawFd;
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let path = reviews_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let held = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        // SAFETY: `held` is open until the releaser drops it below.
        assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            // SAFETY: `held` is still open on this thread.
            unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_UN) };
            drop(held);
        });
        record("a1aaaaaaaaaaaaaa", "keep", None).unwrap();
        releaser.join().unwrap();
        assert_eq!(reviews()["a1aaaaaaaaaaaaaa"].verdict, "keep");
    }

    /// A drop records the verdict before it asks the pack to tombstone the
    /// claim. With the pack off the forget fails, and the drop verdict is
    /// still on disk, not lost with the failed pack change.
    #[test]
    fn a_drop_records_the_verdict_before_the_pack_change() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let old = std::env::var_os("PACKSET_URL");
        std::env::set_var("PACKSET_URL", "off");
        let all = sample();
        assert!(drop_memory("a1", &all).is_err());
        assert_eq!(reviews()["a1aaaaaaaaaaaaaa"].verdict, "drop");
        match old {
            Some(v) => std::env::set_var("PACKSET_URL", v),
            None => std::env::remove_var("PACKSET_URL"),
        }
    }

    /// The measurement behind [`SAME_LESSON`]: no near-topic lesson on the
    /// fixture is taken for another, and the duplicates found are the ones
    /// `measurements/lesson-duplicates.txt` reports.
    #[test]
    fn the_fixture_merges_no_two_different_lessons() {
        let text = include_str!("../tests/fixtures/lesson-duplicates.txt");
        let (mut found, mut pairs, mut wrong) = (0, 0, Vec::new());
        for block in text.split("\n\n") {
            let same: Vec<&str> = block.lines().filter_map(|l| l.strip_prefix("= ")).collect();
            let other: Vec<&str> = block.lines().filter_map(|l| l.strip_prefix("x ")).collect();
            for (i, a) in same.iter().enumerate() {
                for b in &same[i + 1..] {
                    pairs += 1;
                    found += usize::from(same_lesson(b, a));
                }
                for x in &other {
                    if same_lesson(x, a) {
                        wrong.push(format!("{x} | {a}"));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        assert_eq!(
            (found, pairs),
            (12, 78),
            "update measurements/lesson-duplicates.txt"
        );
    }

    #[test]
    fn a_recall_bumps_the_count_the_list_shows() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", dir.path());
        let atoms = vec![atom(
            "a1aaaaaaaaaaaaaa",
            "pin the fuse",
            "saddle",
            "2026-10-16T10:00:00Z",
            "agent-derived",
        )];
        let before = list(&atoms, &BTreeMap::new(), &Filter::default());
        assert_eq!(before[0].recalls, 0);
        assert_eq!(before[0].lookups, 0);
        bump_lookups(&["a1aaaaaaaaaaaaaa".into()]);
        let looked = list(&atoms, &BTreeMap::new(), &Filter::default());
        assert_eq!(looked[0].recalls, 0);
        assert_eq!(looked[0].lookups, 1);
        bump_recalls(&["a1aaaaaaaaaaaaaa".into()]);
        let after = list(&atoms, &BTreeMap::new(), &Filter::default());
        assert_eq!(after[0].recalls, 1);
        assert_eq!(after[0].lookups, 1);
        let said = describe(&after[0]);
        assert!(said.contains("recalled 1x"), "{said}");
        assert!(said.contains("looked up 1x"), "{said}");
        std::fs::write(recall_uses_path(), r#"{"a1aaaaaaaaaaaaaa":4}"#).unwrap();
        let mixed = list(&atoms, &BTreeMap::new(), &Filter::default());
        assert_eq!(mixed[0].earlier, 4);
        assert_eq!(mixed[0].recalls, 1);
        let said = describe(&mixed[0]);
        assert!(
            said.contains("earlier 4x (recall and lookup were one count)"),
            "{said}"
        );
        std::env::remove_var("XDG_STATE_HOME");
    }
}
