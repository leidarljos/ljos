//! `ljos_doctor` over `ljos mcp` marks the extras a seat runs without, so
//! a client does not read an absent `ljos-hud` as a failure, and the
//! encoder hint it carries does not depend on the directory the client
//! started the server in or print a downloaded shell command.

ljos_test_boot::harness!();

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

/// The rows `ljos_doctor` returns for a seat with nothing set up, with
/// `ljos mcp` started in `cwd`.
fn rows(home: &Path, cwd: &Path) -> Vec<Value> {
    let stub = home.join("stub");
    std::fs::create_dir_all(&stub).unwrap();
    let curl = stub.join("curl");
    ljos_cli::install_executable(&curl, "#!/bin/sh\nexit 0\n").unwrap();
    let gitconfig = home.join("gitconfig");
    std::fs::write(&gitconfig, "").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("mcp")
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", stub.display()))
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PACKSET_URL", "off")
        .env("LJOS_NO_NETWORK", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(90));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "ljos_doctor", "arguments": {}}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let answer = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|answer| answer["id"] == 2)
        .unwrap_or_else(|| panic!("no answer\nstdout:\n{stdout}\nstderr:\n{stderr}"));
    let body = &answer["result"]["structuredContent"];
    body["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("no rows in {answer}"))
        .clone()
}

fn row<'a>(rows: &'a [Value], name: &str) -> &'a Value {
    rows.iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no {name} row in {rows:?}"))
}

#[test]
fn optional_extras_are_marked_and_failures_are_not() {
    let home = tempfile::tempdir().unwrap();
    let rows = rows(home.path(), home.path());
    for name in [
        "packset-embed",
        "ljos-hud",
        "cloud guard",
        "runners",
        "git hooks",
    ] {
        let found = row(&rows, name);
        assert_eq!(found["ok"], false, "{name}: {found}");
        assert_eq!(found["optional"], true, "{name}: {found}");
    }
    for name in ["ljos", "seat source"] {
        let found = row(&rows, name);
        assert_eq!(found["ok"], false, "{name}: {found}");
        assert_eq!(found["optional"], false, "{name}: {found}");
    }
    let marked: Vec<&str> = rows
        .iter()
        .filter(|row| row["optional"] == true)
        .filter_map(|row| row["name"].as_str())
        .collect();
    assert!(!marked.contains(&"ljos"), "{marked:?}");
}

#[test]
fn the_encoder_hint_is_the_same_in_a_checkout_and_out_of_one() {
    let home = tempfile::tempdir().unwrap();
    let checkout = home.path().join("checkout");
    std::fs::create_dir_all(checkout.join("scripts")).unwrap();
    std::fs::write(checkout.join("scripts/install.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(checkout.join("install.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    let bare = home.path().join("bare");
    std::fs::create_dir_all(&bare).unwrap();

    let inside = rows(home.path(), &checkout);
    let outside = rows(home.path(), &bare);
    for rows in [&inside, &outside] {
        for found in rows {
            let text = found["state"].as_str().unwrap();
            assert!(
                !text.contains("curl") && !text.contains("wget") && !text.contains("install.sh"),
                "{text}"
            );
        }
        let text = row(rows, "packset-embed")["state"].as_str().unwrap();
        assert!(text.contains(ljos_cli::INSTALL_PAGE), "{text}");
        assert!(text.contains("run `ljos doctor`"), "{text}");
    }
    assert_eq!(
        row(&inside, "packset-embed")["state"],
        row(&outside, "packset-embed")["state"]
    );
}
