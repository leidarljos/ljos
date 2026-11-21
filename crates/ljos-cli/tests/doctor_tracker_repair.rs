//! `ljos doctor` names a tracker git repository whose objects do not
//! read, and prints the repair. It does not run that repair. A healthy
//! checkout gains no new text.

ljos_test_boot::harness!();
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let out = ljos_test_boot::git()
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn doctor(path: &str, home: &Path, tracker: &Path, git_log: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("VISSUE_ROOT", tracker)
        .env("PACKSET_URL", "off")
        .env("LC_ALL", "C")
        .env("GIT_LOG", git_log)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn assert_steps(said: &str, tracker: &Path) {
    let row = said
        .lines()
        .find(|line| line.contains("\ttickets\t"))
        .unwrap_or_else(|| panic!("no tracker row:\n{said}"));
    assert!(row.starts_with("warn\ttickets\t"), "{row}");
    assert!(row.contains("cannot read its objects"), "{row}");
    assert!(row.contains(&tracker.display().to_string()), "{row}");
    assert!(said.contains("\n1. "), "{said}");
    assert!(said.contains("index-pack"), "{said}");
    assert!(
        said.contains("Point the branch at a valid commit"),
        "{said}"
    );
    assert!(said.contains("fetch --refetch origin"), "{said}");
    assert!(said.contains("Set the branch to origin"), "{said}");
    assert!(said.contains("read-tree HEAD"), "{said}");
    assert!(
        said.contains("Never reset it: it may hold unpushed tracker changes"),
        "{said}"
    );
    assert!(!said.contains("fatal:"), "{said}");
}

fn git_did_not_repair(log: &Path) {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    for forbidden in [
        " index-pack ",
        " read-tree ",
        " reset ",
        " fetch ",
        " commit ",
        " update-ref ",
    ] {
        assert!(
            !text.contains(forbidden),
            "doctor ran `{forbidden}`:\n{text}"
        );
    }
}

#[test]
fn doctor_names_a_broken_tracker_and_leaves_a_healthy_one_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = dir.path().join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    git(&tracker, &["init", "-q"]);
    git(&tracker, &["config", "user.email", "seat@example.invalid"]);
    git(&tracker, &["config", "user.name", "seat"]);
    git(&tracker, &["config", "core.hooksPath", "/dev/null"]);
    std::fs::write(tracker.join("Software/issues.org"), "* TODO a\n").unwrap();
    git(&tracker, &["add", "."]);
    git(&tracker, &["commit", "-q", "-m", "seed"]);

    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let real_git = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|dir| Path::new(dir).join("git"))
        .find(|path| path.is_file())
        .expect("git on PATH");
    let git_log = dir.path().join("git.log");
    std::fs::write(
        bin.join("git"),
        format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$*\" >> \"$GIT_LOG\"\n\
             for arg in \"$@\"; do\n\
               case \"$arg\" in\n\
                 index-pack|read-tree|reset|fetch|commit|update-ref)\n\
                   echo \"refusing $*\" >&2\n\
                   exit 99\n\
                   ;;\n\
               esac\n\
             done\n\
             exec '{}' \"$@\"\n",
            real_git.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        bin.join("vissue"),
        format!(
            "#!/bin/sh\n[ \"$1\" = identity ] && printf 'vissue 0.16.2\\nprotocol: 1\\nroot={}\\nprefix=Software\\n'\nexit 0\n",
            tracker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(bin.join("vissue"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("run")).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let clean = doctor(&path, &home, &tracker, &git_log);
    assert!(!clean.contains("cannot read its objects"), "{clean}");
    assert!(!clean.contains("index-pack"), "{clean}");
    assert!(!clean.contains("read-tree"), "{clean}");
    git_did_not_repair(&git_log);

    git(&tracker, &["repack", "-Ad"]);
    let still = doctor(&path, &home, &tracker, &git_log);
    assert!(
        !still.contains("cannot read its objects"),
        "a pack with its index is not new text:\n{still}"
    );

    for ent in std::fs::read_dir(tracker.join(".git/objects/pack")).unwrap() {
        let path = ent.unwrap().path();
        if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("idx") | Some("rev")
        ) {
            std::fs::remove_file(&path).unwrap();
        }
    }
    let index = std::fs::read(tracker.join(".git/index")).ok();
    let _ = std::fs::write(&git_log, "");
    let broken = doctor(&path, &home, &tracker, &git_log);
    assert_steps(&broken, &tracker);
    git_did_not_repair(&git_log);
    assert!(
        std::fs::read_dir(tracker.join(".git/objects/pack"))
            .unwrap()
            .flatten()
            .all(|ent| ent.path().extension().and_then(|ext| ext.to_str()) != Some("idx")),
        "doctor wrote a pack index"
    );
    assert_eq!(std::fs::read(tracker.join(".git/index")).ok(), index);

    let other = dir.path().join("missing-head");
    std::fs::create_dir_all(other.join("Software")).unwrap();
    git(&other, &["init", "-q"]);
    git(&other, &["config", "user.email", "seat@example.invalid"]);
    git(&other, &["config", "user.name", "seat"]);
    git(&other, &["config", "core.hooksPath", "/dev/null"]);
    std::fs::write(other.join("Software/issues.org"), "* TODO a\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-q", "-m", "seed"]);
    let head = ljos_test_boot::git()
        .arg("-C")
        .arg(&other)
        .args(["symbolic-ref", "HEAD"])
        .output()
        .unwrap();
    assert!(head.status.success());
    let rel = String::from_utf8_lossy(&head.stdout).trim().to_string();
    std::fs::write(
        other.join(".git").join(&rel),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .unwrap();
    std::fs::write(
        bin.join("vissue"),
        format!(
            "#!/bin/sh\n[ \"$1\" = identity ] && printf 'vissue 0.16.2\\nprotocol: 1\\nroot={}\\nprefix=Software\\n'\nexit 0\n",
            other.display()
        ),
    )
    .unwrap();
    let _ = std::fs::write(&git_log, "");
    let missing = doctor(&path, &home, &other, &git_log);
    assert_steps(&missing, &other);
    git_did_not_repair(&git_log);
    let head_after = std::fs::read_to_string(other.join(".git/HEAD")).unwrap();
    assert!(
        head_after.contains("refs/heads/"),
        "doctor moved HEAD: {head_after}"
    );
}
