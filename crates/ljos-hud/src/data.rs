//! One snapshot of the habitats the pane shows. Read-only.

use anyhow::{Context, Result};
use claimdag::{WorkGraph, WorkId, WorkStatus};
use serde_json::Value;

use crate::graph::GraphLayout;

/// How a due atom sits on the review clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockState {
    /// Never graded: `due_at` empty.
    Unreviewed,
    /// `due_at` equals now.
    Due,
    /// `due_at` is in the past.
    Overdue,
    /// `due_at` is in the future (not in `due_of`).
    Scheduled,
}

impl ClockState {
    /// Classify one atom against the clock instant.
    #[must_use]
    pub fn of(due_at: &str, now: &str) -> Self {
        if due_at.is_empty() {
            Self::Unreviewed
        } else if due_at < now {
            Self::Overdue
        } else if due_at == now {
            Self::Due
        } else {
            Self::Scheduled
        }
    }

    /// Chip label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreviewed => "unreviewed",
            Self::Due => "due",
            Self::Overdue => "overdue",
            Self::Scheduled => "scheduled",
        }
    }
}

/// Counts the due rail paints as chips. `due` is `due_of` length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClockCounts {
    pub unreviewed: usize,
    pub due: usize,
    pub overdue: usize,
}

impl ClockCounts {
    /// From already-classified `due_of` rows.
    #[must_use]
    pub fn from_due(rows: &[DueRow]) -> Self {
        Self {
            unreviewed: rows
                .iter()
                .filter(|r| r.clock == ClockState::Unreviewed)
                .count(),
            due: rows.len(),
            overdue: rows
                .iter()
                .filter(|r| r.clock == ClockState::Overdue)
                .count(),
        }
    }
}

/// One due atom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueRow {
    pub id: String,
    pub kind: String,
    pub text: String,
    /// Review clock instant, empty when the atom has never been graded.
    pub due_at: String,
    pub clock: ClockState,
}

/// One live claim-graph node. A claim is a lease, not a ticket close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRow {
    pub id: String,
    pub status: String,
    pub summary: String,
    /// Holder, 12 hex, or `-` when unset.
    pub assignee: String,
    pub cas_gen: u64,
    /// `held` when Claimed|Running, else `open`.
    pub occupancy: String,
    /// Last touch, unix seconds. Lease remaining is
    /// [`claimdag::DEFAULT_LEASE_SECS`] minus quiet time.
    pub updated_unix: u64,
}

/// One packset search hit for the island cue.
#[derive(Debug, Clone, PartialEq)]
pub struct IslandHit {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub score: f64,
}

/// One activated island memory.
#[derive(Debug, Clone, PartialEq)]
pub struct IslandRow {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub seed: bool,
    pub activation: f64,
}

/// Island instrument: search hits plus `packset_island(cue, false)`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IslandSnap {
    pub hits: Vec<IslandHit>,
    pub rows: Vec<IslandRow>,
    pub weak: bool,
    /// True when the encoder answered. False is a dense-down banner.
    pub dense: bool,
    pub ok: bool,
    pub banner: String,
}

impl IslandSnap {
    /// No cue yet: not a pack-down.
    #[must_use]
    pub fn idle() -> Self {
        Self {
            dense: true,
            ok: true,
            ..Self::default()
        }
    }
}

/// What the pane paints. Habitat errors become empty columns plus a banner.
/// Trust is a graph, not a list of from→to floats.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub due: Vec<DueRow>,
    pub claims: Vec<ClaimRow>,
    pub graph: GraphLayout,
    /// False when the pack GET failed. Distinct from an honest empty graph.
    pub pack_ok: bool,
    pub banner: String,
    /// [`ljos_cli::review_summary`] on the same atoms as due and trust.
    pub review: String,
    pub clock: ClockCounts,
    pub island: IslandSnap,
    pub timeline: Vec<ljos_cli::Event>,
    pub timeline_issue: String,
    pub timeline_banner: String,
}

impl Snapshot {
    /// Load due, live claims, and trust. Never mutates a store.
    pub fn load() -> Self {
        Self::load_for("", "")
    }

    /// Load with an island cue and a timeline issue. Cue empty skips
    /// island. Issue empty skips the deed rail.
    ///
    /// One pack GET, then [`ljos_cli::due_of`], [`ljos_cli::trust_rows`],
    /// [`ljos_cli::personas_of`], and [`ljos_cli::review_summary`] on the
    /// same atoms. Claims are [`WorkGraph::open_dir`]. Island is
    /// [`ljos_cli::packset_search`] plus [`ljos_cli::packset_island`]
    /// with `fire` false. Timeline is [`ljos_cli::timeline_events`].
    pub fn load_for(cue: &str, issue: &str) -> Self {
        let mut banner = Vec::new();
        let (due, graph, pack_ok, review, clock) = match load_pack_panes() {
            Ok(panes) => panes,
            Err(err) => {
                banner.push(format!("pack: {err}"));
                (
                    Vec::new(),
                    GraphLayout::empty(),
                    false,
                    String::new(),
                    ClockCounts::default(),
                )
            }
        };
        let claims = match load_claims() {
            Ok(rows) => rows,
            Err(err) => {
                banner.push(format!("claims: {err}"));
                Vec::new()
            }
        };
        let island = load_island(cue);
        let (timeline, timeline_issue, timeline_banner) = load_timeline(issue);
        Self {
            due,
            claims,
            graph,
            pack_ok,
            banner: banner.join(" · "),
            review,
            clock,
            island,
            timeline,
            timeline_issue,
            timeline_banner,
        }
    }

    /// A banner-only snapshot when the off-thread load cannot join.
    /// The canvas stays empty; this must not invent personas or edges.
    /// An empty message is first paint, not a fake pack-down.
    pub fn banner_only(msg: String) -> Self {
        Self {
            due: Vec::new(),
            claims: Vec::new(),
            graph: GraphLayout::empty(),
            pack_ok: msg.is_empty(),
            banner: msg,
            review: String::new(),
            clock: ClockCounts::default(),
            island: IslandSnap::idle(),
            timeline: Vec::new(),
            timeline_issue: String::new(),
            timeline_banner: String::new(),
        }
    }
}

type PackPanes = (Vec<DueRow>, GraphLayout, bool, String, ClockCounts);

fn load_pack_panes() -> Result<PackPanes> {
    let client = ljos_cli::pack()?;
    let atoms = client
        .atoms_as_of(&client.workspace(), None)
        .context("pack: GET /v1/atoms failed")?;
    let now = ljos_cli::now_utc();
    let due: Vec<DueRow> = ljos_cli::due_of(&atoms, &now)
        .iter()
        .map(|a| due_row(a, &now))
        .collect();
    let clock = ClockCounts::from_due(&due);
    let review = ljos_cli::review_summary(&atoms, &now);
    let graph = GraphLayout::from_pack(
        &ljos_cli::personas_of(&atoms),
        &ljos_cli::trust_rows(&atoms),
    );
    Ok((due, graph, true, review, clock))
}

fn due_row(atom: &Value, now: &str) -> DueRow {
    let due_at = atom
        .get("due_at")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
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
        clock: ClockState::of(&due_at, now),
        due_at,
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
        .map(claim_row)
        .collect())
}

fn claim_row(n: &claimdag::WorkNode) -> ClaimRow {
    let held = matches!(n.status, WorkStatus::Claimed | WorkStatus::Running);
    ClaimRow {
        id: n.id.to_hex().chars().take(12).collect(),
        status: n.status.as_str().to_string(),
        summary: n.summary.chars().take(96).collect(),
        assignee: assignee_hex(n.assignee),
        cas_gen: n.cas_gen,
        occupancy: if held { "held" } else { "open" }.to_string(),
        updated_unix: n.updated_unix,
    }
}

fn assignee_hex(id: WorkId) -> String {
    if id.is_zero() {
        "-".into()
    } else {
        id.to_hex().chars().take(12).collect()
    }
}

/// Remaining lease seconds: [`claimdag::DEFAULT_LEASE_SECS`] minus quiet time.
#[must_use]
pub fn lease_remaining_secs(updated_unix: u64, now_unix: u64) -> i64 {
    claimdag::DEFAULT_LEASE_SECS as i64 - now_unix.saturating_sub(updated_unix) as i64
}

/// Format remaining lease for a held node.
#[must_use]
pub fn format_lease_remaining(updated_unix: u64, now_unix: u64) -> String {
    let left = lease_remaining_secs(updated_unix, now_unix);
    if left <= 0 {
        format!("overdue {}s", left.unsigned_abs())
    } else if left < 60 {
        format!("{left}s")
    } else {
        format!("{}m", left / 60)
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Held-node remaining at paint time.
#[must_use]
pub fn claim_lease_label(row: &ClaimRow) -> String {
    if row.occupancy != "held" {
        return "—".into();
    }
    format_lease_remaining(row.updated_unix, now_unix())
}

fn load_island(cue: &str) -> IslandSnap {
    let cue = cue.trim();
    if cue.is_empty() {
        return IslandSnap::idle();
    }
    let mut banner = Vec::new();
    let hits = match ljos_cli::packset_search(cue) {
        Ok(found) => found.into_iter().map(island_hit).collect(),
        Err(err) => {
            banner.push(format!("search: {err}"));
            Vec::new()
        }
    };
    match ljos_cli::packset_island(cue, false) {
        Ok(body) => {
            let weak = body["weak"].as_bool().unwrap_or(false);
            let dense = body["dense"].as_bool().unwrap_or(true);
            let rows = body["island"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| {
                    let kind = a.get("kind").and_then(Value::as_str).unwrap_or("");
                    kind != "trust" && kind != "persona"
                })
                .take(8)
                .map(island_row)
                .collect();
            IslandSnap {
                hits,
                rows,
                weak,
                dense,
                ok: true,
                banner: banner.join(" · "),
            }
        }
        Err(err) => {
            banner.push(format!("island: {err}"));
            IslandSnap {
                hits,
                rows: Vec::new(),
                weak: false,
                dense: true,
                ok: false,
                banner: banner.join(" · "),
            }
        }
    }
}

fn island_hit(h: ljos_cli::Hit) -> IslandHit {
    IslandHit {
        id: h
            .id
            .unwrap_or_else(|| "-".into())
            .chars()
            .take(12)
            .collect(),
        kind: h.kind,
        text: h.text.chars().take(96).collect(),
        score: h.score,
    }
}

fn island_row(atom: &Value) -> IslandRow {
    IslandRow {
        id: atom
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("-")
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
        seed: atom.get("seed").and_then(Value::as_bool).unwrap_or(false),
        activation: atom
            .get("activation")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
    }
}

fn load_timeline(issue: &str) -> (Vec<ljos_cli::Event>, String, String) {
    let issue = issue.trim();
    if issue.is_empty() {
        return (Vec::new(), String::new(), String::new());
    }
    match ljos_cli::timeline_events(issue, ljos_cli::SITTING_TIMELINE) {
        Ok(events) => (events, issue.to_string(), String::new()),
        Err(err) => (Vec::new(), issue.to_string(), format!("timeline: {err}")),
    }
}

/// Tracker ids look like `proj-xxxx`.
#[must_use]
pub fn looks_like_issue(s: &str) -> bool {
    let s = s.trim();
    let Some((proj, rest)) = s.split_once('-') else {
        return false;
    };
    !proj.is_empty()
        && proj.chars().all(|c| c.is_ascii_lowercase())
        && !rest.is_empty()
        && rest.chars().all(|c| c.is_ascii_alphanumeric())
        && !rest.contains('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_row_truncates_text() {
        let atom =
            serde_json::json!({"id":"abcdefghijklmnop","kind":"lesson","text":"x".repeat(200)});
        let row = due_row(&atom, "2026-09-20T00:00:00Z");
        assert_eq!(row.id.len(), 12);
        assert_eq!(row.kind, "lesson");
        assert_eq!(row.text.len(), 96);
        assert!(row.due_at.is_empty());
        assert_eq!(row.clock, ClockState::Unreviewed);
    }

    #[test]
    fn due_row_keeps_due_at() {
        let atom = serde_json::json!({
            "id":"abcdefghijklmnop",
            "kind":"lesson",
            "text":"x".repeat(200),
            "due_at":"2026-09-20T10:00:00.000Z"
        });
        let row = due_row(&atom, "2026-09-20T11:00:00.000Z");
        assert_eq!(row.id.len(), 12);
        assert_eq!(row.kind, "lesson");
        assert_eq!(row.text.len(), 96);
        assert_eq!(row.due_at, "2026-09-20T10:00:00.000Z");
        assert_eq!(row.clock, ClockState::Overdue);
    }

    #[test]
    fn clock_state_splits_unreviewed_due_overdue() {
        let now = "2026-09-20T12:00:00Z";
        assert_eq!(ClockState::of("", now), ClockState::Unreviewed);
        assert_eq!(ClockState::of("2026-09-20T12:00:00Z", now), ClockState::Due);
        assert_eq!(
            ClockState::of("2026-09-19T00:00:00Z", now),
            ClockState::Overdue
        );
        assert_eq!(
            ClockState::of("2026-09-21T00:00:00Z", now),
            ClockState::Scheduled
        );
        let rows = vec![
            due_row(
                &serde_json::json!({"id":"aaaaaaaaaaaa","kind":"lesson","text":"a"}),
                now,
            ),
            due_row(
                &serde_json::json!({
                    "id":"bbbbbbbbbbbb","kind":"lesson","text":"b",
                    "due_at":"2026-09-19T00:00:00Z"
                }),
                now,
            ),
            due_row(
                &serde_json::json!({
                    "id":"cccccccccccc","kind":"lesson","text":"c",
                    "due_at":"2026-09-20T12:00:00Z"
                }),
                now,
            ),
        ];
        let counts = ClockCounts::from_due(&rows);
        assert_eq!(counts.unreviewed, 1);
        assert_eq!(counts.due, 3);
        assert_eq!(counts.overdue, 1);
    }

    #[test]
    fn lease_remaining_is_default_minus_quiet() {
        assert_eq!(claimdag::DEFAULT_LEASE_SECS, 900);
        assert_eq!(lease_remaining_secs(1000, 1000), 900);
        assert_eq!(lease_remaining_secs(1000, 1100), 800);
        assert_eq!(lease_remaining_secs(1000, 2000), -100);
        assert_eq!(format_lease_remaining(1000, 1000), "15m");
        assert_eq!(format_lease_remaining(1000, 1860), "40s");
        assert_eq!(format_lease_remaining(1000, 2000), "overdue 100s");
        let open = ClaimRow {
            id: "a".into(),
            status: "ready".into(),
            summary: "s".into(),
            assignee: "-".into(),
            cas_gen: 1,
            occupancy: "open".into(),
            updated_unix: 1000,
        };
        assert_eq!(claim_lease_label(&open), "—");
    }

    #[test]
    fn looks_like_issue_is_proj_dash_id() {
        assert!(looks_like_issue("ljos-9ptd"));
        assert!(looks_like_issue("vissue-k9f1"));
        assert!(!looks_like_issue("trust graph"));
        assert!(!looks_like_issue(""));
        assert!(!looks_like_issue("ljos"));
        assert!(!looks_like_issue("ljos-9ptd-extra"));
    }

    #[test]
    fn island_idle_when_cue_empty() {
        let idle = IslandSnap::idle();
        assert!(idle.ok);
        assert!(idle.dense);
        assert!(!idle.weak);
        assert!(idle.rows.is_empty());
        assert!(idle.hits.is_empty());
        let snap = Snapshot::banner_only(String::new());
        assert!(snap.island.ok);
        assert!(snap.timeline.is_empty());
        assert!(snap.timeline_issue.is_empty());
    }

    #[test]
    fn library_apis_not_cli_or_writer_paths() {
        let src = include_str!("data.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("due_of"));
        assert!(prod.contains("review_summary"));
        assert!(prod.contains("open_dir"));
        assert!(prod.contains("trust_rows"));
        assert!(prod.contains("personas_of"));
        assert!(prod.contains("packset_search"));
        assert!(prod.contains("packset_island(cue, false)"));
        assert!(prod.contains("timeline_events"));
        assert!(!prod.contains("due_report"));
        assert!(!prod.contains("WorkGraph::load_dir"));
        assert!(!prod.contains(".load_dir("));
        assert!(!prod.contains("personas_from_pack"));
        assert!(!prod.contains("trust_from_pack"));
        assert!(!prod.contains("fire: true"));
        assert!(!prod.contains("fire=true"));
        assert!(!prod.contains("vissue update"));
        assert!(!prod.contains(".complete("));
        assert!(!prod.contains(".reclaim("));
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
        assert!(!snap.pack_ok);
        assert_eq!(snap.banner, "pack: down");
        assert!(snap.graph.paint_ops(0, true, 400.0, 400.0).is_empty());
        assert!(snap.island.ok);
        assert!(snap.timeline.is_empty());
    }

    #[test]
    fn boot_banner_only_is_not_pack_down() {
        let snap = Snapshot::banner_only(String::new());
        assert!(snap.pack_ok, "first paint must not fake pack-down");
        assert!(snap.graph.is_empty());
        assert!(snap.banner.is_empty());
        assert!(snap.review.is_empty());
    }
}
