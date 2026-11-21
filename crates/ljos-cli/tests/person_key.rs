//! `ljos person-key set` and `clear` are the person's. Under a runner,
//! or with a runner variable set, they refuse before they read the
//! terminal, and the key the passwd home keeps is left as it was.

ljos_test_boot::harness!();

use std::process::{Command, Stdio};

#[allow(dead_code)]
#[path = "common/pty.rs"]
mod pty;
#[path = "common/runner.rs"]
mod runner;

/// The bytes at the key path inside the isolated passwd home, or `None`
/// when no key is kept there.
fn kept_in(installed: &runner::RunnerDir) -> Option<Vec<u8>> {
    let path = installed.passwd_home().join(".config/ljos/person-key");
    std::fs::read(path).ok()
}

fn ljos<'a>(
    cmd: &'a mut Command,
    home: &std::path::Path,
    installed: &runner::RunnerDir,
) -> &'a mut Command {
    installed
        .env(cmd)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", home)
        .env_remove("LJOS_OFF")
        .env_remove("CURSOR_AGENT")
        .env_remove("CLAUDECODE")
}

#[test]
fn a_runner_in_the_tree_may_not_set_or_clear_the_key() {
    let installed = runner::install_dir();
    let before = kept_in(&installed);
    let home = tempfile::tempdir().unwrap();
    let claude = installed.path().join("claude");
    runner::runner_at(&claude);
    for verb in ["set", "clear"] {
        let out = ljos(
            Command::new(&claude).args([
                "-c",
                "\"$@\"; exit $?",
                "claude",
                env!("CARGO_BIN_EXE_ljos"),
                "person-key",
                verb,
            ]),
            home.path(),
            &installed,
        )
        .stdin(Stdio::null())
        .output()
        .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{verb}: {err}");
        assert!(
            err.contains("a runner is in this process tree"),
            "{verb}: {err}"
        );
    }
    assert_eq!(
        kept_in(&installed),
        before,
        "a refused verb changed the key"
    );
}

/// On a terminal of its own, a shell that carries `the runner mark` is
/// refused and never shown the passphrase prompt.
#[test]
fn a_mark_on_a_terminal_may_not_set_or_clear_the_key() {
    let installed = runner::install_dir();
    let before = kept_in(&installed);
    let home = tempfile::tempdir().unwrap();
    for verb in ["set", "clear"] {
        let term = pty::Pty::open();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(["person-key", verb]);
        ljos(&mut cmd, home.path(), &installed)
            .env("CURSOR_AGENT", "1")
            .stdin(Stdio::null());
        term.attach_output(&mut cmd);
        let child = term.spawn(&mut cmd);
        let out = term.wait_output(child, 30);
        term.finish_reading();
        let shown = term.shown();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{verb}: {err}{shown}");
        assert!(
            err.contains("CURSOR_AGENT") || shown.contains("CURSOR_AGENT"),
            "{verb}: {err}{shown}"
        );
        assert!(
            shown.contains("CURSOR_AGENT"),
            "{verb}: pty must keep refusal on the terminal: {shown}"
        );
        assert!(!shown.contains("passphrase"), "{verb}: {shown}");
    }
    assert_eq!(
        kept_in(&installed),
        before,
        "a refused verb changed the key"
    );
}

/// `keep_in_for_this_process` moves the key, so only a test build has
/// it. The `test-hooks` feature that brings it is on for this package's
/// tests, through its dev-dependency on itself, and off in a build of
/// the binaries, which follows normal edges only.
#[test]
fn only_a_test_build_has_the_hook_that_moves_the_key() {
    let features = |edges: &str| {
        let out = Command::new(env!("CARGO"))
            .args(["tree", "--locked", "--offline", "-p", "ljos", "-i", "ljos"])
            .args(["-e", edges])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let release = features("normal,features");
    assert!(!release.contains("test-hooks"), "{release}");
    let tests = features("normal,dev,features");
    assert!(tests.contains("ljos feature \"test-hooks\""), "{tests}");
}

#[test]
fn status_never_prints_the_hash() {
    let installed = runner::install_dir();
    let home = tempfile::tempdir().unwrap();
    let out = ljos(
        Command::new(env!("CARGO_BIN_EXE_ljos")).args(["person-key", "status"]),
        home.path(),
        &installed,
    )
    .env("CURSOR_AGENT", "1")
    .output()
    .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}");
    assert!(said.starts_with("person-key: "), "{said}");
    assert!(!said.contains("$argon2"), "{said}");
}

#[test]
fn runner_non_temp_homes_root_never_falls_back_to_passwd_home() {
    if std::env::var_os("LJOS_TEST_RUNNER_ROOT_CHILD").is_none() {
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runner_non_temp_homes_root_never_falls_back_to_passwd_home",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("LJOS_TEST_RUNNER_ROOT_CHILD", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "child test failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }

    let temp_target = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("CARGO_TARGET_DIR", temp_target.path());
    }
    let mut bad_homes = vec![std::path::PathBuf::from("/tmp/sandbox")];
    if let Some(pw) = ljos_cli::proc_tree::own_home() {
        if let Some(parent) = pw.parent() {
            bad_homes.push(parent.to_path_buf());
        }
        bad_homes.push(pw);
    }
    for bad in bad_homes {
        unsafe {
            std::env::set_var("HOME", &bad);
        }
        let caught = std::panic::catch_unwind(runner::non_temp_homes_root);
        assert!(
            caught.is_err(),
            "runner::non_temp_homes_root fell back instead of failing loudly for HOME={}: {caught:?}",
            bad.display()
        );
    }
}
