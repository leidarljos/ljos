//! Help text a newcomer reads on `--help` and `-h`.

ljos_test_boot::harness!();

use std::path::PathBuf;
use std::process::Command;

fn help(args: &[&str]) -> String {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env("LJOS_OFF", "1")
        .env("LJOS_HOOKD", "off")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "ljos {}\n{text}{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

#[test]
fn policy_example_has_no_trailing_period() {
    for flag in ["--help", "-h"] {
        let page = help(&["policy", flag]);
        assert!(
            page.contains("Example: `ljos policy -- git push --force`\n"),
            "{flag}\n{page}"
        );
        assert!(
            !page.contains("git push --force`."),
            "{flag} example ends with a period\n{page}"
        );
    }
}

#[test]
fn status_and_verdict_help_start_with_a_capital() {
    for (args, line) in [
        (
            &["finish", "--help"][..],
            "Status: done, failed, or cancelled. Only `--close` with done or cancelled records it",
        ),
        (
            &["finish", "-h"][..],
            "Status: done, failed, or cancelled. Only `--close` with done or cancelled records it",
        ),
        (
            &["complete", "--help"][..],
            "Status to record: done, failed, or cancelled. The default is done",
        ),
        (
            &["complete", "-h"][..],
            "Status to record: done, failed, or cancelled. The default is done",
        ),
        (&["rule", "--help"][..], "Label the warning deny or ask."),
        (&["rule", "-h"][..], "Label the warning deny or ask."),
    ] {
        let page = help(args);
        assert!(page.contains(line), "{args:?} missing {line:?}\n{page}");
    }
    for args in [
        &["finish", "--help"][..],
        &["complete", "--help"][..],
        &["rule", "--help"][..],
    ] {
        let page = help(args);
        assert!(
            !page.contains("\n          done, failed")
                && !page.contains("\n          done (default)")
                && !page.contains("\n          deny or ask labels"),
            "{args:?} still opens a flag with a lowercase fragment\n{page}"
        );
    }
}

#[test]
fn json_flags_name_the_rows_they_print() {
    for flag in ["--help", "-h"] {
        let imports = help(&["memory", "imports", flag]);
        assert!(
            imports.contains("Print each import as JSON, the same rows as the text"),
            "memory imports {flag}\n{imports}"
        );
        let rules = help(&["rules", flag]);
        assert!(
            rules.contains("Print pattern, verdict and reason as JSON"),
            "rules {flag}\n{rules}"
        );
    }
}

#[test]
fn ready_and_list_project_flag_names_tickets() {
    let line = "Limit the list to tickets in one project";
    for args in [
        &["ready", "--help"][..],
        &["ready", "-h"][..],
        &["list", "--help"][..],
        &["list", "-h"][..],
        &["ls", "--help"][..],
    ] {
        let page = help(args);
        assert!(page.contains(line), "{args:?}\n{page}");
    }
}

#[test]
fn seat_grant_quote_is_the_owners_yes() {
    let line = "The owner's yes, quoted as they said it";
    for flag in ["--help", "-h"] {
        let page = help(&["seat", "grant", flag]);
        assert!(page.contains(line), "{flag}\n{page}");
    }
}

#[test]
fn cloud_about_names_the_install_and_the_clone() {
    let words = "Install the release binaries and clone the shared tracker";
    for flag in ["--help", "-h"] {
        let page = help(&["cloud", flag]);
        assert!(page.contains(words), "{flag}\n{page}");
        let root = help(&[flag]);
        assert!(root.contains(words), "top-level {flag} cloud line\n{root}");
    }
    let long = help(&["cloud", "--help"]);
    assert!(
        long.contains("Install the release binaries and clone the shared tracker."),
        "{long}"
    );
}

#[test]
fn reap_about_says_apply_is_what_writes() {
    let words = "Without `--apply` it prints what would happen";
    for flag in ["--help", "-h"] {
        let page = help(&["reap", flag]);
        assert!(page.contains(words), "{flag}\n{page}");
        assert!(!page.contains("A dry run without"), "{flag}\n{page}");
    }
    let long = help(&["reap", "--help"]);
    assert!(
        long.contains("Without `--apply` it prints what would happen."),
        "{long}"
    );
    let root = help(&["--help"]);
    assert!(root.contains(words), "{root}");
}

#[test]
fn memories_prune_example_passes_stale() {
    for flag in ["--help", "-h"] {
        let page = help(&["memories", "prune", flag]);
        assert!(page.contains("`--stale` is required"), "{flag}\n{page}");
        assert!(
            page.contains("Example: `ljos memories prune --stale`"),
            "{flag}\n{page}"
        );
        assert!(
            !page.contains("Example: `ljos memories prune`\n")
                && !page.contains("Example: `ljos memories prune`."),
            "{flag} example omits --stale\n{page}"
        );
    }
}

#[test]
fn everyday_commands_show_a_short_example_on_h() {
    let cases = [
        (&["file", "-h"][..], "ljos file \"Pick a codec\" -p demo"),
        (&["sitting", "-h"][..], "ljos sitting proj-1a2b"),
        (
            &["note", "-h"][..],
            "ljos note \"the door seats when it is cold\"",
        ),
        (
            &["remember", "-h"][..],
            "ljos remember \"The lexical default is BM25+.\"",
        ),
        (&["search", "-h"][..], "ljos search \"lexical default\""),
        (
            &["finish", "-h"][..],
            "ljos finish --lesson \"Cold metal seats.\"",
        ),
        (&["seat", "-h"][..], "Example: `ljos seat`"),
        (&["doctor", "-h"][..], "Example: `ljos doctor`"),
    ];
    for (args, needle) in cases {
        let page = help(args);
        assert!(page.contains(needle), "{args:?} missing {needle}\n{page}");
    }
    let sitting = help(&["sitting", "--help"]);
    assert!(
        sitting.contains("ljos sitting proj-1a2b")
            && sitting.contains("ljos sitting --plan \"Ship the codec\""),
        "{sitting}"
    );
    let seat = help(&["seat", "--help"]);
    assert!(
        seat.contains("Example: `ljos seat`") && seat.contains("ljos seat add alice"),
        "{seat}"
    );
}

fn command_names(help_text: &str) -> Vec<String> {
    help_text
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            if rest.starts_with('-') || rest.starts_with(' ') {
                return None;
            }
            rest.split_whitespace().next().map(str::to_string)
        })
        .collect()
}

#[test]
fn top_level_help_matches_the_snapshot_and_names_real_commands() {
    let long = help(&["--help"]);
    let short = help(&["-h"]);
    assert_eq!(long, short, "-h and --help differ");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/help_top.txt");
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        long, expected,
        "top-level help drifted from tests/help_top.txt"
    );
    let names = command_names(&long);
    assert!(names.len() > 40, "{names:?}");
    for name in &names {
        for flag in ["--help", "-h"] {
            let page = help(&[name, flag]);
            assert!(
                page.contains("Usage: ljos"),
                "{name} {flag} is not a command\n{page}"
            );
        }
    }
    for hidden in [
        "merge-org",
        "hook",
        "bump-plan",
        "retire-notes",
        "slot",
        "start",
    ] {
        assert!(
            !names.iter().any(|name| name == hidden),
            "{hidden} is on the top list"
        );
    }
}
