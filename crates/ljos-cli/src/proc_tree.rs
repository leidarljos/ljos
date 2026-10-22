//! Processes above this one, each named by its pid and its start time, so
//! a pid the kernel has since handed to another process is not taken for
//! the one that was recorded. Read from `/proc`; elsewhere every list is
//! empty and nothing is alive.

use std::path::{Path, PathBuf};

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

/// The stem of a path component, so `cli.js` is the entry `cli`.
fn component_stem(component: &str) -> &str {
    component
        .strip_suffix(".js")
        .or_else(|| component.strip_suffix(".mjs"))
        .or_else(|| component.strip_suffix(".cjs"))
        .unwrap_or(component)
}

/// A path component that names where a runner is packed, not the runner.
fn layout_component(component: &str) -> bool {
    let name = component_stem(component);
    crate::version_like(name)
        || crate::PLACES.contains(&name)
        || crate::ENTRY_STEMS.contains(&name)
        || RUNTIMES.contains(&name)
        || matches!(name, "vendor" | "share")
        || name.starts_with('@')
        || name.contains("unknown-linux")
        || matches!(name, "x86_64" | "aarch64" | "arm64")
        || name.starts_with("armv")
}

/// The runner a component names, after an architecture suffix is dropped.
fn hook_name(component: &str) -> Option<&'static str> {
    let name = component_stem(component);
    let name = name.split_once("-x86_64").map_or(name, |(head, _)| head);
    let name = name.split_once("-aarch64").map_or(name, |(head, _)| head);
    HOOK_RUNNERS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, runner)| *runner)
}

/// The home this user's passwd entry names. A tool call sets `HOME` for
/// whatever it starts, but cannot change this.
#[must_use]
pub fn own_home() -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt as _;
    let mut buf = vec![0u8; 1024];
    loop {
        // SAFETY: passwd is plain data, so all zeroes is a valid value.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: getpwuid_r writes at most buf.len() bytes into buf and
        // points entry's fields into it; buf outlives every read below.
        let rc = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                &mut entry,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut found,
            )
        };
        if rc == libc::ERANGE && buf.len() < 1 << 20 {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if rc != 0 || found.is_null() || entry.pw_dir.is_null() {
            return None;
        }
        // SAFETY: pw_dir is a NUL-terminated string inside buf.
        let dir = unsafe { std::ffi::CStr::from_ptr(entry.pw_dir) };
        let home = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes()));
        return home.is_absolute().then_some(home);
    }
}

/// Where a runner is installed under the person's home ([`own_home`]):
/// a user `bin`, and the prefix npm's own guide gives a user for global
/// packages. These are trusted because the person owns them, which
/// also lets any process of the person's uid, a tool call among them,
/// replace what they hold: the check keeps out other users and copies
/// placed elsewhere, not the person's own account.
const HOME_ROOTS: &[&str] = &[".local/bin", ".cargo/bin", ".npm-global"];

/// Where a runner is installed for every user, and where npm keeps a
/// global package.
const SYSTEM_ROOTS: &[&str] = &[
    "/usr/bin",
    "/bin",
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/usr/lib/node_modules",
    "/usr/local/lib/node_modules",
    "/opt/homebrew/lib/node_modules",
];

/// The directories a runner is installed below, for a person whose
/// passwd home is `home`: the roots above, and an app directory of a
/// runner's name under `~/.local/share` or `/usr/share`, where a native
/// installer keeps the versions `~/.local/bin` links to.
fn install_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = SYSTEM_ROOTS.iter().map(PathBuf::from).collect();
    let mut shares = vec![PathBuf::from("/usr/share")];
    if let Some(home) = home {
        roots.extend(HOME_ROOTS.iter().map(|root| home.join(root)));
        shares.push(home.join(".local/share"));
    }
    for share in shares {
        roots.extend(HOOK_RUNNERS.iter().map(|(name, _)| share.join(name)));
    }
    roots
}

/// Where Nix keeps what it builds.
const NIX_STORE: &str = "/nix/store";

/// The `STORE/HASH-NAME` entry `path` sits in, STORE being Nix's store
/// ([`NIX_STORE`]) and HASH 32 characters of Nix's base 32.
fn nix_entry(path: &Path, store: &Path) -> Option<PathBuf> {
    let name = path.strip_prefix(store).ok()?.iter().next()?.to_str()?;
    let (hash, rest) = name.split_at_checked(32)?;
    let hashed = hash
        .chars()
        .all(|c| "0123456789abcdfghijklmnpqrsvwxyz".contains(c));
    (hashed && rest.len() > 1 && rest.starts_with('-')).then(|| store.join(name))
}

/// The runner `path` names when it sits below a root ([`root_below`]),
/// and that root: the nearest component that is not packing layout
/// ([`layout_component`]), read from the file up to the root's own name,
/// so `share/claude/versions/2.1.3` is Claude Code.
fn runner_below(path: &Path, roots: &[PathBuf], store: &Path) -> Option<(&'static str, PathBuf)> {
    let root = root_below(path, roots, store)?;
    let parts = root
        .file_name()
        .into_iter()
        .chain(path.strip_prefix(&root).ok()?.iter())
        .map(std::ffi::OsStr::to_str)
        .collect::<Option<Vec<_>>>()?;
    let name = parts.iter().rev().find(|part| !layout_component(part))?;
    Some((hook_name(name)?, root))
}

/// The one of `roots`, or the entry of the Nix store `store`, that
/// `path` sits strictly below. The path must be absolute and plain,
/// since a `..` after the root would leave it. A path the kernel marks
/// ` (deleted)` is not the file that was installed.
fn root_below(path: &Path, roots: &[PathBuf], store: &Path) -> Option<PathBuf> {
    let plain = path.is_absolute()
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        });
    if !plain || path.to_str()?.contains(" (deleted)") {
        return None;
    }
    roots
        .iter()
        .cloned()
        .chain(nix_entry(path, store))
        .find(|root| path != root.as_path() && path.starts_with(root))
}

/// A group as the group database lists it.
struct GroupEntry {
    name: std::ffi::OsString,
    members: Vec<std::ffi::OsString>,
}

/// The account lookups [`group_is_private`] makes. `None` is a lookup
/// that failed or that cannot be shown to be complete, which is never
/// read as an empty answer.
trait Accounts {
    fn user_name(&self, uid: u32) -> Option<std::ffi::OsString>;
    fn group(&self, gid: u32) -> Option<GroupEntry>;
    /// The uid of every passwd entry whose primary group is `gid`.
    fn primary_members(&self, gid: u32) -> Option<Vec<u32>>;
}

/// The passwd and group databases: a user's name through the C
/// library, and a group and the accounts in it from `/etc/group` and
/// `/etc/passwd` themselves ([`group_in_files`], [`primary_in_files`]),
/// since a lookup by gid returns only the first line and enumerating
/// NSS can stop short.
struct SystemAccounts;

/// Runs a `get*_r` lookup with a buffer that grows while it is too small.
fn with_buf<T>(mut lookup: impl FnMut(&mut [u8]) -> Result<Option<T>, i32>) -> Option<T> {
    let mut buf = vec![0u8; 1024];
    loop {
        match lookup(&mut buf) {
            Err(libc::ERANGE) if buf.len() < 1 << 20 => buf.resize(buf.len() * 2, 0),
            Ok(found) => return found,
            Err(_) => return None,
        }
    }
}

/// # Safety
/// `ptr` is a NUL-terminated string.
unsafe fn os_string(ptr: *const libc::c_char) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStrExt as _;
    // SAFETY: the caller's contract.
    let bytes = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_bytes();
    std::ffi::OsStr::from_bytes(bytes).to_os_string()
}

impl Accounts for SystemAccounts {
    fn user_name(&self, uid: u32) -> Option<std::ffi::OsString> {
        with_buf(|buf| {
            // SAFETY: passwd is plain data, so all zeroes is a valid value.
            let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
            let mut found: *mut libc::passwd = std::ptr::null_mut();
            // SAFETY: getpwuid_r writes at most buf.len() bytes into buf
            // and points entry's fields into it.
            let rc = unsafe {
                libc::getpwuid_r(
                    uid,
                    &mut entry,
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                    &mut found,
                )
            };
            if rc != 0 {
                return Err(rc);
            }
            // SAFETY: a found entry's pw_name is a string inside buf.
            Ok((!found.is_null() && !entry.pw_name.is_null())
                .then(|| unsafe { os_string(entry.pw_name) }))
        })
    }

    fn group(&self, gid: u32) -> Option<GroupEntry> {
        group_in_files(Path::new(ETC), gid)
    }

    fn primary_members(&self, gid: u32) -> Option<Vec<u32>> {
        primary_in_files(Path::new(ETC), gid)
    }
}

/// Where the account files and `nsswitch.conf` are.
const ETC: &str = "/etc";

/// Whether `etc/nsswitch.conf` sends database `db` to `files` and
/// nothing else, so the file under `etc` is all of it. A missing
/// file, a missing or repeated line, or any other source, such as
/// `systemd`, `sss`, `ldap` or `compat`, may add accounts that file
/// does not list.
fn files_only(etc: &Path, db: &str) -> bool {
    let Ok(conf) = std::fs::read_to_string(etc.join("nsswitch.conf")) else {
        return false;
    };
    let mut lines = conf
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim() == db);
    let (Some((_, sources)), None) = (lines.next(), lines.next()) else {
        return false;
    };
    sources.split_whitespace().eq(["files"])
}

/// The one line of `etc/group` for `gid`, read only when group is
/// that file alone ([`files_only`]). `None` when no line or more than
/// one has that gid, since a lookup returns only the first and the
/// members of the others still hold the group, and when the file
/// cannot be read whole, as for `passwd` ([`primary_in_files`]).
fn group_in_files(etc: &Path, gid: u32) -> Option<GroupEntry> {
    if !files_only(etc, "group") {
        return None;
    }
    let text = std::fs::read_to_string(etc.join("group")).ok()?;
    let mut found = None;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with(['+', '-']) {
            return None;
        }
        let fields: Vec<&str> = line.split(':').collect();
        let [name, _, id, members] = fields[..] else {
            return None;
        };
        if id.parse::<u32>().ok()? != gid {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(GroupEntry {
            name: name.into(),
            members: members
                .split(',')
                .filter(|m| !m.is_empty())
                .map(Into::into)
                .collect(),
        });
    }
    found
}

/// The uid of every line of `etc/passwd` whose primary group is `gid`,
/// read only when passwd is that file alone ([`files_only`]). `None`
/// when the file cannot be read whole: a line that is not seven fields
/// with numeric ids, or a `+` or `-` line, which names accounts kept
/// elsewhere.
fn primary_in_files(etc: &Path, gid: u32) -> Option<Vec<u32>> {
    if !files_only(etc, "passwd") {
        return None;
    }
    let text = std::fs::read_to_string(etc.join("passwd")).ok()?;
    let mut uids = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with(['+', '-']) {
            return None;
        }
        let fields: Vec<&str> = line.split(':').collect();
        let [_, _, uid, primary, _, _, _] = fields[..] else {
            return None;
        };
        let (uid, primary) = (uid.parse::<u32>().ok()?, primary.parse::<u32>().ok()?);
        if primary == gid {
            uids.push(uid);
        }
    }
    Some(uids)
}

/// Whether group `gid` holds no one but user `uid`: it bears the user's
/// name, lists no other member, and is no other account's primary group.
fn group_is_private(accounts: &dyn Accounts, uid: u32, gid: u32) -> bool {
    let (Some(user), Some(group), Some(primary)) = (
        accounts.user_name(uid),
        accounts.group(gid),
        accounts.primary_members(gid),
    ) else {
        return false;
    };
    group.name == user
        && group.members.iter().all(|member| *member == user)
        && primary.iter().all(|other| *other == uid)
}

/// Who [`person_keeps`] judges a file for: this process's user,
/// primary group and supplementary groups (`None` when they could not
/// be read), and whether the primary group is the user's alone, asked
/// only when a file is group-writable.
struct Person<'a> {
    uid: u32,
    gid: u32,
    groups: Option<Vec<u32>>,
    accounts: &'a dyn Accounts,
    private_group: std::cell::OnceCell<bool>,
}

impl<'a> Person<'a> {
    fn new(accounts: &'a dyn Accounts) -> Self {
        // SAFETY: getuid and getgid take no arguments and only read the
        // process identity.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        Person {
            uid,
            gid,
            groups: supplementary_groups(),
            accounts,
            private_group: std::cell::OnceCell::new(),
        }
    }

    /// Whether the person writes what group `gid` may write. Groups that
    /// could not be read count as every group.
    fn in_group(&self, gid: u32) -> bool {
        gid == self.gid
            || self
                .groups
                .as_ref()
                .is_none_or(|groups| groups.contains(&gid))
    }

    fn private_group(&self) -> bool {
        *self
            .private_group
            .get_or_init(|| group_is_private(self.accounts, self.uid, self.gid))
    }
}

/// This process's supplementary groups.
fn supplementary_groups() -> Option<Vec<u32>> {
    // SAFETY: a size of zero asks only for the count and writes nothing.
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    let mut groups = vec![0; usize::try_from(count).ok()?];
    // SAFETY: getgroups writes at most `count` ids into groups.
    let got = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
    groups.truncate(usize::try_from(got).ok()?);
    Some(groups)
}

/// A file's owner, group and mode.
#[derive(Clone, Copy)]
struct Owner {
    uid: u32,
    gid: u32,
    mode: u32,
}

impl From<&std::fs::Metadata> for Owner {
    fn from(meta: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        Owner {
            uid: meta.uid(),
            gid: meta.gid(),
            mode: meta.mode(),
        }
    }
}

/// The owner of `/` and of each directory below it down to `file`, and
/// of `file`, each read from a descriptor opened beneath the one before
/// without following a link, so a directory swapped for a link after
/// `file` was resolved is not walked through. One swapped for another
/// directory leads to another file than `expect`, the (device, inode)
/// the caller holds for `file`. `None` unless every component above
/// `file` is a directory and the file the walk ends on is a regular
/// file that is `expect`.
#[cfg(target_os = "linux")]
fn pinned_walk(file: &Path, expect: (u64, u64)) -> Option<Vec<Owner>> {
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::MetadataExt as _;
    let open = |dir: libc::c_int, name: &std::ffi::CStr| {
        // SAFETY: name is NUL-terminated, and openat only reads it.
        let fd = unsafe {
            libc::openat(
                dir,
                name.as_ptr(),
                libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        // SAFETY: a descriptor openat returned is open and owned here.
        (fd >= 0).then(|| std::fs::File::from(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) }))
    };
    let mut parts = file.components();
    if parts.next() != Some(std::path::Component::RootDir) {
        return None;
    }
    let mut at = open(libc::AT_FDCWD, c"/")?;
    let mut meta = at.metadata().ok()?;
    let mut owners = vec![Owner::from(&meta)];
    for part in parts {
        let std::path::Component::Normal(name) = part else {
            return None;
        };
        if !meta.is_dir() {
            return None;
        }
        at = open(
            at.as_raw_fd(),
            &std::ffi::CString::new(name.as_bytes()).ok()?,
        )?;
        meta = at.metadata().ok()?;
        owners.push(Owner::from(&meta));
    }
    (meta.is_file() && expect == (meta.dev(), meta.ino())).then_some(owners)
}

#[cfg(not(target_os = "linux"))]
fn pinned_walk(_file: &Path, _expect: (u64, u64)) -> Option<Vec<Owner>> {
    None
}

/// Whether a Nix store and an entry in it are as Nix leaves them, so
/// that only Nix can change what the entry holds. The store is root's,
/// the person cannot write it, and it is sticky if its group or anyone
/// may write it; the entry is then root's, as the daemon builds it. On
/// a single-user install the store is the person's and no one else may
/// write it, and the entry is the person's or root's. Such a store, like
/// a home root ([`HOME_ROOTS`]), is writable by a tool call of the
/// person's uid, and is trusted only because the person owns it.
fn store_keeps(store: Owner, entry: Owner, person: &Person) -> bool {
    let group = store.mode & 0o020 != 0;
    let anyone = store.mode & 0o002 != 0;
    if store.uid == 0 {
        let person_writes = anyone
            || (group && person.in_group(store.gid))
            || (person.uid == 0 && store.mode & 0o200 != 0);
        !person_writes && (!group || store.mode & 0o1000 != 0) && entry.uid == 0
    } else {
        store.uid == person.uid && !group && !anyone && (entry.uid == 0 || entry.uid == person.uid)
    }
}

/// Whether root or `person` owns a file, and no one else can write it
/// but `person`'s primary group when that group is theirs alone.
fn person_keeps(file: Owner, person: &Person) -> bool {
    (file.uid == 0 || file.uid == person.uid)
        && file.mode & 0o002 == 0
        && (file.mode & 0o020 == 0 || (file.gid == person.gid && person.private_group()))
}

/// The runner a program file is, when the file is an installed copy for
/// the person whose passwd home is `home` ([`runner_below`]).
/// `~/.local/share/claude/versions/2.1.3` is Claude Code, and so is
/// `/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js`. A bundled
/// runtime (`cursor-agent/versions/V/node`) is the runner it ships in.
/// The file and the roots are compared as the kernel resolves them, so a
/// link from `~/.local/bin` into `/tmp` is read in `/tmp`. A copy whose
/// name is the runner, in a temp directory, a project tree or a `.local`
/// of its own making, is not: the name alone is what a tool call can
/// choose. Nor is a file that someone other than the person could have
/// written, or swapped through a directory between it and its root, or
/// an entry of a Nix store `store` that Nix does not keep
/// ([`store_keeps`]). The directories and the file are judged as they
/// were opened ([`pinned_walk`]) from `path` resolved once, and the
/// file the walk ends on must be `expect`, the (device, inode) the
/// caller holds for the file it means.
fn runner_of_file(
    path: &Path,
    home: Option<&Path>,
    store: &Path,
    person: &Person,
    expect: (u64, u64),
) -> Option<&'static str> {
    runner_at(
        &std::fs::canonicalize(path).ok()?,
        home,
        store,
        person,
        expect,
    )
}

/// [`runner_of_file`] for `file`, a path already resolved.
fn runner_at(
    file: &Path,
    home: Option<&Path>,
    store: &Path,
    person: &Person,
    expect: (u64, u64),
) -> Option<&'static str> {
    let (roots, store) = resolved_roots(home, store);
    let (runner, root) = runner_below(file, &roots, &store)?;
    kept_below(file, &root, &store, person, expect).then_some(runner)
}

/// Whether `path` is a runtime ([`RUNTIMES`]) by its exact name, and
/// an installed copy kept as a runner file is ([`runner_of_file`]):
/// below an install root, left to the person, and the file `expect`.
#[cfg(target_os = "linux")]
fn runtime_of_file(
    path: &Path,
    home: Option<&Path>,
    store: &Path,
    person: &Person,
    expect: (u64, u64),
) -> bool {
    let Ok(file) = std::fs::canonicalize(path) else {
        return false;
    };
    let named = file
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| RUNTIMES.contains(&name));
    let (roots, store) = resolved_roots(home, store);
    named
        && root_below(&file, &roots, &store)
            .is_some_and(|root| kept_below(&file, &root, &store, person, expect))
}

/// The install roots for `home` ([`install_roots`]) and the Nix store
/// `store`, resolved.
fn resolved_roots(home: Option<&Path>, store: &Path) -> (Vec<PathBuf>, PathBuf) {
    let roots = install_roots(home)
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect();
    let store = std::fs::canonicalize(store).unwrap_or_else(|_| store.to_path_buf());
    (roots, store)
}

/// Whether `file`, a resolved path below install root `root`, is the
/// file `expect` and left to the person ([`walk_kept`]) as the walk
/// reads it ([`pinned_walk`]). A file in a temp directory never is.
fn kept_below(file: &Path, root: &Path, store: &Path, person: &Person, expect: (u64, u64)) -> bool {
    if crate::bookkeep::resolved_path_is_temp(file) {
        return false;
    }
    let (Some(owners), Some(at)) = (
        pinned_walk(file, expect),
        root.components().count().checked_sub(1),
    ) else {
        return false;
    };
    walk_kept(&owners, at, root.parent() == Some(store), person)
}

/// Whether the owners [`pinned_walk`] read leave a file to the person,
/// its install root being at `at`. The directory holding the root, the
/// root, and all below it are kept by the person ([`person_keeps`]),
/// since whoever may write the root's directory may rename the root.
/// For an entry of a Nix store, the store and the entry are as Nix keeps
/// them ([`store_keeps`]) instead.
fn walk_kept(owners: &[Owner], at: usize, in_store: bool, person: &Person) -> bool {
    let (Some(&above), Some(&root)) = (
        at.checked_sub(1).and_then(|above| owners.get(above)),
        owners.get(at),
    ) else {
        return false;
    };
    let top = if in_store {
        store_keeps(above, root, person)
    } else {
        person_keeps(above, person)
    };
    top && owners[at..].iter().all(|dir| person_keeps(*dir, person))
}

/// The runner `pid` is, from the file the kernel runs it from, never
/// from the name it gives itself. An installed runtime such as `node`
/// ([`runtime_of_file`]) is the runner whose script it runs, that
/// script's path resolved on disk and judged as a runner file is. The
/// script's path comes from the process's own arguments, so only a
/// runtime the person installed is believed about it.
#[cfg(target_os = "linux")]
#[must_use]
pub fn exe_runner(pid: u32) -> Option<&'static str> {
    use std::os::unix::fs::MetadataExt as _;
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let running = std::fs::metadata(format!("/proc/{pid}/exe"))
        .ok()
        .map(|meta| (meta.dev(), meta.ino()));
    let script = || {
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let script = cmdline
            .split(|b| *b == 0)
            .skip(1)
            .map(String::from_utf8_lossy)
            .find(|a| !a.is_empty() && !a.starts_with('-'))?;
        let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
        Some(cwd.join(script.as_ref()))
    };
    let home = own_home();
    let person = Person::new(&SystemAccounts);
    runner_from(
        &exe,
        running,
        script,
        home.as_deref(),
        Path::new(NIX_STORE),
        &person,
    )
}

/// [`exe_runner`] for a process run from `exe`, whose (device, inode)
/// the kernel reports as `running`, and that names `script` as the
/// script a runtime there runs. `None` when `running` could not be
/// read, since the path alone may name another file than the one the
/// process runs.
#[cfg(target_os = "linux")]
fn runner_from(
    exe: &Path,
    running: Option<(u64, u64)>,
    script: impl FnOnce() -> Option<PathBuf>,
    home: Option<&Path>,
    store: &Path,
    person: &Person,
) -> Option<&'static str> {
    use std::os::unix::fs::MetadataExt as _;
    let running = running?;
    if let Some(r) = runner_of_file(exe, home, store, person, running) {
        return Some(r);
    }
    if !runtime_of_file(exe, home, store, person, running) {
        return None;
    }
    let script = std::fs::canonicalize(script()?).ok()?;
    let meta = std::fs::metadata(&script).ok()?;
    runner_of_file(&script, home, store, person, (meta.dev(), meta.ino()))
}

/// Off Linux there is no `/proc/<pid>/exe` to read nor a descriptor
/// walk ([`pinned_walk`]), so no process is an installed runner.
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

/// One tool call the guard's `PreToolUse` saw: the session and cgroup it
/// could not shed, the start time of that hook (a later process is part
/// of the call), and the runner that spawned the hook.
struct ToolMark {
    sid: u32,
    at: u64,
    runner_pid: u32,
    runner_start: u64,
    shell_pid: u32,
    shell_start: u64,
    cgroup: String,
}

/// The session and cgroup of this process. A child inherits both until
/// it calls `setsid` or migrates cgroup.
struct Here {
    sid: u32,
    cgroup: String,
}

fn marks_path() -> std::path::PathBuf {
    crate::runtime_dir().join("tool-calls")
}

fn broken_path() -> std::path::PathBuf {
    crate::runtime_dir().join("tool-calls.broken")
}

/// The calling process's session id.
fn current_sid() -> Result<u32, String> {
    // SAFETY: getsid(0) reads the calling process's session id and writes
    // nothing the caller owns.
    let sid = unsafe { libc::getsid(0) };
    if sid < 0 {
        return Err(
            "the session id cannot be read, so a tool call cannot be told from the runner".into(),
        );
    }
    u32::try_from(sid).map_err(|_| "the session id does not fit".into())
}

fn here_evidence() -> Result<Here, String> {
    let sid = current_sid()?;
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").map_err(|_| {
        "the cgroup cannot be read, so a tool call cannot be told from the runner".to_string()
    })?;
    if cgroup.is_empty() {
        return Err("the cgroup is empty, so a tool call cannot be told from the runner".into());
    }
    Ok(Here { sid, cgroup })
}

fn self_start() -> Result<u64, String> {
    stat(std::process::id())
        .map(|row| row.start)
        .ok_or_else(|| "this process's start time cannot be read".into())
}

/// Remember that a tool-call record could not be written. A later read
/// then fails closed: a missing record means no tool call, and a failed
/// write must not look like that.
/// A tool call may have happened and its evidence could not be taken.
/// Later reads fail closed instead of looking like a quiet runtime.
pub fn seal_unreadable_tool_call() {
    mark_broken();
}

fn mark_broken() {
    let dir = crate::runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let _ = options.open(broken_path());
}

fn tool_marks() -> Result<Vec<ToolMark>, String> {
    if broken_path().exists() {
        return Err(
            "a tool-call record could not be written, so nothing is kept as the person's".into(),
        );
    }
    let text = match std::fs::read_to_string(marks_path()) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err(
                "the tool-call record cannot be read, so nothing is kept as the person's".into(),
            );
        }
    };
    let mut marks = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 8 || fields[0] != "1" {
            return Err(
                "the tool-call record is corrupt, so nothing is kept as the person's".into(),
            );
        }
        let number = |index: usize, label: &str| -> Result<u64, String> {
            fields[index].parse().map_err(|_| {
                format!(
                    "the tool-call record has a bad {label}, so nothing is kept as the person's"
                )
            })
        };
        let cgroup = hex::decode(fields[7])
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or("the tool-call record has a bad cgroup, so nothing is kept as the person's")?;
        let narrow = |value: u64, label: &str| -> Result<u32, String> {
            u32::try_from(value).map_err(|_| {
                format!(
                    "the tool-call record has a bad {label}, so nothing is kept as the person's"
                )
            })
        };
        marks.push(ToolMark {
            sid: narrow(number(1, "session")?, "session")?,
            at: number(2, "start")?,
            runner_pid: narrow(number(3, "runner")?, "runner")?,
            runner_start: number(4, "runner start")?,
            shell_pid: narrow(number(5, "shell")?, "shell")?,
            shell_start: number(6, "shell start")?,
            cgroup,
        });
    }
    Ok(marks)
}

/// Record the tool call whose guard hook this is. The runner is `runner`,
/// the process that spawned the hook. The hook's own start time is the
/// watermark: a process started later, in this session or cgroup, is the
/// tool call even after it detaches.
///
/// # Errors
///
/// The session, cgroup or start time cannot be read, or the record cannot
/// be written. A failed write leaves a sentinel so later reads fail closed.
pub fn note_tool_call(runner: &Proc) -> Result<(), String> {
    // A tool call whose session, cgroup or start time cannot be read must
    // not look like a runtime that has seen no tool call.
    let here = match here_evidence() {
        Ok(here) => here,
        Err(why) => {
            mark_broken();
            return Err(why);
        }
    };
    let at = match self_start() {
        Ok(at) => at,
        Err(why) => {
            mark_broken();
            return Err(why);
        }
    };
    let dir = crate::runtime_dir();
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    if builder.create(&dir).is_err() {
        mark_broken();
        return Err("the tool-call record directory cannot be created".into());
    }
    // sid, watermark, runner pid and start, tool shell pid and start
    // (unset when the shell does not exist yet), cgroup text as hex.
    // One line stays under PIPE_BUF so the append is one write.
    let line = format!(
        "1\t{}\t{at}\t{}\t{}\t0\t0\t{}\n",
        here.sid,
        runner.pid,
        runner.start,
        hex::encode(here.cgroup.as_bytes())
    );
    if line.len() > 4096 {
        mark_broken();
        return Err("the tool-call record does not fit one write".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let opened = options.open(marks_path());
    let mut file = match opened {
        Ok(file) => file,
        Err(_) => {
            mark_broken();
            return Err("the tool-call record cannot be written".into());
        }
    };
    use std::io::Write as _;
    if file.write_all(line.as_bytes()).is_err() {
        mark_broken();
        return Err("the tool-call record cannot be written".into());
    }
    Ok(())
}

/// Whether `runner` is the runner a person started, rather than a process
/// a recorded tool call still owns. The same runner process that recorded
/// a mark is that opener. Any other process that shares the mark's cgroup
/// or session and is not older than the mark is the tool call. No marks
/// means the person has not made a tool call in this runtime yet.
fn confirm_runner(runner: &Proc) -> Result<(), String> {
    let here = here_evidence()?;
    let marks = tool_marks()?;
    let chain = ancestors();
    for mark in &marks {
        if mark.runner_pid == runner.pid && mark.runner_start == runner.start {
            continue;
        }
        if mark.shell_pid > 0
            && chain
                .iter()
                .any(|proc| proc.pid == mark.shell_pid && proc.start == mark.shell_start)
        {
            return Err(format!(
                "{} (pid {}) descends from a recorded tool call",
                runner.comm, runner.pid
            ));
        }
        let same_place = mark.sid == here.sid || mark.cgroup == here.cgroup;
        if same_place && runner.start >= mark.at {
            return Err(format!(
                "{} (pid {}) shares a recorded tool call's cgroup or session and is not older than that tool call",
                runner.comm, runner.pid
            ));
        }
    }
    Ok(())
}

/// The installed runner above this process, checked against the tool-call
/// record ([`confirm_runner`]). Shells and wrappers are stepped over. The
/// first other program that is not an installed runner ends the walk, so
/// a copy named like a runner cannot hide behind a real one further up.
///
/// # Errors
///
/// No installed runner is above this process, or the one that is still
/// belongs to a recorded tool call.
pub fn placed_runner() -> Result<(), String> {
    for proc in ancestors() {
        if is_shell(&proc) || is_wrapper(&proc) {
            continue;
        }
        if ends_the_walk(&proc) {
            break;
        }
        if exe_runner(proc.pid).is_some() {
            return confirm_runner(&proc);
        }
        return Err(format!(
            "{} (pid {}) is not an installed runner, so this is not the person's runner",
            proc.comm, proc.pid
        ));
    }
    Err("no installed runner is above this process, so this is not the person's runner".into())
}

/// The runner that spawned this `ljos hook` itself, or why none did: the
/// parent must be an installed runner ([`exe_runner`]), with no shell or
/// interpreter between them but the `sh -c` the runner runs its hook
/// command in ([`hook_shell`]). That runner must not run inside a tool
/// call, which a runner above it, a tool call's variables in the
/// environment it was started with, or the guard's tool-call record
/// ([`confirm_runner`]) would show. Only such a hook's prompt is the
/// person's. An environment that cannot be read hides the stamp, so it
/// is refused.
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
    let env = environ(parent.pid).ok_or_else(|| {
        format!(
            "the environment {} (pid {}) was started with cannot be read, so a tool call's stamp would be hidden",
            parent.comm, parent.pid
        )
    })?;
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
    confirm_runner(parent)?;
    Ok((parent.clone(), runner))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`runner_of_file`] for the file `path` names now.
    #[cfg(target_os = "linux")]
    fn runner_here(
        path: &Path,
        home: Option<&Path>,
        store: &Path,
        person: &Person,
    ) -> Option<&'static str> {
        use std::os::unix::fs::MetadataExt as _;
        let meta = std::fs::metadata(path).ok()?;
        runner_of_file(path, home, store, person, (meta.dev(), meta.ino()))
    }

    #[test]
    fn a_runner_is_read_from_the_file_it_runs_from() {
        let roots = install_roots(Some(Path::new("/home/u")));
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
            ("/usr/local/bin/codex", Some("codex")),
            ("/usr/bin/claude", Some("claude")),
            ("/bin/grok", Some("grok")),
            ("/home/u/.cargo/bin/codex", Some("codex")),
            (
                "/home/u/.npm-global/lib/node_modules/@anthropic-ai/claude-code/cli.js",
                Some("claude"),
            ),
            (
                "/nix/store/0c9yapqd0b8y4mj5xj7mrpmhy3l5ykh8-claude-code-2.0.1/bin/claude",
                Some("claude"),
            ),
            ("/usr/bin/node", None),
            ("/usr/bin/bash", None),
            ("/usr/bin/python3", None),
            ("/usr/bin/ssh", None),
            ("/home/u/.cargo/bin/ljos", None),
            ("/tmp/claude", None),
            ("/tmp/claude/versions/2.1.3", None),
            ("/home/u/runners/claude", None),
            ("/home/u/plant/grok", None),
            ("/home/u/plant/bin/claude", None),
            ("/home/u/.local/bin/claude (deleted)", None),
            ("/home/u/plant/.local/bin/claude", None),
            ("/tmp/.local/bin/claude", None),
            ("/tmp/project/node_modules/.bin/claude", None),
            ("/home/u/../other/.local/bin/claude", None),
            ("/home/uu/.local/bin/claude", None),
            ("/home/other/.local/bin/claude", None),
            ("/home/u/.local/binx/claude", None),
            ("/home/u/.local/bin/../../plant/claude", None),
            ("/home/u/.local/share/plant/claude", None),
            ("/home/u/.local/share/claude-plant/claude", None),
            ("/home/u/src/app/node_modules/.bin/claude", None),
            ("/usr/binx/claude", None),
            ("/usr/share/plant/claude", None),
            ("/usr/lib/node_modules/../../../tmp/claude", None),
            ("/opt/plant/claude", None),
            ("/var/tmp/.local/bin/claude", None),
            ("/dev/shm/node_modules/.bin/claude", None),
            ("/nix/store/claude/bin/claude", None),
            (
                "/nix/store/0c9yapqd0b8y4mj5xj7mrpmhy3l5ykh8/bin/claude",
                None,
            ),
            (
                "/nix/store/0c9yapqd0b8y4mj5xj7mrpmhy3l5ykhe-claude-code-2.0.1/bin/claude",
                None,
            ),
        ] {
            let got = runner_below(Path::new(path), &roots, Path::new(NIX_STORE))
                .map(|(runner, _)| runner);
            assert_eq!(got, want, "{path}");
        }
    }

    /// A runner is read where the kernel resolves it. A link from the
    /// person's own `~/.local/bin` into `/tmp`, a path that climbs out of
    /// that home into another, and a file anyone may rewrite or reach
    /// through a directory anyone may write are not installed copies; the
    /// same file kept by the person is.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_install_root_holds_only_what_resolves_inside_it() {
        use std::os::unix::fs::PermissionsExt as _;
        let base = tempfile::Builder::new()
            .prefix(".ljos-roots-")
            .tempdir_in(own_home().expect("this user's passwd entry names a home"))
            .unwrap();
        let home = base.path().join("u");
        let person = Person::new(&SystemAccounts);
        let store = PathBuf::from(NIX_STORE);
        let program = |dir: &Path, name: &str| {
            std::fs::create_dir_all(dir).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, "").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let bin = home.join(".local/bin");
        let claude = program(&bin, "claude");
        assert_eq!(
            runner_here(&claude, Some(&home), &store, &person),
            Some("claude")
        );

        let away = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(program(away.path(), "codex"), bin.join("codex")).unwrap();
        assert_eq!(
            runner_here(&bin.join("codex"), Some(&home), &store, &person),
            None
        );

        program(&base.path().join("other/.local/bin"), "claude");
        let climbed = home.join("../other/.local/bin/claude");
        assert_eq!(runner_here(&climbed, Some(&home), &store, &person), None);

        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o757)).unwrap();
        assert_eq!(runner_here(&claude, Some(&home), &store, &person), None);

        let open = bin.join("open");
        let grok = program(&open, "grok");
        assert_eq!(
            runner_here(&grok, Some(&home), &store, &person),
            Some("grok")
        );
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(runner_here(&grok, Some(&home), &store, &person), None);
    }

    /// Whoever may write the directory an install root sits in may swap
    /// the root, so that directory is judged as the root is: each root
    /// under the person's home is refused once its own directory is
    /// writable by anyone or by a group the person shares.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_root_in_a_directory_others_may_write_is_refused() {
        use std::os::unix::fs::PermissionsExt as _;
        let base = tempfile::Builder::new()
            .prefix(".ljos-above-")
            .tempdir_in(own_home().expect("this user's passwd entry names a home"))
            .unwrap();
        let home = base.path().join("u");
        let store = PathBuf::from(NIX_STORE);
        let person = Person::new(&SystemAccounts);
        let staff = FakeAccounts {
            user: Some("u"),
            group: Some("staff"),
            members: &["u", "v"],
            primary: Some(vec![person.uid]),
        };
        let sharing = Person::new(&staff);
        let mut files: Vec<PathBuf> = vec![
            home.join(".local/bin/claude"),
            home.join(".cargo/bin/codex"),
            home.join(".npm-global/lib/node_modules/@anthropic-ai/claude-code/cli.js"),
        ];
        files.extend(
            HOOK_RUNNERS
                .iter()
                .map(|(name, _)| home.join(".local/share").join(name).join(name)),
        );
        for file in &files {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "").unwrap();
        }
        let set = |dir: &Path, mode| {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        for root in install_roots(Some(&home))
            .iter()
            .filter(|r| r.starts_with(&home))
        {
            let file = files.iter().find(|f| f.starts_with(root)).unwrap();
            let above = root.parent().unwrap();
            let runner = |who| runner_here(file, Some(&home), &store, who);
            assert!(runner(&person).is_some(), "{}", root.display());
            set(above, 0o757);
            assert_eq!(runner(&person), None, "{}", root.display());
            set(above, 0o775);
            assert_eq!(runner(&sharing), None, "{}", root.display());
            set(above, 0o755);
        }

        let uid = person.uid;
        let owner = |uid, mode| Owner {
            uid,
            gid: person.gid,
            mode,
        };
        let walk = |above| [owner(0, 0o755), above, owner(uid, 0o755), owner(uid, 0o755)];
        assert!(walk_kept(&walk(owner(uid, 0o755)), 2, false, &person));
        assert!(walk_kept(&walk(owner(0, 0o755)), 2, false, &person));
        assert!(!walk_kept(&walk(owner(uid + 1, 0o755)), 2, false, &person));
        assert!(!walk_kept(&walk(owner(uid, 0o757)), 2, false, &person));
        assert!(!walk_kept(&walk(owner(uid, 0o775)), 2, false, &sharing));
    }

    /// The file judged is the one opened. A directory swapped after the
    /// path resolved, for a link or for another directory holding a file
    /// of the same name, is not walked through, and a file that is not
    /// the one the process runs is not its runner.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_parent_swapped_after_the_path_resolved_is_refused() {
        use std::os::unix::fs::MetadataExt as _;
        let base = tempfile::Builder::new()
            .prefix(".ljos-swap-")
            .tempdir_in(own_home().expect("this user's passwd entry names a home"))
            .unwrap();
        let home = std::fs::canonicalize(base.path()).unwrap().join("u");
        let person = Person::new(&SystemAccounts);
        let store = PathBuf::from(NIX_STORE);
        let versions = home.join(".local/share/claude/versions");
        let other = home.join("other");
        for dir in [&versions, &other] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("2.1.3"), "").unwrap();
        }
        let id = |path: &Path| {
            let meta = std::fs::metadata(path).unwrap();
            (meta.dev(), meta.ino())
        };
        let file = versions.join("2.1.3");
        let (running, planted) = (id(&file), id(&other.join("2.1.3")));
        let runner = |expect| runner_at(&file, Some(&home), &store, &person, expect);
        assert_eq!(runner(running), Some("claude"));

        std::fs::rename(&versions, home.join("held")).unwrap();
        std::os::unix::fs::symlink(&other, &versions).unwrap();
        assert_eq!(runner(running), None);
        assert_eq!(runner(planted), None);

        std::fs::remove_file(&versions).unwrap();
        std::fs::rename(&other, &versions).unwrap();
        assert_eq!(runner(running), None);
    }

    /// A process is its runner only through the (device, inode) the
    /// kernel reports for its executable. When that cannot be read, the
    /// path the link names is not trusted in its place.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_executable_that_cannot_be_read_is_no_runner() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let base = tempfile::Builder::new()
            .prefix(".ljos-exe-")
            .tempdir_in(own_home().expect("this user's passwd entry names a home"))
            .unwrap();
        let home = base.path().join("u");
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        std::fs::write(&claude, "").unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        let meta = std::fs::metadata(&claude).unwrap();
        let person = Person::new(&SystemAccounts);
        let store = PathBuf::from(NIX_STORE);
        let runner = |running| runner_from(&claude, running, || None, Some(&home), &store, &person);
        assert_eq!(runner(Some((meta.dev(), meta.ino()))), Some("claude"));
        assert_eq!(runner(None), None);
    }

    /// A runtime's script names the runner only when the runtime is an
    /// installed copy of its exact name, kept as a runner file is and the
    /// file the process runs. A `node` of a tool call's own making, or a
    /// program whose name merely starts like one, may name any script.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_script_names_the_runner_only_for_an_installed_runtime() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let base = tempfile::Builder::new()
            .prefix(".ljos-runtime-")
            .tempdir_in(own_home().expect("this user's passwd entry names a home"))
            .unwrap();
        let home = base.path().join("u");
        let away = tempfile::tempdir().unwrap();
        let program = |dir: &Path, name: &str| {
            std::fs::create_dir_all(dir).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, "").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let id = |path: &Path| {
            let meta = std::fs::metadata(path).unwrap();
            (meta.dev(), meta.ino())
        };
        let cli = program(
            &home.join(".npm-global/lib/node_modules/@anthropic-ai/claude-code"),
            "cli.js",
        );
        let bin = home.join(".local/bin");
        let node = program(&bin, "node");
        let person = Person::new(&SystemAccounts);
        let store = PathBuf::from(NIX_STORE);
        let runner = |exe: &Path, running| {
            runner_from(
                exe,
                Some(running),
                || Some(cli.clone()),
                Some(&home),
                &store,
                &person,
            )
        };
        assert_eq!(runner(&node, id(&node)), Some("claude"));
        assert_eq!(runner(&node, id(&cli)), None);
        for planted in [
            program(&bin, "node-v20"),
            program(&bin, "nodejs_modules"),
            program(away.path(), "node"),
        ] {
            assert_eq!(
                runner(&planted, id(&planted)),
                None,
                "{}",
                planted.display()
            );
        }
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o757)).unwrap();
        assert_eq!(runner(&node, id(&node)), None);
    }

    /// A Nix store entry is a runner only while Nix alone keeps it. A
    /// store the person owns counts as a single-user install only while
    /// no one else may write it; a root's store must be closed to the
    /// person, sticky when its group may write it, and hold root's
    /// entries.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_nix_entry_is_a_runner_only_in_a_store_nix_keeps() {
        use std::os::unix::fs::PermissionsExt as _;
        let base = tempfile::Builder::new()
            .prefix(".ljos-store-")
            .tempdir_in(own_home().expect("this user's passwd entry names a home"))
            .unwrap();
        let store = base.path().join("store");
        let bin = store.join("0c9yapqd0b8y4mj5xj7mrpmhy3l5ykh8-claude-code-2.0.1/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        std::fs::write(&claude, "").unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        let person = Person::new(&SystemAccounts);
        let set = |mode| {
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        let runner = || runner_here(&claude, None, &store, &person);

        set(0o755);
        assert_eq!(runner(), Some("claude"));
        for mode in [0o775, 0o757, 0o777, 0o1775, 0o1777] {
            set(mode);
            assert_eq!(runner(), None, "{mode:o}");
        }
        set(0o755);

        let uid = person.uid;
        let nixbld = person.gid + 1;
        let owner = |uid, gid, mode| Owner { uid, gid, mode };
        let outsider = Person {
            groups: Some(vec![]),
            ..Person::new(&SystemAccounts)
        };
        let builder = Person {
            groups: Some(vec![nixbld]),
            ..Person::new(&SystemAccounts)
        };
        let unread = Person {
            groups: None,
            ..Person::new(&SystemAccounts)
        };
        let root_entry = owner(0, 0, 0o555);
        let own_entry = owner(uid, uid, 0o555);
        for (store, entry, who, want) in [
            (owner(0, nixbld, 0o1775), root_entry, &outsider, true),
            (owner(0, 0, 0o755), root_entry, &outsider, true),
            (owner(0, nixbld, 0o1775), own_entry, &outsider, false),
            (owner(0, nixbld, 0o775), root_entry, &outsider, false),
            (owner(0, nixbld, 0o1775), root_entry, &builder, false),
            (owner(0, nixbld, 0o1775), root_entry, &unread, false),
            (owner(0, 0, 0o1777), root_entry, &outsider, false),
            (owner(uid, uid, 0o755), own_entry, &outsider, true),
            (owner(uid, uid, 0o755), root_entry, &outsider, true),
            (owner(uid, uid, 0o775), own_entry, &outsider, false),
            (owner(uid, uid, 0o1777), own_entry, &outsider, false),
            (
                owner(uid, uid, 0o755),
                owner(uid + 1, 0, 0o555),
                &outsider,
                false,
            ),
            (owner(uid + 1, 0, 0o755), own_entry, &outsider, false),
        ] {
            assert_eq!(
                store_keeps(store, entry, who),
                want,
                "{}:{} {:o}",
                store.uid,
                store.gid,
                store.mode
            );
        }
    }

    /// One user `u`, the name and members of whatever group is asked
    /// for, and the uids whose primary group it is.
    #[cfg(target_os = "linux")]
    struct FakeAccounts {
        user: Option<&'static str>,
        group: Option<&'static str>,
        members: &'static [&'static str],
        primary: Option<Vec<u32>>,
    }

    #[cfg(target_os = "linux")]
    impl Accounts for FakeAccounts {
        fn user_name(&self, _uid: u32) -> Option<std::ffi::OsString> {
            self.user.map(Into::into)
        }

        fn group(&self, _gid: u32) -> Option<GroupEntry> {
            self.group.map(|name| GroupEntry {
                name: name.into(),
                members: self.members.iter().map(Into::into).collect(),
            })
        }

        fn primary_members(&self, _gid: u32) -> Option<Vec<u32>> {
            self.primary.clone()
        }
    }

    /// A runner file the person's primary group may write is theirs only
    /// when that group is: named for them, listing no one else, and no
    /// other account's primary group. An account lookup that fails is
    /// not such a group.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_group_write_counts_only_from_a_group_the_person_has_alone() {
        use std::os::unix::fs::PermissionsExt as _;
        // SAFETY: getuid takes no arguments and only reads the process
        // identity.
        let uid = unsafe { libc::getuid() };
        let alone = |members, primary| FakeAccounts {
            user: Some("u"),
            group: Some("u"),
            members,
            primary: Some(primary),
        };
        let shared = [
            FakeAccounts {
                group: Some("staff"),
                ..alone(&[], vec![uid])
            },
            FakeAccounts {
                group: None,
                ..alone(&[], vec![uid])
            },
            alone(&["u", "v"], vec![uid]),
            alone(&["v"], vec![uid]),
            alone(&[], vec![uid, uid + 1]),
            FakeAccounts {
                user: None,
                ..alone(&[], vec![uid])
            },
            FakeAccounts {
                primary: None,
                ..alone(&[], vec![uid])
            },
        ];

        let base = tempfile::Builder::new()
            .prefix(".ljos-group-")
            .tempdir_in(own_home().expect("this user's passwd entry names a home"))
            .unwrap();
        let home = base.path().join("u");
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        std::fs::write(&claude, "").unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o775)).unwrap();
        let store = PathBuf::from(NIX_STORE);

        for accounts in [alone(&[], vec![uid]), alone(&["u"], vec![uid, uid])] {
            assert!(group_is_private(&accounts, uid, 0));
            let person = Person::new(&accounts);
            assert_eq!(
                runner_here(&claude, Some(&home), &store, &person),
                Some("claude")
            );
        }
        for accounts in &shared {
            assert!(!group_is_private(accounts, uid, 0));
            let person = Person::new(accounts);
            assert_eq!(runner_here(&claude, Some(&home), &store, &person), None);
        }
    }

    /// The accounts in a group are read from `passwd` only when
    /// `nsswitch.conf` names that file alone, and a file that cannot be
    /// read whole lists no one rather than fewer.
    #[test]
    fn a_primary_group_is_listed_only_from_a_passwd_file_read_whole() {
        let etc = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| std::fs::write(etc.path().join(name), text).unwrap();
        let passwd = "root:x:0:0::/root:/bin/sh\nu:x:1000:1000::/home/u:/bin/sh\n";
        write("passwd", passwd);
        assert_eq!(primary_in_files(etc.path(), 1000), None, "no nsswitch.conf");
        for conf in [
            "passwd: files systemd\n",
            "passwd: sss files\n",
            "passwd: compat\n",
            "passwd: files [NOTFOUND=return] ldap\n",
            "passwd: files\npasswd: files ldap\n",
            "group: files\n",
        ] {
            write("nsswitch.conf", conf);
            assert_eq!(primary_in_files(etc.path(), 1000), None, "{conf}");
        }
        write(
            "nsswitch.conf",
            "# local\npasswd:   files # only\ngroup: files\n",
        );
        assert_eq!(primary_in_files(etc.path(), 1000), Some(vec![1000]));
        assert_eq!(primary_in_files(etc.path(), 7), Some(vec![]));
        for extra in [
            "+@admins\n",
            "-v\n",
            "v:x:1001\n",
            "v:x:one:1000::/home/v:/bin/sh\n",
        ] {
            write("passwd", &format!("{passwd}{extra}"));
            assert_eq!(primary_in_files(etc.path(), 1000), None, "{extra}");
        }
        write(
            "passwd",
            &format!("{passwd}\n# kept\nv:x:1001:1000::/home/v:/bin/sh\n"),
        );
        assert_eq!(primary_in_files(etc.path(), 1000), Some(vec![1000, 1001]));
        std::fs::remove_file(etc.path().join("passwd")).unwrap();
        assert_eq!(primary_in_files(etc.path(), 1000), None);
    }

    /// A group is read from `group` only when `nsswitch.conf` names that
    /// file alone, and only when one line has its gid: a second line's
    /// members hold the group as well, though a lookup returns the first.
    #[test]
    fn a_group_is_read_only_from_one_line_of_a_group_file_read_whole() {
        let etc = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| std::fs::write(etc.path().join(name), text).unwrap();
        let group = "root:x:0:\nu:x:1000:\nstaff:x:50:u,v\n";
        write("group", group);
        let read = |gid| {
            group_in_files(etc.path(), gid).map(|g| {
                let members: Vec<String> = g
                    .members
                    .iter()
                    .map(|m| m.to_string_lossy().into_owned())
                    .collect();
                (g.name.to_string_lossy().into_owned(), members)
            })
        };
        assert_eq!(read(1000), None, "no nsswitch.conf");
        for conf in ["group: files systemd\n", "group: sss\n", "passwd: files\n"] {
            write("nsswitch.conf", conf);
            assert_eq!(read(1000), None, "{conf}");
        }
        write("nsswitch.conf", "passwd: files\ngroup: files\n");
        assert_eq!(read(1000), Some(("u".into(), vec![])));
        assert_eq!(
            read(50),
            Some(("staff".into(), vec!["u".into(), "v".into()]))
        );
        assert_eq!(read(7), None);
        for extra in ["shadow:x:1000:v\n", "+:::\n", "w:x:1000\n", "w:x:ten:\n"] {
            write("group", &format!("{group}{extra}"));
            assert_eq!(read(1000), None, "{extra}");
        }
    }

    /// A process younger than a recorded tool call, in that call's cgroup
    /// or session, is still the tool call. The runner that recorded the
    /// mark is not. A missing record is not a tool call; a corrupt one
    /// refuses everything.
    #[test]
    fn a_young_runner_in_a_recorded_tool_call_is_refused() {
        let _guard = crate::test_env_lock();
        let opener = Proc {
            pid: 1,
            start: 1,
            comm: "claude".into(),
        };
        let young = Proc {
            pid: 999_999,
            start: u64::MAX,
            comm: "claude".into(),
        };
        let old = Proc {
            pid: 42,
            start: 0,
            comm: "claude".into(),
        };
        assert!(confirm_runner(&young).is_ok(), "no record yet");
        note_tool_call(&opener).unwrap();
        let why = confirm_runner(&young).unwrap_err();
        assert!(why.contains("tool call"), "{why}");
        assert!(confirm_runner(&opener).is_ok(), "the opener recorded it");
        assert!(confirm_runner(&old).is_ok(), "older than the record");
        std::fs::write(marks_path(), "nope\n").unwrap();
        let why = confirm_runner(&old).unwrap_err();
        assert!(why.contains("corrupt"), "{why}");
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
