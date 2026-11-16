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

use packset_client::Hit;
use std::collections::BTreeSet;
use vissue_core::views::IssueRec;

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

/// Insert tracker tickets that match `query`.
///
/// Exact and near titles go in front of the memories already ranked, so
/// they stay first after the limit cut. Other title matches, then notes
/// matches, follow those memories. `hits` is unchanged when the tracker
/// has no match, or when it cannot be read.
pub fn merge(hits: &mut Vec<Hit>, query: &str) {
    let mut found = collect(query);
    if found.is_empty() {
        return;
    }
    let have: BTreeSet<String> = hits.iter().filter_map(|hit| hit.id.clone()).collect();
    found.retain(|row| row.hit.id.as_ref().is_none_or(|id| !have.contains(id)));
    if found.is_empty() {
        return;
    }
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

fn collect(query: &str) -> Vec<Found> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let rows = tickets();
    let titles: Vec<&str> = rows
        .iter()
        .map(|rec| rec.heading.title.trim())
        .filter(|title| !title.is_empty())
        .collect();
    let mut found = Vec::new();
    for rec in &rows {
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

fn tickets() -> Vec<IssueRec> {
    let Ok(layout) = vissue_core::Layout::resolve(None, None) else {
        return Vec::new();
    };
    if layout.require_tracker().is_err() {
        return Vec::new();
    }
    let layouts = vissue_core::Router::load(layout.clone())
        .map(|router| {
            router
                .unique_layouts()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| vec![layout]);
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for layout in &layouts {
        let Ok(rows) = vissue_core::catalog::load_recs(layout) else {
            continue;
        };
        for rec in rows {
            if rec.heading.id.is_empty() {
                continue;
            }
            if seen.insert(rec.heading.id.clone()) {
                out.push(rec);
            }
        }
    }
    out
}

fn notes_text(heading: &vissue_core::IssueHeading) -> String {
    let mut notes = String::new();
    push_note(&mut notes, heading.body.trim());
    for entry in &heading.logbook {
        if entry.is_bookkeeping() {
            continue;
        }
        if let Some(text) = crate::logbook_text(entry) {
            push_note(&mut notes, text.trim());
        }
    }
    for drawer in &heading.extra_drawers {
        push_note(&mut notes, drawer.trim());
    }
    notes
}

fn push_note(notes: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !notes.is_empty() {
        notes.push('\n');
    }
    notes.push_str(text);
}

fn band(query: &str, title: &str, notes: &str, corpus: &[&str]) -> Option<Band> {
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
    // One query word is the subject. Two shared words name the ticket.
    // A longer query that shares only one common word does not: `update`
    // is in many titles, and `kiln` is not.
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

/// Words that label a ticket without naming its subject. One of them,
/// shared with a longer query, is not a title match.
const COMMON_TITLE: &[&str] = &[
    "add", "change", "fix", "make", "new", "note", "set", "update", "use", "work", "issue",
    "ticket", "file", "search", "find", "show", "list", "run", "check", "help", "need", "thing",
    "item", "task", "page", "line", "text", "name", "type", "way", "time", "user", "data", "code",
    "info", "please", "also", "just", "more",
];

fn common_title_word(word: &str, corpus: &[&str]) -> bool {
    if COMMON_TITLE
        .iter()
        .any(|item| word.eq_ignore_ascii_case(item) || crate::word_rank::shares_term(word, item))
    {
        return true;
    }
    let n = corpus.len();
    n >= 2 && corpus.iter().filter(|title| covers(word, title)).count() == n
}

/// Same content words, one content word apart, or a short typo.
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

fn covers_all(words: &[String], text: &str) -> bool {
    !words.is_empty() && words.iter().all(|word| covers(word, text))
}

fn covers(word: &str, text: &str) -> bool {
    crate::word_rank::shares_term(word, text)
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
    use super::{band, Band};

    const TITLE: &str = "GPRidge narrated video set";
    const NOTES: &str = "Milestone: the ridge walk is filmed.";

    fn at(query: &str, title: &str, notes: &str) -> Option<Band> {
        band(query, title, notes, &[title])
    }

    #[test]
    fn a_title_match_outranks_notes_and_an_unrelated_query_misses() {
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
}
