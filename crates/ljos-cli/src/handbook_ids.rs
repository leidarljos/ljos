//! The ticket ids a published page may and may not name.
//!
//! The handbook runs every string it prints through [`IdScreen::redact`]:
//! lessons, preferences, rules, workflow names and titles, `--title`, and a
//! ticket's title, body and notes.

use std::collections::BTreeSet;

use icu_properties::{props::GeneralCategory, CodePointMapData};

/// The words the blocker path writes for a ticket the page does not name.
pub const REPLACEMENT: &str = "external dependency";

const MAX_PASSES: usize = 8;

/// An id longer than this may follow other letters, as in `prekiln-a1b2`.
const GLUED_FROM: usize = 7;

#[derive(Clone, Copy)]
struct Entry {
    c: char,
    start: usize,
    end: usize,
}

/// Ids the page may not name, ids it may, and the shape of an id the tracker
/// has not been asked about.
#[derive(Debug, Default)]
pub struct IdScreen {
    hidden: BTreeSet<String>,
    allowed: BTreeSet<String>,
    projects: BTreeSet<String>,
    lengths: BTreeSet<usize>,
}

impl IdScreen {
    /// `hidden` and `allowed` are ticket ids. `projects` are the project
    /// names the tracker and the router know. `lengths` are the tracker's
    /// `id_length` values; without one the tracker default of 4 is used.
    #[must_use]
    pub fn new(
        hidden: impl IntoIterator<Item = String>,
        allowed: impl IntoIterator<Item = String>,
        projects: impl IntoIterator<Item = String>,
        lengths: impl IntoIterator<Item = usize>,
    ) -> Self {
        let key = |text: String| text.to_ascii_lowercase().replace('_', "-");
        let mut lengths: BTreeSet<usize> = lengths.into_iter().map(|len| len.max(2)).collect();
        if lengths.is_empty() {
            lengths.insert(4);
        }
        Self {
            hidden: hidden.into_iter().map(key).collect(),
            allowed: allowed.into_iter().map(key).collect(),
            projects: projects
                .into_iter()
                .map(key)
                .filter(|name| !name.is_empty())
                .collect(),
            lengths,
        }
    }

    /// Replace each ticket id the page does not publish with
    /// [`REPLACEMENT`].
    ///
    /// The text is read as the public-text screen reads it. Each pass
    /// compatibility folds it, drops zero-width and format marks, makes
    /// look-alike letters and every dash ASCII, and then decodes `%XX`,
    /// `=XX`, `\uXXXX` and `&#NN;` style escapes. Passes repeat until one
    /// decodes nothing, at most eight times. Org emphasis marks
    /// (`*`, `=`, `~`) and a line break next to a `-` do not split a token,
    /// and `_` reads as `-`. A token is replaced from the start of the id
    /// to its end when it names a hidden id, or when it is a known project
    /// name, a `-`, and a suffix of the tracker's `id_length` characters
    /// from `0-9a-z`, unless it is a marked ticket of the project.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        let seq = self.read(text);
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut at = 0;
        while at < seq.len() {
            if !id_char(seq[at].c) {
                at += 1;
                continue;
            }
            let mut end = at;
            while end < seq.len() && id_char(seq[end].c) {
                end += 1;
            }
            let mut first = at;
            let mut last = end;
            while first < last && seq[first].c == '-' {
                first += 1;
            }
            while last > first && seq[last - 1].c == '-' {
                last -= 1;
            }
            if first < last {
                let token: String = seq[first..last].iter().map(|entry| entry.c).collect();
                if let Some(from) = self.names_hidden(&token) {
                    spans.push((seq[first + from].start, seq[last - 1].end));
                }
            }
            at = end;
        }
        splice(text, spans)
    }

    /// Where in `token` a hidden id starts, if it names one.
    fn names_hidden(&self, token: &str) -> Option<usize> {
        if self.allowed.contains(token) {
            return None;
        }
        for id in &self.hidden {
            for (at, _) in token.match_indices(id.as_str()) {
                let before = token[..at].chars().next_back();
                let after = token[at + id.len()..].chars().next();
                if before.is_none_or(|c| c == '-' || id.len() >= GLUED_FROM)
                    && after.is_none_or(|c| c == '-')
                {
                    return Some(at);
                }
            }
        }
        self.projects
            .iter()
            .any(|project| self.id_of(project, token))
            .then_some(0)
    }

    fn id_of(&self, project: &str, token: &str) -> bool {
        let Some(rest) = token
            .strip_prefix(project)
            .and_then(|rest| rest.strip_prefix('-'))
        else {
            return false;
        };
        let suffix = rest.split('-').next().unwrap_or("");
        self.lengths.contains(&suffix.len())
            && suffix
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
            && !self.allowed.contains(&format!("{project}-{suffix}"))
    }

    fn read(&self, text: &str) -> Vec<Entry> {
        let mut entries: Vec<Entry> = text
            .char_indices()
            .map(|(start, c)| Entry {
                c,
                start,
                end: start + c.len_utf8(),
            })
            .collect();
        for _ in 0..MAX_PASSES {
            entries = fold(&entries);
            let (next, changed) = decode_pass(&entries);
            if !changed {
                break;
            }
            entries = next;
        }
        join(fold(&entries))
    }
}

fn id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

fn splice(text: &str, mut spans: Vec<(usize, usize)>) -> String {
    spans.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (start, end) in spans {
        if start < copied {
            copied = copied.max(end);
            continue;
        }
        out.push_str(&text[copied..start]);
        out.push_str(REPLACEMENT);
        copied = end;
    }
    out.push_str(&text[copied..]);
    out
}

fn is_dash(c: char) -> bool {
    c == '-'
        || matches!(c, '\u{2212}' | '\u{02d7}' | '\u{2043}' | '\u{2053}')
        || (!c.is_ascii()
            && CodePointMapData::<GeneralCategory>::new().get(c)
                == GeneralCategory::DashPunctuation)
}

fn fold(entries: &[Entry]) -> Vec<Entry> {
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        for folded in crate::public_text::skeleton_chars(entry.c) {
            let c = if is_dash(folded) || folded == '_' {
                '-'
            } else {
                folded.to_ascii_lowercase()
            };
            out.push(Entry { c, ..*entry });
        }
    }
    out
}

/// Drop what does not split a token: Org emphasis marks, and a line break
/// (with the blanks around it) beside a `-`.
fn join(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
    let mut at = 0;
    while at < entries.len() {
        let c = entries[at].c;
        if matches!(c, '*' | '=' | '~') {
            at += 1;
            continue;
        }
        if c.is_whitespace() {
            let mut end = at;
            while end < entries.len() && entries[end].c.is_whitespace() {
                end += 1;
            }
            let broken = entries[at..end]
                .iter()
                .any(|entry| matches!(entry.c, '\n' | '\r'));
            let after_dash = out.last().is_some_and(|entry| entry.c == '-');
            let before_dash = entries.get(end).is_some_and(|entry| entry.c == '-')
                && out
                    .last()
                    .is_some_and(|entry| entry.c.is_ascii_alphanumeric());
            if broken && (after_dash || before_dash) {
                at = end;
                continue;
            }
            out.extend_from_slice(&entries[at..end]);
            at = end;
            continue;
        }
        out.push(entries[at]);
        at += 1;
    }
    out
}

fn hex(entry: Option<&Entry>) -> Option<u32> {
    entry.and_then(|entry| entry.c.to_digit(16))
}

fn hex_run(entries: &[Entry], from: usize, count: usize) -> Option<u32> {
    let mut value = 0;
    for offset in 0..count {
        value = value * 16 + hex(entries.get(from + offset))?;
    }
    Some(value)
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name.to_ascii_lowercase().as_str() {
        "hyphen" | "dash" => '\u{2010}',
        "minus" => '\u{2212}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "horbar" => '\u{2015}',
        "lowbar" | "underbar" => '_',
        "amp" => '&',
        "num" => '#',
        _ => return None,
    })
}

/// One decoding step at `at`: the character and how many entries it used.
fn decode_at(entries: &[Entry], at: usize) -> Option<(Vec<char>, usize)> {
    match entries[at].c {
        '%' => {
            let mut bytes = Vec::new();
            let mut cursor = at;
            while let Some(value) = (entries.get(cursor).is_some_and(|entry| entry.c == '%'))
                .then(|| hex_run(entries, cursor + 1, 2))
                .flatten()
            {
                bytes.push(u8::try_from(value).ok()?);
                cursor += 3;
            }
            if bytes.is_empty() {
                return None;
            }
            if let Ok(text) = String::from_utf8(bytes.clone()) {
                return Some((text.chars().collect(), cursor - at));
            }
            let byte = bytes[0];
            byte.is_ascii().then(|| (vec![char::from(byte)], 3))
        }
        '=' => {
            let value = hex_run(entries, at + 1, 2)?;
            let c = char::from_u32(value)?;
            (c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).then(|| (vec![c], 3))
        }
        '\\' => match entries.get(at + 1).map(|entry| entry.c) {
            Some('u') if entries.get(at + 2).is_some_and(|entry| entry.c == '{') => {
                let mut cursor = at + 3;
                let mut value = 0u32;
                while let Some(digit) = hex(entries.get(cursor)) {
                    value = value.checked_mul(16)?.checked_add(digit)?;
                    cursor += 1;
                }
                (cursor > at + 3 && entries.get(cursor).is_some_and(|entry| entry.c == '}'))
                    .then(|| char::from_u32(value))
                    .flatten()
                    .map(|c| (vec![c], cursor + 1 - at))
            }
            Some('u') => {
                let c = char::from_u32(hex_run(entries, at + 2, 4)?)?;
                Some((vec![c], 6))
            }
            Some('x') => {
                let c = char::from_u32(hex_run(entries, at + 2, 2)?)?;
                Some((vec![c], 4))
            }
            _ => None,
        },
        '&' => {
            if entries.get(at + 1).is_some_and(|entry| entry.c == '#') {
                let radix_16 = entries
                    .get(at + 2)
                    .is_some_and(|entry| matches!(entry.c, 'x' | 'X'));
                let from = at + if radix_16 { 3 } else { 2 };
                let radix = if radix_16 { 16 } else { 10 };
                let mut cursor = from;
                let mut value = 0u32;
                while let Some(digit) = entries
                    .get(cursor)
                    .and_then(|entry| entry.c.to_digit(radix))
                {
                    value = value.checked_mul(radix)?.checked_add(digit)?;
                    cursor += 1;
                }
                if cursor == from || cursor - from > 8 {
                    return None;
                }
                let semi = entries.get(cursor).is_some_and(|entry| entry.c == ';');
                let c = char::from_u32(value).filter(|c| *c != '\0')?;
                return Some((vec![c], cursor + usize::from(semi) - at));
            }
            let mut cursor = at + 1;
            let mut name = String::new();
            while let Some(entry) = entries.get(cursor) {
                if !entry.c.is_ascii_alphanumeric() || name.len() > 12 {
                    break;
                }
                name.push(entry.c);
                cursor += 1;
            }
            (entries.get(cursor).is_some_and(|entry| entry.c == ';'))
                .then(|| named_entity(&name))
                .flatten()
                .map(|c| (vec![c], cursor + 1 - at))
        }
        _ => None,
    }
}

fn decode_pass(entries: &[Entry]) -> (Vec<Entry>, bool) {
    let mut out = Vec::with_capacity(entries.len());
    let mut changed = false;
    let mut at = 0;
    while at < entries.len() {
        if let Some((chars, used)) = decode_at(entries, at) {
            let (start, end) = (entries[at].start, entries[at + used - 1].end);
            out.extend(chars.into_iter().map(|c| Entry { c, start, end }));
            at += used;
            changed = true;
        } else {
            out.push(entries[at]);
            at += 1;
        }
    }
    (out, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> IdScreen {
        IdScreen::new(
            ["kiln-q7zz".to_string(), "ljos-a1b2c3".to_string()],
            ["acme-keep".to_string()],
            ["acme".to_string(), "kiln".to_string(), "ljos".to_string()],
            [4, 6],
        )
    }

    #[test]
    fn prose_with_a_hyphen_is_kept() {
        let screen = screen();
        for word in [
            "utf-8",
            "covid-19",
            "x86-64",
            "e-mail",
            "sha-256",
            "top-10",
            "h-264",
            "500-mg",
            "iso-8601",
            "gpt-4o",
            "CVE-2024-3094",
            "pre-2020",
            "Nov-2026",
            "Q3-2026",
            "FY-2027",
            "RFC-3339",
            "windows-1252",
            "ubuntu-2404",
            "kiln-schedule",
            "acme-keep",
        ] {
            let text = format!("See {word} now.");
            assert_eq!(screen.redact(&text), text, "{word}");
        }
    }

    #[test]
    fn a_decoded_id_is_replaced_where_it_was_written() {
        let screen = screen();
        assert_eq!(
            screen.redact("a Kiln%252DQ7ZZ b"),
            "a external dependency b"
        );
        assert_eq!(
            screen.redact(r"x kiln\u002dq7zz. kiln&#45;q7zz."),
            "x external dependency. external dependency."
        );
        assert_eq!(
            screen.redact("kiln-\nq7zz and kiln-zzzz-more"),
            "external dependency and external dependency"
        );
        assert_eq!(
            screen.redact("ljos-abcdef, ljos-zzzz, ljos-12345"),
            "external dependency, external dependency, ljos-12345"
        );
        assert_eq!(screen.redact(""), "");
        assert_eq!(screen.redact("plain text"), "plain text");
    }

    #[test]
    fn an_escape_split_by_a_mark_or_written_wide_is_decoded() {
        let screen = IdScreen::new(
            ["software-quiet".to_string()],
            [],
            ["software".to_string()],
            [5],
        );
        for written in [
            "Software%\u{200b}2Dquiet",
            "Software\u{ff05}2Dquiet",
            "Software\u{ff05}\u{ff12}\u{ff24}quiet",
            "Software&\u{200b}#45;quiet",
            "Software\\\u{2060}u002dquiet",
            "Software%\u{200b}25\u{200b}2Dquiet",
        ] {
            assert_eq!(
                screen.redact(&format!("after {written} lands")),
                "after external dependency lands",
                "{written:?}"
            );
        }
    }
}
