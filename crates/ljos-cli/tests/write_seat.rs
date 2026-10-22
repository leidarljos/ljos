//! A tracker write needs a seat somebody named, as a ballot does.
//! Subagent shells with no `LJOS_SEAT` wrote 314 notes as `exec-daemon`
//! and 74 as `cursor`, the programs above them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Bench {
    _dir: tempfile::TempDir,
    root: PathBuf,
    log: PathBuf,
    path: String,
}

impl Bench {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let log = root.join("calls.log");
        script(
            &root,
            "vissue",
            &format!(
                "#!/bin/sh\necho \"vissue $*\" >> '{}'\ncase \"$1\" in\n  show) echo '{{}}' ;;\n  list|board) echo '[]' ;;\nesac\nexit 0\n",
                log.display()
            ),
        );
        for sub in ["home", "run"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        let path = format!(
            "{}:{}",
            root.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Self {
            _dir: dir,
            root,
            log,
            path,
        }
    }

    fn ljos(&self, seat: Option<&str>, args: &[&str]) -> Output {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("GROK_SESSION_ID", "2c12969d-0000-4000-8000-000000000000")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off");
        if let Some(seat) = seat {
            cmd.env("LJOS_SEAT", seat);
        }
        cmd.output().unwrap()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

const WRITES: &[&[&str]] = &[
    &["note", "proj-aaaa", "progress"],
    &["claim", "proj-aaaa"],
    &["sitting", "proj-aaaa"],
    &["finish", "proj-aaaa", "--lesson", "x"],
    &["release", "proj-aaaa"],
    &["file", "a new bug", "--top", "-p", "proj"],
    &["options", "proj-aaaa", "a,b"],
    &["deed", "proj-aaaa", "--add", "d-0000"],
    &["deed", "proj-aaaa", "--remove", "d-0000"],
];

#[test]
fn every_tracker_write_from_a_shell_with_no_seat_is_refused() {
    let b = Bench::new();
    for args in WRITES {
        let out = b.ljos(None, args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} went through: {out:?}");
        assert!(err.contains("no seat is named"), "{args:?}: {err}");
        assert!(
            err.contains("the program above this shell"),
            "{args:?}: {err}"
        );
        assert!(err.contains("LJOS_SEAT"), "{args:?} names the fix: {err}");
        assert!(
            err.contains("seats.toml"),
            "{args:?} names the seats file: {err}"
        );
    }
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

#[test]
fn a_named_seat_writes_and_a_read_needs_no_seat() {
    let b = Bench::new();
    let note = b.ljos(Some("saddle"), &["note", "proj-aaaa", "progress"]);
    let err = String::from_utf8_lossy(&note.stderr);
    assert!(!err.contains("no seat is named"), "{err}");
    assert!(b.calls().contains("vissue note proj-aaaa"), "{}", b.calls());

    let read = b.ljos(None, &["deed", "proj-aaaa"]);
    let err = String::from_utf8_lossy(&read.stderr);
    assert!(
        !err.contains("no seat is named"),
        "listing is a read: {err}"
    );
}
