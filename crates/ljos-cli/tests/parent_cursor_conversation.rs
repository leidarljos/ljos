//! A parent shell's `CURSOR_CONVERSATION_ID` does not name this `ljos` process.
//!
//! The parent's conversation can still leave a record under `XDG_RUNTIME_DIR`.
//! An `ljos` call without the id in its own environment must not take that seat.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::process::{Command, Output, Stdio};

struct World {
    root: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".config/ljos")).unwrap();
        std::fs::create_dir_all(home.join(".local/state")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        Self { root }
    }

    fn home(&self) -> std::path::PathBuf {
        self.root.path().join("home")
    }

    fn run(&self) -> std::path::PathBuf {
        self.root.path().join("run")
    }

    fn under_parent(&self, parent_id: &str, script: &str) -> Output {
        let home = self.home();
        let run = self.run();
        let body = format!(
            "export HOME='{}' XDG_CONFIG_HOME='{}/.config' \
             XDG_STATE_HOME='{}/.local/state' XDG_RUNTIME_DIR='{}' \
             PATH=/usr/bin:/bin LJOS_OFF=1 LJOS_HOOKD=off PACKSET_URL=off; \
             env -u CURSOR_CONVERSATION_ID -u CURSOR_AGENT -u LJOS_SEAT {script}",
            home.display(),
            home.display(),
            home.display(),
            run.display(),
        );
        let mut cmd = Command::new("bash");
        cmd.args(["--noprofile", "--norc", "-c", &body])
            .env("CURSOR_CONVERSATION_ID", parent_id)
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", &run)
            .stdin(Stdio::null());
        seat_env::clear(&mut cmd);
        cmd.output().unwrap()
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

fn field<'a>(text: &'a str, name: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{name}\t")))
        .unwrap_or_else(|| panic!("no {name} in {text}"))
}

#[test]
fn a_parent_cursor_id_and_its_session_record_do_not_name_this_ljos() {
    let world = World::new();
    let parent_id = "0199c0de-cursor-parent-only";
    let forbidden = format!("cursor-{}", ljos_cli::session_tag(parent_id));
    let tag = ljos_cli::session_tag(parent_id);
    let record = world.run().join(format!("session-{tag}"));
    std::fs::write(&record, format!("mcp-seat\nmcp-holder\nids\t{parent_id}\n")).unwrap();
    let bin = env!("CARGO_BIN_EXE_ljos");
    let seat = world.under_parent(parent_id, &format!("{bin} seat"));
    let text = said(&seat);
    assert!(seat.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&seat.stdout);
    assert_ne!(field(&stdout, "seat"), forbidden.as_str(), "{text}");
    assert!(
        !field(&stdout, "source").contains("this conversation's record"),
        "{text}"
    );
    assert!(
        stdout.contains("every-seat") || stdout.contains("reading every seat"),
        "{text}"
    );
}
