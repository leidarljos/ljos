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

/// The bytes at the key path, or `None` when no key is kept there.
fn kept() -> Option<Vec<u8>> {
    let path = ljos_cli::person_key::key_path().expect("a passwd home");
    std::fs::read(path).ok()
}

fn ljos<'a>(cmd: &'a mut Command, home: &std::path::Path) -> &'a mut Command {
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", home)
        .env_remove("LJOS_OFF")
        .env_remove("CURSOR_AGENT")
        .env_remove("CLAUDECODE")
}

#[test]
fn a_runner_in_the_tree_may_not_set_or_clear_the_key() {
    let before = kept();
    let home = tempfile::tempdir().unwrap();
    let installed = runner::install_dir();
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
    assert_eq!(kept(), before, "a refused verb changed the key");
}

/// On a terminal of its own, a shell that carries `CURSOR_AGENT` is
/// refused and never shown the passphrase prompt.
#[test]
fn a_mark_on_a_terminal_may_not_set_or_clear_the_key() {
    let before = kept();
    let home = tempfile::tempdir().unwrap();
    for verb in ["set", "clear"] {
        let term = pty::Pty::open();
        let child = term.spawn(
            ljos(
                Command::new(env!("CARGO_BIN_EXE_ljos")).args(["person-key", verb]),
                home.path(),
            )
            .env("CURSOR_AGENT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        );
        let out = pty::wait_output(child, 30);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{verb}: {err}");
        assert!(err.contains("CURSOR_AGENT"), "{verb}: {err}");
        assert!(
            !term.shown().contains("passphrase"),
            "{verb}: {}",
            term.shown()
        );
    }
    assert_eq!(kept(), before, "a refused verb changed the key");
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
            .args(["-e", edges, "--target", "all"])
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
    let home = tempfile::tempdir().unwrap();
    let out = ljos(
        Command::new(env!("CARGO_BIN_EXE_ljos")).args(["person-key", "status"]),
        home.path(),
    )
    .env("CURSOR_AGENT", "1")
    .output()
    .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}");
    assert!(said.starts_with("person-key: "), "{said}");
    assert!(!said.contains("$argon2"), "{said}");
}
