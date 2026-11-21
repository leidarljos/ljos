//! The prose of the repository's own Org docs, put through the public-text
//! screen. A refused line must be one that names a vendor or model product
//! on purpose, or quotes a private shape as an example. Anything else is a
//! false refusal.

use ljos_cli::public_text::{classify, scan, Class, Line, Screen};
use std::path::{Path, PathBuf};

const VENDOR_WORDS: &[&str] = &[
    "claude",
    "codex",
    "cursor",
    "grok",
    "windsurf",
    "kiro",
    "cline",
    "qwen",
    "copilot",
    "openrouter",
    "llama",
    "gemini",
    "model context protocol",
    "subagent",
    "cohere",
    "openhands",
    "aider",
    "anthropic",
    "xai",
    "gpt",
    "opus",
    "mem0",
];

/// Lines that quote a private shape to say what is refused.
const QUOTED_SHAPES: &[&str] = &[
    "/tmp",
    "/var/tmp",
    "/dev/shm",
    "/proc/net",
    "/srv/handoff",
    "127.0.0.1",
    "::a",
    "0x7f",
    "127.1",
    "alice",
    "GITHUB_TOKEN = x",
    "dead::beef",
    "src_NAME{",
    "hunter",
    "user:pass",
];

fn docs() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/orgmode");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("docs/orgmode")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("org"))
        .collect();
    files.sort();
    files
}

fn prose(text: &str) -> Vec<(usize, &str)> {
    let mut inside = false;
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("#+begin") {
            inside = true;
            continue;
        }
        if lower.starts_with("#+end") {
            inside = false;
            continue;
        }
        let property = trimmed.starts_with(':')
            && trimmed
                .trim_start_matches(':')
                .split(':')
                .next()
                .is_some_and(|name| {
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_uppercase() || matches!(c, '_' | '-'))
                });
        if inside
            || property
            || trimmed.is_empty()
            || trimmed.starts_with(['*', '|'])
            || trimmed.starts_with("#+")
            || (trimmed.starts_with(':') && trimmed.ends_with(':'))
        {
            continue;
        }
        out.push((index + 1, line));
    }
    out
}

fn explained(class: Class, line: &str) -> bool {
    let lower = line.to_lowercase();
    match class {
        Class::Term => VENDOR_WORDS.iter().any(|word| lower.contains(word)),
        _ => QUOTED_SHAPES.iter().any(|shape| line.contains(shape)),
    }
}

#[test]
fn every_refused_doc_line_names_a_vendor_or_quotes_a_private_shape() {
    let screen = Screen::default();
    let mut total = 0;
    let mut refused = 0;
    let mut refused_internal = 0;
    let mut by_class: std::collections::BTreeMap<&str, usize> = Default::default();
    let internal = Screen {
        internal: true,
        ..Screen::default()
    };
    let mut unexplained = Vec::new();
    for path in docs() {
        let text = std::fs::read_to_string(&path).unwrap();
        let lines = prose(&text);
        total += lines.len();
        let block: Vec<Line<'_>> = lines.iter().map(|(_, line)| Line::text(line)).collect();
        let joined = scan(&block, &screen);
        refused_internal += scan(&block, &internal)
            .iter()
            .zip(&lines)
            .filter(|(class, (_, line))| class.is_some() || classify(line, &internal).is_some())
            .count();
        for ((number, line), together) in lines.iter().zip(joined) {
            let alone = classify(line, &screen);
            if let Some(class) = alone.or(together) {
                refused += 1;
                *by_class.entry(class.name()).or_default() += 1;
            }
            for class in [alone, together].into_iter().flatten() {
                if !explained(class, line) {
                    unexplained.push(format!(
                        "{}:{number}: {} | {line}",
                        path.file_name().unwrap().to_string_lossy(),
                        class.name()
                    ));
                }
            }
        }
    }
    eprintln!(
        "docs prose lines {total}, refused {refused} {by_class:?}, refused with the ljos vocabulary on {refused_internal}"
    );
    assert!(total > 2000, "only {total} prose lines were read");
    assert!(
        unexplained.is_empty(),
        "{} false refusals:\n{}",
        unexplained.len(),
        unexplained.join("\n")
    );
}
