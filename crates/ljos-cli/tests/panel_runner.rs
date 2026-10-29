//! Panel member launching across installed runner CLIs and configured
//! command templates, and loud failure when no runner is installed.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

struct World {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    home: PathBuf,
    root: PathBuf,
    state: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let state = dir.path().join("state");
        let home = dir.path().join("home");
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("vissue.toml"),
            "seats = [\"alice\", \"skeptic\"]\n",
        )
        .unwrap();
        std::fs::write(
            state.join("issues.json"),
            r#"{"demo-1":{"id":"demo-1","title":"Ship the feature","body":"Options: A, B\n\nA: ship\nB: wait","type":"decision"}}"#,
        )
        .unwrap();
        let state_s = state.display().to_string();
        script(
            &bin,
            "vissue",
            &format!(
                r#"#!/usr/bin/env python3
import json, os, sys
state = {state_s:?}
os.makedirs(state, exist_ok=True)
args = sys.argv[1:]
cmd = args[0] if args else ""
with open(os.path.join(state, "vissue.log"), "a") as f:
    f.write(" ".join(args) + "\n")
if cmd == "show":
    ident = args[1]
    with open(os.path.join(state, "issues.json")) as f:
        issues = json.load(f)
    meta = dict(issues.get(ident, {{"id": ident, "title": ident, "body": ""}}))
    meta.setdefault("logbook", [])
    print(json.dumps(meta))
elif cmd == "create":
    print("demo-1")
elif cmd in ("claim", "note", "append", "recall"):
    print("demo-1: ok")
elif cmd == "children":
    print("[]")
elif cmd == "vote":
    print("[]")
else:
    print("ok")
"#
            ),
        );
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\necho '{\"id\":\"00000000000000000000000000000001\"}'\n",
        );
        for util in ["sh", "env", "python3"] {
            for sys_dir in ["/usr/bin", "/bin"] {
                let candidate = Path::new(sys_dir).join(util);
                if candidate.is_file() {
                    let link = bin.join(util);
                    if !link.exists() {
                        std::os::unix::fs::symlink(&candidate, link).unwrap();
                    }
                    break;
                }
            }
        }
        Self {
            _dir: dir,
            bin,
            home,
            root,
            state,
        }
    }

    fn command(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", &self.bin)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("ISSUE_ROOT", &self.root)
            .env("VISSUE_STATE", &self.state)
            .env("VISSUE_NO_ROUTE", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_SEAT", "alice")
            .env("VISSUE_AGENT", "alice")
            .env("LJOS_PROJECT", "demo")
            .envs(env.iter().copied())
            .output()
            .unwrap()
    }
}

#[test]
fn panel_run_and_open_panel_fail_loudly_when_no_runner_cli_is_installed() {
    let world = World::new();
    let run_out = world.command(&["panel", "run", "demo-1", "--designers", "1"], &[]);
    let run_err = String::from_utf8_lossy(&run_out.stderr);
    assert!(
        !run_out.status.success(),
        "panel run succeeded with no runner: {run_err}"
    );
    assert!(
        run_err.contains("no panel runner CLI is installed on PATH"),
        "{run_err}"
    );

    let named_out = world.command(
        &[
            "panel",
            "run",
            "demo-1",
            "--designers",
            "1",
            "--harness",
            "grok",
        ],
        &[],
    );
    let named_err = String::from_utf8_lossy(&named_out.stderr);
    assert!(
        !named_out.status.success(),
        "panel run --harness grok succeeded without grok: {named_err}"
    );
    assert!(
        named_err.contains("panel runner \"grok\" is not installed on PATH"),
        "{named_err}"
    );

    let prompt = world.state.join("question.txt");
    std::fs::write(&prompt, "Should we ship the cache?").unwrap();
    let log = world.state.join("open-panel.log");
    let open_out = world.command(
        &[
            "open-panel",
            "--prompt-file",
            prompt.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
        ],
        &[],
    );
    let open_err = String::from_utf8_lossy(&open_out.stderr);
    assert!(
        !open_out.status.success(),
        "open-panel reported success with no runner: {open_err}"
    );
    assert!(
        open_err.contains("no panel runner CLI is installed on PATH"),
        "{open_err}"
    );
    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !log_text.contains("opened "),
        "log claimed members opened: {log_text}"
    );
    assert!(
        log_text.contains("no panel runner CLI is installed on PATH"),
        "{log_text}"
    );
}

#[test]
fn panel_run_detects_installed_runner_cli_and_supports_configured_templates() {
    let world = World::new();
    let marker = world.state.join("codex-called.txt");
    script(
        &world.bin,
        "codex",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
            marker.display()
        ),
    );
    let out = world.command(&["panel", "run", "demo-1", "--designers", "1"], &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.contains("(codex)"), "{stdout}");
    let recorded = std::fs::read_to_string(&marker).unwrap();
    assert!(recorded.contains("exec"), "{recorded}");
    assert!(recorded.contains("Ship the feature"), "{recorded}");

    // Now test a custom runner configured via ljos.toml [panel] command.
    let custom_marker = world.state.join("custom-called.txt");
    script(
        &world.bin,
        "bespoke-runner",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
            custom_marker.display()
        ),
    );
    let cfg_dir = world.home.join(".config/ljos");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("ljos.toml"),
        "[panel]\ncommand = [\"bespoke-runner\", \"--role\", \"{persona}\", \"--prompt\", \"{prompt}\"]\n",
    )
    .unwrap();
    let custom_out = world.command(&["panel", "run", "demo-1", "--designers", "1"], &[]);
    let custom_stdout = String::from_utf8_lossy(&custom_out.stdout);
    let custom_stderr = String::from_utf8_lossy(&custom_out.stderr);
    assert!(
        custom_out.status.success(),
        "stdout: {custom_stdout}\nstderr: {custom_stderr}"
    );
    let custom_recorded = std::fs::read_to_string(&custom_marker).unwrap();
    assert!(
        custom_recorded.contains("--role designer-1"),
        "{custom_recorded}"
    );
    assert!(
        custom_recorded.contains("Ship the feature"),
        "{custom_recorded}"
    );
}
