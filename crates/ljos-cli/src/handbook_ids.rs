//! The ticket ids a published page may and may not name.
//!
//! The handbook runs every string it prints through [`IdScreen::redact`]:
//! lessons, preferences, rules, workflow names and titles, `--title`, and a
//! ticket's title, body and notes.

use std::collections::BTreeSet;

use icu_properties::{props::GeneralCategory, CodePointMapData};

use crate::text_canon::{canonical, Mark};

/// The words the blocker path writes for a ticket the page does not name.
pub const REPLACEMENT: &str = "external dependency";

/// An id longer than this may follow other letters, as in `prekiln-a1b2`.
const GLUED_FROM: usize = 7;

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
    /// The text is read through [`canonical`], as every public-text screen
    /// reads it: one escape layer decoded, every line
    /// break and space separator read alike, and the UTS #39 skeleton of
    /// each character that is not ASCII. Both of its readings, with no
    /// escape decoded and with one escape layer decoded, are searched. Every
    /// dash and `_` then read as `-`. Org emphasis marks (`*`, `=`, `~`) do
    /// not split a token, and neither does a run of blanks beside a `-` that
    /// has a line break or a space other than U+0020 and a tab in it. A
    /// token is replaced from the start of the id to its end when it names a
    /// hidden id, or when it is a known project name, a `-`, and a suffix of
    /// the tracker's `id_length` characters from `0-9a-z`, unless it is a
    /// marked ticket of the project.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        let canon = canonical(text);
        let mut spans: Vec<(usize, usize)> = Vec::new();
        for reading in &canon.readings {
            self.find(&join(text, id_fold(text, reading.clone())), &mut spans);
        }
        splice(text, spans)
    }

    fn find(&self, seq: &[Mark], spans: &mut Vec<(usize, usize)>) {
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

/// Every dash and `_` as `-`, and letters lowercased. A dash is judged on
/// the character as written too, since some dash skeletons are ambiguous.
fn id_fold(text: &str, mut marks: Vec<Mark>) -> Vec<Mark> {
    for mark in &mut marks {
        let written = text[mark.start..mark.end].chars().count() == 1
            && text[mark.start..mark.end].chars().all(is_dash);
        mark.c = if written || is_dash(mark.c) || mark.c == '_' {
            '-'
        } else {
            mark.c.to_ascii_lowercase()
        };
    }
    marks
}

/// Whether a blank was written as a line break or a space other than U+0020
/// and a tab. [`canonical`] reads all of them as `\n` or a space, so the
/// source bytes tell them apart.
fn breaks_line(text: &str, mark: &Mark) -> bool {
    mark.c == '\n' || !matches!(&text[mark.start..mark.end], " " | "\t")
}

/// Drop what does not split a token: Org emphasis marks, and a run of
/// blanks beside a `-` that has a line break in it.
fn join(text: &str, marks: Vec<Mark>) -> Vec<Mark> {
    let mut out: Vec<Mark> = Vec::with_capacity(marks.len());
    let mut at = 0;
    while at < marks.len() {
        let c = marks[at].c;
        if matches!(c, '*' | '=' | '~') {
            at += 1;
            continue;
        }
        if c.is_whitespace() {
            let mut end = at;
            while end < marks.len() && marks[end].c.is_whitespace() {
                end += 1;
            }
            let broken = marks[at..end].iter().any(|mark| breaks_line(text, mark));
            let after_dash = out.last().is_some_and(|mark| mark.c == '-');
            let before_dash = marks.get(end).is_some_and(|mark| mark.c == '-')
                && out
                    .last()
                    .is_some_and(|mark| mark.c.is_ascii_alphanumeric());
            if broken && (after_dash || before_dash) {
                at = end;
                continue;
            }
            out.extend_from_slice(&marks[at..end]);
            at = end;
            continue;
        }
        out.push(marks[at]);
        at += 1;
    }
    out
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
    fn one_decoded_id_layer_is_replaced_where_it_was_written() {
        let screen = screen();
        assert_eq!(screen.redact("a Kiln%252DQ7ZZ b"), "a Kiln%252DQ7ZZ b");
        assert_eq!(
            screen.redact(r"x kiln\u002dq7zz. kiln&#45;q7zz."),
            "x external dependency. external dependency."
        );
        assert_eq!(
            screen.redact("kiln-\nq7zz and kiln-zzzz-more"),
            "external dependency and external dependency"
        );
        for broken in [
            "\u{2028}",
            "\u{0b}",
            "\u{a0}",
            "\u{85}",
            "\r\n",
            "%0A",
            "\u{2029} ",
        ] {
            assert_eq!(
                screen.redact(&format!("kiln-{broken}q7zz.")),
                "external dependency.",
                "{broken:?}"
            );
        }
        assert_eq!(screen.redact("kiln- q7zz"), "kiln- q7zz");
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
        ] {
            assert_eq!(
                screen.redact(&format!("after {written} lands")),
                "after external dependency lands",
                "{written:?}"
            );
        }
        let nested = "Software%\u{200b}25\u{200b}2Dquiet";
        assert_eq!(
            screen.redact(&format!("after {nested} lands")),
            format!("after {nested} lands")
        );
    }
}
