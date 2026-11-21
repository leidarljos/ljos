//! `ljos show` prints the body and the notes, not only the header.
//!
//! A ticket filed with `--body` and two notes shows both, newest last,
//! each with its age and seat. Past eight notes the page names the rest.
//! `--all` and `--verbose` print every note. `--json` carries `body` and
//! `notes`.

ljos_test_boot::harness!();

#[path = "common/sibling.rs"]
mod sibling;

use std::process::{Command, Output};

struct Board {
    dir: tempfile::TempDir,
    root: std::path::PathBuf,
    home: std::path::PathBuf,
    path: String,
}

impl Board {
    fn new() -> Self {
        let vissue = sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"));
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        std::fs::write(root.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let path = format!("{}:/usr/bin:/bin", vissue.parent().unwrap().display());
        Self {
            dir,
            root,
            home,
            path,
        }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_HOOKD", "off")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.ljos(args);
        assert!(out.status.success(), "{args:?}\n{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

fn text(out: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

const BODY: &str = "The door needs a codec that seats when it is cold and the hinge is long enough to wrap past the terminal width a reader uses.";
const FIRST: &str = "the hinge was measured";
const SECOND: &str = "the door seats when it is cold";

#[test]
fn a_filed_body_and_two_notes_show_on_the_page() {
    let board = Board::new();
    let filed = board.ok(&[
        "file",
        "Pick a codec",
        "--top",
        "-p",
        "demo",
        "--body",
        BODY,
    ]);
    let id = filed.lines().next().unwrap().trim().to_string();
    assert!(id.starts_with("demo-"), "{filed}");

    board.ok(&["note", &id, FIRST]);
    board.ok(&["note", &id, SECOND]);

    let shown = board.ok(&["show", &id]);
    assert!(
        shown.lines().next().unwrap().contains(&id),
        "the header line is missing:\n{shown}"
    );
    assert!(shown.contains("blockers: none"), "{shown}");
    assert!(
        shown.contains(BODY) || body_words_in_order(&shown),
        "{shown}"
    );
    assert!(
        shown.lines().all(|line| line.chars().count() <= 80),
        "a line is wider than the terminal:\n{shown}"
    );
    let first_at = shown.find(FIRST).expect("first note");
    let second_at = shown.find(SECOND).expect("second note");
    assert!(first_at < second_at, "newest note is not last:\n{shown}");
    assert!(shown.contains("saddle"), "{shown}");
    assert!(
        shown.contains("today, saddle") || shown.contains("yesterday, saddle"),
        "{shown}"
    );
    assert!(!shown.contains("earlier note"), "{shown}");
    assert!(
        !shown.contains("Noted by"),
        "the seat stamp was printed as a note:\n{shown}"
    );

    let doc: serde_json::Value =
        serde_json::from_str(board.ok(&["show", &id, "--json"]).trim()).unwrap();
    assert_eq!(doc["body"].as_str(), Some(BODY));
    let notes = doc["notes"].as_array().expect("notes");
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(notes[0]["text"].as_str(), Some(FIRST));
    assert_eq!(notes[1]["text"].as_str(), Some(SECOND));
    assert_eq!(notes[0]["seat"].as_str(), Some("saddle"));
    assert_eq!(notes[1]["seat"].as_str(), Some("saddle"));
    assert!(notes[0]["age"].as_str().is_some());
    assert!(notes[1]["age"].as_str().is_some());

    for n in 3..=9 {
        board.ok(&["note", &id, &format!("note {n}")]);
    }
    let capped = board.ok(&["show", &id]);
    assert!(
        !capped.contains(FIRST),
        "the oldest note stayed on the capped page:\n{capped}"
    );
    assert!(capped.contains(SECOND), "{capped}");
    assert!(capped.contains("note 9"), "{capped}");
    assert!(
        capped.contains(&format!("1 earlier note; run `ljos show {id} --all`")),
        "{capped}"
    );
    let second_at = capped.find(SECOND).unwrap();
    let last_at = capped.find("note 9").unwrap();
    assert!(second_at < last_at, "newest note is not last:\n{capped}");

    for flag in ["--all", "--verbose"] {
        let full = board.ok(&["show", &id, flag]);
        assert!(full.contains(FIRST), "{flag}\n{full}");
        assert!(full.contains("note 9"), "{flag}\n{full}");
        assert!(!full.contains("earlier note"), "{flag}\n{full}");
        let first_at = full.find(FIRST).unwrap();
        let last_at = full.find("note 9").unwrap();
        assert!(first_at < last_at, "{flag}\n{full}");
    }

    let all: serde_json::Value =
        serde_json::from_str(board.ok(&["show", &id, "--json"]).trim()).unwrap();
    let notes = all["notes"].as_array().expect("notes");
    assert_eq!(notes.len(), 9, "{notes:?}");
    assert_eq!(notes[0]["text"].as_str(), Some(FIRST));
    assert_eq!(notes.last().unwrap()["text"].as_str(), Some("note 9"));
    assert_eq!(all["body"].as_str(), Some(BODY));

    board.ok(&["note", &id, "ballot-sig: saddle keep ab cd"]);
    let plain = board.ok(&["show", &id]);
    assert!(
        !plain.contains("ballot-sig"),
        "a signature line is not a note a person reads:\n{plain}"
    );
    assert!(plain.contains("note 9"), "{plain}");
    let full = board.ok(&["show", &id, "--all"]);
    assert!(full.contains("ballot-sig: saddle keep ab cd"), "{full}");
}

/// The body is wrapped, so the page may split it. The words stay in order.
fn body_words_in_order(shown: &str) -> bool {
    words_in_order(shown, BODY)
}

fn words_in_order(shown: &str, expected: &str) -> bool {
    let mut rest = shown;
    for word in expected.split_whitespace() {
        let Some(at) = rest.find(word) else {
            return false;
        };
        rest = &rest[at + word.len()..];
    }
    true
}

#[test]
fn a_full_ticket_shows_long_title_claim_family_body_and_notes() {
    let board = Board::new();
    let long_title = "Calibrate the north kiln damper sensor across cold winter mornings before firing the next stoneware glaze batch in workshop two";
    assert!(
        long_title.chars().count() >= 120,
        "title len {}",
        long_title.chars().count()
    );
    let multi_body = "First paragraph describes the cold damper sticking at sunrise when moisture condenses on the cast iron frame.\n\nSecond paragraph asks the holder to record three thermocouple readings before opening the main flue.";

    let parent_id = board
        .ok(&["file", "Kiln winter readiness", "--top", "-p", "demo"])
        .lines()
        .next()
        .unwrap()
        .trim()
        .to_string();
    let blocker_id = board
        .ok(&["file", "Replace worn flue gasket", "--top", "-p", "demo"])
        .lines()
        .next()
        .unwrap()
        .trim()
        .to_string();
    let id = board
        .ok(&[
            "file", long_title, "--parent", &parent_id, "--body", multi_body,
        ])
        .lines()
        .next()
        .unwrap()
        .trim()
        .to_string();
    let child_id = board
        .ok(&["file", "Log morning flue temperatures", "--parent", &id])
        .lines()
        .next()
        .unwrap()
        .trim()
        .to_string();
    board.ok(&["block", &id, "--on", &blocker_id]);
    board.ok(&["claim", &id, "--assignee", "saddle"]);

    let n1 = "checked the cast iron hinge pin for frost";
    let n2 = "measured three cold damper clearances";
    let n3 = "ready to fire once the flue gasket arrives";
    board.ok(&["note", &id, n1]);
    board.ok(&["note", &id, n2]);
    board.ok(&["note", &id, n3]);

    let shown = board.ok(&["show", &id]);
    assert!(
        shown.lines().all(|line| line.chars().count() <= 80),
        "a line is wider than 80 columns:\n{shown}"
    );
    assert!(shown.contains(&id), "{shown}");
    assert!(shown.contains("STARTED"), "{shown}");
    assert!(shown.contains("[#"), "{shown}");
    assert!(!shown.contains("..."), "title or text was cut:\n{shown}");
    assert!(
        words_in_order(&shown, long_title),
        "full title missing or cut:\n{shown}"
    );
    assert!(
        shown.contains("claimed ") && shown.contains(" by saddle"),
        "claim missing:\n{shown}"
    );
    assert!(
        shown.contains(&format!("parent: {parent_id}")) && shown.contains("Kiln winter readiness"),
        "parent missing:\n{shown}"
    );
    assert!(
        shown.contains("children:")
            && shown.contains(&child_id)
            && shown.contains("Log morning flue temperatures"),
        "children missing:\n{shown}"
    );
    assert!(
        shown.contains("blockers:") && shown.contains(&blocker_id) && shown.contains("open"),
        "blockers missing:\n{shown}"
    );
    assert!(
        words_in_order(&shown, multi_body),
        "multi-paragraph body missing:\n{shown}"
    );
    assert!(
        shown.contains("\n\nSecond paragraph"),
        "paragraph break was lost:\n{shown}"
    );
    let p1 = shown.find(n1).expect("note 1");
    let p2 = shown.find(n2).expect("note 2");
    let p3 = shown.find(n3).expect("note 3");
    assert!(p1 < p2 && p2 < p3, "notes out of order:\n{shown}");
    assert!(
        shown.contains("today, saddle") || shown.contains("yesterday, saddle"),
        "note age and seat missing:\n{shown}"
    );

    let doc: serde_json::Value =
        serde_json::from_str(board.ok(&["show", &id, "--json"]).trim()).unwrap();
    assert_eq!(doc["id"].as_str(), Some(id.as_str()));
    assert_eq!(doc["state"].as_str(), Some("STARTED"));
    assert_eq!(doc["title"].as_str(), Some(long_title));
    assert_eq!(doc["claim"]["seat"].as_str(), Some("saddle"));
    assert!(!doc["claim"]["age"].as_str().unwrap_or("").is_empty());
    assert_eq!(doc["parent"]["id"].as_str(), Some(parent_id.as_str()));
    assert_eq!(doc["children"][0]["id"].as_str(), Some(child_id.as_str()));
    assert_eq!(
        doc["blocked_by"][0]["id"].as_str(),
        Some(blocker_id.as_str())
    );
    assert_eq!(doc["body"].as_str(), Some(multi_body));
    let notes = doc["notes"].as_array().expect("notes");
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert_eq!(notes[0]["text"].as_str(), Some(n1));
    assert_eq!(notes[1]["text"].as_str(), Some(n2));
    assert_eq!(notes[2]["text"].as_str(), Some(n3));
    for note in notes {
        assert_eq!(note["seat"].as_str(), Some("saddle"));
        assert!(!note["age"].as_str().unwrap_or("").is_empty());
        assert!(!note["time"].as_str().unwrap_or("").is_empty());
    }
}
