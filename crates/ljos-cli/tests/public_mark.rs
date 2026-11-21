//! `--public` marks a lesson, a preference or a rule as one the person chose
//! to publish. Nothing is marked unless it is asked for.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

#[path = "common/hand_world.rs"]
mod hand_world;

use hand_world::{dump, ready, World};

const MARK: &str = "visibility:public";

fn has(atom: &serde_json::Value, entity: &str) -> bool {
    atom["entities"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row == entity))
}

#[test]
fn remember_marks_a_lesson_public_only_when_asked() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&["remember", "-p", "kiln", "Keep the bench level."]);
    world.ok(&["remember", "-p", "kiln", "--public", "Wax the door hinge."]);
    world.ok(&["remember", "--public", "Oil the east latch."]);
    let plain = world
        .atom("lesson", "Keep the bench level.")
        .expect("plain");
    assert!(!has(&plain, MARK), "{plain}");
    let marked = world.atom("lesson", "Wax the door hinge.").expect("marked");
    assert!(has(&marked, MARK), "{marked}");
    assert!(has(&marked, "project:kiln"), "{marked}");
    let from_checkout = world
        .atom("lesson", "Oil the east latch.")
        .expect("checkout");
    assert!(has(&from_checkout, MARK), "{from_checkout}");
    assert!(has(&from_checkout, "project:kiln"), "{from_checkout}");
}

#[test]
fn prefer_and_rule_mark_a_policy_public_only_when_asked() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&["prefer", "-p", "kiln", "Keep the schedule written down."]);
    world.ok(&["prefer", "--public", "Fire the small pots first."]);
    world.ok(&[
        "rule",
        "cargo test *",
        "--verdict",
        "ask",
        "--why",
        "Run the suite first.",
        "--public",
    ]);
    world.ok(&[
        "prefer",
        "--user",
        "--public",
        "--why",
        "The bench asked for it",
        "Write the kiln notes in the lab book.",
    ]);
    world.ok(&[
        "prefer",
        "--user",
        "--why",
        "The bench asked for it",
        "Keep the glaze notes private.",
    ]);
    let plain = world
        .atom("preference", "Keep the schedule written down.")
        .expect("plain");
    assert!(!has(&plain, MARK), "{plain}");
    let marked = world
        .atom("preference", "Fire the small pots first.")
        .expect("marked");
    assert!(
        has(&marked, MARK) && has(&marked, "project:kiln"),
        "{marked}"
    );
    let rule = world
        .atoms()
        .into_iter()
        .find(|atom| atom["kind"] == "rule" && atom["pattern"] == "cargo test *")
        .expect("rule");
    assert!(has(&rule, MARK) && has(&rule, "project:kiln"), "{rule}");
    let rules = std::fs::read_to_string(world.home.path().join(".config/ljos/user-rules.json"))
        .expect("user rules");
    let rows: serde_json::Value = serde_json::from_str(&rules).expect("user rules are JSON");
    let rows = rows.as_array().cloned().unwrap_or_else(|| {
        rows["rules"]
            .as_array()
            .cloned()
            .unwrap_or_else(|| panic!("no rows in {rules}"))
    });
    let listed = |text: &str| {
        rows.iter()
            .find(|row| row["text"] == text)
            .unwrap_or_else(|| panic!("no row {text:?} in {rules}"))
            .clone()
    };
    assert!(has(&listed("Write the kiln notes in the lab book."), MARK));
    assert!(!has(&listed("Keep the glaze notes private."), MARK));
}

#[test]
fn a_review_marks_an_existing_lesson_and_an_edit_takes_the_mark_off() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--json",
        "Stack the shelves from the bottom.",
    ]);
    let old = world
        .atom("lesson", "Stack the shelves from the bottom.")
        .expect("stored");
    let id = old["id"].as_str().expect("id").to_string();
    assert!(!has(&old, MARK), "{old}");

    let said = world.ok(&["memories", "review", "--public", &id]);
    assert!(said.starts_with("marked "), "{said}");
    let marked = world
        .atom("lesson", "Stack the shelves from the bottom.")
        .expect("marked");
    assert_ne!(marked["id"], old["id"], "{marked}");
    assert_eq!(
        world
            .atoms()
            .iter()
            .filter(|atom| atom["kind"] == "lesson" && atom["tombstone"] != true)
            .count(),
        1,
        "the unmarked row is still live"
    );
    assert!(has(&marked, MARK), "{marked}");
    assert!(
        has(&marked, "project:kiln"),
        "the project was lost: {marked}"
    );

    let again = world.ok(&[
        "memories",
        "review",
        "--public",
        marked["id"].as_str().unwrap(),
    ]);
    assert!(again.contains("already public"), "{again}");

    // The pack already retires the row an edit replaces, and the command's
    // own retire then reports it missing. The row it wrote is what counts.
    world.ljos(&[
        "memories",
        "review",
        "--edit",
        marked["id"].as_str().unwrap(),
        "--text",
        "Stack the shelves from the bottom up.",
    ]);
    let after = world
        .atom("lesson", "Stack the shelves from the bottom up.")
        .expect("edited");
    assert!(!has(&after, MARK), "an edit kept the mark: {after}");
}

#[test]
fn a_runner_cannot_mark_a_memory_public() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&["remember", "-p", "kiln", "Hang the apron by the door."]);
    let stored = world
        .atom("lesson", "Hang the apron by the door.")
        .expect("stored");
    let id = stored["id"].as_str().expect("id");
    let out = world.ljos_with(
        &["memories", "review", "--public", id],
        &[("CLAUDECODE", "1")],
    );
    assert_eq!(out.status.code(), Some(1), "{}", dump(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--public is the person's to give"),
        "{}",
        dump(&out)
    );
    let still = world
        .atom("lesson", "Hang the apron by the door.")
        .expect("still stored");
    assert!(!has(&still, MARK), "{still}");
}

#[test]
fn a_runner_cannot_write_a_public_lesson_preference_or_rule() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    let runner = [("CLAUDECODE", "1")];
    let cases: [(&str, &[&str]); 5] = [
        (
            "remember",
            &[
                "remember",
                "--public",
                "-p",
                "kiln",
                "Shelve the jars by size.",
            ],
        ),
        (
            "prefer",
            &["prefer", "--public", "-p", "kiln", "Keep the jars sealed."],
        ),
        (
            "prefer",
            &[
                "prefer",
                "--user",
                "--public",
                "-p",
                "kiln",
                "--why",
                "The bench asked for it",
                "Label every jar.",
            ],
        ),
        (
            "rule",
            &[
                "rule",
                "--public",
                "-p",
                "kiln",
                "cargo test *",
                "--verdict",
                "ask",
                "--why",
                "Run the suite first.",
            ],
        ),
        (
            "rule",
            &[
                "rule",
                "--user",
                "--public",
                "-p",
                "kiln",
                "cargo build *",
                "--verdict",
                "ask",
                "--why",
                "Build in a clean tree.",
            ],
        ),
    ];
    for (command, args) in cases {
        let out = world.ljos_with(args, &runner);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", dump(&out));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains(&format!("{command}: --public is the person's to give")),
            "{args:?}: {}",
            dump(&out)
        );
    }
    let marked: Vec<_> = world
        .atoms()
        .into_iter()
        .filter(|atom| has(atom, MARK))
        .collect();
    assert!(marked.is_empty(), "a refused write left {marked:?}");
    let rules = world.home.path().join(".config/ljos/user-rules.json");
    let written = std::fs::read_to_string(rules).unwrap_or_default();
    assert!(!written.contains(MARK), "{written}");
}
