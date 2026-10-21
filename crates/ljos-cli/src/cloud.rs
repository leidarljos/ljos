//! One command for a fresh machine that should share the tracker.
//!
//! `ljos cloud up` installs the release binaries into `~/.local/bin`,
//! checks the published SHA-256 sums, puts that directory on PATH, writes
//! the harness MCP command as `ljos-mcp`, and clones `LJOS_TRACKER_REMOTE`.
//! A later boot with the binaries already installed only refreshes the clone.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};

const REQUIRED_BINS: &[&str] = &["ljos", "ljos-mcp", "ljos-policyd"];
const OPTIONAL_BINS: &[&str] = &[
    "packsetd",
    "packset",
    "packset-mcp",
    "vissue",
    "vissue-mcp",
    "deedar",
    "claimdag",
    "ljos-consensus",
];
const INSTALL_URL: &str = "https://github.com/leidarljos/ljos/releases/latest/download/install.sh";
const CLOUD_GUARD_RULES_FILE: &str = "cloud-guard-rules.json";

/// Install binaries, point the harness at them, and clone the shared tracker.
///
/// Idempotent. `force` downloads the release again.
///
/// # Errors
///
/// A checksum does not match, a required binary is missing, or the tracker
/// remote cannot be cloned. A push that fails after a successful clone is
/// reported and left queued; the clone itself stands.
pub fn up(force: bool) -> Result<String> {
    let home = crate::home().context("cloud up: HOME is unset")?;
    let bin_dir = bin_dir(&home);
    let mut lines = Vec::new();
    lines.push(ensure_binaries(&bin_dir, force)?);
    // The config names a bare `ljos-mcp`. The directory has to be on PATH
    // before anything reads that file, including a later step in this process.
    lines.push(ensure_process_path(&bin_dir));

    if agent_id().is_some() && seat_matches_file() {
        // A seat left in the env file by an earlier boot must not hide this
        // boot's agent id. An exported seat that differs from the file stays.
        crate::with_env_mutation(|| std::env::remove_var("LJOS_SEAT"));
    }
    let seat = default_seat();
    lines.push(format!("seat: {seat}"));
    let key = prepare_seat_key(&seat)?;
    lines.push(format!("seat key: {}", key.display()));

    let remote = nonempty_var("LJOS_TRACKER_REMOTE");
    let tracker = remote.as_ref().map(|_| tracker_dir(&home));
    lines.push(ensure_profile(&home, &bin_dir)?);
    lines.push(write_mcp(
        &home,
        &bin_dir.join("ljos-mcp"),
        &seat,
        &key,
        tracker.as_deref(),
        remote.as_deref(),
    )?);

    let mut pairs = vec![
        ("LJOS_SEAT", seat.clone()),
        ("VISSUE_AGENT", seat.clone()),
        ("LJOS_SEAT_KEY", key.display().to_string()),
    ];
    if std::env::var("DEEDAR_HOST_SIGNING_KEY").ok().as_deref() != Some("off") {
        pairs.push(("DEEDAR_HOST_SIGNING_KEY", key.display().to_string()));
    }
    if let (Some(remote), Some(dest)) = (remote.as_deref(), tracker.as_deref()) {
        lines.push(ensure_tracker(remote, dest, &seat)?);
        pairs.push(("LJOS_TRACKER_GIT", "push".into()));
        pairs.push(("LJOS_TRACKER_REMOTE", remote.to_string()));
        pairs.push(("VISSUE_ROOT", dest.display().to_string()));
        pairs.push(("ISSUE_ROOT", dest.display().to_string()));
    } else {
        lines.push(
            "tracker: LJOS_TRACKER_REMOTE is unset, so this machine has no shared tracker".into(),
        );
    }
    write_env(&home.join(".config/ljos/env"), &pairs)?;
    crate::with_env_mutation(|| {
        for (k, v) in &pairs {
            std::env::set_var(k, v);
        }
    });
    lines.push(ensure_cloud_guard_rules()?);
    let reaped = reap_gone_owners(crate::lease_secs());
    if !reaped.trim().is_empty() {
        lines.push(reaped.trim_end().to_string());
    }
    Ok(lines.join("\n") + "\n")
}

/// The seat this boot should write under.
///
/// `LJOS_SEAT` when it is set, with the run id appended when this boot has
/// one, so a reused name (`ridley-ci-a`) is a different seat on every run.
/// Otherwise the harness name and the agent id, and the machine name when
/// there is no agent id.
#[must_use]
pub fn default_seat() -> String {
    let id = agent_id();
    if let Some(seat) = nonempty_var("LJOS_SEAT") {
        return with_run(&crate::seat_slug(&seat), id.as_deref());
    }
    if let Some(id) = id {
        return crate::seat_slug(&format!("{}-{id}", harness_kind()));
    }
    crate::seat_slug(&format!("vm-{}", hostname()))
}

fn with_run(base: &str, id: Option<&str>) -> String {
    let Some(id) = id else {
        return base.to_string();
    };
    if base == id || base.ends_with(&format!("-{id}")) {
        return base.to_string();
    }
    crate::seat_slug(&format!("{base}-{id}"))
}

/// The seat's signing key, created on first use. This is not the machine
/// key at `~/.config/deedar/host.key`.
///
/// # Errors
///
/// No home directory, or the key file cannot be written.
pub fn prepare_seat_key(seat: &str) -> Result<PathBuf> {
    let path =
        crate::ballot_sig::seat_key_file(seat).context("cloud up: no directory for a seat key")?;
    crate::ballot_sig::load_or_create(&path)?;
    Ok(path)
}

/// What `ljos doctor` says about the harness MCP command.
#[must_use]
pub fn mcp_row() -> crate::Habitat {
    match mcp_problem() {
        McpProblem::None => crate::Habitat {
            name: "mcp",
            state: "no harness mcp config".into(),
            ok: true,
        },
        McpProblem::Ready(state) => crate::Habitat {
            name: "mcp",
            state,
            ok: true,
        },
        McpProblem::Warn(state) => crate::Habitat {
            name: "mcp",
            state: format!("warning: {state}"),
            ok: true,
        },
        McpProblem::Missing(state) => crate::Habitat {
            name: "mcp",
            state,
            ok: false,
        },
    }
}

/// What `ljos doctor` says about the cloud guard refusals cloud seats need.
#[must_use]
pub fn guard_row() -> crate::Habitat {
    let Some(path) = cloud_guard_rules_path() else {
        return crate::Habitat {
            name: "cloud guard",
            state: "no config home for cloud guard rules".into(),
            ok: false,
        };
    };
    let wanted = cloud_guard_rules();
    let have = crate::kept_rules(&path);
    if have == wanted {
        return crate::Habitat {
            name: "cloud guard",
            state: format!(
                "{} denies cursor/* pushes and `gh pr create|merge`",
                path.display()
            ),
            ok: true,
        };
    }
    let state = if have.is_empty() {
        format!(
            "missing in {}; run `ljos cloud up` to install the cloud seat refusals",
            path.display()
        )
    } else {
        format!(
            "{} differs from the cloud seat refusals; run `ljos cloud up`",
            path.display()
        )
    };
    crate::Habitat {
        name: "cloud guard",
        state,
        ok: false,
    }
}

/// The stderr line when a configured MCP command does not exist.
/// `None` when there is no config, or the command is there.
#[must_use]
pub fn missing_mcp_line() -> Option<String> {
    match mcp_problem() {
        McpProblem::Missing(state) => Some(state),
        McpProblem::None | McpProblem::Ready(_) | McpProblem::Warn(_) => None,
    }
}

/// Cloud-up refusals over shell commands, in the seat rule format.
#[must_use]
pub(crate) fn guard_line(line: &str) -> Option<crate::Rule> {
    let path = cloud_guard_rules_path()?;
    let denies: Vec<crate::Rule> = crate::kept_rules(&path)
        .into_iter()
        .filter(|rule| rule.verdict == "deny")
        .collect();
    crate::verdict_for(&denies, line).cloned()
}

/// git's org merge driver. The merged text replaces `ours`.
///
/// # Errors
///
/// An issue file did not parse, or the result still has conflict markers.
pub fn merge_org_cli(base: &Path, ours: &Path, theirs: &Path) -> Result<()> {
    crate::tracker_push::merge_org_files(base, ours, theirs).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Hand back `STARTED` claims whose owner is gone and whose claim is older
/// than `lease` seconds. No-op unless `LJOS_TRACKER_GIT=push`. The release
/// is committed and pushed like any other tracker write.
#[must_use]
pub fn reap_gone_owners(lease: u64) -> String {
    crate::load_seat_env();
    if std::env::var("LJOS_TRACKER_GIT").ok().as_deref() != Some("push") {
        return String::new();
    }
    let Ok(layout) = vissue_core::Layout::resolve(None, None) else {
        return String::new();
    };
    let Ok(issues) = vissue_core::store::load_all(&layout) else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let mut out = String::new();
    for (_project, heading) in issues {
        if heading.state != "STARTED" {
            continue;
        }
        let Some(who) = heading.claimed_by().map(str::to_string) else {
            continue;
        };
        if holder_running(&who, &heading.id) {
            continue;
        }
        let idle = match heading.claimed_at().and_then(crate::reap::org_ts) {
            Some(taken) => taken.saturating_add(lease as i64) <= now,
            None => true,
        };
        if !idle {
            continue;
        }
        match release_started(&heading.id, &who) {
            Ok(Some(line)) => {
                out.push_str(&line);
                out.push('\n');
                out.push_str(&crate::persist_tracker(&heading.id, "release"));
            }
            Ok(None) => {}
            Err(e) => out.push_str(&format!("could not release {}: {e}\n", heading.id)),
        }
    }
    out
}

enum McpProblem {
    None,
    Ready(String),
    Warn(String),
    Missing(String),
}

struct McpFile {
    path: PathBuf,
    probe: McpProbe,
    advice: String,
    expands_home: bool,
    /// Parse and read errors fail the row. Harness files are skipped when
    /// they are not the JSON this probe expects (Zed's settings are JSONC).
    strict: bool,
}

enum McpProbe {
    Json(String),
    Text(String),
}

struct LaunchSpec {
    command: String,
    args: Vec<String>,
    env_path: Option<String>,
}

enum LaunchCheck {
    Ready,
    Warn(String),
    Missing(String),
}

fn mcp_problem() -> McpProblem {
    let mut missing = Vec::new();
    let mut warned = Vec::new();
    let mut ready = Vec::new();
    for file in mcp_files() {
        if !file.path.is_file() {
            continue;
        }
        let launch = match &file.probe {
            McpProbe::Json(pointer) => match read_json_launch(&file.path, pointer) {
                Ok(found) => found,
                Err(e) if file.strict => {
                    missing.push(format!(
                        "ljos-mcp: {} is not readable ({e}), so the MCP client has no tools. Run `{}`.",
                        file.path.display(),
                        file.advice
                    ));
                    continue;
                }
                Err(_) => None,
            },
            McpProbe::Text(marker) => read_text_launch(&file.path, marker),
        };
        let Some(launch) = launch else {
            continue;
        };
        match launch_check(&file, &launch) {
            LaunchCheck::Ready => {
                ready.push(format!("{} starts {}", file.path.display(), launch.command))
            }
            LaunchCheck::Warn(why) => warned.push(format!(
                "ljos-mcp: {why} ({}). Run `{}`.",
                file.path.display(),
                file.advice
            )),
            LaunchCheck::Missing(why) => missing.push(format!(
                "ljos-mcp: {why} ({}), so the MCP client has no tools. Run `{}`.",
                file.path.display(),
                file.advice
            )),
        }
    }
    if !missing.is_empty() {
        return McpProblem::Missing(missing.join("; "));
    }
    if !warned.is_empty() {
        return McpProblem::Warn(warned.join("; "));
    }
    if !ready.is_empty() {
        return McpProblem::Ready(ready.join("; "));
    }
    McpProblem::None
}

/// Cursor's `mcp.json`, `LJOS_MCP_CONFIG`, and every harness file ljos
/// writes. One path is checked once. Claude's `mcp add` stores the command
/// in `~/.claude.json`, which the harness table does not name.
fn mcp_files() -> Vec<McpFile> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut push =
        |path: PathBuf, probe: McpProbe, strict: bool, advice: String, expands_home: bool| {
            let kind = match &probe {
                McpProbe::Json(pointer) => format!("json:{pointer}"),
                McpProbe::Text(marker) => format!("text:{marker}"),
            };
            if seen.insert(format!("{kind}:{}", path.display())) {
                out.push(McpFile {
                    path,
                    probe,
                    advice,
                    expands_home,
                    strict,
                });
            }
        };
    if let Some(path) = std::env::var_os("LJOS_MCP_CONFIG").filter(|p| !p.is_empty()) {
        push(
            PathBuf::from(path),
            McpProbe::Json("/mcpServers/ljos".into()),
            true,
            "ljos onboard --harness cursor".into(),
            true,
        );
    }
    if let Ok(home) = crate::home() {
        push(
            home.join(".cursor/mcp.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            true,
            "ljos cloud up".into(),
            true,
        );
        push(
            home.join(".claude.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            false,
            "ljos onboard --harness claude".into(),
            true,
        );
    }
    for harness in harness_shapes() {
        let advice = format!("ljos onboard --harness {}", harness.name);
        let expands_home = harness.name != "codex";
        if let (Some(config), Some(pointer)) = (&harness.config_json, &harness.json_pointer) {
            if !pointer.is_empty() {
                push(
                    crate::expand(config),
                    McpProbe::Json(pointer.clone()),
                    false,
                    advice.clone(),
                    expands_home,
                );
            }
        }
        if let (Some(config), Some(marker)) = (&harness.config, &harness.marker) {
            if !marker.is_empty() {
                push(
                    crate::expand(config),
                    McpProbe::Text(marker.clone()),
                    false,
                    advice.clone(),
                    expands_home,
                );
            }
        }
    }
    out
}

fn harness_shapes() -> Vec<crate::Harness> {
    let shipped: crate::Harnesses = toml::from_str(crate::HARNESSES_EXAMPLE).unwrap_or_default();
    let mut shapes = crate::harnesses_with_shipped(&crate::harnesses_path())
        .map(|h| h.harness)
        .unwrap_or_default();
    for harness in shipped.harness {
        if harness.name.starts_with("runner-with-") || harness.shell {
            continue;
        }
        if !shapes.iter().any(|have| have.name == harness.name) {
            shapes.push(harness);
        }
    }
    shapes.retain(|h| !h.name.starts_with("runner-with-") && !h.shell);
    shapes
}

fn read_json_launch(path: &Path, pointer: &str) -> Result<Option<LaunchSpec>> {
    let text = fs::read_to_string(path).with_context(|| path.display().to_string())?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let doc: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("{}: not JSON", path.display()))?;
    let Some(entry) = doc.pointer(pointer) else {
        return Ok(None);
    };
    Ok(command_and_args(entry))
}

fn command_and_args(entry: &serde_json::Value) -> Option<LaunchSpec> {
    let extra = string_list(entry.get("args"));
    let env_path = entry
        .get("env")
        .and_then(serde_json::Value::as_object)
        .and_then(|env| env.get("PATH"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    match entry.get("command").or_else(|| entry.get("cmd"))? {
        serde_json::Value::String(command) => {
            let command = command.trim();
            (!command.is_empty()).then(|| LaunchSpec {
                command: command.to_string(),
                args: extra,
                env_path,
            })
        }
        serde_json::Value::Array(parts) => {
            let mut argv: Vec<String> = parts
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let command = argv.first()?.trim().to_string();
            if command.is_empty() {
                return None;
            }
            argv.remove(0);
            argv.extend(extra);
            Some(LaunchSpec {
                command,
                args: argv,
                env_path,
            })
        }
        _ => None,
    }
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The `command` or `cmd` in the section the harness marker names.
fn read_text_launch(path: &Path, marker: &str) -> Option<LaunchSpec> {
    let text = fs::read_to_string(path).ok()?;
    let idx = text.find(marker)?;
    let window: String = text[idx..].lines().take(24).collect::<Vec<_>>().join("\n");
    let command = keyed_value(&window, "command").or_else(|| keyed_value(&window, "cmd"))?;
    if command.is_empty() {
        None
    } else {
        Some(LaunchSpec {
            command,
            args: Vec::new(),
            env_path: keyed_value(&window, "PATH"),
        })
    }
}

fn keyed_value(window: &str, key: &str) -> Option<String> {
    let mut rest = window;
    while let Some(at) = rest.find(key) {
        let before = at == 0 || !rest.as_bytes()[at - 1].is_ascii_alphanumeric();
        let after_idx = at + key.len();
        let after = after_idx >= rest.len() || !rest.as_bytes()[after_idx].is_ascii_alphanumeric();
        if before && after {
            let tail = rest[after_idx..].trim_start();
            if let Some(value) = tail.strip_prefix('=').or_else(|| tail.strip_prefix(':')) {
                let parsed = parse_scalar(value.trim_start());
                if !parsed.is_empty() {
                    return Some(parsed);
                }
            }
        }
        rest = &rest[after_idx..];
    }
    None
}

fn parse_scalar(rest: &str) -> String {
    let mut chars = rest.chars();
    match chars.next() {
        Some(q @ ('"' | '\'')) => {
            let raw = chars.as_str();
            match raw.find(q) {
                Some(end) => raw[..end].to_string(),
                None => String::new(),
            }
        }
        _ => rest
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(',')
            .to_string(),
    }
}

/// `$HOME`, `${HOME}`, `${env:HOME}`, `${userHome}` and a leading `~/` are
/// the home directory at launch when the client expands them.
fn expand_launch(command: &str, expands_home: bool) -> String {
    if !expands_home {
        return command.to_string();
    }
    let Ok(home) = crate::home() else {
        return command.to_string();
    };
    let home = home.display().to_string();
    let mut expanded = if let Some(rest) = command.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else if command == "~" {
        home.clone()
    } else {
        command.to_string()
    };
    for token in ["${env:HOME}", "${userHome}", "${HOME}", "$HOME"] {
        if expanded.contains(token) {
            expanded = expanded.replace(token, &home);
        }
    }
    expanded
}

fn launch_check(file: &McpFile, launch: &LaunchSpec) -> LaunchCheck {
    let command = expand_launch(&launch.command, file.expands_home);
    let found_path = command_path(&command, launch.env_path.as_deref());
    let search_path = launch.env_path.as_deref();
    let shell_path = if search_path.is_none() {
        shell_command_path(&command)
    } else {
        None
    };
    let pathy = command.contains('/') || Path::new(&command).is_absolute();
    let chosen = found_path.clone().or_else(|| shell_path.clone());
    let Some(chosen) = chosen else {
        return LaunchCheck::Missing(if pathy {
            format!("{} does not exist", launch.command)
        } else if search_path.is_some() {
            format!("{} is not on env.PATH", launch.command)
        } else {
            format!("{} is not on PATH", launch.command)
        });
    };
    if found_path.is_none() {
        return LaunchCheck::Warn(format!(
            "{} is not on this process PATH, but $SHELL -lc finds {}",
            launch.command,
            chosen.display()
        ));
    }
    if !pathy {
        let own = crate::server_path().ok();
        if let Some(own) = own {
            let name = Path::new(&command).file_name().and_then(|n| n.to_str());
            if name == Some(crate::MCP_SERVER_COMMAND) && !same_binary(&chosen, &own) {
                return LaunchCheck::Warn(format!(
                    "{} resolves to {}, not {}",
                    launch.command,
                    chosen.display(),
                    own.display()
                ));
            }
        }
    }
    let name = Path::new(&command).file_name().and_then(|n| n.to_str());
    if name == Some("ljos-plugin") {
        if let Some(bin) = launch.args.first() {
            if !bin.contains('/')
                && command_path(bin, search_path).is_none()
                && !home_bin_exists(bin)
            {
                return LaunchCheck::Missing(format!("{bin} is not installed"));
            }
        }
    }
    LaunchCheck::Ready
}

fn command_path(command: &str, search_path: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(command);
    if command.contains('/') || path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    match search_path {
        Some(path) => path_lookup(command, path),
        None => which::which(command).ok(),
    }
}

fn path_lookup(command: &str, path: &str) -> Option<PathBuf> {
    if command.contains('/') || command.is_empty() {
        return None;
    }
    for dir in std::env::split_paths(path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn shell_command_path(command: &str) -> Option<PathBuf> {
    if command.contains('/') || command.is_empty() {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let out = Command::new(shell)
        .arg("-lc")
        .arg("command -v -- \"$1\"")
        .arg("_")
        .arg(command)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let found = stdout.lines().next().unwrap_or("").trim();
    if found.is_empty() {
        None
    } else {
        Some(PathBuf::from(found))
    }
}

fn same_binary(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn home_bin_exists(name: &str) -> bool {
    let Ok(home) = crate::home() else {
        return false;
    };
    [home.join(".local/bin"), home.join(".cargo/bin")]
        .into_iter()
        .any(|dir| dir.join(name).is_file())
}

fn ensure_binaries(bin_dir: &Path, force: bool) -> Result<String> {
    if let Some(dir) = std::env::var_os("LJOS_RELEASE_DIR").filter(|p| !p.is_empty()) {
        return install_from_dir(bin_dir, Path::new(&dir));
    }
    if force {
        for name in REQUIRED_BINS.iter().chain(OPTIONAL_BINS.iter()) {
            let _ = fs::remove_file(bin_dir.join(name));
        }
    }
    if bins_ready(bin_dir) {
        return Ok(format!("binaries: already in {}", bin_dir.display()));
    }
    run_installer(bin_dir)?;
    if !bins_ready(bin_dir) {
        bail!(
            "cloud up: {} is missing ljos, ljos-mcp or ljos-policyd after install",
            bin_dir.display()
        );
    }
    Ok(format!("binaries: installed into {}", bin_dir.display()))
}

fn bins_ready(dir: &Path) -> bool {
    REQUIRED_BINS.iter().all(|name| {
        let path = dir.join(name);
        path.is_file() && executable(&path)
    })
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn install_from_dir(bin_dir: &Path, src: &Path) -> Result<String> {
    let sums_path = src.join("SHA256SUMS");
    let text =
        fs::read_to_string(&sums_path).with_context(|| format!("read {}", sums_path.display()))?;
    let mut want = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            bail!("{}:{}: no hash", sums_path.display(), n + 1);
        };
        let Some(name) = parts.next() else {
            bail!("{}:{}: no name", sums_path.display(), n + 1);
        };
        let name = name.trim_start_matches('*');
        want.insert(name.to_string(), hash.to_ascii_lowercase());
    }
    for name in REQUIRED_BINS {
        if !want.contains_key(*name) {
            bail!("{name} is not listed in {}", sums_path.display());
        }
    }
    let mut install = Vec::new();
    for name in REQUIRED_BINS
        .iter()
        .copied()
        .chain(OPTIONAL_BINS.iter().copied())
    {
        if want.contains_key(name) {
            install.push(name);
        }
    }
    for name in &install {
        let path = src.join(name);
        if !path.is_file() {
            bail!(
                "{} is listed in the checksums and is not in {}",
                name,
                src.display()
            );
        }
        let got = sha256_file(&path)?;
        let expect = &want[*name];
        if got != *expect {
            bail!(
                "{name}: checksum mismatch (published {expect}, file {got}); nothing was installed"
            );
        }
    }
    fs::create_dir_all(bin_dir)?;
    for name in &install {
        let dest = bin_dir.join(name);
        fs::copy(src.join(name), &dest)
            .with_context(|| format!("copy {name} to {}", dest.display()))?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
    }
    Ok(format!(
        "binaries: installed {} into {} (checksums match)",
        install.join(", "),
        bin_dir.display()
    ))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut cmd = Command::new("sha256sum");
    cmd.arg(path).stdin(std::process::Stdio::null());
    let out = match cmd.output() {
        Ok(out) if out.status.success() => out,
        _ => {
            let out = Command::new("shasum")
                .args(["-a", "256"])
                .arg(path)
                .stdin(std::process::Stdio::null())
                .output()
                .with_context(|| format!("sha256 {}", path.display()))?;
            if !out.status.success() {
                bail!(
                    "sha256 {}: {}",
                    path.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            out
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let hash = text
        .split_whitespace()
        .next()
        .context("sha256 printed nothing")?;
    Ok(hash.to_ascii_lowercase())
}

fn run_installer(bin_dir: &Path) -> Result<()> {
    let script = installer_script()?;
    let mut cmd = Command::new("sh");
    cmd.arg(&script)
        .arg("--no-onboard")
        .arg("--no-embed")
        .arg("--bin-dir")
        .arg(bin_dir)
        .stdin(std::process::Stdio::null());
    let out = cmd
        .output()
        .with_context(|| format!("run {}", script.display()))?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr);
    let said = if said.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout)
    } else {
        said
    };
    bail!("install.sh: {}", said.trim());
}

fn installer_script() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("LJOS_INSTALL_SH").filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        if !path.is_file() {
            bail!("LJOS_INSTALL_SH {} is not a file", path.display());
        }
        return Ok(path);
    }
    let url = std::env::var("LJOS_INSTALL_URL").unwrap_or_else(|_| INSTALL_URL.to_string());
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let text = agent
        .get(&url)
        .call()
        .map_err(|e| anyhow::anyhow!("{url}: {e}"))?
        .into_string()
        .with_context(|| format!("read {url}"))?;
    let path = std::env::temp_dir().join(format!("ljos-install-{}.sh", std::process::id()));
    let mut file = fs::File::create(&path).with_context(|| format!("write {}", path.display()))?;
    file.write_all(text.as_bytes())?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

fn ensure_tracker(remote: &str, dest: &Path, seat: &str) -> Result<String> {
    if is_work_tree(dest) {
        ensure_identity(dest, seat)?;
        match crate::tracker_push::publish(dest) {
            Ok(()) => Ok(format!("tracker: updated {}", dest.display())),
            Err(e) => Ok(format!(
                "tracker: {} kept local; push queued: {e}",
                dest.display()
            )),
        }
    } else if dest.exists() {
        let empty = fs::read_dir(dest)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !empty {
            bail!(
                "tracker: {} exists and is not a git checkout",
                dest.display()
            );
        }
        fs::remove_dir(dest)?;
        clone_tracker(remote, dest, seat)
    } else {
        clone_tracker(remote, dest, seat)
    }
}

fn clone_tracker(remote: &str, dest: &Path, seat: &str) -> Result<String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = dest.parent().context("tracker path has no parent")?;
    let name = dest
        .file_name()
        .context("tracker path has no name")?
        .to_string_lossy()
        .to_string();
    let out = Command::new("git")
        .current_dir(parent)
        .args(["clone", "--quiet", remote, &name])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .context("git clone")?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        bail!("tracker: clone of {remote} failed: {}", said.trim());
    }
    ensure_identity(dest, seat)?;
    match crate::tracker_push::publish(dest) {
        Ok(()) => Ok(format!("tracker: cloned {remote} to {}", dest.display())),
        Err(e) => Ok(format!(
            "tracker: cloned {remote} to {}; push queued: {e}",
            dest.display()
        )),
    }
}

fn ensure_identity(dir: &Path, seat: &str) -> Result<()> {
    let email = git_local(dir, "user.email");
    if email.map(|v| v.is_empty()).unwrap_or(true) {
        git_set(
            dir,
            "user.email",
            &format!("{seat}@users.noreply.github.com"),
        )?;
        git_set(dir, "user.name", seat)?;
    }
    git_set(dir, "commit.gpgsign", "false")?;
    git_set(dir, "pull.rebase", "true")?;
    Ok(())
}

fn git_local(dir: &Path, key: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["config", "--local", "--get", key])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn git_set(dir: &Path, key: &str, value: &str) -> Result<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["config", "--local", key, value])
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("git config {key}"))?;
    if out.status.success() {
        Ok(())
    } else {
        bail!(
            "git config {key}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )
    }
}

fn is_work_tree(dir: &Path) -> bool {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdin(std::process::Stdio::null())
        .output()
    else {
        return false;
    };
    out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true"
}

fn write_mcp(
    home: &Path,
    server: &Path,
    seat: &str,
    key: &Path,
    tracker: Option<&Path>,
    remote: Option<&str>,
) -> Result<String> {
    if !server.is_file() {
        bail!(
            "mcp: {} does not exist, so the harness was not pointed at it",
            server.display()
        );
    }
    let mut env = serde_json::Map::new();
    env.insert("LJOS_SEAT".into(), serde_json::json!(seat));
    env.insert("VISSUE_AGENT".into(), serde_json::json!(seat));
    env.insert(
        "LJOS_SEAT_KEY".into(),
        serde_json::json!(key.display().to_string()),
    );
    if std::env::var("DEEDAR_HOST_SIGNING_KEY").ok().as_deref() != Some("off") {
        env.insert(
            "DEEDAR_HOST_SIGNING_KEY".into(),
            serde_json::json!(key.display().to_string()),
        );
    }
    if let Some(tracker) = tracker {
        env.insert("LJOS_TRACKER_GIT".into(), serde_json::json!("push"));
        env.insert(
            "VISSUE_ROOT".into(),
            serde_json::json!(tracker.display().to_string()),
        );
        env.insert(
            "ISSUE_ROOT".into(),
            serde_json::json!(tracker.display().to_string()),
        );
    }
    if let Some(remote) = remote {
        env.insert("LJOS_TRACKER_REMOTE".into(), serde_json::json!(remote));
    }
    let entry = serde_json::json!({
        "type": "stdio",
        "command": crate::MCP_SERVER_COMMAND,
        "args": [],
        "env": env,
    });
    let path = home.join(".cursor/mcp.json");
    crate::set_json_entry(&path, "/mcpServers/ljos", &entry)?;
    Ok(format!(
        "mcp: {} starts {} on PATH ({})",
        path.display(),
        crate::MCP_SERVER_COMMAND,
        server.display()
    ))
}

fn write_env(path: &Path, pairs: &[(&str, String)]) -> Result<()> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for (key, value) in pairs {
        if value.contains('\n') {
            bail!("{key} contains a newline");
        }
        let row = format!("{key}={value}");
        if let Some(existing) = lines.iter_mut().find(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with(&format!("{key}=")) || trimmed.starts_with(&format!("{key} ="))
        }) {
            *existing = row;
        } else {
            lines.push(row);
        }
    }
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn ensure_profile(home: &Path, bin_dir: &Path) -> Result<String> {
    let notes = [
        ensure_path_file(&home.join(".profile"), bin_dir, false)?,
        ensure_path_file(&home.join(".bashrc"), bin_dir, true)?,
    ];
    Ok(notes.join("; "))
}

fn ensure_cloud_guard_rules() -> Result<String> {
    let path =
        cloud_guard_rules_path().context("cloud up: no config home for cloud guard rules")?;
    let wanted = cloud_guard_rules();
    let have = crate::kept_rules(&path);
    if have == wanted {
        return Ok(format!(
            "guard: {} already has cloud seat refusals",
            path.display()
        ));
    }
    crate::keep_rules(&path, &wanted);
    let wrote = crate::kept_rules(&path);
    if wrote != wanted {
        bail!(
            "cloud up: could not write {} with the cloud seat refusals",
            path.display()
        );
    }
    Ok(format!(
        "guard: {} now denies cursor/* pushes and `gh pr create|merge`",
        path.display()
    ))
}

fn cloud_guard_rules_path() -> Option<PathBuf> {
    Some(
        crate::config_base()?
            .join("ljos")
            .join(CLOUD_GUARD_RULES_FILE),
    )
}

fn cloud_guard_rules() -> Vec<crate::Rule> {
    vec![
        crate::Rule {
            pattern: "re:git push(?:\\s+\\S+)*\\s(?:\\S*:)?(?:refs/heads/)?cursor/\\S+(?:\\s|$)"
                .into(),
            verdict: "deny".into(),
            reason: "cloud seat rule: never push to cursor/* refs from this machine".into(),
        },
        crate::Rule {
            pattern: "gh* pr create*".into(),
            verdict: "deny".into(),
            reason: "cloud seat rule: never open pull requests from this machine".into(),
        },
        crate::Rule {
            pattern: "gh* pr merge*".into(),
            verdict: "deny".into(),
            reason: "cloud seat rule: never merge pull requests from this machine".into(),
        },
    ]
}

fn path_export_line(bin_dir: &Path) -> String {
    format!("export PATH=\"{}:$PATH\"", bin_dir.display())
}

fn first_interactive_guard(text: &str) -> Option<usize> {
    let mut at = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let guard =
            (trimmed.contains("$PS1") || trimmed.contains("$-")) && trimmed.contains("return");
        if guard {
            return Some(at);
        }
        at += line.len();
    }
    None
}

fn ensure_path_file(path: &Path, bin_dir: &Path, before_interactive_guard: bool) -> Result<String> {
    let have = fs::read_to_string(path).unwrap_or_default();
    let line = path_export_line(bin_dir);
    if have.lines().any(|l| l.trim() == line) {
        return Ok(format!(
            "path: {} already names {}",
            path.display(),
            bin_dir.display()
        ));
    }
    let mut out = have;
    if before_interactive_guard {
        if let Some(at) = first_interactive_guard(&out) {
            let mut next = String::with_capacity(out.len() + line.len() + 1);
            next.push_str(&out[..at]);
            next.push_str(&line);
            next.push('\n');
            next.push_str(&out[at..]);
            out = next;
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&line);
            out.push('\n');
        }
    } else {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&line);
        out.push('\n');
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(format!(
        "path: {} adds {}",
        path.display(),
        bin_dir.display()
    ))
}

/// Prepend `bin_dir` to this process's PATH when it is not already there.
fn ensure_process_path(bin_dir: &Path) -> String {
    let dir = bin_dir.display().to_string();
    let current = std::env::var("PATH").unwrap_or_default();
    if current.split(':').any(|entry| entry == dir) {
        return format!("path: PATH already names {dir}");
    }
    let next = if current.is_empty() {
        dir.clone()
    } else {
        format!("{dir}:{current}")
    };
    crate::with_env_mutation(|| std::env::set_var("PATH", &next));
    format!("path: PATH starts with {dir}")
}

fn release_started(id: &str, holder: &str) -> Result<Option<String>, String> {
    let layout = vissue_core::Layout::resolve(None, None).map_err(|e| e.to_string())?;
    let found = vissue_core::store::find_by_id(&layout, id).map_err(|e| e.to_string())?;
    let Some((_, path, project)) = found else {
        return Ok(None);
    };
    vissue_core::store::with_issues_lock(&path, || {
        let mut doc = vissue_core::IssueDoc::parse_file(&project, &path)?;
        let Some(heading) = doc.headings.iter_mut().find(|h| h.id == id) else {
            return Ok(None);
        };
        let Some(who) = heading.claimed_by().map(str::to_string) else {
            return Ok(None);
        };
        if !crate::same_holder(&who, holder) {
            return Ok(None);
        }
        heading.release_claim();
        if heading.state == "STARTED" {
            heading.record_state_change("TODO");
        }
        doc.write()?;
        Ok(Some(format!(
            "tracker: {id} no longer claimed by {who}; STARTED handed back"
        )))
    })
    .map_err(|e| e.to_string())
}

fn holder_running(assignee: &str, issue: &str) -> bool {
    let Ok(entries) = fs::read_dir(crate::runtime_dir()) else {
        return false;
    };
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let Some(actor) = name.strip_prefix("hold-") else {
            continue;
        };
        if !hold_names_claim(actor, assignee, issue) {
            continue;
        }
        if let Some(hold) = crate::read_hold(actor) {
            if crate::hold_alive(&hold) {
                return true;
            }
        }
    }
    false
}

/// Whether this hold record is the one `ljos claim` wrote for `issue`.
///
/// The filename is the claim-graph actor, a hash, while the tracker stores
/// the seat under `:CLAIMED_BY:`. The record's first line is that seat and
/// its sixth line is the issue. A filename that is already the seat still
/// matches, which is how a hand-written hold keeps its claim. A live hold
/// for another issue does not keep this one.
fn hold_names_claim(actor: &str, assignee: &str, issue: &str) -> bool {
    let path = crate::runtime_dir().join(format!("hold-{actor}"));
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let record_assignee = lines.first().copied().unwrap_or("");
    let node = lines.get(5).copied().unwrap_or("");
    let name_ok =
        crate::same_holder(actor, assignee) || crate::same_holder(record_assignee, assignee);
    if !name_ok {
        return false;
    }
    if node.is_empty() {
        return crate::same_holder(actor, assignee);
    }
    node == issue
}

fn bin_dir(home: &Path) -> PathBuf {
    std::env::var_os("LJOS_BIN_DIR")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/bin"))
}

fn tracker_dir(home: &Path) -> PathBuf {
    std::env::var_os("LJOS_TRACKER_DIR")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share/ljos/tracker"))
}

fn seat_matches_file() -> bool {
    let Some(file) = env_file_value("LJOS_SEAT") else {
        return false;
    };
    std::env::var("LJOS_SEAT").ok().as_deref() == Some(file.as_str())
}

fn env_file_value(key: &str) -> Option<String> {
    let home = crate::home().ok()?;
    let text = fs::read_to_string(home.join(".config/ljos/env")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() == key {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn nonempty_var(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn agent_id() -> Option<String> {
    // A GitHub Actions job is one run, including when a cloud-agent id is
    // also present in the environment. The run id is the suffix.
    if std::env::var("GITHUB_ACTIONS").ok().is_some_and(|v| {
        let v = v.trim();
        !v.is_empty() && !matches!(v, "0" | "false")
    }) {
        if let Some(id) = id_slug("GITHUB_RUN_ID") {
            return Some(id);
        }
    }
    for key in [
        "CURSOR_AGENT_ID",
        "CURSOR_AGENT",
        "CLOUD_AGENT_ID",
        "AGENT_ID",
        "CURSOR_CONVERSATION_ID",
        "CODESPACE_NAME",
        "GITHUB_RUN_ID",
    ] {
        if let Some(id) = id_slug(key) {
            return Some(id);
        }
    }
    None
}

fn id_slug(key: &str) -> Option<String> {
    let id = nonempty_var(key)?;
    if matches!(id.as_str(), "0" | "1" | "true" | "false") {
        return None;
    }
    let slug = crate::seat_slug(&id);
    if slug.is_empty() || slug == "runner" {
        return None;
    }
    Some(slug.chars().take(16).collect())
}

fn harness_kind() -> &'static str {
    if std::env::var_os("CURSOR_AGENT").is_some()
        || std::env::var_os("CURSOR_AGENT_SOCKET").is_some()
        || std::env::var_os("CURSOR_CONVERSATION_ID").is_some()
        || std::env::var_os("CURSOR_AGENT_ID").is_some()
        || Path::new("/run/cursor/api.sock").exists()
    {
        "cursor"
    } else if std::env::var_os("CODESPACES").is_some()
        || std::env::var_os("CODESPACE_NAME").is_some()
    {
        "codespace"
    } else if std::env::var_os("GITHUB_ACTIONS").is_some()
        || std::env::var_os("GITLAB_CI").is_some()
    {
        "ci"
    } else {
        "vm"
    }
}

fn hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "vm".into())
}
