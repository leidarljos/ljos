//! A raw holder and a legacy seat name do not hold a claim. The claim is
//! taken under the seat (`inky-n9o` as `inky`, `disk-saver-t4t` as
//! `disk-saver`, `ljos-bot` as `ljosbot`). `grokbot-sand-sub` is not a seat,
//! and nothing is written.

use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn log_has_claim(log: &str) -> bool {
    log.lines().any(|line| {
        line.contains("vissue claim") || line.contains("vissue create") || line.contains("claimdag")
    })
}

#[test]
fn a_raw_or_legacy_name_does_not_hold_the_claim() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let log_s = log.display();
    script(
        dir.path(),
        "claimdag",
        &format!(
            "#!/bin/sh\necho \"claimdag $*\" >> '{log_s}'\ncase \"$1\" in\n  claim) echo gen=2 ;;\nesac\nexit 0\n"
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
    let session = "0e610c5a-32e0-7dab-eb9b-ab7d13ce6556";
    let ljos = |seat: &str, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("LJOS_SEAT", seat)
            .env("GROK_SESSION_ID", session)
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap()
    };
    let read_log = || std::fs::read_to_string(&log).unwrap_or_default();
    let clear_log = || {
        let _ = std::fs::write(&log, "");
    };

    let out = ljos("inky-n9o", &["claim", "proj-inky"]);
    let calls = read_log();
    assert!(
        out.status.success(),
        "stderr {}\n{calls}",
        String::from_utf8_lossy(&out.stderr)
    );
    let holder = format!("inky@{}", ljos_cli::session_tag(session));
    assert!(
        calls.contains(&format!("vissue claim proj-inky VISSUE_AGENT={holder}")),
        "{calls}"
    );
    assert!(!calls.contains("inky-n9o"), "{calls}");
    clear_log();

    let out = ljos(
        "disk-saver",
        &["claim", "proj-disk", "--assignee", "disk-saver-t4t"],
    );
    let calls = read_log();
    assert!(
        out.status.success(),
        "stderr {}\n{calls}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        calls.contains("vissue claim proj-disk VISSUE_AGENT=disk-saver"),
        "{calls}"
    );
    assert!(!calls.contains("disk-saver-t4t"), "{calls}");
    let actor = ljos_cli::work_id("disk-saver:proj-disk");
    assert!(calls.contains(&format!("--assignee {actor}")), "{calls}");
    clear_log();

    let out = ljos("ljosbot", &["claim", "proj-bot", "--assignee", "ljos-bot"]);
    let calls = read_log();
    assert!(
        out.status.success(),
        "stderr {}\n{calls}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        calls.contains("vissue claim proj-bot VISSUE_AGENT=ljosbot"),
        "{calls}"
    );
    assert!(!calls.contains("ljos-bot"), "{calls}");
    clear_log();

    let out = ljos(
        "saddle",
        &["claim", "proj-sub", "--assignee", "grokbot-sand-sub"],
    );
    let calls = read_log();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "{said}\n{calls}");
    assert!(said.contains("no seat of its own"), "{said}");
    assert!(!log_has_claim(&calls), "{calls}");
    clear_log();

    let out = ljos("grokbot-sand-sub", &["claim", "proj-self"]);
    let calls = read_log();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "{said}\n{calls}");
    assert!(said.contains("no seat of its own"), "{said}");
    assert!(!log_has_claim(&calls), "{calls}");
}
