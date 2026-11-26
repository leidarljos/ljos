//! Tracker tickets on a `ljos search` page.
//!
//! The pack search ranks memories. A ticket lives in the tracker, so a
//! query that is the ticket's own title used to miss it and print
//! `Nothing relevant was found.` This module reads the tracker after that
//! ranking, on the word path and the encoder path alike.
//!
//! A title match comes before a notes match. An exact title, or one a word
//! or a typo away, is placed first. The printed text is the title. The id
//! is the ticket id, which `ljos recall` accepts.
//!
//! The encoder ranks a stored title, so `sign-in` sits next to every other
//! sign-in title and a ticket whose body or notes hold the query never
//! moves. After the merge, a ticket this seat filed, claimed, or noted is
//! placed with this seat's rows when its title, body or notes match. The
//! newest note weighs more than an older one. A ticket in a project this
//! seat works in comes before another project's ticket that only shares a
//! generic word such as docs, release, or video.
//!
//! The seat's own project is the one `LJOS_PROJECT`, the checkout or
//! `vissue identity` names, and the repo it runs in. That project and a
//! project the query names are nearest, then a project the seat's facts
//! name, then any other. A ticket in a farther project comes after nearer
//! ones in its group. When it also misses a query word it leaves the
//! group: the seat's own ticket goes after the seat's memories, and any
//! other goes to the end of the page. That is the word match from another
//! repo that shares one word.
//!
//! Another seat's ticket stays after this seat's rows. That is the line
//! the page prints, that this seat's rows come before another seat's.
//!
//! A pack row with no `seat:` tag is this seat's memory: the pack stamps
//! another seat when the row is theirs. A ticket is not that store. Who
//! filed it, who holds or held the claim, and who wrote its notes are the
//! tracker facts, and a missing tag does not make another seat's ticket
//! this seat's. The one local case is a tracker whose `seats` roster names
//! only this seat. An empty roster is not that case: any seat may write.

use crate::word_rank::{Cover, Have, QueryTerms, TermBag};
use packset_client::Hit;
use std::collections::{BTreeMap, BTreeSet};
use vissue_core::views::IssueRec;

#[cfg(test)]
mod rank_tests;

/// How close a ticket is to the query. Earlier bands are printed first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Band {
    Exact,
    Near,
    Title,
    Notes,
}

struct Found {
    band: Band,
    hit: Hit,
}

/// Claim facts read with one ticket-search corpus.
///
/// A complete load can answer that a missing claim is not held without
/// spawning `vissue claims`. An incomplete load leaves that answer to the
/// existing command fallback.
#[derive(Debug, Default)]
pub struct LoadedClaims {
    by_id: BTreeMap<String, Claim>,
    complete: bool,
}

/// The claim on the heading search kept for one id.
#[derive(Debug)]
struct Claim {
    /// `:CLAIMED_BY:` as the heading writes it.
    holder: String,
    started: bool,
}

/// Who holds one ticket, as a [`LoadedClaims`] read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadedClaim<'a> {
    /// The seat that asked.
    Seat,
    /// A holder that is not that seat, as the heading writes it. `LJOS_HOLD`
    /// or a runner record can still make it this conversation's holder.
    Other { holder: &'a str, started: bool },
    /// No live claim, and every tracker loaded.
    Free,
}

impl LoadedClaims {
    pub(crate) fn claim(&self, id: &str, seat: &str) -> Option<LoadedClaim<'_>> {
        let seat = crate::canonical_seat_name(seat);
        match self.by_id.get(id) {
            Some(claim) if crate::canonical_seat_name(&claim.holder) == seat => {
                Some(LoadedClaim::Seat)
            }
            Some(claim) => Some(LoadedClaim::Other {
                holder: &claim.holder,
                started: claim.started,
            }),
            None if self.complete => Some(LoadedClaim::Free),
            None => None,
        }
    }
}

/// Insert tracker tickets that match `query`.
///
/// Exact and near titles go in front of the memories already ranked, so
/// they stay first after the limit cut. Other title matches, then notes
/// matches, follow those memories. `hits` is unchanged when the tracker
/// has no match, or when it cannot be read.
pub fn merge(hits: &mut Vec<Hit>, query: &str) -> LoadedClaims {
    let (records, claims) = tickets();
    merge_records(hits, query, &records, Here::current);
    claims
}

/// This seat and the tracker it sits in, read only when a page is placed.
struct Here {
    mine: String,
    cwd: Option<std::path::PathBuf>,
    roster: Vec<String>,
    /// The project this seat names and the repo it sits in.
    home: Vec<String>,
}

impl Here {
    fn current() -> Self {
        Self {
            mine: crate::seat_name(),
            cwd: vissue_core::Layout::resolve(None, None)
                .ok()
                .map(|layout| layout.root().to_path_buf()),
            roster: crate::tracker_roster_names(),
            home: home_projects(),
        }
    }
}

/// `LJOS_PROJECT`, the checkout's project, or `vissue identity`, then the
/// name of the git checkout the seat runs in.
fn home_projects() -> Vec<String> {
    let mut home: Vec<String> = crate::seat_tracker_project().into_iter().collect();
    let repo = std::env::current_dir()
        .ok()
        .and_then(|cwd| crate::checkout_root(&cwd))
        .and_then(|top| {
            top.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
    home.extend(repo);
    home
}

fn merge_records(
    hits: &mut Vec<Hit>,
    query: &str,
    records: &[IssueRec],
    here: impl FnOnce() -> Here,
) {
    if query.trim().is_empty() {
        return;
    }
    let tickets = read_tickets(records, query);
    let mut found = collect_records(&tickets);
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
    // The pack may already have returned these tickets, ranked by the
    // title embedding. Who filed, claimed, or noted them still has to
    // move them, including a match the title embedding never returned.
    place_rows(hits, &tickets, here);
}

fn score_intrinsic(rows: &mut [Found]) {
    for (index, row) in rows.iter_mut().enumerate() {
        let dropped = index as f64 * 0.01;
        row.hit.score = (intrinsic(row.band) - dropped).max(0.01);
    }
}

fn lift(rows: &mut [Found], max: f64) {
    let count = rows.len();
    for (index, row) in rows.iter_mut().enumerate() {
        row.hit.score = max + 0.01 * (count - index) as f64;
    }
}

fn sink(rows: &mut [Found], min: f64) {
    let mut score = (min - 0.01).max(0.01);
    for row in rows {
        row.hit.score = score;
        score = (score - 0.01).max(0.01);
    }
}

fn intrinsic(band: Band) -> f64 {
    match band {
        Band::Exact => 1.0,
        Band::Near => 0.95,
        Band::Title => 0.70,
        Band::Notes => 0.45,
    }
}

/// One query, read once for every ticket on the page.
struct Asked {
    text: String,
    norm: String,
    /// Content words, each with its own terms and whether it is a word
    /// that labels titles rather than naming one.
    words: Vec<String>,
    word_terms: Vec<QueryTerms>,
    common: Vec<bool>,
    /// The terms of the content words joined, which a title one word
    /// shorter than the query must be inside.
    joined: TermBag,
    terms: QueryTerms,
    /// Per entry of `terms`, whether it is a generic word.
    generic: Vec<bool>,
}

impl Asked {
    /// `corpus` is every non-empty title, which is what makes a word
    /// common to all of them.
    fn new(query: &str, corpus: &[&TermBag]) -> Self {
        let text = query.trim().to_string();
        let words = content_words(&text);
        let word_terms: Vec<QueryTerms> = words.iter().map(|word| QueryTerms::of(word)).collect();
        let common = words
            .iter()
            .zip(&word_terms)
            .map(|(word, terms)| {
                listed_common(word)
                    || (corpus.len() >= 2 && corpus.iter().all(|title| terms.any_in(*title)))
            })
            .collect();
        let joined = TermBag::of(&words.join(" "));
        let terms = QueryTerms::of(&text);
        let generic = terms
            .written()
            .map(crate::word_rank::generic_term)
            .collect();
        Self {
            norm: norm(&text),
            text,
            words,
            word_terms,
            common,
            joined,
            terms,
            generic,
        }
    }
}

/// What the page reads about one ticket, worked out once.
struct Ticket<'r> {
    rec: &'r IssueRec,
    facts: Facts,
    band: Option<Band>,
    /// Query terms the title, body and notes cover.
    cover: usize,
    /// Every covered query term is a generic word, and there is one.
    generic: bool,
    /// The title, body and notes cover every query term.
    full: bool,
    /// The query names the ticket's project.
    named: bool,
}

impl Ticket<'_> {
    /// Two query terms, or a title band. One shared generic word is not a match.
    fn query_match(&self) -> bool {
        self.cover >= 2 || self.band.is_some()
    }

    fn lead_title(&self) -> bool {
        matches!(self.band, Some(Band::Exact | Band::Near))
    }
}

fn read_tickets<'r>(records: &'r [IssueRec], query: &str) -> Vec<Ticket<'r>> {
    let titles: Vec<Option<TermBag>> = records
        .iter()
        .map(|rec| {
            let title = rec.heading.title.trim();
            (!title.is_empty()).then(|| TermBag::of(title))
        })
        .collect();
    let corpus: Vec<&TermBag> = titles.iter().flatten().collect();
    let asked = Asked::new(query, &corpus);
    let total = asked.terms.written().count();
    let query_bag = TermBag::of(&asked.text);
    let queries: Vec<&QueryTerms> = std::iter::once(&asked.terms)
        .chain(&asked.word_terms)
        .collect();
    let mut blob = Cover::of(&queries);
    let mut named: BTreeMap<&str, bool> = BTreeMap::new();
    records
        .iter()
        .zip(&titles)
        .map(|(rec, title_bag)| {
            let project = rec.project.as_str();
            let named = *named
                .entry(project)
                .or_insert_with(|| QueryTerms::of(project).any_in(&query_bag));
            blob.clear();
            blob.read(rec.heading.title.trim());
            each_note(&rec.heading, |text| blob.read(text));
            let band = title_bag
                .as_ref()
                .and_then(|bag| band_with(&asked, rec.heading.title.trim(), bag, &blob));
            let mut cover = 0;
            let mut generic = true;
            for (hit, generic_term) in asked.terms.hits(&blob).zip(&asked.generic) {
                if hit {
                    cover += 1;
                    generic &= *generic_term;
                }
            }
            Ticket {
                rec,
                facts: facts_of(rec),
                band,
                cover,
                generic: cover > 0 && generic,
                full: total > 0 && cover == total,
                named,
            }
        })
        .collect()
}

fn collect_records(tickets: &[Ticket<'_>]) -> Vec<Found> {
    let mut found = Vec::new();
    for ticket in tickets {
        let title = ticket.rec.heading.title.trim();
        let id = ticket.rec.heading.id.trim();
        if title.is_empty() || id.is_empty() {
            continue;
        }
        let Some(band) = ticket.band else {
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

fn tickets() -> (Vec<IssueRec>, LoadedClaims) {
    let Ok(layout) = vissue_core::Layout::resolve(None, None) else {
        return (Vec::new(), LoadedClaims::default());
    };
    if layout.require_tracker().is_err() {
        return (Vec::new(), LoadedClaims::default());
    }
    let (layouts, mut complete) = match vissue_core::Router::load(layout.clone()) {
        Ok(router) => (
            router
                .unique_layouts()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            true,
        ),
        Err(_) => (vec![layout], false),
    };
    let mut out = Vec::new();
    let mut claims = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for layout in &layouts {
        let records = match vissue_core::catalog::load_recs(layout) {
            Ok(records) => records,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        for rec in records {
            if rec.heading.id.is_empty() {
                continue;
            }
            // A repeated id keeps its first heading, and the claim is that
            // heading's, not another heading's with the same id.
            if seen.insert(rec.heading.id.clone()) {
                if let Some(holder) = rec.heading.claimed_by() {
                    claims.insert(
                        rec.heading.id.clone(),
                        Claim {
                            holder: holder.to_string(),
                            started: rec.heading.state == "STARTED",
                        },
                    );
                }
                out.push(rec);
            }
        }
    }
    (
        out,
        LoadedClaims {
            by_id: claims,
            complete,
        },
    )
}

/// The body, each logbook note and each drawer, trimmed. After the title,
/// that is what a lexical match reads, one line each.
fn each_note(heading: &vissue_core::IssueHeading, mut each: impl FnMut(&str)) {
    each(heading.body.trim());
    for entry in &heading.logbook {
        if entry.is_bookkeeping() {
            continue;
        }
        if let Some(text) = crate::logbook_text(entry) {
            each(text.trim());
        }
    }
    for drawer in &heading.extra_drawers {
        each(drawer.trim());
    }
}

#[cfg(test)]
fn notes_text(heading: &vissue_core::IssueHeading) -> String {
    let mut notes = String::new();
    each_note(heading, |text| {
        if text.is_empty() {
            return;
        }
        if !notes.is_empty() {
            notes.push('\n');
        }
        notes.push_str(text);
    });
    notes
}

/// Title, body and logbook notes, which is what a lexical match reads.
#[cfg(test)]
fn search_blob(rec: &IssueRec) -> String {
    let notes = notes_text(&rec.heading);
    let title = rec.heading.title.trim();
    if notes.is_empty() {
        title.to_string()
    } else {
        format!("{title}\n{notes}")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TouchKind {
    Filed,
    Claim,
    Note,
}

struct Touch {
    seat: String,
    /// Later is larger. A missing stamp is zero.
    when: i64,
    kind: TouchKind,
}

struct Facts {
    touches: Vec<Touch>,
}

impl Facts {
    fn owns(&self, mine: &str) -> bool {
        let mine = crate::canonical_seat_name(mine);
        !mine.is_empty() && self.touches.iter().any(|touch| touch.seat == mine)
    }

    fn recency(&self, mine: &str) -> i64 {
        let mine = crate::canonical_seat_name(mine);
        self.touches
            .iter()
            .filter(|touch| touch.seat == mine)
            .map(|touch| touch.when)
            .max()
            .unwrap_or(0)
    }

    /// The seat to show when this seat has no part in the ticket.
    ///
    /// A note outweighs a claim or a filing when it is the later fact.
    /// Among notes, the newest one weighs the most.
    fn other_seat(&self, mine: &str) -> Option<String> {
        if self.owns(mine) {
            return None;
        }
        let notes = self
            .touches
            .iter()
            .filter(|touch| touch.kind == TouchKind::Note);
        notes
            .max_by_key(|touch| touch.when)
            .or_else(|| self.touches.iter().max_by_key(|touch| touch.when))
            .map(|touch| touch.seat.clone())
    }

    fn names_other(&self, mine: &str) -> bool {
        let mine = crate::canonical_seat_name(mine);
        self.touches.iter().any(|touch| touch.seat != mine)
    }
}

fn named_seat(raw: &str) -> Option<String> {
    let seat = crate::canonical_seat_name(raw);
    (!seat.is_empty()).then_some(seat)
}

fn facts_of(rec: &IssueRec) -> Facts {
    let mut touches = Vec::new();
    if let Some(seat) = rec.heading.claimed_by().and_then(named_seat) {
        touches.push(Touch {
            seat,
            when: stamp_ord(rec.heading.claimed_at().unwrap_or("")),
            kind: TouchKind::Claim,
        });
    }
    for entry in &rec.heading.logbook {
        let Some(text) = crate::logbook_text(entry) else {
            continue;
        };
        let when = stamp_ord(&entry.timestamp);
        if let Some(seat) = released_holder(&text) {
            touches.push(Touch {
                seat,
                when,
                kind: TouchKind::Claim,
            });
            continue;
        }
        let Some((verb, seat)) = crate::parse_logbook_act(&format!("- {text}")) else {
            continue;
        };
        let Some(seat) = named_seat(&seat) else {
            continue;
        };
        let verb = verb.to_ascii_lowercase();
        let kind = if verb.starts_with("noted") {
            TouchKind::Note
        } else if verb.starts_with("filed") {
            TouchKind::Filed
        } else if verb.starts_with("claimed") || verb.starts_with("released") {
            TouchKind::Claim
        } else {
            continue;
        };
        touches.push(Touch { seat, when, kind });
    }
    Facts { touches }
}

/// `claim released: WHO held since ...` names the seat that held the claim.
fn released_holder(text: &str) -> Option<String> {
    let rest = text.trim().strip_prefix("claim released:")?.trim();
    let who = rest.split_whitespace().next().unwrap_or("");
    named_seat(who)
}

/// Digits of an org stamp, date then clock, so a later note sorts higher.
fn stamp_ord(ts: &str) -> i64 {
    let stamp = ts.trim().trim_matches(|ch| ch == '[' || ch == ']');
    let mut date = 0_i64;
    let mut digits = 0_u32;
    for ch in stamp.chars() {
        if digits == 8 {
            break;
        }
        if ch.is_ascii_digit() {
            date = date * 10 + i64::from(ch as u8 - b'0');
            digits += 1;
        }
    }
    if digits < 8 {
        return 0;
    }
    let clock = stamp.split_whitespace().find_map(|token| {
        let (hour, minute) = token.split_once(':')?;
        let hour = hour.parse::<i64>().ok()?;
        let minute = minute.get(..2)?.parse::<i64>().ok()?;
        Some(hour * 100 + minute)
    });
    date * 10_000 + clock.unwrap_or(0)
}

struct Scope {
    cwd: Option<std::path::PathBuf>,
    /// Projects this seat has filed, claimed, or noted, lowercased.
    projects: BTreeSet<String>,
    /// The roster names this seat and nobody else.
    solo: bool,
    /// The project this seat names and the repo it sits in.
    home: Vec<String>,
}

fn in_home(rec: &IssueRec, scope: &Scope) -> bool {
    let name = rec.project.trim();
    !name.is_empty()
        && scope
            .home
            .iter()
            .any(|project| project.trim().eq_ignore_ascii_case(name))
}

/// How far a ticket's project is from this seat. 0 is the seat's own
/// project or repo, or a project the query names. 1 is a project the
/// seat's facts name, and 2 is any other project. `anchored` says some
/// ticket on the tracker is 0; without one, a project the facts name is 0.
fn project_rank(rec: &IssueRec, scope: &Scope, named: bool, anchored: bool) -> u8 {
    if named || in_home(rec, scope) {
        0
    } else if in_working_project(rec, scope) {
        u8::from(anchored)
    } else {
        2
    }
}

/// The ticket rows [`weigh_projects`] moved out of their groups.
struct Weighed {
    /// This seat's own tickets that miss a query word, outside its nearest
    /// project. They stay ahead of another seat's rows.
    owned: Vec<Hit>,
    /// Other tickets that miss a query word, outside the nearest project.
    far: Vec<Hit>,
}

/// Another project weighs less than this seat's own. Within each group a
/// ticket in a nearer project comes first. A ticket outside the nearest
/// project that misses a query word leaves its group: this seat's own goes
/// after this seat's memories, and any other goes to the end of the page.
///
/// `judge` gives a ticket row its [`project_rank`] and whether it covers
/// every query word. Other rows are left in place.
fn weigh_projects(
    lead: &mut [Hit],
    owned: &mut Vec<Hit>,
    open: &mut Vec<Hit>,
    away: &mut Vec<Hit>,
    judge: impl Fn(&Hit) -> Option<(u8, bool)>,
) -> Weighed {
    let rank = |hit: &Hit| judge(hit).map_or(0, |(rank, _)| rank);
    let strong = |hit: &Hit| judge(hit).is_none_or(|(rank, full)| rank == 0 || full);
    lead.sort_by_key(&rank);
    let split = |rows: &mut Vec<Hit>| {
        let (mut keep, weak): (Vec<Hit>, Vec<Hit>) = rows.drain(..).partition(&strong);
        keep.sort_by_key(&rank);
        *rows = keep;
        weak
    };
    let owned = split(owned);
    let mut far = split(open);
    far.extend(split(away));
    Weighed { owned, far }
}

fn scope_of(tickets: &[Ticket<'_>], here: &Here) -> Scope {
    let mut projects = BTreeSet::new();
    for ticket in tickets {
        if ticket.facts.owns(&here.mine) {
            let name = ticket.rec.project.trim().to_lowercase();
            if !name.is_empty() {
                projects.insert(name);
            }
        }
    }
    let mine = crate::canonical_seat_name(&here.mine);
    let solo = here.roster.len() == 1
        && here
            .roster
            .iter()
            .any(|seat| crate::canonical_seat_name(seat) == mine);
    Scope {
        cwd: here.cwd.clone(),
        projects,
        solo,
        home: here.home.clone(),
    }
}

fn in_cwd(scope: &Scope, path: &std::path::Path) -> bool {
    scope
        .cwd
        .as_ref()
        .is_some_and(|root| path.starts_with(root))
}

fn in_working_project(rec: &IssueRec, scope: &Scope) -> bool {
    let name = rec.project.trim();
    !name.is_empty()
        && scope
            .projects
            .iter()
            .any(|project| project.eq_ignore_ascii_case(name))
}

/// The roster names only this seat, the ticket is in that tracker, and no
/// fact names another seat. That is the untagged ticket that is local.
fn solo_local(rec: &IssueRec, scope: &Scope, facts: &Facts, mine: &str) -> bool {
    scope.solo && in_cwd(scope, &rec.path) && !facts.names_other(mine)
}

fn has_seat_tag(hit: &Hit) -> bool {
    hit.entities.iter().any(|tag| tag.starts_with("seat:"))
}

fn set_seat(hit: &mut Hit, seat: &str) {
    let seat = seat.trim();
    if seat.is_empty() {
        return;
    }
    hit.entities.retain(|tag| !tag.starts_with("seat:"));
    hit.entities.push(format!("seat:{seat}"));
}

fn is_ticket_hit(hit: &Hit) -> bool {
    hit.kind == "ticket"
}

/// The first ticket with each id.
fn by_id<'t, 'r>(tickets: &'t [Ticket<'r>]) -> BTreeMap<&'r str, &'t Ticket<'r>> {
    let mut map = BTreeMap::new();
    for ticket in tickets {
        map.entry(ticket.rec.heading.id.as_str()).or_insert(ticket);
    }
    map
}

/// The facts of the last ticket with each id.
fn facts_by_id<'t>(tickets: &'t [Ticket<'_>]) -> BTreeMap<&'t str, &'t Facts> {
    tickets
        .iter()
        .map(|ticket| (ticket.rec.heading.id.as_str(), &ticket.facts))
        .collect()
}

/// Another project's ticket that only shares words such as docs, release, or video.
fn unrelated_generic(ticket: &Ticket<'_>, scope: &Scope) -> bool {
    if in_working_project(ticket.rec, scope) {
        return false;
    }
    // No project from this seat's own facts: the tracker under the working
    // directory is the repo it is searching, and a ticket there is not an
    // unrelated project.
    if scope.projects.is_empty() && in_cwd(scope, &ticket.rec.path) {
        return false;
    }
    ticket.generic
}

fn hit_for(rec: &IssueRec) -> Hit {
    let id = rec.heading.id.trim();
    Hit {
        id: Some(id.to_string()),
        text: rec.heading.title.trim().to_string(),
        score: intrinsic(Band::Notes),
        kind: "ticket".to_string(),
        ts: None,
        entities: vec![format!("issue:{id}")],
        ballots: None,
        of: None,
        rerank_score: None,
        labels: Vec::new(),
    }
}

/// Pull in a match the encoder did not return: a ticket this seat filed,
/// claimed, or noted, or one in a project those facts name.
fn ensure_matches(hits: &mut Vec<Hit>, tickets: &[Ticket<'_>], mine: &str, scope: &Scope) {
    let have: BTreeSet<String> = hits
        .iter()
        .filter_map(|hit| hit.id.as_deref().map(str::to_string))
        .collect();
    let mut extra = Vec::new();
    for ticket in tickets {
        let id = ticket.rec.heading.id.trim();
        if id.is_empty() || have.contains(id) || !ticket.query_match() {
            continue;
        }
        if ticket.facts.owns(mine) || in_working_project(ticket.rec, scope) {
            extra.push(hit_for(ticket.rec));
        }
    }
    hits.extend(extra);
}

fn stamp_ticket(hit: &mut Hit, rec: &IssueRec, facts: &Facts, scope: &Scope, mine: &str) {
    if !is_ticket_hit(hit) {
        return;
    }
    if facts.owns(mine) || solo_local(rec, scope, facts, mine) {
        set_seat(hit, mine);
        return;
    }
    if has_seat_tag(hit) {
        return;
    }
    if let Some(seat) = facts.other_seat(mine) {
        set_seat(hit, &seat);
    }
}

fn demote_unrelated(rows: &mut Vec<Hit>, index: &BTreeMap<&str, &Ticket<'_>>, scope: &Scope) {
    let mut keep = Vec::new();
    let mut sink = Vec::new();
    for hit in rows.drain(..) {
        let ticket = hit.id.as_deref().and_then(|id| index.get(id).copied());
        let generic =
            ticket.is_some_and(|ticket| is_ticket_hit(&hit) && unrelated_generic(ticket, scope));
        if generic {
            sink.push(hit);
        } else {
            keep.push(hit);
        }
    }
    keep.extend(sink);
    *rows = keep;
}

/// Exact and near titles stay first. Then a ticket this seat filed, claimed,
/// or noted, when the title, body, or notes match. The newest of those facts
/// is first. Then this seat's memories, and a ticket in the one-seat roster.
/// Then tickets nobody's facts name. Another seat's rows are last.
///
/// An untagged ticket is not put in this seat's group. It sits before a row
/// that names another seat, so the header stays true, and a project this
/// seat works in still outranks an unrelated ticket that only shares a
/// generic word. A farther project's ticket comes after a nearer one, and
/// one that misses a query word moves down; see [`weigh_projects`].
fn place_rows(hits: &mut Vec<Hit>, tickets: &[Ticket<'_>], here: impl FnOnce() -> Here) {
    if tickets.is_empty() {
        return;
    }
    let here = here();
    let mine = here.mine.as_str();
    let scope = scope_of(tickets, &here);
    ensure_matches(hits, tickets, mine, &scope);
    if hits.is_empty() {
        return;
    }
    let index = by_id(tickets);
    let known = facts_by_id(tickets);
    for hit in hits.iter_mut() {
        let Some(id) = hit.id.as_deref() else {
            continue;
        };
        let Some(ticket) = index.get(id).copied() else {
            continue;
        };
        let Some(facts) = known.get(id).copied() else {
            continue;
        };
        stamp_ticket(hit, ticket.rec, facts, &scope, mine);
    }
    let mut lead = Vec::new();
    let mut owned = Vec::new();
    let mut home = Vec::new();
    let mut open = Vec::new();
    let mut away = Vec::new();
    for hit in hits.drain(..) {
        let ticket = hit.id.as_deref().and_then(|id| index.get(id).copied());
        let facts = hit.id.as_deref().and_then(|id| known.get(id).copied());
        if ticket.is_some_and(Ticket::lead_title) {
            lead.push(hit);
        } else if ticket.is_some_and(|ticket| {
            facts.is_some_and(|facts| facts.owns(mine)) && ticket.query_match()
        }) {
            owned.push(hit);
        } else if is_ticket_hit(&hit) {
            if crate::other_seat(&hit.entities, mine).is_some() {
                away.push(hit);
            } else if ticket.is_some_and(|ticket| {
                facts.is_some_and(|facts| solo_local(ticket.rec, &scope, facts, mine))
            }) {
                home.push(hit);
            } else {
                open.push(hit);
            }
        } else if crate::other_seat(&hit.entities, mine).is_some() {
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
                .map(|facts| facts.recency(mine))
                .unwrap_or(0)
        };
        let cover = |hit: &Hit| {
            hit.id
                .as_deref()
                .and_then(|id| index.get(id))
                .map_or(0, |ticket| ticket.cover)
        };
        recency(right)
            .cmp(&recency(left))
            .then_with(|| cover(right).cmp(&cover(left)))
            .then_with(|| left.id.cmp(&right.id))
    });
    demote_unrelated(&mut open, &index, &scope);
    demote_unrelated(&mut away, &index, &scope);
    let anchored = tickets
        .iter()
        .any(|ticket| ticket.named || in_home(ticket.rec, &scope));
    let weighed = if anchored || !scope.projects.is_empty() {
        weigh_projects(&mut lead, &mut owned, &mut open, &mut away, |hit| {
            let ticket = hit.id.as_deref().and_then(|id| index.get(id).copied())?;
            is_ticket_hit(hit).then(|| {
                (
                    project_rank(ticket.rec, &scope, ticket.named, anchored),
                    ticket.full,
                )
            })
        })
    } else {
        Weighed {
            owned: Vec::new(),
            far: Vec::new(),
        }
    };
    let mut ordered = Vec::with_capacity(
        lead.len()
            + owned.len()
            + home.len()
            + weighed.owned.len()
            + open.len()
            + away.len()
            + weighed.far.len(),
    );
    ordered.extend(lead);
    ordered.extend(owned);
    ordered.extend(home);
    ordered.extend(weighed.owned);
    ordered.extend(open);
    ordered.extend(away);
    ordered.extend(weighed.far);
    *hits = ordered;
}

#[cfg(test)]
fn band(query: &str, title: &str, notes: &str, corpus: &[&str]) -> Option<Band> {
    let bags: Vec<TermBag> = corpus.iter().map(|title| TermBag::of(title)).collect();
    let corpus: Vec<&TermBag> = bags.iter().collect();
    let asked = Asked::new(query, &corpus);
    let title = title.trim();
    let blob = TermBag::of(&format!("{title}\n{notes}"));
    band_with(&asked, title, &TermBag::of(title), &blob)
}

/// `title` is trimmed. `title_bag` is its terms, and `blob` is the terms
/// of the title, body and notes.
fn band_with(asked: &Asked, title: &str, title_bag: &TermBag, blob: &impl Have) -> Option<Band> {
    if asked.text.is_empty() || title.is_empty() {
        return None;
    }
    let title_norm = norm(title);
    if asked.norm == title_norm {
        return Some(Band::Exact);
    }
    if near(asked, title, &title_norm) {
        return Some(Band::Near);
    }
    if asked.words.is_empty() {
        return None;
    }
    // One query word is the subject. Two shared words name the ticket.
    // A longer query that shares only one common word does not: `update`
    // is in many titles, and `kiln` is not.
    let shared: Vec<usize> = (0..asked.words.len())
        .filter(|&index| asked.word_terms[index].any_in(title_bag))
        .collect();
    let title_hit = if asked.words.len() == 1 {
        !shared.is_empty()
    } else if shared.len() >= 2 {
        true
    } else if let Some(&index) = shared.first() {
        !asked.common[index]
    } else {
        false
    };
    if title_hit {
        return Some(Band::Title);
    }
    asked
        .word_terms
        .iter()
        .all(|terms| terms.any_in(blob))
        .then_some(Band::Notes)
}

/// Words that label a ticket without naming its subject. One of them,
/// shared with a longer query, is not a title match.
const COMMON_TITLE: &[&str] = &[
    "add", "change", "fix", "make", "new", "note", "set", "update", "use", "work", "issue",
    "ticket", "file", "search", "find", "show", "list", "run", "check", "help", "need", "thing",
    "item", "task", "page", "line", "text", "name", "type", "way", "time", "user", "data", "code",
    "info", "please", "also", "just", "more",
];

/// A word on [`COMMON_TITLE`], or a form of one. A word in every title is
/// common too: [`Asked::new`] checks that against the corpus once.
fn listed_common(word: &str) -> bool {
    COMMON_TITLE
        .iter()
        .any(|item| word.eq_ignore_ascii_case(item) || crate::word_rank::shares_term(word, item))
}

/// Same content words, one content word apart, or a short typo.
/// The two norms are [`norm`] of the query and the title.
///
/// Each title word is read once, so a long query against a long title is
/// one pass over the title and not one per pair of words.
fn near(asked: &Asked, title: &str, title_norm: &str) -> bool {
    let title_words = title_words(title);
    if asked.words.is_empty() || title_words.is_empty() {
        return false;
    }
    if same_words(asked, &title_words) || one_word_apart(asked, &title_words) {
        return true;
    }
    typo_near(&asked.norm, title_norm)
}

/// A content word of a title, with its own terms and the terms it holds.
struct TitleWord<'t> {
    word: &'t str,
    terms: QueryTerms,
    bag: TermBag,
}

/// The [`content_words`] of `title`, each analyzed once.
fn title_words(title: &str) -> Vec<TitleWord<'_>> {
    title
        .split_whitespace()
        .filter_map(|word| {
            let terms = QueryTerms::of(word);
            if terms.is_empty() {
                return None;
            }
            let bag = TermBag::of(word);
            terms.any_in(&bag).then_some(TitleWord { word, terms, bag })
        })
        .collect()
}

/// Every query word is covered by some title word, and the counts match.
fn same_words(asked: &Asked, title: &[TitleWord<'_>]) -> bool {
    asked.words.len() == title.len()
        && asked
            .word_terms
            .iter()
            .all(|terms| title.iter().any(|have| terms.any_in(&have.bag)))
}

/// The shorter side has three or more words, the longer one more, and
/// every shorter word is in the longer side.
fn one_word_apart(asked: &Asked, title: &[TitleWord<'_>]) -> bool {
    let (query, titled) = (asked.words.len(), title.len());
    let (short, long) = if query <= titled {
        (query, titled)
    } else {
        (titled, query)
    };
    if short < 3 || long - short != 1 {
        return false;
    }
    if query <= titled {
        let joined: Vec<&str> = title.iter().map(|word| word.word).collect();
        let blob = TermBag::of(&joined.join(" "));
        asked.word_terms.iter().all(|terms| terms.any_in(&blob))
    } else {
        title.iter().all(|word| word.terms.any_in(&asked.joined))
    }
}

/// Both texts are already [`norm`]ed.
fn typo_near(query: &str, title: &str) -> bool {
    let (n, m) = (query.chars().count(), title.chars().count());
    let short = n.min(m);
    short >= 12 && n.abs_diff(m) <= 2 && edit_within(query, title, 2)
}

fn norm(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|ch| ch.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn content_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|word| crate::word_rank::shares_term(word, word))
        .map(str::to_string)
        .collect()
}

fn edit_within(left: &str, right: &str, max: usize) -> bool {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let n = left.len();
    let m = right.len();
    if n.abs_diff(m) > max || n > 512 || m > 512 {
        return false;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        let mut row_min = curr[0];
        for j in 1..=m {
            let cost = usize::from(left[i - 1] != right[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(curr[j]);
        }
        if row_min > max {
            return false;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m] <= max
}

#[cfg(test)]
mod tests {
    use super::{band, stamp_ord, Band};

    const TITLE: &str = "GPRidge narrated video set";
    const NOTES: &str = "Milestone: the ridge walk is filmed.";

    fn at(query: &str, title: &str, notes: &str) -> Option<Band> {
        band(query, title, notes, &[title])
    }

    #[test]
    fn a_title_match_outranks_notes_and_an_unrelated_query_misses() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(at(TITLE, TITLE, NOTES), Some(Band::Exact));
        assert_eq!(
            at("gpridge narrated video set", TITLE, NOTES),
            Some(Band::Exact)
        );
        assert_eq!(
            at("the GPRidge narrated video set", TITLE, NOTES),
            Some(Band::Near)
        );
        assert_eq!(at("GPRidge narrated video", TITLE, NOTES), Some(Band::Near));
        assert_eq!(at("narrated video", TITLE, NOTES), Some(Band::Title));
        assert_eq!(at("GPRidge", TITLE, NOTES), Some(Band::Title));
        assert_eq!(
            at("GPRidge video narration", TITLE, NOTES),
            Some(Band::Near)
        );
        assert_eq!(
            at("gpridge VIDEO Narration", TITLE, NOTES),
            Some(Band::Near)
        );
        assert_eq!(at("narrate", TITLE, NOTES), Some(Band::Title));
        assert_eq!(at("videos", TITLE, NOTES), Some(Band::Title));
        assert_eq!(at("kiln", "Fire the kiln", ""), Some(Band::Title));
        assert_eq!(at("milestone filmed", TITLE, NOTES), Some(Band::Notes));
        assert_eq!(at("zzzznotaword", TITLE, NOTES), None);
        assert_eq!(at("the", TITLE, NOTES), None);
    }

    /// Titles a person would file. `update`, `note` and `fix` show up on
    /// more than one, and they do not name the work.
    #[test]
    fn a_single_common_word_is_not_a_title_match() {
        let _env_read = crate::test_env_read_lock();
        let corpus = [
            "Update the mailbox index",
            "Update the docs site",
            "Add a note to the readme",
            "Fix the parser timeout",
            "Fire the kiln overnight",
            "GPRidge narrated video set",
        ];
        let hit = |query: &str, title: &str| band(query, title, "", &corpus);
        assert_eq!(hit("update the note about search", corpus[0]), None);
        assert_eq!(hit("update the note about search", corpus[2]), None);
        assert_eq!(hit("please fix the glaze", corpus[3]), None);
        assert_eq!(hit("kiln overnight", corpus[4]), Some(Band::Title));
        assert_eq!(
            hit("update the kiln temperature", corpus[4]),
            Some(Band::Title)
        );
        assert_eq!(hit("update the kiln temperature", corpus[0]), None);
        assert_eq!(hit("parser timeout", corpus[3]), Some(Band::Title));
        assert_eq!(hit("GPRidge", corpus[5]), Some(Band::Title));
        assert_eq!(hit("update", corpus[0]), Some(Band::Title));
    }

    #[test]
    fn a_later_note_stamp_sorts_above_an_older_one() {
        let _env_read = crate::test_env_read_lock();
        assert!(stamp_ord("[2026-11-16 Mon 18:30]") > stamp_ord("[2026-11-01 Sun 09:00]"));
        assert_eq!(stamp_ord("not a stamp"), 0);
        assert_eq!(stamp_ord(""), 0);
    }
}
