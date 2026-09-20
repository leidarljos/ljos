//! One snapshot of the three habitats the pane shows. Read-only.
//!
//! One pack GET, then [`ljos_cli::due_of`] and [`ljos_cli::trust_rows`].
//! Claims go through [`claimdag::WorkGraph::open_dir`]; absence is a banner,
//! never a silent empty list.

use anyhow::Result;
use claimdag::{WorkGraph, WorkStatus};
use serde_json::Value;

/// One due atom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueRow {
    pub id: String,
    pub kind: String,
    pub text: String,
}

/// One live claim-graph node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRow {
    pub id: String,
    pub status: String,
    pub summary: String,
}

/// One live trust row.
#[derive(Debug, Clone, PartialEq)]
pub struct TrustRow {
    pub from: String,
    pub to: String,
    pub weight: f64,
    pub about: String,
}

/// What the pane paints. Habitat errors become empty columns plus a banner.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub due: Vec<DueRow>,
    pub claims: Vec<ClaimRow>,
    pub trust: Vec<TrustRow>,
    pub banner: String,
}

impl Snapshot {
    /// Empty columns, no banner.
    pub fn empty() -> Self {
        Self {
            due: Vec::new(),
            claims: Vec::new(),
            trust: Vec::new(),
            banner: String::new(),
        }
    }

    /// Load due, live claims, and trust. Never mutates a store.
    pub fn load() -> Self {
        let mut banner = Vec::new();
        let (due, trust) = match load_pack() {
            Ok(pair) => pair,
            Err(err) => {
                banner.push(format!("pack: {err}"));
                (Vec::new(), Vec::new())
            }
        };
        let claims = match load_claims() {
            Ok(rows) => rows,
            Err(err) => {
                banner.push(format!("claims: {err}"));
                Vec::new()
            }
        };
        Self {
            due,
            claims,
            trust,
            banner: banner.join(" · "),
        }
    }
}

fn load_pack() -> Result<(Vec<DueRow>, Vec<TrustRow>)> {
    let client = ljos_cli::pack()?;
    let atoms = client
        .atoms_as_of(&client.workspace(), None)
        .map_err(|err| anyhow::anyhow!("GET /v1/atoms failed: {err}"))?;
    let due = ljos_cli::due_of(&atoms, &ljos_cli::now_utc())
        .iter()
        .map(due_row)
        .collect();
    let trust = ljos_cli::trust_rows(&atoms)
        .into_iter()
        .map(|r| TrustRow {
            from: r.from,
            to: r.to,
            weight: r.weight,
            about: r.about.join(","),
        })
        .collect();
    Ok((due, trust))
}

fn due_row(atom: &Value) -> DueRow {
    DueRow {
        id: atom
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .chars()
            .take(12)
            .collect(),
        kind: atom
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("claim")
            .to_string(),
        text: atom
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(96)
            .collect(),
    }
}

fn load_claims() -> Result<Vec<ClaimRow>> {
    let dir = claimdag::resolve_dir(None);
    let graph = WorkGraph::open_dir(&dir).map_err(|absent| anyhow::anyhow!("{absent}"))?;
    Ok(graph
        .list_view(false, false)
        .into_iter()
        .filter(|n| {
            matches!(
                n.status,
                WorkStatus::Claimed | WorkStatus::Running | WorkStatus::Ready
            )
        })
        .map(|n| ClaimRow {
            id: n.id.to_hex().chars().take(12).collect(),
            status: n.status.as_str().to_string(),
            summary: n.summary.chars().take(96).collect(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_row_truncates_text() {
        let atom =
            serde_json::json!({"id":"abcdefghijklmnop","kind":"lesson","text":"x".repeat(200)});
        let row = due_row(&atom);
        assert_eq!(row.id.len(), 12);
        assert_eq!(row.kind, "lesson");
        assert_eq!(row.text.len(), 96);
    }

    #[test]
    fn open_dir_absence_is_a_banner() {
        let dir =
            std::env::temp_dir().join(format!("ljos-hud-absent-claims-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let err = WorkGraph::open_dir(&dir).expect_err("absent");
        let text = err.to_string();
        assert!(
            text.contains("no work graph") || text.contains("does not exist"),
            "{text}"
        );
        let _guard = crate::env_lock();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("CLAIMDAG_DIR", &dir);
            std::env::set_var("PACKSET_URL", "off");
        }
        let snap = Snapshot::load();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var("CLAIMDAG_DIR");
            std::env::remove_var("PACKSET_URL");
        }
        assert!(
            snap.banner.contains("claims:"),
            "absence is a banner, not a silent empty: {}",
            snap.banner
        );
        assert!(snap.claims.is_empty());
    }

    #[test]
    fn library_apis_only() {
        let src = include_str!("data.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("due_of"));
        assert!(prod.contains("trust_rows"));
        assert!(prod.contains("WorkGraph::open_dir"));
        assert!(prod.contains("list_view"));
        assert!(!prod.contains("due_report"));
        assert!(!prod.contains("load_dir"));
        assert!(!prod.contains("trust_from_pack"));
        assert!(!prod.contains("due_report"));
        assert!(!prod.contains("fire=true"));
        assert!(!prod.contains("graded"));
    }
}
