//! `ljos file` with no `--parent` says where the ticket went.
//! A hold is the parent, and stderr names it. With no hold and no `-p`,
//! the `prefix=` line of `vissue identity` is not a project: the command
//! exits 1 and names `-p PROJECT`. Nothing is asked.

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
        self.file_in(self.dir.path(), args, &[])
    }

    fn file_in(
        &self,
        cwd: &std::path::Path,
        args: &[&str],
        extra_env: &[(&str, &str)],
    ) -> std::process::Output {
        let home = self.dir.path().join("home");
        let path = format!(
            "{}:{}",
            self.dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.arg("file")
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
            .current_dir(cwd);
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
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

/// A `vissue identity` prefix alone does not pick a project.
#[test]
fn a_prefix_outside_a_checkout_names_p() {
    let seat = Seat::new("'vissue test' 'root=/tmp/tracker' 'prefix=ljos'");
    let out = seat.file(&["x"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("name one with -p PROJECT"), "{err}");
    assert!(seat.creates().is_empty(), "{:?}", seat.creates());
}

#[test]
fn a_checkout_with_no_prefix_names_p() {
    let seat = Seat::new("'vissue test' 'root=/tmp/tracker'");
    let out = seat.file(&["x"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("name one with -p PROJECT"), "{err}");
    assert!(seat.creates().is_empty(), "{:?}", seat.creates());
}

#[test]
fn a_scratch_checkout_without_a_project_names_p() {
    let seat = Seat::new("'vissue test'");
    let checkout = seat.dir.path().join("obchk");
    std::fs::create_dir_all(&checkout).unwrap();
    let init = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(&checkout)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let out = seat.file_in(&checkout, &["x"], &[]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("name one with -p PROJECT"), "{err}");
    assert!(seat.creates().is_empty(), "{:?}", seat.creates());
}

#[test]
fn a_scratch_checkout_uses_ljos_project_when_set() {
    let seat = Seat::new("'vissue test'");
    let checkout = seat.dir.path().join("obchk");
    std::fs::create_dir_all(&checkout).unwrap();
    let init = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(&checkout)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let out = seat.file_in(&checkout, &["x"], &[("LJOS_PROJECT", "seatproj")]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create -p seatproj "),
        "{creates:?}"
    );
}
