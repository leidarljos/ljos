//! Opencode lesson writes succeed through both the CLI hook/session path
//! and the `ljos mcp` server path against `packsetd`.

ljos_test_boot::harness!();

#[path = "common/git.rs"]
mod isolated_git;

#[path = "common/seat_env.rs"]
mod seat_env;
#[path = "common/sibling.rs"]
mod sibling;
#[path = "common/tools.rs"]
mod tools;

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn git(dir: &Path, args: &[&str]) {
    let out = isolated_git::command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn result_text(answer: &serde_json::Value) -> String {
    answer["result"]["structuredContent"]["text"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            answer["result"]["content"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()
        })
}

#[test]
fn opencode_lesson_writes_succeed_through_cli_and_mcp() {
    if tools::halt(tools::Tool::Packsetd) || tools::halt(tools::Tool::Vissue) {
        return;
    }
    let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
    let _claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));
    let pack = ljos_test_boot::Packsetd::open();

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let run = dir.path().join("run");
    let tracker = dir.path().join("tracker");
    let project = tracker.join("Software/probe");
    let work = dir.path().join("work");
    for d in [
        home.join(".config"),
        home.join(".local/state"),
        home.join(".local/share"),
        run.clone(),
        project.clone(),
        work.clone(),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        project.join("issues.org"),
        "\
#+TITLE: probe
#+VISSUE: 1
#+TODO: TODO STARTED | DONE

* TODO [#B] Record opencode lessons
:PROPERTIES:
:ID:         probe-oc01
:END:
",
    )
    .unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.name", "probe"]);
    git(&work, &["config", "user.email", "probe@example.invalid"]);
    git(&work, &["config", "commit.gpgsign", "false"]);
    git(&work, &["commit", "-q", "--allow-empty", "-m", "seed"]);

    let path = format!("{}:/usr/bin:/bin", vissue.parent().unwrap().display());
    let session = "ses_opencode_12345678";

    let mut onb_cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    onb_cmd
        .args(["onboard", "--harness", "opencode"])
        .current_dir(&work)
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("USER", "probe")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", pack.url())
        .env("PACKSET_HOME", pack.pack_home())
        .env("PACKSET_AUTH", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_USER_RULES_SYNC", "off");
    seat_env::clear(&mut onb_cmd);
    let onb = onb_cmd.output().unwrap();
    assert!(
        onb.status.success(),
        "onboard failed: {}{}",
        String::from_utf8_lossy(&onb.stdout),
        String::from_utf8_lossy(&onb.stderr)
    );
    assert!(home.join(".config/opencode/opencode.json").is_file());
    assert!(home.join(".config/opencode/plugins/ljos.ts").is_file());

    // Simulate the opencode plugin's UserPromptSubmit hook with OPENCODE_SESSION_ID.
    let mut hook_cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    hook_cmd
        .arg("hook")
        .current_dir(&work)
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("USER", "probe")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", pack.url())
        .env("PACKSET_HOME", pack.pack_home())
        .env("PACKSET_AUTH", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("OPENCODE_SESSION_ID", session)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    seat_env::clear(&mut hook_cmd);
    let mut hook_child = hook_cmd.spawn().unwrap();
    hook_child
        .stdin
        .take()
        .unwrap()
        .write_all(
            serde_json::json!({
                "session_id": session,
                "hook_event_name": "UserPromptSubmit",
                "prompt": "Keep the kiln door latched before firing.",
                "cwd": work,
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    let hook_out = hook_child.wait_with_output().unwrap();
    assert!(
        hook_out.status.success(),
        "hook failed: {}{}",
        String::from_utf8_lossy(&hook_out.stdout),
        String::from_utf8_lossy(&hook_out.stderr)
    );

    // Write a lesson via `ljos remember` with OPENCODE_SESSION_ID set (as opencode's shell.env sets it).
    let mut rem_cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    rem_cmd
        .args([
            "remember",
            "Keep the kiln door latched before firing. Never open it early. Check the gauge.",
        ])
        .current_dir(&work)
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("USER", "probe")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", pack.url())
        .env("PACKSET_HOME", pack.pack_home())
        .env("PACKSET_AUTH", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("OPENCODE_SESSION_ID", session)
        .stdin(Stdio::null());
    seat_env::clear(&mut rem_cmd);
    let rem = rem_cmd.output().unwrap();
    let rem_said = format!(
        "{}{}",
        String::from_utf8_lossy(&rem.stdout),
        String::from_utf8_lossy(&rem.stderr)
    );
    assert!(rem.status.success(), "ljos remember failed:\n{rem_said}");
    assert!(
        !rem_said.contains("POST /v1/atoms"),
        "unexpected POST /v1/atoms failure:\n{rem_said}"
    );

    // Write lessons through `ljos mcp` as opencode (start -> remember -> finish with lesson).
    let mut mcp_cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    mcp_cmd
        .current_dir(&work)
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("USER", "probe")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", pack.url())
        .env("PACKSET_HOME", pack.pack_home())
        .env("PACKSET_AUTH", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("LJOS_MCP_ORPHAN_IDLE", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    seat_env::clear(&mut mcp_cmd);
    let mut mcp = mcp_cmd.spawn().unwrap();
    let pid = mcp.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = mcp.stdin.take().unwrap();
    let mut stdout = BufReader::new(mcp.stdout.take().unwrap());

    let read_id = |stdout: &mut BufReader<_>, id: u64| {
        let mut line = String::new();
        loop {
            line.clear();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0, "eof before {id}");
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if v["id"] == id {
                return v;
            }
        }
    };

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "opencode", "version": "1.19.3"}
            }
        })
    )
    .unwrap();
    stdin.flush().unwrap();
    let init = read_id(&mut stdout, 1);
    assert!(init["result"].is_object(), "{init}");
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();

    for (id, name, args) in [
        (
            2u64,
            "ljos_start",
            serde_json::json!({"seat": "opencode", "issue": "probe-oc01", "task": "record lesson"}),
        ),
        (
            3u64,
            "ljos_remember",
            serde_json::json!({"seat": "opencode", "text": "Opencode MCP remember stores the claim cleanly."}),
        ),
        (
            4u64,
            "ljos_finish",
            serde_json::json!({
                "seat": "opencode",
                "issue": "probe-oc01",
                "lesson": "Opencode finish lesson writes without POST /v1/atoms failure."
            }),
        ),
    ] {
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": name,
                    "arguments": args,
                    "_meta": {"ai.opencode/sessionID": session}
                }
            })
        )
        .unwrap();
        stdin.flush().unwrap();
        let ans = read_id(&mut stdout, id);
        assert_ne!(ans["result"]["isError"], true, "{name} failed: {ans}");
        let text = result_text(&ans);
        assert!(
            !text.contains("POST /v1/atoms"),
            "{name} reported POST /v1/atoms failure: {text}"
        );
    }

    drop(stdin);
    drop(stdout);
    let out = mcp.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "stderr:\n{stderr}");
}
