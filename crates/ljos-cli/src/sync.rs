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
//! machine wrote itself.

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

/// This machine's short host name.
#[must_use]
pub fn host() -> String {
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

/// Identity files that may open a sealed log.
///
/// The machine key when it is an age identity, then the scope `cloud`
/// identity when `cloud up` wrote one. A machine key that is not an
/// identity is still passed when it is the only file, so `age` reports
/// the broken key. A cloud file that is not an identity is left out, so
/// it cannot make `age` refuse the machine key.
fn identities_for_open() -> Vec<PathBuf> {
    let own = identity_path();
    let own_ok = read_identity(&own).is_some_and(|text| age_identity(&text));
    let cloud = cloud_identity_path();
    let cloud_ok = read_identity(&cloud).is_some_and(|text| age_identity(&text));
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
        let made = Command::new("age-keygen")
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
    let out = Command::new("age-keygen")
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
/// `explicit_only` is how an extra scope is sealed. Only an atom whose
/// `scope:` entity names that scope is included, so a default cannot pull
/// an unscoped atom into the shared log.
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
        if scope.scopes.contains_key(scope_name) {
            return Some((host.to_string(), scope_name.to_string()));
        }
        return None;
    }
    Some((stem.to_string(), scope.name.clone()))
}

/// Seal `plain` to the recipients into `out`.
fn seal(plain: &str, recipients: &[String], out: &Path) -> Result<()> {
    if recipients.is_empty() {
        bail!("sync: the scope names no recipients; add this machine's `ljos sync --key` to .ljos/sync.toml");
    }
    let mut cmd = Command::new("age");
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

/// Open a sealed log with one identity file. None when that identity is
/// not a recipient.
#[cfg(test)]
fn open(path: &Path, identity: &Path) -> Result<Option<String>> {
    open_identities(path, &[identity.to_path_buf()])
}

/// Open a sealed log with every identity in `identities`. None when none
/// of them is a recipient.
fn open_identities(path: &Path, identities: &[PathBuf]) -> Result<Option<String>> {
    if identities.is_empty() {
        return Ok(None);
    }
    let mut cmd = Command::new("age");
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
    crate::on_path("age")
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
    if !age_on_path() {
        return Ok(age_missing_line());
    }
    let mut out = String::new();
    for named_scope in scope.all_scopes() {
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

/// What one host's log asks of this pack: the atoms to post, and the texts
/// it sent last time and sends no longer.
#[derive(Debug, Default, PartialEq)]
pub struct Incoming {
    pub post: Vec<Value>,
    pub retired: Vec<String>,
}

/// What adopting one gone host's log asks of this pack: which texts are
/// still live there, and which atoms this host should post as its own.
#[derive(Debug, Default, PartialEq)]
struct AdoptPlan {
    live: BTreeSet<String>,
    post: Vec<Value>,
}

/// The atoms to adopt from one gone host log: one live text once, and never
/// a text this host already holds as its own.
#[must_use]
fn adopt_plan(plain: &str, own: &BTreeSet<String>) -> AdoptPlan {
    let mut plan = AdoptPlan::default();
    for line in plain.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(mut atom) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let text = atom["text"].as_str().unwrap_or("").to_string();
        if text.is_empty() {
            continue;
        }
        let id = crate::work_id(&text);
        if !plan.live.insert(id.clone()) || own.contains(&id) {
            continue;
        }
        if let Some(map) = atom.as_object_mut() {
            map.remove("id");
            let entities: Vec<Value> = map
                .get("entities")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|e| !e.as_str().is_some_and(|tag| tag.starts_with("sync:")))
                .cloned()
                .collect();
            map.insert("entities".into(), Value::Array(entities));
        }
        plan.post.push(atom);
    }
    plan
}

/// Read one host's log against the texts taken from it last time
/// (`before`, for what it retired) and the texts from it this pack
/// already holds (`held`, which are not posted again).
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
        let text = atom["text"].as_str().unwrap_or("").to_string();
        if text.is_empty() {
            continue;
        }
        let id = crate::work_id(&text);
        now.insert(id.clone());
        if held.contains(&id) {
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

fn read_taken(root: &Path, scope: &str, host: &str) -> BTreeSet<String> {
    std::fs::read_to_string(taken_path(root, scope, host))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

fn write_taken(root: &Path, scope: &str, host: &str, plain: &str) -> Result<()> {
    let path = taken_path(root, scope, host);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let ids: BTreeSet<String> = plain
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|a| a["text"].as_str().map(crate::work_id))
        .collect();
    std::fs::write(path, ids.into_iter().map(|i| i + "\n").collect::<String>())?;
    Ok(())
}

fn sender_entity_for_scope(host: &str, scope: &Scope, scope_name: &str) -> String {
    if scope_name == scope.name {
        sender_entity(host)
    } else {
        format!("sync:{host}+{scope_name}")
    }
}

/// Import every other machine's log of the repository's scope into this
/// seat's pack. Returns one line per host.
///
/// # Errors
///
/// The pack not answering, or a log this machine should open failing to.
pub fn import(root: &Path, scope: &Scope) -> Result<String> {
    let me = host();
    let identities = identities_for_open();
    if identities.is_empty() {
        return Ok("sync: no age identity on this machine; `ljos sync --key` makes one\n".into());
    }
    let dir = log_dir(root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(format!("sync: no {} logs yet\n", scope.name));
    };
    let client = crate::pack()?;
    let mut out = String::new();
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".jsonl.age"))
        .collect();
    paths.sort();
    for path in paths {
        let Some((host, scope_name)) = host_scope_from_log_path(&path, scope) else {
            continue;
        };
        if host.is_empty() || host == me {
            continue;
        }
        let Some(plain) = open_identities(&path, &identities)? else {
            out.push_str(&format!(
                "sync: {host}'s {} log is not sealed to this machine\n",
                scope_name
            ));
            continue;
        };
        let workspace = client.workspace();
        let sender = sender_entity_for_scope(&host, scope, &scope_name);
        // The pack names a posted atom afresh, so a text already taken
        // from this host would be held twice; what is held is skipped.
        let held: BTreeSet<String> = crate::atoms_lean(&client, &workspace)?
            .iter()
            .filter(|a| {
                a["entities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|e| e.as_str() == Some(sender.as_str()))
            })
            .filter_map(|a| a["text"].as_str().map(crate::work_id))
            .collect();
        let got = incoming_with_sender(
            &plain,
            &sender,
            &read_taken(root, &scope_name, &host),
            &held,
        );
        let (mut kept, mut refused) = (0usize, 0usize);
        for mut atom in got.post {
            if let Some(map) = atom.as_object_mut() {
                map.insert("workspace".into(), Value::String(workspace.clone()));
            }
            match crate::admit::post_kept(&client, &atom) {
                Ok(_) => kept += 1,
                Err(_) => refused += 1,
            }
        }
        let mut retired = 0usize;
        if !got.retired.is_empty() {
            for atom in crate::atoms_lean(&client, &workspace)? {
                let text = atom["text"].as_str().unwrap_or("");
                let from_host = atom["entities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|e| e.as_str() == Some(sender.as_str()));
                if from_host && got.retired.contains(&crate::work_id(text)) {
                    if let Some(id) = atom["id"].as_str() {
                        if crate::packset_forget(id, None).is_ok() {
                            retired += 1;
                        }
                    }
                }
            }
        }
        write_taken(root, &scope_name, &host, &plain)?;
        out.push_str(&format!(
            "sync: {host}: {kept} {} atoms taken, {refused} refused, {retired} retired\n",
            scope_name
        ));
    }
    Ok(out)
}

fn tracked_path(root: &Path, rel: &str) -> bool {
    git(root, &["ls-files", "--error-unmatch", "--", rel]).is_ok_and(|o| o.status.success())
}

fn stageable_path(root: &Path, rel: &str) -> bool {
    root.join(rel).exists() || tracked_path(root, rel)
}

/// One gone host's log for a scope this repository still configures.
struct GoneLog {
    scope_name: String,
    log_rel: String,
    digest_rel: String,
    plain: String,
}

fn adopt_in_repo(root: &Path, scope: &Scope, gone: &str, push: bool) -> Result<String> {
    let me = host();
    if gone == me {
        bail!("sync: {gone} is this machine; pass a gone host");
    }
    let identities = identities_for_open();
    if identities.is_empty() {
        return Ok("sync: no age identity on this machine; `ljos sync --key` makes one\n".into());
    }
    let names: Vec<String> = scope
        .all_scopes()
        .into_iter()
        .map(|named| named.name.to_string())
        .collect();
    let mut present = Vec::new();
    for name in &names {
        let [log_rel, digest_rel] = host_log_rels_for_scope(gone, &scope.name, name);
        if root.join(&log_rel).is_file() {
            present.push((name.clone(), log_rel, digest_rel));
        }
    }
    if present.is_empty() {
        return Ok(format!(
            "sync: {gone} has no {} log in this tracker\n",
            scope.name
        ));
    }
    let mut opened = Vec::new();
    let mut out = String::new();
    for (name, log_rel, digest_rel) in present {
        match open_identities(&root.join(&log_rel), &identities)? {
            Some(plain) => opened.push(GoneLog {
                scope_name: name,
                log_rel,
                digest_rel,
                plain,
            }),
            None => out.push_str(&format!(
                "sync: {gone}'s {name} log is not sealed to this machine\n"
            )),
        }
    }
    if opened.is_empty() {
        return Ok(out);
    }
    let client = crate::pack()?;
    let workspace = client.workspace();
    let before = crate::atoms_lean(&client, &workspace)?;
    let mut own: BTreeSet<String> = before
        .iter()
        .filter(|a| is_own(a))
        .filter_map(|a| a["text"].as_str().map(crate::work_id))
        .collect();
    for log in &opened {
        let plan = adopt_plan(&log.plain, &own);
        let mut adopted = BTreeSet::new();
        let (mut kept, mut refused) = (0usize, 0usize);
        for mut atom in plan.post {
            let text = atom["text"].as_str().unwrap_or("");
            let id = crate::work_id(text);
            if let Some(map) = atom.as_object_mut() {
                map.insert("workspace".into(), Value::String(workspace.clone()));
            }
            match crate::admit::post_kept(&client, &atom) {
                Ok(_) => {
                    own.insert(id.clone());
                    adopted.insert(id);
                    kept += 1;
                }
                Err(_) => refused += 1,
            }
        }
        let tag = sender_entity_for_scope(gone, scope, &log.scope_name);
        let mut moved = 0usize;
        for atom in &before {
            let Some(text) = atom["text"].as_str() else {
                continue;
            };
            let from_gone = atom["entities"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|e| e.as_str() == Some(tag.as_str()));
            if !from_gone {
                continue;
            }
            let text_id = crate::work_id(text);
            if !plan.live.contains(&text_id) {
                continue;
            }
            if !(own.contains(&text_id) || adopted.contains(&text_id)) {
                continue;
            }
            if let Some(id) = atom["id"].as_str() {
                if crate::packset_forget(id, None).is_ok() {
                    moved += 1;
                }
            }
        }
        out.push_str(&format!(
            "sync: {gone}: {kept} {} atoms adopted, {refused} refused, {moved} moved\n",
            log.scope_name
        ));
    }
    let atoms = crate::atoms_lean(&client, &workspace)?;
    let exported = export_all(root, scope, &atoms)?;
    if exported.contains("age is not installed") {
        bail!("sync: age is not installed, so adoption cannot seal {me}'s log; install age and try again");
    }
    let mut stage = Vec::new();
    for log in &opened {
        for rel in [&log.log_rel, &log.digest_rel] {
            if stageable_path(root, rel) {
                stage.push((*rel).clone());
            }
            let path = root.join(rel);
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        let _ = std::fs::remove_file(taken_path(root, &log.scope_name, gone));
    }
    for name in &names {
        for rel in host_log_rels_for_scope(&me, &scope.name, name) {
            if stageable_path(root, &rel) {
                stage.push(rel);
            }
        }
    }
    let committed = commit_adopt(root, &scope.name, gone, &stage, push);
    out.push_str(&exported);
    out.push_str(&committed);
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
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
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
    match crate::commit_paths(root, files, message) {
        crate::PathCommit::Made => {}
        crate::PathCommit::AlreadyIn => {
            drop(held);
            return if push {
                push_pending(root)
            } else {
                kept_local(root)
            };
        }
        crate::PathCommit::Refused(why) => return format!("sync: commit refused: {why}\n"),
    }
    drop(held);
    if !push {
        return format!("sync: committed {message}; not pushed (LJOS_TRACKER_GIT=commit)\n");
    }
    let remotes = git(root, &["remote"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if remotes.is_empty() {
        return format!(
            "sync: committed {message}; no remote is configured, so nothing was pushed. \
             Set one with `git -C {} remote add origin URL` and `git push -u origin HEAD`.\n",
            root.display()
        );
    }
    // No upstream yet: push_checkout sets it with `-u origin <branch>`.
    // A sibling bot's push is rebased, and an org conflict keeps both
    // headings. Failure stays a failure; the line does not say the log was
    // pushed.
    match crate::tracker_push::push_checkout(root) {
        Ok(()) => format!("sync: committed {message}; pushed\n"),
        Err(e) => format!("sync: committed {message}; push refused: {e}\n"),
    }
}

fn commit_adopt(root: &Path, scope: &str, gone: &str, files: &[String], push: bool) -> String {
    if files.is_empty() {
        return String::new();
    }
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let message = format!("chore(sync): adopt {gone} {scope} atoms");
    commit_files(root, &refs, &message, push)
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
    let refs: Vec<String> = scope
        .all_scopes()
        .into_iter()
        .flat_map(|named_scope| host_log_rels_for_scope(&me, &scope.name, named_scope.name))
        .filter(|rel| stageable_path(root, rel))
        .collect();
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
    let Some(up) = crate::tracker_upstream(root) else {
        return String::new();
    };
    let mut refs = vec![up.clone()];
    refs.extend(
        crate::tracker_mirrors(root, &up)
            .unwrap_or_default()
            .into_iter()
            .map(|(r, b)| format!("{r}/{b}")),
    );
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

fn sync_roots() -> Result<Vec<PathBuf>> {
    let listed = local().repos;
    Ok(if listed.is_empty() {
        vec![tracker_root()?]
    } else {
        listed.iter().map(|r| expand(r)).collect()
    })
}

/// Fold one gone host's atoms into this host's log and drop the gone host's
/// log file from each configured tracker repository.
///
/// # Errors
///
/// A missing host name, trying to adopt this machine's own host name, a
/// scope file that does not parse, the pack not answering, or age refusing.
pub fn adopt(target: &str) -> Result<String> {
    let gone = target.trim();
    if gone.is_empty() {
        bail!("sync: pass a host after --adopt");
    }
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
        out.push_str(&catch_up(&root));
        out.push_str(&adopt_in_repo(&root, &scope, gone, true)?);
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
/// it could not: sealed to other machines, or `age` missing.
fn sealed_rows(root: &Path) -> (Vec<crate::user_rules::UserRule>, usize) {
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
    let identities = identities_for_open();
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
    let Some(scope) = scope.filter(|_| mode != TrackerGit::Off) else {
        let (mut rows, _) = sealed_rows(root);
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
    let (mut rows, closed) = sealed_rows(root);
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

    fn atom(id: &str, text: &str, entities: &[&str]) -> Value {
        serde_json::json!({"id": id, "text": text, "kind": "lesson",
            "entities": entities, "embedding": [0.1, 0.2]})
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
    fn an_adopt_plan_posts_each_live_text_once_without_sync_tags() {
        let plain = format!(
            "{}\n{}\n{}\nnot-json\n",
            atom("a", "fresh", &["seat:x", "sync:gone"]),
            atom("b", "mine", &["scope:cloud"]),
            atom("c", "fresh", &["sync:stale"])
        );
        let own: BTreeSet<String> = [crate::work_id("mine")].into_iter().collect();
        let plan = adopt_plan(&plain, &own);
        let live: BTreeSet<String> = [crate::work_id("fresh"), crate::work_id("mine")]
            .into_iter()
            .collect();
        assert_eq!(plan.live, live);
        assert_eq!(plan.post.len(), 1);
        assert_eq!(plan.post[0]["text"], "fresh");
        assert!(plan.post[0].get("id").is_none());
        assert!(plan.post[0]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| !e.as_str().is_some_and(|tag| tag.starts_with("sync:"))));
    }

    /// Adoption used to read only `<host>.jsonl.age`, so a cloud log on the
    /// gone machine was left behind and never copied into this host's log.
    #[test]
    fn an_adopt_takes_every_configured_scope() {
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
        let packsetd = packsetd_bin();
        let mut daemon = Command::new(&packsetd)
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
            assert!(
                daemon.try_wait().unwrap().is_none(),
                "packsetd exited before listening"
            );
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
            std::env::set_var("PACKSET_URL", format!("http://127.0.0.1:{port}"));
            std::env::set_var("PACKSET_AUTH", "off");
            std::env::set_var("PACKSET_HOME", &pack_home);
            std::env::set_var("PACKSET_WORKSPACE", "adopt-scopes");
            std::env::remove_var(WRITE_SCOPE_KEY);
        }
        let key = identity_path();
        let made = Command::new("age-keygen")
            .arg("-o")
            .arg(&key)
            .output()
            .unwrap();
        assert!(made.status.success(), "age-keygen must be installed");
        let public = Command::new("age-keygen")
            .arg("-y")
            .arg(&key)
            .output()
            .unwrap();
        assert!(public.status.success());
        let public = String::from_utf8_lossy(&public.stdout).trim().to_string();
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(root.join(".ljos")).unwrap();
        std::fs::write(
            root.join(".ljos/sync.toml"),
            format!(
                "name = \"grokbot\"\nrecipients = [\"{public}\"]\n[scopes]\ncloud = [\"{public}\"]\n"
            ),
        )
        .unwrap();
        let scope = scope_of_repo(&root).unwrap().unwrap();
        let seal = |rel: &str, text: &str| {
            let path = root.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            let mut child = Command::new("age")
                .args(["-r", &public, "-o"])
                .arg(&path)
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
                .write_all(text.as_bytes())
                .unwrap();
            let done = child.wait_with_output().unwrap();
            assert!(
                done.status.success(),
                "age: {}",
                String::from_utf8_lossy(&done.stderr)
            );
        };
        let line = |text: &str, scope_name: &str| {
            format!(
                "{}\n",
                serde_json::json!({
                    "kind": "lesson",
                    "level": "explicit",
                    "text": text,
                    "entities": [format!("scope:{scope_name}")],
                })
            )
        };
        let [box_rel, _] = host_log_rels_for_scope("oldbox", "grokbot", "grokbot");
        let [cloud_rel, _] = host_log_rels_for_scope("oldbox", "grokbot", "cloud");
        seal(
            &box_rel,
            &line("The gone host kept this on the box.", "grokbot"),
        );
        seal(
            &cloud_rel,
            &line("The gone host kept this on the cloud.", "cloud"),
        );
        let said = adopt_in_repo(&root, &scope, "oldbox", false).unwrap();
        assert!(said.contains("1 grokbot atoms adopted"), "{said}");
        assert!(said.contains("1 cloud atoms adopted"), "{said}");
        assert!(!root.join(&box_rel).exists(), "the gone box log remains");
        assert!(
            !root.join(&cloud_rel).exists(),
            "the gone cloud log remains"
        );
        let me = host();
        let [mine_box, _] = host_log_rels_for_scope(&me, "grokbot", "grokbot");
        let [mine_cloud, _] = host_log_rels_for_scope(&me, "grokbot", "cloud");
        let open_log = |rel: &str| {
            open(&root.join(rel), &key)
                .unwrap()
                .unwrap_or_else(|| panic!("could not open {rel}"))
        };
        let box_text = open_log(&mine_box);
        let cloud_text = open_log(&mine_cloud);
        assert!(box_text.contains("kept this on the box"), "{box_text}");
        assert!(!box_text.contains("kept this on the cloud"), "{box_text}");
        assert!(
            cloud_text.contains("kept this on the cloud"),
            "{cloud_text}"
        );
        assert!(!cloud_text.contains("kept this on the box"), "{cloud_text}");
    }

    #[test]
    fn an_adopt_names_a_missing_scope() {
        let _g = crate::test_env_lock();
        let said = adopt("otherhost").unwrap();
        assert!(said.contains("names no scope"), "{said}");
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

    #[test]
    fn an_atoms_scope_is_its_entity_or_the_default() {
        assert_eq!(
            atom_scope(&atom("a", "t", &["scope:personal"]), "surf"),
            "personal"
        );
        assert_eq!(atom_scope(&atom("a", "t", &["seat:x"]), "surf"), "surf");
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
            open_identities(&sealed, &identities_for_open())
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
            open_identities(&closed, &identities_for_open()).unwrap(),
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
            open_identities(&sealed, &identities_for_open())
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
        assert_eq!(identities_for_open(), vec![own]);
    }
}
