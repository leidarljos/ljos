//! One line on stderr for the failures a person hits, and the command that fixes it.
//!
//! Exit 2 is a bad invocation. Exit 1 is a runtime failure. Exit 75 is a
//! writer that a later run can reach. The operating system's words come
//! from `from_raw_os_error`, so the line is the same on macOS.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

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
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
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

    fn write_issues(&self, body: &str) {
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(self.root.join("tracker/Software/demo/issues.org"), body).unwrap();
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let bin = self.root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let path = bin.join(name);
        ljos_cli::install_executable(&path, body).unwrap();
        bin
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let home = self.root.join("home");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
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
    assert!(out.status.success(), "{said}");
    assert_eq!(
        one_line(&out.stdout),
        "Memory busy: queued 1 write. A later ljos call from any seat will try to save it, and the pack may still refuse it."
    );
    assert!(out.stderr.is_empty(), "{said}");
}

#[test]
fn a_missing_tracker_program_names_the_install_and_exits_1() {
    let home = Home::new();
    let out = home.run(&["note", "demo-kiln", "hello"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        format!(
            "vissue is not on PATH. Install it with `{}`.",
            ljos_cli::RELEASE_INSTALL_COMMAND
        )
    );
}

#[test]
fn conflicts_names_the_missing_landscape_program_and_its_source() {
    let home = Home::new();
    let out = home.run(&["conflicts"], &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "conflicts: `landscape` was not found on PATH; install it from https://github.com/leidarljos/landscape"
    );
}

#[test]
fn a_missing_claim_program_names_the_install_and_exits_1() {
    let home = Home::new();
    // A TODO ticket that nobody has claimed stops before the claim graph.
    // A claimed sitting still has to name a missing claimdag.
    home.write_issues(
        "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#B] Fire the kiln
:PROPERTIES:
:ID:         demo-kiln
:CLAIMED_BY: alice
:CREATED:    [2026-06-01 Mon]
:END:
",
    );
    let out = home.run(&["finish", "demo-kiln"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        format!(
            "claimdag is not on PATH. Install it with `{}`.",
            ljos_cli::RELEASE_INSTALL_COMMAND
        )
    );
}

#[test]
fn an_unknown_ticket_on_a_sitting_names_search_and_exits_1() {
    let home = Home::new();
    let out = home.run(&["sitting", "demo-gone"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "issue demo-gone not found. `ljos search` finds a ticket."
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
fn an_unknown_ticket_on_tracker_verbs_names_ready_and_exits_1() {
    let home = Home::new();
    let bag = home.root.join("bag");
    let bag_str = bag.to_string_lossy().into_owned();
    let cases: [&[&str]; 14] = [
        &["claim", "demo-gone"],
        &["release", "demo-gone"],
        &["complete", "demo-gone"],
        &["playbook", "demo-gone", "sit"],
        &["note", "demo-gone", "still cold"],
        &["vote", "demo-gone", "--for", "keep", "--used", "none"],
        &["consensus", "demo-gone"],
        &["deed", "demo-gone"],
        &["options", "demo-gone", "keep,drop"],
        &["block", "demo-gone", "demo-kiln"],
        &["withdraw", "demo-gone"],
        &["learn", "demo-gone", "--outcome", "keep"],
        &["handover", "--out", &bag_str, "--issue", "demo-gone"],
        &["predict", "demo-gone", "--expect", "keep"],
    ];
    for args in cases {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(1), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(
            one_line(&out.stderr),
            "issue demo-gone not found. `ljos ready` lists the open tickets.",
            "{args:?}\n{said}"
        );
    }
    let prefix = home.run(&["claim", "demo-ki"], &[("LJOS_SEAT", "alice")]);
    let said = text(&prefix);
    assert_eq!(prefix.status.code(), Some(1), "{said}");
    assert!(prefix.stdout.is_empty(), "{said}");
    assert_eq!(
        one_line(&prefix.stderr),
        "issue demo-ki not found, and the closest is demo-kiln, so run `ljos show demo-kiln`."
    );
}

#[test]
fn a_mistyped_ticket_id_names_the_closest_ticket_on_show_note_and_finish() {
    let home = Home::new();
    for args in [
        &["show", "demo-kln"][..],
        &["note", "demo-kln", "left a note"][..],
        &["finish", "demo-kln"][..],
    ] {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(1), "{args:?}\n{said}");
        assert_eq!(
            one_line(&out.stderr),
            "issue demo-kln not found, and the closest is demo-kiln, so run `ljos show demo-kiln`.",
            "{args:?}\n{said}"
        );
    }
}

#[test]
fn missing_arguments_on_nested_commands_and_empty_prefer_or_policy_name_their_help() {
    let home = Home::new();
    let cases: [(&[&str], &str); 11] = [
        (
            &["seat", "add"],
            "seat add needs <NAME>. `ljos seat add --help` lists what to pass.",
        ),
        (
            &["seat", "grant"],
            "seat grant needs --quote <QUOTE>, --at <AT>, --via <VIA>, <SEAT>, <SPEC>. `ljos seat grant --help` lists what to pass.",
        ),
        (
            &["memory", "export"],
            "memory export needs --out <OUT>. `ljos memory export --help` lists what to pass.",
        ),
        (
            &["memory", "import"],
            "memory import needs <PATH>. `ljos memory import --help` lists what to pass.",
        ),
        (
            &["proposals", "amend"],
            "proposals amend needs --text <TEXT>, <ID>. `ljos proposals amend --help` lists what to pass.",
        ),
        (
            &["record", "check"],
            "record check needs <DIR>. `ljos record check --help` lists what to pass.",
        ),
        (
            &["mesh", "pull"],
            "mesh pull needs --seat <SEAT>, --from <DIR>. `ljos mesh pull --help` lists what to pass.",
        ),
        (
            &["mesh", "push"],
            "mesh push needs --seat <SEAT>, --to <DIR>. `ljos mesh push --help` lists what to pass.",
        ),
        (
            &["prefer"],
            "prefer: the text is empty, so run `ljos prefer \"TEXT\"`.",
        ),
        (
            &["prefer", "--user"],
            "prefer: the text is empty, so run `ljos prefer --user --why REASON \"TEXT\"`.",
        ),
        (
            &["policy"],
            "policy: pass the command to check, so run `ljos policy -- git push --force`.",
        ),
    ];
    for (args, want) in cases {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(one_line(&out.stderr), want, "{args:?}\n{said}");
    }
}

#[test]
fn a_note_with_no_sitting_names_sit() {
    let home = Home::new();
    let out = home.run(&["note", "the glaze crawled"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(one_line(&out.stderr), ljos_cli::NO_OPEN_SITTING);
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
fn a_limit_without_a_value_names_the_long_flag() {
    let home = Home::new();
    let out = home.run(&["search", "kiln", "-n"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "search: --limit needs a value",
        "{said}"
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
    ljos_cli::change_mode(&config, 0o555).unwrap();
    ljos_cli::change_mode(config.join("ljos"), 0o555).unwrap();
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

    let _open = ljos_cli::change_mode(&config, 0o755);

    let _open = ljos_cli::change_mode(config.join("ljos"), 0o755);
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

#[test]
fn a_missing_tracker_names_ljos_onboard_and_exits_1() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let work = root.path().join("work");
    for sub in [".config/ljos", ".local/state/ljos", ".local/share"] {
        std::fs::create_dir_all(home.join(sub)).unwrap();
    }
    std::fs::create_dir_all(&work).unwrap();
    let path_env = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
    let run_unboarded = |args: &[&str], extra_root: Option<&std::path::Path>| {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(&work)
            .env_clear()
            .env("PATH", &path_env)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", root.path().join("run"))
            .env("TMPDIR", root.path())
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_OFF", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_SEAT", "alice");
        if let Some(dir) = extra_root {
            cmd.env("VISSUE_ROOT", dir);
        }
        cmd.output().unwrap()
    };

    let ready = run_unboarded(&["ready"], None);
    let ready_said = text(&ready);
    assert_eq!(ready.status.code(), Some(1), "{ready_said}");
    assert_eq!(
        one_line(&ready.stderr),
        "No tracker found, so run `ljos onboard`. To make one by hand, read https://leidarljos.github.io/ljos/howto-by-hand.html"
    );

    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let filed = run_unboarded(&["file", "Pick a codec", "-p", "demo"], None);
    let filed_said = text(&filed);
    assert_eq!(filed.status.code(), Some(1), "{filed_said}");
    assert_eq!(
        one_line(&filed.stderr),
        "No tracker found, so run `ljos onboard`. To make one by hand, read https://leidarljos.github.io/ljos/howto-by-hand.html"
    );

    let noted = run_unboarded(&["note", "demo-1234", "note text"], None);
    let noted_said = text(&noted);
    assert_eq!(noted.status.code(), Some(1), "{noted_said}");
    assert_eq!(
        one_line(&noted.stderr),
        "No tracker found, so run `ljos onboard`. To make one by hand, read https://leidarljos.github.io/ljos/howto-by-hand.html"
    );

    let shown = run_unboarded(&["show", "demo-1234"], None);
    let shown_said = text(&shown);
    assert_eq!(shown.status.code(), Some(1), "{shown_said}");
    assert_eq!(
        one_line(&shown.stderr),
        "No tracker found, so run `ljos onboard`. To make one by hand, read https://leidarljos.github.io/ljos/howto-by-hand.html"
    );

    let missing_named = root.path().join("missing-tracker");
    let named_out = run_unboarded(
        &["file", "Pick a codec", "-p", "demo"],
        Some(&missing_named),
    );
    let named_said = text(&named_out);
    assert_eq!(named_out.status.code(), Some(1), "{named_said}");
    assert!(
        !one_line(&named_out.stderr).contains("ljos onboard"),
        "{named_said}"
    );
}

#[test]
fn file_without_a_project_names_the_command_to_run() {
    let home = Home::new();
    let out = home.run(&["file", "Repair the kiln door"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "no project: name one with -p NAME, so run `ljos file \"TITLE\" -p NAME`."
    );
}

#[test]
fn a_corrupt_tracker_config_names_doctor() {
    let home = Home::new();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(home.root.join("tracker/vissue.toml"), "{bad toml").unwrap();
    let out = home.run(&["show", "demo-kiln"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "the tracker config does not parse, so run `ljos doctor`."
    );
}

#[test]
fn free_text_on_recall_points_at_search_and_exits_2() {
    let home = Home::new();
    let cases: [(&[&str], &str); 4] = [
        (
            &["recall", "usability first-run ljos cli"],
            "recall: \"usability first-run ljos cli\" is not a ticket id, so run `ljos search \"usability first-run ljos cli\"`.",
        ),
        (
            &["recall", "usability"],
            "recall: \"usability\" is not a ticket id, so run `ljos search \"usability\"`.",
        ),
        (
            &["recall", "   "],
            "recall: the issue id is empty, so run `ljos recall proj-1a2b`.",
        ),
        (
            &["recall", "--", "-flag"],
            "recall: \"-flag\" is not a ticket id, so run `ljos search -- \"-flag\"`.",
        ),
    ];
    for (args, want) in cases {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(one_line(&out.stderr), want, "{args:?}\n{said}");
    }

    // A 32-hex tracker id is an id. A missing one names ready, not search.
    let hex = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let out = home.run(&["recall", hex], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_ne!(out.status.code(), Some(2), "{said}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("is not a ticket id"), "{said}");
    assert!(err.contains("`ljos ready`"), "{said}");
}

#[test]
fn search_with_packset_url_off_reports_packs_turned_off() {
    let home = Home::new();
    let out = home.run(&["search", "usability"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "This seat has packs turned off (PACKSET_URL=off), so nothing was matched by words. Run `unset PACKSET_URL` to turn them on."
    );
    assert!(!said.contains("writer is busy"), "{said}");
}

#[test]
fn unknown_proposals_status_names_valid_values_and_exits_2() {
    let home = Home::new();
    for bad in ["bogus", ""] {
        let out = home.run(&["proposals", "--status", bad], &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{bad:?}\n{said}");
        assert!(out.stdout.is_empty(), "{bad:?}\n{said}");
        let want = if bad.is_empty() {
            "proposals: --status takes open, proposed, accepted, refused, already-in-pack, in-pack, or already in pack, not an empty value."
        } else {
            "proposals: --status takes open, proposed, accepted, refused, already-in-pack, in-pack, or already in pack, not bogus."
        };
        assert_eq!(one_line(&out.stderr), want, "{bad:?}\n{said}");
    }
    for good in ["proposed", "already in pack", "open"] {
        let out = home.run(&["proposals", "--status", good], &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_ne!(out.status.code(), Some(2), "{good}\n{said}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "no proposals\n",
            "{good}\n{said}"
        );
    }
}

#[test]
fn impossible_as_of_dates_exit_2() {
    let home = Home::new();
    let bad = [
        "2026-02-31",
        "2026-01-01garbage",
        "2025-02-29",
        "1900-02-29",
        "2026-04-31",
    ];
    for value in bad {
        let out = home.run(
            &["search", "kiln", "--as-of", value],
            &[("LJOS_SEAT", "alice")],
        );
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{value}\n{said}");
        assert!(out.stdout.is_empty(), "{value}\n{said}");
        assert_eq!(
            one_line(&out.stderr),
            format!("search: --as-of \"{value}\" is not a date; write YYYY-MM-DD or RFC 3339."),
            "{value}\n{said}"
        );
    }
    // A real day, including a leap day, and an RFC 3339 time, are not usage errors.
    for value in [
        "2024-02-29",
        "2000-02-29",
        "2026-01-01",
        "2026-01-01T00:00:00Z",
    ] {
        let out = home.run(
            &["search", "kiln", "--as-of", value],
            &[("LJOS_SEAT", "alice")],
        );
        let said = text(&out);
        assert_ne!(out.status.code(), Some(2), "{value}\n{said}");
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("is not a date"),
            "{value}\n{said}"
        );
    }
}

#[test]
fn invalid_flag_values_on_search_and_remember_exit_2() {
    let home = Home::new();
    let cases: [(&[&str], &str); 6] = [
        (
            &["search", "kiln", "--min-score=-1"],
            "search: --min-score takes a number from 0 up, not -1.",
        ),
        (
            &["search", "kiln", "--min-score", "inf"],
            "search: --min-score needs a finite number, not inf.",
        ),
        (
            &["search", "kiln", "--as-of", "bogus"],
            "search: --as-of \"bogus\" is not a date; write YYYY-MM-DD or RFC 3339.",
        ),
        (
            &["search", "kiln", "--valid-at", "bogus"],
            "search: --valid-at takes YYYY-MM-DD or any, not bogus.",
        ),
        (
            &[
                "remember",
                "--transient",
                "--standing",
                "The kiln fires at dusk.",
            ],
            "remember: pass --transient or --standing, not both.",
        ),
        (
            &[
                "remember",
                "--valid-from",
                "2026-05-10",
                "--valid-to",
                "2026-05-01",
                "The kiln fires at dusk.",
            ],
            "the window ends (2026-05-01) before it starts (2026-05-10)",
        ),
    ];
    for (args, want) in cases {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(one_line(&out.stderr), want, "{args:?}\n{said}");
    }
}
#[test]
fn equals_binds_a_hyphen_leading_lesson_value() {
    let home = Home::new();
    let out = home.run(
        &["finish", "demo-kiln", "--lesson=--json"],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&out);
    assert_ne!(out.status.code(), Some(2), "{said}");
    assert!(!said.contains("--lesson needs a value"), "{said}");
}

#[test]
fn a_negative_on_every_remaining_numeric_flag_names_the_value() {
    let home = Home::new();
    let cases: &[(&[&str], &str)] = &[
        (
            &["panel", "demo-kiln", "--designers", "-1"],
            "panel: --designers takes a number from 0 up, not -1.",
        ),
        (
            &["claim", "--slack", "-1"],
            "claim: --slack takes a number from 0 up, not -1.",
        ),
        (
            &["learn", "demo-kiln", "--outcome", "keep", "--beta", "-0.5"],
            "learn: --beta takes a number in (0, 1), not -0.5.",
        ),
        (
            &["learn", "demo-kiln", "--outcome", "keep", "--share", "-1"],
            "learn: --share takes a number in [0, 1), not -1.",
        ),
        (
            &["finish", "demo-kiln", "--beta", "-0.5"],
            "finish: --beta takes a number in (0, 1), not -0.5.",
        ),
        (
            &["trust", "alice", "bob", "-0.5"],
            "trust: weight takes a number in (0, 1], not -0.5.",
        ),
        (
            &[
                "persona",
                "reviewer",
                "--anchor",
                "-0.5",
                "--view",
                "reads diffs",
            ],
            "persona: --anchor takes a number in [0, 1], not -0.5.",
        ),
        (
            &[
                "persona",
                "reviewer",
                "--anchor=-0.5",
                "--view",
                "reads diffs",
            ],
            "persona: --anchor takes a number in [0, 1], not -0.5.",
        ),
        (
            &[
                "supervise",
                "worker",
                "--harness",
                "codex",
                "--exit",
                "clean",
                "--at",
                "-1",
            ],
            "supervise: --at takes a number from 0 up, not -1.",
        ),
        (
            &[
                "supervise",
                "worker",
                "--harness",
                "codex",
                "--exit",
                "clean",
                "--at",
                "0",
                "--max-restarts",
                "-1",
            ],
            "supervise: --max-restarts takes a number from 0 up, not -1.",
        ),
        (
            &[
                "supervise",
                "worker",
                "--harness",
                "codex",
                "--exit",
                "clean",
                "--at",
                "0",
                "--window",
                "-5",
            ],
            "supervise: --window takes a number from 0 up, not -5.",
        ),
        (
            &[
                "supervise",
                "worker",
                "--harness",
                "codex",
                "--exit",
                "clean",
                "--at",
                "0",
                "--failures",
                "-1",
            ],
            "supervise: --failures takes a number from 0 up, not -1.",
        ),
        (
            &[
                "supervise",
                "worker",
                "--harness",
                "codex",
                "--exit",
                "clean",
                "--at",
                "0",
                "--failures=-1",
            ],
            "supervise: --failures takes a number from 0 up, not -1.",
        ),
        (
            &["complete", "demo-kiln", "--gen", "-1"],
            "complete: --gen takes a number from 0 up, not -1.",
        ),
        (
            &["finish", "demo-kiln", "--gen", "-2"],
            "finish: --gen takes a number from 0 up, not -2.",
        ),
        (
            &[
                "handover",
                "--out",
                "bag",
                "--issue",
                "demo-kiln",
                "--since-size",
                "-1",
            ],
            "handover: --since-size takes a number from 0 up, not -1.",
        ),
        (
            &["table", "demo-kiln", "--show", "scores", "--version", "-1"],
            "table: --version takes a number from 0 up, not -1.",
        ),
        (
            &["table", "demo-kiln", "--show", "scores", "--version=-1"],
            "table: --version takes a number from 0 up, not -1.",
        ),
    ];
    for (args, want) in cases {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(one_line(&out.stderr), *want, "{args:?}\n{said}");
        assert!(
            !said.contains("has no flag") && !said.contains("unexpected argument"),
            "a negative was read as a flag:\n{said}"
        );
    }
    let habit = home.run(&["habit", "temp", "-5"], &[("LJOS_SEAT", "alice")]);
    let said = text(&habit);
    assert!(
        !said.contains("has no flag") && !said.contains("unexpected argument"),
        "a habit reading was read as a flag:\n{said}"
    );
}

#[test]
fn negative_zero_reaches_every_documented_numeric_flag() {
    let home = Home::new();
    let cases: &[(&[&str], bool)] = &[
        (
            &[
                "persona",
                "reviewer",
                "--anchor",
                "-0",
                "--view",
                "reads diffs",
            ],
            true,
        ),
        (&["ready", "-n", "-0"], true),
        (&["search", "kiln", "--limit", "-0"], true),
        (&["calibrate", "-p", "demo", "--rounds", "-0"], true),
        (
            &["wake", "alice", "--command", "true", "--timeout", "-0"],
            true,
        ),
        (
            &["panel", "demo-kiln", "--designers", "-0", "--dry-run"],
            true,
        ),
        (&["claim", "--next", "--slack", "-0"], true),
        (
            &["learn", "demo-kiln", "--outcome", "keep", "--beta", "-0"],
            false,
        ),
        (
            &["learn", "demo-kiln", "--outcome", "keep", "--share", "-0"],
            true,
        ),
        (
            &[
                "supervise",
                "worker",
                "--harness",
                "codex",
                "--exit",
                "clean",
                "--at",
                "0",
                "--failures",
                "-0",
            ],
            true,
        ),
        (&["memories", "prune", "--stale", "--days", "-0"], true),
        (&["claims", "--stale", "--days", "-0"], true),
        (&["release", "--stale", "--days", "-0", "--yes"], true),
        (
            &["vote", "demo-kiln", "--for", "keep", "--confidence", "-0"],
            false,
        ),
        (
            &[
                "file",
                "Title",
                "-p",
                "demo",
                "--quorum",
                "-0",
                "--deadline",
                "2030-01-01T00:00:00Z",
            ],
            true,
        ),
        (
            &["table", "demo-kiln", "--show", "scores", "--version", "-0"],
            true,
        ),
    ];
    for (args, zero_is_in_range) in cases {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        if *zero_is_in_range {
            assert_ne!(out.status.code(), Some(2), "{args:?}\n{said}");
        }
        assert!(!said.contains("does not accept -0"), "{args:?}\n{said}");
        assert!(!said.contains("has no flag"), "{args:?}\n{said}");
        assert!(!said.contains("unexpected argument"), "{args:?}\n{said}");
    }

    let huge = home.run(
        &[
            "memories",
            "prune",
            "--stale",
            "--days",
            "9223372036854775808",
        ],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&huge);
    assert_ne!(huge.status.code(), Some(2), "{said}");
    assert!(!said.contains("does not accept"), "{said}");
}

#[test]
fn positional_flags_seat_values_and_negative_zero_are_unambiguous() {
    let home = Home::new();
    let cases: &[(&[&str], Option<i32>, &str)] = &[
        (
            &["file", "-hello", "-p", "demo"],
            Some(2),
            "pass `--` before text that starts with `-`",
        ),
        (&["file", "-task", "-p", "demo"], Some(2), "<TITLE>"),
        (
            &["finish", "demo-kiln", "-hello"],
            Some(2),
            "pass `--` before text that starts with `-`",
        ),
        (
            &["file", "--unknown-flag"],
            Some(2),
            "pass `--` before text that starts with `-`",
        ),
        (&["search", "kiln", "--limit", "-0"], None, ""),
        (&["conflicts", "--limit", "-0"], None, ""),
        (
            &["panel", "demo-kiln", "--designers", "-0", "--dry-run"],
            None,
            "",
        ),
    ];
    for (args, status, want) in cases {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        if let Some(status) = status {
            assert_eq!(out.status.code(), Some(*status), "{args:?}\n{said}");
            assert!(out.stdout.is_empty(), "{args:?}\n{said}");
            assert!(one_line(&out.stderr).contains(want), "{args:?}\n{said}");
        } else {
            assert_ne!(out.status.code(), Some(2), "{args:?}\n{said}");
            assert!(
                !said.contains("takes a number from 0 up"),
                "{args:?}\n{said}"
            );
            assert!(!said.contains("Usage:"), "{args:?}\n{said}");
            assert!(!said.contains("needs a value"), "{args:?}\n{said}");
        }
    }
}

#[test]
fn forwarded_argv_skips_ljos_short_flag_checks() {
    let home = Home::new();
    for args in [
        &["git-hook", "pre-push", "-x", "origin"][..],
        &["git-hook", "pre-push", "--bogus", "origin"][..],
        &["evidence", "-x"][..],
        &["evidence", "--bogus"][..],
    ] {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_ne!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(!said.contains("has no flag"), "{args:?}\n{said}");
        assert!(!said.contains("needs a value"), "{args:?}\n{said}");
    }
}

#[test]
fn non_text_hyphen_values_name_the_whole_value_flag() {
    let home = Home::new();
    for (args, flag, fragment) in [
        (&["file", "-p", "-demo", "Title"][..], "--project", "-d"),
        (
            &["file", "-t", "-bug", "-p", "demo", "Title"][..],
            "--type",
            "-b",
        ),
    ] {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert_eq!(
            one_line(&out.stderr),
            format!("file: {flag} needs a value"),
            "{args:?}\n{said}"
        );
        assert!(!said.contains(&format!("has no flag {fragment}")), "{said}");
    }
}

#[test]
fn clap_metadata_disambiguates_attached_flags_values_and_positionals() {
    let home = Home::new();
    for args in [
        &["file", "-p", "demo", "-tbug", "Title"][..],
        &["file", "--body", "-hello", "-p", "demo", "Title"][..],
        &["finish", "demo-1", "--lesson", "-hello"][..],
        &["file", "--body", "-quick", "-p", "demo", "Title"][..],
    ] {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_ne!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(!said.contains("Usage:"), "{args:?}\n{said}");
        assert!(!said.contains("needs a value"), "{args:?}\n{said}");
    }

    for (args, command) in [
        (&["search", "file", "-hello"][..], "search"),
        (&["file", "-p", "demo", "-hello", "--", "Title"][..], "file"),
    ] {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        let line = one_line(&out.stderr);
        assert!(
            line.starts_with(&format!("ljos: {command}")),
            "{args:?}\n{said}"
        );
        assert!(
            line.contains("pass `--` before text that starts with `-`"),
            "{args:?}\n{said}"
        );
    }
}

#[test]
fn a_blank_rule_pattern_exits_1() {
    let home = Home::new();
    let out = home.run(
        &["rule", " ", "--why", "the command is named"],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("label rule-input-warning:"), "{said}");
    assert!(
        stderr.contains("a pattern over the command line is required"),
        "{said}"
    );
    assert!(out.stdout.is_empty(), "{said}");
}
#[test]
fn no_write_send_and_options_refusals_exit_1_on_stderr() {
    let home = Home::new();
    for (args, label) in [
        (&["send", "bob", ""][..], "label message-input-warning:"),
        (
            &["options", "demo-kiln", "only"][..],
            "label panel-incomplete:",
        ),
    ] {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(1), "{args:?}\n{said}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(label),
            "{args:?}\n{said}"
        );
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
    }
}

#[test]
fn policy_refusals_use_stderr_and_leave_stdout_empty() {
    if tools::halt(tools::Tool::Packsetd) || tools::halt(tools::Tool::Vissue) {
        return;
    }
    let pack = ljos_test_boot::Packsetd::open();
    let long_message = "x".repeat(16_001);
    let cases = vec![
        (vec!["send", "bob", ""], "message-input-warning"),
        (vec!["send", "bob", "   "], "message-input-warning"),
        (
            vec!["send", "bob", long_message.as_str()],
            "message-input-warning",
        ),
        (vec!["send", "alice", "hello"], "routing-warning"),
        (vec!["options", "demo-kiln", ""], "panel-incomplete"),
        (vec!["options", "demo-kiln", "only"], "panel-incomplete"),
        (
            vec!["rule", " ", "--why", "the command is named"],
            "rule-input-warning",
        ),
        (
            vec!["rule", "git status", "--why", ""],
            "rule-input-warning",
        ),
        (
            vec!["rule", "git status", "--verdict", "maybe", "--why", "test"],
            "rule-input-warning",
        ),
        (
            vec!["rule", "git push*", "--why", "name the branch"],
            "rule-input-warning",
        ),
        (vec!["playbook", "demo-kiln", ""], "playbook-warning"),
        (
            vec!["playbook", "demo-kiln", "not-a-playbook"],
            "playbook-warning",
        ),
        (
            vec!["persona", "", "--view", "reads diffs"],
            "persona-owner-warning",
        ),
        (
            vec!["persona", "reviewer", "--view", ""],
            "persona-owner-warning",
        ),
        (
            vec!["persona", "missing", "--retire"],
            "persona-owner-warning",
        ),
        (
            vec![
                "memories",
                "review",
                "--edit",
                "deadbeef",
                "--text",
                "replacement",
            ],
            "review-input-warning",
        ),
        (
            vec!["accept", "deadbeef", "--quote", "yes", "--via", "alice"],
            "approval-unverified",
        ),
        (
            vec![
                "accept",
                "deadbeef",
                "--quote",
                "yes",
                "--at",
                "2026-11-20T00:00:00Z",
            ],
            "approval-unverified",
        ),
        (
            vec![
                "accept",
                "deadbeef",
                "--quote",
                "yes",
                "--at",
                "2026-11-20T00:00:00Z",
                "--via",
                "alice",
            ],
            "approval-unverified",
        ),
        (
            vec![
                "seat",
                "grant",
                "bob",
                "close=own",
                "--quote",
                "",
                "--at",
                "2026-11-20T00:00:00Z",
                "--via",
                "alice",
            ],
            "grant-unverified",
        ),
        (
            vec![
                "seat",
                "grant",
                "bob",
                "close=own",
                "--quote",
                "yes, grant it",
                "--at",
                "not-a-time",
                "--via",
                "alice",
            ],
            "grant-unverified",
        ),
        (
            vec![
                "seat",
                "grant",
                "alice",
                "close=own",
                "--quote",
                "yes, grant it",
                "--at",
                "2026-11-20T00:00:00Z",
                "--via",
                "alice",
            ],
            "grant-unverified",
        ),
        (vec!["graded", "deadbeef"], "review-not-shown"),
        (
            vec!["claim", "demo-kiln", "--assignee", "bob"],
            "assignee-mismatch",
        ),
    ];
    for (args, label) in cases {
        let home = Home::new();
        let out = home.run(
            &args,
            &[("LJOS_SEAT", "alice"), ("PACKSET_URL", pack.url())],
        );
        let said = text(&out);
        assert!(!out.status.success(), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&format!("label {label}:")),
            "{args:?}\n{said}"
        );
    }

    let path = std::env::var("PATH").unwrap();
    for (args, label, seat) in [
        (
            &["block", "demo-kiln", "--on", "demo-kiln"][..],
            "graph-cycle",
            Some("alice"),
        ),
        (&["reopen", "demo-kiln"], "reopen-warning", Some("alice")),
        (
            &["claim", "demo-kiln", "--assignee", "example-host-sand-sub"],
            "seat-unresolved",
            Some("alice"),
        ),
    ] {
        let home = Home::new();
        let mut env = vec![("PACKSET_URL", pack.url()), ("PATH", path.as_str())];
        if let Some(seat) = seat {
            env.push(("LJOS_SEAT", seat));
        }
        let out = home.run(args, &env);
        let said = text(&out);
        assert!(!out.status.success(), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(&format!("label {label}:")),
            "{args:?}\n{said}"
        );
    }

    let home = Home::new();
    ljos_cli::write_file(
        home.root.join("tracker/vissue.toml"),
        "prefix = \"Software\"\nseats = [\"alice\"]\n",
    )
    .unwrap();
    let out = home.run(
        &["claim", "demo-kiln"],
        &[
            ("LJOS_SEAT", "stranger"),
            ("PACKSET_URL", pack.url()),
            ("PATH", path.as_str()),
        ],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("label seat-unlisted:"),
        "{said}"
    );

    let home = Home::new();
    let bin = home.script("claimdag", "#!/bin/sh\nprintf '[]\\n'\n");
    let stub_path = format!("{}:{path}", bin.display());
    let out = home.run(
        &["claim", "--next"],
        &[
            ("LJOS_SEAT", "alice"),
            ("PACKSET_URL", pack.url()),
            ("PATH", &stub_path),
        ],
    );
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert!(
        String::from_utf8_lossy(&out.stderr).starts_with("label claim-conflict:"),
        "{said}"
    );

    let home = Home::new();
    let tracker = home.root.join("tracker");
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "test"],
        &["config", "user.email", "test@example.invalid"],
        &["config", "commit.gpgsign", "false"],
        &["add", "."],
        &["commit", "-q", "-m", "seed"],
    ] {
        let out = ljos_cli::program("git")
            .unwrap()
            .args(args)
            .current_dir(&tracker)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", text(&out));
    }
    let common = [("PACKSET_URL", pack.url()), ("PATH", path.as_str())];
    for seat in ["alice", "bob"] {
        let added = home.run(
            &["seat", "add", seat],
            &[common[0], common[1], ("LJOS_SEAT", "alice")],
        );
        assert!(added.status.success(), "{seat}: {}", text(&added));
    }
    let granted = home.run(
        &[
            "seat",
            "grant",
            "bob",
            "close=demo-kiln",
            "--quote",
            "yes, bob may close it",
            "--at",
            "2026-11-20T00:00:00Z",
            "--via",
            "alice",
        ],
        &[common[0], common[1], ("LJOS_SEAT", "alice")],
    );
    assert!(granted.status.success(), "{}", text(&granted));
    let issue_path = tracker.join("Software/demo/issues.org");
    let mut issues = std::fs::read_to_string(&issue_path).unwrap();
    issues.push_str(
        "\n* TODO [#C] Cool the kiln\n:PROPERTIES:\n:ID: demo-cool\n:PARENT: demo-kiln\n:END:\n",
    );
    home.write_issues(&issues);
    let claimed = home.run(
        &["claim", "demo-kiln"],
        &[common[0], common[1], ("LJOS_SEAT", "bob")],
    );
    assert!(claimed.status.success(), "{}", text(&claimed));
    let out = home.run(
        &["finish", "demo-kiln", "--close"],
        &[common[0], common[1], ("LJOS_SEAT", "bob")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert!(
        String::from_utf8_lossy(&out.stderr).starts_with("label children-open:"),
        "{said}"
    );
}

#[test]
fn a_very_long_idle_span_does_not_overflow() {
    let home = Home::new();
    let out = home.run(
        &["reap", "--idle", "99999999999999999d"],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&out);
    assert_ne!(out.status.code(), Some(101), "{said}");
    assert!(!said.contains("panicked"), "{said}");
}

#[test]
fn an_empty_value_on_a_count_flag_is_named() {
    let home = Home::new();
    let out = home.run(
        &["search", "kiln", "--limit", ""],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "search: --limit does not accept \"\". The values it takes are in `ljos search --help`.",
        "{said}"
    );
}

#[test]
fn wake_refuses_a_timeout_that_does_not_fit_toml() {
    let home = Home::new();
    let out = home.run(
        &[
            "wake",
            "alice",
            "--command",
            "true",
            "--timeout",
            "18446744073709551615",
        ],
        &[("LJOS_SEAT", "alice")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(out.stdout.is_empty(), "{said}");
    assert_eq!(
        one_line(&out.stderr),
        "wake: --timeout takes a number no larger than 9223372036854775807, not 18446744073709551615.",
        "{said}"
    );
}

#[test]
fn a_span_flag_that_is_not_a_span_exits_2() {
    let home = Home::new();
    let cases: &[(&[&str], &str)] = &[
        (
            &["reap", "--idle", "-1"],
            "reap: --idle \"-1\" wants a number and s, m, h or d",
        ),
        (
            &["reap", "--idle=-1h"],
            "reap: --idle \"-1h\" wants a number and s, m, h or d",
        ),
        (
            &["reap", "--idle", "0"],
            "reap: --idle \"0\" must be at least 1m because claim times have minute resolution",
        ),
        (
            &["reap", "--idle=0s"],
            "reap: --idle \"0s\" must be at least 1m because claim times have minute resolution",
        ),
        (
            &["reap", "--idle", "59s"],
            "reap: --idle \"59s\" must be at least 1m because claim times have minute resolution",
        ),
        (
            &["release", "--stale", "--older-than", "-1"],
            "release: --older-than wants a duration such as 900, 15m, 6h or 2d",
        ),
        (
            &["release", "--stale", "--older-than=-1h"],
            "release: --older-than wants a duration such as 900, 15m, 6h or 2d",
        ),
        (
            &["habit", "temp", "5", "--every", "-1"],
            "habit: --every must be positive",
        ),
        (
            &["habit", "temp", "5", "--every=0d"],
            "habit: --every must be positive",
        ),
        (
            &["habit", "temp", "5", "--every=9223372036854775807w"],
            "habit: --every \"9223372036854775807w\" is too long; write 7d, 24h, 2w or 30m",
        ),
        (
            &["reap", "--idle", "18446744073709551615h"],
            "reap: --idle \"18446744073709551615h\" does not fit in a timestamp",
        ),
        (
            &["reap", "--idle=18446744073709551615h", "--apply"],
            "reap: --idle \"18446744073709551615h\" does not fit in a timestamp",
        ),
        (
            &[
                "release",
                "--stale",
                "--older-than",
                "18446744073709551615h",
            ],
            "release: --older-than \"18446744073709551615h\" does not fit in a timestamp",
        ),
    ];
    for (args, want) in cases {
        let out = home.run(args, &[("LJOS_SEAT", "alice")]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(one_line(&out.stderr), *want, "{args:?}\n{said}");
    }
}
