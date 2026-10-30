//! One issue read from the one project file its id names.
//!
//! The tracker crate's own lookups walk every project of every layout the
//! router knows, to report an id defined twice. On a seat whose routing
//! config names a large vault that walk opens every ledger file in it, and
//! a gate that asks for a few records pays it once per question. The push
//! gate, `ljos vote` and `ljos consensus` ask for one issue at a time, so
//! they read the project the id's prefix names and nothing else.
//!
//! The id's project is the text before its last `-`. The default layout is
//! tried first, then the layout a route sends that project to, then every
//! other routed layout. The first layout whose project file holds the id
//! answers; a second definition of the same id elsewhere is not looked for.
//! Every helper returns `None` when the id is not found that way, and the
//! caller then asks the tracker the slow way.

use serde_json::Value;
use std::path::PathBuf;
use vissue_core::Layout;
use vissue_core::views::IssueRec;

/// The project an id names: the text before its last `-`.
#[must_use]
pub fn project_of(id: &str) -> Option<&str> {
    let (project, suffix) = id.trim().rsplit_once('-')?;
    (!project.is_empty() && !suffix.is_empty()).then_some(project)
}

/// The directory under `layout` that holds `project`, matched exactly and
/// then ignoring case, or `None` when the layout has no such project.
fn project_dir(layout: &Layout, project: &str) -> Option<String> {
    if layout.project_issues_path(project).exists() {
        return Some(project.to_string());
    }
    let lower = project.to_lowercase();
    vissue_core::store::list_projects(layout)
        .ok()?
        .into_iter()
        .find(|name| name.to_lowercase() == lower)
}

/// Every heading of one project, with the file each one lives in.
fn project_recs(layout: &Layout, dir: &str) -> Option<Vec<IssueRec>> {
    let path = layout.project_issues_path(dir);
    let doc = vissue_core::IssueDoc::parse_file(dir, &path).ok()?;
    let files: Vec<PathBuf> = doc
        .headings
        .iter()
        .map(|heading| doc.heading_path(&heading.id).to_path_buf())
        .collect();
    let tag_settings = doc.tag_settings.clone();
    Some(
        doc.headings
            .into_iter()
            .zip(files)
            .map(|(heading, path)| IssueRec {
                project: dir.to_string(),
                heading,
                path,
                tag_settings: tag_settings.clone(),
            })
            .collect(),
    )
}

/// An issue found in its own project: the layout, and every heading of
/// that project, so a caller can read the issue and its siblings from one
/// parse.
#[derive(Debug, Clone)]
pub struct Spot {
    pub layout: Layout,
    pub recs: Vec<IssueRec>,
    id: String,
}

impl Spot {
    /// The record of the issue this spot was found for.
    #[must_use]
    pub fn rec(&self) -> &IssueRec {
        self.recs
            .iter()
            .find(|rec| rec.heading.id == self.id)
            .expect("a spot is only made for an id its recs hold")
    }

    /// The card `vissue show --json` prints for the issue.
    #[must_use]
    pub fn card(&self) -> Option<Value> {
        let svc = vissue_core::catalog::CatalogService::from_recs(&self.recs);
        serde_json::to_value(svc.detail(&self.id).ok()?).ok()
    }

    /// The rows `vissue children ID --json` prints, from the issue's own
    /// project: `ljos file --parent` and the workflow verbs file every
    /// child there.
    #[must_use]
    pub fn children(&self) -> Option<Vec<Value>> {
        let svc = vissue_core::catalog::CatalogService::from_recs(&self.recs);
        let rows = svc.children(&self.id).ok()?;
        serde_json::to_value(rows)
            .ok()?
            .as_array()
            .cloned()
    }

    /// The rows `vissue vote ID --json` prints, read from the heading's
    /// `VOTES` drawer in drawer order. One ballot per agent, the last line
    /// winning, as the tracker reads a hand-edited duplicate.
    #[must_use]
    pub fn ballots(&self) -> Vec<Value> {
        ballots_of(&self.rec().heading)
    }
}

/// What `vissue show ID --json`, `vissue vote ID --json` or `vissue
/// children ID --json` would print, read in this process, or `None` for
/// any other command line and for an id [`find`] does not reach.
#[must_use]
pub fn answer(args: &[&str]) -> Option<String> {
    let [verb, id, "--json"] = args else {
        return None;
    };
    let spot = find(id)?;
    let value = match *verb {
        "show" => spot.card()?,
        "vote" => Value::Array(spot.ballots()),
        "children" => Value::Array(spot.children()?),
        _ => return None,
    };
    serde_json::to_string(&value).ok()
}

/// `vissue vote ID --for CHOICE --used USED [--confidence P]` as `who`,
/// written in this process to the layout [`find`] reaches the id in.
/// `None` when [`find`] does not reach it: the caller runs `vissue`.
pub fn cast(
    id: &str,
    who: &str,
    choice: &str,
    used: &str,
    confidence: Option<&str>,
) -> Option<anyhow::Result<String>> {
    let spot = find(id)?;
    Some(
        vissue_core::ops::vote_with(&spot.layout, id, Some(choice), who, Some(used), confidence)
            .map_err(anyhow::Error::from),
    )
}

/// [`Spot`] for `id` in `layout` alone.
#[must_use]
pub fn in_layout(layout: &Layout, id: &str) -> Option<Spot> {
    let project = project_of(id)?;
    let dir = project_dir(layout, project)?;
    let recs = project_recs(layout, &dir)?;
    recs.iter()
        .any(|rec| rec.heading.id == id)
        .then(|| Spot {
            layout: layout.clone(),
            recs,
            id: id.to_string(),
        })
}

/// [`Spot`] for `id`: the default layout, then the layout a route names
/// for the id's project, then the other routed layouts.
#[must_use]
pub fn find(id: &str) -> Option<Spot> {
    let id = id.trim();
    let project = project_of(id)?;
    let default = Layout::resolve(None, None).ok()?;
    if let Some(spot) = in_layout(&default, id) {
        return Some(spot);
    }
    let router = vissue_core::Router::load(default).ok()?;
    let routed = router.route(project);
    let mut tried = vec![(routed.layout.root().to_path_buf(), routed.layout.prefix().to_string())];
    if let Some(dir) = project_dir(&routed.layout, &routed.dir) {
        if let Some(recs) = project_recs(&routed.layout, &dir) {
            if recs.iter().any(|rec| rec.heading.id == id) {
                return Some(Spot {
                    layout: routed.layout.clone(),
                    recs,
                    id: id.to_string(),
                });
            }
        }
    }
    for layout in router.unique_layouts() {
        let key = (layout.root().to_path_buf(), layout.prefix().to_string());
        if tried.contains(&key) {
            continue;
        }
        tried.push(key);
        if let Some(spot) = in_layout(layout, id) {
            return Some(spot);
        }
    }
    None
}

/// The name of the drawer ballots live in.
const VOTES_DRAWER: &str = "VOTES";

fn drawer_is(drawer: &str, name: &str) -> bool {
    drawer
        .lines()
        .next()
        .map(str::trim)
        .and_then(|first| first.strip_prefix(':'))
        .and_then(|rest| rest.strip_suffix(':'))
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
}

/// `choice used=X confidence=P`, split the way the tracker splits it.
fn split_tail(choice: &str) -> (&str, Option<String>, Option<String>) {
    if let Some((choice, tail)) = choice.split_once(" used=") {
        let (used, confidence) = match tail.split_once(" confidence=") {
            Some((used, confidence)) => (used, Some(confidence.to_string())),
            None => (tail, None),
        };
        let used = (!used.is_empty()).then(|| used.to_string());
        return (choice.trim(), used, confidence);
    }
    if let Some((choice, confidence)) = choice.split_once(" confidence=") {
        return (choice.trim(), None, Some(confidence.to_string()));
    }
    (choice, None, None)
}

/// One drawer line, `[stamp] agent: choice`, as a `vissue vote --json` row.
fn ballot_row(line: &str) -> Option<(String, Value)> {
    let (stamp, rest) = line.strip_prefix('[')?.split_once("] ")?;
    let (agent, choice) = rest.split_once(": ")?;
    let agent = agent.trim();
    let choice = choice.trim();
    if agent.is_empty() || choice.is_empty() {
        return None;
    }
    let (choice, used, confidence) = split_tail(choice);
    Some((
        agent.to_string(),
        serde_json::json!({
            "agent": agent,
            "choice": choice,
            "stamp": format!("[{stamp}]"),
            "used": used,
            "confidence": confidence,
        }),
    ))
}

/// The ballots on `heading`, as `vissue vote ID --json` prints them.
#[must_use]
pub fn ballots_of(heading: &vissue_core::IssueHeading) -> Vec<Value> {
    let Some(drawer) = heading
        .extra_drawers
        .iter()
        .find(|d| drawer_is(d, VOTES_DRAWER))
    else {
        return Vec::new();
    };
    let mut rows: Vec<(String, Value)> = Vec::new();
    for line in drawer.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case(&format!(":{VOTES_DRAWER}:"))
            || trimmed.eq_ignore_ascii_case(":END:")
        {
            continue;
        }
        if let Some((agent, row)) = ballot_row(trimmed) {
            match rows.iter_mut().find(|(a, _)| *a == agent) {
                Some(existing) => existing.1 = row,
                None => rows.push((agent, row)),
            }
        }
    }
    rows.into_iter().map(|(_, row)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARD: &str = "#+TODO: TODO STARTED | DONE CANCELLED\n\n\
* STARTED [#A] Parent :task:\n\
:PROPERTIES:\n\
:ID:         demo-aaaa\n\
:CLAIMED_BY: inky@box\n\
:END:\n\
:VOTES:\n\
[2026-10-30 Fri 09:00] inky@box: land used=none confidence=0.9\n\
[2026-10-30 Fri 09:01] other@box: hold\n\
[2026-10-30 Fri 09:02] inky@box: land used=deed-1\n\
:END:\n\
Body.\n\n\
* TODO [#C] Child :task:\n\
:PROPERTIES:\n\
:ID:         demo-bbbb\n\
:PARENT:     demo-aaaa\n\
:END:\n";

    fn layout_with_board(dir: &std::path::Path) -> Layout {
        std::fs::create_dir_all(dir.join("Issues/demo")).unwrap();
        std::fs::write(dir.join("Issues/demo/issues.org"), BOARD).unwrap();
        Layout::new(dir, "Issues")
    }

    #[test]
    fn the_project_is_the_text_before_the_last_dash() {
        assert_eq!(project_of("ebstack-vdjv"), Some("ebstack"));
        assert_eq!(project_of("teachxai-scicode-w3bv"), Some("teachxai-scicode"));
        assert_eq!(project_of("xAI/code-quality-ffzl"), Some("xAI/code-quality"));
        assert_eq!(project_of("nodash"), None);
        assert_eq!(project_of("trailing-"), None);
    }

    #[test]
    fn a_spot_reads_the_card_the_ballots_and_the_children() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layout_with_board(tmp.path());
        let spot = in_layout(&layout, "demo-aaaa").expect("found");
        let card = spot.card().expect("card");
        assert_eq!(card["state"], "STARTED");
        assert_eq!(card["claimed_by"], "inky@box");
        let ballots = spot.ballots();
        assert_eq!(ballots.len(), 2);
        assert_eq!(ballots[0]["agent"], "inky@box");
        assert_eq!(ballots[0]["choice"], "land");
        assert_eq!(ballots[0]["used"], "deed-1");
        assert!(ballots[0]["confidence"].is_null());
        assert_eq!(ballots[1]["choice"], "hold");
        let children = spot.children().expect("children");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["id"], "demo-bbbb");
        assert!(in_layout(&layout, "demo-zzzz").is_none());
        assert!(in_layout(&layout, "other-aaaa").is_none());
    }

    #[test]
    fn the_card_matches_the_tracker_crate() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layout_with_board(tmp.path());
        let spot = in_layout(&layout, "demo-bbbb").expect("found");
        let full = vissue_core::agent::show_json(&layout, "demo-bbbb").unwrap();
        assert_eq!(spot.card().unwrap(), full);
        let ballots: Vec<Value> = vissue_core::ops::ballots(&layout, "demo-aaaa")
            .unwrap()
            .iter()
            .map(|b| {
                serde_json::json!({
                    "agent": b.agent,
                    "choice": b.choice,
                    "stamp": b.stamp,
                    "used": b.used,
                    "confidence": b.confidence,
                })
            })
            .collect();
        assert_eq!(in_layout(&layout, "demo-aaaa").unwrap().ballots(), ballots);
    }

    #[test]
    fn only_the_three_reads_are_answered_here() {
        assert!(answer(&["vote", "demo-aaaa"]).is_none());
        assert!(answer(&["vote", "demo-aaaa", "--for", "x"]).is_none());
        assert!(answer(&["recall", "demo-aaaa", "--json"]).is_none());
    }

    /// A project of many issues beside the one asked about is not read.
    #[test]
    fn a_lookup_reads_one_project_only() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layout_with_board(tmp.path());
        // A second project whose file does not parse: a walk over every
        // project fails on it, a read of `demo` alone does not.
        std::fs::create_dir_all(tmp.path().join("Issues/broken")).unwrap();
        std::fs::write(
            tmp.path().join("Issues/broken/issues.org"),
            "* TODO no id here\n:PROPERTIES:\n:END:\n",
        )
        .unwrap();
        assert!(vissue_core::agent::show_json(&layout, "demo-aaaa").is_err());
        let spot = in_layout(&layout, "demo-aaaa").expect("found");
        assert_eq!(spot.card().unwrap()["id"], "demo-aaaa");
    }
}
