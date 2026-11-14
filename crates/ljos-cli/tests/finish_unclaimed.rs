//! `ljos finish` on a ticket that was filed and never claimed.
//!
//! The ticket stays TODO. Nothing is written. The line names `ljos sitting`.

ljos_test_boot::harness!();

use std::path::Path;
use std::process::{Command, Stdio};

const ISSUE: &str = "demo-open";

const ORG: &str = "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] Pick a codec
:PROPERTIES:
:ID:         demo-open
:END:

the codec is one file
";

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    let home = root.join("home");
    let tracker = root.join("tracker");
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", root.join("tmp"))
        .env("VISSUE_ROOT", &tracker)
        .env("LJOS_SEAT", "alice")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_HOOKD", "off")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(&home)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")))
}

fn seat(root: &Path) {
    let home = root.join("home");
    for sub in [".config", ".local/share", ".local/state", ".cache", "run"] {
        std::fs::create_dir_all(home.join(sub)).unwrap();
    }
    std::fs::create_dir_all(root.join("tmp")).unwrap();
    let project = root.join("tracker/Software/demo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(project.join("issues.org"), ORG).unwrap();
}

#[test]
fn finish_of_an_unclaimed_ticket_names_sitting_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    seat(root.path());
    let file = root.path().join("tracker/Software/demo/issues.org");
    let before = std::fs::read(&file).unwrap();
    let out = run(root.path(), &["finish", ISSUE]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(out.stdout.is_empty(), "{text}");
    assert_eq!(
        text.trim(),
        "demo-open is not claimed and stays TODO, so run `ljos sitting demo-open`."
    );
    assert!(!text.contains("finished"), "{text}");
    assert!(!text.contains("sitting ended"), "{text}");
    assert!(!text.contains("nothing to commit"), "{text}");
    assert!(!text.contains("as done"), "{text}");
    assert_eq!(std::fs::read(&file).unwrap(), before, "{text}");

    // A lesson is still stored. With the pack off, that attempt is said,
    // the ticket stays TODO, and the same next command is named.
    let with_lesson = run(
        root.path(),
        &["finish", ISSUE, "--lesson", "The codec is one file."],
    );
    let lesson_text = format!(
        "{}{}",
        String::from_utf8_lossy(&with_lesson.stdout),
        String::from_utf8_lossy(&with_lesson.stderr)
    );
    assert_eq!(with_lesson.status.code(), Some(0), "{lesson_text}");
    assert!(
        lesson_text.contains("the lesson was not kept"),
        "{lesson_text}"
    );
    assert!(
        lesson_text
            .contains("demo-open is not claimed and stays TODO, so run `ljos sitting demo-open`."),
        "{lesson_text}"
    );
    assert!(!lesson_text.contains("finished"), "{lesson_text}");
    assert!(!lesson_text.contains("sitting ended"), "{lesson_text}");
    assert_eq!(std::fs::read(&file).unwrap(), before, "{lesson_text}");
}
