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
#[derive(Debug, Clone, PartialEq)]
pub struct Trust {
    pub from: String,
    pub to: String,
    pub weight: f64,
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
    Ok(atom)
}

/// The live rows in a set of atoms: the latest `trust` atom per `(from, to)`.
pub fn trust_rows(atoms: &[Value]) -> Vec<Trust> {
    let mut latest: std::collections::BTreeMap<(String, String), (String, f64)> =
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
        let key = (from.to_string(), to.to_string());
        match latest.get(&key) {
            Some((seen, _)) if *seen > ts => {}
            _ => {
                latest.insert(key, (ts, weight));
            }
        }
    }
    latest
        .into_iter()
        .map(|((from, to), (_, weight))| Trust { from, to, weight })
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
            let current = rows
                .iter()
                .find(|r| r.from == *from && r.to == *to)
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
    out.extend(harness_rows());
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
        let (mut kept, mut refused) = (0usize, Vec::new());
        for atom in &atoms {
            match client.post_atom(atom) {
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
    for atom in body["island"].as_array().into_iter().flatten() {
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
    match run_captured(
        "claimdag",
        &["claim", &id, "--assignee", &work_id(assignee)],
    ) {
        Ok(said) => Ok(said.stdout),
        Err(e) => {
            let text = e.to_string();
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
    let rows = doctor();
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
            let rows = learn(&ballots, option, &trust_from_pack()?, beta)?;
            for row in &rows {
                write_trust(row, &[])?;
            }
            out.push_str(&format!(
                "learned from outcome {option:?}: {} trust rows rewritten\n",
                rows.len()
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
    use std::process::{Command, Stdio};
    let path = which::which(bin).with_context(|| format!("{bin} not on PATH"))?;
    let mut cmd = Command::new(path);
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
