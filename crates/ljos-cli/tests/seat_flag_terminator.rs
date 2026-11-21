//! A global seat flag is read only before the argument terminator.

ljos_test_boot::harness!();

use std::process::Command;

fn run(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    let home = root.join("home");
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("LJOS_SEAT", "alice")
        .env("LJOS_OFF", "1")
        .env("PACKSET_URL", "off")
        .output()
        .unwrap()
}

fn said(out: &std::process::Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn the_terminator_keeps_seat_words_in_the_search_query() {
    let root = tempfile::tempdir().unwrap();
    for (args, shown_query) in [
        (&["search", "kiln", "--", "--seat", "bob"][..], None),
        (&["search", "--", "--seat"][..], Some("--seat")),
        (
            &["search", "zz-seat-marker", "--", "--seat", "bob"][..],
            Some("zz-seat-marker --seat bob"),
        ),
    ] {
        let out = run(root.path(), args);
        let said = said(&out);
        assert_eq!(out.status.code(), Some(0), "{args:?}\n{said}");
        if let Some(query) = shown_query {
            assert!(
                String::from_utf8_lossy(&out.stdout).contains(query),
                "{args:?}\n{said}"
            );
        }
        assert!(!said.contains("--seat needs a value"), "{args:?}\n{said}");
    }
}

#[test]
fn empty_and_flag_like_global_seat_values_are_usage_errors() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        &["--seat", "--json", "search", "kiln"][..],
        &["doctor", "--seat", "--json"][..],
        &["doctor", "--seat="][..],
    ] {
        let out = run(root.path(), args);
        let said = said(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim(),
            "ljos: --seat needs a value",
            "{args:?}\n{said}"
        );
    }
}
