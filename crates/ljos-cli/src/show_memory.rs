//! `ljos show` for one memory.
//!
//! A ticket id still opens the ticket. A memory id opens the memory: the
//! full atom id, the short prefix `ljos search` and `ljos remember` print,
//! or the `local:` form. An id that starts more than one memory lists
//! those ids. An id that is neither names `ljos search`.

use crate::output_wrap::Width;
use anyhow::{bail, Result};
use serde_json::Value;

/// Open `id` as a ticket or a memory.
///
/// A ticket page prints the body and the newest notes. `all` prints every
/// note. `--json` prints the body and every note either way.
///
/// # Errors
///
/// The id is empty, it matches more than one ticket or memory, it is
/// neither, or the tracker cannot be read.
pub fn show_any(id: &str, json: bool, all: bool, width: Width) -> Result<String> {
    let id = id.trim();
    if id.is_empty() {
        bail!("The issue id is empty. `ljos ready` lists the open tickets.");
    }
    if memory_shaped(id) {
        let atoms = gather();
        let hits = matching(&atoms, id);
        match hits.len() {
            1 => return Ok(render(&atoms, hits[0], json, width)),
            n if n > 1 => {
                let ids: Vec<String> = hits
                    .iter()
                    .map(|&index| field(&atoms[index], "id"))
                    .collect();
                bail!("{}", ambiguous(id, &ids));
            }
            _ => {}
        }
    }
    show_ticket_or_neither(id, json, all, width)
}

/// Whether `err` is a show lookup the CLI prints as one line and exits 1.
#[must_use]
pub fn is_show_fault(err: &anyhow::Error) -> bool {
    let text = err.to_string();
    crate::is_ticket_fault(err)
        || text.starts_with("memory ")
        || text.starts_with("The issue id is empty.")
}

fn show_ticket(id: &str, json: bool, all: bool, width: Width) -> Result<String> {
    if json {
        crate::show_json(id)
    } else {
        crate::show_issue(id, all, width)
    }
}

fn show_ticket_or_neither(id: &str, json: bool, all: bool, width: Width) -> Result<String> {
    if let Some(line) = crate::ticket_refusal(id) {
        if plain_miss(&line) {
            bail!("{}", neither(id));
        }
        bail!("{line}");
    }
    match show_ticket(id, json, all, width) {
        Ok(text) => Ok(text),
        Err(err) if plain_miss(&err.to_string()) => {
            if let Some(line) = crate::tracker_place::missing_tracker_line() {
                bail!("{line}");
            }
            bail!("{}", neither(id));
        }
        Err(err) => Err(err),
    }
}

fn plain_miss(line: &str) -> bool {
    let line = line.trim();
    line.contains("not found")
        && !line.contains("closest")
        && !line.contains("matches")
        && !line.contains("neither a ticket")
}

fn neither(id: &str) -> String {
    format!(
        "issue {id} not found. It is neither a ticket nor a memory. `ljos search` finds a memory. `ljos ready` lists the open tickets."
    )
}

fn ambiguous(input: &str, ids: &[String]) -> String {
    let shown = match ids {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        many => {
            let (last, rest) = many.split_last().unwrap_or((&many[0], &[]));
            format!(
                "{} and {last}",
                rest.iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    };
    let first = ids.first().map(String::as_str).unwrap_or(input);
    format!("memory {input} matches {shown}, so run `ljos show {first}`.")
}

/// Hex, a UUID, or a `local:` id. A ticket id such as `proj-1a2b` is not.
fn memory_shaped(id: &str) -> bool {
    let id = id.trim();
    if id.starts_with("local:") {
        return true;
    }
    let hexes = id.chars().filter(|ch| ch.is_ascii_hexdigit()).count();
    hexes >= 6
        && id
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-' || ch == ':')
}

fn gather() -> Vec<Value> {
    let mut atoms = pack_atoms();
    atoms.retain(|atom| !field(atom, "id").is_empty());
    for copy in crate::memory_port::show_copies() {
        let id = field(&copy, "id");
        if id.is_empty() {
            continue;
        }
        if let Some(existing) = atoms.iter_mut().find(|atom| field(atom, "id") == id) {
            if field(existing, "text").trim().is_empty() && !field(&copy, "text").trim().is_empty()
            {
                if let Some(object) = existing.as_object_mut() {
                    object.insert("text".into(), copy["text"].clone());
                }
            }
            continue;
        }
        atoms.push(copy);
    }
    atoms
}

fn pack_atoms() -> Vec<Value> {
    let Ok(client) = crate::pack() else {
        return Vec::new();
    };
    crate::atoms_lean(&client, &client.workspace()).unwrap_or_default()
}

fn matching(atoms: &[Value], input: &str) -> Vec<usize> {
    let mut hits: Vec<usize> = atoms
        .iter()
        .enumerate()
        .filter(|(_, atom)| id_matches(input, &field(atom, "id")))
        .map(|(index, _)| index)
        .collect();
    hits.sort_by(|&left, &right| field(&atoms[left], "id").cmp(&field(&atoms[right], "id")));
    hits.dedup_by(|&mut left, &mut right| field(&atoms[left], "id") == field(&atoms[right], "id"));
    hits
}

/// Exact id, a prefix of at least six characters, or the hex tail `ljos search` prints.
fn id_matches(input: &str, id: &str) -> bool {
    let input = input.trim();
    let id = id.trim();
    if input.is_empty() || id.is_empty() {
        return false;
    }
    if id == input {
        return true;
    }
    if input.chars().count() < 6 {
        return false;
    }
    if id.starts_with(input) {
        return true;
    }
    id.rsplit_once(':')
        .is_some_and(|(_, tail)| !tail.is_empty() && tail.starts_with(input))
}

fn render(atoms: &[Value], index: usize, json: bool, width: Width) -> String {
    let atom = &atoms[index];
    let id = field(atom, "id");
    let view = View {
        id: id.clone(),
        text: field(atom, "text"),
        kind: kind_of(atom),
        level: level_of(atom),
        origin: field(atom, "origin"),
        seat: seat_of(atom),
        session: source_field(atom, "session"),
        written: written_raw(atom),
        accepted: accepted(atom),
        retired: retired(atom),
        superseded_by: superseded_by(&id, atoms),
        closed: closed_window(atom),
        tickets: tickets_of(atom),
        links: links_of(atom),
    };
    if json {
        render_json(&view)
    } else {
        render_page(&view, width)
    }
}

struct View {
    id: String,
    text: String,
    kind: String,
    level: String,
    origin: String,
    seat: String,
    session: String,
    written: String,
    accepted: bool,
    retired: bool,
    superseded_by: Vec<String>,
    closed: bool,
    tickets: Vec<String>,
    links: Vec<String>,
}

fn render_page(view: &View, width: Width) -> String {
    let mut out = crate::scan::wrap_for(&view.id, width);
    out.push('\n');
    let text = view.text.trim();
    if !text.is_empty() {
        out.push_str(&crate::scan::wrap_for(text, width));
        out.push('\n');
    }
    out.push('\n');
    let written = if view.written.is_empty() {
        "none".to_string()
    } else {
        crate::local_when(&view.written)
    };
    let superseded = if view.superseded_by.is_empty() && view.closed {
        "superseded: yes".to_string()
    } else {
        format!("superseded by: {}", list_or_none(&view.superseded_by))
    };
    for line in [
        format!("kind: {}", or_none(&view.kind)),
        format!("level: {}", or_none(&view.level)),
        format!("origin: {}", or_none(&view.origin)),
        format!("seat: {}", or_none(&view.seat)),
        format!("session: {}", or_none(&view.session)),
        format!("written: {written}"),
        format!("accepted: {}", yes_no(view.accepted)),
        format!("retired: {}", yes_no(view.retired)),
        superseded,
        format!("tickets: {}", list_or_none(&view.tickets)),
        format!("links: {}", list_or_none(&view.links)),
    ] {
        out.push_str(&crate::scan::wrap_for(&line, width));
        out.push('\n');
    }
    out
}

fn render_json(view: &View) -> String {
    let doc = serde_json::json!({
        "id": view.id,
        "text": view.text,
        "kind": view.kind,
        "level": view.level,
        "origin": view.origin,
        "seat": view.seat,
        "session": view.session,
        "written": view.written,
        "accepted": view.accepted,
        "retired": view.retired,
        "superseded": !view.superseded_by.is_empty() || view.closed,
        "superseded_by": view.superseded_by,
        "tickets": view.tickets,
        "links": view.links,
    });
    format!("{doc}\n")
}

fn or_none(value: &str) -> &str {
    if value.trim().is_empty() {
        "none"
    } else {
        value.trim()
    }
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

fn yes_no(flag: bool) -> &'static str {
    if flag {
        "yes"
    } else {
        "no"
    }
}

fn field(atom: &Value, key: &str) -> String {
    atom.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn source_field(atom: &Value, key: &str) -> String {
    atom.get("source")
        .and_then(|source| source.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn kind_of(atom: &Value) -> String {
    field(atom, "kind")
}

fn level_of(atom: &Value) -> String {
    let level = field(atom, "level");
    if !level.is_empty() {
        return level;
    }
    let origin = field(atom, "origin");
    if origin.is_empty() {
        return String::new();
    }
    crate::admit::admission_level(&origin).to_string()
}

fn seat_of(atom: &Value) -> String {
    let named = source_field(atom, "seat");
    if !named.is_empty() {
        return named;
    }
    atom.get("entities")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                let rest = item.as_str()?.strip_prefix("seat:")?;
                let rest = rest.trim();
                if rest.is_empty()
                    || rest == "unresolved"
                    || rest.starts_with("import")
                    || rest.starts_with("iid:")
                    || rest.starts_with("label:")
                {
                    None
                } else {
                    Some(rest.to_string())
                }
            })
        })
        .unwrap_or_default()
}

fn written_raw(atom: &Value) -> String {
    for key in ["at", "ts"] {
        let value = source_field(atom, key);
        if !value.is_empty() {
            return value;
        }
    }
    for key in ["ts", "created_at"] {
        let value = field(atom, key);
        if !value.is_empty() {
            return value;
        }
    }
    String::new()
}

fn accepted(atom: &Value) -> bool {
    if field(atom, "level") == crate::admit::LEVEL_EXPLICIT {
        return true;
    }
    let id = field(atom, "id");
    if crate::memories::reviews()
        .get(&id)
        .is_some_and(|review| review.verdict == "keep")
    {
        return true;
    }
    field(atom, "level").is_empty() && field(atom, "origin") == crate::admit::ORIGIN_USER
}

fn retired(atom: &Value) -> bool {
    atom.get("tombstone")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn closed_window(atom: &Value) -> bool {
    if field(atom, "valid_to").is_empty() {
        return false;
    }
    let Some(object) = atom.as_object() else {
        return false;
    };
    !packset_core::record::is_live(object, &crate::now_utc())
}

fn superseded_by(id: &str, atoms: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for atom in atoms {
        let other = field(atom, "id");
        if other.is_empty() || other == id {
            continue;
        }
        if names_supersedes(atom, id) {
            out.push(other);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn names_supersedes(atom: &Value, id: &str) -> bool {
    match atom.get("supersedes") {
        Some(Value::Array(items)) => items.iter().any(|item| link_text(item) == id),
        Some(Value::String(text)) => text.trim() == id,
        _ => false,
    }
}

fn tickets_of(atom: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(items) = atom.get("entities").and_then(Value::as_array) {
        for item in items {
            let Some(ticket) = item
                .as_str()
                .and_then(|tag| tag.strip_prefix("issue:"))
                .map(str::trim)
                .filter(|ticket| !ticket.is_empty())
            else {
                continue;
            };
            let ticket = ticket.to_string();
            if !out.contains(&ticket) {
                out.push(ticket);
            }
        }
    }
    let issue = field(atom, "issue");
    if !issue.is_empty() && !out.contains(&issue) {
        out.push(issue);
    }
    out.sort();
    out.dedup();
    out
}

fn links_of(atom: &Value) -> Vec<String> {
    let Some(items) = atom.get("links").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let id = link_text(item);
        if !id.is_empty() && !out.contains(&id) {
            out.push(id);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn link_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Object(map) => map
            .get("to")
            .or_else(|| map.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ambiguous, id_matches, memory_shaped, render};
    use serde_json::{json, Value};

    fn atom(id: &str, text: &str, extra: Value) -> Value {
        let mut body = json!({
            "id": id,
            "kind": "lesson",
            "level": "explicit",
            "origin": "user-declared",
            "text": text,
            "entities": ["seat:alice", "issue:proj-1a2b"],
            "source": {"seat": "alice", "session": "sess-1", "at": "2026-11-01T12:00:00Z"},
            "links": ["bbbbbbbbcccccccccccccccccccccccccccccccccccccccccccccccccccccccc"]
        });
        if let (Some(object), Some(more)) = (body.as_object_mut(), extra.as_object()) {
            for (key, value) in more {
                object.insert(key.clone(), value.clone());
            }
        }
        body
    }

    #[test]
    fn a_ticket_id_is_not_shaped_like_a_memory() {
        let _env_read = crate::test_env_read_lock();
        assert!(!memory_shaped("proj-1a2b"));
        assert!(!memory_shaped("demo-none"));
        assert!(memory_shaped("01234567"));
        assert!(memory_shaped("local:f0ceee3d"));
        assert!(memory_shaped("0119a00a-1111-4222-8333-444444444444"));
    }

    #[test]
    fn each_id_form_names_one_memory() {
        let _env_read = crate::test_env_read_lock();
        let full = format!("{}{}", "01234567", "89abcdef".repeat(7));
        let local = format!("local:f0ceee3d{}", "ab".repeat(28));
        let atoms = vec![
            atom(&full, "The lexical default is BM25+.", json!({})),
            atom(&local, "Kept beside the pack.", json!({})),
        ];
        assert!(id_matches(&full, &full));
        assert!(id_matches("01234567", &full));
        assert!(id_matches("local:f0ceee3d", &local));
        assert!(id_matches("f0ceee3d", &local));
        assert!(!id_matches("01234567", &local));
        let page = render(&atoms, 0, false, crate::output_wrap::Width::FIXED);
        assert!(page.contains("kind: lesson"), "{page}");
        assert!(page.contains("level: explicit"), "{page}");
        assert!(page.contains("origin: user-declared"), "{page}");
        assert!(page.contains("seat: alice"), "{page}");
        assert!(page.contains("session: sess-1"), "{page}");
        assert!(page.contains("accepted: yes"), "{page}");
        assert!(page.contains("retired: no"), "{page}");
        assert!(page.contains("tickets: proj-1a2b"), "{page}");
        assert!(page.contains("links: bbbbbbbb"), "{page}");
        assert!(
            page.lines().all(|line| line.chars().count() <= 80),
            "{page}"
        );
    }

    #[test]
    fn a_long_lesson_wraps_once_to_the_terminal_width() {
        let _env_read = crate::test_env_read_lock();
        let text = "The kiln holds a long lesson about the lexical default and the fuse that beat BM25 by two points on the turns the panel measured, and the glaze log that the morning firing left behind on the shelf.";
        let atoms = vec![atom("0123456789abcdef", text, json!({}))];
        for cols in [40, 160] {
            let page = render(&atoms, 0, false, crate::output_wrap::Width::columns(cols));
            let once = crate::output_wrap::wrap_prose(text, Some(cols));
            assert!(page.contains(&format!("\n{once}\n")), "{cols}\n{page}");
        }
        assert!(
            crate::output_wrap::wrap_prose(text, Some(160))
                .lines()
                .count()
                == 2
        );
    }

    #[test]
    fn a_long_lesson_wraps_to_eighty_columns() {
        let _env_read = crate::test_env_read_lock();
        let text = "The kiln holds a long lesson about the lexical default and the fuse that beat BM25 by two points on the turns the panel measured.";
        let atoms = vec![atom("0123456789abcdef", text, json!({}))];
        let page = render(&atoms, 0, false, crate::output_wrap::Width::FIXED);
        assert!(text.len() > 80);
        assert!(page.contains("The kiln holds"), "{page}");
        assert!(
            page.lines().all(|line| line.chars().count() <= 80),
            "{page}"
        );
        assert!(
            page.lines().any(|line| line.contains("panel measured")),
            "{page}"
        );
    }

    #[test]
    fn a_retired_memory_stays_readable() {
        let _env_read = crate::test_env_read_lock();
        let id = format!("{}{}", "abcdef01", "23456789".repeat(7));
        let later = format!("{}{}", "abcdef02", "23456789".repeat(7));
        let atoms = vec![
            atom(
                &id,
                "The old fuse was Borda.",
                json!({"tombstone": true, "level": "derived", "origin": "agent-derived"}),
            ),
            atom(&later, "The fuse is CombMNZ.", json!({"supersedes": [id]})),
        ];
        let page = render(&atoms, 0, false, crate::output_wrap::Width::FIXED);
        assert!(page.contains("The old fuse was Borda."), "{page}");
        assert!(page.contains("accepted: no"), "{page}");
        assert!(page.contains("retired: yes"), "{page}");
        assert!(page.contains(&format!("superseded by: {later}")), "{page}");
        let body: Value =
            serde_json::from_str(&render(&atoms, 0, true, crate::output_wrap::Width::FIXED))
                .unwrap();
        assert_eq!(body["retired"], true);
        assert_eq!(body["accepted"], false);
        assert_eq!(body["superseded"], true);
        assert_eq!(body["superseded_by"][0], later);
        assert_eq!(body["text"], "The old fuse was Borda.");
        assert_eq!(body["kind"], "lesson");
        assert_eq!(body["seat"], "alice");
        assert_eq!(body["session"], "sess-1");
        assert_eq!(body["written"], "2026-11-01T12:00:00Z");
        assert_eq!(body["tickets"][0], "proj-1a2b");
    }

    #[test]
    fn an_ambiguous_prefix_names_each_candidate() {
        let _env_read = crate::test_env_read_lock();
        let left = format!("{}{}", "aaaaaaaa", "1".repeat(56));
        let right = format!("{}{}", "aaaaaaaa", "2".repeat(56));
        let line = ambiguous("aaaaaaaa", &[left.clone(), right.clone()]);
        assert!(line.contains(&left), "{line}");
        assert!(line.contains(&right), "{line}");
        assert!(line.contains("`ljos show"), "{line}");
    }
}
