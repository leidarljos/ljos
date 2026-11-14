//! Word ranking for `ljos search` when the encoder is not what ordered the rows.
//!
//! Two paths land here. The local snapshot used to score a row by the fraction
//! of query words that occur in its text, with `_` and `-` kept apart only by
//! the alphanumeric split, no rarity, and a tie broken by id. A pack that
//! answers `engine: linear` (no encoder) fuses a prefix scan with BM25+. That
//! scan keeps `list_scans` as one token, scores a four-letter prefix almost
//! like an exact word, and ljos then put this seat's rows ahead of a closer
//! row from anyone else.
//!
//! The score here is BM25 (`k1 = 1.2`, `b = 0.75`) over identifier-aware
//! tokens. A title or a tag counts twice a body word. Query words that sit
//! next to each other add a proximity bonus. This seat's own row is multiplied
//! by [`SEAT_BOOST`], which is not enough to pass a clearly closer row.
//! A query whose best row does not cover enough of its words is not printed.

use packset_client::Hit;

/// How much a title or a tag counts, against one for the body.
const FIELD_WEIGHT: f64 = 2.0;

/// This seat's row, as a multiplier. Twenty percent keeps a one-word match on
/// this seat ahead of the same word in a slightly shorter row from another
/// seat. It does not pass a row that covers more of the query.
const SEAT_BOOST: f64 = 1.20;

/// BM25 term-frequency saturation. Robertson and Zaragoza, 2009.
const K1: f64 = 1.2;

/// How much document length counts. 0 is none, 1 is full.
const B: f64 = 0.75;

/// Function words. A coding noun such as `test` or `error` is not one:
/// those are what a query is often about.
const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "been", "being", "but", "by", "can", "could", "did",
    "do", "does", "for", "from", "had", "has", "have", "if", "in", "into", "is", "it", "its",
    "just", "me", "more", "most", "my", "no", "not", "of", "on", "or", "our", "please", "should",
    "so", "than", "that", "the", "their", "them", "then", "there", "these", "this", "those", "to",
    "too", "was", "we", "were", "what", "when", "where", "which", "who", "will", "with", "would",
    "you", "your",
];

/// What one search row is ranked on. `own` is this seat, or a row with no seat.
pub struct Doc<'a> {
    pub id: &'a str,
    pub own: bool,
    pub title: &'a str,
    pub tags: &'a str,
    pub body: &'a str,
}

/// How [`rank`] left the corpus.
#[derive(Debug, PartialEq)]
pub enum Ranked {
    /// Best first. `index` is the position in the slice passed to [`rank`].
    Hits(Vec<Scored>),
    /// Something shared a word, and nothing covered enough of the query.
    NoClose,
    /// Nothing shared a content word, or the query had none.
    None,
}

/// One row that cleared the floor, with the score the page prints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    pub index: usize,
    pub score: f64,
}

/// How [`order_hits`] left a search page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordList {
    /// Rows were reordered and weak ones removed.
    Kept,
    /// The page had rows, and none cleared the floor.
    NoClose,
    /// Nothing to print. The usual no-hits line applies.
    None,
}

#[derive(Clone)]
struct Field {
    /// Content terms in reading order, with the word slot they came from.
    /// A stop word consumes a slot and is not stored, so it breaks a phrase.
    seq: Vec<(String, u32)>,
    /// Terms BM25 counts, including the joined form of an identifier.
    bag: Vec<String>,
}

struct Prepared {
    id: String,
    own: bool,
    body: Field,
    title: Field,
    tags: Field,
    /// Every distinct term in any field.
    have: std::collections::BTreeSet<String>,
}

/// The line when rows existed and none cleared the floor.
#[must_use]
pub fn no_close_line(query: &str) -> String {
    let query = query.trim();
    let shown = quote_query(query);
    match broader_command(query) {
        Some(cmd) => format!("No close matches for {shown}. Try a broader query: `{cmd}`."),
        None => format!("No close matches for {shown}. Try a broader query."),
    }
}

/// Whether `text` shares one content word with `query`, after identifier
/// splits. The local snapshot uses this to keep a row the old overlap missed,
/// such as `sha1` against `SHA-1`.
#[must_use]
pub fn shares_term(query: &str, text: &str) -> bool {
    let concepts = concepts(query);
    if concepts.is_empty() {
        return false;
    }
    let field = analyze(text);
    let have: std::collections::BTreeSet<String> = field.bag.into_iter().collect();
    concepts
        .iter()
        .any(|concept| concept_covered(concept, &have))
}

/// Order `docs` for `query`. See the module note for what the score is.
#[must_use]
pub fn rank(docs: &[Doc<'_>], query: &str) -> Ranked {
    let concepts = concepts(query);
    if concepts.is_empty() || docs.is_empty() {
        return Ranked::None;
    }
    let prepared: Vec<Prepared> = docs.iter().map(prepare).collect();
    let df = document_frequency(&prepared, &concepts);
    let n = prepared.len() as f64;
    let avgdl = average_body_len(&prepared);
    let mut any = false;
    let mut close: Vec<(usize, f64, usize, f64, usize)> = Vec::new();
    let need = needed(concepts.len());
    for (index, doc) in prepared.iter().enumerate() {
        let covered = concepts
            .iter()
            .filter(|concept| concept_covered(concept, &doc.have))
            .count();
        if covered == 0 {
            continue;
        }
        any = true;
        if covered < need {
            continue;
        }
        let body = bm25(&doc.body.bag, doc.body.bag.len(), &concepts, &df, n, avgdl);
        let title = bm25(
            &doc.title.bag,
            doc.title.bag.len(),
            &concepts,
            &df,
            n,
            avgdl,
        );
        let tags = bm25(&doc.tags.bag, doc.tags.bag.len(), &concepts, &df, n, avgdl);
        let phrase = phrase_bonus(&doc.body, &doc.title, &concepts, &df, n);
        let mut score = body + FIELD_WEIGHT * title + FIELD_WEIGHT * tags + phrase;
        // A title that names half the query must not outrank a row that names
        // all of it. Squaring the fraction is what makes a partial title lose
        // to a complete sentence: the field weight alone did not.
        let cover = covered as f64 / concepts.len() as f64;
        score *= cover * cover;
        if doc.own {
            score *= SEAT_BOOST;
        }
        close.push((index, score, covered, phrase, doc.body.bag.len()));
    }
    if close.is_empty() {
        return if any { Ranked::NoClose } else { Ranked::None };
    }
    close.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then(b.2.cmp(&a.2))
            .then(b.3.total_cmp(&a.3))
            .then(b.4.cmp(&a.4))
            .then_with(|| prepared[a.0].id.cmp(&prepared[b.0].id))
    });
    Ranked::Hits(
        close
            .into_iter()
            .map(|(index, score, _, _, _)| Scored { index, score })
            .collect(),
    )
}

/// Reorder `hits` for a word-ranked search and drop what does not clear the
/// floor. Notes, attempts and logs stay out, as they do on the other path.
/// The printed score is this ranker's score.
pub fn order_hits(hits: &mut Vec<Hit>, query: &str, attempts: bool) -> WordList {
    hits.retain(|hit| !super::search_skip_kind(hit, attempts) && !hit.text.trim().is_empty());
    if hits.is_empty() {
        return WordList::None;
    }
    let mine = super::seat_name();
    let titles: Vec<String> = hits.iter().map(take_title).collect();
    let tags: Vec<String> = hits.iter().map(tag_text).collect();
    let docs: Vec<Doc<'_>> = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| Doc {
            id: hit.id.as_deref().unwrap_or(""),
            own: !super::foreign_memory(hit, &mine),
            title: &titles[i],
            tags: &tags[i],
            body: hit.text.as_str(),
        })
        .collect();
    match rank(&docs, query) {
        Ranked::Hits(mut scored) => {
            // Equal word scores keep the pack's own number, so a preference
            // the writer scored above a lesson stays above it.
            let incoming: Vec<f64> = hits.iter().map(|hit| hit.score).collect();
            scored.sort_by(|a, b| {
                b.score
                    .total_cmp(&a.score)
                    .then(incoming[b.index].total_cmp(&incoming[a.index]))
                    .then(a.index.cmp(&b.index))
            });
            let mut taken: Vec<Option<Hit>> = hits.drain(..).map(Some).collect();
            for row in &scored {
                if let Some(hit) = taken[row.index].as_mut() {
                    hit.score = row.score;
                    hit.entities.retain(|tag| !tag.starts_with("title:"));
                }
            }
            hits.extend(scored.into_iter().filter_map(|row| taken[row.index].take()));
            WordList::Kept
        }
        Ranked::NoClose => {
            hits.clear();
            WordList::NoClose
        }
        Ranked::None => {
            hits.clear();
            WordList::None
        }
    }
}

fn take_title(hit: &Hit) -> String {
    hit.entities
        .iter()
        .find_map(|tag| tag.strip_prefix("title:"))
        .unwrap_or("")
        .to_string()
}

fn tag_text(hit: &Hit) -> String {
    hit.entities
        .iter()
        .filter_map(|tag| {
            if let Some(rest) = tag.strip_prefix("tag:") {
                return (!rest.is_empty()).then(|| rest.to_string());
            }
            if tag.starts_with("title:") || tag.contains(':') {
                return None;
            }
            Some(tag.clone())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_query(query: &str) -> String {
    if query.is_empty()
        || query
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '"' | '`' | '\\'))
    {
        "this query".to_string()
    } else if query.contains(char::is_whitespace) {
        format!("\"{query}\"")
    } else {
        query.to_string()
    }
}

/// The longest written word, when the query has more than one. That is the
/// broader search: fewer words, the rarest-looking one kept.
fn broader_command(query: &str) -> Option<String> {
    let words = concepts(query);
    if words.len() < 2 {
        return None;
    }
    let best = words.iter().max_by_key(|word| word.chars().count())?;
    let safe = best
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    if !safe {
        return None;
    }
    Some(format!("ljos search {best}"))
}

/// How many of `n` concepts a row must cover. Two words must both be there.
/// A longer query needs half, rounded up, and at least two.
fn needed(n: usize) -> usize {
    if n <= 2 {
        n.max(1)
    } else {
        n.div_ceil(2).max(2)
    }
}

/// Written query words that name something. Order is the order typed.
fn concepts(query: &str) -> Vec<String> {
    query
        .split(|c: char| c.is_whitespace())
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && !is_sep(c)))
        .filter(|word| keep_concept(word))
        .map(str::to_lowercase)
        .collect()
}

fn keep_concept(token: &str) -> bool {
    if token.is_empty() || token.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let letters = token.chars().filter(|c| c.is_alphabetic()).count();
    if letters >= 3 {
        let flat = token
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>();
        return !is_stop(&flat);
    }
    letters >= 2 && token.chars().any(|c| c.is_ascii_digit())
}

fn is_sep(c: char) -> bool {
    matches!(c, '_' | '-' | '.' | '/')
}

fn is_stop(word: &str) -> bool {
    STOP.contains(&word)
}

fn stem(word: &str) -> String {
    let word = word.to_lowercase();
    if word.chars().any(|c| !c.is_ascii_alphabetic()) {
        return word;
    }
    let word = super::singular(&word);
    if word.len() > 5 && word.ends_with("ing") {
        let stem = &word[..word.len() - 3];
        if stem.len() >= 3 {
            return stem.to_string();
        }
    }
    if word.len() > 4 && word.ends_with("ed") {
        let stem = &word[..word.len() - 2];
        if stem.len() >= 3 {
            return stem.to_string();
        }
    }
    word
}

struct Concept {
    /// Stemmed parts. Every one must occur, unless `joined` itself occurs.
    parts: Vec<String>,
    joined: String,
    joined_stems: String,
}

fn concept_of(written: &str) -> Concept {
    let (joined, parts) = split_run(written);
    let stems: Vec<String> = parts.iter().map(|part| stem(part)).collect();
    let joined_stems = stems.concat();
    Concept {
        parts: stems
            .into_iter()
            .filter(|part| part.len() >= 2 && !is_stop(part))
            .collect(),
        joined,
        joined_stems,
    }
}

fn concept_covered(written: &str, have: &std::collections::BTreeSet<String>) -> bool {
    let concept = concept_of(written);
    if !concept.joined.is_empty() && have.contains(&concept.joined) {
        return true;
    }
    if !concept.joined_stems.is_empty()
        && concept.joined_stems != concept.joined
        && have.contains(&concept.joined_stems)
    {
        return true;
    }
    !concept.parts.is_empty() && concept.parts.iter().all(|part| have.contains(part))
}

fn prepare(doc: &Doc<'_>) -> Prepared {
    let body = analyze(doc.body);
    let title = analyze(doc.title);
    let tags = analyze(doc.tags);
    let mut have = std::collections::BTreeSet::new();
    for field in [&body, &title, &tags] {
        have.extend(field.bag.iter().cloned());
    }
    Prepared {
        id: doc.id.to_string(),
        own: doc.own,
        body,
        title,
        tags,
        have,
    }
}

fn analyze(text: &str) -> Field {
    let mut seq = Vec::new();
    let mut bag = Vec::new();
    let mut slot = 0u32;
    for raw in text.split(|c: char| c.is_whitespace()) {
        let raw = raw.trim_matches(|c: char| !c.is_alphanumeric() && !is_sep(c));
        if raw.is_empty() {
            continue;
        }
        let (joined, parts) = split_run(raw);
        let content: Vec<String> = parts
            .iter()
            .map(|part| stem(part))
            .filter(|part| part.len() >= 2 && !is_stop(part))
            .collect();
        if content.is_empty() && joined.chars().filter(|c| c.is_alphabetic()).count() < 3 {
            slot = slot.saturating_add(1);
            continue;
        }
        if !joined.is_empty() && (parts.len() > 1 || joined.chars().any(|c| c.is_ascii_digit())) {
            bag.push(joined);
        }
        let stems_joined = content.concat();
        if content.len() > 1 && stems_joined.chars().count() >= 4 {
            bag.push(stems_joined);
        }
        for term in &content {
            seq.push((term.clone(), slot));
            bag.push(term.clone());
        }
        slot = slot.saturating_add(1);
    }
    // Adjacent content words written as two tokens (`list scans`) share the
    // joined form of `list_scans`.
    let mut extra = Vec::new();
    for pair in seq.windows(2) {
        if pair[1].1 == pair[0].1.saturating_add(1) {
            let joined = format!("{}{}", pair[0].0, pair[1].0);
            if joined.chars().count() >= 4 {
                extra.push(joined);
            }
        }
    }
    bag.extend(extra);
    Field { seq, bag }
}

/// `(alphanumeric join, raw parts before stemming)`.
fn split_run(run: &str) -> (String, Vec<String>) {
    let joined: String = run
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let mut parts = Vec::new();
    for segment in run.split(is_sep) {
        if segment.is_empty() {
            continue;
        }
        for piece in split_camel_digit(segment) {
            if !piece.is_empty() {
                parts.push(piece.to_lowercase());
            }
        }
    }
    if parts.is_empty() && !joined.is_empty() {
        parts.push(joined.clone());
    }
    (joined, parts)
}

fn split_camel_digit(seg: &str) -> Vec<String> {
    let chars: Vec<char> = seg.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    for index in 1..chars.len() {
        let prev = chars[index - 1];
        let here = chars[index];
        let next = chars.get(index + 1).copied();
        let camel = prev.is_lowercase() && here.is_uppercase();
        let acronym = index.saturating_sub(start) > 1
            && prev.is_uppercase()
            && here.is_uppercase()
            && next.is_some_and(char::is_lowercase);
        let digit_edge = (prev.is_ascii_alphabetic() && here.is_ascii_digit())
            || (prev.is_ascii_digit() && here.is_ascii_alphabetic());
        if camel || acronym || digit_edge {
            out.push(chars[start..index].iter().collect());
            start = index;
        }
    }
    out.push(chars[start..].iter().collect());
    out
}

fn document_frequency(
    docs: &[Prepared],
    concepts: &[String],
) -> std::collections::HashMap<String, usize> {
    let mut terms = std::collections::BTreeSet::new();
    for written in concepts {
        let concept = concept_of(written);
        if !concept.joined.is_empty() {
            terms.insert(concept.joined);
        }
        if !concept.joined_stems.is_empty() {
            terms.insert(concept.joined_stems);
        }
        terms.extend(concept.parts);
    }
    let mut df = std::collections::HashMap::new();
    for term in terms {
        let n = docs.iter().filter(|doc| doc.have.contains(&term)).count();
        df.insert(term, n);
    }
    df
}

fn average_body_len(docs: &[Prepared]) -> f64 {
    if docs.is_empty() {
        return 1.0;
    }
    let total: usize = docs.iter().map(|doc| doc.body.bag.len().max(1)).sum();
    (total as f64 / docs.len() as f64).max(1.0)
}

fn idf(n: f64, df: usize) -> f64 {
    if df == 0 {
        return 0.0;
    }
    ((n - df as f64 + 0.5) / (df as f64 + 0.5) + 1.0).ln()
}

fn bm25(
    bag: &[String],
    len: usize,
    concepts: &[String],
    df: &std::collections::HashMap<String, usize>,
    n: f64,
    avgdl: f64,
) -> f64 {
    if bag.is_empty() || len == 0 {
        return 0.0;
    }
    let mut freq: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    for term in bag {
        *freq.entry(term.as_str()).or_insert(0.0) += 1.0;
    }
    let dl = len as f64;
    let norm = 1.0 - B + B * dl / avgdl;
    let mut score = 0.0;
    let mut seen = std::collections::BTreeSet::new();
    for written in concepts {
        let concept = concept_of(written);
        let mut term_score = 0.0;
        for term in std::iter::once(concept.joined)
            .chain(std::iter::once(concept.joined_stems))
            .chain(concept.parts.into_iter())
        {
            if term.len() < 2 || !seen.insert(term.clone()) {
                continue;
            }
            let Some(count) = freq.get(term.as_str()).copied() else {
                continue;
            };
            let docs_with = df.get(&term).copied().unwrap_or(0);
            if docs_with == 0 || count == 0.0 {
                continue;
            }
            let weight = idf(n, docs_with);
            let tf = count * (K1 + 1.0) / (count + K1 * norm);
            term_score += weight * tf;
        }
        score += term_score;
    }
    score
}

fn phrase_bonus(
    body: &Field,
    title: &Field,
    concepts: &[String],
    df: &std::collections::HashMap<String, usize>,
    n: f64,
) -> f64 {
    if concepts.len() < 2 {
        return 0.0;
    }
    let mut bonus = 0.0;
    for pair in concepts.windows(2) {
        let left = concept_of(&pair[0]);
        let right = concept_of(&pair[1]);
        let near = field_near(&body.seq, &left, &right) || field_near(&title.seq, &left, &right);
        if !near {
            continue;
        }
        let idf_left = concept_idf(&left, df, n);
        let idf_right = concept_idf(&right, df, n);
        bonus += 0.35 * idf_left.min(idf_right);
    }
    bonus
}

fn concept_idf(concept: &Concept, df: &std::collections::HashMap<String, usize>, n: f64) -> f64 {
    let mut best = 0.0_f64;
    for term in concept
        .parts
        .iter()
        .chain(std::iter::once(&concept.joined))
        .chain(std::iter::once(&concept.joined_stems))
    {
        if let Some(docs_with) = df.get(term).copied().filter(|docs| *docs > 0) {
            best = best.max(idf(n, docs_with));
        }
    }
    best
}

fn field_near(seq: &[(String, u32)], left: &Concept, right: &Concept) -> bool {
    for (term, slot) in seq {
        if !term_in_concept(term, left) {
            continue;
        }
        if seq.iter().any(|(other, other_slot)| {
            other_slot.abs_diff(*slot) > 0
                && other_slot.abs_diff(*slot) <= 3
                && term_in_concept(other, right)
        }) {
            return true;
        }
    }
    false
}

fn term_in_concept(term: &str, concept: &Concept) -> bool {
    concept.parts.iter().any(|part| part == term)
        || concept.joined == term
        || concept.joined_stems == term
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bag(text: &str) -> std::collections::BTreeSet<String> {
        analyze(text).bag.into_iter().collect()
    }

    #[test]
    fn an_identifier_matches_its_words_its_digits_and_its_stem() {
        let scans = bag("escapees list_scans returns uppercase keys");
        let words = bag("list scans");
        assert!(
            scans.contains("listscans") || scans.contains("listscan"),
            "{scans:?}"
        );
        assert!(
            words.contains("list") && words.contains("scan"),
            "{words:?}"
        );
        assert!(shares_term(
            "list scans",
            "escapees list_scans returns uppercase keys"
        ));
        assert!(shares_term(
            "sha1",
            "git am fails when SHA-1 information is lacking"
        ));
        assert!(shares_term(
            "http2 preface",
            "Send the HTTP/2 preface first."
        ));
        assert!(shares_term(
            "failing integration tests",
            "The integration tests failed after the rename."
        ));
        assert!(!shares_term(
            "xylophone quadrature",
            "a ferry timetable for the docks"
        ));
    }

    /// The doc samples print these scores for a one-row word search.
    #[test]
    fn one_stored_lesson_scores_what_the_docs_print() {
        let score = |body: &str, query: &str| -> f64 {
            let docs = [Doc {
                id: "a",
                own: true,
                title: "",
                tags: "",
                body,
            }];
            match rank(&docs, query) {
                Ranked::Hits(rows) => rows[0].score,
                other => panic!("{query} was not a hit: {other:?}"),
            }
        };
        let near = |got: f64, want: f64, label: &str| {
            assert!(
                (got - want).abs() < 0.005,
                "{label} score {got:.4} is not {want:.2}"
            );
        };
        near(
            score(
                "The lexical default is BM25+. It beat BM25 by two points on turns.",
                "lexical default",
            ),
            0.81,
            "lexical",
        );
        near(
            score("CombMNZ over RRF for fusing two ballots.", "fusing"),
            0.35,
            "fusing",
        );
        near(
            score(
                "Measure the files before picking a codec.",
                "measure the files",
            ),
            0.81,
            "measure",
        );
    }

    #[test]
    fn no_close_line_names_the_query_and_a_broader_command() {
        let line = no_close_line("escapees list_scans uppercase keys");
        assert!(
            line.starts_with("No close matches for \"escapees list_scans uppercase keys\"."),
            "{line}"
        );
        assert!(line.contains("Try a broader query: `ljos search"), "{line}");
        assert!(!line.contains('/'), "{line}");
        let one = no_close_line("xylophone");
        assert_eq!(one, "No close matches for xylophone. Try a broader query.");
    }

    /// A corpus written for this test. The numbers it prints are this
    /// fixture only, not a pack and not a published collection.
    struct Mem {
        id: &'static str,
        seat: &'static str,
        title: &'static str,
        tags: &'static str,
        body: &'static str,
    }

    struct Ask {
        query: &'static str,
        /// The id that should be first.
        primary: &'static str,
        /// Ids that count as on topic in the top five. Includes `primary`.
        relevant: &'static [&'static str],
    }

    fn corpus() -> Vec<Mem> {
        let mut rows = vec![
            Mem {
                id: "m-scans",
                seat: "bao",
                title: "",
                tags: "",
                body: "Escapees list_scans returns uppercase keys for each roster row.",
            },
            Mem {
                id: "d-scans",
                seat: "ada",
                title: "",
                tags: "",
                body: "The key list uses uppercase labels on the scan form.",
            },
            Mem {
                id: "m-am",
                seat: "ada",
                title: "",
                tags: "",
                body: "git am fails when the SHA-1 information is lacking on a stale base.",
            },
            Mem {
                id: "d-am",
                seat: "ada",
                title: "",
                tags: "",
                body: "git fails when base information is lacking and the stale note is unread.",
            },
            Mem {
                id: "m-tests",
                seat: "ada",
                title: "",
                tags: "",
                body: "The integration tests failed after the fixture rename.",
            },
            Mem {
                id: "d-tests",
                seat: "ada",
                title: "",
                tags: "",
                body: "A failing deploy left the unit tests for later.",
            },
            Mem {
                id: "m-http2",
                seat: "ada",
                title: "",
                tags: "",
                body: "Send the HTTP/2 preface before any settings frame.",
            },
            Mem {
                id: "d-http",
                seat: "ada",
                title: "",
                tags: "",
                body: "The preface of the operator manual is one page.",
            },
            Mem {
                id: "m-roster",
                seat: "ada",
                title: "list_scans",
                tags: "escapees",
                body: "The roster endpoint capitalises every name.",
            },
            Mem {
                id: "d-roster",
                seat: "ada",
                title: "",
                tags: "",
                body: "The list of scans is pinned on the roster board.",
            },
            Mem {
                id: "m-phrase",
                seat: "ada",
                title: "",
                tags: "",
                body: "git am rejects a stale base commit when the parent moved.",
            },
            Mem {
                id: "d-phrase",
                seat: "ada",
                title: "",
                tags: "",
                body: "The base image is stale and the commit hook checks the author name.",
            },
            Mem {
                id: "m-foreign",
                seat: "bao",
                title: "",
                tags: "",
                body: "The cedar lane bakery hours are posted on the glass door.",
            },
            Mem {
                id: "d-seat",
                seat: "ada",
                title: "",
                tags: "",
                body: "Cedar lane is closed on Monday.",
            },
            Mem {
                id: "m-kiln",
                seat: "ada",
                title: "",
                tags: "",
                body: "The kiln damper reading is taken before the evening firing.",
            },
            Mem {
                id: "m-cedar",
                seat: "ada",
                title: "",
                tags: "",
                body: "The notice board lists the choir rehearsal time.",
            },
            Mem {
                id: "d-cedar",
                seat: "bao",
                title: "",
                tags: "",
                body: "The notice board lists the choir rehearsal time.",
            },
            Mem {
                id: "m-api",
                seat: "ada",
                title: "",
                tags: "",
                body: "The api client timeout is thirty seconds on the retry path.",
            },
            Mem {
                id: "m-retry",
                seat: "ada",
                title: "",
                tags: "",
                body: "Honor the retry-after header on every 429 from the API.",
            },
            Mem {
                id: "m-kebab",
                seat: "ada",
                title: "",
                tags: "",
                body: "Rebase onto the stale base before sending the branch.",
            },
            Mem {
                id: "d-short",
                seat: "ada",
                title: "",
                tags: "",
                body: "git base",
            },
            Mem {
                id: "m-dots",
                seat: "cam",
                title: "retryAfterHeader",
                tags: "",
                body: "The client reads that header when the call is retried.",
            },
        ];
        for (index, body) in FILLER.iter().enumerate() {
            let seat = ["bao", "cam", "dee", "eve"][index % 4];
            // Leak once per process. The fixture is static for the ranker.
            let id: &'static str = Box::leak(format!("f{index:02}").into_boxed_str());
            let seat: &'static str = seat;
            rows.push(Mem {
                id,
                seat,
                title: "",
                tags: "",
                body,
            });
        }
        rows
    }

    const FILLER: &[&str] = &[
        "git status shows the untracked notes in the scratch folder.",
        "The base branch is protected and a direct push is refused.",
        "A keyboard key sticks when the room is cold.",
        "The packing list for the harbor trip is on the fridge.",
        "The unit tests for the parser run in under a second.",
        "Scan the old photos before the album glue fails.",
        "Uppercase headings in the stylesheet use a small tracking value.",
        "git log is the place to read why the last commit landed.",
        "The database base backup runs after midnight.",
        "House keys live on the hook by the studio door.",
        "A waiting list for the kiln shelf fills up in March.",
        "The flaky test was a clock that moved during the assertion.",
        "The flatbed scan of the poster lost the red channel.",
        "CSS uppercase on a button label is hard to read.",
        "git rebase onto the release branch needs a clean tree.",
        "The base camp list names who carries the water.",
        "API keys for the demo shop are rotated each quarter.",
        "The shopping list missed the oats and the cinnamon.",
        "Integration style tests are not what this note is about.",
        "A document scan of the lease sits in the archive box.",
        "The title uses uppercase for the city name only.",
        "git diff against the base shows the damper change.",
        "The ferry base fare doubles on the holiday weekend.",
        "Remember which key opens the shed, not the house.",
        "The guest list for the reading is capped at forty.",
        "A failed test on the laptop was a full disk, not the code.",
        "Photo scans from the trip are named by the day.",
        "Do not set the whole paragraph to uppercase.",
        "git fetch the base remote before judging the branch.",
        "The tax base for the stall is the weekend takings.",
        "Spare keys are in the tin labelled hardware.",
        "The reading list for the new hire starts with the glossary.",
        "Tests that need the network are skipped on the train.",
        "A preview scan of the plate shows a hairline crack.",
        "The poster title is uppercase and the subtitle is not.",
        "The morning train to the harbor leaves at seven.",
        "Rye bread needs a longer ferment than the weeknight loaf.",
        "The damper on the stove is half open for oak.",
        "A chipped mug is the one the studio uses for ink.",
        "The north bench holds the drying racks.",
        "Salt the pasta water before it boils.",
        "The library card expired in the spring.",
        "Window putty hardens if the tin is left open.",
        "The choir rehearses on Thursday in the side room.",
        "Orange peel in the compost attracts wasps.",
        "The ladder stays in the shed, not the hall.",
        "Wool socks dry flat, away from the stove.",
        "The market stall packs up when the bell rings.",
        "A blunt pencil is fine for the sketch layer.",
        "The cistern was insulated after the cold snap.",
        "Bus route 4 stops at the bakery corner.",
        "The glaze sample tile is numbered on the back.",
        "Leave the studio fan on until the fumes clear.",
        "The recipe card says rest the dough for an hour.",
        "A spare bulb is in the drawer under the kettle.",
        "The footpath to the headland is muddy after rain.",
        "Notebooks live on the shelf above the desk.",
        "The community garden gate latches from the inside.",
        "Steep the tea for four minutes and no longer.",
        "The radio stays off during the firing.",
        "A torn map of the coast is pinned above the bench.",
        "The laundry line faces the afternoon sun.",
        "Oat biscuits keep for a week in the tin.",
        "The porch light is on a timer after dusk.",
        "Sand the edge before the oil coat.",
        "The town clock is two minutes fast.",
        "Bring the crate back to the stall on Sunday.",
        "The ink bottle cap must be closed or it skins.",
        "A folded blanket is the seat cushion on the boat.",
        "The herb pot on the sill wants water every other day.",
    ];

    fn asks() -> Vec<Ask> {
        vec![
            Ask {
                query: "escapees list_scans uppercase keys",
                primary: "m-scans",
                relevant: &["m-scans", "m-roster"],
            },
            Ask {
                query: "git am fails sha1 information lacking stale base",
                primary: "m-am",
                relevant: &["m-am", "d-am", "m-phrase"],
            },
            Ask {
                query: "failing integration tests",
                primary: "m-tests",
                relevant: &["m-tests"],
            },
            Ask {
                query: "http2 preface",
                primary: "m-http2",
                relevant: &["m-http2"],
            },
            Ask {
                query: "escapees roster list_scans",
                primary: "m-roster",
                relevant: &["m-roster", "m-scans"],
            },
            Ask {
                query: "stale base commit",
                primary: "m-phrase",
                relevant: &["m-phrase", "m-am"],
            },
            Ask {
                query: "cedar lane bakery glass door",
                primary: "m-foreign",
                relevant: &["m-foreign"],
            },
            Ask {
                query: "kiln damper reading",
                primary: "m-kiln",
                relevant: &["m-kiln"],
            },
            Ask {
                query: "notice board choir rehearsal",
                primary: "m-cedar",
                relevant: &["m-cedar", "d-cedar"],
            },
            Ask {
                query: "api.client timeout",
                primary: "m-api",
                relevant: &["m-api"],
            },
            Ask {
                query: "retryAfterHeader",
                primary: "m-dots",
                relevant: &["m-dots", "m-retry"],
            },
            Ask {
                query: "stale-base rebase",
                primary: "m-kebab",
                relevant: &["m-kebab", "m-phrase", "m-am"],
            },
        ]
    }

    fn docs_of<'a>(rows: &'a [Mem], body_only: bool) -> Vec<Doc<'a>> {
        rows.iter()
            .map(|row| Doc {
                id: row.id,
                own: row.seat == "ada",
                title: if body_only { "" } else { row.title },
                tags: if body_only { "" } else { row.tags },
                body: row.body,
            })
            .collect()
    }

    fn top_ids(order: &[usize], rows: &[Mem], n: usize) -> Vec<&'static str> {
        order.iter().take(n).map(|index| rows[*index].id).collect()
    }

    fn hit_rates(orders: &[Vec<usize>], rows: &[Mem], asks: &[Ask]) -> (f64, f64) {
        let mut top1 = 0usize;
        let mut top5 = 0usize;
        for (order, ask) in orders.iter().zip(asks) {
            let ids = top_ids(order, rows, 5);
            if ids.first().copied() == Some(ask.primary) {
                top1 += 1;
            }
            if ids.iter().any(|id| ask.relevant.contains(id)) {
                top5 += 1;
            }
        }
        let n = asks.len() as f64;
        (top1 as f64 / n, top5 as f64 / n)
    }

    /// The search re-rank before this change: this seat first, then
    /// [`crate::lexical_match_scores`] on the body alone.
    fn old_search_orders(rows: &[Mem], asks: &[Ask]) -> Vec<Vec<usize>> {
        let _env = crate::test_env_lock();
        unsafe { std::env::set_var("LJOS_SEAT", "ada") };
        asks.iter()
            .map(|ask| {
                let hits: Vec<Hit> = rows
                    .iter()
                    .map(|row| Hit {
                        id: Some(row.id.to_string()),
                        text: row.body.to_string(),
                        score: 1.0,
                        kind: "lesson".into(),
                        ts: None,
                        entities: vec![format!("seat:{}", row.seat)],
                        ballots: None,
                        of: None,
                        rerank_score: None,
                        labels: Vec::new(),
                    })
                    .collect();
                crate::relevance_order(&hits, ask.query)
            })
            .collect()
    }

    /// The local snapshot overlap before this change: the fraction of query
    /// words found in the body. Words split on anything that is not a letter
    /// or a digit. Ties break toward the earlier row.
    fn old_overlap_orders(rows: &[Mem], asks: &[Ask]) -> Vec<Vec<usize>> {
        fn words(text: &str) -> Vec<String> {
            text.split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_lowercase)
                .collect()
        }
        asks.iter()
            .map(|ask| {
                let query = words(ask.query);
                let mut scored: Vec<(usize, f64)> = rows
                    .iter()
                    .enumerate()
                    .filter_map(|(index, row)| {
                        if query.is_empty() {
                            return None;
                        }
                        let body = words(row.body);
                        let hit = query
                            .iter()
                            .filter(|word| body.iter().any(|item| item == *word))
                            .count();
                        let score = hit as f64 / query.len() as f64;
                        (score > 0.0).then_some((index, score))
                    })
                    .collect();
                scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                scored.into_iter().map(|(index, _)| index).collect()
            })
            .collect()
    }

    fn new_orders(rows: &[Mem], asks: &[Ask]) -> Vec<Vec<usize>> {
        let docs = docs_of(rows, false);
        asks.iter()
            .map(|ask| match rank(&docs, ask.query) {
                Ranked::Hits(scored) => scored.into_iter().map(|row| row.index).collect(),
                Ranked::NoClose | Ranked::None => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn the_fixture_puts_the_named_memory_first() {
        let rows = corpus();
        assert!(
            (50..=200).contains(&rows.len()),
            "the fixture is a small corpus, not a dump: {}",
            rows.len()
        );
        let asks = asks();
        let before_search = old_search_orders(&rows, &asks);
        let before_overlap = old_overlap_orders(&rows, &asks);
        let after = new_orders(&rows, &asks);
        let (search_top1, search_top5) = hit_rates(&before_search, &rows, &asks);
        let (overlap_top1, overlap_top5) = hit_rates(&before_overlap, &rows, &asks);
        let (top1, top5) = hit_rates(&after, &rows, &asks);
        // Printed so a run of this test is the measurement. These rates are
        // this fixture only.
        eprintln!(
            "word-rank fixture n={} queries={} before-search top1={search_top1:.3} top5={search_top5:.3} before-overlap top1={overlap_top1:.3} top5={overlap_top5:.3} after top1={top1:.3} top5={top5:.3}",
            rows.len(),
            asks.len()
        );
        assert!(
            top1 > search_top1 && top1 > overlap_top1,
            "the new ranker did not beat both old paths: search {search_top1:.3} overlap {overlap_top1:.3} after {top1:.3}"
        );
        assert!(top1 >= 0.8, "top-1 {top1:.3}");
        assert!(top5 >= 0.9, "top-5 {top5:.3}");
        for (ask, order) in asks.iter().zip(&after) {
            let ids = top_ids(order, &rows, 5);
            assert_eq!(
                ids.first().copied(),
                Some(ask.primary),
                "{} ranked {ids:?}, wanted {}",
                ask.query,
                ask.primary
            );
        }
        let docs = docs_of(&rows, false);
        assert!(
            matches!(rank(&docs, "xylophone quadrature plankton"), Ranked::None),
            "a query that shares nothing should not become a list of rows"
        );
        // One ordinary word of a long query is not a close match.
        assert!(matches!(
            rank(&docs, "escapees list_scans uppercase keys xylophone"),
            Ranked::Hits(_) | Ranked::NoClose
        ),);
    }
}
