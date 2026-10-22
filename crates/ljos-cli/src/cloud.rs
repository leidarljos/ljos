//! One command for a fresh machine that should share the tracker.
//!
//! `ljos cloud up` installs the release binaries into `~/.local/bin`,
//! checks the published SHA-256 sums, puts that directory first on PATH,
//! writes the harness MCP command, and clones `LJOS_TRACKER_REMOTE`.
//! The command is the bare name when this directory is first, and the
//! absolute path when an earlier directory would win. It writes a webhook
//! wake for each `LJOS_WAKE_<SEAT>_URL`. Entries this command writes are
//! marked `source = "env"`; any other wake stays. When
//! `LJOS_CLOUD_AGE_SECRET` names an environment variable, it writes that
//! variable's age identity for scope `cloud` to a mode 0600 file. On a
//! cloud VM it also installs the release's `packset-embed` and the ONNX
//! Runtime it links. A later boot with the binaries already installed only
//! refreshes the clone.

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
/// The rule that refuses a push updating a `cursor/*` ref, read from the
/// push's argv by [`crate::cloud_guard::cursor_push`].
const CURSOR_PUSH: &str = "cloud-cursor-push";
/// The rule that refuses `gh pr create` and its alias `gh pr new`, read
/// from gh's argv by [`crate::cloud_guard::gh_pr`].
const GH_PR_CREATE: &str = "cloud-gh-pr-create";
/// The rule that refuses `gh pr merge`, read the same way.
const GH_PR_MERGE: &str = "cloud-gh-pr-merge";
/// The release workflow's encoder archives, the runtime before the binary
/// that links it. Each unpacks into the prefix above the binary directory.
const ENCODER_ARCHIVES: &[&str] = &["onnxruntime", "packset-embed"];
/// Where the encoder's rpath, `$ORIGIN/../lib/onnxruntime`, finds the runtime.
const ENCODER_LIB: &str = "lib/onnxruntime";
const ENCODER_SONAME: &str = "libonnxruntime.so.1";

/// Names the environment variable that holds the age identity for scope
/// `cloud`. The value is an identity file as `age-keygen` writes it, not a
/// path. The identity stays out of the tracker.
pub const CLOUD_AGE_SECRET: &str = "LJOS_CLOUD_AGE_SECRET";

/// An identity file longer than this is refused. `age-keygen` writes a
/// few hundred bytes.
const CLOUD_AGE_MAX: usize = 8192;

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
    lines.push(ensure_encoder(&bin_dir, force)?);
    let server = bin_dir.join("ljos-mcp");
    // The command is the bare name when this directory is first on PATH,
    // and the absolute path when an earlier directory would win. which()
    // still sees that earlier install until PATH moves, so the choice is
    // made first. Absent from PATH, the bare name stays once prepended.
    let early = (bin_dir_later_on_path(&bin_dir) && server.is_file())
        .then(|| crate::server_command_plan(&server, "cloud up"));
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
    let plan = early.unwrap_or_else(|| {
        if server.is_file() {
            crate::server_command_plan(&server, "cloud up")
        } else {
            crate::ServerCommandPlan {
                command: crate::MCP_SERVER_COMMAND.to_string(),
                note: None,
            }
        }
    });
    lines.push(write_mcp(
        &home,
        &server,
        &plan,
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
    lines.push(ensure_cloud_age()?);
    lines.extend(ensure_wakes()?);
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

/// The seat a run with id `run` takes when it boots with `LJOS_SEAT` set
/// to `base`, as [`default_seat`] names it there.
#[must_use]
pub fn run_seat(base: &str, run: &str) -> String {
    with_run(&crate::seat_slug(base), run_slug(run).as_deref())
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

/// Whether a `cloud age` row is the unset case: no secret was named and
/// no identity file is on disk. That row is `info`. A file with the wrong
/// mode, or a named secret that was not written, is a failure.
#[must_use]
pub(crate) fn cloud_age_is_unset(name: &str, state: &str) -> bool {
    name == "cloud age" && state.starts_with("unset;")
}

/// What `ljos doctor` says about the age identity for scope `cloud`.
#[must_use]
pub fn cloud_age_row() -> crate::Habitat {
    let path = crate::sync::cloud_identity_path();
    match inspect_cloud_age(&path) {
        CloudAge::Good => crate::Habitat {
            name: "cloud age",
            state: format!("scope cloud identity at {} (0600)", path.display()),
            ok: true,
        },
        CloudAge::BadMode(mode) => crate::Habitat {
            name: "cloud age",
            state: format!("{} is mode {mode:04o}; want 0600", path.display()),
            ok: false,
        },
        CloudAge::NotIdentity => crate::Habitat {
            name: "cloud age",
            state: format!("{} is not an age identity", path.display()),
            ok: false,
        },
        CloudAge::Symlink => crate::Habitat {
            name: "cloud age",
            state: format!("{} is a symlink; want a mode 0600 file", path.display()),
            ok: false,
        },
        CloudAge::Unreadable => crate::Habitat {
            name: "cloud age",
            state: format!("{} cannot be read", path.display()),
            ok: false,
        },
        CloudAge::Missing => missing_cloud_age(&path),
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

/// The cloud-up refusal `line` meets when it runs in `cwd`. A rule this
/// ljos names is read from the command's argv; any other rule in the file
/// is matched as a seat rule.
#[must_use]
pub(crate) fn guard_line(line: &str, cwd: Option<&Path>) -> Option<crate::Rule> {
    let path = cloud_guard_rules_path()?;
    let denies = crate::kept_rules(&path)
        .into_iter()
        .filter(|rule| rule.verdict == "deny");
    for rule in denies {
        match rule.pattern.as_str() {
            CURSOR_PUSH => {
                if let Some(why) = crate::cloud_guard::cursor_push(line, cwd) {
                    return Some(crate::Rule {
                        reason: format!("{} ({why})", rule.reason),
                        ..rule
                    });
                }
            }
            GH_PR_CREATE | GH_PR_MERGE => {
                let verbs: &[&str] = if rule.pattern == GH_PR_CREATE {
                    &["create", "new"]
                } else {
                    &["merge"]
                };
                if crate::cloud_guard::gh_pr(line, verbs) {
                    return Some(rule);
                }
            }
            _ => {
                if let Some(hit) = crate::verdict_for(std::slice::from_ref(&rule), line) {
                    return Some(hit.clone());
                }
            }
        }
    }
    None
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
    home: HomeExpand,
    /// Parse and read errors fail the row. Harness files are skipped when
    /// they are not the JSON this probe expects (Zed's settings are JSONC).
    strict: bool,
}

/// How a harness turns a command string into a path.
///
/// Codex stores `$HOME` as text. OpenCode substitutes `{env:VAR}` only.
/// A shell client expands `$HOME`, `${HOME}`, `${env:HOME}`, `${userHome}`
/// and a leading `~/`.
#[derive(Clone, Copy)]
enum HomeExpand {
    Literal,
    Opencode,
    Shell,
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
        |path: PathBuf, probe: McpProbe, strict: bool, advice: String, home: HomeExpand| {
            let kind = match &probe {
                McpProbe::Json(pointer) => format!("json:{pointer}"),
                McpProbe::Text(marker) => format!("text:{marker}"),
            };
            if seen.insert(format!("{kind}:{}", path.display())) {
                out.push(McpFile {
                    path,
                    probe,
                    advice,
                    home,
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
            HomeExpand::Shell,
        );
    }
    if let Ok(home) = crate::home() {
        push(
            home.join(".cursor/mcp.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            true,
            cursor_mcp_advice(),
            HomeExpand::Shell,
        );
        push(
            home.join(".claude.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            false,
            "ljos onboard --harness claude".into(),
            HomeExpand::Shell,
        );
    }
    for harness in harness_shapes() {
        let advice = format!("ljos onboard --harness {}", harness.name);
        let home = match harness.name.as_str() {
            "codex" => HomeExpand::Literal,
            "opencode" | "opencode-v2" => HomeExpand::Opencode,
            _ => HomeExpand::Shell,
        };
        if let (Some(config), Some(pointer)) = (&harness.config_json, &harness.json_pointer) {
            if !pointer.is_empty() {
                push(
                    crate::expand(config),
                    McpProbe::Json(pointer.clone()),
                    false,
                    advice.clone(),
                    home,
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
                    home,
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
        .map(expand_json_path);
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
    let window = entry_text(&text, marker)?;
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

/// The marker's own table, including dotted children such as
/// `[mcp_servers.ljos.env]`. A fixed window runs into the next server,
/// and `NODE_PATH` used to count as `PATH`.
fn entry_text(text: &str, marker: &str) -> Option<String> {
    if !text.contains(marker) {
        return None;
    }
    let header = marker.trim();
    if header.starts_with('[') && header.ends_with(']') && !header.contains('\n') {
        return toml_entry(text, header);
    }
    yaml_entry(text, marker)
}

fn toml_entry(text: &str, header: &str) -> Option<String> {
    let name = header.trim_matches(|c| c == '[' || c == ']').trim();
    if name.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut keeping = false;
    let mut seen = false;
    for line in text.split_inclusive('\n') {
        if let Some(table) = toml_table_name(line) {
            keeping = table == name
                || table
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with('.'));
            if keeping {
                seen = true;
            }
        }
        if keeping {
            out.push_str(line);
        }
    }
    seen.then_some(out)
}

fn toml_table_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    let header = trimmed.split('#').next().unwrap_or(trimmed).trim();
    let inner = if let Some(rest) = header.strip_prefix("[[") {
        rest.strip_suffix("]]")?
    } else if let Some(rest) = header.strip_prefix('[') {
        let rest = rest.strip_suffix(']')?;
        if rest.contains('[') {
            return None;
        }
        rest
    } else {
        return None;
    };
    let name = inner.trim();
    (!name.is_empty()).then_some(name)
}

fn yaml_entry(text: &str, marker: &str) -> Option<String> {
    let found = text.find(marker)?;
    let start = if marker.starts_with('\n') {
        found + 1
    } else {
        text[..found].rfind('\n').map_or(0, |at| at + 1)
    };
    let rest = &text[start..];
    let mut lines = rest.split_inclusive('\n');
    let first = lines.next()?;
    let indent = first.chars().take_while(|c| *c == ' ').count();
    let mut out = String::new();
    out.push_str(first);
    if !first.ends_with('\n') {
        out.push('\n');
    }
    for line in lines {
        if line.trim().is_empty() {
            out.push_str(line);
            continue;
        }
        let ind = line.chars().take_while(|c| *c == ' ').count();
        if ind <= indent {
            break;
        }
        out.push_str(line);
    }
    Some(out)
}

/// `${env:PATH}` and `$PATH` in a JSON `env.PATH`, the same kind of
/// expansion a command already applies to `${env:HOME}`.
fn expand_json_path(value: &str) -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    replace_dollar_path(&value.replace("${env:PATH}", &path), &path)
}

fn replace_dollar_path(value: &str, path: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find("$PATH") {
        let after = at + "$PATH".len();
        let boundary = rest[after..].chars().next().is_none_or(|c| !is_key_char(c));
        out.push_str(&rest[..at]);
        if boundary {
            out.push_str(path);
        } else {
            out.push_str("$PATH");
        }
        rest = &rest[after..];
    }
    out.push_str(rest);
    out
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn keyed_value(window: &str, key: &str) -> Option<String> {
    let mut rest = window;
    while let Some(at) = rest.find(key) {
        let before = at == 0 || !is_key_byte(rest.as_bytes()[at - 1]);
        let after_idx = at + key.len();
        let after = after_idx >= rest.len() || !is_key_byte(rest.as_bytes()[after_idx]);
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

fn is_key_byte(b: u8) -> bool {
    is_key_char(char::from(b))
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

/// Substitute the home tokens this harness actually expands.
fn expand_launch(command: &str, home: HomeExpand) -> String {
    match home {
        HomeExpand::Literal => command.to_string(),
        HomeExpand::Opencode => expand_opencode_home(command),
        HomeExpand::Shell => expand_shell_home(command),
    }
}

/// OpenCode substitutes `{env:HOME}` only. `$HOME`, `${HOME}`, `~/` and
/// `${env:HOME}` stay text: the docs say shell expressions are not expanded.
fn expand_opencode_home(command: &str) -> String {
    let Ok(home) = crate::home() else {
        return command.to_string();
    };
    let home = home.display().to_string();
    let token = "{env:HOME}";
    let mut out = String::new();
    let mut rest = command;
    while let Some(at) = rest.find(token) {
        let shell_form = at > 0 && rest.as_bytes()[at - 1] == b'$';
        if shell_form {
            out.push_str(&rest[..at + token.len()]);
        } else {
            out.push_str(&rest[..at]);
            out.push_str(&home);
        }
        rest = &rest[at + token.len()..];
    }
    out.push_str(rest);
    out
}

/// `$HOME`, `${HOME}`, `${env:HOME}`, `${userHome}` and a leading `~/`.
fn expand_shell_home(command: &str) -> String {
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
    let command = expand_launch(&launch.command, file.home);
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

/// `packset-embed` and the ONNX Runtime it links, from this version's
/// release, on a cloud VM only. Elsewhere the encoder is built from source.
/// A release without them leaves search lexical. Both sums are checked
/// before either archive is unpacked.
fn ensure_encoder(bin_dir: &Path, force: bool) -> Result<String> {
    if !cloud_vm() {
        return Ok("encoder: not a cloud VM, so packset-embed is not downloaded".into());
    }
    let Some(target) = encoder_target() else {
        return Ok(format!(
            "encoder: no release build of packset-embed for {}-{}; search stays lexical",
            std::env::consts::ARCH,
            std::env::consts::OS
        ));
    };
    let lib = bin_dir
        .parent()
        .context("cloud up: the binary directory has no parent for lib/")?
        .join(ENCODER_LIB);
    let bin = bin_dir.join("packset-embed");
    if !force && executable(&bin) && lib.join(ENCODER_SONAME).is_file() {
        return Ok(format!(
            "encoder: packset-embed already in {}",
            bin_dir.display()
        ));
    }
    let parent = lib.parent().context("cloud up: no lib directory")?;
    let stage = parent.join(format!(".onnxruntime-{}", std::process::id()));
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage).with_context(|| format!("create {}", stage.display()))?;
    let said = install_encoder(&stage, target, bin_dir, &lib);
    let _ = fs::remove_dir_all(&stage);
    said
}

fn install_encoder(stage: &Path, target: &str, bin_dir: &Path, lib: &Path) -> Result<String> {
    let mut archives = Vec::new();
    for base in ENCODER_ARCHIVES {
        let name = format!("{base}-{target}.tar.gz");
        let file = stage.join(&name);
        let sums = stage.join(format!("{name}.sha256"));
        match release_asset(&name, &file) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(format!(
                    "encoder: {} has no {name}; search stays lexical",
                    release_source()
                ))
            }
            Err(e) => return Ok(format!("encoder: {e}; search stays lexical")),
        }
        match release_asset(&format!("{name}.sha256"), &sums) {
            Ok(true) => {}
            Ok(false) => bail!("{name} has no published checksum; nothing was installed"),
            Err(e) => return Ok(format!("encoder: {e}; nothing was installed")),
        }
        let want = crate::upgrade::digest_of(&fs::read_to_string(&sums)?).with_context(|| {
            format!("{name}.sha256 is not a sha256 line; nothing was installed")
        })?;
        let got = sha256_file(&file)?;
        if got != want {
            bail!(
                "{name}: checksum mismatch (published {want}, file {got}); nothing was installed"
            );
        }
        archives.push(file);
    }
    let tree = stage.join("tree");
    fs::create_dir_all(&tree)?;
    for file in &archives {
        let out = Command::new("tar")
            .arg("-xzf")
            .arg(file)
            .arg("-C")
            .arg(&tree)
            .stdin(std::process::Stdio::null())
            .output()
            .context("tar")?;
        if !out.status.success() {
            bail!(
                "tar {}: {}; nothing was installed",
                file.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }
    let new_lib = tree.join(ENCODER_LIB);
    let new_bin = tree.join("bin/packset-embed");
    if !new_lib.join(ENCODER_SONAME).is_file() || !new_bin.is_file() {
        bail!("the encoder archives lack bin/packset-embed or {ENCODER_LIB}/{ENCODER_SONAME}; nothing was installed");
    }
    // A running encoder keeps the files it opened; the new ones are renamed in.
    let old = stage.join("old");
    if lib.exists() {
        fs::rename(lib, &old).with_context(|| format!("move {} aside", lib.display()))?;
    }
    if let Err(e) = fs::rename(&new_lib, lib) {
        let _ = fs::rename(&old, lib);
        return Err(e).with_context(|| format!("install {}", lib.display()));
    }
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&new_bin, fs::Permissions::from_mode(0o755))?;
    fs::create_dir_all(bin_dir)?;
    let bin = bin_dir.join("packset-embed");
    fs::rename(&new_bin, &bin).with_context(|| format!("install {}", bin.display()))?;
    Ok(format!(
        "encoder: installed {} and ONNX Runtime in {} (checksums match)",
        bin.display(),
        lib.display()
    ))
}

/// A cloud VM. A CI job is not one, and the box and a laptop build
/// packset-embed from source.
fn cloud_vm() -> bool {
    if github_actions() {
        return false;
    }
    nonempty_var("CURSOR_AGENT_ID").is_some()
        || nonempty_var("CLOUD_AGENT_ID").is_some()
        || Path::new("/run/cursor/api.sock").exists()
}

fn encoder_target() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

fn release_source() -> String {
    match std::env::var_os("LJOS_RELEASE_DIR").filter(|p| !p.is_empty()) {
        Some(dir) => Path::new(&dir).display().to_string(),
        None => format!("release v{}", env!("CARGO_PKG_VERSION")),
    }
}

/// An asset of this version's release into `to`, from `LJOS_RELEASE_DIR`
/// when it is set. `Ok(false)` when the release has no such asset.
fn release_asset(name: &str, to: &Path) -> Result<bool> {
    if let Some(dir) = std::env::var_os("LJOS_RELEASE_DIR").filter(|p| !p.is_empty()) {
        let src = Path::new(&dir).join(name);
        if !src.is_file() {
            return Ok(false);
        }
        fs::copy(&src, to).with_context(|| format!("copy {}", src.display()))?;
        return Ok(true);
    }
    let url = format!(
        "https://github.com/{}/releases/download/v{}/{name}",
        crate::upgrade::RELEASE_REPO,
        env!("CARGO_PKG_VERSION")
    );
    let resp = match ureq::get(&url).timeout(Duration::from_secs(300)).call() {
        Ok(resp) => resp,
        Err(ureq::Error::Status(404, _)) => return Ok(false),
        Err(e) => bail!("{url}: {e}"),
    };
    let mut file = fs::File::create(to).with_context(|| format!("write {}", to.display()))?;
    std::io::copy(
        &mut std::io::Read::take(resp.into_reader(), 512 * 1024 * 1024),
        &mut file,
    )
    .with_context(|| format!("read {url}"))?;
    Ok(true)
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
    plan: &crate::ServerCommandPlan,
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
    let command = plan.command.as_str();
    let entry = serde_json::json!({
        "type": "stdio",
        "command": command,
        "args": [],
        "env": env,
    });
    let path = home.join(".cursor/mcp.json");
    crate::set_json_entry(&path, "/mcpServers/ljos", &entry)?;
    let mut line = if command == crate::MCP_SERVER_COMMAND {
        format!(
            "mcp: {} starts {command} on PATH ({})",
            path.display(),
            server.display()
        )
    } else {
        format!(
            "mcp: {} starts {command} ({})",
            path.display(),
            server.display()
        )
    };
    if let Some(note) = &plan.note {
        line.push_str("; ");
        line.push_str(note);
    }
    Ok(line)
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

/// One `LJOS_WAKE_<SEAT>_URL` read from the environment.
struct WakeVar {
    name: String,
    mid: String,
    url: String,
}

/// A webhook wake for each `LJOS_WAKE_<SEAT>_URL`, keyed by the variable
/// `LJOS_WAKE_<SEAT>_KEY`. The launcher passes the key by name, so it is
/// read when the wake fires and never written down. `GROK_BOT` is the seat
/// `grok-bot`. The lines name the variables and not the URL.
///
/// Two variables whose names slug to one seat are refused, and the line
/// names both. An entry without `source = "env"` was not written by this
/// command and is left in place. Entries this command wrote are written
/// again.
fn ensure_wakes() -> Result<Vec<String>> {
    let existing = crate::wake::load();
    let mut lines = Vec::new();
    for (seat, group) in wake_vars_by_seat() {
        if group.len() > 1 {
            lines.push(collision_line(&seat, &group));
            continue;
        }
        let one = &group[0];
        if crate::wake::check_webhook(&one.url).is_err() {
            lines.push(format!(
                "wake {seat}: {} is not https, nor http on this machine, so no wake was written",
                one.name
            ));
            continue;
        }
        let hand_set = existing
            .get(&seat)
            .is_some_and(|wake| wake.source.as_deref() != Some(crate::wake::ENV_SOURCE));
        if hand_set {
            lines.push(format!(
                "wake {seat}: left the wake cloud up did not write; {} did not replace it",
                one.name
            ));
            continue;
        }
        let key_env = format!("LJOS_WAKE_{}_KEY", one.mid);
        crate::wake::set_env_webhook(&seat, &one.url, &key_env)?;
        let unset = if nonempty_var(&key_env).is_some() {
            ""
        } else {
            ", which is unset now"
        };
        lines.push(format!(
            "wake {seat}: posts to {} with the key in {key_env}{unset}",
            one.name
        ));
    }
    Ok(lines)
}

fn wake_vars_by_seat() -> BTreeMap<String, Vec<WakeVar>> {
    let vars: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect();
    let mut by_seat: BTreeMap<String, Vec<WakeVar>> = BTreeMap::new();
    for (name, value) in vars {
        let Some(mid) = name
            .strip_prefix("LJOS_WAKE_")
            .and_then(|rest| rest.strip_suffix("_URL"))
        else {
            continue;
        };
        let url = value.trim();
        if url.is_empty() || !mid.bytes().any(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        let seat = crate::seat_slug(mid);
        by_seat.entry(seat).or_default().push(WakeVar {
            mid: mid.to_string(),
            url: url.to_string(),
            name,
        });
    }
    by_seat
}

fn collision_line(seat: &str, group: &[WakeVar]) -> String {
    let mut names: Vec<&str> = group.iter().map(|var| var.name.as_str()).collect();
    names.sort_unstable();
    format!(
        "wake {seat}: {} name the same seat, so no wake was written",
        list_names(&names)
    )
}

fn list_names(names: &[&str]) -> String {
    let Some((last, rest)) = names.split_last() else {
        return String::new();
    };
    if rest.is_empty() {
        return (*last).to_string();
    }
    let mut line = rest.join(", ");
    line.push_str(" and ");
    line.push_str(last);
    line
}

fn ensure_profile(home: &Path, bin_dir: &Path) -> Result<String> {
    let notes = [
        ensure_path_file(&home.join(".profile"), bin_dir, false)?,
        ensure_path_file(&home.join(".bashrc"), bin_dir, true)?,
    ];
    Ok(notes.join("; "))
}

/// Whether this machine is a cloud seat: `ljos cloud up` wrote the guard
/// file, and the file still holds the refusals that command writes. A box
/// that never ran it has no such file, so finish exports no deeds.
#[must_use]
pub(crate) fn cloud_seat() -> bool {
    cloud_guard_rules_path().is_some_and(|path| crate::kept_rules(&path) == cloud_guard_rules())
}

pub(crate) fn ensure_cloud_guard_rules() -> Result<String> {
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
            pattern: CURSOR_PUSH.into(),
            verdict: "deny".into(),
            reason: "cloud seat rule: never push to cursor/* refs from this machine".into(),
        },
        crate::Rule {
            pattern: GH_PR_CREATE.into(),
            verdict: "deny".into(),
            reason: "cloud seat rule: never open pull requests from this machine".into(),
        },
        crate::Rule {
            pattern: GH_PR_MERGE.into(),
            verdict: "deny".into(),
            reason: "cloud seat rule: never merge pull requests from this machine".into(),
        },
    ]
}

/// `LJOS_CLOUD_AGE_SECRET` is the name of another variable. The identity
/// itself is never a legal name, so a value pasted into this variable is
/// refused without being written or echoed.
enum NamedSecret {
    Unset,
    Bad(&'static str),
    Name(String),
}

fn named_secret() -> NamedSecret {
    let Ok(raw) = std::env::var(CLOUD_AGE_SECRET) else {
        return NamedSecret::Unset;
    };
    let name = raw.trim();
    if name.is_empty() {
        return NamedSecret::Unset;
    }
    if raw.contains("AGE-SECRET-KEY-") || raw.contains('\n') || raw.contains('\0') {
        return NamedSecret::Bad("holds an age identity; set it to the variable's name");
    }
    if name == CLOUD_AGE_SECRET {
        return NamedSecret::Bad("cannot name itself");
    }
    if !valid_env_name(name) {
        return NamedSecret::Bad("is not an environment variable name");
    }
    NamedSecret::Name(name.to_string())
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    name.len() <= 128 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

enum CloudAge {
    Missing,
    Good,
    BadMode(u32),
    NotIdentity,
    Symlink,
    Unreadable,
}

fn inspect_cloud_age(path: &Path) -> CloudAge {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return CloudAge::Missing,
        Err(_) => return CloudAge::Unreadable,
    };
    if meta.file_type().is_symlink() {
        return CloudAge::Symlink;
    }
    if !meta.is_file() || meta.len() > CLOUD_AGE_MAX as u64 {
        return CloudAge::NotIdentity;
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return CloudAge::Unreadable,
    };
    if !crate::sync::age_identity(&text) {
        return CloudAge::NotIdentity;
    }
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    if mode == 0o600 {
        CloudAge::Good
    } else {
        CloudAge::BadMode(mode)
    }
}

fn missing_cloud_age(path: &Path) -> crate::Habitat {
    match named_secret() {
        NamedSecret::Unset => crate::Habitat {
            name: "cloud age",
            state: format!(
                "unset; no identity for scope cloud. Set {CLOUD_AGE_SECRET} to the secret's name and run `ljos cloud up`"
            ),
            ok: true,
        },
        NamedSecret::Bad(why) => crate::Habitat {
            name: "cloud age",
            state: format!("{CLOUD_AGE_SECRET} {why}"),
            ok: false,
        },
        NamedSecret::Name(name) => {
            let problem = secret_problem(&name);
            let state = match problem {
                Some(problem) => format!("{name} {problem}, and {} is missing", path.display()),
                None => format!(
                    "{name} is set, and {} is missing. Run `ljos cloud up`",
                    path.display()
                ),
            };
            crate::Habitat {
                name: "cloud age",
                state,
                ok: false,
            }
        }
    }
}

/// What is wrong with the named variable, when it cannot be written.
/// `None` when it holds an age identity. The value is not returned.
fn secret_problem(name: &str) -> Option<&'static str> {
    match std::env::var(name) {
        Err(std::env::VarError::NotPresent) => Some("is unset"),
        Err(std::env::VarError::NotUnicode(_)) => Some("is not text"),
        Ok(value) if value.trim().is_empty() => Some("is empty"),
        Ok(value) if value.len() > CLOUD_AGE_MAX => Some("is not an age identity"),
        Ok(value) if !crate::sync::age_identity(&normalize_identity(&value)) => {
            Some("is not an age identity")
        }
        Ok(_) => None,
    }
}

fn normalize_identity(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::new();
    for line in text.trim().lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn ensure_cloud_age() -> Result<String> {
    let name = match named_secret() {
        NamedSecret::Unset => {
            return Ok(format!(
                "cloud age: {CLOUD_AGE_SECRET} is unset, so scope cloud has no identity on this machine"
            ));
        }
        NamedSecret::Bad(why) => bail!("cloud up: {CLOUD_AGE_SECRET} {why}"),
        NamedSecret::Name(name) => name,
    };
    let value = match std::env::var(&name) {
        Err(std::env::VarError::NotPresent) => {
            bail!("cloud up: {name} is unset, so the scope cloud identity was not written")
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("cloud up: {name} is not text, so the scope cloud identity was not written")
        }
        Ok(value) => value,
    };
    if value.trim().is_empty() {
        bail!("cloud up: {name} is empty, so the scope cloud identity was not written");
    }
    if value.len() > CLOUD_AGE_MAX {
        bail!("cloud up: {name} is too long to be an age identity");
    }
    let text = normalize_identity(&value);
    if !crate::sync::age_identity(&text) {
        bail!(
            "cloud up: {name} is not an age identity, so the scope cloud identity was not written"
        );
    }
    let path = crate::sync::cloud_identity_path();
    match install_identity(&path, &text)? {
        Install::Wrote => Ok(format!("cloud age: wrote {} mode 0600", path.display())),
        Install::Mode => Ok(format!("cloud age: set {} to mode 0600", path.display())),
        Install::Same => Ok(format!(
            "cloud age: {} already holds the scope cloud identity (0600)",
            path.display()
        )),
    }
}

enum Install {
    Wrote,
    Mode,
    Same,
}

fn install_identity(path: &Path, text: &str) -> Result<Install> {
    // Follow a symlink and the identity would be written into the target,
    // or the target's mode would change. Replace the link instead.
    let regular = fs::symlink_metadata(path)
        .ok()
        .is_some_and(|meta| meta.file_type().is_file());
    if regular && fs::read_to_string(path).ok().as_deref() == Some(text) {
        if file_mode(path) == Some(0o600) {
            return Ok(Install::Same);
        }
        set_mode_0600(path)?;
        return Ok(Install::Mode);
    }
    write_private(path, text)?;
    Ok(Install::Wrote)
}

fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.file_type().is_file())
        .map(|meta| meta.permissions().mode() & 0o777)
}

fn set_mode_0600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("mode 0600 {}", path.display()))
}

/// Write `text` to `path` as mode 0600. A symlink at `path` is replaced,
/// so the identity is not stored through a link to another file.
fn write_private(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("cloud.age");
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let write = (|| -> Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("write {}", tmp.display()))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        set_mode_0600(&tmp)?;
        fs::rename(&tmp, path).with_context(|| format!("install {}", path.display()))?;
        set_mode_0600(path)?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write
}

fn path_export_line(bin_dir: &Path) -> String {
    format!("export PATH=\"{}:$PATH\"", bin_dir.display())
}

fn first_interactive_guard(text: &str) -> Option<usize> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut at = 0usize;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let one_line =
            (trimmed.contains("$PS1") || trimmed.contains("$-")) && trimmed.contains("return");
        // Ubuntu's skel bashrc splits the test and the return:
        // `case $- in` / `*i*) ;;` / `*) return;;` / `esac`.
        // A one-line search appends PATH after that return, so a
        // non-interactive shell never sees it.
        if one_line || (case_tests_shell(trimmed) && case_body_returns(&lines[i + 1..])) {
            return Some(at);
        }
        at += line.len();
    }
    None
}

/// `case $- in` or `case "$PS1" in`, the interactive test on its own line.
fn case_tests_shell(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix("case") else {
        return false;
    };
    let rest = rest.trim_start();
    let word = rest.split_whitespace().next().unwrap_or("");
    let word = word.trim_matches(|c| c == '"' || c == '\'');
    word == "$-" || word == "$PS1"
}

/// Whether this `case` returns before `esac`. A `case "$TERM"` does not.
fn case_body_returns(lines: &[&str]) -> bool {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("esac") {
            return false;
        }
        if trimmed.contains("return") {
            return true;
        }
    }
    false
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

/// True when `bin_dir` is on PATH and some other directory comes first.
/// `which` would then run that earlier copy.
fn bin_dir_later_on_path(bin_dir: &Path) -> bool {
    let dir = bin_dir.display().to_string();
    let current = std::env::var("PATH").unwrap_or_default();
    let mut saw_other = false;
    for entry in current.split(':').filter(|entry| !entry.is_empty()) {
        if entry == dir {
            return saw_other;
        }
        saw_other = true;
    }
    false
}

/// Put `bin_dir` first on this process's PATH and drop a later copy.
/// Presence is not enough: an earlier directory hides this install.
fn ensure_process_path(bin_dir: &Path) -> String {
    let dir = bin_dir.display().to_string();
    let current = std::env::var("PATH").unwrap_or_default();
    let entries: Vec<&str> = current
        .split(':')
        .filter(|entry| !entry.is_empty())
        .collect();
    if entries.first() == Some(&dir.as_str()) {
        return format!("path: PATH already starts with {dir}");
    }
    let mut next = dir.clone();
    for entry in entries.into_iter().filter(|entry| *entry != dir) {
        next.push(':');
        next.push_str(entry);
    }
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

/// `ljos cloud up` on a cloud VM, codespace, or CI job. A desktop Cursor
/// file is onboarded with the cursor harness; the api socket on a cloud
/// image is not enough, because doctor is often aimed at another home.
fn cursor_mcp_advice() -> String {
    if on_cloud_vm() {
        "ljos cloud up".into()
    } else {
        "ljos onboard --harness cursor".into()
    }
}

fn on_cloud_vm() -> bool {
    if agent_id().is_some() {
        return true;
    }
    match harness_kind() {
        "codespace" | "ci" => return true,
        "cursor" if cursor_agent_env() => return true,
        _ => {}
    }
    // harness_kind answers "cursor" when /run/cursor/api.sock exists, and
    // that hides CI. The variables themselves are still this job.
    if std::env::var_os("GITHUB_ACTIONS").is_some()
        || std::env::var_os("GITLAB_CI").is_some()
        || std::env::var_os("CODESPACES").is_some()
        || std::env::var_os("CODESPACE_NAME").is_some()
    {
        return true;
    }
    cloud_env_file_seat()
}

fn cursor_agent_env() -> bool {
    [
        "CURSOR_AGENT",
        "CURSOR_AGENT_SOCKET",
        "CURSOR_CONVERSATION_ID",
        "CURSOR_AGENT_ID",
        "CLOUD_AGENT_ID",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some())
}

/// `cloud up` records `cursor-ID`, `ci-ID`, or `codespace-ID` in the env
/// file. A later shell without the agent variables still is that VM.
fn cloud_env_file_seat() -> bool {
    let Some(seat) = env_file_value("LJOS_SEAT") else {
        return false;
    };
    ["cursor-", "ci-", "codespace-"]
        .iter()
        .any(|prefix| seat.starts_with(prefix))
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
    if github_actions() {
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

fn github_actions() -> bool {
    std::env::var("GITHUB_ACTIONS").ok().is_some_and(|v| {
        let v = v.trim();
        !v.is_empty() && !matches!(v, "0" | "false")
    })
}

fn id_slug(key: &str) -> Option<String> {
    run_slug(&nonempty_var(key)?)
}

fn run_slug(id: &str) -> Option<String> {
    let id = id.trim();
    if matches!(id, "0" | "1" | "true" | "false") {
        return None;
    }
    let slug = crate::seat_slug(id);
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
