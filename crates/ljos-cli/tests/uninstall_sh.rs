//! `scripts/uninstall.sh` on a home whose ljos was deleted by hand.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn executable(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(path, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/uninstall.sh")
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// A home as a seat leaves it: the editor harness and the terminal harness hooks beside other
/// hooks, MCP entries, a skill, the git hooks, and a bin dir where the
/// `ljos` binary is gone and `ljos-mcp` remains.
fn hand_deleted_home() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let cargo_bin = home.join(".cargo/bin");
    std::fs::create_dir_all(&cargo_bin).unwrap();
    let ljos = cargo_bin.join("ljos").display().to_string();
    write(
        &home.join(".cursor/hooks.json"),
        &serde_json::json!({
            "version": 1,
            "hooks": {
                "beforeShellExecution": [
                    {"command": format!("{ljos} hook"), "timeout": 10, "failClosed": true},
                    {"command": "other-tool check", "timeout": 5}
                ],
                "sessionStart": [{"command": format!("{ljos} hook"), "timeout": 5}]
            }
        })
        .to_string(),
    );
    write(
        &home.join(".claude/settings.json"),
        &serde_json::json!({
            "theme": "dark",
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "other"}]},
                    {"matcher": "Bash|Edit", "hooks": [{"type": "command", "command": format!("{ljos} hook"), "onFailure": "block"}]}
                ],
                "UserPromptSubmit": [{"matcher": "*", "hooks": [{"type": "command", "command": "ljos hook"}]}]
            }
        })
        .to_string(),
    );
    write(
        &home.join(".claude.json"),
        r#"{"mcpServers":{"ljos":{"command":"ljos-mcp"},"other":{"command":"x"}}}"#,
    );
    write(
        &home.join(".codex/config.toml"),
        "model = \"x\"\n\n[mcp_servers.ljos]\ncommand = \"ljos-mcp\"\n\n[mcp_servers.keep]\ncommand = \"k\"\n",
    );
    write(&home.join(".claude/skills/ljos/SKILL.md"), "# ljos\n");
    write(&home.join(".claude/skills/mine/SKILL.md"), "# mine\n");
    executable(&cargo_bin.join("ljos-mcp"), "\u{7f}ELF\n");
    executable(&cargo_bin.join("packset"), "\u{7f}ELF\n");
    let hooks = home.join(".local/share/ljos/git-hooks");
    executable(&hooks.join("pre-commit"), "#!/bin/sh\nljos git-hook\n");
    write(&hooks.join("previous-hooks-path"), "/home/u/myhooks\n");
    write(
        &home.join(".gitconfig"),
        &format!("[core]\n\thooksPath = {}\n", hooks.display()),
    );
    write(&home.join(".local/state/ljos/onboard/manifest.json"), "{}");
    temp
}

fn uninstall(home: &Path, args: &[&str]) -> Output {
    Command::new("sh")
        .arg(script())
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

fn hooks_path(home: &Path) -> String {
    let out = Command::new("git")
        .args(["config", "--global", "--get", "core.hooksPath"])
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn the_script_cleans_a_home_whose_ljos_is_gone() {
    let temp = hand_deleted_home();
    let home = temp.path();
    let cursor = home.join(".cursor/hooks.json");
    let claude = home.join(".claude/settings.json");

    let dry = uninstall(home, &["--dry-run"]);
    let text = said(&dry);
    assert!(dry.status.success(), "{text}");
    assert!(text.contains("would drop the ljos hooks from"), "{text}");
    assert!(
        text.contains("would set core.hooksPath back to /home/u/myhooks"),
        "{text}"
    );
    assert!(
        read(&cursor).contains("ljos hook"),
        "a dry run writes nothing"
    );
    assert!(home.join(".claude/skills/ljos").is_dir());

    let live = uninstall(home, &[]);
    let text = said(&live);
    assert!(live.status.success(), "{text}");

    let cursor_doc: serde_json::Value = serde_json::from_str(&read(&cursor)).unwrap();
    assert_eq!(cursor_doc["version"], 1);
    assert_eq!(
        cursor_doc["hooks"]["beforeShellExecution"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "{cursor_doc}"
    );
    assert_eq!(
        cursor_doc["hooks"]["beforeShellExecution"][0]["command"],
        "other-tool check"
    );
    assert!(
        cursor_doc["hooks"]["sessionStart"].is_null(),
        "{cursor_doc}"
    );

    let claude_doc: serde_json::Value = serde_json::from_str(&read(&claude)).unwrap();
    assert_eq!(claude_doc["theme"], "dark");
    assert_eq!(
        claude_doc["hooks"]["PreToolUse"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        claude_doc["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "other"
    );
    assert!(
        claude_doc["hooks"]["UserPromptSubmit"].is_null(),
        "{claude_doc}"
    );

    let mcp: serde_json::Value = serde_json::from_str(&read(&home.join(".claude.json"))).unwrap();
    assert!(mcp["mcpServers"]["ljos"].is_null(), "{mcp}");
    assert_eq!(mcp["mcpServers"]["other"]["command"], "x");
    let codex = read(&home.join(".codex/config.toml"));
    assert!(!codex.contains("mcp_servers.ljos"), "{codex}");
    assert!(codex.contains("[mcp_servers.keep]"), "{codex}");

    assert!(!home.join(".claude/skills/ljos").exists());
    assert!(home.join(".claude/skills/mine/SKILL.md").is_file());

    assert_eq!(hooks_path(home), "/home/u/myhooks");
    assert!(!home.join(".local/share/ljos/git-hooks").exists());

    let bin = home.join(".cargo/bin");
    assert!(!bin.join("ljos-mcp").exists());
    assert!(bin.join("packset").is_file(), "other seat programs stay");
    let stub = read(&bin.join("ljos"));
    assert!(
        stub.contains("ljos was uninstalled"),
        "the path the hooks named gets the stub: {stub}"
    );
    let answer = Command::new("sh")
        .arg(bin.join("ljos"))
        .arg("hook")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(answer.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&answer.stdout).contains("\"permission\":\"allow\""),
        "{}",
        said(&answer)
    );
    assert!(
        home.join(".local/state/ljos/onboard/manifest.json")
            .is_file(),
        "state stays without --purge"
    );

    let again = uninstall(home, &[]);
    assert!(again.status.success(), "{}", said(&again));
    assert_eq!(
        read(&cursor),
        serde_json::to_string_pretty(&cursor_doc).unwrap() + "\n"
    );
}

#[test]
fn purge_and_no_stub_leave_nothing_of_ljos() {
    let temp = hand_deleted_home();
    let home = temp.path();
    let out = uninstall(home, &["--purge", "--no-stub"]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(!home.join(".cargo/bin/ljos").exists());
    assert!(!home.join(".local/state/ljos").exists());
    assert!(!home.join(".local/share/ljos").exists());
    assert!(home.join(".cargo/bin/packset").is_file());
}

/// With a ljos that knows `uninstall` in the bin dir, the script hands
/// the work to it.
#[test]
fn the_script_hands_over_to_a_ljos_that_knows_uninstall() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let bin = home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
    let out = Command::new("sh")
        .arg(script())
        .arg("--dry-run")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PACKSET_URL", "off")
        .env("DEEDAR_URL", "file:///nonexistent/ljos-uninstall-test")
        .env("DEEDAR_HOST_SIGNING_KEY", "off")
        .env("VISSUE_ROOT", "/nonexistent/ljos-uninstall-test")
        .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LJOS_HOOKD", "off")
        .output()
        .unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "Running {} uninstall --dry-run",
            bin.join("ljos").display()
        )),
        "{text}"
    );
    assert!(text.contains("would replace"), "{text}");
    assert!(
        bin.join("ljos").metadata().unwrap().len() > 100_000,
        "a dry run keeps the binary"
    );
}
