//! Agents write `ljos finish ISSUE "lesson"` without `--lesson`. Real model
//! runs in Claude Code and the curl harness both did; clap refused it and
//! the sitting needed a second try. The positional lesson is now taken.

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
