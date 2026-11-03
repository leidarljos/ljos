//! Hook setups for runners whose hook contract is not the terminal harness's own:
//! GitHub Copilot, Gemini CLI, Windsurf (Devin Desktop), Factory Droid,
//! Kiro, Cline, Qwen Code, Crush, Auggie, goose and Amp.
//!
//! `onboard` writes each runner's hook file with the command
//! `ljos hook --runner NAME`. That command turns the runner's stdin into
//! the terminal harness payload the seat reads, runs the ordinary hook, and
//! writes the answer back in the runner's own field names. An answer
//! allows the call and exits 0. A rule's warning rides in the runner's
//! context field, or on stderr where the runner reads no context. The
//! one deny the seat prints, policyd's block under the person's
//! `block-on-warning`, is the runner's own deny: its fields, and for some
//! runners exit 2.

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

/// Whether the runner reads a deny from the hook's exit code, 2, beside
/// or in place of its JSON.
fn denies_by_exit(runner: &str) -> bool {
    matches!(
        runner,
        "windsurf" | "factory" | "kiro" | "qwen" | "crush" | "goose" | "amp" | "openhands"
    )
}

fn reason_stderr(reason: &str) -> String {
    if reason.trim().is_empty() {
        "ljos denied this command.\n".to_string()
    } else {
        format!("{reason}\n")
    }
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

/// The runner's stdin as the terminal harness payload `ljos hook` reads:
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

/// The context in the seat's answer, whatever shape it came in. The
/// reason of a decision that is no [`denial`], such as an ask only a
/// hook from an older build prints, leads as a warning line: a runner
/// here reads it as an allow.
fn read_answer(out: &str) -> String {
    let trimmed = out.trim();
    let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
        return trimmed.to_string();
    };
    let specific = &v["hookSpecificOutput"];
    let reason = specific["permissionDecisionReason"]
        .as_str()
        .or_else(|| v["reason"].as_str())
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(|r| {
            if r.starts_with("warning:") {
                r.to_string()
            } else {
                format!("warning: {r}")
            }
        });
    let context = specific["additionalContext"]
        .as_str()
        .or_else(|| v["context"].as_str())
        .filter(|c| !c.trim().is_empty())
        .map(str::to_string);
    reason
        .into_iter()
        .chain(context)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The reason and the context of a seat answer that denies a tool call:
/// policyd's block under the person's `block-on-warning`, or the cloud
/// seat push rule's refusal. `None` on any other answer, and on any other
/// event: a held stop's `block` keeps the runner going.
fn denial(event: &str, out: &str) -> Option<(String, String)> {
    if !gate_event(event) {
        return None;
    }
    let trimmed = out.trim();
    let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
        // A plain-text answer is an argv line; a deny says so first.
        let reason = trimmed.strip_prefix("deny:")?;
        return Some((reason.trim().to_string(), String::new()));
    };
    let specific = &v["hookSpecificOutput"];
    let decision = specific["permissionDecision"]
        .as_str()
        .or_else(|| v["decision"].as_str());
    if !matches!(decision, Some("deny" | "block")) {
        return None;
    }
    let field = |inner: &str, top: &str| {
        specific[inner]
            .as_str()
            .or_else(|| v[top].as_str())
            .unwrap_or("")
            .to_string()
    };
    Some((
        field("permissionDecisionReason", "reason"),
        field("additionalContext", "context"),
    ))
}

/// A [`denial`] in the runner's contract: what to print, what to write
/// to stderr, and the exit code. `out` is the seat's whole answer.
fn denied(
    runner: &str,
    event: &str,
    out: &str,
    reason: &str,
    context: &str,
) -> (String, String, i32) {
    let line = |v: Value| v.to_string() + "\n";
    let mut rendered = match runner {
        "factory" | "qwen" => (out.to_string(), String::new(), 0),
        "copilot" => (
            line(json!({
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            })),
            String::new(),
            0,
        ),
        "gemini" => {
            let mut v = json!({ "decision": "deny", "reason": reason });
            if !context.is_empty() {
                v["hookSpecificOutput"] =
                    json!({ "hookEventName": event, "additionalContext": context });
            }
            (line(v), String::new(), 0)
        }
        "openhands" => (
            line(json!({ "decision": "deny", "reason": reason })),
            reason_stderr(reason),
            2,
        ),
        "auggie" => (
            line(json!({ "hookSpecificOutput": {
                "hookEventName": if event.is_empty() { "PreToolUse" } else { event },
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }})),
            String::new(),
            0,
        ),
        "cline" => (
            line(json!({ "cancel": true, "errorMessage": reason })),
            String::new(),
            0,
        ),
        "crush" => (
            line(json!({ "decision": "deny", "reason": reason })),
            String::new(),
            0,
        ),
        // Windsurf, Kiro, goose and Amp read the deny from the exit alone.
        _ => (String::new(), reason_stderr(reason), 2),
    };
    if denies_by_exit(runner) {
        rendered.2 = 2;
        if rendered.1.trim().is_empty() {
            rendered.1 = reason_stderr(reason);
        }
    }
    rendered
}

/// The seat's answer (the terminal harness JSON `ljos hook` prints) in the
/// runner's own field names: what to print, what to write to stderr, and
/// the exit code. A warning is no decision, so each runner goes on with
/// the call as it would with no hook, and the exit is 0. A [`denial`] is
/// the runner's deny. `event` is the event name the runner sent.
#[must_use]
pub fn answer(runner: &str, event: &str, out: &str) -> (String, String, i32) {
    if let Some((reason, context)) = denial(event, out) {
        return denied(runner, event, out, &reason, &context);
    }
    let context = read_answer(out);
    let line = |v: Value| v.to_string() + "\n";
    let named = if event.is_empty() {
        "PreToolUse"
    } else {
        event
    };
    let printed = match runner {
        "gemini" if context.is_empty() => line(json!({})),
        "cline" if context.is_empty() => line(json!({ "cancel": false })),
        _ if context.is_empty() => String::new(),
        "gemini" | "factory" | "qwen" | "auggie" => line(json!({ "hookSpecificOutput": {
            "hookEventName": named,
            "additionalContext": context,
        }})),
        "openhands" => line(json!({ "additionalContext": context })),
        "kiro" => format!("{context}\n"),
        "cline" => line(json!({ "cancel": false, "contextModification": context })),
        "crush" => line(json!({ "context": context })),
        _ => String::new(),
    };
    // These runners read no context from a hook, so stderr is the one
    // place left for a warning; the rest of the context has none.
    let warned = if matches!(runner, "copilot" | "windsurf" | "goose" | "amp") {
        context
            .lines()
            .filter(|l| l.starts_with("warning:"))
            .map(|l| format!("{l}\n"))
            .collect()
    } else {
        String::new()
    };
    (printed, warned, 0)
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

/// The default wait for the inner `ljos hook` on a tool call.
/// [`relay_timeout`] is what [`relay`] uses. Each runner's own timeout is
/// 15 or 20 seconds, and several of them let the call run when it lapses,
/// so the relay answers first on a gate.
pub const INNER_TIMEOUT: std::time::Duration = crate::GUARD_TIMEOUT;

/// A runner's event name as the seat spells it, so [`crate::is_gate_event`]
/// can tell a tool call from a prompt. Gemini's `BeforeTool` and
/// Windsurf's `pre_run_command` are the gate. An empty name is a tool
/// call: Amp sends the arguments alone.
fn seat_event(event: &str) -> &str {
    match event {
        ""
        | "BeforeTool"
        | "pre_run_command"
        | "tool_call"
        | "pre_tool_use"
        | "preToolUse"
        | "beforeShellExecution" => "PreToolUse",
        "BeforeAgent" | "pre_user_prompt" | "beforeSubmitPrompt" | "pre_llm_call" => {
            "UserPromptSubmit"
        }
        "AfterTool" | "post_run_command" | "post_tool_use" | "postToolUse" => "PostToolUse",
        other => other,
    }
}

fn gate_event(event: &str) -> bool {
    crate::is_gate_event(seat_event(event))
}

/// How long [`relay`] waits. A tool call uses [`crate::guard_timeout`].
/// A prompt waits out the inner hook's own deadline
/// ([`crate::HOOK_DEADLINE_MS`]): that hook exits on its own with whatever
/// it has, and a shorter wait kills the answer and the recall in it.
#[must_use]
pub fn relay_timeout(event: &str) -> std::time::Duration {
    if gate_event(event) {
        crate::guard_timeout()
    } else {
        std::time::Duration::from_millis(crate::HOOK_DEADLINE_MS.saturating_add(1_000))
    }
}

/// The answer for an inner hook fault. `what` says how it failed. A
/// fault allows with a note on every event: a prompt that the guard could
/// not finish is not a command, and a command the guard could not judge
/// runs with the warning.
fn fault_output(event: &str, what: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": if event.is_empty() { "PreToolUse" } else { event },
            "additionalContext": crate::unhealthy_note(what),
        }
    })
    .to_string()
}

/// The answer for a relay that failed before it could judge, in the
/// runner's field names: an allow with the unhealthy-guard note.
#[must_use]
pub fn relay_fault_answer(runner: &str, event: &str, what: &str) -> (String, String) {
    let what: String = what
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(240)
        .collect();
    let (out, err, _) = answer(runner, event, &fault_output(event, &what));
    (out, err)
}

/// Run `cmd` as the inner hook with `stdin` as its input, and give back
/// what it printed. A spawn error, a kill by signal, a non-zero exit, or
/// no exit within `timeout` is an allow with a note that says how the
/// guard failed. A successful exit is that answer.
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
    if status.success() {
        return out;
    }
    let what = match status.code() {
        Some(code) => format!("exited with status {code}"),
        None => "was killed by a signal".to_string(),
    };
    fault_output(event, &what)
}

/// Run the seat's hook for a runner: `ljos hook` on the normalized
/// payload, its answer put in the runner's field names. Returns the exit
/// code the runner reads: 0, or 2 for a [`denial`] where the runner reads
/// a deny that way. An inner hook that crashes, exits non-zero, or runs
/// past [`relay_timeout`] is an allow with a note that says the guard
/// failed.
pub fn relay(runner: &str, input: &str, limit: usize, event: Option<&str>) -> i32 {
    let normalized = normalize_input(runner, input);
    let raw = raw_event(input);
    let text = match std::env::current_exe() {
        Ok(exe) => {
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("hook").arg("--limit").arg(limit.to_string());
            cmd.env(crate::rewrite::RELAY_VAR, runner);
            if let Some(e) = event {
                cmd.arg("--event").arg(e);
            }
            run_inner(cmd, &normalized, &raw, relay_timeout(&raw))
        }
        Err(e) => fault_output(&raw, &format!("could not find its own binary ({e})")),
    };
    let (mut out, err, code) = answer(runner, &raw, &text);
    if let Some(line) = first_prompt_start_line(runner, &raw, input) {
        out = with_start_line(&out, &line);
    }
    print!("{out}");
    eprint!("{err}");
    code
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
            // The plugin's hooks file is the seat's whole.
            let want = json!({ "hooks": {
                "PreToolUse": [{ "matcher": "shell", "hooks": [{
                    "type": "command", "command": command, "timeout": 20
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

    const WARNED: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"warning: no force push (ljos rule `git-force-push`)"}}"#;
    const DENY: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"no force push (ljos rule `ljos-policyd`)"}}"#;
    const OLD_ASK: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"a push"}}"#;
    const OLD_BLOCK: &str = r#"{"decision":"block","reason":"no rm -rf"}"#;
    const NOTE: &str = r#"{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"lesson: run the tests"}}"#;

    /// Whether a runner's answer carries any decision but the allow it
    /// takes from no answer at all.
    fn decides(stdout: &str, stderr: &str) -> bool {
        let body = format!("{stdout}{stderr}");
        [
            "\"deny\"",
            "\"ask\"",
            "\"block\"",
            "\"cancel\":true",
            "permissionDecision\"",
        ]
        .iter()
        .any(|word| body.contains(word))
    }

    fn payload(runner: &str, input: Value) -> Value {
        serde_json::from_str(&normalize_input(runner, &input.to_string())).unwrap()
    }

    #[test]
    fn each_runner_payload_reads_as_the_command_it_runs() {
        let cases = [
            (
                "windsurf",
                json!({"agent_action_name": "pre_run_command", "trajectory_id": "task-0001",
                       "tool_info": {"command_line": "git push -f", "cwd": "/tmp"}}),
            ),
            (
                "cline",
                json!({"hookName": "PreToolUse", "taskId": "task-0001", "workspaceRoots": ["/tmp"],
                       "preToolUse": {"toolName": "execute_command", "parameters": {"command": "git push -f"}}}),
            ),
            (
                "cline",
                json!({"hookName": "tool_call", "taskId": "task-0001", "workspaceRoots": ["/tmp"],
                       "tool_call": {"id": "c", "name": "run_commands", "input": {"commands": ["git push -f"]}}}),
            ),
            (
                "copilot",
                json!({"sessionId": "task-0001", "timestamp": 1, "cwd": "/tmp", "toolName": "bash",
                       "toolArgs": "{\"command\": \"git push -f\"}"}),
            ),
            (
                "copilot",
                json!({"hook_event_name": "PreToolUse", "session_id": "task-0001", "cwd": "/tmp",
                       "tool_name": "Bash", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "gemini",
                json!({"hook_event_name": "BeforeTool", "session_id": "task-0001", "cwd": "/tmp",
                       "tool_name": "run_shell_command", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "crush",
                json!({"event": "PreToolUse", "session_id": "task-0001", "cwd": "/tmp",
                       "tool_name": "bash", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "kiro",
                json!({"hook_event_name": "PreToolUse", "session_id": "task-0001", "cwd": "/tmp",
                       "tool_name": "shell", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "factory",
                json!({"hook_event_name": "PreToolUse", "session_id": "task-0001", "cwd": "/tmp",
                       "tool_name": "Execute", "tool_input": {"command": "git push -f"}}),
            ),
            (
                "qwen",
                json!({"hook_event_name": "PreToolUse", "session_id": "task-0001", "cwd": "/tmp",
                       "tool_name": "run_shell_command", "tool_input": {"command": "git push -f"}}),
            ),
        ];
        for (runner, input) in cases {
            let v = payload(runner, input.clone());
            let call = crate::hook_call(&v.to_string());
            assert_eq!(call.event, "PreToolUse", "{runner}: {v}");
            assert_eq!(call.cue, "git push -f", "{runner}: {v}");
            assert_eq!(call.session.as_deref(), Some("task-0001"), "{runner}: {v}");
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
    fn a_rule_reaches_every_runner_as_an_allow_with_its_warning() {
        for runner in RUNNERS {
            let (out, err, _) = answer(runner, "PreToolUse", WARNED);
            assert!(!decides(&out, &err), "{runner}: {out}{err}");
            assert!(
                format!("{out}{err}").contains("warning: no force push"),
                "{runner}: {out}{err}"
            );
        }
        let (out, _, _) = answer("gemini", "BeforeTool", WARNED);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "BeforeTool");
        let (out, _, _) = answer("cline", "PreToolUse", WARNED);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["cancel"], json!(false));
        assert!(v["contextModification"]
            .as_str()
            .unwrap()
            .contains("git-force-push"));
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

        for runner in ["factory", "qwen"] {
            let (out, err, code) = answer(runner, "PreToolUse", DENY);
            assert_eq!(out.as_str(), DENY, "{runner}");
            assert_eq!(code, 2, "{runner}");
            assert!(err.contains("force push"), "{runner}: {err}");
        }

        let (out, _, _) = answer("cline", "PreToolUse", DENY);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["cancel"], json!(true));
        assert!(v["errorMessage"].as_str().unwrap().contains("force push"));

        let (out, err, code) = answer("crush", "PreToolUse", DENY);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["decision"], json!("deny"));
        assert_eq!(code, 2);
        assert!(err.contains("force push"));
    }

    /// A deny or a `block` on a tool call is every runner's deny, by its
    /// fields or its exit. On a stop, a `block` keeps the runner going and
    /// is no deny; an ask, which only an older hook prints, is a warning.
    #[test]
    fn a_tool_call_deny_is_each_runners_deny_and_nothing_else_is() {
        for runner in RUNNERS {
            for (refused, why) in [(DENY, "no force push"), (OLD_BLOCK, "no rm -rf")] {
                let (out, err, code) = answer(runner, "PreToolUse", refused);
                assert!(decides(&out, &err) || code == 2, "{runner}: {out}{err}");
                assert!(format!("{out}{err}").contains(why), "{runner}: {out}{err}");
            }
            for (old, event, why) in [
                (OLD_ASK, "PreToolUse", "warning: a push"),
                (OLD_BLOCK, "Stop", "warning: no rm -rf"),
            ] {
                let (out, err, code) = answer(runner, event, old);
                assert!(!decides(&out, &err) && code == 0, "{runner}: {out}{err}");
                assert!(format!("{out}{err}").contains(why), "{runner}: {out}{err}");
            }
        }
    }

    #[test]
    fn an_allow_says_nothing_and_a_note_reaches_runners_that_take_one() {
        for runner in ["copilot", "windsurf", "kiro", "crush"] {
            assert_eq!(
                answer(runner, "PreToolUse", ""),
                (String::new(), String::new(), 0)
            );
        }
        let (out, _, _) = answer("cline", "PreToolUse", "");
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap(),
            json!({"cancel": false})
        );
        assert_eq!(answer("goose", "UserPromptSubmit", NOTE).1, "");
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

    #[test]
    fn a_relay_fault_allows_and_warns_in_the_runners_contract() {
        for event in ["", "BeforeTool", "UserPromptSubmit", "BeforeAgent"] {
            for runner in RUNNERS {
                let (stdout, stderr) = relay_fault_answer(runner, event, "stdin is not utf-8");
                let body = format!("{stdout}{stderr}");
                assert!(!decides(&stdout, &stderr), "{runner} {event}: {body}");
                if shows_context(runner) {
                    assert!(body.contains("unhealthy"), "{runner}: {body}");
                    assert!(body.contains("stdin is not utf-8"), "{runner}: {body}");
                }
            }
        }
    }

    fn shows_context(runner: &str) -> bool {
        matches!(
            runner,
            "gemini" | "openhands" | "auggie" | "kiro" | "cline" | "crush" | "factory" | "qwen"
        )
    }

    fn allows_fault(out: &str, why: &str) {
        let said = read_answer(out);
        assert!(said.contains("unhealthy"), "{out}");
        assert!(said.contains(why), "{out}");
        for runner in RUNNERS {
            let (stdout, stderr, _) = answer(runner, "PreToolUse", out);
            let body = format!("{stdout}{stderr}");
            assert!(!decides(&stdout, &stderr), "{runner}: {body}");
            if shows_context(runner) {
                assert!(body.contains("unhealthy"), "{runner}: {body}");
                assert!(body.contains(why), "{runner}: {body}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_inner_hook_fault_allows_in_every_runner_shape() {
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
            allows_fault(&fault_output("PreToolUse", why), why);
        }
        allows_fault(&inner("exit 3", 30_000), "exited with status 3");
        let missing = run_inner(
            std::process::Command::new("/nonexistent/ljos"),
            "{}",
            "PreToolUse",
            std::time::Duration::from_secs(1),
        );
        allows_fault(&missing, "did not start");
        assert_eq!(INNER_TIMEOUT, crate::GUARD_TIMEOUT);
    }

    /// A wedged memory pack used to miss the prompt hook's own deadline:
    /// the relay waited five seconds, then denied the prompt with "while
    /// checking this command". Gemini, Qwen, Factory, OpenHands, goose and
    /// Cline all block on that deny. A prompt is not a tool call.
    #[test]
    fn a_prompt_fault_is_not_a_deny() {
        let _lock = crate::test_env_lock();
        let why = format!(
            "gave no answer within {} s",
            crate::GUARD_TIMEOUT.as_secs_f32()
        );
        let prompts = [
            ("gemini", "BeforeAgent"),
            ("qwen", "UserPromptSubmit"),
            ("factory", "UserPromptSubmit"),
            ("openhands", "UserPromptSubmit"),
            ("goose", "UserPromptSubmit"),
            ("cline", "UserPromptSubmit"),
        ];
        for (runner, event) in prompts {
            let fault = fault_output(event, &why);
            assert!(
                !fault.contains("while checking this command"),
                "{runner}: {fault}"
            );
            assert!(
                read_answer(&fault).contains("unhealthy"),
                "{runner}: {fault}"
            );
            let (stdout, stderr, _) = answer(runner, event, &fault);
            let body = format!("{stdout}{stderr}");
            assert!(!decides(&stdout, &stderr), "{runner}: {body}");
            assert!(
                !body.contains("while checking this command"),
                "{runner}: {body}"
            );
        }
        allows_fault(&fault_output("PreToolUse", &why), &why);
        allows_fault(&fault_output("BeforeTool", &why), &why);
        let deadline = std::time::Duration::from_millis(crate::HOOK_DEADLINE_MS);
        assert!(relay_timeout("UserPromptSubmit") >= deadline);
        assert!(relay_timeout("BeforeAgent") >= deadline);
        assert!(relay_timeout("pre_user_prompt") >= deadline);
        assert_eq!(relay_timeout("PreToolUse"), crate::guard_timeout());
        assert_eq!(relay_timeout("BeforeTool"), crate::guard_timeout());
        assert_eq!(relay_timeout(""), crate::guard_timeout());
    }

    #[cfg(unix)]
    #[test]
    fn a_prompt_hook_that_exits_early_is_not_denied() {
        let out = run_inner(
            sh("exit 3"),
            r#"{"hook_event_name":"UserPromptSubmit","prompt":"hello"}"#,
            "UserPromptSubmit",
            std::time::Duration::from_secs(2),
        );
        assert!(!out.contains("while checking this command"), "{out}");
        assert!(read_answer(&out).contains("unhealthy"), "{out}");
    }

    /// An allow that carries the unhealthy-guard sentence, alone or with a
    /// push warning joined on, stays that allow.
    #[cfg(unix)]
    #[test]
    fn an_unhealthy_note_is_not_a_second_deny() {
        let why = "did not answer in time";
        let exact = json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": crate::unhealthy_note(why),
            }
        })
        .to_string();
        let merged = json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": format!(
                    "{}\nthe tip has no passing test record",
                    crate::unhealthy_note(why)
                ),
            }
        })
        .to_string();
        for body in [exact, merged] {
            let out = inner(&format!("printf '%s' '{body}'"), 30_000);
            allows_fault(&out, why);
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_inner_hook_that_answers_keeps_its_answer() {
        assert_eq!(inner("true", 30_000), "", "a clean allow");
        assert_eq!(inner(&format!("printf '%s' '{WARNED}'"), 30_000), WARNED);
        let note = inner(&format!("printf '%s' '{WARNED}'; exit 1"), 30_000);
        allows_fault(&note, "exited with status 1");
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
    fn goose_payload_reaches_the_seat_and_a_rule_is_a_warning_on_stderr() {
        let n = normalize_input(
            "goose",
            r#"{"event":"PreToolUse","session_id":"g-1","tool_name":"shell","tool_input":{"command":"rm -rf /"},"working_dir":"/r"}"#,
        );
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["hook_event_name"], "PreToolUse");
        assert_eq!(v["tool_input"]["command"], "rm -rf /");
        assert_eq!(v["cwd"], "/r");
        assert_eq!(
            answer("goose", "PreToolUse", WARNED),
            (
                String::new(),
                "warning: no force push (ljos rule `git-force-push`)\n".into(),
                0
            )
        );
        assert_eq!(
            answer("goose", "PreToolUse", "{}"),
            (String::new(), String::new(), 0)
        );
    }

    #[test]
    fn amp_reads_cmd_and_a_rule_is_a_warning_on_stderr() {
        let n = normalize_input("amp", r#"{"cmd":"git push --force"}"#);
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["tool_input"]["command"], "git push --force");
        let (out, err, _) = answer("amp", "", WARNED);
        assert_eq!(out, "");
        assert!(err.starts_with("warning: no force push"), "{err}");
        assert_eq!(answer("amp", "", ""), (String::new(), String::new(), 0));
    }

    #[test]
    fn auggie_launch_process_is_the_shell_and_its_warning_is_claude_shaped() {
        let n = normalize_input(
            "auggie",
            r#"{"hook_event_name":"PreToolUse","tool_name":"launch-process","tool_input":{"command":"ls"}}"#,
        );
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["tool_name"], "Bash");
        let (out, _, _) = answer("auggie", "PreToolUse", WARNED);
        let o: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(o["hookSpecificOutput"]["permissionDecision"], Value::Null);
        assert!(o["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .starts_with("warning: no force push"));
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
    fn goose_hook_is_a_plugin_whose_failure_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ljos/hooks/hooks.json");
        let step = hook_step("goose", &file, "/bin/ljos hook", false);
        assert!(step.ok, "{}", step.detail);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["on_failure"],
            Value::Null
        );
        assert!(dir.path().join("ljos/plugin.json").is_file());
        assert!(hook_step("goose", &file, "/bin/ljos hook", false)
            .detail
            .contains("carries"));
    }

    #[test]
    fn openhands_terminal_is_the_shell_and_a_rule_is_context() {
        let n = normalize_input(
            "openhands",
            r#"{"event_type":"PreToolUse","tool_name":"terminal","tool_input":{"command":"rm -rf ~"},"working_dir":"/w"}"#,
        );
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["tool_name"], "Bash");
        assert_eq!(v["cwd"], "/w");
        let (out, _, _) = answer("openhands", "PreToolUse", WARNED);
        let o: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(o["decision"], Value::Null);
        assert!(o["additionalContext"]
            .as_str()
            .unwrap()
            .starts_with("warning: no force push"));
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
