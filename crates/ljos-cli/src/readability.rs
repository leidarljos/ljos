//! The hint printed when a stored lesson reads hard.
//!
//! The pack scores the text as stored. The Automated Readability Index on a
//! short note moves when one word changes, and it counts a path, an id, a
//! code span, a commit SHA and a version as words. This scores again after
//! those are removed. Under the floor below, the hint stays quiet. Above it,
//! the line says the lesson was saved, then names the sentence to change.

use serde_json::{Map, Value};

/// Words under which the grade is noise, after the tokens below are removed.
///
/// A lesson under this many words draws no hint. One more word moves the
/// score by half a point, so the number is not an edit.
pub const MIN_WORDS: usize = 20;

/// A sentence under this many words is short.
///
/// One or two short sentences are not a passage. The hint stays quiet even
/// when the formula's number is high.
pub const SHORT_SENTENCE_WORDS: usize = 20;

/// The aim. The same ceiling the pack uses (`prose::MAX_GRADE`).
pub const TARGET: f64 = packset_core::prose::MAX_GRADE;

/// Characters of the sentence a hint quotes. A longer sentence is cut on a
/// word and marked with `...`.
const QUOTE_CHARS: usize = 80;

/// What the hint tells the person to do.
struct Hint {
    grade: f64,
    /// The sentence as stored, not the copy the score used.
    sentence: String,
    advice: &'static str,
}

/// The grade after noise tokens are removed, and the sentence to quote when
/// the hint should be printed.
struct Scored {
    grade: Option<f64>,
    flagged: Option<String>,
    advice: Option<&'static str>,
}

/// The one line for a readability warning.
///
/// `None` when the lesson is short, or the score after stripping is at or
/// under [`TARGET`]. The line starts with the saved id.
#[must_use]
pub fn reading_line(body: &Value) -> Option<String> {
    let text = body["text"].as_str().unwrap_or("");
    let hint = hint(text)?;
    let id = short_id(body["id"].as_str().unwrap_or("?"));
    let quote = trim_quote(&hint.sentence);
    Some(format!(
        "{id} was saved{}. {}: \"{quote}\". The grade is {} (higher is harder to read), and the aim is {}.",
        saved_where(body),
        hint.advice,
        format_score(hint.grade),
        format_score(TARGET),
    ))
}

/// Put `grade`, `target` and `flagged_sentence` on `body["prose"]`.
///
/// `grade` is the score after identifiers, paths, code spans, SHAs and
/// version strings are removed, or null when that text is under eight
/// words. `target` is [`TARGET`]. `flagged_sentence` is the sentence the
/// hint names, or null when the hint stays quiet.
pub fn attach(body: &mut Value) {
    let text = body["text"].as_str().unwrap_or("").to_string();
    let scored = score(&text);
    let Some(map) = body.as_object_mut() else {
        return;
    };
    let prose = map
        .entry("prose")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(prose) = prose.as_object_mut() else {
        return;
    };
    match scored.grade {
        Some(grade) => {
            prose.insert("grade".into(), number(grade));
        }
        None => {
            prose.insert("grade".into(), Value::Null);
        }
    }
    // The aim is a whole number. JSON keeps it as one.
    prose.insert("target".into(), Value::from(TARGET.round() as i64));
    match scored.flagged {
        Some(sentence) => {
            prose.insert("flagged_sentence".into(), Value::String(sentence));
        }
        None => {
            prose.insert("flagged_sentence".into(), Value::Null);
        }
    }
}

fn hint(text: &str) -> Option<Hint> {
    let scored = score(text);
    Some(Hint {
        grade: scored.grade?,
        sentence: scored.flagged?,
        advice: scored.advice?,
    })
}

fn score(text: &str) -> Scored {
    let originals = sentences_of(text);
    let mut kept: Vec<(String, String, usize)> = Vec::new();
    for sentence in originals {
        let stripped = strip_sentence(&sentence);
        let words = packset_core::prose::assess(&stripped).words;
        if words == 0 {
            continue;
        }
        kept.push((sentence, stripped, words));
    }
    let joined = kept
        .iter()
        .map(|(_, stripped, _)| stripped.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let report = packset_core::prose::assess(&joined);
    let grade = report.grade.map(round2);
    let lengths: Vec<usize> = kept.iter().map(|(_, _, words)| *words).collect();
    if quiet(&lengths, report.words) {
        return Scored {
            grade,
            flagged: None,
            advice: None,
        };
    }
    let Some(grade) = grade else {
        return Scored {
            grade: None,
            flagged: None,
            advice: None,
        };
    };
    if grade <= TARGET {
        return Scored {
            grade: Some(grade),
            flagged: None,
            advice: None,
        };
    }
    let Some((sentence, stripped)) = worst(&kept) else {
        return Scored {
            grade: Some(grade),
            flagged: None,
            advice: None,
        };
    };
    Scored {
        grade: Some(grade),
        flagged: Some(sentence),
        advice: Some(advice_for(&stripped)),
    }
}

/// One or two short sentences, or under [`MIN_WORDS`], are not scored for a hint.
fn quiet(lengths: &[usize], words: usize) -> bool {
    if words < MIN_WORDS {
        return true;
    }
    lengths.len() <= 2 && lengths.iter().all(|n| *n < SHORT_SENTENCE_WORDS)
}

fn sentences_of(text: &str) -> Vec<String> {
    let parts = packset_core::prose::sentences(text);
    if parts.is_empty() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trimmed.to_string()]
        }
    } else {
        parts
    }
}

/// The sentence whose own grade is highest. A sentence too short to grade
/// loses to one that has a grade.
fn worst(kept: &[(String, String, usize)]) -> Option<(String, String)> {
    let mut best: Option<(f64, usize, String, String)> = None;
    for (sentence, stripped, words) in kept {
        let grade = packset_core::prose::assess(stripped)
            .grade
            .map(round2)
            .unwrap_or(f64::NEG_INFINITY);
        let replace = match &best {
            None => true,
            Some((have, have_words, _, _)) => {
                grade > *have || (grade == *have && words > have_words)
            }
        };
        if replace {
            best = Some((grade, *words, sentence.clone(), stripped.clone()));
        }
    }
    best.map(|(_, _, sentence, stripped)| (sentence, stripped))
}

/// Split when the sentence is long. Ask for shorter words when the words
/// themselves carry the score. The two terms are the ones in the index:
/// `0.5 * words` and `4.71 * letters`.
fn advice_for(stripped: &str) -> &'static str {
    let report = packset_core::prose::assess(stripped);
    let words = report.words.max(1) as f64;
    let sentences = report.sentences.max(1) as f64;
    let grade = report.grade.unwrap_or(0.0);
    let length_term = 0.5 * (words / sentences);
    let word_term = grade + 21.43 - length_term;
    // Everyday prose is about five letters a word and fifteen words a sentence.
    let length_excess = length_term - (0.5 * 15.0);
    let word_excess = word_term - (4.71 * 5.0);
    if length_excess > word_excess {
        "Split this sentence"
    } else {
        "Use shorter words in"
    }
}

fn saved_where(body: &Value) -> String {
    if body["kept"].as_str() == Some("local") {
        return " on this machine, not in the pack".to_string();
    }
    match body["copies"].as_u64() {
        Some(1) => " as a shorter copy".to_string(),
        Some(n) => format!(" as {n} shorter copies"),
        None => String::new(),
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn format_score(value: f64) -> String {
    let mut text = format!("{:.2}", round2(value));
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn number(value: f64) -> Value {
    serde_json::Number::from_f64(round2(value))
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn trim_quote(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= QUOTE_CHARS {
        return flat;
    }
    let keep = QUOTE_CHARS - 3;
    let mut out: String = flat.chars().take(keep).collect();
    if let Some(space) = out.rfind(' ') {
        if space > keep / 2 {
            out.truncate(space);
        }
    }
    out.push_str("...");
    out
}

/// `sentence` with code spans, paths, ids, SHAs and versions removed, and a
/// stop on the end so it still counts as one sentence.
fn strip_sentence(sentence: &str) -> String {
    let bare = strip_tokens(&strip_code_spans(sentence));
    let bare = bare.trim();
    if bare.is_empty() {
        return String::new();
    }
    if bare.ends_with(['.', '!', '?']) {
        bare.to_string()
    } else {
        format!("{bare}.")
    }
}

fn strip_code_spans(text: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for ch in text.chars() {
        if ch == '`' {
            in_code = !in_code;
            out.push(' ');
            continue;
        }
        if !in_code {
            out.push(ch);
        }
    }
    out
}

fn strip_tokens(text: &str) -> String {
    text.split_whitespace()
        .filter(|token| !noise_token(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn noise_token(token: &str) -> bool {
    let core = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '.' | ','
                | ';'
                | ':'
                | '!'
                | '?'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '"'
                | '\''
                | '`'
        )
    });
    if core.is_empty() {
        return false;
    }
    is_path(core) || is_sha(core) || is_version(core) || is_identifier(core)
}

fn is_path(core: &str) -> bool {
    core.contains('/') || core.contains('\\') || core.contains("::")
}

/// A commit SHA: seven or more hex digits with a digit in them, or eight or
/// more hex digits. `defaced` is seven letters and stays a word.
fn is_sha(core: &str) -> bool {
    if core.len() < 7 || !core.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    core.len() >= 8 || core.bytes().any(|b| b.is_ascii_digit())
}

/// `0.43.1` and `v0.29`. A single decimal such as `2.5` stays, and so does `v1`.
fn is_version(core: &str) -> bool {
    let (prefixed, rest) = match core.strip_prefix(['v', 'V']) {
        Some(rest) if rest.starts_with(|ch: char| ch.is_ascii_digit()) => (true, rest),
        _ => (false, core),
    };
    let mut parts = rest.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    if first.is_empty() || !first.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut dots = 0usize;
    for part in parts {
        let part = part.split(['-', '+']).next().unwrap_or(part);
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        dots += 1;
    }
    dots > 0 && (prefixed || dots >= 2)
}

/// `ljos-ab12`, `XDG_DATA_HOME`, `proj-1a2b`. A hyphenated word with no digit,
/// such as `well-known`, stays.
fn is_identifier(core: &str) -> bool {
    if let Some(rest) = core.strip_prefix("ljos-") {
        return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    }
    if core.contains('_')
        && core.len() > 1
        && core.bytes().any(|b| b.is_ascii_alphabetic())
        && core.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return true;
    }
    core.contains('-')
        && core.bytes().any(|b| b.is_ascii_digit())
        && core.bytes().any(|b| b.is_ascii_alphabetic())
        && core.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LONG_WORDS: &str = "The configuration management subsystem unfortunately necessitates extraordinarily complicated initialization procedures whenever administrators reconfigure distributed deployment environments across multiple availability zones.";

    const SPLIT_ME: &str = "People should record each lesson with plain words and one short sentence so later readers understand the whole point without extra work from anyone.";

    const SHORT: &str = "The payments service retries webhooks three times with exponential backoff starting at 2 seconds.";

    fn warned(id: &str, text: &str) -> Value {
        json!({
            "id": id,
            "text": text,
            "prose": {"warning": "readability grade 19 exceeds 14; use shorter sentences and shorter words"}
        })
    }

    #[test]
    fn the_aim_is_the_packs_ceiling() {
        assert_eq!(TARGET, 14.0);
        assert_eq!(MIN_WORDS, 20);
        assert_eq!(SHORT_SENTENCE_WORDS, 20);
    }

    #[test]
    fn a_short_lesson_draws_no_hint() {
        let grade = packset_core::prose::assess(SHORT).grade;
        assert!(grade.is_some_and(|g| g > TARGET), "{grade:?}");
        assert_eq!(reading_line(&warned("abcd1234", SHORT)), None);
        let two = "The lexical default is BM25+. Search uses that order.";
        assert_eq!(reading_line(&warned("abcd1234", two)), None);
        let nineteen = LONG_WORDS.trim_end_matches(" zones.");
        assert!(
            nineteen.split_whitespace().count() < SHORT_SENTENCE_WORDS,
            "{nineteen}"
        );
        assert_eq!(reading_line(&warned("abcd1234", nineteen)), None);
    }

    #[test]
    fn paths_ids_and_shas_do_not_raise_the_hint() {
        let prose = "Keep the default and the file holds the key.";
        let noise = " crates/ljos-cli/src/lib.rs ljos-ab12 f3340bce9166923ff769b703939b0c78f74837a8 0.43.1 `packset_core::prose::assess` XDG_DATA_HOME";
        let mut text = String::from("Keep the default and the file holds the key");
        for _ in 0..30 {
            if packset_core::prose::assess(&text)
                .grade
                .is_some_and(|grade| grade > TARGET)
            {
                break;
            }
            text.push_str(noise);
        }
        text.push('.');
        let raw = packset_core::prose::assess(&text).grade;
        assert!(
            raw.is_some_and(|grade| grade > TARGET),
            "the fixture must be one the pack would warn on, got {raw:?}"
        );
        assert_eq!(reading_line(&warned("ffff0000", &text)), None);
        assert_eq!(reading_line(&warned("ffff0000", prose)), None);
        let plain = strip_sentence("See well-known defaced names in ljos-ab12.");
        assert!(plain.contains("well-known"), "{plain}");
        assert!(plain.contains("defaced"), "{plain}");
        assert!(!plain.contains("ljos-ab12"), "{plain}");
    }

    #[test]
    fn a_long_winded_lesson_names_the_sentence_and_starts_with_the_id() {
        let body = warned("abcdef1234567890", LONG_WORDS);
        let line = reading_line(&body).expect("hint");
        assert!(line.starts_with("abcdef12 was saved."), "{line}");
        assert!(line.contains("Use shorter words in:"), "{line}");
        assert!(line.contains("configuration management"), "{line}");
        assert!(line.contains("The grade is "), "{line}");
        assert!(line.contains("the aim is 14"), "{line}");
        assert!(!line.contains('\n'), "{line}");
        let grade_at = line.find("The grade is ").expect("grade");
        let aim_at = line.find("the aim is 14").expect("aim");
        assert_eq!(line.matches("The grade is ").count(), 1, "{line}");
        assert_eq!(line.matches("the aim is ").count(), 1, "{line}");
        assert!(grade_at < aim_at, "{line}");

        let split = reading_line(&warned("abcdef1234567890", SPLIT_ME)).expect("split");
        assert!(split.starts_with("abcdef12 was saved."), "{split}");
        assert!(split.contains("Split this sentence:"), "{split}");
        assert!(split.contains("record each lesson"), "{split}");
    }

    #[test]
    fn json_carries_the_grade_the_aim_and_the_sentence() {
        let mut body = warned("abcdef1234567890", LONG_WORDS);
        attach(&mut body);
        assert!(body["prose"]["grade"].as_f64().is_some_and(|g| g > TARGET));
        assert_eq!(body["prose"]["target"], json!(14));
        let sentence = body["prose"]["flagged_sentence"]
            .as_str()
            .expect("sentence");
        assert!(
            sentence.contains("extraordinarily complicated"),
            "{sentence}"
        );
        assert!(body["prose"]["warning"].as_str().is_some());

        let mut short = warned("abcd1234", SHORT);
        attach(&mut short);
        assert!(short["prose"]["grade"].as_f64().is_some());
        assert_eq!(short["prose"]["target"], json!(14));
        assert!(short["prose"]["flagged_sentence"].is_null());
    }

    #[test]
    fn a_local_copy_still_says_it_was_saved() {
        let mut body = warned("abcdef1234567890", LONG_WORDS);
        body["kept"] = json!("local");
        let line = reading_line(&body).expect("hint");
        assert!(
            line.starts_with("abcdef12 was saved on this machine, not in the pack."),
            "{line}"
        );
    }
}
