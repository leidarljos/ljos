//! `ljos onboard` with no runner prints the MCP entry to paste, and that
//! output has to parse as JSON as it stands.

use std::process::Command;

#[test]
fn the_bare_onboard_prints_json_alone() {
    let home = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("onboard")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    let entry: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"));
    let command = entry["mcpServers"]["ljos"]["command"]
        .as_str()
        .unwrap_or_default();
    assert!(command.ends_with("ljos-mcp"), "{text}");
}
