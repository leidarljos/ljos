//! Trust instrument as a graph: personas are vertices, trust rows are edges.
//!
//! Layout lives here (Fruchterman–Reingold). The canvas maps stored
//! positions; it does not run the spring every frame.

use ljos_cli::{Persona, Trust};

/// Seed for the first placement. The same edge set plus this seed is a
/// stable drawing.
pub const LAYOUT_SEED: u64 = 0xC0FF_EE42_9A7D_51ED;

const LAYOUT_ITERS: u32 = 80;
const DASH: [f32; 2] = [6.0, 4.0];

/// One vertex: a persona, or an endpoint that has no persona atom.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    /// Persona name, or the trust row's `from` / `to`.
    pub name: String,
    /// `None` when the pack has no persona of this name (hollow ring).
    pub anchor: Option<f64>,
    /// Unit-square x after layout. Mapped to pixels at paint.
    pub x: f32,
    /// Unit-square y after layout.
    pub y: f32,
}

impl GraphNode {
    /// Missing persona: stroke only, no fill.
    #[must_use]
    pub fn hollow(&self) -> bool {
        self.anchor.is_none()
    }

    /// Ring stroke. Wider when the persona is less anchored (`1 - anchor`).
    #[must_use]
    pub fn ring_width(&self) -> f32 {
        match self.anchor {
            Some(a) => 1.5 + 8.0 * (1.0 - a.clamp(0.0, 1.0)) as f32,
            None => 1.5,
        }
    }
}

/// One directed trust row.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    /// Listener.
    pub from: String,
    /// Speaker.
    pub to: String,
    pub from_idx: usize,
    pub to_idx: usize,
    pub weight: f64,
    /// Empty is unscoped (solid). Any domain is scoped (dashed).
    pub about: Vec<String>,
}

impl GraphEdge {
    /// Scoped rows are dashed.
    #[must_use]
    pub fn dashed(&self) -> bool {
        !self.about.is_empty()
    }

    /// Stroke width grows with weight.
    #[must_use]
    pub fn stroke_width(&self) -> f32 {
        1.0 + 5.0 * (self.weight.max(0.0) as f32)
    }

    /// Stroke alpha grows with weight, capped at 1.
    #[must_use]
    pub fn stroke_alpha(&self) -> f32 {
        0.3 + 0.7 * (self.weight.clamp(0.0, 1.0) as f32)
    }

    /// Dash pattern for a scoped edge. Empty slice is a solid line.
    #[must_use]
    pub fn dash(&self) -> &'static [f32] {
        if self.dashed() {
            &DASH
        } else {
            &[]
        }
    }

    /// `from → to  weight  about` as the focused-node readout.
    #[must_use]
    pub fn readout(&self) -> String {
        if self.about.is_empty() {
            format!("{} → {}  {:.3}", self.from, self.to, self.weight)
        } else {
            format!(
                "{} → {}  {:.3}  {}",
                self.from,
                self.to,
                self.weight,
                self.about.join(",")
            )
        }
    }
}

/// Vertices ∪ endpoints, plus the trust rows between them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrustGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl TrustGraph {
    /// No vertices, no edges. Pack-down and honest-empty both start here.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// `personas_from_pack() ∪ {Trust.from, Trust.to}`, edges `trust_from_pack()`.
    #[must_use]
    pub fn from_pack(personas: &[Persona], trust: &[Trust]) -> Self {
        Self::from_pack_seeded(personas, trust, LAYOUT_SEED)
    }

    /// Same as [`Self::from_pack`] with an explicit layout seed.
    #[must_use]
    pub fn from_pack_seeded(personas: &[Persona], trust: &[Trust], seed: u64) -> Self {
        let mut names = std::collections::BTreeSet::new();
        let mut anchors = std::collections::BTreeMap::new();
        for p in personas {
            names.insert(p.name.clone());
            anchors.insert(p.name.clone(), p.anchor);
        }
        for row in trust {
            names.insert(row.from.clone());
            names.insert(row.to.clone());
        }
        let mut nodes: Vec<GraphNode> = names
            .into_iter()
            .map(|name| GraphNode {
                anchor: anchors.get(&name).copied(),
                name,
                x: 0.0,
                y: 0.0,
            })
            .collect();
        let index: std::collections::BTreeMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.name.clone(), i))
            .collect();
        let edges: Vec<GraphEdge> = trust
            .iter()
            .filter_map(|row| {
                Some(GraphEdge {
                    from_idx: *index.get(&row.from)?,
                    to_idx: *index.get(&row.to)?,
                    from: row.from.clone(),
                    to: row.to.clone(),
                    weight: row.weight,
                    about: row.about.clone(),
                })
            })
            .collect();
        layout_fr(&mut nodes, &edges, seed);
        Self { nodes, edges }
    }

    /// Topology plus weights. Layout reruns when this changes, not per frame.
    #[must_use]
    pub fn edge_key(&self) -> Vec<(String, String, Vec<String>, u64)> {
        let mut key: Vec<_> = self
            .edges
            .iter()
            .map(|e| {
                (
                    e.from.clone(),
                    e.to.clone(),
                    e.about.clone(),
                    e.weight.to_bits(),
                )
            })
            .collect();
        key.sort();
        key
    }

    /// Vertex names in layout order.
    #[must_use]
    pub fn node_key(&self) -> Vec<String> {
        self.nodes.iter().map(|n| n.name.clone()).collect()
    }

    /// Replace this graph when the edge or vertex set moved; keep positions otherwise.
    pub fn replace_if_edge_set_changed(&mut self, next: Self) {
        if self.edge_key() != next.edge_key() || self.node_key() != next.node_key() {
            *self = next;
        } else {
            for (old, new) in self.nodes.iter_mut().zip(next.nodes.iter()) {
                old.anchor = new.anchor;
            }
        }
    }

    /// Incident edges of `focused`, each as `from → to  weight  about`.
    /// An isolated node reads as its name.
    #[must_use]
    pub fn readout(&self, focused: usize) -> String {
        let Some(node) = self.nodes.get(focused) else {
            return String::new();
        };
        let lines: Vec<String> = self
            .edges
            .iter()
            .filter(|e| e.from_idx == focused || e.to_idx == focused)
            .map(GraphEdge::readout)
            .collect();
        if lines.is_empty() {
            node.name.clone()
        } else {
            lines.join("  ·  ")
        }
    }
}

/// Fruchterman–Reingold on the unit square. Deterministic for a fixed seed.
fn layout_fr(nodes: &mut [GraphNode], edges: &[GraphEdge], seed: u64) {
    let n = nodes.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        nodes[0].x = 0.5;
        nodes[0].y = 0.5;
        return;
    }
    let mut rng = seed | 1;
    let two_pi = std::f32::consts::TAU;
    for (i, node) in nodes.iter_mut().enumerate() {
        let jitter = xorshift(&mut rng) * 0.02;
        let angle = (i as f32) * two_pi / (n as f32) + jitter;
        node.x = 0.5 + 0.35 * angle.cos();
        node.y = 0.5 + 0.35 * angle.sin();
    }
    let k = (1.0 / n as f32).sqrt();
    let mut temp = 0.12_f32;
    let mut dx = vec![0.0_f32; n];
    let mut dy = vec![0.0_f32; n];
    for _ in 0..LAYOUT_ITERS {
        dx.fill(0.0);
        dy.fill(0.0);
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let vx = nodes[i].x - nodes[j].x;
                let vy = nodes[i].y - nodes[j].y;
                let dist2 = vx * vx + vy * vy + 1e-6;
                let dist = dist2.sqrt();
                let force = k * k / dist;
                dx[i] += vx / dist * force;
                dy[i] += vy / dist * force;
            }
        }
        for e in edges {
            if e.from_idx == e.to_idx {
                continue;
            }
            let i = e.from_idx;
            let j = e.to_idx;
            let vx = nodes[i].x - nodes[j].x;
            let vy = nodes[i].y - nodes[j].y;
            let dist = (vx * vx + vy * vy).sqrt().max(1e-6);
            let force = dist * dist / k * (e.weight.max(0.05) as f32);
            dx[i] -= vx / dist * force;
            dy[i] -= vy / dist * force;
            dx[j] += vx / dist * force;
            dy[j] += vy / dist * force;
        }
        for i in 0..n {
            let mag = (dx[i] * dx[i] + dy[i] * dy[i]).sqrt().max(1e-6);
            let step = mag.min(temp);
            nodes[i].x = (nodes[i].x + dx[i] / mag * step).clamp(0.08, 0.92);
            nodes[i].y = (nodes[i].y + dy[i] / mag * step).clamp(0.08, 0.92);
        }
        temp *= 0.95;
    }
}

fn xorshift(state: &mut u64) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    (x as f32) / (u64::MAX as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persona(name: &str, anchor: f64) -> Persona {
        Persona {
            name: name.into(),
            anchor,
            view: "reads the work".into(),
            entities: vec![],
        }
    }

    fn trust(from: &str, to: &str, weight: f64, about: &[&str]) -> Trust {
        Trust {
            from: from.into(),
            to: to.into(),
            weight,
            about: about.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn graph_builds_from_fixture_atoms() {
        let personas = vec![
            persona("alice", 0.4),
            persona("bob", 0.9),
            persona("dave", 0.2),
        ];
        let rows = vec![
            trust("alice", "bob", 0.8, &[]),
            trust("alice", "carol", 0.2, &["hud"]),
        ];
        let g = TrustGraph::from_pack(&personas, &rows);
        let names: Vec<&str> = g.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["alice", "bob", "carol", "dave"]);
        assert_eq!(g.edges.len(), 2);
        let alice = g.nodes.iter().find(|n| n.name == "alice").unwrap();
        let bob = g.nodes.iter().find(|n| n.name == "bob").unwrap();
        let carol = g.nodes.iter().find(|n| n.name == "carol").unwrap();
        let dave = g.nodes.iter().find(|n| n.name == "dave").unwrap();
        assert!(!alice.hollow());
        assert!(carol.hollow(), "missing persona is hollow");
        assert!(alice.ring_width() > bob.ring_width(), "ring ∝ (1 - anchor)");
        assert_eq!(dave.anchor, Some(0.2));
        let solid = g.edges.iter().find(|e| e.to == "bob").unwrap();
        let dashed = g.edges.iter().find(|e| e.to == "carol").unwrap();
        assert!(!solid.dashed());
        assert!(dashed.dashed());
        assert!(solid.stroke_width() > dashed.stroke_width());
        assert!(solid.stroke_alpha() > dashed.stroke_alpha());
        assert!(solid.dash().is_empty());
        assert_eq!(dashed.dash(), DASH.as_slice());
    }

    #[test]
    fn isolated_node_is_kept() {
        let personas = vec![persona("alone", 0.5)];
        let g = TrustGraph::from_pack(&personas, &[]);
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.edges.len(), 0);
        assert_eq!(g.readout(0), "alone");
        assert_eq!(g.nodes[0].x, 0.5);
        assert_eq!(g.nodes[0].y, 0.5);
    }

    #[test]
    fn scoped_vs_unscoped_edge() {
        let personas = vec![persona("a", 0.5), persona("b", 0.5)];
        let rows = vec![trust("a", "b", 0.5, &[]), trust("b", "a", 0.5, &["seat"])];
        let g = TrustGraph::from_pack(&personas, &rows);
        let unscoped = g.edges.iter().find(|e| e.from == "a").unwrap();
        let scoped = g.edges.iter().find(|e| e.from == "b").unwrap();
        assert!(!unscoped.dashed());
        assert!(scoped.dashed());
        assert_eq!(unscoped.readout(), "a → b  0.500");
        assert_eq!(scoped.readout(), "b → a  0.500  seat");
        assert!(g.readout(0).contains("a → b  0.500"));
    }

    #[test]
    fn layout_is_stable_for_a_fixed_seed() {
        let personas = vec![
            persona("alice", 0.4),
            persona("bob", 0.8),
            persona("carol", 0.1),
        ];
        let rows = vec![
            trust("alice", "bob", 0.7, &[]),
            trust("bob", "carol", 0.3, &["hud"]),
        ];
        let a = TrustGraph::from_pack_seeded(&personas, &rows, 42);
        let b = TrustGraph::from_pack_seeded(&personas, &rows, 42);
        let pos = |g: &TrustGraph| {
            g.nodes
                .iter()
                .map(|n| (n.name.clone(), n.x, n.y))
                .collect::<Vec<_>>()
        };
        assert_eq!(pos(&a), pos(&b));
        let c = TrustGraph::from_pack_seeded(&personas, &rows, 99);
        assert_ne!(pos(&a), pos(&c), "a different seed moves the drawing");
        assert_eq!(a.edge_key(), b.edge_key());
        let mut kept = a.clone();
        kept.replace_if_edge_set_changed(b.clone());
        assert_eq!(pos(&kept), pos(&a), "same edge set must not relayout");
    }

    #[test]
    fn empty_pack_is_not_a_fake_graph() {
        let g = TrustGraph::from_pack(&[], &[]);
        assert!(g.nodes.is_empty());
        assert!(g.edges.is_empty());
        assert!(g.readout(0).is_empty());
    }
}
