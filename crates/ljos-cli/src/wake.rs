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
//! In push mode the letter is in this machine's pack and not yet on the
//! tracker the other machine reads. The detached wake pushes the sealed
//! log before it runs the command or posts the webhook, so the send
//! itself does not wait. A refused push is a line in the wake log. The
//! wake still runs, and the send still stands.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// How long a wake command may run before it is stopped, in seconds.
pub const WAKE_TIMEOUT_SECS: u64 = 30;

/// How long a webhook post may take, in seconds.
const WEBHOOK_TIMEOUT_SECS: u64 = 10;

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
}

/// The message a wake is about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    pub seat: String,
    pub from: String,
    pub msg: String,
    pub issue: Option<String>,
    pub interrupt: bool,
}

/// Where the wakes are kept: `wake.toml` beside `harnesses.toml`.
#[must_use]
pub fn path() -> PathBuf {
    super::harnesses_path().with_file_name("wake.toml")
}

/// Every seat's wake. An absent or unreadable file is no wakes.
#[must_use]
pub fn load() -> BTreeMap<String, Wake> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(table: &BTreeMap<String, Wake>) -> Result<()> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = format!(
        "# Wakes `ljos send` starts when mail reaches a seat. `ljos wake --help` sets them.\n{}",
        toml::to_string(table)?
    );
    std::fs::write(&path, text).with_context(|| format!("wake: {}", path.display()))
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

fn check_webhook(url: &str) -> Result<()> {
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|p| url.starts_with(p));
    if !url.starts_with("https://") && !loopback {
        bail!("wake: {url} is not https, nor http on this machine");
    }
    Ok(())
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
    let mut table = load();
    table.insert(
        seat.to_string(),
        Wake {
            command: Some(command.to_string()),
            timeout,
            ..Wake::default()
        },
    );
    save(&table)?;
    Ok(format!(
        "wake {seat}: runs `{command}` when mail reaches it\n"
    ))
}

/// Set a seat's webhook. The key stays in its file or variable.
///
/// # Errors
///
/// The URL is not https or loopback, or `wake.toml` cannot be written.
pub fn set_webhook(
    seat: &str,
    url: &str,
    key_file: Option<&str>,
    key_env: Option<&str>,
) -> Result<String> {
    let url = url.trim();
    check_webhook(url)?;
    let mut table = load();
    table.insert(
        seat.to_string(),
        Wake {
            webhook: Some(url.to_string()),
            key_file: key_file.map(str::to_string),
            key_env: key_env.map(str::to_string),
            ..Wake::default()
        },
    );
    save(&table)?;
    Ok(format!(
        "wake {seat}: posts to {url} when mail reaches it\n"
    ))
}

/// Remove a seat's wake.
///
/// # Errors
///
/// `wake.toml` cannot be written.
pub fn clear(seat: &str) -> Result<String> {
    let mut table = load();
    if table.remove(seat).is_none() {
        return Ok(format!("wake {seat}: none set\n"));
    }
    save(&table)?;
    Ok(format!("wake {seat}: removed\n"))
}

/// What a seat's wake is, in one line.
#[must_use]
pub fn show(seat: &str) -> String {
    match load().get(seat) {
        Some(Wake {
            command: Some(c), ..
        }) => format!("wake {seat}: runs `{c}`\n"),
        Some(Wake {
            webhook: Some(u), ..
        }) => format!("wake {seat}: posts to {u}\n"),
        _ => format!(
            "wake {seat}: none set, so mail waits for its next prompt. `ljos wake --command CMD` sets one\n"
        ),
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

/// Start the wake of each recipient that has one, each in its own session,
/// and name the seats woken. Nothing here fails the send: a wake that
/// cannot start is a line in the wake log.
#[must_use]
pub fn spawn(events: &[Event]) -> Vec<String> {
    let table = load();
    let mut woken = Vec::new();
    for event in events.iter().filter(|e| table.contains_key(&e.seat)) {
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
            Ok(()) => woken.push(event.seat.clone()),
            Err(e) => log(&format!("{}: the wake did not start: {e:#}", event.seat)),
        }
    }
    woken
}

/// The envelope a webhook gets. The body stays in the pack.
#[must_use]
pub fn envelope(event: &Event) -> serde_json::Value {
    serde_json::json!({
        "event": "ljos-mail",
        "seat": event.seat,
        "from": event.from,
        "msg": event.msg,
        "issue": event.issue,
        "interrupt": event.interrupt,
        "read": format!("LJOS_SEAT={} ljos inbox", event.seat),
    })
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

fn post(url: &str, wake: &Wake, event: &Event) -> Result<String> {
    check_webhook(url)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(WEBHOOK_TIMEOUT_SECS))
        .build();
    let mut req = agent.post(url).set("Content-Type", "application/json");
    if let Some(key) = webhook_key(wake) {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    let started = Instant::now();
    let resp = req.send_string(&envelope(event).to_string()).map_err(|e| {
        anyhow::anyhow!("POST {url}: {}", e.to_string().replace(url, "the webhook"))
    })?;
    Ok(format!(
        "posted, {} in {} ms",
        resp.status(),
        started.elapsed().as_millis()
    ))
}

/// Run one seat's wake now and say what happened. The outcome is also a
/// line in the wake log, since a detached wake has nobody to tell.
///
/// In push mode the sealed log is pushed first, from this process, which
/// `send` and `reply` have already detached. The other machine can then
/// read the letter. A push that does not land is recorded here and does
/// not stop the wake.
///
/// # Errors
///
/// The seat has no wake, the guard refuses its command, or the command or
/// the post fails.
pub fn fire(event: &Event) -> Result<String> {
    let table = load();
    let Some(wake) = table.get(&event.seat) else {
        bail!("wake: {} has no wake set", event.seat);
    };
    // The letter is already in the pack. Pushing here, rather than in
    // `send`, keeps the sender's wait to the detach. The other host's
    // inbox stays empty until this push is on the remote.
    if push_mode() {
        if let Err(why) = super::sync::push_sealed_log() {
            log(&format!("{}: {why:#}", event.seat));
        }
    }
    let outcome = if let Some(command) = &wake.command {
        let timeout = Duration::from_secs(wake.timeout.unwrap_or(WAKE_TIMEOUT_SECS).max(1));
        run_command(command, event, timeout)
    } else if let Some(url) = &wake.webhook {
        post(url, wake, event)
    } else {
        Err(anyhow::anyhow!("neither a command nor a webhook"))
    };
    match outcome {
        Ok(said) => {
            log(&format!("{} {}: {said}", event.seat, event.msg));
            Ok(format!("wake {}: {said}\n", event.seat))
        }
        Err(e) => {
            log(&format!("{} {}: {e:#}", event.seat, event.msg));
            Err(e).context(format!("wake {}", event.seat))
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
        };
        let v = envelope(&event);
        assert_eq!(v["seat"], "grok");
        assert_eq!(v["msg"], "m1");
        assert_eq!(v["interrupt"], true);
        assert!(v.get("body").is_none() && v.get("text").is_none(), "{v}");
        assert!(check_webhook("http://example.com/hook").is_err());
        assert!(check_webhook("https://api2.cursor.sh/automations/webhook/x").is_ok());
        assert!(check_webhook("http://127.0.0.1:9/hook").is_ok());
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
}
