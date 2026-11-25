//! The ranking reads each ticket once, and the page it builds is the page
//! the per-call ranking built.
//!
//! [`develop`] is that per-call ranking, kept as it was except that the
//! seat and the tracker come from a [`Here`] instead of the process, and
//! that it ends with the same [`weigh_projects`](super::weigh_projects)
//! step, judged per call. The tracker here is generated: ids, titles,
//! notes and seats are made up.

use super::{merge_records, Here};
use packset_client::Hit;
use std::collections::BTreeMap;
use std::path::PathBuf;
use vissue_core::views::IssueRec;
use vissue_core::{IssueHeading, LogEntry};

mod develop {
    use super::super::{
        content_words, edit_within, facts_of, hit_for, in_cwd, in_home, in_working_project,
        intrinsic, is_ticket_hit, lift, norm, notes_text, project_rank, score_intrinsic,
        search_blob, sink, solo_local, stamp_ticket, weigh_projects, Band, Facts, Found, Here,
        Scope, Weighed, COMMON_TITLE,
    };
    use packset_client::Hit;
    use std::collections::{BTreeMap, BTreeSet};
    use vissue_core::views::IssueRec;

    pub(super) fn merge(hits: &mut Vec<Hit>, query: &str, records: &[IssueRec], here: &Here) {
        let mut found = collect_records(records, query);
        if !found.is_empty() {
            let have: BTreeSet<String> = hits.iter().filter_map(|hit| hit.id.clone()).collect();
            found.retain(|row| row.hit.id.as_ref().is_none_or(|id| !have.contains(id)));
            if !found.is_empty() {
                let (mut front, mut back): (Vec<Found>, Vec<Found>) = found
                    .into_iter()
                    .partition(|row| matches!(row.band, Band::Exact | Band::Near));
                if hits.is_empty() {
                    score_intrinsic(&mut front);
                    score_intrinsic(&mut back);
                } else {
                    let max = hits.iter().fold(0.0_f64, |top, hit| top.max(hit.score));
                    let min = hits.iter().fold(f64::MAX, |low, hit| low.min(hit.score));
                    lift(&mut front, max);
                    sink(&mut back, min);
                }
                let mut merged = Vec::with_capacity(front.len() + hits.len() + back.len());
                merged.extend(front.into_iter().map(|row| row.hit));
                merged.append(hits);
                merged.extend(back.into_iter().map(|row| row.hit));
                *hits = merged;
            }
        }
        place_rows(hits, query, records, here);
    }

    fn collect_records(rows: &[IssueRec], query: &str) -> Vec<Found> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let titles: Vec<&str> = rows
            .iter()
            .map(|rec| rec.heading.title.trim())
            .filter(|title| !title.is_empty())
            .collect();
        let mut found = Vec::new();
        for rec in rows {
            let title = rec.heading.title.trim();
            let id = rec.heading.id.trim();
            if title.is_empty() || id.is_empty() {
                continue;
            }
            let notes = notes_text(&rec.heading);
            let Some(band) = band(query, title, &notes, &titles) else {
                continue;
            };
            found.push(Found {
                band,
                hit: Hit {
                    id: Some(id.to_string()),
                    text: title.to_string(),
                    score: intrinsic(band),
                    kind: "ticket".to_string(),
                    ts: None,
                    entities: vec![format!("issue:{id}")],
                    ballots: None,
                    of: None,
                    rerank_score: None,
                    labels: Vec::new(),
                },
            });
        }
        found.sort_by(|a, b| a.band.cmp(&b.band).then_with(|| a.hit.id.cmp(&b.hit.id)));
        found
    }

    fn scope_of(records: &[IssueRec], here: &Here) -> Scope {
        let mine = here.mine.as_str();
        let cwd = here.cwd.clone();
        let mut projects = BTreeSet::new();
        for rec in records {
            if facts_of(rec).owns(mine) {
                let name = rec.project.trim().to_lowercase();
                if !name.is_empty() {
                    projects.insert(name);
                }
            }
        }
        let roster = here.roster.clone();
        let mine = crate::canonical_seat_name(mine);
        let solo = roster.len() == 1
            && roster
                .iter()
                .any(|seat| crate::canonical_seat_name(seat) == mine);
        Scope {
            cwd,
            projects,
            solo,
            home: here.home.clone(),
        }
    }

    fn by_id(records: &[IssueRec]) -> BTreeMap<&str, &IssueRec> {
        let mut map = BTreeMap::new();
        for rec in records {
            map.entry(rec.heading.id.as_str()).or_insert(rec);
        }
        map
    }

    fn ticket_coverage(query: &str, rec: &IssueRec) -> usize {
        crate::word_rank::term_coverage(query, &search_blob(rec))
    }

    fn any_band(query: &str, rec: &IssueRec, titles: &[&str]) -> bool {
        let notes = notes_text(&rec.heading);
        band(query, rec.heading.title.trim(), &notes, titles).is_some()
    }

    fn query_match(query: &str, rec: &IssueRec, titles: &[&str]) -> bool {
        ticket_coverage(query, rec) >= 2 || any_band(query, rec, titles)
    }

    fn lead_title(query: &str, rec: &IssueRec, titles: &[&str]) -> bool {
        let notes = notes_text(&rec.heading);
        matches!(
            band(query, rec.heading.title.trim(), &notes, titles),
            Some(Band::Exact | Band::Near)
        )
    }

    fn generic_only(query: &str, rec: &IssueRec) -> bool {
        let covered = crate::word_rank::covered_query_terms(query, &search_blob(rec));
        !covered.is_empty()
            && covered
                .iter()
                .all(|term| crate::word_rank::generic_term(term))
    }

    fn unrelated_generic(rec: &IssueRec, scope: &Scope, query: &str) -> bool {
        if in_working_project(rec, scope) {
            return false;
        }
        if scope.projects.is_empty() && in_cwd(scope, &rec.path) {
            return false;
        }
        generic_only(query, rec)
    }

    fn ensure_matches(
        hits: &mut Vec<Hit>,
        query: &str,
        records: &[IssueRec],
        mine: &str,
        scope: &Scope,
        titles: &[&str],
    ) {
        let have: BTreeSet<String> = hits
            .iter()
            .filter_map(|hit| hit.id.as_deref().map(str::to_string))
            .collect();
        let mut extra = Vec::new();
        for rec in records {
            let id = rec.heading.id.trim();
            if id.is_empty() || have.contains(id) || !query_match(query, rec, titles) {
                continue;
            }
            let facts = facts_of(rec);
            if facts.owns(mine) || in_working_project(rec, scope) {
                extra.push(hit_for(rec));
            }
        }
        hits.extend(extra);
    }

    fn demote_unrelated(
        rows: &mut Vec<Hit>,
        index: &BTreeMap<&str, &IssueRec>,
        scope: &Scope,
        query: &str,
    ) {
        let mut keep = Vec::new();
        let mut sink = Vec::new();
        for hit in rows.drain(..) {
            let rec = hit.id.as_deref().and_then(|id| index.get(id).copied());
            let generic =
                rec.is_some_and(|rec| is_ticket_hit(&hit) && unrelated_generic(rec, scope, query));
            if generic {
                sink.push(hit);
            } else {
                keep.push(hit);
            }
        }
        keep.extend(sink);
        *rows = keep;
    }

    fn place_rows(hits: &mut Vec<Hit>, query: &str, records: &[IssueRec], here: &Here) {
        if records.is_empty() || query.trim().is_empty() {
            return;
        }
        let mine = here.mine.clone();
        let scope = scope_of(records, here);
        let titles: Vec<&str> = records
            .iter()
            .map(|rec| rec.heading.title.trim())
            .filter(|title| !title.is_empty())
            .collect();
        ensure_matches(hits, query, records, &mine, &scope, &titles);
        if hits.is_empty() {
            return;
        }
        let index = by_id(records);
        let mut known: BTreeMap<String, Facts> = BTreeMap::new();
        for rec in records {
            known.insert(rec.heading.id.clone(), facts_of(rec));
        }
        for hit in hits.iter_mut() {
            let Some(id) = hit.id.as_deref() else {
                continue;
            };
            let Some(rec) = index.get(id).copied() else {
                continue;
            };
            let Some(facts) = known.get(id) else {
                continue;
            };
            stamp_ticket(hit, rec, facts, &scope, &mine);
        }
        let mut lead = Vec::new();
        let mut owned = Vec::new();
        let mut home = Vec::new();
        let mut open = Vec::new();
        let mut away = Vec::new();
        for hit in hits.drain(..) {
            let rec = hit.id.as_deref().and_then(|id| index.get(id).copied());
            let facts = hit.id.as_deref().and_then(|id| known.get(id));
            if rec.is_some_and(|rec| lead_title(query, rec, &titles)) {
                lead.push(hit);
            } else if rec.is_some_and(|rec| {
                facts.is_some_and(|facts| facts.owns(&mine)) && query_match(query, rec, &titles)
            }) {
                owned.push(hit);
            } else if is_ticket_hit(&hit) {
                if crate::other_seat(&hit.entities, &mine).is_some() {
                    away.push(hit);
                } else if rec.is_some_and(|rec| {
                    facts.is_some_and(|facts| solo_local(rec, &scope, facts, &mine))
                }) {
                    home.push(hit);
                } else {
                    open.push(hit);
                }
            } else if crate::other_seat(&hit.entities, &mine).is_some() {
                away.push(hit);
            } else {
                home.push(hit);
            }
        }
        owned.sort_by(|left, right| {
            let recency = |hit: &Hit| {
                hit.id
                    .as_deref()
                    .and_then(|id| known.get(id))
                    .map(|facts| facts.recency(&mine))
                    .unwrap_or(0)
            };
            let cover = |hit: &Hit| {
                hit.id
                    .as_deref()
                    .and_then(|id| index.get(id).copied())
                    .map(|rec| ticket_coverage(query, rec))
                    .unwrap_or(0)
            };
            recency(right)
                .cmp(&recency(left))
                .then_with(|| cover(right).cmp(&cover(left)))
                .then_with(|| left.id.cmp(&right.id))
        });
        demote_unrelated(&mut open, &index, &scope, query);
        demote_unrelated(&mut away, &index, &scope, query);
        let named = |rec: &IssueRec| crate::word_rank::shares_term(&rec.project, query);
        let anchored = records.iter().any(|rec| named(rec) || in_home(rec, &scope));
        let weighed = if anchored || !scope.projects.is_empty() {
            let total = crate::word_rank::QueryTerms::of(query).written().count();
            weigh_projects(&mut lead, &mut owned, &mut open, &mut away, |hit| {
                let rec = hit.id.as_deref().and_then(|id| index.get(id).copied())?;
                is_ticket_hit(hit).then(|| {
                    let covered =
                        crate::word_rank::covered_query_terms(query, &search_blob(rec)).len();
                    (
                        project_rank(rec, &scope, named(rec), anchored),
                        total > 0 && covered == total,
                    )
                })
            })
        } else {
            Weighed {
                owned: Vec::new(),
                far: Vec::new(),
            }
        };
        let mut ordered = Vec::new();
        ordered.extend(lead);
        ordered.extend(owned);
        ordered.extend(home);
        ordered.extend(weighed.owned);
        ordered.extend(open);
        ordered.extend(away);
        ordered.extend(weighed.far);
        *hits = ordered;
    }

    pub(super) fn band(query: &str, title: &str, notes: &str, corpus: &[&str]) -> Option<Band> {
        let query = query.trim();
        let title = title.trim();
        if query.is_empty() || title.is_empty() {
            return None;
        }
        if norm(query) == norm(title) {
            return Some(Band::Exact);
        }
        if near(query, title) {
            return Some(Band::Near);
        }
        let words = content_words(query);
        if words.is_empty() {
            return None;
        }
        let shared: Vec<&String> = words.iter().filter(|word| covers(word, title)).collect();
        let title_hit = if words.len() == 1 {
            !shared.is_empty()
        } else if shared.len() >= 2 {
            true
        } else if let Some(word) = shared.first() {
            !common_title_word(word, corpus)
        } else {
            false
        };
        if title_hit {
            return Some(Band::Title);
        }
        let blob = format!("{title}\n{notes}");
        covers_all(&words, &blob).then_some(Band::Notes)
    }

    fn common_title_word(word: &str, corpus: &[&str]) -> bool {
        if COMMON_TITLE.iter().any(|item| {
            word.eq_ignore_ascii_case(item) || crate::word_rank::shares_term(word, item)
        }) {
            return true;
        }
        let n = corpus.len();
        n >= 2 && corpus.iter().filter(|title| covers(word, title)).count() == n
    }

    fn near(query: &str, title: &str) -> bool {
        let query_words = content_words(query);
        let title_words = content_words(title);
        if query_words.is_empty() || title_words.is_empty() {
            return false;
        }
        if same_words(&query_words, &title_words) || one_word_apart(&query_words, &title_words) {
            return true;
        }
        typo_near(query, title)
    }

    fn same_words(left: &[String], right: &[String]) -> bool {
        left.len() == right.len()
            && left
                .iter()
                .all(|word| right.iter().any(|have| covers(word, have)))
    }

    fn one_word_apart(left: &[String], right: &[String]) -> bool {
        let (short, long) = if left.len() <= right.len() {
            (left, right)
        } else {
            (right, left)
        };
        if short.len() < 3 || long.len() - short.len() != 1 {
            return false;
        }
        let blob = long.join(" ");
        short.iter().all(|word| covers(word, &blob))
    }

    fn typo_near(query: &str, title: &str) -> bool {
        let query = norm(query);
        let title = norm(title);
        let (n, m) = (query.chars().count(), title.chars().count());
        let short = n.min(m);
        short >= 12 && n.abs_diff(m) <= 2 && edit_within(&query, &title, 2)
    }

    fn covers_all(words: &[String], text: &str) -> bool {
        !words.is_empty() && words.iter().all(|word| covers(word, text))
    }

    fn covers(word: &str, text: &str) -> bool {
        crate::word_rank::shares_term(word, text)
    }
}

/// A small deterministic generator, so a failure names a seed that repeats.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next()).unwrap_or(0) % n.max(1)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

const ROOT: &str = "/srv/tracker";

fn note(timestamp: &str, text: &str) -> LogEntry {
    LogEntry {
        timestamp: timestamp.to_string(),
        from_state: None,
        to_state: None,
        note: Some(text.to_string()),
        raw: None,
    }
}

fn raw_line(timestamp: &str, text: &str) -> LogEntry {
    LogEntry {
        timestamp: timestamp.to_string(),
        from_state: None,
        to_state: None,
        note: None,
        raw: Some(format!("- {text} {timestamp}")),
    }
}

fn ticket(
    project: &str,
    id: &str,
    title: &str,
    body: &str,
    logbook: Vec<LogEntry>,
    claim: Option<(&str, &str)>,
    path: PathBuf,
) -> IssueRec {
    let mut properties = BTreeMap::new();
    if let Some((seat, at)) = claim {
        properties.insert("CLAIMED_BY".to_string(), seat.to_string());
        properties.insert("CLAIMED_AT".to_string(), at.to_string());
    }
    IssueRec {
        project: project.to_string(),
        heading: IssueHeading {
            id: id.to_string(),
            title: title.to_string(),
            state: "TODO".to_string(),
            priority: 'B',
            properties,
            org_tags: Vec::new(),
            statistics: None,
            property_order: Vec::new(),
            extra_drawers: Vec::new(),
            body: body.to_string(),
            logbook,
            line_start: 0,
            line_end: 0,
        },
        path,
        tag_settings: vissue_core::org::TagSettings::default(),
    }
}

const PROJECTS: &[&str] = &["ljos", "vissue", "Kiln", "docs-site", "media", ""];
const SEATS: &[&str] = &["ada", "bao", "cam", "Ada", "dee"];
const WORDS: &[&str] = &[
    "store",
    "mount",
    "mounted",
    "mounting",
    "kiln",
    "glaze",
    "narrated",
    "narration",
    "video",
    "videos",
    "docs",
    "release",
    "embed",
    "records",
    "update",
    "fix",
    "note",
    "search",
    "sign-in",
    "signed-in",
    "SHA-1",
    "sha1",
    "list_scans",
    "listScans",
    "http2",
    "HTTP/2",
    "preface",
    "parser",
    "timeout",
    "mailbox",
    "index",
    "d-seams",
    "migrate",
    "webdav",
    "drain",
    "the",
    "a",
    "of",
    "GPRidge",
    "ridge",
    "walk",
    "backup",
    "drive",
    "overnight",
    "temperature",
    "storage",
    "stores",
    "anneal",
    "annealed",
    "168",
    "restack3",
    "default-on",
    "default",
    "gate",
    "gated",
];
const STAMPS: &[&str] = &[
    "[2026-11-01 Sun 09:00]",
    "[2026-11-16 Mon 18:30]",
    "[2026-11-16 Mon 08:05]",
    "[2026-10-02 Fri 12:00]",
    "",
];

fn phrase(rng: &mut Lcg, min: usize, max: usize) -> String {
    let len = min + rng.below(max - min + 1);
    (0..len)
        .map(|_| rng.pick(WORDS))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A tracker with shared and repeated titles, empty ids and titles, a
/// duplicated id, claims, filed, claimed, noted and released lines, and
/// files inside and outside the working tracker.
fn tracker(seed: u64, n: usize) -> Vec<IssueRec> {
    let mut rng = Lcg(seed);
    let mut out = Vec::with_capacity(n);
    for index in 0..n {
        let project = rng.pick(PROJECTS);
        let id = match rng.below(40) {
            0 => String::new(),
            1 if index > 0 => format!("dup-{}", index % 7),
            _ => format!(
                "{}-{index:04}",
                if project.is_empty() { "x" } else { project }
            ),
        };
        let title = match rng.below(12) {
            0 => String::new(),
            1 => "Store mount".to_string(),
            2 => "Fix the store mount timeout".to_string(),
            3 => "GPRidge narrated video set".to_string(),
            4 => "Anneal 168 restack3 default-on gate".to_string(),
            _ => phrase(&mut rng, 1, 6),
        };
        let body = if rng.chance(60) {
            phrase(&mut rng, 0, 12)
        } else {
            String::new()
        };
        let mut logbook = Vec::new();
        for _ in 0..rng.below(4) {
            let stamp = rng.pick(STAMPS);
            let seat = rng.pick(SEATS);
            logbook.push(match rng.below(6) {
                0 => note(stamp, &format!("filed by {seat}")),
                1 => note(stamp, &format!("noted by {seat}")),
                2 => raw_line(stamp, &format!("claimed by {seat}")),
                3 => note(stamp, &format!("claim released: {seat} held since {stamp}")),
                _ => note(stamp, &phrase(&mut rng, 1, 8)),
            });
        }
        let claim = rng.chance(25).then(|| (rng.pick(SEATS), rng.pick(STAMPS)));
        let dir = if rng.chance(70) { ROOT } else { "/elsewhere" };
        let path = PathBuf::from(format!("{dir}/{project}/issues.org"));
        out.push(ticket(project, &id, &title, &body, logbook, claim, path));
    }
    out
}

fn pack_hits(rng: &mut Lcg, records: &[IssueRec]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for _ in 0..rng.below(6) {
        let seat = rng.pick(SEATS);
        let (id, kind) = if rng.chance(50) && !records.is_empty() {
            let rec = &records[rng.below(records.len())];
            (rec.heading.id.clone(), "ticket")
        } else {
            (format!("mem-{}", rng.below(1000)), "lesson")
        };
        let mut entities = Vec::new();
        if rng.chance(60) {
            entities.push(format!("seat:{seat}"));
        }
        hits.push(Hit {
            id: rng.chance(90).then_some(id),
            text: phrase(rng, 1, 6),
            score: (rng.below(100) as f64) / 100.0,
            kind: kind.to_string(),
            ts: None,
            entities,
            ballots: None,
            of: None,
            rerank_score: None,
            labels: Vec::new(),
        });
    }
    hits
}

fn queries(rng: &mut Lcg, records: &[IssueRec]) -> Vec<String> {
    let mut asks: Vec<String> = [
        "store mount",
        "Store Mount",
        "  store   mount  ",
        "mount",
        "store",
        "mounting the store",
        "fix the store mount timeout",
        "fix store mount timeut",
        "update the note about search",
        "GPRidge video narration",
        "gpridge VIDEO Narration",
        "narrated video",
        "docs video",
        "d-SEAMS migrate video docs embed",
        "sha1 preface",
        "list scans",
        "sign-in",
        "the",
        "",
        "   ",
        "zzzznotaword",
        "kiln glaze overnight",
        "webdav drain delete safety",
        "release",
        "a of the",
        "anneal 168 restack3 default-on gate",
        "default-on gate",
        "restack3 anneal",
        "kiln glaze overnight backup drive",
        "store mount backup drive overnight 24",
        "GPRidge narrated video set ridge walk",
    ]
    .iter()
    .map(|ask| (*ask).to_string())
    .collect();
    for _ in 0..30 {
        asks.push(phrase(rng, 1, 5));
    }
    for _ in 0..15 {
        let rec = &records[rng.below(records.len())];
        asks.push(rec.heading.title.clone());
    }
    asks
}

fn here(mine: &str, solo: bool, cwd: bool) -> Here {
    at_home(mine, solo, cwd, &[])
}

fn at_home(mine: &str, solo: bool, cwd: bool, home: &[&str]) -> Here {
    Here {
        mine: mine.to_string(),
        cwd: cwd.then(|| PathBuf::from(ROOT)),
        roster: if solo {
            vec![mine.to_string()]
        } else {
            vec!["ada".to_string(), "bao".to_string()]
        },
        home: home.iter().map(|name| (*name).to_string()).collect(),
    }
}

#[test]
fn the_page_is_the_page_the_per_call_ranking_built() {
    let mut compared = 0usize;
    let mut moved = 0usize;
    for seed in [7_u64, 1_234, 99_991] {
        let records = tracker(seed, 120);
        let mut rng = Lcg(seed ^ 0x5eed);
        let asks = queries(&mut rng, &records);
        for (index, query) in asks.iter().enumerate() {
            let place = || {
                at_home(
                    ["ada", "bao", "zed"][index % 3],
                    index % 4 == 0,
                    index % 5 != 0,
                    [&[][..], &["Kiln"], &["media", "LJOS"]][index % 7 % 3],
                )
            };
            let start = pack_hits(&mut rng, &records);
            let mut before = start.clone();
            develop::merge(&mut before, query, &records, &place());
            let mut after = start.clone();
            merge_records(&mut after, query, &records, place);
            assert_eq!(
                format!("{after:#?}"),
                format!("{before:#?}"),
                "seed {seed} query {query:?}"
            );
            compared += 1;
            if format!("{start:?}") != format!("{after:?}") {
                moved += 1;
            }
        }
    }
    assert!(compared >= 200, "{compared} pages");
    assert!(
        moved * 2 >= compared,
        "only {moved} of {compared} pages changed"
    );
}

#[test]
fn band_reads_the_same_as_the_per_call_band() {
    let records = tracker(42, 100);
    let titles: Vec<&str> = records
        .iter()
        .map(|rec| rec.heading.title.trim())
        .filter(|title| !title.is_empty())
        .collect();
    let mut rng = Lcg(42);
    for query in queries(&mut rng, &records) {
        for rec in &records {
            let notes = super::notes_text(&rec.heading);
            let title = rec.heading.title.as_str();
            assert_eq!(
                super::band(&query, title, &notes, &titles),
                develop::band(&query, title, &notes, &titles),
                "{query:?} against {title:?}"
            );
        }
    }
}

/// `count` tickets, of which `sharing` titles share one word with the
/// query and nothing else. The shared word cycles through `leads`. Each of
/// those titles asks whether its word is in every title.
fn shared_word_tracker(count: usize, sharing: usize, leads: &[&str]) -> Vec<IssueRec> {
    let step = count / sharing;
    (0..count)
        .map(|index| {
            let title = if index % step == 0 && index / step < sharing {
                let lead = leads[(index / step) % leads.len()];
                format!("{lead} the backup drive {index}")
            } else {
                format!("Kiln glaze batch {index}")
            };
            ticket(
                "ljos",
                &format!("ljos-{index:05}"),
                &title,
                "The overnight firing log.",
                vec![note("[2026-11-16 Mon 18:30]", "filed by bao")],
                None,
                PathBuf::from(format!("{ROOT}/ljos/issues.org")),
            )
        })
        .collect()
}

/// Texts analyzed for `query`, and how many shared-word titles it printed.
fn analyses_for(query: &str, count: usize, sharing: usize, leads: &[&str]) -> (usize, usize) {
    let records = shared_word_tracker(count, sharing, leads);
    let mut hits = Vec::new();
    let before = crate::word_rank::analyzed();
    merge_records(&mut hits, query, &records, || here("ada", false, true));
    let spent = crate::word_rank::analyzed() - before;
    let titled = hits
        .iter()
        .filter(|hit| hit.text.contains(" the backup drive "))
        .count();
    (spent, titled)
}

/// The tracker behind the slow `ljos search "store mount"` had about
/// 24,000 tickets. Ten times the tickets is about ten times the work.
#[test]
fn shared_title_words_cost_linear_work() {
    let (small, small_titled) = analyses_for("store mount", 2_000, 30, &["Mount"]);
    let (large, large_titled) = analyses_for("store mount", 20_000, 300, &["Mount"]);
    assert_eq!(small_titled, 30);
    assert_eq!(large_titled, 300);
    say!("ticket analyses: 2,000 tickets {small}, 20,000 tickets {large}");
    assert!(
        large <= 12 * small,
        "20,000 tickets took {large} analyses against {small} for 2,000"
    );
    assert!(large <= 10 * 20_000, "{large} analyses for 20,000 tickets");
}

/// The query a seat saw hang on 0.45.4: six words, one of them digits and
/// one hyphenated. Each of its four content words is the only word some
/// titles share with it. Ten times the tickets is about ten times the
/// work, and twelve words cost about what two do, per ticket.
#[test]
fn long_mixed_queries_cost_work_linear_in_words_and_tickets() {
    const SIX: &str = "anneal 168 restack3 default-on gate";
    const TWELVE: &str =
        "anneal 168 restack3 default-on gate quench 2048 relink9 opt-in latch ramp soak";
    let leads = ["Anneal", "restack3", "default-on", "gate"];
    let (two, two_titled) = analyses_for("anneal gate", 20_000, 300, &leads);
    let (small, small_titled) = analyses_for(SIX, 2_000, 30, &leads);
    let (large, large_titled) = analyses_for(SIX, 20_000, 300, &leads);
    let (twelve, twelve_titled) = analyses_for(TWELVE, 20_000, 300, &leads);
    assert_eq!(two_titled, 150);
    assert_eq!(small_titled, 30);
    assert_eq!(large_titled, 300);
    assert_eq!(twelve_titled, 300);
    say!(
        "ticket analyses: two words 20,000 tickets {two}; six words 2,000 tickets {small}, 20,000 tickets {large}; twelve words 20,000 tickets {twelve}"
    );
    assert!(
        large <= 12 * small,
        "20,000 tickets took {large} analyses against {small} for 2,000"
    );
    assert!(
        twelve <= two + two / 10,
        "twelve words took {twelve} analyses against {two} for two"
    );
    assert!(large <= 10 * 20_000, "{large} analyses for 20,000 tickets");
}

/// `ljos search "LTO metatensor link"` on seat amsel-paper, word ranked
/// because memory was busy: nine of ten rows were other repos' tickets
/// that share one word. The seat's project comes first, a project the
/// query names and another repo's ticket with every word follow, and the
/// matches from other repos that miss a word are last.
#[test]
fn the_seats_project_comes_before_one_word_matches_from_other_repos() {
    const QUERY: &str = "LTO metatensor link";
    let rows = [
        ("amsel-paper", "amsel-paper-lto", "Link metatensor with LTO"),
        (
            "amsel-paper",
            "amsel-paper-figs",
            "Metatensor figures need the link step",
        ),
        ("metatensor", "metatensor-0001", "Wheel link flags"),
        (
            "chem",
            "chem-0002",
            "Metatensor LTO link error in the chem build",
        ),
        ("Kiln", "Kiln-0003", "Kiln link schedule"),
        ("Kiln", "Kiln-0004", "Link kiln logs"),
        (
            "docs-site",
            "docs-site-0005",
            "Dead link on the landing page",
        ),
        ("ljos", "ljos-0006", "Fix the broken link in the docs"),
        ("ljos", "ljos-0007", "LTO for release builds"),
        ("media", "media-0008", "LTO tape backup"),
        ("media", "media-0009", "Link previews in the gallery"),
        ("vissue", "vissue-0010", "Link checker timeout"),
    ];
    let records_filed = |by_seat: &[&str]| -> Vec<IssueRec> {
        rows.iter()
            .map(|(project, id, title)| {
                let seat = if by_seat.contains(id) {
                    "amsel-paper"
                } else {
                    "bao"
                };
                let filed = vec![note("[2026-11-16 Mon 18:30]", &format!("filed by {seat}"))];
                let path = PathBuf::from(format!("{ROOT}/{project}/issues.org"));
                ticket(project, id, title, "", filed, None, path)
            })
            .collect()
    };
    let records = records_filed(&[]);
    let memory = |id: &str, seat: Option<&str>| Hit {
        id: Some(id.to_string()),
        text: "LTO notes".to_string(),
        score: 0.5,
        kind: "lesson".to_string(),
        ts: None,
        entities: seat
            .map(|seat| format!("seat:{seat}"))
            .into_iter()
            .collect(),
        ballots: None,
        of: None,
        rerank_score: None,
        labels: Vec::new(),
    };
    let start = vec![memory("mem-mine", None), memory("mem-cam", Some("cam"))];
    let page_of = |records: &[IssueRec], home: &[&str]| {
        let mut hits = start.clone();
        merge_records(&mut hits, QUERY, records, || {
            at_home("amsel-paper", false, true, home)
        });
        hits.into_iter()
            .filter_map(|hit| hit.id)
            .collect::<Vec<_>>()
    };
    let page = |home: &[&str]| page_of(&records, home);
    let placed = page(&["amsel-paper"]);
    say!("page for {QUERY:?} on amsel-paper: {placed:?}");
    assert_eq!(
        placed,
        [
            "amsel-paper-lto",
            "mem-mine",
            "mem-cam",
            "amsel-paper-figs",
            "metatensor-0001",
            "chem-0002",
            "Kiln-0003",
            "Kiln-0004",
            "docs-site-0005",
            "ljos-0006",
            "ljos-0007",
            "media-0008",
            "media-0009",
            "vissue-0010",
        ]
    );
    let first_ten = &placed[..10];
    let noise = first_ten
        .iter()
        .filter(|id| {
            !id.starts_with("amsel-paper")
                && !id.starts_with("metatensor")
                && !id.starts_with("mem-")
                && *id != "chem-0002"
        })
        .count();
    assert_eq!(noise, 4, "{first_ten:?}");

    let unplaced = page(&[]);
    let at = |id: &str| unplaced.iter().position(|row| row == id).expect(id);
    assert!(
        at("metatensor-0001") < at("Kiln-0003") && at("chem-0002") < at("Kiln-0003"),
        "a project the query names still comes first: {unplaced:?}"
    );

    // The seat filed two of the other repos' one-word matches. They are its
    // own rows, so they follow its memories and stay ahead of another
    // seat's rows. The rest of those projects still weigh less.
    let filed = records_filed(&["Kiln-0003", "ljos-0007"]);
    let own_filed = page_of(&filed, &["amsel-paper"]);
    say!("page with two filed by amsel-paper: {own_filed:?}");
    assert_eq!(
        own_filed,
        [
            "amsel-paper-lto",
            "mem-mine",
            "Kiln-0003",
            "ljos-0007",
            "mem-cam",
            "amsel-paper-figs",
            "metatensor-0001",
            "chem-0002",
            "Kiln-0004",
            "docs-site-0005",
            "ljos-0006",
            "media-0008",
            "media-0009",
            "vissue-0010",
        ]
    );
}

const ROUGH: &[&str] = &[
    "b+rine_foo",
    "-(brineSpindle)",
    "cedar\u{b}latch",
    "café",
    "CAFÉ",
    "Straße",
    "ΣΊΣΥΦΟΣ",
    "İstanbul",
    "/}MOUNTion",
    "list_scans,",
    "(store)",
    "mount.",
    "HTTPServer",
    "sha-1",
    "\u{a0}kiln",
    "glaze\u{1c}drain",
    "東京",
    "naïve",
    "über-store",
    "-?Stories5",
    "-[-[Glaze",
    "rev x20",
    "it\u{2019}s\u{2014}done",
    "ABcABcABc",
    "\t\u{c}\u{85}\u{2028}",
];

const ROUGH_ASKS: &[&str] = &[
    "brinefoo",
    "brine spindle",
    "brinespindle",
    "cedar latch",
    "café",
    "cafe",
    "pelican",
    "istanbul",
    "mountion",
    "straße",
    "σίσυφος",
    "xylophone quadrature",
    "über store",
    "httpserver",
    "stories5",
    "glaze0",
    "rev20",
    "done",
    "abc",
];

fn roughen(rng: &mut Lcg, text: &str) -> String {
    let mut out = String::new();
    for word in text.split(' ') {
        out.push_str(word);
        out.push_str(rng.pick(&[" ", " ", " ", "\n", "-", "+", ", ", "\u{b}", "\u{a0}", "/"]));
        if rng.chance(20) {
            out.push_str(rng.pick(ROUGH));
            out.push(' ');
        }
    }
    out
}

/// [`tracker`] with punctuation, Unicode and control characters through
/// titles, bodies, notes and drawers, org tags that hold query words, and
/// a second copy of some ids with words of their own.
fn rough_tracker(seed: u64, n: usize) -> Vec<IssueRec> {
    let mut rng = Lcg(seed ^ 0xbeef);
    let mut records = tracker(seed, n);
    for rec in &mut records {
        let heading = &mut rec.heading;
        heading.body = roughen(&mut rng, &heading.body);
        if rng.chance(30) {
            heading.title = roughen(&mut rng, &heading.title);
        }
        for entry in &mut heading.logbook {
            if let Some(text) = entry.note.as_mut() {
                *text = roughen(&mut rng, text);
            }
        }
        if rng.chance(30) {
            let tag = rng.pick(&["pelican", "store", "café"]);
            heading.org_tags.push(tag.to_string());
        }
        if rng.chance(10) {
            heading.extra_drawers.push(roughen(&mut rng, "kiln drawer"));
        }
    }
    for index in 0..n / 10 {
        let mut copy = records[index * 7 % n].clone();
        copy.heading.title = "Second copy zebrafinch".to_string();
        copy.heading.body = roughen(&mut rng, "zebrafinch only here");
        records.push(copy);
    }
    records
}

/// The page 0.45.4 built, with its analyzer and a term bag per ticket, is
/// the page the one analyzer and a query cover build: the same rows in the
/// same order. [`crate::word_rank::fuzz_scale`] reads more generated trackers.
#[test]
fn the_old_analyzer_and_the_cover_build_the_same_page() {
    let mut compared = 0usize;
    let seeds = (0..3 * crate::word_rank::fuzz_scale())
        .map(|round| [3_u64, 71, 4_099][round % 3] + 7_919 * (round / 3) as u64);
    for seed in seeds {
        let records = rough_tracker(seed, 100);
        let mut rng = Lcg(seed);
        let mut asks = queries(&mut rng, &records);
        asks.extend(ROUGH_ASKS.iter().map(|ask| (*ask).to_string()));
        for (index, query) in asks.iter().enumerate() {
            let place = || {
                at_home(
                    ["ada", "bao", "zed"][index % 3],
                    index % 4 == 0,
                    index % 5 != 0,
                    [&[][..], &["Kiln"], &["media", "LJOS"]][index % 7 % 3],
                )
            };
            let start = pack_hits(&mut rng, &records);
            let mut before = start.clone();
            crate::word_rank::with_old_terms(|| {
                develop::merge(&mut before, query, &records, &place());
            });
            let mut after = start;
            merge_records(&mut after, query, &records, place);
            assert_eq!(
                format!("{after:#?}"),
                format!("{before:#?}"),
                "seed {seed} query {query:?}"
            );
            compared += 1;
        }
    }
    assert!(compared >= 250, "{compared} pages");
}

/// `brinefoo` is the joined form of `b+rine_foo` in a body, so the ticket
/// is found. A word only in an org tag, a word only on a second copy of an
/// id the tracker read drops, and an unrelated query add no row.
#[test]
fn a_joined_body_word_is_found_and_a_miss_adds_no_row() {
    let path = PathBuf::from(format!("{ROOT}/Software/issues.org"));
    let filed = vec![note("[2026-11-01 Sun 09:00]", "filed by ada")];
    let mut tagged = ticket(
        "Software",
        "soft-tag",
        "Kiln night shift",
        "overnight firing only",
        filed.clone(),
        None,
        path.clone(),
    );
    tagged.heading.org_tags.push("pelican".to_string());
    let records = vec![
        ticket(
            "Software",
            "soft-punc",
            "Ordinary kiln schedule",
            "see b+rine_foo for the valve",
            filed.clone(),
            None,
            path.clone(),
        ),
        tagged,
        ticket(
            "Software",
            "soft-dup",
            "Glaze batch",
            "cone six",
            filed,
            None,
            path,
        ),
    ];
    for query in ["brinefoo", "pelican", "zebrafinch", "xylophone quadrature"] {
        let mut before = Vec::new();
        crate::word_rank::with_old_terms(|| {
            develop::merge(&mut before, query, &records, &here("ada", false, true));
        });
        let mut after = Vec::new();
        merge_records(&mut after, query, &records, || here("ada", false, true));
        assert_eq!(format!("{after:#?}"), format!("{before:#?}"), "{query}");
        let ids: Vec<_> = after.iter().filter_map(|hit| hit.id.as_deref()).collect();
        if query == "brinefoo" {
            assert_eq!(ids, ["soft-punc"]);
        } else {
            assert!(ids.is_empty(), "{query}: {ids:?}");
        }
    }
}

/// A log of filler, ASCII or not, with the query in the middle: the ticket
/// is found, and a hundred times the filler splits and stems no more words.
#[test]
fn a_long_log_is_not_read_word_by_word() {
    let path = PathBuf::from(format!("{ROOT}/Software/issues.org"));
    let filed = vec![note("[2026-11-01 Sun 09:00]", "filed by ada")];
    let read = |filler: &str, times: usize| {
        let body = format!(
            "{}brine spindle sits in the log {}",
            filler.repeat(times),
            filler.repeat(times)
        );
        let records = vec![
            ticket(
                "Software",
                "soft-1",
                "Kiln schedule for the night",
                &body,
                filed.clone(),
                None,
                path.clone(),
            ),
            ticket(
                "Software",
                "soft-2",
                "Glaze batch",
                &filler.repeat(2 * times),
                filed.clone(),
                None,
                path.clone(),
            ),
        ];
        let before = crate::word_rank::tokens_read();
        let mut hits = Vec::new();
        merge_records(&mut hits, "brine spindle", &records, || {
            here("ada", false, true)
        });
        let ids: Vec<_> = hits.iter().filter_map(|hit| hit.id.clone()).collect();
        assert_eq!(ids, ["soft-1"], "{filler:?}");
        crate::word_rank::tokens_read() - before
    };
    for filler in [
        "qx00000001 ",
        "qx00000001 café ümlaut 東京 ",
        "qx00000001 we\u{2019}re \u{2014} done ",
    ] {
        let short = read(filler, 20);
        let long = read(filler, 2_000);
        assert_eq!(long, short, "{filler:?}");
    }
}
