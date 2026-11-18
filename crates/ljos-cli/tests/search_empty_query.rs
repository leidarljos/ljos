//! `ljos search` with no words is a usage error that names the command to
//! run. It used to print `Nothing matched this query.` and exit 0, as if a
//! search had run and found nothing.

#[path = "common/boot.rs"]
mod boot;

use std::process::{Command, Output, Stdio};

fn search(args: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("search")
        .args(args)
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", dir.path().join("run"))
        .env("TMPDIR", dir.path())
        .env("LJOS_SEAT", "testseat")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("PACKSET_URL", "http://127.0.0.1:9")
        .env("PACKSET_AUTH", "off")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn a_search_with_no_words_names_the_command_to_run() {
    for args in [&[][..], &["  "][..], &["--json"][..]] {
        let out = search(args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {stderr}");
        assert_eq!(
            stderr.trim(),
            "search: the query is empty, so run `ljos search \"QUERY\"`.",
            "{args:?}"
        );
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn a_search_with_a_source_filter_and_no_words_is_not_refused() {
    let out = search(&["--source-seat", "alice"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("the query is empty"), "{stderr}");
}
