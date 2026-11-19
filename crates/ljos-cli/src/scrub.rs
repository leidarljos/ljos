//! One scrub for text that a public page may carry.
//!
//! Workflow render and `render --check` both call [`scrub_public`] and [`scan_public`].

use std::net::{Ipv4Addr, Ipv6Addr};

use icu_properties::{
    props::{DefaultIgnorableCodePoint, GeneralCategory},
    CodePointMapData, CodePointSetData,
};
use unicode_normalization::UnicodeNormalization as _;

/// Remove local paths, file URLs, secrets, IPs, private hosts, emails,
/// Org link brackets, seat tokens, agent ids, `bc-` ids and known seats.
/// Drop any line that names a model, vendor, bot, subagent or LLM.
#[must_use]
pub fn scrub_public(text: &str) -> String {
    scrub_with(text, &live_seats())
}

/// Return a human-readable hit description for every disallowed item found
/// in `text` without modifying it. An empty list means `text` is clean.
#[must_use]
pub fn scan_public(text: &str) -> Vec<String> {
    scan_with(text, &live_seats())
}

fn scrub_with(text: &str, seats: &[String]) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        let (raw_body, nl) = match line.strip_suffix('\n') {
            Some(body) => (body, true),
            None => (line, false),
        };
        let Some(cleaned) = scrub_line(raw_body, seats) else {
            continue;
        };
        out.push_str(&cleaned);
        if nl {
            out.push('\n');
        }
    }
    out
}

fn scan_with(text: &str, seats: &[String]) -> Vec<String> {
    let mut hits = Vec::new();
    for (idx, line) in text.split_inclusive('\n').enumerate() {
        let line_no = idx + 1;
        let body = line.strip_suffix('\n').unwrap_or(line);
        let normalized = normalize_line(body);
        if normalized != body {
            hits.push(format!(
                "line {line_no} contains invisible or non-normalized characters"
            ));
        }
        if names_model_or_bot(&normalized) {
            hits.push(format!("line {line_no} names a model, vendor or tool"));
        }
        let chars: Vec<char> = normalized.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if let Some((end, kind)) = match_token(&chars, i, seats) {
                hits.push(format!("line {line_no} contains {}", kind.label()));
                i = end;
            } else {
                i += 1;
            }
        }
    }
    hits
}

fn scrub_line(raw_body: &str, seats: &[String]) -> Option<String> {
    let was_non_empty = !raw_body.trim().is_empty();
    let mut current = normalize_line(raw_body);
    for _ in 0..16 {
        if names_model_or_bot(&current) {
            return None;
        }
        let cleaned = scrub_tokens(&current, seats);
        if was_non_empty && cleaned.trim().is_empty() {
            return None;
        }
        let next = normalize_line(&cleaned);
        if next == current {
            return Some(next);
        }
        current = next;
    }
    if names_model_or_bot(&current) || (was_non_empty && current.trim().is_empty()) {
        None
    } else {
        Some(current)
    }
}

fn is_invisible(c: char) -> bool {
    let ignorable = CodePointSetData::new::<DefaultIgnorableCodePoint>();
    let gc = CodePointMapData::<GeneralCategory>::new();
    ignorable.contains(c) || gc.get(c) == GeneralCategory::Format || c == '\u{180E}'
}

fn normalize_line(line: &str) -> String {
    let visible: String = line.chars().filter(|&c| !is_invisible(c)).collect();
    visible.nfkc().filter(|&c| !is_invisible(c)).collect()
}

fn live_seats() -> Vec<String> {
    let mut names = Vec::new();
    for key in ["LJOS_SEAT", "VISSUE_AGENT"] {
        if let Ok(value) = std::env::var(key) {
            let value = normalize_line(value.trim());
            if !value.is_empty() {
                names.push(value);
            }
        }
    }
    for seat in crate::tracker_known_seats() {
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
        'а' | 'α' | 'ɑ' => 'a',
        'ь' => 'b',
        'с' | 'ϲ' | 'ᴄ' => 'c',
        'ԁ' | 'ɗ' => 'd',
        'е' | 'є' | 'ε' | 'ᴇ' => 'e',
        'ғ' => 'f',
        'ɡ' => 'g',
        'һ' | 'н' => 'h',
        'і' | 'ї' | 'ι' | 'ɪ' => 'i',
        'ј' | 'ʝ' => 'j',
        'к' | 'κ' => 'k',
        'ӏ' | 'ⅼ' => 'l',
        'м' | 'ᴍ' => 'm',
        'ո' | 'п' | 'ν' => 'n',
        'о' | 'ο' | 'σ' | 'ᴏ' => 'o',
        'р' | 'ρ' => 'p',
        'ԛ' => 'q',
        'г' | 'ʀ' => 'r',
        'ѕ' | 'ꜱ' => 's',
        'т' | 'τ' | 'ᴛ' => 't',
        'υ' | 'ս' => 'u',
        'ѵ' => 'v',
        'ԝ' | 'ω' => 'w',
        'х' | 'χ' => 'x',
        'у' | 'ү' | 'γ' => 'y',
        'ᴢ' => 'z',
        other => other,
    }
}

fn names_model_or_bot(line: &str) -> bool {
    let folded: Vec<char> = normalize_line(line)
        .chars()
        .flat_map(char::to_lowercase)
        .map(fold_lookalike)
        .collect();

    const MODEL_OR_VENDOR_STEMS: &[&str] = &[
        "gpt",
        "chatgpt",
        "claude",
        "anthropic",
        "openai",
        "gemini",
        "grok",
        "copilot",
        "llama",
        "mistral",
        "codex",
        "composer",
        "cursor",
        "opus",
        "sonnet",
        "llm",
        "chatbot",
        "subagent",
    ];
    for stem in MODEL_OR_VENDOR_STEMS {
        let needle: Vec<char> = stem.chars().collect();
        if has_stem(&folded, &needle, true) {
            return true;
        }
    }

    for stem in ["ai", "bot", "bots", "aiagent", "aiagents"] {
        let needle: Vec<char> = stem.chars().collect();
        if has_stem(&folded, &needle, false) {
            return true;
        }
    }

    has_joined_pair(&folded, "sub", "agent", true)
        || has_joined_pair(&folded, "chat", "bot", true)
        || has_joined_pair(&folded, "ai", "agent", true)
        || has_joined_pair(&folded, "language", "model", true)
}

fn has_stem(chars: &[char], needle: &[char], allow_plural_s: bool) -> bool {
    if needle.is_empty() || chars.len() < needle.len() {
        return false;
    }
    for i in 0..=chars.len() - needle.len() {
        if chars[i..i + needle.len()] != *needle {
            continue;
        }
        let before_ok = i == 0 || !chars[i - 1].is_alphanumeric();
        if !before_ok {
            continue;
        }
        let after = i + needle.len();
        if stem_after_ok(chars, after, allow_plural_s) {
            return true;
        }
    }
    false
}

fn stem_after_ok(chars: &[char], after: usize, allow_plural_s: bool) -> bool {
    if after >= chars.len() {
        return true;
    }
    let next = chars[after];
    if !next.is_alphanumeric() || next.is_ascii_digit() {
        return true;
    }
    allow_plural_s
        && next == 's'
        && (after + 1 >= chars.len() || !chars[after + 1].is_alphanumeric())
}

fn has_joined_pair(chars: &[char], first: &str, second: &str, allow_plural_s: bool) -> bool {
    let a: Vec<char> = first.chars().collect();
    let b: Vec<char> = second.chars().collect();
    if chars.len() < a.len() + 1 + b.len() {
        return false;
    }
    for i in 0..=chars.len() - a.len() {
        if chars[i..i + a.len()] != a[..] {
            continue;
        }
        if i > 0 && chars[i - 1].is_alphanumeric() {
            continue;
        }
        let mut j = i + a.len();
        let mut saw_sep = false;
        while j < chars.len() && (chars[j].is_whitespace() || chars[j] == '-' || chars[j] == '_') {
            saw_sep = true;
            j += 1;
        }
        if !saw_sep || j + b.len() > chars.len() || chars[j..j + b.len()] != b[..] {
            continue;
        }
        let after = j + b.len();
        if stem_after_ok(chars, after, allow_plural_s) {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitKind {
    OrgLinkBracket,
    OrgFootnote,
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
            Self::OrgLinkBracket => "Org link brackets",
            Self::OrgFootnote => "an Org footnote reference",
            Self::Secret => "a secret or credential",
            Self::FileUrl => "a file URL",
            Self::LocalPath => "a local path",
            Self::Email => "an email address",
            Self::PrivateHost => "a private hostname",
            Self::IpAddress => "an IP address",
            Self::MarkedId => "an agent or run id",
            Self::Seat => "a seat token or seat name",
        }
    }
}

fn scrub_tokens(line: &str, seats: &[String]) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some((end, _)) = match_token(&chars, i, seats) {
            if let (Some(prev), Some(&next)) = (out.chars().next_back(), chars.get(end)) {
                if prev.is_alphanumeric() && next.is_alphanumeric() {
                    out.push(' ');
                }
            }
            i = end;
            if out.ends_with(' ') && chars.get(i) == Some(&' ') {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn match_token(chars: &[char], i: usize, seats: &[String]) -> Option<(usize, HitKind)> {
    if let Some(end) = take_org_brackets(chars, i) {
        return Some((end, HitKind::OrgLinkBracket));
    }
    if let Some(end) = take_org_footnote(chars, i) {
        return Some((end, HitKind::OrgFootnote));
    }
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
    if let Some(end) = take_assign(chars, i, &["ljos_seat=", "vissue_agent=", "seat="]) {
        return Some((end, HitKind::Seat));
    }
    if let Some(end) = take_seat_colon(chars, i) {
        return Some((end, HitKind::Seat));
    }
    if let Some(end) = take_seat_mention(chars, i) {
        return Some((end, HitKind::Seat));
    }
    if let Some(end) = take_known_seat(chars, i, seats) {
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

fn take_org_brackets(chars: &[char], i: usize) -> Option<usize> {
    let ch = *chars.get(i)?;
    if (ch == '[' || ch == ']') && chars.get(i + 1) == Some(&ch) {
        let mut end = i + 2;
        while chars.get(end) == Some(&ch) {
            end += 1;
        }
        return Some(end);
    }
    None
}

fn take_org_footnote(chars: &[char], i: usize) -> Option<usize> {
    let after = starts_with_ignore_ascii_case(chars, i, "[fn:")?;
    let mut end = after;
    while end < chars.len() {
        if chars[end] == ']' {
            return Some(end + 1);
        }
        if chars[end].is_whitespace() || chars[end] == '[' {
            return None;
        }
        end += 1;
    }
    None
}

fn take_secret(chars: &[char], i: usize) -> Option<usize> {
    if let Some(end) = take_secret_assign(chars, i) {
        return Some(end);
    }
    if let Some(end) = take_bearer(chars, i) {
        return Some(end);
    }
    take_secret_prefix(chars, i)
}

fn take_secret_assign(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_ident_char(chars[i - 1]) {
        return None;
    }
    let mut j = i;
    let mut has_alpha = false;
    while j < chars.len() && is_ident_char(chars[j]) {
        if chars[j].is_ascii_alphabetic() {
            has_alpha = true;
        }
        j += 1;
    }
    if !has_alpha || j >= chars.len() {
        return None;
    }
    let sep = chars[j];
    if sep != '=' && sep != ':' {
        return None;
    }
    let ident: String = chars[i..j].iter().map(|c| c.to_ascii_lowercase()).collect();
    let eq_match = matches!(
        ident.as_str(),
        "key"
            | "token"
            | "secret"
            | "password"
            | "passwd"
            | "passphrase"
            | "apikey"
            | "credential"
            | "credentials"
    ) || ident.ends_with("_key")
        || ident.ends_with("_token")
        || ident.ends_with("_secret")
        || ident.ends_with("_password")
        || ident.ends_with("_passwd")
        || ident.ends_with("_passphrase")
        || ident.ends_with("_credential")
        || ident.ends_with("_credentials")
        || ident.ends_with("_auth");
    let colon_match = matches!(
        ident.as_str(),
        "password"
            | "passwd"
            | "passphrase"
            | "secret"
            | "token"
            | "api_key"
            | "apikey"
            | "access_token"
            | "auth_token"
            | "private_key"
            | "signing_key"
            | "client_secret"
    ) || ident.ends_with("_key")
        || ident.ends_with("_token")
        || ident.ends_with("_secret")
        || ident.ends_with("_password")
        || ident.ends_with("_passwd");
    if (sep == '=' && !eq_match) || (sep == ':' && !colon_match) {
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
    Some(end)
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
    if let Some(after) = starts_with_exact(chars, i, concat!("AK", "IA"))
        .or_else(|| starts_with_exact(chars, i, "ASIA"))
    {
        let mut end = after;
        while end < chars.len() && chars[end].is_ascii_alphanumeric() {
            end += 1;
        }
        if end - after >= 4 {
            return Some(end);
        }
    }
    if let Some(after) = starts_with_exact(chars, i, concat!("glp", "at-")) {
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
    if let Some(after) = starts_with_exact(chars, i, "npm_") {
        let mut end = after;
        while end < chars.len() && chars[end].is_ascii_alphanumeric() {
            end += 1;
        }
        if end - after >= 4 {
            return Some(end);
        }
    }
    if let Some(after) = starts_with_exact(chars, i, "AIza") {
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
    let after = starts_with_ignore_ascii_case(chars, i, "file:")
        .or_else(|| starts_with_ignore_ascii_case(chars, i, "file+sys:"))
        .or_else(|| starts_with_ignore_ascii_case(chars, i, "file+emacs:"))?;
    if chars.get(after) != Some(&'/') {
        return None;
    }
    let mut end = after;
    while end < chars.len() && !is_url_stop(chars[end]) {
        end += 1;
    }
    if end > after + 1 {
        Some(end)
    } else {
        None
    }
}

fn take_path(chars: &[char], i: usize) -> Option<usize> {
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
    if !first.is_ascii_alphanumeric() {
        return None;
    }
    let mut j = local_start + 1;
    while j < chars.len()
        && (chars[j].is_ascii_alphanumeric() || matches!(chars[j], '.' | '_' | '%' | '+' | '-'))
    {
        j += 1;
    }
    if *chars.get(j)? != '@' {
        return None;
    }
    if !chars[j - 1].is_ascii_alphanumeric() && !matches!(chars[j - 1], '_' | '+') {
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
    if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if domain_end < chars.len()
        && (chars[domain_end].is_alphanumeric() || matches!(chars[domain_end], '_' | '-'))
    {
        return None;
    }
    Some(domain_end)
}

fn scan_domain(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start;
    let mut label_len = 0usize;
    let mut last_char = '\0';
    while j < chars.len() {
        let c = chars[j];
        if c.is_ascii_alphanumeric() || (c == '-' && label_len > 0) {
            label_len += 1;
            last_char = c;
            j += 1;
        } else if c == '.' && label_len > 0 && last_char.is_ascii_alphanumeric() {
            if !chars.get(j + 1).is_some_and(char::is_ascii_alphanumeric) {
                break;
            }
            label_len = 0;
            last_char = c;
            j += 1;
        } else {
            break;
        }
    }
    if label_len == 0 || !last_char.is_ascii_alphanumeric() {
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
    if let Some(end) = take_ipv6(chars, i) {
        return Some(end);
    }
    take_ipv4(chars, i)
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
    if candidate.parse::<Ipv4Addr>().is_err() {
        return None;
    }
    if chars.get(end) == Some(&'.')
        && chars
            .get(end + 1)
            .copied()
            .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    if chars.get(end) == Some(&':') && chars.get(end + 1).is_some_and(char::is_ascii_digit) {
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
        }
    }
    if end < chars.len() && chars[end].is_alphanumeric() {
        return None;
    }
    Some(end)
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
    Some(end)
}

fn take_assign(chars: &[char], i: usize, prefixes: &[&str]) -> Option<usize> {
    if i > 0 && is_ident_char(chars[i - 1]) {
        return None;
    }
    let after = prefixes
        .iter()
        .find_map(|p| starts_with_ignore_ascii_case(chars, i, p))?;
    let mut end = after;
    while end < chars.len() && !chars[end].is_whitespace() && !matches!(chars[end], ',' | ';') {
        end += 1;
    }
    if end == after {
        return None;
    }
    Some(end)
}

fn take_seat_colon(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_ident_char(chars[i - 1]) {
        return None;
    }
    let mut end = starts_with_ignore_ascii_case(chars, i, "seat:")?;
    if chars.get(end) == Some(&' ') {
        end += 1;
    }
    let start = end;
    while end < chars.len()
        && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '-'))
    {
        end += 1;
    }
    if end == start {
        return None;
    }
    Some(end)
}

/// `seat <name>`, with whitespace between the two words.
fn take_seat_mention(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_word_char(chars[i - 1]) {
        return None;
    }
    let mut n = starts_with_ignore_ascii_case(chars, i, "seat")?;
    let ws_start = n;
    while n < chars.len() && chars[n].is_whitespace() {
        n += 1;
    }
    if n == ws_start {
        return None;
    }
    let start = *chars.get(n)?;
    if !start.is_ascii_alphabetic() {
        return None;
    }
    let mut end = n;
    while end < chars.len()
        && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '-'))
    {
        end += 1;
    }
    if end == n {
        return None;
    }
    Some(end)
}

fn take_known_seat(chars: &[char], i: usize, seats: &[String]) -> Option<usize> {
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
        if end == chars.len() || !is_word_char(chars[end]) {
            return Some(end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{scan_public, scan_with, scrub_public, scrub_with};

    #[test]
    fn paths_ids_and_model_lines_leave_the_page() {
        let raw = "\
#+title: Release
Write notes in /home/alice/src/app today.
The run is bc-abcdef12 and agent-zzzz9999.
Reviewed by Claude before the tag.
seat:carol packed LJOS_SEAT=carol into the note.
The changelog is the public record.
";
        let once = scrub_public(raw);
        let twice = scrub_public(&once);
        assert_eq!(once, twice, "a second pass changes the page:\n{once}");
        assert!(scan_public(&once).is_empty(), "{once}");
        assert!(once.contains("#+title: Release"), "{once}");
        assert!(
            once.contains("The changelog is the public record."),
            "{once}"
        );
        assert!(!once.contains("/home/alice"), "{once}");
        assert!(!once.contains("bc-abcdef12"), "{once}");
        assert!(!once.contains("agent-zzzz9999"), "{once}");
        assert!(!once.to_ascii_lowercase().contains("claude"), "{once}");
        assert!(!once.contains("carol"), "{once}");
        assert!(!once.contains("LJOS_SEAT"), "{once}");
    }

    #[test]
    fn ordinary_words_and_dotted_text_stay() {
        let raw = "\
Both the tag and the changelog ship.
Update Cargo.toml to v1.2.3, e.g. the crate metadata, i.e. the version.
Use arr[0][1] and see section 3.
";
        assert_eq!(scrub_public(raw), raw);
        assert!(scan_public(raw).is_empty(), "{:?}", scan_public(raw));
    }

    #[test]
    fn versioned_model_names_and_seat_mentions_leave() {
        let raw = "\
Reviewed by GPT-4 and seat saddle.
Claude 3.5 wrote the note.
Gemini 2 checked the page.
gpt-5.6 drafted a line.
Llama-3 left a comment.
Ask seat saddle today.
See section 3.
The changelog is the public record.
";
        let once = scrub_public(raw);
        let twice = scrub_public(&once);
        assert_eq!(once, twice, "a second pass changes the page:\n{once}");
        assert!(scan_public(&once).is_empty(), "{once}");
        assert!(
            once.contains("The changelog is the public record."),
            "{once}"
        );
        assert!(once.contains("See section 3."), "{once}");
        let lower = once.to_ascii_lowercase();
        assert!(!lower.contains("gpt-4"), "{once}");
        assert!(!lower.contains("gpt-5.6"), "{once}");
        assert!(!lower.contains("claude"), "{once}");
        assert!(!lower.contains("gemini"), "{once}");
        assert!(!lower.contains("llama"), "{once}");
        assert!(!lower.contains("saddle"), "{once}");
        assert!(!lower.contains("seat saddle"), "{once}");

        let seats = vec!["saddle".to_string()];
        let named = scrub_with("Packed by saddle after review.\n", &seats);
        assert!(!named.to_ascii_lowercase().contains("saddle"), "{named}");
        assert!(named.contains("Packed by"), "{named}");
        assert!(named.contains("after review."), "{named}");
        assert!(scan_with(&named, &seats).is_empty(), "{named}");
    }

    #[test]
    fn unicode_panics_and_obfuscated_models_are_scrubbed() {
        let raw = "\
Cl\u{200b}aude reviewed this.
\u{ff27}\u{ff30}\u{ff34} drafted this.
\u{ff27}\u{ff30}\u{ff34}\u{ff14} checked this.
C\u{00ad}l\u{200d}aude\u{feff}3 helped.
seat s\u{200b}addle checked it.
\u{ff30}acked cleanly with \u{ff56}\u{ff11}.\u{ff12}.\u{ff13}.
";
        let once = scrub_public(raw);
        let twice = scrub_public(&once);
        assert_eq!(once, twice, "a second pass changed:\n{once}");
        assert!(scan_public(&once).is_empty(), "{:?}", scan_public(&once));
        assert!(!once.to_lowercase().contains("claude"), "{once}");
        assert!(!once.to_lowercase().contains("gpt"), "{once}");
        assert!(!once.to_lowercase().contains("saddle"), "{once}");
        assert!(once.contains("Packed cleanly with v1.2.3."), "{once}");
    }

    #[test]
    fn all_model_vendor_and_agent_classes_are_dropped() {
        let lines = [
            "GPT4 wrote the release note.",
            "Claude3 checked the tag.",
            "ChatGPT drafted a line.",
            "Anthropic policy applies.",
            "OpenAI endpoint note.",
            "Gemini-2.5 verified it.",
            "Grok-4.7 reviewed it.",
            "Copilot suggested a patch.",
            "Llama3 ran locally.",
            "Mistral-7B summarized.",
            "Codex ran the build.",
            "Composer-2.5 edited the file.",
            "Cursor ran a cloud job.",
            "Spawn a subagent for this.",
            "Ask the sub-agent to verify.",
            "A chatbot answered.",
            "The LLM checked the diff.",
            "An AI agent filed it.",
            "An AI-agent noted it.",
        ];
        for line in lines {
            let scrubbed = scrub_public(&format!("{line}\nKeep me.\n"));
            assert_eq!(scrubbed, "Keep me.\n", "leaked line: {line:?}");
            assert!(
                !scan_public(line).is_empty(),
                "scan_public missed: {line:?}"
            );
        }
    }

    #[test]
    fn secrets_paths_urls_ips_hosts_emails_and_links_are_scrubbed() {
        let cases = [
            ("file:///home/x", "file:///home/x"),
            ("file:/etc/passwd", "file:/etc/passwd"),
            ("/home/x/repo", "/home/x"),
            ("GITHUB_TOKEN=zzz", "zzz"),
            ("API_KEY=secret123", "secret123"),
            ("KEY=abc123", "abc123"),
            ("TOKEN=tok999", "tok999"),
            ("password: x", "password"),
            (concat!("x", "ox", "b-12345-abcde"), concat!("x", "oxb-")),
            (concat!("x", "ox", "a-12345-abcde"), concat!("x", "oxa-")),
            (concat!("x", "ox", "p-12345-abcde"), concat!("x", "oxp-")),
            (concat!("gh", "p_1234567890abcdef"), concat!("gh", "p_")),
            (concat!("gh", "o_1234567890abcdef"), concat!("gh", "o_")),
            (concat!("gh", "u_1234567890abcdef"), "ghu_"),
            (concat!("gh", "s_1234567890abcdef"), "ghs_"),
            (concat!("gh", "r_1234567890abcdef"), "ghr_"),
            (
                concat!("github", "_pat_11ABCDEF_xyz"),
                concat!("githu", "b_pat_"),
            ),
            (concat!("sk", "-proj-abc123456"), concat!("s", "k-proj")),
            (concat!("AKI", "AIOSFODNN7EXAMPLE"), concat!("AK", "IA")),
            ("10.0.0.1", "10.0.0.1"),
            ("192.168.1.42:8080", "192.168.1.42"),
            ("fe80::1", "fe80::1"),
            ("2001:db8::1", "2001:db8::1"),
            ("[::1]:8761", "::1"),
            ("box.tail1234.ts.net", "ts.net"),
            ("db.internal", "db.internal"),
            ("host.local", "host.local"),
            ("alice@example.com", "alice@example.com"),
            ("[[[file:/etc/passwd][x]]]", "[["),
            ("[[https://example.com][click]]", "[["),
        ];
        for (input, needle) in cases {
            let line = format!("Prefix {input} suffix.\n");
            let once = scrub_public(&line);
            let twice = scrub_public(&once);
            assert_eq!(once, twice, "not idempotent for {input:?}:\n{once}");
            assert!(
                !once.contains(needle),
                "scrub_public leaked {needle:?} in {input:?} -> {once:?}"
            );
            assert!(
                !once.contains("]]"),
                "scrub_public leaked ]] in {input:?} -> {once:?}"
            );
            assert!(
                !scan_public(input).is_empty(),
                "scan_public missed {input:?}"
            );
            assert!(
                scan_public(&once).is_empty(),
                "scan_public flagged scrubbed output for {input:?}: {once:?} -> {:?}",
                scan_public(&once)
            );
        }
    }

    #[test]
    fn nested_smuggling_is_caught_by_fixed_point() {
        let raw = "Cl[[file:/etc/passwd]]aude wrote this.\n";
        let scrubbed = scrub_public(raw);
        assert!(!scrubbed.to_lowercase().contains("claude"), "{scrubbed}");
        assert!(!scrubbed.contains("file:"), "{scrubbed}");
        assert!(!scrubbed.contains("[["), "{scrubbed}");
        assert!(scan_public(&scrubbed).is_empty(), "{scrubbed}");
    }
}
