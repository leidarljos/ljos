//! How `ljos --help` groups commands.
//!
//! `ljos --help` and `ljos help` show only the daily set in under 24 lines.
//! Every other command still runs, and `ljos help --all` lists every
//! command across six groups.

#[cfg(test)]
use std::collections::BTreeSet;

use clap::Command;

/// Two spaces, the indent clap uses for a command in `--help`.
const INDENT: &str = "  ";

/// Spaces clap leaves between the longest name and its description.
const GAP: usize = 2;

/// The daily set shown by `ljos --help` and `ljos help`.
const DAILY: &[&str] = &[
    "search", "remember", "file", "sitting", "note", "finish", "ready", "show", "doctor",
];

/// `(heading, command names in the order they are printed)` for `ljos help --all`.
///
/// Hidden names sit at the end of their group, so `--all` keeps the same headings.
const GROUPS: &[(&str, &[&str])] = &[
    (
        "Start",
        &[
            "doctor", "onboard", "protocol", "prefer", "remember", "search", "sitting", "finish",
            "due", "help",
        ],
    ),
    (
        "Daily",
        &[
            "note", "file", "show", "ready", "claims", "inbox", "digest", "rule", "rules", "sync",
            "hud", "seat", "cards",
        ],
    ),
    (
        "Tracker",
        &[
            "claim",
            "release",
            "complete",
            "reap",
            "options",
            "block",
            "deed",
            "evidence",
            "record",
            "current",
            "recall",
            "timeline",
            "log",
            "usage",
            "reopen",
            "withdraw",
            "merge-atoms",
            "merge-org",
            "bump-plan",
            "findings",
        ],
    ),
    (
        "Memory",
        &[
            "memory",
            "memories",
            "export",
            "accept",
            "forget",
            "proposals",
            "conflicts",
            "consolidate",
            "hubs",
            "island",
            "handover",
            "receive",
            "graded",
            "habit",
            "retire-notes",
            "judge-score",
        ],
    ),
    (
        "Coordination",
        &[
            "send",
            "read",
            "reply",
            "group",
            "wake",
            "vote",
            "consensus",
            "panel",
            "persona",
            "personas",
            "agents",
            "ask",
            "brief",
            "playbook",
            "playbooks",
            "workflow",
            "predict",
            "trust",
            "calibrate",
            "learn",
            "open-panel",
        ],
    ),
    (
        "Admin",
        &[
            "policy",
            "offboard",
            "uninstall",
            "upgrade",
            "git-hooks",
            "person-key",
            "cloud",
            "mesh",
            "mcp",
            "completions",
            "hook",
            "git-hook",
            "statusline",
            "supervise",
            "slot",
        ],
    ),
];

/// The names in [`GROUPS`], in order, one heading at a time.
#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    GROUPS.iter().flat_map(|(_, names)| names.iter().copied())
}

/// `ljos --help` and `ljos help`, in clap's plain layout.
///
/// With `all` false, only [`DAILY`] is shown under `Daily:`.
/// With `all` true, every command is listed under its group heading.
pub fn render(cmd: &Command, all: bool) -> String {
    let shown: Vec<(&str, &Command)> = if all {
        GROUPS
            .iter()
            .flat_map(|(heading, names)| {
                names
                    .iter()
                    .filter_map(move |name| Some((*heading, cmd.find_subcommand(name)?)))
            })
            .collect()
    } else {
        DAILY
            .iter()
            .filter_map(|name| Some(("Daily", cmd.find_subcommand(name)?)))
            .collect()
    };
    let longest = shown
        .iter()
        .map(|(_, sub)| sub.get_name().len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    if let Some(about) = cmd.get_about() {
        out.push_str(&about.to_string());
        out.push_str("\n\n");
    }
    out.push_str("Usage: ");
    out.push_str(cmd.get_name());
    out.push_str(" <COMMAND>\n\n");
    let mut last = "";
    for (heading, sub) in &shown {
        if *heading != last {
            if !last.is_empty() {
                out.push('\n');
            }
            out.push_str(heading);
            out.push_str(":\n");
            last = heading;
        }
        let name = sub.get_name();
        let about = sub
            .get_about()
            .map(|text| text.to_string().replace('\n', " "))
            .unwrap_or_default();
        let pad = longest + GAP - name.len();
        out.push_str(INDENT);
        out.push_str(name);
        out.push_str(&" ".repeat(pad));
        out.push_str(&about);
        out.push('\n');
    }
    // The same two lines clap prints for this command's flags.
    out.push_str("\nOptions:\n");
    out.push_str("  -h, --help     Print help\n");
    out.push_str("  -V, --version  Print version\n");
    if let Some(after) = cmd.get_after_help() {
        out.push('\n');
        out.push_str(&after.to_string());
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Every subcommand of `cmd` is in one group, and every group name is a
/// subcommand. A hidden command is one clap marks hidden.
#[cfg(test)]
pub fn groups_match(cmd: &Command) -> Result<(), String> {
    let mut have: BTreeSet<&str> = cmd.get_subcommands().map(Command::get_name).collect();
    let mut seen = BTreeSet::new();
    for name in names() {
        if !seen.insert(name) {
            return Err(format!("{name} is in two groups"));
        }
        if !have.remove(name) {
            return Err(format!("{name} is not a command"));
        }
    }
    if !have.is_empty() {
        return Err(format!(
            "not grouped: {}",
            have.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    for name in names() {
        let sub = cmd.find_subcommand(name).expect("just checked");
        let hidden = matches!(
            name,
            "merge-atoms"
                | "merge-org"
                | "bump-plan"
                | "findings"
                | "retire-notes"
                | "judge-score"
                | "open-panel"
                | "hook"
                | "git-hook"
                | "statusline"
                | "supervise"
                | "slot"
        );
        if sub.is_hide_set() != hidden {
            return Err(format!(
                "{name}: hide is {}, the group list says {hidden}",
                sub.is_hide_set()
            ));
        }
    }
    Ok(())
}
