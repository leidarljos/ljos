//! `ljos` with no arguments, and a mistyped command or flag.
//!
//! The bare screen names what ljos is, the seat a read would use and
//! where that came from, then install, doctor, remember and search, and
//! the command reference. A mistyped word names the closest real command
//! or flag and the command to run, and exits 2.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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
    let install = ljos_cli::RELEASE_INSTALL_COMMAND;
    let width = install.len();
    format!(
        "\
Memory, a work tracker and a command guard for coding runners.

{seat}

  {install:<width$}  install ljos
  {doctor:<width$}  check that ljos is ready
  {remember:<width$}  store one lesson
  {search:<width$}  find what is stored

The docs are at https://leidarljos.github.io/ljos/reference-commands.html.
",
        doctor = "ljos doctor",
        remember = "ljos remember TEXT",
        search = "ljos search QUERY",
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
fn a_cargo_build_names_install_and_an_installed_copy_does_not() {
    assert!(ljos_cli::exe_from_cargo_target(std::path::Path::new(
        "/work/ljos/target/debug/ljos"
    )));
    assert!(ljos_cli::exe_from_cargo_target(std::path::Path::new(
        "/work/ljos/target/release/deps/ljos-abc"
    )));
    assert!(!ljos_cli::exe_from_cargo_target(std::path::Path::new(
        "/home/ada/.local/bin/ljos"
    )));
}

/// A `CARGO_TARGET_DIR` of another name is still a cargo build: cargo
/// leaves `.cargo-lock` in the profile directory it builds into.
#[test]
fn a_cargo_build_in_another_target_dir_names_install() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("build-out/debug");
    std::fs::create_dir_all(profile.join("deps")).unwrap();
    assert!(!ljos_cli::exe_from_cargo_build(&profile.join("ljos")));
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(profile.join(".cargo-lock"), "").unwrap();
    assert!(ljos_cli::exe_from_cargo_build(&profile.join("ljos")));
    assert!(ljos_cli::exe_from_cargo_build(
        &profile.join("deps/ljos-abc")
    ));
    let installed = dir.path().join("bin");
    std::fs::create_dir_all(&installed).unwrap();
    assert!(!ljos_cli::exe_from_cargo_build(&installed.join("ljos")));
    assert!(ljos_cli::exe_from_cargo_build(std::path::Path::new(
        "/work/ljos/target/debug/ljos"
    )));
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
        screen("No seat is set. Reads use every seat, so run `export LJOS_SEAT=NAME`.")
    );
}

#[test]
fn seat_says_none_is_set_instead_of_naming_the_read_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["seat"], &[]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(out.stderr.is_empty(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "seat\tnone\n\
         holder\tnone\n\
         source\tno seat is set; reads use every seat\n\
         pick\tSet LJOS_SEAT with `export LJOS_SEAT=NAME`.\n"
    );
}

#[test]
fn seat_help_and_docs_report_none_instead_of_every_seat() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["seat", "--help"], &[]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("every-seat"),
        "seat --help still names every-seat:\n{stdout}"
    );
    assert!(
        stdout.contains("`none`") && stdout.contains("every seat"),
        "seat --help missing none/every seat:\n{stdout}"
    );
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for rel in [
        "docs/orgmode/howto-agents.org",
        "docs/source/howto-agents.rst",
        "docs/orgmode/howto-seats.org",
        "docs/source/howto-seats.rst",
    ] {
        let body = std::fs::read_to_string(root.join(rel)).unwrap();
        assert!(
            !body.contains("every-seat"),
            "{rel} still says a read prints every-seat"
        );
    }
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

/// The line a person reads. Stdout stays empty and the status is 2.
const TYPO_LINES: &[(&[&str], &str)] = &[
    (
        &["serch"],
        "ljos: no command serch, and the closest is search, so run `ljos search`.\n",
    ),
    (
        &["remeber"],
        "ljos: no command remeber, and the closest is remember, so run `ljos remember`.\n",
    ),
    (
        &["finsh", "proj-1a2b"],
        "ljos: no command finsh, and the closest is finish, so run `ljos finish <value>`.\n",
    ),
    (
        &["search", "--josn", "kiln"],
        "ljos: search has no flag --josn, and the closest is --json, so run `ljos search --json <value>`.\n",
    ),
    (
        &["find", "kiln"],
        "ljos: no command find, and the closest is search, so run `ljos search <value>`.\n",
    ),
    (
        &["add", "cold metal"],
        "ljos: no command add, and the closest is remember, so run `ljos remember <value>`.\n",
    ),
    (
        &["note-add", "the", "door", "sticks"],
        "ljos: no command note-add, and the closest is note, so run `ljos note <value>`.\n",
    ),
    (
        &["--seat", "ada", "find", "kiln"],
        "ljos: no command find, and the closest is search, so run `ljos --seat <value> search <value>`.\n",
    ),
    (
        &["search", "-x"],
        "ljos: search has no flag -x, so run `ljos search --help`.\n",
    ),
    (
        &["completions"],
        "ljos: completions needs a shell, so run `ljos completions bash`.\n",
    ),
    (
        &["completions", "bsh"],
        "ljos: no shell bsh, and the closest is bash, so run `ljos completions bash`.\n",
    ),
    (
        &["completions", "pwsh"],
        "ljos: no shell pwsh, and the closest is powershell, so run `ljos completions powershell`.\n",
    ),
    (
        &["finish", "--lesons"],
        "ljos: finish has no flag --lesons, and the closest is --lesson, so run `ljos finish --lesson`.\n",
    ),
    (
        &["help", "serch"],
        "ljos: no command serch, and the closest is search, so run `ljos help search`.\n",
    ),
    (
        &["help", "find"],
        "ljos: no command find, and the closest is search, so run `ljos help search`.\n",
    ),
    (
        &["close", "demo-4b1s"],
        "ljos: no command close, and the closest is finish, so run `ljos finish <value>`.\n",
    ),
    (
        &["create", "Pick a codec"],
        "ljos: no command create, and the closest is file, so run `ljos file <value>`.\n",
    ),
    (
        &["new", "Pick a codec"],
        "ljos: no command new, and the closest is file, so run `ljos file <value>`.\n",
    ),
    (
        &["delete", "a6ae2751"],
        "ljos: no command delete, and the closest is forget, so run `ljos forget <value>`.\n",
    ),
    (
        &["rm", "a6ae2751"],
        "ljos: no command rm, and the closest is forget, so run `ljos forget <value>`.\n",
    ),
    (
        &["status"],
        "ljos: no command status, and the closest is doctor, so run `ljos doctor`.\n",
    ),
    (
        &["stats"],
        "ljos: no command stats, and the closest is digest, so run `ljos digest`.\n",
    ),
    (
        &["edit"],
        "ljos: no command edit, and the closest is memories, so run `ljos memories`.\n",
    ),
    (
        &["help", "close"],
        "ljos: no command close, and the closest is finish, so run `ljos help finish`.\n",
    ),
    (
        &["lessons"],
        "ljos: no command lessons. To find lessons run `ljos search \"<words>\"`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`.\n",
    ),
    (
        &["lesson"],
        "ljos: no command lesson. To find lessons run `ljos search \"<words>\"`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`.\n",
    ),
    (
        &["learnings"],
        "ljos: no command learnings. To find lessons run `ljos search \"<words>\"`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`.\n",
    ),
    (
        &["Lessons"],
        "ljos: no command Lessons. To find lessons run `ljos search \"<words>\"`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`.\n",
    ),
    (
        &["help", "lessons"],
        "ljos: no command lessons. To find lessons run `ljos search \"<words>\"`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`.\n",
    ),
    (
        &["lessons", "kiln"],
        "ljos: no command lessons. To find lessons run `ljos search \"<words>\"`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`.\n",
    ),
    (
        &["--seat", "ada", "lessons", "cold", "metal", "-p", "demo"],
        "ljos: no command lessons. To find lessons run `ljos search \"<words>\"`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`.\n",
    ),
    (
        &["lessons", "--json"],
        "ljos: no command lessons. To find lessons run `ljos search \"<words>\"`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`.\n",
    ),
    (
        &["prop"],
        "ljos: no command prop, and the closest is proposals, so run `ljos proposals`.\n",
    ),
    (
        &["remembering", "cold metal"],
        "ljos: no command remembering, and the closest is remember, so run `ljos remember <value>`.\n",
    ),
    (
        &["searches", "kiln"],
        "ljos: no command searches, and the closest is search, so run `ljos search <value>`.\n",
    ),
    (
        &["search-lessons", "kiln"],
        "ljos: no command search-lessons, and the closest is search, so run `ljos search <value>`.\n",
    ),
    (
        &["note_add", "the door sticks"],
        "ljos: no command note_add, and the closest is note, so run `ljos note <value>`.\n",
    ),
    (
        &["memorize", "cold metal"],
        "ljos: no command memorize, and the closest is remember, so run `ljos remember <value>`.\n",
    ),
    (
        &["review"],
        "ljos: no command review, and the closest is proposals, so run `ljos proposals`.\n",
    ),
    (
        &["help", "prop"],
        "ljos: no command prop, and the closest is proposals, so run `ljos help proposals`.\n",
    ),
];

#[test]
fn mistyped_commands_and_flags_print_one_line() {
    let dir = tempfile::tempdir().unwrap();
    for (args, line) in TYPO_LINES {
        let out = run(dir.path(), args, &[]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), *line, "{args:?}");
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("Usage:"),
            "{args:?} printed clap's usage\n{said}"
        );
    }
}

#[test]
fn help_search_prints_the_search_page() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["help", "search"], &[]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(out.stderr.is_empty(), "{said}");
    let body = String::from_utf8_lossy(&out.stdout);
    assert!(
        body.contains("Usage: ljos search"),
        "help search did not show search\n{body}"
    );
    assert!(
        body.contains("ljos search"),
        "help search did not name the command\n{body}"
    );
}

#[test]
fn listing_words_point_at_ready_and_claims() {
    let dir = tempfile::tempdir().unwrap();
    let line = |word: &str| {
        format!(
            "ljos: no command {word}, and the closest is ready, so run `ljos ready`. `ljos claims` lists what is held.\n"
        )
    };
    for word in ["issues", "tickets", "todo", "ISSUES"] {
        let out = run(dir.path(), &[word], &[]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{word}\n{said}");
        assert!(out.stdout.is_empty(), "{word}\n{said}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), line(word), "{word}");
    }
    let help = run(dir.path(), &["help", "issues"], &[]);
    let said = text(&help);
    assert_eq!(help.status.code(), Some(2), "{said}");
    assert!(help.stdout.is_empty(), "{said}");
    assert_eq!(String::from_utf8_lossy(&help.stderr), line("issues"));
    let memories = run(dir.path(), &["memories", "list", "--help"], &[]);
    let said = text(&memories);
    assert!(memories.status.success(), "{said}");
    assert!(
        !String::from_utf8_lossy(&memories.stderr).contains("no command list"),
        "{said}"
    );
}

#[test]
fn list_and_ls_are_commands_and_a_capital_names_them() {
    let dir = tempfile::tempdir().unwrap();
    for (typed, real) in [("LIST", "list"), ("List", "list"), ("LS", "ls")] {
        let out = run(dir.path(), &[typed], &[]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{typed}\n{said}");
        assert!(out.stdout.is_empty(), "{typed}\n{said}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            format!("ljos: no command {typed}, and the closest is {real}, so run `ljos {real}`.\n"),
            "{typed}"
        );
    }
    for word in ["list", "ls"] {
        let out = run(dir.path(), &["help", word], &[]);
        let said = text(&out);
        assert!(out.status.success(), "{word}\n{said}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("Usage: ljos list"),
            "{word}\n{said}"
        );
    }
}

#[test]
fn a_word_two_verbs_start_with_is_not_guessed() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["mem"], &[]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "ljos: that is not a command, so run `ljos help`.\n"
    );
}

#[test]
fn recall_with_several_words_prints_the_search_that_runs() {
    let dir = tempfile::tempdir().unwrap();
    let cases: [(&[&str], &str); 7] = [
        (
            &["recall", "some", "words"],
            "ljos: recall takes one ticket id, so run `ljos search \"<words>\"` to look up words.\n",
        ),
        (
            &["recall", "some", "words", "--json"],
            "ljos: recall takes one ticket id, so run `ljos search \"<words>\"` to look up words.\n",
        ),
        (
            &["--seat", "ada", "recall", "cold", "metal"],
            "ljos: recall takes one ticket id, so run `ljos search \"<words>\"` to look up words.\n",
        ),
        (
            &["recall", "x", "y z"],
            "ljos: recall takes one ticket id, so run `ljos search \"<words>\"` to look up words.\n",
        ),
        (
            &["recall", "--", "--odd", "words"],
            "ljos: recall takes one ticket id, so run `ljos search \"<words>\"` to look up words.\n",
        ),
        (
            &["recall", "demo-1a2b", "and", "more"],
            "ljos: recall takes one ticket id, so run `ljos recall <id>`.\n",
        ),
        (
            &["recall", "usability"],
            "recall: that text is not a ticket id, so run `ljos search \"<words>\"`.\n",
        ),
    ];
    for (args, want) in cases {
        let out = run(dir.path(), args, &[]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{said}");
        assert!(out.stdout.is_empty(), "{args:?}\n{said}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), want, "{args:?}");
    }
}

#[test]
fn search_takes_unquoted_words_as_one_query_beside_its_flags() {
    let dir = tempfile::tempdir().unwrap();
    let forms: [&[&str]; 4] = [
        &["search", "lexical", "default"],
        &["search", "--limit", "3", "lexical", "default"],
        &["search", "-n", "3", "lexical", "--", "default"],
        &["search", "lexical default"],
    ];
    for args in forms {
        let out = run(dir.path(), args, &[]);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(0), "{args:?}\n{said}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("Nothing matched \"lexical default\""),
            "{args:?}\n{said}"
        );
    }
    for args in [
        &["search", "lexical", "default", "-p", "demo"][..],
        &["search", "-p", "demo", "lexical", "default"][..],
    ] {
        let out = run(dir.path(), args, &[]);
        let said = text(&out);
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("project: demo is not a project"),
            "{args:?}\n{said}"
        );
        assert!(!said.contains("no command"), "{args:?}\n{said}");
    }
}
