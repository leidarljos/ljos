//! A panel closes itself. Once the substance has passed, a nit, a wording
//! note or a style note is written on the issue and never becomes a child.
//! A persona that wants a child marks the objection blocking and gives a
//! reason; the default, with no mark or a mark and no reason, is a nit.
//! The same objection is not filed twice. A dissent child does not file
//! a grandchild unless the new objection is blocking. At the round limit,
//! or when the budget is spent, the item closes as settled-with-notes and
//! lists what remains. Nothing here waits on a person.
//!
//! `ljos consensus` files its children through [`record_dissents`], titled
//! `Dissent holds TEXT` for a blocking objection and `Dissent holds OPTION`
//! for a minority still held after a settled majority. A `fix:` child comes
//! only from `ljos workflow --round` ([`file_fix`]). The round is where the
//! tracker records it: a `panel rN` title that `ljos workflow --round`
//! filed, else the `panel-round: N` note each filing `ljos consensus`
//! writes.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Severities that stay on the issue as notes once the substance has settled.
pub const BELOW_CHILD: &[&str] = &["nit", "wording", "style"];
const DISSENT_TITLE_LIMIT: usize = 120;

/// How serious an objection is. A child is filed only for [`Severity::Blocking`]
/// with a non-empty reason, or for a held minority on an unsettled issue
/// that is not itself a dissent child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Nit,
    Wording,
    Style,
    Blocking,
}

impl Severity {
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word.trim().to_ascii_lowercase().as_str() {
            "nit" => Some(Self::Nit),
            "wording" => Some(Self::Wording),
            "style" => Some(Self::Style),
            "blocking" => Some(Self::Blocking),
            _ => None,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Nit => "nit",
            Self::Wording => "wording",
            Self::Style => "style",
            Self::Blocking => "blocking",
        }
    }

    /// Below the line that files a child.
    #[must_use]
    pub fn is_note(self) -> bool {
        !matches!(self, Self::Blocking)
    }
}

/// One objection a persona raised against a target (an issue, or a span on it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objection {
    pub persona: String,
    pub severity: Severity,
    /// Required for a blocking mark. Empty means the mark did not take,
    /// and the objection is a nit.
    pub reason: String,
    pub text: String,
    pub target: String,
    /// A ballot that still holds a minority option on an issue whose
    /// substance has not settled. Filed once, at the top of a tree only.
    pub held: bool,
}

impl Objection {
    #[must_use]
    pub fn nit(persona: &str, text: &str, target: &str) -> Self {
        Self::noted(persona, Severity::Nit, "", text, target)
    }

    #[must_use]
    pub fn wording(persona: &str, text: &str, target: &str) -> Self {
        Self::noted(persona, Severity::Wording, "", text, target)
    }

    #[must_use]
    pub fn style(persona: &str, text: &str, target: &str) -> Self {
        Self::noted(persona, Severity::Style, "", text, target)
    }

    #[must_use]
    pub fn blocking(persona: &str, reason: &str, text: &str, target: &str) -> Self {
        Self::noted(persona, Severity::Blocking, reason, text, target)
    }

    fn noted(persona: &str, severity: Severity, reason: &str, text: &str, target: &str) -> Self {
        let reason = reason.trim().to_string();
        let severity = if severity == Severity::Blocking && reason.is_empty() {
            Severity::Nit
        } else {
            severity
        };
        Self {
            persona: persona.trim().to_string(),
            severity,
            reason,
            text: text.trim().to_string(),
            target: target.trim().to_string(),
            held: false,
        }
    }

    /// A blocking mark with a reason. Anything else stays a note.
    #[must_use]
    pub fn files_as_child(&self) -> bool {
        self.severity == Severity::Blocking && !self.reason.trim().is_empty()
    }

    /// Whether this objection may become a child from here.
    /// A dissent child (`depth` >= 1) files only a blocking objection.
    #[must_use]
    pub fn may_file(&self, passed: bool, depth: usize) -> bool {
        if self.files_as_child() {
            return true;
        }
        self.held && !passed && depth == 0
    }

    /// Normalized text plus normalized target. Two objections with this
    /// pair are the same objection.
    #[must_use]
    pub fn key(&self) -> (String, String) {
        let text = if self.files_as_child() || self.held {
            dissent_key_text(&self.text)
        } else {
            normalize(&self.text)
        };
        (text, normalize(&self.target))
    }
}

/// Collapse case, punctuation and whitespace so a rephrased comma is the
/// same objection as the last round's comma.
#[must_use]
pub fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn dissent_key_text(text: &str) -> String {
    normalize(&clip(text, DISSENT_TITLE_LIMIT))
}

/// `finding: NAME: TEXT` is a nit. `finding: NAME: wording: TEXT` and
/// `finding: NAME: style: TEXT` stay notes. `finding: NAME: blocking: REASON :: TEXT`
/// files a child. A blocking mark with no reason is a nit. `@ target` at
/// the end names the target; otherwise `default_target` is the target.
#[must_use]
pub fn parse_objection(line: &str, default_target: &str) -> Option<Objection> {
    let line = line.trim().trim_start_matches('-').trim();
    let rest = line
        .strip_prefix("finding:")
        .or_else(|| line.strip_prefix("objection:"))
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let (persona, rest) = rest.split_once(':')?;
    let persona = persona.trim();
    if persona.is_empty() {
        return None;
    }
    let rest = rest.trim();
    let (severity, rest) = match rest.split_once(':') {
        Some((word, after)) => match Severity::parse(word) {
            Some(severity) => (severity, after.trim()),
            None => (Severity::Nit, rest),
        },
        None => (Severity::Nit, rest),
    };
    let (reason, rest) = if severity == Severity::Blocking {
        match rest.split_once("::") {
            Some((reason, text)) => (reason.trim().to_string(), text.trim()),
            None => (String::new(), rest),
        }
    } else {
        (String::new(), rest)
    };
    let (text, target) = match rest.rsplit_once(" @ ") {
        Some((text, target)) if !target.trim().is_empty() => (text.trim(), target.trim()),
        _ => (rest, default_target),
    };
    if text.is_empty() {
        return None;
    }
    Some(Objection::noted(persona, severity, &reason, text, target))
}

/// Rounds, children, and optional wall time or token estimate.
/// Unset wall time and tokens do not bind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeBudget {
    pub max_rounds: usize,
    pub max_children: usize,
    pub max_descendants: usize,
    pub max_wall_secs: Option<u64>,
    pub max_tokens: Option<u64>,
}

impl ScopeBudget {
    /// A single panel: three rounds, a handful of children.
    #[must_use]
    pub fn panel() -> Self {
        Self {
            max_rounds: 3,
            max_children: 8,
            max_descendants: 12,
            max_wall_secs: None,
            max_tokens: None,
        }
    }

    /// A feature workflow, which files design and fix children of its own.
    #[must_use]
    pub fn workflow() -> Self {
        Self {
            max_rounds: 3,
            max_children: 24,
            max_descendants: 48,
            max_wall_secs: None,
            max_tokens: None,
        }
    }

    fn overlay(&mut self, file: &ScopeFile) {
        if let Some(n) = file.max_rounds {
            self.max_rounds = n;
        }
        if let Some(n) = file.max_children {
            self.max_children = n;
        }
        if let Some(n) = file.max_descendants {
            self.max_descendants = n;
        }
        if file.max_wall_secs.is_some() {
            self.max_wall_secs = file.max_wall_secs;
        }
        if file.max_tokens.is_some() {
            self.max_tokens = file.max_tokens;
        }
    }
}

/// Budgets for one panel and for a workflow parent. Defaults apply until
/// a toml file says otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelBudget {
    pub panel: ScopeBudget,
    pub workflow: ScopeBudget,
}

impl Default for PanelBudget {
    fn default() -> Self {
        Self {
            panel: ScopeBudget::panel(),
            workflow: ScopeBudget::workflow(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct BudgetFile {
    #[serde(default)]
    panel: ScopeFile,
    #[serde(default)]
    workflow: ScopeFile,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ScopeFile {
    max_rounds: Option<usize>,
    max_children: Option<usize>,
    max_descendants: Option<usize>,
    max_wall_secs: Option<u64>,
    max_tokens: Option<u64>,
}

/// The budget a toml document names, over the defaults. A document with
/// neither table is the defaults. Unknown keys are ignored.
#[must_use]
pub fn budget_from_toml(text: &str) -> PanelBudget {
    let mut budget = PanelBudget::default();
    let Ok(file) = toml::from_str::<BudgetFile>(text) else {
        return budget;
    };
    budget.panel.overlay(&file.panel);
    budget.workflow.overlay(&file.workflow);
    budget
}

/// Config dir first, then `./vissue.toml` and `./ljos.toml`, then
/// `VISSUE_CONFIG` and `LJOS_CONFIG`. A later file overrides the keys it
/// sets. A missing file is skipped. A file that does not parse is skipped.
#[must_use]
pub fn load_budget() -> PanelBudget {
    let mut budget = PanelBudget::default();
    for path in config_paths(None) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = toml::from_str::<BudgetFile>(&text) else {
            continue;
        };
        budget.panel.overlay(&file.panel);
        budget.workflow.overlay(&file.workflow);
    }
    budget
}

/// The config layers in the order [`load_budget`] reads them. `own` adds
/// a file of that name in the ljos config dir, after `ljos.toml`.
pub(crate) fn config_paths(own: Option<&str>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(base) = config_base() {
        paths.push(base.join("vissue").join("config.toml"));
        paths.push(base.join("ljos").join("ljos.toml"));
        if let Some(name) = own {
            paths.push(base.join("ljos").join(name));
        }
    }
    paths.push(PathBuf::from("vissue.toml"));
    paths.push(PathBuf::from("ljos.toml"));
    if let Some(path) = std::env::var_os("VISSUE_CONFIG").filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("LJOS_CONFIG").filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(path));
    }
    paths
}

pub(crate) fn config_base() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".config"))
                .filter(|p| p.is_absolute())
        })
}

/// Where a panel stands when the next round is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelState {
    pub round: usize,
    pub passed: bool,
    /// 0 on the work. 1 on a dissent child, and so on.
    pub dissent_depth: usize,
    pub children_filed: usize,
    pub open_children: usize,
    /// The round has not been sat: ballots are missing. This is not a pause
    /// for a person to raise the round limit.
    pub awaiting_ballots: bool,
    pub seen: Vec<(String, String)>,
    pub wall_secs: Option<u64>,
    pub tokens: Option<u64>,
    pub objections: Vec<Objection>,
}

impl PanelState {
    #[must_use]
    pub fn at_round(round: usize) -> Self {
        Self {
            round,
            passed: false,
            dissent_depth: 0,
            children_filed: 0,
            open_children: 0,
            awaiting_ballots: false,
            seen: Vec::new(),
            wall_secs: None,
            tokens: None,
            objections: Vec::new(),
        }
    }
}

/// What to do with one objection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fate {
    Note(Objection),
    File(Objection),
    Duplicate(Objection),
    /// Would have been a child, and the budget closed the item instead.
    Listed(Objection),
}

/// What one call does with the objections it was shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disposal {
    pub fates: Vec<Fate>,
    pub close: bool,
    pub summary: String,
}

/// The next step of a workflow panel. A close is the step, not an error
/// and not a wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundPlan {
    OpenRound { n: usize },
    FileBlocking { objections: Vec<Objection> },
    Settle { summary: String },
    Hold { why: String },
    Passed,
}

fn budget_why(state: &PanelState, limits: &ScopeBudget) -> Option<String> {
    if state.round > 0 && state.round >= limits.max_rounds {
        return Some(format!(
            "round {} reached the limit of {}",
            state.round, limits.max_rounds
        ));
    }
    if state.children_filed >= limits.max_children {
        return Some(format!(
            "{} children filed, at the limit of {}",
            state.children_filed, limits.max_children
        ));
    }
    if let (Some(used), Some(max)) = (state.wall_secs, limits.max_wall_secs) {
        if used >= max {
            return Some(format!("wall time {used}s reached the limit of {max}s"));
        }
    }
    if let (Some(used), Some(max)) = (state.tokens, limits.max_tokens) {
        if used >= max {
            return Some(format!("token estimate {used} reached the limit of {max}"));
        }
    }
    None
}

fn fresh(state: &PanelState) -> Vec<Objection> {
    let mut batch = state.seen.iter().cloned().collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    for objection in &state.objections {
        let key = objection.key();
        if batch.contains(&key) {
            continue;
        }
        batch.insert(key);
        out.push(objection.clone());
    }
    out
}

fn summary_of(why: &str, listed: &[Objection]) -> String {
    let mut out = format!("settled-with-notes: {why}");
    if listed.is_empty() {
        return out;
    }
    out.push_str(". remaining:");
    for (i, objection) in listed.iter().take(12).enumerate() {
        out.push_str(&format!(
            " ({}) {} {}: {}",
            i + 1,
            objection.persona,
            objection.severity.word(),
            clip(&objection.text, 80)
        ));
    }
    if listed.len() > 12 {
        out.push_str(&format!(" and {} more", listed.len() - 12));
    }
    out
}

fn clip(text: &str, n: usize) -> String {
    let mut out: String = text.chars().take(n).collect();
    if text.chars().count() > n {
        out.push('…');
    }
    out
}

/// Sort the objections on one issue: notes, one child each for what may
/// be filed, duplicates dropped, and a close when the budget is spent.
#[must_use]
pub fn dispose(state: &PanelState, limits: &ScopeBudget) -> Disposal {
    let mut fates = Vec::new();
    let mut batch = state.seen.iter().cloned().collect::<BTreeSet<_>>();
    let mut fileable = Vec::new();
    for objection in &state.objections {
        let key = objection.key();
        if batch.contains(&key) {
            fates.push(Fate::Duplicate(objection.clone()));
            continue;
        }
        batch.insert(key);
        if objection.may_file(state.passed, state.dissent_depth) {
            fileable.push(objection.clone());
        } else {
            fates.push(Fate::Note(objection.clone()));
        }
    }
    if let Some(why) = budget_why(state, limits) {
        for objection in &fileable {
            fates.push(Fate::Listed(objection.clone()));
        }
        return Disposal {
            fates,
            close: true,
            summary: summary_of(&why, &fileable),
        };
    }
    let room = limits.max_children.saturating_sub(state.children_filed);
    let mut listed = Vec::new();
    for (i, objection) in fileable.into_iter().enumerate() {
        if i < room {
            fates.push(Fate::File(objection));
        } else {
            listed.push(objection);
        }
    }
    let close = !listed.is_empty();
    let summary = if close {
        summary_of(
            &format!(
                "{} children filed, at the limit of {}",
                state.children_filed + room,
                limits.max_children
            ),
            &listed,
        )
    } else {
        String::new()
    };
    for objection in listed {
        fates.push(Fate::Listed(objection));
    }
    Disposal {
        fates,
        close,
        summary,
    }
}

/// The next workflow step. Open children and uncast ballots hold. A
/// blocking objection files one child, including after the substance has
/// passed, and the next round waits until that child is closed. A pass
/// with nothing blocking left is a pass. A round of nits closes. At the
/// round limit a blocking objection is listed, and a pass with nothing
/// left to file stays a pass.
#[must_use]
pub fn plan_round(state: &PanelState, limits: &ScopeBudget) -> RoundPlan {
    if state.open_children > 0 {
        return RoundPlan::Hold {
            why: format!(
                "{} {} still open",
                state.open_children,
                if state.open_children == 1 {
                    "child is"
                } else {
                    "children are"
                }
            ),
        };
    }
    if state.awaiting_ballots {
        return RoundPlan::Hold {
            why: "the panel has not all voted".into(),
        };
    }
    let mut fileable = Vec::new();
    let mut seen = state.seen.iter().cloned().collect::<BTreeSet<_>>();
    let mut addressed = 0;
    for objection in &state.objections {
        let key = objection.key();
        if seen.contains(&key) {
            if objection.files_as_child() {
                addressed += 1;
            }
            continue;
        }
        seen.insert(key);
        if objection.may_file(state.passed, state.dissent_depth) {
            fileable.push(objection.clone());
        }
    }
    if let Some(why) = budget_why(state, limits) {
        if state.passed && fileable.is_empty() {
            return RoundPlan::Passed;
        }
        return RoundPlan::Settle {
            summary: summary_of(&why, &fresh(state)),
        };
    }
    if !fileable.is_empty() {
        let room = limits.max_children.saturating_sub(state.children_filed);
        if room == 0 {
            return RoundPlan::Settle {
                summary: summary_of(
                    &format!(
                        "{} children filed, at the limit of {}",
                        state.children_filed, limits.max_children
                    ),
                    &fileable,
                ),
            };
        }
        fileable.truncate(room);
        return RoundPlan::FileBlocking {
            objections: fileable,
        };
    }
    if state.passed {
        return RoundPlan::Passed;
    }
    if state.round == 0 {
        return RoundPlan::OpenRound { n: 1 };
    }
    if addressed > 0 && state.round < limits.max_rounds {
        return RoundPlan::OpenRound { n: state.round + 1 };
    }
    let listed = fresh(state);
    let why = if listed.is_empty() {
        "no blocking objection remains".to_string()
    } else {
        "the objections are below blocking".to_string()
    };
    RoundPlan::Settle {
        summary: summary_of(&why, &listed),
    }
}

/// Whether `pass` is the unique strict majority, or, on any other ballot,
/// one option leads by a strict majority. A tie has not settled.
#[must_use]
pub fn substance_passed(choices: &[(String, String)]) -> bool {
    if choices.is_empty() {
        return false;
    }
    let norm = |s: &str| s.trim().to_ascii_lowercase();
    let pass = choices.iter().filter(|(_, c)| norm(c) == "pass").count();
    let revise = choices.iter().filter(|(_, c)| norm(c) == "revise").count();
    if pass + revise == choices.len() {
        return pass > revise && pass * 2 > choices.len();
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, choice) in choices {
        *counts.entry(normalize(choice)).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.len() == 1 || (ranked[0].1 > ranked[1].1 && ranked[0].1 * 2 > choices.len())
}

/// True when two choices share the highest count. A single ballot, and a
/// strict lead, are not ties.
fn ballot_tie(choices: &[(String, String)]) -> bool {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, choice) in choices {
        *counts.entry(normalize(choice)).or_default() += 1;
    }
    let Some(max) = counts.values().copied().max() else {
        return false;
    };
    counts.values().filter(|n| **n == max).count() > 1
}

/// Findings on an issue JSON: logbook notes and body lines.
#[must_use]
pub fn objections_in(v: &Value, default_target: &str) -> Vec<Objection> {
    let mut out = Vec::new();
    let mut push = |text: &str| {
        if let Some(objection) = parse_objection(text, default_target) {
            out.push(objection);
        }
    };
    for entry in v["logbook"].as_array().into_iter().flatten() {
        if let Some(note) = entry["note"].as_str() {
            push(note);
        }
        if let Some(raw) = entry["raw"].as_str() {
            push(raw);
        }
    }
    if let Some(body) = v["body"].as_str() {
        for line in body.lines() {
            push(line);
        }
    }
    out
}

/// Depth of a dissent child: the `dissent-depth: N` note, else 1 when the
/// title is a dissent, else 0.
#[must_use]
pub fn dissent_depth(v: &Value) -> usize {
    for entry in v["logbook"].as_array().into_iter().flatten() {
        let Some(note) = entry["note"].as_str() else {
            continue;
        };
        if let Some(rest) = note.trim().strip_prefix("dissent-depth:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                return n;
            }
        }
    }
    let title = v["title"].as_str().unwrap_or("");
    if title.trim_start().starts_with("Dissent holds") {
        1
    } else {
        0
    }
}

/// The text a filed child carries, so a later round recognises it.
#[must_use]
pub fn filed_key(title: &str, target: &str) -> Option<(String, String)> {
    let title = title.trim();
    let text = title
        .strip_prefix("Dissent holds ")
        .or_else(|| title.strip_prefix("fix: "))
        .or_else(|| title.strip_prefix("fix:"))?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some((dissent_key_text(text), normalize(target)))
}

fn project_of(v: &Value, id: &str) -> String {
    v["project"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| id.rsplit_once('-').map(|(p, _)| p.to_string()))
        .unwrap_or_default()
}

/// Options a voter still holds that are not the unique lead. A tie, a
/// single option, and an empty ballot name nothing: there is no minority
/// to file.
fn minority_options(choices: &[(String, String)]) -> Vec<String> {
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for (_, choice) in choices {
        let shown = choice.trim();
        let key = normalize(shown);
        if key.is_empty() {
            continue;
        }
        let row = counts.entry(key).or_insert_with(|| (shown.to_string(), 0));
        row.1 += 1;
    }
    let Some(max) = counts.values().map(|(_, n)| *n).max() else {
        return Vec::new();
    };
    if counts.values().filter(|(_, n)| *n == max).count() != 1 {
        return Vec::new();
    }
    let mut out: Vec<String> = counts
        .into_values()
        .filter(|(_, n)| *n < max)
        .map(|(shown, _)| shown)
        .collect();
    out.sort();
    out
}

/// One `Dissent holds OPTION` child per minority on a settled majority.
/// A child already filed for that option is named and left alone.
fn file_settled_minority(
    issue: &str,
    project: &str,
    choices: &[(String, String)],
    children: &[String],
    depth: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    for option in minority_options(choices) {
        let title = format!("Dissent holds {option}");
        if children.iter().any(|t| normalize(t) == normalize(&title)) {
            lines.push(format!("{issue}: dissent {option} already filed"));
            continue;
        }
        match file_child(issue, project, &title, depth) {
            Ok(id) => lines.push(format!("filed {id}  TODO  [#C]  {title}")),
            Err(e) => lines.push(format!("dissent: file refused: {e:#}")),
        }
    }
    lines
}

/// The note a filing `ljos consensus` writes for each round it sits.
pub const ROUND_NOTE: &str = "panel-round:";

/// The highest round noted on an issue JSON as `panel-round: N`, 0 with none.
#[must_use]
pub fn recorded_round(v: &Value) -> usize {
    v["logbook"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["note"].as_str())
        .filter_map(|n| {
            n.trim()
                .strip_prefix(ROUND_NOTE)?
                .trim()
                .parse::<usize>()
                .ok()
        })
        .max()
        .unwrap_or(0)
}

fn is_closed(v: &Value) -> bool {
    matches!(v["state"].as_str(), Some("DONE" | "CANCELLED"))
}

/// The round this reading sits on the issue, and whether to note it. A
/// `panel rN` title is round N, as `ljos workflow --round` filed it, and
/// needs no note. Any other title sits one round past the last
/// `panel-round: N` note. A closed issue sits no new round.
#[must_use]
pub fn sitting_round(v: &Value) -> (usize, bool) {
    if let Some(n) = crate::panel_round_number(v["title"].as_str().unwrap_or("")) {
        return (n, false);
    }
    let last = recorded_round(v);
    if is_closed(v) {
        return (last, false);
    }
    (last + 1, true)
}

/// Open `Dissent holds` and `fix:` children of `issue`. A missing state
/// counts as open: a stub that lists only the id and the title has not
/// closed the child.
fn open_objection_children(issue: &str) -> Vec<(String, String)> {
    let Ok(said) = crate::run_captured("vissue", &["children", issue, "--json"]) else {
        return Vec::new();
    };
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(&said.stdout) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        let Some(id) = row["id"].as_str() else {
            continue;
        };
        let title = row["title"].as_str().map(str::to_string).or_else(|| {
            crate::issue_record(id)
                .ok()
                .and_then(|v| v["title"].as_str().map(str::to_string))
        });
        let Some(title) = title else {
            continue;
        };
        let titled = title.trim_start();
        if !titled.starts_with("Dissent holds") && !titled.starts_with("fix:") {
            continue;
        }
        let state = row["state"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                crate::issue_record(id)
                    .ok()
                    .and_then(|v| v["state"].as_str().map(str::to_string))
            })
            .unwrap_or_default();
        if matches!(state.as_str(), "DONE" | "CANCELLED") {
            continue;
        }
        out.push((id.to_string(), title));
    }
    out
}

/// The line a split reading prints. The round number is a count. It does
/// not close the ticket.
fn split_advice(issue: &str, round: usize) -> String {
    let open = open_objection_children(issue);
    if open.is_empty() {
        return format!("seats still split at round {round}; open objections: none");
    }
    let named = open
        .iter()
        .map(|(id, title)| format!("{id} {title}"))
        .collect::<Vec<_>>()
        .join("; ");
    format!("seats still split at round {round}; open objections: {named}")
}

/// File dissent children and note the rest for `issue`. A blocking
/// objection with a reason is one `Dissent holds TEXT` child. A settled
/// majority still files one `Dissent holds OPTION` child for each option a
/// voter holds that is not the lead. A tie files none. Each call sits one
/// panel round ([`sitting_round`]). A round count does not change the
/// ticket state. When the seats still split, the reading names the open
/// objection children. The person closes with `ljos finish --close`.
/// A tracker that does not answer leaves the reading as it was: filing
/// is a side effect.
#[must_use]
pub fn record_dissents(issue: &str) -> Vec<String> {
    let Ok(v) = crate::issue_record(issue) else {
        return Vec::new();
    };
    let closed = is_closed(&v);
    let (round, note_round) = sitting_round(&v);
    let mut round_lines = Vec::new();
    if note_round {
        if let Err(e) = note_issue(issue, &format!("{ROUND_NOTE} {round}")) {
            round_lines.push(format!("dissent: round note refused: {e:#}"));
        }
    }
    let choices: Vec<(String, String)> = crate::ballot_rows(issue)
        .into_iter()
        .filter_map(|row| {
            let agent = row.get("agent")?.as_str()?.to_string();
            let choice = row.get("choice")?.as_str()?.to_string();
            Some((agent, choice))
        })
        .collect();
    let passed = substance_passed(&choices);
    // A tie has no held side to file. Both options lead, so a dissent child
    // for each would be the settle saying it could not choose.
    let tied = ballot_tie(&choices);
    let mut objections = objections_in(&v, issue);
    let noted: BTreeSet<(String, String)> = objections.iter().map(Objection::key).collect();
    if !passed && !tied {
        for (persona, choice) in &choices {
            if normalize(choice) == "pass" {
                continue;
            }
            let mut held = Objection::nit(persona, &format!("holds {choice}"), issue);
            held.held = true;
            if !noted.contains(&held.key())
                && !objections
                    .iter()
                    .any(|o| o.persona == *persona && o.files_as_child())
            {
                objections.push(held);
            }
        }
    }
    let children = child_titles(issue);
    let mut seen = Vec::new();
    let mut children_filed = 0;
    for title in &children {
        if let Some(key) = filed_key(title, issue) {
            seen.push(key);
            children_filed += 1;
        }
    }
    let state = PanelState {
        round,
        passed,
        dissent_depth: dissent_depth(&v),
        children_filed,
        open_children: 0,
        awaiting_ballots: false,
        seen,
        wall_secs: None,
        tokens: None,
        objections,
    };
    let limits = load_budget().panel;
    let disposal = dispose(&state, &limits);
    let mut lines = round_lines;
    let project = project_of(&v, issue);
    let depth = state.dissent_depth;
    for fate in disposal.fates {
        match fate {
            Fate::Duplicate(_) => {}
            Fate::Note(objection) if noted.contains(&objection.key()) => {}
            Fate::Note(objection) => match note_issue(issue, &format_note(&objection)) {
                Ok(()) => lines.push(format!(
                    "noted {} from {} on {}: {}",
                    objection.severity.word(),
                    objection.persona,
                    objection.target,
                    objection.text
                )),
                Err(e) => lines.push(format!("dissent: note refused: {e:#}")),
            },
            Fate::File(objection) => {
                let title = dissent_title(&objection);
                match file_child(issue, &project, &title, depth + 1) {
                    Ok(id) => lines.push(format!("filed {id}  TODO  [#C]  {title}")),
                    Err(e) => lines.push(format!("dissent: file refused: {e:#}")),
                }
            }
            Fate::Listed(objection) => lines.push(format!(
                "listed {} from {}: {}",
                objection.severity.word(),
                objection.persona,
                objection.text
            )),
        }
    }
    // A round count, a child count, or a spent budget used to mark the
    // ticket DONE here. The state stays whatever the tracker had. When
    // the seats still split, name the open objection children.
    if !passed && !closed {
        lines.push(split_advice(issue, round));
    }
    // A strict majority has settled the substance. The option a voter
    // still holds, and that is not the lead, is still a dissent child.
    // The tie never reaches here with a minority to name.
    if passed {
        lines.extend(file_settled_minority(
            issue,
            &project,
            &choices,
            &children,
            depth + 1,
        ));
    }
    lines
}

fn format_note(objection: &Objection) -> String {
    if objection.files_as_child() {
        format!(
            "finding: {}: blocking: {} :: {} @ {}",
            objection.persona, objection.reason, objection.text, objection.target
        )
    } else {
        format!(
            "finding: {}: {}: {} @ {}",
            objection.persona,
            objection.severity.word(),
            objection.text,
            objection.target
        )
    }
}

fn dissent_title(objection: &Objection) -> String {
    let text = clip(&objection.text, DISSENT_TITLE_LIMIT);
    format!("Dissent holds {text}")
}

fn child_titles(issue: &str) -> Vec<String> {
    let Ok(said) = crate::run_captured("vissue", &["children", issue, "--json"]) else {
        return Vec::new();
    };
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(&said.stdout) else {
        return Vec::new();
    };
    rows.into_iter()
        .filter_map(|row| {
            let title = row["title"].as_str().map(str::to_string);
            if title.is_some() {
                return title;
            }
            let id = row["id"].as_str()?;
            crate::issue_record(id)
                .ok()
                .and_then(|v| v["title"].as_str().map(str::to_string))
        })
        .collect()
}

fn file_child(parent: &str, project: &str, title: &str, depth: usize) -> Result<String> {
    let mut args = vec!["q".to_string()];
    if !project.is_empty() {
        args.push("-p".into());
        args.push(project.to_string());
    }
    args.extend([
        "--parent".into(),
        parent.to_string(),
        "-t".into(),
        "task".into(),
        title.to_string(),
    ]);
    let said = crate::run_captured("vissue", &args)?;
    let id = said.stdout.trim().to_string();
    if id.is_empty() {
        anyhow::bail!("vissue q printed no id");
    }
    let _ = crate::run_captured("vissue", &["note", &id, &format!("dissent-depth: {depth}")]);
    Ok(id)
}

fn note_issue(id: &str, text: &str) -> Result<()> {
    crate::run_captured("vissue", &["note", id, text])?;
    Ok(())
}

/// Note the summary and mark the issue done. The close is the whole step.
pub fn close_settled(id: &str, summary: &str) -> Result<String> {
    note_issue(id, summary).with_context(|| format!("note {id}"))?;
    crate::run_captured("vissue", &["update", id, "-s", "DONE"])
        .with_context(|| format!("close {id}"))?;
    let mut out = format!("{id} closed as {summary}\n");
    out.push_str(&crate::persist_tracker(id, "settled with notes"));
    Ok(out)
}

/// File one `fix:` child for a blocking objection, for `ljos workflow
/// --round`. The review of it is the next panel round, not a second child.
/// `ljos consensus` does not call this: it files `Dissent holds TEXT`
/// through [`record_dissents`].
pub fn file_fix(parent: &str, project: &str, objection: &Objection) -> Result<String> {
    let title = format!("fix: {}", clip(&objection.text, DISSENT_TITLE_LIMIT));
    let id = crate::workflow_child(parent, project, "task", crate::Stage::Implement, &title)?;
    let _ = note_issue(
        &id,
        &format!(
            "finding: {}: blocking: {} :: {} @ {}",
            objection.persona, objection.reason, objection.text, parent
        ),
    );
    Ok(id)
}

/// One issue in an export, enough to see a tree running away.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    id: String,
    parent: Option<String>,
    title: String,
    open: bool,
    blocking: usize,
}

/// Lines for trees that have run away: more descendants than the budget,
/// or panel rounds that grew without a new blocking objection. The top of
/// the tree is named, not every child. `apply` closes each flagged issue
/// as settled-with-notes.
#[must_use]
pub fn reap_runaways(apply: bool) -> String {
    let Ok(said) = crate::run_captured("vissue", &["export"]) else {
        return String::new();
    };
    let budget = load_budget();
    let mut out = String::new();
    for (id, why) in flag_runaways(&said.stdout, &budget) {
        out.push_str(&format!("runaway: {id} {why}\n"));
        if !apply {
            continue;
        }
        let summary = format!("settled-with-notes: runaway, {why}");
        match close_settled(&id, &summary) {
            Ok(said) => out.push_str(&said),
            Err(e) => out.push_str(&format!("reap: {id}: close refused: {e:#}\n")),
        }
    }
    out
}

/// Runaway trees in a `vissue export` (one JSON issue a line).
#[must_use]
pub fn flag_runaways(export: &str, budget: &PanelBudget) -> Vec<(String, String)> {
    let nodes = nodes_of(export);
    let children = children_of(&nodes);
    let mut desc_memo: BTreeMap<String, usize> = BTreeMap::new();
    let mut round_memo: BTreeMap<String, usize> = BTreeMap::new();
    let mut block_memo: BTreeMap<String, usize> = BTreeMap::new();
    let mut flags = Vec::new();
    for node in &nodes {
        if !node.open {
            continue;
        }
        let limits = if is_workflow(&nodes, &node.id) {
            &budget.workflow
        } else {
            &budget.panel
        };
        let desc = descendants(&node.id, &children, &mut desc_memo, &mut BTreeSet::new());
        let parent_over = node.parent.as_ref().is_some_and(|p| {
            let parent_limits = if is_workflow(&nodes, p) {
                &budget.workflow
            } else {
                &budget.panel
            };
            descendants(p, &children, &mut desc_memo, &mut BTreeSet::new())
                > parent_limits.max_descendants
        });
        if desc > limits.max_descendants && !parent_over {
            flags.push((
                node.id.clone(),
                format!(
                    "has {desc} descendants, over the limit of {}",
                    limits.max_descendants
                ),
            ));
        }
        let round = max_round(
            &node.id,
            &nodes,
            &children,
            &mut round_memo,
            &mut BTreeSet::new(),
        );
        let blocking = blocking_under(
            &node.id,
            &nodes,
            &children,
            &mut block_memo,
            &mut BTreeSet::new(),
        );
        let grew = round >= 2 && round > blocking + 1;
        let parent_grew = node.parent.as_ref().is_some_and(|p| {
            let parent_round =
                max_round(p, &nodes, &children, &mut round_memo, &mut BTreeSet::new());
            let parent_blocking =
                blocking_under(p, &nodes, &children, &mut block_memo, &mut BTreeSet::new());
            parent_round >= round && parent_round >= 2 && parent_round > parent_blocking + 1
        });
        if grew && !parent_grew {
            flags.push((
                node.id.clone(),
                format!(
                    "reached round {round} with {blocking} blocking {}; rounds grew without a new blocking objection",
                    if blocking == 1 { "objection" } else { "objections" }
                ),
            ));
        }
    }
    flags
}

fn is_workflow(nodes: &[Node], id: &str) -> bool {
    nodes.iter().any(|n| {
        n.id == id
            && (n.title.contains("workflow")
                || n.title.starts_with("design:")
                || n.title.starts_with("panel r"))
    }) || nodes
        .iter()
        .any(|n| n.parent.as_deref() == Some(id) && crate::panel_round_number(&n.title).is_some())
}

fn nodes_of(export: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    for line in export.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(id) = v["id"].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        let state = v["state"].as_str().unwrap_or("");
        let open = !matches!(state, "DONE" | "CANCELLED");
        let blocking = objections_in(&v, id)
            .iter()
            .filter(|o| o.files_as_child())
            .count();
        nodes.push(Node {
            id: id.to_string(),
            parent: v["parent"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            title: v["title"].as_str().unwrap_or("").to_string(),
            open,
            blocking,
        });
    }
    nodes
}

fn children_of(nodes: &[Node]) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in nodes {
        if let Some(parent) = &node.parent {
            map.entry(parent.clone()).or_default().push(node.id.clone());
        }
    }
    map
}

fn descendants(
    id: &str,
    children: &BTreeMap<String, Vec<String>>,
    memo: &mut BTreeMap<String, usize>,
    stack: &mut BTreeSet<String>,
) -> usize {
    if let Some(n) = memo.get(id) {
        return *n;
    }
    if !stack.insert(id.to_string()) {
        return 0;
    }
    let n = children.get(id).map_or(0, |kids| {
        kids.iter()
            .map(|kid| 1 + descendants(kid, children, memo, stack))
            .sum()
    });
    stack.remove(id);
    memo.insert(id.to_string(), n);
    n
}

fn max_round(
    id: &str,
    nodes: &[Node],
    children: &BTreeMap<String, Vec<String>>,
    memo: &mut BTreeMap<String, usize>,
    stack: &mut BTreeSet<String>,
) -> usize {
    if let Some(n) = memo.get(id) {
        return *n;
    }
    if !stack.insert(id.to_string()) {
        return 0;
    }
    let own = nodes
        .iter()
        .find(|n| n.id == id)
        .map(|n| crate::panel_round_number(&n.title).unwrap_or(0))
        .unwrap_or(0);
    let below = children.get(id).map_or(0, |kids| {
        kids.iter()
            .map(|kid| max_round(kid, nodes, children, memo, stack))
            .max()
            .unwrap_or(0)
    });
    stack.remove(id);
    let n = own.max(below);
    memo.insert(id.to_string(), n);
    n
}

fn blocking_under(
    id: &str,
    nodes: &[Node],
    children: &BTreeMap<String, Vec<String>>,
    memo: &mut BTreeMap<String, usize>,
    stack: &mut BTreeSet<String>,
) -> usize {
    if let Some(n) = memo.get(id) {
        return *n;
    }
    if !stack.insert(id.to_string()) {
        return 0;
    }
    let own = nodes
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.blocking)
        .unwrap_or(0);
    let below: usize = children.get(id).map_or(0, |kids| {
        kids.iter()
            .map(|kid| blocking_under(kid, nodes, children, memo, stack))
            .sum()
    });
    stack.remove(id);
    let n = own + below;
    memo.insert(id.to_string(), n);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nit(n: usize) -> Objection {
        Objection::nit("ceo", &format!("comma in note {n}"), "anneal-z64x")
    }

    #[test]
    fn the_round_comes_from_the_tracker_not_only_the_title() {
        // The panel eval's twelve decisions had question titles, so the
        // closer read round 0 and log-sink stayed TODO after three rounds.
        let mut v = serde_json::json!({
            "id": "panel-a7o4",
            "title": "log-sink: where the log goes",
            "state": "TODO",
            "logbook": [],
        });
        assert_eq!(sitting_round(&v), (1, true));
        v["logbook"] = serde_json::json!([
            {"note": "panel-round: 2"},
            {"note": "finding: keep: blocking: stderr is what ops reads :: keep stderr"},
            {"note": "panel-round: 1"},
        ]);
        assert_eq!(recorded_round(&v), 2);
        assert_eq!(sitting_round(&v), (3, true));
        assert!(objections_in(&v, "panel-a7o4")
            .iter()
            .all(|o| !o.text.contains("panel-round")));
        let mut state = PanelState::at_round(sitting_round(&v).0);
        state.passed = true;
        let disposal = dispose(&state, &ScopeBudget::panel());
        assert!(disposal.close, "{disposal:?}");
        assert_eq!(
            disposal.summary,
            "settled-with-notes: round 3 reached the limit of 3"
        );
        // A closed issue sits no new round.
        v["state"] = serde_json::json!("DONE");
        assert_eq!(sitting_round(&v), (2, false));
        // A `panel rN` title is the round `ljos workflow --round` filed.
        let titled = serde_json::json!({
            "id": "panel-hrd5",
            "title": "panel r3: cache place",
            "state": "TODO",
            "logbook": [{"note": "panel-round: 7"}],
        });
        assert_eq!(sitting_round(&titled), (3, false));
    }

    #[test]
    fn a_blocking_mark_without_a_reason_is_a_nit() {
        let line = "finding: tester: blocking: the comma is wrong";
        let got = parse_objection(line, "anneal-z64x").unwrap();
        assert_eq!(got.severity, Severity::Nit);
        assert!(!got.files_as_child());
        let marked = parse_objection(
            "finding: tester: blocking: the exit status is dropped :: the gate ignores a failure @ anneal-z64x",
            "other",
        )
        .unwrap();
        assert!(marked.files_as_child());
        assert_eq!(marked.reason, "the exit status is dropped");
        assert_eq!(marked.text, "the gate ignores a failure");
        assert_eq!(marked.target, "anneal-z64x");
        let plain = parse_objection("finding: cto: heading tone", "anneal-z64x").unwrap();
        assert_eq!(plain.severity, Severity::Nit);
        assert_eq!(plain.text, "heading tone");
        let wording =
            parse_objection("finding: cto: wording: heading tone", "anneal-z64x").unwrap();
        assert_eq!(wording.severity, Severity::Wording);
        assert!(parse_objection("not a finding", "x").is_none());
    }

    #[test]
    fn the_same_text_and_target_are_one_objection() {
        let a = Objection::nit("ceo", "Heading, tone!", "anneal-z64x");
        let b = Objection::wording("cto", "heading tone", "Anneal-z64x");
        assert_eq!(a.key(), b.key());
        let other = Objection::nit("ceo", "heading tone", "anneal-other");
        assert_ne!(a.key(), other.key());
    }

    #[test]
    fn an_endless_stream_of_nits_after_a_pass_files_nothing() {
        let limits = ScopeBudget::panel();
        let mut state = PanelState::at_round(1);
        state.passed = true;
        let mut filed = 0;
        let mut opened = 0;
        for n in 0..10 {
            state.objections.push(nit(n));
            state
                .objections
                .push(Objection::wording("cto", "heading tone", "anneal-z64x"));
            state.objections.push(Objection::style(
                "junior-dev",
                "whitespace in the comment",
                "anneal-z64x",
            ));
            match plan_round(&state, &limits) {
                RoundPlan::FileBlocking { objections } => filed += objections.len(),
                RoundPlan::OpenRound { n } => {
                    opened += 1;
                    state.round = n;
                }
                RoundPlan::Settle { summary } => {
                    panic!("a pass does not need a close to stop: {summary}");
                }
                RoundPlan::Passed => {}
                RoundPlan::Hold { why } => panic!("a pass does not hold: {why}"),
            }
        }
        assert_eq!(filed, 0, "nits after a pass are notes");
        assert_eq!(opened, 0, "a pass does not open another round");
        assert!(state.round <= limits.max_rounds);

        // The anneal as it actually ran: revise votes on wording, so the
        // round does not read as passed, and the agent asks for another.
        let mut state = PanelState::at_round(1);
        let mut filed = 0;
        let mut opened = 0;
        let mut settled = 0;
        let mut last = String::new();
        for n in 0..10 {
            state.objections.push(nit(n));
            state
                .objections
                .push(Objection::wording("cto", "heading tone", "anneal-z64x"));
            match plan_round(&state, &limits) {
                RoundPlan::FileBlocking { objections } => filed += objections.len(),
                RoundPlan::OpenRound { n } => {
                    opened += 1;
                    state.round = n;
                }
                RoundPlan::Settle { summary } => {
                    settled += 1;
                    last = summary;
                }
                RoundPlan::Passed | RoundPlan::Hold { .. } => {}
            }
        }
        assert_eq!(filed, 0);
        assert_eq!(opened, 0);
        assert!(settled >= 1);
        assert!(state.round <= limits.max_rounds);
        assert!(last.contains("settled-with-notes"), "{last}");
        assert!(last.contains("comma in note 0"), "{last}");
        assert!(last.contains("heading tone"), "{last}");
        // The repeated wording is one objection, not one per round.
        assert_eq!(last.matches("heading tone").count(), 1, "{last}");
    }

    #[test]
    fn a_blocking_dissent_files_a_child_and_a_nit_does_not_file_a_grandchild() {
        let limits = ScopeBudget::panel();
        let mut state = PanelState::at_round(1);
        state.passed = true;
        state.objections.push(Objection::blocking(
            "tester",
            "the gate drops a real failure",
            "the exit status is ignored",
            "anneal-z64x",
        ));
        state
            .objections
            .push(Objection::nit("ceo", "comma in note 0", "anneal-z64x"));
        let plan = plan_round(&state, &limits);
        let RoundPlan::FileBlocking { objections } = plan else {
            panic!("a blocking dissent is a child: {plan:?}");
        };
        assert_eq!(objections.len(), 1);
        assert!(objections[0].text.contains("exit status"));

        let mut child = PanelState::at_round(1);
        child.dissent_depth = 1;
        child.passed = true;
        child
            .objections
            .push(Objection::nit("ceo", "rename the local", "anneal-child"));
        child
            .objections
            .push(Objection::wording("cto", "heading tone", "anneal-child"));
        match plan_round(&child, &limits) {
            RoundPlan::Passed | RoundPlan::Settle { .. } => {}
            other => panic!("a dissent child does not file a nit grandchild: {other:?}"),
        }
        let mut deeper = child.clone();
        deeper.passed = true;
        deeper.objections.push(Objection::blocking(
            "tester",
            "the child still drops the status",
            "the wait is unchecked",
            "anneal-child",
        ));
        match plan_round(&deeper, &limits) {
            RoundPlan::FileBlocking { objections } => {
                assert_eq!(objections.len(), 1);
                assert!(objections[0].text.contains("unchecked"));
            }
            other => panic!("a blocking objection on a dissent child is still a child: {other:?}"),
        }
    }

    #[test]
    fn new_blocking_objections_stop_at_the_round_limit() {
        let limits = ScopeBudget::panel();
        let mut state = PanelState::at_round(1);
        let mut filed = Vec::new();
        for n in 0..6 {
            state.objections.push(Objection::blocking(
                "tester",
                &format!("failure {n} escapes"),
                &format!("bug {n} is still open"),
                "anneal-z64x",
            ));
            state.open_children = 0;
            match plan_round(&state, &limits) {
                RoundPlan::FileBlocking { objections } => {
                    assert_eq!(objections.len(), 1, "{objections:?}");
                    let objection = &objections[0];
                    filed.push(objection.text.clone());
                    state.seen.push(objection.key());
                    state.children_filed += 1;
                    match plan_round(&state, &limits) {
                        RoundPlan::OpenRound { n } => state.round = n,
                        RoundPlan::Settle { summary } => {
                            assert!(summary.contains("settled-with-notes"), "{summary}");
                            break;
                        }
                        other => panic!("after the fix closed: {other:?}"),
                    }
                }
                RoundPlan::Settle { summary } => {
                    assert!(summary.contains("settled-with-notes"), "{summary}");
                    assert!(summary.contains("bug"), "{summary}");
                    break;
                }
                other => panic!("step {n}: {other:?}"),
            }
        }
        assert!(state.round <= limits.max_rounds, "round {}", state.round);
        assert!(filed.len() < 6, "filed without a cap: {filed:?}");
        assert!(filed.iter().any(|text| text.contains("bug 0")), "{filed:?}");
        assert!(
            !filed.iter().any(|text| text.contains("bug 5")),
            "the later bugs are listed, not filed: {filed:?}"
        );
    }

    #[test]
    fn a_wall_budget_closes_without_filing() {
        let mut limits = ScopeBudget::panel();
        limits.max_wall_secs = Some(60);
        let mut state = PanelState::at_round(1);
        state.wall_secs = Some(120);
        state.objections.push(Objection::blocking(
            "tester",
            "a real failure",
            "the exit status is ignored",
            "anneal-z64x",
        ));
        match plan_round(&state, &limits) {
            RoundPlan::Settle { summary } => {
                assert!(summary.contains("wall time"), "{summary}");
                assert!(summary.contains("exit status"), "{summary}");
            }
            other => panic!("over the wall budget the objection is listed: {other:?}"),
        }
    }

    #[test]
    fn a_settled_majority_names_only_the_minority() {
        let majority = vec![
            ("alice".to_string(), "drop".to_string()),
            ("bob".to_string(), "drop".to_string()),
            ("keeper".to_string(), "keep".to_string()),
        ];
        assert_eq!(minority_options(&majority), vec!["keep".to_string()]);
        let tie = vec![
            ("alice".to_string(), "yes".to_string()),
            ("bob".to_string(), "no".to_string()),
        ];
        assert!(minority_options(&tie).is_empty());
        let unanimous = vec![
            ("alice".to_string(), "ship".to_string()),
            ("bob".to_string(), "ship".to_string()),
        ];
        assert!(minority_options(&unanimous).is_empty());
    }

    #[test]
    fn a_duplicate_is_not_filed_again() {
        let limits = ScopeBudget::panel();
        let mut state = PanelState::at_round(1);
        let objection = Objection::blocking(
            "tester",
            "the gate drops a real failure",
            "the exit status is ignored",
            "anneal-z64x",
        );
        state.seen.push(objection.key());
        state.objections.push(objection);
        state.objections.push(Objection::blocking(
            "cto",
            "the gate drops a real failure",
            "The exit status is ignored.",
            "anneal-z64x",
        ));
        let disposal = dispose(&state, &limits);
        assert!(disposal
            .fates
            .iter()
            .all(|f| matches!(f, Fate::Duplicate(_))));
        assert!(!disposal.close);
    }

    #[test]
    fn a_long_blocking_finding_read_three_times_files_one_child() {
        let finding = "x".repeat(181);
        assert_eq!(finding.chars().count(), 181);
        let objection = Objection::blocking("tester", "this blocks merge", &finding, "panel-long");
        let title = dissent_title(&objection);
        let key = filed_key(&title, "panel-long").expect("dissent title yields a filed key");
        assert_eq!(key, objection.key());

        let mut state = PanelState::at_round(1);
        state.objections = vec![objection.clone()];
        let limits = ScopeBudget::panel();
        let mut filed = 0;
        for read in 0..3 {
            state.seen = if read == 0 {
                Vec::new()
            } else {
                vec![key.clone()]
            };
            let disposal = dispose(&state, &limits);
            filed += disposal
                .fates
                .iter()
                .filter(|f| matches!(f, Fate::File(_)))
                .count();
        }
        assert_eq!(filed, 1);
    }

    #[test]
    fn toml_overlays_the_defaults_and_leaves_the_other_scope() {
        let budget = budget_from_toml(
            "[panel]\nmax_rounds = 2\nmax_tokens = 1000\n[workflow]\nmax_children = 4\n",
        );
        assert_eq!(budget.panel.max_rounds, 2);
        assert_eq!(budget.panel.max_children, ScopeBudget::panel().max_children);
        assert_eq!(budget.panel.max_tokens, Some(1000));
        assert_eq!(budget.workflow.max_children, 4);
        assert_eq!(budget.workflow.max_rounds, 3);
        assert_eq!(budget_from_toml("not toml"), PanelBudget::default());
    }

    #[test]
    fn reap_flags_a_wide_tree_and_rounds_with_no_new_blocking_objection() {
        let budget = PanelBudget::default();
        let mut lines = Vec::new();
        lines.push(
            r#"{"id":"anneal-z64x","title":"anneal the notes","state":"STARTED","parent":""}"#
                .to_string(),
        );
        for n in 1..=10 {
            lines.push(format!(
                r#"{{"id":"anneal-r{n}","title":"panel r{n}: anneal","state":"DONE","parent":"anneal-z64x"}}"#
            ));
        }
        for n in 0..60 {
            lines.push(format!(
                r#"{{"id":"anneal-c{n}","title":"fix: comma {n}","state":"TODO","parent":"anneal-z64x"}}"#
            ));
        }
        // 70 descendants and round 10, with no blocking note.
        let flags = flag_runaways(&lines.join("\n"), &budget);
        let ids: Vec<&str> = flags.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"anneal-z64x"), "{flags:?}");
        let why = flags
            .iter()
            .filter(|(id, _)| id == "anneal-z64x")
            .map(|(_, why)| why.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(why.contains("descendants"), "{why}");
        assert!(why.contains("round 10"), "{why}");
        assert!(why.contains("without a new blocking"), "{why}");

        // One blocking objection justifies a second round. A third does not
        // when nothing new is blocking.
        let small = [
            r#"{"id":"p","title":"widget","state":"STARTED"}"#,
            r#"{"id":"p-r1","title":"panel r1: widget","state":"DONE","parent":"p","logbook":[{"note":"finding: tester: blocking: drops a failure :: exit status @ p"}]}"#,
            r#"{"id":"p-r2","title":"panel r2: widget","state":"DONE","parent":"p"}"#,
        ]
        .join("\n");
        let flags = flag_runaways(&small, &budget);
        assert!(
            flags.iter().all(|(id, _)| id != "p"),
            "round 2 with one blocking objection is the fix, not a runaway: {flags:?}"
        );
        let wide_round = [
            r#"{"id":"p","title":"widget","state":"STARTED"}"#,
            r#"{"id":"p-r1","title":"panel r1: widget","state":"DONE","parent":"p"}"#,
            r#"{"id":"p-r2","title":"panel r2: widget","state":"DONE","parent":"p"}"#,
            r#"{"id":"p-r3","title":"panel r3: widget","state":"DONE","parent":"p"}"#,
        ]
        .join("\n");
        let flags = flag_runaways(&wide_round, &budget);
        assert!(
            flags
                .iter()
                .any(|(id, why)| id == "p" && why.contains("round 3")),
            "{flags:?}"
        );
    }
}
