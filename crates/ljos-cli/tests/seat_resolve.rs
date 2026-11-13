//! The seat rules, in order, and the read and write fallbacks.
//! An explicit `--seat` wins over every later rule.

ljos_test_boot::harness!();

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
fn an_explicit_seat_wins_and_each_later_rule_is_reached() {
    let world = World::new();

    let flagged = world.ljos(&["--seat", "saddle", "seat"], &[("LJOS_SEAT", "other")]);
    let text = said(&flagged);
    assert!(flagged.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&flagged.stdout);
    assert_eq!(field(&stdout, "seat"), "saddle", "{text}");
    assert!(field(&stdout, "source").contains("--seat"), "{text}");

    let from_env = world.ljos(&["seat"], &[("LJOS_SEAT", "envy")]);
    let text = said(&from_env);
    assert!(from_env.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&from_env.stdout);
    assert_eq!(field(&stdout, "seat"), "envy", "{text}");
    assert!(field(&stdout, "source").contains("LJOS_SEAT"), "{text}");

    std::fs::write(world.home().join(".config/ljos/env"), "seat=filed\n").unwrap();
    let from_file = world.ljos(&["seat"], &[]);
    let text = said(&from_file);
    assert!(from_file.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&from_file.stdout);
    assert_eq!(field(&stdout, "seat"), "filed", "{text}");
    assert!(stdout.contains("seat="), "{text}");
    std::fs::write(world.home().join(".config/ljos/env"), "").unwrap();

    let id = "bc-1234abcd-rest";
    std::fs::write(
        world.home().join(".config/ljos/seats.map"),
        format!("{id} mapped\n"),
    )
    .unwrap();
    let from_map = world.ljos(&["seat"], &[("CURSOR_CONVERSATION_ID", id)]);
    let text = said(&from_map);
    assert!(from_map.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&from_map.stdout);
    assert_eq!(field(&stdout, "seat"), "mapped", "{text}");
    assert!(stdout.contains("seats.map"), "{text}");
    std::fs::remove_file(world.home().join(".config/ljos/seats.map")).unwrap();

    std::fs::write(
        world.home().join(".config/ljos/seats.toml"),
        format!("{id:?} = \"toml-seat\"\n"),
    )
    .unwrap();
    let from_toml = world.ljos(&["seat"], &[("CURSOR_CONVERSATION_ID", id)]);
    let text = said(&from_toml);
    assert!(from_toml.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&from_toml.stdout);
    assert_eq!(field(&stdout, "seat"), "toml-seat", "{text}");
    assert!(stdout.contains("seats.toml"), "{text}");
    std::fs::remove_file(world.home().join(".config/ljos/seats.toml")).unwrap();

    let parent = std::process::id();
    let runtime = world.root.path().join("run").join("ljos");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(
        runtime.join(format!("seat-{parent}")),
        "parent-seat\nparent-seat\n",
    )
    .unwrap();
    let from_parent = world.ljos(&["seat"], &[]);
    let text = said(&from_parent);
    assert!(from_parent.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&from_parent.stdout);
    assert_eq!(field(&stdout, "seat"), "parent-seat", "{text}");
    assert!(stdout.contains("parent process seat record"), "{text}");
    std::fs::remove_file(runtime.join(format!("seat-{parent}"))).unwrap();

    std::fs::create_dir_all(world.home().join(".local/state/ljos")).unwrap();
    std::fs::write(
        world.home().join(".local/state/ljos/seat-writes.log"),
        "2026-11-11T00:00:00Z\trecent\tbox\n",
    )
    .unwrap();
    let from_recent = world.ljos(&["seat"], &[]);
    let text = said(&from_recent);
    assert!(from_recent.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&from_recent.stdout);
    assert_eq!(field(&stdout, "seat"), "recent", "{text}");
    assert!(
        stdout.contains("most recent write from this machine"),
        "{text}"
    );
}

#[test]
fn a_read_with_no_seat_covers_every_seat_and_a_write_names_unknown() {
    let world = World::new();
    let read = world.ljos(&["seat"], &[]);
    let text = said(&read);
    assert!(read.status.success(), "{text}");
    let both = format!(
        "{}{}",
        String::from_utf8_lossy(&read.stdout),
        String::from_utf8_lossy(&read.stderr)
    );
    assert!(both.contains("reading every seat"), "{text}");
    assert!(!both.contains("label seat-unresolved"), "{text}");

    let doctor = world.ljos(&["doctor", "--all"], &[]);
    let text = said(&doctor);
    let doctor_text = format!(
        "{}{}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    assert!(
        doctor_text.contains("reading every seat"),
        "doctor names the rule: {text}"
    );
    assert!(!doctor_text.contains("label seat-unresolved"), "{text}");

    let write = world.ljos(&["remember", "The lexical default is BM25+."], &[]);
    let text = said(&write);
    let write_text = format!(
        "{}{}",
        String::from_utf8_lossy(&write.stdout),
        String::from_utf8_lossy(&write.stderr)
    );
    assert!(write_text.contains("this write uses unknown-box"), "{text}");
    assert!(write_text.contains("--seat"), "{text}");
    assert!(write_text.contains("LJOS_SEAT"), "{text}");
    assert!(!write_text.contains("label seat-unresolved"), "{text}");
}

/// A conversation that already has a seat is that seat. The unknown-host
/// line must not name a different one.
#[test]
fn a_recorded_conversation_seat_is_not_called_unknown() {
    let world = World::new();
    let id = "conv-abc-12345678";
    // `policy` is not a read or a write, so it keeps the login seat and
    // leaves the conversation record. `seat` would not: a read with no
    // rule returns before that record is written.
    let primed = world.ljos(
        &["policy", "--", "true"],
        &[
            ("USER", "ubuntu"),
            ("CURSOR_CONVERSATION_ID", id),
            ("CURSOR_AGENT", "1"),
        ],
    );
    assert!(primed.status.success(), "{}", said(&primed));

    let write = world.ljos(
        &["remember", "The lexical default is BM25+."],
        &[
            ("CURSOR_CONVERSATION_ID", id),
            ("CURSOR_AGENT", "1"),
            ("USER", "ubuntu"),
            ("PACKSET_URL", "off"),
        ],
    );
    let text = said(&write);
    let err = String::from_utf8_lossy(&write.stderr);
    assert!(
        !err.contains("unknown-"),
        "the line named a seat the write does not use:\n{text}"
    );

    let seat = world.ljos(
        &["seat"],
        &[("CURSOR_CONVERSATION_ID", id), ("CURSOR_AGENT", "1")],
    );
    let text = said(&seat);
    assert!(seat.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&seat.stdout);
    assert_eq!(field(&stdout, "seat"), "ubuntu", "{text}");
}
