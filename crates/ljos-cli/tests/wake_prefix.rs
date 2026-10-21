//! A wake name ending in `-` is a prefix. Mail to a run seat uses it.

use std::path::Path;
use std::process::{Command, Output};

fn ljos(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("LJOS_TRACKER_GIT", "off")
        .output()
        .expect("spawn ljos")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn the_cli_names_a_prefix_and_shows_which_run_it_matches() {
    let home = tempfile::tempdir().expect("home");
    let root = home.path();
    let set = ljos(
        root,
        &[
            "wake",
            "ljosbot-review-",
            "--webhook",
            "http://127.0.0.1:9/hook",
        ],
    );
    assert!(set.status.success(), "{}", text(&set));
    assert!(
        stdout(&set).contains("when mail reaches a seat that starts with it"),
        "{}",
        text(&set)
    );

    let run = ljos(root, &["wake", "ljosbot-review-abc"]);
    assert!(run.status.success(), "{}", text(&run));
    assert!(
        stdout(&run).contains("matches ljosbot-review-"),
        "{}",
        text(&run)
    );
    assert!(
        stdout(&run).contains("http://127.0.0.1:9/hook"),
        "{}",
        text(&run)
    );

    let other = ljos(root, &["wake", "ljosbot-reviewer"]);
    assert!(other.status.success(), "{}", text(&other));
    assert!(stdout(&other).contains("none set"), "{}", text(&other));

    let bare = ljos(
        root,
        &["wake", "inky", "--webhook", "http://127.0.0.1:9/inky"],
    );
    assert!(bare.status.success(), "{}", text(&bare));
    assert!(
        stdout(&bare).ends_with("when mail reaches it\n"),
        "{}",
        text(&bare)
    );
    let near = ljos(root, &["wake", "inky-2"]);
    assert!(stdout(&near).contains("none set"), "{}", text(&near));
}
