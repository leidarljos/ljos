//! A person at a terminal sits as the seat they named.
//!
//! A conversation id on the shell, the one a runner left when it started
//! the terminal, does not add a session tag. The sitting line names the
//! seat. The same id with no terminal tags the holder.

ljos_test_boot::harness!();

#[path = "common/terminal.rs"]
mod terminal;

use std::path::Path;
use std::process::{Command, Stdio};

fn script(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    ljos_cli::install_executable(path, body).unwrap();
}

fn text_of(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// `ljos sitting --plan` in `shape`, with a Cursor conversation id set.
fn sit(shape: &str) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let tracker = dir.path().join("tracker");
    let bin = dir.path().join("bin");
    let run = dir.path().join("run");
    for d in [&home, &tracker.join("Software"), &bin, &run] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(
        home.join(".gitconfig"),
        "[user]\n\tname = Your Name\n\temail = you@example.com\n",
    )
    .unwrap();
    script(
        &bin.join("claimdag"),
        "#!/bin/sh\ncase \"$1\" in\n  claim|upsert) echo gen=2 ;;\n  list) echo '[]' ;;\n  get) exit 1 ;;\nesac\nexit 0\n",
    );
    script(&bin.join("deedar"), "#!/bin/sh\necho head\nexit 0\n");
    script(
        &bin.join("vissue"),
        "#!/bin/sh\ncase \"$1\" in\n  identity) printf 'vissue test\\nroot=%s\\nprefix=Software\\n' \"$VISSUE_ROOT\" ;;\n  claim|note|update) echo ok ;;\n  show) printf '%s\\n' \"{\\\"id\\\":\\\"$2\\\",\\\"title\\\":\\\"Ship the codec\\\",\\\"state\\\":\\\"TODO\\\"}\" ;;\n  children) echo '[]' ;;\n  *) exit 0 ;;\nesac\n",
    );
    let args = [
        "sitting",
        "--plan",
        "Ship the codec",
        "-p",
        "day",
        "--",
        "measure the files",
        "pick a default",
        "write the note",
    ];
    let shaped = terminal::shape(shape, env!("CARGO_BIN_EXE_ljos"), &args);
    let mut cmd = Command::new(if shape == "terminal" {
        terminal::PYTHON
    } else {
        "setsid"
    });
    if shape == "terminal" {
        cmd.args(&shaped);
    } else {
        cmd.arg("-w").arg(env!("CARGO_BIN_EXE_ljos")).args(args);
    }
    cmd.env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "alice")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .env("PACKSET_AUTH", "off")
        .env(
            "CURSOR_CONVERSATION_ID",
            "bc-872164c1-e14f-56d0-a32e-fc0bd5afad41",
        )
        .env("CURSOR_AGENT", "1")
        .current_dir(&home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

#[test]
fn a_terminal_sitting_names_the_seat_and_a_runner_sitting_keeps_the_tag() {
    let person = sit("terminal");
    let said = text_of(&person);
    assert!(person.status.success(), "{said}");
    let started = said
        .lines()
        .find(|line| line.contains("STARTED under"))
        .unwrap_or_else(|| panic!("no STARTED line:\n{said}"));
    assert!(started.contains("STARTED under alice"), "{started}\n{said}");
    assert!(
        !started.contains('@'),
        "a terminal sitting tagged a runner session:\n{started}\n{said}"
    );
    assert_eq!(
        said.lines()
            .filter(|line| line.starts_with("next:"))
            .count(),
        1,
        "a planned sitting printed its next item more than once:\n{said}"
    );

    let runner = sit("runner");
    let said = text_of(&runner);
    assert!(runner.status.success(), "{said}");
    let started = said
        .lines()
        .find(|line| line.contains("STARTED under"))
        .unwrap_or_else(|| panic!("no STARTED line:\n{said}"));
    assert!(
        started.contains("STARTED under alice@"),
        "a runner sitting dropped the session tag:\n{started}\n{said}"
    );
}
