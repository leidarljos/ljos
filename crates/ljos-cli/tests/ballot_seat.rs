//! A ballot needs a seat somebody named. A subagent with no `LJOS_SEAT`
//! cast 25 forecasts under `exec-daemon`, the program above its shell,
//! and nothing said so (Software-o9bu).

ljos_test_boot::harness!();
use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_persona_ballot_is_recorded_when_no_seat_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let log_s = log.display();
    let drawer = dir.path().join("drawer.json");
    let drawer_s = drawer.display();
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\necho \"vissue $* VISSUE_AGENT=${{VISSUE_AGENT:-}}\" >> '{log_s}'\ncase \"$1 $3\" in\n  show*) echo '{{}}' ;;\n  'vote --for') echo \"[{{\\\"agent\\\":\\\"$VISSUE_AGENT\\\",\\\"choice\\\":\\\"$4\\\"}}]\" > '{drawer_s}' ;;\n  'vote --json') cat '{drawer_s}' 2>/dev/null || echo '[]' ;;\nesac\nexit 0\n"
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
    // No LJOS_SEAT: `--as reviewer` still records the persona. The derived
    // seat is the caster, named on the provenance line.
    let cast = ljos(None, &vote);
    let said = String::from_utf8_lossy(&cast.stdout);
    let err = String::from_utf8_lossy(&cast.stderr);
    assert!(cast.status.success(), "{cast:?}");
    assert!(!err.contains("no seat is named"), "{err}");
    assert!(
        said.contains("recorded on the tracker: reviewer for a"),
        "{cast:?}"
    );
    assert!(said.contains("reviewer cast by "), "{cast:?}");
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("vissue vote proj-aaaa --for a --used none VISSUE_AGENT=reviewer"),
        "{calls}"
    );
}
