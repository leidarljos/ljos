//! `ljos --help` groups commands. Hidden ones still run.

ljos_test_boot::harness!();
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env("LJOS_OFF", "1")
        .output()
        .unwrap()
}

fn text(out: &std::process::Output) -> String {
    format!(
        "status {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn command_names(help: &str) -> Vec<&str> {
    help.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            if rest.starts_with('-') || rest.starts_with(' ') {
                return None;
            }
            rest.split_whitespace().next()
        })
        .collect()
}

#[test]
fn help_groups_the_short_list_and_all_lists_the_rest() {
    let out = run(&["--help"]);
    assert!(out.status.success(), "{}", text(&out));
    let help = String::from_utf8(out.stdout).unwrap();
    let mut at = 0;
    for heading in [
        "Start:",
        "Daily:",
        "Tracker:",
        "Memory:",
        "Coordination:",
        "Admin:",
    ] {
        let found = help
            .find(heading)
            .unwrap_or_else(|| panic!("missing {heading}\n{help}"));
        assert!(found >= at, "{heading} is out of order");
        at = found;
    }
    let names = command_names(&help);
    assert_eq!(names.first().copied(), Some("doctor"), "{names:?}");
    let start = help.find("Start:").expect("Start");
    let daily = help.find("Daily:").expect("Daily");
    let admin = help.find("Admin:").expect("Admin");
    assert_eq!(
        command_names(&help[start..daily]),
        [
            "doctor", "onboard", "protocol", "prefer", "remember", "search", "sitting", "finish",
            "due", "help",
        ]
    );
    assert_eq!(
        command_names(&help[admin..]).first().copied(),
        Some("policy")
    );
    for hidden in ["merge-org", "hook", "bump-plan", "retire-notes", "slot"] {
        assert!(!names.contains(&hidden), "{hidden} is on the short list");
    }
    assert!(help.contains("`ljos help --all` lists every command."));

    let out = run(&["help", "--all"]);
    assert!(out.status.success(), "{}", text(&out));
    let all = String::from_utf8(out.stdout).unwrap();
    let all_names = command_names(&all);
    for hidden in ["merge-org", "hook", "bump-plan", "retire-notes", "slot"] {
        assert!(
            all_names.contains(&hidden),
            "{hidden} missing from help --all"
        );
    }

    for args in [
        &["merge-org", "--help"][..],
        &["hook", "--help"][..],
        &["help", "merge-org"][..],
        &["help", "hook"][..],
    ] {
        let out = run(args);
        assert!(out.status.success(), "{}: {}", args.join(" "), text(&out));
    }
}

#[test]
fn common_commands_include_an_example() {
    for (args, needle) in [
        (&["remember", "--help"][..], "ljos remember"),
        (&["search", "--help"][..], "ljos search"),
        (&["file", "--help"][..], "ljos file"),
        (&["sitting", "--help"][..], "ljos sitting --plan"),
        (&["policy", "--help"][..], "ljos policy --"),
        (&["forget", "--help"][..], "ljos forget"),
        (&["onboard", "--help"][..], "ljos onboard"),
        (&["seat", "--help"][..], "ljos seat add"),
        (&["cards", "--help"][..], "ljos cards"),
        (&["handover", "--help"][..], "ljos handover"),
        (&["receive", "--help"][..], "ljos receive"),
        (&["memory", "integrate", "--help"][..], "does not post"),
    ] {
        let out = run(args);
        let help = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && help.contains(needle),
            "{args:?}\n{help}{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
