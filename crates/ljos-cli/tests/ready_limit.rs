//! `ljos ready` prints the first page of tickets in tracker order.
//!
//! Twenty is the page. `--limit N` prints N, `--all` prints every ticket,
//! and a page that leaves tickets off names the command that lists them.
//! `--json` prints every row unless `--limit` is set. `--mine` is the plan
//! line it already was. A claim on the row is still aged from its stamp.

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

#[test]
fn the_default_page_is_the_first_twenty_in_tracker_order() {
    let board = Board::open(23);
    let page = board.ok(&["ready"]);
    assert!(page.starts_with("ready  shown 20 of 23\n"), "{page}");
    assert!(
        page.trim_end()
            .ends_with("run ljos ready --all to see every ticket"),
        "{page}"
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
        !all.contains("to see every ticket"),
        "a full page still names --all:\n{all}"
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

    let five = board.ok(&["ready", "--limit", "5"]);
    assert!(five.starts_with("ready  shown 5 of 23\n"), "{five}");
    assert_eq!(ticket_ids(&five), every[..5]);
    assert!(
        five.trim_end()
            .ends_with("run ljos ready --all to see every ticket"),
        "{five}"
    );

    let exact = board.ok(&["ready", "--limit", "23"]);
    assert!(exact.starts_with("ready  shown 23 of 23\n"), "{exact}");
    assert!(!exact.contains("to see every ticket"), "{exact}");

    let none = board.ok(&["ready", "--limit", "0"]);
    assert!(none.starts_with("ready  shown 0 of 23\n"), "{none}");
    assert!(ticket_ids(&none).is_empty(), "{none}");
    assert!(
        none.trim_end()
            .ends_with("run ljos ready --all to see every ticket"),
        "{none}"
    );

    let json = board.ok(&["ready", "--json"]);
    assert_eq!(json_ids(&json), every);
    let limited = board.ok(&["ready", "--json", "--limit", "4"]);
    assert_eq!(json_ids(&limited), every[..4]);

    let narrowed = board.ok(&["ready", "-p", "probe"]);
    assert!(
        narrowed.starts_with("ready  shown 20 of 23\n"),
        "{narrowed}"
    );
    assert!(
        narrowed
            .trim_end()
            .ends_with("run ljos ready -p probe --all to see every ticket"),
        "{narrowed}"
    );

    let mine = board.ok(&["ready", "--mine"]);
    assert_eq!(mine, "no unfinished item\n");
    assert!(!board.ids.is_empty());
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
