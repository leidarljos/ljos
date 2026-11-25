//! `ljos table` keeps a CSV on a ticket as a deed, one tracker commit a
//! version.
//!
//! The same name again is the next version, and the older accession is
//! superseded by it. A CSV that is not a table is refused before anything
//! is stored or committed.

ljos_test_boot::harness!();

#[path = "common/deed_world.rs"]
mod deed_world;

use deed_world::{answer, mcp, said, stderr, stdout, World, ISSUE};

const V1: &str = "model,score\nalpha,0.91\nbeta,0.87\n";
const V2: &str = "model,score\nalpha,0.91\nbeta,0.89\ngamma,0.80\n";

#[test]
fn add_list_show_and_version_round_trip() {
    let world = World::new();
    let before = world.commits();
    let file = world.write("scores.csv", V1);
    let added = world.ljos(&["table", ISSUE, "--add", &file]);
    assert!(added.status.success(), "{}", said(&added));
    assert!(
        stdout(&added)
            .contains("table scores v1, 2 rows, 2 columns, deed-file-table-demo-a1b2-scores-v1"),
        "{}",
        said(&added)
    );
    assert_eq!(world.commits(), before + 1);

    let again = world.ljos(&["table", ISSUE, "--add", &file]);
    assert!(again.status.success(), "{}", said(&again));
    assert!(
        stdout(&again).contains("nothing written"),
        "{}",
        said(&again)
    );
    assert_eq!(world.commits(), before + 1);

    let file = world.write("scores.csv", V2);
    let added = world.ljos(&["table", ISSUE, "--add", &file, "--name", "scores"]);
    assert!(added.status.success(), "{}", said(&added));
    assert_eq!(world.commits(), before + 2);
    let subject = world.git(&["log", "-1", "--format=%s"]);
    assert_eq!(
        subject.trim(),
        format!("chore(issues): {ISSUE} added table scores v2 by alice")
    );

    let listed = world.ljos(&["table", ISSUE]);
    assert!(listed.status.success(), "{}", said(&listed));
    let row = stdout(&listed)
        .lines()
        .find(|line| line.starts_with("scores"))
        .unwrap_or_else(|| panic!("{}", said(&listed)))
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        row,
        [
            "scores",
            "3",
            "2",
            "2",
            "deed-file-table-demo-a1b2-scores-v2"
        ]
    );

    let latest = world.ljos(&["table", ISSUE, "--show", "scores"]);
    assert_eq!(stdout(&latest), V2, "{}", said(&latest));
    let older = world.ljos(&[
        "table",
        ISSUE,
        "--show",
        "scores",
        "--version",
        "1",
        "--csv",
    ]);
    assert_eq!(stdout(&older), V1, "{}", said(&older));
    let json = world.ljos(&["table", ISSUE, "--show", "scores", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(value["version"], 2);
    assert_eq!(value["columns"], serde_json::json!(["model", "score"]));
    assert_eq!(value["rows"][2], serde_json::json!(["gamma", "0.80"]));

    let current = world.ljos(&["current", "deed-file-table-demo-a1b2-scores-v1"]);
    assert!(
        stdout(&current).contains("id=deed-file-table-demo-a1b2-scores-v2"),
        "{}",
        said(&current)
    );
}

#[test]
fn a_csv_that_is_not_a_table_is_refused_and_nothing_is_written() {
    let world = World::new();
    let before = (world.commits(), world.tracker_files());
    for (name, bytes, line) in [
        (
            "ragged.csv",
            b"a,b\n1,2\n3\n".to_vec(),
            "line 3 has 1 columns; the header has 2",
        ),
        ("latin.csv", b"name\ncaf\xe9\n".to_vec(), "is not UTF-8"),
        (
            "empty.csv",
            Vec::new(),
            "is empty; a table needs a header row",
        ),
        (
            "quote.csv",
            b"a,b\n\"open,2\n".to_vec(),
            "opens a quoted field that is never closed",
        ),
        (
            "twice.csv",
            b"a,a\n1,2\n".to_vec(),
            "header names \"a\" twice",
        ),
    ] {
        let file = world.write(name, bytes);
        let added = world.ljos(&["table", ISSUE, "--add", &file]);
        assert_eq!(added.status.code(), Some(1), "{name}: {}", said(&added));
        assert!(stderr(&added).contains(line), "{name}: {}", said(&added));
        assert!(stderr(&added).contains(name), "{name}: {}", said(&added));
    }
    assert_eq!((world.commits(), world.tracker_files()), before);
    assert!(!world.store().join("bytes").exists());
}

#[test]
fn the_mcp_tool_adds_lists_and_shows() {
    let world = World::new();
    let call = |id: u64, arguments: serde_json::Value| {
        serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {
            "name": "ljos_table", "arguments": arguments}})
    };
    // The server runs calls side by side, so each waits for the one before.
    let one =
        |id: u64, arguments: serde_json::Value| answer(&mcp(&world, &[call(id, arguments)]), id);
    let added = one(
        1,
        serde_json::json!({"seat": "alice", "issue": ISSUE, "csv": V1, "name": "ranks"}),
    );
    let listed = one(2, serde_json::json!({"issue": ISSUE}));
    let shown = one(3, serde_json::json!({"issue": ISSUE, "show": "ranks"}));
    let (failed, text) = added;
    assert!(!failed, "{text}");
    assert!(text.contains("table ranks v1, 2 rows, 2 columns"), "{text}");
    let (failed, text) = listed;
    assert!(
        !failed && text.contains("deed-file-table-demo-a1b2-ranks-v1"),
        "{text}"
    );
    let (failed, text) = shown;
    assert!(!failed, "{text}");
    assert!(text.contains(&serde_json::json!(V1).to_string()), "{text}");
    assert_eq!(world.git(&["rev-list", "--count", "HEAD"]).trim(), "2");
}

#[test]
fn names_deedar_would_fold_together_are_separate_tables() {
    for (one, two) in [
        ("Scores", "scores"),
        ("a.b", "ab"),
        ("scores_v1", "scores-v1"),
    ] {
        let world = World::new();
        let first = world.write("first.csv", V1);
        let second = world.write("second.csv", V2);
        let added = world.ljos(&["table", ISSUE, "--add", &first, "--name", one]);
        assert!(added.status.success(), "{one}: {}", said(&added));
        let added = world.ljos(&["table", ISSUE, "--add", &second, "--name", two]);
        assert!(
            added.status.success(),
            "{two} after {one}: {}",
            said(&added)
        );

        let listed = world.ljos(&["table", ISSUE]);
        let names: Vec<String> = stdout(&listed)
            .lines()
            .skip_while(|line| !line.starts_with("NAME"))
            .skip(1)
            .filter_map(|line| line.split_whitespace().next().map(str::to_string))
            .collect();
        let mut want = vec![one.to_string(), two.to_string()];
        want.sort();
        assert_eq!(names, want, "{}", said(&listed));
        for (name, bytes) in [(one, V1), (two, V2)] {
            let shown = world.ljos(&["table", ISSUE, "--show", name]);
            assert!(shown.status.success(), "{name}: {}", said(&shown));
            assert_eq!(stdout(&shown), bytes, "{name}");
        }
    }
}

#[test]
fn an_id_another_deed_uses_is_refused_by_name() {
    let world = World::new();
    let other = world.write("other.csv", V2);
    let made = world
        .command("deedar")
        .args([
            "create",
            "file",
            "--id",
            "deed-file-table-demo-a1b2-scores-v1",
            "--name",
            "not a table",
            "--path",
            &other,
            "--agent",
            "alice",
        ])
        .output()
        .unwrap();
    assert!(made.status.success(), "{}", said(&made));
    let before = world.commits();
    let file = world.write("scores.csv", V1);
    let added = world.ljos(&["table", ISSUE, "--add", &file]);
    assert_eq!(added.status.code(), Some(1), "{}", said(&added));
    assert!(
        stderr(&added).contains(
            "would be stored as deed-file-table-demo-a1b2-scores-v1, which \"not a table\" already uses"
        ),
        "{}",
        said(&added)
    );
    assert_eq!(world.commits(), before);
}
