//! A seat key comes from the seat's own config. A project MCP file,
//! CLAUDE_CONFIG_DIR, CODEX_HOME, and a parent's --mcp-config do not
//! mark a server as the seat. Runs the built `ljos` binary.
#![cfg(target_os = "linux")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

#[path = "common/seat_root.rs"]
mod seat_root;

struct Hab {
    root: PathBuf,
    home: PathBuf,
    run: PathBuf,
    repo: PathBuf,
}

impl Drop for Hab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn hab(name: &str) -> Hab {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp).join(format!(
        "m5-{name}-{}-{}",
        std::process::id(),
        nanos % 1_000_000_000
    ));
    let hab = Hab {
        home: root.join("home"),
        run: root.join("run"),
        repo: root.join("repo"),
        root,
    };
    for dir in [&hab.home, &hab.run, &hab.repo] {
        std::fs::create_dir_all(dir).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hab.run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let git = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&hab.repo)
        .status()
        .unwrap();
    assert!(git.success());
    hab
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const ALAN: &str = r#"{"rules":[{"id":"ur-ocaisa","text":"never post or reply to Alan (ocaisa)","author":"rohit","reason":"he asked not to be pinged","deny_mention":{"handles":["ocaisa"],"names":["Alan"]},"deny_reply_to":{"handles":["ocaisa"],"names":["Alan"]},"written_at":"2026-10-25T00:00:00Z","updated_at":"2026-10-25T00:00:00Z","basis":"person"}]}"#;

const FAKE: &str = r#"{"mcpServers":{"github":{"command":"ljos-mcp","args":[]}}}"#;

fn body() -> serde_json::Value {
    serde_json::json!({"owner": "o", "repo": "r", "issue_number": 1, "body": "cc @ocaisa"})
}

fn cursor(tool: &str, server_key: Option<&str>, command: Option<&str>) -> String {
    let mut v = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-probe",
        "hook_event_name": "beforeMCPExecution",
        "tool_name": tool,
        "tool_input": body().to_string(),
    });
    if let Some(server) = server_key {
        if server == "mcp_server_name" {
            v["mcp_server_name"] = serde_json::json!("github");
        } else {
            v["server"] = serde_json::json!("github");
        }
    }
    if let Some(command) = command {
        v["command"] = serde_json::json!(command);
    }
    v.to_string()
}

fn claude_note() -> String {
    serde_json::json!({
        "session_id": "s-f",
        "transcript_path": "/x",
        "cwd": "/w",
        "hook_event_name": "PreToolUse",
        "tool_name": "mcp__github__ljos_note",
        "tool_input": {"text": "cc @ocaisa"},
        "tool_use_id": "toolu_f",
    })
    .to_string()
}

struct Launch {
    extra: Vec<(String, String)>,
    cwd: Option<PathBuf>,
    /// Parent `sh` carries `--mcp-config`. The seat process is `$0 hook`.
    ancestor: Option<String>,
}

fn prepare(h: &Hab, scenario: &str) -> Launch {
    let repo = &h.repo;
    let mut launch = Launch {
        extra: Vec::new(),
        cwd: None,
        ancestor: None,
    };
    match scenario {
        "none" => {}
        "repo .mcp.json" => put(&repo.join(".mcp.json"), FAKE),
        "repo .cursor/mcp.json" => put(&repo.join(".cursor/mcp.json"), FAKE),
        "repo root .mcp.json, cwd in subdir" => {
            put(&repo.join(".mcp.json"), FAKE);
            let sub = repo.join("sub/dir");
            std::fs::create_dir_all(&sub).unwrap();
            launch.cwd = Some(sub);
        }
        "repo .mcp.json, key nested deep" => put(
            &repo.join(".mcp.json"),
            r#"{"anything":[{"deep":{"github":{"command":"ljos-mcp"}}}]}"#,
        ),
        "repo .cursor/mcp.json github = ljos mcp" => put(
            &repo.join(".cursor/mcp.json"),
            r#"{"mcpServers":{"github":{"command":"ljos","args":["mcp"]}}}"#,
        ),
        "CLAUDE_CONFIG_DIR=repo" => {
            put(&repo.join(".claude.json"), FAKE);
            launch
                .extra
                .push(("CLAUDE_CONFIG_DIR".into(), repo.display().to_string()));
        }
        "CODEX_HOME=repo" => {
            put(
                &repo.join("config.toml"),
                "[mcp_servers.github]\ncommand = \"ljos-mcp\"\n",
            );
            launch
                .extra
                .push(("CODEX_HOME".into(), repo.display().to_string()));
        }
        "CLAUDE_CONFIG_DIR plugin from repo" => {
            let root = repo.join("plug");
            put(
                &root.join(".claude-plugin/plugin.json"),
                r#"{"name":"x","mcpServers":{"github":{"command":"ljos-mcp"}}}"#,
            );
            put(
                &repo.join("cfg/plugins/installed_plugins.json"),
                &serde_json::json!({"plugins": {"x@m": [{"installPath": root}]}}).to_string(),
            );
            launch.extra.push((
                "CLAUDE_CONFIG_DIR".into(),
                repo.join("cfg").display().to_string(),
            ));
        }
        "ancestor argv --mcp-config" => launch.ancestor = Some(FAKE.into()),
        "home ~/.cursor/mcp.json real github + repo .mcp.json fake" => {
            put(
                &h.home.join(".cursor/mcp.json"),
                r#"{"mcpServers":{"github":{"url":"https://api.githubcopilot.com/mcp/"},"ljos":{"command":"ljos-mcp"}}}"#,
            );
            put(&repo.join(".mcp.json"), FAKE);
        }
        other => panic!("unknown scenario {other}"),
    }
    launch
}

fn hook(h: &Hab, launch: &Launch) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("hook");
    apply_env(&mut cmd, h, launch);
    cmd.current_dir(launch.cwd.as_deref().unwrap_or(&h.repo))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn apply_env(cmd: &mut Command, h: &Hab, launch: &Launch) {
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &h.home)
        .env("XDG_CONFIG_HOME", h.home.join(".config"))
        .env("XDG_DATA_HOME", h.home.join(".local/share"))
        .env("XDG_CACHE_HOME", h.home.join(".cache"))
        .env("XDG_STATE_HOME", h.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &h.run)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_PROJECT", "probe")
        .env("LJOS_SEAT", "probe-seat")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_USER_RULES_SYNC", "off");
    for (key, value) in &launch.extra {
        cmd.env(key, value);
    }
}

fn call(mut cmd: Command, payload: &str) -> Output {
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(payload.as_bytes()).unwrap();
    drop(stdin);
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() && started.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    child.wait_with_output().unwrap()
}

fn run_case(h: &Hab, launch: &Launch, payload: &str) -> Output {
    if let Some(json) = &launch.ancestor {
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "\"$0\" hook; rc=$?; exit $rc",
            env!("CARGO_BIN_EXE_ljos"),
            "--mcp-config",
            json,
        ]);
        apply_env(&mut cmd, h, launch);
        cmd.current_dir(launch.cwd.as_deref().unwrap_or(&h.repo))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        return call(cmd, payload);
    }
    call(hook(h, launch), payload)
}

fn denies(out: &Output, label: &str) {
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("deny"),
        "{label} allowed the forged seat: {stdout} stderr {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn payloads() -> Vec<(&'static str, String)> {
    vec![
        (
            "cursor beforeMCP mcp__github__ljos_issue_comment",
            cursor(
                "mcp__github__ljos_issue_comment",
                None,
                Some("github-mcp-server"),
            ),
        ),
        (
            "cursor beforeMCP ljos_add_issue_comment server=github",
            cursor("ljos_add_issue_comment", Some("server"), None),
        ),
        (
            "cursor beforeMCP ljos_add_issue_comment mcp_server_name=github",
            cursor("ljos_add_issue_comment", Some("mcp_server_name"), None),
        ),
    ]
}

const SCENARIOS: &[&str] = &[
    "none",
    "repo .mcp.json",
    "repo .cursor/mcp.json",
    "repo root .mcp.json, cwd in subdir",
    "repo .mcp.json, key nested deep",
    "repo .cursor/mcp.json github = ljos mcp",
    "CLAUDE_CONFIG_DIR=repo",
    "CODEX_HOME=repo",
    "ancestor argv --mcp-config",
    "home ~/.cursor/mcp.json real github + repo .mcp.json fake",
];

/// The 27 cursor payloads from a foreign seat layout, plus the control
/// with no forged file. Each one names @ocaisa and is denied.
#[test]
fn a_forged_seat_key_does_not_skip_the_user_rule() {
    let mut missed = Vec::new();
    for scenario in SCENARIOS {
        if *scenario == "none" {
            continue;
        }
        for (name, payload) in payloads() {
            let h = hab("fake");
            put(&h.home.join(".config/ljos/user-rules.json"), ALAN);
            let launch = prepare(&h, scenario);
            let out = run_case(&h, &launch, &payload);
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            if !stdout.contains("deny") {
                missed.push(format!("{scenario} / {name}: {stdout}"));
            }
        }
    }
    assert!(
        missed.is_empty(),
        "forged seat allowed:\n{}",
        missed.join("\n")
    );
}

/// No forged file. The same three payloads are denied from the rule alone.
#[test]
fn a_foreign_ljos_tool_is_denied_with_no_seat_file() {
    for (name, payload) in payloads() {
        let h = hab("none");
        put(&h.home.join(".config/ljos/user-rules.json"), ALAN);
        let launch = prepare(&h, "none");
        let out = run_case(&h, &launch, &payload);
        denies(&out, name);
    }
}

/// A plugin installed under CLAUDE_CONFIG_DIR is that process's choice.
/// It does not name the seat.
#[test]
fn a_plugin_under_claude_config_dir_does_not_name_the_seat() {
    let h = hab("plug");
    put(&h.home.join(".config/ljos/user-rules.json"), ALAN);
    let launch = prepare(&h, "CLAUDE_CONFIG_DIR plugin from repo");
    let payload = serde_json::json!({
        "session_id": "s-f",
        "transcript_path": "/x",
        "cwd": "/w",
        "hook_event_name": "PreToolUse",
        "tool_name": "mcp__plugin_x_github__ljos_issue_comment",
        "tool_input": {"body": "cc @ocaisa"},
        "tool_use_id": "toolu_p",
    })
    .to_string();
    denies(&run_case(&h, &launch, &payload), "plugin");
}

/// Malformed user rules. The three sources Codex forged still deny
/// `mcp__github__ljos_note` instead of taking the early allow.
#[test]
fn a_malformed_rules_file_still_denies_a_forged_seat() {
    let sources = [
        "CODEX_HOME=repo",
        "CLAUDE_CONFIG_DIR=repo",
        "ancestor argv --mcp-config",
    ];
    for scenario in sources {
        let h = hab("malformed");
        put(&h.home.join(".config/ljos/user-rules.json"), "");
        let launch = prepare(&h, scenario);
        let out = run_case(&h, &launch, &claude_note());
        denies(&out, scenario);
    }
}

/// The seat's own `~/.cursor/mcp.json` still answers `mcp__ljos__ljos_note`
/// early. A project file beside it does not change that.
#[test]
fn the_home_cursor_file_still_names_the_seat() {
    let h = hab("home");
    put(
        &h.home.join(".cursor/mcp.json"),
        r#"{"mcpServers":{"ljos":{"command":"ljos-mcp","args":[]}}}"#,
    );
    put(&h.run.join("ljos/hook-trace"), "");
    let payload = serde_json::json!({
        "cursor_version": "2.4.0",
        "conversation_id": "c-home",
        "hook_event_name": "beforeMCPExecution",
        "tool_name": "mcp__ljos__ljos_note",
        "command": "ljos-mcp",
        "tool_input": {"issue": "demo-1", "text": "progress"},
    })
    .to_string();
    let launch = Launch {
        extra: Vec::new(),
        cwd: None,
        ancestor: None,
    };
    let out = run_case(&h, &launch, &payload);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("deny"),
        "the seat's own tool was refused: {stdout}"
    );
}
