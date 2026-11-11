//! The only speaker that may carry this seat's packsetd token.
//!
//! The token stays in this module. Each request is one byte stream to the
//! socket address resolved for the writer's URL, with no proxy and no
//! redirect. The proof runs before the token is written on that stream. A
//! stream that closes is not remembered: the next call proves again before
//! the token goes out. packsetd answers one request and closes, so the two
//! refusals and the call that carries the token are three sockets in a row;
//! the proof is not kept for a later call.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use packset_client::{Error, Hit};
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

/// The seat's pack writer. The token field is private to this module.
pub struct PacksetClient {
    base: String,
    origin: Url,
    addr: Option<SocketAddr>,
    workspace: Option<String>,
    token: Mutex<Option<String>>,
}

impl Clone for PacksetClient {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            origin: self.origin.clone(),
            addr: self.addr,
            workspace: self.workspace.clone(),
            token: Mutex::new(self.token.lock().unwrap_or_else(|p| p.into_inner()).clone()),
        }
    }
}

/// One answer. `trusted` means the token was on this answer, after the
/// proof held.
#[derive(Debug, Clone)]
pub(crate) struct Answer {
    pub status: u16,
    pub body: String,
    pub trusted: bool,
    /// `Retry-After` when the writer sent one, in whole seconds.
    pub retry_after: Option<Duration>,
}

/// Answers a hook process already has for a search or a status read, by
/// request, for [`ANSWER_MEMO`]. A prompt hook asks the same search
/// twice and the status twice; the second asks are answered here. Only a
/// hook keeps one, and any write through the client drops it.
type KeptAnswer = (String, Instant, Answer);
static KEPT_ANSWERS: Mutex<Vec<KeptAnswer>> = Mutex::new(Vec::new());
const ANSWER_MEMO: Duration = Duration::from_secs(2);

fn answer_memo_key(
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    bearer: bool,
) -> Option<String> {
    if method != "GET" || !matches!(path, "/v1/search" | "/v1/status") {
        return None;
    }
    std::env::var_os("LJOS_IN_HOOK")?;
    let mut key = format!("{path}|{bearer}");
    for (k, v) in query {
        key.push('|');
        key.push_str(k);
        key.push('=');
        key.push_str(v);
    }
    Some(key)
}

fn kept_answer(key: &str) -> Option<Answer> {
    let kept = KEPT_ANSWERS.lock().unwrap_or_else(|p| p.into_inner());
    kept.iter()
        .find(|(k, at, _)| k == key && at.elapsed() < ANSWER_MEMO)
        .map(|(_, _, answer)| answer.clone())
}

fn keep_answer(key: String, answer: &Answer) {
    let mut kept = KEPT_ANSWERS.lock().unwrap_or_else(|p| p.into_inner());
    kept.retain(|(k, _, _)| k != &key);
    kept.push((key, Instant::now(), answer.clone()));
}

/// Drop every kept answer. Called on any write through the client.
pub(crate) fn forget_kept_answers() {
    KEPT_ANSWERS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
}

struct Slot {
    stream: Option<TcpStream>,
    /// The proof held on `stream` and the stream is still the one it held on.
    proven: bool,
    token_tag: u64,
    /// The last answer on this socket carried the token.
    carried: bool,
}

fn slots() -> &'static Mutex<HashMap<SocketAddr, Arc<Mutex<Slot>>>> {
    static SLOTS: OnceLock<Mutex<HashMap<SocketAddr, Arc<Mutex<Slot>>>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn slot_for(addr: SocketAddr) -> Arc<Mutex<Slot>> {
    let mut map = slots().lock().unwrap_or_else(|p| p.into_inner());
    map.entry(addr)
        .or_insert_with(|| {
            Arc::new(Mutex::new(Slot {
                stream: None,
                proven: false,
                token_tag: 0,
                carried: false,
            }))
        })
        .clone()
}

fn load_token() -> Option<String> {
    packset_client::token()
}

fn token_tag(token: Option<&str>) -> u64 {
    let Some(token) = token else {
        return 0;
    };
    let mut hash = 0xcbf29ce484222325u64;
    for byte in token.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn nonce() -> Option<String> {
    use std::io::Read as _;
    let mut raw = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut raw))
        .ok()
        .map(|()| hex::encode(raw))
}

fn pin(url: &Url) -> Option<SocketAddr> {
    let host = url.host_str()?;
    let port = url.port_or_known_default()?;
    if !crate::tool_bound::armed() {
        return (host, port).to_socket_addrs().ok()?.next();
    }
    let left = crate::wall_left().unwrap_or(Duration::from_millis(1));
    if left.is_zero() {
        crate::tool_bound::note("packsetd");
        crate::tool_bound::mark_failed("packsetd timed out");
        return None;
    }
    let host = host.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let found = (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next());
        let _ = tx.send(found);
    });
    match rx.recv_timeout(left) {
        Ok(addr) => addr,
        Err(_) => {
            crate::tool_bound::note("packsetd");
            crate::tool_bound::mark_failed("packsetd timed out");
            None
        }
    }
}

fn host_header(url: &Url) -> String {
    match (url.host_str(), url.port()) {
        (Some(host), Some(port)) if host.contains(':') => format!("[{host}]:{port}"),
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        _ => "localhost".to_string(),
    }
}

fn budget() -> Duration {
    let base = crate::pack_budget_ms()
        .or_else(|| {
            std::env::var("PACKSET_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|ms| *ms > 0)
        })
        .map_or(Duration::from_secs(30), Duration::from_millis);
    match crate::wall_left() {
        Some(left) => base.min(left).max(Duration::from_millis(1)),
        None => base,
    }
}

struct Clock {
    deadline: Instant,
    depth: u32,
}

thread_local! {
    /// This thread's pack budget. A nested call keeps the earlier deadline.
    /// Another thread has its own: one search must not spend a read that
    /// is running beside it.
    static CLOCK: std::cell::RefCell<Option<Clock>> = const { std::cell::RefCell::new(None) };
}

/// Bound every pack socket call on this thread until this guard drops.
/// A nested guard on the same thread keeps the earlier deadline, so a
/// hook stays under 2s and an interactive command under 5s across every
/// socket of the call. A call on another thread keeps the budget it armed.
pub struct PackClock {
    /// Drop has to run on the thread that entered, where the clock lives.
    _thread: std::marker::PhantomData<*const ()>,
}

impl PackClock {
    #[must_use]
    pub fn enter(total: Duration) -> Self {
        CLOCK.with(|slot| {
            let mut slot = slot.borrow_mut();
            match slot.as_mut() {
                Some(clock) => clock.depth = clock.depth.saturating_add(1),
                None => {
                    *slot = Some(Clock {
                        deadline: Instant::now() + total,
                        depth: 1,
                    });
                }
            }
        });
        Self {
            _thread: std::marker::PhantomData,
        }
    }
}

impl Drop for PackClock {
    fn drop(&mut self) {
        CLOCK.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some(clock) = slot.as_mut() {
                clock.depth = clock.depth.saturating_sub(1);
                if clock.depth == 0 {
                    *slot = None;
                }
            }
        });
    }
}

fn clock_deadline() -> Option<Instant> {
    CLOCK.with(|slot| slot.borrow().as_ref().map(|clock| clock.deadline))
}

pub(crate) fn clock_armed() -> bool {
    clock_deadline().is_some()
}

/// Time left on the armed pack clock. `None` when no clock is armed.
pub(crate) fn clock_left() -> Option<Duration> {
    clock_deadline().map(|deadline| deadline.saturating_duration_since(Instant::now()))
}

/// A search or island read in this clock already went quiet, or the clock
/// is spent. Later reads must not wait it out again.
pub(crate) fn search_stalled() -> bool {
    pack_quiet() || clock_deadline().is_some_and(|deadline| Instant::now() >= deadline)
}

pub(crate) fn error_is_timeout(err: &Error) -> bool {
    match err {
        Error::Io(io) => {
            io_is_quiet(io)
                || io.to_string().contains("the pack did not answer")
                || io.to_string().contains("packsetd timed out")
        }
        _ => false,
    }
}

/// Whether every socket listening on the loopback port of `url` belongs to
/// another user. Unknown is not foreign.
///
/// A hook keeps its first answer for each url. The socket tables are read
/// in full each time, a hook connects more than once, and it ends within
/// the runner's cut-off. Elsewhere a search keeps its first answer until
/// it ends ([`OwnerOnce`]), and any other call reads them again. A writer
/// that stops leaves its port to whoever binds it next. A long-lived
/// process that kept its first answer would go on carrying the token to
/// another user's listener, or never to its own.
fn foreign(url: &str) -> bool {
    if std::env::var_os("LJOS_IN_HOOK").is_none() {
        return SEARCH_OWNERS.with(|kept| match kept.borrow_mut().as_mut() {
            Some(kept) => match kept.iter().find(|(u, _)| u == url) {
                Some((_, known)) => *known,
                None => {
                    let answer = foreign_read(url);
                    kept.push((url.to_string(), answer));
                    answer
                }
            },
            None => foreign_read(url),
        });
    }
    static KNOWN: std::sync::LazyLock<Mutex<Vec<(String, bool)>>> =
        std::sync::LazyLock::new(|| Mutex::new(Vec::new()));
    if let Some((_, known)) = KNOWN
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|(u, _)| u == url)
    {
        return *known;
    }
    let answer = foreign_read(url);
    KNOWN
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push((url.to_string(), answer));
    answer
}

thread_local! {
    /// [`foreign`]'s answers for the search on this thread, by url, for
    /// as long as an [`OwnerOnce`] lasts.
    static SEARCH_OWNERS: std::cell::RefCell<Option<Vec<(String, bool)>>> =
        const { std::cell::RefCell::new(None) };
}

/// Read the pack port's owner once for the requests of one search on this
/// thread, until the guard drops. Without it, each request of the search
/// that carries the token reads both tables. A guard inside another keeps
/// the outer one's answers.
pub(crate) struct OwnerOnce {
    outer: bool,
}

impl OwnerOnce {
    #[must_use]
    pub(crate) fn enter() -> Self {
        let outer = SEARCH_OWNERS.with(|kept| {
            let mut kept = kept.borrow_mut();
            let outer = kept.is_none();
            if outer {
                *kept = Some(Vec::new());
            }
            outer
        });
        Self { outer }
    }
}

impl Drop for OwnerOnce {
    fn drop(&mut self) {
        if self.outer {
            SEARCH_OWNERS.with(|kept| *kept.borrow_mut() = None);
        }
    }
}

#[cfg(test)]
thread_local! {
    /// How many times this thread read the socket tables.
    static TABLE_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn foreign_read(url: &str) -> bool {
    #[cfg(test)]
    TABLE_READS.with(|reads| reads.set(reads.get() + 1));
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let Some(port) = loopback_port(url) else {
            return false;
        };
        let Ok(me) = std::fs::metadata("/proc/self").map(|m| m.uid()) else {
            return false;
        };
        let owners: Vec<u32> = ["/proc/net/tcp", "/proc/net/tcp6"]
            .iter()
            .filter_map(|table| std::fs::read_to_string(table).ok())
            .flat_map(|text| listen_uids(&text, port))
            .collect();
        !owners.is_empty() && owners.iter().all(|uid| *uid != me)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = url;
        false
    }
}

#[cfg(target_os = "linux")]
fn loopback_port(url: &str) -> Option<u16> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, port.parse().ok()?),
        _ => (authority, 80),
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1]").then_some(port)
}

#[cfg(target_os = "linux")]
fn listen_uids(table: &str, port: u16) -> Vec<u32> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.get(3) != Some(&"0A") {
                return None;
            }
            let hex = fields.get(1)?.rsplit(':').next()?;
            if u16::from_str_radix(hex, 16).ok()? != port {
                return None;
            }
            fields.get(7)?.parse().ok()
        })
        .collect()
}

fn target(path: &str, query: &[(&str, &str)]) -> String {
    if query.is_empty() {
        return path.to_string();
    }
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in query {
        ser.append_pair(key, value);
    }
    format!("{path}?{}", ser.finish())
}

struct Raw {
    status: u16,
    body: String,
    keep: bool,
    retry_after: Option<Duration>,
}

/// Each later wait is this many times the last, and never past five seconds.
const BUSY_GROWTH: u32 = 3;
const BUSY_WAIT_CAP: Duration = Duration::from_secs(5);

thread_local! {
    /// The command's admission deadline, as a wall time. A nested guard
    /// keeps the sooner of the two, so a search inside a longer command
    /// stays short. The socket clock is a separate bound.
    static COMMAND_DEADLINE: std::cell::RefCell<Option<SystemTime>> = const { std::cell::RefCell::new(None) };
    /// The `X-Packset-Deadline` for the request in flight, frozen when the
    /// call starts so a retry does not push the deadline out.
    static HELD_DEADLINE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// How long this command is willing to wait for the pack. Search and
/// recall pass a few seconds. Remember passes twenty. Import passes a minute.
pub struct CommandDeadline {
    prev: Option<SystemTime>,
}

impl CommandDeadline {
    #[must_use]
    pub fn enter(budget: Duration) -> Self {
        let prev = COMMAND_DEADLINE.with(|cell| *cell.borrow());
        let proposed = SystemTime::now() + budget;
        let next = match prev {
            Some(existing) if existing <= proposed => existing,
            _ => proposed,
        };
        COMMAND_DEADLINE.with(|cell| *cell.borrow_mut() = Some(next));
        Self { prev }
    }
}

impl Drop for CommandDeadline {
    fn drop(&mut self) {
        let prev = self.prev.take();
        COMMAND_DEADLINE.with(|cell| *cell.borrow_mut() = prev);
    }
}

struct HeldDeadline {
    prev: Option<String>,
    active: bool,
}

impl HeldDeadline {
    fn enter(budget: Duration) -> Self {
        let prev = HELD_DEADLINE.with(|cell| cell.borrow().clone());
        if prev.is_some() {
            return Self {
                prev,
                active: false,
            };
        }
        let stamp = deadline_stamp(budget);
        HELD_DEADLINE.with(|cell| *cell.borrow_mut() = Some(stamp));
        Self { prev, active: true }
    }
}

impl Drop for HeldDeadline {
    fn drop(&mut self) {
        if self.active {
            let prev = self.prev.clone();
            HELD_DEADLINE.with(|cell| *cell.borrow_mut() = prev);
        }
    }
}

fn is_write(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD")
}

fn command_remaining() -> Option<Duration> {
    let when = COMMAND_DEADLINE.with(|cell| *cell.borrow())?;
    Some(
        when.duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO),
    )
}

/// How long the retries of one call may take. A read stops inside the
/// short search budget. A write may use the command's remaining time,
/// which is longer for remember and import.
pub(crate) fn admission_retry_budget(method: &str, per_try: Duration) -> Duration {
    let fallback = if is_write(method) {
        super::REMEMBER_BUDGET
    } else {
        super::SEARCH_PACK_BUDGET
    };
    let room = match (command_remaining(), clock_left()) {
        (Some(cmd), Some(clock)) => cmd.min(clock),
        (Some(cmd), None) => cmd,
        (None, Some(clock)) => clock,
        (None, None) => fallback,
    };
    let room = if is_write(method) {
        room
    } else {
        room.min(super::SEARCH_PACK_BUDGET)
    };
    per_try.min(room).max(Duration::from_millis(1))
}

fn deadline_stamp(budget: Duration) -> String {
    let proposed = SystemTime::now() + budget;
    let when = match COMMAND_DEADLINE.with(|cell| *cell.borrow()) {
        Some(command) if command < proposed => command,
        _ => proposed,
    };
    unix_ms(when).to_string()
}

fn unix_ms(when: SystemTime) -> u64 {
    when.duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn current_deadline(method: &str) -> String {
    HELD_DEADLINE
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| deadline_stamp(admission_retry_budget(method, budget())))
}

struct Pause {
    sleep: Duration,
    charge: Duration,
}

fn planned_wait(backoff: Duration, retry_after: Option<Duration>) -> Duration {
    match retry_after {
        Some(after) if after > backoff => after,
        _ => backoff,
    }
}

/// Half of `wait`, plus up to the other half. `salt` picks the fraction,
/// so a herd waiting on the same `Retry-After` does not wake together.
fn jittered(wait: Duration, salt: u64) -> Duration {
    let ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX).max(1);
    let half = ms / 2;
    let extra = if half == 0 { 0 } else { salt % (half + 1) };
    Duration::from_millis(half + extra)
}

/// The next wait, when charging it still ends inside `budget`. The sleep
/// is the jittered wait. The charge is the full wait, so every host stops
/// on the same attempt.
fn pause_for(
    backoff: Duration,
    retry_after: Option<Duration>,
    waited: Duration,
    budget: Duration,
    salt: u64,
) -> Option<Pause> {
    let charge = planned_wait(backoff, retry_after);
    if waited.saturating_add(charge) >= budget {
        return None;
    }
    let room = budget.saturating_sub(waited).max(Duration::from_millis(1));
    Some(Pause {
        sleep: jittered(charge, salt)
            .min(room)
            .max(Duration::from_millis(1)),
        charge,
    })
}

fn retry_after_of(header: Option<&str>) -> Option<Duration> {
    let raw = header?.trim();
    if raw.is_empty() {
        return None;
    }
    raw.parse::<u64>().ok().map(Duration::from_secs)
}

fn jitter_salt(asked: u32) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::from(elapsed.subsec_nanos()))
        .unwrap_or(0);
    nanos ^ u64::from(asked)
}

fn shed_error(method: &str) -> Error {
    let line = if is_write(method) {
        super::PACK_SHED_LINE
    } else {
        super::PACK_READ_BUSY_LINE
    };
    Error::Unavailable(line.to_string())
}

fn deadline_error() -> Error {
    Error::Unavailable(super::PACK_DEADLINE_LINE.to_string())
}

#[cfg(test)]
pub(crate) fn busy_attempts(budget: Duration) -> u32 {
    let mut waited = Duration::ZERO;
    let mut wait = super::BUSY_WAIT;
    let mut attempts = 1u32;
    while pause_for(wait, None, waited, budget, 0).is_some() {
        waited = waited.saturating_add(wait);
        wait = (wait * BUSY_GROWTH).min(BUSY_WAIT_CAP);
        attempts = attempts.saturating_add(1);
    }
    attempts
}

/// The pack accepted a connection and then stayed silent, or the connect
/// itself never finished. While a [`QuietPack`] guard is alive, the first
/// such read still waits out its budget and a later read returns before it
/// opens another socket. A consensus reading asks several times. Without
/// the stop, each ask waited the whole budget and the command sat for the
/// sum. Dropping the guard clears it, so a later call can try the pack again.
static PACK_QUIET: AtomicBool = AtomicBool::new(false);
static QUIET_HELD: AtomicUsize = AtomicUsize::new(0);

/// Remember one quiet pack for the reads this guard covers.
pub struct QuietPack;

impl QuietPack {
    /// Start remembering a silent pack. The first guard of a pile drops a
    /// quiet read already seen, so the pile starts with a fresh try.
    #[must_use]
    pub fn enter() -> Self {
        if QUIET_HELD.fetch_add(1, Ordering::AcqRel) == 0 {
            PACK_QUIET.store(false, Ordering::Relaxed);
        }
        Self
    }
}

impl Drop for QuietPack {
    fn drop(&mut self) {
        if QUIET_HELD.fetch_sub(1, Ordering::AcqRel) == 1 {
            PACK_QUIET.store(false, Ordering::Relaxed);
        }
    }
}

fn holding_quiet() -> bool {
    QUIET_HELD.load(Ordering::Relaxed) > 0
}

fn pack_quiet() -> bool {
    holding_quiet() && PACK_QUIET.load(Ordering::Relaxed)
}

fn mark_pack_quiet() {
    if holding_quiet() {
        PACK_QUIET.store(true, Ordering::Relaxed);
    }
}

/// The tool call's own bound is spent. A shorter read, one atom inside
/// its own budget, times out while the call still has time and the caller
/// carries on. That read is not the call failing.
fn tool_bound_spent() -> bool {
    crate::tool_bound::remaining().is_some_and(|left| left <= Duration::from_millis(50))
}

fn pack_io(err: std::io::Error) -> std::io::Error {
    let quiet = io_is_quiet(&err) || err.to_string().contains("the pack did not answer");
    if quiet && tool_bound_spent() {
        crate::tool_bound::note("packsetd");
        crate::tool_bound::mark_failed("packsetd timed out");
        return std::io::Error::new(std::io::ErrorKind::TimedOut, "packsetd timed out");
    }
    err
}

fn quiet_io() -> std::io::Error {
    pack_io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "the pack did not answer",
    ))
}

/// A read or a connect that ended because the socket stayed silent.
/// A refused connect is not this: the writer may still be starting.
fn io_is_quiet(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

fn connect(addr: SocketAddr, timeout: Duration) -> std::io::Result<TcpStream> {
    if pack_quiet() {
        return Err(quiet_io());
    }
    if side_budget_spent() {
        return Err(budget_spent());
    }
    let started = Instant::now();
    let result = connect_once(addr, timeout);
    // A connect that returns a socket is the pack accepting. Time spent
    // there is not a quiet wait. A connect that fails spends the budget.
    // A connect that never finishes is remembered, so the next read does
    // not wait the budget out again.
    if let Err(err) = &result {
        if io_is_quiet(err) {
            mark_pack_quiet();
        }
        super::charge_side_wait(started.elapsed());
    }
    result
}

/// A side event's pack budget is gone. Opening a socket for a one
/// millisecond timer still blocks, and a prompt makes several of those
/// reads, so the call returns before the connect.
fn side_budget_spent() -> bool {
    super::side_remaining_ms() == Some(0)
}

fn budget_spent() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "the side event's pack budget is spent",
    )
}

fn connect_once(addr: SocketAddr, timeout: Duration) -> std::io::Result<TcpStream> {
    let wait = bounded_wait(timeout, true)?;
    let stream = TcpStream::connect_timeout(&addr, wait)?;
    let io_wait = bounded_wait(timeout, false)?;
    stream.set_read_timeout(Some(io_wait))?;
    stream.set_write_timeout(Some(io_wait))?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

/// The wait a socket call may use. With a [`PackClock`], a connect is
/// capped by [`super::pack_connect_budget`] and every call stops at the
/// clock. Past the clock the call returns before it opens a socket.
fn bounded_wait(requested: Duration, connect: bool) -> std::io::Result<Duration> {
    let mut wait = capped_wait(requested);
    if let Some(deadline) = clock_deadline() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(pack_io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the pack did not answer",
            )));
        }
        wait = wait.min(left);
        if connect {
            wait = wait.min(super::pack_connect_budget());
        }
        if wait.is_zero() {
            return Err(pack_io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the pack did not answer",
            )));
        }
    }
    Ok(wait)
}

/// A side-event bound shrinks this wait. A zero bound is one millisecond,
/// so a later read fails at once instead of taking the default.
fn capped_wait(timeout: Duration) -> Duration {
    let wait = super::cap_pack_wait(timeout);
    if wait.is_zero() {
        Duration::from_millis(1)
    } else {
        wait
    }
}

fn write_all(stream: &mut TcpStream, bytes: &[u8]) -> std::io::Result<()> {
    let mut sent = 0;
    while sent < bytes.len() {
        match stream.write(&bytes[sent..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "the pack closed the socket",
                ))
            }
            Ok(n) => sent += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) => return Err(pack_io(err)),
        }
    }
    stream.flush()
}

fn read_response(stream: &mut TcpStream, deadline: Instant) -> std::io::Result<Raw> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if buf.len() > 64 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the pack's headers are too long",
            ));
        }
        arm_read(stream, deadline)?;
        let n = read_some(stream, &mut tmp)?;
        buf.extend_from_slice(&tmp[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
    };
    let header = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the pack's status line is not HTTP",
            )
        })?;
    let mut length = None;
    let mut close = header.starts_with("HTTP/1.0");
    let mut retry_after = None;
    for line in header.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().ok();
        } else if name.eq_ignore_ascii_case("retry-after") {
            retry_after = retry_after_of(Some(value));
        } else if name.eq_ignore_ascii_case("connection")
            && value.trim().eq_ignore_ascii_case("close")
        {
            close = true;
        }
    }
    let mut body = buf.split_off(head_end + 4);
    if let Some(length) = length {
        while body.len() < length {
            arm_read(stream, deadline)?;
            let n = read_some(stream, &mut tmp)?;
            body.extend_from_slice(&tmp[..n]);
            if body.len() > length.saturating_add(64 * 1024) {
                break;
            }
        }
        body.truncate(length);
    } else {
        loop {
            if deadline.saturating_duration_since(Instant::now()).is_zero() {
                break;
            }
            let _ = arm_read(stream, deadline);
            match stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => body.extend_from_slice(&tmp[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => break,
                Err(_) => break,
            }
        }
        close = true;
    }
    // A redirect is the answer. The token must not follow it.
    Ok(Raw {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
        keep: !close && !(300..400).contains(&status),
        retry_after,
    })
}

fn arm_read(stream: &TcpStream, deadline: Instant) -> std::io::Result<()> {
    let remain = deadline.saturating_duration_since(Instant::now());
    if remain.is_zero() {
        return Err(pack_io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "the pack did not answer",
        )));
    }
    stream.set_read_timeout(Some(remain))?;
    Ok(())
}

fn read_some(stream: &mut TcpStream, tmp: &mut [u8]) -> std::io::Result<usize> {
    loop {
        match stream.read(tmp) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "the pack closed the socket",
                ))
            }
            Ok(n) => return Ok(n),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) => return Err(pack_io(err)),
        }
    }
}

fn render(
    method: &str,
    host: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&[u8]>,
) -> Vec<u8> {
    let deadline = current_deadline(method);
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: keep-alive\r\nX-Packset-Deadline: {deadline}\r\n"
    );
    if let Some(token) = token {
        head.push_str("Authorization: Bearer ");
        head.push_str(token);
        head.push_str("\r\n");
    }
    let mut out = Vec::new();
    if let Some(body) = body {
        head.push_str("Content-Type: application/json\r\n");
        head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        out.extend(head.into_bytes());
        out.extend_from_slice(body);
    } else {
        head.push_str("\r\n");
        out.extend(head.into_bytes());
    }
    out
}

struct Wire<'a> {
    method: &'a str,
    path: &'a str,
    query: &'a [(&'a str, &'a str)],
    body: Option<&'a [u8]>,
    token: Option<&'a str>,
}

enum Gate {
    /// The proof held. The call may carry the token.
    Send,
    /// The listener did not ask for the token.
    Bare,
    /// The writer is busy. The call is not sent.
    Busy(Raw),
}

impl PacksetClient {
    pub fn new(base: impl Into<String>) -> Self {
        let mut base = base.into();
        while base.ends_with('/') {
            base.pop();
        }
        let origin = Url::parse(&base).unwrap_or_else(|_| Url::parse("http://127.0.0.1/").unwrap());
        let addr = Url::parse(&base).ok().as_ref().and_then(pin);
        Self {
            base,
            origin,
            addr,
            workspace: None,
            token: Mutex::new(load_token()),
        }
    }

    /// The writer the seat talks to. `PACKSET_URL=off` is no pack.
    pub fn from_env() -> Result<Self, Error> {
        packset_client::load_seat_env();
        let url = std::env::var("PACKSET_URL")
            .or_else(|_| std::env::var("INSIDE_MEMORY_URL"))
            .ok()
            .filter(|url| !url.is_empty());
        match url {
            Some(url) if url == "off" => Err(Error::NoUrl),
            Some(url) => Ok(Self::new(url)),
            None => Ok(Self::new(format!(
                "http://127.0.0.1:{}",
                packset_client::default_port()
            ))),
        }
    }

    #[must_use]
    pub fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        let workspace = workspace.into();
        self.workspace = (!workspace.is_empty()).then_some(workspace);
        self
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Whether this seat has a token to show. The token itself stays here.
    #[must_use]
    pub(crate) fn has_token(&self) -> bool {
        load_token().is_some()
    }

    /// Whether a call may carry the token: the seat has one, and another
    /// user does not hold the loopback port.
    #[must_use]
    pub(crate) fn may_carry(&self) -> bool {
        load_token().is_some() && !foreign(&self.base)
    }

    /// Whether the last answer on this socket carried the token. A closed
    /// socket does not keep this for the next call: that call proves again.
    #[must_use]
    pub(crate) fn carried(&self) -> bool {
        let Some(addr) = self.addr else {
            return false;
        };
        let map = slots().lock().unwrap_or_else(|p| p.into_inner());
        map.get(&addr)
            .is_some_and(|slot| slot.lock().unwrap_or_else(|p| p.into_inner()).carried)
    }

    pub fn workspace(&self) -> String {
        if let Some(workspace) = &self.workspace {
            return workspace.clone();
        }
        if let Ok(workspace) = std::env::var("PACKSET_WORKSPACE") {
            if !workspace.is_empty() {
                return workspace;
            }
        }
        let cwd = std::env::var("HOSTPACK_WORKSPACE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let abs = cwd.canonicalize().unwrap_or(cwd);
        let shown = abs.to_string_lossy().into_owned();
        if let Ok(answer) = self.roundtrip(
            "GET",
            "/v1/identity",
            &[("cwd", shown.as_str())],
            None,
            true,
            budget(),
        ) {
            if answer.status == 200 {
                if let Ok(val) = serde_json::from_str::<Value>(&answer.body) {
                    if let Some(ws) = val.get("workspace").and_then(|v| v.as_str()) {
                        if !ws.is_empty() {
                            return ws.to_string();
                        }
                    }
                }
            }
        }
        format!("dir:{}", abs.display())
    }

    /// One call. `bearer` asks to carry the token; it is left off when
    /// the proof does not hold or another user owns the port. A 3xx is
    /// returned as the answer and not followed. A 503 is asked again
    /// after `Retry-After`, with jitter, while that wait still fits in
    /// the call's budget. A 408 is the deadline passing: it is not asked
    /// again.
    pub(crate) fn roundtrip(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&[u8]>,
        bearer: bool,
        timeout: Duration,
    ) -> Result<Answer, Error> {
        // A write changes what a listing would say: the memo this process
        // keeps and the gate's freshness mark both go.
        if method != "GET" {
            crate::forget_atom_listings();
            crate::forget_gate_fresh();
            forget_kept_answers();
        }
        let memo_key = answer_memo_key(method, path, query, bearer);
        if let Some(answer) = memo_key.as_deref().and_then(kept_answer) {
            return Ok(answer);
        }
        let Some(addr) = self.addr else {
            return Err(Error::Bad(format!(
                "{} is not a socket this seat can pin",
                self.base
            )));
        };
        let token = load_token();
        *self.token.lock().unwrap_or_else(|p| p.into_inner()) = token.clone();
        let allow = bearer && token.is_some() && !foreign(&self.base);
        if crate::tool_bound::armed() {
            crate::tool_bound::note("packsetd");
        }
        let budget = admission_retry_budget(method, timeout);
        let _held = HeldDeadline::enter(budget);
        let mut waited = Duration::ZERO;
        let mut wait = super::BUSY_WAIT;
        let mut asked = 0u32;
        loop {
            let answer = self.exchange(
                addr,
                &Wire {
                    method,
                    path,
                    query,
                    body,
                    token: token.as_deref(),
                },
                allow,
                timeout,
            )?;
            if answer.status == 408 {
                return Err(deadline_error());
            }
            if answer.status != 503 {
                if let Some(key) = &memo_key {
                    if (200..300).contains(&answer.status) {
                        keep_answer(key.clone(), &answer);
                    }
                }
                return Ok(answer);
            }
            asked = asked.saturating_add(1);
            match pause_for(wait, answer.retry_after, waited, budget, jitter_salt(asked)) {
                Some(pause) => {
                    std::thread::sleep(pause.sleep);
                    waited = waited.saturating_add(pause.charge);
                    wait = (wait * BUSY_GROWTH).min(BUSY_WAIT_CAP);
                }
                None => return Err(shed_error(method)),
            }
        }
    }

    fn exchange(
        &self,
        addr: SocketAddr,
        call: &Wire<'_>,
        allow: bool,
        timeout: Duration,
    ) -> Result<Answer, Error> {
        // The proof and the call share this budget. A nested guard keeps
        // the command's earlier deadline, so a hook stays under 2s and an
        // interactive command under 5s across every socket of the call.
        // A restart does not start the budget over.
        let _budget = PackClock::enter(timeout);
        let slot = slot_for(addr);
        let mut live = slot.lock().unwrap_or_else(|p| p.into_inner());
        let tag = token_tag(call.token);
        if live.token_tag != tag {
            live.stream = None;
            live.proven = false;
            live.token_tag = tag;
        }
        let carried = if allow {
            match self.gate(&mut live, addr, timeout)? {
                Gate::Bare => None,
                Gate::Send => call.token,
                Gate::Busy(raw) => {
                    live.carried = false;
                    live.proven = false;
                    return Ok(Answer {
                        status: raw.status,
                        body: raw.body,
                        trusted: false,
                        retry_after: raw.retry_after,
                    });
                }
            }
        } else {
            None
        };
        if live.stream.is_none() {
            match connect(addr, timeout) {
                Ok(stream) => live.stream = Some(stream),
                Err(err) => {
                    live.proven = false;
                    live.carried = false;
                    return Err(Error::Io(err));
                }
            }
            // The probes ran on sockets the writer closed. This socket is
            // the one the token rides, the third step of the same proof,
            // not a later call.
            live.proven = false;
        }
        let raw = match send(
            &mut live,
            &self.origin,
            &Wire {
                token: carried,
                ..*call
            },
            timeout,
        ) {
            Ok(raw) => raw,
            Err(err) => {
                live.stream = None;
                live.proven = false;
                live.carried = false;
                return Err(Error::Io(err));
            }
        };
        let trusted = carried.is_some();
        live.carried = trusted;
        if trusted && live.stream.is_some() {
            live.proven = true;
        } else if live.stream.is_none() {
            live.proven = false;
        }
        Ok(Answer {
            status: raw.status,
            body: raw.body,
            trusted,
            retry_after: raw.retry_after,
        })
    }

    /// Two refusals on this socket, then the call may carry the token.
    /// A busy writer (503) is returned as that answer; the call is not sent.
    fn gate(&self, live: &mut Slot, addr: SocketAddr, timeout: Duration) -> Result<Gate, Error> {
        if live.proven && live.stream.is_some() {
            return Ok(Gate::Send);
        }
        live.proven = false;
        let (Some(id), Some(bogus)) = (nonce(), nonce()) else {
            return Err(Error::Io(std::io::Error::other(
                "no randomness for the pack proof",
            )));
        };
        let workspace = self.proof_workspace();
        let path = format!("/v1/atoms/{}", &id[..32]);
        let query = [("workspace", workspace.as_str())];
        let mut stage = 0u8;
        let mut restarts = 0u8;
        loop {
            if live.stream.is_none() {
                live.stream = Some(connect(addr, timeout).map_err(Error::Io)?);
                live.proven = false;
            }
            let shown = if stage == 0 {
                None
            } else {
                Some(bogus.as_str())
            };
            let raw = match send(
                live,
                &self.origin,
                &Wire {
                    method: "GET",
                    path: &path,
                    query: &query,
                    body: None,
                    token: shown,
                },
                timeout,
            ) {
                Ok(raw) => raw,
                Err(err) => {
                    live.stream = None;
                    live.proven = false;
                    // A silent writer is the answer. Another socket would
                    // wait the budget out again, and three of those is a
                    // minute and a half. A closed socket may be tried
                    // again, inside the same deadline.
                    if io_is_quiet(&err) {
                        return Err(Error::Io(err));
                    }
                    restarts += 1;
                    let spent = clock_deadline().is_some_and(|deadline| Instant::now() >= deadline);
                    if spent || restarts > 2 {
                        return Err(Error::Io(err));
                    }
                    stage = 0;
                    continue;
                }
            };
            // A shed or a passed deadline is the answer. The call is not sent.
            if raw.status == 503 || raw.status == 408 {
                return Ok(Gate::Busy(raw));
            }
            if raw.status != 401 {
                return Ok(Gate::Bare);
            }
            if stage == 0 {
                stage = 1;
                continue;
            }
            return Ok(Gate::Send);
        }
    }

    fn proof_workspace(&self) -> String {
        if let Some(workspace) = &self.workspace {
            return workspace.clone();
        }
        std::env::var("PACKSET_WORKSPACE")
            .ok()
            .filter(|w| !w.is_empty())
            .unwrap_or_else(|| "seat".to_string())
    }

    /// [`Self::roundtrip`] with the token when the proof holds.
    fn tried(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<Answer, Error> {
        self.roundtrip(method, path, query, body, true, timeout)
    }

    fn json_ok<T: DeserializeOwned>(&self, path: &str, answer: Answer) -> Result<T, Error> {
        if !(200..300).contains(&answer.status) {
            return Err(refused(
                &format!("{}{path}", self.base),
                answer.status,
                &answer.body,
            ));
        }
        serde_json::from_str(&answer.body).map_err(Error::from)
    }

    /// The `line` field of `GET /stats`: `ok`, or `busy, retrying` when
    /// the writer is shedding. One try, so doctor does not spend the
    /// read budget on a writer that has no `/stats`. `None` when the
    /// writer does not answer, or the body has no line.
    #[must_use]
    pub fn admission_line(&self) -> Option<String> {
        let timeout =
            Duration::from_millis(300).min(clock_left().unwrap_or(Duration::from_secs(1)));
        let answer = self
            .exchange_direct("GET", "/stats", &[], None, true, timeout)
            .ok()?;
        if answer.status == 503 {
            return Some("busy, retrying".to_string());
        }
        if !(200..300).contains(&answer.status) {
            return None;
        }
        let value: Value = serde_json::from_str(&answer.body).ok()?;
        value
            .get("line")
            .and_then(Value::as_str)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
    }

    /// One exchange, with the deadline header and no retry. `/stats` uses
    /// this so a busy writer is one answer, not a wait.
    fn exchange_direct(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&[u8]>,
        bearer: bool,
        timeout: Duration,
    ) -> Result<Answer, Error> {
        let Some(addr) = self.addr else {
            return Err(Error::Bad(format!(
                "{} is not a socket this seat can pin",
                self.base
            )));
        };
        let token = load_token();
        let allow = bearer && token.is_some() && !foreign(&self.base);
        if crate::tool_bound::armed() {
            crate::tool_bound::note("packsetd");
        }
        let budget = admission_retry_budget(method, timeout);
        let _held = HeldDeadline::enter(budget);
        self.exchange(
            addr,
            &Wire {
                method,
                path,
                query,
                body,
                token: token.as_deref(),
            },
            allow,
            timeout,
        )
    }

    pub fn health(&self) -> Result<String, Error> {
        let answer = self.tried("GET", "/health", &[], None, budget())?;
        if !(200..300).contains(&answer.status) {
            return Err(refused(
                &format!("{}/health", self.base),
                answer.status,
                &answer.body,
            ));
        }
        Ok(answer.body)
    }

    pub fn status(&self, workspace: Option<&str>) -> Result<Value, Error> {
        let query: Vec<(&str, &str)> = workspace
            .map(|workspace| vec![("workspace", workspace)])
            .unwrap_or_default();
        let answer = self.tried("GET", "/v1/status", &query, None, budget())?;
        self.json_ok("/v1/status", answer)
    }

    pub fn post_atom(&self, atom: &Value) -> Result<Value, Error> {
        let bytes = serde_json::to_vec(atom)?;
        let answer = self.tried("POST", "/v1/atoms", &[], Some(&bytes), budget())?;
        self.json_ok("/v1/atoms", answer)
    }

    pub fn delete_atom(
        &self,
        workspace: &str,
        id: &str,
        why: Option<&str>,
    ) -> Result<Value, Error> {
        let mut body = serde_json::json!({"workspace": workspace, "id": id});
        if let Some(accession) = why {
            body["why"] = Value::String(accession.to_string());
        }
        let bytes = serde_json::to_vec(&body)?;
        let answer = self.tried("POST", "/v1/atoms/delete", &[], Some(&bytes), budget())?;
        if answer.status == 404 {
            return Err(Error::Bad(format!("no atom {id}")));
        }
        self.json_ok("/v1/atoms/delete", answer)
    }

    pub fn atoms_of_kind(&self, workspace: &str, kind: &str) -> Result<Vec<Value>, Error> {
        let answer = self.tried(
            "GET",
            "/v1/atoms",
            &[("workspace", workspace), ("kind", kind)],
            None,
            budget(),
        )?;
        let body: Value = self.json_ok("/v1/atoms", answer)?;
        Ok(body
            .get("atoms")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub fn atoms_in_set(&self, workspace: &str, set: &str) -> Result<Vec<Value>, Error> {
        let answer = self.tried(
            "GET",
            "/v1/pack",
            &[("workspace", workspace), ("set", set)],
            None,
            budget(),
        )?;
        let body: Value = self.json_ok("/v1/pack", answer)?;
        Ok(body
            .get("atoms")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub fn sweep(&self, workspace: &str) -> Result<Value, Error> {
        let bytes = serde_json::to_vec(&serde_json::json!({"workspace": workspace}))?;
        let answer = self.tried("POST", "/v1/sweep", &[], Some(&bytes), budget())?;
        self.json_ok("/v1/sweep", answer)
    }

    pub fn grade(&self, workspace: &str, id: &str, recalled: bool) -> Result<Value, Error> {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "workspace": workspace,
            "id": id,
            "recalled": recalled,
        }))?;
        let answer = self.tried("POST", "/v1/grade", &[], Some(&bytes), budget())?;
        self.json_ok("/v1/grade", answer)
    }

    /// Merge `fields` into one current atom, under its id.
    pub fn update_atom(&self, workspace: &str, id: &str, fields: &Value) -> Result<Value, Error> {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "workspace": workspace,
            "id": id,
            "fields": fields,
        }))?;
        let answer = self.tried("POST", "/v1/atoms/update", &[], Some(&bytes), budget())?;
        self.json_ok("/v1/atoms/update", answer)
    }

    pub fn activate_as(
        &self,
        workspace: &str,
        q: &str,
        limit: u32,
        fire: bool,
        lens: Option<&str>,
    ) -> Result<Value, Error> {
        let limit = limit.to_string();
        let fire = if fire { "1" } else { "0" };
        let mut query = vec![
            ("workspace", workspace),
            ("q", q),
            ("limit", limit.as_str()),
            ("fire", fire),
        ];
        if let Some(name) = lens.filter(|n| !n.trim().is_empty()) {
            query.push(("as", name));
        }
        let answer = self.tried("GET", "/v1/activate", &query, None, budget())?;
        self.json_ok("/v1/activate", answer)
    }

    pub fn hubs(&self, workspace: &str, limit: usize) -> Result<Value, Error> {
        let limit = limit.to_string();
        let answer = self.tried(
            "GET",
            "/v1/hubs",
            &[("workspace", workspace), ("limit", limit.as_str())],
            None,
            budget(),
        )?;
        self.json_ok("/v1/hubs", answer)
    }

    pub fn consolidate(&self, workspace: &str, apply: bool) -> Result<Value, Error> {
        let bytes =
            serde_json::to_vec(&serde_json::json!({"workspace": workspace, "apply": apply}))?;
        let answer = self.tried("POST", "/v1/consolidate", &[], Some(&bytes), budget())?;
        self.json_ok("/v1/consolidate", answer)
    }

    pub fn search_opts(
        &self,
        workspace: &str,
        q: &str,
        limit: u32,
        as_of: Option<&str>,
        rerank: bool,
    ) -> Result<Vec<Hit>, Error> {
        Ok(self.search_fetched(workspace, q, limit, as_of, rerank)?.0)
    }

    pub(crate) fn search_fetched(
        &self,
        workspace: &str,
        q: &str,
        limit: u32,
        as_of: Option<&str>,
        rerank: bool,
    ) -> Result<(Vec<Hit>, bool), Error> {
        let limit_s = limit.to_string();
        let mut query = vec![
            ("workspace", workspace),
            ("q", q),
            ("limit", limit_s.as_str()),
        ];
        if let Some(at) = as_of {
            query.push(("as_of", at));
        }
        if rerank {
            query.push(("rerank", "1"));
        }
        // A rerank used to wait at least a minute, past any shorter
        // budget. A clocked call, and a hook, keep the short budget.
        let timeout = if rerank && !clock_armed() && std::env::var_os("LJOS_IN_HOOK").is_none() {
            Duration::from_secs(60).max(budget())
        } else {
            budget()
        };
        let answer = self.tried("GET", "/v1/search", &query, None, timeout)?;
        let trusted = answer.trusted;
        let body: Value = self.json_ok("/v1/search", answer)?;
        let hits = body.get("hits").cloned().unwrap_or(Value::Array(vec![]));
        Ok((serde_json::from_value(hits)?, trusted))
    }

    pub fn fire(&self, workspace: &str, ids: &[String]) -> Result<Value, Error> {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "workspace": workspace,
            "ids": ids,
            "as": "",
        }))?;
        let answer = self.tried("POST", "/v1/fire", &[], Some(&bytes), budget())?;
        self.json_ok("/v1/fire", answer)
    }
}

fn refused(url: &str, code: u16, body: &str) -> Error {
    let reason = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|r| r.as_str()).map(str::to_string))
        .unwrap_or_else(|| body.to_string());
    let reason = reason.trim();
    if code == 401 {
        return Error::Bad(format!(
            "{url}: 401: {reason}; this seat's token does not match the writer's, \
             so it is another user's writer or PACKSET_HOME names another pack"
        ));
    }
    if reason.is_empty() {
        Error::Bad(format!("{url}: status code {code}"))
    } else {
        Error::Bad(format!("{url}: {code}: {reason}"))
    }
}

fn send(live: &mut Slot, origin: &Url, call: &Wire<'_>, timeout: Duration) -> std::io::Result<Raw> {
    if pack_quiet() {
        return Err(quiet_io());
    }
    if side_budget_spent() {
        return Err(budget_spent());
    }
    let started = Instant::now();
    let result = send_once(live, origin, call, timeout);
    // An answer is not charged. A read that ends without one is a quiet
    // socket and spends the budget, so the next read does not open another.
    // Outside a hook the side budget is not armed. The quiet flag is that
    // same stop for a command, which asks the pack more than once.
    if let Err(err) = &result {
        if io_is_quiet(err) {
            mark_pack_quiet();
        }
        super::charge_side_wait(started.elapsed());
    }
    result
}

fn send_once(
    live: &mut Slot,
    origin: &Url,
    call: &Wire<'_>,
    timeout: Duration,
) -> std::io::Result<Raw> {
    let stream = live
        .stream
        .as_mut()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "no pack socket"))?;
    let wait = bounded_wait(timeout, false)?;
    let deadline = Instant::now() + wait;
    stream.set_read_timeout(Some(wait))?;
    stream.set_write_timeout(Some(wait))?;
    let bytes = render(
        call.method,
        &host_header(origin),
        &target(call.path, call.query),
        call.token,
        call.body,
    );
    if let Err(err) = write_all(stream, &bytes) {
        live.stream = None;
        live.proven = false;
        return Err(err);
    }
    match read_response(stream, deadline) {
        Ok(raw) => {
            if !raw.keep {
                live.stream = None;
                live.proven = false;
            }
            Ok(raw)
        }
        Err(err) => {
            live.stream = None;
            live.proven = false;
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{foreign, OwnerOnce, TABLE_READS};

    fn reads() -> usize {
        TABLE_READS.with(std::cell::Cell::get)
    }

    /// The requests of one search share one read of the socket tables for
    /// each url. A guard inside it keeps that read, and a call after the
    /// search reads them again.
    #[test]
    fn a_search_reads_the_port_owner_once_for_each_url() {
        let _g = crate::tests::env_guard();
        // SAFETY: under the lock every environment-reading test takes.
        unsafe { std::env::remove_var("LJOS_IN_HOOK") };
        let (url, other) = ("http://127.0.0.1:9", "http://127.0.0.1:10");
        let start = reads();
        foreign(url);
        foreign(url);
        assert_eq!(reads() - start, 2, "outside a search each call reads");
        {
            let _search = OwnerOnce::enter();
            let first = foreign(url);
            {
                let _inner = OwnerOnce::enter();
                assert_eq!(foreign(url), first);
            }
            assert_eq!(foreign(url), first);
            foreign(other);
            foreign(other);
            assert_eq!(reads() - start, 4, "one search reads once a url");
        }
        foreign(url);
        assert_eq!(reads() - start, 5, "a call after the search reads again");
    }

    fn quiet_pack() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        unsafe {
            std::env::remove_var("PACKSET_TOKEN");
            std::env::remove_var("PACKSET_TOKEN_FILE");
            std::env::remove_var("PACKSET_TIMEOUT_MS");
            std::env::remove_var("LJOS_IN_HOOK");
            std::env::set_var("PACKSET_HOME", home.path());
        }
        home
    }

    /// `on` sees the request text and how many requests came before this one.
    /// It answers with a status, a body, and `Retry-After` seconds when set.
    fn serve(
        on: impl Fn(usize, &str) -> (u16, String, Option<u64>) + Send + 'static,
    ) -> (
        super::PacksetClient,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = std::sync::Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut sock) = stream else { break };
                let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                loop {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let req = String::from_utf8_lossy(&buf).into_owned();
                let n = {
                    let mut log = log.lock().unwrap();
                    log.push(req.clone());
                    log.len() - 1
                };
                let (code, body, retry) = on(n, &req);
                let reason = match code {
                    200 => "OK",
                    408 => "Request Timeout",
                    503 => "Service Unavailable",
                    _ => "Error",
                };
                let retry = retry
                    .map(|secs| format!("Retry-After: {secs}\r\n"))
                    .unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 {code} {reason}\r\n{retry}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            }
        });
        (super::PacksetClient::new(format!("http://{addr}")), seen)
    }

    fn deadline_of(req: &str) -> u64 {
        req.lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("x-packset-deadline")
                    .then(|| value.trim().parse().ok())
                    .flatten()
            })
            .unwrap_or(0)
    }

    #[test]
    fn a_read_waits_out_retry_after_and_then_reads() {
        let _g = crate::tests::env_guard();
        let _home = quiet_pack();
        let (client, seen) = serve(|n, req| {
            assert!(deadline_of(req) > 0, "{req}");
            if n == 0 {
                (
                    503,
                    r#"{"error":"packsetd is busy: the read queue is full"}"#.into(),
                    Some(1),
                )
            } else {
                (200, "ok".into(), None)
            }
        });
        let started = std::time::Instant::now();
        let body = client.health().unwrap();
        let took = started.elapsed();
        assert_eq!(body, "ok");
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(
            took >= std::time::Duration::from_millis(400),
            "Retry-After was not waited: {took:?}"
        );
        assert!(took < std::time::Duration::from_secs(3), "{took:?}");
        let first = &seen.lock().unwrap()[0];
        let now = super::unix_ms(std::time::SystemTime::now());
        let stamp = deadline_of(first);
        assert!(stamp > now, "deadline {stamp} is not after {now}");
        assert!(stamp < now + 5_000, "deadline {stamp} is not a short read");
    }

    #[test]
    fn a_retry_after_past_the_read_budget_fails_in_one_line() {
        let _g = crate::tests::env_guard();
        let _home = quiet_pack();
        let (client, seen) = serve(|_, _| (503, r#"{"error":"busy"}"#.into(), Some(30)));
        let started = std::time::Instant::now();
        let err = client.health().unwrap_err();
        let took = started.elapsed();
        assert_eq!(err.to_string(), crate::PACK_READ_BUSY_LINE, "{err}");
        assert_eq!(seen.lock().unwrap().len(), 1, "the long wait was taken");
        assert!(took < std::time::Duration::from_millis(500), "{took:?}");
    }

    #[test]
    fn a_passed_deadline_is_one_line_and_is_not_retried() {
        let _g = crate::tests::env_guard();
        let _home = quiet_pack();
        let (client, seen) = serve(|_, req| {
            assert!(deadline_of(req) > 0, "{req}");
            (
                408,
                r#"{"error":"the deadline passed before this request ran"}"#.into(),
                None,
            )
        });
        let err = client.health().unwrap_err();
        assert_eq!(err.to_string(), crate::PACK_DEADLINE_LINE, "{err}");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_shed_write_names_the_later_retry() {
        let _g = crate::tests::env_guard();
        let _home = quiet_pack();
        let (client, seen) = serve(|_, _| (503, r#"{"error":"busy"}"#.into(), Some(1)));
        let started = std::time::Instant::now();
        let err = {
            let _deadline = super::CommandDeadline::enter(std::time::Duration::from_millis(200));
            client
                .post_atom(&serde_json::json!({"text": "one write", "workspace": "seat"}))
                .unwrap_err()
        };
        let took = started.elapsed();
        assert_eq!(err.to_string(), crate::PACK_SHED_LINE, "{err}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert!(took < std::time::Duration::from_millis(500), "{took:?}");
    }

    #[test]
    fn stats_line_is_the_admission_line() {
        let _g = crate::tests::env_guard();
        let _home = quiet_pack();
        let (client, seen) = serve(|_, req| {
            let line = req.lines().next().unwrap_or("");
            if line.contains("GET /stats") {
                (200, r#"{"ok":true,"line":"busy, retrying"}"#.into(), None)
            } else {
                (200, r#"{"line":"ok"}"#.into(), None)
            }
        });
        assert_eq!(client.admission_line().as_deref(), Some("busy, retrying"));
        assert_eq!(seen.lock().unwrap().len(), 1);
        let (client, seen) = serve(|_, _| (503, r#"{"error":"busy"}"#.into(), Some(1)));
        assert_eq!(client.admission_line().as_deref(), Some("busy, retrying"));
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "stats does not wait out Retry-After"
        );
    }
}
