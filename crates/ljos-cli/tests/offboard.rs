//! Every shipped runner returns to its original files after onboarding.

ljos_test_boot::harness!();

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ljos_cli::{Harness, Harnesses, HARNESSES_EXAMPLE};

fn expand(home: &Path, path: &str) -> PathBuf {
    path.strip_prefix("~/")
        .map_or_else(|| PathBuf::from(path), |rest| home.join(rest))
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn seed_runner(home: &Path, harness: &Harness) {
    if let Some(config) = harness.config_json.as_deref().or(harness.config.as_deref()) {
        let path = expand(home, config);
        let text = if harness.config_json.is_some() {
            format!("{{\n  \"mine\": \"{}\"\n}}\n", harness.name)
        } else if harness.name == "goose" {
            format!("mine: {}\n", harness.name)
        } else {
            format!("mine = \"{}\"\n", harness.name)
        };
        write(&path, &text);
    }
    if let Some(hooks) = harness.hooks.as_deref() {
        let path = expand(home, hooks);
        if harness.hooks_format.as_deref() == Some("cline") {
            fs::create_dir_all(path).unwrap();
        } else if !path.is_file() {
            write(
                &path,
                &format!("{{\n  \"mine\": \"{}\"\n}}\n", harness.name),
            );
        }
    }
    if let Some(plugin) = harness.plugin.as_deref() {
        write(&expand(home, plugin), "mine\n");
    }
}

fn fake_runner_commands(home: &Path) -> PathBuf {
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    write(
        &bin.join("claude"),
        r#"#!/bin/sh
case "$*" in
  "mcp get ljos") exit 1 ;;
  "mcp add"*) mkdir -p "$HOME"; printf '%s\n' '{"mcpServers":{"ljos":{"command":"ljos-mcp"}}}' > "$HOME/.claude.json" ;;
esac
"#,
    );
    write(
        &bin.join("hermes"),
        r#"#!/bin/sh
mkdir -p "$HOME/.hermes"
printf '%s\n' 'mcp_servers:' '  ljos:' '    command: ljos-mcp' > "$HOME/.hermes/config.yaml"
"#,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["claude", "hermes"] {
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    bin
}

fn files_under(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(at) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, out);
            } else if path.is_file() {
                out.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

fn run(home: &Path, bin: &Path, args: &[&str]) -> Output {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(bin.to_path_buf()).chain(std::env::split_paths(&inherited)),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("PATH", path)
        .env("PACKSET_URL", "off")
        .env("DEEDAR_URL", "file:///nonexistent/ljos-offboard-test")
        .env("DEEDAR_HOST_SIGNING_KEY", "off")
        .env("VISSUE_ROOT", "/nonexistent/ljos-offboard-test")
        .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

#[test]
fn onboard_then_offboard_restores_every_runner() {
    let shipped: Harnesses = toml::from_str(HARNESSES_EXAMPLE).unwrap();
    for harness in shipped
        .harness
        .iter()
        .filter(|h| !h.name.starts_with("runner-with-"))
    {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        seed_runner(home, harness);
        let bin = fake_runner_commands(home);
        let before = files_under(home);

        let onboard = run(home, &bin, &["onboard", "--harness", &harness.name]);
        assert!(
            onboard.status.success(),
            "{} onboard: {}",
            harness.name,
            String::from_utf8_lossy(&onboard.stderr)
        );
        assert!(
            home.join(".local/state/ljos/onboard/manifest.json")
                .is_file(),
            "{} made no manifest",
            harness.name
        );

        let offboard = run(home, &bin, &["offboard"]);
        assert!(
            offboard.status.success(),
            "{} offboard: {}",
            harness.name,
            String::from_utf8_lossy(&offboard.stderr)
        );
        assert_eq!(files_under(home), before, "{}", harness.name);

        let again = run(home, &bin, &["offboard"]);
        assert!(again.status.success(), "{} second offboard", harness.name);
        assert_eq!(
            files_under(home),
            before,
            "{} second offboard",
            harness.name
        );
    }
}

#[test]
fn dry_run_and_one_runner_do_not_touch_other_files() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let bin = fake_runner_commands(home);
    let one = run(home, &bin, &["onboard", "--harness", "zed"]);
    assert!(one.status.success(), "{one:?}");
    let two = run(home, &bin, &["onboard", "--harness", "amazonq"]);
    assert!(two.status.success(), "{two:?}");
    let installed = files_under(home);

    let dry = run(home, &bin, &["offboard", "--runner", "zed", "--dry-run"]);
    assert!(dry.status.success(), "{dry:?}");
    assert_eq!(files_under(home), installed);

    let remove = run(home, &bin, &["offboard", "--runner", "zed"]);
    assert!(remove.status.success(), "{remove:?}");
    assert!(!home.join(".config/zed/settings.json").is_file());
    assert!(home.join(".aws/amazonq/mcp.json").is_file());
}

#[test]
fn offboard_keeps_an_edit_made_after_onboarding() {
    let shipped: Harnesses = toml::from_str(HARNESSES_EXAMPLE).unwrap();
    let zed = shipped.harness.iter().find(|h| h.name == "zed").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_runner(home, zed);
    let bin = fake_runner_commands(home);
    let onboard = run(home, &bin, &["onboard", "--harness", "zed"]);
    assert!(onboard.status.success(), "{onboard:?}");

    let config = home.join(".config/zed/settings.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value["user_edit"] = serde_json::json!(true);
    write(
        &config,
        &(serde_json::to_string_pretty(&value).unwrap() + "\n"),
    );

    let offboard = run(home, &bin, &["offboard"]);
    assert!(offboard.status.success(), "{offboard:?}");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    assert_eq!(value["mine"], "zed");
    assert_eq!(value["user_edit"], true);
    assert!(value.pointer("/context_servers/ljos").is_none(), "{value}");
}

#[test]
fn offboard_keeps_an_edited_hook_beside_its_own_entries() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let bin = fake_runner_commands(home);
    let settings = home.join(".gemini/settings.json");
    write(
        &settings,
        r#"{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "mine",
        "hooks": [
          {
            "type": "command",
            "command": "mine"
          }
        ]
      }
    ]
  }
}
"#,
    );
    let onboard = run(home, &bin, &["onboard", "--harness", "gemini"]);
    assert!(onboard.status.success(), "{onboard:?}");

    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    value["hooks"]["BeforeTool"][0]["hooks"][0]["command"] = serde_json::json!("mine-edited");
    write(
        &settings,
        &(serde_json::to_string_pretty(&value).unwrap() + "\n"),
    );

    let offboard = run(home, &bin, &["offboard"]);
    assert!(offboard.status.success(), "{offboard:?}");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    assert_eq!(
        value["hooks"]["BeforeTool"][0]["hooks"][0]["command"],
        "mine-edited"
    );
    assert!(value.pointer("/mcpServers/ljos").is_none(), "{value}");
    assert!(!value.to_string().contains("ljos hook"), "{value}");
    assert!(
        !home
            .join(".local/state/ljos/onboard/manifest.json")
            .is_file(),
        "the fallback completed the manifest"
    );
}

#[test]
fn an_install_without_a_manifest_uses_ljos_markers() {
    let shipped: Harnesses = toml::from_str(HARNESSES_EXAMPLE).unwrap();
    let zed = shipped.harness.iter().find(|h| h.name == "zed").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    seed_runner(home, zed);
    let bin = fake_runner_commands(home);
    let onboard = run(home, &bin, &["onboard", "--harness", "zed"]);
    assert!(onboard.status.success(), "{onboard:?}");
    fs::remove_dir_all(home.join(".local/state/ljos/onboard")).unwrap();

    let offboard = run(home, &bin, &["offboard", "--runner", "zed"]);
    assert!(offboard.status.success(), "{offboard:?}");
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(home.join(".config/zed/settings.json")).unwrap()).unwrap();
    assert_eq!(config["mine"], "zed");
    assert!(
        config.pointer("/context_servers/ljos").is_none(),
        "{config}"
    );
    assert!(!home.join(".config/zed/skills/ljos/SKILL.md").is_file());
}

#[test]
fn marker_fallback_removes_every_shipped_runner() {
    let shipped: Harnesses = toml::from_str(HARNESSES_EXAMPLE).unwrap();
    for harness in shipped
        .harness
        .iter()
        .filter(|h| !h.name.starts_with("runner-with-"))
    {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let bin = fake_runner_commands(home);
        let before = files_under(home);
        let onboard = run(home, &bin, &["onboard", "--harness", &harness.name]);
        assert!(
            onboard.status.success(),
            "{} onboard: {}",
            harness.name,
            String::from_utf8_lossy(&onboard.stderr)
        );
        fs::remove_dir_all(home.join(".local/state/ljos/onboard")).unwrap();

        let offboard = run(home, &bin, &["offboard", "--runner", &harness.name]);
        assert!(
            offboard.status.success(),
            "{} offboard: {}",
            harness.name,
            String::from_utf8_lossy(&offboard.stderr)
        );
        assert_eq!(files_under(home), before, "{}", harness.name);
    }
}

#[test]
fn aider_roundtrip_and_marker_fallback_restore_the_home() {
    for fallback in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let bin = fake_runner_commands(home);
        write(
            &home.join(".aider.conf.yml"),
            "model: sonnet\nread:\n  - mine.md\n",
        );
        write(&home.join("mine.md"), "mine\n");
        let before = files_under(home);

        let onboard = run(home, &bin, &["onboard", "--harness", "aider"]);
        assert!(onboard.status.success(), "{onboard:?}");
        if fallback {
            fs::remove_dir_all(home.join(".local/state/ljos/onboard")).unwrap();
        }
        let offboard = run(home, &bin, &["offboard", "--runner", "aider"]);
        assert!(offboard.status.success(), "{offboard:?}");
        assert_eq!(files_under(home), before, "fallback={fallback}");
    }
}
