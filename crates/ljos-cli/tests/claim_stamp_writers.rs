//! Tracker writers keep legacy claim-stamped logbook rows verbatim.

ljos_test_boot::harness!();

use std::process::{Command, Output};

struct Board {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    home: std::path::PathBuf,
    run: std::path::PathBuf,
    parent: String,
    child: String,
}

impl Board {
    fn new(with_deed: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        let run = dir.path().join("run");
        std::fs::create_dir_all(root.join("Software/probe")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&run).unwrap();
        let parent = "probe-parent".to_string();
        let child = "probe-child".to_string();
        let deed = if with_deed {
            ":DEEDS:     deed-file-old\n"
        } else {
            ""
        };
        let tracker = format!(
            "#+TITLE: probe issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* TODO [#C] parent :plan:\n\
:PROPERTIES:\n\
:ID:         {parent}\n\
:ORDERED:    t\n\
{deed}\
:END:\n\
:LOGBOOK:\n\
- Note: \"parent note\" Claimed-by: saddle [2026-11-17 Tue 17:13]\n\
- Claimed by saddle [2026-11-17 Tue 17:14]\n\
:END:\n\
* TODO [#C] child :checklist:\n\
:PROPERTIES:\n\
:ID:         {child}\n\
:PARENT:     {parent}\n\
:END:\n\
:LOGBOOK:\n\
- Note: \"child note\" Claimed-by: fortran [2026-11-17 Tue 18:13]\n\
- Claimed by fortran [2026-11-17 Tue 18:14]\n\
:END:\n"
        );
        std::fs::write(root.join("Software/probe/issues.org"), tracker).unwrap();
        Self {
            _dir: dir,
            root,
            home,
            run,
            parent,
            child,
        }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.parent().unwrap())
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", &self.run)
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

    fn board_text(&self) -> String {
        std::fs::read_to_string(self.root.join("Software/probe/issues.org")).unwrap()
    }
}

fn stamp_rows(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.contains("Claimed-by:") || line.starts_with("- Claimed by "))
        .collect()
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn finishing_a_plan_item_keeps_every_legacy_claim_stamp_row() {
    let board = Board::new(false);
    let before = board.board_text();
    let out = board.ljos(&["finish", &board.child]);
    assert_ok(&out);
    let after = board.board_text();
    assert_eq!(stamp_rows(&after), stamp_rows(&before), "{after}");
}

#[test]
fn removing_a_deed_keeps_every_legacy_claim_stamp_row() {
    let board = Board::new(true);
    let before = board.board_text();
    let out = board.ljos(&["deed", &board.parent, "--remove", "deed-file-old"]);
    assert_ok(&out);
    let after = board.board_text();
    assert_eq!(stamp_rows(&after), stamp_rows(&before), "{after}");
}
