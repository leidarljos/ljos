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
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new("bash");
        seat_env::clear(&mut cmd);
        cmd.args(["--noprofile", "--norc", "-c", &body])
            .env("CURSOR_CONVERSATION_ID", parent_id)
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", &run)
            .stdin(Stdio::null());
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
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
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
        stdout.contains("no seat is set")
            || stdout.contains("every-seat")
            || stdout.contains("reading every seat"),
        "{text}"
    );
}

#[test]
fn a_parent_cursor_id_in_the_same_home_does_not_tag_a_named_seat_under_env_i() {
    let world = World::new();
    let home = world.home();
    let run = world.run();
    let bin = env!("CARGO_BIN_EXE_ljos");
    // The trailing `exit` keeps bash from exec'ing `env`, so the shell
    // that carries the id stays the parent of this `ljos`.
    let script = format!(
        "env -i HOME='{home}' XDG_CONFIG_HOME='{home}/.config' \
         XDG_STATE_HOME='{home}/.local/state' XDG_RUNTIME_DIR='{run}' \
         PATH=/usr/bin:/bin LJOS_OFF=1 LJOS_HOOKD=off PACKSET_URL=off \
         LJOS_SEAT=alice {bin} seat; exit $?",
        home = home.display(),
        run = run.display(),
    );
    let mut cmd = ljos_cli::program("bash").unwrap();
    seat_env::clear(&mut cmd);
    cmd.args(["--noprofile", "--norc", "-c", &script])
        .env("CURSOR_CONVERSATION_ID", "0199c0de-cursor-parent-only")
        .env("CURSOR_AGENT", "1")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &run)
        .stdin(Stdio::null());
    let out = cmd.output().unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(field(&stdout, "seat"), "alice", "{text}");
    assert_eq!(field(&stdout, "holder"), "alice", "{text}");
    assert!(
        !field(&stdout, "source").contains("CURSOR_CONVERSATION_ID"),
        "{text}"
    );
}

/// A program on `PATH` for the sitting below.
fn script(dir: &std::path::Path, name: &str, body: &str) {
    ljos_cli::install_executable(&dir.join(name), body).unwrap();
}

/// The output of one step of a shell script that prints `--- NAME` before
/// each step and `NAME=STATUS` after it.
fn step<'a>(text: &'a str, name: &str) -> (&'a str, &'a str) {
    let body = text
        .split_once(&format!("--- {name}\n"))
        .unwrap_or_else(|| panic!("no step {name} in {text}"))
        .1;
    let status = format!("{name}=");
    let (body, rest) = match body.strip_prefix(&status) {
        Some(rest) => ("", rest),
        None => body
            .split_once(&format!("\n{status}"))
            .unwrap_or_else(|| panic!("no status for {name} in {text}")),
    };
    (body, rest.lines().next().unwrap_or(""))
}

/// The sitting a bare `env -i` call opens under a `CURSOR_AGENT` shell is
/// that call's own: it names no conversation, and the next `env -i` call
/// in the same shell reads it back. A runner by name sits above the
/// shell, so the sitting gate is active for each call.
#[test]
fn a_parent_cursor_id_does_not_attribute_a_sitting_under_env_i() {
    let world = World::new();
    let root = world.root.path();
    let home = world.home();
    let run = world.run();
    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let tracker = root.join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"alice\"]\n").unwrap();
    script(
        &bin_dir,
        "vissue",
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"probe-sit\",\"title\":\"probe\",\"state\":\"TODO\"}}' ;;\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\n  vote) printf '%s\\n' '[{{\"agent\":\"alice\",\"choice\":\"approve\"}}]' ;;\n  note) printf '%s: noted\\n' \"$2\" >&2 ;;\nesac\nexit 0\n",
            tracker.display()
        ),
    );
    script(
        &bin_dir,
        "claimdag",
        "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\nesac\necho '[]'\nexit 0\n",
    );
    script(&bin_dir, "deedar", "#!/bin/sh\necho head\nexit 0\n");
    script(&bin_dir, "curl", "#!/bin/sh\nexit 6\n");
    let runner = bin_dir.join("opencode");
    #[expect(
        clippy::disallowed_methods,
        reason = "links a path that is not a git hook"
    )]
    std::os::unix::fs::symlink("/bin/bash", &runner).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "t@example.invalid"],
        &["config", "user.name", "t"],
        &["config", "commit.gpgsign", "false"],
        &["config", "core.hooksPath", "/dev/null"],
        &["commit", "-q", "--allow-empty", "-m", "seed"],
    ] {
        let out = ljos_cli::program("git")
            .unwrap()
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", said(&out));
    }
    let layout = vissue_core::Layout::new(&tracker, "Software");
    vissue_core::ops::create(&layout, "probe", "probe work", Default::default()).unwrap();
    let doc =
        vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe")).unwrap();
    let issue = doc.headings[0].id.clone();
    let parent_id = "0199c0de-cursor-parent-only";
    let bin = env!("CARGO_BIN_EXE_ljos");
    // Each call is a child of this shell, which carries the id and the
    // mark: `env -i` runs from the function, so bash does not exec it.
    let script = format!(
        "bare() {{ env -i HOME='{home}' XDG_CONFIG_HOME='{home}/.config' \
         XDG_STATE_HOME='{home}/.local/state' XDG_RUNTIME_DIR='{run}' \
         PATH='{path}:/usr/bin:/bin' VISSUE_ROOT='{tracker}' ISSUE_ROOT='{tracker}' \
         VISSUE_NO_ROUTE=1 LJOS_TRACKER_GIT=off LJOS_HOOKD=off PACKSET_URL=off \
         LJOS_SEAT=alice LJOS_UNSIGNED_BALLOTS=allow {bin} \"$@\"; }}\n\
         cd '{repo}' || exit 1\n\
         echo '--- sitting'; bare sitting {issue}; echo \"sitting=$?\"\n\
         echo '--- doctor'; bare doctor 2>&1; echo \"doctor=$?\"\n\
         echo '--- note'; bare note 'a working note' 2>&1; echo \"note=$?\"\n\
         head=$(git rev-parse HEAD)\n\
         echo '--- push'; printf 'refs/heads/develop %s refs/heads/develop %s\\n' \"$head\" \"$head\" \
         | bare git-hook pre-push origin x 2>&1; echo \"push=$?\"\n\
         exit 0",
        home = home.display(),
        run = run.display(),
        path = bin_dir.display(),
        tracker = tracker.display(),
        repo = repo.display(),
    );
    let mut cmd = ljos_cli::program("setsid").unwrap();
    seat_env::clear(&mut cmd);
    cmd.args(["-w".as_ref(), runner.as_os_str()])
        .args(["--noprofile", "--norc", "-c", &script])
        .env("CURSOR_CONVERSATION_ID", parent_id)
        .env("CURSOR_AGENT", "1")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &run)
        .stdin(Stdio::null());
    let out = cmd.output().unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (_, sitting) = step(&stdout, "sitting");
    assert_eq!(sitting, "0", "{text}");
    let (doctor, _) = step(&stdout, "doctor");
    let gate = doctor
        .lines()
        .find(|line| line.contains("\tsitting gate\t"))
        .unwrap_or_else(|| panic!("no sitting gate row\n{text}"));
    assert!(
        gate.contains(&issue) && !gate.contains("holds no live sitting"),
        "the next call does not read the hold back: {gate}\n{text}"
    );
    let (note, status) = step(&stdout, "note");
    assert_eq!(status, "0", "{text}");
    assert!(note.contains(&format!("{issue}: noted")), "{text}");
    let (push, status) = step(&stdout, "push");
    assert_eq!(status, "0", "{text}");
    assert!(
        !push.contains("sitting-gate"),
        "the pre-push sitting check did not pass: {push}\n{text}"
    );
    let holds: Vec<String> = std::fs::read_dir(run.join("ljos"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("hold-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert!(!holds.is_empty(), "the sitting wrote no hold\n{text}");
    for hold in &holds {
        let lines: Vec<&str> = hold.lines().collect();
        assert_eq!(lines.first(), Some(&"alice"), "{hold}\n{text}");
        assert_eq!(
            lines.get(8),
            Some(&""),
            "the hold names a conversation: {hold}"
        );
        assert!(!hold.contains(parent_id), "{hold}");
    }
}

#[test]
fn a_parent_cursor_id_in_another_home_does_not_leak_after_a_write_without_xdg_runtime_dir() {
    let parent_root = tempfile::tempdir().unwrap();
    let child_root = tempfile::tempdir().unwrap();
    let child_home = child_root.path().join("home");
    std::fs::create_dir_all(child_home.join(".config/ljos")).unwrap();
    std::fs::create_dir_all(child_home.join(".local/state/ljos")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        child_home.join(".local/state/ljos/seat-writes.log"),
        "2026-11-18T19:00:00.000Z\tada\tbox\n",
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_ljos");
    let stub = child_root.path().join("bin");
    std::fs::create_dir_all(&stub).unwrap();
    let curl = stub.join("curl");
    ljos_cli::install_executable(&curl, "#!/bin/sh\nexit 1\n").unwrap();
    let script = format!(
        "unset XDG_RUNTIME_DIR CURSOR_CONVERSATION_ID CURSOR_AGENT LJOS_SEAT; \
         export HOME='{}' XDG_CONFIG_HOME='{}/.config' XDG_STATE_HOME='{}/.local/state' \
         XDG_CACHE_HOME='{}/.cache' LJOS_SYNC_HOST=box PATH='{}:/usr/bin:/bin' LJOS_OFF=1 LJOS_HOOKD=off PACKSET_URL=off; \
         {bin} seat; printf '%s\\n' '---'; {bin} doctor || true",
        child_home.display(),
        child_home.display(),
        child_home.display(),
        child_home.display(),
        stub.display(),
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = Command::new("bash");
    seat_env::clear(&mut cmd);
    cmd.args(["--noprofile", "--norc", "-c", &script])
        .env("CURSOR_CONVERSATION_ID", "0199c0de-cursor-parent-only")
        .env("HOME", parent_root.path())
        .stdin(Stdio::null());
    let out = cmd.output().unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (seat_out, doctor_out) = stdout.split_once("\n---\n").unwrap_or((&stdout, ""));
    assert_eq!(field(seat_out, "seat"), "ada", "{text}");
    assert_eq!(field(seat_out, "holder"), "ada", "{text}");
    assert_eq!(
        field(seat_out, "source"),
        "most recent write from this machine",
        "{text}"
    );
    assert!(
        !stdout.contains("CURSOR_CONVERSATION_ID"),
        "ancestor CURSOR_CONVERSATION_ID leaked across HOME:\n{text}"
    );
    assert!(
        doctor_out.contains("ok\tseat source\tada named by most recent write from this machine\n"),
        "doctor leaked ancestor session:\n{text}"
    );
}
