//! Facts a session leaves behind, kept without anyone asking. When a
//! runner ends a session, the hook reads its transcript and keeps up to
//! [`MAX_PER_SESSION`] sentences that state something durable: a rule
//! ("always", "never", "must"), a cause ("root cause", "because", "turns
//! out"), or a fix. Each is written at once as an `agent-derived` lesson
//! carrying `via:session-end` and the session, so `ljos memories review`
//! and `ljos memories prune` clean them up later. No model runs: the
//! extraction is a bounded, deterministic read. A sentence already kept
//! (by its normalized form) is not written again. `LJOS_EXTRACT=off`
//! turns it off.

use serde_json::Value;
use std::collections::BTreeSet;

/// The most facts one session writes.
pub const MAX_PER_SESSION: usize = 5;
/// The longest transcript read, in bytes, from its end.
pub const MAX_TRANSCRIPT_BYTES: usize = 2 * 1024 * 1024;

const CUES: &[&str] = &[
    "always ",
    "never ",
    " must ",
    "root cause",
    "turns out",
    "turned out",
    "the fix ",
    "fixed by",
    " because ",
    " requires ",
    " instead of ",
    "does not work",
    "doesn't work",
    "is broken",
    "the reason ",
    "make sure ",
    "remember that",
];

/// A sentence's form for deduplication: lowercase words only.
#[must_use]
pub fn normal(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn texts_of(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| texts_of(x, out)),
        Value::Object(o) => {
            // Tool calls and results are not what the session concluded.
            if matches!(
                o.get("type").and_then(Value::as_str),
                Some("tool_use" | "tool_result" | "thinking" | "image")
            ) {
                return;
            }
            for key in ["text", "content", "message"] {
                if let Some(x) = o.get(key) {
                    texts_of(x, out);
                }
            }
        }
        _ => {}
    }
}

/// The prose of a transcript: JSONL with one message a line (Claude Code,
/// Codex and most runners), or plain text.
#[must_use]
pub fn prose(transcript: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in transcript.lines() {
        match serde_json::from_str::<Value>(line) {
            Ok(v) => {
                let role = v["type"]
                    .as_str()
                    .or_else(|| v["role"].as_str())
                    .or_else(|| v["message"]["role"].as_str())
                    .unwrap_or("");
                if matches!(role, "user" | "assistant" | "") {
                    texts_of(&v, &mut out);
                }
            }
            Err(_) => out.push(line.to_string()),
        }
    }
    out
}

fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut cur = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            cur.push(c);
            // A stop ends a sentence only before a space or the line's end,
            // so `build.rs` and `v0.29.1` stay whole.
            if matches!(c, '.' | '!' | '?') && chars.peek().is_none_or(|n| n.is_whitespace()) {
                out.push(std::mem::take(&mut cur));
            }
        }
        if !cur.trim().is_empty() {
            out.push(cur);
        }
    }
    out.into_iter()
        .map(|s| {
            s.trim()
                .trim_start_matches(['-', '*', '>', ' '])
                .trim()
                .to_string()
        })
        .collect()
}

/// Whether a sentence states something durable enough to keep.
#[must_use]
pub fn is_fact(s: &str) -> bool {
    let n = s.chars().count();
    if !(30..=240).contains(&n) || s.ends_with('?') || s.contains('`') || s.contains("://") {
        return false;
    }
    let low = format!(" {} ", s.to_lowercase());
    // A question, an instruction to this session, or a placeholder.
    if low.starts_with(" can you") || low.starts_with(" please") || low.contains(" i will ") {
        return false;
    }
    CUES.iter().any(|c| low.contains(c))
}

/// Up to [`MAX_PER_SESSION`] facts from a transcript, newest last, with no
/// two of the same normalized form and none in `kept`.
#[must_use]
pub fn facts(transcript: &str, kept: &BTreeSet<String>) -> Vec<String> {
    let mut seen = kept.clone();
    let mut out: Vec<String> = Vec::new();
    for text in prose(transcript) {
        for s in sentences(&text) {
            if !is_fact(&s) {
                continue;
            }
            let key = normal(&s);
            if seen.insert(key) {
                out.push(s);
            }
        }
    }
    // The end of a session is what it concluded.
    let skip = out.len().saturating_sub(MAX_PER_SESSION);
    out.into_iter().skip(skip).collect()
}

fn ledger() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })?;
    Some(base.join("ljos").join("extracted"))
}

/// The normalized forms already kept on this machine.
#[must_use]
pub fn kept() -> BTreeSet<String> {
    ledger()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Read the session's transcript (the hook payload's `transcript_path`),
/// write each new fact as an agent-derived lesson, and return how many were
/// written. Quiet on every failure: a hook must not stop a runner ending.
pub fn at_session_end(input: &str, session: Option<&str>) -> usize {
    if std::env::var("LJOS_EXTRACT").is_ok_and(|v| v == "off") {
        return 0;
    }
    let Ok(v) = serde_json::from_str::<Value>(input) else {
        return 0;
    };
    let Some(path) = v["transcript_path"]
        .as_str()
        .or_else(|| v["transcriptPath"].as_str())
    else {
        return 0;
    };
    let Ok(bytes) = std::fs::read(path) else {
        return 0;
    };
    let tail = &bytes[bytes.len().saturating_sub(MAX_TRANSCRIPT_BYTES)..];
    let text = String::from_utf8_lossy(tail);
    let found = facts(&text, &kept());
    if std::env::var_os("LJOS_EXTRACT_DEBUG").is_some() {
        eprintln!("ljos extract: {} facts from {path}", found.len());
    }
    let mut written = 0;
    let mut keys = Vec::new();
    for f in found {
        let mut tags = vec!["via:session-end".to_string()];
        if let Some(s) = session {
            tags.push(format!("session:{s}"));
        }
        match crate::packset_write_agent_tagged("Remember", &f, &tags) {
            Ok(_) => {
                written += 1;
                keys.push(normal(&f));
            }
            Err(e) => {
                if std::env::var_os("LJOS_EXTRACT_DEBUG").is_some() {
                    eprintln!("ljos extract: {f:?} not written: {e:#}");
                }
            }
        }
    }
    if let (Some(p), false) = (ledger(), keys.is_empty()) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut all: Vec<String> = kept().into_iter().collect();
        all.extend(keys);
        // Bounded: the newest 5000 forms.
        let skip = all.len().saturating_sub(5000);
        let _ = std::fs::write(&p, all[skip..].join("\n") + "\n");
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = r#"{"type":"user","message":{"role":"user","content":"Can you fix the flaky build?"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Looking now."},{"type":"tool_use","name":"Bash","input":{"command":"cargo test because x"}}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The root cause was a shared temp dir between tests. The fix is to give each test its own tempdir. Always run the suite with --test-threads=1 on this box. Done!"}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The root cause was a shared temp dir between tests."}]}}"#;

    #[test]
    fn a_transcript_yields_its_durable_sentences_once() {
        let got = facts(T, &BTreeSet::new());
        assert_eq!(got.len(), 3, "{got:?}");
        assert!(got[0].starts_with("The root cause was"));
        assert!(got.iter().all(|f| !f.contains("Can you")));
        assert!(
            got.iter().all(|f| !f.contains("cargo test")),
            "tool calls are not facts"
        );
    }

    #[test]
    fn already_kept_facts_are_skipped_and_the_count_is_bounded() {
        let kept: BTreeSet<String> = [normal(
            "The root cause was a shared temp dir between tests.",
        )]
        .into();
        assert_eq!(facts(T, &kept).len(), 2);
        let many: String = (0..20)
            .map(|i| format!("Always pin tool number {i} to its tested version.\n"))
            .collect();
        let got = facts(&many, &BTreeSet::new());
        assert_eq!(got.len(), MAX_PER_SESSION);
        assert!(got.last().unwrap().contains("19"), "the newest are kept");
    }

    #[test]
    fn questions_code_and_short_lines_are_not_facts() {
        assert!(!is_fact("Should we always do this before a release?"));
        assert!(!is_fact("Always."));
        assert!(!is_fact(
            "Always run `rm -rf target` before a build here, it helps."
        ));
        assert!(is_fact(
            "Never push to main without a green CI run on the branch."
        ));
    }
}
