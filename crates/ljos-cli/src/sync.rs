//! Sharing the seat's memory across machines through the tracker repository
//! that already travels between them.
//!
//! Each tracker repository names one scope in `.ljos/sync.toml`: the scope's
//! name and the age recipients allowed to read it. Every machine writes only
//! its own log, `.ljos/atoms/<host>.jsonl.age`, sealed to those recipients,
//! so two machines never touch one file and git never has to merge a log.
//! A log carries the machine's live atoms of that scope without their
//! vectors; the receiving pack encodes them again. An atom's scope is its
//! `scope:NAME` entity, else the machine's default scope.
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
        Ok(text) => Ok(Some(
            toml::from_str(&text).with_context(|| path.display().to_string())?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| path.display().to_string()),
    }
}

fn default_scope(repo_scope: &str) -> String {
    local()
        .default_scope
        .unwrap_or_else(|| repo_scope.to_string())
}

/// The scope an atom belongs to: its `scope:NAME` entity, else `default`.
#[must_use]
pub fn atom_scope(atom: &Value, default: &str) -> String {
    atom["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|e| e.strip_prefix("scope:"))
        .map_or_else(|| default.to_string(), str::to_string)
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
#[must_use]
pub fn log_plaintext(atoms: &[Value], scope: &str, default: &str) -> String {
    let mut mine: Vec<Value> = atoms
        .iter()
        .filter(|a| is_own(a) && atom_scope(a, default) == scope)
        .map(travelling)
        .collect();
    mine.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    mine.iter().map(|a| format!("{a}\n")).collect()
}

fn log_dir(root: &Path) -> PathBuf {
    root.join(".ljos").join("atoms")
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

/// Open a sealed log with this machine's identity; none when this machine
/// is not one of its recipients.
fn open(path: &Path, identity: &Path) -> Result<Option<String>> {
    let out = Command::new("age")
        .arg("-d")
        .arg("-i")
        .arg(identity)
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
    let host = host();
    let plain = log_plaintext(atoms, &scope.name, &default_scope(&scope.name));
    let count = plain.lines().count();
    let dir = log_dir(root);
    std::fs::create_dir_all(&dir)?;
    let stamp = dir.join(format!("{host}.digest"));
    let digest = format!(
        "{} {}\n",
        crate::work_id(&plain),
        crate::work_id(&scope.recipients.join(","))
    );
    if std::fs::read_to_string(&stamp).is_ok_and(|d| d == digest) {
        return Ok(format!(
            "sync: {count} {} atoms unchanged since the last export\n",
            scope.name
        ));
    }
    if !age_on_path() {
        return Ok(age_missing_line());
    }
    if let Err(err) = seal(
        &plain,
        &scope.recipients,
        &dir.join(format!("{host}.jsonl.age")),
    ) {
        if age_missing(&err) {
            return Ok(age_missing_line());
        }
        return Err(err);
    }
    std::fs::write(&stamp, digest)?;
    Ok(format!(
        "sync: exported {count} {} atoms to .ljos/atoms/{host}.jsonl.age, sealed to {} recipient{}\n",
        scope.name,
        scope.recipients.len(),
        if scope.recipients.len() == 1 { "" } else { "s" }
    ))
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
            let tag = sender_entity(host);
            if !entities.iter().any(|e| e.as_str() == Some(tag.as_str())) {
                entities.push(Value::String(tag));
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

fn taken_path(scope: &str, host: &str) -> PathBuf {
    state_dir().join(scope).join(format!("{host}.taken"))
}

fn read_taken(scope: &str, host: &str) -> BTreeSet<String> {
    std::fs::read_to_string(taken_path(scope, host))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

fn write_taken(scope: &str, host: &str, plain: &str) -> Result<()> {
    let path = taken_path(scope, host);
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

/// Import every other machine's log of the repository's scope into this
/// seat's pack. Returns one line per host.
///
/// # Errors
///
/// The pack not answering, or a log this machine should open failing to.
pub fn import(root: &Path, scope: &Scope) -> Result<String> {
    let me = host();
    let identity = identity_path();
    if !identity.exists() {
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
        let host = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".jsonl.age"))
            .unwrap_or("")
            .to_string();
        if host.is_empty() || host == me {
            continue;
        }
        let Some(plain) = open(&path, &identity)? else {
            out.push_str(&format!(
                "sync: {host}'s {} log is not sealed to this machine\n",
                scope.name
            ));
            continue;
        };
        let workspace = client.workspace();
        // The pack names a posted atom afresh, so a text already taken
        // from this host would be held twice; what is held is skipped.
        let tag = sender_entity(&host);
        let held: BTreeSet<String> = crate::atoms_lean(&client, &workspace)?
            .iter()
            .filter(|a| {
                a["entities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|e| e.as_str() == Some(tag.as_str()))
            })
            .filter_map(|a| a["text"].as_str().map(crate::work_id))
            .collect();
        let got = incoming(&plain, &host, &read_taken(&scope.name, &host), &held);
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
                    .any(|e| e.as_str() == Some(tag.as_str()));
                if from_host && got.retired.contains(&crate::work_id(text)) {
                    if let Some(id) = atom["id"].as_str() {
                        if crate::packset_forget(id, None).is_ok() {
                            retired += 1;
                        }
                    }
                }
            }
        }
        write_taken(&scope.name, &host, &plain)?;
        out.push_str(&format!(
            "sync: {host}: {kept} {} atoms taken, {refused} refused, {retired} retired\n",
            scope.name
        ));
    }
    Ok(out)
}

fn host_log_rels(host: &str) -> [String; 2] {
    [
        format!(".ljos/atoms/{host}.jsonl.age"),
        format!(".ljos/atoms/{host}.digest"),
    ]
}

fn tracked_path(root: &Path, rel: &str) -> bool {
    git(root, &["ls-files", "--error-unmatch", "--", rel]).is_ok_and(|o| o.status.success())
}

fn stageable_path(root: &Path, rel: &str) -> bool {
    root.join(rel).exists() || tracked_path(root, rel)
}

fn adopt_in_repo(root: &Path, scope: &Scope, gone: &str, push: bool) -> Result<String> {
    let me = host();
    if gone == me {
        bail!("sync: {gone} is this machine; pass a gone host");
    }
    let identity = identity_path();
    if !identity.exists() {
        return Ok("sync: no age identity on this machine; `ljos sync --key` makes one\n".into());
    }
    let [gone_log, gone_digest] = host_log_rels(gone);
    let log_path = root.join(&gone_log);
    if !log_path.is_file() {
        return Ok(format!(
            "sync: {gone} has no {} log in this tracker\n",
            scope.name
        ));
    }
    let Some(plain) = open(&log_path, &identity)? else {
        return Ok(format!(
            "sync: {gone}'s {} log is not sealed to this machine\n",
            scope.name
        ));
    };
    let client = crate::pack()?;
    let workspace = client.workspace();
    let before = crate::atoms_lean(&client, &workspace)?;
    let own: BTreeSet<String> = before
        .iter()
        .filter(|a| is_own(a))
        .filter_map(|a| a["text"].as_str().map(crate::work_id))
        .collect();
    let plan = adopt_plan(&plain, &own);
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
                adopted.insert(id);
                kept += 1;
            }
            Err(_) => refused += 1,
        }
    }
    let tag = sender_entity(gone);
    let mut moved = 0usize;
    for atom in before {
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
    let atoms = crate::atoms_lean(&client, &workspace)?;
    let exported = export(root, scope, &atoms)?;
    if exported.contains("age is not installed") {
        bail!("sync: age is not installed, so adoption cannot seal {me}'s log; install age and try again");
    }
    let gone_paths = [gone_log.clone(), gone_digest.clone()];
    let mut stage = Vec::new();
    for rel in gone_paths {
        if stageable_path(root, &rel) {
            stage.push(rel.clone());
        }
        let path = root.join(rel);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    let _ = std::fs::remove_file(taken_path(&scope.name, gone));
    for rel in host_log_rels(&me) {
        if stageable_path(root, &rel) {
            stage.push(rel);
        }
    }
    let committed = commit_adopt(root, &scope.name, gone, &stage, push);
    let mut out = format!(
        "sync: {gone}: {kept} {} atoms adopted, {refused} refused, {moved} moved\n",
        scope.name
    );
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
        let said = export(&root, &scope, &atoms)?;
        out.push_str(&said);
        out.push_str(&commit_if_sealed(
            &root,
            &scope.name,
            &said,
            mode == TrackerGit::Push,
        ));
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
fn sealed_rows(root: &Path, identity: &Path) -> (Vec<crate::user_rules::UserRule>, usize) {
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
    let mut rows = Vec::new();
    let mut closed = 0;
    for path in paths {
        let opened = if identity.exists() {
            open(&path, identity).ok().flatten()
        } else {
            None
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
    let identity = identity_path();
    let mode = tracker_git(asked);
    let scope = scope_of_repo(root)?;
    let Some(scope) = scope.filter(|_| mode != TrackerGit::Off) else {
        let (mut rows, _) = sealed_rows(root, &identity);
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
    let (mut rows, closed) = sealed_rows(root, &identity);
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
}
