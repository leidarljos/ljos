//! Versioned workflow skeletons.
//!
//! An Org file in the project is a DAG of steps. `define` stores that file
//! as a deed. `start` files one ticket per step under a parent, with block
//! edges for `:DEPENDS:`. A signoff step stays open until the rest of the
//! run, its blockers, a ship consensus and any required deed are in place.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

const STEP_NOTE: &str = "step: ";
const KIND_NOTE: &str = "kind: ";
const QUORUM_NOTE: &str = "quorum: ";
const DEED_NOTE: &str = "deed: ";
const ROLE_NOTE: &str = "role: ";
const NAME_NOTE: &str = "skeleton: ";
const VERSION_NOTE: &str = "skeleton-version: ";
const SHA_NOTE: &str = "skeleton-sha: ";
const DEED_ID_NOTE: &str = "skeleton-deed: ";
const PROPOSAL_NOTE: &str = "skeleton-proposal: ";
const STEPS_NOTE: &str = "skeleton-steps: ";
const REVISION_NOTE: &str = "skeleton-revision: ";

#[derive(Debug, Clone)]
struct RunState {
    name: String,
    version: Option<String>,
    sha: Option<String>,
    steps: BTreeMap<String, String>,
    filing: Option<Filing>,
}

#[derive(Debug, Clone)]
struct StoredRun {
    parent: String,
    name: String,
    sha256: String,
    steps: BTreeMap<String, String>,
    /// The step a start was filing when it wrote this record.
    filing: Option<Filing>,
}

/// A step a start was about to file, and the token written to the index
/// before the create and tagged on the ticket it files. Only a child that
/// carries this token is that ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Filing {
    step: String,
    token: String,
}

const FILING_TAG: &str = "filing-";

/// A step's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Do,
    Review,
    Signoff,
}

impl Kind {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "do" => Some(Self::Do),
            "review" => Some(Self::Review),
            "signoff" => Some(Self::Signoff),
            _ => None,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Do => "do",
            Self::Review => "review",
            Self::Signoff => "signoff",
        }
    }

    fn ticket_type(self) -> &'static str {
        match self {
            Self::Signoff => "decision",
            Self::Do | Self::Review => "task",
        }
    }
}

/// One step in a skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub key: String,
    pub title: String,
    pub depends: Vec<String>,
    pub kind: Kind,
    pub role: Option<String>,
    pub quorum: Option<usize>,
    pub deed: Option<bool>,
    pub body: String,
    pub line: usize,
    body_line: usize,
    key_line: usize,
    role_line: Option<usize>,
    kind_line: usize,
    depends_line: usize,
}

/// A parsed skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skeleton {
    pub title: String,
    pub title_line: usize,
    pub version: i64,
    pub version_line: usize,
    pub steps: Vec<Step>,
    pub source: String,
}

/// `k=v` pairs from `--var`.
///
/// # Errors
///
/// A pair has no `=`, the key is not a variable name, a key is repeated, or
/// a value has a line break or control character.
pub fn parse_vars(pairs: &[String]) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            bail!("workflow: the --var argument is not key=value");
        };
        if key.is_empty() {
            bail!("workflow: `--var` has an empty variable name");
        }
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("workflow: `--var` has a variable name with unsupported characters");
        }
        if value_has_break_or_control(value) {
            bail!("workflow: a `--var` value contains a line break or control character");
        }
        if map.contains_key(key) {
            bail!("workflow: `--var` repeats a variable name");
        }
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

fn value_has_break_or_control(value: &str) -> bool {
    value
        .chars()
        .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
}

/// Parse and validate `text`. `source` is the path named in errors.
///
/// # Errors
///
/// The Org is not a skeleton: a missing title or version, a bad property,
/// a duplicate key, an unknown dependency, a cycle, or a signoff that is
/// not the single step depending on every other step.
pub fn parse_skeleton(text: &str, source: &str) -> Result<Skeleton> {
    let draft = scan(text, source)?;
    validate(draft)
}

/// Replace `{{key}}` from `vars` in titles and bodies.
///
/// # Errors
///
/// A placeholder is unclosed or names a key that was not passed.
pub fn fill_vars(skel: &Skeleton, vars: &BTreeMap<String, String>) -> Result<Skeleton> {
    check_vars_used(skel, vars)?;
    let mut out = skel.clone();
    out.title = fill_text(&skel.title, vars, skel.title_line, &skel.source)?;
    for step in &mut out.steps {
        step.title = fill_text(&step.title, vars, step.line, &skel.source)?;
        step.body = fill_text(&step.body, vars, step.line, &skel.source)?;
    }
    Ok(out)
}

fn check_vars_used(skel: &Skeleton, vars: &BTreeMap<String, String>) -> Result<()> {
    let mut used = BTreeSet::new();
    used.extend(variable_names(&skel.title, skel.title_line, &skel.source)?);
    for step in &skel.steps {
        used.extend(variable_names(&step.title, step.line, &skel.source)?);
        used.extend(variable_names(&step.body, step.body_line, &skel.source)?);
    }
    if vars.keys().any(|key| !used.contains(key)) {
        bail!("workflow: `--var` names a variable the skeleton does not use");
    }
    Ok(())
}

fn variable_names(text: &str, line: usize, source: &str) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            bail!("workflow: {source}:{line}: an unclosed placeholder starts near {start}");
        };
        let key = after[..end].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("workflow: {source}:{line}: placeholder {key:?} is not a variable name");
        }
        names.insert(key.to_string());
        rest = &after[end + 2..];
    }
    Ok(names)
}

/// Deterministic public Org with steps in topological order.
///
/// # Errors
///
/// Author text contains a class that cannot appear on a public page.
pub fn render_skeleton(skel: &Skeleton) -> Result<String> {
    let hits = public_hits(skel);
    if !hits.is_empty() {
        bail!("workflow: public render refused: {}", hits.join("; "));
    }
    Ok(render_org(skel))
}

/// Validate `path` and store it as a deed when the bytes or the version changed.
///
/// The same bytes are a no-op. A different body at the same `#+version` is refused.
///
/// # Errors
///
/// The file is not a skeleton, the version was not bumped, or the deed store
/// did not accept the file.
pub fn define_file(path: &Path) -> Result<String> {
    let text = std::fs::read_to_string(path).context("workflow: read <file>")?;
    let name = skeleton_name(path)?;
    define_named(&name, &text, "<file>", None)
}

fn run_lock_dir(root: &Path) -> PathBuf {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return dot_git.join("ljos-locks");
    }
    if dot_git.is_file() {
        if let Ok(out) = crate::git_at(root)
            .args(["rev-parse", "--git-common-dir"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            if out.status.success() {
                let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !raw.is_empty() {
                    let p = Path::new(&raw);
                    return if p.is_absolute() {
                        p.join("ljos-locks")
                    } else {
                        root.join(p).join("ljos-locks")
                    };
                }
            }
        }
    }
    root.join(".ljos-locks")
}

pub(crate) fn acquire_run_lock(parent: &str) -> Result<crate::CommitLock> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    layout.require_tracker()?;
    let slug: String = parent
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let lock_dir = run_lock_dir(layout.root());
    std::fs::create_dir_all(&lock_dir)
        .with_context(|| format!("workflow: create {}", lock_dir.display()))?;
    let lock_path = lock_dir.join(format!("workflow-start-{slug}.lock"));
    let deadline = std::time::Instant::now() + run_lock_wait();
    match crate::CommitLock::try_acquire_by(&lock_path, deadline) {
        Ok(Some(lock)) => Ok(lock),
        Ok(None) => Err(anyhow::anyhow!(
            "workflow: timed out waiting for the run lock for {parent} at {}",
            lock_path.display()
        )),
        Err(err) => Err(anyhow::anyhow!(
            "workflow: cannot lock the run for {parent} at {}: {err}",
            lock_path.display()
        )),
    }
}

/// How long `--start` waits for the per-run lock.
///
/// `LJOS_WORKFLOW_LOCK_WAIT` is a whole number of seconds. Unset, zero, or
/// not a number keeps the thirty-second default.
fn run_lock_wait() -> std::time::Duration {
    let secs = std::env::var("LJOS_WORKFLOW_LOCK_WAIT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(30);
    std::time::Duration::from_secs(secs)
}

/// File the steps of `name` under `parent`.
///
/// The run keeps the skeleton version from the first start. A later start
/// uses that copy, files a missing step and restores a missing edge. The
/// index records the run before the first tracker write and each step id as
/// soon as its `step:` note is written, so the next start finishes a start
/// that was cut short.
///
/// # Errors
///
/// The skeleton is not stored, `parent` already runs another workflow, a
/// variable is missing, or the tracker refuses a write.
pub fn start_run(
    parent: &str,
    name: &str,
    vars: &BTreeMap<String, String>,
    file: Option<&Path>,
) -> Result<String> {
    let parent = parent.trim();
    if parent.is_empty() {
        bail!("workflow: pass a parent ticket");
    }
    validate_start(parent, name, vars, file)?;
    let _skeleton_lock = acquire_skeleton_lock(name)?;
    let _lock = acquire_run_lock(parent)?;
    let mut latest = require_stored(name)?;
    let parent_card = crate::tracker_show_json(parent)
        .with_context(|| format!("workflow: {parent} is not a ticket"))?;
    if crate::workflow_name_from_issue(&parent_card).is_some() {
        bail!("workflow: {parent} already runs the feature workflow");
    }
    let state = run_state(parent, &parent_card)?;
    let already = match state.as_ref() {
        Some(state) if state.name != name => {
            bail!(
                "workflow: {parent} already runs the {} skeleton",
                state.name
            );
        }
        Some(_) => true,
        None => false,
    };
    let using = if already {
        match state.as_ref().and_then(|state| state.sha.as_deref()) {
            Some(sha) => pinned_stored(&latest, sha, parent, name)?,
            None => latest.clone(),
        }
    } else {
        latest.clone()
    };
    if let Some(path) = file {
        let text = std::fs::read_to_string(path).context("workflow: read --file <value>")?;
        let sha = sha256_hex(text.as_bytes());
        if sha != using.sha256 {
            bail!("workflow: --file <value> does not match the stored skeleton");
        }
    }
    let skel = parse_skeleton(&using.body, &format!("stored:{name}"))?;
    check_vars_used(&skel, vars)?;
    let recs = tracker_recs()?;
    let existing = step_rows(&recs, parent)?;
    let recorded = state
        .as_ref()
        .map(|state| state.steps.clone())
        .unwrap_or_default();
    let mut ids = recorded.clone();
    let prepared = if skel.steps.iter().any(|step| !ids.contains_key(&step.key)) {
        let filled = fill_vars(&skel, vars)?;
        let order = topo(&filled)?;
        let project = parent_card["project"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("workflow: {parent} has no project"))?;
        Some((filled, order, project))
    } else {
        None
    };
    let mut out = String::new();
    let interrupted = state.as_ref().and_then(|state| state.filing.clone());
    let record = |latest: &mut Stored, ids: &BTreeMap<String, String>, filing: Option<&Filing>| {
        append_run(
            latest,
            StoredRun {
                parent: parent.to_string(),
                name: name.to_string(),
                sha256: using.sha256.clone(),
                steps: ids.clone(),
                filing: filing.cloned(),
            },
        );
        write_stored(latest)
    };
    if !already {
        record(&mut latest, &ids, None)?;
        out.push_str(&format!(
            "{parent} runs {name} version {}\nsha {}\n",
            using.version, using.sha256
        ));
    }
    if !ordered_notes(&parent_card)
        .iter()
        .any(|note| note.starts_with(NAME_NOTE))
    {
        write_note(parent, &format!("{NAME_NOTE}{name}"))?;
        write_note(parent, &format!("{VERSION_NOTE}{}", using.version))?;
        write_note(parent, &format!("{SHA_NOTE}{}", using.sha256))?;
        write_note(parent, &format!("{DEED_ID_NOTE}{}", using.accession))?;
    }
    let mut filed_any = false;
    if let Some((filled, order, project)) = &prepared {
        for key in order {
            if ids.contains_key(key) {
                continue;
            }
            let step = filled
                .steps
                .iter()
                .find(|step| step.key == *key)
                .expect("topo key");
            let adopted = match &interrupted {
                Some(filing) if filing.step == *key => {
                    interrupted_step(&recs, parent, key, &filing.token, &ids)?
                }
                _ => None,
            };
            let id = match &adopted {
                Some(id) => id.clone(),
                None => {
                    let filing = Filing {
                        step: key.clone(),
                        token: filing_token()?,
                    };
                    record(&mut latest, &ids, Some(&filing))?;
                    file_step(parent, project, step, &filing.token)?
                }
            };
            ids.insert(step.key.clone(), id.clone());
            record(&mut latest, &ids, None)?;
            write_step_notes(&id, step)?;
            let verb = if adopted.is_some() {
                "adopted"
            } else {
                "filed"
            };
            out.push_str(&format!("{verb} {} {id}\n", step.key));
            filed_any = true;
        }
    }
    let unrecorded: Vec<String> = ids
        .iter()
        .filter(|(key, _)| !recorded.contains_key(*key))
        .filter(|(key, _)| skel.steps.iter().any(|step| step.key == **key))
        .map(|(key, id)| format!("{key}={id}"))
        .collect();
    if !unrecorded.is_empty() {
        write_note(parent, &format!("{STEPS_NOTE}{}", unrecorded.join(" ")))?;
    }
    let mut repaired = false;
    for step in &skel.steps {
        let Some(id) = ids.get(&step.key) else {
            bail!("workflow: {} has no ticket under {parent}", step.key);
        };
        for dep in &step.depends {
            let Some(on) = ids.get(dep) else {
                bail!("workflow: {dep} has no ticket under {parent}");
            };
            let said = crate::block_edge(id, on, false)?;
            if said.contains("already") {
                continue;
            }
            repaired = true;
            if already && !filed_any {
                out.push_str(&format!("repaired {id} waits on {on}\n"));
            } else {
                out.push_str(said.lines().next().unwrap_or(""));
                out.push('\n');
            }
        }
    }
    if already && !filed_any && !repaired {
        out.push_str(&format!(
            "{parent} already runs {name} version {}\n",
            using.version
        ));
        for row in &existing {
            out.push_str(&format!("{}  {}  {}\n", row.key, row.id, row.state));
        }
        out.push_str("nothing new\n");
        return Ok(out);
    }
    if already && !filed_any && repaired {
        let mut head = format!("{parent} already runs {name} version {}\n", using.version);
        head.push_str(&out);
        out = head;
    }
    if !out.contains("filed ") && !repaired {
        out.push_str("nothing new\n");
    }
    if filed_any {
        out.push_str(&crate::persist_tracker(parent, "started a workflow"));
    }
    Ok(out)
}

fn validate_start(
    parent: &str,
    name: &str,
    vars: &BTreeMap<String, String>,
    file: Option<&Path>,
) -> Result<()> {
    let latest = require_stored(name)?;
    let parent_card = crate::tracker_show_json(parent)
        .with_context(|| format!("workflow: {parent} is not a ticket"))?;
    if crate::workflow_name_from_issue(&parent_card).is_some() {
        bail!("workflow: {parent} already runs the feature workflow");
    }
    let state = run_state(parent, &parent_card)?;
    let using = match state.as_ref() {
        Some(state) if state.name != name => {
            bail!(
                "workflow: {parent} already runs the {} skeleton",
                state.name
            );
        }
        Some(state) => match state.sha.as_deref() {
            Some(sha) => pinned_stored(&latest, sha, parent, name)?,
            None => latest,
        },
        None => latest,
    };
    if let Some(path) = file {
        let text = std::fs::read_to_string(path).context("workflow: read --file <value>")?;
        if sha256_hex(text.as_bytes()) != using.sha256 {
            bail!("workflow: the file does not match the stored skeleton");
        }
    }
    let skeleton = parse_skeleton(&using.body, &format!("stored:{name}"))?;
    check_vars_used(&skeleton, vars)?;
    let recorded = state
        .as_ref()
        .map(|state| state.steps.clone())
        .unwrap_or_default();
    if skeleton
        .steps
        .iter()
        .any(|step| !recorded.contains_key(&step.key))
    {
        fill_vars(&skeleton, vars)?;
    }
    Ok(())
}

/// The DAG, ready steps, open blockers and what the signoff still needs.
///
/// `None` when `parent` carries no skeleton.
///
/// # Errors
///
/// The tracker cannot be read.
pub fn status_report(parent: &str) -> Result<Option<String>> {
    let card = crate::tracker_show_json(parent)?;
    let Some(state) = run_state(parent, &card)? else {
        return Ok(None);
    };
    let version = state.version.as_deref().unwrap_or("?");
    let sha = state.sha.as_deref().unwrap_or("?");
    let recs = tracker_recs()?;
    let rows = step_rows(&recs, parent)?;
    let mut out = format!("{} version {version} sha {sha} on {parent}\n", state.name);
    let by_key: BTreeMap<&str, &RunStep> = rows.iter().map(|row| (row.key.as_str(), row)).collect();
    for row in &rows {
        out.push_str(&format!(
            "{}  {}  {}  {}\n",
            row.key, row.kind, row.state, row.id
        ));
        if row.depends.is_empty() {
            out.push_str("  depends on none\n");
        } else {
            let deps = row
                .depends
                .iter()
                .map(|dep| {
                    let state = by_key
                        .get(dep.as_str())
                        .map(|r| r.state.as_str())
                        .unwrap_or("missing");
                    format!("{dep} ({state})")
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("  depends on {deps}\n"));
        }
    }
    let ready = ready_keys(&recs, &rows);
    if ready.is_empty() {
        out.push_str("ready: none\n");
    } else {
        out.push_str(&format!("ready: {}\n", ready.join(" ")));
    }
    let blockers = open_blockers(&recs, &rows);
    if blockers.is_empty() {
        out.push_str("blockers: none\n");
    } else {
        out.push_str("blockers:\n");
        for b in &blockers {
            if b.state == "MISSING" {
                out.push_str(&format!("  {} missing blocking {}\n", b.id, b.step));
            } else if b.via_edge {
                out.push_str(&format!(
                    "  {} {} blocking {}: {}\n",
                    b.id, b.state, b.step, b.title
                ));
            } else {
                out.push_str(&format!(
                    "  {} {} under {}: {}\n",
                    b.id, b.state, b.step, b.title
                ));
            }
        }
    }
    out.push_str("signoff needs:\n");
    let pinned = match pinned_skeleton(parent, &card) {
        Ok(pinned) => pinned,
        Err(err) => {
            out.push_str(&format!(
                "  the pinned skeleton could not be read: {err:#}\n"
            ));
            return Ok(Some(out));
        }
    };
    let recorded = state.steps;
    match pinned.steps.iter().find(|step| step.kind == Kind::Signoff) {
        None => out.push_str("  no signoff step\n"),
        Some(sign) => {
            let missing = signoff_missing(&recs, parent, &pinned, &recorded, &sign.key)?;
            if missing.is_empty() {
                out.push_str("  nothing\n");
            } else {
                for line in missing {
                    out.push_str(&format!("  {line}\n"));
                }
            }
        }
    }
    Ok(Some(out))
}

/// Refuse to complete or finish a signoff step that is still missing work.
///
/// The pinned skeleton decides. The step is found by the ticket id the start
/// command recorded on the parent, and its kind, quorum and deed come from
/// the pinned step, never from tags or notes on the ticket. A ticket that is
/// not a recorded step is left alone, unless it was filed as a signoff step.
/// A tracker that cannot be read is left alone, so a claim id that is not a
/// ticket still completes.
///
/// # Errors
///
/// The ticket is a signoff step and the run, the blockers, the consensus or
/// the deed is not ready. The error lists each missing piece.
pub fn signoff_block(issue: &str) -> Result<()> {
    let Ok(card) = crate::tracker_show_json(issue) else {
        return Ok(());
    };
    let filed_as_signoff = notes_of(&card)
        .iter()
        .any(|note| note.strip_prefix(KIND_NOTE) == Some(Kind::Signoff.word()));
    let run_parent = card["parent"]
        .as_str()
        .map(str::trim)
        .filter(|parent| !parent.is_empty());
    let run = if let Some(parent) = run_parent {
        let parent_card = crate::tracker_show_json(parent)?;
        run_state(parent, &parent_card)?.and_then(|state| {
            let key = state.steps.into_iter().find(|(_, id)| id == issue)?.0;
            Some((parent.to_string(), parent_card, key))
        })
    } else {
        None
    };
    let Some((parent, parent_card, key)) = run else {
        if filed_as_signoff {
            bail!("signoff: {issue} is not a recorded step of a skeleton run");
        }
        return Ok(());
    };
    let pinned = pinned_skeleton(&parent, &parent_card)?;
    let Some(step) = pinned.steps.iter().find(|step| step.key == key) else {
        bail!("signoff: {key} is not a step of the skeleton {parent} pinned");
    };
    if step.kind != Kind::Signoff {
        return Ok(());
    }
    let recs = tracker_recs()?;
    let state = run_state(&parent, &parent_card)?
        .ok_or_else(|| anyhow::anyhow!("signoff: {parent} has no pinned skeleton"))?;
    let missing = signoff_missing(&recs, &parent, &pinned, &state.steps, &key)?;
    if missing.is_empty() {
        return Ok(());
    }
    let mut msg = format!("signoff: {issue} is missing\n");
    for line in &missing {
        msg.push_str(line);
        msg.push('\n');
    }
    bail!("{msg}");
}

/// The skeleton version a run pinned at start.
fn pinned_skeleton(parent: &str, parent_card: &Value) -> Result<Skeleton> {
    let state = run_state(parent, parent_card)?
        .ok_or_else(|| anyhow::anyhow!("signoff: {parent} has no pinned skeleton name"))?;
    let sha = state
        .sha
        .ok_or_else(|| anyhow::anyhow!("signoff: {parent} has no pinned skeleton sha"))?;
    let latest = require_stored(&state.name)?;
    let pinned = pinned_stored(&latest, &sha, parent, &state.name)?;
    parse_skeleton(&pinned.body, &format!("stored:{}", state.name))
}

/// The latest immutable index record for this run.
///
/// Tracker notes mirror these fields for people. They are never read as run
/// state.
fn run_state(parent: &str, card: &Value) -> Result<Option<RunState>> {
    let Some(run) = read_run(parent)? else {
        if ordered_notes(card)
            .iter()
            .any(|note| note.starts_with(NAME_NOTE))
        {
            bail!(
                "workflow: {parent} has no run in the skeleton index; restart the workflow on a new parent with `ljos workflow --start NAME NEW_PARENT`"
            );
        }
        return Ok(None);
    };
    let stored = require_stored(&run.name)?;
    let pinned = pinned_stored(&stored, &run.sha256, parent, &run.name)?;
    Ok(Some(RunState {
        name: run.name,
        version: Some(pinned.version.to_string()),
        sha: Some(run.sha256),
        steps: run.steps,
        filing: run.filing,
    }))
}

fn append_run(stored: &mut Stored, run: StoredRun) {
    if stored.runs.last().is_none_or(|last| {
        last.parent != run.parent
            || last.name != run.name
            || last.sha256 != run.sha256
            || last.steps != run.steps
            || last.filing != run.filing
    }) {
        stored.runs.push(run);
    }
}

fn ordered_notes(card: &Value) -> Vec<String> {
    let mut notes: Vec<(String, usize, String)> = card["logbook"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, entry)| {
            let note = entry["note"].as_str()?.trim();
            if note.is_empty() {
                return None;
            }
            let stamp = entry["timestamp"].as_str().unwrap_or("").to_string();
            Some((stamp, index, note.to_string()))
        })
        .collect();
    // The logbook lists the newest note first.
    notes.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    notes.into_iter().map(|(_, _, note)| note).collect()
}

fn first_note_value(notes: &[String], prefix: &str) -> Option<String> {
    notes
        .iter()
        .find_map(|note| note.strip_prefix(prefix))
        .map(str::trim)
        .filter(|rest| !rest.is_empty())
        .map(str::to_string)
}

fn revision_text(revision: &AppliedRevision) -> String {
    format!(
        "{REVISION_NOTE}{} {} {} {}",
        revision.proposal, revision.version, revision.sha256, revision.accession
    )
}

/// What the signoff step `sign_key` of a pinned run still needs.
fn signoff_missing(
    recs: &[vissue_core::views::IssueRec],
    parent: &str,
    pinned: &Skeleton,
    recorded: &BTreeMap<String, String>,
    sign_key: &str,
) -> Result<Vec<String>> {
    let (rows, mut missing) = missing_pinned_steps(recs, parent, pinned, recorded);
    let Some(sign) = rows.iter().find(|row| row.key == sign_key) else {
        return Ok(missing);
    };
    missing.extend(signoff_lines(recs, &rows, sign)?);
    Ok(missing)
}

/// The rows of a pinned run, one for each step whose recorded ticket is in
/// the tracker, with kind, depends, quorum and deed from the pinned step and
/// the state from the ticket. A step with no recorded id, or whose recorded
/// ticket is gone, is a missing line instead.
fn missing_pinned_steps(
    recs: &[vissue_core::views::IssueRec],
    parent: &str,
    pinned: &Skeleton,
    recorded: &BTreeMap<String, String>,
) -> (Vec<RunStep>, Vec<String>) {
    let mut rows = Vec::new();
    let mut missing = Vec::new();
    for step in &pinned.steps {
        let rec = recorded
            .get(&step.key)
            .and_then(|id| recs.iter().find(|rec| rec.heading.id == *id))
            .filter(|rec| rec.heading.parent().map(str::trim) == Some(parent))
            .filter(|rec| {
                crate::tracker_show_json(&rec.heading.id)
                    .ok()
                    .and_then(|card| step_key_of(&card))
                    .as_deref()
                    == Some(step.key.as_str())
            });
        let Some(rec) = rec else {
            missing.push(format!(
                "{} is missing from the pinned skeleton run",
                step.key
            ));
            continue;
        };
        rows.push(RunStep {
            key: step.key.clone(),
            id: rec.heading.id.clone(),
            state: rec.heading.state.clone(),
            kind: step.kind.word().to_string(),
            depends: step.depends.clone(),
            quorum: step.quorum.unwrap_or(2),
            deed: step.deed == Some(true),
        });
    }
    (order_rows(rows), missing)
}

/// File a proposal ticket whose options are `keep` and `revise`.
///
/// # Errors
///
/// `parent` is not a run of `name`, another writer held the skeleton lock
/// past the wait, or the tracker refuses the ticket.
pub fn revise_from(name: &str, parent: &str) -> Result<String> {
    let _lock = acquire_skeleton_lock(name)?;
    let mut stored = require_stored(name)?;
    let card = crate::tracker_show_json(parent)
        .with_context(|| format!("workflow: {parent} is not a ticket"))?;
    let Some(state) = run_state(parent, &card)? else {
        bail!("workflow: {parent} is not a run of {name}");
    };
    if state.name != name {
        bail!("workflow: {parent} is not a run of {name}");
    }
    let pinned_sha = state
        .sha
        .clone()
        .ok_or_else(|| anyhow::anyhow!("workflow: {parent} has no pinned skeleton sha"))?;
    if let Some(id) = stored.proposal.as_deref() {
        if let Ok(prop) = crate::tracker_show_json(id) {
            let state = prop["state"].as_str().unwrap_or("");
            if !is_closed(state) {
                return Ok(format!("proposal {id} is already open for {name}\n"));
            }
        }
    }
    let recs = tracker_recs()?;
    let rows = step_rows(&recs, parent)?;
    let mut body = String::from("Options: keep, revise\n\n");
    let version = state.version.as_deref().unwrap_or("?");
    body.push_str(&format!(
        "Proposal for {name} version {version}, from {parent}.\n"
    ));
    let rec_by_id: BTreeMap<&str, &vissue_core::views::IssueRec> = recs
        .iter()
        .map(|rec| (rec.heading.id.as_str(), rec))
        .collect();
    let id_by_key: BTreeMap<&str, &str> = rows
        .iter()
        .map(|row| (row.key.as_str(), row.id.as_str()))
        .collect();
    for row in &rows {
        body.push_str(&format!("\n{} ({}, {})\n", row.key, row.id, row.state));
        if let Ok(step_card) = crate::tracker_show_json(&row.id) {
            for note in notes_of(&step_card) {
                if is_internal_note(&note) {
                    continue;
                }
                body.push_str(&format!("  note: {note}\n"));
            }
        }
        let mut seen = BTreeSet::new();
        for (id, state, title) in children_of(&recs, &row.id) {
            seen.insert(id.clone());
            body.push_str(&format!("  blocker {id} {state}: {title}\n"));
        }
        let dep_ids: BTreeSet<&str> = row
            .depends
            .iter()
            .filter_map(|dep| id_by_key.get(dep.as_str()).copied())
            .collect();
        if let Some(rec) = rec_by_id.get(row.id.as_str()) {
            let mut blocked = rec.heading.blocked_by();
            blocked.sort();
            for blocker_id in blocked {
                if dep_ids.contains(blocker_id.as_str()) || !seen.insert(blocker_id.clone()) {
                    continue;
                }
                if let Some(b) = rec_by_id.get(blocker_id.as_str()) {
                    body.push_str(&format!(
                        "  blocker {} {}: {}\n",
                        blocker_id, b.heading.state, b.heading.title
                    ));
                }
            }
        }
    }
    let project = card["project"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("workflow: {parent} has no project"))?;
    let (id, _, _, _) = crate::file_issue(
        project,
        Some(parent),
        Some("decision"),
        Some("proposal"),
        None,
        Some(&body),
        &format!("revise {name}"),
        &crate::seat_name(),
        true,
        None,
    )?;
    if id.is_empty() {
        bail!("workflow: the proposal ticket was not filed");
    }
    stored.proposal = Some(id.clone());
    stored.proposal_runs.push(ProposalRun {
        proposal: id.clone(),
        parent: parent.to_string(),
        sha256: pinned_sha,
    });
    write_stored(&stored)?;
    write_note(parent, &format!("{PROPOSAL_NOTE}{id}"))?;
    let _ = crate::persist_tracker(&id, "filed a proposal");
    Ok(format!(
        "filed proposal {id} for {name} on {parent}\nOptions: keep, revise\n"
    ))
}

/// Store `file` as version + 1 after consensus on the proposal settles `revise`.
///
/// The run the index recorded for the proposal moves to that version, and
/// only while it still pins the sha the proposal was filed against.
/// `--start` does not move a run.
///
/// # Errors
///
/// There is no proposal, the index recorded no run for it, consensus did
/// not settle `revise`, or the file is not a skeleton.
pub fn revise_apply(name: &str, file: &Path) -> Result<String> {
    let _lock = acquire_skeleton_lock(name)?;
    let mut stored = require_stored(name)?;
    let Some(proposal) = stored.proposal.clone() else {
        bail!(
            "workflow: {name} has no proposal; `ljos workflow revise {name} --from PARENT` files one"
        );
    };
    let Some(filed_from) = proposal_run(&stored, &proposal) else {
        bail!(
            "workflow: the skeleton index records no run for proposal {proposal}; `ljos workflow revise {name} --from PARENT` files a new one"
        );
    };
    if let Some(revision) = applied_revision(&stored, &proposal) {
        return finish_revision(
            name,
            &filed_from,
            &revision,
            format!("proposal {proposal} for {name} is already applied\n"),
        );
    }
    if stored.applied.as_deref() == Some(proposal.as_str()) {
        let revision = current_revision(&stored, &proposal);
        stored.revisions.push(AppliedRevision {
            proposal: proposal.clone(),
            version: stored.version,
            sha256: stored.sha256.clone(),
            accession: stored.accession.clone(),
        });
        write_stored(&stored)?;
        return finish_revision(
            name,
            &filed_from,
            &revision,
            format!("proposal {proposal} for {name} is already applied\n"),
        );
    }
    match crate::reap::gate_quorum(&proposal, 2, None)? {
        crate::reap::QuorumGate::Met { .. } => {}
        crate::reap::QuorumGate::Short { have, needed }
        | crate::reap::QuorumGate::Lapsed { have, needed } => {
            bail!("workflow: consensus on {proposal} has {have} of {needed} ballots, so {name} was not changed");
        }
    }
    let choice = settled_choice(&proposal)?;
    if choice.as_deref() != Some("revise") {
        let got = choice.unwrap_or_else(|| "nothing".into());
        bail!("workflow: consensus on {proposal} settled {got}, so {name} was not changed");
    }
    let text = std::fs::read_to_string(file).context("workflow: read --file <value>")?;
    let file_version = version_of(&text).unwrap_or(0);
    let incoming = body_without_version(&text);
    let already_stored =
        |version: i64, body: &str| version > file_version && body_without_version(body) == incoming;
    if already_stored(stored.version, &stored.body)
        || stored
            .versions
            .iter()
            .any(|row| already_stored(row.version, &row.body))
    {
        return Ok(format!(
            "proposal {proposal} for {name} is already applied\n"
        ));
    }
    let next = stored.version + 1;
    let bumped = set_version(&text, next)?;
    let out = define_locked(name, &bumped, "--file <value>", Some(&proposal))?;
    let stored = require_stored(name)?;
    let revision = applied_revision(&stored, &proposal)
        .ok_or_else(|| anyhow::anyhow!("workflow: the applied revision was not recorded"))?;
    finish_revision(name, &filed_from, &revision, out)
}

fn proposal_run(stored: &Stored, proposal: &str) -> Option<ProposalRun> {
    stored
        .proposal_runs
        .iter()
        .find(|row| row.proposal == proposal)
        .cloned()
}

fn current_revision(stored: &Stored, proposal: &str) -> AppliedRevision {
    AppliedRevision {
        proposal: proposal.to_string(),
        version: stored.version,
        sha256: stored.sha256.clone(),
        accession: stored.accession.clone(),
    }
}

fn applied_revision(stored: &Stored, proposal: &str) -> Option<AppliedRevision> {
    let row = stored
        .revisions
        .iter()
        .find(|row| row.proposal == proposal)?;
    Some(row.clone())
}

/// Move the run recorded for the proposal to `revision`, under its run lock.
///
/// The run moves only while it still pins the sha the proposal was filed
/// against, so applying again never moves it twice or moves another run.
fn finish_revision(
    name: &str,
    filed_from: &ProposalRun,
    revision: &AppliedRevision,
    mut out: String,
) -> Result<String> {
    let proposal = filed_from.proposal.as_str();
    let run = filed_from.parent.as_str();
    let version = revision.version;
    let _ = write_note(proposal, &format!("revised {name} to version {version}"));
    let _run_lock = acquire_run_lock(run)?;
    let mut stored = require_stored(name)?;
    let current = stored
        .runs
        .iter()
        .rev()
        .find(|row| row.parent == run && row.name == name)
        .cloned();
    match current {
        Some(current) if current.sha256 == filed_from.sha256 => {
            append_run(
                &mut stored,
                StoredRun {
                    parent: run.to_string(),
                    name: name.to_string(),
                    sha256: revision.sha256.clone(),
                    steps: current.steps,
                    filing: None,
                },
            );
            write_stored(&stored)?;
            let revision_note = revision_text(revision);
            write_note(proposal, &revision_note)?;
            write_note(run, &format!("{VERSION_NOTE}{version}"))?;
            write_note(run, &format!("{SHA_NOTE}{}", revision.sha256))?;
            write_note(run, &format!("{DEED_ID_NOTE}{}", revision.accession))?;
            write_note(run, &revision_note)?;
            out.push_str(&format!("{run} now runs {name} version {version}\n"));
        }
        Some(current) if current.sha256 == revision.sha256 => {
            out.push_str(&format!("{run} already runs {name} version {version}\n"));
        }
        _ => {
            out.push_str(&format!(
                "{run} no longer runs the {name} version proposal {proposal} was filed against, so it stays\n"
            ));
        }
    }
    out.push_str(&format!("noted the proposal {proposal}\n"));
    close_proposal(proposal)?;
    Ok(out)
}

fn close_proposal(id: &str) -> Result<()> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    vissue_core::ops::update(&layout, id, Some("DONE"), None, None, None)
        .with_context(|| format!("workflow: close proposal {id}"))?;
    Ok(())
}

fn version_of(text: &str) -> Option<i64> {
    for line in split_keep(text) {
        let (body, _) = split_nl(line);
        if let Some((key, value)) = keyword(body) {
            if key.eq_ignore_ascii_case("version") {
                return value.trim().parse().ok();
            }
        }
    }
    None
}

fn body_without_version(text: &str) -> String {
    let mut kept = String::new();
    for line in split_keep(text) {
        let (body, _) = split_nl(line);
        if keyword(body).is_some_and(|(key, _)| key.eq_ignore_ascii_case("version")) {
            continue;
        }
        kept.push_str(line);
    }
    sha256_hex(kept.as_bytes())
}

/// Load a skeleton for `render`.
///
/// `file` wins, then the stored deed, then `.ljos/workflows/NAME.org`.
///
/// # Errors
///
/// No copy of the skeleton can be read, or the Org does not validate.
pub fn load_skeleton(name: &str, file: Option<&Path>) -> Result<Skeleton> {
    if let Some(path) = file {
        let text = std::fs::read_to_string(path).context("workflow: read --file <value>")?;
        return parse_skeleton(&text, "--file <value>");
    }
    if let Some(stored) = read_stored(name)? {
        return parse_skeleton(&stored.body, "stored skeleton");
    }
    let path = default_path(name);
    if path.is_file() {
        let text = std::fs::read_to_string(&path).context("workflow: read the default skeleton")?;
        return parse_skeleton(&text, "default skeleton");
    }
    bail!("workflow: <value> is not stored and its default skeleton is not a file");
}

fn default_path(name: &str) -> PathBuf {
    PathBuf::from(".ljos")
        .join("workflows")
        .join(format!("{name}.org"))
}

fn skeleton_name(path: &Path) -> Result<String> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("workflow: <file> needs a name of letters, digits, hyphens or underscores");
    }
    Ok(name.to_string())
}

#[derive(Debug, Clone)]
struct StoredVer {
    version: i64,
    sha256: String,
    accession: String,
    body: String,
}

#[derive(Debug, Clone)]
struct AppliedRevision {
    proposal: String,
    version: i64,
    sha256: String,
    accession: String,
}

/// The run a proposal was filed from and the sha that run had pinned then.
#[derive(Debug, Clone)]
struct ProposalRun {
    proposal: String,
    parent: String,
    sha256: String,
}

#[derive(Debug, Clone)]
struct Stored {
    name: String,
    version: i64,
    sha256: String,
    accession: String,
    body: String,
    proposal: Option<String>,
    applied: Option<String>,
    versions: Vec<StoredVer>,
    revisions: Vec<AppliedRevision>,
    proposal_runs: Vec<ProposalRun>,
    runs: Vec<StoredRun>,
}

fn check_skeleton_name(name: &str) -> Result<()> {
    if !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Ok(());
    }
    bail!("workflow: <value> is not a skeleton name");
}

fn index_path(name: &str) -> Result<PathBuf> {
    check_skeleton_name(name)?;
    Ok(index_dir()?.join(format!("{name}.json")))
}

fn index_dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        })
        .ok_or_else(|| {
            anyhow::anyhow!("workflow: set HOME so the skeleton index has a directory")
        })?;
    Ok(base.join("ljos").join("workflows"))
}

fn read_run(parent: &str) -> Result<Option<StoredRun>> {
    let dir = index_dir()?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(None);
    };
    let mut found = None;
    for entry in entries {
        let entry = entry.with_context(|| format!("workflow: read {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        check_skeleton_name(name)?;
        let Some(stored) = read_stored(name)? else {
            continue;
        };
        let Some(run) = stored
            .runs
            .iter()
            .rev()
            .find(|run| run.parent == parent)
            .cloned()
        else {
            continue;
        };
        if found.is_some() {
            bail!("workflow: {parent} has more than one run in the skeleton index");
        }
        found = Some(run);
    }
    Ok(found)
}

fn read_stored(name: &str) -> Result<Option<Stored>> {
    let path = index_path(name)?;
    if !path.is_file() {
        return Ok(None);
    }
    let dir = path.parent().unwrap_or(&path).display().to_string();
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("workflow: read the index of <value> in {dir}"))?;
    let v: Value = serde_json::from_str(&text)
        .with_context(|| format!("workflow: the index of <value> in {dir}"))?;
    let versions = v["versions"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(StoredVer {
                        version: row["version"].as_i64()?,
                        sha256: row["sha256"].as_str()?.to_string(),
                        accession: row["accession"].as_str().unwrap_or("").to_string(),
                        body: row["body"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let revisions = v["revisions"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(AppliedRevision {
                        proposal: row["proposal"].as_str()?.to_string(),
                        version: row["version"].as_i64()?,
                        sha256: row["sha256"].as_str()?.to_string(),
                        accession: row["accession"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let runs = v["runs"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let parent = row["parent"]
                        .as_str()
                        .filter(|value| !value.trim().is_empty())
                        .context("workflow: a run in the skeleton index has no parent")?;
                    let run_name = row["name"]
                        .as_str()
                        .filter(|value| !value.trim().is_empty())
                        .context("workflow: a run in the skeleton index has no name")?;
                    let sha256 = row["sha256"]
                        .as_str()
                        .filter(|value| !value.trim().is_empty())
                        .context("workflow: a run in the skeleton index has no sha256")?;
                    let object = row["steps"]
                        .as_object()
                        .context("workflow: a run in the skeleton index has no steps")?;
                    let mut steps = BTreeMap::new();
                    for (key, value) in object {
                        let id = value
                            .as_str()
                            .filter(|value| !value.trim().is_empty())
                            .context("workflow: a run step in the skeleton index has no ticket")?;
                        steps.insert(key.clone(), id.to_string());
                    }
                    Ok(StoredRun {
                        parent: parent.to_string(),
                        name: run_name.to_string(),
                        sha256: sha256.to_string(),
                        steps,
                        filing: stored_filing(&row["filing"]),
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let proposal_runs = v["proposal_runs"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let field = |key: &str| {
                        row[key]
                            .as_str()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string)
                    };
                    Some(ProposalRun {
                        proposal: field("proposal")?,
                        parent: field("parent")?,
                        sha256: field("sha256")?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let stored = Stored {
        name: v["name"].as_str().unwrap_or(name).to_string(),
        version: v["version"].as_i64().unwrap_or(0),
        sha256: v["sha256"].as_str().unwrap_or("").to_string(),
        accession: v["accession"].as_str().unwrap_or("").to_string(),
        body: v["body"].as_str().unwrap_or("").to_string(),
        proposal: v["proposal"].as_str().map(str::to_string),
        applied: v["applied"].as_str().map(str::to_string),
        versions,
        revisions,
        proposal_runs,
        runs,
    };
    let matches = |body: &str, sha: &str| sha256_hex(body.as_bytes()) == sha;
    if !matches(&stored.body, &stored.sha256)
        || stored
            .versions
            .iter()
            .any(|row| !matches(&row.body, &row.sha256))
    {
        bail!(
            "workflow: a skeleton body in the index of <value> in {dir} does not match its sha256"
        );
    }
    Ok(Some(stored))
}

/// A `filing` record with a step and a token. Anything else, such as a
/// bare step key, names no ticket, so the next start files a fresh one.
fn stored_filing(row: &Value) -> Option<Filing> {
    let field = |key: &str| {
        row[key]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let token = field("token").filter(|token| is_filing_token(token))?;
    Some(Filing {
        step: field("step")?,
        token,
    })
}

fn is_filing_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn filing_token() -> Result<String> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|err| anyhow::anyhow!("workflow: get a random filing token: {err}"))?;
    Ok(hex::encode(random))
}

fn pinned_stored(latest: &Stored, sha: &str, parent: &str, name: &str) -> Result<Stored> {
    if sha == latest.sha256 {
        return Ok(latest.clone());
    }
    let Some(old) = latest.versions.iter().find(|row| row.sha256 == sha) else {
        bail!("workflow: {parent} runs {name} at sha {sha}, and that copy is not stored");
    };
    Ok(Stored {
        name: latest.name.clone(),
        version: old.version,
        sha256: old.sha256.clone(),
        accession: old.accession.clone(),
        body: old.body.clone(),
        proposal: latest.proposal.clone(),
        applied: latest.applied.clone(),
        versions: Vec::new(),
        revisions: latest.revisions.clone(),
        proposal_runs: latest.proposal_runs.clone(),
        runs: latest.runs.clone(),
    })
}

fn require_stored(name: &str) -> Result<Stored> {
    read_stored(name)?.ok_or_else(|| {
        anyhow::anyhow!(
            "workflow: <value> is not stored; `ljos workflow define` stores the skeleton first"
        )
    })
}

fn write_stored(stored: &Stored) -> Result<()> {
    let path = index_path(&stored.name)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("workflow: create {}", dir.display()))?;
    }
    let versions: Vec<Value> = stored
        .versions
        .iter()
        .map(|row| {
            serde_json::json!({
                "version": row.version,
                "sha256": row.sha256,
                "accession": row.accession,
                "body": row.body,
            })
        })
        .collect();
    let revisions: Vec<Value> = stored
        .revisions
        .iter()
        .map(|row| {
            serde_json::json!({
                "proposal": row.proposal,
                "version": row.version,
                "sha256": row.sha256,
                "accession": row.accession,
            })
        })
        .collect();
    let proposal_runs: Vec<Value> = stored
        .proposal_runs
        .iter()
        .map(|row| {
            serde_json::json!({
                "proposal": row.proposal,
                "parent": row.parent,
                "sha256": row.sha256,
            })
        })
        .collect();
    let runs: Vec<Value> = stored
        .runs
        .iter()
        .map(|run| {
            serde_json::json!({
                "parent": run.parent,
                "name": run.name,
                "sha256": run.sha256,
                "steps": run.steps,
                "filing": run.filing.as_ref().map(|filing| serde_json::json!({
                    "step": filing.step,
                    "token": filing.token,
                })),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "name": stored.name,
        "version": stored.version,
        "sha256": stored.sha256,
        "accession": stored.accession,
        "body": stored.body,
        "proposal": stored.proposal,
        "applied": stored.applied,
        "versions": versions,
        "revisions": revisions,
        "proposal_runs": proposal_runs,
        "runs": runs,
    });
    let bytes = serde_json::to_string_pretty(&doc)?;
    write_index_atomic(&path, bytes.as_bytes())?;
    Ok(())
}

#[expect(
    clippy::disallowed_methods,
    reason = "renames a path that is not a git hook"
)]
fn write_index_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path
        .parent()
        .context("workflow: the skeleton index has no directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("workflow: create {}", dir.display()))?;
    if path
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        bail!("workflow: {} is a symlink", path.display());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(
        ".{}.{}.{nanos}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("index"),
        std::process::id()
    ));
    let write_tmp = || -> Result<()> {
        #[expect(
            clippy::disallowed_methods,
            reason = "opens a path that is not a git hook"
        )]
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .with_context(|| format!("workflow: create {}", tmp.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    };
    if let Err(err) = write_tmp() {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("workflow: replace {}", path.display()));
    }
    if let Ok(dir_file) = std::fs::File::open(dir) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}

struct SkeletonLock(std::fs::File);

fn acquire_skeleton_lock(name: &str) -> Result<SkeletonLock> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    check_skeleton_name(name)?;
    let index = index_path(name)?;
    let dir = index
        .parent()
        .context("workflow: the skeleton index has no directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("workflow: create {}", dir.display()))?;
    let lock_path = dir.join(format!("{name}.lock"));
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&lock_path)
        .with_context(|| format!("workflow: lock {}", lock_path.display()))?;
    let deadline = std::time::Instant::now() + run_lock_wait();
    let mut pause = std::time::Duration::from_millis(10);
    loop {
        // SAFETY: flock on a descriptor this function owns.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(SkeletonLock(file));
        }
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::EWOULDBLOCK | libc::EINTR) => {}
            _ => {
                return Err(err).with_context(|| format!("workflow: lock {}", lock_path.display()));
            }
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "workflow: timed out waiting for the skeleton lock for {name} at {}",
                lock_path.display()
            );
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        std::thread::sleep(pause.min(left));
        pause = pause
            .saturating_mul(2)
            .min(std::time::Duration::from_millis(100));
    }
}

impl Drop for SkeletonLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        // SAFETY: the descriptor is still open.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn define_named(name: &str, text: &str, source: &str, proposal: Option<&str>) -> Result<String> {
    let _lock = acquire_skeleton_lock(name)?;
    define_locked(name, text, source, proposal)
}

fn define_locked(name: &str, text: &str, source: &str, proposal: Option<&str>) -> Result<String> {
    let skel = parse_skeleton(text, source)?;
    let sha = sha256_hex(text.as_bytes());
    let prev = read_stored(name)?;
    if let Some(prev) = &prev {
        if prev.sha256 == sha {
            return Ok(format!(
                "{name} version {} is already stored\ndeed {}\nsha {}\n",
                prev.version, prev.accession, prev.sha256
            ));
        }
        if skel.version <= prev.version {
            bail!(
                "workflow: {source}:{}: the body changed and #+version is still {}; bump #+version",
                skel.version_line,
                skel.version
            );
        }
    }
    let accession = store_deed(name, skel.version, text)?;
    let mut versions = Vec::new();
    let mut kept_proposal = None;
    let mut kept_applied = None;
    let mut revisions = Vec::new();
    let mut proposal_runs = Vec::new();
    let mut runs = Vec::new();
    if let Some(prev) = prev {
        versions = prev.versions;
        if !versions.iter().any(|row| row.sha256 == prev.sha256) {
            versions.push(StoredVer {
                version: prev.version,
                sha256: prev.sha256,
                accession: prev.accession,
                body: prev.body,
            });
        }
        kept_proposal = prev.proposal;
        kept_applied = prev.applied;
        revisions = prev.revisions;
        proposal_runs = prev.proposal_runs;
        runs = prev.runs;
    }
    if let Some(proposal) = proposal {
        revisions.push(AppliedRevision {
            proposal: proposal.to_string(),
            version: skel.version,
            sha256: sha.clone(),
            accession: accession.clone(),
        });
    }
    let stored = Stored {
        name: name.to_string(),
        version: skel.version,
        sha256: sha.clone(),
        accession: accession.clone(),
        body: text.to_string(),
        proposal: proposal.map(str::to_string).or(kept_proposal),
        applied: proposal.map(str::to_string).or(kept_applied),
        versions,
        revisions,
        proposal_runs,
        runs,
    };
    write_stored(&stored)?;
    Ok(format!(
        "stored {name} version {}\ndeed {accession}\nsha {sha}\n",
        skel.version
    ))
}

fn store_deed(name: &str, version: i64, text: &str) -> Result<String> {
    ensure_deed_dir()?;
    let dir = std::env::temp_dir();
    let (mut file, path) = exclusive_deed_temp(&dir, name, version)?;
    std::io::Write::write_all(&mut file, text.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("workflow: write {}", path.display()))?;
    drop(file);
    let seat = crate::seat_name();
    let deed_name = format!("workflow-{name}-v{version}");
    let path_s = path.display().to_string();
    let said = crate::run_captured(
        "deedar",
        &[
            "create", "file", "--path", &path_s, "--agent", &seat, "--name", &deed_name,
        ],
    );
    let _ = std::fs::remove_file(&path);
    let said = said.with_context(|| format!("workflow: deedar did not store {name}"))?;
    accession_of(&said.stdout).ok_or_else(|| {
        anyhow::anyhow!(
            "workflow: deedar stored {name} and printed no deed id\n{}",
            said.stdout
        )
    })
}

/// Create a new file in `dir` with `O_EXCL` and `O_NOFOLLOW`.
///
/// The first name is the historical pid path. A symlink there makes `open`
/// fail, and the next name is tried. The link target is not opened.
fn exclusive_deed_temp(dir: &Path, name: &str, version: i64) -> Result<(std::fs::File, PathBuf)> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::FromRawFd;

    let pid = std::process::id();
    let mut names = vec![format!("ljos-workflow-{name}-{version}-{pid}.org")];
    for n in 0..16 {
        names.push(format!("ljos-workflow-{name}-{version}-{pid}-{n}.org"));
    }
    let mut last = None;
    for file_name in names {
        let path = dir.join(&file_name);
        let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            bail!(
                "workflow: temp path {} contains an interior NUL",
                path.display()
            );
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "creates a deed temp file with O_EXCL and O_NOFOLLOW, never a git hook"
        )]
        // SAFETY: `c_path` is a NUL-terminated path. `O_EXCL` creates the
        // name only when it is absent. `O_NOFOLLOW` fails when that name is
        // a symlink instead of writing through it. The descriptor is owned
        // by the `File` below.
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd >= 0 {
            #[expect(
                clippy::disallowed_methods,
                reason = "owns the descriptor opened above, never stdout or stderr"
            )]
            // SAFETY: `fd` was just opened and nothing else owns it.
            let file = unsafe { std::fs::File::from_raw_fd(fd) };
            return Ok((file, path));
        }
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::EEXIST | libc::ELOOP) => {
                last = Some(err);
            }
            _ => {
                return Err(err)
                    .with_context(|| format!("workflow: create a temp file at {}", path.display()))
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no temp name"))).with_context(|| {
        format!(
            "workflow: create a temp file in {} without following a symlink",
            dir.display()
        )
    })
}

fn accession_of(text: &str) -> Option<String> {
    for key in ["id=", "accession="] {
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix(key) {
                let id = rest.split_whitespace().next().unwrap_or("");
                if id.starts_with("deed-") {
                    return Some(id.to_string());
                }
            }
        }
    }
    text.split_whitespace()
        .find(|word| word.starts_with("deed-"))
        .map(str::to_string)
}

fn ensure_deed_dir() -> Result<()> {
    if let Ok(url) = std::env::var("DEEDAR_URL") {
        if let Some(path) = url.strip_prefix("file://") {
            if !path.is_empty() {
                std::fs::create_dir_all(path)
                    .with_context(|| format!("workflow: create {path}"))?;
            }
        }
        return Ok(());
    }
    if let Some(dir) = crate::data_base() {
        std::fs::create_dir_all(dir.join("deedar").join("store"))?;
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let dig = Sha256::digest(bytes);
    hex::encode(dig)
}

fn set_version(text: &str, version: i64) -> Result<String> {
    let mut found = false;
    let mut out = String::new();
    for line in split_keep(text) {
        let (body, nl) = split_nl(line);
        if keyword(body).is_some_and(|(key, _)| key.eq_ignore_ascii_case("version")) {
            if found {
                bail!("workflow: --file <value> has more than one #+version");
            }
            found = true;
            out.push_str(&format!("#+version: {version}"));
            if nl {
                out.push('\n');
            }
            continue;
        }
        out.push_str(line);
    }
    if !found {
        bail!("workflow: --file <value> has no #+version");
    }
    Ok(out)
}

struct Draft {
    source: String,
    title: Option<(String, usize)>,
    version: Option<(i64, usize)>,
    steps: Vec<Step>,
}

fn scan(text: &str, source: &str) -> Result<Draft> {
    let mut draft = Draft {
        source: source.to_string(),
        title: None,
        version: None,
        steps: Vec::new(),
    };
    let mut current: Option<Building> = None;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim_end();
        if let Some(step) = current.as_mut() {
            if step.in_props {
                if line.trim() == ":END:" {
                    step.in_props = false;
                    continue;
                }
                if line.trim().is_empty() {
                    continue;
                }
                read_prop(step, line, line_no, source)?;
                continue;
            }
            if line.trim() == ":PROPERTIES:" && step.body.trim().is_empty() {
                step.in_props = true;
                continue;
            }
            if heading_title(line).is_some() {
                draft.steps.push(finish_step(step, source)?);
                current = Some(Building::begin(line, line_no, source)?);
                continue;
            }
            step.body_line.get_or_insert(line_no);
            step.body.push_str(line);
            step.body.push('\n');
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(title) = heading_title(line) {
            if draft.title.is_none() || draft.version.is_none() {
                bail!("workflow: {source}:{line_no}: #+title and #+version come before the steps");
            }
            let _ = title;
            current = Some(Building::begin(line, line_no, source)?);
            continue;
        }
        if let Some((key, value)) = keyword(line) {
            if key.eq_ignore_ascii_case("title") {
                if draft.title.is_some() {
                    bail!("workflow: {source}:{line_no}: a second #+title");
                }
                if value.is_empty() {
                    bail!("workflow: {source}:{line_no}: #+title is empty");
                }
                draft.title = Some((value.to_string(), line_no));
                continue;
            }
            if key.eq_ignore_ascii_case("version") {
                if draft.version.is_some() {
                    bail!("workflow: {source}:{line_no}: a second #+version");
                }
                let version: i64 = value.parse().map_err(|_| {
                    anyhow::anyhow!(
                        "workflow: {source}:{line_no}: #+version {value:?} is not an integer"
                    )
                })?;
                draft.version = Some((version, line_no));
                continue;
            }
        }
    }
    if let Some(step) = current.as_ref() {
        draft.steps.push(finish_step(step, source)?);
    }
    if draft.title.is_none() {
        bail!("workflow: {source}:1: missing #+title");
    }
    if draft.version.is_none() {
        bail!("workflow: {source}:1: missing #+version");
    }
    if draft.steps.is_empty() {
        bail!("workflow: {source}:1: the skeleton has no steps");
    }
    Ok(draft)
}

struct Building {
    line: usize,
    title: String,
    in_props: bool,
    saw_props: bool,
    key: Option<(String, usize)>,
    depends: Option<(Vec<String>, usize)>,
    kind: Option<(Kind, usize)>,
    role: Option<(String, usize)>,
    quorum: Option<usize>,
    deed: Option<bool>,
    body: String,
    body_line: Option<usize>,
}

impl Building {
    fn begin(line: &str, line_no: usize, source: &str) -> Result<Self> {
        let title = heading_title(line).ok_or_else(|| {
            anyhow::anyhow!("workflow: {source}:{line_no}: a step heading is empty")
        })?;
        Ok(Self {
            line: line_no,
            title,
            in_props: false,
            saw_props: false,
            key: None,
            depends: None,
            kind: None,
            role: None,
            quorum: None,
            deed: None,
            body: String::new(),
            body_line: None,
        })
    }
}

fn read_prop(step: &mut Building, line: &str, line_no: usize, source: &str) -> Result<()> {
    step.saw_props = true;
    let Some((key, value)) = prop_pair(line) else {
        bail!("workflow: {source}:{line_no}: {line:?} is not a property");
    };
    match key {
        "STEP" => {
            if step.key.is_some() {
                bail!("workflow: {source}:{line_no}: a second :STEP:");
            }
            if !is_key(value) {
                bail!("workflow: {source}:{line_no}: step key {value:?} is empty or has a space");
            }
            step.key = Some((value.to_string(), line_no));
        }
        "DEPENDS" => {
            if step.depends.is_some() {
                bail!("workflow: {source}:{line_no}: a second :DEPENDS:");
            }
            let deps = value.split_whitespace().map(str::to_string).collect();
            step.depends = Some((deps, line_no));
        }
        "KIND" => {
            let Some(kind) = Kind::parse(value) else {
                bail!("workflow: {source}:{line_no}: kind {value:?} is not do, review or signoff");
            };
            step.kind = Some((kind, line_no));
        }
        "ROLE" => step.role = Some((value.to_string(), line_no)),
        "QUORUM" => {
            let n: usize = value.parse().map_err(|_| {
                anyhow::anyhow!(
                    "workflow: {source}:{line_no}: :QUORUM: {value:?} is not an integer"
                )
            })?;
            if n == 0 {
                bail!("workflow: {source}:{line_no}: :QUORUM: is 0");
            }
            step.quorum = Some(n);
        }
        "DEED" => {
            let yes = match value.to_ascii_lowercase().as_str() {
                "yes" => true,
                "no" => false,
                _ => bail!("workflow: {source}:{line_no}: :DEED: {value:?} is not yes or no"),
            };
            step.deed = Some(yes);
        }
        _ => bail!("workflow: {source}:{line_no}: unknown property :{key}:"),
    }
    Ok(())
}

fn finish_step(step: &Building, source: &str) -> Result<Step> {
    if !step.saw_props {
        bail!(
            "workflow: {source}:{}: {} has no property drawer",
            step.line,
            step.title
        );
    }
    let Some((key, key_line)) = step.key.clone() else {
        bail!(
            "workflow: {source}:{}: {} has no :STEP:",
            step.line,
            step.title
        );
    };
    let Some((depends, depends_line)) = step.depends.clone() else {
        bail!("workflow: {source}:{}: {key} has no :DEPENDS:", step.line);
    };
    let Some((kind, kind_line)) = step.kind else {
        bail!("workflow: {source}:{key_line}: {key} has no :KIND:");
    };
    let key_hits = crate::scrub::scan_step_key(&key, key_line);
    if !key_hits.is_empty() {
        bail!(
            "workflow: {source}: step key {key:?} failed public check: {}",
            key_hits.join("; ")
        );
    }
    let leading_blank_lines = step.body.chars().take_while(|c| *c == '\n').count();
    Ok(Step {
        key,
        title: step.title.clone(),
        depends,
        kind,
        role: step.role.as_ref().map(|(value, _)| value.clone()),
        quorum: step.quorum,
        deed: step.deed,
        body: step.body.trim_matches('\n').to_string(),
        line: step.line,
        body_line: step
            .body_line
            .unwrap_or(step.line)
            .saturating_add(leading_blank_lines),
        key_line,
        role_line: step.role.as_ref().map(|(_, line)| *line),
        kind_line,
        depends_line,
    })
}

fn validate(draft: Draft) -> Result<Skeleton> {
    let source = &draft.source;
    let mut seen = BTreeSet::new();
    for step in &draft.steps {
        if !seen.insert(step.key.clone()) {
            bail!(
                "workflow: {source}:{}: duplicate step key {:?}",
                step.line,
                step.key
            );
        }
    }
    let keys: BTreeSet<&str> = draft.steps.iter().map(|s| s.key.as_str()).collect();
    for step in &draft.steps {
        let mut listed = BTreeSet::new();
        for dep in &step.depends {
            if !listed.insert(dep.as_str()) {
                bail!(
                    "workflow: {source}:{}: {} lists {dep} twice in :DEPENDS:",
                    step.depends_line,
                    step.key
                );
            }
            if !keys.contains(dep.as_str()) {
                bail!(
                    "workflow: {source}:{}: {} depends on unknown step {dep:?}",
                    step.depends_line,
                    step.key
                );
            }
            if dep == &step.key {
                bail!(
                    "workflow: {source}:{}: cycle through {}",
                    step.depends_line,
                    step.key
                );
            }
        }
    }
    cycle_check(&draft.steps, source)?;
    let signoffs: Vec<&Step> = draft
        .steps
        .iter()
        .filter(|s| s.kind == Kind::Signoff)
        .collect();
    if signoffs.len() != 1 {
        let line = signoffs.get(1).map(|s| s.kind_line).unwrap_or(1);
        bail!("workflow: {source}:{line}: need exactly one signoff step");
    }
    let sign = signoffs[0];
    let reached = ancestors(sign, &draft.steps);
    for step in &draft.steps {
        if step.key == sign.key {
            continue;
        }
        if !reached.contains(step.key.as_str()) {
            bail!(
                "workflow: {source}:{}: signoff {} does not depend on {}",
                sign.depends_line,
                sign.key,
                step.key
            );
        }
    }
    let (title, title_line) = draft.title.unwrap();
    let (version, version_line) = draft.version.unwrap();
    Ok(Skeleton {
        title,
        title_line,
        version,
        version_line,
        steps: draft.steps,
        source: draft.source,
    })
}

fn cycle_check(steps: &[Step], source: &str) -> Result<()> {
    let mut color: BTreeMap<&str, u8> = BTreeMap::new();
    let mut stack: Vec<&str> = Vec::new();
    fn walk<'a>(
        key: &'a str,
        steps: &'a [Step],
        color: &mut BTreeMap<&'a str, u8>,
        stack: &mut Vec<&'a str>,
        source: &str,
    ) -> Result<()> {
        match color.get(key).copied().unwrap_or(0) {
            1 => {
                let at = stack.iter().position(|k| *k == key).unwrap_or(0);
                let mut names: Vec<&str> = stack[at..].to_vec();
                names.push(key);
                let line = steps
                    .iter()
                    .find(|s| s.key == key)
                    .map(|s| s.depends_line)
                    .unwrap_or(1);
                bail!(
                    "workflow: {source}:{line}: cycle through {}",
                    names.join(", ")
                );
            }
            2 => return Ok(()),
            _ => {}
        }
        color.insert(key, 1);
        stack.push(key);
        if let Some(step) = steps.iter().find(|s| s.key == key) {
            for dep in &step.depends {
                walk(dep, steps, color, stack, source)?;
            }
        }
        stack.pop();
        color.insert(key, 2);
        Ok(())
    }
    for step in steps {
        walk(&step.key, steps, &mut color, &mut stack, source)?;
    }
    Ok(())
}

fn ancestors<'a>(sign: &'a Step, steps: &'a [Step]) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&str> = sign.depends.iter().map(String::as_str).collect();
    while let Some(key) = stack.pop() {
        if !seen.insert(key) {
            continue;
        }
        if let Some(step) = steps.iter().find(|s| s.key == key) {
            stack.extend(step.depends.iter().map(String::as_str));
        }
    }
    seen
}

fn unique_len<'a>(depends: impl IntoIterator<Item = &'a String>) -> usize {
    let mut seen = BTreeSet::new();
    depends
        .into_iter()
        .filter(|dep| seen.insert(dep.as_str()))
        .count()
}

fn topo(skel: &Skeleton) -> Result<Vec<String>> {
    let mut indeg: BTreeMap<&str, usize> = BTreeMap::new();
    for step in &skel.steps {
        indeg.insert(&step.key, unique_len(&step.depends));
    }
    let mut ready: BTreeSet<&str> = indeg
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| *k)
        .collect();
    let mut order = Vec::new();
    while let Some(key) = ready.pop_first() {
        order.push(key.to_string());
        for step in &skel.steps {
            if step.depends.iter().any(|dep| dep == key) {
                let n = indeg.get_mut(step.key.as_str()).expect("step");
                *n = n.saturating_sub(1);
                if *n == 0 {
                    ready.insert(&step.key);
                }
            }
        }
    }
    if order.len() != skel.steps.len() {
        bail!("workflow: {}: the steps contain a cycle", skel.source);
    }
    Ok(order)
}

fn public_hits(skel: &Skeleton) -> Vec<String> {
    let screen = crate::scrub::public_screen();
    public_hits_with_screen(skel, &screen)
}

fn public_hits_with_screen(skel: &Skeleton, screen: &crate::public_text::Screen) -> Vec<String> {
    let mut hits = crate::scrub::scan_body_with_screen(&skel.title, skel.title_line, screen);
    for step in &skel.steps {
        let title_hits = crate::scrub::scan_body_with_screen(&step.title, step.line, screen);
        hits.extend(title_hits.iter().cloned());
        if let (Some(role), Some(line)) = (&step.role, step.role_line) {
            hits.extend(crate::scrub::scan_role_with_screen(role, line, screen));
        }
        if !step.body.is_empty() {
            let body_hits = crate::scrub::scan_body_with_screen(&step.body, step.body_line, screen);
            if title_hits.is_empty() && body_hits.is_empty() {
                hits.extend(crate::scrub::scan_body_with_screen(
                    &format!("{}{}", step.title, step.body),
                    step.line,
                    screen,
                ));
            }
            hits.extend(body_hits);
        }
    }
    hits.sort();
    hits.dedup();
    hits
}

fn render_org(skel: &Skeleton) -> String {
    let order = topo(skel).unwrap_or_else(|_| skel.steps.iter().map(|s| s.key.clone()).collect());
    let mut out = format!("#+title: {}\n#+version: {}\n\n", skel.title, skel.version);
    for key in &order {
        let Some(step) = skel.steps.iter().find(|s| s.key == *key) else {
            continue;
        };
        out.push_str(&format!("* {}\n:PROPERTIES:\n", step.title));
        out.push_str(&format!(":STEP: {}\n", step.key));
        if step.depends.is_empty() {
            out.push_str(":DEPENDS:\n");
        } else {
            out.push_str(&format!(":DEPENDS: {}\n", step.depends.join(" ")));
        }
        out.push_str(&format!(":KIND: {}\n", step.kind.word()));
        if let Some(role) = &step.role {
            out.push_str(&format!(":ROLE: {role}\n"));
        }
        if let Some(n) = step.quorum {
            out.push_str(&format!(":QUORUM: {n}\n"));
        }
        if let Some(yes) = step.deed {
            out.push_str(&format!(":DEED: {}\n", if yes { "yes" } else { "no" }));
        }
        out.push_str(":END:\n");
        if !step.body.is_empty() {
            out.push('\n');
            out.push_str(&step.body);
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str("#+begin_example\n");
    for key in &order {
        let Some(step) = skel.steps.iter().find(|s| s.key == *key) else {
            continue;
        };
        for dep in &step.depends {
            out.push_str(dep);
            out.push_str(" -> ");
            out.push_str(&step.key);
            out.push('\n');
        }
    }
    out.push_str("#+end_example\n");
    out
}

/// Test helper that exercises the same screen as workflow author text.
#[cfg(test)]
fn rendered_page_hits(page: &str, screen: &crate::public_text::Screen) -> Vec<String> {
    crate::scrub::scan_public_with_screen(page, screen)
}

fn fill_text(
    text: &str,
    vars: &BTreeMap<String, String>,
    line: usize,
    source: &str,
) -> Result<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            bail!("workflow: {source}:{line}: an unclosed placeholder starts near {start}");
        };
        let key = after[..end].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("workflow: {source}:{line}: placeholder {key:?} is not a variable name");
        }
        let Some(value) = vars.get(key) else {
            bail!(
                "workflow: {source}:{line}: unknown variable {key}; pass it with `--var {key}=VALUE`"
            );
        };
        if value_has_break_or_control(value) {
            bail!(
                "workflow: {source}:{line}: `--var` value for `{key}` contains a line break or control character"
            );
        }
        out.push_str(value);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn heading_title(line: &str) -> Option<String> {
    if !line.starts_with('*') {
        return None;
    }
    let stars = line.chars().take_while(|c| *c == '*').count();
    let rest = line[stars..].trim();
    if rest.is_empty() {
        return None;
    }
    Some(rest.to_string())
}

fn keyword(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("#+")?;
    let (key, value) = rest.split_once(':')?;
    Some((key.trim(), value.trim()))
}

fn prop_pair(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    let rest = line.strip_prefix(':')?;
    let (key, value) = rest.split_once(':')?;
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    Some((key, value.trim()))
}

fn is_key(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn split_keep(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    if rest.is_empty() {
        return out;
    }
    while let Some(at) = rest.find('\n') {
        out.push(&rest[..=at]);
        rest = &rest[at + 1..];
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

fn split_nl(line: &str) -> (&str, bool) {
    match line.strip_suffix('\n') {
        Some(body) => (body, true),
        None => (line, false),
    }
}

struct RunStep {
    key: String,
    id: String,
    state: String,
    kind: String,
    depends: Vec<String>,
    quorum: usize,
    deed: bool,
}

fn tracker_recs() -> Result<Vec<vissue_core::views::IssueRec>> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    layout.require_tracker()?;
    vissue_core::catalog::load_recs(&layout).map_err(anyhow::Error::from)
}

fn step_rows(recs: &[vissue_core::views::IssueRec], parent: &str) -> Result<Vec<RunStep>> {
    let mut rows = Vec::new();
    for rec in recs {
        if rec.heading.parent().map(str::trim) != Some(parent) {
            continue;
        }
        let Ok(card) = crate::tracker_show_json(&rec.heading.id) else {
            continue;
        };
        let Some(key) = step_key_of(&card) else {
            continue;
        };
        let kind = step_kind_of(&card).unwrap_or_else(|| "do".into());
        let quorum = quorum_of(&card);
        let deed = deed_of(&card);
        let depends = depends_of(&card);
        rows.push(RunStep {
            key,
            id: rec.heading.id.clone(),
            state: rec.heading.state.clone(),
            kind,
            depends,
            quorum,
            deed,
        });
    }
    Ok(order_rows(rows))
}

/// Steps whose dependencies are earlier come first. A tie is the key.
fn order_rows(rows: Vec<RunStep>) -> Vec<RunStep> {
    let mut indeg: BTreeMap<&str, usize> = BTreeMap::new();
    for row in &rows {
        indeg.insert(&row.key, 0);
    }
    for row in &rows {
        let mut seen = BTreeSet::new();
        let n = row
            .depends
            .iter()
            .filter(|dep| indeg.contains_key(dep.as_str()) && seen.insert(dep.as_str()))
            .count();
        if let Some(slot) = indeg.get_mut(row.key.as_str()) {
            *slot = n;
        }
    }
    let mut ready: BTreeSet<&str> = indeg
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| *k)
        .collect();
    let mut order = Vec::new();
    while let Some(key) = ready.pop_first() {
        order.push(key.to_string());
        for row in &rows {
            if row.depends.iter().any(|dep| dep == key) {
                if let Some(n) = indeg.get_mut(row.key.as_str()) {
                    *n = n.saturating_sub(1);
                    if *n == 0 {
                        ready.insert(&row.key);
                    }
                }
            }
        }
    }
    let mut left = rows;
    let mut out = Vec::new();
    for key in order {
        if let Some(at) = left.iter().position(|row| row.key == key) {
            out.push(left.remove(at));
        }
    }
    left.sort_by(|a, b| a.key.cmp(&b.key));
    out.extend(left);
    out
}

fn step_tags(step: &Step) -> String {
    let mut tags = vec![
        format!("step-{}", step.key),
        format!("kind-{}", step.kind.word()),
    ];
    let mut seen = BTreeSet::new();
    for dep in &step.depends {
        if seen.insert(dep.as_str()) {
            tags.push(format!("depends-{dep}"));
        }
    }
    if let Some(n) = step.quorum {
        tags.push(format!("quorum-{n}"));
    } else if step.kind == Kind::Signoff {
        tags.push("quorum-2".to_string());
    }
    if let Some(yes) = step.deed {
        tags.push(format!("deed-{}", if yes { "yes" } else { "no" }));
    }
    tags.join(",")
}

/// The ticket a start cut short filed for step `key` before it recorded the
/// id: the one child of `parent` that no recorded step names and that
/// carries the `filing-` tag with `token`, which the index held before the
/// create. Step tags and `step:` notes are not proof, since a ticket filed
/// by hand can carry the same ones. A child with no `step:` note gets it
/// here; a child noted for another step is not taken.
///
/// # Errors
///
/// More than one such child, or the tracker cannot be read or noted.
fn interrupted_step(
    recs: &[vissue_core::views::IssueRec],
    parent: &str,
    key: &str,
    token: &str,
    ids: &BTreeMap<String, String>,
) -> Result<Option<String>> {
    let tag = format!("{FILING_TAG}{token}");
    let mut found = Vec::new();
    for rec in recs {
        if rec.heading.parent().map(str::trim) != Some(parent)
            || ids.values().any(|id| *id == rec.heading.id)
        {
            continue;
        }
        let card = crate::tracker_show_json(&rec.heading.id)?;
        if !meta_tags(&card).contains(&tag) {
            continue;
        }
        match step_key_of(&card) {
            Some(noted) if noted == key => found.push((rec.heading.id.clone(), false)),
            None => found.push((rec.heading.id.clone(), true)),
            Some(_) => {}
        }
    }
    if found.len() > 1 {
        let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
        bail!(duplicate_filing_refusal(parent, key, &tag, &ids));
    }
    let Some((id, unnoted)) = found.pop() else {
        return Ok(None);
    };
    if unnoted {
        write_note(&id, &format!("{STEP_NOTE}{key}"))?;
    }
    Ok(Some(id))
}

/// The refusal for more than one child of `parent` that carries `tag` for
/// step `key`. Closing a child does not drop it from the search, so the
/// recovery is to take `tag` off every child but the one to keep.
fn duplicate_filing_refusal(parent: &str, key: &str, tag: &str, ids: &[&str]) -> String {
    format!(
        "workflow: {parent} has {} children filed for step {key} with the {tag} tag: {}; remove the {tag} tag from every one of them except the child to keep, then start again",
        ids.len(),
        ids.join(", ")
    )
}

/// File the ticket of `step` under `parent` with the `step:` note that binds
/// it and the `filing-` tag that names this attempt. The caller records the
/// id in the index before [`write_step_notes`].
fn file_step(parent: &str, project: &str, step: &Step, token: &str) -> Result<String> {
    let tags = format!("{},{FILING_TAG}{token}", step_tags(step));
    let body = if step.body.is_empty() {
        None
    } else {
        Some(step.body.as_str())
    };
    let (id, _, _, _) = crate::file_issue(
        project,
        Some(parent),
        Some(step.kind.ticket_type()),
        Some(&tags),
        None,
        body,
        &step.title,
        &crate::seat_name(),
        true,
        None,
    )?;
    if id.is_empty() {
        bail!("workflow: vissue filed no id for {}", step.key);
    }
    write_note(&id, &format!("{STEP_NOTE}{}", step.key))?;
    Ok(id)
}

fn write_step_notes(id: &str, step: &Step) -> Result<()> {
    write_note(id, &format!("{KIND_NOTE}{}", step.kind.word()))?;
    write_note(id, &format!("depends: {}", step.depends.join(" ")))?;
    if let Some(role) = &step.role {
        write_note(id, &format!("{ROLE_NOTE}{role}"))?;
    }
    if let Some(n) = step.quorum {
        write_note(id, &format!("{QUORUM_NOTE}{n}"))?;
    } else if step.kind == Kind::Signoff {
        write_note(id, &format!("{QUORUM_NOTE}2"))?;
    }
    if let Some(yes) = step.deed {
        write_note(
            id,
            &format!("{DEED_NOTE}{}", if yes { "yes" } else { "no" }),
        )?;
    }
    Ok(())
}

fn write_note(id: &str, text: &str) -> Result<()> {
    crate::run_captured("vissue", &["note", id, text])
        .with_context(|| format!("workflow: note on {id}"))?;
    Ok(())
}

fn notes_of(card: &Value) -> Vec<String> {
    card["logbook"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["note"].as_str().map(str::trim).map(str::to_string))
        .filter(|note| !note.is_empty())
        .collect()
}

fn is_internal_note(note: &str) -> bool {
    note.starts_with(STEP_NOTE)
        || note.starts_with(KIND_NOTE)
        || note.starts_with(QUORUM_NOTE)
        || note.starts_with(DEED_NOTE)
        || note.starts_with(ROLE_NOTE)
        || note.starts_with("depends: ")
}

/// The skeleton name on a parent, from the note the start command writes.
#[must_use]
pub fn bound_skeleton(card: &Value) -> Option<String> {
    first_note_value(&ordered_notes(card), NAME_NOTE)
}

/// Tags in file order, case preserved. `tags` comes before `org_tags`.
fn meta_tags(card: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["tags", "org_tags"] {
        let Some(rows) = card[key].as_array() else {
            continue;
        };
        for row in rows {
            let Some(tag) = row.as_str().map(str::trim).filter(|tag| !tag.is_empty()) else {
                continue;
            };
            if out
                .iter()
                .any(|have: &String| have.eq_ignore_ascii_case(tag))
            {
                continue;
            }
            out.push(tag.to_string());
        }
    }
    out
}

fn tag_rests(card: &Value, prefix: &str) -> Vec<String> {
    let prefix = prefix.to_ascii_lowercase();
    meta_tags(card)
        .into_iter()
        .filter_map(|tag| {
            let lower = tag.to_ascii_lowercase();
            let rest = lower.strip_prefix(&prefix)?;
            if rest.is_empty() {
                return None;
            }
            Some(tag[tag.len() - rest.len()..].to_string())
        })
        .collect()
}

fn step_key_of(card: &Value) -> Option<String> {
    first_note_value(&ordered_notes(card), STEP_NOTE)
}

fn step_kind_of(card: &Value) -> Option<String> {
    let kinds = tag_rests(card, "kind-");
    if kinds
        .iter()
        .any(|kind| kind.eq_ignore_ascii_case("signoff"))
    {
        return Some("signoff".into());
    }
    kinds
        .into_iter()
        .next()
        .map(|kind| kind.to_ascii_lowercase())
}

fn quorum_of(card: &Value) -> usize {
    tag_rests(card, "quorum-")
        .iter()
        .filter_map(|n| n.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .max()
        .unwrap_or(2)
}

fn deed_of(card: &Value) -> bool {
    tag_rests(card, "deed-")
        .iter()
        .any(|value| value.eq_ignore_ascii_case("yes"))
}

fn depends_of(card: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for dep in tag_rests(card, "depends-") {
        if !out.iter().any(|have| have == &dep) {
            out.push(dep);
        }
    }
    out
}

fn is_closed(state: &str) -> bool {
    matches!(state, "DONE" | "CANCELLED")
}

fn ready_keys(recs: &[vissue_core::views::IssueRec], rows: &[RunStep]) -> Vec<String> {
    let state: BTreeMap<&str, &str> = rows
        .iter()
        .map(|row| (row.key.as_str(), row.state.as_str()))
        .collect();
    let state_by_id: BTreeMap<&str, &str> = recs
        .iter()
        .map(|rec| (rec.heading.id.as_str(), rec.heading.state.as_str()))
        .collect();
    rows.iter()
        .filter(|row| !is_closed(&row.state))
        .filter(|row| {
            row.depends
                .iter()
                .all(|dep| state.get(dep.as_str()).is_some_and(|st| *st == "DONE"))
        })
        .filter(|row| !has_open_child(recs, &row.id))
        .filter(|row| !has_open_blocker(recs, &state_by_id, &row.id))
        .map(|row| row.key.clone())
        .collect()
}

fn has_open_child(recs: &[vissue_core::views::IssueRec], id: &str) -> bool {
    children_of(recs, id)
        .iter()
        .any(|(_, state, _)| !is_closed(state))
}

fn has_open_blocker(
    recs: &[vissue_core::views::IssueRec],
    state_by_id: &BTreeMap<&str, &str>,
    id: &str,
) -> bool {
    let Some(rec) = recs.iter().find(|rec| rec.heading.id == id) else {
        return false;
    };
    rec.heading
        .blocked_by()
        .iter()
        .any(|blocker| match state_by_id.get(blocker.as_str()) {
            Some(state) => !is_closed(state),
            None => false,
        })
}

fn children_of(
    recs: &[vissue_core::views::IssueRec],
    parent: &str,
) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for rec in recs {
        if rec.heading.parent().map(str::trim) == Some(parent) {
            out.push((
                rec.heading.id.clone(),
                rec.heading.state.clone(),
                rec.heading.title.clone(),
            ));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

struct OpenBlocker {
    step: String,
    id: String,
    state: String,
    title: String,
    via_edge: bool,
}

fn open_blockers(recs: &[vissue_core::views::IssueRec], rows: &[RunStep]) -> Vec<OpenBlocker> {
    let rec_by_id: BTreeMap<&str, &vissue_core::views::IssueRec> = recs
        .iter()
        .map(|rec| (rec.heading.id.as_str(), rec))
        .collect();
    let id_by_key: BTreeMap<&str, &str> = rows
        .iter()
        .map(|row| (row.key.as_str(), row.id.as_str()))
        .collect();
    let mut out = Vec::new();
    for row in rows {
        let mut seen = BTreeSet::new();
        for (id, state, title) in children_of(recs, &row.id) {
            if !is_closed(&state) && seen.insert(id.clone()) {
                out.push(OpenBlocker {
                    step: row.key.clone(),
                    id,
                    state,
                    title,
                    via_edge: false,
                });
            }
        }
        let dep_ids: BTreeSet<&str> = row
            .depends
            .iter()
            .filter_map(|dep| id_by_key.get(dep.as_str()).copied())
            .collect();
        if let Some(rec) = rec_by_id.get(row.id.as_str()) {
            let mut blocked = rec.heading.blocked_by();
            blocked.sort();
            for blocker_id in blocked {
                if dep_ids.contains(blocker_id.as_str()) {
                    continue;
                }
                match rec_by_id.get(blocker_id.as_str()) {
                    Some(b) => {
                        if !is_closed(&b.heading.state) && seen.insert(blocker_id.clone()) {
                            out.push(OpenBlocker {
                                step: row.key.clone(),
                                id: blocker_id,
                                state: b.heading.state.clone(),
                                title: b.heading.title.clone(),
                                via_edge: true,
                            });
                        }
                    }
                    None => {
                        if seen.insert(blocker_id.clone()) {
                            out.push(OpenBlocker {
                                step: row.key.clone(),
                                id: blocker_id,
                                state: "MISSING".into(),
                                title: "missing blocker ticket".into(),
                                via_edge: true,
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

fn signoff_lines(
    recs: &[vissue_core::views::IssueRec],
    rows: &[RunStep],
    sign: &RunStep,
) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    for row in rows {
        if row.id == sign.id {
            continue;
        }
        if row.state != "DONE" {
            missing.push(format!("{} is {}", row.key, row.state));
        }
    }
    for b in open_blockers(recs, rows) {
        if b.state == "MISSING" {
            continue;
        } else if b.via_edge {
            missing.push(format!(
                "blocker {} is open blocking {}: {} ({})",
                b.id, b.step, b.title, b.state
            ));
        } else {
            missing.push(format!(
                "blocker {} is open under {}: {} ({})",
                b.id, b.step, b.title, b.state
            ));
        }
    }
    match crate::reap::gate_quorum(&sign.id, sign.quorum, None) {
        Ok(crate::reap::QuorumGate::Met { .. }) => {}
        Ok(
            crate::reap::QuorumGate::Short { have, needed }
            | crate::reap::QuorumGate::Lapsed { have, needed },
        ) => {
            missing.push(format!("consensus has {have} of {needed} ballots"));
        }
        Err(err) => missing.push(format!("consensus could not be read: {err:#}")),
    }
    match settled_choice(&sign.id) {
        Ok(Some(word)) if word == "ship" => {}
        Ok(Some(word)) => missing.push(format!("consensus settled {word}, not ship")),
        Ok(None) => missing.push("consensus has not settled ship".into()),
        Err(err) => missing.push(format!("consensus could not be read: {err:#}")),
    }
    if sign.deed {
        missing.extend(deed_lines(&sign.id)?);
    }
    Ok(missing)
}

fn deed_lines(issue: &str) -> Result<Vec<String>> {
    let card = crate::tracker_show_json(issue)?;
    let ids = deed_ids(&card);
    if ids.is_empty() {
        return Ok(vec!["no deed is named on the step".into()]);
    }
    let mut bad = Vec::new();
    let mut passed = false;
    for id in &ids {
        match crate::run_captured("deedar", &["evidence", id]) {
            Ok(said) => {
                let _ = crate::evidence_here(&said.stdout);
                passed = true;
            }
            Err(err) => bad.push(format!("deed {id} did not pass evidence: {err:#}")),
        }
    }
    if passed {
        Ok(Vec::new())
    } else {
        Ok(bad)
    }
}

fn deed_ids(card: &Value) -> Vec<String> {
    match &card["deeds"] {
        Value::Array(rows) => rows
            .iter()
            .filter_map(|row| {
                row.as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
            .collect(),
        Value::String(text) => text.split_whitespace().map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

fn settled_choice(issue: &str) -> Result<Option<String>> {
    let from_tool = if crate::on_path("vissue") {
        crate::run_captured("vissue", &["consensus", issue])
            .ok()
            .and_then(|said| consensus_word(&said.stdout))
    } else {
        None
    };
    if from_tool.is_some() {
        return Ok(from_tool);
    }
    let raw = crate::raw_ballot_rows(issue).unwrap_or_default();
    let rows = crate::panel_gate::counted_ballots(issue, &raw);
    let choices: Vec<String> = rows
        .iter()
        .filter_map(|row| row["choice"].as_str().map(str::trim).map(str::to_string))
        .filter(|choice| !choice.is_empty())
        .collect();
    if choices.is_empty() {
        return Ok(None);
    }
    if choices.iter().all(|choice| choice == &choices[0]) {
        return Ok(Some(choices[0].clone()));
    }
    Ok(None)
}

fn consensus_word(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(rest) = line.split("holds:").nth(1) else {
            continue;
        };
        let word = rest.split_whitespace().next().unwrap_or("");
        let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
        if !word.is_empty() {
            return Some(word.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skel(text: &str) -> Result<Skeleton> {
        parse_skeleton(text, "sample.org")
    }

    fn fixed_public_screen() -> crate::public_text::Screen {
        crate::public_text::Screen {
            seats: vec!["kiln-q7zz".to_string()],
            internal: false,
        }
    }

    const OK: &str = "\
#+title: Release
#+version: 1

* Write the changelog
:PROPERTIES:
:STEP: changelog
:DEPENDS:
:KIND: do
:END:
Notes for {{version}}.

* Bump the version
:PROPERTIES:
:STEP: version
:DEPENDS: changelog
:KIND: do
:END:

* Sign off
:PROPERTIES:
:STEP: signoff
:DEPENDS: version
:KIND: signoff
:QUORUM: 2
:DEED: yes
:END:
Options: ship, hold
";

    #[test]
    fn a_name_like_src_dir_renders_and_inline_babel_is_refused() {
        let _env_read = crate::test_env_read_lock();
        let prose = OK.replace(
            "Notes for {{version}}.",
            "Set the src_dir option and keep my_src_file{x} as it is.",
        );
        let page = render_skeleton(&skel(&prose).unwrap()).unwrap();
        assert!(page.contains("src_dir option"), "{page}");
        let screen = fixed_public_screen();
        for line in [
            "src_sh{ls}",
            "run src_python[:var x=1]{x+1} now",
            "call_check(1)",
            "call_check[:var x=1](1)",
        ] {
            assert_eq!(
                rendered_page_hits(line, &screen),
                vec!["line 1 contains Org structure".to_string()],
                "{line}"
            );
        }
    }

    #[test]
    fn babel_written_in_another_case_or_encoded_is_refused() {
        let _env_read = crate::test_env_read_lock();
        let screen = fixed_public_screen();
        for line in [
            "#+BEGIN_SRC sh",
            "#+\u{0392}EGIN_SRC sh",
            "#+\u{ff22}egin_src sh",
            "%23%2Bbegin_src sh",
            "&#35;+CALL: x()",
            "SRC_SH{ls}",
            "#+begin=5Fsrc sh",
            ":RESULTS: silent",
        ] {
            assert_eq!(
                rendered_page_hits(line, &screen),
                vec!["line 1 contains Org structure".to_string()],
                "{line:?}"
            );
        }
    }

    #[test]
    fn fixed_roster_is_checked_in_each_render_field_and_the_page() {
        let _env_read = crate::test_env_read_lock();
        fn assert_refused(skel: &Skeleton, screen: &crate::public_text::Screen, line: usize) {
            assert_eq!(
                public_hits_with_screen(skel, screen),
                vec![format!("line {line} contains seat name")]
            );
            let page = render_org(skel);
            assert!(
                rendered_page_hits(&page, screen)
                    .iter()
                    .any(|hit| hit.ends_with("contains seat name")),
                "{page}"
            );
        }

        let screen = fixed_public_screen();
        let clean = skel(OK).unwrap();

        let mut title = clean.clone();
        title.title = "Release kiln-q7zz".to_string();
        assert_refused(&title, &screen, title.title_line);

        let mut step_title = clean.clone();
        step_title.steps[0].title = "Ask kiln-q7zz".to_string();
        assert_refused(&step_title, &screen, step_title.steps[0].line);

        let mut role = clean.clone();
        role.steps[0].role = Some("kiln-q7zz".to_string());
        role.steps[0].role_line = Some(role.steps[0].line);
        assert_refused(&role, &screen, role.steps[0].line);

        let mut body = clean;
        body.steps[0].body = "Ask kiln-q7zz for notes.".to_string();
        assert_refused(&body, &screen, body.steps[0].body_line);
    }

    #[test]
    fn a_secret_split_between_a_step_heading_and_body_is_refused() {
        let _env_read = crate::test_env_read_lock();
        let screen = fixed_public_screen();
        let (title, body) = ("Set the api_key", "= Zq8vRt3wPx9Lm2Kd");
        for alone in [title, body] {
            assert!(
                crate::scrub::scan_body_with_screen(alone, 1, &screen).is_empty(),
                "{alone:?} is refused alone, so it does not test the join"
            );
        }
        let mut split = skel(OK).unwrap();
        split.steps[0].title = title.to_string();
        split.steps[0].body = body.to_string();
        let hits = public_hits_with_screen(&split, &screen);
        assert!(hits.iter().any(|hit| hit.contains("secret")), "{hits:#?}");
    }

    #[test]
    fn a_valid_skeleton_parses_and_fills() {
        let _env_read = crate::test_env_read_lock();
        let parsed = skel(OK).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.steps.len(), 3);
        assert_eq!(parsed.steps[2].kind, Kind::Signoff);
        assert_eq!(parsed.steps[2].quorum, Some(2));
        assert_eq!(parsed.steps[2].deed, Some(true));
        let mut vars = BTreeMap::new();
        vars.insert("version".into(), "1.2.0".into());
        let filled = fill_vars(&parsed, &vars).unwrap();
        assert!(
            filled.steps[0].body.contains("1.2.0"),
            "{}",
            filled.steps[0].body
        );
        let err = fill_vars(&parsed, &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("sample.org:"), "{err}");
        assert!(err.contains("version"), "{err}");
        assert!(err.contains("--var"), "{err}");
    }

    #[test]
    fn a_title_only_placeholder_is_filled() {
        let _env_read = crate::test_env_read_lock();
        let titled = OK.replace("{{version}}", "the release").replacen(
            "#+title: ",
            "#+title: {{version}} ",
            1,
        );
        let parsed = skel(&titled).unwrap();
        assert!(parsed.title.contains("{{version}}"), "{}", parsed.title);
        let mut vars = BTreeMap::new();
        vars.insert("version".into(), "1.2.0".into());
        let filled = fill_vars(&parsed, &vars).unwrap();
        assert!(filled.title.starts_with("1.2.0 "), "{}", filled.title);
        assert!(!filled.title.contains("{{"), "{}", filled.title);
        let page = render_skeleton(&filled).unwrap();
        assert!(!page.contains("{{version}}"), "{page}");
    }

    #[test]
    fn errors_name_the_line() {
        let _env_read = crate::test_env_read_lock();
        let dup = OK.replace(":STEP: version", ":STEP: changelog");
        let err = skel(&dup).unwrap_err().to_string();
        assert!(err.contains("sample.org:"), "{err}");
        assert!(err.contains("duplicate step key"), "{err}");

        let unknown = OK.replace(":DEPENDS: changelog", ":DEPENDS: missing");
        let err = skel(&unknown).unwrap_err().to_string();
        assert!(err.contains("unknown step"), "{err}");
        assert!(err.contains("sample.org:"), "{err}");

        let cycle = "\
#+title: Loop
#+version: 1
* One
:PROPERTIES:
:STEP: one
:DEPENDS: two
:KIND: do
:END:
* Two
:PROPERTIES:
:STEP: two
:DEPENDS: one
:KIND: do
:END:
* Ship
:PROPERTIES:
:STEP: ship
:DEPENDS: one
:KIND: signoff
:END:
";
        let err = skel(cycle).unwrap_err().to_string();
        assert!(err.contains("cycle through"), "{err}");
        assert!(err.contains("sample.org:"), "{err}");

        let gap = "\
#+title: Gap
#+version: 1
* One
:PROPERTIES:
:STEP: one
:DEPENDS:
:KIND: do
:END:
* Two
:PROPERTIES:
:STEP: two
:DEPENDS:
:KIND: do
:END:
* Ship
:PROPERTIES:
:STEP: ship
:DEPENDS: one
:KIND: signoff
:END:
";
        let err = skel(gap).unwrap_err().to_string();
        assert!(err.contains("does not depend on two"), "{err}");

        let none = "\
#+title: None
#+version: 1
* One
:PROPERTIES:
:STEP: one
:DEPENDS:
:KIND: do
:END:
";
        let err = skel(none).unwrap_err().to_string();
        assert!(err.contains("exactly one signoff"), "{err}");

        let bad = "\
#+title: Bad
#+version: soon
* One
:PROPERTIES:
:STEP: one
:DEPENDS:
:KIND: do
:END:
* Ship
:PROPERTIES:
:STEP: ship
:DEPENDS: one
:KIND: signoff
:END:
";
        let err = skel(bad).unwrap_err().to_string();
        assert!(err.contains("not an integer"), "{err}");
        assert!(err.contains("sample.org:2:"), "{err}");
    }

    #[test]
    fn render_is_stable_and_refuses_private_text() {
        let _env_read = crate::test_env_read_lock();
        let text = OK.replace("Notes for {{version}}.", "Keep this prose byte for byte.");
        let parsed = skel(&text).unwrap();
        let once = render_skeleton(&parsed).unwrap();
        let twice = render_skeleton(&parsed).unwrap();
        assert_eq!(once, twice);
        assert!(once.contains("#+title: Release"), "{once}");
        assert!(!once.to_ascii_lowercase().contains("#+begin_src"), "{once}");
        assert!(once.contains("#+begin_example"), "{once}");
        assert!(
            once.find("* Write the changelog").unwrap() < once.find("* Bump the version").unwrap()
        );
        assert!(once.contains("changelog -> version"), "{once}");
        assert!(once.contains("Keep this prose byte for byte."), "{once}");
        let order = topo(&parsed).unwrap();
        assert_eq!(order, ["changelog", "version", "signoff"]);

        let private = OK.replace("Notes for {{version}}.", "Read /home/alice/src today.");
        let err = render_skeleton(&skel(&private).unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains("line "), "{err}");
        assert!(err.contains("private path"), "{err}");
    }

    #[test]
    fn a_var_that_is_not_a_pair_does_not_echo_the_argument() {
        let _env_read = crate::test_env_read_lock();
        let err = parse_vars(&["hunter2".into()]).unwrap_err().to_string();
        assert!(err.contains("key=value"), "{err}");
        assert!(!err.contains("hunter2"), "{err}");
        let err = parse_vars(&["=hunter2".into()]).unwrap_err().to_string();
        assert!(err.contains("empty variable name"), "{err}");
        assert!(!err.contains("hunter2"), "{err}");
    }

    #[test]
    fn var_errors_never_echo_a_typed_key_or_value() {
        let _env_read = crate::test_env_read_lock();
        for pair in [
            "bad-key=hunter2",
            "k\u{1b}[31mRED=hunter2",
            "sk-SECRETKEY=a\nb",
            "SECRETKEY=hunter2\nb",
            "SECRETKEY=hunter2\u{1b}[31m",
        ] {
            let err = parse_vars(&[pair.into()]).unwrap_err().to_string();
            assert!(!err.contains("bad-key"), "{err:?}");
            assert!(!err.contains("RED"), "{err:?}");
            assert!(!err.contains("SECRETKEY"), "{err:?}");
            assert!(!err.contains("hunter2"), "{err:?}");
        }

        let err = parse_vars(&["safe=first".into(), "safe=second".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("repeats a variable name"), "{err}");
        assert!(!err.contains("safe"), "{err}");

        let parsed = skel(OK).unwrap();
        let err = fill_vars(
            &parsed,
            &BTreeMap::from([
                ("version".to_string(), "1".to_string()),
                ("SECRETKEY".to_string(), "hunter2".to_string()),
            ]),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("does not use"), "{err}");
        assert!(!err.contains("SECRETKEY"), "{err}");
        assert!(!err.contains("hunter2"), "{err}");
    }

    #[test]
    fn a_dependency_listed_twice_is_refused() {
        let _env_read = crate::test_env_read_lock();
        let text = "\
#+title: Build
#+version: 1
* Build it
:PROPERTIES:
:STEP: build
:DEPENDS:
:KIND: do
:END:
* Ship it
:PROPERTIES:
:STEP: ship
:DEPENDS: build build
:KIND: signoff
:END:
";
        let err = skel(text).unwrap_err().to_string();
        assert!(err.contains("lists build twice"), "{err}");
        assert!(err.contains(":DEPENDS:"), "{err}");
        assert!(!err.contains("cycle"), "{err}");
    }

    #[test]
    fn step_identity_comes_from_its_first_note_and_attributes_come_from_tags() {
        let _env_read = crate::test_env_read_lock();
        let tagged = serde_json::json!({
            "tags": ["step-signoff", "kind-signoff", "kind-do", "quorum-1", "quorum-3", "deed-yes", "depends-Build"],
            "logbook": [
                {"note": "kind: do"},
                {"note": "kind: signoff"},
                {"note": "depends: other"}
            ]
        });
        assert_eq!(step_kind_of(&tagged).as_deref(), Some("signoff"));
        assert_eq!(step_key_of(&tagged), None);
        assert_eq!(quorum_of(&tagged), 3);
        assert!(deed_of(&tagged));
        assert_eq!(depends_of(&tagged), ["Build"]);
        let note_only = serde_json::json!({
            "logbook": [{"note": "kind: signoff"}, {"note": "step: signoff"}]
        });
        assert_eq!(step_kind_of(&note_only), None);
        assert_eq!(step_key_of(&note_only).as_deref(), Some("signoff"));
    }

    #[test]
    fn a_duplicate_filing_refusal_names_the_tag_every_child_and_the_recovery() {
        let _env_read = crate::test_env_read_lock();
        let tag = format!("{FILING_TAG}0123456789abcdef");
        let err = duplicate_filing_refusal(
            "demo-p1",
            "version",
            &tag,
            &["demo-c1", "demo-c2", "demo-c3"],
        );
        assert!(err.starts_with("workflow: demo-p1 has 3 children"), "{err}");
        assert!(err.contains("step version"), "{err}");
        assert!(err.contains("demo-c1, demo-c2, demo-c3"), "{err}");
        assert!(
            err.contains(
                "remove the filing-0123456789abcdef tag from every one of them except the child to keep, then start again"
            ),
            "{err}"
        );
        assert!(!err.contains("close all but one"), "{err}");
    }

    #[test]
    fn a_duplicate_filing_refusal_names_two_children() {
        let _env_read = crate::test_env_read_lock();
        let err =
            duplicate_filing_refusal("demo-p1", "changelog", "filing-feed", &["demo-a", "demo-b"]);
        assert!(err.contains("2 children filed for step changelog"), "{err}");
        assert!(
            err.contains("the filing-feed tag: demo-a, demo-b;"),
            "{err}"
        );
        assert!(err.contains("remove the filing-feed tag"), "{err}");
    }
}
