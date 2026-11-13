//! Read a graphify `graph.json` and render a recall section.
//!
//! graphify writes `graphify-out/graph.json` as NetworkX node-link JSON.
//! `graph.schema_version` is the major. This reader accepts major 1, the
//! major `graphify/export.py` writes (`GRAPH_SCHEMA_VERSION`). Edges are
//! the `links` array, or `edges` when `links` is absent. Each edge carries
//! `confidence`: `EXTRACTED`, `INFERRED` or `AMBIGUOUS`. A node's line is
//! `source_location`, such as `L10`. Degree is how many edges touch the
//! node. The file does not store it.
//!
//! No graph file is an empty section. That path does not parse a file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

/// The graph.json major this ljos reads. graphify 0.9 writes 1.
pub const SUPPORTED_SCHEMA_MAJOR: u64 = 1;

/// How many matching nodes a recall lists.
const NODE_LIMIT: usize = 5;

/// How many edges a listed node shows.
const EDGE_LIMIT: usize = 3;

/// A query word shorter than this is dropped.
const MIN_WORD: usize = 3;

/// One graphify graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    /// Major from `graph.schema_version`.
    pub schema_major: u64,
    /// `graph.graphify_version` when the file carries one.
    pub graphify_version: Option<String>,
    /// Nodes, in file order.
    pub nodes: Vec<Node>,
    /// Edges from `links`, or from `edges` when `links` is absent.
    pub edges: Vec<Edge>,
}

/// One node. `degree` is counted from the edges.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Stable id.
    pub id: String,
    /// Human-readable name. The id when the file has no label.
    pub label: String,
    /// Source path, relative to the graphify scan root.
    pub source_file: String,
    /// Location text, such as `L10` or `§3.1`.
    pub source_location: String,
    /// Line from a location that starts with `L` and digits.
    pub line: Option<u32>,
    /// Community id. Absent when the file has none.
    pub community: Option<i64>,
    /// Edges that touch this node. A self-loop counts once.
    pub degree: u32,
}

/// One edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Source node id.
    pub source: String,
    /// Target node id.
    pub target: String,
    /// Verb, such as `calls` or `contains`.
    pub relation: String,
    /// `EXTRACTED`, `INFERRED`, `AMBIGUOUS`, or whatever the file wrote.
    pub confidence: String,
    /// `confidence_score` when it is a finite number.
    pub confidence_score: Option<f64>,
}

/// The code context section for `issue`, or empty when no graph is in use.
///
/// A file that is present but not a supported graph yields the refusal in
/// the section. Recall still returns the working set.
#[must_use]
pub fn recall_section(issue: &str) -> String {
    let Some(path) = located_graph() else {
        return String::new();
    };
    match read_graph(&path) {
        Ok(graph) => section(&graph, &recall_query(issue)),
        Err(err) => {
            let msg = format!("{err:#}").replace('\n', " ");
            format!("\nCode context\n  {msg}\n")
        }
    }
}

/// Read `path`. A leading `//` comment block is ignored so a fixture can
/// name its licence. graphify's own files start at `{`.
///
/// # Errors
///
/// The file cannot be read, the JSON is not a graph, `graph.schema_version`
/// is missing, or its major is not [`SUPPORTED_SCHEMA_MAJOR`].
pub fn read_graph(path: &Path) -> Result<Graph> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("{}: could not read graph.json", path.display()))?;
    parse_graph(json_body(&raw)).map_err(|err| anyhow::anyhow!("{}: {err}", path.display()))
}

/// The graph file in use, if one exists and graphify is not switched off.
///
/// `LJOS_GRAPHIFY=off` (also `0`, `false`, `no`) selects none.
/// `LJOS_GRAPHIFY` set to a path selects that file, when it is a file.
/// Otherwise the first `graphify = "..."` in `$LJOS_CONFIG`, `./ljos.toml`,
/// the seat's `ljos.toml`, then `~/.config/ljos/ljos.toml`. A relative path
/// is from that file's directory. `off` in the key selects none.
/// With no setting, the search walks from the working directory to the
/// repository root for `graphify-out/graph.json` and stops at `.git`.
#[must_use]
pub fn located_graph() -> Option<PathBuf> {
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match configured(&start) {
        Some(Setting::Off) => None,
        Some(Setting::Path(path)) => path.is_file().then_some(path),
        None => find_default_graph(&start),
    }
}

/// Matching nodes and their top edges, or empty when nothing matches.
#[must_use]
pub fn section(graph: &Graph, query: &str) -> String {
    let hits = matching(graph, query);
    if hits.is_empty() {
        return String::new();
    }
    let mut out = String::from("\nCode context\n");
    for node in hits {
        out.push_str("  ");
        out.push_str(&flat(&node.label));
        out.push_str("  ");
        out.push_str(&place(node));
        if let Some(community) = node.community {
            out.push_str("  community ");
            out.push_str(&community.to_string());
        }
        out.push_str("  degree ");
        out.push_str(&node.degree.to_string());
        out.push('\n');
        for edge in top_edges(&graph.edges, &node.id) {
            out.push_str("    ");
            out.push_str(&edge_line(graph, edge, &node.id));
            out.push('\n');
        }
    }
    out
}

fn recall_query(issue: &str) -> String {
    let issue = issue.trim();
    let mut parts = Vec::new();
    if !issue.is_empty() {
        parts.push(issue.to_string());
    }
    if let Ok(card) = crate::tracker_show_json(issue) {
        if let Some(title) = card
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty() && *title != issue)
        {
            parts.push(title.to_string());
        }
        if let Some(tags) = card.get("org_tags").and_then(Value::as_array) {
            for tag in tags {
                if let Some(tag) = tag.as_str().map(str::trim).filter(|tag| !tag.is_empty()) {
                    parts.push(tag.to_string());
                }
            }
        }
    }
    parts.join(" ")
}

fn matching<'a>(graph: &'a Graph, query: &str) -> Vec<&'a Node> {
    let raw = query.to_lowercase();
    let words = query_words(&raw);
    if words.is_empty() && raw.trim().is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(&Node, u32)> = graph
        .nodes
        .iter()
        .filter_map(|node| {
            let score = node_score(node, &words, &raw);
            (score > 0).then_some((node, score))
        })
        .collect();
    hits.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(b.0.degree.cmp(&a.0.degree))
            .then(a.0.label.cmp(&b.0.label))
            .then(a.0.id.cmp(&b.0.id))
    });
    hits.truncate(NODE_LIMIT);
    hits.into_iter().map(|(node, _)| node).collect()
}

fn node_score(node: &Node, words: &[String], raw: &str) -> u32 {
    let label = node.label.to_lowercase();
    let bare = label.trim_end_matches(['(', ')']).trim().to_string();
    let id = node.id.to_lowercase();
    let file = node.source_file.to_lowercase();
    let mut score = 0;
    if bare.chars().count() >= MIN_WORD && raw.contains(&bare) {
        score += 4;
    }
    for word in words {
        if bare == *word || id == *word {
            score += 3;
        } else if bare.contains(word) || id.contains(word) {
            score += 2;
        } else if file.contains(word) {
            score += 1;
        }
    }
    score
}

fn query_words(raw: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    for ch in raw.chars() {
        if ch.is_alphanumeric() {
            cur.push(ch);
        } else if !cur.is_empty() {
            push_word(&mut words, &mut cur);
        }
    }
    if !cur.is_empty() {
        push_word(&mut words, &mut cur);
    }
    words
}

fn push_word(words: &mut Vec<String>, cur: &mut String) {
    if cur.chars().count() >= MIN_WORD {
        words.push(std::mem::take(cur));
    } else {
        cur.clear();
    }
}

fn top_edges<'a>(edges: &'a [Edge], node_id: &str) -> Vec<&'a Edge> {
    let mut hit: Vec<&Edge> = edges
        .iter()
        .filter(|edge| edge.source == node_id || edge.target == node_id)
        .collect();
    hit.sort_by(|a, b| {
        confidence_rank(&a.confidence)
            .cmp(&confidence_rank(&b.confidence))
            .then_with(|| score_key(b.confidence_score).cmp(&score_key(a.confidence_score)))
            .then_with(|| a.relation.cmp(&b.relation))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.target.cmp(&b.target))
    });
    hit.truncate(EDGE_LIMIT);
    hit
}

fn confidence_rank(tag: &str) -> u8 {
    match tag {
        "EXTRACTED" => 0,
        "INFERRED" => 1,
        "AMBIGUOUS" => 2,
        _ => 3,
    }
}

/// Higher scores sort first. A missing score sorts last inside its tag.
fn score_key(score: Option<f64>) -> i64 {
    score
        .filter(|value| value.is_finite())
        .map(|value| (value * 1000.0).round() as i64)
        .unwrap_or(i64::MIN)
}

fn edge_line(graph: &Graph, edge: &Edge, node_id: &str) -> String {
    let (arrow, other) = if edge.target == node_id && edge.source != node_id {
        ("<--", edge.source.as_str())
    } else {
        ("-->", edge.target.as_str())
    };
    format!(
        "{arrow} {}  {}  [{}]",
        label_of(graph, other),
        flat(&edge.relation),
        flat(&edge.confidence)
    )
}

fn label_of(graph: &Graph, id: &str) -> String {
    let label = graph
        .nodes
        .iter()
        .find(|node| node.id == id)
        .map(|node| node.label.as_str())
        .unwrap_or(id);
    flat(label)
}

fn place(node: &Node) -> String {
    let file = flat(&node.source_file);
    if !file.is_empty() {
        if let Some(line) = node.line {
            return format!("{file}:{line}");
        }
        let location = flat(&node.source_location);
        if location.is_empty() {
            return file;
        }
        return format!("{file} {location}");
    }
    if let Some(line) = node.line {
        return line.to_string();
    }
    flat(&node.source_location)
}

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn line_number(location: &str) -> Option<u32> {
    let rest = location
        .strip_prefix('L')
        .or_else(|| location.strip_prefix('l'))?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn parse_graph(text: &str) -> Result<Graph, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|err| format!("not a graphify graph.json ({err})"))?;
    let obj = value
        .as_object()
        .ok_or_else(|| "not a graphify graph.json".to_string())?;
    let meta = obj.get("graph").and_then(Value::as_object);
    let version = meta
        .and_then(|graph| graph.get("schema_version"))
        .ok_or_else(|| {
            format!("has no graph.schema_version; this ljos reads major {SUPPORTED_SCHEMA_MAJOR}")
        })?;
    let schema_major = schema_major(version)?;
    let graphify_version = meta
        .and_then(|graph| graph.get("graphify_version"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    let nodes_value = obj
        .get("nodes")
        .ok_or_else(|| "has no nodes list".to_string())?;
    let nodes_value = nodes_value
        .as_array()
        .ok_or_else(|| "nodes is not a list".to_string())?;
    let mut nodes: Vec<Node> = nodes_value.iter().filter_map(parse_node).collect();
    let edge_key = if obj.contains_key("links") {
        "links"
    } else {
        "edges"
    };
    let edges = match obj.get(edge_key) {
        None => Vec::new(),
        Some(value) => {
            let list = value
                .as_array()
                .ok_or_else(|| format!("{edge_key} is not a list"))?;
            list.iter().filter_map(parse_edge).collect()
        }
    };
    assign_degree(&mut nodes, &edges);
    Ok(Graph {
        schema_major,
        graphify_version,
        nodes,
        edges,
    })
}

fn schema_major(value: &Value) -> Result<u64, String> {
    let major = match value {
        Value::Number(number) => {
            if let Some(major) = number.as_u64() {
                major
            } else if let Some(major) = number.as_i64() {
                if major < 0 {
                    return Err(unsupported(&format!("schema major {major}")));
                }
                u64::try_from(major).map_err(|_| unsupported("schema_version"))?
            } else if let Some(number) = number.as_f64() {
                if number.is_finite() && number >= 0.0 && number.fract() == 0.0 {
                    number as u64
                } else {
                    return Err(unsupported(&format!("schema_version {number}")));
                }
            } else {
                return Err(unsupported("schema_version"));
            }
        }
        Value::String(text) => {
            let head = text.split('.').next().unwrap_or("").trim();
            head.parse::<u64>()
                .map_err(|_| unsupported(&format!("schema_version {text:?}")))?
        }
        _ => return Err(unsupported("graph.schema_version")),
    };
    if major != SUPPORTED_SCHEMA_MAJOR {
        return Err(unsupported(&format!("schema major {major}")));
    }
    Ok(major)
}

fn unsupported(what: &str) -> String {
    format!("{what} is not supported; this ljos reads major {SUPPORTED_SCHEMA_MAJOR}")
}

fn parse_node(value: &Value) -> Option<Node> {
    let obj = value.as_object()?;
    let id = obj.get("id")?.as_str()?.trim();
    if id.is_empty() {
        return None;
    }
    let label = obj
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or(id)
        .to_string();
    let source_file = obj
        .get("source_file")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let source_location = obj
        .get("source_location")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let line = line_number(source_location.trim());
    let community = obj.get("community").and_then(json_i64);
    Some(Node {
        id: id.to_string(),
        label,
        source_file,
        source_location,
        line,
        community,
        degree: 0,
    })
}

fn parse_edge(value: &Value) -> Option<Edge> {
    let obj = value.as_object()?;
    let source = obj.get("source")?.as_str()?.trim().to_string();
    let target = obj.get("target")?.as_str()?.trim().to_string();
    if source.is_empty() || target.is_empty() {
        return None;
    }
    let relation = obj
        .get("relation")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let confidence = obj
        .get("confidence")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let confidence_score = obj
        .get("confidence_score")
        .and_then(Value::as_f64)
        .filter(|score| score.is_finite());
    Some(Edge {
        source,
        target,
        relation,
        confidence,
        confidence_score,
    })
}

fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

fn assign_degree(nodes: &mut [Node], edges: &[Edge]) {
    let mut degree: HashMap<&str, u32> = HashMap::new();
    for edge in edges {
        bump(&mut degree, &edge.source);
        if edge.source != edge.target {
            bump(&mut degree, &edge.target);
        }
    }
    for node in nodes {
        node.degree = degree.get(node.id.as_str()).copied().unwrap_or(0);
    }
}

fn bump<'a>(degree: &mut HashMap<&'a str, u32>, id: &'a str) {
    let entry = degree.entry(id).or_insert(0);
    *entry = entry.saturating_add(1);
}

/// Drop a UTF-8 BOM and leading `//` lines. The rest is the JSON text.
fn json_body(raw: &str) -> &str {
    let mut rest = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    loop {
        let trimmed = rest.trim_start();
        let Some(after) = trimmed.strip_prefix("//") else {
            return trimmed;
        };
        rest = match after.find('\n') {
            Some(index) => &after[index + 1..],
            None => "",
        };
    }
}

enum Setting {
    Off,
    Path(PathBuf),
}

fn configured(start: &Path) -> Option<Setting> {
    if let Some(value) = std::env::var("LJOS_GRAPHIFY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Some(resolve_setting(&value, start));
    }
    for path in config_files(start) {
        if let Some(value) = graphify_key(&path) {
            let base = path.parent().unwrap_or(start);
            return Some(resolve_setting(&value, base));
        }
    }
    None
}

fn resolve_setting(value: &str, base: &Path) -> Setting {
    let value = unquote(value.trim());
    if is_off(value) {
        Setting::Off
    } else {
        let path = PathBuf::from(value);
        let path = if path.is_absolute() {
            path
        } else {
            base.join(path)
        };
        Setting::Path(path)
    }
}

fn is_off(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "off" | "0" | "false" | "no"
    )
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let open = bytes[0];
        let close = bytes[bytes.len() - 1];
        if (open == b'"' && close == b'"') || (open == b'\'' && close == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn graphify_key(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix("graphify") else {
            continue;
        };
        let rest = rest.trim();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = unquote(rest.trim());
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }
    None
}

fn config_files(start: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("LJOS_CONFIG").filter(|value| !value.is_empty()) {
        out.push(PathBuf::from(path));
    }
    out.push(start.join("ljos.toml"));
    if let Some(base) = config_home() {
        if let Some(seat) = seat_name() {
            out.push(base.join("ljos").join("seats").join(seat).join("ljos.toml"));
        }
        out.push(base.join("ljos").join("ljos.toml"));
    }
    out
}

fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
}

fn seat_name() -> Option<String> {
    let name = std::env::var("LJOS_SEAT").ok()?;
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        None
    } else {
        Some(name.to_string())
    }
}

fn find_default_graph(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("graphify-out").join("graph.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join(".git").exists() {
            return None;
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graphify/graph.json")
    }

    fn fixture() -> Graph {
        read_graph(&fixture_path()).expect("fixture graph")
    }

    #[test]
    fn the_fixture_names_the_apache_origin() {
        let text = std::fs::read_to_string(fixture_path()).unwrap();
        assert!(text.starts_with("//"), "{text}");
        assert!(text.contains("Apache-2.0"), "{text}");
        assert!(text.contains("Graphify-Labs/graphify"), "{text}");
        assert!(
            text.contains("Safi Shamsi and the Graphify contributors"),
            "{text}"
        );
    }

    #[test]
    fn the_fixture_is_schema_major_one() {
        let graph = fixture();
        assert_eq!(graph.schema_major, 1);
        assert_eq!(graph.graphify_version.as_deref(), Some("0.9.108"));
        let attention = graph
            .nodes
            .iter()
            .find(|node| node.id == "n_attention")
            .expect("attention");
        assert_eq!(attention.label, "MultiHeadAttention");
        assert_eq!(attention.source_file, "model.py");
        assert_eq!(attention.line, Some(10));
        assert_eq!(attention.community, Some(0));
        assert_eq!(attention.degree, 2);
        let concept = graph
            .nodes
            .iter()
            .find(|node| node.id == "n_concept_attn")
            .expect("concept");
        assert_eq!(concept.line, None);
        assert_eq!(concept.source_location, "§3.1");
        assert_eq!(concept.community, Some(1));
        assert!(graph
            .edges
            .iter()
            .any(|edge| { edge.confidence == "EXTRACTED" && edge.relation == "contains" }));
        assert!(graph
            .edges
            .iter()
            .any(|edge| edge.confidence == "INFERRED" && edge.relation == "implements"));
    }

    #[test]
    fn a_section_lists_file_line_and_confidence_tags() {
        let graph = fixture();
        let text = section(&graph, "MultiHeadAttention");
        assert_eq!(
            text,
            "\nCode context\n  \
             MultiHeadAttention  model.py:10  community 0  degree 2\n    \
             <-- Transformer  contains  [EXTRACTED]\n    \
             --> attention mechanism  implements  [INFERRED]\n"
        );
        let layer = section(&graph, "LayerNorm");
        assert!(
            layer.contains("LayerNorm  model.py:20  community 0  degree 2"),
            "{layer}"
        );
        assert!(layer.contains("[EXTRACTED]"), "{layer}");
        assert!(layer.contains("[AMBIGUOUS]"), "{layer}");
        assert!(section(&graph, "quay schedule").is_empty());
    }

    #[test]
    fn schema_major_two_is_refused() {
        let err =
            parse_graph(r#"{"graph":{"schema_version":2},"nodes":[],"links":[]}"#).unwrap_err();
        assert!(err.contains("schema major 2 is not supported"), "{err}");
        assert!(err.contains("this ljos reads major 1"), "{err}");
    }

    #[test]
    fn a_missing_schema_version_is_refused() {
        let err = parse_graph(r#"{"graph":{},"nodes":[],"links":[]}"#).unwrap_err();
        assert!(err.contains("has no graph.schema_version"), "{err}");
        assert!(err.contains("this ljos reads major 1"), "{err}");
    }

    #[test]
    fn a_string_major_one_is_accepted_and_major_two_is_not() {
        let ok = parse_graph(
            r#"{"graph":{"schema_version":"1.2.0","graphify_version":"0.9.108"},"nodes":[{"id":"a","label":"Alpha","source_file":"a.rs","source_location":"L3"}],"edges":[{"source":"a","target":"a","relation":"calls","confidence":"EXTRACTED","confidence_score":1.0}]}"#,
        )
        .unwrap();
        assert_eq!(ok.schema_major, 1);
        assert_eq!(ok.nodes[0].line, Some(3));
        assert_eq!(ok.nodes[0].degree, 1);
        let text = section(&ok, "Alpha");
        assert!(text.contains("a.rs:3"), "{text}");
        assert!(text.contains("[EXTRACTED]"), "{text}");
        let err = parse_graph(r#"{"graph":{"schema_version":"2.0"},"nodes":[]}"#).unwrap_err();
        assert!(err.contains("schema major 2 is not supported"), "{err}");
    }

    #[test]
    fn a_non_integer_schema_version_is_refused() {
        let err = parse_graph(r#"{"graph":{"schema_version":1.5},"nodes":[]}"#).unwrap_err();
        assert!(err.contains("is not supported"), "{err}");
        assert!(err.contains("this ljos reads major 1"), "{err}");
    }

    #[test]
    fn three_edges_are_kept_and_extracted_ranks_first() {
        let graph = parse_graph(
            r#"{"graph":{"schema_version":1},"nodes":[
                {"id":"hub","label":"Hub","source_file":"hub.rs","source_location":"L1"},
                {"id":"a","label":"Aaa","source_file":"a.rs","source_location":"L1"},
                {"id":"b","label":"Bbb","source_file":"b.rs","source_location":"L1"},
                {"id":"c","label":"Ccc","source_file":"c.rs","source_location":"L1"},
                {"id":"d","label":"Ddd","source_file":"d.rs","source_location":"L1"}
            ],"links":[
                {"source":"hub","target":"a","relation":"uses","confidence":"INFERRED","confidence_score":0.95},
                {"source":"hub","target":"b","relation":"calls","confidence":"EXTRACTED","confidence_score":1},
                {"source":"hub","target":"c","relation":"imports","confidence":"EXTRACTED","confidence_score":1},
                {"source":"d","target":"hub","relation":"references","confidence":"EXTRACTED","confidence_score":1},
                {"source":"hub","target":"a","relation":"mentions","confidence":"AMBIGUOUS","confidence_score":0.2}
            ]}"#,
        )
        .unwrap();
        let text = section(&graph, "Hub");
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("    "))
            .collect();
        assert_eq!(
            lines,
            [
                "    --> Bbb  calls  [EXTRACTED]",
                "    --> Ccc  imports  [EXTRACTED]",
                "    <-- Ddd  references  [EXTRACTED]",
            ],
            "{text}"
        );
        assert!(!text.contains("INFERRED"), "{text}");
        assert_eq!(
            graph
                .nodes
                .iter()
                .find(|node| node.id == "hub")
                .unwrap()
                .degree,
            5
        );
    }

    #[test]
    fn the_default_search_stops_at_the_repository_root() {
        let outer = tempfile::tempdir().unwrap();
        let outer_out = outer.path().join("graphify-out");
        std::fs::create_dir(&outer_out).unwrap();
        std::fs::write(outer_out.join("graph.json"), "{}").unwrap();
        let repo = outer.path().join("repo");
        let nested = repo.join("src");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir(repo.join(".git")).unwrap();
        assert!(find_default_graph(&nested).is_none());
        let inner = repo.join("graphify-out");
        std::fs::create_dir(&inner).unwrap();
        let file = inner.join("graph.json");
        std::fs::write(&file, "{}").unwrap();
        assert_eq!(find_default_graph(&nested).unwrap(), file);
    }

    #[test]
    fn a_graphify_key_resolves_against_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let toml = dir.path().join("ljos.toml");
        std::fs::write(&toml, "graphify = \"custom/graph.json\"\n").unwrap();
        assert_eq!(graphify_key(&toml).as_deref(), Some("custom/graph.json"));
        let setting = resolve_setting(&graphify_key(&toml).unwrap(), dir.path());
        match setting {
            Setting::Path(path) => assert_eq!(path, dir.path().join("custom/graph.json")),
            Setting::Off => panic!("path"),
        }
        assert!(matches!(resolve_setting("off", dir.path()), Setting::Off));
    }
}
