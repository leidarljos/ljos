//! Common commands with nothing set but `LJOS_SEAT`.
//!
//! Search, remember, file, note and finish do not ask a question. A pack
//! that is down or missing is one line on stderr.

ljos_test_boot::harness!();

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], extra: &[(&str, &str)]) -> Output {
    let home = tempfile::tempdir().unwrap();
    for sub in [".config", ".local/state", ".local/share", "run", "tmp"] {
        std::fs::create_dir_all(home.path().join(sub)).unwrap();
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("TMPDIR", home.path().join("tmp"))
        .env("LJOS_SEAT", "saddle")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "400")
        .envs(extra.iter().copied())
        .current_dir(home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Keep the temp dir alive until the child exits.
    let out = cmd.output().unwrap();
    // The directory is dropped here. The child has finished.
    drop(home);
    out
}

fn lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn assert_one_fault(out: &Output, args: &[&str]) {
    let stdout = lines(&out.stdout);
    let stderr = lines(&out.stderr);
    assert!(
        stdout
            .iter()
            .all(|line| !line.contains('?') && !line.contains("type yes")),
        "{args:?} asked on stdout: {stdout:?}"
    );
    assert!(
        stderr
            .iter()
            .all(|line| !line.contains('?') && !line.contains("type yes")),
        "{args:?} asked on stderr: {stderr:?}"
    );
    assert_eq!(stderr.len(), 1, "{args:?} was not one line: {stderr:?}");
    assert!(
        stderr[0].contains("packsetd is unreachable") || stderr[0].contains("not on PATH"),
        "{args:?}: {stderr:?}"
    );
    assert!(
        !stderr[0].contains("Caused by"),
        "{args:?} printed a chain: {stderr:?}"
    );
}

#[test]
fn search_and_remember_say_one_line_when_the_pack_is_down() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let url = format!("http://127.0.0.1:{port}");
    let pack = [("PACKSET_URL", url.as_str())];
    let search = run(&["search", "fix the broken links on the docs"], &pack);
    assert_eq!(
        search.status.code(),
        Some(75),
        "{:?}",
        lines(&search.stderr)
    );
    assert_one_fault(&search, &["search"]);
    let remember = run(
        &["remember", "The kiln fires at dusk on the north bench."],
        &pack,
    );
    assert_eq!(
        remember.status.code(),
        Some(75),
        "{:?}",
        lines(&remember.stderr)
    );
    assert_one_fault(&remember, &["remember"]);
}

#[test]
fn note_without_a_ticket_is_a_label() {
    let out = run(&["note", "the glaze crawled on the north tile"], &[]);
    let stdout = lines(&out.stdout);
    let stderr = lines(&out.stderr);
    assert!(out.status.success(), "stdout {stdout:?} stderr {stderr:?}");
    assert!(stderr.is_empty(), "{stderr:?}");
    assert_eq!(stdout.len(), 1, "{stdout:?}");
    assert!(
        stdout[0].starts_with("label unattached-note:"),
        "{stdout:?}"
    );
    assert!(!stdout[0].contains('?'), "{stdout:?}");
}

#[test]
fn file_and_finish_do_not_ask_and_name_a_missing_tool_in_one_line() {
    let file = run(&["file", "Repair the kiln door"], &[]);
    let file_err = lines(&file.stderr);
    assert!(
        file_err.iter().all(|line| !line.contains("type yes")),
        "{file_err:?}"
    );
    if !file.status.success() {
        assert_eq!(file_err.len(), 1, "{file_err:?}");
    }
    let finish = run(
        &[
            "finish",
            "proj-aaaa",
            "--lesson",
            "The kiln door seats when it is cold.",
        ],
        &[],
    );
    let err = lines(&finish.stderr);
    let out = lines(&finish.stdout);
    assert!(
        err.iter()
            .chain(&out)
            .all(|line| !line.contains("type yes") && !line.contains("--close")),
        "finish asked or closed: stdout {out:?} stderr {err:?}"
    );
    if !finish.status.success() {
        assert_eq!(err.len(), 1, "{err:?}");
        assert!(
            err[0].contains("not on PATH")
                || err[0].contains("unreachable")
                || err[0].contains("not found"),
            "{err:?}"
        );
    }
}

#[test]
fn cards_name_the_directory_when_both_files_are_missing() {
    let out = run(&["cards"], &[]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("no USER.md or MEMORY.md"), "{text}");
    assert!(
        text.contains("ljos reads them and does not write them"),
        "{text}"
    );
}

#[test]
fn mesh_pull_names_a_missing_directory() {
    let out = run(
        &[
            "mesh",
            "pull",
            "--seat",
            "box",
            "--from",
            "/no/such-ljos-handoff",
        ],
        &[],
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("does not exist"), "{err}");
    assert!(err.contains(".jsonl"), "{err}");
}

#[test]
fn mesh_acl_names_the_example_file() {
    let out = run(&["mesh", "acl"], &[]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("examples/mesh.toml"), "{err}");
    assert!(err.contains("ljos mesh up"), "{err}");
}

#[test]
fn bump_plan_and_findings_name_the_eb_stack_files() {
    let plan = run(
        &[
            "bump-plan",
            "out",
            "--project",
            "demo",
            "--parent",
            "demo-319z",
            "--dry-run",
        ],
        &[],
    );
    let plan_err = String::from_utf8_lossy(&plan.stderr);
    assert!(!plan.status.success(), "{plan_err}");
    assert!(plan_err.contains("eb-stack"), "{plan_err}");
    let findings = run(&["findings", "out/campaign.json"], &[]);
    let findings_err = String::from_utf8_lossy(&findings.stderr);
    assert!(!findings.status.success(), "{findings_err}");
    assert!(findings_err.contains("campaign.json"), "{findings_err}");
}

#[test]
fn a_failed_wake_shows_the_command_output() {
    let home = tempfile::tempdir().unwrap();
    for sub in [".config/ljos", ".local/state", "run", "tmp"] {
        std::fs::create_dir_all(home.path().join(sub)).unwrap();
    }
    let mut base = Command::new(env!("CARGO_BIN_EXE_ljos"));
    base.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("TMPDIR", home.path().join("tmp"))
        .env("LJOS_SEAT", "seat-a")
        .env("LJOS_TRACKER_GIT", "off")
        .current_dir(home.path())
        .stdin(Stdio::null());
    let set = base
        .args([
            "wake",
            "--timeout",
            "5",
            "--command",
            "echo woke-fail >&2; exit 2",
        ])
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let fired = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["wake", "seat-a", "--fire"])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("TMPDIR", home.path().join("tmp"))
        .env("LJOS_SEAT", "seat-a")
        .env("LJOS_TRACKER_GIT", "off")
        .current_dir(home.path())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&fired.stderr);
    assert!(!fired.status.success(), "{err}");
    assert!(err.contains("woke-fail"), "{err}");
    assert!(err.contains("exited"), "{err}");
}

#[test]
fn seat_grant_names_this_seat_when_via_does_not_match() {
    let out = run(
        &[
            "seat",
            "grant",
            "carol",
            "close=own",
            "--quote",
            "carol may close what she files",
            "--at",
            "2026-10-29T02:00:00Z",
            "--via",
            "bob",
        ],
        &[],
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("this seat is saddle"), "{text}");
    assert!(text.contains("--via names bob"), "{text}");
}

#[test]
fn a_question_is_not_read_from_stdin() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("run")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["note", "left on the bench"])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("LJOS_SEAT", "saddle")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"yes\n").unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{:?}", lines(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("unattached-note"), "{stdout}");
    assert!(!stdout.contains("yes"), "{stdout}");
}
