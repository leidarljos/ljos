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

/// Printed on stderr. `grok-policyd` is the TCB when it exists.
pub const POLICY_TCB: &str =
    "argv law. grok-policyd is the TCB when present. Reloading a pack is not a check.";

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
    let client =
        PacksetClient::from_env().context("PACKSET_URL unset; remember/prefer POST /v1/atoms")?;
    let workspace = client.workspace();
    post_claim(&client, label, text, &workspace)
}

/// Retire one atom from the workspace the cwd resolves to.
///
/// The daemon tombstones rather than erases: the atom stops being recalled and
/// the pack still records that it was held and withdrawn. That is the right
/// shape for standing knowledge, where "we no longer believe this" is itself
/// worth keeping.
///
/// # Errors
///
/// An unset `PACKSET_URL`, an id the workspace does not hold, or the request's.
pub fn packset_forget(id: &str) -> Result<Value> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("forget: an atom id is required");
    }
    let client =
        PacksetClient::from_env().context("PACKSET_URL unset; forget POSTs /v1/atoms/delete")?;
    let workspace = client.workspace();
    client
        .delete_atom(&workspace, trimmed)
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
    let client = PacksetClient::from_env().context("PACKSET_URL unset; trust lives in the pack")?;
    let workspace = client.workspace();
    let atoms = client
        .atoms_as_of(&workspace, None)
        .context("trust: GET /v1/atoms failed")?;
    Ok(trust_rows(&atoms))
}

/// POST one trust row.
pub fn write_trust(row: &Trust, why: &[String]) -> Result<Value> {
    let client = PacksetClient::from_env().context("PACKSET_URL unset; trust lives in the pack")?;
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
            state: "PACKSET_URL unset".into(),
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
        Err(_) => lines.push("no pack: PACKSET_URL unset, atoms not enclosed".into()),
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
        let client =
            PacksetClient::from_env().context("PACKSET_URL unset; import POSTs /v1/atoms")?;
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

/// The live atoms whose review is due at `now` (RFC 3339 UTC), soonest first.
pub fn due_of(atoms: &[Value], now: &str) -> Vec<Value> {
    let mut due: Vec<Value> = atoms
        .iter()
        .filter(|a| {
            a.get("due_at")
                .and_then(Value::as_str)
                .is_some_and(|d| !d.is_empty() && d <= now)
        })
        .cloned()
        .collect();
    due.sort_by(|a, b| a["due_at"].as_str().cmp(&b["due_at"].as_str()));
    due
}

/// What the pack holds for review now.
pub fn due() -> Result<Vec<Value>> {
    let client = PacksetClient::from_env().context("PACKSET_URL unset; due reads /v1/atoms")?;
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
                a["due_at"].as_str().unwrap_or(""),
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
    let client = PacksetClient::from_env().context("PACKSET_URL unset; graded POSTs /v1/grade")?;
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

pub fn packset_search(query: &str) -> Result<Vec<Hit>> {
    let q = query.trim();
    if q.is_empty() {
        bail!("search: empty query");
    }
    let client =
        PacksetClient::from_env().context("PACKSET_URL unset; search is GET /v1/search")?;
    let workspace = client.workspace();
    client
        .search(&workspace, q, 10)
        .context("search: GET /v1/search failed")
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

/// `ljos-consensus` first, with the pack's trust rows when there are any,
/// then `vissue consensus`. Missing bins are skipped.
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
        steps.push(ConsensusStep {
            bin: "vissue",
            args: vec!["consensus".into(), id.into()],
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
        let steps = consensus_steps("id", true, false, &rows).unwrap();
        assert_eq!(steps[0].args[3], "--trust");
        assert_eq!(steps[0].args[4], r#"[["a","b",0.5]]"#);
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
        assert_eq!(ids, ["late", "later"]);
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
        let body = client.delete_atom("ws", "atom-1").unwrap();
        assert_eq!(body["id"], "atom-1");
        let req = captured.lock().unwrap().clone();
        assert!(req.contains("POST"), "{req}");
        assert!(req.contains("/v1/atoms/delete"), "{req}");
        assert!(req.contains("\"id\":\"atom-1\""), "{req}");
        assert!(req.contains("\"workspace\":\"ws\""), "{req}");
    }

    /// An id is the whole of the request, so an empty one is a mistake worth
    /// naming rather than a delete of whatever the server decides that means.
    #[test]
    fn forget_refuses_an_empty_id() {
        let err = packset_forget("   ").unwrap_err();
        assert!(err.to_string().contains("atom id is required"), "{err}");
    }
}
