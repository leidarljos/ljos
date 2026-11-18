//! Tracker git operations pass `gc.auto=0`, so commits and syncs do not
//! trigger an auto-gc repack that stalls the command.

ljos_test_boot::harness!();

#[path = "common/sibling.rs"]
mod sibling;
#[path = "common/tools.rs"]
mod tools;

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn git(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn pack_files(dir: &Path) -> Vec<String> {
    let pack_dir = dir.join(".git/objects/pack");
    let Ok(entries) = std::fs::read_dir(&pack_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.ends_with(".pack").then_some(name)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn tracker_commits_do_not_trigger_git_auto_gc_repack() {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let tracker = dir.path().join("tracker");
    let project = tracker.join("Software/probe");
    std::fs::create_dir_all(home.join(".config/ljos")).unwrap();
    std::fs::create_dir_all(home.join(".local/state/ljos")).unwrap();
    std::fs::create_dir_all(dir.path().join("run")).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    git(&tracker, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("core.hooksPath", "/dev/null"),
        ("user.name", "probe"),
        ("user.email", "probe@example.invalid"),
        ("commit.gpgsign", "false"),
        ("gc.autoDetach", "false"),
        ("gc.autoPackLimit", "1"),
    ] {
        git(&tracker, &["config", k, v]);
    }

    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        project.join("issues.org"),
        "\
#+TITLE: probe
#+VISSUE: 1
#+TODO: TODO STARTED | DONE

* TODO [#B] Keep auto gc off tracker commits
:PROPERTIES:
:ID:         probe-autogc
:END:
",
    )
    .unwrap();
    git(&tracker, &["add", "-A"]);
    git(
        &tracker,
        &["-c", "gc.auto=0", "commit", "-q", "-m", "seed-1"],
    );
    git(&tracker, &["repack", "-d"]);

    std::fs::write(tracker.join("extra.txt"), "second pack\n").unwrap();
    git(&tracker, &["add", "extra.txt"]);
    git(
        &tracker,
        &["-c", "gc.auto=0", "commit", "-q", "-m", "seed-2"],
    );
    git(&tracker, &["repack", "-d"]);

    let before = pack_files(&tracker);
    assert_eq!(
        before.len(),
        2,
        "expected two packs before tracker write: {before:?}"
    );

    let head_before = String::from_utf8_lossy(&git(&tracker, &["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();

    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["note", "probe-autogc", "recorded without auto gc"])
        .current_dir(&tracker)
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", dir.path().join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "probe")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("PACKSET_URL", "off")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "ljos note failed:\n{said}");
    let head_after = String::from_utf8_lossy(&git(&tracker, &["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    assert_ne!(
        head_after, head_before,
        "expected a tracker commit after ljos note:\n{said}"
    );

    let after = pack_files(&tracker);
    assert_eq!(
        after, before,
        "tracker commit triggered git auto-gc repack (before={before:?}, after={after:?})"
    );
}

fn loose_objects(dir: &Path) -> usize {
    let out = git(dir, &["count-objects"]);
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .and_then(|word| word.parse::<usize>().ok())
        .unwrap_or(0)
}

#[test]
fn tracker_loose_objects_stay_bounded_over_200_commits() {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
    let _claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let tracker = dir.path().join("tracker");
    let project = tracker.join("Software/probe");
    std::fs::create_dir_all(home.join(".config/ljos")).unwrap();
    std::fs::create_dir_all(home.join(".local/state/ljos")).unwrap();
    std::fs::create_dir_all(dir.path().join("run")).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    git(&tracker, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("core.hooksPath", "/dev/null"),
        ("user.name", "probe"),
        ("user.email", "probe@example.invalid"),
        ("commit.gpgsign", "false"),
        ("gc.auto", "200"),
        ("gc.autoDetach", "false"),
    ] {
        git(&tracker, &["config", k, v]);
    }

    // Blob "anchor-11\n" hashes to 17311b936343c60fc459bac3a04b6752a9894476
    // so objects/17/ has a loose object for `git gc --auto`.
    std::fs::write(tracker.join("anchor.txt"), "anchor-11\n").unwrap();
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        project.join("issues.org"),
        "\
#+TITLE: probe
#+VISSUE: 1
#+TODO: TODO STARTED | DONE

* TODO [#B] Bound loose objects
:PROPERTIES:
:ID:         probe-bound
:END:
",
    )
    .unwrap();
    git(&tracker, &["add", "-A"]);
    git(&tracker, &["-c", "gc.auto=0", "commit", "-q", "-m", "seed"]);

    let path = format!("{}:/usr/bin:/bin", vissue.parent().unwrap().display());
    let run_ljos = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(&tracker)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", dir.path().join("run"))
            .env("CLAIMDAG_DIR", dir.path().join("claims"))
            .env("VISSUE_ROOT", &tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "probe")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", "off")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "ljos {args:?} failed:\n{said}");
    };

    for i in 0..200 {
        let issues = project.join("issues.org");
        let mut text = std::fs::read_to_string(&issues).unwrap();
        text.push_str(&format!("note {i}\n"));
        std::fs::write(&issues, text).unwrap();
        assert!(
            ljos_cli::commit_paths(
                &tracker,
                &["Software/probe/issues.org"],
                &format!("chore(issues): probe-bound noted {i}")
            ) == ljos_cli::PathCommit::Made
        );
    }

    // Per-commit tracker git operations kept gc.auto=0, so no commit packed mid-stream.
    assert!(
        pack_files(&tracker).is_empty(),
        "per-commit tracker operations packed early"
    );
    let before = loose_objects(&tracker);
    assert!(before >= 200, "expected >= 200 loose objects, got {before}");

    run_ljos(&["claim", "probe-bound"]);
    run_ljos(&["finish", "probe-bound"]);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if !tracker.join(".git/gc.pid").exists() && !pack_files(&tracker).is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    let packs = pack_files(&tracker);
    assert!(!packs.is_empty(), "finish did not trigger out-of-band gc");
    let after = loose_objects(&tracker);
    assert!(
        after < 200,
        "loose objects stayed unbounded after finish (before={before}, after={after})"
    );
}
