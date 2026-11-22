//! A panic after `ljos_cli::output::install_panic_hook` is written as
//! prose on stderr, so on a terminal it wraps to the width. The test runs
//! itself again on a pty and panics there.

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;

use std::process::{Command, Stdio};

const CHILD: &str = "LJOS_PANIC_WRAP_CHILD";

const MESSAGE: &str = "the kiln log could not be read because the damper readings were cut off before the glaze batch was opened";

#[test]
fn a_panic_message_wraps_on_a_tty() {
    if std::env::var_os(CHILD).is_some() {
        ljos_cli::output::install_panic_hook();
        panic!("{MESSAGE}");
    }
    let cols = 40;
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "a_panic_message_wraps_on_a_tty",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(CHILD, "1")
    .env("COLUMNS", cols.to_string())
    .env_remove("RUST_BACKTRACE")
    .stdin(Stdio::null());
    let term = pty::Pty::open();
    term.attach_output(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, 30);
    term.finish_reading();
    let shown = term.shown().replace("\r\n", "\n");
    assert!(!out.status.success(), "{shown}");
    let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(words(&shown).contains(MESSAGE), "{shown}");
    assert!(shown.contains("panicked at"), "{shown}");
    let start = shown.find("\nthe kiln log").expect("message line");
    for line in shown[start + 1..]
        .lines()
        .take_while(|line| !line.is_empty())
    {
        if line.starts_with("note: ") {
            break;
        }
        assert!(line.chars().count() <= cols, "{line:?}\n{shown}");
    }
}
