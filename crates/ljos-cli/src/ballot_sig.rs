//! Signed seat ballots. A seat signs each ballot it casts with its own
//! ed25519 key, made on its first vote at
//! `$XDG_DATA_HOME/ljos/seats/SEAT/ballot.key` (or `LJOS_SEAT_KEY`), so
//! seats sharing one HOME still sign apart. The signature rides on the
//! issue as a `ballot-sig:` note. A ballot the tracker held before its
//! first pin, by commit order ([`recorded_before_signing`]), is legacy
//! unsigned and still counts. The tracker pins each seat's public key the
//! first time it signs (`.ljos-seat-keys` at the tracker root, committed),
//! so a later ballot under that name signed by another key is a mismatch. The push gate counts a seat ballot only
//! when its latest signature covers exactly that voter, issue and choice
//! under the pinned key.
//!
//! A ballot carries the harness session and a marker (`ljos-ballot/v2`):
//! `-`, or [`SEAT_NONCE`] when only the seat named the session. The
//! session is the harness id, else `VISSUE_AGENT`, else `LJOS_SEAT`, else
//! the MCP connection. It is never a fresh nonce of this process.
//! Minting another seat inside the same session does not make a second voter.

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The note prefix a signature rides under.
pub const SIG_NOTE: &str = "ballot-sig: ";
/// The note prefix a withdrawn ballot rides under.
pub const WITHDRAWN_NOTE: &str = "ballot-withdrawn: ";
/// The pin file at the tracker root.
pub const PIN_FILE: &str = ".ljos-seat-keys";

/// The bytes a ballot signature covers. v1 is the choice alone. v2 also
/// covers the harness session and its marker, so a later edit of either
/// fails the check.
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
/// fallback: it requires [`parse_note`] and [`verifies`] for the issue
/// being cast on. `None` when the note is not a ballot signature.
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
    pub issue: Option<String>,
}

/// One token per field, so the note stays three words.
#[must_use]
pub fn withdrawn_line(voter: &str, harness: &str, nonce: &str) -> String {
    withdrawn_line_on(voter, harness, nonce, None)
}

/// A withdrawn ballot that names the issue it applies to:
/// `ballot-withdrawn: VOTER HARNESS NONCE ISSUE`.
#[must_use]
pub fn withdrawn_line_on(voter: &str, harness: &str, nonce: &str, issue: Option<&str>) -> String {
    let issue = issue.map(field).filter(|id| id != "-");
    if let Some(issue) = issue {
        return format!(
            "{WITHDRAWN_NOTE}{} {} {} {}",
            field(voter),
            field(harness),
            field(nonce),
            issue
        );
    }
    format!(
        "{WITHDRAWN_NOTE}{} {} {}",
        field(voter),
        field(harness),
        field(nonce)
    )
}

/// `ballot-withdrawn: VOTER HARNESS NONCE` (legacy), or
/// `ballot-withdrawn: VOTER HARNESS NONCE ISSUE`.
#[must_use]
pub fn parse_withdrawn(note: &str) -> Option<WithdrawnNote> {
    let rest = note.trim().strip_prefix(WITHDRAWN_NOTE)?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let (voter, harness, nonce, issue) = match parts.as_slice() {
        [voter, harness, nonce] => (*voter, *harness, *nonce, None),
        [voter, harness, nonce, issue] => (*voter, *harness, *nonce, Some((*issue).to_string())),
        _ => return None,
    };
    Some(WithdrawnNote {
        voter: voter.to_string(),
        harness: harness.to_string(),
        nonce: nonce.to_string(),
        issue,
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

pub(crate) fn is_hex_len(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `sig_hex` is an ed25519 signature by `key_hex` over `message`.
///
/// The check is strict: a small-order key or `R` never verifies. The
/// plain equation holds for the identity key, `R` the identity and
/// `s = 0` over every message.
#[must_use]
pub fn signature_ok(key_hex: &str, sig_hex: &str, message: &[u8]) -> bool {
    let (Ok(key), Ok(sig)) = (hex::decode(key_hex), hex::decode(sig_hex)) else {
        return false;
    };
    let (Ok(key), Ok(sig)) = (<[u8; 32]>::try_from(key), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    let Ok(verifying) = VerifyingKey::from_bytes(&key) else {
        return false;
    };
    verifying
        .verify_strict(message, &Signature::from_bytes(&sig))
        .is_ok()
}

/// Whether `key_hex` is an ed25519 point of small order.
#[must_use]
pub fn weak_key(key_hex: &str) -> bool {
    let Ok(Ok(key)) = hex::decode(key_hex).map(<[u8; 32]>::try_from) else {
        return false;
    };
    VerifyingKey::from_bytes(&key).is_ok_and(|key| key.is_weak())
}

/// One name per signing key, for counting keys: the key with its
/// small-order part cleared, in lower-case hex. Strict verification
/// refuses a small-order key but not a key with a small-order part, so
/// one secret signs under eight public keys that differ only there, and
/// under either case of hex digit. A string that is not a key is its hex
/// in lower case.
#[must_use]
pub fn key_fingerprint(key_hex: &str) -> String {
    let key = hex::decode(key_hex)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok());
    match key {
        Some(key) => hex::encode(key.to_edwards().mul_by_cofactor().compress().to_bytes()),
        None => key_hex.to_ascii_lowercase(),
    }
}

/// Public key and signature, both lower-case hex, for `message` under `key`.
#[must_use]
pub fn signed_hex(key: &SigningKey, message: &[u8]) -> (String, String) {
    (
        hex::encode(key.verifying_key().to_bytes()),
        hex::encode(key.sign(message).to_bytes()),
    )
}

/// Remember `key` as `voter`'s pin, or refuse a different key.
///
/// # Errors
///
/// `voter` is already pinned to another key.
pub fn observe_pin(pins: &mut BTreeMap<String, String>, voter: &str, key: &str) -> Result<()> {
    let fingerprint = key_fingerprint(key);
    match pins.get(voter) {
        Some(pinned) if key_fingerprint(pinned) != fingerprint => bail!(
            "{voter} is pinned to key {}…, got {}…",
            key_prefix(pinned),
            key_prefix(key)
        ),
        Some(_) => Ok(()),
        None => {
            pins.insert(voter.to_string(), key.to_string());
            Ok(())
        }
    }
}

fn key_prefix(key: &str) -> &str {
    &key[..12.min(key.len())]
}

pub(crate) fn key_from_file(path: &std::path::Path) -> Result<SigningKey> {
    let raw = std::fs::read(path).with_context(|| format!("seat key {}", path.display()))?;
    let seed: [u8; 32] = raw
        .get(..32)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| anyhow::anyhow!("seat key {} holds fewer than 32 bytes", path.display()))?;
    Ok(SigningKey::from_bytes(&seed))
}

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
///
/// The directory keeps ASCII letters, digits, `-` and `.`, and writes any
/// other byte, `_` included, and a leading `.` as `%HH`, so no two seats
/// share a file. Older builds folded those bytes to `_`; a key at that
/// folded name moves here on first use. No name written here has a `_`,
/// so the folded file is no seat's current key.
#[must_use]
pub fn seat_key_file(seat: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let seats = base.join("ljos").join("seats");
    let seat = seat.trim();
    let name = seat_dir_name(seat);
    let path = seats.join(&name).join("ballot.key");
    let folded = folded_seat_dir_name(seat);
    if folded != name && !folded.starts_with('.') && !path.exists() {
        let old = seats.join(folded).join("ballot.key");
        if old.is_file() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::rename(&old, &path);
        }
    }
    Some(path)
}

fn seat_dir_name(seat: &str) -> String {
    let mut out = String::with_capacity(seat.len());
    for (at, b) in seat.bytes().enumerate() {
        if b.is_ascii_alphanumeric() || b == b'-' || (b == b'.' && at > 0) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02x}"));
        }
    }
    out
}

fn folded_seat_dir_name(seat: &str) -> String {
    seat.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
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

/// Whether `voter`'s ballot for `choice` on `issue` was recorded before
/// signing began in this tracker, by commit order: its `] VOTER: CHOICE`
/// line is in that issue's own entry in every parent of each commit that
/// brought the pin file into a tree none of its parents held it in. No
/// git date is read. `Ok(false)` on a tracker that is not a git repository
/// or has no pin committed; `LJOS_UNSIGNED_BALLOTS=allow` is for a tracker
/// that never signed.
///
/// # Errors
///
/// [`crate::slot::require_tracker_history`] could not make the history
/// readable. A missing parent is not "the ballot was not there yet".
pub fn recorded_before_signing(issue: &str, voter: &str, choice: &str) -> Result<bool, String> {
    let Some(root) = env_root() else {
        return Ok(false);
    };
    recorded_before_signing_in(&root, issue, voter, choice)
}

/// [`recorded_before_signing`] in the tracker at `root`.
///
/// # Errors
///
/// [`crate::slot::require_tracker_history`] could not make the history
/// readable.
pub fn recorded_before_signing_in(
    root: &Path,
    issue: &str,
    voter: &str,
    choice: &str,
) -> Result<bool, String> {
    crate::slot::require_tracker_history(root)?;
    let log = git_in(
        root,
        &[
            "--no-pager",
            "log",
            "--full-history",
            "-m",
            "--diff-filter=A",
            "--format=%H %P",
            "--",
            PIN_FILE,
        ],
    )
    .unwrap_or_default();
    let held = |commit: &str| crate::slot::file_at_in(root, commit, PIN_FILE).is_some();
    let starts: Vec<Vec<&str>> = log
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let commit = parts.next()?;
            let parents: Vec<&str> = parts.collect();
            (held(commit) && !parents.iter().any(|p| held(p))).then_some(parents)
        })
        .collect();
    let needle = format!("] {}: {}", voter.trim(), choice.trim());
    Ok(!starts.is_empty()
        && starts.iter().all(|parents| {
            !parents.is_empty()
                && parents
                    .iter()
                    .all(|p| crate::slot::note_on_issue_at_in(root, p, issue, &needle))
        }))
}

pub(crate) fn env_root() -> Option<PathBuf> {
    std::env::var_os("VISSUE_ROOT")
        .filter(|r| !r.is_empty())
        .map(PathBuf::from)
}

/// What `git ARGS` prints in the tracker at `VISSUE_ROOT`, trimmed, when
/// it succeeds. Replacements and grafts are off ([`git_history`]).
pub(crate) fn tracker_git(args: &[&str]) -> Option<String> {
    git_in(&env_root()?, args)
}

/// Whether `VISSUE_ROOT` names a git checkout. `None` when no tracker root is
/// configured or git could not run.
#[must_use]
pub fn tracker_is_git_checkout() -> Option<bool> {
    let root = env_root()?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()?;
    if !out.status.success() {
        return Some(false);
    }
    Some(String::from_utf8_lossy(&out.stdout).trim() == "true")
}

/// `git` in `root` with replacements and grafts off. Pin order is the
/// parents stored in the commits. A `refs/replace` ref or `info/grafts`
/// rewrites those parents, so a read of tracker history must not follow
/// either. An exported `GIT_DIR` or object directory would answer for
/// another repository in place of `root`, so those are dropped too.
pub(crate) fn git_history(root: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    crate::clear_git_repo_env(&mut cmd);
    cmd.arg("--no-replace-objects")
        .arg("-C")
        .arg(root)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_GRAFT_FILE", "/dev/null");
    cmd
}

/// What `git ARGS` prints in `root`, trimmed, when it succeeds.
/// Replacements and grafts are off ([`git_history`]).
pub(crate) fn git_in(root: &Path, args: &[&str]) -> Option<String> {
    let out = git_history(root).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The note text for a ballot signed by `key` (v1, choice only).
#[must_use]
pub fn sign_with(key: &SigningKey, issue: &str, voter: &str, choice: &str) -> String {
    let (public, sig) = signed_hex(key, &message(issue, voter, choice));
    format!(
        "{SIG_NOTE}{} {} {} {}",
        voter.trim(),
        choice.trim(),
        public,
        sig
    )
}

/// The note text for a ballot bound to a harness session and its marker.
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
    let (public, sig) = signed_hex(key, &message_v2(issue, voter, choice, &harness, &nonce));
    format!(
        "{SIG_NOTE}{} {} {} {} {} {}",
        voter.trim(),
        choice.trim(),
        public,
        sig,
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
    signature_ok(&note.key, &note.sig, &covered(issue, note))
}

/// The tracker root the pins live at.
fn pin_path() -> Option<PathBuf> {
    Some(env_root()?.join(PIN_FILE))
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

fn parse_pins_strict(text: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(voter) = it.next() else {
            continue;
        };
        let key = it
            .next()
            .ok_or_else(|| anyhow::anyhow!("pin line {} must be `voter key`", idx + 1))?;
        if it.next().is_some() {
            bail!("pin line {} must be `voter key`", idx + 1);
        }
        if out.insert(voter.to_string(), key.to_string()).is_some() {
            bail!("duplicate pin voter `{voter}`");
        }
    }
    Ok(out)
}

/// The pinned keys for record checking, and where they came from.
///
/// When there is no pin file, checking starts with no pins and the source is
/// `none: first-seen in this check`. A link to a missing file is a pin file
/// that cannot be read, not an absent one.
pub fn pins_for_check() -> Result<(BTreeMap<String, String>, String)> {
    match env_root() {
        Some(root) => pins_for_check_in(&root),
        None => Ok((BTreeMap::new(), NO_PIN_FILE.into())),
    }
}

const NO_PIN_FILE: &str = "none: first-seen in this check";

/// [`pins_for_check`] for the tracker at `root`.
pub fn pins_for_check_in(root: &Path) -> Result<(BTreeMap<String, String>, String)> {
    let path = root.join(PIN_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(&path).is_ok() {
                bail!("pin file {} is a link to a missing file", path.display());
            }
            return Ok((BTreeMap::new(), NO_PIN_FILE.into()));
        }
        Err(err) => return Err(err).with_context(|| format!("pin file {}", path.display())),
    };
    let pins = parse_pins_strict(&text).with_context(|| format!("pin file {}", path.display()))?;
    Ok((pins, path.display().to_string()))
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

/// Run `f` as an MCP call that named `session`, made by the subagent
/// `agent`. One server answers a parent and all its subagents, so a call
/// that names no agent is the parent's, whatever this process holds.
pub async fn scope_session<F: std::future::Future>(
    session: Option<String>,
    agent: Option<String>,
    f: F,
) -> F::Output {
    let agent = agent
        .map(|a| field(&a))
        .filter(|a| a != "-")
        .unwrap_or_default();
    let f = CALL_AGENT.scope(agent, f);
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
/// harness named one: the agent a hook saw make this call, else a Codex
/// thread under its root session. The command line takes no agent, so a
/// parent cannot cast as its subagent.
fn subagent(parent: &str) -> Option<String> {
    if let Ok(agent) = CALL_AGENT.try_with(Clone::clone) {
        return (!agent.is_empty()).then_some(agent);
    }
    PROCESS_AGENT
        .get_or_init(|| {
            crate::subagent::claim_command(parent)
                .filter(|a| !a.is_empty())
                .map(|a| field(&a))
                .or_else(|| codex_thread(parent))
        })
        .clone()
}

static HOOKED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// The session a hook saw run this process's `ljos` line, plus its
/// subagent, for a process whose environment names no session: a seat
/// whose runner stamps no id in the shell still binds what its hook saw,
/// rather than the seat. An MCP call names its own session.
fn hooked_session() -> Option<String> {
    if CALL_AGENT.try_with(|_| ()).is_ok() {
        return None;
    }
    HOOKED
        .get_or_init(|| {
            let (session, agent) = crate::subagent::claim_unsessioned_command()?;
            let (session, agent) = (field(&session), field(&agent));
            Some(if agent == "-" {
                session
            } else {
                format!("{session}:{agent}")
            })
        })
        .clone()
}

/// The thread Codex runs a subagent in, under the root session:
/// `CODEX_THREAD_ID` when it is not `CODEX_SESSION_ID`, the root's.
fn codex_thread(parent: &str) -> Option<String> {
    let root = env_token("CODEX_SESSION_ID")?;
    let thread = env_token("CODEX_THREAD_ID")?;
    (root == parent && thread != root).then_some(thread)
}

/// Remember the hook payload's session for this process, and export it for
/// a child when the environment has not already named one. An id that
/// names nobody ([`crate::usable_session_id`]) is no session.
pub fn export_hook_session(id: Option<&str>) {
    crate::with_env_mutation(|| {
        let id = id.and_then(crate::usable_session_id).map(field);
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
/// hook, then a runner-stamped `*_SESSION_ID` / thread / conversation id,
/// then `LJOS_HARNESS_SESSION`, then the run ([`run_session`]).
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
    let stamped = crate::stamped_sessions();
    const PREFER: &[&str] = &[
        "CURSOR_SESSION_ID",
        "CLAUDE_SESSION_ID",
        // A Codex subagent's thread is not its session: [`codex_thread`]
        // names it under the root.
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
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
        if let Some(s) = stamped
            .iter()
            .find(|(k, _)| k == *key)
            .map(|(_, v)| field(v))
            .or_else(|| env_token(key))
        {
            // A shell runner's env file shares `NAME-shell` across every
            // conversation; it holds claims and names no session.
            if s.len() >= 8 && !(*key == "LJOS_SESSION_ID" && crate::is_shared_holder_id(&s)) {
                return s;
            }
        }
    }
    if let Some(s) = crate::runner_session_id().map(|s| field(&s)) {
        return s;
    }
    if let Some(s) = env_token("LJOS_HARNESS_SESSION") {
        return s;
    }
    run_session().unwrap_or_default()
}

#[cfg(test)]
static RUNNER_SESSIONS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// Stand in for the runner a unit test runs without, until the value
/// returned is dropped.
#[cfg(test)]
#[must_use]
pub(crate) fn fake_runner_sessions(ids: &[&str]) -> impl Drop {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            *RUNNER_SESSIONS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }
    *RUNNER_SESSIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(ids.iter().map(|s| (*s).to_string()).collect());
    Reset
}

/// The sessions the runner above this process gave the process it
/// started, read from that process's environment and never from this
/// one's: `LJOS_HARNESS_SESSION=forged ljos ...` sets ours, not that.
/// Every id there is the runner's, so a bind may name any of them. The
/// session an MCP call's `_meta` names is not: any client of the server
/// writes it, so it scopes a call ([`call_session`]) and never vouches
/// for one. Empty when no runner is above this process.
#[must_use]
pub fn runner_sessions() -> Vec<String> {
    #[cfg(test)]
    if let Some(ids) = RUNNER_SESSIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    {
        return ids;
    }
    let Some(started) = crate::proc_tree::started_by_runner() else {
        return Vec::new();
    };
    crate::proc_tree::environ(started)
        .unwrap_or_default()
        .into_iter()
        .filter(|(k, v)| k == "LJOS_HARNESS_SESSION" || crate::runner_session_var(k, v))
        .map(|(_, v)| field(v.trim()))
        .filter(|v| !v.is_empty() && v != "-")
        .collect()
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

/// The nonce of this process, handed to a child `ljos`. No ballot carries
/// it: [`identity`] binds the session alone.
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
/// With no session in the environment, the one the hook saw run this
/// line comes before the seat.
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
    if let Some(harness) = hooked_session() {
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
    let pinned = match pin_path() {
        Some(p) if !pins().contains_key(voter.trim()) => pin_first(&p, voter.trim(), &mine)?,
        _ => pins(),
    };
    let mine_fp = key_fingerprint(&mine);
    if let Some(k) = pinned
        .get(voter.trim())
        .filter(|k| key_fingerprint(k) != mine_fp)
    {
        bail!(
            "seat {voter} is pinned to key {}…, and this key is {}…; the ballot is unsigned and no gate counts it",
            &k[..12.min(k.len())],
            &mine[..12]
        );
    }
    Ok((key, mine))
}

/// Pin `mine` for `voter` unless another pin for it landed first, and
/// return the pins. Seats pin on their first ballot, often at once: the
/// read and the write hold the tracker's commit lock, so no seat's pin is
/// lost, and the file is replaced whole, so a `git add` never hashes half
/// of it. The seat's tracker commit carries the pin.
fn pin_first(path: &Path, voter: &str, mine: &str) -> Result<BTreeMap<String, String>> {
    let _held = crate::tracker_commit_lock(path.parent().unwrap_or_else(|| Path::new(".")));
    let mut pinned = std::fs::read_to_string(path)
        .map(|t| parse_pins(&t))
        .unwrap_or_default();
    if !pinned.contains_key(voter) {
        pinned.insert(voter.to_string(), mine.to_string());
        let text: String = pinned.iter().map(|(v, k)| format!("{v} {k}\n")).collect();
        vissue_core::store::replace_file_atomically(path, &text)?;
    }
    Ok(pinned)
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
    if let Some((existing, withdrawn)) = newest_sig(issue, voter) {
        if verifies(issue, &existing) {
            match resign_of(&existing, choice, voter, withdrawn) {
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
/// choice. A withdrawn ballot is re-signed, even for the same choice, so
/// the new ballot stands above the withdrawal cut. Any other key,
/// including the machine key, does not sign again.
fn resign_of(existing: &SigNote, choice: &str, voter: &str, withdrawn: bool) -> Resign {
    let mine = current_public_hex(voter);
    if mine.as_deref() == Some(existing.key.as_str()) {
        if existing.choice == choice && !withdrawn {
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

fn same_voter(signed: &str, voter: &str) -> bool {
    signed == voter.trim() || signed == field(voter)
}

fn newest_sig(issue: &str, voter: &str) -> Option<(SigNote, bool)> {
    let layout = vissue_core::Layout::resolve(None, None).ok()?;
    let (heading, _, _) = vissue_core::store::find_by_id(&layout, issue).ok()??;
    let mut cut = None;
    let mut newest = None;
    for (at, entry) in heading.logbook.iter().enumerate() {
        let Some(text) = entry.note.as_deref() else {
            continue;
        };
        if cut.is_none() {
            if let Some(withdrawn) = parse_withdrawn(text) {
                if same_voter(&withdrawn.voter, voter) {
                    cut = Some(at);
                }
            }
        }
        if newest.is_none() {
            if let Some(sig) = parse_note(text) {
                if same_voter(&sig.voter, voter) {
                    newest = Some((at, sig));
                }
            }
        }
        if cut.is_some() && newest.is_some() {
            break;
        }
    }
    let (at, sig) = newest?;
    let withdrawn = cut.is_some_and(|cut_at| cut_at < at);
    Some((sig, withdrawn))
}

pub(crate) fn write_sig_note(issue: &str, note: &str) -> Result<()> {
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
    let Some(voter) = own_voter() else {
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

/// The name this process's seat casts and signs under: the seat its write
/// check named, else `VISSUE_AGENT`, else `LJOS_SEAT`.
#[must_use]
pub fn own_voter() -> Option<String> {
    crate::checked_seat().or_else(|| {
        ["VISSUE_AGENT", "LJOS_SEAT"].iter().find_map(|key| {
            std::env::var(key)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
    })
}

/// Leave `ballot-withdrawn: VOTER HARNESS NONCE` on `issue` for this session.
///
/// # Errors
///
/// The tracker did not take the note.
pub fn note_withdrawn(issue: &str, voter: &str) -> Result<String> {
    note_withdrawn_on(issue, voter, issue)
}

/// Leave `ballot-withdrawn: VOTER HARNESS NONCE ISSUE` on `logbook_issue`
/// for this session.
///
/// # Errors
///
/// The tracker did not take the note.
pub fn note_withdrawn_on(
    logbook_issue: &str,
    voter: &str,
    withdrawn_issue: &str,
) -> Result<String> {
    let id = identity();
    let note = withdrawn_line_on(voter, &id.harness, &id.nonce, Some(withdrawn_issue));
    crate::run_captured("vissue", &["note", logbook_issue, &note]).with_context(|| {
        format!("ballot withdrawn; the note on {logbook_issue} was not written")
    })?;
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
    signature_ok(
        &note.key,
        &note.sig,
        &candidate_message(
            &note.issue,
            &note.seat,
            &note.harness,
            &note.nonce,
            &saw_token(&note.saw),
        ),
    )
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
    let (public, sig) = signed_hex(
        &key,
        &candidate_message(issue, seat, &harness, &nonce, &saw_tok),
    );
    Ok(format!(
        "{CANDIDATE_NOTE}{} {} {} {} {} {} {}",
        field(issue),
        field(seat),
        harness,
        nonce,
        saw_tok,
        public,
        sig
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
                Some(k) if key_fingerprint(k) == key_fingerprint(&n.key) => {}
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
    use std::path::Path;
    use std::process::Command;

    fn key(b: u8) -> SigningKey {
        SigningKey::from_bytes(&[b; 32])
    }

    fn git(dir: &Path, args: &[&str], env: &[(&str, String)]) {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(dir).args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
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
    fn a_key_at_its_folded_name_moves_to_the_seat_file() {
        let _lock = crate::test_env_lock();
        let data = tempfile::tempdir().unwrap();
        crate::with_env_mutation(|| {
            // SAFETY: this test holds the process environment lock.
            unsafe { std::env::set_var("XDG_DATA_HOME", data.path()) };
        });
        let folded = data.path().join("ljos/seats/judge_model-1.5/ballot.key");
        std::fs::create_dir_all(folded.parent().unwrap()).unwrap();
        std::fs::write(&folded, [7u8; 32]).unwrap();
        let path = seat_key_file("judge:model-1.5").unwrap();
        assert_ne!(path, folded);
        assert_eq!(std::fs::read(&path).unwrap(), vec![7u8; 32]);
        assert!(!folded.exists());
        assert_eq!(seat_key_file("judge:model-1.5").unwrap(), path);
        let slug = seat_key_file("disk-saver").unwrap();
        assert_eq!(slug, data.path().join("ljos/seats/disk-saver/ballot.key"));
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
        assert_eq!(parsed.issue, None);
        let on = withdrawn_line_on("seat a", "sess-1", "-", Some("child-a"));
        assert_eq!(on, "ballot-withdrawn: seat_a sess-1 - child-a");
        let parsed = parse_withdrawn(&on).unwrap();
        assert_eq!(parsed.issue.as_deref(), Some("child-a"));
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

    #[test]
    fn a_legacy_ballot_is_read_on_its_own_issue() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("issues.org");
        git(dir.path(), &["init"], &[]);
        git(dir.path(), &["config", "user.name", "Rohit Goswami"], &[]);
        git(
            dir.path(),
            &["config", "user.email", "rgoswami@ieee.org"],
            &[],
        );
        std::fs::write(
            &file,
            "* TODO [#C] old\n:PROPERTIES:\n:ID:         probe-old\n:END:\n- [X] elder: rail\n",
        )
        .unwrap();
        git(dir.path(), &["add", "issues.org"], &[]);
        git(dir.path(), &["commit", "-m", "old ballot"], &[]);
        std::fs::write(dir.path().join(PIN_FILE), "inky 00\n").unwrap();
        git(dir.path(), &["add", PIN_FILE], &[]);
        git(dir.path(), &["commit", "-m", "first pin"], &[]);
        std::fs::write(
            &file,
            "* TODO [#C] old\n:PROPERTIES:\n:ID:         probe-old\n:END:\n- [X] elder: rail\n\
             * TODO [#C] new\n:PROPERTIES:\n:ID:         probe-new\n:END:\n- [X] elder: rail\n",
        )
        .unwrap();
        git(dir.path(), &["add", "issues.org"], &[]);
        git(dir.path(), &["commit", "-m", "new ballot"], &[]);
        assert!(recorded_before_signing_in(dir.path(), "probe-old", "elder", "rail").unwrap());
        assert!(!recorded_before_signing_in(dir.path(), "probe-new", "elder", "rail").unwrap());
    }

    #[test]
    fn seat_check_accepts_the_same_key_in_uppercase_hex() {
        let alice = key(8);
        let note = sign_with(&alice, "d-1", "alice", "ship");
        let pins: BTreeMap<String, String> = [(
            "alice".to_string(),
            hex::encode(alice.verifying_key().to_bytes()).to_ascii_uppercase(),
        )]
        .into();
        assert!(seat_problems("d-1", &[("alice", "ship")], &[note], &pins).is_empty());
    }

    /// One server answers a parent and its subagents: each call is the
    /// session plus the agent it names, and a call that names none is the
    /// parent's.
    #[tokio::test]
    async fn an_mcp_call_from_a_subagent_is_a_session_of_its_own() {
        let call = |agent: Option<&str>| {
            scope_session(
                Some("sess-mcp-0001".into()),
                agent.map(str::to_string),
                async { identity().harness },
            )
        };
        assert_eq!(call(Some("thread-a")).await, "sess-mcp-0001:thread-a");
        assert_eq!(call(Some("thread-b")).await, "sess-mcp-0001:thread-b");
        assert_eq!(call(None).await, "sess-mcp-0001");
    }
}
