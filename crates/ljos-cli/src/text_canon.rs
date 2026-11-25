//! The one reading of author text that every public-text screen uses.
//!
//! The handbook public-text screen, the handbook id screen and the workflow
//! render scan all call [`canonical`], so a character one of them reads one
//! way the others read the same way. Only a detection copy is made. The
//! author's text is never rewritten.
//!
//! [`canonical`] works in four steps.
//!
//! 1. Decode. Compatibility decomposition (NFKD) is applied to every
//!    character, default-ignorable and format characters are dropped, and one
//!    layer is decoded from
//!    percent escapes, HTML character references (numeric and every HTML5
//!    name), quoted-printable `=XX` hex pairs and soft line breaks, and
//!    `\uXXXX`, `\u{...}` and `\xXX` escapes. An escape whose bytes are not
//!    UTF-8, or that names a control character other than a line break or a
//!    tab, is left as written. Nested encodings are not decoded.
//! 2. Breaks. Every Unicode line break (`\n`, `\r`, `\r\n`, U+000B, U+000C,
//!    U+0085, U+2028, U+2029) becomes `\n`, and a tab or any space separator
//!    (U+00A0, U+1680, U+2000 to U+200A, U+202F, U+205F, U+3000) becomes a
//!    space. This runs after decoding, so a decoded `%0A` is a line break.
//! 3. Allowlist. Each character, as written and as each pass decodes it,
//!    is checked before NFKD, with a base character and the combining marks
//!    after it composed (NFC). One that is not printable ASCII, a line break,
//!    a space separator or one of [`PUBLIC_EXTRA`] is listed in
//!    [`Canon::outside`]. So a fullwidth `０`, a mathematical `𝗢` and a
//!    zero-width space are listed, though NFKD reads the first two as ASCII
//!    and drops the third. A screen refuses such text after its pattern
//!    scans have had their say, so a look-alike path is still named a path.
//! 4. Skeleton. Each character that is not ASCII is replaced with its UTS #39
//!    confusable skeleton from `unicode-security`, and combining marks are
//!    dropped. ASCII is kept as written, since the skeleton also maps `0` to
//!    `O`, `1` to `l` and `m` to `rn`. A skeleton that equals the skeleton of
//!    one or more ASCII characters is read as one of those: a digit when the
//!    nearest kept characters around it are digits, dots or colons and not
//!    letters, else a lowercase letter, else the first in ASCII order.
//!
//! [`Canon::readings`] is the text with no escape decoded and, when it
//! differs, the text with one layer decoded. A screen scans both.

use std::collections::HashMap;
use std::sync::OnceLock;

use icu_properties::{
    props::{DefaultIgnorableCodePoint, GeneralCategory},
    CodePointMapData, CodePointSetData,
};
use unicode_normalization::UnicodeNormalization as _;

/// Characters a public page may carry besides printable ASCII and a line
/// break: the characters the documentation itself uses, after NFC.
/// `tests/public_text_docs.rs` checks the list against every non-ASCII
/// character in `docs/`.
pub const PUBLIC_EXTRA: &[char] = &[
    '\u{00b7}', '\u{00e9}', '\u{00f0}', '\u{00f3}', '\u{02bc}', '\u{2026}', '\u{2212}',
];

/// The line breaks [`canonical`] reads as `\n`.
pub(crate) const LINE_BREAKS: &[char] = &[
    '\n', '\r', '\u{0b}', '\u{0c}', '\u{85}', '\u{2028}', '\u{2029}',
];

/// One character of a detection copy and the bytes of the source text it
/// was read from.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mark {
    pub c: char,
    pub start: usize,
    pub end: usize,
}

/// What [`canonical`] reads in a text.
#[derive(Clone, Debug)]
pub(crate) struct Canon {
    /// The detection copy with no escape decoded, then, when it differs, the
    /// copy with one escape layer decoded.
    pub readings: Vec<Vec<Mark>>,
    /// A character that is not ASCII was read as ASCII.
    pub mixed: bool,
    /// Each character outside the public allowlist and the byte offset in
    /// the source text it was read from, in source order.
    pub outside: Vec<(char, usize)>,
}

impl Canon {
    /// The last reading, which has one escape layer decoded.
    #[cfg(test)]
    pub(crate) fn decoded(&self) -> &[Mark] {
        self.readings.last().map_or(&[], Vec::as_slice)
    }
}

/// Whether `c` is printable ASCII, a line break, or in [`PUBLIC_EXTRA`].
#[must_use]
pub fn allowed(c: char) -> bool {
    matches!(c, ' '..='~' | '\n') || PUBLIC_EXTRA.contains(&c)
}

pub(crate) fn is_invisible(c: char) -> bool {
    if c.is_ascii() {
        return false;
    }
    let ignorable = CodePointSetData::new::<DefaultIgnorableCodePoint>();
    let gc = CodePointMapData::<GeneralCategory>::new();
    ignorable.contains(c) || gc.get(c) == GeneralCategory::Format || c == '\u{180E}'
}

fn is_nonspacing(c: char) -> bool {
    !c.is_ascii()
        && CodePointMapData::<GeneralCategory>::new().get(c) == GeneralCategory::NonspacingMark
}

fn is_space(c: char) -> bool {
    c == '\t'
        || (!c.is_ascii()
            && CodePointMapData::<GeneralCategory>::new().get(c) == GeneralCategory::SpaceSeparator)
}

/// NFKC with invisible and format characters removed. A fullwidth `＊`
/// reads as `*` and a split `g\u{200b}pt` as `gpt`. Org structure probes
/// read this copy, because Emacs reads look-alike letters as themselves.
#[must_use]
pub fn detection_copy(text: &str) -> String {
    text.nfkc().filter(|c| !is_invisible(*c)).collect()
}

/// Read `text` as every public-text screen reads it. See the module
/// comment for the steps.
///
pub(crate) fn canonical(text: &str) -> Canon {
    let source: Vec<Mark> = text
        .char_indices()
        .map(|(start, c)| Mark {
            c,
            start,
            end: start + c.len_utf8(),
        })
        .collect();
    let mut outside = Vec::new();
    let plain = decompose(&source, &mut outside);
    let decoded = decode(&plain, &mut outside);
    let plain = breaks(&plain);
    let decoded = breaks(&decoded);
    outside.sort_by_key(|(_, at)| *at);
    outside.dedup_by_key(|(_, at)| *at);
    let (plain, mut mixed) = skeleton(&plain);
    let (decoded, decoded_mixed) = skeleton(&decoded);
    mixed |= decoded_mixed;
    let same = plain.len() == decoded.len() && plain.iter().zip(&decoded).all(|(a, b)| a.c == b.c);
    let readings = if same {
        vec![plain]
    } else {
        vec![plain, decoded]
    };
    Canon {
        readings,
        mixed,
        outside,
    }
}

/// The decoded reading of `text` as one string, as [`canonical`] reads it.
///
#[cfg(test)]
pub(crate) fn canonical_text(text: &str) -> String {
    canonical(text)
        .decoded()
        .iter()
        .map(|mark| mark.c)
        .collect()
}

/// The reading cut at each `\n`, with the line breaks left out.
pub(crate) fn lines(marks: &[Mark]) -> Vec<&[Mark]> {
    marks.split(|mark| mark.c == '\n').collect()
}

/// Whether a screen that glues `left` to `right` keeps a space between
/// them. A line break can cut a token in two only between two characters a
/// token may contain. Sentence punctuation followed by a capital starts a
/// new token.
pub(crate) fn keeps_space(left: &[char], right: &[char]) -> bool {
    let token = |c: &char| c.is_ascii_alphanumeric() || "._-/:@+=~%".contains(*c);
    let (Some(last), Some(first)) = (left.last(), right.first()) else {
        return false;
    };
    !token(last) || !token(first) || (matches!(last, '.' | '!' | '?') && first.is_ascii_uppercase())
}

/// Push the characters of `marks` outside the allowlist onto `out`, once
/// each base character and the combining marks after it are composed (NFC),
/// so `e` and U+0301 read as `é`. A line break or a space separator is
/// read as `\n` or a space and passes.
fn outside_of(marks: &[Mark], out: &mut Vec<(char, usize)>) {
    let mut at = 0;
    while at < marks.len() {
        let mut end = at + 1;
        while end < marks.len() && is_combining(marks[end].c) {
            end += 1;
        }
        let cluster: String = marks[at..end].iter().map(|mark| mark.c).collect();
        let passes = |c: &char| allowed(*c) || LINE_BREAKS.contains(c) || is_space(*c);
        if let Some(c) = cluster.nfc().find(|c| !passes(c)) {
            out.push((c, marks[at].start));
        }
        at = end;
    }
}

fn is_combining(c: char) -> bool {
    !c.is_ascii()
        && matches!(
            CodePointMapData::<GeneralCategory>::new().get(c),
            GeneralCategory::NonspacingMark
                | GeneralCategory::SpacingMark
                | GeneralCategory::EnclosingMark
        )
}

/// NFKD of each character, with invisible and format characters dropped.
/// Each character is checked against the allowlist as it arrives, before
/// NFKD, so a mathematical `𝗢` or a fullwidth `０` is outside the allowlist
/// although it reads as ASCII.
fn decompose(marks: &[Mark], outside: &mut Vec<(char, usize)>) -> Vec<Mark> {
    outside_of(marks, outside);
    let mut out = Vec::with_capacity(marks.len());
    for mark in marks {
        if mark.c.is_ascii() {
            out.push(*mark);
            continue;
        }
        for c in std::iter::once(mark.c).nfkd() {
            if !is_invisible(c) {
                out.push(Mark { c, ..*mark });
            }
        }
    }
    out
}

fn decode(marks: &[Mark], outside: &mut Vec<(char, usize)>) -> Vec<Mark> {
    let (decoded, _) = decode_pass(marks);
    decompose(&decoded, outside)
}

fn breaks(marks: &[Mark]) -> Vec<Mark> {
    let mut out: Vec<Mark> = Vec::with_capacity(marks.len());
    for (at, mark) in marks.iter().enumerate() {
        let c = if mark.c == '\r' && marks.get(at + 1).is_some_and(|next| next.c == '\n') {
            continue;
        } else if LINE_BREAKS.contains(&mark.c) {
            '\n'
        } else if is_space(mark.c) {
            ' '
        } else {
            mark.c
        };
        out.push(Mark { c, ..*mark });
    }
    out
}

/// Printable ASCII characters keyed by their skeleton.
fn ascii_by_skeleton() -> &'static HashMap<String, Vec<char>> {
    static TABLE: OnceLock<HashMap<String, Vec<char>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: HashMap<String, Vec<char>> = HashMap::new();
        for c in ' '..='~' {
            let key: String = unicode_security::skeleton(&c.to_string()).collect();
            table.entry(key).or_default().push(c);
        }
        table
    })
}

#[derive(Clone, Copy)]
enum Slot {
    Kept(Mark),
    Choice(Mark, &'static [char]),
}

fn skeleton(marks: &[Mark]) -> (Vec<Mark>, bool) {
    let table = ascii_by_skeleton();
    let mut slots: Vec<Slot> = Vec::with_capacity(marks.len());
    let mut mixed = false;
    let push = |slots: &mut Vec<Slot>, mark: Mark, key: &str| -> bool {
        match table.get(key).map(Vec::as_slice) {
            Some([one]) => slots.push(Slot::Kept(Mark { c: *one, ..mark })),
            Some(many) => slots.push(Slot::Choice(mark, many)),
            None => return false,
        }
        true
    };
    for mark in marks {
        if mark.c.is_ascii() {
            slots.push(Slot::Kept(*mark));
            continue;
        }
        let direct = match mark.c {
            '\u{00b7}' | '\u{06d4}' => Some("."),
            '\u{00f0}' => Some("d"),
            '\u{02bc}' => Some("'"),
            '\u{2014}' | '\u{2212}' => Some("-"),
            '\u{2026}' => Some("..."),
            _ => None,
        };
        if let Some(ascii) = direct {
            slots.extend(ascii.chars().map(|c| Slot::Kept(Mark { c, ..*mark })));
            mixed = true;
            continue;
        }
        let shape: String = unicode_security::skeleton(&mark.c.to_string()).collect();
        if push(&mut slots, *mark, &shape) {
            mixed = true;
            continue;
        }
        for c in shape.chars() {
            if is_nonspacing(c) {
                continue;
            }
            if c.is_ascii() && push(&mut slots, *mark, &c.to_string()) {
                mixed = true;
                continue;
            }
            slots.push(Slot::Kept(Mark { c, ..*mark }));
        }
    }
    let out = (0..slots.len())
        .map(|at| match slots[at] {
            Slot::Kept(mark) => mark,
            Slot::Choice(mark, choices) => Mark {
                c: choose(&slots, at, choices),
                ..mark
            },
        })
        .collect();
    (out, mixed)
}

/// The nearest kept character on one side of `at`, within the word.
fn neighbour(slots: &[Slot], mut at: usize, forward: bool) -> Option<char> {
    loop {
        at = if forward {
            at.checked_add(1).filter(|next| *next < slots.len())?
        } else {
            at.checked_sub(1)?
        };
        match slots[at] {
            Slot::Kept(mark) if mark.c.is_whitespace() => return None,
            Slot::Kept(mark) => return Some(mark.c),
            Slot::Choice(..) => {}
        }
    }
}

fn choose(slots: &[Slot], at: usize, choices: &[char]) -> char {
    let around = [neighbour(slots, at, false), neighbour(slots, at, true)];
    let numeric = !around.iter().flatten().any(char::is_ascii_alphabetic)
        && around
            .iter()
            .flatten()
            .any(|c| c.is_ascii_digit() || matches!(c, '.' | ':'));
    let digit = choices.iter().copied().find(char::is_ascii_digit);
    let lower = choices.iter().copied().find(char::is_ascii_lowercase);
    let letter = choices.iter().copied().find(char::is_ascii_alphabetic);
    match (numeric, digit) {
        (true, Some(d)) => d,
        _ => lower.or(letter).unwrap_or(choices[0]),
    }
}

fn hex(mark: Option<&Mark>) -> Option<u8> {
    mark.and_then(|mark| mark.c.to_digit(16))
        .and_then(|d| u8::try_from(d).ok())
}

fn hex_byte(marks: &[Mark], from: usize) -> Option<u8> {
    Some(hex(marks.get(from))? * 16 + hex(marks.get(from + 1))?)
}

/// Whether a decoded character may stand in for its escape. A control
/// character other than a tab or a line break is left as written, so
/// `n=10` or `%07` does not turn into something a page could not show.
fn decodable(c: char) -> bool {
    !c.is_control() || c == '\t' || LINE_BREAKS.contains(&c)
}

/// The text of `bytes` when it is UTF-8 with only decodable characters.
fn decoded_bytes(bytes: &[u8]) -> Option<Vec<char>> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.chars().all(decodable).then(|| text.chars().collect())
}

/// A run of `lead`-prefixed hex byte escapes at `at`, each `width` marks
/// long: `%XX`, `=XX` or `\xXX`. The run is decoded only when its bytes are
/// UTF-8.
fn byte_run(marks: &[Mark], at: usize, lead: &[char], width: usize) -> Option<(Vec<char>, usize)> {
    let mut bytes = Vec::new();
    let mut cursor = at;
    while bytes.len() < 4
        && lead
            .iter()
            .enumerate()
            .all(|(offset, c)| marks.get(cursor + offset).is_some_and(|mark| mark.c == *c))
    {
        let Some(byte) = hex_byte(marks, cursor + lead.len()) else {
            break;
        };
        bytes.push(byte);
        cursor += width;
    }
    while !bytes.is_empty() {
        if let Some(chars) = decoded_bytes(&bytes) {
            return Some((chars, bytes.len() * width));
        }
        bytes.pop();
    }
    None
}

/// `=` and the blanks after it before a line break: a quoted-printable soft
/// line break.
fn soft_break(marks: &[Mark], at: usize) -> Option<usize> {
    let mut cursor = at + 1;
    while marks
        .get(cursor)
        .is_some_and(|mark| matches!(mark.c, ' ' | '\t'))
    {
        cursor += 1;
    }
    let first = marks.get(cursor)?.c;
    if !LINE_BREAKS.contains(&first) {
        return None;
    }
    let crlf = first == '\r' && marks.get(cursor + 1).is_some_and(|mark| mark.c == '\n');
    Some(cursor + 1 + usize::from(crlf) - at)
}

/// An HTML character reference at `at`: `&#NN;`, `&#xHH;` or a named one.
fn reference(marks: &[Mark], at: usize) -> Option<(Vec<char>, usize)> {
    if marks.get(at + 1).is_some_and(|mark| mark.c == '#') {
        let radix_16 = marks
            .get(at + 2)
            .is_some_and(|mark| matches!(mark.c, 'x' | 'X'));
        let from = at + if radix_16 { 3 } else { 2 };
        let radix = if radix_16 { 16 } else { 10 };
        let mut cursor = from;
        let mut value = 0u32;
        while let Some(digit) = marks.get(cursor).and_then(|mark| mark.c.to_digit(radix)) {
            value = value.checked_mul(radix)?.checked_add(digit)?;
            cursor += 1;
        }
        if cursor == from || cursor - from > 8 {
            return None;
        }
        let semi = marks.get(cursor).is_some_and(|mark| mark.c == ';');
        let c = char::from_u32(value).filter(|c| decodable(*c))?;
        return Some((vec![c], cursor + usize::from(semi) - at));
    }
    let mut cursor = at + 1;
    let mut name = String::from("&");
    while let Some(mark) = marks.get(cursor) {
        if !mark.c.is_ascii_alphanumeric() || name.len() > 32 {
            break;
        }
        name.push(mark.c);
        cursor += 1;
    }
    if name.len() < 2 || marks.get(cursor).is_none_or(|mark| mark.c != ';') {
        return None;
    }
    name.push(';');
    let text = html_escape::decode_html_entities(&name);
    (text != name && text.chars().all(decodable)).then(|| (text.chars().collect(), cursor + 1 - at))
}

/// One decoding step at `at`: the characters and how many marks it used.
fn decode_at(marks: &[Mark], at: usize) -> Option<(Vec<char>, usize)> {
    match marks[at].c {
        '%' => byte_run(marks, at, &['%'], 3),
        '=' => soft_break(marks, at)
            .map(|used| (Vec::new(), used))
            .or_else(|| byte_run(marks, at, &['='], 3)),
        '&' => reference(marks, at),
        '\\' => match marks.get(at + 1).map(|mark| mark.c) {
            Some('u') if marks.get(at + 2).is_some_and(|mark| mark.c == '{') => {
                let mut cursor = at + 3;
                let mut value = 0u32;
                while let Some(digit) = marks.get(cursor).and_then(|mark| mark.c.to_digit(16)) {
                    value = value.checked_mul(16)?.checked_add(digit)?;
                    cursor += 1;
                }
                (cursor > at + 3 && marks.get(cursor).is_some_and(|mark| mark.c == '}'))
                    .then(|| char::from_u32(value))
                    .flatten()
                    .filter(|c| decodable(*c))
                    .map(|c| (vec![c], cursor + 1 - at))
            }
            Some('u') => {
                let value = (0..4).try_fold(0u32, |value, offset| {
                    Some(value * 16 + u32::from(hex(marks.get(at + 2 + offset))?))
                })?;
                let c = char::from_u32(value).filter(|c| decodable(*c))?;
                Some((vec![c], 6))
            }
            Some('x') => byte_run(marks, at, &['\\', 'x'], 4),
            _ => None,
        },
        _ => None,
    }
}

fn decode_pass(marks: &[Mark]) -> (Vec<Mark>, bool) {
    let mut out = Vec::with_capacity(marks.len());
    let mut changed = false;
    let mut at = 0;
    while at < marks.len() {
        if let Some((chars, used)) = decode_at(marks, at) {
            let (start, end) = (marks[at].start, marks[at + used - 1].end);
            out.extend(chars.into_iter().map(|c| Mark { c, start, end }));
            at += used;
            changed = true;
        } else {
            out.push(marks[at]);
            at += 1;
        }
    }
    (out, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> String {
        canonical_text(text)
    }

    #[test]
    fn one_written_escape_layer_is_read() {
        let _env_read = crate::test_env_read_lock();
        for (written, want) in [
            ("a%\u{200b}2Db", "a-b"),
            ("a\u{ff05}2Db", "a-b"),
            ("a\u{ff05}\u{ff12}\u{ff24}b", "a-b"),
            ("a%\u{200b}25\u{200b}2Db", "a%2Db"),
            ("a&\u{200b}#45;b", "a-b"),
            ("a\\\u{2060}u002db", "a-b"),
            ("a&amp;#45;b", "a&#45;b"),
            ("a%2Fhome%2Fx", "a/home/x"),
        ] {
            assert_eq!(read(written), want, "{written:?}");
        }
    }

    #[test]
    fn quoted_printable_decodes_any_hex_pair_and_soft_breaks() {
        let _env_read = crate::test_env_read_lock();
        for (written, want) in [
            ("=2Fhome=2Falice", "/home/alice"),
            ("10=2E0=2E0=2E5", "10.0.0.5"),
            ("alice=40example.com", "alice@example.com"),
            ("ali=\nce", "alice"),
            ("ali= \r\nce", "alice"),
            ("caf=C3=A9", "cafe"),
            ("width=20", "width "),
            ("n=10 and port=80", "n=10 and port=80"),
            ("color=FF0000", "color=FF0000"),
        ] {
            assert_eq!(read(written), want, "{written:?}");
        }
    }

    #[test]
    fn every_html5_named_reference_is_decoded() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(read("&sol;home&sol;x&commat;y&period;z"), "/home/x@y.z");
        let canon = canonical("&acy;");
        assert_eq!(canon.outside, vec![('\u{0430}', 0)]);
        assert_eq!(read("&bogus; &amp"), "&bogus; &amp");
    }

    #[test]
    fn the_skeleton_folds_slashes_dots_and_look_alikes() {
        let _env_read = crate::test_env_read_lock();
        for (written, want) in [
            ("\u{2215}home\u{2044}u\u{29f8}x", "/home/u/x"),
            ("\u{2571}a\u{30ce}b\u{2216}c", "/a/b\\c"),
            ("10\u{0660}0\u{0701}0\u{0660}5", "10.0.0.5"),
            ("p\u{0430}ss\u{0435}r", "passer"),
            ("1\u{041e}.\u{041e}.0.5", "10.0.0.5"),
            ("/h\u{041e}me", "/hOme"),
            ("a\u{0399}ice", "alice"),
            ("leiðarljós café", "leidarljos cafe"),
            ("deep\u{02bc}seek", "deep'seek"),
            ("a\u{06d4}b", "a.b"),
            ("well\u{2014}known", "well-known"),
        ] {
            assert_eq!(read(written), want, "{written:?}");
        }
    }

    #[test]
    fn every_line_break_and_space_is_read_alike() {
        let _env_read = crate::test_env_read_lock();
        let text = "a\rb\r\nc\u{0b}d\u{0c}e\u{85}f\u{2028}g\u{2029}h%0Ai";
        assert_eq!(read(text), "a\nb\nc\nd\ne\nf\ng\nh\ni");
        assert_eq!(read("a\u{a0}b\u{2003}c\u{3000}d\te"), "a b c d e");
        assert!(canonical("a\u{2028}b").outside.is_empty());
    }

    #[test]
    fn a_character_outside_the_allowlist_is_reported() {
        let _env_read = crate::test_env_read_lock();
        for (text, outside) in [
            ("plain ascii", None),
            ("leiðarljós café", None),
            ("cafe\u{0301}", None),
            ("\u{0430}", Some('\u{0430}')),
            ("x\u{0338}", Some('\u{0338}')),
            ("g\u{200b}pt", Some('\u{200b}')),
            ("%D0%B0", Some('\u{0430}')),
            ("a\u{06d4}b", Some('\u{06d4}')),
            ("\u{2019}", Some('\u{2019}')),
            ("bell\u{7}", Some('\u{7}')),
        ] {
            let found = canonical(text).outside.first().map(|(c, _)| *c);
            assert_eq!(found, outside, "{text:?}");
        }
    }

    #[test]
    fn only_one_escape_layer_is_decoded() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(read("%252Fhome"), "%2Fhome");
        assert_eq!(read("&amp;sol;home"), "&sol;home");
    }

    #[test]
    fn a_decoded_character_spans_its_escape() {
        let _env_read = crate::test_env_read_lock();
        let canon = canonical("x%2Dy");
        let dash = canon.decoded().iter().find(|mark| mark.c == '-').unwrap();
        assert_eq!((dash.start, dash.end), (1, 4));
        assert_eq!(canon.readings.len(), 2);
    }
}
