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
        self.ljos_with_seat("saddle", args)
    }

    fn ljos_with_seat(&self, seat: &str, args: &[&str]) -> Output {
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
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

    fn issues_path(&self) -> std::path::PathBuf {
        self.root.join("Software/probe/issues.org")
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
    let doc: serde_json::Value =
        serde_json::from_str(board.ok(&["ready", "--json"]).trim()).unwrap();
    let rows = doc["rows"].as_array().expect("rows");
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
    assert!(back.status.success(), "{said}");
    assert!(said.contains("label graph-cycle"), "{said}");
    assert!(said.contains("cycle"), "{said}");
    assert_eq!(board.heading(&board.b).blocked_by(), Vec::<String>::new());

    let own = board.ljos(&["block", &board.c, "--on", &board.c]);
    let said = text(&own);
    assert!(own.status.success(), "{said}");
    assert!(said.contains("label graph-cycle"), "{said}");
    assert!(said.contains("cannot block itself"), "{said}");

    board.ok(&["block", &board.c, "--on", &board.a]);
    let around = board.ljos(&["block", &board.b, "--on", &board.c]);
    let said = text(&around);
    assert!(around.status.success(), "{said}");
    assert!(said.contains("label graph-cycle"), "{said}");
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
fn two_ids_record_the_same_wait_as_on() {
    let board = Board::new();
    let said = board.ok(&["block", &board.a, &board.b]);
    assert!(
        said.contains(&format!("block: {} waits on {}", board.a, board.b)),
        "{said}"
    );
    assert_eq!(board.heading(&board.a).blocked_by(), vec![board.b.clone()]);

    let more = board.ok(&["block", &board.c, &board.a, &board.b]);
    assert!(
        more.contains(&format!("block: {} waits on {}", board.c, board.a)),
        "{more}"
    );
    assert!(
        more.contains(&format!("block: {} waits on {}", board.c, board.b)),
        "{more}"
    );
    assert_eq!(
        board.heading(&board.c).blocked_by(),
        vec![board.a.clone(), board.b.clone()]
    );
}

#[test]
fn a_following_id_beside_on_or_off_is_refused() {
    let board = Board::new();
    let line = "block: a following id stands in for --on, so drop --on and --off.";
    for args in [
        vec![
            "block",
            board.a.as_str(),
            board.b.as_str(),
            "--on",
            board.c.as_str(),
        ],
        vec![
            "block",
            board.a.as_str(),
            board.b.as_str(),
            "--off",
            board.c.as_str(),
        ],
        vec![
            "block",
            board.a.as_str(),
            "--on",
            board.b.as_str(),
            board.c.as_str(),
        ],
        vec![
            "block",
            board.a.as_str(),
            "--off",
            board.b.as_str(),
            board.c.as_str(),
        ],
    ] {
        let out = board.ljos(&args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{}", text(&out));
        let lines: Vec<_> = err.lines().filter(|line| !line.trim().is_empty()).collect();
        assert_eq!(lines, [line], "{args:?}\n{}", text(&out));
        assert!(out.stdout.is_empty(), "{args:?}\n{}", text(&out));
    }
    assert!(board.heading(&board.a).blocked_by().is_empty());
    assert!(board.heading(&board.b).blocked_by().is_empty());
    assert!(board.heading(&board.c).blocked_by().is_empty());
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

#[test]
fn a_ticket_like_word_after_block_suggests_on() {
    let board = Board::new();
    let out = board.ljos(&["block", &board.a, "probe-AB12"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        err.contains("looks like a ticket id")
            && err.contains("ljos block ISSUE --on ID")
            && !err.contains("probe-AB12"),
        "{err}"
    );
    assert!(board.heading(&board.a).blocked_by().is_empty());
}

#[test]
fn block_keeps_same_minute_note_state_and_claim_lines_on_all_tickets() {
    let board = Board::new();
    let minute = "2026-11-17 Tue 17:13";
    let log = |name: &str| {
        format!(
            ":LOGBOOK:\n- Note: \"{name} note\" [{minute}]\n- State \"STARTED\" from \"TODO\" [{minute}]\n- Claimed by saddle [{minute}]\n:END:\n"
        )
    };
    let tracker = format!(
        "#+TITLE: probe issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* TODO [#C] alpha\n:PROPERTIES:\n:ID:         {}\n:END:\n{}\n\
* TODO [#C] beta\n:PROPERTIES:\n:ID:         {}\n:END:\n{}\n\
* TODO [#C] gamma\n:PROPERTIES:\n:ID:         {}\n:END:\n{}\n",
        board.a,
        log("alpha"),
        board.b,
        log("beta"),
        board.c,
        log("gamma")
    );
    std::fs::write(board.issues_path(), tracker).unwrap();

    let said = board.ok(&["block", &board.a, "--on", &board.b]);
    assert!(said.contains("waits on"), "{said}");

    let org = std::fs::read_to_string(board.issues_path()).unwrap();
    for name in ["alpha", "beta", "gamma"] {
        let note = format!("- Note: \"{name} note\" [{minute}]");
        let state = format!("- State \"STARTED\" from \"TODO\" [{minute}]");
        let claim = format!("- Claimed by saddle [{minute}]");
        assert_eq!(org.matches(&note).count(), 1, "{org}");
        assert_eq!(org.matches(&state).count(), 3, "{org}");
        assert_eq!(org.matches(&claim).count(), 3, "{org}");
    }

    let shown = board.ok(&["show", &board.a]);
    assert!(shown.contains("alpha note"), "{shown}");
}

#[test]
fn block_after_concurrent_claims_and_notes_keeps_each_tickets_own_logbook_rows() {
    let board = Board::new();
    let seats = [
        ("bounty".to_string(), board.a.clone()),
        ("fortranbot".to_string(), board.b.clone()),
        ("commbot".to_string(), board.c.clone()),
    ];
    let board_ref = &board;
    std::thread::scope(|scope| {
        for (seat, id) in seats {
            scope.spawn(move || {
                let claimed = board_ref.ljos_with_seat(&seat, &["claim", id.as_str()]);
                assert!(claimed.status.success(), "{seat}\n{}", text(&claimed));
                let note = format!("{seat} note at noon");
                let noted = board_ref.ljos_with_seat(&seat, &["note", id.as_str(), &note]);
                assert!(noted.status.success(), "{seat}\n{}", text(&noted));
            });
        }
    });

    let path = board.issues_path();
    let fixed = "[2026-11-17 Tue 12:00]";
    let normalized = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| {
            if let Some(tail) = line.trim().strip_prefix("- Noted by ") {
                let seat = tail.split(" [").next().unwrap_or("").trim();
                return format!("- Claimed by {seat} {fixed}");
            }
            if line.contains("- Claimed by ") || line.contains("- Note: \"") {
                if let Some((head, _)) = line.rsplit_once('[') {
                    return format!("{} {}", head.trim_end(), fixed);
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, normalized).unwrap();

    let said = board.ok(&["block", &board.a, "--on", &board.b]);
    assert!(said.contains("waits on"), "{said}");
    let org = std::fs::read_to_string(path).unwrap();

    for (seat, id) in [
        ("bounty", &board.a),
        ("fortranbot", &board.b),
        ("commbot", &board.c),
    ] {
        let note = format!("- Note: \"{seat} note at noon\" {fixed}");
        let claim = format!("- Claimed by {seat} {fixed}");
        assert_eq!(org.matches(&note).count(), 1, "{id}\n{org}");
        assert_eq!(org.matches(&claim).count(), 1, "{id}\n{org}");
        let shown = board.ok(&["show", id]);
        assert!(
            shown.contains(&format!("{seat} note at noon")),
            "{id}\n{shown}"
        );
    }
}

#[test]
fn block_keeps_legacy_claimed_by_rows_on_every_ticket() {
    let board = Board::new();
    let minute = "[2026-11-17 Tue 17:13]";
    let tracker = format!(
        "#+TITLE: probe issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* TODO [#C] alpha\n:PROPERTIES:\n:ID:         {}\n:END:\n:LOGBOOK:\n\
- Note: \"noon\" Claimed-by: saddle {minute}\n\
- Note: \"noon\" Claimed-by: other {minute}\n\
- Note: \"noon\" {minute}\n\
:END:\n\
* TODO [#C] beta\n:PROPERTIES:\n:ID:         {}\n:END:\n:LOGBOOK:\n\
- Note: \"beta note\" Claimed-by: fortran {minute}\n\
- Note: \"beta plain\" {minute}\n\
:END:\n",
        board.a, board.b
    );
    std::fs::write(board.issues_path(), tracker).unwrap();

    let said = board.ok(&["block", &board.a, "--on", &board.b]);
    assert!(said.contains("waits on"), "{said}");
    let org = std::fs::read_to_string(board.issues_path()).unwrap();
    for line in [
        format!("- Note: \"noon\" Claimed-by: saddle {minute}"),
        format!("- Note: \"noon\" Claimed-by: other {minute}"),
        format!("- Note: \"noon\" {minute}"),
        format!("- Note: \"beta note\" Claimed-by: fortran {minute}"),
        format!("- Note: \"beta plain\" {minute}"),
    ] {
        assert_eq!(org.matches(line.as_str()).count(), 1, "{line}\n{org}");
    }
}

fn board_with_legacy_stamps(board: &Board, minute: &str) {
    let tracker = format!(
        "#+TITLE: probe issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* TODO [#C] alpha\n:PROPERTIES:\n:ID:         {}\n:END:\n:LOGBOOK:\n\
- Note: \"noon\" Claimed-by: saddle {minute}\n\
- Note: \"noon\" Claimed-by: other {minute}\n\
- Note: \"noon\" {minute}\n\
:END:\n\
* TODO [#C] beta\n:PROPERTIES:\n:ID:         {}\n:END:\n:LOGBOOK:\n\
- Note: \"beta note\" Claimed-by: fortran {minute}\n\
- Note: \"beta plain\" {minute}\n\
:END:\n",
        board.a, board.b
    );
    std::fs::write(board.issues_path(), tracker).unwrap();
}

fn assert_stamps_kept(board: &Board, minute: &str, what: &str) {
    let org = std::fs::read_to_string(board.issues_path()).unwrap();
    for line in [
        format!("- Note: \"noon\" Claimed-by: saddle {minute}"),
        format!("- Note: \"noon\" Claimed-by: other {minute}"),
        format!("- Note: \"noon\" {minute}"),
        format!("- Note: \"beta note\" Claimed-by: fortran {minute}"),
        format!("- Note: \"beta plain\" {minute}"),
    ] {
        assert_eq!(
            org.matches(line.as_str()).count(),
            1,
            "{what} changed {line}\n{org}"
        );
    }
}

#[test]
fn parent_keeps_legacy_claimed_by_rows_on_every_ticket() {
    let board = Board::new();
    let minute = "[2026-11-17 Tue 17:13]";
    board_with_legacy_stamps(&board, minute);
    let said = board.ok(&["parent", &board.a, &board.b]);
    assert!(said.contains("now has parent"), "{said}");
    assert_stamps_kept(&board, minute, "ljos parent");
}

#[test]
fn note_keeps_legacy_claimed_by_rows_on_every_ticket() {
    let board = Board::new();
    let minute = "[2026-11-17 Tue 17:13]";
    board_with_legacy_stamps(&board, minute);
    let said = board.ok(&["note", &board.a, "a later word"]);
    assert!(said.contains("noted"), "{said}");
    assert_stamps_kept(&board, minute, "ljos note");
}

#[test]
fn a_block_and_its_release_render_old_claimed_by_rows_unchanged() {
    let board = Board::new();
    let minute = "[2026-11-17 Tue 17:13]";
    board_with_legacy_stamps(&board, minute);
    board.ok(&["block", &board.a, "--on", &board.b]);
    assert_stamps_kept(&board, minute, "ljos block --on");
    let first = std::fs::read_to_string(board.issues_path()).unwrap();
    board.ok(&["block", &board.a, "--off", &board.b]);
    assert_stamps_kept(&board, minute, "ljos block --off");
    board.ok(&["block", &board.a, "--on", &board.b]);
    let again = std::fs::read_to_string(board.issues_path()).unwrap();
    assert_eq!(
        first
            .lines()
            .filter(|l| l.contains("Claimed-by"))
            .collect::<Vec<_>>(),
        again
            .lines()
            .filter(|l| l.contains("Claimed-by"))
            .collect::<Vec<_>>(),
        "a second render changed a stamp"
    );
}
