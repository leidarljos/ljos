//! A ballot needs a seat somebody named. A subagent with no `LJOS_SEAT`
//! cast 25 forecasts under `exec-daemon`, the program above its shell,
//! and nothing said so (Software-o9bu).

use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_ballot_from_a_shell_with_no_seat_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let log_s = log.display();
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\necho \"vissue $* VISSUE_AGENT=${{VISSUE_AGENT:-}}\" >> '{log_s}'\ncase \"$1 $3\" in\n  show*) echo '{{}}' ;;\n  'vote --json') echo '[{{\"agent\":\"reviewer\",\"choice\":\"a\"}}]' ;;\nesac\nexit 0\n"
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
    let ljos = |seat: Option<&str>, args: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("GROK_SESSION_ID", "2c12969d-0000-4000-8000-000000000000")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off");
        if let Some(seat) = seat {
            cmd.env("LJOS_SEAT", seat);
        }
        cmd.output().unwrap()
    };
    let vote = [
        "vote",
        "proj-aaaa",
        "--for",
        "a",
        "--used",
        "none",
        "--as",
        "reviewer",
    ];
    let refused = ljos(None, &vote);
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(
        !refused.status.success(),
        "a vote with no seat went through: {refused:?}"
    );
    assert!(err.contains("no seat is named"), "{err}");
    assert!(err.contains("LJOS_SEAT"), "names the fix: {err}");
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!calls.contains("vissue vote"), "nothing was cast: {calls}");

    // The stub tracker answers the read-back with the ballot, so the cast
    // is proved by the tracker, not by the exit of `vissue vote`.
    let cast = ljos(Some("saddle"), &vote);
    assert!(cast.status.success(), "{cast:?}");
    assert!(
        String::from_utf8_lossy(&cast.stdout).contains("recorded on the tracker: reviewer for a"),
        "{cast:?}"
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("vissue vote proj-aaaa --for a --used none VISSUE_AGENT=reviewer"),
        "{calls}"
    );
}
