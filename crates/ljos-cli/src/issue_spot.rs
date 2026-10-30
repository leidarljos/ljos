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
//!
//! A `vissue` process the seat runs about one issue is pointed at the
//! layout found here, with routing off ([`route_env`]), so it reads that
//! one tracker instead of walking every routed one.

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

/// Verbs whose second word is the issue they read or write.
const ID_VERBS: &[&str] = &["show", "vote", "children", "recall", "note", "append"];

/// The environment that points a `vissue VERB ID ...` process at the
/// layout [`find`] reaches `ID` in, with routing off, so the process
/// reads that tracker alone: `ISSUE_ROOT` and `VISSUE_ROOT` (the tracker
/// reads `ISSUE_ROOT` first), `VISSUE_PREFIX`, and `VISSUE_NO_ROUTE`.
/// Empty for any other command line and for an id [`find`] does not
/// reach; the process then routes as it always does.
#[must_use]
pub fn route_env(args: &[&str]) -> Vec<(&'static str, String)> {
    let [verb, id, ..] = args else {
        return Vec::new();
    };
    if !ID_VERBS.contains(verb) || id.starts_with('-') {
        return Vec::new();
    }
    let Some(spot) = find(id) else {
        return Vec::new();
    };
    let root = spot.layout.root().display().to_string();
    vec![
        ("ISSUE_ROOT", root.clone()),
        ("VISSUE_ROOT", root),
        ("VISSUE_PREFIX", spot.layout.prefix().to_string()),
        ("VISSUE_NO_ROUTE", "1".to_string()),
    ]
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
    fn the_card_matches_the_tracker_crate() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layout_with_board(tmp.path());
        for id in ["demo-aaaa", "demo-bbbb"] {
            let spot = in_layout(&layout, id).expect("found");
            let full = vissue_core::agent::show_json(&layout, id).unwrap();
            assert_eq!(spot.card().unwrap(), full);
            assert_eq!(spot.rec().heading.id, id);
        }
        assert!(in_layout(&layout, "demo-zzzz").is_none());
        assert!(in_layout(&layout, "other-aaaa").is_none());
    }

    /// A project beside the one asked about is not read: a walk over every
    /// project fails on a file that does not parse, a read of the issue's
    /// own project does not.
    #[test]
    fn a_lookup_reads_one_project_only() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layout_with_board(tmp.path());
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

    #[test]
    fn only_a_verb_on_an_issue_is_pointed_at_a_layout() {
        assert!(route_env(&["q", "-p", "demo", "title"]).is_empty());
        assert!(route_env(&["vote", "--json"]).is_empty());
        assert!(route_env(&["create", "demo-aaaa"]).is_empty());
        assert!(route_env(&["vote"]).is_empty());
    }
}
