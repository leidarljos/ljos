//! A guard that fixes a command instead of only warning about it.
//!
//! ljos-policyd answers `allow\trewrite\tLINE` when the line should run as
//! LINE. The guard reads LINE again. A runner whose `PreToolUse` hook can
//! replace the tool input gets LINE back as that input. A runner without
//! that field, or a LINE the guard does not take as it stands, runs the
//! original line with a warning that names LINE.
//!
//! The local table names a fix in a policyd warning it knows. That call
//! runs as written, and the warning names the fix.

use std::cell::RefCell;

use serde_json::{json, Value};

use crate::{HookCall, HookShape, Rule};

/// Set on the inner `ljos hook` a runner relay starts. The relay renders
/// a decision and a reason but no updated input, so an allow that carries
/// one would run the line as written.
pub const RELAY_VAR: &str = "LJOS_HOOK_RELAY";

/// The tool names a shell call arrives under: The terminal harness and the command harness
/// `Bash`, the editor harness `Shell`, the build harness `run_terminal_command`.
const SHELL_TOOLS: &[&str] = &["Bash", "Shell", "run_terminal_command"];

const POLICYD_REASON: &str = "ljos-policyd rewrote this line";

/// Characters that make a line more than one simple command. The local
/// table leaves such a line to the warning as it came.
const SHELL_META: &[char] = &[
    ';', '&', '|', '<', '>', '$', '`', '\\', '(', ')', '{', '}', '\n', '\r',
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    pub command: String,
    pub reason: String,
}

/// What the hook does with a call that has a rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Run this line, which the guard judged and accepted, in place of
    /// the call's line.
    Run(Rewrite),
    /// Run the call's own line, with this rule's warning.
    Warn(Rule),
}

/// The new line in a policyd rewrite answer, `allow\trewrite\tLINE`.
/// This is the one place that format is read.
///
/// ljos asks about `bash -c SCRIPT`, and policyd 0.6.0 answers with that
/// argv rewritten and quoted: `bash -c 'uv run x.py'`. That one wrapper
/// is ljos's own and is taken off. A LINE in any other shape is the line.
#[must_use]
pub fn from_policyd(text: &str) -> Option<String> {
    let mut parts = text.trim().lines().next()?.splitn(3, '\t');
    if parts.next()? != "allow" || parts.next()? != "rewrite" {
        return None;
    }
    let line = parts.next()?.trim();
    let line = match crate::shell_words(line).as_slice() {
        [shell, flag, script] if shell == "bash" && flag == "-c" => script.trim().to_string(),
        _ => line.to_string(),
    };
    (!line.is_empty()).then_some(line)
}

/// Hold policyd's rewrite of `line`, which it judged as `script`. The
/// script can differ from the line: a here-document body is taken out, or
/// a continuation is joined. Then the rewrite is inexact, so the line
/// runs as written with a warning that names the rewrite.
pub(crate) fn hold_exact(line: &str, script: &str, command: String) -> Option<Rule> {
    if script.trim() == line.trim() {
        hold(line, command);
        return None;
    }
    Some(Rule {
        pattern: "ljos-policyd".into(),
        verdict: "warn".into(),
        reason: format!(
            "{POLICYD_REASON} to `{command}`, but it judged a copy with its here-documents \
             or continuations changed, so the new line is not exact. The line runs as written"
        ),
    })
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
    /// The policyd codes this row answers.
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

/// The local table's fix for `line`, warned on with `code`: `python x.py
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
        })
}

/// Why the guard does not take `line` as it stands, or None when it
/// does. `judge` reads a line as the guard would and holds a rewrite as
/// [`crate::tcb_verdict_in`] does, so a line rewritten again is not taken.
fn doubt_on(line: &str, judge: impl Fn(&str) -> Option<Rule>) -> Option<String> {
    let _ = take_held(line);
    if let Some(rule) = judge(line) {
        return Some(format!("the guard warns on it: {}", rule.reason));
    }
    take_held(line).map(|again| format!("the guard rewrites it again, to `{again}`"))
}

/// What the hook does with a tool call that has a rewrite, or None when
/// it has none and the usual answer stands.
///
/// A policyd rewrite runs only when the guard takes the new line as it
/// stands. Otherwise the call's own line runs with a warning. A local fix
/// is named in the policyd warning when the guard takes that line.
pub fn pick(
    call: &HookCall,
    verdict: Option<&Rule>,
    judge: impl Fn(&str) -> Option<Rule>,
) -> Option<Outcome> {
    if call.event != "PreToolUse" {
        return None;
    }
    let held = take_held(&call.cue);
    let line = call.cue.trim();
    match verdict {
        None => {
            let command = held.filter(|command| command != line)?;
            Some(match doubt_on(&command, judge) {
                None => Outcome::Run(Rewrite {
                    command,
                    reason: POLICYD_REASON.to_string(),
                }),
                Some(why) => Outcome::Warn(Rule {
                    pattern: "ljos-policyd".into(),
                    verdict: "warn".into(),
                    reason: format!(
                        "{POLICYD_REASON} to `{command}`, and {why}. The line runs as written"
                    ),
                }),
            })
        }
        Some(rule) if rule.pattern == "ljos-policyd" => {
            let code = rule.reason.split(':').next().unwrap_or("").trim();
            let fix = local(code, line).filter(|fix| fix.command != line)?;
            doubt_on(&fix.command, judge).is_none().then(|| {
                let mut warned = rule.clone();
                warned.reason = format!(
                    "{}. Run `{}` instead: {}",
                    rule.reason, fix.command, fix.reason
                );
                Outcome::Warn(warned)
            })
        }
        Some(_) => None,
    }
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
    let outcome = pick(call, verdict, judge)?;
    let relayed = std::env::var_os(RELAY_VAR).is_some();
    Some(answer_with(input, call, context, &outcome, relayed))
}

/// [`answer`] for a known outcome. A warning is the usual allow with that
/// line. A line to run is the updated input where the runner takes one. A
/// runner that cannot take one runs the original line, with a warning
/// that names the new one.
#[must_use]
pub fn answer_with(
    input: &str,
    call: &HookCall,
    context: &str,
    outcome: &Outcome,
    relayed: bool,
) -> String {
    let warn = |rule: &Rule| crate::hook_output_ruled(call, context, Some(rule));
    match outcome {
        Outcome::Warn(rule) => warn(rule),
        Outcome::Run(fix) => (!relayed)
            .then(|| updated_input(input, call, context, fix))
            .flatten()
            .unwrap_or_else(|| warn(&unchangeable(fix))),
    }
}

/// The warning for a runner whose hook cannot replace the command.
fn unchangeable(fix: &Rewrite) -> Rule {
    Rule {
        pattern: "ljos-policyd".into(),
        verdict: "warn".into(),
        reason: format!(
            "{}. Run `{}` instead. This runner cannot take a new command, so the original \
             line runs as written",
            fix.reason, fix.command
        ),
    }
}

fn joined(context: &str, line: &str) -> String {
    if context.is_empty() {
        line.to_string()
    } else {
        format!("{context}\n{line}")
    }
}

/// The terminal, command and build harnesses read `hookSpecificOutput.updatedInput`
/// beside `permissionDecision: allow`; the editor harness's `preToolUse` reads
/// `updated_input` beside `permission: allow`. Each replaces the whole
/// input, so every other field the runner sent goes back unchanged.
fn updated_input(input: &str, call: &HookCall, context: &str, fix: &Rewrite) -> Option<String> {
    let v: Value = serde_json::from_str(input.trim()).ok()?;
    // the build harness cut the input short, so it cannot be sent back whole.
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
            // the command harness fails a hook on a field it does not take, and then
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook_call;

    fn uv_warning() -> Rule {
        Rule {
            pattern: "ljos-policyd".into(),
            verdict: "warn".into(),
            reason: crate::policyd_reason("python-requires-uv"),
        }
    }

    fn allow_all(_: &str) -> Option<Rule> {
        None
    }

    fn answer_json(input: &str, outcome: &Outcome, relayed: bool) -> Value {
        let out = answer_with(input, &hook_call(input), "", outcome, relayed);
        serde_json::from_str(out.trim()).unwrap()
    }

    /// The answer when policyd rewrote the call's line to `uv run x.py --n
    /// 3` and the guard accepts that line.
    fn policyd_run(input: &str, relayed: bool) -> Value {
        let call = hook_call(input);
        hold(&call.cue, "uv run x.py --n 3".into());
        let outcome = pick(&call, None, allow_all).expect("a rewrite");
        assert!(matches!(outcome, Outcome::Run(_)), "{outcome:?}");
        answer_json(input, &outcome, relayed)
    }

    /// The answer when policyd warned on the call for want of uv.
    fn local_warning(input: &str) -> Value {
        let call = hook_call(input);
        let outcome = pick(&call, Some(&uv_warning()), allow_all).expect("a fix");
        assert!(matches!(outcome, Outcome::Warn(_)), "{outcome:?}");
        answer_json(input, &outcome, false)
    }

    /// Whether a runner reads the answer as holding the call, whichever
    /// field carries the decision.
    fn holds(v: &Value) -> bool {
        let decision = v["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .or_else(|| v["permission"].as_str())
            .or_else(|| v["decision"].as_str());
        matches!(decision, Some("deny" | "ask" | "block"))
    }

    const CLAUDE: &str = r#"{"session_id":"s-1","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"python x.py --n 3","description":"run it","timeout":60000},"cwd":"/w"}"#;
    const CODEX: &str = r#"{"session_id":"s-1","turn_id":"t-1","hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"c-1","tool_input":{"command":"python x.py --n 3"},"cwd":"/w"}"#;
    const CURSOR_TOOL: &str = r#"{"cursor_version":"3.2.0","conversation_id":"conv-1","hook_event_name":"preToolUse","tool_name":"Shell","tool_input":{"command":"python x.py --n 3","working_directory":"/w"},"tool_use_id":"u-1","cwd":"/w"}"#;
    const CURSOR_SHELL: &str = r#"{"cursor_version":"3.2.0","conversation_id":"conv-1","hook_event_name":"beforeShellExecution","command":"python x.py --n 3","cwd":"/w","sandbox":false}"#;
    const GROK: &str = r#"{"hookEventName":"pre_tool_use","hook_event_name":"PreToolUse","sessionId":"g-1","cwd":"/w","workspaceRoot":"/w","permissionMode":"default","toolName":"run_terminal_command","toolInput":{"command":"python x.py --n 3"},"toolUseId":"u-1","toolInputTruncated":false}"#;
    const READ: &str = r#"{"session_id":"s-1","hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":"/w/x.py","command":"python x.py --n 3"},"cwd":"/w"}"#;
    const RUNNERS: &[&str] = &[CLAUDE, CODEX, CURSOR_TOOL, CURSOR_SHELL, GROK];

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
        // policyd 0.6.0 answers with the `bash -c` argv ljos sent, quoted.
        for (text, line) in [
            ("allow\trewrite\tbash -c 'uv run x.py'\n", "uv run x.py"),
            (
                "allow\trewrite\tbash -c 'uv run x.py --n 3'",
                "uv run x.py --n 3",
            ),
            (
                "allow\trewrite\tbash -c 'uv run '\\''a b.py'\\'''",
                "uv run 'a b.py'",
            ),
            (
                "allow\trewrite\tbash -c \"uv run x.py > out\"",
                "uv run x.py > out",
            ),
            ("allow\trewrite\tbash -c 'x' extra", "bash -c 'x' extra"),
            ("allow\trewrite\tsh -c 'uv run x.py'", "sh -c 'uv run x.py'"),
        ] {
            assert_eq!(from_policyd(text).as_deref(), Some(line), "{text:?}");
        }
        for text in [
            "allow\trewrite\tbash -c ''",
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
    fn claude_code_gets_an_accepted_rewrite_as_updated_input() {
        let v = policyd_run(CLAUDE, false);
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
    fn codex_gets_an_accepted_rewrite_and_no_field_it_would_fail_on() {
        let v = policyd_run(CODEX, false);
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
    fn cursor_pre_tool_use_gets_an_accepted_rewrite_as_updated_input() {
        let v = policyd_run(CURSOR_TOOL, false);
        assert_eq!(v["permission"], "allow");
        assert_eq!(
            v["updated_input"],
            json!({"command":"uv run x.py --n 3","working_directory":"/w"})
        );
        assert!(v.get("hookSpecificOutput").is_none());
    }

    #[test]
    fn grok_build_gets_updated_input_unless_its_input_was_cut() {
        let v = policyd_run(GROK, false);
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
        let v = policyd_run(&cut, false);
        assert!(!holds(&v), "{v}");
        assert!(v["hookSpecificOutput"].get("updatedInput").is_none());
        assert!(
            v.to_string().contains("Run `uv run x.py --n 3` instead"),
            "{v}"
        );
    }

    /// A runner that cannot take a new command runs the original line,
    /// with a warning that names the new one: the editor harness's
    /// `beforeShellExecution`, and every runner behind the relay.
    #[test]
    fn an_accepted_rewrite_is_a_warning_where_input_cannot_change() {
        let cases = [
            (CURSOR_SHELL, false),
            (CLAUDE, true),
            (CODEX, true),
            (GROK, true),
            (CURSOR_TOOL, true),
        ];
        for (input, relayed) in cases {
            let v = policyd_run(input, relayed);
            assert!(!holds(&v), "{input} relayed={relayed}\n{v}");
            assert!(v["hookSpecificOutput"].get("updatedInput").is_none(), "{v}");
            assert!(v.get("updated_input").is_none(), "{v}");
            let said = v.to_string();
            assert!(said.contains("Run `uv run x.py --n 3` instead"), "{said}");
            assert!(said.contains("line runs as written"), "{said}");
        }
    }

    /// A rewrite of a copy that is not the line cannot be applied exactly,
    /// so the line runs as written with a warning and nothing is held.
    #[test]
    fn a_rewrite_of_a_changed_copy_warns_and_runs_the_line() {
        let line = "python x.py <<EOF\nbody\nEOF";
        let rule = hold_exact(line, "python x.py <<EOF\nEOF", "uv run x.py".into())
            .expect("a changed copy warns");
        assert!(
            rule.reason.contains("The line runs as written"),
            "{}",
            rule.reason
        );
        assert!(take_held(line).is_none());
        assert!(hold_exact("python x.py", "python x.py", "uv run x.py".into()).is_none());
        assert_eq!(take_held("python x.py").as_deref(), Some("uv run x.py"));
    }

    /// A local fix is named in the warning, and the line runs as written:
    /// no runner gets an updated input it might drop.
    #[test]
    fn a_local_fix_is_a_warning_on_every_runner() {
        for input in RUNNERS {
            let v = local_warning(input);
            assert!(!holds(&v), "{input}\n{v}");
            assert!(v["hookSpecificOutput"].get("updatedInput").is_none(), "{v}");
            assert!(v.get("updated_input").is_none(), "{v}");
            let said = v.to_string();
            assert!(said.contains("Run `uv run x.py --n 3` instead"), "{said}");
            assert!(said.contains("python-requires-uv: "), "{said}");
        }
        let v = local_warning(CODEX);
        let specific = &v["hookSpecificOutput"];
        assert!(specific.get("permissionDecision").is_none(), "{v}");
        assert!(specific["additionalContext"]
            .as_str()
            .unwrap()
            .contains("Run `uv run x.py --n 3` instead"));
    }

    /// A local fix the guard would warn on is not named. The policyd
    /// warning stands as it came.
    #[test]
    fn a_local_fix_the_guard_warns_on_leaves_the_warning_as_it_came() {
        let call = hook_call(CLAUDE);
        let warns = |_: &str| Some(uv_warning());
        assert_eq!(pick(&call, Some(&uv_warning()), warns), None);
        let again = |line: &str| {
            hold(line, "echo again".into());
            None
        };
        assert_eq!(pick(&call, Some(&uv_warning()), again), None);
    }

    /// Review repro 1: the recheck of policyd's new line rewrites it
    /// again. The guard's last word was another line, so the new line
    /// does not run; the original runs with a warning that names both.
    #[test]
    fn a_rewrite_the_recheck_rewrites_again_runs_the_line_with_a_warning() {
        let call = hook_call(CLAUDE);
        hold(&call.cue, "python evil.py".into());
        let judge = |line: &str| {
            if line == "python evil.py" {
                hold(line, "echo safe".into());
            }
            None
        };
        let Some(Outcome::Warn(rule)) = pick(&call, None, judge) else {
            panic!("a rewrite rewritten again must warn");
        };
        assert!(rule.reason.contains("`python evil.py`"), "{}", rule.reason);
        assert!(rule.reason.contains("`echo safe`"), "{}", rule.reason);
        for input in RUNNERS {
            let call = hook_call(input);
            hold(&call.cue, "python evil.py".into());
            let outcome = pick(&call, None, judge).unwrap();
            let v = answer_json(input, &outcome, false);
            assert!(!holds(&v), "{input}\n{v}");
            assert!(!v.to_string().contains("updatedInput"), "{v}");
            assert!(!v.to_string().contains("updated_input"), "{v}");
        }
    }

    /// Review repro 2: the recheck warns on policyd's new line. The
    /// original runs, and its warning names the new line and the rule.
    #[test]
    fn a_rewrite_the_recheck_warns_on_runs_the_line_with_a_warning() {
        let warns = |_: &str| Some(uv_warning());
        for input in RUNNERS {
            let call = hook_call(input);
            hold(&call.cue, "uv run evil.py".into());
            let outcome = pick(&call, None, warns).expect("a warning, not a silent allow");
            let Outcome::Warn(rule) = &outcome else {
                panic!("{outcome:?}");
            };
            assert!(rule.reason.contains("`uv run evil.py`"), "{}", rule.reason);
            assert!(
                rule.reason.contains("python-requires-uv"),
                "{}",
                rule.reason
            );
            let v = answer_json(input, &outcome, false);
            assert!(!holds(&v), "{input}\n{v}");
            assert!(v.to_string().contains("`uv run evil.py`"), "{v}");
            let v = answer_json(input, &outcome, true);
            assert!(!holds(&v), "relayed {input}\n{v}");
        }
    }

    #[test]
    fn no_rewrite_without_a_held_line_or_a_known_warning() {
        let call = hook_call(CLAUDE);
        assert!(pick(&call, None, allow_all).is_none(), "nothing held");
        hold("some other line", "uv run x.py".into());
        assert!(
            pick(&call, None, allow_all).is_none(),
            "held for another line"
        );
        hold(&call.cue, call.cue.clone());
        assert!(pick(&call, None, allow_all).is_none(), "the same line");
        let ask = Rule {
            pattern: "git-force-push".into(),
            verdict: "ask".into(),
            reason: "a force push".into(),
        };
        hold(&call.cue, "uv run x.py --n 3".into());
        assert!(pick(&call, Some(&ask), allow_all).is_none());
        assert!(
            take_held(&call.cue).is_none(),
            "a warning drops the held line"
        );
        let prompt = hook_call(r#"{"hook_event_name":"UserPromptSubmit","prompt":"python x.py"}"#);
        assert!(pick(&prompt, Some(&uv_warning()), allow_all).is_none());
    }

    /// A Read call is not a shell call, so an accepted rewrite of its cue
    /// is never sent back as its input.
    #[test]
    fn a_read_call_never_gets_updated_input() {
        let call = hook_call(READ);
        assert_eq!(call.event, "PreToolUse");
        hold(&call.cue, "uv run x.py --n 3".into());
        let outcome = pick(&call, None, allow_all).expect("a rewrite of the Read cue");
        assert!(matches!(outcome, Outcome::Run(_)), "{outcome:?}");
        let v = answer_json(READ, &outcome, false);
        assert!(v["hookSpecificOutput"].get("updatedInput").is_none(), "{v}");
        assert!(!holds(&v), "{v}");
    }

    /// The fake policyd, answering in 0.6.0's `bash -c` form: `python a.py`
    /// is rewritten to an accepted line, `python b.py` to one a rule warns
    /// on, and `python c.py` to one that is rewritten again.
    const FAKE_POLICYD: &str = r#"#!/bin/sh
case "$5" in
  "python a.py") printf "allow\trewrite\tbash -c 'uv run a.py'\n" ;;
  "python b.py") printf "allow\trewrite\tbash -c 'uv run b.py'\n" ;;
  "python c.py") printf "allow\trewrite\tbash -c 'uv run c.py'\n" ;;
  "uv run b.py") printf 'allow\twarn\tpython-requires-uv\tuse uv\n' ;;
  "uv run c.py") printf "allow\trewrite\tbash -c 'echo c'\n" ;;
  *) echo allow ;;
esac
"#;

    /// The guard holds a policyd rewrite for the hook, and the real guard
    /// as the recheck swaps in only an accepted line.
    #[test]
    fn the_real_guard_lets_only_an_accepted_rewrite_run() {
        use std::os::unix::fs::PermissionsExt as _;
        let _g = crate::tests::env_guard();
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("policyd");
        std::fs::write(&bin, FAKE_POLICYD).unwrap();
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
        let dir = Some(home.path());
        let judge = |line: &str| crate::tcb_verdict_in(line, dir);
        let outcome = |script: &str| {
            let input = CLAUDE.replace("python x.py --n 3", &format!("python {script}"));
            let call = hook_call(&input);
            let verdict = crate::tcb_verdict_in(&call.cue, dir);
            assert!(verdict.is_none(), "{script}: {verdict:?}");
            pick(&call, verdict.as_ref(), judge)
        };
        let accepted = outcome("a.py");
        let warned = outcome("b.py");
        let again = outcome("c.py");
        unsafe {
            for (k, v) in saved {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        assert_eq!(
            accepted,
            Some(Outcome::Run(Rewrite {
                command: "uv run a.py".into(),
                reason: POLICYD_REASON.into(),
            }))
        );
        let Some(Outcome::Warn(rule)) = warned else {
            panic!("a substitute a rule warns on must warn: {warned:?}");
        };
        assert!(
            rule.reason.contains("python-requires-uv"),
            "{}",
            rule.reason
        );
        let Some(Outcome::Warn(rule)) = again else {
            panic!("a substitute rewritten again must warn: {again:?}");
        };
        assert!(rule.reason.contains("`echo c`"), "{}", rule.reason);
    }

    /// A policyd rewrite the guard accepts reaches the runner as its new
    /// input, and the same swap is said and kept in the rewrite log. With
    /// the repository's `rewrite = false` the line runs as written, and
    /// the declined swap is still kept.
    #[test]
    fn a_policyd_rewrite_is_passed_on_and_also_noted_and_logged() {
        use std::os::unix::fs::PermissionsExt as _;
        let _g = crate::tests::env_guard();
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("proj");
        std::fs::create_dir_all(&repo).unwrap();
        let init = crate::git_cmd()
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let bin = home.path().join("policyd");
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"$5\" in\n  \"python x.py --n 3\") printf \"allow\\trewrite\\tbash -c 'uv run x.py --n 3'\\n\" ;;\n  *) echo allow ;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        // SAFETY: under the lock every environment-reading test takes,
        // which puts each name back when it drops.
        unsafe {
            std::env::set_var("HOME", home.path());
            std::env::set_var("XDG_DATA_HOME", home.path().join("data"));
            std::env::set_var("XDG_CONFIG_HOME", home.path().join("config"));
            std::env::set_var("POLICYD_BIN", &bin);
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("LJOS_OFF", "0");
            std::env::set_var("LJOS_GUARD", "on");
            for key in ["POLICYD_REQUIRED", "LJOS_GUARD_FAIL_OPEN", "LJOS_CONFIG"] {
                std::env::remove_var(key);
            }
        }
        let dir = Some(repo.as_path());
        let judge = |line: &str| crate::tcb_verdict_in(line, dir);
        let call = hook_call(CLAUDE);

        let _ = crate::take_guard_note();
        let verdict = crate::tcb_verdict_in(&call.cue, dir);
        assert!(verdict.is_none(), "{verdict:?}");
        let note = crate::take_guard_note().expect("the swap is said");
        let outcome = pick(&call, None, judge).expect("the rewrite is passed on");
        let v = answer_json(CLAUDE, &outcome, false);
        assert_eq!(
            v["hookSpecificOutput"]["updatedInput"]["command"], "uv run x.py --n 3",
            "{v}"
        );
        assert!(note.contains("`uv run x.py --n 3`"), "{note}");
        assert!(note.contains("`python x.py --n 3`"), "{note}");
        let kept = crate::swap_log::rows();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!(kept[0].applied, "{kept:?}");
        assert_eq!(kept[0].ran, "uv run x.py --n 3");

        std::fs::write(repo.join("ljos.toml"), "rewrite = false\n").unwrap();
        let _ = crate::take_guard_note();
        let verdict = crate::tcb_verdict_in(&call.cue, dir);
        assert!(verdict.is_none(), "{verdict:?}");
        assert!(crate::take_guard_note().is_none(), "nothing was swapped");
        assert_eq!(pick(&call, None, judge), None, "the line runs as written");
        let kept = crate::swap_log::rows();
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert!(!kept[1].applied, "the switch declined it: {kept:?}");
    }
}
