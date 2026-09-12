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
}
