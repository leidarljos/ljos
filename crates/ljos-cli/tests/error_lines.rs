//! One line on stderr for the failures a person hits, and the command that fixes it.
//!
//! Exit 2 is a bad invocation. Exit 1 is a runtime failure. Exit 75 is a
//! writer that a later run can reach. The operating system's words come
//! from `from_raw_os_error`, so the line is the same on macOS.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

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

    fn write_issues(&self, body: &str) {
        std::fs::write(self.root.join("tracker/Software/demo/issues.org"), body).unwrap();
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
    assert!(out.status.success(), "{said}");
    assert_eq!(
        one_line(&out.stdout),
        "Memory busy: queued 1 write, it will be saved on a later call."
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
        "vissue is not on PATH. Install it with `cargo install --locked vissue-cli`."
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
        "claimdag is not on PATH. Install it with `cargo install --locked claimdag-cli`."
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
            "proposals: --status takes open, accepted, refused, already-in-pack, or in-pack, not an empty value."
        } else {
            "proposals: --status takes open, accepted, refused, already-in-pack, or in-pack, not bogus."
        };
        assert_eq!(one_line(&out.stderr), want, "{bad:?}\n{said}");
    }
}
