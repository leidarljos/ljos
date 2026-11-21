//! One MCP process serves two seats.
//!
//! Each note commit names the seat that call passed.
//! A write with no `seat` argument and no stamped seat of its own is refused.
//! A read still runs. The runner's seat variables are not this process's seat.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(root: &Path, config: &Path, args: &[&str]) -> String {
    let out = ljos_test_boot::git()
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn two_seats_keep_their_notes_and_a_seatless_write_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let tracker = root.path().join("tracker");
    let bin = root.path().join("bin");
    let run = root.path().join("run");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(tracker.join("Software/probe")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    let issues = tracker.join("Software/probe/issues.org");
    std::fs::write(
        &issues,
        "* TODO [#C] Probe\n:PROPERTIES:\n:ID:         probe-a1b2\n:END:\n",
    )
    .unwrap();
    let board = issues.display().to_string();
    script(
        &bin,
        "vissue",
        &format!("#!/bin/sh\nif [ \"$1\" = note ]; then printf '%s\\n' \"- $3\" >> '{board}'; fi\nexit 0\n"),
    );
    script(&bin, "claimdag", "#!/bin/sh\nexit 1\n");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
    let gitconfig = root.path().join("gitconfig");
    std::fs::write(&gitconfig, "[core]\n\thooksPath = /dev/null\n").unwrap();
    git(&tracker, &gitconfig, &["init", "-q"]);
    git(
        &tracker,
        &gitconfig,
        &["config", "user.name", "Ada Example"],
    );
    git(
        &tracker,
        &gitconfig,
        &["config", "user.email", "ada@example.invalid"],
    );
    git(
        &tracker,
        &gitconfig,
        &["config", "core.hooksPath", "/dev/null"],
    );
    git(&tracker, &gitconfig, &["add", "."]);
    let seeded = ljos_test_boot::git()
        .arg("-C")
        .arg(&tracker)
        .args(["commit", "-q", "-m", "seed"])
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada Example")
        .env("GIT_AUTHOR_EMAIL", "ada@example.invalid")
        .env("GIT_COMMITTER_NAME", "Ada Example")
        .env("GIT_COMMITTER_EMAIL", "ada@example.invalid")
        .output()
        .unwrap();
    assert!(
        seeded.status.success(),
        "seed: {}",
        String::from_utf8_lossy(&seeded.stderr)
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .current_dir(&tracker)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("VISSUE_ROOT", &tracker)
        .env("ISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "hlebot")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("LJOS_SYNC_HOST", "runner")
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let note = |id: u64, seat: Option<&str>, text: &str| {
        let mut arguments = serde_json::json!({"issue": "probe-a1b2", "text": text});
        if let Some(seat) = seat {
            arguments["seat"] = serde_json::Value::String(seat.to_string());
        }
        serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
            "name":"ljos_note","arguments": arguments
        }})
    };
    let lines = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"shared","version":"1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        note(2, Some("gpridge-paper"), "paper one"),
        note(3, Some("hlebot"), "bot one"),
        note(4, Some("gpridge-paper"), "paper two"),
        note(5, None, "guessed"),
        serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{
            "name":"ljos_guard_check","arguments":{"command":"printf ok"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{
        "name":"ljos_finish","arguments":{
            "seat":"gpridge-paper","issue":"probe-a1b2","lesson":"The probe held."
        }}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let stdout = child.stdout.take().unwrap();
    let mut answers = std::collections::BTreeMap::new();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(id) = value["id"].as_u64() {
            answers.insert(id, value);
        }
    }
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    let answer = |id: u64| {
        answers
            .get(&id)
            .unwrap_or_else(|| panic!("no answer {id}: {answers:?}\n{stderr}"))
            .clone()
    };
    let text = |value: &serde_json::Value| {
        value["result"]["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|block| block["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    for id in [2, 3, 4] {
        let said = answer(id);
        assert_ne!(
            said["result"]["isError"], true,
            "note {id}: {said}\n{stderr}"
        );
    }
    let refused = answer(5);
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    let refused_text = text(&refused);
    assert!(
        refused_text.contains("seat") && refused_text.lines().count() <= 3,
        "the refusal is not one line naming seat:\n{refused_text}"
    );
    assert!(
        !refused_text.contains("hlebot") && !refused_text.contains("gpridge-paper"),
        "a guessed seat was named:\n{refused_text}"
    );
    let guard = answer(6);
    assert_ne!(
        guard["result"]["isError"], true,
        "a read was refused: {guard}"
    );
    assert!(
        !text(&guard).contains("pass the seat argument"),
        "a read asked for seat: {guard}"
    );
    let finished = text(&answer(7));
    assert!(
        finished.contains("ticket probe-a1b2 was not closed:"),
        "finish did not say why the ticket stayed open:\n{finished}\n{stderr}"
    );
    assert_eq!(finished.matches("was not closed:").count(), 1, "{finished}");

    let subjects = git(&tracker, &gitconfig, &["log", "--reverse", "--format=%s"]);
    let notes: Vec<&str> = subjects
        .lines()
        .filter(|line| line.contains(" noted by "))
        .collect();
    assert_eq!(
        notes,
        [
            "chore(issues): probe-a1b2 noted by gpridge-paper",
            "chore(issues): probe-a1b2 noted by hlebot",
            "chore(issues): probe-a1b2 noted by gpridge-paper",
        ],
        "subjects:\n{subjects}"
    );
    let board_text = std::fs::read_to_string(&issues).unwrap();
    assert!(board_text.contains("paper one"), "{board_text}");
    assert!(board_text.contains("bot one"), "{board_text}");
    assert!(board_text.contains("paper two"), "{board_text}");
    assert!(!board_text.contains("guessed"), "{board_text}");
}
