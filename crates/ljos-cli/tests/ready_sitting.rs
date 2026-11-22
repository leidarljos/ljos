//! `ljos ready --mine` on a sitting that claimed one ticket.
//!
//! With no plan, the line names that ticket and says it has no plan
//! items. A claimed ticket with no open sitting is still
//! `no unfinished item`.

ljos_test_boot::harness!();

use std::process::{Command, Stdio};

const ISSUE: &str = "anneal-1mr1";
const TITLE: &str = "Hold the melt";

const ORG: &str = "\
#+TITLE: anneal
#+VISSUE: 1
#+CATEGORY: anneal
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#C] Hold the melt
:PROPERTIES:
:ID:         anneal-1mr1
:CLAIMED_BY: saddle
:END:
";

struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        for name in ["home", "run", "tmp", "tracker/Software/anneal"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        std::fs::write(
            dir.path().join("tracker/vissue.toml"),
            "prefix = \"Software\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("tracker/Software/anneal/issues.org"), ORG).unwrap();
        Self { dir }
    }

    fn hold(&self, issue: &str) {
        let run = self.dir.path().join("run/ljos");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            run.join("hold-sitting"),
            format!("saddle\nsaddle\n1\nljos\n2026-01-01T00:00:00Z\n{issue}\n-\n"),
        )
        .unwrap();
    }

    fn ready_mine(&self, extra: &[&str]) -> (bool, String, String) {
        let home = self.dir.path().join("home");
        let mut args = vec!["ready", "--mine"];
        args.extend_from_slice(extra);
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(&args)
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("TMPDIR", self.dir.path().join("tmp"))
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("PACKSET_AUTH", "off")
            .current_dir(self.dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

#[test]
fn a_claimed_ticket_without_a_sitting_is_not_listed() {
    let seat = Seat::new();
    let (ok, stdout, stderr) = seat.ready_mine(&[]);
    assert!(ok, "{stdout}{stderr}");
    assert_eq!(stdout, "no unfinished item\n", "{stderr}");
}

#[test]
fn an_open_sitting_names_its_claimed_ticket() {
    let seat = Seat::new();
    seat.hold(ISSUE);
    let (ok, stdout, stderr) = seat.ready_mine(&[]);
    assert!(ok, "{stdout}{stderr}");
    assert!(
        stdout.contains(&format!(
            "the sitting's ticket is {ISSUE} and has no plan items"
        )),
        "{stdout}{stderr}"
    );
    assert!(stdout.contains(TITLE), "{stdout}");
    assert!(
        !stdout.contains("no unfinished item"),
        "the sitting should not read as empty work:\n{stdout}"
    );
    let (ok, other, stderr) = seat.ready_mine(&["-p", "other"]);
    assert!(ok, "{other}{stderr}");
    assert_eq!(
        other, "no unfinished item\n",
        "another project should not name this ticket:\n{stderr}"
    );
}
