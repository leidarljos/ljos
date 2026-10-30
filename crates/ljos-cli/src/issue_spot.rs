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
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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

/// One file's length and modification time.
type Stamp = (PathBuf, u64, std::time::SystemTime);

/// What a project's files look like on disk: the length and modification
/// time of `issues.org`, of a ledger's `issues/` directory beside it, and
/// of each file in that directory. Two reads with the same signature read
/// the same bytes, so a parse is kept while the signature holds.
fn signature(path: &Path) -> Option<Vec<Stamp>> {
    let stamp = |p: &Path| -> Option<Stamp> {
        let meta = std::fs::metadata(p).ok()?;
        Some((p.to_path_buf(), meta.len(), meta.modified().ok()?))
    };
    let mut out = vec![stamp(path)?];
    if let Some(ledger) = path.parent().map(|dir| dir.join("issues")) {
        if let Ok(entries) = std::fs::read_dir(&ledger) {
            let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            files.sort();
            out.push(stamp(&ledger)?);
            for file in files {
                out.push(stamp(&file)?);
            }
        }
    }
    Some(out)
}

/// Parses this process already made, by project file. A gate asks about
/// an issue, its parent and each child, each several times; one parse
/// answers them all while the files stay as they were.
static PARSED: Mutex<BTreeMap<(PathBuf, String), (Vec<Stamp>, Arc<Vec<IssueRec>>)>> =
    Mutex::new(BTreeMap::new());

/// Every heading of one project, with the file each one lives in.
fn project_recs(layout: &Layout, dir: &str) -> Option<Arc<Vec<IssueRec>>> {
    let path = layout.project_issues_path(dir);
    let key = (path.clone(), dir.to_string());
    let sig = signature(&path);
    if let Some(sig) = &sig {
        let held = PARSED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((seen, recs)) = held.get(&key) {
            if seen == sig {
                return Some(Arc::clone(recs));
            }
        }
    }
    let recs = Arc::new(parse_recs(&path, dir)?);
    if let Some(sig) = sig {
        PARSED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, (sig, Arc::clone(&recs)));
    }
    Some(recs)
}

fn parse_recs(path: &Path, dir: &str) -> Option<Vec<IssueRec>> {
    let doc = vissue_core::IssueDoc::parse_file(dir, path).ok()?;
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
    pub recs: Arc<Vec<IssueRec>>,
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
    let default_key = (default.root().to_path_buf(), default.prefix().to_string());
    let router = vissue_core::Router::load(default).ok()?;
    let routed = router.route(project);
    let mut tried = vec![
        (default_key.0, default_key.1),
        (routed.layout.root().to_path_buf(), routed.layout.prefix().to_string()),
    ];
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

/// The reads whose answer is kept: `vissue show|vote|children ID --json`.
const KEPT_READS: &[&str] = &["show", "vote", "children"];

/// A kept read: the command line and the layout it was pointed at, the
/// signature of the issue's project files when it ran, and what it printed.
type KeptRead = (Vec<String>, Vec<Stamp>, String);

/// Reads this process already ran. The gate asks the same question of
/// the same issue several times in one check; each `vissue` process on a
/// large tracker walks every project in it.
static READS: Mutex<Vec<KeptRead>> = Mutex::new(Vec::new());

/// The key a kept read is filed under, and the project signature it holds
/// for: `None` for any command line but a kept read about an issue
/// [`find`] reaches.
#[must_use]
pub fn read_key(args: &[&str]) -> Option<(Vec<String>, Vec<Stamp>)> {
    let [verb, id, "--json"] = args else {
        return None;
    };
    if !KEPT_READS.contains(verb) {
        return None;
    }
    let spot = find(id)?;
    let sig = signature(&spot.layout.project_issues_path(&spot.rec().project))?;
    let mut key: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    key.push(spot.layout.root().display().to_string());
    key.push(spot.layout.prefix().to_string());
    Some((key, sig))
}

/// What the read filed under `key` printed, while the project's files
/// still have signature `sig`.
#[must_use]
pub fn kept_read(key: &[String], sig: &[Stamp]) -> Option<String> {
    READS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(k, s, _)| k == key && s == sig)
        .map(|(_, _, out)| out.clone())
}

/// File what a read printed.
pub fn keep_read(key: Vec<String>, sig: Vec<Stamp>, out: &str) {
    let mut held = READS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.retain(|(k, _, _)| *k != key);
    held.push((key, sig, out.to_string()));
}

/// Drop every kept read. Any `vissue` command that is not a kept read may
/// write the tracker, and a tracker that is not a file on this disk (a
/// stand-in in a test) changes no signature when it does.
pub fn forget_reads() {
    READS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    TRACKERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// The ballots in a heading's `:VOTES:` drawer, read the way the tracker
/// crate reads them: one ballot per agent, the last line winning, and a
/// line that is not a ballot left out.
#[must_use]
pub fn heading_ballots(heading: &vissue_core::IssueHeading) -> Vec<vissue_core::ops::Ballot> {
    let Some(drawer) = heading
        .extra_drawers
        .iter()
        .find(|d| drawer_name_is(d, "VOTES"))
    else {
        return Vec::new();
    };
    let mut ballots: Vec<vissue_core::ops::Ballot> = Vec::new();
    for line in drawer.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case(":VOTES:")
            || trimmed.eq_ignore_ascii_case(":END:")
        {
            continue;
        }
        if let Some(b) = parse_ballot(trimmed) {
            match ballots.iter_mut().find(|x| x.agent == b.agent) {
                Some(existing) => *existing = b,
                None => ballots.push(b),
            }
        }
    }
    ballots
}

fn drawer_name_is(drawer: &str, name: &str) -> bool {
    drawer
        .lines()
        .next()
        .map(str::trim)
        .and_then(|first| first.strip_prefix(':'))
        .and_then(|rest| rest.strip_suffix(':'))
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
}

/// `[date] agent: choice used=X confidence=P`, the first `": "` ending the
/// agent.
fn parse_ballot(line: &str) -> Option<vissue_core::ops::Ballot> {
    let (stamp, rest) = line.strip_prefix('[')?.split_once("] ")?;
    let (agent, choice) = rest.split_once(": ")?;
    let agent = agent.trim();
    let choice = choice.trim();
    if agent.is_empty() || choice.is_empty() {
        return None;
    }
    let (choice, used, confidence) = split_ballot_tail(choice);
    Some(vissue_core::ops::Ballot {
        agent: agent.to_string(),
        choice: choice.to_string(),
        stamp: format!("[{stamp}]"),
        used,
        confidence,
    })
}

fn split_ballot_tail(choice: &str) -> (&str, Option<String>, Option<String>) {
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

/// The ballots on `id` in `layout`, from the id's own project, or `None`
/// when that project does not hold it.
#[must_use]
pub fn ballots_in(layout: &Layout, id: &str) -> Option<Vec<vissue_core::ops::Ballot>> {
    in_layout(layout, id.trim()).map(|spot| heading_ballots(&spot.rec().heading))
}

/// Whole trackers this process parsed, by layout, with the signature of the
/// project whose issue asked. A child may live in another project of the
/// same tracker, so `children` reads them all; one parse answers every
/// `children` question while the asking project keeps its files.
static TRACKERS: Mutex<BTreeMap<(PathBuf, String), (Vec<Stamp>, Arc<Vec<IssueRec>>)>> =
    Mutex::new(BTreeMap::new());

fn tracker_recs(spot: &Spot) -> Option<Arc<Vec<IssueRec>>> {
    let key = (
        spot.layout.root().to_path_buf(),
        spot.layout.prefix().to_string(),
    );
    let sig = signature(&spot.layout.project_issues_path(&spot.rec().project))?;
    {
        let held = TRACKERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((seen, recs)) = held.get(&key) {
            if *seen == sig {
                return Some(Arc::clone(recs));
            }
        }
    }
    let recs = Arc::new(vissue_core::catalog::load_recs(&spot.layout).ok()?);
    TRACKERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, (sig, Arc::clone(&recs)));
    Some(recs)
}

/// Whether the `vissue` on `PATH` is a compiled executable. Only the
/// tracker's own binary reads the files the way this process does; a
/// script by that name (a wrapper, or a stand-in in a test) answers
/// however it was written to, so it is asked.
fn tracker_is_executable() -> bool {
    use std::io::Read as _;
    let Ok(path) = which::which("vissue") else {
        return false;
    };
    let mut magic = [0u8; 4];
    if std::fs::File::open(&path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_err()
    {
        return false;
    }
    matches!(
        magic,
        [0x7f, b'E', b'L', b'F']
            | [0xcf, 0xfa, 0xed, 0xfe]
            | [0xce, 0xfa, 0xed, 0xfe]
            | [0xca, 0xfe, 0xba, 0xbe]
            | [b'M', b'Z', _, _]
    )
}

/// What `vissue vote ID --json` and `vissue children ID --json` print,
/// answered in this process from the layout [`find`] reaches `ID` in. A
/// gate and a consensus ask these about many issues, and each `vissue`
/// process pays its own start and its own walk. `None` for any other
/// command line, for an id [`find`] does not reach, and when the `vissue`
/// on `PATH` is a script rather than the tracker's executable; the caller
/// then asks that `vissue`.
#[must_use]
pub fn answer(args: &[&str]) -> Option<String> {
    let [verb, id, "--json"] = args else {
        return None;
    };
    if !matches!(*verb, "vote" | "children") || !tracker_is_executable() {
        return None;
    }
    match *verb {
        "vote" => {
            let spot = find(id)?;
            let rows: Vec<Value> = heading_ballots(&spot.rec().heading)
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
            Some(Value::Array(rows).to_string())
        }
        "children" => {
            let spot = find(id)?;
            let recs = tracker_recs(&spot)?;
            let rows = vissue_core::catalog::children_from(&recs, id.trim()).ok()?;
            let mut out = serde_json::to_string_pretty(&rows).ok()?;
            out.push('\n');
            Some(out)
        }
        _ => None,
    }
}

/// Whether `args` is a kept read's command line, whatever the issue.
#[must_use]
pub fn is_kept_read(args: &[&str]) -> bool {
    matches!(args, [verb, _, "--json"] if KEPT_READS.contains(verb))
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

    /// A parse is reused while the file is unchanged and read again once
    /// it changes, so a write between two reads is seen.
    #[test]
    fn a_kept_parse_follows_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layout_with_board(tmp.path());
        let first = in_layout(&layout, "demo-bbbb").expect("found");
        assert_eq!(first.card().unwrap()["state"], "TODO");
        let again = in_layout(&layout, "demo-bbbb").expect("found");
        assert!(Arc::ptr_eq(&first.recs, &again.recs), "the parse is kept");
        // A longer heading changes the length even inside one clock tick.
        let moved = BOARD.replace("* TODO [#C] Child", "* STARTED [#C] Child");
        std::fs::write(tmp.path().join("Issues/demo/issues.org"), moved).unwrap();
        let after = in_layout(&layout, "demo-bbbb").expect("found");
        assert_eq!(after.card().unwrap()["state"], "STARTED");
    }

    /// A kept read answers only while the project's files keep their
    /// signature, and any other tracker command drops every kept read.
    #[test]
    fn a_kept_read_lasts_while_the_files_do() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layout_with_board(tmp.path());
        let file = layout.project_issues_path("demo");
        let before = signature(&file).unwrap();
        let key = vec![
            "vote".to_string(),
            "demo-aaaa".to_string(),
            "--json".to_string(),
            tmp.path().display().to_string(),
        ];
        keep_read(key.clone(), before.clone(), "[]");
        assert_eq!(kept_read(&key, &before).as_deref(), Some("[]"));
        std::fs::write(&file, format!("{BOARD}\n")).unwrap();
        let after = signature(&file).unwrap();
        assert_ne!(before, after, "a longer file has another signature");
        assert!(kept_read(&key, &after).is_none());
        assert!(is_kept_read(&["children", "demo-aaaa", "--json"]));
        assert!(!is_kept_read(&["vote", "demo-aaaa", "--for", "x"]));
        keep_read(key.clone(), after.clone(), "[]");
        forget_reads();
        assert!(kept_read(&key, &after).is_none());
    }

    #[test]
    fn only_a_verb_on_an_issue_is_pointed_at_a_layout() {
        assert!(route_env(&["q", "-p", "demo", "title"]).is_empty());
        assert!(route_env(&["vote", "--json"]).is_empty());
        assert!(route_env(&["create", "demo-aaaa"]).is_empty());
        assert!(route_env(&["vote"]).is_empty());
    }

    /// The drawer is read as the tracker crate reads it: a recast agent's
    /// last line wins, the `used` and `confidence` tails split off, a
    /// line that is not a ballot is left out, and a heading with no
    /// drawer has no ballots.
    #[test]
    fn ballots_match_the_tracker_crate() {
        let tmp = tempfile::tempdir().unwrap();
        let board = BOARD.replace(
            "[2026-10-30 Fri 09:00] inky@box: land used=none confidence=0.9\n",
            "[2026-10-30 Fri 09:00] inky@box: land used=none confidence=0.9\n\
             [2026-10-30 Fri 09:01] pinky: hold: with a colon\n\
             not a ballot line\n\
             [2026-10-30 Fri 09:02] clyde: ship confidence=0.4\n\
             [2026-10-30 Fri 09:03] blinky: wait used=dd-1,dd-2\n\
             [2026-10-30 Fri 09:04] pinky: land used=\n",
        );
        std::fs::create_dir_all(tmp.path().join("Issues/demo")).unwrap();
        std::fs::write(tmp.path().join("Issues/demo/issues.org"), board).unwrap();
        let layout = Layout::new(tmp.path(), "Issues");
        for id in ["demo-aaaa", "demo-bbbb"] {
            let ours = ballots_in(&layout, id).expect("found");
            let theirs = vissue_core::ops::ballots(&layout, id).unwrap();
            assert_eq!(ours, theirs, "{id}");
        }
        assert_eq!(ballots_in(&layout, "demo-aaaa").unwrap().len(), 4);
        assert!(ballots_in(&layout, "demo-zzzz").is_none());
    }

    /// `children` reads the whole tracker: a child filed in another project
    /// of the same layout is listed, as the tracker lists it.
    #[test]
    fn children_match_the_tracker_crate() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = layout_with_board(tmp.path());
        std::fs::create_dir_all(tmp.path().join("Issues/other")).unwrap();
        std::fs::write(
            tmp.path().join("Issues/other/issues.org"),
            "#+TODO: TODO STARTED | DONE CANCELLED\n\n\
             * TODO [#A] Far child :task:\n\
             :PROPERTIES:\n\
             :ID:         other-cccc\n\
             :PARENT:     demo-aaaa\n\
             :END:\n",
        )
        .unwrap();
        let spot = in_layout(&layout, "demo-aaaa").expect("found");
        let recs = tracker_recs(&spot).expect("parsed");
        let ours = vissue_core::catalog::children_from(&recs, "demo-aaaa").unwrap();
        let all = vissue_core::catalog::load_recs(&layout).unwrap();
        let theirs = vissue_core::catalog::CatalogService::from_recs(&all)
            .children("demo-aaaa")
            .unwrap();
        assert_eq!(ours, theirs);
        let ids: Vec<&str> = ours.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["other-cccc", "demo-bbbb"]);
        let again = tracker_recs(&spot).expect("parsed");
        assert!(Arc::ptr_eq(&recs, &again), "the parse is kept");
        // A write to the asking project is seen by the next read.
        let moved = BOARD.replace("* TODO [#C] Child", "* STARTED [#C] Child");
        std::fs::write(tmp.path().join("Issues/demo/issues.org"), moved).unwrap();
        let spot = in_layout(&layout, "demo-aaaa").expect("found");
        let after = tracker_recs(&spot).expect("parsed");
        assert!(!Arc::ptr_eq(&recs, &after), "a changed project is read again");
        let kids = vissue_core::catalog::children_from(&after, "demo-aaaa").unwrap();
        assert_eq!(kids[1].state, "STARTED");
    }
}
