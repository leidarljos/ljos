//! Releasing or completing a sitting drops the tracker's claim along with
//! the claim graph's. Before, the claim graph freed the node and the
//! tracker kept `CLAIMED_BY`, so `ljos claims` listed every finished
//! sitting as still held (Software-5lck). A release also puts the issue
//! back to the state it had before the claim, so it reads as available
//! again; a completion keeps the state. A claim a git hook took under the
//! bare seat is the same holder as `SEAT@TAG`.

ljos_test_boot::harness!();
use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const ORG: &str = "#+TITLE: proj issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* STARTED [#C] released\n:PROPERTIES:\n:ID:         proj-aaaa\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: saddle@main\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] completed\n:PROPERTIES:\n:ID:         proj-bbbb\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: saddle@main\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] someone else's\n:PROPERTIES:\n:ID:         proj-cccc\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: inky@main\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] hook claimed\n:PROPERTIES:\n:ID:         proj-dddd\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: saddle\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\
:LOGBOOK:\n- State \"STARTED\" from \"BLOCKED\" [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] hex id, nobody on it\n:PROPERTIES:\n:ID:         aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
:CREATED:    [2026-10-11 Sun]\n:END:\n";

fn heading<'a>(org: &'a str, id: &str) -> &'a str {
    let at = org.find(&format!(":ID:         {id}")).unwrap();
    let start = org[..at].rfind("\n* ").unwrap();
    let end = org[at..].find("\n* ").map_or(org.len(), |e| at + e);
    &org[start..end]
}

#[test]
fn release_reopens_and_complete_keeps_the_state() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let log_s = log.display();
    script(
        dir.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\necho \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  get) echo 'gen=2 state=claimed' ;;\n  release) echo gen=3 ;;\n  complete) echo done ;;\nesac\nexit 0\n"
        ),
    );
    script(
        dir.path(),
        "vissue",
        &format!("#!/bin/sh\necho \"vissue $*\" >> '{log_s}'\nexit 0\n"),
    );
    let tracker = dir.path().join("tracker");
    let file = tracker.join("Software/proj/issues.org");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, ORG).unwrap();
    let home = dir.path().join("home");
    let runtime = dir.path().join("run");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ljos = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("VISSUE_ROOT", &tracker)
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        assert!(out.status.success(), "ljos {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let released = ljos(&["release", "proj-aaaa", "--assignee", "saddle@main"]);
    let completed = ljos(&[
        "complete",
        "proj-bbbb",
        "--assignee",
        "saddle@main",
        "--gen",
        "2",
    ]);
    let other = ljos(&["release", "proj-cccc", "--assignee", "saddle@main"]);
    let bare = ljos(&["release", "proj-dddd", "--assignee", "saddle@main"]);
    // The id is 32 hex, so it is its own claim-graph node. Release still
    // has to leave TODO: STARTED with nobody on it is not a ready issue
    // and not a held one (ljos-cjd8).
    let hex_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert_eq!(hex_id.len(), 32, "the fixture id has to be 32 hex");
    let hex = ljos(&["release", hex_id, "--assignee", "saddle@main"]);
    let org = std::fs::read_to_string(&file).unwrap();
    for id in ["proj-aaaa", "proj-bbbb", "proj-dddd"] {
        let h = heading(&org, id);
        assert!(!h.contains(":CLAIMED_BY:"), "{id} still claimed:\n{org}");
    }
    assert!(
        heading(&org, "proj-aaaa").starts_with("\n* TODO"),
        "a release with no earlier state is TODO:\n{org}"
    );
    assert!(
        heading(&org, "proj-bbbb").starts_with("\n* STARTED"),
        "a completion keeps its state:\n{org}"
    );
    assert!(
        heading(&org, "proj-dddd").starts_with("\n* BLOCKED"),
        "a release goes back to the state before the claim:\n{org}"
    );
    assert!(
        released.contains("no longer claimed by saddle@main; back to TODO"),
        "{released}"
    );
    assert!(
        completed.contains("no longer claimed by saddle@main; its state stays"),
        "{completed}"
    );
    assert!(
        bare.contains("no longer claimed by saddle; back to BLOCKED"),
        "{bare}"
    );
    assert!(
        heading(&org, "proj-cccc").contains(":CLAIMED_BY: inky@main"),
        "another name's claim stays:\n{org}"
    );
    assert!(
        heading(&org, "proj-cccc").starts_with("\n* STARTED"),
        "another name's claim keeps its state:\n{org}"
    );
    assert!(other.contains("stays claimed by inky@main"), "{other}");
    assert!(
        heading(&org, hex_id).starts_with("\n* TODO"),
        "a release of a STARTED issue with nobody on it is TODO:\n{org}"
    );
    assert!(
        !heading(&org, hex_id).contains(":CLAIMED_BY:"),
        "the hex id grew a claimant:\n{org}"
    );
    assert!(
        hex.contains("nobody held it") || hex.contains("back to TODO"),
        "{hex}"
    );
}

/// `ljos finish` without `--close` ends the sitting the way a release
/// does: the claim goes, and the issue is back to the state before the
/// claim, so nothing reads STARTED with nobody on it (ljos-cjd8). A finish
/// with `--close` keeps the state for the DONE that follows, or the
/// CANCELLED of a cancelled sitting.
#[test]
fn finish_without_close_reopens_and_finish_with_close_does_not() {
    const FINISH_ORG: &str = "#+TITLE: proj issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* STARTED [#C] finished open\n:PROPERTIES:\n:ID:         proj-eeee\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: saddle@main\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] finished closed\n:PROPERTIES:\n:ID:         proj-ffff\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: saddle@main\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] finished cancelled\n:PROPERTIES:\n:ID:         proj-kkkk\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: saddle@main\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n";
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let log_s = log.display();
    script(
        dir.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\necho \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  get) echo 'gen=2 state=claimed' ;;\n  release) echo gen=3 ;;\n  complete) echo done ;;\nesac\nexit 0\n"
        ),
    );
    script(
        dir.path(),
        "vissue",
        &format!("#!/bin/sh\necho \"vissue $*\" >> '{log_s}'\nexit 0\n"),
    );
    let tracker = dir.path().join("tracker");
    let file = tracker.join("Software/proj/issues.org");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, FINISH_ORG).unwrap();
    let home = dir.path().join("home");
    let runtime = dir.path().join("run");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ljos = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("VISSUE_ROOT", &tracker)
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        assert!(out.status.success(), "ljos {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let open = ljos(&[
        "finish",
        "proj-eeee",
        "--assignee",
        "saddle@main",
        "--gen",
        "2",
    ]);
    let refuse = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("VISSUE_ROOT", &tracker)
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("CURSOR_AGENT", "1")
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.status.success(),
            "ljos {args:?} should label the close and write nothing: {text}"
        );
        assert!(
            !text.contains("label close-unverified") && text.contains("Nothing was written."),
            "ljos {args:?} close refusal format changed: {text}"
        );
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    let closed = refuse(&[
        "finish",
        "proj-ffff",
        "--assignee",
        "saddle@main",
        "--gen",
        "2",
        "--close",
    ]);
    let cancelled = refuse(&[
        "finish",
        "proj-kkkk",
        "--status",
        "cancelled",
        "--assignee",
        "saddle@main",
        "--gen",
        "2",
        "--close",
    ]);
    let org = std::fs::read_to_string(&file).unwrap();
    assert!(
        !heading(&org, "proj-eeee").contains(":CLAIMED_BY:"),
        "proj-eeee still claimed:\n{org}"
    );
    assert!(
        heading(&org, "proj-ffff").contains(":CLAIMED_BY:"),
        "a refused close still wrote:\n{org}"
    );
    assert!(
        heading(&org, "proj-kkkk").contains(":CLAIMED_BY:"),
        "a refused cancel still wrote:\n{org}"
    );
    assert!(
        heading(&org, "proj-eeee").starts_with("\n* TODO"),
        "a finish without a close hands the issue back:\n{org}"
    );
    assert!(
        open.contains("sitting ended; ticket proj-eeee is still open"),
        "{open}"
    );
    assert!(
        !open.contains("back to TODO"),
        "finish must not read as the work being undone: {open}"
    );
    assert!(!open.contains("stays open and nobody holds it"), "{open}");
    assert!(!open.contains("--close"), "{open}");
    assert!(closed.contains("a person closes a ticket"), "{closed}");
    assert!(closed.contains("Nothing was written"), "{closed}");
    assert!(
        cancelled.contains("a person closes a ticket"),
        "{cancelled}"
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        !calls.contains("vissue update proj-ffff -s DONE"),
        "{calls}"
    );
    assert!(
        !calls.contains("vissue update proj-kkkk -s CANCELLED"),
        "{calls}"
    );
}

/// The claim graph refuses a release of a node nobody holds there: a
/// lease reclaim already freed it, a completion ended it, or the graph
/// never had it. The tracker can still read STARTED under this holder or
/// under nobody, and a release is the only verb that hands it back, so it
/// still has to (ljos-cjd8). A heading another name holds, or one that is
/// not STARTED and unclaimed, is left alone and the release still fails.
#[test]
fn release_hands_the_tracker_back_when_the_claim_graph_holds_nothing() {
    const UNHELD_ORG: &str = "#+TITLE: proj issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* STARTED [#C] lease reclaimed\n:PROPERTIES:\n:ID:         proj-gggg\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: saddle@main\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] completed, nobody on it\n:PROPERTIES:\n:ID:         proj-hhhh\n:CREATED:    [2026-10-11 Sun]\n:END:\n\
:LOGBOOK:\n- State \"STARTED\" from \"BLOCKED\" [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] hex id the graph never had\n:PROPERTIES:\n:ID:         bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\
:CREATED:    [2026-10-11 Sun]\n:END:\n\n\
* STARTED [#C] someone else's\n:PROPERTIES:\n:ID:         proj-iiii\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: inky@main\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* TODO [#C] nothing to hand back\n:PROPERTIES:\n:ID:         proj-jjjj\n:CREATED:    [2026-10-11 Sun]\n:END:\n\n\
* TODO [#C] session already finished\n:PROPERTIES:\n\
:ID:         cccccccccccccccccccccccccccccccc\n:CREATED:    [2026-10-11 Sun]\n:END:\n";
    let dir = tempfile::tempdir().unwrap();
    // The refusals claimdag 0.8 gives for a node that is not claimed or
    // running, and for an id it has no node for.
    script(
        dir.path(),
        "claimdag",
        "#!/bin/sh\ncase \"$1\" in\n  release)\n    case \"$2\" in\n      bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb) echo 'release: not found' >&2 ;;\n      cccccccccccccccccccccccccccccccc) echo 'release: status done' >&2 ;;\n      *) echo 'release: status ready' >&2 ;;\n    esac\n    exit 1 ;;\nesac\nexit 0\n",
    );
    script(dir.path(), "vissue", "#!/bin/sh\nexit 0\n");
    let tracker = dir.path().join("tracker");
    let file = tracker.join("Software/proj/issues.org");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, UNHELD_ORG).unwrap();
    let home = dir.path().join("home");
    let runtime = dir.path().join("run");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let release = |id: &str| {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["release", id, "--assignee", "saddle@main"])
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("VISSUE_ROOT", &tracker)
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap()
    };
    let hex_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    for (id, back) in [
        (
            "proj-gggg",
            "no longer claimed by saddle@main; back to TODO",
        ),
        ("proj-hhhh", "back to BLOCKED; nobody held it"),
        (hex_id, "back to TODO; nobody held it"),
    ] {
        let out = release(id);
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "release {id}: {out:?}");
        assert!(said.contains(back), "release {id}: {said}");
    }
    for id in ["proj-iiii", "proj-jjjj"] {
        let out = release(id);
        assert!(!out.status.success(), "release {id} passed: {out:?}");
    }
    let already_done = release("cccccccccccccccccccccccccccccccc");
    assert!(
        already_done.status.success(),
        "release of finished session: {already_done:?}"
    );
    assert!(already_done.stdout.is_empty(), "{already_done:?}");
    assert!(already_done.stderr.is_empty(), "{already_done:?}");
    let org = std::fs::read_to_string(&file).unwrap();
    for (id, state) in [
        ("proj-gggg", "TODO"),
        ("proj-hhhh", "BLOCKED"),
        (hex_id, "TODO"),
        ("proj-iiii", "STARTED"),
        ("proj-jjjj", "TODO"),
    ] {
        assert!(
            heading(&org, id).starts_with(&format!("\n* {state}")),
            "{id} is not {state}:\n{org}"
        );
    }
    for id in ["proj-gggg", "proj-hhhh", hex_id] {
        assert!(
            !heading(&org, id).contains(":CLAIMED_BY:"),
            "{id} still claimed:\n{org}"
        );
    }
    assert!(
        heading(&org, "proj-iiii").contains(":CLAIMED_BY: inky@main"),
        "another name's claim went:\n{org}"
    );
}

struct LiveSleeper(std::process::Child);

impl LiveSleeper {
    fn spawn() -> Self {
        let child = Command::new("sleep")
            .arg("30")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for LiveSleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn finish_takes_over_older_holders_of_same_seat_and_respects_live_and_other_seats() {
    const TAKEOVER_ORG: &str = "#+TITLE: Software issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* STARTED [#C] graph already reclaimed\n:PROPERTIES:\n:ID:         Software-oc9m\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: dseams-paper@dorx1f91mq\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] graph held, hold file gone\n:PROPERTIES:\n:ID:         Software-oc9n\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: dseams-paper@dorx1f91mq\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] dead session hold file\n:PROPERTIES:\n:ID:         Software-oc9o\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: dseams-paper@dorx1f91mq\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] live conversation on this host\n:PROPERTIES:\n:ID:         Software-oc9p\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: dseams-paper@live111111\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] other seat\n:PROPERTIES:\n:ID:         Software-oc9q\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: other-seat@dorx1f91mq\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n";

    let dir = tempfile::tempdir().unwrap();
    let oc9m = ljos_cli::work_id("Software-oc9m");
    let oc9n = ljos_cli::work_id("Software-oc9n");
    let oc9o = ljos_cli::work_id("Software-oc9o");
    let oc9p = ljos_cli::work_id("Software-oc9p");
    let oc9q = ljos_cli::work_id("Software-oc9q");
    let actor_oc9n = ljos_cli::work_id("dseams-paper@dorx1f91mq:Software-oc9n");
    let actor_oc9o = ljos_cli::work_id("dseams-paper@dorx1f91mq:Software-oc9o");
    let actor_oc9p = ljos_cli::work_id("dseams-paper@live111111:Software-oc9p");
    let actor_oc9q = ljos_cli::work_id("other-seat@dorx1f91mq:Software-oc9q");

    script(
        dir.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\n\
case \"$1\" in\n\
  get)\n\
    case \"$2\" in\n\
      {oc9m}) echo \"$2  ready  task  unset  gen=2  assignee=00000000000000000000000000000000  Software-oc9m\" ;;\n\
      {oc9n}) echo \"$2  claimed  task  unset  gen=2  assignee={actor_oc9n}  Software-oc9n\" ;;\n\
      {oc9o}) echo \"$2  claimed  task  unset  gen=2  assignee={actor_oc9o}  Software-oc9o\" ;;\n\
      {oc9p}) echo \"$2  claimed  task  unset  gen=2  assignee={actor_oc9p}  Software-oc9p\" ;;\n\
      {oc9q}) echo \"$2  claimed  task  unset  gen=2  assignee={actor_oc9q}  Software-oc9q\" ;;\n\
    esac ;;\n\
  release|complete)\n\
    actor=$4\n\
    case \"$2\" in\n\
      {oc9m}) echo \"$2 done\" ;;\n\
      {oc9n}) [ \"$actor\" = '{actor_oc9n}' ] || {{ echo 'complete: not assignee' >&2; exit 1; }}; echo \"$2 done\" ;;\n\
      {oc9o}) [ \"$actor\" = '{actor_oc9o}' ] || {{ echo 'complete: not assignee' >&2; exit 1; }}; echo \"$2 done\" ;;\n\
      {oc9p}) [ \"$actor\" = '{actor_oc9p}' ] || {{ echo 'complete: not assignee' >&2; exit 1; }}; echo \"$2 done\" ;;\n\
      {oc9q}) [ \"$actor\" = '{actor_oc9q}' ] || {{ echo 'complete: not assignee' >&2; exit 1; }}; echo \"$2 done\" ;;\n\
    esac ;;\n\
esac\nexit 0\n"
        ),
    );
    script(
        dir.path(),
        "vissue",
        "#!/bin/sh\necho '{\"state\":\"STARTED\"}' ; exit 0\n",
    );

    let tracker = dir.path().join("tracker");
    let file = tracker.join("Software/Software/issues.org");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, TAKEOVER_ORG).unwrap();
    let home = dir.path().join("home");
    let runtime = dir.path().join("run");
    let ljos_run = runtime.join("ljos");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&ljos_run).unwrap();
    std::fs::write(
        ljos_run.join(format!("hold-{actor_oc9o}")),
        "dseams-paper@dorx1f91mq:Software-oc9o\ndseams-paper\n4194303\nno-such-runner\n2026-10-11T05:00:00Z\nSoftware-oc9o\n-\n",
    )
    .unwrap();
    let sleeper = LiveSleeper::spawn();
    let live_hold = ljos_run.join(format!("hold-{actor_oc9p}"));
    std::fs::write(
        &live_hold,
        format!(
            "dseams-paper@live111111:Software-oc9p\ndseams-paper\n{}\nsleep\n2026-10-11T05:00:00Z\nSoftware-oc9p\n-\n",
            sleeper.pid(),
        ),
    )
    .unwrap();

    let path = format!("{}:/usr/bin:/bin", dir.path().display());
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("VISSUE_ROOT", &tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "dseams-paper")
            .env("CURSOR_CONVERSATION_ID", "li41adshxv-current-session")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap()
    };

    for id in ["Software-oc9m", "Software-oc9n", "Software-oc9o"] {
        let out = run(&["finish", id]);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "finish {id}: {said}");
        assert!(
            !said.contains("stays claimed"),
            "finish {id} stayed claimed: {said}"
        );
        let takeover_lines: Vec<_> = said
            .lines()
            .filter(|l| l.contains("dseams-paper@dorx1f91mq"))
            .collect();
        assert_eq!(
            takeover_lines.len(),
            1,
            "finish {id} should print one line naming the old holder:\n{said}"
        );
    }

    for id in ["Software-oc9p", "Software-oc9q"] {
        let out = run(&["finish", id]);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.status.success(), "finish {id} should refuse: {said}");
    }

    let org = std::fs::read_to_string(&file).unwrap();
    for id in ["Software-oc9m", "Software-oc9n", "Software-oc9o"] {
        let h = heading(&org, id);
        assert!(h.starts_with("\n* TODO"), "{id} not TODO:\n{org}");
        assert!(!h.contains(":CLAIMED_BY:"), "{id} still claimed:\n{org}");
    }
    assert!(
        heading(&org, "Software-oc9p").contains(":CLAIMED_BY: dseams-paper@live111111"),
        "live conversation claim was dropped:\n{org}"
    );
    assert!(
        live_hold.is_file(),
        "live conversation hold record was dropped"
    );
    assert!(
        heading(&org, "Software-oc9q").contains(":CLAIMED_BY: other-seat@dorx1f91mq"),
        "other seat claim was dropped:\n{org}"
    );
}

/// A live process recorded under the bare seat is another conversation.
/// The tagged name of this seat must not finish or release that claim.
#[test]
fn finish_and_release_refuse_a_live_bare_seat_claim() {
    const ORG: &str = "#+TITLE: Software issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* STARTED [#C] live bare seat finish\n:PROPERTIES:\n:ID:         Software-bare\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: dseams-paper\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n\n\
* STARTED [#C] live bare seat release\n:PROPERTIES:\n:ID:         Software-bar2\n:CREATED:    [2026-10-11 Sun]\n\
:CLAIMED_BY: dseams-paper\n:CLAIMED_AT: [2026-10-11 Sun 05:00]\n:END:\n";

    let dir = tempfile::tempdir().unwrap();
    let bare = ljos_cli::work_id("Software-bare");
    let bar2 = ljos_cli::work_id("Software-bar2");
    let actor_bare = ljos_cli::work_id("dseams-paper:Software-bare");
    let actor_bar2 = ljos_cli::work_id("dseams-paper:Software-bar2");
    script(
        dir.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\n\
case \"$1\" in\n\
  get)\n\
    case \"$2\" in\n\
      {bare}) echo \"$2  claimed  task  unset  gen=2  assignee={actor_bare}  Software-bare\" ;;\n\
      {bar2}) echo \"$2  claimed  task  unset  gen=2  assignee={actor_bar2}  Software-bar2\" ;;\n\
    esac ;;\n\
  release|complete)\n\
    actor=$4\n\
    case \"$2\" in\n\
      {bare}) [ \"$actor\" = '{actor_bare}' ] || {{ echo \"$1: not assignee\" >&2; exit 1; }}; echo \"$2 done\" ;;\n\
      {bar2}) [ \"$actor\" = '{actor_bar2}' ] || {{ echo \"$1: not assignee\" >&2; exit 1; }}; echo \"$2 done\" ;;\n\
    esac ;;\n\
esac\nexit 0\n"
        ),
    );
    script(
        dir.path(),
        "vissue",
        "#!/bin/sh\necho '{{\"state\":\"STARTED\"}}' ; exit 0\n",
    );
    let tracker = dir.path().join("tracker");
    let file = tracker.join("Software/Software/issues.org");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, ORG).unwrap();
    let home = dir.path().join("home");
    let runtime = dir.path().join("run");
    let ljos_run = runtime.join("ljos");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&ljos_run).unwrap();
    let sleeper = LiveSleeper::spawn();
    for (actor, id) in [
        (actor_bare.as_str(), "Software-bare"),
        (actor_bar2.as_str(), "Software-bar2"),
    ] {
        std::fs::write(
            ljos_run.join(format!("hold-{actor}")),
            format!(
                "dseams-paper:{id}\ndseams-paper\n{}\nsleep\n2026-10-11T05:00:00Z\n{id}\n-\n",
                sleeper.pid()
            ),
        )
        .unwrap();
    }
    let path = format!("{}:/usr/bin:/bin", dir.path().display());
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("VISSUE_ROOT", &tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "dseams-paper")
            .env("CURSOR_CONVERSATION_ID", "li41adshxv-current-session")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap()
    };
    for (cmd, id) in [("finish", "Software-bare"), ("release", "Software-bar2")] {
        let out = run(&[cmd, id]);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !out.status.success(),
            "{cmd} {id} stole a live claim:\n{said}"
        );
        assert!(
            said.contains("still running")
                || said.contains("another conversation")
                || said.contains("stays claimed"),
            "{cmd} {id} did not name the live claim:\n{said}"
        );
    }
    let org = std::fs::read_to_string(&file).unwrap();
    for id in ["Software-bare", "Software-bar2"] {
        assert!(
            heading(&org, id).contains(":CLAIMED_BY: dseams-paper"),
            "{id} claim was cleared:\n{org}"
        );
        assert!(
            heading(&org, id).starts_with("\n* STARTED"),
            "{id} left STARTED:\n{org}"
        );
    }
}
