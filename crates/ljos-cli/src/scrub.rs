//! Detection for author text that a public page may carry.
//!
//! Detection normalizes a copy. Rendering keeps the author's bytes.

use std::net::{Ipv4Addr, Ipv6Addr};

use icu_properties::{
    props::{DefaultIgnorableCodePoint, GeneralCategory},
    CodePointMapData, CodePointSetData,
};
use unicode_normalization::UnicodeNormalization as _;

include!("scrub_fold.rs");

/// Return `text` unchanged.
///
/// Kept as a compatibility helper for callers that used the old scrubber.
/// Public rendering now refuses detected author text instead of rewriting it.
#[must_use]
pub fn scrub_public(text: &str) -> String {
    text.to_string()
}

/// Return a human-readable hit description for every disallowed item found
/// in `text` without modifying it. An empty list means `text` is clean.
#[must_use]
pub fn scan_public(text: &str) -> Vec<String> {
    scan_with(text, &declared_seats(), true, 1)
}

/// Scan author-controlled text, assigning its first line `first_line`.
#[must_use]
pub fn scan_author(text: &str, first_line: usize) -> Vec<String> {
    scan_with(text, &declared_seats(), false, first_line)
}

/// Scan a structural step key before the renderer writes it.
#[must_use]
pub fn scan_step_key(text: &str, line: usize) -> Vec<String> {
    let seats = declared_seats();
    let mut hits = scan_with(text, &seats, false, line);
    let normalized = normalize_line(text);
    if seats
        .iter()
        .any(|seat| seat.eq_ignore_ascii_case(&normalized))
    {
        hits.push(format!("line {line} contains seat name"));
    }
    hits
}

/// Scan a `:ROLE:` value. A value that is a declared seat is a hit.
#[must_use]
pub fn scan_role(text: &str, line: usize) -> Vec<String> {
    let mut hits = scan_author(text, line);
    let normalized = normalize_line(text);
    let trimmed = normalized.trim();
    if declared_seats()
        .iter()
        .any(|seat| seat.eq_ignore_ascii_case(trimmed))
    {
        hits.push(format!("line {line} contains seat name"));
    }
    hits
}

/// Scan body text and reject normalized Org structure prefixes.
#[must_use]
pub fn scan_body(text: &str, first_line: usize) -> Vec<String> {
    let mut hits = scan_with(text, &declared_seats(), false, first_line);
    for (idx, line) in text.lines().enumerate() {
        let normalized = normalize_line(line);
        let trimmed = normalized.trim_start();
        if trimmed.starts_with('*') || trimmed.starts_with("#+") || is_drawer_start(trimmed) {
            hits.push(format!("line {} contains Org structure", first_line + idx));
        }
    }
    hits
}

fn scan_with(text: &str, seats: &[String], skip_generated: bool, first_line: usize) -> Vec<String> {
    let bodies: Vec<&str> = text
        .split_inclusive('\n')
        .map(|line| line.strip_suffix('\n').unwrap_or(line))
        .collect();
    let mut hits = Vec::new();
    for (idx, body) in bodies.iter().enumerate() {
        let line_no = first_line + idx;
        if invisible_hit(body) {
            hits.push(format!("line {line_no} contains invisible character"));
        }
        let normalized = normalize_line(body);
        let chars: Vec<char> = normalized.chars().collect();
        let generated = skip_generated && is_generated_line(body);
        if generated {
            let mut i = 0;
            while i < chars.len() {
                if let Some(end) = take_secret(&chars, i) {
                    hits.push(format!("line {line_no} contains secret"));
                    i = end.max(i + 1);
                } else {
                    i = skip_run(&chars, i);
                }
            }
            continue;
        }
        if org_export_construct(&normalized) {
            hits.push(format!("line {line_no} contains Org structure"));
        }
        if names_model_or_bot(&normalized) {
            hits.push(format!("line {line_no} contains tool or model name"));
        }
        let mut i = 0;
        while i < chars.len() {
            if let Some((end, kind)) = match_token(&chars, i, seats) {
                hits.push(format!("line {line_no} contains {}", kind.label()));
                i = end.max(i + 1);
            } else {
                i = skip_run(&chars, i);
            }
        }
    }
    for idx in 0..bodies.len().saturating_sub(1) {
        let left = bodies[idx];
        let right = bodies[idx + 1];
        if skip_generated && (is_generated_line(left) || is_generated_line(right)) {
            continue;
        }
        let line_no = first_line + idx;
        for kind in cross_line_kinds(left, right) {
            let label = kind.label();
            let already = hits.iter().any(|hit| {
                hit == &format!("line {line_no} contains {label}")
                    || hit == &format!("line {} contains {label}", line_no + 1)
            });
            if !already {
                hits.push(format!("line {line_no} contains {label}"));
            }
        }
    }
    hits
}

/// A secret or mailbox split by a newline. Each line is whitespace-folded,
/// then the pair is joined with no separator and with one space. A hit counts
/// only when the match crosses the join and neither line has that class alone.
fn cross_line_kinds(left: &str, right: &str) -> Vec<HitKind> {
    let left = fold_space(&normalize_line(left));
    let right = fold_space(&normalize_line(right));
    if left.is_empty() || right.is_empty() {
        return Vec::new();
    }
    let mut kinds = Vec::new();
    let break_at = left.chars().count();
    for (joined, gap) in [
        (format!("{left}{right}"), 0usize),
        (format!("{left} {right}"), 1),
    ] {
        let chars: Vec<char> = joined.chars().collect();
        for (start, end, kind) in secret_email_spans(&chars) {
            // `gap` is 1 when a space was inserted. The match has to include
            // a character from each line, not only that space.
            let crosses = start < break_at && end > break_at + gap;
            if crosses && !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
    }
    kinds.retain(|kind| !line_has_kind(&left, *kind) && !line_has_kind(&right, *kind));
    kinds
}

fn fold_space(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn line_has_kind(normalized_folded: &str, kind: HitKind) -> bool {
    let chars: Vec<char> = normalized_folded.chars().collect();
    secret_email_spans(&chars).iter().any(|span| span.2 == kind)
}

fn secret_email_spans(chars: &[char]) -> Vec<(usize, usize, HitKind)> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some((end, kind)) = match_token(chars, i, &[]) {
            if matches!(kind, HitKind::Secret | HitKind::Email) {
                spans.push((i, end, kind));
                i = end.max(i + 1);
                continue;
            }
        }
        i = skip_run(chars, i);
    }
    spans
}

fn skip_run(chars: &[char], i: usize) -> usize {
    let mut j = i + 1;
    if chars.get(i).is_some_and(|c| is_scan_run(*c)) {
        while j < chars.len() && is_scan_run(chars[j]) {
            j += 1;
        }
    }
    j
}

fn is_scan_run(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | '/')
}

fn org_export_construct(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("#+begin_export")
        || trimmed.starts_with("#+call:")
        || trimmed.starts_with("#+include:")
        || trimmed.starts_with("#+setupfile:")
        || line.contains("{{{")
        || has_export_snippet(line)
        || has_inline_babel(line)
        || has_html_event(line)
}

fn has_export_snippet(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == b'@' && bytes[i + 1] == b'@' {
            let mut j = i + 2;
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b':' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

fn has_inline_babel(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 5 < bytes.len() {
        let src = bytes[i..].starts_with(b"src_");
        let call = bytes[i..].starts_with(b"call_");
        if (src || call) && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let mut j = i + if src { 4 } else { 5 };
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-') {
                    j += 1;
                }
                let open = if src { b'{' } else { b'(' };
                if j < bytes.len() && bytes[j] == open {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

fn has_html_event(line: &str) -> bool {
    if line.contains("<script") || line.contains("<iframe") {
        return true;
    }
    (line.contains("<img") || line.contains("<svg") || line.contains("<video"))
        && (line.contains("onerror")
            || line.contains("onload")
            || line.contains("onclick")
            || line.contains("onmouseover"))
}

fn is_generated_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(":STEP:")
        || trimmed.starts_with(":DEPENDS:")
        || trimmed.starts_with(":KIND:")
        || trimmed == ":PROPERTIES:"
        || trimmed == ":END:"
        || trimmed == "#+begin_src dot"
        || trimmed == "#+end_src"
        || trimmed == "digraph workflow {"
        || trimmed == "rankdir=LR;"
        || trimmed == "}"
        || (trimmed.starts_with('"') && (trimmed.contains(" [label=") || trimmed.contains(" -> ")))
}

fn is_drawer_start(line: &str) -> bool {
    let Some(rest) = line.strip_prefix(':') else {
        return false;
    };
    let Some((name, after)) = rest.split_once(':') else {
        return false;
    };
    !name.is_empty()
        && after.trim().is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_invisible(c: char) -> bool {
    let ignorable = CodePointSetData::new::<DefaultIgnorableCodePoint>();
    let gc = CodePointMapData::<GeneralCategory>::new();
    ignorable.contains(c) || gc.get(c) == GeneralCategory::Format || c == '\u{180E}'
}

fn is_nonspacing(c: char) -> bool {
    let gc = CodePointMapData::<GeneralCategory>::new();
    gc.get(c) == GeneralCategory::NonspacingMark
}

fn is_zwj(c: char) -> bool {
    matches!(c, '\u{200c}' | '\u{200d}')
}

/// Arabic and Indic letters use ZWJ and ZWNJ as part of the word.
fn uses_zwj(c: char) -> bool {
    matches!(
        c,
        '\u{0600}'..='\u{06FF}'
            | '\u{0750}'..='\u{077F}'
            | '\u{08A0}'..='\u{08FF}'
            | '\u{0900}'..='\u{0D7F}'
            | '\u{FB50}'..='\u{FDFF}'
            | '\u{FE70}'..='\u{FEFF}'
    )
}

fn invisible_hit(line: &str) -> bool {
    let linguistic = line.chars().any(uses_zwj);
    line.chars()
        .any(|c| is_invisible(c) && !(linguistic && is_zwj(c)))
}

/// A copy for matching. The author's bytes stay as they were.
///
/// The copy is NFKC, then invisibles are dropped, then combining marks are
/// dropped, then each character is replaced by its confusable skeleton and
/// lowercased. Phrase matching also squeezes whitespace.
fn normalize_line(line: &str) -> String {
    let compatible: String = line.nfkc().filter(|&c| !is_invisible(c)).collect();
    let mut out = String::new();
    for c in compatible.nfd() {
        if is_nonspacing(c) || is_invisible(c) {
            continue;
        }
        let folded = fold_lookalike(c);
        if let Some(skel) = fold_confusable(folded) {
            for piece in skel.chars() {
                out.extend(piece.to_lowercase());
            }
        } else {
            out.extend(folded.to_lowercase());
        }
    }
    out
}

fn declared_seats() -> Vec<String> {
    let mut names = Vec::new();
    for seat in crate::tracker_roster_names() {
        let norm = normalize_line(seat.trim());
        if !norm.is_empty() {
            names.push(norm);
        }
    }
    names.retain(|name| {
        name.chars().count() >= 2
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    });
    names.sort_by(|a, b| {
        b.chars()
            .count()
            .cmp(&a.chars().count())
            .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
    });
    names.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
    names
}

fn fold_lookalike(c: char) -> char {
    match c {
        'А' | 'а' | 'Α' | 'α' | 'ɑ' => 'a',
        'В' | 'в' | 'ь' => 'b',
        'С' | 'с' | 'Ϲ' | 'ϲ' | 'ᴄ' => 'c',
        'ԁ' | 'ɗ' => 'd',
        'Е' | 'е' | 'Ε' | 'ε' | 'є' | 'ᴇ' => 'e',
        'ғ' => 'f',
        'ɡ' => 'g',
        'Η' | 'һ' | 'Н' | 'н' => 'h',
        'Ι' | 'І' | 'і' | 'ї' | 'ι' | 'ɪ' => 'i',
        'Ј' | 'ј' | 'ʝ' => 'j',
        'Κ' | 'К' | 'к' | 'κ' => 'k',
        'ӏ' | 'ⅼ' => 'l',
        'Μ' | 'М' | 'м' | 'ᴍ' => 'm',
        'ո' | 'П' | 'п' | 'ν' => 'n',
        'Ο' | 'О' | 'о' | 'ο' | 'σ' | 'ᴏ' => 'o',
        'Ρ' | 'Р' | 'р' | 'ρ' => 'p',
        'ԛ' => 'q',
        'г' | 'ʀ' => 'r',
        'ѕ' | 'ꜱ' => 's',
        'Τ' | 'Т' | 'т' | 'τ' | 'ᴛ' => 't',
        'υ' | 'ս' => 'u',
        'ѵ' => 'v',
        'ԝ' | 'ω' => 'w',
        'Χ' | 'Х' | 'х' | 'χ' => 'x',
        'Υ' | 'У' | 'у' | 'ү' | 'γ' => 'y',
        'ᴢ' => 'z',
        other => other,
    }
}

/// The same vocabulary the handbook screen matches, plus the unversioned
/// product names a public page was still publishing.
const TERMS: &[&str] = &[
    "gpt",
    "chatgpt",
    "claude",
    "anthropic",
    "cursor",
    "copilot",
    "gemini",
    "llama",
    "deepseek",
    "qwen",
    "grok",
    "openai",
    "agent",
    "subagent",
    "sub-agent",
    "o1",
    "o3",
    "o4",
    "llm",
    "genai",
    "gen-ai",
    "language model",
    "foundation model",
    "model context protocol",
    "ai",
    "cohere",
    "huggingface",
    "hugging face",
    "hugging-face",
    "hugging_face",
    "xai",
    "x.ai",
    "gemma",
    "deepmind",
    "ollama",
    "openrouter",
    "groq",
    "windsurf",
    "cline",
    "kiro",
    "aider",
    "codeium",
    "tabnine",
    "midjourney",
    "dall-e",
    "dalle",
    "kimi",
    "glm",
    "ernie",
    "minimax",
    "stable diffusion",
    "bard",
    "ai-generated",
    "ai generated",
    "cloud agent",
    "cloud-agent",
    "co-authored-by",
    "co-authored by",
];

fn names_model_or_bot(line: &str) -> bool {
    let squeezed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if TERMS
        .iter()
        .any(|phrase| contains_phrase(&squeezed, phrase))
    {
        return true;
    }
    let chars: Vec<char> = squeezed.chars().collect();
    has_product_version(&chars, "gpt", false)
        || has_product_version(&chars, "claude", false)
        || has_named_product_version(&chars, "claude", "sonnet")
        || has_named_product_version(&chars, "claude", "opus")
        || has_family_version(&chars, "sonnet")
        || has_product_version(&chars, "gemini", false)
        || has_product_version(&chars, "qwen", true)
        || has_product_version(&chars, "llama", false)
        || has_product_version(&chars, "deepseek", true)
        || has_product_version(&chars, "mistral", false)
        || has_o_series_mini(&chars)
}

fn term_edge(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

fn contains_phrase(line: &str, phrase: &str) -> bool {
    line.match_indices(phrase).any(|(at, found)| {
        let before = line[..at].chars().next_back();
        let after = line[at + found.len()..].chars().next();
        before.is_none_or(|c| !term_edge(c)) && after.is_none_or(|c| !term_edge(c))
    })
}

fn has_product_version(chars: &[char], product: &str, joined: bool) -> bool {
    let needle: Vec<char> = product.chars().collect();
    for at in 0..chars.len() {
        if at + needle.len() > chars.len() || chars[at..at + needle.len()] != needle {
            continue;
        }
        if at > 0 && chars[at - 1].is_alphanumeric() {
            continue;
        }
        let mut n = at + needle.len();
        if chars.get(n).is_some_and(|c| matches!(c, ' ' | '-')) {
            while chars.get(n).is_some_and(|c| matches!(c, ' ' | '-')) {
                n += 1;
            }
        }
        if joined && chars.get(n) == Some(&'v') {
            n += 1;
        }
        if !chars.get(n).is_some_and(char::is_ascii_digit) {
            continue;
        }
        n += 1;
        while chars
            .get(n)
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        {
            n += 1;
        }
        if n == chars.len() || !chars[n].is_alphanumeric() {
            return true;
        }
    }
    false
}

fn has_named_product_version(chars: &[char], product: &str, family: &str) -> bool {
    let product: Vec<char> = product.chars().collect();
    let family: Vec<char> = family.chars().collect();
    for at in 0..chars.len() {
        if at + product.len() > chars.len() || chars[at..at + product.len()] != product {
            continue;
        }
        if at > 0 && chars[at - 1].is_alphanumeric() {
            continue;
        }
        let mut n = at + product.len();
        if !chars.get(n).is_some_and(|c| matches!(c, ' ' | '-')) {
            continue;
        }
        while chars.get(n).is_some_and(|c| matches!(c, ' ' | '-')) {
            n += 1;
        }
        if n + family.len() > chars.len() || chars[n..n + family.len()] != family {
            continue;
        }
        n += family.len();
        while chars.get(n).is_some_and(|c| matches!(c, ' ' | '-')) {
            n += 1;
        }
        if chars.get(n).is_some_and(char::is_ascii_digit) {
            return true;
        }
    }
    false
}

fn has_family_version(chars: &[char], family: &str) -> bool {
    let family: Vec<char> = family.chars().collect();
    for at in 0..chars.len() {
        if at + family.len() > chars.len() || chars[at..at + family.len()] != family {
            continue;
        }
        if at > 0 && term_edge(chars[at - 1]) {
            continue;
        }
        let mut n = at + family.len();
        while chars.get(n).is_some_and(|c| matches!(c, ' ' | '-')) {
            n += 1;
        }
        if n == at + family.len() {
            continue;
        }
        if chars.get(n).is_some_and(char::is_ascii_digit) {
            return true;
        }
    }
    false
}

fn has_o_series_mini(chars: &[char]) -> bool {
    chars.windows(7).enumerate().any(|(at, window)| {
        (at == 0 || !chars[at - 1].is_alphanumeric())
            && window[0] == 'o'
            && window[1].is_ascii_digit()
            && window[2] == '-'
            && window[3..] == ['m', 'i', 'n', 'i']
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitKind {
    Secret,
    FileUrl,
    LocalPath,
    Email,
    PrivateHost,
    IpAddress,
    MarkedId,
    Seat,
}

impl HitKind {
    fn label(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::FileUrl | Self::LocalPath => "private path",
            Self::Email => "email",
            Self::PrivateHost => "host",
            Self::IpAddress => "address",
            Self::MarkedId => "tool or model name",
            Self::Seat => "seat name",
        }
    }
}

fn match_token(chars: &[char], i: usize, seats: &[String]) -> Option<(usize, HitKind)> {
    if let Some(end) = take_secret(chars, i) {
        return Some((end, HitKind::Secret));
    }
    if let Some(end) = take_file_url(chars, i) {
        return Some((end, HitKind::FileUrl));
    }
    if let Some(end) = take_path(chars, i) {
        return Some((end, HitKind::LocalPath));
    }
    if let Some(end) = take_email(chars, i) {
        return Some((end, HitKind::Email));
    }
    if let Some(end) = take_private_host(chars, i) {
        return Some((end, HitKind::PrivateHost));
    }
    if let Some(end) = take_ip(chars, i) {
        return Some((end, HitKind::IpAddress));
    }
    if let Some(end) = take_marked(chars, i, &["bc-", "agent-"]) {
        return Some((end, HitKind::MarkedId));
    }
    if let Some(end) = take_known_seat_in_context(chars, i, seats) {
        return Some((end, HitKind::Seat));
    }
    None
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn starts_with_ignore_ascii_case(chars: &[char], at: usize, prefix: &str) -> Option<usize> {
    let mut i = at;
    for p in prefix.chars() {
        let c = *chars.get(i)?;
        if !c.eq_ignore_ascii_case(&p) {
            return None;
        }
        i += 1;
    }
    Some(i)
}

fn starts_with_exact(chars: &[char], at: usize, prefix: &str) -> Option<usize> {
    let mut i = at;
    for p in prefix.chars() {
        if *chars.get(i)? != p {
            return None;
        }
        i += 1;
    }
    Some(i)
}

fn take_secret(chars: &[char], i: usize) -> Option<usize> {
    if let Some(end) = take_url_credentials(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_slack_hook(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_curl_user(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_authorization(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_secret_assign(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_bearer(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_pem(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_jwt(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_secret_prefix(chars, i) {
        return Some(end);
    }
    take_opaque(chars, i)
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn secret_name(name: &str) -> bool {
    matches!(
        name,
        "token"
            | "secret"
            | "password"
            | "passwd"
            | "passphrase"
            | "pass"
            | "apikey"
            | "credential"
            | "credentials"
            | "api_key"
            | "access_token"
            | "auth_token"
            | "private_key"
            | "signing_key"
            | "client_secret"
            | "clientsecret"
            | "pgpassword"
            | "accountkey"
            | "sharedaccesskey"
    ) || name.ends_with("_key")
        || name.ends_with("_token")
        || name.ends_with("_secret")
        || name.ends_with("_password")
        || name.ends_with("_passwd")
        || name.ends_with("_passphrase")
        || name.ends_with("_credential")
        || name.ends_with("_credentials")
        || name.ends_with("_auth")
        || name.ends_with("_authorization")
        || (name.len() > "secret".len() && name.ends_with("secret"))
        || (name.len() > "password".len() && name.ends_with("password"))
}

fn take_secret_assign(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_name_char(chars[i - 1]) {
        return None;
    }
    if chars.get(i) == Some(&'-') {
        return None;
    }
    let mut j = i;
    let mut has_alpha = false;
    while j < chars.len() && is_name_char(chars[j]) {
        if chars[j].is_ascii_alphabetic() {
            has_alpha = true;
        }
        j += 1;
    }
    while j > i && chars[j - 1] == '-' {
        j -= 1;
    }
    if !has_alpha || j >= chars.len() {
        return None;
    }
    let ident_end = j;
    if matches!(chars.get(j), Some('"' | '\'')) {
        j += 1;
    }
    while j < chars.len() && matches!(chars[j], ' ' | '\t') {
        j += 1;
    }
    if j >= chars.len() {
        return None;
    }
    let sep = chars[j];
    if sep != '=' && sep != ':' {
        return None;
    }
    let ident: String = chars[i..ident_end].iter().collect();
    let folded = ident.replace('-', "_");
    let named = secret_name(&ident) || (ident.contains('-') && secret_name(&folded));
    if !named {
        return None;
    }
    let mut end = j + 1;
    while end < chars.len() && matches!(chars[end], ' ' | '\t') {
        end += 1;
    }
    let val_start = end;
    if let Some(&quote @ ('"' | '\'')) = chars.get(end) {
        end += 1;
        while end < chars.len() {
            if chars[end] == quote {
                end += 1;
                break;
            }
            end += 1;
        }
    } else {
        while end < chars.len()
            && !chars[end].is_whitespace()
            && !matches!(chars[end], ',' | ';' | ')' | ']' | '>' | '"' | '\'' | '`')
        {
            end += 1;
        }
    }
    if sep == ':' && end == val_start {
        return None;
    }
    let value: String = chars[val_start..end].iter().collect();
    let bare = value.trim_matches(|c| c == '"' || c == '\'');
    if sep == ':' && assignment_is_prose(bare, chars, end) {
        return None;
    }
    Some(end)
}

/// `pass: all tests green` is a sentence. `Authorization: Basic <token>` is not.
fn assignment_is_prose(value: &str, chars: &[char], end: usize) -> bool {
    if value.is_empty() || value.chars().any(|c| !c.is_ascii_alphabetic()) {
        return false;
    }
    let mut i = end;
    let mut words = 0usize;
    while i < chars.len() {
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while chars.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
            i += 1;
        }
        if i == start || chars.get(i).is_some_and(|c| !c.is_whitespace()) {
            return false;
        }
        if i - start >= 8 {
            return false;
        }
        words += 1;
    }
    words >= 2
}

fn take_url_credentials(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_name_char(chars[i - 1]) {
        return None;
    }
    if !chars.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut j = i + 1;
    while chars
        .get(j)
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, '+' | '-' | '.'))
    {
        j += 1;
    }
    let user_start = starts_with_exact(chars, j, "://")?;
    let mut k = user_start;
    let mut at = None;
    while k < chars.len() && !is_url_stop(chars[k]) && chars[k] != '/' {
        if chars[k] == '@' {
            at = Some(k);
            break;
        }
        k += 1;
    }
    let at = at?;
    if at == user_start {
        return None;
    }
    let userinfo = &chars[user_start..at];
    if !userinfo.contains(&':') && !userinfo.iter().any(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(at + 1)
}

fn take_authorization(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_ident_char(chars[i - 1]) {
        return None;
    }
    let mut n = starts_with_ignore_ascii_case(chars, i, "authorization")?;
    while chars.get(n).is_some_and(|c| matches!(c, ' ' | '\t')) {
        n += 1;
    }
    if chars.get(n) != Some(&':') {
        return None;
    }
    n += 1;
    while chars.get(n).is_some_and(|c| matches!(c, ' ' | '\t')) {
        n += 1;
    }
    for scheme in ["bearer", "token", "basic"] {
        if let Some(after) = starts_with_ignore_ascii_case(chars, n, scheme) {
            n = after;
            while chars.get(n).is_some_and(|c| matches!(c, ' ' | '\t')) {
                n += 1;
            }
            let start = n;
            while chars.get(n).is_some_and(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '~' | '+' | '-' | '/' | '=')
            }) {
                n += 1;
            }
            return (n > start).then_some(n);
        }
    }
    None
}

fn take_bearer(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_ident_char(chars[i - 1]) {
        return None;
    }
    let mut j = starts_with_ignore_ascii_case(chars, i, "bearer")?;
    let ws_start = j;
    while j < chars.len() && matches!(chars[j], ' ' | '\t') {
        j += 1;
    }
    if j == ws_start {
        return None;
    }
    let tok_start = j;
    while j < chars.len()
        && (chars[j].is_ascii_alphanumeric()
            || matches!(chars[j], '.' | '_' | '~' | '+' | '-' | '/' | '='))
    {
        j += 1;
    }
    if j - tok_start < 4 {
        return None;
    }
    Some(j)
}

fn take_pem(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1] == '-' {
        return None;
    }
    let mut j = starts_with_exact(chars, i, "-----begin ")?;
    let label = j;
    while j < chars.len() && chars[j] != '-' {
        j += 1;
    }
    if j == label || !chars[label..j].iter().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    starts_with_exact(chars, j, "-----")
}

fn take_jwt(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (chars[i - 1].is_ascii_alphanumeric() || matches!(chars[i - 1], '_' | '-')) {
        return None;
    }
    starts_with_exact(chars, i, "eyj")?;
    let header = scan_b64(chars, i)?;
    if header - i < 12 || chars.get(header) != Some(&'.') {
        return None;
    }
    let payload_at = header + 1;
    let payload = scan_b64(chars, payload_at)?;
    if payload - payload_at < 8 {
        return None;
    }
    if chars.get(payload) == Some(&'.') {
        let sig_at = payload + 1;
        if let Some(sig) = scan_b64(chars, sig_at) {
            if sig - sig_at >= 8 && jwt_boundary(chars, sig) {
                return Some(sig);
            }
            if sig == sig_at && jwt_boundary(chars, sig_at) {
                return Some(sig_at);
            }
        }
    }
    jwt_boundary(chars, payload).then_some(payload)
}

fn jwt_boundary(chars: &[char], at: usize) -> bool {
    chars
        .get(at)
        .is_none_or(|c| !(c.is_ascii_alphanumeric() || matches!(*c, '_' | '-')))
}

fn scan_b64(chars: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    if !chars
        .get(j)
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, '_' | '-'))
    {
        return None;
    }
    while chars
        .get(j)
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, '_' | '-'))
    {
        j += 1;
    }
    Some(j)
}

fn take_secret_prefix(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        return None;
    }
    if let Some(after) = starts_with_ignore_ascii_case(chars, i, concat!("x", "ox")) {
        let kind = *chars.get(after)?;
        if matches!(kind.to_ascii_lowercase(), 'a' | 'b' | 'p' | 'e' | 'r' | 's')
            && chars.get(after + 1) == Some(&'-')
        {
            let mut end = after + 2;
            while end < chars.len()
                && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '-'))
            {
                end += 1;
            }
            if end > after + 2 {
                return Some(end);
            }
        }
    }
    if let Some(after) = starts_with_ignore_ascii_case(chars, i, "gh") {
        let kind = *chars.get(after)?;
        if matches!(kind.to_ascii_lowercase(), 'p' | 'o' | 'u' | 's' | 'r')
            && chars.get(after + 1) == Some(&'_')
        {
            let mut end = after + 2;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            if end > after + 2 {
                return Some(end);
            }
        }
    }
    if let Some(after) = starts_with_ignore_ascii_case(chars, i, concat!("githu", "b_pat_")) {
        let mut end = after;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        if end > after {
            return Some(end);
        }
    }
    for prefix in ["sk_live_", "sk_test_", "rk_live_", concat!("h", "f_")] {
        if let Some(after) = starts_with_exact(chars, i, prefix) {
            let mut end = after;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            let min = if prefix == concat!("h", "f_") { 16 } else { 8 };
            if end - after >= min {
                return Some(end);
            }
        }
    }
    if let Some(after) = starts_with_exact(chars, i, concat!("s", "k-")) {
        let mut end = after;
        while end < chars.len()
            && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '-'))
        {
            end += 1;
        }
        if end - after >= 3 {
            return Some(end);
        }
    }
    if let Some(after) =
        starts_with_exact(chars, i, "akia").or_else(|| starts_with_exact(chars, i, "asia"))
    {
        let mut end = after;
        while end < chars.len() && chars[end].is_ascii_alphanumeric() {
            end += 1;
        }
        if end - after >= 4 {
            return Some(end);
        }
    }
    for prefix in [concat!("glp", "at-"), "gldt-", "glrt-", "xapp-"] {
        if let Some(after) = starts_with_exact(chars, i, prefix) {
            let mut end = after;
            while end < chars.len()
                && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '-'))
            {
                end += 1;
            }
            if end - after >= 3 {
                return Some(end);
            }
        }
    }
    if let Some(after) = starts_with_exact(chars, i, "npm_") {
        let mut end = after;
        while end < chars.len() && chars[end].is_ascii_alphanumeric() {
            end += 1;
        }
        if end - after >= 4 && chars[after..end].iter().any(|c| c.is_ascii_digit()) {
            return Some(end);
        }
    }
    if let Some(after) = starts_with_exact(chars, i, "aiza") {
        let mut end = after;
        while end < chars.len()
            && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '-'))
        {
            end += 1;
        }
        if end - after >= 8 {
            return Some(end);
        }
    }
    None
}

fn is_url_stop(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '<' | '>' | '`' | ')' | '(' | '[' | ']' | ',' | ';'
        )
}

fn take_file_url(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        return None;
    }
    let after = [
        "file+sys:",
        "file+emacs:",
        "file:",
        "elisp:",
        "shell:",
        "eshell:",
        "javascript:",
        "help:",
        "info:",
    ]
    .iter()
    .find_map(|prefix| starts_with_exact(chars, i, prefix))?;
    if chars.get(after).is_none_or(|c| is_link_stop(*c)) {
        return None;
    }
    let mut end = after;
    while end < chars.len() && !is_link_stop(chars[end]) {
        end += 1;
    }
    (end > after).then_some(end)
}

fn is_link_stop(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`' | '[' | ']' | ',' | ';')
}

fn take_path(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i) == Some(&'$') {
        if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            return None;
        }
        let end = starts_with_exact(chars, i, "${home}")
            .or_else(|| starts_with_exact(chars, i, "$home"))?;
        let mut stop = end;
        while stop < chars.len() && !is_url_stop(chars[stop]) {
            stop += 1;
        }
        return (stop > i).then_some(stop);
    }
    if chars.get(i) == Some(&'~') {
        if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            return None;
        }
        if chars.get(i + 1) == Some(&'/') {
            let mut end = i + 2;
            while end < chars.len() && !is_url_stop(chars[end]) {
                end += 1;
            }
            return Some(end);
        }
        let mut j = i + 1;
        if !chars.get(j).is_some_and(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        while chars
            .get(j)
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, '-' | '_' | '.'))
        {
            j += 1;
        }
        if chars.get(j) != Some(&'/') {
            return None;
        }
        let mut end = j + 1;
        while end < chars.len() && !is_url_stop(chars[end]) {
            end += 1;
        }
        return Some(end);
    }
    if (i == 0 || !chars[i - 1].is_ascii_alphanumeric())
        && chars.get(i).is_some_and(char::is_ascii_alphabetic)
        && chars.get(i + 1) == Some(&':')
        && chars.get(i + 2).is_some_and(|c| matches!(*c, '\\' | '/'))
    {
        let mut end = i + 3;
        while end < chars.len() && !is_url_stop(chars[end]) {
            end += 1;
        }
        return Some(end);
    }
    if chars.get(i) == Some(&'.')
        && chars.get(i + 1) == Some(&'.')
        && chars.get(i + 2).is_some_and(|c| matches!(*c, '/' | '\\'))
    {
        let mut end = i + 3;
        while end < chars.len() && !is_url_stop(chars[end]) {
            end += 1;
        }
        return Some(end);
    }
    if *chars.get(i)? != '/' {
        return None;
    }
    if i > 0 {
        let prev = chars[i - 1];
        if prev.is_alphanumeric() || matches!(prev, '_' | ':' | '/' | '.') {
            return None;
        }
    }
    let mut end = i + 1;
    while end < chars.len() && !is_url_stop(chars[end]) {
        end += 1;
    }
    let path: String = chars[i..end].iter().collect();
    if matches!(
        path.as_str(),
        "/dev/null" | "/dev/zero" | "/dev/stdin" | "/dev/stdout" | "/dev/stderr" | "/dev/tty"
    ) {
        return None;
    }
    if path.split('/').filter(|seg| !seg.is_empty()).count() >= 2 {
        Some(end)
    } else {
        None
    }
}

fn take_email(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '_' | '.')) {
        return None;
    }
    let local_start = starts_with_ignore_ascii_case(chars, i, "mailto:").unwrap_or(i);
    let first = *chars.get(local_start)?;
    if !first.is_alphanumeric() {
        return None;
    }
    let mut j = local_start + 1;
    while j < chars.len()
        && (chars[j].is_alphanumeric() || matches!(chars[j], '.' | '_' | '%' | '+' | '-'))
    {
        j += 1;
    }
    if *chars.get(j)? != '@' {
        return take_obfuscated_email(chars, local_start, j);
    }
    if !chars[j - 1].is_alphanumeric() && !matches!(chars[j - 1], '_' | '+') {
        return None;
    }
    let domain_start = j + 1;
    let domain_end = scan_domain(chars, domain_start)?;
    let domain: String = chars[domain_start..domain_end].iter().collect();
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return None;
    }
    let tld = labels.last()?;
    if !tld_ok(tld) {
        return None;
    }
    if domain_end < chars.len()
        && (chars[domain_end].is_alphanumeric() || matches!(chars[domain_end], '_' | '-'))
    {
        return None;
    }
    Some(domain_end)
}

fn tld_ok(tld: &str) -> bool {
    if tld.chars().count() < 2 {
        return false;
    }
    if tld.chars().all(char::is_alphabetic) {
        return true;
    }
    let bytes = tld.as_bytes();
    bytes.len() > 4
        && bytes.starts_with(b"xn--")
        && !tld.ends_with('-')
        && tld.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn scan_domain(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start;
    let mut label_len = 0usize;
    let mut last_char = '\0';
    while j < chars.len() {
        let c = chars[j];
        if c.is_alphanumeric() || (c == '-' && label_len > 0) {
            label_len += 1;
            last_char = c;
            j += 1;
        } else if c == '.' && label_len > 0 && last_char.is_alphanumeric() {
            if !chars.get(j + 1).is_some_and(|c| c.is_alphanumeric()) {
                break;
            }
            label_len = 0;
            last_char = c;
            j += 1;
        } else {
            break;
        }
    }
    if label_len == 0 || !last_char.is_alphanumeric() {
        return None;
    }
    Some(j)
}

fn take_private_host(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '_' | '-' | '.')) {
        return None;
    }
    let mut host_start = i;
    let mut has_scheme = false;
    for scheme in [
        "https://", "http://", "ssh://", "wss://", "ws://", "ftp://", "tcp://",
    ] {
        if let Some(after) = starts_with_ignore_ascii_case(chars, i, scheme) {
            host_start = after;
            has_scheme = true;
            break;
        }
    }
    if has_scheme {
        let mut scan = host_start;
        while scan < chars.len() && !is_url_stop(chars[scan]) && !matches!(chars[scan], '/' | ':') {
            if chars[scan] == '@' {
                host_start = scan + 1;
                break;
            }
            scan += 1;
        }
    }
    let host_end = scan_domain(chars, host_start)?;
    let host: String = chars[host_start..host_end]
        .iter()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    const PRIVATE_SUFFIXES: &[&str] = &[
        ".ts.net",
        ".tailscale.net",
        ".local",
        ".localdomain",
        ".internal",
        ".lan",
        ".home",
        ".home.arpa",
        ".arpa",
        ".corp",
        ".intranet",
        ".private",
    ];
    let is_private_suffix = PRIVATE_SUFFIXES.iter().any(|suf| host.ends_with(suf));
    let is_localhost_url = host == "localhost"
        && (has_scheme
            || (chars.get(host_end) == Some(&':')
                && chars.get(host_end + 1).is_some_and(char::is_ascii_digit)));
    if !is_private_suffix && !is_localhost_url {
        return None;
    }
    if host_end < chars.len()
        && (chars[host_end].is_alphanumeric() || matches!(chars[host_end], '_' | '-'))
    {
        return None;
    }
    let mut end = host_end;
    if chars.get(end) == Some(&':') && chars.get(end + 1).is_some_and(char::is_ascii_digit) {
        end += 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
    }
    if has_scheme && chars.get(end) == Some(&'/') {
        while end < chars.len() && !is_url_stop(chars[end]) {
            end += 1;
        }
    }
    Some(end)
}

fn take_ip(chars: &[char], i: usize) -> Option<usize> {
    if let Some(end) = take_bracketed_ipv6(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_hex_ip(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_ipv6(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_ipv4(chars, i) {
        return Some(end);
    }
    take_integer_ip(chars, i)
}

fn take_bracketed_ipv6(chars: &[char], i: usize) -> Option<usize> {
    if *chars.get(i)? != '[' {
        return None;
    }
    if i > 0 && chars[i - 1].is_alphanumeric() {
        return None;
    }
    let mut close = i + 1;
    while close < chars.len() && chars[close] != ']' {
        if chars[close].is_whitespace() || chars[close] == '[' {
            return None;
        }
        close += 1;
    }
    if close >= chars.len() || chars[close] != ']' {
        return None;
    }
    let inner: String = chars[i + 1..close].iter().collect();
    let addr_part = inner.split('%').next().unwrap_or("");
    if addr_part.parse::<Ipv6Addr>().is_err() {
        return None;
    }
    let mut end = close + 1;
    if chars.get(end) == Some(&':') && chars.get(end + 1).is_some_and(char::is_ascii_digit) {
        end += 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
    }
    Some(end)
}

fn take_ipv6(chars: &[char], i: usize) -> Option<usize> {
    let ch = *chars.get(i)?;
    if !ch.is_ascii_hexdigit() && ch != ':' {
        return None;
    }
    if i > 0 && (chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], ':' | '.')) {
        return None;
    }
    let mut end = i;
    while end < chars.len() && (chars[end].is_ascii_hexdigit() || matches!(chars[end], ':' | '.')) {
        end += 1;
    }
    while end > i && chars[end - 1] == '.' {
        end -= 1;
    }
    if end > i + 1 && chars[end - 1] == ':' && chars[end - 2] != ':' {
        end -= 1;
    }
    if end <= i {
        return None;
    }
    let candidate: String = chars[i..end].iter().collect();
    if candidate == "::" || candidate.len() < 3 {
        return None;
    }
    if !(candidate.chars().any(|c| c.is_ascii_digit())
        || candidate.matches(':').count() >= 3
        || candidate.split(':').any(|g| g.len() >= 3))
    {
        return None;
    }
    if candidate.parse::<Ipv6Addr>().is_err() {
        return None;
    }
    let groups: Vec<&str> = candidate
        .split(':')
        .filter(|group| !group.is_empty())
        .collect();
    if groups.len() < 2
        || !groups
            .iter()
            .any(|group| group.chars().any(|c| c.is_ascii_digit()))
    {
        return None;
    }
    if chars.get(end) == Some(&'%') && chars.get(end + 1).is_some_and(char::is_ascii_alphanumeric) {
        end += 1;
        while end < chars.len()
            && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '-' | '.'))
        {
            end += 1;
        }
    }
    if end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == ':') {
        return None;
    }
    Some(end)
}

fn take_ipv4(chars: &[char], i: usize) -> Option<usize> {
    if !chars.get(i)?.is_ascii_digit() {
        return None;
    }
    if i > 0 && (chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '.' | '_')) {
        return None;
    }
    let mut end = i;
    while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.') {
        end += 1;
    }
    while end > i && chars[end - 1] == '.' {
        end -= 1;
    }
    if end <= i {
        return None;
    }
    let candidate: String = chars[i..end].iter().collect();
    let address = parse_ipv4_form(&candidate)?;
    if chars.get(end) == Some(&'.')
        && chars
            .get(end + 1)
            .copied()
            .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    let has_port =
        chars.get(end) == Some(&':') && chars.get(end + 1).is_some_and(char::is_ascii_digit);
    let mut has_cidr = false;
    if has_port {
        end += 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
    } else if chars.get(end) == Some(&'/') && chars.get(end + 1).is_some_and(char::is_ascii_digit) {
        let mut cidr_end = end + 1;
        while cidr_end < chars.len() && chars[cidr_end].is_ascii_digit() {
            cidr_end += 1;
        }
        let bits: Option<u8> = chars[end + 1..cidr_end]
            .iter()
            .collect::<String>()
            .parse()
            .ok();
        if bits.is_some_and(|b| b <= 32)
            && !chars
                .get(cidr_end)
                .is_some_and(|c| c.is_alphanumeric() || *c == '/')
        {
            end = cidr_end;
            has_cidr = true;
        }
    }
    let octets = address.octets();
    let private = octets[0] == 10
        || octets[0] == 127
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 100 && (64..=127).contains(&octets[1]));
    // Two- and three-part forms such as 1.89, 3.12.3, 0.5 and 139.1 are
    // versions and decimals. A private short form such as 127.1 stays.
    if candidate.matches('.').count() < 3 && !private && !has_port && !has_cidr {
        return None;
    }
    if !private && !has_port && !has_cidr && has_version_context(chars, i) {
        return None;
    }
    if end < chars.len() && chars[end].is_alphanumeric() {
        return None;
    }
    Some(end)
}

fn parse_ipv4_form(candidate: &str) -> Option<Ipv4Addr> {
    if let Ok(address) = candidate.parse::<Ipv4Addr>() {
        return Some(address);
    }
    let parts: Vec<&str> = candidate
        .split('.')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let nums: Vec<u32> = parts
        .iter()
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    if nums.iter().any(|n| *n > 255) {
        return None;
    }
    let octets = match *nums.as_slice() {
        [a, b] => [a as u8, 0, 0, b as u8],
        [a, b, c] => [a as u8, b as u8, 0, c as u8],
        _ => return None,
    };
    Some(Ipv4Addr::from(octets))
}

fn take_hex_ip(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1].is_ascii_alphanumeric() {
        return None;
    }
    let start = starts_with_exact(chars, i, "0x")?;
    let mut end = start;
    while chars.get(end).is_some_and(|c| c.is_ascii_hexdigit()) {
        end += 1;
    }
    let digits = end - start;
    if !(6..=8).contains(&digits) {
        return None;
    }
    if chars.get(end).is_some_and(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let hex: String = chars[start..end].iter().collect();
    u32::from_str_radix(&hex, 16).ok()?;
    Some(end)
}

fn take_integer_ip(chars: &[char], i: usize) -> Option<usize> {
    if !chars.get(i)?.is_ascii_digit() {
        return None;
    }
    if i > 0 && chars[i - 1].is_ascii_alphanumeric() {
        return None;
    }
    let mut end = i;
    while chars.get(end).is_some_and(|c| c.is_ascii_digit()) {
        end += 1;
    }
    let digits = end - i;
    if !(8..=10).contains(&digits) {
        return None;
    }
    if chars
        .get(end)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '.')
    {
        return None;
    }
    let text: String = chars[i..end].iter().collect();
    let value: u32 = text.parse().ok()?;
    if value < 0x0100_0000 {
        return None;
    }
    Some(end)
}

fn has_version_context(chars: &[char], at: usize) -> bool {
    let start = at.saturating_sub(24);
    let before: String = chars[start..at].iter().collect();
    ["ships", "release", "firmware", "version", "section"]
        .iter()
        .any(|word| {
            before
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|part| part == *word)
        })
}

fn take_slack_hook(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_name_char(chars[i - 1]) {
        return None;
    }
    let mut at = i;
    if let Some(after) = starts_with_exact(chars, i, "https://") {
        at = after;
    } else if let Some(after) = starts_with_exact(chars, i, "http://") {
        at = after;
    }
    let after = starts_with_exact(chars, at, "hooks.slack.com/")?;
    if starts_with_exact(chars, after, "services/").is_none()
        && starts_with_exact(chars, after, "workflows/").is_none()
    {
        return None;
    }
    let mut end = after;
    while end < chars.len() && !is_url_stop(chars[end]) {
        end += 1;
    }
    Some(end)
}

fn take_curl_user(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && !chars[i - 1].is_whitespace() {
        return None;
    }
    let after = if let Some(end) = starts_with_exact(chars, i, "--user") {
        end
    } else if let Some(end) = starts_with_exact(chars, i, "-u") {
        end
    } else {
        return None;
    };
    if !chars.get(after).is_some_and(|c| c.is_whitespace()) {
        return None;
    }
    let mut j = after;
    while chars.get(j).is_some_and(|c| c.is_whitespace()) {
        j += 1;
    }
    let start = j;
    let mut colon = None;
    while j < chars.len() && !chars[j].is_whitespace() {
        if chars[j] == ':' && colon.is_none() {
            colon = Some(j);
        }
        j += 1;
    }
    let colon = colon?;
    if colon == start || colon + 1 == j {
        return None;
    }
    Some(j)
}

fn take_opaque(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (chars[i - 1].is_ascii_alphanumeric() || matches!(chars[i - 1], '+' | '/' | '=')) {
        return None;
    }
    if !chars.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let mut end = i;
    while chars
        .get(end)
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, '+' | '/' | '='))
    {
        end += 1;
    }
    let n = end - i;
    if n < 32 {
        return None;
    }
    let token: String = chars[i..end].iter().collect();
    let hex = token.chars().all(|c| c.is_ascii_hexdigit());
    if hex && (n == 40 || n == 64) {
        return None;
    }
    let distinct = token
        .chars()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if distinct < 8 || !token.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if chars
        .get(end)
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, '+' | '/' | '='))
    {
        return None;
    }
    Some(end)
}

fn take_obfuscated_email(chars: &[char], local_start: usize, local_end: usize) -> Option<usize> {
    if local_end <= local_start {
        return None;
    }
    let mut j = local_end;
    while chars.get(j).is_some_and(|c| c.is_whitespace()) {
        j += 1;
    }
    let domain_at = if let Some(after) = starts_with_exact(chars, j, "(at)") {
        after
    } else if let Some(after) = starts_with_exact(chars, j, "[at]") {
        after
    } else {
        return None;
    };
    let mut domain_start = domain_at;
    while chars.get(domain_start).is_some_and(|c| c.is_whitespace()) {
        domain_start += 1;
    }
    let domain_end = scan_domain(chars, domain_start)?;
    let domain: String = chars[domain_start..domain_end].iter().collect();
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 || !tld_ok(labels.last()?) {
        return None;
    }
    if domain_end < chars.len()
        && (chars[domain_end].is_alphanumeric() || matches!(chars[domain_end], '_' | '-'))
    {
        return None;
    }
    Some(domain_end)
}

fn take_marked(chars: &[char], i: usize, prefixes: &[&str]) -> Option<usize> {
    if i > 0 && is_word_char(chars[i - 1]) {
        return None;
    }
    let after = prefixes
        .iter()
        .find_map(|p| starts_with_exact(chars, i, p))?;
    let mut end = after;
    while end < chars.len()
        && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '-'))
    {
        end += 1;
    }
    if end - after < 4 {
        return None;
    }
    let prefix: String = chars[i..after].iter().collect();
    if prefix == "agent-" && !chars[after..end].iter().any(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(end)
}

fn take_known_seat_in_context(chars: &[char], i: usize, seats: &[String]) -> Option<usize> {
    if i > 0 && is_word_char(chars[i - 1]) {
        return None;
    }
    for name in seats {
        let len = name.chars().count();
        if len < 2 || i + len > chars.len() {
            continue;
        }
        let matches = chars[i..i + len]
            .iter()
            .zip(name.chars())
            .all(|(&a, b)| a.to_lowercase().eq(b.to_lowercase()));
        if !matches {
            continue;
        }
        let end = i + len;
        if (end == chars.len() || !is_word_char(chars[end])) && seat_context_before(chars, i) {
            return Some(end);
        }
    }
    None
}

fn seat_context_before(chars: &[char], at: usize) -> bool {
    let prefix: String = chars[..at].iter().collect::<String>().to_ascii_lowercase();
    let trimmed = prefix.trim_end_matches(|c: char| c.is_whitespace() || c == ':' || c == '=');
    let word = trimmed
        .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    matches!(
        word,
        "seat"
            | "by"
            | "from"
            | "as"
            | "author"
            | "reviewer"
            | "owner"
            | "ljos_seat"
            | "vissue_agent"
    )
}

#[cfg(test)]
mod tests {
    use super::scan_author;

    #[test]
    fn a_long_repeated_line_scans_in_linear_time() {
        for unit in ["a-", "a."] {
            let line = unit.repeat(40_000);
            assert_eq!(line.chars().count(), 80_000);
            let started = std::time::Instant::now();
            let hits = scan_author(&line, 1);
            let elapsed = started.elapsed();
            assert!(
                elapsed < std::time::Duration::from_secs(2),
                "{unit:?} took {elapsed:?}"
            );
            assert!(hits.is_empty(), "{unit:?}: {hits:?}");
        }
    }
}
