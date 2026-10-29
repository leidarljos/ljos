//! Undo runner files written by `ljos onboard`.
//!
//! Onboarding records each changed file, its previous bytes, and the bytes
//! it installed. Offboarding restores an unchanged installed file exactly.
//! A later edit is reversed only when the installed part can be identified.
//! Older installs have no record, so their runner paths are cleaned by the
//! markers ljos wrote into skills, hooks, plugins, env files and MCP entries.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Harness, Harnesses, OnboardPlace, Step};

const VERSION: u32 = 1;

#[derive(Debug, Default, Deserialize, Serialize)]
struct Manifest {
    version: u32,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Entry {
    runner: String,
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before_mode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after_mode: Option<u32>,
}

#[derive(Clone)]
struct FileState {
    bytes: Vec<u8>,
    mode: Option<u32>,
}

pub(crate) struct Journal {
    runner: String,
    file: PathBuf,
    skills: Option<PathBuf>,
    place: Option<OwnedPlace>,
    before: BTreeMap<PathBuf, Option<FileState>>,
}

#[derive(Clone)]
struct OwnedPlace {
    hooks_dir: PathBuf,
    guard_dir: PathBuf,
    git_config: Option<PathBuf>,
    git_env: Vec<(String, String)>,
}

impl OwnedPlace {
    fn from(place: OnboardPlace<'_>) -> Self {
        let git_config = place
            .git_env
            .iter()
            .find_map(|(key, value)| (*key == "GIT_CONFIG_GLOBAL").then(|| PathBuf::from(value)));
        Self {
            hooks_dir: place.hooks_dir.to_path_buf(),
            guard_dir: place.guard_dir.to_path_buf(),
            git_config,
            git_env: place
                .git_env
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        }
    }
}

impl Journal {
    pub(crate) fn begin(
        file: &Path,
        runner: &str,
        skills: Option<&Path>,
        place: Option<OnboardPlace<'_>>,
        dry: bool,
    ) -> Option<Self> {
        if dry || runner == "json" {
            return None;
        }
        let place = place.map(OwnedPlace::from);
        let paths = candidate_paths(file, runner, skills, place.as_ref());
        let before = paths
            .into_iter()
            .map(|path| {
                let state = read_state(&path);
                (path, state)
            })
            .collect();
        Some(Self {
            runner: runner.to_string(),
            file: file.to_path_buf(),
            skills: skills.map(Path::to_path_buf),
            place,
            before,
        })
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        for path in candidate_paths(
            &self.file,
            &self.runner,
            self.skills.as_deref(),
            self.place.as_ref(),
        ) {
            self.before.entry(path).or_insert(None);
        }
        let mut manifest = match read_manifest() {
            Ok(manifest) => manifest,
            Err(error) if missing_manifest(&error) => Manifest::default(),
            Err(error) => return Err(error),
        };
        manifest.version = VERSION;
        for (path, before) in self.before {
            let after = read_state(&path);
            if same_state(before.as_ref(), after.as_ref()) {
                continue;
            }
            let before_name = before
                .as_ref()
                .map(|state| store_backup(&state.bytes))
                .transpose()?;
            let after_name = after
                .as_ref()
                .map(|state| store_backup(&state.bytes))
                .transpose()?;
            manifest.entries.push(Entry {
                runner: self.runner.clone(),
                path,
                before: before_name,
                after: after_name,
                before_mode: before.as_ref().and_then(|state| state.mode),
                after_mode: after.as_ref().and_then(|state| state.mode),
            });
        }
        write_manifest(&manifest)
    }
}

fn state_root() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
        .join("onboard")
}

fn manifest_path() -> PathBuf {
    state_root().join("manifest.json")
}

fn backup_dir() -> PathBuf {
    state_root().join("backups")
}

fn read_manifest() -> Result<Manifest> {
    let path = manifest_path();
    let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    let manifest: Manifest =
        serde_json::from_str(&text).with_context(|| format!("{} is not JSON", path.display()))?;
    Ok(manifest)
}

fn write_manifest(manifest: &Manifest) -> Result<()> {
    let path = manifest_path();
    if manifest.entries.is_empty() {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(backup_dir());
        remove_empty_parents(path.parent(), state_root().parent());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(manifest)?;
    text.push('\n');
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn store_backup(bytes: &[u8]) -> Result<String> {
    let name = hex::encode(Sha256::digest(bytes));
    let path = backup_dir().join(&name);
    if !path.is_file() {
        fs::create_dir_all(backup_dir())?;
        fs::write(&path, bytes)?;
    }
    Ok(name)
}

fn load_backup(name: &str) -> Result<Vec<u8>> {
    let path = backup_dir().join(name);
    fs::read(&path).with_context(|| format!("missing onboard backup {}", path.display()))
}

fn read_state(path: &Path) -> Option<FileState> {
    let bytes = fs::read(path).ok()?;
    Some(FileState {
        bytes,
        mode: file_mode(path),
    })
}

#[cfg(unix)]
fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .ok()
        .map(|meta| meta.permissions().mode())
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) -> std::io::Result<()> {
    Ok(())
}

fn same_state(left: Option<&FileState>, right: Option<&FileState>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.bytes == right.bytes && left.mode == right.mode,
        _ => false,
    }
}

fn harness_for(file: &Path, runner: &str) -> Option<Harness> {
    crate::harnesses_with_shipped(file)
        .ok()
        .and_then(|all| all.harness.into_iter().find(|h| h.name == runner))
        .or_else(|| {
            toml::from_str::<Harnesses>(crate::HARNESSES_EXAMPLE)
                .ok()?
                .harness
                .into_iter()
                .find(|h| h.name == runner)
        })
}

fn add_tree_files(paths: &mut BTreeSet<PathBuf>, dir: &Path, predicate: impl Fn(&Path) -> bool) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && predicate(&path) {
            paths.insert(path);
        }
    }
}

fn candidate_paths(
    file: &Path,
    runner: &str,
    skills: Option<&Path>,
    place: Option<&OwnedPlace>,
) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::from([file.to_path_buf()]);
    if runner == "aider" {
        if let Ok(home) = std::env::var("HOME") {
            paths.insert(PathBuf::from(&home).join(".aider/ljos-conventions.md"));
            paths.insert(PathBuf::from(home).join(".aider.conf.yml"));
        }
        return paths;
    }
    if runner == "grok" {
        paths.insert(crate::expand("~/.grok/hooks/ljos.json"));
    }
    let Some(mut harness) = harness_for(file, runner) else {
        return paths;
    };
    if let Some(skills) = skills {
        harness.skills = Some(skills.display().to_string());
    }
    if let Some(config) = harness.config.as_deref() {
        paths.insert(crate::expand(config));
    }
    if let Some(config) = harness.json_config() {
        paths.insert(crate::expand(config));
    }
    if runner == "claude" {
        paths.insert(crate::expand("~/.claude.json"));
    }
    if let Some(hooks) = harness.hooks.as_deref() {
        let hooks = crate::expand(hooks);
        if harness.hooks_format.as_deref() == Some("cline") {
            paths.insert(hooks.join("PreToolUse"));
            paths.insert(hooks.join("UserPromptSubmit"));
        } else {
            paths.insert(hooks.clone());
        }
        if let Some(format) = harness.hooks_format.as_deref() {
            if matches!(format, "auggie" | "amp") {
                paths.insert(crate::runner_hooks::script_path(format, &hooks));
            }
            if format == "goose" {
                if let Some(root) = hooks.parent().and_then(Path::parent) {
                    paths.insert(root.join("plugin.json"));
                }
            }
        }
    }
    if let Some(plugin) = harness.plugin.as_deref() {
        paths.insert(crate::expand(plugin));
    }
    if let Some(skills) = harness.skills.as_deref() {
        let skill = crate::expand(skills).join("ljos/SKILL.md");
        paths.insert(skill.clone());
        paths.insert(skill.with_file_name(".ljos-skill-sha256"));
    }
    if let Some(agents) = harness.agents.as_deref() {
        let agents = crate::expand(agents);
        add_tree_files(&mut paths, &agents, |path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("ljos-") && name.ends_with(".md"))
        });
    }
    if harness.shell {
        let env = crate::env_path_for(file, &harness);
        paths.insert(env.clone());
        let name = env
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("env");
        paths.insert(env.with_file_name(format!(".{name}.ljos-wrote")));
        let guard = place
            .map(|place| place.guard_dir.clone())
            .unwrap_or_else(crate::guard_bin_dir);
        for command in crate::GUARD_SHIMMED {
            paths.insert(guard.join(command));
        }
    }
    if runner == "grokbot" {
        let hooks = place
            .map(|place| place.hooks_dir.clone())
            .unwrap_or_else(crate::git_hooks::onboard_hooks_dir);
        paths.extend(crate::git_hooks::onboard_hook_paths(&hooks));
        let git_env: Vec<(&str, &str)> = place
            .map(|place| {
                place
                    .git_env
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(configured) = crate::git_hooks::configured_hooks_path(&git_env) {
            paths.extend(crate::git_hooks::onboard_hook_paths(&configured));
        }
        paths.insert(hooks.join("previous-hooks-path"));
        let git_config = place
            .and_then(|place| place.git_config.clone())
            .or_else(|| std::env::var_os("GIT_CONFIG_GLOBAL").map(PathBuf::from))
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".gitconfig"))
            });
        if let Some(git_config) = git_config {
            paths.insert(git_config);
        }
    }
    paths
}

/// Undo onboarding for every runner, or only `runner`.
///
/// An unchanged file is restored byte for byte. A file with later edits is
/// left in place unless the installed span is still present.
pub fn offboard(runner: Option<&str>, dry: bool) -> Result<Vec<Step>> {
    match read_manifest() {
        Ok(mut manifest) => {
            let mut steps = Vec::new();
            let mut kept = Vec::new();
            let mut marker_fallback = BTreeSet::new();
            let mut selected = false;
            for entry in manifest.entries.into_iter().rev() {
                if runner.is_some_and(|name| name != entry.runner) {
                    kept.push(entry);
                    continue;
                }
                selected = true;
                let (step, done) = reverse_entry(&entry, dry)?;
                steps.push(step);
                if !done || dry {
                    if !done {
                        marker_fallback.insert(entry.runner.clone());
                    }
                    kept.push(entry);
                }
            }
            kept.reverse();
            manifest.entries = kept;
            for runner in &marker_fallback {
                steps.extend(legacy_offboard(Some(runner), dry)?);
            }
            if !dry {
                manifest
                    .entries
                    .retain(|entry| !marker_fallback.contains(&entry.runner));
                write_manifest(&manifest)?;
            }
            if runner.is_some() && !selected {
                steps.extend(legacy_offboard(runner, dry)?);
            }
            if steps.is_empty() {
                steps.push(Step {
                    what: "offboard".into(),
                    detail: "nothing installed by ljos onboard".into(),
                    ok: true,
                });
            }
            Ok(steps)
        }
        Err(err) if missing_manifest(&err) => legacy_offboard(runner, dry),
        Err(err) => Err(err),
    }
}

fn missing_manifest(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
}

fn reverse_entry(entry: &Entry, dry: bool) -> Result<(Step, bool)> {
    let before = entry.before.as_deref().map(load_backup).transpose()?;
    let after = entry.after.as_deref().map(load_backup).transpose()?;
    let current = read_state(&entry.path);
    let exact = match (current.as_ref(), after.as_deref()) {
        (None, None) => true,
        (Some(current), Some(after)) => current.bytes == after && current.mode == entry.after_mode,
        _ => false,
    };
    let action = if exact {
        Some((before.clone(), entry.before_mode))
    } else if current.is_none() && before.is_none() {
        return Ok((
            Step {
                what: entry.runner.clone(),
                detail: format!("{} is already absent", entry.path.display()),
                ok: true,
            },
            true,
        ));
    } else {
        current
            .as_ref()
            .map(|state| state.bytes.as_slice())
            .zip(after.as_deref())
            .and_then(|(current, after)| {
                reverse_changed(before.as_deref().unwrap_or_default(), after, current)
            })
            .map(|bytes| (Some(bytes), current.as_ref().and_then(|state| state.mode)))
    };
    let Some(action) = action else {
        return Ok((
            Step {
                what: entry.runner.clone(),
                detail: format!(
                    "left {}; it was edited after onboarding",
                    entry.path.display()
                ),
                ok: true,
            },
            false,
        ));
    };
    let verb = if action.0.is_some() {
        "restore"
    } else {
        "remove"
    };
    if dry {
        return Ok((
            Step {
                what: entry.runner.clone(),
                detail: format!("would {verb} {}", entry.path.display()),
                ok: true,
            },
            true,
        ));
    }
    match action.0 {
        Some(bytes) => {
            if let Some(parent) = entry.path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&entry.path, bytes)?;
            set_mode(&entry.path, action.1)?;
        }
        None => {
            let _ = fs::remove_file(&entry.path);
            remove_empty_parents(entry.path.parent(), home_path().as_deref());
        }
    }
    Ok((
        Step {
            what: entry.runner.clone(),
            detail: format!("{verb}d {}", entry.path.display()),
            ok: true,
        },
        true,
    ))
}

fn reverse_changed(before: &[u8], after: &[u8], current: &[u8]) -> Option<Vec<u8>> {
    let mut prefix = 0;
    let common = before.len().min(after.len());
    while prefix < common && before[prefix] == after[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < before.len().saturating_sub(prefix)
        && suffix < after.len().saturating_sub(prefix)
        && before[before.len() - 1 - suffix] == after[after.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let old = &before[prefix..before.len() - suffix];
    let installed = &after[prefix..after.len() - suffix];
    if installed.is_empty() {
        return None;
    }
    let positions: Vec<usize> = current
        .windows(installed.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == installed).then_some(at))
        .collect();
    if positions.len() != 1 {
        return None;
    }
    let at = positions[0];
    let mut out = Vec::with_capacity(current.len() + old.len() - installed.len());
    out.extend_from_slice(&current[..at]);
    out.extend_from_slice(old);
    out.extend_from_slice(&current[at + installed.len()..]);
    Some(out)
}

fn legacy_offboard(runner: Option<&str>, dry: bool) -> Result<Vec<Step>> {
    let file = crate::harnesses_path();
    let all = crate::harnesses_with_shipped(&file)?;
    let shipped = toml::from_str::<Harnesses>(crate::HARNESSES_EXAMPLE).unwrap_or_default();
    let names: BTreeSet<String> = all
        .harness
        .iter()
        .chain(shipped.harness.iter())
        .map(|h| h.name.clone())
        .chain(["aider".to_string()])
        .filter(|name| !name.starts_with("runner-with-"))
        .filter(|name| runner.is_none_or(|selected| selected == name))
        .collect();
    let mut steps = Vec::new();
    for name in names {
        if name == "aider" {
            legacy_aider(dry, &mut steps)?;
            continue;
        }
        if let Some(harness) = harness_for(&file, &name) {
            legacy_harness(&harness, dry, &mut steps)?;
            clean_shipped_shape(&file, &name, dry, &mut steps)?;
        }
    }
    if steps.is_empty() {
        steps.push(Step {
            what: "offboard".into(),
            detail: runner.map_or_else(
                || "nothing installed by ljos onboard".to_string(),
                |runner| format!("no ljos markers found for {runner}"),
            ),
            ok: true,
        });
    }
    Ok(steps)
}

fn clean_shipped_shape(path: &Path, runner: &str, dry: bool, steps: &mut Vec<Step>) -> Result<()> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let marker = format!("# The shipped {runner} shape, added by ljos onboard.\n");
    let Some(marker_at) = text.find(&marker) else {
        return Ok(());
    };
    let start = text[..marker_at].rfind('\n').map_or(marker_at, |at| at);
    let body = marker_at + marker.len();
    let end = text[body..]
        .find("\n[[harness]]")
        .map_or(text.len(), |at| body + at + 1);
    if dry {
        steps.push(would_remove(runner, path));
        return Ok(());
    }
    let mut out = String::new();
    out.push_str(&text[..start]);
    out.push_str(&text[end..]);
    if out.trim().is_empty() {
        fs::remove_file(path)?;
        remove_empty_parents(path.parent(), home_path().as_deref());
    } else {
        fs::write(path, out)?;
    }
    steps.push(removed(runner, path));
    Ok(())
}

fn legacy_harness(harness: &Harness, dry: bool, steps: &mut Vec<Step>) -> Result<()> {
    if let (Some(config), Some(pointer)) = (harness.json_config(), harness.json_pointer.as_deref())
    {
        clean_json_pointer(&crate::expand(config), pointer, &harness.name, dry, steps)?;
    }
    if let Some(config) = harness.config.as_deref() {
        clean_config_marker(
            &crate::expand(config),
            harness.marker.as_deref().unwrap_or(""),
            &harness.name,
            dry,
            steps,
        )?;
    }
    if let Some(agents) = harness.agents.as_deref() {
        let agents = crate::expand(agents);
        let files: Vec<PathBuf> = fs::read_dir(&agents)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("ljos-") && name.ends_with(".md"))
            })
            .collect();
        for path in files {
            remove_owned(
                &path,
                |bytes| String::from_utf8_lossy(bytes).contains(crate::AGENT_MARK),
                &harness.name,
                dry,
                steps,
            )?;
        }
    }
    if harness.name == "claude" {
        clean_json_pointer(
            &crate::expand("~/.claude.json"),
            "/mcpServers/ljos",
            &harness.name,
            dry,
            steps,
        )?;
    }
    if let Some(hooks) = harness.hooks.as_deref() {
        let hooks = crate::expand(hooks);
        clean_hooks(&hooks, harness, dry, steps)?;
        if let Some(format) = harness.hooks_format.as_deref() {
            if matches!(format, "auggie" | "amp") {
                remove_owned(
                    &crate::runner_hooks::script_path(format, &hooks),
                    |bytes| String::from_utf8_lossy(bytes).contains("Written by ljos onboard"),
                    &harness.name,
                    dry,
                    steps,
                )?;
            }
            if format == "goose" {
                if let Some(root) = hooks.parent().and_then(Path::parent) {
                    remove_owned(
                        &root.join("plugin.json"),
                        |bytes| {
                            let text = String::from_utf8_lossy(bytes);
                            text.contains("\"name\": \"ljos\"")
                                && text.contains("written by ljos onboard")
                        },
                        &harness.name,
                        dry,
                        steps,
                    )?;
                }
            }
        }
    }
    if let Some(plugin) = harness.plugin.as_deref() {
        remove_owned(
            &crate::expand(plugin),
            |bytes| String::from_utf8_lossy(bytes).contains("ljos hook"),
            &harness.name,
            dry,
            steps,
        )?;
    }
    if let Some(skills) = harness.skills.as_deref() {
        let skill = crate::expand(skills).join("ljos/SKILL.md");
        clean_skill(&skill, &harness.name, dry, steps)?;
    }
    if harness.name == "grok" {
        remove_owned(
            &crate::expand("~/.grok/hooks/ljos.json"),
            |bytes| String::from_utf8_lossy(bytes).contains("ljos hook"),
            &harness.name,
            dry,
            steps,
        )?;
    }
    if harness.shell {
        clean_env(
            &crate::env_path_for(&crate::harnesses_path(), harness),
            harness,
            dry,
            steps,
        )?;
        for command in crate::GUARD_SHIMMED {
            remove_owned(
                &crate::guard_bin_dir().join(command),
                |bytes| bytes == crate::GUARD_SHIM.as_bytes(),
                &harness.name,
                dry,
                steps,
            )?;
        }
    }
    if harness.name == "grokbot" && crate::git_hooks::onboarding_installed() {
        if dry {
            steps.push(Step {
                what: harness.name.clone(),
                detail: "would remove ljos git hooks".into(),
                ok: true,
            });
        } else {
            crate::git_hooks::uninstall()?;
            let git_config = std::env::var_os("GIT_CONFIG_GLOBAL")
                .map(PathBuf::from)
                .or_else(|| home_path().map(|home| home.join(".gitconfig")));
            if let Some(path) = git_config {
                if fs::read(&path).is_ok_and(|bytes| bytes.is_empty()) {
                    let _ = fs::remove_file(path);
                }
            }
            steps.push(Step {
                what: harness.name.clone(),
                detail: "removed ljos git hooks".into(),
                ok: true,
            });
        }
    }
    Ok(())
}

fn clean_skill(path: &Path, runner: &str, dry: bool, steps: &mut Vec<Step>) -> Result<()> {
    let sidecar = path.with_file_name(".ljos-skill-sha256");
    let Ok(bytes) = fs::read(path) else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&bytes);
    if !text.starts_with("---\nname: ljos\n") || !text.contains("# The seat protocol") {
        return Ok(());
    }
    if let Ok(recorded) = fs::read_to_string(&sidecar) {
        let current = hex::encode(Sha256::digest(&bytes));
        if recorded.trim() != current {
            steps.push(Step {
                what: runner.to_string(),
                detail: format!("left {}; it was edited after onboarding", path.display()),
                ok: true,
            });
            return Ok(());
        }
    }
    if dry {
        steps.push(would_remove(runner, path));
        return Ok(());
    }
    fs::remove_file(path)?;
    let _ = fs::remove_file(sidecar);
    remove_empty_parents(path.parent(), home_path().as_deref());
    steps.push(removed(runner, path));
    Ok(())
}

fn clean_json_pointer(
    path: &Path,
    pointer: &str,
    runner: &str,
    dry: bool,
    steps: &mut Vec<Step>,
) -> Result<()> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&text) else {
        return Ok(());
    };
    if root.pointer(pointer).is_none() {
        return Ok(());
    }
    if dry {
        steps.push(would_remove(runner, path));
        return Ok(());
    }
    if remove_json_pointer(&mut root, pointer) {
        prune_empty(&mut root);
        if root.as_object().is_some_and(serde_json::Map::is_empty) {
            fs::remove_file(path)?;
            remove_empty_parents(path.parent(), home_path().as_deref());
        } else {
            let mut out = serde_json::to_string_pretty(&root)?;
            out.push('\n');
            fs::write(path, out)?;
        }
        steps.push(removed(runner, path));
    }
    Ok(())
}

fn remove_json_pointer(root: &mut Value, pointer: &str) -> bool {
    let mut parts: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
    let Some(last) = parts.pop() else {
        return false;
    };
    let mut at = root;
    for part in parts {
        let Some(next) = at.get_mut(part) else {
            return false;
        };
        at = next;
    }
    at.as_object_mut()
        .and_then(|obj| obj.remove(last))
        .is_some()
}

fn clean_config_marker(
    path: &Path,
    marker: &str,
    runner: &str,
    dry: bool,
    steps: &mut Vec<Step>,
) -> Result<()> {
    if marker.is_empty() {
        return Ok(());
    }
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let Some(start) = text.find(marker.trim_start_matches('\n')) else {
        return Ok(());
    };
    let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
    let rest = &text[start..];
    let end = if marker.contains("mcp_servers") {
        rest[1..]
            .find("\n[")
            .map_or(text.len(), |at| start + at + 1)
    } else if marker.contains("  ljos:") {
        rest.lines()
            .skip(1)
            .scan(
                start + rest.lines().next().map_or(0, |line| line.len() + 1),
                |at, line| {
                    let here = *at;
                    *at += line.len() + 1;
                    Some((here, line))
                },
            )
            .find(|(_, line)| !line.starts_with("    ") && !line.trim().is_empty())
            .map_or(text.len(), |(at, _)| at)
    } else {
        return Ok(());
    };
    if dry {
        steps.push(would_remove(runner, path));
    } else {
        let mut out = String::new();
        out.push_str(&text[..line_start]);
        out.push_str(&text[end..]);
        if matches!(out.trim(), "" | "mcp_servers:" | "extensions:") {
            fs::remove_file(path)?;
            remove_empty_parents(path.parent(), home_path().as_deref());
        } else {
            fs::write(path, out)?;
        }
        steps.push(removed(runner, path));
    }
    Ok(())
}

fn clean_hooks(path: &Path, harness: &Harness, dry: bool, steps: &mut Vec<Step>) -> Result<()> {
    if harness.hooks_format.as_deref() == Some("cline") {
        for event in ["PreToolUse", "UserPromptSubmit"] {
            remove_owned(
                &path.join(event),
                |bytes| String::from_utf8_lossy(bytes).contains("Written by ljos onboard"),
                &harness.name,
                dry,
                steps,
            )?;
        }
        return Ok(());
    }
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&text) else {
        return Ok(());
    };
    let before = root.clone();
    strip_hook_values(&mut root);
    if root == before {
        return Ok(());
    }
    prune_empty(&mut root);
    if dry {
        steps.push(would_remove(&harness.name, path));
    } else if only_hook_scaffold(&root) {
        fs::remove_file(path)?;
        remove_empty_parents(path.parent(), home_path().as_deref());
        steps.push(removed(&harness.name, path));
    } else {
        let mut out = serde_json::to_string_pretty(&root)?;
        out.push('\n');
        fs::write(path, out)?;
        steps.push(removed(&harness.name, path));
    }
    Ok(())
}

fn prune_empty(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values.iter_mut() {
                prune_empty(value);
            }
            values.retain(|value| !empty_json(value));
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                prune_empty(value);
            }
            values.retain(|_, value| !empty_json(value));
        }
        _ => {}
    }
}

fn empty_json(value: &Value) -> bool {
    value.as_array().is_some_and(Vec::is_empty)
        || value.as_object().is_some_and(serde_json::Map::is_empty)
}

fn only_hook_scaffold(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.is_empty() || object.keys().all(|key| key == "version"))
}

fn strip_hook_values(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values.iter_mut() {
                let had_hook = contains_hook_command(value);
                strip_hook_values(value);
                if had_hook && hook_container_empty(value) {
                    *value = Value::Null;
                }
            }
            values.retain(|item| !item.is_null() && !owned_hook_item(item));
        }
        Value::Object(values) => {
            values.retain(|key, value| key != "ljos" || !contains_hook_command(value));
            for value in values.values_mut() {
                strip_hook_values(value);
            }
        }
        _ => {}
    }
}

fn hook_container_empty(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.get("hooks").is_some_and(|hooks| {
            hooks.as_array().is_some_and(Vec::is_empty)
                || hooks.as_object().is_some_and(serde_json::Map::is_empty)
        })
    })
}

fn owned_hook_item(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let has_nested_list = object.values().any(Value::is_array);
    !has_nested_list && contains_hook_command(value)
}

fn contains_hook_command(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("ljos hook") || text.contains("ljos-guard-"),
        Value::Array(values) => values.iter().any(contains_hook_command),
        Value::Object(values) => values.values().any(contains_hook_command),
        _ => false,
    }
}

fn clean_env(path: &Path, harness: &Harness, dry: bool, steps: &mut Vec<Step>) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("env");
    let mark = path.with_file_name(format!(".{name}.ljos-wrote"));
    let Ok(template) = fs::read_to_string(&mark) else {
        return remove_owned(
            path,
            |bytes| String::from_utf8_lossy(bytes).contains("ljos/guard-bin"),
            &harness.name,
            dry,
            steps,
        );
    };
    let current = fs::read_to_string(path).unwrap_or_default();
    let mut kept: Vec<&str> = current.lines().collect();
    for line in template.lines() {
        if let Some(at) = kept.iter().position(|candidate| *candidate == line) {
            kept.remove(at);
        }
    }
    if dry {
        steps.push(would_remove(&harness.name, path));
        return Ok(());
    }
    if kept.iter().all(|line| line.trim().is_empty()) {
        let _ = fs::remove_file(path);
    } else {
        fs::write(path, kept.join("\n") + "\n")?;
    }
    let _ = fs::remove_file(mark);
    steps.push(removed(&harness.name, path));
    Ok(())
}

fn legacy_aider(dry: bool, steps: &mut Vec<Step>) -> Result<()> {
    let home = home_path().unwrap_or_else(|| PathBuf::from("."));
    let convention = home.join(".aider/ljos-conventions.md");
    let entry = convention.display().to_string();
    remove_owned(
        &convention,
        |bytes| String::from_utf8_lossy(bytes).starts_with("---\nname: ljos\n"),
        "aider",
        dry,
        steps,
    )?;
    let config = home.join(".aider.conf.yml");
    let Ok(text) = fs::read_to_string(&config) else {
        return Ok(());
    };
    let out = aider_without_entry(&text, &entry);
    if out != text {
        if dry {
            steps.push(would_remove("aider", &config));
        } else if out.trim().is_empty() {
            fs::remove_file(&config)?;
            remove_empty_parents(config.parent(), home_path().as_deref());
            steps.push(removed("aider", &config));
        } else {
            fs::write(&config, out)?;
            steps.push(removed("aider", &config));
        }
    }
    Ok(())
}

fn aider_without_entry(text: &str, entry: &str) -> String {
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|line| {
            line.trim()
                .trim_start_matches("- ")
                .trim_matches(['"', '\''])
                != entry
        })
        .collect();
    let empty_read: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(at, line)| {
            if line.trim() != "read:" {
                return None;
            }
            let has_item = lines[at + 1..]
                .iter()
                .take_while(|next| next.trim().is_empty() || next.starts_with(char::is_whitespace))
                .any(|next| next.trim_start().starts_with("- "));
            (!has_item).then_some(at)
        })
        .collect();
    for at in empty_read.into_iter().rev() {
        lines.remove(at);
    }
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

fn remove_owned(
    path: &Path,
    owns: impl Fn(&[u8]) -> bool,
    runner: &str,
    dry: bool,
    steps: &mut Vec<Step>,
) -> Result<()> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(());
    };
    if !owns(&bytes) {
        return Ok(());
    }
    if dry {
        steps.push(would_remove(runner, path));
    } else {
        fs::remove_file(path)?;
        remove_empty_parents(path.parent(), home_path().as_deref());
        steps.push(removed(runner, path));
    }
    Ok(())
}

fn would_remove(runner: &str, path: &Path) -> Step {
    Step {
        what: runner.to_string(),
        detail: format!("would remove ljos from {}", path.display()),
        ok: true,
    }
}

fn removed(runner: &str, path: &Path) -> Step {
    Step {
        what: runner.to_string(),
        detail: format!("removed ljos from {}", path.display()),
        ok: true,
    }
}

fn home_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn remove_empty_parents(mut parent: Option<&Path>, stop: Option<&Path>) {
    while let Some(path) = parent {
        if stop.is_some_and(|stop| path == stop) || fs::remove_dir(path).is_err() {
            break;
        }
        parent = path.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_edit_survives_the_reverse_span() {
        let before = b"one\ntwo\n";
        let after = b"one\nljos\ntwo\n";
        let current = b"mine\none\nljos\ntwo\n";
        assert_eq!(
            reverse_changed(before, after, current).as_deref(),
            Some(&b"mine\none\ntwo\n"[..])
        );
    }

    #[test]
    fn removing_a_json_pointer_keeps_the_other_entries() {
        let mut value: Value =
            serde_json::from_str(r#"{"mcpServers":{"mine":{},"ljos":{"command":"ljos-mcp"}}}"#)
                .unwrap();
        assert!(remove_json_pointer(&mut value, "/mcpServers/ljos"));
        assert!(value.pointer("/mcpServers/mine").is_some());
        assert!(value.pointer("/mcpServers/ljos").is_none());
    }
}
