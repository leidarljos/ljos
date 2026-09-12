//! One seat over the habitats. It does not own them.
//!
//! Cards are read-only. Remember/Prefer POST `/v1/atoms` and never extract
//! on write. Consensus is a different crate, then the tracker verb. Policyd
//! is argv law: this process does not reload a pack as a check.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use packset_client::{Hit, PacksetClient};
use serde_json::Value;

/// Working-core files this seat will print. Nothing else, and never write.
pub const CARD_NAMES: &[&str] = &["USER.md", "MEMORY.md"];

/// The sitting protocol: which store answers which question, the order of
/// verbs before, during and after the work, and the refusals worth knowing.
/// `ljos protocol` prints it, `ljos onboard` installs it as a skill, and the
/// server serves it at `ljos://protocol`. Harness agnostic on purpose.
pub const PROTOCOL: &str = include_str!("../doc/protocol.md");

/// The skill file a harness loads: front matter, then the protocol.
#[must_use]
pub fn skill_text() -> String {
    format!(
        "---\nname: ljos\ndescription: >\n  The seat protocol for vissue, packset, deedar, claimdag and \
consensus through ljos: which store answers which question, the order of verbs in a \
sitting, and the refusals worth knowing. Load before any work that touches an issue, \
a memory, a deed, a claim or a vote.\n---\n\n{PROTOCOL}"
    )
}

/// One step an onboarding took, or would take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub what: String,
    pub detail: String,
    pub ok: bool,
}

/// One agent runner, as the seat's own configuration describes it. The seat
/// ships no runner's name: the file at [`harnesses_path`] names them, one
/// table each, and `onboard` and `doctor` read it.
///
/// A runner registers MCP servers one of two ways. `register` is a command
/// that does it (`{server}` is replaced by the path to `ljos-mcp`) and
/// `registered` a command that exits 0 once it is done. Or `config` is a
/// file the runner reads, `marker` a line that means the entry is present,
/// and `snippet` what to append when it is not. `skills` is the directory
/// the runner loads skills from; the protocol goes to `<skills>/ljos/SKILL.md`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Harness {
    pub name: String,
    #[serde(default)]
    pub register: Vec<String>,
    #[serde(default)]
    pub registered: Vec<String>,
    #[serde(default)]
    pub config: Option<String>,
    #[serde(default)]
    pub marker: Option<String>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub skills: Option<String>,
    /// A JSON settings file the runner reads hooks from, in the shape
    /// `{"hooks": {"<Event>": [{"matcher": "...", "hooks": [{"type":
    /// "command", "command": "..."}]}]}}`. `onboard` merges the seat's
    /// memory hook into it, so what the seat knows about a command or a
    /// prompt reaches the agent at the point of action.
    #[serde(default)]
    pub hooks: Option<String>,
    /// The events the memory hook fires on. Empty means [`HOOK_EVENTS`],
    /// the prompt event alone: a panel of this seat's personas settled on
    /// prompts over tool calls, because a turn issues many shell commands
    /// and one prompt. `["UserPromptSubmit", "PreToolUse"]` injects on both.
    #[serde(default)]
    pub hook_events: Vec<String>,
}

/// The whole file: `[[harness]]` tables.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Harnesses {
    #[serde(default)]
    pub harness: Vec<Harness>,
}

/// An example of the file, with placeholder names. `ljos onboard --example`
/// prints it; the two shapes are a registering command and a config file.
pub const HARNESSES_EXAMPLE: &str = r#"# ~/.config/ljos/harnesses.toml: the agent runners on this machine.
# {server} is replaced by the path to ljos-mcp. Paths may start with ~.

[[harness]]
name = "runner-with-a-command"
register = ["runner", "mcp", "add", "-s", "user", "ljos", "--", "{server}"]
registered = ["runner", "mcp", "get", "ljos"]
skills = "~/.runner/skills"
hooks = "~/.runner/settings.json"
# hook_events = ["UserPromptSubmit", "PreToolUse"]   # the default is the prompt alone

[[harness]]
name = "runner-with-a-config-file"
config = "~/.other/config.toml"
marker = "[mcp_servers.ljos]"
snippet = "\n[mcp_servers.ljos]\ncommand = \"{server}\"\nargs = []\n"
skills = "~/.other/skills"
"#;

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME unset; onboard needs a home directory")
}

/// `~` at the start of a configured path is the home directory.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().map_or_else(|_| PathBuf::from(path), |h| h.join(rest)),
        None => PathBuf::from(path),
    }
}

/// Where the runners are described: `$XDG_CONFIG_HOME/ljos/harnesses.toml`.
#[must_use]
pub fn harnesses_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|r| !r.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().ok().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("ljos")
        .join("harnesses.toml")
}

/// Parse the runners file. An absent file is no runners, not an error.
///
/// # Errors
///
/// A file that is present and not this shape.
pub fn harnesses_from(path: &Path) -> Result<Harnesses> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("{}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Harnesses::default()),
        Err(e) => Err(e).with_context(|| format!("{}", path.display())),
    }
}

/// Where `ljos-mcp` is, as the runner will start it.
fn server_path() -> Result<PathBuf> {
    which::which("ljos-mcp").context("ljos-mcp not on PATH; install it beside ljos")
}

/// The MCP server entry any runner that reads JSON accepts.
pub fn server_entry() -> Result<Value> {
    Ok(serde_json::json!({
        "mcpServers": {
            "ljos": {
                "type": "stdio",
                "command": server_path()?.display().to_string(),
                "args": [],
                "env": {}
            }
        }
    }))
}

fn write_skill(dir: &Path, dry: bool) -> Step {
    let path = dir.join("ljos").join("SKILL.md");
    let text = skill_text();
    if std::fs::read_to_string(&path).is_ok_and(|have| have == text) {
        return Step {
            what: "skill".into(),
            detail: format!("{} is current", path.display()),
            ok: true,
        };
    }
    if dry {
        return Step {
            what: "skill".into(),
            detail: format!("would write {}", path.display()),
            ok: true,
        };
    }
    let written = std::fs::create_dir_all(path.parent().unwrap_or(dir))
        .and_then(|()| std::fs::write(&path, text));
    match written {
        Ok(()) => Step {
            what: "skill".into(),
            detail: format!("wrote {}", path.display()),
            ok: true,
        },
        Err(e) => Step {
            what: "skill".into(),
            detail: format!("{}: {e}", path.display()),
            ok: false,
        },
    }
}

fn filled(argv: &[String], server: &Path) -> Vec<String> {
    argv.iter()
        .map(|a| a.replace("{server}", &server.display().to_string()))
        .collect()
}

/// Whether a runner with a `registered` command already has the server.
fn is_registered(h: &Harness, server: &Path) -> Option<bool> {
    if !h.registered.is_empty() {
        let argv = filled(&h.registered, server);
        return Some(
            argv.first().is_some_and(|bin| on_path(bin)) && {
                let (bin, rest) = (&argv[0], &argv[1..]);
                run_captured(bin, rest).is_ok()
            },
        );
    }
    if let (Some(config), Some(marker)) = (&h.config, &h.marker) {
        return Some(std::fs::read_to_string(expand(config)).is_ok_and(|t| t.contains(marker)));
    }
    None
}

fn register_step(h: &Harness, server: &Path, dry: bool) -> Step {
    let what = format!("{} mcp", h.name);
    match is_registered(h, server) {
        Some(true) => Step {
            what,
            detail: "ljos registered".into(),
            ok: true,
        },
        None => Step {
            what,
            detail: "no register or config in harnesses.toml; paste `ljos onboard --harness json`"
                .into(),
            ok: false,
        },
        Some(false) if !h.register.is_empty() => {
            let argv = filled(&h.register, server);
            if !on_path(&argv[0]) {
                return Step {
                    what,
                    detail: format!("{} not on PATH", argv[0]),
                    ok: false,
                };
            }
            if dry {
                return Step {
                    what,
                    detail: format!("would run {}", argv.join(" ")),
                    ok: true,
                };
            }
            match run_captured(&argv[0], &argv[1..]) {
                Ok(_) => Step {
                    what,
                    detail: format!("ran {}", argv.join(" ")),
                    ok: true,
                },
                Err(e) => Step {
                    what,
                    detail: e.to_string().lines().next().unwrap_or("").to_string(),
                    ok: false,
                },
            }
        }
        Some(false) => {
            let config = expand(h.config.as_deref().unwrap_or_default());
            let snippet = h
                .snippet
                .as_deref()
                .unwrap_or_default()
                .replace("{server}", &server.display().to_string());
            if snippet.is_empty() {
                return Step {
                    what,
                    detail: format!("no snippet to append to {}", config.display()),
                    ok: false,
                };
            }
            if dry {
                return Step {
                    what,
                    detail: format!("would append the entry to {}", config.display()),
                    ok: true,
                };
            }
            let mut text = std::fs::read_to_string(&config).unwrap_or_default();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&snippet);
            let written = config
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&config, text));
            match written {
                Ok(()) => Step {
                    what,
                    detail: format!("appended the entry to {}", config.display()),
                    ok: true,
                },
                Err(e) => Step {
                    what,
                    detail: format!("{}: {e}", config.display()),
                    ok: false,
                },
            }
        }
    }
}

/// Register the server and install the skill for one runner named in the
/// runners file. `json` registers nothing and returns the entry to paste.
/// `dry` reports without writing.
///
/// # Errors
///
/// No such runner in the file, no home directory, or `ljos-mcp` not on `PATH`.
pub fn onboard(harness: &str, dry: bool) -> Result<Vec<Step>> {
    onboard_from(&harnesses_path(), harness, dry)
}

pub fn onboard_from(file: &Path, harness: &str, dry: bool) -> Result<Vec<Step>> {
    if harness == "json" {
        return Ok(vec![Step {
            what: "json".into(),
            detail: serde_json::to_string_pretty(&server_entry()?)?,
            ok: true,
        }]);
    }
    let all = harnesses_from(file)?;
    let Some(h) = all.harness.iter().find(|h| h.name == harness) else {
        let names: Vec<&str> = all.harness.iter().map(|h| h.name.as_str()).collect();
        bail!(
            "onboard: no runner {harness:?} in {}; it names {}. `ljos onboard --example` \
             prints the file's shape, and `--harness json` prints the entry to paste anywhere.",
            file.display(),
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        );
    };
    let server = server_path()?;
    let mut steps = vec![host_key_step(dry), register_step(h, &server, dry)];
    if let Some(file) = &h.hooks {
        steps.push(hook_step(&expand(file), &hook_events_of(h), dry));
    }
    match &h.skills {
        Some(dir) => steps.push(write_skill(&expand(dir), dry)),
        None => steps.push(Step {
            what: "skill".into(),
            detail: "no skills directory in harnesses.toml; `ljos protocol` prints the text".into(),
            ok: false,
        }),
    }
    Ok(steps)
}

/// The events the memory hook fires on when a runner's table names none:
/// the prompt, which carries the task in the person's words. A tool call
/// carries the command about to run and is a cue too; a runner asks for it
/// with `hook_events`. The default came out of a panel of this seat's
/// personas: a turn issues many shell commands and one prompt.
pub const HOOK_EVENTS: &[&str] = &["UserPromptSubmit"];

/// The events the hook knows a matcher for; any other event takes `*`.
pub const HOOK_MATCHERS: &[(&str, &str)] = &[("PreToolUse", "Bash"), ("UserPromptSubmit", "*")];

fn hook_matcher(event: &str) -> &'static str {
    HOOK_MATCHERS
        .iter()
        .find(|(e, _)| *e == event)
        .map_or("*", |(_, m)| m)
}

/// The events a runner's table asks for, or the default.
fn hook_events_of(h: &Harness) -> Vec<String> {
    if h.hook_events.is_empty() {
        HOOK_EVENTS.iter().map(|e| (*e).to_string()).collect()
    } else {
        h.hook_events.clone()
    }
}

fn is_seat_hook(h: &Value) -> bool {
    h["command"]
        .as_str()
        .is_some_and(|c| c.contains("ljos") && c.ends_with(" hook"))
}

/// The command the runner's hook runs.
fn hook_command() -> String {
    which::which("ljos").map_or_else(
        |_| "ljos hook".to_string(),
        |p| format!("{} hook", p.display()),
    )
}

/// Merge the seat's memory hook into a runner's hooks file, once per event.
/// The file is JSON with a `hooks` object of event name to matcher groups;
/// a group whose command is the seat's is left alone, so the step is
/// idempotent.
fn hook_step(file: &Path, events: &[String], dry: bool) -> Step {
    let what = "hook".to_string();
    let mut root: Value = match std::fs::read_to_string(file) {
        Ok(text) if !text.trim().is_empty() => match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                return Step {
                    what,
                    detail: format!("{}: not JSON: {e}", file.display()),
                    ok: false,
                }
            }
        },
        _ => serde_json::json!({}),
    };
    let command = hook_command();
    let Some(obj) = root.as_object_mut() else {
        return Step {
            what,
            detail: format!("{}: not a JSON object", file.display()),
            ok: false,
        };
    };
    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return Step {
            what,
            detail: format!("{}: hooks is not an object", file.display()),
            ok: false,
        };
    };
    // Reconcile: the seat's hook is on the events asked for and on no
    // other, and every group that is not the seat's is left alone.
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for event in events {
        let groups = hooks
            .entry(event.clone())
            .or_insert_with(|| serde_json::json!([]));
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        let present = groups.iter().any(|g| {
            g["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .any(is_seat_hook)
        });
        if present {
            continue;
        }
        groups.push(serde_json::json!({
            "matcher": hook_matcher(event),
            "hooks": [{"type": "command", "command": command, "timeout": 20}]
        }));
        added.push(event.clone());
    }
    for (event, groups) in hooks.iter_mut() {
        if events.contains(event) {
            continue;
        }
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        let before = groups.len();
        groups.retain(|g| {
            !g["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .any(is_seat_hook)
        });
        if groups.len() != before {
            removed.push(event.clone());
        }
    }
    if added.is_empty() && removed.is_empty() {
        return Step {
            what,
            detail: format!(
                "{} carries the memory hook on {}",
                file.display(),
                events.join(", ")
            ),
            ok: true,
        };
    }
    let mut change = Vec::new();
    if !added.is_empty() {
        change.push(format!("add it on {}", added.join(", ")));
    }
    if !removed.is_empty() {
        change.push(format!("drop it from {}", removed.join(", ")));
    }
    let change = change.join(" and ");
    if dry {
        return Step {
            what,
            detail: format!("would {change} in {}", file.display()),
            ok: true,
        };
    }
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| serde_json::to_string_pretty(&root).map_err(std::io::Error::other))
        .and_then(|text| std::fs::write(file, text + "\n"));
    match written {
        Ok(()) => Step {
            what,
            detail: format!("memory hook: {change} in {}", file.display()),
            ok: true,
        },
        Err(e) => Step {
            what,
            detail: format!("{}: {e}", file.display()),
            ok: false,
        },
    }
}

/// Whether a runner's hooks file carries the memory hook on every event.
fn hook_installed(file: &Path, events: &[String]) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    events.iter().all(|event| {
        root["hooks"][event.as_str()]
            .as_array()
            .into_iter()
            .flatten()
            .any(|g| {
                g["hooks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(is_seat_hook)
            })
    })
}

/// What the runner's hook hands the seat: the event, and the text worth
/// asking the pack about. From a tool call, the command about to run; from
/// a prompt, the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCall {
    pub event: String,
    pub cue: String,
    /// The runner's session, when it says: each memory is injected once
    /// per session, so the same lesson does not arrive on every command.
    pub session: Option<String>,
}

/// Read a hook call from the runner's JSON, or from plain text (an argv
/// under argv law). Fields: `hook_event_name`, `tool_name`, `tool_input`
/// (its `command`, else every string value joined), `prompt`.
#[must_use]
pub fn hook_call(input: &str) -> HookCall {
    let trimmed = input.trim();
    let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
        return HookCall {
            event: "argv".into(),
            cue: trimmed.to_string(),
            session: None,
        };
    };
    let session = v["session_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let event = v["hook_event_name"]
        .as_str()
        .unwrap_or("PreToolUse")
        .to_string();
    let cue = if let Some(p) = v["prompt"].as_str() {
        p.to_string()
    } else if let Some(c) = v["tool_input"]["command"].as_str() {
        c.to_string()
    } else if let Some(map) = v["tool_input"].as_object() {
        map.values()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    };
    HookCall {
        event,
        cue,
        session,
    }
}

/// Where the ids already injected in a session are kept: the runtime
/// directory, so they go with the login and never into the pack.
fn seen_path(session: &str) -> Option<PathBuf> {
    let safe: String = session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return None;
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|r| !r.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("ljos");
    Some(dir.join(format!("hook-seen-{safe}")))
}

fn seen_ids(session: Option<&str>) -> std::collections::BTreeSet<String> {
    session
        .and_then(seen_path)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

fn mark_seen(session: Option<&str>, ids: &[String]) {
    let Some(path) = session.and_then(seen_path) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    for id in ids {
        text.push_str(id);
        text.push('\n');
    }
    let _ = std::fs::write(path, text);
}

/// The floor a hit must reach, as a share of the strongest hit's score, to
/// be injected. A command line matches many claims weakly; only the ones
/// that match it as well as the best does are worth the agent's context.
pub const HOOK_SCORE_FLOOR: f64 = 0.6;

/// The context the hook injects: the island the cue activates, standing
/// preferences first because they bear on what to do, then lessons. Empty
/// when the pack holds nothing on it or does not answer; a hook that fails
/// must not stop the runner, so this never errors.
#[must_use]
pub fn hook_context(call: &HookCall, limit: usize) -> String {
    let cue = call.cue.trim();
    if cue.len() < 3 {
        return String::new();
    }
    let Ok(hits) = packset_search(cue) else {
        return String::new();
    };
    let top = hits.iter().map(|h| h.score).fold(0.0_f64, f64::max);
    if top <= 0.0 {
        return String::new();
    }
    let seen = seen_ids(call.session.as_deref());
    let mut rows: Vec<&Hit> = hits
        .iter()
        .filter(|h| !UNREVIEWED_KINDS.contains(&h.kind.as_str()))
        .filter(|h| h.score >= top * HOOK_SCORE_FLOOR)
        .filter(|h| h.id.as_ref().is_none_or(|id| !seen.contains(id)))
        .collect();
    rows.sort_by(|a, b| {
        let pa = a.kind == "preference";
        let pb = b.kind == "preference";
        pb.cmp(&pa).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let rows: Vec<&Hit> = rows.into_iter().take(limit).collect();
    let lines: Vec<String> = rows
        .iter()
        .map(|h| {
            format!(
                "- [{}] {}",
                if h.kind.is_empty() { "claim" } else { &h.kind },
                h.text.trim()
            )
        })
        .collect();
    let nudge = due_nudge(call);
    if lines.is_empty() {
        return nudge;
    }
    mark_seen(
        call.session.as_deref(),
        &rows.iter().filter_map(|h| h.id.clone()).collect::<Vec<_>>(),
    );
    let mut out = format!(
        "What this seat already knows that bears on this (from the pack; `ljos search` for more):\n{}",
        lines.join("\n")
    );
    if !nudge.is_empty() {
        out.push('\n');
        out.push_str(&nudge);
    }
    out
}

/// On a prompt, once per session: how many claims are due for review. The
/// review loop runs only when somebody grades, and nobody grades what they
/// were not told about.
fn due_nudge(call: &HookCall) -> String {
    if call.event != "UserPromptSubmit" {
        return String::new();
    }
    let key = "due-nudge".to_string();
    if seen_ids(call.session.as_deref()).contains(&key) {
        return String::new();
    }
    let Ok(client) = pack() else {
        return String::new();
    };
    let Ok(atoms) = client.atoms_as_of(&client.workspace(), None) else {
        return String::new();
    };
    let due = due_of(&atoms, &now_utc()).len();
    if due == 0 {
        return String::new();
    }
    mark_seen(call.session.as_deref(), &[key]);
    format!(
        "{due} claim{} due for review in this seat: `ljos due`, read each, then `ljos graded ID` (or `--lapsed`).",
        if due == 1 { " is" } else { "s are" }
    )
}

/// The hook's answer in the runner's JSON: `additionalContext` under the
/// event that fired. Empty context is no output, which the runner reads as
/// no opinion.
#[must_use]
pub fn hook_output(call: &HookCall, context: &str) -> String {
    if context.is_empty() {
        return String::new();
    }
    if call.event == "argv" {
        return format!("{context}\n");
    }
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": call.event,
            "additionalContext": context
        }
    })
    .to_string()
        + "\n"
}

pub fn format_steps(steps: &[Step]) -> String {
    steps
        .iter()
        .map(|s| {
            format!(
                "{}\t{}\t{}\n",
                if s.ok { "ok" } else { "no" },
                s.what,
                s.detail
            )
        })
        .collect()
}

/// The runner rows for `doctor`, one pair per runner the file names.
fn harness_rows() -> Vec<Habitat> {
    let path = harnesses_path();
    let all = match harnesses_from(&path) {
        Ok(all) => all,
        Err(e) => {
            return vec![Habitat {
                name: "runners",
                state: format!("{e:#}"),
                ok: false,
            }]
        }
    };
    if all.harness.is_empty() {
        return vec![Habitat {
            name: "runners",
            state: format!(
                "none named in {}; `ljos onboard --example` prints the shape",
                path.display()
            ),
            ok: false,
        }];
    }
    let server = server_path().unwrap_or_else(|_| PathBuf::from("ljos-mcp"));
    let mut rows = Vec::new();
    for h in &all.harness {
        let registered = is_registered(h, &server) == Some(true);
        rows.push(Habitat {
            name: "runner mcp",
            state: if registered {
                format!("{}: ljos registered", h.name)
            } else {
                format!(
                    "{}: not registered; ljos onboard --harness {}",
                    h.name, h.name
                )
            },
            ok: registered,
        });
        let skill = h
            .skills
            .as_deref()
            .map(|d| expand(d).join("ljos").join("SKILL.md"));
        let current = skill
            .as_ref()
            .is_some_and(|p| std::fs::read_to_string(p).is_ok_and(|t| t == skill_text()));
        if let Some(file) = &h.hooks {
            let path = expand(file);
            let installed = hook_installed(&path, &hook_events_of(h));
            rows.push(Habitat {
                name: "runner hook",
                state: if installed {
                    format!("{}: memory hook on {}", h.name, path.display())
                } else {
                    format!(
                        "{}: no memory hook; ljos onboard --harness {}",
                        h.name, h.name
                    )
                },
                ok: installed,
            });
        }
        rows.push(Habitat {
            name: "runner skill",
            state: match (&skill, current) {
                (Some(p), true) => format!("{}: {}", h.name, p.display()),
                (Some(p), false) if p.is_file() => {
                    format!(
                        "{}: {} is stale; ljos onboard --harness {}",
                        h.name,
                        p.display(),
                        h.name
                    )
                }
                (Some(_), false) => {
                    format!("{}: absent; ljos onboard --harness {}", h.name, h.name)
                }
                (None, _) => format!("{}: no skills directory named", h.name),
            },
            ok: current,
        });
    }
    rows
}

/// Make the seat's host key at `~/.config/deedar/host.key` when there is
/// none, so handovers go out signed from the first one. An existing key, or
/// one named by `DEEDAR_HOST_SIGNING_KEY`, is left alone.
fn host_key_step(dry: bool) -> Step {
    if let Some(path) = host_key_path() {
        return Step {
            what: "host key".into(),
            detail: format!("{} exists", path.display()),
            ok: true,
        };
    }
    if std::env::var_os("DEEDAR_HOST_SIGNING_KEY").is_some_and(|r| r == "off") {
        return Step {
            what: "host key".into(),
            detail: "DEEDAR_HOST_SIGNING_KEY=off; handovers go out unsigned on purpose".into(),
            ok: true,
        };
    }
    let Some(path) = default_host_key_path() else {
        return Step {
            what: "host key".into(),
            detail: "no home directory to keep a key in".into(),
            ok: false,
        };
    };
    if dry {
        return Step {
            what: "host key".into(),
            detail: format!("would write a 32-byte seed to {}", path.display()),
            ok: true,
        };
    }
    let made = (|| -> std::io::Result<()> {
        use std::io::Read;
        let mut seed = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut seed)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, seed)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    })();
    match made {
        Ok(()) => Step {
            what: "host key".into(),
            detail: format!("wrote a 32-byte seed to {}", path.display()),
            ok: true,
        },
        Err(e) => Step {
            what: "host key".into(),
            detail: format!("{}: {e}", path.display()),
            ok: false,
        },
    }
}

/// `$XDG_CONFIG_HOME/deedar/host.key`, whether or not it exists.
fn default_host_key_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|r| !r.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().ok().map(|h| h.join(".config")))?;
    Some(config.join("deedar").join("host.key"))
}

/// The host key `deedar` will sign with: `DEEDAR_HOST_SIGNING_KEY`, else
/// `~/.config/deedar/host.key` when it exists. `off` is no key on purpose.
fn host_key_path() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("DEEDAR_HOST_SIGNING_KEY").filter(|r| !r.is_empty()) {
        return (raw != "off").then(|| PathBuf::from(raw));
    }
    let path = default_host_key_path()?;
    path.is_file().then_some(path)
}

/// Printed on stderr. `grok-policyd` is the TCB when it exists.
pub const POLICY_TCB: &str =
    "argv law. grok-policyd is the TCB when present. Reloading a pack is not a check.";

/// The workspace the seat's memory lives in when nothing names one. The
/// pack's command line keys a workspace to the repository it stands in;
/// a seat is one memory across every repository it works in, so the seat
/// pins one. `PACKSET_WORKSPACE` overrides it.
pub const SEAT_WORKSPACE: &str = "seat";

/// The pack client. With nothing set it speaks to `127.0.0.1:8761` about
/// the `seat` workspace; `PACKSET_URL` points elsewhere, `PACKSET_WORKSPACE`
/// names another workspace, and `PACKSET_URL=off` is the one way to have no
/// pack.
pub fn pack() -> Result<PacksetClient> {
    let workspace = std::env::var("PACKSET_WORKSPACE")
        .ok()
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| SEAT_WORKSPACE.to_string());
    Ok(PacksetClient::from_env()
        .context("PACKSET_URL=off: this seat has no pack on purpose")?
        .with_workspace(workspace))
}

pub fn join(parts: &[String]) -> String {
    parts.join(" ")
}

/// Remember → lesson, Prefer → preference. Trust rows go through [`trust_atom`].
pub fn atom_kind(label: &str) -> Result<&'static str> {
    match label {
        "Remember" => Ok("lesson"),
        "Prefer" => Ok("preference"),
        other => bail!("unknown write kind {other}"),
    }
}

/// Explicit claim body. The text is stored as given; never harvested.
pub fn atom_body(kind: &str, text: &str, workspace: &str) -> Value {
    serde_json::json!({
        "schema": "inside.atom/v1",
        "kind": kind,
        "level": "explicit",
        "text": text,
        "workspace": workspace,
    })
}

/// POST one explicit claim. Callers pass Remember/Prefer only.
pub fn post_claim(
    client: &PacksetClient,
    label: &str,
    text: &str,
    workspace: &str,
) -> Result<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("{label}: empty text is not a claim");
    }
    let kind = atom_kind(label)?;
    let atom = atom_body(kind, trimmed, workspace);
    client
        .post_atom(&atom)
        .with_context(|| format!("{label}: POST /v1/atoms failed"))
}

pub fn packset_write(label: &str, text: &str) -> Result<Value> {
    let client = pack()?;
    let workspace = client.workspace();
    post_claim(&client, label, text, &workspace)
}

/// Retire one atom from the workspace the cwd resolves to, optionally naming
/// the deed that withdrew it.
///
/// The daemon tombstones rather than erases: the atom stops being recalled and
/// the pack still records that it was held and withdrawn. That is the right
/// shape for standing knowledge, where "we no longer believe this" is itself
/// worth keeping.
///
/// `why` is a deed accession and the pack refuses free text in its place. It
/// runs the same join as a remembered claim's `entities`, in the same
/// direction: the pack cites the deed store, never the other way round. A
/// retraction the work justified is therefore checkable with `deedar evidence`
/// like any other citation, and one nothing justified simply carries no `why`.
///
/// # Errors
///
/// An unset `PACKSET_URL`, an id the workspace does not hold, a `why` that is
/// not an accession, or the request's.
pub fn packset_forget(id: &str, why: Option<&str>) -> Result<Value> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("forget: an atom id is required");
    }
    let why = why.map(str::trim).filter(|w| !w.is_empty());
    let client = pack()?;
    let workspace = client.workspace();
    client
        .delete_atom(&workspace, trimmed, why)
        .with_context(|| format!("forget: POST /v1/atoms/delete failed for {trimmed}"))
}

/// One row of the influence graph: `from` listens to `to` with `weight`.
/// `about` scopes the row to the domains it speaks to: a row with none
/// applies everywhere, a row with some applies when one of them meets the
/// issue at hand (its title, or the entities of the island it activates).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Trust {
    pub from: String,
    pub to: String,
    pub weight: f64,
    pub about: Vec<String>,
}

/// A voter with a view of its own: a persona. `anchor` in `[0, 1]` is how
/// far it moves off its ballot in a settle; 0 never moves, 1 is a plain
/// DeGroot voter. `entities` are the domains it speaks to.
#[derive(Debug, Clone, PartialEq)]
pub struct Persona {
    pub name: String,
    pub anchor: f64,
    pub view: String,
    pub entities: Vec<String>,
}

/// The `persona` atom for the pack: kind `persona`, the view as text.
///
/// # Errors
///
/// An empty name, an anchor outside `[0, 1]`, or an empty view.
pub fn persona_atom(p: &Persona, workspace: &str) -> Result<Value> {
    let name = p.name.trim();
    if name.is_empty() {
        bail!("persona: a name is required");
    }
    if !(0.0..=1.0).contains(&p.anchor) {
        bail!("persona: anchor {} is not in [0, 1]", p.anchor);
    }
    let view = p.view.trim();
    if view.is_empty() {
        bail!("persona: say in a sentence or two how {name} reads the work");
    }
    let mut atom = atom_body("persona", view, workspace);
    atom["name"] = Value::String(name.into());
    atom["anchor"] = serde_json::json!(p.anchor);
    if !p.entities.is_empty() {
        atom["entities"] = Value::Array(
            p.entities
                .iter()
                .map(|e| Value::String(e.to_lowercase()))
                .collect(),
        );
    }
    Ok(atom)
}

/// POST one persona.
pub fn write_persona(p: &Persona) -> Result<Value> {
    let client = pack()?;
    let workspace = client.workspace();
    client
        .post_atom(&persona_atom(p, &workspace)?)
        .context("persona: POST /v1/atoms failed")
}

/// The live personas: the latest `persona` atom per name.
pub fn personas_of(atoms: &[Value]) -> Vec<Persona> {
    let mut latest: std::collections::BTreeMap<String, (String, Persona)> =
        std::collections::BTreeMap::new();
    for atom in atoms {
        if atom.get("kind").and_then(Value::as_str) != Some("persona") {
            continue;
        }
        let (Some(name), Some(anchor)) = (
            atom.get("name").and_then(Value::as_str),
            atom.get("anchor").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let ts = atom
            .get("ts")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let p = Persona {
            name: name.to_string(),
            anchor,
            view: atom
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            entities: words_of(atom.get("entities")),
        };
        match latest.get(name) {
            Some((seen, _)) if *seen > ts => {}
            _ => {
                latest.insert(name.to_string(), (ts, p));
            }
        }
    }
    latest.into_values().map(|(_, p)| p).collect()
}

/// The personas in the seat's pack.
pub fn personas_from_pack() -> Result<Vec<Persona>> {
    let client = pack()?;
    let atoms = client
        .atoms_as_of(&client.workspace(), None)
        .context("persona: GET /v1/atoms failed")?;
    Ok(personas_of(&atoms))
}

/// The brief a subagent playing a persona starts from: the persona's view
/// and domains, what the seat knows on those domains (preferences first),
/// and the issue's working set. One text, so a panel member reads the
/// same seat the rest do and still reads it its own way.
///
/// # Errors
///
/// No such persona in the pack, or the tracker or pack not answering.
pub fn brief(name: &str, issue: &str) -> Result<String> {
    let personas = personas_from_pack()?;
    let Some(p) = personas.iter().find(|p| p.name == name) else {
        let names: Vec<&str> = personas.iter().map(|p| p.name.as_str()).collect();
        bail!(
            "brief: no persona {name:?} in the pack; the pack holds {}",
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        );
    };
    let mut out = format!(
        "You are {}. {}\nYou hold your ballot at anchor {:.2}{}.\n",
        p.name,
        p.view,
        p.anchor,
        if p.entities.is_empty() {
            String::new()
        } else {
            format!("; you speak to {}", p.entities.join(", "))
        }
    );
    let mut seen = std::collections::BTreeSet::new();
    let mut lines = Vec::new();
    let cues: Vec<String> = if p.entities.is_empty() {
        vec![issue_title(issue)?]
    } else {
        p.entities.clone()
    };
    for cue in &cues {
        let Ok(hits) = packset_search(cue) else {
            continue;
        };
        for h in hits.into_iter().take(5) {
            if UNREVIEWED_KINDS.contains(&h.kind.as_str()) {
                continue;
            }
            if let Some(id) = &h.id {
                if !seen.insert(id.clone()) {
                    continue;
                }
            }
            lines.push((
                h.kind == "preference",
                format!("- [{}] {}", h.kind, h.text.trim()),
            ));
        }
    }
    lines.sort_by(|a, b| b.0.cmp(&a.0));
    if !lines.is_empty() {
        out.push_str("\nWhat this seat knows on your domains:\n");
        for (_, l) in lines.iter().take(8) {
            out.push_str(l);
            out.push('\n');
        }
    }
    out.push_str("\nThe work:\n");
    out.push_str(&run_captured("vissue", &["recall", issue])?.stdout);
    out.push_str(&format!(
        "\nRead it your way and end with one ballot: `ljos vote {issue} --for OPTION --as {}`.\n",
        p.name
    ));
    Ok(out)
}

/// Anchors as the settles take them: `{"name": anchor, ...}`.
pub fn anchors_json(personas: &[Persona]) -> String {
    let map: serde_json::Map<String, Value> = personas
        .iter()
        .map(|p| (p.name.clone(), serde_json::json!(p.anchor)))
        .collect();
    Value::Object(map).to_string()
}

fn words_of(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_lowercase)
        .collect()
}

/// The domains an issue's island speaks to: the entities of the memories
/// its title activates, most frequent first, eight at most. What `learn`
/// scopes its rows to.
///
/// # Errors
///
/// The tracker or the pack not answering.
pub fn island_entities(issue: &str) -> Result<Vec<String>> {
    let title = issue_title(issue)?;
    let island = packset_island(&title, false)?;
    let ids: Vec<&str> = island["island"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["id"].as_str())
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let client = pack()?;
    let atoms = client
        .atoms_as_of(&client.workspace(), None)
        .context("island: GET /v1/atoms failed")?;
    let mut count: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for atom in &atoms {
        if atom
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| ids.contains(&id))
        {
            for e in words_of(atom.get("entities")) {
                *count.entry(e).or_insert(0) += 1;
            }
        }
    }
    let mut ranked: Vec<(String, usize)> = count.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(ranked.into_iter().take(8).map(|(e, _)| e).collect())
}

/// The words an issue is about, for scoping trust rows: its title, lower
/// case, three letters or longer.
pub fn topic_words(title: &str) -> Vec<String> {
    let mut words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(str::to_lowercase)
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

/// The rows that apply to an issue about `topic`: every unscoped row, and
/// every scoped row one of whose domains is among the topic's words.
pub fn rows_about(rows: &[Trust], topic: &[String]) -> Vec<Trust> {
    // A scoped row that applies stands in for the unscoped row of the same
    // pair, so the settle sees one weight per pair and never a sum of two.
    let mut chosen: std::collections::BTreeMap<(String, String), Trust> =
        std::collections::BTreeMap::new();
    for r in rows {
        let applies = r.about.is_empty() || r.about.iter().any(|a| topic.contains(a));
        if !applies {
            continue;
        }
        let key = (r.from.clone(), r.to.clone());
        match chosen.get(&key) {
            Some(have) if !have.about.is_empty() && r.about.is_empty() => {}
            _ => {
                chosen.insert(key, r.clone());
            }
        }
    }
    chosen.into_values().collect()
}

/// The personas after an outcome: one whose ballot the outcome refuted
/// moves its anchor toward one by `1 - beta` of the gap, so a persona that
/// keeps being wrong listens more; a vindicated one keeps its anchor. The
/// personas that voted are the only ones touched. Acemoglu, Como, Fagnani
/// and Ozdaglar (doi:10.1287/moor.1120.0570) show what a stubborn wrong
/// voter does to a pool; this is the seat's remedy.
#[must_use]
pub fn learn_anchors(
    personas: &[Persona],
    ballots: &[(String, String)],
    outcome: &str,
    beta: f64,
) -> Vec<Persona> {
    let outcome = outcome.trim();
    personas
        .iter()
        .filter(|p| {
            ballots
                .iter()
                .any(|(agent, choice)| *agent == p.name && choice != outcome)
        })
        .map(|p| Persona {
            anchor: (p.anchor + (1.0 - p.anchor) * (1.0 - beta)).min(1.0),
            ..p.clone()
        })
        .collect()
}

/// [`learn_about`] and [`learn_anchors`] together, written to the pack:
/// the rows, then the personas the outcome moved. Returns what was written.
///
/// # Errors
///
/// The pack refusing a row or a persona.
pub fn learn_and_write(
    ballots: &[(String, String)],
    outcome: &str,
    beta: f64,
    about: &[String],
) -> Result<(Vec<Trust>, Vec<Persona>)> {
    let rows = learn_about(ballots, outcome, &trust_from_pack()?, beta, about)?;
    let moved = learn_anchors(&personas_from_pack()?, ballots, outcome, beta);
    // Every row lands before anything is printed, so a closed pipe cannot
    // leave the graph half written.
    for row in &rows {
        write_trust(row, &[])?;
    }
    for p in &moved {
        write_persona(p)?;
    }
    Ok((rows, moved))
}

/// The factor a refuted voter's rows shrink by (Hedge, doi:10.1006/jcss.1997.1504).
pub const LEARN_BETA: f64 = 0.5;

/// The least a row can fall to, so a voter who is right again is heard again.
pub const TRUST_FLOOR: f64 = 0.01;

/// A `trust` atom for one row. `why` are deed accessions it cites.
pub fn trust_atom(row: &Trust, why: &[String], workspace: &str) -> Result<Value> {
    let (from, to) = (row.from.trim(), row.to.trim());
    if from.is_empty() || to.is_empty() {
        bail!("trust: from and to are required");
    }
    if from == to {
        bail!("trust: {from} cannot weigh itself; self weight is the settle's");
    }
    if !(row.weight > 0.0 && row.weight <= 1.0) {
        bail!("trust: weight {} is not in (0, 1]", row.weight);
    }
    let mut atom = atom_body(
        "trust",
        &format!("{from} weighs {to} at {:.3}.", row.weight),
        workspace,
    );
    atom["from"] = Value::String(from.into());
    atom["to"] = Value::String(to.into());
    atom["weight"] = serde_json::json!(row.weight);
    if !why.is_empty() {
        atom["entities"] = Value::Array(why.iter().map(|w| Value::String(w.clone())).collect());
    }
    if !row.about.is_empty() {
        atom["about"] = Value::Array(
            row.about
                .iter()
                .map(|w| Value::String(w.to_lowercase()))
                .collect(),
        );
    }
    Ok(atom)
}

/// The live rows in a set of atoms: the latest `trust` atom per `(from, to)`.
pub fn trust_rows(atoms: &[Value]) -> Vec<Trust> {
    // The latest row per (from, to, scope): an unscoped row and a scoped one
    // for the same pair are different rows, and a later row of the same
    // scope supersedes.
    let mut latest: std::collections::BTreeMap<(String, String, Vec<String>), (String, f64)> =
        std::collections::BTreeMap::new();
    for atom in atoms {
        if atom.get("kind").and_then(Value::as_str) != Some("trust") {
            continue;
        }
        let (Some(from), Some(to), Some(weight)) = (
            atom.get("from").and_then(Value::as_str),
            atom.get("to").and_then(Value::as_str),
            atom.get("weight").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let ts = atom
            .get("ts")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut about = words_of(atom.get("about"));
        about.sort_unstable();
        let key = (from.to_string(), to.to_string(), about);
        match latest.get(&key) {
            Some((seen, _)) if *seen > ts => {}
            _ => {
                latest.insert(key, (ts, weight));
            }
        }
    }
    latest
        .into_iter()
        .map(|((from, to, about), (_, weight))| Trust {
            from,
            to,
            weight,
            about,
        })
        .collect()
}

/// Rows as the consensus takes them: `[[from, to, weight], ...]`.
pub fn trust_json(rows: &[Trust]) -> String {
    let tuples: Vec<Value> = rows
        .iter()
        .map(|r| serde_json::json!([r.from, r.to, r.weight]))
        .collect();
    Value::Array(tuples).to_string()
}

/// `(agent, choice)` pairs from a tracker's `vote --json`.
pub fn ballots_from_json(raw: &str) -> Result<Vec<(String, String)>> {
    let rows: Vec<Value> = serde_json::from_str(raw).context("ballots: not a JSON array")?;
    rows.iter()
        .map(|row| {
            let agent = row.get("agent").and_then(Value::as_str);
            let choice = row.get("choice").and_then(Value::as_str);
            match (agent, choice) {
                (Some(a), Some(c)) => Ok((a.to_string(), c.to_string())),
                _ => bail!("ballots: a row without agent and choice"),
            }
        })
        .collect()
}

/// The rows every voter holds on every other after `outcome` is known: a
/// voter whose ballot was refuted shrinks by `beta`, floored at
/// [`TRUST_FLOOR`]; a missing row starts at one. Complete, so the settle
/// sees the whole graph.
pub fn learn(
    ballots: &[(String, String)],
    outcome: &str,
    rows: &[Trust],
    beta: f64,
) -> Result<Vec<Trust>> {
    learn_about(ballots, outcome, rows, beta, &[])
}

/// [`learn`] writing rows scoped to `about`: the domains the issue's island
/// speaks to, so that being wrong about one topic does not cost a voter its
/// standing on every other. An empty `about` is the unscoped rule.
pub fn learn_about(
    ballots: &[(String, String)],
    outcome: &str,
    rows: &[Trust],
    beta: f64,
    about: &[String],
) -> Result<Vec<Trust>> {
    if !(beta > 0.0 && beta < 1.0) {
        bail!("learn: beta {beta} is not in (0, 1)");
    }
    let outcome = outcome.trim();
    if outcome.is_empty() {
        bail!("learn: an outcome is required");
    }
    let mut agents: Vec<&str> = ballots.iter().map(|(a, _)| a.as_str()).collect();
    agents.sort_unstable();
    agents.dedup();
    if agents.len() < 2 {
        bail!("learn: fewer than two voters, nothing to weigh");
    }
    let refuted = |agent: &str| {
        ballots
            .iter()
            .any(|(a, choice)| a == agent && choice != outcome)
    };
    let mut out = Vec::new();
    for from in &agents {
        for to in &agents {
            if from == to {
                continue;
            }
            // The row being moved is the one of this scope; a scoped learn
            // starts from the unscoped row when it has none of its own.
            let current = rows
                .iter()
                .find(|r| r.from == *from && r.to == *to && r.about == about)
                .or_else(|| {
                    rows.iter()
                        .find(|r| r.from == *from && r.to == *to && r.about.is_empty())
                })
                .map_or(1.0, |r| r.weight);
            let next = if refuted(to) {
                (current * beta).max(TRUST_FLOOR)
            } else {
                current
            };
            out.push(Trust {
                from: (*from).to_string(),
                to: (*to).to_string(),
                weight: next,
                about: about.to_vec(),
            });
        }
    }
    Ok(out)
}

/// The live trust rows in the seat's pack.
pub fn trust_from_pack() -> Result<Vec<Trust>> {
    let client = pack()?;
    let workspace = client.workspace();
    let atoms = client
        .atoms_as_of(&workspace, None)
        .context("trust: GET /v1/atoms failed")?;
    Ok(trust_rows(&atoms))
}

/// POST one trust row.
pub fn write_trust(row: &Trust, why: &[String]) -> Result<Value> {
    let client = pack()?;
    let workspace = client.workspace();
    client
        .post_atom(&trust_atom(row, why, &workspace)?)
        .context("trust: POST /v1/atoms failed")
}

/// One habitat and whether it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Habitat {
    pub name: &'static str,
    pub state: String,
    pub ok: bool,
}

/// The habitats the seat needs.
pub const REQUIRED: &[&str] = &["vissue", "deedar", "packset"];

/// Which habitats answer: binaries on `PATH`, the pack over `PACKSET_URL`, the
/// deed store, the tracker, the claim graph.
pub fn doctor() -> Vec<Habitat> {
    // The runner rows ask the runners' own command lines, which start slowly;
    // they run beside the seat's rows rather than after them.
    let (mut out, runners) = std::thread::scope(|s| {
        let runners = s.spawn(harness_rows);
        let seat = doctor_seat();
        (seat, runners.join().unwrap_or_default())
    });
    out.extend(runners);
    out
}

/// The seat's own rows: binaries, pack, host key, deed store, tracker,
/// claim graph. What a sitting checks; the runner rows are onboarding.
pub fn doctor_seat() -> Vec<Habitat> {
    let mut out = Vec::new();
    for bin in [
        "vissue",
        "deedar",
        "claimdag",
        "packset",
        "packsetd",
        "ljos-consensus",
        "ljos-mcp",
    ] {
        let found = which::which(bin).ok();
        out.push(Habitat {
            name: bin,
            state: found
                .as_ref()
                .map_or_else(|| "not on PATH".to_string(), |p| p.display().to_string()),
            ok: found.is_some(),
        });
    }
    out.push(match PacksetClient::from_env() {
        Ok(client) => match client.health() {
            Ok(_) => Habitat {
                name: "pack",
                state: format!("{} workspace {}", client.base(), client.workspace()),
                ok: true,
            },
            Err(e) => Habitat {
                name: "pack",
                state: format!("{} does not answer: {e}", client.base()),
                ok: false,
            },
        },
        Err(_) => Habitat {
            name: "pack",
            state: "PACKSET_URL=off: no pack on purpose".into(),
            ok: false,
        },
    });
    out.push(match host_key_path() {
        Some(path) => {
            let seed = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) == 32;
            Habitat {
                name: "host key",
                state: if seed {
                    format!("{} (32-byte seed)", path.display())
                } else {
                    format!("{} is not a 32-byte seed", path.display())
                },
                ok: seed,
            }
        }
        None => Habitat {
            name: "host key",
            state: "none at ~/.config/deedar/host.key and DEEDAR_HOST_SIGNING_KEY unset; \
                    handovers go out unsigned"
                .into(),
            ok: false,
        },
    });
    for (name, bin, args) in [
        ("deed store", "deedar", &["log", "head"][..]),
        ("tracker", "vissue", &["identity"][..]),
        ("claim graph", "claimdag", &["list"][..]),
    ] {
        out.push(match run_captured(bin, args) {
            Ok(said) => Habitat {
                name,
                state: said.stdout.lines().next().unwrap_or("").to_string(),
                ok: true,
            },
            Err(e) => Habitat {
                name,
                state: e.to_string().lines().next().unwrap_or("").to_string(),
                ok: false,
            },
        });
    }
    out
}

/// Whether every required habitat answers.
pub fn healthy(rows: &[Habitat]) -> bool {
    rows.iter()
        .all(|h| h.ok || !REQUIRED.contains(&h.name) && h.name != "pack")
}

pub fn format_doctor(rows: &[Habitat]) -> String {
    rows.iter()
        .map(|h| {
            format!(
                "{}	{}	{}
",
                if h.ok { "ok" } else { "no" },
                h.name,
                h.state
            )
        })
        .collect()
}

/// The accessions a satchel's description says it needs.
pub fn needs_of(satchel_json: &str) -> Result<Vec<String>> {
    let v: Value = serde_json::from_str(satchel_json).context("satchel.json")?;
    Ok(v.get("needs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default())
}

/// Deeds to enclose: the satchel's `needs` plus what the pack cites, once each.
pub fn enclose(needs: Vec<String>, cited: &str) -> Vec<String> {
    let mut all: Vec<String> = needs
        .into_iter()
        .chain(cited.lines().map(str::trim).map(str::to_string))
        .filter(|s| !s.is_empty())
        .collect();
    all.sort();
    all.dedup();
    all
}

/// Pack a slice of the seat into `out`: the tracker's satchel, the pack's
/// atoms, the deeds both cite, sealed, and signed when a host key is set.
pub fn handover(out: &Path, projects: &[String], issues: &[String]) -> Result<Vec<String>> {
    if projects.is_empty() && issues.is_empty() {
        bail!("handover: name a project or an issue");
    }
    let mut lines = Vec::new();
    let mut args = vec![
        "satchel".to_string(),
        "--out".into(),
        out.display().to_string(),
    ];
    for p in projects {
        args.push("--project".into());
        args.push(p.clone());
    }
    for i in issues {
        args.push("--issue".into());
        args.push(i.clone());
    }
    lines.push(run_captured("vissue", &args)?.stdout.trim_end().to_string());

    let mut cited = String::new();
    match PacksetClient::from_env() {
        Ok(client) => {
            let atoms_dir = out.join("data").join("atoms");
            match run_captured(
                "packset",
                &[
                    "export",
                    "--into",
                    &atoms_dir.display().to_string(),
                    &client.workspace(),
                ],
            ) {
                Ok(said) => {
                    cited = said.stdout;
                    lines.push(said.stderr.trim_end().to_string());
                }
                Err(e) => lines.push(format!("atoms not enclosed: {e}")),
            }
        }
        Err(_) => lines.push("no pack: PACKSET_URL=off, atoms not enclosed".into()),
    }

    let description = std::fs::read_to_string(out.join("data").join("satchel.json"))
        .context("handover: the satchel has no description")?;
    let deeds = enclose(needs_of(&description)?, &cited);
    if deeds.is_empty() {
        lines.push("no deeds cited".into());
    } else {
        let deeds_dir = out.join("data").join("deeds");
        let said = run_fed(
            "deedar",
            &["export", "--into", &deeds_dir.display().to_string(), "-"],
            &format!(
                "{}
",
                deeds.join(
                    "
"
                )
            ),
        )?;
        lines.push(said.stdout.trim_end().to_string());
    }

    lines.push(
        run_captured("vissue", &["satchel", "--seal", &out.display().to_string()])?
            .stdout
            .trim_end()
            .to_string(),
    );
    if std::env::var_os("DEEDAR_HOST_SIGNING_KEY").is_some() {
        let manifest = out.join("manifest-sha256.txt");
        let said = run_captured(
            "deedar",
            &["vouch", "sign", &manifest.display().to_string()],
        )?;
        lines.push(said.stdout.trim_end().to_string());
    } else {
        lines.push("unsigned: DEEDAR_HOST_SIGNING_KEY unset".into());
    }
    Ok(lines)
}

/// Check a satchel that arrived: manifest, deed receipts, signature, and what
/// the atoms hold; with `import`, POST the atoms into this seat's pack.
pub fn receive(dir: &Path, since: Option<&Path>, import: bool) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    lines.push(
        run_captured(
            "vissue",
            &["satchel", "--verify", &dir.display().to_string()],
        )?
        .stdout
        .trim_end()
        .to_string(),
    );
    if dir.join("data").join("deeds").is_dir() {
        let mut args = vec!["check".to_string(), dir.display().to_string()];
        if let Some(bridge) = since {
            args.push("--since".into());
            args.push(bridge.display().to_string());
        }
        lines.push(run_captured("deedar", &args)?.stdout.trim_end().to_string());
    } else {
        lines.push("no deeds enclosed".into());
    }
    let manifest = dir.join("manifest-sha256.txt");
    if manifest.with_extension("txt.sig").is_file() {
        lines.push(
            run_captured(
                "deedar",
                &["vouch", "check", &manifest.display().to_string()],
            )?
            .stdout
            .trim_end()
            .to_string(),
        );
    } else {
        lines.push("unsigned".into());
    }

    let atoms = enclosed_atoms(dir)?;
    let rows = trust_rows(&atoms);
    lines.push(format!(
        "{} atoms enclosed, {} trust rows",
        atoms.len(),
        rows.len()
    ));
    if import {
        let client = pack()?;
        let workspace = client.workspace();
        let (mut kept, mut refused) = (0usize, Vec::new());
        for atom in &atoms {
            // The atoms arrive stamped with the sender's workspace; they join
            // this seat's, or the import lands in a workspace nobody reads.
            let mut atom = atom.clone();
            if let Some(map) = atom.as_object_mut() {
                map.insert("workspace".into(), Value::String(workspace.clone()));
            }
            match client.post_atom(&atom) {
                Ok(_) => kept += 1,
                Err(e) => refused.push(e.to_string()),
            }
        }
        lines.push(format!("{kept} atoms imported, {} refused", refused.len()));
        lines.extend(refused.into_iter().take(5));
    }
    Ok(lines)
}

/// Every atom in a satchel's `data/atoms/*.jsonl`.
pub fn enclosed_atoms(dir: &Path) -> Result<Vec<Value>> {
    let atoms_dir = dir.join("data").join("atoms");
    let Ok(entries) = std::fs::read_dir(&atoms_dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let text = std::fs::read_to_string(entry.path())?;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push(
                serde_json::from_str(line).with_context(|| entry.path().display().to_string())?,
            );
        }
    }
    Ok(out)
}

/// Kinds that are weighed, not recalled, and so never come up for review.
const UNREVIEWED_KINDS: &[&str] = &["trust", "persona"];

/// Whether an atom is a claim the review clock should hold at all.
fn reviewable(a: &Value) -> bool {
    !UNREVIEWED_KINDS.contains(&a.get("kind").and_then(Value::as_str).unwrap_or(""))
}

/// The live atoms whose review is due at `now` (RFC 3339 UTC), soonest first.
/// A claim that has never entered the review clock has no `due_at`; it is
/// due now, and grading it puts it on the clock. Trust and persona rows are
/// weighed, not recalled, and never come up.
pub fn due_of(atoms: &[Value], now: &str) -> Vec<Value> {
    let mut due: Vec<Value> = atoms
        .iter()
        .filter(|a| reviewable(a))
        .filter(|a| {
            a.get("due_at")
                .and_then(Value::as_str)
                .is_none_or(|d| d.is_empty() || d <= now)
        })
        .cloned()
        .collect();
    due.sort_by(|a, b| {
        a["due_at"]
            .as_str()
            .unwrap_or("")
            .cmp(b["due_at"].as_str().unwrap_or(""))
    });
    due
}

/// One line on the state of the review clock: how many are due, how many
/// are scheduled, and when the next one comes up. An empty `due` with a
/// next date is a clock that is running; an empty `due` with nothing
/// scheduled is a seat that has remembered nothing.
pub fn review_summary(atoms: &[Value], now: &str) -> String {
    let due = due_of(atoms, now).len();
    let mut later: Vec<&str> = atoms
        .iter()
        .filter(|a| reviewable(a))
        .filter_map(|a| a.get("due_at").and_then(Value::as_str))
        .filter(|d| !d.is_empty() && *d > now)
        .collect();
    later.sort_unstable();
    match later.first() {
        Some(next) => format!("{due} due; {} scheduled, next at {next}", later.len()),
        None if due == 0 => "0 due; nothing scheduled: this seat has remembered nothing yet".into(),
        None => format!("{due} due; nothing else scheduled"),
    }
}

/// The review clock as `ljos due` prints it: the due atoms, then the summary.
pub fn due_report() -> Result<String> {
    let client = pack()?;
    let atoms = client
        .atoms_as_of(&client.workspace(), None)
        .context("due: GET /v1/atoms failed")?;
    let now = now_utc();
    Ok(format!(
        "{}{}\n",
        format_due(&due_of(&atoms, &now)),
        review_summary(&atoms, &now)
    ))
}

/// What the pack holds for review now.
pub fn due() -> Result<Vec<Value>> {
    let client = pack()?;
    let atoms = client
        .atoms_as_of(&client.workspace(), None)
        .context("due: GET /v1/atoms failed")?;
    Ok(due_of(&atoms, &now_utc()))
}

pub fn format_due(atoms: &[Value]) -> String {
    atoms
        .iter()
        .map(|a| {
            format!(
                "{}	{}	{}	{}
",
                a["due_at"]
                    .as_str()
                    .filter(|d| !d.is_empty())
                    .unwrap_or("unreviewed"),
                a["kind"].as_str().unwrap_or(""),
                a["id"].as_str().unwrap_or("-"),
                a["text"].as_str().unwrap_or("")
            )
        })
        .collect()
}

/// Grade one review: recalled moves the atom out, lapsed brings it back sooner.
pub fn graded(id: &str, recalled: bool) -> Result<Value> {
    let id = id.trim();
    if id.is_empty() {
        bail!("graded: an atom id is required");
    }
    let client = pack()?;
    client
        .grade(&client.workspace(), id, recalled)
        .with_context(|| format!("graded: POST /v1/grade failed for {id}"))
}

fn now_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    // Civil date from days since the epoch (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Run a habitat's verb with `input` on stdin.
pub fn run_fed(bin: &str, args: &[impl AsRef<str>], input: &str) -> Result<Said> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let path = which::which(bin).with_context(|| format!("{bin} not on PATH"))?;
    let mut cmd = Command::new(path);
    for a in args {
        cmd.arg(a.as_ref());
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("{bin}: could not start"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        let why = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        bail!("{bin} exited {}: {why}", out.status);
    }
    Ok(Said { stdout, stderr })
}

/// A claimdag id for a name: the name itself when it is already 32 hex, else
/// FNV-1a 128 of it. One tracker id maps to one node; one assignee to one actor.
pub fn work_id(name: &str) -> String {
    let name = name.trim();
    if name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
        return name.to_ascii_lowercase();
    }
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut h = OFFSET;
    for b in name.bytes() {
        h ^= u128::from(b);
        h = h.wrapping_mul(PRIME);
    }
    format!("{h:032x}")
}

/// The claimdag node standing for `issue`, minted with the tracker id as its
/// summary when the graph does not hold it yet.
pub fn node_for(issue: &str) -> Result<String> {
    let id = work_id(issue);
    if id != issue.trim() && run_captured("claimdag", &["get", &id]).is_err() {
        run_captured(
            "claimdag",
            &["upsert", "--id", &id, "--summary", issue.trim()],
        )
        .with_context(|| format!("claim: could not mint a node for {issue}"))?;
    }
    Ok(id)
}

/// The memories a task activates: the pack's island around the cue. With
/// `fire`, the strongest of them fire together and their links gain weight.
pub fn packset_island(cue: &str, fire: bool) -> Result<Value> {
    let cue = cue.trim();
    if cue.is_empty() {
        bail!("island: pass the task or question at hand");
    }
    let client = pack()?;
    let workspace = client.workspace();
    client
        .activate(&workspace, cue, 24, fire)
        .context("island: GET /v1/activate failed")
}

/// One line per activated memory: activation, seed mark, id, text.
pub fn format_island(body: &Value) -> String {
    let mut out = String::new();
    for atom in body["island"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| reviewable(a))
    {
        out.push_str(&format!(
            "{:.3}\t{}\t{}\t{}\n",
            atom["activation"].as_f64().unwrap_or(0.0),
            if atom["seed"].as_bool().unwrap_or(false) {
                "seed"
            } else {
                "    "
            },
            atom["id"].as_str().unwrap_or("-"),
            atom["text"].as_str().unwrap_or("")
        ));
    }
    out
}

pub fn packset_search(query: &str) -> Result<Vec<Hit>> {
    let q = query.trim();
    if q.is_empty() {
        bail!("search: empty query");
    }
    let client = pack()?;
    let workspace = client.workspace();
    client
        .search(&workspace, q, 10)
        .context("search: GET /v1/search failed")
}

/// Take a session node, and when the claim graph refuses because the
/// assignee still holds another node, say which tracker id that is and the
/// two verbs that free it. The bare refusal names a 32-hex id nobody can
/// act on.
///
/// # Errors
///
/// The refusal, explained, or any other failure of the claim graph.
pub fn claim(node: &str, assignee: &str) -> Result<String> {
    let id = node_for(node)?;
    let actor = work_id(assignee);
    match run_captured("claimdag", &["claim", &id, "--assignee", &actor]) {
        Ok(said) => Ok(said.stdout),
        Err(e) => {
            let text = e.to_string();
            // A tracker id maps to one node. When an earlier sitting finished
            // it, this is a new sitting on the same work: reopen, then claim.
            if ["status done", "status failed", "status cancelled"]
                .iter()
                .any(|s| text.contains(s))
            {
                run_captured("claimdag", &["reopen", &id, "--actor", &actor])?;
                let said = run_captured("claimdag", &["claim", &id, "--assignee", &actor])?;
                return Ok(format!("reopened a finished session node\n{}", said.stdout));
            }
            if !text.contains("assignee busy") {
                return Err(e);
            }
            let held: Vec<String> = text
                .split_whitespace()
                .filter(|w| w.len() == 32 && w.chars().all(|c| c.is_ascii_hexdigit()))
                .map(str::to_string)
                .collect();
            let mut lines = vec![format!(
                "claim: {assignee} already holds a live node; one live claim per assignee."
            )];
            for hex in &held {
                let name = run_captured("claimdag", &["get", hex])
                    .ok()
                    .and_then(|s| {
                        s.stdout
                            .lines()
                            .next()
                            .and_then(|l| l.split_whitespace().last())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| hex.clone());
                lines.push(format!(
                    "  holds {name}: `ljos complete {name} --status done` finishes it, \
                     `ljos release {name} --assignee {assignee}` hands it back"
                ));
            }
            bail!("{}", lines.join("\n"))
        }
    }
}

/// Hand a session node back before it is terminal: ready again, assignee
/// cleared, generation moved.
///
/// # Errors
///
/// The claim graph's refusal: not held, or held by somebody else.
pub fn release(node: &str, assignee: &str) -> Result<String> {
    let id = node_for(node)?;
    Ok(run_captured("claimdag", &["release", &id, "--actor", &work_id(assignee)])?.stdout)
}

/// The issue's title, for a cue, from the tracker.
fn issue_title(issue: &str) -> Result<String> {
    let said = run_captured("vissue", &["show", issue, "--json"])?;
    let v: Value = serde_json::from_str(&said.stdout).context("vissue show --json")?;
    Ok(v.get("title")
        .and_then(Value::as_str)
        .unwrap_or(issue)
        .to_string())
}

/// Open a sitting on an issue, in the protocol's order, and stop at the
/// first habitat that does not answer: doctor, cards, the review clock,
/// the island the issue's title activates, the working set, the claim.
/// One verb, so the loop that makes the seat a memory runs every time and
/// not only when somebody remembers to run it.
///
/// # Errors
///
/// A required habitat down, or the claim refused (the refusal names what
/// the assignee still holds).
pub fn sitting(issue: &str, assignee: &str, cards_dir: &Path) -> Result<String> {
    let mut out = String::new();
    let rows = doctor_seat();
    out.push_str("== doctor\n");
    out.push_str(&format_doctor(&rows));
    if !healthy(&rows) {
        bail!("{out}sitting: a required habitat does not answer; nothing was claimed");
    }
    out.push_str("== cards\n");
    out.push_str(&cards(cards_dir)?);
    out.push_str("== due\n");
    out.push_str(&due_report()?);
    let title = issue_title(issue)?;
    out.push_str(&format!("== island: {title}\n"));
    out.push_str(&format_island(&packset_island(&title, false)?));
    out.push_str("== recall\n");
    out.push_str(&run_captured("vissue", &["recall", issue])?.stdout);
    out.push_str("== claim\n");
    out.push_str(&claim(issue, assignee)?);
    Ok(out)
}

/// Close a sitting: remember the lesson when there is one, fire the island
/// the issue's title activates, complete the session node, and learn from
/// the outcome when one is named. Without a lesson the report says so,
/// because a sitting that taught nothing worth two sentences is rare and
/// worth noticing.
///
/// # Errors
///
/// Any habitat refusing; the pack refuses a lesson longer than two
/// sentences, the claim graph a status that is not terminal.
pub fn finish(
    issue: &str,
    status: &str,
    lesson: Option<&str>,
    outcome: Option<&str>,
    beta: f64,
) -> Result<String> {
    let mut out = String::new();
    match lesson.map(str::trim).filter(|l| !l.is_empty()) {
        Some(text) => {
            let body = packset_write("Remember", text)?;
            out.push_str(&format!(
                "remembered {}\n",
                body.get("id").and_then(Value::as_str).unwrap_or("-")
            ));
        }
        None => out.push_str(
            "no lesson remembered this sitting; `ljos remember` takes one in two sentences\n",
        ),
    }
    let title = issue_title(issue)?;
    let island = packset_island(&title, true)?;
    let fired = island["island"].as_array().map_or(0, Vec::len);
    out.push_str(&format!(
        "fired the island for {title:?}: {fired} memories\n"
    ));
    let terminal = ["done", "failed", "cancelled"];
    if !terminal.contains(&status) {
        bail!("finish: status {status:?} is not one of done, failed, cancelled");
    }
    run_captured(
        "claimdag",
        &["complete", &node_for(issue)?, "--status", status],
    )?;
    out.push_str(&format!(
        "completed the session node for {issue} as {status}\n"
    ));
    if let Some(option) = outcome.map(str::trim).filter(|o| !o.is_empty()) {
        let said = run_captured("vissue", &["vote", issue, "--json"])?;
        let ballots = ballots_from_json(&said.stdout)?;
        if ballots.len() < 2 {
            out.push_str("outcome named but fewer than two ballots; nothing to learn from\n");
        } else {
            let about = island_entities(issue).unwrap_or_default();
            let (rows, moved) = learn_and_write(&ballots, option, beta, &about)?;
            out.push_str(&format!(
                "learned from outcome {option:?}: {} trust rows rewritten, {} persona anchors moved\n",
                rows.len(),
                moved.len()
            ));
        }
    }
    out.push_str(&format!(
        "the ticket stays {issue}'s state; `vissue update {issue} -s DONE` closes it\n"
    ));
    Ok(out)
}

/// Turn a project's voting history into trust rows without anyone naming
/// an outcome: Dawid and Skene's accuracy per voter
/// (doi:10.2307/2346806), from `ljos-consensus reliability`, written as the
/// weight every other voter gives that voter. That is the weight a linear
/// opinion pool gives a source believed that reliable (Genest and Zidek,
/// doi:10.1214/ss/1177013825). Rows are complete and floored at
/// [`TRUST_FLOOR`], so the settle sees the whole graph.
///
/// # Errors
///
/// No issue with two or more ballots, the consensus binary absent, or the
/// pack refusing a row.
pub fn calibrate(project: &str, rounds: usize) -> Result<Vec<Trust>> {
    let said = run_captured(
        "ljos-consensus",
        &[
            "reliability",
            "--project",
            project,
            "--rounds",
            &rounds.to_string(),
        ],
    )?;
    let v: Value = serde_json::from_str(&said.stdout).context("reliability: not JSON")?;
    let accuracy = v
        .get("accuracy")
        .and_then(Value::as_object)
        .context("reliability: no accuracy object")?;
    let mut voters: Vec<(&str, f64)> = accuracy
        .iter()
        .filter_map(|(k, val)| val.as_f64().map(|a| (k.as_str(), a)))
        .collect();
    voters.sort_by(|a, b| a.0.cmp(b.0));
    if voters.len() < 2 {
        bail!("calibrate: fewer than two voters in {project}");
    }
    let mut rows = Vec::new();
    for (from, _) in &voters {
        for (to, acc) in &voters {
            if from == to {
                continue;
            }
            rows.push(Trust {
                from: (*from).to_string(),
                to: (*to).to_string(),
                weight: acc.clamp(TRUST_FLOOR, 1.0),
                about: Vec::new(),
            });
        }
    }
    for row in &rows {
        write_trust(row, &[])?;
    }
    Ok(rows)
}

pub fn format_hits(hits: &[Hit]) -> String {
    let mut out = String::new();
    for h in hits {
        let id = h.id.as_deref().unwrap_or("-");
        out.push_str(&format!("{:.4}\t{}\t{}\t{}\n", h.score, h.kind, id, h.text));
    }
    out
}

/// Read-only cards. Only [`CARD_NAMES`], never created, never written.
pub fn cards(dir: &Path) -> Result<String> {
    let mut out = String::new();
    for name in CARD_NAMES {
        let p = dir.join(name);
        if p.is_file() {
            out.push_str(&format!("--- {} ---\n", p.display()));
            out.push_str(&std::fs::read_to_string(&p)?);
        }
    }
    Ok(out)
}

pub fn policy_line(argv: &[String]) -> Result<String> {
    if argv.is_empty() {
        bail!("policy: pass the argv to check");
    }
    Ok(argv.join(" "))
}

/// The argv line, then what the pack knows that bears on it: the memory a
/// policy layer injects beside its verdict. The line prints even when the
/// pack is down; the memory is the part that may be empty.
pub fn policy_with_memory(argv: &[String]) -> Result<String> {
    let line = policy_line(argv)?;
    let call = HookCall {
        event: "argv".into(),
        cue: line.clone(),
        session: None,
    };
    let context = hook_context(&call, 5);
    Ok(if context.is_empty() {
        format!("{line}\n")
    } else {
        format!("{line}\n{context}\n")
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsensusStep {
    pub bin: &'static str,
    pub args: Vec<String>,
}

/// `ljos-consensus` first, then `vissue consensus`, both under the pack's
/// trust rows when there are any. Missing bins are skipped.
pub fn consensus_steps(
    id: &str,
    have_ljos: bool,
    have_vissue: bool,
    trust: &[Trust],
) -> Result<Vec<ConsensusStep>> {
    consensus_steps_anchored(id, have_ljos, have_vissue, trust, &[])
}

/// The tag on an issue that asks for bounded confidence: a panel for a
/// broad audience is allowed to settle into clusters, and the settle says
/// how far apart they are, where a single-position model would average
/// them away. Without it the anchored model runs.
pub const BROAD_TAG: &str = "broad";

/// The confidence bound a `broad` issue settles under: voters within this
/// L1 distance of each other's opinion listen to each other.
pub const BROAD_EPSILON: f64 = 1.0;

/// The model flags an issue's tags ask for, beside the rows and anchors.
/// The kind of work sets the dynamics: `broad` runs bounded confidence.
#[must_use]
pub fn settle_flags_for(tags: &[String]) -> Vec<String> {
    if tags.iter().any(|t| t == BROAD_TAG) {
        vec!["--epsilon".into(), BROAD_EPSILON.to_string()]
    } else {
        Vec::new()
    }
}

/// [`consensus_steps_anchored`] with the model flags the issue's tags ask
/// for on the model crate's settle.
pub fn consensus_steps_for(
    id: &str,
    have_ljos: bool,
    have_vissue: bool,
    trust: &[Trust],
    personas: &[Persona],
    tags: &[String],
) -> Result<Vec<ConsensusStep>> {
    let mut steps = consensus_steps_anchored(id, have_ljos, have_vissue, trust, personas)?;
    let flags = settle_flags_for(tags);
    if !flags.is_empty() {
        for step in steps.iter_mut().filter(|s| s.bin == "ljos-consensus") {
            step.args.extend(flags.iter().cloned());
        }
    }
    Ok(steps)
}

/// [`consensus_steps`] passing the personas' anchors to both settles as
/// `--susceptibility-of`, so a persona holds its ballot as much as it says.
pub fn consensus_steps_anchored(
    id: &str,
    have_ljos: bool,
    have_vissue: bool,
    trust: &[Trust],
    personas: &[Persona],
) -> Result<Vec<ConsensusStep>> {
    if !have_ljos && !have_vissue {
        bail!("neither ljos-consensus nor vissue is on PATH");
    }
    let mut steps = Vec::new();
    if have_ljos {
        let mut args = vec!["settle".to_string(), "--issue".into(), id.into()];
        if !trust.is_empty() {
            args.push("--trust".into());
            args.push(trust_json(trust));
        }
        if !personas.is_empty() {
            args.push("--susceptibility-of".into());
            args.push(anchors_json(personas));
        }
        steps.push(ConsensusStep {
            bin: "ljos-consensus",
            args,
        });
    }
    if have_vissue {
        let mut args = vec!["consensus".to_string(), id.into()];
        if !trust.is_empty() {
            args.push("--trust".into());
            args.push(trust_json(trust));
        }
        if !personas.is_empty() {
            args.push("--susceptibility-of".into());
            args.push(anchors_json(personas));
        }
        steps.push(ConsensusStep {
            bin: "vissue",
            args,
        });
    }
    Ok(steps)
}

pub fn on_path(bin: &str) -> bool {
    which::which(bin).is_ok()
}

pub fn run(bin: &str, args: &[impl AsRef<str>]) -> Result<()> {
    run_as(bin, args, None)
}

/// [`run`] with `VISSUE_AGENT` set to `identity`, so a ballot or a claim is
/// recorded under a persona's name rather than the seat's.
pub fn run_as(bin: &str, args: &[impl AsRef<str>], identity: Option<&str>) -> Result<()> {
    use std::process::{Command, Stdio};
    let path = which::which(bin).with_context(|| format!("{bin} not on PATH"))?;
    let mut cmd = Command::new(path);
    if let Some(who) = identity.map(str::trim).filter(|w| !w.is_empty()) {
        cmd.env("VISSUE_AGENT", who);
    }
    for a in args {
        cmd.arg(a.as_ref());
    }
    let st = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !st.success() {
        bail!("{bin} exited {st}");
    }
    Ok(())
}

/// What a habitat printed, kept for a caller that has to hand it on. A
/// non-zero exit is an error carrying stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub stdout: String,
    pub stderr: String,
}

pub fn run_captured(bin: &str, args: &[impl AsRef<str>]) -> Result<Said> {
    use std::process::{Command, Stdio};
    let path = which::which(bin).with_context(|| format!("{bin} not on PATH"))?;
    let mut cmd = Command::new(path);
    for a in args {
        cmd.arg(a.as_ref());
    }
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("{bin}: could not start"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        let why = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        bail!("{bin} exited {}: {why}", out.status);
    }
    Ok(Said { stdout, stderr })
}

pub fn card_paths(dir: &Path) -> Vec<PathBuf> {
    CARD_NAMES.iter().map(|n| dir.join(n)).collect()
}

#[cfg(test)]
mod tests {
    /// A hook call is read from the runner's JSON or from plain text, and
    /// the answer is the runner's shape only when there is something to say.
    #[test]
    fn hook_calls_are_read_and_answered_in_the_runners_shape() {
        let tool = hook_call(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"cargo test","description":"run"}}"#,
        );
        assert_eq!(tool.event, "PreToolUse");
        assert_eq!(tool.cue, "cargo test");
        let prompt = hook_call(r#"{"hook_event_name":"UserPromptSubmit","prompt":"fix the fuse"}"#);
        assert_eq!(prompt.cue, "fix the fuse");
        let argv = hook_call("rm -rf build");
        assert_eq!(argv.event, "argv");
        assert_eq!(argv.session, None);
        let with_session = hook_call(
            r#"{"session_id":"abc/../x 1","hook_event_name":"PreToolUse","tool_input":{"command":"ls"}}"#,
        );
        assert_eq!(with_session.session.as_deref(), Some("abc/../x 1"));
        assert!(seen_path("abc/../x 1")
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("hook-seen-abcx1"));
        assert_eq!(seen_path("/../"), None);
        assert_eq!(hook_output(&argv, ""), "");
        assert_eq!(hook_output(&argv, "- [lesson] x"), "- [lesson] x\n");
        let out = hook_output(&tool, "- [preference] y");
        let v: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(
            v["hookSpecificOutput"]["additionalContext"],
            "- [preference] y"
        );
        assert!(
            hook_context(
                &HookCall {
                    event: "argv".into(),
                    cue: "ab".into(),
                    session: None
                },
                8
            )
            .is_empty(),
            "a cue too short asks nothing"
        );
    }

    /// The memory hook merges into a runner's hooks file once per event and
    /// is not added twice.
    #[test]
    fn the_memory_hook_is_merged_once() {
        let dir = std::env::temp_dir().join(format!("ljos-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(
            &file,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"other"}]}]},"theme":"dark"}"#,
        )
        .unwrap();
        let both: Vec<String> = vec!["UserPromptSubmit".into(), "PreToolUse".into()];
        let prompts: Vec<String> = HOOK_EVENTS.iter().map(|e| (*e).to_string()).collect();
        assert_eq!(prompts, ["UserPromptSubmit"], "the panel's default");
        assert!(!hook_installed(&file, &both));
        let dry = hook_step(&file, &both, true);
        assert!(
            dry.ok && dry.detail.starts_with("would add it on"),
            "{dry:?}"
        );
        let step = hook_step(&file, &both, false);
        assert!(step.ok, "{step:?}");
        assert!(hook_installed(&file, &both));
        let again = hook_step(&file, &both, false);
        assert!(
            again.detail.contains("carries the memory hook on"),
            "{again:?}"
        );
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark", "the rest of the file is kept");
        assert_eq!(
            v["hooks"]["PreToolUse"].as_array().unwrap().len(),
            2,
            "the other hook stays"
        );
        assert_eq!(v["hooks"]["UserPromptSubmit"].as_array().unwrap().len(), 1);
        // Narrowing to the default drops the seat's tool-call group and
        // leaves the other tool's group alone.
        let narrowed = hook_step(&file, &prompts, false);
        assert!(
            narrowed.detail.contains("drop it from PreToolUse"),
            "{narrowed:?}"
        );
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "other");
        assert!(hook_installed(&file, &prompts));
        assert!(!hook_installed(&file, &both));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scoped row applies when the issue is about one of its domains; an
    /// unscoped row applies everywhere; a scoped learn starts from the
    /// unscoped row and leaves it standing.
    #[test]
    fn scoped_rows_apply_to_their_topic_and_learn_writes_in_scope() {
        let everywhere = row("a", "b", 0.9);
        let mut on_docs = row("a", "b", 0.2);
        on_docs.about = vec!["docs".into()];
        let rows = vec![everywhere.clone(), on_docs.clone()];
        let topic = topic_words("Rewrite the docs site");
        assert_eq!(topic, ["docs", "rewrite", "site", "the"]);
        // On the docs topic the scoped row stands in for the unscoped one;
        // elsewhere the unscoped row is the one that applies.
        assert_eq!(rows_about(&rows, &topic), vec![on_docs.clone()]);
        assert_eq!(
            rows_about(&rows, &topic_words("Fix the fuse")),
            vec![everywhere.clone()]
        );

        let ballots = vec![
            ("a".to_string(), "ship".to_string()),
            ("b".to_string(), "hold".to_string()),
        ];
        let learned = learn_about(&ballots, "ship", &rows, 0.5, &["fuse".to_string()]).unwrap();
        let ab = learned
            .iter()
            .find(|r| r.from == "a" && r.to == "b")
            .unwrap();
        assert_eq!(ab.about, ["fuse"]);
        assert!(
            (ab.weight - 0.45).abs() < 1e-9,
            "starts from the unscoped 0.9: {ab:?}"
        );
        let ba = learned
            .iter()
            .find(|r| r.from == "b" && r.to == "a")
            .unwrap();
        assert!((ba.weight - 1.0).abs() < 1e-9, "a was right: {ba:?}");

        // Rows read back keep scoped and unscoped apart, latest per scope.
        let atoms = vec![
            trust_atom(&everywhere, &[], "ws").unwrap(),
            trust_atom(&on_docs, &[], "ws").unwrap(),
        ];
        let mut back = trust_rows(&atoms);
        back.sort_by(|x, y| x.about.cmp(&y.about));
        assert_eq!(back, vec![everywhere, on_docs]);
    }

    /// A persona is a voter with an anchor; the latest atom per name wins and
    /// the anchors go to the settle as one object.
    #[test]
    fn personas_are_latest_per_name_and_anchor_the_settle() {
        let p = Persona {
            name: "reviewer".into(),
            anchor: 0.2,
            view: "Reads for what could break in production.".into(),
            entities: vec!["Release".into()],
        };
        let mut a = persona_atom(&p, "ws").unwrap();
        a["ts"] = Value::String("2026-01-01T00:00:00Z".into());
        let mut later = a.clone();
        later["anchor"] = serde_json::json!(0.4);
        later["ts"] = Value::String("2026-02-01T00:00:00Z".into());
        let got = personas_of(&[a, later]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].anchor, 0.4);
        assert_eq!(got[0].entities, ["release"]);
        assert_eq!(anchors_json(&got), r#"{"reviewer":0.4}"#);
        // A refuted persona listens more next time; a vindicated one does
        // not move; one that did not vote is untouched.
        let ballots = vec![
            ("reviewer".to_string(), "hold".to_string()),
            ("reader".to_string(), "ship".to_string()),
        ];
        let moved = learn_anchors(&got, &ballots, "ship", 0.5);
        assert_eq!(moved.len(), 1);
        assert!(
            (moved[0].anchor - 0.7).abs() < 1e-9,
            "0.4 + 0.6 * 0.5: {moved:?}"
        );
        assert!(learn_anchors(&got, &ballots, "hold", 0.5).is_empty());
        assert!(persona_atom(
            &Persona {
                anchor: 1.5,
                ..p.clone()
            },
            "ws"
        )
        .is_err());
        let steps = consensus_steps_anchored("x-1", true, true, &[], &got).unwrap();
        for step in &steps {
            assert!(
                step.args.contains(&"--susceptibility-of".to_string()),
                "{step:?}"
            );
        }
        // The kind of work sets the dynamics: a broad-audience issue runs
        // bounded confidence on the model crate, and the tracker verb, which
        // has no such model, is left as it was.
        let broad =
            consensus_steps_for("x-1", true, true, &[], &got, &["broad".to_string()]).unwrap();
        assert!(
            broad[0].args.contains(&"--epsilon".to_string()),
            "{:?}",
            broad[0]
        );
        assert!(
            !broad[1].args.contains(&"--epsilon".to_string()),
            "{:?}",
            broad[1]
        );
        assert!(settle_flags_for(&["feature".to_string()]).is_empty());
    }

    /// A claim that never entered the clock is due now; a scheduled one is
    /// not; trust rows never are; and the summary says whether the clock runs.
    #[test]
    fn unreviewed_claims_are_due_and_the_summary_says_if_the_clock_runs() {
        let atoms = vec![
            serde_json::json!({"id": "a", "kind": "conclusion", "text": "old", "due_at": ""}),
            serde_json::json!({"id": "b", "kind": "conclusion", "text": "older"}),
            serde_json::json!({"id": "c", "kind": "conclusion", "text": "later",
                "due_at": "2030-01-01T00:00:00Z"}),
            serde_json::json!({"id": "d", "kind": "conclusion", "text": "past",
                "due_at": "2020-01-01T00:00:00Z"}),
            serde_json::json!({"id": "t", "kind": "trust", "text": "x weighs y"}),
        ];
        let now = "2026-01-01T00:00:00Z";
        let due: Vec<String> = super::due_of(&atoms, now)
            .iter()
            .map(|a| a["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            due,
            ["a", "b", "d"],
            "unreviewed first, then the past-due one"
        );
        assert_eq!(
            super::review_summary(&atoms, now),
            "3 due; 1 scheduled, next at 2030-01-01T00:00:00Z"
        );
        assert_eq!(
            super::review_summary(&[atoms[4].clone()], now),
            "0 due; nothing scheduled: this seat has remembered nothing yet"
        );
        assert!(super::format_due(&super::due_of(&atoms, now)).starts_with("unreviewed\t"));
    }

    /// The example file parses, and onboarding a config-file runner from it
    /// appends the entry once and writes the skill once; a dry run writes
    /// nothing; an unnamed runner is refused with the names the file holds.
    #[test]
    fn onboarding_a_config_file_runner_writes_once() {
        let all: super::Harnesses = toml::from_str(super::HARNESSES_EXAMPLE).expect("parses");
        assert_eq!(all.harness.len(), 2);
        assert_eq!(all.harness[1].marker.as_deref(), Some("[mcp_servers.ljos]"));

        let dir = std::env::temp_dir().join(format!("ljos-onboard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tempdir");
        let config = dir.join("config.toml");
        let skills = dir.join("skills");
        let file = dir.join("harnesses.toml");
        std::fs::write(
            &file,
            format!(
                "[[harness]]\nname = \"r\"\nconfig = {config:?}\nmarker = \"[mcp_servers.ljos]\"\n\
                 snippet = \"\\n[mcp_servers.ljos]\\ncommand = \\\"{{server}}\\\"\\n\"\nskills = {skills:?}\n",
                config = config.display().to_string(),
                skills = skills.display().to_string(),
            ),
        )
        .expect("write");

        let refused = super::onboard_from(&file, "nobody", true)
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("no runner \"nobody\"") && refused.contains("names r"),
            "{refused}"
        );

        let steps = match super::onboard_from(&file, "r", true) {
            Ok(steps) => steps,
            // Without ljos-mcp on PATH there is nothing to register; the
            // refusal says so and the rest of the check needs the binary.
            Err(e) => {
                assert!(e.to_string().contains("ljos-mcp not on PATH"), "{e}");
                return;
            }
        };
        assert!(steps.iter().all(|s| s.ok), "{steps:?}");
        assert!(
            steps[0].detail.starts_with("would append"),
            "{}",
            steps[0].detail
        );
        assert!(!config.exists() && !skills.exists(), "a dry run wrote");

        let steps = super::onboard_from(&file, "r", false).expect("onboards");
        assert!(steps.iter().all(|s| s.ok), "{steps:?}");
        let written = std::fs::read_to_string(&config).expect("config written");
        assert_eq!(written.matches("[mcp_servers.ljos]").count(), 1);
        assert!(written.contains("ljos-mcp"), "{written}");
        let skill = std::fs::read_to_string(skills.join("ljos/SKILL.md")).expect("skill written");
        assert!(skill.starts_with("---\nname: ljos\n"));
        assert!(skill.contains("## Before the work"));

        let again = super::onboard_from(&file, "r", false).expect("onboards again");
        assert_eq!(again[0].detail, "ljos registered");
        assert!(
            again[1].detail.ends_with("is current"),
            "{}",
            again[1].detail
        );
        assert_eq!(
            std::fs::read_to_string(&config)
                .expect("config")
                .matches("[mcp_servers.ljos]")
                .count(),
            1,
            "the entry was appended twice"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A non-zero exit is an error carrying what was said on stderr.
    #[test]
    fn a_refusal_is_an_error_not_an_answer() {
        let err = run_captured("false", &[] as &[&str]).unwrap_err();
        assert!(err.to_string().contains("false exited"), "{err}");
        let said = run_captured("sh", &["-c", "echo answered; echo aside >&2"]).unwrap();
        assert_eq!(said.stdout.trim(), "answered");
        assert_eq!(said.stderr.trim(), "aside");
        let said = run_captured("sh", &["-c", "echo reason >&2; exit 3"]).unwrap_err();
        assert!(said.to_string().contains("reason"), "{said}");
    }

    #[test]
    fn join_keeps_spaces() {
        assert_eq!(
            join(&["the default fuse".into(), "is CombMNZ".into()]),
            "the default fuse is CombMNZ"
        );
    }

    #[test]
    fn remember_is_lesson_prefer_is_preference() {
        assert_eq!(atom_kind("Remember").unwrap(), "lesson");
        assert_eq!(atom_kind("Prefer").unwrap(), "preference");
        assert!(atom_kind("extract").is_err());
    }

    #[test]
    fn atom_body_is_explicit_and_unextracted() {
        let v = atom_body("lesson", "the default fuse is CombMNZ", "ws");
        assert_eq!(v["schema"], "inside.atom/v1");
        assert_eq!(v["kind"], "lesson");
        assert_eq!(v["level"], "explicit");
        assert_eq!(v["text"], "the default fuse is CombMNZ");
        assert_eq!(v["workspace"], "ws");
        // Never harvest a transcript: the text is the claim, not a prefix parse.
        let raw = atom_body("lesson", "Remember: pin the review set", "ws");
        assert_eq!(raw["text"], "Remember: pin the review set");
    }

    #[test]
    fn empty_claim_is_refused() {
        let client = PacksetClient::new("http://127.0.0.1:1");
        let err = post_claim(&client, "Remember", "   ", "ws").unwrap_err();
        assert!(err.to_string().contains("empty text"));
    }

    #[test]
    fn cards_are_the_two_named_files_only() {
        assert_eq!(CARD_NAMES, &["USER.md", "MEMORY.md"]);
        let dir = std::env::temp_dir().join(format!("ljos-cards-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("USER.md"), "user card\n").unwrap();
        std::fs::write(dir.join("MEMORY.md"), "memory card\n").unwrap();
        std::fs::write(dir.join("NOTES.md"), "must not appear\n").unwrap();
        let out = cards(&dir).unwrap();
        assert!(out.contains("user card"));
        assert!(out.contains("memory card"));
        assert!(!out.contains("must not appear"));
        assert!(!out.contains("NOTES.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn policy_prints_argv_and_does_not_reload() {
        assert!(policy_line(&[]).is_err());
        assert_eq!(policy_line(&["ls".into(), "-la".into()]).unwrap(), "ls -la");
        let note = POLICY_TCB.to_ascii_lowercase();
        assert!(note.contains("grok-policyd"));
        assert!(note.contains("not a check"));
        assert!(!note.contains("grokos policy reload"));
        assert!(!note.contains("policy reload"));
    }

    #[test]
    fn consensus_is_ljos_then_vissue() {
        let steps = consensus_steps("vissue-1a5a", true, true, &[]).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].bin, "ljos-consensus");
        assert_eq!(steps[0].args, vec!["settle", "--issue", "vissue-1a5a"]);
        assert_eq!(steps[1].bin, "vissue");
        assert_eq!(steps[1].args, vec!["consensus", "vissue-1a5a"]);
    }

    #[test]
    fn consensus_carries_the_packs_trust() {
        let rows = vec![row("a", "b", 0.5)];
        let steps = consensus_steps("id", true, true, &rows).unwrap();
        assert_eq!(steps[0].args[3], "--trust");
        assert_eq!(steps[0].args[4], r#"[["a","b",0.5]]"#);
        assert_eq!(
            steps[1].args,
            vec!["consensus", "id", "--trust", r#"[["a","b",0.5]]"#]
        );
    }

    #[test]
    fn consensus_skips_a_missing_bin() {
        let only_v = consensus_steps("id", false, true, &[]).unwrap();
        assert_eq!(only_v.len(), 1);
        assert_eq!(only_v[0].bin, "vissue");
        let only_l = consensus_steps("id", true, false, &[]).unwrap();
        assert_eq!(only_l[0].bin, "ljos-consensus");
        assert!(consensus_steps("id", false, false, &[]).is_err());
    }

    fn row(from: &str, to: &str, weight: f64) -> Trust {
        Trust {
            about: Vec::new(),
            from: from.into(),
            to: to.into(),
            weight,
        }
    }

    #[test]
    fn a_trust_atom_is_one_edge_with_its_evidence() {
        let atom = trust_atom(&row("a", "b", 0.25), &["deed-x-y".into()], "ws").unwrap();
        assert_eq!(atom["kind"], "trust");
        assert_eq!(atom["from"], "a");
        assert_eq!(atom["to"], "b");
        assert_eq!(atom["weight"], 0.25);
        assert_eq!(atom["entities"], serde_json::json!(["deed-x-y"]));
        assert_eq!(atom["text"], "a weighs b at 0.250.");
        assert!(trust_atom(&row("a", "a", 0.5), &[], "ws").is_err());
        assert!(trust_atom(&row("a", "b", 0.0), &[], "ws").is_err());
        assert!(trust_atom(&row("a", "b", 1.5), &[], "ws").is_err());
        assert!(trust_atom(&row("", "b", 0.5), &[], "ws").is_err());
    }

    #[test]
    fn the_latest_row_per_pair_wins() {
        let atoms = vec![
            serde_json::json!({"kind": "trust", "from": "a", "to": "b", "weight": 0.9, "ts": "2026-01-01T00:00:00Z"}),
            serde_json::json!({"kind": "trust", "from": "a", "to": "b", "weight": 0.3, "ts": "2026-02-01T00:00:00Z"}),
            serde_json::json!({"kind": "trust", "from": "b", "to": "a", "weight": 0.7}),
            serde_json::json!({"kind": "lesson", "text": "not a row"}),
            serde_json::json!({"kind": "trust", "from": "b", "weight": 0.7}),
        ];
        let rows = trust_rows(&atoms);
        assert_eq!(rows, vec![row("a", "b", 0.3), row("b", "a", 0.7)]);
        assert_eq!(trust_json(&rows), r#"[["a","b",0.3],["b","a",0.7]]"#);
    }

    #[test]
    fn ballots_are_agent_and_choice() {
        let rows =
            ballots_from_json(r#"[{"agent":"a","choice":"ship","stamp":"[2026-01-01]"}]"#).unwrap();
        assert_eq!(rows, vec![("a".to_string(), "ship".to_string())]);
        assert!(ballots_from_json(r#"[{"agent":"a"}]"#).is_err());
        assert!(ballots_from_json("{}").is_err());
    }

    /// A refuted voter loses weight in every other voter's row; a vindicated
    /// one keeps it; the rows come back complete.
    #[test]
    fn learning_downweights_the_refuted_voter() {
        let ballots = vec![
            ("a".to_string(), "ship".to_string()),
            ("b".to_string(), "ship".to_string()),
            ("c".to_string(), "hold".to_string()),
        ];
        let rows = learn(&ballots, "ship", &[], 0.5).unwrap();
        assert_eq!(rows.len(), 6);
        let w = |from: &str, to: &str| {
            rows.iter()
                .find(|r| r.from == from && r.to == to)
                .unwrap()
                .weight
        };
        assert_eq!(w("a", "b"), 1.0);
        assert_eq!(w("a", "c"), 0.5);
        assert_eq!(w("b", "c"), 0.5);
        assert_eq!(w("c", "a"), 1.0);

        let again = learn(&ballots, "ship", &rows, 0.5).unwrap();
        let w2 = |from: &str, to: &str| {
            again
                .iter()
                .find(|r| r.from == from && r.to == to)
                .unwrap()
                .weight
        };
        assert_eq!(w2("a", "c"), 0.25);
        assert_eq!(w2("a", "b"), 1.0);

        let floored = learn(&ballots, "ship", &[row("a", "c", 0.015)], 0.5).unwrap();
        let low = floored
            .iter()
            .find(|r| r.from == "a" && r.to == "c")
            .unwrap();
        assert_eq!(low.weight, TRUST_FLOOR);

        assert!(learn(&ballots, "ship", &[], 1.0).is_err());
        assert!(learn(&ballots, "  ", &[], 0.5).is_err());
        assert!(learn(&ballots[..1], "ship", &[], 0.5).is_err());
    }

    #[test]
    fn a_name_is_one_work_id_and_hex_passes_through() {
        let a = work_id("demo-riml");
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(a, work_id(" demo-riml "));
        assert_ne!(a, work_id("demo-rimm"));
        assert_eq!(work_id(&a.to_ascii_uppercase()), a);
        assert_ne!(work_id("seat"), work_id("reader"));
    }

    #[test]
    fn an_island_prints_one_memory_a_line() {
        let body = serde_json::json!({"island": [
            {"id": "a", "text": "one", "activation": 1.0, "seed": true},
            {"id": "b", "text": "two", "activation": 0.25, "seed": false}
        ]});
        assert_eq!(
            format_island(&body),
            "1.000\tseed\ta\tone\n0.250\t    \tb\ttwo\n"
        );
        assert!(format_island(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn a_fed_verb_reads_its_stdin() {
        let said = run_fed("cat", &[] as &[&str], "one\ntwo\n").unwrap();
        assert_eq!(said.stdout, "one\ntwo\n");
        assert!(run_fed("sh", &["-c", "exit 2"], "").is_err());
    }

    #[test]
    fn needs_and_cited_are_enclosed_once_each() {
        let needs = needs_of(r#"{"needs":["deed-b-2","deed-a-1"],"other":1}"#).unwrap();
        assert_eq!(needs, vec!["deed-b-2", "deed-a-1"]);
        assert_eq!(
            enclose(needs, "deed-a-1\n\ndeed-c-3\n"),
            vec!["deed-a-1", "deed-b-2", "deed-c-3"]
        );
        assert!(needs_of("{}").unwrap().is_empty());
        assert!(needs_of("not json").is_err());
    }

    #[test]
    fn due_is_the_past_soonest_first() {
        let atoms = vec![
            serde_json::json!({"id": "late", "due_at": "2026-02-01T00:00:00.000Z"}),
            serde_json::json!({"id": "later", "due_at": "2026-03-01T00:00:00.000Z"}),
            serde_json::json!({"id": "future", "due_at": "2099-01-01T00:00:00.000Z"}),
            serde_json::json!({"id": "never"}),
            serde_json::json!({"id": "blank", "due_at": ""}),
        ];
        let due = due_of(&atoms, "2026-06-01T00:00:00.000Z");
        let ids: Vec<&str> = due.iter().map(|a| a["id"].as_str().unwrap()).collect();
        // A claim that never entered the clock is due now, ahead of the
        // past-due ones; the future one waits.
        assert_eq!(ids, ["never", "blank", "late", "later"]);
        assert!(now_utc().ends_with(".000Z"));
        assert!(now_utc().as_str() > "2026-01-01T00:00:00.000Z");
    }

    #[test]
    fn the_doctor_names_every_habitat_and_the_pack_gates_health() {
        let rows = doctor();
        let names: Vec<&str> = rows.iter().map(|h| h.name).collect();
        for want in [
            "vissue",
            "deedar",
            "packset",
            "pack",
            "host key",
            "deed store",
            "tracker",
        ] {
            assert!(names.contains(&want), "{names:?}");
        }
        let table = format_doctor(&rows);
        assert_eq!(table.lines().count(), rows.len());
        let sick = vec![Habitat {
            name: "pack",
            state: "PACKSET_URL unset".into(),
            ok: false,
        }];
        assert!(!healthy(&sick));
        let fine = vec![Habitat {
            name: "claimdag",
            state: "not on PATH".into(),
            ok: false,
        }];
        assert!(healthy(&fine));
    }

    #[test]
    fn enclosed_atoms_are_read_from_every_jsonl_in_the_bag() {
        let dir = std::env::temp_dir().join(format!("ljos-bag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let atoms = dir.join("data").join("atoms");
        std::fs::create_dir_all(&atoms).unwrap();
        std::fs::write(
            atoms.join("a.jsonl"),
            "{\"kind\":\"lesson\",\"text\":\"one\"}\n\n{\"kind\":\"trust\",\"from\":\"a\",\"to\":\"b\",\"weight\":0.5}\n",
        )
        .unwrap();
        std::fs::write(
            atoms.join("b.jsonl"),
            "{\"kind\":\"preference\",\"text\":\"two\"}\n",
        )
        .unwrap();
        let read = enclosed_atoms(&dir).unwrap();
        assert_eq!(read.len(), 3);
        assert_eq!(trust_rows(&read).len(), 1);
        assert!(enclosed_atoms(&dir.join("nowhere")).unwrap().is_empty());
        std::fs::write(atoms.join("c.jsonl"), "not json\n").unwrap();
        assert!(enclosed_atoms(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);

        let table = format_due(&[serde_json::json!({
            "id": "x", "kind": "lesson", "text": "t", "due_at": "2026-01-01T00:00:00.000Z"
        })]);
        assert_eq!(table, "2026-01-01T00:00:00.000Z\tlesson\tx\tt\n");
    }

    fn read_http(s: &mut impl Read) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = s.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = &buf[..at];
                let mut need = 0usize;
                for line in headers.split(|b| *b == b'\n') {
                    let line = std::str::from_utf8(line).unwrap_or("").trim();
                    if let Some(v) = line
                        .split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                        .map(|(_, v)| v.trim())
                    {
                        need = v.parse().unwrap_or(0);
                    }
                }
                let have = buf.len().saturating_sub(at + 4);
                if have >= need {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn serve_capture() -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(String::new()));
        let slot = captured.clone();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                *slot.lock().unwrap() = read_http(&mut s);
                let body =
                    r#"{"id":"atom-1","kind":"lesson","text":"the default fuse is CombMNZ"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}"), captured)
    }

    #[test]
    fn remember_posts_v1_atoms() {
        let (url, captured) = serve_capture();
        let client = PacksetClient::new(&url);
        let body = post_claim(&client, "Remember", "the default fuse is CombMNZ", "ws").unwrap();
        assert_eq!(body["id"], "atom-1");
        let req = captured.lock().unwrap().clone();
        assert!(req.contains("POST"), "{req}");
        assert!(req.contains("/v1/atoms"), "{req}");
        assert!(req.contains("\"kind\":\"lesson\""), "{req}");
        assert!(req.contains("the default fuse is CombMNZ"), "{req}");
        assert!(req.contains("\"level\":\"explicit\""), "{req}");
        assert!(!req.contains("extract"), "{req}");
    }

    #[test]
    fn forget_posts_the_id_and_workspace() {
        let (url, captured) = serve_capture();
        let client = PacksetClient::new(&url);
        let body = client.delete_atom("ws", "atom-1", None).unwrap();
        assert_eq!(body["id"], "atom-1");
        let req = captured.lock().unwrap().clone();
        assert!(req.contains("POST"), "{req}");
        assert!(req.contains("/v1/atoms/delete"), "{req}");
        assert!(req.contains("\"id\":\"atom-1\""), "{req}");
        assert!(req.contains("\"workspace\":\"ws\""), "{req}");
        // No deed named, no field: the pack should not have to tell an absent
        // citation from an empty one.
        assert!(!req.contains("\"why\""), "{req}");
    }

    /// The deed rides with the retraction, so the pack can write it onto the
    /// tombstone in the same step the atom leaves the live set.
    #[test]
    fn forget_carries_the_deed_that_withdrew_the_claim() {
        let (url, captured) = serve_capture();
        let client = PacksetClient::new(&url);
        client
            .delete_atom("ws", "atom-1", Some("deed-patch-overlay"))
            .unwrap();
        let req = captured.lock().unwrap().clone();
        assert!(req.contains("\"why\":\"deed-patch-overlay\""), "{req}");
    }

    /// An id is the whole of the request, so an empty one is a mistake worth
    /// naming rather than a delete of whatever the server decides that means.
    #[test]
    fn forget_refuses_an_empty_id() {
        let err = packset_forget("   ", None).unwrap_err();
        assert!(err.to_string().contains("atom id is required"), "{err}");
    }
}
