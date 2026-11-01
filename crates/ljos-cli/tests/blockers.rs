//! A ticket can wait on another. The edge lives on the tracker, the ready
//! list leaves the waiting ticket out while that blocker is open, and a
//! loop is refused.

ljos_test_boot::harness!();

use std::process::{Command, Output};

struct Board {
    dir: tempfile::TempDir,
    root: std::path::PathBuf,
    home: std::path::PathBuf,
    a: String,
    b: String,
    c: String,
}

impl Board {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        let layout = vissue_core::Layout::new(&root, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        for title in ["alpha", "beta", "gamma"] {
            vissue_core::ops::create(&layout, "probe", title, vissue_core::CreateOpts::default())
                .unwrap();
        }
        let doc = vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe"))
            .unwrap();
        let ids: Vec<String> = doc.headings.iter().map(|h| h.id.clone()).collect();
        assert_eq!(ids.len(), 3, "three tickets");
        Self {
            dir,
            root,
            home,
            a: ids[0].clone(),
            b: ids[1].clone(),
            c: ids[2].clone(),
        }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
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
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.ljos(args);
        assert!(out.status.success(), "{args:?}\n{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn heading(&self, id: &str) -> vissue_core::IssueHeading {
        let layout = vissue_core::Layout::new(&self.root, "Software");
        let doc = vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe"))
            .unwrap();
        doc.headings
            .into_iter()
            .find(|h| h.id == id)
            .unwrap_or_else(|| panic!("{id} missing"))
    }

    fn finish(&self, id: &str) {
        let layout = vissue_core::Layout::new(&self.root, "Software");
        vissue_core::ops::update(&layout, id, Some("DONE"), None, None, None).unwrap();
    }
}

fn text(out: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn an_open_blocker_leaves_the_ticket_off_the_ready_list() {
    let board = Board::new();
    let ready = board.ok(&["ready"]);
    assert!(ready.contains(&board.a), "{ready}");
    assert!(ready.contains(&board.b), "{ready}");
    assert!(ready.contains(&board.c), "{ready}");

    let said = board.ok(&["block", &board.a, "--on", &board.b]);
    assert!(said.contains("waits on"), "{said}");
    let waiting = board.heading(&board.a);
    assert_eq!(waiting.state, "TODO", "the edge is not a state change");
    assert_eq!(waiting.blocked_by(), vec![board.b.clone()]);
    let org = std::fs::read_to_string(board.root.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains(":BLOCKED_BY:") && org.contains(&board.b),
        "{org}"
    );

    let ready = board.ok(&["ready"]);
    assert!(
        !ready.contains(&board.a),
        "open blocker still listed:\n{ready}"
    );
    assert!(ready.contains(&board.b), "{ready}");
    assert!(ready.contains(&board.c), "{ready}");
    let narrowed = board.ok(&["ready", "-p", "probe"]);
    assert!(!narrowed.contains(&board.a), "{narrowed}");
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(board.ok(&["ready", "--json"]).trim()).unwrap();
    let ids: Vec<&str> = rows.iter().filter_map(|row| row["id"].as_str()).collect();
    assert!(!ids.contains(&board.a.as_str()), "{ids:?}");
    assert!(ids.contains(&board.b.as_str()), "{ids:?}");

    let shown = board.ok(&["show", &board.a]);
    assert!(shown.contains(&board.b), "{shown}");
    assert!(shown.contains("open"), "{shown}");

    let vissue_ready = vissue_core::report::ready(
        &vissue_core::Layout::new(&board.root, "Software"),
        Some("probe"),
    )
    .unwrap();
    assert!(
        !vissue_ready.contains(&board.a),
        "vissue ready still lists the waiting ticket:\n{vissue_ready}"
    );

    let back = board.ljos(&["block", &board.b, "--on", &board.a]);
    let said = text(&back);
    assert!(!back.status.success(), "{said}");
    assert!(said.contains("cycle"), "{said}");
    assert_eq!(board.heading(&board.b).blocked_by(), Vec::<String>::new());

    let own = board.ljos(&["block", &board.c, "--on", &board.c]);
    let said = text(&own);
    assert!(!own.status.success(), "{said}");
    assert!(said.contains("cannot block itself"), "{said}");

    board.ok(&["block", &board.c, "--on", &board.a]);
    let around = board.ljos(&["block", &board.b, "--on", &board.c]);
    let said = text(&around);
    assert!(!around.status.success(), "{said}");
    assert!(said.contains("cycle"), "{said}");

    board.finish(&board.b);
    let ready = board.ok(&["ready"]);
    assert!(
        ready.contains(&board.a),
        "a finished blocker still hides the ticket:\n{ready}"
    );
    assert!(!ready.contains(&board.b), "{ready}");
    assert!(
        !ready.contains(&board.c),
        "c still waits on open a:\n{ready}"
    );
    let shown = board.ok(&["show", &board.a]);
    assert!(shown.contains("closed"), "{shown}");

    let dropped = board.ok(&["block", &board.a, "--off", &board.b]);
    assert!(dropped.contains("no longer waits"), "{dropped}");
    assert!(board.heading(&board.a).blocked_by().is_empty());
    let shown = board.ok(&["show", &board.a]);
    assert!(shown.contains("blockers: none"), "{shown}");
}

#[test]
fn a_missing_ticket_is_named_and_nothing_is_written() {
    let board = Board::new();
    let missing = board.ljos(&["block", &board.a, "--on", "probe-nosuch"]);
    let said = text(&missing);
    assert!(!missing.status.success(), "{said}");
    assert!(said.contains("probe-nosuch"), "{said}");
    assert!(board.heading(&board.a).blocked_by().is_empty());
}
