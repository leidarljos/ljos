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
//! A row that matches any query term is a candidate. More matched terms rank
//! higher, then rarer terms. Matching is case-insensitive. A light suffix
//! rule treats `narration`, `narrated` and `narrate` as one word, and `video`
//! and `videos` as one. A word in every row is common once four rows carry
//! it, and a row that only shares that word is left out. A smaller store
//! still returns the row. When nothing is close, the broader query names a
//! term that occurs.

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

/// How many rows a word must sit in before a word that is in every row
/// is common. Two lessons can share a word. That store is too small for
/// the word to be common, and a query that meets a lesson on that word
/// alone still returns the lesson. Four rows is where the cut used for
/// a large store begins.
const COMMON_ROWS: usize = 4;

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
    /// Something shared a word, and nothing cleared the floor.
    /// `suggest` names a broader search whose term occurs in the corpus.
    NoClose { suggest: Option<String> },
    /// Nothing shared a content word, or the query had none.
    None,
}

/// One row that cleared the floor, with the score the page prints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    pub index: usize,
    /// Query terms this row covered, including a word-form match.
    pub covered: usize,
    pub score: f64,
}

/// How [`order_hits`] left a search page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordList {
    /// Rows were reordered and weak ones removed.
    Kept,
    /// The page had rows, and none cleared the floor.
    /// `suggest` is the broader command when a term in the corpus can be named.
    NoClose { suggest: Option<String> },
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
///
/// The broader command is the longest content word. Callers that have the
/// corpus use [`no_close_line_with`] so the named term is one that occurs.
#[must_use]
pub fn no_close_line(query: &str) -> String {
    no_close_line_with(query, broader_command(query).as_deref())
}

/// [`no_close_line`] with the broader command already chosen.
/// `cmd` is omitted when no content word is safe to print, or none of them
/// occur in the corpus.
#[must_use]
pub fn no_close_line_with(query: &str, cmd: Option<&str>) -> String {
    let query = query.trim();
    let shown = quote_query(query);
    match cmd.map(str::trim).filter(|cmd| !cmd.is_empty()) {
        Some(cmd) => format!("No close matches for {shown}. Try a broader query: `{cmd}`."),
        None => format!("No close matches for {shown}. Try a broader query."),
    }
}

/// Lowest score kept for a row that does not name a subject word.
///
/// Two words of a six-word query is 0.33, which is the unrelated lesson a
/// local snapshot returned for `ridley-core landing order rebase review`.
/// A one-word subject search such as `fusing` scores about 0.35, so this
/// line leaves that row in. `0` turns the floor off.
pub const DEFAULT_MIN_SCORE: f64 = 34.0;

/// The score a person reads, from 0 to 100.
///
/// A word match is a fraction of the query, so 0.33 is 33. A meaning score
/// the pack already reports on this scale, such as 71 or 100, stays.
#[must_use]
pub fn printed_score(score: f64) -> f64 {
    if !score.is_finite() || score <= 0.0 {
        return 0.0;
    }
    let scaled = if score <= 1.0 { score * 100.0 } else { score };
    scaled.min(100.0)
}

/// Words that show up in lessons about other work. One of them, or a run
/// of them, is not the subject of a longer query. `test` and `error` are
/// not on this list: a query is often about those.
const WEAK: &[&str] = &[
    "add", "also", "amount", "case", "change", "check", "code", "core", "count", "data", "end",
    "file", "find", "fix", "full", "group", "help", "high", "info", "item", "just", "level",
    "line", "list", "low", "make", "more", "name", "need", "new", "note", "number", "order",
    "page", "part", "please", "point", "problem", "rate", "result", "review", "run", "safety",
    "search", "set", "show", "side", "size", "system", "task", "text", "thing", "time", "total",
    "type", "update", "use", "user", "value", "way", "weight", "work",
];

/// Words that name a kind of page rather than a project. Sorted for search.
const GENERIC: &[&str] = &[
    "doc", "docs", "embed", "record", "records", "release", "video",
];

/// Similarity another seat must reach when it shares no distinctive term.
///
/// [`crate::CROSS_SEAT_COSINE`] still keeps a row that names one of those
/// terms and is only short of half of them. A fused score is not this
/// number: `71` is how the pack combined its ballots, not how close the
/// text is.
const OTHER_SEAT_SIMILARITY: f64 = 0.80;

/// The line when the floor removed every row.
///
/// The hint names `--min-score 0`, which prints the rows the floor removed.
#[must_use]
pub fn no_relevant_line(query: &str) -> String {
    let shown = quote_query(query.trim());
    format!("No relevant matches for {shown}. Pass --min-score 0 to see every row.")
}

/// The command `--json` names when the floor removed every row.
#[must_use]
pub fn min_score_off_command(query: &str) -> String {
    let query = query.trim();
    let unsafe_query = query.is_empty()
        || query
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '"' | '`' | '\\'));
    if unsafe_query {
        return "ljos search --min-score 0".to_string();
    }
    if query.contains(char::is_whitespace) {
        format!("ljos search \"{query}\" --min-score 0")
    } else {
        format!("ljos search {query} --min-score 0")
    }
}

/// Drop rows the floor does not keep. `min_score` of 0 keeps every row.
///
/// Returns whether rows were present and none remain, which is the page
/// [`no_relevant_line`] names. A ticket stays: the tracker match already
/// refused a single common word.
#[must_use]
pub fn apply_relevance_floor(hits: &mut Vec<Hit>, query: &str, min_score: f64) -> bool {
    if min_score <= 0.0 || hits.is_empty() {
        return false;
    }
    let before = hits.len();
    hits.retain(|hit| keeps_row(hit, query, min_score));
    before > 0 && hits.is_empty()
}

/// Whether `hit` is worth printing for `query` at `min_score`.
///
/// A row that shares a distinctive word stays, including a stem such as
/// `narration` against `narrated`, even when the stored score is only the
/// placeholder for a word the overlap count missed. A longer query that
/// the row meets only in a stopword or a common weak word does not stay,
/// and neither does a meaning score that is only a fused ballot total.
/// Another seat has to cover half of the distinctive terms, or reach
/// [`OTHER_SEAT_SIMILARITY`] when it covers none. A row that shares no
/// word stays on this seat when its score is at the floor, which is how a
/// tag search returns a lesson that does not spell the tag.
#[must_use]
pub fn keeps_row(hit: &Hit, query: &str, min_score: f64) -> bool {
    if min_score <= 0.0 {
        return true;
    }
    if hit.kind == "ticket" {
        return true;
    }
    if !hit.score.is_finite() || hit.score < 0.0 {
        return false;
    }
    let blob = hit_blob(hit);
    if shares_only_stopwords(query, &blob) {
        return false;
    }
    let covered = covered_terms(query, &blob);
    let distinctive = covered.iter().filter(|term| !term_is_weak(term)).count();
    let query_distinctive = concepts(query)
        .iter()
        .filter(|term| !term_is_weak(term))
        .count();
    let other = from_another_seat(hit);
    let cosine = hit_cosine(hit);
    if query_distinctive == 0 {
        if covered.is_empty() {
            return similarity_keeps(other, cosine, false)
                || (!other && printed_score(hit.score) >= min_score);
        }
        return printed_score(hit.score) >= min_score || similarity_keeps(other, cosine, true);
    }
    if enough_distinctive(distinctive, query_distinctive, other) {
        return true;
    }
    if similarity_keeps(other, cosine, distinctive > 0) {
        return true;
    }
    // No distinctive word. This seat's pack hit that shares nothing still
    // answers. A shared generic word does not, at any fused score.
    distinctive == 0 && covered.is_empty() && !other && printed_score(hit.score) >= min_score
}

/// Half of the distinctive terms, rounded up. This seat needs one.
fn enough_distinctive(covered: usize, total: usize, other: bool) -> bool {
    if covered == 0 || total == 0 {
        return false;
    }
    if !other {
        return true;
    }
    covered >= total.div_ceil(2)
}

/// `cosine` is the encoder's similarity, from 0 to 1. Absent means the
/// pack did not send one. A score above 1 is a fused total and is not read.
fn similarity_keeps(other: bool, cosine: Option<f64>, has_distinctive: bool) -> bool {
    let Some(cosine) = cosine else {
        return false;
    };
    let bar = if other && !has_distinctive {
        OTHER_SEAT_SIMILARITY
    } else {
        super::CROSS_SEAT_COSINE
    };
    cosine >= bar
}

fn from_another_seat(hit: &Hit) -> bool {
    super::other_seat(&hit.entities, &super::seat_name()).is_some()
}

fn hit_cosine(hit: &Hit) -> Option<f64> {
    hit.entities.iter().find_map(|tag| {
        tag.strip_prefix("cosine:")
            .and_then(|raw| raw.parse::<f64>().ok())
            .filter(|cosine| cosine.is_finite() && (0.0..=1.0).contains(cosine))
    })
}

fn hit_blob(hit: &Hit) -> String {
    let mut blob = hit.text.clone();
    for tag in &hit.entities {
        if let Some(rest) = tag
            .strip_prefix("title:")
            .or_else(|| tag.strip_prefix("tag:"))
        {
            blob.push(' ');
            blob.push_str(rest);
        }
    }
    blob
}

fn shares_only_stopwords(query: &str, text: &str) -> bool {
    let have: std::collections::BTreeSet<String> = raw_words(text).into_iter().collect();
    let shared: Vec<String> = raw_words(query)
        .into_iter()
        .filter(|word| have.contains(word))
        .collect();
    !shared.is_empty()
        && shared
            .iter()
            .all(|word| is_stop(word) || word.chars().count() < 3)
}

fn raw_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn covered_terms(query: &str, text: &str) -> Vec<String> {
    concepts(query)
        .into_iter()
        .filter(|term| shares_term(term, text))
        .collect()
}

pub(crate) fn term_is_weak(term: &str) -> bool {
    let concept = concept_of(term);
    let parts = if concept.parts.is_empty() {
        vec![concept.joined]
    } else {
        concept.parts
    };
    !parts.is_empty() && parts.iter().all(|part| is_weak_word(part))
}

fn is_weak_word(word: &str) -> bool {
    WEAK.binary_search(&word).is_ok()
}

/// Whether `text` shares one content word with `query`, after identifier
/// splits. The local snapshot uses this to keep a row the old overlap missed,
/// such as `sha1` against `SHA-1`.
#[must_use]
pub fn shares_term(query: &str, text: &str) -> bool {
    term_coverage(query, text) > 0
}

/// How many query terms occur in `text`.
///
/// Identifier splits and stems count, so `sign-in` covers `signed-in` and
/// `SHA-1` covers `sha1`. A stop word is not a term.
#[must_use]
pub fn term_coverage(query: &str, text: &str) -> usize {
    let terms = QueryTerms::of(query);
    if terms.is_empty() {
        return 0;
    }
    terms.coverage(&TermBag::of(text))
}

/// Query terms that occur in `text`, in the order they were typed.
#[must_use]
pub fn covered_query_terms(query: &str, text: &str) -> Vec<String> {
    let terms = QueryTerms::of(query);
    if terms.is_empty() {
        return Vec::new();
    }
    terms.covered(&TermBag::of(text))
}

/// The terms of one text, analyzed once so many queries can read them.
pub struct TermBag(std::collections::BTreeSet<String>);

impl TermBag {
    #[must_use]
    pub fn of(text: &str) -> Self {
        Self(analyze(text).bag.into_iter().collect())
    }
}

/// A query's terms, analyzed once and checked against any number of
/// [`TermBag`]s. [`term_coverage`] and [`covered_query_terms`] read the
/// same terms.
pub struct QueryTerms {
    terms: Vec<(String, Concept)>,
}

impl QueryTerms {
    #[must_use]
    pub fn of(query: &str) -> Self {
        Self {
            terms: concepts(query)
                .into_iter()
                .map(|written| {
                    let concept = concept_of(&written);
                    (written, concept)
                })
                .collect(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// The terms in the order they were typed.
    pub fn written(&self) -> impl Iterator<Item = &str> {
        self.terms.iter().map(|(written, _)| written.as_str())
    }

    /// Whether each term occurs in `bag`, in the order they were typed.
    pub fn hits<'a>(&'a self, bag: &'a TermBag) -> impl Iterator<Item = bool> + 'a {
        self.terms
            .iter()
            .map(move |(_, concept)| covered_by(concept, &bag.0))
    }

    /// Whether any term occurs in `bag`.
    #[must_use]
    pub fn any_in(&self, bag: &TermBag) -> bool {
        self.hits(bag).any(|hit| hit)
    }

    /// How many terms occur in `bag`.
    #[must_use]
    pub fn coverage(&self, bag: &TermBag) -> usize {
        self.hits(bag).filter(|hit| *hit).count()
    }

    /// The terms that occur in `bag`, in the order they were typed.
    #[must_use]
    pub fn covered(&self, bag: &TermBag) -> Vec<String> {
        self.written()
            .zip(self.hits(bag))
            .filter(|(_, hit)| *hit)
            .map(|(written, _)| written.to_string())
            .collect()
    }
}

/// Whether `term` is a common word that does not name a project or a subject.
///
/// `docs`, `release` and `video` are the ones a long query shares with an
/// unrelated ticket. A weak word such as `update` counts too. A project
/// token such as `d-seams` does not.
#[must_use]
pub fn generic_term(term: &str) -> bool {
    let concept = concept_of(term);
    // The joined form is the word as written. A stem such as `emb` from
    // `embed` is not a second word that has to be generic on its own.
    if generic_word(term) || generic_word(&concept.joined) {
        return true;
    }
    !concept.parts.is_empty() && concept.parts.iter().all(|part| generic_word(part))
}

fn listed_generic(word: &str) -> bool {
    GENERIC.binary_search(&word).is_ok() || is_weak_word(word) || is_stop(word)
}

fn generic_word(word: &str) -> bool {
    let lower = word.to_lowercase();
    let stemmed = stem(&lower);
    listed_generic(&lower) || listed_generic(&stemmed)
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
    let form_df = form_document_frequency(&prepared, &concepts);
    let n = prepared.len();
    let n_f = n as f64;
    let avgdl = average_body_len(&prepared);
    let mut concept_df = vec![0usize; concepts.len()];
    for doc in &prepared {
        for (index, concept) in concepts.iter().enumerate() {
            if concept_covered(concept, &doc.have) {
                concept_df[index] += 1;
            }
        }
    }
    let common: Vec<bool> = concept_df
        .iter()
        .map(|hits| *hits >= COMMON_ROWS && *hits == n)
        .collect();
    let mut any = false;
    let mut close: Vec<(usize, f64, usize, f64, usize)> = Vec::new();
    for (index, doc) in prepared.iter().enumerate() {
        let matched: Vec<usize> = concepts
            .iter()
            .enumerate()
            .filter(|(_, concept)| concept_covered(concept, &doc.have))
            .map(|(index, _)| index)
            .collect();
        if matched.is_empty() {
            continue;
        }
        any = true;
        if !keep_partial(&matched, &common, concepts.len()) {
            continue;
        }
        let covered = matched.len();
        let body = bm25(
            &doc.body.bag,
            doc.body.bag.len(),
            &concepts,
            &df,
            &form_df,
            n_f,
            avgdl,
        );
        let title = bm25(
            &doc.title.bag,
            doc.title.bag.len(),
            &concepts,
            &df,
            &form_df,
            n_f,
            avgdl,
        );
        let tags = bm25(
            &doc.tags.bag,
            doc.tags.bag.len(),
            &concepts,
            &df,
            &form_df,
            n_f,
            avgdl,
        );
        let phrase = phrase_bonus(&doc.body, &doc.title, &concepts, &df, n_f);
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
        return if any {
            Ranked::NoClose {
                suggest: suggest_hitting(&concepts, &concept_df),
            }
        } else {
            Ranked::None
        };
    }
    // More matched terms first. The same coverage then prefers the rarer
    // terms, which is the BM25 score.
    close.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then(b.1.total_cmp(&a.1))
            .then(b.3.total_cmp(&a.3))
            .then(b.4.cmp(&a.4))
            .then_with(|| prepared[a.0].id.cmp(&prepared[b.0].id))
    });
    Ranked::Hits(
        close
            .into_iter()
            .map(|(index, score, covered, _, _)| Scored {
                index,
                covered,
                score,
            })
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
                b.covered
                    .cmp(&a.covered)
                    .then(b.score.total_cmp(&a.score))
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
        Ranked::NoClose { suggest } => {
            hits.clear();
            WordList::NoClose { suggest }
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

pub(crate) fn quote_query(query: &str) -> String {
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

/// A shorter search, when one content word is not the whole query.
///
/// Stop words are skipped. The longest content word is the one named,
/// which is the rarest-looking word in the query. The same length keeps
/// the earlier word. Absent when that word is the query, or when it is
/// not one shell token.
#[must_use]
pub fn shorter_search(query: &str) -> Option<String> {
    let mut best: Option<&str> = None;
    for raw in query.split(char::is_whitespace) {
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && !is_sep(c));
        if word.is_empty() || !keep_concept(word) {
            continue;
        }
        let longer = best.is_none_or(|have| word.chars().count() > have.chars().count());
        if longer {
            best = Some(word);
        }
    }
    let best = best?;
    if query.trim().eq_ignore_ascii_case(best) {
        return None;
    }
    let safe = best
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    if !safe {
        return None;
    }
    Some(format!("ljos search {best}"))
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

/// A partial overlap stays when it is more than one term, or the one term
/// is not common. One query word always stays: that word is the subject.
/// A word is common when it is in every row and at least [`COMMON_ROWS`]
/// rows carry it. Two lessons can share a word without that word being
/// common.
fn keep_partial(matched: &[usize], common: &[bool], query_terms: usize) -> bool {
    if matched.is_empty() {
        return false;
    }
    if query_terms <= 1 || matched.len() >= 2 {
        return true;
    }
    !common[matched[0]]
}

/// The longest query term that occurs, as a shell command.
/// One hitting term is that term. A term with no row is not named.
fn suggest_hitting(concepts: &[String], df: &[usize]) -> Option<String> {
    if concepts.len() < 2 {
        return None;
    }
    let mut best: Option<&str> = None;
    for (word, hits) in concepts.iter().zip(df) {
        if *hits == 0 {
            continue;
        }
        let replace = best.is_none_or(|have| word.chars().count() > have.chars().count());
        if replace {
            best = Some(word);
        }
    }
    let best = best?;
    let safe = best
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    safe.then(|| format!("ljos search {best}"))
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
    covered_by(&concept_of(written), have)
}

fn covered_by(concept: &Concept, have: &std::collections::BTreeSet<String>) -> bool {
    if !concept.joined.is_empty() && have.contains(&concept.joined) {
        return true;
    }
    if !concept.joined_stems.is_empty()
        && concept.joined_stems != concept.joined
        && have.contains(&concept.joined_stems)
    {
        return true;
    }
    if !concept.parts.is_empty() && concept.parts.iter().all(|part| have.contains(part)) {
        return true;
    }
    form_covered(concept, have)
}

/// A shared prefix plus an inflection, so `narration` covers `narrated`.
fn forms_match(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    if !left.bytes().all(|byte| byte.is_ascii_lowercase())
        || !right.bytes().all(|byte| byte.is_ascii_lowercase())
    {
        return false;
    }
    let left_b = left.as_bytes();
    let right_b = right.as_bytes();
    let mut shared = 0usize;
    while shared < left_b.len() && shared < right_b.len() && left_b[shared] == right_b[shared] {
        shared += 1;
    }
    shared >= 5 && inflection(&left[shared..]) && inflection(&right[shared..])
}

fn inflection(rest: &str) -> bool {
    matches!(
        rest,
        "" | "e" | "ed" | "es" | "s" | "ing" | "ion" | "tion" | "ation"
    )
}

fn form_covered(concept: &Concept, have: &std::collections::BTreeSet<String>) -> bool {
    let joined = [concept.joined.as_str(), concept.joined_stems.as_str()];
    let joined_ok = joined
        .iter()
        .filter(|term| term.len() >= 2)
        .any(|term| have.iter().any(|got| forms_match(got, term)));
    if joined_ok {
        return true;
    }
    !concept.parts.is_empty()
        && concept
            .parts
            .iter()
            .all(|part| have.iter().any(|got| forms_match(got, part)))
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

fn form_document_frequency(
    docs: &[Prepared],
    concepts: &[String],
) -> std::collections::HashMap<String, usize> {
    let mut terms = std::collections::BTreeSet::new();
    for written in concepts {
        let concept = concept_of(written);
        if concept.joined.len() >= 2 {
            terms.insert(concept.joined);
        }
        if concept.joined_stems.len() >= 2 {
            terms.insert(concept.joined_stems);
        }
        terms.extend(concept.parts);
    }
    let mut df = std::collections::HashMap::new();
    for term in terms {
        let n = docs
            .iter()
            .filter(|doc| doc.have.iter().any(|got| forms_match(got, &term)))
            .count();
        df.insert(term, n);
    }
    df
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
    form_df: &std::collections::HashMap<String, usize>,
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
        let mut exact = false;
        for term in std::iter::once(concept.joined.clone())
            .chain(std::iter::once(concept.joined_stems.clone()))
            .chain(concept.parts.iter().cloned())
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
            exact = true;
            let weight = idf(n, docs_with);
            let tf = count * (K1 + 1.0) / (count + K1 * norm);
            term_score += weight * tf;
        }
        if !exact {
            term_score += form_term_score(&concept, &freq, form_df, n, norm);
        }
        score += term_score;
    }
    score
}

/// BM25 for a word form when the exact stem is absent.
/// An exact hit keeps the previous weight, so a stored lesson's score stays.
fn form_term_score(
    concept: &Concept,
    freq: &std::collections::HashMap<&str, f64>,
    form_df: &std::collections::HashMap<String, usize>,
    n: f64,
    norm: f64,
) -> f64 {
    let mut best: f64 = 0.0;
    let mut terms = Vec::new();
    if concept.joined.len() >= 2 {
        terms.push(concept.joined.as_str());
    }
    if concept.joined_stems.len() >= 2 {
        terms.push(concept.joined_stems.as_str());
    }
    terms.extend(concept.parts.iter().map(String::as_str));
    for term in terms {
        let Some(docs_with) = form_df.get(term).copied().filter(|docs| *docs > 0) else {
            continue;
        };
        let mut count = 0.0;
        for (got, times) in freq {
            if forms_match(got, term) {
                count += times;
            }
        }
        if count == 0.0 {
            continue;
        }
        let weight = idf(n, docs_with);
        let tf = count * (K1 + 1.0) / (count + K1 * norm);
        best = best.max(weight * tf);
    }
    best
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
        assert_eq!(
            term_coverage(
                "usglobalmail sign-in preflight",
                "Odysee sign-in link relay"
            ),
            1
        );
        assert_eq!(
            term_coverage("usglobalmail sign-in preflight", "referral codes"),
            0
        );
        assert_eq!(
            term_coverage(
                "usglobalmail sign-in preflight",
                "The usglobalmail sign-in preflight is the archive check."
            ),
            3
        );
        assert!(generic_term("docs"));
        assert!(generic_term("release"));
        assert!(generic_term("video"));
        assert!(generic_term("videos"));
        assert!(generic_term("embed"));
        assert!(generic_term("records"));
        assert!(!generic_term("d-seams"));
        assert!(!generic_term("readcon"));
        assert!(!generic_term("migrate"));
        assert_eq!(
            covered_query_terms("d-SEAMS migrate video docs embed", "Embed the docs video"),
            ["video", "docs", "embed"]
        );
        assert!(shares_term("narration", "GPRidge narrated video set"));
        assert!(shares_term("narrate", "GPRidge narrated video set"));
        assert!(shares_term("narrated", "A narrated ridge walk."));
        assert!(shares_term("videos", "GPRidge narrated video set"));
        assert!(shares_term("video", "Two videos of the ridge."));
        assert!(!shares_term("xylophone", "GPRidge narrated video set"));
    }

    #[test]
    fn a_partial_title_outranks_one_shared_word() {
        let ticket = Doc {
            id: "Software-6mrw",
            own: true,
            title: "GPRidge narrated video set",
            tags: "",
            body: "Milestone: the ridge walk is filmed.",
        };
        let tape = Doc {
            id: "tape",
            own: true,
            title: "",
            tags: "",
            body: "A video tape of the harbor.",
        };
        let docs = [ticket, tape];
        let rows = match rank(&docs, "GPRidge video narration") {
            Ranked::Hits(rows) => rows,
            other => panic!("the title was not a hit: {other:?}"),
        };
        assert_eq!(rows[0].index, 0, "the ticket should rank first");
        assert!(
            rows[0].covered >= 2,
            "two title words should count: {rows:?}"
        );
        let folded = match rank(&docs, "gpridge VIDEO Narration") {
            Ranked::Hits(rows) => rows,
            other => panic!("case folding missed the title: {other:?}"),
        };
        assert_eq!(folded[0].index, 0);
    }

    #[test]
    fn a_broader_query_names_a_term_that_occurs() {
        let docs: Vec<Doc<'_>> = (0..COMMON_ROWS)
            .map(|_| Doc {
                id: "tape",
                own: true,
                title: "",
                tags: "",
                body: "The video catalog is on the shelf.",
            })
            .collect();
        match rank(&docs, "GPRidge video narration") {
            Ranked::NoClose { suggest } => {
                assert_eq!(suggest.as_deref(), Some("ljos search video"));
            }
            other => panic!("a common word alone should not be a close match: {other:?}"),
        }
        assert!(matches!(rank(&docs, "xylophone plankton"), Ranked::None));
    }

    #[test]
    fn two_lessons_keep_a_single_shared_word() {
        let kiln = Doc {
            id: "kiln",
            own: true,
            title: "",
            tags: "",
            body: "Seat the kiln door seal only when the clay is cold.",
        };
        let jar = Doc {
            id: "jar",
            own: true,
            title: "",
            tags: "",
            body: "A wax seal keeps the jar shut.",
        };
        let docs = [kiln, jar];
        match rank(&docs, "seal hinge") {
            Ranked::Hits(rows) => {
                assert!(
                    rows.iter().any(|row| row.index == 0),
                    "the kiln lesson matches seal: {rows:?}"
                );
            }
            other => panic!("two lessons should keep a one-word match: {other:?}"),
        }
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

    #[test]
    fn a_shorter_search_keeps_the_longest_content_word() {
        assert_eq!(
            shorter_search("usability friction confusing error").as_deref(),
            Some("ljos search usability")
        );
        assert_eq!(
            shorter_search("which fusion").as_deref(),
            Some("ljos search fusion")
        );
        assert_eq!(
            shorter_search("the usability").as_deref(),
            Some("ljos search usability")
        );
        assert_eq!(
            shorter_search("fusion method").as_deref(),
            Some("ljos search fusion")
        );
        assert_eq!(
            shorter_search("Carrollton Nov 23 morning timing").as_deref(),
            Some("ljos search Carrollton")
        );
        assert_eq!(shorter_search("usability"), None);
        assert_eq!(shorter_search("the"), None);
        assert_eq!(shorter_search("  Fusion  "), None);
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
                Ranked::NoClose { .. } | Ranked::None => Vec::new(),
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
        say!(
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
        // One ordinary word of a long query may be a hit when that word is
        // rare, and it is not a list of unrelated rows.
        assert!(matches!(
            rank(&docs, "escapees list_scans uppercase keys xylophone"),
            Ranked::Hits(_) | Ranked::NoClose { .. }
        ),);
    }

    fn lesson(text: &str, score: f64) -> Hit {
        Hit {
            id: Some(format!("local:{score}")),
            text: text.to_string(),
            score,
            kind: "lesson".into(),
            ts: None,
            entities: vec!["seat:ada".into()],
            ballots: None,
            of: None,
            rerank_score: None,
            labels: Vec::new(),
        }
    }

    /// Overlap the local snapshot used to print: query words found in the
    /// text, split on anything that is not a letter or a digit.
    fn overlap_score(text: &str, query: &str) -> f64 {
        let words = |raw: &str| -> Vec<String> {
            raw.split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_lowercase)
                .collect()
        };
        let query = words(query);
        if query.is_empty() {
            return 0.0;
        }
        let body = words(text);
        let hit = query
            .iter()
            .filter(|word| body.iter().any(|item| item == *word))
            .count();
        hit as f64 / query.len() as f64
    }

    const RIDLEY: &str = "ridley-core landing order rebase review";
    const JOSS: &str =
        "The JOSS editor keeps review comments in document order beside the line they mark.";
    const NARRATED: &str = "GPRidge narrated video set is the milestone for the ridge walk.";
    const RIDLEY_LESSON: &str =
        "Ridley-core landing order is rebase, then review, before the branch is pushed.";

    #[test]
    fn weak_words_stay_sorted_for_the_lookup() {
        let mut sorted = WEAK.to_vec();
        sorted.sort_unstable();
        assert_eq!(WEAK, sorted.as_slice());
    }

    #[test]
    fn an_off_topic_lesson_at_the_reported_score_is_dropped() {
        let score = overlap_score(JOSS, RIDLEY);
        assert!(
            (score - 0.33).abs() < 0.01,
            "the reported row scored 0.33, got {score}"
        );
        let mut hits = vec![
            lesson(JOSS, score),
            lesson(
                "The damper sticks when the kiln is cold.",
                overlap_score("The damper sticks when the kiln is cold.", RIDLEY),
            ),
            lesson(NARRATED, overlap_score(NARRATED, RIDLEY)),
        ];
        hits.retain(|hit| hit.score > 0.0);
        assert!(
            apply_relevance_floor(&mut hits, RIDLEY, DEFAULT_MIN_SCORE),
            "the floor should remove every row: {hits:?}"
        );
        assert!(hits.is_empty());
        let line = no_relevant_line(RIDLEY);
        assert!(line.starts_with("No relevant matches for \""), "{line}");
        assert!(line.contains(RIDLEY), "{line}");
        assert!(line.contains("--min-score 0"), "{line}");
    }

    #[test]
    fn min_score_zero_keeps_the_off_topic_row() {
        let mut hits = vec![lesson(JOSS, overlap_score(JOSS, RIDLEY))];
        assert!(!apply_relevance_floor(&mut hits, RIDLEY, 0.0));
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn exact_stemmed_and_partial_overlap_stay() {
        let mut hits = vec![lesson(NARRATED, overlap_score(NARRATED, NARRATED))];
        assert!(!apply_relevance_floor(
            &mut hits,
            NARRATED,
            DEFAULT_MIN_SCORE
        ));
        assert_eq!(hits.len(), 1, "exact");

        let partial = "narrated video";
        let mut hits = vec![lesson(NARRATED, overlap_score(NARRATED, partial))];
        assert!(!apply_relevance_floor(
            &mut hits,
            partial,
            DEFAULT_MIN_SCORE
        ));
        assert_eq!(hits.len(), 1, "partial");

        // `narration` shares no exact word with `narrated`, so the snapshot
        // score is the placeholder above the old no-subject floor. The stem
        // still names the subject, and the row stays.
        let mut hits = vec![lesson(NARRATED, 0.21)];
        assert!(!apply_relevance_floor(
            &mut hits,
            "GPRidge video narration",
            DEFAULT_MIN_SCORE
        ));
        assert_eq!(hits.len(), 1, "stemmed title");
        let mut hits = vec![lesson("A narrated ridge walk.", 0.21)];
        assert!(!apply_relevance_floor(
            &mut hits,
            "narration",
            DEFAULT_MIN_SCORE
        ));
        assert_eq!(hits.len(), 1, "stemmed word");
    }

    #[test]
    fn a_stemmed_hit_does_not_outrank_the_exact_word() {
        let score = |body: &str, query: &str| {
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
        let exact = score(NARRATED, "narrated");
        let stemmed = score(NARRATED, "narration");
        assert!(exact + 1e-9 >= stemmed, "exact {exact} stemmed {stemmed}");
        assert!(stemmed > 0.0, "{stemmed}");
    }

    #[test]
    fn a_real_landing_lesson_stays_and_the_joss_note_does_not() {
        let mut hits = vec![
            lesson(JOSS, overlap_score(JOSS, RIDLEY)),
            lesson(RIDLEY_LESSON, overlap_score(RIDLEY_LESSON, RIDLEY)),
        ];
        assert!(!apply_relevance_floor(&mut hits, RIDLEY, DEFAULT_MIN_SCORE));
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].text.contains("Ridley-core"), "{hits:?}");
    }

    #[test]
    fn a_stopword_or_a_single_weak_word_is_not_a_match() {
        let stop = "The notes are on the desk by the window.";
        let weak = "Please review the glossary before the talk.";
        let mut hits = vec![
            lesson(stop, overlap_score(stop, "the kiln schedule")),
            lesson(weak, overlap_score(weak, "kiln review")),
        ];
        assert!(
            hits.iter().all(|hit| hit.score > 0.0),
            "the rows do overlap: {hits:?}"
        );
        assert!(
            apply_relevance_floor(&mut hits, "the kiln schedule", DEFAULT_MIN_SCORE)
                || hits.is_empty()
        );
        let mut weak_only = vec![lesson(weak, overlap_score(weak, "kiln review"))];
        assert!(apply_relevance_floor(
            &mut weak_only,
            "kiln review",
            DEFAULT_MIN_SCORE
        ));
        let mut one = vec![lesson(weak, 1.0)];
        assert!(!apply_relevance_floor(
            &mut one,
            "review",
            DEFAULT_MIN_SCORE
        ));
        assert_eq!(one.len(), 1, "a query that is the weak word stays");
    }

    #[test]
    fn a_low_meaning_score_drops_and_a_high_one_stays() {
        let mut low = lesson("A ferry timetable for the docks.", 0.2);
        low.entities.push("cosine:0.20".into());
        let mut high = lesson("Oak Tree on Nov 23 is the morning plan.", 0.91);
        high.entities.push("semantic:1".into());
        high.entities.push("cosine:0.84".into());
        let mut hits = vec![low, high.clone()];
        assert!(!apply_relevance_floor(
            &mut hits,
            "morning plan for the oak",
            DEFAULT_MIN_SCORE
        ));
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].text.contains("Oak"), "{hits:?}");
        let mut only = vec![high];
        only[0].score = 0.2;
        only[0].entities.retain(|tag| !tag.starts_with("cosine:"));
        only[0].entities.push("cosine:0.20".into());
        assert!(apply_relevance_floor(
            &mut only,
            "xylophone plankton",
            DEFAULT_MIN_SCORE
        ));
        // A pack hit the tag search returned, whose text does not spell the tag.
        let mut tagged_hit = lesson(
            "Courier retries a dropped letter before the quay opens.",
            1.0,
        );
        tagged_hit.entities.clear();
        let mut tagged = vec![tagged_hit];
        assert!(!apply_relevance_floor(
            &mut tagged,
            "cue-tag1 mail wake",
            DEFAULT_MIN_SCORE
        ));
        assert_eq!(tagged.len(), 1);
    }

    /// Another seat's meaning hit. The fused score is 71 and the only shared
    /// word is `safety`. That is not a similarity and it does not name
    /// webdav, drain, or delete.
    #[test]
    fn a_cross_seat_meaning_hit_needs_a_distinctive_term_or_a_high_cosine() {
        let _env = crate::test_env_lock();
        unsafe { std::env::set_var("LJOS_SEAT", "ada") };
        let query = "webdav drain delete safety";
        let travel = "travel weighs safety at 1.000.";
        let mut weak = lesson(travel, 71.0);
        weak.entities = vec!["seat:travel".into(), "cosine:0.71".into()];
        weak.ballots = Some(2);
        weak.of = Some(3);
        let mut hits = vec![weak];
        assert!(
            apply_relevance_floor(&mut hits, query, DEFAULT_MIN_SCORE),
            "a generic word must not carry another seat: {hits:?}"
        );
        let mut shown = lesson(travel, 71.0);
        shown.entities = vec!["seat:travel".into(), "cosine:0.71".into()];
        let mut kept = vec![shown];
        assert!(!apply_relevance_floor(&mut kept, query, 0.0));
        assert_eq!(kept.len(), 1);

        let mut close = lesson("The copy is removed only after the new one is stored.", 0.4);
        close.entities = vec!["seat:travel".into(), "cosine:0.86".into()];
        let mut hits = vec![close];
        assert!(!apply_relevance_floor(&mut hits, query, DEFAULT_MIN_SCORE));
        assert_eq!(hits.len(), 1, "a high similarity still answers");

        let mut named = lesson(
            "WebDAV drain delete keeps the previous copy until the new one is stored.",
            0.9,
        );
        named.entities = vec!["seat:ada".into()];
        let mut other = lesson(travel, 71.0);
        other.entities = vec!["seat:travel".into(), "cosine:0.71".into()];
        let mut hits = vec![other, named];
        assert!(!apply_relevance_floor(&mut hits, query, DEFAULT_MIN_SCORE));
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].text.contains("WebDAV"), "{hits:?}");
    }
}
