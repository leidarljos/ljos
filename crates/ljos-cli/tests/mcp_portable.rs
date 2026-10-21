//! A harness config written under one home has to start under another.
//!
//! Cursor syncs `mcp.json` onto a cloud VM. An absolute path from the
//! machine that wrote it (`/home/box/.cargo/bin/ljos-mcp`) is not there,
//! so the client discovers no tools.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn exe(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ljos")).with_file_name(name)
}

fn install_pair(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for name in ["ljos", "ljos-mcp"] {
        let dest = dir.join(name);
        fs::copy(exe(name), &dest).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn stub_claude(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        path,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/claude-argv\"\ncase \"$*\" in *'mcp get'*) exit 1 ;; esac\nexit 0\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn stub(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, "#!/bin/sh\nprintf ran > \"$1\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn onboard_with_path(ljos: &Path, home: &Path, path: &str, args: &[&str]) -> std::process::Output {
    let tracker = home.join("tracker");
    fs::create_dir_all(tracker.join("Software")).unwrap();
    fs::create_dir_all(home.join(".local/share")).unwrap();
    fs::create_dir_all(home.join("run")).unwrap();
    Command::new(ljos)
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PACKSET_URL", "off")
        .env("DEEDAR_HOST_SIGNING_KEY", "off")
        .env(
            "DEEDAR_URL",
            format!("file://{}", home.join("deeds").display()),
        )
        .env("VISSUE_ROOT", &tracker)
        .env("ISSUE_ROOT", &tracker)
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_CONFIG_SYSTEM", home.join("gitconfig"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap()
}

fn onboard(ljos: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    onboard_with_path(
        ljos,
        home,
        &format!("{}:/usr/bin:/bin", ljos.parent().unwrap().display()),
        args,
    )
}

fn said(out: &std::process::Output) -> String {
    format!(
        "status {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn json_command(path: &Path) -> (String, Vec<String>) {
    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let entry = &doc["mcpServers"]["ljos"];
    let command = entry["command"].as_str().unwrap_or("").to_string();
    let args = entry
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    (command, args)
}

fn toml_command(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find_map(|line| {
            let rest = line.strip_prefix("command")?.trim_start();
            let rest = rest.strip_prefix('=')?.trim();
            let quote = rest.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            rest[1..].split(quote).next().map(str::to_string)
        })
        .unwrap_or_default()
}

/// Run `command` as a harness would, under `home`, with only `bin` on PATH
/// ahead of the system directories. The config bytes are not rewritten.
fn launch(command: &str, args: &[String], home: &Path, bin: &Path) -> std::process::Output {
    Command::new(command)
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap()
}

#[test]
fn a_config_written_under_one_home_runs_under_another() {
    let root = tempfile::tempdir().unwrap();
    let home_a = root.path().join("home-a");
    let bin_a = home_a.join(".cargo/bin");
    install_pair(&bin_a);
    let ljos = bin_a.join("ljos");

    let out = onboard(&ljos, &home_a, &["onboard", "--harness", "cursor"]);
    assert!(out.status.success(), "{}", said(&out));
    let cursor = home_a.join(".cursor/mcp.json");
    let written = fs::read(cursor).unwrap();
    let (command, args) = json_command(&home_a.join(".cursor/mcp.json"));
    assert!(
        !command.contains(home_a.to_str().unwrap()),
        "command names the home that wrote it: {command}"
    );
    assert!(
        !Path::new(&command).is_absolute(),
        "command is an absolute path: {command}"
    );

    let out = onboard(&ljos, &home_a, &["onboard", "--harness", "codex"]);
    assert!(out.status.success(), "{}", said(&out));
    let codex = fs::read_to_string(home_a.join(".codex/config.toml")).unwrap();
    let codex_command = toml_command(&codex);
    assert_eq!(codex_command, command, "{codex}");

    // Claude stores the command it is given. A stub records that argv.
    stub_claude(&bin_a.join("claude"));
    let out = onboard(&ljos, &home_a, &["onboard", "--harness", "claude"]);
    assert!(out.status.success(), "{}", said(&out));
    let argv = fs::read_to_string(home_a.join("claude-argv")).unwrap();
    assert!(
        argv.contains("ljos-mcp") && !argv.contains(home_a.to_str().unwrap()),
        "{argv}"
    );

    let out = onboard(&ljos, &home_a, &["onboard", "--harness", "json"]);
    assert!(out.status.success(), "{}", said(&out));
    let pasted: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(pasted["mcpServers"]["ljos"]["command"], command);

    // The file is used as written. The binary from home A is gone, which
    // is the cloud VM: that directory does not exist there.
    fs::remove_dir_all(&bin_a).unwrap();
    let home_b = root.path().join("home-b");
    let bin_b = home_b.join("bin");
    let marker = home_b.join("ran");
    stub(&bin_b.join("ljos-mcp"));
    fs::create_dir_all(home_b.join(".cursor")).unwrap();
    fs::write(home_b.join(".cursor/mcp.json"), &written).unwrap();
    assert_eq!(fs::read(home_b.join(".cursor/mcp.json")).unwrap(), written);

    let mut args = args;
    args.push(marker.display().to_string());
    let ran = launch(&command, &args, &home_b, &bin_b);
    assert!(ran.status.success(), "{}", said(&ran));
    assert_eq!(fs::read_to_string(&marker).unwrap(), "ran");
}

#[test]
fn onboard_replaces_an_absolute_server_path() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".cargo/bin");
    install_pair(&bin);
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(
        home.join(".cursor/mcp.json"),
        "{\n  \"mcpServers\": {\n    \"ljos\": {\n      \"command\": \"/home/box/.cargo/bin/ljos-mcp\",\n      \"args\": []\n    }\n  }\n}\n",
    )
    .unwrap();
    let out = onboard(
        &bin.join("ljos"),
        &home,
        &["onboard", "--harness", "cursor"],
    );
    assert!(out.status.success(), "{}", said(&out));
    let (command, _) = json_command(&home.join(".cursor/mcp.json"));
    assert_eq!(command, "ljos-mcp");
    assert!(!command.contains("/home/box"), "{command}");
}

#[test]
fn onboard_keeps_an_absolute_server_path_that_exists_here() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".cargo/bin");
    install_pair(&bin);
    fs::create_dir_all(home.join(".codex")).unwrap();
    let installed = bin.join("ljos-mcp");
    fs::write(
        home.join(".codex/config.toml"),
        format!(
            "[mcp_servers.ljos]\ncommand = \"{}\"\nargs = []\n",
            installed.display()
        ),
    )
    .unwrap();
    let out = onboard_with_path(
        &bin.join("ljos"),
        &home,
        "/usr/bin:/bin",
        &["onboard", "--harness", "codex"],
    );
    assert!(out.status.success(), "{}", said(&out));
    let text = fs::read_to_string(home.join(".codex/config.toml")).unwrap();
    assert!(
        text.contains(&format!("command = \"{}\"", installed.display())),
        "{text}"
    );
}

#[test]
fn onboard_writes_an_absolute_command_when_path_cannot_find_ljos_mcp() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".cargo/bin");
    install_pair(&bin);
    let out = onboard_with_path(
        &bin.join("ljos"),
        &home,
        "/usr/bin:/bin",
        &["onboard", "--harness", "cursor"],
    );
    assert!(out.status.success(), "{}", said(&out));
    let (command, _) = json_command(&home.join(".cursor/mcp.json"));
    assert_eq!(command, bin.join("ljos-mcp").display().to_string());
    let output = said(&out);
    assert!(
        output.contains("PATH does not resolve ljos-mcp"),
        "{output}"
    );

    let out = onboard_with_path(
        &bin.join("ljos"),
        &home,
        "/usr/bin:/bin",
        &["onboard", "--harness", "json"],
    );
    assert!(out.status.success(), "{}", said(&out));
    let pasted: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        pasted["mcpServers"]["ljos"]["command"],
        bin.join("ljos-mcp").display().to_string()
    );
}

#[test]
fn onboard_writes_an_absolute_command_when_path_points_to_another_install() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".cargo/bin");
    install_pair(&bin);
    let stale = root.path().join("stale/bin");
    stub(&stale.join("ljos-mcp"));
    let path = format!("{}:{}:/usr/bin:/bin", stale.display(), bin.display());
    let out = onboard_with_path(
        &bin.join("ljos"),
        &home,
        &path,
        &["onboard", "--harness", "cursor"],
    );
    assert!(out.status.success(), "{}", said(&out));
    let (command, _) = json_command(&home.join(".cursor/mcp.json"));
    assert_eq!(
        command,
        bin.join("ljos-mcp").display().to_string(),
        "{command}"
    );
    let said = said(&out);
    assert!(
        said.contains(&format!(
            "PATH resolves ljos-mcp to {}/ljos-mcp",
            stale.display()
        )),
        "{said}"
    );
}

#[test]
fn onboard_rewrites_only_the_ljos_entry_in_toml() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".cargo/bin");
    install_pair(&bin);
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.ljos]\ncommand = \"/home/box/.cargo/bin/ljos-mcp\"\nargs = []\n\n[mcp_servers.ljos-dev]\ncommand = \"/opt/pinned/ljos-mcp\"\nargs = []\n# keep /home/box/.cargo/bin/ljos-mcp in this comment\n",
    )
    .unwrap();
    let out = onboard(&bin.join("ljos"), &home, &["onboard", "--harness", "codex"]);
    assert!(out.status.success(), "{}", said(&out));
    let text = fs::read_to_string(home.join(".codex/config.toml")).unwrap();
    assert!(
        text.contains("[mcp_servers.ljos]\ncommand = \"ljos-mcp\""),
        "{text}"
    );
    assert!(
        text.contains("[mcp_servers.ljos-dev]\ncommand = \"/opt/pinned/ljos-mcp\""),
        "{text}"
    );
    assert!(
        text.contains("# keep /home/box/.cargo/bin/ljos-mcp in this comment"),
        "{text}"
    );
}

#[test]
fn onboard_does_not_rewrite_a_sibling_hermes_command() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".cargo/bin");
    install_pair(&bin);
    fs::create_dir_all(home.join(".hermes")).unwrap();
    fs::write(
        home.join(".hermes/config.yaml"),
        "mcp_servers:\n  ljos-dev:\n    command: /opt/pinned/ljos-mcp\n  ljos:\n    command: /home/box/.cargo/bin/ljos-mcp\n",
    )
    .unwrap();
    let out = onboard(
        &bin.join("ljos"),
        &home,
        &["onboard", "--harness", "hermes"],
    );
    assert!(out.status.success(), "{}", said(&out));
    let text = fs::read_to_string(home.join(".hermes/config.yaml")).unwrap();
    assert!(
        text.contains("  ljos-dev:\n    command: /opt/pinned/ljos-mcp"),
        "{text}"
    );
    assert!(text.contains("  ljos:\n    command: ljos-mcp"), "{text}");
}
