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

/// The processes above `pid`, nearest first, up to and including pid 1:
/// in a container that may be the harness itself.
#[must_use]
pub fn above(pid: u32) -> Vec<Proc> {
    let mut out = Vec::new();
    let mut next = stat(pid).map(|s| s.ppid);
    while let Some(p) = next.filter(|&p| p > 0 && out.len() < DEPTH) {
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

/// `pid`'s parent and its own name.
#[cfg(not(target_os = "macos"))]
fn parent_and_name(pid: u32) -> Option<(u32, String)> {
    stat(pid).map(|s| (s.ppid, s.comm))
}

#[cfg(target_os = "macos")]
fn parent_and_name(pid: u32) -> Option<(u32, String)> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: the buffer is a zeroed proc_bsdinfo of the size passed.
    let n = unsafe {
        libc::proc_pidinfo(
            libc::c_int::try_from(pid).ok()?,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::addr_of_mut!(info).cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    let comm: Vec<u8> = info
        .pbi_comm
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| c.to_ne_bytes()[0])
        .collect();
    Some((info.pbi_ppid, String::from_utf8_lossy(&comm).into_owned()))
}

fn split_environ(raw: &[u8]) -> Vec<(String, String)> {
    raw.split(|b| *b == 0)
        .filter_map(|kv| {
            let kv = String::from_utf8_lossy(kv);
            let (k, v) = kv.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// The environment `pid` was started with. A program it runs can set its
/// own variables, but cannot change what its parent was started with.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn environ(pid: u32) -> Option<Vec<(String, String)>> {
    std::fs::read(format!("/proc/{pid}/environ"))
        .ok()
        .map(|raw| split_environ(&raw))
}

#[cfg(target_os = "macos")]
#[must_use]
pub fn environ(pid: u32) -> Option<Vec<(String, String)>> {
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROCARGS2,
        libc::c_int::try_from(pid).ok()?,
    ];
    let mut size: libc::size_t = 0;
    // SAFETY: a null buffer asks sysctl for the size alone.
    let asked = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if asked != 0 || size < 4 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: buf holds the size sysctl reported.
    let read = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return None;
    }
    buf.truncate(size);
    // argc, the executable path and its padding, argc words, then the
    // environment up to an empty string.
    let argc = usize::try_from(i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?)).ok()?;
    let mut words = buf[4..]
        .split(|b| *b == 0)
        .skip(1)
        .skip_while(|w| w.is_empty());
    for _ in 0..argc {
        words.next()?;
    }
    let env: Vec<u8> = words
        .take_while(|w| !w.is_empty())
        .flat_map(|w| w.iter().copied().chain([0]))
        .collect();
    Some(split_environ(&env))
}

/// The process the nearest runner on this process's chain started: the
/// shell it opened for a tool call, or this process when the runner ran
/// it directly. `None` when the chain reaches the person's session or the
/// top without a runner.
#[must_use]
pub fn started_by_runner() -> Option<u32> {
    let mut child = std::process::id();
    for _ in 0..DEPTH {
        let (ppid, _) = parent_and_name(child)?;
        if ppid == 0 {
            return None;
        }
        let (_, comm) = parent_and_name(ppid)?;
        if crate::is_harness_process(ppid, &comm) {
            return Some(child);
        }
        if crate::is_session(&comm) || ppid == 1 {
            return None;
        }
        child = ppid;
    }
    None
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

fn is_harness(p: &Proc) -> bool {
    crate::is_harness_process(p.pid, &p.comm)
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

/// Where a chain stops being the hook's own: the person's session, a
/// terminal or the init system ([`crate::is_session`]), or pid 1 unless
/// it is a harness. Every sitting in that session shares it, and it
/// outlives each of them.
fn ends_the_walk(p: &Proc) -> bool {
    crate::is_session(&p.comm) || (p.pid == 1 && !is_harness(p))
}

/// The program that ran the process whose ancestors are `chain`, read up
/// to the session ([`ends_the_walk`]), which is never the program: the
/// nearest harness ([`crate::is_harness_process`]), so a `uv`, `npx` or
/// `node` that only started the hook and exits with it does not outrank
/// the harness above it; else the nearest one that is neither a shell
/// nor a wrapper, which keeps a harness that runs as plain `node`; else
/// the outermost of the shells and wrappers below the session. `None`
/// when the session ran the process itself.
#[must_use]
pub fn runner(chain: &[Proc]) -> Option<&Proc> {
    let own = chain.iter().take_while(|p| !ends_the_walk(p));
    own.clone()
        .find(|p| is_harness(p))
        .or_else(|| own.clone().find(|p| !is_shell(p) && !is_wrapper(p)))
        .or_else(|| own.last())
}

/// The runners that spawn `ljos hook`, by the name of the program they
/// are: `claude-code` is Claude Code's npm package.
const HOOK_RUNNERS: &[(&str, &str)] = &[
    ("claude", "claude"),
    ("claude-code", "claude"),
    ("codex", "codex"),
    ("cursor-agent", "cursor-agent"),
    ("cursor", "cursor"),
    ("grok", "grok"),
    ("opencode", "opencode"),
    ("gemini", "gemini"),
    ("agy", "agy"),
    ("antigravity", "agy"),
];

/// Runtimes a runner may ship as: the program is the directory or script
/// above them.
const RUNTIMES: &[&str] = &["node", "nodejs", "bun", "deno"];

/// The runner a program file is, read up its path past versions, places
/// and entry scripts: `~/.local/share/claude/versions/2.1.3` is Claude
/// Code, and so is `@anthropic-ai/claude-code/cli.js`. A bundled runtime
/// (`cursor-agent/versions/V/node`) is the runner it ships in.
fn runner_of_file(path: &std::path::Path) -> Option<&'static str> {
    let mut parts: Vec<String> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(str::to_string))
        .collect();
    while let Some(last) = parts.pop() {
        let last = last.trim_end_matches(" (deleted)");
        let name = last
            .strip_suffix(".js")
            .or_else(|| last.strip_suffix(".mjs"))
            .or_else(|| last.strip_suffix(".cjs"))
            .unwrap_or(last);
        if crate::version_like(name)
            || crate::PLACES.contains(&name)
            || crate::ENTRY_STEMS.contains(&name)
            || RUNTIMES.contains(&name)
        {
            continue;
        }
        let name = name.split_once("-x86_64").map_or(name, |(n, _)| n);
        let name = name.split_once("-aarch64").map_or(name, |(n, _)| n);
        return HOOK_RUNNERS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, r)| *r);
    }
    None
}

/// The runner `pid` is, from the file the kernel runs it from, never
/// from the name it gives itself. A runtime such as `node` is the runner
/// whose script it runs, that script's path resolved on disk.
#[cfg(target_os = "linux")]
#[must_use]
pub fn exe_runner(pid: u32) -> Option<&'static str> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    if let Some(r) = runner_of_file(&exe) {
        return Some(r);
    }
    let base = exe.file_name()?.to_str()?;
    if !RUNTIMES.iter().any(|r| base.starts_with(r)) {
        return None;
    }
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let script = cmdline
        .split(|b| *b == 0)
        .skip(1)
        .map(String::from_utf8_lossy)
        .find(|a| !a.is_empty() && !a.starts_with('-'))?;
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
    let script = std::fs::canonicalize(cwd.join(script.as_ref())).ok()?;
    runner_of_file(&script)
}

#[cfg(not(target_os = "linux"))]
#[must_use]
pub fn exe_runner(_pid: u32) -> Option<&'static str> {
    None
}

/// Whether `cmd`, a shell's `-c` line, runs `ljos hook` and nothing else:
/// the program (`ljos`, a path to it, or the plugin launcher with `ljos`),
/// `hook`, and flags. `${NAME}` and double quotes are the runner's own
/// spelling of the launcher; a redirection, pipe or second command,
/// which a tool call needs to hand the hook a payload, is not.
fn bare_hook_command(cmd: &str) -> bool {
    let mut plain = String::new();
    let mut rest = cmd;
    while let Some(at) = rest.find("${") {
        plain.push_str(&rest[..at]);
        let Some(close) = rest[at..].find('}') else {
            return false;
        };
        let name = &rest[at + 2..at + close];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        plain.push_str("VAR");
        rest = &rest[at + close + 1..];
    }
    plain.push_str(rest);
    let plain = plain.replace('"', "");
    if plain.contains(|c: char| "<>|;&$`(){}\\'\n\r*?[]~#!".contains(c)) {
        return false;
    }
    let words: Vec<&str> = plain.split_whitespace().collect();
    let base = |w: &str| w.rsplit('/').next().unwrap_or(w).to_string();
    let at = match words.first().map(|w| base(w)).as_deref() {
        Some("ljos-plugin") if words.get(1) == Some(&"ljos") => 2,
        Some("ljos") => 1,
        _ => return false,
    };
    words.get(at) == Some(&"hook")
        && words[at + 1..].iter().all(|w| {
            w.starts_with("--")
                || w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

/// Whether `pid` is the shell a runner opens to run its configured hook
/// command: a shell by the file it runs from, started as `SHELL -c CMD`
/// with [`bare_hook_command`] for CMD. `dash` forks that command rather
/// than replacing itself with it.
#[cfg(target_os = "linux")]
fn hook_shell(pid: u32) -> bool {
    let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else {
        return false;
    };
    let shell = exe
        .file_name()
        .and_then(|f| f.to_str())
        .is_some_and(|f| SHELLS.contains(&f));
    let argv: Vec<String> = std::fs::read(format!("/proc/{pid}/cmdline"))
        .unwrap_or_default()
        .split(|b| *b == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    shell
        && matches!(argv.as_slice(), [_, c, cmd, last] if c == "-c" && last.is_empty() && bare_hook_command(cmd))
}

#[cfg(not(target_os = "linux"))]
fn hook_shell(_pid: u32) -> bool {
    false
}

/// The nearest process above this one that is a runner by the file the
/// kernel runs it from ([`exe_runner`]), and which runner it is.
#[must_use]
pub fn runner_above() -> Option<(Proc, &'static str)> {
    ancestors()
        .into_iter()
        .find_map(|p| exe_runner(p.pid).map(|r| (p, r)))
}

/// Variables a runner gives the shell of a tool call. A runner started
/// with one runs inside another runner's tool call.
fn tool_call_stamp(env: &[(String, String)]) -> Option<&str> {
    env.iter()
        .find(|(k, v)| {
            matches!(
                k.as_str(),
                "CLAUDECODE" | "CURSOR_AGENT" | "CODEX_THREAD_ID"
            ) || (crate::runner_session_var(k, v) && !k.starts_with("LJOS_"))
        })
        .map(|(k, _)| k.as_str())
}

/// The runner that spawned this `ljos hook` itself, or why none did: the
/// parent must be a runner's own program ([`exe_runner`]), with no shell
/// or interpreter between them but the `sh -c` the runner runs its hook
/// command in ([`hook_shell`]), and that runner must not run inside a
/// tool call, which a runner above it or a tool call's variables in the
/// environment it was started with would show. Only such a hook's prompt
/// is the person's.
///
/// # Errors
///
/// Why the parent is not a runner the person started.
pub fn hook_runner() -> Result<(Proc, &'static str), String> {
    let mut chain = ancestors();
    let parent = chain.first().ok_or("it has no parent process")?;
    if exe_runner(parent.pid).is_none() && chain.len() > 1 && hook_shell(parent.pid) {
        chain.remove(0);
    }
    let parent = &chain[0];
    let Some(runner) = exe_runner(parent.pid) else {
        return Err(format!(
            "its parent, {} (pid {}), is not a runner's own program: a shell, an interpreter or ssh ran it, so a tool call may have",
            parent.comm, parent.pid
        ));
    };
    let env = environ(parent.pid).unwrap_or_default();
    if let Some(key) = tool_call_stamp(&env) {
        return Err(format!(
            "{runner} (pid {}) was started with {key} set, inside another runner's tool call",
            parent.pid
        ));
    }
    // The Cursor app also hosts the person's own terminals; its agent's
    // shells carry `CURSOR_AGENT`, which the stamp above reads.
    let own = chain[1..].iter().take_while(|p| !ends_the_walk(p));
    if let Some((p, other)) = own
        .filter_map(|p| exe_runner(p.pid).map(|r| (p, r)))
        .find(|(_, r)| *r != "cursor")
    {
        return Err(format!(
            "{runner} (pid {}) runs under {other} (pid {}), inside a tool call",
            parent.pid, p.pid
        ));
    }
    Ok((parent.clone(), runner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runner_is_read_from_the_file_it_runs_from() {
        use std::path::Path;
        for (path, want) in [
            ("/home/u/.local/share/claude/versions/2.1.3", Some("claude")),
            ("/home/u/.local/bin/claude", Some("claude")),
            (
                "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
                Some("claude"),
            ),
            (
                "/usr/lib/node_modules/@openai/codex/vendor/x86_64-unknown-linux-musl/codex/codex",
                Some("codex"),
            ),
            (
                "/home/u/.local/share/cursor-agent/versions/2025.10.2-abc/node",
                Some("cursor-agent"),
            ),
            ("/usr/share/cursor/cursor", Some("cursor")),
            ("/usr/bin/node", None),
            ("/usr/bin/bash", None),
            ("/usr/bin/python3", None),
            ("/usr/bin/ssh", None),
            ("/home/u/.cargo/bin/ljos", None),
        ] {
            assert_eq!(runner_of_file(Path::new(path)), want, "{path}");
        }
    }

    #[test]
    fn a_tool_calls_variables_mark_a_runner_started_inside_one() {
        let env = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
        assert_eq!(tool_call_stamp(&env("CLAUDECODE", "1")), Some("CLAUDECODE"));
        assert_eq!(
            tool_call_stamp(&env("CURSOR_AGENT", "1")),
            Some("CURSOR_AGENT")
        );
        assert!(tool_call_stamp(&env("CURSOR_CONVERSATION_ID", "0199a1b2-c3d4")).is_some());
        assert_eq!(tool_call_stamp(&env("LJOS_SESSION_ID", "mine-0001")), None);
        assert_eq!(tool_call_stamp(&env("HOME", "/home/u")), None);
    }

    #[test]
    fn only_a_bare_hook_command_is_the_runners_hook_shell() {
        for cmd in [
            "ljos hook",
            "/home/u/.cargo/bin/ljos hook --runner codex",
            "ljos hook --event SessionStart",
            "\"${CLAUDE_PLUGIN_ROOT}/bin/ljos-plugin\" ljos hook",
            "\"${CURSOR_PLUGIN_ROOT}/bin/ljos-plugin\" ljos hook",
        ] {
            assert!(bare_hook_command(cmd), "{cmd}");
        }
        for cmd in [
            "ljos hook < plant.json",
            "cat plant.json | ljos hook",
            "ljos hook <<'EOF'\n{}\nEOF",
            "ljos status; ljos hook",
            "ljos hook $(cat p)",
            "python3 plant.py",
            "ljos remember a b c d",
            "ljos-plugin hook",
            "bash -c 'ljos hook'",
        ] {
            assert!(!bare_hook_command(cmd), "{cmd}");
        }
    }

    /// The test runner is no runner's program, so a hook it spawned is
    /// not the person's.
    #[test]
    fn a_hook_this_test_runs_has_no_runner() {
        let why = hook_runner().unwrap_err();
        assert!(why.contains("not a runner's own program"), "{why}");
    }

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
    fn a_child_is_read_with_the_environment_it_was_started_with() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .env("LJOS_PROC_TREE_PROBE", "started-with")
            .spawn()
            .unwrap();
        // Until the child has exec'd, its environment is the fork's.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let env = loop {
            let env = environ(child.id()).unwrap_or_default();
            if env.iter().any(|(k, _)| k == "LJOS_PROC_TREE_PROBE")
                || std::time::Instant::now() > deadline
            {
                break env;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(
            env.iter()
                .any(|(k, v)| k == "LJOS_PROC_TREE_PROBE" && v == "started-with"),
            "{env:?}"
        );
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

    #[test]
    fn a_session_above_the_hook_is_never_its_runner() {
        for session in ["tmux", "tmux: server", "gnome-terminal-", "systemd", "sshd"] {
            let chain = [named(4, "sh"), named(5, "bash"), named(2, session)];
            assert_eq!(runner(&chain).map(|p| p.pid), Some(5), "{session}");
            let chain = [named(4, "sh"), named(2, session), named(10, "claude")];
            assert_eq!(runner(&chain).map(|p| p.pid), Some(4), "{session}");
            assert_eq!(runner(&[named(2, session), named(10, "claude")]), None);
        }
        let chain = [named(4, "sh"), named(7, "git"), named(2, "kitty")];
        assert_eq!(runner(&chain).map(|p| p.pid), Some(7));
    }

    /// A process no kernel runs, so no `/proc` entry names it: pids stop
    /// below 2^22.
    fn fake(n: u32, comm: &str) -> Proc {
        named((1 << 22) + n, comm)
    }

    #[test]
    fn a_harness_outranks_a_runtime_that_only_started_the_hook() {
        let chain = [
            fake(1, "sh"),
            fake(2, "uv"),
            fake(3, "uv"),
            fake(4, "claude"),
        ];
        assert_eq!(runner(&chain), Some(&chain[3]));
        let chain = [fake(1, "sh"), fake(2, "node"), fake(3, "claude")];
        assert_eq!(runner(&chain), Some(&chain[2]));
        let chain = [
            fake(1, "sh"),
            fake(2, "npx"),
            fake(3, "bash"),
            fake(4, "cursor-agent"),
            fake(5, "tmux"),
        ];
        assert_eq!(runner(&chain), Some(&chain[3]));
        let chain = [
            fake(1, "sh"),
            fake(2, "uv"),
            fake(3, "codex"),
            fake(4, "agy"),
        ];
        assert_eq!(runner(&chain), Some(&chain[2]), "the nearest harness");
    }

    #[test]
    fn a_node_harness_with_no_known_harness_above_is_the_runner() {
        let chain = [
            fake(1, "sh"),
            fake(2, "node"),
            fake(3, "bash"),
            fake(4, "tmux"),
        ];
        assert_eq!(runner(&chain), Some(&chain[1]));
        assert_eq!(runner(&chain[..2]), Some(&chain[1]));
        // The walk stops at the session: a harness above it is not read.
        let chain = [
            fake(1, "sh"),
            fake(2, "node"),
            fake(3, "tmux"),
            fake(4, "claude"),
        ];
        assert_eq!(runner(&chain), Some(&chain[1]));
    }

    #[test]
    fn pid_1_is_the_runner_only_as_a_harness() {
        let chain = [fake(1, "sh"), fake(2, "uv"), named(1, "claude")];
        assert_eq!(runner(&chain), Some(&chain[2]), "a container's harness");
        let chain = [fake(1, "sh"), fake(2, "bash"), named(1, "tini")];
        assert_eq!(runner(&chain), Some(&chain[1]), "an init is no runner");
        let chain = [fake(1, "sh"), fake(2, "node"), named(1, "dumb-init")];
        assert_eq!(runner(&chain), Some(&chain[1]));
        assert_eq!(runner(&[named(1, "tini")]), None);
    }

    #[test]
    fn the_walk_reads_up_to_a_process_with_no_parent() {
        let chain = ancestors();
        let top = chain.last().expect("the test runner is above this process");
        assert!(
            chain.len() == DEPTH || stat(top.pid).is_some_and(|s| s.ppid == 0),
            "{chain:?}"
        );
    }
}
