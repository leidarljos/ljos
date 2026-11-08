//! Facts a session leaves behind, kept without anyone asking. When a
//! runner ends a session, the hook reads its transcript and keeps up to
//! [`MAX_PER_SESSION`] sentences that state something durable: a rule
//! ("always", "never", "must"), a cause ("root cause", "because", "turns
//! out"), or a fix. Each is written at once as a `note` whose
//! `source.basis` is `agent`, carrying `via:session-end`, the session,
//! and the open sitting's issue. The read side goes by that stored basis:
//! recall, search and the island label it an unverified agent note, and
//! no prompt hook hands it on as a fact or a rule. With no sitting,
//! packset has no place for a note, and the write is skipped. No model
//! runs: the extraction is a bounded, deterministic read. A sentence
//! already kept (by its normalized form) is not written again.
//!
//! The read runs at session end. `LJOS_EXTRACT=off` turns it off.
//! A mined sentence is still nobody's claim: the note stays on the
//! tracker, and no prompt hook injects it.

use serde_json::Value;
use std::collections::BTreeSet;

/// The most facts one session writes.
pub const MAX_PER_SESSION: usize = 5;
/// The longest transcript read, in bytes, from its end.
pub const MAX_TRANSCRIPT_BYTES: usize = 2 * 1024 * 1024;

/// The last `cap` bytes of `path`, or the whole file when it is shorter.
/// The head of a long transcript is not read. `None` when the file cannot
/// be opened.
#[must_use]
pub fn read_tail(path: &std::path::Path, cap: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(cap as u64);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.take(len - start).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

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

/// The prose of a transcript: JSONL with one message a line (the terminal harness,
/// the command harness and most runners), or plain text.
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

/// The pack's reason with the HTTP status left off, so a debug line does
/// not repeat `400:`.
fn plain_failure(err: &anyhow::Error) -> String {
    let text = format!("{err:#}");
    let parts: Vec<&str> = text.split(": ").collect();
    for (i, part) in parts.iter().enumerate() {
        let status = part.trim().parse::<u16>();
        if status.is_ok_and(|code| (400..=599).contains(&code)) {
            let rest = parts[i + 1..].join(": ");
            if !rest.is_empty() {
                return rest;
            }
        }
    }
    text
}

/// Whether session-end extraction runs. It does, unless `LJOS_EXTRACT=off`.
#[must_use]
pub fn asked_for() -> bool {
    asked_by(std::env::var("LJOS_EXTRACT").ok().as_deref())
}

/// [`asked_for`] over the variable's value, so a test names it.
#[must_use]
pub fn asked_by(set: Option<&str>) -> bool {
    !matches!(set.map(str::trim), Some("off"))
}

/// Hand the session finish to a detached process and return. The hook
/// does not read the transcript and does not call the pack. The child
/// fires the session's memories, writes the notes, and drains a queued
/// tracker push. Returns 0: the count is not known until the child ends.
pub fn at_session_end(input: &str, session: Option<&str>) -> usize {
    schedule_finish(input, session);
    0
}

/// Write the hook payload and start `ljos hook --finish-session` in its
/// own session. A failure to start is one line. The hook does not do the
/// work itself.
pub fn schedule_finish(input: &str, session: Option<&str>) {
    let dir = crate::runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(dir.join("session-finish.done"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let payload = dir.join(format!("session-finish-{stamp}.json"));
    let body = serde_json::json!({
        "session": session,
        "input": input,
    });
    if std::fs::write(&payload, body.to_string()).is_err() {
        eprintln!("ljos extract: the session finish could not be queued");
        return;
    }
    let bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "ljos".to_string());
    let args = vec![
        "hook".to_string(),
        "--finish-session".to_string(),
        payload.display().to_string(),
    ];
    let log = dir.join("session-finish.log");
    if crate::detach(&bin, &args, &log).is_err() {
        let _ = std::fs::remove_file(&payload);
        eprintln!("ljos extract: the session finish could not be started");
    }
}

/// The detached session finish. `path` is the job [`schedule_finish`] wrote.
///
/// # Errors
///
/// The job file cannot be read.
pub fn run_finish_job(path: &std::path::Path) -> anyhow::Result<()> {
    // This process is not the runner's hook. A pack read may take its
    // own timeout. The wall budget stays off.
    unsafe { std::env::remove_var("LJOS_IN_HOOK") };
    let raw = std::fs::read_to_string(path)
        .map_err(|err| anyhow::anyhow!("session finish: {} ({err})", path.display()))?;
    let _ = std::fs::remove_file(path);
    let job: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let input = job["input"].as_str().unwrap_or("").to_string();
    let session = job["session"].as_str().map(str::to_string);
    let _ = crate::session_end(session.as_deref());
    let lines = write_notes(&input, session.as_deref());
    let _ = crate::drain_tracker_pushes();
    mark_done(&lines);
    Ok(())
}

fn mark_done(lines: &[String]) {
    let path = crate::runtime_dir().join("session-finish.done");
    let body = if lines.is_empty() {
        "ok\n".to_string()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    let _ = std::fs::write(path, body);
}

/// Read the session's transcript (the hook payload's `transcript_path`)
/// from its tail, write each new sentence as an unverified note on the
/// open sitting, and return the lines this printed. The prose is the
/// seat's own reading, so the note is never shown as a fact. With no
/// sitting the write is skipped: packset keeps a note only on a ticket.
/// Quiet on every other failure.
fn write_notes(input: &str, session: Option<&str>) -> Vec<String> {
    if !asked_for() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(input) else {
        return Vec::new();
    };
    let Some(path) = v["transcript_path"]
        .as_str()
        .or_else(|| v["transcriptPath"].as_str())
    else {
        return Vec::new();
    };
    let Some(text) = read_tail(std::path::Path::new(path), MAX_TRANSCRIPT_BYTES) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let say = |lines: &mut Vec<String>, line: String| {
        eprintln!("{line}");
        lines.push(line);
    };
    let found = facts(&text, &kept());
    let debug = std::env::var_os("LJOS_EXTRACT_DEBUG").is_some();
    if debug {
        say(
            &mut lines,
            format!("ljos extract: {} facts from {path}", found.len()),
        );
    }
    if found.is_empty() {
        return lines;
    }
    // Packset keeps a note only on a ticket, and has no form for one that
    // names none. Skip before the post, one line, rather than collect a
    // refusal per fact.
    if crate::live_held_issue().is_none() {
        for fact in &found {
            crate::memory_port::keep_unattached(fact);
        }
        say(
            &mut lines,
            format!("ljos extract: {}", crate::NOTE_WITHOUT_TICKET),
        );
        return lines;
    }
    let mut written = 0;
    let mut keys = Vec::new();
    for f in found {
        let mut tags = vec!["via:session-end".to_string()];
        if let Some(s) = session {
            tags.push(format!("session:{s}"));
        }
        match crate::packset_write_agent_note_tagged(&f, &tags) {
            Ok(_) => {
                written += 1;
                keys.push(normal(&f));
            }
            Err(e) => {
                if debug {
                    say(
                        &mut lines,
                        format!("ljos extract: {f:?} not written: {}", plain_failure(&e)),
                    );
                }
            }
        }
    }
    if debug {
        say(&mut lines, format!("ljos extract: wrote {written}"));
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
    lines
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

    /// Unset runs the read. Only `off` stops it.
    #[test]
    fn extraction_runs_unless_turned_off() {
        assert!(asked_by(None));
        assert!(asked_by(Some("")));
        assert!(asked_by(Some("on")));
        assert!(asked_by(Some(" on ")));
        assert!(asked_by(Some("1")));
        assert!(!asked_by(Some("off")));
        assert!(!asked_by(Some(" off ")));
    }

    #[test]
    fn a_long_transcript_is_read_from_its_tail() {
        let dir = std::env::temp_dir().join(format!("ljos-tail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let mut body =
            String::from("Always keep this sentence that sits at the head of the file.\n");
        let pad = format!("{}\n", "x".repeat(200));
        while body.len() < 8_000 {
            body.push_str(&pad);
        }
        body.push_str("Never push to main without a green CI run on the branch.\n");
        std::fs::write(&path, &body).unwrap();
        let got = read_tail(&path, 800).unwrap();
        assert!(got.contains("Never push"), "{got}");
        assert!(!got.contains("sits at the head"), "{got}");
        assert!(got.len() <= 800, "{}", got.len());
        let _ = std::fs::remove_dir_all(&dir);
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
