//! A standing close grant lives in the tracker's vissue.toml.
//! The owner's quoted yes is the grant. A hand edit is not.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const ORG: &str = "\
#+TITLE: proj issues
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* STARTED [#C] papers holds this
:PROPERTIES:
:ID:         proj-own
:CREATED:    [2026-10-11 Sun]
:CLAIMED_BY: papers
:CLAIMED_AT: [2026-10-11 Sun 05:00]
:END:

* STARTED [#C] inky holds this
:PROPERTIES:
:ID:         proj-else
:CREATED:    [2026-10-11 Sun]
:CLAIMED_BY: inky
:CLAIMED_AT: [2026-10-11 Sun 05:00]
:END:

* TODO [#C] papers filed this
:PROPERTIES:
:ID:         proj-filed
:CREATED:    [2026-10-11 Sun]
:END:
:LOGBOOK:
- Filed by papers [2026-10-11 Sun 05:00]
:END:

* STARTED [#C] reachbook holds this
:PROPERTIES:
:ID:         proj-hand
:CREATED:    [2026-10-11 Sun]
:CLAIMED_BY: reachbook
:CLAIMED_AT: [2026-10-11 Sun 05:00]
:END:

* STARTED [#C] clerk holds this
:PROPERTIES:
:ID:         proj-clerk
:CREATED:    [2026-10-11 Sun]
:CLAIMED_BY: clerk
:CLAIMED_AT: [2026-10-11 Sun 05:00]
:END:
";

struct World {
    root: PathBuf,
    tracker: PathBuf,
    home: PathBuf,
    path: String,
    log: PathBuf,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        let tracker = root.join("tracker");
        let home = root.join("home");
        let bin = root.join("bin");
        std::fs::create_dir_all(&tracker).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let log = root.join("calls.log");
        let log_s = log.display();
        script(
            &bin,
            "claimdag",
            &format!(
                "#!/bin/sh\necho \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  complete) echo done ;;\nesac\nexit 0\n"
            ),
        );
        script(
            &bin,
            "vissue",
            &format!("#!/bin/sh\necho \"vissue $*\" >> '{log_s}'\nexit 0\n"),
        );
        let file = tracker.join("Software/proj/issues.org");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, ORG).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&tracker)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", &tracker)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Rohit Goswami"]);
        git(&["config", "user.email", "rgoswami@ieee.org"]);
        git(&["config", "commit.gpgsign", "false"]);
        git(&["config", "core.hooksPath", "/dev/null"]);
        let path = format!("{}:/usr/bin:/bin", bin.display());
        Self {
            root,
            tracker,
            home,
            path,
            log,
        }
    }

    fn ljos(&self, seat: &str, args: &[&str], runner: bool) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("VISSUE_ROOT", &self.tracker)
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off");
        if runner {
            cmd.env("CURSOR_AGENT", "1");
        }
        cmd.output().unwrap()
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
fn a_quoted_grant_closes_own_tickets_and_a_hand_edit_does_not() {
    let w = World::new();
    assert!(
        !w.home.join(".config/ljos/seats.toml").exists(),
        "a fresh home has no seats file"
    );
    for name in ["clerk", "papers", "reachbook"] {
        let out = w.ljos("clerk", &["seat", "add", name], false);
        assert!(out.status.success(), "add {name}: {}", text(&out));
    }
    let quote = "yes papers may close its own";
    let at = "2026-10-29T00:00:00Z";
    let granted = w.ljos(
        "clerk",
        &[
            "seat",
            "grant",
            "papers",
            "close=own",
            "--quote",
            quote,
            "--at",
            at,
            "--via",
            "clerk",
        ],
        false,
    );
    assert!(granted.status.success(), "grant: {}", text(&granted));
    let roster = std::fs::read_to_string(w.tracker.join("vissue.toml")).unwrap();
    assert!(roster.contains(quote), "{roster}");
    assert!(roster.contains(at), "{roster}");
    assert!(roster.contains("via = \"clerk\""), "{roster}");
    let commit = roster
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("commit = ")
                .map(|v| v.trim_matches('"'))
        })
        .unwrap_or_else(|| panic!("no commit in {roster}"))
        .to_string();
    assert_eq!(commit.len(), 40, "{commit}");

    let own = w.ljos(
        "papers",
        &[
            "finish",
            "proj-own",
            "--close",
            "--gen",
            "2",
            "--assignee",
            "papers",
        ],
        true,
    );
    let said = text(&own);
    assert!(own.status.success(), "own close: {said}");
    assert!(said.contains("standing grant allowed this close"), "{said}");
    assert!(said.contains(quote), "{said}");
    assert!(said.contains(at), "{said}");
    assert!(said.contains("via: clerk"), "{said}");
    assert!(said.contains(&commit), "{said}");
    let calls = std::fs::read_to_string(&w.log).unwrap();
    assert!(
        calls.contains("vissue note proj-own standing grant allowed this close"),
        "{calls}"
    );
    assert!(calls.contains(quote), "{calls}");
    assert!(calls.contains(&commit), "{calls}");
    assert!(calls.contains("vissue update proj-own -s DONE"), "{calls}");

    let filed = w.ljos(
        "papers",
        &[
            "finish",
            "proj-filed",
            "--close",
            "--gen",
            "2",
            "--assignee",
            "papers",
        ],
        true,
    );
    assert!(filed.status.success(), "filed close: {}", text(&filed));
    assert!(
        std::fs::read_to_string(&w.log)
            .unwrap()
            .contains("vissue update proj-filed -s DONE"),
        "{}",
        std::fs::read_to_string(&w.log).unwrap()
    );

    let other = w.ljos(
        "papers",
        &[
            "finish",
            "proj-else",
            "--close",
            "--gen",
            "2",
            "--assignee",
            "papers",
        ],
        true,
    );
    let said = text(&other);
    assert!(!other.status.success(), "other ticket closed: {said}");
    assert!(
        said.contains("not papers's to close") || said.contains("not papers"),
        "{said}"
    );
    assert!(
        !std::fs::read_to_string(&w.log)
            .unwrap()
            .contains("vissue update proj-else"),
        "the other ticket was written"
    );

    let ungranted = w.ljos(
        "clerk",
        &[
            "finish",
            "proj-clerk",
            "--close",
            "--gen",
            "2",
            "--assignee",
            "clerk",
        ],
        true,
    );
    let said = text(&ungranted);
    assert!(!ungranted.status.success(), "ungranted closed: {said}");
    assert!(said.contains("ljos seat grant"), "{said}");
    assert!(said.contains("a person closes a ticket"), "{said}");
    assert!(
        !std::fs::read_to_string(&w.log)
            .unwrap()
            .contains("vissue update proj-clerk"),
        "an ungranted close wrote"
    );

    let mut doc: toml::Value =
        toml::from_str(&std::fs::read_to_string(w.tracker.join("vissue.toml")).unwrap()).unwrap();
    let seats = doc.get_mut("seats").unwrap().as_table_mut().unwrap();
    let mut row = toml::map::Map::new();
    row.insert("close".into(), toml::Value::String("own".into()));
    row.insert(
        "quote".into(),
        toml::Value::String("typed in by hand".into()),
    );
    row.insert(
        "at".into(),
        toml::Value::String("2026-10-29T01:00:00Z".into()),
    );
    row.insert("via".into(), toml::Value::String("clerk".into()));
    row.insert("commit".into(), toml::Value::String(commit.clone()));
    seats.insert("reachbook".into(), toml::Value::Table(row));
    std::fs::write(
        w.tracker.join("vissue.toml"),
        toml::to_string_pretty(&doc).unwrap(),
    )
    .unwrap();
    let handed = w.ljos(
        "reachbook",
        &[
            "finish",
            "proj-hand",
            "--close",
            "--gen",
            "2",
            "--assignee",
            "reachbook",
        ],
        true,
    );
    let said = text(&handed);
    assert!(!handed.status.success(), "hand edit closed: {said}");
    assert!(said.contains("ljos seat grant"), "{said}");
    assert!(
        !std::fs::read_to_string(&w.log)
            .unwrap()
            .contains("vissue update proj-hand"),
        "a hand-edited grant closed the ticket"
    );

    let self_grant = w.ljos(
        "papers",
        &[
            "seat",
            "grant",
            "papers",
            "close=own",
            "--quote",
            "papers grants papers",
            "--at",
            at,
            "--via",
            "papers",
        ],
        false,
    );
    let said = text(&self_grant);
    assert!(!self_grant.status.success(), "self grant: {said}");
    assert!(said.contains("cannot grant itself"), "{said}");
}
