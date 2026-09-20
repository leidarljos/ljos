//! One snapshot of the three habitats the pane shows. Read-only.

use anyhow::{Context, Result};
use claimdag::{WorkGraph, WorkStatus};
use serde_json::Value;

use crate::graph::GraphLayout;

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

/// What the pane paints. Habitat errors become empty columns plus a banner.
/// Trust is a graph, not a list of from→to floats.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub due: Vec<DueRow>,
    pub claims: Vec<ClaimRow>,
    pub graph: GraphLayout,
    pub banner: String,
}

impl Snapshot {
    /// Load due, live claims, and trust. Never mutates a store.
    ///
    /// One pack GET, then [`ljos_cli::due_of`], [`ljos_cli::trust_rows`],
    /// and [`ljos_cli::personas_of`] on the same atoms. Claims are
    /// [`WorkGraph::open_dir`]; the writer-empty graph constructor is not used.
    pub fn load() -> Self {
        let mut banner = Vec::new();
        let (due, graph) = match load_pack_panes() {
            Ok(pair) => pair,
            Err(err) => {
                banner.push(format!("pack: {err}"));
                (Vec::new(), GraphLayout::empty())
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
            graph,
            banner: banner.join(" · "),
        }
    }

    /// A banner-only snapshot when the off-thread load cannot join.
    /// The canvas stays empty; this must not invent personas or edges.
    pub fn banner_only(msg: String) -> Self {
        Self {
            due: Vec::new(),
            claims: Vec::new(),
            graph: GraphLayout::empty(),
            banner: msg,
        }
    }
}

fn load_pack_panes() -> Result<(Vec<DueRow>, GraphLayout)> {
    let client = ljos_cli::pack()?;
    let atoms = client
        .atoms_as_of(&client.workspace(), None)
        .context("pack: GET /v1/atoms failed")?;
    let now = ljos_cli::now_utc();
    let due = ljos_cli::due_of(&atoms, &now).iter().map(due_row).collect();
    let graph = GraphLayout::from_pack(
        &ljos_cli::personas_of(&atoms),
        &ljos_cli::trust_rows(&atoms),
    );
    Ok((due, graph))
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
    let graph = WorkGraph::open_dir(&dir)?;
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
    fn library_apis_not_cli_or_writer_paths() {
        let src = include_str!("data.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("due_of"));
        assert!(prod.contains("open_dir"));
        assert!(prod.contains("trust_rows"));
        assert!(prod.contains("personas_of"));
        assert!(!prod.contains("due_report"));
        assert!(!prod.contains("WorkGraph::load_dir"));
        assert!(!prod.contains(".load_dir("));
        assert!(!prod.contains("personas_from_pack"));
        assert!(!prod.contains("trust_from_pack"));
        assert!(
            !prod.contains("Ok(Vec::new())"),
            "open_dir Err must not become an empty column"
        );
    }

    #[test]
    fn banner_only_is_an_empty_canvas() {
        let snap = Snapshot::banner_only("pack: down".into());
        assert!(snap.graph.is_empty());
        assert!(snap.due.is_empty());
        assert!(snap.claims.is_empty());
        assert_eq!(snap.banner, "pack: down");
        assert!(snap.graph.paint_ops(0, true, 400.0, 400.0).is_empty());
    }
}
