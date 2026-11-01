//! The tools a runner lives in: terminal multiplexers and agent runtimes. A
//! tool opens a pane the person can watch and hands the runner a line, and
//! its `alive` verb says whether the pane is still there.
//!
//! `[[tool]]` tables in harnesses.toml declare them beside the runners.
//! herdr and tmux ship as shapes ([`SHIPPED_TOOLS`]); a table of the same
//! name replaces a shipped one. Every verb is an argv with
//! `{placeholders}`. A tool the seat has never heard of is one table. A
//! tool that refuses is named with its own words and the next one is
//! tried, so a shape gone stale against its tool shows in what `hand`
//! returns.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};

/// One tool, as harnesses.toml declares it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Tool {
    pub name: String,
    /// Exits 0 when the tool can open a pane on this machine now.
    #[serde(default)]
    pub detect: Vec<String>,
    /// Ways to open the persona's pane, tried in order until one exits 0.
    /// The pane runs `{script}`, which changes to `{home}` first. With
    /// `run` set it starts as a shell, and `run` types the script into it.
    #[serde(default)]
    pub open: Vec<Vec<String>>,
    /// Where `open`'s JSON output names the new pane, as a JSON pointer.
    #[serde(default)]
    pub pane_pointer: Option<String>,
    /// Types the pane script into the pane `open` made, for a tool whose
    /// panes start as shells.
    #[serde(default)]
    pub run: Vec<String>,
    /// Starts the pane script again in a pane whose runner exited.
    #[serde(default)]
    pub respawn: Vec<String>,
    /// Hands the runner one line, through the tool's agent interface where
    /// it has one and as typed keys where it does not; each argv runs in
    /// order.
    #[serde(default)]
    pub prompt: Vec<Vec<String>>,
    /// Types the line raw when `prompt` is refused or absent, as for a
    /// runner the tool does not recognise as an agent.
    #[serde(default)]
    pub type_line: Vec<Vec<String>>,
    /// Exits 0 while the pane is there.
    #[serde(default)]
    pub alive: Vec<String>,
    /// Exits 0 once a fresh runner takes input; retried until `ready_s`.
    #[serde(default)]
    pub ready: Vec<String>,
    /// Seconds to wait for `ready`, or to sleep when there is none.
    #[serde(default)]
    pub ready_s: Option<u64>,
}

/// The shapes the seat ships, in the order they are tried: herdr's agent
/// API when its server answers, else tmux.
pub const SHIPPED_TOOLS: &str = r#"
[[tool]]
name = "herdr"
# `herdr status server` exits 0 even with no server running;
# `workspace list` needs the socket, so it fails without one.
detect = ["herdr", "workspace", "list"]
# One workspace a persona, opened in its home as its seat; the root pane is
# a shell, and the pane script is typed into it.
open = [["herdr", "workspace", "create", "--cwd", "{home}", "--env", "LJOS_SEAT={name}", "--label", "{label}", "--no-focus"]]
pane_pointer = "/result/root_pane/pane_id"
run = ["herdr", "pane", "run", "{pane}", "sh {script_q}"]
respawn = ["herdr", "pane", "run", "{pane}", "sh {script_q}"]
prompt = [["herdr", "agent", "prompt", "{pane}", "{line}"]]
type_line = [["herdr", "pane", "send-text", "{pane}", "{line}"], ["herdr", "pane", "send-keys", "{pane}", "enter"]]
alive = ["herdr", "pane", "get", "{pane}"]
# `herdr agent wait` answers agent_not_found until herdr recognises the
# runner; a runner it never recognises gets the line typed raw once
# `ready_s` runs out.
ready = ["herdr", "agent", "wait", "{pane}", "--until", "idle", "--until", "done", "--timeout", "5000"]
ready_s = 20

[[tool]]
name = "tmux"
detect = ["tmux", "-V"]
open = [
  ["tmux", "new-window", "-d", "-t", "{session}", "-n", "{name}", "sh", "{script}"],
  ["tmux", "new-session", "-d", "-s", "{session}", "-n", "{name}", "sh", "{script}"],
]
respawn = ["tmux", "respawn-window", "-k", "-t", "{session}:{name}", "sh", "{script}"]
prompt = [["tmux", "send-keys", "-t", "{session}:{name}", "-l", "{line}"], ["tmux", "send-keys", "-t", "{session}:{name}", "Enter"]]
alive = ["tmux", "list-panes", "-t", "{session}:{name}"]
ready_s = 8
"#;

#[derive(serde::Deserialize)]
struct ToolFile {
    #[serde(default)]
    tool: Vec<Tool>,
}

/// The shipped shapes, parsed.
#[must_use]
pub fn shipped() -> Vec<Tool> {
    toml::from_str::<ToolFile>(SHIPPED_TOOLS)
        .map(|f| f.tool)
        .unwrap_or_default()
}

/// The tools in the order they are tried: the file's own, then each
/// shipped shape the file did not replace.
#[must_use]
pub fn ordered(declared: &[Tool]) -> Vec<Tool> {
    let mut out: Vec<Tool> = declared.to_vec();
    for t in shipped() {
        if !out.iter().any(|d| d.name == t.name) {
            out.push(t);
        }
    }
    out
}

/// What fills a verb's placeholders.
#[derive(Debug, Clone, Default)]
pub struct Vars(pub BTreeMap<&'static str, String>);

impl Vars {
    #[must_use]
    pub fn with(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.0.insert(key, value.into());
        self
    }
}

/// A word quoted for `sh`.
#[must_use]
pub fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// `argv` with every `{key}` replaced from `vars`. A placeholder with no
/// value is left as written, so a typo reaches the tool and its refusal
/// names it.
#[must_use]
pub fn fill(argv: &[String], vars: &Vars) -> Vec<String> {
    argv.iter()
        .map(|word| {
            let mut w = word.clone();
            for (k, v) in &vars.0 {
                w = w.replace(&format!("{{{k}}}"), v);
            }
            w
        })
        .collect()
}

/// What a tool printed when it ran a verb.
#[derive(Debug, Clone)]
pub struct Ran {
    pub stdout: String,
}

/// Run one filled argv. A refusal carries the tool's first stderr line, or
/// its first stdout line when stderr is empty, and the argv's verb words.
///
/// # Errors
///
/// The verb is empty, the program will not start, or it exited non-zero.
pub fn run(tool: &str, argv: &[String]) -> Result<Ran> {
    let Some((bin, rest)) = argv.split_first() else {
        bail!("{tool}: an empty verb");
    };
    let out = std::process::Command::new(bin)
        .args(rest)
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("{tool}: {bin} would not start"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let said = stderr
            .lines()
            .chain(stdout.lines())
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("no output")
            .to_string();
        let verb: Vec<&str> = argv.iter().take(3).map(String::as_str).collect();
        bail!("{tool}: `{}` exited {}: {said}", verb.join(" "), out.status);
    }
    Ok(Ran { stdout })
}

/// Whether a verb that answers by its exit status says yes. An empty verb
/// says no.
#[must_use]
pub fn answers(argv: &[String]) -> bool {
    let Some((bin, rest)) = argv.split_first() else {
        return false;
    };
    std::process::Command::new(bin)
        .args(rest)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The pane `open` named, read through `pointer` from its JSON output.
#[must_use]
pub fn pane_from(stdout: &str, pointer: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    match v.pointer(pointer)? {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// The tools that answer here, in the order they would be tried. A named
/// tool (`LJOS_PANE_TOOL`) narrows the list to that one.
#[must_use]
pub fn available(declared: &[Tool], only: Option<&str>) -> Vec<Tool> {
    ordered(declared)
        .into_iter()
        .filter(|t| only.is_none_or(|n| t.name == n))
        .filter(|t| answers(&t.detect))
        .collect()
}

/// Open a pane with `tool`: the first `open` that exits 0, then `run` when
/// the pane starts as a shell. Returns the pane the tool named, or the
/// empty string for a tool whose verbs name the pane themselves.
///
/// # Errors
///
/// There is no `open` verb, every `open` refused, the pane pointer found no
/// pane, or `run` refused.
pub fn open(tool: &Tool, vars: &Vars) -> Result<String> {
    let mut refusals = Vec::new();
    for argv in &tool.open {
        match run(&tool.name, &fill(argv, vars)) {
            Ok(ran) => {
                let pane = tool
                    .pane_pointer
                    .as_deref()
                    .and_then(|p| pane_from(&ran.stdout, p))
                    .unwrap_or_default();
                if tool.pane_pointer.is_some() && pane.is_empty() {
                    bail!(
                        "{}: open printed no pane at {}",
                        tool.name,
                        tool.pane_pointer.as_deref().unwrap_or("")
                    );
                }
                if !tool.run.is_empty() {
                    let vars = vars.clone().with("pane", pane.clone());
                    run(&tool.name, &fill(&tool.run, &vars))?;
                }
                return Ok(pane);
            }
            Err(e) => refusals.push(format!("{e:#}")),
        }
    }
    if refusals.is_empty() {
        bail!("{}: no open verb", tool.name);
    }
    bail!("{}", refusals.join("; "))
}

/// Hand the runner one line: `prompt`, and `type_line` when `prompt` is
/// refused or absent.
///
/// # Errors
///
/// `prompt` refused and there is no `type_line`, `type_line` refused, or
/// the tool has neither.
pub fn send(tool: &Tool, vars: &Vars) -> Result<()> {
    let steps = |list: &[Vec<String>]| -> Result<()> {
        for argv in list {
            run(&tool.name, &fill(argv, vars))?;
        }
        Ok(())
    };
    match steps(&tool.prompt) {
        Ok(()) if !tool.prompt.is_empty() => Ok(()),
        first => {
            if tool.type_line.is_empty() {
                return first.and_then(|()| bail!("{}: no prompt verb", tool.name));
            }
            steps(&tool.type_line).map_err(|e| match first {
                Err(f) => anyhow::anyhow!("{f:#}; then {e:#}"),
                Ok(()) => e,
            })
        }
    }
}

/// Wait until a fresh runner takes input: `ready` retried each second until
/// `ready_s` runs out, or a plain sleep of `ready_s` for a tool with no
/// `ready`.
pub fn wait_ready(tool: &Tool, vars: &Vars) {
    let budget = std::time::Duration::from_secs(tool.ready_s.unwrap_or(0));
    if tool.ready.is_empty() {
        std::thread::sleep(budget);
        return;
    }
    let argv = fill(&tool.ready, vars);
    let start = std::time::Instant::now();
    while !answers(&argv) {
        if start.elapsed() >= budget {
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// The doctor's row on panes: which tool would open a persona's pane, and
/// which others answer.
#[must_use]
pub fn doctor_state(declared: &[Tool]) -> (bool, String) {
    let here = available(declared, std::env::var("LJOS_PANE_TOOL").ok().as_deref());
    match here.split_first() {
        Some((first, [])) => (true, format!("{} opens persona panes", first.name)),
        Some((first, rest)) => (
            true,
            format!(
                "{} opens persona panes; {} also here",
                first.name,
                rest.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")
            ),
        ),
        None => (
            false,
            format!(
                "none of {} answers; a persona session needs one, or a [[tool]] table in harnesses.toml",
                ordered(declared)
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_shapes_parse_herdr_first() {
        let tools = shipped();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["herdr", "tmux"]);
        let herdr = &tools[0];
        assert_eq!(
            herdr.pane_pointer.as_deref(),
            Some("/result/root_pane/pane_id")
        );
        assert!(herdr.open[0].contains(&"--no-focus".to_string()));
        assert!(
            herdr.prompt[0].starts_with(&["herdr".into(), "agent".into(), "prompt".into()]),
            "herdr's agent API, not the send verb it dropped"
        );
        assert!(herdr.ready.contains(&"--until".to_string()));
    }

    #[test]
    fn a_declared_tool_replaces_the_shipped_one_of_its_name() {
        let mine = Tool {
            name: "tmux".into(),
            detect: vec!["true".into()],
            ..Tool::default()
        };
        let zellij = Tool {
            name: "zellij".into(),
            ..Tool::default()
        };
        let tools = ordered(&[zellij, mine.clone()]);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["zellij", "tmux", "herdr"]);
        assert_eq!(tools[1], mine);
    }

    #[test]
    fn placeholders_fill_and_an_unknown_one_reaches_the_tool() {
        let vars = Vars::default()
            .with("home", "/s/personas/reviewer")
            .with("name", "reviewer")
            .with("script_q", sh_quote("/s/it's/pane.sh"));
        let got = fill(
            &[
                "--cwd".into(),
                "{home}".into(),
                "LJOS_SEAT={name}".into(),
                "sh {script_q}".into(),
                "{nope}".into(),
            ],
            &vars,
        );
        assert_eq!(
            got,
            [
                "--cwd",
                "/s/personas/reviewer",
                "LJOS_SEAT=reviewer",
                "sh '/s/it'\\''s/pane.sh'",
                "{nope}"
            ]
        );
    }

    #[test]
    fn the_pane_is_read_off_the_json_open_printed() {
        let out = r#"{"id":"cli:workspace:create","result":{"root_pane":{"pane_id":"w1:p2"}}}"#;
        assert_eq!(
            pane_from(out, "/result/root_pane/pane_id").as_deref(),
            Some("w1:p2")
        );
        assert_eq!(pane_from("not json", "/result/root_pane/pane_id"), None);
        assert_eq!(pane_from(out, "/result/tab/tab_id"), None);
    }

    #[test]
    fn a_refusal_says_what_the_tool_said() {
        let err = run(
            "fake",
            &[
                "sh".into(),
                "-c".into(),
                "echo 'unknown option: --no-focus' >&2; exit 2".into(),
            ],
        )
        .unwrap_err();
        let said = format!("{err:#}");
        assert!(said.contains("unknown option: --no-focus"), "{said}");
        assert!(said.starts_with("fake: `sh -c"), "{said}");
    }

    #[test]
    fn a_refused_prompt_falls_to_typing_the_line() {
        let dir = tempfile::tempdir().unwrap();
        let got = dir.path().join("typed");
        let tool = Tool {
            name: "fake".into(),
            prompt: vec![vec!["false".into()]],
            type_line: vec![vec![
                "sh".into(),
                "-c".into(),
                format!("printf '%s' \"$0\" > {}", got.display()),
                "{line}".into(),
            ]],
            ..Tool::default()
        };
        send(&tool, &Vars::default().with("line", "read the inbox")).unwrap();
        assert_eq!(std::fs::read_to_string(&got).unwrap(), "read the inbox");
        let refused = Tool {
            name: "fake".into(),
            prompt: vec![vec!["false".into()]],
            ..Tool::default()
        };
        assert!(send(&refused, &Vars::default()).is_err());
    }

    #[test]
    fn the_first_open_that_takes_wins_and_the_rest_are_reasons() {
        let tool = Tool {
            name: "fake".into(),
            open: vec![
                vec![
                    "sh".into(),
                    "-c".into(),
                    "echo 'no session' >&2; exit 1".into(),
                ],
                vec!["sh".into(), "-c".into(), "echo '{\"pane\":\"p7\"}'".into()],
            ],
            pane_pointer: Some("/pane".into()),
            ..Tool::default()
        };
        assert_eq!(open(&tool, &Vars::default()).unwrap(), "p7");
        let none = Tool {
            name: "fake".into(),
            open: vec![vec![
                "sh".into(),
                "-c".into(),
                "echo 'refused' >&2; exit 3".into(),
            ]],
            ..Tool::default()
        };
        let err = format!("{:#}", open(&none, &Vars::default()).unwrap_err());
        assert!(err.contains("refused"), "{err}");
    }
}
