//! Processes above this one, each named by its pid and its start time, so
//! a pid the kernel has since handed to another process is not taken for
//! the one that was recorded. Read from `/proc`; elsewhere every list is
//! empty and nothing is alive.

/// One process: its pid, when it started (clock ticks after boot), and its
/// name at the time it was read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Proc {
    pub pid: u32,
    pub start: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comm: String,
}

impl Proc {
    /// The same process: the name may change at exec, the start does not.
    #[must_use]
    pub fn same(&self, other: &Self) -> bool {
        self.pid == other.pid && self.start == other.start
    }
}

/// How far up a chain is read.
const DEPTH: usize = 32;

struct Stat {
    comm: String,
    dead: bool,
    ppid: u32,
    start: u64,
}

fn stat(pid: u32) -> Option<Stat> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open = raw.find('(')?;
    let close = raw.rfind(')')?;
    let comm = raw.get(open + 1..close)?.to_string();
    // After the name: state, ppid, ... and starttime, field 22 of stat(5).
    let rest: Vec<&str> = raw.get(close + 1..)?.split_whitespace().collect();
    Some(Stat {
        comm,
        dead: matches!(*rest.first()?, "Z" | "X" | "x"),
        ppid: rest.get(1)?.parse().ok()?,
        start: rest.get(19)?.parse().ok()?,
    })
}

/// The process `pid` names now.
#[must_use]
pub fn of(pid: u32) -> Option<Proc> {
    stat(pid).map(|s| Proc {
        pid,
        start: s.start,
        comm: s.comm,
    })
}

/// The processes above `pid`, nearest first, up to but not including init.
#[must_use]
pub fn above(pid: u32) -> Vec<Proc> {
    let mut out = Vec::new();
    let mut next = stat(pid).map(|s| s.ppid);
    while let Some(p) = next.filter(|&p| p > 1 && out.len() < DEPTH) {
        let Some(s) = stat(p) else {
            break;
        };
        next = Some(s.ppid);
        out.push(Proc {
            pid: p,
            start: s.start,
            comm: s.comm,
        });
    }
    out
}

/// The processes above this one, nearest first.
#[must_use]
pub fn ancestors() -> Vec<Proc> {
    above(std::process::id())
}

/// Whether `p` still runs: the same pid with the same start, and not a
/// zombie waiting to be reaped.
#[must_use]
pub fn alive(p: &Proc) -> bool {
    stat(p.pid).is_some_and(|s| s.start == p.start && !s.dead)
}

/// This boot, so a process recorded before a reboot or on another machine
/// never matches one here.
#[must_use]
pub fn boot() -> &'static str {
    static BOOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BOOT.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()
            .map(|s| {
                s.trim()
                    .chars()
                    .filter(char::is_ascii_hexdigit)
                    .take(12)
                    .collect()
            })
            .filter(|s: &String| !s.is_empty())
            .unwrap_or_else(|| "-".into())
    })
}

/// `p` as one word that names it on this machine and boot.
#[must_use]
pub fn token(p: &Proc) -> String {
    format!("{}:{}:{}", boot(), p.pid, p.start)
}

/// Whether the process a [`token`] names still runs here.
#[must_use]
pub fn token_alive(token: &str) -> bool {
    let mut parts = token.splitn(3, ':');
    let (Some(at), Some(pid), Some(start)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(pid), Ok(start)) = (pid.parse(), start.parse()) else {
        return false;
    };
    at == boot()
        && alive(&Proc {
            pid,
            start,
            comm: String::new(),
        })
}

const SHELLS: &[&str] = &[
    "sh", "bash", "dash", "zsh", "ksh", "mksh", "ash", "yash", "fish", "tcsh", "csh", "busybox",
];

/// Programs that run a command and stand between it and the shell that
/// asked for it.
const WRAPPERS: &[&str] = &[
    "env", "nice", "nohup", "timeout", "stdbuf", "setsid", "ionice", "chrt", "taskset", "time",
    "flock", "sudo", "doas", "xargs",
];

fn is_shell(p: &Proc) -> bool {
    SHELLS.contains(&p.comm.as_str())
}

fn is_wrapper(p: &Proc) -> bool {
    WRAPPERS.contains(&p.comm.as_str())
}

/// The shell that ran the process whose ancestors are `chain`: past any
/// wrappers, the outermost of the shells nested directly above it, so a
/// script that starts a fresh `sh -c` per line is still one shell. `None`
/// when a program other than a shell ran it.
#[must_use]
pub fn calling_shell(chain: &[Proc]) -> Option<&Proc> {
    let mut rest = chain.iter().skip_while(|p| is_wrapper(p));
    let mut top = rest.next().filter(|p| is_shell(p))?;
    for p in rest {
        if is_shell(p) {
            top = p;
        } else if !is_wrapper(p) {
            break;
        }
    }
    Some(top)
}

/// The program that ran the process whose ancestors are `chain`: the
/// nearest one that is neither a shell nor a wrapper. A chain of shells
/// alone ends at its outermost shell.
#[must_use]
pub fn runner(chain: &[Proc]) -> Option<&Proc> {
    chain
        .iter()
        .find(|p| !is_shell(p) && !is_wrapper(p))
        .or_else(|| chain.last())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(pid: u32, comm: &str) -> Proc {
        Proc {
            pid,
            start: u64::from(pid) * 10,
            comm: comm.into(),
        }
    }

    #[test]
    fn this_process_has_live_ancestors_and_a_dead_pid_does_not_match() {
        let chain = ancestors();
        assert!(!chain.is_empty(), "the test runner is above this process");
        assert!(chain.iter().all(alive));
        let parent = &chain[0];
        assert!(token_alive(&token(parent)));
        let reused = Proc {
            start: parent.start + 1,
            ..parent.clone()
        };
        assert!(!alive(&reused));
        assert!(!token_alive(&format!(
            "other-boot:{}:{}",
            parent.pid, parent.start
        )));
    }

    #[test]
    fn a_reaped_child_is_not_alive() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let p = of(pid).unwrap();
        assert!(alive(&p));
        assert_eq!(above(pid).first().map(|q| q.pid), Some(std::process::id()));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!alive(&p));
    }

    #[test]
    fn the_calling_shell_is_the_outermost_of_a_nested_run() {
        let harness = named(10, "claude");
        let chain = [
            named(3, "timeout"),
            named(4, "sh"),
            named(5, "bash"),
            harness.clone(),
        ];
        assert_eq!(calling_shell(&chain), Some(&chain[2]));
        let chain = [
            named(4, "sh"),
            named(6, "env"),
            named(5, "bash"),
            named(7, "node"),
            named(8, "bash"),
        ];
        assert_eq!(calling_shell(&chain), Some(&chain[2]));
        assert_eq!(calling_shell(&[harness.clone(), named(5, "bash")]), None);
        assert_eq!(calling_shell(&[named(3, "env"), harness]), None);
        assert_eq!(calling_shell(&[]), None);
    }

    #[test]
    fn the_runner_is_above_the_shells_and_wrappers_it_started() {
        let chain = [
            named(4, "sh"),
            named(3, "timeout"),
            named(5, "bash"),
            named(10, "claude"),
            named(2, "tmux"),
        ];
        assert_eq!(runner(&chain).map(|p| p.pid), Some(10));
        assert_eq!(runner(&chain[3..]).map(|p| p.pid), Some(10));
        let shells = [named(4, "sh"), named(5, "bash")];
        assert_eq!(runner(&shells).map(|p| p.pid), Some(5));
        assert_eq!(runner(&[]), None);
    }
}
