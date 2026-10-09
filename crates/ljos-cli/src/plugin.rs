//! A harness child, restarted the way OTP restarts one.
//!
//! Permanent restarts however it exited, transient only on a crash,
//! temporary never. `max_restarts` exits inside `window_secs` give up
//! (Armstrong, *Making reliable distributed systems in the presence of
//! software errors*). `ljos supervise` prints a `herdr agent start`
//! line and does not run it. A persona pane is opened by the `[[tool]]`
//! table in `persona_session`, not by that line.

use std::path::Path;

use serde::Serialize;

/// How a child is restarted. The three OTP values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restart {
    /// Restart however it exited.
    Permanent,
    /// Restart only when it crashed.
    Transient,
    /// Never restart.
    Temporary,
}

impl Restart {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "permanent" => Some(Self::Permanent),
            "transient" => Some(Self::Transient),
            "temporary" => Some(Self::Temporary),
            _ => None,
        }
    }
}

/// Whether the child exited on its own or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Clean,
    Crash,
}

impl ExitKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "clean" => Some(Self::Clean),
            "crash" => Some(Self::Crash),
            _ => None,
        }
    }
}

/// What the supervisor does with one exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Restart,
    Stop,
    /// The intensity was exceeded: stop, and do not try again in this window.
    GiveUp,
}

/// One harness child. `max_restarts` exits inside `window_secs` give up.
#[derive(Debug, Clone)]
pub struct Child {
    pub name: String,
    pub harness: String,
    pub restart: Restart,
    pub max_restarts: u32,
    pub window_secs: u64,
}

/// The exits already counted, as seconds since the epoch.
#[derive(Debug, Clone)]
pub struct Supervisor {
    pub child: Child,
    pub failures: Vec<u64>,
}

impl Supervisor {
    /// Classify `kind` at time `now`. A restart records `now` so the next
    /// exit sees it. A give-up does not record another failure.
    pub fn on_exit(&mut self, kind: ExitKind, now: u64) -> Action {
        match (self.child.restart, kind) {
            (Restart::Temporary, _) | (Restart::Transient, ExitKind::Clean) => Action::Stop,
            (Restart::Permanent, _) | (Restart::Transient, ExitKind::Crash) => {
                let window = self.child.window_secs;
                self.failures.retain(|t| now.saturating_sub(*t) < window);
                if self.failures.len() as u32 >= self.child.max_restarts {
                    Action::GiveUp
                } else {
                    self.failures.push(now);
                    Action::Restart
                }
            }
        }
    }
}

/// The herdr line that would start this child again: `herdr agent start
/// persona-NAME --no-focus --cwd HOME -- sh SCRIPT`. The script is the
/// one that execs the harness. `ljos supervise` prints the line and does
/// not run it.
#[must_use]
pub fn herdr_argv(name: &str, home: &Path, script: &Path) -> Vec<String> {
    vec![
        "herdr".into(),
        "agent".into(),
        "start".into(),
        format!("persona-{name}"),
        "--no-focus".into(),
        "--cwd".into(),
        home.display().to_string(),
        "--".into(),
        "sh".into(),
        script.display().to_string(),
    ]
}

/// What `ljos supervise` prints.
#[derive(Debug, Serialize)]
pub struct Decision {
    pub action: Action,
    pub harness: String,
    pub herdr: Vec<String>,
    pub failures: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn child(restart: Restart) -> Supervisor {
        Supervisor {
            child: Child {
                name: "grok-1".into(),
                harness: "grok".into(),
                restart,
                max_restarts: 3,
                window_secs: 60,
            },
            failures: Vec::new(),
        }
    }

    #[test]
    fn a_permanent_child_restarts_and_then_gives_up() {
        let mut sup = child(Restart::Permanent);
        assert_eq!(sup.on_exit(ExitKind::Crash, 10), Action::Restart);
        assert_eq!(sup.on_exit(ExitKind::Clean, 20), Action::Restart);
        assert_eq!(sup.on_exit(ExitKind::Crash, 30), Action::Restart);
        assert_eq!(sup.on_exit(ExitKind::Crash, 40), Action::GiveUp);
        assert_eq!(sup.failures, vec![10, 20, 30]);
    }

    #[test]
    fn an_old_failure_leaves_the_window() {
        let mut sup = child(Restart::Permanent);
        sup.failures = vec![10, 20, 30];
        assert_eq!(sup.on_exit(ExitKind::Crash, 100), Action::Restart);
        assert_eq!(sup.failures, vec![100]);
    }

    #[test]
    fn transient_stops_on_a_clean_exit_and_temporary_never_restarts() {
        let mut sup = child(Restart::Transient);
        assert_eq!(sup.on_exit(ExitKind::Clean, 5), Action::Stop);
        assert!(sup.failures.is_empty());
        assert_eq!(sup.on_exit(ExitKind::Crash, 6), Action::Restart);
        let mut once = child(Restart::Temporary);
        assert_eq!(once.on_exit(ExitKind::Crash, 1), Action::Stop);
    }

    #[test]
    fn herdr_starts_a_grok_child_and_a_claude_child_the_same_way() {
        let grok = herdr_argv(
            "grok-1",
            Path::new("/s/personas/grok-1"),
            Path::new("/s/run.sh"),
        );
        let claude = herdr_argv(
            "claude-1",
            Path::new("/s/personas/claude-1"),
            Path::new("/s/run.sh"),
        );
        assert_eq!(
            grok,
            [
                "herdr",
                "agent",
                "start",
                "persona-grok-1",
                "--no-focus",
                "--cwd",
                "/s/personas/grok-1",
                "--",
                "sh",
                "/s/run.sh",
            ]
        );
        assert_eq!(claude[3], "persona-claude-1");
        assert_eq!(grok[1..3], claude[1..3]);
    }
}
