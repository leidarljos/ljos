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
/// written and the undo as writes and is never rewound, and without the
/// lock files, which are never removed.
fn tracked(world: &World) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    world
        .tracker_files()
        .into_iter()
        .filter(|(path, _)| {
            !path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with(".vault-events") || name.ends_with(".lock")
            })
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
fn a_refused_first_row_says_nothing_was_written_and_logs_no_write() {
    let world = World::new();
    let events = world.tracker().join("Software/.vault-events.jsonl");
    let log = || std::fs::read(&events).unwrap_or_default();
    let before = log();
    let files = tracked(&world);
    let file = world.write(
        "bad-first.jsonl",
        "{\"title\": \"Row 1\", \"parent\": \"demo-0000\"}\n{\"title\": \"Row 2\"}\n",
    );
    let filed = world.ljos(&["file", "--from", &file, "-p", "demo"]);
    assert_eq!(filed.status.code(), Some(1), "{}", said(&filed));
    assert!(
        stderr(&filed).contains("row 1: --parent demo-0000")
            && stderr(&filed).contains("; nothing was written"),
        "{}",
        said(&filed)
    );
    assert!(!stderr(&filed).contains("0 tickets"), "{}", said(&filed));
    assert_eq!(log(), before, "{}", String::from_utf8_lossy(&log()));
    assert_eq!(tracked(&world), files);
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

/// A second project with one ticket, committed.
fn other_project(world: &World) {
    let dir = world.tracker().join("Software/other");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("issues.org"),
        "* TODO [#B] Other plan\n:PROPERTIES:\n:ID:         other-c3d4\n:END:\n",
    )
    .unwrap();
    world.git(&["add", "."]);
    world.git(&["commit", "-q", "-m", "other"]);
}

#[test]
fn each_row_is_filed_in_its_parents_project() {
    let world = World::new();
    other_project(&world);
    let before = world.commits();
    let file = world.write(
        "rows.jsonl",
        format!(
            "{{\"title\": \"Under demo\", \"parent\": \"{ISSUE}\"}}\n{{\"title\": \"Under other\", \"parent\": \"other-c3d4\"}}\n"
        ),
    );
    let filed = world.ljos(&["file", "--from", &file]);
    assert!(filed.status.success(), "{}", said(&filed));
    let ids: Vec<String> = stdout(&filed).lines().map(str::to_string).collect();
    assert_eq!(ids.len(), 2, "{}", said(&filed));
    assert!(ids[0].starts_with("demo-"), "{ids:?}");
    assert!(ids[1].starts_with("other-"), "{ids:?}");
    assert!(
        stderr(&filed).contains("filed 2 tickets in demo, other"),
        "{}",
        said(&filed)
    );
    assert_eq!(world.commits(), before + 1);
    let demo = std::fs::read_to_string(world.tracker().join("Software/demo/issues.org")).unwrap();
    let other = std::fs::read_to_string(world.tracker().join("Software/other/issues.org")).unwrap();
    assert!(
        demo.contains("Under demo") && !demo.contains("Under other"),
        "{demo}"
    );
    assert!(other.contains("Under other"), "{other}");
    assert!(other.contains(":PARENT:     other-c3d4"), "{other}");
    let changed = world.git(&["show", "--name-only", "--format=", "HEAD"]);
    assert!(
        changed.contains("Software/demo/issues.org")
            && changed.contains("Software/other/issues.org"),
        "{changed}"
    );

    // `-p` does not move a row away from its parent's project, as with a
    // single `ljos file --parent`.
    let file = world.write(
        "pinned.jsonl",
        "{\"title\": \"Still under other\", \"parent\": \"other-c3d4\"}\n",
    );
    let filed = world.ljos(&["file", "--from", &file, "-p", "demo"]);
    assert!(filed.status.success(), "{}", said(&filed));
    assert!(stdout(&filed).starts_with("other-"), "{}", said(&filed));
    assert!(
        stderr(&filed).contains("filed in other, the project of its parent other-c3d4"),
        "{}",
        said(&filed)
    );
}

#[test]
fn a_refused_row_takes_back_every_project_it_wrote() {
    let world = World::new();
    other_project(&world);
    let before = world.commits();
    let files = tracked(&world);
    let file = world.write(
        "rows.jsonl",
        format!(
            "{{\"title\": \"Under demo\", \"parent\": \"{ISSUE}\"}}\n{{\"title\": \"Under other\", \"parent\": \"other-c3d4\"}}\n{{\"title\": \"Lost\", \"parent\": \"other-zzzz\"}}\n"
        ),
    );
    let filed = world.ljos(&["file", "--from", &file]);
    assert_eq!(filed.status.code(), Some(1), "{}", said(&filed));
    assert!(
        stderr(&filed).contains("row 3: --parent other-zzzz"),
        "{}",
        said(&filed)
    );
    assert_eq!(tracked(&world), files);
    assert_eq!(world.commits(), before);
}

#[test]
fn a_taken_back_batch_leaves_the_issues_lock_file() {
    let world = World::new();
    let lock = world.tracker().join("Software/demo/issues.org.lock");
    assert!(!lock.exists(), "{}", lock.display());
    let mut rows = jsonl(3);
    rows.push_str("{\"title\": \"Row 4\", \"parent\": \"demo-0000\"}\n");
    let file = world.write("bad-parent.jsonl", rows);
    let filed = world.ljos(&["file", "--from", &file, "-p", "demo"]);
    assert_eq!(filed.status.code(), Some(1), "{}", said(&filed));
    assert!(
        stderr(&filed)
            .contains("nothing was filed: the 3 tickets written before it were taken back"),
        "{}",
        said(&filed)
    );
    assert!(
        lock.is_file(),
        "a writer waiting on {} would lock a different file",
        lock.display()
    );
}

#[test]
fn a_commit_another_writer_holds_off_stamps_no_project() {
    use std::os::unix::io::AsRawFd as _;
    let world = World::new();
    other_project(&world);
    let held = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(world.tracker().join(".ljos-commit.lock"))
        .unwrap();
    // SAFETY: flock on a descriptor this test owns until it returns.
    assert_eq!(
        unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let before = world.commits();
    let file = world.write(
        "rows.jsonl",
        format!(
            "{{\"title\": \"Under demo\", \"parent\": \"{ISSUE}\"}}\n{{\"title\": \"Under other\", \"parent\": \"other-c3d4\"}}\n"
        ),
    );
    let filed = world
        .command(env!("CARGO_BIN_EXE_ljos"))
        .env("LJOS_TRACKER_LOCK_WAIT", "0")
        .args(["file", "--from", &file])
        .output()
        .unwrap();
    assert!(filed.status.success(), "{}", said(&filed));
    assert!(
        stderr(&filed).contains("so this write is on disk and not committed"),
        "{}",
        said(&filed)
    );
    assert_eq!(world.commits(), before);
    for project in ["demo", "other"] {
        let org = std::fs::read_to_string(
            world
                .tracker()
                .join(format!("Software/{project}/issues.org")),
        )
        .unwrap();
        assert!(!org.contains("Filed by"), "{project} was stamped: {org}");
    }
    drop(held);
}

#[test]
fn a_csv_header_named_twice_is_refused_and_nothing_is_filed() {
    let world = World::new();
    let before = world.commits();
    let files = tracked(&world);
    let file = world.write("rows.csv", "title,title \nkept,dropped\n");
    let filed = world.ljos(&["file", "--from", &file, "-p", "demo"]);
    assert_eq!(filed.status.code(), Some(1), "{}", said(&filed));
    assert!(stdout(&filed).is_empty(), "{}", said(&filed));
    assert!(
        stderr(&filed).contains("header columns 1 (\"title\") and 2 (\"title \") are both title"),
        "{}",
        said(&filed)
    );
    assert_eq!(tracked(&world), files);
    assert_eq!(world.commits(), before);
}

/// A hold record for alice that this test process, the parent of `ljos`,
/// matches.
fn hold(world: &World, issue: &str) {
    let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
    let runtime = world.dir().join("run/ljos");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(
        runtime.join("hold-alice"),
        format!(
            "alice\nalice\n{}\n{}\n2026-11-03T00:00:00Z\n{issue}\n-\n",
            std::process::id(),
            comm.trim()
        ),
    )
    .unwrap();
}

fn held_ljos(world: &World, args: &[&str]) -> std::process::Output {
    world
        .command(env!("CARGO_BIN_EXE_ljos"))
        .env("LJOS_HOLD", "alice")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn rows_with_no_parent_go_under_the_held_issue_as_ljos_file_does() {
    let world = World::new();
    other_project(&world);
    hold(&world, ISSUE);
    let rows = world.write(
        "rows.jsonl",
        "{\"title\": \"Held child\"}\n{\"title\": \"Own parent\", \"parent\": \"other-c3d4\"}\n",
    );

    let single = held_ljos(&world, &["file", "Single child"]);
    assert!(single.status.success(), "{}", said(&single));
    let filed = held_ljos(&world, &["file", "--from", &rows]);
    assert!(filed.status.success(), "{}", said(&filed));
    let ids: Vec<String> = stdout(&filed).lines().map(str::to_string).collect();
    assert!(
        ids[0].starts_with("demo-") && ids[1].starts_with("other-"),
        "{ids:?}"
    );
    assert!(
        stderr(&filed).contains(&format!(
            "filed under {ISSUE} in demo, the issue this conversation holds"
        )),
        "{}",
        said(&filed)
    );
    let demo = std::fs::read_to_string(world.tracker().join("Software/demo/issues.org")).unwrap();
    assert_eq!(
        demo.matches(&format!(":PARENT:     {ISSUE}")).count(),
        2,
        "the single child and the held row: {demo}"
    );

    let top = world.write("top.jsonl", "{\"title\": \"At the top\"}\n");
    let filed = held_ljos(&world, &["file", "--from", &top, "--top", "-p", "demo"]);
    assert!(filed.status.success(), "{}", said(&filed));
    let id = stdout(&filed).trim().to_string();
    let demo = std::fs::read_to_string(world.tracker().join("Software/demo/issues.org")).unwrap();
    let block = &demo[demo.find(&format!(":ID:         {id}")).unwrap()..];
    let block = &block[..block.find(":END:").unwrap()];
    assert!(!block.contains(":PARENT:"), "{demo}");

    let elsewhere = world.write("elsewhere.jsonl", "{\"title\": \"Tool bug\"}\n");
    let filed = held_ljos(&world, &["file", "--from", &elsewhere, "-p", "other"]);
    assert!(filed.status.success(), "{}", said(&filed));
    assert!(stdout(&filed).starts_with("other-"), "{}", said(&filed));
    assert!(
        stderr(&filed).contains(&format!("filed at the top of other, not under {ISSUE}")),
        "{}",
        said(&filed)
    );
}
