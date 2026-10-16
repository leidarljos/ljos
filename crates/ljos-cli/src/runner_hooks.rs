//! Hook setups for runners whose hook contract is not Claude Code's own:
//! GitHub Copilot, Gemini CLI, Windsurf (Devin Desktop), Factory Droid,
//! Kiro, Cline, Qwen Code, Crush, Auggie, goose and Amp.
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
    "copilot",
    "gemini",
    "windsurf",
    "factory",
    "kiro",
    "cline",
    "qwen",
    "crush",
    "auggie",
    "goose",
    "amp",
    "openhands",
];

/// Runners whose hook entry must name one program with no arguments (Amp's
/// delegate) or a `.sh` file (Auggie): onboard writes this script beside
/// the settings file, and the entry names it.
fn needs_script(runner: &str) -> bool {
    matches!(runner, "auggie" | "amp")
}

/// The wrapper script's path for a runner whose hooks live in `file`.
#[must_use]
pub fn script_path(runner: &str, file: &Path) -> std::path::PathBuf {
    file.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("ljos-guard-{runner}.sh"))
}

/// Whether `name` is a runner this module writes hooks for.
#[must_use]
pub fn is_runner(name: &str) -> bool {
    RUNNERS.contains(&name)
}

/// Whether the runner shows the person a prompt on an `ask`.
fn asks(runner: &str) -> bool {
    matches!(runner, "copilot" | "factory" | "qwen" | "amp")
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
        "goose" => {
            let event = v["event"].as_str().unwrap_or("PreToolUse");
            out.insert("hook_event_name".into(), json!(event));
            put(&mut out, "session_id", v["session_id"].as_str());
            put(&mut out, "cwd", v["working_dir"].as_str());
            put(&mut out, "tool_name", v["tool_name"].as_str());
            put(&mut out, "prompt", v["message"].as_str());
            if !v["tool_input"].is_null() {
                out.insert("tool_input".into(), v["tool_input"].clone());
            }
        }
        "openhands" => {
            let event = v["event_type"].as_str().unwrap_or("PreToolUse");
            out.insert("hook_event_name".into(), json!(event));
            put(&mut out, "session_id", v["session_id"].as_str());
            put(&mut out, "cwd", v["working_dir"].as_str());
            put(&mut out, "prompt", v["message"].as_str());
            let tool = v["tool_name"]
                .as_str()
                .map(|t| if t == "terminal" { "Bash" } else { t });
            put(&mut out, "tool_name", tool);
            if !v["tool_input"].is_null() {
                out.insert("tool_input".into(), v["tool_input"].clone());
            }
        }
        "auggie" => {
            let mut v = v;
            if v["tool_name"].as_str() == Some("launch-process") {
                v["tool_name"] = json!("Bash");
            }
            return v.to_string();
        }
        "amp" => {
            // Amp pipes the tool's arguments alone and names the tool in
            // the environment; Bash takes `cmd`.
            let tool = std::env::var("AGENT_TOOL_NAME").unwrap_or_else(|_| "Bash".into());
            out.insert("hook_event_name".into(), json!("PreToolUse"));
            out.insert("tool_name".into(), json!(tool));
            if let Ok(t) = std::env::var("AMP_THREAD_ID") {
                put(&mut out, "session_id", Some(&t));
            }
            put(&mut out, "cwd", v["cwd"].as_str());
            match v["cmd"]
                .as_str()
                .map(str::to_string)
                .or_else(|| command_of(&v))
            {
                Some(c) => {
                    out.insert("tool_input".into(), json!({ "command": c }));
                }
                None => {
                    out.insert("tool_input".into(), v.clone());
                }
            }
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
        ("windsurf" | "kiro" | "goose", Some(_)) => (String::new(), said.reason + "\n", 2),
        ("goose", None) => (String::new(), String::new(), 0),
        ("openhands", Some(_)) => (
            line(json!({ "decision": "deny", "reason": said.reason })),
            said.reason + "\n",
            2,
        ),
        ("openhands", None) if !said.context.is_empty() => (
            line(json!({ "additionalContext": said.context })),
            String::new(),
            0,
        ),
        ("amp", Some("ask")) => (String::new(), said.reason + "\n", 1),
        ("amp", Some(_)) => (String::new(), said.reason + "\n", 2),
        ("amp", None) => (String::new(), String::new(), 0),
        ("auggie", Some(d)) => (
            line(json!({ "hookSpecificOutput": {
                "hookEventName": if event.is_empty() { "PreToolUse" } else { event },
                "permissionDecision": d,
                "permissionDecisionReason": said.reason,
            }})),
            String::new(),
            0,
        ),
        ("auggie", None) if !said.context.is_empty() => (
            line(json!({ "hookSpecificOutput": {
                "hookEventName": if event.is_empty() { "PreToolUse" } else { event },
                "additionalContext": said.context,
            }})),
            String::new(),
            0,
        ),
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
            [
                "hook_event_name",
                "agent_action_name",
                "hookName",
                "event",
                "event_type",
            ]
            .iter()
            .find_map(|k| v[*k].as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

/// The default wait for the inner `ljos hook`. [`crate::guard_timeout`] is
/// what [`relay`] uses, so `LJOS_GUARD_TIMEOUT` changes it. Each runner's
/// own timeout is 15 or 20 seconds, and several of them let the call run
/// when it lapses, so the relay answers first.
pub const INNER_TIMEOUT: std::time::Duration = crate::GUARD_TIMEOUT;

/// An allow, with a note, for an inner hook that gave no verdict. `what`
/// says how it failed. A command no rule would refuse is allowed; the
/// dangerous ones are judged before this runs.
fn fault_output(event: &str, what: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": if event.is_empty() { "PreToolUse" } else { event },
            "additionalContext": crate::unhealthy_note(what),
        }
    })
    .to_string()
}

/// The command a normalized payload would run, or empty when it has none.
fn tool_command(normalized: &str) -> String {
    serde_json::from_str::<Value>(normalized.trim())
        .ok()
        .and_then(|v| command_of(&v["tool_input"]))
        .unwrap_or_default()
}

/// The fast in-process verdict, in the shape the inner hook prints, when
/// the command is one the static check refuses. `None` means the slow
/// hook still has to run.
fn early_answer(normalized: &str, event: &str) -> Option<String> {
    let line = tool_command(normalized);
    let rule = crate::static_guard(&line)?;
    let event = if event.is_empty() {
        "PreToolUse"
    } else {
        event
    };
    let call = crate::HookCall {
        event: event.to_string(),
        cue: line,
        session: None,
        shape: crate::HookShape::CamelCase,
    };
    Some(crate::hook_output_ruled(&call, "", Some(&rule)))
}

/// Run `cmd` as the inner hook with `stdin` as its input, and give back
/// what it printed. A spawn error, a kill by signal, a non-zero exit, or
/// no exit within `timeout` is an allow that says the guard is unhealthy.
/// A non-zero exit that still printed a deny or an ask keeps it.
#[must_use]
pub fn run_inner(
    mut cmd: std::process::Command,
    stdin: &str,
    event: &str,
    timeout: std::time::Duration,
) -> String {
    use std::io::{Read, Write};
    let spawned = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return fault_output(event, &format!("did not start ({e})")),
    };
    // Feed and drain on their own threads, so a hook that reads nothing or
    // prints a lot cannot stall the clock below.
    let input = stdin.to_string();
    // The writer is never joined: a child that left a grandchild holding
    // its stdin could block it, and the process exits soon after.
    if let Some(mut pipe) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = pipe.write_all(input.as_bytes());
        });
    }
    let reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("gave no answer within {} s", timeout.as_secs_f32()));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(e) => break Err(format!("could not be waited on ({e})")),
        }
    };
    let status = match status {
        Ok(status) => status,
        // A killed hook's children may still hold the pipe; do not wait
        // for the reader.
        Err(what) => return fault_output(event, &what),
    };
    let out = reader
        .and_then(|r| r.join().ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    if status.success() || read_answer(&out).decision.is_some() {
        return out;
    }
    let what = match status.code() {
        Some(code) => format!("exited with status {code}"),
        None => "was killed by a signal".to_string(),
    };
    fault_output(event, &what)
}

/// Run the seat's hook for a runner: `ljos hook` on the normalized
/// payload, its answer put in the runner's contract. Returns the exit
/// code the runner reads. A force push, a seat deny, and an `rm -rf` are
/// judged here, before the inner hook. An inner hook that crashes, exits
/// non-zero without a decision, or runs past [`crate::guard_timeout`]
/// allows the call and says the guard is unhealthy.
pub fn relay(runner: &str, input: &str, limit: usize, event: Option<&str>) -> anyhow::Result<i32> {
    let normalized = normalize_input(runner, input);
    let raw = raw_event(input);
    let text = match early_answer(&normalized, &raw) {
        Some(text) => text,
        None => match std::env::current_exe() {
            Ok(exe) => {
                let mut cmd = std::process::Command::new(exe);
                cmd.arg("hook").arg("--limit").arg(limit.to_string());
                if let Some(e) = event {
                    cmd.arg("--event").arg(e);
                }
                run_inner(cmd, &normalized, &raw, crate::guard_timeout())
            }
            Err(e) => fault_output(&raw, &format!("could not find its own binary ({e})")),
        },
    };
    let (mut out, err, code) = answer(runner, &raw, &text);
    if let Some(line) = first_prompt_start_line(runner, &raw, input) {
        out = with_start_line(&out, &line);
    }
    print!("{out}");
    eprint!("{err}");
    Ok(code)
}

/// OpenHands drops a `SessionStart` hook's output, so the session-start
/// line rides on the session's first prompt: once per session id, marked
/// under the state directory.
fn first_prompt_start_line(runner: &str, raw: &str, input: &str) -> Option<String> {
    if runner != "openhands" || raw != "UserPromptSubmit" {
        return None;
    }
    let v: Value = serde_json::from_str(input.trim()).ok()?;
    let sid: String = v["session_id"]
        .as_str()?
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if sid.is_empty() {
        return None;
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/state")))?;
    let mark = state.join("ljos/openhands-started").join(&sid);
    if mark.exists() {
        return None;
    }
    let _ = mark.parent().map(std::fs::create_dir_all);
    let _ = std::fs::write(&mark, "");
    Some(crate::session_start_for(Some(&sid)))
}

/// An OpenHands answer with `line` put ahead of its context.
fn with_start_line(out: &str, line: &str) -> String {
    let mut v: Value = serde_json::from_str(out.trim()).unwrap_or_else(|_| json!({}));
    if v.get("decision").is_some() {
        return out.to_string();
    }
    let rest = v["additionalContext"].as_str().unwrap_or("").to_string();
    v["additionalContext"] = json!(if rest.is_empty() {
        line.to_string()
    } else {
        format!("{line}\n\n{rest}")
    });
    v.to_string() + "\n"
}

/// The hook command a runner's file names.
#[must_use]
pub fn runner_command(base: &str, runner: &str) -> String {
    format!("{base} --runner {runner}")
}

fn ours(v: &Value, runner: &str) -> bool {
    let t = v.to_string();
    t.contains(&format!("--runner {runner}")) || t.contains(&format!("ljos-guard-{runner}.sh"))
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
        "openhands" => {
            let hooks = hooks_of(obj)?;
            let a = ensure_in(
                hooks,
                "PreToolUse",
                group(Some("terminal"), command, 20, runner),
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
        "auggie" => {
            let hooks = hooks_of(obj)?;
            ensure_in(
                hooks,
                "PreToolUse",
                group(Some("launch-process"), command, 20_000, runner),
                runner,
            )
        }
        "goose" => {
            // The plugin's hooks file is the seat's whole; a hook that
            // cannot run blocks the call.
            let want = json!({ "hooks": {
                "PreToolUse": [{ "matcher": "shell", "hooks": [{
                    "type": "command", "command": command, "timeout": 20, "on_failure": "block"
                }]}],
                "UserPromptSubmit": [{ "hooks": [{
                    "type": "command", "command": command, "timeout": 15
                }]}]
            }});
            let same = Value::Object(obj.clone()) == want;
            *obj = want.as_object().cloned().unwrap_or_default();
            !same
        }
        "amp" => {
            let list = obj
                .entry("amp.permissions".to_string())
                .or_insert_with(|| json!([]));
            let Some(list) = list.as_array_mut() else {
                return Err("amp.permissions is not a list".into());
            };
            if list.iter().any(|r| ours(r, runner)) {
                false
            } else {
                // First, since Amp applies the first rule that matches.
                list.insert(
                    0,
                    json!({ "tool": "Bash", "action": "delegate", "to": command }),
                );
                true
            }
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
    let mut command = runner_command(base_command, runner);
    if runner == "cline" {
        return cline_step(file, &command, dry);
    }
    let script = needs_script(runner).then(|| script_path(runner, file));
    let script_text = format!(
        "#!/bin/sh\n# Written by ljos onboard: the seat's guard for {runner}.\nexec {command}\n"
    );
    let script_stale = script
        .as_ref()
        .is_some_and(|p| std::fs::read_to_string(p).ok().as_deref() != Some(script_text.as_str()));
    if let Some(p) = &script {
        command = p.display().to_string();
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
    let manifest = (runner == "goose")
        .then(|| {
            file.parent()
                .and_then(Path::parent)
                .map(|d| d.join("plugin.json"))
        })
        .flatten();
    let manifest_missing = manifest.as_ref().is_some_and(|m| !m.is_file());
    if !changed && !script_stale && !manifest_missing {
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
        .and_then(|text| std::fs::write(file, text + "\n"))
        .and_then(|()| match &script {
            Some(p) => std::fs::write(p, &script_text).and_then(|()| set_executable(p)),
            None => Ok(()),
        })
        .and_then(|()| match &manifest {
            Some(m) if manifest_missing => std::fs::write(
                m,
                "{\n  \"name\": \"ljos\",\n  \"version\": \"1.0.0\",\n  \"description\": \
                 \"The ljos seat's memory and guard, written by ljos onboard\"\n}\n",
            ),
            _ => Ok(()),
        });
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

    #[cfg(unix)]
    fn sh(script: &str) -> std::process::Command {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }

    #[cfg(unix)]
    fn inner(script: &str, timeout_ms: u64) -> String {
        run_inner(
            sh(script),
            r#"{"tool_name":"Bash"}"#,
            "PreToolUse",
            std::time::Duration::from_millis(timeout_ms),
        )
    }

    /// Runners that show an allow's note. The others drop it and still
    /// exit 0, which is an allow.
    fn shows_context(runner: &str) -> bool {
        matches!(
            runner,
            "gemini" | "openhands" | "auggie" | "kiro" | "cline" | "crush" | "factory" | "qwen"
        )
    }

    fn allows_unhealthy(out: &str, why: &str) {
        let said = read_answer(out);
        assert!(said.decision.is_none(), "{out}");
        assert!(out.contains("unhealthy"), "{out}");
        assert!(out.contains(why), "{out}");
        assert!(!out.contains("refused unchecked"), "{out}");
        for runner in RUNNERS {
            let (stdout, stderr, code) = answer(runner, "PreToolUse", out);
            let body = format!("{stdout}{stderr}");
            assert_eq!(code, 0, "{runner}: {body}");
            assert!(
                !body.contains("\"deny\"") && !body.contains("\"cancel\":true"),
                "{runner}: {body}"
            );
            if shows_context(runner) {
                assert!(body.contains("unhealthy"), "{runner}: {body}");
                assert!(body.contains(why), "{runner}: {body}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_inner_hook_that_fails_allows_and_says_the_guard_is_unhealthy() {
        // The timeout, the exit, and the signal are injected. A wall-clock
        // race is what made a slow runner report the timeout instead of
        // the signal.
        let timeout = std::time::Duration::from_secs(5);
        let injected = [
            format!("gave no answer within {} s", timeout.as_secs_f32()),
            "exited with status 3".to_string(),
            "was killed by a signal".to_string(),
        ];
        for why in &injected {
            allows_unhealthy(&fault_output("PreToolUse", why), why);
        }
        allows_unhealthy(&inner("exit 3", 30_000), "exited with status 3");
        let missing = run_inner(
            std::process::Command::new("/nonexistent/ljos"),
            "{}",
            "PreToolUse",
            std::time::Duration::from_secs(1),
        );
        allows_unhealthy(&missing, "did not start");
        assert_eq!(INNER_TIMEOUT, crate::GUARD_TIMEOUT);
    }

    #[cfg(unix)]
    #[test]
    fn an_inner_hook_that_answers_keeps_its_answer() {
        assert_eq!(inner("true", 30_000), "", "a clean allow");
        assert_eq!(inner(&format!("printf '%s' '{DENY}'"), 30_000), DENY);
        assert_eq!(
            inner(&format!("printf '%s' '{DENY}'; exit 1"), 30_000),
            DENY,
            "a deny printed before a non-zero exit stands"
        );
        let note = inner(&format!("printf '%s' '{NOTE}'; exit 1"), 30_000);
        assert!(read_answer(&note).decision.is_none(), "{note}");
        assert!(note.contains("unhealthy"), "{note}");
        assert!(note.contains("exited with status 1"), "{note}");
    }

    #[test]
    fn a_known_dangerous_command_is_judged_before_the_inner_hook() {
        use std::os::unix::fs::PermissionsExt;
        let _lock = crate::test_env_lock();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!("ljos-early-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let keys = [
            "HOME",
            "XDG_DATA_HOME",
            "LJOS_OFF",
            "POLICYD_BIN",
            "PACKSET_WORKSPACE",
        ];
        struct RestoreEnv(Vec<(String, Option<std::ffi::OsString>)>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                for (k, v) in &self.0 {
                    unsafe {
                        match v {
                            Some(v) => std::env::set_var(k, v),
                            None => std::env::remove_var(k),
                        }
                    }
                }
            }
        }
        let saved = RestoreEnv(
            keys.iter()
                .map(|k| ((*k).to_string(), std::env::var_os(k)))
                .collect(),
        );
        let data = dir.join("data");
        unsafe {
            std::env::set_var("HOME", &dir);
            std::env::set_var("XDG_DATA_HOME", &data);
            std::env::set_var("LJOS_OFF", "0");
            std::env::set_var("POLICYD_BIN", dir.join("does-not-run"));
            std::env::set_var("PACKSET_WORKSPACE", "early-guard");
        }
        let force = json!({"tool_input": {"command": "git push --force origin main"}}).to_string();
        let started = std::time::Instant::now();
        let asked = early_answer(&force, "PreToolUse").expect("a force push asks");
        let plain = json!({"tool_input": {"command": "git status"}}).to_string();
        assert!(
            early_answer(&plain, "").is_none(),
            "git status is not dangerous"
        );
        let rm = json!({"tool_input": {"command": "rm -rf /home/u/proj"}}).to_string();
        let denied = early_answer(&rm, "PreToolUse").expect("rm -rf denies");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the static check waited on the guard"
        );
        assert_eq!(
            read_answer(&asked).decision.as_deref(),
            Some("ask"),
            "{asked}"
        );
        assert!(asked.contains("git-force-push"), "{asked}");
        assert_eq!(
            read_answer(&denied).decision.as_deref(),
            Some("deny"),
            "{denied}"
        );
        assert!(denied.contains("rm-rf-outside-tmp"), "{denied}");
        drop(saved);
        let _ = std::fs::remove_dir_all(&dir);
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
                "goose" => {
                    assert_eq!(found("/hooks/PreToolUse/0/hooks/0/command"), json!(command));
                }
                "auggie" => {
                    assert_eq!(
                        found("/hooks/PreToolUse/0/matcher"),
                        json!("launch-process")
                    );
                    let script = script_path("auggie", &file).display().to_string();
                    assert_eq!(found("/hooks/PreToolUse/0/hooks/0/command"), json!(script));
                }
                "amp" => {
                    assert_eq!(found("/amp.permissions/0/action"), json!("delegate"));
                }
                "openhands" => {
                    assert_eq!(found("/hooks/PreToolUse/0/matcher"), json!("terminal"));
                    assert_eq!(found("/hooks/PreToolUse/0/hooks/0/command"), json!(command));
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

    #[test]
    fn goose_payload_reaches_the_seat_and_a_deny_exits_2() {
        let n = normalize_input(
            "goose",
            r#"{"event":"PreToolUse","session_id":"g-1","tool_name":"shell","tool_input":{"command":"rm -rf /"},"working_dir":"/r"}"#,
        );
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["hook_event_name"], "PreToolUse");
        assert_eq!(v["tool_input"]["command"], "rm -rf /");
        assert_eq!(v["cwd"], "/r");
        let deny = r#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no"}}"#;
        assert_eq!(
            answer("goose", "PreToolUse", deny),
            (String::new(), "no\n".into(), 2)
        );
        assert_eq!(answer("goose", "PreToolUse", "{}").2, 0);
    }

    #[test]
    fn amp_reads_cmd_and_rejects_with_2_and_asks_with_1() {
        let n = normalize_input("amp", r#"{"cmd":"git push --force"}"#);
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["tool_input"]["command"], "git push --force");
        let deny = r#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no"}}"#;
        assert_eq!(answer("amp", "", deny).2, 2);
        let ask = r#"{"hookSpecificOutput":{"permissionDecision":"ask","permissionDecisionReason":"sure?"}}"#;
        assert_eq!(answer("amp", "", ask).2, 1);
        assert_eq!(answer("amp", "", "").2, 0);
    }

    #[test]
    fn auggie_launch_process_is_the_shell_and_its_deny_is_claude_shaped() {
        let n = normalize_input(
            "auggie",
            r#"{"hook_event_name":"PreToolUse","tool_name":"launch-process","tool_input":{"command":"ls"}}"#,
        );
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["tool_name"], "Bash");
        let deny = r#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no"}}"#;
        let (out, _, code) = answer("auggie", "PreToolUse", deny);
        let o: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(o["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(code, 0);
    }

    #[test]
    fn auggie_and_amp_hooks_name_a_script_and_are_written_once() {
        for (runner, name) in [("auggie", "settings.json"), ("amp", "settings.json")] {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join(name);
            std::fs::write(
                &file,
                r#"{"keep": 1, "amp.permissions": [{"tool": "Read", "action": "allow"}]}"#,
            )
            .unwrap();
            let step = hook_step(runner, &file, "/bin/ljos hook", false);
            assert!(step.ok, "{}", step.detail);
            let script = script_path(runner, &file);
            let text = std::fs::read_to_string(&script).unwrap();
            assert!(text.contains(&format!("/bin/ljos hook --runner {runner}")));
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(v["keep"], 1);
            assert!(v.to_string().contains(&script.display().to_string()));
            if runner == "amp" {
                assert_eq!(v["amp.permissions"][0]["action"], "delegate");
                assert_eq!(v["amp.permissions"][1]["tool"], "Read");
            }
            let again = hook_step(runner, &file, "/bin/ljos hook", false);
            assert!(again.detail.contains("carries"), "{}", again.detail);
        }
    }

    #[test]
    fn goose_hook_is_a_plugin_that_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ljos/hooks/hooks.json");
        let step = hook_step("goose", &file, "/bin/ljos hook", false);
        assert!(step.ok, "{}", step.detail);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["on_failure"],
            "block"
        );
        assert!(dir.path().join("ljos/plugin.json").is_file());
        assert!(hook_step("goose", &file, "/bin/ljos hook", false)
            .detail
            .contains("carries"));
    }

    #[test]
    fn openhands_terminal_is_the_shell_and_a_deny_exits_2() {
        let n = normalize_input(
            "openhands",
            r#"{"event_type":"PreToolUse","tool_name":"terminal","tool_input":{"command":"rm -rf ~"},"working_dir":"/w"}"#,
        );
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["tool_name"], "Bash");
        assert_eq!(v["cwd"], "/w");
        let deny = r#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no"}}"#;
        let (out, _, code) = answer("openhands", "PreToolUse", deny);
        assert!(out.contains("\"decision\":\"deny\""));
        assert_eq!(code, 2);
    }

    #[test]
    fn the_start_line_leads_an_openhands_prompt_note() {
        let out = with_start_line("{\"additionalContext\":\"lesson\"}\n", "ljos is on: x");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["additionalContext"], "ljos is on: x\n\nlesson");
        assert_eq!(
            with_start_line("", "ljos is on").trim(),
            r#"{"additionalContext":"ljos is on"}"#
        );
    }
}
