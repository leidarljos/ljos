//! The subagent a tool call runs for, so its ballot binds its own session
//! rather than its parent's. Claude Code hands a hook the subagent's
//! `agent_id` and the call's `tool_use_id`, but gives the command it runs
//! no variable that names the agent. The hook leaves the call under the
//! runtime directory, and the `ljos` the command runs, or the MCP server
//! the call reaches, takes it back once.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// How long a call the hook saw waits to be taken: the longest a Claude
/// Code shell command runs.
const CALL_TTL_MS: u64 = 600_000;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Call {
    session: String,
    agent: String,
    argv: Vec<String>,
    at: u64,
}

fn root() -> PathBuf {
    crate::runtime_dir().join("agent-calls")
}

/// A name safe as one path component. `.` is left out, so it can join a
/// call to its index.
fn token(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn shell_dir(root: &Path, session: &str) -> PathBuf {
    root.join("shell").join(token(session))
}

fn mcp_path(root: &Path, call: &str) -> PathBuf {
    root.join("mcp").join(format!("{call}.json"))
}

fn put(path: &Path, call: &Call) {
    let Some(dir) = path.parent() else {
        return;
    };
    let Ok(text) = serde_json::to_string(call) else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn read(path: &Path) -> Option<Call> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The calls under `dir` still waiting, each with its file. A call past
/// [`CALL_TTL_MS`] is dropped.
fn waiting(dir: &Path, now: u64) -> Vec<(PathBuf, Call)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Some(call) = read(&path) else {
            continue;
        };
        if now.saturating_sub(call.at) > CALL_TTL_MS {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        out.push((path, call));
    }
    out
}

fn text(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v[*k].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Whether `tool` is one of the seat's MCP tools as Claude Code names it,
/// `mcp__SERVER__ljos_NAME`, whichever server key carries the seat.
#[must_use]
pub fn is_seat_mcp_tool(tool: &str) -> bool {
    tool.strip_prefix("mcp__")
        .and_then(|rest| rest.rsplit_once("__"))
        .is_some_and(|(server, name)| !server.is_empty() && name.starts_with("ljos_"))
}

/// Leave what a hook payload says about its tool call. On `PreToolUse`,
/// each `ljos` line of a shell call is kept for the process that runs it,
/// and a subagent's call to the seat's MCP tool for the server. On
/// `PostToolUse` what the call did not take is dropped. The payload's
/// agent is this hook process's own. Returns whether the call is a
/// `PreToolUse` on the seat's MCP tool: the server judges that call, so
/// the hook has nothing more to say.
pub fn note_hook_call(input: &str, event: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(input.trim()) else {
        return false;
    };
    let agent = text(&v, &["agent_id", "agentId"]).map(|a| crate::ballot_sig::field(&a));
    crate::ballot_sig::hook_agent(agent.clone());
    let tool = text(&v, &["tool_name", "toolName"]).unwrap_or_default();
    let seat_tool = event == "PreToolUse" && is_seat_mcp_tool(&tool);
    let session = text(&v, &["session_id", "sessionId", "conversation_id"])
        .map(|s| crate::ballot_sig::field(&s));
    let call = text(&v, &["tool_use_id", "toolUseId"])
        .map(|c| token(&c))
        .filter(|c| !c.is_empty());
    let (Some(session), Some(call)) = (session, call) else {
        return seat_tool;
    };
    let root = root();
    match event {
        "PreToolUse" if seat_tool => {
            if let Some(agent) = agent {
                let found = Call {
                    session,
                    agent,
                    argv: Vec::new(),
                    at: now_ms(),
                };
                put(&mcp_path(&root, &call), &found);
            }
        }
        "PreToolUse" => {
            let input = if v["tool_input"].is_object() {
                &v["tool_input"]
            } else {
                &v["toolInput"]
            };
            let Some(line) = input["command"].as_str() else {
                return false;
            };
            let at = now_ms();
            let dir = shell_dir(&root, &session);
            for (i, argv) in crate::ljos_invocations(line).into_iter().enumerate() {
                let found = Call {
                    session: session.clone(),
                    agent: agent.clone().unwrap_or_default(),
                    argv,
                    at,
                };
                put(&dir.join(format!("{call}.{i}.json")), &found);
            }
        }
        "PostToolUse" | "PostToolUseFailure" => forget(&root, &session, &call),
        _ => {}
    }
    seat_tool
}

fn forget(root: &Path, session: &str, call: &str) {
    let _ = std::fs::remove_file(mcp_path(root, call));
    let Ok(entries) = std::fs::read_dir(shell_dir(root, session)) else {
        return;
    };
    let prefix = format!("{call}.");
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Whether the line a hook saw runs as `argv`. A word the shell expands
/// (a variable, a substitution, a glob, a tilde) stands for any one word.
fn argv_matches(written: &[String], argv: &[String]) -> bool {
    written.len() == argv.len()
        && written
            .iter()
            .zip(argv)
            .all(|(w, a)| w == a || w.contains(['$', '`', '*', '?', '[', '~']))
}

/// The agent of the `ljos` line this process runs, when a hook saw it
/// under `session`. The newest call whose line is this argv is taken, so
/// no other process takes it too and a parent's call is never answered
/// by an older one its subagent left. An empty agent is the parent's.
#[must_use]
pub fn claim_command(session: &str) -> Option<String> {
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    claim_command_in(&root(), session, &argv, now_ms())
}

fn claim_command_in(root: &Path, session: &str, argv: &[String], now: u64) -> Option<String> {
    let mut found: Vec<(PathBuf, Call)> = waiting(&shell_dir(root, session), now)
        .into_iter()
        .filter(|(_, c)| c.session == session && argv_matches(&c.argv, argv))
        .collect();
    found.sort_by(|a, b| b.1.at.cmp(&a.1.at).then_with(|| b.0.cmp(&a.0)));
    found
        .into_iter()
        .find_map(|(path, c)| std::fs::remove_file(&path).is_ok().then_some(c.agent))
}

/// The session and subagent of a Claude Code MCP call, by the tool use id
/// its `_meta` carries, when its hook saw a subagent make it. Taken once.
#[must_use]
pub fn claim_mcp_call(call: &str) -> Option<(String, String)> {
    claim_mcp_in(&root(), call, now_ms())
}

fn claim_mcp_in(root: &Path, call: &str, now: u64) -> Option<(String, String)> {
    let call = token(call);
    if call.is_empty() {
        return None;
    }
    let path = mcp_path(root, &call);
    let found = waiting(&root.join("mcp"), now)
        .into_iter()
        .find(|(p, _)| *p == path)?
        .1;
    std::fs::remove_file(&path).ok()?;
    (!found.agent.is_empty()).then_some((found.session, found.agent))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    fn call(session: &str, agent: &str, argv: &str, at: u64) -> Call {
        Call {
            session: session.into(),
            agent: agent.into(),
            argv: words(argv),
            at,
        }
    }

    #[test]
    fn a_line_matches_its_argv_and_an_expanded_word_matches_any() {
        let argv = words("vote demo-a --for ship --used none");
        assert!(argv_matches(&argv, &argv));
        assert!(argv_matches(
            &words("vote $ISSUE --for ship --used none"),
            &argv
        ));
        assert!(!argv_matches(
            &words("vote demo-a --for hold --used none"),
            &argv
        ));
        assert!(!argv_matches(&words("vote demo-a --for ship"), &argv));
    }

    #[test]
    fn the_newest_matching_call_is_taken_once_and_a_stale_one_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell_dir(dir.path(), "sess-1");
        let line = "vote demo-a --for ship --used none";
        put(
            &shell.join("toolu_old.0.json"),
            &call("sess-1", "agent-b", line, 1_000),
        );
        put(
            &shell.join("toolu_new.0.json"),
            &call("sess-1", "", line, 2_000),
        );
        put(
            &shell.join("toolu_sub.0.json"),
            &call("sess-1", "agent-a", line, 1_500),
        );
        put(
            &shell.join("toolu_other.0.json"),
            &call("sess-1", "agent-c", "recall demo-a", 2_500),
        );
        let argv = words(line);
        let now = 3_000;
        assert_eq!(
            claim_command_in(dir.path(), "sess-1", &argv, now).as_deref(),
            Some("")
        );
        assert_eq!(
            claim_command_in(dir.path(), "sess-1", &argv, now).as_deref(),
            Some("agent-a")
        );
        assert_eq!(
            claim_command_in(dir.path(), "sess-2", &argv, now),
            None,
            "another session's calls are not read"
        );
        let late = 1_000 + CALL_TTL_MS + 1;
        assert_eq!(claim_command_in(dir.path(), "sess-1", &argv, late), None);
        assert!(!shell.join("toolu_old.0.json").exists());
    }

    #[test]
    fn an_mcp_call_names_its_subagent_once() {
        let dir = tempfile::tempdir().unwrap();
        put(
            &mcp_path(dir.path(), "toolu_a"),
            &call("sess-1", "agent-a", "", 1_000),
        );
        assert_eq!(
            claim_mcp_in(dir.path(), "toolu_a", 2_000),
            Some(("sess-1".into(), "agent-a".into()))
        );
        assert_eq!(claim_mcp_in(dir.path(), "toolu_a", 2_000), None);
        assert_eq!(claim_mcp_in(dir.path(), "toolu_b", 2_000), None);
        put(
            &mcp_path(dir.path(), "toolu_c"),
            &call("sess-1", "agent-c", "", 1_000),
        );
        assert_eq!(claim_mcp_in(dir.path(), "../toolu_c", 2_000), None);
        assert_eq!(
            claim_mcp_in(dir.path(), "toolu_c", 1_000 + CALL_TTL_MS + 1),
            None
        );
    }

    #[test]
    fn the_seat_mcp_tools_are_known_by_name() {
        assert!(is_seat_mcp_tool("mcp__ljos__ljos_vote"));
        assert!(is_seat_mcp_tool("mcp__plugin_ljos_ljos__ljos_vote"));
        assert!(!is_seat_mcp_tool("mcp__github__add_issue_comment"));
        assert!(!is_seat_mcp_tool("ljos_vote"));
        assert!(!is_seat_mcp_tool("Bash"));
    }
}
