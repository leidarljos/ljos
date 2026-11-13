//! `ljos <command> --help` against the command reference.
//!
//! The flag list under `ljos-help-flags` in the command reference is the
//! long flags and aliases help prints. A flag help names that the command
//! does not have fails here, and so does a flag the synopsis names that
//! help does not print.

ljos_test_boot::harness!();

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

fn ljos(args: &[&str]) -> String {
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

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Long flags and visible aliases on an option line. `--help` and
/// `--version` are clap's, not the command's.
fn flags_of(help: &str) -> BTreeSet<String> {
    let mut flags = BTreeSet::new();
    let mut options = false;
    for line in help.lines() {
        if line == "Options:" {
            options = true;
            continue;
        }
        if !options {
            continue;
        }
        if !line.is_empty() && !line.starts_with(' ') {
            break;
        }
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with("[alias") {
            flags.extend(long_names(trimmed));
            continue;
        }
        if indent > 8 || !is_option_line(trimmed) {
            continue;
        }
        for name in long_names(trimmed) {
            if name != "help" && name != "version" {
                flags.insert(name);
            }
        }
    }
    flags
}

fn is_option_line(trimmed: &str) -> bool {
    let rest = match trimmed.strip_prefix('-') {
        Some(rest) => rest,
        None => return false,
    };
    if let Some(rest) = rest.strip_prefix(|c: char| c.is_ascii_alphanumeric()) {
        if let Some(rest) = rest.strip_prefix(", ") {
            return rest.starts_with("--");
        }
    }
    trimmed.starts_with("--")
}

fn long_names(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find("--") {
        let after = &rest[at + 2..];
        let end = after
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
            .unwrap_or(after.len());
        let name = &after[..end];
        if !name.is_empty()
            && !name.ends_with('-')
            && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && !names.iter().any(|have| have == name)
        {
            names.push(name.to_string());
        }
        rest = &after[end..];
    }
    names
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

fn walk(prefix: &[String], tree: &mut BTreeMap<String, (String, BTreeSet<String>)>) {
    let mut args: Vec<&str> = prefix.iter().map(String::as_str).collect();
    args.push("--help");
    let help = ljos(&args);
    let path = prefix.join(" ");
    let flags = flags_of(&help);
    tree.insert(path, (help, flags));
    for name in subcommands(&tree[prefix.join(" ").as_str()].0) {
        let mut next = prefix.to_vec();
        next.push(name);
        walk(&next, tree);
    }
}

fn index_lines(tree: &BTreeMap<String, (String, BTreeSet<String>)>) -> String {
    let mut out = String::new();
    for (path, (_, flags)) in tree {
        out.push_str(path);
        for flag in flags {
            out.push(' ');
            out.push_str("--");
            out.push_str(flag);
        }
        out.push('\n');
    }
    out
}

fn marked(text: &str, begin: &str, end: &str) -> String {
    let start = text
        .find(begin)
        .unwrap_or_else(|| panic!("missing {begin}"))
        + begin.len();
    let stop = text[start..]
        .find(end)
        .unwrap_or_else(|| panic!("missing {end}"))
        + start;
    text[start..stop]
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#') && !line.starts_with(".. ")
        })
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_marked(path: &std::path::Path, begin: &str, end: &str, body: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    let start = text.find(begin).unwrap() + begin.len();
    let stop = text[start..].find(end).unwrap() + start;
    let mut next = String::new();
    next.push_str(&text[..start]);
    next.push('\n');
    next.push_str(body);
    if !body.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&text[stop..]);
    std::fs::write(path, next).unwrap();
}

fn mentioned_flags(
    help: &str,
    own: &BTreeSet<String>,
    tree: &BTreeMap<String, (String, BTreeSet<String>)>,
) -> BTreeSet<String> {
    let mut allow = own.clone();
    for (path, (_, flags)) in tree {
        let name = path.split(' ').next_back().unwrap_or(path);
        if help.contains(&format!("ljos {path}")) || help.contains(&format!("ljos {name}")) {
            allow.extend(flags.iter().cloned());
        }
    }
    allow
}

/// Flags named in the arguments section are accepted and passed on.
fn passed_through(help: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut args = false;
    for line in help.lines() {
        if line == "Arguments:" {
            args = true;
            continue;
        }
        if args && (line == "Options:" || (!line.is_empty() && !line.starts_with(' '))) {
            break;
        }
        if args {
            names.extend(long_names(line));
        }
    }
    names
}

fn stray_flags(help: &str, allowed: &BTreeSet<String>) -> Vec<String> {
    let mut stray = Vec::new();
    for line in help.lines() {
        // Flags after ` -- ` belong to the command being shown, not to ljos.
        let head = line.split(" -- ").next().unwrap_or(line);
        for name in long_names(head) {
            if name == "help" || name == "version" {
                continue;
            }
            if !allowed.contains(&name) && !stray.contains(&name) {
                stray.push(name);
            }
        }
    }
    stray
}

fn command_paths_in(synopsis: &str, known: &BTreeSet<String>) -> Vec<(String, BTreeSet<String>)> {
    let synopsis = synopsis.replace("\\vert", "|");
    let mut found = Vec::new();
    for segment in synopsis.split(" / ") {
        let mut path = Vec::new();
        let head = segment.split('[').next().unwrap_or(segment);
        for token in head.split_whitespace() {
            if token.starts_with('-') || token.chars().any(|c| c.is_ascii_uppercase()) {
                break;
            }
            if token.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                path.push(token.to_string());
                let joined = path.join(" ");
                if !known.contains(&joined) {
                    path.pop();
                    break;
                }
            } else {
                break;
            }
        }
        if path.is_empty() {
            continue;
        }
        let mut flags = BTreeSet::new();
        for name in long_names(segment) {
            flags.insert(name);
        }
        found.push((path.join(" "), flags));
    }
    found
}

/// A synopsis flag may belong to the command or to one of its subcommands.
fn flag_is_on(tree: &BTreeMap<String, (String, BTreeSet<String>)>, path: &str, flag: &str) -> bool {
    if tree
        .get(path)
        .is_some_and(|(_, flags)| flags.contains(flag))
    {
        return true;
    }
    let prefix = format!("{path} ");
    tree.iter()
        .any(|(name, (_, flags))| name.starts_with(&prefix) && flags.contains(flag))
}

fn word_is(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .any(|token| token == word)
}

fn prose_names(prose: &str, path: &str) -> bool {
    let words: Vec<&str> = path.split(' ').collect();
    prose
        .lines()
        .any(|line| words.iter().all(|word| word_is(line, word)))
}

fn synopses(org: &str) -> Vec<String> {
    org.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("- =")?;
            Some(rest.split("= ::").next()?.to_string())
        })
        .collect()
}

#[test]
fn help_and_the_reference_list_the_same_flags() {
    let mut tree = BTreeMap::new();
    let root = ljos(&["help", "--all"]);
    for name in root_names(&root) {
        walk(&[name], &mut tree);
    }
    let rendered = index_lines(&tree).trim_end().to_string();
    let root_dir = repo_root();
    let org_path = root_dir.join("docs/orgmode/reference-commands.org");
    let rst_path = root_dir.join("docs/source/reference-commands.rst");
    if std::env::var_os("LJOS_WRITE_HELP_FLAGS").is_some() {
        write_marked(
            &org_path,
            "# BEGIN ljos-help-flags",
            "# END ljos-help-flags",
            &rendered,
        );
        write_marked(
            &rst_path,
            ".. BEGIN ljos-help-flags",
            ".. END ljos-help-flags",
            &rendered,
        );
        return;
    }
    let org = std::fs::read_to_string(&org_path).unwrap();
    let rst = std::fs::read_to_string(&rst_path).unwrap();
    let org_index = marked(&org, "# BEGIN ljos-help-flags", "# END ljos-help-flags");
    let rst_index = marked(&rst, ".. BEGIN ljos-help-flags", ".. END ljos-help-flags");
    assert_eq!(
        org_index, rendered,
        "docs/orgmode/reference-commands.org drifted from ljos --help"
    );
    assert_eq!(
        rst_index, rendered,
        "docs/source/reference-commands.rst drifted from ljos --help"
    );

    let known: BTreeSet<String> = tree.keys().cloned().collect();
    let prose = org.split("# BEGIN ljos-help-flags").next().unwrap_or(&org);
    for path in &known {
        assert!(
            prose_names(prose, path),
            "the command reference never names `{path}`"
        );
    }

    for synopsis in synopses(&org) {
        for (path, flags) in command_paths_in(&synopsis, &known) {
            for flag in &flags {
                assert!(
                    flag_is_on(&tree, &path, flag),
                    "`{path}` documents --{flag}, which `ljos {path} --help` does not have\n{synopsis}"
                );
            }
        }
    }

    let all_flags: BTreeSet<String> = tree
        .values()
        .flat_map(|(_, flags)| flags.iter().cloned())
        .collect();
    let mut invented = Vec::new();
    for name in long_names(prose) {
        // `--help` is clap's. `--by` is a flag `ljos claims` passes to `vissue claims`.
        if name == "help" || name == "version" || name == "by" {
            continue;
        }
        if !all_flags.contains(&name) && !invented.contains(&name) {
            invented.push(name);
        }
    }
    assert!(
        invented.is_empty(),
        "the command reference names flags help does not have: {invented:?}"
    );

    for (path, (help, flags)) in &tree {
        let mut allowed = mentioned_flags(help, flags, &tree);
        allowed.extend(passed_through(help));
        for name in subcommands(help) {
            let child = if path.is_empty() {
                name
            } else {
                format!("{path} {name}")
            };
            if let Some((_, child_flags)) = tree.get(&child) {
                allowed.extend(child_flags.iter().cloned());
            }
        }
        let stray = stray_flags(help, &allowed);
        assert!(
            stray.is_empty(),
            "`ljos {path} --help` names flags the command does not have: {stray:?}\n{help}"
        );
    }

    let exits = [
        ("doctor", "Exit 1"),
        ("doctor", "has not been created yet"),
        ("inbox", "exits 75"),
        ("search", "exits 75"),
        ("policy", "cloud seat push rule"),
        ("hook", "cloud seat push rule"),
        ("hook", "exits 0"),
        ("consensus", "exits 1"),
        ("consensus", "exits 3"),
        ("hud", "exits 127"),
        ("mesh up", "non-zero"),
        ("record check", "Exits 1"),
    ];
    for (path, needle) in exits {
        let help = &tree[path].0;
        assert!(
            help.contains(needle),
            "`ljos {path} --help` does not say {needle:?}"
        );
        assert!(
            org.contains(needle)
                || org
                    .to_ascii_lowercase()
                    .contains(&needle.to_ascii_lowercase()),
            "the command reference does not say {needle:?} for {path}"
        );
    }
}
