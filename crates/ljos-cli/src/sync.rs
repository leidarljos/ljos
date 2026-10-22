//! Sharing the seat's memory across machines through the tracker repository
//! that already travels between them.
//!
//! Each tracker repository names one or more scopes in `.ljos/sync.toml`.
//! The primary scope keeps the legacy keys (`name`, `recipients`), and
//! optional `[scopes]` entries add named scopes with their own recipients.
//! Every machine writes only its own log for each configured scope under
//! `.ljos/atoms/`, so two machines never touch one file and git never has to
//! merge a log. A log carries the machine's live atoms of that scope without
//! their vectors; the receiving pack encodes them again. An atom's scope is
//! its `scope:NAME` entity. An atom with none stays on the primary scope.
//! A machine `default_scope` that names an extra scope is refused, so an
//! unscoped box atom is never sealed into that shared log.
//!
//! Import posts every atom of every other machine's log to this seat's pack,
//! which answers a text it already holds with the claim it has, so an import
//! twice is an import once. Retirement travels by difference: the texts an
//! import took from a host are remembered, and a text that host no longer
//! sends retires the local copy that came from it, never a claim this
//! machine wrote itself. Every log is retired before any log is posted.
//! A text that moved scopes would otherwise be answered with the old claim,
//! and the old scope's retirement would delete that answer.
//!
//! `--adopt HOST` posts that host's live atoms as this machine's own, with
//! origin `peer` and each log's scope, then deletes every log of that host.
//! The deletion is one commit, made only after every post and forget has
//! landed. Its subject `chore(sync): adopt HOST SCOPE atoms` names the
//! primary scope; a `host+scope` file in that commit is an extra scope.
//! Another machine's import reads the subject and retires that host's
//! copies in each scope whose log the commit removed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// The shared scope a box and its VMs use. It is an extra scope under
/// `[scopes]`. A primary named this would send unscoped atoms and every
/// finish lesson into the shared log.
pub const CLOUD_SCOPE: &str = "cloud";

/// Set by `ljos cloud up` in `~/.config/ljos/env`. Writers tag memory and
/// lessons `scope:<value>` explicitly. This is not `default_scope`: an
/// unscoped atom still stays on the primary scope.
pub const WRITE_SCOPE_KEY: &str = "LJOS_WRITE_SCOPE";

/// The scope a tracker repository shares: `.ljos/sync.toml`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Scope {
    /// The scope's name; an atom carrying `scope:NAME` belongs to it.
    pub name: String,
    /// The age public keys of the machines allowed to read this scope.
    pub recipients: Vec<String>,
    /// Projects of this repository whose lessons belong to another scope:
    /// `[projects] tools = "shared"` sends a lesson written on a `tools`
    /// issue with the `shared` scope's log instead of this one.
    #[serde(default)]
    pub projects: BTreeMap<String, String>,
    /// Additional named scopes in this repository:
    /// `[scopes] cloud = ["age1..."]`.
    #[serde(default)]
    pub scopes: BTreeMap<String, Vec<String>>,
}

impl Scope {
    /// The scope a lesson on an issue of `project` belongs to.
    #[must_use]
    pub fn for_project(&self, project: &str) -> String {
        self.projects
            .get(project)
            .cloned()
            .unwrap_or_else(|| self.name.clone())
    }

    fn recipients_for(&self, name: &str) -> Option<&[String]> {
        if name == self.name {
            Some(&self.recipients)
        } else {
            self.scopes.get(name).map(Vec::as_slice)
        }
    }

    fn all_scopes(&self) -> Vec<ScopeView<'_>> {
        let mut scopes = vec![ScopeView {
            name: &self.name,
            recipients: &self.recipients,
        }];
        scopes.extend(
            self.scopes
                .iter()
                .map(|(name, recipients)| ScopeView { name, recipients }),
        );
        scopes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScopeView<'a> {
    name: &'a str,
    recipients: &'a [String],
}

/// This machine's defaults: `~/.config/ljos/sync.toml`, `default_scope`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct Local {
    #[serde(default)]
    default_scope: Option<String>,
    /// The tracker repositories this machine shares, when more than the
    /// tracker root: a machine can hold one scope's repository beside
    /// another's.
    #[serde(default)]
    repos: Vec<String>,
}

fn local() -> Local {
    std::fs::read_to_string(config_dir().join("sync.toml"))
        .ok()
        .and_then(|t| toml::from_str::<Local>(&t).ok())
        .unwrap_or_default()
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map_or_else(|| PathBuf::from(path), |h| PathBuf::from(h).join(rest)),
        None => PathBuf::from(path),
    }
}

/// The entity an imported atom carries: which machine sent it.
#[must_use]
pub fn sender_entity(host: &str) -> String {
    format!("sync:{host}")
}

/// This machine's short host name, as [`machine_name`] folds it. Export,
/// adopt and every log path use this one name, so the log an export
/// writes is the one an adopt commit stages.
#[must_use]
pub fn host() -> String {
    machine_name(&raw_host())
}

fn raw_host() -> String {
    if let Some(host) = std::env::var_os("LJOS_TEST_HOSTNAME").and_then(|raw| {
        let host = raw.to_string_lossy().trim().to_string();
        (!host.is_empty()).then_some(host)
    }) {
        return host;
    }
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|h| h.trim().to_string())
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "host".into())
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("ljos")
}

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
        .join("sync")
}

/// This machine's age identity, made on first use.
#[must_use]
pub fn identity_path() -> PathBuf {
    config_dir().join("age.key")
}

/// The age identity for scope `cloud`, written by `ljos cloud up` from a
/// named secret. It is not [`identity_path`]: that key is this machine's
/// own. Scope `cloud` is sealed to one shared recipient, and the private
/// key is not in the tracker, so a VM cannot read the scope without this
/// file.
#[must_use]
pub fn cloud_identity_path() -> PathBuf {
    config_dir().join("cloud.age")
}

/// Bech32 charset, upper case, as `age` writes an identity line.
const AGE_BECH32: &[u8] = b"QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L";

/// Whether `text` is an age identity file: comments, blank lines, and one
/// or more `AGE-SECRET-KEY-1` lines. The payload length is the X25519
/// identity `age` writes today.
#[must_use]
pub(crate) fn age_identity(text: &str) -> bool {
    if text.len() > 8192 || text.contains('\0') {
        return false;
    }
    let mut found = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix("AGE-SECRET-KEY-1") else {
            return false;
        };
        if !(58..=96).contains(&rest.len()) || !rest.bytes().all(|b| AGE_BECH32.contains(&b)) {
            return false;
        }
        found = true;
    }
    found
}

/// Identity files that may open a sealed file of scope `scope_name`.
///
/// The machine key when it is an age identity, then the scope `cloud`
/// identity when `cloud up` wrote one and `scope_name` is `cloud`.
/// Memory leaves the box only as scope `cloud`, so the cloud identity
/// is never tried on another scope's log. A machine key that is not an
/// identity is still passed when it is the only file, so `age` reports
/// the broken key. A cloud file that is not an identity is left out, so
/// it cannot make `age` refuse the machine key.
fn identities_for_open(scope_name: &str) -> Vec<PathBuf> {
    let own = identity_path();
    let own_ok = read_identity(&own).is_some_and(|text| age_identity(&text));
    let cloud = cloud_identity_path();
    let cloud_ok =
        scope_name == CLOUD_SCOPE && read_identity(&cloud).is_some_and(|text| age_identity(&text));
    let mut ids = Vec::new();
    // A valid machine key is used. A broken one is passed only when the
    // cloud identity cannot stand in, so age still reports that key.
    if own_ok || (own.is_file() && !cloud_ok) {
        ids.push(own);
    }
    if cloud_ok {
        ids.push(cloud);
    }
    ids
}

fn read_identity(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 8192 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The public key of this machine's identity, making the identity first when
/// there is none. This is what goes into a scope's `recipients`.
///
/// # Errors
///
/// `age-keygen` missing or refusing.
pub fn public_key() -> Result<String> {
    let key = identity_path();
    if !key.exists() {
        if let Some(dir) = key.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let made = keygen_cmd()
            .arg("-o")
            .arg(&key)
            .stdin(Stdio::null())
            .output()
            .context("age-keygen not on PATH; install age")?;
        if !made.status.success() {
            bail!(
                "age-keygen: {}",
                String::from_utf8_lossy(&made.stderr).trim()
            );
        }
    }
    let out = keygen_cmd()
        .arg("-y")
        .arg(&key)
        .output()
        .context("age-keygen not on PATH; install age")?;
    if !out.status.success() {
        bail!(
            "age-keygen -y: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The scope a tracker repository declares, when it declares one.
///
/// # Errors
///
/// A `.ljos/sync.toml` that is present and does not parse.
pub fn scope_of_repo(root: &Path) -> Result<Option<Scope>> {
    let path = root.join(".ljos").join("sync.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let scope: Scope = toml::from_str(&text).with_context(|| path.display().to_string())?;
            check_scope_name(&path, &scope.name)?;
            if scope.name == CLOUD_SCOPE {
                bail!(
                    "{} names primary scope `cloud`, which is reserved for the shared scope; put cloud under [scopes]",
                    path.display()
                );
            }
            if scope.scopes.contains_key(&scope.name) {
                bail!(
                    "{} repeats `{}` in [scopes]; keep the primary scope in `name` and `recipients`",
                    path.display(),
                    scope.name
                );
            }
            for name in scope.scopes.keys().chain(scope.projects.values()) {
                check_scope_name(&path, name)?;
            }
            Ok(Some(scope))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| path.display().to_string()),
    }
}

/// A scope name is non-empty and drawn from `[a-z0-9-]`. `/` would escape
/// the atoms directory and `+` is the separator in a log file name.
fn valid_scope_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn check_scope_name(path: &Path, name: &str) -> Result<()> {
    if valid_scope_name(name) {
        return Ok(());
    }
    bail!(
        "{} names `{name}`, but a scope name is non-empty and only [a-z0-9-]",
        path.display()
    );
}

/// The scope this machine's memory and lessons are tagged with.
///
/// `cloud up` writes [`WRITE_SCOPE_KEY`] into the process and into
/// `~/.config/ljos/env`. The value has to be a scope name. Unset, writers
/// leave the atom unscoped and the repository's primary scope applies.
#[must_use]
pub fn machine_write_scope() -> Option<String> {
    let raw = std::env::var(WRITE_SCOPE_KEY)
        .ok()
        .or_else(write_scope_from_env_file)?;
    let name = raw.trim();
    valid_scope_name(name).then(|| name.to_string())
}

fn write_scope_from_env_file() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let text = std::fs::read_to_string(PathBuf::from(home).join(".config/ljos/env")).ok()?;
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        (key.trim() == WRITE_SCOPE_KEY).then(|| value.trim().to_string())
    })
}

/// Scope tag for a finish lesson. A VM tags `cloud` explicitly so the
/// lesson does not follow the repository's primary scope into the box log.
#[must_use]
pub fn scope_for_lesson(repo_scope: Option<String>) -> Option<String> {
    machine_write_scope().or(repo_scope)
}

/// The scope unscoped atoms take. An explicit `default_scope` may name the
/// primary scope. One that names an extra scope is refused: applying it
/// would seal an unscoped box atom into that extra scope's log.
fn configured_default(scope: &Scope) -> Result<String> {
    let chosen = local().default_scope.unwrap_or_else(|| scope.name.clone());
    if !valid_scope_name(&chosen) {
        bail!("sync: default_scope `{chosen}` is not a scope name; use [a-z0-9-]");
    }
    if scope.scopes.contains_key(&chosen) {
        bail!(
            "sync: default_scope `{chosen}` names an extra scope; unscoped atoms stay on the primary scope `{}`",
            scope.name
        );
    }
    Ok(chosen)
}

/// The `scope:NAME` entity on `atom`, when it has one.
fn explicit_scope(atom: &Value) -> Option<&str> {
    atom["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|e| e.strip_prefix("scope:"))
}

fn default_scope(repo_scope: &str) -> String {
    local()
        .default_scope
        .unwrap_or_else(|| repo_scope.to_string())
}

/// The scope an atom belongs to: its `scope:NAME` entity, else `default`.
#[must_use]
pub fn atom_scope(atom: &Value, default: &str) -> String {
    explicit_scope(atom).map_or_else(|| default.to_string(), str::to_string)
}

/// An atom as it travels: without its vector, and without a sender tag of
/// another machine, since a host sends only what it holds as its own.
#[must_use]
pub fn travelling(atom: &Value) -> Value {
    let mut a = atom.clone();
    if let Some(map) = a.as_object_mut() {
        map.remove("embedding");
    }
    a
}

/// Whether this machine holds an atom as its own rather than as an import.
#[must_use]
pub fn is_own(atom: &Value) -> bool {
    !atom["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|e| e.starts_with("sync:"))
}

/// The plaintext of this machine's log for a scope: its own live atoms of
/// that scope, sorted by id, one JSON object a line.
///
/// An extra scope is sealed with `explicit_only`, so only an atom whose
/// `scope:` entity names that scope is included. A default cannot pull an
/// unscoped atom into the shared log.
#[must_use]
pub fn log_plaintext(atoms: &[Value], scope: &str, default: &str) -> String {
    log_lines(atoms, scope, default, false)
}

fn log_lines(atoms: &[Value], scope: &str, default: &str, explicit_only: bool) -> String {
    let mut mine: Vec<Value> = atoms
        .iter()
        .filter(|a| belongs_to_scope(a, scope, default, explicit_only))
        .map(travelling)
        .collect();
    mine.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    mine.iter().map(|a| format!("{a}\n")).collect()
}

fn belongs_to_scope(atom: &Value, scope: &str, default: &str, explicit_only: bool) -> bool {
    if !is_own(atom) {
        return false;
    }
    match explicit_scope(atom) {
        Some(name) => name == scope,
        None => !explicit_only && default == scope,
    }
}

fn log_dir(root: &Path) -> PathBuf {
    root.join(".ljos").join("atoms")
}

fn log_suffix(primary_scope: &str, scope_name: &str) -> String {
    if scope_name == primary_scope {
        String::new()
    } else {
        format!("+{scope_name}")
    }
}

fn host_log_rels_for_scope(host: &str, primary_scope: &str, scope_name: &str) -> [String; 2] {
    let suffix = log_suffix(primary_scope, scope_name);
    [
        format!(".ljos/atoms/{host}{suffix}.jsonl.age"),
        format!(".ljos/atoms/{host}{suffix}.digest"),
    ]
}

fn host_scope_from_log_path(path: &Path, scope: &Scope) -> Option<(String, String)> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".jsonl.age")?;
    if let Some((host, scope_name)) = stem.rsplit_once('+') {
        if host.is_empty() || scope_name.is_empty() {
            return None;
        }
        // A scope that was removed still names a log. Import retires it.
        return Some((host.to_string(), scope_name.to_string()));
    }
    if stem.is_empty() {
        return None;
    }
    Some((stem.to_string(), scope.name.clone()))
}

fn scope_is_configured(scope: &Scope, name: &str) -> bool {
    name == scope.name || scope.scopes.contains_key(name)
}

/// Seal `plain` to the recipients into `out`.
fn seal(plain: &str, recipients: &[String], out: &Path) -> Result<()> {
    if recipients.is_empty() {
        bail!("sync: the scope names no recipients; add this machine's `ljos sync --key` to .ljos/sync.toml");
    }
    let mut cmd = age_cmd();
    for r in recipients {
        cmd.arg("-r").arg(r);
    }
    let tmp = out.with_extension("age.tmp");
    let mut child = cmd
        .arg("-o")
        .arg(&tmp)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("age not on PATH; install age")?;
    use std::io::Write;
    child
        .stdin
        .take()
        .context("age: no stdin")?
        .write_all(plain.as_bytes())?;
    let done = child.wait_with_output()?;
    if !done.status.success() {
        let _ = std::fs::remove_file(&tmp);
        bail!("age: {}", String::from_utf8_lossy(&done.stderr).trim());
    }
    std::fs::rename(&tmp, out)?;
    Ok(())
}

/// The public key written above an age identity (`# public key: age1...`).
/// Absent on a key that was copied without that comment.
fn commented_public_key(identity: &Path) -> Option<String> {
    let text = std::fs::read_to_string(identity).ok()?;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("# public key:") else {
            continue;
        };
        let Some(key) = rest.split_whitespace().next() else {
            continue;
        };
        if key.starts_with("age1") {
            return Some(key.to_string());
        }
    }
    None
}

/// This machine's recipient: the identity's public-key comment, else
/// `age-keygen -y` (or `rage-keygen` when age-keygen is not installed).
fn identity_recipient(identity: &Path) -> Result<String> {
    if let Some(key) = commented_public_key(identity) {
        return Ok(key);
    }
    let out = keygen_cmd()
        .arg("-y")
        .arg(identity)
        .output()
        .context("age-keygen not on PATH; install age")?;
    if !out.status.success() {
        bail!(
            "age-keygen -y: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let key = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if key.is_empty() {
        bail!("age-keygen -y: no public key");
    }
    Ok(key)
}

/// A decrypt miss. This machine's own recipient is an error: the log was
/// sealed to it and then failed to open. Any other key stays a skip.
fn miss_report(host: &str, scope: &str, mine: &str, recipients: &[String]) -> Result<String> {
    if recipients.iter().any(|recipient| recipient == mine) {
        bail!("sync: {host}'s {scope} log is sealed to this machine and did not open");
    }
    Ok(format!(
        "sync: {host}'s {scope} log is not sealed to this machine\n"
    ))
}

/// Open a sealed log with one identity file. None when that identity is
/// not a recipient.
#[cfg(test)]
fn open(path: &Path, identity: &Path) -> Result<Option<String>> {
    open_identities(path, &[identity.to_path_buf()])
}

/// Open a sealed log with every identity in `identities`. None when none
/// of them is a recipient. `age` and `rage` run with `LC_ALL`, `LANG`,
/// `LANGUAGE`, and `LC_MESSAGES` set to C, so a French miss stays a miss
/// instead of aborting the rest of the import.
fn open_identities(path: &Path, identities: &[PathBuf]) -> Result<Option<String>> {
    if identities.is_empty() {
        return Ok(None);
    }
    let mut cmd = age_cmd();
    cmd.arg("-d");
    for identity in identities {
        cmd.arg("-i").arg(identity);
    }
    let out = cmd
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("age not on PATH; install age")?;
    let decrypt_miss = {
        let stderr = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
        stderr.contains("no identity matched") || stderr.contains("no matching keys found")
    };
    if out.status.success() {
        Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
    } else if decrypt_miss {
        Ok(None)
    } else {
        bail!(
            "age -d {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )
    }
}

/// One line when `age` is not installed. The seal is skipped; the rest of
/// the sync still succeeds. The next finish tries again.
fn age_missing_line() -> String {
    "sync: age is not installed, so the log was not sealed; install age to sync it\n".into()
}

#[cfg(test)]
thread_local! {
    /// `Some` forces whether this thread treats `age` as installed.
    /// `None` asks the real `PATH`. Hiding `age` stays on this thread so
    /// a test does not empty `PATH` for every other test in the process.
    static AGE_ON_PATH: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether `age` can be run. A test may force the answer on its own thread.
fn age_on_path() -> bool {
    #[cfg(test)]
    if let Some(present) = AGE_ON_PATH.with(|cell| cell.get()) {
        return present;
    }
    crate::on_path("age") || crate::on_path("rage")
}

/// Run an age-compatible tool in the C locale. rage asks sys-locale for
/// languages in the order `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`.
/// A parent `LANGUAGE=fr` or `LC_MESSAGES=fr` stays French when the child
/// only sets `LC_ALL` and `LANG`, so "Aucune clef correspondante" would
/// not match the English miss text and import would abort every later host.
fn c_locale(mut cmd: Command) -> Command {
    cmd.env("LC_ALL", "C");
    cmd.env("LANG", "C");
    cmd.env("LANGUAGE", "C");
    cmd.env("LC_MESSAGES", "C");
    cmd
}

fn age_compatible<'a>(age_name: &'a str, rage_name: &'a str) -> &'a str {
    if crate::on_path(age_name) || !crate::on_path(rage_name) {
        age_name
    } else {
        rage_name
    }
}

fn age_cmd() -> Command {
    c_locale(Command::new(age_compatible("age", "rage")))
}

fn keygen_cmd() -> Command {
    c_locale(Command::new(age_compatible("age-keygen", "rage-keygen")))
}

/// Hide `age` on this thread until the guard drops. `PATH` stays as it is.
#[cfg(test)]
#[must_use]
fn hide_age() -> impl Drop {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            AGE_ON_PATH.with(|cell| cell.set(None));
        }
    }
    AGE_ON_PATH.with(|cell| cell.set(Some(false)));
    Guard
}

fn age_missing(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
    })
}

/// Write this machine's log for the repository's scope when its atoms
/// changed since the last write. Returns the line that says what happened.
/// A missing `age` skips the seal and is not an error.
///
/// # Errors
///
/// The pack not answering, or age refusing to seal for a reason other than
/// being absent.
pub fn export(root: &Path, scope: &Scope, atoms: &[Value]) -> Result<String> {
    let default = configured_default(scope)?;
    export_for_scope(root, scope, &scope.name, atoms, &default)
}

pub(crate) fn export_named(
    root: &Path,
    scope: &Scope,
    scope_name: &str,
    atoms: &[Value],
) -> Result<String> {
    let default = configured_default(scope)?;
    export_for_scope(root, scope, scope_name, atoms, &default)
}

fn export_for_scope(
    root: &Path,
    scope: &Scope,
    scope_name: &str,
    atoms: &[Value],
    default: &str,
) -> Result<String> {
    let Some(recipients) = scope.recipients_for(scope_name) else {
        bail!("sync: scope `{scope_name}` is not named in .ljos/sync.toml");
    };
    export_named_scope(root, &scope.name, scope_name, recipients, atoms, default)
}

fn export_named_scope(
    root: &Path,
    primary_scope: &str,
    scope_name: &str,
    recipients: &[String],
    atoms: &[Value],
    default: &str,
) -> Result<String> {
    let host = host();
    let explicit_only = scope_name != primary_scope;
    let plain = log_lines(atoms, scope_name, default, explicit_only);
    let count = plain.lines().count();
    std::fs::create_dir_all(log_dir(root))?;
    let [log_rel, digest_rel] = host_log_rels_for_scope(&host, primary_scope, scope_name);
    let log = root.join(&log_rel);
    let stamp = root.join(&digest_rel);
    let digest = format!(
        "{} {}\n",
        crate::work_id(&plain),
        crate::work_id(&recipients.join(","))
    );
    if std::fs::read_to_string(&stamp).is_ok_and(|d| d == digest) {
        return Ok(format!(
            "sync: {count} {} atoms unchanged since the last export\n",
            scope_name
        ));
    }
    if !age_on_path() {
        return Ok(age_missing_line());
    }
    if let Err(err) = seal(&plain, recipients, &log) {
        if age_missing(&err) {
            return Ok(age_missing_line());
        }
        return Err(err);
    }
    std::fs::write(&stamp, digest)?;
    Ok(format!(
        "sync: exported {count} {} atoms to {log_rel}, sealed to {} recipient{}\n",
        scope_name,
        recipients.len(),
        if recipients.len() == 1 { "" } else { "s" }
    ))
}

fn export_all(root: &Path, scope: &Scope, atoms: &[Value]) -> Result<String> {
    let default = configured_default(scope)?;
    drop_stale_scope_files(root, scope);
    if !age_on_path() {
        return Ok(age_missing_line());
    }
    let mut out = String::new();
    for named_scope in scope.all_scopes() {
        // An extra scope with no recipients used to fail here, after the
        // primary log was already sealed, and the commit never ran. Skip
        // it. An empty primary still errors in `seal`.
        if named_scope.name != scope.name && named_scope.recipients.is_empty() {
            out.push_str(&format!(
                "sync: scope `{}` names no recipients; skipped\n",
                named_scope.name
            ));
            continue;
        }
        let said = export_named_scope(
            root,
            &scope.name,
            named_scope.name,
            named_scope.recipients,
            atoms,
            &default,
        )?;
        if said == age_missing_line() {
            return Ok(said);
        }
        out.push_str(&said);
    }
    Ok(out)
}

/// What one host's log asks of this pack: the atoms to post, and the
/// memories it sent last time and sends no longer.
#[derive(Debug, Default, PartialEq)]
pub struct Incoming {
    pub post: Vec<Value>,
    pub retired: Vec<String>,
}

/// What adopting one gone host's log asks of this pack: which memories are
/// still live there, and which atoms this host should post as its own.
#[derive(Debug, Default, PartialEq)]
struct AdoptPlan {
    /// [`memory_key`] of every live atom in the log, including ones this
    /// host already holds and so will not post again.
    live: BTreeSet<String>,
    post: Vec<Value>,
}

/// The ticket an atom belongs to. The pack treats the same words on two
/// tickets as two memories; the `issue` field is that identity, and an
/// `issue:` entity is the same fact when the field was left off.
fn atom_issue(atom: &Value) -> String {
    atom.get("issue")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|issue| !issue.is_empty())
        .map(str::to_string)
        .or_else(|| {
            atom.get("entities")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .find_map(|tag| tag.strip_prefix("issue:"))
                .map(str::trim)
                .filter(|issue| !issue.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// One memory: the text and the ticket. Scope is not in the key. The
/// caller passes only the keys of the scope being adopted, so a private
/// copy of the same words does not swallow the cloud copy.
fn memory_key(atom: &Value) -> Option<String> {
    let text = atom
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if text.is_empty() {
        return None;
    }
    Some(crate::work_id(&format!("{text}\n{}", atom_issue(atom))))
}

/// Keys this host holds as its own in `scope`. An import tagged `sync:` is
/// not its own, and an atom of another scope is not this log's memory.
fn own_memory_keys(atoms: &[Value], scope: &str) -> BTreeSet<String> {
    let default = default_scope(scope);
    atoms
        .iter()
        .filter(|atom| is_own(atom) && atom_scope(atom, &default) == scope)
        .filter_map(memory_key)
        .collect()
}

/// The atoms to adopt from one gone host log.
///
/// One memory once. A memory this host already holds in `scope` is not
/// posted again. The repost is origin `peer`, whatever origin the sender
/// wrote, and it carries `scope:<scope>` so it stays in this log rather
/// than falling into the machine's default scope. Sender tags are dropped:
/// the atom is this host's own after the post.
#[must_use]
fn adopt_plan(plain: &str, scope: &str, own: &BTreeSet<String>) -> AdoptPlan {
    let mut plan = AdoptPlan::default();
    let scope_tag = format!("scope:{scope}");
    for line in plain.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(mut atom) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(key) = memory_key(&atom) else {
            continue;
        };
        if !plan.live.insert(key.clone()) || own.contains(&key) {
            continue;
        }
        if let Some(map) = atom.as_object_mut() {
            map.remove("id");
            // The sender's level would keep a user-declared atom explicit.
            // Origin `peer` is what the writer is asked to store; the level
            // is the writer's, from that origin, not the one in the log.
            map.remove("level");
            map.insert(
                "origin".into(),
                Value::String(crate::admit::ORIGIN_PEER.to_string()),
            );
            let mut entities: Vec<Value> = map
                .get("entities")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|entity| {
                    !entity
                        .as_str()
                        .is_some_and(|tag| tag.starts_with("sync:") || tag.starts_with("scope:"))
                })
                .cloned()
                .collect();
            entities.push(Value::String(scope_tag.clone()));
            map.insert("entities".into(), Value::Array(entities));
        }
        plan.post.push(atom);
    }
    plan
}

/// Read one host's log against the memories taken from it last time
/// (`before`, for what it retired) and the memories from it this pack
/// already holds (`held`, which are not posted again). Both are
/// [`memory_key`]s, so the same words on another ticket are not held.
#[must_use]
pub fn incoming(
    plain: &str,
    host: &str,
    before: &BTreeSet<String>,
    held: &BTreeSet<String>,
) -> Incoming {
    incoming_with_sender(plain, &sender_entity(host), before, held)
}

#[must_use]
fn incoming_with_sender(
    plain: &str,
    sender: &str,
    before: &BTreeSet<String>,
    held: &BTreeSet<String>,
) -> Incoming {
    let mut post = Vec::new();
    let mut now = BTreeSet::new();
    for line in plain.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(mut atom) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(key) = memory_key(&atom) else {
            continue;
        };
        now.insert(key.clone());
        if held.contains(&key) {
            continue;
        }
        if let Some(map) = atom.as_object_mut() {
            let mut entities: Vec<Value> = map
                .get("entities")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !entities.iter().any(|e| e.as_str() == Some(sender)) {
                entities.push(Value::String(sender.to_string()));
            }
            map.insert("entities".into(), Value::Array(entities));
            map.insert(
                "origin".into(),
                Value::String(crate::admit::ORIGIN_PEER.to_string()),
            );
            map.remove("id");
        }
        post.push(atom);
    }
    let retired = before.difference(&now).cloned().collect();
    Incoming { post, retired }
}

fn taken_path(root: &Path, scope: &str, host: &str) -> PathBuf {
    state_dir()
        .join(crate::work_id(&root.display().to_string()))
        .join(scope)
        .join(format!("{host}.taken"))
}

/// First line of a `.taken` file that lists [`memory_key`]s. A file
/// without it lists one [`crate::work_id`] of the text a line, written
/// before the ticket was in the key.
const TAKEN_KEYS: &str = "# text and issue";

/// The memories taken from `host` last time. A file of texts is read
/// through the atoms this pack holds from `sender`: each text stands for
/// every copy of it, so the first import after the change still retires
/// what the host dropped and posts nothing again.
fn read_taken(
    root: &Path,
    scope: &str,
    host: &str,
    atoms: &[Value],
    sender: &str,
) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(taken_path(root, scope, host)) else {
        return BTreeSet::new();
    };
    let mut lines = text.lines();
    if lines.next() == Some(TAKEN_KEYS) {
        return lines.map(str::to_string).collect();
    }
    let texts: BTreeSet<&str> = text.lines().collect();
    atoms
        .iter()
        .filter(|atom| has_entity(atom, sender))
        .filter(|atom| {
            atom["text"]
                .as_str()
                .is_some_and(|text| texts.contains(crate::work_id(text).as_str()))
        })
        .filter_map(memory_key)
        .collect()
}

fn write_taken(root: &Path, scope: &str, host: &str, plain: &str) -> Result<()> {
    let path = taken_path(root, scope, host);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let keys: String = log_keys(plain).into_iter().map(|key| key + "\n").collect();
    std::fs::write(path, format!("{TAKEN_KEYS}\n{keys}"))?;
    Ok(())
}

/// [`memory_key`] of every atom in a log.
fn log_keys(plain: &str) -> BTreeSet<String> {
    plain
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|atom| memory_key(&atom))
        .collect()
}

fn sender_entity_for_scope(host: &str, scope: &Scope, scope_name: &str) -> String {
    if scope_name == scope.name {
        sender_entity(host)
    } else {
        format!("sync:{host}+{scope_name}")
    }
}

/// Host files `<host>+<scope>.jsonl.age` and their digests for scopes this
/// repository no longer configures. Other machines' files stay: each host
/// deletes its own on export.
fn stale_scope_rels(root: &Path, scope: &Scope) -> Vec<String> {
    let me = host();
    let mut rels = Vec::new();
    let mut push = |name: &str| {
        if extra_scope_of_host_file(name, &me)
            .is_some_and(|scope_name| !scope.scopes.contains_key(&scope_name))
        {
            rels.push(format!(".ljos/atoms/{name}"));
        }
    };
    if let Ok(entries) = std::fs::read_dir(log_dir(root)) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                push(name);
            }
        }
    }
    if let Ok(out) = git(root, &["ls-files", "--", ".ljos/atoms"]) {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Some(name) = Path::new(line).file_name().and_then(|n| n.to_str()) {
                    push(name);
                }
            }
        }
    }
    rels.sort();
    rels.dedup();
    rels
}

fn extra_scope_of_host_file(name: &str, host: &str) -> Option<String> {
    let rest = name.strip_prefix(&format!("{host}+"))?;
    let scope_name = rest
        .strip_suffix(".jsonl.age")
        .or_else(|| rest.strip_suffix(".digest"))?;
    (!scope_name.is_empty() && !scope_name.contains('+')).then(|| scope_name.to_string())
}

fn drop_stale_scope_files(root: &Path, scope: &Scope) {
    for rel in stale_scope_rels(root, scope) {
        let path = root.join(rel);
        if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn remembered_hosts(root: &Path) -> Vec<(String, String)> {
    let dir = state_dir().join(crate::work_id(&root.display().to_string()));
    let Ok(scopes) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for scope_ent in scopes.flatten() {
        if !scope_ent.path().is_dir() {
            continue;
        }
        let scope_name = scope_ent.file_name().to_string_lossy().into_owned();
        let Ok(hosts) = std::fs::read_dir(scope_ent.path()) else {
            continue;
        };
        for host_ent in hosts.flatten() {
            let fname = host_ent.file_name().to_string_lossy().into_owned();
            if let Some(host) = fname.strip_suffix(".taken") {
                if !host.is_empty() {
                    out.push((scope_name.clone(), host.to_string()));
                }
            }
        }
    }
    out
}

/// Import every other machine's log of the repository's scope into this
/// seat's pack. Returns one line per host and scope.
///
/// Plans are taken from the pack as it stands. Every retirement runs,
/// then every post. The pack answers a text it already holds with that
/// claim, so posting a moved text and retiring the old scope afterwards
/// deletes the claim the new scope just answered. An adopt commit is
/// retired after those posts: the gone log is already absent, and its
/// `sync:` copies are what the commit names.
///
/// # Errors
///
/// The pack not answering, or a log this machine should open failing to.
pub fn import(root: &Path, scope: &Scope) -> Result<String> {
    let me = host();
    if scope
        .all_scopes()
        .iter()
        .all(|named| identities_for_open(named.name).is_empty())
    {
        return Ok("sync: no age identity on this machine; `ljos sync --key` makes one\n".into());
    }
    // Retirement reads the adopt commits, not the directory. A missing
    // atoms directory, or one that only holds this machine, still has to
    // reach that step: the adopt commit is what removes the gone log.
    let gone = adopted_hosts(root, &me);
    let (jobs, mut out) = import_jobs(root, scope, &me)?;
    let saw_file = any_log_file(root);
    if jobs.is_empty() && out.is_empty() && gone.is_empty() && !saw_file {
        return Ok(format!("sync: no {} logs yet\n", scope.name));
    }
    // Nothing opened and nothing to retire. Asking the pack here replaced
    // the skip lines with PACKSET_URL=off. An owned miss already returned
    // from `import_jobs`.
    if jobs.is_empty() && gone.is_empty() {
        return Ok(out);
    }
    let client = crate::pack()?;
    if !jobs.is_empty() {
        let workspace = client.workspace();
        let atoms = crate::atoms_lean(&client, &workspace)?;
        let plans: Vec<ImportPlan> = jobs
            .into_iter()
            .map(|job| {
                let sender = sender_entity_for_scope(&job.host, scope, &job.scope_name);
                let held = held_ids(&atoms, &sender);
                let incoming = incoming_with_sender(
                    &job.plain,
                    &sender,
                    &read_taken(root, &job.scope_name, &job.host, &atoms, &sender),
                    &held,
                );
                ImportPlan {
                    host: job.host,
                    scope_name: job.scope_name,
                    plain: job.plain,
                    sender,
                    incoming,
                }
            })
            .collect();
        let retired: Vec<usize> = plans
            .iter()
            .map(|plan| forget_retired(&atoms, &plan.sender, &plan.incoming.retired))
            .collect();
        for (plan, retired) in plans.iter().zip(retired) {
            let (kept, refused) = post_incoming(&client, &workspace, &plan.incoming.post);
            write_taken(root, &plan.scope_name, &plan.host, &plan.plain)?;
            out.push_str(&format!(
                "sync: {}: {kept} {} atoms taken, {refused} refused, {retired} retired\n",
                plan.host, plan.scope_name
            ));
        }
    }
    out.push_str(&retire_adopted(scope, &gone, &client)?);
    // A retirement that found nothing is not news. A tracker with no log
    // files still says so, as it did before adopt commits were read.
    if out.is_empty() && !saw_file {
        return Ok(format!("sync: no {} logs yet\n", scope.name));
    }
    Ok(out)
}

fn any_log_file(root: &Path) -> bool {
    std::fs::read_dir(log_dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.path().to_string_lossy().ends_with(".jsonl.age"))
}

struct ImportJob {
    host: String,
    scope_name: String,
    plain: String,
}

fn import_jobs(root: &Path, scope: &Scope, me: &str) -> Result<(Vec<ImportJob>, String)> {
    let mut jobs = Vec::new();
    let mut out = String::new();
    let mut seen = BTreeSet::new();
    // The public key of each identity this machine opens with, computed on
    // its first miss. A log sealed to one of them that still does not open
    // is an error, and it is raised before the pack is asked.
    let mut mine: BTreeMap<PathBuf, String> = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(log_dir(root)) {
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.to_string_lossy().ends_with(".jsonl.age"))
            .collect();
        // `+` sorts before `.`, so `<host>+cloud` would be read before
        // `<host>`. The primary log comes first; retirement of every log
        // still runs before any post, in `import`.
        paths.sort_by_key(|path| import_log_rank(path));
        for path in paths {
            let Some((host, scope_name)) = host_scope_from_log_path(&path, scope) else {
                continue;
            };
            if host.is_empty() || machine_name(&host) == me {
                continue;
            }
            seen.insert((host.clone(), scope_name.clone()));
            if !scope_is_configured(scope, &scope_name) {
                // The scope was removed or renamed. Do not read the log.
                // Everything previously taken from it retires.
                jobs.push(ImportJob {
                    host,
                    scope_name,
                    plain: String::new(),
                });
                continue;
            }
            let identities = identities_for_open(&scope_name);
            match open_identities(&path, &identities)? {
                Some(plain) => jobs.push(ImportJob {
                    host,
                    scope_name,
                    plain,
                }),
                None => {
                    // The log's own recipients. A cloud log lists different
                    // keys than the primary, and a miss is an error only
                    // when one of those keys is an identity this log was
                    // tried with.
                    let recipients = scope.recipients_for(&scope_name).unwrap_or(&[]);
                    let mut skip =
                        format!("sync: {host}'s {scope_name} log is not sealed to this machine\n");
                    for identity in &identities {
                        let key = match mine.entry(identity.clone()) {
                            std::collections::btree_map::Entry::Occupied(known) => {
                                known.get().clone()
                            }
                            std::collections::btree_map::Entry::Vacant(slot) => {
                                // A keygen miss is not a recipient we know.
                                // Aborting here dropped every later host.
                                match identity_recipient(identity) {
                                    Ok(key) => {
                                        slot.insert(key.clone());
                                        key
                                    }
                                    Err(_) => continue,
                                }
                            }
                        };
                        skip = miss_report(&host, &scope_name, &key, recipients)?;
                    }
                    out.push_str(&skip);
                }
            }
        }
    }
    for (scope_name, host) in remembered_hosts(root) {
        if host == me || scope_is_configured(scope, &scope_name) {
            continue;
        }
        if !seen.insert((host.clone(), scope_name.clone())) {
            continue;
        }
        jobs.push(ImportJob {
            host,
            scope_name,
            plain: String::new(),
        });
    }
    Ok((jobs, out))
}

struct ImportPlan {
    host: String,
    scope_name: String,
    plain: String,
    sender: String,
    incoming: Incoming,
}

/// `<host>.jsonl.age` before `<host>+<scope>.jsonl.age`.
fn import_log_rank(path: &Path) -> (u8, String) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    (u8::from(name.contains('+')), name)
}

fn has_entity(atom: &Value, entity: &str) -> bool {
    atom["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|tag| tag.as_str() == Some(entity))
}

/// Memories this pack already holds from `sender`, by [`memory_key`]. A
/// later post of one is skipped: the pack would answer with the claim it
/// has.
fn held_ids(atoms: &[Value], sender: &str) -> BTreeSet<String> {
    atoms
        .iter()
        .filter(|atom| has_entity(atom, sender))
        .filter_map(memory_key)
        .collect()
}

fn forget_retired(atoms: &[Value], sender: &str, gone: &[String]) -> usize {
    if gone.is_empty() {
        return 0;
    }
    let mut retired = 0usize;
    for atom in atoms {
        let Some(key) = memory_key(atom) else {
            continue;
        };
        if !gone.contains(&key) || !has_entity(atom, sender) {
            continue;
        }
        if let Some(id) = atom["id"].as_str() {
            if crate::packset_forget(id, None).is_ok() {
                retired += 1;
            }
        }
    }
    retired
}

fn post_incoming(
    client: &packset_client::PacksetClient,
    workspace: &str,
    post: &[Value],
) -> (usize, usize) {
    let (mut kept, mut refused) = (0usize, 0usize);
    for atom in post {
        let mut atom = atom.clone();
        if let Some(map) = atom.as_object_mut() {
            map.insert("workspace".into(), Value::String(workspace.to_string()));
        }
        match crate::admit::post_kept(client, &atom) {
            Ok(_) => kept += 1,
            Err(_) => refused += 1,
        }
    }
    (kept, refused)
}

/// Log stems whose newest deletion is an adopt commit, and whose file is
/// not back in HEAD. A stem is `host` or `host+scope`. A re-added log is
/// imported above; this set is what the other machines still hold as
/// `sync:` copies after the file is gone.
fn adopted_hosts(root: &Path, me: &str) -> BTreeSet<String> {
    adopted_stems(root)
        .into_iter()
        .filter(|stem| log_stem_host(stem).is_some_and(|host| host != me))
        .filter(|stem| !in_head(root, &format!(".ljos/atoms/{stem}.jsonl.age")))
        .collect()
}

/// Forget `sync:` copies for each log [`adopted_hosts`] named. A stem of
/// `host` is the primary scope; `host+scope` is that extra scope and its
/// sender is `sync:host+scope`. A forget that does not land is counted and
/// tried again on the next import. It does not fail the import, and it
/// does not skip the other logs. Quiet when there was nothing to forget.
fn retire_adopted(
    scope: &Scope,
    hosts: &BTreeSet<String>,
    client: &crate::PacksetClient,
) -> Result<String> {
    if hosts.is_empty() {
        return Ok(String::new());
    }
    let atoms = crate::atoms_lean(client, &client.workspace())?;
    let mut out = String::new();
    for stem in hosts {
        let Some(host) = log_stem_host(stem) else {
            continue;
        };
        let scope_name = stem
            .split_once('+')
            .map(|(_, extra)| extra)
            .unwrap_or(scope.name.as_str());
        let sender = match stem.split_once('+') {
            Some((_, extra)) => format!("sync:{host}+{extra}"),
            None => sender_entity(host),
        };
        let (retired, failed) =
            forget_tagged(&atoms, &sender, |atom| in_adopted_scope(atom, scope_name));
        if retired == 0 && failed == 0 {
            continue;
        }
        out.push_str(&format!(
            "sync: {host}: adopted away, {retired} {scope_name} atoms retired, {failed} still held\n"
        ));
    }
    Ok(out)
}

/// A host name that is safe as one path segment under `.ljos/atoms`.
///
/// [`machine_name`] folds `../x` into a different name. Adopting that
/// folded name would open some other host's log, so a name that is not
/// already a machine name is refused. The path is still built from
/// [`machine_name`], so a caller that skips this check cannot leave the
/// atoms directory.
fn require_host_name(raw: &str) -> Result<String> {
    let gone = raw.trim();
    if gone.is_empty() {
        bail!("sync: pass a host after --adopt");
    }
    if machine_name(gone) != gone {
        bail!("sync: {gone} is not a host name");
    }
    Ok(gone.to_string())
}

fn host_log_rels(host: &str) -> [String; 2] {
    let host = machine_name(host);
    [
        format!(".ljos/atoms/{host}.jsonl.age"),
        format!(".ljos/atoms/{host}.digest"),
    ]
}

struct HostLog {
    scope_name: String,
    age: String,
    digest: String,
}

/// Every log this repository would hold for `host`: the primary file, then
/// one `host+scope` file per extra scope. Absent files are not an error;
/// the caller decides which of these are actually in the tree.
fn logs_of_host(scope: &Scope, host: &str) -> Vec<HostLog> {
    let host = machine_name(host);
    scope
        .all_scopes()
        .into_iter()
        .map(|named| {
            let [age, digest] = host_log_rels_for_scope(&host, &scope.name, named.name);
            HostLog {
                scope_name: named.name.to_string(),
                age,
                digest,
            }
        })
        .collect()
}

fn tracked_path(root: &Path, rel: &str) -> bool {
    git(root, &["ls-files", "--error-unmatch", "--", rel]).is_ok_and(|o| o.status.success())
}

fn stageable_path(root: &Path, rel: &str) -> bool {
    root.join(rel).exists() || tracked_path(root, rel)
}

/// A host whose log moved within this window may still be alive. Adopting
/// it then leaves every text in both machines' logs.
const ADOPT_QUIET: std::time::Duration = std::time::Duration::from_secs(14 * 24 * 60 * 60);

/// What one `ljos sync --adopt` may do beyond posting.
#[derive(Clone, Copy)]
struct AdoptOpts {
    push: bool,
    /// Adopt even when the log was committed inside [`ADOPT_QUIET`].
    force: bool,
    /// Print the plan and do not post, forget, or commit.
    dry_run: bool,
}

/// Where the gone host's log stands relative to HEAD.
#[derive(Clone, Copy)]
enum GoneLog {
    /// Tracked, and the worktree matches HEAD.
    Clean,
    /// In HEAD, missing on disk: the deletion was started and not committed.
    Resume,
    /// Tracked, and the worktree differs. `git rm` would refuse.
    Dirty,
    /// On disk, not in HEAD. Deleting it would not be recoverable.
    Untracked,
    /// Neither in HEAD nor on disk.
    Absent,
}

fn in_head(root: &Path, rel: &str) -> bool {
    let spec = format!("HEAD:{rel}");
    git(root, &["cat-file", "-e", &spec]).is_ok_and(|out| out.status.success())
}

fn gone_log_state(root: &Path, rel: &str) -> GoneLog {
    let on_disk = root.join(rel).is_file();
    let tracked = in_head(root, rel);
    if tracked && !on_disk {
        return GoneLog::Resume;
    }
    if on_disk && !tracked {
        return GoneLog::Untracked;
    }
    if !tracked {
        return GoneLog::Absent;
    }
    let clean =
        git(root, &["diff", "--quiet", "HEAD", "--", rel]).is_ok_and(|out| out.status.success());
    if clean {
        GoneLog::Clean
    } else {
        GoneLog::Dirty
    }
}

fn log_committed_at(root: &Path, rel: &str) -> Option<i64> {
    let out = git(root, &["log", "-1", "--format=%ct", "--", rel]).ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|age| i64::try_from(age.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Whether `committed_at` is old enough to treat the host as gone.
fn host_is_quiet(committed_at: i64, now: i64) -> bool {
    let quiet = i64::try_from(ADOPT_QUIET.as_secs()).unwrap_or(i64::MAX);
    now.saturating_sub(committed_at) >= quiet
}

fn age_phrase(committed_at: i64, now: i64) -> String {
    let secs = now.saturating_sub(committed_at).max(0);
    let days = secs / 86_400;
    if days >= 1 {
        format!("{days} days ago")
    } else {
        format!("{} hours ago", secs / 3600)
    }
}

fn scope_names(scopes: &[String]) -> String {
    if scopes.is_empty() {
        "none".to_string()
    } else {
        scopes.join(", ")
    }
}

fn adopt_scope_summary(gone: &str, adopted: &[String], skipped: &[String]) -> String {
    format!(
        "sync: {gone}: adopted scopes {}; skipped scopes {}\n",
        scope_names(adopted),
        scope_names(skipped)
    )
}

fn head_blob(root: &Path, rel: &str) -> Result<Vec<u8>> {
    let spec = format!("HEAD:{rel}");
    let out = git(root, &["cat-file", "blob", &spec])?;
    if !out.status.success() {
        bail!("sync: {rel} is not in this tracker");
    }
    Ok(out.stdout)
}

/// Decrypt `bytes` with `identities`. The bytes are a sealed log, which
/// `age` reads from a path, so they go to a temporary file that is removed
/// before this returns.
/// A sealed blob `age` will only read from a path. Removed when dropped,
/// including when decrypt fails.
struct SealedTemp(PathBuf);

impl Drop for SealedTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn open_sealed_bytes(bytes: &[u8], identities: &[PathBuf]) -> Result<Option<String>> {
    static TEMPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = std::env::temp_dir().join(format!(
        "ljos-adopt-{}-{}.age",
        std::process::id(),
        TEMPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&tmp, bytes)?;
    let _remove = SealedTemp(tmp.clone());
    open_identities(&tmp, identities)
}

fn sealed_plain(root: &Path, rel: &str, identities: &[PathBuf]) -> Result<Option<String>> {
    let bytes = if root.join(rel).is_file() {
        std::fs::read(root.join(rel))?
    } else if in_head(root, rel) {
        head_blob(root, rel)?
    } else {
        return Ok(None);
    };
    open_sealed_bytes(&bytes, identities)
}

/// Memories in `before` (the keys [`read_taken`] returned) that `plain` no
/// longer carries. The same difference import uses to retire a host's
/// copies.
fn retired_keys(plain: &str, before: &BTreeSet<String>) -> BTreeSet<String> {
    before.difference(&log_keys(plain)).cloned().collect()
}

/// Forget atoms tagged `sender` for which `keep` is true. The count is
/// how many the pack dropped, and how many it still holds. `sender` is
/// `sync:<host>` for the primary log and `sync:<host>+<scope>` for an
/// extra one; those are different copies.
fn forget_tagged(atoms: &[Value], sender: &str, keep: impl Fn(&Value) -> bool) -> (usize, usize) {
    let mut done = 0usize;
    let mut failed = 0usize;
    for atom in atoms {
        let tagged = atom["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|entity| entity.as_str() == Some(sender));
        if !tagged || !keep(atom) {
            continue;
        }
        let Some(id) = atom["id"].as_str().filter(|id| !id.is_empty()) else {
            failed += 1;
            continue;
        };
        if crate::packset_forget(id, None).is_ok() {
            done += 1;
        } else {
            failed += 1;
        }
    }
    (done, failed)
}

struct PackAdopt {
    kept: usize,
    refused: usize,
    moved: usize,
    retired: usize,
    /// A post was not visible as this host's own atom afterwards, or a
    /// forget did not land. The gone log has to stay.
    failed: bool,
}

/// Post the plan, retire texts the gone host already dropped, and forget
/// `sync:` copies only where a different own atom of that memory exists.
/// An explicit `scope:` of another repository stays. Import does not add
/// one, so a copy with no scope tag is this log's atom, not the default
/// scope's.
fn in_adopted_scope(atom: &Value, scope: &str) -> bool {
    atom["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|tag| tag.strip_prefix("scope:"))
        .is_none_or(|name| name == scope)
}

fn adopt_pack(
    root: &Path,
    scope_name: &str,
    gone: &str,
    plain: &str,
    sender: &str,
) -> Result<PackAdopt> {
    let client = crate::pack()?;
    let workspace = client.workspace();
    let before = crate::atoms_lean(&client, &workspace)?;
    let own = own_memory_keys(&before, scope_name);
    // Import's retirement, run before the log is dropped. It is by
    // memory, so a ticket the gone host dropped is retired even while the
    // same words remain on another ticket.
    let taken = read_taken(root, scope_name, gone, &before, sender);
    let dropped = retired_keys(plain, &taken);
    let (retired, retire_failed) = forget_tagged(&before, sender, |atom| {
        in_adopted_scope(atom, scope_name)
            && memory_key(atom).is_some_and(|key| dropped.contains(&key))
    });
    let plan = adopt_plan(plain, scope_name, &own);
    for atom in &plan.post {
        let mut posted = atom.clone();
        if let Some(map) = posted.as_object_mut() {
            map.insert("workspace".into(), Value::String(workspace.clone()));
        }
        let _ = crate::admit::post_kept(&client, &posted);
    }
    // The pack may answer the post with the `sync:` atom it already holds
    // when the text matches. That Ok is not an adoption: forget only when
    // a re-list shows a different own atom of the same memory.
    let after = crate::atoms_lean(&client, &workspace)?;
    let owned = own_memory_keys(&after, scope_name);
    let mut kept = 0usize;
    let mut refused = 0usize;
    for atom in &plan.post {
        if memory_key(atom).is_some_and(|key| owned.contains(&key)) {
            kept += 1;
        } else {
            refused += 1;
        }
    }
    let (moved, move_failed) = forget_tagged(&after, sender, |atom| {
        in_adopted_scope(atom, scope_name)
            && memory_key(atom).is_some_and(|key| plan.live.contains(&key) && owned.contains(&key))
    });
    Ok(PackAdopt {
        kept,
        refused,
        moved,
        retired,
        failed: retire_failed > 0 || refused > 0 || move_failed > 0,
    })
}

fn dry_adopt_line(
    root: &Path,
    scope_name: &str,
    gone: &str,
    plain: &str,
    rel: &str,
    sender: &str,
) -> Result<String> {
    let client = crate::pack()?;
    let atoms = crate::atoms_lean(&client, &client.workspace())?;
    let own = own_memory_keys(&atoms, scope_name);
    let plan = adopt_plan(plain, scope_name, &own);
    let taken = read_taken(root, scope_name, gone, &atoms, sender);
    let retired = retired_keys(plain, &taken);
    Ok(format!(
        "sync: {gone}: would adopt {} {scope_name} atoms, retire {}, and drop {rel}\n",
        plan.post.len(),
        retired.len()
    ))
}

/// The subject import recognises as "this host was adopted". The host is
/// the first word, so a name with a space cannot be one; [`machine_name`]
/// does not produce one.
fn adopt_commit_subject(subject: &str, host: &str) -> bool {
    let Some(rest) = subject.trim().strip_prefix("chore(sync): adopt ") else {
        return false;
    };
    let Some((named, tail)) = rest.split_once(' ') else {
        return false;
    };
    named == host && tail.ends_with(" atoms")
}

/// The host a log stem names. `gone` is the primary file. `gone+cloud` is
/// host `gone` and the extra scope `cloud`. A `+` with nothing on one side
/// is not a file this crate writes.
fn log_stem_host(stem: &str) -> Option<&str> {
    match stem.split_once('+') {
        Some((host, extra)) if !host.is_empty() && !extra.is_empty() => Some(host),
        Some(_) => None,
        None if stem.is_empty() => None,
        None => Some(stem),
    }
}

fn age_stem(rel: &str) -> Option<&str> {
    rel.strip_prefix(".ljos/atoms/")?.strip_suffix(".jsonl.age")
}

/// Stems whose newest deletion under `.ljos/atoms` is an adopt commit.
/// A stem is `host` or `host+scope`. `log` is `git log --diff-filter=D
/// --pretty=format:%x1e%s --name-only`, newest first. A later deletion
/// that is not an adopt wins. The subject names the host, not the whole
/// stem, so one adopt commit covers the primary file and every extra
/// scope file it deletes. The caller drops a stem whose file is back.
fn hosts_removed_by_adopt(log: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut adopted = BTreeSet::new();
    let mut subject = String::new();
    for line in log.lines() {
        if let Some(rest) = line.strip_prefix('\u{1e}') {
            subject = rest.to_string();
            continue;
        }
        let Some(name) = line.strip_prefix(".ljos/atoms/") else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".jsonl.age") else {
            continue;
        };
        let Some(host) = log_stem_host(stem) else {
            continue;
        };
        if !seen.insert(stem.to_string()) {
            continue;
        }
        if adopt_commit_subject(&subject, host) {
            adopted.insert(stem.to_string());
        }
    }
    adopted
}

fn adopted_stems(root: &Path) -> BTreeSet<String> {
    let Ok(out) = git(
        root,
        &[
            "log",
            "--diff-filter=D",
            "--pretty=format:%x1e%s",
            "--name-only",
            "--",
            ".ljos/atoms",
        ],
    ) else {
        return BTreeSet::new();
    };
    if !out.status.success() {
        return BTreeSet::new();
    }
    hosts_removed_by_adopt(&String::from_utf8_lossy(&out.stdout))
}

fn catch_up_failed(said: &str) -> bool {
    said.contains("fetch failed") || said.contains("could not merge")
}

fn git_first_line(out: &std::process::Output) -> String {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("git failed")
        .trim()
        .to_string()
}

/// Remove a tracked path so the following commit records the deletion.
///
/// `git rm` refuses a dirty file, which is what keeps an uncommitted edit.
/// It also drops the path from the index, and `commit --only` then cannot
/// name it, so the HEAD entry is put back without restoring the worktree
/// file. A crash after `git rm` leaves that staged deletion; reset still
/// makes the path committable. The caller holds [`crate::tracker_commit_lock`].
fn stage_deletion(root: &Path, rel: &str) -> Result<()> {
    let dry = git(root, &["rm", "-n", "--ignore-unmatch", "--", rel])?;
    if !dry.status.success() {
        bail!(
            "sync: {rel} was not removed ({}); the log was kept",
            git_first_line(&dry)
        );
    }
    let removed = git(root, &["rm", "--ignore-unmatch", "--", rel])?;
    if !removed.status.success() {
        bail!(
            "sync: {rel} was not removed ({}); the log was kept",
            git_first_line(&removed)
        );
    }
    let reset = git(root, &["reset", "-q", "--", rel])?;
    if !reset.status.success() {
        bail!(
            "sync: {rel} was not removed ({}); the log was kept",
            git_first_line(&reset)
        );
    }
    if root.join(rel).exists() {
        bail!("sync: {rel} is still on disk; the log was kept");
    }
    Ok(())
}

struct CommitReport {
    said: String,
    /// `Made` or `AlreadyIn`: the paths are in HEAD. A refused commit is
    /// not, and the taken file has to stay so the next run can finish.
    landed: bool,
}

/// The words [`commit_files`] prints. The caller has dropped the tracker
/// lock; a push takes it again inside `prepare_tracker`.
fn report_commit(root: &Path, message: &str, push: bool, made: crate::PathCommit) -> CommitReport {
    match made {
        crate::PathCommit::Refused(why) => {
            return CommitReport {
                said: format!("sync: commit refused: {why}\n"),
                landed: false,
            };
        }
        crate::PathCommit::AlreadyIn => {
            return CommitReport {
                said: if push {
                    push_pending(root)
                } else {
                    kept_local(root)
                },
                landed: true,
            };
        }
        crate::PathCommit::Made => {}
    }
    if !push {
        return CommitReport {
            said: format!("sync: committed {message}; not pushed (LJOS_TRACKER_GIT=commit)\n"),
            landed: true,
        };
    }
    let remotes = git(root, &["remote"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if remotes.is_empty() {
        return CommitReport {
            said: format!(
                "sync: committed {message}; no remote is configured, so nothing was pushed. \
                 Set one with `git -C {} remote add origin URL` and `git push -u origin HEAD`.\n",
                root.display()
            ),
            landed: true,
        };
    }
    match crate::tracker_push::push_checkout(root) {
        Ok(()) => CommitReport {
            said: format!("sync: committed {message}; pushed\n"),
            landed: true,
        },
        Err(e) => CommitReport {
            said: format!("sync: committed {message}; push refused: {e}\n"),
            landed: true,
        },
    }
}

fn adopt_in_repo(root: &Path, scope: &Scope, gone: &str, opts: AdoptOpts) -> Result<String> {
    let gone = require_host_name(gone)?;
    let me = host();
    if gone == me {
        bail!("sync: {gone} is this machine; pass a gone host");
    }
    // One host, every scope. A dirty or untracked file stops the whole
    // adopt before the pack is asked, so a bad extra log cannot delete
    // the primary one. An absent file is skipped; if every file is
    // absent, the line is the same one a primary-only repository prints.
    let adopted = adopted_stems(root);
    let mut present = Vec::new();
    for log in logs_of_host(scope, &gone) {
        match gone_log_state(root, &log.age) {
            GoneLog::Absent => {
                if age_stem(&log.age).is_some_and(|stem| adopted.contains(stem)) {
                    let _ = std::fs::remove_file(taken_path(root, &log.scope_name, &gone));
                }
            }
            GoneLog::Untracked => bail!(
                "sync: {gone}'s {} log is not committed; it was left in place",
                log.scope_name
            ),
            GoneLog::Dirty => bail!(
                "sync: {gone}'s {} log differs from the committed file; it was left in place",
                log.scope_name
            ),
            GoneLog::Clean | GoneLog::Resume => {
                if matches!(gone_log_state(root, &log.digest), GoneLog::Dirty) {
                    bail!(
                        "sync: {gone}'s {} digest differs from the committed file; the log was left in place",
                        log.scope_name
                    );
                }
                present.push(log);
            }
        }
    }
    if present.is_empty() {
        return Ok(format!(
            "sync: {gone} has no {} log in this tracker\n",
            scope.name
        ));
    }
    // A resume already started the deletion, and refusing it would leave
    // the worktree file gone and the commit unmade. Recipients are age
    // keys, not host names, so a host still listed there cannot be told
    // apart from one that left; the log's last commit is the signal. Any
    // log that is still young, and not a resume, refuses the rest: adopting
    // the quiet scope alone would leave the live one in place.
    if !opts.force {
        let now = unix_now();
        for log in &present {
            if matches!(gone_log_state(root, &log.age), GoneLog::Resume) {
                continue;
            }
            match log_committed_at(root, &log.age) {
                Some(at) if host_is_quiet(at, now) => {}
                Some(at) => bail!(
                    "sync: {gone}'s {} log was committed {}; pass --force to adopt a host that may still be alive",
                    log.scope_name,
                    age_phrase(at, now)
                ),
                None => bail!(
                    "sync: {gone}'s {} log has no commit time; pass --force to adopt a host that may still be alive",
                    log.scope_name
                ),
            }
        }
    }
    if scope
        .all_scopes()
        .iter()
        .all(|named| identities_for_open(named.name).is_empty())
    {
        return Ok("sync: no age identity on this machine; `ljos sync --key` makes one\n".into());
    }
    let mut out = String::new();
    let mut opened = Vec::with_capacity(present.len());
    let mut adopted_scopes = Vec::new();
    let mut skipped_scopes = Vec::new();
    for log in &present {
        let identities = identities_for_open(&log.scope_name);
        match sealed_plain(root, &log.age, &identities)? {
            Some(plain) => {
                adopted_scopes.push(log.scope_name.clone());
                opened.push((log, plain));
            }
            None => {
                skipped_scopes.push(log.scope_name.clone());
                out.push_str(&format!(
                    "sync: {gone}'s {} log is not sealed to this machine\n",
                    log.scope_name
                ));
            }
        }
    }
    if opened.is_empty() {
        out.push_str(&adopt_scope_summary(
            &gone,
            &adopted_scopes,
            &skipped_scopes,
        ));
        return Ok(out);
    }
    if opts.dry_run {
        for (log, plain) in &opened {
            let sender = sender_entity_for_scope(&gone, scope, &log.scope_name);
            out.push_str(&dry_adopt_line(
                root,
                &log.scope_name,
                &gone,
                plain,
                &log.age,
                &sender,
            )?);
        }
        out.push_str(&adopt_scope_summary(
            &gone,
            &adopted_scopes,
            &skipped_scopes,
        ));
        return Ok(out);
    }
    let mut rows = Vec::with_capacity(opened.len());
    let mut failed = false;
    for (log, plain) in &opened {
        let sender = sender_entity_for_scope(&gone, scope, &log.scope_name);
        let done = adopt_pack(root, &log.scope_name, &gone, plain, &sender)?;
        failed |= done.failed;
        rows.push(done);
    }
    for ((log, _), done) in opened.iter().zip(&rows) {
        out.push_str(&format!(
            "sync: {gone}: {} {} atoms adopted, {} refused, {} moved, {} retired",
            done.kept, log.scope_name, done.refused, done.moved, done.retired
        ));
        // One scope failing keeps every file. Saying so on each line is
        // what a single-scope failure already printed, and it stops a
        // sibling scope from looking deleted.
        if failed {
            out.push_str("; the log was kept");
        }
        out.push('\n');
    }
    out.push_str(&adopt_scope_summary(
        &gone,
        &adopted_scopes,
        &skipped_scopes,
    ));
    if failed {
        return Ok(out);
    }
    let client = crate::pack()?;
    let atoms = crate::atoms_lean(&client, &client.workspace())?;
    let exported = if scope.scopes.is_empty() {
        export(root, scope, &atoms)?
    } else {
        export_all(root, scope, &atoms)?
    };
    if exported.contains("age is not installed") {
        bail!(
            "{out}sync: age is not installed, so adoption cannot seal {me}'s log; install age and try again"
        );
    }
    out.push_str(&exported);
    let mut drop_rels = Vec::new();
    for (log, _) in &opened {
        drop_rels.push(log.age.clone());
        if matches!(
            gone_log_state(root, &log.digest),
            GoneLog::Clean | GoneLog::Resume
        ) {
            drop_rels.push(log.digest.clone());
        }
    }
    // One subject for every scope file. Import matches the host, the
    // first word, and reads a `host+scope` path in this commit as that
    // extra scope. The primary name is enough.
    let message = format!("chore(sync): adopt {gone} {} atoms", scope.name);
    let made = {
        let held = crate::tracker_commit_lock(root);
        for rel in &drop_rels {
            if let Err(err) = stage_deletion(root, rel) {
                drop(held);
                bail!("{out}{err}");
            }
        }
        let mut stage = drop_rels;
        let mine: Vec<String> = if scope.scopes.is_empty() {
            host_log_rels(&me).to_vec()
        } else {
            scope
                .all_scopes()
                .into_iter()
                .flat_map(|named| host_log_rels_for_scope(&me, &scope.name, named.name))
                .collect()
        };
        for rel in mine {
            if stageable_path(root, &rel) {
                stage.push(rel);
            }
        }
        for rel in stale_scope_rels(root, scope) {
            if !stage.contains(&rel) && stageable_path(root, &rel) {
                stage.push(rel);
            }
        }
        let refs: Vec<&str> = stage.iter().map(String::as_str).collect();
        let made = crate::commit_paths(root, &refs, &message);
        drop(held);
        made
    };
    let report = report_commit(root, &message, opts.push, made);
    if report.landed {
        for (log, _) in &opened {
            if !in_head(root, &log.age) {
                let _ = std::fs::remove_file(taken_path(root, &log.scope_name, &gone));
            }
        }
    }
    out.push_str(&report.said);
    if opened.iter().any(|(log, _)| in_head(root, &log.age)) {
        out.push_str("sync: the log was kept\n");
    }
    Ok(out)
}

/// The scope of the repository that holds an issue, when it declares one.
#[must_use]
pub fn scope_for_issue(issue: &str) -> Option<String> {
    let hit = vissue_core::Layout::resolve(None, None)
        .and_then(vissue_core::Router::load)
        .and_then(|router| router.find_by_id(issue))
        .ok()?;
    let root = hit
        .path
        .ancestors()
        .find(|d| d.join(".git").exists())?
        .to_path_buf();
    scope_of_repo(&root)
        .ok()
        .flatten()
        .map(|s| s.for_project(&hit.project))
}

/// The tracker repository this seat writes, its root directory.
fn tracker_root() -> Result<PathBuf> {
    let layout = vissue_core::Layout::resolve(None, None).map_err(anyhow::Error::from)?;
    Ok(layout.root().to_path_buf())
}

fn git(root: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut cmd = crate::git_at(root);
    cmd.args(args).stdin(Stdio::null()).output()
}

/// Commit the log after an export. A skipped seal with no file yet has
/// nothing to stage, and that is not a failure. `push` false keeps the
/// commit on this machine, and says so when a commit is waiting: the
/// ticket commit and this log share one branch, and a silent return was
/// how a later push of the log carried the ticket off the machine.
fn commit_if_sealed(root: &Path, scope: &str, exported: &str, push: bool) -> String {
    let age = log_dir(root).join(format!("{}.jsonl.age", host()));
    if exported.contains("age is not installed") && !age.is_file() {
        return if push {
            String::new()
        } else {
            kept_local(root)
        };
    }
    commit_log(root, scope, push)
}

fn any_scope_log_file(root: &Path, scope: &Scope, host: &str) -> bool {
    scope.all_scopes().into_iter().any(|named_scope| {
        let [log_rel, _] = host_log_rels_for_scope(host, &scope.name, named_scope.name);
        root.join(log_rel).is_file()
    })
}

fn commit_if_sealed_scopes(root: &Path, scope: &Scope, exported: &str, push: bool) -> String {
    let me = host();
    if exported.contains("age is not installed") && !any_scope_log_file(root, scope, &me) {
        return if push {
            String::new()
        } else {
            kept_local(root)
        };
    }
    commit_logs(root, scope, push)
}

/// The line finish prints when commit-only mode leaves the branch here.
fn kept_local(root: &Path) -> String {
    if commits_to_push(root) {
        "sync: not pushed (LJOS_TRACKER_GIT=commit)\n".into()
    } else {
        String::new()
    }
}

fn commit_files(root: &Path, files: &[&str], message: &str, push: bool) -> String {
    let held = crate::tracker_commit_lock(root);
    let made = crate::commit_paths(root, files, message);
    drop(held);
    // No upstream yet: push_checkout sets it with `-u origin <branch>`.
    // A sibling bot's push is rebased, and an org conflict keeps both
    // headings. Failure stays a failure; the line does not say the log was
    // pushed.
    report_commit(root, message, push, made).said
}

/// Push commits this checkout has not sent. Atoms that did not change used
/// to return before this, so a `.gitattributes` commit sat unpushed.
/// Nothing to send, and a checkout with no remote, stay quiet.
fn push_pending(root: &Path) -> String {
    let remotes = git(root, &["remote"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if remotes.is_empty() || !commits_to_push(root) {
        return String::new();
    }
    match crate::tracker_push::push_checkout(root) {
        Ok(()) => "sync: pushed pending commits\n".into(),
        Err(e) => format!("sync: push refused: {e}\n"),
    }
}

/// Whether `HEAD` has a commit the upstream does not. No upstream, with a
/// remote, is a branch that has never been pushed.
fn commits_to_push(root: &Path) -> bool {
    let ahead = |spec: &str| -> Option<u64> {
        let out = git(root, &["rev-list", "--count", spec]).ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    };
    if ahead("@{upstream}..HEAD").is_some_and(|n| n > 0) {
        return true;
    }
    if git(root, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_ok_and(|o| o.status.success()) {
        return false;
    }
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if branch.is_empty() || branch == "HEAD" {
        return git(root, &["rev-parse", "--verify", "HEAD"]).is_ok_and(|o| o.status.success());
    }
    let remote_ref = format!("origin/{branch}");
    match ahead(&format!("{remote_ref}..HEAD")) {
        Some(n) => n > 0,
        None => git(root, &["rev-parse", "--verify", "HEAD"]).is_ok_and(|o| o.status.success()),
    }
}

/// Commit this host's log alone and push it to the upstream and every other
/// remote that carries the branch. `push` false commits and pushes nothing,
/// not even a commit already waiting.
fn commit_log(root: &Path, scope: &str, push: bool) -> String {
    let host = host();
    let files = [
        format!(".ljos/atoms/{host}.jsonl.age"),
        format!(".ljos/atoms/{host}.digest"),
    ];
    let message = format!("chore(sync): {host} {scope} atoms");
    commit_files(root, &[&files[0], &files[1]], &message, push)
}

fn commit_logs(root: &Path, scope: &Scope, push: bool) -> String {
    let me = host();
    drop_stale_scope_files(root, scope);
    let mut refs: Vec<String> = scope
        .all_scopes()
        .into_iter()
        .flat_map(|named_scope| host_log_rels_for_scope(&me, &scope.name, named_scope.name))
        .filter(|rel| stageable_path(root, rel))
        .collect();
    for rel in stale_scope_rels(root, scope) {
        if !refs.contains(&rel) && stageable_path(root, &rel) {
            refs.push(rel);
        }
    }
    if refs.is_empty() {
        return if push {
            push_pending(root)
        } else {
            kept_local(root)
        };
    }
    let files: Vec<&str> = refs.iter().map(String::as_str).collect();
    let message = format!("chore(sync): {me} {} atoms", scope.name);
    commit_files(root, &files, &message, push)
}

/// Fetch every remote and fast-forward to each one that is ahead. Machines
/// that push to different remotes of one tracker then read each other's
/// logs whenever the history is linear; a real divergence stays for the
/// doctor's split row and a person.
fn catch_up(root: &Path) -> String {
    if git(root, &["fetch", "-q", "--all"]).map_or(true, |o| !o.status.success()) {
        return "sync: fetch failed; reading the logs as they are\n".into();
    }
    let refs = followed_refs(root);
    if refs.is_empty() {
        return String::new();
    }
    let mut stuck = Vec::new();
    let _held = crate::tracker_commit_lock(root);
    for r in refs {
        let forward = git(root, &["merge", "-q", "--ff-only", &r]);
        if forward.is_ok_and(|o| o.status.success()) {
            continue;
        }
        // Diverged: this host committed while another pushed. Merge, as the
        // tracker's own push does; a merge that stops is undone and named.
        let merged = git(root, &["merge", "-q", "--no-edit", &r]);
        if !merged.is_ok_and(|o| o.status.success()) {
            let _ = git(root, &["merge", "--abort"]);
            stuck.push(r);
        }
    }
    if stuck.is_empty() {
        String::new()
    } else {
        format!(
            "sync: could not merge {}; `ljos doctor` names the split\n",
            stuck.join(", ")
        )
    }
}

/// The remote-tracking refs [`catch_up`] merges: the upstream, and the
/// same branch on every other remote. None when there is no upstream.
fn followed_refs(root: &Path) -> Vec<String> {
    let Some(up) = crate::tracker_upstream(root) else {
        return Vec::new();
    };
    let mut refs = vec![up.clone()];
    refs.extend(
        crate::tracker_mirrors(root, &up)
            .unwrap_or_default()
            .into_iter()
            .map(|(r, b)| format!("{r}/{b}")),
    );
    refs
}

/// One pass over the tracker repository's scope: pull, take every other
/// machine's log, write and push this machine's. `pull_import` and `export`
/// pick the halves; the sitting takes the first, finish the second.
///
/// # Errors
///
/// A scope file that does not parse, the pack not answering, or age
/// refusing to seal.
pub fn sync_repo(pull_import: bool, export_log: bool) -> Result<String> {
    sync_repos(pull_import, export_log, true)
}

/// [`sync_repo`] as the sitting and finish run it: a repository with no
/// scope file is skipped without a word. Sync is opt-in, and a hint on
/// every sitting pointed agents at a remote the person had not asked for.
/// `ljos sync` by hand still names the missing file.
///
/// # Errors
///
/// As [`sync_repo`].
pub fn sync_repo_in_passing(pull_import: bool, export_log: bool) -> Result<String> {
    sync_repos(pull_import, export_log, false)
}

/// Export this machine's sealed log and push it.
///
/// A wake in push mode does this before it fires. The letter is already
/// written, and the send has already returned: a refusal is for the wake
/// log, not an error the send should grow. A tracker with no scope has
/// nothing to push, and that is not a refusal.
///
/// # Errors
///
/// The pack did not answer, age refused to seal, or the commit or the
/// push was refused. The message is the line the wake log stores.
pub(crate) fn push_sealed_log() -> Result<()> {
    let said = sync_repo_in_passing(false, true)?;
    if let Some(why) = said.lines().map(str::trim).find(|line| {
        line.contains("push refused")
            || line.contains("commit refused")
            || line.contains("no remote is configured")
    }) {
        bail!("{why}");
    }
    Ok(())
}

/// The commit at `HEAD` whose tree holds this machine's sealed log.
///
/// A wake in push mode reads this after the push and puts it on the
/// envelope. A refused push still leaves the commit here, and the next
/// push sends it. `None` when no tracker checkout holds the log.
pub(crate) fn log_commit() -> Option<String> {
    let spec = format!("HEAD:.ljos/atoms/{}.jsonl.age", host());
    for root in &configured_roots() {
        let Some(out) = git(root, &["rev-parse", "HEAD"]).ok() else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if sha.is_empty() {
            continue;
        }
        if git(root, &["cat-file", "-e", &spec]).is_ok_and(|o| o.status.success()) {
            return Some(sha);
        }
    }
    None
}

/// Tracker checkouts this machine shares. Named repos, or the tracker
/// root when none are named. A missing tracker is no checkouts.
fn configured_roots() -> Vec<PathBuf> {
    let listed = local().repos;
    if listed.is_empty() {
        tracker_root().ok().into_iter().collect()
    } else {
        listed.iter().map(|r| expand(r)).collect()
    }
}

fn has_remote(root: &Path) -> bool {
    git(root, &["remote"])
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

fn hex_commit(commit: &str) -> bool {
    (7..=64).contains(&commit.len()) && commit.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_ancestor(root: &Path, commit: &str) -> bool {
    git(root, &["merge-base", "--is-ancestor", commit, "HEAD"]).is_ok_and(|o| o.status.success())
}

/// Whether a fetched remote-tracking ref reaches `commit`. The object
/// store is not asked: a commit made here and never pushed is in it too.
fn on_remote(root: &Path, commit: &str) -> bool {
    followed_refs(root).iter().any(|remote_ref| {
        git(root, &["merge-base", "--is-ancestor", commit, remote_ref])
            .is_ok_and(|o| o.status.success())
    })
}

/// Bring `commit` into `HEAD` when a fetched remote ref already has it.
///
/// A commit no remote ref reaches is named and not fast-forwarded to,
/// even when this checkout holds it, and the caller still imports the
/// remote head. A fetch that already failed is not named twice.
fn note_commit(root: &Path, commit: &str, fetched: bool) -> String {
    if !on_remote(root, commit) {
        return if fetched {
            format!("sync: {commit} is not on the remote; imported the remote head\n")
        } else {
            String::new()
        };
    }
    if is_ancestor(root, commit) {
        return String::new();
    }
    let _held = crate::tracker_commit_lock(root);
    let forwarded =
        git(root, &["merge", "-q", "--ff-only", commit]).is_ok_and(|o| o.status.success());
    if forwarded && is_ancestor(root, commit) {
        return String::new();
    }
    format!("sync: {commit} is not in this checkout; imported the remote head\n")
}

/// How long `ljos inbox --sync COMMIT` keeps fetching while no remote ref
/// has the commit: `LJOS_INBOX_SYNC_WAIT` seconds, else 30. `0` fetches
/// once.
fn inbox_sync_wait() -> std::time::Duration {
    let secs = std::env::var("LJOS_INBOX_SYNC_WAIT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(30);
    std::time::Duration::from_secs(secs)
}

/// [`catch_up`] every root until one has `commit` on a remote ref, or
/// `wait` is spent. The wake can name a commit whose push has not landed
/// yet, so the pause between passes starts at a quarter second and
/// doubles up to four. When every fetch fails there is no next pass. The
/// notes are each root's from the last pass.
fn catch_up_until(roots: &[PathBuf], commit: &str, wait: std::time::Duration) -> Vec<String> {
    let deadline = std::time::Instant::now() + wait;
    let mut pause = std::time::Duration::from_millis(250);
    loop {
        let said: Vec<String> = roots.iter().map(|root| catch_up(root)).collect();
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero()
            || said.iter().all(|line| line.contains("fetch failed"))
            || roots.iter().any(|root| on_remote(root, commit))
        {
            return said;
        }
        std::thread::sleep(pause.min(left));
        pause = (pause * 2).min(std::time::Duration::from_secs(4));
    }
}

/// Fetch the tracker and import its sealed logs for `ljos inbox --sync`.
///
/// `commit` asks that this checkout contain that commit, and the fetch is
/// tried again until a remote ref has it, for up to [`inbox_sync_wait`].
/// Absent, or empty, one fetch of the remote head is enough. A commit the
/// remote still does not have imports the remote head and says so. A
/// checkout with no remote is left alone, and that is not an error. The
/// returned lines are notes for stderr.
///
/// # Errors
///
/// `commit` is not 7 to 64 hex digits, a scope file does not parse, or
/// the import fails. A fetch that fails is a note, not an error.
pub(crate) fn fetch_for_inbox(commit: Option<&str>) -> Result<String> {
    let commit = commit.map(str::trim).filter(|name| !name.is_empty());
    if let Some(commit) = commit {
        if !hex_commit(commit) {
            bail!("sync: {commit} is not a commit");
        }
    }
    let roots: Vec<PathBuf> = configured_roots()
        .into_iter()
        .filter(|root| has_remote(root))
        .collect();
    let notes = match commit {
        Some(commit) => catch_up_until(&roots, commit, inbox_sync_wait()),
        None => roots.iter().map(|root| catch_up(root)).collect(),
    };
    let mut out = String::new();
    for (root, fetched) in roots.iter().zip(notes) {
        let fetched_ok = !fetched.contains("fetch failed");
        out.push_str(&fetched);
        if let Some(commit) = commit {
            out.push_str(&note_commit(root, commit, fetched_ok));
        }
        if let Some(scope) = scope_of_repo(root)? {
            out.push_str(&import(root, &scope)?);
        }
    }
    Ok(out)
}

fn sync_roots() -> Result<Vec<PathBuf>> {
    let listed = local().repos;
    Ok(if listed.is_empty() {
        vec![tracker_root()?]
    } else {
        listed.iter().map(|r| expand(r)).collect()
    })
}

/// Fold one gone host's atoms into this host's logs and drop that host's
/// log files from each configured tracker repository.
///
/// `force` adopts a log committed within the last 14 days. `dry_run`
/// prints the plan and writes nothing. A fetch or merge that did not
/// bring a repository current adopts nothing in it: the remote log may
/// be newer than the one that would be deleted.
///
/// # Errors
///
/// A missing host name, trying to adopt this machine's own host name, a
/// scope file that does not parse, the pack not answering, or age refusing.
/// A log that is too new, unless `force`. A tracker that is not current.
pub fn adopt(target: &str, force: bool, dry_run: bool) -> Result<String> {
    let gone = require_host_name(target)?;
    if gone == host() {
        bail!("sync: {gone} is this machine; pass a gone host");
    }
    let mut out = String::new();
    for root in sync_roots()? {
        if let Err(e) = crate::tracker_push::prepare_tracker(&root, false, true) {
            out.push_str(&format!("sync: merge setup: {e}\n"));
        }
        let Some(scope) = scope_of_repo(&root)? else {
            out.push_str(&format!(
                "sync: {} names no scope; write .ljos/sync.toml with name and recipients (`ljos sync --key` prints this machine's)\n",
                root.display()
            ));
            continue;
        };
        let caught = catch_up(&root);
        if catch_up_failed(&caught) {
            bail!(
                "sync: {gone} was not adopted from {}; the tracker is not current\n{caught}",
                root.display()
            );
        }
        out.push_str(&caught);
        out.push_str(&adopt_in_repo(
            &root,
            &scope,
            &gone,
            AdoptOpts {
                push: true,
                force,
                dry_run,
            },
        )?);
    }
    Ok(out)
}

fn sync_repos(pull_import: bool, export_log: bool, asked: bool) -> Result<String> {
    let mut out = String::new();
    for root in sync_roots()? {
        out.push_str(&sync_one(&root, pull_import, export_log, asked)?);
    }
    Ok(out)
}

/// What a sync may do with the tracker's git.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackerGit {
    /// Commit and push.
    Push,
    /// Commit, push nothing.
    Commit,
    /// Leave the checkout alone.
    Off,
}

/// `LJOS_TRACKER_GIT` read as the tracker commit reads it: `off`, `0` or
/// `false` is off, `commit` commits without pushing, anything else pushes.
fn tracker_git_value(value: Option<&str>) -> TrackerGit {
    match value {
        Some("off" | "0" | "false") => TrackerGit::Off,
        Some("commit") => TrackerGit::Commit,
        _ => TrackerGit::Push,
    }
}

/// The git a sync may do. A sync in passing, from the sitting, finish or
/// the server's exit, follows `LJOS_TRACKER_GIT` as the ticket commit
/// does. Commit mode neither fetches nor pushes: both carry the branch,
/// and the pull then reads the pack. `ljos sync` run by hand does what it
/// was asked.
fn tracker_git(asked: bool) -> TrackerGit {
    if asked {
        return TrackerGit::Push;
    }
    tracker_git_value(std::env::var("LJOS_TRACKER_GIT").ok().as_deref())
}

fn sync_one(root: &Path, pull_import: bool, export_log: bool, asked: bool) -> Result<String> {
    let root = root.to_path_buf();
    let mut out = String::new();
    let mode = tracker_git(asked);
    if mode == TrackerGit::Off {
        // No merge attributes, no pull, no commit. A repository with no
        // scope stays quiet, as it does with git on.
        return Ok(match scope_of_repo(&root)? {
            Some(_) => "sync: off (LJOS_TRACKER_GIT); `ljos sync` runs it by hand\n".into(),
            None => String::new(),
        });
    }
    // Every sync writes the tracker's merge attributes and registers the
    // atoms driver. A later conflict then unions org files and re-exports
    // this machine's atoms.
    if let Err(e) = crate::tracker_push::prepare_tracker(&root, false, true) {
        out.push_str(&format!("sync: merge setup: {e}\n"));
    }
    let Some(scope) = scope_of_repo(&root)? else {
        if !asked {
            return Ok(String::new());
        }
        out.push_str(&format!(
            "sync: {} names no scope; write .ljos/sync.toml with name and recipients (`ljos sync --key` prints this machine's)\n",
            root.display()
        ));
        return Ok(out);
    };
    if pull_import {
        if mode == TrackerGit::Commit {
            // The sitting's half of the same sync finish pushes. Fetching
            // the tracker is a network pull of the pack log, and the import
            // then reads and posts this seat's pack. A pack that does not
            // answer in time is ureq's "timed out reading response", which
            // the sitting prints as cause 3 of that error. `ljos sync` by
            // hand still fetches and imports.
            out.push_str("sync: not fetched (LJOS_TRACKER_GIT=commit)\n");
        } else {
            out.push_str(&catch_up(&root));
            out.push_str(&import(&root, &scope)?);
        }
    }
    if export_log {
        let client = crate::pack()?;
        let atoms = crate::atoms_lean(&client, &client.workspace())?;
        if scope.scopes.is_empty() {
            let said = export(&root, &scope, &atoms)?;
            out.push_str(&said);
            out.push_str(&commit_if_sealed(
                &root,
                &scope.name,
                &said,
                mode == TrackerGit::Push,
            ));
        } else {
            let said = export_all(&root, &scope, &atoms)?;
            out.push_str(&said);
            out.push_str(&commit_if_sealed_scopes(
                &root,
                &scope,
                &said,
                mode == TrackerGit::Push,
            ));
        }
    }
    Ok(out)
}

/// Whether user-rule sync is off for `value` of `LJOS_USER_RULES_SYNC`.
/// Unset is off in tests, so a hook test does not pull the login's tracker.
/// Anywhere else, unset means on and `off` is the switch.
fn user_rules_sync_off_value(value: Option<&str>) -> bool {
    match value {
        Some("off") => true,
        Some(_) => false,
        None => cfg!(test),
    }
}

fn user_rules_sync_off() -> bool {
    user_rules_sync_off_value(std::env::var("LJOS_USER_RULES_SYNC").ok().as_deref())
}

/// Tracker checkouts that carry user rules. A guessed working directory is
/// not one: that is the tree the seat was started in, not the repository
/// that travels between machines. `LJOS_USER_RULES_SYNC=off` names none.
/// In tests the variable has to be set to something other than `off` before
/// a tracker is read, so an unset shell does not sync the login's rules.
#[must_use]
pub fn user_rule_trackers() -> Vec<PathBuf> {
    if user_rules_sync_off() {
        return Vec::new();
    }
    if let Some(one) = std::env::var("LJOS_USER_RULES_TRACKER")
        .ok()
        .filter(|path| !path.trim().is_empty())
    {
        return vec![expand(one.trim())];
    }
    let listed = local().repos;
    if !listed.is_empty() {
        return listed.iter().map(|path| expand(path)).collect();
    }
    if let Some(root) = std::env::var_os("ISSUE_ROOT")
        .or_else(|| std::env::var_os("VISSUE_ROOT"))
        .filter(|root| !root.is_empty())
    {
        return vec![PathBuf::from(root)];
    }
    if let Some(root) = seat_tracker() {
        return vec![root];
    }
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("vissue.toml").is_file() {
            return vec![cwd];
        }
    }
    Vec::new()
}

fn seat_tracker() -> Option<PathBuf> {
    #[derive(serde::Deserialize)]
    struct Seat {
        root: Option<String>,
    }
    let path = std::env::var_os("VISSUE_CONFIG")
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(config_dir().parent()?.join("vissue").join("config.toml")))?;
    let text = std::fs::read_to_string(path).ok()?;
    let root = toml::from_str::<Seat>(&text).ok()?.root?;
    let path = expand(root.trim());
    path.is_dir().then_some(path)
}

fn machine_name(host: &str) -> String {
    let clean: String = host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let clean = clean.trim_matches('.').to_string();
    if clean.is_empty() {
        "host".into()
    } else {
        clean
    }
}

fn looks_like_git(root: &Path) -> bool {
    let git = root.join(".git");
    git.is_dir() || git.is_file()
}

fn repo_rows(root: &Path) -> Vec<crate::user_rules::UserRule> {
    let dir = root.join(".ljos").join("user-rules");
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    let mut rows = Vec::new();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(parsed) = crate::user_rules::parse_rules(&text) {
            rows.extend(parsed);
        }
    }
    rows
}

/// How long a git past its budget has to clean up after SIGTERM.
const TERM_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

fn git_within(
    root: &Path,
    args: &[&str],
    deadline: std::time::Instant,
) -> Result<std::process::Output> {
    use std::os::unix::process::CommandExt;
    let budget = deadline.saturating_duration_since(std::time::Instant::now());
    if budget.is_zero() {
        bail!(
            "sync: user rules: no time left for git {}",
            args.first().copied().unwrap_or("")
        );
    }
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: the closure runs in the forked child before exec. setpgid
    // makes that child a group leader so a timed-out fetch can be killed
    // with the helpers it started. The child has no other threads.
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
    let child = cmd.spawn().with_context(|| {
        format!(
            "sync: user rules: git {}",
            args.first().copied().unwrap_or("")
        )
    })?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(budget) {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(err)) => Err(err).context("sync: user rules: git"),
        Err(_) => {
            // git removes index.lock when it is terminated and leaves it
            // when it is killed: an add cut off by SIGKILL left an empty
            // index.lock that refused every later commit on the checkout.
            // A group still running after the grace is killed.
            // SAFETY: signals to the child's process group, whose id is the
            // child's pid after setpgid, and to the child itself.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
                libc::kill(pid as i32, libc::SIGTERM);
            }
            if rx.recv_timeout(TERM_GRACE).is_err() {
                // SAFETY: as above.
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
            bail!(
                "sync: user rules: the tracker did not answer within {} ms",
                budget.as_millis()
            )
        }
    }
}

fn git_succeeded(out: &std::process::Output) -> bool {
    out.status.success()
}

fn one_line(out: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&out.stderr);
    let text = if text.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout)
    } else {
        text
    };
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn integrate_remote(root: &Path, deadline: std::time::Instant) -> Result<()> {
    let fetched = git_within(root, &["fetch", "-q", "--all"], deadline)?;
    if !git_succeeded(&fetched) {
        bail!("sync: user rules: fetch failed: {}", one_line(&fetched));
    }
    let Some(up) = crate::tracker_upstream(root) else {
        return Ok(());
    };
    let _held = held_by(root, deadline)?;
    let forward = git_within(root, &["merge", "-q", "--ff-only", &up], deadline)?;
    if git_succeeded(&forward) {
        return Ok(());
    }
    let merged = git_within(root, &["merge", "-q", "--no-edit", &up], deadline)?;
    if git_succeeded(&merged) {
        return Ok(());
    }
    let _ = git_within(
        root,
        &["merge", "--abort"],
        std::time::Instant::now() + std::time::Duration::from_secs(2),
    );
    bail!(
        "sync: user rules: could not merge {up}: {}",
        one_line(&merged)
    )
}

/// The tracker's commit lock, given up at `deadline`. A hook's budget is
/// spent waiting for the lock, not on a git that would be cut off while it
/// holds the index.
fn held_by(root: &Path, deadline: std::time::Instant) -> Result<crate::CommitLock> {
    crate::tracker_commit_lock_by(root, deadline).with_context(|| {
        format!(
            "sync: user rules: another seat held {} past the budget; nothing was merged or committed",
            root.display()
        )
    })
}

/// Commit `rels` alone, a deletion included, under the tracker's commit
/// lock. False when none of them changed.
fn commit_host(
    root: &Path,
    rels: &[String],
    host: &str,
    deadline: std::time::Instant,
) -> Result<bool> {
    crate::refuse_ljos_checkout(root).map_err(|why| anyhow::anyhow!("sync: user rules: {why}"))?;
    let held = held_by(root, deadline)?;
    let paths: Vec<&str> = rels.iter().map(String::as_str).collect();
    let mut add = vec!["add", "-A", "--"];
    add.extend(&paths);
    let added = git_within(root, &add, deadline)?;
    if !git_succeeded(&added) {
        if let Some(orphan) = crate::orphaned_index_lock(root) {
            bail!("sync: user rules: {orphan}");
        }
        bail!(
            "sync: user rules: could not stage {}: {}",
            rels.join(", "),
            one_line(&added)
        );
    }
    let mut diff = vec!["diff", "--cached", "--quiet", "--"];
    diff.extend(&paths);
    let staged = git_within(root, &diff, deadline)?;
    if git_succeeded(&staged) {
        drop(held);
        return Ok(false);
    }
    let message = format!("chore(sync): {host} user rules");
    let mut commit = vec!["commit", "-q", "--only", "-m", message.as_str(), "--"];
    commit.extend(&paths);
    let committed = git_within(root, &commit, deadline)?;
    drop(held);
    if git_succeeded(&committed) {
        Ok(true)
    } else {
        bail!("sync: user rules: commit refused: {}", one_line(&committed))
    }
}

fn push_host(root: &Path, deadline: std::time::Instant) -> Result<()> {
    let upstream = git(root, &["rev-parse", "--abbrev-ref", "@{upstream}"])
        .ok()
        .is_some_and(|out| out.status.success());
    let args: &[&str] = if upstream {
        &["push", "-q"]
    } else {
        &["push", "-q", "-u", "origin", "HEAD"]
    };
    let pushed = git_within(root, args, deadline)?;
    if git_succeeded(&pushed) {
        return Ok(());
    }
    let said = format!(
        "{}\n{}",
        one_line(&pushed),
        String::from_utf8_lossy(&pushed.stderr)
    );
    if !crate::tracker_push::rejected(&said) {
        bail!("sync: user rules: push refused: {}", one_line(&pushed));
    }
    integrate_remote(root, deadline)?;
    let again = git_within(root, args, deadline)?;
    if git_succeeded(&again) {
        Ok(())
    } else {
        bail!("sync: user rules: push refused: {}", one_line(&again))
    }
}

/// This host's sealed user-rule file in a tracker checkout.
fn sealed_rules_rel(name: &str) -> String {
    format!(".ljos/user-rules/{name}.json.age")
}

/// The plaintext file a host wrote before user rules were sealed.
fn plain_rules_rel(name: &str) -> String {
    format!(".ljos/user-rules/{name}.json")
}

/// Rows of every sealed user-rule file this machine can open, and how many
/// it could not: sealed to other machines, or `age` missing. The files are
/// sealed to the tracker's primary scope, `scope_name`, which is empty
/// when the tracker names none.
fn sealed_rows(root: &Path, scope_name: &str) -> (Vec<crate::user_rules::UserRule>, usize) {
    let dir = root.join(".ljos").join("user-rules");
    let Ok(read) = std::fs::read_dir(&dir) else {
        return (Vec::new(), 0);
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".json.age"))
        .collect();
    paths.sort();
    let identities = identities_for_open(scope_name);
    let mut rows = Vec::new();
    let mut closed = 0;
    for path in paths {
        let opened = if identities.is_empty() {
            None
        } else {
            open_identities(&path, &identities).ok().flatten()
        };
        match opened.and_then(|text| crate::user_rules::parse_rules(&text).ok()) {
            Some(parsed) => rows.extend(parsed),
            None => closed += 1,
        }
    }
    (rows, closed)
}

/// Merge `rows` into the local file and return the merged rows. Nothing
/// read and nothing local leaves the file untouched.
fn merge_into_local(
    rows: Vec<crate::user_rules::UserRule>,
) -> Result<Vec<crate::user_rules::UserRule>> {
    let local = crate::user_rules::read_all()?;
    if rows.is_empty() {
        return Ok(local);
    }
    let mut all = rows;
    all.extend(local);
    let merged = crate::user_rules::merge_rows(all);
    crate::user_rules::replace_stored(&merged)?;
    Ok(merged)
}

/// Where this machine remembers what it last sealed for a tracker, so an
/// unchanged set of rules is not sealed and committed again. It stays on
/// this machine: a digest of the rules in the tracker would say something
/// about them.
fn rules_stamp_path(root: &Path, name: &str) -> PathBuf {
    state_dir()
        .join("user-rules")
        .join(crate::work_id(&root.display().to_string()))
        .join(format!("{name}.digest"))
}

/// What sealing this host's rules did.
#[derive(Debug, PartialEq, Eq)]
enum Sealed {
    Wrote,
    Unchanged,
    AgeMissing,
}

fn seal_rules(root: &Path, scope: &Scope, rendered: &str, name: &str) -> Result<Sealed> {
    let out = root.join(sealed_rules_rel(name));
    let stamp = rules_stamp_path(root, name);
    let digest = format!(
        "{} {}\n",
        crate::work_id(rendered),
        crate::work_id(&scope.recipients.join(","))
    );
    if out.is_file() && std::fs::read_to_string(&stamp).is_ok_and(|have| have == digest) {
        return Ok(Sealed::Unchanged);
    }
    if !age_on_path() {
        return Ok(Sealed::AgeMissing);
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Err(err) = seal(rendered, &scope.recipients, &out) {
        if age_missing(&err) {
            return Ok(Sealed::AgeMissing);
        }
        return Err(err);
    }
    if let Some(parent) = stamp.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&stamp, digest)?;
    Ok(Sealed::Wrote)
}

fn tracked(root: &Path, rel: &str) -> bool {
    git(root, &["ls-files", "--error-unmatch", "--", rel]).is_ok_and(|out| out.status.success())
}

/// Share user rules through a tracker checkout. Nothing leaves this
/// machine unless the tracker's `.ljos/sync.toml` names a scope: then the
/// tracker is fetched, every host's rules are merged with the local file by
/// id, and this machine's rules are sealed with age to the scope's
/// recipients, committed as `.ljos/user-rules/<host>.json.age` and pushed.
/// A plaintext `<host>.json` this machine wrote before is removed in the
/// same commit. Plaintext files from other hosts are still read. With no
/// scope, or `LJOS_TRACKER_GIT=off` in passing, the rules already in the
/// checkout are read and the tracker is not touched. `LJOS_TRACKER_GIT=commit`
/// commits and pushes nothing. `asked` is `ljos prefer --user --sync`,
/// which pushes whatever `LJOS_TRACKER_GIT` says. `budget` bounds every
/// git call. A fetch that does not return leaves the local file as it was.
///
/// # Errors
///
/// The checkout is not git, the scope file does not parse, the fetch did
/// not answer, age refused to seal, or the local file cannot be written.
pub fn exchange_user_rules(
    root: &Path,
    host: &str,
    budget: std::time::Duration,
    asked: bool,
) -> Result<String> {
    if !root.is_dir() {
        bail!("sync: user rules: {} is not a directory", root.display());
    }
    if !looks_like_git(root) {
        bail!("sync: user rules: {} is not a git checkout", root.display());
    }
    let mode = tracker_git(asked);
    let scope = scope_of_repo(root)?;
    let sealed_to = scope.as_ref().map(|s| s.name.clone()).unwrap_or_default();
    let Some(scope) = scope.filter(|_| mode != TrackerGit::Off) else {
        let (mut rows, _) = sealed_rows(root, &sealed_to);
        rows.extend(repo_rows(root));
        let merged = merge_into_local(rows)?;
        if !asked {
            return Ok(String::new());
        }
        let live = merged.iter().filter(|rule| !rule.deleted).count();
        return Ok(format!(
            "sync: user rules: {} names no scope, so they stay on this machine; write .ljos/sync.toml with name and recipients (`ljos sync --key` prints this machine's)\n\
             sync: user rules: {live} on this machine\n",
            root.display()
        ));
    };
    let deadline = std::time::Instant::now() + budget;
    let remote = git(root, &["remote"])
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty());
    if remote {
        integrate_remote(root, deadline)?;
    }
    let (mut rows, closed) = sealed_rows(root, &scope.name);
    rows.extend(repo_rows(root));
    let merged = merge_into_local(rows)?;
    let mut out = String::new();
    if remote {
        out.push_str("sync: user rules: fetched\n");
    }
    if closed > 0 {
        out.push_str(&format!(
            "sync: user rules: {closed} sealed file{} not opened on this machine\n",
            if closed == 1 { "" } else { "s" }
        ));
    }
    let live = merged.iter().filter(|rule| !rule.deleted).count();
    if merged.is_empty() && !root.join(plain_rules_rel(&machine_name(host))).exists() {
        out.push_str("sync: user rules: 0 on this machine\n");
        return Ok(out);
    }
    let name = machine_name(host);
    match seal_rules(
        root,
        &scope,
        &crate::user_rules::render_rules(&merged)?,
        &name,
    )? {
        Sealed::AgeMissing => {
            out.push_str("sync: user rules: age is not installed, so they were not sealed or sent; install age to sync them\n");
            out.push_str(&format!("sync: user rules: {live} on this machine\n"));
            return Ok(out);
        }
        Sealed::Wrote | Sealed::Unchanged => {}
    }
    let mut rels = vec![sealed_rules_rel(&name)];
    let plain = plain_rules_rel(&name);
    if root.join(&plain).exists() {
        std::fs::remove_file(root.join(&plain))?;
    }
    if tracked(root, &plain) {
        rels.push(plain);
    }
    let committed = commit_host(root, &rels, &name, deadline)?;
    match mode {
        TrackerGit::Push if remote && (committed || commits_to_push(root)) => {
            push_host(root, deadline)?;
            out.push_str("sync: user rules: sealed and pushed\n");
        }
        TrackerGit::Commit if committed => {
            out.push_str(
                "sync: user rules: sealed and committed; not pushed (LJOS_TRACKER_GIT=commit)\n",
            );
        }
        _ => {}
    }
    out.push_str(&format!("sync: user rules: {live} on this machine\n"));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packsetd_bin() -> PathBuf {
        if let Some(path) = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("packsetd"))
                .find(|candidate| candidate.is_file())
        }) {
            return path;
        }
        if let Some(home) = std::env::var_os("HOME") {
            let path = PathBuf::from(home).join(".local/bin/packsetd");
            if path.is_file() {
                return path;
            }
        }
        panic!("packsetd is not on PATH; CI builds it from packset develop");
    }

    fn lesson_line(text: &str, scope_name: &str) -> String {
        format!(
            "{}\n",
            serde_json::json!({
                "kind": "lesson",
                "level": "explicit",
                "text": text,
                "entities": [format!("scope:{scope_name}")],
            })
        )
    }

    fn seal_for(path: &Path, public: &str, plain: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut child = Command::new("age")
            .args(["-r", public, "-o"])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(plain.as_bytes())
            .unwrap();
        let done = child.wait_with_output().unwrap();
        assert!(
            done.status.success(),
            "age: {}",
            String::from_utf8_lossy(&done.stderr)
        );
    }

    fn age_keygen(path: &Path) -> String {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(path)
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "age-keygen must be installed: {}",
            String::from_utf8_lossy(&made.stderr)
        );
        let public = Command::new("age-keygen")
            .arg("-y")
            .arg(path)
            .output()
            .unwrap();
        assert!(public.status.success());
        String::from_utf8_lossy(&public.stdout).trim().to_string()
    }

    struct PackDaemon(std::process::Child);
    impl Drop for PackDaemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Caller holds [`crate::test_env_lock`] and sets the pack environment.
    fn spawn_pack(pack_home: &Path) -> (u16, PackDaemon) {
        std::fs::create_dir_all(pack_home).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut daemon = Command::new(packsetd_bin())
            .args(["--port", &port.to_string(), "--home"])
            .arg(pack_home)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", pack_home)
            .env("PACKSET_HOME", pack_home)
            .env("PACKSET_AUTH", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(daemon.try_wait().unwrap().is_none(), "packsetd exited");
            assert!(
                std::time::Instant::now() < deadline,
                "packsetd did not listen"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        (port, PackDaemon(daemon))
    }

    /// `age` that writes its stdin to `-o`. The caller holds [`crate::test_env_lock`].
    fn with_fake_age<T>(dir: &Path, body: impl FnOnce() -> T) -> T {
        use std::os::unix::fs::PermissionsExt as _;
        let fake_age = dir.join("age");
        std::fs::write(
            &fake_age,
            "#!/bin/sh\nout=''\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    -o) out=\"$2\"; shift 2 ;;\n    -r) shift 2 ;;\n    *) shift ;;\n  esac\ndone\ncat > \"$out\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_age, std::fs::Permissions::from_mode(0o755)).unwrap();
        struct PathGuard(Option<std::ffi::OsString>);
        impl Drop for PathGuard {
            fn drop(&mut self) {
                // SAFETY: the caller holds the process env lock.
                unsafe {
                    if let Some(value) = &self.0 {
                        std::env::set_var("PATH", value);
                    } else {
                        std::env::remove_var("PATH");
                    }
                }
            }
        }
        let before = std::env::var_os("PATH");
        let mut entries = vec![dir.to_path_buf()];
        if let Some(old) = &before {
            entries.extend(std::env::split_paths(old));
        }
        let joined = std::env::join_paths(entries).unwrap();
        // SAFETY: the caller holds the process env lock.
        unsafe { std::env::set_var("PATH", &joined) };
        let _guard = PathGuard(before);
        body()
    }

    fn atom(id: &str, text: &str, entities: &[&str]) -> Value {
        serde_json::json!({"id": id, "text": text, "kind": "lesson",
            "entities": entities, "embedding": [0.1, 0.2]})
    }

    fn hooked_repo(root: &Path, pre_commit: &str) -> String {
        use std::os::unix::fs::PermissionsExt as _;
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "seat@example.invalid"]);
        git(&["config", "user.name", "seat"]);
        std::fs::write(root.join("seed"), "seed\n").unwrap();
        git(&["add", "seed"]);
        git(&["commit", "-q", "-m", "seed"]);
        let hooks = root.join(".git/test-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, pre_commit).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(&["config", "core.hooksPath", &hooks.display().to_string()]);
        let rel = sealed_rules_rel("probe-host");
        std::fs::create_dir_all(root.join(".ljos/user-rules")).unwrap();
        std::fs::write(root.join(&rel), "sealed\n").unwrap();
        rel
    }

    /// A commit cut off by the hook's budget leaves no index.lock: git is
    /// terminated, and cleans up, before anything kills it.
    #[test]
    fn a_commit_past_its_budget_leaves_no_index_lock() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = hooked_repo(root, "#!/bin/sh\nsleep 30\n");
        let started = std::time::Instant::now();
        let deadline = started + std::time::Duration::from_millis(1500);
        let cut = commit_host(root, &[rel], "probe-host", deadline).unwrap_err();
        assert!(format!("{cut:#}").contains("did not answer"), "{cut:#}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        assert!(
            !root.join(".git/index.lock").exists(),
            "the cut-off commit left index.lock behind"
        );
    }

    /// A seat holding the checkout past the budget stops the commit before
    /// any git starts, instead of leaving git no time to finish.
    #[test]
    fn a_held_checkout_stops_the_commit_at_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = hooked_repo(root, "#!/bin/sh\nexit 0\n");
        let held = crate::tracker_commit_lock(root);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        let waited =
            commit_host(root, std::slice::from_ref(&rel), "probe-host", deadline).unwrap_err();
        assert!(
            format!("{waited:#}").contains("past the budget"),
            "{waited:#}"
        );
        drop(held);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        assert!(commit_host(root, &[rel], "probe-host", deadline).unwrap());
    }

    #[test]
    fn an_unset_user_rules_sync_is_off_in_tests() {
        assert!(user_rules_sync_off_value(None));
        assert!(user_rules_sync_off_value(Some("off")));
        assert!(!user_rules_sync_off_value(Some("on")));
    }

    #[test]
    fn a_repository_with_no_scope_is_named_only_when_sync_is_asked_for() {
        let _scratch = tempfile::tempdir().unwrap();
        let root = _scratch.path().to_path_buf();
        let asked = sync_one(&root, true, false, true).unwrap();
        assert!(asked.contains("names no scope"), "{asked}");
        assert_eq!(sync_one(&root, true, false, false).unwrap(), "");
        assert_eq!(sync_one(&root, false, true, false).unwrap(), "");
        let _ = std::fs::remove_dir(&root);
    }

    /// A tracker with no remote used to say the log was pushed to nothing.
    #[test]
    fn a_log_with_no_remote_says_how_to_set_one() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "seat@example.invalid"]);
        git(&["config", "user.name", "seat"]);
        git(&["config", "core.hooksPath", "/dev/null"]);
        let atoms = root.join(".ljos/atoms");
        std::fs::create_dir_all(&atoms).unwrap();
        let name = host();
        std::fs::write(atoms.join(format!("{name}.jsonl.age")), "sealed\n").unwrap();
        std::fs::write(atoms.join(format!("{name}.digest")), "digest\n").unwrap();
        let said = commit_log(root, "surf", true);
        assert!(said.contains("no remote is configured"), "{said}");
        assert!(said.contains("remote add origin"), "{said}");
        assert!(said.contains("git push -u origin HEAD"), "{said}");
        assert!(!said.contains("pushed to nothing"), "{said}");
        assert_eq!(commit_log(root, "surf", true), "");
    }

    /// Atoms that did not change still push a commit already waiting,
    /// such as the tracker's merge attributes.
    #[test]
    fn unchanged_atoms_still_push_a_pending_commit() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let root = dir.path().join("local");
        let git = |cwd: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare"]);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "seat@example.invalid"]);
        git(&root, &["config", "user.name", "seat"]);
        git(&root, &["config", "core.hooksPath", "/dev/null"]);
        let atoms = root.join(".ljos/atoms");
        std::fs::create_dir_all(&atoms).unwrap();
        let name = host();
        std::fs::write(atoms.join(format!("{name}.jsonl.age")), "sealed\n").unwrap();
        std::fs::write(atoms.join(format!("{name}.digest")), "digest\n").unwrap();
        std::fs::write(root.join("README"), "seed\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "seed"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&root, &["push", "-q", "-u", "origin", "HEAD"]);
        std::fs::write(root.join(".gitattributes"), "*.org merge=union\n").unwrap();
        git(&root, &["add", ".gitattributes"]);
        git(
            &root,
            &[
                "commit",
                "-q",
                "-m",
                "chore(tracker): union org files and re-export atoms on merge",
            ],
        );
        let said = commit_log(&root, "surf", true);
        assert!(
            said.contains("pushed pending commits"),
            "an unchanged atom log must still push: {said}"
        );
        let remote_log = std::process::Command::new("git")
            .arg("-C")
            .arg(&remote)
            .args(["log", "--format=%s"])
            .output()
            .unwrap();
        let subjects = String::from_utf8_lossy(&remote_log.stdout);
        assert!(
            subjects.contains("union org files"),
            "the pending commit was not pushed: {subjects}"
        );
        assert_eq!(commit_log(&root, "surf", true), "");
    }

    #[test]
    fn tracker_git_reads_as_the_ticket_commit_reads_it() {
        assert_eq!(tracker_git_value(None), TrackerGit::Push);
        assert_eq!(tracker_git_value(Some("")), TrackerGit::Push);
        assert_eq!(tracker_git_value(Some("commit")), TrackerGit::Commit);
        for off in ["off", "0", "false"] {
            assert_eq!(tracker_git_value(Some(off)), TrackerGit::Off, "{off}");
        }
        assert_eq!(tracker_git(true), TrackerGit::Push);
    }

    /// `LJOS_TRACKER_GIT=commit` kept the ticket commit local, and the sync
    /// at finish pushed it anyway with the log.
    #[test]
    fn a_commit_only_tracker_commits_the_log_and_pushes_nothing() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let root = dir.path().join("local");
        let git = |cwd: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare"]);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "seat@example.invalid"]);
        git(&root, &["config", "user.name", "seat"]);
        git(&root, &["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(root.join("README"), "seed\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "seed"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&root, &["push", "-q", "-u", "origin", "HEAD"]);
        std::fs::write(root.join("issues.org"), "* TODO kept local\n").unwrap();
        git(&root, &["add", "issues.org"]);
        git(&root, &["commit", "-q", "-m", "chore(issues): kept local"]);
        let atoms = root.join(".ljos/atoms");
        std::fs::create_dir_all(&atoms).unwrap();
        let name = host();
        std::fs::write(atoms.join(format!("{name}.jsonl.age")), "sealed\n").unwrap();
        std::fs::write(atoms.join(format!("{name}.digest")), "digest\n").unwrap();
        let said = commit_log(&root, "surf", false);
        assert!(
            said.contains("not pushed (LJOS_TRACKER_GIT=commit)"),
            "{said}"
        );
        assert!(git(&root, &["log", "-1", "--format=%s"]).contains("chore(sync)"));
        assert_eq!(
            commit_log(&root, "surf", false),
            "sync: not pushed (LJOS_TRACKER_GIT=commit)\n"
        );
        let subjects = git(&remote, &["log", "--format=%s"]);
        assert!(!subjects.contains("kept local"), "{subjects}");
        assert!(!subjects.contains("chore(sync)"), "{subjects}");
    }

    /// Sitting's pack sync fetches the tracker and then reads the pack.
    /// Commit-only mode does neither: a fetch is the pull of the same log
    /// finish would push, and the import is the read that times out.
    #[test]
    fn a_commit_only_sitting_does_not_fetch_or_read_the_pack() {
        let _g = crate::test_env_lock();
        let before = std::env::var_os("LJOS_TRACKER_GIT");
        let config_before = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("LJOS_TRACKER_GIT", "commit");
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        std::fs::write(config.join("ljos/age.key"), "not a key\n").unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &config);
        let remote = dir.path().join("remote.git");
        let root = dir.path().join("local");
        let other = dir.path().join("other");
        let git = |cwd: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.email", "seat@example.invalid"]);
        git(&root, &["config", "user.name", "seat"]);
        git(&root, &["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(root.join("README"), "seed\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "seed"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&root, &["push", "-q", "-u", "origin", "HEAD"]);
        std::fs::create_dir_all(&other).unwrap();
        git(&other, &["clone", "-q", remote.to_str().unwrap(), "."]);
        git(&other, &["config", "user.email", "seat@example.invalid"]);
        git(&other, &["config", "user.name", "seat"]);
        git(&other, &["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(other.join("README"), "theirs\n").unwrap();
        git(&other, &["add", "README"]);
        git(&other, &["commit", "-q", "-m", "from the other machine"]);
        git(&other, &["push", "-q", "origin", "HEAD"]);
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            "name = \"surf\"\nrecipients = [\"age1test\"]\n",
        )
        .unwrap();
        std::fs::write(root.join(".ljos/atoms/otherhost.jsonl.age"), "sealed\n").unwrap();
        let said = sync_one(&root, true, false, false).unwrap();
        let local = git(&root, &["log", "--format=%s"]);
        match before {
            Some(value) => std::env::set_var("LJOS_TRACKER_GIT", value),
            None => std::env::remove_var("LJOS_TRACKER_GIT"),
        }
        match config_before {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        assert!(
            said.contains("not fetched (LJOS_TRACKER_GIT=commit)"),
            "{said}"
        );
        assert!(
            !said.contains("timed out"),
            "commit mode still read the pack: {said}"
        );
        assert!(
            !local.contains("from the other machine"),
            "sitting fetched the pack log: {local}"
        );
    }

    /// `LJOS_TRACKER_GIT=off` leaves the tracker as it is in passing: no
    /// attributes, no commit, one line when a scope is set up.
    #[test]
    fn an_off_tracker_is_left_alone_in_passing() {
        let _g = crate::test_env_lock();
        let before = std::env::var_os("LJOS_TRACKER_GIT");
        std::env::set_var("LJOS_TRACKER_GIT", "off");
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let init = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        assert!(init.status.success());
        let quiet = sync_one(root, true, true, false).unwrap();
        std::fs::create_dir_all(root.join(".ljos")).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            "name = \"surf\"\nrecipients = []\n",
        )
        .unwrap();
        let said = sync_one(root, true, true, false).unwrap();
        match before {
            Some(value) => std::env::set_var("LJOS_TRACKER_GIT", value),
            None => std::env::remove_var("LJOS_TRACKER_GIT"),
        }
        assert_eq!(quiet, "");
        assert!(said.contains("sync: off (LJOS_TRACKER_GIT)"), "{said}");
        assert!(!root.join(".gitattributes").exists());
        assert!(!root.join(".ljos/atoms").exists());
    }

    /// Finish's sync used to print an os error when `age` was not installed.
    #[test]
    fn a_missing_age_skips_the_seal_without_an_os_error() {
        let _hide = hide_age();
        let root = tempfile::tempdir().unwrap();
        let scope: Scope =
            toml::from_str("name = \"surf\"\nrecipients = [\"age1test\"]\n").unwrap();
        let said = export(root.path(), &scope, &[atom("a", "kept", &[])]).unwrap();
        assert!(said.contains("age is not installed"), "{said}");
        assert!(said.contains("install age"), "{said}");
        assert!(!said.contains("os error"), "{said}");
        assert!(!said.to_lowercase().contains("not found"), "{said}");
        assert!(!log_dir(root.path())
            .join(format!("{}.jsonl.age", host()))
            .is_file());
        assert_eq!(commit_if_sealed(root.path(), "surf", &said, true), "");
    }

    #[test]
    fn a_log_carries_own_atoms_of_its_scope_without_vectors() {
        let atoms = vec![
            atom("b", "second", &["seat:x"]),
            atom("a", "first", &[]),
            atom("c", "personal", &["scope:personal"]),
            atom("d", "imported", &["sync:otherhost"]),
        ];
        let plain = log_plaintext(&atoms, "surf", "surf");
        let lines: Vec<Value> = plain
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{plain}");
        assert_eq!(lines[0]["id"], "a");
        assert_eq!(lines[1]["id"], "b");
        assert!(lines.iter().all(|a| a.get("embedding").is_none()));
        assert_eq!(log_plaintext(&atoms, "personal", "surf").lines().count(), 1);
    }

    #[test]
    fn a_project_named_in_the_scope_file_takes_its_own_scope() {
        let scope: Scope = toml::from_str(
            "name = \"personal\"\nrecipients = [\"age1x\"]\n[projects]\ntools = \"shared\"\n",
        )
        .unwrap();
        assert_eq!(scope.for_project("tools"), "shared");
        assert_eq!(scope.for_project("garden"), "personal");
        let bare: Scope = toml::from_str("name = \"shared\"\nrecipients = []\n").unwrap();
        assert!(bare.projects.is_empty());
    }

    #[test]
    fn a_scope_file_may_name_additional_scopes() {
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let grokbot: Vec<&str> = scope
            .recipients_for("grokbot")
            .unwrap()
            .iter()
            .map(String::as_str)
            .collect();
        let cloud: Vec<&str> = scope
            .recipients_for("cloud")
            .unwrap()
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(grokbot, vec!["age1box"]);
        assert_eq!(cloud, vec!["age1cloud"]);
        assert!(scope.recipients_for("missing").is_none());
        let names: Vec<&str> = scope
            .all_scopes()
            .into_iter()
            .map(|named| named.name)
            .collect();
        assert_eq!(names, vec!["grokbot", "cloud"]);
    }

    #[test]
    fn a_log_file_names_its_host_and_its_scope() {
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let primary = PathBuf::from(".ljos/atoms/laptop.jsonl.age");
        assert_eq!(
            host_scope_from_log_path(&primary, &scope).unwrap(),
            ("laptop".into(), "grokbot".into())
        );
        let extra = PathBuf::from(".ljos/atoms/laptop+cloud.jsonl.age");
        assert_eq!(
            host_scope_from_log_path(&extra, &scope).unwrap(),
            ("laptop".into(), "cloud".into())
        );
        // A scope that left the config still names its log. Import retires
        // what was taken from it and does not open the file.
        assert_eq!(
            host_scope_from_log_path(
                &PathBuf::from(".ljos/atoms/laptop+missing.jsonl.age"),
                &scope
            )
            .unwrap(),
            ("laptop".into(), "missing".into())
        );
        assert!(
            host_scope_from_log_path(&PathBuf::from(".ljos/atoms/+cloud.jsonl.age"), &scope)
                .is_none()
        );
        assert!(host_scope_from_log_path(
            &PathBuf::from(".ljos/atoms/laptop+cloud.digest"),
            &scope
        )
        .is_none());
    }

    #[test]
    fn a_grokbot_atom_never_lands_in_the_cloud_file() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        let fake_age = dir.path().join("age");
        std::fs::write(
            &fake_age,
            "#!/bin/sh\nout=''\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    -o) out=\"$2\"; shift 2 ;;\n    -r) shift 2 ;;\n    *) shift ;;\n  esac\ndone\ncat > \"$out\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_age, std::fs::Permissions::from_mode(0o755)).unwrap();
        struct PathGuard(Option<std::ffi::OsString>);
        impl Drop for PathGuard {
            fn drop(&mut self) {
                // SAFETY: this test holds the process env lock.
                unsafe {
                    if let Some(value) = &self.0 {
                        std::env::set_var("PATH", value);
                    } else {
                        std::env::remove_var("PATH");
                    }
                }
            }
        }
        let before = std::env::var_os("PATH");
        let mut entries = vec![dir.path().to_path_buf()];
        if let Some(old) = &before {
            entries.extend(std::env::split_paths(old));
        }
        let joined = std::env::join_paths(entries).unwrap();
        // SAFETY: this test holds the process env lock.
        unsafe { std::env::set_var("PATH", &joined) };
        let _guard = PathGuard(before);
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let atoms = vec![
            atom("a", "box only", &["scope:grokbot"]),
            atom("b", "cloud only", &["scope:cloud"]),
        ];
        export_for_scope(dir.path(), &scope, "grokbot", &atoms, "grokbot").unwrap();
        export_for_scope(dir.path(), &scope, "cloud", &atoms, "grokbot").unwrap();
        let host = host();
        let [cloud_rel, _] = host_log_rels_for_scope(&host, &scope.name, "cloud");
        let cloud_text = std::fs::read_to_string(dir.path().join(cloud_rel)).unwrap();
        assert!(
            cloud_text.contains("cloud only"),
            "cloud file lost the cloud atom: {cloud_text}"
        );
        assert!(
            !cloud_text.contains("box only"),
            "cloud file leaked a grokbot atom: {cloud_text}"
        );
        let [grok_rel, _] = host_log_rels_for_scope(&host, &scope.name, "grokbot");
        let grok_text = std::fs::read_to_string(dir.path().join(grok_rel)).unwrap();
        assert!(grok_text.contains("box only"), "{grok_text}");
        assert!(!grok_text.contains("cloud only"), "{grok_text}");
    }

    #[test]
    fn an_import_tags_the_sender_and_names_what_it_retired() {
        let plain = format!(
            "{}\n{}\n",
            atom("a", "kept", &["seat:x"]),
            atom("b", "new", &[])
        );
        let before: BTreeSet<String> = [crate::work_id("kept"), crate::work_id("dropped")].into();
        let got = incoming(&plain, "rglat", &before, &BTreeSet::new());
        assert_eq!(got.post.len(), 2);
        let again = incoming(&plain, "rglat", &before, &[crate::work_id("kept")].into());
        assert_eq!(
            again.post.len(),
            1,
            "a text already held is not posted again"
        );
        assert_eq!(again.post[0]["text"], "new");
        assert!(got.post.iter().all(|a| a.get("id").is_none()));
        assert!(got.post.iter().all(|a| a["entities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "sync:rglat")));
        assert!(got.post.iter().all(|a| a["origin"] == "peer"));
        assert_eq!(got.retired, vec![crate::work_id("dropped")]);
        let claimed = "{\"id\":\"c\",\"text\":\"claimed\",\"kind\":\"lesson\",\"origin\":\"user-declared\",\"entities\":[]}\n";
        let overwrote = incoming(claimed, "rglat", &BTreeSet::new(), &BTreeSet::new());
        assert_eq!(overwrote.post[0]["origin"], "peer");
    }

    #[test]
    fn an_adopt_plan_keeps_each_issue_in_the_logs_scope() {
        let plain = format!(
            "{}\n{}\n{}\n{}\nnot-json\n",
            serde_json::json!({
                "id": "a", "text": "fresh", "kind": "lesson",
                "origin": "user-declared", "level": "explicit",
                "entities": ["seat:x", "sync:gone"]
            }),
            serde_json::json!({
                "id": "b", "text": "mine", "kind": "lesson",
                "entities": ["scope:cloud"]
            }),
            serde_json::json!({
                "id": "c", "text": "fresh", "kind": "lesson", "issue": "ljos-b",
                "entities": ["sync:stale", "issue:ljos-b"]
            }),
            serde_json::json!({
                "id": "d", "text": "fresh", "kind": "lesson",
                "entities": ["sync:gone"]
            })
        );
        let held = [
            serde_json::json!({
                "id": "o", "text": "mine", "kind": "lesson",
                "entities": ["scope:cloud"]
            }),
            serde_json::json!({
                "id": "p", "text": "fresh", "kind": "lesson",
                "entities": ["scope:private"]
            }),
        ];
        let own = own_memory_keys(&held, "cloud");
        assert_eq!(own.len(), 1, "a private copy is not this scope's own");
        let plan = adopt_plan(&plain, "cloud", &own);
        assert_eq!(
            plan.post.len(),
            2,
            "same words on two tickets are two posts"
        );
        assert!(plan.post.iter().all(|atom| atom["origin"] == "peer"));
        assert!(plan.post.iter().all(|atom| atom.get("id").is_none()));
        assert!(plan.post.iter().all(|atom| atom.get("level").is_none()));
        let issues: BTreeSet<&str> = plan
            .post
            .iter()
            .map(|atom| atom["issue"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(issues, BTreeSet::from(["", "ljos-b"]));
        for atom in &plan.post {
            let tags: Vec<&str> = atom["entities"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect();
            assert!(tags.contains(&"scope:cloud"), "{tags:?}");
            assert!(tags.iter().all(|tag| !tag.starts_with("sync:")), "{tags:?}");
        }
        assert!(plan.live.contains(&memory_key(&held[0]).unwrap()));
    }

    #[test]
    fn a_host_name_cannot_leave_the_atoms_directory() {
        let err = require_host_name("../x").unwrap_err().to_string();
        assert!(err.contains("not a host name"), "{err}");
        assert!(require_host_name("").is_err());
        assert_eq!(require_host_name("gone-host").unwrap(), "gone-host");
        let rels = host_log_rels("../x");
        assert!(
            rels.iter()
                .all(|rel| rel.starts_with(".ljos/atoms/") && !rel.contains("..")),
            "{rels:?}"
        );
    }

    #[test]
    fn an_untagged_sync_copy_belongs_to_the_log_being_adopted() {
        let cloud = serde_json::json!({"text": "a", "entities": ["sync:gone", "scope:cloud"]});
        let bare = serde_json::json!({"text": "a", "entities": ["sync:gone"]});
        let private = serde_json::json!({"text": "a", "entities": ["sync:gone", "scope:private"]});
        assert!(in_adopted_scope(&cloud, "cloud"));
        assert!(in_adopted_scope(&bare, "cloud"));
        assert!(!in_adopted_scope(&private, "cloud"));
    }

    #[test]
    fn a_host_is_quiet_only_after_a_fortnight() {
        let now = 1_700_000_000_i64;
        let quiet = i64::try_from(ADOPT_QUIET.as_secs()).unwrap();
        assert!(host_is_quiet(now - quiet, now));
        assert!(!host_is_quiet(now - quiet + 1, now));
        assert!(host_is_quiet(now - quiet - 86_400, now));
        assert!(!host_is_quiet(now + 10, now));
        assert_eq!(age_phrase(now - 3 * 86_400, now), "3 days ago");
        assert_eq!(age_phrase(now - 90, now), "0 hours ago");
        assert!(!catch_up_failed(""));
        assert!(!catch_up_failed("sync: committed\n"));
        assert!(catch_up_failed(
            "sync: fetch failed; reading the logs as they are\n"
        ));
        assert!(catch_up_failed(
            "sync: could not merge origin/main; `ljos doctor` names the split\n"
        ));
    }

    #[test]
    fn an_adopt_commit_is_the_newest_deletion_of_that_host() {
        let log = "\
\u{1e}chore(sync): adopt gone cloud atoms
.ljos/atoms/gone.digest
.ljos/atoms/gone.jsonl.age
\u{1e}chore(sync): otherhost cloud atoms
.ljos/atoms/stay.jsonl.age
\u{1e}chore: drop back
.ljos/atoms/back.jsonl.age
\u{1e}chore(sync): adopt back cloud atoms
.ljos/atoms/back.jsonl.age
";
        assert_eq!(
            hosts_removed_by_adopt(log),
            BTreeSet::from(["gone".to_string()])
        );
        assert!(adopt_commit_subject(
            "chore(sync): adopt gone cloud atoms",
            "gone"
        ));
        assert!(!adopt_commit_subject(
            "chore(sync): adopt gone cloud atoms",
            "other"
        ));
        assert!(!adopt_commit_subject(
            "chore(sync): gone cloud atoms",
            "gone"
        ));
        assert!(!adopt_commit_subject("chore(sync): adopt gone", "gone"));
        let scoped = "\
\u{1e}chore(sync): adopt gone box atoms
.ljos/atoms/gone+cloud.jsonl.age
.ljos/atoms/gone.jsonl.age
\u{1e}chore: drop extra
.ljos/atoms/stay+cloud.jsonl.age
";
        assert_eq!(
            hosts_removed_by_adopt(scoped),
            BTreeSet::from(["gone".to_string(), "gone+cloud".to_string()])
        );
        let replaced = "\
\u{1e}chore: drop the extra by hand
.ljos/atoms/gone+cloud.jsonl.age
\u{1e}chore(sync): adopt gone box atoms
.ljos/atoms/gone+cloud.jsonl.age
.ljos/atoms/gone.jsonl.age
";
        assert_eq!(
            hosts_removed_by_adopt(replaced),
            BTreeSet::from(["gone".to_string()])
        );
    }

    fn hold_env(dir: &Path) -> crate::EnvLock {
        let lock = crate::test_env_lock();
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        std::env::set_var("XDG_STATE_HOME", dir.join("state"));
        std::fs::create_dir_all(dir.join("config/ljos")).unwrap();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        lock
    }

    fn tracker(dir: &Path) -> PathBuf {
        let root = dir.join("tracker");
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        let run = |args: &[&str]| {
            let out = git(&root, args).unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "seat@example.invalid"]);
        run(&["config", "user.name", "seat"]);
        run(&["config", "core.hooksPath", "/dev/null"]);
        run(&["config", "commit.gpgsign", "false"]);
        root
    }

    fn commit_rel(root: &Path, rel: &str, body: &str, message: &str) {
        if let Some(parent) = root.join(rel).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(root.join(rel), body).unwrap();
        let out = git(root, &["add", "--", rel]).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = git(root, &["commit", "-q", "-m", message, "--", rel]).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn cloud() -> Scope {
        Scope {
            name: "cloud".into(),
            recipients: vec!["age1test".into()],
            projects: BTreeMap::new(),
            scopes: BTreeMap::new(),
        }
    }

    /// A log that is dirty, untracked, or still young is not deleted, and
    /// the pack is not asked. Force does not override a dirty file.
    #[test]
    fn a_log_that_is_not_a_quiet_commit_stays_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let _env = hold_env(dir.path());
        let root = tracker(dir.path());
        let rel = ".ljos/atoms/gone.jsonl.age";
        let opts = AdoptOpts {
            push: false,
            force: true,
            dry_run: true,
        };
        std::fs::write(root.join(rel), "not committed\n").unwrap();
        let err = adopt_in_repo(&root, &cloud(), "gone", opts).unwrap_err();
        assert!(err.to_string().contains("not committed"), "{err}");
        assert!(root.join(rel).is_file(), "an untracked log was deleted");

        commit_rel(&root, rel, "sealed\n", "log");
        std::fs::write(root.join(rel), "dirty\n").unwrap();
        let err = adopt_in_repo(&root, &cloud(), "gone", opts).unwrap_err();
        assert!(err.to_string().contains("differs"), "{err}");
        assert_eq!(
            std::fs::read_to_string(root.join(rel)).unwrap(),
            "dirty\n",
            "a dirty log was deleted"
        );
        let held = crate::tracker_commit_lock(&root);
        let removed = stage_deletion(&root, rel).unwrap_err();
        drop(held);
        assert!(removed.to_string().contains("was not removed"), "{removed}");
        assert_eq!(std::fs::read_to_string(root.join(rel)).unwrap(), "dirty\n");

        let out = git(&root, &["checkout", "-q", "--", rel]).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let err = adopt_in_repo(
            &root,
            &cloud(),
            "gone",
            AdoptOpts {
                push: false,
                force: false,
                dry_run: true,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        assert_eq!(std::fs::read_to_string(root.join(rel)).unwrap(), "sealed\n");
        let said = adopt_in_repo(&root, &cloud(), "gone", opts).unwrap();
        assert!(said.contains("no age identity"), "{said}");
        assert_eq!(std::fs::read_to_string(root.join(rel)).unwrap(), "sealed\n");
        assert!(in_head(&root, rel));
    }

    /// The deletion is a commit of the named paths. A crash that left the
    /// file missing, or a `git rm` that had not been committed, still
    /// records it. `.taken` goes only once that commit is in HEAD, and
    /// only when the newest deletion is an adopt.
    #[test]
    fn the_gone_log_is_committed_away_and_taken_waits_for_that() {
        let dir = tempfile::tempdir().unwrap();
        let _env = hold_env(dir.path());
        let root = tracker(dir.path());
        let age = ".ljos/atoms/gone.jsonl.age";
        let digest = ".ljos/atoms/gone.digest";
        commit_rel(&root, age, "sealed\n", "log");
        commit_rel(&root, digest, "digest\n", "digest");
        std::fs::write(root.join("other.txt"), "keep\n").unwrap();
        let out = git(&root, &["add", "other.txt"]).unwrap();
        assert!(out.status.success());
        let out = git(&root, &["commit", "-q", "-m", "other"]).unwrap();
        assert!(out.status.success());
        std::fs::write(root.join("other.txt"), "staged\n").unwrap();
        let out = git(&root, &["add", "other.txt"]).unwrap();
        assert!(out.status.success());

        std::fs::remove_file(root.join(age)).unwrap();
        let held = crate::tracker_commit_lock(&root);
        stage_deletion(&root, age).unwrap();
        stage_deletion(&root, digest).unwrap();
        let made =
            crate::commit_paths(&root, &[age, digest], "chore(sync): adopt gone cloud atoms");
        drop(held);
        assert_eq!(made, crate::PathCommit::Made);
        assert!(!in_head(&root, age));
        assert!(!in_head(&root, digest));
        assert!(!root.join(age).exists());
        let status = git(&root, &["status", "--porcelain"]).unwrap();
        let status = String::from_utf8_lossy(&status.stdout);
        assert!(
            status.contains("other.txt"),
            "the unrelated stage was committed or lost: {status}"
        );
        assert!(!status.contains("gone"), "{status}");

        let taken = taken_path(&root, "cloud", "gone");
        std::fs::create_dir_all(taken.parent().unwrap()).unwrap();
        std::fs::write(&taken, "abc\n").unwrap();
        let said = adopt_in_repo(
            &root,
            &cloud(),
            "gone",
            AdoptOpts {
                push: false,
                force: true,
                dry_run: false,
            },
        )
        .unwrap();
        assert!(said.contains("has no cloud log"), "{said}");
        assert!(!taken.exists(), "an adopt deletion left .taken behind");

        commit_rel(&root, ".ljos/atoms/otherhost.jsonl.age", "x\n", "log");
        let other = root.join(".ljos/atoms/otherhost.jsonl.age");
        let out = git(
            &root,
            &["rm", "-q", "--", ".ljos/atoms/otherhost.jsonl.age"],
        )
        .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = git(
            &root,
            &["commit", "-q", "-m", "chore(sync): otherhost cloud atoms"],
        )
        .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let kept = taken_path(&root, "cloud", "otherhost");
        std::fs::write(&kept, "abc\n").unwrap();
        let said = adopt_in_repo(
            &root,
            &cloud(),
            "otherhost",
            AdoptOpts {
                push: false,
                force: true,
                dry_run: false,
            },
        )
        .unwrap();
        assert!(said.contains("has no cloud log"), "{said}");
        assert!(kept.exists(), "a non-adopt deletion cleared .taken");
        assert!(!other.exists());
    }

    #[test]
    fn a_resume_without_an_identity_does_not_commit_the_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let _env = hold_env(dir.path());
        let root = tracker(dir.path());
        let rel = ".ljos/atoms/gone.jsonl.age";
        commit_rel(&root, rel, "sealed\n", "log");
        let head = git(&root, &["rev-parse", "HEAD"]).unwrap();
        std::fs::remove_file(root.join(rel)).unwrap();
        let said = adopt_in_repo(
            &root,
            &cloud(),
            "gone",
            AdoptOpts {
                push: false,
                force: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert!(said.contains("no age identity"), "{said}");
        assert!(in_head(&root, rel), "the resume committed the deletion");
        assert!(!root.join(rel).exists());
        let again = git(&root, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head.stdout, again.stdout);
    }

    #[test]
    fn a_failed_fetch_does_not_adopt() {
        let dir = tempfile::tempdir().unwrap();
        let _env = hold_env(dir.path());
        let root = tracker(dir.path());
        std::fs::write(
            root.join(".ljos/sync.toml"),
            "name = \"box\"\nrecipients = [\"age1test\"]\n",
        )
        .unwrap();
        let rel = ".ljos/atoms/gone.jsonl.age";
        commit_rel(&root, rel, "sealed\n", "log");
        let out = git(
            &root,
            &["remote", "add", "origin", "/nonexistent/ljos-tracker.git"],
        )
        .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let listed = root.display().to_string();
        std::fs::write(
            dir.path().join("config/ljos/sync.toml"),
            format!("repos = [\"{listed}\"]\n"),
        )
        .unwrap();
        let err = adopt("gone", true, false).unwrap_err();
        let err = err.to_string();
        assert!(err.contains("not current"), "{err}");
        assert!(err.contains("fetch failed"), "{err}");
        assert_eq!(std::fs::read_to_string(root.join(rel)).unwrap(), "sealed\n");
        assert!(in_head(&root, rel));
    }

    #[test]
    fn an_adopt_names_a_missing_scope() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(&root).unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        let listed = root.display().to_string();
        assert!(
            !listed.contains('"'),
            "the temporary path is not a toml string"
        );
        std::fs::write(
            config.join("ljos/sync.toml"),
            format!("repos = [\"{listed}\"]\n"),
        )
        .unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &config);
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let head = git(&source, &["rev-parse", "HEAD"]).unwrap();
        let said = adopt("otherhost", false, false).unwrap();
        let again = git(&source, &["rev-parse", "HEAD"]).unwrap();
        assert!(said.contains("names no scope"), "{said}");
        assert!(
            !said.contains("ljos checkout"),
            "adopt fell back to the source tree: {said}"
        );
        assert_eq!(
            head.stdout, again.stdout,
            "adopt committed the ljos checkout"
        );
        assert!(!source.join(".gitattributes").exists());
    }

    /// The suite's working directory is inside the ljos checkout. A tracker
    /// root that falls back there must not commit, and must not write the
    /// attributes file the stray commit `3df31c6` added.
    #[test]
    fn a_test_cannot_commit_the_ljos_checkout() {
        // Git-spawning tests share the process with writers that replace
        // PATH. The env lock is what puts the real PATH back first.
        let _g = crate::test_env_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let head = git(&root, &["rev-parse", "HEAD"]).unwrap();
        assert!(head.status.success());
        let err = crate::tracker_push::prepare_tracker(&root, false, true).unwrap_err();
        assert!(err.contains("ljos checkout"), "{err}");
        assert!(
            !root.join(".gitattributes").exists(),
            "prepare_tracker wrote attributes into the crate"
        );
        match crate::commit_paths(&root, &["Cargo.toml"], "do not commit the checkout") {
            crate::PathCommit::Refused(why) => assert!(why.contains("ljos checkout"), "{why}"),
            other => panic!("commit was not refused: {other:?}"),
        }
        let again = git(&root, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head.stdout, again.stdout);
    }

    #[test]
    fn a_sealed_log_opens_for_a_recipient_and_not_for_another() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let key = |name: &str| {
            let path = dir.path().join(name);
            let made = Command::new("age-keygen")
                .arg("-o")
                .arg(&path)
                .output()
                .unwrap();
            assert!(made.status.success(), "age-keygen must be installed");
            let public = Command::new("age-keygen")
                .arg("-y")
                .arg(&path)
                .output()
                .unwrap();
            (
                path,
                String::from_utf8_lossy(&public.stdout).trim().to_string(),
            )
        };
        let (mine, my_public) = key("mine.key");
        let (other, _) = key("other.key");
        let out = dir.path().join("host.jsonl.age");
        seal("{\"text\":\"x\"}\n", &[my_public], &out).unwrap();
        assert_eq!(
            open(&out, &mine).unwrap().as_deref(),
            Some("{\"text\":\"x\"}\n")
        );
        assert_eq!(open(&out, &other).unwrap(), None);
        assert!(seal("x", &[], &out).is_err(), "no recipients is refused");
    }

    /// rage under fr_FR says "Aucune clef correspondante" instead of
    /// "no matching keys found". That used to abort import for every
    /// host still waiting.
    #[test]
    fn a_localized_rage_miss_stays_a_miss_under_lc_all_c() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let script = "#!/bin/sh\nif [ \"$LC_ALL\" = C ]; then echo 'no matching keys found' >&2; exit 1; fi\necho 'Aucune clef correspondante' >&2; exit 1\n";
        for name in ["age", "rage"] {
            let path = bin.join(name);
            std::fs::write(&path, script).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let old_path = std::env::var("PATH").unwrap_or_default();
        // Only the fakes. A real age later on PATH would hide rage.
        std::env::set_var("PATH", &bin);
        let log = dir.path().join("peer.jsonl.age");
        std::fs::write(&log, b"sealed").unwrap();
        let id = dir.path().join("age.key");
        std::fs::write(&id, b"key").unwrap();
        let opened = open(&log, &id);
        std::fs::remove_file(bin.join("age")).unwrap();
        let via_rage = open(&log, &id);
        std::env::set_var("PATH", &old_path);
        assert!(matches!(opened, Ok(None)), "age: {opened:?}");
        assert!(matches!(via_rage, Ok(None)), "rage: {via_rage:?}");
    }

    /// rage (via sys-locale) asks LANGUAGE before LC_ALL, then
    /// LC_MESSAGES, then LANG. A parent LANGUAGE=fr or LC_MESSAGES=fr
    /// stays French when the child only sets LC_ALL and LANG, and the
    /// English miss check then aborts import.
    #[test]
    fn a_rage_miss_stays_a_miss_when_language_is_french() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let script = "#!/bin/sh\nif [ \"$LC_ALL\" = C ] && [ \"$LANG\" = C ] && [ \"$LANGUAGE\" = C ] && [ \"$LC_MESSAGES\" = C ]; then echo 'no matching keys found' >&2; exit 1; fi\necho 'Aucune clef correspondante' >&2; exit 1\n";
        for name in ["age", "rage"] {
            let path = bin.join(name);
            std::fs::write(&path, script).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let old_path = std::env::var("PATH").unwrap_or_default();
        let old_language = std::env::var("LANGUAGE").ok();
        let old_lc_messages = std::env::var("LC_MESSAGES").ok();
        std::env::set_var("LANGUAGE", "fr");
        std::env::set_var("LC_MESSAGES", "fr");
        std::env::set_var("PATH", &bin);
        let log = dir.path().join("peer.jsonl.age");
        std::fs::write(&log, b"sealed").unwrap();
        let id = dir.path().join("age.key");
        std::fs::write(&id, b"key").unwrap();
        let opened = open(&log, &id);
        std::fs::remove_file(bin.join("age")).unwrap();
        let via_rage = open(&log, &id);
        std::env::set_var("PATH", &old_path);
        match old_language {
            Some(value) => std::env::set_var("LANGUAGE", value),
            None => std::env::remove_var("LANGUAGE"),
        }
        match old_lc_messages {
            Some(value) => std::env::set_var("LC_MESSAGES", value),
            None => std::env::remove_var("LC_MESSAGES"),
        }
        assert!(
            matches!(opened, Ok(None)),
            "age kept the French miss: {opened:?}"
        );
        assert!(
            matches!(via_rage, Ok(None)),
            "rage kept the French miss: {via_rage:?}"
        );
    }

    /// A miss on a log this machine's key is listed for is a broken seal,
    /// not a host to skip. Asking the pack first hid that behind
    /// PACKSET_URL=off.
    #[test]
    fn a_log_sealed_to_this_machine_that_will_not_open_is_an_error() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let age = bin.join("age");
        std::fs::write(
            &age,
            "#!/bin/sh\necho 'no matching keys found' >&2\nexit 1\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&age, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("PATH", &bin);
        std::env::set_var("PACKSET_URL", "off");
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        std::fs::write(
            config.join("ljos/age.key"),
            "# public key: age1ownedkey\nAGE-SECRET-KEY-1TEST\n",
        )
        .unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &config);
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        std::fs::write(root.join(".ljos/atoms/peerbox.jsonl.age"), b"sealed").unwrap();
        let scope: Scope =
            toml::from_str("name = \"surf\"\nrecipients = [\"age1ownedkey\"]\n").unwrap();
        let err = import(&root, &scope).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("peerbox")
                && msg.contains("sealed to this machine")
                && msg.contains("did not open"),
            "{msg}"
        );
        assert!(!msg.contains("PACKSET_URL"), "{msg}");
        assert!(!msg.contains("not sealed"), "{msg}");
    }

    /// No `# public key:` comment: the recipient comes from age-keygen -y.
    #[test]
    fn a_log_sealed_to_the_keygen_recipient_that_will_not_open_is_an_error() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("age"),
            "#!/bin/sh\necho 'no matching keys found' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::write(
            bin.join("age-keygen"),
            "#!/bin/sh\nif [ \"$1\" = -y ]; then echo age1fromkeygen; exit 0; fi\nexit 1\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        for name in ["age", "age-keygen"] {
            std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        std::env::set_var("PATH", &bin);
        std::env::set_var("PACKSET_URL", "off");
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        std::fs::write(config.join("ljos/age.key"), "AGE-SECRET-KEY-1TEST\n").unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &config);
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        std::fs::write(root.join(".ljos/atoms/peerbox.jsonl.age"), b"sealed").unwrap();
        let scope: Scope =
            toml::from_str("name = \"surf\"\nrecipients = [\"age1fromkeygen\"]\n").unwrap();
        let msg = import(&root, &scope).unwrap_err().to_string();
        assert!(
            msg.contains("peerbox") && msg.contains("did not open"),
            "{msg}"
        );
        assert!(!msg.contains("PACKSET_URL"), "{msg}");
    }

    /// Scope `cloud` is opened with another identity too. A miss on its
    /// log is still an error when that identity's recipient is listed,
    /// even though the machine key is not. Another scope listing it stays
    /// a skip: its logs are never tried with the cloud identity.
    #[test]
    fn a_log_sealed_to_the_cloud_identity_that_will_not_open_is_an_error() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("age"),
            "#!/bin/sh\necho 'no matching keys found' >&2\nexit 1\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("age"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("PATH", &bin);
        std::env::set_var("PACKSET_URL", "off");
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        let secret = format!("AGE-SECRET-KEY-1{}\n", "Q".repeat(58));
        std::fs::write(
            config.join("ljos/age.key"),
            format!("# public key: age1machine\n{secret}"),
        )
        .unwrap();
        std::fs::write(
            config.join("ljos/cloud.age"),
            format!("# public key: age1cloud\n{secret}"),
        )
        .unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &config);
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        let cloud_log = root.join(".ljos/atoms/peerbox+cloud.jsonl.age");
        std::fs::write(&cloud_log, b"sealed").unwrap();
        let scope: Scope = toml::from_str(
            "name = \"surf\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let msg = import(&root, &scope).unwrap_err().to_string();
        assert!(
            msg.contains("peerbox's cloud log")
                && msg.contains("sealed to this machine")
                && msg.contains("did not open"),
            "{msg}"
        );
        assert!(!msg.contains("not sealed"), "{msg}");
        assert!(!msg.contains("PACKSET_URL"), "{msg}");

        std::fs::remove_file(&cloud_log).unwrap();
        std::fs::write(root.join(".ljos/atoms/peerbox.jsonl.age"), b"sealed").unwrap();
        let scope: Scope =
            toml::from_str("name = \"surf\"\nrecipients = [\"age1cloud\"]\n").unwrap();
        let msg = import(&root, &scope).unwrap();
        assert!(
            msg.contains("sync: peerbox's surf log is not sealed to this machine"),
            "{msg}"
        );
        assert!(!msg.contains("PACKSET_URL"), "{msg}");
        assert!(!msg.contains("did not open"), "{msg}");
    }

    /// An extra scope's recipients are its own. A primary log is not owned
    /// just because `[scopes]` lists this machine, and a `+cloud` log is.
    #[test]
    fn a_miss_uses_the_log_scopes_recipients() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("age"),
            "#!/bin/sh\necho 'no matching keys found' >&2\nexit 1\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("age"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("PATH", &bin);
        std::env::set_var("PACKSET_URL", "off");
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        let secret = format!("AGE-SECRET-KEY-1{}\n", "Q".repeat(58));
        std::fs::write(
            config.join("ljos/age.key"),
            format!("# public key: age1cloud\n{secret}"),
        )
        .unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &config);
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        std::fs::write(root.join(".ljos/atoms/peerbox+cloud.jsonl.age"), b"sealed").unwrap();
        let msg = import(&root, &scope).unwrap_err().to_string();
        assert!(
            msg.contains("peerbox's cloud log") && msg.contains("did not open"),
            "{msg}"
        );
        assert!(!msg.contains("PACKSET_URL"), "{msg}");

        std::fs::remove_file(root.join(".ljos/atoms/peerbox+cloud.jsonl.age")).unwrap();
        std::fs::write(root.join(".ljos/atoms/peerbox.jsonl.age"), b"sealed").unwrap();
        let msg = import(&root, &scope).unwrap();
        assert!(
            msg.contains("sync: peerbox's grokbot log is not sealed to this machine"),
            "a primary log must stay a skip when only the extra scope lists us: {msg}"
        );
        assert!(!msg.contains("PACKSET_URL"), "{msg}");
        assert!(!msg.contains("did not open"), "{msg}");
        assert!(!msg.contains("is sealed to this machine"), "{msg}");
    }

    /// No public-key comment, and age-keygen -y fails. That identity is not
    /// a known recipient, so another machine's logs stay skips. Stopping
    /// on the keygen miss, or asking the pack when nothing opened, hid
    /// those lines behind an error.
    #[test]
    fn a_keygen_miss_on_foreign_logs_stays_a_skip() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("age"),
            "#!/bin/sh\necho 'no matching keys found' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::write(
            bin.join("age-keygen"),
            "#!/bin/sh\necho 'age-keygen: refusing' >&2\nexit 1\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        for name in ["age", "age-keygen"] {
            std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let old_path = std::env::var("PATH").unwrap_or_default();
        let old_pack = std::env::var("PACKSET_URL").ok();
        let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let old_host = std::env::var("LJOS_TEST_HOSTNAME").ok();
        std::env::set_var("PATH", &bin);
        std::env::set_var("PACKSET_URL", "off");
        std::env::set_var("LJOS_TEST_HOSTNAME", "thisbox");
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        std::fs::write(
            config.join("ljos/age.key"),
            format!("AGE-SECRET-KEY-1{}\n", "Q".repeat(58)),
        )
        .unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &config);
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        std::fs::write(root.join(".ljos/atoms/peerbox.jsonl.age"), b"sealed").unwrap();
        std::fs::write(root.join(".ljos/atoms/otherbox.jsonl.age"), b"sealed").unwrap();
        let scope: Scope =
            toml::from_str("name = \"surf\"\nrecipients = [\"age1someoneelse\"]\n").unwrap();
        let result = import(&root, &scope);
        std::env::set_var("PATH", old_path);
        match old_pack {
            Some(value) => std::env::set_var("PACKSET_URL", value),
            None => std::env::remove_var("PACKSET_URL"),
        }
        match old_xdg {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match old_host {
            Some(value) => std::env::set_var("LJOS_TEST_HOSTNAME", value),
            None => std::env::remove_var("LJOS_TEST_HOSTNAME"),
        }
        let msg = result.expect("foreign logs stay skips");
        assert!(
            msg.contains("sync: otherbox's surf log is not sealed to this machine"),
            "{msg}"
        );
        assert!(
            msg.contains("sync: peerbox's surf log is not sealed to this machine"),
            "{msg}"
        );
        assert!(!msg.contains("PACKSET_URL"), "{msg}");
        assert!(!msg.contains("age-keygen"), "{msg}");
        assert!(!msg.contains("did not open"), "{msg}");
    }

    #[test]
    fn a_miss_for_someone_else_stays_a_skip_line() {
        let skip = miss_report("peerbox", "surf", "age1mine", &["age1other".into()]).unwrap();
        assert_eq!(
            skip,
            "sync: peerbox's surf log is not sealed to this machine\n"
        );
        let owned = miss_report(
            "peerbox",
            "surf",
            "age1mine",
            &["age1other".into(), "age1mine".into()],
        )
        .unwrap_err()
        .to_string();
        assert!(owned.contains("sealed to this machine"), "{owned}");
        assert!(owned.contains("did not open"), "{owned}");
        assert!(!owned.contains("not sealed"), "{owned}");
    }

    #[test]
    fn an_atoms_scope_is_its_entity_or_the_default() {
        assert_eq!(
            atom_scope(&atom("a", "t", &["scope:personal"]), "surf"),
            "personal"
        );
        assert_eq!(atom_scope(&atom("a", "t", &["seat:x"]), "surf"), "surf");
    }

    #[test]
    fn inbox_sync_refuses_a_name_that_is_not_a_commit() {
        let err = fetch_for_inbox(Some("nope")).unwrap_err();
        assert_eq!(err.to_string(), "sync: nope is not a commit");
    }

    /// A commit made here and never pushed is in the object store, but no
    /// remote ref has it. It is named, and `HEAD` is not moved onto it.
    #[test]
    fn a_commit_only_this_checkout_has_is_not_on_the_remote() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let root = dir.path().join("local");
        let git = |cwd: &Path, args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare"]);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "seat@example.invalid"]);
        git(&root, &["config", "user.name", "seat"]);
        git(&root, &["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(root.join("README"), "seed\n").unwrap();
        git(&root, &["add", "README"]);
        git(&root, &["commit", "-q", "-m", "seed"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&root, &["push", "-q", "-u", "origin", "HEAD"]);
        let head = git(&root, &["rev-parse", "HEAD"]);
        let local = git(
            &root,
            &["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "local"],
        );

        assert_eq!(
            note_commit(&root, &local, true),
            format!("sync: {local} is not on the remote; imported the remote head\n")
        );
        assert_eq!(
            git(&root, &["rev-parse", "HEAD"]),
            head,
            "HEAD moved onto a commit the remote does not have"
        );

        let branch = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let refspec = format!("{local}:refs/heads/{branch}");
        git(&root, &["push", "-q", "origin", &refspec]);
        git(&root, &["fetch", "-q", "origin"]);
        assert_eq!(note_commit(&root, &local, true), "");
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), local);
    }

    #[test]
    fn an_age_identity_file_is_one_secret_line() {
        let line = format!("AGE-SECRET-KEY-1{}", "Q".repeat(58));
        let file = format!("# created: 2021-01-02T15:30:45Z\n# public key: age1example\n{line}\n");
        assert!(age_identity(&file));
        assert!(!age_identity(
            "age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p\n"
        ));
        assert!(!age_identity(&line.to_lowercase()));
        assert!(!age_identity("AGE-SECRET-KEY-1\n"));
        assert!(!age_identity("# only a comment\n"));
        assert!(!age_identity(&format!("{line}\nnot a key\n")));
        assert!(!age_identity(&format!("{line}\0\n")));
    }

    #[test]
    fn the_cloud_identity_opens_a_log_without_a_machine_key() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        std::env::set_var("HOME", dir.path().join("home"));
        let cloud = cloud_identity_path();
        assert!(
            cloud.starts_with(dir.path()),
            "cloud identity left the test directory: {}",
            cloud.display()
        );
        std::fs::create_dir_all(cloud.parent().unwrap()).unwrap();
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(&cloud)
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "age-keygen must be installed: {}",
            String::from_utf8_lossy(&made.stderr)
        );
        let public = Command::new("age-keygen")
            .arg("-y")
            .arg(&cloud)
            .output()
            .unwrap();
        assert!(public.status.success());
        let public = String::from_utf8_lossy(&public.stdout).trim().to_string();
        assert!(!identity_path().exists(), "the machine key was invented");
        let sealed = dir.path().join("cloud.jsonl.age");
        seal("{\"text\":\"from cloud\"}\n", &[public], &sealed).unwrap();
        assert_eq!(
            open_identities(&sealed, &identities_for_open(CLOUD_SCOPE))
                .unwrap()
                .as_deref(),
            Some("{\"text\":\"from cloud\"}\n")
        );

        let other = dir.path().join("other.key");
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(&other)
            .output()
            .unwrap();
        assert!(made.status.success());
        let other_public = Command::new("age-keygen")
            .arg("-y")
            .arg(&other)
            .output()
            .unwrap();
        let other_public = String::from_utf8_lossy(&other_public.stdout)
            .trim()
            .to_string();
        let closed = dir.path().join("closed.jsonl.age");
        seal("nope\n", &[other_public], &closed).unwrap();
        assert_eq!(
            open_identities(&closed, &identities_for_open(CLOUD_SCOPE)).unwrap(),
            None
        );

        let own = identity_path();
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(&own)
            .output()
            .unwrap();
        assert!(made.status.success());
        assert_eq!(
            open_identities(&sealed, &identities_for_open(CLOUD_SCOPE))
                .unwrap()
                .as_deref(),
            Some("{\"text\":\"from cloud\"}\n"),
            "the machine key must not hide the cloud identity"
        );
    }

    #[test]
    fn a_broken_cloud_file_is_not_passed_to_age() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        let own = identity_path();
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(&own)
            .output()
            .unwrap();
        assert!(made.status.success(), "age-keygen must be installed");
        std::fs::write(cloud_identity_path(), "not a key\n").unwrap();
        assert_eq!(identities_for_open(CLOUD_SCOPE), vec![own]);
    }

    /// A pack that keeps what is posted to it and lists it back.
    fn serve_pack(posted: std::sync::Arc<std::sync::Mutex<Vec<Value>>>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while let Ok(n) = stream.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buf[..at]).to_ascii_lowercase();
                    let need: usize = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|n| n.trim().parse().ok())
                        .unwrap_or(0);
                    if buf.len() >= at + 4 + need {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf).into_owned();
                let line = req.lines().next().unwrap_or("");
                let body = req.split_once("\r\n\r\n").map_or("", |(_, body)| body);
                let answer = if line.starts_with("GET /v1/atoms") {
                    serde_json::json!({ "atoms": posted.lock().unwrap().clone() }).to_string()
                } else if line.starts_with("POST /v1/atoms ") {
                    let mut atom: Value = serde_json::from_str(body).unwrap_or_default();
                    let mut held = posted.lock().unwrap();
                    atom["id"] = Value::String(format!("posted-{}", held.len()));
                    held.push(atom.clone());
                    atom.to_string()
                } else {
                    "{}".into()
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        url
    }

    /// Point this process at `dir` as a VM that holds `cloud.age` and no
    /// machine key. Returns the cloud identity's public key.
    fn vm_with_only_the_cloud_identity(dir: &Path) -> String {
        std::env::set_var("HOME", dir.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        std::env::set_var("XDG_STATE_HOME", dir.join("state"));
        std::env::set_var("PACKSET_TIMEOUT_MS", "5000");
        let cloud = cloud_identity_path();
        std::fs::create_dir_all(cloud.parent().unwrap()).unwrap();
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(&cloud)
            .output()
            .unwrap();
        assert!(made.status.success(), "age-keygen must be installed");
        let public = Command::new("age-keygen")
            .arg("-y")
            .arg(&cloud)
            .output()
            .unwrap();
        assert!(public.status.success());
        assert!(!identity_path().exists());
        String::from_utf8_lossy(&public.stdout).trim().to_string()
    }

    fn posted_texts(posted: &std::sync::Mutex<Vec<Value>>) -> Vec<String> {
        posted
            .lock()
            .unwrap()
            .iter()
            .filter_map(|a| a["text"].as_str().map(str::to_string))
            .collect()
    }

    /// Memory leaves the box only as scope cloud. A box scope's log is not
    /// tried with the cloud identity, even one the cloud key could open.
    #[test]
    fn a_box_scope_log_is_never_opened_with_the_cloud_identity() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let public = vm_with_only_the_cloud_identity(dir.path());
        assert!(identities_for_open("grokbot").is_empty());
        assert_eq!(
            identities_for_open(CLOUD_SCOPE),
            vec![cloud_identity_path()]
        );
        let root = dir.path().join("tracker");
        let atoms = log_dir(&root);
        std::fs::create_dir_all(&atoms).unwrap();
        let scope: Scope = toml::from_str(&format!(
            "name = \"grokbot\"\nrecipients = [\"{public}\"]\n[scopes]\ncloud = [\"{public}\"]\n"
        ))
        .unwrap();
        let to_cloud = std::slice::from_ref(&public);
        let box_log = format!("{}\n", atom("a", "box only", &["scope:grokbot"]));
        seal(&box_log, to_cloud, &atoms.join("gonehost.jsonl.age")).unwrap();
        let cloud_log = format!("{}\n", atom("b", "cloud only", &["scope:cloud"]));
        seal(
            &cloud_log,
            to_cloud,
            &atoms.join("gonehost+cloud.jsonl.age"),
        )
        .unwrap();
        let posted = std::sync::Arc::default();
        std::env::set_var("PACKSET_URL", serve_pack(std::sync::Arc::clone(&posted)));
        let said = import(&root, &scope).unwrap();
        assert!(
            said.contains("gonehost's grokbot log is not sealed to this machine"),
            "{said}"
        );
        assert!(said.contains("gonehost: 1 cloud atoms taken"), "{said}");
        assert_eq!(posted_texts(&posted), vec!["cloud only"]);
    }

    /// A VM holds cloud.age and no machine key. Adopting a gone host reads
    /// that host's scope cloud log with the cloud identity, keeps its atoms
    /// in this host's cloud log, and keeps the primary log in place.
    #[test]
    fn adopt_reads_a_gone_hosts_cloud_log_with_only_the_cloud_identity() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let public = vm_with_only_the_cloud_identity(dir.path());
        let box_public = age_keygen(&dir.path().join("box.key"));
        let root = dir.path().join("tracker");
        let atoms = log_dir(&root);
        std::fs::create_dir_all(&atoms).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            format!(
                "name = \"grokbot\"\nrecipients = [\"{box_public}\"]\n[scopes]\ncloud = [\"{public}\"]\n"
            ),
        )
        .unwrap();
        let scope = scope_of_repo(&root).unwrap().unwrap();
        let box_plain = format!(
            "{}\n",
            atom("b", "from the primary log", &["scope:grokbot"])
        );
        seal(
            &box_plain,
            std::slice::from_ref(&box_public),
            &atoms.join("gonehost.jsonl.age"),
        )
        .unwrap();
        std::fs::write(atoms.join("gonehost.digest"), "digest\n").unwrap();
        let plain = format!("{}\n", atom("a", "from the gone host", &["scope:cloud"]));
        seal(
            &plain,
            std::slice::from_ref(&public),
            &atoms.join("gonehost+cloud.jsonl.age"),
        )
        .unwrap();
        std::fs::write(atoms.join("gonehost+cloud.digest"), "digest\n").unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "seat@example.invalid"]);
        git(&["config", "user.name", "seat"]);
        git(&["config", "core.hooksPath", "/dev/null"]);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "seed"]);
        let posted = std::sync::Arc::default();
        std::env::set_var("PACKSET_URL", serve_pack(std::sync::Arc::clone(&posted)));

        let said = adopt_in_repo(&root, &scope, "gonehost", opts(true, false)).unwrap();
        assert!(
            said.contains("gonehost: 1 cloud atoms adopted, 0 refused"),
            "{said}"
        );
        assert!(
            said.contains("gonehost's grokbot log is not sealed to this machine"),
            "{said}"
        );
        assert_eq!(posted_texts(&posted), vec!["from the gone host"]);
        let mine = atoms.join(format!("{}+cloud.jsonl.age", host()));
        let kept = open_identities(&mine, &identities_for_open(CLOUD_SCOPE))
            .unwrap()
            .unwrap_or_default();
        assert!(kept.contains("from the gone host"), "{kept}");
        assert!(atoms.join("gonehost.jsonl.age").exists());
        assert!(!atoms.join("gonehost+cloud.jsonl.age").exists());
        assert!(git(&["log", "-1", "--format=%s"]).contains("adopt gonehost grokbot atoms"));
        let tracked = git(&["ls-files", ".ljos/atoms"]);
        assert!(tracked.contains("gonehost.jsonl.age"), "{tracked}");
        assert!(!tracked.contains("gonehost+cloud"), "{tracked}");
    }

    #[test]
    fn adopt_reads_a_gone_hosts_primary_log_without_the_cloud_identity() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let box_public = make_key();
        let cloud_public = age_keygen(&dir.path().join("cloud.key"));
        let root = dir.path().join("tracker");
        let atoms = log_dir(&root);
        std::fs::create_dir_all(&atoms).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            format!(
                "name = \"grokbot\"\nrecipients = [\"{box_public}\"]\n[scopes]\ncloud = [\"{cloud_public}\"]\n"
            ),
        )
        .unwrap();
        let scope = scope_of_repo(&root).unwrap().unwrap();
        let box_plain = format!(
            "{}\n",
            atom("a", "from the primary log", &["scope:grokbot"])
        );
        seal(
            &box_plain,
            std::slice::from_ref(&box_public),
            &atoms.join("gonehost.jsonl.age"),
        )
        .unwrap();
        std::fs::write(atoms.join("gonehost.digest"), "digest\n").unwrap();
        let cloud_plain = format!("{}\n", atom("b", "from the cloud log", &["scope:cloud"]));
        seal(
            &cloud_plain,
            std::slice::from_ref(&cloud_public),
            &atoms.join("gonehost+cloud.jsonl.age"),
        )
        .unwrap();
        std::fs::write(atoms.join("gonehost+cloud.digest"), "digest\n").unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "seat@example.invalid"]);
        git(&["config", "user.name", "seat"]);
        git(&["config", "core.hooksPath", "/dev/null"]);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "seed"]);
        let posted = std::sync::Arc::default();
        std::env::set_var("PACKSET_URL", serve_pack(std::sync::Arc::clone(&posted)));

        let said = adopt_in_repo(&root, &scope, "gonehost", opts(true, false)).unwrap();
        assert!(
            said.contains("gonehost: 1 grokbot atoms adopted, 0 refused"),
            "{said}"
        );
        assert!(
            said.contains("gonehost's cloud log is not sealed to this machine"),
            "{said}"
        );
        assert_eq!(posted_texts(&posted), vec!["from the primary log"]);
        let mine = atoms.join(format!("{}.jsonl.age", host()));
        let kept = open_identities(&mine, &identities_for_open("grokbot"))
            .unwrap()
            .unwrap_or_default();
        assert!(kept.contains("from the primary log"), "{kept}");
        assert!(!atoms.join("gonehost.jsonl.age").exists());
        assert!(atoms.join("gonehost+cloud.jsonl.age").exists());
        assert!(git(&["log", "-1", "--format=%s"]).contains("adopt gonehost grokbot atoms"));
        let tracked = git(&["ls-files", ".ljos/atoms"]);
        assert!(!tracked.contains("gonehost.jsonl.age"), "{tracked}");
        assert!(tracked.contains("gonehost+cloud.jsonl.age"), "{tracked}");
    }

    const OLD_COMMIT: &str = "100000000 +0000";

    fn begin(dir: &Path) -> crate::EnvLock {
        let lock = hold_env(dir);
        std::env::set_var("HOME", dir);
        lock
    }

    fn point_pack(url: &str) {
        std::env::set_var("PACKSET_URL", url);
        std::env::set_var("PACKSET_WORKSPACE", "seat");
        std::env::set_var("PACKSET_TIMEOUT_MS", "5000");
    }

    fn make_key() -> String {
        let path = identity_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "age-keygen: {}",
            String::from_utf8_lossy(&made.stderr)
        );
        let public = Command::new("age-keygen")
            .arg("-y")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            public.status.success(),
            "{}",
            String::from_utf8_lossy(&public.stderr)
        );
        String::from_utf8_lossy(&public.stdout).trim().to_string()
    }

    fn commit_dated(root: &Path, rels: &[&str], message: &str, date: Option<&str>) {
        let mut add = vec!["add", "--"];
        add.extend_from_slice(rels);
        let out = git(root, &add).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(root)
            .args(["commit", "-q", "-m", message, "--"])
            .args(rels);
        if let Some(date) = date {
            cmd.env("GIT_AUTHOR_DATE", date);
            cmd.env("GIT_COMMITTER_DATE", date);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn scope_of(public: &str) -> Scope {
        Scope {
            name: "cloud".into(),
            recipients: vec![public.to_string()],
            projects: BTreeMap::new(),
            scopes: BTreeMap::new(),
        }
    }

    fn cloud_plain() -> String {
        format!(
            "{}\n{}\n{}\n",
            serde_json::json!({
                "text": "fresh", "kind": "lesson", "origin": "user-declared",
                "level": "explicit", "entities": ["seat:x"]
            }),
            serde_json::json!({
                "text": "fresh", "kind": "lesson", "issue": "ljos-b",
                "entities": ["issue:ljos-b"]
            }),
            serde_json::json!({"text": "mine", "kind": "lesson", "entities": ["scope:cloud"]}),
        )
    }

    struct Fixture {
        root: PathBuf,
        public: String,
    }

    /// A quiet cloud log sealed to this machine, and a taken file that
    /// still names a text the log no longer carries.
    fn fixture(dir: &Path, date: Option<&str>) -> Fixture {
        assert_ne!(host(), "gone", "the fixture host would be this machine");
        let public = make_key();
        let root = tracker(dir);
        std::fs::write(
            root.join(".ljos/sync.toml"),
            format!("name = \"cloud\"\nrecipients = [\"{public}\"]\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("config/ljos/sync.toml"),
            "default_scope = \"private\"\n",
        )
        .unwrap();
        let age = ".ljos/atoms/gone.jsonl.age";
        seal(
            &cloud_plain(),
            std::slice::from_ref(&public),
            &root.join(age),
        )
        .unwrap();
        std::fs::write(root.join(".ljos/atoms/gone.digest"), "digest\n").unwrap();
        commit_dated(
            &root,
            &[age, ".ljos/atoms/gone.digest", ".ljos/sync.toml"],
            "log",
            date,
        );
        let taken = taken_path(&root, "cloud", "gone");
        std::fs::create_dir_all(taken.parent().unwrap()).unwrap();
        std::fs::write(&taken, format!("{}\n", crate::work_id("old"))).unwrap();
        Fixture { root, public }
    }

    fn hex_id(n: u64) -> String {
        format!("{n:032x}")
    }

    fn seeded(n: u64, text: &str, issue: &str, tags: &[&str]) -> Value {
        let mut atom = serde_json::json!({
            "id": hex_id(n),
            "text": text,
            "kind": "lesson",
            "entities": tags,
        });
        if !issue.is_empty() {
            atom["issue"] = Value::String(issue.to_string());
        }
        atom
    }

    fn seed_cloud(store: &std::sync::Mutex<Vec<Value>>) {
        *store.lock().unwrap() = vec![
            seeded(101, "fresh", "", &["sync:gone", "scope:cloud"]),
            seeded(
                102,
                "fresh",
                "ljos-b",
                &["sync:gone", "scope:cloud", "issue:ljos-b"],
            ),
            seeded(103, "mine", "", &["scope:cloud"]),
            seeded(104, "fresh", "", &["scope:private"]),
            seeded(105, "old", "", &["sync:gone"]),
            seeded(106, "old", "", &["sync:gone", "scope:private"]),
            seeded(107, "fresh", "", &["sync:gone", "scope:private"]),
        ];
    }

    fn ids(store: &std::sync::Mutex<Vec<Value>>) -> BTreeSet<String> {
        store
            .lock()
            .unwrap()
            .iter()
            .filter_map(|atom| atom["id"].as_str().map(str::to_string))
            .collect()
    }

    fn has(store: &std::sync::Mutex<Vec<Value>>, n: u64) -> bool {
        ids(store).contains(&hex_id(n))
    }

    fn opts(force: bool, dry_run: bool) -> AdoptOpts {
        AdoptOpts {
            push: false,
            force,
            dry_run,
        }
    }

    /// Both tickets are posted into the log's scope, the private copy does
    /// not swallow them, and the log is deleted only after that is visible.
    #[test]
    fn an_adopt_posts_each_issue_then_drops_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let fix = fixture(dir.path(), Some(OLD_COMMIT));
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        seed_cloud(&store);
        let said = adopt_in_repo(
            &fix.root,
            &scope_of(&fix.public),
            "gone",
            opts(false, false),
        )
        .unwrap();
        assert!(
            said.starts_with("sync: gone: 2 cloud atoms adopted, 0 refused, 2 moved, 1 retired\n"),
            "{said}"
        );
        assert!(said.contains("not pushed"), "{said}");
        assert!(!has(&store, 101), "the cloud sync copy was kept");
        assert!(!has(&store, 102), "the other ticket's sync copy was kept");
        assert!(!has(&store, 105), "a text the host dropped was kept");
        assert!(has(&store, 103), "this machine's own atom was forgotten");
        assert!(has(&store, 104), "the private copy was forgotten");
        assert!(has(&store, 106), "another scope's sync copy was forgotten");
        assert!(has(&store, 107), "another scope's live copy was forgotten");
        let live = store.lock().unwrap().clone();
        let posted: Vec<&Value> = live
            .iter()
            .filter(|atom| atom["origin"] == "peer")
            .collect();
        assert_eq!(posted.len(), 2, "{live:?}");
        assert!(posted.iter().all(|atom| atom.get("level").is_none()));
        assert!(posted.iter().all(|atom| {
            let tags = atom["entities"].to_string();
            tags.contains("scope:cloud") && !tags.contains("sync:")
        }));
        let cloud_log = log_plaintext(&live, "cloud", "private");
        let rows: BTreeSet<(String, String)> = cloud_log
            .lines()
            .map(|line| {
                let atom: Value = serde_json::from_str(line).unwrap();
                (
                    atom["text"].as_str().unwrap_or("").to_string(),
                    atom["issue"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            BTreeSet::from([
                ("fresh".to_string(), String::new()),
                ("fresh".to_string(), "ljos-b".to_string()),
                ("mine".to_string(), String::new()),
            ])
        );
        let private_log = log_plaintext(&live, "private", "private");
        assert_eq!(private_log.lines().count(), 1, "{private_log}");
        assert!(!private_log.contains("ljos-b"), "{private_log}");
        let mine = log_dir(&fix.root).join(format!("{}.jsonl.age", host()));
        let opened = open(&mine, &identity_path()).unwrap().unwrap();
        assert_eq!(opened.lines().count(), 3, "{opened}");
        assert!(opened.contains("scope:cloud"), "{opened}");
        assert!(!opened.contains("sync:"), "{opened}");
        assert!(!in_head(&fix.root, ".ljos/atoms/gone.jsonl.age"));
        assert!(!in_head(&fix.root, ".ljos/atoms/gone.digest"));
        assert!(!taken_path(&fix.root, "cloud", "gone").exists());
        let subject = git(&fix.root, &["log", "-1", "--format=%s"]).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&subject.stdout).trim(),
            "chore(sync): adopt gone cloud atoms"
        );
    }

    /// The export and the adopt commit name this host's log the same way,
    /// so the commit that drops the gone log carries the adopted atoms.
    #[test]
    fn an_adopt_commit_carries_the_log_of_a_host_with_a_space() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        std::env::set_var("LJOS_TEST_HOSTNAME", "bad host");
        let fix = fixture(dir.path(), Some(OLD_COMMIT));
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        seed_cloud(&store);
        let said = adopt_in_repo(
            &fix.root,
            &scope_of(&fix.public),
            "gone",
            opts(false, false),
        )
        .unwrap();
        assert!(said.contains("2 cloud atoms adopted"), "{said}");
        assert!(!in_head(&fix.root, ".ljos/atoms/gone.jsonl.age"));
        let changed = git(&fix.root, &["show", "--name-only", "--format=", "HEAD"]).unwrap();
        let changed = String::from_utf8_lossy(&changed.stdout);
        let rel = ".ljos/atoms/bad-host.jsonl.age";
        assert!(changed.lines().any(|line| line == rel), "{changed}");
        let sealed = head_blob(&fix.root, rel).unwrap();
        let opened = open_sealed_bytes(&sealed, &[identity_path()])
            .unwrap()
            .unwrap();
        assert_eq!(opened.lines().count(), 3, "{opened}");
        let raw = log_dir(&fix.root).join("bad host.jsonl.age");
        assert!(!raw.exists());
        assert_eq!(require_host_name(&host()).unwrap(), host());
        let stale = "{\"text\":\"stale\",\"kind\":\"lesson\",\"entities\":[]}\n";
        seal(stale, std::slice::from_ref(&fix.public), &raw).unwrap();
        let said = import(&fix.root, &scope_of(&fix.public)).unwrap();
        assert!(
            !said.contains("bad host:"),
            "this host's log under its raw name was imported: {said}"
        );
    }

    #[test]
    fn a_refused_post_keeps_the_log_and_its_sync_copies() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let fix = fixture(dir.path(), Some(OLD_COMMIT));
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::RefusePosts);
        point_pack(&url);
        seed_cloud(&store);
        let before = std::fs::read(fix.root.join(".ljos/atoms/gone.jsonl.age")).unwrap();
        let said = adopt_in_repo(
            &fix.root,
            &scope_of(&fix.public),
            "gone",
            opts(false, false),
        )
        .unwrap();
        assert!(said.contains("2 refused"), "{said}");
        assert!(said.contains("the log was kept"), "{said}");
        assert_eq!(
            std::fs::read(fix.root.join(".ljos/atoms/gone.jsonl.age")).unwrap(),
            before
        );
        assert!(in_head(&fix.root, ".ljos/atoms/gone.jsonl.age"));
        assert!(taken_path(&fix.root, "cloud", "gone").exists());
        assert!(has(&store, 101));
        assert!(has(&store, 102));
        assert!(!has(&store, 105), "the dropped text is still retired");
        let subject = git(&fix.root, &["log", "-1", "--format=%s"]).unwrap();
        assert_eq!(String::from_utf8_lossy(&subject.stdout).trim(), "log");
    }

    #[test]
    fn an_echoed_sync_atom_is_not_treated_as_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let fix = fixture(dir.path(), Some(OLD_COMMIT));
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::EchoText);
        point_pack(&url);
        seed_cloud(&store);
        let said = adopt_in_repo(
            &fix.root,
            &scope_of(&fix.public),
            "gone",
            opts(false, false),
        )
        .unwrap();
        assert!(said.contains("2 refused"), "{said}");
        assert!(said.contains("the log was kept"), "{said}");
        assert!(in_head(&fix.root, ".ljos/atoms/gone.jsonl.age"));
        assert!(has(&store, 101));
        assert!(has(&store, 102));
        let live = store.lock().unwrap().clone();
        assert!(live.iter().all(|atom| atom["origin"] != "peer"), "{live:?}");
    }

    #[test]
    fn a_dry_run_prints_the_plan_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let fix = fixture(dir.path(), Some(OLD_COMMIT));
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        seed_cloud(&store);
        let head = git(&fix.root, &["rev-parse", "HEAD"]).unwrap();
        let said =
            adopt_in_repo(&fix.root, &scope_of(&fix.public), "gone", opts(false, true)).unwrap();
        assert!(
            said.contains(
                "would adopt 2 cloud atoms, retire 1, and drop .ljos/atoms/gone.jsonl.age"
            ),
            "{said}"
        );
        assert_eq!(ids(&store).len(), 7, "a dry run posted or forgot");
        assert!(fix.root.join(".ljos/atoms/gone.jsonl.age").is_file());
        assert!(taken_path(&fix.root, "cloud", "gone").exists());
        let again = git(&fix.root, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head.stdout, again.stdout);
    }

    #[test]
    fn a_young_log_needs_force_and_a_dry_run_still_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let fix = fixture(dir.path(), None);
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        seed_cloud(&store);
        let err = adopt_in_repo(&fix.root, &scope_of(&fix.public), "gone", opts(false, true))
            .unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        assert_eq!(ids(&store).len(), 7);
        let head = git(&fix.root, &["rev-parse", "HEAD"]).unwrap();
        let said =
            adopt_in_repo(&fix.root, &scope_of(&fix.public), "gone", opts(true, true)).unwrap();
        assert!(said.contains("would adopt 2"), "{said}");
        assert_eq!(ids(&store).len(), 7);
        assert!(fix.root.join(".ljos/atoms/gone.jsonl.age").is_file());
        let again = git(&fix.root, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head.stdout, again.stdout);
    }

    /// The worktree file is gone and the commit is not. Pack work runs
    /// from the blob in HEAD, and a young log does not need `--force`
    /// again: the deletion already started.
    #[test]
    fn a_missing_log_is_resumed_from_head() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let fix = fixture(dir.path(), None);
        std::fs::remove_file(fix.root.join(".ljos/atoms/gone.jsonl.age")).unwrap();
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        seed_cloud(&store);
        let said = adopt_in_repo(
            &fix.root,
            &scope_of(&fix.public),
            "gone",
            opts(false, false),
        )
        .unwrap();
        assert!(
            said.starts_with("sync: gone: 2 cloud atoms adopted, 0 refused, 2 moved, 1 retired\n"),
            "{said}"
        );
        assert!(!in_head(&fix.root, ".ljos/atoms/gone.jsonl.age"));
        assert!(!in_head(&fix.root, ".ljos/atoms/gone.digest"));
        assert!(!taken_path(&fix.root, "cloud", "gone").exists());
        assert!(!has(&store, 101));
        assert!(has(&store, 104));
    }

    fn adopt_deletion(root: &Path) {
        let rel = ".ljos/atoms/gone.jsonl.age";
        commit_rel(root, rel, "sealed\n", "log");
        let out = git(root, &["rm", "-q", "--", rel]).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = git(
            root,
            &["commit", "-q", "-m", "chore(sync): adopt gone cloud atoms"],
        )
        .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn an_import_retires_an_adopted_host_without_its_log() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let public = make_key();
        let root = tracker(dir.path());
        adopt_deletion(&root);
        // The deletion is the only file under `.ljos`, so the directory may
        // already be gone. Import has to retire without it.
        if root.join(".ljos").exists() {
            std::fs::remove_dir_all(root.join(".ljos")).unwrap();
        }
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        *store.lock().unwrap() = vec![
            seeded(101, "fresh", "", &["sync:gone", "scope:cloud"]),
            seeded(105, "old", "", &["sync:gone"]),
            seeded(106, "old", "", &["sync:gone", "scope:private"]),
        ];
        let said = import(&root, &scope_of(&public)).unwrap();
        assert!(
            said.contains("sync: gone: adopted away, 2 cloud atoms retired, 0 still held\n"),
            "{said}"
        );
        assert!(!has(&store, 101));
        assert!(!has(&store, 105));
        assert!(has(&store, 106));

        let (url, store) = crate::serve_sync_pack(crate::SyncPack::FailDeletes);
        point_pack(&url);
        *store.lock().unwrap() = vec![
            seeded(101, "fresh", "", &["sync:gone", "scope:cloud"]),
            seeded(105, "old", "", &["sync:gone"]),
        ];
        let said = import(&root, &scope_of(&public)).unwrap();
        assert!(
            said.contains("0 cloud atoms retired, 2 still held"),
            "{said}"
        );
        assert!(has(&store, 101));
        assert!(has(&store, 105));
        let again = import(&root, &scope_of(&public)).unwrap();
        assert!(again.contains("still held"), "{again}");
        assert!(has(&store, 101));
    }

    #[test]
    fn a_readded_log_is_imported_instead_of_retired() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let public = make_key();
        let root = tracker(dir.path());
        let rel = ".ljos/atoms/gone.jsonl.age";
        let plain = "{\"text\":\"back\",\"kind\":\"lesson\",\"entities\":[]}\n";
        seal(plain, std::slice::from_ref(&public), &root.join(rel)).unwrap();
        commit_dated(&root, &[rel], "log", None);
        let out = git(&root, &["rm", "-q", "--", rel]).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = git(
            &root,
            &["commit", "-q", "-m", "chore(sync): adopt gone cloud atoms"],
        )
        .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        seal(plain, std::slice::from_ref(&public), &root.join(rel)).unwrap();
        commit_dated(&root, &[rel], "bring the log back", None);
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        let said = import(&root, &scope_of(&public)).unwrap();
        assert!(said.contains("1 cloud atoms taken"), "{said}");
        assert!(!said.contains("adopted away"), "{said}");
        let live = store.lock().unwrap().clone();
        assert_eq!(live.len(), 1, "{live:?}");
        assert!(live[0]["entities"].to_string().contains("sync:gone"));
    }

    #[test]
    fn an_import_with_nothing_to_retire_still_says_there_are_no_logs() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let public = make_key();
        let root = tracker(dir.path());
        adopt_deletion(&root);
        if root.join(".ljos").exists() {
            std::fs::remove_dir_all(root.join(".ljos")).unwrap();
        }
        let (url, _store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        let said = import(&root, &scope_of(&public)).unwrap();
        assert!(said.contains("no cloud logs yet"), "{said}");
        assert!(!said.contains("adopted away"), "{said}");
    }

    fn same_words(issue: &str) -> Value {
        serde_json::json!({
            "text": "same words", "kind": "lesson", "issue": issue, "entities": []
        })
    }

    fn peer_log(dir: &Path, rows: &[Value]) -> (PathBuf, Scope) {
        assert_ne!(host(), "peer", "the log's host would be this machine");
        let public = make_key();
        let root = tracker(dir);
        let plain: String = rows.iter().map(|row| format!("{row}\n")).collect();
        let rel = ".ljos/atoms/peer.jsonl.age";
        seal(&plain, std::slice::from_ref(&public), &root.join(rel)).unwrap();
        commit_dated(&root, &[rel], "log", None);
        (root, scope_of(&public))
    }

    /// The same words on two tickets are two memories. A log that moved
    /// them to another ticket is taken, and the copy of the ticket it
    /// dropped is retired.
    #[test]
    fn an_import_keys_each_text_by_its_ticket() {
        let same: BTreeSet<String> = [crate::work_id("same words")].into();
        let row = format!("{}\n", same_words("proj-b"));
        let got = incoming(&row, "peer", &same, &same);
        assert_eq!(got.post.len(), 1, "the other ticket was taken as held");
        assert_eq!(got.retired.len(), 1, "the dropped ticket was not retired");

        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let (root, scope) = peer_log(dir.path(), &[same_words("proj-b")]);
        write_taken(
            &root,
            "cloud",
            "peer",
            &format!("{}\n", same_words("proj-a")),
        )
        .unwrap();
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        *store.lock().unwrap() = vec![seeded(201, "same words", "proj-a", &["sync:peer"])];
        let said = import(&root, &scope).unwrap();
        assert!(
            said.contains("sync: peer: 1 cloud atoms taken, 0 refused, 1 retired\n"),
            "{said}"
        );
        let live = store.lock().unwrap().clone();
        assert_eq!(live.len(), 1, "{live:?}");
        assert_eq!(live[0]["issue"], "proj-b", "{live:?}");
    }

    /// A `.taken` written before the key carried the ticket names texts.
    /// The first import after the change posts nothing again, retires the
    /// text the host dropped, and rewrites the file by memory.
    #[test]
    fn an_import_reads_a_taken_file_of_texts() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let row = serde_json::json!({
            "text": "kept", "kind": "lesson", "issue": "proj-a", "entities": []
        });
        let (root, scope) = peer_log(dir.path(), std::slice::from_ref(&row));
        let taken = taken_path(&root, "cloud", "peer");
        std::fs::create_dir_all(taken.parent().unwrap()).unwrap();
        let texts = format!(
            "{}\n{}\n",
            crate::work_id("kept"),
            crate::work_id("dropped")
        );
        std::fs::write(&taken, texts).unwrap();
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        *store.lock().unwrap() = vec![
            seeded(201, "kept", "proj-a", &["sync:peer"]),
            seeded(202, "dropped", "proj-a", &["sync:peer"]),
        ];
        let said = import(&root, &scope).unwrap();
        assert!(
            said.contains("sync: peer: 0 cloud atoms taken, 0 refused, 1 retired\n"),
            "{said}"
        );
        assert!(has(&store, 201));
        assert!(!has(&store, 202));
        let rewritten = std::fs::read_to_string(&taken).unwrap();
        assert!(
            rewritten.contains(&memory_key(&row).unwrap()),
            "{rewritten}"
        );
        assert!(!rewritten.contains(&crate::work_id("kept")), "{rewritten}");
        let again = import(&root, &scope).unwrap();
        assert!(
            again.contains("sync: peer: 0 cloud atoms taken, 0 refused, 0 retired\n"),
            "{again}"
        );
        assert!(has(&store, 201));
    }

    /// Primary `box` plus an extra `cloud` log. Both are adopted in one
    /// commit, each atom stays in its own scope, and a later import
    /// retires `sync:gone` and `sync:gone+cloud` separately.
    fn two_scope_logs(dir: &Path) -> (PathBuf, Scope) {
        let public = make_key();
        let root = tracker(dir);
        let text = format!(
            "name = \"box\"\nrecipients = [\"{public}\"]\n\n[scopes]\ncloud = [\"{public}\"]\n"
        );
        let scope: Scope = toml::from_str(&text).unwrap();
        std::fs::write(root.join(".ljos/sync.toml"), &text).unwrap();
        std::fs::write(
            dir.join("config/ljos/sync.toml"),
            "default_scope = \"private\"\n",
        )
        .unwrap();
        seal(
            "{\"text\":\"box-live\",\"kind\":\"lesson\",\"entities\":[\"scope:box\"]}\n",
            std::slice::from_ref(&public),
            &root.join(".ljos/atoms/gone.jsonl.age"),
        )
        .unwrap();
        seal(
            "{\"text\":\"cloud-live\",\"kind\":\"lesson\",\"entities\":[\"scope:cloud\"]}\n",
            std::slice::from_ref(&public),
            &root.join(".ljos/atoms/gone+cloud.jsonl.age"),
        )
        .unwrap();
        std::fs::write(root.join(".ljos/atoms/gone.digest"), "d\n").unwrap();
        std::fs::write(root.join(".ljos/atoms/gone+cloud.digest"), "d\n").unwrap();
        commit_dated(
            &root,
            &[
                ".ljos/sync.toml",
                ".ljos/atoms/gone.jsonl.age",
                ".ljos/atoms/gone.digest",
                ".ljos/atoms/gone+cloud.jsonl.age",
                ".ljos/atoms/gone+cloud.digest",
            ],
            "logs",
            Some(OLD_COMMIT),
        );
        let taken = taken_path(&root, "cloud", "gone");
        std::fs::create_dir_all(taken.parent().unwrap()).unwrap();
        std::fs::write(&taken, format!("{}\n", crate::work_id("cloud-old"))).unwrap();
        (root, scope)
    }

    #[test]
    fn an_adopt_takes_every_scope_log_of_the_host() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let (root, scope) = two_scope_logs(dir.path());
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        *store.lock().unwrap() = vec![
            seeded(201, "box-live", "", &["sync:gone", "scope:box"]),
            seeded(202, "cloud-live", "", &["sync:gone+cloud", "scope:cloud"]),
            seeded(203, "cloud-old", "", &["sync:gone+cloud"]),
            seeded(204, "cloud-old", "", &["sync:gone+cloud", "scope:private"]),
            seeded(205, "box-live", "", &["scope:private"]),
        ];
        let said = adopt_in_repo(&root, &scope, "gone", opts(false, false)).unwrap();
        assert!(
            said.starts_with(
                "sync: gone: 1 box atoms adopted, 0 refused, 1 moved, 0 retired\n\
                 sync: gone: 1 cloud atoms adopted, 0 refused, 1 moved, 1 retired\n"
            ),
            "{said}"
        );
        assert!(!has(&store, 201));
        assert!(!has(&store, 202));
        assert!(!has(&store, 203));
        assert!(has(&store, 204), "another scope's copy was forgotten");
        assert!(has(&store, 205), "a private copy was forgotten");
        let live = store.lock().unwrap().clone();
        let posted: Vec<&Value> = live
            .iter()
            .filter(|atom| atom["origin"] == "peer")
            .collect();
        assert_eq!(posted.len(), 2, "{live:?}");
        assert!(posted.iter().all(|atom| {
            let tags = atom["entities"].to_string();
            !tags.contains("sync:")
        }));
        let box_log = log_plaintext(&live, "box", "private");
        assert!(box_log.contains("box-live"), "{box_log}");
        assert!(!box_log.contains("cloud-live"), "{box_log}");
        let cloud_log = log_plaintext(&live, "cloud", "private");
        assert!(cloud_log.contains("cloud-live"), "{cloud_log}");
        assert!(!cloud_log.contains("box-live"), "{cloud_log}");
        assert!(!in_head(&root, ".ljos/atoms/gone.jsonl.age"));
        assert!(!in_head(&root, ".ljos/atoms/gone+cloud.jsonl.age"));
        assert!(!in_head(&root, ".ljos/atoms/gone.digest"));
        assert!(!in_head(&root, ".ljos/atoms/gone+cloud.digest"));
        assert!(!taken_path(&root, "cloud", "gone").exists());
        let subject = git(&root, &["log", "-1", "--format=%s"]).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&subject.stdout).trim(),
            "chore(sync): adopt gone box atoms"
        );
        let names = git(
            &root,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
        )
        .unwrap();
        let names = String::from_utf8_lossy(&names.stdout);
        assert!(names.contains("gone.jsonl.age"), "{names}");
        assert!(names.contains("gone+cloud.jsonl.age"), "{names}");

        *store.lock().unwrap() = vec![
            seeded(301, "box-live", "", &["sync:gone", "scope:box"]),
            seeded(302, "cloud-live", "", &["sync:gone+cloud", "scope:cloud"]),
            seeded(303, "cloud-live", "", &["sync:gone+cloud"]),
            seeded(304, "cloud-live", "", &["sync:gone+cloud", "scope:private"]),
        ];
        let said = import(&root, &scope).unwrap();
        assert!(
            said.contains("sync: gone: adopted away, 1 box atoms retired, 0 still held\n"),
            "{said}"
        );
        assert!(
            said.contains("sync: gone: adopted away, 2 cloud atoms retired, 0 still held\n"),
            "{said}"
        );
        assert!(!has(&store, 301));
        assert!(!has(&store, 302));
        assert!(!has(&store, 303));
        assert!(has(&store, 304));
    }

    #[test]
    fn a_refused_extra_scope_keeps_every_log() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let (root, scope) = two_scope_logs(dir.path());
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::RefusePosts);
        point_pack(&url);
        *store.lock().unwrap() = vec![
            seeded(201, "box-live", "", &["sync:gone", "scope:box"]),
            seeded(202, "cloud-live", "", &["sync:gone+cloud", "scope:cloud"]),
        ];
        let said = adopt_in_repo(&root, &scope, "gone", opts(false, false)).unwrap();
        assert!(said.contains("0 box atoms adopted, 1 refused"), "{said}");
        assert!(said.contains("0 cloud atoms adopted, 1 refused"), "{said}");
        assert_eq!(said.matches("the log was kept").count(), 2, "{said}");
        assert!(in_head(&root, ".ljos/atoms/gone.jsonl.age"));
        assert!(in_head(&root, ".ljos/atoms/gone+cloud.jsonl.age"));
        let live = store.lock().unwrap().clone();
        assert!(live.iter().all(|atom| atom["origin"] != "peer"), "{live:?}");
    }

    #[test]
    fn a_young_extra_scope_blocks_the_adopt() {
        let dir = tempfile::tempdir().unwrap();
        let _env = begin(dir.path());
        let (root, scope) = two_scope_logs(dir.path());
        let public = Command::new("age-keygen")
            .arg("-y")
            .arg(identity_path())
            .output()
            .unwrap();
        assert!(public.status.success());
        let public = String::from_utf8_lossy(&public.stdout).trim().to_string();
        seal(
            "{\"text\":\"cloud-live\",\"kind\":\"lesson\",\"entities\":[\"scope:cloud\",\"seat:again\"]}\n",
            std::slice::from_ref(&public),
            &root.join(".ljos/atoms/gone+cloud.jsonl.age"),
        )
        .unwrap();
        std::fs::write(root.join(".ljos/atoms/gone+cloud.digest"), "newer\n").unwrap();
        commit_dated(
            &root,
            &[
                ".ljos/atoms/gone+cloud.jsonl.age",
                ".ljos/atoms/gone+cloud.digest",
            ],
            "cloud again",
            None,
        );
        let err = adopt_in_repo(&root, &scope, "gone", opts(false, true)).unwrap_err();
        assert!(err.to_string().contains("cloud"), "{err}");
        assert!(err.to_string().contains("--force"), "{err}");
        assert!(root.join(".ljos/atoms/gone.jsonl.age").is_file());
        assert!(root.join(".ljos/atoms/gone+cloud.jsonl.age").is_file());
        let (url, store) = crate::serve_sync_pack(crate::SyncPack::Keep);
        point_pack(&url);
        let said = adopt_in_repo(&root, &scope, "gone", opts(true, true)).unwrap();
        assert!(said.contains("drop .ljos/atoms/gone.jsonl.age"), "{said}");
        assert!(
            said.contains("drop .ljos/atoms/gone+cloud.jsonl.age"),
            "{said}"
        );
        assert!(store.lock().unwrap().is_empty(), "a dry run posted");
        assert!(in_head(&root, ".ljos/atoms/gone.jsonl.age"));
        assert!(in_head(&root, ".ljos/atoms/gone+cloud.jsonl.age"));
    }

    /// Finish tags a lesson `scope:<primary>`. A primary named `cloud`
    /// would put that lesson, and every unscoped atom, on the shared log.
    #[test]
    fn a_primary_scope_named_cloud_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".ljos")).unwrap();
        let path = root.join(".ljos/sync.toml");
        std::fs::write(&path, "name = \"cloud\"\nrecipients = [\"age1cloud\"]\n").unwrap();
        let alone = scope_of_repo(root).unwrap_err().to_string();
        assert!(
            alone.contains("primary") && alone.contains("cloud"),
            "{alone}"
        );
        std::fs::write(
            &path,
            "name = \"cloud\"\nrecipients = [\"age1cloud\"]\n[scopes]\nnotes = [\"age1notes\"]\n",
        )
        .unwrap();
        let beside = scope_of_repo(root).unwrap_err().to_string();
        assert!(
            beside.contains("primary") && beside.contains("[scopes]"),
            "{beside}"
        );
        std::fs::write(
            &path,
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let scope = scope_of_repo(root).unwrap().unwrap();
        assert_eq!(scope.name, "grokbot");
        assert_eq!(scope.for_project("garden"), "grokbot");
        assert!(scope.scopes.contains_key(CLOUD_SCOPE));
    }

    #[test]
    fn vm_memory_lands_in_the_cloud_log_and_box_memory_does_not() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".config/ljos")).unwrap();
        // SAFETY: this test holds the process env lock.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
            std::env::set_var(WRITE_SCOPE_KEY, CLOUD_SCOPE);
        }
        assert_eq!(
            scope_for_lesson(Some("grokbot".into())).as_deref(),
            Some(CLOUD_SCOPE),
            "a VM lesson must be tagged cloud, not the repository primary"
        );
        let vm_memory = crate::atom_body("lesson", "The VM kept a fact.", "seat");
        let vm_rule = crate::atom_body("preference", "The VM prefers short logs.", "seat");
        let vm_lesson = crate::atom_body("lesson", "The VM learned a lesson.", "seat");
        for atom in [&vm_memory, &vm_rule, &vm_lesson] {
            let tagged = atom["entities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tag| tag == "scope:cloud");
            assert!(tagged, "vm write missed scope:cloud: {atom}");
        }
        // SAFETY: this test holds the process env lock.
        unsafe { std::env::remove_var(WRITE_SCOPE_KEY) };
        assert_eq!(
            scope_for_lesson(Some("grokbot".into())).as_deref(),
            Some("grokbot")
        );
        let box_memory = crate::atom_body("lesson", "The box kept a fact.", "seat");
        assert!(
            !box_memory["entities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tag| { tag.as_str().is_some_and(|text| text.starts_with("scope:")) }),
            "box memory was scoped: {box_memory}"
        );
        std::fs::write(home.join(".config/ljos/env"), "LJOS_WRITE_SCOPE=cloud\n").unwrap();
        assert_eq!(machine_write_scope().as_deref(), Some(CLOUD_SCOPE));

        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let said = with_fake_age(dir.path(), || {
            export_all(&root, &scope, &[vm_memory, vm_rule, vm_lesson, box_memory])
        })
        .unwrap();
        assert!(!said.contains("default_scope"), "{said}");
        let host = host();
        let [cloud_rel, _] = host_log_rels_for_scope(&host, "grokbot", "cloud");
        let cloud = std::fs::read_to_string(root.join(cloud_rel)).unwrap();
        assert!(cloud.contains("The VM kept a fact."), "{cloud}");
        assert!(cloud.contains("The VM prefers short logs."), "{cloud}");
        assert!(cloud.contains("The VM learned a lesson."), "{cloud}");
        assert!(!cloud.contains("The box kept a fact."), "{cloud}");
        let [primary_rel, _] = host_log_rels_for_scope(&host, "grokbot", "grokbot");
        let primary = std::fs::read_to_string(root.join(primary_rel)).unwrap();
        assert!(primary.contains("The box kept a fact."), "{primary}");
        assert!(!primary.contains("The VM kept a fact."), "{primary}");
        assert!(!primary.contains("The VM learned a lesson."), "{primary}");
        assert!(!primary.contains("The VM prefers short logs."), "{primary}");
    }

    #[test]
    fn a_scope_name_is_lowercase_words_and_digits() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".ljos")).unwrap();
        let path = root.join(".ljos/sync.toml");
        let refused = [
            "name = \"\"\nrecipients = []\n",
            "name = \"Cloud\"\nrecipients = []\n",
            "name = \"a/b\"\nrecipients = []\n",
            "name = \"a+b\"\nrecipients = []\n",
            "name = \"grokbot\"\nrecipients = []\n[scopes]\n\"a/b\" = []\n",
            "name = \"grokbot\"\nrecipients = []\n[scopes]\n\"a+b\" = []\n",
            "name = \"grokbot\"\nrecipients = []\n[projects]\ntools = \"Shared\"\n",
        ];
        for text in refused {
            std::fs::write(&path, text).unwrap();
            let err = scope_of_repo(root).unwrap_err().to_string();
            assert!(
                err.contains("[a-z0-9-]"),
                "expected a name refusal for {text:?}, got {err}"
            );
        }
        std::fs::write(
            &path,
            "name = \"grok-bot\"\nrecipients = []\n[scopes]\ncloud = []\nnotes-2 = []\n",
        )
        .unwrap();
        let scope = scope_of_repo(root).unwrap().unwrap();
        assert_eq!(scope.name, "grok-bot");
        assert!(scope.scopes.contains_key("cloud"));
        assert!(scope.scopes.contains_key("notes-2"));
    }

    /// An extra scope used to inherit `default_scope`, so `default_scope =
    /// "cloud"` sealed an unscoped box atom into `<host>+cloud`.
    #[test]
    fn an_unscoped_atom_is_not_sealed_into_an_extra_scope() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        // SAFETY: this test holds the process env lock.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let atoms = vec![
            atom("a", "box only", &[]),
            atom("b", "cloud only", &["scope:cloud"]),
            atom("c", "named box", &["scope:grokbot"]),
            atom("d", "lesson on the primary", &["scope:grokbot"]),
        ];
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let said = with_fake_age(dir.path(), || export_all(&root, &scope, &atoms)).unwrap();
        assert!(!said.contains("default_scope"), "{said}");
        let host = host();
        let [cloud_rel, _] = host_log_rels_for_scope(&host, "grokbot", "cloud");
        let cloud = std::fs::read_to_string(root.join(cloud_rel)).unwrap();
        assert!(cloud.contains("cloud only"), "{cloud}");
        assert!(!cloud.contains("box only"), "{cloud}");
        assert!(!cloud.contains("named box"), "{cloud}");
        assert!(!cloud.contains("lesson on the primary"), "{cloud}");
        let [primary_rel, _] = host_log_rels_for_scope(&host, "grokbot", "grokbot");
        let primary = std::fs::read_to_string(root.join(primary_rel)).unwrap();
        assert!(primary.contains("box only"), "{primary}");
        assert!(primary.contains("named box"), "{primary}");
        assert!(primary.contains("lesson on the primary"), "{primary}");
        assert!(!primary.contains("cloud only"), "{primary}");
    }

    #[test]
    fn a_default_scope_naming_an_extra_scope_is_refused() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        std::fs::write(config.join("ljos/sync.toml"), "default_scope = \"cloud\"\n").unwrap();
        // SAFETY: this test holds the process env lock.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let err = export_all(dir.path(), &scope, &[atom("a", "box only", &[])])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("default_scope") && err.contains("cloud"),
            "{err}"
        );
        let named = export_named(dir.path(), &scope, "cloud", &[atom("a", "box only", &[])])
            .unwrap_err()
            .to_string();
        assert!(named.contains("default_scope"), "{named}");
        assert!(!dir.path().join(".ljos").join("atoms").exists());
    }

    #[test]
    fn an_export_deletes_a_log_for_a_removed_scope() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        // SAFETY: this test holds the process env lock.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let root = dir.path().join("repo");
        let me = host();
        let atoms = root.join(".ljos/atoms");
        std::fs::create_dir_all(&atoms).unwrap();
        let stale = atoms.join(format!("{me}+notes.jsonl.age"));
        let stale_digest = atoms.join(format!("{me}+notes.digest"));
        std::fs::write(&stale, "old\n").unwrap();
        std::fs::write(&stale_digest, "old\n").unwrap();
        let theirs = atoms.join("peer+notes.jsonl.age");
        std::fs::write(&theirs, "keep\n").unwrap();
        let said = with_fake_age(dir.path(), || export_all(&root, &scope, &[])).unwrap();
        assert!(!stale.exists(), "own stale log remains\n{said}");
        assert!(!stale_digest.exists(), "own stale digest remains");
        assert!(theirs.exists(), "another host's log was deleted");
    }

    /// `[scopes] cloud = []` used to fail `export_all` after the primary
    /// log was sealed, so `sync_one` never reached the commit.
    #[test]
    fn an_empty_extra_scope_is_skipped_and_the_primary_still_commits() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        // SAFETY: this test holds the process env lock.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "seat@example.invalid"]);
        git(&["config", "user.name", "seat"]);
        git(&["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(root.join("README"), "seed\n").unwrap();
        git(&["add", "README"]);
        git(&["commit", "-q", "-m", "seed"]);
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = []\n",
        )
        .unwrap();
        let atoms = vec![atom("a", "box only", &["scope:grokbot"])];
        let said = with_fake_age(dir.path(), || export_all(&root, &scope, &atoms)).unwrap();
        assert!(
            said.contains("scope `cloud` names no recipients; skipped"),
            "{said}"
        );
        let me = host();
        let [primary_rel, _] = host_log_rels_for_scope(&me, "grokbot", "grokbot");
        let [cloud_rel, _] = host_log_rels_for_scope(&me, "grokbot", "cloud");
        let primary = std::fs::read_to_string(root.join(&primary_rel)).unwrap();
        assert!(primary.contains("box only"), "{primary}");
        assert!(
            !root.join(&cloud_rel).exists(),
            "an empty extra scope was sealed"
        );
        let committed = commit_if_sealed_scopes(&root, &scope, &said, false);
        assert!(committed.contains("committed"), "{committed}");
        let tracked = git(&["ls-files"]);
        assert!(tracked.contains(&format!("{me}.jsonl.age")), "{tracked}");
        assert!(!tracked.contains("+cloud"), "{tracked}");
    }

    #[test]
    fn an_empty_primary_scope_still_refuses_to_export() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        // SAFETY: this test holds the process env lock.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = []\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let err = export_all(&root, &scope, &[atom("a", "box only", &[])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("no recipients"), "{err}");
        let [cloud_rel, _] = host_log_rels_for_scope(&host(), "grokbot", "cloud");
        assert!(
            !root.join(cloud_rel).exists(),
            "the extra scope was sealed after the primary refused"
        );
    }

    #[test]
    fn commit_logs_commits_each_scope_and_drops_a_removed_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "seat@example.invalid"]);
        git(&["config", "user.name", "seat"]);
        git(&["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(root.join("README"), "seed\n").unwrap();
        git(&["add", "README"]);
        git(&["commit", "-q", "-m", "seed"]);
        let me = host();
        let atoms = root.join(".ljos/atoms");
        std::fs::create_dir_all(&atoms).unwrap();
        let notes = format!(".ljos/atoms/{me}+notes.jsonl.age");
        let notes_digest = format!(".ljos/atoms/{me}+notes.digest");
        std::fs::write(root.join(&notes), "old\n").unwrap();
        std::fs::write(root.join(&notes_digest), "old\n").unwrap();
        git(&["add", &notes, &notes_digest]);
        git(&["commit", "-q", "-m", "notes"]);
        for rel in [
            format!(".ljos/atoms/{me}.jsonl.age"),
            format!(".ljos/atoms/{me}.digest"),
            format!(".ljos/atoms/{me}+cloud.jsonl.age"),
            format!(".ljos/atoms/{me}+cloud.digest"),
        ] {
            std::fs::write(root.join(&rel), "sealed\n").unwrap();
        }
        let scope: Scope = toml::from_str(
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let said = commit_logs(root, &scope, false);
        assert!(said.contains("committed"), "{said}");
        let tracked = git(&["ls-files"]);
        assert!(tracked.contains(&format!("{me}.jsonl.age")), "{tracked}");
        assert!(
            tracked.contains(&format!("{me}+cloud.jsonl.age")),
            "{tracked}"
        );
        assert!(tracked.contains(&format!("{me}+cloud.digest")), "{tracked}");
        assert!(!tracked.contains("notes"), "{tracked}\n{said}");
    }

    #[test]
    fn a_removed_scope_log_is_not_imported() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        std::fs::write(root.join(".ljos/atoms/peer+notes.jsonl.age"), "not-age\n").unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let scope = scope_of_repo(root).unwrap().unwrap();
        let (jobs, out) = import_jobs(root, &scope, "this-host").unwrap();
        assert!(out.is_empty(), "{out}");
        assert_eq!(jobs.len(), 1, "the removed scope was skipped");
        assert_eq!(jobs[0].host, "peer");
        assert_eq!(jobs[0].scope_name, "notes");
        assert!(
            jobs[0].plain.is_empty(),
            "a removed scope was decrypted and imported"
        );
    }

    #[test]
    fn an_import_retires_atoms_when_the_extra_scope_is_gone() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let config = home.join(".config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        let pack_home = dir.path().join("pack");
        std::fs::create_dir_all(&pack_home).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut daemon = Command::new(packsetd_bin())
            .args(["--port", &port.to_string(), "--home"])
            .arg(&pack_home)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("PACKSET_HOME", &pack_home)
            .env("PACKSET_AUTH", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(daemon.try_wait().unwrap().is_none(), "packsetd exited");
            assert!(
                std::time::Instant::now() < deadline,
                "packsetd did not listen"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        struct Kill(std::process::Child);
        impl Drop for Kill {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let _daemon = Kill(daemon);
        // SAFETY: this test holds the process env lock.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config);
            std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));
            std::env::set_var("PACKSET_URL", format!("http://127.0.0.1:{port}"));
            std::env::set_var("PACKSET_AUTH", "off");
            std::env::set_var("PACKSET_HOME", &pack_home);
            std::env::set_var("PACKSET_WORKSPACE", "retire-scope");
            std::env::remove_var(WRITE_SCOPE_KEY);
        }
        let key = identity_path();
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(&key)
            .output()
            .unwrap();
        assert!(made.status.success(), "age-keygen must be installed");
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        std::fs::write(root.join(".ljos/atoms/peer+notes.jsonl.age"), "not-age\n").unwrap();
        let client = crate::pack().unwrap();
        let text = "Old notes should retire.";
        let posted = crate::admit::post_kept(
            &client,
            &serde_json::json!({
                "kind": "lesson",
                "level": "explicit",
                "text": text,
                "workspace": client.workspace(),
                "entities": ["sync:peer+notes"],
            }),
        )
        .unwrap();
        assert!(posted["id"].is_string(), "{posted}");
        write_taken(
            &root,
            "notes",
            "peer",
            &format!("{}\n", serde_json::json!({"text": text})),
        )
        .unwrap();
        let scope = scope_of_repo(&root).unwrap().unwrap();
        let said = import(&root, &scope).unwrap();
        assert!(
            said.contains("peer: 0 notes atoms taken, 0 refused, 1 retired"),
            "{said}"
        );
        let left = crate::atoms_lean(&client, &client.workspace()).unwrap();
        assert!(
            !left.iter().any(|atom| atom["text"] == text),
            "the removed scope's atom is still held: {left:?}"
        );
    }

    #[test]
    fn taken_files_are_keyed_by_tracker_as_well_as_scope() {
        let a = taken_path(Path::new("/tmp/tracker-a"), "grokbot", "peer");
        let b = taken_path(Path::new("/tmp/tracker-b"), "grokbot", "peer");
        assert_ne!(a, b, "two trackers shared one taken file");
        assert_eq!(a.file_name().unwrap(), "peer.taken");
        assert_eq!(a.parent().unwrap().file_name().unwrap(), "grokbot");
        assert_ne!(
            a.parent().unwrap().parent().unwrap(),
            b.parent().unwrap().parent().unwrap()
        );
    }

    /// `<host>+cloud.jsonl.age` sorts before `<host>.jsonl.age` because
    /// `+` is before `.`. Import has to read the primary log first.
    #[test]
    fn import_jobs_reads_the_primary_log_before_an_extra_scope() {
        // Another test puts a fake `age` on PATH. Without the lock this
        // test opens that script after its directory is gone.
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
        let public = age_keygen(&identity_path());
        std::fs::create_dir_all(root.join(".ljos")).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            format!(
                "name = \"grokbot\"\nrecipients = [\"{public}\"]\n[scopes]\ncloud = [\"{public}\"]\n"
            ),
        )
        .unwrap();
        let box_line = lesson_line("Kept on the box.", "grokbot");
        let cloud_line = lesson_line("Kept on the cloud.", "cloud");
        seal_for(
            &root.join(".ljos/atoms/peer+cloud.jsonl.age"),
            &public,
            &cloud_line,
        );
        seal_for(&root.join(".ljos/atoms/peer.jsonl.age"), &public, &box_line);
        let scope = scope_of_repo(root).unwrap().unwrap();
        let (jobs, out) = import_jobs(root, &scope, "this-host").unwrap();
        assert!(out.is_empty(), "{out}");
        assert_eq!(jobs.len(), 2);
        assert_eq!(
            jobs[0].scope_name, "grokbot",
            "the extra scope was read first"
        );
        assert!(
            jobs[0].plain.contains("Kept on the box."),
            "{}",
            jobs[0].plain
        );
        assert_eq!(jobs[1].scope_name, "cloud");
        assert!(
            jobs[1].plain.contains("Kept on the cloud."),
            "{}",
            jobs[1].plain
        );
    }

    /// Moving a text between scopes used to drop it for one sync: the pack
    /// answers a duplicate with the claim it holds, and retiring the old
    /// scope afterwards deleted that claim.
    #[test]
    fn a_text_moved_between_scopes_is_kept() {
        let _g = crate::test_env_lock();
        assert_ne!(
            host(),
            "peer",
            "this machine's host would skip the peer logs"
        );
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let config = home.join(".config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        let pack_home = dir.path().join("pack");
        let (port, _daemon) = spawn_pack(&pack_home);
        // SAFETY: this test holds the process env lock.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config);
            std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));
            std::env::set_var("PACKSET_URL", format!("http://127.0.0.1:{port}"));
            std::env::set_var("PACKSET_AUTH", "off");
            std::env::set_var("PACKSET_HOME", &pack_home);
            std::env::set_var("PACKSET_WORKSPACE", "move-scope");
            std::env::remove_var(WRITE_SCOPE_KEY);
        }
        let public = age_keygen(&identity_path());
        let client = crate::pack().unwrap();
        for (from_scope, to_scope, text) in [
            ("grokbot", "cloud", "The note moved to the cloud."),
            ("cloud", "grokbot", "The note moved back to the box."),
        ] {
            let root = dir.path().join(format!("{from_scope}-to-{to_scope}"));
            std::fs::create_dir_all(root.join(".ljos")).unwrap();
            std::fs::write(
                root.join(".ljos/sync.toml"),
                format!(
                    "name = \"grokbot\"\nrecipients = [\"{public}\"]\n[scopes]\ncloud = [\"{public}\"]\n"
                ),
            )
            .unwrap();
            let sender = if from_scope == "grokbot" {
                "sync:peer".to_string()
            } else {
                format!("sync:peer+{from_scope}")
            };
            let posted = crate::admit::post_kept(
                &client,
                &serde_json::json!({
                    "kind": "lesson",
                    "level": "explicit",
                    "text": text,
                    "workspace": client.workspace(),
                    "entities": [sender],
                }),
            )
            .unwrap();
            assert!(
                posted["id"].is_string(),
                "{from_scope}->{to_scope}: {posted}"
            );
            write_taken(
                &root,
                from_scope,
                "peer",
                &format!("{}\n", serde_json::json!({"text": text})),
            )
            .unwrap();
            let scope = scope_of_repo(&root).unwrap().unwrap();
            for name in ["grokbot", "cloud"] {
                let [rel, _] = host_log_rels_for_scope("peer", "grokbot", name);
                let plain = if name == to_scope {
                    lesson_line(text, name)
                } else {
                    String::new()
                };
                seal_for(&root.join(rel), &public, &plain);
            }
            let said = import(&root, &scope).unwrap();
            let expect_sender = if to_scope == "grokbot" {
                "sync:peer".to_string()
            } else {
                format!("sync:peer+{to_scope}")
            };
            let left = crate::atoms_lean(&client, &client.workspace()).unwrap();
            let hits: Vec<_> = left.iter().filter(|atom| atom["text"] == text).collect();
            assert_eq!(
                hits.len(),
                1,
                "{from_scope}->{to_scope} dropped or duplicated the text: {left:?}\n{said}"
            );
            assert!(
                has_entity(hits[0], &expect_sender),
                "{from_scope}->{to_scope} kept the old sender: {:?}\n{said}",
                hits[0]["entities"]
            );
            assert!(
                said.contains(&format!("1 {to_scope} atoms taken")),
                "{from_scope}->{to_scope}: {said}"
            );
            assert!(
                said.contains(&format!("0 {from_scope} atoms taken, 0 refused, 1 retired")),
                "{from_scope}->{to_scope}: {said}"
            );
        }
    }

    /// A live extra-scope log is posted under `sync:<host>+<scope>`, not the
    /// primary sender.
    #[test]
    fn an_extra_scope_log_is_imported_under_its_sender() {
        let _g = crate::test_env_lock();
        assert_ne!(host(), "peer");
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let config = home.join(".config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        let pack_home = dir.path().join("pack");
        let (port, _daemon) = spawn_pack(&pack_home);
        // SAFETY: this test holds the process env lock.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config);
            std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));
            std::env::set_var("PACKSET_URL", format!("http://127.0.0.1:{port}"));
            std::env::set_var("PACKSET_AUTH", "off");
            std::env::set_var("PACKSET_HOME", &pack_home);
            std::env::set_var("PACKSET_WORKSPACE", "import-cloud");
            std::env::remove_var(WRITE_SCOPE_KEY);
        }
        let public = age_keygen(&identity_path());
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(root.join(".ljos")).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            format!(
                "name = \"grokbot\"\nrecipients = [\"{public}\"]\n[scopes]\ncloud = [\"{public}\"]\n"
            ),
        )
        .unwrap();
        let text = "The other host kept this on the cloud.";
        seal_for(
            &root.join(".ljos/atoms/peer+cloud.jsonl.age"),
            &public,
            &lesson_line(text, "cloud"),
        );
        let scope = scope_of_repo(&root).unwrap().unwrap();
        let said = import(&root, &scope).unwrap();
        assert!(
            said.contains("peer: 1 cloud atoms taken, 0 refused, 0 retired"),
            "{said}"
        );
        let client = crate::pack().unwrap();
        let left = crate::atoms_lean(&client, &client.workspace()).unwrap();
        let hits: Vec<_> = left.iter().filter(|atom| atom["text"] == text).collect();
        assert_eq!(hits.len(), 1, "{left:?}");
        assert!(
            has_entity(hits[0], "sync:peer+cloud"),
            "{:?}",
            hits[0]["entities"]
        );
        assert!(
            !has_entity(hits[0], "sync:peer"),
            "{:?}",
            hits[0]["entities"]
        );
    }

    /// A VM holds the cloud identity and not the machine key. The cloud log
    /// is taken. The primary log, sealed to someone else, is not.
    #[test]
    fn a_vm_import_with_only_the_cloud_key_takes_the_cloud_log() {
        let _g = crate::test_env_lock();
        assert_ne!(host(), "peer");
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let config = home.join(".config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        let pack_home = dir.path().join("pack");
        let (port, _daemon) = spawn_pack(&pack_home);
        // SAFETY: this test holds the process env lock.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config);
            std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));
            std::env::set_var("PACKSET_URL", format!("http://127.0.0.1:{port}"));
            std::env::set_var("PACKSET_AUTH", "off");
            std::env::set_var("PACKSET_HOME", &pack_home);
            std::env::set_var("PACKSET_WORKSPACE", "vm-cloud-key");
            std::env::remove_var(WRITE_SCOPE_KEY);
        }
        assert!(!identity_path().exists(), "the machine key was invented");
        let cloud_public = age_keygen(&cloud_identity_path());
        let other_public = age_keygen(&dir.path().join("other.key"));
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(root.join(".ljos")).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            format!(
                "name = \"grokbot\"\nrecipients = [\"{other_public}\"]\n[scopes]\ncloud = [\"{cloud_public}\"]\n"
            ),
        )
        .unwrap();
        let cloud_text = "The VM kept this on the cloud.";
        let box_text = "The box kept this on the primary.";
        seal_for(
            &root.join(".ljos/atoms/peer.jsonl.age"),
            &other_public,
            &lesson_line(box_text, "grokbot"),
        );
        seal_for(
            &root.join(".ljos/atoms/peer+cloud.jsonl.age"),
            &cloud_public,
            &lesson_line(cloud_text, "cloud"),
        );
        let scope = scope_of_repo(&root).unwrap().unwrap();
        let said = import(&root, &scope).unwrap();
        assert!(
            said.contains("peer's grokbot log is not sealed to this machine"),
            "{said}"
        );
        assert!(
            said.contains("peer: 1 cloud atoms taken, 0 refused, 0 retired"),
            "{said}"
        );
        let client = crate::pack().unwrap();
        let left = crate::atoms_lean(&client, &client.workspace()).unwrap();
        assert!(
            left.iter()
                .any(|atom| atom["text"] == cloud_text && has_entity(atom, "sync:peer+cloud")),
            "{left:?}"
        );
        assert!(
            !left.iter().any(|atom| atom["text"] == box_text),
            "the primary log was opened without the machine key: {left:?}"
        );
        assert!(!identity_path().exists(), "import wrote a machine key");
    }

    /// The atoms merge driver re-exports `<host>+cloud` from atoms whose
    /// `scope:` entity is cloud. A grokbot atom and an unscoped atom stay out.
    #[test]
    fn the_merge_driver_reexports_an_extra_scope_without_primary_atoms() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let config = home.join(".config");
        std::fs::create_dir_all(config.join("ljos")).unwrap();
        let pack_home = dir.path().join("pack");
        let (port, _daemon) = spawn_pack(&pack_home);
        // SAFETY: this test holds the process env lock.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &config);
            std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));
            std::env::set_var("PACKSET_URL", format!("http://127.0.0.1:{port}"));
            std::env::set_var("PACKSET_AUTH", "off");
            std::env::set_var("PACKSET_HOME", &pack_home);
            std::env::set_var("PACKSET_WORKSPACE", "merge-cloud");
            std::env::remove_var(WRITE_SCOPE_KEY);
        }
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(root.join(".ljos")).unwrap();
        let git = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-q", "-b", "main"])
            .output()
            .unwrap();
        assert!(
            git.status.success(),
            "{}",
            String::from_utf8_lossy(&git.stderr)
        );
        std::fs::write(
            root.join(".ljos/sync.toml"),
            "name = \"grokbot\"\nrecipients = [\"age1box\"]\n[scopes]\ncloud = [\"age1cloud\"]\n",
        )
        .unwrap();
        let client = crate::pack().unwrap();
        for (text, entities) in [
            ("Box memory stays on the box.", vec!["scope:grokbot"]),
            ("Cloud memory stays on the cloud.", vec!["scope:cloud"]),
            ("An unscoped atom stays on the box.", vec![]),
        ] {
            crate::admit::post_kept(
                &client,
                &serde_json::json!({
                    "kind": "lesson",
                    "level": "explicit",
                    "text": text,
                    "workspace": client.workspace(),
                    "entities": entities,
                }),
            )
            .unwrap();
        }
        let me = host();
        let cloud_rel = format!(".ljos/atoms/{me}+cloud.jsonl.age");
        let primary_rel = format!(".ljos/atoms/{me}.jsonl.age");
        with_fake_age(dir.path(), || {
            let cloud = root.join(&cloud_rel);
            std::fs::create_dir_all(cloud.parent().unwrap()).unwrap();
            std::fs::write(&cloud, "stale\n").unwrap();
            crate::tracker_push::merge_atoms_driver(&cloud, &cloud_rel).unwrap();
            let cloud_text = std::fs::read_to_string(&cloud).unwrap();
            assert!(
                cloud_text.contains("Cloud memory stays on the cloud."),
                "{cloud_text}"
            );
            assert!(
                !cloud_text.contains("Box memory stays on the box."),
                "{cloud_text}"
            );
            assert!(
                !cloud_text.contains("An unscoped atom stays on the box."),
                "{cloud_text}"
            );
            let primary = root.join(&primary_rel);
            std::fs::write(&primary, "stale\n").unwrap();
            crate::tracker_push::merge_atoms_driver(&primary, &primary_rel).unwrap();
            let primary_text = std::fs::read_to_string(&primary).unwrap();
            assert!(
                primary_text.contains("Box memory stays on the box."),
                "{primary_text}"
            );
            assert!(
                primary_text.contains("An unscoped atom stays on the box."),
                "{primary_text}"
            );
            assert!(
                !primary_text.contains("Cloud memory stays on the cloud."),
                "{primary_text}"
            );
        });
    }
}
