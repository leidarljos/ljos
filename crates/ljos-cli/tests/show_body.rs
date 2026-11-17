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
    let mut rest = shown;
    for word in BODY.split_whitespace() {
        let Some(at) = rest.find(word) else {
            return false;
        };
        rest = &rest[at + word.len()..];
    }
    true
}
