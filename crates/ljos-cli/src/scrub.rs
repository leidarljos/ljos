//! One scrub for text that a public page may carry.
//!
//! Workflow render and `render --check` both call [`scrub_public`].
//! A handbook page uses the same function when one is rendered here.

/// Remove local absolute paths, seat tokens, agent ids and `bc-` ids.
/// Drop any line that names a bot or a model, including a version suffix.
/// Drop `seat <name>` mentions and known seat names.
#[must_use]
pub fn scrub_public(text: &str) -> String {
    scrub_with(text, &live_seats())
}

fn scrub_with(text: &str, seats: &[String]) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(body) => (body, true),
            None => (line, false),
        };
        if names_model_or_bot(body) {
            continue;
        }
        let cleaned = scrub_tokens(body, seats);
        if cleaned.trim().is_empty() && !body.trim().is_empty() {
            continue;
        }
        out.push_str(&cleaned);
        if nl {
            out.push('\n');
        }
    }
    out
}

fn live_seats() -> Vec<String> {
    let mut names = Vec::new();
    for key in ["LJOS_SEAT", "VISSUE_AGENT"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                names.push(value.to_string());
            }
        }
    }
    names.extend(crate::tracker_known_seats());
    names.retain(|name| {
        name.chars().count() >= 2
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    });
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

fn names_model_or_bot(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    const WORDS: &[&str] = &[
        "ai",
        "bot",
        "bots",
        "chatbot",
        "chatbots",
        "llm",
        "gpt",
        "chatgpt",
        "openai",
        "claude",
        "anthropic",
        "grok",
        "gemini",
        "llama",
        "copilot",
        "opus",
        "sonnet",
        "composer",
    ];
    if WORDS.iter().any(|word| has_word(&lower, word)) {
        return true;
    }
    lower.contains("language model")
}

/// A family word matches on its own and with a version suffix (`gpt-4`, `llama-3`).
fn has_word(lower: &str, word: &str) -> bool {
    let bytes = lower.as_bytes();
    let needle = word.as_bytes();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(word) {
        let at = from + rel;
        let before_ok = at == 0 || !is_word(bytes[at - 1]);
        let after = at + needle.len();
        let after_ok = after == bytes.len() || model_boundary(bytes, after);
        if before_ok && after_ok {
            return true;
        }
        from = at + 1;
    }
    false
}

/// End of the token, or a hyphenated version such as `-4` or `-5.6`.
fn model_boundary(bytes: &[u8], after: usize) -> bool {
    if after >= bytes.len() || !is_word(bytes[after]) {
        return true;
    }
    bytes[after] == b'-' && bytes.get(after + 1).is_some_and(|b| b.is_ascii_digit())
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn scrub_tokens(line: &str, seats: &[String]) -> String {
    let mut out = String::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = take_path(line, i) {
            i = end;
            continue;
        }
        if let Some(end) = take_marked(line, i, &["bc-", "agent-"]) {
            i = end;
            continue;
        }
        if let Some(end) = take_assign(line, i, &["LJOS_SEAT=", "VISSUE_AGENT=", "seat="]) {
            i = end;
            continue;
        }
        if let Some(end) = take_seat_colon(line, i) {
            i = end;
            continue;
        }
        if let Some(end) = take_seat_mention(line, i) {
            i = end;
            continue;
        }
        if let Some(end) = take_known_seat(line, i, seats) {
            i = end;
            continue;
        }
        let ch = line[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn take_path(line: &str, i: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if bytes.get(i) != Some(&b'/') {
        return None;
    }
    if i > 0 {
        let prev = bytes[i - 1];
        if prev.is_ascii_alphanumeric() || prev == b':' || prev == b'_' || prev == b'/' {
            return None;
        }
    }
    let rest = &line[i + 1..];
    let rel = rest.find(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`' | ')' | '(' | ',' | ';')
    });
    let end = i + 1 + rel.unwrap_or(rest.len());
    let path = &line[i..end];
    if path.matches('/').count() >= 2 && path.len() > 2 {
        Some(end)
    } else {
        None
    }
}

fn take_marked(line: &str, i: usize, prefixes: &[&str]) -> Option<usize> {
    let bytes = line.as_bytes();
    if i > 0 && is_word(bytes[i - 1]) {
        return None;
    }
    let rest = &line[i..];
    let prefix = prefixes.iter().copied().find(|p| rest.starts_with(p))?;
    let tail = &rest[prefix.len()..];
    let n = tail
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        .count();
    if n < 4 {
        return None;
    }
    Some(i + prefix.len() + n)
}

fn take_assign(line: &str, i: usize, prefixes: &[&str]) -> Option<usize> {
    let rest = &line[i..];
    let prefix = prefixes.iter().copied().find(|p| {
        rest.starts_with(p)
            || rest
                .to_ascii_lowercase()
                .starts_with(&p.to_ascii_lowercase())
    })?;
    let tail = &rest[prefix.len()..];
    let n = tail
        .bytes()
        .take_while(|b| !b.is_ascii_whitespace() && *b != b',' && *b != b';')
        .count();
    if n == 0 {
        return None;
    }
    Some(i + prefix.len() + n)
}

fn take_seat_colon(line: &str, i: usize) -> Option<usize> {
    let rest = &line[i..];
    let lower = rest.to_ascii_lowercase();
    let prefix = if lower.starts_with("seat:") {
        "seat:"
    } else {
        return None;
    };
    let mut n = prefix.len();
    let bytes = rest.as_bytes();
    if bytes.get(n) == Some(&b' ') {
        n += 1;
    }
    let name = rest[n..]
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        .count();
    if name == 0 {
        return None;
    }
    Some(i + n + name)
}

/// `seat <name>`, with a space between the two words.
fn take_seat_mention(line: &str, i: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if i > 0 && is_word(bytes[i - 1]) {
        return None;
    }
    let rest = &line[i..];
    if rest.len() < 5 || !rest[..4].eq_ignore_ascii_case("seat") {
        return None;
    }
    if !rest.as_bytes()[4].is_ascii_whitespace() {
        return None;
    }
    let mut n = 4;
    while rest
        .as_bytes()
        .get(n)
        .is_some_and(|b| b.is_ascii_whitespace())
    {
        n += 1;
    }
    let start = rest.as_bytes().get(n).copied()?;
    if !start.is_ascii_alphabetic() {
        return None;
    }
    let name = rest[n..]
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        .count();
    if name == 0 {
        return None;
    }
    Some(i + n + name)
}

fn take_known_seat(line: &str, i: usize, seats: &[String]) -> Option<usize> {
    let bytes = line.as_bytes();
    if i > 0 && is_word(bytes[i - 1]) {
        return None;
    }
    let rest = &line[i..];
    for name in seats {
        if name.len() < 2 || rest.len() < name.len() {
            continue;
        }
        if !rest[..name.len()].eq_ignore_ascii_case(name) {
            continue;
        }
        let end = i + name.len();
        if end == line.len() || !is_word(bytes[end]) {
            return Some(end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{scrub_public, scrub_with};

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
    fn ordinary_words_stay() {
        let raw = "Both the tag and the changelog ship.\n";
        assert_eq!(scrub_public(raw), raw);
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
    }
}
