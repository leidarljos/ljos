//! The force-push samples in the README and the guides are the line
//! `ljos hook` prints for the payload each page pipes in, with no seat
//! rules, no `ljos-policyd` and the ask pack switched off. The hook is a
//! runner's, so it runs in a sitting, from a checkout on a topic branch.

ljos_test_boot::harness!();
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

/// An open ticket the tracker has claimed by `sample-holder`, and a hold
/// whose conversation is this test.
fn sit(tracker: &Path, run: &Path) {
    let project = tracker.join("Software/sample");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("issues.org"),
        "* STARTED [#C] sample work\n:PROPERTIES:\n:ID:         sample-0001\n:CLAIMED_BY: sample-holder\n:END:\n",
    )
    .unwrap();
    let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
    std::fs::create_dir_all(run.join("ljos")).unwrap();
    std::fs::write(
        run.join("ljos/hold-sample"),
        format!(
            "sample-holder\nsample\n{}\n{}\n2026-01-01T00:00:00Z\nsample-0001\n-\n",
            std::process::id(),
            comm.trim()
        ),
    )
    .unwrap();
}

fn hook(payload: &str) -> String {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("repo");
    let tracker = home.path().join("tracker");
    let run = home.path().join("run");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "topic"]);
    sit(&tracker, &run);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("hook")
        .current_dir(&repo)
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("VISSUE_ROOT", &tracker)
        .env("LJOS_SEAT", "sample")
        .env("LJOS_HOLD", "sample-holder")
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("LJOS_GUARD", "on")
        .env_remove("POLICYD_BIN")
        .env_remove("POLICYD_REQUIRED")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "ljos hook exited {:?}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn each_force_push_sample_is_what_the_hook_prints() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cases = [
        (
            r#"{"hook_event_name":"PreToolUse","tool_input":{"command":"git push --force"}}"#,
            r#"{"hookSpecificOutput""#,
            &[
                "README.md",
                "docs/source/howto.rst",
                "docs/orgmode/howto.org",
            ][..],
        ),
        (
            r#"{"hookEventName":"pre_tool_use","toolInput":{"command":"git push --force"}}"#,
            r#"{"decision""#,
            &["docs/source/grok-build.rst", "docs/orgmode/grok-build.org"][..],
        ),
    ];
    for (payload, shape, files) in cases {
        let printed = hook(payload);
        assert!(printed.contains("git-force-push"), "{payload}: {printed}");
        for file in files {
            let Ok(text) = std::fs::read_to_string(root.join(file)) else {
                continue;
            };
            let samples: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with(shape) && line.contains("git-force-push"))
                .collect();
            assert!(!samples.is_empty(), "{file} shows no force-push sample");
            for sample in samples {
                assert_eq!(
                    sample, printed,
                    "{file}: the sample is not what the hook prints"
                );
            }
        }
    }
}
