//! `ljos protocol` is the short form. `--full` is the whole text.

ljos_test_boot::harness!();

use std::process::Command;

fn protocol(home: &std::path::Path, full: bool) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("protocol");
    if full {
        cmd.arg("--full");
    }
    let out = cmd
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("PACKSET_URL", "off")
        .output()
        .expect("ljos");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

#[test]
fn protocol_is_short_and_full_is_the_whole_text() {
    let home = tempfile::tempdir().expect("home");
    let short = protocol(home.path(), false);
    let lines = short.lines().count();
    assert!(lines <= 60, "ljos protocol printed {lines} lines:\n{short}");
    for step in [
        "ljos sitting",
        "ljos note",
        "ljos remember",
        "ljos finish",
        "ljos protocol --full",
    ] {
        assert!(short.contains(step), "{step} missing:\n{short}");
    }
    let full = protocol(home.path(), true);
    assert!(
        full.lines().count() > 200,
        "full text was not the long protocol"
    );
    assert!(full.contains("# The seat protocol"));
    assert!(full.contains("== blockers"));
    assert!(full.lines().count() > lines);
}
