//! `ljos mesh up`, `ljos mesh down` and `ljos mesh acl`: join this machine
//! to its swarm's tailnet on a Headscale login server, leave it, and print
//! the policy that server enforces. A seat works without the mesh, so `up`
//! and `down` end every failure with one line and exit 0.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// `mesh.toml` in the ljos config directory. Every port the mesh uses is
/// written there once, and `up` and `acl` both read it.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    login_server: String,
    /// The environment variable that holds the join key.
    key_env: String,
    swarm: String,
    ports: Ports,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Ports {
    api: u16,
    store_ro: u16,
    docs_ro: u16,
    drop: u16,
    /// This machine's SOCKS5 proxy in userspace mode. Not a box port.
    socks: u16,
}

impl Ports {
    fn on_box(&self) -> [u16; 4] {
        [self.api, self.store_ro, self.docs_ro, self.drop]
    }
}

/// The bound on one `up` or `down`, in seconds: the `timeout -k 5 30` the
/// documented caller wraps a sitting in. `LJOS_MESH_WAIT` sets another.
const WAIT_SECS: u64 = 30;
/// How long a stopped tailscaled has to exit before it is killed: the
/// `-k 5` of that bound.
const GRACE: Duration = Duration::from_secs(5);
/// A delay a person reads as no delay: 0.1 s (Nielsen, Usability
/// Engineering, 1993, §5.5).
const INSTANT_MS: u64 = 100;
/// The step between looks at a socket or a process. Each polled wait ends
/// up to one step late, and one `up` sits through at most two of them (a
/// stale daemon's exit, then the new socket), so two steps fit in
/// `INSTANT_MS`. A look is one stat or one signal.
const POLL: Duration = Duration::from_millis(INSTANT_MS / 2);
/// Random hex digits in a node name. Three bytes give N = 2^24 names, so n
/// nodes in one swarm share a name with odds near n²/2N (the birthday
/// bound): under 1 in 1,000 for at most 183 nodes, under 1 in 100 for at
/// most 579.
const NODE_HEX: usize = 6;
/// The longest swarm id a node name fits: a DNS label is at most 63
/// octets (RFC 1035 §2.3.4), less `ag-`, a dash and the hex digits.
const SWARM_MAX: usize = 63 - "ag-".len() - 1 - NODE_HEX;
/// CAP_NET_ADMIN's bit in a capability mask (linux/capability.h).
const CAP_NET_ADMIN: u32 = 12;

const SOCKET: &str = "tailscaled.sock";
const PID: &str = "tailscaled.pid";
const LOG: &str = "tailscaled.log";
const NODE: &str = "node";
const JOINED: &str = "joined";

fn wait() -> Duration {
    let secs = std::env::var("LJOS_MESH_WAIT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(WAIT_SECS);
    Duration::from_secs(secs)
}

/// The socket, the node name, tailscaled's pid and log: outside any
/// checkout, and gone after `mesh down`.
fn state_dir() -> PathBuf {
    crate::runtime_dir_path().join("mesh")
}

/// The node `ljos mesh up` joined as, while the tailscaled it started runs.
/// A daemon that is gone takes the label with it.
#[must_use]
pub fn joined_node() -> Option<String> {
    let dir = state_dir();
    let node = std::fs::read_to_string(dir.join(JOINED))
        .ok()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())?;
    if still_up(&dir) {
        return Some(node);
    }
    let _ = std::fs::remove_file(dir.join(JOINED));
    None
}

/// Whether the pid file still names the tailscaled `start_daemon`
/// recorded. On Linux that is one read of `/proc`, so every write checks.
#[cfg(target_os = "linux")]
fn still_up(dir: &Path) -> bool {
    recorded(dir).is_some_and(|(_, ours)| ours)
}

/// The same check with `ps` run once per pid file in this process, so a
/// pack write does not start a process each time. Later writes ask only
/// whether the pid is live. A pid reused between two writes of one
/// process keeps the label until that process ends.
#[cfg(not(target_os = "linux"))]
fn still_up(dir: &Path) -> bool {
    use std::sync::{Mutex, PoisonError};
    static SEEN: Mutex<Option<String>> = Mutex::new(None);
    let Ok(text) = std::fs::read_to_string(dir.join(PID)) else {
        return false;
    };
    let mut seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    let pid = text
        .lines()
        .next()
        .and_then(|l| l.trim().parse::<libc::pid_t>().ok());
    // SAFETY: signal 0 sends nothing; it only asks whether the pid exists.
    let live = pid.is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0);
    if live && seen.as_deref() == Some(text.as_str()) {
        return true;
    }
    let ours = recorded(dir).is_some_and(|(_, ours)| ours);
    *seen = ours.then_some(text);
    ours
}

/// Returns the start time of `pid` in clock ticks since boot. It returns
/// none when no live process has that pid. A reused pid starts later, so
/// the start time pins down one process.
#[cfg(target_os = "linux")]
fn started(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The name before the last `)` may contain spaces. After it come the
    // state (field 3) and, 19 fields on, the start time (field 22), as
    // proc_pid_stat(5) numbers them.
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    if matches!(fields.next()?, "Z" | "X") {
        return None;
    }
    fields.nth(18).map(str::to_string)
}

/// When `pid` started, as `ps` prints it, or none when no process has it.
/// `lstart` counts whole seconds, so a pid reused within the second the
/// daemon started would match. macOS hands out pids in turn up to 99,999,
/// so that needs the pids to wrap within that second.
#[cfg(not(target_os = "linux"))]
fn started(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let when = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !when.is_empty()).then_some(when)
}

/// The pid and start time in the pid file. An ljos before the start time
/// check wrote the pid alone.
fn pid_file(dir: &Path) -> Option<(u32, Option<String>)> {
    let text = std::fs::read_to_string(dir.join(PID)).ok()?;
    let mut lines = text.lines();
    let pid = lines.next()?.trim().parse::<u32>().ok()?;
    let at = lines.next().map(str::trim).filter(|at| !at.is_empty());
    Some((pid, at.map(str::to_string)))
}

/// The pid in the pid file, and whether it still names the tailscaled
/// `start_daemon` recorded: the same pid with the same start time.
fn recorded(dir: &Path) -> Option<(u32, bool)> {
    let (pid, at) = pid_file(dir)?;
    let ours = at.is_some_and(|at| started(pid).as_deref() == Some(at.as_str()));
    Some((pid, ours))
}

/// The note for a pid file with no start time, or none.
fn old_format(dir: &Path, then: &str) -> Option<String> {
    let (pid, None) = pid_file(dir)? else {
        return None;
    };
    Some(format!(
        "{PID} names pid {pid} with no start time, so {then}"
    ))
}

fn load() -> Result<Config> {
    let path = crate::config_base()
        .context("no config directory: HOME and XDG_CONFIG_HOME are unset")?
        .join("ljos")
        .join("mesh.toml");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("no mesh config at {}", path.display()))?;
    let config: Config =
        toml::from_str(&text).with_context(|| format!("{} does not read", path.display()))?;
    check_swarm(&config.swarm)?;
    Ok(config)
}

fn check_swarm(swarm: &str) -> Result<()> {
    let label = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
    if swarm.is_empty()
        || swarm.len() > SWARM_MAX
        || !swarm.bytes().all(label)
        || swarm.starts_with('-')
        || swarm.ends_with('-')
    {
        bail!(
            "swarm id {swarm:?} is not 1 to {SWARM_MAX} lowercase letters, digits and inner dashes"
        );
    }
    Ok(())
}

/// Whitespace folded to single spaces, and the key, when one was read,
/// replaced wherever a child echoed it.
fn one_line(text: &str, key: Option<&str>) -> String {
    let text = key.map_or_else(|| text.to_string(), |k| text.replace(k, "[key]"));
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `ljos mesh up`: one line, and the caller exits 0 whatever it says.
#[must_use]
pub fn up() -> String {
    let deadline = Instant::now() + wait();
    let off = |e: anyhow::Error| format!("mesh up: {e:#}; staying off the mesh");
    match load() {
        Err(e) => one_line(&off(e), None),
        Ok(config) => {
            let key = std::env::var(&config.key_env)
                .ok()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty());
            let line = join(&config, key.as_deref(), deadline).unwrap_or_else(off);
            one_line(&line, key.as_deref())
        }
    }
}

fn join(config: &Config, key: Option<&str>, deadline: Instant) -> Result<String> {
    let tailscale = which::which("tailscale").map_err(|_| anyhow!("tailscale is not on PATH"))?;
    let tailscaled =
        which::which("tailscaled").map_err(|_| anyhow!("tailscaled is not on PATH"))?;
    let dir = state_dir();
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .with_context(|| format!("{} could not be made", dir.display()))?;
    }
    let node = node_name(&dir, &config.swarm)?;
    let swarm = &config.swarm;
    let socket = dir.join(SOCKET);
    let answered = status(&tailscale, &socket, &config.key_env, deadline)?;
    if answered
        .as_ref()
        .is_some_and(|s| s["BackendState"] == "Running" && s["Self"]["HostName"] == node.as_str())
    {
        std::fs::write(dir.join(JOINED), &node)?;
        let old = old_format(
            &dir,
            "writes carry no source.mesh label and mesh down signals nothing",
        )
        .map(|note| format!("; {note}"))
        .unwrap_or_default();
        return Ok(format!(
            "mesh up: {node} is already up in swarm {swarm}{old}"
        ));
    }
    let key = key.ok_or_else(|| anyhow!("{} is unset", config.key_env))?;
    let kernel = kernel_tun();
    let mut started = None;
    let mut stale = None;
    if answered.is_none() {
        stale = stop_daemon(&dir, deadline);
        let _ = std::fs::remove_file(&socket);
        started = Some(start_daemon(&tailscaled, &dir, config, kernel)?);
    }
    let joined = wait_for_socket(&socket, started.as_mut(), deadline)
        .and_then(|()| login(&tailscale, &socket, config, key, &node, deadline));
    if let Err(e) = joined {
        if let Some(mut child) = started {
            kill_group(child.id(), libc::SIGKILL);
            let _ = child.wait();
            let _ = std::fs::remove_file(dir.join(PID));
            let _ = std::fs::remove_file(&socket);
        }
        return Err(e);
    }
    std::fs::write(dir.join(JOINED), &node)?;
    let mode = if kernel {
        "a kernel TUN".to_string()
    } else {
        format!("userspace mode, SOCKS5 on localhost:{}", config.ports.socks)
    };
    let unrecorded = started.as_ref().and_then(|_| {
        old_format(
            &dir,
            "writes carry no source.mesh label and mesh down signals nothing",
        )
    });
    let notes: String = stale
        .into_iter()
        .chain(unrecorded)
        .map(|note| format!("; {note}"))
        .collect();
    Ok(format!(
        "mesh up: {node} joined swarm {swarm} in {mode}{notes}"
    ))
}

/// `ag-<swarm>-<hex>`, made once per state directory. The machine's host
/// name is never read, so nothing it holds reaches the login server.
fn node_name(dir: &Path, swarm: &str) -> Result<String> {
    let path = dir.join(NODE);
    let prefix = format!("ag-{swarm}-");
    if let Ok(kept) = std::fs::read_to_string(&path) {
        let kept = kept.trim();
        let ours = kept
            .strip_prefix(&prefix)
            .is_some_and(|hex| hex.len() == NODE_HEX && hex.bytes().all(|b| b.is_ascii_hexdigit()));
        if ours {
            return Ok(kept.to_string());
        }
    }
    let mut bytes = [0u8; NODE_HEX / 2];
    getrandom::fill(&mut bytes).map_err(|e| anyhow!("no random bytes for a node name: {e}"))?;
    let node = format!("{prefix}{}", hex::encode(bytes));
    std::fs::write(&path, &node)?;
    Ok(node)
}

/// Kernel TUN when this process holds CAP_NET_ADMIN and `/dev/net/tun`
/// exists. Anywhere else, userspace networking.
fn kernel_tun() -> bool {
    let caps = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|l| l.strip_prefix("CapEff:"))
                .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
        });
    caps.is_some_and(|c| (c >> CAP_NET_ADMIN) & 1 == 1) && Path::new("/dev/net/tun").exists()
}

/// A tailscale command on this state's socket. The key's variable is not
/// passed on, so no child holds the key in its environment.
fn tailscale_on(tailscale: &Path, socket: &Path, key_env: &str) -> Command {
    let mut cmd = Command::new(tailscale);
    cmd.arg(format!("--socket={}", socket.display()));
    if !key_env.is_empty() {
        cmd.env_remove(key_env);
    }
    cmd
}

fn code(status: std::process::ExitStatus) -> String {
    status
        .code()
        .map_or_else(|| "on a signal".into(), |c| c.to_string())
}

fn run_within(cmd: Command, deadline: Instant, what: &str) -> Result<Output> {
    let left = deadline.saturating_duration_since(Instant::now());
    crate::output_within(cmd, None, left).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            anyhow!("{what} did not finish within {} s", wait().as_secs())
        } else {
            anyhow!("{what} could not run: {e}")
        }
    })
}

/// `tailscale status --json`, or none when no daemon answers on `socket`.
/// A status still running at the deadline leaves no time to join.
fn status(
    tailscale: &Path,
    socket: &Path,
    key_env: &str,
    deadline: Instant,
) -> Result<Option<Value>> {
    if !socket.exists() {
        return Ok(None);
    }
    let mut cmd = tailscale_on(tailscale, socket, key_env);
    cmd.args(["status", "--json"]);
    let left = deadline.saturating_duration_since(Instant::now());
    match crate::output_within(cmd, None, left) {
        Ok(out) => Ok(serde_json::from_slice(&out.stdout).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            bail!(
                "tailscale status did not finish within {} s",
                wait().as_secs()
            )
        }
        Err(_) => Ok(None),
    }
}

/// tailscaled in its own process group, so `mesh down` can stop it and
/// what it started. The node key stays in memory, its state and logs stay
/// in `dir`, and no log leaves for a log server.
fn start_daemon(tailscaled: &Path, dir: &Path, config: &Config, kernel: bool) -> Result<Child> {
    use std::os::unix::process::CommandExt as _;
    let log = std::fs::File::create(dir.join(LOG))?;
    let mut cmd = Command::new(tailscaled);
    cmd.arg("--state=mem:")
        .arg(format!("--statedir={}", dir.display()))
        .arg(format!("--socket={}", dir.join(SOCKET).display()))
        .arg("--no-logs-no-support")
        .env("TS_LOGS_DIR", dir)
        .env_remove(&config.key_env);
    if !kernel {
        cmd.arg("--tun=userspace-networking")
            .arg(format!("--socks5-server=localhost:{}", config.ports.socks));
    }
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0);
    let child = cmd.spawn().context("tailscaled could not start")?;
    record(dir, child.id(), started(child.id()))?;
    Ok(child)
}

/// Write the pid file: the daemon's pid and its start time `at`. With no
/// start time the file has the pid alone, as an older ljos wrote it, and
/// the daemon runs on unlabelled: `down` cannot tell it from a reused pid.
fn record(dir: &Path, pid: u32, at: Option<String>) -> Result<()> {
    let at = at.map(|at| format!("{at}\n")).unwrap_or_default();
    std::fs::write(dir.join(PID), format!("{pid}\n{at}"))?;
    Ok(())
}

fn wait_for_socket(socket: &Path, mut child: Option<&mut Child>, deadline: Instant) -> Result<()> {
    while !socket.exists() {
        if let Some(status) = child.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
            bail!("tailscaled exited {status} before it opened its socket");
        }
        if Instant::now() >= deadline {
            bail!("tailscaled opened no socket within {} s", wait().as_secs());
        }
        std::thread::sleep(POLL);
    }
    Ok(())
}

fn login(
    tailscale: &Path,
    socket: &Path,
    config: &Config,
    key: &str,
    node: &str,
    deadline: Instant,
) -> Result<()> {
    let mut cmd = tailscale_on(tailscale, socket, &config.key_env);
    cmd.args(["up", "--login-server", &config.login_server])
        .args(["--authkey", key, "--hostname", node])
        .args(["--accept-routes=false", "--accept-dns=false"]);
    let out = run_within(cmd, deadline, "tailscale up")?;
    if out.status.success() {
        return Ok(());
    }
    let said = [&out.stderr, &out.stdout]
        .into_iter()
        .find_map(|bytes| {
            String::from_utf8_lossy(bytes)
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(str::to_string)
        })
        .map(|line| format!(": {line}"))
        .unwrap_or_default();
    bail!("tailscale up exited {}{said}", code(out.status))
}

/// Signal the process group `pid` leads.
fn kill_group(pid: u32, signal: libc::c_int) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // Group 1 is every process the user can signal, and group 0 is this one.
    if pid <= 1 {
        return false;
    }
    // SAFETY: a negative pid signals the group `start_daemon` made with
    // `process_group(0)`, which this process is not in.
    unsafe { libc::kill(-pid, signal) == 0 }
}

/// Stop the tailscaled `start_daemon` recorded, within the grace. A pid
/// that no longer names it is never signalled, and the note says so.
fn stop_daemon(dir: &Path, deadline: Instant) -> Option<String> {
    let note = match recorded(dir) {
        Some((pid, true)) => {
            if kill_group(pid, libc::SIGTERM) {
                let until = deadline.min(Instant::now() + GRACE);
                while kill_group(pid, 0) && Instant::now() < until {
                    std::thread::sleep(POLL);
                }
                // A pid is not reused while a group with that id has a
                // process in it (POSIX, Process Group Lifetime). So the
                // group is still this daemon's while the pid names no live
                // process or still names the daemon, and only then does it
                // get SIGKILL.
                if started(pid).is_none() || recorded(dir).is_some_and(|(_, ours)| ours) {
                    kill_group(pid, libc::SIGKILL);
                }
            }
            None
        }
        Some((pid, false)) => old_format(dir, "nothing was signalled").or_else(|| {
            Some(format!(
                "{PID} named pid {pid}, no longer the tailscaled ljos started, so nothing was signalled"
            ))
        }),
        None => None,
    };
    let _ = std::fs::remove_file(dir.join(PID));
    note
}

/// `ljos mesh down`: log out, stop tailscaled, remove the state directory.
/// One line, and the caller exits 0 whatever it says.
#[must_use]
pub fn down() -> String {
    let dir = state_dir();
    if !dir.exists() {
        return "mesh down: not on the mesh".into();
    }
    let deadline = Instant::now() + wait();
    let key_env = load().map(|c| c.key_env).unwrap_or_default();
    let node = std::fs::read_to_string(dir.join(NODE))
        .map(|n| n.trim().to_string())
        .unwrap_or_default();
    let mut notes = Vec::new();
    let socket = dir.join(SOCKET);
    if socket.exists() {
        match which::which("tailscale") {
            Ok(tailscale) => {
                let mut cmd = tailscale_on(&tailscale, &socket, &key_env);
                cmd.arg("logout");
                match run_within(cmd, deadline, "tailscale logout") {
                    Ok(out) if out.status.success() => {}
                    Ok(out) => notes.push(format!("tailscale logout exited {}", code(out.status))),
                    Err(e) => notes.push(format!("{e:#}")),
                }
            }
            Err(_) => notes.push("tailscale is not on PATH, so there was no logout".into()),
        }
    }
    notes.extend(stop_daemon(&dir, deadline));
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        notes.push(format!("{} stays: {e}", dir.display()));
    }
    let who = if node.is_empty() {
        "this machine"
    } else {
        &node
    };
    let mut line = format!("mesh down: {who} left the mesh");
    if !notes.is_empty() {
        line = format!("{line}; {}", notes.join("; "));
    }
    one_line(&line, None)
}

/// `ljos mesh acl`: the Headscale ACL policy for the config's swarm and
/// each of `more`. A swarm reaches its own nodes on every port and the box
/// on its listed ports. There is no other rule, so no swarm reaches
/// another and the box reaches none. No user owns a tag: the join key
/// carries it, so a node cannot tag itself into another swarm.
///
/// # Errors
///
/// A config that does not read, or a swarm id that cannot be a tag.
pub fn acl(more: &[String]) -> Result<String> {
    let config = load()?;
    let mut swarms = vec![config.swarm.as_str()];
    for swarm in more {
        check_swarm(swarm)?;
        if !swarms.contains(&swarm.as_str()) {
            swarms.push(swarm);
        }
    }
    let ports = config.ports.on_box().map(|p| p.to_string()).join(",");
    let mut owners = serde_json::Map::new();
    owners.insert("tag:box".into(), json!([]));
    let mut acls = Vec::new();
    for swarm in swarms {
        let tag = format!("tag:swarm-{swarm}");
        acls.push(json!({"action": "accept", "src": [tag], "dst": [format!("{tag}:*")]}));
        acls.push(json!({"action": "accept", "src": [tag], "dst": [format!("tag:box:{ports}")]}));
        owners.insert(tag, json!([]));
    }
    Ok(serde_json::to_string_pretty(
        &json!({"tagOwners": owners, "acls": acls}),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemon_whose_start_time_cannot_be_read_runs_on_and_down_signals_nothing() {
        use std::os::unix::process::CommandExt as _;
        let dir = tempfile::tempdir().unwrap();
        // `cat` on a pipe this test holds runs until it is killed.
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        record(dir.path(), pid, None).unwrap();
        let named = format!("{PID} names pid {pid} with no start time");
        let up = old_format(
            dir.path(),
            "writes carry no source.mesh label and mesh down signals nothing",
        )
        .expect("up says the pid file has no start time");
        assert!(up.starts_with(&named), "{up}");
        let down = stop_daemon(dir.path(), Instant::now() + GRACE)
            .expect("down says why it signalled nothing");
        assert!(
            down.starts_with(&named) && down.ends_with("nothing was signalled"),
            "{down}"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "down signalled pid {pid}"
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}
