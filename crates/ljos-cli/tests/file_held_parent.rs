//! `ljos file` with no `--parent` says where the ticket went.
//! A hold is the parent, and stderr names it. With no hold and no `-p`,
//! the project is the `prefix=` line of `vissue identity`. With no prefix
//! either, the ticket goes in Software. Nothing is asked.

ljos_test_boot::harness!();
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    fn new(identity: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let stub = dir.path().join("vissue");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\necho \"vissue $*\" >> '{log}'\ncase \"$1\" in\n  identity) printf '%s\\n' {identity} ;;\n  create) echo ljos-new1 ;;\nesac\nexit 0\n",
                log = log.display(),
                identity = identity,
            ),
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("run")).unwrap();
        Self { dir }
    }

    /// A hold record this command's parent process matches: the seat is
    /// `rohit` and the pid is this test, which is the parent of `ljos`.
    fn hold(&self, issue: &str) {
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
        let runtime = self.dir.path().join("home/run/ljos");
        std::fs::create_dir_all(&runtime).unwrap();
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

    fn file(&self, args: &[&str]) -> std::process::Output {
        let home = self.dir.path().join("home");
        let path = format!(
            "{}:{}",
            self.dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .arg("file")
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("LJOS_SEAT", "rohit")
            .env("LJOS_HOLD", "rohit")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            // A git checkout names its own project. This seat has none, so
            // the hold and `vissue identity` decide where the ticket goes.
            .current_dir(self.dir.path())
            .output()
            .unwrap()
    }

    fn creates(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("calls.log"))
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("vissue create"))
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn a_held_issue_is_named_when_the_file_lands_under_it() {
    let seat = Seat::new("'vissue test'");
    seat.hold("eb-stack-hqqx");
    let out = seat.file(&["a tool bug"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create -p eb-stack --parent eb-stack-hqqx"),
        "{creates:?}"
    );
    assert!(err.contains("eb-stack-hqqx"), "{err}");
    assert!(err.contains("--top -p"), "{err}");
}

#[test]
fn file_prints_only_the_id_on_stdout() {
    let seat = Seat::new("'vissue test'");
    seat.hold("eb-stack-hqqx");
    let out = seat.file(&["a tool bug"]);
    let said = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}{err}");
    assert_eq!(said, "ljos-new1\n", "{said:?}");
    assert_eq!(said.lines().count(), 1, "{said:?}");
    assert!(err.contains("eb-stack-hqqx"), "{err}");
}

#[test]
fn porcelain_file_under_a_hold_prints_only_the_id() {
    let seat = Seat::new("'vissue test'");
    seat.hold("eb-stack-hqqx");
    let out = seat.file(&["a tool bug", "--porcelain"]);
    let said = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}{err}");
    assert_eq!(said, "ljos-new1\n", "{said:?}");
    assert!(err.contains("eb-stack-hqqx"), "{err}");
    assert!(err.contains("--top -p"), "{err}");
}

/// `vissue identity` still prints a prefix when this directory is not a
/// git checkout. The line names that default and the `-p` command.
#[test]
fn a_prefix_outside_a_checkout_names_the_flag() {
    let seat = Seat::new("'vissue test' 'root=/tmp/tracker' 'prefix=ljos'");
    let out = seat.file(&["x"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create -p ljos "),
        "{creates:?}"
    );
    assert!(!creates[0].contains("--parent"), "{creates:?}");
    assert!(
        err.contains(
            "filed in ljos. This directory is not a git checkout, so run `ljos file TITLE -p NAME`."
        ),
        "{err}"
    );
    assert!(!err.contains("this checkout's project"), "{err}");
}

#[test]
fn a_checkout_with_no_prefix_files_in_software() {
    let seat = Seat::new("'vissue test' 'root=/tmp/tracker'");
    let out = seat.file(&["x"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create -p Software "),
        "{creates:?}"
    );
    assert!(err.contains("filed in Software"), "{err}");
    assert!(!err.contains('?'), "file asked a question:\n{err}");
}
