//! `ljos ready` prints the first page of tickets a seat can take.
//!
//! Twenty is the page. `-n` and `--limit` print that many, `--all` prints
//! every ticket, and a short page names the command that prints the rest.
//! This seat's started tickets come first, then higher priority, then a
//! newer ticket. `--json` prints every row unless `-n` is set, in that
//! order. `--mine` is the plan line it already was. A claim on the row is
//! still aged from its stamp.

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

fn json_ids(page: &str) -> Vec<String> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(page.trim()).unwrap();
    rows.iter()
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect()
}

fn page_head(page: &str) -> &str {
    page.split("\n\n").next().unwrap_or(page)
}

#[test]
fn the_default_page_names_the_command_for_the_rest() {
    let board = Board::open(23);
    let page = board.ok(&["ready"]);
    let head = page_head(&page);
    assert!(
        head.starts_with("ready  shown 20 of 23, so run `ljos ready -n 23`."),
        "{page}"
    );
    assert!(
        !page.trim_end().ends_with("so run `ljos ready -n 23`."),
        "the command belongs on the header, not after the rows:\n{page}"
    );
    let shown = ticket_ids(&page);
    assert_eq!(shown.len(), 20, "{page}");
    assert_eq!(
        page.lines().filter(|line| line.contains("claimed")).count(),
        20,
        "a claim age was dropped from the page:\n{page}"
    );

    let all = board.ok(&["ready", "--all"]);
    assert!(all.starts_with("ready  shown 23 of 23\n"), "{all}");
    assert!(
        !page_head(&all).contains("-n "),
        "a full page still names a command:\n{all}"
    );
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
    assert!(
        page_head(&five).starts_with("ready  shown 5 of 23, so run `ljos ready -n 23`."),
        "{five}"
    );
    assert_eq!(ticket_ids(&five), every[..5]);
    let long = board.ok(&["ready", "--limit", "5"]);
    assert_eq!(ticket_ids(&long), ticket_ids(&five), "{long}");

    let exact = board.ok(&["ready", "-n", "23"]);
    assert!(exact.starts_with("ready  shown 23 of 23\n"), "{exact}");
    assert!(!page_head(&exact).contains("-n "), "{exact}");

    let none = board.ok(&["ready", "-n", "0"]);
    assert!(
        page_head(&none).starts_with("ready  shown 0 of 23, so run `ljos ready -n 23`."),
        "{none}"
    );
    assert!(ticket_ids(&none).is_empty(), "{none}");

    let json = board.ok(&["ready", "--json"]);
    assert_eq!(json_ids(&json), every);
    let limited = board.ok(&["ready", "--json", "-n", "4"]);
    assert_eq!(json_ids(&limited), every[..4]);

    let narrowed = board.ok(&["ready", "-p", "probe"]);
    assert!(
        page_head(&narrowed)
            .starts_with("ready  shown 20 of 23, so run `ljos ready -p probe -n 23`."),
        "{narrowed}"
    );

    let mine = board.ok(&["ready", "--mine"]);
    assert_eq!(mine, "no unfinished item\n");
    assert!(!board.ids.is_empty());
}

#[test]
fn started_work_comes_before_priority_and_newer_tickets() {
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

* STARTED [#C] Mine late
:PROPERTIES:
:ID:         probe-m1
:CREATED:    [2026-08-01 Sat 09:00]
:CLAIMED_BY: saddle
:CLAIMED_AT: [2026-08-01 Sat 09:00]
:END:

* STARTED [#A] Mine early
:PROPERTIES:
:ID:         probe-m2
:CREATED:    [2026-02-01 Sun]
:CLAIMED_BY: saddle
:CLAIMED_AT: [2026-02-01 Sun 08:00]
:END:

* STARTED [#A] Theirs newest
:PROPERTIES:
:ID:         probe-t1
:CREATED:    [2026-07-01 Wed 18:40]
:CLAIMED_BY: other
:CLAIMED_AT: [2026-07-01 Wed 18:40]
:END:

* TODO [#A] New high
:PROPERTIES:
:ID:         probe-a1
:CREATED:    [2026-06-01 Mon]
:END:

* TODO [#A] Old high
:PROPERTIES:
:ID:         probe-a2
:CREATED:    [2026-01-02 Fri]
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
:CREATED:    [2026-09-01 Tue]
:END:
",
    )
    .unwrap();
    let want = [
        "probe-m2", "probe-m1", "probe-t1", "probe-a1", "probe-a2", "probe-b1", "probe-c1",
    ];
    let all = board.ok(&["ready", "--all"]);
    assert!(all.starts_with("ready  shown 7 of 7\n"), "{all}");
    assert_eq!(ticket_ids(&all), want, "{all}");
    assert!(
        !all.contains("probe-w1"),
        "an open blocker stayed on the page:\n{all}"
    );
    assert!(all.contains("claimed"), "{all}");

    let two = board.ok(&["ready", "-n", "2"]);
    assert!(
        page_head(&two).contains("ready  shown 2 of 7, so run `ljos ready -n 7`."),
        "{two}"
    );
    assert_eq!(ticket_ids(&two), want[..2]);
    let json = board.ok(&["ready", "--json", "-n", "3"]);
    assert_eq!(json_ids(&json), want[..3]);
    let every = board.ok(&["ready", "--json"]);
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
