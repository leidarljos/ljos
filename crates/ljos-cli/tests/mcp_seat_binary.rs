//! `ljos_doctor` over MCP checks the sibling `ljos` binary instead of
//! comparing `ljos` against `ljos-mcp` itself.

ljos_test_boot::harness!();
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

fn call_doctor(server: &Path, bin_dir: &Path) -> serde_json::Value {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(server);
    cmd.env_clear()
        .env("TMPDIR", std::env::temp_dir())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("PACKSET_URL", "off")
        .env("PATH", format!("{}:/usr/bin:/bin", bin_dir.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    std::fs::create_dir_all(home.path().join("run")).unwrap();
    let mut child = cmd.spawn().unwrap();
    let lines = [
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"}}
        }),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "ljos_doctor", "arguments": {}}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        !stderr.contains("panicked"),
        "stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    let answer = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value["id"] == 2)
        .unwrap_or_else(|| panic!("no doctor answer\nstdout:\n{stdout}\nstderr:\n{stderr}"));
    let rows = answer["result"]["structuredContent"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("no rows in doctor answer: {answer}"));
    rows.iter()
        .find(|row| row["name"] == "seat binary")
        .unwrap_or_else(|| panic!("no seat binary row: {answer}"))
        .clone()
}

#[test]
fn mcp_doctor_passes_seat_binary_when_ljos_matches_and_flags_a_stub() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("good");
    std::fs::create_dir_all(&good).unwrap();
    let good_ljos = good.join("ljos");
    let good_mcp = good.join("ljos-mcp");
    std::fs::copy(env!("CARGO_BIN_EXE_ljos"), &good_ljos).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_ljos-mcp"), &good_mcp).unwrap();

    let ok_row = call_doctor(&good_mcp, &good);
    assert_eq!(ok_row["ok"], true, "{ok_row}");
    let state = ok_row["state"].as_str().unwrap_or("");
    assert!(state.contains("is this ljos"), "{ok_row}");

    let tampered = dir.path().join("tampered");
    std::fs::create_dir_all(&tampered).unwrap();
    let stub = tampered.join("ljos");
    let bad_mcp = tampered.join("ljos-mcp");
    std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::copy(env!("CARGO_BIN_EXE_ljos-mcp"), &bad_mcp).unwrap();

    let bad_row = call_doctor(&bad_mcp, &tampered);
    assert_eq!(bad_row["ok"], false, "{bad_row}");
    let bad_state = bad_row["state"].as_str().unwrap_or("");
    assert!(bad_state.contains("not a binary"), "{bad_row}");
}
