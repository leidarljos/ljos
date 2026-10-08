//! A persona's session: the runner that thinks as the persona, in a pane
//! the person can watch and talk to, kept across hand-offs.
//!
//! Each persona has a home directory. Its runner starts there, as the seat
//! named after the persona, so its memories, ballots and trust rows are
//! the persona's. A runner's `[[harness]]` table names how it starts and
//! how it resumes the latest session of the directory it starts in, so
//! the home is the session key: the second
//! hand-off resumes the first conversation, and no id is stored. A pane
//! that is still open is handed the next task in place. A task is written
//! to the persona's inbox and the pane is told one line naming the file,
//! since a long text typed into a runner's prompt submits at its first
//! line break.
//!
//! The pane opens in the first tool that answers ([`crate::tools`]): herdr
//! through its agent API, else tmux, else a `[[tool]]` the file declares. A
//! tool that refuses is named in what `hand` returns. The pane script
//! supervises its runner as an OTP supervisor does a transient child: a
//! runner that exits non-zero is resumed, at most [`RESTARTS`] times in
//! [`RESTART_WINDOW_S`] seconds, and a runner that exits 0 is done.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::tools::{self, Tool, Vars};

/// The tmux session persona windows open in, and the herdr label prefix.
pub const PERSONA_SESSION: &str = "ljos-personas";

/// How many times a pane resumes a runner that failed, within
/// [`RESTART_WINDOW_S`], before it leaves the pane to the person.
pub const RESTARTS: u32 = 3;

/// The window the restart count is kept over.
pub const RESTART_WINDOW_S: u64 = 60;

/// The argv that starts the runner named `runner` in a persona's home,
/// from its `[[harness]]` table: `start` the first time (the runner's name
/// alone when unset), `resume` after, which continues the latest session
/// of the directory it starts in. A runner with no `resume` starts fresh
/// each time. `None` when the table names no such runner.
#[must_use]
pub fn runner_argv_in(all: &crate::Harnesses, runner: &str, resume: bool) -> Option<Vec<String>> {
    let h = all.harness.iter().find(|h| h.name == runner)?;
    let start = if h.start.is_empty() {
        vec![h.name.clone()]
    } else {
        h.start.clone()
    };
    Some(if resume && !h.resume.is_empty() {
        h.resume.clone()
    } else {
        start
    })
}

/// [`runner_argv_in`] over this machine's runners file.
#[must_use]
pub fn runner_argv(runner: &str, resume: bool) -> Option<Vec<String>> {
    runner_argv_in(
        &crate::harnesses_from(&crate::harnesses_path()).ok()?,
        runner,
        resume,
    )
}

/// The runners this machine names.
#[must_use]
pub fn runner_names() -> Vec<String> {
    crate::harnesses_from(&crate::harnesses_path())
        .map(|all| all.harness.into_iter().map(|h| h.name).collect())
        .unwrap_or_default()
}

/// The `[[tool]]` tables this machine declares.
fn declared_tools() -> Vec<Tool> {
    crate::harnesses_from(&crate::harnesses_path())
        .map(|all| all.tool)
        .unwrap_or_default()
}

/// `$XDG_STATE_HOME/ljos/personas/NAME`: where the persona's runner works
/// and keeps its session.
#[must_use]
pub fn home(name: &str) -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
        .join("personas")
        .join(name)
}

/// The script a persona's pane runs: the runner as the seat named after
/// the persona, in its home, resumed with `again` when it fails (a
/// transient restart, bounded by [`RESTARTS`] in [`RESTART_WINDOW_S`]),
/// then a shell left open for the person. While a runner is up, the
/// script's pid is in `.runner.pid`.
#[must_use]
pub fn pane_script(name: &str, argv: &[String], again: &[String], home: &Path) -> String {
    let words = |a: &[String]| {
        a.iter()
            .map(|w| tools::sh_quote(w))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let again = if again.is_empty() { argv } else { again };
    format!(
        "#!/bin/sh\nprintf '\\033]2;%s\\007' {n}\ncd {h} || exit 1\necho $$ > .runner.pid\n\
         LJOS_SEAT={n} {cmd}\ncode=$?\ntries=0\nsince=$(date +%s)\n\
         while [ \"$code\" -ne 0 ]; do\n  now=$(date +%s)\n  \
         if [ $((now - since)) -ge {window} ]; then since=$now; tries=0; fi\n  \
         tries=$((tries + 1))\n  if [ \"$tries\" -gt {max} ]; then\n    \
         echo \"persona {name}: the runner failed $tries times in {window}s; not restarting\"\n    break\n  fi\n  \
         echo \"persona {name}: the runner exited $code; resuming, $tries of {max}\"\n  sleep 1\n  \
         LJOS_SEAT={n} {again}\n  code=$?\ndone\nrm -f .runner.pid\n\
         echo \"persona {name}: the runner exited; this pane stays for reading\"\n\
         exec \"${{SHELL:-/bin/sh}}\" -i\n",
        n = tools::sh_quote(name),
        h = tools::sh_quote(&home.display().to_string()),
        cmd = words(argv),
        again = words(again),
        window = RESTART_WINDOW_S,
        max = RESTARTS,
    )
}

/// The placeholders a tool's verbs see for this persona.
fn vars_for(name: &str, home: &Path, pane: &str) -> Vars {
    let script = home.join("pane.sh").display().to_string();
    Vars::default()
        .with("name", name)
        .with("label", format!("persona-{name}"))
        .with("session", PERSONA_SESSION)
        .with("home", home.display().to_string())
        .with("script_q", tools::sh_quote(&script))
        .with("script", script)
        .with("pane", pane)
}

/// The tool and pane a persona's last pane opened in, from `.pane`.
fn read_record(home: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(home.join(".pane")).ok()?;
    let mut lines = text.lines();
    let tool = lines.next()?.trim().to_string();
    let pane = lines.next().unwrap_or("").trim().to_string();
    (!tool.is_empty()).then_some((tool, pane))
}

fn write_record(home: &Path, tool: &str, pane: &str) -> Result<()> {
    std::fs::write(home.join(".pane"), format!("{tool}\n{pane}\n"))
        .with_context(|| format!("{}", home.join(".pane").display()))
}

/// Whether the pane script's runner is up: `.runner.pid` names a live
/// process.
fn runner_up(home: &Path) -> bool {
    std::fs::read_to_string(home.join(".runner.pid"))
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .is_some_and(crate::pid_alive)
}

/// What a pane is called where the person looks for it.
fn describe(tool: &str, pane: &str, name: &str) -> String {
    match tool {
        "tmux" => format!("tmux {PERSONA_SESSION}:{name}"),
        _ if pane.is_empty() => format!("{tool} persona-{name}"),
        _ => format!("{tool} pane {pane}"),
    }
}

/// A persona's pane as it stands.
enum Pane {
    /// No pane, or one its tool no longer has.
    Gone,
    /// The pane is there and its runner has exited.
    Idle(Tool, String),
    /// The pane is there and its runner is up.
    Live(Tool, String),
}

fn pane_state(name: &str, home: &Path, declared: &[Tool]) -> Pane {
    let Some((tool_name, pane)) = read_record(home) else {
        // A window opened before panes kept a record.
        let legacy = std::process::Command::new("tmux")
            .args(["list-windows", "-t", PERSONA_SESSION, "-F", "#W"])
            .stderr(std::process::Stdio::null())
            .output()
            .is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|w| w == name)
            });
        return match tools::ordered(declared)
            .into_iter()
            .find(|t| t.name == "tmux")
        {
            Some(tmux) if legacy => Pane::Live(tmux, String::new()),
            _ => Pane::Gone,
        };
    };
    let Some(tool) = tools::ordered(declared)
        .into_iter()
        .find(|t| t.name == tool_name)
    else {
        return Pane::Gone;
    };
    let vars = vars_for(name, home, &pane);
    if !tools::answers(&tools::fill(&tool.alive, &vars)) {
        return Pane::Gone;
    }
    if runner_up(home) {
        Pane::Live(tool, pane)
    } else {
        Pane::Idle(tool, pane)
    }
}

/// Where a persona's pane is open now, with its runner up.
#[must_use]
pub fn live_pane(name: &str) -> Option<String> {
    match pane_state(name, &home(name), &declared_tools()) {
        Pane::Live(tool, pane) => Some(describe(&tool.name, &pane, name)),
        _ => None,
    }
}

/// Hand `task` to the persona `name`, whose runner is `runner`: into its
/// open pane, back into a pane whose runner exited, or a new pane that
/// continues its session (or starts one). Returns where it runs, and any
/// tool that refused on the way.
///
/// # Errors
///
/// An unknown runner, no tool that opens a pane, or the line not reaching
/// the pane.
pub fn hand(name: &str, runner: &str, task: &str) -> Result<String> {
    let home = home(name);
    let inbox = home.join("inbox");
    std::fs::create_dir_all(&inbox)
        .with_context(|| format!("persona {name}: {}", inbox.display()))?;
    // Two tasks in one second must not share a file.
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let file = inbox.join(format!("{millis}-{}.md", std::process::id()));
    std::fs::write(&file, task)?;
    let line = format!(
        "Read {} and do what it asks, through ljos; it is your next task as {name}.",
        file.display()
    );
    let declared = declared_tools();
    let started = home.join(".started");
    let argv_for = |resume: bool| {
        runner_argv(runner, resume).with_context(|| {
            format!(
                "persona {name}: runner {runner:?} is not a [[harness]] in {}",
                crate::harnesses_path().display()
            )
        })
    };
    let script = home.join("pane.sh");
    match pane_state(name, &home, &declared) {
        Pane::Live(tool, pane) => {
            tools::send(&tool, &vars_for(name, &home, &pane).with("line", line))
                .with_context(|| format!("persona {name}: the line did not reach its pane"))?;
            return Ok(describe(&tool.name, &pane, name));
        }
        Pane::Idle(tool, pane) if !tool.respawn.is_empty() => {
            let again = argv_for(true)?;
            std::fs::write(&script, pane_script(name, &again, &again, &home))?;
            let vars = vars_for(name, &home, &pane);
            tools::run(&tool.name, &tools::fill(&tool.respawn, &vars))?;
            tools::wait_ready(&tool, &vars);
            tools::send(&tool, &vars.with("line", line))
                .with_context(|| format!("persona {name}: the line did not reach its pane"))?;
            return Ok(describe(&tool.name, &pane, name));
        }
        Pane::Idle(..) | Pane::Gone => {}
    }
    let argv = argv_for(started.exists())?;
    let again = argv_for(true)?;
    std::fs::write(&script, pane_script(name, &argv, &again, &home))?;
    let only = std::env::var("LJOS_PANE_TOOL")
        .ok()
        .filter(|v| !v.is_empty());
    let here = tools::available(&declared, only.as_deref());
    if here.is_empty() {
        bail!("persona {name}: {}", tools::doctor_state(&declared).1);
    }
    let mut refused = Vec::new();
    for tool in here {
        let vars = vars_for(name, &home, "");
        let pane = match tools::open(&tool, &vars) {
            Ok(p) => p,
            Err(e) => {
                refused.push(format!("{e:#}"));
                continue;
            }
        };
        write_record(&home, &tool.name, &pane)?;
        let _ = std::fs::write(&started, crate::now_utc());
        let vars = vars_for(name, &home, &pane);
        tools::wait_ready(&tool, &vars);
        tools::send(&tool, &vars.with("line", line))
            .with_context(|| format!("persona {name}: the line did not reach {}", tool.name))?;
        let mut said = describe(&tool.name, &pane, name);
        if !refused.is_empty() {
            said.push_str(&format!("; before it, {}", refused.join("; ")));
        }
        return Ok(said);
    }
    bail!("persona {name}: every tool refused: {}", refused.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runner_starts_fresh_then_resumes_its_home_session() {
        let all: crate::Harnesses = toml::from_str(concat!(
            "[[harness]]\nname = \"grok\"\nresume = [\"grok\", \"--continue\"]\n",
            "[[harness]]\nname = \"plain\"\nstart = [\"plain-cli\", \"--tui\"]\n",
        ))
        .unwrap();
        assert_eq!(runner_argv_in(&all, "grok", false).unwrap(), ["grok"]);
        assert_eq!(
            runner_argv_in(&all, "grok", true).unwrap(),
            ["grok", "--continue"]
        );
        assert_eq!(
            runner_argv_in(&all, "plain", true).unwrap(),
            ["plain-cli", "--tui"],
            "no resume, a fresh start"
        );
        assert!(runner_argv_in(&all, "nobody", false).is_none());
    }

    #[test]
    fn the_pane_runs_the_runner_as_the_persona_and_stays_open() {
        let s = pane_script(
            "buildengineer",
            &["grok".into(), "--continue".into()],
            &["grok".into(), "--continue".into()],
            Path::new("/s/personas/buildengineer"),
        );
        assert!(s.contains("cd '/s/personas/buildengineer'"));
        assert!(s.contains("LJOS_SEAT='buildengineer' 'grok' '--continue'"));
        assert!(s.trim_end().ends_with("-i"));
    }

    /// A runner that fails is resumed until the restart budget runs out; one
    /// that exits 0 is not run again; the pid file is gone either way.
    #[test]
    fn the_pane_resumes_a_failing_runner_a_bounded_number_of_times() {
        let dir = tempfile::tempdir().unwrap();
        let count = dir.path().join("runs");
        let fail = vec![
            "sh".to_string(),
            "-c".into(),
            format!("echo run >> {}; exit 3", count.display()),
        ];
        let script = pane_script("p", &fail, &fail, dir.path())
            .replace("exec \"${SHELL:-/bin/sh}\" -i\n", "");
        let script = script.replace("sleep 1\n", "");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let runs = std::fs::read_to_string(&count).unwrap().lines().count();
        assert_eq!(runs as u32, 1 + RESTARTS, "{said}");
        assert!(said.contains("not restarting"), "{said}");
        assert!(!dir.path().join(".runner.pid").exists());
        let ok = vec![
            "sh".to_string(),
            "-c".into(),
            format!("echo run >> {}.ok", count.display()),
        ];
        let script =
            pane_script("p", &ok, &fail, dir.path()).replace("exec \"${SHELL:-/bin/sh}\" -i\n", "");
        std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .status()
            .unwrap();
        let runs = std::fs::read_to_string(format!("{}.ok", count.display()))
            .unwrap()
            .lines()
            .count();
        assert_eq!(runs, 1, "a runner that exits 0 is done");
    }

    #[test]
    fn a_pane_record_round_trips_and_names_the_pane() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), "herdr", "w1:p2").unwrap();
        assert_eq!(
            read_record(dir.path()),
            Some(("herdr".to_string(), "w1:p2".to_string()))
        );
        assert_eq!(describe("herdr", "w1:p2", "rev"), "herdr pane w1:p2");
        assert_eq!(describe("tmux", "", "rev"), "tmux ljos-personas:rev");
        assert!(!runner_up(dir.path()), "no pid file, no runner");
        std::fs::write(
            dir.path().join(".runner.pid"),
            std::process::id().to_string(),
        )
        .unwrap();
        assert!(runner_up(dir.path()));
    }
}
