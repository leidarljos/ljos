//! `--public` marks a lesson, a preference or a rule as one the person chose
//! to publish. Nothing is marked unless it is asked for.

ljos_test_boot::harness!();

use std::process::Command;

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
fn a_public_mark_preserves_the_stored_claim_fields() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    let text = "Turn the kiln down after the last shelf.";
    world.post_atom(&serde_json::json!({
        "schema": "inside.atom/v1",
        "kind": "lesson",
        "level": "explicit",
        "text": text,
        "workspace": "hand",
        "issue": "kiln-42",
        "valid_to": "2027-01-02T03:04:05Z",
        "entities": [
            "project:kiln",
            "issue:kiln-42",
            "valid_to:2027-01-02T03:04:05Z",
            "horizon:transient"
        ],
        "source": {
            "seat": "saddle",
            "basis": "person",
            "host": "wheel",
            "session": "night"
        }
    }));
    let before = world.atom("lesson", text).expect("stored");
    let id = before["id"].as_str().unwrap();
    world.ok(&["memories", "review", "--public", id]);
    let after = world.atom("lesson", text).expect("marked");
    for field in ["issue", "valid_to"] {
        assert_eq!(after[field], before[field], "{field}: {after}");
    }
    assert_eq!(after["source"]["host"], "wheel", "{after}");
    assert_eq!(after["source"]["session"], "night", "{after}");
    for entity in [
        "project:kiln",
        "issue:kiln-42",
        "valid_to:2027-01-02T03:04:05Z",
        "horizon:transient",
        MARK,
    ] {
        assert!(has(&after, entity), "missing {entity}: {after}");
    }
    assert!(!has(&after, "horizon:standing"), "{after}");
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

    world.ok(&["remember", "-p", "kiln", "Fold the spare apron."]);
    let drop_id = world
        .atom("lesson", "Fold the spare apron.")
        .expect("drop candidate")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let public_id = still["id"].as_str().unwrap();
    let out = world.ljos_with(
        &[
            "memories", "review", "--drop", &drop_id, "--public", public_id,
        ],
        &[("CLAUDECODE", "1")],
    );
    assert_eq!(out.status.code(), Some(1), "{}", dump(&out));
    assert!(
        world.atom("lesson", "Fold the spare apron.").is_some(),
        "the drop ran before the public refusal: {}",
        dump(&out)
    );
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

#[test]
fn remember_public_marks_a_private_repeat() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    let text = "Keep the west latch oiled.";
    world.ok(&["remember", "-p", "kiln", text]);
    let said = world.ok(&["remember", "-p", "kiln", "--public", text]);
    assert!(!said.contains("reinforced"), "{said}");
    let marked = world.atom("lesson", text).expect("marked");
    assert!(has(&marked, MARK), "{marked}");
    assert!(has(&marked, "project:kiln"), "{marked}");
    let live = world
        .atoms()
        .iter()
        .filter(|atom| {
            atom["kind"] == "lesson" && atom["text"] == text && atom["tombstone"] != true
        })
        .count();
    assert_eq!(live, 1, "the private row stayed live");
}

/// The marked row says the public repeat's words, and no live row marked
/// public says the private wording.
fn published_as(world: &World, public: &str, private: &str) {
    let marked = world.atom("lesson", public).expect("marked");
    assert!(has(&marked, MARK), "{marked}");
    assert!(has(&marked, "project:kiln"), "{marked}");
    let leaked: Vec<_> = world
        .atoms()
        .into_iter()
        .filter(|atom| atom["tombstone"] != true && has(atom, MARK))
        .filter(|atom| atom.to_string().contains(private))
        .collect();
    assert!(
        leaked.is_empty(),
        "the private wording is public: {leaked:?}"
    );
}

#[test]
fn a_public_near_repeat_publishes_its_own_words() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    let private = "Always run all of the database migrations before you start the server.";
    let public = "Run the database migrations before the server starts.";
    assert!(ljos_cli::memories::same_lesson(public, private));
    world.ok(&["remember", "-p", "kiln", private]);
    let said = world.ok(&["remember", "-p", "kiln", "--public", public]);
    assert!(!said.contains("reinforced"), "{said}");
    published_as(&world, public, private);
}

#[test]
fn a_public_alias_repeat_publishes_its_own_words() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    let private = "Lock the glaze book in the office drawer.";
    let public = "Keep the glaze notes written down.";
    assert!(!ljos_cli::memories::same_lesson(public, private));
    world.post_atom(&serde_json::json!({
        "schema": "inside.atom/v1",
        "kind": "lesson",
        "level": "explicit",
        "text": private,
        "aliases": [public],
        "workspace": "hand",
        "entities": ["project:kiln"],
        "source": {"seat": "saddle", "basis": "person"}
    }));
    let held = world.atom("lesson", private).expect("stored");
    assert_eq!(held["aliases"], serde_json::json!([public]), "{held}");
    let said = world.ok(&["remember", "-p", "kiln", "--public", public]);
    assert!(!said.contains("reinforced"), "{said}");
    published_as(&world, public, private);
}

#[test]
fn a_public_repeat_for_another_project_is_refused_and_leaves_the_row() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ticket("other", "other-seed", "Seed the other project", None, "");
    let text = "Keep the north vent clear.";
    world.ok(&["remember", "-p", "kiln", text]);
    let out = world.ljos(&["remember", "-p", "other", "--public", text]);
    assert!(!out.status.success(), "{}", dump(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already names a project, so leave -p off"),
        "{}",
        dump(&out)
    );
    let row = world.atom("lesson", text).expect("row");
    assert!(!has(&row, MARK), "{row}");
    assert!(has(&row, "project:kiln"), "{row}");
    assert!(!has(&row, "project:other"), "{row}");
    let said = world.ok(&["remember", "-p", "kiln", "--public", text]);
    assert!(!said.contains("reinforced"), "{said}");
    let marked = world.atom("lesson", text).expect("marked");
    assert!(has(&marked, MARK), "{marked}");
}

#[test]
fn review_public_records_a_project_when_the_row_names_none() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&["remember", "Sweep the clay dust."]);
    let stored = world
        .atom("lesson", "Sweep the clay dust.")
        .expect("stored");
    assert!(
        !stored["entities"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| {
                row.as_str()
                    .is_some_and(|entity| entity.starts_with("project:"))
            })),
        "{stored}"
    );
    let id = stored["id"].as_str().expect("id");
    let said = world.ok(&["memories", "review", "--public", id]);
    assert!(said.starts_with("marked "), "{said}");
    let marked = world
        .atom("lesson", "Sweep the clay dust.")
        .expect("marked");
    assert!(has(&marked, MARK), "{marked}");
    assert!(has(&marked, "project:kiln"), "{marked}");

    world.ok(&["remember", "Rinse the boards."]);
    world.ticket("other", "other-o1", "Other work", None, "");
    let stored = world.atom("lesson", "Rinse the boards.").expect("stored");
    let id = stored["id"].as_str().expect("id").to_string();
    let said = world.ok(&["memories", "review", "--public", &id, "-p", "other"]);
    assert!(said.starts_with("marked "), "{said}");
    let marked = world.atom("lesson", "Rinse the boards.").expect("marked");
    assert!(has(&marked, "project:other"), "{marked}");
    assert!(!has(&marked, "project:kiln"), "{marked}");

    world.ok(&["remember", "-p", "kiln", "Stack the dry racks."]);
    let stored = world
        .atom("lesson", "Stack the dry racks.")
        .expect("stored");
    let id = stored["id"].as_str().expect("id");
    let out = world.ljos(&["memories", "review", "--public", id, "-p", "other"]);
    assert!(!out.status.success(), "{}", dump(&out));
    let said = dump(&out);
    assert!(said.contains("leave -p off"), "{said}");
    assert!(!said.contains("kiln"), "{said}");

    let bare = world.ljos(&["memories", "review", "-p", "kiln"]);
    assert!(!bare.status.success(), "{}", dump(&bare));
    assert!(
        dump(&bare).contains("belongs with --public"),
        "{}",
        dump(&bare)
    );
}

/// The library write the agent paths use. Inert unless a parent case started it.
#[test]
fn agent_public_repeat_child() {
    let Ok(which) = std::env::var("LJOS_AGENT_REPEAT_CHILD") else {
        return;
    };
    let text = std::env::var("LJOS_AGENT_REPEAT_TEXT").expect("text");
    let _public = ljos_cli::RememberPublic::set(which != "person-private");
    let _project = ljos_cli::RememberProject::set(Some("kiln".to_string()));
    let body = match which.as_str() {
        "agent" => ljos_cli::packset_write_agent("Remember", &text, None, Some(false)),
        "mcp" => ljos_cli::packset_write_mcp("Remember", &text, None, Some(false)),
        "person" | "person-private" => {
            ljos_cli::packset_write_as("Remember", &text, None, Some(false))
        }
        other => panic!("unknown path {other}"),
    }
    .unwrap_or_else(|err| panic!("{which}: {err:#}"));
    assert_ne!(body["reinforced"], serde_json::Value::Bool(true), "{body}");
}

#[test]
fn agent_writes_mark_a_private_repeat_public() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    let home = world.home.path();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let run_child = |which: &str, text: &str| {
        Command::new(std::env::current_exe().expect("test bin"))
            .args(["agent_public_repeat_child", "--exact", "--test-threads=1"])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", home)
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("PACKSET_PORT", world.port.to_string())
            .env("PACKSET_URL", format!("http://127.0.0.1:{}", world.port))
            .env("PACKSET_HOME", home.join(".local/share/packset"))
            .env("PACKSET_WORKSPACE", "hand")
            .env("PACKSET_AUTH", "off")
            .env("VISSUE_ROOT", &world.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_AGENT_REPEAT_CHILD", which)
            .env("LJOS_AGENT_REPEAT_TEXT", text)
            .current_dir(&world.root)
            .output()
            .expect("spawn repeat child")
    };
    for (which, text) in [
        ("agent", "Keep the north latch oiled."),
        ("mcp", "Keep the south latch oiled."),
        ("person", "Keep the west latch oiled."),
    ] {
        if which == "person" {
            let private = run_child("person-private", text);
            assert!(
                private.status.success(),
                "private person: {}",
                dump(&private)
            );
        } else {
            world.ok(&["remember", "-p", "kiln", text]);
        }
        let out = run_child(which, text);
        assert!(out.status.success(), "{which}: {}", dump(&out));
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(!said.contains("reinforced"), "{which}: {said}");
        let marked = world.atom("lesson", text).expect("marked");
        assert!(has(&marked, MARK), "{which}: {marked}");
        assert!(has(&marked, "project:kiln"), "{which}: {marked}");
        let live = world
            .atoms()
            .iter()
            .filter(|atom| {
                atom["kind"] == "lesson" && atom["text"] == text && atom["tombstone"] != true
            })
            .count();
        assert_eq!(live, 1, "{which}: the private row stayed live");
    }
}

#[test]
fn a_public_lesson_the_pack_did_not_take_is_not_queued() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    let down = [("PACKSET_URL", "http://127.0.0.1:9"), ("PACKSET_PORT", "9")];
    let queue = world.home.path().join(".local/state/ljos/note-queue.jsonl");
    let out = world.ljos_with(
        &[
            "remember",
            "-p",
            "kiln",
            "--public",
            "Oil the east hinge every week.",
        ],
        &down,
    );
    assert!(!out.status.success(), "{}", dump(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not queued"),
        "{}",
        dump(&out)
    );
    let rows = std::fs::read_to_string(&queue).unwrap_or_default();
    assert!(
        !rows.contains("east hinge"),
        "a public lesson was queued:\n{rows}"
    );
    world.ljos_with(
        &["remember", "-p", "kiln", "Sweep the west bench daily."],
        &down,
    );
    let rows = std::fs::read_to_string(&queue).unwrap_or_default();
    assert!(
        rows.contains("west bench"),
        "the private control was not queued:\n{rows}"
    );
    assert!(!rows.contains("east hinge"), "{rows}");
}
