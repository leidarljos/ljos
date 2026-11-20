//! Whether author text may appear on a public page.
//!
//! The screen detects and refuses. It never rewrites: a caller that gets a
//! class back keeps its original string and leaves the item out.
//!
//! Each line is turned into a detection copy: NFKC, invisible and format
//! characters dropped, combining marks dropped, confusable letters folded to
//! ASCII. The copy is cut into tokens (words, CamelCase pieces, paths, URLs,
//! numbers with their units, versions, Rust paths, `name@semver`) and each
//! token is classified, so an ordinary version or a `std::ops::Add` is a
//! token of its own kind before any rule for addresses or secrets sees it.
//! Two adjacent lines are also tried joined, so a secret or an address split
//! by a line break is still found.
//!
//! One function, [`scan`], answers for a whole block of lines. [`classify`]
//! and [`classify_id`] are thin cases of it, so an item check and the check
//! of the text that is finally written cannot disagree.

use std::net::Ipv6Addr;

use icu_properties::{
    props::{DefaultIgnorableCodePoint, GeneralCategory},
    CodePointMapData, CodePointSetData,
};
use unicode_normalization::UnicodeNormalization as _;

include!("public_text_fold.rs");

/// What a public export screens against besides the fixed patterns.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// Seat names known to the tracker or the pack.
    pub seats: Vec<String>,
    /// Also refuse ljos's own vocabulary (`seat`, `hook`, `packset` ...). A
    /// page about ljos itself leaves this off.
    pub internal: bool,
}

/// The kind of private pattern a string matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// A model, product or tooling word.
    Term,
    /// A seat name.
    Seat,
    /// A filesystem path or a `file:` link.
    Path,
    /// A key, token, password or other credential.
    Secret,
    /// An IPv4 or IPv6 address, or a private host name.
    Address,
    /// An email address.
    Mailbox,
    /// Org markup that Emacs would run or include when it exports the page.
    Org,
}

impl Class {
    /// The word a diagnostic prints for this class.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Term => "term",
            Self::Seat => "seat",
            Self::Path => "path",
            Self::Secret => "secret",
            Self::Address => "address",
            Self::Mailbox => "email",
            Self::Org => "org-markup",
        }
    }
}

/// How one line of a block is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Prose.
    Text,
    /// An identifier the export writes itself. An id made only of hex digits
    /// is an opaque key, not a credential.
    Id,
    /// A line the export writes itself that is Org structure, such as the
    /// `#+begin_src dot` around a graph. It is read like prose, except that
    /// its leading `#+` keyword is not refused.
    Structure,
}

/// One line of a block, with the way it is read.
#[derive(Clone, Copy, Debug)]
pub struct Line<'a> {
    pub text: &'a str,
    pub kind: Kind,
}

impl<'a> Line<'a> {
    /// A line of prose.
    #[must_use]
    pub fn text(text: &'a str) -> Self {
        Self {
            text,
            kind: Kind::Text,
        }
    }

    /// An identifier written by the export.
    #[must_use]
    pub fn id(text: &'a str) -> Self {
        Self {
            text,
            kind: Kind::Id,
        }
    }

    /// A line of Org structure written by the export.
    #[must_use]
    pub fn structure(text: &'a str) -> Self {
        Self {
            text,
            kind: Kind::Structure,
        }
    }
}

/// Whether `text` matches a private pattern.
#[must_use]
pub fn refuses(text: &str, screen: &Screen) -> bool {
    classify(text, screen).is_some()
}

/// The first private pattern `text` matches, if any. The caller keeps its
/// original string: only a detection copy is normalized.
#[must_use]
pub fn classify(text: &str, screen: &Screen) -> Option<Class> {
    let flat = text.replace("\r\n", "\n");
    let lines: Vec<Line<'_>> = flat
        .split(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'])
        .filter(|line| !line.trim().is_empty())
        .map(Line::text)
        .collect();
    scan(&lines, screen).into_iter().flatten().next()
}

/// Like [`classify`], for an identifier the export writes itself.
#[must_use]
pub fn classify_id(id: &str, screen: &Screen) -> Option<Class> {
    scan(&[Line::id(id)], screen).into_iter().next().flatten()
}

/// The detection copy of `text`: NFKC with invisible and format characters
/// removed. A fullwidth `＊` reads as `*` and a split `g\u{200b}pt` as `gpt`.
#[must_use]
pub fn detection_copy(text: &str) -> String {
    text.nfkc().filter(|c| !is_invisible(*c)).collect()
}

/// One character as the screen reads it: compatibility-folded, invisible and
/// combining marks dropped, look-alike letters mapped to ASCII. A character
/// that is only invisible gives nothing.
pub(crate) fn skeleton_chars(c: char) -> Vec<char> {
    let mut out = Vec::new();
    let compatible: String = c.to_string().nfkc().filter(|c| !is_invisible(*c)).collect();
    for c in compatible.nfd() {
        if is_nonspacing(c) || is_invisible(c) {
            continue;
        }
        let c = fold_lookalike(c);
        if !c.is_ascii() {
            if let Some(skeleton) = fold_confusable(c) {
                out.extend(skeleton.chars());
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// The class of each line of a block, or `None` for a clean line.
///
/// A line is read alone and then with its neighbour, as one string with no
/// separator and as one string with a space. A hit that needs the join marks
/// both lines. A PEM block marks every line from its `BEGIN` line to its
/// `END` line.
#[must_use]
pub fn scan(lines: &[Line<'_>], screen: &Screen) -> Vec<Option<Class>> {
    let seats = seat_keys(screen);
    let preps: Vec<Prep> = lines.iter().map(|line| Prep::new(line.text)).collect();
    let mut out: Vec<Option<Class>> = lines
        .iter()
        .zip(&preps)
        .map(|(line, prep)| class_of(prep, &seats, screen.internal, line.kind))
        .collect();
    let mut inside = false;
    for (index, prep) in preps.iter().enumerate() {
        let begins = prep.l.contains("begin") && private_key(&prep.l);
        if !inside && begins {
            inside = true;
            out[index] = Some(Class::Secret);
            continue;
        }
        if inside {
            out[index] = Some(Class::Secret);
            if prep.l.contains("-----end") || prep.l.contains("private key-----") {
                inside = false;
            }
        }
    }
    let mut run: Vec<usize> = Vec::new();
    for index in 0..=lines.len() {
        let eligible = index < lines.len()
            && out[index].is_none()
            && lines[index].kind != Kind::Id
            && !preps[index].s.trim().is_empty();
        if eligible {
            run.push(index);
            continue;
        }
        let blank = index < lines.len()
            && out[index].is_none()
            && lines[index].kind != Kind::Id
            && preps[index].s.trim().is_empty();
        if blank {
            continue;
        }
        mark_joined(&run, &preps, &seats, screen.internal, &mut out);
        run.clear();
    }
    out
}

/// The most lines tried as one string, and how many are shared by the next
/// try, so a secret cut into pieces by line breaks is found wherever the cuts
/// fall.
const WINDOW: usize = 200;
const OVERLAP: usize = 20;

fn window_class(
    members: &[usize],
    preps: &[Prep],
    seats: &[String],
    internal: bool,
) -> Option<Class> {
    let mut glued = Prep::empty();
    let mut spaced = Prep::empty();
    for (at, &index) in members.iter().enumerate() {
        glued = glued.joined(&preps[index], None);
        spaced = spaced.joined(&preps[index], if at == 0 { None } else { Some(' ') });
    }
    joined_class(&glued, seats, internal).or_else(|| joined_class(&spaced, seats, internal))
}

/// Try consecutive clean lines as one string, and mark the fewest lines
/// that still make the match.
fn mark_joined(
    run: &[usize],
    preps: &[Prep],
    seats: &[String],
    internal: bool,
    out: &mut [Option<Class>],
) {
    if run.len() < 2 {
        return;
    }
    let mut start = 0;
    loop {
        let end = (start + WINDOW).min(run.len());
        let chunk = &run[start..end];
        let mut pending: Vec<&[usize]> = vec![chunk];
        while let Some(part) = pending.pop() {
            if part.len() < 2 || part.iter().any(|&index| out[index].is_some()) {
                continue;
            }
            let Some(class) = window_class(part, preps, seats, internal) else {
                continue;
            };
            let (mut from, mut to) = (0, part.len());
            while to - from > 2
                && window_class(&part[from + 1..to], preps, seats, internal).is_some()
            {
                from += 1;
            }
            while to - from > 2
                && window_class(&part[from..to - 1], preps, seats, internal).is_some()
            {
                to -= 1;
            }
            for &index in &part[from..to] {
                out[index] = Some(class);
            }
            pending.push(&part[..from]);
            pending.push(&part[to..]);
        }
        if end == run.len() {
            break;
        }
        start = end - OVERLAP;
    }
}

fn joined_class(prep: &Prep, seats: &[String], internal: bool) -> Option<Class> {
    if term_hit(prep, internal) {
        Some(Class::Term)
    } else if seat_hit(prep, seats) {
        Some(Class::Seat)
    } else if secret_hit(prep, false) {
        Some(Class::Secret)
    } else if mailbox_hit(prep) {
        Some(Class::Mailbox)
    } else {
        None
    }
}

fn class_of(prep: &Prep, seats: &[String], internal: bool, kind: Kind) -> Option<Class> {
    let identifier = kind == Kind::Id;
    if term_hit(prep, internal) {
        Some(Class::Term)
    } else if seat_hit(prep, seats) {
        Some(Class::Seat)
    } else if path_hit(prep) {
        Some(Class::Path)
    } else if secret_hit(prep, identifier) {
        Some(Class::Secret)
    } else if address_hit(prep) {
        Some(Class::Address)
    } else if mailbox_hit(prep) {
        Some(Class::Mailbox)
    } else if kind != Kind::Id && org_construct(&prep.fold, kind == Kind::Structure) {
        Some(Class::Org)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// The detection copy
// ---------------------------------------------------------------------------

fn is_invisible(c: char) -> bool {
    if c.is_ascii() {
        return false;
    }
    let ignorable = CodePointSetData::new::<DefaultIgnorableCodePoint>();
    let gc = CodePointMapData::<GeneralCategory>::new();
    ignorable.contains(c) || gc.get(c) == GeneralCategory::Format || c == '\u{180E}'
}

fn is_nonspacing(c: char) -> bool {
    if c.is_ascii() {
        return false;
    }
    CodePointMapData::<GeneralCategory>::new().get(c) == GeneralCategory::NonspacingMark
}

/// Letters that read as an ASCII letter but are not in the confusables map
/// for the form they take, and the look-alikes of `/`, `\` and `.`.
fn fold_lookalike(c: char) -> char {
    match c {
        '\u{0410}' | '\u{0430}' | '\u{0391}' | '\u{03b1}' | '\u{0251}' | '\u{1d00}' => 'a',
        '\u{0412}' | '\u{0432}' | '\u{044c}' => 'b',
        '\u{0421}' | '\u{0441}' | '\u{03f9}' | '\u{03f2}' | '\u{1d04}' => 'c',
        '\u{0501}' | '\u{0257}' | '\u{1d05}' => 'd',
        '\u{0415}' | '\u{0435}' | '\u{0395}' | '\u{03b5}' | '\u{0454}' | '\u{1d07}' => 'e',
        '\u{0493}' => 'f',
        '\u{0261}' => 'g',
        '\u{0397}' | '\u{04bb}' | '\u{041d}' | '\u{043d}' => 'h',
        '\u{0399}' | '\u{0406}' | '\u{0456}' | '\u{0457}' | '\u{03b9}' | '\u{026a}'
        | '\u{0131}' => 'i',
        '\u{0408}' | '\u{0458}' | '\u{029d}' => 'j',
        '\u{039a}' | '\u{041a}' | '\u{043a}' | '\u{03ba}' => 'k',
        '\u{04cf}' | '\u{217c}' | '\u{029f}' => 'l',
        '\u{039c}' | '\u{041c}' | '\u{043c}' | '\u{1d0d}' => 'm',
        '\u{0578}' | '\u{041f}' | '\u{043f}' | '\u{03bd}' => 'n',
        '\u{039f}' | '\u{041e}' | '\u{043e}' | '\u{03bf}' | '\u{03c3}' | '\u{1d0f}' => 'o',
        '\u{03a1}' | '\u{0420}' | '\u{0440}' | '\u{03c1}' => 'p',
        '\u{051b}' => 'q',
        '\u{0433}' | '\u{0280}' => 'r',
        '\u{0455}' | '\u{a731}' => 's',
        '\u{03a4}' | '\u{0422}' | '\u{0442}' | '\u{03c4}' | '\u{1d1b}' => 't',
        '\u{03c5}' | '\u{057d}' | '\u{1d1c}' => 'u',
        '\u{0475}' => 'v',
        '\u{051d}' | '\u{03c9}' => 'w',
        '\u{03a7}' | '\u{0425}' | '\u{0445}' | '\u{03c7}' => 'x',
        '\u{03a5}' | '\u{0423}' | '\u{0443}' | '\u{04af}' | '\u{03b3}' => 'y',
        '\u{1d22}' => 'z',
        '\u{2215}' | '\u{2044}' | '\u{29f8}' | '\u{ff0f}' => '/',
        '\u{29f9}' | '\u{ff3c}' => '\\',
        '\u{3002}' | '\u{2024}' | '\u{ff61}' | '\u{ff0e}' => '.',
        other => other,
    }
}

/// The detection copy of one line, in two alignments: `fold` keeps case and
/// `l` is the same characters lowercased, one for one.
struct Prep {
    /// A letter was folded from a look-alike in another script.
    mixed: bool,
    /// Lines were run together with no separator, so a word may be two.
    glued: bool,
    /// Several lines were read as one string.
    joined: bool,
    /// Where in `fold` one line ends and the next begins with no separator.
    seams: Vec<usize>,
    fold: Vec<char>,
    s: String,
    l: String,
}

impl Prep {
    fn new(text: &str) -> Self {
        let compatible: String = text.nfkc().filter(|c| !is_invisible(*c)).collect();
        let mut fold: Vec<char> = Vec::with_capacity(compatible.len());
        let mut mixed = false;
        for c in compatible.nfd() {
            if is_nonspacing(c) || is_invisible(c) {
                continue;
            }
            let original = c;
            let c = fold_lookalike(c);
            if !c.is_ascii() {
                if let Some(skeleton) = fold_confusable(c) {
                    mixed = true;
                    fold.extend(skeleton.chars());
                    continue;
                }
            } else if !original.is_ascii() {
                mixed = true;
            }
            fold.push(c);
        }
        let mut prep = Self::from_fold(fold);
        prep.mixed = mixed;
        prep
    }

    fn empty() -> Self {
        Self::from_fold(Vec::new())
    }

    fn from_fold(fold: Vec<char>) -> Self {
        let low: Vec<char> = fold
            .iter()
            .map(|c| c.to_lowercase().next().unwrap_or(*c))
            .collect();
        let s = fold.iter().collect();
        let l = low.iter().collect();
        Self {
            mixed: false,
            glued: false,
            joined: false,
            seams: Vec::new(),
            fold,
            s,
            l,
        }
    }

    fn joined(&self, next: &Prep, separator: Option<char>) -> Prep {
        let mut fold = self.fold.clone();
        let mut seams = self.seams.clone();
        if separator.is_none() && !self.fold.is_empty() {
            seams.push(self.fold.len());
        }
        fold.extend(separator);
        let shift = fold.len();
        seams.extend(next.seams.iter().map(|seam| seam + shift));
        fold.extend(next.fold.iter());
        let mut prep = Self::from_fold(fold);
        prep.seams = seams;
        prep.mixed = self.mixed || next.mixed;
        prep.glued = self.glued || (separator.is_none() && !self.fold.is_empty());
        prep.joined = true;
        prep
    }
}

fn seat_keys(screen: &Screen) -> Vec<String> {
    let mut keys: Vec<String> = screen
        .seats
        .iter()
        .map(|seat| Prep::new(seat.trim()).l)
        .filter(|seat| !seat.is_empty() && !seat.chars().any(char::is_whitespace))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

// ---------------------------------------------------------------------------
// Org constructs
// ---------------------------------------------------------------------------

/// `#+` keyword lines that are inert when Emacs exports a page.
const INERT_KEYWORDS: &[&str] = &[
    "begin_example",
    "end_example",
    "begin_quote",
    "end_quote",
    "begin_verse",
    "end_verse",
    "begin_center",
    "end_center",
    "begin_comment",
    "end_comment",
    "caption",
    "name",
];

/// A line with markup that Emacs runs, includes or passes through when it
/// exports the page: a keyword line, a source block, an inline `src_` or
/// `call_`, a macro, an export snippet, an `elisp:` or `shell:` link or a
/// script tag. The words alone (`src_dir`, `call me`, `:results`) are prose.
fn org_construct(chars: &[char], structure: bool) -> bool {
    let text: String = chars.iter().collect();
    let lower = text.to_lowercase();
    let keyword_line = if structure {
        None
    } else {
        lower.trim_start().strip_prefix("#+")
    };
    if let Some(rest) = keyword_line {
        let keyword: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .collect();
        if !keyword.is_empty() && !INERT_KEYWORDS.contains(&keyword.as_str()) {
            return true;
        }
    }
    if lower.contains("{{{")
        || lower.contains("[[shell:")
        || lower.contains("[[elisp:")
        || lower.contains("<script")
        || lower.contains("<iframe")
    {
        return true;
    }
    let low: Vec<char> = lower.chars().collect();
    let mut at = 0;
    while at < low.len() {
        if low[at] == '@' && low.get(at + 1) == Some(&'@') {
            let mut end = at + 2;
            while end < low.len() && low[end].is_ascii_alphabetic() {
                end += 1;
            }
            if end > at + 2 && low.get(end) == Some(&':') {
                return true;
            }
        }
        let start_ok = at == 0 || !low[at - 1].is_alphanumeric();
        if start_ok
            && (inline_babel_at(&low, at, "src_", &['{', '['])
                || inline_babel_at(&low, at, "call_", &['(', '[']))
        {
            return true;
        }
        at += 1;
    }
    false
}

fn inline_babel_at(low: &[char], at: usize, word: &str, opens: &[char]) -> bool {
    let want: Vec<char> = word.chars().collect();
    if low.len() < at + want.len() + 1 || low[at..at + want.len()] != want[..] {
        return false;
    }
    let mut end = at + want.len();
    while end < low.len()
        && (low[end].is_ascii_alphanumeric() || matches!(low[end], '_' | '-' | '.'))
    {
        end += 1;
    }
    end > at + want.len() && low.get(end).is_some_and(|c| opens.contains(c))
}

// ---------------------------------------------------------------------------
// Words: vendor, model, product and tooling names
// ---------------------------------------------------------------------------

/// Names that are refused as the start of a word, with any suffix.
const TIER_A: &[&str] = &[
    "claude",
    "anthropic",
    "openai",
    "chatgpt",
    "copilot",
    "deepseek",
    "huggingface",
    "gemini",
    "ollama",
    "openrouter",
    "windsurf",
    "midjourney",
    "perplexity",
    "mixtral",
    "vicuna",
    "langchain",
    "llamaindex",
    "autogpt",
    "babyagi",
    "openhands",
    "sourcegraph",
    "codeium",
    "tabnine",
    "deepmind",
];

/// Names that are refused as a whole word, as a plural or before a digit.
const WORDS: &[&str] = &[
    "gpt",
    "cursorrules",
    "cursorignore",
    "composer",
    "opus",
    "sonnet",
    "codex",
    "llama",
    "qwen",
    "grok",
    "mistral",
    "cohere",
    "gemma",
    "groq",
    "cline",
    "kiro",
    "aider",
    "kimi",
    "glm",
    "ernie",
    "minimax",
    "bard",
    "xai",
    "dalle",
    "genai",
    "subagent",
    "agentic",
    "chatbot",
    "llm",
    "ai",
];

/// ljos's own vocabulary. A page about another project must not carry it, so
/// a caller that publishes such a page sets [`Screen::internal`].
const INTERNAL_WORDS: &[&str] = &[
    "bot",
    "mesh",
    "mcp",
    "hook",
    "policyd",
    "phronesis",
    "packset",
    "packsetd",
    "seat",
    "sitting",
    "deed",
    "consensus",
];

const PHRASES: &[&str] = &[
    "hugging face",
    "language model",
    "foundation model",
    "model context protocol",
    "artificial intelligence",
    "stable diffusion",
    "dall e",
    "gen ai",
    "co authored by",
    "phi 3",
    "falcon 40b",
    "yi 34b",
    "continue dev",
    "x ai",
];

fn term_hit(prep: &Prep, internal: bool) -> bool {
    let text = strip_org_entities(&prep.fold);
    let runs = alnum_runs(&text);
    let words: Vec<String> = runs
        .iter()
        .map(|&(from, to)| text[from..to].iter().collect())
        .collect();
    let lows: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
    for (word, low) in words.iter().zip(&lows) {
        if whole_hit(low, internal) || leet_hit(low) {
            return true;
        }
        if camel_pieces(word)
            .iter()
            .any(|piece| piece_hit(&piece.to_lowercase(), internal))
        {
            return true;
        }
    }
    phrase_hit(&lows)
        || concat_hit(&lows)
        || spaced_hit(&runs, &lows, &text, internal)
        || cursor_hit(&runs, &words, &lows, &text, prep.mixed)
}

/// Words that follow `cursor` in the editor's own product names.
const CURSOR_NEXT: &[&str] = &[
    "agent",
    "agents",
    "ide",
    "cli",
    "rules",
    "composer",
    "tab",
    "bugbot",
    "sdk",
    "cloud",
    "background",
    "com",
    "sh",
];

/// A lowercase `cursor` is an ordinary word. The editor is named by any other
/// casing (`Cursor`, `CURSOR`), a word that follows it (`cursor-agent`,
/// `Cursor IDE`), or a `.cursor` directory.
fn cursor_hit(
    runs: &[(usize, usize)],
    words: &[String],
    lows: &[String],
    text: &[char],
    mixed: bool,
) -> bool {
    for word in words {
        let pieces: Vec<String> = camel_pieces(word)
            .iter()
            .map(|piece| piece.to_lowercase())
            .collect();
        if pieces
            .windows(2)
            .any(|pair| pair[0] == "cursor" && CURSOR_NEXT.contains(&pair[1].as_str()))
        {
            return true;
        }
    }
    for (at, low) in lows.iter().enumerate() {
        if low != "cursor" {
            continue;
        }
        if mixed || runs.len() == 1 {
            return true;
        }
        let (from, to) = runs[at];
        if from > 0 && text[from - 1] == '.' && (from == 1 || !text[from - 2].is_alphanumeric()) {
            return true;
        }
        if let Some(next) = lows.get(at + 1) {
            let gap: Vec<char> = text[to..runs[at + 1].0].to_vec();
            let joined = gap.iter().all(|c| matches!(c, ' ' | '-' | '_' | '.')) && gap.len() == 1;
            if joined && CURSOR_NEXT.contains(&next.as_str()) {
                return true;
            }
        }
        let word = &words[at];
        if word.as_str() == "cursor" {
            continue;
        }
        return true;
    }
    false
}

fn strip_org_entities(chars: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' {
            let mut end = index + 1;
            while end < chars.len() && chars[end].is_ascii_alphabetic() {
                end += 1;
            }
            if end > index + 1 && chars.get(end) == Some(&'{') && chars.get(end + 1) == Some(&'}') {
                index = end + 2;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn alnum_runs(chars: &[char]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_alphanumeric() {
            let from = index;
            while index < chars.len() && chars[index].is_alphanumeric() {
                index += 1;
            }
            runs.push((from, index));
        } else {
            index += 1;
        }
    }
    runs
}

fn camel_pieces(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut pieces = Vec::new();
    let mut current = String::new();
    for (index, &c) in chars.iter().enumerate() {
        if index > 0 {
            let before = chars[index - 1];
            let after = chars.get(index + 1).copied();
            let split = (before.is_lowercase() && c.is_uppercase())
                || (before.is_uppercase()
                    && c.is_uppercase()
                    && after.is_some_and(char::is_lowercase))
                || (before.is_alphabetic() != c.is_alphabetic());
            if split {
                pieces.push(std::mem::take(&mut current));
            }
        }
        current.push(c);
    }
    pieces.push(current);
    pieces
}

fn o_series(low: &str) -> bool {
    let Some(rest) = low.strip_prefix('o') else {
        return false;
    };
    let mut chars = rest.chars();
    matches!(chars.next(), Some('1' | '3' | '4'))
        && matches!(chars.as_str(), "" | "mini" | "preview" | "pro")
}

fn word_matches(low: &str, term: &str, digits: bool) -> bool {
    low == term
        || low.strip_suffix('s') == Some(term)
        || (digits
            && low
                .strip_prefix(term)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c.is_ascii_digit()))
}

fn vocabulary<'a>(internal: bool) -> impl Iterator<Item = &'a &'static str> {
    WORDS
        .iter()
        .chain(INTERNAL_WORDS.iter().filter(move |_| internal))
}

fn whole_hit(low: &str, internal: bool) -> bool {
    if o_series(low) || TIER_A.iter().any(|term| low.starts_with(term)) {
        return true;
    }
    vocabulary(internal).any(|term| word_matches(low, term, true))
}

fn piece_hit(low: &str, internal: bool) -> bool {
    TIER_A.iter().any(|term| low.starts_with(term))
        || vocabulary(internal).any(|term| word_matches(low, term, false))
}

fn leet_hit(low: &str) -> bool {
    if !low.chars().any(|c| c.is_ascii_digit()) || !low.chars().any(char::is_alphabetic) {
        return false;
    }
    let mapped: String = low
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            other => other,
        })
        .collect();
    mapped != low && TIER_A.iter().any(|term| mapped.starts_with(term))
}

fn phrase_hit(lows: &[String]) -> bool {
    PHRASES.iter().any(|phrase| {
        let want: Vec<&str> = phrase.split(' ').collect();
        lows.windows(want.len()).any(|window| {
            window.iter().enumerate().all(|(at, have)| {
                have == want[at]
                    || (at + 1 == want.len() && want.len() > 1 && *have == format!("{}s", want[at]))
            })
        })
    })
}

fn concat_hit(lows: &[String]) -> bool {
    for from in 0..lows.len() {
        for count in 2..=3 {
            if from + count > lows.len() {
                continue;
            }
            let joined: String = lows[from..from + count].concat();
            if TIER_A.iter().any(|term| joined == *term) {
                return true;
            }
        }
    }
    false
}

/// Single letters set apart by one space, dot, hyphen or underscore: `C l a
/// u d e`, `C.l.a.u.d.e`, `A.I.`.
fn spaced_hit(runs: &[(usize, usize)], lows: &[String], text: &[char], internal: bool) -> bool {
    let single = |at: usize| runs[at].1 - runs[at].0 == 1;
    let mut index = 0;
    while index < runs.len() {
        if !single(index) {
            index += 1;
            continue;
        }
        let mut end = index;
        let mut dots = true;
        while end + 1 < runs.len()
            && single(end + 1)
            && runs[end + 1].0 == runs[end].1 + 1
            && matches!(text[runs[end].1], ' ' | '.' | '-' | '_')
        {
            dots &= text[runs[end].1] == '.';
            end += 1;
        }
        let count = end - index + 1;
        if count >= 2 && (count >= 4 || dots) && whole_hit(&lows[index..=end].concat(), internal) {
            return true;
        }
        index = end + 1;
    }
    false
}

fn seat_hit(prep: &Prep, seats: &[String]) -> bool {
    if seats.is_empty() {
        return false;
    }
    let idents: Vec<&str> = prep
        .l
        .split(|c: char| !(c.is_alphanumeric() || matches!(c, '-' | '_')))
        .filter(|word| !word.is_empty())
        .collect();
    let runs: Vec<&str> = prep
        .l
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    seats
        .iter()
        .any(|seat| idents.contains(&seat.as_str()) || runs.contains(&seat.as_str()))
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const ROOTS: &[&str] = &[
    "users",
    "root",
    "home",
    "workspace",
    "cursor",
    "tmp",
    "opt",
    "var",
    "etc",
    "mnt",
    "srv",
    "data",
    "media",
    "proc",
    "run",
    "volumes",
    "snap",
    "usr",
    "nix",
    "exec-daemon",
    "agent",
    "sys",
    "dev",
    "boot",
    "lib",
    "lib64",
    "bin",
    "sbin",
    "private",
    "library",
    "applications",
];

const SECRET_SEGMENTS: &[&str] = &[
    "secrets",
    "secret",
    ".ssh",
    ".aws",
    ".gnupg",
    ".kube",
    ".docker",
    ".netrc",
    "id_rsa",
    "id_ed25519",
    "credentials",
];

const SECRET_FILES: &[&str] = &[".env", ".pem", ".key", ".p12", ".pfx", ".kdbx"];

const HTTP_VERBS: &[&str] = &["get", "post", "put", "delete", "patch", "head", "options"];

/// Tools whose own directories a page may name.
const OWN_TOOLS: &[&str] = &["ljos", "deedar", "vissue", "claimdag", "graphify"];

/// Home directories named by a placeholder, not a person.
const PLACEHOLDER_HOMES: &[&str] = &[
    "you", "user", "username", "name", "me", "yourname", "youruser",
];

/// Shell start-up files every account has.
const OWN_FILES: &[&str] = &[
    "~/.config",
    "~/.local",
    "~/.cache",
    "~/.local/share",
    "~/.local/state",
    "~/.bashrc",
    "~/.zshrc",
    "~/.profile",
    "~/.bash_profile",
    "~/.config/fish/config.fish",
    "~/.config/fish/completions",
    "~/.config/elvish/rc.elv",
    "~/.zshenv",
    "~/.local/bin",
];

fn own_home_path(lc: &str) -> bool {
    if OWN_FILES.iter().any(|file| {
        lc == *file || (file.matches('/').count() > 1 && lc.starts_with(&format!("{file}/")))
    }) {
        return true;
    }
    let Some(rest) = lc.strip_prefix("~/") else {
        return false;
    };
    let mut parts = rest.split('/');
    let (Some(first), Some(second), third) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let tool = |name: Option<&str>| {
        name.is_some_and(|name| {
            OWN_TOOLS
                .iter()
                .any(|tool| name == *tool || name.starts_with(&format!("{tool}-")))
        })
    };
    (first == ".config" && tool(Some(second)))
        || (matches!(first, ".local")
            && matches!(second, "share" | "state" | "cache")
            && tool(third))
        || (first == ".cache" && tool(Some(second)))
}

fn path_hit(prep: &Prep) -> bool {
    if path_in(&prep.s) {
        return true;
    }
    let collapsed = collapse_slashes(&prep.s);
    collapsed != prep.s && path_in(&collapsed)
}

/// `/ home / ubuntu` reads as `/home/ubuntu`.
fn collapse_slashes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_whitespace() {
            let mut end = index;
            while end < chars.len() && chars[end].is_whitespace() {
                end += 1;
            }
            if chars.get(end) == Some(&'/') || out.ends_with('/') {
                index = end;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn path_in(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        let before = index
            .checked_sub(1)
            .map(|at| words[at].to_lowercase())
            .unwrap_or_default();
        for candidate in candidates(word) {
            if path_candidate(candidate, &before) {
                return true;
            }
        }
    }
    false
}

fn lead_trim(text: &str) -> &str {
    text.trim_start_matches(|c: char| {
        matches!(c, '(' | '[' | '{' | '<' | '"' | '\'' | '`' | '*' | '=')
    })
}

/// The word, and what follows each mark that can open a path inside it.
fn candidates(word: &str) -> Vec<&str> {
    let mut out = vec![lead_trim(word)];
    let bytes = word.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'-' && bytes[1].is_ascii_alphabetic() && bytes[2] == b'/' {
        out.push(&word[2..]);
    }
    for (at, c) in word.char_indices() {
        if matches!(
            c,
            '=' | ',' | ';' | '"' | '\'' | '(' | '[' | '<' | '`' | '|' | '>' | ':'
        ) {
            out.push(lead_trim(&word[at + c.len_utf8()..]));
        }
    }
    out
}

fn path_candidate(candidate: &str, before: &str) -> bool {
    let trimmed = candidate.trim_end_matches(|c: char| {
        matches!(
            c,
            '.' | ','
                | ';'
                | ':'
                | '!'
                | '?'
                | ')'
                | ']'
                | '}'
                | '"'
                | '\''
                | '`'
                | '>'
                | '*'
                | '='
        )
    });
    if trimmed.is_empty() {
        return false;
    }
    let lc = trimmed.to_lowercase();
    if let Some((_, rest)) = lc.split_once("://") {
        if lc.starts_with("file://") {
            return lc.len() > "file://".len();
        }
        let path = rest.find('/').map_or("", |at| &rest[at..]);
        return path
            .match_indices("//")
            .any(|(at, _)| absolute_path(&path[at + 1..], ""));
    }
    if let Some(target) = lc.strip_prefix("file:") {
        if target.starts_with('/')
            || target.starts_with('~')
            || target.starts_with("../")
            || target.contains('\\')
            || target.contains("/../")
        {
            return true;
        }
    }
    let bytes = lc.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        return true;
    }
    if lc.starts_with("\\\\") && lc.len() > 2 {
        return true;
    }
    if lc.starts_with('%') && (lc.contains("%\\") || lc.contains("%/")) {
        return true;
    }
    if lc.starts_with('~') {
        return home_path(&lc);
    }
    if is_variable_path(&lc) {
        return true;
    }
    if lc.starts_with("../") || lc.starts_with("..\\") || lc.contains("/../") {
        return true;
    }
    if lc.contains('/') && secret_segments(&lc) {
        return true;
    }
    absolute_path(&lc, before)
}

fn home_path(lc: &str) -> bool {
    if let Some(rest) = lc.strip_prefix("~/") {
        if rest.is_empty() {
            return false;
        }
        return !own_home_path(lc);
    }
    let name_end = lc[1..]
        .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.')))
        .map(|at| at + 1);
    name_end.is_some_and(|end| end > 1 && lc[end..].starts_with('/'))
}

fn is_variable_path(lc: &str) -> bool {
    const NAMES: &[&str] = &["home", "userprofile", "appdata", "tmpdir", "temp", "user"];
    let Some(rest) = lc.strip_prefix('$') else {
        return false;
    };
    let rest = rest.strip_prefix('{').unwrap_or(rest);
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let after = &rest[name.len()..];
    let after = after.strip_prefix('}').unwrap_or(after);
    if !((NAMES.contains(&name.as_str()) || name.starts_with("xdg_"))
        && (after.starts_with('/') || after.starts_with('\\')))
    {
        return false;
    }
    let rest = after.trim_start_matches(['/', '\\']);
    let mut parts = rest.split('/');
    let tool = |part: Option<&str>| part.is_some_and(|part| OWN_TOOLS.contains(&part));
    if name.starts_with("xdg_") && tool(parts.next()) {
        return false;
    }
    if name == "home" {
        let home = format!("~/{rest}");
        if own_home_path(&home) {
            return false;
        }
    }
    true
}

fn secret_segments(lc: &str) -> bool {
    lc.split('/').any(|segment| {
        SECRET_SEGMENTS.contains(&segment)
            || SECRET_FILES.iter().any(|file| segment.ends_with(file))
    })
}

fn absolute_path(lc: &str, before: &str) -> bool {
    if !lc.starts_with('/') || lc.starts_with("//") {
        return false;
    }
    let segments: Vec<&str> = lc[1..].split('/').filter(|part| !part.is_empty()).collect();
    if segments.len() < 2 || !ROOTS.contains(&segments[0]) {
        return false;
    }
    let system_command = match segments[0] {
        "bin" | "sbin" | "dev" => true,
        "usr" => {
            segments[1] == "bin"
                || segments[1] == "sbin"
                || (segments[1] == "local" && segments.get(2) == Some(&"bin"))
        }
        _ => false,
    };
    if system_command {
        return false;
    }
    if matches!(segments[0], "home" | "users") && PLACEHOLDER_HOMES.contains(&segments[1]) {
        return false;
    }
    if HTTP_VERBS.contains(&before) {
        return false;
    }
    !(segments[0] == "var" && segments[1] == "log")
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

fn secret_hit(prep: &Prep, identifier: bool) -> bool {
    (!identifier && !prep.joined && prep.s.split_whitespace().any(|word| word_shape(word, true)))
        || seam_run_hit(prep)
        || shapes_hit(&prep.fold)
        || credential_hit(&prep.fold, prep.l.contains("digest"), prep.glued)
        || scheme_hit(&prep.s)
        || user_pass(&prep.s)
        || command_login(&prep.s)
        || cookie_hit(&prep.fold)
        || glued_password_flag(&prep.fold)
        || private_key(&prep.l)
        || opaque_hit(&prep.s, identifier)
}

/// A run of letters and digits cut by a line break, each half too short to
/// count alone.
fn seam_run_hit(prep: &Prep) -> bool {
    let fold = &prep.fold;
    prep.seams.iter().any(|&seam| {
        let left = fold[..seam]
            .iter()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric())
            .count();
        let right = fold[seam..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric())
            .count();
        if left == 0 || right == 0 || left >= 24 || right >= 24 {
            return false;
        }
        let run: String = fold[seam - left..seam + right].iter().collect();
        mixed_run(&run) && !digest_run(&run)
    })
}

/// Whether one word looks like a credential. The screen's secret test runs
/// on every line of an export, with the word shapes of [`word_shape`] read
/// as prose. This is the same test on one word, with those shapes read as
/// an excerpt reads them, so the prompt excerpt `ljos remember` keeps and an
/// export use one matcher and differ only in how a bare marker is read.
#[must_use]
pub fn secret_word(word: &str) -> bool {
    word_shape(word, false) || secret_hit(&Prep::new(word), false)
}

const ASSIGNMENTS: &[&str] = &[
    "key=",
    "token=",
    "secret=",
    "password=",
    "passwd=",
    "key:",
    "token:",
    "secret:",
    "password:",
];

// Token prefixes are split with concat! so a secret scanner on the repository
// does not read this table as a leaked token.
const KEY_PREFIXES: &[&str] = &[
    concat!("s", "k-"),
    concat!("gh", "p_"),
    concat!("gh", "o_"),
    concat!("github", "_pat_"),
    concat!("xo", "x"),
    "akia",
    concat!("gl", "pat-"),
];

/// The word shapes beyond the screen's named formats: a `key=` or `token:`
/// style assignment, a known key prefix with any tail, and a long run of
/// mixed letters and digits. An excerpt redacts a marker whether or not a
/// value follows it and counts the letters and digits of the whole word. In
/// prose a marker needs a value after it, so `=api_key=` and `password=` on
/// their own name the variable and not a value, but `password==x` and
/// `secret::x` have one. A capitalised name before `::`, as in `Token::Eof`,
/// is a type path. A key prefix is read after the punctuation that wraps a
/// word. A run of 24 letters and digits counts when it is unbroken, or when
/// `-`, `.` or `_` break it and one piece is itself a mixed run of 8 or
/// more, unless the run is 32 or more hex digits, which the screen reads
/// with its hash context. Words joined only by `-` are prose, as in a long
/// slug.
fn word_shape(word: &str, prose: bool) -> bool {
    let lower = word.to_ascii_lowercase();
    let assigned = ASSIGNMENTS.iter().any(|marker| {
        lower
            .match_indices(marker)
            .any(|(at, _)| !prose || marker_has_value(&word[at..], marker.len()))
    });
    let unwrapped = lower.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    if assigned || KEY_PREFIXES.iter().any(|p| unwrapped.starts_with(p)) {
        return true;
    }
    if word.contains('/') {
        return false;
    }
    if !prose {
        let core: String = word.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        return mixed_run(&core);
    }
    word.split(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '.' | '_'))
        .any(prose_run)
}

/// Whether a value follows the marker at the start of `from`.
fn marker_has_value(from: &str, marker: usize) -> bool {
    let rest = &from[marker..];
    if from.starts_with(|c: char| c.is_ascii_uppercase())
        && from[..marker].ends_with(':')
        && rest.starts_with(':')
        && !rest.chars().any(|c| c.is_ascii_digit())
    {
        return false;
    }
    rest.chars().any(|c| c.is_ascii_alphanumeric())
}

fn mixed_run(core: &str) -> bool {
    core.len() >= 24
        && core.chars().any(|c| c.is_ascii_digit())
        && core.chars().any(|c| c.is_ascii_alphabetic())
}

fn digest_run(core: &str) -> bool {
    core.len() >= 32 && core.chars().all(|c| c.is_ascii_hexdigit())
}

/// A word segment read as prose: one run of letters and digits, or a run
/// that `-`, `.` or `_` break with a mixed piece of 8 or more in it.
fn prose_run(segment: &str) -> bool {
    let pieces: Vec<&str> = segment
        .split(['-', '.', '_'])
        .filter(|piece| !piece.is_empty())
        .collect();
    if pieces
        .iter()
        .any(|piece| mixed_run(piece) && !digest_run(piece))
    {
        return true;
    }
    let core: String = pieces.concat();
    mixed_run(&core)
        && !digest_run(&core)
        && pieces.iter().any(|piece| {
            piece.len() >= 8
                && piece.chars().any(|c| c.is_ascii_digit())
                && piece.chars().any(|c| c.is_ascii_alphabetic())
        })
}

fn shape_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

fn shapes_hit(chars: &[char]) -> bool {
    let mut index = 0;
    while index < chars.len() {
        if !shape_char(chars[index]) {
            index += 1;
            continue;
        }
        let from = index;
        while index < chars.len() && shape_char(chars[index]) {
            index += 1;
        }
        let token = &chars[from..index];
        for at in 0..token.len() {
            if (at == 0 || matches!(token[at - 1], '_' | '-' | '.'))
                && shape_at(&token[at..].iter().collect::<String>())
            {
                return true;
            }
        }
    }
    let glue: Vec<char> = chars
        .iter()
        .copied()
        .filter(|c| !c.is_whitespace() && !matches!(c, '*' | '`'))
        .collect();
    glued_shape(&glue)
}

const AWS_PREFIXES: &[&str] = &[
    "akia", "asia", "abia", "acca", "agpa", "aida", "aipa", "anpa", "anva", "aroa",
];

fn tail_is(text: &str, min: usize, ok: impl Fn(char) -> bool) -> bool {
    text.chars().count() >= min && text.chars().all(ok)
}

fn is_b64url(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
}

fn shape_at(text: &str) -> bool {
    let low = text.to_lowercase();
    let after = |prefix: &str| -> Option<&str> {
        low.starts_with(prefix)
            .then(|| text.get(prefix.len()..))
            .flatten()
    };
    if text.len() >= 20 && AWS_PREFIXES.iter().any(|p| low.starts_with(p)) {
        if let Some(tail) = text.get(4..20) {
            if tail.chars().all(|c| c.is_ascii_alphanumeric()) {
                return true;
            }
        }
    }
    if let Some(tail) = after("aiza") {
        if tail_is(tail, 35, is_b64url) {
            return true;
        }
    }
    if let Some(tail) = after("sk") {
        if tail.len() == 32 && tail.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    if let Some(tail) = after("xapp-") {
        if tail_is(tail, 10, is_b64url) {
            return true;
        }
    }
    if let Some(tail) = after(concat!("xo", "x")) {
        if tail
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
            && tail.len() >= 3
            && tail.chars().all(is_b64url)
        {
            return true;
        }
    }
    for (prefix, min) in [
        ("dop_v1_", 20),
        ("doo_v1_", 20),
        ("dor_v1_", 20),
        (concat!("h", "f_"), 30),
        ("pypi-", 20),
        (concat!("gh", "p_"), 20),
        (concat!("gh", "o_"), 20),
        ("ghu_", 20),
        ("ghs_", 20),
        ("ghr_", 20),
        (concat!("github", "_pat_"), 20),
        (concat!("gl", "pat-"), 10),
        ("sk_live_", 8),
        ("sk_test_", 8),
        ("rk_live_", 8),
        ("rk_test_", 8),
        ("pk_live_", 8),
        (concat!("s", "k-"), 20),
        ("npm_", 30),
        ("shpat_", 20),
        ("shpss_", 20),
        ("whsec_", 20),
        ("ya29.", 20),
    ] {
        if let Some(tail) = after(prefix) {
            if tail_is(tail, min, |c| is_b64url(c) || c == '.') {
                return true;
            }
        }
    }
    if let Some(tail) = after("key-") {
        if tail.len() == 32 && tail.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    if let Some(tail) = after("sg.") {
        if tail_is(tail, 40, |c| is_b64url(c) || c == '.') {
            return true;
        }
    }
    if text.starts_with("AAAAB3NzaC1") {
        return true;
    }
    if text.starts_with("eyJ") {
        let parts: Vec<&str> = text.split('.').collect();
        if parts.len() >= 2
            && parts[0].len() >= 8
            && parts[1].len() >= 8
            && parts[..2].iter().all(|p| p.chars().all(is_b64url))
        {
            return true;
        }
    }
    false
}

/// Shapes found after spaces and `*` marks are squeezed out. Only the
/// shapes whose prefix is itself distinctive count here.
fn glued_shape(glue: &[char]) -> bool {
    let n = glue.len();
    for at in 0..n {
        if at + 20 <= n {
            let head: String = glue[at..at + 4].iter().collect::<String>().to_uppercase();
            if matches!(head.as_str(), concat!("AK", "IA") | "ASIA")
                && glue[at + 4..at + 20]
                    .iter()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && glue[at + 4..at + 20].iter().any(char::is_ascii_digit)
            {
                return true;
            }
        }
        if at + 40 <= n {
            let head: String = glue[at..at + 4].iter().collect();
            if matches!(
                head.as_str(),
                concat!("gh", "p_") | concat!("gh", "o_") | "ghu_" | "ghs_" | "ghr_"
            ) && glue[at + 4..at + 40]
                .iter()
                .all(char::is_ascii_alphanumeric)
            {
                return true;
            }
        }
        if at + 12 <= n {
            let head: String = glue[at..at + 8].iter().collect();
            if matches!(
                head.as_str(),
                "sk_live_" | "sk_test_" | "rk_live_" | "rk_test_"
            ) {
                return true;
            }
        }
    }
    false
}

const STRONG: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "passphrase",
    "secret",
    "token",
    "apikey",
    "apitoken",
    "apisecret",
    "authorization",
    "credential",
    "credentials",
    "signature",
    "bearer",
];

const STRONG_PAIRS: &[&str] = &[
    "apikey",
    "accesskey",
    "secretkey",
    "privatekey",
    "signingkey",
    "encryptionkey",
    "authkey",
    "sessionkey",
    "licensekey",
    "authtoken",
    "accesstoken",
    "refreshtoken",
    "bearertoken",
    "idtoken",
    "clientsecret",
    "sessiontoken",
    "passphrase",
];

const WEAK: &[&str] = &["key", "pass", "pw", "auth", "sig"];

/// Short secrets that are ordinary words elsewhere (`pin the version`). They
/// count only before a connector and a value with a digit.
const CODES: &[&str] = &["pin", "otp", "passcode", "pincode"];

/// Words that join a credential name to its value in prose.
const CONNECTORS: &[&str] = &["is", "was", "are", "equals", "to"];

/// Values that follow `<credential> is` in prose and are not a credential.
const PROSE_VALUES: &[&str] = &[
    "left",
    "visible",
    "hidden",
    "out",
    "up",
    "down",
    "exposed",
    "displayed",
    "included",
    "excluded",
    "omitted",
    "held",
    "named",
    "called",
    "marked",
    "unchanged",
    "not",
    "never",
    "always",
    "also",
    "only",
    "now",
    "then",
    "here",
    "there",
    "used",
    "using",
    "sent",
    "shown",
    "kept",
    "saved",
    "read",
    "written",
    "passed",
    "given",
    "stolen",
    "leaked",
    "weak",
    "strong",
    "safe",
    "secure",
    "long",
    "short",
    "same",
    "different",
    "default",
    "correct",
    "incorrect",
    "enough",
    "ready",
    "done",
    "good",
    "bad",
    "fine",
    "ok",
    "okay",
    "like",
    "about",
    "above",
    "below",
    "again",
    "already",
    "either",
    "neither",
    "both",
    "supplied",
    "provided",
    "rotated",
    "revoked",
    "removed",
    "deleted",
    "changed",
    "checked",
    "compared",
    "verified",
    "validated",
    "accepted",
    "rejected",
    "refused",
    "logged",
    "printed",
    "echoed",
    "masked",
    "encrypted",
    "encoded",
    "decoded",
    "signed",
    "issued",
    "needed",
    "known",
    "unknown",
    "private",
    "public",
    "secret",
    "available",
    "unavailable",
    "blank",
    "case",
    "cached",
    "loaded",
    "fetched",
    "requested",
    "asked",
    "prompted",
    "entered",
    "typed",
    "copied",
    "pasted",
    "stored",
    "one",
    "per",
    "for",
    "from",
    "in",
    "on",
    "of",
    "at",
    "by",
    "with",
    "and",
    "or",
    "if",
    "when",
    "while",
    "as",
    "so",
    "but",
    "no",
    "any",
    "all",
    "each",
    "every",
    "some",
    "more",
    "most",
    "less",
    "than",
    "too",
    "very",
];

const QUALIFIERS: &[&str] = &[
    "api",
    "secret",
    "private",
    "access",
    "auth",
    "signing",
    "encryption",
    "ssh",
    "license",
    "session",
    "refresh",
    "client",
    "bearer",
    "pass",
];

const SCHEME_WORDS: &[&str] = &["bearer", "basic", "token", "digest", "negotiate"];

/// Words that follow a credential name in prose and are not a credential.
const STOP: &[&str] = &[
    "a",
    "an",
    "the",
    "this",
    "that",
    "these",
    "those",
    "it",
    "its",
    "see",
    "reset",
    "required",
    "optional",
    "must",
    "should",
    "shall",
    "may",
    "can",
    "will",
    "none",
    "null",
    "nil",
    "true",
    "false",
    "yes",
    "no",
    "na",
    "tbd",
    "todo",
    "unset",
    "empty",
    "redacted",
    "hidden",
    "value",
    "values",
    "field",
    "fields",
    "file",
    "files",
    "flow",
    "policy",
    "manager",
    "rotation",
    "hash",
    "hashed",
    "hashing",
    "stored",
    "store",
    "storage",
    "expires",
    "expired",
    "expire",
    "valid",
    "invalid",
    "missing",
    "wrong",
    "set",
    "read",
    "write",
    "send",
    "sent",
    "used",
    "use",
    "uses",
    "using",
    "never",
    "always",
    "only",
    "not",
    "is",
    "are",
    "was",
    "were",
    "be",
    "being",
    "been",
    "has",
    "have",
    "do",
    "does",
    "for",
    "to",
    "of",
    "in",
    "on",
    "at",
    "by",
    "with",
    "from",
    "per",
    "and",
    "or",
    "if",
    "when",
    "then",
    "else",
    "example",
    "sample",
    "here",
    "there",
    "below",
    "above",
    "docs",
    "documentation",
    "vault",
    "header",
    "headers",
    "scheme",
    "authentication",
    "authorization",
    "bearer",
    "basic",
    "digest",
    "login",
    "logins",
    "user",
    "users",
    "username",
    "name",
    "type",
    "kind",
    "length",
    "size",
    "limit",
    "limits",
    "budget",
    "bucket",
    "count",
    "counts",
    "lifetime",
    "ttl",
    "scope",
    "scopes",
    "handling",
    "handler",
    "check",
    "checks",
    "checked",
    "validation",
    "validate",
    "validated",
    "verify",
    "verified",
    "request",
    "requests",
    "response",
    "responses",
    "error",
    "errors",
    "failure",
    "failed",
    "needed",
    "need",
    "needs",
    "ok",
    "okay",
    "good",
    "bad",
    "enabled",
    "disabled",
    "off",
    "rotate",
    "rotated",
    "rotates",
    "change",
    "changes",
    "changed",
    "prompt",
    "reuse",
    "strength",
    "complexity",
    "min",
    "max",
    "minimum",
    "maximum",
    "default",
    "defaults",
    "depends",
    "based",
    "via",
    "through",
    "kept",
    "keeps",
    "rules",
    "rule",
    "string",
    "str",
    "bool",
    "int",
    "integer",
    "vec",
    "option",
    "u8",
    "u16",
    "u32",
    "u64",
    "i32",
    "i64",
    "usize",
    "number",
    "object",
    "any",
    "text",
    "bytes",
    "token",
    "tokens",
    "password",
    "passwords",
    "secrets",
    "key",
    "keys",
    "all",
    "reader",
    "lexer",
    "parser",
    "next",
    "each",
    "every",
    "your",
    "our",
    "my",
    "their",
    "his",
    "her",
    "which",
    "what",
    "who",
    "how",
    "why",
    "where",
    "tokenizer",
    "current",
    "previous",
    "first",
    "last",
    "new",
    "old",
    "one",
    "two",
    "three",
    "pass",
    "fail",
    "passes",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Strength {
    Strong,
    Weak,
    /// A short secret such as `pin`.
    Code,
    /// A longer name that contains a credential word, such as
    /// `SECRET_KEY_BASE`: the value has to look like one too.
    Compound,
}

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-')
}

fn split_components(ident: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for piece in ident.split(['_', '-']).filter(|piece| !piece.is_empty()) {
        for sub in camel_pieces(piece) {
            parts.push(sub.to_lowercase());
        }
    }
    parts
}

fn singular(word: &str) -> &str {
    match word {
        "passwords" | "secrets" | "tokens" | "apikeys" => &word[..word.len() - 1],
        other => other,
    }
}

fn name_strength(ident: &str, digest: bool) -> Option<Strength> {
    let comps = split_components(ident.trim_start_matches('-'));
    let last = singular(comps.last()?);
    if STRONG.contains(&last) {
        return Some(Strength::Strong);
    }
    let n = comps.len();
    if n >= 2 && STRONG_PAIRS.contains(&format!("{}{}", comps[n - 2], comps[n - 1]).as_str()) {
        return Some(Strength::Strong);
    }
    if n >= 3
        && matches!(last, "id" | "name")
        && STRONG_PAIRS.contains(&format!("{}{}", comps[n - 3], comps[n - 2]).as_str())
    {
        return Some(Strength::Strong);
    }
    if CODES.contains(&last) {
        return Some(Strength::Code);
    }
    if WEAK.contains(&last) || (digest && matches!(last, "response" | "nonce" | "cnonce")) {
        return Some(Strength::Weak);
    }
    if n >= 2
        && comps.iter().any(|part| {
            matches!(
                singular(part),
                "password" | "passwd" | "pwd" | "secret" | "token" | "passphrase" | "credential"
            )
        })
    {
        return Some(Strength::Compound);
    }
    None
}

enum Mode {
    Sep,
    Is,
    Flag,
    Bare,
}

/// A credential name followed by something that looks like its value:
/// `password: x`, `token=x`, `the password is hunter2`, `PASSWORD - x`,
/// `--password x`, `API key abcdef123`. The value must not be a
/// placeholder, a call, a stop word or a short number.
fn credential_hit(chars: &[char], digest: bool, glued: bool) -> bool {
    let n = chars.len();
    let mut idents: Vec<(usize, usize)> = Vec::new();
    let mut index = 0;
    while index < n {
        if ident_char(chars[index]) {
            let from = index;
            while index < n && ident_char(chars[index]) {
                index += 1;
            }
            idents.push((from, index));
        } else {
            index += 1;
        }
    }
    for (at, &(from, end)) in idents.iter().enumerate() {
        let ident: String = chars[from..end].iter().collect();
        let flag = ident.starts_with("--");
        if from > 0
            && matches!(chars[from - 1], '=' | '~')
            && chars.get(end) == Some(&chars[from - 1])
        {
            continue;
        }
        let mut strength = name_strength(&ident, digest);
        if at > 0 && strength != Some(Strength::Strong) {
            let (pfrom, pend) = idents[at - 1];
            let prev: String = chars[pfrom..pend].iter().collect::<String>().to_lowercase();
            let last = ident.to_lowercase();
            if pend + 1 == from
                && chars[pend] == ' '
                && QUALIFIERS.contains(&prev.as_str())
                && matches!(last.as_str(), "key" | "token" | "phrase" | "secret")
            {
                strength = Some(Strength::Strong);
            }
        }
        let Some(strength) = strength else { continue };
        let strong = strength == Strength::Strong;
        let code = strength == Strength::Code;
        let mut here = end;
        while here < n && matches!(chars[here], '"' | '\'' | '`' | ']' | ')') {
            here += 1;
        }
        let gap = here;
        while here < n && matches!(chars[here], ' ' | '\t') {
            here += 1;
        }
        let spaced = here > gap;
        if here >= n {
            continue;
        }
        let next = chars.get(here + 1).copied();
        if chars[here] == '=' && spaced && verbatim_span(chars, here) {
            continue;
        }
        if chars[here] == '='
            && !spaced
            && !next.is_some_and(char::is_alphanumeric)
            && chars[..here].iter().filter(|c| **c == '=').count() % 2 == 1
        {
            continue;
        }
        if chars[here] == ':'
            && next == Some('=')
            && !chars.get(here + 2).is_some_and(|c| c.is_alphanumeric())
            && chars[..here].iter().filter(|c| **c == '=').count() % 2 == 1
        {
            continue;
        }
        let (mode, value_from) = if chars[here] == ':' && next != Some(':') {
            (Mode::Sep, here + 1 + usize::from(next == Some('=')))
        } else if chars[here] == '=' && !matches!(next, Some('=' | '>' | '~')) {
            (Mode::Sep, here + 1)
        } else if spaced && matches!(chars[here], '-' | '\u{2013}') && next == Some(' ') {
            (Mode::Sep, here + 2)
        } else if let Some(len) = connector_len(chars, here, spaced, strong, code) {
            (Mode::Is, here + len)
        } else if flag && spaced {
            (Mode::Flag, here)
        } else if spaced && (strong || code) {
            (Mode::Bare, here)
        } else {
            continue;
        };
        let mut words = value_words(chars, value_from);
        let lowered = ident.to_lowercase();
        if lowered.contains("authorization") || lowered == "bearer" {
            while words.len() > 1
                && SCHEME_WORDS.contains(&trim_value(&words[0]).to_lowercase().as_str())
            {
                words.remove(0);
            }
        }
        let Some(value) = words.first() else { continue };
        let trimmed = trim_value(value);
        let label = lowered.ends_with('s')
            && words.len() > 1
            && trimmed.chars().all(|c| c.is_ascii_lowercase());
        if label && matches!(mode, Mode::Sep) {
            continue;
        }
        let ok = match mode {
            Mode::Sep | Mode::Flag => {
                credential_value(value)
                    && match strength {
                        Strength::Strong => true,
                        Strength::Weak => {
                            trimmed.chars().count() >= 3
                                && (!matches!(mode, Mode::Flag)
                                    || trimmed.chars().any(|c| !c.is_ascii_lowercase()))
                        }
                        Strength::Code => code_value(trimmed),
                        Strength::Compound => {
                            trimmed.chars().count() >= 6
                                && trimmed.chars().any(|c| c.is_ascii_digit())
                        }
                    }
            }
            Mode::Is => {
                let digit = trimmed.chars().any(|c| c.is_ascii_digit());
                if code {
                    code_value(trimmed)
                } else {
                    credential_value(value)
                        && trimmed.chars().count() >= 3
                        && (digit
                            || trimmed.chars().any(|c| !c.is_alphanumeric())
                            || (!glued && !prose_value(trimmed)))
                }
            }
            Mode::Bare => {
                if code {
                    code_value(trimmed)
                } else {
                    credential_value(value)
                        && trimmed.chars().count() >= 6
                        && trimmed.chars().any(|c| c.is_ascii_digit())
                        && trimmed.chars().any(char::is_alphabetic)
                }
            }
        };
        if ok {
            return true;
        }
    }
    false
}

/// A connector between a credential name and its value: `is`, `was`, `are`,
/// `equals`, `to`, `=>` or `->`. The length is counted in characters.
fn connector_len(
    chars: &[char],
    here: usize,
    spaced: bool,
    strong: bool,
    code: bool,
) -> Option<usize> {
    if !(strong || code) {
        return None;
    }
    if matches!(chars[here], '=' | '-') && chars.get(here + 1) == Some(&'>') {
        return Some(2);
    }
    if !spaced {
        return None;
    }
    CONNECTORS
        .iter()
        .filter(|word| strong || **word != "to")
        .find(|word| is_word_at(chars, here, &[word]))
        .map(|word| word.len())
}

/// A word that follows `<credential> is` in ordinary prose: a listed word or
/// a participle, adjective or adverb by its ending.
fn prose_value(value: &str) -> bool {
    let lower = value.to_lowercase();
    PROSE_VALUES.contains(&lower.as_str())
        || ["ed", "ing", "ly", "ble", "ous", "ful", "less"]
            .iter()
            .any(|end| lower.ends_with(end))
}

/// A PIN or one-time code: four to twelve letters and digits with a digit.
fn code_value(value: &str) -> bool {
    let len = value.chars().count();
    (4..=12).contains(&len)
        && value.chars().all(char::is_alphanumeric)
        && value.chars().any(|c| c.is_ascii_digit())
}

/// `=word=` after a space is an Org verbatim span, not an assignment.
fn verbatim_span(chars: &[char], at: usize) -> bool {
    let mut end = at + 1;
    while end < chars.len() && !chars[end].is_whitespace() && chars[end] != '=' {
        end += 1;
    }
    end > at + 1
        && chars.get(end) == Some(&'=')
        && chars.get(end + 1).is_none_or(|c| !c.is_alphanumeric())
}

/// A `Cookie` or `Set-Cookie` header that carries `name=value`.
fn cookie_hit(chars: &[char]) -> bool {
    let text: String = chars.iter().collect::<String>().to_lowercase();
    let mut from = 0;
    while let Some(found) = text[from..].find("cookie") {
        let at = from + found + "cookie".len();
        from = at;
        let rest = text[at..].trim_start();
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        for pair in rest.split([';', ' ']) {
            if let Some((name, value)) = pair.split_once('=') {
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                    && value.trim_matches('"').chars().count() >= 8
                    && !placeholder(value)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// The `mysql -p<password>` form: the value is glued to the flag.
fn glued_password_flag(chars: &[char]) -> bool {
    let text: String = chars.iter().collect();
    let words: Vec<&str> = text.split_whitespace().collect();
    let database = text.to_lowercase().contains("mysql")
        || text.to_lowercase().contains("mariadb")
        || words
            .iter()
            .any(|word| word.starts_with("-u") && word.len() > 2 && !word.starts_with("--"));
    database
        && words.iter().any(|word| {
            word.strip_prefix("-p").is_some_and(|value| {
                let value = value.trim_matches(|c: char| matches!(c, '"' | '\'' | '`'));
                value.chars().count() >= 3
                    && !value.starts_with('-')
                    && !placeholder(value)
                    && value.chars().all(|c| c.is_ascii_graphic())
            })
        })
}

fn is_word_at(chars: &[char], at: usize, words: &[&str]) -> bool {
    words.iter().any(|word| {
        let len = word.chars().count();
        at + len < chars.len()
            && chars[at..at + len]
                .iter()
                .collect::<String>()
                .eq_ignore_ascii_case(word)
            && chars[at + len] == ' '
    })
}

/// The first few words after `from`, split on spaces.
fn value_words(chars: &[char], from: usize) -> Vec<String> {
    let tail: String = chars[from.min(chars.len())..].iter().collect();
    tail.split_whitespace()
        .take(3)
        .map(str::to_string)
        .collect()
}

fn trim_value(value: &str) -> &str {
    value.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\''
                | '`'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | ','
                | ';'
                | '.'
                | '<'
                | '>'
                | '*'
        )
    })
}

fn numeric_like(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '_' | '%'))
}

fn placeholder(value: &str) -> bool {
    value.starts_with('<')
        || value.starts_with('$')
        || value.starts_with('%')
        || value.starts_with("{{")
        || value.contains("${")
}

/// Whether the word after a credential name could be a credential: it is not
/// a placeholder, a call, a path expression, a stop word or a short number.
fn credential_value(value: &str) -> bool {
    if placeholder(value)
        || value.starts_with('=')
        || value.starts_with('-')
        || value.contains('(')
        || value.contains("::")
    {
        return false;
    }
    let trimmed = trim_value(value);
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return false;
    }
    let metavar = trimmed.trim_end_matches('=');
    if metavar.chars().count() >= 2 && metavar.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if STOP.contains(&lower.as_str()) {
        return false;
    }
    if trimmed
        .chars()
        .all(|c| matches!(c, '*' | '.' | 'x' | 'X' | '-' | '_'))
        && (trimmed.chars().count() >= 3 || !trimmed.contains(['x', 'X']))
    {
        return false;
    }
    !(numeric_like(trimmed) && trimmed.chars().count() <= 8)
}

fn encoded(value: &str) -> bool {
    let t = trim_value(value);
    t.chars().count() >= 8
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_' | '.'))
        && (t.chars().any(|c| c.is_ascii_digit())
            || (t.chars().any(|c| c.is_ascii_uppercase())
                && t.chars().any(|c| c.is_ascii_lowercase())))
}

/// `Bearer`, `Basic` and `token` before a value.
fn scheme_hit(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (at, word) in words.iter().enumerate() {
        let name = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        let Some(value) = words.get(at + 1) else {
            continue;
        };
        if placeholder(value) {
            continue;
        }
        let lower = trim_value(value).to_lowercase();
        if STOP.contains(&lower.as_str()) {
            continue;
        }
        let hit = match name.as_str() {
            "bearer" => {
                let t = trim_value(value);
                t.chars().count() >= 8
                    && t.chars().all(|c| {
                        c.is_ascii_alphanumeric()
                            || matches!(c, '.' | '_' | '~' | '+' | '/' | '=' | '-')
                    })
            }
            "basic" | "token" => encoded(value),
            _ => false,
        };
        if hit {
            return true;
        }
    }
    false
}

/// A login given on a command line: `-u user:pass`, `--user user:pass` and
/// `sshpass -p value`.
fn command_login(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    let secret_word = |word: &str| {
        let value = trim_value(word);
        !value.is_empty()
            && !placeholder(value)
            && !value.starts_with('-')
            && !STOP.contains(&value.to_lowercase().as_str())
    };
    for (at, word) in words.iter().enumerate() {
        let Some(next) = words.get(at + 1) else {
            continue;
        };
        let flag = word.to_lowercase();
        if matches!(flag.as_str(), "-u" | "--user" | "--proxy-user") {
            let Some((name, pass)) = next.split_once(':') else {
                continue;
            };
            let numeric = |part: &str| part.chars().all(|c| c.is_ascii_digit());
            if !name.is_empty()
                && !name.contains(['/', '\\'])
                && !numeric(name)
                && !pass.starts_with("//")
                && secret_word(pass)
            {
                return true;
            }
        }
        if flag == "-p"
            && words[..at]
                .iter()
                .rev()
                .take(6)
                .any(|before| before.to_lowercase().trim_matches('`') == "sshpass")
            && secret_word(next)
        {
            return true;
        }
    }
    false
}

fn user_pass(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let Some((left, host)) = token.rsplit_once('@') else {
            return false;
        };
        let Some((user, pass)) = left.rsplit_once(':') else {
            return false;
        };
        let user = user.rsplit(['/', '\\']).next().unwrap_or(user);
        !user.is_empty() && !pass.is_empty() && !pass.starts_with("//") && !host.is_empty()
    })
}

fn private_key(lower: &str) -> bool {
    let collapsed = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.match_indices("begin ").any(|(at, _)| {
        let tail = &collapsed[at + "begin ".len()..];
        tail.char_indices()
            .take(40)
            .any(|(offset, _)| tail[offset..].starts_with("private key"))
    })
}

const HASH_WORDS: &[&str] = &[
    "md5",
    "md5sum",
    "sha",
    "sha1",
    "sha1sum",
    "sha256",
    "sha256sum",
    "sha512",
    "sha512sum",
    "blake2",
    "blake3",
    "checksum",
    "digest",
    "hash",
    "crc",
];

fn base64_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-')
}

fn entropy(run: &[char]) -> f64 {
    let mut counts = std::collections::BTreeMap::new();
    for c in run {
        *counts.entry(*c).or_insert(0_u32) += 1;
    }
    let total = f64::from(u32::try_from(run.len()).unwrap_or(u32::MAX));
    counts
        .values()
        .map(|count| {
            let share = f64::from(*count) / total;
            -share * share.log2()
        })
        .sum()
}

fn wordlike(segment: &str) -> bool {
    if segment.chars().all(|c| c.is_ascii_digit()) {
        return segment.len() <= 6;
    }
    if !segment.chars().all(|c| c.is_ascii_alphabetic()) || segment.len() > 20 {
        return false;
    }
    let upper = segment.chars().filter(char::is_ascii_uppercase).count();
    upper == 0
        || upper == segment.len()
        || (upper == 1
            && segment
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase()))
}

/// Words joined by `-`, `_`, `/`, `+` or `=`: a slug, a snake_case name or a
/// branch path.
fn slugish(run: &str) -> bool {
    let segments: Vec<&str> = run
        .split(['-', '_', '/', '+', '='])
        .filter(|segment| !segment.is_empty())
        .collect();
    segments.len() >= 3
        && segments.iter().filter(|segment| wordlike(segment)).count() * 5 >= segments.len() * 4
}

fn camel_ident(run: &str) -> bool {
    if !run.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let pieces = camel_pieces(run);
    pieces.len() >= 3
        && pieces.iter().all(|piece| {
            piece.len() <= 16
                && piece.chars().all(|c| c.is_ascii_alphanumeric())
                && piece.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        })
}

fn b64_value(c: char) -> Option<u8> {
    Some(match c {
        'A'..='Z' => c as u8 - b'A',
        'a'..='z' => c as u8 - b'a' + 26,
        '0'..='9' => c as u8 - b'0' + 52,
        '+' | '-' => 62,
        '/' | '_' => 63,
        _ => return None,
    })
}

/// Whether `run` decodes as base64 to mostly printable bytes.
fn decodes_to_text(run: &str) -> bool {
    let core = run.trim_end_matches('=');
    let values: Vec<u8> = core.chars().filter_map(b64_value).collect();
    if values.len() != core.chars().count() || values.len() < 16 {
        return false;
    }
    let mut bytes = Vec::new();
    for chunk in values.chunks(4) {
        let mut acc: u32 = 0;
        for value in chunk {
            acc = (acc << 6) | u32::from(*value);
        }
        let missing = u32::try_from(4 - chunk.len()).unwrap_or(0);
        acc <<= 6 * missing;
        let produced = chunk.len() * 6 / 8;
        for at in 0..produced {
            let shift = 16 - 8 * u32::try_from(at).unwrap_or(0);
            bytes.push(u8::try_from((acc >> shift) & 0xff).unwrap_or(0));
        }
    }
    let printable = bytes
        .iter()
        .filter(|b| (0x20..0x7f).contains(*b) || matches!(**b, b'\n' | b'\t'))
        .count();
    bytes.len() >= 12 && printable * 10 >= bytes.len() * 9
}

fn digit_groups(run: &str) -> usize {
    let mut groups = 0;
    let mut inside = false;
    for c in run.chars() {
        if c.is_ascii_digit() {
            if !inside {
                groups += 1;
            }
            inside = true;
        } else {
            inside = false;
        }
    }
    groups
}

/// Long opaque runs. A URL is read only after its `?`, so a path segment
/// stays. A 40, 64 or 128 digit hex word of either case is a commit, tree
/// or digest id and stays, as does a 32 digit one after a hash name. Any
/// other hex run of 32 or more is a key, unless it repeats. A run of 40 or
/// more is a key when it is not a slug or a CamelCase name and is random
/// enough. A shorter run is a key when it decodes to text, or mixes
/// capitals, lowercase and two or more digit groups.
fn opaque_hit(text: &str, identifier: bool) -> bool {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (at, token) in tokens.iter().enumerate() {
        let region = if token.contains("://") {
            match token.split_once('?') {
                Some((_, query)) => query.replace(['=', '&', ';', '#'], " "),
                None => continue,
            }
        } else {
            (*token).to_string()
        };
        let hash_context = at > 0
            && HASH_WORDS.contains(
                &tokens[at - 1]
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
                    .as_str(),
            );
        for piece in region.split_whitespace() {
            if run_hit(piece, identifier, hash_context) {
                return true;
            }
        }
    }
    false
}

fn run_hit(piece: &str, identifier: bool, hash_context: bool) -> bool {
    let chars: Vec<char> = piece.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if !base64_char(chars[index]) {
            index += 1;
            continue;
        }
        let from = index;
        while index < chars.len() && base64_char(chars[index]) {
            index += 1;
        }
        let text: String = chars[from..index].iter().collect();
        if run_is_key(&text, identifier, hash_context) {
            return true;
        }
    }
    false
}

/// Three or more pieces cut at `-`, `_` or `/` that are plain words, such
/// as `ljos-UID/home-1a2b3c`: a name, not a key.
fn word_slug(core: &str) -> bool {
    let pieces: Vec<&str> = core
        .split(['-', '_', '/'])
        .filter(|piece| {
            piece.chars().count() >= 3 && piece.chars().all(|c| c.is_ascii_alphabetic())
        })
        .collect();
    let plain = pieces
        .iter()
        .filter(|piece| {
            piece.chars().all(|c| c.is_ascii_lowercase())
                || piece[1..].chars().all(|c| c.is_ascii_lowercase())
        })
        .count();
    pieces.len() >= 3 && plain >= 2
}

fn run_is_key(text: &str, identifier: bool, hash_context: bool) -> bool {
    let core = text.trim_matches(|c| matches!(c, '-' | '_' | '/' | '+' | '='));
    let len = core.chars().count();
    if len < 16 {
        return false;
    }
    let core_chars: Vec<char> = core.chars().collect();
    if core.chars().all(|c| c.is_ascii_hexdigit()) {
        if len < 32 || identifier {
            return false;
        }
        let kept = matches!(len, 40 | 64 | 128)
            || len % 40 == 0
            || len % 64 == 0
            || (len == 32 && hash_context)
            || entropy(&core_chars) < 2.5;
        return !kept;
    }
    if camel_ident(core) || slugish(core) || word_slug(core) {
        return false;
    }
    if len >= 40 {
        return entropy(&core_chars) >= 3.9;
    }
    let upper = core.chars().any(|c| c.is_ascii_uppercase());
    let lower = core.chars().any(|c| c.is_ascii_lowercase());
    if upper && lower && decodes_to_text(core) {
        return true;
    }
    len >= 24 && upper && lower && digit_groups(core) >= 3
}

// ---------------------------------------------------------------------------
// Addresses
// ---------------------------------------------------------------------------

const ADDRESS_WORDS: &[&str] = &[
    "ip",
    "ipv4",
    "host",
    "hostname",
    "addr",
    "address",
    "ping",
    "ssh",
    "scp",
    "sftp",
    "curl",
    "wget",
    "telnet",
    "nc",
    "ncat",
    "connect",
    "server",
    "listen",
    "listens",
    "listening",
    "bind",
    "bound",
    "socket",
    "proxy",
    "gateway",
    "dns",
    "url",
    "endpoint",
    "remote",
];

const FILLERS: &[&str] = &[
    "to", "at", "on", "via", "from", "the", "of", "a", "is", "was",
];

const VERSION_WORDS: &[&str] = &[
    "version",
    "versions",
    "release",
    "releases",
    "build",
    "v",
    "ver",
    "rev",
    "revision",
    "tag",
    "tagged",
    "upgrade",
    "upgraded",
    "downgrade",
    "firmware",
    "patch",
];

const PUBLIC_RESOLVERS: &[[u32; 4]] = &[
    [1, 1, 1, 1],
    [1, 0, 0, 1],
    [8, 8, 8, 8],
    [8, 8, 4, 4],
    [9, 9, 9, 9],
    [208, 67, 222, 222],
    [208, 67, 220, 220],
];

const PRIVATE_SUFFIXES: &[&str] = &[
    ".ts.net",
    ".internal",
    ".corp",
    ".lan",
    ".intranet",
    ".home.arpa",
];

fn address_hit(prep: &Prep) -> bool {
    let fold: Vec<char> = defang(&prep.s).chars().collect();
    let net: Vec<char> = fold
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    ipv4_hit(&net) || dashed_hit(&net) || ipv6_hit(&net, &fold) || private_host(&net)
}

/// `[.]`, `(.)`, `{.}`, `\.`, `[dot]` and ` dot ` between digits read as `.`.
fn defang(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if let Some(mark) = ["[.]", "(.)", "{.}", "\\.", "[dot]", "(dot)", "{dot}"]
            .iter()
            .find(|mark| {
                chars[index..]
                    .iter()
                    .zip(mark.chars())
                    .all(|(actual, expected)| actual.eq_ignore_ascii_case(&expected))
                    && chars.len() - index >= mark.chars().count()
            })
        {
            out.push('.');
            index += mark.chars().count();
            continue;
        }
        if chars[index] == ' '
            && index > 0
            && chars[index - 1].is_ascii_digit()
            && chars[index..]
                .iter()
                .zip([' ', 'd', 'o', 't', ' '])
                .all(|(actual, expected)| actual.eq_ignore_ascii_case(&expected))
            && chars.get(index + 5).is_some_and(char::is_ascii_digit)
        {
            out.push('.');
            index += 5;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn digits_at(chars: &[char], start: usize) -> usize {
    chars[start..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count()
}

/// A number starts at a digit. Dots immediately before it are skipped, so
/// `...10.0.0.5` and `e.g.10.0.0.1` are read. A digit before those dots is a
/// version (`1.10.0.0.5`) and is not a start. Only a `v` glued to the first
/// digit is version context (`v10.0.0.5`). Any other letter leaves the
/// number visible (`x10.0.0.1`).
fn number_start(chars: &[char], start: usize) -> bool {
    if !chars.get(start).is_some_and(char::is_ascii_digit) {
        return false;
    }
    let dots = chars[..start]
        .iter()
        .rev()
        .take_while(|c| **c == '.')
        .count();
    if start == dots {
        return true;
    }
    let before = chars[start - dots - 1];
    if before.is_ascii_digit() {
        return false;
    }
    dots > 0 || before != 'v'
}

/// Reads `text` as `inet_aton` does: one to four parts split by dots, each a
/// `0x` hex, a leading-zero octal or a decimal number of any length, the last
/// part filling the bytes the others leave. A bad digit, or a part too large
/// for its bytes, is `None`.
fn inet_aton(text: &str) -> Option<u32> {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut value = 0u64;
    for (index, part) in parts.iter().enumerate() {
        let (digits, radix) = match part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
            Some(hex) => (hex, 16),
            None if part.len() > 1 && part.starts_with('0') => (&part[1..], 8),
            None => (*part, 10),
        };
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return None;
        }
        let number = u64::from_str_radix(digits, radix).ok()?;
        let bits = if index + 1 == parts.len() {
            8 * (4 - index)
        } else {
            8
        };
        if number >> bits != 0 {
            return None;
        }
        value = (value << bits) | number;
    }
    u32::try_from(value).ok()
}

/// The end of the letters, digits, `_` and dots from `start`, taking a dot
/// only when a digit follows it.
fn ipv4_end(chars: &[char], start: usize) -> usize {
    let mut at = start;
    loop {
        while chars
            .get(at)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            at += 1;
        }
        if chars.get(at) == Some(&'.') && chars.get(at + 1).is_some_and(char::is_ascii_digit) {
            at += 1;
        } else {
            return at;
        }
    }
}

fn words_before(chars: &[char], start: usize, count: usize) -> Vec<String> {
    let from = start.saturating_sub(64);
    let before: String = chars[from..start].iter().collect();
    before
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .rev()
        .take(count)
        .map(str::to_string)
        .collect()
}

fn words_after(chars: &[char], end: usize, count: usize) -> Vec<String> {
    let to = (end + 64).min(chars.len());
    let after: String = chars[end..to].iter().collect();
    after
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(count)
        .map(str::to_string)
        .collect()
}

/// A word before or after the number that points at an address, looking past
/// fillers such as `to` and `at`, within `window` words on that side.
/// `//` and `@` count on either side of the number (`//10.1`, `10.1@`).
fn address_context(chars: &[char], start: usize, end: usize, window: usize) -> bool {
    let from = start.saturating_sub(48);
    let before: String = chars[from..start].iter().collect();
    if before.ends_with("//") || before.ends_with('@') {
        return true;
    }
    let to = (end + 48).min(chars.len());
    let after: String = chars[end..to].iter().collect();
    if after.starts_with("//") || after.starts_with('@') {
        return true;
    }
    let near = |words: Vec<String>| {
        words
            .iter()
            .filter(|word| !FILLERS.contains(&word.as_str()))
            .take(window)
            .any(|word| ADDRESS_WORDS.contains(&word.as_str()))
    };
    near(words_before(chars, start, 8)) || near(words_after(chars, end, 8))
}

fn port_follows(chars: &[char], at: usize) -> bool {
    chars.get(at) == Some(&':')
        && chars.get(at + 1).is_some_and(char::is_ascii_digit)
        && digits_at(chars, at + 1) <= 5
}

fn private_set(value: u32) -> bool {
    let a = value >> 24;
    let b = (value >> 16) & 0xff;
    a == 10
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..=127).contains(&b))
}

/// A number `inet_aton` reads. Four dot-separated parts are an address unless
/// the quad is a decimal resolver, netmask or version. One to three parts, or
/// a bare integer, are an address when the value is private and an address
/// word, a port, `//` or `@` sits on either side of the number.
fn ipv4_hit(chars: &[char]) -> bool {
    (0..chars.len())
        .filter(|&start| number_start(chars, start))
        .any(|start| {
            let end = ipv4_end(chars, start);
            let text: String = chars[start..end].iter().collect();
            let Some(value) = inet_aton(&text) else {
                return false;
            };
            let parts = text.split('.').count();
            if parts == 4 {
                let decimal = text
                    .split('.')
                    .all(|part| part.len() == 1 || !part.starts_with('0'));
                return !(decimal && ipv4_exempt(value, chars, start, end));
            }
            private_set(value)
                && (port_follows(chars, end) || address_context(chars, start, end, 2))
        })
}

/// A plain dotted quad that reads as a mask, a resolver or a version.
/// A mask is contiguous: ones, then zeros.
fn ipv4_exempt(value: u32, chars: &[char], start: usize, end: usize) -> bool {
    let octets = value.to_be_bytes().map(u32::from);
    if value.leading_ones() + value.trailing_zeros() == 32 || PUBLIC_RESOLVERS.contains(&octets) {
        return true;
    }
    let before = words_before(chars, start, 2);
    let after = words_after(chars, end, 3);
    before
        .iter()
        .chain(&after)
        .any(|word| VERSION_WORDS.contains(&word.as_str()))
}

fn dashed_hit(chars: &[char]) -> bool {
    for start in 0..chars.len() {
        if !chars[start].is_ascii_digit() || (start > 0 && chars[start - 1].is_ascii_digit()) {
            continue;
        }
        let mut at = start;
        let mut valid = true;
        for part in 0..4 {
            let len = digits_at(chars, at);
            let digits: String = chars[at..at + len].iter().collect();
            if len == 0 || !matches!(digits.parse::<u32>(), Ok(0..=255)) {
                valid = false;
                break;
            }
            at += len;
            if part < 3 {
                if chars.get(at) == Some(&'-') {
                    at += 1;
                } else {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            continue;
        }
        let ip_prefix = start >= 3 && chars[start - 3..start] == ['i', 'p', '-'];
        let rest: String = chars[at..(at + 12).min(chars.len())].iter().collect();
        let dns_suffix = [".nip.io", ".sslip.io", ".xip.io"]
            .iter()
            .any(|suffix| rest.starts_with(suffix));
        if ip_prefix || dns_suffix {
            return true;
        }
    }
    false
}

fn private_host(chars: &[char]) -> bool {
    let text: String = chars.iter().collect();
    PRIVATE_SUFFIXES.iter().any(|suffix| {
        text.match_indices(suffix).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let after = text[at + suffix.len()..].chars().next();
            before.is_some_and(char::is_alphanumeric)
                && after.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '-' | '_')))
        })
    })
}

/// The longest IPv6 text that parses: eight groups and an IPv4 tail.
const MAX_ADDRESS: usize = 45;

fn ipv6_char(c: char) -> bool {
    c.is_ascii_hexdigit() || matches!(c, ':' | '.')
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether the text contains an IPv6 address, with or without a zone, brackets
/// or a port.
///
/// A run of hex digits, colons and dots is parsed with
/// [`std::net::Ipv6Addr`], from the start of the run and from after each
/// single colon. A literal never starts in the middle of a word, so the
/// `::add` of `std::ops::Add` and the `::1` of `Foo::1` are paths. A literal
/// with a digit is an address. One without a digit is an address when it
/// opens or closes with `::` (`::a`, `::ffff`) or when every group is four
/// or more digits and none is capitalized (`dead::beef`, but not the Rust
/// path `Face::Fade`). A run never starts at a dot, so the `::a` of `x.::a`
/// is read after it.
fn ipv6_hit(chars: &[char], fold: &[char]) -> bool {
    let mut index = 0;
    while index < chars.len() {
        if !ipv6_char(chars[index]) || chars[index] == '.' {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && ipv6_char(chars[index]) {
            index += 1;
        }
        if run_is_ipv6(chars, fold, start, index) {
            return true;
        }
    }
    false
}

fn run_is_ipv6(chars: &[char], fold: &[char], start: usize, end: usize) -> bool {
    for at in start..end {
        if at > start && (chars[at - 1] != ':' || (at - 1 > start && chars[at - 2] == ':')) {
            continue;
        }
        if at == start && at > 0 && word_char(chars[at - 1]) {
            continue;
        }
        if candidate_is_ipv6(chars, fold, at, end) {
            return true;
        }
    }
    false
}

fn candidate_is_ipv6(chars: &[char], fold: &[char], at: usize, end: usize) -> bool {
    for stop in at + 2..=end.min(at + MAX_ADDRESS) {
        if stop == end {
            if !ipv6_end_ok(chars, stop) {
                continue;
            }
        } else if chars[stop] == ':' {
            if chars.get(stop + 1) == Some(&':') {
                continue;
            }
        } else if chars[stop] != '.' {
            continue;
        }
        let text: String = chars[at..stop].iter().collect();
        // `::` between other text is a separator (`term :: description`).
        // Alone on the line it is the unspecified address.
        if text == "::" && fold.iter().any(|c| !(*c == ':' || c.is_whitespace())) {
            continue;
        }
        if text.matches(':').count() < 2 || text.parse::<Ipv6Addr>().is_err() {
            continue;
        }
        if !text.chars().any(|c| c.is_ascii_digit()) {
            let original: String = fold[at..stop].iter().collect();
            if !digit_free_address(&original, fold.get(stop).copied()) {
                continue;
            }
        }
        return true;
    }
    false
}

fn digit_free_address(original: &str, next: Option<char>) -> bool {
    let groups: Vec<&str> = original
        .split(':')
        .filter(|group| !group.is_empty())
        .collect();
    let capital = groups.iter().any(|group| {
        let mut chars = group.chars();
        chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.any(|c| c.is_ascii_lowercase())
    });
    if capital {
        return false;
    }
    if original.ends_with("::")
        && next.is_some_and(|c| c.is_alphanumeric() || matches!(c, '<' | '{'))
    {
        return false;
    }
    original.starts_with("::")
        || original.ends_with("::")
        || (groups.len() >= 2 && groups.iter().all(|group| group.len() >= 4))
}

fn ipv6_end_ok(chars: &[char], stop: usize) -> bool {
    match chars.get(stop) {
        None | Some('%') => true,
        Some(c) => !word_char(*c),
    }
}

// ---------------------------------------------------------------------------
// Mailboxes
// ---------------------------------------------------------------------------

const KNOWN_TLDS: &[&str] = &[
    "com", "org", "net", "edu", "gov", "mil", "int", "io", "co", "dev", "app", "info", "biz",
    "xyz", "me", "us", "uk", "de", "fr", "nl", "se", "ch", "ca", "au", "jp", "cn", "in", "eu",
    "ru", "br", "es", "it", "no", "fi", "dk", "pl", "ie", "nz", "za", "sh", "ai", "cloud", "tech",
    "online", "site", "store", "blog", "email", "name", "pro", "ninja", "internal", "corp", "lan",
    "local", "home", "test", "invalid", "at", "be", "cz", "mx", "kr", "tv", "cc",
];

const PROVIDERS: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "outlook.com",
    "hotmail.com",
    "live.com",
    "yahoo.com",
    "icloud.com",
    "me.com",
    "aol.com",
    "proton.me",
    "protonmail.com",
    "pm.me",
    "fastmail.com",
    "hey.com",
    "gmx.com",
    "gmx.net",
    "yandex.com",
    "mail.com",
    "zoho.com",
    "example.com",
    "example.org",
    "example.net",
];

/// Words that come before `at` in prose and are not a mailbox name.
const AT_STOP: &[&str] = &[
    "look",
    "looks",
    "looked",
    "looking",
    "point",
    "points",
    "pointed",
    "pointing",
    "arrive",
    "arrived",
    "arrives",
    "meet",
    "meets",
    "met",
    "available",
    "reach",
    "reached",
    "reachable",
    "see",
    "seen",
    "visit",
    "visited",
    "stare",
    "stared",
    "aim",
    "aimed",
    "directed",
    "hosted",
    "served",
    "running",
    "live",
    "lives",
    "lived",
    "based",
    "stay",
    "stayed",
    "wait",
    "waiting",
    "waited",
    "found",
    "find",
    "page",
    "site",
    "docs",
    "service",
    "endpoint",
    "listening",
    "listen",
    "work",
    "works",
    "working",
    "worked",
    "home",
    "me",
    "us",
    "him",
    "her",
    "them",
    "you",
    "it",
    "one",
    "here",
    "there",
    "start",
    "started",
    "stop",
    "stopped",
    "end",
    "ends",
    "ended",
    "ping",
    "pinged",
    "check",
    "checked",
    "go",
    "goes",
    "gone",
    "open",
    "opened",
    "set",
    "run",
    "ran",
    "ok",
    "a",
    "an",
    "the",
    "is",
    "was",
    "are",
    "were",
    "be",
    "been",
    "not",
    "and",
    "or",
    "of",
    "to",
    "in",
    "on",
    "if",
    "so",
    "as",
    "but",
    "dot",
    "now",
    "then",
    "just",
    "only",
    "even",
    "also",
    "all",
    "any",
    "some",
    "each",
    "both",
    "once",
    "least",
    "most",
    "first",
    "last",
    "next",
    "same",
    "every",
    "glanced",
    "laughed",
];

/// Words that stand in the host slot of `name at host dot tld` and are not a host.
const HOST_STOP: &[&str] = &[
    "a", "an", "the", "this", "that", "these", "those", "my", "our", "your", "his", "her", "their",
    "its", "it", "me", "us", "you", "them", "home", "work", "night", "once", "all", "least",
    "most", "first", "last", "any", "some", "each", "one", "two", "same", "which", "what", "there",
    "here", "no", "school", "dot",
];

fn mailbox_hit(prep: &Prep) -> bool {
    let decoded = decode_marks(&prep.fold);
    let marked = replace_markers(&decoded);
    direct_email(&marked) || spelled_email(&marked)
}

fn lower_eq(chars: &[char], at: usize, pattern: &str) -> bool {
    let want: Vec<char> = pattern.chars().collect();
    at + want.len() <= chars.len()
        && chars[at..at + want.len()]
            .iter()
            .zip(&want)
            .all(|(have, want)| have.to_lowercase().next() == Some(*want))
}

/// Numeric entities, `%xx` (also double encoded), `\uXXXX`, `\xXX` and `=40`
/// for `@` and `.` become the character.
fn decode_marks(chars: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == '&' && chars.get(index + 1) == Some(&'#') {
            let hex = matches!(chars.get(index + 2), Some('x' | 'X'));
            let from = index + if hex { 3 } else { 2 };
            let len = chars[from.min(chars.len())..]
                .iter()
                .take_while(|d| {
                    if hex {
                        d.is_ascii_hexdigit()
                    } else {
                        d.is_ascii_digit()
                    }
                })
                .count();
            if len > 0 && len <= 8 {
                let digits: String = chars[from..from + len].iter().collect();
                let code = u32::from_str_radix(&digits, if hex { 16 } else { 10 }).unwrap_or(0);
                let end = from + len + usize::from(chars.get(from + len) == Some(&';'));
                if code == 64 || code == 46 {
                    out.push(if code == 64 { '@' } else { '.' });
                    index = end;
                    continue;
                }
            }
        }
        if lower_eq(chars, index, "&commat;") {
            out.push('@');
            index += 8;
            continue;
        }
        if lower_eq(chars, index, "&period;") {
            out.push('.');
            index += 8;
            continue;
        }
        if c == '%' {
            let mut at = index + 1;
            while lower_eq(chars, at, "25") {
                at += 2;
            }
            if lower_eq(chars, at, "40") {
                out.push('@');
                index = at + 2;
                continue;
            }
            if lower_eq(chars, at, "2e") {
                out.push('.');
                index = at + 2;
                continue;
            }
        }
        if c == '\\' {
            if lower_eq(chars, index, "\\u0040") {
                out.push('@');
                index += 6;
                continue;
            }
            if lower_eq(chars, index, "\\x40") {
                out.push('@');
                index += 4;
                continue;
            }
            if lower_eq(chars, index, "\\u002e") {
                out.push('.');
                index += 6;
                continue;
            }
        }
        if c == '='
            && chars.get(index + 1) == Some(&'4')
            && chars.get(index + 2) == Some(&'0')
            && index > 0
            && chars[index - 1].is_alphanumeric()
            && chars.get(index + 3).is_some_and(|d| d.is_alphanumeric())
        {
            out.push('@');
            index += 3;
            continue;
        }
        out.push(c);
        index += 1;
    }
    out
}

const AT_MARKERS: &[&str] = &[
    "(at)",
    "[at]",
    "{at}",
    "<at>",
    "( at )",
    "[ at ]",
    "{ at }",
    "\u{3014}at\u{3015}",
    "\u{3010}at\u{3011}",
];

const DOT_MARKERS: &[&str] = &[
    "(dot)", "[dot]", "{dot}", "<dot>", "( dot )", "[ dot ]", "[.]", "(.)", "{.}",
];

/// A bracketed `at` or `dot` becomes `@` or `.`, and the spaces around it go.
fn replace_markers(chars: &[char]) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut index = 0;
    'outer: while index < chars.len() {
        for (markers, replacement) in [(AT_MARKERS, '@'), (DOT_MARKERS, '.')] {
            for marker in markers {
                if lower_eq(chars, index, marker) {
                    while out.last().is_some_and(|c| c.is_whitespace()) {
                        out.pop();
                    }
                    out.push(replacement);
                    index += marker.chars().count();
                    while chars.get(index).is_some_and(|c| c.is_whitespace()) {
                        index += 1;
                    }
                    continue 'outer;
                }
            }
        }
        if lower_eq(chars, index, "(a)")
            && index > 0
            && chars[index - 1].is_alphanumeric()
            && chars.get(index + 3).is_some_and(|c| c.is_alphanumeric())
        {
            out.push('@');
            index += 3;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn local_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

fn domain_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '-')
}

/// `name@host.tld`, also with spaces around the `@`. The domain needs a
/// dot and an alphabetic top-level domain, so `pkg@1.2.3`, `foo@bar`,
/// `@Override` and `img@sha256:abc` are not addresses, and the `git` user
/// of a remote URL is not a mailbox. With a space in the shape the top-level
/// domain must also be a known one.
fn direct_email(chars: &[char]) -> bool {
    let mut squeezed: Vec<char> = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == ' '
            && chars.get(index + 1) == Some(&'.')
            && chars.get(index + 2) == Some(&' ')
            && index > 0
            && chars[index - 1].is_alphanumeric()
            && chars.get(index + 3).is_some_and(|c| c.is_alphanumeric())
        {
            squeezed.push('.');
            index += 3;
            continue;
        }
        squeezed.push(chars[index]);
        index += 1;
    }
    for (at, &c) in squeezed.iter().enumerate() {
        if c != '@' {
            continue;
        }
        let mut left = at;
        let mut spaces_before = 0;
        while left > 0 && squeezed[left - 1] == ' ' && spaces_before < 2 {
            left -= 1;
            spaces_before += 1;
        }
        let local_end = left;
        while left > 0 && local_char(squeezed[left - 1]) {
            left -= 1;
        }
        if local_end == left {
            continue;
        }
        let local: String = squeezed[left..local_end].iter().collect();
        if local.eq_ignore_ascii_case("git")
            || PLACEHOLDER_HOMES.contains(&local.to_lowercase().as_str())
            || !local.chars().any(char::is_alphanumeric)
        {
            continue;
        }
        let mut right = at + 1;
        let mut spaces_after = 0;
        while right < squeezed.len() && squeezed[right] == ' ' && spaces_after < 2 {
            right += 1;
            spaces_after += 1;
        }
        let mut stop = right;
        while stop < squeezed.len() && domain_char(squeezed[stop]) {
            stop += 1;
        }
        let domain: String = squeezed[right..stop]
            .iter()
            .collect::<String>()
            .to_lowercase();
        let domain = domain.trim_end_matches(['.', '-']);
        let labels: Vec<&str> = domain.split('.').collect();
        if labels.len() < 2 || labels.iter().any(|label| label.is_empty()) {
            continue;
        }
        let tld = labels[labels.len() - 1];
        if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        if (spaces_before > 0 || spaces_after > 0) && !KNOWN_TLDS.contains(&tld) {
            continue;
        }
        return true;
    }
    false
}

fn leet_letters(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'l',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            other => other,
        })
        .collect()
}

/// `name at host dot tld`, with `-` or `_` as the separator too. The name
/// must not be a word that precedes `at` in prose, so `look at the dot
/// product` and `meet me at noon dot sharp` stay.
fn spelled_email(chars: &[char]) -> bool {
    let text: String = chars.iter().collect();
    let dashed: String = text
        .chars()
        .map(|c| if matches!(c, '-' | '_') { ' ' } else { c })
        .collect();
    spelled_in(&text) || spelled_in(&dashed)
}

fn spelled_in(text: &str) -> bool {
    let tokens: Vec<String> = text
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| {
                matches!(
                    c,
                    ',' | ';' | ':' | '!' | '?' | '"' | '\'' | '(' | ')' | '`' | '[' | ']'
                )
            })
            .to_lowercase()
        })
        .collect();
    for (at, token) in tokens.iter().enumerate() {
        if !matches!(token.as_str(), "at" | "@") || at == 0 {
            continue;
        }
        let name = &tokens[at - 1];
        if name.is_empty()
            || name.chars().count() > 40
            || !name.chars().any(char::is_alphabetic)
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
            || AT_STOP.contains(&name.as_str())
        {
            continue;
        }
        let mut host_at = at + 1;
        if tokens.get(host_at).map(String::as_str) == Some("the") {
            host_at += 1;
        }
        let Some(host) = tokens.get(host_at) else {
            continue;
        };
        if host.contains('.') {
            if PROVIDERS.contains(&host.trim_end_matches('.')) {
                return true;
            }
            continue;
        }
        if host.is_empty()
            || host.chars().count() > 40
            || !host.chars().all(char::is_alphanumeric)
            || HOST_STOP.contains(&host.as_str())
        {
            continue;
        }
        if !matches!(
            tokens.get(host_at + 1).map(String::as_str),
            Some("dot" | ".")
        ) {
            continue;
        }
        if let Some(tld) = tokens.get(host_at + 2) {
            let letters = leet_letters(tld.trim_end_matches('.'));
            if (2..=10).contains(&letters.chars().count())
                && letters.chars().all(|c| c.is_ascii_alphabetic())
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare() -> Screen {
        Screen::default()
    }

    #[test]
    fn each_class_has_a_name() {
        let cases = [
            ("ask Claude", Class::Term, "term"),
            ("ask saddle", Class::Seat, "seat"),
            ("in /home/ada", Class::Path, "path"),
            ("password: x", Class::Secret, "secret"),
            ("at 10.0.0.1", Class::Address, "address"),
            ("at ::a", Class::Address, "address"),
            ("to a@b.example", Class::Mailbox, "email"),
            ("call_foo()", Class::Org, "org-markup"),
        ];
        let screen = Screen {
            seats: vec!["saddle".into()],
            ..Screen::default()
        };
        for (text, class, name) in cases {
            assert_eq!(classify(text, &screen), Some(class), "{text}");
            assert_eq!(class.name(), name);
        }
        assert_eq!(classify("Publish the archive.", &screen), None);
    }

    #[test]
    fn the_same_text_gets_the_same_answer_as_a_line_and_as_a_block() {
        let screen = bare();
        let text = "Tune 3.5 ms latency";
        assert_eq!(classify(text, &screen), None);
        let block = scan(
            &[Line::text("** TODO server-a Tune 3.5 ms latency")],
            &screen,
        );
        assert_eq!(block, vec![None]);
        assert_eq!(classify_id("server-a", &screen), None);
    }

    #[test]
    fn defanging_keeps_following_ipv6_text_aligned() {
        let screen = bare();
        for mark in ["[.]", "(.)", "{.}", "\\.", "[dot]", " dot "] {
            let prefix = format!("1{mark}2");

            let text = format!("{prefix} Face::Fade");
            assert_eq!(classify(&text, &screen), None, "{text:?}");
        }
    }

    #[test]
    fn a_number_is_read_as_inet_aton_reads_it() {
        for (text, value) in [
            ("10.0.0.1", Some(0x0a00_0001)),
            ("012.0.0.1", Some(0x0a00_0001)),
            ("00012.0.0.1", Some(0x0a00_0001)),
            ("0000000012.0.0.1", Some(0x0a00_0001)),
            ("0x0000000a.0.0.1", Some(0x0a00_0001)),
            ("0X0A.0.0.1", Some(0x0a00_0001)),
            ("167772161", Some(0x0a00_0001)),
            ("10.1", Some(0x0a00_0001)),
            ("10.0.1", Some(0x0a00_0001)),
            ("0x7f.1", Some(0x7f00_0001)),
            ("10.65536", Some(0x0a01_0000)),
            ("4294967295", Some(u32::MAX)),
            ("0x1ffffffff", None),
            ("4294967296", None),
            ("10.16777216", None),
            ("256.0.0.1", None),
            ("1.2.3.4.5", None),
            ("08.0.0.1", None),
            ("0x.0.0.1", None),
            ("0xg.0.0.1", None),
            ("+1.0.0.1", None),
            ("10..1", None),
        ] {
            assert_eq!(inet_aton(text), value, "{text:?}");
        }
    }

    #[test]
    fn a_private_number_in_any_inet_aton_form_is_refused() {
        let screen = bare();
        for text in [
            "00012.0.0.1",
            "0000000012.0.0.1",
            "0x0000000a.0.0.1",
            "012.0.0.1",
            "10.0.0.1",
            "ping 167772161",
            "host 10.1",
            "ping 127.1",
        ] {
            assert_eq!(classify(text, &screen), Some(Class::Address), "{text:?}");
        }
        for text in [
            "8.8.8.8",
            "release v10.0.0.5",
            "0x1ffffffff",
            "ping 0x1ffffffff",
            "0x10000000a.0.0.1",
            "167772161 rows",
            "10.1 percent",
            "2130706433",
        ] {
            assert_eq!(classify(text, &screen), None, "{text:?}");
        }
        for text in [
            "ip 167772161",
            "ip 10.1",
            "e.g.10.0.0.1",
            "x.10.0.0.1",
            "x10.0.0.1",
            "version=x10.0.0.1",
            "10.1@",
            "10.1//",
            "::",
            "fe80::1",
            "1[.]2 ::a",
        ] {
            assert_eq!(classify(text, &screen), Some(Class::Address), "{text:?}");
        }
    }

    #[test]
    fn a_digit_free_ipv6_literal_in_a_code_span_is_refused() {
        let screen = bare();
        for text in ["`::a`", "=::a=", "run `dead::beef` now"] {
            assert_eq!(classify(text, &screen), Some(Class::Address), "{text:?}");
        }
    }

    #[test]
    fn an_ipv6_literal_after_a_dot_is_refused() {
        let screen = bare();
        for text in ["x.::a", "x..::a", "x.dead::beef"] {
            assert_eq!(classify(text, &screen), Some(Class::Address), "{text:?}");
        }
    }

    #[test]
    fn only_a_contiguous_netmask_is_exempt() {
        let screen = bare();
        for text in ["255.0.255.0", "255.255.0.255", "255.254.255.0"] {
            assert_eq!(classify(text, &screen), Some(Class::Address), "{text:?}");
        }
        for text in [
            "255.255.240.0",
            "255.255.255.255",
            "255.255.255.0",
            "0.0.0.0",
        ] {
            assert_eq!(classify(text, &screen), None, "{text:?}");
        }
    }

    #[test]
    fn an_address_word_after_a_short_private_number_refuses_it() {
        let screen = bare();
        for text in ["127.1 host", "10.1 is the gateway", "167772161 server"] {
            assert_eq!(classify(text, &screen), Some(Class::Address), "{text:?}");
        }
        for text in ["Ubuntu 10.04 LTS", "$10.05", "at 10.05", "10.1 percent"] {
            assert_eq!(classify(text, &screen), None, "{text:?}");
        }
    }

    #[test]
    fn the_export_and_the_excerpt_read_one_matcher() {
        let screen = bare();
        for text in [
            concat!("Use s", "k-abc for the bench."),
            "Seed with a1b2c3d4e5f6a7b8c9d0e1f2g3 first.",
            "Set token=abc when firing.",
            concat!("Send gh", "p_x with it."),
        ] {
            assert!(refuses(text, &screen), "{text}");
            assert!(text.split_whitespace().any(secret_word), "{text}");
        }
        for text in [
            "The name is =api_key= here.",
            "Set password= and then read it.",
            "See prompt-hook.rows-10000.after for it.",
            "The commit is 6fac806b5ab91106bdbbdf24429a191d5371f349.",
            "if token == Token::Eof then stop",
        ] {
            assert!(!refuses(text, &screen), "{text}");
        }
        assert!(secret_word("password="), "an excerpt redacts a bare marker");
    }

    #[test]
    fn the_export_and_the_excerpt_agree_word_for_word_but_where_documented() {
        let screen = bare();
        let both = [
            "key=x",
            concat!("s", "k-abc"),
            concat!("s", "k-"),
            concat!("gh", "p_x"),
            concat!("AK", "IAshort"),
            concat!("(s", "k-abc)"),
            concat!("\"s", "k-abc\""),
            concat!("=s", "k-abc="),
            concat!("'gh", "p_x',"),
            concat!("[xo", "xb-1]"),
            "password==hunter2",
            "password==x",
            "token::abc123",
            "secret::x",
            "token:=abc",
            concat!("xo", "xb-1"),
            concat!("gl", "pat-x"),
            "a1a1a1a1a1a1a1a1a1a1a1a1",
            "abcdefghij0123456789klmno",
            "0123456789abcdef0123456789",
            "a1b2c3d4e5f6-g7h8i9j0k1l2m3",
            "a1b2c3d4e5f6.g7h8i9j0k1l2m3",
            "a1b2c3d4e5f6_g7h8i9j0k1l2m3",
        ];
        for word in both {
            assert!(secret_word(word), "excerpt: {word}");
            assert!(refuses(word, &screen), "export: {word}");
        }
        let excerpt_only = [
            "token=",
            "password=",
            "token:",
            "key:",
            "=api_key=",
            "Token::Eof",
            "abc-def-ghi-jkl-mno-pqr-stu-0123",
            "prompt-hook.rows-10000.after",
            "aarch64-unknown-linux-gnu-static",
        ];
        for word in excerpt_only {
            assert!(secret_word(word), "excerpt: {word}");
            assert!(!refuses(word, &screen), "export: {word}");
        }
        for word in ["plain", "well-known", "iso-8601", "x86-64", "utf-8"] {
            assert!(!secret_word(word) && !refuses(word, &screen), "{word}");
        }
        for text in ["pw: hunter2", "The pw: hunter2 is set.", "pass: hunter2"] {
            assert!(refuses(text, &screen), "{text}");
        }
        assert!(!refuses("The env var is password: on its own.", &screen));
    }

    #[test]
    fn a_run_cut_over_lines_is_found_and_two_words_are_not() {
        let screen = bare();
        let found = scan(
            &[
                Line::text("Bench value"),
                Line::text("a1b2c3d4e5"),
                Line::text("f6g7h8i9j0"),
                Line::text("k1l2m3n4o5"),
            ],
            &screen,
        );
        assert_eq!(
            found.iter().map(Option::is_some).collect::<Vec<_>>(),
            vec![false, true, true, true]
        );
        let words = scan(
            &[Line::text("an ordinary"), Line::text("sentence of words")],
            &screen,
        );
        assert_eq!(words, vec![None, None]);
    }

    #[test]
    fn a_secret_split_by_a_line_break_marks_both_lines() {
        let found = scan(
            &[Line::text("AKI"), Line::text("AIOSFODNN7EXAMPLE")],
            &bare(),
        );
        assert_eq!(found, vec![Some(Class::Secret), Some(Class::Secret)]);
        let prefix_alone = scan(
            &[
                Line::text(concat!("AK", "IAIOSFODNN")),
                Line::text("7EXAMPLE"),
            ],
            &bare(),
        );
        assert_eq!(prefix_alone[0], Some(Class::Secret));
        let clean = scan(&[Line::text("one"), Line::text("two")], &bare());
        assert_eq!(clean, vec![None, None]);
    }

    #[test]
    fn every_line_of_a_private_key_block_is_flagged() {
        let lines = [
            "keep",
            concat!("-----BEGIN PRIVATE", " KEY-----"),
            "MIIEvQIBADAN",
            "abcdefabcdef",
            concat!("-----END PRIVATE", " KEY-----"),
            "after",
        ];
        let lines: Vec<Line<'_>> = lines.iter().map(|text| Line::text(text)).collect();
        let found: Vec<bool> = scan(&lines, &bare()).iter().map(Option::is_some).collect();
        assert_eq!(found, vec![false, true, true, true, true, false]);
    }

    #[test]
    fn an_id_made_of_hex_digits_is_an_opaque_key_not_a_credential() {
        let id = "0123456789abcdef0123456789abcdef0123";
        assert_eq!(classify_id(id, &bare()), None);
        assert!(refuses(id, &bare()));
    }

    #[test]
    fn a_seat_name_is_refused_as_a_word() {
        let screen = Screen {
            seats: vec!["saddle".into()],
            ..Screen::default()
        };
        assert!(refuses("Ask saddle later.", &screen));
        assert!(!refuses("Publish the signed archive.", &screen));
    }

    #[test]
    fn the_detection_copy_drops_invisible_characters() {
        assert_eq!(detection_copy("g\u{200b}pt \u{ff0a}"), "gpt *");
    }
}
