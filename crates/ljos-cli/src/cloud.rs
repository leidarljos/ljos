//! One command for a fresh machine that should share the tracker.
//!
//! `ljos cloud up` installs the release binaries into `~/.local/bin`,
//! checks the published SHA-256 sums, puts that directory first on PATH,
//! writes the harness MCP command, and clones `LJOS_TRACKER_REMOTE`.
//! The command is always the absolute path, so a client with another PATH
//! still starts this install. It writes a
//! webhook wake for each `LJOS_WAKE_<SEAT>_URL`. Entries this command
//! writes are marked `source = "env"`; any other wake stays. When
//! `LJOS_CLOUD_AGE_SECRET` names an environment variable, it writes that
//! variable's age identity for scope `cloud` to a mode 0600 file. On a
//! cloud VM it also installs the release's `packset-embed` and the ONNX
//! Runtime it links. A later boot with the binaries already installed
//! refreshes the clone, rewrites env-owned wakes, and refreshes the scope
//! cloud identity from the named secret.

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
/// The one cloud push rule, read from the push's argv by
/// [`crate::cloud_guard::cursor_push`].
const CURSOR_PUSH: &str = "cloud-cursor-push";
/// The release workflow's encoder archives, the runtime before the binary
/// that links it. Each unpacks into the prefix above the binary directory.
const ENCODER_ARCHIVES: &[&str] = &["onnxruntime", "packset-embed"];
/// Where the encoder's rpath, `$ORIGIN/../lib/onnxruntime`, finds the runtime.
const ENCODER_LIB: &str = "lib/onnxruntime";
#[cfg(target_os = "macos")]
const ENCODER_SONAME: &str = "libonnxruntime.1.dylib";
#[cfg(not(target_os = "macos"))]
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
    up_at(force, false)
}

/// Install binaries, point the harness at them, and clone the shared tracker.
///
/// `repoint` replaces the repository an earlier `cloud up` recorded. The
/// shell guard refuses that flag, so a seat cannot move its own allowlist.
///
/// # Errors
///
/// As [`up`]. A second repository is left unrecorded when `repoint` is false.
pub fn up_at(force: bool, repoint: bool) -> Result<String> {
    let home = crate::home().context("cloud up: HOME is unset")?;
    let bin_dir = bin_dir(&home);
    let mut lines = Vec::new();
    lines.push(ensure_binaries(&bin_dir, force)?);
    lines.push(ensure_encoder(&bin_dir, force)?);
    let server = bin_dir.join("ljos-mcp");
    // The harness spawns servers with a PATH of its own, which no edited
    // profile reaches, so the entry names the install by its absolute path.
    // The note still says what the PATH cloud up started with would run.
    let mut plan = crate::server_command_plan(&server, "cloud up");
    plan.command = server.display().to_string();
    lines.push(ensure_process_path(&bin_dir));

    if agent_id().is_some() && seat_matches_file() {
        // A seat left in the env file by an earlier boot must not hide this
        // boot's agent id. An exported seat that differs from the file stays.
        crate::with_env_mutation(|| std::env::remove_var("LJOS_SEAT"));
    }
    let seat = default_seat();
    lines.push(format!("seat: {seat}"));
    let key = prepare_seat_key(&seat)?;
    let public = crate::ballot_sig::key_from_file(&key)?.verifying_key();
    lines.push(format!(
        "seat key: {}, public {}",
        key.display(),
        hex::encode(public.to_bytes())
    ));

    let dest = tracker_dir(&home);
    let found = crate::tracker_attach::find(nonempty_var("LJOS_TRACKER_REMOTE").as_deref(), &dest);
    let attached = match &found {
        Some(remote) => {
            let (line, ok) = ensure_tracker(remote, &dest, &seat, &home)?;
            lines.push(line);
            ok
        }
        None => {
            lines.push(
                "tracker: LJOS_TRACKER_REMOTE is unset and no tracker checkout is in the workspace, so this machine has no shared tracker".into(),
            );
            false
        }
    };
    let remote = found.as_ref().filter(|_| attached).map(|r| r.shown.clone());
    let tracker = remote.as_ref().map(|_| dest.clone());
    lines.push(ensure_profile(&home, &bin_dir)?);
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
        // Tag this VM's memory and lessons scope:cloud. Not default_scope:
        // an unscoped box atom must not follow a default into the cloud log.
        (
            crate::sync::WRITE_SCOPE_KEY,
            crate::sync::CLOUD_SCOPE.to_string(),
        ),
        (crate::sync::SYNC_HOST_KEY, seat.clone()),
    ];
    if std::env::var("DEEDAR_HOST_SIGNING_KEY").ok().as_deref() != Some("off") {
        pairs.push(("DEEDAR_HOST_SIGNING_KEY", key.display().to_string()));
    }
    if let (Some(remote), Some(dest)) = (remote.as_deref(), tracker.as_deref()) {
        pairs.push(("LJOS_TRACKER_GIT", "push".into()));
        pairs.push(("LJOS_TRACKER_REMOTE", remote.to_string()));
        pairs.push(("VISSUE_ROOT", dest.display().to_string()));
        pairs.push(("ISSUE_ROOT", dest.display().to_string()));
    }
    write_env(&home.join(".config/ljos/env"), &pairs)?;
    crate::with_env_mutation(|| {
        for (k, v) in &pairs {
            std::env::set_var(k, v);
        }
    });
    lines.push(ensure_cloud_guard_rules()?);
    // After the rule file, so a second `cloud up` still says the refusals
    // are already installed. Git runs this hook for every push; the rule
    // file cannot see a push a script starts.
    lines.push(crate::cloud_hook::ensure(repoint)?);
    // Retire env-owned wakes before any later step can fail, so a removed
    // `LJOS_WAKE_<SEAT>_URL` cannot leave a stale URL that still sees a new key.
    lines.extend(ensure_wakes()?);
    lines.push(ensure_cloud_age()?);
    let reaped = reap_gone_owners(Some(&seat), crate::lease_secs());
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
///
/// # Errors
///
/// `base` already ends with the run id. [`default_seat`] keeps such a
/// seat as it is, so a boot that runs `cloud up` again does not append
/// the id twice, and the bare name on that run boots as the same seat.
pub fn run_seat(base: &str, run: &str) -> Result<String> {
    let base = crate::seat_slug(base);
    let id = run_slug(run);
    if let Some(id) = id.as_deref() {
        if let Some(bare) = base.strip_suffix(id).and_then(|b| b.strip_suffix('-')) {
            bail!(
                "{base} already ends with run id {id}, and {bare} on that run boots as {base} \
                 too; name the seat as LJOS_SEAT holds it before `cloud up` appends the id"
            );
        }
    }
    Ok(with_run(&base, id.as_deref()))
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

/// What `ljos doctor` says about the one cloud push rule. A marked seat
/// is `ok` when the pre-push hook is installed. An unmarked machine is
/// told to run `ljos cloud up`.
#[must_use]
pub fn guard_row() -> crate::Habitat {
    let Some(path) = crate::cloud_hook::allow_path() else {
        return crate::Habitat {
            name: "cloud guard",
            state: "no config home for the cloud seat".into(),
            ok: false,
        };
    };
    if !path.is_file() {
        return crate::Habitat {
            name: "cloud guard",
            state: format!(
                "missing in {}; run `ljos cloud up` to install the cloud seat push rule",
                path.display()
            ),
            ok: false,
        };
    }
    let base = format!(
        "{} denies a push outside this seat's named branches and develop",
        path.display()
    );
    match crate::cloud_hook::hook_line() {
        Ok(line) => crate::Habitat {
            name: "cloud guard",
            state: format!("{base}; {line}"),
            ok: true,
        },
        Err(why) => crate::Habitat {
            name: "cloud guard",
            state: format!("{base}; {why}"),
            ok: false,
        },
    }
}

/// Whether a cloud-guard doctor row should print `warn`. A missing file
/// stays `info`.
#[must_use]
pub(crate) fn guard_row_is_warning(state: &str) -> bool {
    state.contains("pre-push hook")
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

/// The one cloud rule: a push from a cloud seat may update only that
/// seat's named branches and `develop`, and may not force or delete a ref.
/// There is no refusal file. A machine `ljos cloud up` has not marked is
/// not a cloud seat, so this says nothing.
#[must_use]
pub(crate) fn guard_line(line: &str, cwd: Option<&Path>) -> Option<crate::Rule> {
    if !cloud_seat() {
        return None;
    }
    let why = crate::cloud_guard::cursor_push(line, cwd)?;
    Some(crate::Rule {
        pattern: CURSOR_PUSH.into(),
        verdict: "deny".into(),
        reason: format!("cloud seat rule: {why}"),
    })
}

/// git's org merge driver. The merged text replaces `ours`.
///
/// # Errors
///
/// An issue file did not parse, or the result still has conflict markers.
pub fn merge_org_cli(base: &Path, ours: &Path, theirs: &Path) -> Result<()> {
    crate::tracker_push::merge_org_files(base, ours, theirs).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Hand back `STARTED` claims this machine made whose owner is gone and
/// whose claim is older than `lease` seconds. A claim is this machine's
/// when a hold record here names it. Another machine's claim has no record
/// here, so it is never released from here. `seat` narrows the release to
/// that seat's claims. No-op unless `LJOS_TRACKER_GIT=push`. The release
/// is committed and pushed like any other tracker write.
#[must_use]
pub fn reap_gone_owners(seat: Option<&str>, lease: u64) -> String {
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
        if seat.is_some_and(|seat| {
            crate::canonical_seat_name(&who) != crate::canonical_seat_name(seat)
        }) {
            continue;
        }
        if local_hold(&who, &heading.id) != Some(false) {
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
    expand: Expand,
    /// The PATH the client searches for a bare command when that is not a
    /// shell's: launchd's, for a macOS app opened from the Dock.
    launch_path: Option<String>,
    /// Parse and read errors fail the row. Harness files are skipped when
    /// they are not the JSON this probe expects (Zed's settings are JSONC).
    strict: bool,
}

/// How a harness substitutes variables in a command and its env.PATH.
///
/// Codex stores `$HOME` as text. OpenCode substitutes `{env:VAR}` only.
/// Cursor substitutes `${env:VAR}` and `${userHome}`. Claude Code
/// substitutes `${VAR}` and `${VAR:-default}` and keeps an unset `${VAR}`
/// as written. Any other client is read as a shell: `$HOME`, `${HOME}`,
/// `${env:HOME}`, `${userHome}` and a leading `~/` in a command, and
/// `$PATH`, `${PATH}` and `${env:PATH}` in env.PATH.
#[derive(Clone, Copy)]
enum Expand {
    Literal,
    Opencode,
    Cursor,
    Claude,
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
    collect_mcp_files(true)
}

/// `include_user_harness` reads `harnesses.toml`. The seat-key scan leaves
/// it out: that file is not the MCP config, and a special file there must
/// not stall a hook.
fn collect_mcp_files(include_user_harness: bool) -> Vec<McpFile> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut push = |path: PathBuf,
                    probe: McpProbe,
                    strict: bool,
                    advice: String,
                    expand: Expand,
                    launch_path: Option<&str>| {
        let kind = match &probe {
            McpProbe::Json(pointer) => format!("json:{pointer}"),
            McpProbe::Text(marker) => format!("text:{marker}"),
        };
        if seen.insert(format!("{kind}:{}", path.display())) {
            out.push(McpFile {
                path,
                probe,
                advice,
                expand,
                launch_path: launch_path.map(str::to_string),
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
            Expand::Shell,
            None,
        );
    }
    if let Ok(home) = crate::home() {
        push(
            home.join(".cursor/mcp.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            true,
            cursor_mcp_advice(),
            Expand::Cursor,
            None,
        );
        push(
            home.join(".claude.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            false,
            "ljos onboard --harness claude".into(),
            Expand::Claude,
            None,
        );
    }
    for harness in harness_shapes_for(include_user_harness) {
        let advice = format!("ljos onboard --harness {}", harness.name);
        let expand = match harness.name.as_str() {
            "codex" => Expand::Literal,
            "opencode" | "opencode-v2" => Expand::Opencode,
            "cursor" => Expand::Cursor,
            _ => Expand::Shell,
        };
        if let (Some(config), Some(pointer)) = (harness.json_config(), &harness.json_pointer) {
            if !pointer.is_empty() {
                push(
                    crate::expand(config),
                    McpProbe::Json(pointer.clone()),
                    false,
                    advice.clone(),
                    expand,
                    harness.launch_path(),
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
                    expand,
                    harness.launch_path(),
                );
            }
        }
    }
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|p| !p.is_empty()) {
        push(
            PathBuf::from(&dir).join(".claude.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            false,
            "ljos onboard --harness claude".into(),
            Expand::Claude,
            None,
        );
    }
    if let Some(dir) = std::env::var_os("CODEX_HOME").filter(|p| !p.is_empty()) {
        push(
            PathBuf::from(&dir).join("config.toml"),
            McpProbe::Text("[mcp_servers.ljos]".into()),
            false,
            "ljos onboard --harness codex".into(),
            Expand::Literal,
            None,
        );
    }
    for dir in project_config_dirs() {
        push(
            dir.join(".mcp.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            false,
            "project .mcp.json".into(),
            Expand::Shell,
            None,
        );
        push(
            dir.join(".cursor/mcp.json"),
            McpProbe::Json("/mcpServers/ljos".into()),
            false,
            "project .cursor/mcp.json".into(),
            Expand::Cursor,
            None,
        );
    }
    out
}

/// The working directory and, when it sits inside a checkout, that
/// checkout's root. A directory that is already gone is left out: the
/// hook must not stall on it.
fn project_config_dirs() -> Vec<PathBuf> {
    let Ok(cwd) = std::env::current_dir() else {
        return Vec::new();
    };
    if !cwd.is_dir() {
        return Vec::new();
    }
    let mut dirs = vec![cwd.clone()];
    let mut at = cwd.clone();
    loop {
        if at.join(".git").exists() {
            if at != cwd {
                dirs.push(at);
            }
            break;
        }
        if !at.pop() {
            break;
        }
    }
    dirs
}

fn harness_shapes_for(include_user_harness: bool) -> Vec<crate::Harness> {
    let shipped: crate::Harnesses = toml::from_str(crate::HARNESSES_EXAMPLE).unwrap_or_default();
    let mut shapes = if include_user_harness {
        crate::harnesses_with_shipped(&crate::harnesses_path())
            .map(|h| h.harness)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
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

/// The most of an MCP config the seat-key scan reads. A larger file is
/// not a config this hook will trust.
const MCP_CONFIG_CAP: u64 = 1024 * 1024;

/// Server keys whose command is this seat's MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SeatMcpKeys {
    /// No config file was there.
    Unknown,
    /// A file that has to parse could not be read.
    Ambiguous,
    /// Keys found in the files that were read.
    Known(BTreeSet<String>),
}

/// Keys in the seat's MCP config whose command is `ljos-mcp`.
/// An absent config is [`SeatMcpKeys::Unknown`]. A strict file that is
/// not readable JSON is [`SeatMcpKeys::Ambiguous`] only when no other
/// file or plugin install named a key. One unreadable file hides its
/// own entries and nothing else. Harness files that are not the JSON
/// or TOML this scan expects are skipped.
///
/// A seat key comes from the seat's own files: the home MCP configs,
/// the harness files, `LJOS_MCP_CONFIG`, and a plugin installed under
/// the home `.claude` directory. A project `.mcp.json` or
/// `.cursor/mcp.json`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, and a
/// parent's `--mcp-config` argument do not add a key.
pub(crate) fn seat_mcp_keys() -> SeatMcpKeys {
    let mut keys = BTreeSet::new();
    let mut saw = false;
    let mut ambiguous = false;
    for file in collect_mcp_files(false) {
        if !seat_config_file(&file.path) {
            continue;
        }
        match read_seat_keys(&file) {
            KeyRead::Absent | KeyRead::Skip => {}
            KeyRead::Ambiguous => ambiguous = true,
            KeyRead::Keys(found) => {
                saw = true;
                keys.extend(found);
            }
        }
    }
    let plugin = plugin_seat_keys();
    if !plugin.is_empty() {
        saw = true;
        keys.extend(plugin);
    }
    if saw {
        SeatMcpKeys::Known(keys)
    } else if ambiguous {
        SeatMcpKeys::Ambiguous
    } else {
        SeatMcpKeys::Unknown
    }
}

/// Whether `path` is the seat's own MCP config. Project files and a
/// config directory named by `CLAUDE_CONFIG_DIR` or `CODEX_HOME` are
/// left out, unless that path is the home file the harness already reads.
fn seat_config_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str());
    if name == Some(".mcp.json") {
        return false;
    }
    if name == Some("mcp.json")
        && path
            .parent()
            .and_then(|dir| dir.file_name())
            .and_then(|n| n.to_str())
            == Some(".cursor")
    {
        let home_cursor = crate::home().ok().map(|home| home.join(".cursor/mcp.json"));
        if home_cursor.as_deref() != Some(path) {
            return false;
        }
    }
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|p| !p.is_empty()) {
        let from_env = PathBuf::from(&dir).join(".claude.json");
        if path == from_env {
            let home_claude = crate::home().ok().map(|home| home.join(".claude.json"));
            if home_claude.as_deref() != Some(path) {
                return false;
            }
        }
    }
    if let Some(dir) = std::env::var_os("CODEX_HOME").filter(|p| !p.is_empty()) {
        let from_env = PathBuf::from(&dir).join("config.toml");
        if path == from_env {
            let home_codex = crate::home()
                .ok()
                .map(|home| home.join(".codex/config.toml"));
            if home_codex.as_deref() != Some(path) {
                return false;
            }
        }
    }
    true
}

#[derive(Debug)]
enum KeyRead {
    Absent,
    Skip,
    Ambiguous,
    Keys(BTreeSet<String>),
}

enum TextRead {
    Absent,
    Bad,
    Text(String),
}

fn read_seat_keys(file: &McpFile) -> KeyRead {
    let text = match read_config_text(&file.path) {
        TextRead::Absent => return KeyRead::Absent,
        TextRead::Bad => {
            return if file.strict {
                KeyRead::Ambiguous
            } else {
                KeyRead::Skip
            };
        }
        TextRead::Text(text) => text,
    };
    if text.trim().is_empty() {
        return KeyRead::Keys(BTreeSet::new());
    }
    match &file.probe {
        McpProbe::Json(_) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(doc) => KeyRead::Keys(json_seat_keys(&doc)),
            Err(_) => {
                if file.strict {
                    KeyRead::Ambiguous
                } else {
                    KeyRead::Skip
                }
            }
        },
        McpProbe::Text(marker) => {
            if let Ok(doc) = toml::from_str::<toml::Value>(&text) {
                return KeyRead::Keys(toml_seat_keys(&doc));
            }
            let mut keys = BTreeSet::new();
            if let Some(key) = text_marker_key(&text, marker) {
                remember_key(&mut keys, &key);
            }
            KeyRead::Keys(keys)
        }
    }
}

fn read_config_text(path: &Path) -> TextRead {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    // `~/.claude.json` stores per-project history and grows past the
    // cap. The seat key is the top-level `mcpServers` value. Read that
    // and stop, so the history is not pulled in and the cap does not hide
    // the key.
    if path.file_name().and_then(|name| name.to_str()) == Some(".claude.json") {
        return read_claude_config(path);
    }
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return TextRead::Absent,
        Err(_) => return TextRead::Bad,
    };
    let Ok(meta) = file.metadata() else {
        return TextRead::Bad;
    };
    if !meta.is_file() || meta.len() > MCP_CONFIG_CAP {
        return TextRead::Bad;
    }
    let mut text = String::new();
    match file.take(MCP_CONFIG_CAP).read_to_string(&mut text) {
        Ok(_) => TextRead::Text(text),
        Err(_) => TextRead::Bad,
    }
}

fn read_claude_config(path: &Path) -> TextRead {
    use std::io::BufReader;
    use std::os::unix::fs::OpenOptionsExt;
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return TextRead::Absent,
        Err(_) => return TextRead::Bad,
    };
    let Ok(meta) = file.metadata() else {
        return TextRead::Bad;
    };
    if !meta.is_file() {
        return TextRead::Bad;
    }
    if meta.len() == 0 {
        return TextRead::Text(String::new());
    }
    let mut reader = BufReader::with_capacity(8192, file);
    match claude_mcp_servers_value(&mut reader) {
        ClaudeRead::Servers(body) => TextRead::Text(body),
        ClaudeRead::NoServers => TextRead::Text("{}".into()),
        ClaudeRead::Bad => TextRead::Bad,
    }
}

enum ClaudeRead {
    Servers(String),
    NoServers,
    Bad,
}

/// The raw JSON of the top-level `mcpServers` value. Reading stops once
/// that value is in hand, so a history array after it is not consumed.
fn claude_mcp_servers_value(reader: &mut impl std::io::Read) -> ClaudeRead {
    let mut inp = JsonIn {
        reader,
        unread: None,
    };
    skip_ws(&mut inp);
    if inp.peek() != Some(b'{') {
        return ClaudeRead::Bad;
    }
    let _ = inp.bump();
    loop {
        skip_ws(&mut inp);
        match inp.peek() {
            None => return ClaudeRead::Bad,
            Some(b'}') => return ClaudeRead::NoServers,
            Some(b'"') => {}
            Some(_) => return ClaudeRead::Bad,
        }
        let Some(key) = read_json_string(&mut inp) else {
            return ClaudeRead::Bad;
        };
        skip_ws(&mut inp);
        if inp.bump() != Some(b':') {
            return ClaudeRead::Bad;
        }
        skip_ws(&mut inp);
        if key == "mcpServers" {
            let Some(raw) = read_json_value(&mut inp) else {
                return ClaudeRead::Bad;
            };
            let Ok(text) = String::from_utf8(raw) else {
                return ClaudeRead::Bad;
            };
            return ClaudeRead::Servers(format!("{{\"mcpServers\":{text}}}"));
        }
        if read_json_value(&mut inp).is_none() {
            return ClaudeRead::Bad;
        }
        skip_ws(&mut inp);
        match inp.peek() {
            Some(b',') => {
                let _ = inp.bump();
            }
            Some(b'}') => return ClaudeRead::NoServers,
            _ => return ClaudeRead::Bad,
        }
    }
}

struct JsonIn<'a> {
    reader: &'a mut dyn std::io::Read,
    unread: Option<u8>,
}

impl JsonIn<'_> {
    fn bump(&mut self) -> Option<u8> {
        if let Some(byte) = self.unread.take() {
            return Some(byte);
        }
        let mut buf = [0u8; 1];
        match self.reader.read(&mut buf) {
            Ok(1) => Some(buf[0]),
            _ => None,
        }
    }

    fn peek(&mut self) -> Option<u8> {
        if let Some(byte) = self.unread {
            return Some(byte);
        }
        let byte = self.bump()?;
        self.unread = Some(byte);
        Some(byte)
    }
}

fn skip_ws(inp: &mut JsonIn<'_>) {
    while inp.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
        let _ = inp.bump();
    }
}

fn read_json_string(inp: &mut JsonIn<'_>) -> Option<String> {
    if inp.bump() != Some(b'"') {
        return None;
    }
    let mut out = Vec::new();
    loop {
        let byte = inp.bump()?;
        if byte == b'\\' {
            let esc = inp.bump()?;
            let decoded = match esc {
                b'"' | b'\\' | b'/' => esc,
                b'b' => 0x08,
                b'f' => 0x0c,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'u' => {
                    let mut hex = [0u8; 4];
                    for slot in &mut hex {
                        *slot = inp.bump()?;
                    }
                    let text = std::str::from_utf8(&hex).ok()?;
                    let code = u32::from_str_radix(text, 16).ok()?;
                    let ch = char::from_u32(code)?;
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    continue;
                }
                _ => return None,
            };
            out.push(decoded);
            continue;
        }
        if byte == b'"' {
            break;
        }
        out.push(byte);
    }
    String::from_utf8(out).ok()
}

fn read_json_value(inp: &mut JsonIn<'_>) -> Option<Vec<u8>> {
    match inp.peek()? {
        b'{' => read_json_container(inp, b'{', b'}'),
        b'[' => read_json_container(inp, b'[', b']'),
        b'"' => read_json_string_raw(inp),
        b't' => read_json_lit(inp, b"true"),
        b'f' => read_json_lit(inp, b"false"),
        b'n' => read_json_lit(inp, b"null"),
        b'-' | b'0'..=b'9' => read_json_number(inp),
        _ => None,
    }
}

fn read_json_container(inp: &mut JsonIn<'_>, open: u8, close: u8) -> Option<Vec<u8>> {
    let first = inp.bump()?;
    if first != open {
        return None;
    }
    let mut out = vec![first];
    let mut depth = 1u32;
    let mut in_str = false;
    let mut escaped = false;
    while depth > 0 {
        let byte = inp.bump()?;
        out.push(byte);
        if in_str {
            if escaped {
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                in_str = false;
            }
            continue;
        }
        if byte == b'"' {
            in_str = true;
        } else if byte == open {
            depth += 1;
        } else if byte == close {
            depth -= 1;
        }
    }
    Some(out)
}

fn read_json_string_raw(inp: &mut JsonIn<'_>) -> Option<Vec<u8>> {
    if inp.bump() != Some(b'"') {
        return None;
    }
    let mut out = vec![b'"'];
    loop {
        let byte = inp.bump()?;
        out.push(byte);
        if byte == b'\\' {
            out.push(inp.bump()?);
            continue;
        }
        if byte == b'"' {
            break;
        }
    }
    Some(out)
}

fn read_json_lit(inp: &mut JsonIn<'_>, lit: &[u8]) -> Option<Vec<u8>> {
    for expected in lit {
        if inp.bump()? != *expected {
            return None;
        }
    }
    Some(lit.to_vec())
}

fn read_json_number(inp: &mut JsonIn<'_>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    while inp.peek().is_some_and(|byte| {
        byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E')
    }) {
        out.push(inp.bump()?);
    }
    (!out.is_empty()).then_some(out)
}

/// `mcp__plugin_<plugin>_<server>__...` for an installed plugin whose
/// server command is the seat. `enabledPlugins` must name the plugin
/// when that object is present. No settings file means the install counts.
fn plugin_seat_keys() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    // A plugin install under the home directory is the seat's own.
    // CLAUDE_CONFIG_DIR is not: that directory is chosen by the process.
    if let Ok(home) = crate::home() {
        plugin_keys_from(&home.join(".claude"), &mut out);
    }
    out
}

fn plugin_keys_from(base: &Path, out: &mut BTreeSet<String>) {
    let enabled = enabled_plugins(&base.join("settings.json"));
    let Some(text) = read_capped(&base.join("plugins/installed_plugins.json")) else {
        return;
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let Some(plugins) = doc.get("plugins").and_then(serde_json::Value::as_object) else {
        return;
    };
    for (id, entries) in plugins {
        if !plugin_allowed(&enabled, id) {
            continue;
        }
        let Some(list) = entries.as_array() else {
            continue;
        };
        for entry in list {
            let Some(install) = entry
                .get("installPath")
                .and_then(serde_json::Value::as_str)
                .filter(|path| !path.is_empty())
            else {
                continue;
            };
            let plugin_json = PathBuf::from(install).join(".claude-plugin/plugin.json");
            let Some(body) = read_capped(&plugin_json) else {
                continue;
            };
            let Ok(spec) = serde_json::from_str::<serde_json::Value>(&body) else {
                continue;
            };
            let Some(name) = spec.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let Some(servers) = spec.get("mcpServers") else {
                continue;
            };
            for key in json_seat_keys(servers) {
                remember_key(out, &format!("plugin_{name}_{key}"));
            }
        }
    }
}

fn enabled_plugins(path: &Path) -> Option<serde_json::Map<String, serde_json::Value>> {
    let text = read_capped(path)?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    doc.get("enabledPlugins")
        .and_then(serde_json::Value::as_object)
        .cloned()
}

fn plugin_allowed(enabled: &Option<serde_json::Map<String, serde_json::Value>>, id: &str) -> bool {
    match enabled {
        None => true,
        Some(map) => map.get(id).and_then(serde_json::Value::as_bool) == Some(true),
    }
}

fn read_capped(path: &Path) -> Option<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MCP_CONFIG_CAP {
        return None;
    }
    let mut text = String::new();
    file.take(MCP_CONFIG_CAP).read_to_string(&mut text).ok()?;
    Some(text)
}

/// Whether a Cursor payload `command` field launches the seat server.
/// The field is one string, so `ljos mcp` and `ljos-plugin ljos-mcp`
/// are split on whitespace. An empty field is not a command.
pub(crate) fn payload_command_is_seat(command: &str) -> bool {
    let mut parts = command.split_whitespace();
    let Some(bin) = parts.next() else {
        return false;
    };
    let args: Vec<String> = parts.map(str::to_string).collect();
    command_launches_seat(bin, &args)
}

fn command_launches_seat(command: &str, args: &[String]) -> bool {
    let base = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);
    if base == crate::MCP_SERVER_COMMAND {
        return true;
    }
    // The shipped plugin starts the seat as `ljos-plugin ljos-mcp`.
    if base == "ljos-plugin" && args.iter().any(|arg| arg == crate::MCP_SERVER_COMMAND) {
        return true;
    }
    base == "ljos" && args.first().map(String::as_str) == Some("mcp")
}

/// Claude Code's tool segment keeps `[A-Za-z0-9_-]` and turns every other
/// character into `_`. `ljos.seat` is the tool `mcp__ljos_seat__...`.
/// The raw key is kept as well. Case is not folded.
fn claude_tool_segment(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn remember_key(out: &mut BTreeSet<String>, key: &str) {
    if key.is_empty() {
        return;
    }
    let norm = claude_tool_segment(key);
    if norm != key {
        out.insert(norm);
    }
    out.insert(key.to_string());
}

fn entry_is_off(disabled: Option<bool>, enabled: Option<bool>) -> bool {
    disabled == Some(true) || enabled == Some(false)
}

fn json_entry_launches(value: &serde_json::Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    if entry_is_off(
        obj.get("disabled").and_then(serde_json::Value::as_bool),
        obj.get("enabled").and_then(serde_json::Value::as_bool),
    ) {
        return false;
    }
    command_and_args(value).is_some_and(|spec| command_launches_seat(&spec.command, &spec.args))
}

fn json_seat_keys(value: &serde_json::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    fn walk(value: &serde_json::Value, out: &mut BTreeSet<String>) {
        if let Some(list) = value.as_array() {
            for item in list {
                walk(item, out);
            }
            return;
        }
        let Some(obj) = value.as_object() else {
            return;
        };
        for (key, child) in obj {
            if json_entry_launches(child) {
                remember_key(out, key);
            }
            walk(child, out);
        }
    }
    walk(value, &mut out);
    out
}

fn toml_bool(value: Option<&toml::Value>) -> Option<bool> {
    value.and_then(toml::Value::as_bool)
}

fn toml_strings(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn toml_entry_launches(value: &toml::Value) -> bool {
    let Some(table) = value.as_table() else {
        return false;
    };
    if entry_is_off(
        toml_bool(table.get("disabled")),
        toml_bool(table.get("enabled")),
    ) {
        return false;
    }
    let args = toml_strings(table.get("args"));
    let Some(cmd) = table.get("command").or_else(|| table.get("cmd")) else {
        return false;
    };
    match cmd {
        toml::Value::String(command) => command_launches_seat(command.trim(), &args),
        toml::Value::Array(parts) => {
            let mut argv: Vec<String> = parts
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect();
            if argv.is_empty() {
                return false;
            }
            let command = argv.remove(0);
            argv.extend(args);
            command_launches_seat(command.trim(), &argv)
        }
        _ => false,
    }
}

fn toml_seat_keys(value: &toml::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    fn walk(value: &toml::Value, out: &mut BTreeSet<String>) {
        let Some(table) = value.as_table() else {
            return;
        };
        for (key, child) in table {
            if toml_entry_launches(child) {
                remember_key(out, key);
            }
            walk(child, out);
        }
    }
    walk(value, &mut out);
    out
}

fn marker_server_key(marker: &str) -> Option<String> {
    let header = marker.trim();
    if let Some(inner) = header
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        let name = inner.split('.').next_back()?.trim();
        return (!name.is_empty()).then(|| name.to_string());
    }
    let line = header.lines().next()?.trim().trim_end_matches(':').trim();
    if line.is_empty() || line.contains(' ') || line.contains('\t') {
        return None;
    }
    Some(line.to_string())
}

fn text_marker_key(text: &str, marker: &str) -> Option<String> {
    let window = entry_text(text, marker)?;
    let command = keyed_value(&window, "command").or_else(|| keyed_value(&window, "cmd"))?;
    let args = keyed_value(&window, "args").into_iter().collect::<Vec<_>>();
    if !command_launches_seat(command.trim(), &args) {
        return None;
    }
    marker_server_key(marker)
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
    } else {
        let rest = header.strip_prefix('[')?;
        let rest = rest.strip_suffix(']')?;
        if rest.contains('[') {
            return None;
        }
        rest
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

/// The env.PATH the client hands the server, with this process's PATH
/// substituted where the client would substitute its own.
fn expand_env_path(value: &str, expand: Expand) -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    match expand {
        Expand::Literal => value.to_string(),
        Expand::Opencode => replace_opencode_env(value, "PATH", &path),
        Expand::Cursor => expand_cursor(value),
        Expand::Claude => expand_claude(value),
        Expand::Shell => replace_dollar_path(
            &value
                .replace("${env:PATH}", &path)
                .replace("${PATH}", &path),
            &path,
        ),
    }
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
fn expand_launch(command: &str, expand: Expand) -> String {
    match expand {
        Expand::Literal => command.to_string(),
        Expand::Opencode => expand_opencode_home(command),
        Expand::Cursor => expand_cursor(command),
        Expand::Claude => expand_claude(command),
        Expand::Shell => expand_shell_home(command),
    }
}

/// Cursor's `${env:NAME}` and `${userHome}`. `$HOME`, `${HOME}` and `~/`
/// stay text.
fn expand_cursor(text: &str) -> String {
    expand_braced(text, |inner| match inner {
        "userHome" => crate::home().ok().map(|h| h.display().to_string()),
        _ => std::env::var(inner.strip_prefix("env:")?).ok(),
    })
}

/// Claude Code's `${VAR}` and `${VAR:-default}`. An unset `${VAR}` with no
/// default stays as written, as do `$HOME`, `~/` and Cursor's forms.
fn expand_claude(text: &str) -> String {
    expand_braced(text, |inner| {
        let (name, default) = match inner.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (inner, None),
        };
        if !is_env_name(name) {
            return None;
        }
        std::env::var(name)
            .ok()
            .or_else(|| default.map(str::to_string))
    })
}

fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Each `${...}` in `text` replaced by what `resolve` gives for its inside,
/// or kept as written when it gives nothing.
fn expand_braced(text: &str, resolve: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        let inner = &rest[at + 2..];
        let Some(len) = inner.find('}') else {
            break;
        };
        out.push_str(&rest[..at]);
        match resolve(&inner[..len]) {
            Some(value) => out.push_str(&value),
            None => out.push_str(&rest[at..at + len + 3]),
        }
        rest = &inner[len + 1..];
    }
    out.push_str(rest);
    out
}

/// OpenCode substitutes `{env:HOME}` only. `$HOME`, `${HOME}`, `~/` and
/// `${env:HOME}` stay text: the docs say shell expressions are not expanded.
fn expand_opencode_home(command: &str) -> String {
    let Ok(home) = crate::home() else {
        return command.to_string();
    };
    replace_opencode_env(command, "HOME", &home.display().to_string())
}

/// OpenCode's `{env:NAME}` replaced by `value`. `${env:NAME}` is not
/// OpenCode's form and stays text.
fn replace_opencode_env(text: &str, name: &str, value: &str) -> String {
    let token = format!("{{env:{name}}}");
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(&token) {
        let shell_form = at > 0 && rest.as_bytes()[at - 1] == b'$';
        if shell_form {
            out.push_str(&rest[..at + token.len()]);
        } else {
            out.push_str(&rest[..at]);
            out.push_str(value);
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
    let command = expand_launch(&launch.command, file.expand);
    let env_path = launch
        .env_path
        .as_deref()
        .map(|path| expand_env_path(path, file.expand));
    let search_path = env_path.as_deref().or(file.launch_path.as_deref());
    let found_path = command_path(&command, search_path);
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
        } else if env_path.is_some() {
            format!("{} is not on env.PATH", launch.command)
        } else if let Some(path) = &file.launch_path {
            format!("{} is not on the app's PATH, {path}", launch.command)
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
    let name = Path::new(&command).file_name().and_then(|n| n.to_str());
    if name == Some(crate::MCP_SERVER_COMMAND) {
        if let Ok(own) = crate::server_path() {
            if !same_binary(&chosen, &own) {
                return LaunchCheck::Warn(if pathy {
                    format!("{} is not this install's {}", launch.command, own.display())
                } else {
                    format!(
                        "{} resolves to {}, not {}",
                        launch.command,
                        chosen.display(),
                        own.display()
                    )
                });
            }
        }
    }
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
    // A directory is mode 0755, and `metadata` follows a symlink. Either
    // would count as an installed encoder and skip the download.
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Copy `src` beside `dest` and rename it over `dest`. A running binary
/// cannot be opened for writing (ETXTBSY), and the `ljos` running cloud up
/// is often the one being replaced; a rename leaves it running from the
/// old inode.
fn install_binary(src: &Path, dest: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("ljos");
    let tmp = dest.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let copied = (|| -> Result<()> {
        fs::copy(src, &tmp)
            .with_context(|| format!("copy {} to {}", src.display(), tmp.display()))?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
        fs::rename(&tmp, dest).with_context(|| format!("install {}", dest.display()))?;
        Ok(())
    })();
    if copied.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    copied
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
    let mut copied = Vec::new();
    let mut kept = Vec::new();
    for name in &install {
        let dest = bin_dir.join(name);
        if executable(&dest) && sha256_file(&dest).is_ok_and(|got| got == want[*name]) {
            kept.push(*name);
            continue;
        }
        install_binary(&src.join(name), &dest)?;
        copied.push(*name);
    }
    let mut line = if copied.is_empty() {
        format!(
            "binaries: {} already in {}",
            kept.join(", "),
            bin_dir.display()
        )
    } else {
        format!(
            "binaries: installed {} into {}",
            copied.join(", "),
            bin_dir.display()
        )
    };
    if !copied.is_empty() && !kept.is_empty() {
        line.push_str(&format!("; {} already there", kept.join(", ")));
    }
    line.push_str(" (checksums match)");
    Ok(line)
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
/// release, on a cloud VM only. Elsewhere nothing is downloaded.
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
    // `tar -xzf` unpacks a symlink, and `is_file` follows it, so the
    // install would put that link on PATH and chmod its target. Refuse
    // those members, hard links, and paths that leave the tree, in both
    // archives, before unpacking either.
    for file in &archives {
        refuse_unsafe_archive(file)?;
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
    refuse_unsafe_tree(&tree)?;
    let new_lib = tree.join(ENCODER_LIB);
    let new_bin = tree.join("bin/packset-embed");
    if !regular_file(&new_lib.join(ENCODER_SONAME)) || !regular_file(&new_bin) {
        bail!("the encoder archives lack bin/packset-embed or {ENCODER_LIB}/{ENCODER_SONAME}; nothing was installed");
    }
    // A running encoder keeps the inode it opened, so the new files are
    // renamed in. chmod and the destination directory happen first: a
    // failure after the runtime rename used to leave the new runtime and
    // delete the old one with the stage. Both names move aside, then both
    // new names move in, and either failure puts the previous pair back.
    use std::os::unix::fs::PermissionsExt;
    if let Err(err) = fs::set_permissions(&new_bin, fs::Permissions::from_mode(0o755)) {
        bail!("encoder install failed ({err}); nothing was installed");
    }
    if let Err(err) = fs::create_dir_all(bin_dir) {
        bail!("encoder install failed ({err}); nothing was installed");
    }
    let bin = bin_dir.join("packset-embed");
    swap_encoder(stage, &new_lib, &new_bin, lib, &bin)?;
    Ok(format!(
        "encoder: installed {} and ONNX Runtime in {} (checksums match)",
        bin.display(),
        lib.display()
    ))
}

/// `symlink_metadata` sees a dangling symlink. `exists` follows links and
/// would leave that name in the way of the rename.
fn path_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => bail!("encoder install failed ({err}); nothing was installed"),
    }
}

/// Stage both names, swap both, and roll both back. The stage directory is
/// removed by the caller, so a backup that is still there at return is gone.
fn swap_encoder(
    stage: &Path,
    new_lib: &Path,
    new_bin: &Path,
    lib: &Path,
    bin: &Path,
) -> Result<()> {
    let backup_lib = stage.join("old-lib");
    let backup_bin = stage.join("old-bin");
    let had_lib = path_present(lib)?;
    let had_bin = path_present(bin)?;
    let mut bin_aside = false;

    let fail = |bin_aside: bool, cause: &str| -> Result<()> {
        let lib_err = had_lib
            .then(|| fs::rename(&backup_lib, lib).err())
            .flatten();
        let bin_err = bin_aside
            .then(|| fs::rename(&backup_bin, bin).err())
            .flatten();
        match lib_err.or(bin_err) {
            None => bail!("encoder install failed ({cause}); nothing was installed"),
            Some(err) => bail!(
                "encoder install failed ({cause}); restoring the previous encoder also failed: {err}"
            ),
        }
    };

    if had_lib {
        if let Err(err) = fs::rename(lib, &backup_lib) {
            bail!("encoder install failed ({err}); nothing was installed");
        }
    }
    if had_bin {
        if let Err(err) = fs::rename(bin, &backup_bin) {
            return fail(false, &err.to_string());
        }
        bin_aside = true;
    }
    if let Err(err) = fs::rename(new_lib, lib) {
        return fail(bin_aside, &err.to_string());
    }
    if let Err(err) = fs::rename(new_bin, bin) {
        // The new runtime is occupying the live name. Move it back into the
        // stage, or delete it, so the previous runtime can return.
        if fs::rename(lib, new_lib).is_err() {
            let _ = fs::remove_dir_all(lib);
        }
        return fail(bin_aside, &err.to_string());
    }
    Ok(())
}

/// A cloud VM. A CI job is not one, so `cloud up` does not download
/// the encoder. The same [`runtime`] answers doctor.
fn cloud_vm() -> bool {
    let rt = runtime();
    !rt.ci && (rt.cloud_id || rt.api_socket)
}

fn encoder_target() -> Option<&'static str> {
    encoder_target_for(std::env::consts::ARCH, std::env::consts::OS)
}

fn encoder_target_for(arch: &str, os: &str) -> Option<&'static str> {
    match (arch, os) {
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        ("x86_64", "macos") => Some("x86_64-apple-darwin"),
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
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

/// Clone or update the tracker at `dest`. The flag is false when no clone
/// is there afterwards, and the line then says what to set.
fn ensure_tracker(
    remote: &crate::tracker_attach::Remote,
    dest: &Path,
    seat: &str,
    home: &Path,
) -> Result<(String, bool)> {
    let secret = crate::tracker_attach::token().or_else(|| remote.secret.clone().map(|s| s.0));
    let token = match secret {
        Some(token) => crate::tracker_attach::token_url(&remote.fetch)
            .map(|url| crate::tracker_attach::store_token(home, &token).map(|helper| (helper, url)))
            .transpose()?,
        None => None,
    };
    if is_work_tree(dest) {
        ensure_identity(dest, seat)?;
        if let Some((helper, url)) = &token {
            crate::tracker_attach::configure(dest, Some(helper), url)?;
        }
        let line = match crate::tracker_push::publish(dest) {
            Ok(()) => format!("tracker: updated {}", dest.display()),
            Err(e) => format!(
                "tracker: {} kept local; push queued: {}",
                dest.display(),
                hide(&e, remote)
            ),
        };
        return Ok((line, true));
    }
    if dest.exists() {
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
    }
    Ok(match clone_tracker(remote, dest, seat, token.as_ref()) {
        Ok(line) => (line, true),
        Err(e) => (
            format!(
                "{}. This machine has no shared tracker. Set {} to a token that can push to {}, or add that repository to this environment",
                hide(&format!("{e:#}"), remote),
                crate::tracker_attach::TOKEN_VAR,
                remote.shown
            ),
            false,
        ),
    })
}

/// `text` with the fetch URL shown as written down, and no token in it.
fn hide(text: &str, remote: &crate::tracker_attach::Remote) -> String {
    let mut text = text.replace(&remote.fetch, &remote.shown);
    let tokens = [
        crate::tracker_attach::token(),
        remote.secret.clone().map(|s| s.0),
    ];
    for token in tokens.into_iter().flatten() {
        text = text.replace(&token, "***");
    }
    text
}

/// Clone `remote` to `dest`. `token` is the credential helper and the URL
/// it answers for.
fn clone_tracker(
    remote: &crate::tracker_attach::Remote,
    dest: &Path,
    seat: &str,
    token: Option<&(String, String)>,
) -> Result<String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = dest.parent().context("tracker path has no parent")?;
    let name = dest
        .file_name()
        .context("tracker path has no name")?
        .to_string_lossy()
        .to_string();
    let (url, helper) = match token {
        Some((helper, url)) => (url.as_str(), Some(helper.as_str())),
        None => (remote.fetch.as_str(), None),
    };
    let shown = &remote.shown;
    crate::tracker_attach::refuse_secret_rewrite(url, shown)?;
    let out = crate::git_cmd()
        .current_dir(parent)
        .args(crate::tracker_attach::clone_config(url, helper))
        .args(["clone", "--quiet", url, &name])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_ASKPASS")
        .stdin(std::process::Stdio::null())
        .output()
        .context("git clone")?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        bail!("tracker: clone of {shown} failed: {}", said.trim());
    }
    if let Some(helper) = helper {
        crate::tracker_attach::configure(dest, Some(helper), url)?;
    }
    ensure_identity(dest, seat)?;
    let from = &remote.from;
    match crate::tracker_push::publish(dest) {
        Ok(()) => Ok(format!(
            "tracker: cloned {shown} to {} (from {from})",
            dest.display()
        )),
        Err(e) => Ok(format!(
            "tracker: cloned {shown} to {} (from {from}); push queued: {}",
            dest.display(),
            hide(&e, remote)
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
    let out = crate::git_cmd()
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
    let out = crate::git_cmd()
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
    let Ok(out) = crate::git_cmd()
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
    env.insert(crate::sync::SYNC_HOST_KEY.into(), serde_json::json!(seat));
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
    let mut line = format!("mcp: {} starts {command}", path.display());
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
    let by_seat = wake_vars_by_seat();
    let existing = crate::wake::load();
    let mut lines = Vec::new();
    for seat in existing.keys() {
        let env_owned = existing
            .get(seat)
            .is_some_and(|wake| wake.source.as_deref() == Some(crate::wake::ENV_SOURCE));
        if env_owned && !by_seat.contains_key(seat) && crate::wake::clear_env_webhook(seat)? {
            lines.push(format!(
                "wake {seat}: removed the stale wake cloud up wrote; no LJOS_WAKE_<SEAT>_URL names it now"
            ));
        }
    }
    for (seat, group) in by_seat {
        if group.len() > 1 {
            lines.push(collision_line(
                &seat,
                &group,
                crate::wake::clear_env_webhook(&seat)?,
            ));
            continue;
        }
        let one = &group[0];
        if crate::wake::check_webhook(&one.url).is_err() {
            let said = if crate::wake::clear_env_webhook(&seat)? {
                "removed the stale wake cloud up wrote"
            } else {
                "no wake was written"
            };
            lines.push(format!(
                "wake {seat}: {} is not https, nor http on this machine, so {said}",
                one.name
            ));
            continue;
        }
        let key_env = format!("LJOS_WAKE_{}_KEY", one.mid);
        let wrote = crate::wake::set_env_webhook(&seat, &one.url, &key_env)?;
        if !wrote {
            lines.push(format!(
                "wake {seat}: left the wake cloud up did not write; {} did not replace it",
                one.name
            ));
            continue;
        }
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

fn collision_line(seat: &str, group: &[WakeVar], retired: bool) -> String {
    let mut names: Vec<&str> = group.iter().map(|var| var.name.as_str()).collect();
    names.sort_unstable();
    let said = if retired {
        "removed the stale wake cloud up wrote"
    } else {
        "no wake was written"
    };
    format!(
        "wake {seat}: {} name the same seat, so {said}",
        list_names(&names),
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

/// Whether this machine is a cloud seat. `ljos cloud up` marks it by
/// writing the push allowlist. A box that never ran it has no file, so
/// finish exports no deeds.
#[must_use]
pub fn cloud_seat() -> bool {
    crate::cloud_hook::allow_path().is_some_and(|path| path.is_file())
}

/// Whether `ljos cloud up` has marked this machine. The old refusal file
/// does not count.
#[must_use]
pub fn any_cloud_marker() -> bool {
    cloud_seat()
}

/// Delete a refusal file an older `ljos cloud up` wrote. The one push
/// rule is not stored in a file.
pub fn replace_stale_guard() {
    let Some(path) = cloud_guard_rules_path() else {
        return;
    };
    if path.is_file() {
        let _ = fs::remove_file(path);
    }
}

/// Replace the push allowlist `ljos cloud up` writes. `code` and `tracker`
/// are remote URLs. Branch names are the short names (`develop`).
///
/// # Errors
///
/// No config home, or the allowlist file cannot be replaced.
pub fn write_push_allowlist(code: &[&str], tracker: &[&str], branches: &[&str]) -> Result<()> {
    crate::cloud_hook::write_push_allowlist(code, tracker, branches)
}

pub(crate) fn ensure_cloud_guard_rules() -> Result<String> {
    let path =
        crate::cloud_hook::allow_path().context("cloud up: no config home for the cloud seat")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !path.is_file() {
        fs::write(&path, "")?;
    }
    replace_stale_guard();
    Ok(
        "guard: a cloud seat may push only its named branches and develop, and may not force or delete a ref"
            .into(),
    )
}

fn cloud_guard_rules_path() -> Option<PathBuf> {
    Some(
        crate::config_base()?
            .join("ljos")
            .join(CLOUD_GUARD_RULES_FILE),
    )
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
    let unwound = unwind_system_links(path);
    let path = unwound.as_path();
    // A symlink anywhere in the path would make the read, the chmod, and
    // the replace land in the directory the link points at. The final
    // component is still refused below; this catches a parent.
    // A symlink the system owns, such as `/tmp` or `/var`, is already
    // replaced above. A link a user can swap is not.
    refuse_symlink_parent(path)?;
    // Follow a symlink and the identity would be written into the target,
    // or the target's mode would change. A link is left to write_private,
    // which refuses it.
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

/// True when a parent of `path` is a symlink. The final component is not
/// looked at. Missing parents are not links.
fn ancestor_is_symlink(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let mut walked = PathBuf::new();
    for component in parent.components() {
        match component {
            std::path::Component::RootDir => walked.push(component),
            std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                walked.push(name);
                if fs::symlink_metadata(&walked).is_ok_and(|meta| meta.file_type().is_symlink()) {
                    return true;
                }
            }
            _ => return true,
        }
    }
    false
}

/// A symlink owned by root, in a directory owned by root that other
/// users cannot write. `/tmp` and `/var` on macOS are this. A link in a
/// directory the user can write is a swap, and is not.
fn system_owned_symlink(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_symlink() || meta.uid() != 0 {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(dir) = fs::symlink_metadata(parent) else {
        return false;
    };
    if dir.uid() != 0 {
        return false;
    }
    let mode = dir.mode();
    if mode & 0o002 != 0 {
        return false;
    }
    if mode & 0o020 != 0 && dir.gid() != 0 {
        return false;
    }
    true
}

/// Replace symlink components the system owns with their targets, and
/// leave every other component as given. A user link stays a link so the
/// caller can refuse it.
fn unwind_system_links(path: &Path) -> PathBuf {
    fn walk(path: &Path, depth: u8) -> PathBuf {
        if depth > 8 {
            return path.to_path_buf();
        }
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::RootDir => out.push(component),
                std::path::Component::CurDir => {}
                std::path::Component::Normal(name) => {
                    out.push(name);
                    if system_owned_symlink(&out) {
                        let Ok(target) = fs::read_link(&out) else {
                            continue;
                        };
                        let next = if target.is_absolute() {
                            target
                        } else {
                            out.parent().unwrap_or(Path::new("/")).join(target)
                        };
                        out = walk(&next, depth + 1);
                    }
                }
                _ => return path.to_path_buf(),
            }
        }
        out
    }
    walk(path, 0)
}

fn refuse_symlink_parent(path: &Path) -> Result<()> {
    if ancestor_is_symlink(path) {
        bail!(
            "cloud up: {} is reached through a symlink, so the scope cloud identity was not written",
            path.display()
        );
    }
    Ok(())
}

/// Open `path` by walking from `/` with `O_NOFOLLOW`, so a symlink in any
/// component is not followed. The returned fd is the last component.
fn open_nofollow(path: &Path) -> std::io::Result<fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    let absolute;
    let path = if path.is_absolute() {
        path
    } else {
        absolute = std::env::current_dir()?.join(path);
        absolute.as_path()
    };
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => names.push(name),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "path is not direct",
                ));
            }
        }
    }
    let mut current = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    for (index, name) in names.iter().enumerate() {
        let last = index + 1 == names.len();
        let mut bytes = name.as_bytes().to_vec();
        if bytes.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "nul in path",
            ));
        }
        bytes.push(0);
        let mut flags = libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_RDONLY;
        if !last {
            flags |= libc::O_DIRECTORY;
        }
        // SAFETY: current is an open directory fd. The name is a NUL-terminated
        // byte string with no interior NUL. openat returns a new fd or -1.
        let fd = unsafe { libc::openat(current.as_raw_fd(), bytes.as_ptr().cast(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fd is newly created, not shared, and was opened with O_CLOEXEC.
        current = unsafe { fs::File::from_raw_fd(fd) };
    }
    Ok(current)
}

fn set_mode_0600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let unwound = unwind_system_links(path);
    let path = unwound.as_path();
    // Linux returns ENOTDIR, not ELOOP, when O_DIRECTORY meets a symlink
    // parent. Name that before opening so the chmod is not the probe.
    let linked = ancestor_is_symlink(path)
        || fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    if linked {
        bail!("mode 0600 {}: path is a symlink", path.display());
    }
    let file = match open_nofollow(path) {
        Ok(file) => file,
        Err(err) if matches!(err.raw_os_error(), Some(libc::ELOOP) | Some(libc::ENOTDIR)) => {
            bail!("mode 0600 {}: path is a symlink", path.display());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("mode 0600 {}", path.display()));
        }
    };
    if !file.metadata().is_ok_and(|meta| meta.file_type().is_file()) {
        bail!("mode 0600 {}: target is not a file", path.display());
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("mode 0600 {}", path.display()))
}

/// Write `text` to `path` as mode 0600. A symlink at `path` is refused, so
/// the identity is not stored through a link to another file. The temp
/// file beside it is made new, mode 0600 from the start, and is not a
/// link: a file or link already at that name is refused, not written
/// through, so no other user can read the identity even for a moment.
fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let unwound = unwind_system_links(path);
    let path = unwound.as_path();
    refuse_symlink_parent(path)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        bail!(
            "cloud up: {} is a symlink, so the scope cloud identity was not written; remove it and run `ljos cloud up` again",
            path.display()
        );
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("cloud.age");
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .with_context(|| format!("create {}", tmp.display()))?;
    let write = (|| -> Result<()> {
        // The umask may clear bits of the mode asked for at creation.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path).with_context(|| format!("install {}", path.display()))?;
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
        // `case $- in` / `*i*) ;;` / `*) return;;` / `esac`, and an
        // `if [ -z "$PS1" ]; then` / `return` / `fi` splits them the same
        // way. A one-line search appends PATH after that return, so a
        // non-interactive shell never sees it.
        let rest = &lines[i + 1..];
        if one_line
            || (case_tests_shell(trimmed) && block_returns(rest, "esac"))
            || (if_tests_shell(trimmed) && block_returns(rest, "fi"))
        {
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

/// `if [ -z "$PS1" ]; then` or `if [[ $- != *i* ]]`, an `if` that tests
/// for an interactive shell.
fn if_tests_shell(trimmed: &str) -> bool {
    trimmed.starts_with("if ") && (trimmed.contains("$PS1") || trimmed.contains("$-"))
}

/// Whether the block returns before the `esac` or `fi` that closes it. A
/// `case "$TERM"` does not.
fn block_returns(lines: &[&str], end: &str) -> bool {
    for line in lines {
        let trimmed = line.trim();
        let first = trimmed.split([';', ' ', '\t']).next().unwrap_or("");
        if first == end {
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

/// Whether a hold record on this machine names the claim: `Some(true)`
/// when one of them names a runner that is alive, `Some(false)` when every
/// one names a runner that is gone, and `None` when none names it. `None`
/// is a claim another machine made.
fn local_hold(assignee: &str, issue: &str) -> Option<bool> {
    let entries = fs::read_dir(crate::runtime_dir()).ok()?;
    let mut found = None;
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let Some(actor) = name.strip_prefix("hold-") else {
            continue;
        };
        let naming = hold_names_claim(actor, assignee, issue);
        if naming == HoldNames::Other {
            continue;
        }
        let Some(hold) = crate::read_hold(actor) else {
            continue;
        };
        if crate::hold_alive(&hold) {
            return Some(true);
        }
        // A hold that names no issue cannot say which claims it held, so
        // once its runner is gone it releases none of them.
        if naming == HoldNames::Issue {
            found = Some(false);
        }
    }
    found
}

#[derive(Debug, PartialEq, Eq)]
enum HoldNames {
    /// The record names this issue.
    Issue,
    /// A record named by the seat itself with a blank issue line: it
    /// keeps every claim of that seat while its runner lives.
    AnyOfSeat,
    Other,
}

/// Whether this hold record is the one `ljos claim` wrote for `issue`.
///
/// The filename is the claim-graph actor, a hash, while the tracker stores
/// the seat under `:CLAIMED_BY:`. The record's first line is that seat and
/// its sixth line is the issue. A filename that is already the seat still
/// matches, which is how a hand-written hold keeps its claim. A live hold
/// for another issue does not keep this one.
fn hold_names_claim(actor: &str, assignee: &str, issue: &str) -> HoldNames {
    let path = crate::runtime_dir().join(format!("hold-{actor}"));
    let Ok(text) = fs::read_to_string(path) else {
        return HoldNames::Other;
    };
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let record_assignee = lines.first().copied().unwrap_or("");
    let node = lines.get(5).copied().unwrap_or("");
    let name_ok =
        crate::same_holder(actor, assignee) || crate::same_holder(record_assignee, assignee);
    if !name_ok {
        return HoldNames::Other;
    }
    if node.is_empty() {
        return if crate::same_holder(actor, assignee) {
            HoldNames::AnyOfSeat
        } else {
            HoldNames::Other
        };
    }
    if node == issue {
        HoldNames::Issue
    } else {
        HoldNames::Other
    }
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
    let rt = runtime();
    if agent_id().is_some() || rt.ci || rt.codespace {
        return true;
    }
    // A desktop terminal sets CURSOR_AGENT=1 and nothing else. The api
    // socket is not enough either: doctor is often aimed at another home.
    if rt.cursor_harness && rt.cloud_agent {
        return true;
    }
    cloud_env_file_seat()
}

/// Where this process is running. [`cloud_vm`], [`on_cloud_vm`] and
/// [`harness_kind`] all read this, so a CI value cannot count in one and
/// not the other.
fn runtime() -> Runtime {
    let github_actions = env_is_true("GITHUB_ACTIONS");
    let cloud_id =
        nonempty_var("CURSOR_AGENT_ID").is_some() || nonempty_var("CLOUD_AGENT_ID").is_some();
    let api_socket = Path::new("/run/cursor/api.sock").exists();
    // CURSOR_AGENT=1 is the desktop agent terminal. The socket path and
    // the conversation id are set on a cloud VM and are not that flag.
    let cloud_agent = cloud_id
        || nonempty_var("CURSOR_AGENT_SOCKET").is_some()
        || nonempty_var("CURSOR_CONVERSATION_ID").is_some();
    let cursor_harness = cloud_agent
        || api_socket
        || std::env::var_os("CURSOR_AGENT").is_some()
        || std::env::var_os("CURSOR_AGENT_ID").is_some();
    let codespace = env_is_true("CODESPACES") || nonempty_var("CODESPACE_NAME").is_some();
    // By value, as each system documents it. Presence of `false` is not a job.
    // GitHub Actions: GITHUB_ACTIONS=true. GitLab: GITLAB_CI=true and CI=true.
    // Buildkite: BUILDKITE=true. Azure Pipelines: TF_BUILD=True.
    // CircleCI: CIRCLECI=true. Jenkins: JENKINS_URL is the controller URL.
    let ci = github_actions
        || env_is_true("GITLAB_CI")
        || env_is_true("CI")
        || env_is_true("BUILDKITE")
        || env_is_true("TF_BUILD")
        || env_is_true("CIRCLECI")
        || nonempty_var("JENKINS_URL").is_some();
    Runtime {
        github_actions,
        ci,
        codespace,
        cloud_id,
        api_socket,
        cursor_harness,
        cloud_agent,
    }
}

struct Runtime {
    github_actions: bool,
    ci: bool,
    codespace: bool,
    cloud_id: bool,
    api_socket: bool,
    cursor_harness: bool,
    cloud_agent: bool,
}

/// The value `true`, ignoring case and surrounding space.
fn env_is_true(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
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
    runtime().github_actions
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
    let rt = runtime();
    if rt.cursor_harness {
        "cursor"
    } else if rt.codespace {
        "codespace"
    } else if rt.ci {
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

#[derive(Debug)]
struct ArchiveMember {
    /// tar's verbose type character: `-` file, `d` directory, `l` symlink.
    kind: u8,
    name: Vec<u8>,
}

/// Members of one encoder archive. A listing tar will not parse is refused,
/// so a hostile header cannot be unpacked on the strength of a later stat.
fn refuse_unsafe_archive(file: &Path) -> Result<()> {
    let listing = tar_verbose_listing(file)?;
    let members = match parse_tar_listing(&listing) {
        Ok(members) => members,
        Err(err) => bail!("tar {}: {err}; nothing was installed", file.display()),
    };
    for member in members {
        let shown = String::from_utf8_lossy(&member.name);
        if member.name.is_empty() || member.name.contains(&0) {
            bail!(
                "encoder archive {}: a member path is empty; nothing was installed",
                file.display()
            );
        }
        if !archive_member_path_ok(&member.name) {
            bail!(
                "encoder archive {}: member {shown} has an absolute path or '..'; nothing was installed",
                file.display()
            );
        }
        match member.kind {
            b'-' | b'd' => {}
            b'l' => bail!(
                "encoder archive {}: member {shown} is a symlink; nothing was installed",
                file.display()
            ),
            b'h' => bail!(
                "encoder archive {}: member {shown} is a hard link; nothing was installed",
                file.display()
            ),
            _ => bail!(
                "encoder archive {}: member {shown} is not a regular file; nothing was installed",
                file.display()
            ),
        }
    }
    Ok(())
}

fn archive_member_path_ok(name: &[u8]) -> bool {
    use std::os::unix::ffi::OsStrExt;
    use std::path::Component;
    if name.is_empty() || name.contains(&0) || name[0] == b'/' {
        return false;
    }
    let path = Path::new(std::ffi::OsStr::from_bytes(name));
    if path.is_absolute() {
        return false;
    }
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// `tar -tv`. GNU tar is asked for shell quotes. A tar that rejects those
/// flags, such as the one shipped on macOS, is asked again with `-tvzf`
/// only. A listing that does not parse is refused.
fn tar_verbose_listing(file: &Path) -> Result<Vec<u8>> {
    let run = |args: &[&str]| {
        Command::new("tar")
            .args(args)
            .arg(file)
            .stdin(std::process::Stdio::null())
            .env("LC_ALL", "C")
            .output()
            .context("tar")
    };
    let gnu = run(&["--full-time", "--quoting-style=shell-always", "-tvzf"])?;
    if gnu.status.success() {
        return Ok(gnu.stdout);
    }
    let err = String::from_utf8_lossy(&gnu.stderr);
    let unsupported = err.contains("not supported")
        || err.to_ascii_lowercase().contains("unrecognized")
        || err.to_ascii_lowercase().contains("unknown option")
        || err.to_ascii_lowercase().contains("illegal option");
    if !unsupported {
        bail!(
            "tar {}: {}; nothing was installed",
            file.display(),
            err.trim()
        );
    }
    let plain = run(&["-tvzf"])?;
    if !plain.status.success() {
        bail!(
            "tar {}: {}; nothing was installed",
            file.display(),
            String::from_utf8_lossy(&plain.stderr).trim()
        );
    }
    Ok(plain.stdout)
}

/// `tar -tv`, either shell-quoted or the short listing. Names may contain
/// newlines in the quoted form, so those records are split on the quotes.
fn parse_tar_listing(bytes: &[u8]) -> Result<Vec<ArchiveMember>> {
    if let Ok(members) = parse_quoted_tar_listing(bytes) {
        if !members.is_empty() || bytes.iter().all(|byte| *byte == b'\n') {
            return Ok(members);
        }
    }
    parse_plain_tar_listing(bytes)
}

fn parse_quoted_tar_listing(bytes: &[u8]) -> Result<Vec<ArchiveMember>> {
    let mut members = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            i += 1;
            continue;
        }
        let kind = bytes[i];
        let quote = bytes[i..]
            .iter()
            .position(|b| *b == b'\'')
            .context("unquoted tar member")?;
        let start = i + quote;
        let (name, used) = parse_shell_single(&bytes[start..])?;
        let mut j = start + used;
        let rest = &bytes[j..];
        if rest.starts_with(b" -> ") || rest.starts_with(b" link to ") {
            let target = rest
                .iter()
                .position(|b| *b == b'\'')
                .context("unquoted link target")?;
            let (_, used_target) = parse_shell_single(&rest[target..])?;
            j += target + used_target;
        }
        if j < bytes.len() && bytes[j] == b'\n' {
            j += 1;
        } else if j < bytes.len() {
            bail!("tar member list has trailing bytes");
        }
        members.push(ArchiveMember { kind, name });
        i = j;
    }
    Ok(members)
}

/// A `tar -tv` line without shell quotes. The name follows the date.
/// A symlink's target after ` -> ` is not part of the name.
fn parse_plain_tar_listing(bytes: &[u8]) -> Result<Vec<ArchiveMember>> {
    let text = String::from_utf8_lossy(bytes);
    let mut members = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let kind = line.as_bytes()[0];
        let name = plain_member_name(line).context("tar member has no name")?;
        members.push(ArchiveMember {
            kind,
            name: name.into_bytes(),
        });
    }
    if members.is_empty() {
        bail!("tar member list is empty");
    }
    Ok(members)
}

fn plain_member_name(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 4 < bytes.len() {
        let month = bytes[i].is_ascii_uppercase()
            && bytes[i + 1].is_ascii_lowercase()
            && bytes[i + 2].is_ascii_lowercase()
            && bytes[i + 3] == b' ';
        if month {
            let mut parts = line[i..].split_whitespace();
            let _month = parts.next()?;
            let _day = parts.next()?;
            let _when = parts.next()?;
            let name = parts.collect::<Vec<_>>().join(" ");
            let name = name.split(" -> ").next().unwrap_or(&name);
            let name = name.split(" link to ").next().unwrap_or(name);
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            return Some(name.to_string());
        }
        i += 1;
    }
    None
}

/// One shell-always single-quoted word. `'\''` is a literal quote.
fn parse_shell_single(bytes: &[u8]) -> Result<(Vec<u8>, usize)> {
    if bytes.first() != Some(&b'\'') {
        bail!("tar member is not shell-quoted");
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            i += 1;
            if bytes.get(i) == Some(&b'\\') && bytes.get(i + 1) == Some(&b'\'') {
                out.push(b'\'');
                i += 2;
                if bytes.get(i) != Some(&b'\'') {
                    bail!("broken shell quote in tar member");
                }
                i += 1;
                continue;
            }
            return Ok((out, i));
        }
        out.push(bytes[i]);
        i += 1;
    }
    bail!("unterminated shell quote in tar member")
}

/// What the unpack left on disk. `symlink_metadata` does not follow a link,
/// so a link tar created is still a link here.
fn refuse_unsafe_tree(root: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for ent in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let path = ent?.path();
            let meta =
                fs::symlink_metadata(&path).with_context(|| format!("stat {}", path.display()))?;
            let rel = path.strip_prefix(root).unwrap_or(path.as_path());
            let ft = meta.file_type();
            if ft.is_symlink() {
                bail!(
                    "encoder archive member {} is a symlink; nothing was installed",
                    rel.display()
                );
            }
            if ft.is_dir() {
                pending.push(path);
                continue;
            }
            if !ft.is_file() {
                bail!(
                    "encoder archive member {} is not a regular file; nothing was installed",
                    rel.display()
                );
            }
            if meta.nlink() != 1 {
                bail!(
                    "encoder archive member {} is a hard link; nothing was installed",
                    rel.display()
                );
            }
        }
    }
    Ok(())
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

#[cfg(all(test, unix))]
mod archive_member_tests {
    use super::{archive_member_path_ok, parse_tar_listing, refuse_unsafe_archive};

    #[test]
    fn shell_quotes_and_escaping_paths_parse() {
        let mut raw = b"-rw-r--r-- 0/0 3 1970-01-01 00:00:00 'bin/odd '".to_vec();
        raw.extend_from_slice(b"\\''name'\\''\nwith space'\n");
        raw.extend_from_slice(
            b"lrwxrwxrwx ubuntu/ubuntu 0 2026-10-22 01:11:10 'bin/packset-embed' -> '/etc/passwd'\n",
        );
        raw.extend_from_slice(
            b"hrw-r--r-- ubuntu/ubuntu 0 2026-10-22 01:11:10 'bin/real' link to 'bin/packset-embed'\n",
        );
        raw.extend_from_slice(b"prw-r--r-- ubuntu/ubuntu 0 2026-10-22 01:11:10 'bin/fifo'\n");
        raw.extend_from_slice(b"-rw-r--r-- 0/0 17 1970-01-01 00:00:00 '/bin/packset-embed'\n");
        raw.extend_from_slice(b"-rw-r--r-- 0/0 7 1970-01-01 00:00:00 '../escaped'\n");
        let members = parse_tar_listing(&raw).unwrap();
        assert_eq!(members.len(), 6, "{members:?}");
        assert_eq!(members[0].kind, b'-');
        assert_eq!(members[0].name, b"bin/odd 'name'\nwith space");
        assert_eq!(members[1].kind, b'l');
        assert_eq!(members[2].kind, b'h');
        assert_eq!(members[3].kind, b'p');
        assert_eq!(members[4].name, b"/bin/packset-embed");
        assert_eq!(members[5].name, b"../escaped");
        assert!(archive_member_path_ok(&members[0].name));
        assert!(!archive_member_path_ok(&members[4].name));
        assert!(!archive_member_path_ok(&members[5].name));
        assert!(!archive_member_path_ok(b"bin/../../escaped2"));
        assert!(archive_member_path_ok(
            b"lib/onnxruntime/libonnxruntime.so.1"
        ));
        assert!(archive_member_path_ok(b"lib/"));
    }

    #[test]
    fn tar_listing_refuses_a_symlink_member() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("tree");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", dir.join("bin/packset-embed")).unwrap();
        let archive = root.path().join("bad.tar.gz");
        let out = std::process::Command::new("tar")
            .arg("-C")
            .arg(&dir)
            .arg("-czf")
            .arg(&archive)
            .arg("bin")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let err = refuse_unsafe_archive(&archive).unwrap_err().to_string();
        assert!(err.contains("symlink"), "{err}");
        assert!(err.contains("nothing was installed"), "{err}");
    }

    /// The short listing has no shell quotes. A symlink is still named,
    /// and its target is not part of the member path.
    #[test]
    fn a_short_tar_listing_names_a_symlink() {
        let raw = b"\
lrwxr-xr-x  0 0 0 0 Jan  1  1970 bin/packset-embed -> /etc/passwd
-rw-r--r--  0 0 0 17 Jan  1  1970 bin/real
";
        let members = parse_tar_listing(raw).unwrap();
        assert_eq!(members.len(), 2, "{members:?}");
        assert_eq!(members[0].kind, b'l');
        assert_eq!(members[0].name, b"bin/packset-embed");
        assert_eq!(members[1].kind, b'-');
        assert_eq!(members[1].name, b"bin/real");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const SECRET: &str = "AGE-SECRET-KEY-1PLANTED\n";

    fn temp_of(path: &Path) -> PathBuf {
        path.with_file_name(format!(".cloud.age.tmp-{}", std::process::id()))
    }

    #[test]
    fn a_link_planted_at_the_temp_name_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.age");
        let outside = dir.path().join("outside");
        fs::write(&outside, "keep\n").unwrap();
        let tmp = temp_of(&path);
        std::os::unix::fs::symlink(&outside, &tmp).unwrap();
        assert!(write_private(&path, SECRET).is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "keep\n");
        assert!(fs::symlink_metadata(&tmp).unwrap().file_type().is_symlink());
        assert!(!path.exists());
    }

    #[test]
    fn a_readable_file_planted_at_the_temp_name_never_holds_the_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.age");
        let tmp = temp_of(&path);
        fs::write(&tmp, "planted\n").unwrap();
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(write_private(&path, SECRET).is_err());
        assert_eq!(fs::read_to_string(&tmp).unwrap(), "planted\n");
        assert_eq!(
            fs::metadata(&tmp).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert!(!path.exists());

        fs::remove_file(&tmp).unwrap();
        write_private(&path, SECRET).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), SECRET);
        assert_eq!(
            fs::symlink_metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!tmp.exists());
    }

    /// A Dock app looks for a bare `ljos-mcp` on its own PATH, not on this
    /// process's PATH or a login shell's.
    #[test]
    fn a_launchd_app_looks_for_a_bare_server_on_its_own_path() {
        let scratch = tempfile::tempdir().unwrap();
        let bin = scratch.path().join("bin");
        let empty = scratch.path().join("empty");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&empty).unwrap();
        let server = bin.join("ljos-mcp");
        fs::write(&server, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&server, fs::Permissions::from_mode(0o755)).unwrap();
        let launch = LaunchSpec {
            command: "ljos-mcp".into(),
            args: Vec::new(),
            env_path: None,
        };
        let file = |launch_path: &Path| McpFile {
            path: scratch.path().join("claude_desktop_config.json"),
            probe: McpProbe::Json("/mcpServers/ljos".into()),
            advice: "ljos onboard --harness claude-desktop".into(),
            expand: Expand::Shell,
            launch_path: Some(launch_path.display().to_string()),
            strict: false,
        };
        match launch_check(&file(&empty), &launch) {
            LaunchCheck::Missing(why) => {
                assert!(why.contains(&empty.display().to_string()), "{why}");
            }
            _ => panic!("ljos-mcp is not on the app's PATH"),
        }
        assert!(!matches!(
            launch_check(&file(&bin), &launch),
            LaunchCheck::Missing(_)
        ));
    }

    #[test]
    fn mode_fix_refuses_a_symlink_planted_at_the_target_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cloud.age");
        let outside = dir.path().join("outside");
        fs::write(&path, SECRET).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::write(&outside, "keep\n").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();

        assert!(set_mode_0600(&path).is_err());
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o644,
            "a swapped symlink changed another file's mode"
        );
    }

    /// O_NOFOLLOW on the last component still follows a symlink parent, so
    /// chmod would change the file in the directory the link points at.
    #[test]
    fn mode_fix_refuses_a_symlink_parent() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let path = real.join("cloud.age");
        fs::write(&path, SECRET).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let via_dir = dir.path().join("via");
        std::os::unix::fs::symlink(&real, &via_dir).unwrap();
        let via = via_dir.join("cloud.age");

        let err = set_mode_0600(&via).unwrap_err();
        assert!(err.to_string().contains("symlink"), "{err}");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644,
            "a parent symlink changed the target's mode"
        );

        let err = match install_identity(&via, SECRET) {
            Err(err) => err,
            Ok(_) => panic!("parent symlink was accepted"),
        };
        assert!(err.to_string().contains("symlink"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), SECRET);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );

        let err = match install_identity(&via, "AGE-SECRET-KEY-1OTHER\n") {
            Err(err) => err,
            Ok(_) => panic!("parent symlink was written through"),
        };
        assert!(err.to_string().contains("symlink"), "{err}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            SECRET,
            "the identity was written through the parent symlink"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );

        set_mode_0600(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "a direct path is still mode 0600"
        );
    }

    #[test]
    fn a_renamed_server_key_counts_when_its_command_is_the_seat() {
        let doc = serde_json::json!({
            "mcpServers": {
                "my__seat": {"command": "/usr/local/bin/ljos-mcp", "args": []},
                "github": {"command": "github-mcp"},
                "off": {"command": "ljos-mcp", "disabled": true},
                "via": {"command": "ljos", "args": ["mcp"]}
            }
        });
        let keys = json_seat_keys(&doc);
        assert!(keys.contains("my__seat"), "{keys:?}");
        assert!(keys.contains("via"), "{keys:?}");
        assert!(!keys.contains("github"), "{keys:?}");
        assert!(!keys.contains("off"), "{keys:?}");
    }

    #[test]
    fn a_toml_server_table_names_the_seat_key() {
        let doc: toml::Value =
            toml::from_str("[mcp_servers.ljos]\ncommand = \"ljos-mcp\"\nargs = []\n").unwrap();
        let keys = toml_seat_keys(&doc);
        assert!(keys.contains("ljos"), "{keys:?}");
        let other: toml::Value =
            toml::from_str("[mcp_servers.github]\ncommand = \"github-mcp\"\n").unwrap();
        assert!(!toml_seat_keys(&other).contains("github"));
    }

    #[test]
    fn a_yaml_marker_names_the_seat_when_the_command_matches() {
        let text = "extensions:\n  ljos:\n    command: ljos-mcp\n    args: []\n";
        assert_eq!(
            text_marker_key(text, "  ljos:\n    command:"),
            Some("ljos".to_string())
        );
        assert_eq!(text_marker_key(text, "  other:\n    command:"), None);
    }

    #[test]
    fn a_strict_config_that_is_not_a_file_is_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::create_dir(&path).unwrap();
        let file = McpFile {
            path: path.clone(),
            probe: McpProbe::Json("/mcpServers/ljos".into()),
            advice: String::new(),
            expand: Expand::Shell,
            launch_path: None,
            strict: true,
        };
        assert!(matches!(read_seat_keys(&file), KeyRead::Ambiguous));
        let missing = McpFile {
            path: dir.path().join("absent.json"),
            probe: McpProbe::Json("/mcpServers/ljos".into()),
            advice: String::new(),
            expand: Expand::Shell,
            launch_path: None,
            strict: true,
        };
        assert!(matches!(read_seat_keys(&missing), KeyRead::Absent));
        fs::remove_dir(&path).unwrap();
        fs::write(&path, "{not json").unwrap();
        assert!(matches!(read_seat_keys(&file), KeyRead::Ambiguous));
        let loose = McpFile {
            strict: false,
            ..file
        };
        assert!(matches!(read_seat_keys(&loose), KeyRead::Skip));
    }

    #[test]
    fn a_payload_command_names_the_seat_server() {
        assert!(payload_command_is_seat("ljos-mcp"));
        assert!(payload_command_is_seat("/opt/ljos/bin/ljos-mcp"));
        assert!(payload_command_is_seat("ljos-plugin ljos-mcp"));
        assert!(payload_command_is_seat("ljos mcp"));
        assert!(!payload_command_is_seat("github-mcp-server"));
        assert!(!payload_command_is_seat("ljos-plugin"));
        assert!(!payload_command_is_seat(""));
    }

    #[test]
    fn a_large_claude_document_yields_only_mcp_servers() {
        let history = "x".repeat(1000);
        let mut body = String::from(
            r#"{"mcpServers":{"ljos.seat":{"command":"ljos-mcp","args":[]}},"projects":{"/w":{"history":["#,
        );
        body.push('"');
        body.push_str(&history);
        body.push_str(r#""]}}}"#);
        let found = claude_mcp_servers_value(&mut body.as_bytes());
        let ClaudeRead::Servers(text) = found else {
            panic!("mcpServers was not read");
        };
        assert!(text.contains("ljos.seat"), "{text}");
        assert!(!text.contains(&history[..32]), "history was kept: {text}");
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let keys = json_seat_keys(&doc);
        assert!(keys.contains("ljos.seat"), "{keys:?}");
        assert!(keys.contains("ljos_seat"), "{keys:?}");
    }

    #[test]
    fn encoder_ships_for_macos_and_linux_on_x86_64_and_aarch64() {
        for (arch, os, want) in [
            ("x86_64", "linux", "x86_64-unknown-linux-gnu"),
            ("aarch64", "linux", "aarch64-unknown-linux-gnu"),
            ("x86_64", "macos", "x86_64-apple-darwin"),
            ("aarch64", "macos", "aarch64-apple-darwin"),
        ] {
            assert_eq!(encoder_target_for(arch, os), Some(want));
        }
        assert_eq!(encoder_target_for("x86_64", "windows"), None);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let yml = fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let triples: Vec<&str> = yml
            .lines()
            .filter_map(|l| l.trim().strip_prefix("triple: "))
            .collect();
        assert_eq!(
            triples,
            [
                "x86_64-unknown-linux-gnu",
                "aarch64-unknown-linux-gnu",
                "aarch64-apple-darwin",
                "x86_64-apple-darwin",
            ]
        );
        assert!(
            yml.contains("protoc-21.12-osx-universal_binary.zip"),
            "Darwin encoder build must pin protoc 3.21.12"
        );
        assert_eq!(
            yml.matches("ONNX_CUSTOM_PROTOC_EXECUTABLE=$PROTOC").count(),
            2,
            "both macOS and Linux encoder builds pass ONNX_CUSTOM_PROTOC_EXECUTABLE"
        );
        assert_eq!(
            yml.matches("ONNX_CUSTOM_PROTOC_EXECUTABLE:[A-Z]+=$PROTOC")
                .count(),
            2,
            "both macOS and Linux encoder builds verify ONNX_CUSTOM_PROTOC_EXECUTABLE in CMakeCache.txt"
        );
    }
}
