//! `ljos mesh up`, `ljos mesh down`, `ljos mesh status` and `ljos mesh acl`:
//! join this machine to the Headscale mesh, leave it, print the nodes the
//! daemon reports, and print the policy `mesh.toml` describes. `up` takes a
//! key file, a login server and a hostname. A seat works without the mesh.
//! A command that could not do what was asked prints one line and exits
//! non-zero. `mesh down` when this machine is not on the mesh exits 0.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// `mesh.toml` in the ljos config directory. `ljos mesh acl` reads it.
/// `up` and `down` do not: the join comes from flags and the environment.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[allow(dead_code)]
    login_server: String,
    /// Kept so a file written for an earlier mesh still parses.
    #[allow(dead_code)]
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
    /// Not a box port. `up` uses [`SOCKS_PORT`] on its own.
    #[allow(dead_code)]
    socks: u16,
}

impl Ports {
    fn on_box(&self) -> [u16; 4] {
        [self.api, self.store_ro, self.docs_ro, self.drop]
    }
}

/// The leidarljos Headscale server. `--login-server` and
/// `LJOS_MESH_LOGIN_SERVER` override it.
const DEFAULT_LOGIN_SERVER: &str = "https://hs.grund.rgoswami.me";

/// The bound on one `up` or `down`, in seconds: the `timeout -k 5 30` the
/// documented caller wraps a sitting in. `LJOS_MESH_WAIT` sets another.
/// Each external call in the join uses this same bound.
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
/// CAP_NET_ADMIN's bit in a capability mask (linux/capability.h). Creating
/// a TUN interface needs it. Opening `/dev/net/tun` does not.
const CAP_NET_ADMIN: u32 = 12;

/// SOCKS5 port in userspace mode. Tailscale's userspace networking guide
/// uses 1055, and `examples/mesh.toml` records the same port. The proxy
/// listens on localhost only.
const SOCKS_PORT: u16 = 1055;

/// Static tailscale build used when `tailscale` or `tailscaled` is not on
/// `PATH`. The archive on pkgs.tailscale.com is
/// `tailscale_1.90.6_amd64.tgz`.
const TAILSCALE_VERSION: &str = "1.90.6";
/// pkgs.tailscale.com names the x86_64 archive `amd64`.
const TAILSCALE_ARCH: &str = "amd64";
/// sha256 of that tarball. The published file
/// `tailscale_1.90.6_amd64.tgz.sha256` is this hex and nothing else
/// (pkgs.tailscale.com, 2026-11-03).
const TARBALL_SHA256: &str = "cc51b5b6fe86a25e66b2ef037334e799a61d06dc64fabcadfe6e2a7588a2b2c7";
/// Bytes in that tarball: Content-Length on pkgs.tailscale.com,
/// 2026-11-03. The reader stops here so a different body cannot fill the
/// disk; the sha256 then rejects it.
const TARBALL_MAX_BYTES: u64 = 33_387_158;

const SOCKET: &str = "tailscaled.sock";
const PID: &str = "tailscaled.pid";
const LOG: &str = "tailscaled.log";
const JOINED: &str = "joined";
const RELEASE: &str = "release";

/// Arguments for `ljos mesh up`. Empty fields fall through to the environment
/// and then to the defaults in [`up`].
pub struct UpArgs {
    pub key_file: Option<PathBuf>,
    pub login_server: Option<String>,
    pub hostname: Option<String>,
}

/// What `up` or `down` prints. `error` goes to stderr and the process
/// exits non-zero: the command could not do what was asked.
#[must_use]
pub struct MeshOut {
    text: String,
    error: bool,
}

impl MeshOut {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }

    /// Print the line on stdout, or on stderr when it records a failure.
    pub fn write(&self) {
        if self.text.is_empty() {
            return;
        }
        if self.error {
            eprintln!("{}", self.text);
        } else {
            println!("{}", self.text);
        }
    }

    /// Whether the command could not do what was asked.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.error
    }
}

struct Fail {
    step: &'static str,
    err: anyhow::Error,
}

impl Fail {
    fn step(step: &'static str, err: anyhow::Error) -> Self {
        Self { step, err }
    }

    fn line(&self, verb: &str) -> String {
        one_line(&format!("mesh {verb}: {}: {:#}", self.step, self.err))
    }
}

fn call_timeout() -> Duration {
    let secs = std::env::var("LJOS_MESH_WAIT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(WAIT_SECS);
    Duration::from_secs(secs)
}

fn timed_out(err: &anyhow::Error) -> bool {
    err.to_string().starts_with("did not finish")
}

/// Binaries, the socket, the pid and the log: under the ljos state
/// directory, outside any checkout.
fn state_dir() -> PathBuf {
    crate::state_base_dir().join("mesh")
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
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "no mesh config at {}. Copy examples/mesh.toml from the ljos repository to that path. `ljos mesh up` does not read this file",
            path.display()
        )
    })?;
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

/// Whitespace folded to single spaces.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The join key, read only so a child that echoes it can be scrubbed.
/// The file is not copied and the bytes are not written anywhere.
fn redact(text: &str, key_file: Option<&Path>) -> String {
    let Some(path) = key_file else {
        return text.to_string();
    };
    let Ok(raw) = std::fs::read(path) else {
        return text.to_string();
    };
    let owned = String::from_utf8_lossy(&raw);
    let mut needle = owned.trim().to_string();
    let out = if needle.is_empty() {
        text.to_string()
    } else {
        text.replace(&needle, "[key]")
    };
    use zeroize::Zeroize;
    needle.zeroize();
    out
}

fn known_key(args: &UpArgs) -> Option<PathBuf> {
    let path = args
        .key_file
        .clone()
        .or_else(|| std::env::var_os("LJOS_MESH_KEY_FILE").map(PathBuf::from))?;
    path.is_file().then_some(path)
}

fn scrub(text: &str, key_file: Option<&Path>) -> String {
    one_line(&redact(text, key_file))
}

/// `ljos mesh up`. A successful join prints this node's name, its mesh
/// IPv4 and IPv6, and each peer's name and addresses. Any failure is one
/// line; [`MeshOut::write`] sends it to stderr.
pub fn up(args: UpArgs) -> MeshOut {
    let key = known_key(&args);
    match bring_up(&args) {
        Ok(text) => MeshOut::ok(redact(&text, key.as_deref())),
        Err(fail) => MeshOut::err(scrub(&fail.line("up"), key.as_deref())),
    }
}

fn bring_up(args: &UpArgs) -> Result<String, Fail> {
    let dir = state_dir();
    ensure_dir(&dir).map_err(|e| Fail::step("state", e))?;
    let hostname = hostname_of(args).map_err(|e| Fail::step("hostname", e))?;
    let server = login_server_of(args);
    let (tailscale, tailscaled) = ensure_bins(&dir)?;
    let socket = dir.join(SOCKET);
    let answered =
        status_json(&tailscale, &socket).map_err(|e| Fail::step("tailscale status", e))?;
    if answered
        .as_ref()
        .is_some_and(|status| running_as(status, &hostname))
    {
        let _ = std::fs::write(dir.join(JOINED), format!("{hostname}\n"));
        let mut notes = Vec::new();
        if let Some(note) = old_format(
            &dir,
            "writes carry no source.mesh label and mesh down signals nothing",
        ) {
            notes.push(note);
        }
        return Ok(with_notes(
            mesh_status(&tailscale, &socket, &hostname)?,
            &notes,
        ));
    }
    let key = key_file(args).map_err(|e| Fail::step("key file", e))?;
    let kernel = tun_usable();
    let (started, stale) = daemon_for(&tailscaled, &dir, &socket, answered.is_none(), kernel)?;
    let started_here = started.is_some();
    if let Err(e) = login(&tailscale, &socket, &server, &key, &hostname) {
        reap(started, &dir);
        return Err(Fail::step("tailscale up", e));
    }
    // Detach. Dropping `Child` does not stop the daemon.
    drop(started);
    let _ = std::fs::write(dir.join(JOINED), format!("{hostname}\n"));
    let mut notes = Vec::new();
    if let Some(note) = stale {
        notes.push(note);
    }
    if started_here {
        if let Some(note) = old_format(
            &dir,
            "writes carry no source.mesh label and mesh down signals nothing",
        ) {
            notes.push(note);
        }
    }
    Ok(with_notes(
        mesh_status(&tailscale, &socket, &hostname)?,
        &notes,
    ))
}

fn with_notes(line: String, notes: &[String]) -> String {
    if notes.is_empty() {
        line
    } else {
        format!("{line}; {}", notes.join("; "))
    }
}

/// This node, then each peer, from `tailscale status --json`.
/// `TailscaleIPs` lists the mesh addresses; a colon marks IPv6.
fn mesh_status(tailscale: &Path, socket: &Path, hostname: &str) -> Result<String, Fail> {
    match status_json(tailscale, socket) {
        Ok(Some(status)) => Ok(status_line(&status, hostname)),
        Ok(None) => Ok(format!("mesh up: {hostname}")),
        Err(e) if timed_out(&e) => Err(Fail::step("tailscale status", e)),
        Err(_) => Ok(format!("mesh up: {hostname}")),
    }
}

fn status_line(status: &Value, hostname: &str) -> String {
    let name = status["Self"]["HostName"]
        .as_str()
        .filter(|name| !name.is_empty())
        .unwrap_or(hostname);
    let (v4, v6) = mesh_addrs(&status["Self"]);
    let mut parts = vec![name.to_string()];
    if !v4.is_empty() {
        parts.push(v4);
    }
    if !v6.is_empty() {
        parts.push(v6);
    }
    let mut line = format!("mesh up: {}", parts.join(" "));
    let peers = peer_list(status);
    if !peers.is_empty() {
        line.push_str(" peer ");
        line.push_str(&peers);
    }
    line
}

fn mesh_addrs(node: &Value) -> (String, String) {
    let mut v4 = String::new();
    let mut v6 = String::new();
    if let Some(ips) = node["TailscaleIPs"].as_array() {
        for ip in ips.iter().filter_map(Value::as_str) {
            if ip.contains(':') {
                if v6.is_empty() {
                    v6 = ip.to_string();
                }
            } else if v4.is_empty() {
                v4 = ip.to_string();
            }
        }
    }
    (v4, v6)
}

fn peer_list(status: &Value) -> String {
    let Some(peers) = status["Peer"].as_object() else {
        return String::new();
    };
    let mut lines = Vec::new();
    for peer in peers.values() {
        let name = peer["HostName"].as_str().unwrap_or("");
        let (v4, v6) = mesh_addrs(peer);
        let mut bits = Vec::new();
        if !name.is_empty() {
            bits.push(name.to_string());
        }
        if !v4.is_empty() {
            bits.push(v4);
        }
        if !v6.is_empty() {
            bits.push(v6);
        }
        if !bits.is_empty() {
            lines.push(bits.join(" "));
        }
    }
    lines.sort();
    lines.join(", ")
}

fn daemon_for(
    tailscaled: &Path,
    dir: &Path,
    socket: &Path,
    start: bool,
    kernel: bool,
) -> Result<(Option<Child>, Option<String>), Fail> {
    if !start {
        return Ok((None, None));
    }
    let stale = stop_daemon(dir, Instant::now() + GRACE);
    let _ = std::fs::remove_file(socket);
    let mut child = match start_daemon(tailscaled, dir, kernel) {
        Ok(child) => child,
        Err(e) => return Err(Fail::step("tailscaled", e)),
    };
    if let Err(e) = wait_for_socket(socket, Some(&mut child), Instant::now() + call_timeout()) {
        reap(Some(child), dir);
        return Err(Fail::step("tailscaled", e));
    }
    Ok((Some(child), stale))
}

fn ensure_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("{} could not be made", dir.display()))?;
    Ok(())
}

fn hostname_of(args: &UpArgs) -> Result<String> {
    if let Some(name) = args
        .hostname
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Ok(name.to_string());
    }
    system_hostname()
}

fn system_hostname() -> Result<String> {
    let mut buf = [0u8; 256];
    // POSIX.1-2008 says the name fits in 255 bytes plus a NUL. Linux
    // HOST_NAME_MAX is 64. 256 covers both.
    // SAFETY: gethostname writes at most `buf.len()` bytes into `buf` and
    // does not retain the pointer.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        bail!("gethostname: {}", std::io::Error::last_os_error());
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]).trim().to_string();
    if name.is_empty() {
        bail!("the system hostname is empty");
    }
    Ok(name)
}

fn login_server_of(args: &UpArgs) -> String {
    args.login_server
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("LJOS_MESH_LOGIN_SERVER")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_LOGIN_SERVER.to_string())
}

fn key_file(args: &UpArgs) -> Result<PathBuf> {
    let path = args
        .key_file
        .clone()
        .or_else(|| std::env::var_os("LJOS_MESH_KEY_FILE").map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty())
        .context("set --key-file or LJOS_MESH_KEY_FILE")?;
    let meta = std::fs::metadata(&path).with_context(|| path.display().to_string())?;
    if !meta.is_file() {
        bail!("{} is not a file", path.display());
    }
    Ok(path)
}

fn running_as(status: &Value, hostname: &str) -> bool {
    status["BackendState"].as_str() == Some("Running")
        && status["Self"]["HostName"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(hostname))
}

/// `/dev/net/tun`, or `LJOS_MESH_TUN` when that names another node.
/// Missing, or not openable for read and write, means userspace networking.
fn tun_path() -> PathBuf {
    std::env::var_os("LJOS_MESH_TUN")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/dev/net/tun"))
}

/// Effective capability set includes `CAP_NET_ADMIN`. No `/proc` (not
/// Linux) is an unusable TUN: userspace networking then.
fn cap_net_admin() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return false;
    };
    let Some(hex) = status.lines().find_map(|line| line.strip_prefix("CapEff:")) else {
        return false;
    };
    u64::from_str_radix(hex.trim(), 16).is_ok_and(|caps| (caps >> CAP_NET_ADMIN) & 1 == 1)
}

fn tun_usable() -> bool {
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(tun_path())
        .is_ok();
    opened && cap_net_admin()
}

fn on_path() -> Option<(PathBuf, PathBuf)> {
    Some((
        which::which("tailscale").ok()?,
        which::which("tailscaled").ok()?,
    ))
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn cached(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let release = std::fs::read_to_string(dir.join(RELEASE)).ok()?;
    if release.trim() != TAILSCALE_VERSION {
        return None;
    }
    let tailscale = dir.join("tailscale");
    let tailscaled = dir.join("tailscaled");
    (executable(&tailscale) && executable(&tailscaled)).then_some((tailscale, tailscaled))
}

/// Both binaries from one place. A `tailscale` and a `tailscaled` from
/// different builds disagree on the socket, so a single hit on `PATH`
/// still downloads the pinned pair.
fn ensure_bins(dir: &Path) -> Result<(PathBuf, PathBuf), Fail> {
    if let Some(pair) = on_path() {
        return Ok(pair);
    }
    if let Some(pair) = cached(dir) {
        return Ok(pair);
    }
    fetch(dir)?;
    cached(dir).ok_or_else(|| {
        Fail::step(
            "tailscale",
            anyhow!("the tarball unpacked no tailscale binaries"),
        )
    })
}

fn tarball_name() -> String {
    format!("tailscale_{TAILSCALE_VERSION}_{TAILSCALE_ARCH}.tgz")
}

fn fetch(dir: &Path) -> Result<(), Fail> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(Fail::step(
            "tailscale",
            anyhow!(
                "the pinned tailscale {TAILSCALE_VERSION} build is {TAILSCALE_ARCH}, and this machine is {}",
                std::env::consts::ARCH
            ),
        ));
    }
    let tarball = dir.join(tarball_name());
    let fetched = download(&tarball).map_err(|e| Fail::step("download", e));
    if let Err(fail) = fetched {
        let _ = std::fs::remove_file(&tarball);
        return Err(fail);
    }
    let hashed = match sha256_file(&tarball) {
        Ok(got) if got == TARBALL_SHA256 => Ok(()),
        Ok(got) => Err(Fail::step(
            "checksum",
            anyhow!("published {TARBALL_SHA256}, file {got}"),
        )),
        Err(e) => Err(Fail::step("checksum", e)),
    };
    if let Err(fail) = hashed {
        let _ = std::fs::remove_file(&tarball);
        return Err(fail);
    }
    let unpacked = unpack(&tarball, dir).map_err(|e| Fail::step("tailscale", e));
    let _ = std::fs::remove_file(&tarball);
    unpacked
}

fn download(dest: &Path) -> Result<()> {
    use std::io::Read as _;
    let url = format!("https://pkgs.tailscale.com/stable/{}", tarball_name());
    let agent = ureq::AgentBuilder::new().timeout(call_timeout()).build();
    let resp = agent.get(&url).call().with_context(|| url.to_string())?;
    let mut file =
        std::fs::File::create(dest).with_context(|| format!("write {}", dest.display()))?;
    let mut reader = resp.into_reader().take(TARBALL_MAX_BYTES);
    std::io::copy(&mut reader, &mut file).with_context(|| format!("read {url}"))?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::Digest as _;
    let mut file = std::fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

fn unpack(tarball: &Path, dir: &Path) -> Result<()> {
    let tree = dir.join("unpack");
    let _ = std::fs::remove_dir_all(&tree);
    std::fs::create_dir_all(&tree)?;
    let prefix = format!("tailscale_{TAILSCALE_VERSION}_{TAILSCALE_ARCH}");
    let mut cmd = Command::new("tar");
    cmd.arg("-xzf")
        .arg(tarball)
        .arg("-C")
        .arg(&tree)
        .arg(format!("{prefix}/tailscale"))
        .arg(format!("{prefix}/tailscaled"));
    let out = run(cmd)?;
    if !out.status.success() {
        bail!("tar exited {}{}", code(out.status), last_line(&out));
    }
    for name in ["tailscale", "tailscaled"] {
        place_bin(&tree.join(&prefix).join(name), &dir.join(name))?;
    }
    let _ = std::fs::remove_dir_all(&tree);
    std::fs::write(dir.join(RELEASE), format!("{TAILSCALE_VERSION}\n"))?;
    Ok(())
}

fn place_bin(src: &Path, dest: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let meta =
        std::fs::symlink_metadata(src).with_context(|| format!("missing {}", src.display()))?;
    if !meta.is_file() {
        bail!("{} is not a regular file", src.display());
    }
    let _ = std::fs::remove_file(dest);
    std::fs::rename(src, dest).with_context(|| format!("install {}", dest.display()))?;
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn tailscale_cmd(tailscale: &Path, socket: &Path) -> Command {
    let mut cmd = Command::new(tailscale);
    cmd.arg(format!("--socket={}", socket.display()));
    // The key file path is not a secret. The bytes stay in the file, so
    // no child receives them through the environment either.
    cmd.env_remove("LJOS_MESH_KEY_FILE");
    cmd
}

fn code(status: std::process::ExitStatus) -> String {
    status
        .code()
        .map_or_else(|| "on a signal".into(), |c| c.to_string())
}

fn last_line(out: &Output) -> String {
    [&out.stderr, &out.stdout]
        .into_iter()
        .find_map(|bytes| {
            String::from_utf8_lossy(bytes)
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(|line| format!(": {line}"))
        })
        .unwrap_or_default()
}

fn run(cmd: Command) -> Result<Output> {
    let timeout = call_timeout();
    crate::output_within(cmd, None, timeout).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            anyhow!("did not finish within {} s", timeout.as_secs())
        } else {
            anyhow!("could not run: {e}")
        }
    })
}

/// `tailscale status --json`, or none when no daemon answers on `socket`.
fn status_json(tailscale: &Path, socket: &Path) -> Result<Option<Value>> {
    if !socket.exists() {
        return Ok(None);
    }
    let mut cmd = tailscale_cmd(tailscale, socket);
    cmd.args(["status", "--json"]);
    match run(cmd) {
        Ok(out) if out.status.success() => Ok(serde_json::from_slice(&out.stdout).ok()),
        Ok(_) => Ok(None),
        Err(e) if timed_out(&e) => Err(e),
        Err(_) => Ok(None),
    }
}

/// tailscaled in its own process group, so `mesh down` can stop it and
/// what it started. The node key stays in memory (`--state=mem:`), its
/// state and logs stay in `dir`, and no log leaves for a log server.
fn start_daemon(tailscaled: &Path, dir: &Path, kernel: bool) -> Result<Child> {
    use std::os::unix::process::CommandExt as _;
    let log = std::fs::File::create(dir.join(LOG))?;
    let mut cmd = Command::new(tailscaled);
    cmd.arg("--state=mem:")
        .arg(format!("--statedir={}", dir.display()))
        .arg(format!("--socket={}", dir.join(SOCKET).display()))
        .arg("--no-logs-no-support")
        .env("TS_LOGS_DIR", dir)
        .env_remove("LJOS_MESH_KEY_FILE");
    if !kernel {
        cmd.arg("--tun=userspace-networking")
            .arg(format!("--socks5-server=localhost:{SOCKS_PORT}"));
    }
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0);
    let child = cmd.spawn().context("could not start")?;
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
            bail!("exited {status} before it opened its socket");
        }
        if Instant::now() >= deadline {
            bail!("opened no socket within {} s", call_timeout().as_secs());
        }
        std::thread::sleep(POLL);
    }
    Ok(())
}

fn login(tailscale: &Path, socket: &Path, server: &str, key: &Path, hostname: &str) -> Result<()> {
    let mut cmd = tailscale_cmd(tailscale, socket);
    // The key already carries the tag. Headscale refuses a second request
    // ("requested tags are invalid or not permitted", seen 2026-11-03),
    // so `--advertise-tags` is not passed. `--reset` lets a later up
    // change the flags below without aborting.
    cmd.arg("up")
        .arg("--accept-routes=false")
        .arg("--accept-dns=false")
        .arg(format!("--login-server={server}"))
        .arg(format!("--auth-key=file:{}", key.display()))
        .arg(format!("--hostname={hostname}"))
        .arg("--reset");
    let out = run(cmd)?;
    if out.status.success() {
        return Ok(());
    }
    bail!("exited {}{}", code(out.status), last_line(&out));
}

fn reap(child: Option<Child>, dir: &Path) {
    if let Some(mut child) = child {
        kill_group(child.id(), libc::SIGKILL);
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(dir.join(PID));
    let _ = std::fs::remove_file(dir.join(SOCKET));
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

fn clear_run(dir: &Path) {
    for name in [SOCKET, PID, LOG, JOINED] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    if std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none()) {
        let _ = std::fs::remove_dir(dir);
    }
}

/// `ljos mesh down`: log out, then stop the tailscaled this command
/// started, and only that pid. One line. Not being on the mesh exits 0.
/// A logout that failed exits non-zero.
pub fn down() -> MeshOut {
    let key = std::env::var_os("LJOS_MESH_KEY_FILE")
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    let out = down_inner();
    let text = scrub(&out.text, key.as_deref());
    if out.error {
        MeshOut::err(text)
    } else {
        MeshOut::ok(text)
    }
}

fn down_inner() -> MeshOut {
    let dir = state_dir();
    let socket = dir.join(SOCKET);
    let joined = dir.join(JOINED);
    if !socket.exists() && !dir.join(PID).exists() && !joined.exists() {
        return MeshOut::ok("mesh down: not on the mesh");
    }
    let hostname = std::fs::read_to_string(&joined).unwrap_or_default();
    let hostname = hostname.trim().to_string();
    let mut errors = Vec::new();
    if socket.exists() {
        match logout_bin(&dir) {
            Some(tailscale) => {
                let mut cmd = tailscale_cmd(&tailscale, &socket);
                cmd.arg("logout");
                match run(cmd) {
                    Ok(out) if out.status.success() => {}
                    Ok(out) => errors.push(format!(
                        "tailscale logout: exited {}{}",
                        code(out.status),
                        last_line(&out)
                    )),
                    Err(e) => errors.push(format!("tailscale logout: {e:#}")),
                }
            }
            None => errors.push("tailscale logout: tailscale is not available".into()),
        }
    }
    let note = stop_daemon(&dir, Instant::now() + GRACE);
    clear_run(&dir);
    if errors.is_empty() {
        let who = if hostname.is_empty() {
            "this machine"
        } else {
            &hostname
        };
        let mut line = format!("mesh down: {who} left the mesh");
        if let Some(note) = note {
            line = format!("{line}; {note}");
        }
        MeshOut::ok(line)
    } else {
        let mut line = errors.join("; ");
        if let Some(note) = note {
            line = format!("{line}; {note}");
        }
        MeshOut::err(format!("mesh down: {line}"))
    }
}

fn logout_bin(dir: &Path) -> Option<PathBuf> {
    on_path()
        .map(|(tailscale, _)| tailscale)
        .or_else(|| cached(dir).map(|(tailscale, _)| tailscale))
}

/// `ljos mesh status`: the nodes `tailscale status --json` reports, each
/// with the tags, addresses and id the daemon gave it, and whether the
/// command could not read them. A node the JSON marks ephemeral says so.
/// Nothing here names a particular host. `true` is a missing `tailscale`
/// or a status call that did not finish. Not being on the mesh is an
/// answer, so that line is `false`.
#[must_use]
pub fn status_text() -> (String, bool) {
    let key_env = load().map(|c| c.key_env).unwrap_or_default();
    let Ok(tailscale) = which::which("tailscale") else {
        return (
            "mesh status: tailscale is not on PATH; ljos mesh up downloads the pinned build".into(),
            true,
        );
    };
    let socket = state_dir().join(SOCKET);
    match status_for_text(&tailscale, &socket, &key_env) {
        Ok(Some(value)) => (format_status(&value), false),
        Ok(None) => ("mesh status: not on the mesh".into(), false),
        Err(e) => (format!("mesh status: {e:#}"), true),
    }
}

/// `tailscale status --json` for [`status_text`]. `key_env` is removed from
/// the child, the same way a join does not hand the key to tailscale.
fn status_for_text(tailscale: &Path, socket: &Path, key_env: &str) -> Result<Option<Value>> {
    if !socket.exists() {
        return Ok(None);
    }
    let mut cmd = tailscale_cmd(tailscale, socket);
    if !key_env.is_empty() {
        cmd.env_remove(key_env);
    }
    cmd.args(["status", "--json"]);
    match run(cmd) {
        Ok(out) if out.status.success() => Ok(serde_json::from_slice(&out.stdout).ok()),
        Ok(_) => Ok(None),
        Err(e) if timed_out(&e) => Err(e),
        Err(_) => Ok(None),
    }
}

/// The lines [`status_text`] prints for one status document. Peers come
/// from the `Peer` object. Tags, ids and addresses are the fields the
/// daemon set. A missing `Ephemeral` field is not called either way.
#[must_use]
pub fn format_status(status: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(node) = node_of(&status["Self"]) {
        lines.push(render_node("self", &node));
    }
    if let Some(peers) = status.get("Peer").and_then(Value::as_object) {
        let mut peers: Vec<Node> = peers.values().filter_map(node_of).collect();
        peers.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        for peer in peers {
            lines.push(render_node("peer", &peer));
        }
    }
    if lines.is_empty() {
        "mesh status: no node".into()
    } else {
        lines.join("\n")
    }
}

struct Node {
    name: String,
    id: String,
    ips: Vec<String>,
    tags: Vec<String>,
    ephemeral: Option<bool>,
}

fn node_of(value: &Value) -> Option<Node> {
    let name = value.get("HostName")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let strings = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Node {
        name: name.to_string(),
        id: match value.get("ID") {
            Some(Value::String(id)) => id.trim().to_string(),
            Some(Value::Number(id)) => id.to_string(),
            _ => String::new(),
        },
        ips: strings("TailscaleIPs"),
        tags: strings("Tags"),
        ephemeral: value.get("Ephemeral").and_then(Value::as_bool),
    })
}

fn render_node(kind: &str, node: &Node) -> String {
    let id = if node.id.is_empty() {
        String::new()
    } else {
        format!(" id {}", node.id)
    };
    let ips = if node.ips.is_empty() {
        String::new()
    } else {
        format!(" {}", node.ips.join(" "))
    };
    let tags = if node.tags.is_empty() {
        String::new()
    } else {
        format!(" tags {}", node.tags.join(","))
    };
    let life = match node.ephemeral {
        Some(true) => " ephemeral",
        Some(false) => " non-ephemeral",
        None => "",
    };
    format!("mesh status: {kind} {}{id}{ips}{tags}{life}", node.name)
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
