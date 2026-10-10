//! Hook setups for runners whose hook contract is not Claude Code's own:
//! GitHub Copilot, Gemini CLI, Windsurf (Devin Desktop), Factory Droid,
//! Kiro, Cline, Qwen Code and Crush.
//!
//! `onboard` writes each runner's hook file with the command
//! `ljos hook --runner NAME`. That command turns the runner's stdin into
//! the Claude Code payload the seat reads, runs the ordinary hook, and
//! writes the answer back in the runner's own contract: its field names,
//! and for some runners an exit code. A runner that cannot ask the person
//! gets a deny in place of an ask, with the reason saying so.

use serde_json::{json, Map, Value};
use std::path::Path;

use crate::Step;

/// The runners this module serves, by the name `--runner` takes.
pub const RUNNERS: &[&str] = &[
    "copilot", "gemini", "windsurf", "factory", "kiro", "cline", "qwen", "crush",
];

/// Whether `name` is a runner this module writes hooks for.
#[must_use]
pub fn is_runner(name: &str) -> bool {
    RUNNERS.contains(&name)
}

/// Whether the runner shows the person a prompt on an `ask`.
fn asks(runner: &str) -> bool {
    matches!(runner, "copilot" | "factory" | "qwen")
}

/// The event names Gemini CLI uses, as the seat's.
fn gemini_event(raw: &str) -> &str {
    match raw {
        "BeforeTool" => "PreToolUse",
        "AfterTool" => "PostToolUse",
        "BeforeAgent" => "UserPromptSubmit",
        other => other,
    }
}

/// A command line out of a tool's arguments: `command`, or a list of
/// `commands`, one to a line.
fn command_of(args: &Value) -> Option<String> {
    if let Some(c) = args["command"].as_str() {
        return Some(c.to_string());
    }
    args["commands"].as_array().map(|list| {
        list.iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// The runner's stdin as the Claude Code payload `ljos hook` reads:
/// `hook_event_name`, `session_id`, `cwd`, and `tool_name` with
/// `tool_input`, or `prompt`. A payload that is not JSON, or a runner
/// that already sends this shape, is returned as it came.
#[must_use]
pub fn normalize_input(runner: &str, input: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(input.trim()) else {
        return input.to_string();
    };
    let mut out = Map::new();
    let put = |out: &mut Map<String, Value>, k: &str, val: Option<&str>| {
        if let Some(s) = val.filter(|s| !s.is_empty()) {
            out.insert(k.into(), Value::String(s.to_string()));
        }
    };
    match runner {
        "windsurf" => {
            let action = v["agent_action_name"].as_str().unwrap_or("");
            let info = &v["tool_info"];
            let event = match action {
                "pre_run_command" => "PreToolUse",
                "post_run_command" => "PostToolUse",
                "pre_user_prompt" => "UserPromptSubmit",
                other => other,
            };
            out.insert("hook_event_name".into(), json!(event));
            put(&mut out, "session_id", v["trajectory_id"].as_str());
            put(&mut out, "cwd", info["cwd"].as_str());
            if let Some(line) = info["command_line"].as_str() {
                out.insert("tool_name".into(), json!("Bash"));
                out.insert("tool_input".into(), json!({ "command": line }));
            }
            put(&mut out, "prompt", info["user_prompt"].as_str());
        }
        "cline" => {
            put(&mut out, "session_id", v["taskId"].as_str());
            put(&mut out, "cwd", v["workspaceRoots"][0].as_str());
            if let Some(pre) = v.get("preToolUse").filter(|p| p.is_object()) {
                let tool = pre["toolName"].as_str().or_else(|| pre["tool"].as_str());
                out.insert("hook_event_name".into(), json!("PreToolUse"));
                put(&mut out, "tool_name", tool);
                let args = &pre["parameters"];
                match command_of(args) {
                    Some(c) => {
                        out.insert("tool_input".into(), json!({ "command": c }));
                    }
                    None => {
                        out.insert("tool_input".into(), args.clone());
                    }
                }
            } else if let Some(call) = v.get("tool_call").filter(|p| p.is_object()) {
                out.insert("hook_event_name".into(), json!("PreToolUse"));
                put(&mut out, "tool_name", call["name"].as_str());
                let args = &call["input"];
                match command_of(args) {
                    Some(c) => {
                        out.insert("tool_input".into(), json!({ "command": c }));
                    }
                    None => {
                        out.insert("tool_input".into(), args.clone());
                    }
                }
            } else if let Some(p) = v["userPromptSubmit"]["prompt"].as_str() {
                out.insert("hook_event_name".into(), json!("UserPromptSubmit"));
                out.insert("prompt".into(), json!(p));
            } else {
                return input.to_string();
            }
        }
        "copilot" if v.get("toolName").is_some() || v.get("sessionId").is_some() => {
            // The camelCase contract: `toolArgs` may arrive as JSON text.
            let event = if v.get("toolName").is_some() {
                "PreToolUse"
            } else if v.get("prompt").is_some() {
                "UserPromptSubmit"
            } else {
                "SessionStart"
            };
            out.insert("hook_event_name".into(), json!(event));
            put(&mut out, "session_id", v["sessionId"].as_str());
            put(&mut out, "cwd", v["cwd"].as_str());
            put(&mut out, "tool_name", v["toolName"].as_str());
            put(&mut out, "prompt", v["prompt"].as_str());
            let args = match &v["toolArgs"] {
                Value::String(s) => serde_json::from_str(s).unwrap_or(json!({ "command": s })),
                other => other.clone(),
            };
            if !args.is_null() {
                out.insert("tool_input".into(), args);
            }
        }
        "gemini" => {
            let mut v = v;
            if let Some(e) = v["hook_event_name"].as_str() {
                let seat = gemini_event(e).to_string();
                v["hook_event_name"] = Value::String(seat);
            }
            return v.to_string();
        }
        "crush" if v.get("hook_event_name").is_none() => {
            let mut v = v;
            let event = v["event"].as_str().unwrap_or("PreToolUse").to_string();
            v["hook_event_name"] = Value::String(event);
            return v.to_string();
        }
        _ => return input.to_string(),
    }
    Value::Object(out).to_string()
}

/// What the seat's answer says, whatever shape it came in.
#[derive(Debug, Default, PartialEq, Eq)]
struct Said {
    decision: Option<String>,
    reason: String,
    context: String,
}

fn read_answer(out: &str) -> Said {
    let trimmed = out.trim();
    let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
        // A plain-text answer is an argv line; a deny says so first.
        if trimmed.starts_with("deny:") {
            return Said {
                decision: Some("deny".into()),
                reason: trimmed.trim_start_matches("deny:").trim().to_string(),
                context: String::new(),
            };
        }
        return Said {
            context: trimmed.to_string(),
            ..Said::default()
        };
    };
    let specific = &v["hookSpecificOutput"];
    let decision = specific["permissionDecision"]
        .as_str()
        .or_else(|| v["decision"].as_str())
        .map(|d| if d == "block" { "deny" } else { d })
        .filter(|d| matches!(*d, "deny" | "ask"))
        .map(str::to_string);
    let reason = specific["permissionDecisionReason"]
        .as_str()
        .or_else(|| v["reason"].as_str())
        .unwrap_or("")
        .to_string();
    let context = specific["additionalContext"]
        .as_str()
        .or_else(|| v["context"].as_str())
        .unwrap_or("")
        .to_string();
    Said {
        decision,
        reason,
        context,
    }
}

/// The seat's answer (the Claude Code JSON `ljos hook` prints) in the
/// runner's contract: what to print, what to write to stderr, and the
/// exit code. `event` is the event name the runner sent.
#[must_use]
pub fn answer(runner: &str, event: &str, out: &str) -> (String, String, i32) {
    let mut said = read_answer(out);
    if said.decision.as_deref() == Some("ask") && !asks(runner) {
        said.decision = Some("deny".into());
        said.reason = format!(
            "ask the person before running this: {}. This runner cannot ask, so retrying \
             returns this same refusal: stop, tell the person the exact command, and leave \
             it for them to run.",
            said.reason
        );
    }
    let line = |v: Value| v.to_string() + "\n";
    match (runner, said.decision.as_deref()) {
        ("factory" | "qwen", _) => (out.to_string(), String::new(), 0),
        ("copilot", Some(d)) => (
            line(json!({
                "permissionDecision": d,
                "permissionDecisionReason": said.reason,
            })),
            String::new(),
            0,
        ),
        ("copilot", None) => (String::new(), String::new(), 0),
        ("gemini", d) => {
            let mut v = Map::new();
            if let Some(d) = d {
                v.insert("decision".into(), json!(d));
                v.insert("reason".into(), json!(said.reason));
            }
            if !said.context.is_empty() {
                v.insert(
                    "hookSpecificOutput".into(),
                    json!({ "hookEventName": event, "additionalContext": said.context }),
                );
            }
            (line(Value::Object(v)), String::new(), 0)
        }
        ("windsurf" | "kiro", Some(_)) => (String::new(), said.reason + "\n", 2),
        ("kiro", None) if !said.context.is_empty() => (said.context + "\n", String::new(), 0),
        ("windsurf" | "kiro", None) => (String::new(), String::new(), 0),
        ("cline", Some(_)) => (
            line(json!({ "cancel": true, "errorMessage": said.reason })),
            String::new(),
            0,
        ),
        ("cline", None) => {
            let mut v = json!({ "cancel": false });
            if !said.context.is_empty() {
                v["contextModification"] = json!(said.context);
            }
            (line(v), String::new(), 0)
        }
        ("crush", Some(d)) => (
            line(json!({ "decision": d, "reason": said.reason })),
            String::new(),
            0,
        ),
        ("crush", None) if !said.context.is_empty() => {
            (line(json!({ "context": said.context })), String::new(), 0)
        }
        _ => (String::new(), String::new(), 0),
    }
}

/// The event name in a runner's stdin, as the runner spelled it.
#[must_use]
pub fn raw_event(input: &str) -> String {
    serde_json::from_str::<Value>(input.trim())
        .ok()
        .and_then(|v| {
            ["hook_event_name", "agent_action_name", "hookName", "event"]
                .iter()
                .find_map(|k| v[*k].as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

/// Run the seat's hook for a runner: `ljos hook` on the normalized
/// payload, its answer put in the runner's contract. Returns the exit
/// code the runner reads.
pub fn relay(runner: &str, input: &str, limit: usize, event: Option<&str>) -> anyhow::Result<i32> {
    use std::io::Write;
    let normalized = normalize_input(runner, input);
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("hook").arg("--limit").arg(limit.to_string());
    if let Some(e) = event {
        cmd.arg("--event").arg(e);
    }
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(normalized.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let (out, err, code) = answer(runner, &raw_event(input), &text);
    print!("{out}");
    eprint!("{err}");
    Ok(code)
}

/// The hook command a runner's file names.
#[must_use]
pub fn runner_command(base: &str, runner: &str) -> String {
    format!("{base} --runner {runner}")
}

fn ours(v: &Value, runner: &str) -> bool {
    v.to_string().contains(&format!("--runner {runner}"))
}

/// Push `item` onto the array at `key` of `obj` unless one already names
/// the seat's command. True when it was added.
fn ensure_in(obj: &mut Map<String, Value>, key: &str, item: Value, runner: &str) -> bool {
    let list = obj.entry(key.to_string()).or_insert_with(|| json!([]));
    let Some(list) = list.as_array_mut() else {
        return false;
    };
    if list.iter().any(|g| ours(g, runner)) {
        return false;
    }
    list.push(item);
    true
}

fn group(matcher: Option<&str>, command: &str, timeout: u64, runner: &str) -> Value {
    let mut hook = json!({ "type": "command", "command": command, "timeout": timeout });
    if runner == "gemini" {
        hook["name"] = json!("ljos");
    }
    let mut g = json!({ "hooks": [hook] });
    if let Some(m) = matcher {
        g["matcher"] = json!(m);
    }
    g
}

fn hooks_of(obj: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, String> {
    obj.entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "hooks is not an object".to_string())
}

/// Merge or write the runner's hook file so it runs `command`. A file
/// the seat owns whole (Copilot's and Kiro's) is rewritten; a shared
/// settings file keeps every entry that is not the seat's.
fn spec(runner: &str, root: &mut Value, command: &str) -> Result<bool, String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "not a JSON object".to_string())?;
    let changed = match runner {
        "copilot" => {
            let want = json!({
                "version": 1,
                "hooks": { "PreToolUse": [{
                    "type": "command", "matcher": "Bash", "bash": command, "timeoutSec": 20
                }]}
            });
            let same = Value::Object(obj.clone()) == want;
            *obj = want.as_object().cloned().unwrap_or_default();
            !same
        }
        "kiro" => {
            let want = json!({
                "version": "v1",
                "hooks": [{
                    "name": "ljos guard",
                    "description": "The seat's guard on shell commands; written by ljos onboard.",
                    "trigger": "PreToolUse",
                    "matcher": "shell",
                    "action": { "type": "command", "command": command },
                    "timeout": 20
                }]
            });
            let same = Value::Object(obj.clone()) == want;
            *obj = want.as_object().cloned().unwrap_or_default();
            !same
        }
        "gemini" => {
            let hooks = hooks_of(obj)?;
            let a = ensure_in(
                hooks,
                "BeforeTool",
                group(Some("run_shell_command"), command, 20_000, runner),
                runner,
            );
            let b = ensure_in(
                hooks,
                "BeforeAgent",
                group(None, command, 15_000, runner),
                runner,
            );
            a || b
        }
        "qwen" => {
            let hooks = hooks_of(obj)?;
            let a = ensure_in(
                hooks,
                "PreToolUse",
                group(Some("^run_shell_command$"), command, 20, runner),
                runner,
            );
            let b = ensure_in(
                hooks,
                "UserPromptSubmit",
                group(None, command, 15, runner),
                runner,
            );
            a || b
        }
        "factory" => {
            // hooks.json is keyed by event at the top level.
            let a = ensure_in(
                obj,
                "PreToolUse",
                group(Some("Execute"), command, 20, runner),
                runner,
            );
            let b = ensure_in(
                obj,
                "UserPromptSubmit",
                group(None, command, 15, runner),
                runner,
            );
            a || b
        }
        "windsurf" => {
            let hooks = hooks_of(obj)?;
            ensure_in(
                hooks,
                "pre_run_command",
                json!({ "command": command, "show_output": false }),
                runner,
            )
        }
        "crush" => {
            let hooks = hooks_of(obj)?;
            ensure_in(
                hooks,
                "PreToolUse",
                json!({ "name": "ljos", "matcher": "^bash$", "command": command, "timeout": 20 }),
                runner,
            )
        }
        other => return Err(format!("no hook shape for {other}")),
    };
    Ok(changed)
}

/// The text of a Cline hook script that runs the seat's hook.
#[must_use]
pub fn cline_script(command: &str) -> String {
    format!("#!/bin/sh\n# Written by ljos onboard: the seat's hook for Cline.\nexec {command}\n")
}

/// Write the runner's hooks at `file` (for Cline, the hooks directory).
#[must_use]
pub fn hook_step(runner: &str, file: &Path, base_command: &str, dry: bool) -> Step {
    let what = "hook".to_string();
    let command = runner_command(base_command, runner);
    if runner == "cline" {
        return cline_step(file, &command, dry);
    }
    let mut root: Value = match std::fs::read_to_string(file) {
        Ok(text) if !text.trim().is_empty() => match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                return Step {
                    what,
                    detail: format!("{}: not JSON: {e}", file.display()),
                    ok: false,
                }
            }
        },
        _ => json!({}),
    };
    let changed = match spec(runner, &mut root, &command) {
        Ok(c) => c,
        Err(e) => {
            return Step {
                what,
                detail: format!("{}: {e}", file.display()),
                ok: false,
            }
        }
    };
    if !changed {
        return Step {
            what,
            detail: format!("{} carries the seat's {runner} hook", file.display()),
            ok: true,
        };
    }
    if dry {
        return Step {
            what,
            detail: format!("would write the seat's {runner} hook to {}", file.display()),
            ok: true,
        };
    }
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| serde_json::to_string_pretty(&root).map_err(std::io::Error::other))
        .and_then(|text| std::fs::write(file, text + "\n"));
    match written {
        Ok(()) => Step {
            what,
            detail: format!("wrote the seat's {runner} hook to {}", file.display()),
            ok: true,
        },
        Err(e) => Step {
            what,
            detail: format!("{}: {e}", file.display()),
            ok: false,
        },
    }
}

/// Cline runs an executable named after the event from its hooks
/// directory. A script there that is not the seat's is left alone.
fn cline_step(dir: &Path, command: &str, dry: bool) -> Step {
    let what = "hook".to_string();
    let text = cline_script(command);
    let mut todo = Vec::new();
    for event in ["PreToolUse", "UserPromptSubmit"] {
        let path = dir.join(event);
        match std::fs::read_to_string(&path) {
            Ok(t) if t == text => {}
            Ok(t) if !t.contains("ljos onboard") => {
                return Step {
                    what,
                    detail: format!(
                        "{} is someone else's hook; add `{command}` to it",
                        path.display()
                    ),
                    ok: false,
                }
            }
            _ => todo.push(path),
        }
    }
    if todo.is_empty() {
        return Step {
            what,
            detail: format!("{} carries the seat's cline hooks", dir.display()),
            ok: true,
        };
    }
    if dry {
        return Step {
            what,
            detail: format!("would write the seat's cline hooks to {}", dir.display()),
            ok: true,
        };
    }
    for path in &todo {
        let written = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(path, &text))
            .and_then(|()| set_executable(path));
        if let Err(e) = written {
            return Step {
                what,
                detail: format!("{}: {e}", path.display()),
                ok: false,
            };
        }
    }
    Step {
        what,
        detail: format!("wrote the seat's cline hooks to {}", dir.display()),
        ok: true,
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DENY: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"no force push (seat rule `ljos-policyd`)"}}"#;
    const ASK: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"a push"}}"#;
    const NOTE: &str = r#"{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"lesson: run the tests"}}"#;

    fn payload(runner: &str, input: Value) -> Value {
        serde_json::from_str(&normalize_input(runner, &input.to_string())).unwrap()
    }

    #[test]
    fn each_runner_payload_reads_as_the_command_it_runs() {
        let cases = [
            (
                "windsurf",
                json!({"agent_action_name": "pre_run_command", "trajectory_id": "t1",
                       "tool_info": {"command_line": "git push -f", "cwd": "/tmp"}}),
            ),
            (
                "cline",
                json!({"hookName": "PreToolUse", "taskId": "t1", "workspaceRoots": ["/tmp"],
                       "preToolUse": {"toolName": "execute_command", "parameters": {"command": "git push -f"}}}),
            ),
            (
                "cline",
                json!({"hookName": "tool_call", "taskId": "t1", "workspaceRoots": ["/tmp"],
                       "tool_call": {"id": "c", "name": "run_commands", "input": {"commands": ["git push -f"]}}}),
            ),
            (
                "copilot",
                json!({"sessionId": "t1", "timestamp": 1, "cwd": "/tmp", "toolName": "bash",
                       "toolArgs": "{\"command\": \"git push -f\"}"}),
            ),
            (
                "copilot",
                json!({"hook_event_name": "PreToolUse", "session_id": "t1", "cwd": "/tmp",
                       "tool_name": "Bash", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "gemini",
                json!({"hook_event_name": "BeforeTool", "session_id": "t1", "cwd": "/tmp",
                       "tool_name": "run_shell_command", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "crush",
                json!({"event": "PreToolUse", "session_id": "t1", "cwd": "/tmp",
                       "tool_name": "bash", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "kiro",
                json!({"hook_event_name": "PreToolUse", "session_id": "t1", "cwd": "/tmp",
                       "tool_name": "shell", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "factory",
                json!({"hook_event_name": "PreToolUse", "session_id": "t1", "cwd": "/tmp",
                       "tool_name": "Execute", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "qwen",
                json!({"hook_event_name": "PreToolUse", "session_id": "t1", "cwd": "/tmp",
                       "tool_name": "run_shell_command", "tool_input": {"command": "git push -f"}}),
            ),
        ];
        for (runner, input) in cases {
            let v = payload(runner, input.clone());
            let call = crate::hook_call(&v.to_string());
            assert_eq!(call.event, "PreToolUse", "{runner}: {v}");
            assert_eq!(call.cue, "git push -f", "{runner}: {v}");
            assert_eq!(call.session.as_deref(), Some("t1"), "{runner}: {v}");
            assert_eq!(
                crate::hook_directory(&v.to_string()).unwrap(),
                std::fs::canonicalize("/tmp").unwrap(),
                "{runner}"
            );
        }
        let prompt = payload(
            "gemini",
            json!({"hook_event_name": "BeforeAgent", "session_id": "t", "prompt": "fix the build"}),
        );
        let call = crate::hook_call(&prompt.to_string());
        assert_eq!(
            (call.event.as_str(), call.cue.as_str()),
            ("UserPromptSubmit", "fix the build")
        );
        let prompt = payload(
            "cline",
            json!({"hookName": "UserPromptSubmit", "taskId": "t", "userPromptSubmit": {"prompt": "fix it"}}),
        );
        assert_eq!(
            crate::hook_call(&prompt.to_string()).event,
            "UserPromptSubmit"
        );
    }

    #[test]
    fn a_deny_reaches_each_runner_in_its_own_contract() {
        let (out, _, code) = answer("copilot", "PreToolUse", DENY);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!((v["permissionDecision"].as_str(), code), (Some("deny"), 0));
        assert!(v["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("force push"));

        let (out, _, code) = answer("gemini", "BeforeTool", DENY);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!((v["decision"].as_str(), code), (Some("deny"), 0));
        assert!(v["reason"].as_str().unwrap().contains("force push"));

        for runner in ["windsurf", "kiro"] {
            let (out, err, code) = answer(runner, "PreToolUse", DENY);
            assert_eq!((out.as_str(), code), ("", 2), "{runner}");
            assert!(err.contains("force push"), "{runner}");
        }

        let (out, _, _) = answer("cline", "PreToolUse", DENY);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["cancel"], json!(true));
        assert!(v["errorMessage"].as_str().unwrap().contains("force push"));

        let (out, _, _) = answer("crush", "PreToolUse", DENY);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["decision"], json!("deny"));

        for runner in ["factory", "qwen"] {
            let (out, _, code) = answer(runner, "PreToolUse", DENY);
            assert_eq!(
                (out.as_str(), code),
                (DENY, 0),
                "{runner} takes Claude Code's shape"
            );
        }
    }

    #[test]
    fn an_ask_is_a_deny_where_the_runner_cannot_ask() {
        let (out, _, _) = answer("copilot", "PreToolUse", ASK);
        assert!(out.contains("\"ask\""));
        for runner in ["gemini", "cline", "crush"] {
            let (out, _, _) = answer(runner, "PreToolUse", ASK);
            assert!(!out.contains("\"ask\""), "{runner}: {out}");
            assert!(out.contains("cannot ask"), "{runner}: {out}");
        }
        let (_, err, code) = answer("windsurf", "PreToolUse", ASK);
        assert_eq!(code, 2);
        assert!(err.contains("ask the person"));
    }

    #[test]
    fn an_allow_says_nothing_and_a_note_reaches_runners_that_take_one() {
        for runner in ["copilot", "windsurf", "kiro", "crush"] {
            assert_eq!(
                answer(runner, "PreToolUse", ""),
                (String::new(), String::new(), 0)
            );
        }
        let (out, _, code) = answer("cline", "PreToolUse", "");
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap(),
            json!({"cancel": false})
        );
        assert_eq!(code, 0);
        let (out, _, _) = answer("gemini", "BeforeAgent", NOTE);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "BeforeAgent");
        assert_eq!(
            v["hookSpecificOutput"]["additionalContext"],
            "lesson: run the tests"
        );
        let (out, _, _) = answer("cline", "UserPromptSubmit", NOTE);
        assert!(out.contains("contextModification"));
    }

    #[test]
    fn hook_files_are_written_in_each_runner_shape_and_never_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let base = "/opt/bin/ljos hook";
        for runner in RUNNERS {
            let file = if *runner == "cline" {
                dir.join("cline-hooks")
            } else {
                dir.join(format!("{runner}.json"))
            };
            if *runner == "gemini" {
                // A settings file with entries of its own keeps them.
                std::fs::write(&file, r#"{"mcpServers":{"x":{}},"hooks":{"BeforeTool":[{"matcher":"x","hooks":[]}]}}"#).unwrap();
            }
            let first = hook_step(runner, &file, base, false);
            assert!(first.ok, "{runner}: {}", first.detail);
            assert!(first.detail.contains("wrote"), "{runner}: {}", first.detail);
            let again = hook_step(runner, &file, base, false);
            assert!(
                again.ok && again.detail.contains("carries"),
                "{runner}: {}",
                again.detail
            );
            let command = format!("{base} --runner {runner}");
            if *runner == "cline" {
                for event in ["PreToolUse", "UserPromptSubmit"] {
                    let text = std::fs::read_to_string(file.join(event)).unwrap();
                    assert!(text.contains(&command));
                }
                continue;
            }
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            let found = |p: &str| v.pointer(p).cloned().unwrap_or(Value::Null);
            match *runner {
                "copilot" => {
                    assert_eq!(found("/version"), json!(1));
                    assert_eq!(found("/hooks/PreToolUse/0/bash"), json!(command));
                    assert_eq!(found("/hooks/PreToolUse/0/matcher"), json!("Bash"));
                }
                "gemini" => {
                    assert_eq!(found("/mcpServers/x"), json!({}));
                    assert_eq!(found("/hooks/BeforeTool/0/matcher"), json!("x"));
                    assert_eq!(
                        found("/hooks/BeforeTool/1/matcher"),
                        json!("run_shell_command")
                    );
                    assert_eq!(
                        found("/hooks/BeforeTool/1/hooks/0/timeout"),
                        json!(20_000),
                        "milliseconds"
                    );
                    assert_eq!(
                        found("/hooks/BeforeAgent/0/hooks/0/command"),
                        json!(command)
                    );
                }
                "qwen" => {
                    assert_eq!(
                        found("/hooks/PreToolUse/0/matcher"),
                        json!("^run_shell_command$")
                    );
                    assert_eq!(
                        found("/hooks/PreToolUse/0/hooks/0/timeout"),
                        json!(20),
                        "seconds"
                    );
                }
                "factory" => {
                    assert_eq!(found("/PreToolUse/0/matcher"), json!("Execute"));
                    assert_eq!(found("/PreToolUse/0/hooks/0/command"), json!(command));
                }
                "windsurf" => {
                    assert_eq!(found("/hooks/pre_run_command/0/command"), json!(command));
                }
                "kiro" => {
                    assert_eq!(found("/version"), json!("v1"));
                    assert_eq!(found("/hooks/0/trigger"), json!("PreToolUse"));
                    assert_eq!(found("/hooks/0/action/command"), json!(command));
                }
                "crush" => {
                    assert_eq!(found("/hooks/PreToolUse/0/matcher"), json!("^bash$"));
                    assert_eq!(found("/hooks/PreToolUse/0/command"), json!(command));
                }
                _ => unreachable!(),
            }
        }
        let foreign = dir.join("foreign-cline");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("PreToolUse"), "#!/bin/sh\necho mine\n").unwrap();
        let step = hook_step("cline", &foreign, base, false);
        assert!(
            !step.ok && step.detail.contains("someone else's"),
            "{}",
            step.detail
        );
    }
}
