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

/// Remember → lesson, Prefer → preference. No other write kinds.
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

/// `ljos-consensus` first, then `vissue consensus`. Missing bins are skipped.
pub fn consensus_steps(id: &str, have_ljos: bool, have_vissue: bool) -> Result<Vec<ConsensusStep>> {
    if !have_ljos && !have_vissue {
        bail!("neither ljos-consensus nor vissue is on PATH");
    }
    let mut steps = Vec::new();
    if have_ljos {
        steps.push(ConsensusStep {
            bin: "ljos-consensus",
            args: vec!["settle".into(), "--issue".into(), id.into()],
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

pub fn card_paths(dir: &Path) -> Vec<PathBuf> {
    CARD_NAMES.iter().map(|n| dir.join(n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

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
        let steps = consensus_steps("vissue-1a5a", true, true).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].bin, "ljos-consensus");
        assert_eq!(steps[0].args, vec!["settle", "--issue", "vissue-1a5a"]);
        assert_eq!(steps[1].bin, "vissue");
        assert_eq!(steps[1].args, vec!["consensus", "vissue-1a5a"]);
    }

    #[test]
    fn consensus_skips_a_missing_bin() {
        let only_v = consensus_steps("id", false, true).unwrap();
        assert_eq!(only_v.len(), 1);
        assert_eq!(only_v[0].bin, "vissue");
        let only_l = consensus_steps("id", true, false).unwrap();
        assert_eq!(only_l[0].bin, "ljos-consensus");
        assert!(consensus_steps("id", false, false).is_err());
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
