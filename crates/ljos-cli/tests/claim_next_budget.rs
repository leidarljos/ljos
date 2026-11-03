//! `ljos claim` with no id stops when the claim graph does not answer
//! within the sitting claim budget. A ready row that answers at once is
//! claimed.

ljos_test_boot::harness!();

use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// The budget this run gives the claim. The product default is 10 seconds.
/// Two seconds is long enough for a graph that answers at once, and short
/// enough that waiting out the stand-in's sleep would miss it.
const CLAIM_WAIT_SECS: u64 = 2;

/// Room past [`CLAIM_WAIT_SECS`] for starting the binary and the capture
/// wait's 20 ms poll. It is not a second product budget: the stand-in
/// sleeps 30 seconds, and a seat that waited that out would miss this.
const START_SLACK_SECS: u64 = 3;

/// How long the slow stand-in's `ready` sleeps. Longer than the claim
/// budget, so a seat that returns inside the budget did not wait it out.
const READY_SLEEP_SECS: u64 = 30;

const READY_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn ljos(dir: &Path, claim_wait: Option<u64>, args: &[&str]) -> (Output, Duration) {
    let home = dir.join("home");
    let runtime = dir.join("run");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let path = format!("{dir}:/usr/bin:/bin", dir = dir.display());
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("VISSUE_ROOT", dir.join("tracker"))
        .env("PACKSET_URL", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .stdin(Stdio::null());
    if let Some(secs) = claim_wait {
        cmd.env("LJOS_SITTING_CLAIM_WAIT", secs.to_string());
    }
    let started = Instant::now();
    let out = cmd.output().unwrap();
    (out, started.elapsed())
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `ready` sleeps [`READY_SLEEP_SECS`]. Every other claimdag verb returns
/// at once, so only the listing can hold the seat.
fn slow_ready(dir: &Path, log: &Path) {
    script(
        dir,
        "claimdag",
        &format!(
            "#!/bin/sh\necho \"claimdag $*\" >> '{log}'\ncase \"$1\" in\n  ready) sleep {READY_SLEEP_SECS} ;;\nesac\nexit 0\n",
            log = log.display(),
        ),
    );
}

/// One ready row, answered at once. `get` is not asked: the id is already
/// 32 hex, so the claim uses it as the node.
fn one_ready_row(dir: &Path, log: &Path) {
    script(
        dir,
        "claimdag",
        &format!(
            "#!/bin/sh\necho \"claimdag $*\" >> '{log}'\ncase \"$1\" in\n  ready) echo '[{{\"id\":\"{READY_ID}\",\"summary\":\"one ready node\"}}]' ;;\n  claim) echo gen=2 ;;\nesac\nexit 0\n",
            log = log.display(),
        ),
    );
}

#[test]
fn a_slow_ready_exits_inside_the_claim_budget() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("claimdag.log");
    slow_ready(dir.path(), &log);
    let (out, took) = ljos(dir.path(), Some(CLAIM_WAIT_SECS), &["claim"]);
    let err = stderr_of(&out);
    let bound = Duration::from_secs(CLAIM_WAIT_SECS + START_SLACK_SECS);
    assert!(
        took < bound,
        "claim waited {took:?}, bound {bound:?}\n{err}"
    );
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("claimdag ready"), "{err}");
    assert!(err.contains("claim graph"), "{err}");
    assert!(err.contains("ljos claim ID"), "{err}");
    assert!(err.contains("LJOS_SITTING_CLAIM_WAIT"), "{err}");
    let lines: Vec<&str> = err
        .lines()
        .filter(|line| line.contains("claimdag ready"))
        .collect();
    assert_eq!(lines.len(), 1, "{err}");
}

#[test]
fn a_ready_row_is_claimed() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("claimdag.log");
    one_ready_row(dir.path(), &log);
    let (out, took) = ljos(dir.path(), None, &["claim"]);
    let err = stderr_of(&out);
    assert!(
        out.status.success(),
        "stderr {err}\nlog {}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
    assert!(
        took < Duration::from_secs(CLAIM_WAIT_SECS + START_SLACK_SECS),
        "a ready row waited {took:?}\n{err}"
    );
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        calls.contains(&format!("claimdag claim {READY_ID} --assignee ")),
        "{calls}"
    );
}
