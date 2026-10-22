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
                "#!/bin/sh\necho \"vissue $* VISSUE_AGENT=${{VISSUE_AGENT:-}}\" >> '{}'\ncase \"$1\" in\n  show) echo '{{}}' ;;\n  list|board) echo '[]' ;;\nesac\nexit 0\n",
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
        let env: Vec<(&str, &str)> = seat.map(|s| ("LJOS_SEAT", s)).into_iter().collect();
        self.ljos_env(&env, args)
    }

    fn ljos_env(&self, env: &[(&str, &str)], args: &[&str]) -> Output {
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
            .env("PACKSET_URL", "off")
            .envs(env.iter().copied());
        cmd.output().unwrap()
    }

    fn seats(&self, body: &str) -> PathBuf {
        let dir = self.root.join("home/.config/ljos");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("seats.toml");
        std::fs::write(&path, body).unwrap();
        path
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

#[test]
fn with_a_seats_file_only_a_listed_seat_writes() {
    let b = Bench::new();
    let note = ["note", "proj-aaaa", "progress"];
    let open = b.ljos(Some("stranger"), &note);
    assert!(
        !String::from_utf8_lossy(&open.stderr).contains("no seat named"),
        "with no seats file any named seat writes: {open:?}"
    );
    assert!(b.calls().contains("vissue note"), "{}", b.calls());

    let b = Bench::new();
    let path = b.seats("\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n");
    let refused = b.ljos(Some("stranger"), &note);
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert!(err.contains(&path.display().to_string()), "{err}");
    assert!(err.contains("saddle"), "names the seats it lists: {err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    let listed = b.ljos(Some("saddle"), &note);
    assert!(
        !String::from_utf8_lossy(&listed.stderr).contains("no seat named"),
        "{listed:?}"
    );
    assert!(b.calls().contains("vissue note"), "{}", b.calls());
}

#[test]
fn a_conversation_the_seats_file_maps_writes_as_that_seat() {
    let b = Bench::new();
    b.seats("\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n");
    let out = b.ljos_env(
        &[(
            "CURSOR_CONVERSATION_ID",
            "0b1c2d3e-0000-4000-8000-000000000000",
        )],
        &["note", "proj-aaaa", "progress"],
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("no seat is named"), "{err}");
    assert!(!err.contains("no seat named"), "{err}");
    assert!(b.calls().contains("vissue note proj-aaaa"), "{}", b.calls());
}

#[test]
fn a_write_records_the_seat_the_seats_file_maps() {
    let b = Bench::new();
    b.seats("\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n");
    b.ljos_env(
        &[(
            "CURSOR_CONVERSATION_ID",
            "0b1c2d3e-0000-4000-8000-000000000000",
        )],
        &["note", "proj-aaaa", "progress"],
    );
    let calls = b.calls();
    let note = calls
        .lines()
        .find(|l| l.starts_with("vissue note proj-aaaa"))
        .unwrap_or_else(|| panic!("no note: {calls}"));
    assert!(note.ends_with("VISSUE_AGENT=saddle"), "{note}");
}

#[test]
fn an_assignee_that_is_another_seat_is_refused() {
    let b = Bench::new();
    for args in [
        &["claim", "proj-aaaa", "--assignee", "stranger"][..],
        &["sitting", "proj-aaaa", "--assignee", "stranger"],
        &[
            "finish",
            "proj-aaaa",
            "--lesson",
            "x",
            "--assignee",
            "stranger",
        ],
        &["complete", "proj-aaaa", "--assignee", "stranger"],
    ] {
        let out = b.ljos(Some("saddle"), args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} went through: {out:?}");
        assert!(
            err.contains("stranger is not this seat, saddle"),
            "{args:?}: {err}"
        );
    }
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

#[test]
fn release_hands_back_only_to_a_seat_the_seats_file_lists() {
    let b = Bench::new();
    b.seats(
        "\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n\
         \"1c2d3e4f-0000-4000-8000-000000000000\" = \"helper\"\n",
    );
    let refused = b.ljos(
        Some("saddle"),
        &["release", "proj-aaaa", "--assignee", "stranger"],
    );
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    let handed = b.ljos(
        Some("saddle"),
        &["release", "proj-aaaa", "--assignee", "helper"],
    );
    let err = String::from_utf8_lossy(&handed.stderr);
    assert!(!err.contains("no seat named"), "{err}");
    assert!(!err.contains("is not this seat"), "{err}");
}

#[test]
fn a_seats_array_is_the_list_of_seats() {
    let b = Bench::new();
    let note = ["note", "proj-aaaa", "progress"];
    b.seats("seats = [\"saddle\"]\n");
    let refused = b.ljos(Some("stranger"), &note);
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert!(err.contains("which lists saddle"), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    b.ljos(Some("saddle"), &note);
    assert!(b.calls().contains("vissue note"), "{}", b.calls());
}

#[test]
fn a_seats_file_that_lists_no_seat_refuses_every_write() {
    let b = Bench::new();
    let path = b.seats("# seats go here\n[owner]\nname = 1\n");
    let out = b.ljos(Some("saddle"), &["note", "proj-aaaa", "progress"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{out:?}");
    assert!(err.contains("lists no seat"), "{err}");
    assert!(err.contains(&path.display().to_string()), "{err}");
    assert!(err.contains("seats = [\"saddle\"]"), "names the fix: {err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}
