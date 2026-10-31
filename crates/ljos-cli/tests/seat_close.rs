//! A standing close grant lives in the tracker's vissue.toml.
//! The owner's quoted yes is the grant. A hand edit is not.

ljos_test_boot::harness!();
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

* STARTED [#C] scribe holds this
:PROPERTIES:
:ID:         proj-scribe
:CREATED:    [2026-10-11 Sun]
:CLAIMED_BY: scribe
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

fn finish_close(w: &World, seat: &str, issue: &str) -> std::process::Output {
    w.ljos(
        seat,
        &["finish", issue, "--close", "--gen", "2", "--assignee", seat],
        true,
    )
}

/// `ljos hook` under a runner, judging `line` and not running it.
fn hook_line(w: &World, session: &str, line: &str) -> Output {
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": session,
        "cwd": &w.root,
        "tool_name": "Bash",
        "tool_input": {"command": line},
    });
    let mut cmd = runner_cmd(w, &["hook"]);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn runner_cmd(w: &World, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env_clear()
        .env("PATH", &w.path)
        .env("HOME", &w.home)
        .env("XDG_CONFIG_HOME", w.home.join(".config"))
        .env("XDG_STATE_HOME", w.home.join(".state"))
        .env("XDG_RUNTIME_DIR", w.root.join("run"))
        .env("VISSUE_ROOT", &w.tracker)
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("LJOS_GUARD", "on")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("CURSOR_AGENT", "1")
        .stdin(Stdio::null());
    // SAFETY: setsid is async-signal-safe and the closure touches nothing
    // of the parent's.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = std::fs::create_dir_all(w.root.join("run"));
    cmd
}

fn runner_finish(w: &World, seat: &str, issue: &str) -> Output {
    runner_cmd(
        w,
        &["finish", issue, "--close", "--gen", "2", "--assignee", seat],
    )
    .env("LJOS_SEAT", seat)
    .output()
    .unwrap()
}

fn hook_allows(out: &Output) -> bool {
    out.status.success() && !ljos_cli::answer_denies(&String::from_utf8_lossy(&out.stdout))
}

fn log_updates(w: &World, issue: &str) -> usize {
    std::fs::read_to_string(&w.log)
        .unwrap()
        .matches(&format!("vissue update {issue} "))
        .count()
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

/// A grant that names tickets closes those and refuses the rest.
/// A row typed into the file does not count. `close=own` still does.
#[test]
fn a_named_list_closes_those_tickets_and_a_hand_edit_does_not() {
    let w = World::new();
    for name in ["clerk", "papers", "reachbook", "inky", "scribe"] {
        let out = w.ljos("clerk", &["seat", "add", name], false);
        assert!(out.status.success(), "add {name}: {}", text(&out));
    }
    let quote = "yes papers may close the named tickets";
    let at = "2026-10-29T00:00:00Z";
    let granted = w.ljos(
        "clerk",
        &[
            "seat",
            "grant",
            "papers",
            "close=proj-else,proj-filed",
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
    let doc: toml::Value = toml::from_str(&roster).unwrap();
    let row = &doc["seats"]["papers"];
    assert_eq!(
        row["close"].as_str(),
        Some("proj-else,proj-filed"),
        "{roster}"
    );
    assert_eq!(row["quote"].as_str(), Some(quote));
    assert_eq!(row["at"].as_str(), Some(at));
    assert_eq!(row["via"].as_str(), Some("clerk"));
    let commit = row["commit"].as_str().unwrap().to_string();
    assert_eq!(commit.len(), 40, "{commit}");

    let listed = finish_close(&w, "papers", "proj-else");
    let said = text(&listed);
    assert!(listed.status.success(), "listed close: {said}");
    assert!(said.contains("standing grant allowed this close"), "{said}");
    assert!(said.contains(quote), "{said}");
    assert!(said.contains(at), "{said}");
    assert!(said.contains("via: clerk"), "{said}");
    assert!(said.contains(&commit), "{said}");
    let calls = std::fs::read_to_string(&w.log).unwrap();
    assert!(
        calls.contains("vissue note proj-else standing grant allowed this close"),
        "{calls}"
    );
    assert!(calls.contains(quote) && calls.contains(&commit), "{calls}");
    assert!(calls.contains("vissue update proj-else -s DONE"), "{calls}");

    let also = finish_close(&w, "papers", "proj-filed");
    assert!(also.status.success(), "second listed id: {}", text(&also));
    assert_eq!(log_updates(&w, "proj-filed"), 1);

    let unlisted = finish_close(&w, "papers", "proj-own");
    let said = text(&unlisted);
    assert!(!unlisted.status.success(), "unlisted closed: {said}");
    assert!(
        said.contains("not papers's to close") || said.contains("not papers"),
        "{said}"
    );
    assert_eq!(log_updates(&w, "proj-own"), 0, "an unlisted id was written");

    let own = w.ljos(
        "papers",
        &[
            "seat",
            "grant",
            "clerk",
            "close=own",
            "--quote",
            "yes clerk may close its own",
            "--at",
            at,
            "--via",
            "papers",
        ],
        false,
    );
    assert!(own.status.success(), "close=own: {}", text(&own));
    let roster = std::fs::read_to_string(w.tracker.join("vissue.toml")).unwrap();
    let doc: toml::Value = toml::from_str(&roster).unwrap();
    assert_eq!(doc["seats"]["clerk"]["close"].as_str(), Some("own"));
    let clerk_close = finish_close(&w, "clerk", "proj-clerk");
    let said = text(&clerk_close);
    assert!(clerk_close.status.success(), "own close: {said}");
    assert!(said.contains("standing grant allowed this close"), "{said}");
    assert!(said.contains("yes clerk may close its own"), "{said}");
    assert_eq!(log_updates(&w, "proj-clerk"), 1);

    let ids = w.root.join("ids.txt");
    std::fs::write(&ids, "proj-hand\n\nproj-inky\n").unwrap();
    let from_file = w.ljos(
        "clerk",
        &[
            "seat",
            "grant",
            "reachbook",
            "--ids-file",
            ids.to_str().unwrap(),
            "--quote",
            "yes reachbook may close the file",
            "--at",
            at,
            "--via",
            "clerk",
        ],
        false,
    );
    assert!(from_file.status.success(), "ids file: {}", text(&from_file));
    let roster = std::fs::read_to_string(w.tracker.join("vissue.toml")).unwrap();
    let doc: toml::Value = toml::from_str(&roster).unwrap();
    assert_eq!(
        doc["seats"]["reachbook"]["close"].as_str(),
        Some("proj-hand,proj-inky"),
        "{roster}"
    );
    let file_commit = doc["seats"]["reachbook"]["commit"]
        .as_str()
        .unwrap()
        .to_string();
    let named = finish_close(&w, "reachbook", "proj-hand");
    let said = text(&named);
    assert!(named.status.success(), "file id: {said}");
    assert!(said.contains("yes reachbook may close the file"), "{said}");
    assert!(said.contains(&file_commit), "{said}");
    assert_eq!(log_updates(&w, "proj-hand"), 1);
    let other = finish_close(&w, "reachbook", "proj-own");
    let said = text(&other);
    assert!(
        !other.status.success(),
        "file grant closed another id: {said}"
    );
    assert_eq!(log_updates(&w, "proj-own"), 0);

    let mut doc: toml::Value =
        toml::from_str(&std::fs::read_to_string(w.tracker.join("vissue.toml")).unwrap()).unwrap();
    let seats = doc.get_mut("seats").unwrap().as_table_mut().unwrap();
    let mut row = toml::map::Map::new();
    row.insert("close".into(), toml::Value::String("proj-scribe".into()));
    row.insert(
        "quote".into(),
        toml::Value::String("typed in by hand".into()),
    );
    row.insert(
        "at".into(),
        toml::Value::String("2026-10-29T01:00:00Z".into()),
    );
    row.insert("via".into(), toml::Value::String("clerk".into()));
    row.insert("commit".into(), toml::Value::String(commit));
    seats.insert("scribe".into(), toml::Value::Table(row));
    let papers = seats.get_mut("papers").unwrap().as_table_mut().unwrap();
    papers.insert(
        "close".into(),
        toml::Value::String("proj-else,proj-own".into()),
    );
    std::fs::write(
        w.tracker.join("vissue.toml"),
        toml::to_string_pretty(&doc).unwrap(),
    )
    .unwrap();
    let handed = finish_close(&w, "scribe", "proj-scribe");
    let said = text(&handed);
    assert!(!handed.status.success(), "hand edit closed: {said}");
    assert!(said.contains("ljos seat grant"), "{said}");
    assert_eq!(
        log_updates(&w, "proj-scribe"),
        0,
        "a hand-written row closed"
    );
    let edited = finish_close(&w, "papers", "proj-own");
    let said = text(&edited);
    assert!(!edited.status.success(), "edited list closed: {said}");
    assert!(said.contains("ljos seat grant"), "{said}");
    assert_eq!(log_updates(&w, "proj-own"), 0, "a hand-edited list closed");
    let before = log_updates(&w, "proj-else");
    let again = finish_close(&w, "papers", "proj-else");
    assert!(
        !again.status.success(),
        "edited grant still closed: {}",
        text(&again)
    );
    assert_eq!(log_updates(&w, "proj-else"), before);
}

/// The hook used to refuse every finish close before finish could read a
/// grant. A grant that covers the ticket is allowed, and running it closes
/// the ticket and notes the grant. With no grant, finish refuses and the
/// ticket stays open. A raw tracker close under a runner is still refused.
#[test]
fn a_granted_close_passes_the_hook_and_an_ungranted_close_stays_open() {
    let w = World::new();
    for name in ["clerk", "s"] {
        let out = w.ljos("clerk", &["seat", "add", name], false);
        assert!(out.status.success(), "add {name}: {}", text(&out));
    }
    let quote = "yes s may close proj-own";
    let at = "2026-10-29T00:00:00Z";
    let granted = w.ljos(
        "clerk",
        &[
            "seat",
            "grant",
            "s",
            "close=proj-own",
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

    let line = "LJOS_SEAT=s ljos finish proj-own --close";
    let judged = hook_line(&w, "granted-close-own", line);
    let said = text(&judged);
    assert!(hook_allows(&judged), "granted close was refused:\n{said}");

    let closed = runner_finish(&w, "s", "proj-own");
    let said = text(&closed);
    assert!(closed.status.success(), "granted finish: {said}");
    assert!(said.contains("standing grant allowed this close"), "{said}");
    assert!(said.contains(quote), "{said}");
    assert!(said.contains(at), "{said}");
    assert!(said.contains("via: clerk"), "{said}");
    let calls = std::fs::read_to_string(&w.log).unwrap();
    assert!(
        calls.contains("vissue note proj-own standing grant allowed this close"),
        "{calls}"
    );
    assert!(calls.contains(quote), "{calls}");
    assert!(calls.contains("vissue update proj-own -s DONE"), "{calls}");

    let bare = "LJOS_SEAT=clerk ljos finish proj-clerk --close";
    let judged = hook_line(&w, "granted-close-none", bare);
    let said = text(&judged);
    assert!(
        hook_allows(&judged),
        "ungranted close was refused before finish:\n{said}"
    );
    let refused = runner_finish(&w, "clerk", "proj-clerk");
    let said = text(&refused);
    assert!(!refused.status.success(), "ungranted close wrote: {said}");
    assert!(said.contains("a person closes a ticket"), "{said}");
    assert!(said.contains("ljos seat grant"), "{said}");
    assert_eq!(log_updates(&w, "proj-clerk"), 0, "an ungranted close wrote");

    let raw = hook_line(&w, "granted-close-raw", "vissue close proj-clerk");
    let said = text(&raw);
    assert!(!hook_allows(&raw), "raw close was allowed:\n{said}");
    assert!(
        said.contains("ljos finish proj-clerk"),
        "raw close did not name finish:\n{said}"
    );
    assert!(
        !said.contains("--close"),
        "raw close named the flag:\n{said}"
    );
    assert_eq!(log_updates(&w, "proj-clerk"), 0, "a raw close wrote");
}
