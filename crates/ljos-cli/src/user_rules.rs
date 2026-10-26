//! Standing rules shared by every seat of this user, and by every machine
//! that shares the tracker repository.
//!
//! `ljos prefer --user` and `ljos rule --user` write them. The copy the
//! hooks read is the user config, not the pack, so the review clock never
//! sees them. The tracker carries one file per machine, and a sync merges
//! those files by id.
//!
//! A row written before `basis` was kept has neither `basis` nor `from`.
//! It stays in the preface, marked not confirmed, until the person runs
//! `ljos prefer --user --review --keep`. The cutoff is a local receipt
//! written once by the first person-side command. A row written after
//! that receipt with no basis stays out. A hook does not write the
//! receipt: it runs under the runner.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Rule;

/// How much of the preface a brief or a hook will carry.
pub const PREFACE_BUDGET: usize = 1200;

/// Handles and names a guard matches. The prose of the rule is not read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct People {
    /// GitHub logins, without a leading `@`, compared case-insensitively.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<String>,
    /// Display names, matched as whole words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
}

/// One standing rule. `due_at` is never written. A sweep drops a row only
/// when that field is present and already past, which a row this module
/// writes is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRule {
    pub id: String,
    pub text: String,
    pub author: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "people_empty")]
    pub deny_mention: People,
    #[serde(default, skip_serializing_if = "people_empty")]
    pub deny_reply_to: People,
    pub written_at: String,
    /// When this row last changed. A merge keeps the newer stamp, and with
    /// it that row's author and reason.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_at: String,
    /// A deletion. The row stays so an older copy of the same id does not
    /// come back on the next sync.
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
    /// The lesson id a promote copied, when this row came from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Who stands behind a rule written as words or an argv law, as
    /// `source.basis` on a seat atom: `person` only when the person typed
    /// it or said it in their latest prompt. A row with neither this nor
    /// `from` was written before that stamp. It stays visible, marked
    /// held, when it is older than the cutoff receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    /// [`lesson_hash`] of that lesson's record as the promote read it. A
    /// record under `from` that hashes otherwise is another lesson.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson_hash: Option<String>,
    /// Absent on every row the commands write. Present only so a sweep can
    /// drop a row that was given a review date by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<String>,
}

fn people_empty(p: &People) -> bool {
    p.handles.is_empty() && p.names.is_empty()
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    rules: Vec<UserRule>,
}

/// `$XDG_CONFIG_HOME/ljos/user-rules.json`, else `~/.config/ljos/user-rules.json`.
#[must_use]
pub fn rules_path() -> Option<PathBuf> {
    Some(super::config_base()?.join("ljos").join("user-rules.json"))
}

/// The most a user-rules file may hold. A larger file is not read.
pub const RULES_FILE_CAP: u64 = 1024 * 1024;

/// The rows on disk that are still in force. An absent file is an empty
/// list. Tombstones are left out.
///
/// # Errors
///
/// The file is empty, a dangling symlink, not a regular file of at most
/// [`RULES_FILE_CAP`] bytes, not readable, or not the JSON this module writes.
pub fn load() -> Result<Vec<UserRule>> {
    Ok(read_all()?
        .into_iter()
        .filter(|rule| !rule.deleted)
        .collect())
}

/// Every row on disk, tombstones included.
///
/// # Errors
///
/// The file is empty, a dangling symlink, not a regular file of at most
/// [`RULES_FILE_CAP`] bytes, not readable, or not the JSON this module writes.
pub(crate) fn read_all() -> Result<Vec<UserRule>> {
    let Some(path) = rules_path() else {
        return Ok(Vec::new());
    };
    // Empty text, whitespace, and JSON that does not parse can be a
    // rewrite caught between a truncate and the new bytes. A dangling
    // symlink, a FIFO, and a file over the cap are the file they are,
    // and those errors return at once.
    // The stamp is the length, the mtime, and a prefix of the bytes.
    // A stamp that stays the same for `RULES_STABLE` is a corrupt file,
    // and that error is returned. A stamp that changes is a rewrite still in
    // progress: keep reading until a parse succeeds or `RULES_HARD` has
    // passed, and then fail closed. A covered write is not allowed on
    // that last error.
    let started = std::time::Instant::now();
    let mut stable_since = std::time::Instant::now();
    let mut last_stamp: Option<RulesStamp> = None;
    loop {
        match read_rules_text(&path) {
            Ok(None) => return Ok(Vec::new()),
            Ok(Some(text)) => match parse_rules(&text) {
                Ok(rules) => return Ok(rules),
                Err(err) => {
                    let err = err.context(format!(
                        "user rules: {} is not a user-rules file",
                        path.display()
                    ));
                    if rules_read_gave_up(
                        &path,
                        Some(text.as_bytes()),
                        &mut last_stamp,
                        &mut stable_since,
                        started,
                    ) {
                        return Err(err);
                    }
                }
            },
            Err(err) if rules_rewrite_symptom(&err) => {
                if rules_read_gave_up(&path, None, &mut last_stamp, &mut stable_since, started) {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// How long an unchanged bad file is treated as corrupt.
const RULES_STABLE: std::time::Duration = std::time::Duration::from_millis(80);
/// How long a file whose bytes keep changing is re-read.
const RULES_HARD: std::time::Duration = std::time::Duration::from_millis(200);

#[derive(PartialEq, Eq)]
struct RulesStamp {
    len: u64,
    mtime: Option<std::time::SystemTime>,
    prefix: [u8; 64],
    prefix_len: usize,
}

fn rules_rewrite_symptom(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains(" is empty")
}

fn rules_stamp(path: &std::path::Path, bytes: Option<&[u8]>) -> RulesStamp {
    let meta = std::fs::symlink_metadata(path).ok();
    let prefix_src = bytes.unwrap_or(b"");
    let n = prefix_src.len().min(64);
    let mut prefix = [0u8; 64];
    prefix[..n].copy_from_slice(&prefix_src[..n]);
    RulesStamp {
        len: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        mtime: meta.as_ref().and_then(|m| m.modified().ok()),
        prefix,
        prefix_len: n,
    }
}

/// `true` when this read should be the last: the stamp has not moved for
/// [`RULES_STABLE`], or the hard cap has passed.
fn rules_read_gave_up(
    path: &std::path::Path,
    bytes: Option<&[u8]>,
    last: &mut Option<RulesStamp>,
    stable_since: &mut std::time::Instant,
    started: std::time::Instant,
) -> bool {
    let stamp = rules_stamp(path, bytes);
    let same = match last {
        Some(prev) if *prev == stamp => true,
        _ => {
            *last = Some(stamp);
            *stable_since = std::time::Instant::now();
            false
        }
    };
    if started.elapsed() >= RULES_HARD {
        return true;
    }
    same && stable_since.elapsed() >= RULES_STABLE
}

/// The text of the user-rules file. `Ok(None)` when no file is there.
/// A zero-byte file, a file of only whitespace, and a dangling symlink
/// are errors. The open does not block: a FIFO, a directory, or any
/// other non-regular file is an error, and a regular file over
/// [`RULES_FILE_CAP`] is not read.
fn read_rules_text(path: &std::path::Path) -> Result<Option<String>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    bail!("user rules: {} is a dangling symlink", path.display());
                }
                Ok(_) => Err(anyhow::Error::from(err)
                    .context(format!("user rules: read {} failed", path.display()))),
                Err(stat) if stat.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(stat) => Err(anyhow::Error::from(stat)
                    .context(format!("user rules: stat {} failed", path.display()))),
            };
        }
        Err(err) => {
            return Err(anyhow::Error::from(err)
                .context(format!("user rules: read {} failed", path.display())));
        }
    };
    // Stat the opened descriptor. The type and the size are this inode.
    let meta = file
        .metadata()
        .with_context(|| format!("user rules: stat {} failed", path.display()))?;
    if !meta.is_file() {
        bail!("user rules: {} is not a regular file", path.display());
    }
    if meta.len() == 0 {
        bail!("user rules: {} is empty", path.display());
    }
    if meta.len() > RULES_FILE_CAP {
        bail!(
            "user rules: {} is {} bytes, over the {} byte cap",
            path.display(),
            meta.len(),
            RULES_FILE_CAP
        );
    }
    let mut text = String::new();
    file.take(RULES_FILE_CAP)
        .read_to_string(&mut text)
        .with_context(|| format!("user rules: read {} failed", path.display()))?;
    if text.trim().is_empty() {
        bail!("user rules: {} is empty", path.display());
    }
    Ok(Some(text))
}

/// Parse one user-rules document. An empty body is an empty list.
///
/// # Errors
///
/// The text is not the JSON this module writes.
pub(crate) fn parse_rules(text: &str) -> Result<Vec<UserRule>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let file: File = serde_json::from_str(text)?;
    Ok(file.rules)
}

/// The document this module writes, tombstones included.
///
/// # Errors
///
/// The rows cannot be encoded.
pub(crate) fn render_rules(rules: &[UserRule]) -> Result<String> {
    let rules: Vec<UserRule> = rules.iter().cloned().map(dated).collect();
    let body = serde_json::to_string_pretty(&File { rules })?;
    Ok(format!("{body}\n"))
}

fn store(rules: &[UserRule]) -> Result<()> {
    let _durable = crate::bookkeep::durable_write();
    let path = rules_path().context("user rules: no home and no XDG_CONFIG_HOME")?;
    let Some(parent) = path.parent() else {
        anyhow::bail!("user rules: {} has no directory", path.display());
    };
    std::fs::create_dir_all(parent)
        .with_context(|| format!("user rules: create {} failed", parent.display()))?;
    let body = render_rules(rules)?;
    if std::fs::read(&path).is_ok_and(|have| have == body.as_bytes()) {
        return Ok(());
    }
    // A unique temp in the same directory, fsync, then rename. Two stores
    // no longer share `user-rules.json.tmp`, and a reader never sees a
    // truncated document from this write.
    let tmp = unique_rules_temp(parent)?;
    let write_tmp = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .with_context(|| format!("user rules: create {} failed", tmp.display()))?;
        file.write_all(body.as_bytes())
            .with_context(|| format!("user rules: write {} failed", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("user rules: sync {} failed", tmp.display()))?;
        Ok(())
    })();
    if let Err(err) = write_tmp {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err).context(format!("user rules: rename into {} failed", path.display()));
    }
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn unique_rules_temp(parent: &std::path::Path) -> Result<std::path::PathBuf> {
    for n in 0..32u32 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = parent.join(format!(
            ".user-rules.{}.{nanos}.{n}.tmp",
            std::process::id()
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    anyhow::bail!(
        "user rules: could not pick a temp name in {}",
        parent.display()
    );
}

/// Replace the local file. Used by a sync, which must not start another sync.
pub(crate) fn replace_stored(rules: &[UserRule]) -> Result<()> {
    store(rules)
}

fn append(rule: UserRule) -> Result<UserRule> {
    let mut rules = read_all()?;
    rules.push(rule.clone());
    store(&rules)?;
    sync_after_write();
    Ok(rule)
}

/// Whether a row would be due at `now`. A row with no `due_at` is never due.
#[must_use]
pub fn is_due(rule: &UserRule, now: &str) -> bool {
    rule.due_at
        .as_deref()
        .is_some_and(|due| !due.is_empty() && due <= now)
}

/// How many rows would be due at `now`. Ordinary rows contribute nothing.
#[must_use]
pub fn review_due(now: &str) -> usize {
    load()
        .unwrap_or_default()
        .iter()
        .filter(|rule| is_due(rule, now))
        .count()
}

/// Drop rows whose `due_at` is already past `now`, and leave every other
/// row where it is. Returns how many were dropped.
///
/// # Errors
///
/// The file cannot be read or rewritten.
pub fn sweep(now: &str) -> Result<usize> {
    let rules = read_all()?;
    let kept: Vec<UserRule> = rules
        .iter()
        .filter(|rule| !is_due(rule, now))
        .cloned()
        .collect();
    let dropped = rules.len() - kept.len();
    if dropped > 0 {
        store(&kept)?;
    }
    Ok(dropped)
}

fn new_id(text: &str) -> String {
    let mut raw = [0u8; 16];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = file.read_exact(&mut raw);
    }
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for (i, byte) in raw.iter_mut().enumerate().take(8) {
        *byte ^= (tick >> (i * 8)) as u8;
    }
    for (i, byte) in text.bytes().enumerate() {
        raw[i % raw.len()] ^= byte.wrapping_add(i as u8);
    }
    let mut hex = String::with_capacity(32);
    for byte in raw {
        hex.push(b"0123456789abcdef"[(byte >> 4) as usize] as char);
        hex.push(b"0123456789abcdef"[(byte & 0x0f) as usize] as char);
    }
    format!("ur-{hex}")
}

fn stamp(rule: &UserRule) -> &str {
    let at = rule.updated_at.trim();
    if at.is_empty() {
        rule.written_at.trim()
    } else {
        at
    }
}

fn dated(mut rule: UserRule) -> UserRule {
    if rule.updated_at.trim().is_empty() {
        rule.updated_at = rule.written_at.clone();
    }
    rule
}

/// Merge rows by id. The newer `updated_at` wins, and that row's author
/// and reason come with it. A tombstone beats an older edit of the same
/// id, and it also beats a live row that carries the same stamp.
#[must_use]
pub fn merge_rows(rows: impl IntoIterator<Item = UserRule>) -> Vec<UserRule> {
    let mut best: std::collections::BTreeMap<String, UserRule> = std::collections::BTreeMap::new();
    for rule in rows {
        let id = rule.id.trim().to_string();
        if id.is_empty() {
            continue;
        }
        let rule = dated(rule);
        if best.get(&id).is_some_and(|have| beats(have, &rule)) {
            continue;
        }
        best.insert(id, rule);
    }
    best.into_values().collect()
}

fn beats(have: &UserRule, incoming: &UserRule) -> bool {
    match stamp(have).cmp(stamp(incoming)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => have.deleted && !incoming.deleted,
    }
}

fn normalize_handle(raw: &str) -> Result<String> {
    let handle = raw.trim().trim_start_matches('@');
    if handle.is_empty()
        || !handle
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        bail!("prefer: {raw:?} is not a handle");
    }
    Ok(handle.to_ascii_lowercase())
}

fn clean_people(people: &People) -> Result<People> {
    let mut handles = Vec::new();
    for handle in &people.handles {
        let handle = normalize_handle(handle)?;
        if !handles.contains(&handle) {
            handles.push(handle);
        }
    }
    let mut names = Vec::new();
    for name in &people.names {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            bail!("prefer: a name needs letters");
        }
        if !names
            .iter()
            .any(|kept: &String| kept.eq_ignore_ascii_case(&name))
        {
            names.push(name);
        }
    }
    Ok(People { handles, names })
}

fn fresh(text: &str, reason: &str, from: Option<String>) -> Result<UserRule> {
    let text = text.trim();
    let reason = reason.trim();
    if text.is_empty() {
        bail!("prefer: a user rule needs the words it states");
    }
    if reason.is_empty() {
        bail!("prefer: a user rule needs a reason, so say why with --why");
    }
    let now = super::now_utc();
    Ok(UserRule {
        id: new_id(text),
        text: text.to_string(),
        author: super::holder_name(),
        reason: reason.to_string(),
        pattern: None,
        verdict: None,
        deny_mention: People::default(),
        deny_reply_to: People::default(),
        written_at: now.clone(),
        updated_at: now,
        deleted: false,
        from,
        basis: None,
        lesson_hash: None,
        due_at: None,
    })
}

/// Write one prose rule. It is not also posted to the seat pack.
///
/// # Errors
///
/// The text or the reason is empty, a handle is not a login, or the file
/// cannot be written.
pub fn write_preference(
    text: &str,
    reason: &str,
    deny_mention: &People,
    deny_reply_to: &People,
) -> Result<UserRule> {
    // A person typing the command is the words. Under a runner the words
    // have to be in the prompt the hook kept, the same check MCP makes,
    // and the process has to sit under the runner the person started, not
    // a copy a tool call detached.
    if super::under_a_runner() {
        if let Err(why) = super::proc_tree::placed_runner() {
            bail!("prefer: {why}; only the person's own words can be stored here");
        }
        super::refuse_unless_person_words("prefer", text)?;
    }
    let mut rule = fresh(text, reason, None)?;
    rule.basis = Some(super::Basis::of_typed(text).as_str().to_string());
    rule.deny_mention = clean_people(deny_mention)?;
    rule.deny_reply_to = clean_people(deny_reply_to)?;
    append(rule)
}

/// Write one argv law into the user file, under the same checks as a seat
/// rule, with `basis` as the caller decided it, as a seat rule takes it.
///
/// # Errors
///
/// [`super::validate_rule`] refuses the pattern, or the file cannot be written.
pub fn write_argv(rule: &Rule, basis: super::Basis) -> Result<UserRule> {
    super::validate_rule(rule)?;
    let mut row = fresh(rule.reason.trim(), rule.reason.trim(), None)?;
    row.pattern = Some(rule.pattern.trim().to_string());
    row.verdict = Some(rule.verdict.clone());
    row.basis = Some(basis.as_str().to_string());
    append(row)
}

/// Copy one lesson atom into a user rule. The atom is not modified.
///
/// # Errors
///
/// The atom is not a lesson, its text or the reason is empty, or the file
/// cannot be written.
pub fn promote_atom(
    atom: &Value,
    reason: &str,
    deny_mention: &People,
    deny_reply_to: &People,
) -> Result<UserRule> {
    let kind = atom.get("kind").and_then(Value::as_str).unwrap_or("");
    if kind != "lesson" {
        let id = atom.get("id").and_then(Value::as_str).unwrap_or("(no id)");
        bail!("prefer: {id} is a {kind}, and only a lesson is promoted");
    }
    let id = atom
        .get("id")
        .and_then(Value::as_str)
        .context("prefer: the lesson has no id")?
        .to_string();
    let text = atom
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let mut rule = fresh(text, reason, Some(id))?;
    rule.lesson_hash = Some(lesson_hash(atom));
    rule.deny_mention = clean_people(deny_mention)?;
    rule.deny_reply_to = clean_people(deny_reply_to)?;
    append(rule)
}

/// Sha256 of what makes lesson record `atom` the lesson it is: its kind,
/// its text, its `ts` and `valid_from`, and the origin it was stored
/// under as [`super::stored_by`] reads it. packsetd keeps the first `ts`
/// when a tombstoned id is posted again and stamps a new `valid_from`, so
/// a repost is another lesson even in the same words, as are an edit of
/// the text and an origin changed in place. Links, grades and the update
/// stamp move on writes that leave the lesson as it was, and are not read.
#[must_use]
pub fn lesson_hash(atom: &Value) -> String {
    use sha2::{Digest, Sha256};
    let field = |key: &str| atom.get(key).and_then(Value::as_str).unwrap_or("");
    let mut hash = Sha256::new();
    for part in [
        field("kind"),
        field("text"),
        field("ts"),
        field("valid_from"),
        super::stored_by(atom).origin(),
    ] {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hex::encode(hash.finalize())
}

/// Tombstone `id`. The author and the reason stay, so a later sync can say
/// who removed it and why the rule stood. The lesson, if this row was
/// promoted, is not touched.
///
/// # Errors
///
/// No such row, or the file cannot be written.
pub fn forget(id: &str) -> Result<UserRule> {
    let id = id.trim();
    if id.is_empty() {
        bail!("prefer: a user rule id is required");
    }
    let mut rules = read_all()?;
    let Some(rule) = rules.iter_mut().find(|rule| rule.id == id) else {
        bail!("prefer: no user rule {id}");
    };
    if !rule.deleted {
        rule.deleted = true;
        rule.updated_at = super::now_utc();
        let written = rule.clone();
        store(&rules)?;
        sync_after_write();
        return Ok(written);
    }
    Ok(rule.clone())
}

/// Read lesson `id` from this seat's pack and copy it into a user rule.
/// The lesson stays in the pack. Under a runner, or with `person_words`
/// (the MCP path), the lesson's text must be in the person's latest
/// prompt, as [`write_preference`] asks of the words.
///
/// # Errors
///
/// The pack does not answer, no such lesson, the text is not the
/// person's words when they are required, or [`promote_atom`] refuses it.
pub fn promote_lesson(
    id: &str,
    reason: &str,
    deny_mention: &People,
    deny_reply_to: &People,
    person_words: bool,
) -> Result<UserRule> {
    let id = id.trim();
    if id.is_empty() {
        bail!("prefer: a lesson id is required");
    }
    let client = super::pack()?;
    let atoms =
        super::atoms_lean(&client, &client.workspace()).context("prefer: GET /v1/atoms failed")?;
    let atom = atoms
        .into_iter()
        .find(|atom| atom.get("id").and_then(Value::as_str) == Some(id))
        .with_context(|| format!("prefer: no atom {id} in this seat's pack"))?;
    if person_words || super::under_a_runner() {
        if super::under_a_runner() {
            if let Err(why) = super::proc_tree::placed_runner() {
                bail!("prefer: {why}; only the person's own words can be stored here");
            }
        }
        let text = atom.get("text").and_then(Value::as_str).unwrap_or("");
        super::refuse_unless_person_words("prefer", text)?;
    }
    promote_atom(&atom, reason, deny_mention, deny_reply_to)
}

fn sync_budget_ms(default: u64) -> u64 {
    std::env::var("LJOS_USER_RULES_SYNC_MS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|ms: &u64| *ms > 0)
        .unwrap_or(default)
}

fn sync_after_write() {
    let budget = std::time::Duration::from_millis(sync_budget_ms(4_000));
    let host = super::sync::host();
    for root in super::sync::user_rule_trackers() {
        let _ = super::sync::exchange_user_rules(&root, &host, budget, false);
    }
}

/// Pull and push before a hook builds its line. A tracker that does not
/// answer is ignored, and the local file is what the hook shows.
pub fn sync_for_hook() {
    // The tracker exchange is not a pack read. The side budget is for a
    // quiet pack socket. A fetch slower than that budget still has to
    // finish, or a rule written on another machine never arrives and the
    // pack is never asked.
    let budget = std::time::Duration::from_millis(sync_budget_ms(1_500));
    let host = super::sync::host();
    for root in super::sync::user_rule_trackers() {
        let _ = super::sync::exchange_user_rules(&root, &host, budget, false);
    }
}

/// One pull and push, as `ljos prefer --user --sync` runs it.
///
/// # Errors
///
/// Sync is switched off, no tracker is configured, or the tracker did not
/// answer inside the budget.
pub fn sync_now() -> Result<String> {
    if std::env::var("LJOS_USER_RULES_SYNC").ok().as_deref() == Some("off") {
        bail!("prefer: user-rule sync is off");
    }
    let roots = super::sync::user_rule_trackers();
    if roots.is_empty() {
        bail!(
            "prefer: no tracker repository to sync user rules through. Set VISSUE_ROOT, or repos in ~/.config/ljos/sync.toml"
        );
    }
    let budget = std::time::Duration::from_millis(sync_budget_ms(8_000));
    let host = super::sync::host();
    let mut out = String::new();
    for root in roots {
        out.push_str(&super::sync::exchange_user_rules(
            &root, &host, budget, true,
        )?);
    }
    if out.is_empty() {
        out.push_str("sync: user rules: none\n");
    }
    Ok(out)
}

/// The rule as one line a person reads after writing it.
#[must_use]
pub fn ack(rule: &UserRule) -> String {
    if rule.deleted {
        format!("user rule {}\tremoved\t{}", rule.id, rule.text)
    } else {
        format!("user rule {}\t{}", rule.id, rule.text)
    }
}

/// The rule as the object an MCP tool returns.
#[must_use]
pub fn to_value(rule: &UserRule) -> Value {
    serde_json::to_value(rule).unwrap_or(Value::Null)
}

/// Text lines for `ljos rules`, empty when there are none. A row held
/// from before a basis was kept is listed apart from a promoted rule the
/// pack does not confirm, which is unverified, with why.
#[must_use]
pub fn format_list() -> String {
    let Ok(rules) = load() else {
        return String::new();
    };
    let (mut persons, mut held, mut unverified) = (String::new(), String::new(), String::new());
    for rule in &rules {
        let law = match (&rule.verdict, &rule.pattern) {
            (Some(verdict), Some(pattern)) => format!("{verdict}\t{pattern}\t"),
            _ => String::new(),
        };
        let line = format!("{law}{}\t{}\t{}", rule.text, rule.author, rule.reason);
        if is_held(rule) {
            held.push_str(&format!("held\t{line}\n"));
            continue;
        }
        match unconfirmed(rule) {
            None => persons.push_str(&format!("user\t{line}\n")),
            Some(why) => unverified.push_str(&format!("unverified\t{line}\t{why}\n")),
        }
    }
    let mut out = String::new();
    if !persons.is_empty() {
        out.push_str("user rules (shared, never reviewed):\n");
        out.push_str(&persons);
    }
    if !held.is_empty() {
        out.push_str("user rules held from before a basis was kept:\n");
        out.push_str(&held);
    }
    if !unverified.is_empty() {
        out.push_str("user rules not confirmed as the person's (unverified):\n");
        out.push_str(&unverified);
    }
    out
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn omission(left: usize) -> String {
    if left == 0 {
        String::new()
    } else {
        format!("{left} user rules omitted.\n")
    }
}

/// The preface injected ahead of seat memory. Empty when there are no rules
/// the preface may show. A held row is shown in its own section, not as a
/// rule the person confirmed.
#[must_use]
pub fn preface() -> String {
    let rules = load().unwrap_or_default();
    let standing: Vec<UserRule> = rules
        .iter()
        .filter(|rule| persons_rule(rule))
        .cloned()
        .collect();
    let held: Vec<UserRule> = rules.iter().filter(|rule| is_held(rule)).cloned().collect();
    render_preface(&standing, &held, PREFACE_BUDGET)
}

/// Whether the preface may state `rule` as the person's. A rule written
/// as words or an argv law is when its basis is the person's. A rule
/// promoted from a lesson is only when this read got
/// that lesson from this seat's pack just now, under its id, stored as
/// the person's, and it is the record the rule was promoted from: its
/// [`lesson_hash`] is the one the row kept. A row with no hash kept has
/// nothing to match. Anything less leaves the rule out of the preface,
/// not stated as unverified: the preface is read as the person's, and a
/// pack that is slow, down or answers for another atom confirms nothing.
fn persons_rule(rule: &UserRule) -> bool {
    unconfirmed(rule).is_none()
}

/// Why `rule` may not be read as the person's, or `None` when it may: the
/// test [`persons_rule`] states.
fn unconfirmed(rule: &UserRule) -> Option<String> {
    let Some(id) = rule.from.as_deref() else {
        return (rule.basis.as_deref() != Some(super::Basis::Person.as_str())).then(|| {
            "written without the person at the terminal or in their latest prompt".to_string()
        });
    };
    let Some(kept) = rule.lesson_hash.as_deref() else {
        return Some(format!(
            "promoted from lesson {id} with no hash of its record kept; promote it again"
        ));
    };
    match super::atom_now(id) {
        Ok(Some(lesson)) if !super::person_stored(&lesson) => Some(format!(
            "the pack does not hold lesson {id} as the person's"
        )),
        Ok(Some(lesson)) if lesson_hash(&lesson) != kept => Some(format!(
            "lesson {id} in the pack is not the record this rule was promoted from"
        )),
        Ok(Some(_)) => None,
        Ok(None) => Some(format!("the pack does not hold lesson {id}")),
        Err(why) => Some(why),
    }
}

/// `$XDG_CONFIG_HOME/ljos/user-rule-basis-cutoff`. One RFC 3339 instant,
/// written once. Hooks do not read the pack to decide this.
fn cutoff_path() -> Option<PathBuf> {
    Some(
        super::config_base()?
            .join("ljos")
            .join("user-rule-basis-cutoff"),
    )
}

fn parse_instant(raw: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let raw = raw.trim();
    (!raw.is_empty())
        .then(|| chrono::DateTime::parse_from_rfc3339(raw).ok())
        .flatten()
}

fn cutoff_instant() -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let text = std::fs::read_to_string(cutoff_path()?).ok()?;
    text.lines().find_map(parse_instant)
}

/// Record the cutoff for user-rule basis, once, from a person-side command.
///
/// The instant is this run's time. A preference cutoff from the earlier
/// upgrade is not used: user rules written after that receipt still had
/// no basis, and treating them as new would drop them. Under a runner
/// this does nothing. A file that is already there is left as it is.
pub fn note_cutoff() {
    if super::under_a_runner() {
        return;
    }
    let Some(path) = cutoff_path() else {
        return;
    };
    if path.exists() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, format!("{}\n", super::now_utc()));
}

/// A row from before basis was kept: no `basis`, no lesson it was promoted
/// from, and either no cutoff yet or a `written_at` older than it. A stamp
/// that does not parse is treated as older, so a row is not dropped because
/// its time cannot be read. A basis of `agent`, or any other basis, is a
/// decision already made and is not held.
fn is_held(rule: &UserRule) -> bool {
    if rule.from.is_some() || rule.basis.is_some() {
        return false;
    }
    match cutoff_instant() {
        None => true,
        Some(cutoff) => parse_instant(&rule.written_at).is_none_or(|ts| ts < cutoff),
    }
}

/// How many user rules [`is_held`] counts. Session start and the doctor
/// report this. Neither path is required to have written the cutoff.
#[must_use]
pub fn held_count() -> usize {
    load()
        .unwrap_or_default()
        .iter()
        .filter(|rule| is_held(rule))
        .count()
}

/// The sentence the doctor and `ljos prefer --user --review` share.
#[must_use]
pub fn review_sentence(n: usize) -> String {
    format!(
        "{n} older user rule{} await{} review (`ljos prefer --user --review`)",
        if n == 1 { "" } else { "s" },
        if n == 1 { "s" } else { "" }
    )
}

/// List the user rules the upgrade held back.
///
/// # Errors
///
/// The user-rule file cannot be read.
pub fn review_list() -> Result<String> {
    note_cutoff();
    let rules = load()?;
    let held: Vec<&UserRule> = rules.iter().filter(|rule| is_held(rule)).collect();
    if held.is_empty() {
        return Ok("no older user rules await review\n".to_string());
    }
    let mut out = format!("{}\n", review_sentence(held.len()));
    for rule in held {
        out.push_str(&format!(
            "{}  {}  {}\n  ljos prefer --user --review --keep {}\n",
            rule.id,
            rule.written_at,
            one_line(&rule.text),
            rule.id
        ));
    }
    Ok(out)
}

/// Keep one held user rule as the person's.
///
/// A runner is refused, and so is anything [`super::person_present`] does
/// not prove. The basis becomes `person` and `updated_at` moves, so a sync
/// keeps this copy.
///
/// # Errors
///
/// The id is not a held user rule, this process is under a runner, or the
/// person is not at its terminal.
pub fn keep_held(id: &str) -> Result<String> {
    if super::under_a_runner() {
        bail!(
            "prefer: a runner cannot keep a user rule. \
             Run `ljos prefer --user --review --keep {id}` from a terminal of your own"
        );
    }
    if !super::person_present() {
        bail!(
            "prefer: keeping a user rule needs a terminal. \
             Run `ljos prefer --user --review --keep {id}` from a terminal of your own"
        );
    }
    note_cutoff();
    let mut rules = read_all()?;
    let ids: Vec<String> = rules
        .iter()
        .filter(|rule| is_held(rule))
        .map(|rule| rule.id.clone())
        .collect();
    let id = super::resolve_short_id("prefer", id.trim(), &ids)?;
    let Some(rule) = rules.iter_mut().find(|rule| rule.id == id && is_held(rule)) else {
        bail!("prefer: {id} is not an older user rule awaiting review");
    };
    rule.basis = Some(super::Basis::Person.as_str().to_string());
    rule.updated_at = super::now_utc();
    let shown = format!("user rule {}\tkept\t{}\n", rule.id, rule.text);
    store(&rules)?;
    Ok(shown)
}

fn render_preface(standing: &[UserRule], held: &[UserRule], budget: usize) -> String {
    if budget == 0 || (standing.is_empty() && held.is_empty()) {
        return String::new();
    }
    let mut out = if standing.is_empty() {
        String::new()
    } else {
        render(standing, budget)
    };
    if held.is_empty() {
        return out;
    }
    let gap = usize::from(!out.is_empty());
    let room = budget.saturating_sub(out.chars().count() + gap);
    let rest = render_headed(
        held,
        "Older user rules, not confirmed (`ljos prefer --user --review`):\n",
        room,
    );
    if rest.is_empty() {
        return out;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&rest);
    if out.chars().count() > budget {
        out = out.chars().take(budget).collect();
    }
    out.trim_end().to_string()
}

fn render(rules: &[UserRule], budget: usize) -> String {
    render_headed(
        rules,
        "User rules (standing, shared by every seat on this machine):\n",
        budget,
    )
}

fn render_headed(rules: &[UserRule], header: &str, budget: usize) -> String {
    if rules.is_empty() || budget == 0 {
        return String::new();
    }
    let mut body = String::new();
    let mut shown = 0usize;
    for rule in rules {
        let line = format!("- {}\n", one_line(&rule.text));
        let note = omission(rules.len() - (shown + 1));
        let used = header.chars().count()
            + body.chars().count()
            + line.chars().count()
            + note.chars().count();
        if used <= budget {
            body.push_str(&line);
            shown += 1;
            continue;
        }
        if shown == 0 {
            let note = omission(rules.len().saturating_sub(1));
            let room = budget.saturating_sub(header.chars().count() + note.chars().count());
            let take = room.saturating_sub(6);
            if take > 0 {
                let cut: String = one_line(&rule.text).chars().take(take).collect();
                body.push_str(&format!("- {cut}...\n"));
                shown = 1;
            }
        }
        break;
    }
    if shown == 0 {
        return String::new();
    }
    let mut out = format!("{header}{body}");
    let left = rules.len() - shown;
    if left > 0 {
        out.push_str(&omission(left));
    }
    if out.chars().count() > budget {
        out = out.chars().take(budget).collect();
    }
    out.trim_end().to_string()
}

/// `body`, with the user-rule preface in front. An empty preface leaves
/// `body` as it is.
#[must_use]
pub fn prepend(body: String) -> String {
    let head = preface();
    if head.is_empty() {
        body
    } else if body.is_empty() {
        head
    } else {
        format!("{head}\n{body}")
    }
}

/// `text` with one leading preface removed, so a session line and a prompt
/// note that both carry it are not pasted twice.
#[must_use]
pub fn without_preface(text: &str) -> String {
    let head = preface();
    if head.is_empty() {
        return text.to_string();
    }
    text.trim_start()
        .strip_prefix(&head)
        .map(|rest| rest.trim_start().to_string())
        .unwrap_or_else(|| text.to_string())
}

fn as_argv(rule: &UserRule) -> Option<Rule> {
    Some(Rule {
        pattern: rule.pattern.clone()?,
        verdict: rule.verdict.clone().unwrap_or_else(|| "ask".to_string()),
        reason: rule.reason.clone(),
    })
}

fn fired(rule: &UserRule) -> Rule {
    let why = rule.reason.trim();
    let text = rule.text.trim();
    let reason = if text.is_empty() || text == why {
        why.to_string()
    } else {
        format!("{text} ({why})")
    };
    reported(
        rule,
        Rule {
            pattern: format!("user-rule:{}", rule.id),
            verdict: "deny".into(),
            reason,
        },
    )
}

/// `law`, fired by `rule`, as a guard reports it. A promoted rule the pack
/// does not confirm still fires, with a reason that says it is unverified
/// rather than the person's: a deny or an ask only takes a command away,
/// so honouring an unconfirmed one never widens what the agent may run,
/// while dropping it would let a slow, down or forged pack lift a deny.
fn reported(rule: &UserRule, mut law: Rule) -> Rule {
    if let Some(why) = unconfirmed(rule) {
        law.reason = format!(
            "unverified, not confirmed as the person's rule ({why}): {}",
            law.reason
        );
    }
    law
}

/// Whether `rule` is an outbound-post denial, the only verdict
/// `beforeMCPExecution` enforces.
#[must_use]
pub fn mcp_event_denies(event: &str, rule: &Rule) -> bool {
    event == "beforeMCPExecution"
        && rule.verdict == "deny"
        && rule.pattern.starts_with("user-rule:")
}

struct Post {
    bodies: Vec<String>,
    reply_values: Vec<String>,
}

/// Say a failed user-rules read once per process. Each hook is its own
/// process, so a burst of calls does not repeat the line.
fn note_rules_read_error(err: &anyhow::Error) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static NOTED: AtomicBool = AtomicBool::new(false);
    if NOTED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!("ljos: {err:#}");
}

/// A GitHub write the rules cover, refused because the file could not be
/// used. The reason names the file. The pattern keeps the MCP deny.
fn github_write_blocked(err: &anyhow::Error) -> Rule {
    note_rules_read_error(err);
    Rule {
        pattern: "user-rule:file".into(),
        verdict: "deny".into(),
        reason: format!("user rules could not be read ({err:#}), so this GitHub write is refused"),
    }
}

/// The user-rule verdict for a shell line, if one fires.
#[must_use]
pub fn guard_line(line: &str) -> Option<Rule> {
    if line.trim().is_empty() {
        return None;
    }
    let rules = match load() {
        Ok(rules) => rules,
        Err(err) => {
            if posts_of(line).is_empty() {
                return None;
            }
            return Some(github_write_blocked(&err));
        }
    };
    if rules.is_empty() {
        return None;
    }
    let (rows, argv): (Vec<&UserRule>, Vec<Rule>) = rules
        .iter()
        .filter_map(|rule| Some((rule, as_argv(rule)?)))
        .unzip();
    // The rule verdict_for gives a line nested too deep is no row.
    let report = |law: &Rule| match argv.iter().position(|row| std::ptr::eq(row, law)) {
        Some(at) => reported(rows[at], law.clone()),
        None => law.clone(),
    };
    if let Some(hit) = super::verdict_for(&argv, line).filter(|rule| rule.verdict == "deny") {
        return Some(report(hit));
    }
    if let Some(hit) = outbound(line, &rules) {
        return Some(hit);
    }
    super::verdict_for(&argv, line).map(report)
}

/// The user-rule verdict for a hook payload that names a tool, if the tool
/// is a GitHub comment, review, issue or pull-request write.
#[must_use]
pub fn guard_payload(raw: &str) -> Option<Rule> {
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    let tool = value
        .get("tool_name")
        .or_else(|| value.get("toolName"))
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/toolCall/name").and_then(Value::as_str))?;
    let server = ["mcp_server_name", "mcpServerName", "server_name", "server"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str));
    // The MCP server binary. A `command` inside tool_input is an argument
    // of the tool, not this field.
    let command = value.get("command").and_then(Value::as_str);
    let input = value
        .get("tool_input")
        .or_else(|| value.get("toolInput"))
        .or_else(|| value.pointer("/toolCall/args"));
    let parsed = match input {
        Some(Value::String(text)) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        Some(other) => other.clone(),
        None => Value::Null,
    };
    guard_mcp(tool, server, command, &parsed)
}

fn guard_mcp(
    tool: &str,
    server: Option<&str>,
    command: Option<&str>,
    input: &Value,
) -> Option<Rule> {
    if !github_write(tool, server, command) && !cursor_http_mcp_write(tool, input) {
        return None;
    }
    let rules = match load() {
        Ok(rules) => rules,
        Err(err) => return Some(github_write_blocked(&err)),
    };
    let post = mcp_fields(input, tool);
    rules.iter().find(|rule| post_hits(rule, &post)).map(fired)
}

fn github_server(name: &str) -> bool {
    name.to_ascii_lowercase().contains("github")
}

/// Cursor names an HTTP MCP tool `MCP:<tool>` and sends no server field.
/// A write verb with `owner` and `repo` is the same GitHub write as
/// `mcp__github__<tool>`.
fn cursor_http_mcp_write(tool: &str, input: &Value) -> bool {
    let Some(short) = tool.strip_prefix("MCP:") else {
        return false;
    };
    if short.is_empty() {
        return false;
    }
    let short = short.to_ascii_lowercase();
    if short.starts_with("get_")
        || short.starts_with("list_")
        || short.starts_with("search_")
        || short.starts_with("read_")
        || short.contains("_read")
    {
        return false;
    }
    if !["comment", "review", "issue", "pull"]
        .iter()
        .any(|kind| short.contains(kind))
    {
        return false;
    }
    let Some(obj) = input.as_object() else {
        return false;
    };
    obj.contains_key("owner") && obj.contains_key("repo")
}

fn github_write(tool: &str, server: Option<&str>, command: Option<&str>) -> bool {
    // The seat's own tool on its configured server is not a GitHub write,
    // even when that server key contains "github". A payload command that
    // is not the seat server is a foreign tool.
    if crate::subagent::is_confirmed_seat_call(tool, server, command) {
        return false;
    }
    let tool_l = tool.to_ascii_lowercase();
    let embedded = crate::subagent::mcp_prefixed_tool(&tool_l).map(|(name, _)| name.to_string());
    let short = tool_l.rsplit("__").next().unwrap_or(&tool_l);
    let short = short.rsplit('/').next().unwrap_or(short);
    let server_github =
        server.is_some_and(github_server) || embedded.as_deref().is_some_and(github_server);
    let githubish = server_github || tool_l.contains("github");
    if !githubish {
        return false;
    }
    if short.starts_with("get_")
        || short.starts_with("list_")
        || short.starts_with("search_")
        || short.starts_with("read_")
        || short.contains("_read")
    {
        return false;
    }
    // mcp__github__ljos_NAME is a GitHub server wearing the seat's prefix.
    if server_github && short.starts_with("ljos_") {
        return true;
    }
    ["comment", "review", "issue", "pull"]
        .iter()
        .any(|kind| short.contains(kind))
}

fn outbound(line: &str, rules: &[UserRule]) -> Option<Rule> {
    let posts = posts_of(line);
    if posts.is_empty() {
        return None;
    }
    rules.iter().find_map(|rule| {
        posts
            .iter()
            .any(|post| post_hits(rule, post))
            .then(|| fired(rule))
    })
}

fn post_hits(rule: &UserRule, post: &Post) -> bool {
    let mentioned = post
        .bodies
        .iter()
        .any(|body| mentions(&rule.deny_mention, body));
    let replies = post
        .reply_values
        .iter()
        .any(|value| identifies(&rule.deny_reply_to, value));
    mentioned || replies
}

fn posts_of(line: &str) -> Vec<Post> {
    let mut out = Vec::new();
    for sub in super::nested_lines(line, 0) {
        for raw in super::raw_segments(&sub) {
            let words = super::shell_words(&raw);
            if words.is_empty() {
                continue;
            }
            let (at, _) = super::prefix_end(&words);
            if at >= words.len() {
                continue;
            }
            if let Some(post) = gh_post(&words[at..]) {
                out.push(post);
            }
        }
    }
    out
}

fn gh_post(words: &[String]) -> Option<Post> {
    let bin = words.first()?.rsplit(['/', '\\']).next()?;
    if bin != "gh" {
        return None;
    }
    let args = skip_global(&words[1..]);
    let group = args.first()?.as_str();
    let verb = args.get(1).map(String::as_str);
    let write = match (group, verb) {
        ("pr" | "issue", Some("comment" | "review" | "create" | "edit")) => true,
        ("api", _) => api_write(&args[1..]),
        _ => false,
    };
    if !write {
        return None;
    }
    Some(Post {
        bodies: bodies_of(&args),
        reply_values: reply_values_of(&args),
    })
}

fn skip_global(words: &[String]) -> Vec<String> {
    let mut i = 0;
    while i < words.len() {
        let word = words[i].as_str();
        if word == "--" {
            return words[i + 1..].to_vec();
        }
        if !word.starts_with('-') {
            break;
        }
        let takes_value = matches!(word, "-R" | "--repo" | "--hostname");
        i += if word.contains('=') || !takes_value {
            1
        } else {
            2
        };
    }
    words[i..].to_vec()
}

fn api_write(args: &[String]) -> bool {
    matches!(method_of(args).as_deref(), Some("POST" | "PATCH" | "PUT"))
}

fn method_of(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let word = args[i].as_str();
        if let Some(value) = word
            .strip_prefix("--method=")
            .or_else(|| word.strip_prefix("-X="))
        {
            return Some(value.to_ascii_uppercase());
        }
        if word == "--method" || word == "-X" {
            return args.get(i + 1).map(|value| value.to_ascii_uppercase());
        }
        if let Some(value) = word.strip_prefix("-X") {
            if !value.is_empty() && !value.starts_with('-') {
                return Some(value.to_ascii_uppercase());
            }
        }
        i += 1;
    }
    None
}

const BODY_FLAGS: &[&str] = &["--body", "-b", "--title", "-t", "--subject"];
const FIELD_FLAGS: &[&str] = &["-f", "-F", "--field", "--raw-field"];
const BODY_KEYS: &[&str] = &["body", "title", "comment", "review"];
const REPLY_KEYS: &[&str] = &["in_reply_to", "in_reply_to_user", "reply_to", "reply-to"];

fn bodies_of(args: &[String]) -> Vec<String> {
    let mut out = flag_values(args, BODY_FLAGS);
    for (key, value) in assignments(args) {
        if BODY_KEYS.iter().any(|name| key.eq_ignore_ascii_case(name)) {
            out.push(value);
        }
    }
    out
}

fn reply_values_of(args: &[String]) -> Vec<String> {
    let mut out = flag_values(args, &["--reply-to", "--reply-to-user"]);
    for (key, value) in assignments(args) {
        if REPLY_KEYS.iter().any(|name| key.eq_ignore_ascii_case(name)) {
            out.push(value);
        }
    }
    for arg in args {
        if let Some((_, query)) = arg.split_once('?') {
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    if REPLY_KEYS.iter().any(|name| key.eq_ignore_ascii_case(name)) {
                        out.push(value.to_string());
                    }
                }
            }
        }
    }
    out
}

fn assignments(args: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let word = &args[i];
        if FIELD_FLAGS.contains(&word.as_str()) {
            if let Some(next) = args.get(i + 1) {
                if let Some((key, value)) = next.split_once('=') {
                    out.push((key.to_string(), value.to_string()));
                }
            }
            i += 2;
            continue;
        }
        if let Some(rest) = word
            .strip_prefix("--raw-field=")
            .or_else(|| word.strip_prefix("--field="))
        {
            if let Some((key, value)) = rest.split_once('=') {
                out.push((key.to_string(), value.to_string()));
            }
        }
        i += 1;
    }
    out
}

fn flag_values(args: &[String], names: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let word = &args[i];
        if let Some((key, value)) = word.split_once('=') {
            if names.contains(&key) && !value.is_empty() {
                out.push(value.to_string());
            }
        }
        if names.contains(&word.as_str()) {
            if let Some(next) = args.get(i + 1) {
                if !next.starts_with('-') {
                    out.push(next.clone());
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn mcp_fields(input: &Value, tool: &str) -> Post {
    let mut bodies = Vec::new();
    let mut reply_values = Vec::new();
    collect_keyed(
        input,
        &["body", "comment", "title", "review", "message", "content"],
        &mut bodies,
    );
    collect_keyed(
        input,
        &[
            "in_reply_to",
            "in_reply_to_user",
            "inReplyTo",
            "reply_to",
            "replyTo",
            "user",
            "author",
            "reviewer",
        ],
        &mut reply_values,
    );
    if tool.to_ascii_lowercase().contains("reply") {
        collect_strings(input, &mut reply_values);
    }
    Post {
        bodies,
        reply_values,
    }
}

fn collect_keyed(value: &Value, keys: &[&str], out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if keys.iter().any(|name| key.eq_ignore_ascii_case(name)) {
                    collect_strings(child, out);
                } else {
                    collect_keyed(child, keys, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_keyed(child, keys, out);
            }
        }
        _ => {}
    }
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => {
            for child in items {
                collect_strings(child, out);
            }
        }
        Value::Object(map) => {
            for child in map.values() {
                collect_strings(child, out);
            }
        }
        _ => {}
    }
}

fn mentions(people: &People, text: &str) -> bool {
    people
        .handles
        .iter()
        .any(|handle| mentions_handle(text, handle))
        || people.names.iter().any(|name| mentions_name(text, name))
}

fn identifies(people: &People, value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let bare = value.trim_start_matches('@').to_ascii_lowercase();
    people.handles.iter().any(|handle| {
        let handle = handle.trim().trim_start_matches('@').to_ascii_lowercase();
        !handle.is_empty() && (bare == handle || mentions_handle(value, &handle))
    }) || people.names.iter().any(|name| mentions_name(value, name))
}

fn mentions_handle(text: &str, handle: &str) -> bool {
    let handle = handle.trim().trim_start_matches('@').to_ascii_lowercase();
    if handle.is_empty() {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    let needle = format!("@{handle}");
    let bytes = lower.as_bytes();
    let mark = needle.as_bytes();
    let mut i = 0;
    while i + mark.len() <= bytes.len() {
        if &bytes[i..i + mark.len()] == mark {
            let after = bytes.get(i + mark.len()).copied();
            if after.is_none_or(|c| !is_handle_char(c as char)) {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_handle_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

fn mentions_name(text: &str, name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let mut i = 0;
    while i + name.len() <= text.len() {
        if text[i..i + name.len()] == name[..] {
            let before = i == 0 || !text[i - 1].is_alphanumeric();
            let after = i + name.len();
            let after_ok = after == text.len() || !text[after].is_alphanumeric();
            if before && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated() -> (super::super::EnvLock, tempfile::TempDir) {
        let lock = super::super::test_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
            std::env::set_var("HOME", dir.path());
            std::env::set_var("LJOS_GUARD", "on");
            std::env::set_var("LJOS_HOLD", "rohit");
            std::env::set_var("LJOS_SEAT", "claude");
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("PACKSET_WORKSPACE", "seat-a");
            std::env::set_var("LJOS_USER_RULES_SYNC", "off");
        }
        super::super::PERSON_AT_TERMINAL.with(|p| p.set(true));
        (lock, dir)
    }

    fn alan() -> UserRule {
        write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People {
                handles: vec!["ocaisa".into()],
                names: vec!["Alan".into()],
            },
            &People {
                handles: vec!["@ocaisa".into()],
                names: vec!["Alan".into()],
            },
        )
        .unwrap()
    }

    /// These tests write a user rule as the person at a terminal. A
    /// runner's variables on the shell that runs `cargo test` must not make
    /// them an agent's, so each is run again in a child that carries them.
    #[test]
    fn the_user_rule_tests_pass_under_a_runner_environment() {
        let runner: &[(&str, &str)] = &[
            ("CURSOR_CONVERSATION_ID", "0199a1b2-c3d4-e5f6-runner"),
            ("CURSOR_AGENT", "1"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_SESSION_ID", "0199a1b2-claude-session"),
            ("CODEX_THREAD_ID", "0199a1b2-codex-thread"),
        ];
        for test in [
            "a_user_rule_is_surfaced_in_a_second_seat",
            "the_guard_denies_a_mention_and_a_reply_and_allows_another_comment",
            "two_homes_with_different_rules_converge",
        ] {
            let test = format!("user_rules::tests::{test}");
            for env in [runner, &[("CURSOR_CONVERSATION_ID", "x")]] {
                let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
                cmd.args(["--exact", &test, "--test-threads=1"]);
                for (key, value) in env {
                    cmd.env(key, value);
                }
                let out = cmd.output().unwrap();
                let said = String::from_utf8_lossy(&out.stdout).into_owned()
                    + &String::from_utf8_lossy(&out.stderr);
                assert!(out.status.success(), "{test} under {env:?}:\n{said}");
                assert!(said.contains("1 passed"), "{test} under {env:?}:\n{said}");
            }
        }
    }

    #[test]
    fn a_user_rule_is_surfaced_in_a_second_seat() {
        let (_lock, dir) = isolated();
        let written = alan();
        assert_eq!(written.author, "rohit");
        assert!(written.due_at.is_none());
        unsafe {
            std::env::set_var("LJOS_SEAT", "codex");
            std::env::set_var("PACKSET_WORKSPACE", "seat-b");
        }
        let path = rules_path().unwrap();
        assert!(path.starts_with(dir.path().join("config")));
        assert!(!path.to_string_lossy().contains("codex"));
        assert!(!path.to_string_lossy().contains("claude"));
        let head = preface();
        assert!(head.starts_with("User rules"), "{head}");
        assert!(head.contains("never post or reply to Alan"), "{head}");
        let start = super::super::session_start_for(Some("seat-two"));
        assert!(start.starts_with("User rules"), "{start}");
        let at = start.find("never post or reply to Alan").unwrap();
        let on = start.find("ljos is on").unwrap();
        assert!(at < on, "{start}");
        let call = super::super::HookCall {
            event: "UserPromptSubmit".into(),
            cue: "open an easybuild pull request".into(),
            session: Some("seat-two".into()),
            shape: super::super::HookShape::Asks,
        };
        let (note, ids) = super::super::hook_note(&call, 5);
        assert!(note.starts_with("User rules"), "{note}");
        assert!(note.contains("never post or reply to Alan"), "{note}");
        assert!(ids.iter().all(|id| !id.starts_with("ur-")), "{ids:?}");
        let claude = super::super::hook_call(
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"claude-1","prompt":"open an easybuild pull request"}"#,
        );
        let (claude_note, _) = super::super::hook_note(&claude, 5);
        let claude_out = super::super::hook_output(&claude, &claude_note);
        assert!(claude_out.contains("additionalContext"), "{claude_out}");
        assert!(
            claude_out.contains("never post or reply to Alan"),
            "{claude_out}"
        );
        let agy = super::super::hook_call_as(
            r#"{"conversationId":"agy-1","invocationNum":1,"prompt":"ship the easybuild change"}"#,
            Some("PreInvocation"),
        );
        assert_eq!(agy.event, "UserPromptSubmit");
        assert_eq!(agy.shape, super::super::HookShape::Steps);
        let (agy_note, _) = super::super::hook_note(&agy, 5);
        let agy_out = super::super::hook_output(&agy, &agy_note);
        assert!(agy_out.contains("injectSteps"), "{agy_out}");
        assert!(agy_out.contains("never post or reply to Alan"), "{agy_out}");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path().join("other"));
        }
        assert!(preface().is_empty(), "a second config home has no rules");
    }

    #[test]
    fn a_user_rule_does_not_decay_after_simulated_time() {
        let (_lock, _dir) = isolated();
        let written = alan();
        let later = "2099-01-01T00:00:00Z";
        assert_eq!(sweep(later).unwrap(), 0);
        assert_eq!(review_due(later), 0);
        assert!(!is_due(&written, later));
        let kept = load().unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].text, written.text);
        assert!(kept[0].due_at.is_none());
        let raw = std::fs::read_to_string(rules_path().unwrap()).unwrap();
        assert!(!raw.contains("\"due_at\""), "{raw}");
        let mut file: File = serde_json::from_str(&raw).unwrap();
        file.rules.push(UserRule {
            id: "ur-expired".into(),
            text: "a row that was given a date".into(),
            author: "rohit".into(),
            reason: "fixture".into(),
            pattern: None,
            verdict: None,
            deny_mention: People::default(),
            deny_reply_to: People::default(),
            written_at: "2020-01-01T00:00:00Z".into(),
            updated_at: "2020-01-01T00:00:00Z".into(),
            deleted: false,
            from: None,
            basis: None,
            lesson_hash: None,
            due_at: Some("2020-01-02T00:00:00Z".into()),
        });
        std::fs::write(
            rules_path().unwrap(),
            serde_json::to_string_pretty(&file).unwrap(),
        )
        .unwrap();
        assert_eq!(sweep(later).unwrap(), 1);
        let kept = load().unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, written.id);
        assert_eq!(review_due(later), 0);
    }

    #[test]
    fn the_preface_stays_inside_its_budget_and_keeps_the_first_rule() {
        let (_lock, _dir) = isolated();
        let first = "a".repeat(80);
        write_preference(&first, "first", &People::default(), &People::default()).unwrap();
        for n in 0..30 {
            let text = format!("rule {n} {}", "b".repeat(200));
            write_preference(&text, "later", &People::default(), &People::default()).unwrap();
        }
        let head = preface();
        assert!(
            head.chars().count() <= PREFACE_BUDGET,
            "{}",
            head.chars().count()
        );
        assert!(head.contains(&first[..40]), "{head}");
        assert!(head.contains("omitted"), "{head}");
    }

    #[test]
    fn the_guard_denies_a_mention_and_a_reply_and_allows_another_comment() {
        let (_lock, _dir) = isolated();
        write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let mention = "gh pr comment 12 --body 'please ask @ocaisa'";
        assert!(
            super::super::static_guard(mention).is_none(),
            "prose alone is not a deny"
        );
        alan();
        let denied = super::super::static_guard(mention).expect("mention");
        assert_eq!(denied.verdict, "deny");
        assert!(denied.pattern.starts_with("user-rule:"), "{denied:?}");
        assert!(
            denied.reason.contains("ocaisa") || denied.reason.contains("Alan"),
            "{denied:?}"
        );
        let reply = "gh api --method POST repos/org/repo/issues/comments/7/replies -f body=seen -F in_reply_to_user=ocaisa";
        let denied = super::super::static_guard(reply).expect("reply");
        assert_eq!(denied.verdict, "deny");
        let allowed = "gh pr comment 12 --body 'tests passed'";
        assert!(
            super::super::static_guard(allowed).is_none(),
            "unrelated comment"
        );
        assert!(super::super::static_guard("gh pr comment 12 --body 'see ocaisa'").is_none());
        assert!(
            super::super::static_guard("gh pr comment 12 --body 'please ask @ocaisa2'").is_none()
        );
        assert!(
            super::super::static_guard("gh pr comment 12 --body 'alanine is a molecule'").is_none()
        );
        let named =
            super::super::static_guard("gh pr comment 12 --body 'ask Alan to look'").expect("name");
        assert_eq!(named.verdict, "deny");
        assert!(super::super::static_guard("gh api user/ocaisa").is_none());
        assert!(super::super::static_guard("echo please ask @ocaisa").is_none());
        let mcp = r#"{"hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"please ask @ocaisa","owner":"org","repo":"repo"}}"#;
        assert!(guard_payload(mcp).is_some());
        let mcp_reply = r#"{"tool_name":"add_reply_to_pull_request_comment","mcp_server_name":"github","tool_input":{"body":"seen","in_reply_to_user":"ocaisa"}}"#;
        assert!(guard_payload(mcp_reply).is_some());
        let mcp_ok = r#"{"tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"tests passed"}}"#;
        assert!(guard_payload(mcp_ok).is_none());
        let mcp_read = r#"{"tool_name":"list_issues","mcp_server_name":"github","tool_input":{"body":"@ocaisa"}}"#;
        assert!(guard_payload(mcp_read).is_none());
        let cursor = super::super::hook_call(
            r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"please ask @ocaisa"}}"#,
        );
        let verdict = guard_payload(
            r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"please ask @ocaisa"}}"#,
        )
        .unwrap();
        let out = super::super::hook_output_ruled(&cursor, "", Some(&verdict));
        assert!(out.contains("\"permission\":\"deny\""), "{out}");
        let payload = r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"please ask @ocaisa"}}"#;
        let (out, code) = super::super::cursor_side_answer(payload, None).unwrap();
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("\"permission\":\"deny\""), "{out}");
        assert!(out.contains("ocaisa") || out.contains("Alan"), "{out}");
    }

    fn rules_file() -> std::path::PathBuf {
        let path = rules_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::remove_file(&path);
        path
    }

    /// A missing file is an empty list. A FIFO, a directory, and a file
    /// over the cap fail at once, without reading the bytes.
    #[test]
    fn a_rules_file_must_be_a_small_regular_file() {
        let (_lock, _dir) = isolated();
        let path = rules_file();
        let _ = std::fs::remove_file(&path);
        assert!(load().unwrap().is_empty(), "a missing file is no rules");

        std::fs::create_dir(&path).unwrap();
        let err = load().unwrap_err();
        assert!(format!("{err:#}").contains("not a regular file"), "{err:#}");
        let _ = std::fs::remove_dir(&path);

        let file = std::fs::File::create(&path).unwrap();
        file.set_len(RULES_FILE_CAP + 1).unwrap();
        drop(file);
        let err = load().unwrap_err();
        assert!(format!("{err:#}").contains("byte cap"), "{err:#}");
        std::fs::remove_file(&path).unwrap();

        let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        // SAFETY: mkfifo creates a FIFO at this temp path with the given mode.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) }, 0);
        let started = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(load().map(|_| ()));
        });
        let err = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("load blocked on a FIFO")
            .expect_err("a FIFO is not a rules file");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "load took {:?}",
            started.elapsed()
        );
        assert!(format!("{err:#}").contains("not a regular file"), "{err:#}");
    }

    /// A zero-byte file, a blank file, and a dangling symlink are not an
    /// empty rule list. A covered GitHub write is refused and the reason
    /// names the path. A missing file and a real empty list do not.
    #[test]
    fn an_empty_rules_file_and_a_dangling_link_deny_a_github_write() {
        let (_lock, _dir) = isolated();
        let benign = r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"tests passed"}}"#;
        let read_tool = r#"{"tool_name":"list_issues","mcp_server_name":"github","tool_input":{"owner":"org"}}"#;
        let path = rules_file();
        std::fs::write(&path, "").unwrap();
        let hit = guard_payload(benign).expect("empty file allowed a write");
        assert_eq!(hit.verdict, "deny");
        assert!(
            hit.reason.contains(&path.display().to_string()),
            "{}",
            hit.reason
        );
        assert!(guard_payload(read_tool).is_none(), "a read was denied");
        let shell = guard_line("gh pr comment 12 --body 'tests passed'").unwrap();
        assert!(
            shell.reason.contains(&path.display().to_string()),
            "{}",
            shell.reason
        );
        assert!(guard_line("gh issue list").is_none(), "a read was denied");
        std::fs::write(&path, " \n").unwrap();
        let hit = guard_payload(benign).expect("blank file allowed a write");
        assert!(
            hit.reason.contains(&path.display().to_string()),
            "{}",
            hit.reason
        );
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(path.with_file_name("no-such-rules.json"), &path).unwrap();
        let hit = guard_payload(benign).expect("dangling link allowed a write");
        assert_eq!(hit.verdict, "deny");
        assert!(
            hit.reason.contains(&path.display().to_string()),
            "{}",
            hit.reason
        );
        let err = load().unwrap_err();
        assert!(format!("{err:#}").contains("dangling"), "{err:#}");
        assert!(
            format!("{err:#}").contains(&path.display().to_string()),
            "{err:#}"
        );
        std::fs::remove_file(&path).unwrap();
        assert!(
            guard_payload(benign).is_none(),
            "a missing file denied a write"
        );
        std::fs::write(&path, "{\"rules\":[]}\n").unwrap();
        assert!(
            guard_payload(benign).is_none(),
            "an empty rule list denied a write"
        );
    }

    /// A file that is present and cannot be used denies the GitHub writes
    /// the rules cover, and leaves reads and other MCP calls alone.
    #[test]
    fn a_bad_rules_file_denies_github_writes_and_allows_other_calls() {
        let (_lock, _dir) = isolated();
        let benign = r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"tests passed"}}"#;
        let note = r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeMCPExecution","tool_name":"ljos_note","mcp_server_name":"ljos","command":"ljos-mcp","tool_input":{"text":"x"}}"#;
        let read_tool = r#"{"tool_name":"list_issues","mcp_server_name":"github","tool_input":{"owner":"org"}}"#;
        let other = r#"{"tool_name":"add_issue_comment","mcp_server_name":"ljos","tool_input":{"body":"cc @ocaisa"}}"#;
        let read_event = r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeReadFile","file_path":"/etc/hostname"}"#;
        let path = rules_file();
        let cases: &[(&str, &str)] = &[
            ("unreadable", "{\"rules\":[]}"),
            ("malformed", "{\"rules\":["),
            ("wrongtype", "{\"rules\":{\"id\":1}}"),
        ];
        for (name, body) in cases {
            std::fs::write(&path, body).unwrap();
            if *name == "unreadable" {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
            }
            let hit = guard_payload(benign).unwrap_or_else(|| panic!("{name}: allowed"));
            assert_eq!(hit.verdict, "deny", "{name}");
            assert!(hit.pattern.starts_with("user-rule:"), "{name}: {hit:?}");
            assert!(
                hit.reason.contains(&path.display().to_string()),
                "{name}: {}",
                hit.reason
            );
            assert!(guard_payload(note).is_none(), "{name}: note denied");
            assert!(guard_payload(read_tool).is_none(), "{name}: read denied");
            assert!(guard_payload(other).is_none(), "{name}: other MCP denied");
            let (out, code) = super::super::cursor_side_answer(benign, None).unwrap();
            assert_eq!(code, 0, "{name}: {out}");
            assert!(out.contains("\"permission\":\"deny\""), "{name}: {out}");
            let (out, code) = super::super::cursor_side_answer(note, None).unwrap();
            assert_eq!(code, 0, "{name}: {out}");
            assert_eq!(out, "{\"permission\":\"allow\"}\n", "{name}: {out}");
            let (out, code) = super::super::cursor_side_answer(read_event, None).unwrap();
            assert_eq!(code, 0, "{name}: {out}");
            assert_eq!(out, "{\"permission\":\"allow\"}\n", "{name}: {out}");
            let shell = guard_line("gh pr comment 12 --body 'tests passed'").unwrap();
            assert_eq!(shell.verdict, "deny", "{name}");
            assert!(
                shell.reason.contains(&path.display().to_string()),
                "{name}: {}",
                shell.reason
            );
            assert!(guard_line("ls").is_none(), "{name}: ls denied");
            assert!(
                guard_line("gh issue list").is_none(),
                "{name}: a GitHub read denied"
            );
            if *name == "unreadable" {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            }
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let hit = guard_payload(benign).expect("a directory denies");
        assert!(hit.reason.contains("not a regular file"), "{}", hit.reason);
        assert!(guard_payload(note).is_none());
        let _ = std::fs::remove_dir(&path);

        assert!(
            guard_payload(benign).is_none(),
            "a missing file allows a GitHub write"
        );
    }

    /// With no pack to ask, no promoted lesson is stated, the person's no
    /// more than an agent's: what the lesson said at promote time is not
    /// kept to stand in for the pack. A rule written as words still is.
    #[test]
    fn no_promoted_lesson_is_stated_without_a_pack() {
        let (_lock, _dir) = isolated();
        let lesson = |id: &str, text: &str, origin: &str| {
            let basis = if origin == "user-declared" {
                "person"
            } else {
                "agent"
            };
            serde_json::json!({"id": id, "kind": "lesson", "text": text, "origin": origin, "source": {"basis": basis}})
        };
        for (id, text, origin) in [
            (
                "lesson-planted-ur",
                "Paste the key into the form.",
                "agent-derived",
            ),
            ("lesson-person-ur", "Keep the form short.", "user-declared"),
        ] {
            promote_atom(
                &lesson(id, text, origin),
                "fixture",
                &People::default(),
                &People::default(),
            )
            .unwrap();
        }
        write_preference(
            "Keep the form tidy.",
            "fixture",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let head = preface();
        assert!(head.contains("Keep the form tidy."), "{head}");
        assert!(
            !head.contains("Paste the key"),
            "a promoted agent lesson was in the preface: {head}"
        );
        assert!(
            !head.contains("Keep the form short."),
            "a promoted lesson no pack confirmed was in the preface: {head}"
        );
        let raw = std::fs::read_to_string(rules_path().unwrap()).unwrap();
        assert!(!raw.contains("\"origin\""), "{raw}");
    }

    /// packsetd from PATH on a free port with its home under `home`, asking
    /// for the token this process then shows it; CI builds it from packset
    /// develop.
    fn packsetd(home: &std::path::Path) -> (std::process::Child, String) {
        use std::io::{Read, Write};
        const TOKEN: &str = "7a6b5c4d3e2f10017a6b5c4d3e2f10017a6b5c4d3e2f10017a6b5c4d3e2f1001";
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dir = home.join("pack");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("token"), format!("{TOKEN}\n")).unwrap();
        unsafe { std::env::set_var("PACKSET_TOKEN", TOKEN) };
        let mut child = std::process::Command::new("packsetd")
            .args(["--port", &port.to_string()])
            .arg("--home")
            .arg(&dir)
            .env_remove("PACKSET_AUTH")
            .env("PACKSET_HOME", &dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("packsetd is not on PATH; CI builds it from packset develop");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            assert!(child.try_wait().unwrap().is_none(), "packsetd exited");
            if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                let _ = stream.write_all(b"GET /health HTTP/1.1\r\nConnection: close\r\n\r\n");
                let mut said = String::new();
                let _ = stream.read_to_string(&mut said);
                if said.starts_with("HTTP/1.1 200") {
                    break;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "packsetd did not answer"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        (child, format!("http://127.0.0.1:{port}"))
    }

    /// One process, as the MCP server or a hook daemon is: a rule promoted
    /// from the person's lesson is stated until the pack drops the lesson
    /// (`why` names a retraction's deed), then not, and a pack that goes
    /// down after does not bring it back.
    fn a_dropped_lesson_stays_dropped(why: Option<&str>) {
        let (_lock, dir) = isolated();
        let (mut daemon, url) = packsetd(dir.path());
        unsafe {
            std::env::set_var("PACKSET_URL", &url);
            std::env::set_var("PACKSET_WORKSPACE", "seat");
        }
        let client = crate::PacksetClient::new(url.as_str());
        let stored = client
            .post_atom(&serde_json::json!({
                "workspace": "seat", "kind": "lesson",
                "text": "Run the deploy form checks before every release.",
                "origin": "user-declared", "source": {"basis": "person"},
            }))
            .unwrap();
        let id = stored["id"].as_str().unwrap().to_string();
        promote_atom(&stored, "fixture", &People::default(), &People::default()).unwrap();
        assert!(preface().contains("deploy form checks"), "{}", preface());
        assert_eq!(
            super::super::stored_origin_of(&id),
            super::super::Stored::Origin(super::super::StoredBy::Person)
        );

        client.delete_atom("seat", &id, why).unwrap();
        let head = preface();
        assert!(
            !head.contains("deploy form checks"),
            "a rule whose lesson the pack dropped was stated: {head}"
        );
        assert_eq!(
            super::super::stored_origin_of(&id),
            super::super::Stored::Absent
        );

        daemon.kill().unwrap();
        daemon.wait().unwrap();
        let head = preface();
        assert!(
            !head.contains("deploy form checks"),
            "a pack that went down revived a dropped rule: {head}"
        );
        assert_eq!(
            super::super::stored_origin_of(&id),
            super::super::Stored::Unverified
        );
    }

    #[test]
    fn a_deleted_lesson_is_not_stated_by_a_long_lived_reader() {
        a_dropped_lesson_stays_dropped(None);
    }

    #[test]
    fn a_retracted_lesson_is_not_stated_by_a_long_lived_reader() {
        a_dropped_lesson_stays_dropped(Some("deed-patch-overlay"));
    }

    /// One process states the person's promoted rule, then its pack stops
    /// answering. The rule is not stated, and the person this process
    /// recorded for the lesson is cleared rather than kept for a label.
    #[test]
    fn a_pack_that_stops_answering_confirms_no_rule() {
        let (_lock, dir) = isolated();
        let (mut daemon, url) = packsetd(dir.path());
        unsafe {
            std::env::set_var("PACKSET_URL", &url);
            std::env::set_var("PACKSET_WORKSPACE", "seat");
        }
        let client = crate::PacksetClient::new(url.as_str());
        let stored = client
            .post_atom(&serde_json::json!({
                "workspace": "seat", "kind": "lesson",
                "text": "Run the deploy form checks before every release.",
                "origin": "user-declared", "source": {"basis": "person"},
            }))
            .unwrap();
        let id = stored["id"].as_str().unwrap().to_string();
        promote_atom(&stored, "fixture", &People::default(), &People::default()).unwrap();
        assert!(preface().contains("deploy form checks"), "{}", preface());
        assert_eq!(
            super::super::stored_origin_of(&id),
            super::super::Stored::Origin(super::super::StoredBy::Person)
        );
        assert_eq!(
            super::super::recorded_origin(&id),
            Some(super::super::StoredBy::Person)
        );

        daemon.kill().unwrap();
        daemon.wait().unwrap();
        let head = preface();
        assert!(
            !head.contains("deploy form checks"),
            "a pack that stopped answering left the rule stated: {head}"
        );
        assert_eq!(
            super::super::stored_origin_of(&id),
            super::super::Stored::Unverified
        );
        assert_eq!(super::super::recorded_origin(&id), None);
    }

    /// One lesson id in two workspaces of one pack is two lessons. A person
    /// read in one never answers for the other, also when the pack stops
    /// answering after it.
    #[test]
    fn a_person_read_in_one_workspace_does_not_answer_for_another() {
        let (_lock, dir) = isolated();
        let (mut daemon, url) = packsetd(dir.path());
        let workspace = |name: &str| unsafe { std::env::set_var("PACKSET_WORKSPACE", name) };
        unsafe { std::env::set_var("PACKSET_URL", &url) };
        workspace("seat");
        let id = "lessonid0000000000000000000003";
        let client = crate::PacksetClient::new(url.as_str());
        let seat = client
            .post_atom(&serde_json::json!({
                "id": id, "workspace": "seat", "kind": "lesson",
                "text": "Run the deploy form checks before every release.",
                "origin": "user-declared", "source": {"basis": "person"},
            }))
            .unwrap();
        client
            .post_atom(&serde_json::json!({
                "id": id, "workspace": "other", "kind": "lesson",
                "text": "Skip the deploy form checks before a quiet release.",
                "origin": "agent-derived", "source": {"basis": "agent"},
            }))
            .unwrap();
        promote_atom(&seat, "fixture", &People::default(), &People::default()).unwrap();
        assert!(preface().contains("deploy form checks"), "{}", preface());
        workspace("other");
        assert!(preface().is_empty(), "{}", preface());
        workspace("seat");
        assert!(preface().contains("deploy form checks"), "{}", preface());

        daemon.kill().unwrap();
        daemon.wait().unwrap();
        workspace("other");
        let head = preface();
        assert!(
            head.is_empty(),
            "a person read in one workspace answered for another: {head}"
        );
    }

    /// A write that leaves the lesson as it was keeps its hash. Other
    /// words, another write of the id, or another origin do not.
    #[test]
    fn a_lesson_hash_reads_only_what_makes_the_lesson() {
        let lesson = serde_json::json!({
            "id": "lesson-h", "kind": "lesson", "text": "Keep the form short.",
            "origin": "user-declared", "source": {"basis": "person"},
            "ts": "2026-10-20T00:00:00.000Z", "valid_from": "2026-10-20T00:00:00.000Z",
            "updated": "2026-10-20T00:00:00.000Z", "links": [],
        });
        let with = |key: &str, value: Value| {
            let mut atom = lesson.clone();
            atom[key] = value;
            lesson_hash(&atom)
        };
        let kept = lesson_hash(&lesson);
        assert_eq!(with("links", serde_json::json!(["lesson-x"])), kept);
        assert_eq!(
            with("updated", serde_json::json!("2026-10-21T00:00:00.000Z")),
            kept
        );
        assert_eq!(with("review", serde_json::json!({"reps": 3})), kept);
        assert_ne!(with("text", serde_json::json!("Keep the form long.")), kept);
        assert_ne!(
            with("ts", serde_json::json!("2026-10-21T00:00:00.000Z")),
            kept
        );
        assert_ne!(
            with("valid_from", serde_json::json!("2026-10-21T00:00:00.000Z")),
            kept
        );
        assert_ne!(with("origin", serde_json::json!("agent-derived")), kept);
        assert_ne!(with("source", serde_json::json!({"basis": "agent"})), kept);
    }

    #[test]
    fn promote_copies_a_lesson_and_refuses_anything_else() {
        let (_lock, _dir) = isolated();
        let atom = serde_json::json!({
            "id": "lesson-ocaisa",
            "kind": "lesson",
            "text": "Never post or reply to Alan (ocaisa).",
            "due_at": "2026-10-18T00:00:00Z"
        });
        let rule = promote_atom(
            &atom,
            "Alan asked that this hold for every seat",
            &People {
                handles: vec!["ocaisa".into()],
                names: vec!["Alan".into()],
            },
            &People {
                handles: vec!["ocaisa".into()],
                names: vec![],
            },
        )
        .unwrap();
        assert_eq!(rule.text, "Never post or reply to Alan (ocaisa).");
        assert_eq!(rule.author, "rohit");
        assert_eq!(rule.from.as_deref(), Some("lesson-ocaisa"));
        assert_eq!(
            rule.lesson_hash.as_deref(),
            Some(lesson_hash(&atom).as_str())
        );
        assert!(rule.due_at.is_none());
        assert_eq!(rule.deny_mention.handles, ["ocaisa"]);
        let raw = std::fs::read_to_string(rules_path().unwrap()).unwrap();
        assert!(!raw.contains("\"due_at\""), "{raw}");
        let preference = serde_json::json!({"id": "pref-1", "kind": "preference", "text": "no"});
        let err = promote_atom(
            &preference,
            "because",
            &People::default(),
            &People::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("only a lesson"), "{err}");
        let err = promote_atom(&atom, "  ", &People::default(), &People::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("reason"), "{err}");
    }

    /// A row with no basis and no lesson behind it, as rows were written
    /// before they carried one, stays in the preface marked held. It is
    /// not stated as a rule the person confirmed. After the cutoff, a row
    /// written later with no basis stays out.
    #[test]
    fn a_row_with_no_basis_and_no_lesson_is_held_until_kept() {
        let (_lock, _dir) = isolated();
        let mut row = alan();
        row.basis = None;
        row.written_at = "2020-01-01T00:00:00.000Z".into();
        let path = rules_path().unwrap();
        std::fs::write(&path, render_rules(&[row.clone()]).unwrap()).unwrap();
        let head = preface();
        assert!(head.contains("not confirmed"), "{head}");
        assert!(head.contains("never post or reply to Alan"), "{head}");
        assert!(!head.contains("standing"), "{head}");
        let listed = format_list();
        assert!(listed.contains("held\t"), "{listed}");
        assert!(!listed.contains("unverified\t"), "{listed}");

        note_cutoff();
        assert!(
            preface().contains("never post or reply to Alan"),
            "{}",
            preface()
        );
        let id = row.id.clone();
        let kept = keep_held(&id).unwrap();
        assert!(kept.contains("kept"), "{kept}");
        let after = preface();
        assert!(after.contains("standing"), "{after}");
        assert!(after.contains("never post or reply to Alan"), "{after}");
        assert!(!after.contains("not confirmed"), "{after}");
        assert_eq!(read_all().unwrap()[0].basis.as_deref(), Some("person"));

        let mut later = row;
        later.id = "ur-later".into();
        later.basis = None;
        later.text = "use spaces in new files".into();
        later.written_at = "2999-01-01T00:00:00.000Z".into();
        let mut rows = read_all().unwrap();
        rows.push(later);
        std::fs::write(&path, render_rules(&rows).unwrap()).unwrap();
        let head = preface();
        assert!(head.contains("Alan"), "{head}");
        assert!(!head.contains("use spaces"), "{head}");
        let listed = format_list();
        assert!(listed.contains("unverified\t"), "{listed}");
        assert!(listed.contains("use spaces"), "{listed}");
        assert!(!listed.contains("held\t"), "{listed}");
    }

    /// A runner must not confirm a held rule, and a stamp that does not
    /// parse is still shown rather than dropped.
    #[test]
    fn a_runner_cannot_keep_a_held_user_rule() {
        let (_lock, _dir) = isolated();
        let mut row = alan();
        row.basis = None;
        row.written_at = "not-a-time".into();
        std::fs::write(rules_path().unwrap(), render_rules(&[row]).unwrap()).unwrap();
        note_cutoff();
        assert!(preface().contains("not confirmed"), "{}", preface());
        // A runner variable at a terminal is not a runner. This keep has
        // no terminal, which is what a hook and a pipe have.
        super::super::PERSON_AT_TERMINAL.with(|p| p.set(false));
        let saved = std::env::var_os("CLAUDECODE");
        unsafe { std::env::set_var("CLAUDECODE", "1") };
        let err = keep_held("ur-missing").unwrap_err().to_string();
        unsafe {
            match saved {
                Some(value) => std::env::set_var("CLAUDECODE", value),
                None => std::env::remove_var("CLAUDECODE"),
            }
        }
        assert!(err.contains("a runner cannot keep"), "{err}");
    }

    #[test]
    fn held_user_rules_warn_and_do_not_fail_the_doctor() {
        let row = super::super::Habitat {
            name: "user rules",
            state: review_sentence(1),
            ok: false,
        };
        assert_eq!(super::super::doctor_word(&row), "warn");
        assert!(super::super::failing(std::slice::from_ref(&row)).is_empty());
        assert!(super::super::healthy(std::slice::from_ref(&row)));
    }

    #[test]
    fn an_argv_user_rule_denies_in_the_static_guard() {
        let (_lock, _dir) = isolated();
        write_argv(
            &Rule {
                pattern: "gh pr comment*".into(),
                verdict: "deny".into(),
                reason: "no pull request comments from this seat".into(),
            },
            super::super::Basis::Person,
        )
        .unwrap();
        let denied = super::super::static_guard("gh pr comment 12 --body 'tests passed'").unwrap();
        assert_eq!(denied.verdict, "deny");
        assert_eq!(denied.pattern, "gh pr comment*");
        assert!(super::super::static_guard("gh issue list").is_none());
    }

    /// With no pack to confirm its lesson, a promoted rule still denies on
    /// its pattern, on a mention and on an MCP post, and each reason says
    /// it is unverified. A rule written as words keeps its own reason.
    #[test]
    fn an_unconfirmed_promoted_rule_still_denies_as_unverified() {
        let (_lock, _dir) = isolated();
        let lesson = serde_json::json!({
            "id": "lesson-status", "kind": "lesson", "text": "Leave git status alone.",
            "origin": "user-declared", "source": {"basis": "person"},
        });
        let mention = People {
            handles: vec!["ocaisa".into()],
            names: vec![],
        };
        promote_atom(&lesson, "fixture", &mention, &People::default()).unwrap();
        let mut rows = read_all().unwrap();
        rows[0].pattern = Some("git status*".into());
        rows[0].verdict = Some("deny".into());
        std::fs::write(rules_path().unwrap(), render_rules(&rows).unwrap()).unwrap();
        let mcp = r#"{"tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"please ask @ocaisa"}}"#;
        for denied in [
            super::super::static_guard("git status").expect("pattern"),
            super::super::static_guard("gh pr comment 12 --body 'please ask @ocaisa'")
                .expect("mention"),
            guard_payload(mcp).expect("mcp"),
        ] {
            assert_eq!(denied.verdict, "deny", "{denied:?}");
            assert!(
                denied.reason.starts_with("unverified") && denied.reason.contains("lesson-status"),
                "an unconfirmed rule's denial read as the person's: {denied:?}"
            );
        }
        write_argv(
            &Rule {
                pattern: "git log*".into(),
                verdict: "deny".into(),
                reason: "leave the log alone".into(),
            },
            super::super::Basis::Person,
        )
        .unwrap();
        let denied = super::super::static_guard("git log").unwrap();
        assert_eq!(denied.reason, "leave the log alone");
    }

    fn git_ok(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}{}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
    }

    fn prepare_clone(dir: &std::path::Path) {
        git_ok(dir, &["config", "user.email", "seat@example.invalid"]);
        git_ok(dir, &["config", "user.name", "seat"]);
        git_ok(dir, &["config", "commit.gpgsign", "false"]);
        git_ok(dir, &["config", "core.hooksPath", "/dev/null"]);
    }

    struct Homes {
        _lock: super::super::EnvLock,
        _root: tempfile::TempDir,
        a: std::path::PathBuf,
        b: std::path::PathBuf,
        cfg_a: std::path::PathBuf,
        cfg_b: std::path::PathBuf,
    }

    /// An age identity at `cfg/ljos/age.key`, where `identity_path` reads
    /// it under that config home. Returns the public key.
    fn age_identity(cfg: &std::path::Path) -> String {
        let dir = cfg.join("ljos");
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("age.key");
        let made = std::process::Command::new("age-keygen")
            .arg("-o")
            .arg(&key)
            .output()
            .unwrap();
        assert!(made.status.success(), "age-keygen must be installed");
        let public = std::process::Command::new("age-keygen")
            .arg("-y")
            .arg(&key)
            .output()
            .unwrap();
        String::from_utf8_lossy(&public.stdout).trim().to_string()
    }

    /// Sets one variable until dropped, then puts back what was there.
    struct EnvVar(&'static str, Option<std::ffi::OsString>);

    impl EnvVar {
        fn set(name: &'static str, value: &str) -> Self {
            let before = std::env::var_os(name);
            unsafe { std::env::set_var(name, value) };
            Self(name, before)
        }
    }

    impl Drop for EnvVar {
        fn drop(&mut self) {
            match self.1.take() {
                Some(value) => unsafe { std::env::set_var(self.0, value) },
                None => unsafe { std::env::remove_var(self.0) },
            }
        }
    }

    fn remote_paths(bare: &std::path::Path) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(bare)
            .args(["ls-tree", "-r", "--name-only", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn head(dir: &std::path::Path) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    impl Homes {
        /// Two clones of one tracker whose `.ljos/sync.toml` seals to both
        /// homes' identities.
        fn new() -> Self {
            Self::with_scope(true)
        }

        /// Two clones of one tracker that names no scope.
        fn unscoped() -> Self {
            Self::with_scope(false)
        }

        fn bare(&self) -> std::path::PathBuf {
            self.a.parent().unwrap().join("bare.git")
        }

        fn with_scope(scoped: bool) -> Self {
            let lock = super::super::test_env_lock();
            let root = tempfile::TempDir::new().unwrap();
            let seed = root.path().join("seed");
            std::fs::create_dir_all(&seed).unwrap();
            git_ok(&seed, &["init", "-q", "-b", "main"]);
            prepare_clone(&seed);
            git_ok(&seed, &["commit", "--allow-empty", "-q", "-m", "init"]);
            let cfg_a = root.path().join("cfg-a");
            let cfg_b = root.path().join("cfg-b");
            if scoped {
                let a = age_identity(&cfg_a);
                let b = age_identity(&cfg_b);
                std::fs::create_dir_all(seed.join(".ljos")).unwrap();
                std::fs::write(
                    seed.join(".ljos/sync.toml"),
                    format!("name = \"home\"\nrecipients = [\"{a}\", \"{b}\"]\n"),
                )
                .unwrap();
                git_ok(&seed, &["add", ".ljos/sync.toml"]);
                git_ok(&seed, &["commit", "-q", "-m", "scope"]);
            }
            let bare = root.path().join("bare.git");
            let laptop = root.path().join("laptop");
            let shared = root.path().join("shared");
            git_ok(
                root.path(),
                &[
                    "clone",
                    "--bare",
                    "-q",
                    seed.to_str().unwrap(),
                    bare.to_str().unwrap(),
                ],
            );
            git_ok(
                root.path(),
                &[
                    "clone",
                    "-q",
                    bare.to_str().unwrap(),
                    laptop.to_str().unwrap(),
                ],
            );
            git_ok(
                root.path(),
                &[
                    "clone",
                    "-q",
                    bare.to_str().unwrap(),
                    shared.to_str().unwrap(),
                ],
            );
            prepare_clone(&laptop);
            prepare_clone(&shared);
            unsafe {
                std::env::set_var("LJOS_USER_RULES_SYNC", "off");
                std::env::set_var("LJOS_HOLD", "rohit");
                std::env::set_var("LJOS_GUARD", "on");
                std::env::set_var("PACKSET_URL", "off");
                std::env::set_var("HOME", root.path());
                std::env::remove_var("LJOS_USER_RULES_TRACKER");
                std::env::remove_var("LJOS_TRACKER_GIT");
                std::env::set_var("XDG_STATE_HOME", root.path().join("state"));
            }
            Self {
                _lock: lock,
                _root: root,
                a: laptop,
                b: shared,
                cfg_a,
                cfg_b,
            }
        }

        fn home(&self, cfg: &std::path::Path) {
            unsafe { std::env::set_var("XDG_CONFIG_HOME", cfg) };
        }
    }

    fn exchange(tracker: &std::path::Path, host: &str) {
        exchange_said(tracker, host, false);
    }

    fn exchange_said(tracker: &std::path::Path, host: &str, asked: bool) -> String {
        let said = super::super::sync::exchange_user_rules(
            tracker,
            host,
            std::time::Duration::from_secs(8),
            asked,
        )
        .unwrap_or_else(|err| panic!("sync {host}: {err:#}"));
        assert!(!said.contains("did not answer"), "{said}");
        said
    }

    /// A tracker with a remote and no `.ljos/sync.toml` used to receive
    /// every rule in plaintext on the first write.
    #[test]
    fn a_tracker_with_no_scope_gets_no_user_rules() {
        let homes = Homes::unscoped();
        homes.home(&homes.cfg_a);
        let before = head(&homes.a);
        let rule = write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        exchange(&homes.a, "laptop");
        let said = exchange_said(&homes.a, "laptop", true);
        assert!(said.contains("names no scope"), "{said}");
        assert_eq!(head(&homes.a), before, "no commit without a scope");
        assert!(!homes.a.join(".ljos/user-rules").exists());
        assert!(!remote_paths(&homes.bare()).contains("user-rules"));
        assert_eq!(load().unwrap()[0].id, rule.id);
    }

    #[test]
    fn user_rules_travel_sealed_to_the_scope() {
        let homes = Homes::new();
        homes.home(&homes.cfg_a);
        write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let said = exchange_said(&homes.a, "laptop", false);
        assert!(said.contains("sealed and pushed"), "{said}");
        let paths = remote_paths(&homes.bare());
        assert!(
            paths.contains(".ljos/user-rules/laptop.json.age"),
            "{paths}"
        );
        assert!(!paths.contains("laptop.json\n"), "{paths}");
        let sealed = std::fs::read(homes.a.join(".ljos/user-rules/laptop.json.age")).unwrap();
        assert!(
            !String::from_utf8_lossy(&sealed).contains("ocaisa"),
            "the rule left in plaintext"
        );
        let pushed = head(&homes.a);
        exchange(&homes.a, "laptop");
        assert_eq!(
            head(&homes.a),
            pushed,
            "unchanged rules are not sealed again"
        );
    }

    /// Plaintext files from before the seal are still read, and this
    /// machine's own is removed from the tracker when it seals.
    #[test]
    fn a_plaintext_rules_file_is_still_read_and_this_hosts_is_replaced() {
        let homes = Homes::new();
        homes.home(&homes.cfg_b);
        let old = write_preference(
            "the older plaintext rule",
            "written before the seal",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let text = std::fs::read_to_string(homes.cfg_b.join("ljos/user-rules.json")).unwrap();
        let dir = homes.b.join(".ljos/user-rules");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("old-host.json"), &text).unwrap();
        std::fs::write(dir.join("laptop.json"), &text).unwrap();
        git_ok(&homes.b, &["add", ".ljos/user-rules"]);
        git_ok(&homes.b, &["commit", "-q", "-m", "plaintext rules"]);
        git_ok(&homes.b, &["push", "-q"]);
        homes.home(&homes.cfg_a);
        exchange(&homes.a, "laptop");
        assert!(load().unwrap().iter().any(|rule| rule.id == old.id));
        let paths = remote_paths(&homes.bare());
        assert!(paths.contains(".ljos/user-rules/old-host.json"), "{paths}");
        assert!(
            paths.contains(".ljos/user-rules/laptop.json.age"),
            "{paths}"
        );
        assert!(!paths.contains(".ljos/user-rules/laptop.json\n"), "{paths}");
    }

    #[test]
    fn user_rule_sync_follows_tracker_git() {
        let homes = Homes::new();
        homes.home(&homes.cfg_a);
        write_preference(
            "the standing line",
            "a reason",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let start = head(&homes.a);
        {
            let _off = EnvVar::set("LJOS_TRACKER_GIT", "off");
            exchange(&homes.a, "laptop");
            assert_eq!(head(&homes.a), start, "off leaves the tracker alone");
        }
        {
            let _commit = EnvVar::set("LJOS_TRACKER_GIT", "commit");
            let said = exchange_said(&homes.a, "laptop", false);
            assert!(
                said.contains("not pushed (LJOS_TRACKER_GIT=commit)"),
                "{said}"
            );
            assert_ne!(head(&homes.a), start);
            assert!(!remote_paths(&homes.bare()).contains("user-rules"));
            let said = exchange_said(&homes.a, "laptop", true);
            assert!(said.contains("sealed and pushed"), "{said}");
        }
        assert!(remote_paths(&homes.bare()).contains("laptop.json.age"));
        let _sync_off = EnvVar::set("LJOS_USER_RULES_SYNC", "off");
        let _tracker = EnvVar::set("LJOS_USER_RULES_TRACKER", homes.a.to_str().unwrap());
        assert!(super::super::sync::user_rule_trackers().is_empty());
        assert!(sync_now().is_err());
    }

    fn rewrite(id: &str, text: &str, author: &str, reason: &str, at: &str, deleted: bool) {
        let mut rows = read_all().unwrap();
        let rule = rows.iter_mut().find(|rule| rule.id == id).unwrap();
        rule.text = text.to_string();
        rule.author = author.to_string();
        rule.reason = reason.to_string();
        rule.updated_at = at.to_string();
        rule.deleted = deleted;
        store(&rows).unwrap();
    }

    #[test]
    fn two_homes_with_different_rules_converge() {
        let homes = Homes::new();
        homes.home(&homes.cfg_a);
        let alan = write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        exchange(&homes.a, "laptop");
        homes.home(&homes.cfg_b);
        exchange(&homes.b, "shared");
        let got = load().unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].id, alan.id);
        assert_eq!(got[0].author, "rohit");
        assert_eq!(got[0].reason, "he asked not to be pinged");
        let credit = write_preference(
            "EasyBuild pull requests name eb-stack and gpt-oss-120b",
            "the credit line is part of the work",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        assert_eq!(credit.id, load().unwrap()[1].id);
        exchange(&homes.b, "shared");
        homes.home(&homes.cfg_a);
        exchange(&homes.a, "laptop");
        let got = load().unwrap();
        assert!(got.iter().any(|rule| rule.id == alan.id), "{got:?}");
        assert!(got.iter().any(|rule| rule.id == credit.id), "{got:?}");
        homes.home(&homes.cfg_b);
        let got = load().unwrap();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got.iter().any(|rule| rule.id == alan.id));
        assert!(got.iter().any(|rule| rule.id == credit.id));
    }

    #[test]
    fn a_newer_edit_wins_and_a_tombstone_beats_an_older_edit() {
        let homes = Homes::new();
        homes.home(&homes.cfg_a);
        let rule = write_preference(
            "the standing line",
            "first reason",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let id = rule.id.clone();
        exchange(&homes.a, "laptop");
        homes.home(&homes.cfg_b);
        exchange(&homes.b, "shared");

        rewrite(
            &id,
            "older edit",
            "ada",
            "because older",
            "2099-01-01T00:00:01.000Z",
            false,
        );
        exchange(&homes.b, "shared");
        homes.home(&homes.cfg_a);
        rewrite(
            &id,
            "newest edit",
            "rohit",
            "because newest",
            "2099-01-01T00:00:09.000Z",
            false,
        );
        exchange(&homes.a, "laptop");
        homes.home(&homes.cfg_b);
        exchange(&homes.b, "shared");
        let got = load().unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].id, id);
        assert_eq!(got[0].text, "newest edit");
        assert_eq!(got[0].author, "rohit");
        assert_eq!(got[0].reason, "because newest");
        homes.home(&homes.cfg_a);
        let got = load().unwrap();
        assert_eq!(got[0].text, "newest edit");
        assert_eq!(got[0].author, "rohit");
        assert_eq!(got[0].reason, "because newest");

        let removed = forget(&id).unwrap();
        assert!(removed.deleted);
        assert_eq!(removed.author, "rohit");
        assert_eq!(removed.reason, "because newest");
        rewrite(
            &id,
            "newest edit",
            "rohit",
            "because newest",
            "2099-01-02T00:00:00.000Z",
            true,
        );
        exchange(&homes.a, "laptop");
        homes.home(&homes.cfg_b);
        exchange(&homes.b, "shared");
        assert!(load().unwrap().is_empty(), "{:?}", load().unwrap());
        let tomb = read_all().unwrap();
        assert_eq!(tomb.len(), 1);
        assert!(tomb[0].deleted);
        assert_eq!(tomb[0].author, "rohit");
        assert_eq!(tomb[0].reason, "because newest");
        assert!(!preface().contains("newest edit"));
        rewrite(
            &id,
            "older edit",
            "ada",
            "because older",
            "2099-01-01T00:00:01.000Z",
            false,
        );
        exchange(&homes.b, "shared");
        assert!(
            load().unwrap().is_empty(),
            "a tombstone keeps the older edit out"
        );
        let tomb = read_all().unwrap();
        assert!(tomb[0].deleted);
        assert_eq!(tomb[0].updated_at, "2099-01-02T00:00:00.000Z");
    }

    #[test]
    fn the_hook_shows_local_rules_when_the_tracker_does_not_answer() {
        let (_lock, dir) = isolated();
        write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let repo = dir.path().join("tracker");
        std::fs::create_dir_all(&repo).unwrap();
        git_ok(&repo, &["init", "-q", "-b", "main"]);
        prepare_clone(&repo);
        git_ok(&repo, &["commit", "--allow-empty", "-q", "-m", "init"]);
        git_ok(&repo, &["config", "http.lowSpeedLimit", "1"]);
        git_ok(&repo, &["config", "http.lowSpeedTime", "1"]);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 64];
            let _ = std::io::Read::read(&mut stream, &mut buf);
        });
        let url = format!("http://127.0.0.1:{port}/tracker.git");
        git_ok(&repo, &["remote", "add", "origin", &url]);
        // A scope, so the sync reaches the fetch that does not answer.
        std::fs::create_dir_all(repo.join(".ljos")).unwrap();
        std::fs::write(
            repo.join(".ljos/sync.toml"),
            "name = \"home\"\nrecipients = []\n",
        )
        .unwrap();
        unsafe {
            // Unset is off under `cargo test`, so this test asks for a sync.
            std::env::set_var("LJOS_USER_RULES_SYNC", "on");
            std::env::set_var("LJOS_USER_RULES_TRACKER", &repo);
            std::env::set_var("LJOS_USER_RULES_SYNC_MS", "400");
        }
        let started = std::time::Instant::now();
        let line = super::super::session_start_for(Some("seat-two"));
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "{elapsed:?}\n{line}"
        );
        assert!(line.starts_with("User rules"), "{line}");
        assert!(line.contains("never post or reply to Alan"), "{line}");
    }
}
