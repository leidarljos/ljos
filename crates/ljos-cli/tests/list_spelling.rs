//! `ljos list` and `ljos ls` are `ljos ready`.
//!
//! A new seat types `list` for the open tickets. The word runs the ready
//! page, takes the flags `ready` takes, and prints what `ready` prints.

ljos_test_boot::harness!();

use std::process::{Command, Output};

struct Board {
    dir: tempfile::TempDir,
    root: std::path::PathBuf,
    home: std::path::PathBuf,
}

impl Board {
    fn open() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        let layout = vissue_core::Layout::new(&root, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        for (project, title) in [
            ("probe", "tune the kiln"),
            ("probe", "mend the door"),
            ("probe", "sand the shelf"),
            ("other", "paint the sign"),
        ] {
            let made = vissue_core::ops::create(
                &layout,
                project,
                title,
                vissue_core::CreateOpts {
                    quiet: true,
                    ..vissue_core::CreateOpts::default()
                },
            )
            .unwrap();
            if title == "mend the door" {
                vissue_core::ops::claim_as(&layout, made.trim(), false, "saddle").unwrap();
            }
        }
        Self { dir, root, home }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
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
}

fn said(out: &Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// `ljos <spelling> FLAGS` is the same run as `ljos ready FLAGS`.
fn same_as_ready(board: &Board, spelling: &str, flags: &[&str]) -> Output {
    let ready_args: Vec<&str> = std::iter::once("ready")
        .chain(flags.iter().copied())
        .collect();
    let spelled_args: Vec<&str> = std::iter::once(spelling)
        .chain(flags.iter().copied())
        .collect();
    let ready = board.ljos(&ready_args);
    let spelled = board.ljos(&spelled_args);
    assert_eq!(
        spelled.status.code(),
        ready.status.code(),
        "ljos {spelling} {flags:?} exits differently from ready\nready: {}\n{spelling}: {}",
        said(&ready),
        said(&spelled)
    );
    assert_eq!(
        String::from_utf8_lossy(&spelled.stdout),
        String::from_utf8_lossy(&ready.stdout),
        "ljos {spelling} {flags:?} prints a different page from ready"
    );
    assert_eq!(
        String::from_utf8_lossy(&spelled.stderr),
        String::from_utf8_lossy(&ready.stderr),
        "ljos {spelling} {flags:?} says something else on stderr than ready"
    );
    spelled
}

const FILTERS: &[&[&str]] = &[
    &[],
    &["-p", "probe"],
    &["--project", "other"],
    &["-p", "nothing-here"],
    &["-n", "2"],
    &["--limit", "1", "-p", "probe"],
    &["--all"],
    &["--json"],
    &["--json", "-p", "probe", "-n", "2"],
    &["--mine"],
    &["--mine", "-p", "probe"],
];

#[test]
fn list_and_ls_print_the_ready_page() {
    let board = Board::open();
    for spelling in ["list", "ls"] {
        for flags in FILTERS {
            same_as_ready(&board, spelling, flags);
        }
    }
}

#[test]
fn list_and_ls_succeed_and_name_the_open_tickets() {
    let board = Board::open();
    for spelling in ["list", "ls"] {
        let out = board.ljos(&[spelling]);
        let text = said(&out);
        assert!(out.status.success(), "{text}");
        assert!(out.stderr.is_empty(), "{text}");
        let page = String::from_utf8_lossy(&out.stdout);
        assert!(page.starts_with("ready  shown 4 of 4\n"), "{text}");
        for title in [
            "tune the kiln",
            "mend the door",
            "sand the shelf",
            "paint the sign",
        ] {
            assert!(page.contains(title), "{spelling} left out {title}\n{text}");
        }
        assert!(!page.contains("no command"), "{text}");
        let one = board.ljos(&[spelling, "-p", "other"]);
        let page = String::from_utf8_lossy(&one.stdout);
        assert!(one.status.success(), "{}", said(&one));
        assert!(page.starts_with("ready  shown 1 of 1\n"), "{page}");
        assert!(page.contains("paint the sign"), "{page}");
        assert!(!page.contains("tune the kiln"), "{page}");
    }
}

#[test]
fn list_json_is_the_ready_page_json() {
    let board = Board::open();
    let out = same_as_ready(&board, "list", &["--json", "-n", "3"]);
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|err| panic!("{err}: {}", said(&out)));
    assert_eq!(doc["shown"], 3, "{doc}");
    assert_eq!(doc["total"], 4, "{doc}");
    assert_eq!(doc["rows"].as_array().map(Vec::len), Some(3), "{doc}");
}

#[test]
fn list_refuses_the_flag_pairs_ready_refuses() {
    let board = Board::open();
    for spelling in ["list", "ls"] {
        for flags in [
            &["--json", "--mine"][..],
            &["--all", "-n", "2"][..],
            &["--all", "--mine"][..],
            &["--nope"][..],
            &["-n", "many"][..],
        ] {
            let ready_args: Vec<&str> = std::iter::once("ready")
                .chain(flags.iter().copied())
                .collect();
            let spelled_args: Vec<&str> = std::iter::once(spelling)
                .chain(flags.iter().copied())
                .collect();
            let ready = board.ljos(&ready_args);
            let spelled = board.ljos(&spelled_args);
            assert_eq!(ready.status.code(), Some(2), "{flags:?}\n{}", said(&ready));
            assert_eq!(
                spelled.status.code(),
                Some(2),
                "{spelling} {flags:?}\n{}",
                said(&spelled)
            );
            assert!(spelled.stdout.is_empty(), "{}", said(&spelled));
            assert!(!spelled.stderr.is_empty(), "{}", said(&spelled));
        }
    }
}

#[test]
fn help_lists_list_beside_ready() {
    let board = Board::open();
    let help = board.ljos(&["help"]);
    let text = said(&help);
    assert!(help.status.success(), "{text}");
    let page = String::from_utf8_lossy(&help.stdout);
    let row = |name: &str| {
        page.lines()
            .position(|line| line.split_whitespace().next() == Some(name))
            .unwrap_or_else(|| panic!("`ljos help` has no {name} row\n{page}"))
    };
    assert_eq!(row("list"), row("ready") + 1, "{page}");
    let line = page.lines().nth(row("list")).unwrap();
    assert!(line.contains("ljos ready"), "{line}");

    for topic in ["list", "ls"] {
        let out = board.ljos(&["help", topic]);
        let text = said(&out);
        assert!(out.status.success(), "{text}");
        assert!(out.stderr.is_empty(), "{text}");
        let page = String::from_utf8_lossy(&out.stdout);
        assert!(page.contains("Usage: ljos list"), "{text}");
        assert!(page.contains("--project"), "{text}");
        assert!(!page.contains("no command"), "{text}");
    }
    for spelling in ["list", "ls"] {
        let out = board.ljos(&[spelling, "--help"]);
        let text = said(&out);
        assert!(out.status.success(), "{text}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("Usage: ljos list"),
            "{text}"
        );
    }
}

#[test]
fn other_listing_words_still_point_at_ready() {
    let board = Board::open();
    for word in ["issues", "tickets", "todo"] {
        let out = board.ljos(&[word]);
        assert_eq!(out.status.code(), Some(2), "{}", said(&out));
        assert!(out.stdout.is_empty(), "{}", said(&out));
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            format!(
                "ljos: no command {word}, and the closest is ready, so run `ljos ready`. `ljos claims` lists what is held.\n"
            )
        );
    }
}
