//! A guard that fixes a command instead of only refusing it.
//!
//! ljos-policyd answers `allow\trewrite\tLINE` when the line should run as
//! LINE. A runner whose `PreToolUse` hook can replace the tool input gets
//! LINE back as that input. A runner without that field gets a note. The
//! local table gives the same fix for the refusals it knows, so the fix
//! works with a policyd that can only refuse.

use std::cell::RefCell;

use serde_json::{json, Value};

use crate::{HookCall, HookShape, Rule};

/// Set on the inner `ljos hook` a runner relay starts. The relay renders
/// a decision and a reason but no updated input, so an allow that carries
/// one would run the line as written.
pub const RELAY_VAR: &str = "LJOS_HOOK_RELAY";

/// The tool names a shell call arrives under: Claude Code and Codex
/// `Bash`, Cursor `Shell`, Grok Build `run_terminal_command`.
const SHELL_TOOLS: &[&str] = &["Bash", "Shell", "run_terminal_command"];

const POLICYD_REASON: &str = "ljos-policyd rewrote this line";

/// Characters that make a line more than one simple command. The local
/// table leaves such a line to the refusal.
const SHELL_META: &[char] = &[
    ';', '&', '|', '<', '>', '$', '`', '\\', '(', ')', '{', '}', '\n', '\r',
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    pub command: String,
    pub reason: String,
    /// Whether the guard allowed the line as written. A refused line that
    /// the runner cannot rewrite stays refused.
    pub allowed: bool,
}

/// The new line in a policyd rewrite answer, `allow\trewrite\tLINE`.
/// This is the one place that format is read.
#[must_use]
pub fn from_policyd(text: &str) -> Option<String> {
    let mut parts = text.trim().lines().next()?.splitn(3, '\t');
    if parts.next()? != "allow" || parts.next()? != "rewrite" {
        return None;
    }
    let line = parts.next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

thread_local! {
    static HELD: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// Keep policyd's rewrite of `line` for the hook that asked about it.
pub(crate) fn hold(line: &str, command: String) {
    HELD.with(|held| *held.borrow_mut() = Some((line.to_string(), command)));
}

fn take_held(line: &str) -> Option<String> {
    HELD.with(|held| held.borrow_mut().take())
        .filter(|(held, _)| held == line)
        .map(|(_, command)| command)
}

struct Fix {
    /// The refusal codes this row answers.
    codes: &'static [&'static str],
    /// The interpreter, with or without a version suffix or a directory.
    program: &'static str,
    /// What the first argument ends with.
    script: &'static str,
    /// What runs the script instead of the interpreter.
    run: &'static str,
    reason: &'static str,
}

const LOCAL: &[Fix] = &[Fix {
    codes: &["python-requires-uv", "python-requires-uv-run"],
    program: "python",
    script: ".py",
    run: "uv run",
    reason: "a Python script runs with `uv run`, which reads its PEP 723 block",
}];

fn is_program(word: &str, program: &str) -> bool {
    let name = word.rsplit('/').next().unwrap_or(word);
    name.strip_prefix(program)
        .is_some_and(|version| version.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// The local table's fix for `line`, refused with `code`: `python x.py
/// ARGS` becomes `uv run x.py ARGS`, the arguments kept as written.
#[must_use]
pub fn local(code: &str, line: &str) -> Option<Rewrite> {
    let line = line.trim();
    if line.contains(SHELL_META) {
        return None;
    }
    let (program, rest) = line.split_once(char::is_whitespace)?;
    let rest = rest.trim_start();
    let script = rest.split_whitespace().next()?;
    if script.starts_with('-') || script.contains(['\'', '"']) {
        return None;
    }
    LOCAL
        .iter()
        .find(|fix| {
            fix.codes.contains(&code)
                && is_program(program, fix.program)
                && script.ends_with(fix.script)
        })
        .map(|fix| Rewrite {
            command: format!("{} {rest}", fix.run),
            reason: fix.reason.to_string(),
            allowed: false,
        })
}

/// The rewrite for a tool call: policyd's for a line it allowed, else the
/// local table's for a policyd refusal it knows. `judge` reads the new
/// line as the guard would, and a new line it refuses is no rewrite.
pub fn pick(
    call: &HookCall,
    verdict: Option<&Rule>,
    judge: impl Fn(&str) -> Option<Rule>,
) -> Option<Rewrite> {
    if call.event != "PreToolUse" {
        return None;
    }
    let held = take_held(&call.cue);
    let fix = match verdict {
        None => held.map(|command| Rewrite {
            command,
            reason: POLICYD_REASON.to_string(),
            allowed: true,
        }),
        Some(rule) if rule.verdict == "deny" && rule.pattern == "ljos-policyd" => {
            let code = rule.reason.split(':').next().unwrap_or("").trim();
            local(code, &call.cue)
        }
        Some(_) => None,
    }?;
    (fix.command != call.cue.trim() && judge(&fix.command).is_none()).then_some(fix)
}

/// The hook's answer when the call has a rewrite, else None and the
/// usual answer stands.
pub fn answer(
    input: &str,
    call: &HookCall,
    context: &str,
    verdict: Option<&Rule>,
    judge: impl Fn(&str) -> Option<Rule>,
) -> Option<String> {
    let fix = pick(call, verdict, judge)?;
    let relayed = std::env::var_os(RELAY_VAR).is_some();
    Some(answer_with(input, call, context, verdict, &fix, relayed))
}

/// [`answer`] for a known fix: the updated input where the runner takes
/// one, else the note.
#[must_use]
pub fn answer_with(
    input: &str,
    call: &HookCall,
    context: &str,
    verdict: Option<&Rule>,
    fix: &Rewrite,
    relayed: bool,
) -> String {
    if !relayed {
        if let Some(out) = updated_input(input, call, context, fix) {
            return out;
        }
    }
    note(input, call, context, verdict, fix)
}

fn joined(context: &str, line: &str) -> String {
    if context.is_empty() {
        line.to_string()
    } else {
        format!("{context}\n{line}")
    }
}

/// Claude Code, Codex and Grok Build read `hookSpecificOutput.updatedInput`
/// beside `permissionDecision: allow`; Cursor's `preToolUse` reads
/// `updated_input` beside `permission: allow`. Each replaces the whole
/// input, so every other field the runner sent goes back unchanged.
fn updated_input(input: &str, call: &HookCall, context: &str, fix: &Rewrite) -> Option<String> {
    let v: Value = serde_json::from_str(input.trim()).ok()?;
    // Grok Build cut the input short, so it cannot be sent back whole.
    if v["toolInputTruncated"].as_bool() == Some(true) {
        return None;
    }
    let tool = v["tool_name"].as_str().or_else(|| v["toolName"].as_str())?;
    if !SHELL_TOOLS.contains(&tool) {
        return None;
    }
    let mut given = if v["tool_input"].is_object() {
        v["tool_input"].clone()
    } else {
        v["toolInput"].clone()
    };
    let fields = given.as_object_mut()?;
    if fields.get("command").and_then(Value::as_str).map(str::trim) != Some(call.cue.trim()) {
        return None;
    }
    fields.insert("command".into(), json!(fix.command));
    let said = format!(
        "ljos ran `{}` in place of `{}`: {}",
        fix.command,
        call.cue.trim(),
        fix.reason
    );
    let out = match call.shape {
        HookShape::Cursor => json!({
            "permission": "allow",
            "updated_input": given,
            "agent_message": said,
        }),
        HookShape::Asks | HookShape::DenyOnly | HookShape::CamelCase => {
            let mut specific = json!({
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": given,
                "additionalContext": joined(context, &said),
            });
            // Codex fails a hook on a field it does not take, and then
            // runs the line as written.
            if call.shape != HookShape::DenyOnly {
                specific["permissionDecisionReason"] = json!(said);
            }
            let mut out = json!({ "hookSpecificOutput": specific });
            if call.shape == HookShape::CamelCase {
                out["decision"] = json!("allow");
            }
            out
        }
        _ => return None,
    };
    Some(out.to_string() + "\n")
}

/// The answer for a runner that cannot take the new line. A line the
/// guard allowed runs as written and the agent reads the new line. A line
/// it refused stays refused, and the refusal names the new line.
fn note(
    input: &str,
    call: &HookCall,
    context: &str,
    verdict: Option<&Rule>,
    fix: &Rewrite,
) -> String {
    match verdict.filter(|_| !fix.allowed) {
        Some(rule) => {
            let mut refused = rule.clone();
            refused.reason = format!(
                "{}. Run `{}` instead: {}",
                rule.reason, fix.command, fix.reason
            );
            crate::approval::hook_output(input, call, context, Some(&refused))
        }
        None => {
            let said = format!(
                "{}: this runner cannot take a new command, so it ran as written. Run `{}` next time",
                fix.reason, fix.command
            );
            if call.shape.is_cursor() {
                return json!({ "permission": "allow", "agent_message": said }).to_string() + "\n";
            }
            crate::hook_output_ruled(call, &joined(context, &said), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook_call;

    fn uv_refusal() -> Rule {
        Rule {
            pattern: "ljos-policyd".into(),
            verdict: "deny".into(),
            reason: crate::policyd_reason("python-requires-uv"),
        }
    }

    fn allow_all(_: &str) -> Option<Rule> {
        None
    }

    fn answer_json(input: &str, verdict: Option<&Rule>, relayed: bool) -> Value {
        let call = hook_call(input);
        let fix = pick(&call, verdict, allow_all).expect("a rewrite");
        let out = answer_with(input, &call, "", verdict, &fix, relayed);
        serde_json::from_str(out.trim()).unwrap()
    }

    const CLAUDE: &str = r#"{"session_id":"s-1","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"python x.py --n 3","description":"run it","timeout":60000},"cwd":"/w"}"#;
    const CODEX: &str = r#"{"session_id":"s-1","turn_id":"t-1","hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"c-1","tool_input":{"command":"python x.py --n 3"},"cwd":"/w"}"#;
    const CURSOR_TOOL: &str = r#"{"cursor_version":"3.2.0","conversation_id":"conv-1","hook_event_name":"preToolUse","tool_name":"Shell","tool_input":{"command":"python x.py --n 3","working_directory":"/w"},"tool_use_id":"u-1","cwd":"/w"}"#;
    const CURSOR_SHELL: &str = r#"{"cursor_version":"3.2.0","conversation_id":"conv-1","hook_event_name":"beforeShellExecution","command":"python x.py --n 3","cwd":"/w","sandbox":false}"#;
    const GROK: &str = r#"{"hookEventName":"pre_tool_use","hook_event_name":"PreToolUse","sessionId":"g-1","cwd":"/w","workspaceRoot":"/w","permissionMode":"default","toolName":"run_terminal_command","toolInput":{"command":"python x.py --n 3"},"toolUseId":"u-1","toolInputTruncated":false}"#;

    #[test]
    fn the_policyd_rewrite_line_is_read_and_nothing_else_is() {
        assert_eq!(
            from_policyd("allow\trewrite\tuv run x.py\n").as_deref(),
            Some("uv run x.py")
        );
        assert_eq!(
            from_policyd("allow\trewrite\tuv run x.py --n 3").as_deref(),
            Some("uv run x.py --n 3")
        );
        for text in [
            "allow",
            "allow\trewrite\t",
            "allow\trewrite",
            "deny\trewrite\tuv run x.py",
            "allow\tfail-open\tthe ljos guard is unhealthy",
            "allow\tthe ljos guard is unhealthy (did not start)",
            "deny\tpython-requires-uv",
            r#"{"decision":"rewrite","command":"uv run x.py"}"#,
        ] {
            assert_eq!(from_policyd(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_local_table_runs_a_python_script_with_uv() {
        let fix = |line: &str| local("python-requires-uv", line).map(|f| f.command);
        assert_eq!(fix("python x.py").as_deref(), Some("uv run x.py"));
        assert_eq!(fix("python3 x.py").as_deref(), Some("uv run x.py"));
        assert_eq!(
            fix("python3.12 scripts/x.py").as_deref(),
            Some("uv run scripts/x.py")
        );
        assert_eq!(fix("/usr/bin/python3 x.py").as_deref(), Some("uv run x.py"));
        assert_eq!(
            fix("python x.py --name 'a b'").as_deref(),
            Some("uv run x.py --name 'a b'")
        );
        assert_eq!(
            local("python-requires-uv-run", "python x.py")
                .map(|f| f.command)
                .as_deref(),
            Some("uv run x.py")
        );
        for line in [
            "python",
            "python -c 'print(1)'",
            "python -m pytest",
            "python x.txt",
            "pythonista x.py",
            "python x.py && rm -rf /",
            "python x.py | tee out",
            "python x.py > out",
            "python $(echo x.py)",
            "FOO=1 python x.py",
            "uv run x.py",
        ] {
            assert_eq!(fix(line), None, "{line}");
        }
        assert_eq!(local("rm-rf-outside-tmp", "python x.py"), None);
    }

    #[test]
    fn claude_code_gets_the_new_line_as_updated_input() {
        let v = answer_json(CLAUDE, Some(&uv_refusal()), false);
        let specific = &v["hookSpecificOutput"];
        assert_eq!(specific["hookEventName"], "PreToolUse");
        assert_eq!(specific["permissionDecision"], "allow");
        assert_eq!(
            specific["updatedInput"],
            json!({"command":"uv run x.py --n 3","description":"run it","timeout":60000})
        );
        let said = specific["additionalContext"].as_str().unwrap();
        assert!(
            said.contains("`uv run x.py --n 3` in place of `python x.py --n 3`"),
            "{said}"
        );
        assert!(v.get("decision").is_none());
    }

    #[test]
    fn codex_gets_updated_input_and_no_field_it_would_fail_on() {
        let v = answer_json(CODEX, Some(&uv_refusal()), false);
        let specific = &v["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "allow");
        assert_eq!(
            specific["updatedInput"],
            json!({"command":"uv run x.py --n 3"})
        );
        assert!(specific.get("permissionDecisionReason").is_none());
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["hookSpecificOutput"]);
    }

    #[test]
    fn cursor_pre_tool_use_gets_updated_input() {
        let v = answer_json(CURSOR_TOOL, Some(&uv_refusal()), false);
        assert_eq!(v["permission"], "allow");
        assert_eq!(
            v["updated_input"],
            json!({"command":"uv run x.py --n 3","working_directory":"/w"})
        );
        assert!(v.get("hookSpecificOutput").is_none());
    }

    #[test]
    fn cursor_before_shell_execution_refuses_with_the_exact_line() {
        let v = answer_json(CURSOR_SHELL, Some(&uv_refusal()), false);
        assert_eq!(v["permission"], "deny");
        let said = v["agent_message"].as_str().unwrap();
        assert!(said.contains("Run `uv run x.py --n 3` instead"), "{said}");
        assert!(said.starts_with("python-requires-uv: "), "{said}");
        assert!(v.get("updated_input").is_none());
    }

    #[test]
    fn grok_build_gets_updated_input_unless_its_input_was_cut() {
        let v = answer_json(GROK, Some(&uv_refusal()), false);
        assert_eq!(v["decision"], "allow");
        let specific = &v["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "allow");
        assert_eq!(
            specific["updatedInput"],
            json!({"command":"uv run x.py --n 3"})
        );
        let cut = GROK.replace(
            r#""toolInputTruncated":false"#,
            r#""toolInputTruncated":true"#,
        );
        let v = answer_json(&cut, Some(&uv_refusal()), false);
        assert_eq!(v["decision"], "deny");
        assert!(v["reason"]
            .as_str()
            .unwrap()
            .contains("Run `uv run x.py --n 3` instead"));
        assert!(v["hookSpecificOutput"].get("updatedInput").is_none());
    }

    #[test]
    fn a_relayed_runner_keeps_the_refusal_with_the_line_in_it() {
        let v = answer_json(CLAUDE, Some(&uv_refusal()), true);
        let specific = &v["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "deny");
        assert!(specific.get("updatedInput").is_none());
        let reason = specific["permissionDecisionReason"].as_str().unwrap();
        assert!(
            reason.contains("Run `uv run x.py --n 3` instead"),
            "{reason}"
        );
    }

    #[test]
    fn a_policyd_rewrite_of_an_allowed_line_is_a_note_where_input_cannot_change() {
        let line = "python x.py --n 3";
        hold(line, "uv run x.py --n 3".into());
        let v = answer_json(CLAUDE, None, false);
        assert_eq!(
            v["hookSpecificOutput"]["updatedInput"]["command"],
            "uv run x.py --n 3"
        );

        hold(line, "uv run x.py --n 3".into());
        let v = answer_json(CLAUDE, None, true);
        let specific = &v["hookSpecificOutput"];
        assert!(specific.get("permissionDecision").is_none());
        let said = specific["additionalContext"].as_str().unwrap();
        assert!(said.starts_with(POLICYD_REASON), "{said}");
        assert!(said.contains("Run `uv run x.py --n 3` next time"), "{said}");

        hold(line, "uv run x.py --n 3".into());
        let v = answer_json(CURSOR_SHELL, None, false);
        assert_eq!(v["permission"], "allow");
        assert!(v["agent_message"]
            .as_str()
            .unwrap()
            .contains("uv run x.py --n 3"));
    }

    #[test]
    fn no_rewrite_without_a_fix_the_guard_also_allows() {
        let call = hook_call(CLAUDE);
        assert!(pick(&call, None, allow_all).is_none(), "nothing held");
        hold("some other line", "uv run x.py".into());
        assert!(
            pick(&call, None, allow_all).is_none(),
            "held for another line"
        );
        let refuse = |_: &str| Some(uv_refusal());
        assert!(pick(&call, Some(&uv_refusal()), refuse).is_none());
        let ask = Rule {
            pattern: "git-force-push".into(),
            verdict: "ask".into(),
            reason: "a force push".into(),
        };
        hold("python x.py --n 3", "uv run x.py --n 3".into());
        assert!(pick(&call, Some(&ask), allow_all).is_none());
        assert!(
            take_held("python x.py --n 3").is_none(),
            "a refusal drops the held line"
        );
        let prompt = hook_call(r#"{"hook_event_name":"UserPromptSubmit","prompt":"python x.py"}"#);
        assert!(pick(&prompt, Some(&uv_refusal()), allow_all).is_none());
        let read = r#"{"hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"command":"python x.py --n 3"}}"#;
        let call = hook_call(read);
        let fix = pick(&call, Some(&uv_refusal()), allow_all).unwrap();
        let v: Value = serde_json::from_str(
            answer_with(read, &call, "", Some(&uv_refusal()), &fix, false).trim(),
        )
        .unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    }

    /// A policyd that answers the rewrite line allows the call, and the
    /// hook finds the new line held for it.
    #[test]
    fn the_guard_holds_a_policyd_rewrite_for_the_hook() {
        use std::os::unix::fs::PermissionsExt as _;
        let _g = crate::tests::env_guard();
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("policyd");
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"$5\" in python*) printf 'allow\\trewrite\\tuv run x.py\\n' ;; *) echo allow ;; esac\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let keys = [
            "HOME",
            "POLICYD_BIN",
            "PACKSET_URL",
            "LJOS_OFF",
            "LJOS_GUARD",
            "POLICYD_REQUIRED",
            "LJOS_GUARD_FAIL_OPEN",
        ];
        let saved: Vec<_> = keys.iter().map(|k| (*k, std::env::var_os(k))).collect();
        unsafe {
            std::env::set_var("HOME", home.path());
            std::env::set_var("POLICYD_BIN", &bin);
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("LJOS_OFF", "0");
            std::env::set_var("LJOS_GUARD", "on");
            std::env::remove_var("POLICYD_REQUIRED");
            std::env::remove_var("LJOS_GUARD_FAIL_OPEN");
        }
        let line = "python x.py";
        let verdict = crate::tcb_verdict_in(line, Some(home.path()));
        let held = take_held(line);
        unsafe {
            for (k, v) in saved {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        assert!(verdict.is_none(), "{verdict:?}");
        assert_eq!(held.as_deref(), Some("uv run x.py"));
    }
}
