//! `ljos ready --mine` on a sitting that claimed one ticket.
//!
//! With no plan, the line names that ticket and says it has no plan
//! items. A claimed ticket with no open sitting is still
//! `no unfinished item`.

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;

use std::process::{Command, Stdio};

const ISSUE: &str = "anneal-1mr1";
const TITLE: &str = "Hold the melt";

fn org(title: &str) -> String {
    format!(
        "\
#+TITLE: anneal
#+VISSUE: 1
#+CATEGORY: anneal
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#C] {title}
:PROPERTIES:
:ID:         {ISSUE}
:CLAIMED_BY: saddle
:END:
"
    )
}

struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    fn new() -> Self {
        Self::titled(TITLE)
    }

    fn titled(title: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for name in ["home", "run", "tmp", "tracker/Software/anneal"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        std::fs::write(
            dir.path().join("tracker/vissue.toml"),
            "prefix = \"Software\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tracker/Software/anneal/issues.org"),
            org(title),
        )
        .unwrap();
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
        let mut args = vec!["ready", "--mine"];
        args.extend_from_slice(extra);
        let out = self
            .command(&args)
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

    /// `ljos ready --mine` with stdout on a terminal `cols` columns wide.
    fn ready_mine_on_terminal(&self, cols: usize) -> String {
        let term = pty::Pty::open();
        let mut cmd = self.command(&["ready", "--mine"]);
        cmd.env("COLUMNS", cols.to_string())
            .stdin(Stdio::null())
            .stderr(Stdio::piped());
        term.attach_stdout(&mut cmd);
        let child = term.spawn(&mut cmd);
        let out = term.wait_output(child, 20);
        term.finish_reading();
        let shown = term.shown().replace("\r\n", "\n");
        assert!(
            out.status.success(),
            "{shown}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        shown
    }

    fn command(&self, args: &[&str]) -> Command {
        let home = self.dir.path().join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
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
            .current_dir(self.dir.path());
        cmd
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

#[test]
fn the_sitting_ticket_line_follows_the_terminal_width() {
    let long = "Hold the melt at the strain point until the thermocouple on the lehr reads steady for an hour";
    let seat = Seat::titled(long);
    seat.hold(ISSUE);
    let lead = format!("{ISSUE}  ");
    let cont = " ".repeat(lead.len());
    for cols in [30usize, 200] {
        let shown = seat.ready_mine_on_terminal(cols);
        let title = ljos_cli::output_wrap::wrap_hanging_prose(long, cols, &lead, &cont);
        assert!(shown.contains(&format!("{title}\n")), "{cols}\n{shown}");
        for line in shown.lines() {
            assert!(
                ljos_cli::output_wrap::prose_display_width(line) <= cols,
                "{cols}: {line:?}\n{shown}"
            );
        }
    }
}
