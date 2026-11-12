//! A day's work: one parent and its checklist, filed in one tracker commit.
//!
//! `ljos sitting --plan` and `ljos_start` with an items list use this. The
//! parent is claimed by the sitting. `ljos ready --mine` and the sitting
//! name the next unfinished child. `ljos finish` on a child ticks it off.

use anyhow::{bail, Context, Result};

/// One filed plan: the parent, the children in the order they were given,
/// and the text a seat reads.
pub struct Opened {
    pub parent: String,
    pub children: Vec<(String, String)>,
    pub text: String,
}

/// File `title` and `items` in one tracker commit.
///
/// The parent carries the `plan` tag and `:ORDERED: t`. Each item is a
/// `checklist` child, appended in the order given. The text is the
/// checklist, the next item, and the commit line.
///
/// # Errors
///
/// The title or the items are blank, or the tracker cannot be written.
pub fn open_plan(project: Option<&str>, title: &str, items: &[String]) -> Result<Opened> {
    let title = title.trim();
    if title.is_empty() {
        bail!("plan: the title is empty");
    }
    let items = clean_items(items)?;
    let project = named_project(project);
    let _held = Batch::enter();
    let layout = vissue_core::Layout::resolve(None, None)?;
    let parent = create(&layout, &project, title, None, "plan")?;
    mark_ordered(&parent)?;
    let mut children = Vec::with_capacity(items.len());
    for item in &items {
        let id = create(&layout, &project, item, Some(parent.as_str()), "checklist")?;
        children.push((id, item.clone()));
    }
    let mut ids = vec![parent.clone()];
    ids.extend(children.iter().map(|(id, _)| id.clone()));
    for id in &ids {
        let _ = crate::persist_tracker(id, "filed");
    }
    let committed = crate::flush_tracker_commits();
    let text = render(&parent, title, &children, &committed);
    Ok(Opened {
        parent,
        children,
        text,
    })
}

/// File `items` under an existing parent, in one tracker commit.
///
/// # Errors
///
/// The parent is not a ticket, the items are blank, or the tracker cannot
/// be written.
pub fn add_items(parent: &str, items: &[String]) -> Result<Opened> {
    let parent = parent.trim();
    if parent.is_empty() {
        bail!("plan: the parent id is empty");
    }
    let items = clean_items(items)?;
    let layout = vissue_core::Layout::resolve(None, None)?;
    let hit = vissue_core::Router::load(layout.clone())
        .context("plan: the tracker could not be read")?
        .find_by_id(parent)
        .with_context(|| format!("plan: {parent} is not a ticket"))?;
    let project = hit.project.clone();
    let title = issue_title(parent).unwrap_or_else(|| parent.to_string());
    let _held = Batch::enter();
    if !is_plan(parent) {
        mark_ordered(parent)?;
    }
    let mut children = Vec::with_capacity(items.len());
    for item in &items {
        let id = create(&layout, &project, item, Some(parent), "checklist")?;
        children.push((id, item.clone()));
    }
    for (id, _) in &children {
        let _ = crate::persist_tracker(id, "filed");
    }
    let committed = crate::flush_tracker_commits();
    let text = render(parent, &title, &children, &committed);
    Ok(Opened {
        parent: parent.to_string(),
        children,
        text,
    })
}

/// The next unfinished child of a plan this seat holds.
///
/// Empty of plans: one line that says so.
///
/// # Errors
///
/// The tracker cannot be read.
pub fn ready_mine() -> Result<String> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    layout.require_tracker()?;
    let recs =
        vissue_core::catalog::load_recs(&layout).context("ready: the tracker could not be read")?;
    let seat = crate::canonical_seat_name(&crate::seat_name());
    let mut out = String::new();
    for rec in &recs {
        if !held_by(&rec.heading, &seat) || !plan_heading(&rec.heading) {
            continue;
        }
        match next_open(&recs, &rec.heading.id) {
            Some((id, title)) => out.push_str(&format!("next: {id}  {title}\n")),
            None => out.push_str(&format!("{}  plan complete\n", rec.heading.id)),
        }
    }
    if out.is_empty() {
        out.push_str("no unfinished item\n");
    }
    Ok(out)
}

/// The next line a sitting prints for `issue`, when it is a plan or one of
/// its items. Empty for any other ticket.
#[must_use]
pub fn next_item_line(issue: &str) -> String {
    let Ok(recs) = load_recs() else {
        return String::new();
    };
    let Some(parent) = plan_root(&recs, issue) else {
        return String::new();
    };
    match next_open(&recs, &parent) {
        Some((id, title)) => format!("next: {id}  {title}\n"),
        None => "plan complete\n".to_string(),
    }
}

/// Mark a checklist item done and name the next one.
///
/// `None` when `issue` is not a checklist item, so an ordinary finish
/// proceeds. A checklist item has no claim of its own: the parent is what
/// the sitting holds.
///
/// # Errors
///
/// The item is a checklist child and the tracker cannot record the tick.
pub fn tick_checklist(issue: &str) -> Result<Option<String>> {
    let recs = match load_recs() {
        Ok(recs) => recs,
        Err(_) => return Ok(None),
    };
    if !checklist_heading(heading_of(&recs, issue)) {
        return Ok(None);
    }
    let layout = vissue_core::Layout::resolve(None, None)?;
    vissue_core::ops::update(&layout, issue, Some("DONE"), None, None, None)
        .with_context(|| format!("finish: could not tick {issue}"))?;
    let next = next_item_line(issue);
    let persisted = crate::persist_tracker(issue, "ticked");
    Ok(Some(format!("ticked {issue}\n{next}{persisted}")))
}

fn clean_items(items: &[String]) -> Result<Vec<String>> {
    let items: Vec<String> = items
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();
    if items.is_empty() {
        bail!("plan: name at least one item after --");
    }
    Ok(items)
}

fn named_project(project: Option<&str>) -> String {
    if let Some(name) = project.map(str::trim).filter(|name| !name.is_empty()) {
        return name.to_string();
    }
    if let Ok(said) = crate::run_captured("vissue", &["identity"]) {
        if let Some(prefix) = said.stdout.lines().find_map(|line| {
            let value = line.trim().strip_prefix("prefix=")?.trim();
            (!value.is_empty()).then(|| value.to_string())
        }) {
            return prefix;
        }
    }
    "Software".to_string()
}

fn create(
    layout: &vissue_core::Layout,
    project: &str,
    title: &str,
    parent: Option<&str>,
    tag: &str,
) -> Result<String> {
    let dir = layout.project_issues_path(project);
    if let Some(parent_dir) = dir.parent() {
        std::fs::create_dir_all(parent_dir)
            .with_context(|| format!("plan: could not make {}", parent_dir.display()))?;
    }
    let text = vissue_core::ops::create(
        layout,
        project,
        title,
        vissue_core::ops::CreateOpts {
            issue_type: Some("task"),
            tags: Some(tag),
            parent,
            quiet: true,
            ..vissue_core::ops::CreateOpts::default()
        },
    )
    .with_context(|| format!("plan: could not file {title:?}"))?;
    let id = text.trim().to_string();
    if id.is_empty() || !id.contains('-') {
        bail!("plan: filing {title:?} printed no id");
    }
    Ok(id)
}

fn mark_ordered(id: &str) -> Result<()> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    let hit = vissue_core::Router::load(layout)
        .context("plan: the tracker could not be read")?
        .find_by_id(id)
        .with_context(|| format!("plan: {id} is not a ticket"))?;
    let path = hit.path.clone();
    let project = hit.project.clone();
    vissue_core::store::with_issues_lock(&path, || {
        let mut doc = vissue_core::IssueDoc::parse_file(&project, &path)?;
        let heading = doc
            .headings
            .iter_mut()
            .find(|heading| heading.id == id)
            .ok_or_else(|| vissue_core::Error::IssueNotFound { id: id.to_string() })?;
        vissue_core::props::insert(&mut heading.properties, "ORDERED", "t".to_string());
        if !heading.org_tags.iter().any(|tag| tag == "plan") {
            heading.org_tags.push("plan".to_string());
        }
        doc.write()?;
        Ok(())
    })
    .with_context(|| format!("plan: could not mark {id} ordered"))?;
    Ok(())
}

fn render(parent: &str, title: &str, children: &[(String, String)], committed: &str) -> String {
    let mut out = format!("{parent}  {title}\n");
    for (id, item) in children {
        out.push_str(&format!("  {id}  {item}\n"));
    }
    if let Some((id, item)) = children.first() {
        out.push_str(&format!("next: {id}  {item}\n"));
    }
    out.push_str(committed);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn load_recs() -> Result<Vec<vissue_core::views::IssueRec>> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    Ok(vissue_core::catalog::load_recs(&layout)?)
}

fn heading_of<'a>(
    recs: &'a [vissue_core::views::IssueRec],
    id: &str,
) -> Option<&'a vissue_core::IssueHeading> {
    recs.iter()
        .find(|rec| rec.heading.id == id)
        .map(|rec| &rec.heading)
}

fn plan_heading(heading: &vissue_core::IssueHeading) -> bool {
    heading.org_tags.iter().any(|tag| tag == "plan")
        || vissue_core::org::org_property_is_set(&heading.properties, "ORDERED")
}

fn checklist_heading(heading: Option<&vissue_core::IssueHeading>) -> bool {
    heading.is_some_and(|heading| heading.org_tags.iter().any(|tag| tag == "checklist"))
}

fn is_plan(id: &str) -> bool {
    load_recs()
        .ok()
        .and_then(|recs| heading_of(&recs, id).map(plan_heading))
        .unwrap_or(false)
}

fn held_by(heading: &vissue_core::IssueHeading, seat: &str) -> bool {
    if seat.is_empty() || heading.state != "STARTED" {
        return false;
    }
    heading
        .claimed_by()
        .is_some_and(|who| crate::canonical_seat_name(who) == seat)
}

/// The plan `issue` belongs to: itself when it is the parent, its parent
/// when it is a checklist item.
fn plan_root(recs: &[vissue_core::views::IssueRec], issue: &str) -> Option<String> {
    let heading = heading_of(recs, issue)?;
    if plan_heading(heading) {
        return Some(issue.to_string());
    }
    if !checklist_heading(Some(heading)) {
        return None;
    }
    let parent = heading.parent()?.to_string();
    plan_heading(heading_of(recs, &parent)?).then_some(parent)
}

fn next_open(recs: &[vissue_core::views::IssueRec], parent: &str) -> Option<(String, String)> {
    let mut children: Vec<&vissue_core::views::IssueRec> = recs
        .iter()
        .filter(|rec| rec.heading.parent() == Some(parent))
        .collect();
    children.sort_by(|a, b| {
        a.heading
            .line_start
            .cmp(&b.heading.line_start)
            .then(a.heading.id.cmp(&b.heading.id))
    });
    children.into_iter().find_map(|rec| {
        if matches!(rec.heading.state.as_str(), "DONE" | "CANCELLED") {
            None
        } else {
            Some((rec.heading.id.clone(), rec.heading.title.clone()))
        }
    })
}

fn issue_title(id: &str) -> Option<String> {
    load_recs()
        .ok()
        .and_then(|recs| heading_of(&recs, id).map(|heading| heading.title.clone()))
}

/// Holds tracker commits until drop, so a plan that fails still commits
/// what it managed to file, and a plan that flushes does not commit twice.
struct Batch;

impl Batch {
    fn enter() -> Self {
        crate::batch_tracker_commits();
        Self
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        let _ = crate::flush_tracker_commits();
    }
}

/// The issues file of `project`, for tests that read the order back.
#[cfg(test)]
fn issues_file(root: &std::path::Path, project: &str) -> std::path::PathBuf {
    let layout = vissue_core::Layout::new(root, "Software");
    layout.project_issues_path(project)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn log_subjects(root: &Path) -> Vec<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["log", "--format=%s"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn a_plan_is_one_commit_and_the_next_item_advances() {
        let _env = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "ada@example.invalid"]);
        git(&root, &["config", "user.name", "ada"]);
        unsafe {
            std::env::set_var("VISSUE_ROOT", &root);
            std::env::set_var("LJOS_SEAT", "ada");
            std::env::remove_var("LJOS_TRACKER_GIT");
            std::env::remove_var("ISSUE_ROOT");
        }
        let items = [
            "measure the files".to_string(),
            "pick a default".to_string(),
            "write the note".to_string(),
        ];
        let opened = open_plan(Some("day"), "Ship the codec", &items).unwrap();
        let subjects = log_subjects(&root);
        let filed: Vec<&String> = subjects
            .iter()
            .filter(|line| line.contains("filed"))
            .collect();
        assert_eq!(filed.len(), 1, "{subjects:?}");
        let subject = filed[0];
        assert!(subject.contains(&opened.parent), "{subject}");
        for (id, _) in &opened.children {
            assert!(subject.contains(id), "{subject}");
            let filed_at = subject.find(id).unwrap();
            let parent_at = subject.find(&opened.parent).unwrap();
            assert!(parent_at < filed_at, "{subject}");
        }
        let first = subject.find(&opened.children[0].0).unwrap();
        let second = subject.find(&opened.children[1].0).unwrap();
        let third = subject.find(&opened.children[2].0).unwrap();
        assert!(first < second && second < third, "{subject}");

        let board = std::fs::read_to_string(issues_file(&root, "day")).unwrap();
        let places: Vec<usize> = items
            .iter()
            .map(|item| board.find(item.as_str()).unwrap())
            .collect();
        assert!(places[0] < places[1] && places[1] < places[2], "{board}");
        assert!(board.find("Ship the codec").unwrap() < places[0], "{board}");
        assert!(board.contains(":ORDERED:"), "{board}");

        assert!(opened
            .text
            .contains(&format!("{}  Ship the codec", opened.parent)));
        for (id, item) in &opened.children {
            assert!(
                opened.text.contains(&format!("{id}  {item}")),
                "{}",
                opened.text
            );
        }
        assert!(
            opened.text.contains(&format!(
                "next: {}  {}",
                opened.children[0].0, opened.children[0].1
            )),
            "{}",
            opened.text
        );

        let layout = vissue_core::Layout::new(&root, "Software");
        vissue_core::ops::update(&layout, &opened.parent, Some("STARTED"), None, None, None)
            .unwrap();
        vissue_core::store::with_issues_lock(&issues_file(&root, "day"), || {
            let path = issues_file(&root, "day");
            let mut doc = vissue_core::IssueDoc::parse_file("day", &path)?;
            let heading = doc
                .headings
                .iter_mut()
                .find(|heading| heading.id == opened.parent)
                .unwrap();
            vissue_core::props::insert(&mut heading.properties, "CLAIMED_BY", "ada".to_string());
            doc.write()?;
            Ok::<(), vissue_core::Error>(())
        })
        .unwrap();
        let mine = ready_mine().unwrap();
        assert_eq!(
            mine,
            format!("next: {}  measure the files\n", opened.children[0].0)
        );

        let ticked = tick_checklist(&opened.children[0].0).unwrap().unwrap();
        assert!(ticked.starts_with(&format!("ticked {}", opened.children[0].0)));
        assert!(
            ticked.contains(&format!("next: {}  pick a default", opened.children[1].0)),
            "{ticked}"
        );
        let ticked = tick_checklist(&opened.children[1].0).unwrap().unwrap();
        assert!(
            ticked.contains(&format!("next: {}  write the note", opened.children[2].0)),
            "{ticked}"
        );
        let ticked = tick_checklist(&opened.children[2].0).unwrap().unwrap();
        assert!(ticked.contains("plan complete"), "{ticked}");
        assert!(tick_checklist("day-nope").unwrap().is_none());
    }
}
