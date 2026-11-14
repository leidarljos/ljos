//! One line on stderr for the failures a person hits, and the command that fixes it.
//!
//! Exit 2 is a bad invocation. Exit 1 is a runtime failure. Exit 75 is a
//! writer that a later run can reach. The operating system's words come
//! from `from_raw_os_error`, so the line is the same on macOS.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "work",
            "tracker/Software/demo",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        std::fs::write(
            root.join("tracker/Software/demo/issues.org"),
            "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#B] Fire the kiln
:PROPERTIES:
:ID:         demo-kiln
:CREATED:    [2026-06-01 Mon]
:END:

* TODO [#B] Glaze the tile
:PROPERTIES:
:ID:         demo-tile
:CREATED:    [2026-06-01 Mon]
:END:
",
        )
        .unwrap();
        Self { root }
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_OFF", "1")
            .env("PACKSET_URL", "off")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LANG", "C.UTF-8")
            .stdin(std::process::Stdio::null());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }
}

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn one_line(bytes: &[u8]) -> String {
    let lines: Vec<_> = String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    lines.into_iter().next().unwrap()
}

fn os_words(code: i32) -> String {
    let text = std::io::Error::from_raw_os_error(code).to_string();
    match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => text[..at].to_string(),
        _ => text,
    }
}

#[test]
fn a_turned_off_pack_names_unset_and_exits_1() {
    let home = Home::new();
    let out = home.run(
        &["remember", "The kiln fires at dusk."],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "this seat has packs turned off (PACKSET_URL=off); 1 lesson is queued. Run `unset PACKSET_URL` to send it."
    );
    assert!(!said.contains("Caused by"), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
}

#[test]
fn a_slow_writer_names_search_and_exits_75() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _held = listener;
        std::thread::sleep(std::time::Duration::from_secs(8));
    });
    let home = Home::new();
    let url = format!("http://127.0.0.1:{port}");
    let out = home.run(
        &["remember", "The kiln fires at dusk."],
        &[
            ("LJOS_SEAT", "alice"),
            ("PACKSET_URL", &url),
            ("PACKSET_AUTH", "off"),
            ("PACKSET_TIMEOUT_MS", "300"),
        ],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(75), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "pack busy: the lesson is queued and will be sent on the next call. Retry later with `ljos remember`."
    );
}

#[test]
fn a_missing_tracker_program_names_the_install_and_exits_1() {
    let home = Home::new();
    let out = home.run(&["note", "demo-kiln", "hello"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "vissue is not on PATH. Install it with `cargo install --locked vissue-cli`."
    );
}

#[test]
fn a_missing_claim_program_names_the_install_and_exits_1() {
    let home = Home::new();
    let out = home.run(&["finish", "demo-kiln"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "claimdag is not on PATH. Install it with `cargo install --locked claimdag-cli`."
    );
}

#[test]
fn an_unknown_ticket_on_a_sitting_names_ready_and_exits_1() {
    let home = Home::new();
    let out = home.run(&["sitting", "demo-gone"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "issue demo-gone not found. `ljos ready` lists the open tickets."
    );
}

#[test]
fn an_unknown_ticket_on_a_timeline_names_ready_and_exits_1() {
    let home = Home::new();
    let out = home.run(&["timeline", "demo-gone"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "issue demo-gone not found. `ljos ready` lists the open tickets."
    );
}

#[test]
fn a_note_with_no_sitting_names_sit() {
    let home = Home::new();
    let out = home.run(&["note", "the glaze crawled"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert_eq!(
        one_line(&out.stdout),
        "label unattached-note: no open sitting. Start one with `ljos sitting`."
    );
    assert!(out.stderr.is_empty(), "{said}");
}

#[test]
fn a_bad_limit_names_help_and_exits_2() {
    let home = Home::new();
    let out = home.run(
        &["search", "--limit", "nope", "kiln"],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "search: --limit does not accept nope. The values it takes are in `ljos search --help`."
    );
}

#[test]
fn a_missing_since_names_help_and_exits_2() {
    let home = Home::new();
    let out = home.run(&["usage"], &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "usage needs a start day. Pass it as `ljos usage --since YYYY-MM-DD`."
    );
}

#[test]
fn a_bad_day_names_the_flag_and_exits_2() {
    let home = Home::new();
    let out = home.run(
        &[
            "remember",
            "--valid-from",
            "yesterday",
            "The kiln fires at dusk.",
        ],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "remember: --valid-from takes a day written YYYY-MM-DD, not yesterday."
    );
}

#[test]
fn a_bad_exit_kind_names_the_command_and_exits_2() {
    let home = Home::new();
    let out = home.run(
        &[
            "supervise",
            "demo",
            "--harness",
            "shell",
            "--exit",
            "banana",
            "--at",
            "1",
        ],
        &[],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "supervise: --exit takes clean or crash, not banana."
    );
}

#[test]
fn a_failed_upgrade_download_names_the_retry() {
    let why = "https://api.github.com/repos/leidarljos/ljos/releases/latest: status code 403";
    assert_eq!(
        ljos_cli::upgrade::unreachable_release("ljos", why),
        format!(
            "could not reach the ljos release ({why}). Check the network, then run `ljos upgrade`."
        )
    );
}

#[test]
fn a_read_only_store_names_doctor_and_exits_1() {
    let home = Home::new();
    let config = home.root.join("home/.config");
    let mode = std::fs::Permissions::from_mode(0o555);
    std::fs::set_permissions(&config, mode).unwrap();
    std::fs::set_permissions(config.join("ljos"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = home.run(
        &[
            "prefer",
            "--user",
            "--why",
            "because",
            "Keep the glaze thin.",
        ],
        &[("LJOS_SEAT", "alice")],
    );
    let _open = std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o755));
    let _open =
        std::fs::set_permissions(config.join("ljos"), std::fs::Permissions::from_mode(0o755));
    let said = text(&out);
    let words = os_words(libc::EACCES);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        format!("The store is not writable ({words}). `ljos doctor` names the directory to fix.")
    );
    assert!(!said.contains("os error"), "{said}");
}

#[test]
fn a_full_disk_names_the_command_again() {
    let again = "ljos remember TEXT";
    let err = anyhow::Error::from(std::io::Error::from_raw_os_error(libc::ENOSPC));
    let line = ljos_cli::user_fault_for(&err, again);
    let words = os_words(libc::ENOSPC);
    assert_eq!(
        line,
        format!("The disk is full ({words}). Free some space, then run `{again}` again.")
    );
    assert!(!line.contains("os error"));
    assert!(!line.contains('\n'));
    let quota = anyhow::Error::from(std::io::Error::from_raw_os_error(libc::EDQUOT));
    let line = ljos_cli::user_fault_for(&quota, again);
    assert!(line.contains(&os_words(libc::EDQUOT)), "{line}");
    let read_only = anyhow::Error::from(std::io::Error::from_raw_os_error(libc::EROFS));
    assert_eq!(
        ljos_cli::user_fault_for(&read_only, again),
        format!(
            "The store is not writable ({}). `ljos doctor` names the directory to fix.",
            os_words(libc::EROFS)
        )
    );
}

#[test]
fn debug_prints_the_cause_after_the_line() {
    let home = Home::new();
    let out = home.run(
        &["remember", "The kiln fires at dusk."],
        &[("LJOS_SEAT", "alice"), ("LJOS_DEBUG", "1")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    let err = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<_> = err.lines().filter(|line| !line.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "{err}");
    assert_eq!(
        lines[0],
        "this seat has packs turned off (PACKSET_URL=off); 1 lesson is queued. Run `unset PACKSET_URL` to send it."
    );
    assert!(lines[1].contains("packset url missing"), "{err}");
    assert!(!err.contains("Caused by"), "{err}");
}
