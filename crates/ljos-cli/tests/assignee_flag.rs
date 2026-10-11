//! A name passed to `--assignee` on the command line is the name the
//! claim is held under, the seat's own name too. Before, the seat's own
//! name fell through to the conversation's holder, so the tracker named a
//! session id and a board filtered by seat missed the claim.

use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_seat_named_on_the_command_line_holds_the_claim() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let log_s = log.display();
    script(
        dir.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\necho \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  claim) echo gen=2 ;;\n  complete) echo done ;;\nesac\nexit 0\n"
        ),
    );
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\necho \"vissue $* VISSUE_AGENT=${{VISSUE_AGENT:-}}\" >> '{log_s}'\ncase \"$1\" in\n  show) echo '{{}}' ;;\n  claim) echo claimed ;;\nesac\nexit 0\n"
        ),
    );
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
            .env("LJOS_SEAT", "saddle")
            .env("GROK_SESSION_ID", "0e610c5a-32e0-7dab-eb9b-ab7d13ce6556")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        assert!(out.status.success(), "ljos {args:?}: {out:?}");
    };
    ljos(&["claim", "proj-aaaa", "--assignee", "saddle"]);
    ljos(&[
        "complete",
        "proj-aaaa",
        "--assignee",
        "saddle",
        "--gen",
        "2",
    ]);
    let calls = std::fs::read_to_string(&log).unwrap();
    let node = ljos_cli::work_id("proj-aaaa");
    let actor = ljos_cli::work_id("saddle:proj-aaaa");
    assert!(
        calls.contains(&format!("claimdag claim {node} --assignee {actor}")),
        "{calls}"
    );
    assert!(
        calls.contains("vissue claim proj-aaaa VISSUE_AGENT=saddle"),
        "the tracker names the seat: {calls}"
    );
    assert!(
        calls.contains(&format!("claimdag complete {node} --actor {actor}")),
        "{calls}"
    );
}
