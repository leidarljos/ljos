//! `ljos memories prune --days` takes a number of days from 0 up.
//!
//! A negative count moved the cutoff into the future, so the command listed
//! nothing, said it was a dry run and exited 0. It now names the flag and
//! the value and exits 2, with a live pack behind it.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::process::{Output, Stdio};

struct Seat {
    home: tempfile::TempDir,
    run: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn on(url: String) -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            run: tempfile::tempdir().unwrap(),
            url,
        }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        ljos_cli::program(env!("CARGO_BIN_EXE_ljos"))
            .unwrap()
            .args(args)
            .current_dir(self.home.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home.path())
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.run.path())
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_negative_day_count_on_prune_exits_2() {
    if tools::halt(tools::Tool::Packsetd) {
        return;
    }
    let daemon = ljos_test_boot::Packsetd::open();
    let seat = Seat::on(daemon.url().to_string());
    let cases: &[&[&str]] = &[
        &["memories", "prune", "--stale", "--days", "-1"],
        &["memories", "prune", "--stale", "--days=-1"],
        &["memories", "prune", "--days", "-1", "--stale"],
        &["memories", "prune", "--stale", "--days", "-1", "--apply"],
        &["memories", "prune", "--stale", "--days", "-1", "--json"],
    ];
    for args in cases {
        let out = seat.ljos(args);
        let text = said(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{text}");
        assert!(out.stdout.is_empty(), "{args:?}\n{text}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim_end(),
            "memories prune: --days takes a number from 0 up, not -1.",
            "{args:?}\n{text}"
        );
    }
}

#[test]
fn a_day_count_that_is_not_a_whole_number_exits_2() {
    if tools::halt(tools::Tool::Packsetd) {
        return;
    }
    let daemon = ljos_test_boot::Packsetd::open();
    let seat = Seat::on(daemon.url().to_string());
    for value in ["abc", "1.5", "", "99999999999999999999"] {
        let out = seat.ljos(&["memories", "prune", "--stale", "--days", value]);
        let text = said(&out);
        assert_eq!(out.status.code(), Some(2), "{value:?}\n{text}");
        assert!(out.stdout.is_empty(), "{value:?}\n{text}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("memories prune: --days does not accept"),
            "{value:?}\n{text}"
        );
    }
}

#[test]
fn zero_and_a_very_large_day_count_still_run() {
    if tools::halt(tools::Tool::Packsetd) {
        return;
    }
    let daemon = ljos_test_boot::Packsetd::open();
    let seat = Seat::on(daemon.url().to_string());
    for value in ["0", "30", "9223372036854775807"] {
        let out = seat.ljos(&["memories", "prune", "--stale", "--days", value]);
        let text = said(&out);
        assert_eq!(out.status.code(), Some(0), "{value}\n{text}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("0 stale; dry run"),
            "{value}\n{text}"
        );
    }
}
