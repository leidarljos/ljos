//! Wrapping follows the kind of output. Machine flags, hooks, hook servers
//! and tool servers never wrap stdout or stderr. Help is prose and wraps
//! on a terminal. Each check runs the binary on a pseudo-terminal at
//! `COLUMNS=20`.

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

const COLS: usize = 20;

fn hermetic(bin: &str, home: &Path) -> Command {
    let mut cmd = Command::new(bin);
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", home)
        .env("PACKSET_URL", "off")
        .env("LJOS_SEAT", "alice")
        .current_dir(home)
        .stdin(Stdio::null());
    cmd
}

fn ljos(home: &Path, args: &[&str]) -> Command {
    let mut cmd = hermetic(env!("CARGO_BIN_EXE_ljos"), home);
    cmd.args(args);
    cmd
}

/// What `cmd` shows with stdout and stderr on a pty `COLUMNS=20` wide.
fn on_tty(cmd: Command) -> (Option<i32>, String) {
    on_tty_with_input(cmd, None)
}

fn on_tty_with_input(mut cmd: Command, input: Option<&str>) -> (Option<i32>, String) {
    cmd.env("COLUMNS", COLS.to_string());
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let term = pty::Pty::open();
    term.attach_output(&mut cmd);
    let mut child = term.spawn(&mut cmd);
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let out = term.wait_output(child, 60);
    term.finish_reading();
    (out.status.code(), term.shown().replace("\r\n", "\n"))
}

/// A line wider than the terminal is allowed only when it has no place to
/// break: one word, or one code span, which the wrapper keeps whole.
fn fits(line: &str) -> bool {
    if line.chars().count() <= COLS {
        return true;
    }
    let mut outside = String::new();
    let mut open: Option<usize> = None;
    let mut rest = line.trim();
    while let Some(ch) = rest.chars().next() {
        if ch == '`' {
            let run = rest.len() - rest.trim_start_matches('`').len();
            open = match open {
                None => Some(run),
                Some(n) if n == run => None,
                kept => kept,
            };
            rest = &rest[run..];
            continue;
        }
        if open.is_none() {
            outside.push(ch);
        }
        rest = &rest[ch.len_utf8()..];
    }
    !outside
        .trim_matches(|ch: char| ch == ':' || ch == '.')
        .contains(' ')
}

fn assert_human_fits(what: &str, shown: &str) {
    for line in shown.lines() {
        assert!(fits(line), "{what}: {line:?} is wider than {COLS}\n{shown}");
    }
}

fn assert_all_json(what: &str, shown: &str) {
    let lines: Vec<&str> = shown.lines().filter(|line| !line.is_empty()).collect();
    assert!(!lines.is_empty(), "{what}: nothing shown");
    for line in lines {
        assert!(
            serde_json::from_str::<serde_json::Value>(line).is_ok(),
            "{what}: {line:?} is not JSON\n{shown}"
        );
    }
}

/// Every top-level command's name, read from `ljos help --all` on a pipe.
fn top_level_commands(home: &Path) -> Vec<String> {
    let out = ljos(home, &["help", "--all"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            let name = rest.split_whitespace().next()?;
            (!rest.starts_with(' ') && name.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
                .then(|| name.to_string())
        })
        .collect();
    assert!(names.len() > 40, "{names:?}");
    names
}

#[test]
fn every_help_fits_and_every_json_export_parses_at_twenty_columns() {
    let home = tempfile::tempdir().unwrap();
    let mut cases: Vec<Vec<String>> = vec![
        vec!["--help".into()],
        vec!["help".into()],
        vec!["help".into(), "--all".into()],
    ];
    for name in top_level_commands(home.path()) {
        cases.push(vec![name, "--help".into()]);
    }
    for args in &cases {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, shown) = on_tty(ljos(home.path(), &args));
        let what = format!("ljos {}", args.join(" "));
        assert_eq!(code, Some(0), "{what}\n{shown}");
        assert!(shown.lines().count() > 1, "{what}\n{shown}");
        assert_human_fits(&what, &shown);
    }
    for args in [
        ["memory", "export", "--out", "-", "--json"].as_slice(),
        ["export", "--out", "-", "--json"].as_slice(),
    ] {
        let (code, shown) = on_tty(ljos(home.path(), args));
        let what = format!("ljos {}", args.join(" "));
        assert_eq!(code, Some(0), "{what}\n{shown}");
        assert_all_json(&what, &shown);
    }
}

#[test]
fn root_help_wraps_on_a_tty() {
    let home = tempfile::tempdir().unwrap();
    let piped = ljos(home.path(), &["--help"]).output().unwrap();
    let piped = String::from_utf8_lossy(&piped.stdout).into_owned();
    assert!(piped.lines().any(|line| line.chars().count() > COLS));
    let (code, shown) = on_tty(ljos(home.path(), &["--help"]));
    assert_eq!(code, Some(0), "{shown}");
    assert!(shown.lines().count() > piped.lines().count(), "{shown}");
    assert_human_fits("ljos --help", &shown);
}

#[test]
fn export_to_stdout_never_wraps_stderr() {
    let home = tempfile::tempdir().unwrap();
    let said = "exported 0 memories to -";
    assert!(said.len() > COLS);
    let (code, shown) = on_tty(ljos(home.path(), &["memory", "export", "--out", "-"]));
    assert_eq!(code, Some(0), "{shown}");
    assert!(shown.lines().any(|line| line == said), "{shown}");

    let (code, shown) = on_tty(ljos(
        home.path(),
        &["memory", "export", "--out", "-", "--json"],
    ));
    assert_eq!(code, Some(0), "{shown}");
    assert_all_json("export --out - --json", &shown);
    assert!(
        shown.lines().any(|line| line.contains("\"memories\":0")),
        "{shown}"
    );
}

#[test]
fn every_machine_kind_entry_point_is_unwrapped_on_a_tty() {
    let home = tempfile::tempdir().unwrap();
    let mut cases = Vec::new();

    cases.push((
        "--json",
        ljos(home.path(), &["memory", "export", "--json"]),
        "memory export needs --out <OUT>. `ljos memory export --help` lists what to pass.",
        None,
    ));
    cases.push((
        "--porcelain",
        ljos(home.path(), &["search", "--porcelain", "--unknown"]),
        "ljos: search has no flag --unknown, so run `ljos search --help`.",
        None,
    ));
    cases.push((
        "--out -",
        ljos(
            home.path(),
            &["memory", "export", "--out", "-", "--unknown"],
        ),
        "ljos: memory export has no flag --unknown, so run `ljos memory export --help`.",
        None,
    ));
    let mut hookd = ljos(home.path(), &["hookd"]);
    hookd.env(
        "XDG_RUNTIME_DIR",
        "/proc/ljos-output-kind-directory-cannot-be-created",
    );
    cases.push((
        "hookd",
        hookd,
        "ljos hookd: cannot open /proc/ljos-output-kind-directory-cannot-be-created/ljos/hookd.lock",
        None,
    ));
    cases.push((
        "ljos mcp",
        ljos(home.path(), &["mcp"]),
        "Error: connection closed: initialize request",
        None,
    ));
    cases.push((
        "ljos-mcp",
        hermetic(env!("CARGO_BIN_EXE_ljos-mcp"), home.path()),
        "Error: connection closed: initialize request",
        None,
    ));

    let socket_dir = home.path().join("ljos");
    std::fs::create_dir_all(&socket_dir).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(socket_dir.join("hookd.sock")).unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(stream.try_clone().unwrap()),
            &mut String::new(),
        )
        .unwrap();
        writeln!(stream, "full 1").unwrap();
    });
    let payload = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "output-kind",
        "hook_event_name": "beforeShellExecution",
        "command": "true",
        "cwd": home.path(),
    })
    .to_string();
    cases.push((
        "hook admission",
        ljos(home.path(), &["hook"]),
        "ljos hook: hook queue: 1 hook calls are already waiting for a slot; the call is judged in this process",
        Some(payload),
    ));

    for (what, cmd, line, input) in cases {
        assert!(line.len() > COLS);
        let (code, shown) = on_tty_with_input(cmd, input.as_deref());
        if what == "hook admission" {
            assert_eq!(code, Some(0), "{what}\n{shown}");
        } else {
            assert_ne!(code, Some(0), "{what}\n{shown}");
        }
        assert!(shown.lines().any(|shown| shown == line), "{what}\n{shown}");
    }
}

#[test]
fn the_error_that_ends_ljos_mcp_is_the_same_on_a_pipe() {
    let home = tempfile::tempdir().unwrap();
    let line = "Error: connection closed: initialize request";
    let piped = hermetic(env!("CARGO_BIN_EXE_ljos-mcp"), home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(piped.status.code(), Some(1), "{piped:?}");
    assert_eq!(String::from_utf8_lossy(&piped.stderr), format!("{line}\n"));
}
