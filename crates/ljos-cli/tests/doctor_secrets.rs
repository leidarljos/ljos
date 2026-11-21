//! `ljos doctor` and `ljos_doctor` name a secret variable and say whether it
//! is set. They never print its value.

ljos_test_boot::harness!();

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

const AGE_SECRET: &str = "FakeAgeSecretValue77";
const HOST_KEY: &str = "ed25519:ZmFrZS1ob3N0LXNpZ25pbmctc2VlZA";
const TRACKER_TOKEN: &str = "fake-tracker-token-5521";
const LAUNCHER_KEY: &str = "fake-launcher-key-8830";
const SEAT_KEY: &str = "fake-seat-key-1904";
const PACK_TOKEN: &str = "fake-pack-token-3377";
const WAKE_KEY: &str = "fake-wake-key-6642";
const IDENTITY: &str =
    "AGE-SECRET-KEY-1FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE";

fn command(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    let gitconfig = home.join("gitconfig");
    std::fs::write(&gitconfig, "").unwrap();
    cmd.env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PACKSET_URL", "off")
        .env("LJOS_NO_NETWORK", "1")
        .env("LJOS_CLOUD_AGE_SECRET", AGE_SECRET)
        .env("DEEDAR_HOST_SIGNING_KEY", HOST_KEY)
        .env("LJOS_TRACKER_TOKEN", TRACKER_TOKEN)
        .env("LJOS_WAKE_LAUNCHER_KEY", LAUNCHER_KEY)
        .env("LJOS_SEAT_KEY", SEAT_KEY)
        .env("PACKSET_TOKEN", PACK_TOKEN)
        .env("LJOS_WAKE_INKY_KEY", WAKE_KEY)
        .current_dir(home);
    cmd
}

fn cli_doctor(mut cmd: Command) -> String {
    let out = cmd.args(["doctor", "--all"]).output().unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn mcp_doctor(mut cmd: Command) -> (String, Vec<Value>) {
    let mut child = cmd
        .arg("mcp")
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
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let answer = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|answer| answer["id"] == 2)
        .unwrap_or_else(|| panic!("no answer\nstdout:\n{stdout}\nstderr:\n{stderr}"));
    let rows = answer["result"]["structuredContent"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("no rows in {answer}"))
        .clone();
    (format!("{stdout}{stderr}"), rows)
}

fn row<'a>(rows: &'a [Value], name: &str) -> &'a str {
    rows.iter()
        .find(|row| row["name"] == name)
        .and_then(|row| row["state"].as_str())
        .unwrap_or_else(|| panic!("no {name} row in {rows:?}"))
}

fn no_secret(said: &str, values: &[&str]) {
    for value in values {
        assert!(!said.contains(value), "{value} was printed:\n{said}");
    }
}

#[test]
fn the_cli_doctor_prints_no_secret_value() {
    let home = tempfile::tempdir().unwrap();
    let said = cli_doctor(command(home.path()));
    no_secret(
        &said,
        &[
            AGE_SECRET,
            HOST_KEY,
            TRACKER_TOKEN,
            LAUNCHER_KEY,
            SEAT_KEY,
            PACK_TOKEN,
            WAKE_KEY,
        ],
    );
    assert!(said.contains("cloud age"), "{said}");
    assert!(said.contains("LJOS_CLOUD_AGE_SECRET names a variable that is unset"));
    assert!(said.contains("DEEDAR_HOST_SIGNING_KEY is set"), "{said}");
    assert!(said.contains("LJOS_TRACKER_TOKEN is set from"), "{said}");
    assert!(
        said.contains("LJOS_WAKE_LAUNCHER_KEY is set from"),
        "{said}"
    );
}

#[test]
fn the_mcp_doctor_prints_no_secret_value() {
    let home = tempfile::tempdir().unwrap();
    let (said, rows) = mcp_doctor(command(home.path()));
    no_secret(
        &said,
        &[
            AGE_SECRET,
            HOST_KEY,
            TRACKER_TOKEN,
            LAUNCHER_KEY,
            SEAT_KEY,
            PACK_TOKEN,
            WAKE_KEY,
        ],
    );
    assert!(row(&rows, "cloud age").starts_with("LJOS_CLOUD_AGE_SECRET names a variable"));
    assert!(row(&rows, "host key").starts_with("DEEDAR_HOST_SIGNING_KEY is set"));
}

#[test]
fn a_pasted_age_identity_is_not_printed() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = command(home.path());
    cmd.env("LJOS_CLOUD_AGE_SECRET", IDENTITY);
    let said = cli_doctor(cmd);
    no_secret(&said, &[IDENTITY, "1FAKEFAKE"]);
    assert!(said.contains("age identity; set it to the variable"));
    let mut cmd = command(home.path());
    cmd.env("LJOS_CLOUD_AGE_SECRET", IDENTITY);
    let (said, rows) = mcp_doctor(cmd);
    no_secret(&said, &[IDENTITY, "1FAKEFAKE"]);
    assert!(row(&rows, "cloud age").contains("age identity; set it to the variable"));
}

#[test]
fn a_named_variable_that_is_set_is_named() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = command(home.path());
    cmd.env("LJOS_CLOUD_AGE_SECRET", "LJOS_FAKE_IDENTITY_VAR")
        .env("LJOS_FAKE_IDENTITY_VAR", IDENTITY);
    let said = cli_doctor(cmd);
    no_secret(&said, &[IDENTITY, "1FAKEFAKE"]);
    assert!(said.contains("LJOS_FAKE_IDENTITY_VAR is "));
}

#[test]
fn a_host_key_path_is_printed_as_a_path() {
    let home = tempfile::tempdir().unwrap();
    let key = home.path().join("host.key");
    std::fs::write(&key, [7u8; 32]).unwrap();
    let mut cmd = command(home.path());
    cmd.env("DEEDAR_HOST_SIGNING_KEY", &key);
    let said = cli_doctor(cmd);
    assert!(said.contains(&key.display().to_string()), "{said}");
    assert!(said.contains("the signing key for deeds"), "{said}");
    no_secret(&said, &[AGE_SECRET, TRACKER_TOKEN, LAUNCHER_KEY]);
}

#[test]
fn a_seat_key_path_is_printed_and_its_contents_are_not() {
    let home = tempfile::tempdir().unwrap();
    let key = home.path().join("ballot.key");
    std::fs::write(&key, SEAT_KEY).unwrap();
    let mut cmd = command(home.path());
    cmd.env("LJOS_SEAT_KEY", &key);
    let said = cli_doctor(cmd);
    assert!(said.contains(&key.display().to_string()), "{said}");
    no_secret(&said, &[SEAT_KEY]);
}
