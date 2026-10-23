//! `ljos doctor` warns when the tracker checkout holds issue edits no
//! commit has. Raw `vissue` closes sat uncommitted on the box for days,
//! and every row stayed green while no other host saw them.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

#[test]
fn doctor_warns_on_uncommitted_issue_edits_in_the_tracker() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = dir.path().join("tracker");
    std::fs::create_dir_all(tracker.join("Software/issues")).unwrap();
    git(&tracker, &["init", "-q"]);
    git(&tracker, &["config", "user.email", "seat@example.invalid"]);
    git(&tracker, &["config", "user.name", "seat"]);
    git(&tracker, &["config", "core.hooksPath", "/dev/null"]);
    std::fs::write(tracker.join("Software/issues.org"), "* TODO a\n").unwrap();
    git(&tracker, &["add", "."]);
    git(&tracker, &["commit", "-q", "-m", "seed"]);
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("vissue");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\n[ \"$1\" = identity ] && printf 'vissue 0.16.2\\nprotocol: 1\\nroot={}\\nprefix=Software\\n'\nexit 0\n",
            tracker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("run")).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let doctor = |env: &[(&str, &str)]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .arg("doctor")
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("VISSUE_ROOT", &tracker)
            .env("PACKSET_URL", "off")
            .envs(env.iter().copied())
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let tracker_row = |said: &str| {
        said.lines()
            .find(|l| l.contains("\ttracker\t"))
            .map(str::to_string)
    };

    let clean = doctor(&[]);
    assert!(!clean.contains("uncommitted"), "{clean}");

    std::fs::write(tracker.join("Software/issues.org"), "* DONE a\n").unwrap();
    std::fs::write(tracker.join("Software/issues/Software-b.org"), "* TODO b\n").unwrap();
    let dirty = doctor(&[]);
    let row = tracker_row(&dirty).unwrap_or_else(|| panic!("no tracker row:\n{dirty}"));
    assert!(row.starts_with("warn\ttracker\t"), "{row}");
    assert!(
        row.contains("uncommitted issue edits in Software/issues.org and 1 more"),
        "{row}"
    );

    let off = doctor(&[("LJOS_TRACKER_GIT", "off")]);
    assert!(!off.contains("uncommitted"), "{off}");

    git(&tracker, &["add", "."]);
    git(&tracker, &["commit", "-q", "-m", "close a"]);
    let committed = doctor(&[]);
    assert!(!committed.contains("uncommitted"), "{committed}");
}
