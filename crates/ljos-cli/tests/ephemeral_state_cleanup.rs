//! A shipped `ljos` with no absolute state home removes its private
//! temporary state on every exit path that uses it.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str]) -> Output {
    let tmp = root.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts the shipped binary whose process-exit cleanup is under test"
    )]
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TMPDIR", &tmp)
        .env("PACKSET_URL", "off")
        .env("LJOS_SEAT", "cleanup-test")
        .env("LJOS_TRACKER_GIT", "off")
        .env("VISSUE_NO_ROUTE", "1")
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn ephemeral_roots(tmp: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(tmp)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("ljos-state-"))
        })
        .collect()
}

#[test]
fn no_home_remember_search_and_doctor_discard_temporary_state() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        &["remember", "A temporary lesson is discarded."][..],
        &["search", "temporary lesson"][..],
        &["doctor"][..],
    ] {
        let out = run(root.path(), args);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            text.contains("state is in a temporary directory and will be discarded"),
            "{}:\n{text}",
            args.join(" ")
        );
        let leaked = ephemeral_roots(&root.path().join("tmp"));
        assert!(
            leaked.is_empty(),
            "{} left temporary state: {leaked:?}\n{text}",
            args.join(" ")
        );
    }
}
