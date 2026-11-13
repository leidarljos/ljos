//! `ljos completions` and `ljos` with no arguments.

ljos_test_boot::harness!();

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_RUNTIME_DIR", root)
        .env("VISSUE_ROOT", root.join("tracker"))
        .env("LJOS_OFF", "1")
        .env("LJOS_HOOKD", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")))
}

fn text(out: &Output) -> String {
    format!(
        "status {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn has_name(body: &str, name: &str) -> bool {
    body.match_indices(name).any(|(at, _)| {
        let before = body[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        let after = body[at + name.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        before && after
    })
}

fn root_names(help: &str) -> Vec<String> {
    help.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            if rest.starts_with('-') || rest.starts_with(' ') {
                return None;
            }
            rest.split_whitespace().next().map(str::to_string)
        })
        .collect()
}

fn subcommands(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if line.is_empty() || (!line.starts_with(' ') && line.ends_with(':')) {
            break;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        if rest.starts_with(' ') || rest.starts_with('-') {
            continue;
        }
        let Some(name) = rest.split_whitespace().next() else {
            continue;
        };
        if name == "help" {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

fn command_names(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let listed = run(root, &["help", "--all"]);
    assert!(listed.status.success(), "{}", text(&listed));
    let mut pending: Vec<Vec<String>> = root_names(&String::from_utf8_lossy(&listed.stdout))
        .into_iter()
        .map(|name| vec![name])
        .collect();
    while let Some(prefix) = pending.pop() {
        let name = prefix.last().expect("path").clone();
        names.insert(name);
        let mut args: Vec<&str> = prefix.iter().map(String::as_str).collect();
        args.push("--help");
        let help = run(root, &args);
        assert!(help.status.success(), "{}", text(&help));
        let body = String::from_utf8_lossy(&help.stdout);
        assert!(
            body.contains("Example:"),
            "`ljos {} --help` has no example line\n{body}",
            prefix.join(" ")
        );
        for child in subcommands(&body) {
            let mut next = prefix.clone();
            next.push(child);
            pending.push(next);
        }
    }
    names
}

#[test]
fn bare_ljos_prints_the_first_commands() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &[]);
    let body = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", text(&out));
    for line in [
        "Memory, a work tracker and a command guard for coding runners.",
        "no seat rule matched; reading every seat, so run `export LJOS_SEAT=NAME`.",
        "ljos search QUERY",
        "ljos remember TEXT",
        "ljos file TITLE",
        "ljos doctor",
        "The docs are at https://leidarljos.github.io/ljos/reference-commands.html.",
    ] {
        assert!(body.contains(line), "missing {line}\n{body}");
    }
    assert!(
        !body.contains("ljos recall"),
        "bare ljos listed recall\n{body}"
    );
    assert!(
        !body.contains("Usage:") && !body.contains("Start:"),
        "bare ljos printed the command list\n{body}"
    );
}

#[test]
fn each_shell_script_names_every_subcommand() {
    let dir = tempfile::tempdir().unwrap();
    let names = command_names(dir.path());
    assert!(names.contains("completions"), "{names:?}");
    assert!(names.contains("remember"), "{names:?}");
    assert!(names.len() > 40, "{names:?}");
    for shell in ["bash", "zsh", "fish"] {
        let out = run(dir.path(), &["completions", shell]);
        assert!(out.status.success(), "{shell}: {}", text(&out));
        let body = String::from_utf8_lossy(&out.stdout);
        assert!(
            body.contains("completions --candidates"),
            "{shell} does not ask for seat or ticket names\n{body}"
        );
        for name in &names {
            assert!(
                has_name(&body, name),
                "{shell} completion script has no subcommand {name}"
            );
        }
    }
}

#[test]
fn seat_and_ticket_completion_reads_a_small_tracker() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = dir.path().join("tracker");
    let project = tracker.join("Software/demo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "[seats.alice]\n[seats.bob]\n").unwrap();
    std::fs::write(
        project.join("issues.org"),
        "\
* TODO [#C] Pick a codec
:PROPERTIES:
:ID:         demo-2f5j
:END:
* DONE [#B] Old
:PROPERTIES:
:ID:         demo-old1
:END:
* STARTED Live
:PROPERTIES:
:ID:         demo-live
:END:
",
    )
    .unwrap();

    let seats = run(dir.path(), &["completions", "--candidates", "seats"]);
    assert!(seats.status.success(), "{}", text(&seats));
    assert_eq!(
        String::from_utf8_lossy(&seats.stdout),
        "alice\nbob\n",
        "{}",
        text(&seats)
    );
    let tickets = run(dir.path(), &["completions", "--candidates", "tickets"]);
    assert!(tickets.status.success(), "{}", text(&tickets));
    assert_eq!(
        String::from_utf8_lossy(&tickets.stdout),
        "demo-2f5j\ndemo-live\n",
        "{}",
        text(&tickets)
    );
}

#[test]
fn a_large_tracker_completes_no_seats_or_tickets() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = dir.path().join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    let mut roster = String::from("[seats.alice]\n");
    roster.push_str(&"# .\n".repeat(100_000));
    std::fs::write(tracker.join("vissue.toml"), roster).unwrap();
    for n in 0..50 {
        let project = tracker.join(format!("Software/p{n:02}"));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("issues.org"),
            format!("* TODO [#C] Item\n:PROPERTIES:\n:ID:         p{n:02}-aa\n:END:\n"),
        )
        .unwrap();
    }
    let seats = run(dir.path(), &["completions", "--candidates", "seats"]);
    assert!(seats.status.success(), "{}", text(&seats));
    assert!(
        seats.stdout.is_empty(),
        "a large roster was completed:\n{}",
        text(&seats)
    );
    let tickets = run(dir.path(), &["completions", "--candidates", "tickets"]);
    assert!(tickets.status.success(), "{}", text(&tickets));
    assert!(
        tickets.stdout.is_empty(),
        "a wide tracker was completed:\n{}",
        text(&tickets)
    );
}
