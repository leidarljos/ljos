//! A standing choice the person states in chat.
//!
//! "From now on always use bun, never yarn" is the person telling the seat
//! something to keep. The prompt hook saves that sentence, in the person's
//! words, as a standing preference, and says so in one line with the id
//! that takes it back. It stands at once only when this process is not
//! under a runner. Under a runner the same bytes are a forged hook as
//! easily as a real one, so the sentence is held for review and is not
//! given a person basis.
//!
//! An agent's `ljos remember` of words the person typed in their latest
//! prompt is the same case, so [`said_in_latest_prompt`] lets it stand.
//!
//! `LJOS_CAPTURE` picks what happens: `save` (the default), `nudge` (one
//! line asking the agent to write it), or `off`.

use serde_json::Value;
use sha2::Digest as _;

use super::{
    add_entities, admit, atom_body, mark_seen, pack, pasted_or_quoted, runtime_dir, seen_ids,
    stamp_horizon, with_pack_timeout, HookCall,
};

/// What the prompt hook does with a standing choice it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    /// Write it as the person's preference and say so.
    Save,
    /// Ask the agent, in one line, to write it.
    Nudge,
    /// Leave it.
    Off,
}

/// The mode `LJOS_CAPTURE` names. Unset or unknown is [`CaptureMode::Save`].
#[must_use]
pub fn capture_mode() -> CaptureMode {
    match std::env::var("LJOS_CAPTURE")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "nudge" => CaptureMode::Nudge,
        "off" | "0" | "no" | "false" => CaptureMode::Off,
        _ => CaptureMode::Save,
    }
}

/// A standing choice read from a prompt: what kind of claim it is and the
/// person's sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stated {
    /// `preference`, or `lesson` for "remember that ...".
    pub kind: &'static str,
    pub text: String,
}

/// The longest sentence read as a standing choice. A longer one is a
/// paragraph of context, not a rule.
pub const STATED_MAX_CHARS: usize = 200;

/// Openings that state a standing choice when a clause starts with them.
const LEADS: &[&str] = &[
    "from now on",
    "from here on",
    "henceforth",
    "always ",
    "never ",
    "remember that ",
    "remember to ",
    "do not ever ",
    "don't ever ",
    "dont ever ",
    "make sure to always ",
    "make sure you always ",
    "make sure you never ",
    "i prefer ",
    "we prefer ",
    "we always ",
    "we never ",
    "stop using ",
];

/// Openings that state a standing choice when a comma follows them:
/// "going forward, ..." but not "going forward is ...".
const COMMA_LEADS: &[&str] = &["going forward,", "in future,", "in the future,"];

/// Phrases that make a sentence standing wherever they sit in it.
const ANYWHERE: &[&str] = &["from now on", "from here on", "henceforth"];

/// Phrases that make a sentence standing when they end it: "use bun going
/// forward".
const ENDINGS: &[&str] = &["from now on", "going forward", "in future", "from here on"];

/// Words after "always" or "never" that make it a remark, not a rule:
/// "never mind", "always the same", "never seen this".
const NOT_A_RULE: &[&str] = &[
    "mind", "again", "the", "a", "an", "been", "seen", "heard", "was", "were", "is", "are", "had",
    "did", "worked", "works", "work", "happened", "happens", "thought", "knew", "got", "gets",
    "fails", "failed", "breaks", "broke", "on", "in", "at", "for", "with", "by", "when", "since",
    "before", "after", "really", "quite", "sure", "ever", "once", "seems", "seemed",
];

/// Words that open a sentence and some filler before it.
const FILLER: &[&str] = &[
    "ok", "okay", "so", "also", "and", "btw", "oh", "hey", "note", "fyi", "please", "but",
];

/// Openings of a question that the person did not end with `?`.
const QUESTION_OPENINGS: &[&str] = &[
    "what", "why", "how", "can", "could", "should", "would", "do", "does", "did", "is", "are",
    "will", "which", "who", "where", "when", "was", "were", "shall",
];

/// The prompt's sentences: lines split after `.`, `!` and `?` that a space
/// or the line's end follows. `Cargo.toml` and `e.g.` stay whole.
fn sentences(prompt: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in prompt.lines() {
        let mut cur = String::new();
        let chars: Vec<char> = line.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            cur.push(*c);
            let ends =
                matches!(c, '.' | '!' | '?') && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
            if ends {
                let s = cur.trim().to_string();
                if !s.is_empty() {
                    out.push(s);
                }
                cur.clear();
            }
        }
        let s = cur.trim().to_string();
        if !s.is_empty() {
            out.push(s);
        }
    }
    out
}

/// `text` with every backtick span blanked, so a lead inside code
/// (`` `--always-retry` ``) does not count.
fn outside_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for c in text.chars() {
        if c == '`' {
            inside = !inside;
            out.push(' ');
        } else if inside {
            out.push('x');
        } else {
            out.push(c);
        }
    }
    out
}

/// The sentence with filler words at its front taken off: "ok, from now
/// on ..." reads as "from now on ...".
fn strip_filler(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        let word_end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '\'')
            .unwrap_or(rest.len());
        let word = rest[..word_end].to_ascii_lowercase();
        if word.is_empty() || !FILLER.contains(&word.as_str()) {
            return rest;
        }
        let after = &rest[word_end..];
        let after = after.trim_start_matches(|c: char| c == ',' || c == ':' || c.is_whitespace());
        if after.len() == rest.len() || after.is_empty() {
            return rest;
        }
        rest = after;
    }
}

/// Whether one clause, lowercased, opens with a standing lead.
fn clause_leads(clause: &str) -> bool {
    let c = clause.trim_start();
    LEADS.iter().any(|lead| {
        let Some(rest) = c.strip_prefix(lead) else {
            return false;
        };
        if matches!(*lead, "always " | "never ") {
            let next = rest
                .split(|ch: char| !ch.is_alphanumeric() && ch != '\'')
                .next()
                .unwrap_or("");
            return !next.is_empty() && !NOT_A_RULE.contains(&next);
        }
        true
    })
}

/// Whether a sentence, as typed, states a standing choice.
fn states_a_rule(sentence: &str) -> bool {
    let words = sentence.split_whitespace().count();
    if !(3..=40).contains(&words) || sentence.chars().count() > STATED_MAX_CHARS {
        return false;
    }
    if sentence.trim_end().ends_with('?') {
        return false;
    }
    let plain = outside_code(strip_filler(sentence)).to_lowercase();
    let first = plain
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .next()
        .unwrap_or("");
    if QUESTION_OPENINGS.contains(&first) || first == "you" {
        return false;
    }
    let bare = plain.trim_end_matches(|c: char| !c.is_alphanumeric());
    if ANYWHERE.iter().any(|p| plain.contains(p))
        || ENDINGS.iter().any(|p| bare.ends_with(p))
        || COMMA_LEADS
            .iter()
            .any(|p| plain.trim_start().starts_with(p))
    {
        return true;
    }
    plain.split([',', ';']).any(clause_leads)
}

/// The words saved for a stated choice: the sentence with its filler
/// taken off, capitalized. "Remember that X" saves X as a lesson.
fn saved_words(sentence: &str) -> Stated {
    let s = strip_filler(sentence).trim();
    let lower = s.to_lowercase();
    let (kind, body) = match lower.strip_prefix("remember that ") {
        Some(_) => ("lesson", &s["remember that ".len()..]),
        None => ("preference", s),
    };
    let mut chars = body.trim().chars();
    let text = match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    Stated { kind, text }
}

/// The standing choice a prompt states, if it states one: first-person or
/// imperative phrasing such as "from now on", "always", "never", "going
/// forward" or "remember that", outside code and quotes, in a sentence of
/// at most [`STATED_MAX_CHARS`] characters. A question, pasted output and
/// a quoted rule state nothing.
#[must_use]
pub fn stated_preference(prompt: &str) -> Option<Stated> {
    if pasted_or_quoted(prompt) {
        return None;
    }
    sentences(prompt)
        .into_iter()
        .find(|s| states_a_rule(s))
        .map(|s| saved_words(&s))
        .filter(|s| s.text.split_whitespace().count() >= 2)
}

/// Whether `LJOS_UNATTENDED` says no person is at this session: `1`,
/// `true`, `yes` or `on`. An overnight runner or a script driving an agent
/// sets it, and its prompts are then nobody's stated choice.
#[must_use]
pub fn unattended() -> bool {
    std::env::var("LJOS_UNATTENDED").is_ok_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// The thread a Codex rollout file is for, from a name such as
/// `rollout-2026-10-11T09-30-00-<uuid>.jsonl`, or `None` for any other
/// name.
fn rollout_thread(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?;
    let rest = name.strip_prefix("rollout-")?;
    let stem = &rest[..rest.find(".jsonl")?];
    // The stamp is `YYYY-MM-DDTHH-MM-SS-`, twenty characters.
    let id = stem.get(20..56)?;
    let tail = &stem[56..];
    let uuid_shape = id.char_indices().all(|(i, c)| {
        if matches!(i, 8 | 13 | 18 | 23) {
            c == '-'
        } else {
            c.is_ascii_hexdigit()
        }
    });
    (uuid_shape && (tail.is_empty() || tail.starts_with('_'))).then(|| id.to_ascii_lowercase())
}

/// Why a prompt event does not carry the person's words, or `None` when
/// it does. A prompt event is not always typed by a person: Codex fires it
/// inside a subagent, with the brief the parent agent wrote as `prompt`.
/// A spawned agent's payload names it with `agent_id`, as Claude Code's
/// does inside a subagent, and Codex's own review thread names a rollout
/// file for a thread other than the session's. [`unattended`] covers a
/// session with nobody there at all.
#[must_use]
pub fn not_the_person(input: &str) -> Option<&'static str> {
    if unattended() {
        return Some("LJOS_UNATTENDED is set");
    }
    let v: Value = serde_json::from_str(input.trim()).ok()?;
    let named = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };
    if named("agent_id") || named("agentId") {
        return Some("a subagent's brief");
    }
    let session = v
        .get("session_id")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let thread = v
        .get("transcript_path")
        .and_then(Value::as_str)
        .and_then(rollout_thread);
    match (session, thread) {
        (Some(session), Some(thread)) if session != thread => Some("another thread's prompt"),
        _ => None,
    }
}

/// How long a prompt counts as the latest one for [`said_in_latest_prompt`].
pub const LATEST_PROMPT_SECS: u64 = 3600;

/// The longest prompt kept for [`said_in_latest_prompt`].
const LATEST_PROMPT_CHARS: usize = 8000;

fn latest_prompt_path() -> std::path::PathBuf {
    runtime_dir().join("latest-prompt")
}

/// Keep the person's prompt, so an agent's `ljos remember` of their words
/// can be told from the agent's own lesson. The file sits in the login's
/// runtime directory, readable by its owner only, and each prompt
/// replaces it. Off when [`capture_mode`] is off. A bind beside the file
/// records the hash, mtime and session, and [`said_in_latest_prompt`]
/// requires it: a plain write of the prompt file does not count, and
/// neither does a prompt bound to another session or to none.
pub fn note_latest_prompt(prompt: &str) {
    note_latest_prompt_for(prompt, None);
}

/// [`note_latest_prompt`] recording `session`, the hook's session id.
pub fn note_latest_prompt_for(prompt: &str, session: Option<&str>) {
    let sessions: Vec<String> = session.map(str::to_string).into_iter().collect();
    note_latest_prompt_as(prompt, &sessions);
}

/// [`note_latest_prompt`] recording every id in `sessions`: a runner may
/// name one conversation by several, and the process that checks the
/// bind sees whichever the runner stamped on it.
pub fn note_latest_prompt_as(prompt: &str, sessions: &[String]) {
    note_latest_prompt_from(prompt, sessions, None, None);
}

/// [`note_latest_prompt_as`] recording `runner`, the process that spawned
/// the hook ([`crate::proc_tree::hook_runner`]): a process under that
/// very runner is in its conversation whatever its environment names.
/// `transcript` is the runner's own record of the conversation, as its
/// hook payload names it.
pub fn note_latest_prompt_from(
    prompt: &str,
    sessions: &[String],
    runner: Option<&crate::proc_tree::Proc>,
    transcript: Option<&str>,
) {
    if capture_mode() == CaptureMode::Off || pasted_or_quoted(prompt) {
        return;
    }
    let path = latest_prompt_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = prompt.chars().take(LATEST_PROMPT_CHARS).collect();
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let wrote = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if wrote.is_ok() {
        let bind = Bind {
            runner: runner.map_or_else(|| "-".to_string(), crate::proc_tree::token),
            at: now_nanos(),
            transcript: bind_session(transcript),
        };
        let _ = write_prompt_bind(&path, text.as_bytes(), &bind, sessions);
    }
}

/// What the hook records beside the hash: the runner that spawned it,
/// when it wrote the prompt, and the runner's record of the conversation.
struct Bind {
    runner: String,
    at: u128,
    transcript: String,
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// The transcript a prompt hook payload names, as Claude Code, Codex and
/// Cursor send it.
#[must_use]
pub fn payload_transcript(input: &str) -> Option<String> {
    let v: Value = serde_json::from_str(input.trim()).ok()?;
    v["transcript_path"]
        .as_str()
        .or_else(|| v["transcriptPath"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// How long after the hook's time the bind's own write may land.
const BIND_WRITE_NANOS: u128 = 500_000_000;

/// The latest of `path`'s mtime and ctime in nanoseconds. A process can
/// set the mtime it likes; the kernel sets the ctime on every change.
fn changed_nanos(path: &std::path::Path) -> Option<u128> {
    use std::os::unix::fs::MetadataExt as _;
    let m = std::fs::metadata(path).ok()?;
    let at = |s: i64, ns: i64| -> Option<u128> {
        Some(u128::try_from(s).ok()? * 1_000_000_000 + u128::try_from(ns).ok()?)
    };
    Some(at(m.ctime(), m.ctime_nsec())?.max(at(m.mtime(), m.mtime_nsec())?))
}

/// Whether `kept` agrees with the runner's own record of the person's
/// turn, when the runner above keeps one ([`crate::turn_record`]). The
/// record is the one the bind names, inside that runner's own directory,
/// or for Cursor the transcript of a conversation the runner's
/// environment names. Claude Code and Codex always name theirs, so a bind
/// that names none, or names a file elsewhere, is refused under them.
fn agrees_with_record(kept: &[u8], named: &str, runner_ids: &[String]) -> bool {
    use crate::turn_record;
    let Some((runner, kind)) = crate::proc_tree::runner_above() else {
        return true;
    };
    let env = crate::proc_tree::environ(runner.pid).unwrap_or_default();
    let roots = turn_record::roots(kind, &env);
    if roots.is_empty() {
        return true;
    }
    let record = if named == "-" {
        roots
            .iter()
            .find_map(|r| turn_record::cursor_record(r, runner_ids))
            .filter(|_| matches!(kind, "cursor" | "cursor-agent"))
    } else {
        turn_record::within(std::path::Path::new(named), &roots)
    };
    let Some(record) = record else {
        return named == "-" && !turn_record::required(kind);
    };
    // A turn's tool output can push the person's entry out of the tail.
    let Some(text) = crate::bookkeep::read_text_bounded(&record)
        .filter(|t| !turn_record::latest_turn(t).is_empty())
        .or_else(|| {
            record
                .is_file()
                .then(|| std::fs::read_to_string(&record).ok())
                .flatten()
        })
    else {
        return false;
    };
    if !turn_record::required(kind) && turn_record::latest_turn(&text).is_empty() {
        return true;
    }
    turn_record::agrees(&text, &String::from_utf8_lossy(kept))
}

fn latest_prompt_bind_path() -> std::path::PathBuf {
    runtime_dir().join("latest-prompt.bind")
}

fn mtime_nanos(path: &std::path::Path) -> Option<u128> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|age| age.as_nanos())
}

fn bind_session(session: Option<&str>) -> String {
    match session
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.contains(['\n', '\r']))
    {
        Some(s) => s.to_string(),
        None => "-".to_string(),
    }
}

/// The bind the hook writes beside the prompt: version, sha256 of the
/// bytes, mtime in nanoseconds, the runner that spawned the hook as a
/// [`crate::proc_tree::token`] or `-`, the hook's time in nanoseconds,
/// the runner's transcript or `-`, and the session ids, one a line. The
/// gate reads it back, so a later write of the prompt file no longer
/// matches.
fn write_prompt_bind(
    path: &std::path::Path,
    bytes: &[u8],
    bind: &Bind,
    sessions: &[String],
) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let Some(mtime) = mtime_nanos(path) else {
        return Err(std::io::Error::other("prompt mtime"));
    };
    let hash = hex::encode(sha2::Sha256::digest(bytes));
    let mut ids: Vec<String> = Vec::new();
    for s in sessions {
        let s = bind_session(Some(s));
        if s != "-" && !ids.contains(&s) {
            ids.push(s);
        }
    }
    if ids.is_empty() {
        ids.push(bind_session(None));
    }
    let body = format!(
        "3\n{hash}\n{mtime}\n{}\n{}\n{}\n{}\n",
        bind.runner,
        bind.at,
        bind.transcript,
        ids.join("\n")
    );
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(latest_prompt_bind_path())?;
    f.write_all(body.as_bytes())
}

/// Whether the prompt file still matches the bind the hook wrote, and the
/// bind is this conversation's: it names a session from the environment
/// of the runner above this process ([`crate::ballot_sig::runner_sessions`]),
/// or the runner it records is above this process. A bind with any other
/// session, or none, is a prompt from somewhere else, and with no runner
/// above nothing matches. An MCP call's `_meta` session only narrows
/// this: one the runner above did not give, or the bind does not name
/// when the runner gave none, is another conversation's call.
///
/// The prompt must also agree with the runner's own record of the turn
/// ([`agrees_with_record`]), and neither file may have changed after the
/// hook's time: a tool call that rewrites them leaves a later ctime.
fn prompt_bind_matches(path: &std::path::Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let Some(mtime) = mtime_nanos(path) else {
        return false;
    };
    let bind_path = latest_prompt_bind_path();
    let Ok(bind) = std::fs::read_to_string(&bind_path) else {
        return false;
    };
    let mut lines = bind.lines();
    let (Some("3"), Some(hash), Some(stored), Some(spawner), Some(at), Some(transcript)) = (
        lines.next(),
        lines.next(),
        lines.next(),
        lines.next(),
        lines.next().and_then(|t| t.parse::<u128>().ok()),
        lines.next(),
    ) else {
        return false;
    };
    let late = |p: &std::path::Path| {
        changed_nanos(p).is_none_or(|c| c > at.saturating_add(BIND_WRITE_NANOS))
    };
    if late(path) || late(&bind_path) {
        return false;
    }
    let bound: Vec<String> = lines
        .map(crate::ballot_sig::field)
        .filter(|s| s != "-")
        .collect();
    let runner = crate::ballot_sig::runner_sessions();
    let above = spawner != "-"
        && crate::proc_tree::ancestors()
            .iter()
            .any(|p| crate::proc_tree::token(p) == spawner);
    if !(above || bound.iter().any(|s| runner.contains(s))) {
        return false;
    }
    if let Some(call) = crate::ballot_sig::call_session() {
        let within = if runner.is_empty() { &bound } else { &runner };
        if !within.contains(&call) {
            return false;
        }
    }
    hash == hex::encode(sha2::Sha256::digest(&bytes))
        && stored == mtime.to_string()
        && agrees_with_record(&bytes, transcript, &runner)
}

/// Lowercase words and nothing else, for comparing a sentence with a
/// prompt whatever their spacing and punctuation.
fn words_only(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// One word of a prompt, lowercased, where it sits in the text, and
/// whether a sentence, a line or a clause ends before it. `stop_before`
/// is a sentence's end only: `.`, `!`, `?`, `;` or a blank line, not the
/// line break or colon a person puts inside one sentence.
struct Word {
    word: String,
    start: usize,
    end: usize,
    sentence_before: bool,
    stop_before: bool,
    clause_before: bool,
}

fn prompt_words(text: &str) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let (mut sentence, mut stop, mut clause) = (true, true, true);
    let mut line_ended = false;
    let mut at = None;
    let close =
        |out: &mut Vec<Word>, from: usize, to: usize, s: &mut bool, p: &mut bool, c: &mut bool| {
            let raw = &text[from..to];
            let lead = raw.len() - raw.trim_start_matches('\'').len();
            let word = raw.trim_matches('\'');
            if !word.is_empty() {
                out.push(Word {
                    word: word.to_lowercase(),
                    start: from + lead,
                    end: from + lead + word.len(),
                    sentence_before: std::mem::take(s),
                    stop_before: std::mem::take(p),
                    clause_before: std::mem::take(c),
                });
            }
        };
    for (i, ch) in text.char_indices() {
        if ch.is_alphanumeric() || ch == '\'' {
            at.get_or_insert(i);
            continue;
        }
        if let Some(from) = at.take() {
            close(&mut out, from, i, &mut sentence, &mut stop, &mut clause);
            line_ended = false;
        }
        if matches!(ch, '.' | '!' | '?' | ';' | '\n') {
            sentence = true;
        }
        if matches!(ch, '.' | '!' | '?' | ';') || (ch == '\n' && line_ended) {
            stop = true;
        }
        if ch == '\n' {
            line_ended = true;
        }
        if matches!(
            ch,
            ',' | ':'
                | '('
                | ')'
                | '['
                | ']'
                | '"'
                | '\u{201c}'
                | '\u{201d}'
                | '\u{2013}'
                | '\u{2014}'
        ) {
            clause = true;
        }
    }
    if let Some(from) = at {
        close(
            &mut out,
            from,
            text.len(),
            &mut sentence,
            &mut stop,
            &mut clause,
        );
    }
    out
}

/// Words that turn a clause around: a quote that leaves one of them
/// behind in its sentence says something the person did not.
fn turns_it_around(word: &str) -> bool {
    matches!(
        word,
        "not"
            | "never"
            | "no"
            | "nor"
            | "dont"
            | "cannot"
            | "without"
            | "avoid"
            | "unless"
            | "except"
            | "if"
            | "only"
            | "until"
    ) || word.ends_with("n't")
}

/// `text` with case, spacing and plain punctuation set aside, and every
/// other mark kept: a quote may drop a comma, not add a backtick.
fn marks_kept(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !matches!(
                c,
                '.' | ','
                    | ';'
                    | ':'
                    | '!'
                    | '?'
                    | '\''
                    | '"'
                    | '('
                    | ')'
                    | '-'
                    | '\u{2018}'
                    | '\u{2019}'
                    | '\u{201c}'
                    | '\u{201d}'
                    | '\u{2013}'
                    | '\u{2014}'
            )
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` in NFKC with every apostrophe a keyboard or an editor types
/// read as `'`, so `Don’t` is the word `don't` and turns a clause around.
fn plain(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    text.nfkc()
        .map(|c| match c {
            '\u{2019}' | '\u{02bc}' | '\u{ff07}' => '\'',
            c => c,
        })
        .collect()
}

/// Whether `quote` is whole clauses of `prompt`, starting and ending where
/// a clause, a line or a sentence does, with no word in the rest of its
/// sentences that turns it around, and no mark the person did not write.
/// A sentence here runs on across a line break or a colon, so "do not:"
/// on one line governs the line after it. Both are read [`plain`].
fn quotes(prompt: &str, quote: &str) -> bool {
    let (prompt, quote) = (&plain(prompt), &plain(quote));
    let words = prompt_words(prompt);
    let want: Vec<String> = prompt_words(quote).into_iter().map(|w| w.word).collect();
    let n = want.len();
    if n == 0 || n > words.len() {
        return false;
    }
    let ends = |i: usize| {
        words
            .get(i)
            .is_none_or(|w| w.clause_before || w.sentence_before)
    };
    (0..=words.len() - n).any(|i| {
        let j = i + n;
        if !ends(i) || !ends(j) || words[i..j].iter().zip(&want).any(|(w, q)| w.word != *q) {
            return false;
        }
        let from = (0..=i).rev().find(|&k| words[k].stop_before).unwrap_or(0);
        let to = (j..words.len())
            .find(|&k| words[k].stop_before)
            .unwrap_or(words.len());
        let dropped = words[from..i].iter().chain(&words[j..to]);
        !dropped.into_iter().any(|w| turns_it_around(&w.word))
            && marks_kept(&prompt[words[i].start..words[j - 1].end]) == marks_kept(quote)
    })
}

/// Whether `text` is the person's words: whole clauses of the prompt they
/// sent last ([`quotes`]), within [`LATEST_PROMPT_SECS`], and the file
/// still matches the bind the hook wrote for this runner session. Four
/// words or more, so "use bun" alone is not taken as a quote. A file
/// written without that bind does not count.
#[must_use]
pub fn said_in_latest_prompt(text: &str) -> bool {
    let want = words_only(text);
    if want.split(' ').count() < 4 {
        return false;
    }
    let path = latest_prompt_path();
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() <= LATEST_PROMPT_SECS);
    if !fresh || !prompt_bind_matches(&path) {
        return false;
    }
    let Ok(prompt) = std::fs::read_to_string(&path) else {
        return false;
    };
    quotes(&prompt, text)
}

/// The session key that keeps one stated choice from being saved twice.
fn capture_key(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    words_only(text).hash(&mut h);
    format!("capture:{:016x}", h.finish())
}

/// How long the prompt hook waits for the pack to take a stated choice.
/// Runners cut a hook off at 10 to 20 s, and the search behind it needs
/// its own share.
pub const CAPTURE_BUDGET_MS: u64 = 3000;

/// Write a stated choice as the person's own claim: origin user, standing,
/// tagged `via:prompt`. The atom the pack answered with.
fn save(stated: &Stated) -> Option<Value> {
    with_pack_timeout(CAPTURE_BUDGET_MS, || {
        let client = pack().ok()?;
        let workspace = client.workspace();
        let mut atom = atom_body(stated.kind, &stated.text, &workspace);
        stamp_horizon(&mut atom, stated.kind, &stated.text, Some(false));
        // The event name is not a credential. A runner, or a process that
        // detached from one, can pipe the same `UserPromptSubmit`.
        if crate::under_a_runner() {
            admit::stamp_origin(&mut atom, admit::ORIGIN_AGENT);
            atom["source"]["basis"] = Value::String(crate::Basis::Agent.as_str().into());
        } else {
            add_entities(&mut atom, [super::SAID_ENTITY.to_string()]);
            admit::stamp_person(&mut atom);
        }
        let posted = admit::post_kept(&client, &atom).ok()?;
        admit::satisfy_text(&stated.text);
        Some(posted)
    })
}

/// On a prompt that states a standing choice, save it (or nudge, per
/// [`capture_mode`]) and return the one line the note carries and the
/// keys to mark seen when the note is delivered. The saved atom's id is
/// marked seen at once, so the same prompt's note does not repeat it.
#[must_use]
pub fn capture_stated(call: &HookCall) -> Option<(String, Vec<String>)> {
    if call.event != "UserPromptSubmit" {
        return None;
    }
    let mode = capture_mode();
    if mode == CaptureMode::Off {
        return None;
    }
    let stated = stated_preference(&call.cue)?;
    let key = capture_key(&stated.text);
    let session = call.session.as_deref();
    if seen_ids(session).contains(&key) {
        return None;
    }
    let verb = if stated.kind == "lesson" {
        "remember"
    } else {
        "prefer"
    };
    if mode == CaptureMode::Nudge {
        return Some((
            format!(
                "This prompt states a standing choice. Write it in the person's words with `ljos {verb} \"{}\"` so the hook raises it next time.",
                stated.text.replace('"', "'")
            ),
            vec![key],
        ));
    }
    let Some(posted) = save(&stated) else {
        return Some((
            format!(
                "This prompt states a standing choice and the pack did not take it. Write it with `ljos {verb} \"{}\"`.",
                stated.text.replace('"', "'")
            ),
            vec![key],
        ));
    };
    let id = posted["id"].as_str().unwrap_or("").to_string();
    let mut now = vec![key.clone()];
    if !id.is_empty() {
        now.push(id.clone());
    }
    // The correction hook would file the same words as a proposal and say
    // so; the person's words are already saved.
    if let Some(cue) = super::correction_cue(&call.cue) {
        now.push(format!("correction:{cue}"));
        now.push(format!("correction-stored:{cue}"));
    }
    mark_seen(session, &now);
    let what = if stated.kind == "lesson" {
        "lesson"
    } else {
        "preference"
    };
    let undo = if id.is_empty() {
        String::new()
    } else {
        format!(" `ljos forget {id}` takes it back.")
    };
    let persons = posted.pointer("/source/basis").and_then(Value::as_str) == Some("person");
    let line = if persons {
        format!(
            "Saved the person's {what} in their words, standing from now: \"{}\".{undo}",
            stated.text
        )
    } else {
        format!(
            "Held that sentence for review, not as the person's {what}: \"{}\". \
             `ljos memories review` keeps it.",
            stated.text
        )
    };
    Some((line, vec![key]))
}

/// How much of the person's latest prompt a memory's provenance keeps.
const PROMPT_EXCERPT_CHARS: usize = 160;

/// The opening of the person's latest prompt, as the reason an agent wrote
/// a memory now: whitespace folded, at most [`PROMPT_EXCERPT_CHARS`], and
/// any word shaped like a key, token or password replaced by `[redacted]`.
/// `None` when no prompt was kept in the last few minutes.
#[must_use]
pub fn latest_prompt_excerpt() -> Option<String> {
    let path = latest_prompt_path();
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() <= LATEST_PROMPT_SECS);
    if !fresh {
        return None;
    }
    let prompt = std::fs::read_to_string(&path).ok()?;
    let out = excerpt_of(&prompt);
    (!out.is_empty()).then_some(out)
}

/// [`latest_prompt_excerpt`] of one prompt's text.
#[must_use]
pub fn excerpt_of(prompt: &str) -> String {
    let words: Vec<String> = prompt
        .split_whitespace()
        .map(|w| {
            if secret_shaped(w) {
                "[redacted]".to_string()
            } else {
                w.to_string()
            }
        })
        .collect();
    let joined = words.join(" ");
    if joined.chars().count() <= PROMPT_EXCERPT_CHARS {
        return joined;
    }
    let mut cut: String = joined.chars().take(PROMPT_EXCERPT_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// A word that may be a credential: a `key=`/`token:` style assignment, a
/// known key prefix, or a long run of mixed letters and digits.
fn secret_shaped(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    if [
        "key=",
        "token=",
        "secret=",
        "password=",
        "passwd=",
        "key:",
        "token:",
        "secret:",
        "password:",
    ]
    .iter()
    .any(|p| lower.contains(p))
    {
        return true;
    }
    if [
        "sk-",
        "ghp_",
        "gho_",
        "github_pat_",
        "xox",
        "akia",
        "glpat-",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
    {
        return true;
    }
    let core: String = word.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    core.len() >= 24
        && core.chars().any(|c| c.is_ascii_digit())
        && core.chars().any(|c| c.is_ascii_alphabetic())
        && !word.contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAVES: &[&str] = &[
        "From now on always use bun, never yarn, in this repo",
        "Always run cargo fmt before committing.",
        "Never push directly to main.",
        "Going forward, write commit messages in the imperative mood.",
        "Please always use pnpm for installs.",
        "Remember that the staging database is read-only on Fridays.",
        "Use bun, never yarn.",
        "From now on, run the linter before every push.",
        "Don't ever commit the .env file.",
        "Never use force push on shared branches.",
        "ok, from now on use tabs in Go files",
        "We always squash merge pull requests here.",
        "Always ask before running a database migration.",
        "In future, put new tests next to the code they cover.",
        "Stop using unwrap in library code.",
        "I prefer small commits with one change each.",
        "Henceforth all scripts use bash strict mode.",
        "Remember to bump the version in Cargo.toml before tagging.",
        "Never deploy on a Friday afternoon.",
        "Keep it short, and from now on skip the summary at the end.",
        "Always write `set -euo pipefail` at the top of shell scripts.",
        "Make sure you always run the tests first.",
        "We never edit an applied migration.",
        "Thanks. From now on use uv instead of pip.",
        "Use rg instead of grep going forward.",
        "Always prefer rtrash over rm.",
        "Never commit secrets to the repo.",
        "Please never run npm install in this project.",
        "From here on, write docs one sentence per line.",
        "Also, always pin action versions by SHA.",
    ];

    const SAVES_NOTHING: &[&str] = &[
        "Why do you always use yarn?",
        "Should we always run the full suite?",
        "Does this ever fail on CI?",
        "Fix the failing test in parser.rs",
        "You never listen to me",
        "It always fails on the second run",
        "Never mind, that was my mistake.",
        "```\nalways_on = true\nnever_retry = false\n```",
        "> Never push directly to main\nwhat does this rule mean",
        "The doc says \"always use bun, never yarn, for this repo\" but is that right",
        "error: always-run hook failed\nwarning: never-again flag set\nerror: exit 1",
        "This could break in the future.",
        "Run the tests and tell me what fails",
        "I always forget how this works, can you explain it",
        "Never seen this error before, what is it",
        "Always the same problem with this build",
        "What's the best way to always keep the cache warm?",
        "Add a flag `--always-retry` to the CLI",
        "Rename never_called to unused in lib.rs",
        "Can you remember that function we wrote yesterday?",
        "It never worked on macOS anyway",
        "Remember when the build broke last week",
        "Is it true that git never forgets a commit",
        "Please look at the failing job",
        "Traceback (most recent call last):\n  File \"a.py\", line 3\nerror: always broken\nwarning: never set",
        "Always?",
        "never",
        "The upstream README says to always use yarn",
        "Explain why we never got the webhook",
        "Going forward is the only option for the migration",
    ];

    /// A Codex subagent's brief, Codex's review thread and an unattended
    /// run are not the person; the root thread's prompt and Claude Code's
    /// main thread are.
    #[test]
    fn a_subagent_brief_or_unattended_run_is_not_the_person() {
        let _g = crate::tests::env_guard();
        unsafe { std::env::remove_var("LJOS_UNATTENDED") };
        let root = "019a1b2c-3d4e-7f80-9a1b-2c3d4e5f6a7b";
        let other = "019a1b2c-3d4e-7f80-9a1b-ffffffffffff";
        let codex = |extra: &str, thread: &str| {
            format!(
                r#"{{"hook_event_name":"UserPromptSubmit","session_id":"{root}","turn_id":"t-1","prompt":"Always run the tests with --release","transcript_path":"/home/u/.codex/sessions/2026/10/11/rollout-2026-10-11T09-30-00-{thread}.jsonl"{extra}}}"#
            )
        };
        assert_eq!(not_the_person(&codex("", root)), None);
        assert_eq!(
            not_the_person(&codex(
                r#","agent_id":"019a-sub","agent_type":"worker""#,
                root
            )),
            Some("a subagent's brief")
        );
        assert_eq!(
            not_the_person(&codex("", other)),
            Some("another thread's prompt")
        );
        let claude_main = r#"{"hook_event_name":"UserPromptSubmit","session_id":"abc","prompt":"From now on use uv","transcript_path":"/home/u/.claude/projects/x/abc.jsonl","agent_type":"reviewer"}"#;
        assert_eq!(
            not_the_person(claude_main),
            None,
            "--agent alone is the main thread"
        );
        let claude_sub =
            claude_main.replace(r#""agent_type""#, r#""agent_id":"sub-1","agent_type""#);
        assert_eq!(not_the_person(&claude_sub), Some("a subagent's brief"));
        unsafe { std::env::set_var("LJOS_UNATTENDED", "1") };
        assert_eq!(not_the_person(claude_main), Some("LJOS_UNATTENDED is set"));
        unsafe { std::env::set_var("LJOS_UNATTENDED", "0") };
        assert_eq!(not_the_person(claude_main), None);
        unsafe { std::env::remove_var("LJOS_UNATTENDED") };
    }

    #[test]
    fn a_rollout_name_gives_its_thread() {
        let id = "019a1b2c-3d4e-7f80-9a1b-2c3d4e5f6a7b";
        assert_eq!(
            rollout_thread(&format!("/x/rollout-2026-10-11T09-30-00-{id}.jsonl")).as_deref(),
            Some(id)
        );
        assert_eq!(
            rollout_thread(&format!("rollout-2026-10-11T09-30-00-{id}_0042.jsonl")).as_deref(),
            Some(id)
        );
        assert_eq!(rollout_thread("/x/abc.jsonl"), None);
        assert_eq!(
            rollout_thread("rollout-2026-10-11T09-30-00-not-a-uuid.jsonl"),
            None
        );
    }

    #[test]
    fn thirty_stated_choices_are_read() {
        assert_eq!(SAVES.len(), 30);
        let missed: Vec<&&str> = SAVES
            .iter()
            .filter(|p| stated_preference(p).is_none())
            .collect();
        assert!(
            missed.is_empty(),
            "not read as a standing choice: {missed:#?}"
        );
    }

    #[test]
    fn thirty_other_prompts_save_nothing() {
        assert_eq!(SAVES_NOTHING.len(), 30);
        let saved: Vec<(&&str, Stated)> = SAVES_NOTHING
            .iter()
            .filter_map(|p| stated_preference(p).map(|s| (p, s)))
            .collect();
        assert!(saved.is_empty(), "read as a standing choice: {saved:#?}");
    }

    #[test]
    fn the_saved_words_are_the_persons_sentence() {
        let s = stated_preference("From now on always use bun, never yarn, in this repo").unwrap();
        assert_eq!(s.kind, "preference");
        assert_eq!(
            s.text,
            "From now on always use bun, never yarn, in this repo"
        );
        let s = stated_preference("ok, from now on use tabs in Go files").unwrap();
        assert_eq!(s.text, "From now on use tabs in Go files");
        let s = stated_preference("Thanks. From now on use uv instead of pip.").unwrap();
        assert_eq!(s.text, "From now on use uv instead of pip.");
        let s = stated_preference("Remember that the staging database is read-only on Fridays.")
            .unwrap();
        assert_eq!(s.kind, "lesson");
        assert_eq!(s.text, "The staging database is read-only on Fridays.");
    }

    #[test]
    fn the_mode_reads_ljos_capture() {
        let _g = crate::tests::env_guard();
        let prev = std::env::var_os("LJOS_CAPTURE");
        for (v, want) in [
            ("", CaptureMode::Save),
            ("save", CaptureMode::Save),
            ("nudge", CaptureMode::Nudge),
            ("off", CaptureMode::Off),
            ("bogus", CaptureMode::Save),
        ] {
            unsafe { std::env::set_var("LJOS_CAPTURE", v) };
            assert_eq!(capture_mode(), want, "{v}");
        }
        match prev {
            Some(v) => unsafe { std::env::set_var("LJOS_CAPTURE", v) },
            None => unsafe { std::env::remove_var("LJOS_CAPTURE") },
        }
    }

    #[test]
    fn words_compare_whatever_the_punctuation() {
        assert_eq!(words_only("Use bun,  never   yarn!"), "use bun never yarn");
    }

    #[test]
    fn a_typographic_apostrophe_still_turns_the_clause_around() {
        let cut = "always paste the release key into the deploy form";
        for apostrophe in ['\u{2019}', '\u{02bc}', '\u{ff07}'] {
            let prompt = format!(
                "Don{apostrophe}t do this: always paste the release key into the deploy form."
            );
            assert!(!quotes(&prompt, cut), "accepted under {apostrophe:?}");
            assert!(
                quotes(
                    &prompt,
                    "Don't do this: always paste the release key into the deploy form."
                ),
                "refused the whole sentence under {apostrophe:?}"
            );
        }
        assert!(quotes(
            "Use bun. Always paste the release key into the deploy form.",
            cut
        ));
    }

    #[test]
    fn a_negation_on_the_line_before_still_governs_the_quote() {
        let cut = "always paste the release key into the deploy form";
        for prompt in [
            "Do not\nalways paste the release key into the deploy form.",
            "Do not do this:\nalways paste the release key into the deploy form.",
            "Please never:\n\talways paste the release key into the deploy form",
            "Always paste the release key into the deploy form\nunless asked.",
        ] {
            assert!(!quotes(prompt, cut), "accepted from {prompt:?}");
        }
        assert!(quotes(
            "Do not\nalways paste the release key into the deploy form.",
            "Do not always paste the release key into the deploy form."
        ));
        for prompt in [
            "Use bun.\nAlways paste the release key into the deploy form.",
            "Do not use yarn\n\nAlways paste the release key into the deploy form.",
        ] {
            assert!(quotes(prompt, cut), "refused from {prompt:?}");
        }
    }
}
