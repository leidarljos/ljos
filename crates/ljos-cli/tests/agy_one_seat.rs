//! A harness id sits as the roster seat that belongs to that harness.
//! A seat a person named after a harness is not taken by the harness id.

fn tracker(seats: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    // The person's global config may carry commit hooks; the fixture's
    // repository reads none of it.
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("vissue.toml"), seats).unwrap();
    git(&["add", "vissue.toml"]);
    git(&["config", "user.email", "ada@example.invalid"]);
    git(&["config", "user.name", "ada"]);
    git(&["commit", "-q", "-m", "roster"]);
    std::env::set_var("VISSUE_ROOT", dir.path());
    std::env::set_var("ISSUE_ROOT", dir.path());
    std::env::set_var("VISSUE_NO_ROUTE", "1");
    dir
}

fn seat_of() -> String {
    ljos_cli::resolve_call_seat(None).unwrap().seat
}

#[test]
fn harness_ids_follow_the_vissue_roster() {
    let saved: Vec<(&str, Option<std::ffi::OsString>)> =
        ["VISSUE_ROOT", "ISSUE_ROOT", "VISSUE_NO_ROUTE", "LJOS_SEAT"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect();
    struct PutBack(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for PutBack {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let _restore = PutBack(saved);
    let _listed = tracker("seats = [\"antigravity\", \"free-agy\"]\n");
    std::env::set_var("LJOS_SEAT", "antigravity");
    assert_eq!(
        seat_of(),
        "antigravity",
        "a harness id on the roster is kept"
    );

    let _mapped = tracker("seats = [\"free-agy\", \"claude-reviewer\"]\n");
    std::env::set_var("LJOS_SEAT", "antigravity");
    assert_eq!(
        seat_of(),
        "free-agy",
        "an unlisted harness id sits as its roster seat"
    );
    std::env::set_var("LJOS_SEAT", "claude");
    let claude = seat_of();
    assert_ne!(
        claude, "claude-reviewer",
        "a harness id does not take a seat a person named after it"
    );
    assert_eq!(claude, "claude");
}
