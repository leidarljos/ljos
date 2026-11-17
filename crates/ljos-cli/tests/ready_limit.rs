//! `ljos ready` prints the first page of tickets a seat can take.
//!
//! Twenty is the page. `-n` and `--limit` print that many, `--all` prints
//! every ticket, and a short page names `--limit`, `--all`, `-p` and
//! `--mine`. This seat's claims come first, then free tickets, then
//! another seat's claims. Inside that, higher priority, then the newest
//! note, claim or filing, then the id. `--json` prints `shown`, `total`
//! and `rows` for that same page. `--mine` is the plan line it already
//! was. A claim on the row is still aged from its stamp.

ljos_test_boot::harness!();

use std::process::{Command, Output};

struct Board {
    dir: tempfile::TempDir,
    root: std::path::PathBuf,
    home: std::path::PathBuf,
    ids: Vec<String>,
}

impl Board {
    fn open(count: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        let layout = vissue_core::Layout::new(&root, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        let mut ids = Vec::with_capacity(count);
        for n in 0..count {
            let made = vissue_core::ops::create(
                &layout,
                "probe",
                &format!("ticket {n:02}"),
                vissue_core::CreateOpts {
                    quiet: true,
                    ..vissue_core::CreateOpts::default()
                },
            )
            .unwrap();
            let id = made.trim().to_string();
            vissue_core::ops::claim_as(&layout, &id, false, "saddle").unwrap();
            ids.push(id);
        }
        Self {
            dir,
            root,
            home,
            ids,
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
        assert!(
            out.status.success(),
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

fn ticket_ids(page: &str) -> Vec<String> {
    page.lines()
        .filter(|line| line.contains(" [#"))
        .filter_map(|line| line.split_whitespace().next().map(str::to_string))
        .collect()
}

fn json_doc(page: &str) -> serde_json::Value {
    serde_json::from_str(page.trim()).unwrap_or_else(|err| panic!("{err}: {page}"))
}

fn json_ids(page: &str) -> Vec<String> {
    let doc = json_doc(page);
    doc["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("rows missing: {page}"))
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect()
}

fn json_counts(page: &str) -> (u64, u64, usize) {
    let doc = json_doc(page);
    let shown = doc["shown"]
        .as_u64()
        .unwrap_or_else(|| panic!("shown: {page}"));
    let total = doc["total"]
        .as_u64()
        .unwrap_or_else(|| panic!("total: {page}"));
    let rows = doc["rows"].as_array().map(Vec::len).unwrap_or(0);
    (shown, total, rows)
}

const HINT: &str = "See more with `--limit` or `--all`. Narrow with `-p` or `--mine`.";

fn assert_short_page(page: &str, shown: usize, total: usize) {
    let mut lines = page.lines();
    let header = format!("ready  shown {shown} of {total}");
    assert_eq!(lines.next(), Some(header.as_str()), "{page}");
    if shown < total {
        let hint = lines.next().unwrap_or("");
        assert_eq!(hint, HINT, "{page}");
        assert!(hint.chars().count() <= 80, "{hint}");
        assert_eq!(page.matches(HINT).count(), 1, "{page}");
        assert!(
            !page.trim_end().ends_with(HINT),
            "the hint belongs under the header:\n{page}"
        );
    } else {
        assert!(!page.contains(HINT), "{page}");
    }
}

#[test]
fn the_default_page_is_short_and_names_how_to_see_more() {
    let board = Board::open(23);
    let page = board.ok(&["ready"]);
    assert_short_page(&page, 20, 23);
    let shown = ticket_ids(&page);
    assert_eq!(shown.len(), 20, "{page}");
    assert_eq!(
        page.lines().filter(|line| line.contains("claimed")).count(),
        20,
        "a claim age was dropped from the page:\n{page}"
    );

    let all = board.ok(&["ready", "--all"]);
    assert_short_page(&all, 23, 23);
    let every = ticket_ids(&all);
    assert_eq!(every.len(), 23, "{all}");
    assert_eq!(shown, every[..20]);
    assert_eq!(
        all.lines().filter(|line| line.contains("claimed")).count(),
        23,
        "{all}"
    );
    assert!(all.contains("by saddle"), "{all}");

    let five = board.ok(&["ready", "-n", "5"]);
    assert_short_page(&five, 5, 23);
    assert_eq!(ticket_ids(&five), every[..5]);
    let long = board.ok(&["ready", "--limit", "5"]);
    assert_short_page(&long, 5, 23);
    assert_eq!(ticket_ids(&long), ticket_ids(&five), "{long}");

    let exact = board.ok(&["ready", "-n", "23"]);
    assert_short_page(&exact, 23, 23);

    let none = board.ok(&["ready", "-n", "0"]);
    assert_short_page(&none, 0, 23);
    assert!(ticket_ids(&none).is_empty(), "{none}");

    let json = board.ok(&["ready", "--json"]);
    assert_eq!(json_counts(&json), (20, 23, 20), "{json}");
    assert_eq!(json_ids(&json), every[..20]);
    let limited = board.ok(&["ready", "--json", "-n", "4"]);
    assert_eq!(json_counts(&limited), (4, 23, 4), "{limited}");
    assert_eq!(json_ids(&limited), every[..4]);
    let every_json = board.ok(&["ready", "--json", "--all"]);
    assert_eq!(json_counts(&every_json), (23, 23, 23), "{every_json}");
    assert_eq!(json_ids(&every_json), every);

    let narrowed = board.ok(&["ready", "-p", "probe"]);
    assert_short_page(&narrowed, 20, 23);
    assert_eq!(ticket_ids(&narrowed), shown, "{narrowed}");

    let mine = board.ok(&["ready", "--mine"]);
    assert_eq!(mine, "no unfinished item\n");
    assert!(!board.ids.is_empty());
}

#[test]
fn own_claims_come_first_then_priority_then_recent_activity() {
    let board = Board::open(0);
    std::fs::write(board.root.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    let org = board.root.join("Software/probe/issues.org");
    std::fs::create_dir_all(org.parent().unwrap()).unwrap();
    std::fs::write(
        &org,
        "\
#+TITLE: probe
#+VISSUE: 1
#+CATEGORY: probe
#+TODO: TODO STARTED | DONE

* STARTED [#A] Mine early file
:PROPERTIES:
:ID:         probe-m2
:CREATED:    [2026-02-01 Sun]
:CLAIMED_BY: saddle
:CLAIMED_AT: [2026-02-01 Sun 08:00]
:END:

* STARTED [#C] Mine late
:PROPERTIES:
:ID:         probe-m1
:CREATED:    [2026-08-01 Sat 09:00]
:CLAIMED_BY: saddle
:CLAIMED_AT: [2026-08-01 Sat 09:00]
:END:

* STARTED [#A] Mine old file, new note
:PROPERTIES:
:ID:         probe-m3
:CREATED:    [2026-01-01 Thu]
:CLAIMED_BY: saddle
:CLAIMED_AT: [2026-01-01 Thu 08:00]
:END:
:LOGBOOK:
- Note: \"the glaze cooled\" [2026-10-01 Thu 11:00]
:END:

* STARTED [#A] Theirs old file, new claim
:PROPERTIES:
:ID:         probe-t1
:CREATED:    [2026-01-15 Thu]
:CLAIMED_BY: other
:CLAIMED_AT: [2026-07-01 Wed 18:40]
:END:

* STARTED [#A] Theirs newer file, older claim
:PROPERTIES:
:ID:         probe-t0
:CREATED:    [2026-05-01 Fri]
:CLAIMED_BY: other
:CLAIMED_AT: [2026-03-01 Sun 08:00]
:END:

* TODO [#A] New high
:PROPERTIES:
:ID:         probe-a1
:CREATED:    [2026-06-01 Mon]
:END:

* TODO [#A] Same day, later id
:PROPERTIES:
:ID:         probe-a3
:CREATED:    [2026-06-01 Mon]
:END:

* TODO [#A] Old high, new note
:PROPERTIES:
:ID:         probe-a2
:CREATED:    [2026-01-02 Fri]
:END:
:LOGBOOK:
- Note: \"still the next kiln\" [2026-09-01 Tue 10:00]
:END:

* TODO [#B] Mid later id
:PROPERTIES:
:ID:         probe-b2
:CREATED:    [2026-03-01 Sun]
:END:

* TODO [#B] Mid
:PROPERTIES:
:ID:         probe-b1
:CREATED:    [2026-03-01 Sun]
:END:

* TODO [#C] Low
:PROPERTIES:
:ID:         probe-c1
:CREATED:    [2026-01-01 Thu]
:END:

* TODO [#A] Wait
:PROPERTIES:
:ID:         probe-w1
:BLOCKED_BY: probe-a1
:CREATED:    [2026-09-15 Tue]
:END:
",
    )
    .unwrap();
    // This seat, then free tickets, then another seat. Priority, then the
    // newest note, claim or filing, then the id. probe-t1's claim is newer
    // than its filing and newer than probe-t0, and it still follows the
    // free tickets. probe-a2's note is newer than probe-a1's filing.
    // probe-b2 is written above probe-b1 and the id still comes first.
    let want = [
        "probe-m3", "probe-m2", "probe-m1", "probe-a2", "probe-a1", "probe-a3", "probe-b1",
        "probe-b2", "probe-c1", "probe-t1", "probe-t0",
    ];
    let all = board.ok(&["ready", "--all"]);
    assert_short_page(&all, want.len(), want.len());
    assert_eq!(ticket_ids(&all), want, "{all}");
    assert!(
        !all.contains("probe-w1"),
        "an open blocker stayed on the page:\n{all}"
    );
    assert!(all.contains("claimed"), "{all}");
    let ids = ticket_ids(&all);
    let other = ids.iter().position(|id| id == "probe-t1").unwrap();
    let free = ids.iter().position(|id| id == "probe-c1").unwrap();
    assert!(
        other > free,
        "another seat's claim sat above free work:\n{all}"
    );

    let two = board.ok(&["ready", "-n", "2"]);
    assert_short_page(&two, 2, want.len());
    assert_eq!(ticket_ids(&two), want[..2]);
    let json = board.ok(&["ready", "--json", "-n", "3"]);
    assert_eq!(json_counts(&json), (3, want.len() as u64, 3), "{json}");
    assert_eq!(json_ids(&json), want[..3]);
    let every = board.ok(&["ready", "--json"]);
    assert_eq!(
        json_counts(&every),
        (want.len() as u64, want.len() as u64, want.len()),
        "{every}"
    );
    assert_eq!(json_ids(&every), want);
}

#[test]
fn limit_and_all_are_not_combined() {
    let board = Board::open(1);
    let out = board.ljos(&["ready", "--all", "--limit", "1"]);
    assert!(!out.status.success(), "both flags were accepted");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--all"), "{err}");
    assert!(err.contains("--limit"), "{err}");
}
