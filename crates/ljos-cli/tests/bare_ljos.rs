//! `ljos` with no arguments, and a mistyped command or flag.
//!
//! The bare screen names what ljos is, the seat a read would use and
//! where that came from, then search, remember, file and doctor, and
//! the command reference. A mistyped word names the closest real command
//! or flag and the command to run, and exits 2.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("TMPDIR", root)
        .env("LJOS_OFF", "1")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .stdin(Stdio::null());
    seat_env::clear(&mut cmd);
    for (key, value) in extra {
        cmd.env(key, value);
    }
    cmd.output()
        .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")))
}

fn screen(seat: &str) -> String {
    format!(
        "\
Memory, a work tracker and a command guard for coding runners.

{seat}

  ljos search QUERY   find what is stored
  ljos remember TEXT  store one lesson
  ljos file TITLE     open a ticket
  ljos doctor         check that ljos is ready

The docs are at https://leidarljos.github.io/ljos/reference-commands.html.
"
    )
}

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn bare_ljos_names_the_seat_and_the_first_commands() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &[], &[]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(out.stderr.is_empty(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        screen("no seat rule matched; reading every seat, so run `export LJOS_SEAT=NAME`.")
    );
}

#[test]
fn bare_ljos_names_an_exported_seat() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &[], &[("LJOS_SEAT", "ada")]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(out.stderr.is_empty(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        screen("This seat is ada, named by LJOS_SEAT.")
    );
}

#[test]
fn bare_ljos_names_a_session_seat() {
    let dir = tempfile::tempdir().unwrap();
    let id = "cursor-session-example";
    let seat = format!("cursor-{}", ljos_cli::session_tag(id));
    let out = run(dir.path(), &[], &[("CURSOR_CONVERSATION_ID", id)]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(out.stderr.is_empty(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        screen(&format!(
            "This seat is {seat}, named by CURSOR_CONVERSATION_ID in this process."
        ))
    );
}

#[test]
fn a_mistyped_search_names_search_and_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["serch"], &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "ljos: no command serch, and the closest is search, so run `ljos search`.\n"
    );
}

#[test]
fn a_mistyped_remember_names_remember_and_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["remeber"], &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "ljos: no command remeber, and the closest is remember, so run `ljos remember`.\n"
    );
}

#[test]
fn a_mistyped_lesson_flag_names_lesson_and_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["finish", "--lesons"], &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "ljos: finish has no flag --lesons, and the closest is --lesson, so run `ljos finish --lesson`.\n"
    );
}
