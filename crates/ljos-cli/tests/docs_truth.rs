//! Statements in the docs and the README that a built binary can check.
//!
//! Each test reads a page and then runs the program the page describes.
//! A page that names a count, a row, an event or a help line the program
//! does not print fails here.

ljos_test_boot::harness!();

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn page(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|err| panic!("{rel}: {err}"))
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[cfg(unix)]
fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Home {
    root: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for sub in [
            ".config",
            ".local/share",
            ".local/state",
            ".cache",
            "run",
            "tmp",
            "bin",
        ] {
            std::fs::create_dir_all(root.path().join(sub)).unwrap();
        }
        let bin = root.path().join("bin");
        script(
            &bin,
            "packset",
            "#!/bin/sh\n[ \"$1\" = ensure ] && echo writer up && exit 0\nexit 1\n",
        );
        script(
            &bin,
            "deedar",
            "#!/bin/sh\n[ \"$1 $2\" = \"host accept\" ] && echo 'host key listed' && exit 0\nexit 1\n",
        );
        script(&bin, "curl", "#!/bin/sh\nexit 7\n");
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn command(&self, program: &str) -> Command {
        let home = self.path();
        let bins = Path::new(program).parent().unwrap();
        let path = std::env::join_paths([
            home.join("bin"),
            bins.to_path_buf(),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ])
        .unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .env("HOME", home)
            .env("PATH", path)
            .env("PACKSET_PORT", port.to_string())
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("LJOS_TRACKER_GIT", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn ljos(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .output()
            .unwrap()
    }
}

fn measured(key: &str) -> String {
    let text = page("measurements/mcp-size.txt");
    let prefix = format!("{key}: ");
    text.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("measurements/mcp-size.txt has no {key}"))
        .to_string()
}

/// The count the introduction and the comparison quote is the one the
/// measurement records, not the count of an older release.
#[test]
fn the_core_tool_count_in_the_docs_is_the_measured_count() {
    let core = measured("core.tools");
    for (rel, line) in [
        (
            "docs/orgmode/introduction.org",
            format!("The server lists {core} core tools by default"),
        ),
        (
            "docs/source/introduction.rst",
            format!("The server lists {core} core tools by default"),
        ),
        (
            "docs/orgmode/compare.org",
            format!("yes, {core} core tools"),
        ),
        ("docs/source/compare.rst", format!("yes, {core} core tools")),
    ] {
        assert!(
            page(rel).contains(&line),
            "{rel} does not say {line:?}; the measurement says {core}"
        );
    }
    for rel in ["docs/orgmode/introduction.org", "docs/orgmode/compare.org"] {
        let text = page(rel);
        let stale: Vec<_> = text
            .lines()
            .filter(|line| line.contains("20 core tools"))
            .collect();
        assert!(
            stale.is_empty(),
            "{rel} still says 20 core tools: {stale:?}"
        );
    }
}

/// A runner's seat on the CLI sees the bare claim line on stderr. The
/// `label claim-conflict:` form is what `ljos mcp` returns.
#[test]
fn the_docs_do_not_give_the_cli_claim_refusal_a_label() {
    for rel in [
        "README.md",
        "docs/orgmode/getting-started.org",
        "docs/source/getting-started.rst",
    ] {
        let text = page(rel);
        assert!(
            !text.contains("label claim-conflict: claim:"),
            "{rel} shows the labelled line as CLI output"
        );
        assert!(
            !text.contains("prints `label claim-conflict:`"),
            "{rel} says the CLI claim prints the label"
        );
    }
    let start = page("docs/orgmode/getting-started.org");
    assert!(
        start.contains("the exit status is 1"),
        "getting-started does not name the exit status of the refused claim"
    );
}

fn registered_events(output: &str) -> Vec<String> {
    let row = output
        .lines()
        .find(|line| line.contains("memory hook: added on "))
        .unwrap_or_else(|| panic!("no hook row in\n{output}"));
    let (_, rest) = row.split_once("added on ").unwrap();
    let (events, _) = rest.split_once(" in ").unwrap();
    events.split(", ").map(str::to_string).collect()
}

fn number_word(n: usize) -> &'static str {
    match n {
        6 => "six",
        7 => "seven",
        8 => "eight",
        9 => "nine",
        other => panic!("no word for {other}"),
    }
}

/// The architecture page counts and names the events each runner gets.
#[test]
fn the_architecture_page_names_the_events_onboard_registers() {
    let page = page("docs/orgmode/architecture.org").replace('\n', " ");
    for (harness, from, to) in [
        (
            "claude",
            "The terminal harness registers",
            "The command harness registers",
        ),
        (
            "codex",
            "The command harness registers",
            "The build harness",
        ),
    ] {
        let home = Home::new();
        let out = home.ljos(&["onboard", "--harness", harness]);
        let text = said(&out);
        assert!(out.status.success(), "{text}");
        let events = registered_events(&text);
        let (_, after) = page
            .split_once(from)
            .unwrap_or_else(|| panic!("no {from:?}"));
        let (sentence, _) = after.split_once(to).unwrap_or_else(|| panic!("no {to:?}"));
        let word = number_word(events.len());
        assert!(
            sentence.trim_start().starts_with(word),
            "onboard {harness} registers {} events, the page says {sentence:?}",
            events.len()
        );
        for event in &events {
            assert!(
                sentence.contains(&format!("={event}=")),
                "the page leaves out {event} for {harness}: {sentence:?}"
            );
        }
    }
}

/// The how-to filters doctor's output on a row name. The name has to be
/// the one doctor prints.
#[test]
fn the_signing_key_how_to_greps_a_row_doctor_prints() {
    let org = page("docs/orgmode/howto-handover.org");
    let rst = page("docs/source/howto-handover.rst");
    for (rel, text) in [("org", &org), ("rst", &rst)] {
        assert!(
            text.contains("ljos doctor --all | grep 'signing key'"),
            "{rel} does not grep for the signing key row"
        );
        assert!(
            !text.contains("grep 'host key'"),
            "{rel} greps a row name doctor does not print"
        );
    }
    let home = Home::new();
    let onboard = home.ljos(&["onboard"]);
    assert!(onboard.status.success(), "{}", said(&onboard));
    let out = home.ljos(&["doctor", "--all"]);
    let text = said(&out);
    assert!(
        text.lines().any(|line| line.contains("\tsigning key\t")),
        "doctor prints no signing key row:\n{text}"
    );
}

/// A page names a checkout of the person who wrote it, nothing a reader
/// could not have.
#[test]
fn no_page_names_a_build_path() {
    let mut found = Vec::new();
    for dir in ["docs/orgmode", "docs/source"] {
        for entry in std::fs::read_dir(root().join(dir)).unwrap().flatten() {
            let path = entry.path();
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if ext != "org" && ext != "rst" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for line in text.lines() {
                for needle in ["/agent/repos", "/home/ubuntu", "/target/debug/"] {
                    if line.contains(needle) {
                        found.push(format!("{}: {needle}", path.display()));
                    }
                }
            }
        }
    }
    let readme = page("README.md");
    for needle in ["/agent/repos", "/home/ubuntu", "/target/debug/"] {
        if readme.contains(needle) {
            found.push(format!("README.md: {needle}"));
        }
    }
    assert!(found.is_empty(), "build paths in the docs: {found:#?}");
}

/// With no seat named, a write uses the login name or a short stable name.
/// Help does not say `unknown-` and a host name.
#[test]
fn help_names_the_seat_a_write_uses() {
    let home = Home::new();
    for verb in ["seat", "remember"] {
        let out = home.ljos(&[verb, "--help"]);
        let text = said(&out);
        assert!(out.status.success(), "{text}");
        assert!(
            !text.contains("unknown-"),
            "ljos {verb} --help names the old fallback:\n{text}"
        );
        assert!(
            text.contains("login name"),
            "ljos {verb} --help does not name the login name:\n{text}"
        );
    }
}

fn session(env: &[(&str, &str)], calls: &[(&str, serde_json::Value)]) -> Vec<serde_json::Value> {
    let home = Home::new();
    let onboard = home.ljos(&["onboard"]);
    assert!(onboard.status.success(), "{}", said(&onboard));
    let mut cmd = home.command(env!("CARGO_BIN_EXE_ljos-mcp"));
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.stdin(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"docs","version":"0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_string(),
    ];
    for (i, (name, args)) in calls.iter().enumerate() {
        lines.push(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 10 + i,
                "method": "tools/call",
                "params": {"name": name, "arguments": args},
            })
            .to_string(),
        );
    }
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn answer(replies: &[serde_json::Value], id: u64) -> &serde_json::Value {
    replies
        .iter()
        .find(|reply| reply["id"] == id)
        .unwrap_or_else(|| panic!("no answer to {id} in {replies:?}"))
}

fn tool_names(env: &[(&str, &str)]) -> Vec<String> {
    let replies = session(env, &[]);
    answer(&replies, 2)["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_eb_stack_pages_name_cli_commands_not_unserved_mcp_tools() {
    for rel in [
        "docs/orgmode/howto-eb-stack.org",
        "docs/source/howto-eb-stack.rst",
    ] {
        let text = page(rel);
        for name in [
            "eb_recipe_check",
            "eb_package_bump",
            "eb_recipe_lint",
            "eb_target_doctor",
            "eb_campaign_run",
            "eb_campaign_status",
        ] {
            assert!(!text.contains(name), "{rel} names {name}");
        }
        for command in [
            "eb-stack recipe check",
            "eb-stack package bump",
            "eb-stack recipe lint",
            "eb-stack target doctor",
            "eb-stack campaign",
        ] {
            assert!(text.contains(command), "{rel} omits {command}");
        }
    }
}

fn listed_between(text: &str, from: &str, to: &str) -> BTreeSet<String> {
    let (_, after) = text
        .split_once(from)
        .unwrap_or_else(|| panic!("no {from:?}"));
    let (span, _) = after.split_once(to).unwrap_or_else(|| panic!("no {to:?}"));
    span.split('=')
        .filter(|piece| {
            piece.starts_with("ljos_") && piece.chars().all(|c| c.is_alphanumeric() || c == '_')
        })
        .map(str::to_string)
        .collect()
}

/// The MCP page names every tool the server lists under all, once, as a
/// writer or as a reader. The server refuses a write that names no seat
/// and answers a reader without one.
#[test]
fn the_mcp_page_sorts_every_tool_into_writers_or_readers() {
    let text = page("docs/orgmode/reference-mcp.org");
    let writers = listed_between(&text, "Writers:", "Readers:");
    let readers = listed_between(&text, "Readers:", "Resources:");
    assert!(
        writers.is_disjoint(&readers),
        "a tool is both a writer and a reader: {:?}",
        writers.intersection(&readers).collect::<Vec<_>>()
    );
    let all: BTreeSet<String> = tool_names(&[("LJOS_MCP_TOOLS", "all")])
        .into_iter()
        .collect();
    let named: BTreeSet<String> = writers.union(&readers).cloned().collect();
    assert_eq!(all, named, "the page and the server list different tools");
    assert_eq!(
        all.len().to_string(),
        measured("all.tools"),
        "the server lists a different count than the measurement"
    );

    // A writer is refused with no seat before its arguments are read. A
    // reader is not. These seven writers and ljos_habit read until a call
    // asks them to write, so they are left out of the first check.
    let conditional: BTreeSet<&str> = [
        "ljos_deed",
        "ljos_table",
        "ljos_vote",
        "ljos_bump_plan",
        "ljos_findings",
        "ljos_group",
        "ljos_island",
        "ljos_habit",
    ]
    .into_iter()
    .collect();
    let probes: Vec<_> = all
        .iter()
        .map(|name| (name.as_str(), serde_json::json!({})))
        .collect();
    let replies = session(&[("LJOS_MCP_TOOLS", "all"), ("LJOS_SEAT", "")], &probes);
    let mut wrong = Vec::new();
    for (i, name) in all.iter().enumerate() {
        if conditional.contains(name.as_str()) {
            continue;
        }
        let reply = answer(&replies, 10 + i as u64);
        let refused = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("pass `seat` in arguments");
        if refused != writers.contains(name) {
            wrong.push(format!(
                "{name}: the page says {}, the server {} it for a seat",
                if writers.contains(name) {
                    "writer"
                } else {
                    "reader"
                },
                if refused { "refuses" } else { "answers" }
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
