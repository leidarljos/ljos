//! A consensus reading must not walk the tracker with git log -S.

ljos_test_boot::harness!();

#[test]
fn a_consensus_reading_does_not_pick_the_whole_history() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("git.log");
    let git = dir.path().join("git");
    ljos_cli::install_executable(
        &git,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let rows = vec![serde_json::json!({"agent": "ada", "choice": "ship"})];
    let saved: Vec<_> = ["PATH", "XDG_RUNTIME_DIR"]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
    unsafe {
        ljos_cli::test_env_guards::set_var_unchecked("PATH", &path);
        ljos_cli::test_env_guards::set_var_unchecked("XDG_RUNTIME_DIR", dir.path().join("run"));
    }
    let _ = ljos_cli::push_cite_reading(
        "proj-fast",
        &rows,
        &std::collections::BTreeSet::new(),
        "ada",
    );
    let _ = ljos_cli::protected_push_reading("proj-fast", None);
    for (key, value) in saved {
        match value {
            Some(value) => unsafe { ljos_cli::test_env_guards::set_var_unchecked(key, value) },
            None => unsafe { ljos_cli::test_env_guards::remove_var_unchecked(key) },
        }
    }
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !text.contains("-S") && !text.contains("--full-history"),
        "consensus walked history:\n{text}"
    );
}
