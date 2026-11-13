//! Next-step lines are one sentence, then `, so run \`command\`.`.
//!
//! Nineteen user-facing lines already used that form and twenty-eight used
//! `. Run \`command\``. The no-hits line stays on the first form, and this
//! test reads the sources so the other form cannot come back.

use std::path::Path;

#[test]
fn no_hits_keeps_the_so_run_line() {
    assert_eq!(
        ljos_cli::scan::no_hits_line("which fusion", false),
        "No hits for \"which fusion\", so run `ljos search fusion`."
    );
}

#[test]
fn sources_do_not_use_the_other_next_step_form() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut bad = Vec::new();
    for path in rust_files(&root) {
        let text = std::fs::read_to_string(&path).unwrap();
        for (index, line) in text.lines().enumerate() {
            let code = strip_line_comment(line);
            if other_form(code) {
                bad.push(format!("{}:{}: {line}", display(&path), index + 1));
            }
        }
        let mut from = 0;
        while let Some(at) = text[from..].find(", so run `") {
            let abs = from + at;
            let window = &text[abs..(abs + 500).min(text.len())];
            let closed = window.contains("`.") || window.contains("`\"");
            if !closed {
                let line = text[..abs].bytes().filter(|byte| *byte == b'\n').count() + 1;
                bad.push(format!(
                    "{}:{line}: a `, so run` line does not end with the command and a period",
                    display(&path)
                ));
            }
            from = abs + ", so run `".len();
        }
    }
    assert!(
        bad.is_empty(),
        "the other next-step form is back:\n{}",
        bad.join("\n")
    );
}

fn other_form(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(at) = line[start..].find(". Run `") {
        let abs = start + at;
        let numbered = abs > 0 && bytes[abs - 1].is_ascii_digit();
        if !numbered {
            return true;
        }
        start = abs + ". Run `".len();
    }
    false
}

/// Drop a `//` comment that is not inside a string, so a comment cannot hide
/// the form and a comment that only mentions it is not a hit.
fn strip_line_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && quote == Some(b'"') {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            quote = match quote {
                Some(open) if open == byte => None,
                Some(_) => quote,
                None => Some(byte),
            };
            index += 1;
            continue;
        }
        if quote.is_none() && byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            return &line[..index];
        }
        index += 1;
    }
    line
}

fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out
}

fn display(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(path)
        .display()
        .to_string()
}
