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
/// A pair has no `=`, or the key is empty.
pub fn parse_vars(pairs: &[String]) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            bail!("workflow: `{pair}` is not a key=value pair");
        };
        if key.is_empty() {
            bail!("workflow: `{pair}` has an empty variable name");
        }
        if value_has_break_or_control(value) {
            bail!("workflow: `--var` value for `{key}` contains a line break or control character");
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
    let mut out = skel.clone();
    for step in &mut out.steps {
        step.title = fill_text(&step.title, vars, step.line, &skel.source)?;
        step.body = fill_text(&step.body, vars, step.line, &skel.source)?;
    }
    Ok(out)
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
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("workflow: read {}", path.display()))?;
    let name = skeleton_name(path)?;
    define_named(&name, &text, &path.display().to_string(), None)
}

fn run_lock_dir(root: &Path) -> PathBuf {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return dot_git.join("ljos-locks");
    }
    if dot_git.is_file() {
        if let Ok(out) = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
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

fn acquire_run_lock(parent: &str) -> Result<crate::CommitLock> {
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
    crate::CommitLock::acquire_by(&lock_path, deadline).ok_or_else(|| {
        anyhow::anyhow!(
            "workflow: timed out waiting for the run lock for {parent} at {}",
            lock_path.display()
        )
    })
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
/// uses that copy, files a missing step and restores a missing edge.
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
    let _lock = acquire_run_lock(parent)?;
    let latest = require_stored(name)?;
    let parent_card = crate::tracker_show_json(parent)
        .with_context(|| format!("workflow: {parent} is not a ticket"))?;
    if crate::workflow_name_from_issue(&parent_card).is_some() {
        bail!("workflow: {parent} already runs the feature workflow");
    }
    let already = match note_rest(&parent_card, NAME_NOTE) {
        Some(have) if have != name => {
            bail!("workflow: {parent} already runs the {have} skeleton");
        }
        Some(_) => true,
        None => false,
    };
    let using = if already {
        match note_rest(&parent_card, SHA_NOTE) {
            Some(sha) => pinned_stored(&latest, &sha, parent, name)?,
            None => latest.clone(),
        }
    } else {
        latest
    };
    if let Some(path) = file {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("workflow: read {}", path.display()))?;
        let sha = sha256_hex(text.as_bytes());
        if sha != using.sha256 {
            bail!(
                "workflow: {} does not match the stored {name} skeleton {}",
                path.display(),
                using.sha256
            );
        }
    }
    let skel = parse_skeleton(&using.body, &format!("stored:{name}"))?;
    let recs = tracker_recs()?;
    let existing = step_rows(&recs, parent)?;
    let mut ids: BTreeMap<String, String> = existing
        .iter()
        .map(|row| (row.key.clone(), row.id.clone()))
        .collect();
    let filled = if skel.steps.iter().any(|step| !ids.contains_key(&step.key)) {
        Some(fill_vars(&skel, vars)?)
    } else {
        None
    };
    let mut out = String::new();
    if !already {
        write_note(parent, &format!("{NAME_NOTE}{name}"))?;
        write_note(parent, &format!("{VERSION_NOTE}{}", using.version))?;
        write_note(parent, &format!("{SHA_NOTE}{}", using.sha256))?;
        write_note(parent, &format!("{DEED_ID_NOTE}{}", using.accession))?;
        out.push_str(&format!(
            "{parent} runs {name} version {}\nsha {}\n",
            using.version, using.sha256
        ));
    }
    let mut filed_any = false;
    if let Some(filled) = &filled {
        let project = parent_card["project"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("workflow: {parent} has no project"))?
            .to_string();
        let order = topo(filled)?;
        for key in &order {
            if ids.contains_key(key) {
                continue;
            }
            let step = filled
                .steps
                .iter()
                .find(|step| step.key == *key)
                .expect("topo key");
            let id = file_step(parent, &project, step)?;
            ids.insert(step.key.clone(), id.clone());
            out.push_str(&format!("filed {} {id}\n", step.key));
            filed_any = true;
        }
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

/// The DAG, ready steps, open blockers and what the signoff still needs.
///
/// `None` when `parent` carries no skeleton.
///
/// # Errors
///
/// The tracker cannot be read.
pub fn status_report(parent: &str) -> Result<Option<String>> {
    let card = crate::tracker_show_json(parent)?;
    let Some(name) = note_rest(&card, NAME_NOTE) else {
        return Ok(None);
    };
    let version = note_rest(&card, VERSION_NOTE).unwrap_or_else(|| "?".into());
    let sha = note_rest(&card, SHA_NOTE).unwrap_or_else(|| "?".into());
    let recs = tracker_recs()?;
    let rows = step_rows(&recs, parent)?;
    let mut out = format!("{name} version {version} sha {sha} on {parent}\n");
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
    match rows.iter().find(|row| row.kind == "signoff") {
        None => out.push_str("  no signoff step\n"),
        Some(sign) => {
            let missing = signoff_lines(&recs, &rows, sign)?;
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
/// A ticket that is not a signoff step is left alone. A tracker that cannot
/// be read is left alone, so a claim id that is not a ticket still completes.
///
/// # Errors
///
/// The ticket is a signoff step and the run, the blockers, the consensus or
/// the deed is not ready. The error lists each missing piece.
pub fn signoff_block(issue: &str) -> Result<()> {
    let Ok(card) = crate::tracker_show_json(issue) else {
        return Ok(());
    };
    if step_kind_of(&card).as_deref() != Some("signoff") {
        return Ok(());
    }
    let parent = card["parent"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("signoff: {issue} has no parent run"))?;
    let recs = tracker_recs()?;
    let rows = step_rows(&recs, parent)?;
    let Some(sign) = rows.iter().find(|row| row.id == issue) else {
        bail!("signoff: {issue} is not a step of {parent}");
    };
    let missing = signoff_lines(&recs, &rows, sign)?;
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

/// File a proposal ticket whose options are `keep` and `revise`.
///
/// # Errors
///
/// `parent` is not a run of `name`, or the tracker refuses the ticket.
pub fn revise_from(name: &str, parent: &str) -> Result<String> {
    let mut stored = require_stored(name)?;
    let card = crate::tracker_show_json(parent)
        .with_context(|| format!("workflow: {parent} is not a ticket"))?;
    if note_rest(&card, NAME_NOTE).as_deref() != Some(name) {
        bail!("workflow: {parent} is not a run of {name}");
    }
    if let Some(id) = note_rest(&card, PROPOSAL_NOTE) {
        if let Ok(prop) = crate::tracker_show_json(&id) {
            let state = prop["state"].as_str().unwrap_or("");
            if !is_closed(state) {
                return Ok(format!("proposal {id} is already open for {name}\n"));
            }
        }
    }
    let recs = tracker_recs()?;
    let rows = step_rows(&recs, parent)?;
    let mut body = String::from("Options: keep, revise\n\n");
    let version = note_rest(&card, VERSION_NOTE).unwrap_or_else(|| "?".into());
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
    write_note(parent, &format!("{PROPOSAL_NOTE}{id}"))?;
    stored.proposal = Some(id.clone());
    write_stored(&stored)?;
    let _ = crate::persist_tracker(&id, "filed a proposal");
    Ok(format!(
        "filed proposal {id} for {name} on {parent}\nOptions: keep, revise\n"
    ))
}

/// Store `file` as version + 1 after consensus on the proposal settles `revise`.
///
/// The run that filed the proposal moves to that version. `--start` does not.
///
/// # Errors
///
/// There is no proposal, consensus did not settle `revise`, or the file is
/// not a skeleton.
pub fn revise_apply(name: &str, file: &Path) -> Result<String> {
    let _lock = acquire_skeleton_lock(name)?;
    let stored = require_stored(name)?;
    let Some(proposal) = stored.proposal.clone() else {
        bail!(
            "workflow: {name} has no proposal; `ljos workflow revise {name} --from PARENT` files one"
        );
    };
    if stored.applied.as_deref() == Some(proposal.as_str()) {
        return Ok(format!(
            "proposal {proposal} for {name} is already applied\n"
        ));
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
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("workflow: read {}", file.display()))?;
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
    let bumped = set_version(&text, next, file)?;
    let mut out = define_locked(name, &bumped, &file.display().to_string(), Some(&proposal))?;
    let mut stored = require_stored(name)?;
    stored.applied = Some(proposal.clone());
    write_stored(&stored)?;
    let _ = write_note(&proposal, &format!("revised {name} to version {next}"));
    if let Some(run) = proposal_parent(&proposal) {
        if let Ok(card) = crate::tracker_show_json(&run) {
            if note_rest(&card, NAME_NOTE).as_deref() == Some(name) {
                let stored = require_stored(name)?;
                write_note(&run, &format!("{VERSION_NOTE}{}", stored.version))?;
                write_note(&run, &format!("{SHA_NOTE}{}", stored.sha256))?;
                write_note(&run, &format!("{DEED_ID_NOTE}{}", stored.accession))?;
                out.push_str(&format!(
                    "{run} now runs {name} version {}\n",
                    stored.version
                ));
            }
        }
    }
    out.push_str(&format!("noted the proposal {proposal}\n"));
    close_proposal(&proposal)?;
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

fn proposal_parent(id: &str) -> Option<String> {
    let card = crate::tracker_show_json(id).ok()?;
    let parent = card["parent"].as_str()?.trim();
    if parent.is_empty() {
        None
    } else {
        Some(parent.to_string())
    }
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
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("workflow: read {}", path.display()))?;
        return parse_skeleton(&text, &path.display().to_string());
    }
    if let Some(stored) = read_stored(name)? {
        return parse_skeleton(&stored.body, &format!("stored:{name}"));
    }
    let path = default_path(name);
    if path.is_file() {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("workflow: read {}", path.display()))?;
        return parse_skeleton(&text, &path.display().to_string());
    }
    bail!(
        "workflow: {name} is not stored and {} is not a file",
        path.display()
    );
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
        bail!(
            "workflow: {} needs a file name of letters, digits, hyphens or underscores",
            path.display()
        );
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
struct Stored {
    name: String,
    version: i64,
    sha256: String,
    accession: String,
    body: String,
    proposal: Option<String>,
    applied: Option<String>,
    versions: Vec<StoredVer>,
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
    bail!("workflow: `{name}` is not a skeleton name");
}

fn index_path(name: &str) -> Result<PathBuf> {
    check_skeleton_name(name)?;
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        })
        .ok_or_else(|| {
            anyhow::anyhow!("workflow: set HOME so the skeleton index has a directory")
        })?;
    Ok(base
        .join("ljos")
        .join("workflows")
        .join(format!("{name}.json")))
}

fn read_stored(name: &str) -> Result<Option<Stored>> {
    let path = index_path(name)?;
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("workflow: read {}", path.display()))?;
    let v: Value =
        serde_json::from_str(&text).with_context(|| format!("workflow: {}", path.display()))?;
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
    Ok(Some(Stored {
        name: v["name"].as_str().unwrap_or(name).to_string(),
        version: v["version"].as_i64().unwrap_or(0),
        sha256: v["sha256"].as_str().unwrap_or("").to_string(),
        accession: v["accession"].as_str().unwrap_or("").to_string(),
        body: v["body"].as_str().unwrap_or("").to_string(),
        proposal: v["proposal"].as_str().map(str::to_string),
        applied: v["applied"].as_str().map(str::to_string),
        versions,
    }))
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
    })
}

fn require_stored(name: &str) -> Result<Stored> {
    read_stored(name)?.ok_or_else(|| {
        anyhow::anyhow!(
            "workflow: {name} is not stored; `ljos workflow define` stores the skeleton first"
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
    let doc = serde_json::json!({
        "name": stored.name,
        "version": stored.version,
        "sha256": stored.sha256,
        "accession": stored.accession,
        "body": stored.body,
        "proposal": stored.proposal,
        "applied": stored.applied,
        "versions": versions,
    });
    let bytes = serde_json::to_string_pretty(&doc)?;
    write_index_atomic(&path, bytes.as_bytes())?;
    Ok(())
}

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
    }
    let stored = Stored {
        name: name.to_string(),
        version: skel.version,
        sha256: sha.clone(),
        accession: accession.clone(),
        body: text.to_string(),
        proposal: proposal.map(str::to_string).or(kept_proposal),
        applied: kept_applied,
        versions,
    };
    write_stored(&stored)?;
    Ok(format!(
        "stored {name} version {}\ndeed {accession}\nsha {sha}\n",
        skel.version
    ))
}

fn store_deed(name: &str, version: i64, text: &str) -> Result<String> {
    ensure_deed_dir()?;
    let tmp = std::env::temp_dir().join(format!(
        "ljos-workflow-{name}-{version}-{}.org",
        std::process::id()
    ));
    std::fs::write(&tmp, text).with_context(|| format!("workflow: write {}", tmp.display()))?;
    let seat = crate::seat_name();
    let deed_name = format!("workflow-{name}-v{version}");
    let said = crate::run_captured(
        "deedar",
        &[
            "create",
            "file",
            "--path",
            &tmp.display().to_string(),
            "--agent",
            &seat,
            "--name",
            &deed_name,
        ],
    );
    let _ = std::fs::remove_file(&tmp);
    let said = said.with_context(|| format!("workflow: deedar did not store {name}"))?;
    accession_of(&said.stdout).ok_or_else(|| {
        anyhow::anyhow!(
            "workflow: deedar stored {name} and printed no deed id\n{}",
            said.stdout
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

fn set_version(text: &str, version: i64, path: &Path) -> Result<String> {
    let mut found = false;
    let mut out = String::new();
    for line in split_keep(text) {
        let (body, nl) = split_nl(line);
        if keyword(body).is_some_and(|(key, _)| key.eq_ignore_ascii_case("version")) {
            if found {
                bail!("workflow: {} has more than one #+version", path.display());
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
        bail!("workflow: {} has no #+version", path.display());
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
        for dep in &step.depends {
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

fn topo(skel: &Skeleton) -> Result<Vec<String>> {
    let mut indeg: BTreeMap<&str, usize> = BTreeMap::new();
    for step in &skel.steps {
        indeg.insert(&step.key, step.depends.len());
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
    let mut hits = crate::scrub::scan_body(&skel.title, skel.title_line);
    for step in &skel.steps {
        hits.extend(crate::scrub::scan_body(&step.title, step.line));
        if let (Some(role), Some(line)) = (&step.role, step.role_line) {
            hits.extend(crate::scrub::scan_role(role, line));
        }
        if !step.body.is_empty() {
            hits.extend(crate::scrub::scan_body(&step.body, step.body_line));
        }
    }
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
    out.push_str("#+begin_src dot\n");
    out.push_str("digraph workflow {\n");
    out.push_str("  rankdir=LR;\n");
    for key in &order {
        let Some(step) = skel.steps.iter().find(|s| s.key == *key) else {
            continue;
        };
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\"];\n",
            dot_escape(&step.key),
            dot_escape(&step.title)
        ));
    }
    for key in &order {
        let Some(step) = skel.steps.iter().find(|s| s.key == *key) else {
            continue;
        };
        for dep in &step.depends {
            out.push_str(&format!(
                "  \"{}\" -> \"{}\";\n",
                dot_escape(dep),
                dot_escape(&step.key)
            ));
        }
    }
    out.push_str("}\n");
    out.push_str("#+end_src\n");
    out
}

fn dot_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
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
        let quorum = note_rest(&card, QUORUM_NOTE)
            .and_then(|n| n.parse().ok())
            .unwrap_or(2);
        let deed = note_rest(&card, DEED_NOTE).as_deref() == Some("yes");
        let depends = note_rest(&card, "depends: ")
            .map(|text| text.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
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
        let n = row
            .depends
            .iter()
            .filter(|dep| indeg.contains_key(dep.as_str()))
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

fn file_step(parent: &str, project: &str, step: &Step) -> Result<String> {
    let tags = format!("step-{},kind-{}", step.key, step.kind.word());
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
    write_note(&id, &format!("{KIND_NOTE}{}", step.kind.word()))?;
    write_note(&id, &format!("depends: {}", step.depends.join(" ")))?;
    if let Some(role) = &step.role {
        write_note(&id, &format!("{ROLE_NOTE}{role}"))?;
    }
    if let Some(n) = step.quorum {
        write_note(&id, &format!("{QUORUM_NOTE}{n}"))?;
    } else if step.kind == Kind::Signoff {
        write_note(&id, &format!("{QUORUM_NOTE}2"))?;
    }
    if let Some(yes) = step.deed {
        write_note(
            &id,
            &format!("{DEED_NOTE}{}", if yes { "yes" } else { "no" }),
        )?;
    }
    Ok(id)
}

fn write_note(id: &str, text: &str) -> Result<()> {
    crate::run_captured("vissue", &["note", id, text])
        .with_context(|| format!("workflow: note on {id}"))?;
    Ok(())
}

fn note_rest(card: &Value, prefix: &str) -> Option<String> {
    crate::latest_note_value(card, prefix)
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

fn tag_list(card: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["tags", "org_tags"] {
        if let Some(rows) = card[key].as_array() {
            for row in rows {
                if let Some(tag) = row.as_str() {
                    out.push(tag.to_ascii_lowercase());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn step_key_of(card: &Value) -> Option<String> {
    if let Some(key) = note_rest(card, STEP_NOTE) {
        return Some(key);
    }
    tag_list(card)
        .into_iter()
        .find_map(|tag| tag.strip_prefix("step-").map(str::to_string))
}

fn step_kind_of(card: &Value) -> Option<String> {
    if let Some(kind) = note_rest(card, KIND_NOTE) {
        return Some(kind);
    }
    tag_list(card)
        .into_iter()
        .find_map(|tag| tag.strip_prefix("kind-").map(str::to_string))
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
    fn a_valid_skeleton_parses_and_fills() {
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
    fn errors_name_the_line() {
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
        let text = OK.replace("Notes for {{version}}.", "Keep this prose byte for byte.");
        let parsed = skel(&text).unwrap();
        let once = render_skeleton(&parsed).unwrap();
        let twice = render_skeleton(&parsed).unwrap();
        assert_eq!(once, twice);
        assert!(once.contains("#+title: Release"), "{once}");
        assert!(once.contains("#+begin_src dot"), "{once}");
        assert!(
            once.find("* Write the changelog").unwrap() < once.find("* Bump the version").unwrap()
        );
        assert!(once.contains("\"changelog\" -> \"version\""), "{once}");
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
}
