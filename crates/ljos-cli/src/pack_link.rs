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
use std::time::{Duration, Instant};

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
#[derive(Debug)]
pub(crate) struct Answer {
    pub status: u16,
    pub body: String,
    pub trusted: bool,
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
    (host, port).to_socket_addrs().ok()?.next()
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
    std::env::var("PACKSET_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .map_or(Duration::from_secs(30), Duration::from_millis)
}

/// Whether every socket listening on the loopback port of `url` belongs to
/// another user. Unknown is not foreign.
fn foreign(url: &str) -> bool {
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

fn quiet_io() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::TimedOut, "the pack did not answer")
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
    let wait = capped_wait(timeout);
    let stream = TcpStream::connect_timeout(&addr, wait)?;
    stream.set_read_timeout(Some(wait))?;
    stream.set_write_timeout(Some(wait))?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
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
            Err(err) => return Err(err),
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
    for line in header.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().ok();
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
    })
}

fn arm_read(stream: &TcpStream, deadline: Instant) -> std::io::Result<()> {
    let remain = deadline.saturating_duration_since(Instant::now());
    if remain.is_zero() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "the pack did not answer",
        ));
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
            Err(err) => return Err(err),
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
    let mut head =
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: keep-alive\r\n");
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
        let cwd = std::env::var("GROKOS_WORKSPACE")
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

    /// One request. `bearer` asks to carry the token; it is left off when
    /// the proof does not hold or another user owns the port. A 3xx is
    /// returned as the answer and not followed. HTTP 503 is one answer:
    /// the caller decides whether to ask again.
    pub(crate) fn roundtrip(
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
        *self.token.lock().unwrap_or_else(|p| p.into_inner()) = token.clone();
        let allow = bearer && token.is_some() && !foreign(&self.base);
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

    fn exchange(
        &self,
        addr: SocketAddr,
        call: &Wire<'_>,
        allow: bool,
        timeout: Duration,
    ) -> Result<Answer, Error> {
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
                    restarts += 1;
                    if restarts > 2 {
                        return Err(Error::Io(err));
                    }
                    stage = 0;
                    continue;
                }
            };
            if raw.status == 503 {
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

    /// [`Self::roundtrip`] again while the writer answers busy, inside `timeout`.
    fn tried(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<Answer, Error> {
        let started = Instant::now();
        let mut wait = Duration::from_millis(50);
        for n in 0..4 {
            let answer = self.roundtrip(method, path, query, body, true, timeout)?;
            if answer.status == 503 && n < 3 && started.elapsed() + wait < timeout {
                std::thread::sleep(wait);
                wait *= 3;
                continue;
            }
            return Ok(answer);
        }
        Err(Error::Bad(format!(
            "{}{path}: 503: packsetd is busy",
            self.base
        )))
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
        let timeout = if rerank {
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
    let wait = capped_wait(timeout);
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
