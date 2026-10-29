//! A consensus reading must not walk the tracker with git log -S.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;

#[test]
fn a_consensus_reading_does_not_pick_the_whole_history() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("git.log");
    let git = dir.path().join("git");
    std::fs::write(
        &git,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let rows = vec![serde_json::json!({"agent": "ada", "choice": "ship"})];
    let previous = std::env::var_os("PATH");
    unsafe { std::env::set_var("PATH", &path) };
    let _ = ljos_cli::push_cite_reading(
        "proj-fast",
        &rows,
        &std::collections::BTreeSet::new(),
        "ada",
    );
    let _ = ljos_cli::protected_push_reading("proj-fast", None);
    match previous {
        Some(value) => unsafe { std::env::set_var("PATH", value) },
        None => unsafe { std::env::remove_var("PATH") },
    }
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !text.contains("-S") && !text.contains("--full-history"),
        "consensus walked history:\n{text}"
    );
}
