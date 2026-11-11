//! Copies of a memory the pack will store.
//!
//! Packset's atom gate is [`packset_core::prose::refuse`] for
//! [`packset_core::ProseRole::Atom`], plus [`packset_core::record::TEXT_SOFT_CAP`]
//! and [`packset_core::record::validate`]. A note of kind `note` skips the
//! sentence cap, and a pack older than that retries the note as a `summary`,
//! which does not. Every copy is cut so both gates take it: at most
//! [`packset_core::prose::MAX_ATOM_SENTENCES`] sentences, no sentence of
//! [`packset_core::prose::VERY_HARD_WORDS`] or more, and the grade and adverb
//! ceilings that `refuse` enforces. The ticket keeps the text as it was written.

use serde_json::{Map, Value};

/// Whether `text` is a secret, an instruction to the model, or text a
/// reader cannot see. Packset 0.17.6 stores those and labels them. ljos
/// still keeps them out of an export, a split, and an alias. The words
/// are the ones a refusal used to carry, so a caller that matches them
/// still matches.
pub(crate) fn reject_unsafe(text: &str) -> Result<(), String> {
    let flagged = packset_core::record::flags(text);
    if flagged
        .iter()
        .any(|(label, _)| *label == packset_core::record::INVISIBLE_UNICODE)
    {
        return Err("invisible unicode is rejected".into());
    }
    match packset_core::record::secret_kind(text) {
        Some(kind) if kind == packset_core::record::GENERIC_SECRET => {
            Err("credential-shaped text is rejected".into())
        }
        Some(kind) => Err(format!("credential-shaped text is rejected ({kind})")),
        None if packset_core::record::looks_like_injection(text) => Err(
            "text that instructs the model reading it is rejected; write the claim it stands for"
                .into(),
        ),
        None => Ok(()),
    }
}

/// Whether `text` passes the pack's atom gate as an agent lesson and as a note.
#[must_use]
pub fn pack_copy_ok(text: &str) -> bool {
    gate_refusal(text).is_none()
}

/// Why the pack's atom gate would turn `text` away as an agent lesson or a
/// note, in the pack's words. `None` when it passes.
#[must_use]
pub fn gate_refusal(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return Some("atom text is required".into());
    }
    let cap = packset_core::record::TEXT_SOFT_CAP;
    if text.chars().count() > cap {
        return Some(format!(
            "atom text is past the soft cap of {cap} characters"
        ));
    }
    if let Err(why) = packset_core::prose_refuse(text, packset_core::ProseRole::Atom) {
        return Some(why.to_string());
    }
    let mut lesson = Map::new();
    lesson.insert("kind".into(), Value::String("lesson".into()));
    lesson.insert("level".into(), Value::String("explicit".into()));
    lesson.insert("text".into(), Value::String(text.to_string()));
    lesson.insert("workspace".into(), Value::String("seat".into()));
    lesson.insert("origin".into(), Value::String("agent-derived".into()));
    if let Err(why) = packset_core::record::validate(&mut lesson) {
        return Some(why.to_string());
    }
    let mut note = Map::new();
    note.insert(
        "kind".into(),
        Value::String(packset_core::record::NOTE_KIND.into()),
    );
    note.insert("level".into(), Value::String("explicit".into()));
    note.insert("text".into(), Value::String(text.to_string()));
    note.insert("workspace".into(), Value::String("seat".into()));
    note.insert("origin".into(), Value::String("agent-derived".into()));
    note.insert("issue".into(), Value::String("probe".into()));
    packset_core::record::validate(&mut note)
        .err()
        .map(|why| why.to_string())
}

/// `text` cut into copies [`pack_copy_ok`] accepts.
///
/// A text that already passes is returned unchanged, so a short lesson stays
/// one post. A secret or an instruction is not trimmed into a stored fragment.
#[must_use]
pub fn pack_copies(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || reject_unsafe(trimmed).is_err() {
        return Vec::new();
    }
    if pack_copy_ok(trimmed) {
        return vec![trimmed.to_string()];
    }
    let parts = packset_core::prose::sentences(trimmed);
    let mut clauses = Vec::new();
    if parts.is_empty() {
        clauses.extend(shrink_clause(trimmed, 0));
    } else {
        for part in &parts {
            clauses.extend(shrink_clause(part, 0));
        }
    }
    pack_groups(&clauses)
        .into_iter()
        .filter(|copy| pack_copy_ok(copy))
        .collect()
}

fn sentence_count(text: &str) -> usize {
    let n = packset_core::prose::sentences(text).len();
    if n == 0 {
        1
    } else {
        n
    }
}

fn end_sentence(text: &str) -> String {
    let text = text.trim().trim_end_matches([',', ';', ':', ' ']).trim();
    if text.is_empty() {
        return String::new();
    }
    if text.ends_with(['.', '!', '?']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

const SPLITS: &[&str] = &["; ", ", and ", ", but ", ", or ", " and ", " but ", " or "];

fn shrink_clause(text: &str, depth: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() || reject_unsafe(text).is_err() {
        return Vec::new();
    }
    if pack_copy_ok(text) {
        return vec![text.to_string()];
    }
    if depth > 24 {
        return chunk_words(text);
    }
    for sep in SPLITS {
        let Some((left, right)) = text.split_once(sep) else {
            continue;
        };
        let left = left.trim();
        let right = right.trim();
        if left.is_empty() || right.is_empty() {
            continue;
        }
        if left.chars().count() >= text.chars().count()
            || right.chars().count() >= text.chars().count()
        {
            continue;
        }
        let mut out = shrink_clause(left, depth + 1);
        out.extend(shrink_clause(right, depth + 1));
        if !out.is_empty() && out.iter().all(|part| pack_copy_ok(part)) {
            return out;
        }
    }
    chunk_words(text)
}

/// `text` cut into the fewest runs of near-equal length that each pass as
/// one sentence. Even runs leave no one-word tail for [`pack_groups`] to
/// glue onto a cut sentence.
fn chunk_words(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for pieces in 1..=words.len() {
        let copies: Vec<String> = words
            .chunks(words.len().div_ceil(pieces))
            .map(|run| end_sentence(&run.join(" ")))
            .collect();
        if copies.iter().all(|copy| {
            copy.chars().count() <= packset_core::record::TEXT_SOFT_CAP
                && pack_copy_ok(copy)
                && sentence_count(copy) <= 1
        }) {
            return copies;
        }
    }
    Vec::new()
}

fn pack_groups(parts: &[String]) -> Vec<String> {
    let max = packset_core::prose::MAX_ATOM_SENTENCES;
    let mut out = Vec::new();
    let mut cur = String::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() || !pack_copy_ok(part) {
            continue;
        }
        if cur.is_empty() {
            cur = part.to_string();
            continue;
        }
        let joined = format!("{cur} {part}");
        if sentence_count(&joined) <= max && pack_copy_ok(&joined) {
            cur = joined;
        } else {
            out.push(std::mem::take(&mut cur));
            cur = part.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_copies(text: &str) -> Vec<String> {
        let copies = pack_copies(text);
        assert!(!copies.is_empty(), "no copy of {text:?}");
        for copy in &copies {
            assert!(pack_copy_ok(copy), "{copy}");
            assert!(
                copy.chars().count() <= packset_core::record::TEXT_SOFT_CAP,
                "{copy}"
            );
            assert!(
                packset_core::prose::sentences(copy).len()
                    <= packset_core::prose::MAX_ATOM_SENTENCES,
                "{copy}"
            );
            for sentence in packset_core::prose::sentences(copy) {
                let words = sentence.split_whitespace().count();
                assert!(
                    words < packset_core::prose::VERY_HARD_WORDS,
                    "{words} words: {sentence}"
                );
            }
        }
        copies
    }

    #[test]
    fn a_short_sentence_is_one_unchanged_copy() {
        let text = "pin the fuse";
        assert_eq!(pack_copies(text), vec![text.to_string()]);
    }

    #[test]
    fn a_long_sentence_is_split_under_the_word_limit() {
        // Two sentences fit in one atom. A sentence longer than that still
        // has to become more than one copy.
        let words = packset_core::prose::VERY_HARD_WORDS * 2 + 5;
        let text = "word ".repeat(words);
        let text = text.trim();
        let copies = assert_copies(text);
        assert!(copies.len() > 1, "{copies:?}");
        assert_ne!(copies[0], text);
    }

    #[test]
    fn three_sentences_are_not_posted_as_one_atom() {
        let text = "The parser kept the hyphen. The test pins it. The release ships it.";
        let copies = assert_copies(text);
        assert!(copies.len() > 1, "{copies:?}");
        assert!(copies.iter().all(|copy| copy != text), "{copies:?}");
    }

    #[test]
    fn a_note_past_the_soft_cap_is_cut_into_copies_inside_it() {
        let text = "The cat sat on the mat. ".repeat(40);
        assert!(text.chars().count() > packset_core::record::TEXT_SOFT_CAP);
        let copies = assert_copies(&text);
        assert!(copies.len() > 1, "{copies:?}");
        assert!(copies.iter().all(|copy| copy != text.trim()));
    }
}
