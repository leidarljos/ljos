//! One command per runner writes that runner's config into a fresh home.
//!
//! Claude Code, Cursor and Codex each take `ljos onboard --harness NAME`.
//! The command writes the file that runner reads, leaves every other key,
//! prints the path and `ljos doctor`, and a second run does not change
//! those bytes. Claude Code does not need the `claude` program.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
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
            "#!/bin/sh\n[ \"$1\" = ensure ] && echo writer up && exit 0\necho \"packset stub: $*\" >&2\nexit 1\n",
        );
        script(
            &bin,
            "deedar",
            "#!/bin/sh\n[ \"$1 $2\" = \"host accept\" ] && echo 'host key listed' && exit 0\necho \"deedar stub: $*\" >&2\nexit 1\n",
        );
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn ljos(&self, args: &[&str]) -> Output {
        let home = self.path();
        let bins = Path::new(env!("CARGO_BIN_EXE_ljos")).parent().unwrap();
        let path = std::env::join_paths([
            home.join("bin"),
            bins.to_path_buf(),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ])
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("HOME", home)
            .env("PATH", path)
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
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn onboard(home: &Home, runner: &str) -> String {
    let out = home.ljos(&["onboard", "--harness", runner]);
    let text = said(&out);
    assert!(out.status.success(), "{runner}: {text}");
    assert!(
        text.contains("so run `ljos doctor --all`"),
        "{runner} did not name the next command: {text}"
    );
    text
}

/// Claude Code stores user MCP servers in `~/.claude.json`.
/// `~/.claude/mcp.json` is not a file Claude Code reads.
#[test]
fn claude_code_one_command_writes_claude_json() {
    let home = Home::new();
    let config = home.path().join(".claude.json");
    write(
        &config,
        "{\n  \"userID\": \"keep-me\",\n  \"mcpServers\": {\n    \"other\": {\"command\": \"keep-me\"}\n  }\n}\n",
    );
    let said = onboard(&home, "claude");
    assert!(said.contains(config.to_str().unwrap()), "{said}");
    assert!(
        !home.path().join("bin").join("claude").exists(),
        "the command required the claude program"
    );
    let mcp = text(&config);
    let doc: serde_json::Value = serde_json::from_str(&mcp).unwrap();
    assert_eq!(doc["userID"], "keep-me", "{mcp}");
    assert_eq!(doc["mcpServers"]["other"]["command"], "keep-me", "{mcp}");
    assert_eq!(doc["mcpServers"]["ljos"]["type"], "stdio", "{mcp}");
    assert!(
        doc["mcpServers"]["ljos"]["command"]
            .as_str()
            .is_some_and(|c| c == "ljos-mcp" || c.ends_with("/ljos-mcp")),
        "{mcp}"
    );
    assert!(text(&home.path().join(".claude/settings.json")).contains("ljos"));
    assert!(
        text(&home.path().join(".claude/skills/ljos/SKILL.md")).starts_with("---\nname: ljos\n")
    );
    assert!(
        !home.path().join(".claude/mcp.json").exists(),
        "MCP config was written where Claude Code does not read it"
    );

    let snap = [
        config.clone(),
        home.path().join(".claude/settings.json"),
        home.path().join(".claude/skills/ljos/SKILL.md"),
    ]
    .into_iter()
    .map(|p| (p.clone(), bytes(&p)))
    .collect::<Vec<_>>();
    let again = onboard(&home, "claude");
    assert!(again.contains("ljos registered"), "{again}");
    for (path, before) in snap {
        assert_eq!(
            bytes(&path),
            before,
            "second run rewrote {}",
            path.display()
        );
    }
}

/// Cursor's IDE and its agent command read `~/.cursor/mcp.json`.
#[test]
fn cursor_one_command_writes_mcp_json() {
    let home = Home::new();
    let config = home.path().join(".cursor/mcp.json");
    write(
        &config,
        "{\n  \"note\": \"leave this\",\n  \"mcpServers\": {\n    \"other\": {\"command\": \"keep-me\"}\n  }\n}\n",
    );
    let said = onboard(&home, "cursor");
    assert!(said.contains(config.to_str().unwrap()), "{said}");
    let mcp = text(&config);
    let doc: serde_json::Value = serde_json::from_str(&mcp).unwrap();
    assert_eq!(doc["note"], "leave this", "{mcp}");
    assert_eq!(doc["mcpServers"]["other"]["command"], "keep-me", "{mcp}");
    assert_eq!(doc["mcpServers"]["ljos"]["type"], "stdio", "{mcp}");
    assert!(
        doc["mcpServers"]["ljos"]["command"]
            .as_str()
            .is_some_and(|c| c.contains("ljos-mcp")),
        "{mcp}"
    );
    assert!(text(&home.path().join(".cursor/hooks.json")).contains("ljos"));
    assert!(
        text(&home.path().join(".cursor/skills/ljos/SKILL.md")).starts_with("---\nname: ljos\n")
    );

    let snap = [
        config.clone(),
        home.path().join(".cursor/hooks.json"),
        home.path().join(".cursor/skills/ljos/SKILL.md"),
    ]
    .into_iter()
    .map(|p| (p.clone(), bytes(&p)))
    .collect::<Vec<_>>();
    let again = onboard(&home, "cursor");
    assert!(again.contains("ljos registered"), "{again}");
    for (path, before) in snap {
        assert_eq!(
            bytes(&path),
            before,
            "second run rewrote {}",
            path.display()
        );
    }
}

/// Codex reads MCP servers from `~/.codex/config.toml` and skills from
/// `~/.agents/skills`. `~/.codex/skills` is the old directory.
#[test]
fn codex_one_command_writes_config_toml_and_agents_skills() {
    let home = Home::new();
    let config = home.path().join(".codex/config.toml");
    write(
        &config,
        "model = \"keep-me\"\n\n[mcp_servers.other]\ncommand = \"keep-me\"\n",
    );
    let said = onboard(&home, "codex");
    assert!(said.contains(config.to_str().unwrap()), "{said}");
    let toml = text(&config);
    assert!(toml.contains("model = \"keep-me\""), "{toml}");
    assert!(toml.contains("[mcp_servers.other]"), "{toml}");
    assert!(toml.contains("command = \"keep-me\""), "{toml}");
    assert!(toml.contains("[mcp_servers.ljos]"), "{toml}");
    assert!(toml.contains("ljos-mcp"), "{toml}");
    assert!(text(&home.path().join(".codex/hooks.json")).contains("ljos"));
    assert!(
        text(&home.path().join(".agents/skills/ljos/SKILL.md")).starts_with("---\nname: ljos\n")
    );
    assert!(
        !home.path().join(".codex/skills/ljos/SKILL.md").exists(),
        "the skill was written to the old Codex directory"
    );

    let snap = [
        config.clone(),
        home.path().join(".codex/hooks.json"),
        home.path().join(".agents/skills/ljos/SKILL.md"),
    ]
    .into_iter()
    .map(|p| (p.clone(), bytes(&p)))
    .collect::<Vec<_>>();
    let again = onboard(&home, "codex");
    assert!(again.contains("ljos registered"), "{again}");
    for (path, before) in snap {
        assert_eq!(
            bytes(&path),
            before,
            "second run rewrote {}",
            path.display()
        );
    }
}
