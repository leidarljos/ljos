//! A project path must not become an issue id, and a held id is resolved
//! before any line that says the ticket was filed.
//!
//! `Software/hle-mm` used to land at `<tracker>/Software/Software/hle-mm`
//! with `:ID: Software/hle-mm-yled`. The hold then remembered `hle-mm-yled`,
//! which no lookup resolved, and `ljos file` printed the filing line and
//! then the miss.

ljos_test_boot::harness!();

use std::process::Command;

struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    fn new(create_status: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let stub = dir.path().join("vissue");
        ljos_cli::install_executable(
            &stub,
            format!(
                "#!/bin/sh\necho \"vissue $*\" >> '{log}'\ncase \"$1\" in\n  identity) printf '%s\\n' 'vissue test' ;;\n  create) echo stub-id; exit {create_status} ;;\nesac\nexit 0\n",
                log = log.display(),
                create_status = create_status,
            ),
        )
        .unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("run")).unwrap();
        let demo = dir.path().join("tracker/Software/demo");
        std::fs::create_dir_all(&demo).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            demo.join("issues.org"),
            "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] A real ticket
:PROPERTIES:
:ID:         demo-aaaa
:END:
",
        )
        .unwrap();
        Self { dir }
    }

    fn tracker_root(&self) -> std::path::PathBuf {
        self.dir.path().join("tracker")
    }

    fn hold(&self, issue: &str) {
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
        let runtime = self.dir.path().join("home/run/ljos");
        std::fs::create_dir_all(&runtime).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            runtime.join("hold-rohit"),
            format!(
                "rohit\nrohit\n{}\n{}\n2026-11-03T00:00:00Z\n{issue}\n-\n",
                std::process::id(),
                comm.trim()
            ),
        )
        .unwrap();
    }

    fn hold_tracker(&self, issue: &str, root: &std::path::Path) {
        self.hold(issue);
        let path = self.hold_path();
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str(&format!("{}\tSoftware\n", root.display()));
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, text).unwrap();
    }

    fn nested(&self) {
        let board = self.dir.path().join("tracker/Software/Software/hle-mm");
        std::fs::create_dir_all(&board).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            board.join("issues.org"),
            "\
#+TITLE: hle-mm
#+VISSUE: 1
#+CATEGORY: hle-mm
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#A] Nested mill
:PROPERTIES:
:ID:         Software/hle-mm-yled
:END:
",
        )
        .unwrap();
    }

    fn file(&self, args: &[&str]) -> std::process::Output {
        self.run(&[&["file"], args].concat())
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let home = self.dir.path().join("home");
        let path = format!(
            "{}:{}",
            self.dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "rohit")
            .env("LJOS_HOLD", "rohit")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_OFF", "1")
            .current_dir(self.dir.path())
            .output()
            .unwrap();
    }

    fn creates(&self) -> Vec<String> {
        let path = self.dir.path().join("calls.log");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        text.lines()
            .filter(|line| line.starts_with("vissue create"))
            .map(str::to_string)
            .collect()
    }

    fn hold_path(&self) -> std::path::PathBuf {
        self.dir.path().join("home/run/ljos/hold-rohit")
    }
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_repeated_org_segment_is_the_project_name() {
    let seat = Seat::new("0");
    let out = seat.file(&["a ticket", "--top", "-p", "Software/hle-mm"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create --project=hle-mm "),
        "{creates:?}"
    );
    assert!(!creates[0].contains("Software/"), "{creates:?}");
    assert!(!said.contains("not found"), "{said}");
}

#[test]
fn a_project_path_is_refused_in_one_line() {
    let seat = Seat::new("0");
    let out = seat.file(&["a ticket", "--top", "-p", "foo/bar"]);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("pass -p bar"), "{said}");
    assert!(said.contains("not -p foo/bar"), "{said}");
    assert!(!said.contains("filed"), "{said}");
    assert!(seat.creates().is_empty(), "{said}");
}

#[test]
fn a_missing_hold_is_cleared_and_the_ticket_is_filed_at_the_top() {
    let seat = Seat::new("0");
    seat.hold("hle-mm-yled");
    let out = seat.file(&["a ticket", "-p", "hle-mm"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("held issue hle-mm-yled is gone; filed at the top of hle-mm"),
        "{said}"
    );
    assert!(!said.contains("filed under"), "{said}");
    assert!(!said.contains("not found"), "{said}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create --project=hle-mm "),
        "{creates:?}"
    );
    assert!(!creates[0].contains("--parent"), "{creates:?}");
    assert!(!seat.hold_path().exists(), "the hold file stayed");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "stub-id\n");
}

#[test]
fn a_hold_from_another_tracker_is_not_the_parent() {
    let seat = Seat::new("0");
    let elsewhere = seat.tracker_root().join("elsewhere");
    seat.hold_tracker("demo-bist", &elsewhere);
    let out = seat.file(&["Pick a codec", "-p", "demo"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("held issue demo-bist is on another tracker; filed at the top of demo"),
        "{said}"
    );
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(!creates[0].contains("--parent"), "{creates:?}");
    assert!(
        seat.hold_path().exists(),
        "the other tracker's sitting was removed"
    );
}

#[test]
fn a_hold_on_this_tracker_is_still_the_parent() {
    let seat = Seat::new("0");
    seat.hold_tracker("demo-aaaa", &seat.tracker_root());
    let out = seat.file(&["a ticket", "-p", "demo"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(!said.contains("another tracker"), "{said}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].contains("--parent") && creates[0].contains("demo-aaaa"),
        "{creates:?}\n{said}"
    );
}

#[test]
fn a_failed_file_does_not_say_it_filed() {
    let seat = Seat::new("1");
    seat.hold("hle-mm-yled");
    let out = seat.file(&["a ticket", "-p", "hle-mm"]);
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(!said.contains("filed under"), "{said}");
    assert!(!said.contains("is gone"), "{said}");
    assert!(!said.contains("filed at the top"), "{said}");
    assert!(!seat.hold_path().exists(), "the hold file stayed");
}

#[test]
fn a_held_alias_files_under_the_id_on_disk() {
    let seat = Seat::new("0");
    seat.nested();
    seat.hold("hle-mm-yled");
    let out = seat.file(&["a child"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("filed under Software/hle-mm-yled in hle-mm"),
        "{said}"
    );
    assert!(!said.contains("not found"), "{said}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].contains("--project=hle-mm ")
            && creates[0].contains("--parent=Software/hle-mm-yled"),
        "{creates:?}"
    );
}

#[test]
fn show_finds_the_legacy_id_and_the_bare_id() {
    let seat = Seat::new("0");
    seat.nested();
    for id in ["Software/hle-mm-yled", "hle-mm-yled"] {
        let out = seat.run(&["show", id]);
        let said = text(&out);
        assert!(out.status.success(), "{id}: {said}");
        assert!(said.contains("Nested mill"), "{id}: {said}");
        assert!(said.contains("Software/hle-mm-yled"), "{id}: {said}");
    }
    let missed = seat.run(&["show", "Software-yled"]);
    let said = text(&missed);
    assert!(!missed.status.success(), "{said}");
    assert!(said.contains("not found"), "{said}");
    assert!(said.contains("ljos ready"), "{said}");
}
