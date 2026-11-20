//! `ljos file --from` files every row of a JSONL or CSV file in one
//! tracker commit, or none of them.

ljos_test_boot::harness!();

#[path = "common/deed_world.rs"]
mod deed_world;

use deed_world::{answer, mcp, said, stderr, stdout, World, ISSUE};

fn jsonl(rows: usize) -> String {
    (1..=rows)
        .map(|n| {
            serde_json::json!({"title": format!("Row {n}"), "body": format!("score {n}"), "tags": ["score"]})
                .to_string()
                + "\n"
        })
        .collect()
}

/// The tracker files without vissue's change log, which records the rows
/// written and the undo as writes and is never rewound.
fn tracked(world: &World) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    world
        .tracker_files()
        .into_iter()
        .filter(|(path, _)| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".vault-events"))
        })
        .collect()
}

fn headings(world: &World) -> usize {
    std::fs::read_to_string(world.tracker().join("Software/demo/issues.org"))
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("* ") || line.starts_with("** "))
        .count()
}

#[test]
fn a_hundred_and_fifty_rows_make_one_tracker_commit() {
    let world = World::new();
    let before = world.commits();
    let headed = headings(&world);
    let file = world.write("rows.jsonl", jsonl(150));
    let filed = world.ljos(&["file", "--from", &file, "-p", "demo", "--priority", "C"]);
    assert!(filed.status.success(), "{}", said(&filed));
    let ids: Vec<String> = stdout(&filed).lines().map(str::to_string).collect();
    assert_eq!(ids.len(), 150, "{}", said(&filed));
    assert!(ids.iter().all(|id| id.starts_with("demo-")), "{ids:?}");
    assert!(
        stderr(&filed).contains("filed 150 tickets in demo"),
        "{}",
        said(&filed)
    );
    assert_eq!(world.commits(), before + 1, "{}", said(&filed));
    assert_eq!(headings(&world), headed + 150);
    let subject = world.git(&["log", "-1", "--format=%s"]);
    assert_eq!(
        subject.trim(),
        format!("chore(issues): {} filed with 149 more by alice", ids[0])
    );
    let body = world.git(&["log", "-1", "--format=%b"]);
    assert!(ids.iter().all(|id| body.contains(id.as_str())), "{body}");
    let org = std::fs::read_to_string(world.tracker().join("Software/demo/issues.org")).unwrap();
    assert!(org.contains("* TODO [#C] Row 150"), "{org}");
}

#[test]
fn a_csv_with_parents_files_children_in_one_commit() {
    let world = World::new();
    let before = world.commits();
    let file = world.write(
        "rows.csv",
        format!("title,parent,priority\nFirst,{ISSUE},A\n\"Second, quoted\",{ISSUE},\n"),
    );
    let filed = world.ljos(&["file", "--from", &file]);
    assert!(filed.status.success(), "{}", said(&filed));
    assert_eq!(stdout(&filed).lines().count(), 2, "{}", said(&filed));
    assert_eq!(world.commits(), before + 1);
    let org = std::fs::read_to_string(world.tracker().join("Software/demo/issues.org")).unwrap();
    assert!(org.contains("* TODO [#A] First\n"), "{org}");
    assert!(org.contains("* TODO [#C] Second, quoted\n"), "{org}");
    assert_eq!(
        org.matches(&format!(":PARENT:     {ISSUE}")).count(),
        2,
        "{org}"
    );
}

#[test]
fn a_bad_row_leaves_the_tracker_unchanged() {
    let world = World::new();
    let before = world.commits();
    let files = tracked(&world);

    let mut rows = jsonl(149);
    rows.push_str("{\"title\": \"Row 150\", \"priority\": \"Z\"}\n");
    let file = world.write("bad-priority.jsonl", rows);
    let filed = world.ljos(&["file", "--from", &file, "-p", "demo"]);
    assert_eq!(filed.status.code(), Some(1), "{}", said(&filed));
    assert!(stdout(&filed).is_empty(), "{}", said(&filed));
    assert!(
        stderr(&filed).contains("row 150: priority \"Z\" is not A, B or C"),
        "{}",
        said(&filed)
    );
    assert_eq!(tracked(&world), files);
    assert_eq!(world.commits(), before);

    let mut rows = jsonl(149);
    rows.push_str("{\"title\": \"Row 150\", \"parent\": \"demo-0000\"}\n");
    let file = world.write("bad-parent.jsonl", rows);
    let filed = world.ljos(&["file", "--from", &file, "-p", "demo"]);
    assert_eq!(filed.status.code(), Some(1), "{}", said(&filed));
    assert!(stdout(&filed).is_empty(), "{}", said(&filed));
    assert!(
        stderr(&filed).contains("row 150: --parent demo-0000"),
        "{}",
        said(&filed)
    );
    assert!(
        stderr(&filed)
            .contains("nothing was filed: the 149 tickets written before it were taken back"),
        "{}",
        said(&filed)
    );
    assert_eq!(tracked(&world), files);
    assert_eq!(world.commits(), before);
    assert_eq!(world.git(&["diff", "--stat", "HEAD"]), "");
}

#[test]
fn the_mcp_tool_files_rows_in_one_commit() {
    let world = World::new();
    let before = world.commits();
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
    "name": "ljos_file_from", "arguments": {
        "seat": "alice",
        "project": "demo",
        "rows": [{"title": "From MCP one"}, {"title": "From MCP two", "tags": "x,y"}],
    }}});
    let (failed, text) = answer(&mcp(&world, &[call]), 1);
    assert!(!failed, "{text}");
    assert_eq!(text.matches("demo-").count(), 2, "{text}");
    assert_eq!(world.commits(), before + 1, "{text}");
    let org = std::fs::read_to_string(world.tracker().join("Software/demo/issues.org")).unwrap();
    assert!(
        org.contains("From MCP one") && org.contains("From MCP two"),
        "{org}"
    );
}
