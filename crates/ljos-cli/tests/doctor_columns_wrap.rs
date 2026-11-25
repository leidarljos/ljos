//! Doctor output stays unchanged off a TTY.

ljos_test_boot::harness!();

use std::process::{Command, Stdio};

fn run_doctor(columns: Option<&str>) -> (bool, String, String) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".state")).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    std::fs::create_dir_all(root.path().join("tmp")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("doctor")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("TMPDIR", root.path().join("tmp"))
        .env("PACKSET_URL", "off")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .current_dir(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cols) = columns {
        cmd.env("COLUMNS", cols);
    }
    let out = cmd.output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn columns_sixty_non_tty_is_unchanged() {
    let (_ok, stdout, stderr) = run_doctor(Some("60"));
    let text = format!("{stdout}{stderr}");
    assert!(!text.is_empty(), "doctor printed nothing");
    assert!(
        text.lines().any(|line| line.contains('\t')),
        "doctor rows should stay tab-separated in non-tty output:\n{text}"
    );
}

#[test]
fn no_columns_keeps_non_tty_rows_tab_separated() {
    let (_ok, stdout, _stderr) = run_doctor(None);
    assert!(
        stdout.lines().any(|line| line.contains('\t')),
        "doctor rows should stay tab-separated without COLUMNS:\n{stdout}"
    );
}
