//! Two sessions of one runner, with `LJOS_SEAT` unset, are two seats.
//!
//! Mail uses the session name. Recall still reads the runner's family.
//! An exported `LJOS_SEAT` is the seat.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::process::Command;

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

    fn ljos(&self, args: &[&str], extra: &[(&str, &str)]) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join(".config"))
            .env("XDG_STATE_HOME", self.home().join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("LJOS_SYNC_HOST", "box")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off");
        seat_env::clear(&mut cmd);
        for (key, value) in extra {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }
}

fn said(out: &std::process::Output) -> String {
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
fn two_sessions_differ_and_an_explicit_seat_wins() {
    let world = World::new();
    let one = world.ljos(
        &["seat"],
        &[("CLAUDE_CODE_SESSION_ID", "0199a1b2-claude-session-one")],
    );
    let two = world.ljos(
        &["seat"],
        &[("CLAUDE_CODE_SESSION_ID", "0199a1b2-claude-session-two")],
    );
    let left = said(&one);
    let right = said(&two);
    assert!(one.status.success(), "{left}");
    assert!(two.status.success(), "{right}");
    let left_out = String::from_utf8_lossy(&one.stdout);
    let right_out = String::from_utf8_lossy(&two.stdout);
    let a = field(&left_out, "seat");
    let b = field(&right_out, "seat");
    assert_ne!(a, b, "{left}{right}");
    assert!(a.starts_with("claude-"), "{left}");
    assert!(b.starts_with("claude-"), "{right}");
    assert!(
        String::from_utf8_lossy(&one.stdout).contains("CLAUDE_CODE_SESSION_ID"),
        "{left}"
    );

    let named = world.ljos(
        &["seat"],
        &[
            ("LJOS_SEAT", "ada"),
            ("CLAUDE_CODE_SESSION_ID", "0199a1b2-claude-session-one"),
        ],
    );
    let text = said(&named);
    assert!(named.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&named.stdout);
    assert_eq!(field(&stdout, "seat"), "ada", "{text}");
    assert!(field(&stdout, "source").contains("LJOS_SEAT"), "{text}");
}

/// The parent has a Claude session id. This process is started without it,
/// so the parent's id does not name the seat.
#[test]
fn a_parent_session_id_does_not_name_this_process() {
    let world = World::new();
    let id = "0199a1b2-claude-parent-only";
    let forbidden = format!("claude-{}", ljos_cli::session_tag(id));
    let previous = std::env::var_os("CLAUDE_CODE_SESSION_ID");
    std::env::set_var("CLAUDE_CODE_SESSION_ID", id);
    let out = world.ljos(&["seat"], &[]);
    match previous {
        Some(value) => std::env::set_var("CLAUDE_CODE_SESSION_ID", value),
        None => std::env::remove_var("CLAUDE_CODE_SESSION_ID"),
    }
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_ne!(field(&stdout, "seat"), forbidden.as_str(), "{text}");
    assert!(
        stdout.contains("every-seat") || stdout.contains("reading every seat"),
        "{text}"
    );
}
