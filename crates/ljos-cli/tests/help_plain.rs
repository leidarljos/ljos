//! Every subcommand's `--help` is plain enough for a newcomer.
//!
//! The one-line description has no unexplained shop-talk. A later use of
//! one of those words is allowed once the same page defines it in a few
//! words, or when the word is the name of an `ljos` command. Each page
//! shows one example.

ljos_test_boot::harness!();

use std::process::Command;

/// Words a newcomer does not know. A help page may use one only in the
/// sentence that defines it, or as the name of an `ljos` command.
const UNEXPLAINED: &[&str] = &["pack", "writer", "island", "seed", "deed", "sitting"];

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env("LJOS_OFF", "1")
        .env("LJOS_HOOKD", "off")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "ljos {}\n{text}{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

fn subcommands(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if line.is_empty() || (!line.starts_with(' ') && line.ends_with(':')) {
            break;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        if rest.starts_with(' ') || rest.starts_with('-') {
            continue;
        }
        let Some(name) = rest.split_whitespace().next() else {
            continue;
        };
        if name == "help" {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

fn root_names(help: &str) -> Vec<String> {
    help.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            if rest.starts_with('-') || rest.starts_with(' ') {
                return None;
            }
            rest.split_whitespace().next().map(str::to_string)
        })
        .collect()
}

fn is_term(token: &str, word: &str) -> bool {
    token.eq_ignore_ascii_case(word) || token.eq_ignore_ascii_case(&format!("{word}s"))
}

/// Alphabetic tokens, with the byte range of each. A token glued to a
/// hyphen, as in `already-in-pack` or `packset`, is not a separate word.
fn tokens(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let prev_hyphen = start > 0 && bytes[start - 1] == b'-';
            let next_hyphen = i < bytes.len() && bytes[i] == b'-';
            if !prev_hyphen && !next_hyphen {
                out.push((start, i, text[start..i].to_string()));
            }
        } else {
            i += 1;
        }
    }
    out
}

fn line_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (i, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            spans.push((start, i));
            start = i + 1;
        }
    }
    if start <= text.len() {
        spans.push((start, text.len()));
    }
    spans
}

/// `ljos island` and `Usage: ljos sitting` name the command. The word
/// there is not shop-talk.
fn command_mentions(text: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    for (start, end) in line_spans(text) {
        let line = &text[start..end];
        let local = tokens(line);
        let mut i = 0;
        while i < local.len() {
            if local[i].2 == "ljos" {
                i += 1;
                while i < local.len() && local[i].2.bytes().all(|b| b.is_ascii_lowercase()) {
                    let (a, b, word) = &local[i];
                    if UNEXPLAINED.iter().any(|term| is_term(word, term)) {
                        hits.push((start + a, start + b));
                    }
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }
    hits
}

fn sentences(text: &str) -> Vec<String> {
    let flat: String = text
        .chars()
        .map(|ch| if ch == '\n' { ' ' } else { ch })
        .collect();
    let chars: Vec<char> = flat.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, ch) in chars.iter().enumerate() {
        cur.push(*ch);
        if matches!(ch, '.' | '!' | '?') {
            let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
            let next_digit = chars.get(i + 1).is_some_and(|next| next.is_ascii_digit());
            if prev_digit && next_digit {
                continue;
            }
            let boundary = chars.get(i + 1).is_none_or(|next| next.is_whitespace());
            if boundary {
                let sentence = cur.trim().to_string();
                if !sentence.is_empty() {
                    out.push(sentence);
                }
                cur.clear();
            }
        }
    }
    let tail = cur.trim().to_string();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn word_bare(word: &str) -> String {
    word.chars().filter(|ch| ch.is_ascii_alphabetic()).collect()
}

/// `The pack is this seat's memory store.` A definition is one short sentence.
fn is_definition(sentence: &str, word: &str) -> bool {
    let parts: Vec<String> = sentence.split_whitespace().map(word_bare).collect();
    let parts: Vec<&str> = parts
        .iter()
        .filter(|part| !part.is_empty())
        .map(String::as_str)
        .collect();
    if parts.len() < 4 || parts.len() > 16 {
        return false;
    }
    if !parts[0].eq_ignore_ascii_case("a") && !parts[0].eq_ignore_ascii_case("the") {
        return false;
    }
    if !is_term(parts[1], word) {
        return false;
    }
    parts.iter().any(|part| part.eq_ignore_ascii_case("is"))
}

fn definitions(text: &str, word: &str) -> Vec<String> {
    sentences(text)
        .into_iter()
        .filter(|sentence| is_definition(sentence, word))
        .collect()
}

fn first_paragraph(text: &str) -> &str {
    text.split_once("\n\n")
        .map(|(head, _)| head)
        .unwrap_or(text)
}

fn mentioned(span: (usize, usize), mentions: &[(usize, usize)]) -> bool {
    mentions.contains(&span)
}

fn unexplained(help: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let about = first_paragraph(help);
    let mentions = command_mentions(help);
    for word in UNEXPLAINED {
        let about_hits: Vec<_> = tokens(about)
            .into_iter()
            .filter(|(_, _, token)| is_term(token, word))
            .collect();
        if !about_hits.is_empty() {
            problems.push(format!(
                "the one-line description uses `{word}` before it is defined"
            ));
        }
        let defs = definitions(help, word);
        if defs.len() > 1 {
            problems.push(format!("`{word}` is defined {} times", defs.len()));
        }
        let body_hits: Vec<_> = tokens(help)
            .into_iter()
            .filter(|(_, _, token)| is_term(token, word))
            .filter(|(start, end, _)| !mentioned((*start, *end), &mentions))
            .collect();
        if body_hits.is_empty() {
            continue;
        }
        if defs.len() != 1 {
            let sample = body_hits
                .first()
                .map(|(start, end, _)| &help[*start..*end])
                .unwrap_or(word);
            problems.push(format!(
                "`{word}` is used ({sample}) with no one-sentence definition"
            ));
        }
    }
    problems
}

fn example_problem(help: &str) -> Option<String> {
    let mut lines = help.lines();
    while let Some(line) = lines.next() {
        if !line.contains("Example:") {
            continue;
        }
        let mut window = line.to_string();
        for follow in lines.by_ref() {
            if follow.trim().is_empty() {
                break;
            }
            window.push(' ');
            window.push_str(follow.trim());
            if window.contains("ljos") {
                break;
            }
        }
        if window.contains("ljos") {
            return None;
        }
        return Some(format!("the example names no ljos command: {window}"));
    }
    Some("no Example: line".to_string())
}

fn is_flag_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    indent <= 6 && (trimmed.starts_with("--") || trimmed.starts_with('-'))
}

fn is_arg_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    indent <= 6 && (trimmed.starts_with('<') || trimmed.starts_with('['))
}

fn description_of(lines: &[&str], at: usize) -> String {
    let header = lines[at];
    let indent = header.len() - header.trim_start().len();
    let mut text = String::new();
    for line in lines.iter().skip(at + 1) {
        if line.is_empty() {
            if !text.is_empty() {
                break;
            }
            continue;
        }
        let inner = line.len() - line.trim_start().len();
        if inner <= indent {
            break;
        }
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(trimmed);
    }
    text
}

fn empty_flags(help: &str) -> Vec<String> {
    let lines: Vec<&str> = help.lines().collect();
    let mut section = "";
    let mut empty = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if *line == "Options:" || *line == "Arguments:" {
            section = line;
            continue;
        }
        if section.is_empty() {
            continue;
        }
        if !line.is_empty() && !line.starts_with(' ') {
            section = "";
            continue;
        }
        let header = (section == "Options:" && is_flag_line(line))
            || (section == "Arguments:" && is_arg_line(line));
        if !header {
            continue;
        }
        if line.contains("--help") || line.contains("--version") {
            continue;
        }
        let said = description_of(&lines, i);
        if said.split_whitespace().count() < 2 {
            empty.push(format!("{}: {said}", line.trim()));
        }
    }
    empty
}

fn walk(prefix: &[String], problems: &mut Vec<String>) {
    let mut args: Vec<&str> = prefix.iter().map(String::as_str).collect();
    args.push("--help");
    let help = run(&args);
    let path = prefix.join(" ");
    if let Some(err) = example_problem(&help) {
        problems.push(format!("ljos {path}: {err}"));
    }
    for err in unexplained(&help) {
        problems.push(format!("ljos {path}: {err}"));
    }
    for flag in empty_flags(&help) {
        problems.push(format!(
            "ljos {path}: flag help does not say what changes: {flag}"
        ));
    }
    for name in subcommands(&help) {
        let mut next = prefix.to_vec();
        next.push(name);
        walk(&next, problems);
    }
}

#[test]
fn every_subcommand_help_has_an_example_and_no_unexplained_terms() {
    let mut problems = Vec::new();
    let listed = run(&["help", "--all"]);
    for name in root_names(&listed) {
        walk(&[name], &mut problems);
    }
    assert!(
        problems.is_empty(),
        "help is not plain:\n{}",
        problems.join("\n")
    );
}
