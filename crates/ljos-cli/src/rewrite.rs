//! A guard that fixes a command instead of only refusing it.
//!
//! ljos-policyd answers `allow\trewrite\tLINE` when the line should run as
//! LINE. The guard judges LINE again. A runner whose `PreToolUse` hook can
//! replace the tool input gets LINE back as that input. A runner without
//! that field is refused with LINE named. A LINE the guard does not accept
//! as it stands refuses the call. The original line never runs.
//!
//! The local table names a fix in a policyd refusal it knows. That call
//! stays a deny, so a runner that drops an updated input still refuses it.

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
}

/// What the hook does with a call that has a rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Run this line, which the guard judged and accepted, in place of
    /// the call's line.
    Run(Rewrite),
    /// Refuse the call. The original line does not run.
    Refuse(Rule),
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
/// a continuation is joined. Then the rewrite is inexact, so the call is
/// refused and neither line runs.
pub(crate) fn hold_exact(line: &str, script: &str, command: String) -> Option<Rule> {
    if script.trim() == line.trim() {
        hold(line, command);
        return None;
    }
    Some(Rule {
        pattern: "ljos-policyd".into(),
        verdict: "deny".into(),
        reason: format!(
            "{POLICYD_REASON} to `{command}`, but it judged a copy with its here-documents \
             or continuations changed, so the new line is not exact. Neither line runs"
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
        })
}

/// Why the guard does not accept `line` as it stands, or None when it
/// does. `judge` reads a line as the guard would and holds a rewrite as
/// [`crate::tcb_verdict_in`] does, so a line rewritten again is refused.
fn refusal_of(line: &str, judge: impl Fn(&str) -> Option<Rule>) -> Option<String> {
    let _ = take_held(line);
    if let Some(rule) = judge(line) {
        return Some(format!("the guard refuses it: {}", rule.reason));
    }
    take_held(line).map(|again| format!("the guard rewrites it again, to `{again}`"))
}

/// What the hook does with a tool call that has a rewrite, or None when
/// it has none and the usual answer stands.
///
/// A policyd rewrite runs only when the guard accepts the new line as it
/// stands. Otherwise the call is refused. A local fix names the new line
/// in the policyd refusal when the guard accepts that line, and the call
/// stays refused either way.
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
            Some(match refusal_of(&command, judge) {
                None => Outcome::Run(Rewrite {
                    command,
                    reason: POLICYD_REASON.to_string(),
                }),
                Some(why) => Outcome::Refuse(Rule {
                    pattern: "ljos-policyd".into(),
                    verdict: "deny".into(),
                    reason: format!(
                        "{POLICYD_REASON} to `{command}`, and {why}. Neither line runs"
                    ),
                }),
            })
        }
        Some(rule) if rule.verdict == "deny" && rule.pattern == "ljos-policyd" => {
            let code = rule.reason.split(':').next().unwrap_or("").trim();
            let fix = local(code, line).filter(|fix| fix.command != line)?;
            refusal_of(&fix.command, judge).is_none().then(|| {
                let mut refused = rule.clone();
                refused.reason = format!(
                    "{}. Run `{}` instead: {}",
                    rule.reason, fix.command, fix.reason
                );
                Outcome::Refuse(refused)
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

/// [`answer`] for a known outcome. A refusal is the runner's deny. A line
/// to run is the updated input where the runner takes one. A runner that
/// cannot take one is refused with the new line named, so the original
/// line never runs.
#[must_use]
pub fn answer_with(
    input: &str,
    call: &HookCall,
    context: &str,
    outcome: &Outcome,
    relayed: bool,
) -> String {
    let refuse = |rule: &Rule| crate::approval::hook_output(input, call, context, Some(rule));
    match outcome {
        Outcome::Refuse(rule) => refuse(rule),
        Outcome::Run(fix) => (!relayed)
            .then(|| updated_input(input, call, context, fix))
            .flatten()
            .unwrap_or_else(|| refuse(&unchangeable(fix))),
    }
}

/// The deny for a runner whose hook cannot replace the command.
fn unchangeable(fix: &Rewrite) -> Rule {
    Rule {
        pattern: "ljos-policyd".into(),
        verdict: "deny".into(),
        reason: format!(
            "{}. Run `{}` instead. This runner cannot take a new command, so the original \
             line does not run",
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

    /// The answer when policyd refused the call for want of uv.
    fn local_refusal(input: &str) -> Value {
        let call = hook_call(input);
        let outcome = pick(&call, Some(&uv_refusal()), allow_all).expect("a fix");
        assert!(matches!(outcome, Outcome::Refuse(_)), "{outcome:?}");
        answer_json(input, &outcome, false)
    }

    /// The decision a runner reads, whichever field carries it.
    fn decision(v: &Value) -> &str {
        v["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .or_else(|| v["permission"].as_str())
            .or_else(|| v["decision"].as_str())
            .unwrap_or("")
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
        assert_eq!(decision(&v), "deny", "{v}");
        assert!(v["hookSpecificOutput"].get("updatedInput").is_none());
        assert!(
            v.to_string().contains("Run `uv run x.py --n 3` instead"),
            "{v}"
        );
    }

    /// A runner that cannot take a new command is refused with the new
    /// line named, so the original line never runs: Cursor's
    /// `beforeShellExecution`, and every runner behind the relay.
    #[test]
    fn an_accepted_rewrite_is_a_deny_where_input_cannot_change() {
        let cases = [
            (CURSOR_SHELL, false),
            (CLAUDE, true),
            (CODEX, true),
            (GROK, true),
            (CURSOR_TOOL, true),
        ];
        for (input, relayed) in cases {
            let v = policyd_run(input, relayed);
            assert_eq!(decision(&v), "deny", "{input} relayed={relayed}\n{v}");
            assert!(v["hookSpecificOutput"].get("updatedInput").is_none(), "{v}");
            assert!(v.get("updated_input").is_none(), "{v}");
            let said = v.to_string();
            assert!(said.contains("Run `uv run x.py --n 3` instead"), "{said}");
            assert!(said.contains("the original line does not run"), "{said}");
        }
    }

    /// A rewrite of a copy that is not the line cannot be applied exactly,
    /// so the call is refused and nothing is held.
    #[test]
    fn a_rewrite_of_a_changed_copy_refuses_the_call() {
        let line = "python x.py <<EOF\nbody\nEOF";
        let rule = hold_exact(line, "python x.py <<EOF\nEOF", "uv run x.py".into())
            .expect("a changed copy refuses");
        assert_eq!(rule.verdict, "deny");
        assert!(rule.reason.contains("Neither line runs"), "{}", rule.reason);
        assert!(take_held(line).is_none());
        assert!(hold_exact("python x.py", "python x.py", "uv run x.py".into()).is_none());
        assert_eq!(take_held("python x.py").as_deref(), Some("uv run x.py"));
    }

    /// Review repro: a denial sent as allow plus updatedInput runs the
    /// refused line on a runner that drops updatedInput. Each runner gets
    /// a deny with no updated input, and the deny names the fix.
    #[test]
    fn a_local_fix_is_a_deny_on_every_runner() {
        for input in RUNNERS {
            let mut v = local_refusal(input);
            assert_eq!(decision(&v), "deny", "{input}\n{v}");
            assert!(v["hookSpecificOutput"].get("updatedInput").is_none(), "{v}");
            assert!(v.get("updated_input").is_none(), "{v}");
            let said = v.to_string();
            assert!(said.contains("Run `uv run x.py --n 3` instead"), "{said}");
            assert!(said.contains("python-requires-uv: "), "{said}");
            if let Some(specific) = v["hookSpecificOutput"].as_object_mut() {
                specific.remove("updatedInput");
            }
            v.as_object_mut().unwrap().remove("updated_input");
            assert_eq!(decision(&v), "deny", "dropping the input still refuses");
        }
        let v = local_refusal(CODEX);
        let specific = &v["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "deny");
        assert!(specific["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("Run `uv run x.py --n 3` instead"));
    }

    /// A local fix the guard would refuse is not named. The policyd
    /// refusal stands as it came.
    #[test]
    fn a_refused_local_fix_leaves_the_refusal_as_it_came() {
        let call = hook_call(CLAUDE);
        let refuse = |_: &str| Some(uv_refusal());
        assert_eq!(pick(&call, Some(&uv_refusal()), refuse), None);
        let again = |line: &str| {
            hold(line, "echo again".into());
            None
        };
        assert_eq!(pick(&call, Some(&uv_refusal()), again), None);
    }

    /// Review repro 1: the recheck of policyd's new line rewrites it
    /// again. The guard's last word was another line, so the new line
    /// does not run, and neither does the original.
    #[test]
    fn a_rewrite_the_recheck_rewrites_again_refuses_the_call() {
        let call = hook_call(CLAUDE);
        hold(&call.cue, "python evil.py".into());
        let judge = |line: &str| {
            if line == "python evil.py" {
                hold(line, "echo safe".into());
            }
            None
        };
        let Some(Outcome::Refuse(rule)) = pick(&call, None, judge) else {
            panic!("a rewrite rewritten again must refuse");
        };
        assert_eq!(rule.verdict, "deny");
        assert!(rule.reason.contains("`python evil.py`"), "{}", rule.reason);
        assert!(rule.reason.contains("`echo safe`"), "{}", rule.reason);
        for input in RUNNERS {
            let call = hook_call(input);
            hold(&call.cue, "python evil.py".into());
            let outcome = pick(&call, None, judge).unwrap();
            let v = answer_json(input, &outcome, false);
            assert_eq!(decision(&v), "deny", "{input}\n{v}");
            assert!(!v.to_string().contains("updatedInput"), "{v}");
            assert!(!v.to_string().contains("updated_input"), "{v}");
        }
    }

    /// Review repro 2: the recheck denies policyd's new line. The hook
    /// used to fall back to the original verdict and allow the original.
    #[test]
    fn a_rewrite_the_recheck_denies_refuses_the_call() {
        let refuse = |_: &str| Some(uv_refusal());
        for input in RUNNERS {
            let call = hook_call(input);
            hold(&call.cue, "uv run evil.py".into());
            let outcome = pick(&call, None, refuse).expect("a refusal, not the original");
            let Outcome::Refuse(rule) = &outcome else {
                panic!("{outcome:?}");
            };
            assert!(rule.reason.contains("`uv run evil.py`"), "{}", rule.reason);
            assert!(
                rule.reason.contains("python-requires-uv"),
                "{}",
                rule.reason
            );
            let v = answer_json(input, &outcome, false);
            assert_eq!(decision(&v), "deny", "{input}\n{v}");
            let v = answer_json(input, &outcome, true);
            assert_eq!(decision(&v), "deny", "relayed {input}\n{v}");
        }
    }

    #[test]
    fn no_rewrite_without_a_held_line_or_a_known_refusal() {
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
            "a refusal drops the held line"
        );
        let prompt = hook_call(r#"{"hook_event_name":"UserPromptSubmit","prompt":"python x.py"}"#);
        assert!(pick(&prompt, Some(&uv_refusal()), allow_all).is_none());
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
        assert_eq!(decision(&v), "deny", "{v}");
    }

    /// The fake policyd, answering in 0.6.0's `bash -c` form: `python a.py`
    /// is rewritten to an accepted line, `python b.py` to a refused one,
    /// and `python c.py` to one that is rewritten again.
    const FAKE_POLICYD: &str = r#"#!/bin/sh
case "$5" in
  "python a.py") printf "allow\trewrite\tbash -c 'uv run a.py'\n" ;;
  "python b.py") printf "allow\trewrite\tbash -c 'uv run b.py'\n" ;;
  "python c.py") printf "allow\trewrite\tbash -c 'uv run c.py'\n" ;;
  "uv run b.py") printf 'deny\tpython-requires-uv\n'; exit 2 ;;
  "uv run c.py") printf "allow\trewrite\tbash -c 'echo c'\n" ;;
  *) echo allow ;;
esac
"#;

    /// The guard holds a policyd rewrite for the hook, and the real guard
    /// as the recheck lets only an accepted line run.
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
        let refused = outcome("b.py");
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
        let Some(Outcome::Refuse(rule)) = refused else {
            panic!("a refused substitute must refuse the call: {refused:?}");
        };
        assert!(
            rule.reason.contains("python-requires-uv"),
            "{}",
            rule.reason
        );
        let Some(Outcome::Refuse(rule)) = again else {
            panic!("a substitute rewritten again must refuse the call: {again:?}");
        };
        assert!(rule.reason.contains("`echo c`"), "{}", rule.reason);
    }
}
