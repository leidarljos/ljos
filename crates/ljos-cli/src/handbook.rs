//! A deterministic Org export of one project's public notes.
//!
//! Only what the person marked public leaves: a lesson, a preference or a
//! rule with the public entity, a ticket tagged `public`, and a workflow file
//! with `#+public: yes`. The public-text screen is a backstop over those
//! items. A hit in a marked item refuses the whole export.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;
use vissue_core::views::IssueRec;

use crate::handbook_ids::IdScreen;
use crate::public_text::{self, Screen};

/// The title when `--title` is not given. It names no project.
pub const DEFAULT_TITLE: &str = "Project handbook";

const MEMORY_KINDS: &[&str] = &["lesson", "preference", "rule"];

/// The ticket tag that marks a ticket public.
const PUBLIC_TAG: &str = "public";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRow {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub entities: Vec<String>,
    pub kept: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyRow {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub pattern: Option<String>,
    pub verdict: Option<String>,
    pub entities: Vec<String>,
    pub kept: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowRow {
    pub name: String,
    pub title: String,
    /// The file says `#+public: yes`.
    pub public: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketRow {
    pub id: String,
    pub project: String,
    pub state: String,
    pub title: String,
    pub body: String,
    pub notes: Vec<String>,
    pub parent: Option<String>,
    pub blocked_by: Vec<String>,
    /// The ticket carries the tag `public`.
    pub public: bool,
}

/// The marked items that read as private. One of these refuses the export.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refusals {
    lines: Vec<String>,
}

impl Refusals {
    /// Record one marked item that did not pass, by source and class. The
    /// text itself is never kept.
    fn refuse(&mut self, source: &str, class: &str) {
        self.lines
            .push(format!("handbook: refused {source}: {class}"));
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The error that names every refused item and says nothing was written.
    fn error(&self) -> anyhow::Error {
        let n = self.lines.len();
        let plural = if n == 1 { "item is" } else { "items are" };
        anyhow::anyhow!(
            "{}\nhandbook: nothing was written, because {n} marked {plural} not public-safe",
            self.lines.join("\n")
        )
    }
}

/// Whether the entities carry the public mark.
fn is_public(entities: &[String]) -> bool {
    entities.iter().any(|entity| entity == crate::PUBLIC_ENTITY)
}

/// A source label for a diagnostic. An id that is itself private is not
/// printed.
fn named(kind: &str, id: &str, screen: &Screen) -> String {
    if id.is_empty() || public_text::classify_id(id, screen).is_some() {
        format!("{kind} (id withheld)")
    } else {
        format!("{kind} {id}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub project: String,
    pub tags: Vec<String>,
    pub trees: Vec<String>,
    pub query: Option<String>,
    /// The text of `--title`. Without it the title is [`DEFAULT_TITLE`].
    pub title: Option<String>,
    pub ids: bool,
    pub include_unreviewed: bool,
    pub workflows: Option<PathBuf>,
    pub date: Option<String>,
    pub seats: Vec<String>,
    /// Project names the tracker and the router know. Filled in from the
    /// tracker, not from the command line.
    pub projects: Vec<String>,
    /// The `id_length` of each tracker the router reaches.
    pub id_lengths: Vec<usize>,
}

struct KeptTicket {
    row: TicketRow,
    title: String,
    summary: Vec<String>,
}

/// Write the handbook, or compare an existing file with a fresh export.
///
/// # Errors
///
/// The project is unknown, the pack or the tracker cannot be read, the
/// output cannot be written, or `--check` finds drift or unsafe text.
pub fn run(options: &Options, out: &Path, check: bool) -> Result<()> {
    let mut options = options.clone();
    let asked = options.project.clone();
    options.project = crate::require_known_project(&asked)
        .map_err(|_| anyhow::anyhow!("handbook: --project is not a project"))?;
    let dest = written_file(out);
    if check {
        return check_output(&options, dest.as_deref());
    }
    let text = export(&options, dest.as_deref())?;
    write_output(out, &text)?;
    Ok(())
}

fn written_file(out: &Path) -> Option<PathBuf> {
    if to_stdout(out) {
        None
    } else {
        Some(file_for(out))
    }
}

/// The file `--out` names: `faq.org` inside a directory, else the path
/// itself. A link to a directory is a link, so it is not entered.
fn file_for(out: &Path) -> PathBuf {
    if std::fs::symlink_metadata(out).is_ok_and(|meta| meta.is_dir()) {
        out.join("faq.org")
    } else {
        out.to_path_buf()
    }
}

fn check_output(options: &Options, dest: Option<&Path>) -> Result<()> {
    let Some(dest) = dest else {
        bail!("handbook: --check needs --out FILE");
    };
    let existing =
        std::fs::read_to_string(dest).with_context(|| "handbook: cannot read the --out file")?;
    if existing.is_empty() {
        bail!("handbook: the --out file is empty");
    }
    let (loaded, atoms) = prepare(options)?;
    let fresh = compose(&loaded, &atoms, Some(dest))?;
    let screen = screen_of(&loaded);
    let mut failed = false;
    if existing.as_bytes() != fresh.as_bytes() {
        failed = true;
        crate::say!("handbook: the --out file differs from a fresh export");
    }
    failed |= report_lines("the --out file", &existing, &screen);
    for path in workflow_paths(&loaded)? {
        if same_file(&path, dest) {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| "handbook: cannot read a workflow file")?;
        if !workflow_public(&text) {
            continue;
        }
        failed |= report_lines("a workflow file", &text, &screen);
    }
    if failed {
        bail!("handbook: public-safety check failed");
    }
    Ok(())
}

/// Lines the renderer writes that are Org structure. Text from an item that
/// looks like one is written with an escape in front, so it is not equal.
const STRUCTURE_LINES: &[&str] = &[
    "#+options: toc:2 num:nil",
    "#+TODO: TODO | DONE CANCELLED",
    "#+begin_src dot",
    "#+end_src",
];

/// How the screen reads one rendered line. A line the renderer writes to
/// point at a workflow file is checked by the name inside it: the `file:`
/// link around the name is structure, not a path someone typed. An `:ID:`
/// line is checked as an id, so a 32-digit pack id is not a secret.
fn screen_line(line: &str) -> public_text::Line<'_> {
    if let Some(id) = line.strip_prefix(":ID: ") {
        return public_text::Line::id(id);
    }
    if STRUCTURE_LINES.contains(&line)
        || line.starts_with("#+title: ")
        || line.starts_with("#+date: ")
        || matches!(line, ":PROPERTIES:" | ":END:")
        || line
            .strip_prefix('*')
            .and_then(|rest| rest.chars().next())
            .is_some_and(char::is_whitespace)
    {
        return public_text::Line::structure(line);
    }
    let name = line
        .strip_prefix("- [[file:")
        .and_then(|rest| rest.strip_suffix(".org]]"))
        .and_then(|rest| rest.split_once(".org]["))
        .filter(|(path, label)| path == label);
    match name {
        Some((path, _)) => public_text::Line::text(path),
        None => public_text::Line::text(line),
    }
}

/// The lines of rendered text that are not public, with the class of each.
/// This is the one check for an item about to be written, for the whole
/// export and for `--check`, so they cannot disagree.
fn unsafe_lines(text: &str, screen: &Screen) -> Vec<(usize, public_text::Class)> {
    let lines: Vec<public_text::Line<'_>> = text.lines().map(screen_line).collect();
    public_text::scan(&lines, screen)
        .into_iter()
        .enumerate()
        .filter_map(|(index, class)| class.map(|class| (index, class)))
        .collect()
}

/// The first private pattern in a block of lines about to be written.
fn gate_block(block: &str, screen: &Screen) -> Option<public_text::Class> {
    unsafe_lines(block, screen).first().map(|(_, class)| *class)
}

/// Append `block` when the exact lines to be written are public. Otherwise
/// record the source and class, and the export is refused at the end.
fn push_block(
    out: &mut String,
    block: &str,
    screen: &Screen,
    refused: &mut Refusals,
    source: &str,
) {
    match gate_block(block, screen) {
        Some(class) => refused.refuse(source, class.name()),
        None => out.push_str(block),
    }
}

fn report_lines(what: &str, text: &str, screen: &Screen) -> bool {
    let found = unsafe_lines(text, screen);
    for (index, _) in &found {
        crate::say!("{what}:{}: unsafe text", index + 1);
    }
    !found.is_empty()
}

fn export(options: &Options, skip: Option<&Path>) -> Result<String> {
    let (loaded, atoms) = prepare(options)?;
    compose(&loaded, &atoms, skip)
}

fn prepare(options: &Options) -> Result<(Options, Vec<Value>)> {
    let atoms = list_atoms()?;
    let mut loaded = options.clone();
    loaded.seats = seat_names(&atoms);
    (loaded.projects, loaded.id_lengths) = tracker_ids();
    Ok((loaded, atoms))
}

fn compose(options: &Options, atoms: &[Value], skip: Option<&Path>) -> Result<String> {
    let reviews = crate::memories::reviews();
    let (memories, mut policies) = rows_from(atoms, &reviews);
    policies.extend(user_policies()?);
    let tickets = if tracker_present() {
        load_tickets()?
    } else if options.trees.is_empty() {
        Vec::new()
    } else {
        load_tickets()?
    };
    let workflows = load_workflows(options, skip)?;
    let text = render(options, &memories, &policies, &tickets, &workflows)?;
    let screen = screen_of(options);
    if let Some((index, class)) = unsafe_lines(&text, &screen).first() {
        bail!(
            "handbook: refused to write unsafe text: line {} reads as {}",
            index + 1,
            class.name()
        );
    }
    Ok(text)
}

fn screen_of(options: &Options) -> Screen {
    Screen {
        seats: options.seats.clone(),
        internal: true,
    }
}

fn list_atoms() -> Result<Vec<Value>> {
    let client = crate::pack()?;
    let workspace = client.workspace();
    let mut atoms = Vec::new();
    for kind in MEMORY_KINDS {
        let (filtered, _) =
            crate::atoms_listed_of(&client, &workspace, true, Some(kind), None, false)?;
        atoms.extend(filtered.iter().cloned());
    }
    Ok(atoms)
}

fn seat_names(atoms: &[Value]) -> Vec<String> {
    let mut names = BTreeSet::new();
    for name in crate::tracker_known_seats() {
        push_seat(&mut names, &name);
    }
    for name in crate::tracker_roster_names() {
        push_seat(&mut names, &name);
    }
    for atom in atoms {
        if let Some(name) = atom["source"]["seat"].as_str() {
            push_seat(&mut names, name);
        }
        for entity in atom_entities(atom) {
            if let Some(name) = entity.strip_prefix(crate::SEAT_ENTITY) {
                push_seat(&mut names, name);
            }
        }
    }
    names.into_iter().collect()
}

fn push_seat(names: &mut BTreeSet<String>, name: &str) {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return;
    }
    names.insert(name.to_string());
}

fn rows_from(
    atoms: &[Value],
    reviews: &BTreeMap<String, crate::memories::Review>,
) -> (Vec<MemoryRow>, Vec<PolicyRow>) {
    let now = crate::now_utc();
    let mut memories = Vec::new();
    let mut policies = Vec::new();
    for atom in atoms
        .iter()
        .filter(|atom| atom["tombstone"] != Value::Bool(true) && crate::live_now(atom, &now))
    {
        let Some(kind) = atom["kind"].as_str() else {
            continue;
        };
        let id = atom["id"].as_str().unwrap_or("").to_string();
        let kept = kept_atom(atom, &id, reviews);
        let text = atom["text"].as_str().unwrap_or("").to_string();
        let entities = atom_entities(atom);
        if kind == "lesson" {
            memories.push(MemoryRow {
                id,
                kind: kind.to_string(),
                text,
                entities,
                kept,
            });
        } else if matches!(kind, "preference" | "rule") {
            policies.push(PolicyRow {
                id,
                kind: kind.to_string(),
                text,
                pattern: atom["pattern"].as_str().map(str::to_string),
                verdict: atom["verdict"].as_str().map(str::to_string),
                entities,
                kept,
            });
        }
    }
    (memories, policies)
}

fn kept_atom(atom: &Value, id: &str, reviews: &BTreeMap<String, crate::memories::Review>) -> bool {
    if reviews
        .get(id)
        .is_some_and(|review| review.verdict == "drop")
    {
        return false;
    }
    if reviews
        .get(id)
        .is_some_and(|review| review.verdict == "keep")
    {
        return true;
    }
    atom["level"].as_str() == Some(crate::admit::LEVEL_EXPLICIT) || crate::person_stored(atom)
}

fn user_policies() -> Result<Vec<PolicyRow>> {
    let rules = crate::user_rules::load()?;
    Ok(rules
        .into_iter()
        .map(|rule| {
            let kind = if rule.pattern.is_some() {
                "rule"
            } else {
                "preference"
            };
            PolicyRow {
                id: rule.id,
                kind: kind.to_string(),
                text: rule.text,
                pattern: rule.pattern,
                verdict: rule.verdict,
                entities: rule.entities,
                kept: true,
            }
        })
        .collect())
}

fn tracker_present() -> bool {
    vissue_core::Layout::resolve(None, None)
        .and_then(|layout| layout.require_tracker())
        .is_ok()
}

/// The project names and `id_length` values of the trackers the router
/// reaches. Without a tracker both are empty.
fn tracker_ids() -> (Vec<String>, Vec<usize>) {
    let projects = crate::known_projects();
    let Ok(layout) = vissue_core::Layout::resolve(None, None) else {
        return (projects, Vec::new());
    };
    let layouts: Vec<vissue_core::Layout> = vissue_core::Router::load(layout.clone())
        .map(|router| router.unique_layouts().into_iter().cloned().collect())
        .unwrap_or_else(|_| vec![layout]);
    let lengths = layouts
        .iter()
        .filter_map(|layout| vissue_core::config::VissueConfig::load(layout).ok())
        .map(|config| config.issues.id_length)
        .collect();
    (projects, lengths)
}

fn load_tickets() -> Result<Vec<TicketRow>> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    layout.require_tracker()?;
    let router = vissue_core::Router::load(layout.clone())?;
    let layouts: Vec<vissue_core::Layout> = router.unique_layouts().into_iter().cloned().collect();
    let mut rows = Vec::new();
    for layout in if layouts.is_empty() {
        vec![layout]
    } else {
        layouts
    } {
        rows.extend(vissue_core::catalog::load_recs(&layout)?);
    }
    rows.sort_by(|left, right| left.heading.id.cmp(&right.heading.id));
    rows.dedup_by(|left, right| left.heading.id == right.heading.id);
    Ok(rows.into_iter().map(ticket_of).collect())
}

fn ticket_of(rec: IssueRec) -> TicketRow {
    let notes = rec
        .heading
        .logbook
        .iter()
        .filter(|entry| !entry.is_bookkeeping())
        .filter_map(crate::logbook_text)
        .collect();
    TicketRow {
        id: rec.heading.id.clone(),
        project: rec.project,
        state: rec.heading.state.clone(),
        title: rec.heading.title.clone(),
        body: rec.heading.body.clone(),
        notes,
        parent: rec.heading.parent().map(str::to_string),
        blocked_by: rec.heading.blocked_by(),
        public: rec
            .heading
            .tags()
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case(PUBLIC_TAG)),
    }
}

fn workflow_dir(options: &Options) -> Result<Option<PathBuf>> {
    if let Some(dir) = &options.workflows {
        if !dir.is_dir() {
            bail!("handbook: the --workflows directory was not found");
        }
        return Ok(Some(dir.clone()));
    }
    let default = PathBuf::from(".ljos/workflows");
    Ok(default.is_dir().then_some(default))
}

fn workflow_paths(options: &Options) -> Result<Vec<PathBuf>> {
    let Some(dir) = workflow_dir(options)? else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("org"))
        .collect();
    paths.sort();
    Ok(paths)
}

fn load_workflows(options: &Options, skip: Option<&Path>) -> Result<Vec<WorkflowRow>> {
    let mut rows = Vec::new();
    for path in workflow_paths(options)? {
        if skip.is_some_and(|dest| same_file(&path, dest)) {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| "handbook: cannot read a workflow file")?;
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("workflow")
            .to_string();
        let title = header_value(&text, "title").unwrap_or_else(|| name.clone());
        rows.push(WorkflowRow {
            name,
            title,
            public: workflow_public(&text),
        });
    }
    Ok(rows)
}

/// The one line that marks a workflow file. It is compared byte for byte, so
/// another case, spacing, value, tab, trailing space or a CR from CRLF line
/// endings does not mark the file.
const WORKFLOW_MARK: &str = "#+public: yes";

/// A workflow file is public when a line of its header is exactly
/// [`WORKFLOW_MARK`].
fn workflow_public(text: &str) -> bool {
    header_lines(text).any(|line| line == WORKFLOW_MARK)
}

/// The first `#+NAME: value` line of the file header, with the value
/// trimmed. The key is read without regard to case.
fn header_value(text: &str, name: &str) -> Option<String> {
    header_lines(text).find_map(|line| {
        let (key, value) = line.strip_prefix("#+")?.split_once(':')?;
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

/// The `#+KEY: value` lines of the file header, as written. The header is
/// the run of blank lines, comments and `#+KEY: value` lines at the top of
/// the file. A line that starts a block, a heading or a paragraph ends it,
/// so a keyword in a block or in the body is not read.
fn header_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| (line, line.trim_end_matches('\r')))
        .take_while(|(_, bare)| {
            if bare.trim().is_empty() || *bare == "#" || bare.starts_with("# ") {
                return true;
            }
            let Some((key, _)) = bare
                .strip_prefix("#+")
                .and_then(|rest| rest.split_once(':'))
            else {
                return false;
            };
            !(key.is_empty()
                || key.contains(char::is_whitespace)
                || key.to_ascii_lowercase().starts_with("begin"))
        })
        .map(|(line, _)| line)
        .filter(|line| line.starts_with("#+"))
}

fn same_file(left: &Path, right: &Path) -> bool {
    absolute(left) == absolute(right)
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// Render rows the caller already loaded.
///
/// # Errors
///
/// A requested ticket is missing, outside the project or not marked public,
/// `--date` is not a calendar day, a marked item is not public-safe, or
/// nothing marked matches.
pub fn render(
    options: &Options,
    memories: &[MemoryRow],
    policies: &[PolicyRow],
    tickets: &[TicketRow],
    workflows: &[WorkflowRow],
) -> Result<String> {
    let mut refused = Refusals::default();
    let screen = screen_of(options);
    let ids = id_screen(options, tickets);
    let titled = Options {
        title: options.title.as_deref().map(|title| ids.redact(title)),
        ..options.clone()
    };
    let options = &titled;
    let memories = &redacted_memories(&ids, memories);
    let policies = &redacted_policies(&ids, policies);
    let tickets = &redacted_tickets(&ids, tickets);
    let workflows = &redacted_workflows(&ids, workflows);
    if let Some(date) = &options.date {
        if !date_shaped(date) {
            bail!("handbook: --date is not YYYY-MM-DD");
        }
        if !real_day(date) {
            bail!("handbook: --date: that day does not exist");
        }
        if public_text::refuses(date, &screen) {
            bail!("handbook: --date is not public");
        }
    }
    let lessons = select_lessons(memories, options, &screen, &mut refused);
    let policies = select_policies(policies, options, &screen, &mut refused);
    let selected = select_tickets(tickets, options, &screen, &mut refused)?;
    let workflows = select_workflows(workflows, options, &screen, &mut refused);
    let mut out = String::new();
    write_preamble(&mut out, options, &screen, &mut refused);
    write_lessons(&mut out, &lessons, options.ids, &screen, &mut refused);
    write_policies(&mut out, &policies, options.ids, &screen, &mut refused);
    if !options.trees.is_empty() && !selected.is_empty() {
        write_tickets(
            &mut out,
            tickets,
            &selected,
            &options.project,
            options.ids,
            &screen,
            &mut refused,
        );
    }
    write_workflows(&mut out, &workflows);
    if !refused.is_empty() {
        return Err(refused.error());
    }
    if lessons.is_empty() && policies.is_empty() && selected.is_empty() && workflows.is_empty() {
        bail!("handbook: --project has no items marked public that match");
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Every ticket in the tracker is known, so one the page does not publish
/// is named nowhere, in a ticket or out of one.
fn id_screen(options: &Options, tickets: &[TicketRow]) -> IdScreen {
    let hidden = tickets
        .iter()
        .filter(|row| !row.public || !same_project(&row.project, &options.project))
        .map(|row| row.id.clone());
    let allowed = tickets
        .iter()
        .filter(|row| row.public && same_project(&row.project, &options.project))
        .map(|row| row.id.clone());
    let projects = options
        .projects
        .iter()
        .cloned()
        .chain(tickets.iter().map(|row| row.project.clone()))
        .chain([options.project.clone()]);
    IdScreen::new(
        hidden,
        allowed,
        projects,
        options.id_lengths.iter().copied(),
    )
}

fn redacted_memories(ids: &IdScreen, rows: &[MemoryRow]) -> Vec<MemoryRow> {
    rows.iter()
        .map(|row| MemoryRow {
            text: ids.redact(&row.text),
            ..row.clone()
        })
        .collect()
}

fn redacted_policies(ids: &IdScreen, rows: &[PolicyRow]) -> Vec<PolicyRow> {
    rows.iter()
        .map(|row| PolicyRow {
            text: ids.redact(&row.text),
            pattern: row.pattern.as_deref().map(|pattern| ids.redact(pattern)),
            verdict: row.verdict.as_deref().map(|verdict| ids.redact(verdict)),
            ..row.clone()
        })
        .collect()
}

fn redacted_tickets(ids: &IdScreen, rows: &[TicketRow]) -> Vec<TicketRow> {
    rows.iter()
        .map(|row| TicketRow {
            title: ids.redact(&row.title),
            body: ids.redact(&row.body),
            notes: row.notes.iter().map(|note| ids.redact(note)).collect(),
            ..row.clone()
        })
        .collect()
}

fn redacted_workflows(ids: &IdScreen, rows: &[WorkflowRow]) -> Vec<WorkflowRow> {
    rows.iter()
        .map(|row| WorkflowRow {
            name: ids.redact(&row.name),
            title: ids.redact(&row.title),
            ..row.clone()
        })
        .collect()
}

fn date_shaped(date: &str) -> bool {
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

/// A day on the calendar. There is no year 0.
fn real_day(date: &str) -> bool {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .is_ok_and(|day| chrono::Datelike::year(&day) >= 1)
}

fn write_preamble(out: &mut String, options: &Options, screen: &Screen, refused: &mut Refusals) {
    let title = options
        .title
        .as_deref()
        .map_or_else(
            || Some(DEFAULT_TITLE.to_string()),
            |text| publish(text, screen, refused, "--title"),
        )
        .unwrap_or_else(|| DEFAULT_TITLE.to_string());
    out.push_str(&format!("#+title: {}\n", inline_escape(&title)));
    out.push_str("#+options: toc:2 num:nil\n");
    out.push_str("#+TODO: TODO | DONE CANCELLED\n");
    if let Some(date) = &options.date {
        out.push_str(&format!("#+date: {}\n", inline_escape(date)));
    }
}

fn select_lessons(
    memories: &[MemoryRow],
    options: &Options,
    screen: &Screen,
    refused: &mut Refusals,
) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for memory in memories {
        if memory.kind != "lesson" || !(memory.kept || options.include_unreviewed) {
            continue;
        }
        if !is_public(&memory.entities) {
            continue;
        }
        if !scoped(&memory.entities, &options.project, &options.tags) {
            continue;
        }
        let source = named("lesson", &memory.id, screen);
        let Some(text) = publish(&memory.text, screen, refused, &source) else {
            continue;
        };
        if query_match(options.query.as_deref(), &text) {
            rows.push((text, memory.id.clone()));
        }
    }
    dedup(&mut rows);
    rows
}

fn select_policies(
    policies: &[PolicyRow],
    options: &Options,
    screen: &Screen,
    refused: &mut Refusals,
) -> Vec<(String, String, String)> {
    let mut rows = Vec::new();
    for policy in policies {
        if !matches!(policy.kind.as_str(), "preference" | "rule") {
            continue;
        }
        if !(policy.kept || options.include_unreviewed) {
            continue;
        }
        if !is_public(&policy.entities) {
            continue;
        }
        if !scoped(&policy.entities, &options.project, &options.tags) {
            continue;
        }
        let raw = policy_text(policy);
        let source = named(&policy.kind, &policy.id, screen);
        let Some(text) = publish(&raw, screen, refused, &source) else {
            continue;
        };
        if query_match(options.query.as_deref(), &text) {
            rows.push((policy.kind.clone(), text, policy.id.clone()));
        }
    }
    rows.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    let mut seen = BTreeSet::new();
    rows.retain(|(_, text, _)| seen.insert(text.clone()));
    rows
}

fn policy_text(policy: &PolicyRow) -> String {
    match (&policy.verdict, &policy.pattern) {
        (Some(verdict), Some(pattern)) => format!("{verdict}: {pattern} - {}", policy.text),
        _ => policy.text.clone(),
    }
}

fn select_tickets(
    tickets: &[TicketRow],
    options: &Options,
    screen: &Screen,
    refused: &mut Refusals,
) -> Result<BTreeMap<String, KeptTicket>> {
    if options.trees.is_empty() {
        return Ok(BTreeMap::new());
    }
    let by_id: BTreeMap<String, TicketRow> = tickets
        .iter()
        .cloned()
        .map(|ticket| (ticket.id.clone(), ticket))
        .collect();
    for root in &options.trees {
        match by_id.get(root) {
            None => bail!("handbook: a --tree ticket was not found"),
            Some(ticket) if !same_project(&ticket.project, &options.project) => {
                bail!("handbook: a --tree ticket is not in the --project");
            }
            Some(ticket) if !ticket.public => {
                bail!("handbook: a --tree ticket is not marked public");
            }
            Some(_) => {}
        }
    }
    let keep = ticket_closure(&by_id, &options.trees, &options.project);
    let mut selected = BTreeMap::new();
    for id in keep {
        let Some(row) = by_id.get(&id) else {
            continue;
        };
        let Some(title) = publish(
            &row.title,
            screen,
            refused,
            &named("ticket title", &row.id, screen),
        ) else {
            continue;
        };
        let mut summary = Vec::new();
        let mut parts = Vec::new();
        if !row.body.trim().is_empty() {
            parts.push(("ticket body", row.body.clone()));
        }
        parts.extend(row.notes.iter().map(|note| ("ticket note", note.clone())));
        for (kind, part) in parts {
            if let Some(text) = publish(&part, screen, refused, &named(kind, &row.id, screen)) {
                summary.push(text);
            }
        }
        let published = format!("{title}\n{}", summary.join("\n"));
        if !query_match(options.query.as_deref(), &published) {
            continue;
        }
        selected.insert(
            id,
            KeptTicket {
                row: row.clone(),
                title,
                summary,
            },
        );
    }
    Ok(selected)
}

fn select_workflows(
    workflows: &[WorkflowRow],
    options: &Options,
    screen: &Screen,
    refused: &mut Refusals,
) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for (index, workflow) in workflows.iter().enumerate() {
        if !workflow.public {
            continue;
        }
        let place = index + 1;
        let Some(name) = publish(
            &workflow.name,
            screen,
            refused,
            &format!("workflow {place} name"),
        ) else {
            continue;
        };
        let Some(title) = publish(
            &workflow.title,
            screen,
            refused,
            &format!("workflow {place} title"),
        ) else {
            continue;
        };
        let published = format!("{name}\n{title}");
        if query_match(options.query.as_deref(), &published) {
            rows.push((name, title));
        }
    }
    rows.sort();
    rows.dedup();
    rows
}

fn write_lessons(
    out: &mut String,
    lessons: &[(String, String)],
    ids: bool,
    screen: &Screen,
    refused: &mut Refusals,
) {
    if lessons.is_empty() {
        return;
    }
    out.push_str("\n* Project knowledge\n");
    out.push_str("** Lessons\n");
    for (text, id) in lessons {
        let source = named("lesson", id, screen);
        write_item(
            out,
            text,
            shown_id(ids, id, screen),
            screen,
            refused,
            &source,
        );
    }
}

fn write_policies(
    out: &mut String,
    policies: &[(String, String, String)],
    ids: bool,
    screen: &Screen,
    refused: &mut Refusals,
) {
    if policies.is_empty() {
        return;
    }
    out.push_str("\n* Policies\n");
    for kind in ["preference", "rule"] {
        let rows: Vec<_> = policies
            .iter()
            .filter(|(row_kind, _, _)| row_kind == kind)
            .collect();
        if rows.is_empty() {
            continue;
        }
        let heading = match kind {
            "preference" => "Standing preferences",
            _ => "Rules",
        };
        out.push_str(&format!("** {heading}\n"));
        for (_, text, id) in rows {
            let source = named(kind, id, screen);
            write_item(
                out,
                text,
                shown_id(ids, id, screen),
                screen,
                refused,
                &source,
            );
        }
    }
}

fn shown_id<'a>(ids: bool, id: &'a str, screen: &Screen) -> Option<&'a str> {
    (ids && public_text::classify_id(id, screen).is_none() && !org_markup(id)).then_some(id)
}

fn write_tickets(
    out: &mut String,
    tickets: &[TicketRow],
    selected: &BTreeMap<String, KeptTicket>,
    project: &str,
    ids: bool,
    screen: &Screen,
    refused: &mut Refusals,
) {
    let by_id: BTreeMap<_, _> = tickets
        .iter()
        .map(|ticket| (ticket.id.as_str(), ticket))
        .collect();
    out.push_str("\n* Release tickets\n");
    for id in outline_order(selected) {
        let Some(ticket) = selected.get(&id) else {
            continue;
        };
        let source = named("ticket", &id, screen);
        let depth = ticket_depth(&id, selected);
        let stars = "*".repeat(depth + 2);
        let state = org_state(&ticket.row.state);
        let id_text = shown_id(true, &ticket.row.id, screen)
            .map(|id| format!("{} ", inline_escape(id)))
            .unwrap_or_default();
        let heading = format!(
            "{stars} {state} {id_text}{}\n",
            heading_escape(
                &ticket.title,
                shown_id(true, &ticket.row.id, screen).is_none()
            )
        );
        push_block(out, &heading, screen, refused, &format!("{source} title"));
        if shown_id(ids, &ticket.row.id, screen).is_some() {
            let drawer = format!(
                ":PROPERTIES:\n:ID: {}\n:END:\n",
                inline_escape(&ticket.row.id)
            );
            push_block(out, &drawer, screen, refused, &format!("{source} id"));
        }
        for line in &ticket.summary {
            write_item(out, line, None, screen, refused, &source);
        }
        let mut deps: Vec<&str> = ticket
            .row
            .blocked_by
            .iter()
            .filter(|blocker| selected.contains_key(blocker.as_str()))
            .filter_map(|blocker| shown_id(true, blocker, screen))
            .collect();
        deps.sort_unstable();
        if !deps.is_empty() {
            let shown: Vec<String> = deps.iter().map(|id| inline_escape(id)).collect();
            let line = format!("- Depends on: {}\n", shown.join(", "));
            push_block(out, &line, screen, refused, &format!("{source} depends"));
        }
        let external = ticket.row.blocked_by.iter().any(|blocker| {
            by_id
                .get(blocker.as_str())
                .is_none_or(|other| !other.public || !same_project(&other.project, project))
        });
        if external {
            out.push_str("- external dependency\n");
        }
    }
    let edges = blocker_edges(selected);
    out.push_str("\n** Dependency graph\n#+begin_src dot\n");
    out.push_str("digraph release {\n");
    for id in selected.keys() {
        if shown_id(true, id, screen).is_none() {
            continue;
        }
        let line = format!("  {};\n", dot_id(id));
        push_block(out, &line, screen, refused, "dependency graph");
    }
    for (from, to) in &edges {
        if shown_id(true, from, screen).is_none() || shown_id(true, to, screen).is_none() {
            continue;
        }
        let line = format!(
            "  {} -> {} [label=\"depends\"];\n",
            dot_id(from),
            dot_id(to)
        );
        push_block(out, &line, screen, refused, "dependency graph");
    }
    out.push_str("}\n#+end_src\n");
    let nodes = selected.keys().cloned().collect();
    let (order, cycles) = release_order(&nodes, &edges);
    out.push_str("\n** Release order\n");
    for id in &order {
        if shown_id(true, id, screen).is_none() {
            continue;
        }
        let line = format!("- {}\n", inline_escape(id));
        push_block(out, &line, screen, refused, "release order");
    }
    for cycle in &cycles {
        let shown: Vec<String> = cycle
            .iter()
            .filter(|id| shown_id(true, id, screen).is_some())
            .map(|id| inline_escape(id))
            .collect();
        if !shown.is_empty() {
            let line = format!("- Cycle: {}\n", shown.join(", "));
            push_block(out, &line, screen, refused, "release order");
        }
    }
}

fn write_workflows(out: &mut String, workflows: &[(String, String)]) {
    if workflows.is_empty() {
        return;
    }
    out.push_str("\n* Workflows\n");
    for (name, title) in workflows {
        let link = link_escape(name);
        out.push_str(&format!("** {}\n", heading_escape(title, true)));
        out.push_str(&format!("- [[file:{link}.org][{link}.org]]\n"));
    }
}

fn write_item(
    out: &mut String,
    text: &str,
    id: Option<&str>,
    screen: &Screen,
    refused: &mut Refusals,
    source: &str,
) {
    let mut lines = text.split('\n');
    let first = lines.next().unwrap_or("");
    let mut block = String::new();
    if let Some(id) = id {
        block.push_str("*** ");
        block.push_str(&heading_escape(first, true));
        block.push('\n');
        block.push_str(":PROPERTIES:\n");
        block.push_str(&format!(":ID: {}\n", inline_escape(id)));
        block.push_str(":END:\n");
    } else {
        block.push_str("- ");
        block.push_str(&inline_escape(first));
        block.push('\n');
    }
    for line in lines {
        block.push_str("  ");
        block.push_str(&continuation_escape(line));
        block.push('\n');
    }
    push_block(out, &block, screen, refused, source);
}

/// `--out` names the process's standard output: `-`, or a path that is the
/// same stream.
fn to_stdout(out: &Path) -> bool {
    matches!(
        out.to_str(),
        Some("-" | "/dev/stdout" | "/dev/fd/1" | "/proc/self/fd/1")
    )
}

/// A path as a diagnostic names it: absolute, with the parent resolved.
fn shown(path: &Path) -> String {
    let full = absolute(path);
    match (full.parent().map(std::fs::canonicalize), full.file_name()) {
        (Some(Ok(parent)), Some(name)) => parent.join(name).display().to_string(),
        _ => full.display().to_string(),
    }
}

fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        // SAFETY: `access` reads the NUL-terminated path and nothing else.
        .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0)
}

/// Refuse a target the export should not replace: a symbolic link, whether
/// it resolves or dangles, a file that is not a regular file, and a file
/// the person made read-only. A path that does not exist is fine.
fn guard_target(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    let name = shown(path);
    if meta.file_type().is_symlink() {
        bail!("handbook: {name} is a symbolic link, so name the file it points to");
    }
    if !meta.is_file() {
        bail!("handbook: {name} is not a regular file");
    }
    if meta.permissions().mode() & 0o222 == 0 || !writable(path) {
        bail!("handbook: {name} is read-only, so make it writable or name another file");
    }
    Ok(())
}

fn write_output(out: &Path, text: &str) -> Result<()> {
    if to_stdout(out) {
        crate::emit_raw!("{text}");
        return Ok(());
    }
    let path = file_for(out);
    guard_target(&path)?;
    let name = shown(&path);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|err| anyhow::anyhow!("handbook: cannot create {}: {err}", shown(parent)))?;
        if !writable(parent) {
            bail!(
                "handbook: cannot write {name}: the directory {} is not writable",
                shown(parent)
            );
        }
    }
    replace_file(&path, text.as_bytes())
        .map_err(|err| anyhow::anyhow!("handbook: cannot write {name}: {err}"))?;
    Ok(())
}

/// Write `bytes` to a temp file beside `path`, sync it and rename it over
/// `path`. A write that fails or is cut short leaves the old file whole.
fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    replace_with(path, |file| file.write_all(bytes))
}

fn replace_with(
    path: &Path,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let dir = std::fs::canonicalize(parent)?;
    let target = dir.join(name);
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = dir.join(tmp_name);
    #[expect(
        clippy::disallowed_methods,
        reason = "writes and renames a file that is not a git hook"
    )]
    let attempt = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        fill(&mut file)?;
        file.sync_all()?;
        drop(file);
        if let Ok(meta) = std::fs::symlink_metadata(&target) {
            if meta.is_file() {
                use std::os::unix::fs::PermissionsExt;
                crate::change_mode(&tmp, meta.permissions().mode() & 0o666)?;
            }
        }
        std::fs::rename(&tmp, &target)
    })();
    if let Err(err) = attempt {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    std::fs::File::open(&dir).and_then(|handle| handle.sync_all())
}

fn atom_entities(atom: &Value) -> Vec<String> {
    atom["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn scoped(entities: &[String], project: &str, tags: &[String]) -> bool {
    project_ok(entities, project) && tags_ok(entities, tags)
}

fn project_ok(entities: &[String], project: &str) -> bool {
    let projects: Vec<&str> = entities
        .iter()
        .filter_map(|entity| entity.strip_prefix("project:"))
        .collect();
    projects
        .iter()
        .any(|name| name.eq_ignore_ascii_case(project))
}

fn tags_ok(entities: &[String], tags: &[String]) -> bool {
    tags.iter().all(|tag| {
        entities.iter().any(|entity| {
            entity
                .strip_prefix("tag:")
                .is_some_and(|name| name.eq_ignore_ascii_case(tag))
        })
    })
}

fn query_match(query: Option<&str>, text: &str) -> bool {
    query.is_none_or(|query| text.to_lowercase().contains(&query.to_lowercase()))
}

fn same_project(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn publish(text: &str, screen: &Screen, refused: &mut Refusals, source: &str) -> Option<String> {
    let mut kept = Vec::new();
    let mut stripped = false;
    for line in text.lines() {
        if bookkeeping_line(line) {
            stripped = true;
            continue;
        }
        kept.push(line);
    }
    if kept.is_empty() {
        return None;
    }
    let body = if stripped {
        kept.join("\n")
    } else {
        text.trim_end_matches(['\n', '\r']).to_string()
    };
    if body.trim().is_empty() {
        return None;
    }
    let lines: Vec<public_text::Line<'_>> =
        body.lines().map(public_text::Line::structure).collect();
    let class = public_text::scan(&lines, screen)
        .into_iter()
        .flatten()
        .next()
        .map(public_text::Class::name)
        .or_else(|| org_markup(&body).then_some("org-markup"));
    if let Some(class) = class {
        refused.refuse(source, class);
        None
    } else {
        Some(body)
    }
}

fn bookkeeping_line(line: &str) -> bool {
    let bare = line.trim().trim_start_matches(['-', '+', '*']).trim();
    let lower = bare.to_ascii_lowercase();
    state_change(&lower) || seat_act(&lower) || clock_line(&lower) || timestamp_line(bare)
}

/// `State "DONE" from "TODO"`, with an optional org time at the end.
/// A sentence that only begins with "State" is author text.
fn state_change(lower: &str) -> bool {
    let Some(rest) = lower.strip_prefix("state ") else {
        return false;
    };
    let Some(rest) = quoted_piece(rest.trim_start()) else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix("from ") else {
        return false;
    };
    let Some(rest) = quoted_piece(rest.trim_start()) else {
        return false;
    };
    let tail = rest.trim();
    tail.is_empty() || org_time(tail)
}

fn quoted_piece(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('"')?;
    let end = rest.find('"')?;
    if end == 0 {
        return None;
    }
    Some(&rest[end + 1..])
}

/// `Noted by saddle` or `Finished by alice [YYYY-MM-DD ...]`.
/// The seat is one word. More words are a sentence.
fn seat_act(lower: &str) -> bool {
    for prefix in ["note added by ", "noted by ", "claimed by ", "finished by "] {
        let Some(rest) = lower.strip_prefix(prefix) else {
            continue;
        };
        let rest = rest.trim();
        let Some(seat) = rest.split_whitespace().next() else {
            return false;
        };
        if seat.is_empty()
            || !seat
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        {
            return false;
        }
        let tail = rest[seat.len()..].trim();
        return tail.is_empty() || org_time(tail);
    }
    false
}

fn clock_line(lower: &str) -> bool {
    let Some(rest) = lower.strip_prefix("clock:") else {
        return false;
    };
    org_time(rest.trim_start())
}

fn org_time(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('[') else {
        return false;
    };
    let bytes = rest.as_bytes();
    bytes.len() >= 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes.last() == Some(&b']')
        && !bytes[..bytes.len() - 1].contains(&b']')
}

fn timestamp_line(bare: &str) -> bool {
    org_time(bare)
}

fn dedup(rows: &mut Vec<(String, String)>) {
    rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut seen = BTreeSet::new();
    rows.retain(|(text, _)| seen.insert(text.clone()));
}

fn ticket_closure(
    tickets: &BTreeMap<String, TicketRow>,
    roots: &[String],
    project: &str,
) -> BTreeSet<String> {
    let in_project = |id: &String| {
        tickets
            .get(id)
            .is_some_and(|row| row.public && same_project(&row.project, project))
    };
    let mut keep = BTreeSet::new();
    let mut queue: VecDeque<String> = roots.iter().filter(|id| in_project(id)).cloned().collect();
    while let Some(id) = queue.pop_front() {
        if !keep.insert(id.clone()) {
            continue;
        }
        if let Some(ticket) = tickets.get(&id) {
            queue.extend(
                ticket
                    .blocked_by
                    .iter()
                    .filter(|id| in_project(id))
                    .cloned(),
            );
        }
    }
    keep
}

fn blocker_edges(tickets: &BTreeMap<String, KeptTicket>) -> BTreeSet<(String, String)> {
    let mut edges = BTreeSet::new();
    for ticket in tickets.values() {
        for blocker in &ticket.row.blocked_by {
            if tickets.contains_key(blocker) {
                edges.insert((blocker.clone(), ticket.row.id.clone()));
            }
        }
    }
    edges
}

fn ticket_depth(id: &str, tickets: &BTreeMap<String, KeptTicket>) -> usize {
    let mut depth = 0;
    let mut current = id;
    let mut seen = BTreeSet::new();
    while seen.insert(current.to_string()) {
        let Some(parent) = tickets
            .get(current)
            .and_then(|ticket| ticket.row.parent.as_deref())
            .filter(|parent| tickets.contains_key(*parent))
        else {
            break;
        };
        depth += 1;
        current = parent;
    }
    depth
}

fn outline_order(tickets: &BTreeMap<String, KeptTicket>) -> Vec<String> {
    fn visit(
        id: &str,
        children: &BTreeMap<String, Vec<String>>,
        seen: &mut BTreeSet<String>,
        out: &mut Vec<String>,
    ) {
        if !seen.insert(id.to_string()) {
            return;
        }
        out.push(id.to_string());
        for child in children.get(id).into_iter().flatten() {
            visit(child, children, seen, out);
        }
    }

    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ticket in tickets.values() {
        if let Some(parent) = ticket
            .row
            .parent
            .as_ref()
            .filter(|parent| tickets.contains_key(*parent))
        {
            children
                .entry(parent.clone())
                .or_default()
                .push(ticket.row.id.clone());
        }
    }
    for rows in children.values_mut() {
        rows.sort();
    }
    let mut roots: Vec<String> = tickets
        .values()
        .filter(|ticket| {
            ticket
                .row
                .parent
                .as_ref()
                .is_none_or(|parent| !tickets.contains_key(parent))
        })
        .map(|ticket| ticket.row.id.clone())
        .collect();
    roots.sort();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for root in roots {
        visit(&root, &children, &mut seen, &mut out);
    }
    for id in tickets.keys() {
        visit(id, &children, &mut seen, &mut out);
    }
    out
}

/// Blocker order, and the cycles: one list of ids for each strongly
/// connected component, in id order.
///
/// A cycle is a strongly connected component of more than one id, or an id
/// that blocks itself. An id that only waits on a cycle is not in one: it
/// stays in the order, after the ids it can wait for.
#[must_use]
pub fn release_order(
    nodes: &BTreeSet<String>,
    edges: &BTreeSet<(String, String)>,
) -> (Vec<String>, Vec<Vec<String>>) {
    let ids: Vec<&String> = nodes.iter().collect();
    let index: BTreeMap<&String, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    let mut selfish = vec![false; ids.len()];
    for (from, to) in edges {
        if let (Some(&from), Some(&to)) = (index.get(from), index.get(to)) {
            if from == to {
                selfish[from] = true;
            }
            next[from].push(to);
        }
    }
    let (component, count) = strong_components(&next);
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (node, &comp) in component.iter().enumerate() {
        members[comp].push(node);
    }
    let size: Vec<usize> = members.iter().map(Vec::len).collect();
    let first: Vec<usize> = members.iter().map(|nodes| nodes[0]).collect();
    let cyclic: Vec<bool> = (0..ids.len())
        .map(|node| size[component[node]] > 1 || selfish[node])
        .collect();
    let mut degree = vec![0usize; count];
    let mut after: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); count];
    for (node, targets) in next.iter().enumerate() {
        for &target in targets {
            let (from, to) = (component[node], component[target]);
            if from != to && after[from].insert(to) {
                degree[to] += 1;
            }
        }
    }
    let mut ready: BTreeSet<(usize, usize)> = (0..count)
        .filter(|comp| degree[*comp] == 0)
        .map(|comp| (first[comp], comp))
        .collect();
    let mut order = Vec::new();
    while let Some((_, comp)) = ready.pop_first() {
        for &node in &members[comp] {
            if !cyclic[node] {
                order.push(ids[node].clone());
            }
        }
        for &to in &after[comp] {
            degree[to] -= 1;
            if degree[to] == 0 {
                ready.insert((first[to], to));
            }
        }
    }
    let mut cycles: Vec<Vec<String>> = members
        .iter()
        .filter(|nodes| nodes.iter().any(|node| cyclic[*node]))
        .map(|nodes| nodes.iter().map(|node| ids[*node].clone()).collect())
        .collect();
    cycles.sort();
    (order, cycles)
}

/// Tarjan's strongly connected components, without recursion. Returns the
/// component of each node and the number of components.
fn strong_components(next: &[Vec<usize>]) -> (Vec<usize>, usize) {
    const NONE: usize = usize::MAX;
    let n = next.len();
    let mut index = vec![NONE; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut component = vec![NONE; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut counter = 0;
    let mut found = 0;
    for root in 0..n {
        if index[root] != NONE {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(node, edge)) = work.last() {
            if edge == 0 && index[node] == NONE {
                index[node] = counter;
                low[node] = counter;
                counter += 1;
                stack.push(node);
                on_stack[node] = true;
            }
            if let Some(&target) = next[node].get(edge) {
                work.last_mut().expect("a frame").1 += 1;
                if index[target] == NONE {
                    work.push((target, 0));
                } else if on_stack[target] {
                    low[node] = low[node].min(index[target]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == index[node] {
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    component[member] = found;
                    if member == node {
                        break;
                    }
                }
                found += 1;
            }
        }
    }
    (component, found)
}

/// Whether `text` contains Org inline syntax: a link, a macro, a footnote
/// reference, a citation, a target, an export snippet, or an inline source
/// or babel call. The export keeps text as written, so a line that would
/// parse as one of these is left out instead of rewritten. Only a detection
/// copy is normalized.
fn org_markup(text: &str) -> bool {
    let copy = public_text::detection_copy(text).to_lowercase();
    copy.contains("[[")
        || copy.contains("{{{")
        || copy.contains("[fn:")
        || copy.contains("[cite:")
        || copy.contains("[cite/")
        || copy.lines().any(|line| {
            target(line)
                || pair_on_line(line, "@@", "@@")
                || inline_call(line, "src_", ['{', '['])
                || inline_call(line, "call_", ['(', '['])
        })
}

fn pair_on_line(line: &str, open: &str, close: &str) -> bool {
    line.split_once(open)
        .is_some_and(|(_, rest)| rest.contains(close))
}

/// `<<name>>`: Org needs a name with no `<` or `>` and no space at either
/// end, so a spaced shift such as `a << b >> c` is not a target.
fn target(line: &str) -> bool {
    line.char_indices().any(|(at, _)| {
        let Some(rest) = line[at..].strip_prefix("<<") else {
            return false;
        };
        let end = rest.find(['<', '>']).unwrap_or(rest.len());
        let name = &rest[..end];
        rest[end..].starts_with(">>")
            && name.chars().next().is_some_and(|ch| !ch.is_whitespace())
            && name
                .chars()
                .next_back()
                .is_some_and(|ch| !ch.is_whitespace())
    })
}

/// `src_name{...}` or `call_name(...)`: the word, a name with no space, and
/// then one of `marks`. `src_dir` alone is a name, not a block.
fn inline_call(line: &str, word: &str, marks: [char; 2]) -> bool {
    line.match_indices(word).any(|(at, _)| {
        let starts = line[..at]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_alphanumeric());
        let rest = &line[at + word.len()..];
        let name = rest
            .find(|ch: char| ch.is_whitespace() || marks.contains(&ch))
            .unwrap_or(rest.len());
        starts && name > 0 && rest[name..].starts_with(marks)
    })
}

/// One line of public text with no line break left in it.
fn flatten(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

fn inline_escape(text: &str) -> String {
    flatten(text)
}

/// Text for a heading line. A leading TODO keyword, `COMMENT` or priority
/// cookie would change what the heading means, so a backslash goes first.
fn heading_escape(text: &str, guard: bool) -> String {
    let flat = inline_escape(text);
    let probe = public_text::detection_copy(&flat);
    let probe = probe.trim_start();
    let word = probe.split(' ').next().unwrap_or("");
    let keyword = matches!(word, "TODO" | "DONE" | "CANCELLED" | "COMMENT");
    if guard && (keyword || probe.starts_with("[#")) {
        format!("\\{flat}")
    } else {
        flat
    }
}

fn link_escape(name: &str) -> String {
    inline_escape(name)
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

/// A continuation line of a list item or heading body. A line that would
/// open a drawer, a heading, a keyword or a block gets a backslash after its
/// indent, so it stays text.
fn continuation_escape(line: &str) -> String {
    let escaped = inline_escape(line);
    let rest = escaped.trim_start();
    let indent = &escaped[..escaped.len() - rest.len()];
    let probe = public_text::detection_copy(rest);
    if structural(probe.trim_start()) {
        format!("{indent}\\{rest}")
    } else {
        escaped
    }
}

fn structural(rest: &str) -> bool {
    rest.starts_with("#+")
        || rest == "#"
        || rest.starts_with("# ")
        || rest.starts_with('|')
        || rule_mark(rest)
        || heading_mark(rest)
        || drawer_mark(rest)
        || planning_mark(rest)
}

/// `SCHEDULED:`, `DEADLINE:` or `CLOSED:` at the start of a line is planning
/// info to Org, and misplaced anywhere but under a heading.
fn planning_mark(rest: &str) -> bool {
    let lower = rest.to_ascii_lowercase();
    ["scheduled:", "deadline:", "closed:"]
        .iter()
        .any(|mark| lower.starts_with(mark))
}

fn rule_mark(rest: &str) -> bool {
    let dashes = rest.trim_end();
    dashes.len() >= 5 && dashes.chars().all(|ch| ch == '-')
}

fn heading_mark(rest: &str) -> bool {
    let after = rest.trim_start_matches('*');
    after.len() < rest.len() && (after.is_empty() || after.starts_with(char::is_whitespace))
}

fn drawer_mark(rest: &str) -> bool {
    let Some(body) = rest.strip_prefix(':') else {
        return false;
    };
    let Some((name, tail)) = body.split_once(':') else {
        return false;
    };
    !name.is_empty()
        && !name.contains(char::is_whitespace)
        && (tail.is_empty() || tail.starts_with(char::is_whitespace))
}

fn dot_id(id: &str) -> String {
    let mut escaped = String::new();
    for ch in inline_escape(id).chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            _ => escaped.push(ch),
        }
    }
    format!("\"{escaped}\"")
}

fn org_state(state: &str) -> &'static str {
    if state.eq_ignore_ascii_case("DONE") {
        "DONE"
    } else if state.eq_ignore_ascii_case("CANCELLED") {
        "CANCELLED"
    } else {
        "TODO"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Options {
        Options {
            project: "Software".into(),
            tags: Vec::new(),
            trees: vec!["Software-root".into()],
            query: None,
            title: None,
            ids: false,
            include_unreviewed: false,
            workflows: None,
            date: None,
            seats: Vec::new(),
            projects: Vec::new(),
            id_lengths: Vec::new(),
        }
    }

    fn lesson(id: &str, text: &str, entities: &[&str]) -> MemoryRow {
        MemoryRow {
            id: id.into(),
            kind: "lesson".into(),
            text: text.into(),
            entities: entities
                .iter()
                .map(|entity| (*entity).to_string())
                .chain([crate::PUBLIC_ENTITY.to_string()])
                .collect(),
            kept: true,
        }
    }

    fn unmarked(mut row: MemoryRow) -> MemoryRow {
        row.entities.retain(|entity| entity != crate::PUBLIC_ENTITY);
        row
    }

    fn flow(name: &str, title: &str) -> WorkflowRow {
        WorkflowRow {
            name: name.into(),
            title: title.into(),
            public: true,
        }
    }

    fn ticket(
        id: &str,
        project: &str,
        state: &str,
        title: &str,
        parent: Option<&str>,
        blocked_by: &[&str],
    ) -> TicketRow {
        TicketRow {
            id: id.into(),
            project: project.into(),
            state: state.into(),
            title: title.into(),
            body: String::new(),
            notes: Vec::new(),
            parent: parent.map(str::to_string),
            blocked_by: blocked_by.iter().map(|id| (*id).to_string()).collect(),
            public: true,
        }
    }

    fn render_text(options: &Options, memories: &[MemoryRow], tickets: &[TicketRow]) -> String {
        render(options, memories, &[], tickets, &[]).unwrap()
    }

    #[test]
    fn parent_links_nest_and_blockers_order() {
        let memories = [lesson(
            "m1",
            "Publish the signed archive.",
            &["project:Software"],
        )];
        let tickets = [
            ticket(
                "Software-root",
                "Software",
                "TODO",
                "Cut the release",
                None,
                &["Software-a"],
            ),
            ticket(
                "Software-a",
                "Software",
                "DONE",
                "Sign the archive",
                Some("Software-root"),
                &[],
            ),
        ];
        let text = render_text(&options(), &memories, &tickets);
        assert!(!text.contains("Cycle:"));
        assert!(!text.contains("label=\"child\""));
        assert!(text.contains("\"Software-a\" -> \"Software-root\" [label=\"depends\"]"));
        let order_a = text.find("- Software-a\n").unwrap();
        let order_root = text.find("- Software-root\n").unwrap();
        assert!(order_a < order_root);
        let nested_root = text.find("** TODO Software-root").unwrap();
        let nested_child = text.find("*** DONE Software-a").unwrap();
        assert!(nested_root < nested_child);
        assert!(!text.lines().any(|line| {
            let mut bytes = line.bytes();
            bytes.next().is_some_and(|byte| byte.is_ascii_digit()) && line.contains(". ")
        }));
    }

    #[test]
    fn tree_follows_blockers_only() {
        let tickets = [
            ticket(
                "Software-root",
                "Software",
                "TODO",
                "Cut the release",
                None,
                &["Software-dep"],
            ),
            ticket(
                "Software-dep",
                "Software",
                "DONE",
                "Sign the archive",
                Some("Software-umbrella"),
                &["Software-deep"],
            ),
            ticket("Software-deep", "Software", "DONE", "Build it", None, &[]),
            ticket(
                "Software-sibling",
                "Software",
                "TODO",
                "Unrelated sibling work",
                None,
                &["Software-dep"],
            ),
            ticket(
                "Software-umbrella",
                "Software",
                "TODO",
                "Unrelated umbrella",
                None,
                &[],
            ),
            ticket(
                "Software-kid",
                "Software",
                "TODO",
                "Unrelated child",
                Some("Software-dep"),
                &[],
            ),
        ];
        let text = render_text(&options(), &[], &tickets);
        for kept in ["Software-root", "Software-dep", "Software-deep"] {
            assert!(text.contains(kept), "{kept} is missing\n{text}");
        }
        for leaked in [
            "Software-sibling",
            "Unrelated sibling work",
            "Software-umbrella",
            "Unrelated umbrella",
            "Software-kid",
            "Unrelated child",
        ] {
            assert!(!text.contains(leaked), "{leaked} leaked\n{text}");
        }
    }

    #[test]
    fn date_has_to_be_a_real_calendar_day() {
        let memories = [lesson("m1", "Publish it.", &["project:Software"])];
        let mut options = options();
        options.trees.clear();
        for bad in [
            "2026-99-99",
            "2026-13-01",
            "2026-00-10",
            "2026-02-30",
            "2025-02-29",
            "2026-04-31",
            "2026-01-00",
            "0000-01-01",
            "0000-12-31",
        ] {
            options.date = Some(bad.into());
            let err = render(&options, &memories, &[], &[], &[]).unwrap_err();
            assert_eq!(err.to_string(), "handbook: --date: that day does not exist");
        }
        for bad in ["2026-1-2", "26-01-02", "2026/01/02", ""] {
            options.date = Some(bad.into());
            let err = render(&options, &memories, &[], &[], &[]).unwrap_err();
            assert_eq!(err.to_string(), "handbook: --date is not YYYY-MM-DD");
        }
        for good in [
            "0001-01-01",
            "2026-01-02",
            "2024-02-29",
            "2000-02-29",
            "2026-12-31",
        ] {
            options.date = Some(good.into());
            let text = render(&options, &memories, &[], &[], &[]).unwrap();
            assert!(text.contains(&format!("#+date: {good}\n")), "{text}");
        }
    }

    #[test]
    fn a_blocker_cycle_is_named_and_the_export_continues() {
        let nodes = ["a", "b", "c"].map(String::from).into();
        let edges = [
            ("a".into(), "b".into()),
            ("b".into(), "a".into()),
            ("c".into(), "a".into()),
        ]
        .into();
        let (order, cycles) = release_order(&nodes, &edges);
        assert_eq!(order, vec!["c".to_string()]);
        assert_eq!(cycles, vec![vec!["a".to_string(), "b".to_string()]]);
    }

    #[test]
    fn only_a_real_cycle_is_named() {
        let nodes: BTreeSet<String> = ["a", "b", "c", "d", "e", "f", "g"].map(String::from).into();
        let edges: BTreeSet<(String, String)> = [
            ("a", "b"),
            ("b", "a"),
            ("b", "c"),
            ("d", "e"),
            ("e", "f"),
            ("f", "d"),
            ("g", "g"),
        ]
        .map(|(from, to)| (from.to_string(), to.to_string()))
        .into();
        let (order, cycles) = release_order(&nodes, &edges);
        let named = |list: &[&str]| list.iter().map(|id| (*id).to_string()).collect::<Vec<_>>();
        assert_eq!(
            cycles,
            vec![named(&["a", "b"]), named(&["d", "e", "f"]), named(&["g"])]
        );
        assert_eq!(order, vec!["c".to_string()]);
    }

    #[test]
    fn a_long_blocker_chain_does_not_overflow_the_stack() {
        let names: Vec<String> = (0..50_000).map(|n| format!("t{n:06}")).collect();
        let nodes: BTreeSet<String> = names.iter().cloned().collect();
        let edges: BTreeSet<(String, String)> = names
            .windows(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect();
        let (order, cycles) = release_order(&nodes, &edges);
        assert_eq!(order, names);
        assert!(cycles.is_empty());
    }

    #[test]
    fn a_node_downstream_of_a_cycle_is_not_called_cyclic() {
        let tickets = [
            ticket(
                "Software-c",
                "Software",
                "TODO",
                "Ship",
                None,
                &["Software-b"],
            ),
            ticket(
                "Software-b",
                "Software",
                "TODO",
                "Second half",
                None,
                &["Software-a"],
            ),
            ticket(
                "Software-a",
                "Software",
                "TODO",
                "First half",
                None,
                &["Software-b"],
            ),
        ];
        let mut options = options();
        options.trees = vec!["Software-c".into()];
        let text = render_text(&options, &[], &tickets);
        assert!(text.contains("- Cycle: Software-a, Software-b\n"), "{text}");
        assert!(!text.contains("Cycle: Software-a, Software-b, Software-c"));
        let order = text.split("** Release order\n").nth(1).unwrap();
        assert!(order.contains("- Software-c\n"), "{text}");
    }

    #[test]
    fn disjoint_cycles_are_one_line_each() {
        let mut rows = Vec::new();
        for (id, waits) in [
            ("Software-a", "Software-b"),
            ("Software-b", "Software-a"),
            ("Software-c", "Software-d"),
            ("Software-d", "Software-c"),
        ] {
            rows.push(ticket(id, "Software", "TODO", "Half", None, &[waits]));
        }
        rows.push(ticket(
            "Software-root",
            "Software",
            "TODO",
            "Ship",
            None,
            &["Software-a", "Software-c"],
        ));
        let text = render_text(&options(), &[], &rows);
        let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("- Cycle:")).collect();
        assert_eq!(
            lines,
            [
                "- Cycle: Software-a, Software-b",
                "- Cycle: Software-c, Software-d"
            ],
            "{text}"
        );
    }

    #[test]
    fn a_cross_project_blocker_is_opaque() {
        let tickets = [
            ticket(
                "Software-root",
                "Software",
                "TODO",
                "Cut the release",
                None,
                &["Other-1"],
            ),
            ticket("Other-1", "Other", "TODO", "Foreign title", None, &[]),
        ];
        let text = render_text(&options(), &[], &tickets);
        assert!(text.contains("- external dependency\n"));
        assert!(!text.contains("Other-1"));
        assert!(!text.contains("Foreign title"));
    }

    #[test]
    fn an_unmarked_ticket_is_not_exported_and_is_an_external_dependency() {
        let mut hidden = ticket(
            "Software-hidden",
            "Software",
            "TODO",
            "Hidden plan",
            None,
            &[],
        );
        hidden.public = false;
        hidden.body = "Hidden body text.".into();
        let tickets = [
            ticket(
                "Software-root",
                "Software",
                "TODO",
                "Cut the release",
                None,
                &["Software-hidden"],
            ),
            hidden.clone(),
        ];
        let text = render_text(&options(), &[], &tickets);
        assert!(text.contains("- external dependency\n"), "{text}");
        for leaked in ["Software-hidden", "Hidden plan", "Hidden body text."] {
            assert!(!text.contains(leaked), "{leaked} leaked\n{text}");
        }
        let mut options = options();
        options.trees = vec!["Software-hidden".into()];
        let err = render(&options, &[], &[], &tickets, &[])
            .unwrap_err()
            .to_string();
        assert_eq!(err, "handbook: a --tree ticket is not marked public");
    }

    #[test]
    fn the_title_comes_from_a_fixed_string_or_a_checked_flag() {
        let rows = [lesson("m1", "Publish it.", &["project:Software"])];
        let mut options = options();
        options.trees.clear();
        let text = render(&options, &rows, &[], &[], &[]).unwrap();
        assert!(text.starts_with("#+title: Project handbook\n"), "{text}");
        options.title = Some("Release notes".into());
        let text = render(&options, &rows, &[], &[], &[]).unwrap();
        assert!(text.starts_with("#+title: Release notes\n"), "{text}");
        options.title = Some("Notes for /home/ada".into());
        let err = render(&options, &rows, &[], &[], &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("refused --title"), "{err}");
        assert!(!err.contains("ada"), "{err}");
    }

    #[test]
    fn tags_narrow_the_project() {
        let memories = [
            lesson("bare", "Bare tag only.", &["release"]),
            lesson("other", "Other project.", &["project:Other", "tag:release"]),
            lesson(
                "both",
                "Both projects.",
                &["project:Software", "project:Other", "tag:release"],
            ),
            lesson(
                "kept",
                "Publish the signed archive.",
                &["project:Software", "tag:release", "tag:extra"],
            ),
            lesson("untagged", "No tag here.", &["project:Software"]),
        ];
        let mut options = options();
        options.trees.clear();
        options.tags = vec!["release".into()];
        let text = render(&options, &memories, &[], &[], &[]).unwrap();
        let body = text.split("* Project knowledge").nth(1).unwrap_or("");
        assert!(body.contains("Publish the signed archive."));
        assert!(!body.contains("Bare tag only."));
        assert!(!body.contains("Other project."));
        assert!(body.contains("Both projects."));
        assert!(!body.contains("No tag here."));
    }

    #[test]
    fn text_stays_verbatim_and_a_marked_secret_refuses_everything() {
        let memories = [
            lesson("clean", "- keep the leading mark", &["project:Software"]),
            lesson("colon", ":keep this colon", &["project:Software"]),
            lesson(
                "dup-a",
                "Publish the signed archive.",
                &["project:Software"],
            ),
            lesson(
                "dup-b",
                "Publish the signed archive.",
                &["project:Software"],
            ),
            lesson(
                "book",
                "Publish the signed archive.\nNoted by saddle\nCLOCK: [2026-01-02 Fri 09:00]",
                &["project:Software"],
            ),
            lesson(
                "author-finished",
                "Finished by Friday.",
                &["project:Software"],
            ),
            lesson(
                "author-date",
                "[2026-11-24] Rotated the release key.",
                &["project:Software"],
            ),
        ];
        let mut options = options();
        options.trees.clear();
        let text = render(&options, &memories, &[], &[], &[]).unwrap();
        assert!(text.contains("- - keep the leading mark\n"));
        assert!(text.contains("- :keep this colon\n"));
        assert!(!text.contains("Noted by"));
        assert!(!text.contains("CLOCK"));
        assert!(!text.contains("2026-01-02"));
        assert!(text.contains("Finished by Friday."), "{text}");
        assert!(
            text.contains("[2026-11-24] Rotated the release key."),
            "{text}"
        );
        let state_lesson = lesson(
            "state-line",
            "State requirements clearly.\nThen check them.",
            &["project:Software"],
        );
        let mut with_state = memories.to_vec();
        with_state.push(state_lesson);
        let text = render(&options, &with_state, &[], &[], &[]).unwrap();
        assert!(text.contains("State requirements clearly."), "{text}");
        assert!(text.contains("Then check them."), "{text}");
        let flip = lesson(
            "state-flip",
            "State \"DONE\" from \"TODO\" [2026-01-02 Fri 09:00]\nKeep the shelf list.",
            &["project:Software"],
        );
        let mut with_flip = memories.to_vec();
        with_flip.push(flip);
        let text = render(&options, &with_flip, &[], &[], &[]).unwrap();
        assert!(text.contains("Keep the shelf list."), "{text}");
        assert!(!text.contains("State \"DONE\""), "{text}");
        assert_eq!(text.matches("Publish the signed archive.").count(), 1);

        let secret = lesson(
            "secret",
            "Publish the archive and password: hunter2",
            &["project:Software"],
        );
        let mut with_secret = memories.to_vec();
        with_secret.push(secret.clone());
        let err = render(&options, &with_secret, &[], &[], &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("handbook: refused lesson secret: "), "{err}");
        assert!(err.contains("nothing was written"), "{err}");
        assert!(
            !err.contains("hunter2") && !err.contains("password"),
            "{err}"
        );

        let mut left_unmarked = memories.to_vec();
        left_unmarked.push(unmarked(secret));
        let text = render(&options, &left_unmarked, &[], &[], &[]).unwrap();
        assert!(!text.contains("hunter2"));
    }

    #[test]
    fn an_unmarked_row_is_never_exported() {
        let rows = [
            lesson("a", "Sign the archive.", &["project:Software"]),
            unmarked(lesson("b", "Hide this plain note.", &["project:Software"])),
        ];
        let mut options = options();
        options.trees.clear();
        options.ids = true;
        let text = render(&options, &rows, &[], &[], &[]).unwrap();
        assert!(text.contains("Sign the archive."));
        assert!(!text.contains("Hide this plain note."));
        assert!(!text.contains(":ID: b"));
        let only_unmarked = [rows[1].clone()];
        let err = render(&options, &only_unmarked, &[], &[], &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("no items marked public"), "{err}");
    }

    #[test]
    fn cancelled_stays_cancelled_and_ids_are_links() {
        let tickets = [ticket(
            "Software-root",
            "Software",
            "CANCELLED",
            "Cut the release",
            None,
            &[],
        )];
        let mut options = options();
        options.ids = true;
        let memories = [lesson(
            "m1",
            "Publish the signed archive.",
            &["project:Software"],
        )];
        let text = render(&options, &memories, &[], &tickets, &[]).unwrap();
        assert!(text.contains("** CANCELLED Software-root"));
        assert!(!text.contains("DONE Software-root"));
        assert!(text.contains(":ID: m1"));
        assert!(text.contains(":PROPERTIES:\n:ID: Software-root\n:END:\n"));
        assert!(!text.contains("- Publish the signed archive.\n:PROPERTIES:"));
    }

    #[test]
    fn a_pack_id_is_shown_and_a_private_id_is_not() {
        let screen = Screen::default();
        let pack = "0123456789abcdef0123456789abcdef";
        assert_eq!(shown_id(true, pack, &screen), Some(pack));
        assert_eq!(shown_id(false, pack, &screen), None);
        assert_eq!(
            shown_id(true, "Software-root", &screen),
            Some("Software-root")
        );
        for id in [
            "[[x]]",
            "/home/ada",
            "password:x",
            "10.0.0.1",
            "a@b.example",
        ] {
            assert_eq!(shown_id(true, id, &screen), None, "{id}");
        }
        assert!(gate_block(&format!(":ID: {pack}\n"), &screen).is_none());
        assert!(gate_block(&format!("- {pack}\n"), &screen).is_some());
        assert!(gate_block(":ID: /home/ada\n", &screen).is_some());
    }

    #[test]
    fn dot_ids_are_quoted() {
        assert_eq!(dot_id("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn inline_org_syntax_is_detected_and_plain_text_is_not() {
        for markup in [
            "[[https://example.com/notes][the notes]]",
            "[[https://example.com/notes][the notes]",
            "a [[b]] c",
            "{{{title}}}",
            "[fn:1] note",
            "text[fn::inline]",
            "[cite:@key]",
            "[cite/t:@key]",
            "<<target>>",
            "<<<radio>>>",
            "@@html:<b>@@",
            "src_sh{echo}",
            "src_python[:exports code]{1+1}",
            "call_name(1)",
            "call_name[:x y](1)",
            "[\u{200b}[x]]",
            "\u{ff3b}\u{ff3b}x\u{ff3d}\u{ff3d}",
            "[[x]]",
        ] {
            assert!(org_markup(markup), "kept {markup:?}");
        }
        for plain in [
            "src_dir is a folder",
            "call_me maybe",
            "see src_",
            "use [x] and [ ] boxes",
            "a << b and c >> d is shifting",
            "cat <<EOF",
            "a single @@ mark",
            "mail me @ home",
            "{{ braces }}",
            "[fn] and [cite] words",
            "https://example.com/a_b",
            "term :: description",
        ] {
            assert!(!org_markup(plain), "refused {plain:?}");
        }
    }

    #[test]
    fn an_error_does_not_repeat_a_value_that_is_not_public() {
        let memories = [lesson("a", "Sign the archive.", &["project:Software"])];
        let mut options = options();
        options.trees.clear();
        for date in [
            "PASSWORD = hunter2",
            concat!("AK", "IA", "IOSFODNN7EXAMPLE"),
            "alice@example.com",
        ] {
            options.date = Some(date.into());
            let err = render(&options, &memories, &[], &[], &[])
                .unwrap_err()
                .to_string();
            assert_eq!(err, "handbook: --date is not YYYY-MM-DD");
        }
        options.date = Some("2026-1-2".into());
        let err = render(&options, &memories, &[], &[], &[])
            .unwrap_err()
            .to_string();
        assert_eq!(err, "handbook: --date is not YYYY-MM-DD");
        options.date = None;
        options.trees = vec!["password:hunter2".into()];
        let err = render(&options, &memories, &[], &[], &[])
            .unwrap_err()
            .to_string();
        assert_eq!(err, "handbook: a --tree ticket was not found");
        options.trees = vec!["Software-gone".into()];
        let err = render(&options, &memories, &[], &[], &[])
            .unwrap_err()
            .to_string();
        assert_eq!(err, "handbook: a --tree ticket was not found");
    }

    #[test]
    fn a_marked_vocabulary_word_or_seat_name_refuses_the_export() {
        let plain = lesson("plain", "Sign the archive.", &["project:Software"]);
        let mut options = options();
        options.trees.clear();
        options.seats = vec!["saddle".into()];
        for (id, text, word) in [
            ("w1", "The packset signs the archive.", "packset"),
            ("s1", "Ask saddle later.", "saddle"),
        ] {
            let rows = [plain.clone(), lesson(id, text, &["project:Software"])];
            let err = render(&options, &rows, &[], &[], &[])
                .unwrap_err()
                .to_string();
            assert!(err.contains(&format!("refused lesson {id}")), "{err}");
            assert!(!err.contains(word), "{err}");
            let quiet = [plain.clone(), unmarked(rows[1].clone())];
            let text = render(&options, &quiet, &[], &[], &[]).unwrap();
            assert!(!text.contains(word));
        }
    }

    const IPV6: &[&str] = &[
        "2001:db8::1",
        "::1",
        "fe80::1%eth0",
        "[2001:db8::1]:8080",
        "::ffff:10.0.0.1",
        "::a",
        "dead::beef",
        "::ffff",
        "cafe::beef",
        "2001:db8::1:abcde",
    ];

    fn policy(text: &str) -> PolicyRow {
        PolicyRow {
            id: "p1".into(),
            kind: "preference".into(),
            text: text.into(),
            pattern: None,
            verdict: None,
            entities: vec!["project:Software".into(), crate::PUBLIC_ENTITY.into()],
            kept: true,
        }
    }

    fn assert_refused_on_every_surface(needle: &str, leak: &str) {
        let clean_lesson = lesson("ok", "Publish the signed archive.", &["project:Software"]);
        let clean_ticket = ticket("Software-root", "Software", "TODO", "Cut it", None, &[]);
        let mut plain = options();
        plain.trees.clear();
        let mut titled = plain.clone();
        titled.title = Some(leak.to_string());
        let cases: Vec<(&str, Options, Vec<MemoryRow>, Vec<PolicyRow>)> = vec![
            ("title", titled, vec![clean_lesson.clone()], Vec::new()),
            (
                "lesson",
                plain.clone(),
                vec![
                    clean_lesson.clone(),
                    lesson("bad", leak, &["project:Software"]),
                ],
                Vec::new(),
            ),
            (
                "policy",
                plain.clone(),
                vec![clean_lesson.clone()],
                vec![policy(leak)],
            ),
        ];
        for (surface, options, memories, policies) in cases {
            let err = render(&options, &memories, &policies, &[], &[])
                .unwrap_err()
                .to_string();
            assert!(!err.contains(needle), "{surface} echoed {needle}\n{err}");
            assert!(err.contains("nothing was written"), "{surface}\n{err}");
        }

        let mut titled = clean_ticket.clone();
        titled.title = leak.to_string();
        let mut noted = clean_ticket.clone();
        noted.notes = vec![leak.to_string()];
        let mut bodied = clean_ticket.clone();
        bodied.body = leak.to_string();
        for (surface, row) in [
            ("ticket title", titled),
            ("ticket note", noted),
            ("ticket body", bodied),
        ] {
            let keep = [clean_lesson.clone()];
            let err = render(&options(), &keep, &[], &[row], &[])
                .unwrap_err()
                .to_string();
            assert!(!err.contains(needle), "{surface} echoed {needle}\n{err}");
            assert!(err.contains("nothing was written"), "{surface}\n{err}");
        }

        for (surface, name, title) in [
            ("workflow filename", leak, "Notes"),
            ("workflow title", "notes", leak),
        ] {
            let flows = [flow(name, title)];
            let err = render(
                &plain,
                std::slice::from_ref(&clean_lesson),
                &[],
                &[],
                &flows,
            )
            .unwrap_err()
            .to_string();
            assert!(!err.contains(needle), "{surface} echoed {needle}\n{err}");
            assert!(err.contains("nothing was written"), "{surface}\n{err}");
        }
    }

    #[test]
    fn a_marked_ipv6_address_refuses_every_surface() {
        for address in IPV6 {
            assert_refused_on_every_surface(address, &format!("Reach the box at {address} today."));
        }
    }

    const HOSTILE: &str = "First line\n:PROPERTIES:\n:ID: evil\n:END:\n* Fake heading\n** TODO Another\n#+title: owned\n#+TODO: A | B\n#+begin_src sh\nrm it\n  #+BEGIN_EXAMPLE\n  :END:\n#+end_src\n-----\n| a | b |\n# comment\nLast line";

    type Mark = (&'static str, fn(&str) -> bool);

    fn structure(text: &str) -> Vec<(&'static str, usize)> {
        let marks: [Mark; 5] = [
            ("heading", |line| {
                line.starts_with('*') && line.trim_start_matches('*').starts_with(' ')
            }),
            ("keyword", |line| line.trim_start().starts_with("#+")),
            ("comment", |line| {
                let line = line.trim_start();
                line == "#" || line.starts_with("# ")
            }),
            ("table", |line| line.trim_start().starts_with('|')),
            ("drawer", |line| {
                let line = line.trim();
                line.len() > 2
                    && line.starts_with(':')
                    && line.ends_with(':')
                    && !line.contains(' ')
            }),
        ];
        marks
            .iter()
            .map(|(name, test)| (*name, text.lines().filter(|line| test(line)).count()))
            .collect()
    }

    fn render_multiline(ids: bool, field: &str) -> String {
        let memories = [lesson("m1", field, &["project:Software"])];
        let policies = [policy(field)];
        let mut ticket = ticket(
            "Software-root",
            "Software",
            "TODO",
            &field.replace('\n', " "),
            None,
            &[],
        );
        ticket.body = field.to_string();
        ticket.notes = vec![field.to_string()];
        let flows = [flow("notes", field)];
        let mut options = options();
        options.ids = ids;
        options.title = Some(field.to_string());
        render(&options, &memories, &policies, &[ticket], &flows).unwrap()
    }

    fn lint_summary(text: &str) -> Option<String> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let emacs = std::env::split_paths(&path)
            .map(|dir| dir.join("emacs"))
            .find(|candidate| candidate.is_file());
        let Some(emacs) = emacs else {
            let line = "emacs 29.3 is not on PATH, so run `apt-get install emacs-nox`.";
            assert!(
                std::env::var("LJOS_TEST_REQUIRE_TOOLS").ok().as_deref() != Some("1"),
                "{line}"
            );
            crate::say!("{line}");
            return None;
        };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("handbook.org");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&file, text).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(dir.path().join("notes.org"), "#+title: Notes\n").unwrap();
        let lisp = format!(
            "(progn (require 'org) (find-file {file:?}) (setq org-confirm-babel-evaluate nil) \
             (let* ((tree (org-element-parse-buffer)) (lint (org-lint))) \
             (dolist (r lint) (message \"LINT %s\" r)) \
             (message \"KEYWORDS %s\" (mapconcat #'identity (org-element-map tree 'keyword (lambda (k) (org-element-property :key k))) \",\")) \
             (message \"DRAWERS %d\" (length (org-element-map tree '(drawer property-drawer) #'identity))) \
             (message \"BLOCKS %d\" (length (org-element-map tree '(src-block example-block export-block quote-block special-block verse-block center-block comment-block dynamic-block) #'identity))) \
             (message \"HEADLINES %d\" (length (org-element-map tree 'headline #'identity))) \
             (message \"COMMENTS %d\" (length (org-element-map tree '(comment comment-block) #'identity))) \
             (message \"TABLES %d\" (length (org-element-map tree 'table #'identity))) \
             (kill-emacs (if lint 1 0))))"
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = std::process::Command::new(emacs)
            .args(["--batch", "--eval", &lisp])
            .env("HOME", dir.path())
            .output()
            .expect("emacs");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(out.status.success(), "org-lint\n{stderr}\n{text}");
        Some(stderr)
    }

    #[test]
    fn multiline_text_cannot_inject_org() {
        for ids in [false, true] {
            let plain = render_multiline(ids, "A plain line\nand a second one");
            let hostile = render_multiline(ids, HOSTILE);
            assert_eq!(
                structure(&hostile),
                structure(&plain),
                "ids={ids}\n{hostile}"
            );
            for line in hostile.lines() {
                assert!(
                    !line.starts_with("* Fake") && !line.starts_with("** TODO Another"),
                    "ids={ids}: {line:?}\n{hostile}"
                );
            }
            let linted_plain = lint_summary(&plain);
            let linted_hostile = lint_summary(&hostile);
            assert_eq!(linted_hostile, linted_plain, "ids={ids}\n{hostile}");
        }
    }

    #[test]
    fn marked_inline_org_syntax_refuses_every_surface() {
        for markup in [
            "[[https://example.com/notes][the notes]]",
            "[[https://example.com/notes][the notes]",
            "{{{title}}}",
            "[fn:1] note",
            "[cite:@key]",
            "<<target>>",
            "@@html:<b>@@",
            "src_sh{echo}",
            "call_name(1)",
        ] {
            assert_refused_on_every_surface(markup, &format!("Read {markup} first."));
        }
    }

    #[test]
    fn planning_and_lookalike_structure_lines_get_a_backslash() {
        for line in [
            "SCHEDULED: <2026-01-02 Fri>",
            "  deadline: <2026-01-03 Sat>",
            "Closed: [2026-01-04 Sun]",
            "\u{200b}* heading",
            "\u{ff0a} heading",
            "\u{ff03}\u{ff0b}title: x",
            "\u{feff}  :PROPERTIES:",
            "\u{200e}-----",
            "\u{ff5c} a \u{ff5c}",
            "\u{ff1a}END\u{ff1a}",
        ] {
            let out = continuation_escape(line);
            let probe = public_text::detection_copy(&out);
            assert!(
                probe.trim_start().starts_with('\\'),
                "{line:?} became {out:?}"
            );
            assert!(out.contains(line.trim_start()), "{out:?} lost text");
        }
        for line in ["SCHEDULED later", "a scheduled: b", "  plain text", "x * y"] {
            assert_eq!(continuation_escape(line), line.replace('\t', " "));
        }
    }

    #[test]
    fn accepted_text_is_written_as_it_was() {
        let text = "Read src_dir, a << b and [x] and https://example.com/a_b first.";
        let memories = [lesson("m1", text, &["project:Software"])];
        let mut options = options();
        options.trees.clear();
        let out = render(&options, &memories, &[], &[], &[]).unwrap();
        assert!(out.contains(&format!("- {text}\n")), "{out}");
    }

    #[test]
    fn a_workflow_name_with_a_line_break_stays_one_link() {
        let flows = [flow("no\n* tes]]t\n#+title: x", "Notes")];
        let mut options = options();
        options.trees.clear();
        let memories = [lesson("m1", "Publish it.", &["project:Software"])];
        let text = render(&options, &memories, &[], &[], &flows).unwrap();
        let tail = text.split("* Workflows\n").nth(1).unwrap();
        assert_eq!(tail.lines().count(), 2, "{text}");
        assert!(
            tail.lines()
                .nth(1)
                .unwrap()
                .starts_with("- [[file:no * tes\\]\\]t #+title: x.org]["),
            "{text}"
        );
    }

    #[test]
    fn the_workflow_header_alone_marks_and_titles() {
        assert!(workflow_public("#+public: yes\n"));
        assert!(workflow_public(
            "# note\n\n#+TITLE: x\n#+public: yes\n* h\n"
        ));
        for text in [
            "public: yes\n",
            "##+public: yes\n",
            "+public: yes\n",
            " #+public: yes\n",
            "#+public: no\n",
            "#+PUBLIC: yes\n",
            "#+Public: YES\n",
            "#+public: Yes\n",
            "#+public: YES\n",
            "#+public: t\n",
            "#+public: T\n",
            "#+public: true\n",
            "#+public: TRUE\n",
            "#+public:yes\n",
            "#+public:  yes\n",
            "#+public:\tyes\n",
            "#+public: yes \n",
            "#+public: yes\t\n",
            "#+public: yes\r\n",
            "#+title: x\r\n#+public: yes\r\n",
            "#+ public: yes\n",
            "#+public : yes\n",
            "#+title: x\n\nbody\n#+public: yes\n",
            "#+begin_example\n#+public: yes\n#+end_example\n",
            "#+title: x\n#+begin_src sh :results x\n#+public: yes\n#+end_src\n",
            "* h\n#+public: yes\n",
        ] {
            assert!(!workflow_public(text), "{text:?}");
        }
        assert_eq!(
            header_value("#+title: One\n#+title: Two\n", "title").as_deref(),
            Some("One")
        );
        assert_eq!(header_value("#+begin_example\n#+title: x\n", "title"), None);
        assert_eq!(header_value("text\n#+title: x\n", "title"), None);
        assert_eq!(header_value("title: x\n", "title"), None);
    }

    #[test]
    fn a_failed_or_cut_short_write_leaves_the_old_file_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.org");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, "old\n").unwrap();
        let cut = replace_with(&path, |file| {
            use std::io::Write;
            file.write_all(b"new, but cut sh")?;
            Err(std::io::Error::other("interrupted"))
        });
        assert!(cut.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
        replace_file(&path, b"new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        let fresh = dir.path().join("fresh.org");
        replace_file(&fresh, b"made\n").unwrap();
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "made\n");
    }

    #[test]
    fn a_link_in_the_way_is_replaced_and_never_written_through() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim.org");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&victim, "the victim\n").unwrap();
        let swapped = dir.path().join("book.org");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(&victim, &swapped).unwrap();
        replace_file(&swapped, b"the page\n").unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "the victim\n");
        let meta = std::fs::symlink_metadata(&swapped).unwrap();
        assert!(meta.is_file(), "the link was kept");
        assert_eq!(std::fs::read_to_string(&swapped).unwrap(), "the page\n");
        let dangling = dir.path().join("gone.org");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(dir.path().join("nowhere.org"), &dangling).unwrap();
        replace_file(&dangling, b"the page\n").unwrap();
        assert!(!dir.path().join("nowhere.org").exists());
        assert!(std::fs::symlink_metadata(&dangling).unwrap().is_file());
    }

    #[test]
    fn a_directory_out_names_faq_org_inside_it_and_a_link_to_one_is_not_entered() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(file_for(dir.path()), dir.path().join("faq.org"));
        write_output(dir.path(), "page\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("faq.org")).unwrap(),
            "page\n"
        );
        let link = dir.path().join("aim");
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        symlink(dir.path(), &link).unwrap();
        assert_eq!(file_for(&link), link);
        assert!(write_output(&link, "other\n").is_err());
    }

    fn exported_with(title: &str, note: &str) -> String {
        let mut quiet = ticket("Software-quiet", "Software", "TODO", "Quiet", None, &[]);
        quiet.public = false;
        let mut root = ticket("Software-root", "Software", "TODO", title, None, &[]);
        root.notes = vec![note.into()];
        render_text(&options(), &[], &[root, quiet])
    }

    fn leaves_no_id(text: &str, what: &str) {
        let lower = text.to_lowercase();
        for left in ["quiet", "uiet", "followup"] {
            assert!(!lower.contains(left), "{what} left {left:?}: {text}");
        }
    }

    #[test]
    fn a_hidden_id_is_redacted_whatever_follows_or_disguises_it() {
        let spellings = [
            "Software-quiet-followup",
            "Software-quiet_2",
            "Software-quiet_",
            "Software-quiet-",
            "SOFTWARE-QUIET",
            "Software\u{2011}quiet",
            "Software\u{2013}quiet",
            "Software\u{2212}quiet",
            "Software\u{2e3a}quiet",
            "Software\u{301c}quiet",
            "Software\u{200b}-quiet",
            "Software-qu\u{200d}iet",
            "\u{ff33}oftware-quiet",
            "Software%2dquiet",
            "Software%2Dquiet",
            "%53oftware-quiet",
            "Software&#45;quiet",
            "Software&#x2d;quiet",
            "Software&#X2D;quiet",
            "Software&hyphen;quiet",
            "Software\\u002dquiet",
            "Software\\u{2d}quiet",
            "Software=2Dquiet",
            "Software=2dquiet",
            "Software-*quiet*",
            "Software-=quiet=",
            "~Software~-quiet",
            "*Software*-quiet",
            "Software-\nquiet",
            "Software-\n  quiet",
            "Software\n-quiet",
            "Software_quiet",
            "Software-qu\u{0456}et-x",
        ];
        for spelling in spellings {
            let as_note = exported_with("Cut it", &format!("Wait for {spelling} first."));
            assert!(
                as_note.contains("external dependency"),
                "{spelling:?} in a note: {as_note}"
            );
            leaves_no_id(&as_note, &format!("{spelling:?} in a note"));
            let as_title = exported_with(&format!("Cut {spelling} now"), "Plain.");
            assert!(
                as_title.contains("external dependency"),
                "{spelling:?} in a title: {as_title}"
            );
            leaves_no_id(&as_title, &format!("{spelling:?} in a title"));
        }
    }

    fn hidden_in_world() -> Vec<TicketRow> {
        let mut quiet = ticket("Software-quiet", "Software", "TODO", "Quiet", None, &[]);
        quiet.public = false;
        let mut kiln = ticket("kiln-q7zz", "kiln", "TODO", "Kiln", None, &[]);
        kiln.public = true;
        let root = ticket("Software-root", "Software", "TODO", "Cut it", None, &[]);
        vec![root, quiet, kiln]
    }

    fn tracker_options() -> Options {
        let mut options = options();
        options.trees.clear();
        options
    }

    #[test]
    fn a_hidden_id_is_redacted_in_every_published_string() {
        let tickets = hidden_in_world();
        let mut lesson = lesson(
            "m1",
            "Do not cut before kiln-q7zz lands.",
            &["project:Software"],
        );
        lesson.kind = "lesson".into();
        let preference = PolicyRow {
            id: "p1".into(),
            kind: "preference".into(),
            text: "Hold the release until Software-quiet is done.".into(),
            pattern: None,
            verdict: None,
            entities: vec![crate::PUBLIC_ENTITY.into(), "project:Software".into()],
            kept: true,
        };
        let rule = PolicyRow {
            id: "p2".into(),
            kind: "rule".into(),
            text: "Never run it before Software-quiet.".into(),
            pattern: Some("cut Software-quiet".into()),
            verdict: Some("deny".into()),
            entities: vec![crate::PUBLIC_ENTITY.into(), "project:Software".into()],
            kept: true,
        };
        let mut options = tracker_options();
        options.title = Some("Before Software-quiet".into());
        let flows = [flow("Software-quiet-train", "Train for Software-quiet")];
        let text = render(&options, &[lesson], &[preference, rule], &tickets, &flows).unwrap();
        leaves_no_id(&text, "a published string");
        assert!(!text.contains("q7zz"), "{text}");
        assert!(
            text.contains("Do not cut before external dependency lands."),
            "{text}"
        );
        assert!(
            text.contains("#+title: Before external dependency"),
            "{text}"
        );
        assert!(
            text.contains("Hold the release until external dependency is done."),
            "{text}"
        );
        assert!(text.contains("Train for external dependency"), "{text}");
    }

    #[test]
    fn a_marked_ticket_of_the_project_is_named_outside_a_ticket() {
        let tickets = hidden_in_world();
        let note = lesson(
            "m1",
            "Follows Software-root and Software-root-2.",
            &["project:Software"],
        );
        let text = render(&tracker_options(), &[note], &[], &tickets, &[]).unwrap();
        assert!(
            text.contains("Follows Software-root and Software-root-2."),
            "{text}"
        );
    }

    fn redacts_in_lesson(options: &Options, tickets: &[TicketRow], text: &str) -> bool {
        let lesson = lesson(
            "m1",
            &format!("Wait for {text} first."),
            &["project:Software"],
        );
        let page = render(options, &[lesson], &[], tickets, &[]).unwrap();
        page.contains("Wait for external dependency first.")
    }

    #[test]
    fn a_token_of_a_known_project_with_an_id_suffix_is_redacted() {
        let tickets = hidden_in_world();
        let options = tracker_options();
        for token in [
            "Software-zzzz",
            "software-zz99",
            "SOFTWARE-0000",
            "software-a1b2",
            "software_zzzz",
            "Software\u{2011}abcd",
            "software%2Dzzzz",
            "software-zzzz-later",
            "kiln-zzzz",
            "kiln-0000",
        ] {
            assert!(redacts_in_lesson(&options, &tickets, token), "{token}");
        }
    }

    #[test]
    fn the_tracker_id_length_and_known_projects_set_the_shape() {
        let tickets = hidden_in_world();
        let mut options = tracker_options();
        options.id_lengths = vec![6];
        assert!(redacts_in_lesson(&options, &tickets, "software-abcdef"));
        assert!(redacts_in_lesson(&options, &tickets, "software-012345"));
        assert!(!redacts_in_lesson(&options, &tickets, "software-zzzz"));
        let mut options = tracker_options();
        options.projects = vec!["acme".into(), "ljos-hud".into()];
        assert!(redacts_in_lesson(&options, &tickets, "acme-zzzz"));
        assert!(redacts_in_lesson(&options, &tickets, "ljos-hud-ab12"));
        assert!(!redacts_in_lesson(&options, &tickets, "ljos-zzzz"));
        assert!(!redacts_in_lesson(&options, &tickets, "unknown-zzzz"));
    }

    #[test]
    fn ordinary_hyphenated_words_and_published_ids_are_not_redacted() {
        let mut options = tracker_options();
        options.projects = vec!["acme".into()];
        let words = [
            "well-known",
            "x-ray",
            "utf-8",
            "covid-19",
            "x86-64",
            "e-mail",
            "sha-256",
            "top-10",
            "h-264",
            "500-mg",
            "iso-8601",
            "CVE-2024-3094",
            "pre-2020",
            "Nov-2026",
            "Q3-2026",
            "FY-2027",
            "RFC-3339",
            "windows-1252",
            "ubuntu-2404",
            "Software-root",
            "Software-root-2",
            "software-release",
            "kiln-schedule",
        ];
        let tickets = hidden_in_world();
        for word in words {
            let lesson = lesson(
                "m1",
                &format!("Keep {word} as written."),
                &["project:Software"],
            );
            let page = render(&options, &[lesson], &[], &tickets, &[])
                .unwrap_or_else(|err| panic!("{word}: {err}"));
            assert!(
                page.contains(&format!("Keep {word} as written.")),
                "{word}: {page}"
            );
        }
    }

    #[test]
    fn a_note_does_not_name_a_ticket_the_page_leaves_out() {
        let mut hidden = ticket("Software-private", "Software", "TODO", "Quiet", None, &[]);
        hidden.public = false;
        let mut other = ticket("Other-1", "Other", "TODO", "Foreign", None, &[]);
        other.public = true;
        let mut root = ticket(
            "Software-root",
            "Software",
            "TODO",
            "Cut the release",
            None,
            &[],
        );
        root.body = "Wait for Software-private first.".into();
        root.notes = vec![
            "Coordinate with Software-private before release.".into(),
            "Ask (other-1), then Software-root-2 and Software-root.".into(),
        ];
        let text = render_text(&options(), &[], &[root, hidden, other]);
        assert!(
            text.contains("Wait for external dependency first."),
            "{text}"
        );
        assert!(
            text.contains("Coordinate with external dependency before release."),
            "{text}"
        );
        assert!(
            text.contains("Ask (external dependency), then Software-root-2 and Software-root."),
            "{text}"
        );
        assert!(!text.to_lowercase().contains("software-private"), "{text}");
        assert!(!text.contains("Other-1"), "{text}");
    }

    #[test]
    fn a_marked_private_workflow_name_refuses_and_an_unmarked_one_is_left_out() {
        let mut hidden = flow("hidden", "Hidden train");
        hidden.public = false;
        let workflows = [flow("release", "Release train"), hidden];
        let mut options = options();
        options.trees.clear();
        let text = render(&options, &[], &[], &[], &workflows).unwrap();
        assert!(text.contains("[[file:release.org][release.org]]"));
        assert!(!text.contains("hidden"));
        assert!(!text.contains("#+version"));
        let err = render(&options, &[], &[], &[], &[flow("grok", "Release notes")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("refused workflow 1 name"), "{err}");
        assert!(!err.contains("grok"), "{err}");
    }
}
