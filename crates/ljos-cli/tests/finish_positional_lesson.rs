//! Agents write `ljos finish ISSUE "lesson"` without `--lesson`. Real model
//! runs in the terminal harness and the curl harness both did; clap refused it and
//! the sitting needed a second try. The positional lesson is now taken.

ljos_test_boot::harness!();
use std::process::Command;

#[test]
fn a_lesson_after_the_issue_is_taken_without_the_flag() {
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["finish", "Rig-0000", "a lesson in plain words", "--help"])
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(!text.contains("unexpected argument"), "{text}");
    let both = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["finish", "Rig-0000", "words", "--lesson", "flag"])
        .output()
        .unwrap();
    assert!(!both.status.success(), "a lesson given twice is refused");
    assert!(String::from_utf8_lossy(&both.stderr).contains("cannot be used with"));
}

/// Help, version and a missing subcommand are not memories. A private
/// home and state dir stay empty of a note queue.
#[test]
fn help_and_version_do_not_write_a_memory() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env("HOME", &home)
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_RUNTIME_DIR", &home)
            .env("VISSUE_ROOT", home.join("tracker"))
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_SEAT", "newbie")
            .output()
            .unwrap()
    };
    for args in [
        &["--help"][..],
        &["--version"][..],
        &["search", "--help"][..],
        &["finish", "--help"][..],
    ] {
        let out = run(args);
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success() || text.contains("Usage") || text.contains("ljos"),
            "{args:?} {text}"
        );
    }
    let missing = run(&[]);
    assert!(!missing.status.success(), "a bare ljos is a usage error");
    let state = home.join(".state").join("ljos");
    assert!(
        !state.join("note-queue.jsonl").exists(),
        "a usage exit queued a memory"
    );
    if state.exists() {
        for entry in std::fs::read_dir(&state).unwrap() {
            let path = entry.unwrap().path();
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !body.contains("Usage:"),
                "{} held a help screen",
                path.display()
            );
        }
    }
}
