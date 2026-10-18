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
:LOGBOOK:\n- State \"STARTED\" from \"BLOCKED\" [2026-10-11 Sun 05:00]\n:END:\n";

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
}
