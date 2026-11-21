//! The degraded line names `ljos doctor` inside backticks, so the text
//! that quotes the whole line wraps it in a double backtick span. A single
//! span around `ljos doctor` ends at the first inner backtick, and the
//! full stop after it is left as a stray span opener.

ljos_test_boot::harness!();

use std::path::Path;
use std::process::Command;

const BROKEN: &str = "`ljos doctor`.`";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root")
}

#[test]
fn no_text_closes_its_code_span_on_the_doctor_name() {
    let mut bad = Vec::new();
    for file in [
        "README.md",
        "crates/ljos-cli/doc/protocol.md",
        "skills/ljos/SKILL.md",
        "crates/ljos-cli/src/main.rs",
    ] {
        let text = std::fs::read_to_string(root().join(file)).unwrap();
        for (n, line) in text.lines().enumerate() {
            if line.contains(BROKEN) {
                bad.push(format!("{file}:{}", n + 1));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "single span around the doctor line: {bad:?}"
    );
}

#[test]
fn the_help_for_search_and_remember_has_whole_code_spans() {
    for command in ["search", "remember"] {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args([command, "--help"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        let help = String::from_utf8_lossy(&out.stdout);
        assert!(help.contains("ljos doctor"), "{command}: {help}");
        assert!(!help.contains(BROKEN), "{command}: {help}");
    }
}
