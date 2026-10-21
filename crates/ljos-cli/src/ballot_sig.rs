//! Signed seat ballots. A seat signs each ballot it casts with its own
//! ed25519 key, made on its first vote at
//! `$XDG_DATA_HOME/ljos/seats/SEAT/ballot.key` (or `LJOS_SEAT_KEY`), so
//! seats sharing one HOME still sign apart. The signature rides on the
//! issue as a `ballot-sig:` note. A ballot the tracker committed before
//! signing began ([`SIGNING_SINCE`]) is legacy unsigned and still counts. The tracker
//! pins each seat's public key the first time it signs (`.ljos-seat-keys`
//! at the tracker root, committed), so a later ballot under that name signed
//! by another key is a mismatch. The push gate counts a seat ballot only
//! when its latest signature covers exactly that voter, issue and choice
//! under the pinned key.
//!
//! A ballot carries the harness session and a nonce (`ljos-ballot/v2`).
//! The session is the harness id, else `VISSUE_AGENT`, else `LJOS_SEAT`,
//! else the MCP connection. It is never a fresh nonce of this process.
//! Minting another seat inside the same session does not make a second voter.

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The note prefix a signature rides under.
pub const SIG_NOTE: &str = "ballot-sig: ";
/// The note prefix a withdrawn ballot rides under.
pub const WITHDRAWN_NOTE: &str = "ballot-withdrawn: ";
/// The pin file at the tracker root.
pub const PIN_FILE: &str = ".ljos-seat-keys";

/// The bytes a ballot signature covers. v1 is the choice alone. v2 also
/// covers the harness session and the process nonce, so a later edit of
/// either fails the check.
#[must_use]
pub fn message(issue: &str, voter: &str, choice: &str) -> Vec<u8> {
    format!(
        "ljos-ballot/v1\n{}\n{}\n{}\n",
        issue.trim(),
        voter.trim(),
        choice.trim()
    )
    .into_bytes()
}

/// [`message`] plus the session binding.
#[must_use]
pub fn message_v2(issue: &str, voter: &str, choice: &str, harness: &str, nonce: &str) -> Vec<u8> {
    format!(
        "ljos-ballot/v2\n{}\n{}\n{}\n{}\n{}\n",
        issue.trim(),
        voter.trim(),
        choice.trim(),
        harness.trim(),
        nonce.trim()
    )
    .into_bytes()
}

/// A signature note's fields. `harness` and `nonce` are set when the note
/// is a v2 binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigNote {
    pub voter: String,
    pub choice: String,
    pub key: String,
    pub sig: String,
    pub harness: Option<String>,
    pub nonce: Option<String>,
}

impl SigNote {
    /// Whether only the seat named the session this ballot is bound to.
    #[must_use]
    pub fn seat_only(&self) -> bool {
        self.nonce.as_deref() == Some(SEAT_NONCE)
    }
}

/// `ballot-sig: VOTER CHOICE KEYHEX SIGHEX`, or the same plus
/// `HARNESS NONCE` when the ballot is bound to a session. The choice may
/// hold spaces (`beside binary`): the key and signature are found by their
/// hex lengths, counted from the end, and the choice is the text between
/// the voter and the key. A note whose key is not 64 hex digits parses only
/// in the four- or six-field shape.
#[must_use]
pub fn parse_note(note: &str) -> Option<SigNote> {
    let rest = note.trim().strip_prefix(SIG_NOTE)?;
    let at = spans(rest);
    let n = at.len();
    if n < 4 {
        return None;
    }
    let tok = |i: usize| &rest[at[i].0..at[i].1];
    let key_at = |i: usize| is_hex_len(tok(i), 64) && is_hex_len(tok(i + 1), 128);
    // Where the key sits, and whether a session follows the signature.
    let (k, bound) = if n >= 6 && key_at(n - 4) {
        (n - 4, true)
    } else if key_at(n - 2) {
        (n - 2, false)
    } else if n == 6 {
        (2, true)
    } else if n == 4 {
        (2, false)
    } else {
        return None;
    };
    let choice = rest[at[1].0..at[k - 1].1].trim();
    Some(SigNote {
        voter: tok(0).to_string(),
        choice: choice.to_string(),
        key: tok(k).to_string(),
        sig: tok(k + 1).to_string(),
        harness: bound.then(|| tok(k + 2).to_string()),
        nonce: bound.then(|| tok(k + 3).to_string()),
    })
}

/// The session a ballot note binds, as [`crate::panel_gate::session_key`]
/// reads it. A note that does not parse but has six or more fields still
/// names its session in the last two. The cast gate does not use that
/// fallback: it requires [`parse_note`] and [`verifies`] on the issue's own
/// logbook. `None` when the note is not a ballot signature.
#[must_use]
pub fn note_session(note: &str) -> Option<(String, String)> {
    if let Some(sig) = parse_note(note) {
        return Some((
            sig.harness.unwrap_or_default(),
            sig.nonce.unwrap_or_default(),
        ));
    }
    let rest = note.trim().strip_prefix(SIG_NOTE)?;
    let at = spans(rest);
    let n = at.len();
    if n < 6 {
        return None;
    }
    let tok = |i: usize| rest[at[i].0..at[i].1].to_string();
    Some((tok(n - 2), tok(n - 1)))
}

/// A withdrawn ballot: `ballot-withdrawn: VOTER HARNESS NONCE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnNote {
    pub voter: String,
    pub harness: String,
    pub nonce: String,
}

/// One token per field, so the note stays three words.
#[must_use]
pub fn withdrawn_line(voter: &str, harness: &str, nonce: &str) -> String {
    format!(
        "{WITHDRAWN_NOTE}{} {} {}",
        field(voter),
        field(harness),
        field(nonce)
    )
}

/// `ballot-withdrawn: VOTER HARNESS NONCE`, or `None` when the note is not
/// that shape.
#[must_use]
pub fn parse_withdrawn(note: &str) -> Option<WithdrawnNote> {
    let rest = note.trim().strip_prefix(WITHDRAWN_NOTE)?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let [voter, harness, nonce] = parts.as_slice() else {
        return None;
    };
    Some(WithdrawnNote {
        voter: (*voter).to_string(),
        harness: (*harness).to_string(),
        nonce: (*nonce).to_string(),
    })
}

/// Byte ranges of the whitespace-separated fields of `text`.
fn spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(b) = start.take() {
                out.push((b, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(b) = start {
        out.push((b, text.len()));
    }
    out
}

fn is_hex_len(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn key_from_file(path: &std::path::Path) -> Result<SigningKey> {
    let raw = std::fs::read(path).with_context(|| format!("seat key {}", path.display()))?;
    let seed: [u8; 32] = raw
        .get(..32)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| anyhow::anyhow!("seat key {} holds fewer than 32 bytes", path.display()))?;
    Ok(SigningKey::from_bytes(&seed))
}

/// The commit time of the signing change: a ballot the tracker recorded
/// before it is legacy unsigned and still counts; one after needs a
/// signature.
pub const SIGNING_SINCE: i64 = 1_791_984_778;

/// Where `seat`'s ballot key lives: `LJOS_SEAT_KEY` when that path is this
/// seat's own key and not the machine key, else
/// `$XDG_DATA_HOME/ljos/seats/SEAT/ballot.key` (`~/.local/share` when unset).
/// Seats that share one HOME still get one key each. The machine key at
/// `~/.config/deedar/host.key` is never used.
#[must_use]
pub fn seat_key_path(seat: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LJOS_SEAT_KEY").filter(|p| !p.is_empty()) {
        let path = PathBuf::from(p);
        if !is_box_host_key(&path) && seat_may_use_override(seat) {
            return Some(path);
        }
    }
    seat_key_file(seat)
}

/// The per-seat key file, ignoring `LJOS_SEAT_KEY`.
#[must_use]
pub fn seat_key_file(seat: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let name: String = seat
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    Some(
        base.join("ljos")
            .join("seats")
            .join(name)
            .join("ballot.key"),
    )
}

/// The machine deed key. A seat key must not be this file.
#[must_use]
pub fn is_box_host_key(path: &std::path::Path) -> bool {
    path.ends_with("deedar/host.key")
}

fn seat_may_use_override(seat: &str) -> bool {
    let seat = seat.trim();
    let own = |key: &str| std::env::var(key).ok().is_some_and(|v| v.trim() == seat);
    match (
        std::env::var("LJOS_SEAT")
            .ok()
            .filter(|s| !s.trim().is_empty()),
        std::env::var("VISSUE_AGENT")
            .ok()
            .filter(|s| !s.trim().is_empty()),
    ) {
        (None, None) => true,
        _ => own("LJOS_SEAT") || own("VISSUE_AGENT"),
    }
}

/// The key at `path`, made on first use: 32 random bytes, mode 0600.
///
/// # Errors
///
/// The key cannot be read or written.
pub fn load_or_create(path: &std::path::Path) -> Result<SigningKey> {
    if !path.exists() {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let mut seed = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut seed)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("seat key {}", path.display()))?;
        std::io::Write::write_all(&mut f, &seed)?;
    }
    key_from_file(path)
}

/// When the tracker recorded `voter`'s ballot for `choice` on `issue`: the
/// newest commit that added or removed its `] VOTER: CHOICE` line in the
/// file holding the issue. `None` when the tracker is not a git repository
/// or the ballot is not committed yet.
#[must_use]
pub fn recorded_at(issue: &str, voter: &str, choice: &str) -> Option<i64> {
    let root = std::env::var_os("VISSUE_ROOT").filter(|r| !r.is_empty())?;
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let pattern = format!(":ID:[[:space:]]+{issue}[[:space:]]*$");
    let files = git(&["grep", "-l", "-E", &pattern])?;
    let file = files.lines().next()?.to_string();
    let needle = format!("] {}: {}", voter.trim(), choice.trim());
    git(&["log", "-1", "--format=%ct", "-S", &needle, "--", &file])?
        .parse()
        .ok()
}

/// The note text for a ballot signed by `key` (v1, choice only).
#[must_use]
pub fn sign_with(key: &SigningKey, issue: &str, voter: &str, choice: &str) -> String {
    let sig = key.sign(&message(issue, voter, choice));
    format!(
        "{SIG_NOTE}{} {} {} {}",
        voter.trim(),
        choice.trim(),
        hex::encode(key.verifying_key().to_bytes()),
        hex::encode(sig.to_bytes())
    )
}

/// The note text for a ballot bound to a harness session and a process nonce.
#[must_use]
pub fn sign_bound(
    key: &SigningKey,
    issue: &str,
    voter: &str,
    choice: &str,
    harness: &str,
    nonce: &str,
) -> String {
    let harness = field(harness);
    let nonce = field(nonce);
    let sig = key.sign(&message_v2(issue, voter, choice, &harness, &nonce));
    format!(
        "{SIG_NOTE}{} {} {} {} {} {}",
        voter.trim(),
        choice.trim(),
        hex::encode(key.verifying_key().to_bytes()),
        hex::encode(sig.to_bytes()),
        harness,
        nonce
    )
}

fn covered(issue: &str, note: &SigNote) -> Vec<u8> {
    match (&note.harness, &note.nonce) {
        (Some(h), Some(n)) => message_v2(issue, &note.voter, &note.choice, h, n),
        _ => message(issue, &note.voter, &note.choice),
    }
}

/// Whether `note` is a valid signature over this issue's ballot.
#[must_use]
pub fn verifies(issue: &str, note: &SigNote) -> bool {
    let (Ok(k), Ok(s)) = (hex::decode(&note.key), hex::decode(&note.sig)) else {
        return false;
    };
    let (Ok(k), Ok(s)) = (<[u8; 32]>::try_from(k), <[u8; 64]>::try_from(s)) else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_bytes(&k) else {
        return false;
    };
    vk.verify(&covered(issue, note), &Signature::from_bytes(&s))
        .is_ok()
}

/// The tracker root the pins live at.
fn pin_path() -> Option<PathBuf> {
    let root = std::env::var_os("VISSUE_ROOT").filter(|r| !r.is_empty())?;
    Some(PathBuf::from(root).join(PIN_FILE))
}

/// `voter keyhex` lines.
#[must_use]
pub fn parse_pins(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

/// The pinned keys, empty when there is no pin file.
#[must_use]
pub fn pins() -> BTreeMap<String, String> {
    pin_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_pins(&t))
        .unwrap_or_default()
}

/// A token safe to put in a signature note: no whitespace.
#[must_use]
pub fn field(raw: &str) -> String {
    let t: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '@') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if t.is_empty() {
        "-".into()
    } else {
        t
    }
}

/// The harness session, and a nonce that is not a process id. The session
/// comes from the MCP connection, the hook, or the environment, then from
/// `VISSUE_AGENT` or `LJOS_SEAT`. The nonce is `-`, or [`SEAT_NONCE`] when
/// the seat stands in for the session: one holder is one session across
/// processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentity {
    pub harness: String,
    pub nonce: String,
}

/// The nonce of a session only the seat names: no harness session and no
/// run id was found. [`crate::panel_gate::session_key`] reads the harness
/// alone when one is set, so the key is still the seat.
pub const SEAT_NONCE: &str = "seat";

impl SessionIdentity {
    /// Whether only the seat names this session.
    #[must_use]
    pub fn seat_only(&self) -> bool {
        self.nonce == SEAT_NONCE
    }
}

/// What a session only the seat names means, where a ballot is cast or
/// counted.
#[must_use]
pub fn unverified_sentence(seat: &str) -> String {
    format!(
        "unverified session: nothing named the run but seat {seat}, so every run of that seat is one session"
    )
}

tokio::task_local! {
    static CALL_SESSION: String;
    static CALL_AGENT: String;
}

std::thread_local! {
    static HOOK_SESSION: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The session id the MCP call named, when this task is inside one.
#[must_use]
pub fn call_session() -> Option<String> {
    CALL_SESSION
        .try_with(|s| s.clone())
        .ok()
        .filter(|s| !s.is_empty())
}

/// Run `f` as an MCP call that named `session`. One server answers a
/// parent and all its subagents, so the call is the parent's, whatever
/// this process holds.
pub async fn scope_session<F: std::future::Future>(session: Option<String>, f: F) -> F::Output {
    let f = CALL_AGENT.scope(String::new(), f);
    match session.map(|s| field(&s)).filter(|s| s != "-") {
        Some(s) => CALL_SESSION.scope(s, f).await,
        None => f.await,
    }
}

static PROCESS_AGENT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// Name the subagent a hook payload says this hook process runs for.
pub fn hook_agent(agent: Option<String>) {
    let _ = PROCESS_AGENT.set(agent);
}

/// The subagent this process or MCP call runs for under `parent`, when the
/// harness named one: the agent a hook saw make this call. The command
/// line takes no agent, so a parent cannot cast as its subagent.
fn subagent(parent: &str) -> Option<String> {
    if let Ok(agent) = CALL_AGENT.try_with(Clone::clone) {
        return (!agent.is_empty()).then_some(agent);
    }
    PROCESS_AGENT
        .get_or_init(|| {
            crate::subagent::claim_command(parent)
                .filter(|a| !a.is_empty())
                .map(|a| field(&a))
        })
        .clone()
}

/// Remember the hook payload's session for this process, and export it for
/// a child when the environment has not already named one.
pub fn export_hook_session(id: Option<&str>) {
    crate::with_env_mutation(|| {
        let id = id.map(str::trim).filter(|s| !s.is_empty()).map(field);
        HOOK_SESSION.with(|c| *c.borrow_mut() = id.clone());
        if let Some(id) = id {
            if std::env::var("LJOS_HARNESS_SESSION")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .is_none()
            {
                std::env::set_var("LJOS_HARNESS_SESSION", id);
            }
        }
    });
}

fn env_token(key: &str) -> Option<String> {
    let v = std::env::var(key).ok()?;
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    Some(field(v))
}

static CONNECTION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Pin the MCP connection's session for this process. The first call wins.
/// A later `_meta` session id and a later `seat` argument do not replace it.
/// With no hint, the runner's `LJOS_HARNESS_SESSION` is the connection.
/// Nothing is pinned when that is empty: a per-process nonce is not a session.
pub fn pin_connection_session(hint: Option<&str>) {
    let Some(chosen) = hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(field)
        .filter(|s| s != "-")
        .or_else(|| env_token("LJOS_HARNESS_SESSION"))
    else {
        return;
    };
    let _ = CONNECTION.set(chosen);
}

/// The session pinned for this MCP connection, when [`pin_connection_session`]
/// has run.
#[must_use]
pub fn connection_session() -> Option<String> {
    CONNECTION.get().cloned()
}

/// The harness session id: the MCP connection, then the MCP call, then the
/// hook, then `LJOS_HARNESS_SESSION`, then a runner `*_SESSION_ID` / thread
/// id, then the run ([`run_session`]).
/// The connection pin is the process, not the `seat` argument.
#[must_use]
pub fn harness_session() -> String {
    if let Some(s) = connection_session() {
        return s;
    }
    if let Some(s) = call_session() {
        return s;
    }
    if let Some(s) = HOOK_SESSION.with(|c| c.borrow().clone()) {
        return s;
    }
    if let Some(s) = env_token("LJOS_HARNESS_SESSION") {
        return s;
    }
    const PREFER: &[&str] = &[
        "CURSOR_SESSION_ID",
        "CLAUDE_SESSION_ID",
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
        "GROK_SESSION_ID",
        "SEAT_SESSION_ID",
        "LJOS_SESSION_ID",
        // Claude Code also stamps a remote-control id that comes and goes,
        // and opencode may run under another runner's id. Each names its
        // own session here, so the ballot binds the id its hook reports.
        "CLAUDE_CODE_SESSION_ID",
        "OPENCODE_SESSION_ID",
    ];
    for key in PREFER {
        if let Some(s) = env_token(key) {
            if s.len() >= 8 {
                return s;
            }
        }
    }
    crate::runner_session_id()
        .map(|s| field(&s))
        .or_else(run_session)
        .unwrap_or_default()
}

/// The run this process belongs to, when no conversation id was stamped:
/// a GitHub Actions run, else a cloud agent. Every process of one run is
/// one session, as `ljos cloud up` gives the run one seat.
fn run_session() -> Option<String> {
    let actions = std::env::var("GITHUB_ACTIONS").is_ok_and(|v| {
        let v = v.trim();
        !v.is_empty() && !matches!(v, "0" | "false")
    });
    let ci: &[&str] = if actions { &["GITHUB_RUN_ID"] } else { &[] };
    ci.iter()
        .chain(&["CURSOR_AGENT_ID", "CLOUD_AGENT_ID"])
        .filter_map(|key| env_token(key))
        .find(|s| s.len() >= 8)
}

fn random_hex(n: usize) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok()
    {
        return hex::encode(buf);
    }
    format!("{:x}{:x}", std::process::id(), std::process::id())
}

static PROCESS_NONCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The nonce of this process. Seats this process mints share it.
#[must_use]
pub fn process_nonce() -> String {
    if let Some(s) = env_token("LJOS_PROCESS_NONCE") {
        if s != "-" {
            return s;
        }
    }
    PROCESS_NONCE.get_or_init(|| random_hex(16)).clone()
}

/// A person-named agent or seat, when it is not a pronoun.
fn named_session_actor(key: &str) -> Option<String> {
    let name = env_token(key)?;
    let lower = name.to_ascii_lowercase();
    if matches!(lower.as_str(), "seat" | "you" | "agent" | "runner" | "-") {
        return None;
    }
    Some(name)
}

/// This process's session identity. Stable across processes of one holder.
/// A subagent the harness named is `SESSION:AGENT`, a session of its own.
#[must_use]
pub fn identity() -> SessionIdentity {
    let harness = harness_session();
    if !harness.is_empty() {
        let harness = match subagent(&harness) {
            Some(agent) => format!("{harness}:{agent}"),
            None => harness,
        };
        return SessionIdentity {
            harness,
            nonce: "-".into(),
        };
    }
    match named_session_actor("VISSUE_AGENT").or_else(|| named_session_actor("LJOS_SEAT")) {
        Some(seat) => SessionIdentity {
            harness: seat,
            nonce: SEAT_NONCE.into(),
        },
        None => SessionIdentity {
            harness: "-".into(),
            nonce: "-".into(),
        },
    }
}

/// The key `voter` signs with, pinned on first use.
fn signing_key(voter: &str) -> Result<(SigningKey, String)> {
    let path = seat_key_path(voter)
        .ok_or_else(|| anyhow::anyhow!("no place for a seat key: HOME and XDG_DATA_HOME are unset; the ballot is unsigned and no gate counts it"))?;
    let key = load_or_create(&path)?;
    let mine = hex::encode(key.verifying_key().to_bytes());
    let mut pinned = pins();
    match pinned.get(voter.trim()) {
        Some(k) if *k != mine => bail!(
            "seat {voter} is pinned to key {}…, and this key is {}…; the ballot is unsigned and no gate counts it",
            &k[..12.min(k.len())],
            &mine[..12]
        ),
        Some(_) => {}
        None => {
            if let Some(p) = pin_path() {
                pinned.insert(voter.trim().to_string(), mine.clone());
                let text: String = pinned.iter().map(|(v, k)| format!("{v} {k}\n")).collect();
                std::fs::write(&p, text)?;
                if let Some(root) = p.parent() {
                    let _ = std::process::Command::new("git")
                        .arg("-C")
                        .arg(root)
                        .args(["add", PIN_FILE])
                        .output();
                }
            }
        }
    }
    Ok((key, mine))
}

/// Sign this seat's ballot, bound to the harness session, and leave the
/// note on the issue. A seat whose key differs from
/// its pin is refused, and the ballot stays unsigned (so the gate will not
/// count it).
///
/// # Errors
///
/// No seat key, a key that does not match the pin, or a tracker write that
/// failed.
pub fn sign_ballot(issue: &str, voter: &str, choice: &str) -> Result<String> {
    if let Some(existing) = newest_sig(issue, voter) {
        if verifies(issue, &existing) {
            match resign_of(&existing, choice, voter) {
                Resign::Keep => {
                    return Ok(format!("ballot already signed by {voter}\n"));
                }
                Resign::Refuse => {
                    bail!("will not re-sign {voter}'s ballot");
                }
                Resign::Sign => {}
            }
        }
    }
    let (key, mine) = signing_key(voter)?;
    let id = identity();
    let note = sign_bound(&key, issue, voter, choice, &id.harness, &id.nonce);
    if write_sig_note(issue, &note).is_err() {
        crate::run_captured("vissue", &["note", issue, &note])?;
    }
    let session = if id.harness == "-" {
        format!("nonce {}", &id.nonce[..12.min(id.nonce.len())])
    } else {
        id.harness.clone()
    };
    let mut said = format!(
        "ballot signed by {voter} with key {}… session {session}\n",
        &mine[..12.min(mine.len())]
    );
    if id.seat_only() {
        said.push_str(&format!("{}\n", unverified_sentence(&id.harness)));
    }
    Ok(said)
}

enum Resign {
    Keep,
    Refuse,
    Sign,
}

/// A signature that already verifies stays. The same key may sign a new
/// choice. Any other key, including the machine key, does not sign again.
fn resign_of(existing: &SigNote, choice: &str, voter: &str) -> Resign {
    let mine = current_public_hex(voter);
    if mine.as_deref() == Some(existing.key.as_str()) {
        if existing.choice == choice {
            Resign::Keep
        } else {
            Resign::Sign
        }
    } else {
        Resign::Refuse
    }
}

fn current_public_hex(voter: &str) -> Option<String> {
    let path = seat_key_path(voter)?;
    let key = key_from_file(&path).ok()?;
    Some(hex::encode(key.verifying_key().to_bytes()))
}

fn newest_sig(issue: &str, voter: &str) -> Option<SigNote> {
    let layout = vissue_core::Layout::resolve(None, None).ok()?;
    let (heading, _, _) = vissue_core::store::find_by_id(&layout, issue).ok()??;
    heading.logbook.iter().find_map(|entry| {
        let text = entry.note.as_deref()?;
        let sig = parse_note(text)?;
        (sig.voter == voter).then_some(sig)
    })
}

fn write_sig_note(issue: &str, note: &str) -> Result<()> {
    let layout = vissue_core::Layout::resolve(None, None)?;
    vissue_core::ops::note(&layout, issue, note)?;
    Ok(())
}

/// Sign the ballot this process's seat cast on `issue`, when it has one.
///
/// # Errors
///
/// The tracker cannot be read, or the ballot is already signed by another key.
pub fn sign_own_ballot(issue: &str) -> Result<String> {
    let voter = std::env::var("VISSUE_AGENT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("LJOS_SEAT")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    let Some(voter) = voter else {
        return Ok(String::new());
    };
    let layout = vissue_core::Layout::resolve(None, None)?;
    let ballots = vissue_core::ops::ballots(&layout, issue)?;
    let Some(choice) = ballots
        .iter()
        .find(|b| b.agent == voter)
        .map(|b| b.choice.clone())
    else {
        return Ok(String::new());
    };
    sign_ballot(issue, &voter, &choice)
}

/// Leave `ballot-withdrawn: VOTER HARNESS NONCE` on `issue` for this session.
///
/// # Errors
///
/// The tracker did not take the note.
pub fn note_withdrawn(issue: &str, voter: &str) -> Result<String> {
    let id = identity();
    let note = withdrawn_line(voter, &id.harness, &id.nonce);
    crate::run_captured("vissue", &["note", issue, &note])
        .with_context(|| format!("ballot withdrawn; the note on {issue} was not written"))?;
    Ok(note)
}

/// The note prefix a design candidate's signature rides under.
pub const CANDIDATE_NOTE: &str = "candidate-sig: ";

/// The bytes a candidate signature covers, including which other candidates
/// this session had already read.
#[must_use]
pub fn candidate_message(
    issue: &str,
    seat: &str,
    harness: &str,
    nonce: &str,
    saw: &str,
) -> Vec<u8> {
    format!(
        "ljos-candidate/v1\n{}\n{}\n{}\n{}\n{}\n",
        issue.trim(),
        seat.trim(),
        harness.trim(),
        nonce.trim(),
        saw.trim()
    )
    .into_bytes()
}

/// A signed candidate binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateNote {
    pub issue: String,
    pub seat: String,
    pub harness: String,
    pub nonce: String,
    pub saw: Vec<String>,
    pub key: String,
    pub sig: String,
}

fn saw_token(saw: &[String]) -> String {
    if saw.is_empty() {
        "-".into()
    } else {
        saw.iter().map(|s| field(s)).collect::<Vec<_>>().join(",")
    }
}

fn saw_list(token: &str) -> Vec<String> {
    if token.is_empty() || token == "-" {
        Vec::new()
    } else {
        token
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "-")
            .map(str::to_string)
            .collect()
    }
}

/// `candidate-sig: ISSUE SEAT HARNESS NONCE SAW KEYHEX SIGHEX`.
#[must_use]
pub fn parse_candidate(note: &str) -> Option<CandidateNote> {
    let rest = note.trim().strip_prefix(CANDIDATE_NOTE)?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let [issue, seat, harness, nonce, saw, key, sig] = parts.as_slice() else {
        return None;
    };
    Some(CandidateNote {
        issue: (*issue).to_string(),
        seat: (*seat).to_string(),
        harness: (*harness).to_string(),
        nonce: (*nonce).to_string(),
        saw: saw_list(saw),
        key: (*key).to_string(),
        sig: (*sig).to_string(),
    })
}

/// Whether `note` is a valid signature over that candidate binding.
#[must_use]
pub fn candidate_verifies(note: &CandidateNote) -> bool {
    let (Ok(k), Ok(s)) = (hex::decode(&note.key), hex::decode(&note.sig)) else {
        return false;
    };
    let (Ok(k), Ok(s)) = (<[u8; 32]>::try_from(k), <[u8; 64]>::try_from(s)) else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_bytes(&k) else {
        return false;
    };
    vk.verify(
        &candidate_message(
            &note.issue,
            &note.seat,
            &note.harness,
            &note.nonce,
            &saw_token(&note.saw),
        ),
        &Signature::from_bytes(&s),
    )
    .is_ok()
}

/// Sign `issue` as a design candidate of `seat` and return the note.
///
/// # Errors
///
/// No seat key, or a key that does not match the pin.
pub fn sign_candidate(
    issue: &str,
    seat: &str,
    id: &SessionIdentity,
    saw: &[String],
) -> Result<String> {
    let (key, _) = signing_key(seat)?;
    let harness = field(&id.harness);
    let nonce = field(&id.nonce);
    let saw_tok = saw_token(saw);
    let sig = key.sign(&candidate_message(issue, seat, &harness, &nonce, &saw_tok));
    Ok(format!(
        "{CANDIDATE_NOTE}{} {} {} {} {} {} {}",
        field(issue),
        field(seat),
        harness,
        nonce,
        saw_tok,
        hex::encode(key.verifying_key().to_bytes()),
        hex::encode(sig.to_bytes())
    ))
}

/// Problems with the seat ballots on `issue`: each `(voter, choice)` needs
/// its newest signature note to name that choice, verify, and use the
/// pinned key. `notes` are the issue's logbook notes, newest first.
#[must_use]
pub fn seat_problems(
    issue: &str,
    seats: &[(&str, &str)],
    notes: &[String],
    pins: &BTreeMap<String, String>,
) -> Vec<String> {
    seat_check(issue, seats, notes, pins, &|_, _| false).0
}

/// [`seat_problems`] with grandfathering: an unsigned ballot `legacy`
/// says the tracker recorded before signing began counts, and is named in
/// the second list as legacy unsigned.
#[must_use]
pub fn seat_check(
    issue: &str,
    seats: &[(&str, &str)],
    notes: &[String],
    pins: &BTreeMap<String, String>,
    legacy: &dyn Fn(&str, &str) -> bool,
) -> (Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    let mut old = Vec::new();
    for (voter, choice) in seats {
        let newest = notes
            .iter()
            .filter_map(|n| parse_note(n))
            .find(|n| n.voter == *voter);
        match newest {
            None if legacy(voter, choice) => old.push((*voter).to_string()),
            None => out.push(format!("{voter}'s ballot is unsigned")),
            Some(n) if n.choice != *choice => out.push(format!(
                "{voter}'s signature is for {} and the ballot says {choice}",
                n.choice
            )),
            Some(n) if !verifies(issue, &n) => {
                out.push(format!("{voter}'s signature does not verify"));
            }
            Some(n) => match pins.get(*voter) {
                Some(k) if *k == n.key => {}
                Some(_) => out.push(format!(
                    "{voter}'s ballot is signed by a key other than its pinned one"
                )),
                None => out.push(format!("{voter} has no pinned key on this tracker")),
            },
        }
    }
    (out, old)
}

/// The logbook notes of an issue JSON, newest first.
#[must_use]
pub fn notes_of(v: &Value) -> Vec<String> {
    v["logbook"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["note"].as_str().map(str::to_string))
        .collect()
}

/// Whether unsigned seat ballots are let through on purpose
/// (`LJOS_UNSIGNED_BALLOTS=allow`), for a tracker from before signing.
#[must_use]
pub fn unsigned_allowed() -> bool {
    std::env::var("LJOS_UNSIGNED_BALLOTS").is_ok_and(|v| v == "allow")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> SigningKey {
        SigningKey::from_bytes(&[b; 32])
    }

    #[test]
    fn a_signed_pinned_ballot_counts_and_every_forgery_does_not() {
        let alice = key(1);
        let mallory = key(2);
        let pins: BTreeMap<String, String> = [(
            "alice".to_string(),
            hex::encode(alice.verifying_key().to_bytes()),
        )]
        .into();
        let good = sign_with(&alice, "d-1", "alice", "ship");
        assert!(seat_problems(
            "d-1",
            &[("alice", "ship")],
            std::slice::from_ref(&good),
            &pins
        )
        .is_empty());
        // Unsigned.
        let p = seat_problems("d-1", &[("alice", "ship")], &[], &pins);
        assert!(p[0].contains("unsigned"), "{p:?}");
        // Signed by another key under alice's name.
        let forged = sign_with(&mallory, "d-1", "alice", "ship");
        let p = seat_problems("d-1", &[("alice", "ship")], &[forged], &pins);
        assert!(p[0].contains("other than its pinned"), "{p:?}");
        // The ballot changed after signing.
        let p = seat_problems(
            "d-1",
            &[("alice", "hold")],
            std::slice::from_ref(&good),
            &pins,
        );
        assert!(p[0].contains("is for ship"), "{p:?}");
        // A signature lifted from another issue.
        let p = seat_problems("d-2", &[("alice", "ship")], &[good], &pins);
        assert!(p[0].contains("does not verify"), "{p:?}");
        // No pin on this tracker.
        let lone = sign_with(&mallory, "d-1", "bob", "ship");
        let p = seat_problems("d-1", &[("bob", "ship")], &[lone], &pins);
        assert!(p[0].contains("no pinned key"), "{p:?}");
    }

    #[test]
    fn an_old_unsigned_ballot_is_legacy_and_a_new_one_is_not() {
        let pins = BTreeMap::new();
        let legacy = |v: &str, _: &str| v == "inky";
        let (bad, old) = seat_check(
            "d-1",
            &[("inky", "ship"), ("bob", "ship")],
            &[],
            &pins,
            &legacy,
        );
        assert_eq!(old, vec!["inky".to_string()]);
        assert_eq!(bad, vec!["bob's ballot is unsigned".to_string()]);
    }

    #[test]
    fn seats_sharing_a_home_get_their_own_keys() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("ljos/seats/inky/ballot.key");
        let b = dir.path().join("ljos/seats/disk-saver/ballot.key");
        let ka = load_or_create(&a).unwrap();
        let kb = load_or_create(&b).unwrap();
        assert_ne!(ka.verifying_key(), kb.verifying_key());
        assert_eq!(
            load_or_create(&a).unwrap().verifying_key(),
            ka.verifying_key()
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&a).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn the_newest_signature_wins() {
        let a = key(3);
        let pins: BTreeMap<String, String> =
            [("a".to_string(), hex::encode(a.verifying_key().to_bytes()))].into();
        let new = sign_with(&a, "d-1", "a", "hold");
        let old = sign_with(&a, "d-1", "a", "ship");
        assert!(seat_problems("d-1", &[("a", "hold")], &[new, old], &pins).is_empty());
    }

    #[test]
    fn a_choice_with_a_space_parses_whole_and_verifies() {
        // The panel eval's unscored case: session spaced-1 cast
        // `beside binary` as seat-a, then `xdg home`.
        let k = key(5);
        let bound = sign_bound(&k, "panel-spcd", "seat-a", "beside binary", "spaced-1", "-");
        let parsed = parse_note(&bound).expect("a bound note with a spaced choice");
        assert_eq!(parsed.voter, "seat-a");
        assert_eq!(parsed.choice, "beside binary");
        assert_eq!(parsed.harness.as_deref(), Some("spaced-1"));
        assert_eq!(parsed.nonce.as_deref(), Some("-"));
        assert!(verifies("panel-spcd", &parsed));
        assert_eq!(
            note_session(&bound),
            Some(("spaced-1".to_string(), "-".to_string()))
        );
        // v1, three words: six fields, and still not read as a session.
        let plain = sign_with(&k, "panel-spcd", "seat-a", "xdg home dir");
        let parsed = parse_note(&plain).expect("a v1 note with a spaced choice");
        assert_eq!(parsed.choice, "xdg home dir");
        assert_eq!(parsed.harness, None);
        assert!(verifies("panel-spcd", &parsed));
        // Two spaces inside the choice stay two, so the signature holds.
        let wide = sign_bound(&k, "panel-spcd", "seat-a", "xdg  home", "spaced-1", "-");
        let parsed = parse_note(&wide).unwrap();
        assert_eq!(parsed.choice, "xdg  home");
        assert!(verifies("panel-spcd", &parsed));
        let pins: BTreeMap<String, String> = [(
            "seat-a".to_string(),
            hex::encode(k.verifying_key().to_bytes()),
        )]
        .into();
        assert!(seat_problems(
            "panel-spcd",
            &[("seat-a", "beside binary")],
            &[bound],
            &pins
        )
        .is_empty());
    }

    #[test]
    fn a_note_that_does_not_parse_still_names_its_session() {
        let odd = "ballot-sig: seat-a beside binary ab cd spaced-1 -";
        assert_eq!(parse_note(odd), None);
        assert_eq!(
            note_session(odd),
            Some(("spaced-1".to_string(), "-".to_string()))
        );
        assert_eq!(note_session("ballot-sig: seat-a ship ab"), None);
        assert_eq!(note_session("finding: seat-a: nit: name the unit"), None);
    }

    #[test]
    fn a_withdrawn_note_is_three_fields() {
        let line = withdrawn_line("seat a", "sess-1", "-");
        assert_eq!(line, "ballot-withdrawn: seat_a sess-1 -");
        let parsed = parse_withdrawn(&line).unwrap();
        assert_eq!(parsed.voter, "seat_a");
        assert_eq!(parsed.harness, "sess-1");
        assert_eq!(parsed.nonce, "-");
        assert!(parse_withdrawn("ballot-withdrawn: only-one").is_none());
        assert!(parse_withdrawn("ballot-sig: seat-a ship").is_none());
    }

    #[test]
    fn a_session_binding_is_signed_and_a_changed_session_is_not() {
        let alice = key(4);
        let note = sign_bound(&alice, "d-1", "alice", "ship", "sess-alice-0001", "nonce-1");
        let parsed = parse_note(&note).unwrap();
        assert_eq!(parsed.harness.as_deref(), Some("sess-alice-0001"));
        assert_eq!(parsed.nonce.as_deref(), Some("nonce-1"));
        assert!(verifies("d-1", &parsed));
        let mut moved = parsed.clone();
        moved.harness = Some("sess-other-0001".into());
        assert!(
            !verifies("d-1", &moved),
            "a rewritten session still verified"
        );
        let pins: BTreeMap<String, String> = [(
            "alice".to_string(),
            hex::encode(alice.verifying_key().to_bytes()),
        )]
        .into();
        assert!(seat_problems("d-1", &[("alice", "ship")], &[note], &pins).is_empty());
    }
}
