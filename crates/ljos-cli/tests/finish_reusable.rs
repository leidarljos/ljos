//! Finishing a sitting on a reusable parent hands its claim back.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[path = "common/sibling.rs"]
mod sibling;

const ISSUE: &str = "ljos-wh2y";

struct World {
    root: tempfile::TempDir,
    siblings: PathBuf,
}

impl World {
    fn new() -> Self {
        let claimdag = sibling::require("claimdag").unwrap_or_else(|line| panic!("{line}"));
        let vissue = sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"));
        let siblings = claimdag.parent().unwrap().to_path_buf();
        assert_eq!(vissue.parent(), Some(siblings.as_path()));
        let root = tempfile::tempdir().unwrap();
        for path in [
            root.path().join("bin"),
            root.path().join("graph"),
            root.path().join("home"),
            root.path().join("run"),
            root.path().join("tracker/Software/ljos"),
        ] {
            std::fs::create_dir_all(path).unwrap();
        }
        script(&root.path().join("bin/deedar"), "#!/bin/sh\necho head\n");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            root.path().join("tracker/vissue.toml"),
            "prefix = \"Software\"\n",
        )
        .unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            root.path().join("tracker/Software/ljos/issues.org"),
            "\
#+TITLE: ljos issues
#+VISSUE: 1
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#C] Keep the long parent reusable
:PROPERTIES:
:ID:         ljos-wh2y
:CREATED:    [2026-11-17 Tue]
:END:
:LOGBOOK:
- Claimed by saddle [2026-11-16 Mon 09:00]
:END:
",
        )
        .unwrap();
        Self { root, siblings }
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.root.path().join("home");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    self.root.path().join("bin").display(),
                    self.siblings.display()
                ),
            )
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("CLAIMDAG_DIR", self.root.path().join("graph"))
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("PACKSET_AUTH", "off")
            .current_dir(self.root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
    }

    fn org(&self) -> String {
        std::fs::read_to_string(self.root.path().join("tracker/Software/ljos/issues.org")).unwrap()
    }
}

fn script(path: &Path, body: &str) {
    ljos_cli::install_executable(path, body).unwrap();
}

fn text(out: &Output) -> String {
    format!(
        "exit {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn plain_finish_after_repeated_sitting_hands_the_parent_back_once() {
    let world = World::new();

    for _ in 0..2 {
        let sitting = world.run(&["sitting", ISSUE]);
        assert!(sitting.status.success(), "{}", text(&sitting));
    }

    let finished = world.run(&["finish", ISSUE]);
    let finished_text = text(&finished);
    assert!(finished.status.success(), "{finished_text}");
    let finished_out = String::from_utf8_lossy(&finished.stdout);
    assert!(
        finished_out.contains(&format!("sitting ended; ticket {ISSUE} is still open")),
        "{finished_text}"
    );
    assert!(
        finished_out.contains("so run `ljos remember \"...\"` to add one."),
        "post-finish lesson hint did not name `ljos remember`: {finished_text}"
    );
    assert!(
        !finished_out.contains(&format!("ljos finish {ISSUE} --lesson")),
        "post-finish hint named `ljos finish` on an unclaimed ticket: {finished_text}"
    );
    assert!(!finished_text.contains("finished ljos-wh2y as done"));
    assert!(!finished_text.contains("release: status done"));

    let handed_back = world.org();
    assert!(
        handed_back.contains("* TODO [#C] Keep the long parent reusable"),
        "{handed_back}"
    );
    assert!(!handed_back.contains(":CLAIMED_BY:"), "{handed_back}");
    assert!(
        !handed_back.contains("- Note: \"playbook:\""),
        "{handed_back}"
    );
    assert_eq!(handed_back.matches("- Claimed by saddle ").count(), 1);
}

#[test]
fn finish_hands_a_reusable_parent_back_before_the_next_sitting() {
    let world = World::new();

    let first = world.run(&["sitting", ISSUE]);
    assert!(first.status.success(), "{}", text(&first));

    let noted = world.run(&["note", ISSUE, "children"]);
    assert!(noted.status.success(), "{}", text(&noted));
    assert!(
        String::from_utf8_lossy(&noted.stdout).contains(&format!("{ISSUE}: noted")),
        "{}",
        text(&noted)
    );

    let finished = world.run(&[
        "finish",
        ISSUE,
        "--lesson",
        "A reusable parent stays open between sittings.",
    ]);
    let finished_text = text(&finished);
    assert!(finished.status.success(), "{finished_text}");
    assert!(
        String::from_utf8_lossy(&finished.stdout)
            .contains(&format!("sitting ended; ticket {ISSUE} is still open")),
        "{finished_text}"
    );
    assert!(!finished_text.contains("finished ljos-wh2y as done"));
    assert!(!finished_text.contains("release: status done"));

    let handed_back = world.org();
    assert!(
        handed_back.contains("* TODO [#C] Keep the long parent reusable"),
        "{handed_back}"
    );
    assert!(!handed_back.contains(":CLAIMED_BY:"), "{handed_back}");
    assert!(
        handed_back.contains("- Note: \"children\""),
        "{handed_back}"
    );
    assert!(
        !handed_back.contains("- Note: \"playbook:\""),
        "{handed_back}"
    );
    assert_eq!(handed_back.matches("- Claimed by saddle ").count(), 1);

    let again = world.run(&["sitting", ISSUE]);
    let again_text = text(&again);
    assert!(again.status.success(), "{again_text}");
    assert!(
        !again_text.contains("reopened a finished session node"),
        "{again_text}"
    );
    let reclaimed = world.org();
    assert!(
        reclaimed.contains("* STARTED [#C] Keep the long parent reusable"),
        "{reclaimed}"
    );
    assert!(reclaimed.contains(":CLAIMED_BY: saddle"), "{reclaimed}");
    assert!(!reclaimed.contains("- Note: \"playbook:\""), "{reclaimed}");
    assert_eq!(reclaimed.matches("- Claimed by saddle ").count(), 1);
}
