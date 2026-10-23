//! Releasing or completing a sitting drops the tracker's claim along with
//! the claim graph's. Before, the claim graph freed the node and the
//! tracker kept `CLAIMED_BY`, so `ljos claims` listed every finished
//! sitting as still held (Software-5lck). A release also puts the issue
//! back to the state it had before the claim, so it reads as available
//! again; a completion keeps the state. A claim a git hook took under the
//! bare seat is the same holder as `SEAT@TAG`.

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
            "#!/bin/sh\necho \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  release) echo gen=3 ;;\n  complete) echo done ;;\nesac\nexit 0\n"
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
            "#!/bin/sh\necho \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  release) echo gen=3 ;;\n  complete) echo done ;;\nesac\nexit 0\n"
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
    let closed = ljos(&[
        "finish",
        "proj-ffff",
        "--assignee",
        "saddle@main",
        "--gen",
        "2",
        "--close",
    ]);
    let cancelled = ljos(&[
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
    for id in ["proj-eeee", "proj-ffff", "proj-kkkk"] {
        assert!(
            !heading(&org, id).contains(":CLAIMED_BY:"),
            "{id} still claimed:\n{org}"
        );
    }
    assert!(
        heading(&org, "proj-eeee").starts_with("\n* TODO"),
        "a finish without --close hands the issue back:\n{org}"
    );
    assert!(
        open.contains("no longer claimed by saddle@main; back to TODO"),
        "{open}"
    );
    assert!(open.contains("stays open and nobody holds it"), "{open}");
    assert!(
        closed.contains("no longer claimed by saddle@main; its state stays"),
        "{closed}"
    );
    assert!(
        cancelled.contains("closed the ticket proj-kkkk as CANCELLED"),
        "{cancelled}"
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains("vissue update proj-ffff -s DONE"), "{calls}");
    assert!(
        calls.contains("vissue update proj-kkkk -s CANCELLED"),
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
* TODO [#C] nothing to hand back\n:PROPERTIES:\n:ID:         proj-jjjj\n:CREATED:    [2026-10-11 Sun]\n:END:\n";
    let dir = tempfile::tempdir().unwrap();
    // The refusals claimdag 0.8 gives for a node that is not claimed or
    // running, and for an id it has no node for.
    script(
        dir.path(),
        "claimdag",
        "#!/bin/sh\ncase \"$1\" in\n  release)\n    case \"$2\" in\n      bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb) echo 'release: not found' >&2 ;;\n      *) echo 'release: status ready' >&2 ;;\n    esac\n    exit 1 ;;\nesac\nexit 0\n",
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
